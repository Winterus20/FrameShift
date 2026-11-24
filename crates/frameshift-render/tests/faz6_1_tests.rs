use frameshift_render::{
    FnFrameProvider, GpuCompositor, NoopAnimator, OutputAlphaMode, RenderError, RenderOptions,
    RenderedFrame, SourceFrame, TonemapMode,
};
use frameshift_schema::{
    CURRENT_SCHEMA_VERSION, ColorSpace, GraphNodeDef, NodeId, NodeKind, ParamBindingDef, Project,
    RenderGraphDef, RenderProfileDef, RenderProfileKind, RenderProfilesDef, Resolution, SourceFit,
    TimelineDef, WorkingPixelFormat,
};
use frameshift_time::{FLICKS_PER_SECOND, FPS_30, Flicks};

fn pattern_a(width: u32, height: u32) -> SourceFrame {
    let mut data = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let idx = ((y * width + x) * 4) as usize;
            let checker = ((x / 8) + (y / 8)) % 2 == 0;
            data[idx] = if checker { 230 } else { 30 };
            data[idx + 1] = ((x * 255) / width.max(1)) as u8;
            data[idx + 2] = 60;
            data[idx + 3] = 255;
        }
    }
    SourceFrame::rgba8_straight(width, height, data)
}

fn pattern_b(width: u32, height: u32) -> SourceFrame {
    let mut data = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let idx = ((y * width + x) * 4) as usize;
            let stripe = (y / 6) % 2 == 0;
            data[idx] = 40;
            data[idx + 1] = if stripe { 220 } else { 70 };
            data[idx + 2] = ((y * 255) / height.max(1)) as u8;
            data[idx + 3] = 255;
        }
    }
    SourceFrame::rgba8_straight(width, height, data)
}

fn anime_ink_plate(width: u32, height: u32) -> SourceFrame {
    let mut data = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let idx = ((y * width + x) * 4) as usize;
            let is_line = (x as i32 - y as i32).abs() <= 2
                || (x > width / 3 && x < width / 3 + 3)
                || (y > height / 2 && y < height / 2 + 3);
            if is_line {
                data[idx] = 15;
                data[idx + 1] = 15;
                data[idx + 2] = 22;
            } else {
                data[idx] = 220;
                data[idx + 1] = 175;
                data[idx + 2] = 150;
            }
            data[idx + 3] = 255;
        }
    }
    SourceFrame::rgba8_straight(width, height, data)
}

fn bright_center_on_black(width: u32, height: u32) -> SourceFrame {
    let mut data = vec![[0.0f32, 0.0, 0.0, 1.0]; (width * height) as usize];
    let cx0 = width / 2 - 4;
    let cx1 = width / 2 + 4;
    let cy0 = height / 2 - 4;
    let cy1 = height / 2 + 4;
    for y in cy0..cy1 {
        for x in cx0..cx1 {
            data[(y * width + x) as usize] = [4.0, 3.0, 2.0, 1.0];
        }
    }
    SourceFrame::rgba_f32_premul(width, height, data)
}

fn test_opts() -> RenderOptions {
    RenderOptions {
        tonemap_mode: Some(TonemapMode::None),
        output_alpha_mode: OutputAlphaMode::Straight,
        ..Default::default()
    }
}

