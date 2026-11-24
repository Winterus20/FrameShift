use frameshift_render::{
    FnFrameProvider, GpuCompositor, NoopAnimator, OutputAlphaMode, ProjectAnimator,
    RenderOptions, SourceFrame, TonemapMode,
};
use frameshift_schema::{
    CURRENT_SCHEMA_VERSION, ColorSpace, GraphNodeDef, NodeId, NodeKind, ParamBindingDef, Project,
    RenderGraphDef, RenderProfileDef, RenderProfileKind, RenderProfilesDef, Resolution, SourceFit,
    TimelineDef, WorkingPixelFormat,
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
    bindings: Vec<ParamBindingDef>,
) -> Project {
    let profile = |kind: RenderProfileKind| RenderProfileDef {
        kind,
        resolution: Resolution { width, height },
        motion_blur_sub_samples: 1,
        shutter_angle_deg: 180.0,
        pixel_format: WorkingPixelFormat::Rgba16FloatPremul,
    };
    Project {
        schema_version: CURRENT_SCHEMA_VERSION,
        revision_id: 1,
        name: "optical_flow_test".into(),
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
        param_bindings: bindings,
        keyframe_tracks: vec![],
        creative_decisions: vec![],
        preferences: vec![],
        proxy_config: Default::default(),
        hw_accel_policy: Default::default(),
    }
}

fn moving_block_graph(visualize_flow: bool, shutter_amount: f32, sensitivity: f32) -> RenderGraphDef {
    RenderGraphDef {
        output_node: NodeId::from("of_blur"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("bg"),
                kind: NodeKind::Solid {
                    color_rgba: [0.02, 0.02, 0.04, 1.0],
                    color_space: ColorSpace::LinearRec709,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("fg_src"),
                kind: NodeKind::Solid {
                    color_rgba: [1.0, 0.85, 0.25, 1.0],
                    color_space: ColorSpace::LinearRec709,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("fg_xform"),
                kind: NodeKind::Transform2D {
                    translate: [0.0, 0.0],
                    scale: [0.28, 0.28],
                    rotation_rad: 0.0,
                    anchor: [0.5, 0.5],
                    filter: Default::default(),
                },
                inputs: vec![NodeId::from("fg_src")],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("comp"),
                kind: NodeKind::Blend {
                    mode: frameshift_schema::BlendMode::Normal,
                    opacity: 1.0,
                },
                inputs: vec![NodeId::from("bg"), NodeId::from("fg_xform")],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("of_blur"),
                kind: NodeKind::OpticalFlowBlur {
                    shutter_amount,
                    samples: 16,
                    max_blur_px: 48.0,
                    sensitivity,
                    flow_smoothness: 0.3,
                    visualize_flow,
                },
                inputs: vec![NodeId::from("comp")],
                enabled: true,
            },
        ],
    }
}

#[test]
fn optical_flow_blur_visualizes_motion_and_smears_moving_edge_without_corrupting_static() {
    let w = 160u32;
    let h = 90u32;
    let frame_ticks = FPS_24.frame_duration().unwrap();
    let eval_tick = Flicks(frame_ticks.0 * 4);
    let mut compositor = GpuCompositor::new_headless().expect("gpu compositor");
    let no_provider = FnFrameProvider(|id: &str, _, _, _| {
        panic!("unexpected source frame request for {id}");
    });
    let opts = test_opts();

    let move_bindings = vec![ParamBindingDef {
        node_id: NodeId::from("fg_xform"),
        param_name: "translate_x".into(),
        expression: "(t_sec - 0.16) * 1.4".into(),
    }];

    // Visualize flow map
    let vis_proj = make_project(w, h, moving_block_graph(true, 1.0, 0.45), move_bindings.clone());
    let vis_anim = ProjectAnimator::new(&vis_proj).unwrap();
    let vis_frame = compositor
        .render_frame(
            &vis_proj.graph,
            eval_tick,
            frame_ticks,
            w,
            h,
            &no_provider,
            &vis_anim,
            &opts,
        )
        .unwrap();
    let non_black_flow_pixels = vis_frame
        .rgba8
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|px| px[0] > 8 || px[1] > 8 || px[2] > 8)
        .count();
    assert!(
        non_black_flow_pixels > 20,
        "expected visible optical flow vectors around moving block, got {non_black_flow_pixels}"
    );

    // Unblurred reference (shutter_amount = 0.0) vs blurred (shutter_amount = 2.0)
    let sharp_proj = make_project(w, h, moving_block_graph(false, 0.0, 0.45), move_bindings.clone());
    let sharp_anim = ProjectAnimator::new(&sharp_proj).unwrap();
    let sharp_frame = compositor
        .render_frame(
            &sharp_proj.graph,
            eval_tick,
            frame_ticks,
            w,
            h,
            &no_provider,
            &sharp_anim,
            &opts,
        )
        .unwrap();

    let blur_proj = make_project(w, h, moving_block_graph(false, 2.0, 0.45), move_bindings);
    let blur_anim = ProjectAnimator::new(&blur_proj).unwrap();
    let blur_frame = compositor
        .render_frame(
            &blur_proj.graph,
            eval_tick,
            frame_ticks,
            w,
            h,
            &no_provider,
            &blur_anim,
            &opts,
        )
        .unwrap();

    assert_ne!(
        sharp_frame.fnv1a_hash, blur_frame.fnv1a_hash,
        "optical flow blur must modify moving scene pixels"
    );
    let midtone_count = |buf: &[u8]| {
        buf.as_chunks::<4>()
            .0
            .iter()
            .filter(|px| px[0] > 55 && px[0] < 225)
            .count()
    };
    let sharp_mid = midtone_count(&sharp_frame.rgba8);
    let blur_mid = midtone_count(&blur_frame.rgba8);
    assert!(
        blur_mid > sharp_mid,
        "expected smeared intermediate edge pixels ({blur_mid} > {sharp_mid})"
    );

    // Static scene: shutter_amount = 2.0 must keep pixels identical to unblurred frame.
    let static_sharp_proj = make_project(w, h, moving_block_graph(false, 0.0, 0.35), vec![]);
    let static_blur_proj = make_project(w, h, moving_block_graph(false, 2.0, 0.35), vec![]);
    let s0 = compositor
        .render_frame(
            &static_sharp_proj.graph,
            eval_tick,
            frame_ticks,
            w,
            h,
            &no_provider,
            &NoopAnimator,
            &opts,
        )
        .unwrap();
    let s1 = compositor
        .render_frame(
            &static_blur_proj.graph,
            eval_tick,
            frame_ticks,
            w,
            h,
            &no_provider,
            &NoopAnimator,
            &opts,
        )
        .unwrap();
    assert_eq!(
        s0.fnv1a_hash, s1.fnv1a_hash,
        "static scene must remain identical under OpticalFlowBlur"
    );
}

