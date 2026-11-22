use frameshift_core::{
    AudioRetimeMode, CoreError, Engine, Flicks, MediaResolver, OutputAlphaMode, Project,
    ProjectCommand, ProjectSession, RenderOptions, RenderProfileKind, RenderProgress, RenderRange,
    RenderStage, slice_audio_buffer,
};
use frameshift_media::{
    AlphaMode, DEFAULT_AUDIO_SAMPLE_RATE, FfmpegBridge, Rgba8Frame, SyntheticMediaGenerator,
};
use frameshift_render::RenderError;
use frameshift_schema::{
    AssetDef, AssetKind, BlendMode, ColorSpace, CubicBezierSegmentDef, GraphNodeDef, LutSourceDef,
    NodeId, NodeKind, ParamBindingDef, RenderGraphDef, Resolution, SampleFilter, SourceFit,
    TextAlign2D,
};
use frameshift_time::{
    FLICKS_PER_SECOND, FPS_24, FPS_60, SpeedCurve, ticks_to_audio_samples_nearest,
};
use frameshift_vector::VectorError;
use std::path::PathBuf;

struct TempDirGuard(PathBuf);

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn unique_temp_dir(label: &str) -> TempDirGuard {
    let dir = std::env::temp_dir().join(format!(
        "frameshift_core_test_{label}_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp test dir");
    TempDirGuard(dir)
}

#[test]
fn engine_renders_single_frame_with_cache_telemetry_and_seek_determinism() {
    let mut engine = Engine::new_headless().expect("headless engine");
    let mut project = Project::golden_scene_preset();
    project.render_profiles.draft.resolution = Resolution {
        width: 160,
        height: 90,
    };

    let drop_tick = Flicks(2 * FLICKS_PER_SECOND + FLICKS_PER_SECOND / 2);
    let f1 = engine
        .render_frame_at(&project, RenderProfileKind::Draft, drop_tick)
        .expect("first draft frame");
    let first_tel = engine.cache_telemetry();
    assert!(first_tel.misses >= 1);

    let f2 = engine
        .render_frame_at(&project, RenderProfileKind::Draft, drop_tick)
        .expect("cached draft frame");
    let second_tel = engine.cache_telemetry();
    assert!(second_tel.hits > first_tel.hits);
    assert_eq!(f1.fnv1a_hash64(), f2.fnv1a_hash64());
    assert_eq!(f1.data, f2.data);

    // Clearing cache and re-rendering must produce bit-identical pixels.
    engine.clear_cache();
    let f3 = engine
        .render_frame_at(&project, RenderProfileKind::Draft, drop_tick)
        .expect("cold re-render");
    assert_eq!(f1.fnv1a_hash64(), f3.fnv1a_hash64());
}

#[test]
fn split_clip_preserves_rendered_frames_across_the_new_boundary() {
    let mut original = Project::golden_scene_preset();
    original.render_profiles.draft.resolution = Resolution {
        width: 64,
        height: 36,
    };
    let mut session = ProjectSession::new(original.clone()).expect("valid golden project");
    session
        .apply_command(
            1,
            ProjectCommand::SplitClip {
                clip_id: "clip_a".into(),
                split_tick: Flicks(FLICKS_PER_SECOND),
                right_clip_id: "clip_a_tail".into(),
            },
        )
        .expect("source-routed split succeeds");
    let split = session.project();
    let sample_tick = Flicks(FLICKS_PER_SECOND + FLICKS_PER_SECOND / 2);

    let mut original_engine = Engine::new_headless().expect("headless engine");
    let before = original_engine
        .render_frame_at(&original, RenderProfileKind::Draft, sample_tick)
        .expect("render unsplit timeline");
    let mut split_engine = Engine::new_headless().expect("second headless engine");
    let after = split_engine
        .render_frame_at(split, RenderProfileKind::Draft, sample_tick)
        .expect("render split timeline");
    let mut differing_channels = 0usize;
    let mut differing_alpha = 0usize;
    let mut max_color_delta = 0u8;
    let mut first_difference = None;
    for (index, (&actual, &expected)) in after.data.iter().zip(&before.data).enumerate() {
        let delta = actual.abs_diff(expected);
        differing_channels += usize::from(delta > 0);
        if index % 4 == 3 {
            differing_alpha += usize::from(delta > 0);
        } else {
            max_color_delta = max_color_delta.max(delta);
        }
        if delta > 0 && first_difference.is_none() {
            first_difference = Some((index, actual, expected));
        }
    }
    assert_eq!(differing_alpha, 0, "split render changed alpha coverage");
    assert!(
        max_color_delta <= 1,
        "split render changed {differing_channels} channels (max color delta {max_color_delta}); first difference {first_difference:?}; hashes {} vs {}",
        after.fnv1a_hash64(),
        before.fnv1a_hash64()
    );
}

#[test]
fn engine_resolves_moving_mask_synthetic_asset_and_on_disk_mp4() {
    let temp = unique_temp_dir("disk_and_mask");
    let disk_clip_path = temp.0.join("disk_clip.mp4");

    // Create a short real on-disk MP4 via SyntheticMediaGenerator + FfmpegBridge.
    let mut synthetic_frames = Vec::new();
    for idx in 0..12 {
        let tick = FPS_24.frame_to_ticks(idx).unwrap();
        synthetic_frames.push(SyntheticMediaGenerator::generate_clip_a_frame(
            128, 72, tick,
        ));
    }
    let (audio, _) = SyntheticMediaGenerator::generate_beat_audio(
        Flicks(FLICKS_PER_SECOND / 2),
        120,
        DEFAULT_AUDIO_SAMPLE_RATE,
    );
    FfmpegBridge::encode_mp4(&synthetic_frames, Some(&audio), FPS_24, &disk_clip_path)
        .expect("encode on-disk test mp4");

    // Build a project using the on-disk MP4 + built-in synthetic moving_mask asset.
    let mut project = Project::golden_scene_preset();
    project.timeline.duration_ticks = Flicks(FLICKS_PER_SECOND / 2);
    project.timeline.fps = FPS_60;
    project.render_profiles.draft.resolution = Resolution {
        width: 128,
        height: 72,
    };
    project.render_profiles.draft.motion_blur_sub_samples = 2;

    let mut session = ProjectSession::new(project).expect("valid session");
    let rev = session.revision_id();
    let rev = session
        .apply_command(
            rev,
            ProjectCommand::AddAsset(AssetDef {
                id: "asset_disk".into(),
                kind: AssetKind::Video,
                path: disk_clip_path.to_string_lossy().into_owned(),
                duration_ticks: Some(Flicks(FLICKS_PER_SECOND / 2)),
                fps: Some(FPS_24),
                sample_rate_hz: None,
                resolution: Some(Resolution {
                    width: 128,
                    height: 72,
                }),
            }),
        )
        .expect("add disk asset");
    let rev = session
        .apply_command(
            rev,
            ProjectCommand::AddAsset(AssetDef {
                id: "asset_moving_mask".into(),
                kind: AssetKind::Video,
                path: "synthetic://moving_mask".into(),
                duration_ticks: Some(Flicks(FLICKS_PER_SECOND / 2)),
                fps: Some(FPS_24),
                sample_rate_hz: None,
                resolution: Some(Resolution {
                    width: 128,
                    height: 72,
                }),
            }),
        )
        .expect("add synthetic moving mask asset");
    let rev = session
        .apply_command(
            rev,
            ProjectCommand::UpsertGraphNode(GraphNodeDef {
                id: NodeId::from("src_a"),
                kind: NodeKind::SourceVideo {
                    asset_id: "asset_disk".into(),
                    clip_id: Some("clip_a".into()),
                    color_space: frameshift_schema::ColorSpace::DisplaySrgb,
                    fit: SourceFit::Stretch,
                },
                inputs: vec![],
                enabled: true,
            }),
        )
        .expect("point src_a to disk asset");
    let _rev = session
        .apply_command(
            rev,
            ProjectCommand::UpsertGraphNode(GraphNodeDef {
                id: NodeId::from("moving_mask_xform"),
                kind: NodeKind::SourceVideo {
                    asset_id: "asset_moving_mask".into(),
                    clip_id: None,
                    color_space: frameshift_schema::ColorSpace::LinearRec709,
                    fit: SourceFit::Stretch,
                },
                inputs: vec![],
                enabled: true,
            }),
        )
        .expect("point mask input to synthetic moving_mask");

    let out_mp4 = temp.0.join("hybrid_out.mp4");
    let mut engine = Engine::new_headless().expect("engine");
    let summary = engine
        .render_video_mp4(session.project(), RenderProfileKind::Draft, &out_mp4)
        .expect("render hybrid on-disk + synthetic project");

    assert_eq!(summary.frame_count, 30);
    assert!(summary.has_audio);
    assert!(out_mp4.is_file());

    let probed = FfmpegBridge::probe_media_info(&out_mp4).expect("probe hybrid mp4");
    assert_eq!(probed.width, 128);
    assert_eq!(probed.height, 72);
    assert!(probed.has_audio);
}

#[test]
fn missing_non_synthetic_asset_returns_explicit_error() {
    let mut engine = Engine::new_headless().expect("engine");
    let mut project = Project::golden_scene_preset();
    project.assets[0].path = "does_not_exist/real_clip.mp4".into();
    project.assets[0].id = "custom_missing".into();
    project.timeline.clips[0].asset_id = "custom_missing".into();
    if let NodeKind::SourceVideo { asset_id, .. } = &mut project.graph.nodes[0].kind {
        *asset_id = "custom_missing".into();
    }

    let err = engine
        .render_frame_at(&project, RenderProfileKind::Draft, Flicks::ZERO)
        .expect_err("missing disk asset must error");
    assert!(matches!(err, CoreError::Render(_)));
}

#[test]
fn audio_retime_mode_pitch_preserving_wsola_succeeds() {
    let (buf, _) = SyntheticMediaGenerator::generate_beat_audio(
        Flicks(FLICKS_PER_SECOND),
        120,
        DEFAULT_AUDIO_SAMPLE_RATE,
    );
    let curve = SpeedCurve::constant(1.5).unwrap();
    let retimed = MediaResolver::retime_audio_buffer(
        &buf,
        &curve,
        Flicks(FLICKS_PER_SECOND),
        Flicks::ZERO,
        AudioRetimeMode::PitchPreserving,
    )
    .expect("PitchPreserving must succeed via WSOLA");
    assert_eq!(retimed.sample_rate, DEFAULT_AUDIO_SAMPLE_RATE);
    assert_eq!(retimed.channels, buf.channels);
    assert!(retimed.frame_count() > 0);
    let energy: f32 = retimed.samples.iter().map(|s| s * s).sum();
    assert!(energy > 0.0);
}

#[test]
fn inspection_pack_and_phase5_proxy_hw_succeed() {
    let temp = unique_temp_dir("inspection_and_phase5");
    let mut engine = Engine::new_headless().expect("headless engine");
    let mut project = Project::golden_scene_preset();
    // Use faster resolution for unit test speed while keeping crop_fullres >= 640x360.
    project.render_profiles.draft.resolution = Resolution {
        width: 640,
        height: 360,
    };
    project.render_profiles.final_profile.resolution = Resolution {
        width: 640,
        height: 360,
    };
    project
        .render_profiles
        .final_profile
        .motion_blur_sub_samples = 4;

    let pack_dir = temp.0.join("inspection");
    let manifest = engine
        .export_inspection_pack(&project, &pack_dir)
        .expect("export inspection pack");
    assert_eq!(manifest.revision_id, project.revision_id);
    assert_eq!(manifest.sampled_ticks.len(), 6);
    assert!(manifest.contact_sheet_path.is_file());
    assert!(manifest.mask_pack_path.is_file());
    assert!(manifest.transition_pack_path.is_file());
    assert!(manifest.velocity_pack_path.is_file());
    assert!(manifest.crop_fullres_path.is_file());
    assert!(manifest.fx_showcase_pack_path.is_file());
    assert!(manifest.manifest_path.is_file());
    let showcase =
        frameshift_media::Rgba8Frame::load_png(&manifest.fx_showcase_pack_path, Flicks::ZERO)
            .expect("load fx_showcase_pack.png");
    assert_eq!(showcase.width, 1920);
    assert_eq!(showcase.height, 1080);
    assert_eq!(
        manifest.epistemic_scope.not_proven_by_contact_sheet,
        vec![
            "inter_frame_flicker".to_string(),
            "motion_smoothness".to_string(),
            "audio_rhythm_feel".to_string(),
        ]
    );
    let crop = frameshift_media::Rgba8Frame::load_png(&manifest.crop_fullres_path, Flicks::ZERO)
        .expect("load crop_fullres.png");
    assert_eq!(crop.width, 640);
    assert_eq!(crop.height, 360);

    let p5_dir = temp.0.join("phase5");
    let p5 = engine
        .verify_phase5_proxy_and_hw(&p5_dir)
        .expect("phase 5 verification");
    assert!(p5.all_gates_passed, "phase 5 failed: {p5:?}");
    assert!(p5.pts_index_preserved);
    assert!(p5.semantic_timing_identical);
    assert!(p5.hw_fallback_mp4_verified);
}

fn minimal_render_project(width: u32, height: u32, graph: RenderGraphDef) -> Project {
    let mut project = Project::golden_scene_preset();
    project.param_bindings.clear();
    for clip in &mut project.timeline.clips {
        clip.event_animations.clear();
    }
    project.render_profiles.draft.resolution = Resolution { width, height };
    project.render_profiles.draft.motion_blur_sub_samples = 1;
    project.graph = graph;
    project
}

fn find_system_ttf_for_core_test() -> Option<String> {
    let candidates = [
        r"C:\Windows\Fonts\arial.ttf",
        r"C:\Windows\Fonts\impact.ttf",
        r"C:\Windows\Fonts\segoeui.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/System/Library/Fonts/Helvetica.ttc",
    ];
    for path in candidates {
        if std::path::Path::new(path).is_file() {
            return Some(path.to_string());
        }
    }
    None
}

#[test]
fn engine_source_image_png_alpha_and_jpeg_fallback() {
    let temp = unique_temp_dir("source_image");
    let png_path = temp.0.join("overlay.png");
    let jpg_src_path = temp.0.join("photo_src.png");
    let jpg_path = temp.0.join("photo.jpg");

    let mut overlay = Rgba8Frame::filled(
        64,
        32,
        Flicks::ZERO,
        [0, 240, 255, 255],
        AlphaMode::Straight,
    )
    .expect("overlay frame");
    for y in 0..32 {
        for x in 0..32 {
            overlay.set_pixel(x, y, [0, 0, 0, 0]);
        }
    }
    overlay.save_png(&png_path).expect("save png overlay");

    let photo_src = Rgba8Frame::filled(
        64,
        32,
        Flicks::ZERO,
        [40, 220, 80, 255],
        AlphaMode::Straight,
    )
    .expect("photo src");
    photo_src.save_png(&jpg_src_path).expect("save jpg source");
    let status = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-i"])
        .arg(&jpg_src_path)
        .arg(&jpg_path)
        .status()
        .expect("spawn ffmpeg for jpeg fixture");
    assert!(status.success(), "ffmpeg png->jpg failed");

    let mut project = minimal_render_project(
        128,
        128,
        RenderGraphDef {
            output_node: NodeId::from("comp"),
            nodes: vec![
                GraphNodeDef {
                    id: NodeId::from("bg"),
                    kind: NodeKind::Solid {
                        color_rgba: [0.85, 0.05, 0.05, 1.0],
                        color_space: ColorSpace::LinearRec709,
                    },
                    inputs: Vec::new(),
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("img_png_node"),
                    kind: NodeKind::SourceImage {
                        asset_id: "asset_png".into(),
                        color_space: ColorSpace::DisplaySrgb,
                        fit: SourceFit::Contain,
                        opacity: 1.0,
                    },
                    inputs: Vec::new(),
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("comp"),
                    kind: NodeKind::Blend {
                        mode: BlendMode::Normal,
                        opacity: 1.0,
                    },
                    inputs: vec![NodeId::from("bg"), NodeId::from("img_png_node")],
                    enabled: true,
                },
            ],
        },
    );
    project.assets.push(AssetDef {
        id: "asset_png".into(),
        kind: AssetKind::Image,
        path: png_path.to_string_lossy().into_owned(),
        duration_ticks: None,
        fps: None,
        sample_rate_hz: None,
        resolution: Some(Resolution {
            width: 64,
            height: 32,
        }),
    });
    project.assets.push(AssetDef {
        id: "asset_jpg".into(),
        kind: AssetKind::Image,
        path: jpg_path.to_string_lossy().into_owned(),
        duration_ticks: None,
        fps: None,
        sample_rate_hz: None,
        resolution: Some(Resolution {
            width: 64,
            height: 32,
        }),
    });

    let mut engine = Engine::new_headless().expect("engine");
    let frame_a = engine
        .render_frame_at(&project, RenderProfileKind::Draft, Flicks::ZERO)
        .expect("render PNG SourceImage over solid");
    assert_eq!(engine.decoder_telemetry().spawns, 0);

    // Top letterbox row (y=4) and left transparent half (x=16, y=64) must reveal the red background.
    let top_px = frame_a.get_pixel(64, 4).unwrap();
    assert!(
        top_px[0] > 180 && top_px[1] < 90 && top_px[2] < 90,
        "Contain letterbox should preserve red bg: {top_px:?}"
    );
    let left_px = frame_a.get_pixel(16, 64).unwrap();
    assert!(
        left_px[0] > 180 && left_px[1] < 90 && left_px[2] < 90,
        "PNG alpha half should reveal red bg: {left_px:?}"
    );
    // Right opaque cyan half (x=96, y=64) must show cyan.
    let right_px = frame_a.get_pixel(96, 64).unwrap();
    assert!(
        right_px[0] < 80 && right_px[1] > 180 && right_px[2] > 180,
        "PNG opaque half should composite cyan: {right_px:?}"
    );

    // Second render hits caches and is bit-identical.
    let frame_b = engine
        .render_frame_at(&project, RenderProfileKind::Draft, Flicks::ZERO)
        .expect("re-render cached PNG SourceImage");
    assert_eq!(frame_a.fnv1a_hash64(), frame_b.fnv1a_hash64());

    // JPEG fallback via ffmpeg single-frame decode (Cover).
    project.graph = RenderGraphDef {
        output_node: NodeId::from("img_jpg_node"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("img_jpg_node"),
            kind: NodeKind::SourceImage {
                asset_id: "asset_jpg".into(),
                color_space: ColorSpace::DisplaySrgb,
                fit: SourceFit::Cover,
                opacity: 1.0,
            },
            inputs: Vec::new(),
            enabled: true,
        }],
    };
    let jpg_frame = engine
        .render_frame_at(&project, RenderProfileKind::Draft, Flicks::ZERO)
        .expect("render JPEG SourceImage");
    let center_px = jpg_frame.get_pixel(64, 64).unwrap();
    assert!(
        center_px[1] > 160 && center_px[0] < 100,
        "JPEG Cover should decode green frame: {center_px:?}"
    );
    assert_eq!(engine.decoder_telemetry().spawns, 0);
}

