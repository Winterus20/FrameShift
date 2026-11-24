use frameshift_core::{
    DslCompileOptions, Engine, LintOptions, LintSeverity, NleExportOptions, NleImportOptions,
    PlanarTrackProjectOptions, PointTrackProjectOptions, SmartCacheConfig, SmartCacheMode,
    TrackPlanarOptions, TrackPointOptions, TrackerBindTarget, calculate_template_metadata_with_beats,
    compile_motion_str, diff_frame, discover_ai_models, export_project_fcpxml_to_file,
    export_project_otio_to_file, import_project_fcpxml_from_file, import_project_otio_from_file,
    instantiate_parametric_template_with_beats, lint_project, nle_markers_from_beats,
    verify_manifest_sha256,
};
use frameshift_media::{
    AlphaMode, BandOnsetSummary, FfmpegBridge, MultibandBeatReport, Rgba8Frame, srgb_to_linear,
};
use frameshift_schema::template_parametric::{
    BeatSelectionSource, DurationPolicy, MetadataRulesDef, MotionBlueprintDef,
    PARAMETRIC_TEMPLATE_SPEC_VERSION, ParametricTemplateSpec, TemplatePropDef, TemplatePropKind,
};
use frameshift_schema::{
    AnimationEasing, AnimationKeyframe, AssetDef, AssetKind, BlendMode, ColorSpace,
    EventAnchorDef, EventBoundAnimation, EventOrigin, EventTimingConstraint, GraphNodeDef,
    MatteMode, NodeId, NodeKind, ParamBindingDef, Project, RenderGraphDef, RenderProfileKind,
    Resolution, SourceFit, TextAlign2D, WritingMode2D,
};
use frameshift_time::{FLICKS_PER_SECOND, FPS_24, FPS_60, Flicks, Rational32, SpeedCurve};
use std::collections::BTreeMap;
use std::path::PathBuf;

struct TempDirGuard(PathBuf);

impl TempDirGuard {
    fn new(prefix: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "frameshift_{prefix}_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp test dir");
        Self(dir)
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn minimal_text_node(y_offset_norm: f32, x_offset_norm: Option<f32>, color_rgba: [f32; 4]) -> NodeKind {
    NodeKind::VectorText {
        text: "TEST".into(),
        font_size_norm: 0.08,
        color_rgba,
        outline_rgba: [0.0, 0.0, 0.0, 1.0],
        outline_width_norm: 0.0,
        align_center: true,
        y_offset_norm,
        font_family: None,
        font_path: None,
        font_weight: 700,
        align: Some(TextAlign2D::Center),
        x_offset_norm,
        tracking_em: None,
        line_height_em: None,
        opacity: 1.0,
        fallback_fonts: Vec::new(),
        writing_mode: WritingMode2D::HorizontalTb,
        var_wght: None,
        var_wdth: None,
        var_slnt: None,
        color_bottom_rgba: None,
        gradient_angle_deg: 0.0,
        outer_outline_rgba: [0.0, 0.0, 0.0, 0.0],
        outer_outline_width_norm: 0.0,
        outline_gap_norm: 0.0,
        shadow_offset_norm: [0.0, 0.0],
        shadow_blur_norm: 0.0,
        shadow_rgba: [0.0, 0.0, 0.0, 0.0],
        glow_radius_norm: 0.0,
        glow_rgba: [0.0, 0.0, 0.0, 0.0],
        hdr_intensity: 1.0,
        gpu_slug: false,
        text_animator: None,
    }
}

#[test]
fn p3_nv12_decode_and_smart_cache_preserve_exact_pixels_across_fps() {
    let tmp = TempDirGuard::new("p3_nv12_cache");
    let mp4_path = tmp.0.join("source_24fps.mp4");
    let src_frames: Vec<Rgba8Frame> = (0..6)
        .map(|i| {
            let pts = FPS_24.frame_to_ticks(i).unwrap();
            let mut frame =
                Rgba8Frame::filled(160, 120, pts, [40, 120, 210, 255], AlphaMode::Straight)
                    .unwrap();
            let rx = (i as u32 * 16) % 100;
            for y in 30..70 {
                for x in rx..(rx + 30) {
                    frame.set_pixel(x, y, [240, 70, 50, 255]);
                }
            }
            frame
        })
        .collect();
    FfmpegBridge::encode_mp4(&src_frames, None, FPS_24, &mp4_path).expect("encode mp4");

    let mut project = Project::golden_scene_preset();
    project.param_bindings.clear();
    project.timeline.clips[0].event_animations.clear();
    project.timeline.fps = FPS_60;
    project.timeline.duration_ticks = Flicks(FLICKS_PER_SECOND / 4);
    project.timeline.clips.truncate(1);
    project.timeline.clips[0].timeline_end = project.timeline.duration_ticks;
    project.timeline.clips[0].speed_curve = SpeedCurve::constant(1.0).unwrap();
    project.assets[0].path = mp4_path.to_string_lossy().into_owned();
    project.assets[0].fps = Some(FPS_24);
    project.assets[0].resolution = Some(Resolution {
        width: 160,
        height: 120,
    });
    project.render_profiles.draft.resolution = Resolution {
        width: 160,
        height: 120,
    };
    project.render_profiles.draft.motion_blur_sub_samples = 1;
    project.graph = RenderGraphDef {
        output_node: NodeId::from("src_contain"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("src_contain"),
            kind: NodeKind::SourceVideo {
                asset_id: "asset_clip_a".into(),
                clip_id: Some("clip_a".into()),
                color_space: ColorSpace::DisplaySrgb,
                fit: SourceFit::Contain,
            },
            inputs: Vec::new(),
            enabled: true,
        }],
    };

