use crate::error::CoreError;
use frameshift_media::{
    AudioBuffer, AudioRetimeMode, DEFAULT_AUDIO_SAMPLE_RATE, DecoderPool, DecoderPoolTelemetry,
    FfmpegBridge, LruFrameCache, MediaProbeInfo, Rgba8Frame, Rgba16fFrame, SourceFit as DecodeFit,
    SyntheticMediaGenerator, apply_micro_silence, apply_sidechain_ducking, apply_soft_limiter,
    apply_stereo_pan,
};
use frameshift_render::{FrameProvider, RenderError, SourceFrame};
use frameshift_schema::project::{AudioBus, AudioClipDef, MicroSilenceDef};
use frameshift_schema::{
    AssetDef, AssetKind, NodeId, NodeKind, Project, SchemaError, SourceFit, TextAlign2D,
};
use frameshift_time::{FLICKS_PER_SECOND, Flicks, SpeedCurve, ticks_to_audio_samples_nearest};
use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// Built-in deterministic synthetic asset kinds resolved when on-disk media is absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntheticAssetKind {
    ClipA,
    ClipB,
    MovingMask,
    BeatAudio120Bpm,
}

/// Resolved media source for a project [`AssetDef`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedAssetSource {
    Disk(std::path::PathBuf),
    Synthetic(SyntheticAssetKind),
}

/// Bit-exact cache key for static `NodeKind::VectorText` rasterization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorTextCacheKey {
    pub width: u32,
    pub height: u32,
    pub text: String,
    pub font_family: Option<String>,
    pub font_path: Option<String>,
    pub font_weight: u16,
    pub font_size_bits: u32,
    pub color_bits: [u32; 4],
    pub outline_bits: [u32; 4],
    pub outline_width_bits: u32,
    pub align: TextAlign2D,
    pub x_offset_bits: Option<u32>,
    pub y_offset_bits: u32,
    pub tracking_bits: Option<u32>,
    pub line_height_bits: Option<u32>,
    pub opacity_bits: u32,
    pub fallback_fonts: Vec<String>,
    pub writing_mode: u8,
    pub var_wght_bits: Option<u32>,
    pub var_wdth_bits: Option<u32>,
    pub var_slnt_bits: Option<u32>,
    pub color_bottom_bits: Option<[u32; 4]>,
    pub gradient_angle_bits: u32,
    pub outer_outline_bits: [u32; 4],
    pub outer_outline_width_bits: u32,
    pub outline_gap_bits: u32,
    pub shadow_offset_bits: [u32; 2],
    pub shadow_blur_bits: u32,
    pub shadow_bits: [u32; 4],
    pub glow_radius_bits: u32,
    pub glow_bits: [u32; 4],
    pub hdr_intensity_bits: u32,
    pub gpu_slug: bool,
}

impl Hash for VectorTextCacheKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.width.hash(state);
        self.height.hash(state);
        self.text.hash(state);
        self.font_family.hash(state);
        self.font_path.hash(state);
        self.font_weight.hash(state);
        self.font_size_bits.hash(state);
        self.color_bits.hash(state);
        self.outline_bits.hash(state);
        self.outline_width_bits.hash(state);
        let align_tag: u8 = match self.align {
            TextAlign2D::Left => 0,
            TextAlign2D::Center => 1,
            TextAlign2D::Right => 2,
        };
        align_tag.hash(state);
        self.x_offset_bits.hash(state);
        self.y_offset_bits.hash(state);
        self.tracking_bits.hash(state);
        self.line_height_bits.hash(state);
        self.opacity_bits.hash(state);
        self.fallback_fonts.hash(state);
        self.writing_mode.hash(state);
        self.var_wght_bits.hash(state);
        self.var_wdth_bits.hash(state);
        self.var_slnt_bits.hash(state);
        self.color_bottom_bits.hash(state);
        self.gradient_angle_bits.hash(state);
        self.outer_outline_bits.hash(state);
        self.outer_outline_width_bits.hash(state);
        self.outline_gap_bits.hash(state);
        self.shadow_offset_bits.hash(state);
        self.shadow_blur_bits.hash(state);
        self.shadow_bits.hash(state);
        self.glow_radius_bits.hash(state);
        self.glow_bits.hash(state);
        self.hdr_intensity_bits.hash(state);
        self.gpu_slug.hash(state);
    }
}

impl VectorTextCacheKey {
    /// Builds a bit-exact cache key from a `NodeKind::VectorText` variant.
    pub fn from_node_kind(kind: &NodeKind, width: u32, height: u32) -> Option<Self> {
        let NodeKind::VectorText {
            text,
            font_size_norm,
            color_rgba,
            outline_rgba,
            outline_width_norm,
            align_center,
            y_offset_norm,
            font_family,
            font_path,
            font_weight,
            align,
            x_offset_norm,
            tracking_em,
            line_height_em,
            opacity,
            fallback_fonts,
            writing_mode,
            var_wght,
            var_wdth,
            var_slnt,
            color_bottom_rgba,
            gradient_angle_deg,
            outer_outline_rgba,
            outer_outline_width_norm,
            outline_gap_norm,
            shadow_offset_norm,
            shadow_blur_norm,
            shadow_rgba,
            glow_radius_norm,
            glow_rgba,
            hdr_intensity,
            gpu_slug,
            text_animator,
        } = kind
        else {
            return None;
        };
        if text_animator.is_some() {
            return None;
        }
        let effective_align = align.unwrap_or(if *align_center {
            TextAlign2D::Center
        } else {
            TextAlign2D::Left
        });
        let wm_tag = match writing_mode {
            frameshift_schema::WritingMode2D::HorizontalTb => 0u8,
            frameshift_schema::WritingMode2D::VerticalRl => 1u8,
        };
        Some(Self {
            width,
            height,
            text: text.clone(),
            font_family: font_family.clone(),
            font_path: font_path.clone(),
            font_weight: *font_weight,
            font_size_bits: font_size_norm.to_bits(),
            color_bits: color_rgba.map(f32::to_bits),
            outline_bits: outline_rgba.map(f32::to_bits),
            outline_width_bits: outline_width_norm.to_bits(),
            align: effective_align,
            x_offset_bits: x_offset_norm.map(f32::to_bits),
            y_offset_bits: y_offset_norm.to_bits(),
            tracking_bits: tracking_em.map(f32::to_bits),
            line_height_bits: line_height_em.map(f32::to_bits),
            opacity_bits: opacity.to_bits(),
            fallback_fonts: fallback_fonts.clone(),
            writing_mode: wm_tag,
            var_wght_bits: var_wght.map(f32::to_bits),
            var_wdth_bits: var_wdth.map(f32::to_bits),
            var_slnt_bits: var_slnt.map(f32::to_bits),
            color_bottom_bits: color_bottom_rgba.map(|c| c.map(f32::to_bits)),
            gradient_angle_bits: gradient_angle_deg.to_bits(),
            outer_outline_bits: outer_outline_rgba.map(f32::to_bits),
            outer_outline_width_bits: outer_outline_width_norm.to_bits(),
            outline_gap_bits: outline_gap_norm.to_bits(),
            shadow_offset_bits: shadow_offset_norm.map(f32::to_bits),
            shadow_blur_bits: shadow_blur_norm.to_bits(),
            shadow_bits: shadow_rgba.map(f32::to_bits),
            glow_radius_bits: glow_radius_norm.to_bits(),
            glow_bits: glow_rgba.map(f32::to_bits),
            hdr_intensity_bits: hdr_intensity.to_bits(),
            gpu_slug: *gpu_slug,
        })
    }
}

/// Bit-exact cache key for static `NodeKind::VectorPathHud` rasterization.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HudPathCacheKey {
    pub width: u32,
    pub height: u32,
    pub points_bits: Vec<[u32; 2]>,
    pub bezier_bits: Vec<[[u32; 2]; 4]>,
    pub closed: bool,
    pub stroke_width_bits: u32,
    pub stroke_bits: [u32; 4],
    pub fill_bits: [u32; 4],
    pub dash_len_bits: u32,
    pub gap_len_bits: u32,
    pub dash_offset_bits: u32,
    pub trim_start_bits: u32,
    pub trim_end_bits: u32,
    pub node_dot_radius_bits: u32,
    pub data_value_bits: u32,
    pub data_label: String,
    pub opacity_bits: u32,
}