#[test]
fn optical_flow_blur_suppresses_false_blur_across_hard_scene_cut() {
    let w = 96u32;
    let h = 64u32;
    let frame_ticks = FPS_24.frame_duration().unwrap();
    let eval_tick = frame_ticks; // frame 1 (after cut from frame 0)
    let mut compositor = GpuCompositor::new_headless().expect("gpu compositor");
    let opts = test_opts();

    let graph_vis = RenderGraphDef {
        output_node: NodeId::from("of_blur"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("src"),
                kind: NodeKind::SourceVideo {
                    asset_id: "cut_clip".into(),
                    clip_id: None,
                    color_space: ColorSpace::LinearRec709,
                    fit: SourceFit::Stretch,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("of_blur"),
                kind: NodeKind::OpticalFlowBlur {
                    shutter_amount: 1.5,
                    samples: 12,
                    max_blur_px: 48.0,
                    sensitivity: 0.25,
                    flow_smoothness: 0.25,
                    visualize_flow: true,
                },
                inputs: vec![NodeId::from("src")],
                enabled: true,
            },
        ],
    };

    let provider = FnFrameProvider(|_id: &str, src_tick: Flicks, pw: u32, ph: u32| {
        let mut rgba = vec![0u8; (pw * ph * 4) as usize];
        let is_cut = src_tick.0 >= frame_ticks.0;
        for y in 0..ph {
            for x in 0..pw {
                let idx = ((y * pw + x) * 4) as usize;
                if is_cut {
                    let v = if ((x / 8) + (y / 8)) % 2 == 0 { 245 } else { 180 };
                    rgba[idx..idx + 4].copy_from_slice(&[v, v, v, 255]);
                } else {
                    rgba[idx..idx + 4].copy_from_slice(&[5, 5, 5, 255]);
                }
            }
        }
        Ok(SourceFrame::rgba8_straight(pw, ph, rgba))
    });

    let proj = make_project(w, h, graph_vis, vec![]);
    let anim = ProjectAnimator::new(&proj).unwrap();
    let vis = compositor
        .render_frame(
            &proj.graph,
            eval_tick,
            frame_ticks,
            w,
            h,
            &provider,
            &anim,
            &opts,
        )
        .unwrap();

    let max_luma = vis
        .rgba8
        .as_chunks::<4>()
        .0
        .iter()
        .map(|px| px[0].max(px[1]).max(px[2]))
        .max()
        .unwrap_or(0);
    assert!(
        max_luma <= 10,
        "scene-cut sensitivity guard should zero confidence across hard cut, got max channel {max_luma}"
    );
}

