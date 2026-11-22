use frameshift_render::{
    FnFrameProvider, GpuCompositor, NodeAnimator, NoopAnimator, OutputAlphaMode, ProjectAnimator,
    RenderError, RenderOptions, RenderedFrame, SourceFrame, TonemapMode,
};
use frameshift_schema::{
    ColorSpace, GraphNodeDef, NodeId, NodeKind, ParamBindingDef, Project, RenderGraphDef,
    SourceFit,
};
use frameshift_time::{FLICKS_PER_SECOND, Flicks};
use std::path::Path;

fn checker_plate(width: u32, height: u32) -> SourceFrame {
    let mut data = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let idx = ((y * width + x) * 4) as usize;
            let checker = ((x / 8) + (y / 8)) % 2 == 0;
            data[idx] = if checker { 235 } else { 40 };
            data[idx + 1] = ((x * 255) / width.max(1)) as u8;
            data[idx + 2] = ((y * 255) / height.max(1)) as u8;
            data[idx + 3] = 255;
        }
    }
    SourceFrame::rgba8_straight(width, height, data)
}

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
    let provider = FnFrameProvider(|_: &str, _: Flicks, w: u32, h: u32| Ok(checker_plate(w, h)));
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

#[test]
fn transform3d_perspective_render_is_deterministic_and_responds_to_rotation() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let (w, h) = (96, 64);

    let make_graph = |rot_deg: [f32; 3], dof_blur_strength: f32| RenderGraphDef {
        output_node: NodeId::from("t3d"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("src"),
                kind: NodeKind::SourceVideo {
                    asset_id: "plate".into(),
                    clip_id: None,
                    color_space: ColorSpace::LinearRec709,
                    fit: SourceFit::Stretch,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("t3d"),
                kind: NodeKind::Transform3D {
                    pos: [0.0, 0.0, 0.0],
                    rot_deg,
                    scale: [1.0, 1.0, 1.0],
                    fov_deg: 50.0,
                    camera_pos: [0.0, 0.0, 2.5],
                    camera_rot_deg: [0.0, 0.0, 0.0],
                    dof_focal_dist: 2.5,
                    dof_blur_strength,
                },
                inputs: vec![NodeId::from("src")],
                enabled: true,
            },
        ],
    };

    let flat = render_at(&mut compositor, &make_graph([0.0, 0.0, 0.0], 0.0), Flicks::ZERO, w, h)
        .expect("flat transform3d render");
    let tilted_1 =
        render_at(&mut compositor, &make_graph([15.0, 35.0, -8.0], 0.0), Flicks::ZERO, w, h)
            .expect("tilted transform3d render 1");
    let tilted_2 =
        render_at(&mut compositor, &make_graph([15.0, 35.0, -8.0], 0.0), Flicks::ZERO, w, h)
            .expect("tilted transform3d render 2");
    let dof_tilted =
        render_at(&mut compositor, &make_graph([15.0, 35.0, -8.0], 1.2), Flicks::ZERO, w, h)
            .expect("dof transform3d render");

    assert_eq!(tilted_1.fnv1a_hash, tilted_2.fnv1a_hash);
    assert_ne!(flat.rgba8, tilted_1.rgba8);
    assert_ne!(tilted_1.rgba8, dof_tilted.rgba8);
    assert!(count_non_zero_alpha(&tilted_1.rgba8) > 100);
    // Perspective tilt should leave transparent background outside the rotated quad.
    assert!(count_non_zero_alpha(&tilted_1.rgba8) < (w * h) as usize);
}

