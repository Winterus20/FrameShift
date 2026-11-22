use frameshift_render::{
    AnimeImpactFrameUniform, BlendUniform, ColorBasicUniform, ColorTransform, CubeLut3D,
    FnFrameProvider, FrameProvider, GpuCompositor, HalftoneGlowUniform, Lut3DUniform,
    MaskMatteUniform, MotionBlurUniform, MotionTileShakeUniform, NodeAnimator, NoopAnimator,
    OutputAlphaMode, ProjectAnimator, RadialShockwaveUniform, RenderError, RenderOptions,
    ShutterOverride, SourceFrame, TonemapMode, TransformUniform, WhipTransitionUniform,
    compute_shutter_samples,
};
use frameshift_schema::{
    AssetDef, AssetKind, BlendMode, CURRENT_SCHEMA_VERSION, ColorSpace, GraphNodeDef, MatteMode,
    NodeId, NodeKind, ParamBindingDef, Project, RenderGraphDef, RenderProfileDef,
    RenderProfileKind, RenderProfilesDef, Resolution, SampleFilter, ShutterPhase, SourceFit,
    TimelineDef, WorkingPixelFormat,
};
use frameshift_time::{FLICKS_PER_SECOND, FPS_60, Flicks};

fn build_synthetic_frame(
    asset_id: &str,
    source_tick: Flicks,
    width: u32,
    height: u32,
) -> SourceFrame {
    let mut data = vec![0u8; (width * height * 4) as usize];
    let t_sec = source_tick.0 as f64 / FLICKS_PER_SECOND as f64;
    let phase = ((t_sec * 120.0) as i64).rem_euclid(width as i64) as u32;

    for y in 0..height {
        for x in 0..width {
            let idx = ((y * width + x) * 4) as usize;
            if asset_id.contains('a') {
                // High-contrast vertical stripe + diagonal gradient for clip A
                let bar = if ((x + phase) % 16) < 6 { 240u8 } else { 28u8 };
                let g = ((y * 255) / height.max(1)) as u8;
                data[idx] = bar;
                data[idx + 1] = g;
                data[idx + 2] = 40;
                data[idx + 3] = 255;
            } else {
                // Checker + horizontal gradient for clip B
                let checker = if ((x / 8) + (y / 8)) % 2 == 0 {
                    210u8
                } else {
                    45u8
                };
                let r = ((x * 255) / width.max(1)) as u8;
                data[idx] = r;
                data[idx + 1] = 80;
                data[idx + 2] = checker;
                data[idx + 3] = 255;
            }
        }
    }
    SourceFrame::rgba8_straight(width, height, data)
}

fn build_full_test_dag(sub_samples: u32, phase: ShutterPhase) -> RenderGraphDef {
    RenderGraphDef {
        output_node: NodeId::from("mb_out"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("src_a"),
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
                id: NodeId::from("src_b"),
                kind: NodeKind::SourceVideo {
                    asset_id: "clip_b".into(),
                    clip_id: None,
                    color_space: ColorSpace::DisplaySrgb,
                    fit: SourceFit::Stretch,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("mask_solid"),
                kind: NodeKind::Solid {
                    color_rgba: [1.0, 1.0, 1.0, 1.0],
                    color_space: ColorSpace::LinearRec709,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("mask_xform"),
                kind: NodeKind::Transform2D {
                    translate: [0.05, 0.0],
                    scale: [0.55, 0.75],
                    rotation_rad: 0.15,
                    anchor: [0.5, 0.5],
                    filter: SampleFilter::CatmullRom,
                },
                inputs: vec![NodeId::from("mask_solid")],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("masked_a"),
                kind: NodeKind::MaskMatte {
                    mode: MatteMode::Alpha,
                    invert: false,
                    feather: 0.08,
                    opaque_black_outside: false,
                },
                inputs: vec![NodeId::from("src_a"), NodeId::from("mask_xform")],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("tint"),
                kind: NodeKind::Solid {
                    color_rgba: [0.05, 0.12, 0.22, 0.35],
                    color_space: ColorSpace::LinearRec709,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("blended_b"),
                kind: NodeKind::Blend {
                    mode: BlendMode::Screen,
                    opacity: 0.6,
                },
                inputs: vec![NodeId::from("src_b"), NodeId::from("tint")],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("whip"),
                kind: NodeKind::ShaderTransition {
                    shader_name: "transition_whip_chromatic".into(),
                    progress: 0.45,
                    direction_rad: 0.25,
                    whip_strength: 0.9,
                    chromatic_aberration: 0.8,
                    impact_flash: 0.35,
                },
                inputs: vec![NodeId::from("masked_a"), NodeId::from("blended_b")],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("graded"),
                kind: NodeKind::ColorBasic {
                    exposure: 0.15,
                    contrast: 1.1,
                    saturation: 1.15,
                    flash_amount: 0.2,
                },
                inputs: vec![NodeId::from("whip")],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("mb_out"),
                kind: NodeKind::MotionBlurShutter {
                    shutter_angle_deg: 180.0,
                    sub_samples,
                    phase,
                    tonemap: true,
                },
                inputs: vec![NodeId::from("graded")],
                enabled: true,
            },
        ],
    }
}

struct MovingTransformAnimator {
    center_tick: Flicks,
    frame_ticks: Flicks,
}

impl NodeAnimator for MovingTransformAnimator {
    fn evaluate_node(
        &self,
        node_id: &NodeId,
        base: &NodeKind,
        sub_tick: Flicks,
    ) -> Result<NodeKind, RenderError> {
        let dt_frames = (sub_tick.0 - self.center_tick.0) as f32 / (self.frame_ticks.0 as f32);
        let mut kind = base.clone();
        if node_id.0 == "mask_xform"
            && let NodeKind::Transform2D {
                translate,
                rotation_rad,
                ..
            } = &mut kind
        {
            translate[0] += dt_frames * 0.35;
            *rotation_rad += dt_frames * 0.8;
        } else if node_id.0 == "whip"
            && let NodeKind::ShaderTransition { progress, .. } = &mut kind
        {
            *progress = (*progress + dt_frames * 0.4).clamp(0.0, 1.0);
        }
        Ok(kind)
    }
}

#[test]
fn uniform_struct_sizes_match_wgsl_layouts() {
    assert_eq!(std::mem::size_of::<TransformUniform>(), 64);
    assert_eq!(std::mem::size_of::<MaskMatteUniform>(), 32);
    assert_eq!(std::mem::size_of::<BlendUniform>(), 32);
    assert_eq!(std::mem::size_of::<ColorBasicUniform>(), 48);
    assert_eq!(std::mem::size_of::<WhipTransitionUniform>(), 32);
    assert_eq!(std::mem::size_of::<MotionBlurUniform>(), 32);
    assert_eq!(std::mem::size_of::<Lut3DUniform>(), 48);
    assert_eq!(std::mem::size_of::<MotionTileShakeUniform>(), 48);
    assert_eq!(std::mem::size_of::<AnimeImpactFrameUniform>(), 48);
    assert_eq!(std::mem::size_of::<RadialShockwaveUniform>(), 48);
    assert_eq!(std::mem::size_of::<HalftoneGlowUniform>(), 48);
}

#[test]
fn compiles_all_fifteen_wgsl_pipelines_headless() {
    let compositor = GpuCompositor::new_headless().expect("headless GPU compositor must init");
    let pipelines = compositor.pipelines();
    let _ = (
        &pipelines.common_fullscreen,
        &pipelines.transform,
        &pipelines.mask_matte,
        &pipelines.blend,
        &pipelines.color_basic,
        &pipelines.transition_whip_chromatic,
        &pipelines.transition_cross_zoom,
        &pipelines.transition_directional_warp,
        &pipelines.transition_glitch_displace,
        &pipelines.fx_motion_tile_shake,
        &pipelines.fx_anime_impact_frame,
        &pipelines.fx_radial_shockwave,
        &pipelines.fx_halftone_glow,
        &pipelines.motion_blur_accumulate,
        &pipelines.motion_blur_tonemap_srgb,
        &pipelines.lut3d,
    );
    assert!(!compositor.context().adapter_info.name.is_empty());
}

#[test]
fn full_dag_execution_is_non_blank_and_deterministic() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let graph = build_full_test_dag(4, ShutterPhase::Centered);
    let provider = FnFrameProvider(|asset_id: &str, tick: Flicks, w: u32, h: u32| {
        Ok(build_synthetic_frame(asset_id, tick, w, h))
    });
    let frame_ticks = FPS_60.frame_duration().unwrap();
    let tick = Flicks(FLICKS_PER_SECOND);
    let opts = RenderOptions::default();

    let first = compositor
        .render_frame(
            &graph,
            tick,
            frame_ticks,
            96,
            54,
            &provider,
            &NoopAnimator,
            &opts,
        )
        .expect("first render");
    let second = compositor
        .render_frame(
            &graph,
            tick,
            frame_ticks,
            96,
            54,
            &provider,
            &NoopAnimator,
            &opts,
        )
        .expect("second render");

    assert_eq!(first.width, 96);
    assert_eq!(first.height, 54);
    assert_eq!(first.tick, tick);
    assert_eq!(first.rgba8.len(), 96 * 54 * 4);
    assert_eq!(first.fnv1a_hash, second.fnv1a_hash);
    assert_eq!(first.rgba8, second.rgba8);

    // Ensure non-blank and non-uniform output
    let min_rgb = first
        .rgba8
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|px| &px[..3])
        .copied()
        .min()
        .unwrap();
    let max_rgb = first
        .rgba8
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|px| &px[..3])
        .copied()
        .max()
        .unwrap();
    assert!(
        max_rgb > min_rgb + 20,
        "rendered frame should have dynamic visual content"
    );
    assert!(
        compositor.texture_pool().stats().reuses > 0,
        "TexturePool should reuse Rgba16Float textures across passes"
    );
}

