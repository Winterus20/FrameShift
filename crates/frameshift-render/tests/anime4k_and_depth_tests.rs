use frameshift_render::{
    FnFrameProvider, GpuCompositor, NoopAnimator, OutputAlphaMode, ProjectAnimator,
    RenderOptions, SourceFrame, TonemapMode,
};
use frameshift_schema::{
    Anime4KPreset, CURRENT_SCHEMA_VERSION, Camera3DDef, Camera3DMode, ColorSpace, GraphNodeDef,
    NodeId, NodeKind, ParamBindingDef, Project, RenderGraphDef, RenderProfileDef,
    RenderProfileKind, RenderProfilesDef, Resolution, SceneDepthPlateDef, Shape3DKind, SourceFit,
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
        name: "anime4k_depth_test".into(),
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

#[test]
fn anime4k_all_presets_preserve_alpha_and_transform_cel_edges() {
    let w = 96u32;
    let h = 96u32;
    let frame_ticks = FPS_24.frame_duration().unwrap();
    let mut compositor = GpuCompositor::new_headless().expect("gpu compositor");
    let opts = test_opts();

    // Synthetic cel-shaded anime card:
    // - Transparent outer border (y < 16 || y >= 80 || x < 16 || x >= 80) -> alpha == 0
    // - Bright cel fill with a crisp 3px dark diagonal ink line + slight anti-aliased edge
    let provider = FnFrameProvider(|_id: &str, _tick: Flicks, pw: u32, ph: u32| {
        let mut rgba = vec![0u8; (pw * ph * 4) as usize];
        for y in 0..ph {
            for x in 0..pw {
                let idx = ((y * pw + x) * 4) as usize;
                if x < 16 || x >= pw - 16 || y < 16 || y >= ph - 16 {
                    rgba[idx..idx + 4].copy_from_slice(&[0, 0, 0, 0]);
                } else {
                    let dist = (x as i32 - y as i32).abs();
                    if dist <= 1 {
                        rgba[idx..idx + 4].copy_from_slice(&[24, 18, 28, 255]);
                    } else if dist == 2 {
                        rgba[idx..idx + 4].copy_from_slice(&[135, 110, 125, 255]);
                    } else {
                        rgba[idx..idx + 4].copy_from_slice(&[235, 200, 175, 255]);
                    }
                }
            }
        }
        Ok(SourceFrame::rgba8_straight(pw, ph, rgba))
    });

    let build_graph = |preset: Anime4KPreset, strength: f32, clamp_highlights: bool| RenderGraphDef {
        output_node: NodeId::from("a4k"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("src"),
                kind: NodeKind::SourceVideo {
                    asset_id: "cel".into(),
                    clip_id: None,
                    color_space: ColorSpace::DisplaySrgb,
                    fit: SourceFit::Stretch,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("a4k"),
                kind: NodeKind::Anime4K {
                    preset,
                    strength,
                    clamp_highlights,
                },
                inputs: vec![NodeId::from("src")],
                enabled: true,
            },
        ],
    };

    let base_graph = build_graph(Anime4KPreset::ModeA, 0.0, true);
    let base_frame = compositor
        .render_frame(
            &base_graph,
            Flicks::ZERO,
            frame_ticks,
            w,
            h,
            &provider,
            &NoopAnimator,
            &opts,
        )
        .unwrap();

    for preset in [
        Anime4KPreset::ModeA,
        Anime4KPreset::ModeB,
        Anime4KPreset::ModeC,
        Anime4KPreset::RestoreOnly,
        Anime4KPreset::LineArt,
    ] {
        let g = build_graph(preset, 1.0, true);
        let out = compositor
            .render_frame(
                &g,
                Flicks::ZERO,
                frame_ticks,
                w,
                h,
                &provider,
                &NoopAnimator,
                &opts,
            )
            .unwrap();

        // Transparent border must stay completely transparent.
        for y in 0..10 {
            for x in 0..w {
                let idx = ((y * w + x) * 4) as usize;
                assert_eq!(
                    out.rgba8[idx + 3], 0,
                    "{preset:?} leaked alpha into transparent border at ({x},{y})"
                );
            }
        }

        // Opaque region near the diagonal ink line must be visibly enhanced relative to strength=0.
        let mut diff_sum = 0u64;
        for y in 20..(h - 20) {
            for x in 20..(w - 20) {
                let idx = ((y * w + x) * 4) as usize;
                for c in 0..3 {
                    diff_sum += (out.rgba8[idx + c] as i32 - base_frame.rgba8[idx + c] as i32)
                        .unsigned_abs() as u64;
                }
            }
        }
        assert!(
            diff_sum > 200,
            "{preset:?} produced insufficient pixel change vs strength=0 ({diff_sum})"
        );
    }

    let stats = compositor.texture_pool().stats();
    assert!(
        stats.reuses > 0,
        "expected Anime4K ping-pong passes to reuse pooled textures: {stats:?}"
    );
}

#[test]
fn scene3d_depth_plate_disparity_gamma_and_rg16_modes() {
    let w = 128u32;
    let h = 128u32;
    let frame_ticks = FPS_24.frame_duration().unwrap();
    let mut compositor = GpuCompositor::new_headless().expect("gpu compositor");
    let opts = test_opts();

    // Uniform depth = 0.502 (R=128/255), RG16 low byte = 0 (B/G=0).
    let provider_rg16 = FnFrameProvider(|_id: &str, _tick: Flicks, pw: u32, ph: u32| {
        let mut rgba = vec![0u8; (pw * ph * 4) as usize];
        for px in rgba.as_chunks_mut::<4>().0 {
            *px = [128, 0, 128, 255];
        }
        Ok(SourceFrame::rgba8_straight(pw, ph, rgba))
    });

    // Camera at z=3.0 looking at origin; shape at z=0.5 -> eye_z = 2.5.
    // With plate near_z=1.0, far_z=5.0, d ~= 0.5:
    // - linear (disparity_mode=false): eye_z = mix(5.0, 1.0, 0.5) = 3.0 > 2.5 -> VISIBLE
    // - disparity (disparity_mode=true, gamma=1.0): eye_z = 1 / mix(0.2, 1.0, 0.5) = 1.667 < 2.5 -> OCCLUDED
    // - disparity (disparity_mode=true, gamma=3.0): d^3=0.125, eye_z = 1 / mix(0.2, 1.0, 0.125) = 3.333 > 2.5 -> VISIBLE
    let render_with_dp = |comp: &mut GpuCompositor, dp: SceneDepthPlateDef| {
        let graph = RenderGraphDef {
            output_node: NodeId::from("scene"),
            nodes: vec![
                GraphNodeDef {
                    id: NodeId::from("depth_src"),
                    kind: NodeKind::SourceVideo {
                        asset_id: "dp".into(),
                        clip_id: None,
                        color_space: ColorSpace::LinearRec709,
                        fit: SourceFit::Stretch,
                    },
                    inputs: vec![],
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("box3d"),
                    kind: NodeKind::Shape3D {
                        shape_kind: Shape3DKind::NeonTile,
                        size: 1.8,
                        aspect: 1.0,
                        stroke_width: 0.04,
                        corner_radius: 0.0,
                        inner_ratio: 0.382,
                        extrude_depth: 0.05,
                        bevel_size: 0.0,
                        material_mode: 2,
                        color_top_rgba: [1.0, 0.85, 0.2, 1.0],
                        color_bottom_rgba: [1.0, 0.4, 0.1, 1.0],
                        side_color_rgba: [0.8, 0.3, 0.1, 1.0],
                        outline_color_rgba: [1.0, 0.85, 0.2, 1.0],
                        pos: [0.0, 0.0, 0.5],
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
                    },
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
                        depth_plate: Some(dp),
                    },
                    inputs: vec![NodeId::from("box3d")],
                    enabled: true,
                },
            ],
        };
        let proj = make_project(w, h, graph, vec![]);
        let anim = ProjectAnimator::new(&proj).unwrap();
        let frame = comp
            .render_frame(
                &proj.graph,
                Flicks::ZERO,
                frame_ticks,
                w,
                h,
                &provider_rg16,
                &anim,
                &opts,
            )
            .unwrap();
        frame
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|px| px[3] > 20)
            .count()
    };

    let linear_vis = render_with_dp(
        &mut compositor,
        SceneDepthPlateDef {
            node_id: NodeId::from("depth_src"),
            near_z: 1.0,
            far_z: 5.0,
            invert: false,
            depth_gamma: 1.0,
            disparity_mode: false,
            rg16_packed: false,
        },
    );
    let disp_vis = render_with_dp(
        &mut compositor,
        SceneDepthPlateDef {
            node_id: NodeId::from("depth_src"),
            near_z: 1.0,
            far_z: 5.0,
            invert: false,
            depth_gamma: 1.0,
            disparity_mode: true,
            rg16_packed: true,
        },
    );
    let gamma_vis = render_with_dp(
        &mut compositor,
        SceneDepthPlateDef {
            node_id: NodeId::from("depth_src"),
            near_z: 1.0,
            far_z: 5.0,
            invert: false,
            depth_gamma: 3.0,
            disparity_mode: true,
            rg16_packed: true,
        },
    );

    assert!(linear_vis > 500, "linear mode expected visible shape, got {linear_vis}");
    assert_eq!(disp_vis, 0, "disparity mode expected occluded shape, got {disp_vis}");
    assert!(gamma_vis > 500, "depth_gamma=3.0 expected visible shape, got {gamma_vis}");
}

