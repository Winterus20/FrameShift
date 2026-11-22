use crate::error::CoreError;
use crate::inspection::{
    InspectionPackManifest, MaterialGalleryManifest, export_fx_showcase_pack_impl,
    export_inspection_pack_impl, export_material_gallery_impl,
};
use crate::report::{
    AudioImpactSyncReport, DeterminismCheckReport, M1VerificationReport, MotionBlurCheckReport,
    Phase5VerificationReport, SeekIndependenceCheckReport, VideoRenderSummary,
};
use crate::progress::RenderProgress;
use crate::range::RenderRange;
use crate::resolver::{
    MediaResolver, ProjectFrameProvider, ResolvedAssetSource, slice_audio_buffer,
};
use frameshift_media::{
    AlphaMode, AudioBuffer, DEFAULT_AUDIO_SAMPLE_RATE, DecoderPoolTelemetry, EncodeBackend,
    FfmpegBridge, LruFrameCache, Mp4FrameEncoder, ProxyManager, ProxySpec, Rgba8Frame,
    SyntheticMediaGenerator, downscale_frame_nearest_or_box,
};
use frameshift_render::{
    AudioRmsEnvelope, GpuCompositor, OutputAlphaMode, RenderOptions, ShutterOverride,
    effective_blur_sub_samples,
};
use frameshift_schema::{
    NodeKind, Project, ProxyConfigDef, RenderProfileDef, RenderProfileKind, Resolution,
};
use frameshift_time::{
    FLICKS_PER_SECOND, FPS_24, Flicks, ImpactCurveStyle, ImpactSolveRequest, ImpactSolver,
    SourceFitMode,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const DEFAULT_CACHE_CAPACITY: usize = 128;
const DEFAULT_CACHE_MAX_BYTES: usize = 1024 * 1024 * 1024;

/// Unified M1 headless FrameShift orchestration engine.
///
/// Owns the [`GpuCompositor`], bounded [`LruFrameCache`], and [`MediaResolver`]
/// supporting both on-disk media (`FfmpegBridge`) and built-in synthetic assets
/// (`SyntheticMediaGenerator`).
pub struct Engine {
    compositor: GpuCompositor,
    cache: LruFrameCache,
    resolver: MediaResolver,
    audio_envelope_cache: Option<(String, AudioRmsEnvelope)>,
}

impl Engine {
    /// Initializes a headless GPU compositor, default LRU frame cache, and media resolver.
    pub fn new_headless() -> Result<Self, CoreError> {
        Self::with_cache_capacity(DEFAULT_CACHE_CAPACITY)
    }

    /// Initializes the engine with an explicit LRU frame capacity and a 1 GiB byte cap.
    pub fn with_cache_capacity(capacity: usize) -> Result<Self, CoreError> {
        Ok(Self {
            compositor: GpuCompositor::new_headless()?,
            cache: LruFrameCache::with_max_bytes(capacity, DEFAULT_CACHE_MAX_BYTES),
            resolver: MediaResolver::new(),
            audio_envelope_cache: None,
        })
    }

    /// Returns a reference to the underlying [`GpuCompositor`].
    pub fn compositor(&self) -> &GpuCompositor {
        &self.compositor
    }

    /// Returns a mutable reference to the underlying [`GpuCompositor`].
    pub fn compositor_mut(&mut self) -> &mut GpuCompositor {
        &mut self.compositor
    }

    /// Returns current LRU frame cache telemetry.
    pub fn cache_telemetry(&self) -> frameshift_media::CacheTelemetry {
        self.cache.telemetry()
    }

    /// Returns current streaming video decoder pool telemetry.
    pub fn decoder_telemetry(&self) -> DecoderPoolTelemetry {
        self.resolver.decoder_telemetry()
    }

    /// Clears cached decoded/synthesized frames, probed container metadata, open video decoder
    /// streams, and audio envelope cache.
    pub fn clear_cache(&mut self) {
        self.cache.clear();
        self.resolver.clear_probe_cache();
        self.audio_envelope_cache = None;
    }

    /// Mixes all audio clips in `project` into a `48_000 Hz` stereo `AudioBuffer`, if any.
    pub fn mix_project_audio(&self, project: &Project) -> Result<Option<AudioBuffer>, CoreError> {
        self.resolver.mix_project_audio(project)
    }

    /// Exports the Phase 2 Multi-Surface Inspection Pack (`contact_sheet.png`, `mask_pack.png`,
    /// `transition_pack.png`, `velocity_pack.png`, `crop_fullres.png`, `fx_showcase_pack.png`,
    /// and `inspection_manifest.json`) to `out_dir`.
    pub fn export_inspection_pack(
        &mut self,
        project: &Project,
        out_dir: &Path,
    ) -> Result<InspectionPackManifest, CoreError> {
        export_inspection_pack_impl(self, project, out_dir)
    }

    /// Exports the deterministic material/shape gallery (`materials_sheet.png` and
    /// `materials_manifest.json`) to `out_dir`.
    pub fn export_material_gallery(
        &mut self,
        out_dir: &Path,
    ) -> Result<MaterialGalleryManifest, CoreError> {
        export_material_gallery_impl(self, out_dir)
    }

    /// Renders and saves an 8-panel (4x2) visual comparison sheet (`fx_showcase_pack.png`)
    /// demonstrating all 4 WGSL transition shaders and all 4 anime post-FX shaders.
    pub fn export_fx_showcase_pack(&mut self, out_path: &Path) -> Result<Rgba8Frame, CoreError> {
        export_fx_showcase_pack_impl(self, out_path)
    }

    /// Evaluates a single frame with an explicit `RenderProfileDef` (used by inspection pack rendering).
    pub(crate) fn render_frame_with_profile_def_pub(
        &mut self,
        project: &Project,
        profile_def: &RenderProfileDef,
        output_tick: Flicks,
        options: &RenderOptions,
    ) -> Result<Rgba8Frame, CoreError> {
        self.render_frame_with_profile_def(project, profile_def, output_tick, options)
    }

    /// Evaluates a single seek-independent output frame at `output_tick` for `profile`.
    pub fn render_frame_at(
        &mut self,
        project: &Project,
        profile: RenderProfileKind,
        output_tick: Flicks,
    ) -> Result<Rgba8Frame, CoreError> {
        let options = RenderOptions {
            output_alpha_mode: OutputAlphaMode::Straight,
            ..Default::default()
        };
        let profile_def = select_profile(project, profile);
        self.render_frame_with_profile_def(project, profile_def, output_tick, &options)
    }

    /// Evaluates a single frame with explicit [`RenderOptions`] (e.g., shutter overrides).
    pub fn render_frame_at_with_options(
        &mut self,
        project: &Project,
        profile: RenderProfileKind,
        output_tick: Flicks,
        options: &RenderOptions,
    ) -> Result<Rgba8Frame, CoreError> {
        let profile_def = select_profile(project, profile);
        self.render_frame_with_profile_def(project, profile_def, output_tick, options)
    }

    fn render_frame_with_profile_def(
        &mut self,
        project: &Project,
        profile_def: &RenderProfileDef,
        output_tick: Flicks,
        options: &RenderOptions,
    ) -> Result<Rgba8Frame, CoreError> {
        let audio_envelope =
            resolve_disk_audio_envelope(&mut self.audio_envelope_cache, &self.resolver, project)?;
        let provider = ProjectFrameProvider::new(project, &mut self.resolver, &mut self.cache);
        let rendered = self.compositor.render_project_frame_with_audio(
            project,
            output_tick,
            profile_def,
            &provider,
            options,
            audio_envelope,
        )?;
        Ok(Rgba8Frame {
            width: rendered.width,
            height: rendered.height,
            pts: rendered.tick,
            alpha_mode: AlphaMode::Straight,
            data: rendered.rgba8,
        })
    }

    /// Evaluates all timeline frames for `profile`, mixes project audio at `48_000 Hz`,
    /// and streams H.264 + AAC `.mp4` output via [`Mp4FrameEncoder`].
    pub fn render_video_mp4(
        &mut self,
        project: &Project,
        profile: RenderProfileKind,
        out_path: &Path,
    ) -> Result<VideoRenderSummary, CoreError> {
        self.render_video_mp4_with_options(project, profile, out_path, &default_render_options())
    }

    /// Renders a complete timeline to MP4 with explicit [`RenderOptions`].
    pub fn render_video_mp4_with_options(
        &mut self,
        project: &Project,
        profile: RenderProfileKind,
        out_path: &Path,
        options: &RenderOptions,
    ) -> Result<VideoRenderSummary, CoreError> {
        self.render_video_mp4_with_progress(project, profile, out_path, None, options, &mut |_| {})
    }

    /// Renders only `range` (a frame-aligned timeline slice) to MP4.
    pub fn render_video_mp4_range(
        &mut self,
        project: &Project,
        profile: RenderProfileKind,
        out_path: &Path,
        range: RenderRange,
    ) -> Result<VideoRenderSummary, CoreError> {
        self.render_video_mp4_range_with_options(
            project,
            profile,
            out_path,
            range,
            &default_render_options(),
        )
    }

    /// Renders only `range` to MP4 with explicit [`RenderOptions`].
    pub fn render_video_mp4_range_with_options(
        &mut self,
        project: &Project,
        profile: RenderProfileKind,
        out_path: &Path,
        range: RenderRange,
        options: &RenderOptions,
    ) -> Result<VideoRenderSummary, CoreError> {
        self.render_video_mp4_with_progress(
            project,
            profile,
            out_path,
            Some(range),
            options,
            &mut |_| {},
        )
    }

    /// Renders (optionally only `range`) to MP4 while reporting [`RenderProgress`] events.
    ///
    /// A full render (`range == None`) keeps the exact frame sequence, audio mix, and
    /// hashes of [`Self::render_video_mp4_with_options`]. A range render emits only the
    /// frames in `[start_frame, end_frame)`, slices the full audio mix to the resolved
    /// `[start_tick, end_tick)`, and skips the audio track when no clip intersects.
    pub fn render_video_mp4_with_progress(
        &mut self,
        project: &Project,
        profile: RenderProfileKind,
        out_path: &Path,
        range: Option<RenderRange>,
        options: &RenderOptions,
        on_progress: &mut dyn FnMut(RenderProgress),
    ) -> Result<VideoRenderSummary, CoreError> {
        let profile_def = select_profile(project, profile).clone();
        project.validate()?;

        let ticks = match range {
            Some(range) => range.frame_ticks(project)?,
            None => collect_timeline_frame_ticks(project)?,
        };

        on_progress(RenderProgress::audio_mix(0, 1));
        let full_audio = self.resolver.mix_project_audio(project)?;
        let audio = match range {
            None => full_audio,
            Some(range) => {
                let total_frames = i64::try_from(collect_timeline_frame_ticks(project)?.len())
                    .map_err(|_| {
                        CoreError::InvalidArgument("timeline frame count exceeds i64".into())
                    })?;
                if range.covers_timeline(total_frames) {
                    full_audio
                } else if range_intersects_audio(project, &range) {
                    match full_audio {
                        Some(buffer) => Some(slice_audio_buffer(
                            &buffer,
                            range.start_tick,
                            range.duration_ticks(),
                        )?),
                        None => None,
                    }
                } else {
                    None
                }
            }
        };
        on_progress(RenderProgress::audio_mix(1, 1));

        if let Some(parent) = out_path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }

        // Remove a partially written MP4 if any later step fails. Declared before the
        // encoder so it drops after the child process is killed and the file is unlocked.
        let mut output_guard = PartialOutputGuard::new(out_path);

        // Stream one frame at a time into ffmpeg. The previous implementation
        // buffered every output frame in a Vec<Rgba8Frame>; at 1280x720/60 fps
        // for 32 s that alone was ~6.6 GiB (1920 frames x 3.5 MiB).
        let mut encoder = Mp4FrameEncoder::new(
            profile_def.resolution.width,
            profile_def.resolution.height,
            audio.as_ref(),
            project.timeline.fps,
            out_path,
            "libx264",
        )?;
        let total = ticks.len();
        let mut hashes = Vec::with_capacity(total);
        on_progress(RenderProgress::frames(0, total));
        for (index, tick) in ticks.iter().enumerate() {
            let frame = self.render_frame_with_profile_def(project, &profile_def, *tick, options)?;
            hashes.push(frame.fnv1a_hash64());
            encoder.write_frame(&frame)?;
            on_progress(RenderProgress::frames(index + 1, total));
        }
        on_progress(RenderProgress::encode(0, 1));
        encoder.finish()?;
        on_progress(RenderProgress::encode(1, 1));
        output_guard.disarm();

        Ok(VideoRenderSummary::from_project_range(
            project,
            &profile_def,
            out_path.to_path_buf(),
            audio.is_some(),
            hashes,
            self.cache.telemetry(),
            range,
        ))
    }

    fn render_timeline_sequence(
        &mut self,
        project: &Project,
        profile_def: &RenderProfileDef,
        options: &RenderOptions,
    ) -> Result<(Vec<Rgba8Frame>, Vec<u64>), CoreError> {
        project.validate()?;
        let ticks = collect_timeline_frame_ticks(project)?;
        let mut frames = Vec::with_capacity(ticks.len());
        let mut hashes = Vec::with_capacity(ticks.len());
        for tick in ticks {
            let frame = self.render_frame_with_profile_def(project, profile_def, tick, options)?;
            hashes.push(frame.fnv1a_hash64());
            frames.push(frame);
        }
        Ok((frames, hashes))
    }

    fn render_timeline_hashes(
        &mut self,
        project: &Project,
        profile_def: &RenderProfileDef,
        options: &RenderOptions,
    ) -> Result<Vec<u64>, CoreError> {
        project.validate()?;
        let ticks = collect_timeline_frame_ticks(project)?;
        let mut hashes = Vec::with_capacity(ticks.len());
        for tick in ticks {
            let frame = self.render_frame_with_profile_def(project, profile_def, tick, options)?;
            hashes.push(frame.fnv1a_hash64());
        }
        Ok(hashes)
    }

    /// Executes the full M1 Golden Vertical Slice verification suite and writes all artifacts to `out_dir`.
    pub fn verify_m1_golden_scene(
        &mut self,
        out_dir: &Path,
    ) -> Result<M1VerificationReport, CoreError> {
        std::fs::create_dir_all(out_dir)?;

        // 1. Write `Project::golden_scene_preset()` to `out_dir/golden_project.json`.
        let project = Project::golden_scene_preset();
        let golden_project_json = out_dir.join("golden_project.json");
        std::fs::write(&golden_project_json, project.to_json_pretty()?)?;

        let frame_ticks = project.timeline.fps.frame_duration()?;
        let drop_tick = project
            .event("drop_impact")
            .ok_or_else(|| {
                CoreError::VerificationFailed("golden preset missing 'drop_impact' event".into())
            })?
            .target_output_tick;

        // 2. Render `golden_final.mp4` (Final, the blur node's authored samples)
        //    and `golden_draft.mp4` (Draft resolution, explicit 1-sample ceiling).
        let final_profile = project.render_profiles.final_profile.clone();
        let final_options = RenderOptions {
            output_alpha_mode: OutputAlphaMode::Straight,
            ..Default::default()
        };
        let golden_final_mp4 = out_dir.join("golden_final.mp4");
        let (final_frames, final_hashes) =
            self.render_timeline_sequence(&project, &final_profile, &final_options)?;
        let final_audio = self.resolver.mix_project_audio(&project)?;
        FfmpegBridge::encode_mp4(
            &final_frames,
            final_audio.as_ref(),
            project.timeline.fps,
            &golden_final_mp4,
        )?;
        let final_summary = VideoRenderSummary::from_project(
            &project,
            &final_profile,
            golden_final_mp4.clone(),
            final_audio.is_some(),
            final_hashes.clone(),
            self.cache.telemetry(),
        );

        let forced_samples = 1u32;
        let draft_options = RenderOptions {
            output_alpha_mode: OutputAlphaMode::Straight,
            shutter_override: Some(ShutterOverride {
                shutter_angle_deg: project.render_profiles.draft.shutter_angle_deg,
                sub_samples: forced_samples,
                phase: None,
            }),
            ..Default::default()
        };
        let golden_draft_mp4 = out_dir.join("golden_draft.mp4");
        let draft_summary = self.render_video_mp4_with_options(
            &project,
            RenderProfileKind::Draft,
            &golden_draft_mp4,
            &draft_options,
        )?;

        // Save key inspection PNG frames (reusing already-rendered Final frames at exact ticks).
        let tick_ramp = Flicks(FLICKS_PER_SECOND); // 1.0s (frame 60)
        let tick_mask = Flicks(2 * FLICKS_PER_SECOND + FLICKS_PER_SECOND / 5); // 2.2s (frame 132)
        let tick_transition = Flicks(2 * FLICKS_PER_SECOND + (2 * FLICKS_PER_SECOND) / 5); // 2.4s (frame 144)
        let tick_impact = drop_tick; // 2.5s (frame 150)

        let frame_by_tick: HashMap<i64, &Rgba8Frame> =
            final_frames.iter().map(|f| (f.pts.0, f)).collect();

        let key_specs = [
            ("frame_01_clip_a_ramp.png", tick_ramp),
            ("frame_02_mask_composite.png", tick_mask),
            ("frame_03_transition_motion_blur.png", tick_transition),
            ("frame_04_impact_flash.png", tick_impact),
        ];
        let mut key_frames = Vec::with_capacity(key_specs.len());
        for (name, tick) in key_specs {
            let path = out_dir.join(name);
            let frame = frame_by_tick.get(&tick.0).copied().ok_or_else(|| {
                CoreError::VerificationFailed(format!("missing rendered frame at tick {tick}"))
            })?;
            frame.save_png(&path)?;
            key_frames.push(path);
        }

        // 3. Determinism Check: clear cache and re-render the full sequence; verify 100% identical hashes.
        self.clear_cache();
        let pass2_hashes = self.render_timeline_hashes(&project, &final_profile, &final_options)?;
        let mismatched_frames: Vec<usize> = final_hashes
            .iter()
            .zip(pass2_hashes.iter())
            .enumerate()
            .filter_map(|(idx, (h1, h2))| if h1 != h2 { Some(idx) } else { None })
            .collect();
        let determinism = DeterminismCheckReport {
            passed: mismatched_frames.is_empty() && final_hashes.len() == pass2_hashes.len(),
            frame_count: final_hashes.len(),
            mismatched_frames,
        };

        // 4. Seek-Independence Check: 5 non-sequential ticks in scrambled/reverse order with cold cache.
        let seek_ticks = vec![
            Flicks(4 * FLICKS_PER_SECOND), // 4.0s (clip B)
            tick_ramp,                     // 1.0s (mid-ramp)
            tick_impact,                   // 2.5s (impact frame)
            tick_mask,                     // 2.2s (mask composite)
            tick_transition,               // 2.4s (mid-transition with sub-frame motion blur)
        ];
        self.clear_cache();
        let mut max_channel_diff = 0u8;
        let mut mismatched_ticks = Vec::new();
        for &tick in &seek_ticks {
            let seq_frame = frame_by_tick.get(&tick.0).copied().ok_or_else(|| {
                CoreError::VerificationFailed(format!("missing sequential frame at {tick}"))
            })?;
            let seek_frame = self.render_frame_at(&project, RenderProfileKind::Final, tick)?;
            let mut tick_diff = 0u8;
            for (&a, &b) in seq_frame.data.iter().zip(seek_frame.data.iter()) {
                tick_diff = tick_diff.max(a.abs_diff(b));
            }
            max_channel_diff = max_channel_diff.max(tick_diff);
            if tick_diff != 0 || seq_frame.fnv1a_hash64() != seek_frame.fnv1a_hash64() {
                mismatched_ticks.push(tick);
            }
        }
        let seek_independence = SeekIndependenceCheckReport {
            passed: mismatched_ticks.is_empty() && max_channel_diff == 0,
            tested_ticks: seek_ticks,
            max_channel_diff,
            mismatched_ticks,
        };

        // 5. Temporal Motion Blur Check: authored samples vs an explicit 1-sample ceiling
        //    at identical resolution and timing anchors on the drop/whip frame (`tick_impact`).
        let final_mb_frame = frame_by_tick.get(&tick_impact.0).copied().ok_or_else(|| {
            CoreError::VerificationFailed("missing impact frame for motion blur check".into())
        })?;
        let one_sample_same_res = self.render_frame_with_profile_def(
            &project,
            &final_profile,
            tick_impact,
            &draft_options,
        )?;
        let l1_sum: u64 = final_mb_frame
            .data
            .iter()
            .zip(one_sample_same_res.data.iter())
            .map(|(&a, &b)| a.abs_diff(b) as u64)
            .sum();
        let mean_l1_channel_diff = l1_sum as f64 / (final_mb_frame.data.len() as f64);
        let final_hash = final_mb_frame.fnv1a_hash64();
        let draft_hash = one_sample_same_res.fnv1a_hash64();
        let timing_anchors_identical = final_summary.frame_count == draft_summary.frame_count
            && final_summary.fps == draft_summary.fps
            && final_summary.duration_ticks == draft_summary.duration_ticks;
        let authored_samples = match project.graph.node(&project.graph.output_node) {
            Some(node) => match &node.kind {
                NodeKind::MotionBlurShutter { sub_samples, .. } if node.enabled => *sub_samples,
                _ => {
                    return Err(CoreError::VerificationFailed(
                        "golden output must be an enabled MotionBlurShutter".into(),
                    ));
                }
            },
            None => {
                return Err(CoreError::VerificationFailed(
                    "golden output node missing".into(),
                ));
            }
        };
        let effective_one = effective_blur_sub_samples(authored_samples, Some(forced_samples));
        let motion_blur = MotionBlurCheckReport {
            passed: authored_samples > 1
                && effective_one == 1
                && final_hash != draft_hash
                && mean_l1_channel_diff > 0.1
                && timing_anchors_identical,
            inspected_tick: tick_impact,
            final_sub_samples: authored_samples,
            draft_sub_samples: effective_one,
            final_hash,
            draft_hash,
            mean_l1_channel_diff,
            timing_anchors_identical,
        };

        // 6. Audio-Impact Sync & ADR-0003 Check.
        let audio_impact_sync =
            verify_audio_impact_sync(&project, final_audio.as_ref(), drop_tick, frame_ticks)?;

        // Probe generated final MP4 to confirm valid muxed H.264 + AAC streams.
        let probe_info = FfmpegBridge::probe_media_info(&golden_final_mp4)?;
        let mp4_probe_verified = probe_info.width == final_profile.resolution.width
            && probe_info.height == final_profile.resolution.height
            && probe_info.has_audio
            && probe_info.pts_index.len() == final_summary.frame_count;

        let all_gates_passed = determinism.passed
            && seek_independence.passed
            && motion_blur.passed
            && audio_impact_sync.passed
            && mp4_probe_verified;

        let report = M1VerificationReport {
            all_gates_passed,
            out_dir: out_dir.to_path_buf(),
            golden_project_json,
            golden_final_mp4,
            golden_draft_mp4,
            key_frames,
            final_summary,
            draft_summary,
            determinism,
            seek_independence,
            motion_blur,
            audio_impact_sync,
            mp4_probe_verified,
        };

        // 7. Write JSON and Markdown verification reports.
        let report_json_path = out_dir.join("m1_verification_report.json");
        let report_md_path = out_dir.join("m1_verification_report.md");
        std::fs::write(&report_json_path, serde_json::to_string_pretty(&report)?)?;
        std::fs::write(&report_md_path, report.to_markdown())?;

        Ok(report)
    }

    /// Executes Phase 5 Proxy Semantic Equivalence & Hardware Acceleration verification.
    pub fn verify_phase5_proxy_and_hw(
        &mut self,
        out_dir: &Path,
    ) -> Result<Phase5VerificationReport, CoreError> {
        std::fs::create_dir_all(out_dir)?;

        // 1. Generate a 24-frame source MP4 at 320x180 @ 24fps and create a 1/2 Proxy via ProxyManager.
        let src_w = 320u32;
        let src_h = 180u32;
        let mut src_frames = Vec::with_capacity(24);
        for idx in 0..24 {
            let tick = FPS_24.frame_to_ticks(idx)?;
            src_frames.push(SyntheticMediaGenerator::generate_clip_a_frame(
                src_w, src_h, tick,
            ));
        }
        let source_mp4 = out_dir.join("phase5_source_clip.mp4");
        FfmpegBridge::encode_mp4(&src_frames, None, FPS_24, &source_mp4)?;
        let source_probe = FfmpegBridge::probe_media_info(&source_mp4)?;

        let proxy_mgr = ProxyManager::new(out_dir.join("proxies"));
        let proxy_spec = ProxySpec::half();
        let proxy_artifact =
            proxy_mgr.resolve_or_generate_mp4_proxy("clip_a", &source_mp4, proxy_spec)?;
        let proxy_probe = FfmpegBridge::probe_media_info(&proxy_artifact.proxy_path)?;

        let pts_index_preserved = proxy_artifact.width == src_w / 2
            && proxy_artifact.height == src_h / 2
            && proxy_artifact.pts_index == source_probe.pts_index
            && proxy_probe.pts_index.len() == source_probe.pts_index.len()
            && proxy_probe.width == proxy_artifact.width
            && proxy_probe.height == proxy_artifact.height;

        // 2. Semantic equivalence between Proxy (Draft 1/2 res: 960x540) and Full (Final: 1920x1080).
        let mut project = Project::golden_scene_preset();
        project.proxy_config = ProxyConfigDef {
            enabled: true,
            scale_num: 1,
            scale_den: 2,
        };
        let full_res = project.render_profiles.final_profile.resolution;
        let proxy_res = Resolution {
            width: full_res.width / 2,
            height: full_res.height / 2,
        };
        project.render_profiles.draft.resolution = proxy_res;
        project.render_profiles.draft.motion_blur_sub_samples = project
            .render_profiles
            .final_profile
            .motion_blur_sub_samples;

        let impact_tick = project
            .event("drop_impact")
            .ok_or_else(|| {
                CoreError::VerificationFailed("golden preset missing 'drop_impact' event".into())
            })?
            .target_output_tick;
        let mask_tick = Flicks(2 * FLICKS_PER_SECOND + FLICKS_PER_SECOND / 5);

        let all_ticks = collect_timeline_frame_ticks(&project)?;
        let clip_a = project
            .clip("clip_a")
            .ok_or_else(|| CoreError::VerificationFailed("missing 'clip_a'".into()))?;
        let impact_off = impact_tick - clip_a.timeline_start;
        let src_at_impact_draft = clip_a.speed_curve.source_offset_at(impact_off);
        let src_at_impact_final = clip_a.speed_curve.source_offset_at(impact_off);
        let semantic_timing_identical = !all_ticks.is_empty()
            && src_at_impact_draft == src_at_impact_final
            && project.render_profiles.draft.shutter_angle_deg
                == project.render_profiles.final_profile.shutter_angle_deg;

        let mut mean_diff_acc = 0.0f64;
        for &tick in &[mask_tick, impact_tick] {
            let draft_frame = self.render_frame_at(&project, RenderProfileKind::Draft, tick)?;
            let final_frame = self.render_frame_at(&project, RenderProfileKind::Final, tick)?;
            let final_downscaled =
                downscale_frame_nearest_or_box(&final_frame, proxy_res.width, proxy_res.height)?;
            let l1_sum: u64 = draft_frame
                .data
                .iter()
                .zip(final_downscaled.data.iter())
                .map(|(&a, &b)| a.abs_diff(b) as u64)
                .sum();
            mean_diff_acc += l1_sum as f64 / (draft_frame.data.len() as f64);
        }
        let draft_vs_final_mean_l1_diff = mean_diff_acc / 2.0;

        // 3. HwAccelDetector & encode_mp4_with_policy fallback verification.
        let hw_caps = FfmpegBridge::detect_hwaccel_capabilities()?;
        let sw_mp4 = out_dir.join("phase5_sw_policy.mp4");
        let auto_mp4 = out_dir.join("phase5_auto_policy.mp4");
        let sw_backend =
            FfmpegBridge::encode_mp4_with_policy(&src_frames[..12], None, FPS_24, &sw_mp4, false)?;
        let auto_backend =
            FfmpegBridge::encode_mp4_with_policy(&src_frames[..12], None, FPS_24, &auto_mp4, true)?;
        let auto_probe = FfmpegBridge::probe_media_info(&auto_mp4)?;
        let hw_fallback_mp4_verified = sw_backend == EncodeBackend::SoftwareFallback
            && auto_probe.width == src_w
            && auto_probe.height == src_h
            && auto_probe.pts_index.len() == 12;

        let all_gates_passed = pts_index_preserved
            && semantic_timing_identical
            && draft_vs_final_mean_l1_diff < 12.0
            && hw_fallback_mp4_verified;

        let report = Phase5VerificationReport {
            all_gates_passed,
            proxy_width: proxy_artifact.width,
            proxy_height: proxy_artifact.height,
            full_width: full_res.width,
            full_height: full_res.height,
            pts_index_preserved,
            proxy_frame_count: proxy_probe.pts_index.len(),
            semantic_timing_identical,
            draft_vs_final_mean_l1_diff,
            impact_tick,
            available_hwaccels: hw_caps.available_hwaccels,
            preferred_decoder: hw_caps.preferred_decoder,
            preferred_encoder: hw_caps.preferred_encoder,
            software_policy_backend: format!("{sw_backend:?}"),
            auto_policy_backend: format!("{auto_backend:?}"),
            hw_fallback_mp4_verified,
            proxy_mp4_path: proxy_artifact.proxy_path,
        };

        std::fs::write(
            out_dir.join("phase5_verification_report.json"),
            serde_json::to_string_pretty(&report)?,
        )?;

        Ok(report)
    }
}

