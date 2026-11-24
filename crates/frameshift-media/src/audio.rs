use crate::error::MediaError;
use frameshift_time::{
    AUDIO_48_KHZ, FLICKS_PER_SECOND, Flicks, SpeedCurve, SpeedCurveSpec, audio_samples_to_ticks,
    ticks_to_audio_samples_nearest,
};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

/// Standard internal audio mixing sample rate (`48_000 Hz`, `14_700` Flicks/sample).
pub const DEFAULT_AUDIO_SAMPLE_RATE: u32 = AUDIO_48_KHZ;

/// Explicit audio behavior when a video clip undergoes speed-curve retiming (`timebase.md` §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AudioRetimeMode {
    /// Silences audio on retimed clips (common default for AMVs with separate BGM).
    #[default]
    Mute,
    /// Maps audio sample positions directly through the speed-curve integral (pitch shifts with speed).
    Varispeed,
    /// Deterministic Hann-windowed WSOLA pitch-preserving time-stretch.
    PitchPreserving,
}

/// Result of inspecting audio amplitude and RMS within a `Flicks` window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioPeakInfo {
    pub peak_sample_index: usize,
    pub peak_tick: Flicks,
    pub peak_amplitude: f32,
    pub rms: f32,
}

/// Summary of RMS energy envelope, detected onset/beat ticks, and estimated tempo (BPM).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioAnalysisReport {
    pub window_ticks: Flicks,
    pub rms_envelope: Vec<(Flicks, f32)>,
    pub detected_beats: Vec<Flicks>,
    pub estimated_bpm: f64,
}

/// Interleaved linear PCM `f32` audio buffer.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioBuffer {
    pub sample_rate: u32,
    pub channels: u16,
    /// Interleaved `f32` samples (`frame_count * channels`).
    pub samples: Vec<f32>,
}

impl AudioBuffer {
    /// Constructs a validated `AudioBuffer`.
    pub fn new(sample_rate: u32, channels: u16, samples: Vec<f32>) -> Result<Self, MediaError> {
        if sample_rate == 0 {
            return Err(MediaError::InvalidAudioFormat(
                "sample_rate must be non-zero".into(),
            ));
        }
        if channels == 0 {
            return Err(MediaError::InvalidAudioFormat(
                "channels must be non-zero".into(),
            ));
        }
        let ch = channels as usize;
        if !samples.len().is_multiple_of(ch) {
            return Err(MediaError::InvalidAudioFormat(format!(
                "sample count {} is not divisible by channel count {channels}",
                samples.len()
            )));
        }
        Ok(Self {
            sample_rate,
            channels,
            samples,
        })
    }

    /// Creates a silent `AudioBuffer` of `frame_count` audio frames.
    pub fn silence(
        sample_rate: u32,
        channels: u16,
        frame_count: usize,
    ) -> Result<Self, MediaError> {
        let total = frame_count
            .checked_mul(channels as usize)
            .ok_or_else(|| MediaError::InvalidAudioFormat("audio buffer size overflow".into()))?;
        Self::new(sample_rate, channels, vec![0.0; total])
    }

    /// Creates a silent `AudioBuffer` spanning `duration` Flicks.
    pub fn silence_for_duration(
        sample_rate: u32,
        channels: u16,
        duration: Flicks,
    ) -> Result<Self, MediaError> {
        if duration.0 < 0 {
            return Err(MediaError::InvalidAudioFormat(
                "audio duration cannot be negative".into(),
            ));
        }
        let frames = ticks_to_audio_samples_nearest(duration, sample_rate)?;
        let frame_count = usize::try_from(frames)
            .map_err(|_| MediaError::InvalidAudioFormat("audio frame count overflow".into()))?;
        Self::silence(sample_rate, channels, frame_count)
    }

    /// Number of multi-channel audio frames.
    #[inline]
    pub fn frame_count(&self) -> usize {
        self.samples.len() / (self.channels as usize)
    }

    /// Exact duration of this buffer in `Flicks`.
    pub fn duration_ticks(&self) -> Result<Flicks, MediaError> {
        let frames_i64 = i64::try_from(self.frame_count())
            .map_err(|_| MediaError::InvalidAudioFormat("frame count exceeds i64".into()))?;
        Ok(audio_samples_to_ticks(frames_i64, self.sample_rate)?)
    }

    /// Linearly interpolates a sample at fractional frame position `frame_pos` for `channel`.
    /// Returns `0.0` outside `[0, frame_count - 1]`.
    pub fn sample_at_interpolated(&self, frame_pos: f64, channel: u16) -> f32 {
        let frames = self.frame_count();
        if frames == 0 || !frame_pos.is_finite() || frame_pos < 0.0 {
            return 0.0;
        }
        let ch_count = self.channels as usize;
        let ch = (channel as usize).min(ch_count - 1);
        let max_idx = (frames - 1) as f64;
        if frame_pos > max_idx {
            return 0.0;
        }
        let i0 = frame_pos.floor() as usize;
        let frac = (frame_pos - i0 as f64) as f32;
        let s0 = self.samples[i0 * ch_count + ch];
        if i0 + 1 < frames {
            let s1 = self.samples[(i0 + 1) * ch_count + ch];
            s0 + (s1 - s0) * frac
        } else {
            s0
        }
    }

    /// Resamples this buffer to `target_sample_rate` using exact `i128` tick-ratio mapping.
    pub fn resample_to(&self, target_sample_rate: u32) -> Result<Self, MediaError> {
        if target_sample_rate == 0 {
            return Err(MediaError::InvalidAudioFormat(
                "target sample_rate must be non-zero".into(),
            ));
        }
        if self.sample_rate == target_sample_rate {
            return Ok(self.clone());
        }
        let src_frames = self.frame_count();
        if src_frames == 0 {
            return Self::silence(target_sample_rate, self.channels, 0);
        }
        let dst_frames_i128 = ((src_frames as i128) * (target_sample_rate as i128)
            + (self.sample_rate as i128) / 2)
            / (self.sample_rate as i128);
        let dst_frames = usize::try_from(dst_frames_i128)
            .map_err(|_| MediaError::InvalidAudioFormat("resampled frame count overflow".into()))?;
        let ch_count = self.channels as usize;
        let mut out = Vec::with_capacity(dst_frames * ch_count);
        let ratio = self.sample_rate as f64 / target_sample_rate as f64;
        for i in 0..dst_frames {
            let src_pos = (i as f64) * ratio;
            for ch in 0..self.channels {
                out.push(self.sample_at_interpolated(src_pos, ch));
            }
        }
        Self::new(target_sample_rate, self.channels, out)
    }

    /// Mixes `source` into `self` starting at `offset_ticks` with `gain_db` gain.
    ///
    /// Delegates to [`Self::mix_in_with_gain_curve`] with a constant `1.0` curve, which keeps
    /// the mixed samples bit-identical to a plain constant-gain mix.
    pub fn mix_in(
        &mut self,
        source: &AudioBuffer,
        offset_ticks: Flicks,
        gain_db: f32,
    ) -> Result<(), MediaError> {
        self.mix_in_with_gain_curve(source, offset_ticks, gain_db, |_| 1.0)
    }