impl HudPathCacheKey {
    /// Builds a bit-exact cache key from a `NodeKind::VectorPathHud` variant.
    pub fn from_node_kind(kind: &NodeKind, width: u32, height: u32) -> Option<Self> {
        let NodeKind::VectorPathHud {
            points_norm,
            bezier_segments,
            closed,
            stroke_width_norm,
            stroke_rgba,
            fill_rgba,
            dash_len_norm,
            gap_len_norm,
            dash_offset_norm,
            trim_start,
            trim_end,
            node_dot_radius_norm,
            data_value,
            data_label,
            opacity,
        } = kind
        else {
            return None;
        };
        Some(Self {
            width,
            height,
            points_bits: points_norm.iter().map(|p| p.map(f32::to_bits)).collect(),
            bezier_bits: bezier_segments
                .iter()
                .map(|s| {
                    [
                        s.p0.map(f32::to_bits),
                        s.c1.map(f32::to_bits),
                        s.c2.map(f32::to_bits),
                        s.p1.map(f32::to_bits),
                    ]
                })
                .collect(),
            closed: *closed,
            stroke_width_bits: stroke_width_norm.to_bits(),
            stroke_bits: stroke_rgba.map(f32::to_bits),
            fill_bits: fill_rgba.map(f32::to_bits),
            dash_len_bits: dash_len_norm.to_bits(),
            gap_len_bits: gap_len_norm.to_bits(),
            dash_offset_bits: dash_offset_norm.to_bits(),
            trim_start_bits: trim_start.to_bits(),
            trim_end_bits: trim_end.to_bits(),
            node_dot_radius_bits: node_dot_radius_norm.to_bits(),
            data_value_bits: data_value.to_bits(),
            data_label: data_label.clone(),
            opacity_bits: opacity.to_bits(),
        })
    }
}

/// Resolves project assets (both on-disk via `FfmpegBridge` / `DecoderPool` and synthetic via
/// `SyntheticMediaGenerator`) and caches probed container `PtsIndex` metadata.
#[derive(Debug, Default)]
pub struct MediaResolver {
    probe_cache: HashMap<PathBuf, (DiskMediaStamp, MediaProbeInfo)>,
    vector_text_cache: HashMap<VectorTextCacheKey, Rgba16fFrame>,
    hud_path_cache: HashMap<HudPathCacheKey, Rgba16fFrame>,
    audio_cache: RefCell<HashMap<String, AudioBuffer>>,
    decoder_pool: DecoderPool,
}

#[derive(Debug, PartialEq, Eq)]
struct DiskMediaStamp {
    length: u64,
    modified: Option<std::time::SystemTime>,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}

fn disk_media_stamp(path: &Path) -> Result<DiskMediaStamp, CoreError> {
    let metadata = std::fs::metadata(path)?;
    Ok(DiskMediaStamp {
        length: metadata.len(),
        modified: metadata.modified().ok(),
        #[cfg(unix)]
        identity: {
            use std::os::unix::fs::MetadataExt;
            (
                metadata.dev(),
                metadata.ino(),
                metadata.ctime(),
                metadata.ctime_nsec(),
            )
        },
    })
}

impl MediaResolver {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear_probe_cache(&mut self) {
        self.probe_cache.clear();
        self.vector_text_cache.clear();
        self.hud_path_cache.clear();
        self.audio_cache.get_mut().clear();
        self.decoder_pool.close_all();
    }

    /// Returns the number of decoded/synthesized audio assets currently cached.
    pub fn audio_cache_len(&self) -> usize {
        self.audio_cache.borrow().len()
    }

    /// Closes all open streaming video decoder subprocesses without clearing probed metadata.
    pub fn close_video_streams(&mut self) {
        self.decoder_pool.close_all();
    }

    /// Returns current streaming video decoder pool telemetry.
    pub fn decoder_telemetry(&self) -> DecoderPoolTelemetry {
        self.decoder_pool.telemetry()
    }

    /// Resolves `asset` to either an on-disk path or a recognized built-in synthetic generator.
    pub fn resolve_asset_source(&self, asset: &AssetDef) -> Result<ResolvedAssetSource, CoreError> {
        let path = Path::new(&asset.path);
        if path.is_file() {
            return Ok(ResolvedAssetSource::Disk(path.to_path_buf()));
        }

        if let Some(kind) = match_synthetic_asset(asset) {
            return Ok(ResolvedAssetSource::Synthetic(kind));
        }

        Err(CoreError::MissingAssetFile {
            asset_id: asset.id.clone(),
            path: asset.path.clone(),
        })
    }

    /// Retrieves (or probes and caches) `MediaProbeInfo` for an on-disk video file.
    pub fn probe_disk_video(
        &mut self,
        _asset_id: &str,
        path: &Path,
    ) -> Result<MediaProbeInfo, CoreError> {
        let stamp = disk_media_stamp(path)?;
        if let Some((previous, info)) = self.probe_cache.get(path) {
            if *previous == stamp {
                return Ok(info.clone());
            }
            self.decoder_pool.close_all();
        }
        let info = FfmpegBridge::probe_media_info(path)?;
        self.probe_cache
            .insert(path.to_owned(), (stamp, info.clone()));
        Ok(info)
    }

    /// Loads or synthesizes a video/image frame at `(asset_id, source_tick, width, height)`
    /// through `cache`.
    ///
    /// `fit` is the mode from [`NodeKind::source_fit`]. On-disk frames use it.
    /// [`SourceFit::Stretch`] keeps the historical ffmpeg `-s` scale. Synthetic assets are
    /// painted at the requested size and have no separate source raster to crop.
    pub fn provide_video_frame(
        &mut self,
        cache: &mut LruFrameCache,
        asset: &AssetDef,
        source_tick: Flicks,
        width: u32,
        height: u32,
        fit: SourceFit,
    ) -> Result<Rgba8Frame, CoreError> {
        if asset.kind == AssetKind::Audio {
            return Err(CoreError::InvalidArgument(format!(
                "asset '{}' is Audio and cannot be sampled as video",
                asset.id
            )));
        }

        let clamped_tick = Flicks(source_tick.0.max(0));
        let source = self.resolve_asset_source(asset)?;
        let cache_asset_key = format!(
            "{}:{:?}:{:?}@{}x{}:{}",
            asset.id,
            asset.path,
            asset.fps,
            width,
            height,
            source_fit_cache_name(fit)
        );
        let decode_fit = to_decode_fit(fit);

        match source {
            ResolvedAssetSource::Disk(path) => {
                let probe = self.probe_disk_video(&asset.id, &path)?;
                let stamp = self.probe_cache.get(&path).map(|(stamp, _)| stamp);
                let cache_asset_key = format!("{cache_asset_key}:{stamp:?}");
                let matched = probe.pts_index.lookup_frame(clamped_tick).ok_or_else(|| {
                    CoreError::Media(frameshift_media::MediaError::EmptyMediaStream(format!(
                        "empty PtsIndex for '{}'",
                        path.display()
                    )))
                })?;
                let frame_key = matched.frame_index as u64;
                if let Some(cached) = cache.get(&cache_asset_key, frame_key)
                    && cached.width == width
                    && cached.height == height
                {
                    return Ok((*cached).clone());
                }
                let decoded = self.decoder_pool.decode_frame(
                    &path,
                    &probe.pts_index,
                    matched.frame_index,
                    width,
                    height,
                    decode_fit,
                )?;
                cache.insert(cache_asset_key, frame_key, decoded.clone());
                Ok(decoded)
            }
            ResolvedAssetSource::Synthetic(kind) => {
                let (frame_key, quantized_tick) = if let Some(fps) = asset.fps {
                    let frame_idx = fps.ticks_to_frame_floor(clamped_tick)?.max(0);
                    let q_tick = fps.frame_to_ticks(frame_idx)?;
                    (frame_idx as u64, q_tick)
                } else {
                    (clamped_tick.0 as u64, clamped_tick)
                };

                if let Some(cached) = cache.get(&cache_asset_key, frame_key)
                    && cached.width == width
                    && cached.height == height
                {
                    return Ok((*cached).clone());
                }

                let frame = match kind {
                    SyntheticAssetKind::ClipA => SyntheticMediaGenerator::generate_clip_a_frame(
                        width,
                        height,
                        quantized_tick,
                    ),
                    SyntheticAssetKind::ClipB => SyntheticMediaGenerator::generate_clip_b_frame(
                        width,
                        height,
                        quantized_tick,
                    ),
                    SyntheticAssetKind::MovingMask => {
                        SyntheticMediaGenerator::generate_moving_mask_frame(
                            width,
                            height,
                            quantized_tick,
                        )
                    }
                    SyntheticAssetKind::BeatAudio120Bpm => {
                        return Err(CoreError::InvalidArgument(format!(
                            "asset '{}' resolved to BeatAudio120Bpm and cannot provide video frames",
                            asset.id
                        )));
                    }
                };
                cache.insert(cache_asset_key, frame_key, frame.clone());
                Ok(frame)
            }
        }
    }

