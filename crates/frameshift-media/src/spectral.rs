//! Pure-Rust multi-band spectral flux onset detection, Ellis autocorrelation/DP
//! tempo & beat tracking, and 4/4 downbeat (bar-phase) estimation.
//!
//! Zero external FFT crates are used: [`Radix2Fft`] implements an in-place
//! iterative Radix-2 Cooley-Tukey FFT over `f64` butterflies for full
//! cross-platform determinism, outputting `Flicks(i64)` (`705_600_000 / s`)
//! onset, beat-grid, and downbeat timestamps.

use crate::audio::{AudioBuffer, make_hann_window};
use crate::error::MediaError;
use frameshift_time::{
    FLICKS_PER_SECOND, Flicks, audio_samples_to_ticks, ticks_to_audio_samples_nearest,
};
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

const BAR_SPECTRUM_BANDS: usize = 12;

/// Pure-Rust deterministic iterative Radix-2 Cooley-Tukey FFT plan for `N = 2^k`.
#[derive(Debug, Clone)]
pub struct Radix2Fft {
    len: usize,
    bit_rev: Vec<usize>,
    twiddles_re: Vec<f64>,
    twiddles_im: Vec<f64>,
}

impl Radix2Fft {
    /// Builds an FFT plan for a power-of-two length `len >= 2`.
    pub fn new(len: usize) -> Result<Self, MediaError> {
        if !len.is_power_of_two() || len < 2 {
            return Err(MediaError::InvalidAudioFormat(format!(
                "FFT size must be a power of two >= 2, got {len}"
            )));
        }
        let bits = len.trailing_zeros();
        let bit_rev = (0..len)
            .map(|i| i.reverse_bits() >> (usize::BITS - bits))
            .collect();
        let half = len / 2;
        let mut twiddles_re = Vec::with_capacity(half);
        let mut twiddles_im = Vec::with_capacity(half);
        let angle_step = -2.0 * PI / (len as f64);
        for k in 0..half {
            let angle = angle_step * (k as f64);
            twiddles_re.push(angle.cos());
            twiddles_im.push(angle.sin());
        }
        Ok(Self {
            len,
            bit_rev,
            twiddles_re,
            twiddles_im,
        })
    }

    /// Transform length `N`.
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if `len == 0` (always `false` for valid plans).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Computes real-input DFT magnitudes `|X[k]|` for `k in 0..out_bins.len().min(N / 2 + 1)`.
    pub fn forward_real_magnitudes(&self, windowed_frame: &[f32], out_bins: &mut [f32]) {
        let mut re = vec![0.0f64; self.len];
        let mut im = vec![0.0f64; self.len];
        self.forward_real_magnitudes_with_scratch(windowed_frame, out_bins, &mut re, &mut im);
    }

    fn forward_real_magnitudes_with_scratch(
        &self,
        windowed_frame: &[f32],
        out_bins: &mut [f32],
        re: &mut [f64],
        im: &mut [f64],
    ) {
        let n = self.len;
        let copy_len = windowed_frame.len().min(n);
        for (dst, &src_idx) in re.iter_mut().zip(self.bit_rev.iter()) {
            *dst = if src_idx < copy_len {
                f64::from(windowed_frame[src_idx])
            } else {
                0.0
            };
        }
        im[..n].fill(0.0);

        let mut half_size = 1usize;
        while half_size < n {
            let size = half_size << 1;
            let step = n / size;
            for start in (0..n).step_by(size) {
                for j in 0..half_size {
                    let tw_idx = j * step;
                    let wr = self.twiddles_re[tw_idx];
                    let wi = self.twiddles_im[tw_idx];
                    let even = start + j;
                    let odd = even + half_size;

                    let ore = re[odd];
                    let oim = im[odd];
                    let tre = ore * wr - oim * wi;
                    let tim = ore * wi + oim * wr;

                    let ere = re[even];
                    let eim = im[even];
                    re[even] = ere + tre;
                    im[even] = eim + tim;
                    re[odd] = ere - tre;
                    im[odd] = eim - tim;
                }
            }
            half_size = size;
        }

        let bin_count = out_bins.len().min(n / 2 + 1);
        for (k, out) in out_bins.iter_mut().take(bin_count).enumerate() {
            *out = re[k].hypot(im[k]) as f32;
        }
    }
}

/// Configuration for multi-band spectral flux, adaptive onset picking, tempo, and 4/4 downbeats.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MultibandFluxConfig {
    pub fft_size: usize,
    pub window_ticks: Flicks,
    pub sub_hz: (f32, f32),
    pub mid_hz: (f32, f32),
    pub high_hz: (f32, f32),
    pub log_gamma: f32,
    pub max_filter_bins: usize,
    pub median_mult: f32,
    pub mean_mult: f32,
    pub floor_mult: f32,
    pub refractory_ticks: Flicks,
    pub beats_per_bar: u8,
    pub band_weights: [f32; 3],
}

impl Default for MultibandFluxConfig {
    fn default() -> Self {
        Self {
            fft_size: 4096,
            window_ticks: Flicks(FLICKS_PER_SECOND / 100), // 10 ms = 7_056_000 Flicks
            sub_hz: (20.0, 120.0),
            mid_hz: (120.0, 2000.0),
            high_hz: (2000.0, 16000.0),
            log_gamma: 100.0,
            max_filter_bins: 1,
            median_mult: 1.35,
            mean_mult: 0.25,
            floor_mult: 0.05,
            refractory_ticks: Flicks(84_672_000), // 120 ms
            beats_per_bar: 4,
            band_weights: [0.45, 0.35, 0.20],
        }
    }
}

/// Per-band onset timestamps and normalized `[0.0, 1.0]` spectral envelopes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BandOnsetSummary {
    pub onsets: Vec<Flicks>,
    pub flux_envelope: Vec<f32>,
    pub energy_envelope: Vec<f32>,
}

/// Full multi-band spectral flux, tempo, beat-grid, and 4/4 downbeat analysis report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MultibandBeatReport {
    pub window_ticks: Flicks,
    pub fft_size: usize,
    pub estimated_bpm: f64,
    pub tempo_confidence: f64,
    pub beats_per_bar: u8,
    pub downbeat_phase: u8,
    pub bar_phase_confidence: f64,
    pub beat_grid: Vec<Flicks>,
    pub downbeats: Vec<Flicks>,
    pub sub: BandOnsetSummary,
    pub mid: BandOnsetSummary,
    pub high: BandOnsetSummary,
}

/// Multi-band audio envelope and beat/downbeat anchor state for expression evaluation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioBandEnvelope {
    pub window_ticks: Flicks,
    pub rms: Vec<f32>,
    pub sub: Vec<f32>,
    pub mid: Vec<f32>,
    pub high: Vec<f32>,
    pub estimated_bpm: f64,
    pub beat_anchor_tick: Flicks,
    pub downbeat_anchor_tick: Flicks,
    pub beats_per_bar: u8,
}