fn render_graph(
    compositor: &mut GpuCompositor,
    graph: &RenderGraphDef,
    width: u32,
    height: u32,
) -> RenderedFrame {
    let provider = FnFrameProvider(|asset_id: &str, _: Flicks, w: u32, h: u32| {
        Ok(match asset_id {
            "a" => pattern_a(w, h),
            "b" => pattern_b(w, h),
            "ink" => anime_ink_plate(w, h),
            "spot" => bright_center_on_black(w, h),
            _ => pattern_a(w, h),
        })
    });
    compositor
        .render_frame(
            graph,
            Flicks::ZERO,
            Flicks(FLICKS_PER_SECOND / 30),
            width,
            height,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("render graph")
}

#[test]
fn six_new_transitions_are_deterministic_and_respect_endpoints() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let (w, h) = (64, 64);

    let solo_a = RenderGraphDef {
        output_node: NodeId::from("a"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("a"),
            kind: NodeKind::SourceVideo {
                asset_id: "a".into(),
                clip_id: None,
                color_space: ColorSpace::LinearRec709,
                fit: SourceFit::Stretch,
            },
            inputs: vec![],
            enabled: true,
        }],
    };
    let solo_b = RenderGraphDef {
        output_node: NodeId::from("b"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("b"),
            kind: NodeKind::SourceVideo {
                asset_id: "b".into(),
                clip_id: None,
                color_space: ColorSpace::LinearRec709,
                fit: SourceFit::Stretch,
            },
            inputs: vec![],
            enabled: true,
        }],
    };

    let pure_a = render_graph(&mut compositor, &solo_a, w, h);
    let pure_b = render_graph(&mut compositor, &solo_b, w, h);

    let shaders = [
        "dreamy_zoom",
        "swirl",
        "linear_blur",
        "burn",
        "ripple",
        "kaleidoscope",
    ];

    for shader in shaders {
        let make_transition = |progress: f32| RenderGraphDef {
            output_node: NodeId::from("tr"),
            nodes: vec![
                solo_a.nodes[0].clone(),
                solo_b.nodes[0].clone(),
                GraphNodeDef {
                    id: NodeId::from("tr"),
                    kind: NodeKind::ShaderTransition {
                        shader_name: shader.into(),
                        progress,
                        direction_rad: 0.4,
                        whip_strength: 0.8,
                        chromatic_aberration: 0.02,
                        impact_flash: 0.35,
                    },
                    inputs: vec![NodeId::from("a"), NodeId::from("b")],
                    enabled: true,
                },
            ],
        };

        let at_start = render_graph(&mut compositor, &make_transition(0.0), w, h);
        assert_eq!(
            at_start.rgba8, pure_a.rgba8,
            "{shader} at progress=0.0 must output pure tex_a"
        );

        let at_end = render_graph(&mut compositor, &make_transition(1.0), w, h);
        assert_eq!(
            at_end.rgba8, pure_b.rgba8,
            "{shader} at progress=1.0 must output pure tex_b"
        );

        let mid_1 = render_graph(&mut compositor, &make_transition(0.5), w, h);
        let mid_2 = render_graph(&mut compositor, &make_transition(0.5), w, h);
        assert_eq!(
            mid_1.fnv1a_hash, mid_2.fnv1a_hash,
            "{shader} mid-frame must be deterministic"
        );
        assert_ne!(
            mid_1.rgba8, pure_a.rgba8,
            "{shader} mid-frame must differ from pure tex_a"
        );
        assert_ne!(
            mid_1.rgba8, pure_b.rgba8,
            "{shader} mid-frame must differ from pure tex_b"
        );
    }
}

#[test]
fn deep_glow_spreads_energy_to_dark_neighbors_and_is_deterministic() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let (w, h) = (64, 64);

    let make_glow_graph = |mip_levels: u32| RenderGraphDef {
        output_node: NodeId::from("glow"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("src"),
                kind: NodeKind::SourceVideo {
                    asset_id: "spot".into(),
                    clip_id: None,
                    color_space: ColorSpace::LinearRec709,
                    fit: SourceFit::Stretch,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("glow"),
                kind: NodeKind::DeepGlow {
                    threshold: 0.5,
                    knee: 0.25,
                    intensity: 1.5,
                    radius: 1.5,
                    tint_rgb: [1.0, 0.9, 0.8],
                    mip_levels,
                    scatter: 0.65,
                    anamorphic_ratio: 0.0,
                    chromatic_shift: 0.0,
                },
                inputs: vec![NodeId::from("src")],
                enabled: true,
            },
        ],
    };

    let g5 = make_glow_graph(5);
    let out1 = render_graph(&mut compositor, &g5, w, h);
    let out2 = render_graph(&mut compositor, &g5, w, h);
    assert_eq!(out1.fnv1a_hash, out2.fnv1a_hash);

    // Pixel (16, 32) is 12px left of the bright center square (where source is pure black).
    let neighbor_idx = ((32 * w + 16) * 4) as usize;
    let r = out1.rgba8[neighbor_idx];
    let g = out1.rgba8[neighbor_idx + 1];
    let b = out1.rgba8[neighbor_idx + 2];
    assert!(
        r > 0 || g > 0 || b > 0,
        "expected DeepGlow energy to reach dark neighbor at (16,32), got [{r},{g},{b}]"
    );

    // Clamping edge mip_levels (1 -> 2, 10 -> 6) must succeed without panic.
    let _ = render_graph(&mut compositor, &make_glow_graph(1), w, h);
    let _ = render_graph(&mut compositor, &make_glow_graph(10), w, h);
}