#[test]
fn text3d_renders_real_windows_ttf_and_rejects_missing_font_cleanly() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let (w, h) = (128, 72);
    let font_path = find_windows_ttf();

    let graph = RenderGraphDef {
        output_node: NodeId::from("title"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("title"),
            kind: NodeKind::Text3D {
                text: "BANG!".into(),
                font_path,
                font_size: 0.55,
                extrude_depth: 0.16,
                bevel_size: 0.01,
                color_top_rgba: [1.0, 0.95, 1.0, 1.0],
                color_bottom_rgba: [1.0, 0.2, 0.75, 1.0],
                side_color_rgba: [0.25, 0.05, 0.45, 1.0],
                pos: [0.0, 0.0, 0.0],
                rot_deg: [12.0, -20.0, 0.0],
                camera_pos: [0.0, 0.0, 2.5],
                camera_rot_deg: [0.0, 0.0, 0.0],
                fov_deg: 50.0,
                char_stagger_sec: 0.06,
                char_bounce_amp: 0.08,
                char_wave_freq: 4.0,
                time_offset_sec: 0.0,
                outline_width: 0.0,
                outline_color_rgba: [1.0, 1.0, 1.0, 1.0],
                material_mode: 0,
                color_mode: 0,
                palette_rgba: Vec::new(),
                pop_duration_sec: 0.0,
                pop_overshoot: 2.15,
                spin_in_deg: 0.0,
                char_jitter_deg: 0.0,
                arc_curvature: 0.0,
                arc_axis: 0,
                echo_count: 0,
                echo_offset: [0.0, 0.0, -0.35],
                echo_alpha_decay: 0.55,
                echo_scale_step: 0.94,
                hdr_intensity: 1.0,
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

    let t0 = render_at(&mut compositor, &graph, Flicks::ZERO, w, h).expect("text3d t0");
    let t0_again =
        render_at(&mut compositor, &graph, Flicks::ZERO, w, h).expect("text3d t0 repeat");
    let t1 = render_at(&mut compositor, &graph, Flicks(FLICKS_PER_SECOND / 2), w, h)
        .expect("text3d t=0.5s");

    assert_eq!(t0.fnv1a_hash, t0_again.fnv1a_hash);
    assert!(count_non_zero_alpha(&t0.rgba8) > 80);
    assert!(max_rgb_luma(&t0.rgba8) > 50);
    assert_ne!(t0.rgba8, t1.rgba8, "per-character wave must animate over time");

    // Missing font must return an explicit RenderError without panicking.
    let missing_graph = RenderGraphDef {
        output_node: NodeId::from("missing"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("missing"),
            kind: NodeKind::Text3D {
                text: "ERR".into(),
                font_path: r"C:\Windows\Fonts\definitely_missing_font_faz6_2.ttf".into(),
                font_size: 0.45,
                extrude_depth: 0.12,
                bevel_size: 0.01,
                color_top_rgba: [1.0; 4],
                color_bottom_rgba: [1.0; 4],
                side_color_rgba: [0.5; 4],
                pos: [0.0; 3],
                rot_deg: [0.0; 3],
                camera_pos: [0.0, 0.0, 2.5],
                camera_rot_deg: [0.0; 3],
                fov_deg: 50.0,
                char_stagger_sec: 0.06,
                char_bounce_amp: 0.08,
                char_wave_freq: 4.0,
                time_offset_sec: 0.0,
                outline_width: 0.0,
                outline_color_rgba: [1.0; 4],
                material_mode: 0,
                color_mode: 0,
                palette_rgba: Vec::new(),
                pop_duration_sec: 0.0,
                pop_overshoot: 2.15,
                spin_in_deg: 0.0,
                char_jitter_deg: 0.0,
                arc_curvature: 0.0,
                arc_axis: 0,
                echo_count: 0,
                echo_offset: [0.0, 0.0, -0.35],
                echo_alpha_decay: 0.55,
                echo_scale_step: 0.94,
                hdr_intensity: 1.0,
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
    let err = render_at(&mut compositor, &missing_graph, Flicks::ZERO, w, h)
        .expect_err("missing font must fail cleanly");
    assert!(
        matches!(err, RenderError::Vector(_)),
        "expected RenderError::Vector, got {err:?}"
    );
}

#[test]
fn particle_field3d_is_seek_independent_and_non_empty() {
    let (w, h) = (96, 64);
    let graph = RenderGraphDef {
        output_node: NodeId::from("particles"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("particles"),
            kind: NodeKind::ParticleField3D {
                count: 140,
                seed: 42,
                shape: 1,
                color_a_rgba: [1.0, 0.3, 0.85, 1.0],
                color_b_rgba: [0.2, 0.9, 1.0, 1.0],
                speed: 0.9,
                spread: 1.8,
                size: 0.08,
                camera_pos: [0.0, 0.0, 2.5],
                camera_rot_deg: [0.0, 0.0, 0.0],
                fov_deg: 50.0,
                time_offset_sec: 0.0,
                dof_focal_dist: 2.5,
                dof_blur_strength: 0.35,
                opacity: 1.0,
            },
            inputs: vec![],
            enabled: true,
        }],
    };

    let target_tick = Flicks(FLICKS_PER_SECOND * 3 / 2);

    // Direct seek to t = 1.5s on a fresh compositor.
    let mut direct_comp = GpuCompositor::new_headless().expect("direct compositor");
    let direct_frame = render_at(&mut direct_comp, &graph, target_tick, w, h)
        .expect("direct seek particle render");

    // Sequential render from t = 0.0 -> 0.5 -> 1.0 -> 1.5 on a separate compositor.
    let mut seq_comp = GpuCompositor::new_headless().expect("seq compositor");
    for step in [0i64, 1, 2] {
        let tick = Flicks(FLICKS_PER_SECOND * step / 2);
        let _ = render_at(&mut seq_comp, &graph, tick, w, h).expect("seq step");
    }
    let seq_frame = render_at(&mut seq_comp, &graph, target_tick, w, h)
        .expect("seq target particle render");

    assert_eq!(direct_frame.fnv1a_hash, seq_frame.fnv1a_hash);
    assert_eq!(direct_frame.rgba8, seq_frame.rgba8);
    assert!(count_non_zero_alpha(&direct_frame.rgba8) > 30);
    assert!(max_rgb_luma(&direct_frame.rgba8) > 50);
}

#[test]
fn ribbon_trail3d_renders_non_empty_hdr_and_supports_safe_expr_animation() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let (w, h) = (96, 64);

    let mut project = Project::golden_scene_preset();
    project.name = "ribbon3d_test".into();
    project.timeline.duration_ticks = Flicks(FLICKS_PER_SECOND * 2);
    project.timeline.clips.clear();
    project.timeline.audio_clips.clear();
    project.assets.clear();
    project.events.clear();
    project.graph = RenderGraphDef {
        output_node: NodeId::from("ribbons"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("ribbons"),
            kind: NodeKind::RibbonTrail3D {
                ribbon_count: 4,
                segments: 48,
                seed: 7,
                width: 0.05,
                speed: 1.0,
                radius: 1.0,
                trim_start: 0.0,
                trim_end: 1.0,
                color_a_rgba: [1.0, 0.15, 0.7, 1.0],
                color_b_rgba: [0.1, 0.95, 1.0, 1.0],
                camera_pos: [0.0, 0.0, 2.5],
                camera_rot_deg: [0.0, 0.0, 0.0],
                fov_deg: 50.0,
                time_offset_sec: 0.0,
                ribbon_style: 0,
                strand_spacing: 0.06,
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
        }],
    };
    project.param_bindings = vec![ParamBindingDef {
        node_id: NodeId::from("ribbons"),
        param_name: "trim_end".into(),
        expression: "clamp(0.2 + 0.8 * progress, 0.0, 1.0)".into(),
    }];
    project.render_profiles.draft.resolution.width = w;
    project.render_profiles.draft.resolution.height = h;
    project.validate().expect("valid project");

    let provider = FnFrameProvider(|_: &str, _: Flicks, width: u32, height: u32| {
        Ok(checker_plate(width, height))
    });
    let profile = project.render_profiles.draft.clone();
    let opts = RenderOptions {
        tonemap_mode: Some(TonemapMode::AcesLite),
        output_alpha_mode: OutputAlphaMode::Straight,
        ..Default::default()
    };

    let early = compositor
        .render_project_frame(&project, Flicks(FLICKS_PER_SECOND / 10), &profile, &provider, &opts)
        .expect("early ribbon render");
    let late = compositor
        .render_project_frame(&project, Flicks(FLICKS_PER_SECOND), &profile, &provider, &opts)
        .expect("late ribbon render");
    let late_again = compositor
        .render_project_frame(&project, Flicks(FLICKS_PER_SECOND), &profile, &provider, &opts)
        .expect("late ribbon repeat");

    assert_eq!(late.fnv1a_hash, late_again.fnv1a_hash);
    assert!(count_non_zero_alpha(&late.rgba8) > 40);
    assert!(max_rgb_luma(&late.rgba8) > 80);
    assert_ne!(early.rgba8, late.rgba8);

    // Also confirm ProjectAnimator accepts all new SafeExpr bindings cleanly.
    project.param_bindings.push(ParamBindingDef {
        node_id: NodeId::from("ribbons"),
        param_name: "time_offset_sec".into(),
        expression: "0.25".into(),
    });
    let animator = ProjectAnimator::new(&project).expect("compile bindings");
    let evaluated = animator
        .evaluate_node(
            &NodeId::from("ribbons"),
            &project.graph.nodes[0].kind,
            Flicks::ZERO,
        )
        .expect("eval node");
    match evaluated {
        NodeKind::RibbonTrail3D {
            trim_end,
            time_offset_sec,
            ..
        } => {
            assert!((trim_end - 0.2).abs() < 1e-4);
            assert!((time_offset_sec - 0.25).abs() < 1e-4);
        }
        other => panic!("expected RibbonTrail3D, got {other:?}"),
    }
}

#[test]
fn text3d_bevel_size_changes_rendered_output_and_mesh_cache_key() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let (w, h) = (128, 72);
    let font_path = find_windows_ttf();

    let make_graph = |bevel_size: f32| RenderGraphDef {
        output_node: NodeId::from("title"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("title"),
            kind: NodeKind::Text3D {
                text: "FX".into(),
                font_path: font_path.clone(),
                font_size: 0.65,
                extrude_depth: 0.20,
                bevel_size,
                color_top_rgba: [1.0, 0.95, 1.0, 1.0],
                color_bottom_rgba: [1.0, 0.2, 0.75, 1.0],
                side_color_rgba: [0.25, 0.05, 0.45, 1.0],
                pos: [0.0, 0.0, 0.0],
                rot_deg: [18.0, -25.0, 0.0],
                camera_pos: [0.0, 0.0, 2.2],
                camera_rot_deg: [0.0, 0.0, 0.0],
                fov_deg: 50.0,
                char_stagger_sec: 0.0,
                char_bounce_amp: 0.0,
                char_wave_freq: 0.0,
                time_offset_sec: 0.0,
                outline_width: 0.0,
                outline_color_rgba: [1.0, 1.0, 1.0, 1.0],
                material_mode: 0,
                color_mode: 0,
                palette_rgba: Vec::new(),
                pop_duration_sec: 0.0,
                pop_overshoot: 2.15,
                spin_in_deg: 0.0,
                char_jitter_deg: 0.0,
                arc_curvature: 0.0,
                arc_axis: 0,
                echo_count: 0,
                echo_offset: [0.0, 0.0, -0.35],
                echo_alpha_decay: 0.55,
                echo_scale_step: 0.94,
                hdr_intensity: 1.0,
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

    let flat = render_at(&mut compositor, &make_graph(0.0), Flicks::ZERO, w, h).expect("flat");
    let beveled =
        render_at(&mut compositor, &make_graph(0.04), Flicks::ZERO, w, h).expect("beveled");

    assert_ne!(
        flat.fnv1a_hash, beveled.fnv1a_hash,
        "Text3D bevel_size must produce a distinct mesh cache entry and rendered frame"
    );
}
