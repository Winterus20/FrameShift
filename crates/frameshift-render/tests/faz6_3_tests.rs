use frameshift_render::{
    FnFrameProvider, GpuCompositor, NodeAnimator, NoopAnimator, OutputAlphaMode, ProjectAnimator,
    RenderError, RenderOptions, RenderedFrame, SourceFrame, TonemapMode,
};
use frameshift_schema::{
    Camera3DDef, Camera3DMode, ColorSpace, GraphNodeDef, NodeId, NodeKind, ParamBindingDef,
    Project, RenderGraphDef, RenderProfileDef, RenderProfileKind, RenderProfilesDef, Resolution,
    Shape3DKind, WorkingPixelFormat,
};
use frameshift_time::{FLICKS_PER_SECOND, FPS_30, Flicks};
use std::collections::HashSet;
use std::path::Path;

fn test_opts() -> RenderOptions {
    RenderOptions {
        tonemap_mode: Some(TonemapMode::None),
        output_alpha_mode: OutputAlphaMode::Straight,
        ..Default::default()
    }
}

fn render_at(
    compositor: &mut GpuCompositor,
    graph: &RenderGraphDef,
    tick: Flicks,
    width: u32,
    height: u32,
) -> Result<RenderedFrame, RenderError> {
    let provider = FnFrameProvider(|_: &str, _: Flicks, w: u32, h: u32| {
        Ok(SourceFrame::rgba8_straight(
            w,
            h,
            vec![255; (w * h * 4) as usize],
        ))
    });
    compositor.render_frame(
        graph,
        tick,
        Flicks(FLICKS_PER_SECOND / 30),
        width,
        height,
        &provider,
        &NoopAnimator,
        &test_opts(),
    )
}

fn count_non_zero_alpha(rgba8: &[u8]) -> usize {
    rgba8.as_chunks::<4>().0.iter().filter(|px| px[3] > 0).count()
}

fn max_rgb_luma(rgba8: &[u8]) -> u8 {
    rgba8
        .as_chunks::<4>()
        .0
        .iter()
        .map(|px| px[0].max(px[1]).max(px[2]))
        .max()
        .unwrap_or(0)
}

fn find_windows_ttf() -> String {
    for candidate in [
        r"C:\Windows\Fonts\arialbd.ttf",
        r"C:\Windows\Fonts\arial.ttf",
        r"C:\Windows\Fonts\impact.ttf",
    ] {
        if Path::new(candidate).exists() {
            return candidate.to_string();
        }
    }
    panic!("expected at least one standard Windows TTF font in C:\\Windows\\Fonts");
}

fn make_shape_node(
    id: &str,
    shape_kind: Shape3DKind,
    pos: [f32; 3],
    color_top: [f32; 4],
    color_bottom: [f32; 4],
) -> GraphNodeDef {
    GraphNodeDef {
        id: NodeId::from(id),
        kind: NodeKind::Shape3D {
            shape_kind,
            size: 0.9,
            aspect: 1.0,
            stroke_width: 0.08,
            corner_radius: 0.08,
            inner_ratio: 0.382,
            extrude_depth: 0.14,
            bevel_size: 0.015,
            material_mode: 2, // Flat2DStroked for crisp occlusion & color assertions
            color_top_rgba: color_top,
            color_bottom_rgba: color_bottom,
            side_color_rgba: color_bottom,
            outline_color_rgba: color_top,
            pos,
            rot_deg: [0.0, 0.0, 0.0],
            scale: [1.0, 1.0, 1.0],
            camera_pos: [0.0, 0.0, 2.5],
            camera_rot_deg: [0.0, 0.0, 0.0],
            fov_deg: 50.0,
            pop_duration_sec: 0.0,
            pop_overshoot: 2.15,
            spin_in_deg: 0.0,
            echo_count: 0,
            echo_offset: [0.0, 0.0, -0.35],
            echo_alpha_decay: 0.55,
            hdr_intensity: 1.0,
            time_offset_sec: 0.0,
            opacity: 1.0,
        },
        inputs: vec![],
        enabled: true,
    }
}

fn extract_patch(rgba8: &[u8], width: u32, x0: u32, y0: u32, pw: u32, ph: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((pw * ph * 4) as usize);
    for y in y0..(y0 + ph) {
        let row_start = ((y * width + x0) * 4) as usize;
        let row_end = row_start + (pw * 4) as usize;
        out.extend_from_slice(&rgba8[row_start..row_end]);
    }
    out
}