#[test]
fn motion_blur_sub_samples_1_vs_8_and_phase_produce_measurable_differences() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let provider = FnFrameProvider(|asset_id: &str, tick: Flicks, w: u32, h: u32| {
        Ok(build_synthetic_frame(asset_id, tick, w, h))
    });
    let frame_ticks = FPS_60.frame_duration().unwrap();
    let tick = Flicks(FLICKS_PER_SECOND);
    let animator = MovingTransformAnimator {
        center_tick: tick,
        frame_ticks,
    };
    let opts = RenderOptions::default();

    let graph_1 = build_full_test_dag(1, ShutterPhase::Centered);
    let graph_8_centered = build_full_test_dag(8, ShutterPhase::Centered);
    let graph_8_trailing = build_full_test_dag(8, ShutterPhase::Trailing);

    let f1 = compositor
        .render_frame(
            &graph_1,
            tick,
            frame_ticks,
            96,
            54,
            &provider,
            &animator,
            &opts,
        )
        .expect("1-sample render");
    let f8_c = compositor
        .render_frame(
            &graph_8_centered,
            tick,
            frame_ticks,
            96,
            54,
            &provider,
            &animator,
            &opts,
        )
        .expect("8-sample centered render");
    let f8_t = compositor
        .render_frame(
            &graph_8_trailing,
            tick,
            frame_ticks,
            96,
            54,
            &provider,
            &animator,
            &opts,
        )
        .expect("8-sample trailing render");

    assert_ne!(
        f1.fnv1a_hash, f8_c.fnv1a_hash,
        "motion blur sub_samples=8 must differ from sub_samples=1 on moving transform/transition"
    );
    assert_ne!(
        f8_c.fnv1a_hash, f8_t.fnv1a_hash,
        "centered vs trailing shutter phase must differ on moving transform"
    );

    let l1_diff: u64 = f1
        .rgba8
        .iter()
        .zip(f8_c.rgba8.iter())
        .map(|(&a, &b)| a.abs_diff(b) as u64)
        .sum();
    let mean_diff = l1_diff as f64 / f1.rgba8.len() as f64;
    assert!(
        mean_diff > 0.5,
        "expected measurable pixel difference between 1 and 8 sub-samples, got mean_diff={mean_diff}"
    );
}

#[test]
fn golden_scene_preset_renders_with_project_animator_and_f32_source() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let project = Project::golden_scene_preset();
    let provider = FnFrameProvider(|asset_id: &str, tick: Flicks, w: u32, h: u32| {
        if asset_id.starts_with("vector_text://") {
            return Err(RenderError::FrameProvider {
                asset_id: asset_id.to_owned(),
                source_tick: tick.0,
                message: "use built-in vector text rasterizer".into(),
            });
        }
        Ok(build_synthetic_frame(asset_id, tick, w, h))
    });
    let profile = RenderProfileDef {
        kind: RenderProfileKind::Draft,
        resolution: Resolution {
            width: 128,
            height: 72,
        },
        motion_blur_sub_samples: 4,
        shutter_angle_deg: 180.0,
        pixel_format: WorkingPixelFormat::Rgba16FloatPremul,
    };

    let drop_tick = Flicks(2 * FLICKS_PER_SECOND + FLICKS_PER_SECOND / 2);
    let later_tick = Flicks(2 * FLICKS_PER_SECOND + (3 * FLICKS_PER_SECOND) / 4);
    let opts = RenderOptions {
        tonemap_mode: Some(TonemapMode::Reinhard),
        output_alpha_mode: OutputAlphaMode::Straight,
        exposure: 1.0,
        shutter_override: Some(ShutterOverride {
            shutter_angle_deg: 180.0,
            sub_samples: 4,
            phase: Some(ShutterPhase::Centered),
        }),
        ..Default::default()
    };

    let at_drop = compositor
        .render_project_frame(&project, drop_tick, &profile, &provider, &opts)
        .expect("golden scene at drop");
    let at_drop_repeat = compositor
        .render_project_frame(&project, drop_tick, &profile, &provider, &opts)
        .expect("golden scene at drop repeat");
    let after_drop = compositor
        .render_project_frame(&project, later_tick, &profile, &provider, &opts)
        .expect("golden scene after drop");

    assert_eq!(at_drop.rgba8.len(), 128 * 72 * 4);
    assert_eq!(at_drop.fnv1a_hash, at_drop_repeat.fnv1a_hash);
    assert_ne!(at_drop.fnv1a_hash, after_drop.fnv1a_hash);

    // Event-bound / SafeExpr flash at drop_impact should produce higher average luma than after drop
    let mean_luma = |rgba: &[u8]| -> f64 {
        let sum: u64 = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .map(|px| px[0] as u64 + px[1] as u64 + px[2] as u64)
            .sum();
        sum as f64 / (rgba.len() as f64 / 4.0)
    };
    assert!(
        mean_luma(&at_drop.rgba8) > mean_luma(&after_drop.rgba8),
        "event-bound impact flash should brighten frame at drop_impact"
    );

    // Also verify ProjectAnimator evaluates speed curve and RgbaF32 path via evaluate_subframe
    let animator = ProjectAnimator::new(&project).expect("golden bindings");
    let f32_provider = FnFrameProvider(|asset_id: &str, tick: Flicks, w: u32, h: u32| {
        if asset_id.starts_with("vector_text://") {
            return Err(RenderError::FrameProvider {
                asset_id: asset_id.to_owned(),
                source_tick: tick.0,
                message: "use built-in vector text rasterizer".into(),
            });
        }
        let pixels = vec![[0.6_f32, 0.3, 0.1, 1.0]; (w * h) as usize];
        Ok(SourceFrame::rgba_f32_premul(w, h, pixels))
    });
    let frame_ticks = project.timeline.fps.frame_duration().unwrap();
    let tex = compositor
        .evaluate_subframe(
            &project.graph,
            drop_tick,
            frame_ticks,
            64,
            36,
            &f32_provider,
            &animator,
        )
        .expect("evaluate_subframe with RgbaF32");
    compositor.texture_pool_mut().release(tex);
}

