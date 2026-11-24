use std::path::Path;

use frameshift_render::{
    FnFrameProvider, GpuCompositor, NoopAnimator, OutputAlphaMode, RenderError, RenderOptions,
    TonemapMode,
};
use frameshift_schema::{
    BevelProfile3D, GraphNodeDef, NodeId, NodeKind, ParamBindingDef, Project, RenderGraphDef,
    RenderProfileDef, RenderProfileKind, RenderProfilesDef, Resolution, TextAlign2D, TimelineDef,
    TextAnimatorDef, TextSelectorUnit, WorkingPixelFormat, WritingMode2D, CURRENT_SCHEMA_VERSION,
};
use frameshift_time::{Flicks, FLICKS_PER_SECOND, FPS_60};

fn first_existing_font(candidates: &[&str]) -> Option<String> {
    candidates
        .iter()
        .find(|p| Path::new(p).is_file())
        .map(|s| (*s).to_string())
}

fn test_opts() -> RenderOptions {
    RenderOptions {
        tonemap_mode: Some(TonemapMode::None),
        output_alpha_mode: OutputAlphaMode::Straight,
        ..Default::default()
    }
}

fn no_override_provider() -> impl frameshift_render::FrameProvider {
    FnFrameProvider(|asset_id: &str, tick: Flicks, _w: u32, _h: u32| {
        Err(RenderError::FrameProvider {
            asset_id: asset_id.to_owned(),
            source_tick: tick.0,
            message: "no external frame".into(),
        })
    })
}

fn count_non_zero_alpha(rgba8: &[u8]) -> usize {
    rgba8.as_chunks::<4>().0.iter().filter(|p| p[3] > 10).count()
}