#[test]
fn anime_sharpen_and_film_optics_are_deterministic_and_modify_output() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let (w, h) = (64, 64);

    let base_graph = RenderGraphDef {
        output_node: NodeId::from("src"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("src"),
            kind: NodeKind::SourceVideo {
                asset_id: "ink".into(),
                clip_id: None,
                color_space: ColorSpace::LinearRec709,
                fit: SourceFit::Stretch,
            },
            inputs: vec![],
            enabled: true,
        }],
    };
    let raw = render_graph(&mut compositor, &base_graph, w, h);

    let sharpen_graph = RenderGraphDef {
        output_node: NodeId::from("sharp"),
        nodes: vec![
            base_graph.nodes[0].clone(),
            GraphNodeDef {
                id: NodeId::from("sharp"),
                kind: NodeKind::AnimeSharpen {
                    line_strength: 0.75,
                    sharpen_amount: 0.9,
                    anti_ringing: 0.8,
                },
                inputs: vec![NodeId::from("src")],
                enabled: true,
            },
        ],
    };
    let s1 = render_graph(&mut compositor, &sharpen_graph, w, h);
    let s2 = render_graph(&mut compositor, &sharpen_graph, w, h);
    assert_eq!(s1.fnv1a_hash, s2.fnv1a_hash);
    assert_ne!(s1.rgba8, raw.rgba8);

    let make_optics = |grain_seed: f32| RenderGraphDef {
        output_node: NodeId::from("optics"),
        nodes: vec![
            base_graph.nodes[0].clone(),
            GraphNodeDef {
                id: NodeId::from("optics"),
                kind: NodeKind::FilmOptics {
                    lens_k1: 0.05,
                    lens_k2: 0.01,
                    chromatic_edge: 0.012,
                    grain_amount: 0.15,
                    grain_seed,
                    halation_amount: 0.25,
                    vignette_amount: 0.35,
                    auto_scale: true,
                    grain_size_px: 1.65,
                    grain_chroma: 0.25,
                    halation_threshold: 0.65,
                },
                inputs: vec![NodeId::from("src")],
                enabled: true,
            },
        ],
    };
    let o1 = render_graph(&mut compositor, &make_optics(1.0), w, h);
    let o2 = render_graph(&mut compositor, &make_optics(1.0), w, h);
    let o_seed2 = render_graph(&mut compositor, &make_optics(7.5), w, h);
    assert_eq!(o1.fnv1a_hash, o2.fnv1a_hash);
    assert_ne!(o1.rgba8, raw.rgba8);
    assert_ne!(o1.fnv1a_hash, o_seed2.fnv1a_hash);
}