    let mut engine_cached = Engine::new_headless().expect("cached engine");
    assert_eq!(
        engine_cached.smart_cache_config().mode,
        SmartCacheMode::MemoryOnly
    );
    let mut engine_uncached = Engine::new_headless_with_smart_cache(SmartCacheConfig {
        mode: SmartCacheMode::Disabled,
        ..Default::default()
    })
    .expect("uncached engine");

    // Consecutive 60fps ticks map to the same 24fps source frame, triggering SmartCache GPU hits.
    for f_idx in 0..6 {
        let tick = FPS_60.frame_to_ticks(f_idx).unwrap();
        let a = engine_cached
            .render_frame_at(&project, RenderProfileKind::Draft, tick)
            .expect("cached render");
        let b = engine_uncached
            .render_frame_at(&project, RenderProfileKind::Draft, tick)
            .expect("uncached render");
        assert_eq!(a.data, b.data, "frame {f_idx} mismatch with SmartCache");
    }

    let stats = engine_cached.smart_cache_stats();
    assert!(
        stats.gpu_hits >= 3,
        "expected >= 3 GPU SmartCache hits on 24->60fps duplicate frames, got {stats:?}"
    );
    engine_cached.clear_cache();
    engine_uncached.clear_cache();
}

#[test]
fn p2_lint_catches_all_four_silent_traps_and_structural_rules() {
    let tmp = TempDirGuard::new("p2_lint");
    let black_mp4 = tmp.0.join("black_start.mp4");

    // 6 black frames + 6 bright frames @ 24fps (0.25s black + 0.25s bright)
    let mut frames = Vec::new();
    for i in 0..6 {
        let pts = FPS_24.frame_to_ticks(i).unwrap();
        frames.push(Rgba8Frame::filled(64, 64, pts, [0, 0, 0, 255], AlphaMode::Straight).unwrap());
    }
    for i in 6..12 {
        let pts = FPS_24.frame_to_ticks(i).unwrap();
        frames.push(
            Rgba8Frame::filled(64, 64, pts, [220, 120, 60, 255], AlphaMode::Straight).unwrap(),
        );
    }
    FfmpegBridge::encode_mp4(&frames, None, FPS_24, &black_mp4).expect("encode black mp4");

    let mut project = Project::golden_scene_preset();
    project.assets[0].path = black_mp4.to_string_lossy().into_owned();
    project.assets[0].duration_ticks = None; // LNT-MAT-001 missing duration_ticks on disk asset
    project.timeline.clips[0].timeline_end = Flicks(FLICKS_PER_SECOND / 5);
    project.timeline.clips[0].speed_curve = SpeedCurve::constant(1.0).unwrap();

    // Inject silent traps + structural/security/perf issues:
    project.graph.nodes.push(GraphNodeDef {
        id: NodeId::from("offscreen_txt"),
        kind: minimal_text_node(1.4, Some(-0.2), [0.5, 0.5, 0.5, 1.0]), // LNT-TXT-001 + LNT-CLR-001
        inputs: Vec::new(),
        enabled: true,
    });
    project.graph.nodes.push(GraphNodeDef {
        id: NodeId::from("opaque_solid"),
        kind: NodeKind::Solid {
            color_rgba: [0.4, 0.4, 0.4, 1.0],
            color_space: ColorSpace::LinearRec709, // LNT-CLR-001
        },
        inputs: Vec::new(),
        enabled: true,
    });
    project.graph.nodes.push(GraphNodeDef {
        id: NodeId::from("cover_blend"),
        kind: NodeKind::Blend {
            mode: BlendMode::Normal,
            opacity: 1.0,
        },
        inputs: vec![NodeId::from("src_a"), NodeId::from("opaque_solid")], // LNT-CMP-001
        enabled: true,
    });
    project.graph.nodes.push(GraphNodeDef {
        id: NodeId::from("zero_adj"),
        kind: NodeKind::AdjustmentLayer {
            start_tick: Flicks(100),
            end_tick: Flicks(100), // LNT-TIM-001 Error
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
        },
        inputs: vec![NodeId::from("cover_blend")],
        enabled: true,
    });
    project.graph.nodes.push(GraphNodeDef {
        id: NodeId::from("matte_black"),
        kind: NodeKind::MaskMatte {
            mode: MatteMode::Luma,
            invert: false,
            feather: 0.0,
            choke_px: 0.0,
            spatial_feather_px: 0.0,
            opacity: 1.0,
            opaque_black_outside: true,
        },
        inputs: vec![NodeId::from("zero_adj"), NodeId::from("src_b")],
        enabled: true,
    });
    project.graph.nodes.push(GraphNodeDef {
        id: NodeId::from("lut_after_matte"),
        kind: NodeKind::Lut3D {
            input: NodeId::from("matte_black"), // LNT-MAT-001
            lut_preset: "anime_cyber_contrast".into(),
            intensity: 0.5,
            lut_source: None,
            input_space: None,
            output_space: None,
        },
        inputs: vec![NodeId::from("matte_black")],
        enabled: true,
    });
    project.graph.nodes.push(GraphNodeDef {
        id: NodeId::from("nested_precomp"),
        kind: NodeKind::Precomp {
            sub_graph: RenderGraphDef {
                output_node: NodeId::from("inner_blur"),
                nodes: vec![
                    GraphNodeDef {
                        id: NodeId::from("inner_wgsl"),
                        kind: NodeKind::CustomWgsl {
                            wgsl_source: "// test".into(),
                            expected_inputs: 0,
                            params: [0.0; 16],
                        }, // LNT-SEC-001
                        inputs: Vec::new(),
                        enabled: true,
                    },
                    GraphNodeDef {
                        id: NodeId::from("inner_blur"),
                        kind: NodeKind::MotionBlurShutter {
                            shutter_angle_deg: 180.0,
                            sub_samples: 24, // LNT-PERF-001 high sub_samples + nested blur
                            phase: Default::default(),
                            tonemap: true,
                        },
                        inputs: vec![NodeId::from("inner_wgsl")],
                        enabled: true,
                    },
                ],
            },
            time_offset_ticks: 0,
            time_scale: 1.0,
            opacity: 1.0,
        },
        inputs: Vec::new(),
        enabled: true,
    });
    project.graph.nodes.push(GraphNodeDef {
        id: NodeId::from("dead_shock"),
        kind: NodeKind::RadialShockwave {
            center_uv: [0.5, 0.5],
            radius: 0.0, // LNT-DAG-001 unreachable + LNT-MAT-001 static 0 radius
            ring_width: 0.1,
            refraction_amp: 0.03,
            chromatic_aberration: 0.01,
            zoom_blur_strength: 0.0,
        },
        inputs: vec![NodeId::from("src_a")],
        enabled: false,
    });
    project.graph.nodes.push(GraphNodeDef {
        id: NodeId::from("combine_all"),
        kind: NodeKind::Blend {
            mode: BlendMode::Normal,
            opacity: 1.0,
        },
        inputs: vec![NodeId::from("lut_after_matte"), NodeId::from("nested_precomp")],
        enabled: true,
    });
    if let Some(out) = project
        .graph
        .nodes
        .iter_mut()
        .find(|n| n.id == project.graph.output_node)
    {
        out.inputs = vec![NodeId::from("combine_all")];
    }
    project.param_bindings.push(ParamBindingDef {
        node_id: NodeId::from("dead_shock"), // LNT-DAG-002 disabled node target
        param_name: "zoom_blur_strength".into(),
        expression: "0.5".into(),
    });
    project.timeline.clips[0].event_animations.push(EventBoundAnimation {
        id: "bad_anim".into(),
        event_id: "drop_impact".into(),
        target_node_id: NodeId::from("dead_shock"),
        parameter: "zoom_blur_strength".into(), // LNT-DAG-002 conflict + empty keyframes + 0 duration
        offset_from_event: Flicks::ZERO,
        duration: Flicks::ZERO,
        keyframes: vec![],
        hold_after: false,
    });
    project.timeline.clips[0].event_animations.push(EventBoundAnimation {
        id: "valid_anim_stub".into(),
        event_id: "drop_impact".into(),
        target_node_id: NodeId::from("offscreen_txt"),
        parameter: "opacity".into(),
        offset_from_event: Flicks::ZERO,
        duration: Flicks(100),
        keyframes: vec![AnimationKeyframe {
            progress: 0.5,
            value: 1.0,
            easing: AnimationEasing::Linear,
        }],
        hold_after: true,
    });

    // Without check_media: no LNT-MED-001
    let report_no_media = lint_project(&project, &LintOptions::default());
    assert!(!report_no_media.ok);
    let codes: Vec<&str> = report_no_media
        .findings
        .iter()
        .map(|f| f.code.as_str())
        .collect();
    for expected in [
        "LNT-TXT-001",
        "LNT-CLR-001",
        "LNT-CMP-001",
        "LNT-DAG-001",
        "LNT-DAG-002",
        "LNT-TIM-001",
        "LNT-SEC-001",
        "LNT-PERF-001",
        "LNT-MAT-001",
    ] {
        assert!(
            codes.contains(&expected),
            "expected {expected} in {codes:?}"
        );
    }
    assert!(!codes.contains(&"LNT-MED-001"));

    // With check_media + strict_sandbox: LNT-MED-001 fires & LNT-SEC-001 becomes Error.
    let report_media = lint_project(
        &project,
        &LintOptions {
            check_media: true,
            strict_sandbox: true,
            ..Default::default()
        },
    );
    assert!(
        report_media
            .findings
            .iter()
            .any(|f| f.code == "LNT-MED-001"),
        "expected LNT-MED-001 when check_media=true"
    );
    assert!(
        report_media
            .findings
            .iter()
            .any(|f| f.code == "LNT-SEC-001" && f.severity == LintSeverity::Error)
    );
}