#[test]
fn shutter_sample_math_and_error_cases() {
    let frame_ticks = FPS_60.frame_duration().unwrap();
    let tick = Flicks(FLICKS_PER_SECOND);

    let single =
        compute_shutter_samples(tick, frame_ticks, 180.0, 1, ShutterPhase::Centered).unwrap();
    assert_eq!(single, vec![tick]);

    let zero_angle =
        compute_shutter_samples(tick, frame_ticks, 0.0, 8, ShutterPhase::Centered).unwrap();
    assert_eq!(zero_angle, vec![tick]);

    let centered =
        compute_shutter_samples(tick, frame_ticks, 180.0, 4, ShutterPhase::Centered).unwrap();
    assert_eq!(centered.len(), 4);
    assert!(centered[0] < tick && centered[3] > tick);

    let trailing =
        compute_shutter_samples(tick, frame_ticks, 180.0, 4, ShutterPhase::Trailing).unwrap();
    assert_eq!(trailing.len(), 4);
    assert!(trailing.iter().all(|&t| t <= tick));

    assert!(matches!(
        compute_shutter_samples(tick, frame_ticks, 180.0, 0, ShutterPhase::Centered),
        Err(RenderError::InvalidMotionBlurParams(_))
    ));
    assert!(matches!(
        compute_shutter_samples(tick, frame_ticks, -10.0, 4, ShutterPhase::Centered),
        Err(RenderError::InvalidMotionBlurParams(_))
    ));
}

#[test]
fn diamond_dag_evaluates_shared_source_once_per_subframe_and_rejects_unknown_transition() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let calls = AtomicUsize::new(0);
    let provider = FnFrameProvider(|asset_id: &str, tick: Flicks, w: u32, h: u32| {
        calls.fetch_add(1, Ordering::Relaxed);
        Ok(build_synthetic_frame(asset_id, tick, w, h))
    });

    // Diamond graph: shared_src feeds both Transform2D and MaskMatte -> Blend
    let diamond = RenderGraphDef {
        output_node: NodeId::from("final_blend"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("shared_src"),
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
                id: NodeId::from("shifted"),
                kind: NodeKind::Transform2D {
                    translate: [0.1, 0.0],
                    scale: [1.0, 1.0],
                    rotation_rad: 0.0,
                    anchor: [0.5, 0.5],
                    filter: SampleFilter::Bilinear,
                },
                inputs: vec![NodeId::from("shared_src")],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("luma_masked"),
                kind: NodeKind::MaskMatte {
                    mode: MatteMode::Luma,
                    invert: true,
                    feather: 0.0,
                    opaque_black_outside: false,
                },
                inputs: vec![NodeId::from("shifted"), NodeId::from("shared_src")],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("final_blend"),
                kind: NodeKind::Blend {
                    mode: BlendMode::Overlay,
                    opacity: 0.8,
                },
                inputs: vec![NodeId::from("shared_src"), NodeId::from("luma_masked")],
                enabled: true,
            },
        ],
    };

    let frame_ticks = FPS_60.frame_duration().unwrap();
    let frame = compositor
        .render_frame(
            &diamond,
            Flicks(FLICKS_PER_SECOND),
            frame_ticks,
            64,
            36,
            &provider,
            &NoopAnimator,
            &RenderOptions::default(),
        )
        .expect("diamond render");
    assert_eq!(frame.rgba8.len(), 64 * 36 * 4);
    assert_eq!(
        calls.load(Ordering::Relaxed),
        1,
        "shared SourceVideo node must be decoded/uploaded only once per subframe"
    );

    let bad_transition = RenderGraphDef {
        output_node: NodeId::from("bad"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("s1"),
                kind: NodeKind::Solid {
                    color_rgba: [1.0, 0.0, 0.0, 1.0],
                    color_space: ColorSpace::LinearRec709,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("bad"),
                kind: NodeKind::ShaderTransition {
                    shader_name: "nonexistent_transition".into(),
                    progress: 0.5,
                    direction_rad: 0.0,
                    whip_strength: 1.0,
                    chromatic_aberration: 0.0,
                    impact_flash: 0.0,
                },
                inputs: vec![NodeId::from("s1"), NodeId::from("s1")],
                enabled: true,
            },
        ],
    };
    let err = compositor
        .render_frame(
            &bad_transition,
            Flicks::ZERO,
            frame_ticks,
            32,
            32,
            &provider,
            &NoopAnimator,
            &RenderOptions::default(),
        )
        .unwrap_err();
    assert!(matches!(err, RenderError::UnsupportedShaderTransition(_)));
}