#[test]
fn custom_wgsl_renders_valid_shaders_caches_pipelines_and_rejects_invalid_without_panic() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let (w, h) = (32, 32);

    // 0-input generator shader using uv + params[0]
    let mut params = [0.0f32; 16];
    params[0] = 0.8;
    params[1] = 0.25;
    let gen_src = "return vec4<f32>(params[0].x, uv.x * params[0].y, 0.1, 1.0);".to_string();
    let gen_graph = RenderGraphDef {
        output_node: NodeId::from("gen"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("gen"),
            kind: NodeKind::CustomWgsl {
                wgsl_source: gen_src.clone(),
                expected_inputs: 0,
                params,
            },
            inputs: vec![],
            enabled: true,
        }],
    };
    let gen1 = render_graph(&mut compositor, &gen_graph, w, h);
    let cache_after_first = compositor.pipelines().custom_wgsl_cache_len();
    let gen2 = render_graph(&mut compositor, &gen_graph, w, h);
    assert_eq!(gen1.fnv1a_hash, gen2.fnv1a_hash);
    assert_eq!(
        compositor.pipelines().custom_wgsl_cache_len(),
        cache_after_first,
        "repeat render must hit pipeline cache"
    );
    assert!(gen1.rgba8[0] > 180, "expected strong red from p0=0.8");

    // 1-input shader using tex0 + linear_sampler + p0
    let mut one_params = [0.0f32; 16];
    one_params[0] = 0.5;
    let one_graph = RenderGraphDef {
        output_node: NodeId::from("fx"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("a"),
                kind: NodeKind::Solid {
                    color_rgba: [0.8, 0.4, 0.2, 1.0],
                    color_space: ColorSpace::LinearRec709,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("fx"),
                kind: NodeKind::CustomWgsl {
                    wgsl_source: "let c = textureSampleLevel(tex0, linear_sampler, uv, 0.0); return vec4<f32>(c.rgb * params[0].x, c.a);".into(),
                    expected_inputs: 1,
                    params: one_params,
                },
                inputs: vec![NodeId::from("a")],
                enabled: true,
            },
        ],
    };
    let one_out = render_graph(&mut compositor, &one_graph, w, h);
    assert!(
        (155..=185).contains(&one_out.rgba8[0]),
        "expected sRGB(~170) for linear 0.8*0.5=0.4, got {}",
        one_out.rgba8[0]
    );

    // 2-input shader blending tex0 and tex1
    let mut two_params = [0.0f32; 16];
    two_params[0] = 0.5;
    let two_graph = RenderGraphDef {
        output_node: NodeId::from("mix2"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("a"),
                kind: NodeKind::Solid {
                    color_rgba: [1.0, 0.0, 0.0, 1.0],
                    color_space: ColorSpace::LinearRec709,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("b"),
                kind: NodeKind::Solid {
                    color_rgba: [0.0, 0.0, 1.0, 1.0],
                    color_space: ColorSpace::LinearRec709,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("mix2"),
                kind: NodeKind::CustomWgsl {
                    wgsl_source: "let ca = textureSampleLevel(tex0, linear_sampler, uv, 0.0); let cb = textureSampleLevel(tex1, linear_sampler, uv, 0.0); return mix(ca, cb, params[0].x);".into(),
                    expected_inputs: 2,
                    params: two_params,
                },
                inputs: vec![NodeId::from("a"), NodeId::from("b")],
                enabled: true,
            },
        ],
    };
    let two_out = render_graph(&mut compositor, &two_graph, w, h);
    assert!(two_out.rgba8[0] > 100 && two_out.rgba8[2] > 100);

    // Invalid WGSL cases must return Err(RenderError::ShaderCompilation) without panicking.
    let invalid_cases = [
        "loop { break; } return vec4<f32>(1.0);",
        "this is not valid wgsl ;;;",
        "return 123;",
        "let x = 1.0;",
    ];
    for bad_src in invalid_cases {
        let mut bad_comp = GpuCompositor::new_headless().expect("headless init");
        let bad_graph = RenderGraphDef {
            output_node: NodeId::from("bad"),
            nodes: vec![GraphNodeDef {
                id: NodeId::from("bad"),
                kind: NodeKind::CustomWgsl {
                    wgsl_source: bad_src.into(),
                    expected_inputs: 0,
                    params: [0.0; 16],
                },
                inputs: vec![],
                enabled: true,
            }],
        };
        let provider =
            FnFrameProvider(|_: &str, _: Flicks, _: u32, _: u32| unreachable!("no source"));
        let res = bad_comp.render_frame(
            &bad_graph,
            Flicks::ZERO,
            Flicks(FLICKS_PER_SECOND / 30),
            w,
            h,
            &provider,
            &NoopAnimator,
            &test_opts(),
        );
        assert!(
            matches!(res, Err(RenderError::ShaderCompilation { .. })),
            "expected ShaderCompilation error for '{bad_src}', got {res:?}"
        );
    }

    // Direct compile_custom_wgsl check for input_count > 2
    let device = compositor.context().device.clone();
    let mut pipelines =
        frameshift_render::ShaderPipelines::compile_all(&device).expect("compile_all");
    let err = pipelines.compile_custom_wgsl(&device, "return vec4<f32>(1.0);", 3);
    assert!(matches!(err, Err(RenderError::ShaderCompilation { .. })));
}

#[test]
fn custom_wgsl_rejects_while_loops_outside_comments() {
    let compositor = GpuCompositor::new_headless().expect("headless init");
    let device = compositor.context().device.clone();
    let mut pipelines =
        frameshift_render::ShaderPipelines::compile_all(&device).expect("compile_all");

    for bad_while in [
        "while (true) {} return vec4<f32>(1.0);",
        "while false { break; } return vec4<f32>(1.0);",
    ] {
        let res = pipelines.compile_custom_wgsl(&device, bad_while, 0);
        assert!(
            matches!(res, Err(RenderError::ShaderCompilation { .. })),
            "expected ShaderCompilation error for '{bad_while}', got {:?}",
            res.err()
        );
    }

    // Commented `while` and identifiers containing `while` must still be allowed.
    let allowed = pipelines.compile_custom_wgsl(
        &device,
        "// while (true) {}\n/* while (true) { /* nested while */ } */\nlet while_count = 1.0;\nreturn vec4<f32>(while_count, 0.0, 0.0, 1.0);",
        0,
    );
    assert!(
        allowed.is_ok(),
        "commented while should compile: {:?}",
        allowed.err()
    );
}