#[test]
fn engine_precomp_nested_inputs_time_remap_and_streaming_video_decoder() {
    let temp = unique_temp_dir("precomp_streaming");
    let mp4_path = temp.0.join("stream_clip.mp4");

    let frames: Vec<Rgba8Frame> = (0..12)
        .map(|i| {
            SyntheticMediaGenerator::generate_clip_a_frame(
                96,
                54,
                FPS_24.frame_to_ticks(i).unwrap(),
            )
        })
        .collect();
    FfmpegBridge::encode_mp4(&frames, None, FPS_24, &mp4_path).expect("encode test mp4");

    let sub_graph = RenderGraphDef {
        output_node: NodeId::from("sub_blend"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("sub_in"),
                kind: NodeKind::PrecompInput { slot: 0 },
                inputs: Vec::new(),
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("sub_grade"),
                kind: NodeKind::ColorBasic {
                    exposure: 0.25,
                    contrast: 1.1,
                    saturation: 1.0,
                    flash_amount: 0.0,
                },
                inputs: vec![NodeId::from("sub_in")],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("sub_label"),
                kind: NodeKind::VectorText {
                    text: "PRECOMP".into(),
                    font_size_norm: 0.14,
                    color_rgba: [1.0, 1.0, 1.0, 1.0],
                    outline_rgba: [0.0, 0.0, 0.0, 1.0],
                    outline_width_norm: 0.008,
                    align_center: true,
                    y_offset_norm: 0.5,
                    font_family: None,
                    font_path: None,
                    font_weight: 400,
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
            GraphNodeDef {
                id: NodeId::from("sub_blend"),
                kind: NodeKind::Blend {
                    mode: BlendMode::Normal,
                    opacity: 1.0,
                },
                inputs: vec![NodeId::from("sub_grade"), NodeId::from("sub_label")],
                enabled: true,
            },
        ],
    };

    let mut project = minimal_render_project(
        96,
        54,
        RenderGraphDef {
            output_node: NodeId::from("precomp"),
            nodes: vec![
                GraphNodeDef {
                    id: NodeId::from("src_vid"),
                    kind: NodeKind::SourceVideo {
                        asset_id: "asset_stream".into(),
                        clip_id: None,
                        color_space: ColorSpace::DisplaySrgb,
                        fit: SourceFit::Stretch,
                    },
                    inputs: Vec::new(),
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("precomp"),
                    kind: NodeKind::Precomp {
                        sub_graph,
                        time_offset_ticks: 0,
                        time_scale: 1.0,
                        opacity: 1.0,
                    },
                    inputs: vec![NodeId::from("src_vid")],
                    enabled: true,
                },
            ],
        },
    );
    project.assets.push(AssetDef {
        id: "asset_stream".into(),
        kind: AssetKind::Video,
        path: mp4_path.to_string_lossy().into_owned(),
        duration_ticks: Some(Flicks(FLICKS_PER_SECOND / 2)),
        fps: Some(FPS_24),
        sample_rate_hz: None,
        resolution: Some(Resolution {
            width: 96,
            height: 54,
        }),
    });

    let mut engine = Engine::new_headless().expect("engine");
    let t0 = FPS_24.frame_to_ticks(0).unwrap();
    let t1 = FPS_24.frame_to_ticks(1).unwrap();
    let t2 = FPS_24.frame_to_ticks(2).unwrap();

    let seq0 = engine
        .render_frame_at(&project, RenderProfileKind::Draft, t0)
        .expect("render frame 0");
    let _seq1 = engine
        .render_frame_at(&project, RenderProfileKind::Draft, t1)
        .expect("render frame 1");
    let seq2 = engine
        .render_frame_at(&project, RenderProfileKind::Draft, t2)
        .expect("render frame 2");

    let tel = engine.decoder_telemetry();
    assert_eq!(
        tel.spawns, 1,
        "sequential streaming should reuse 1 ffmpeg process"
    );
    assert_eq!(tel.seek_reopens, 0);
    assert_ne!(seq0.fnv1a_hash64(), seq2.fnv1a_hash64());

    // Seek-independence with cold cache.
    engine.clear_cache();
    assert_eq!(engine.decoder_telemetry().active_streams, 0);
    let cold2 = engine
        .render_frame_at(&project, RenderProfileKind::Draft, t2)
        .expect("cold seek render frame 2");
    assert_eq!(seq2.fnv1a_hash64(), cold2.fnv1a_hash64());

    // Precomp with opacity == 0.0 short-circuits cleanly to transparent black.
    if let NodeKind::Precomp { opacity, .. } = &mut project.graph.nodes[1].kind {
        *opacity = 0.0;
    }
    let zero_op = engine
        .render_frame_at(&project, RenderProfileKind::Draft, t2)
        .expect("zero opacity precomp");
    assert!(zero_op.data.iter().all(|&b| b == 0));

    engine.clear_cache();
}

#[test]
fn engine_vector_text_real_ttf_and_font_not_found_error() {
    let mut project = minimal_render_project(
        256,
        128,
        RenderGraphDef {
            output_node: NodeId::from("vtext"),
            nodes: vec![GraphNodeDef {
                id: NodeId::from("vtext"),
                kind: NodeKind::VectorText {
                    text: "MISSING".into(),
                    font_size_norm: 0.16,
                    color_rgba: [1.0, 1.0, 1.0, 1.0],
                    outline_rgba: [0.0; 4],
                    outline_width_norm: 0.0,
                    align_center: true,
                    y_offset_norm: 0.5,
                    font_family: None,
                    font_path: Some(r"C:\nonexistent_frameshift_fonts\missing_999.ttf".into()),
                    font_weight: 400,
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
            }],
        },
    );

    let mut engine = Engine::new_headless().expect("engine");
    let static_err = engine
        .render_frame_at(&project, RenderProfileKind::Draft, Flicks::ZERO)
        .expect_err("missing font_path must not silently fall back");
    assert!(
        matches!(
            static_err,
            CoreError::Render(RenderError::Vector(VectorError::FontNotFound(_)))
        ),
        "expected RenderError::Vector(FontNotFound), got {static_err:?}"
    );

    // Dynamic parameter binding forces compositor internal TTF path; FontNotFound must still propagate.
    project.graph.nodes[0].kind = NodeKind::VectorText {
        text: "MISSING_FAMILY".into(),
        font_size_norm: 0.16,
        color_rgba: [1.0, 1.0, 1.0, 1.0],
        outline_rgba: [0.0; 4],
        outline_width_norm: 0.0,
        align_center: true,
        y_offset_norm: 0.5,
        font_family: Some("FrameShiftNonExistentFamily_98765".into()),
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
    };
    project.param_bindings.push(ParamBindingDef {
        node_id: NodeId::from("vtext"),
        param_name: "y_offset_norm".into(),
        expression: "0.5 + 0.05 * t_sec".into(),
    });
    let dyn_err = engine
        .render_frame_at(&project, RenderProfileKind::Draft, Flicks::ZERO)
        .expect_err("missing font_family with dynamic binding must return FontNotFound");
    assert!(
        matches!(
            dyn_err,
            CoreError::Render(RenderError::Vector(VectorError::FontNotFound(_)))
        ),
        "expected dynamic RenderError::Vector(FontNotFound), got {dyn_err:?}"
    );

    if let Some(ttf_path) = find_system_ttf_for_core_test() {
        project.param_bindings.clear();
        project.graph.nodes[0].kind = NodeKind::VectorText {
            text: "FRAME\nSHIFT".into(),
            font_size_norm: 0.16,
            color_rgba: [1.0, 0.95, 0.85, 1.0],
            outline_rgba: [0.1, 0.1, 0.2, 1.0],
            outline_width_norm: 0.01,
            align_center: false,
            y_offset_norm: 0.5,
            font_family: None,
            font_path: Some(ttf_path),
            font_weight: 400,
            align: Some(TextAlign2D::Right),
            x_offset_norm: Some(0.92),
            tracking_em: Some(0.04),
            line_height_em: Some(1.25),
            opacity: 0.9,
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
        };
        let ttf_frame = engine
            .render_frame_at(&project, RenderProfileKind::Draft, Flicks::ZERO)
            .expect("render real TTF VectorText");
        let non_empty = ttf_frame
            .data
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] > 10)
            .count();
        assert!(
            non_empty > 40,
            "expected rendered TTF glyphs, got {non_empty} pixels"
        );

        // Re-render with dynamic opacity binding as well.
        project.param_bindings.push(ParamBindingDef {
            node_id: NodeId::from("vtext"),
            param_name: "opacity".into(),
            expression: "0.75".into(),
        });
        let dyn_ttf = engine
            .render_frame_at(&project, RenderProfileKind::Draft, Flicks::ZERO)
            .expect("render dynamic real TTF VectorText");
        assert_ne!(ttf_frame.fnv1a_hash64(), dyn_ttf.fnv1a_hash64());
    }
}