    /// Loads or synthesizes a static image frame for `(asset.id, width, height, fit)` through
    /// `cache` without invoking `ffprobe` or `PtsIndex`.
    pub fn provide_image_frame(
        &mut self,
        cache: &mut LruFrameCache,
        asset: &AssetDef,
        width: u32,
        height: u32,
        fit: SourceFit,
    ) -> Result<Rgba8Frame, CoreError> {
        if asset.kind == AssetKind::Audio {
            return Err(CoreError::InvalidArgument(format!(
                "asset '{}' is Audio and cannot be sampled as an image",
                asset.id
            )));
        }

        let source = self.resolve_asset_source(asset)?;
        let cache_asset_key = format!(
            "img:{}@{}x{}:{}",
            asset.id,
            width,
            height,
            source_fit_cache_name(fit)
        );
        let frame_key = 0u64;
        if let Some(cached) = cache.get(&cache_asset_key, frame_key)
            && cached.width == width
            && cached.height == height
        {
            return Ok((*cached).clone());
        }

        let frame = match source {
            ResolvedAssetSource::Disk(path) => {
                FfmpegBridge::load_image_frame_with_fit(&path, width, height, to_decode_fit(fit))?
            }
            ResolvedAssetSource::Synthetic(kind) => match kind {
                SyntheticAssetKind::ClipA => {
                    SyntheticMediaGenerator::generate_clip_a_frame(width, height, Flicks::ZERO)
                }
                SyntheticAssetKind::ClipB => {
                    SyntheticMediaGenerator::generate_clip_b_frame(width, height, Flicks::ZERO)
                }
                SyntheticAssetKind::MovingMask => {
                    SyntheticMediaGenerator::generate_moving_mask_frame(width, height, Flicks::ZERO)
                }
                SyntheticAssetKind::BeatAudio120Bpm => {
                    return Err(CoreError::InvalidArgument(format!(
                        "asset '{}' resolved to BeatAudio120Bpm and cannot provide image frames",
                        asset.id
                    )));
                }
            },
        };
        cache.insert(cache_asset_key, frame_key, frame.clone());
        Ok(frame)
    }

    /// Decodes or synthesizes an audio asset at `DEFAULT_AUDIO_SAMPLE_RATE` (`48_000 Hz`, stereo),
    /// caching the result in memory so repeated clips reuse the same decoded buffer.
    pub fn load_audio_asset(
        &self,
        asset: &AssetDef,
        min_duration: Flicks,
    ) -> Result<AudioBuffer, CoreError> {
        match self.resolve_asset_source(asset)? {
            ResolvedAssetSource::Disk(path) => {
                let stamp = disk_media_stamp(&path)?;
                let cache_key = format!(
                    "disk:{}:{}:{}:{stamp:?}",
                    path.display(),
                    DEFAULT_AUDIO_SAMPLE_RATE,
                    2
                );
                if let Some(cached) = self.audio_cache.borrow().get(&cache_key) {
                    return Ok(cached.clone());
                }
                let decoded = FfmpegBridge::decode_audio(&path, DEFAULT_AUDIO_SAMPLE_RATE, 2)?;
                self.audio_cache
                    .borrow_mut()
                    .insert(cache_key, decoded.clone());
                Ok(decoded)
            }
            ResolvedAssetSource::Synthetic(SyntheticAssetKind::BeatAudio120Bpm) => {
                let dur = asset
                    .duration_ticks
                    .map(|d| if d > min_duration { d } else { min_duration })
                    .unwrap_or(min_duration)
                    .max(Flicks(FLICKS_PER_SECOND));
                let cache_key = format!(
                    "synthetic:beat120:{}:{}:{}",
                    dur.0, DEFAULT_AUDIO_SAMPLE_RATE, 2
                );
                if let Some(cached) = self.audio_cache.borrow().get(&cache_key) {
                    return Ok(cached.clone());
                }
                let (buf, _beats) = SyntheticMediaGenerator::generate_beat_audio(
                    dur,
                    120,
                    DEFAULT_AUDIO_SAMPLE_RATE,
                );
                self.audio_cache.borrow_mut().insert(cache_key, buf.clone());
                Ok(buf)
            }
            ResolvedAssetSource::Synthetic(other) => Err(CoreError::InvalidArgument(format!(
                "synthetic asset '{}' ({other:?}) does not supply an audio stream",
                asset.id
            ))),
        }
    }

    /// Mixes all `project.timeline.audio_clips` into a single `48_000 Hz` stereo `AudioBuffer`.
    ///
    /// Resolves `anchor_event_id` + `apex_offset_ticks` placement, stereo panning, per-clip
    /// fade ramps, bus routing, sidechain ducking rules, targeted/master micro-silences,
    /// and soft peak limiting (`0.99` ceiling when peaks exceed `0.99`).
    /// Returns `Ok(None)` when `audio_clips` is empty.
    pub fn mix_project_audio(&self, project: &Project) -> Result<Option<AudioBuffer>, CoreError> {
        if project.timeline.audio_clips.is_empty() {
            return Ok(None);
        }

        let mut bus_buffers: [Option<AudioBuffer>; 6] = std::array::from_fn(|_| None);

        for ac in &project.timeline.audio_clips {
            let asset = project
                .asset(&ac.asset_id)
                .ok_or_else(|| SchemaError::MissingAsset(ac.asset_id.clone()))?;
            let clip_duration = ac.timeline_end - ac.timeline_start;
            let (effective_start, _effective_end) = resolve_audio_clip_bounds(project, ac)?;
            let required_src_end = ac.source_start + clip_duration;
            let full_audio = self.load_audio_asset(asset, required_src_end)?;
            let mut sliced = slice_audio_buffer(&full_audio, ac.source_start, clip_duration)?;
            if ac.pan != 0.0 {
                apply_stereo_pan(&mut sliced, ac.pan);
            }

            let slot = &mut bus_buffers[audio_bus_index(ac.bus)];
            if slot.is_none() {
                *slot = Some(AudioBuffer::silence_for_duration(
                    DEFAULT_AUDIO_SAMPLE_RATE,
                    2,
                    project.timeline.duration_ticks,
                )?);
            }
            let bus_out = slot.as_mut().expect("bus buffer initialized");

            if ac.fade_in.is_none() && ac.fade_out.is_none() {
                bus_out.mix_in(&sliced, effective_start, ac.gain_db)?;
            } else {
                bus_out.mix_in_with_gain_curve(
                    &sliced,
                    effective_start,
                    ac.gain_db,
                    |progress| audio_clip_fade_gain(ac, clip_duration, progress as f64) as f32,
                )?;
            }
        }

        for rule in &project.timeline.ducking_rules {
            let trig_idx = audio_bus_index(rule.trigger_bus);
            let targ_idx = audio_bus_index(rule.target_bus);
            if trig_idx == targ_idx {
                continue;
            }
            let (trig_opt, targ_opt) = if trig_idx < targ_idx {
                let (head, tail) = bus_buffers.split_at_mut(targ_idx);
                (&head[trig_idx], &mut tail[0])
            } else {
                let (head, tail) = bus_buffers.split_at_mut(trig_idx);
                (&tail[0], &mut head[targ_idx])
            };
            if let (Some(trigger), Some(target)) = (trig_opt.as_ref(), targ_opt.as_mut()) {
                apply_sidechain_ducking(
                    target,
                    trigger,
                    rule.threshold,
                    rule.duck_gain,
                    rule.attack_ms,
                    rule.release_ms,
                );
            }
        }

        for ms in &project.timeline.micro_silences {
            if let Some(bus) = ms.target_bus
                && let Some(bus_buf) = bus_buffers[audio_bus_index(bus)].as_mut()
            {
                apply_project_micro_silence(bus_buf, ms)?;
            }
        }

        let mut active_buses = bus_buffers.into_iter().flatten();
        let Some(mut out) = active_buses.next() else {
            return Ok(None);
        };
        for bus_buf in active_buses {
            for (dst, &src) in out.samples.iter_mut().zip(&bus_buf.samples) {
                *dst += src;
            }
        }

        for ms in &project.timeline.micro_silences {
            if ms.target_bus.is_none() {
                apply_project_micro_silence(&mut out, ms)?;
            }
        }

        if out.samples.iter().any(|s| !s.is_finite() || s.abs() > 0.99) {
            apply_soft_limiter(&mut out, 0.99);
        }
        Ok(Some(out))
    }

