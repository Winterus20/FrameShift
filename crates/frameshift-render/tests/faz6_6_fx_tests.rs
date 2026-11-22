use std::collections::HashSet;
use std::path::Path;

use frameshift_render::{
    FnFrameProvider, GpuCompositor, NoopAnimator, OutputAlphaMode, RenderError, RenderOptions,
    SourceFrame, Text2DJfaUniform, TonemapMode,
};
use frameshift_schema::{
    ColorSpace, GraphNodeDef, HalftoneColorMode, NodeId, NodeKind, RenderGraphDef, SourceFit,
    TextAlign2D, WritingMode2D,
};
use frameshift_time::{FLICKS_PER_SECOND, FPS_60, Flicks};

fn test_opts() -> RenderOptions {
    RenderOptions {
        tonemap_mode: Some(TonemapMode::None),
        output_alpha_mode: OutputAlphaMode::Straight,
        ..Default::default()
    }
}

fn plate_a(w: u32, h: u32) -> SourceFrame {
    let mut data = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let idx = ((y * w + x) * 4) as usize;
            let checker = ((x / 10) + (y / 10)) % 2 == 0;
            let r = if checker { 230 } else { 40 };
            let g = ((x * 255) / w.max(1)) as u8;
            let b = ((y * 255) / h.max(1)) as u8;
            data[idx] = r;
            data[idx + 1] = g;
            data[idx + 2] = b;
            data[idx + 3] = 255;
        }
    }
    SourceFrame::rgba8_straight(w, h, data)
}

fn plate_b(w: u32, h: u32) -> SourceFrame {
    let mut data = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let idx = ((y * w + x) * 4) as usize;
            let stripe = (x / 8) % 2 == 0;
            data[idx] = if stripe { 30 } else { 210 };
            data[idx + 1] = 190u8.saturating_sub(((y * 160) / h.max(1)) as u8);
            data[idx + 2] = 240;
            data[idx + 3] = 255;
        }
    }
    SourceFrame::rgba8_straight(w, h, data)
}

fn centered_glow_sprite(w: u32, h: u32) -> SourceFrame {
    let mut data = vec![0u8; (w * h * 4) as usize];
    let cx = w as f32 * 0.5;
    let cy = h as f32 * 0.5;
    for y in 0..h {
        for x in 0..w {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let d = (dx * dx + dy * dy).sqrt();
            if d <= 14.0 {
                let idx = ((y * w + x) * 4) as usize;
                data[idx] = 255;
                data[idx + 1] = 120;
                data[idx + 2] = 40;
                data[idx + 3] = 255;
            }
        }
    }
    SourceFrame::rgba8_straight(w, h, data)
}

#[test]
fn radial_shockwave_smooth_derivative_and_spectral_ca_render_cleanly() {
    assert_eq!(std::mem::size_of::<Text2DJfaUniform>(), 16);

    let mut compositor = GpuCompositor::new_headless().expect("headless compositor");
    let frame_ticks = FPS_60.frame_duration().unwrap();
    let provider = FnFrameProvider(|_: &str, _: Flicks, w: u32, h: u32| {
        // Smooth radial ramp so any C0 sign(diff) seam would create a sharp jump.
        let mut data = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let idx = ((y * w + x) * 4) as usize;
                let v = ((x * 240) / w.max(1)) as u8;
                data[idx] = v;
                data[idx + 1] = ((y * 240) / h.max(1)) as u8;
                data[idx + 2] = 180;
                data[idx + 3] = 255;
            }
        }
        Ok(SourceFrame::rgba8_straight(w, h, data))
    });

    let graph = RenderGraphDef {
        output_node: NodeId::from("shock"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("src"),
                kind: NodeKind::SourceVideo {
                    asset_id: "ramp".into(),
                    clip_id: None,
                    color_space: ColorSpace::LinearRec709,
                    fit: SourceFit::Stretch,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("shock"),
                kind: NodeKind::RadialShockwave {
                    center_uv: [0.5, 0.5],
                    radius: 0.30,
                    ring_width: 0.16,
                    refraction_amp: 0.08,
                    zoom_blur_strength: 0.25,
                    chromatic_aberration: 0.06,
                },
                inputs: vec![NodeId::from("src")],
                enabled: true,
            },
        ],
    };

    let out_a = compositor
        .render_frame(&graph, Flicks::ZERO, frame_ticks, 160, 90, &provider, &NoopAnimator, &test_opts())
        .expect("shockwave pass 1");
    let out_b = compositor
        .render_frame(&graph, Flicks::ZERO, frame_ticks, 160, 90, &provider, &NoopAnimator, &test_opts())
        .expect("shockwave pass 2");
    assert_eq!(out_a.fnv1a_hash, out_b.fnv1a_hash, "shockwave must be deterministic");

    // Across the ring crest (radius = 0.30 * 90 = 27px above center y=45 -> y=18),
    // adjacent vertical pixels must transition smoothly without a sign(diff) step spike.
    let mut max_step = 0u8;
    for y in 12..25 {
        let i0 = ((y * 160 + 80) * 4) as usize;
        let i1 = (((y + 1) * 160 + 80) * 4) as usize;
        max_step = max_step.max(out_a.rgba8[i0].abs_diff(out_a.rgba8[i1]));
    }
    assert!(
        max_step < 28,
        "shockwave crest must be C^1-smooth without sign(diff) tear (max_step={max_step})"
    );
}

