use crate::audio::{AudioBuffer, BiquadFilterKind, apply_biquad_filter};
use crate::error::MediaError;
use crate::frame::{AlphaMode, Rgba8Frame};
use frameshift_time::{
    FLICKS_PER_SECOND, Flicks, audio_samples_to_ticks, ticks_to_audio_samples_nearest,
};
use serde::{Deserialize, Serialize};
use std::f32::consts::FRAC_1_SQRT_2;

/// Default frame count per base-level (`L0`) bucket in [`WaveformPyramid`].
///
/// At `48_000 Hz`, 256 frames corresponds to `5.33 ms`, yielding ~`1.8 MB` total pyramid
/// memory for a 5-minute stereo audio track.
pub const DEFAULT_WAVEFORM_BASE_BUCKET_FRAMES: usize = 256;

/// Channel layout when rendering multi-channel [`AudioBuffer`] waveforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaveformChannelLayout {
    /// Mixes all channels into a single centered waveform plot.
    #[default]
    MonoMix,
    /// Renders channels stacked vertically (e.g., `L` top, `R` bottom for stereo).
    StereoSplit,
}

/// Visual coloring style for rendered waveform envelopes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaveformStyle {
    /// Classic two-layer envelope: translucent min/max peak outer band + bright RMS inner core.
    #[default]
    DualEnvelope,
    /// 3-band RBJ biquad crossover spectral RGB coloring (`Low=Red`, `Mid=Green`, `High=Cyan`).
    SpectralRgb,
}

/// Amplitude vertical mapping scale.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaveformAmplitudeScale {
    /// Linear amplitude mapping in `[-1.0, 1.0]`.
    #[default]
    Linear,
    /// Logarithmic decibel mapping from `floor_db` (e.g. `-60.0`) up to `0 dB`.
    Decibel { floor_db: f32 },
}

/// Category of an annotation marker overlaid on a rendered waveform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaveformMarkerKind {
    Beat,
    Downbeat,
    EventAnchor,
    CuePeak,
    Playhead,
}

/// Annotated timeline marker drawn on top of the waveform plot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaveformMarker {
    pub tick: Flicks,
    pub label: Option<String>,
    pub kind: WaveformMarkerKind,
}

/// Aggregated summary for a single horizontal pixel column or pyramid bucket.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct WaveformColumn {
    pub min: f32,
    pub max: f32,
    pub rms: f32,
    /// Per-band RMS energy `(Low 20-250 Hz, Mid 250-2500 Hz, High 2500+ Hz)`.
    pub band_energy: [f32; 3],
}

/// Multi-resolution power-of-4 peak & RMS pyramid for fast `O(width)` zoom/pan rendering.
#[derive(Debug, Clone, PartialEq)]
pub struct WaveformPyramid {
    pub sample_rate: u32,
    pub channels: u16,
    pub total_frames: usize,
    pub base_bucket_frames: usize,
    /// Per-channel levels: `levels[channel][level_idx][bucket_idx]`.
    /// Level 0 has bucket size `base_bucket_frames`, and each subsequent level
    /// aggregates 4 child buckets (`4x`, `16x`, `64x`, ...).
    pub levels: Vec<Vec<Vec<WaveformColumn>>>,
}

impl WaveformPyramid {
    /// Builds a [`WaveformPyramid`] with [`DEFAULT_WAVEFORM_BASE_BUCKET_FRAMES`] (`256`).
    ///
    /// When `compute_spectral` is `true`, runs zero-crate 2nd-order RBJ biquad crossover
    /// filters (`250 Hz` and `2500 Hz`) via [`apply_biquad_filter`] to populate `band_energy`.
    pub fn build(buffer: &AudioBuffer, compute_spectral: bool) -> Result<Self, MediaError> {
        Self::build_with_bucket_size(buffer, DEFAULT_WAVEFORM_BASE_BUCKET_FRAMES, compute_spectral)
    }

    /// Builds a [`WaveformPyramid`] with a custom `base_bucket_frames` (`>= 1`).
    pub fn build_with_bucket_size(
        buffer: &AudioBuffer,
        base_bucket_frames: usize,
        compute_spectral: bool,
    ) -> Result<Self, MediaError> {
        let base_bucket_frames = base_bucket_frames.max(1);
        let total_frames = buffer.frame_count();
        let ch_count = buffer.channels as usize;
        if total_frames == 0 {
            return Err(MediaError::EmptyMediaStream(
                "cannot build waveform pyramid from empty audio buffer".into(),
            ));
        }

        let spectral_bands = if compute_spectral {
            let mut low = buffer.clone();
            apply_biquad_filter(&mut low, BiquadFilterKind::LowPass, 250.0, FRAC_1_SQRT_2);

            let mut mid = buffer.clone();
            apply_biquad_filter(&mut mid, BiquadFilterKind::HighPass, 250.0, FRAC_1_SQRT_2);
            apply_biquad_filter(&mut mid, BiquadFilterKind::LowPass, 2500.0, FRAC_1_SQRT_2);

            let mut high = buffer.clone();
            apply_biquad_filter(
                &mut high,
                BiquadFilterKind::HighPass,
                2500.0,
                FRAC_1_SQRT_2,
            );
            Some((low, mid, high))
        } else {
            None
        };

        let l0_len = total_frames.div_ceil(base_bucket_frames);
        let mut levels = Vec::with_capacity(ch_count);

        for ch in 0..ch_count {
            let mut l0 = Vec::with_capacity(l0_len);
            for b in 0..l0_len {
                let start_f = b * base_bucket_frames;
                let end_f = ((b + 1) * base_bucket_frames).min(total_frames);
                let count = (end_f - start_f).max(1) as f64;

                let mut min_v = f32::INFINITY;
                let mut max_v = f32::NEG_INFINITY;
                let mut sum_sq = 0.0f64;
                let mut lo_sq = 0.0f64;
                let mut mid_sq = 0.0f64;
                let mut hi_sq = 0.0f64;

                for f in start_f..end_f {
                    let idx = f * ch_count + ch;
                    let s = sanitize_sample(buffer.samples[idx]);
                    if s < min_v {
                        min_v = s;
                    }
                    if s > max_v {
                        max_v = s;
                    }
                    sum_sq += (s as f64) * (s as f64);

                    if let Some((ref lo_b, ref mid_b, ref hi_b)) = spectral_bands {
                        let sl = sanitize_sample(lo_b.samples[idx]) as f64;
                        let sm = sanitize_sample(mid_b.samples[idx]) as f64;
                        let sh = sanitize_sample(hi_b.samples[idx]) as f64;
                        lo_sq += sl * sl;
                        mid_sq += sm * sm;
                        hi_sq += sh * sh;
                    }
                }

                if !min_v.is_finite() {
                    min_v = 0.0;
                }
                if !max_v.is_finite() {
                    max_v = 0.0;
                }

                let rms = (sum_sq / count).sqrt() as f32;
                let band_energy = if spectral_bands.is_some() {
                    [
                        (lo_sq / count).sqrt() as f32,
                        (mid_sq / count).sqrt() as f32,
                        (hi_sq / count).sqrt() as f32,
                    ]
                } else {
                    [0.0; 3]
                };

                l0.push(WaveformColumn {
                    min: min_v,
                    max: max_v,
                    rms,
                    band_energy,
                });
            }

            let mut ch_levels = vec![l0];
            while ch_levels.last().is_some_and(|lvl| lvl.len() > 1) {
                let prev = ch_levels.last().expect("non-empty pyramid levels");
                let mut next = Vec::with_capacity(prev.len().div_ceil(4));
                for chunk in prev.chunks(4) {
                    next.push(merge_columns(chunk));
                }
                ch_levels.push(next);
            }
            levels.push(ch_levels);
        }

        Ok(Self {
            sample_rate: buffer.sample_rate,
            channels: buffer.channels,
            total_frames,
            base_bucket_frames,
            levels,
        })
    }

    /// Returns `true` if spectral band energies were populated during construction.
    pub fn has_spectral_data(&self) -> bool {
        self.levels
            .first()
            .and_then(|lvls| lvls.first())
            .is_some_and(|l0| {
                l0.iter()
                    .any(|c| c.band_energy[0] > 0.0 || c.band_energy[1] > 0.0 || c.band_energy[2] > 0.0)
            })
    }

    /// Exact duration of the underlying audio stream in [`Flicks`].
    pub fn duration_ticks(&self) -> Result<Flicks, MediaError> {
        let frames_i64 = i64::try_from(self.total_frames)
            .map_err(|_| MediaError::InvalidAudioFormat("frame count exceeds i64".into()))?;
        Ok(audio_samples_to_ticks(frames_i64, self.sample_rate)?)
    }