#[test]
fn engine_vector_path_hud_corner_pin_and_grid_warp() {
    let mut engine = Engine::new_headless().expect("engine");

    // 1. VectorPathHud standalone render
    let hud_project = minimal_render_project(
        192,
        108,
        RenderGraphDef {
            output_node: NodeId::from("hud"),
            nodes: vec![GraphNodeDef {
                id: NodeId::from("hud"),
                kind: NodeKind::VectorPathHud {
                    points_norm: Vec::new(),
                    bezier_segments: vec![CubicBezierSegmentDef {
                        p0: [0.15, 0.75],
                        c1: [0.35, 0.15],
                        c2: [0.65, 0.85],
                        p1: [0.85, 0.25],
                    }],
                    closed: false,
                    stroke_width_norm: 0.015,
                    stroke_rgba: [0.1, 0.95, 1.0, 1.0],
                    fill_rgba: [0.0; 4],
                    dash_len_norm: 0.04,
                    gap_len_norm: 0.02,
                    dash_offset_norm: 0.0,
                    trim_start: 0.0,
                    trim_end: 0.9,
                    node_dot_radius_norm: 0.02,
                    data_value: 88.4,
                    data_label: "SYNC ".into(),
                    opacity: 1.0,
                },
                inputs: Vec::new(),
                enabled: true,
            }],
        },
    );
    let hud_frame = engine
        .render_frame_at(&hud_project, RenderProfileKind::Draft, Flicks::ZERO)
        .expect("render VectorPathHud");
    let hud_ink = hud_frame
        .data
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[3] > 16)
        .count();
    assert!(hud_ink > 50, "expected HUD ink pixels, got {hud_ink}");

    // 2. CornerPin warping a Solid fill inward leaves corner pixels transparent and center opaque.
    let cp_project = minimal_render_project(
        128,
        128,
        RenderGraphDef {
            output_node: NodeId::from("cp"),
            nodes: vec![
                GraphNodeDef {
                    id: NodeId::from("src"),
                    kind: NodeKind::Solid {
                        color_rgba: [0.9, 0.3, 0.1, 1.0],
                        color_space: ColorSpace::LinearRec709,
                    },
                    inputs: Vec::new(),
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("cp"),
                    kind: NodeKind::CornerPin {
                        tl: [0.2, 0.2],
                        tr: [0.8, 0.25],
                        br: [0.85, 0.8],
                        bl: [0.15, 0.85],
                        filter: SampleFilter::CatmullRom,
                        opacity: 1.0,
                    },
                    inputs: vec![NodeId::from("src")],
                    enabled: true,
                },
            ],
        },
    );
    let cp_frame = engine
        .render_frame_at(&cp_project, RenderProfileKind::Draft, Flicks::ZERO)
        .expect("render CornerPin");
    assert_eq!(
        cp_frame.get_pixel(2, 2).unwrap()[3],
        0,
        "outside CornerPin quad must be transparent"
    );
    assert!(
        cp_frame.get_pixel(64, 64).unwrap()[3] > 240,
        "inside CornerPin quad must be opaque"
    );

    // 3. GridWarp over VectorPathHud produces distinct warped pixels when mix_amount == 1.0 vs 0.0.
    let mut gw_project = minimal_render_project(
        128,
        128,
        RenderGraphDef {
            output_node: NodeId::from("gw"),
            nodes: vec![
                hud_project.graph.nodes[0].clone(),
                GraphNodeDef {
                    id: NodeId::from("gw"),
                    kind: NodeKind::GridWarp {
                        cols: 3,
                        rows: 3,
                        control_offsets: vec![
                            [0.0, 0.0],
                            [0.0, 0.0],
                            [0.0, 0.0],
                            [0.0, 0.0],
                            [0.12, -0.08],
                            [0.0, 0.0],
                            [0.0, 0.0],
                            [0.0, 0.0],
                            [0.0, 0.0],
                        ],
                        wave_amp: [0.03, 0.02],
                        wave_freq: [2.0, 2.0],
                        wave_phase: 0.4,
                        mix_amount: 0.0,
                        filter: SampleFilter::Bilinear,
                        opacity: 1.0,
                    },
                    inputs: vec![NodeId::from("hud")],
                    enabled: true,
                },
            ],
        },
    );
    let unwarped = engine
        .render_frame_at(&gw_project, RenderProfileKind::Draft, Flicks::ZERO)
        .expect("render GridWarp mix=0");
    if let NodeKind::GridWarp { mix_amount, .. } = &mut gw_project.graph.nodes[1].kind {
        *mix_amount = 1.0;
    }
    let warped = engine
        .render_frame_at(&gw_project, RenderProfileKind::Draft, Flicks::ZERO)
        .expect("render GridWarp mix=1");
    assert_ne!(unwarped.fnv1a_hash64(), warped.fnv1a_hash64());
}