impl AudioBandEnvelope {
    /// Builds an [`AudioBandEnvelope`] from a full-band RMS curve and [`MultibandBeatReport`].
    pub fn from_rms_and_report(rms: Vec<f32>, report: &MultibandBeatReport) -> Self {
        let beat_anchor_tick = report
            .beat_grid
            .first()
            .copied()
            .unwrap_or(Flicks::ZERO);
        let downbeat_anchor_tick = report
            .downbeats
            .first()
            .copied()
            .unwrap_or(beat_anchor_tick);
        Self {
            window_ticks: report.window_ticks,
            rms,
            sub: report.sub.energy_envelope.clone(),
            mid: report.mid.energy_envelope.clone(),
            high: report.high.energy_envelope.clone(),
            estimated_bpm: report.estimated_bpm,
            beat_anchor_tick,
            downbeat_anchor_tick,
            beats_per_bar: report.beats_per_bar.max(1),
        }
    }

    /// Samples full-band RMS at `tick` with linear interpolation clamped to `[0.0, 1.0]`.
    pub fn sample_rms_at(&self, tick: Flicks) -> f64 {
        sample_envelope_at(&self.rms, self.window_ticks, tick)
    }

    /// Samples `(rms, sub, mid, high)` at `tick`, each linearly interpolated in `[0.0, 1.0]`.
    pub fn sample_bands_at(&self, tick: Flicks) -> (f64, f64, f64, f64) {
        (
            sample_envelope_at(&self.rms, self.window_ticks, tick),
            sample_envelope_at(&self.sub, self.window_ticks, tick),
            sample_envelope_at(&self.mid, self.window_ticks, tick),
            sample_envelope_at(&self.high, self.window_ticks, tick),
        )
    }

    /// Computes `(beat_phase, downbeat_phase)` in `[0.0, 1.0)` using `estimated_bpm` when valid,
    /// falling back to `120 BPM` aligned to `fallback_beat_anchor` if tempo is unavailable.
    pub fn phases_at(&self, tick: Flicks, fallback_beat_anchor: Flicks) -> (f64, f64) {
        let (bpm, beat_anchor, downbeat_anchor) =
            if self.estimated_bpm.is_finite() && self.estimated_bpm >= 20.0 {
                (
                    self.estimated_bpm,
                    self.beat_anchor_tick,
                    self.downbeat_anchor_tick,
                )
            } else {
                (120.0, fallback_beat_anchor, fallback_beat_anchor)
            };
        let beat_period_ticks = (60.0 / bpm) * (FLICKS_PER_SECOND as f64);
        let bar_period_ticks = beat_period_ticks * f64::from(self.beats_per_bar.max(1));
        let beat_phase = ((tick.0 - beat_anchor.0) as f64 / beat_period_ticks).rem_euclid(1.0);
        let downbeat_phase =
            ((tick.0 - downbeat_anchor.0) as f64 / bar_period_ticks).rem_euclid(1.0);
        (beat_phase, downbeat_phase)
    }
}