#[test]
fn scene3d_shared_depth_occlusion_and_lookat_camera() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let (w, h) = (128, 72);

    let make_occlusion_graph = |order: [&str; 2], cam_roll: f32| {
        let front = make_shape_node(
            "front_cyan",
            Shape3DKind::Diamond3D,
            [0.0, 0.0, 0.45],
            [0.0, 1.0, 1.0, 1.0],
            [0.0, 0.9, 1.0, 1.0],
        );
        let back = make_shape_node(
            "back_magenta",
            Shape3DKind::NeonTile,
            [0.0, 0.0, -0.45],
            [1.0, 0.0, 0.6, 1.0],
            [0.9, 0.0, 0.5, 1.0],
        );
        let solid = GraphNodeDef {
            id: NodeId::from("card_src"),
            kind: NodeKind::Solid {
                color_rgba: [0.2, 0.9, 0.2, 1.0],
                color_space: ColorSpace::LinearRec709,
            },
            inputs: vec![],
            enabled: true,
        };
        let card = GraphNodeDef {
            id: NodeId::from("card_3d"),
            kind: NodeKind::Transform3D {
                pos: [0.0, 0.0, -1.2],
                rot_deg: [0.0, 0.0, 0.0],
                scale: [0.4, 0.4, 1.0],
                fov_deg: 50.0,
                camera_pos: [0.0, 0.0, 5.0], // intentionally different; Scene3D must override
                camera_rot_deg: [0.0, 0.0, 0.0],
                dof_focal_dist: 2.5,
                dof_blur_strength: 0.0,
            },
            inputs: vec![NodeId::from("card_src")],
            enabled: true,
        };

        let mut inputs: Vec<NodeId> = order.iter().map(|s| NodeId::from(*s)).collect();
        inputs.push(NodeId::from("card_3d"));

        RenderGraphDef {
            output_node: NodeId::from("scene"),
            nodes: vec![
                front,
                back,
                solid,
                card,
                GraphNodeDef {
                    id: NodeId::from("scene"),
                    kind: NodeKind::Scene3D {
                        camera: Camera3DDef {
                            pos: [0.25, 0.15, 2.6],
                            rot_deg: [0.0, 0.0, cam_roll],
                            target: [0.0, 0.0, 0.0],
                            mode: Camera3DMode::LookAt,
                            fov_deg: 50.0,
                            ..Default::default()
                        },
                        background_rgba: [0.0, 0.0, 0.0, 0.0],
                    },
                    inputs,
                    enabled: true,
                },
            ],
        }
    };

    let order_a = render_at(
        &mut compositor,
        &make_occlusion_graph(["front_cyan", "back_magenta"], 0.0),
        Flicks::ZERO,
        w,
        h,
    )
    .expect("order A");
    let order_b = render_at(
        &mut compositor,
        &make_occlusion_graph(["back_magenta", "front_cyan"], 0.0),
        Flicks::ZERO,
        w,
        h,
    )
    .expect("order B");
    let rolled = render_at(
        &mut compositor,
        &make_occlusion_graph(["front_cyan", "back_magenta"], 28.0),
        Flicks::ZERO,
        w,
        h,
    )
    .expect("rolled lookat");
    let rolled_repeat = render_at(
        &mut compositor,
        &make_occlusion_graph(["front_cyan", "back_magenta"], 28.0),
        Flicks::ZERO,
        w,
        h,
    )
    .expect("rolled lookat repeat");

    assert_eq!(
        order_a.fnv1a_hash, order_b.fnv1a_hash,
        "Scene3D Depth32Float must occlude identically regardless of child input order"
    );
    assert_eq!(rolled.fnv1a_hash, rolled_repeat.fnv1a_hash);
    assert_ne!(order_a.rgba8, rolled.rgba8, "LookAt roll must rotate image");
    assert!(count_non_zero_alpha(&order_a.rgba8) > 200);
}