#[test]
fn cube_lut_parse_and_presets_validate_and_grade() {
    let cube = "\
# Sample Iridas .cube LUT
TITLE \"Cyber Test\"
LUT_3D_SIZE 2
DOMAIN_MIN 0.0 0.0 0.0
DOMAIN_MAX 1.0 1.0 1.0
0.0 0.0 0.0
1.0 0.0 0.0
0.0 1.0 0.0
1.0 1.0 0.0
0.0 0.0 1.0
1.0 0.0 1.0
0.0 1.0 1.0
1.0 1.0 1.0
";
    let parsed = CubeLut3D::parse_cube(cube).expect("valid 2x2x2 .cube parses");
    assert_eq!(parsed.title, "Cyber Test");
    assert_eq!(parsed.size, 2);
    assert_eq!(parsed.data_rgba16f_or_f32.len(), 8);
    assert_eq!(parsed.input_space(), ColorSpace::LinearRec709);
    assert_eq!(parsed.output_space(), ColorSpace::LinearRec709);
    let mid = parsed.apply_rgb([0.25, 0.5, 0.75]);
    assert!((mid[0] - 0.25).abs() < 1e-5);
    assert!((mid[1] - 0.5).abs() < 1e-5);
    assert!((mid[2] - 0.75).abs() < 1e-5);

    // Invalid .cube inputs must return RenderError::InvalidLut3D
    assert!(matches!(
        CubeLut3D::parse_cube("TITLE \"NoSize\"\n0 0 0\n"),
        Err(RenderError::InvalidLut3D(_))
    ));
    assert!(matches!(
        CubeLut3D::parse_cube("LUT_1D_SIZE 16\n"),
        Err(RenderError::InvalidLut3D(_))
    ));
    assert!(matches!(
        CubeLut3D::parse_cube(
            "LUT_3D_SIZE 2\nDOMAIN_MIN 1.0 0.0 0.0\nDOMAIN_MAX 0.5 1.0 1.0\n0 0 0\n"
        ),
        Err(RenderError::InvalidLut3D(_))
    ));

    // GPU Lut3D comparison: identity vs anime_cyber_contrast / warm_sunset / bleach_bypass
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let provider = FnFrameProvider(|asset_id: &str, tick: Flicks, w: u32, h: u32| {
        Ok(build_synthetic_frame(asset_id, tick, w, h))
    });
    let frame_ticks = FPS_60.frame_duration().unwrap();
    let opts = RenderOptions {
        tonemap_mode: Some(TonemapMode::None),
        output_alpha_mode: OutputAlphaMode::Premultiplied,
        ..Default::default()
    };

    let base_graph = RenderGraphDef {
        output_node: NodeId::from("src"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("src"),
            kind: NodeKind::SourceVideo {
                asset_id: "clip_a".into(),
                clip_id: None,
                color_space: ColorSpace::DisplaySrgb,
                fit: SourceFit::Stretch,
            },
            inputs: vec![],
            enabled: true,
        }],
    };
    let lut_graph = |preset: &str| RenderGraphDef {
        output_node: NodeId::from("lut"),
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
                id: NodeId::from("lut"),
                kind: NodeKind::Lut3D {
                    input: NodeId::from("src"),
                    lut_preset: preset.into(),
                    intensity: 1.0,
                    lut_source: None,
                    input_space: None,
                    output_space: None,
                },
                inputs: vec![NodeId::from("src")],
                enabled: true,
            },
        ],
    };

    let base_frame = compositor
        .render_frame(
            &base_graph,
            Flicks::ZERO,
            frame_ticks,
            64,
            36,
            &provider,
            &NoopAnimator,
            &opts,
        )
        .expect("base render");
    let id_frame = compositor
        .render_frame(
            &lut_graph("identity"),
            Flicks::ZERO,
            frame_ticks,
            64,
            36,
            &provider,
            &NoopAnimator,
            &opts,
        )
        .expect("identity LUT render");

    let max_id_diff = base_frame
        .rgba8
        .iter()
        .zip(id_frame.rgba8.iter())
        .map(|(&a, &b)| a.abs_diff(b))
        .max()
        .unwrap_or(0);
    assert!(
        max_id_diff <= 1,
        "identity 3D LUT must preserve pixels within 1 LSB, got max_diff={max_id_diff}"
    );

    for preset in ["anime_cyber_contrast", "warm_sunset", "bleach_bypass"] {
        let graded = compositor
            .render_frame(
                &lut_graph(preset),
                Flicks::ZERO,
                frame_ticks,
                64,
                36,
                &provider,
                &NoopAnimator,
                &opts,
            )
            .expect("preset LUT render");
        assert_ne!(
            graded.fnv1a_hash, id_frame.fnv1a_hash,
            "preset '{preset}' must measurably grade image compared to identity"
        );
    }

    // Verify invalid preset returns RenderError::InvalidLut3D
    let err = compositor
        .render_frame(
            &lut_graph("nonexistent_preset"),
            Flicks::ZERO,
            frame_ticks,
            32,
            32,
            &provider,
            &NoopAnimator,
            &opts,
        )
        .unwrap_err();
    assert!(matches!(err, RenderError::InvalidLut3D(_)));
}

#[test]
fn vector_text_renders_standalone_and_supports_provider_override() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let frame_ticks = FPS_60.frame_duration().unwrap();
    let opts = RenderOptions {
        tonemap_mode: Some(TonemapMode::None),
        output_alpha_mode: OutputAlphaMode::Premultiplied,
        ..Default::default()
    };

    let make_text_graph = |align_center: bool, y_offset_norm: f32| RenderGraphDef {
        output_node: NodeId::from("title"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("title"),
            kind: NodeKind::VectorText {
                text: "SHIFT 02!".into(),
                font_size_norm: 0.24,
                color_rgba: [1.0, 0.9, 0.8, 1.0],
                outline_rgba: [0.1, 0.2, 0.4, 0.9],
                outline_width_norm: 0.02,
                align_center,
                y_offset_norm,
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
            },
            inputs: vec![],
            enabled: true,
        }],
    };

    let no_override_provider = FnFrameProvider(|asset_id: &str, tick: Flicks, _w: u32, _h: u32| {
        Err(RenderError::FrameProvider {
            asset_id: asset_id.to_owned(),
            source_tick: tick.0,
            message: "no external frame".into(),
        })
    });

    let centered = compositor
        .render_frame(
            &make_text_graph(true, 0.5),
            Flicks::ZERO,
            frame_ticks,
            160,
            64,
            &no_override_provider,
            &NoopAnimator,
            &opts,
        )
        .expect("centered vector text");
    let left_aligned = compositor
        .render_frame(
            &make_text_graph(false, 0.5),
            Flicks::ZERO,
            frame_ticks,
            160,
            64,
            &no_override_provider,
            &NoopAnimator,
            &opts,
        )
        .expect("left-aligned vector text");

    let non_empty_pixels = centered
        .rgba8
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|px| px[3] > 16)
        .count();
    assert!(
        non_empty_pixels > 40,
        "built-in VectorText rasterizer should draw visible glyph pixels, got {non_empty_pixels}"
    );
    assert_ne!(
        centered.fnv1a_hash, left_aligned.fnv1a_hash,
        "align_center true vs false must move text position"
    );

    // Verify FrameProvider override via "vector_text://<node_id>"
    let override_provider = FnFrameProvider(|asset_id: &str, _tick: Flicks, w: u32, h: u32| {
        assert_eq!(asset_id, "vector_text://title");
        let pixels = vec![[0.25_f32, 0.75, 0.5, 1.0]; (w * h) as usize];
        Ok(SourceFrame::rgba_f32_premul(w, h, pixels))
    });
    let overridden = compositor
        .render_frame(
            &make_text_graph(true, 0.5),
            Flicks::ZERO,
            frame_ticks,
            64,
            32,
            &override_provider,
            &NoopAnimator,
            &opts,
        )
        .expect("vector text provider override");
    assert_ne!(overridden.fnv1a_hash, centered.fnv1a_hash);
    assert!(
        overridden
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .all(|px| px[1] > px[0])
    );
}