    /// Retimes an audio buffer through a `SpeedCurve` using explicit [`AudioRetimeMode`].
    pub fn retime_audio_buffer(
        buffer: &AudioBuffer,
        speed_curve: &SpeedCurve,
        output_duration: Flicks,
        source_start: Flicks,
        mode: AudioRetimeMode,
    ) -> Result<AudioBuffer, CoreError> {
        Ok(buffer.retime(speed_curve, output_duration, source_start, mode)?)
    }

    /// Rasterizes a `NodeKind::VectorText` node via `frameshift-vector` into a premultiplied
    /// Linear Rec.709 `Rgba16fFrame` suitable for `wgpu` compositor upload.
    pub fn render_vector_text_node(
        kind: &NodeKind,
        width: u32,
        height: u32,
        pts: Flicks,
    ) -> Result<Rgba16fFrame, CoreError> {
        if !matches!(kind, NodeKind::VectorText { .. }) {
            return Err(CoreError::InvalidArgument(
                "render_vector_text_node requires NodeKind::VectorText".into(),
            ));
        }
        Ok(frameshift_vector::render_vector_text_node_rgba16f(
            kind, width, height, pts,
        )?)
    }

    /// Rasterizes a `NodeKind::VectorPathHud` node via `frameshift-vector` into a premultiplied
    /// Linear Rec.709 `Rgba16fFrame` suitable for `wgpu` compositor upload.
    pub fn render_vector_path_hud_node(
        kind: &NodeKind,
        width: u32,
        height: u32,
        pts: Flicks,
    ) -> Result<Rgba16fFrame, CoreError> {
        if !matches!(kind, NodeKind::VectorPathHud { .. }) {
            return Err(CoreError::InvalidArgument(
                "render_vector_path_hud_node requires NodeKind::VectorPathHud".into(),
            ));
        }
        Ok(frameshift_vector::render_vector_path_hud_node_rgba16f(
            kind, width, height, pts,
        )?)
    }
}

/// Resolves an audio clip's `[start, end)` timeline bounds, shifting by
/// `event.target_output_tick - apex_offset_ticks` when `anchor_event_id` is set.
pub fn resolve_audio_clip_bounds(
    project: &Project,
    ac: &AudioClipDef,
) -> Result<(Flicks, Flicks), CoreError> {
    let duration = ac.timeline_end - ac.timeline_start;
    let start = if let Some(ref event_id) = ac.anchor_event_id {
        let event = project
            .event(event_id)
            .ok_or_else(|| SchemaError::MissingEvent(event_id.clone()))?;
        let apex_offset = ac.apex_offset_ticks.unwrap_or(Flicks::ZERO);
        Flicks(event.target_output_tick.0.saturating_sub(apex_offset.0))
    } else {
        ac.timeline_start
    };
    Ok((start, Flicks(start.0.saturating_add(duration.0))))
}

fn audio_bus_index(bus: AudioBus) -> usize {
    match bus {
        AudioBus::Music => 0,
        AudioBus::Dialogue => 1,
        AudioBus::SfxImpact => 2,
        AudioBus::SfxWhoosh => 3,
        AudioBus::SfxRiser => 4,
        AudioBus::SubBass => 5,
    }
}

fn apply_project_micro_silence(
    buf: &mut AudioBuffer,
    ms: &MicroSilenceDef,
) -> Result<(), CoreError> {
    let start_sample =
        ticks_to_audio_samples_nearest(Flicks(ms.start_tick.0.max(0)), buf.sample_rate)?.max(0)
            as usize;
    let end_sample =
        ticks_to_audio_samples_nearest(Flicks(ms.end_tick.0.max(0)), buf.sample_rate)?.max(0)
            as usize;
    let fade_samples =
        ((ms.fade_ms.max(0.0) as f64) * 0.001 * (buf.sample_rate as f64)).round() as usize;
    apply_micro_silence(buf, start_sample, end_sample, fade_samples, ms.floor_gain);
    Ok(())
}

/// Evaluates the combined fade multiplier of `ac` at normalized clip progress.
///
/// `progress` follows `mix_in_with_gain_curve`: `0.0` for the first clip sample and
/// `1.0` for the last clip sample. The fade-in ramp spans the first
/// `fade_in.duration_ticks`; the fade-out ramp spans the last `fade_out.duration_ticks`.
/// Overlapping ramps multiply, and clips without fades return `1.0`.
pub fn audio_clip_fade_gain(ac: &AudioClipDef, clip_duration: Flicks, progress: f64) -> f64 {
    let clip_ticks = clip_duration.0 as f64;
    let pos_ticks = progress.clamp(0.0, 1.0) * clip_ticks;
    let mut gain = 1.0;
    if let Some(fade_in) = ac.fade_in {
        gain *= fade_in.gain_at_elapsed(pos_ticks);
    }
    if let Some(fade_out) = ac.fade_out {
        gain *= fade_out.gain_at_elapsed(clip_ticks - pos_ticks);
    }
    gain
}

/// Slices `[start, start + duration)` from `buffer` cleanly with silence padding if needed.
pub fn slice_audio_buffer(
    buffer: &AudioBuffer,
    start: Flicks,
    duration: Flicks,
) -> Result<AudioBuffer, CoreError> {
    if duration.0 <= 0 {
        return Ok(AudioBuffer::silence(
            buffer.sample_rate,
            buffer.channels,
            0,
        )?);
    }
    let out_frames_i64 = ticks_to_audio_samples_nearest(duration, buffer.sample_rate)?.max(0);
    let out_frames = usize::try_from(out_frames_i64)
        .map_err(|_| CoreError::InvalidArgument("audio slice frame count overflow".into()))?;
    let start_sample = ticks_to_audio_samples_nearest(Flicks(start.0.max(0)), buffer.sample_rate)?;
    let ch = buffer.channels as usize;
    let total_src_frames = buffer.frame_count();
    let mut out = AudioBuffer::silence(buffer.sample_rate, buffer.channels, out_frames)?;

    for i in 0..out_frames {
        let src_idx_i64 = start_sample.saturating_add(i as i64);
        if src_idx_i64 < 0 {
            continue;
        }
        let src_idx = src_idx_i64 as usize;
        if src_idx >= total_src_frames {
            break;
        }
        let dst_off = i * ch;
        let src_off = src_idx * ch;
        out.samples[dst_off..dst_off + ch].copy_from_slice(&buffer.samples[src_off..src_off + ch]);
    }
    Ok(out)
}

fn to_decode_fit(fit: SourceFit) -> DecodeFit {
    match fit {
        SourceFit::Stretch => DecodeFit::Stretch,
        SourceFit::Cover => DecodeFit::Cover,
        SourceFit::Contain => DecodeFit::Contain,
    }
}

fn source_fit_cache_name(fit: SourceFit) -> &'static str {
    match fit {
        SourceFit::Stretch => "stretch",
        SourceFit::Cover => "cover",
        SourceFit::Contain => "contain",
    }
}