    /// Mixes `source` into `self` starting at `offset_ticks` with `gain_db` gain scaled by a
    /// per-frame `gain_curve`.
    ///
    /// `gain_curve` receives the normalized source frame position: `0.0` for the first frame
    /// and `1.0` for the last frame (a single-frame source passes `0.0`). Its return value is
    /// multiplied by the `gain_db` base multiplier and must be finite and non-negative; any
    /// invalid value returns [`MediaError::InvalidAudioFormat`].
    ///
    /// Channel and sample-rate handling matches [`Self::mix_in`]: a mono source is duplicated
    /// to every destination channel, extra destination channels repeat the source channels,
    /// and a source at a different sample rate is resampled first. Curve values are validated
    /// before any sample is written, so an invalid curve leaves `self` unchanged.
    pub fn mix_in_with_gain_curve<F: Fn(f32) -> f32>(
        &mut self,
        source: &AudioBuffer,
        offset_ticks: Flicks,
        gain_db: f32,
        gain_curve: F,
    ) -> Result<(), MediaError> {
        if !gain_db.is_finite() {
            return Err(MediaError::InvalidAudioFormat(
                "gain_db must be finite".into(),
            ));
        }
        let resampled_owned;
        let src = if source.sample_rate == self.sample_rate {
            source
        } else {
            resampled_owned = source.resample_to(self.sample_rate)?;
            &resampled_owned
        };

        let base_gain = 10.0f32.powf(gain_db / 20.0);
        let src_frames = src.frame_count();
        let denom = src_frames.saturating_sub(1).max(1) as f64;
        let mut frame_gains = Vec::with_capacity(src_frames);
        for frame in 0..src_frames {
            let progress = (frame as f64 / denom) as f32;
            let curve = gain_curve(progress);
            if !curve.is_finite() || curve < 0.0 {
                return Err(MediaError::InvalidAudioFormat(format!(
                    "gain curve value {curve} at source frame {frame} must be finite and non-negative"
                )));
            }
            let gain = base_gain * curve;
            if !gain.is_finite() {
                return Err(MediaError::InvalidAudioFormat(format!(
                    "gain curve value {curve} at source frame {frame} produces a non-finite gain"
                )));
            }
            frame_gains.push(gain);
        }

        let start_frame = ticks_to_audio_samples_nearest(offset_ticks, self.sample_rate)?;
        let dst_frames = self.frame_count();
        let dst_ch = self.channels as usize;
        let src_ch = src.channels as usize;

        for (sf, &gain) in frame_gains.iter().enumerate() {
            let df_i64 = start_frame.saturating_add(sf as i64);
            if df_i64 < 0 {
                continue;
            }
            let df = df_i64 as usize;
            if df >= dst_frames {
                break;
            }
            for ch in 0..dst_ch {
                let sample = if src_ch == 1 {
                    src.samples[sf]
                } else if ch < src_ch {
                    src.samples[sf * src_ch + ch]
                } else {
                    src.samples[sf * src_ch + (ch % src_ch)]
                };
                self.samples[df * dst_ch + ch] += sample * gain;
            }
        }
        Ok(())
    }

    /// Mixes multiple `(buffer, timeline_offset, gain_db)` tracks into a single output buffer.
    pub fn mix_tracks(
        sample_rate: u32,
        channels: u16,
        duration: Flicks,
        tracks: &[(&AudioBuffer, Flicks, f32)],
    ) -> Result<Self, MediaError> {
        let mut out = Self::silence_for_duration(sample_rate, channels, duration)?;
        for &(buf, offset, gain_db) in tracks {
            out.mix_in(buf, offset, gain_db)?;
        }
        for s in &mut out.samples {
            *s = s.clamp(-1.0, 1.0);
        }
        Ok(out)
    }

    /// Retimes this audio buffer through `speed_curve` over `output_duration`.
    ///
    /// - `AudioRetimeMode::Mute`: returns silence.
    /// - `AudioRetimeMode::Varispeed`: resamples continuously along `speed_curve.source_offset_f64`.
    /// - `AudioRetimeMode::PitchPreserving`: deterministic Hann-windowed WSOLA time-stretch.
    pub fn retime(
        &self,
        speed_curve: &SpeedCurve,
        output_duration: Flicks,
        source_start: Flicks,
        mode: AudioRetimeMode,
    ) -> Result<Self, MediaError> {
        match mode {
            AudioRetimeMode::PitchPreserving => {
                self.retime_pitch_preserving_curve(speed_curve, output_duration, source_start)
            }
            AudioRetimeMode::Mute => {
                Self::silence_for_duration(self.sample_rate, self.channels, output_duration)
            }
            AudioRetimeMode::Varispeed => {
                let mut out =
                    Self::silence_for_duration(self.sample_rate, self.channels, output_duration)?;
                let out_frames = out.frame_count();
                let ch_count = self.channels as usize;
                let samples_per_flick = self.sample_rate as f64 / FLICKS_PER_SECOND as f64;
                let source_start_f64 = source_start.0 as f64;

                for out_idx in 0..out_frames {
                    let out_tick = audio_samples_to_ticks(out_idx as i64, self.sample_rate)?;
                    let speed = speed_curve.instant_speed_at(out_tick);
                    if speed == 0.0 {
                        continue;
                    }
                    let src_tick = source_start_f64 + speed_curve.source_offset_f64(out_tick);
                    let src_sample_pos = src_tick * samples_per_flick;
                    for ch in 0..self.channels {
                        out.samples[out_idx * ch_count + ch as usize] =
                            self.sample_at_interpolated(src_sample_pos, ch);
                    }
                }
                Ok(out)
            }
        }
    }

    /// Retimes this audio buffer by constant `speed_ratio` (`0.25..=4.0`) while preserving
    /// fundamental pitch using deterministic pure-Rust Hann-windowed WSOLA.
    ///
    /// Output duration is approximately `input_duration / speed_ratio`.
    pub fn retime_pitch_preserving(&self, speed_ratio: f64) -> Result<Self, MediaError> {
        if !speed_ratio.is_finite() || !(0.25..=4.0).contains(&speed_ratio) {
            return Err(MediaError::InvalidAudioFormat(format!(
                "pitch-preserving speed_ratio {speed_ratio} must be finite and within 0.25..=4.0"
            )));
        }
        let in_frames = self.frame_count();
        if in_frames == 0 {
            return Self::silence(self.sample_rate, self.channels, 0);
        }
        if (speed_ratio - 1.0).abs() < 1e-9 {
            return Ok(self.clone());
        }

        let out_frames = ((in_frames as f64) / speed_ratio).round().max(1.0) as usize;
        if in_frames < 32 {
            return self.linear_stretch_fallback(out_frames);
        }

        let ch_count = self.channels as usize;
        let win_len = wsola_window_len(self.sample_rate, in_frames);
        let syn_hop = (win_len / 4).max(1);
        let corr_len = (win_len / 2).clamp(8, 480).min(in_frames);
        let delta_max = ((self.sample_rate as usize) / 80).clamp(16, win_len);

        let mono = self.to_mono_mixdown();
        let hann = make_hann_window(win_len);

        let mut out_acc = vec![0.0f64; out_frames * ch_count];
        let mut weight_acc = vec![0.0f64; out_frames];

        // First grain at origin anchors the waveform phase.
        accumulate_grain(
            &self.samples,
            ch_count,
            in_frames,
            0,
            &mut out_acc,
            &mut weight_acc,
            out_frames,
            0,
            &hann,
        );
        let mut prev_m = 0usize;

        let mut y_pos = syn_hop;
        while y_pos < out_frames {
            let ideal_ana = ((y_pos as f64) * speed_ratio).round() as isize;
            let ref_pos = (prev_m + syn_hop).min(in_frames.saturating_sub(corr_len));
            let best_m =
                find_best_wsola_offset(&mono, in_frames, ref_pos, ideal_ana, corr_len, delta_max);
            accumulate_grain(
                &self.samples,
                ch_count,
                in_frames,
                best_m,
                &mut out_acc,
                &mut weight_acc,
                out_frames,
                y_pos,
                &hann,
            );
            prev_m = best_m;
            y_pos += syn_hop;
        }

        normalize_overlap_add(
            self.sample_rate,
            self.channels,
            out_frames,
            &out_acc,
            &weight_acc,
        )
    }