#[test]
fn param_bindings_safe_expr_modulates_node_parameters() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let sec = FLICKS_PER_SECOND;
    let project = Project {
        schema_version: CURRENT_SCHEMA_VERSION,
        revision_id: 2,
        name: "SafeExpr Modulation Test".into(),
        assets: vec![AssetDef {
            id: "asset_a".into(),
            kind: AssetKind::Video,
            path: "synthetic://a".into(),
            duration_ticks: Some(Flicks(2 * sec)),
            fps: Some(FPS_60),
            sample_rate_hz: None,
            resolution: Some(Resolution {
                width: 64,
                height: 64,
            }),
        }],
        timeline: TimelineDef {
            fps: FPS_60,
            duration_ticks: Flicks(2 * sec),
            resolution: Resolution {
                width: 64,
                height: 64,
            },
            clips: vec![],
            audio_clips: vec![],
        },
        events: vec![],
        graph: RenderGraphDef {
            output_node: NodeId::from("lut_out"),
            nodes: vec![
                GraphNodeDef {
                    id: NodeId::from("solid_bg"),
                    kind: NodeKind::Solid {
                        color_rgba: [0.3, 0.4, 0.5, 1.0],
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
                    inputs: vec![NodeId::from("solid_bg")],
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("grade"),
                    kind: NodeKind::ColorBasic {
                        exposure: 0.0,
                        contrast: 1.0,
                        saturation: 1.0,
                        flash_amount: 0.0,
                    },
                    inputs: vec![NodeId::from("xform")],
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("lut_out"),
                    kind: NodeKind::Lut3D {
                        input: NodeId::from("grade"),
                        lut_preset: "anime_cyber_contrast".into(),
                        intensity: 0.0,
                        lut_source: None,
                        input_space: None,
                        output_space: None,
                    },
                    inputs: vec![NodeId::from("grade")],
                    enabled: true,
                },
            ],
        },
        render_profiles: RenderProfilesDef {
            draft: RenderProfileDef {
                kind: RenderProfileKind::Draft,
                resolution: Resolution {
                    width: 64,
                    height: 64,
                },
                motion_blur_sub_samples: 1,
                shutter_angle_deg: 0.0,
                pixel_format: WorkingPixelFormat::Rgba16FloatPremul,
            },
            final_profile: RenderProfileDef {
                kind: RenderProfileKind::Final,
                resolution: Resolution {
                    width: 64,
                    height: 64,
                },
                motion_blur_sub_samples: 1,
                shutter_angle_deg: 0.0,
                pixel_format: WorkingPixelFormat::Rgba16FloatPremul,
            },
        },
        param_bindings: vec![
            ParamBindingDef {
                node_id: NodeId::from("xform"),
                param_name: "scale".into(),
                expression: "lerp(0.5, 1.0, progress)".into(),
            },
            ParamBindingDef {
                node_id: NodeId::from("xform"),
                param_name: "rotation_rad".into(),
                expression: "progress * 0.8".into(),
            },
            ParamBindingDef {
                node_id: NodeId::from("grade"),
                param_name: "exposure".into(),
                expression: "t_sec * 0.5".into(),
            },
            ParamBindingDef {
                node_id: NodeId::from("lut_out"),
                param_name: "intensity".into(),
                expression: "clamp(progress, 0.0, 1.0)".into(),
            },
        ],
        creative_decisions: vec![],
        preferences: vec![],
        proxy_config: Default::default(),
        hw_accel_policy: Default::default(),
    };

    let animator = ProjectAnimator::new(&project).expect("modulation bindings");
    let evaluated = animator
        .evaluate_node(
            &NodeId::from("xform"),
            &project.graph.node(&NodeId::from("xform")).unwrap().kind,
            Flicks(sec), // progress = 0.5
        )
        .expect("modulation eval");
    match evaluated {
        NodeKind::Transform2D {
            scale,
            rotation_rad,
            ..
        } => {
            assert!((scale[0] - 0.75).abs() < 1e-5);
            assert!((scale[1] - 0.75).abs() < 1e-5);
            assert!((rotation_rad - 0.4).abs() < 1e-5);
        }
        other => panic!("unexpected node kind: {other:?}"),
    }

    let provider = FnFrameProvider(|asset_id: &str, tick: Flicks, _w: u32, _h: u32| {
        Err(RenderError::FrameProvider {
            asset_id: asset_id.to_owned(),
            source_tick: tick.0,
            message: "unused".into(),
        })
    });
    let opts = RenderOptions::default();
    let f_early = compositor
        .render_project_frame(
            &project,
            Flicks::ZERO,
            &project.render_profiles.draft,
            &provider,
            &opts,
        )
        .expect("early frame");
    let f_late = compositor
        .render_project_frame(
            &project,
            Flicks(sec),
            &project.render_profiles.draft,
            &provider,
            &opts,
        )
        .expect("late frame");
    assert_ne!(
        f_early.fnv1a_hash, f_late.fnv1a_hash,
        "SafeExpr ParamBindingDef modulation must produce distinct frames across time"
    );
}

