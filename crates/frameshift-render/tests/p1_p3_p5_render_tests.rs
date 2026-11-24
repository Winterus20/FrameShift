use frameshift_render::{
    ChromaLocation, FnFrameProvider, GpuCompositor, NodeAnimator, NoopAnimator, OutputAlphaMode,
    ProjectAnimator, RenderOptions, ShaderPipelines, Sha256, SmartCacheConfig, SmartCacheMode,
    SourceFrame, TonemapMode, TrackPlanarOptions, TrackPointOptions, TrackerBindTarget,
    YuvColorInfo, YuvColorMatrix, YuvColorRange, YuvTransfer, planar_track_to_keyframes,
    point_track_to_keyframes, readback_rgba16f_roi, track_planar_sequence, track_point_sequence,
};
use frameshift_schema::{
    AnimationEasing, CURRENT_SCHEMA_VERSION, ColorSpace, DisplacementMode,
    DisplacementWrap, EchoOperator, GraphNodeDef, KeyframeTrackDef, MatteMode, NodeId, NodeKind,
    ParamBindingDef, Project, RenderGraphDef, RenderProfileDef, RenderProfileKind,
    RenderProfilesDef, Resolution, SampleFilter, ShapeMaskKeyframe, ShapeMaskVertex, SourceFit,
    TimelineDef, TrackKeyframe, WorkingPixelFormat,
};
use frameshift_time::{FLICKS_PER_SECOND, FPS_24, Flicks};

fn test_opts() -> RenderOptions {
    RenderOptions {
        tonemap_mode: Some(TonemapMode::None),
        output_alpha_mode: OutputAlphaMode::Straight,
        ..Default::default()
    }
}

fn make_project(
    width: u32,
    height: u32,
    graph: RenderGraphDef,
    keyframe_tracks: Vec<KeyframeTrackDef>,
    param_bindings: Vec<ParamBindingDef>,
) -> Project {
    let profile = |kind: RenderProfileKind| RenderProfileDef {
        kind,
        resolution: Resolution { width, height },
        motion_blur_sub_samples: 1,
        shutter_angle_deg: 0.0,
        pixel_format: WorkingPixelFormat::Rgba16FloatPremul,
    };
    Project {
        schema_version: CURRENT_SCHEMA_VERSION,
        revision_id: 1,
        name: "p1_p3_p5_test".into(),
        assets: vec![],
        timeline: TimelineDef {
            fps: FPS_24,
            duration_ticks: Flicks(2 * FLICKS_PER_SECOND),
            resolution: Resolution { width, height },
            clips: vec![],
            audio_clips: vec![],
            ducking_rules: vec![],
            micro_silences: vec![],
        },
        events: vec![],
        graph,
        render_profiles: RenderProfilesDef {
            draft: profile(RenderProfileKind::Draft),
            final_profile: profile(RenderProfileKind::Final),
        },
        param_bindings,
        keyframe_tracks,
        creative_decisions: vec![],
        preferences: vec![],
        proxy_config: Default::default(),
        hw_accel_policy: Default::default(),
    }
}

#[test]
fn new_wave1_wgsl_shaders_compile_in_wgpu() {
    let ctx = frameshift_render::GpuContext::new_headless().expect("headless GPU context");
    let _pipelines = ShaderPipelines::compile_all(&ctx.device).expect("all WGSL pipelines compile");
}