#[test]
fn engine_lut3d_cube_and_ocio_configs() {
    let temp = unique_temp_dir("lut3d_cube_ocio");

    // 1. Write a 2x2x2 .cube LUT that maps every color toward strong green [0.0, 1.0, 0.0]
    let cube_path = temp.0.join("force_green.cube");
    let mut cube = String::from(
        "TITLE \"ForceGreen\"\nLUT_3D_SIZE 2\nDOMAIN_MIN 0.0 0.0 0.0\nDOMAIN_MAX 1.0 1.0 1.0\n",
    );
    for _b in 0..2 {
        for _g in 0..2 {
            for _r in 0..2 {
                cube.push_str("0.0 1.0 0.0\n");
            }
        }
    }
    std::fs::write(&cube_path, cube).expect("write .cube");

    // 2. Write an OCIO v1 config with .spi1d + MatrixTransform (same math as ocio.rs unit test)
    let luts_dir = temp.0.join("luts");
    std::fs::create_dir_all(&luts_dir).expect("create luts dir");
    let mut spi1d = String::from("Version 1\nFrom 0.0 1.0\nLength 17\nComponents 1\n{\n");
    for i in 0..17 {
        let x = i as f32 / 16.0;
        spi1d.push_str(&format!("  {}\n", x * x));
    }
    spi1d.push_str("}\n");
    std::fs::write(luts_dir.join("gamma2.spi1d"), spi1d).expect("write spi1d");

    let ocio_path = temp.0.join("config.ocio");
    let ocio_yaml = r#"
ocio_profile_version: 1
search_path: luts

roles:
  scene_linear: linear_709

colorspaces:
  - !<ColorSpace>
    name: linear_709
    family: Linear
    isdata: false

  - !<ColorSpace>
    name: CustomLook
    family: Look
    to_reference: !<GroupTransform>
      children:
        - !<FileTransform>
          src: gamma2.spi1d
          interpolation: linear
        - !<MatrixTransform>
          matrix: [1.0, 0.0, 0.0, 0.0,
                   0.0, 0.2, 0.0, 0.0,
                   0.0, 0.0, 1.0, 0.0,
                   0.0, 0.0, 0.0, 1.0]
          offset: [0.5, 0.0, 0.0, 0.0]
"#;
    std::fs::write(&ocio_path, ocio_yaml).expect("write config.ocio");

    let mut project = minimal_render_project(
        64,
        64,
        RenderGraphDef {
            output_node: NodeId::from("lut"),
            nodes: vec![
                GraphNodeDef {
                    id: NodeId::from("src"),
                    kind: NodeKind::Solid {
                        color_rgba: [0.5, 0.5, 0.5, 1.0],
                        color_space: ColorSpace::LinearRec709,
                    },
                    inputs: Vec::new(),
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("lut"),
                    kind: NodeKind::Lut3D {
                        input: NodeId::from("src"),
                        lut_preset: String::new(),
                        intensity: 1.0,
                        lut_source: Some(LutSourceDef::CubeFile {
                            path: cube_path.to_string_lossy().into_owned(),
                            lut_size_override: None,
                        }),
                        input_space: None,
                        output_space: None,
                    },
                    inputs: vec![NodeId::from("src")],
                    enabled: true,
                },
            ],
        },
    );

    let mut engine = Engine::new_headless().expect("engine");
    let cube_frame = engine
        .render_frame_at(&project, RenderProfileKind::Draft, Flicks::ZERO)
        .expect("render Lut3D CubeFile");
    let cpx = cube_frame.get_pixel(32, 32).unwrap();
    assert!(
        cpx[1] > 230 && cpx[0] < 20 && cpx[2] < 20,
        "CubeFile ForceGreen expected pure green, got {cpx:?}"
    );

    project.graph.nodes[1].kind = NodeKind::Lut3D {
        input: NodeId::from("src"),
        lut_preset: String::new(),
        intensity: 1.0,
        lut_source: Some(LutSourceDef::Ocio {
            config_path: ocio_path.to_string_lossy().into_owned(),
            src_space: "CustomLook".into(),
            dst_space: "scene_linear".into(),
            look: None,
            lut_size: 17,
            shaper_space: None,
        }),
        input_space: None,
        output_space: None,
    };
    let ocio_frame = engine
        .render_frame_at(&project, RenderProfileKind::Draft, Flicks::ZERO)
        .expect("render Lut3D Ocio");
    let opx = ocio_frame.get_pixel(32, 32).unwrap();
    // For input 0.5: 0.5^2 = 0.25 -> R = 0.25 + 0.5 = 0.75 (sRGB ~225), G = 0.25*0.2 = 0.05 (sRGB ~64)
    assert!(
        opx[0] > 190 && opx[1] < 100 && opx[0] > opx[1] + 90,
        "OCIO CustomLook should boost red and attenuate green, got {opx:?}"
    );
}