#[test]
fn all_four_transitions_and_four_anime_postfx_render_distinct_deterministic_frames() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let provider = FnFrameProvider(|asset_id: &str, tick: Flicks, w: u32, h: u32| {
        Ok(build_synthetic_frame(asset_id, tick, w, h))
    });
    let frame_ticks = FPS_60.frame_duration().unwrap();
    let tick = Flicks(FLICKS_PER_SECOND);
    let opts = RenderOptions {
        tonemap_mode: Some(TonemapMode::None),
        output_alpha_mode: OutputAlphaMode::Straight,
        ..Default::default()
    };

    let assert_dynamic_non_blank = |rgba: &[u8], label: &str| {
        let min_rgb = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|px| &px[..3])
            .copied()
            .min()
            .unwrap();
        let max_rgb = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|px| &px[..3])
            .copied()
            .max()
            .unwrap();
        assert!(
            max_rgb > min_rgb + 20,
            "{label} should produce non-blank dynamic pixels (min={min_rgb}, max={max_rgb})"
        );
    };

    // 1. Verify all 4 transitions + aliases
    let transition_variants = [
        ("whip_chromatic", "transition_whip_chromatic.wgsl"),
        ("cross_zoom", "transition_cross_zoom.wgsl"),
        ("directional_warp", "transition_directional_warp.wgsl"),
        ("glitch_displace", "transition_glitch_displace.wgsl"),
    ];
    let mut trans_hashes = Vec::new();
    for (short_name, wgsl_alias) in transition_variants {
        let make_graph = |shader_name: &str| RenderGraphDef {
            output_node: NodeId::from("tr"),
            nodes: vec![
                GraphNodeDef {
                    id: NodeId::from("a"),
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
                    id: NodeId::from("b"),
                    kind: NodeKind::SourceVideo {
                        asset_id: "clip_b".into(),
                        clip_id: None,
                        color_space: ColorSpace::DisplaySrgb,
                        fit: SourceFit::Stretch,
                    },
                    inputs: vec![],
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("tr"),
                    kind: NodeKind::ShaderTransition {
                        shader_name: shader_name.into(),
                        progress: 0.5,
                        direction_rad: 0.35,
                        whip_strength: 1.1,
                        chromatic_aberration: 0.04,
                        impact_flash: 0.25,
                    },
                    inputs: vec![NodeId::from("a"), NodeId::from("b")],
                    enabled: true,
                },
            ],
        };

        let f1 = compositor
            .render_frame(
                &make_graph(short_name),
                tick,
                frame_ticks,
                96,
                54,
                &provider,
                &NoopAnimator,
                &opts,
            )
            .expect("short transition render");
        let f2 = compositor
            .render_frame(
                &make_graph(wgsl_alias),
                tick,
                frame_ticks,
                96,
                54,
                &provider,
                &NoopAnimator,
                &opts,
            )
            .expect("alias transition render");

        assert_eq!(
            f1.fnv1a_hash, f2.fnv1a_hash,
            "alias {wgsl_alias} must match {short_name}"
        );
        assert_dynamic_non_blank(&f1.rgba8, short_name);
        trans_hashes.push(f1.fnv1a_hash);
    }
    for i in 0..trans_hashes.len() {
        for j in (i + 1)..trans_hashes.len() {
            assert_ne!(
                trans_hashes[i], trans_hashes[j],
                "all 4 transition shaders must produce distinct outputs"
            );
        }
    }

    // 2. Verify all 4 anime post-FX nodes individually against base input
    let base_graph = RenderGraphDef {
        output_node: NodeId::from("a"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("a"),
            kind: NodeKind::SourceVideo {
                asset_id: "clip_a".into(),
                clip_id: None,
                color_space: ColorSpace::DisplaySrgb,
                fit: SourceFit::Stretch,
            },
            inputs: vec![],
            enabled: true,
        }],
    };
    let base_frame = compositor
        .render_frame(
            &base_graph,
            tick,
            frame_ticks,
            96,
            54,
            &provider,
            &NoopAnimator,
            &opts,
        )
        .expect("base frame");

    let fx_nodes = [
        (
            "MotionTileShake",
            NodeKind::MotionTileShake {
                tile_scale: 1.25,
                mirror_edges: true,
                shake_amp_norm: [0.05, 0.03],
                shake_rot_rad: 0.12,
                shake_zoom: 0.1,
                shake_freq_hz: 14.0,
                shake_decay: 3.5,
                event_delta_sec: 0.04,
            },
        ),
        (
            "AnimeImpactFrame",
            NodeKind::AnimeImpactFrame {
                luma_threshold: 0.45,
                edge_weight: 1.3,
                invert_amount: 0.85,
                speed_line_density: 0.7,
                speed_line_intensity: 0.8,
                center_uv: [0.5, 0.5],
                mix_amount: 0.95,
            },
        ),
        (
            "RadialShockwave",
            NodeKind::RadialShockwave {
                center_uv: [0.5, 0.5],
                radius: 0.35,
                ring_width: 0.14,
                refraction_amp: 0.08,
                zoom_blur_strength: 0.25,
                chromatic_aberration: 0.04,
            },
        ),
        (
            "HalftoneGlow",
            NodeKind::HalftoneGlow {
                dot_scale: 48.0,
                angle_rad: std::f32::consts::FRAC_PI_4,
                halftone_mix: 0.5,
                glow_threshold: 0.4,
                glow_intensity: 1.1,
                glow_radius_px: 14.0,
                color_mode: frameshift_schema::HalftoneColorMode::Monochrome,
                dot_softness: 0.0,
            },
        ),
    ];

    let mut fx_hashes = Vec::new();
    for (label, fx_kind) in fx_nodes {
        let fx_graph = RenderGraphDef {
            output_node: NodeId::from("fx"),
            nodes: vec![
                GraphNodeDef {
                    id: NodeId::from("a"),
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
                    id: NodeId::from("fx"),
                    kind: fx_kind,
                    inputs: vec![NodeId::from("a")],
                    enabled: true,
                },
            ],
        };
        let f1 = compositor
            .render_frame(
                &fx_graph,
                tick,
                frame_ticks,
                96,
                54,
                &provider,
                &NoopAnimator,
                &opts,
            )
            .expect(label);
        let f2 = compositor
            .render_frame(
                &fx_graph,
                tick,
                frame_ticks,
                96,
                54,
                &provider,
                &NoopAnimator,
                &opts,
            )
            .expect(label);
        assert_eq!(
            f1.fnv1a_hash, f2.fnv1a_hash,
            "{label} must be deterministic"
        );
        assert_ne!(
            f1.fnv1a_hash, base_frame.fnv1a_hash,
            "{label} must transform base frame"
        );
        assert_dynamic_non_blank(&f1.rgba8, label);
        fx_hashes.push(f1.fnv1a_hash);
    }
    for i in 0..fx_hashes.len() {
        for j in (i + 1)..fx_hashes.len() {
            assert_ne!(
                fx_hashes[i], fx_hashes[j],
                "all 4 anime post-FX nodes must produce distinct outputs"
            );
        }
    }
}

#[test]
fn anime_fx_showcase_preset_renders_end_to_end() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let project = Project::anime_fx_showcase_preset();
    let provider = FnFrameProvider(|asset_id: &str, tick: Flicks, w: u32, h: u32| {
        Ok(build_synthetic_frame(asset_id, tick, w, h))
    });
    let profile = RenderProfileDef {
        kind: RenderProfileKind::Draft,
        resolution: Resolution {
            width: 128,
            height: 72,
        },
        motion_blur_sub_samples: 1,
        shutter_angle_deg: 0.0,
        pixel_format: WorkingPixelFormat::Rgba16FloatPremul,
    };
    let opts = RenderOptions {
        tonemap_mode: Some(TonemapMode::AcesLite),
        output_alpha_mode: OutputAlphaMode::Straight,
        ..Default::default()
    };

    let drop_tick = Flicks(2 * FLICKS_PER_SECOND + FLICKS_PER_SECOND / 2);
    let r1 = compositor
        .render_project_frame(&project, drop_tick, &profile, &provider, &opts)
        .expect("anime_fx_showcase_preset render 1");
    let r2 = compositor
        .render_project_frame(&project, drop_tick, &profile, &provider, &opts)
        .expect("anime_fx_showcase_preset render 2");

    assert_eq!(r1.width, 128);
    assert_eq!(r1.height, 72);
    assert_eq!(r1.fnv1a_hash, r2.fnv1a_hash);
    let min_rgb = r1
        .rgba8
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|px| &px[..3])
        .copied()
        .min()
        .unwrap();
    let max_rgb = r1
        .rgba8
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|px| &px[..3])
        .copied()
        .max()
        .unwrap();
    assert!(
        max_rgb > min_rgb + 15,
        "anime_fx_showcase_preset output should be non-blank"
    );
}

fn solid_node(id: &str, rgba: [f32; 4], enabled: bool) -> GraphNodeDef {
    GraphNodeDef {
        id: NodeId::from(id),
        kind: NodeKind::Solid {
            color_rgba: rgba,
            color_space: ColorSpace::LinearRec709,
        },
        inputs: vec![],
        enabled,
    }
}