#[test]
fn scene3d_z_buffer_bokeh_dof_blurs_out_of_focus_object() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let (w, h) = (160, 90);

    let make_dof_graph = |blur_strength: f32| {
        // Near object on the left (dist ~ 1.6 from cam at z=3.0)
        let near_obj = make_shape_node(
            "near_star",
            Shape3DKind::HollowStar5,
            [-1.0, 0.0, 1.4],
            [1.0, 0.8, 0.1, 1.0],
            [1.0, 0.4, 0.1, 1.0],
        );
        // In-focus object in the center (front face at z=0.0, dist=3.0 == target distance)
        let mut focus_obj = make_shape_node(
            "focus_diamond",
            Shape3DKind::Diamond3D,
            [0.0, 0.0, -0.02],
            [0.1, 1.0, 0.9, 1.0],
            [0.1, 0.7, 1.0, 1.0],
        );
        if let NodeKind::Shape3D {
            extrude_depth,
            bevel_size,
            ..
        } = &mut focus_obj.kind
        {
            *extrude_depth = 0.04;
            *bevel_size = 0.0;
        }
        // Far object on the right (dist ~ 6.2)
        let far_obj = make_shape_node(
            "far_tile",
            Shape3DKind::NeonTile,
            [2.0, 0.0, -3.2],
            [1.0, 0.2, 0.8, 1.0],
            [0.7, 0.1, 0.9, 1.0],
        );

        RenderGraphDef {
            output_node: NodeId::from("scene"),
            nodes: vec![
                near_obj,
                focus_obj,
                far_obj,
                GraphNodeDef {
                    id: NodeId::from("scene"),
                    kind: NodeKind::Scene3D {
                        camera: Camera3DDef {
                            pos: [0.0, 0.0, 3.0],
                            rot_deg: [0.0, 0.0, 0.0],
                            target: [0.0, 0.0, 0.0],
                            mode: Camera3DMode::LookAt,
                            fov_deg: 55.0,
                            near: 0.05,
                            far: 100.0,
                            dof_focal_dist: 1.0, // overridden by autofocus_target
                            dof_autofocus_target: true,
                            dof_blur_strength: blur_strength,
                            dof_max_radius_px: 18.0,
                            dof_samples: 24,
                            dof_highlight_boost: 0.75,
                            ..Default::default()
                        },
                        background_rgba: [0.0, 0.0, 0.0, 0.0],
                    },
                    inputs: vec![
                        NodeId::from("near_star"),
                        NodeId::from("focus_diamond"),
                        NodeId::from("far_tile"),
                    ],
                    enabled: true,
                },
            ],
        }
    };

    let sharp = render_at(&mut compositor, &make_dof_graph(0.0), Flicks::ZERO, w, h)
        .expect("sharp scene3d");
    let bokeh_1 = render_at(&mut compositor, &make_dof_graph(1.2), Flicks::ZERO, w, h)
        .expect("bokeh scene3d 1");
    let bokeh_2 = render_at(&mut compositor, &make_dof_graph(1.2), Flicks::ZERO, w, h)
        .expect("bokeh scene3d 2");

    assert_eq!(bokeh_1.fnv1a_hash, bokeh_2.fnv1a_hash);
    assert_ne!(
        sharp.rgba8, bokeh_1.rgba8,
        "Z-buffer Bokeh DoF must blur out-of-focus objects"
    );

    // The center patch containing the in-focus object at target (z=0) must remain sharp.
    let center_sharp = extract_patch(&sharp.rgba8, w, 72, 37, 16, 16);
    let center_bokeh = extract_patch(&bokeh_1.rgba8, w, 72, 37, 16, 16);
    assert_eq!(
        center_sharp, center_bokeh,
        "In-focus target pixels must remain crisp under DoF"
    );
}