#[test]
fn p1_keyframe_tracks_evaluate_and_layer_before_param_bindings() {
    let graph = RenderGraphDef {
        output_node: NodeId::from("xform"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("solid"),
                kind: NodeKind::Solid {
                    color_rgba: [1.0, 0.2, 0.2, 1.0],
                    color_space: ColorSpace::LinearRec709,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("xform"),
                kind: NodeKind::Transform2D {
                    translate: [0.0, 0.0],
                    scale: [1.0, 1.0],
                    rotation_rad: 0.0,
                    anchor: [0.5, 0.5],
                    filter: SampleFilter::Bilinear,
                },
                inputs: vec![NodeId::from("solid")],
                enabled: true,
            },
        ],
    };

    let tracks = vec![
        KeyframeTrackDef {
            id: "tx_track".into(),
            target_node: NodeId::from("xform"),
            target_param: "translate_x".into(),
            keyframes: vec![
                TrackKeyframe {
                    tick: Flicks(0),
                    value: 0.0,
                    easing: AnimationEasing::Linear,
                },
                TrackKeyframe {
                    tick: Flicks(FLICKS_PER_SECOND),
                    value: 0.4,
                    easing: AnimationEasing::Linear,
                },
            ],
        },
        KeyframeTrackDef {
            id: "ty_track".into(),
            target_node: NodeId::from("xform"),
            target_param: "translate_y".into(),
            keyframes: vec![
                TrackKeyframe {
                    tick: Flicks(0),
                    value: 0.1,
                    easing: AnimationEasing::Linear,
                },
                TrackKeyframe {
                    tick: Flicks(FLICKS_PER_SECOND),
                    value: 0.5,
                    easing: AnimationEasing::Linear,
                },
            ],
        },
    ];
    // Binding overrides translate_y after keyframe_tracks.
    let bindings = vec![ParamBindingDef {
        node_id: NodeId::from("xform"),
        param_name: "translate_y".into(),
        expression: "-0.25".into(),
    }];

    let project = make_project(64, 64, graph, tracks, bindings);
    project.validate().expect("project validates");
    let animator = ProjectAnimator::new(&project).expect("animator");
    let base = &project.graph.node(&NodeId::from("xform")).unwrap().kind;
    let eval_mid = animator
        .evaluate_node(&NodeId::from("xform"), base, Flicks(FLICKS_PER_SECOND / 2))
        .expect("eval mid");
    match eval_mid {
        NodeKind::Transform2D { translate, .. } => {
            assert!((translate[0] - 0.2).abs() < 1e-5);
            assert!((translate[1] - (-0.25)).abs() < 1e-5);
        }
        other => panic!("unexpected node kind: {other:?}"),
    }
}