    fn retime_pitch_preserving_curve(
        &self,
        speed_curve: &SpeedCurve,
        output_duration: Flicks,
        source_start: Flicks,
    ) -> Result<Self, MediaError> {
        if output_duration.0 < 0 {
            return Err(MediaError::InvalidAudioFormat(
                "audio duration cannot be negative".into(),
            ));
        }
        let out_frames =
            ticks_to_audio_samples_nearest(output_duration, self.sample_rate)? as usize;
        if out_frames == 0 || self.frame_count() == 0 {
            return Self::silence(self.sample_rate, self.channels, out_frames);
        }

        match speed_curve.spec() {
            SpeedCurveSpec::Freeze => Self::silence(self.sample_rate, self.channels, out_frames),
            SpeedCurveSpec::Constant { speed } => {
                if *speed == 0.0 {
                    return Self::silence(self.sample_rate, self.channels, out_frames);
                }
                if !speed.is_finite() || !(0.25..=4.0).contains(speed) {
                    return Err(MediaError::InvalidAudioFormat(format!(
                        "pitch-preserving speed {speed} must be finite and within 0.25..=4.0"
                    )));
                }
                let start_frame_i64 =
                    ticks_to_audio_samples_nearest(source_start, self.sample_rate)?;
                let ch_count = self.channels as usize;
                let in_frames = self.frame_count();
                let start_frame = start_frame_i64.max(0) as usize;
                if start_frame >= in_frames {
                    return Self::silence(self.sample_rate, self.channels, out_frames);
                }
                let needed_src = ((out_frames as f64) * (*speed)).round().max(1.0) as usize;
                let end_frame = (start_frame + needed_src).min(in_frames);
                let slice_samples =
                    self.samples[start_frame * ch_count..end_frame * ch_count].to_vec();
                let slice = Self::new(self.sample_rate, self.channels, slice_samples)?;
                let mut stretched = slice.retime_pitch_preserving(*speed)?;
                stretched.samples.resize(out_frames * ch_count, 0.0);
                Ok(stretched)
            }
            SpeedCurveSpec::PiecewiseLinear { .. }
            | SpeedCurveSpec::CubicHermite { .. }
            | SpeedCurveSpec::CubicBezierValue { .. }
            | SpeedCurveSpec::CubicBezierSpeed { .. } => {
                let in_frames = self.frame_count();
                if in_frames < 32 {
                    return self.retime(
                        speed_curve,
                        output_duration,
                        source_start,
                        AudioRetimeMode::Varispeed,
                    );
                }

                let ch_count = self.channels as usize;
                let win_len = wsola_window_len(self.sample_rate, in_frames);
                let syn_hop = (win_len / 4).max(1);
                let corr_len = (win_len / 2).clamp(8, 480).min(in_frames);
                let delta_max = ((self.sample_rate as usize) / 80).clamp(16, win_len);
                let samples_per_flick = self.sample_rate as f64 / FLICKS_PER_SECOND as f64;
                let source_start_f64 = source_start.0 as f64;

                let mono = self.to_mono_mixdown();
                let hann = make_hann_window(win_len);

                let mut out_acc = vec![0.0f64; out_frames * ch_count];
                let mut weight_acc = vec![0.0f64; out_frames];
                let mut prev_m: Option<usize> = None;

                let mut y_pos = 0usize;
                while y_pos < out_frames {
                    let out_tick = audio_samples_to_ticks(y_pos as i64, self.sample_rate)?;
                    let inst_speed = speed_curve.instant_speed_at(out_tick);
                    if inst_speed < 0.0 {
                        return Err(MediaError::InvalidAudioFormat(
                            "pitch-preserving retime does not support negative speed".into(),
                        ));
                    }
                    if inst_speed > 0.0 {
                        let src_tick = source_start_f64 + speed_curve.source_offset_f64(out_tick);
                        let ideal_ana = (src_tick * samples_per_flick).round() as isize;
                        if ideal_ana >= 0 && (ideal_ana as usize) < in_frames {
                            let best_m = if let Some(pm) = prev_m {
                                let ref_pos =
                                    (pm + syn_hop).min(in_frames.saturating_sub(corr_len));
                                find_best_wsola_offset(
                                    &mono, in_frames, ref_pos, ideal_ana, corr_len, delta_max,
                                )
                            } else {
                                (ideal_ana as usize).min(in_frames.saturating_sub(corr_len))
                            };
                            accumulate_grain(
                                &self.samples,
                                ch_count,
                                in_frames,
                                best_m,
                                &mut out_acc,
                                &mut weight_acc,
                                out_frames,
                                y_pos,
                                &hann,
                            );
                            prev_m = Some(best_m);
                        }
                    }
                    y_pos += syn_hop;
                }

                normalize_overlap_add(
                    self.sample_rate,
                    self.channels,
                    out_frames,
                    &out_acc,
                    &weight_acc,
                )
            }
        }
    }

    fn linear_stretch_fallback(&self, out_frames: usize) -> Result<Self, MediaError> {
        let in_frames = self.frame_count();
        let ch_count = self.channels as usize;
        let mut out = Vec::with_capacity(out_frames * ch_count);
        let ratio = if out_frames > 1 {
            (in_frames.saturating_sub(1)) as f64 / (out_frames - 1) as f64
        } else {
            0.0
        };
        for i in 0..out_frames {
            let pos = (i as f64) * ratio;
            for ch in 0..self.channels {
                out.push(self.sample_at_interpolated(pos, ch));
            }
        }
        Self::new(self.sample_rate, self.channels, out)
    }

    fn to_mono_mixdown(&self) -> Vec<f32> {
        let frames = self.frame_count();
        let ch_count = self.channels as usize;
        let inv_ch = 1.0 / (ch_count as f32);
        let mut mono = Vec::with_capacity(frames);
        for f in 0..frames {
            let base = f * ch_count;
            let mut sum = 0.0f32;
            for ch in 0..ch_count {
                sum += self.samples[base + ch];
            }
            mono.push(sum * inv_ch);
        }
        mono
    }

    /// Computes the unnormalized linear RMS energy envelope (`[0.0, 1.0]`) across uniform
    /// windows `[i * window_ticks, (i + 1) * window_ticks)`, averaging across all channels.
    ///
    /// Silent windows produce `0.0`.
    pub fn compute_rms_envelope(&self, window_ticks: Flicks) -> Result<Vec<f32>, MediaError> {
        if window_ticks.0 <= 0 {
            return Err(MediaError::InvalidAudioFormat(
                "RMS window_ticks must be positive".into(),
            ));
        }
        if self.frame_count() == 0 {
            return Ok(Vec::new());
        }

        let windows = self.compute_rms_windows(window_ticks)?;
        Ok(windows
            .into_iter()
            .map(|(_, _, _, rms)| {
                if rms.is_finite() {
                    rms.clamp(0.0, 1.0)
                } else {
                    0.0
                }
            })
            .collect())
    }

    fn compute_rms_windows(
        &self,
        window_ticks: Flicks,
    ) -> Result<Vec<(Flicks, usize, usize, f32)>, MediaError> {
        let total_frames = self.frame_count();
        let ch_count = self.channels as usize;
        let mut windows = Vec::new();

        let mut w_idx: i64 = 0;
        loop {
            let start_tick = Flicks(w_idx.saturating_mul(window_ticks.0));
            let start_frame =
                ticks_to_audio_samples_nearest(start_tick, self.sample_rate)?.max(0) as usize;
            if start_frame >= total_frames {
                break;
            }
            let end_tick = Flicks((w_idx + 1).saturating_mul(window_ticks.0));
            let raw_end =
                ticks_to_audio_samples_nearest(end_tick, self.sample_rate)?.max(0) as usize;
            let end_frame = raw_end.max(start_frame + 1).min(total_frames);

            let mut sum_sq = 0.0f64;
            let mut count = 0usize;
            for f in start_frame..end_frame {
                let base = f * ch_count;
                for ch in 0..ch_count {
                    let s = self.samples[base + ch] as f64;
                    sum_sq += s * s;
                    count += 1;
                }
            }
            let rms = if count > 0 {
                (sum_sq / count as f64).sqrt() as f32
            } else {
                0.0
            };

            windows.push((start_tick, start_frame, end_frame, rms));
            w_idx += 1;
        }
        Ok(windows)
    }