fn small_draft_project(label: &str, duration: Flicks) -> (TempDirGuard, Project) {
    let temp = unique_temp_dir(label);
    let mut project = Project::golden_scene_preset();
    project.timeline.duration_ticks = duration;
    project.render_profiles.draft.resolution = Resolution {
        width: 64,
        height: 36,
    };
    project.render_profiles.draft.motion_blur_sub_samples = 1;
    (temp, project)
}

fn default_options() -> RenderOptions {
    RenderOptions {
        output_alpha_mode: OutputAlphaMode::Straight,
        ..Default::default()
    }
}

#[test]
fn range_render_matches_full_render_hashes_and_slices_audio() {
    let (temp, project) = small_draft_project("range_render", Flicks(FLICKS_PER_SECOND / 2));
    let mut engine = Engine::new_headless().expect("engine");
    let full = engine
        .render_video_mp4(&project, RenderProfileKind::Draft, &temp.0.join("full.mp4"))
        .expect("full render");
    assert_eq!(full.frame_count, 30);
    assert_eq!(full.range, None);
    assert!(full.has_audio);

    let range = RenderRange::resolve(
        &project,
        FPS_60.frame_to_ticks(10).unwrap(),
        FPS_60.frame_to_ticks(20).unwrap(),
    )
    .expect("range resolves");
    let partial = engine
        .render_video_mp4_range(
            &project,
            RenderProfileKind::Draft,
            &temp.0.join("range.mp4"),
            range,
        )
        .expect("range render");
    assert_eq!(partial.frame_count, 10);
    assert_eq!(partial.frame_hashes, full.frame_hashes[10..20]);
    assert_eq!(partial.range, Some(range));
    assert_eq!(partial.duration_ticks, project.timeline.duration_ticks);
    assert!(partial.has_audio);

    let probed =
        FfmpegBridge::probe_media_info(&temp.0.join("range.mp4")).expect("probe range mp4");
    assert_eq!(probed.width, 64);
    assert_eq!(probed.height, 36);
    assert!(probed.has_audio);
}