#[test]
fn p1_displacement_map_shifts_and_respects_transparent_map_alpha() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let w = 64u32;
    let h = 64u32;

    // Source has left half red, right half blue.
    // Map has top half opaque bright red (R=1, A=1 -> positive +X displacement), bottom half transparent (A=0 -> 0 shift).
    let provider = FnFrameProvider(|asset_id: &str, _tick: Flicks, pw: u32, ph: u32| {
        let mut pixels = vec![[0.0f32; 4]; (pw * ph) as usize];
        if asset_id == "src_split" {
            for y in 0..ph {
                for x in 0..pw {
                    let px = if x < pw / 2 {
                        [1.0, 0.0, 0.0, 1.0]
                    } else {
                        [0.0, 0.0, 1.0, 1.0]
                    };
                    pixels[(y * pw + x) as usize] = px;
                }
            }
        } else {
            for y in 0..ph {
                for x in 0..pw {
                    let px = if y < ph / 2 {
                        [1.0, 0.5, 0.5, 1.0]
                    } else {
                        [0.0, 0.0, 0.0, 0.0]
                    };
                    pixels[(y * pw + x) as usize] = px;
                }
            }
        }
        Ok(SourceFrame::rgba_f32_premul(pw, ph, pixels))
    });

    let graph = RenderGraphDef {
        output_node: NodeId::from("disp"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("src"),
                kind: NodeKind::SourceVideo {
                    asset_id: "src_split".into(),
                    clip_id: None,
                    color_space: ColorSpace::LinearRec709,
                    fit: SourceFit::Stretch,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("map"),
                kind: NodeKind::SourceVideo {
                    asset_id: "disp_map".into(),
                    clip_id: None,
                    color_space: ColorSpace::LinearRec709,
                    fit: SourceFit::Stretch,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("disp"),
                kind: NodeKind::DisplacementMap {
                    channel_mode: DisplacementMode::Rg,
                    edge_mode: DisplacementWrap::Clamp,
                    scale_x: 0.4,
                    scale_y: 0.0,
                    neutral_level: 0.5,
                    angle_rad: 0.0,
                    chromatic_split: 0.0,
                    opacity: 1.0,
                },
                inputs: vec![NodeId::from("src"), NodeId::from("map")],
                enabled: true,
            },
        ],
    };

    let frame_ticks = FPS_24.frame_duration().unwrap();
    let rendered = compositor
        .render_frame(
            &graph,
            Flicks::ZERO,
            frame_ticks,
            w,
            h,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("displacement map render");

    // Top row, slightly right of center (x = 36, y = 16): shifted left toward red!
    let top_right = ((16 * w + 36) * 4) as usize;
    assert!(
        rendered.rgba8[top_right] > 200 && rendered.rgba8[top_right + 2] < 30,
        "top half with R=1,A=1 should pull red from left, got {:?}",
        &rendered.rgba8[top_right..top_right + 4]
    );

    // Bottom row, same x = 36, y = 48: map has A=0, so ZERO ghost shift -> remains blue!
    let bot_right = ((48 * w + 36) * 4) as usize;
    assert!(
        rendered.rgba8[bot_right + 2] > 200 && rendered.rgba8[bot_right] < 30,
        "bottom half with A=0 must have 0 displacement, got {:?}",
        &rendered.rgba8[bot_right..bot_right + 4]
    );
}

#[test]
fn p1_frame_echo_is_seek_independent_and_composites_trail() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let w = 64u32;
    let h = 64u32;
    let frame_ticks = FPS_24.frame_duration().unwrap();

    // Moving white square shifting +8 px per frame to the right.
    let provider = FnFrameProvider(|_asset_id: &str, tick: Flicks, pw: u32, ph: u32| {
        let frame_idx = (tick.0 / (FLICKS_PER_SECOND / 24)).max(0) as u32;
        let cx = 12 + frame_idx * 8;
        let mut pixels = vec![[0.0f32; 4]; (pw * ph) as usize];
        for y in 24..40 {
            for x in cx.saturating_sub(4)..(cx + 4).min(pw) {
                pixels[(y * pw + x) as usize] = [1.0, 1.0, 1.0, 1.0];
            }
        }
        Ok(SourceFrame::rgba_f32_premul(pw, ph, pixels))
    });

    let graph = RenderGraphDef {
        output_node: NodeId::from("echo"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("mover"),
                kind: NodeKind::SourceVideo {
                    asset_id: "box".into(),
                    clip_id: None,
                    color_space: ColorSpace::LinearRec709,
                    fit: SourceFit::Stretch,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("echo"),
                kind: NodeKind::FrameEcho {
                    echo_step_ticks: -frame_ticks.0,
                    echo_count: 3,
                    starting_intensity: 1.0,
                    decay: 0.5,
                    color_decay_rgb: [0.5, 0.8, 1.0],
                    hue_shift_deg: 0.0,
                    operator: EchoOperator::CompositeUnder,
                },
                inputs: vec![NodeId::from("mover")],
                enabled: true,
            },
        ],
    };

    // Seek directly to frame 3 without rendering frames 0..2 first.
    let seek_tick = Flicks(frame_ticks.0 * 3);
    let direct_seek = compositor
        .render_frame(
            &graph,
            seek_tick,
            frame_ticks,
            w,
            h,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("direct seek render");

    // Current box at frame 3 is at cx = 36 (bright white); echo 1 at cx = 28 (tinted blue-ish, alpha=0.5).
    let head_px = ((32 * w + 36) * 4) as usize;
    let trail_px = ((32 * w + 28) * 4) as usize;
    assert!(direct_seek.rgba8[head_px] > 240 && direct_seek.rgba8[head_px + 3] > 240);
    assert!(
        direct_seek.rgba8[trail_px + 3] >= 100 && direct_seek.rgba8[trail_px + 2] > direct_seek.rgba8[trail_px],
        "expected tinted ghost trail at cx=28, got {:?}",
        &direct_seek.rgba8[trail_px..trail_px + 4]
    );

    // Sequential render from 0..=3 on fresh compositor must match seek_tick bit-for-bit.
    let mut seq_comp = GpuCompositor::new_headless().expect("seq init");
    for f in 0..3 {
        let _ = seq_comp
            .render_frame(
                &graph,
                Flicks(frame_ticks.0 * f),
                frame_ticks,
                w,
                h,
                &provider,
                &NoopAnimator,
                &test_opts(),
            )
            .unwrap();
    }
    let seq_f3 = seq_comp
        .render_frame(
            &graph,
            seek_tick,
            frame_ticks,
            w,
            h,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .unwrap();
    assert_eq!(direct_seek.fnv1a_hash, seq_f3.fnv1a_hash);
}

#[test]
fn p1_shape_mask_morphs_feathers_on_gpu_and_feeds_mask_matte() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let w = 64u32;
    let h = 64u32;
    let frame_ticks = FPS_24.frame_duration().unwrap();

    let tri = vec![
        ShapeMaskVertex {
            pos: [0.5, 0.2],
            in_tangent: [0.0, 0.0],
            out_tangent: [0.0, 0.0],
        },
        ShapeMaskVertex {
            pos: [0.8, 0.8],
            in_tangent: [0.0, 0.0],
            out_tangent: [0.0, 0.0],
        },
        ShapeMaskVertex {
            pos: [0.2, 0.8],
            in_tangent: [0.0, 0.0],
            out_tangent: [0.0, 0.0],
        },
    ];
    let rect = vec![
        ShapeMaskVertex {
            pos: [0.2, 0.2],
            in_tangent: [0.0, 0.0],
            out_tangent: [0.0, 0.0],
        },
        ShapeMaskVertex {
            pos: [0.8, 0.2],
            in_tangent: [0.0, 0.0],
            out_tangent: [0.0, 0.0],
        },
        ShapeMaskVertex {
            pos: [0.8, 0.8],
            in_tangent: [0.0, 0.0],
            out_tangent: [0.0, 0.0],
        },
        ShapeMaskVertex {
            pos: [0.2, 0.8],
            in_tangent: [0.0, 0.0],
            out_tangent: [0.0, 0.0],
        },
    ];

    let graph = RenderGraphDef {
        output_node: NodeId::from("masked"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("solid"),
                kind: NodeKind::Solid {
                    color_rgba: [0.0, 1.0, 0.0, 1.0],
                    color_space: ColorSpace::LinearRec709,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("mask"),
                kind: NodeKind::ShapeMask {
                    vertices: vec![],
                    keyframes: vec![
                        ShapeMaskKeyframe {
                            tick: Flicks(0),
                            vertices: tri,
                            easing: AnimationEasing::Linear,
                        },
                        ShapeMaskKeyframe {
                            tick: Flicks(FLICKS_PER_SECOND),
                            vertices: rect,
                            easing: AnimationEasing::Linear,
                        },
                    ],
                    closed: true,
                    feather_px: 8.0,
                    expand_px: 0.0,
                    invert: false,
                    opacity: 1.0,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("masked"),
                kind: NodeKind::MaskMatte {
                    mode: MatteMode::Alpha,
                    invert: false,
                    feather: 0.0,
                    opaque_black_outside: false,
                    opacity: 1.0,
                    choke_px: 0.0,
                    spatial_feather_px: 0.0,
                },
                inputs: vec![NodeId::from("solid"), NodeId::from("mask")],
                enabled: true,
            },
        ],
    };

    let provider = FnFrameProvider(|_: &str, _: Flicks, _: u32, _: u32| unreachable!());
    let f_t0 = compositor
        .render_frame(
            &graph,
            Flicks::ZERO,
            frame_ticks,
            w,
            h,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("shape mask t=0");
    let f_t1 = compositor
        .render_frame(
            &graph,
            Flicks(FLICKS_PER_SECOND),
            frame_ticks,
            w,
            h,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("shape mask t=1");

    assert_ne!(f_t0.fnv1a_hash, f_t1.fnv1a_hash);
    // Center pixel (32, 32) is opaque green; corner (2, 2) is transparent; edge near (13, 32) is feathered (0 < a < 255).
    let center = ((32 * w + 32) * 4) as usize;
    let corner = ((2 * w + 2) * 4) as usize;
    let edge = ((32 * w + 12) * 4) as usize;
    assert!(f_t1.rgba8[center + 3] > 230 && f_t1.rgba8[center + 1] > 230);
    assert!(f_t1.rgba8[corner + 3] < 20);
    assert!(
        f_t1.rgba8[edge + 3] > 10 && f_t1.rgba8[edge + 3] < 245,
        "feathered edge alpha should be soft, got {}",
        f_t1.rgba8[edge + 3]
    );
}

#[test]
fn p3_gpu_nv12_yuv_to_linear_preserves_neutral_gray_and_contain_rect() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let dst_w = 64u32;
    let dst_h = 64u32;
    let src_w = 32u32;
    let src_h = 32u32;
    let frame_ticks = FPS_24.frame_duration().unwrap();

    // Neutral mid-gray in BT.709 Limited: Y = 126, U = 128, V = 128.
    // SourceFit::Contain rect = [0.25, 0.25, 0.75, 0.75].
    let provider = FnFrameProvider(|asset_id: &str, _tick: Flicks, _w: u32, _h: u32| {
        let y_plane = vec![126u8; (src_w * src_h) as usize];
        let uv_plane = vec![128u8; ((src_w / 2) * (src_h / 2) * 2) as usize];
        let color_info = YuvColorInfo {
            matrix: YuvColorMatrix::Bt709,
            range: YuvColorRange::Limited,
            chroma_location: ChromaLocation::Left,
            transfer: YuvTransfer::Srgb,
        };
        if asset_id == "nv12_alpha" {
            let mut alpha = vec![0u8; (src_w * src_h) as usize];
            for y in 0..src_h {
                for x in src_w / 2..src_w {
                    alpha[(y * src_w + x) as usize] = 255;
                }
            }
            Ok(SourceFrame::nv12_with_alpha(
                src_w,
                src_h,
                y_plane,
                uv_plane,
                alpha,
                color_info,
                [0.0, 0.0, 1.0, 1.0],
            ))
        } else {
            Ok(SourceFrame::nv12(
                src_w,
                src_h,
                y_plane,
                uv_plane,
                color_info,
                [0.25, 0.25, 0.75, 0.75],
            ))
        }
    });

    let contain_graph = RenderGraphDef {
        output_node: NodeId::from("nv12"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("nv12"),
            kind: NodeKind::SourceVideo {
                asset_id: "nv12_contain".into(),
                clip_id: None,
                color_space: ColorSpace::DisplaySrgb,
                fit: SourceFit::Contain,
            },
            inputs: vec![],
            enabled: true,
        }],
    };

    let rendered = compositor
        .render_frame(
            &contain_graph,
            Flicks::ZERO,
            frame_ticks,
            dst_w,
            dst_h,
            &provider,
            &NoopAnimator,
            &RenderOptions {
                tonemap_mode: Some(TonemapMode::None),
                output_alpha_mode: OutputAlphaMode::Premultiplied,
                ..Default::default()
            },
        )
        .expect("nv12 contain render");

    // Outside [0.25, 0.75] must be transparent black [0, 0, 0, 0].
    let outside = ((4 * dst_w + 4) * 4) as usize;
    assert_eq!(&rendered.rgba8[outside..outside + 4], &[0, 0, 0, 0]);

    // Inside [0.25, 0.75] must be exact neutral gray (R == G == B within 1 LSB).
    let inside = ((32 * dst_w + 32) * 4) as usize;
    let [r, g, b, a] = [
        rendered.rgba8[inside],
        rendered.rgba8[inside + 1],
        rendered.rgba8[inside + 2],
        rendered.rgba8[inside + 3],
    ];
    assert_eq!(a, 255);
    assert!(
        (r as i16 - g as i16).abs() <= 1 && (g as i16 - b as i16).abs() <= 1,
        "128/255 chroma center must yield neutral gray, got [{r},{g},{b},{a}]"
    );
    assert!((r as i16 - 128).abs() <= 3, "expected ~128 gray, got {r}");

    // Test optional R8 alpha plane.
    let alpha_graph = RenderGraphDef {
        output_node: NodeId::from("nv12a"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("nv12a"),
            kind: NodeKind::SourceVideo {
                asset_id: "nv12_alpha".into(),
                clip_id: None,
                color_space: ColorSpace::DisplaySrgb,
                fit: SourceFit::Stretch,
            },
            inputs: vec![],
            enabled: true,
        }],
    };
    let rendered_a = compositor
        .render_frame(
            &alpha_graph,
            Flicks::ZERO,
            frame_ticks,
            dst_w,
            dst_h,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("nv12 alpha render");
    let left_px = ((32 * dst_w + 8) * 4) as usize;
    let right_px = ((32 * dst_w + 48) * 4) as usize;
    assert_eq!(&rendered_a.rgba8[left_px..left_px + 4], &[0, 0, 0, 0]);
    assert_eq!(rendered_a.rgba8[right_px + 3], 255);
}

#[test]
fn p3_smart_cache_sha256_and_three_tier_hit_eviction_and_determinism() {
    // 1. Standard NIST SHA-256 vector check ("abc").
    let mut h = Sha256::new();
    h.update(b"abc");
    let digest = h.finalize();
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(
        hex,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );

    // 2. Heavy graph: Solid -> DeepGlow.
    let w = 64u32;
    let h_px = 64u32;
    let frame_ticks = FPS_24.frame_duration().unwrap();
    let graph = RenderGraphDef {
        output_node: NodeId::from("glow"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("bg"),
                kind: NodeKind::Solid {
                    color_rgba: [1.5, 0.8, 0.3, 1.0],
                    color_space: ColorSpace::LinearRec709,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("glow"),
                kind: NodeKind::DeepGlow {
                    threshold: 0.4,
                    knee: 0.5,
                    intensity: 1.2,
                    radius: 1.5,
                    tint_rgb: [1.0, 0.9, 0.8],
                    mip_levels: 4,
                    scatter: 0.65,
                    anamorphic_ratio: 0.0,
                    chromatic_shift: 0.0,
                },
                inputs: vec![NodeId::from("bg")],
                enabled: true,
            },
        ],
    };
    let provider = FnFrameProvider(|_: &str, _: Flicks, _: u32, _: u32| unreachable!());

    // Baseline with SmartCache Disabled.
    let mut comp_off = GpuCompositor::new_headless_with_cache(SmartCacheConfig {
        mode: SmartCacheMode::Disabled,
        ..Default::default()
    })
    .expect("off init");
    let off_frame = comp_off
        .render_frame(
            &graph,
            Flicks::ZERO,
            frame_ticks,
            w,
            h_px,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .unwrap();

    // Tier 1 + Tier 2 + Tier 3 Disk cache.
    let temp_dir =
        std::env::temp_dir().join(format!("frameshift_smart_cache_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);

    let cfg_disk = SmartCacheConfig {
        mode: SmartCacheMode::MemoryAndDisk,
        gpu_max_bytes: 192 * 1024 * 1024,
        cpu_max_bytes: 128 * 1024 * 1024,
        disk_max_bytes: 32 * 1024 * 1024,
        cache_dir: Some(temp_dir.clone()),
        ..Default::default()
    };
    let mut comp = GpuCompositor::new_headless_with_cache(cfg_disk.clone()).expect("cache init");
    let f1 = comp
        .render_frame(
            &graph,
            Flicks::ZERO,
            frame_ticks,
            w,
            h_px,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .unwrap();
    assert_eq!(f1.fnv1a_hash, off_frame.fnv1a_hash);
    assert_eq!(comp.smart_cache_stats().gpu_hits, 0);

    // Second frame hits Tier 1 GPU cache (both bg and glow!).
    let f2 = comp
        .render_frame(
            &graph,
            Flicks(frame_ticks.0),
            frame_ticks,
            w,
            h_px,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .unwrap();
    assert_eq!(f2.fnv1a_hash, off_frame.fnv1a_hash);
    assert!(comp.smart_cache_stats().gpu_hits >= 2);

    // Force Tier 1 GPU eviction (gpu_max_bytes = 0) -> heavy node DeepGlow hits Tier 2 CPU RAM!
    let mut cfg_cpu_only_tier = cfg_disk.clone();
    cfg_cpu_only_tier.gpu_max_bytes = 0;
    comp.set_smart_cache_config(cfg_cpu_only_tier);
    let f3 = comp
        .render_frame(
            &graph,
            Flicks(frame_ticks.0 * 2),
            frame_ticks,
            w,
            h_px,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .unwrap();
    assert_eq!(f3.fnv1a_hash, off_frame.fnv1a_hash);
    assert!(comp.smart_cache_stats().cpu_hits >= 1);

    // Fresh compositor with same disk cache_dir -> heavy node DeepGlow hits Tier 3 Disk!
    let mut comp_cold_ram = GpuCompositor::new_headless_with_cache(cfg_disk).expect("disk init");
    let f4 = comp_cold_ram
        .render_frame(
            &graph,
            Flicks(frame_ticks.0 * 3),
            frame_ticks,
            w,
            h_px,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .unwrap();
    assert_eq!(f4.fnv1a_hash, off_frame.fnv1a_hash);
    assert!(comp_cold_ram.smart_cache_stats().disk_hits >= 1);

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn p5_point_and_planar_tracker_recovers_motion_and_emits_keyframes() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let w = 96u32;
    let h = 96u32;
    let frame_ticks = FPS_24.frame_duration().unwrap();

    // Moving high-contrast textured patch translating +3 px/frame in X and +2 px/frame in Y.
    let provider = FnFrameProvider(|_asset_id: &str, tick: Flicks, pw: u32, ph: u32| {
        let f = (tick.0 / (FLICKS_PER_SECOND / 24)).max(0) as f32;
        let cx = 36.0 + f * 3.0;
        let cy = 36.0 + f * 2.0;
        let mut pixels = vec![[0.08f32, 0.08, 0.1, 1.0]; (pw * ph) as usize];
        for y in 0..ph {
            for x in 0..pw {
                let dx = x as f32 - cx;
                let dy = y as f32 - cy;
                if dx.abs() <= 14.0 && dy.abs() <= 14.0 {
                    let pattern = 0.5 + 0.4 * (dx * 0.55).sin() * (dy * 0.55).cos();
                    let border = if dx.abs() > 11.0 || dy.abs() > 11.0 {
                        0.95
                    } else {
                        pattern
                    };
                    pixels[(y * pw + x) as usize] = [border, border * 0.85, 0.3, 1.0];
                }
            }
        }
        Ok(SourceFrame::rgba_f32_premul(pw, ph, pixels))
    });

    let graph = RenderGraphDef {
        output_node: NodeId::from("src"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("src"),
            kind: NodeKind::SourceVideo {
                asset_id: "track_clip".into(),
                clip_id: None,
                color_space: ColorSpace::LinearRec709,
                fit: SourceFit::Stretch,
            },
            inputs: vec![],
            enabled: true,
        }],
    };

    let ticks: Vec<Flicks> = (0..5).map(|i| Flicks(frame_ticks.0 * i)).collect();
    let start_uv = [36.0 / w as f32, 36.0 / h as f32];
    let pt_res = track_point_sequence(
        &mut compositor,
        &graph,
        &NodeId::from("src"),
        &ticks,
        frame_ticks,
        w,
        h,
        &provider,
        &NoopAnimator,
        start_uv,
        &TrackPointOptions::default(),
    )
    .expect("point track");

    assert_eq!(pt_res.samples.len(), 5);
    assert!(pt_res.lost_ticks.is_empty(), "lost ticks: {:?}", pt_res.lost_ticks);
    assert!(
        pt_res.mean_fb_error_px <= 1.5,
        "FB error too high: {}",
        pt_res.mean_fb_error_px
    );
    let last_pt = pt_res.samples.last().unwrap();
    let expected_end = [(36.0 + 12.0) / w as f32, (36.0 + 8.0) / h as f32];
    let err_px = (((last_pt.point_uv[0] - expected_end[0]) * w as f32).powi(2)
        + ((last_pt.point_uv[1] - expected_end[1]) * h as f32).powi(2))
    .sqrt();
    assert!(
        err_px <= 2.5,
        "expected point near {expected_end:?}, got {:?} (err={err_px:.2}px)",
        last_pt.point_uv
    );

    let xform_kfs = point_track_to_keyframes(
        "pt",
        &NodeId::from("xform"),
        TrackerBindTarget::Transform2D,
        &pt_res,
        w,
        h,
        None,
    );
    assert_eq!(xform_kfs.len(), 2);

    // 4-corner planar tracker test on the same translating textured patch.
    let start_corners = [
        [24.0 / w as f32, 24.0 / h as f32],
        [48.0 / w as f32, 24.0 / h as f32],
        [48.0 / w as f32, 48.0 / h as f32],
        [24.0 / w as f32, 48.0 / h as f32],
    ];
    let planar_res = track_planar_sequence(
        &mut compositor,
        &graph,
        &NodeId::from("src"),
        &ticks,
        frame_ticks,
        w,
        h,
        &provider,
        &NoopAnimator,
        start_corners,
        &TrackPlanarOptions::default(),
    )
    .expect("planar track");

    assert_eq!(planar_res.samples.len(), 5);
    assert!(planar_res.lost_ticks.is_empty());
    let last_planar = planar_res.samples.last().unwrap();
    let planar_err_px = (((last_planar.center_uv[0] - expected_end[0]) * w as f32).powi(2)
        + ((last_planar.center_uv[1] - expected_end[1]) * h as f32).powi(2))
    .sqrt();
    assert!(
        planar_err_px <= 3.0,
        "planar center err={planar_err_px:.2}px, center={:?}",
        last_planar.center_uv
    );

    let cp_kfs = planar_track_to_keyframes(
        "cp",
        &NodeId::from("pin"),
        TrackerBindTarget::CornerPin,
        &planar_res,
        w,
        h,
        None,
    );
    assert_eq!(cp_kfs.len(), 8);

    // Direct 32x32 ROI readback verification.
    let tex = compositor
        .evaluate_subframe(&graph, Flicks::ZERO, frame_ticks, w, h, &provider, &NoopAnimator)
        .expect("subframe");
    let roi = readback_rgba16f_roi(
        &compositor.context().device,
        &compositor.context().queue,
        &tex.texture,
        w,
        h,
        16,
        16,
        32,
        32,
    )
    .expect("32x32 roi readback");
    assert_eq!(roi.len(), 32 * 32);
    compositor.texture_pool_mut().release(tex);
}