    /// Computes a windowed RMS energy envelope (`window_ticks` per step), detects
    /// local onset/beat energy peaks above an adaptive threshold with refractory spacing,
    /// and estimates tempo in BPM.
    pub fn analyze_beats_and_energy(
        &self,
        window_ticks: Flicks,
    ) -> Result<AudioAnalysisReport, MediaError> {
        if window_ticks.0 <= 0 {
            return Err(MediaError::InvalidAudioFormat(
                "analysis window_ticks must be positive".into(),
            ));
        }
        if self.frame_count() == 0 {
            return Err(MediaError::EmptyMediaStream(
                "cannot analyze beats and energy on empty audio buffer".into(),
            ));
        }

        let windows = self.compute_rms_windows(window_ticks)?;
        let mut rms_envelope = Vec::with_capacity(windows.len());
        let mut window_ranges: Vec<(usize, usize, f32)> = Vec::with_capacity(windows.len());
        for (start_tick, start_frame, end_frame, rms) in windows {
            rms_envelope.push((start_tick, rms));
            window_ranges.push((start_frame, end_frame, rms));
        }

        let max_rms = window_ranges.iter().map(|w| w.2).fold(0.0f32, f32::max);
        if max_rms < 1e-6 {
            return Ok(AudioAnalysisReport {
                window_ticks,
                rms_envelope,
                detected_beats: Vec::new(),
                estimated_bpm: 0.0,
            });
        }

        let n = window_ranges.len();
        let mut prefix = Vec::with_capacity(n + 1);
        prefix.push(0.0f64);
        for &(_, _, r) in &window_ranges {
            prefix.push(prefix.last().unwrap() + r as f64);
        }

        let ch_count = self.channels as usize;
        let ctx_half = ((FLICKS_PER_SECOND / 2) / window_ticks.0).max(4) as usize;
        let refractory_ticks = FLICKS_PER_SECOND / 5; // 200 ms -> up to 300 BPM
        let mut detected_beats: Vec<Flicks> = Vec::new();

        for i in 0..n {
            let cur = window_ranges[i].2;
            let prev = if i == 0 { 0.0 } else { window_ranges[i - 1].2 };
            let next = if i + 1 == n {
                0.0
            } else {
                window_ranges[i + 1].2
            };
            if !(cur > prev && cur >= next) {
                continue;
            }

            let lo = i.saturating_sub(ctx_half);
            let hi = (i + ctx_half + 1).min(n);
            let local_mean = ((prefix[hi] - prefix[lo]) / (hi - lo) as f64) as f32;
            let adaptive_threshold = local_mean * 1.4 + 0.06 * max_rms;

            if cur >= adaptive_threshold {
                let (sf, ef, _) = window_ranges[i];
                let mut best_frame = sf;
                let mut best_amp = -1.0f32;
                for f in sf..ef {
                    let base = f * ch_count;
                    let mut frame_amp = 0.0f32;
                    for ch in 0..ch_count {
                        let a = self.samples[base + ch].abs();
                        if a > frame_amp {
                            frame_amp = a;
                        }
                    }
                    if frame_amp > best_amp {
                        best_amp = frame_amp;
                        best_frame = f;
                    }
                }

                let beat_tick = audio_samples_to_ticks(best_frame as i64, self.sample_rate)?;
                if detected_beats
                    .last()
                    .is_none_or(|&prev_tick| beat_tick.0 - prev_tick.0 >= refractory_ticks)
                {
                    detected_beats.push(beat_tick);
                }
            }
        }

        let estimated_bpm = estimate_bpm_from_beats(&detected_beats);

        Ok(AudioAnalysisReport {
            window_ticks,
            rms_envelope,
            detected_beats,
            estimated_bpm,
        })
    }

    /// Finds the peak amplitude and RMS in `[center - half_window, center + half_window]`.
    pub fn detect_peak_in_window(
        &self,
        center: Flicks,
        half_window: Flicks,
    ) -> Result<AudioPeakInfo, MediaError> {
        let frames = self.frame_count();
        if frames == 0 {
            return Err(MediaError::EmptyMediaStream(
                "cannot detect audio peak in empty buffer".into(),
            ));
        }
        let start_tick = Flicks(center.0.saturating_sub(half_window.0.max(0)));
        let end_tick = Flicks(center.0.saturating_add(half_window.0.max(0)));
        let start_idx =
            ticks_to_audio_samples_nearest(start_tick, self.sample_rate)?.max(0) as usize;
        let end_idx = (ticks_to_audio_samples_nearest(end_tick, self.sample_rate)?.max(0) as usize)
            .min(frames - 1);

        if start_idx >= frames || start_idx > end_idx {
            return Err(MediaError::InvalidAudioFormat(
                "peak detection window falls outside buffer".into(),
            ));
        }

        let ch_count = self.channels as usize;
        let mut peak_idx = start_idx;
        let mut peak_amp = -1.0f32;
        let mut sum_sq = 0.0f64;
        let mut count = 0usize;

        for frame_idx in start_idx..=end_idx {
            let mut frame_peak = 0.0f32;
            for ch in 0..ch_count {
                let v = self.samples[frame_idx * ch_count + ch];
                let abs_v = v.abs();
                if abs_v > frame_peak {
                    frame_peak = abs_v;
                }
                sum_sq += (v as f64) * (v as f64);
                count += 1;
            }
            if frame_peak > peak_amp {
                peak_amp = frame_peak;
                peak_idx = frame_idx;
            }
        }

        let peak_tick = audio_samples_to_ticks(peak_idx as i64, self.sample_rate)?;
        let rms = if count > 0 {
            (sum_sq / count as f64).sqrt() as f32
        } else {
            0.0
        };

        Ok(AudioPeakInfo {
            peak_sample_index: peak_idx,
            peak_tick,
            peak_amplitude: peak_amp,
            rms,
        })
    }

    /// Detects local transient peaks above `threshold` separated by at least `min_gap`.
    pub fn detect_transients(
        &self,
        threshold: f32,
        min_gap: Flicks,
    ) -> Result<Vec<AudioPeakInfo>, MediaError> {
        let frames = self.frame_count();
        if frames == 0 {
            return Ok(Vec::new());
        }
        let min_gap_samples =
            ticks_to_audio_samples_nearest(Flicks(min_gap.0.max(0)), self.sample_rate)?.max(1)
                as usize;
        let ch_count = self.channels as usize;
        let mut peaks = Vec::new();
        let mut i = 0usize;
        while i < frames {
            let amp = (0..ch_count)
                .map(|ch| self.samples[i * ch_count + ch].abs())
                .fold(0.0f32, f32::max);
            if amp >= threshold {
                // Find the maximum within a small local cluster to land on the exact transient apex.
                let search_end = (i + min_gap_samples / 2).min(frames);
                let mut best_idx = i;
                let mut best_amp = amp;
                for j in (i + 1)..search_end {
                    let a = (0..ch_count)
                        .map(|ch| self.samples[j * ch_count + ch].abs())
                        .fold(0.0f32, f32::max);
                    if a > best_amp {
                        best_amp = a;
                        best_idx = j;
                    }
                }
                let peak_tick = audio_samples_to_ticks(best_idx as i64, self.sample_rate)?;
                peaks.push(AudioPeakInfo {
                    peak_sample_index: best_idx,
                    peak_tick,
                    peak_amplitude: best_amp,
                    rms: best_amp,
                });
                i = best_idx + min_gap_samples;
            } else {
                i += 1;
            }
        }
        Ok(peaks)
    }

    /// Serializes samples as little-endian IEEE 754 `f32` PCM bytes.
    pub fn to_f32le_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.samples.len() * 4);
        for &s in &self.samples {
            out.extend_from_slice(&s.to_le_bytes());
        }
        out
    }

    /// Writes this buffer as an uncompressed IEEE-float WAV (`audio/wav`, format tag 3).
    pub fn write_wav(&self, path: &Path) -> Result<(), MediaError> {
        let data_bytes = u32::try_from(self.samples.len() * 4)
            .map_err(|_| MediaError::InvalidAudioFormat("WAV data exceeds 4 GiB limit".into()))?;
        let byte_rate = self
            .sample_rate
            .checked_mul(self.channels as u32)
            .and_then(|v| v.checked_mul(4))
            .ok_or_else(|| MediaError::InvalidAudioFormat("WAV byte rate overflow".into()))?;
        let block_align = self.channels * 4;
        let riff_size = data_bytes
            .checked_add(36)
            .ok_or_else(|| MediaError::InvalidAudioFormat("WAV RIFF size overflow".into()))?;

        let file = File::create(path)?;
        let mut w = BufWriter::new(file);
        w.write_all(b"RIFF")?;
        w.write_all(&riff_size.to_le_bytes())?;
        w.write_all(b"WAVE")?;
        w.write_all(b"fmt ")?;
        w.write_all(&16u32.to_le_bytes())?;
        w.write_all(&3u16.to_le_bytes())?; // IEEE float PCM
        w.write_all(&self.channels.to_le_bytes())?;
        w.write_all(&self.sample_rate.to_le_bytes())?;
        w.write_all(&byte_rate.to_le_bytes())?;
        w.write_all(&block_align.to_le_bytes())?;
        w.write_all(&32u16.to_le_bytes())?;
        w.write_all(b"data")?;
        w.write_all(&data_bytes.to_le_bytes())?;
        for &s in &self.samples {
            w.write_all(&s.to_le_bytes())?;
        }
        w.flush()?;
        Ok(())
    }
}

/// Filter response kind for [`apply_biquad_filter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BiquadFilterKind {
    LowPass,
    HighPass,
}