fn render_rgba(
    compositor: &mut GpuCompositor,
    graph: &RenderGraphDef,
    animator: &dyn NodeAnimator,
    provider: &dyn FrameProvider,
    options: &RenderOptions,
) -> Result<Vec<u8>, RenderError> {
    let frame = compositor.render_frame(
        graph,
        Flicks::ZERO,
        Flicks(FLICKS_PER_SECOND / 24),
        32,
        32,
        provider,
        animator,
        options,
    )?;
    Ok(frame.rgba8)
}

struct ShiftAnimator;

impl NodeAnimator for ShiftAnimator {
    fn evaluate_node(
        &self,
        node_id: &NodeId,
        base: &NodeKind,
        sub_tick: Flicks,
    ) -> Result<NodeKind, RenderError> {
        let mut kind = base.clone();
        if node_id.0 == "xf"
            && let NodeKind::Transform2D { translate, .. } = &mut kind
        {
            translate[0] = (sub_tick.0 as f32) / (FLICKS_PER_SECOND as f32) * 4.0;
        }
        Ok(kind)
    }
}

#[test]
fn disabled_node_passes_input_or_base_and_skips_effect() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let refusing = FnFrameProvider(|_: &str, _: Flicks, _: u32, _: u32| {
        Err(RenderError::FrameProvider {
            asset_id: "unused".into(),
            source_tick: 0,
            message: "disabled generator must not decode".into(),
        })
    });
    let opts = RenderOptions {
        tonemap_mode: Some(TonemapMode::None),
        ..Default::default()
    };

    let plate = [0.2, 0.55, 0.85, 1.0];
    let plate_graph = RenderGraphDef {
        output_node: NodeId::from("src"),
        nodes: vec![solid_node("src", plate, true)],
    };
    let plate_px = render_rgba(
        &mut compositor,
        &plate_graph,
        &NoopAnimator,
        &refusing,
        &opts,
    )
    .expect("solid plate");

    let mut graded = plate_graph.clone();
    graded.output_node = NodeId::from("grade");
    graded.nodes.push(GraphNodeDef {
        id: NodeId::from("grade"),
        kind: NodeKind::ColorBasic {
            exposure: 2.0,
            contrast: 1.4,
            saturation: 0.2,
            flash_amount: 0.85,
        },
        inputs: vec![NodeId::from("src")],
        enabled: false,
    });
    let bypassed = render_rgba(&mut compositor, &graded, &NoopAnimator, &refusing, &opts)
        .expect("disabled grade");
    assert_eq!(bypassed, plate_px, "disabled 1-input node copies its input");
    graded.nodes.last_mut().unwrap().enabled = true;
    let graded_px = render_rgba(&mut compositor, &graded, &NoopAnimator, &refusing, &opts)
        .expect("enabled grade");
    assert_ne!(graded_px, plate_px);

    let base = [0.15, 0.75, 0.2, 1.0];
    let top = [0.9, 0.1, 0.15, 1.0];
    let mut blend = RenderGraphDef {
        output_node: NodeId::from("over"),
        nodes: vec![
            solid_node("base", base, true),
            solid_node("top", top, true),
            GraphNodeDef {
                id: NodeId::from("over"),
                kind: NodeKind::Blend {
                    mode: BlendMode::Normal,
                    opacity: 1.0,
                },
                inputs: vec![NodeId::from("base"), NodeId::from("top")],
                enabled: false,
            },
        ],
    };
    let base_only = RenderGraphDef {
        output_node: NodeId::from("base"),
        nodes: vec![solid_node("base", base, true)],
    };
    let base_px = render_rgba(&mut compositor, &base_only, &NoopAnimator, &refusing, &opts)
        .expect("base solid");
    let blend_off = render_rgba(&mut compositor, &blend, &NoopAnimator, &refusing, &opts)
        .expect("disabled blend");
    assert_eq!(blend_off, base_px, "disabled 2-input node returns its base");
    blend.nodes.last_mut().unwrap().enabled = true;
    let blend_on = render_rgba(&mut compositor, &blend, &NoopAnimator, &refusing, &opts)
        .expect("enabled blend");
    assert_ne!(blend_on, base_px);

    let cleared = RenderGraphDef {
        output_node: NodeId::from("src"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("src"),
            kind: NodeKind::SourceVideo {
                asset_id: "plate".into(),
                clip_id: None,
                color_space: ColorSpace::LinearRec709,
                fit: SourceFit::Stretch,
            },
            inputs: vec![],
            enabled: false,
        }],
    };
    let clear_px = render_rgba(&mut compositor, &cleared, &NoopAnimator, &refusing, &opts)
        .expect("disabled source does not decode");
    assert!(clear_px.iter().all(|b| *b == 0));

    let stripes = FnFrameProvider(|asset_id: &str, tick: Flicks, w: u32, h: u32| {
        Ok(build_synthetic_frame(asset_id, tick, w, h))
    });
    let blur_opts = RenderOptions {
        tonemap_mode: Some(TonemapMode::None),
        shutter_override: Some(ShutterOverride {
            shutter_angle_deg: 180.0,
            sub_samples: 8,
            phase: None,
        }),
        ..Default::default()
    };
    let still = RenderGraphDef {
        output_node: NodeId::from("xf"),
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
                id: NodeId::from("xf"),
                kind: NodeKind::Transform2D {
                    translate: [0.0, 0.0],
                    scale: [1.0, 1.0],
                    rotation_rad: 0.0,
                    anchor: [0.5, 0.5],
                    filter: SampleFilter::Bilinear,
                },
                inputs: vec![NodeId::from("src")],
                enabled: true,
            },
        ],
    };
    let mut blurred = still.clone();
    blurred.output_node = NodeId::from("blur");
    blurred.nodes.push(GraphNodeDef {
        id: NodeId::from("blur"),
        kind: NodeKind::MotionBlurShutter {
            shutter_angle_deg: 180.0,
            sub_samples: 8,
            phase: ShutterPhase::Centered,
            tonemap: false,
        },
        inputs: vec![NodeId::from("xf")],
        enabled: false,
    });
    let still_px = render_rgba(
        &mut compositor,
        &still,
        &ShiftAnimator,
        &stripes,
        &blur_opts,
    )
    .expect("shifted plate");
    let blur_off = render_rgba(
        &mut compositor,
        &blurred,
        &ShiftAnimator,
        &stripes,
        &blur_opts,
    )
    .expect("disabled blur");
    assert_eq!(
        blur_off, still_px,
        "disabled motion blur copies its input and ignores shutter override"
    );
    blurred.nodes.last_mut().unwrap().enabled = true;
    let blur_on = render_rgba(
        &mut compositor,
        &blurred,
        &ShiftAnimator,
        &stripes,
        &blur_opts,
    )
    .expect("enabled blur");
    assert_ne!(blur_on, still_px);
}