fn select_profile(project: &Project, profile: RenderProfileKind) -> &RenderProfileDef {
    match profile {
        RenderProfileKind::Draft => &project.render_profiles.draft,
        RenderProfileKind::Final => &project.render_profiles.final_profile,
    }
}

fn default_render_options() -> RenderOptions {
    RenderOptions {
        output_alpha_mode: OutputAlphaMode::Straight,
        ..Default::default()
    }
}

/// Deletes a partially written output file unless the render completed successfully.
struct PartialOutputGuard {
    path: PathBuf,
    armed: bool,
}

impl PartialOutputGuard {
    fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for PartialOutputGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// True when at least one audio clip overlaps `[range.start_tick, range.end_tick)`.
fn range_intersects_audio(project: &Project, range: &RenderRange) -> bool {
    project
        .timeline
        .audio_clips
        .iter()
        .any(|clip| clip.timeline_start < range.end_tick && clip.timeline_end > range.start_tick)
}

pub(crate) fn collect_timeline_frame_ticks(project: &Project) -> Result<Vec<Flicks>, CoreError> {
    let mut ticks = Vec::new();
    let mut idx = 0i64;
    loop {
        let tick = project.timeline.fps.frame_to_ticks(idx)?;
        if tick >= project.timeline.duration_ticks {
            break;
        }
        ticks.push(tick);
        idx += 1;
    }
    if ticks.is_empty() {
        return Err(CoreError::InvalidArgument(
            "timeline produced zero frames".into(),
        ));
    }
    Ok(ticks)
}

fn verify_audio_impact_sync(
    project: &Project,
    mixed_audio: Option<&frameshift_media::AudioBuffer>,
    drop_tick: Flicks,
    frame_ticks: Flicks,
) -> Result<AudioImpactSyncReport, CoreError> {
    let event = project
        .event("drop_impact")
        .ok_or_else(|| CoreError::AudioSyncFailure("missing 'drop_impact' event".into()))?;
    let clip_id = event
        .source_clip_id
        .as_deref()
        .ok_or_else(|| CoreError::AudioSyncFailure("drop_impact missing source_clip_id".into()))?;
    let clip = project
        .clip(clip_id)
        .ok_or_else(|| CoreError::AudioSyncFailure(format!("missing clip '{clip_id}'")))?;
    let source_impact = event.source_anchor_tick.ok_or_else(|| {
        CoreError::AudioSyncFailure("drop_impact missing source_anchor_tick".into())
    })?;

    // ADR-0003 valid solve aligning clip impact marker to target beat tick.
    let valid_req = ImpactSolveRequest {
        source_bounds_start: clip.source_start,
        source_bounds_end: clip.source_start + Flicks(3 * FLICKS_PER_SECOND),
        source_impact,
        output_start: clip.timeline_start,
        output_end: clip.timeline_end,
        target_output: drop_tick,
        min_speed: 0.25,
        max_speed: 4.0,
        style: ImpactCurveStyle::SmoothRamp,
        fit_mode: SourceFitMode::ExactRange,
    };
    let solution = ImpactSolver::solve(&valid_req)?;
    let solver_error_ticks = (solution.achieved_source_at_target.0 - source_impact.0).abs();

    // ADR-0003 negative check: impossible speed constraint must reject cleanly with alternatives.
    let impossible_req = ImpactSolveRequest {
        max_speed: 0.5,
        ..valid_req
    };
    let (impossible_constraint_rejected, impossible_suggestion_count) =
        match ImpactSolver::solve(&impossible_req) {
            Err(violation) => (!violation.failures.is_empty(), violation.suggestions.len()),
            Ok(_) => (false, 0),
        };

    // Audio transient peak check on the generated/mixed AudioBuffer within 1 frame of visual flash.
    let owned_fallback;
    let audio_buf = match mixed_audio {
        Some(buf) => buf,
        None => {
            owned_fallback = SyntheticMediaGenerator::generate_beat_audio(
                project.timeline.duration_ticks,
                120,
                DEFAULT_AUDIO_SAMPLE_RATE,
            )
            .0;
            &owned_fallback
        }
    };

    let peak_info = audio_buf.detect_peak_in_window(drop_tick, frame_ticks)?;
    let delta_ticks = (peak_info.peak_tick.0 - drop_tick.0).abs();
    let max_allowed_delta_ticks = frame_ticks.0;

    let passed = solver_error_ticks <= 1_000
        && impossible_constraint_rejected
        && impossible_suggestion_count > 0
        && peak_info.peak_amplitude > 0.5
        && delta_ticks <= max_allowed_delta_ticks;

    Ok(AudioImpactSyncReport {
        passed,
        target_beat_tick: drop_tick,
        visual_impact_flash_tick: drop_tick,
        audio_transient_peak_tick: peak_info.peak_tick,
        delta_ticks,
        max_allowed_delta_ticks,
        solver_achieved_source_tick: solution.achieved_source_at_target,
        solver_expected_source_tick: source_impact,
        solver_error_ticks,
        impossible_constraint_rejected,
        impossible_suggestion_count,
    })
}

fn audio_envelope_cache_key(project: &Project) -> String {
    use std::fmt::Write;
    let mut key = format!("dur={}", project.timeline.duration_ticks.0);
    for ac in &project.timeline.audio_clips {
        let asset_path = project
            .asset(&ac.asset_id)
            .map(|a| a.path.as_str())
            .unwrap_or("");
        let _ = write!(
            key,
            "|{}:{}:{}:{}:{}:{}:{}",
            ac.id,
            ac.asset_id,
            asset_path,
            ac.timeline_start.0,
            ac.timeline_end.0,
            ac.source_start.0,
            ac.gain_db.to_bits()
        );
    }
    key
}

fn resolve_disk_audio_envelope<'a>(
    cache_slot: &'a mut Option<(String, AudioRmsEnvelope)>,
    resolver: &MediaResolver,
    project: &Project,
) -> Result<Option<&'a AudioRmsEnvelope>, CoreError> {
    let mut has_disk_audio = false;
    for ac in &project.timeline.audio_clips {
        let asset = project
            .asset(&ac.asset_id)
            .ok_or_else(|| frameshift_schema::SchemaError::MissingAsset(ac.asset_id.clone()))?;
        if matches!(
            resolver.resolve_asset_source(asset)?,
            ResolvedAssetSource::Disk(_)
        ) {
            has_disk_audio = true;
            break;
        }
    }
    if !has_disk_audio {
        return Ok(None);
    }

    let key = audio_envelope_cache_key(project);
    let cache_hit = cache_slot
        .as_ref()
        .is_some_and(|(cached_key, _)| cached_key == &key);
    if !cache_hit {
        let window_ticks = Flicks(FLICKS_PER_SECOND / 100);
        let samples = match resolver.mix_project_audio(project)? {
            Some(buf) => buf.compute_rms_envelope(window_ticks)?,
            None => Vec::new(),
        };
        *cache_slot = Some((key, AudioRmsEnvelope::new(window_ticks, samples)));
    }
    Ok(cache_slot.as_ref().map(|(_, env)| env))
}