#[test]
fn scene3d_depth_plate_with_dof_does_not_bleed_void_alpha_onto_focused_object() {
    let w = 128u32;
    let h = 128u32;
    let frame_ticks = FPS_24.frame_duration().unwrap();
    let mut compositor = GpuCompositor::new_headless().expect("gpu compositor");
    let opts = test_opts();

    // Left 45% of depth plate is close foreground (d=1.0 -> eye_z=0.8),
    // right 55% is far background (d=0.0 -> eye_z=5.0).
    // The foreground region has NO 3D mesh drawn on it (color_tex.a == 0.0), so
    // fx_dof_coc MUST treat alpha <= 1e-4 as void (0 CoC) instead of bleeding foreground
    // blur onto the in-focus Shape3D at eye_z = 3.0 on the right half!
    let provider = FnFrameProvider(|_id: &str, _tick: Flicks, pw: u32, ph: u32| {
        let mut rgba = vec![0u8; (pw * ph * 4) as usize];
        for y in 0..ph {
            for x in 0..pw {
                let idx = ((y * pw + x) * 4) as usize;
                let v = if x < (pw * 45) / 100 { 255 } else { 0 };
                rgba[idx..idx + 4].copy_from_slice(&[v, v, v, 255]);
            }
        }
        Ok(SourceFrame::rgba8_straight(pw, ph, rgba))
    });

    let render_dof = |comp: &mut GpuCompositor, blur_strength: f32| {
        let graph = RenderGraphDef {
            output_node: NodeId::from("scene"),
            nodes: vec![
                GraphNodeDef {
                    id: NodeId::from("depth_src"),
                    kind: NodeKind::SourceVideo {
                        asset_id: "dp".into(),
                        clip_id: None,
                        color_space: ColorSpace::LinearRec709,
                        fit: SourceFit::Stretch,
                    },
                    inputs: vec![],
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("box3d"),
                    kind: NodeKind::Shape3D {
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
                        color_bottom_rgba: [1.0, 0.9, 0.2, 1.0],
                        side_color_rgba: [1.0, 0.9, 0.2, 1.0],
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
                    },
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
                            dof_focal_dist: 3.0,
                            dof_blur_strength: blur_strength,
                            dof_max_radius_px: 24.0,
                            ..Default::default()
                        },
                        background_rgba: [0.0, 0.0, 0.0, 0.0],
                        depth_plate: Some(SceneDepthPlateDef {
                            node_id: NodeId::from("depth_src"),
                            near_z: 0.8,
                            far_z: 5.0,
                            invert: false,
                            depth_gamma: 1.0,
                            disparity_mode: true,
                            rg16_packed: false,
                        }),
                    },
                    inputs: vec![NodeId::from("box3d")],
                    enabled: true,
                },
            ],
        };
        let proj = make_project(w, h, graph, vec![]);
        let anim = ProjectAnimator::new(&proj).unwrap();
        comp.render_frame(
            &proj.graph,
            Flicks::ZERO,
            frame_ticks,
            w,
            h,
            &provider,
            &anim,
            &opts,
        )
        .unwrap()
    };

    let no_dof = render_dof(&mut compositor, 0.0);
    let with_dof = render_dof(&mut compositor, 2.0);

    // Sample pixels right next to the depth-plate occlusion seam (x = 45..60%) on the visible shape.
    // With alpha <= 1e-4 void gating, the transparent foreground depth plate must NOT smear onto the shape.
    let mut max_alpha_diff = 0i32;
    let mut compared = 0usize;
    for y in (h / 3)..((2 * h) / 3) {
        for x in ((w * 48) / 100)..((w * 65) / 100) {
            let idx = ((y * w + x) * 4) as usize;
            if no_dof.rgba8[idx + 3] > 200 {
                compared += 1;
                max_alpha_diff = max_alpha_diff
                    .max((no_dof.rgba8[idx + 3] as i32 - with_dof.rgba8[idx + 3] as i32).abs());
            }
        }
    }
    assert!(compared > 50, "expected enough opaque pixels to compare");
    assert!(
        max_alpha_diff <= 4,
        "transparent depth_plate foreground bled DoF CoC onto focused shape (max alpha diff = {max_alpha_diff})"
    );
}