    /// Samples `column_count` summarized columns across `[start_frame, end_frame)` using the
    /// coarsest appropriate pyramid level for `O(column_count)` rendering.
    pub fn sample_columns(
        &self,
        channel_idx: Option<usize>,
        start_frame: usize,
        end_frame: usize,
        column_count: usize,
    ) -> Vec<WaveformColumn> {
        self.sample_columns_with_buffer(None, channel_idx, start_frame, end_frame, column_count)
    }

    /// Samples `column_count` columns across `[start_frame, end_frame)`, falling back to exact
    /// sample scanning on `raw_buffer` when `frames_per_px < base_bucket_frames`.
    pub fn sample_columns_with_buffer(
        &self,
        raw_buffer: Option<&AudioBuffer>,
        channel_idx: Option<usize>,
        start_frame: usize,
        end_frame: usize,
        column_count: usize,
    ) -> Vec<WaveformColumn> {
        if column_count == 0 {
            return Vec::new();
        }
        let sf = start_frame.min(self.total_frames);
        let ef = end_frame.min(self.total_frames);
        if sf >= ef || self.levels.is_empty() {
            return vec![WaveformColumn::default(); column_count];
        }

        let span = ef - sf;
        let frames_per_px = span as f64 / column_count as f64;
        let mut out = Vec::with_capacity(column_count);

        // If raw buffer is provided and zoomed inside a single base bucket, compute exact PCM peaks.
        if let Some(buf) = raw_buffer
            && frames_per_px < self.base_bucket_frames as f64
            && buf.frame_count() >= ef
            && buf.channels == self.channels
        {
            let ch_count = buf.channels as usize;
            let l0_buckets = &self.levels[0][0];
            for col in 0..column_count {
                let f0 = sf + (col * span) / column_count;
                let f1 = (sf + ((col + 1) * span) / column_count)
                    .max(f0 + 1)
                    .min(ef);
                let mut col_val =
                    sample_raw_range_column(buf, ch_count, channel_idx, f0, f1);
                // Inherit spectral tint from the overlapping L0 bucket(s) if available.
                let b0 = (f0 / self.base_bucket_frames).min(l0_buckets.len().saturating_sub(1));
                let b1 = ((f1.saturating_sub(1)) / self.base_bucket_frames)
                    .min(l0_buckets.len().saturating_sub(1));
                col_val.band_energy =
                    self.aggregate_band_energy(channel_idx, 0, b0, b1);
                out.push(col_val);
            }
            return out;
        }

        // Choose coarsest level whose bucket size is <= frames_per_px / 2.
        let target_bucket = (frames_per_px * 0.5).floor().max(1.0) as usize;
        let num_levels = self.levels[0].len();
        let mut level_idx = 0usize;
        let mut bucket_frames = self.base_bucket_frames;
        while level_idx + 1 < num_levels && bucket_frames * 4 <= target_bucket {
            level_idx += 1;
            bucket_frames *= 4;
        }

        for col in 0..column_count {
            let f0 = sf + (col * span) / column_count;
            let f1 = (sf + ((col + 1) * span) / column_count)
                .max(f0 + 1)
                .min(ef);
            let b0 = f0 / bucket_frames;
            let b1 = (f1.saturating_sub(1)) / bucket_frames;
            out.push(self.aggregate_range(channel_idx, level_idx, b0, b1));
        }

        out
    }

    fn aggregate_range(
        &self,
        channel_idx: Option<usize>,
        level_idx: usize,
        b0: usize,
        b1: usize,
    ) -> WaveformColumn {
        match channel_idx {
            Some(ch) => {
                let Some(ch_levels) = self.levels.get(ch) else {
                    return WaveformColumn::default();
                };
                let lvl = &ch_levels[level_idx.min(ch_levels.len() - 1)];
                if lvl.is_empty() {
                    return WaveformColumn::default();
                }
                let lo = b0.min(lvl.len() - 1);
                let hi = b1.min(lvl.len() - 1).max(lo);
                merge_columns(&lvl[lo..=hi])
            }
            None => {
                let mut per_ch = Vec::with_capacity(self.levels.len());
                for ch_levels in &self.levels {
                    let lvl = &ch_levels[level_idx.min(ch_levels.len() - 1)];
                    if !lvl.is_empty() {
                        let lo = b0.min(lvl.len() - 1);
                        let hi = b1.min(lvl.len() - 1).max(lo);
                        per_ch.push(merge_columns(&lvl[lo..=hi]));
                    }
                }
                if per_ch.is_empty() {
                    WaveformColumn::default()
                } else {
                    merge_columns(&per_ch)
                }
            }
        }
    }

    fn aggregate_band_energy(
        &self,
        channel_idx: Option<usize>,
        level_idx: usize,
        b0: usize,
        b1: usize,
    ) -> [f32; 3] {
        self.aggregate_range(channel_idx, level_idx, b0, b1).band_energy
    }
}

/// Complete rendering options for [`render_waveform_rgba`] and [`render_waveform_png_bytes`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WaveformRenderOptions {
    pub width: u32,
    pub height: u32,
    /// Optional `[start_tick, end_tick)` zoom window in Flicks; `None` renders full buffer.
    pub range_ticks: Option<(Flicks, Flicks)>,
    pub style: WaveformStyle,
    pub channel_layout: WaveformChannelLayout,
    pub amplitude_scale: WaveformAmplitudeScale,
    pub normalize_to_peak: bool,
    pub show_ruler: bool,
    pub show_zero_line: bool,
    pub background_rgba: [u8; 4],
    pub peak_rgba: [u8; 4],
    pub rms_rgba: [u8; 4],
    pub beat_ticks: Vec<Flicks>,
    pub downbeat_ticks: Vec<Flicks>,
    pub cue_ticks: Vec<Flicks>,
    pub markers: Vec<WaveformMarker>,
    pub playhead_tick: Option<Flicks>,
    pub title: Option<String>,
    pub revision_id: Option<u64>,
    pub bpm: Option<f64>,
}

impl Default for WaveformRenderOptions {
    fn default() -> Self {
        Self {
            width: 1600,
            height: 360,
            range_ticks: None,
            style: WaveformStyle::DualEnvelope,
            channel_layout: WaveformChannelLayout::MonoMix,
            amplitude_scale: WaveformAmplitudeScale::Linear,
            normalize_to_peak: false,
            show_ruler: true,
            show_zero_line: true,
            background_rgba: [12, 14, 20, 255],
            peak_rgba: [16, 185, 129, 135],
            rms_rgba: [110, 231, 183, 240],
            beat_ticks: Vec::new(),
            downbeat_ticks: Vec::new(),
            cue_ticks: Vec::new(),
            markers: Vec::new(),
            playhead_tick: None,
            title: None,
            revision_id: None,
            bpm: None,
        }
    }
}

impl WaveformRenderOptions {
    /// Creates default render options for the given target image dimensions.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            ..Self::default()
        }
    }
}

/// Stateless CPU rasterizer turning [`AudioBuffer`] or [`WaveformPyramid`] into RGBA8/PNG images.
pub struct WaveformRenderer;

/// Renders `buffer` into an RGBA8 [`Rgba8Frame`] according to `options`.
pub fn render_waveform_rgba(
    buffer: &AudioBuffer,
    options: &WaveformRenderOptions,
) -> Result<Rgba8Frame, MediaError> {
    WaveformRenderer::render_rgba8(buffer, options)
}

/// Renders `buffer` into encoded PNG bytes in memory according to `options`.
pub fn render_waveform_png_bytes(
    buffer: &AudioBuffer,
    options: &WaveformRenderOptions,
) -> Result<Vec<u8>, MediaError> {
    let frame = WaveformRenderer::render_rgba8(buffer, options)?;
    frame.encode_png_bytes()
}

impl WaveformRenderer {
    /// Builds a pyramid (with spectral bands only when [`WaveformStyle::SpectralRgb`] is active)
    /// and renders an [`Rgba8Frame`].
    pub fn render_rgba8(
        buffer: &AudioBuffer,
        options: &WaveformRenderOptions,
    ) -> Result<Rgba8Frame, MediaError> {
        let compute_spectral = options.style == WaveformStyle::SpectralRgb;
        let pyramid = WaveformPyramid::build(buffer, compute_spectral)?;
        Self::render_from_pyramid(&pyramid, Some(buffer), options)
    }

