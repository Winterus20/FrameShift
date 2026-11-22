use frameshift_render::{
    FnFrameProvider, GpuCompositor, NoopAnimator, OutputAlphaMode, RenderError, RenderOptions,
    SourceFrame, TonemapMode, compositor::remap_precomp_tick,
};
use frameshift_schema::{
    AssetDef, AssetKind, BlendMode, CURRENT_SCHEMA_VERSION, ColorSpace, CubicBezierSegmentDef,
    ExtendedColorSpace, GraphNodeDef, LutSourceDef, MAX_PRECOMP_DEPTH, NodeId, NodeKind,
    ParamBindingDef, Project, RenderGraphDef, RenderProfileDef, RenderProfileKind,
    RenderProfilesDef, Resolution, SampleFilter, SchemaError, ShutterPhase, SourceFit,
    TextAlign2D, TimelineDef, WorkingPixelFormat,
};
use frameshift_time::{FLICKS_PER_SECOND, FPS_30, FPS_60, Flicks};
use frameshift_vector::VectorError;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

fn test_opts() -> RenderOptions {
    RenderOptions {
        tonemap_mode: Some(TonemapMode::None),
        output_alpha_mode: OutputAlphaMode::Straight,
        ..Default::default()
    }
}

fn make_temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "frameshift_faz6_4_{label}_{}_{}",
        std::process::id(),
        nanos
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn make_test_project(
    name: &str,
    assets: Vec<AssetDef>,
    graph: RenderGraphDef,
    param_bindings: Vec<ParamBindingDef>,
    width: u32,
    height: u32,
) -> Project {
    let profile = |kind| RenderProfileDef {
        kind,
        resolution: Resolution { width, height },
        motion_blur_sub_samples: 1,
        shutter_angle_deg: 0.0,
        pixel_format: WorkingPixelFormat::Rgba16FloatPremul,
    };
    Project {
        schema_version: CURRENT_SCHEMA_VERSION,
        revision_id: 1,
        name: name.into(),
        assets,
        timeline: TimelineDef {
            fps: FPS_60,
            duration_ticks: Flicks(2 * FLICKS_PER_SECOND),
            resolution: Resolution { width, height },
            clips: vec![],
            audio_clips: vec![],
        },
        events: vec![],
        graph,
        render_profiles: RenderProfilesDef {
            draft: profile(RenderProfileKind::Draft),
            final_profile: profile(RenderProfileKind::Final),
        },
        param_bindings,
        creative_decisions: vec![],
        preferences: vec![],
        proxy_config: Default::default(),
        hw_accel_policy: Default::default(),
    }
}

fn checkerboard_frame(w: u32, h: u32) -> SourceFrame {
    let mut data = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let idx = ((y * w + x) * 4) as usize;
            let checker = if ((x / 8) + (y / 8)).is_multiple_of(2) {
                235u8
            } else {
                35u8
            };
            data[idx] = ((x * 255) / w.max(1)) as u8;
            data[idx + 1] = ((y * 255) / h.max(1)) as u8;
            data[idx + 2] = checker;
            data[idx + 3] = 255;
        }
    }
    SourceFrame::rgba8_straight(w, h, data)
}

fn find_windows_ttf() -> String {
    for candidate in [
        r"C:\Windows\Fonts\arial.ttf",
        r"C:\Windows\Fonts\arialbd.ttf",
        r"C:\Windows\Fonts\calibri.ttf",
        r"C:\Windows\Fonts\segoeui.ttf",
        r"C:\Windows\Fonts\impact.ttf",
    ] {
        if Path::new(candidate).exists() {
            return candidate.to_string();
        }
    }
    panic!("expected at least one standard Windows TTF font in C:\\Windows\\Fonts");
}