#[test]
fn scene3d_depth_plate_occludes_3d_objects_behind_foreground() {
    use frameshift_schema::{Camera3DDef, Camera3DMode, SceneDepthPlateDef, Shape3DKind};

    let w = 128u32;
    let h = 128u32;
    let frame_ticks = FPS_24.frame_duration().unwrap();
    let mut compositor = GpuCompositor::new_headless().expect("gpu compositor");
    let opts = test_opts();

    // Left half of depth plate = near foreground (white = 1.0 -> eye_z = 1.0),
    // Right half = far background (black = 0.0 -> eye_z = 5.0).
    let provider = FnFrameProvider(|_id: &str, _tick: Flicks, pw: u32, ph: u32| {
        let mut rgba = vec![0u8; (pw * ph * 4) as usize];
        for y in 0..ph {
            for x in 0..pw {
                let idx = ((y * pw + x) * 4) as usize;
                let v = if x < pw / 2 { 255 } else { 0 };
                rgba[idx..idx + 4].copy_from_slice(&[v, v, v, 255]);
            }
        }
        Ok(SourceFrame::rgba8_straight(pw, ph, rgba))
    });

    let box3d_kind = NodeKind::Shape3D {
        shape_kind: Shape3DKind::NeonTile,
        size: 1.6,
        aspect: 1.0,
        stroke_width: 0.04,
        corner_radius: 0.0,
        inner_ratio: 0.382,
        extrude_depth: 0.05,
        bevel_size: 0.0,
        material_mode: 2,
        color_top_rgba: [1.0, 0.9, 0.2, 1.0],
        color_bottom_rgba: [1.0, 0.5, 0.1, 1.0],
        side_color_rgba: [0.8, 0.4, 0.1, 1.0],
        outline_color_rgba: [1.0, 0.9, 0.2, 1.0],
        pos: [0.0, 0.0, 0.0],
        rot_deg: [0.0, 0.0, 0.0],
        scale: [1.0, 1.0, 1.0],
        camera_pos: [0.0, 0.0, 3.0],
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
    };

    // Camera at z=3.0 looking at origin (0,0,0), so shape at z=0.0 is at eye_z=3.0:
    // Behind left foreground (eye_z=1.0 < 3.0) -> occluded!
    // In front of right background (eye_z=5.0 > 3.0) -> visible!
    let graph = RenderGraphDef {
        output_node: NodeId::from("scene"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("depth_src"),
                kind: NodeKind::SourceVideo {
                    asset_id: "depth_plate".into(),
                    clip_id: None,
                    color_space: ColorSpace::LinearRec709,
                    fit: SourceFit::Stretch,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("box3d"),
                kind: box3d_kind,
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("scene"),
                kind: NodeKind::Scene3D {
                    camera: Camera3DDef {
                        pos: [0.0, 0.0, 3.0],
                        target: [0.0, 0.0, 0.0],
                        mode: Camera3DMode::LookAt,
                        near: 0.1,
                        far: 20.0,
                        ..Default::default()
                    },
                    background_rgba: [0.0, 0.0, 0.0, 0.0],
                    depth_plate: Some(SceneDepthPlateDef {
                        node_id: NodeId::from("depth_src"),
                        near_z: 1.0,
                        far_z: 5.0,
                        invert: false,
                        ..Default::default()
                    }),
                },
                inputs: vec![NodeId::from("box3d")],
                enabled: true,
            },
        ],
    };

    let proj = make_project(w, h, graph, vec![]);
    let anim = ProjectAnimator::new(&proj).unwrap();
    let frame = compositor
        .render_frame(
            &proj.graph,
            Flicks::ZERO,
            frame_ticks,
            w,
            h,
            &provider,
            &anim,
            &opts,
        )
        .unwrap();

    let mut left_visible = 0usize;
    let mut right_visible = 0usize;
    for y in 0..h {
        for x in 0..w {
            let idx = ((y * w + x) * 4) as usize;
            if frame.rgba8[idx + 3] > 20 {
                if x < w / 2 - 4 {
                    left_visible += 1;
                } else if x > w / 2 + 4 {
                    right_visible += 1;
                }
            }
        }
    }
    assert_eq!(
        left_visible, 0,
        "left foreground (eye_z=1.0) must occlude shape at eye_z=3.0"
    );
    assert!(
        right_visible > 100,
        "right background (eye_z=5.0) must leave shape visible, got {right_visible} pixels"
    );
}