/// Applies soft `tanh` saturation above a smooth knee so peaks never exceed `ceiling`.
///
/// Signals below `0.75 * ceiling` pass through unmodified (`C^1` smooth transition into `tanh`).
pub fn apply_soft_limiter(buf: &mut AudioBuffer, ceiling: f32) {
    if !ceiling.is_finite() {
        return;
    }
    if ceiling <= 0.0 {
        buf.samples.fill(0.0);
        return;
    }
    let c = ceiling;
    let knee = 0.75 * c;
    let headroom = c - knee;
    for s in &mut buf.samples {
        let x = *s;
        if !x.is_finite() {
            *s = if x.is_nan() {
                0.0
            } else if x.is_sign_positive() {
                c
            } else {
                -c
            };
            continue;
        }
        let abs_x = x.abs();
        if abs_x <= knee {
            continue;
        }
        let limited = if headroom > 1e-7 {
            knee + headroom * ((abs_x - knee) / headroom).tanh()
        } else {
            c
        };
        *s = x.signum() * limited.clamp(0.0, c);
    }
}

/// Applies a 2nd-order Robert Bristow-Johnson (RBJ) biquad filter in-place per channel.
pub fn apply_biquad_filter(
    buf: &mut AudioBuffer,
    kind: BiquadFilterKind,
    cutoff_hz: f32,
    q: f32,
) {
    let frames = buf.frame_count();
    if frames == 0 || !cutoff_hz.is_finite() || !q.is_finite() {
        return;
    }

    let fs = buf.sample_rate as f64;
    let f0 = (cutoff_hz as f64).clamp(1.0, fs * 0.49);
    let q_clamped = (q as f64).max(0.05);

    let omega0 = 2.0 * std::f64::consts::PI * f0 / fs;
    let cos_w0 = omega0.cos();
    let sin_w0 = omega0.sin();
    let alpha = sin_w0 / (2.0 * q_clamped);

    let (b0, b1, b2, a0, a1, a2) = match kind {
        BiquadFilterKind::LowPass => {
            let b1 = 1.0 - cos_w0;
            let b0 = b1 * 0.5;
            (b0, b1, b0, 1.0 + alpha, -2.0 * cos_w0, 1.0 - alpha)
        }
        BiquadFilterKind::HighPass => {
            let one_plus = 1.0 + cos_w0;
            let b0 = one_plus * 0.5;
            (b0, -one_plus, b0, 1.0 + alpha, -2.0 * cos_w0, 1.0 - alpha)
        }
    };

    let b0 = b0 / a0;
    let b1 = b1 / a0;
    let b2 = b2 / a0;
    let a1 = a1 / a0;
    let a2 = a2 / a0;

    let ch_count = buf.channels as usize;
    for ch in 0..ch_count {
        let mut z1 = 0.0f64;
        let mut z2 = 0.0f64;
        for f in 0..frames {
            let idx = f * ch_count + ch;
            let x = buf.samples[idx] as f64;
            let x = if x.is_finite() { x } else { 0.0 };
            let y = b0 * x + z1;
            z1 = b1 * x - a1 * y + z2;
            z2 = b2 * x - a2 * y;
            buf.samples[idx] = y as f32;
        }
    }
}