#[test]
fn p2_diff_frame_computes_metrics_bbox_and_3panel_png() {
    let tmp = TempDirGuard::new("p2_diff");
    let a = Rgba8Frame::filled(64, 48, Flicks::ZERO, [20, 30, 40, 255], AlphaMode::Straight).unwrap();
    let mut b = a.clone();

    let identical = diff_frame(&a, &b, None).expect("identical diff");
    assert!(identical.identical);
    assert_eq!(identical.mae, 0.0);
    assert_eq!(identical.rmse, 0.0);
    assert_eq!(identical.psnr_db, None);
    assert_eq!(identical.changed_pixels, 0);
    assert_eq!(identical.changed_bbox, None);

    for y in 12..27 {
        for x in 10..30 {
            b.set_pixel(x, y, [220, 180, 90, 255]);
        }
    }
    let diff_png = tmp.0.join("diff_3panel.png");
    let report = diff_frame(&a, &b, Some(&diff_png)).expect("diff with png");
    assert!(!report.identical);
    assert!(report.mae > 0.0);
    assert!(report.rmse > 0.0);
    assert!(report.psnr_db.is_some());
    assert_eq!(report.changed_pixels, 300);
    assert_eq!(report.changed_bbox, Some([10, 12, 20, 15]));
    assert!(diff_png.is_file());

    let loaded = Rgba8Frame::load_png(&diff_png, Flicks::ZERO).expect("load 3panel png");
    assert_eq!(loaded.width, 64 * 3);
    assert_eq!(loaded.height, 48);
}