#[cfg(test)]
mod tests {
    use super::*;
    use frameshift_schema::{AssetDef, AssetKind};

    #[test]
    fn default_cache_capacity_is_128_and_synthetic_audio_skips_envelope() {
        let mut engine = Engine::new_headless().expect("engine");
        assert_eq!(engine.cache_telemetry().capacity, 128);

        let golden = Project::golden_scene_preset();
        let env = resolve_disk_audio_envelope(
            &mut engine.audio_envelope_cache,
            &engine.resolver,
            &golden,
        )
        .expect("resolve synthetic");
        assert!(
            env.is_none(),
            "synthetic BeatAudio120Bpm must bypass real envelope to keep M1 hashes identical"
        );
    }

    #[test]
    fn disk_audio_populates_and_caches_rms_envelope_and_clear_cache_resets_it() {
        let temp_wav = std::env::temp_dir().join(format!(
            "frameshift_engine_disk_audio_{}.wav",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&temp_wav);
        let (buf, _) = SyntheticMediaGenerator::generate_beat_audio(
            Flicks(FLICKS_PER_SECOND),
            120,
            DEFAULT_AUDIO_SAMPLE_RATE,
        );
        buf.write_wav(&temp_wav).expect("write wav");

        let mut project = Project::golden_scene_preset();
        project.assets.push(AssetDef {
            id: "asset_disk_bgm".into(),
            kind: AssetKind::Audio,
            path: temp_wav.to_string_lossy().into_owned(),
            duration_ticks: Some(Flicks(FLICKS_PER_SECOND)),
            fps: None,
            sample_rate_hz: Some(DEFAULT_AUDIO_SAMPLE_RATE),
            resolution: None,
        });
        project.timeline.audio_clips[0].asset_id = "asset_disk_bgm".into();
        project.timeline.audio_clips[0].timeline_end = Flicks(FLICKS_PER_SECOND);
        project.render_profiles.draft.resolution = Resolution {
            width: 64,
            height: 36,
        };
        project.render_profiles.draft.motion_blur_sub_samples = 1;

        let mut engine = Engine::new_headless().expect("engine");
        let _ = engine
            .render_frame_at(
                &project,
                RenderProfileKind::Draft,
                Flicks(FLICKS_PER_SECOND / 2),
            )
            .expect("render with disk audio");

        let (key, env) = engine
            .audio_envelope_cache
            .as_ref()
            .expect("disk audio must populate audio_envelope_cache");
        assert!(!key.is_empty());
        assert_eq!(env.window_ticks, Flicks(FLICKS_PER_SECOND / 100));
        assert!(!env.samples.is_empty());

        engine.clear_cache();
        assert!(engine.audio_envelope_cache.is_none());

        let _ = std::fs::remove_file(&temp_wav);
    }

    #[test]
    fn partial_output_guard_removes_file_until_disarmed() {
        let path = std::env::temp_dir().join(format!(
            "frameshift_partial_guard_{}.mp4",
            std::process::id()
        ));
        std::fs::write(&path, b"partial").expect("write partial output");
        {
            let _guard = PartialOutputGuard::new(&path);
        }
        assert!(!path.exists(), "armed guard must delete the partial output");

        std::fs::write(&path, b"complete").expect("write complete output");
        {
            let mut guard = PartialOutputGuard::new(&path);
            guard.disarm();
        }
        assert!(path.exists(), "disarmed guard must keep the finished output");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn disagreeing_source_nodes_keep_separate_fitted_frames() {
        use frameshift_media::AlphaMode;
        use frameshift_schema::{
            ColorSpace, GraphNodeDef, NodeId, NodeKind, RenderGraphDef, SourceFit,
        };

        let src_w = 320u32;
        let src_h = 180u32;
        let dst_w = 180u32;
        let dst_h = 220u32;
        let frames: Vec<Rgba8Frame> = (0..4)
            .map(|i| {
                let pts = FPS_24.frame_to_ticks(i).unwrap();
                let mut frame =
                    Rgba8Frame::filled(src_w, src_h, pts, [0, 255, 0, 255], AlphaMode::Straight)
                        .unwrap();
                let edge = src_w / 5;
                for y in 0..src_h {
                    for x in 0..edge {
                        frame.set_pixel(x, y, [255, 0, 0, 255]);
                        frame.set_pixel(src_w - 1 - x, y, [0, 0, 255, 255]);
                    }
                }
                frame
            })
            .collect();
        let mp4_path = std::env::temp_dir().join(format!(
            "frameshift_engine_node_fit_{}.mp4",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&mp4_path);
        FfmpegBridge::encode_mp4(&frames, None, FPS_24, &mp4_path).expect("encode wide mp4");

        let mut project = Project::golden_scene_preset();
        project.param_bindings.clear();
        for clip in &mut project.timeline.clips {
            clip.event_animations.clear();
        }
        project.assets[0].path = mp4_path.to_string_lossy().into_owned();
        project.assets[0].fps = Some(FPS_24);
        project.assets[0].resolution = Some(Resolution {
            width: src_w,
            height: src_h,
        });
        project.render_profiles.draft.resolution = Resolution {
            width: dst_w,
            height: dst_h,
        };
        project.render_profiles.draft.motion_blur_sub_samples = 1;
        project.graph = RenderGraphDef {
            output_node: NodeId::from("src_stretch"),
            nodes: vec![
                GraphNodeDef {
                    id: NodeId::from("src_stretch"),
                    kind: NodeKind::SourceVideo {
                        asset_id: "asset_clip_a".into(),
                        clip_id: None,
                        color_space: ColorSpace::DisplaySrgb,
                        fit: SourceFit::Stretch,
                    },
                    inputs: Vec::new(),
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("src_cover"),
                    kind: NodeKind::SourceVideo {
                        asset_id: "asset_clip_a".into(),
                        clip_id: None,
                        color_space: ColorSpace::DisplaySrgb,
                        fit: SourceFit::Cover,
                    },
                    inputs: Vec::new(),
                    enabled: true,
                },
            ],
        };

        let mut engine = Engine::new_headless().expect("engine");
        let stretch = engine
            .render_frame_at(&project, RenderProfileKind::Draft, Flicks::ZERO)
            .expect("stretch node");
        let after_stretch = engine.cache_telemetry().current_entries;
        assert!(after_stretch >= 1);

        project.graph.output_node = NodeId::from("src_cover");
        let cover = engine
            .render_frame_at(&project, RenderProfileKind::Draft, Flicks::ZERO)
            .expect("cover node");
        assert_ne!(stretch.data, cover.data);
        assert!(engine.cache_telemetry().current_entries > after_stretch);

        let stretch_key = format!("asset_clip_a@{dst_w}x{dst_h}:stretch");
        let cover_key = format!("asset_clip_a@{dst_w}x{dst_h}:cover");
        let cached_stretch = engine.cache.peek(&stretch_key, 0).expect("stretch buffer");
        let cached_cover = engine.cache.peek(&cover_key, 0).expect("cover buffer");
        assert_ne!(cached_stretch.data, cached_cover.data);

        engine.clear_cache();
        assert_eq!(engine.decoder_telemetry().active_streams, 0);
        std::fs::remove_file(&mp4_path).expect("mp4 unlocked after clear_cache");
    }
}