/// Applies a smooth micro-silence dip across frame range `[start_sample, end_sample)`.
///
/// Ramps smoothly from `1.0` down to `floor_gain` over `fade_samples` at entry and back to
/// `1.0` over `fade_samples` at exit using raised-cosine ramps so adjacent audio stays click-free.
pub fn apply_micro_silence(
    buf: &mut AudioBuffer,
    start_sample: usize,
    end_sample: usize,
    fade_samples: usize,
    floor_gain: f32,
) {
    let frames = buf.frame_count();
    let start = start_sample.min(frames);
    let end = end_sample.min(frames);
    if frames == 0 || start >= end {
        return;
    }

    let floor = if floor_gain.is_finite() {
        floor_gain.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let len = end - start;
    let eff_fade = fade_samples.min(len / 2);
    let ch_count = buf.channels as usize;

    for f in start..end {
        let gain = if eff_fade == 0 {
            floor
        } else if f - start < eff_fade {
            let t = (f - start) as f32 / eff_fade as f32;
            let alpha = 0.5 * (1.0 - (std::f32::consts::PI * t).cos());
            1.0 + (floor - 1.0) * alpha
        } else if end - f <= eff_fade {
            let t = (end - f) as f32 / eff_fade as f32;
            let alpha = 0.5 * (1.0 - (std::f32::consts::PI * t).cos());
            1.0 + (floor - 1.0) * alpha
        } else {
            floor
        };
        let base = f * ch_count;
        for ch in 0..ch_count {
            buf.samples[base + ch] *= gain;
        }
    }
}

/// Ducks `target` smoothly to `duck_gain` whenever `trigger` exceeds `threshold`.
pub fn apply_sidechain_ducking(
    target: &mut AudioBuffer,
    trigger: &AudioBuffer,
    threshold: f32,
    duck_gain: f32,
    attack_ms: f32,
    release_ms: f32,
) {
    let target_frames = target.frame_count();
    if target_frames == 0
        || trigger.frame_count() == 0
        || !threshold.is_finite()
        || !duck_gain.is_finite()
        || !attack_ms.is_finite()
        || !release_ms.is_finite()
    {
        return;
    }

    let resampled_trigger;
    let trig = if trigger.sample_rate == target.sample_rate {
        trigger
    } else if let Ok(r) = trigger.resample_to(target.sample_rate) {
        resampled_trigger = r;
        &resampled_trigger
    } else {
        return;
    };

    let thresh = threshold.max(0.0) as f64;
    let duck = duck_gain.clamp(0.0, 1.0) as f64;
    let fs = target.sample_rate as f64;

    let attack_sec = (attack_ms.max(0.0) as f64) * 0.001;
    let release_sec = (release_ms.max(0.0) as f64) * 0.001;
    let attack_coeff = if attack_sec <= 0.0 {
        0.0
    } else {
        (-1.0 / (attack_sec * fs)).exp()
    };
    let release_coeff = if release_sec <= 0.0 {
        0.0
    } else {
        (-1.0 / (release_sec * fs)).exp()
    };
    // Short peak-hold decay prevents zero-crossings in tonal triggers from chattering the ducker.
    let trig_hold_sec = (release_sec * 0.25).clamp(0.0005, 0.005);
    let trig_decay = if release_sec <= 0.0 {
        0.0
    } else {
        (-1.0 / (trig_hold_sec * fs)).exp()
    };

    let target_ch = target.channels as usize;
    let trig_ch = trig.channels as usize;
    let trig_frames = trig.frame_count();

    let mut trig_env = 0.0f64;
    let mut gain = 1.0f64;

    for f in 0..target_frames {
        let det = if f < trig_frames {
            let base = f * trig_ch;
            let mut peak = 0.0f64;
            for ch in 0..trig_ch {
                let v = trig.samples[base + ch].abs() as f64;
                if v.is_finite() && v > peak {
                    peak = v;
                }
            }
            peak
        } else {
            0.0
        };

        if det > trig_env {
            trig_env = det;
        } else {
            trig_env = trig_decay * trig_env + (1.0 - trig_decay) * det;
        }

        let desired_gain = if trig_env > thresh { duck } else { 1.0 };
        if desired_gain < gain {
            gain = attack_coeff * gain + (1.0 - attack_coeff) * desired_gain;
        } else {
            gain = release_coeff * gain + (1.0 - release_coeff) * desired_gain;
        }

        let g = gain as f32;
        let base = f * target_ch;
        for ch in 0..target_ch {
            target.samples[base + ch] *= g;
        }
    }
}

/// Applies constant-power (equal-power) stereo panning across `pan` in `-1.0..=1.0`.
///
/// Mono buffers are promoted in-place to 2-channel stereo (`L^2 + R^2 = mono^2`); stereo and
/// multi-channel buffers scale channels `0` (left) and `1` (right) by `(cos(theta), sin(theta))`.
pub fn apply_stereo_pan(buf: &mut AudioBuffer, pan: f32) {
    if !pan.is_finite() || buf.frame_count() == 0 {
        return;
    }
    let clamped = pan.clamp(-1.0, 1.0) as f64;
    let theta = (clamped + 1.0) * 0.25 * std::f64::consts::PI;
    let gain_l = theta.cos() as f32;
    let gain_r = theta.sin() as f32;

    if buf.channels == 1 {
        let mut stereo = Vec::with_capacity(buf.samples.len() * 2);
        for &s in &buf.samples {
            stereo.push(s * gain_l);
            stereo.push(s * gain_r);
        }
        buf.channels = 2;
        buf.samples = stereo;
        return;
    }

    let ch_count = buf.channels as usize;
    let frames = buf.frame_count();
    for f in 0..frames {
        let base = f * ch_count;
        buf.samples[base] *= gain_l;
        buf.samples[base + 1] *= gain_r;
    }
}

fn wsola_window_len(sample_rate: u32, in_frames: usize) -> usize {
    let nominal = ((sample_rate as f64) * 0.040).round() as usize;
    let rounded = (nominal / 4).max(4) * 4;
    if rounded <= in_frames {
        rounded
    } else {
        ((in_frames / 2) / 4).max(4) * 4
    }
    .min(in_frames.max(4))
}

fn make_hann_window(len: usize) -> Vec<f32> {
    let mut w = Vec::with_capacity(len);
    let denom = len as f64;
    for i in 0..len {
        let phase = 2.0 * std::f64::consts::PI * ((i as f64) + 0.5) / denom;
        w.push((0.5 * (1.0 - phase.cos())) as f32);
    }
    w
}

fn find_best_wsola_offset(
    mono: &[f32],
    in_frames: usize,
    ref_pos: usize,
    ideal_ana: isize,
    corr_len: usize,
    delta_max: usize,
) -> usize {
    let max_start = in_frames.saturating_sub(corr_len);
    let clamped_ideal = ideal_ana.clamp(0, max_start as isize) as usize;
    let search_lo = (ideal_ana - delta_max as isize).max(0) as usize;
    let search_hi = ((ideal_ana + delta_max as isize).min(max_start as isize)).max(0) as usize;
    if search_lo >= search_hi {
        return clamped_ideal;
    }

    let template = &mono[ref_pos..ref_pos + corr_len];
    let template_energy: f64 = template.iter().map(|&v| (v as f64) * (v as f64)).sum();
    if template_energy < 1e-12 {
        return clamped_ideal;
    }

    let delta_denom = delta_max.max(1) as f64;
    let mut best_coarse = clamped_ideal;
    let mut best_score = f64::NEG_INFINITY;

    let mut m = search_lo;
    while m <= search_hi {
        let cand = &mono[m..m + corr_len];
        let ncc = normalized_cross_correlation(template, template_energy, cand);
        let dist_penalty = 1e-3 * ((m as isize - ideal_ana).abs() as f64 / delta_denom);
        let score = ncc - dist_penalty;
        if score > best_score {
            best_score = score;
            best_coarse = m;
        }
        m += 2;
    }

    let fine_lo = best_coarse.saturating_sub(2).max(search_lo);
    let fine_hi = (best_coarse + 2).min(search_hi);
    let mut best_m = best_coarse;
    for m in fine_lo..=fine_hi {
        let cand = &mono[m..m + corr_len];
        let ncc = normalized_cross_correlation(template, template_energy, cand);
        let dist_penalty = 1e-3 * ((m as isize - ideal_ana).abs() as f64 / delta_denom);
        let score = ncc - dist_penalty;
        if score > best_score {
            best_score = score;
            best_m = m;
        }
    }

    best_m
}

#[inline]
fn normalized_cross_correlation(template: &[f32], template_energy: f64, cand: &[f32]) -> f64 {
    let mut dot = 0.0f64;
    let mut cand_energy = 0.0f64;
    for (&t, &c) in template.iter().zip(cand.iter()) {
        let tf = t as f64;
        let cf = c as f64;
        dot += tf * cf;
        cand_energy += cf * cf;
    }
    let denom = (template_energy * cand_energy).sqrt();
    if denom < 1e-12 { 0.0 } else { dot / denom }
}

#[allow(clippy::too_many_arguments)]
fn accumulate_grain(
    in_samples: &[f32],
    ch_count: usize,
    in_frames: usize,
    in_start: usize,
    out_acc: &mut [f64],
    weight_acc: &mut [f64],
    out_frames: usize,
    out_start: usize,
    hann: &[f32],
) {
    let copy_len = hann
        .len()
        .min(in_frames.saturating_sub(in_start))
        .min(out_frames.saturating_sub(out_start));
    for j in 0..copy_len {
        let w = hann[j] as f64;
        let in_base = (in_start + j) * ch_count;
        let out_base = (out_start + j) * ch_count;
        for ch in 0..ch_count {
            out_acc[out_base + ch] += (in_samples[in_base + ch] as f64) * w;
        }
        weight_acc[out_start + j] += w;
    }
}

fn normalize_overlap_add(
    sample_rate: u32,
    channels: u16,
    out_frames: usize,
    out_acc: &[f64],
    weight_acc: &[f64],
) -> Result<AudioBuffer, MediaError> {
    let ch_count = channels as usize;
    let mut out_samples = Vec::with_capacity(out_frames * ch_count);
    for (f, &w) in weight_acc.iter().enumerate().take(out_frames) {
        let base = f * ch_count;
        if w > 1e-12 {
            let inv_w = 1.0 / w;
            for ch in 0..ch_count {
                out_samples.push((out_acc[base + ch] * inv_w) as f32);
            }
        } else {
            out_samples.resize(out_samples.len() + ch_count, 0.0);
        }
    }
    AudioBuffer::new(sample_rate, channels, out_samples)
}

fn estimate_bpm_from_beats(beats: &[Flicks]) -> f64 {
    if beats.len() < 2 {
        return 0.0;
    }
    let mut all_ibis: Vec<f64> = beats
        .windows(2)
        .filter_map(|w| {
            let dt = (w[1].0 - w[0].0) as f64 / FLICKS_PER_SECOND as f64;
            if dt > 0.0 { Some(dt) } else { None }
        })
        .collect();
    if all_ibis.is_empty() {
        return 0.0;
    }

    let mut plausible: Vec<f64> = all_ibis
        .iter()
        .copied()
        .filter(|&dt| (0.15..=2.5).contains(&dt))
        .collect();
    let pool = if plausible.is_empty() {
        &mut all_ibis
    } else {
        &mut plausible
    };
    pool.sort_by(|a, b| a.total_cmp(b));
    let median = pool[pool.len() / 2];

    let mut sum = 0.0f64;
    let mut count = 0usize;
    for &dt in pool.iter() {
        if (dt - median).abs() <= median * 0.15 {
            sum += dt;
            count += 1;
        }
    }
    let avg_ibi = if count > 0 {
        sum / count as f64
    } else {
        median
    };
    if avg_ibi > 0.0 { 60.0 / avg_ibi } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_rms_envelope_handles_silence_channels_and_clamping() {
        let window_ticks = Flicks(FLICKS_PER_SECOND / 100); // 10 ms -> 480 frames @ 48 kHz
        let frames_per_window = 480usize;

        // Stereo buffer with 4 windows:
        // W0: silent -> 0.0
        // W1: L=+0.5, R=-0.5 -> RMS = 0.5
        // W2: L=+0.8, R=0.0 -> RMS = sqrt(0.64 / 2) = sqrt(0.32)
        // W3: L=+2.0, R=+2.0 -> raw RMS = 2.0 -> clamped to 1.0
        let mut samples = Vec::with_capacity(frames_per_window * 4 * 2);
        for _ in 0..frames_per_window {
            samples.extend_from_slice(&[0.0, 0.0]);
        }
        for _ in 0..frames_per_window {
            samples.extend_from_slice(&[0.5, -0.5]);
        }
        for _ in 0..frames_per_window {
            samples.extend_from_slice(&[0.8, 0.0]);
        }
        for _ in 0..frames_per_window {
            samples.extend_from_slice(&[2.0, 2.0]);
        }

        let buf = AudioBuffer::new(48_000, 2, samples).unwrap();
        let env = buf.compute_rms_envelope(window_ticks).unwrap();
        assert_eq!(env.len(), 4);
        assert_eq!(env[0], 0.0);
        assert!((env[1] - 0.5).abs() < 1e-6, "env[1]={}", env[1]);
        let expected_w2 = (0.32f32).sqrt();
        assert!(
            (env[2] - expected_w2).abs() < 1e-6,
            "env[2]={} expected={expected_w2}",
            env[2]
        );
        assert_eq!(env[3], 1.0);

        // Empty buffer yields empty envelope
        let empty = AudioBuffer::silence(48_000, 2, 0).unwrap();
        assert!(empty.compute_rms_envelope(window_ticks).unwrap().is_empty());

        // Non-positive window_ticks returns InvalidAudioFormat
        assert!(matches!(
            buf.compute_rms_envelope(Flicks::ZERO),
            Err(MediaError::InvalidAudioFormat(_))
        ));
    }

    #[test]
    fn mix_in_with_constant_curve_is_bit_identical_to_mix_in() {
        // 44.1 kHz stereo source forces the resample path; the offset exercises partial mixing.
        let src_samples: Vec<f32> = (0..4_410 * 2)
            .map(|i| (((i * 37) % 101) as f32 - 50.0) / 83.0)
            .collect();
        let source = AudioBuffer::new(44_100, 2, src_samples).unwrap();
        let offset = Flicks(FLICKS_PER_SECOND / 5);

        let mut expected = AudioBuffer::silence(48_000, 2, 24_000).unwrap();
        expected.mix_in(&source, offset, 0.0).unwrap();

        let mut actual = AudioBuffer::silence(48_000, 2, 24_000).unwrap();
        actual
            .mix_in_with_gain_curve(&source, offset, 0.0, |_| 1.0)
            .unwrap();

        assert_eq!(actual.samples.len(), expected.samples.len());
        for (idx, (a, e)) in actual.samples.iter().zip(&expected.samples).enumerate() {
            assert_eq!(a.to_bits(), e.to_bits(), "sample {idx} differs: {a} vs {e}");
        }

        // The constant-curve path also stays bit-identical for a non-unity base gain.
        let mut expected_6db = AudioBuffer::silence(48_000, 2, 24_000).unwrap();
        expected_6db.mix_in(&source, offset, -6.0).unwrap();
        let mut actual_6db = AudioBuffer::silence(48_000, 2, 24_000).unwrap();
        actual_6db
            .mix_in_with_gain_curve(&source, offset, -6.0, |_| 1.0)
            .unwrap();
        assert_eq!(actual_6db.to_f32le_bytes(), expected_6db.to_f32le_bytes());

        // Independent oracle: at 0 dB the first mixed frame equals the resampled source frame.
        let resampled = source.resample_to(48_000).unwrap();
        let start_frame = 9_600usize; // 0.2 s @ 48 kHz
        assert_eq!(
            actual.samples[start_frame * 2].to_bits(),
            resampled.samples[0].to_bits()
        );
        assert_eq!(
            actual.samples[start_frame * 2 + 1].to_bits(),
            resampled.samples[1].to_bits()
        );
    }

    #[test]
    fn mix_in_with_gain_curve_applies_linear_fade_in() {
        let frames = 480usize;
        let source = AudioBuffer::new(48_000, 1, vec![1.0; frames]).unwrap();
        let mut out = AudioBuffer::silence(48_000, 1, frames).unwrap();
        out.mix_in_with_gain_curve(&source, Flicks::ZERO, 0.0, |x| x)
            .unwrap();

        assert_eq!(out.samples[0], 0.0);
        assert!((out.samples[frames / 4] - 0.25).abs() < 0.01);
        assert!((out.samples[frames / 2] - 0.5).abs() < 0.01);
        assert!((out.samples[frames - 1] - 1.0).abs() < 1e-6);
        assert!(out.samples.windows(2).all(|w| w[1] >= w[0]));
    }

    #[test]
    fn mix_in_with_gain_curve_applies_linear_fade_out() {
        let frames = 480usize;
        let source = AudioBuffer::new(48_000, 1, vec![1.0; frames]).unwrap();
        let mut out = AudioBuffer::silence(48_000, 1, frames).unwrap();
        out.mix_in_with_gain_curve(&source, Flicks::ZERO, 0.0, |x| 1.0 - x)
            .unwrap();

        assert!((out.samples[0] - 1.0).abs() < 1e-6);
        assert!((out.samples[frames / 2] - 0.5).abs() < 0.01);
        assert_eq!(out.samples[frames - 1], 0.0);
        assert!(out.samples.windows(2).all(|w| w[1] <= w[0]));
    }

    #[test]
    fn mix_in_with_gain_curve_supports_offset_and_resampling() {
        let source = AudioBuffer::new(44_100, 1, vec![1.0; 44_100]).unwrap();
        let offset = Flicks(FLICKS_PER_SECOND / 2); // 0.5 s
        let mut out = AudioBuffer::silence(48_000, 2, 72_000).unwrap();
        out.mix_in_with_gain_curve(&source, offset, -6.0, |x| x)
            .unwrap();

        let start = 24_000usize; // 0.5 s @ 48 kHz
        assert!(out.samples[..start * 2].iter().all(|&s| s == 0.0));
        assert_eq!(out.samples[start * 2], 0.0);

        let base_gain = 10.0f32.powf(-6.0 / 20.0);
        let mid = start + 24_000;
        let expected_mid = base_gain * (24_000.0f32 / 47_999.0);
        assert!((out.samples[mid * 2] - expected_mid).abs() < 1e-6);
        // Mono source duplicated into both stereo channels.
        assert_eq!(out.samples[mid * 2], out.samples[mid * 2 + 1]);

        // The resampled tail approaches the full curve value at the end of the source.
        let tail = start + 47_998;
        assert!((out.samples[tail * 2] - base_gain).abs() < 1e-4);
        assert!(out.samples.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn mix_in_with_gain_curve_preserves_channel_mismatch_behavior() {
        // Mono -> stereo duplicates the mono frame into both destination channels.
        let mono = AudioBuffer::new(48_000, 1, vec![0.25, -0.5, 0.75]).unwrap();
        let mut mono_to_stereo = AudioBuffer::silence(48_000, 2, 3).unwrap();
        mono_to_stereo
            .mix_in_with_gain_curve(&mono, Flicks::ZERO, 0.0, |_| 1.0)
            .unwrap();
        assert_eq!(
            mono_to_stereo.samples,
            vec![0.25, 0.25, -0.5, -0.5, 0.75, 0.75]
        );

        // Stereo -> mono keeps the first source channel, exactly like `mix_in`.
        let stereo = AudioBuffer::new(48_000, 2, vec![0.1, 0.9, 0.2, 0.8]).unwrap();
        let mut stereo_to_mono = AudioBuffer::silence(48_000, 1, 2).unwrap();
        stereo_to_mono
            .mix_in_with_gain_curve(&stereo, Flicks::ZERO, 0.0, |_| 1.0)
            .unwrap();
        assert_eq!(stereo_to_mono.samples, vec![0.1, 0.2]);

        // Constant-curve output stays bit-identical to `mix_in` in both directions.
        let mut expected_stereo = AudioBuffer::silence(48_000, 2, 3).unwrap();
        expected_stereo.mix_in(&mono, Flicks::ZERO, 0.0).unwrap();
        assert_eq!(
            mono_to_stereo.to_f32le_bytes(),
            expected_stereo.to_f32le_bytes()
        );
        let mut expected_mono = AudioBuffer::silence(48_000, 1, 2).unwrap();
        expected_mono.mix_in(&stereo, Flicks::ZERO, 0.0).unwrap();
        assert_eq!(
            stereo_to_mono.to_f32le_bytes(),
            expected_mono.to_f32le_bytes()
        );
    }

    #[test]
    fn mix_in_with_gain_curve_rejects_invalid_curve_values() {
        let source = AudioBuffer::new(48_000, 1, vec![1.0; 16]).unwrap();

        let mut nan_out = AudioBuffer::silence(48_000, 1, 16).unwrap();
        let nan_err = nan_out
            .mix_in_with_gain_curve(&source, Flicks::ZERO, 0.0, |_| f32::NAN)
            .unwrap_err();
        assert!(matches!(nan_err, MediaError::InvalidAudioFormat(_)));
        assert!(nan_out.samples.iter().all(|&s| s == 0.0));

        let mut negative_out = AudioBuffer::silence(48_000, 1, 16).unwrap();
        let negative_err = negative_out
            .mix_in_with_gain_curve(&source, Flicks::ZERO, 0.0, |_| -0.5)
            .unwrap_err();
        assert!(matches!(negative_err, MediaError::InvalidAudioFormat(_)));

        // A finite but absurdly large gain_db overflows the f32 multiplier and is rejected.
        let mut overflow_out = AudioBuffer::silence(48_000, 1, 16).unwrap();
        let overflow_err = overflow_out
            .mix_in_with_gain_curve(&source, Flicks::ZERO, f32::MAX, |_| 1.0)
            .unwrap_err();
        assert!(matches!(overflow_err, MediaError::InvalidAudioFormat(_)));
    }

    #[test]
    fn mix_in_with_gain_curve_rejects_non_finite_gain_db() {
        let source = AudioBuffer::new(48_000, 1, vec![1.0; 16]).unwrap();
        let mut out = AudioBuffer::silence(48_000, 1, 16).unwrap();
        let err = out
            .mix_in_with_gain_curve(&source, Flicks::ZERO, f32::NEG_INFINITY, |_| 1.0)
            .unwrap_err();
        assert!(matches!(err, MediaError::InvalidAudioFormat(_)));
        assert!(out.samples.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn soft_limiter_caps_peaks_smoothly_and_preserves_quiet_signal() {
        let ceiling = 0.90f32;
        let mut buf =
            AudioBuffer::new(48_000, 1, vec![0.0, 0.25, -0.5, 0.9, 1.5, -3.0, 10.0]).unwrap();
        apply_soft_limiter(&mut buf, ceiling);

        // Quiet samples below the knee (0.75 * 0.9 = 0.675) are untouched.
        assert_eq!(buf.samples[0], 0.0);
        assert!((buf.samples[1] - 0.25).abs() < 1e-6);
        assert!((buf.samples[2] + 0.5).abs() < 1e-6);

        // All samples strictly bounded by ceiling, monotonically preserving hot peaks.
        assert!(buf.samples.iter().all(|&s| s.abs() <= ceiling));
        assert!(buf.samples[4] > buf.samples[3]);
        assert!(buf.samples[6] > buf.samples[4]);
        assert!(buf.samples[5] < -0.675 && buf.samples[5] >= -ceiling);
    }

    #[test]
    fn biquad_lowpass_and_highpass_attenuate_expected_bands() {
        let fs = 48_000u32;
        let frames = 4800usize; // 100 ms
        let mut low_sine = Vec::with_capacity(frames * 2);
        let mut high_sine = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            let t = i as f64 / fs as f64;
            let lo = (2.0 * std::f64::consts::PI * 100.0 * t).sin() as f32;
            let hi = (2.0 * std::f64::consts::PI * 6000.0 * t).sin() as f32;
            low_sine.extend_from_slice(&[lo, lo]);
            high_sine.extend_from_slice(&[hi, hi]);
        }

        let rms_steady = |b: &AudioBuffer| -> f32 {
            let start = b.frame_count() / 2;
            let slice = &b.samples[start * 2..];
            (slice.iter().map(|&x| x * x).sum::<f32>() / slice.len() as f32).sqrt()
        };

        // Low-pass at 500 Hz passes 100 Hz and strongly attenuates 6 kHz.
        let mut lp_lo = AudioBuffer::new(fs, 2, low_sine.clone()).unwrap();
        let mut lp_hi = AudioBuffer::new(fs, 2, high_sine.clone()).unwrap();
        let butterworth_q = std::f32::consts::FRAC_1_SQRT_2;
        apply_biquad_filter(&mut lp_lo, BiquadFilterKind::LowPass, 500.0, butterworth_q);
        apply_biquad_filter(&mut lp_hi, BiquadFilterKind::LowPass, 500.0, butterworth_q);
        assert!(rms_steady(&lp_lo) > 0.65);
        assert!(rms_steady(&lp_hi) < 0.02);

        // High-pass at 1000 Hz passes 6 kHz and strongly attenuates 100 Hz.
        let mut hp_lo = AudioBuffer::new(fs, 2, low_sine).unwrap();
        let mut hp_hi = AudioBuffer::new(fs, 2, high_sine).unwrap();
        apply_biquad_filter(&mut hp_lo, BiquadFilterKind::HighPass, 1000.0, butterworth_q);
        apply_biquad_filter(&mut hp_hi, BiquadFilterKind::HighPass, 1000.0, butterworth_q);
        assert!(rms_steady(&hp_lo) < 0.02);
        assert!(rms_steady(&hp_hi) > 0.65);
    }

    #[test]
    fn micro_silence_dips_target_window_without_touching_surroundings() {
        let mut buf = AudioBuffer::new(48_000, 2, vec![1.0; 100 * 2]).unwrap();
        apply_micro_silence(&mut buf, 20, 60, 10, 0.05);

        // Samples outside [20, 60) remain untouched.
        assert!(buf.samples[..20 * 2].iter().all(|&s| s == 1.0));
        assert!(buf.samples[60 * 2..].iter().all(|&s| s == 1.0));

        // Midpoint inside [30, 50) hits floor_gain.
        assert!((buf.samples[40 * 2] - 0.05).abs() < 1e-6);
        assert!((buf.samples[40 * 2 + 1] - 0.05).abs() < 1e-6);

        // Entry ramp decreases monotonically; exit ramp increases monotonically.
        assert!(buf.samples[20 * 2] > buf.samples[25 * 2]);
        assert!(buf.samples[25 * 2] > buf.samples[30 * 2]);
        assert!(buf.samples[50 * 2] < buf.samples[55 * 2]);
        assert!(buf.samples[55 * 2] < buf.samples[59 * 2]);
    }

    #[test]
    fn sidechain_ducking_attenuates_during_trigger_and_recovers_after() {
        let fs = 48_000u32;
        let frames = 24_000usize; // 500 ms
        let mut target = AudioBuffer::new(fs, 2, vec![1.0; frames * 2]).unwrap();

        // Trigger active in [4800, 12000) -> 100 ms .. 250 ms.
        let mut trig_samples = vec![0.0f32; frames];
        for (i, s) in trig_samples.iter_mut().enumerate() {
            if (4_800..12_000).contains(&i) {
                let t = i as f64 / fs as f64;
                *s = (2.0 * std::f64::consts::PI * 200.0 * t).sin() as f32 * 0.9;
            }
        }
        let trigger = AudioBuffer::new(fs, 1, trig_samples).unwrap();

        apply_sidechain_ducking(&mut target, &trigger, 0.3, 0.2, 5.0, 40.0);

        // Before trigger: untouched at 1.0.
        assert!((target.samples[1_000 * 2] - 1.0).abs() < 1e-6);
        // Well inside trigger (200 ms = frame 9600): ducked close to 0.2.
        assert!(target.samples[9_600 * 2] < 0.25);
        assert!(target.samples[9_600 * 2] >= 0.19);
        // Well after release (480 ms = frame 23040): recovered near 1.0.
        assert!(target.samples[23_040 * 2] > 0.95);
    }

    #[test]
    fn stereo_pan_follows_equal_power_law_and_promotes_mono() {
        let mut left = AudioBuffer::new(48_000, 2, vec![1.0, 1.0]).unwrap();
        apply_stereo_pan(&mut left, -1.0);
        assert!((left.samples[0] - 1.0).abs() < 1e-6);
        assert!(left.samples[1].abs() < 1e-6);

        let mut right = AudioBuffer::new(48_000, 2, vec![1.0, 1.0]).unwrap();
        apply_stereo_pan(&mut right, 1.0);
        assert!(right.samples[0].abs() < 1e-6);
        assert!((right.samples[1] - 1.0).abs() < 1e-6);

        let mut center = AudioBuffer::new(48_000, 2, vec![1.0, 1.0]).unwrap();
        apply_stereo_pan(&mut center, 0.0);
        let power = center.samples[0].powi(2) + center.samples[1].powi(2);
        assert!((power - 1.0).abs() < 1e-5);
        assert!((center.samples[0] - center.samples[1]).abs() < 1e-6);

        // Mono buffer promotes cleanly to 2-channel stereo preserving total power.
        let mut mono = AudioBuffer::new(48_000, 1, vec![0.8]).unwrap();
        apply_stereo_pan(&mut mono, 0.35);
        assert_eq!(mono.channels, 2);
        assert_eq!(mono.frame_count(), 1);
        let mono_power = mono.samples[0].powi(2) + mono.samples[1].powi(2);
        assert!((mono_power - 0.8 * 0.8).abs() < 1e-5);
        assert!(mono.samples[1] > mono.samples[0]);
    }
}