#[test]
fn p2_motion_dsl_compiles_deterministically_renders_and_rejects_lint_errors() {
    let script = r#"
---
name: "Asta Mini Drop"
fps: 24
res: 320x180
bpm: 120
lut: anime_cyber_contrast(0.65)
---
asset clip_a video path="synthetic://clip_a"
asset clip_b video path="synthetic://clip_b"
asset bgm audio path="synthetic://beat"

audio a_bgm asset=bgm range=0s..3s src=0s bus=music fade_out=0.25s
shot c1 asset=clip_a range=0s..1.5s src=0s fit=cover
shot c2 asset=clip_b range=1.5s..3s src=0s fit=cover
beat_drop drop_1 at=1.5s clip=c1 src=1.4s label="Main Drop"
transition whip_chromatic between=c1,c2 at=@drop_1 dur=0.25s whip=0.85 ca=0.03
fx xenoz_impact_glow on=tr_01 at=@drop_1 dur=0.4s glow_intensity=1.1
text "DEVIL UNION" at=1.5s..2.8s pos=(0.5, 0.78) size=0.08 color=#FFF5EB anim=fade
shutter angle=180 samples=4 phase=centered
"#;
    let out1 = compile_motion_str(script, &DslCompileOptions::default()).expect("compile 1");
    let out2 = compile_motion_str(script, &DslCompileOptions::default()).expect("compile 2");
    assert_eq!(out1.project.to_json_pretty().unwrap(), out2.project.to_json_pretty().unwrap());
    assert!(out1.lint_report.ok);
    assert_eq!(out1.project.schema_version.minor, 2);

    // Verify sRGB -> Linear Rec.709 conversion on `#FFF5EB`:
    let txt_node = out1
        .project
        .graph
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::VectorText { .. }))
        .expect("VectorText node");
    if let NodeKind::VectorText { color_rgba, .. } = txt_node.kind {
        assert!((color_rgba[1] - srgb_to_linear(0xF5 as f32 / 255.0)).abs() < 1e-4);
        assert!((color_rgba[2] - srgb_to_linear(0xEB as f32 / 255.0)).abs() < 1e-4);
    }

    // Render drop frame via Engine:
    let mut engine = Engine::new_headless().expect("engine");
    let frame = engine
        .render_frame_at(&out1.project, RenderProfileKind::Final, Flicks(FLICKS_PER_SECOND * 3 / 2))
        .expect("render compiled DSL frame");
    assert_eq!(frame.width, 320);
    assert_eq!(frame.height, 180);

    // Off-screen text must be rejected by the post-compile lint gate:
    let bad_script = r#"