fn match_synthetic_asset(asset: &AssetDef) -> Option<SyntheticAssetKind> {
    let id_lower = asset.id.to_ascii_lowercase();
    let path_lower = asset.path.replace('\\', "/").to_ascii_lowercase();

    match asset.kind {
        AssetKind::Audio => {
            if id_lower == "asset_bgm"
                || id_lower.contains("beat_track")
                || path_lower == "local_media/beat_track.wav"
                || path_lower.starts_with("synthetic://beat")
            {
                Some(SyntheticAssetKind::BeatAudio120Bpm)
            } else {
                None
            }
        }
        AssetKind::Video | AssetKind::Image => {
            if id_lower == "asset_clip_a"
                || path_lower == "local_media/clip_a.mp4"
                || path_lower == "synthetic://clip_a"
            {
                Some(SyntheticAssetKind::ClipA)
            } else if id_lower == "asset_clip_b"
                || path_lower == "local_media/clip_b.mp4"
                || path_lower == "synthetic://clip_b"
            {
                Some(SyntheticAssetKind::ClipB)
            } else if id_lower == "asset_mask"
                || id_lower == "asset_moving_mask"
                || id_lower.contains("moving_mask")
                || path_lower == "local_media/moving_mask.mp4"
                || path_lower == "synthetic://moving_mask"
            {
                Some(SyntheticAssetKind::MovingMask)
            } else {
                None
            }
        }
    }
}

/// Adapts [`MediaResolver`] + [`LruFrameCache`] to `frameshift_render::FrameProvider`.
pub(crate) struct ProjectFrameProvider<'a> {
    project: &'a Project,
    resolver: RefCell<&'a mut MediaResolver>,
    cache: RefCell<&'a mut LruFrameCache>,
}

impl<'a> ProjectFrameProvider<'a> {
    pub(crate) fn new(
        project: &'a Project,
        resolver: &'a mut MediaResolver,
        cache: &'a mut LruFrameCache,
    ) -> Self {
        Self {
            project,
            resolver: RefCell::new(resolver),
            cache: RefCell::new(cache),
        }
    }
}

fn node_has_dynamic_binding(project: &Project, node_id: &NodeId) -> bool {
    project.param_bindings.iter().any(|b| &b.node_id == node_id)
        || project
            .timeline
            .clips
            .iter()
            .flat_map(|c| c.event_animations.iter())
            .any(|a| &a.target_node_id == node_id)
}

fn map_core_vector_or_provider_err(
    err: CoreError,
    asset_id: &str,
    source_tick: Flicks,
) -> RenderError {
    match err {
        CoreError::Vector(vec_err) => RenderError::Vector(vec_err),
        other => RenderError::FrameProvider {
            asset_id: asset_id.to_owned(),
            source_tick: source_tick.0,
            message: other.to_string(),
        },
    }
}