    /// Renders an [`Rgba8Frame`] from a pre-built [`WaveformPyramid`], optionally using
    /// `raw_fallback` for sub-sample polyline rendering at extreme zoom-in (`frames_per_px < 1.0`).
    pub fn render_from_pyramid(
        pyramid: &WaveformPyramid,
        raw_fallback: Option<&AudioBuffer>,
        options: &WaveformRenderOptions,
    ) -> Result<Rgba8Frame, MediaError> {
        if options.width == 0 || options.height == 0 {
            return Err(MediaError::InvalidDimensions {
                width: options.width,
                height: options.height,
            });
        }
        if pyramid.total_frames == 0 {
            return Err(MediaError::EmptyMediaStream(
                "cannot render waveform from empty audio stream".into(),
            ));
        }

        let total_duration = pyramid.duration_ticks()?;
        let (start_tick, end_tick) = match options.range_ticks {
            Some((s, e)) => {
                if s >= e {
                    return Err(MediaError::InvalidAudioFormat(format!(
                        "invalid waveform range_ticks: start ({}) must be < end ({})",
                        s.0, e.0
                    )));
                }
                (Flicks(s.0.max(0)), Flicks(e.0.max(s.0 + 1)))
            }
            None => (Flicks::ZERO, Flicks(total_duration.0.max(1))),
        };

        let start_frame = (ticks_to_audio_samples_nearest(start_tick, pyramid.sample_rate)?
            .max(0) as usize)
            .min(pyramid.total_frames.saturating_sub(1));
        let end_frame = (ticks_to_audio_samples_nearest(end_tick, pyramid.sample_rate)?.max(0)
            as usize)
            .clamp(start_frame + 1, pyramid.total_frames);

        let w = options.width as i32;
        let h = options.height as i32;
        let mut canvas = WaveformCanvas::new(options.width, options.height, options.background_rgba);

        let has_ruler = options.show_ruler && h >= 56 && w >= 120;
        let header_h = if has_ruler { 20 } else { 0 };
        let ruler_h = if has_ruler { 18 } else { 0 };
        let pad_x = if has_ruler { 10 } else { 0 };
        let pad_bottom = if has_ruler { 8 } else { 0 };

        let plot_x = pad_x;
        let plot_y = header_h + ruler_h + if has_ruler { 4 } else { 0 };
        let plot_w = (w - pad_x * 2).max(1);
        let plot_h = (h - plot_y - pad_bottom).max(1);

        if has_ruler {
            canvas.fill_rect(0, 0, w, header_h, [20, 24, 34, 255]);
            canvas.fill_rect(0, header_h - 1, w, 1, [44, 52, 70, 255]);
            canvas.fill_rect(plot_x, header_h, plot_w, ruler_h, [15, 18, 26, 255]);
            canvas.stroke_rect(plot_x, plot_y, plot_w, plot_h, [44, 52, 70, 255]);

            let header_text = build_header_label(options, start_tick, end_tick);
            canvas.draw_text(10, 6, &header_text, 1, [232, 238, 248, 255]);
            draw_time_ruler(
                &mut canvas,
                plot_x,
                header_h,
                plot_w,
                ruler_h,
                start_tick,
                end_tick,
            );
        }

        // Compute normalization gain if requested.
        let mut normalize_gain = 1.0f32;
        if options.normalize_to_peak {
            let probe_cols =
                pyramid.sample_columns(None, start_frame, end_frame, plot_w as usize);
            let max_peak = probe_cols
                .iter()
                .map(|c| c.min.abs().max(c.max.abs()))
                .fold(0.0f32, f32::max);
            if max_peak > 1e-6 {
                normalize_gain = 1.0 / max_peak;
            }
        }

        let span_frames = end_frame - start_frame;
        let frames_per_px = span_frames as f64 / plot_w as f64;

        struct ChannelPlotRect<'a> {
            channel_idx: Option<usize>,
            label: Option<&'a str>,
            rect: (i32, i32, i32, i32),
        }