---
name: "Broken Text"
fps: 24
res: 320x180
---
asset clip_a video path="synthetic://clip_a"
shot c1 asset=clip_a range=0s..1s
text "OFFSCREEN" at=0s..1s pos=(0.5, 1.4)
"#;
    assert!(compile_motion_str(bad_script, &DslCompileOptions::default()).is_err());
}

#[test]
fn p2_parametric_template_adapts_cuts_and_duration_to_bpm_and_props() {
    let spec = ParametricTemplateSpec {
        spec_version: PARAMETRIC_TEMPLATE_SPEC_VERSION,
        template_id: "beat_montage_v1".into(),
        name: "Beat Montage".into(),
        description: "BPM-adaptive montage".into(),
        props_schema: vec![
            TemplatePropDef {
                id: "clips".into(),
                label: "Clips".into(),
                kind: TemplatePropKind::VideoClipList {
                    min_items: 2,
                    max_items: 8,
                },
                required: true,
                default_value: None,
            },
            TemplatePropDef {
                id: "bgm".into(),
                label: "BGM".into(),
                kind: TemplatePropKind::AudioFile,
                required: true,
                default_value: None,
            },
            TemplatePropDef {
                id: "title".into(),
                label: "Title".into(),
                kind: TemplatePropKind::Text { max_len: Some(32) },
                required: false,
                default_value: Some(serde_json::json!("HYPER DROP")),
            },
            TemplatePropDef {
                id: "aspect".into(),
                label: "Aspect".into(),
                kind: TemplatePropKind::Choice {
                    options: vec!["16:9".into(), "9:16".into()],
                },
                required: false,
                default_value: Some(serde_json::json!("16:9")),
            },
        ],
        metadata_rules: MetadataRulesDef {
            default_fps: Rational32 { num: 24, den: 1 },
            aspect_presets: BTreeMap::from([
                (
                    "16:9".into(),
                    Resolution {
                        width: 320,
                        height: 180,
                    },
                ),
                (
                    "9:16".into(),
                    Resolution {
                        width: 180,
                        height: 320,
                    },
                ),
            ]),
            duration_policy: DurationPolicy::BeatCount { beats: 8 },
            beat_source: BeatSelectionSource::BeatGrid,
            cuts_per_beat: Rational32 { num: 1, den: 1 },
            min_segment_ms: 200,
            transition_shader: "whip_chromatic".into(),
            transition_dur_ms: 160,
            drop_fx_preset_id: Some("xenoz_impact_glow".into()),
            drop_fx_window_ms: [-100, 300],
        },
        motion_blueprint: MotionBlueprintDef::AutoCut {
            include_title_prop: Some("title".into()),
            shutter_angle_deg: Some(180.0),
            shutter_samples: Some(2),
        },
        review: Default::default(),
    };

    let props = serde_json::json!({
        "clips": ["synthetic://clip_a", "synthetic://clip_b"],
        "bgm": "synthetic://beat",
        "aspect": "9:16"
    });

    let slow_beats = MultibandBeatReport {
        window_ticks: Flicks(FLICKS_PER_SECOND / 100),
        fft_size: 4096,
        estimated_bpm: 90.0,
        tempo_confidence: 0.9,
        beats_per_bar: 4,
        downbeat_phase: 0,
        bar_phase_confidence: 0.9,
        beat_grid: (0..8)
            .map(|i| Flicks(i * (FLICKS_PER_SECOND * 2 / 3)))
            .collect(),
        downbeats: vec![Flicks(0), Flicks(FLICKS_PER_SECOND * 8 / 3)],
        sub: BandOnsetSummary {
            onsets: vec![],
            flux_envelope: vec![],
            energy_envelope: vec![],
        },
        mid: BandOnsetSummary {
            onsets: vec![],
            flux_envelope: vec![],
            energy_envelope: vec![],
        },
        high: BandOnsetSummary {
            onsets: vec![],
            flux_envelope: vec![],
            energy_envelope: vec![],
        },
    };
    let fast_beats = MultibandBeatReport {
        estimated_bpm: 180.0,
        beat_grid: (0..8).map(|i| Flicks(i * (FLICKS_PER_SECOND / 3))).collect(),
        downbeats: vec![Flicks(0), Flicks(FLICKS_PER_SECOND * 4 / 3)],
        ..slow_beats.clone()
    };

    let meta_slow = calculate_template_metadata_with_beats(&spec, &props, Some(&slow_beats))
        .expect("slow meta");
    let (project_fast, meta_fast, lint) =
        instantiate_parametric_template_with_beats(&spec, &props, Some(&fast_beats))
            .expect("fast instantiate");

    assert_eq!(meta_fast.resolved_resolution.width, 180);
    assert_eq!(meta_fast.resolved_resolution.height, 320);
    assert!(meta_slow.resolved_duration_ticks > meta_fast.resolved_duration_ticks);
    assert!(meta_fast.cut_count >= 4);
    assert!(lint.ok);
    assert!(project_fast.validate().is_ok());

    let mut engine = Engine::new_headless().expect("engine");
    let rendered = engine
        .render_frame_at(&project_fast, RenderProfileKind::Final, Flicks::ZERO)
        .expect("render template frame");
    assert_eq!(rendered.width, 180);
    assert_eq!(rendered.height, 320);
}