#[test]
fn range_render_skips_audio_when_no_clip_intersects() {
    let (temp, mut project) = small_draft_project("range_no_audio", Flicks(FLICKS_PER_SECOND / 2));
    project.timeline.audio_clips[0].timeline_end = Flicks(FLICKS_PER_SECOND / 5);
    let range = RenderRange::resolve(
        &project,
        Flicks(FLICKS_PER_SECOND / 4),
        Flicks(FLICKS_PER_SECOND / 2),
    )
    .expect("range resolves");
    let mut engine = Engine::new_headless().expect("engine");
    let summary = engine
        .render_video_mp4_range(
            &project,
            RenderProfileKind::Draft,
            &temp.0.join("silent.mp4"),
            range,
        )
        .expect("silent range render");
    assert!(!summary.has_audio);
    let probed =
        FfmpegBridge::probe_media_info(&temp.0.join("silent.mp4")).expect("probe silent mp4");
    assert!(!probed.has_audio);
}

#[test]
fn failed_render_removes_partial_output_file() {
    let (temp, mut project) = small_draft_project("partial_output", Flicks(FLICKS_PER_SECOND / 10));
    let out = temp.0.join("partial.mp4");
    // Force a per-frame render failure after the streaming encoder has been started.
    project.assets[0].path = "does_not_exist/real_clip.mp4".into();
    project.assets[0].id = "custom_missing".into();
    project.timeline.clips[0].asset_id = "custom_missing".into();
    if let NodeKind::SourceVideo { asset_id, .. } = &mut project.graph.nodes[0].kind {
        *asset_id = "custom_missing".into();
    }
    let mut engine = Engine::new_headless().expect("engine");
    let err = engine
        .render_video_mp4(&project, RenderProfileKind::Draft, &out)
        .expect_err("missing asset must fail the render");
    assert!(matches!(err, CoreError::Render(_)));
    assert!(
        !out.exists(),
        "failed render must not leave a partial MP4 behind"
    );
}