#[test]
fn cube_1d_and_combined_1d_3d_lut_plus_cache_key_and_color_bridge() {
    let temp_dir = make_temp_dir("cube_lut");
    let cube_1d_path = temp_dir.join("invert_1d.cube");
    let mut cube_1d = String::from("TITLE \"Invert1D\"\nLUT_1D_SIZE 5\n");
    for i in 0..5 {
        let v = 1.0 - (i as f32 / 4.0);
        cube_1d.push_str(&format!("{v:.6} {v:.6} {v:.6}\n"));
    }
    std::fs::write(&cube_1d_path, &cube_1d).expect("write 1d cube");

    let shaper_cube_path = temp_dir.join("shaper_plus_3d.cube");
    let mut combined = String::from(
        "TITLE \"Combined1D3D\"\n\
         LUT_1D_SIZE 5\n\
         LUT_1D_INPUT_RANGE -0.5 1.5\n\
         LUT_3D_SIZE 2\n\
         LUT_3D_INPUT_RANGE 0.0 1.0\n",
    );
    for i in 0..5 {
        let v = i as f32 / 4.0;
        combined.push_str(&format!("{v:.6} {v:.6} {v:.6}\n"));
    }
    for b in 0..2 {
        for g in 0..2 {
            for r in 0..2 {
                combined.push_str(&format!(
                    "{:.6} {:.6} {:.6}\n",
                    r as f32 * 0.8,
                    g as f32 * 0.5,
                    b as f32 * 0.25
                ));
            }
        }
    }
    std::fs::write(&shaper_cube_path, &combined).expect("write combined cube");

    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let frame_ticks = FPS_60.frame_duration().unwrap();
    let provider = FnFrameProvider(|_: &str, _: Flicks, _: u32, _: u32| {
        unreachable!("Solid source does not query provider")
    });

    let build_graph = |lut_source: LutSourceDef,
                       input_space: Option<ExtendedColorSpace>,
                       output_space: Option<ExtendedColorSpace>| RenderGraphDef {
        output_node: NodeId::from("lut"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("src"),
                kind: NodeKind::Solid {
                    color_rgba: [0.5, 0.5, 0.5, 1.0],
                    color_space: ColorSpace::LinearRec709,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("lut"),
                kind: NodeKind::Lut3D {
                    input: NodeId::from("src"),
                    lut_preset: String::new(),
                    intensity: 1.0,
                    lut_source: Some(lut_source),
                    input_space,
                    output_space,
                },
                inputs: vec![NodeId::from("src")],
                enabled: true,
            },
        ],
    };

    // 1. Standalone 1D .cube (0.5 linear -> 0.5 inverted = 0.5; test [0.2, 0.8, 0.1] instead!)
    let mut g_1d = build_graph(
        LutSourceDef::CubeFile {
            path: cube_1d_path.to_string_lossy().into_owned(),
            lut_size_override: None,
        },
        None,
        None,
    );
    g_1d.nodes[0].kind = NodeKind::Solid {
        color_rgba: [0.2, 0.8, 0.1, 1.0],
        color_space: ColorSpace::LinearRec709,
    };
    let out_1d = compositor
        .render_frame(
            &g_1d,
            Flicks::ZERO,
            frame_ticks,
            8,
            8,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("1D cube render");
    // Expected inverted linear [0.8, 0.2, 0.9] -> sRGB approx [231, 124, 243]
    assert!(
        out_1d.rgba8[0] > 215 && out_1d.rgba8[1] < 140 && out_1d.rgba8[2] > 230,
        "1D invert LUT should invert linear RGB on GPU, got {:?}",
        &out_1d.rgba8[0..4]
    );

    // 2. Combined 1D shaper + 3D LUT .cube:
    // Input linear 0.5 -> shaper [-0.5, 1.5] -> 0.5 -> 3D [0.4, 0.25, 0.125]
    let g_combined = build_graph(
        LutSourceDef::CubeFile {
            path: shaper_cube_path.to_string_lossy().into_owned(),
            lut_size_override: None,
        },
        None,
        None,
    );
    let out_combined = compositor
        .render_frame(
            &g_combined,
            Flicks::ZERO,
            frame_ticks,
            8,
            8,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("combined 1D+3D cube render");
    assert!(
        out_combined.rgba8[0] > out_combined.rgba8[1]
            && out_combined.rgba8[1] > out_combined.rgba8[2]
            && out_combined.rgba8[2] > 70,
        "combined 1D+3D LUT should produce R > G > B gradient scale, got {:?}",
        &out_combined.rgba8[0..4]
    );

    // 3. Color space bridge: Identity LUT with DisplaySrgb -> DisplaySrgb preserves LinearRec709 input
    let g_bridge_same = build_graph(
        LutSourceDef::Preset {
            name: "identity".into(),
        },
        Some(ExtendedColorSpace::DisplaySrgb),
        Some(ExtendedColorSpace::DisplaySrgb),
    );
    let out_bridge_same = compositor
        .render_frame(
            &g_bridge_same,
            Flicks::ZERO,
            frame_ticks,
            8,
            8,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("bridged srgb->srgb render");
    // 0.5 linear -> sRGB ~ 0.735 -> byte ~ 187..188
    assert!(
        (out_bridge_same.rgba8[0] as i16 - 188).abs() <= 3,
        "identity DisplaySrgb->DisplaySrgb bridge must preserve 0.5 linear (~188 sRGB), got {:?}",
        &out_bridge_same.rgba8[0..4]
    );

    // Different input_space/output_space must NOT collide in LutCacheKey!
    let g_bridge_diff = build_graph(
        LutSourceDef::Preset {
            name: "identity".into(),
        },
        Some(ExtendedColorSpace::LinearRec709),
        Some(ExtendedColorSpace::DisplaySrgb),
    );
    let out_bridge_diff = compositor
        .render_frame(
            &g_bridge_diff,
            Flicks::ZERO,
            frame_ticks,
            8,
            8,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("bridged linear->srgb render");
    assert_ne!(
        out_bridge_same.fnv1a_hash, out_bridge_diff.fnv1a_hash,
        "LutCacheKey must distinguish differing input_space/output_space"
    );

    // 4. Modifying the .cube file on disk must invalidate the LutCacheKey via mtime/len fingerprint
    let mut updated_cube = String::from("LUT_3D_SIZE 2\n");
    for b in 0..2 {
        for g in 0..2 {
            for r in 0..2 {
                updated_cube.push_str(&format!(
                    "{:.6} {:.6} {:.6}\n",
                    r as f32 * 0.1,
                    g as f32 * 0.9,
                    b as f32 * 0.2
                ));
            }
        }
    }
    std::fs::write(&shaper_cube_path, &updated_cube).expect("overwrite cube file");
    let out_updated = compositor
        .render_frame(
            &g_combined,
            Flicks::ZERO,
            frame_ticks,
            8,
            8,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("re-render modified cube file");
    assert_ne!(
        out_combined.fnv1a_hash, out_updated.fnv1a_hash,
        "updated .cube file on disk must invalidate GPU LUT cache"
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn ocio_yaml_file_and_matrix_transform_and_builtin_transform_render_on_gpu() {
    let temp_dir = make_temp_dir("ocio_lut");
    let luts_dir = temp_dir.join("luts");
    std::fs::create_dir_all(&luts_dir).expect("create luts dir");

    // External 3D LUT inside search_path that swaps R and B channels
    let swap_cube_path = luts_dir.join("swap_rb.cube");
    let mut swap_cube = String::from("LUT_3D_SIZE 2\n");
    for b in 0..2 {
        for g in 0..2 {
            for r in 0..2 {
                swap_cube.push_str(&format!("{b}.0 {g}.0 {r}.0\n"));
            }
        }
    }
    std::fs::write(&swap_cube_path, swap_cube).expect("write swap_rb.cube");

    let ocio_path = temp_dir.join("config.ocio");
    let ocio_yaml = r#"
ocio_profile_version: 1
search_path: luts
roles:
  scene_linear: lin_ref
colorspaces:
  - !<ColorSpace>
    name: lin_ref
  - !<ColorSpace>
    name: grade_space
    from_reference: !<GroupTransform>
      children:
        - !<FileTransform> {src: swap_rb.cube, interpolation: linear}
        - !<MatrixTransform> {matrix: [0.5, 0, 0, 0,  0, 1.0, 0, 0,  0, 0, 1.0, 0,  0, 0, 0, 1]}
"#;
    std::fs::write(&ocio_path, ocio_yaml).expect("write config.ocio");

    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let frame_ticks = FPS_60.frame_duration().unwrap();
    let provider = FnFrameProvider(|_: &str, _: Flicks, _: u32, _: u32| {
        unreachable!("Solid source does not query provider")
    });

    // Source color: R=0.0, G=0.25, B=0.8 (linear)
    // FileTransform swaps R/B -> [0.8, 0.25, 0.0], MatrixTransform halves R -> [0.4, 0.25, 0.0]
    let ocio_graph = RenderGraphDef {
        output_node: NodeId::from("ocio"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("src"),
                kind: NodeKind::Solid {
                    color_rgba: [0.0, 0.25, 0.8, 1.0],
                    color_space: ColorSpace::LinearRec709,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("ocio"),
                kind: NodeKind::Lut3D {
                    input: NodeId::from("src"),
                    lut_preset: String::new(),
                    intensity: 1.0,
                    lut_source: Some(LutSourceDef::Ocio {
                        config_path: ocio_path.to_string_lossy().into_owned(),
                        src_space: "scene_linear".into(),
                        dst_space: "grade_space".into(),
                        look: None,
                        lut_size: 17,
                        shaper_space: None,
                    }),
                    input_space: None,
                    output_space: None,
                },
                inputs: vec![NodeId::from("src")],
                enabled: true,
            },
        ],
    };

    let ocio_out = compositor
        .render_frame(
            &ocio_graph,
            Flicks::ZERO,
            frame_ticks,
            8,
            8,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("OCIO FileTransform + MatrixTransform render");
    assert!(
        ocio_out.rgba8[0] > 155 && ocio_out.rgba8[1] > 120 && ocio_out.rgba8[2] <= 4,
        "OCIO swap_rb + 0.5*R should produce R > G > B~0, got {:?}",
        &ocio_out.rgba8[0..4]
    );

    // BuiltinTransform: DisplaySrgb -> LinearRec709 on linear [0.5, 0.5, 0.5] -> ~0.214 linear (~127 sRGB)
    let builtin_graph = RenderGraphDef {
        output_node: NodeId::from("builtin"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("src"),
                kind: NodeKind::Solid {
                    color_rgba: [0.5, 0.5, 0.5, 1.0],
                    color_space: ColorSpace::LinearRec709,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("builtin"),
                kind: NodeKind::Lut3D {
                    input: NodeId::from("src"),
                    lut_preset: String::new(),
                    intensity: 1.0,
                    lut_source: Some(LutSourceDef::BuiltinTransform {
                        src_space: ExtendedColorSpace::DisplaySrgb,
                        dst_space: ExtendedColorSpace::LinearRec709,
                        lut_size: 33,
                    }),
                    input_space: None,
                    output_space: None,
                },
                inputs: vec![NodeId::from("src")],
                enabled: true,
            },
        ],
    };

    let builtin_out = compositor
        .render_frame(
            &builtin_graph,
            Flicks::ZERO,
            frame_ticks,
            8,
            8,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("BuiltinTransform render");
    assert!(
        (builtin_out.rgba8[0] as i16 - 127).abs() <= 4,
        "DisplaySrgb->LinearRec709 on 0.5 should output ~127 sRGB, got {:?}",
        &builtin_out.rgba8[0..4]
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn source_image_caches_on_gpu_across_frames_and_motion_blur_and_respects_opacity() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let calls = AtomicUsize::new(0);
    let provider = FnFrameProvider(|asset_id: &str, tick: Flicks, w: u32, h: u32| {
        assert_eq!(asset_id, "img_plate");
        assert_eq!(tick, Flicks::ZERO);
        calls.fetch_add(1, Ordering::SeqCst);
        Ok(SourceFrame::rgba8_straight(
            w,
            h,
            [200u8, 100, 50, 255].repeat((w * h) as usize),
        ))
    });

    let make_graph = |opacity: f32, with_blur: bool| {
        let mut nodes = vec![GraphNodeDef {
            id: NodeId::from("img"),
            kind: NodeKind::SourceImage {
                asset_id: "img_plate".into(),
                color_space: ColorSpace::DisplaySrgb,
                fit: SourceFit::Cover,
                opacity,
            },
            inputs: vec![],
            enabled: true,
        }];
        let output_node = if with_blur {
            nodes.push(GraphNodeDef {
                id: NodeId::from("blur"),
                kind: NodeKind::MotionBlurShutter {
                    shutter_angle_deg: 180.0,
                    sub_samples: 4,
                    phase: ShutterPhase::Centered,
                    tonemap: false,
                },
                inputs: vec![NodeId::from("img")],
                enabled: true,
            });
            NodeId::from("blur")
        } else {
            NodeId::from("img")
        };
        RenderGraphDef { output_node, nodes }
    };

    let frame_ticks = FPS_60.frame_duration().unwrap();

    // Opacity <= 0.001 clears to transparent without invoking provider
    let zero_out = compositor
        .render_frame(
            &make_graph(0.0, false),
            Flicks::ZERO,
            frame_ticks,
            16,
            16,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("zero opacity source image");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(&zero_out.rgba8[0..4], &[0, 0, 0, 0]);

    // First opaque render + 4-sample MotionBlurShutter render must invoke provider ONLY ONCE!
    let f1 = compositor
        .render_frame(
            &make_graph(1.0, false),
            Flicks::ZERO,
            frame_ticks,
            16,
            16,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("first source image render");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(
        (f1.rgba8[0] as i16 - 200).abs() <= 2
            && (f1.rgba8[1] as i16 - 100).abs() <= 2
            && (f1.rgba8[2] as i16 - 50).abs() <= 2
            && f1.rgba8[3] == 255,
        "sRGB SourceImage roundtrip should preserve [200,100,50,255], got {:?}",
        &f1.rgba8[0..4]
    );

    let f_blur = compositor
        .render_frame(
            &make_graph(1.0, true),
            Flicks(FLICKS_PER_SECOND / 2),
            frame_ticks,
            16,
            16,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("motion blur over cached source image");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "image_gpu_cache must prevent re-fetching during MotionBlurShutter sub-samples"
    );
    assert_eq!(f_blur.fnv1a_hash, f1.fnv1a_hash);

    // Animated opacity = 0.5 via ProjectAnimator hits same GPU cache and scales alpha to ~128
    let mut project = make_test_project(
        "SourceImage Opacity Anim",
        vec![AssetDef {
            id: "img_plate".into(),
            kind: AssetKind::Image,
            path: "synthetic://img_plate".into(),
            duration_ticks: None,
            fps: None,
            sample_rate_hz: None,
            resolution: Some(Resolution {
                width: 16,
                height: 16,
            }),
        }],
        make_graph(1.0, false),
        vec![ParamBindingDef {
            node_id: NodeId::from("img"),
            param_name: "opacity".into(),
            expression: "0.5".into(),
        }],
        16,
        16,
    );
    let half_out = compositor
        .render_project_frame(
            &project,
            Flicks::ZERO,
            &project.render_profiles.draft,
            &provider,
            &test_opts(),
        )
        .expect("half opacity source image");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(
        (half_out.rgba8[3] as i16 - 128).abs() <= 2,
        "expected ~128 alpha at opacity=0.5, got {:?}",
        &half_out.rgba8[0..4]
    );

    // Changing fit must use a distinct GPU cache entry
    project.graph.nodes[0].kind = NodeKind::SourceImage {
        asset_id: "img_plate".into(),
        color_space: ColorSpace::DisplaySrgb,
        fit: SourceFit::Contain,
        opacity: 1.0,
    };
    project.param_bindings.clear();
    compositor
        .render_project_frame(
            &project,
            Flicks::ZERO,
            &project.render_profiles.draft,
            &provider,
            &test_opts(),
        )
        .expect("contain fit render");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn precomp_with_inputs_time_remap_opacity_depth_and_motion_blur_seek_independence() {
    assert_eq!(
        remap_precomp_tick(Flicks(1_000), 500, 1.0),
        Flicks(1_500)
    );
    assert_eq!(
        remap_precomp_tick(Flicks(1_000), -200, 2.5),
        Flicks(2_300)
    );

    let sub_graph = RenderGraphDef {
        output_node: NodeId::from("sub_blend"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("in_plate"),
                kind: NodeKind::PrecompInput { slot: 0 },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("in_tint"),
                kind: NodeKind::PrecompInput { slot: 1 },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("sub_xform"),
                kind: NodeKind::Transform2D {
                    translate: [0.0, 0.0],
                    scale: [1.0, 1.0],
                    rotation_rad: 0.0,
                    anchor: [0.5, 0.5],
                    filter: SampleFilter::Bilinear,
                },
                inputs: vec![NodeId::from("in_plate")],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("sub_blend"),
                kind: NodeKind::Blend {
                    mode: BlendMode::Add,
                    opacity: 0.35,
                },
                inputs: vec![NodeId::from("sub_xform"), NodeId::from("in_tint")],
                enabled: true,
            },
        ],
    };

    let build_project = |sub_samples: u32, precomp_opacity: f32, time_scale: f32| {
        let graph = RenderGraphDef {
            output_node: NodeId::from("outer_blur"),
            nodes: vec![
                GraphNodeDef {
                    id: NodeId::from("plate"),
                    kind: NodeKind::SourceVideo {
                        asset_id: "clip_a".into(),
                        clip_id: None,
                        color_space: ColorSpace::DisplaySrgb,
                        fit: SourceFit::Stretch,
                    },
                    inputs: vec![],
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("tint"),
                    kind: NodeKind::Solid {
                        color_rgba: [0.15, 0.05, 0.25, 1.0],
                        color_space: ColorSpace::LinearRec709,
                    },
                    inputs: vec![],
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("precomp"),
                    kind: NodeKind::Precomp {
                        sub_graph: sub_graph.clone(),
                        time_offset_ticks: 0,
                        time_scale,
                        opacity: precomp_opacity,
                    },
                    inputs: vec![NodeId::from("plate"), NodeId::from("tint")],
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("outer_blur"),
                    kind: NodeKind::MotionBlurShutter {
                        shutter_angle_deg: 270.0,
                        sub_samples,
                        phase: ShutterPhase::Centered,
                        tonemap: false,
                    },
                    inputs: vec![NodeId::from("precomp")],
                    enabled: true,
                },
            ],
        };
        make_test_project(
            "Precomp MotionBlur Test",
            vec![AssetDef {
                id: "clip_a".into(),
                kind: AssetKind::Video,
                path: "synthetic://clip_a".into(),
                duration_ticks: Some(Flicks(4 * FLICKS_PER_SECOND)),
                fps: Some(FPS_60),
                sample_rate_hz: None,
                resolution: Some(Resolution {
                    width: 64,
                    height: 64,
                }),
            }],
            graph,
            vec![ParamBindingDef {
                node_id: NodeId::from("sub_xform"),
                param_name: "translate_x".into(),
                expression: "t_sec * 0.45".into(),
            }],
            64,
            64,
        )
    };

    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let provider = FnFrameProvider(|_: &str, _: Flicks, w: u32, h: u32| Ok(checkerboard_frame(w, h)));

    let t_a = Flicks(FLICKS_PER_SECOND / 2);
    let t_b = Flicks(FLICKS_PER_SECOND + FLICKS_PER_SECOND / 3);

    let proj_blur8 = build_project(8, 1.0, 1.5);
    let proj_blur1 = build_project(1, 1.0, 1.5);

    let f_a1 = compositor
        .render_project_frame(
            &proj_blur8,
            t_a,
            &proj_blur8.render_profiles.draft,
            &provider,
            &test_opts(),
        )
        .expect("render t_a first");
    let f_b = compositor
        .render_project_frame(
            &proj_blur8,
            t_b,
            &proj_blur8.render_profiles.draft,
            &provider,
            &test_opts(),
        )
        .expect("render t_b");
    let f_a2 = compositor
        .render_project_frame(
            &proj_blur8,
            t_a,
            &proj_blur8.render_profiles.draft,
            &provider,
            &test_opts(),
        )
        .expect("seek back to t_a");

    assert_eq!(
        f_a1.fnv1a_hash, f_a2.fnv1a_hash,
        "Precomp + MotionBlurShutter must be strictly seek-independent"
    );
    assert_ne!(
        f_a1.fnv1a_hash, f_b.fnv1a_hash,
        "remapped sub_graph animation must advance across time"
    );

    let f_sharp = compositor
        .render_project_frame(
            &proj_blur1,
            t_a,
            &proj_blur1.render_profiles.draft,
            &provider,
            &test_opts(),
        )
        .expect("sub_samples=1 render");
    assert_ne!(
        f_a1.fnv1a_hash, f_sharp.fnv1a_hash,
        "outer MotionBlurShutter (sub_samples=8 vs 1) must blur animated Precomp contents"
    );

    // Precomp opacity = 0.0 -> transparent black without evaluating sub_graph
    let proj_zero = build_project(1, 0.0, 1.0);
    let f_zero = compositor
        .render_project_frame(
            &proj_zero,
            t_a,
            &proj_zero.render_profiles.draft,
            &provider,
            &test_opts(),
        )
        .expect("precomp opacity 0");
    assert!(
        f_zero
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .all(|&px| px == [0, 0, 0, 0]),
        "Precomp with opacity <= 0.001 must clear to transparent black"
    );

    // Precomp opacity = 0.5 -> scaled premultiplied output
    let proj_half = build_project(1, 0.5, 1.5);
    let f_half = compositor
        .render_project_frame(
            &proj_half,
            t_a,
            &proj_half.render_profiles.draft,
            &provider,
            &test_opts(),
        )
        .expect("precomp opacity 0.5");
    let center_idx = (32 * 64 + 32) * 4;
    assert!(
        (f_half.rgba8[center_idx + 3] as i16 - 128).abs() <= 2,
        "expected ~50% alpha at center for Precomp opacity=0.5, got {}",
        f_half.rgba8[center_idx + 3]
    );
    assert!(
        (f_half.rgba8[3] as i16 * 2 - f_sharp.rgba8[3] as i16).abs() <= 2,
        "expected edge alpha to scale by 0.5: half={}, full={}",
        f_half.rgba8[3],
        f_sharp.rgba8[3]
    );

    let proj_scale_one = build_project(1, 1.0, 1.0);
    let f_scale_one = compositor
        .render_project_frame(
            &proj_scale_one,
            t_a,
            &proj_scale_one.render_profiles.draft,
            &provider,
            &test_opts(),
        )
        .expect("precomp time_scale=1.0");
    assert_ne!(
        f_sharp.fnv1a_hash, f_scale_one.fnv1a_hash,
        "Precomp time_scale=1.5 vs 1.0 must produce distinct remapped output"
    );

    // Exceeding MAX_PRECOMP_DEPTH (9 nested precomps) must fail with PrecompDepthExceeded
    let mut deep_graph = RenderGraphDef {
        output_node: NodeId::from("leaf"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("leaf"),
            kind: NodeKind::Solid {
                color_rgba: [1.0, 0.0, 0.0, 1.0],
                color_space: ColorSpace::LinearRec709,
            },
            inputs: vec![],
            enabled: true,
        }],
    };
    for level in 0..=MAX_PRECOMP_DEPTH {
        deep_graph = RenderGraphDef {
            output_node: NodeId::new(format!("p_{level}")),
            nodes: vec![GraphNodeDef {
                id: NodeId::new(format!("p_{level}")),
                kind: NodeKind::Precomp {
                    sub_graph: deep_graph,
                    time_offset_ticks: 0,
                    time_scale: 1.0,
                    opacity: 1.0,
                },
                inputs: vec![],
                enabled: true,
            }],
        };
    }
    let err = compositor
        .render_frame(
            &deep_graph,
            Flicks::ZERO,
            FPS_30.frame_duration().unwrap(),
            16,
            16,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .unwrap_err();
    assert!(
        matches!(err, RenderError::Schema(SchemaError::PrecompDepthExceeded { .. })),
        "expected PrecompDepthExceeded, got {err:?}"
    );
}

#[test]
fn vector_path_hud_renders_bezier_dash_trim_data_value_and_animates() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let frame_ticks = FPS_60.frame_duration().unwrap();
    let fallback_provider = FnFrameProvider(|asset_id: &str, tick: Flicks, _: u32, _: u32| {
        Err(RenderError::FrameProvider {
            asset_id: asset_id.to_owned(),
            source_tick: tick.0,
            message: "no external frame".into(),
        })
    });

    let make_hud_graph = |trim_end: f32, data_value: f32| RenderGraphDef {
        output_node: NodeId::from("hud"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("hud"),
            kind: NodeKind::VectorPathHud {
                points_norm: vec![],
                bezier_segments: vec![CubicBezierSegmentDef {
                    p0: [0.1, 0.8],
                    c1: [0.3, 0.15],
                    c2: [0.7, 0.85],
                    p1: [0.9, 0.2],
                }],
                closed: false,
                stroke_width_norm: 0.015,
                stroke_rgba: [0.1, 0.95, 1.0, 1.0],
                fill_rgba: [0.0, 0.0, 0.0, 0.0],
                dash_len_norm: 0.04,
                gap_len_norm: 0.02,
                dash_offset_norm: 0.0,
                trim_start: 0.0,
                trim_end,
                node_dot_radius_norm: 0.025,
                data_value,
                data_label: "SPD ".into(),
                opacity: 1.0,
            },
            inputs: vec![],
            enabled: true,
        }],
    };

    let full = compositor
        .render_frame(
            &make_hud_graph(1.0, 99.5),
            Flicks::ZERO,
            frame_ticks,
            160,
            96,
            &fallback_provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("full VectorPathHud render");
    let partial = compositor
        .render_frame(
            &make_hud_graph(0.35, 12.0),
            Flicks::ZERO,
            frame_ticks,
            160,
            96,
            &fallback_provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("trimmed VectorPathHud render");

    let count_visible = |rgba: &[u8]| {
        rgba.as_chunks::<4>()
            .0
            .iter()
            .filter(|px| px[3] > 32)
            .count()
    };
    let full_px = count_visible(&full.rgba8);
    let partial_px = count_visible(&partial.rgba8);
    assert!(
        full_px > 100 && partial_px > 20 && full_px > partial_px,
        "expected full trim to have more visible HUD pixels than 0.35 trim (full={full_px}, partial={partial_px})"
    );
    assert_ne!(full.fnv1a_hash, partial.fnv1a_hash);

    // SafeExpr animation on VectorPathHud trim_end + data_value
    let proj = make_test_project(
        "VectorPathHud Anim",
        vec![],
        make_hud_graph(1.0, 0.0),
        vec![
            ParamBindingDef {
                node_id: NodeId::from("hud"),
                param_name: "trim_end".into(),
                expression: "lerp(0.25, 1.0, progress)".into(),
            },
            ParamBindingDef {
                node_id: NodeId::from("hud"),
                param_name: "data_value".into(),
                expression: "progress * 100.0".into(),
            },
        ],
        160,
        96,
    );
    let anim_early = compositor
        .render_project_frame(
            &proj,
            Flicks(FLICKS_PER_SECOND / 4),
            &proj.render_profiles.draft,
            &fallback_provider,
            &test_opts(),
        )
        .expect("early HUD anim");
    let anim_late = compositor
        .render_project_frame(
            &proj,
            Flicks((3 * FLICKS_PER_SECOND) / 2),
            &proj.render_profiles.draft,
            &fallback_provider,
            &test_opts(),
        )
        .expect("late HUD anim");
    assert_ne!(anim_early.fnv1a_hash, anim_late.fnv1a_hash);

    // Provider MissingAsset (as returned by ProjectFrameProvider) also falls back cleanly
    let missing_asset_provider = FnFrameProvider(|asset_id: &str, _: Flicks, _: u32, _: u32| {
        Err(RenderError::Schema(SchemaError::MissingAsset(
            asset_id.to_owned(),
        )))
    });
    let via_missing_asset = compositor
        .render_frame(
            &make_hud_graph(1.0, 99.5),
            Flicks::ZERO,
            frame_ticks,
            160,
            96,
            &missing_asset_provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("MissingAsset fallback on vector_path_hud://");
    assert_eq!(via_missing_asset.fnv1a_hash, full.fnv1a_hash);

    // Explicit provider override works
    let override_provider = FnFrameProvider(|asset_id: &str, _: Flicks, w: u32, h: u32| {
        assert_eq!(asset_id, "vector_path_hud://hud");
        Ok(SourceFrame::rgba_f32_premul(
            w,
            h,
            vec![[0.9, 0.1, 0.2, 1.0]; (w * h) as usize],
        ))
    });
    let overridden = compositor
        .render_frame(
            &make_hud_graph(1.0, 99.5),
            Flicks::ZERO,
            frame_ticks,
            32,
            32,
            &override_provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("override vector_path_hud://");
    assert!(overridden.rgba8[0] > 220 && overridden.rgba8[1] < 100);
}

#[test]
fn corner_pin_perspective_and_degenerate_quad_guard_and_safe_expr() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let frame_ticks = FPS_60.frame_duration().unwrap();
    let provider = FnFrameProvider(|_: &str, _: Flicks, w: u32, h: u32| {
        Ok(SourceFrame::rgba8_straight(
            w,
            h,
            [240u8, 120, 40, 255].repeat((w * h) as usize),
        ))
    });

    let make_cp_graph =
        |tl: [f32; 2], tr: [f32; 2], br: [f32; 2], bl: [f32; 2], filter: SampleFilter| {
            RenderGraphDef {
                output_node: NodeId::from("cp"),
                nodes: vec![
                    GraphNodeDef {
                        id: NodeId::from("src"),
                        kind: NodeKind::SourceVideo {
                            asset_id: "clip_a".into(),
                            clip_id: None,
                            color_space: ColorSpace::DisplaySrgb,
                            fit: SourceFit::Stretch,
                        },
                        inputs: vec![],
                        enabled: true,
                    },
                    GraphNodeDef {
                        id: NodeId::from("cp"),
                        kind: NodeKind::CornerPin {
                            tl,
                            tr,
                            br,
                            bl,
                            filter,
                            opacity: 1.0,
                        },
                        inputs: vec![NodeId::from("src")],
                        enabled: true,
                    },
                ],
            }
        };

    // Perspective trapezoid inside [0.2..0.8]
    let persp_graph = make_cp_graph(
        [0.25, 0.2],
        [0.75, 0.25],
        [0.85, 0.85],
        [0.15, 0.8],
        SampleFilter::CatmullRom,
    );
    let persp = compositor
        .render_frame(
            &persp_graph,
            Flicks::ZERO,
            frame_ticks,
            64,
            64,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("perspective corner pin");
    // Top-left corner (0, 0) is outside the trapezoid -> transparent
    assert_eq!(&persp.rgba8[0..4], &[0, 0, 0, 0]);
    // Center pixel (32, 32) is inside the trapezoid -> opaque source color
    let center_idx = (32 * 64 + 32) * 4;
    assert_eq!(persp.rgba8[center_idx + 3], 255);
    assert!(persp.rgba8[center_idx] > 220);

    // Degenerate collinear quad must NOT panic and must output transparent pixels
    let degen_graph = make_cp_graph(
        [0.0, 0.5],
        [0.3, 0.5],
        [0.7, 0.5],
        [1.0, 0.5],
        SampleFilter::Bilinear,
    );
    let degen = compositor
        .render_frame(
            &degen_graph,
            Flicks::ZERO,
            frame_ticks,
            32,
            32,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("degenerate corner pin should safely clear");
    assert!(
        degen
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .all(|&px| px == [0, 0, 0, 0])
    );

    // SafeExpr animation on CornerPin parameters
    let proj = make_test_project(
        "CornerPin Anim",
        vec![],
        persp_graph,
        vec![
            ParamBindingDef {
                node_id: NodeId::from("cp"),
                param_name: "tr_x".into(),
                expression: "lerp(0.55, 0.9, progress)".into(),
            },
            ParamBindingDef {
                node_id: NodeId::from("cp"),
                param_name: "opacity".into(),
                expression: "0.75".into(),
            },
        ],
        64,
        64,
    );
    let anim_0 = compositor
        .render_project_frame(
            &proj,
            Flicks::ZERO,
            &proj.render_profiles.draft,
            &provider,
            &test_opts(),
        )
        .expect("cp anim t=0");
    let anim_1 = compositor
        .render_project_frame(
            &proj,
            Flicks(FLICKS_PER_SECOND),
            &proj.render_profiles.draft,
            &provider,
            &test_opts(),
        )
        .expect("cp anim t=1");
    assert_ne!(anim_0.fnv1a_hash, anim_1.fnv1a_hash);
}

#[test]
fn grid_warp_control_mesh_and_wave_displace_and_animate() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let frame_ticks = FPS_60.frame_duration().unwrap();
    let provider = FnFrameProvider(|_: &str, _: Flicks, w: u32, h: u32| Ok(checkerboard_frame(w, h)));

    let make_gw_graph =
        |offsets: Vec<[f32; 2]>, wave_amp: [f32; 2], mix_amount: f32, filter: SampleFilter| {
            RenderGraphDef {
                output_node: NodeId::from("gw"),
                nodes: vec![
                    GraphNodeDef {
                        id: NodeId::from("src"),
                        kind: NodeKind::SourceVideo {
                            asset_id: "clip_a".into(),
                            clip_id: None,
                            color_space: ColorSpace::DisplaySrgb,
                            fit: SourceFit::Stretch,
                        },
                        inputs: vec![],
                        enabled: true,
                    },
                    GraphNodeDef {
                        id: NodeId::from("gw"),
                        kind: NodeKind::GridWarp {
                            cols: 3,
                            rows: 3,
                            control_offsets: offsets,
                            wave_amp,
                            wave_freq: [2.0, 2.0],
                            wave_phase: 0.4,
                            mix_amount,
                            filter,
                            opacity: 1.0,
                        },
                        inputs: vec![NodeId::from("src")],
                        enabled: true,
                    },
                ],
            }
        };

    let identity = compositor
        .render_frame(
            &make_gw_graph(vec![[0.0, 0.0]; 9], [0.0, 0.0], 0.0, SampleFilter::Bilinear),
            Flicks::ZERO,
            frame_ticks,
            64,
            64,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("grid warp identity");

    let mut offsets = vec![[0.0, 0.0]; 9];
    offsets[4] = [0.12, -0.09]; // center vertex displacement
    let warped_catmull = compositor
        .render_frame(
            &make_gw_graph(offsets.clone(), [0.04, 0.03], 1.0, SampleFilter::CatmullRom),
            Flicks::ZERO,
            frame_ticks,
            64,
            64,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("grid warp catmull-rom");
    assert_ne!(
        identity.fnv1a_hash, warped_catmull.fnv1a_hash,
        "GridWarp control mesh + wave must measurably deform source image"
    );

    // SafeExpr animation on wave_phase
    let proj = make_test_project(
        "GridWarp Anim",
        vec![],
        make_gw_graph(offsets, [0.05, 0.04], 1.0, SampleFilter::Bilinear),
        vec![ParamBindingDef {
            node_id: NodeId::from("gw"),
            param_name: "wave_phase".into(),
            expression: "t_sec * 4.0".into(),
        }],
        64,
        64,
    );
    let gw_t0 = compositor
        .render_project_frame(
            &proj,
            Flicks::ZERO,
            &proj.render_profiles.draft,
            &provider,
            &test_opts(),
        )
        .expect("grid warp t=0");
    let gw_t1 = compositor
        .render_project_frame(
            &proj,
            Flicks(FLICKS_PER_SECOND / 2),
            &proj.render_profiles.draft,
            &provider,
            &test_opts(),
        )
        .expect("grid warp t=0.5");
    assert_ne!(gw_t0.fnv1a_hash, gw_t1.fnv1a_hash);
}

#[test]
fn vector_text_renders_real_ttf_multiline_alignment_and_propagates_font_not_found() {
    let font_path = find_windows_ttf();
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let frame_ticks = FPS_60.frame_duration().unwrap();

    let fallback_provider = FnFrameProvider(|asset_id: &str, tick: Flicks, _: u32, _: u32| {
        Err(RenderError::FrameProvider {
            asset_id: asset_id.to_owned(),
            source_tick: tick.0,
            message: "no external frame".into(),
        })
    });

    let make_ttf_graph = |fp: Option<String>, ff: Option<String>, align: TextAlign2D| RenderGraphDef {
        output_node: NodeId::from("title"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("title"),
            kind: NodeKind::VectorText {
                text: "FRAME\nSHIFT".into(),
                font_size_norm: 0.22,
                color_rgba: [1.0, 0.95, 0.85, 1.0],
                outline_rgba: [0.1, 0.2, 0.6, 1.0],
                outline_width_norm: 0.015,
                align_center: true,
                y_offset_norm: 0.5,
                font_family: ff,
                font_path: fp,
                font_weight: 400,
                align: Some(align),
                x_offset_norm: None,
                tracking_em: Some(0.04),
                line_height_em: Some(1.25),
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
            inputs: vec![],
            enabled: true,
        }],
    };

    let left = compositor
        .render_frame(
            &make_ttf_graph(Some(font_path.clone()), None, TextAlign2D::Left),
            Flicks::ZERO,
            frame_ticks,
            240,
            120,
            &fallback_provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("real TTF multiline left");
    let right = compositor
        .render_frame(
            &make_ttf_graph(Some(font_path.clone()), None, TextAlign2D::Right),
            Flicks::ZERO,
            frame_ticks,
            240,
            120,
            &fallback_provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("real TTF multiline right");

    let left_visible = left
        .rgba8
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|px| px[3] > 32)
        .count();
    assert!(
        left_visible > 200,
        "real TTF multiline VectorText must produce substantial glyph pixels, got {left_visible}"
    );
    assert_ne!(
        left.fnv1a_hash, right.fnv1a_hash,
        "Left vs Right TTF alignment must shift glyph placement"
    );

    // SafeExpr animation on VectorText parameters (tracking_em, x_offset_norm, opacity)
    let anim_proj = make_test_project(
        "VectorText TTF Anim",
        vec![],
        make_ttf_graph(Some(font_path), None, TextAlign2D::Center),
        vec![
            ParamBindingDef {
                node_id: NodeId::from("title"),
                param_name: "tracking_em".into(),
                expression: "lerp(0.0, 0.25, progress)".into(),
            },
            ParamBindingDef {
                node_id: NodeId::from("title"),
                param_name: "x_offset_norm".into(),
                expression: "lerp(0.35, 0.65, progress)".into(),
            },
        ],
        240,
        120,
    );
    let anim_a = compositor
        .render_project_frame(
            &anim_proj,
            Flicks::ZERO,
            &anim_proj.render_profiles.draft,
            &fallback_provider,
            &test_opts(),
        )
        .expect("VectorText anim t=0");
    let anim_b = compositor
        .render_project_frame(
            &anim_proj,
            Flicks(FLICKS_PER_SECOND),
            &anim_proj.render_profiles.draft,
            &fallback_provider,
            &test_opts(),
        )
        .expect("VectorText anim t=1");
    assert_ne!(anim_a.fnv1a_hash, anim_b.fnv1a_hash);

    // FontNotFound propagation via missing font_path
    let missing_path_graph = make_ttf_graph(
        Some("C:/definitely/missing/font_path_xyz.ttf".into()),
        None,
        TextAlign2D::Center,
    );
    let err_path = compositor
        .render_frame(
            &missing_path_graph,
            Flicks::ZERO,
            frame_ticks,
            128,
            64,
            &fallback_provider,
            &NoopAnimator,
            &test_opts(),
        )
        .unwrap_err();
    assert!(
        matches!(err_path, RenderError::Vector(VectorError::FontNotFound(_))),
        "missing font_path must propagate VectorError::FontNotFound, got {err_path:?}"
    );

    // FontNotFound propagation via nonexistent font_family even under ProjectAnimator
    let missing_family_proj = make_test_project(
        "Missing Family",
        vec![],
        make_ttf_graph(
            None,
            Some("DefinitelyNonExistentFontFamilyXYZ123".into()),
            TextAlign2D::Center,
        ),
        vec![ParamBindingDef {
            node_id: NodeId::from("title"),
            param_name: "opacity".into(),
            expression: "0.9".into(),
        }],
        128,
        64,
    );
    let err_family = compositor
        .render_project_frame(
            &missing_family_proj,
            Flicks::ZERO,
            &missing_family_proj.render_profiles.draft,
            &fallback_provider,
            &test_opts(),
        )
        .unwrap_err();
    assert!(
        matches!(err_family, RenderError::Vector(VectorError::FontNotFound(_))),
        "missing font_family must propagate VectorError::FontNotFound, got {err_family:?}"
    );

    // Unexpected FrameProvider error (e.g., wrapped FontNotFound or I/O failure) must NOT be swallowed
    let wrapped_font_err_provider =
        FnFrameProvider(|asset_id: &str, tick: Flicks, _: u32, _: u32| {
            Err(RenderError::FrameProvider {
                asset_id: asset_id.to_owned(),
                source_tick: tick.0,
                message: "vector error: font file not found: wrapped.ttf".into(),
            })
        });
    let ok_fallback_graph = make_ttf_graph(None, None, TextAlign2D::Center);
    let err_wrapped = compositor
        .render_frame(
            &ok_fallback_graph,
            Flicks::ZERO,
            frame_ticks,
            64,
            64,
            &wrapped_font_err_provider,
            &NoopAnimator,
            &test_opts(),
        )
        .unwrap_err();
    assert!(
        matches!(err_wrapped, RenderError::Vector(VectorError::FontNotFound(_))),
        "wrapped provider FontNotFound must not be swallowed, got {err_wrapped:?}"
    );

    let io_err_provider = FnFrameProvider(|asset_id: &str, tick: Flicks, _: u32, _: u32| {
        Err(RenderError::FrameProvider {
            asset_id: asset_id.to_owned(),
            source_tick: tick.0,
            message: "simulated disk I/O failure".into(),
        })
    });
    let err_io = compositor
        .render_frame(
            &ok_fallback_graph,
            Flicks::ZERO,
            frame_ticks,
            64,
            64,
            &io_err_provider,
            &NoopAnimator,
            &test_opts(),
        )
        .unwrap_err();
    assert!(
        matches!(err_io, RenderError::FrameProvider { .. }),
        "non-override provider error must not be swallowed, got {err_io:?}"
    );
}