#[test]
fn all_ten_transitions_16x9_preserve_endpoints_determinism_and_dynamic_fx() {
    let mut compositor = GpuCompositor::new_headless().expect("headless compositor");
    let frame_ticks = FPS_60.frame_duration().unwrap();
    let provider = FnFrameProvider(|asset_id: &str, _: Flicks, w: u32, h: u32| {
        if asset_id == "a" {
            Ok(plate_a(w, h))
        } else {
            Ok(plate_b(w, h))
        }
    });

    let solo_graph = |asset: &str| RenderGraphDef {
        output_node: NodeId::from("src"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("src"),
            kind: NodeKind::SourceVideo {
                asset_id: asset.into(),
                clip_id: None,
                color_space: ColorSpace::LinearRec709,
                fit: SourceFit::Stretch,
            },
            inputs: vec![],
            enabled: true,
        }],
    };

    let (w, h) = (160, 90);
    let pure_a = compositor
        .render_frame(&solo_graph("a"), Flicks::ZERO, frame_ticks, w, h, &provider, &NoopAnimator, &test_opts())
        .expect("pure a");
    let pure_b = compositor
        .render_frame(&solo_graph("b"), Flicks::ZERO, frame_ticks, w, h, &provider, &NoopAnimator, &test_opts())
        .expect("pure b");

    let mut mid_hashes = HashSet::new();
    for &shader_name in NodeKind::BUILTIN_TRANSITION_SHADERS {
        let make_tr = |progress: f32| RenderGraphDef {
            output_node: NodeId::from("tr"),
            nodes: vec![
                solo_graph("a").nodes[0].clone(),
                solo_graph("b").nodes[0].clone(),
                GraphNodeDef {
                    id: NodeId::from("tr"),
                    kind: NodeKind::ShaderTransition {
                        shader_name: shader_name.into(),
                        progress,
                        direction_rad: std::f32::consts::FRAC_PI_4,
                        whip_strength: 1.0,
                        chromatic_aberration: 0.04,
                        impact_flash: 0.2,
                    },
                    inputs: vec![NodeId::from("src"), NodeId::from("src")],
                    enabled: true,
                },
            ],
        };
        // Fix distinct input IDs cleanly
        let make_tr_fixed = |progress: f32| {
            let mut g = make_tr(progress);
            g.nodes[0].id = NodeId::from("a");
            g.nodes[1].id = NodeId::from("b");
            g.nodes[2].inputs = vec![NodeId::from("a"), NodeId::from("b")];
            g
        };

        let f0 = compositor
            .render_frame(&make_tr_fixed(0.0), Flicks::ZERO, frame_ticks, w, h, &provider, &NoopAnimator, &test_opts())
            .expect("p=0");
        assert_eq!(f0.rgba8, pure_a.rgba8, "{shader_name} p=0 must equal tex_a");

        let f1 = compositor
            .render_frame(&make_tr_fixed(1.0), Flicks::ZERO, frame_ticks, w, h, &provider, &NoopAnimator, &test_opts())
            .expect("p=1");
        assert_eq!(f1.rgba8, pure_b.rgba8, "{shader_name} p=1 must equal tex_b");

        let fm1 = compositor
            .render_frame(&make_tr_fixed(0.5), Flicks::ZERO, frame_ticks, w, h, &provider, &NoopAnimator, &test_opts())
            .expect("p=0.5 a");
        let fm2 = compositor
            .render_frame(&make_tr_fixed(0.5), Flicks::ZERO, frame_ticks, w, h, &provider, &NoopAnimator, &test_opts())
            .expect("p=0.5 b");
        assert_eq!(fm1.fnv1a_hash, fm2.fnv1a_hash, "{shader_name} must be deterministic");
        assert_ne!(fm1.rgba8, pure_a.rgba8);
        assert_ne!(fm1.rgba8, pure_b.rgba8);
        mid_hashes.insert(fm1.fnv1a_hash);
    }

    assert_eq!(
        mid_hashes.len(),
        NodeKind::BUILTIN_TRANSITION_SHADERS.len(),
        "all 10 transitions must produce distinct mid-transition hashes"
    );
}