        let channel_rects: Vec<ChannelPlotRect<'_>> = match options.channel_layout {
            WaveformChannelLayout::MonoMix => {
                vec![ChannelPlotRect {
                    channel_idx: None,
                    label: None,
                    rect: (plot_x, plot_y, plot_w, plot_h),
                }]
            }
            WaveformChannelLayout::StereoSplit => {
                if pyramid.channels >= 2 && plot_h >= 24 {
                    let gap = 4;
                    let top_h = (plot_h - gap) / 2;
                    let bot_h = (plot_h - gap - top_h).max(1);
                    vec![
                        ChannelPlotRect {
                            channel_idx: Some(0),
                            label: Some("L"),
                            rect: (plot_x, plot_y, plot_w, top_h),
                        },
                        ChannelPlotRect {
                            channel_idx: Some(1),
                            label: Some("R"),
                            rect: (plot_x, plot_y + top_h + gap, plot_w, bot_h),
                        },
                    ]
                } else {
                    vec![ChannelPlotRect {
                        channel_idx: Some(0),
                        label: None,
                        rect: (plot_x, plot_y, plot_w, plot_h),
                    }]
                }
            }
        };

        for slot in &channel_rects {
            draw_db_guides(&mut canvas, slot.rect, options.amplitude_scale);

            if frames_per_px < 1.0
                && let Some(buf) = raw_fallback
                && buf.frame_count() >= end_frame
            {
                let spectral_hint = if options.style == WaveformStyle::SpectralRgb {
                    let cols = pyramid.sample_columns(slot.channel_idx, start_frame, end_frame, 1);
                    cols.first().map(|c| c.band_energy)
                } else {
                    None
                };
                Self::paint_polyline_into_rgba_rect(
                    &mut canvas.data,
                    options.width,
                    options.height,
                    slot.rect,
                    buf,
                    slot.channel_idx,
                    start_frame,
                    end_frame,
                    options.style,
                    options.peak_rgba,
                    options.rms_rgba,
                    options.amplitude_scale,
                    normalize_gain,
                    options.show_zero_line,
                    spectral_hint,
                );
            } else {
                let cols = pyramid.sample_columns_with_buffer(
                    raw_fallback,
                    slot.channel_idx,
                    start_frame,
                    end_frame,
                    slot.rect.2.max(1) as usize,
                );
                Self::paint_into_rgba_rect(
                    &mut canvas.data,
                    options.width,
                    options.height,
                    slot.rect,
                    &cols,
                    options.style,
                    options.peak_rgba,
                    options.rms_rgba,
                    options.amplitude_scale,
                    normalize_gain,
                    options.show_zero_line,
                    None,
                );
            }

            if has_ruler && let Some(lbl) = slot.label {
                canvas.draw_text(
                    slot.rect.0 + 4,
                    slot.rect.1 + 4,
                    lbl,
                    1,
                    [142, 156, 180, 255],
                );
            }
        }

        if has_ruler {
            let db_label = match options.amplitude_scale {
                WaveformAmplitudeScale::Linear => "0DB / -6DB",
                WaveformAmplitudeScale::Decibel { floor_db } => {
                    if (floor_db + 60.0).abs() < 0.5 {
                        "0DB / -60DB"
                    } else {
                        "LOG DB"
                    }
                }
            };
            let lbl_x = (plot_x + plot_w - (db_label.len() as i32) * 6 - 4).max(plot_x + 2);
            canvas.draw_text(lbl_x, plot_y + 4, db_label, 1, [108, 122, 146, 255]);
        }

        // Overlay markers: Beat, Downbeat, EventAnchor, CuePeak, Playhead.
        draw_all_markers(
            &mut canvas,
            (plot_x, plot_y, plot_w, plot_h),
            start_tick,
            end_tick,
            options,
        );

        Rgba8Frame::new(
            options.width,
            options.height,
            start_tick,
            AlphaMode::Straight,
            canvas.data,
        )
    }

    /// Anti-aliased vertical column rasterizer painting pre-sampled [`WaveformColumn`]s directly
    /// into an existing straight-alpha RGBA8 pixel buffer inside `rect = (x, y, w, h)`.
    ///
    /// Designed so GUI panels (such as `snapshot.rs`) and standalone waveform renders share the
    /// exact same sub-pixel coverage rasterizer without allocating intermediate frames.
    #[allow(clippy::too_many_arguments)]
    pub fn paint_into_rgba_rect(
        dst_pixels: &mut [u8],
        canvas_w: u32,
        canvas_h: u32,
        rect: (i32, i32, i32, i32),
        columns: &[WaveformColumn],
        style: WaveformStyle,
        peak_rgba: [u8; 4],
        rms_rgba: [u8; 4],
        amplitude_scale: WaveformAmplitudeScale,
        normalize_gain: f32,
        show_zero_line: bool,
        gain_envelope: Option<&[f32]>,
    ) {
        let (rx, ry, rw, rh) = rect;
        if rw <= 0 || rh <= 0 || columns.is_empty() || canvas_w == 0 || canvas_h == 0 {
            return;
        }
        let expected_len = (canvas_w as usize) * (canvas_h as usize) * 4;
        if dst_pixels.len() < expected_len {
            return;
        }

        let mid_y = ry as f32 + (rh as f32) * 0.5;
        let half_h = ((rh as f32) * 0.5 - 1.0).max(1.0);
        let gain_base = if normalize_gain.is_finite() && normalize_gain > 0.0 {
            normalize_gain
        } else {
            1.0
        };

        let mut prev_peak_range: Option<(f32, f32)> = None;
        let col_count = rw as usize;

        for px_offset in 0..col_count {
            let px = rx + px_offset as i32;
            if px < 0 || px >= canvas_w as i32 {
                continue;
            }

            let col = if columns.len() == col_count {
                columns[px_offset]
            } else {
                let c0 = (px_offset * columns.len()) / col_count;
                let c1 = (((px_offset + 1) * columns.len()) / col_count)
                    .max(c0 + 1)
                    .min(columns.len());
                merge_columns(&columns[c0..c1])
            };

            let env_gain = gain_envelope
                .and_then(|env| {
                    if env.is_empty() {
                        None
                    } else {
                        let idx = (px_offset * env.len()) / col_count;
                        Some(env[idx.min(env.len() - 1)].max(0.0))
                    }
                })
                .unwrap_or(1.0);
            let total_gain = gain_base * env_gain;

            let s_min = scale_signed_amplitude(col.min * total_gain, amplitude_scale);
            let s_max = scale_signed_amplitude(col.max * total_gain, amplitude_scale);
            let s_rms = scale_unsigned_amplitude(col.rms.abs() * total_gain, amplitude_scale);

            let (col_peak_rgba, col_rms_rgba) = match style {
                WaveformStyle::DualEnvelope => (peak_rgba, rms_rgba),
                WaveformStyle::SpectralRgb => spectral_colors_from_bands(col.band_energy),
            };

            // Peak vertical span [y_peak_top, y_peak_bot]
            let mut y_peak_top = mid_y - s_max * half_h;
            let mut y_peak_bot = mid_y - s_min * half_h;
            if y_peak_top > y_peak_bot {
                std::mem::swap(&mut y_peak_top, &mut y_peak_bot);
            }
            if y_peak_bot - y_peak_top < 1.0 {
                let center = 0.5 * (y_peak_top + y_peak_bot);
                y_peak_top = center - 0.5;
                y_peak_bot = center + 0.5;
            }
            if let Some((prev_top, prev_bot)) = prev_peak_range {
                if y_peak_top > prev_bot {
                    y_peak_top = prev_bot;
                } else if y_peak_bot < prev_top {
                    y_peak_bot = prev_top;
                }
            }
            prev_peak_range = Some((y_peak_top, y_peak_bot));

            let clamp_top = ry as f32;
            let clamp_bot = (ry + rh) as f32;
            y_peak_top = y_peak_top.clamp(clamp_top, clamp_bot);
            y_peak_bot = y_peak_bot.clamp(clamp_top, clamp_bot);

            paint_aa_vertical_span(
                dst_pixels,
                canvas_w,
                canvas_h,
                px,
                ry,
                rh,
                y_peak_top,
                y_peak_bot,
                col_peak_rgba,
            );

            // RMS inner core span clamped within peak span
            if s_rms > 1e-5 {
                let mut y_rms_top = (mid_y - s_rms * half_h).max(y_peak_top);
                let mut y_rms_bot = (mid_y + s_rms * half_h).min(y_peak_bot);
                if y_rms_bot > y_rms_top {
                    y_rms_top = y_rms_top.clamp(clamp_top, clamp_bot);
                    y_rms_bot = y_rms_bot.clamp(clamp_top, clamp_bot);
                    paint_aa_vertical_span(
                        dst_pixels,
                        canvas_w,
                        canvas_h,
                        px,
                        ry,
                        rh,
                        y_rms_top,
                        y_rms_bot,
                        col_rms_rgba,
                    );
                }
            }
        }

        if show_zero_line {
            let zy = mid_y.round() as i32;
            if zy >= ry && zy < ry + rh && zy >= 0 && zy < canvas_h as i32 {
                let x0 = rx.max(0);
                let x1 = (rx + rw).min(canvas_w as i32);
                for px in x0..x1 {
                    blend_pixel_raw(dst_pixels, canvas_w, px, zy, [148, 163, 184, 72]);
                }
            }
        }
    }

    /// Extreme zoom-in (`frames_per_px < 1.0`) polyline + sample-dot rasterizer directly from
    /// raw [`AudioBuffer`] samples so zero-crossings remain continuous and crisp.
    #[allow(clippy::too_many_arguments)]
    pub fn paint_polyline_into_rgba_rect(
        dst_pixels: &mut [u8],
        canvas_w: u32,
        canvas_h: u32,
        rect: (i32, i32, i32, i32),
        buffer: &AudioBuffer,
        channel_idx: Option<usize>,
        start_frame: usize,
        end_frame: usize,
        style: WaveformStyle,
        peak_rgba: [u8; 4],
        rms_rgba: [u8; 4],
        amplitude_scale: WaveformAmplitudeScale,
        normalize_gain: f32,
        show_zero_line: bool,
        spectral_hint: Option<[f32; 3]>,
    ) {
        let (rx, ry, rw, rh) = rect;
        if rw <= 0 || rh <= 0 || end_frame <= start_frame || buffer.frame_count() == 0 {
            return;
        }
        let expected_len = (canvas_w as usize) * (canvas_h as usize) * 4;
        if dst_pixels.len() < expected_len {
            return;
        }

        let mid_y = ry as f32 + (rh as f32) * 0.5;
        let half_h = ((rh as f32) * 0.5 - 2.0).max(1.0);
        let span = (end_frame - start_frame) as f64;
        let px_per_frame = rw as f64 / span;

        if show_zero_line {
            let zy = mid_y.round() as i32;
            if zy >= ry && zy < ry + rh && zy >= 0 && zy < canvas_h as i32 {
                let x0 = rx.max(0);
                let x1 = (rx + rw).min(canvas_w as i32);
                for px in x0..x1 {
                    blend_pixel_raw(dst_pixels, canvas_w, px, zy, [148, 163, 184, 72]);
                }
            }
        }

        let (fill_rgba, line_rgba) = match style {
            WaveformStyle::DualEnvelope => (
                [peak_rgba[0], peak_rgba[1], peak_rgba[2], (peak_rgba[3] / 2).max(40)],
                rms_rgba,
            ),
            WaveformStyle::SpectralRgb => {
                let bands = spectral_hint.unwrap_or([0.0, 1.0, 0.0]);
                let (p, r) = spectral_colors_from_bands(bands);
                ([p[0], p[1], p[2], 64], r)
            }
        };

        let sample_lo = start_frame.saturating_sub(1);
        let sample_hi = (end_frame + 1).min(buffer.frame_count());
        let mut pts: Vec<(f32, f32)> = Vec::with_capacity(sample_hi.saturating_sub(sample_lo));

        for f in sample_lo..sample_hi {
            let rel = (f as f64 + 0.5 - start_frame as f64) / span;
            let x = rx as f32 + (rel as f32) * (rw as f32);
            let raw_s = sample_frame_value(buffer, channel_idx, f);
            let scaled = scale_signed_amplitude(raw_s * normalize_gain, amplitude_scale);
            let y = (mid_y - scaled * half_h).clamp(ry as f32, (ry + rh - 1) as f32);
            pts.push((x, y));
        }

        for pair in pts.windows(2) {
            let (x0, y0) = pair[0];
            let (x1, y1) = pair[1];
            draw_aa_segment_clipped(
                dst_pixels,
                canvas_w,
                canvas_h,
                rect,
                x0,
                y0,
                x1,
                y1,
                1.6,
                line_rgba,
            );
            // Subtle vertical stem fill toward center line for readability
            let min_x = x0.max(rx as f32).floor() as i32;
            let max_x = x1.min((rx + rw - 1) as f32).ceil() as i32;
            let dx = (x1 - x0).abs().max(1e-5);
            for px in min_x..=max_x {
                let t = ((px as f32 + 0.5 - x0) / dx).clamp(0.0, 1.0);
                let sy = y0 + (y1 - y0) * t;
                let top = sy.min(mid_y);
                let bot = sy.max(mid_y);
                paint_aa_vertical_span(
                    dst_pixels,
                    canvas_w,
                    canvas_h,
                    px,
                    ry,
                    rh,
                    top,
                    bot,
                    fill_rgba,
                );
            }
        }

        // Draw crisp sample nodes when samples are spaced >= 4 px apart
        if px_per_frame >= 4.0 {
            for &(px, py) in &pts {
                draw_aa_disc_clipped(
                    dst_pixels,
                    canvas_w,
                    canvas_h,
                    rect,
                    px,
                    py,
                    2.1,
                    [232, 238, 248, 245],
                );
            }
        }
    }
}