#[test]
fn p5_tracker_bridge_nle_roundtrip_and_ai_manifest_work_end_to_end() {
    let tmp = TempDirGuard::new("p5_bridge");

    // 1. Point + Planar Tracker -> Project.keyframe_tracks via Engine:
    let mut project = Project::golden_scene_preset();
    project.param_bindings.clear();
    project.timeline.clips[0].event_animations.clear();
    project.timeline.fps = FPS_24;
    project.timeline.duration_ticks = Flicks(FLICKS_PER_SECOND / 4); // 6 frames
    project.timeline.clips.truncate(1);
    project.timeline.clips[0].timeline_end = project.timeline.duration_ticks;
    project.timeline.clips[0].speed_curve = SpeedCurve::constant(1.0).unwrap();
    project.render_profiles.draft.resolution = Resolution {
        width: 128,
        height: 72,
    };
    project.render_profiles.draft.motion_blur_sub_samples = 1;
    project.graph = RenderGraphDef {
        output_node: NodeId::from("corner_out"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("src_a"),
                kind: NodeKind::SourceVideo {
                    asset_id: "asset_clip_a".into(),
                    clip_id: Some("clip_a".into()),
                    color_space: ColorSpace::DisplaySrgb,
                    fit: SourceFit::Stretch,
                },
                inputs: Vec::new(),
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("follow_2d"),
                kind: NodeKind::Transform2D {
                    translate: [0.0, 0.0],
                    scale: [1.0, 1.0],
                    rotation_rad: 0.0,
                    anchor: [0.5, 0.5],
                    filter: Default::default(),
                },
                inputs: vec![NodeId::from("src_a")],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("corner_out"),
                kind: NodeKind::CornerPin {
                    tl: [0.0, 0.0],
                    tr: [1.0, 0.0],
                    br: [1.0, 1.0],
                    bl: [0.0, 1.0],
                    opacity: 1.0,
                    filter: Default::default(),
                },
                inputs: vec![NodeId::from("follow_2d")],
                enabled: true,
            },
        ],
    };

    let mut engine = Engine::new_headless().expect("engine");
    let track_end_tick = project.timeline.duration_ticks;
    let pt_report = engine
        .track_point_to_project(
            &mut project,
            &PointTrackProjectOptions {
                source_node_id: NodeId::from("src_a"),
                target_node_id: NodeId::from("follow_2d"),
                bind_target: TrackerBindTarget::Transform2D,
                track_id_prefix: "pt_1".into(),
                start_tick: Flicks::ZERO,
                end_tick: track_end_tick,
                start_point_uv: [0.5, 0.5],
                point_options: TrackPointOptions::default(),
                profile: RenderProfileKind::Draft,
            },
        )
        .expect("track point to project");
    assert_eq!(pt_report.frames_tracked, 6);
    assert!(!project.keyframe_tracks.is_empty());

    let pl_report = engine
        .track_planar_to_project(
            &mut project,
            &PlanarTrackProjectOptions {
                source_node_id: NodeId::from("src_a"),
                target_node_id: NodeId::from("corner_out"),
                bind_target: TrackerBindTarget::CornerPin,
                track_id_prefix: "pl_1".into(),
                start_tick: Flicks::ZERO,
                end_tick: track_end_tick,
                start_corners_uv: [[0.25, 0.25], [0.75, 0.25], [0.75, 0.75], [0.25, 0.75]],
                planar_options: TrackPlanarOptions::default(),
                profile: RenderProfileKind::Draft,
            },
        )
        .expect("track planar to project");
    assert_eq!(pl_report.frames_tracked, 6);
    assert!(project.keyframe_tracks.len() >= 10);

    let tracked_frame = engine
        .render_frame_at(&project, RenderProfileKind::Draft, FPS_24.frame_to_ticks(2).unwrap())
        .expect("render tracked frame");
    assert_eq!(tracked_frame.width, 128);

    // 2. NLE .otio + .fcpxml export & re-import via frameshift-core bridge:
    let golden = Project::golden_scene_preset();
    let beat_report = MultibandBeatReport {
        window_ticks: Flicks(FLICKS_PER_SECOND / 100),
        fft_size: 4096,
        estimated_bpm: 120.0,
        tempo_confidence: 1.0,
        beats_per_bar: 4,
        downbeat_phase: 0,
        bar_phase_confidence: 1.0,
        beat_grid: vec![Flicks(0), Flicks(FLICKS_PER_SECOND / 2), Flicks(FLICKS_PER_SECOND)],
        downbeats: vec![Flicks(0)],
        sub: BandOnsetSummary {
            onsets: vec![],
            flux_envelope: vec![],
            energy_envelope: vec![],
        },
        mid: BandOnsetSummary {
            onsets: vec![],
            flux_envelope: vec![],
            energy_envelope: vec![],
        },
        high: BandOnsetSummary {
            onsets: vec![],
            flux_envelope: vec![],
            energy_envelope: vec![],
        },
    };
    let nle_opts = NleExportOptions {
        extra_markers: nle_markers_from_beats(&beat_report, FPS_60.frame_duration().unwrap()),
    };
    let otio_path = tmp.0.join("timeline.otio");
    let fcpxml_path = tmp.0.join("timeline.fcpxml");
    export_project_otio_to_file(&golden, &otio_path, &nle_opts).expect("export otio");
    export_project_fcpxml_to_file(&golden, &fcpxml_path, &nle_opts).expect("export fcpxml");

    let imported_otio =
        import_project_otio_from_file(&otio_path, &NleImportOptions::default()).expect("import otio");
    let imported_fcpxml = import_project_fcpxml_from_file(&fcpxml_path, &NleImportOptions::default())
        .expect("import fcpxml");
    assert!(imported_otio.validate().is_ok());
    assert!(imported_fcpxml.validate().is_ok());
    assert!(!imported_otio.timeline.clips.is_empty());
    assert!(!imported_fcpxml.timeline.clips.is_empty());

    // 3. AI Manifest discovery + SHA-256 verification via frameshift-core re-exports:
    let model_dir = tmp.0.join("models").join("demo_matte");
    std::fs::create_dir_all(&model_dir).unwrap();
    let onnx_path = model_dir.join("demo.onnx");
    let onnx_bytes = b"synthetic-onnx-bytes";
    std::fs::write(&onnx_path, onnx_bytes).unwrap();
    let hash = FfmpegBridge::hash_bytes(onnx_bytes).sha256;
    let manifest_json = serde_json::json!({
        "schema_version": "frameshift.ai_model.v1",
        "id": "demo_matte",
        "display_name": "Demo Matte",
        "version": "1.0.0",
        "task": "matte",
        "runtime": "onnx_pipe",
        "entrypoint": "demo.onnx",
        "sha256": hash,
        "license": "MIT"
    });
    std::fs::write(
        model_dir.join("model_manifest.json"),
        serde_json::to_string_pretty(&manifest_json).unwrap(),
    )
    .unwrap();
    let discovered = discover_ai_models(&tmp.0.join("models")).expect("discover models");
    assert_eq!(discovered.len(), 1);
    assert_eq!(discovered[0].manifest.id, "demo_matte");
    assert!(verify_manifest_sha256(&discovered[0].manifest, &onnx_path, false).is_ok());

    // Tampered file must fail strict SHA-256 check:
    std::fs::write(&onnx_path, b"tampered-bytes").unwrap();
    assert!(verify_manifest_sha256(&discovered[0].manifest, &onnx_path, false).is_err());

    let _ = EventTimingConstraint::Absolute;
    let _ = EventAnchorDef {
        event_id: "x".into(),
        label: "x".into(),
        source_clip_id: None,
        source_anchor_tick: None,
        target_output_tick: Flicks::ZERO,
        origin: EventOrigin::UserMarked,
        timing: EventTimingConstraint::Absolute,
    };
    let _ = AssetDef {
        id: "a".into(),
        kind: AssetKind::Video,
        path: "p".into(),
        duration_ticks: None,
        fps: None,
        sample_rate_hz: None,
        resolution: None,
    };
}