#[test]
fn text3d_kinetics_pop_in_palette_chrome_outline_and_echo() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let (w, h) = (160, 90);
    let font_path = find_windows_ttf();

    let make_text_graph = |material_mode: u32, echo_count: u32, outline_width: f32| RenderGraphDef {
        output_node: NodeId::from("title"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("title"),
            kind: NodeKind::Text3D {
                text: "RIVAL".into(),
                font_path: font_path.clone(),
                font_size: 0.48,
                extrude_depth: 0.14,
                bevel_size: 0.01,
                color_top_rgba: [1.0, 0.95, 1.0, 1.0],
                color_bottom_rgba: [1.0, 0.2, 0.75, 1.0],
                side_color_rgba: [0.25, 0.05, 0.45, 1.0],
                pos: [0.0, 0.0, 0.0],
                rot_deg: [8.0, -14.0, 0.0],
                camera_pos: [0.0, 0.0, 2.5],
                camera_rot_deg: [0.0, 0.0, 0.0],
                fov_deg: 50.0,
                char_stagger_sec: 0.08,
                char_bounce_amp: 0.05,
                char_wave_freq: 4.0,
                time_offset_sec: 0.0,
                outline_width,
                outline_color_rgba: [1.0, 1.0, 1.0, 1.0],
                material_mode,
                color_mode: 1, // palette cycle
                palette_rgba: vec![
                    [1.0, 0.15, 0.4, 1.0],
                    [0.1, 0.95, 1.0, 1.0],
                    [1.0, 0.85, 0.1, 1.0],
                    [0.7, 0.2, 1.0, 1.0],
                ],
                pop_duration_sec: 0.35,
                pop_overshoot: 2.15,
                spin_in_deg: 180.0,
                char_jitter_deg: 6.0,
                arc_curvature: 0.35,
                arc_axis: 0,
                echo_count,
                echo_offset: [0.0, 0.0, -0.28],
                echo_alpha_decay: 0.55,
                echo_scale_step: 0.94,
                hdr_intensity: 1.15,
                opacity: 1.0,
                align: Default::default(),
                line_height_em: None,
                tracking_em: 0.0,
                bevel_profile: Default::default(),
                bevel_segments: 4,
                fallback_fonts: Vec::new(),
                var_wght: None,
                var_wdth: None,
                var_slnt: None,
            },
            inputs: vec![],
            enabled: true,
        }],
    };

    let chrome_echo = make_text_graph(1, 3, 0.03);
    let t0 = render_at(&mut compositor, &chrome_echo, Flicks::ZERO, w, h).expect("pop t=0");
    let t_mid = render_at(
        &mut compositor,
        &chrome_echo,
        Flicks(FLICKS_PER_SECOND / 5),
        w,
        h,
    )
    .expect("pop mid");
    let t_settled = render_at(
        &mut compositor,
        &chrome_echo,
        Flicks(FLICKS_PER_SECOND),
        w,
        h,
    )
    .expect("pop settled");
    let flat_no_echo = render_at(
        &mut compositor,
        &make_text_graph(2, 0, 0.03),
        Flicks(FLICKS_PER_SECOND),
        w,
        h,
    )
    .expect("flat2d no echo");

    assert_eq!(
        count_non_zero_alpha(&t0.rgba8),
        0,
        "At t=0 with pop_duration_sec>0, chars have not popped in yet"
    );
    let mid_px = count_non_zero_alpha(&t_mid.rgba8);
    let settled_px = count_non_zero_alpha(&t_settled.rgba8);
    assert!(mid_px > 20);
    assert!(settled_px > mid_px);
    assert!(max_rgb_luma(&t_settled.rgba8) > 100);
    assert_ne!(t_settled.rgba8, flat_no_echo.rgba8);
}

#[test]
fn shape3d_all_five_primitives_render_non_empty() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let (w, h) = (96, 96);

    let kinds = [
        Shape3DKind::NeonTile,
        Shape3DKind::HollowStar5,
        Shape3DKind::Heart3D,
        Shape3DKind::LightningBolt,
        Shape3DKind::Diamond3D,
    ];

    let mut hashes = HashSet::new();
    for kind in kinds {
        let mut node = make_shape_node(
            "shape",
            kind,
            [0.0, 0.0, 0.0],
            [0.2, 0.95, 1.0, 1.0],
            [1.0, 0.15, 0.75, 1.0],
        );
        if let NodeKind::Shape3D {
            material_mode,
            rot_deg,
            ..
        } = &mut node.kind
        {
            *material_mode = 0; // Neon3D lighting
            *rot_deg = [15.0, -18.0, 5.0];
        }
        let graph = RenderGraphDef {
            output_node: NodeId::from("shape"),
            nodes: vec![node],
        };
        let frame = render_at(&mut compositor, &graph, Flicks::ZERO, w, h)
            .unwrap_or_else(|e| panic!("shape {kind:?} render failed: {e}"));
        assert!(
            count_non_zero_alpha(&frame.rgba8) > 80,
            "{kind:?} rendered too few pixels"
        );
        assert!(
            max_rgb_luma(&frame.rgba8) > 60,
            "{kind:?} rendered too dark"
        );
        hashes.insert(frame.fnv1a_hash);
    }

    assert_eq!(
        hashes.len(),
        5,
        "all 5 Shape3DKind primitives must produce distinct rendered frames"
    );
}