fn blur_probe_project(sub_samples: u32, blur_is_output: bool) -> Project {
    let sec = FLICKS_PER_SECOND;
    let mut nodes = vec![
        GraphNodeDef {
            id: NodeId::from("src"),
            kind: NodeKind::SourceVideo {
                asset_id: "asset_a".into(),
                clip_id: None,
                color_space: ColorSpace::DisplaySrgb,
                fit: SourceFit::Stretch,
            },
            inputs: vec![],
            enabled: true,
        },
        GraphNodeDef {
            id: NodeId::from("blur"),
            kind: NodeKind::MotionBlurShutter {
                shutter_angle_deg: 180.0,
                sub_samples,
                phase: ShutterPhase::Centered,
                tonemap: false,
            },
            inputs: vec![NodeId::from("src")],
            enabled: true,
        },
    ];
    let output_node = if blur_is_output {
        NodeId::from("blur")
    } else {
        nodes.push(GraphNodeDef {
            id: NodeId::from("grade"),
            kind: NodeKind::ColorBasic {
                exposure: 0.0,
                contrast: 1.0,
                saturation: 1.0,
                flash_amount: 0.0,
            },
            inputs: vec![NodeId::from("blur")],
            enabled: true,
        });
        NodeId::from("grade")
    };
    Project {
        schema_version: CURRENT_SCHEMA_VERSION,
        revision_id: 1,
        name: "blur probe".into(),
        assets: vec![],
        timeline: TimelineDef {
            fps: FPS_60,
            duration_ticks: Flicks(2 * sec),
            resolution: Resolution {
                width: 64,
                height: 36,
            },
            clips: vec![],
            audio_clips: vec![],
        },
        events: vec![],
        graph: RenderGraphDef { output_node, nodes },
        render_profiles: RenderProfilesDef {
            draft: RenderProfileDef {
                kind: RenderProfileKind::Draft,
                resolution: Resolution {
                    width: 64,
                    height: 36,
                },
                motion_blur_sub_samples: 4,
                shutter_angle_deg: 90.0,
                pixel_format: WorkingPixelFormat::Rgba16FloatPremul,
            },
            final_profile: RenderProfileDef {
                kind: RenderProfileKind::Final,
                resolution: Resolution {
                    width: 64,
                    height: 36,
                },
                motion_blur_sub_samples: 16,
                shutter_angle_deg: 270.0,
                pixel_format: WorkingPixelFormat::Rgba16FloatPremul,
            },
        },
        param_bindings: vec![],
        creative_decisions: vec![],
        preferences: vec![],
        proxy_config: Default::default(),
        hw_accel_policy: Default::default(),
    }
}

fn render_blur_probe(
    compositor: &mut GpuCompositor,
    project: &Project,
    profile: &RenderProfileDef,
    ceiling: Option<(u32, f32)>,
) -> u64 {
    let opts = RenderOptions {
        tonemap_mode: Some(TonemapMode::None),
        output_alpha_mode: OutputAlphaMode::Straight,
        exposure: 1.0,
        shutter_override: ceiling.map(|(sub_samples, shutter_angle_deg)| ShutterOverride {
            shutter_angle_deg,
            sub_samples,
            phase: None,
        }),
        ..Default::default()
    };
    let provider = FnFrameProvider(|asset_id: &str, tick: Flicks, w: u32, h: u32| {
        Ok(build_synthetic_frame(asset_id, tick, w, h))
    });
    compositor
        .render_project_frame(
            project,
            Flicks(FLICKS_PER_SECOND),
            profile,
            &provider,
            &opts,
        )
        .expect("blur probe render")
        .fnv1a_hash
}

#[test]
fn node_sub_samples_survive_draft_profile_unless_ceiling_is_explicit() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let output_blur = blur_probe_project(8, true);
    let draft = &output_blur.render_profiles.draft;
    let final_profile = &output_blur.render_profiles.final_profile;
    let draft_hash = render_blur_probe(&mut compositor, &output_blur, draft, None);
    let final_count_hash = render_blur_probe(&mut compositor, &output_blur, final_profile, None);
    assert_eq!(
        draft_hash, final_count_hash,
        "draft profile 4 and final profile 16 must leave authored sub_samples 8 in place"
    );

    let capped = render_blur_probe(&mut compositor, &output_blur, draft, Some((1, 180.0)));
    assert_ne!(
        capped, draft_hash,
        "explicit ceiling 1 must lower authored sub_samples 8"
    );
    let raised = render_blur_probe(&mut compositor, &output_blur, draft, Some((32, 180.0)));
    assert_eq!(
        raised, draft_hash,
        "a ceiling above the authored count must not raise sub_samples"
    );
    let authored_one = blur_probe_project(1, true);
    let one_hash = render_blur_probe(
        &mut compositor,
        &authored_one,
        &authored_one.render_profiles.final_profile,
        None,
    );
    assert_eq!(capped, one_hash);
    let root_angle_zero = render_blur_probe(&mut compositor, &output_blur, draft, Some((32, 0.0)));
    assert_eq!(
        root_angle_zero, one_hash,
        "an explicit root override still replaces shutter angle on the enabled output blur"
    );

    let inner = blur_probe_project(8, false);
    let inner_draft =
        render_blur_probe(&mut compositor, &inner, &inner.render_profiles.draft, None);
    let inner_final = render_blur_probe(
        &mut compositor,
        &inner,
        &inner.render_profiles.final_profile,
        None,
    );
    assert_eq!(
        inner_draft, inner_final,
        "an inner blur keeps its authored sub_samples when the profile disagrees"
    );
    let inner_capped = render_blur_probe(
        &mut compositor,
        &inner,
        &inner.render_profiles.draft,
        Some((1, 180.0)),
    );
    assert_ne!(inner_capped, inner_draft);
    let inner_one = blur_probe_project(1, false);
    let inner_one_hash = render_blur_probe(
        &mut compositor,
        &inner_one,
        &inner_one.render_profiles.draft,
        None,
    );
    assert_eq!(inner_capped, inner_one_hash);
    let inner_angle_ignored = render_blur_probe(
        &mut compositor,
        &inner,
        &inner.render_profiles.draft,
        Some((32, 0.0)),
    );
    assert_eq!(
        inner_angle_ignored, inner_draft,
        "a root shutter-angle override must not rewrite an inner blur"
    );
}

#[test]
fn bad_expression_stops_frame_evaluation() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let mut project = blur_probe_project(1, false);
    project.param_bindings.push(ParamBindingDef {
        node_id: NodeId::from("grade"),
        param_name: "exposure".into(),
        expression: "1 / (t_sec - t_sec)".into(),
    });
    let err = compositor
        .render_project_frame(
            &project,
            Flicks::ZERO,
            &project.render_profiles.draft,
            &FnFrameProvider(|asset_id: &str, tick: Flicks, w: u32, h: u32| {
                Ok(build_synthetic_frame(asset_id, tick, w, h))
            }),
            &RenderOptions {
                tonemap_mode: Some(TonemapMode::None),
                ..Default::default()
            },
        )
        .expect_err("division by zero must not render the base exposure");
    match err {
        RenderError::Expression {
            node_id,
            param_name,
            message,
        } => {
            assert_eq!(node_id, NodeId::from("grade"));
            assert_eq!(param_name, "exposure");
            assert!(message.contains("division by zero"), "{message}");
        }
        other => panic!("expected Expression, got {other}"),
    }
}