#[test]
fn wgsl_slug_renders_gradient_double_anime_outline_knockout_glow_and_shadow() {
    let Some(font_path) = first_existing_font(&[
        r"C:\Windows\Fonts\impact.ttf",
        r"C:\Windows\Fonts\arialbd.ttf",
        r"C:\Windows\Fonts\arial.ttf",
    ]) else {
        return;
    };

    let mut compositor = GpuCompositor::new_headless().expect("headless compositor");
    let frame_ticks = FPS_60.frame_duration().unwrap();
    let provider = no_override_provider();

    let make_graph = |with_fx: bool, gpu_slug: bool| RenderGraphDef {
        output_node: NodeId::from("slug_title"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("slug_title"),
            kind: NodeKind::VectorText {
                text: "FRAME\nSHIFT".into(),
                font_size_norm: 0.22,
                color_rgba: [1.0, 0.96, 0.88, 1.0],
                outline_rgba: if with_fx {
                    [0.05, 0.04, 0.14, 1.0]
                } else {
                    [0.0; 4]
                },
                outline_width_norm: if with_fx { 0.012 } else { 0.0 },
                align_center: true,
                y_offset_norm: 0.5,
                font_family: None,
                font_path: Some(font_path.clone()),
                font_weight: 400,
                align: Some(TextAlign2D::Center),
                x_offset_norm: None,
                tracking_em: Some(0.03),
                line_height_em: Some(1.15),
                opacity: 1.0,
                fallback_fonts: Vec::new(),
                writing_mode: WritingMode2D::HorizontalTb,
                var_wght: None,
                var_wdth: None,
                var_slnt: None,
                color_bottom_rgba: Some([1.0, 0.18, 0.52, 1.0]),
                gradient_angle_deg: 12.0,
                outer_outline_rgba: if with_fx {
                    [0.12, 0.92, 1.0, 1.0]
                } else {
                    [0.0; 4]
                },
                outer_outline_width_norm: if with_fx { 0.010 } else { 0.0 },
                outline_gap_norm: if with_fx { 0.006 } else { 0.0 },
                shadow_offset_norm: if with_fx { [0.015, 0.018] } else { [0.0, 0.0] },
                shadow_blur_norm: if with_fx { 0.012 } else { 0.0 },
                shadow_rgba: if with_fx {
                    [0.02, 0.02, 0.05, 0.85]
                } else {
                    [0.0; 4]
                },
                glow_radius_norm: if with_fx { 0.03 } else { 0.0 },
                glow_rgba: if with_fx {
                    [0.95, 0.2, 0.7, 0.65]
                } else {
                    [0.0; 4]
                },
                hdr_intensity: 1.25,
                gpu_slug,
                text_animator: None,
            },
            inputs: vec![],
            enabled: true,
        }],
    };

    let bare_a = compositor
        .render_frame(
            &make_graph(false, true),
            Flicks::ZERO,
            frame_ticks,
            320,
            180,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("bare slug");
    let bare_b = compositor
        .render_frame(
            &make_graph(false, true),
            Flicks::ZERO,
            frame_ticks,
            320,
            180,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("bare slug repeat");
    assert_eq!(bare_a.fnv1a_hash, bare_b.fnv1a_hash, "Slug must be deterministic");
    let bare_px = count_non_zero_alpha(&bare_a.rgba8);
    assert!(bare_px > 200, "Slug must render non-empty glyphs (got {bare_px})");

    let fx_frame = compositor
        .render_frame(
            &make_graph(true, true),
            Flicks::ZERO,
            frame_ticks,
            320,
            180,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("slug with double outline + glow + shadow");
    let fx_px = count_non_zero_alpha(&fx_frame.rgba8);
    assert!(
        fx_px > bare_px + 200,
        "2D FX (double outline + glow + shadow) must expand visible coverage ({fx_px} vs {bare_px})"
    );
    assert_ne!(bare_a.fnv1a_hash, fx_frame.fnv1a_hash);
}

#[test]
fn shaped_text_animator_selects_grapheme_range_and_staggers_in_ticks() {
    let font_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fonts/Anton-Regular.ttf");
    if !Path::new(font_path).is_file() {
        return;
    }
    let mut compositor = GpuCompositor::new_headless().expect("headless compositor");
    let provider = no_override_provider();
    let frame_ticks = FPS_60.frame_duration().expect("frame duration");
    let graph = RenderGraphDef {
        output_node: NodeId::from("title"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("title"),
            kind: NodeKind::VectorText {
                text: "AB".into(),
                font_size_norm: 0.36,
                color_rgba: [1.0, 1.0, 1.0, 1.0],
                outline_rgba: [0.0; 4],
                outline_width_norm: 0.0,
                align_center: true,
                y_offset_norm: 0.5,
                font_family: None,
                font_path: Some(font_path.into()),
                font_weight: 400,
                align: Some(TextAlign2D::Center),
                x_offset_norm: None,
                tracking_em: Some(0.0),
                line_height_em: None,
                opacity: 1.0,
                fallback_fonts: Vec::new(),
                writing_mode: WritingMode2D::HorizontalTb,
                var_wght: None,
                var_wdth: None,
                var_slnt: None,
                color_bottom_rgba: None,
                gradient_angle_deg: 0.0,
                outer_outline_rgba: [0.0; 4],
                outer_outline_width_norm: 0.0,
                outline_gap_norm: 0.0,
                shadow_offset_norm: [0.0; 2],
                shadow_blur_norm: 0.0,
                shadow_rgba: [0.0; 4],
                glow_radius_norm: 0.0,
                glow_rgba: [0.0; 4],
                hdr_intensity: 1.0,
                gpu_slug: false,
                text_animator: Some(TextAnimatorDef {
                    unit: TextSelectorUnit::Grapheme,
                    range_start: 0,
                    range_end: Some(1),
                    start_tick: 0,
                    duration_ticks: FLICKS_PER_SECOND,
                    delay_ticks: 0,
                    position_from_norm: [0.0; 2],
                    scale_from: 1.0,
                    opacity_from: 0.0,
                    ..Default::default()
                }),
            },
            inputs: vec![],
            enabled: true,
        }],
    };
    let (start_coverage, middle_coverage, end_coverage) = {
        let mut render_at = |tick| {
            compositor
                .render_frame(
                    &graph,
                    tick,
                    frame_ticks,
                    160,
                    90,
                    &provider,
                    &NoopAnimator,
                    &test_opts(),
                )
                .expect("text animator render")
        };

        let start = render_at(Flicks::ZERO);
        let middle = render_at(Flicks(FLICKS_PER_SECOND / 2));
        let end = render_at(Flicks(FLICKS_PER_SECOND));
        (
            count_non_zero_alpha(&start.rgba8),
            count_non_zero_alpha(&middle.rgba8),
            count_non_zero_alpha(&end.rgba8),
        )
    };
    assert!(start_coverage > 0, "the unselected B remains visible");
    assert!(middle_coverage > start_coverage);
    assert!(end_coverage > middle_coverage);

    let mut word_graph = graph.clone();
    let NodeKind::VectorText {
        text,
        text_animator,
        ..
    } = &mut word_graph.nodes[0].kind
    else {
        unreachable!("test graph contains vector text")
    };
    *text = "A B".into();
    *text_animator = Some(TextAnimatorDef {
        unit: TextSelectorUnit::Word,
        range_start: 1,
        range_end: Some(2),
        start_tick: 0,
        duration_ticks: FLICKS_PER_SECOND,
        delay_ticks: 0,
        position_from_norm: [0.0; 2],
        scale_from: 1.0,
        opacity_from: 0.0,
        ..Default::default()
    });
    let mut render_word_at = |tick| {
        compositor
            .render_frame(
                &word_graph,
                tick,
                frame_ticks,
                160,
                90,
                &provider,
                &NoopAnimator,
                &test_opts(),
            )
            .expect("word selector render")
    };
    let start_word = render_word_at(Flicks::ZERO);
    let middle_word = render_word_at(Flicks(FLICKS_PER_SECOND / 2));
    let end_word = render_word_at(Flicks(FLICKS_PER_SECOND));
    let start_word_coverage = count_non_zero_alpha(&start_word.rgba8);
    let middle_word_coverage = count_non_zero_alpha(&middle_word.rgba8);
    let end_word_coverage = count_non_zero_alpha(&end_word.rgba8);
    assert!(start_word_coverage > 0, "the unselected A remains visible");
    assert!(middle_word_coverage > start_word_coverage);
    assert!(end_word_coverage > middle_word_coverage);
}

#[test]
fn wgsl_slug_supports_tategaki_japanese_and_animated_variable_font() {
    let Some(jp_path) = first_existing_font(&[
        r"C:\Windows\Fonts\YuGothB.ttc",
        r"C:\Windows\Fonts\msgothic.ttc",
    ]) else {
        return;
    };
    let Some(vf_path) = first_existing_font(&[
        r"C:\Windows\Fonts\bahnschrift.ttf",
        r"C:\Windows\Fonts\arialbd.ttf",
    ]) else {
        return;
    };

    let mut compositor = GpuCompositor::new_headless().expect("headless compositor");
    let frame_ticks = FPS_60.frame_duration().unwrap();
    let provider = no_override_provider();

    let tategaki_graph = RenderGraphDef {
        output_node: NodeId::from("jp_vert"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("jp_vert"),
            kind: NodeKind::VectorText {
                text: "東京\n渋谷".into(),
                font_size_norm: 0.18,
                color_rgba: [1.0, 0.95, 0.9, 1.0],
                outline_rgba: [0.05, 0.05, 0.12, 1.0],
                outline_width_norm: 0.01,
                align_center: true,
                y_offset_norm: 0.5,
                font_family: None,
                font_path: Some(jp_path),
                font_weight: 400,
                align: Some(TextAlign2D::Center),
                x_offset_norm: None,
                tracking_em: None,
                line_height_em: Some(1.3),
                opacity: 1.0,
                fallback_fonts: Vec::new(),
                writing_mode: WritingMode2D::VerticalRl,
                var_wght: None,
                var_wdth: None,
                var_slnt: None,
                color_bottom_rgba: Some([0.3, 0.85, 1.0, 1.0]),
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
    let jp_out = compositor
        .render_frame(
            &tategaki_graph,
            Flicks::ZERO,
            frame_ticks,
            240,
            240,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("tategaki gpu render");
    assert!(count_non_zero_alpha(&jp_out.rgba8) > 200);

    // Variable font weight animation via SafeExpr binding
    let profile = RenderProfileDef {
        kind: RenderProfileKind::Draft,
        resolution: Resolution {
            width: 240,
            height: 120,
        },
        motion_blur_sub_samples: 1,
        shutter_angle_deg: 0.0,
        pixel_format: WorkingPixelFormat::Rgba16FloatPremul,
    };
    let vf_project = Project {
        schema_version: CURRENT_SCHEMA_VERSION,
        revision_id: 1,
        name: "VF Anim".into(),
        assets: vec![],
        timeline: TimelineDef {
            fps: FPS_60,
            duration_ticks: Flicks(2 * FLICKS_PER_SECOND),
            resolution: Resolution {
                width: 240,
                height: 120,
            },
            clips: vec![],
            audio_clips: vec![],
            ducking_rules: vec![],
            micro_silences: vec![],
        },
        events: vec![],
        graph: RenderGraphDef {
            output_node: NodeId::from("vf"),
            nodes: vec![GraphNodeDef {
                id: NodeId::from("vf"),
                kind: NodeKind::VectorText {
                    text: "KINETIC".into(),
                    font_size_norm: 0.34,
                    color_rgba: [1.0, 1.0, 1.0, 1.0],
                    outline_rgba: [0.0; 4],
                    outline_width_norm: 0.0,
                    align_center: true,
                    y_offset_norm: 0.5,
                    font_family: None,
                    font_path: Some(vf_path),
                    font_weight: 400,
                    align: Some(TextAlign2D::Center),
                    x_offset_norm: None,
                    tracking_em: None,
                    line_height_em: None,
                    opacity: 1.0,
                    fallback_fonts: Vec::new(),
                    writing_mode: WritingMode2D::HorizontalTb,
                    var_wght: Some(300.0),
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
        },
        render_profiles: RenderProfilesDef {
            draft: profile.clone(),
            final_profile: profile,
        },
        param_bindings: vec![ParamBindingDef {
            node_id: NodeId::from("vf"),
            param_name: "var_wght".into(),
            expression: "300.0 + 400.0 * clamp(t_sec, 0.0, 1.0)".into(),
        }],
        creative_decisions: vec![],
        preferences: vec![],
        proxy_config: Default::default(),
        hw_accel_policy: Default::default(),
    };

    let vf_t0 = compositor
        .render_project_frame(
            &vf_project,
            Flicks::ZERO,
            &vf_project.render_profiles.draft,
            &provider,
            &test_opts(),
        )
        .expect("vf t=0");
    let vf_t1 = compositor
        .render_project_frame(
            &vf_project,
            Flicks(FLICKS_PER_SECOND),
            &vf_project.render_profiles.draft,
            &provider,
            &test_opts(),
        )
        .expect("vf t=1");
    assert_ne!(vf_t0.fnv1a_hash, vf_t1.fnv1a_hash);
}

#[test]
fn text3d_multiline_curved_bevels_and_iridescent_chrome_render() {
    let Some(font_path) = first_existing_font(&[
        r"C:\Windows\Fonts\impact.ttf",
        r"C:\Windows\Fonts\arialbd.ttf",
    ]) else {
        return;
    };

    let mut compositor = GpuCompositor::new_headless().expect("headless compositor");
    let frame_ticks = FPS_60.frame_duration().unwrap();
    let provider = no_override_provider();

    let make_3d_graph = |profile: BevelProfile3D, material_mode: u32| RenderGraphDef {
        output_node: NodeId::from("t3d"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("t3d"),
            kind: NodeKind::Text3D {
                text: "HYPER\nSHIFT".into(),
                font_path: font_path.clone(),
                font_size: 0.44,
                extrude_depth: 0.18,
                bevel_size: 0.03,
                color_top_rgba: [1.0, 0.96, 1.0, 1.0],
                color_bottom_rgba: [0.35, 0.78, 1.0, 1.0],
                side_color_rgba: [0.22, 0.15, 0.45, 1.0],
                pos: [0.0, 0.0, 0.0],
                rot_deg: [12.0, -18.0, 0.0],
                camera_pos: [0.0, 0.0, 2.3],
                camera_rot_deg: [0.0, 0.0, 0.0],
                fov_deg: 50.0,
                char_stagger_sec: 0.0,
                char_bounce_amp: 0.0,
                char_wave_freq: 0.0,
                time_offset_sec: 0.0,
                outline_width: 0.0,
                outline_color_rgba: [1.0; 4],
                material_mode,
                color_mode: 0,
                palette_rgba: Vec::new(),
                pop_duration_sec: 0.0,
                pop_overshoot: 2.15,
                spin_in_deg: 0.0,
                char_jitter_deg: 0.0,
                arc_curvature: 0.15,
                arc_axis: 0,
                echo_count: 2,
                echo_offset: [0.0, 0.0, -0.25],
                echo_alpha_decay: 0.5,
                echo_scale_step: 0.95,
                hdr_intensity: 1.25,
                opacity: 1.0,
                align: TextAlign2D::Center,
                line_height_em: Some(1.1),
                tracking_em: 0.03,
                bevel_profile: profile,
                bevel_segments: 6,
                fallback_fonts: Vec::new(),
                var_wght: None,
                var_wdth: None,
                var_slnt: None,
            },
            inputs: vec![],
            enabled: true,
        }],
    };

    let flat = compositor
        .render_frame(
            &make_3d_graph(BevelProfile3D::Flat, 1),
            Flicks::ZERO,
            frame_ticks,
            240,
            160,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("flat 3d");
    let round_irid = compositor
        .render_frame(
            &make_3d_graph(BevelProfile3D::Round, 3),
            Flicks::ZERO,
            frame_ticks,
            240,
            160,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("round iridescent 3d");
    let stepped = compositor
        .render_frame(
            &make_3d_graph(BevelProfile3D::Stepped, 3),
            Flicks::ZERO,
            frame_ticks,
            240,
            160,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("stepped iridescent 3d");

    assert!(count_non_zero_alpha(&round_irid.rgba8) > 200);
    assert_ne!(flat.fnv1a_hash, round_irid.fnv1a_hash);
    assert_ne!(round_irid.fnv1a_hash, stepped.fnv1a_hash);
}