#[test]
fn ribbon_trail3d_parallel_triple_bundle_and_legacy_compat() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let (w, h) = (128, 72);

    let make_ribbon_graph = |ribbon_style: u32| RenderGraphDef {
        output_node: NodeId::from("ribbons"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("ribbons"),
            kind: NodeKind::RibbonTrail3D {
                ribbon_count: 3,
                segments: 48,
                seed: 9,
                width: 0.045,
                speed: 1.0,
                radius: 0.9,
                trim_start: 0.1,
                trim_end: 0.9,
                color_a_rgba: [1.0, 0.2, 0.85, 1.0],
                color_b_rgba: [0.1, 0.95, 1.0, 1.0],
                camera_pos: [0.0, 0.0, 2.5],
                camera_rot_deg: [0.0, 0.0, 0.0],
                fov_deg: 50.0,
                time_offset_sec: 0.0,
                ribbon_style,
                strand_spacing: 0.08,
                curve_bend: 0.5,
                pos: [0.0, 0.0, 0.0],
                rot_deg: [0.0, 0.0, 0.0],
                scale: [1.0, 1.0, 1.0],
                opacity: 1.0,
                core_whiten: 0.0,
                core_intensity: 1.0,
                halo_intensity: 1.0,
                saturation: 1.0,
                min_width_px: 0.0,
                normal_blend: false,
            },
            inputs: vec![],
            enabled: true,
        }],
    };

    let t = Flicks(FLICKS_PER_SECOND / 2);
    let legacy = render_at(&mut compositor, &make_ribbon_graph(0), t, w, h).expect("legacy");
    let bundle_1 = render_at(&mut compositor, &make_ribbon_graph(1), t, w, h).expect("bundle 1");
    let bundle_2 = render_at(&mut compositor, &make_ribbon_graph(1), t, w, h).expect("bundle 2");
    let helix = render_at(&mut compositor, &make_ribbon_graph(2), t, w, h).expect("helix");

    assert_eq!(bundle_1.fnv1a_hash, bundle_2.fnv1a_hash);
    assert!(count_non_zero_alpha(&legacy.rgba8) > 40);
    assert!(count_non_zero_alpha(&bundle_1.rgba8) > 40);
    assert!(count_non_zero_alpha(&helix.rgba8) > 40);
    assert_ne!(legacy.rgba8, bundle_1.rgba8);
    assert_ne!(bundle_1.rgba8, helix.rgba8);
}