fn sanitize_sample(v: f32) -> f32 {
    if v.is_finite() { v } else { 0.0 }
}

fn sample_frame_value(buffer: &AudioBuffer, channel_idx: Option<usize>, frame: usize) -> f32 {
    let ch_count = buffer.channels as usize;
    let base = frame * ch_count;
    match channel_idx {
        Some(ch) => sanitize_sample(buffer.samples[base + ch.min(ch_count - 1)]),
        None => {
            let mut sum = 0.0f32;
            for ch in 0..ch_count {
                sum += sanitize_sample(buffer.samples[base + ch]);
            }
            sum / (ch_count as f32)
        }
    }
}

fn sample_raw_range_column(
    buf: &AudioBuffer,
    ch_count: usize,
    channel_idx: Option<usize>,
    f0: usize,
    f1: usize,
) -> WaveformColumn {
    let mut min_v = f32::INFINITY;
    let mut max_v = f32::NEG_INFINITY;
    let mut sum_sq = 0.0f64;
    let count = (f1 - f0).max(1) as f64;
    for f in f0..f1 {
        let s = match channel_idx {
            Some(ch) => sanitize_sample(buf.samples[f * ch_count + ch.min(ch_count - 1)]),
            None => {
                let base = f * ch_count;
                let mut sum = 0.0f32;
                for ch in 0..ch_count {
                    sum += sanitize_sample(buf.samples[base + ch]);
                }
                sum / (ch_count as f32)
            }
        };
        if s < min_v {
            min_v = s;
        }
        if s > max_v {
            max_v = s;
        }
        sum_sq += (s as f64) * (s as f64);
    }
    if !min_v.is_finite() {
        min_v = 0.0;
    }
    if !max_v.is_finite() {
        max_v = 0.0;
    }
    WaveformColumn {
        min: min_v,
        max: max_v,
        rms: (sum_sq / count).sqrt() as f32,
        band_energy: [0.0; 3],
    }
}

fn merge_columns(cols: &[WaveformColumn]) -> WaveformColumn {
    if cols.is_empty() {
        return WaveformColumn::default();
    }
    let mut min_v = f32::INFINITY;
    let mut max_v = f32::NEG_INFINITY;
    let mut rms_sq = 0.0f64;
    let mut b0_sq = 0.0f64;
    let mut b1_sq = 0.0f64;
    let mut b2_sq = 0.0f64;
    for c in cols {
        if c.min < min_v {
            min_v = c.min;
        }
        if c.max > max_v {
            max_v = c.max;
        }
        let r = c.rms as f64;
        rms_sq += r * r;
        b0_sq += (c.band_energy[0] as f64).powi(2);
        b1_sq += (c.band_energy[1] as f64).powi(2);
        b2_sq += (c.band_energy[2] as f64).powi(2);
    }
    let n = cols.len() as f64;
    WaveformColumn {
        min: if min_v.is_finite() { min_v } else { 0.0 },
        max: if max_v.is_finite() { max_v } else { 0.0 },
        rms: (rms_sq / n).sqrt() as f32,
        band_energy: [
            (b0_sq / n).sqrt() as f32,
            (b1_sq / n).sqrt() as f32,
            (b2_sq / n).sqrt() as f32,
        ],
    }
}

fn scale_signed_amplitude(v: f32, scale: WaveformAmplitudeScale) -> f32 {
    if !v.is_finite() {
        return 0.0;
    }
    match scale {
        WaveformAmplitudeScale::Linear => v.clamp(-1.0, 1.0),
        WaveformAmplitudeScale::Decibel { floor_db } => {
            let floor = floor_db.min(-6.0);
            let mag = v.abs();
            if mag <= 1e-6 {
                return 0.0;
            }
            let db = 20.0 * mag.log10();
            let norm = ((db - floor) / -floor).clamp(0.0, 1.0);
            v.signum() * norm
        }
    }
}

fn scale_unsigned_amplitude(v: f32, scale: WaveformAmplitudeScale) -> f32 {
    scale_signed_amplitude(v.abs(), scale).abs()
}

fn spectral_colors_from_bands(band_energy: [f32; 3]) -> ([u8; 4], [u8; 4]) {
    let e_lo = band_energy[0].max(0.0);
    let e_mid = band_energy[1].max(0.0);
    let e_hi = band_energy[2].max(0.0);
    let max_e = e_lo.max(e_mid).max(e_hi);
    if max_e <= 1e-7 {
        return ([96, 112, 136, 108], [142, 160, 184, 220]);
    }
    let w_lo = (e_lo / max_e).clamp(0.0, 1.0).powf(0.65);
    let w_mid = (e_mid / max_e).clamp(0.0, 1.0).powf(0.65);
    let w_hi = (e_hi / max_e).clamp(0.0, 1.0).powf(0.65);
    let sum = (w_lo + w_mid + w_hi).max(1e-6);

    // Low=Red/Rose [244,63,94], Mid=Emerald [34,197,94], High=Cyan [56,189,248]
    let r = ((244.0 * w_lo + 34.0 * w_mid + 56.0 * w_hi) / sum).clamp(0.0, 255.0);
    let g = ((63.0 * w_lo + 197.0 * w_mid + 189.0 * w_hi) / sum).clamp(0.0, 255.0);
    let b = ((94.0 * w_lo + 94.0 * w_mid + 248.0 * w_hi) / sum).clamp(0.0, 255.0);

    let peak_rgba = [r.round() as u8, g.round() as u8, b.round() as u8, 112];
    let rms_rgba = [
        (r + (255.0 - r) * 0.25).round() as u8,
        (g + (255.0 - g) * 0.25).round() as u8,
        (b + (255.0 - b) * 0.25).round() as u8,
        235,
    ];
    (peak_rgba, rms_rgba)
}