#[test]
fn project_animator_updates_faz6_1_node_parameters() {
    let project = Project {
        schema_version: CURRENT_SCHEMA_VERSION,
        revision_id: 1,
        name: "faz6.1 anim".into(),
        assets: vec![],
        timeline: TimelineDef {
            fps: FPS_30,
            duration_ticks: Flicks(FLICKS_PER_SECOND),
            resolution: Resolution {
                width: 32,
                height: 32,
            },
            clips: vec![],
            audio_clips: vec![],
            ducking_rules: vec![],
            micro_silences: vec![],
        },
        events: vec![],
        graph: RenderGraphDef {
            output_node: NodeId::from("wgsl"),
            nodes: vec![
                GraphNodeDef {
                    id: NodeId::from("src"),
                    kind: NodeKind::Solid {
                        color_rgba: [1.0, 1.0, 1.0, 1.0],
                        color_space: ColorSpace::LinearRec709,
                    },
                    inputs: vec![],
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("glow"),
                    kind: NodeKind::DeepGlow {
                        threshold: 0.8,
                        knee: 0.5,
                        intensity: 0.5,
                        radius: 1.0,
                        tint_rgb: [1.0, 1.0, 1.0],
                        mip_levels: 4,
                        scatter: 0.65,
                        anamorphic_ratio: 0.0,
                        chromatic_shift: 0.0,
                    },
                    inputs: vec![NodeId::from("src")],
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("sharp"),
                    kind: NodeKind::AnimeSharpen {
                        line_strength: 0.2,
                        sharpen_amount: 0.3,
                        anti_ringing: 0.8,
                    },
                    inputs: vec![NodeId::from("glow")],
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("optics"),
                    kind: NodeKind::FilmOptics {
                        lens_k1: 0.0,
                        lens_k2: 0.0,
                        chromatic_edge: 0.0,
                        grain_amount: 0.0,
                        grain_seed: 1.0,
                        halation_amount: 0.0,
                        vignette_amount: 0.0,
                        auto_scale: true,
                        grain_size_px: 1.65,
                        grain_chroma: 0.25,
                        halation_threshold: 0.65,
                    },
                    inputs: vec![NodeId::from("sharp")],
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("wgsl"),
                    kind: NodeKind::CustomWgsl {
                        wgsl_source: "let c = textureSampleLevel(tex0, linear_sampler, uv, 0.0); return vec4<f32>(c.rgb * params[0].x * params[3].w, c.a);".into(),
                        expected_inputs: 1,
                        params: [0.0; 16],
                    },
                    inputs: vec![NodeId::from("optics")],
                    enabled: true,
                },
            ],
        },
        render_profiles: RenderProfilesDef {
            draft: RenderProfileDef {
                kind: RenderProfileKind::Draft,
                resolution: Resolution {
                    width: 32,
                    height: 32,
                },
                motion_blur_sub_samples: 1,
                shutter_angle_deg: 180.0,
                pixel_format: WorkingPixelFormat::Rgba16FloatPremul,
            },
            final_profile: RenderProfileDef {
                kind: RenderProfileKind::Final,
                resolution: Resolution {
                    width: 32,
                    height: 32,
                },
                motion_blur_sub_samples: 1,
                shutter_angle_deg: 180.0,
                pixel_format: WorkingPixelFormat::Rgba16FloatPremul,
            },
        },
        param_bindings: vec![
            ParamBindingDef {
                node_id: NodeId::from("glow"),
                param_name: "intensity".into(),
                expression: "1.25".into(),
            },
            ParamBindingDef {
                node_id: NodeId::from("sharp"),
                param_name: "sharpen_amount".into(),
                expression: "0.75".into(),
            },
            ParamBindingDef {
                node_id: NodeId::from("optics"),
                param_name: "vignette_amount".into(),
                expression: "0.4".into(),
            },
            ParamBindingDef {
                node_id: NodeId::from("wgsl"),
                param_name: "p0".into(),
                expression: "0.5".into(),
            },
            ParamBindingDef {
                node_id: NodeId::from("wgsl"),
                param_name: "p15".into(),
                expression: "0.8".into(),
            },
        ],
        keyframe_tracks: vec![],
        creative_decisions: vec![],
        preferences: vec![],
        proxy_config: Default::default(),
        hw_accel_policy: Default::default(),
    };
    project.validate().expect("project validates");

    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let provider = FnFrameProvider(|_: &str, _: Flicks, _: u32, _: u32| unreachable!());
    let profile = project.render_profiles.draft.clone();
    let frame = compositor
        .render_project_frame(&project, Flicks::ZERO, &profile, &provider, &test_opts())
        .expect("animated project render");
    assert!(frame.rgba8[0] > 0);
}