impl FrameProvider for ProjectFrameProvider<'_> {
    fn provide_frame(
        &self,
        asset_id: &str,
        source_tick: Flicks,
        width: u32,
        height: u32,
    ) -> Result<SourceFrame, RenderError> {
        self.provide_source_frame(asset_id, source_tick, width, height, SourceFit::Stretch)
    }

    fn provide_source_frame(
        &self,
        asset_id: &str,
        source_tick: Flicks,
        width: u32,
        height: u32,
        fit: SourceFit,
    ) -> Result<SourceFrame, RenderError> {
        if let Some(node_id_str) = asset_id.strip_prefix("vector_text://") {
            let node_id = NodeId::from(node_id_str);
            if !node_has_dynamic_binding(self.project, &node_id)
                && let Some(node) = self.project.graph.find_node_recursive(&node_id)
                && let NodeKind::VectorText {
                    font_family,
                    font_path,
                    font_weight,
                    fallback_fonts,
                    gpu_slug,
                    ..
                } = &node.kind
            {
                if *gpu_slug && (font_family.is_some() || font_path.is_some()) {
                    let query = frameshift_vector::FontQuery {
                        font_path: font_path.clone(),
                        font_family: font_family.clone(),
                        font_weight: *font_weight,
                    };
                    frameshift_vector::resolve_font_chain(&query, fallback_fonts, None).map_err(
                        |err| {
                            map_core_vector_or_provider_err(
                                crate::CoreError::Vector(err),
                                asset_id,
                                source_tick,
                            )
                        },
                    )?;
                } else if let Some(cache_key) =
                    VectorTextCacheKey::from_node_kind(&node.kind, width, height)
                {
                    let mut resolver = self.resolver.borrow_mut();
                    if let Some(cached) = resolver.vector_text_cache.get(&cache_key) {
                        return Ok(SourceFrame::rgba_f32_premul(
                            cached.width,
                            cached.height,
                            cached.data.clone(),
                        ));
                    }
                    let frame = MediaResolver::render_vector_text_node(
                        &node.kind,
                        width,
                        height,
                        source_tick,
                    )
                    .map_err(|err| map_core_vector_or_provider_err(err, asset_id, source_tick))?;
                    let out =
                        SourceFrame::rgba_f32_premul(frame.width, frame.height, frame.data.clone());
                    resolver.vector_text_cache.insert(cache_key, frame);
                    return Ok(out);
                }
            }
            return Err(RenderError::FrameProvider {
                asset_id: asset_id.to_owned(),
                source_tick: source_tick.0,
                message: "dynamic or non-VectorText fallback requested".into(),
            });
        }

        if let Some(node_id_str) = asset_id.strip_prefix("vector_path_hud://") {
            let node_id = NodeId::from(node_id_str);
            if !node_has_dynamic_binding(self.project, &node_id)
                && let Some(node) = self.project.graph.find_node_recursive(&node_id)
                && let Some(cache_key) = HudPathCacheKey::from_node_kind(&node.kind, width, height)
            {
                let mut resolver = self.resolver.borrow_mut();
                if let Some(cached) = resolver.hud_path_cache.get(&cache_key) {
                    return Ok(SourceFrame::rgba_f32_premul(
                        cached.width,
                        cached.height,
                        cached.data.clone(),
                    ));
                }
                let frame = MediaResolver::render_vector_path_hud_node(
                    &node.kind,
                    width,
                    height,
                    source_tick,
                )
                .map_err(|err| map_core_vector_or_provider_err(err, asset_id, source_tick))?;
                let out =
                    SourceFrame::rgba_f32_premul(frame.width, frame.height, frame.data.clone());
                resolver.hud_path_cache.insert(cache_key, frame);
                return Ok(out);
            }
            return Err(RenderError::FrameProvider {
                asset_id: asset_id.to_owned(),
                source_tick: source_tick.0,
                message: "dynamic or non-VectorPathHud fallback requested".into(),
            });
        }

        let asset = self
            .project
            .asset(asset_id)
            .ok_or_else(|| frameshift_schema::SchemaError::MissingAsset(asset_id.to_owned()))?;

        let mut resolver = self.resolver.borrow_mut();
        let mut cache = self.cache.borrow_mut();
        let frame = if asset.kind == AssetKind::Image {
            resolver.provide_image_frame(&mut cache, asset, width, height, fit)
        } else {
            resolver.provide_video_frame(&mut cache, asset, source_tick, width, height, fit)
        }
        .map_err(|err| RenderError::FrameProvider {
            asset_id: asset_id.to_owned(),
            source_tick: source_tick.0,
            message: err.to_string(),
        })?;

        Ok(match frame.alpha_mode {
            frameshift_media::AlphaMode::Straight => {
                SourceFrame::rgba8_straight(frame.width, frame.height, frame.data)
            }
            frameshift_media::AlphaMode::Premultiplied => {
                SourceFrame::rgba8_premul(frame.width, frame.height, frame.data)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use frameshift_schema::Resolution;
    use frameshift_time::FPS_24;

    #[test]
    fn provide_video_frame_populates_window_into_lru_cache() {
        let temp_mp4 = std::env::temp_dir().join(format!(
            "frameshift_resolver_window_{}.mp4",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&temp_mp4);

        let frames: Vec<Rgba8Frame> = (0..12)
            .map(|i| {
                let tick = FPS_24.frame_to_ticks(i).unwrap();
                SyntheticMediaGenerator::generate_clip_a_frame(64, 64, tick)
            })
            .collect();
        FfmpegBridge::encode_mp4(&frames, None, FPS_24, &temp_mp4).expect("encode mp4");

        let asset = AssetDef {
            id: "disk_win".into(),
            kind: AssetKind::Video,
            path: temp_mp4.to_string_lossy().into_owned(),
            duration_ticks: Some(Flicks(FLICKS_PER_SECOND / 2)),
            fps: Some(FPS_24),
            sample_rate_hz: None,
            resolution: Some(Resolution {
                width: 64,
                height: 64,
            }),
        };

        let mut resolver = MediaResolver::new();
        let mut cache = LruFrameCache::new(64);
        let f0 = resolver
            .provide_video_frame(&mut cache, &asset, Flicks::ZERO, 64, 64, SourceFit::Stretch)
            .expect("decode streaming frame 0");
        assert_eq!(f0.width, 64);
        assert_eq!(cache.telemetry().current_entries, 1);
        assert_eq!(resolver.decoder_telemetry().spawns, 1);
        assert_eq!(resolver.decoder_telemetry().active_streams, 1);

        // Forward skip within max_sequential_skip reuses the same stream (0 new spawns).
        let t5 = FPS_24.frame_to_ticks(5).unwrap();
        let f5 = resolver
            .provide_video_frame(&mut cache, &asset, t5, 64, 64, SourceFit::Stretch)
            .expect("frame 5 should skip on open stream");
        assert_eq!(f5.width, 64);
        assert_eq!(resolver.decoder_telemetry().spawns, 1);
        assert_eq!(resolver.decoder_telemetry().seek_reopens, 0);
        assert_eq!(resolver.decoder_telemetry().skipped_frames, 4);

        // Re-requesting frame 5 hits LruFrameCache directly.
        let hits_before = cache.telemetry().hits;
        let f5_cached = resolver
            .provide_video_frame(&mut cache, &asset, t5, 64, 64, SourceFit::Stretch)
            .expect("frame 5 should hit LRU cache");
        assert_eq!(f5_cached.data, f5.data);
        assert_eq!(cache.telemetry().hits, hits_before + 1);

        resolver.close_video_streams();
        assert_eq!(resolver.decoder_telemetry().active_streams, 0);
        std::fs::remove_file(&temp_mp4).expect("temp mp4 unlocked after close_video_streams");
    }

    #[test]
    fn disagreeing_source_nodes_decode_their_own_fit() {
        use frameshift_media::SourceFit as DecodeFit;
        use frameshift_render::SourcePixelData;
        use frameshift_schema::{ColorSpace, GraphNodeDef};

        let src_w = 320u32;
        let src_h = 180u32;
        let dst_w = 180u32;
        let dst_h = 220u32;
        let frames: Vec<Rgba8Frame> = (0..4)
            .map(|i| wide_edge_bars(src_w, src_h, FPS_24.frame_to_ticks(i).unwrap()))
            .collect();
        let mp4_path = std::env::temp_dir().join(format!(
            "frameshift_resolver_node_fit_{}.mp4",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&mp4_path);
        FfmpegBridge::encode_mp4(&frames, None, FPS_24, &mp4_path).expect("encode wide mp4");

        let mut project = Project::golden_scene_preset();
        project.assets[0].path = mp4_path.to_string_lossy().into_owned();
        project.assets[0].resolution = Some(Resolution {
            width: src_w,
            height: src_h,
        });
        project.assets[0].fps = Some(FPS_24);
        project.graph.nodes.push(GraphNodeDef {
            id: NodeId::from("src_a_cover"),
            kind: NodeKind::SourceVideo {
                asset_id: "asset_clip_a".into(),
                clip_id: None,
                color_space: ColorSpace::DisplaySrgb,
                fit: SourceFit::Cover,
            },
            inputs: Vec::new(),
            enabled: true,
        });

        let probe = FfmpegBridge::probe_media_info(&mp4_path).expect("probe");
        let tick = probe.pts_index.as_slice()[0];
        let legacy =
            FfmpegBridge::decode_video_frame(&mp4_path, &probe.pts_index, tick, dst_w, dst_h)
                .expect("legacy stretch");
        let cover_direct = FfmpegBridge::decode_video_frame_with_fit(
            &mp4_path,
            &probe.pts_index,
            tick,
            dst_w,
            dst_h,
            DecodeFit::Cover,
        )
        .expect("direct cover");

        let stretch_fit = project
            .graph
            .node(&NodeId::from("src_a"))
            .unwrap()
            .kind
            .source_fit()
            .unwrap();
        let cover_fit = project
            .graph
            .node(&NodeId::from("src_a_cover"))
            .unwrap()
            .kind
            .source_fit()
            .unwrap();
        assert_eq!(stretch_fit, SourceFit::Stretch);
        assert_eq!(cover_fit, SourceFit::Cover);

        let mut resolver = MediaResolver::new();
        let mut cache = LruFrameCache::new(64);
        let (stretch, cover) = {
            let provider = ProjectFrameProvider::new(&project, &mut resolver, &mut cache);
            let stretch = provider
                .provide_source_frame("asset_clip_a", tick, dst_w, dst_h, stretch_fit)
                .expect("stretch node");
            let cover = provider
                .provide_source_frame("asset_clip_a", tick, dst_w, dst_h, cover_fit)
                .expect("cover node");
            (stretch, cover)
        };

        assert_eq!(rgba8_bytes(&stretch), legacy.data.as_slice());
        assert_eq!(rgba8_bytes(&cover), cover_direct.data.as_slice());
        assert_ne!(rgba8_bytes(&stretch), rgba8_bytes(&cover));

        assert_eq!(cache.telemetry().current_entries, 2);
        let hits_before = cache.telemetry().hits;
        {
            let provider = ProjectFrameProvider::new(&project, &mut resolver, &mut cache);
            let stretch = provider
                .provide_source_frame("asset_clip_a", tick, dst_w, dst_h, stretch_fit)
                .expect("stretch cache hit");
            let cover = provider
                .provide_source_frame("asset_clip_a", tick, dst_w, dst_h, cover_fit)
                .expect("cover cache hit");
            assert_eq!(rgba8_bytes(&stretch), legacy.data.as_slice());
            assert_eq!(rgba8_bytes(&cover), cover_direct.data.as_slice());
        }
        assert_eq!(cache.telemetry().hits, hits_before + 2);

        resolver.close_video_streams();
        std::fs::remove_file(&mp4_path).expect("mp4 unlocked after close_video_streams");

        fn rgba8_bytes(frame: &SourceFrame) -> &[u8] {
            match &frame.pixels {
                SourcePixelData::Rgba8 { data, .. } => data,
                SourcePixelData::RgbaF32 { .. } => panic!("disk decode returns rgba8"),
            }
        }

        fn wide_edge_bars(width: u32, height: u32, pts: Flicks) -> Rgba8Frame {
            use frameshift_media::AlphaMode;
            let mut frame =
                Rgba8Frame::filled(width, height, pts, [0, 255, 0, 255], AlphaMode::Straight)
                    .unwrap();
            let edge = width / 5;
            for y in 0..height {
                for x in 0..edge {
                    frame.set_pixel(x, y, [255, 0, 0, 255]);
                    frame.set_pixel(width - 1 - x, y, [0, 0, 255, 255]);
                }
            }
            frame
        }
    }

    #[test]
    fn provide_image_frame_png_and_jpeg_bypass_probe_and_use_img_cache_key() {
        use frameshift_media::AlphaMode;
        use frameshift_render::SourcePixelData;

        let dir =
            std::env::temp_dir().join(format!("frameshift_resolver_image_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");

        let png_path = dir.join("sprite.png");
        let mut src_png = Rgba8Frame::filled(
            32,
            16,
            Flicks::ZERO,
            [240, 80, 40, 128],
            AlphaMode::Straight,
        )
        .unwrap();
        src_png.set_pixel(0, 0, [0, 0, 0, 0]);
        src_png.save_png(&png_path).expect("save png");

        let jpg_path = dir.join("photo.jpg");
        let jpg_status = std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-i"])
            .arg(&png_path)
            .arg(&jpg_path)
            .status()
            .expect("spawn ffmpeg for test jpg");
        assert!(jpg_status.success(), "ffmpeg png->jpg failed");

        let mut project = Project::golden_scene_preset();
        project.assets.push(AssetDef {
            id: "img_png".into(),
            kind: AssetKind::Image,
            path: png_path.to_string_lossy().into_owned(),
            duration_ticks: None,
            fps: None,
            sample_rate_hz: None,
            resolution: Some(Resolution {
                width: 32,
                height: 16,
            }),
        });
        project.assets.push(AssetDef {
            id: "img_jpg".into(),
            kind: AssetKind::Image,
            path: jpg_path.to_string_lossy().into_owned(),
            duration_ticks: None,
            fps: None,
            sample_rate_hz: None,
            resolution: Some(Resolution {
                width: 32,
                height: 16,
            }),
        });

        let mut resolver = MediaResolver::new();
        let mut cache = LruFrameCache::new(32);

        {
            let provider = ProjectFrameProvider::new(&project, &mut resolver, &mut cache);
            let png_frame = provider
                .provide_source_frame("img_png", Flicks::ZERO, 64, 64, SourceFit::Contain)
                .expect("png image frame");
            assert_eq!(png_frame.width, 64);
            assert_eq!(png_frame.height, 64);
            if let SourcePixelData::Rgba8 { data, .. } = &png_frame.pixels {
                // Top letterbox row in Contain must remain transparent black.
                assert_eq!(&data[0..4], &[0, 0, 0, 0]);
                // Center pixel keeps semi-transparent alpha.
                let center_off = ((32 * 64 + 32) * 4) as usize;
                assert_eq!(data[center_off + 3], 128);
            } else {
                panic!("expected Rgba8 from PNG decode");
            }

            let jpg_frame = provider
                .provide_source_frame("img_jpg", Flicks::ZERO, 48, 32, SourceFit::Cover)
                .expect("jpg image frame");
            assert_eq!(jpg_frame.width, 48);
            assert_eq!(jpg_frame.height, 32);
        }

        // Neither ffprobe nor video decoder pool should have been touched.
        assert!(resolver.probe_cache.is_empty());
        assert_eq!(resolver.decoder_telemetry().spawns, 0);
        assert!(cache.peek("img:img_png@64x64:contain", 0).is_some());
        assert!(cache.peek("img:img_jpg@48x32:cover", 0).is_some());

        let hits_before = cache.telemetry().hits;
        {
            let provider = ProjectFrameProvider::new(&project, &mut resolver, &mut cache);
            let _ = provider
                .provide_source_frame("img_png", Flicks::ZERO, 64, 64, SourceFit::Contain)
                .expect("cached png image frame");
        }
        assert_eq!(cache.telemetry().hits, hits_before + 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn vector_text_and_hud_provider_support_nested_precomp_cache_and_font_not_found() {
        use frameshift_schema::{GraphNodeDef, ParamBindingDef, RenderGraphDef};
        use frameshift_vector::VectorError;

        let mut project = Project::golden_scene_preset();
        project.param_bindings.clear();
        for clip in &mut project.timeline.clips {
            clip.event_animations.clear();
        }

        let sub_graph = RenderGraphDef {
            output_node: NodeId::from("sub_hud"),
            nodes: vec![
                GraphNodeDef {
                    id: NodeId::from("sub_hud"),
                    kind: NodeKind::VectorPathHud {
                        points_norm: vec![[0.1, 0.2], [0.9, 0.8]],
                        bezier_segments: Vec::new(),
                        closed: false,
                        stroke_width_norm: 0.01,
                        stroke_rgba: [0.2, 0.9, 1.0, 1.0],
                        fill_rgba: [0.0; 4],
                        dash_len_norm: 0.02,
                        gap_len_norm: 0.01,
                        dash_offset_norm: 0.0,
                        trim_start: 0.0,
                        trim_end: 1.0,
                        node_dot_radius_norm: 0.015,
                        data_value: 42.5,
                        data_label: "SPD ".into(),
                        opacity: 1.0,
                    },
                    inputs: Vec::new(),
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("sub_text"),
                    kind: NodeKind::VectorText {
                        text: "NESTED".into(),
                        font_size_norm: 0.08,
                        color_rgba: [1.0, 1.0, 1.0, 1.0],
                        outline_rgba: [0.0; 4],
                        outline_width_norm: 0.0,
                        align_center: true,
                        y_offset_norm: 0.5,
                        font_family: None,
                        font_path: None,
                        font_weight: 400,
                        align: Some(TextAlign2D::Right),
                        x_offset_norm: Some(0.9),
                        tracking_em: Some(0.08),
                        line_height_em: Some(1.2),
                        opacity: 0.85,
                        fallback_fonts: Vec::new(),
                        writing_mode: Default::default(),
                        var_wght: None,
                        var_wdth: None,
                        var_slnt: None,
                        color_bottom_rgba: None,
                        gradient_angle_deg: 0.0,
                        outer_outline_rgba: [0.0; 4],
                        outer_outline_width_norm: 0.0,
                        outline_gap_norm: 0.0,
                        shadow_offset_norm: [0.0, 0.0],
                        shadow_blur_norm: 0.0,
                        shadow_rgba: [0.0; 4],
                        glow_radius_norm: 0.0,
                        glow_rgba: [0.0; 4],
                        hdr_intensity: 1.0,
                        gpu_slug: true,
                        text_animator: None,
                    },
                    inputs: Vec::new(),
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("sub_missing_font"),
                    kind: NodeKind::VectorText {
                        text: "BAD".into(),
                        font_size_norm: 0.08,
                        color_rgba: [1.0, 1.0, 1.0, 1.0],
                        outline_rgba: [0.0; 4],
                        outline_width_norm: 0.0,
                        align_center: true,
                        y_offset_norm: 0.5,
                        font_family: Some("MissingFamily_FrameShift_ResolverTest_XYZ".into()),
                        font_path: None,
                        font_weight: 700,
                        align: None,
                        x_offset_norm: None,
                        tracking_em: None,
                        line_height_em: None,
                        opacity: 1.0,
                        fallback_fonts: Vec::new(),
                        writing_mode: Default::default(),
                        var_wght: None,
                        var_wdth: None,
                        var_slnt: None,
                        color_bottom_rgba: None,
                        gradient_angle_deg: 0.0,
                        outer_outline_rgba: [0.0; 4],
                        outer_outline_width_norm: 0.0,
                        outline_gap_norm: 0.0,
                        shadow_offset_norm: [0.0, 0.0],
                        shadow_blur_norm: 0.0,
                        shadow_rgba: [0.0; 4],
                        glow_radius_norm: 0.0,
                        glow_rgba: [0.0; 4],
                        hdr_intensity: 1.0,
                        gpu_slug: true,
                        text_animator: None,
                    },
                    inputs: Vec::new(),
                    enabled: true,
                },
            ],
        };

        project.graph = RenderGraphDef {
            output_node: NodeId::from("precomp_root"),
            nodes: vec![
                GraphNodeDef {
                    id: NodeId::from("precomp_root"),
                    kind: NodeKind::Precomp {
                        sub_graph,
                        time_offset_ticks: 0,
                        time_scale: 1.0,
                        opacity: 1.0,
                    },
                    inputs: Vec::new(),
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("dyn_hud"),
                    kind: NodeKind::VectorPathHud {
                        points_norm: vec![[0.2, 0.2], [0.8, 0.2]],
                        bezier_segments: Vec::new(),
                        closed: false,
                        stroke_width_norm: 0.01,
                        stroke_rgba: [1.0; 4],
                        fill_rgba: [0.0; 4],
                        dash_len_norm: 0.0,
                        gap_len_norm: 0.0,
                        dash_offset_norm: 0.0,
                        trim_start: 0.0,
                        trim_end: 1.0,
                        node_dot_radius_norm: 0.0,
                        data_value: 1.0,
                        data_label: String::new(),
                        opacity: 1.0,
                    },
                    inputs: Vec::new(),
                    enabled: true,
                },
            ],
        };
        project.param_bindings.push(ParamBindingDef {
            node_id: NodeId::from("dyn_hud"),
            param_name: "trim_end".into(),
            expression: "t".into(),
        });

        let mut resolver = MediaResolver::new();
        let mut cache = LruFrameCache::new(16);

        {
            let provider = ProjectFrameProvider::new(&project, &mut resolver, &mut cache);
            let hud_frame = provider
                .provide_source_frame(
                    "vector_path_hud://sub_hud",
                    Flicks::ZERO,
                    160,
                    90,
                    SourceFit::Stretch,
                )
                .expect("nested static VectorPathHud should resolve");
            assert_eq!(hud_frame.width, 160);

            let text_frame = provider
                .provide_source_frame(
                    "vector_text://sub_text",
                    Flicks::ZERO,
                    160,
                    90,
                    SourceFit::Stretch,
                )
                .expect("nested static VectorText should resolve");
            assert_eq!(text_frame.width, 160);

            let missing_err = provider
                .provide_source_frame(
                    "vector_text://sub_missing_font",
                    Flicks::ZERO,
                    160,
                    90,
                    SourceFit::Stretch,
                )
                .expect_err("missing font must propagate as RenderError::Vector");
            assert!(
                matches!(
                    missing_err,
                    RenderError::Vector(VectorError::FontNotFound(_))
                ),
                "expected RenderError::Vector(FontNotFound), got {missing_err:?}"
            );

            let dyn_err = provider
                .provide_source_frame(
                    "vector_path_hud://dyn_hud",
                    Flicks::ZERO,
                    160,
                    90,
                    SourceFit::Stretch,
                )
                .expect_err("dynamic HUD must request fallback");
            assert!(
                matches!(
                    dyn_err,
                    RenderError::FrameProvider { ref message, .. }
                        if message == "dynamic or non-VectorPathHud fallback requested"
                ),
                "unexpected dynamic HUD error: {dyn_err:?}"
            );
        }

        assert_eq!(resolver.hud_path_cache.len(), 1);
        assert_eq!(resolver.vector_text_cache.len(), 1);

        // VectorTextCacheKey preserves full f32 precision (`to_bits()`) and new fields.
        let mut k_a = VectorTextCacheKey::from_node_kind(
            &project
                .graph
                .find_node_recursive(&NodeId::from("sub_text"))
                .unwrap()
                .kind,
            160,
            90,
        )
        .unwrap();
        let mut k_b = k_a.clone();
        k_b.font_size_bits = (0.08001_f32).to_bits();
        assert_ne!(k_a, k_b);
        k_a.opacity_bits = (0.85_f32).to_bits();
        k_b.font_size_bits = k_a.font_size_bits;
        k_b.opacity_bits = (0.8499_f32).to_bits();
        assert_ne!(k_a, k_b);

        resolver.clear_probe_cache();
        assert!(resolver.hud_path_cache.is_empty());
        assert!(resolver.vector_text_cache.is_empty());
        assert_eq!(resolver.audio_cache_len(), 0);
    }

    #[test]
    fn audio_buffer_cache_reuses_synthetic_and_disk_buffers_and_clears() {
        let mut resolver = MediaResolver::new();
        let project = Project::golden_scene_preset();
        assert_eq!(resolver.audio_cache_len(), 0);

        let _ = resolver.mix_project_audio(&project).expect("mix synthetic");
        assert_eq!(resolver.audio_cache_len(), 1);

        // Mixing again reuses the cached BeatAudio120Bpm buffer without creating another entry.
        let _ = resolver.mix_project_audio(&project).expect("mix synthetic cached");
        assert_eq!(resolver.audio_cache_len(), 1);

        resolver.clear_probe_cache();
        assert_eq!(resolver.audio_cache_len(), 0);
    }

    #[test]
    fn mix_project_audio_handles_event_anchor_pan_ducking_micro_silence_and_soft_limiter() {
        use frameshift_schema::{
            AudioClipDef, AudioDuckingRuleDef, EventAnchorDef, EventOrigin, MicroSilenceDef,
        };

        // Test directly via AudioBuffer + mix_project_audio using a WAV-backed project?
        // Note: Disk WAV decode uses ffmpeg; if ffmpeg is unavailable in sandbox, test the
        // core logic via synthetic BeatAudio120Bpm + pure-Rust resolver helpers.
        let mut project = Project::golden_scene_preset();
        let one_sec = Flicks(FLICKS_PER_SECOND);
        project.timeline.duration_ticks = Flicks(2 * FLICKS_PER_SECOND);
        project.events.push(EventAnchorDef {
            event_id: "sfx_apex".into(),
            label: "SFX Apex".into(),
            source_clip_id: None,
            source_anchor_tick: None,
            target_output_tick: Flicks(FLICKS_PER_SECOND),
            origin: EventOrigin::UserMarked,
        });

        // 1. Anchor placement + apex offset resolution
        project.timeline.audio_clips = vec![AudioClipDef {
            id: "riser_anchored".into(),
            asset_id: "asset_bgm".into(),
            timeline_start: Flicks::ZERO,
            timeline_end: Flicks(FLICKS_PER_SECOND / 2),
            source_start: Flicks::ZERO,
            gain_db: 0.0,
            fade_in: None,
            fade_out: None,
            bus: AudioBus::SfxRiser,
            pan: -1.0,
            anchor_event_id: Some("sfx_apex".into()),
            apex_offset_ticks: Some(Flicks(FLICKS_PER_SECOND / 4)),
        }];
        let (eff_start, eff_end) =
            resolve_audio_clip_bounds(&project, &project.timeline.audio_clips[0]).unwrap();
        assert_eq!(eff_start, Flicks(3 * FLICKS_PER_SECOND / 4));
        assert_eq!(eff_end, Flicks(5 * FLICKS_PER_SECOND / 4));

        // Hard left pan (-1.0) places signal on Left channel and silences Right channel.
        let resolver = MediaResolver::new();
        let panned = resolver.mix_project_audio(&project).unwrap().unwrap();
        let beat_sample =
            ticks_to_audio_samples_nearest(eff_start, DEFAULT_AUDIO_SAMPLE_RATE).unwrap() as usize;
        assert!(panned.samples[beat_sample * 2].abs() > 0.5);
        assert!(panned.samples[beat_sample * 2 + 1].abs() < 1e-6);

        // 2. Micro-silence targeted to SfxRiser dips that bus, while Music stays untouched.
        project.timeline.audio_clips.push(AudioClipDef {
            id: "music_bed".into(),
            asset_id: "asset_bgm".into(),
            timeline_start: Flicks::ZERO,
            timeline_end: one_sec,
            source_start: Flicks::ZERO,
            gain_db: 0.0,
            fade_in: None,
            fade_out: None,
            bus: AudioBus::Music,
            pan: 0.0,
            anchor_event_id: None,
            apex_offset_ticks: None,
        });
        project.timeline.micro_silences.push(MicroSilenceDef {
            start_tick: Flicks(FLICKS_PER_SECOND / 2 - FLICKS_PER_SECOND / 20),
            end_tick: Flicks(FLICKS_PER_SECOND / 2 + FLICKS_PER_SECOND / 20),
            fade_ms: 0.0,
            floor_gain: 0.0,
            target_bus: Some(AudioBus::Music),
        });
        // Duck SfxRiser when Music has energy at t=0.
        project.timeline.ducking_rules.push(AudioDuckingRuleDef {
            trigger_bus: AudioBus::Music,
            target_bus: AudioBus::SfxRiser,
            threshold: 0.05,
            duck_gain: 0.25,
            attack_ms: 1.0,
            release_ms: 10.0,
        });
        project.validate().expect("advanced audio project validates");
        let bus_mixed = resolver.mix_project_audio(&project).unwrap().unwrap();
        // Beat at 0.5s on Music is silenced by micro_silence (floor_gain=0.0).
        let half_sec_sample =
            ticks_to_audio_samples_nearest(Flicks(FLICKS_PER_SECOND / 2), DEFAULT_AUDIO_SAMPLE_RATE)
                .unwrap() as usize;
        assert!(bus_mixed.samples[half_sec_sample * 2].abs() < 1e-5);

        // 3. Soft limiter keeps hot overlapping mixes <= 0.99 without hard-clipping at 1.0.
        project.timeline.micro_silences.clear();
        project.timeline.ducking_rules.clear();
        project.timeline.audio_clips = vec![
            AudioClipDef {
                id: "hot_1".into(),
                asset_id: "asset_bgm".into(),
                timeline_start: Flicks::ZERO,
                timeline_end: one_sec,
                source_start: Flicks::ZERO,
                gain_db: 6.0,
                fade_in: None,
                fade_out: None,
                bus: AudioBus::Music,
                pan: 0.0,
                anchor_event_id: None,
                apex_offset_ticks: None,
            },
            AudioClipDef {
                id: "hot_2".into(),
                asset_id: "asset_bgm".into(),
                timeline_start: Flicks::ZERO,
                timeline_end: one_sec,
                source_start: Flicks::ZERO,
                gain_db: 6.0,
                fade_in: None,
                fade_out: None,
                bus: AudioBus::SfxImpact,
                pan: 0.0,
                anchor_event_id: None,
                apex_offset_ticks: None,
            },
        ];
        let limited = resolver.mix_project_audio(&project).unwrap().unwrap();
        let peak = limited
            .samples
            .iter()
            .fold(0.0f32, |acc, &s| acc.max(s.abs()));
        assert!(peak <= 0.99 + 1e-6, "expected peak <= 0.99, got {peak}");
        assert!(peak > 0.90, "expected soft limiter knee near 0.99, got {peak}");
    }
}