#[allow(clippy::too_many_arguments)]
fn paint_aa_vertical_span(
    dst_pixels: &mut [u8],
    canvas_w: u32,
    canvas_h: u32,
    px: i32,
    ry: i32,
    rh: i32,
    y_top: f32,
    y_bot: f32,
    rgba: [u8; 4],
) {
    if px < 0 || px >= canvas_w as i32 || rgba[3] == 0 {
        return;
    }
    let y0 = (y_top.floor() as i32).max(ry).max(0);
    let y1 = (y_bot.ceil() as i32).min(ry + rh - 1).min(canvas_h as i32 - 1);
    if y0 > y1 {
        return;
    }
    for py in y0..=y1 {
        let cov = (y_bot.min(py as f32 + 1.0) - y_top.max(py as f32)).clamp(0.0, 1.0);
        if cov <= 0.001 {
            continue;
        }
        let a = ((rgba[3] as f32) * cov).round().clamp(0.0, 255.0) as u8;
        if a > 0 {
            blend_pixel_raw(dst_pixels, canvas_w, px, py, [rgba[0], rgba[1], rgba[2], a]);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_aa_segment_clipped(
    dst_pixels: &mut [u8],
    canvas_w: u32,
    canvas_h: u32,
    rect: (i32, i32, i32, i32),
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    thickness: f32,
    rgba: [u8; 4],
) {
    let (rx, ry, rw, rh) = rect;
    let half_w = (thickness * 0.5).max(0.5);
    let pad = half_w + 1.0;
    let min_x = (x0.min(x1) - pad).floor() as i32;
    let max_x = (x0.max(x1) + pad).ceil() as i32;
    let min_y = (y0.min(y1) - pad).floor() as i32;
    let max_y = (y0.max(y1) + pad).ceil() as i32;

    let bx0 = min_x.max(rx).max(0);
    let bx1 = max_x.min(rx + rw - 1).min(canvas_w as i32 - 1);
    let by0 = min_y.max(ry).max(0);
    let by1 = max_y.min(ry + rh - 1).min(canvas_h as i32 - 1);
    if bx0 > bx1 || by0 > by1 {
        return;
    }

    let vx = x1 - x0;
    let vy = y1 - y0;
    let len_sq = vx * vx + vy * vy;

    for py in by0..=by1 {
        let cy = py as f32 + 0.5;
        for px in bx0..=bx1 {
            let cx = px as f32 + 0.5;
            let dist = if len_sq <= 1e-6 {
                ((cx - x0).powi(2) + (cy - y0).powi(2)).sqrt()
            } else {
                let t = (((cx - x0) * vx + (cy - y0) * vy) / len_sq).clamp(0.0, 1.0);
                let proj_x = x0 + t * vx;
                let proj_y = y0 + t * vy;
                ((cx - proj_x).powi(2) + (cy - proj_y).powi(2)).sqrt()
            };
            let cov = (half_w + 0.5 - dist).clamp(0.0, 1.0);
            if cov > 0.001 {
                let a = ((rgba[3] as f32) * cov).round().clamp(0.0, 255.0) as u8;
                if a > 0 {
                    blend_pixel_raw(dst_pixels, canvas_w, px, py, [rgba[0], rgba[1], rgba[2], a]);
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_aa_disc_clipped(
    dst_pixels: &mut [u8],
    canvas_w: u32,
    canvas_h: u32,
    rect: (i32, i32, i32, i32),
    cx: f32,
    cy: f32,
    radius: f32,
    rgba: [u8; 4],
) {
    let (rx, ry, rw, rh) = rect;
    let pad = radius + 1.0;
    let bx0 = ((cx - pad).floor() as i32).max(rx).max(0);
    let bx1 = ((cx + pad).ceil() as i32)
        .min(rx + rw - 1)
        .min(canvas_w as i32 - 1);
    let by0 = ((cy - pad).floor() as i32).max(ry).max(0);
    let by1 = ((cy + pad).ceil() as i32)
        .min(ry + rh - 1)
        .min(canvas_h as i32 - 1);
    for py in by0..=by1 {
        let y = py as f32 + 0.5;
        for px in bx0..=bx1 {
            let x = px as f32 + 0.5;
            let dist = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
            let cov = (radius + 0.5 - dist).clamp(0.0, 1.0);
            if cov > 0.001 {
                let a = ((rgba[3] as f32) * cov).round().clamp(0.0, 255.0) as u8;
                if a > 0 {
                    blend_pixel_raw(dst_pixels, canvas_w, px, py, [rgba[0], rgba[1], rgba[2], a]);
                }
            }
        }
    }
}

#[inline]
fn blend_pixel_raw(dst_pixels: &mut [u8], canvas_w: u32, x: i32, y: i32, rgba: [u8; 4]) {
    let idx = ((y as usize) * (canvas_w as usize) + (x as usize)) * 4;
    let a = rgba[3] as u32;
    if a == 0 {
        return;
    }
    if a == 255 {
        dst_pixels[idx..idx + 4].copy_from_slice(&rgba);
        return;
    }
    let inv = 255 - a;
    for (c, &src_ch) in rgba.iter().enumerate().take(3) {
        let dst = dst_pixels[idx + c] as u32;
        let src = src_ch as u32;
        dst_pixels[idx + c] = ((src * a + dst * inv + 127) / 255) as u8;
    }
    let dst_a = dst_pixels[idx + 3] as u32;
    dst_pixels[idx + 3] = (a + ((dst_a * inv + 127) / 255)).min(255) as u8;
}

fn draw_db_guides(
    canvas: &mut WaveformCanvas,
    rect: (i32, i32, i32, i32),
    amplitude_scale: WaveformAmplitudeScale,
) {
    let (rx, ry, rw, rh) = rect;
    let mid_y = ry as f32 + (rh as f32) * 0.5;
    let half_h = ((rh as f32) * 0.5 - 1.0).max(1.0);
    let guide_mag = scale_unsigned_amplitude(0.5, amplitude_scale); // -6 dB
    let top_y = (mid_y - guide_mag * half_h).round() as i32;
    let bot_y = (mid_y + guide_mag * half_h).round() as i32;
    for gy in [top_y, bot_y] {
        if gy > ry && gy < ry + rh - 1 {
            canvas.draw_dashed_hline(rx, rx + rw, gy, [44, 52, 70, 120]);
        }
    }
}

fn build_header_label(
    options: &WaveformRenderOptions,
    start_tick: Flicks,
    end_tick: Flicks,
) -> String {
    let mut parts = Vec::new();
    if let Some(rev) = options.revision_id {
        parts.push(format!("REV #{rev}"));
    } else {
        parts.push("WAVEFORM".to_owned());
    }
    if let Some(ref title) = options.title
        && !title.is_empty()
    {
        parts.push(title.clone());
    }
    if let Some(bpm) = options.bpm
        && bpm > 0.0
    {
        parts.push(format!("BPM {bpm:.1}"));
    }
    let style_str = match options.style {
        WaveformStyle::DualEnvelope => "DUAL",
        WaveformStyle::SpectralRgb => "SPECTRAL",
    };
    parts.push(style_str.to_owned());
    parts.push(format!(
        "[{:.3}S..{:.3}S]",
        start_tick.to_seconds_f64(),
        end_tick.to_seconds_f64()
    ));
    parts.join(" | ")
}

fn draw_time_ruler(
    canvas: &mut WaveformCanvas,
    rx: i32,
    ry: i32,
    rw: i32,
    rh: i32,
    start_tick: Flicks,
    end_tick: Flicks,
) {
    let span_ticks = (end_tick.0 - start_tick.0).max(1);
    let span_sec = span_ticks as f64 / FLICKS_PER_SECOND as f64;
    let px_per_sec = rw as f64 / span_sec.max(1e-9);

    // Choose a clean tick interval in seconds so labels remain >= 56px apart.
    let candidates = [
        0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0,
        60.0, 120.0, 300.0,
    ];
    let step_sec = candidates
        .into_iter()
        .find(|&s| s * px_per_sec >= 58.0)
        .unwrap_or_else(|| (58.0 / px_per_sec.max(1e-6)).ceil());
    let step_ticks = ((step_sec * FLICKS_PER_SECOND as f64).round() as i64).max(1);

    let first_idx = (start_tick.0 + step_ticks - 1) / step_ticks;
    let mut last_right = rx - 10;
    let mut idx = first_idx;
    loop {
        let tick = Flicks(idx.saturating_mul(step_ticks));
        if tick > end_tick {
            break;
        }
        let ratio = (tick.0 - start_tick.0) as f64 / span_ticks as f64;
        let tx = rx + (ratio * (rw as f64)).round() as i32;
        if tx >= rx && tx < rx + rw {
            canvas.draw_vline(tx, ry + rh - 6, ry + rh, [100, 116, 139, 255]);
            let sec = tick.to_seconds_f64();
            let label = if step_sec < 0.05 {
                format!("{:.0}MS", sec * 1000.0)
            } else if step_sec < 1.0 {
                format!("{sec:.2}S")
            } else {
                format!("{sec:.1}S")
            };
            let lw = (label.len() as i32) * 6;
            let lx = (tx + 2).min(rx + rw - lw - 2);
            if lx >= last_right + 8 && lx >= rx {
                canvas.draw_text(lx, ry + 3, &label, 1, [142, 156, 180, 255]);
                last_right = lx + lw;
            }
        }
        idx += 1;
        if idx - first_idx > 512 {
            break;
        }
    }
}

fn draw_all_markers(
    canvas: &mut WaveformCanvas,
    plot_rect: (i32, i32, i32, i32),
    start_tick: Flicks,
    end_tick: Flicks,
    options: &WaveformRenderOptions,
) {
    let (rx, ry, rw, rh) = plot_rect;
    let span = (end_tick.0 - start_tick.0).max(1) as f64;
    let tick_to_x = |t: Flicks| -> Option<i32> {
        if t < start_tick || t > end_tick {
            return None;
        }
        let ratio = (t.0 - start_tick.0) as f64 / span;
        let x = rx + (ratio * ((rw - 1).max(1) as f64)).round() as i32;
        Some(x.clamp(rx, rx + rw - 1))
    };

    // 1. Beats
    for &t in &options.beat_ticks {
        if let Some(x) = tick_to_x(t) {
            canvas.draw_vline(x, ry, ry + rh, [110, 231, 183, 120]);
        }
    }
    // 2. Downbeats
    for &t in &options.downbeat_ticks {
        if let Some(x) = tick_to_x(t) {
            canvas.draw_vline(x, ry, ry + rh, [56, 189, 248, 220]);
            canvas.draw_vline(x + 1, ry, ry + rh, [56, 189, 248, 160]);
            canvas.fill_rect(x - 2, ry, 5, 4, [56, 189, 248, 255]);
        }
    }
    // 3. Cue ticks
    for &t in &options.cue_ticks {
        if let Some(x) = tick_to_x(t) {
            canvas.draw_dashed_vline(x, ry, ry + rh, [250, 204, 21, 220]);
            canvas.draw_diamond(x, ry + 6, 4, [250, 204, 21, 255]);
        }
    }
    // 4. Custom markers
    for m in &options.markers {
        let Some(x) = tick_to_x(m.tick) else {
            continue;
        };
        match m.kind {
            WaveformMarkerKind::Beat => {
                canvas.draw_vline(x, ry, ry + rh, [110, 231, 183, 120]);
            }
            WaveformMarkerKind::Downbeat => {
                canvas.draw_vline(x, ry, ry + rh, [56, 189, 248, 220]);
                canvas.draw_vline(x + 1, ry, ry + rh, [56, 189, 248, 160]);
            }
            WaveformMarkerKind::EventAnchor | WaveformMarkerKind::CuePeak => {
                canvas.draw_dashed_vline(x, ry, ry + rh, [250, 204, 21, 220]);
                canvas.draw_diamond(x, ry + 6, 4, [250, 204, 21, 255]);
                if let Some(ref lbl) = m.label {
                    canvas.draw_text(
                        (x + 6).min(rx + rw - (lbl.len() as i32) * 6 - 2),
                        ry + 4,
                        lbl,
                        1,
                        [250, 204, 21, 245],
                    );
                }
            }
            WaveformMarkerKind::Playhead => {
                canvas.draw_vline(x, ry, ry + rh, [239, 68, 68, 255]);
                canvas.draw_vline(x + 1, ry, ry + rh, [239, 68, 68, 255]);
                canvas.draw_down_triangle(x, ry, 5, 7, [239, 68, 68, 255]);
            }
        }
    }
    // 5. Explicit playhead
    if let Some(t) = options.playhead_tick
        && let Some(x) = tick_to_x(t)
    {
        canvas.draw_vline(x, ry, ry + rh, [239, 68, 68, 255]);
        canvas.draw_vline(x + 1, ry, ry + rh, [239, 68, 68, 255]);
        canvas.draw_down_triangle(x, ry, 5, 7, [239, 68, 68, 255]);
    }
}

struct WaveformCanvas {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl WaveformCanvas {
    fn new(width: u32, height: u32, fill: [u8; 4]) -> Self {
        let count = (width as usize) * (height as usize);
        let mut data = Vec::with_capacity(count * 4);
        for _ in 0..count {
            data.extend_from_slice(&fill);
        }
        Self {
            width,
            height,
            data,
        }
    }

    #[inline]
    fn blend_pixel(&mut self, x: i32, y: i32, rgba: [u8; 4]) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        blend_pixel_raw(&mut self.data, self.width, x, y, rgba);
    }

    fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, rgba: [u8; 4]) {
        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = (x + w).min(self.width as i32);
        let y1 = (y + h).min(self.height as i32);
        for py in y0..y1 {
            for px in x0..x1 {
                self.blend_pixel(px, py, rgba);
            }
        }
    }

    fn stroke_rect(&mut self, x: i32, y: i32, w: i32, h: i32, rgba: [u8; 4]) {
        if w <= 0 || h <= 0 {
            return;
        }
        self.fill_rect(x, y, w, 1, rgba);
        self.fill_rect(x, y + h - 1, w, 1, rgba);
        self.fill_rect(x, y, 1, h, rgba);
        self.fill_rect(x + w - 1, y, 1, h, rgba);
    }

    fn draw_vline(&mut self, x: i32, y0: i32, y1: i32, rgba: [u8; 4]) {
        for y in y0.min(y1)..y0.max(y1) {
            self.blend_pixel(x, y, rgba);
        }
    }

    fn draw_dashed_vline(&mut self, x: i32, y0: i32, y1: i32, rgba: [u8; 4]) {
        let mut y = y0.min(y1);
        let end = y0.max(y1);
        while y < end {
            for dy in 0..4 {
                if y + dy < end {
                    self.blend_pixel(x, y + dy, rgba);
                }
            }
            y += 8;
        }
    }

    fn draw_dashed_hline(&mut self, x0: i32, x1: i32, y: i32, rgba: [u8; 4]) {
        let mut x = x0.min(x1);
        let end = x0.max(x1);
        while x < end {
            for dx in 0..4 {
                if x + dx < end {
                    self.blend_pixel(x + dx, y, rgba);
                }
            }
            x += 8;
        }
    }

    fn draw_diamond(&mut self, cx: i32, cy: i32, radius: i32, rgba: [u8; 4]) {
        for dy in -radius..=radius {
            let span = radius - dy.abs();
            for dx in -span..=span {
                self.blend_pixel(cx + dx, cy + dy, rgba);
            }
        }
    }

    fn draw_down_triangle(&mut self, cx: i32, top_y: i32, half_w: i32, h: i32, rgba: [u8; 4]) {
        let h = h.max(1);
        for row in 0..h {
            let span = ((h - 1 - row) * half_w) / h;
            for dx in -span..=span {
                self.blend_pixel(cx + dx, top_y + row, rgba);
            }
        }
    }

    fn draw_text(&mut self, x: i32, y: i32, text: &str, scale: i32, rgba: [u8; 4]) {
        let scale = scale.max(1);
        let advance = 6 * scale;
        let mut cursor_x = x;
        for ch in text.chars() {
            let glyph = glyph_5x7(ch);
            for (row, bits) in glyph.iter().enumerate() {
                for col in 0..5 {
                    if (bits >> (4 - col)) & 1 == 1 {
                        for sy in 0..scale {
                            for sx in 0..scale {
                                self.blend_pixel(
                                    cursor_x + col * scale + sx,
                                    y + (row as i32) * scale + sy,
                                    rgba,
                                );
                            }
                        }
                    }
                }
            }
            cursor_x += advance;
        }
    }
}

/// Compact 5x7 ASCII bitmap font (uppercase-folded) with zero external crate dependencies.
fn glyph_5x7(ch: char) -> [u8; 7] {
    match ch.to_ascii_uppercase() {
        'A' => [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'B' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110],
        'C' => [0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110],
        'D' => [0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110],
        'E' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111],
        'F' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000],
        'G' => [0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110],
        'H' => [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'I' => [0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        'J' => [0b00111, 0b00010, 0b00010, 0b00010, 0b10010, 0b10010, 0b01100],
        'K' => [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001],
        'L' => [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111],
        'M' => [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001],
        'N' => [0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001],
        'O' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'P' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000],
        'Q' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101],
        'R' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001],
        'S' => [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110],
        'T' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
        'U' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'V' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100],
        'W' => [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001],
        'X' => [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001],
        'Y' => [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100],
        'Z' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111],
        '0' => [0b01110, 0b10011, 0b10101, 0b10101, 0b11001, 0b10001, 0b01110],
        '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        '2' => [0b01110, 0b10001, 0b00001, 0b00110, 0b01000, 0b10000, 0b11111],
        '3' => [0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110],
        '4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
        '5' => [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
        '6' => [0b01110, 0b10000, 0b11110, 0b10001, 0b10001, 0b10001, 0b01110],
        '7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
        '8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        '9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110],
        ':' => [0b00000, 0b00100, 0b00100, 0b00000, 0b00100, 0b00100, 0b00000],
        '.' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b01100],
        ',' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00100, 0b00100, 0b01000],
        '-' => [0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000],
        '+' => [0b00000, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000],
        '/' => [0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000],
        '|' => [0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
        '#' => [0b01010, 0b11111, 0b01010, 0b01010, 0b11111, 0b01010, 0b00000],
        '[' => [0b01110, 0b01000, 0b01000, 0b01000, 0b01000, 0b01000, 0b01110],
        ']' => [0b01110, 0b00010, 0b00010, 0b00010, 0b00010, 0b00010, 0b01110],
        '_' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b11111],
        ' ' => [0b00000; 7],
        _ => [0b11111, 0b10001, 0b00010, 0b00100, 0b00100, 0b00000, 0b00100],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synthetic::SyntheticMediaGenerator;

    fn sine_buffer(sample_rate: u32, channels: u16, frames: usize, freq_hz: f32, amp: f32) -> AudioBuffer {
        let mut samples = Vec::with_capacity(frames * channels as usize);
        for f in 0..frames {
            let t = f as f32 / sample_rate as f32;
            let v = amp * (2.0 * std::f32::consts::PI * freq_hz * t).sin();
            for _ in 0..channels {
                samples.push(v);
            }
        }
        AudioBuffer::new(sample_rate, channels, samples).unwrap()
    }

    #[test]
    fn silent_buffer_renders_clean_png_deterministically() {
        let buf = AudioBuffer::silence(48_000, 2, 48_000).unwrap();
        let opts = WaveformRenderOptions::new(640, 200);
        let frame_a = render_waveform_rgba(&buf, &opts).unwrap();
        let frame_b = render_waveform_rgba(&buf, &opts).unwrap();
        assert_eq!(frame_a.width, 640);
        assert_eq!(frame_a.height, 200);
        assert_eq!(frame_a.fnv1a_hash64(), frame_b.fnv1a_hash64());

        let png_bytes = render_waveform_png_bytes(&buf, &opts).unwrap();
        assert_eq!(&png_bytes[0..8], b"\x89PNG\r\n\x1a\n");
        let decoded = Rgba8Frame::decode_png_bytes(&png_bytes, Flicks::ZERO).unwrap();
        assert_eq!(decoded.width, 640);
        assert_eq!(decoded.height, 200);
    }

    #[test]
    fn sine_wave_pyramid_preserves_exact_peak_and_rms() {
        let buf = sine_buffer(48_000, 2, 48_000, 1000.0, 0.8);
        let pyr = WaveformPyramid::build(&buf, false).unwrap();
        assert!(pyr.levels[0].len() >= 3);
        let cols = pyr.sample_columns(None, 0, 48_000, 200);
        assert_eq!(cols.len(), 200);
        let max_peak = cols.iter().map(|c| c.max).fold(0.0f32, f32::max);
        let min_peak = cols.iter().map(|c| c.min).fold(0.0f32, f32::min);
        let avg_rms = cols.iter().map(|c| c.rms).sum::<f32>() / cols.len() as f32;
        assert!((max_peak - 0.8).abs() < 0.02, "max_peak={max_peak}");
        assert!((min_peak + 0.8).abs() < 0.02, "min_peak={min_peak}");
        let expected_rms = 0.8 * std::f32::consts::FRAC_1_SQRT_2;
        assert!((avg_rms - expected_rms).abs() < 0.03, "avg_rms={avg_rms}");
    }

    #[test]
    fn stereo_split_renders_independent_left_and_right_channels() {
        let frames = 48_000;
        let mut samples = Vec::with_capacity(frames * 2);
        for f in 0..frames {
            let t = f as f32 / 48_000.0;
            let l = 0.9 * (2.0 * std::f32::consts::PI * 440.0 * t).sin();
            samples.push(l);
            samples.push(0.0); // silent right channel
        }
        let buf = AudioBuffer::new(48_000, 2, samples).unwrap();
        let mut opts = WaveformRenderOptions::new(400, 240);
        opts.channel_layout = WaveformChannelLayout::StereoSplit;
        opts.show_ruler = false;
        let frame = render_waveform_rgba(&buf, &opts).unwrap();

        // Top half (L) has active waveform pixels away from center, bottom half (R) stays silent
        // away from the -6 dB guide line (y=152) and zero line (y=181).
        let mut top_lit = 0usize;
        let mut bot_lit = 0usize;
        for y in 10..48 {
            if frame.get_pixel(200, y) != Some(opts.background_rgba) {
                top_lit += 1;
            }
        }
        for y in 128..148 {
            if frame.get_pixel(200, y) != Some(opts.background_rgba) {
                bot_lit += 1;
            }
        }
        assert!(top_lit > 10, "expected active left channel pixels, got {top_lit}");
        assert_eq!(bot_lit, 0, "expected silent right channel off-center, got {bot_lit}");
    }

    #[test]
    fn extreme_zoom_in_polyline_renders_continuous_zero_crossings() {
        let buf = sine_buffer(48_000, 1, 48_000, 1000.0, 0.9);
        // 48 frames = 1 ms -> 400 px width => 0.12 frames/px (< 1.0 triggers polyline mode)
        let mut opts = WaveformRenderOptions::new(400, 180);
        opts.range_ticks = Some((Flicks::ZERO, Flicks(FLICKS_PER_SECOND / 1000)));
        opts.show_ruler = false;
        let frame = render_waveform_rgba(&buf, &opts).unwrap();

        let mut non_bg = 0usize;
        for y in 0..frame.height {
            for x in 0..frame.width {
                if frame.get_pixel(x, y) != Some(opts.background_rgba) {
                    non_bg += 1;
                }
            }
        }
        assert!(non_bg > 500, "expected continuous polyline waveform pixels, got {non_bg}");
    }

    #[test]
    fn spectral_rgb_distinguishes_low_mid_and_high_frequency_tones() {
        let low = sine_buffer(48_000, 1, 24_000, 80.0, 0.7);
        let mid = sine_buffer(48_000, 1, 24_000, 1000.0, 0.7);
        let high = sine_buffer(48_000, 1, 24_000, 6000.0, 0.7);

        let p_lo = WaveformPyramid::build(&low, true).unwrap();
        let p_mid = WaveformPyramid::build(&mid, true).unwrap();
        let p_hi = WaveformPyramid::build(&high, true).unwrap();

        let c_lo = p_lo.sample_columns(None, 4096, 20_000, 1)[0].band_energy;
        let c_mid = p_mid.sample_columns(None, 4096, 20_000, 1)[0].band_energy;
        let c_hi = p_hi.sample_columns(None, 4096, 20_000, 1)[0].band_energy;

        assert!(c_lo[0] > c_lo[1] && c_lo[0] > c_lo[2], "low band should dominate 80Hz: {c_lo:?}");
        assert!(c_mid[1] > c_mid[0] && c_mid[1] > c_mid[2], "mid band should dominate 1kHz: {c_mid:?}");
        assert!(c_hi[2] > c_hi[0] && c_hi[2] > c_hi[1], "high band should dominate 6kHz: {c_hi:?}");

        let mut opts = WaveformRenderOptions::new(320, 120);
        opts.style = WaveformStyle::SpectralRgb;
        opts.show_ruler = false;
        let f_lo = render_waveform_rgba(&low, &opts).unwrap();
        let f_hi = render_waveform_rgba(&high, &opts).unwrap();
        assert_ne!(f_lo.fnv1a_hash64(), f_hi.fnv1a_hash64());
        let px_lo = f_lo.get_pixel(160, 60).unwrap();
        let px_hi = f_hi.get_pixel(160, 60).unwrap();
        assert!(px_lo[0] > px_lo[2], "80Hz spectral pixel should lean red: {px_lo:?}");
        assert!(px_hi[2] > px_hi[0], "6kHz spectral pixel should lean blue/cyan: {px_hi:?}");
    }

    #[test]
    fn beat_and_cue_markers_render_on_synthetic_beat_audio() {
        let (buf, beats) = SyntheticMediaGenerator::generate_beat_audio(
            Flicks(2 * FLICKS_PER_SECOND),
            120,
            48_000,
        );
        let mut opts = WaveformRenderOptions::new(800, 240);
        opts.downbeat_ticks = vec![beats[0]];
        opts.beat_ticks = beats[1..].to_vec();
        opts.cue_ticks = vec![Flicks(FLICKS_PER_SECOND)];
        opts.playhead_tick = Some(Flicks(FLICKS_PER_SECOND / 2));
        opts.revision_id = Some(42);
        opts.bpm = Some(120.0);

        let frame = render_waveform_rgba(&buf, &opts).unwrap();
        assert_eq!(frame.width, 800);
        assert_eq!(frame.height, 240);

        // Verify playhead red pixels exist around x ~ 10 + (780 * 0.25) = 205
        let mut found_red = false;
        for x in 200..212 {
            if frame.get_pixel(x, 120) == Some([239, 68, 68, 255]) {
                found_red = true;
                break;
            }
        }
        assert!(found_red, "expected red playhead line at 0.5s");
    }

    #[test]
    fn rejects_invalid_dimensions_empty_buffer_and_inverted_range() {
        let buf = sine_buffer(48_000, 1, 1024, 440.0, 0.5);
        assert!(matches!(
            render_waveform_rgba(&buf, &WaveformRenderOptions::new(0, 100)),
            Err(MediaError::InvalidDimensions { .. })
        ));

        let mut bad_range = WaveformRenderOptions::new(320, 120);
        bad_range.range_ticks = Some((Flicks(1000), Flicks(1000)));
        assert!(matches!(
            render_waveform_rgba(&buf, &bad_range),
            Err(MediaError::InvalidAudioFormat(_))
        ));

        let empty = AudioBuffer::silence(48_000, 1, 0).unwrap();
        assert!(matches!(
            render_waveform_rgba(&empty, &WaveformRenderOptions::new(320, 120)),
            Err(MediaError::EmptyMediaStream(_))
        ));
    }
}