#[test]
fn render_progress_reports_frames_audio_and_encode_stages() {
    let (temp, project) = small_draft_project("render_progress", Flicks(FLICKS_PER_SECOND / 10));
    let mut engine = Engine::new_headless().expect("engine");
    let mut events = Vec::new();
    let summary = engine
        .render_video_mp4_with_progress(
            &project,
            RenderProfileKind::Draft,
            &temp.0.join("progress.mp4"),
            None,
            &default_options(),
            &mut |event| events.push(event),
        )
        .expect("render with progress");
    assert_eq!(summary.frame_count, 6);
    assert_eq!(events.first(), Some(&RenderProgress::audio_mix(0, 1)));
    assert_eq!(
        events
            .iter()
            .filter(|event| event.stage == RenderStage::Frames)
            .count(),
        7
    );
    assert_eq!(
        events
            .iter()
            .find(|event| event.stage == RenderStage::Frames),
        Some(&RenderProgress::frames(0, 6))
    );
    assert!(events.contains(&RenderProgress::audio_mix(1, 1)));
    assert!(events.contains(&RenderProgress::encode(0, 1)));
    assert_eq!(events.last(), Some(&RenderProgress::encode(1, 1)));
}

#[test]
fn audio_slice_length_matches_ticks_and_first_sample() {
    let (full, _) = SyntheticMediaGenerator::generate_beat_audio(
        Flicks(2 * FLICKS_PER_SECOND),
        120,
        DEFAULT_AUDIO_SAMPLE_RATE,
    );
    let start = Flicks(FLICKS_PER_SECOND / 2);
    let duration = Flicks(FLICKS_PER_SECOND / 4);
    let sliced = slice_audio_buffer(&full, start, duration).expect("slice audio");
    let expected_frames =
        ticks_to_audio_samples_nearest(duration, DEFAULT_AUDIO_SAMPLE_RATE).expect("sample count");
    assert_eq!(sliced.frame_count(), expected_frames as usize);
    let start_sample = ticks_to_audio_samples_nearest(start, DEFAULT_AUDIO_SAMPLE_RATE)
        .expect("start sample") as usize;
    let channels = full.channels as usize;
    assert_eq!(
        &sliced.samples[..channels],
        &full.samples[start_sample * channels..start_sample * channels + channels]
    );
    assert_eq!(sliced.sample_rate, DEFAULT_AUDIO_SAMPLE_RATE);
}