fn sample_envelope_at(samples: &[f32], window_ticks: Flicks, tick: Flicks) -> f64 {
    if samples.is_empty() || window_ticks.0 <= 0 || tick.0 < 0 {
        return 0.0;
    }
    let pos = tick.0 as f64 / window_ticks.0 as f64;
    let idx = pos.floor() as usize;
    if idx >= samples.len() {
        return 0.0;
    }
    let frac = pos - (idx as f64);
    let s0 = f64::from(samples[idx]);
    let s1 = samples.get(idx + 1).copied().map(f64::from).unwrap_or(s0);
    let interp = s0 + (s1 - s0) * frac;
    if interp.is_finite() {
        interp.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

impl AudioBuffer {
    /// Analyzes this audio buffer into 3-band spectral flux, adaptive per-band onsets,
    /// Ellis autocorrelation/DP beat grid, and 4/4 downbeat timestamps in `Flicks(i64)`.
    pub fn analyze_multiband_beats(
        &self,
        config: MultibandFluxConfig,
    ) -> Result<MultibandBeatReport, MediaError> {
        validate_config(&config)?;
        let total_frames = self.frame_count();
        if total_frames == 0 {
            return Err(MediaError::EmptyMediaStream(
                "cannot analyze multiband beats on empty audio buffer".into(),
            ));
        }

        let mono = self.to_mono_mixdown();
        let center_samples = compute_frame_centers(total_frames, config.window_ticks, self.sample_rate)?;
        let frame_count = center_samples.len();

        let max_abs = mono.iter().fold(0.0f32, |acc, &s| acc.max(s.abs()));
        if max_abs < 1e-6 || frame_count == 0 {
            return Ok(empty_report(&config, frame_count));
        }

        let n = config.fft_size;
        let fft = Radix2Fft::new(n)?;
        let hann = make_hann_window(n);
        let hann_sum: f32 = hann.iter().sum::<f32>().max(1.0);
        let norm_scale = 2.0 / hann_sum;

        let sub_bins = hz_range_to_bins(config.sub_hz, self.sample_rate, n);
        let mid_bins = hz_range_to_bins(config.mid_hz, self.sample_rate, n);
        let high_bins = hz_range_to_bins(config.high_hz, self.sample_rate, n);
        let bar_bins = build_log_bar_bins(self.sample_rate, n);

        let lookback = (3 * n) / 4;
        let bin_len = n / 2 + 1;
        let mut windowed = vec![0.0f32; n];
        let mut mags = vec![0.0f32; bin_len];
        let mut log_cur = vec![0.0f32; bin_len];
        let mut log_prev = vec![0.0f32; bin_len];
        let mut scratch_re = vec![0.0f64; n];
        let mut scratch_im = vec![0.0f64; n];

        let mut raw_sub_sf = Vec::with_capacity(frame_count);
        let mut raw_mid_sf = Vec::with_capacity(frame_count);
        let mut raw_high_sf = Vec::with_capacity(frame_count);
        let mut raw_sub_e = Vec::with_capacity(frame_count);
        let mut raw_mid_e = Vec::with_capacity(frame_count);
        let mut raw_high_e = Vec::with_capacity(frame_count);
        let mut bar_spectra = Vec::with_capacity(frame_count);

        for &center in &center_samples {
            let win_start = (center as isize) - (lookback as isize);
            for (j, (&w, slot)) in hann.iter().zip(windowed.iter_mut()).enumerate() {
                let idx = win_start + (j as isize);
                *slot = if idx >= 0 && (idx as usize) < total_frames {
                    mono[idx as usize] * w
                } else {
                    0.0
                };
            }

            fft.forward_real_magnitudes_with_scratch(
                &windowed,
                &mut mags,
                &mut scratch_re,
                &mut scratch_im,
            );

            for (dst, &m) in log_cur.iter_mut().zip(mags.iter()) {
                *dst = (1.0 + config.log_gamma * (m * norm_scale)).ln();
            }

            raw_sub_sf.push(band_flux(&log_cur, &log_prev, sub_bins, config.max_filter_bins));
            raw_mid_sf.push(band_flux(&log_cur, &log_prev, mid_bins, config.max_filter_bins));
            raw_high_sf.push(band_flux(&log_cur, &log_prev, high_bins, config.max_filter_bins));

            raw_sub_e.push(band_rms_energy(&mags, norm_scale, sub_bins));
            raw_mid_e.push(band_rms_energy(&mags, norm_scale, mid_bins));
            raw_high_e.push(band_rms_energy(&mags, norm_scale, high_bins));

            bar_spectra.push(compute_bar_spectrum(&log_cur, &bar_bins));
            log_prev.copy_from_slice(&log_cur);
        }

        let global_e_ref = raw_sub_e
            .iter()
            .chain(raw_mid_e.iter())
            .chain(raw_high_e.iter())
            .fold(0.0f32, |acc, &v| acc.max(v));
        let sub_energy = normalize_energy(&raw_sub_e, global_e_ref);
        let mid_energy = normalize_energy(&raw_mid_e, global_e_ref);
        let high_energy = normalize_energy(&raw_high_e, global_e_ref);

        let global_sf_ref = raw_sub_sf
            .iter()
            .chain(raw_mid_sf.iter())
            .chain(raw_high_sf.iter())
            .fold(0.0f32, |acc, &v| acc.max(v));
        let sub_flux = normalize_band_flux(&raw_sub_sf, &raw_sub_e, global_sf_ref, global_e_ref);
        let mid_flux = normalize_band_flux(&raw_mid_sf, &raw_mid_e, global_sf_ref, global_e_ref);
        let high_flux = normalize_band_flux(&raw_high_sf, &raw_high_e, global_sf_ref, global_e_ref);

        let sub_onsets = pick_band_onsets(
            &sub_flux,
            &mono,
            &center_samples,
            self.sample_rate,
            &config,
        )?;
        let mid_onsets = pick_band_onsets(
            &mid_flux,
            &mono,
            &center_samples,
            self.sample_rate,
            &config,
        )?;
        let high_onsets = pick_band_onsets(
            &high_flux,
            &mono,
            &center_samples,
            self.sample_rate,
            &config,
        )?;

        let mut odf_mix = Vec::with_capacity(frame_count);
        let [w_sub, w_mid, w_high] = config.band_weights;
        for i in 0..frame_count {
            odf_mix.push(w_sub * sub_flux[i] + w_mid * mid_flux[i] + w_high * high_flux[i]);
        }
        let max_odf = odf_mix.iter().fold(0.0f32, |acc, &v| acc.max(v));
        if max_odf > 1e-6 {
            let inv = 1.0 / max_odf;
            for v in &mut odf_mix {
                *v *= inv;
            }
        }

        let (estimated_bpm, tempo_confidence, beat_frames) = estimate_tempo_and_beat_frames(
            &odf_mix,
            &mono,
            &center_samples,
            self.sample_rate,
            &config,
        )?;

        let mut beat_grid = Vec::with_capacity(beat_frames.len());
        for &bf in &beat_frames {
            beat_grid.push(refine_onset_tick(
                bf,
                &odf_mix,
                &mono,
                &center_samples,
                self.sample_rate,
            )?);
        }

        let (downbeat_phase, bar_phase_confidence, downbeats) = estimate_downbeats(
            &beat_frames,
            &beat_grid,
            &sub_flux,
            &sub_energy,
            &mid_flux,
            &high_flux,
            &bar_spectra,
            config.beats_per_bar,
        );

        Ok(MultibandBeatReport {
            window_ticks: config.window_ticks,
            fft_size: config.fft_size,
            estimated_bpm,
            tempo_confidence,
            beats_per_bar: config.beats_per_bar,
            downbeat_phase,
            bar_phase_confidence,
            beat_grid,
            downbeats,
            sub: BandOnsetSummary {
                onsets: sub_onsets,
                flux_envelope: sub_flux,
                energy_envelope: sub_energy,
            },
            mid: BandOnsetSummary {
                onsets: mid_onsets,
                flux_envelope: mid_flux,
                energy_envelope: mid_energy,
            },
            high: BandOnsetSummary {
                onsets: high_onsets,
                flux_envelope: high_flux,
                energy_envelope: high_energy,
            },
        })
    }

    /// Computes both full-band RMS and multi-band spectral beat/downbeat envelopes in one call.
    pub fn analyze_band_envelope(
        &self,
        config: MultibandFluxConfig,
    ) -> Result<(AudioBandEnvelope, MultibandBeatReport), MediaError> {
        let report = self.analyze_multiband_beats(config)?;
        let rms = self.compute_rms_envelope(config.window_ticks)?;
        let envelope = AudioBandEnvelope::from_rms_and_report(rms, &report);
        Ok((envelope, report))
    }
}

fn validate_config(config: &MultibandFluxConfig) -> Result<(), MediaError> {
    if !config.fft_size.is_power_of_two() || config.fft_size < 2 {
        return Err(MediaError::InvalidAudioFormat(format!(
            "fft_size must be a power of two >= 2, got {}",
            config.fft_size
        )));
    }
    if config.window_ticks.0 <= 0 {
        return Err(MediaError::InvalidAudioFormat(
            "analysis window_ticks must be positive".into(),
        ));
    }
    if config.refractory_ticks.0 < 0 {
        return Err(MediaError::InvalidAudioFormat(
            "refractory_ticks must be non-negative".into(),
        ));
    }
    if config.beats_per_bar == 0 {
        return Err(MediaError::InvalidAudioFormat(
            "beats_per_bar must be >= 1".into(),
        ));
    }
    if !config.log_gamma.is_finite() || config.log_gamma <= 0.0 {
        return Err(MediaError::InvalidAudioFormat(
            "log_gamma must be finite and positive".into(),
        ));
    }
    for (name, (lo, hi)) in [
        ("sub_hz", config.sub_hz),
        ("mid_hz", config.mid_hz),
        ("high_hz", config.high_hz),
    ] {
        if !lo.is_finite() || !hi.is_finite() || lo < 0.0 || lo >= hi {
            return Err(MediaError::InvalidAudioFormat(format!(
                "invalid frequency band {name}: ({lo}, {hi})"
            )));
        }
    }
    Ok(())
}

fn empty_report(config: &MultibandFluxConfig, frame_count: usize) -> MultibandBeatReport {
    let empty_band = BandOnsetSummary {
        onsets: Vec::new(),
        flux_envelope: vec![0.0; frame_count],
        energy_envelope: vec![0.0; frame_count],
    };
    MultibandBeatReport {
        window_ticks: config.window_ticks,
        fft_size: config.fft_size,
        estimated_bpm: 0.0,
        tempo_confidence: 0.0,
        beats_per_bar: config.beats_per_bar,
        downbeat_phase: 0,
        bar_phase_confidence: 0.0,
        beat_grid: Vec::new(),
        downbeats: Vec::new(),
        sub: empty_band.clone(),
        mid: empty_band.clone(),
        high: empty_band,
    }
}

fn compute_frame_centers(
    total_frames: usize,
    window_ticks: Flicks,
    sample_rate: u32,
) -> Result<Vec<usize>, MediaError> {
    let mut centers = Vec::new();
    let mut w_idx: i64 = 0;
    loop {
        let tick = Flicks(w_idx.saturating_mul(window_ticks.0));
        let sample = ticks_to_audio_samples_nearest(tick, sample_rate)?.max(0) as usize;
        if sample >= total_frames {
            break;
        }
        centers.push(sample);
        w_idx += 1;
    }
    Ok(centers)
}

fn hz_range_to_bins(range: (f32, f32), sample_rate: u32, fft_size: usize) -> (usize, usize) {
    let df = (sample_rate as f32) / (fft_size as f32);
    let max_bin = fft_size / 2;
    let lo = ((range.0 / df).ceil() as usize).clamp(1, max_bin);
    let hi = ((range.1 / df).floor() as usize).clamp(lo, max_bin);
    (lo, hi)
}

fn build_log_bar_bins(sample_rate: u32, fft_size: usize) -> [(usize, usize); BAR_SPECTRUM_BANDS] {
    let min_hz = 40.0f64;
    let max_hz = (f64::from(sample_rate) * 0.45).min(12_000.0).max(min_hz * 2.0);
    let ratio = (max_hz / min_hz).powf(1.0 / (BAR_SPECTRUM_BANDS as f64));
    let mut out = [(1usize, 1usize); BAR_SPECTRUM_BANDS];
    for (idx, slot) in out.iter_mut().enumerate() {
        let f0 = (min_hz * ratio.powi(idx as i32)) as f32;
        let f1 = (min_hz * ratio.powi((idx + 1) as i32)) as f32;
        *slot = hz_range_to_bins((f0, f1), sample_rate, fft_size);
    }
    out
}

fn band_flux(
    log_cur: &[f32],
    log_prev: &[f32],
    bins: (usize, usize),
    max_filter_bins: usize,
) -> f32 {
    let (k_lo, k_hi) = bins;
    if k_lo > k_hi || k_hi >= log_cur.len() {
        return 0.0;
    }
    let mut sum = 0.0f32;
    for (k, &cur_val) in log_cur.iter().enumerate().take(k_hi + 1).skip(k_lo) {
        let ref_lo = k.saturating_sub(max_filter_bins).max(k_lo);
        let ref_hi = (k + max_filter_bins).min(k_hi);
        let mut ref_val = 0.0f32;
        for &prev in &log_prev[ref_lo..=ref_hi] {
            if prev > ref_val {
                ref_val = prev;
            }
        }
        let diff = cur_val - ref_val;
        if diff > 0.0 {
            sum += diff;
        }
    }
    sum / ((k_hi - k_lo + 1) as f32)
}

fn band_rms_energy(mags: &[f32], norm_scale: f32, bins: (usize, usize)) -> f32 {
    let (k_lo, k_hi) = bins;
    if k_lo > k_hi || k_hi >= mags.len() {
        return 0.0;
    }
    let mut sum_sq = 0.0f64;
    for &m in &mags[k_lo..=k_hi] {
        let v = f64::from(m * norm_scale);
        sum_sq += v * v;
    }
    sum_sq.sqrt() as f32
}

fn compute_bar_spectrum(
    log_cur: &[f32],
    bar_bins: &[(usize, usize); BAR_SPECTRUM_BANDS],
) -> [f32; BAR_SPECTRUM_BANDS] {
    let mut vec = [0.0f32; BAR_SPECTRUM_BANDS];
    for (slot, &(lo, hi)) in vec.iter_mut().zip(bar_bins.iter()) {
        if lo <= hi && hi < log_cur.len() {
            let sum: f32 = log_cur[lo..=hi].iter().copied().sum();
            *slot = sum / ((hi - lo + 1) as f32);
        }
    }
    vec
}

fn normalize_band_flux(
    raw_sf: &[f32],
    raw_e: &[f32],
    global_sf_ref: f32,
    global_e_ref: f32,
) -> Vec<f32> {
    let band_e_max = raw_e.iter().fold(0.0f32, |acc, &v| acc.max(v));
    if band_e_max < global_e_ref * 0.08 {
        return vec![0.0; raw_sf.len()];
    }
    let band_max = raw_sf.iter().fold(0.0f32, |acc, &v| acc.max(v));
    let denom = band_max.max(global_sf_ref * 0.08).max(1e-4);
    let inv = 1.0 / denom;
    raw_sf.iter().map(|&v| (v * inv).clamp(0.0, 1.0)).collect()
}

fn normalize_energy(raw: &[f32], global_ref: f32) -> Vec<f32> {
    let band_max = raw.iter().fold(0.0f32, |acc, &v| acc.max(v));
    let denom = band_max.max(global_ref * 0.04).max(1e-4);
    let inv = 1.0 / denom;
    raw.iter().map(|&v| (v * inv).clamp(0.0, 1.0)).collect()
}

fn pick_band_onsets(
    flux: &[f32],
    mono: &[f32],
    center_samples: &[usize],
    sample_rate: u32,
    config: &MultibandFluxConfig,
) -> Result<Vec<Flicks>, MediaError> {
    let frames = pick_adaptive_peaks(flux, config);
    let mut onsets: Vec<Flicks> = Vec::with_capacity(frames.len());
    for frame_idx in frames {
        let tick = refine_onset_tick(frame_idx, flux, mono, center_samples, sample_rate)?;
        if onsets
            .last()
            .is_none_or(|&prev| tick.0 - prev.0 >= config.refractory_ticks.0)
        {
            onsets.push(tick);
        }
    }
    Ok(onsets)
}

fn pick_adaptive_peaks(odf: &[f32], config: &MultibandFluxConfig) -> Vec<usize> {
    let n = odf.len();
    if n == 0 {
        return Vec::new();
    }
    let max_odf = odf.iter().fold(0.0f32, |acc, &v| acc.max(v));
    if max_odf < 0.08 {
        return Vec::new();
    }

    let half_win = ((FLICKS_PER_SECOND / 4) / config.window_ticks.0).max(4) as usize;
    let delta_floor = (config.floor_mult * max_odf).max(0.06);
    let mut window_buf = Vec::with_capacity(half_win * 2 + 1);
    let mut picked = Vec::new();

    for i in 0..n {
        let cur = odf[i];
        let prev = if i == 0 { 0.0 } else { odf[i - 1] };
        if cur <= prev {
            continue;
        }
        let lo_local = i.saturating_sub(2);
        let hi_local = (i + 2).min(n - 1);
        let is_local_max = odf[lo_local..=hi_local].iter().all(|&v| cur >= v);
        if !is_local_max {
            continue;
        }

        let w_lo = i.saturating_sub(half_win);
        let w_hi = (i + half_win).min(n - 1);
        window_buf.clear();
        window_buf.extend_from_slice(&odf[w_lo..=w_hi]);
        let sum: f32 = window_buf.iter().copied().sum();
        let mean = sum / (window_buf.len() as f32);
        window_buf.sort_by(|a, b| a.total_cmp(b));
        let median = window_buf[window_buf.len() / 2];
        let threshold = delta_floor + config.median_mult * median + config.mean_mult * mean;
        if cur > threshold {
            picked.push(i);
        }
    }

    picked
}

fn refine_onset_tick(
    frame_idx: usize,
    odf: &[f32],
    mono: &[f32],
    center_samples: &[usize],
    sample_rate: u32,
) -> Result<Flicks, MediaError> {
    if center_samples.is_empty() || mono.is_empty() {
        return Ok(Flicks::ZERO);
    }
    let idx = frame_idx.min(center_samples.len() - 1);
    let center = center_samples[idx];
    let hop = if center_samples.len() >= 2 {
        (center_samples[1] - center_samples[0]).max(1)
    } else {
        (sample_rate as usize / 100).max(1)
    };

    let lo = center.saturating_sub(hop + hop / 2);
    let hi = (center + hop / 2 + 1).min(mono.len());
    if lo < hi {
        let mut local_max = 0.0f32;
        let mut max_pos = center.min(hi - 1);
        for (offset, &s) in mono[lo..hi].iter().enumerate() {
            let a = s.abs();
            if a > local_max {
                local_max = a;
                max_pos = lo + offset;
            }
        }
        let pre_lo = lo.saturating_sub(hop);
        let pre_len = lo - pre_lo;
        let pre_rms = if pre_len > 0 {
            let sum_sq: f64 = mono[pre_lo..lo]
                .iter()
                .map(|&s| f64::from(s) * f64::from(s))
                .sum();
            (sum_sq / (pre_len as f64)).sqrt() as f32
        } else {
            0.0
        };
        if local_max > 0.02 && local_max > pre_rms * 2.5 {
            // Find the earliest crisp attack crossing 75% of the local transient peak.
            let attack_thresh = local_max * 0.75;
            let mut attack_pos = max_pos;
            for (offset, &s) in mono[lo..=max_pos].iter().enumerate() {
                if s.abs() >= attack_thresh {
                    attack_pos = lo + offset;
                    break;
                }
            }
            return Ok(audio_samples_to_ticks(attack_pos as i64, sample_rate)?);
        }
    }

    let frac_offset = if idx > 0 && idx + 1 < odf.len() {
        parabolic_offset(f64::from(odf[idx - 1]), f64::from(odf[idx]), f64::from(odf[idx + 1]))
    } else {
        0.0
    };
    let refined_sample = ((center as f64) + frac_offset * (hop as f64))
        .round()
        .clamp(0.0, (mono.len().saturating_sub(1)) as f64) as i64;
    Ok(audio_samples_to_ticks(refined_sample, sample_rate)?)
}

fn parabolic_offset(y_prev: f64, y_cur: f64, y_next: f64) -> f64 {
    let denom = y_prev - 2.0 * y_cur + y_next;
    if denom.abs() < 1e-9 || denom >= 0.0 {
        0.0
    } else {
        (0.5 * (y_prev - y_next) / denom).clamp(-0.5, 0.5)
    }
}

fn estimate_tempo_and_beat_frames(
    odf: &[f32],
    mono: &[f32],
    center_samples: &[usize],
    sample_rate: u32,
    config: &MultibandFluxConfig,
) -> Result<(f64, f64, Vec<usize>), MediaError> {
    let t_len = odf.len();
    let fallback_peaks = pick_adaptive_peaks(odf, config);
    if t_len < 8 {
        let mut fallback_ticks = Vec::with_capacity(fallback_peaks.len());
        for &p in &fallback_peaks {
            fallback_ticks.push(refine_onset_tick(p, odf, mono, center_samples, sample_rate)?);
        }
        return Ok((
            estimate_bpm_from_ticks(&fallback_ticks),
            0.0,
            fallback_peaks,
        ));
    }

    let hop_sec = (config.window_ticks.0 as f64) / (FLICKS_PER_SECOND as f64);
    let lag_min = (60.0 / (220.0 * hop_sec)).round().max(2.0) as usize;
    let lag_max_raw = (60.0 / (40.0 * hop_sec)).round().max((lag_min + 1) as f64) as usize;
    let lag_max = lag_max_raw.min(t_len / 2);

    if lag_max <= lag_min {
        let mut fallback_ticks = Vec::with_capacity(fallback_peaks.len());
        for &p in &fallback_peaks {
            fallback_ticks.push(refine_onset_tick(p, odf, mono, center_samples, sample_rate)?);
        }
        return Ok((
            estimate_bpm_from_ticks(&fallback_ticks),
            0.0,
            fallback_peaks,
        ));
    }

    let mean = odf.iter().map(|&v| f64::from(v)).sum::<f64>() / (t_len as f64);
    let centered: Vec<f64> = odf.iter().map(|&v| f64::from(v) - mean).collect();
    let energy0: f64 = centered.iter().map(|&v| v * v).sum();
    if energy0 < 1e-9 {
        return Ok((0.0, 0.0, Vec::new()));
    }

    let max_acf_lag = (lag_max * 2).min(t_len - 1);
    let mut acf = vec![0.0f64; max_acf_lag + 1];
    for lag in 1..=max_acf_lag {
        let mut sum = 0.0f64;
        for t in 0..(t_len - lag) {
            sum += centered[t] * centered[t + lag];
        }
        acf[lag] = sum / energy0;
    }

    let mut scores = vec![0.0f64; lag_max + 1];
    let mut comb_vals = vec![0.0f64; lag_max + 1];
    let mut best_lag = lag_min;
    let mut best_score = f64::NEG_INFINITY;
    for lag in lag_min..=lag_max {
        let r1 = acf[lag];
        let r2 = if lag * 2 < acf.len() { acf[lag * 2] } else { 0.0 };
        let r_half = if lag >= 4 { acf[lag / 2] } else { 0.0 };
        let comb = r1 + 0.5 * r2 + 0.10 * r_half;
        let bpm = 60.0 / ((lag as f64) * hop_sec);
        let prior = (-0.5 * ((bpm / 120.0).log2() / 0.9).powi(2)).exp();
        let score = comb.max(0.0) * prior;
        comb_vals[lag] = comb;
        scores[lag] = score;
        if score > best_score {
            best_score = score;
            best_lag = lag;
        }
    }

    let interp_offset = if best_lag > lag_min && best_lag < lag_max {
        parabolic_offset(
            scores[best_lag - 1],
            scores[best_lag],
            scores[best_lag + 1],
        )
    } else {
        0.0
    };
    let period_frames = ((best_lag as f64) + interp_offset).max(1.0);
    let estimated_bpm = 60.0 / (period_frames * hop_sec);
    let tempo_confidence = (comb_vals[best_lag] / 1.75).clamp(0.0, 1.0);

    let duration_sec = (t_len as f64) * hop_sec;
    if duration_sec < 1.5 || comb_vals[best_lag] < 0.12 {
        let mut fallback_ticks = Vec::with_capacity(fallback_peaks.len());
        for &p in &fallback_peaks {
            fallback_ticks.push(refine_onset_tick(p, odf, mono, center_samples, sample_rate)?);
        }
        let bpm = if comb_vals[best_lag] >= 0.12 {
            estimated_bpm
        } else {
            let ibi_bpm = estimate_bpm_from_ticks(&fallback_ticks);
            if ibi_bpm > 0.0 { ibi_bpm } else { estimated_bpm }
        };
        return Ok((bpm, tempo_confidence, fallback_peaks));
    }

    let beat_frames = dp_align_beats(odf, period_frames);
    Ok((estimated_bpm, tempo_confidence, beat_frames))
}

fn dp_align_beats(odf: &[f32], period_frames: f64) -> Vec<usize> {
    let n = odf.len();
    if n == 0 {
        return Vec::new();
    }
    let eta = 7.0f64;
    let min_dt = (0.5 * period_frames).floor().max(1.0) as usize;
    let max_dt = (2.0 * period_frames).ceil().max((min_dt + 1) as f64) as usize;

    let mut dp = Vec::with_capacity(n);
    for &v in odf {
        dp.push(f64::from(v));
    }
    let mut prev: Vec<Option<usize>> = vec![None; n];

    for t in min_dt..n {
        let lo = t.saturating_sub(max_dt);
        let hi = t - min_dt;
        let odf_t = f64::from(odf[t]);
        let mut best_val = dp[t];
        let mut best_prev = None;
        for (tp, &dp_tp) in dp.iter().enumerate().take(hi + 1).skip(lo) {
            let dt = (t - tp) as f64;
            let penalty = eta * (dt / period_frames).ln().powi(2);
            let cand = odf_t + dp_tp - penalty;
            if cand > best_val {
                best_val = cand;
                best_prev = Some(tp);
            }
        }
        dp[t] = best_val;
        prev[t] = best_prev;
    }

    let tail_start = n.saturating_sub(max_dt);
    let mut end_idx = tail_start;
    let mut end_score = f64::NEG_INFINITY;
    for (idx, &val) in dp.iter().enumerate().skip(tail_start) {
        if val > end_score {
            end_score = val;
            end_idx = idx;
        }
    }

    let mut frames = Vec::new();
    let mut cur = Some(end_idx);
    while let Some(idx) = cur {
        frames.push(idx);
        cur = prev[idx];
    }
    frames.reverse();

    // Snap each DP beat to the strongest ODF peak within +-2 frames and drop silent endpoints.
    for frame in &mut frames {
        let lo = frame.saturating_sub(2);
        let hi = (*frame + 2).min(n - 1);
        let mut best_i = *frame;
        let mut best_v = odf[*frame];
        for (offset, &v) in odf[lo..=hi].iter().enumerate() {
            if v > best_v {
                best_v = v;
                best_i = lo + offset;
            }
        }
        *frame = best_i;
    }
    frames.dedup();
    frames
        .into_iter()
        .filter(|&idx| {
            let lo = idx.saturating_sub(2);
            let hi = (idx + 2).min(n - 1);
            odf[lo..=hi].iter().fold(0.0f32, |a, &b| a.max(b)) >= 0.05
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn estimate_downbeats(
    beat_frames: &[usize],
    beat_grid: &[Flicks],
    sub_flux: &[f32],
    sub_energy: &[f32],
    mid_flux: &[f32],
    high_flux: &[f32],
    bar_spectra: &[[f32; BAR_SPECTRUM_BANDS]],
    beats_per_bar: u8,
) -> (u8, f64, Vec<Flicks>) {
    let bpb = (beats_per_bar.max(1)) as usize;
    if beat_frames.is_empty() || beat_grid.is_empty() || bar_spectra.is_empty() {
        return (0, 0.0, Vec::new());
    }
    if beat_frames.len() < bpb || bpb == 1 {
        return (0, 0.0, beat_grid.iter().step_by(bpb).copied().collect());
    }

    let t_len = sub_flux.len();
    let mut s_sub = Vec::with_capacity(beat_frames.len());
    let mut e_sub = Vec::with_capacity(beat_frames.len());
    let mut backbeat = Vec::with_capacity(beat_frames.len());
    let mut beat_vecs = Vec::with_capacity(beat_frames.len());

    for &bf in beat_frames {
        let lo = bf.saturating_sub(2);
        let hi = (bf + 2).min(t_len - 1);
        let sf_sub = sub_flux[lo..=hi].iter().fold(0.0f32, |a, &b| a.max(b));
        let en_sub = sub_energy[lo..=hi].iter().fold(0.0f32, |a, &b| a.max(b));
        let bb = mid_flux[lo..=hi]
            .iter()
            .zip(high_flux[lo..=hi].iter())
            .fold(0.0f32, |acc, (&m, &h)| acc.max(m + h));

        let spec_hi = (bf + 2).min(bar_spectra.len() - 1);
        let spec_lo = bf.min(spec_hi);
        let mut avg = [0.0f32; BAR_SPECTRUM_BANDS];
        let count = (spec_hi - spec_lo + 1) as f32;
        for row in &bar_spectra[spec_lo..=spec_hi] {
            for (dst, &val) in avg.iter_mut().zip(row.iter()) {
                *dst += val;
            }
        }
        for val in &mut avg {
            *val /= count;
        }

        s_sub.push(f64::from(sf_sub));
        e_sub.push(f64::from(en_sub));
        backbeat.push(f64::from(bb));
        beat_vecs.push(avg);
    }

    let mut delta_bar_s = vec![0.0f64; beat_frames.len()];
    for k in 1..beat_frames.len() {
        delta_bar_s[k] = cosine_distance(&beat_vecs[k], &beat_vecs[k - 1]);
    }

    let mut phase_scores = vec![0.0f64; bpb];
    for (p, score_slot) in phase_scores.iter_mut().enumerate() {
        let mut down_sum = 0.0f64;
        let mut down_cnt = 0usize;
        let mut back_sum = 0.0f64;
        let mut back_cnt = 0usize;

        for k in 0..beat_frames.len() {
            let slot = k % bpb;
            if slot == p {
                down_sum += 0.50 * s_sub[k] + 0.30 * delta_bar_s[k] + 0.20 * e_sub[k];
                down_cnt += 1;
            } else if bpb >= 4 && (slot == (p + 1) % bpb || slot == (p + 3) % bpb) {
                back_sum += backbeat[k];
                back_cnt += 1;
            }
        }

        let mean_down = if down_cnt > 0 {
            down_sum / (down_cnt as f64)
        } else {
            0.0
        };
        let mean_back = if back_cnt > 0 {
            back_sum / (back_cnt as f64)
        } else {
            0.0
        };
        *score_slot = mean_down + 0.25 * mean_back;
    }

    let mut best_p = 0usize;
    let mut best_score = f64::NEG_INFINITY;
    let mut second_score = f64::NEG_INFINITY;
    for (p, &score) in phase_scores.iter().enumerate() {
        if score > best_score {
            second_score = best_score;
            best_score = score;
            best_p = p;
        } else if score > second_score {
            second_score = score;
        }
    }

    let confidence = if second_score.is_finite() {
        ((best_score - second_score) / (best_score.abs() + 1e-6)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let downbeats = beat_grid
        .iter()
        .enumerate()
        .filter(|(k, _)| k % bpb == best_p)
        .map(|(_, &t)| t)
        .collect();

    (best_p as u8, confidence, downbeats)
}

fn cosine_distance(a: &[f32; BAR_SPECTRUM_BANDS], b: &[f32; BAR_SPECTRUM_BANDS]) -> f64 {
    let mut dot = 0.0f64;
    let mut na = 0.0f64;
    let mut nb = 0.0f64;
    for (&va, &vb) in a.iter().zip(b.iter()) {
        let fa = f64::from(va);
        let fb = f64::from(vb);
        dot += fa * fb;
        na += fa * fa;
        nb += fb * fb;
    }
    let denom = (na * nb).sqrt();
    if denom < 1e-9 {
        0.0
    } else {
        (1.0 - (dot / denom).clamp(-1.0, 1.0)).clamp(0.0, 2.0)
    }
}

fn estimate_bpm_from_ticks(beats: &[Flicks]) -> f64 {
    if beats.len() < 2 {
        return 0.0;
    }
    let mut ibis: Vec<f64> = beats
        .windows(2)
        .filter_map(|w| {
            let dt = (w[1].0 - w[0].0) as f64 / (FLICKS_PER_SECOND as f64);
            if dt > 0.0 { Some(dt) } else { None }
        })
        .collect();
    if ibis.is_empty() {
        return 0.0;
    }
    ibis.sort_by(|a, b| a.total_cmp(b));
    let median = ibis[ibis.len() / 2];
    if median > 0.0 { 60.0 / median } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::DEFAULT_AUDIO_SAMPLE_RATE;
    use crate::synthetic::SyntheticMediaGenerator;
    use frameshift_time::FLICKS_PER_MILLISECOND;

    fn add_decay_tone(
        buf: &mut AudioBuffer,
        start_tick: Flicks,
        freq_hz: f32,
        amp: f32,
        decay_rate: f32,
        duration_sec: f32,
        noise_mix: f32,
    ) {
        let sr = buf.sample_rate;
        let start_frame = ticks_to_audio_samples_nearest(start_tick, sr).unwrap() as usize;
        let len = ((sr as f32) * duration_sec).round() as usize;
        let total = buf.frame_count();
        let ch = buf.channels as usize;
        let mut seed = 0x1234_5678u32 ^ (start_frame as u32);
        for offset in 0..len {
            let f = start_frame + offset;
            if f >= total {
                break;
            }
            let dt = (offset as f32) / (sr as f32);
            let attack = (dt / 0.008).min(1.0);
            let fade_out = if duration_sec > 0.02 && dt > duration_sec - 0.015 {
                ((duration_sec - dt) / 0.015).clamp(0.0, 1.0)
            } else {
                1.0
            };
            let env = attack * fade_out * (-dt * decay_rate).exp();
            let tone = (2.0 * std::f32::consts::PI * freq_hz * dt).sin();
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = ((seed >> 16) as f32 / 32768.0) - 1.0;
            let sample = amp * env * (tone * (1.0 - noise_mix) + noise * noise_mix);
            for c in 0..ch {
                let idx = f * ch + c;
                buf.samples[idx] = (buf.samples[idx] + sample).clamp(-1.0, 1.0);
            }
        }
    }

    fn build_synthetic_4_4_kit(bpm: u32, bars: usize, downbeat_shift_beats: usize) -> AudioBuffer {
        let sr = DEFAULT_AUDIO_SAMPLE_RATE;
        let total_beats = bars * 4;
        let beat_ticks_i128 = (60i128 * FLICKS_PER_SECOND as i128) / (bpm as i128);
        let duration = Flicks((beat_ticks_i128 * (total_beats as i128)) as i64);
        let mut buf = AudioBuffer::silence_for_duration(sr, 2, duration).unwrap();

        for b in 0..total_beats {
            let tick = Flicks((beat_ticks_i128 * (b as i128)) as i64);
            let bar_pos = (b + 4 - (downbeat_shift_beats % 4)) % 4;
            match bar_pos {
                0 => {
                    // Downbeat: strong 60 Hz kick + low-mid body
                    add_decay_tone(&mut buf, tick, 60.0, 0.85, 16.0, 0.14, 0.0);
                    add_decay_tone(&mut buf, tick, 110.0, 0.20, 22.0, 0.10, 0.0);
                }
                1 | 3 => {
                    // Backbeat: 400 Hz snare + crisp snap
                    add_decay_tone(&mut buf, tick, 400.0, 0.65, 28.0, 0.09, 0.25);
                }
                _ => {
                    // Beat 3: lighter kick
                    add_decay_tone(&mut buf, tick, 60.0, 0.45, 20.0, 0.10, 0.0);
                }
            }
            // Hi-hat on eighth-note offbeats (8 kHz)
            let offbeat = Flicks(tick.0 + (beat_ticks_i128 as i64) / 2);
            if offbeat < duration {
                add_decay_tone(&mut buf, offbeat, 8000.0, 0.40, 65.0, 0.04, 0.15);
            }
        }
        buf
    }

    #[test]
    fn radix2_fft_matches_naive_dft_and_parseval() {
        let n = 256usize;
        let fft = Radix2Fft::new(n).unwrap();
        let mut signal = vec![0.0f32; n];
        for (i, s) in signal.iter_mut().enumerate() {
            let t = i as f32 / n as f32;
            *s = 0.7 * (2.0 * std::f32::consts::PI * 5.0 * t).sin()
                + 0.4 * (2.0 * std::f32::consts::PI * 23.0 * t).cos()
                + 0.2 * (2.0 * std::f32::consts::PI * 64.0 * t).sin();
        }
        let mut mags = vec![0.0f32; n / 2 + 1];
        fft.forward_real_magnitudes(&signal, &mut mags);

        // Compare against naive DFT for all non-negative bins.
        for (k, &got) in mags.iter().enumerate() {
            let mut re = 0.0f64;
            let mut im = 0.0f64;
            for (idx, &x) in signal.iter().enumerate() {
                let angle = -2.0 * PI * (k as f64) * (idx as f64) / (n as f64);
                re += f64::from(x) * angle.cos();
                im += f64::from(x) * angle.sin();
            }
            let expected = re.hypot(im) as f32;
            assert!(
                (got - expected).abs() < 1e-3,
                "bin {k} mismatch: got {got}, expected {expected}"
            );
        }

        // Verify Parseval energy conservation.
        let time_energy: f64 = signal.iter().map(|&x| f64::from(x) * f64::from(x)).sum();
        let mut freq_energy = f64::from(mags[0]).powi(2) + f64::from(mags[n / 2]).powi(2);
        for &m in &mags[1..(n / 2)] {
            freq_energy += 2.0 * f64::from(m).powi(2);
        }
        freq_energy /= n as f64;
        assert!(
            (time_energy - freq_energy).abs() < 1e-3,
            "Parseval mismatch: time={time_energy}, freq={freq_energy}"
        );
    }

    #[test]
    fn radix2_fft_rejects_invalid_sizes() {
        for bad in [0usize, 1, 3, 6, 1000] {
            assert!(matches!(
                Radix2Fft::new(bad),
                Err(MediaError::InvalidAudioFormat(_))
            ));
        }
    }

    #[test]
    fn multiband_isolates_kick_snare_and_hihat_and_detects_4_4_downbeat() {
        let buf = build_synthetic_4_4_kit(120, 4, 0);
        let report = buf
            .analyze_multiband_beats(MultibandFluxConfig::default())
            .unwrap();

        assert!(
            (report.estimated_bpm - 120.0).abs() <= 1.5,
            "expected ~120 BPM, got {}",
            report.estimated_bpm
        );
        assert!(report.sub.onsets.len() >= 6, "sub onsets: {:?}", report.sub.onsets);
        assert!(report.mid.onsets.len() >= 6, "mid onsets: {:?}", report.mid.onsets);
        assert!(
            report.high.onsets.len() >= 12,
            "high onsets: {:?}",
            report.high.onsets
        );
        assert_eq!(report.downbeat_phase, 0);
        assert_eq!(report.downbeats.len(), 4);

        let expected_bars = [
            Flicks(0),
            Flicks(2 * FLICKS_PER_SECOND),
            Flicks(4 * FLICKS_PER_SECOND),
            Flicks(6 * FLICKS_PER_SECOND),
        ];
        for (got, expected) in report.downbeats.iter().zip(expected_bars.iter()) {
            let err_ms = (got.0 - expected.0).abs() / FLICKS_PER_MILLISECOND;
            assert!(
                err_ms <= 20,
                "downbeat {got} differed from {expected} by {err_ms} ms"
            );
        }
    }

    #[test]
    fn multiband_detects_shifted_downbeat_phase() {
        let buf = build_synthetic_4_4_kit(120, 4, 1);
        let report = buf
            .analyze_multiband_beats(MultibandFluxConfig::default())
            .unwrap();

        assert!(
            (report.estimated_bpm - 120.0).abs() <= 1.5,
            "expected ~120 BPM, got {}",
            report.estimated_bpm
        );
        assert!(!report.downbeats.is_empty());
        // Beat 1 is at 0.5 s = 352_800_000 Flicks.
        let expected_first = Flicks(FLICKS_PER_SECOND / 2);
        let first_err_ms = (report.downbeats[0].0 - expected_first.0).abs() / FLICKS_PER_MILLISECOND;
        assert!(
            first_err_ms <= 25,
            "expected shifted first downbeat near 0.5s, got {} (err {first_err_ms} ms)",
            report.downbeats[0]
        );
    }

    #[test]
    fn multiband_handles_synthetic_880hz_beat_audio_and_silence() {
        let duration = Flicks(4 * FLICKS_PER_SECOND);
        let (beat_buf, expected_beats) =
            SyntheticMediaGenerator::generate_beat_audio(duration, 120, DEFAULT_AUDIO_SAMPLE_RATE);
        let report = beat_buf
            .analyze_multiband_beats(MultibandFluxConfig::default())
            .unwrap();

        assert!(
            (report.estimated_bpm - 120.0).abs() <= 1.5,
            "expected ~120 BPM on 880 Hz beat audio, got {}",
            report.estimated_bpm
        );
        assert_eq!(report.mid.onsets.len(), expected_beats.len());
        assert_eq!(report.beat_grid.len(), expected_beats.len());

        let silence = AudioBuffer::silence(DEFAULT_AUDIO_SAMPLE_RATE, 2, 48_000).unwrap();
        let silent_report = silence
            .analyze_multiband_beats(MultibandFluxConfig::default())
            .unwrap();
        assert_eq!(silent_report.estimated_bpm, 0.0);
        assert!(silent_report.beat_grid.is_empty());
        assert!(silent_report.downbeats.is_empty());
        assert!(silent_report.sub.onsets.is_empty());
        assert!(silent_report.sub.energy_envelope.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn audio_band_envelope_samples_and_computes_phases_deterministically() {
        let env = AudioBandEnvelope {
            window_ticks: Flicks(100),
            rms: vec![0.2, 0.8, 1.5],
            sub: vec![0.9, 0.3],
            mid: vec![0.1, 0.5],
            high: vec![0.0, 0.7],
            estimated_bpm: 120.0,
            beat_anchor_tick: Flicks(0),
            downbeat_anchor_tick: Flicks(FLICKS_PER_SECOND / 2),
            beats_per_bar: 4,
        };
        let (rms, sub, mid, high) = env.sample_bands_at(Flicks(50));
        assert!((rms - 0.5).abs() < 1e-6);
        assert!((sub - 0.6).abs() < 1e-6);
        assert!((mid - 0.3).abs() < 1e-6);
        assert!((high - 0.35).abs() < 1e-6);
        assert_eq!(env.sample_rms_at(Flicks(250)), 1.0);
        assert_eq!(env.sample_rms_at(Flicks(-10)), 0.0);

        let (beat_p, down_p) = env.phases_at(Flicks(FLICKS_PER_SECOND / 2), Flicks::ZERO);
        assert!(beat_p.abs() < 1e-9);
        assert!(down_p.abs() < 1e-9);

        let (beat_p2, down_p2) = env.phases_at(Flicks(FLICKS_PER_SECOND), Flicks::ZERO);
        assert!(beat_p2.abs() < 1e-9);
        assert!((down_p2 - 0.25).abs() < 1e-9);
    }

    #[test]
    fn multiband_single_band_isolation_suppresses_silent_bands() {
        let sr = DEFAULT_AUDIO_SAMPLE_RATE;
        let duration = Flicks(4 * FLICKS_PER_SECOND);
        let mut kick_only = AudioBuffer::silence_for_duration(sr, 2, duration).unwrap();
        for b in 0..8 {
            add_decay_tone(
                &mut kick_only,
                Flicks(b * (FLICKS_PER_SECOND / 2)),
                60.0,
                0.85,
                16.0,
                0.12,
                0.0,
            );
        }
        let kick_report = kick_only
            .analyze_multiband_beats(MultibandFluxConfig::default())
            .unwrap();
        assert_eq!(kick_report.sub.onsets.len(), 8);
        assert!(
            kick_report.mid.onsets.is_empty(),
            "pure 60 Hz kick leaked into mid: {:?}",
            kick_report.mid.onsets
        );
        assert!(
            kick_report.high.onsets.is_empty(),
            "pure 60 Hz kick leaked into high: {:?}",
            kick_report.high.onsets
        );

        let mut hat_only = AudioBuffer::silence_for_duration(sr, 2, duration).unwrap();
        for b in 0..8 {
            add_decay_tone(
                &mut hat_only,
                Flicks(b * (FLICKS_PER_SECOND / 2)),
                8000.0,
                0.65,
                60.0,
                0.04,
                0.0,
            );
        }
        let hat_report = hat_only
            .analyze_multiband_beats(MultibandFluxConfig::default())
            .unwrap();
        assert_eq!(hat_report.high.onsets.len(), 8);
        assert!(
            hat_report.sub.onsets.is_empty(),
            "pure 8 kHz hi-hat leaked into sub: {:?}",
            hat_report.sub.onsets
        );
        assert!(
            hat_report.mid.onsets.is_empty(),
            "pure 8 kHz hi-hat leaked into mid: {:?}",
            hat_report.mid.onsets
        );
    }

    #[test]
    fn multiband_tracks_fast_170bpm_and_slow_90bpm_without_octave_errors() {
        // 170 BPM four-on-the-floor anime/EDM kick + offbeat hi-hat.
        let sr = DEFAULT_AUDIO_SAMPLE_RATE;
        let beat_ticks_170 = (60i128 * FLICKS_PER_SECOND as i128) / 170;
        let dur_170 = Flicks((beat_ticks_170 * 24) as i64);
        let mut fast = AudioBuffer::silence_for_duration(sr, 2, dur_170).unwrap();
        for b in 0..24 {
            let tick = Flicks((beat_ticks_170 * b) as i64);
            add_decay_tone(&mut fast, tick, 60.0, 0.85, 22.0, 0.12, 0.0);
            let off = Flicks(tick.0 + (beat_ticks_170 as i64) / 2);
            if off < dur_170 {
                add_decay_tone(&mut fast, off, 8000.0, 0.35, 70.0, 0.04, 0.1);
            }
        }
        let fast_report = fast
            .analyze_multiband_beats(MultibandFluxConfig::default())
            .unwrap();
        assert!(
            (fast_report.estimated_bpm - 170.0).abs() <= 2.5,
            "expected ~170 BPM, got {}",
            fast_report.estimated_bpm
        );

        let slow = build_synthetic_4_4_kit(90, 4, 0);
        let slow_report = slow
            .analyze_multiband_beats(MultibandFluxConfig::default())
            .unwrap();
        assert!(
            (slow_report.estimated_bpm - 90.0).abs() <= 2.0,
            "expected ~90 BPM, got {}",
            slow_report.estimated_bpm
        );
    }

    #[test]
    fn multiband_handles_non_48khz_sample_rate() {
        let sr = 44_100u32;
        let duration = Flicks(4 * FLICKS_PER_SECOND);
        let (buf, expected_beats) =
            SyntheticMediaGenerator::generate_beat_audio(duration, 120, sr);
        let report = buf
            .analyze_multiband_beats(MultibandFluxConfig::default())
            .unwrap();
        assert!(
            (report.estimated_bpm - 120.0).abs() <= 1.5,
            "expected ~120 BPM @ 44.1 kHz, got {}",
            report.estimated_bpm
        );
        assert_eq!(report.mid.onsets.len(), expected_beats.len());
    }
}