#[test]
fn scene3d_safe_expr_easings_drive_camera_and_children() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let (w, h) = (128, 72);

    let mut project = Project::golden_scene_preset();
    project.name = "scene3d_easings_test".into();
    project.timeline.duration_ticks = Flicks(FLICKS_PER_SECOND * 2);
    project.timeline.clips.clear();
    project.timeline.audio_clips.clear();
    project.assets.clear();
    project.events.clear();

    let shape = make_shape_node(
        "hero_heart",
        Shape3DKind::Heart3D,
        [0.0, 0.0, 0.0],
        [1.0, 0.25, 0.65, 1.0],
        [0.8, 0.05, 0.35, 1.0],
    );
    let ribbon = GraphNodeDef {
        id: NodeId::from("bundle"),
        kind: NodeKind::RibbonTrail3D {
            ribbon_count: 3,
            segments: 36,
            seed: 3,
            width: 0.04,
            speed: 1.0,
            radius: 0.9,
            trim_start: 0.0,
            trim_end: 1.0,
            color_a_rgba: [0.1, 0.95, 1.0, 1.0],
            color_b_rgba: [1.0, 0.2, 0.8, 1.0],
            camera_pos: [0.0, 0.0, 2.5],
            camera_rot_deg: [0.0, 0.0, 0.0],
            fov_deg: 50.0,
            time_offset_sec: 0.0,
            ribbon_style: 1,
            strand_spacing: 0.07,
            curve_bend: 0.45,
            pos: [0.0, 0.0, 0.0],
            rot_deg: [0.0, 0.0, 0.0],
            scale: [1.0, 1.0, 1.0],
            opacity: 1.0,
            core_whiten: 0.0,
            core_intensity: 1.0,
            halo_intensity: 1.0,
            saturation: 1.0,
            min_width_px: 0.0,
            normal_blend: false,
        },
        inputs: vec![],
        enabled: true,
    };
    let scene = GraphNodeDef {
        id: NodeId::from("scene"),
        kind: NodeKind::Scene3D {
            camera: Camera3DDef {
                pos: [0.0, 0.0, 2.6],
                rot_deg: [0.0, 0.0, 0.0],
                target: [0.0, 0.0, 0.0],
                mode: Camera3DMode::LookAt,
                fov_deg: 50.0,
                ..Default::default()
            },
            background_rgba: [0.0, 0.0, 0.0, 0.0],
        },
        inputs: vec![NodeId::from("hero_heart"), NodeId::from("bundle")],
        enabled: true,
    };

    project.graph = RenderGraphDef {
        output_node: NodeId::from("scene"),
        nodes: vec![shape, ribbon, scene],
    };
    project.param_bindings = vec![
        ParamBindingDef {
            node_id: NodeId::from("scene"),
            param_name: "cam_x".into(),
            expression: "lerp(-0.6, 0.6, ease_out_expo(progress))".into(),
        },
        ParamBindingDef {
            node_id: NodeId::from("scene"),
            param_name: "cam_roll".into(),
            expression: "25.0 * ease_out_back(progress)".into(),
        },
        ParamBindingDef {
            node_id: NodeId::from("scene"),
            param_name: "fov_deg".into(),
            expression: "45.0 + 15.0 * ease_in_out_expo(progress)".into(),
        },
        ParamBindingDef {
            node_id: NodeId::from("hero_heart"),
            param_name: "scale_x".into(),
            expression: "0.7 + 0.5 * ease_in_expo(progress)".into(),
        },
        ParamBindingDef {
            node_id: NodeId::from("hero_heart"),
            param_name: "opacity".into(),
            expression: "step(0.4, progress)".into(),
        },
        ParamBindingDef {
            node_id: NodeId::from("bundle"),
            param_name: "opacity".into(),
            expression: "clamp(1.0 - exp(-t_sec * 4.0), 0.0, 1.0)".into(),
        },
    ];
    project.render_profiles = RenderProfilesDef {
        draft: RenderProfileDef {
            kind: RenderProfileKind::Draft,
            resolution: Resolution {
                width: w,
                height: h,
            },
            motion_blur_sub_samples: 1,
            shutter_angle_deg: 0.0,
            pixel_format: WorkingPixelFormat::Rgba16FloatPremul,
        },
        final_profile: RenderProfileDef {
            kind: RenderProfileKind::Final,
            resolution: Resolution {
                width: w,
                height: h,
            },
            motion_blur_sub_samples: 1,
            shutter_angle_deg: 0.0,
            pixel_format: WorkingPixelFormat::Rgba16FloatPremul,
        },
    };
    project.timeline.fps = FPS_30;
    project.timeline.resolution = Resolution {
        width: w,
        height: h,
    };
    project.validate().expect("valid project");

    let provider = FnFrameProvider(|_: &str, _: Flicks, width: u32, height: u32| {
        Ok(SourceFrame::rgba8_straight(
            width,
            height,
            vec![0; (width * height * 4) as usize],
        ))
    });
    let profile = project.render_profiles.draft.clone();
    let opts = test_opts();

    // At t=0: hero_heart opacity == 0.0 (skipped), bundle opacity == 0.0 (skipped) -> empty frame!
    let at_zero = compositor
        .render_project_frame(&project, Flicks::ZERO, &profile, &provider, &opts)
        .expect("render t=0");
    assert_eq!(
        count_non_zero_alpha(&at_zero.rgba8),
        0,
        "Zero opacity children must early-skip in Scene3D"
    );

    let at_late = compositor
        .render_project_frame(
            &project,
            Flicks(FLICKS_PER_SECOND * 3 / 2),
            &profile,
            &provider,
            &opts,
        )
        .expect("render t=1.5s");
    assert!(count_non_zero_alpha(&at_late.rgba8) > 120);

    // Verify ProjectAnimator resolves Scene3D camera params accurately.
    let anim = ProjectAnimator::new(&project).expect("animator");
    let eval_scene = anim
        .evaluate_node(
            &NodeId::from("scene"),
            &project.graph.nodes[2].kind,
            Flicks(FLICKS_PER_SECOND * 2),
        )
        .expect("eval scene");
    match eval_scene {
        NodeKind::Scene3D { camera, .. } => {
            assert!((camera.pos[0] - 0.6).abs() < 1e-4);
            assert!((camera.rot_deg[2] - 25.0).abs() < 1e-4);
            assert!((camera.fov_deg - 60.0).abs() < 1e-4);
        }
        other => panic!("expected Scene3D, got {other:?}"),
    }
}