#[test]
fn halftone_glow_cmyk_colordot_and_additive_guard_work() {
    let mut compositor = GpuCompositor::new_headless().expect("headless compositor");
    let frame_ticks = FPS_60.frame_duration().unwrap();
    let provider = FnFrameProvider(|_: &str, _: Flicks, w: u32, h: u32| {
        Ok(centered_glow_sprite(w, h))
    });

    let make_ht = |color_mode: HalftoneColorMode, dot_softness: f32| RenderGraphDef {
        output_node: NodeId::from("ht"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("src"),
                kind: NodeKind::SourceVideo {
                    asset_id: "sprite".into(),
                    clip_id: None,
                    color_space: ColorSpace::LinearRec709,
                    fit: SourceFit::Stretch,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("ht"),
                kind: NodeKind::HalftoneGlow {
                    dot_scale: 28.0,
                    angle_rad: 0.35,
                    halftone_mix: 0.85,
                    glow_threshold: 0.25,
                    glow_intensity: 0.9,
                    glow_radius_px: 12.0,
                    color_mode,
                    dot_softness,
                },
                inputs: vec![NodeId::from("src")],
                enabled: true,
            },
        ],
    };

    let opts = RenderOptions {
        tonemap_mode: Some(TonemapMode::None),
        output_alpha_mode: OutputAlphaMode::Premultiplied,
        ..Default::default()
    };

    let mono = compositor
        .render_frame(&make_ht(HalftoneColorMode::Monochrome, 0.05), Flicks::ZERO, frame_ticks, 128, 96, &provider, &NoopAnimator, &opts)
        .expect("mono");
    let cmyk = compositor
        .render_frame(&make_ht(HalftoneColorMode::Cmyk, 0.05), Flicks::ZERO, frame_ticks, 128, 96, &provider, &NoopAnimator, &opts)
        .expect("cmyk");
    let color_dot = compositor
        .render_frame(&make_ht(HalftoneColorMode::ColorDot, 0.05), Flicks::ZERO, frame_ticks, 128, 96, &provider, &NoopAnimator, &opts)
        .expect("color_dot");

    assert_ne!(mono.fnv1a_hash, cmyk.fnv1a_hash);
    assert_ne!(mono.fnv1a_hash, color_dot.fnv1a_hash);
    assert_ne!(cmyk.fnv1a_hash, color_dot.fnv1a_hash);

    // Far top-left corner (0, 0) outside glow reach must stay strictly transparent zero.
    assert_eq!(&cmyk.rgba8[0..4], &[0, 0, 0, 0]);
}

#[test]
fn text2d_jfa_sdf_renders_thick_smooth_outlines_and_shadow_deterministically() {
    let font_path = [
        r"C:\Windows\Fonts\impact.ttf",
        r"C:\Windows\Fonts\arialbd.ttf",
        r"C:\Windows\Fonts\arial.ttf",
    ]
    .into_iter()
    .find(|p| Path::new(p).is_file())
    .map(str::to_owned);
    let Some(font_path) = font_path else {
        return;
    };

    let mut compositor = GpuCompositor::new_headless().expect("headless compositor");
    let frame_ticks = FPS_60.frame_duration().unwrap();
    let provider = FnFrameProvider(|asset_id: &str, tick: Flicks, _w: u32, _h: u32| {
        Err(RenderError::FrameProvider {
            asset_id: asset_id.to_owned(),
            source_tick: tick.0,
            message: "no external frame".into(),
        })
    });

    let graph = RenderGraphDef {
        output_node: NodeId::from("jfa_txt"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("jfa_txt"),
            kind: NodeKind::VectorText {
                text: "JFA\nSDF".into(),
                font_size_norm: 0.24,
                color_rgba: [1.0, 0.96, 0.9, 1.0],
                outline_rgba: [0.08, 0.04, 0.16, 1.0],
                outline_width_norm: 0.024,
                align_center: true,
                y_offset_norm: 0.5,
                font_family: None,
                font_path: Some(font_path),
                font_weight: 400,
                align: Some(TextAlign2D::Center),
                x_offset_norm: None,
                tracking_em: Some(0.04),
                line_height_em: Some(1.15),
                opacity: 1.0,
                fallback_fonts: Vec::new(),
                writing_mode: WritingMode2D::HorizontalTb,
                var_wght: None,
                var_wdth: None,
                var_slnt: None,
                color_bottom_rgba: Some([0.2, 0.85, 1.0, 1.0]),
                gradient_angle_deg: 0.0,
                outer_outline_rgba: [1.0, 0.25, 0.65, 1.0],
                outer_outline_width_norm: 0.018,
                outline_gap_norm: 0.010,
                shadow_offset_norm: [0.02, 0.025],
                shadow_blur_norm: 0.025,
                shadow_rgba: [0.02, 0.02, 0.06, 0.85],
                glow_radius_norm: 0.05,
                glow_rgba: [0.3, 0.9, 1.0, 0.7],
                hdr_intensity: 1.2,
                gpu_slug: true,
                text_animator: None,
            },
            inputs: vec![],
            enabled: true,
        }],
    };

    let f1 = compositor
        .render_frame(&graph, Flicks(FLICKS_PER_SECOND), frame_ticks, 320, 180, &provider, &NoopAnimator, &test_opts())
        .expect("jfa text 1");
    let f2 = compositor
        .render_frame(&graph, Flicks(FLICKS_PER_SECOND), frame_ticks, 320, 180, &provider, &NoopAnimator, &test_opts())
        .expect("jfa text 2");
    assert_eq!(f1.fnv1a_hash, f2.fnv1a_hash, "JFA text2d_fx must be deterministic");

    let non_zero = f1.rgba8.as_chunks::<4>().0.iter().filter(|px| px[3] > 10).count();
    assert!(non_zero > 2500, "JFA thick outlines+glow+shadow must expand coverage (got {non_zero})");
}
