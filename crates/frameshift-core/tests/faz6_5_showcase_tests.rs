use std::path::{Path, PathBuf};

use frameshift_core::{Engine, Flicks, Project, RenderProfileKind};
use frameshift_media::{AlphaMode, Rgba8Frame};
use frameshift_schema::{
    BevelProfile3D, BlendMode, ColorSpace, GraphNodeDef, NodeId, NodeKind, RenderGraphDef,
    Resolution, TextAlign2D, WritingMode2D,
};

fn first_existing_font(candidates: &[&str]) -> Option<String> {
    candidates
        .iter()
        .find(|p| Path::new(p).is_file())
        .map(|s| (*s).to_string())
}

fn blit_panel(dst: &mut [u8], dst_w: u32, src: &Rgba8Frame, ox: u32, oy: u32) {
    for y in 0..src.height {
        let dy = oy + y;
        let src_row = (y * src.width * 4) as usize;
        let dst_row = ((dy * dst_w + ox) * 4) as usize;
        let bytes = (src.width * 4) as usize;
        dst[dst_row..dst_row + bytes].copy_from_slice(&src.data[src_row..src_row + bytes]);
    }
}

#[test]
fn generate_faz6_5_typography_showcase_png() {
    let assets_fonts = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("assets")
        .join("fonts");
    let asset_or = |file: &str, fallbacks: &[&str]| -> Option<String> {
        let p = assets_fonts.join(file);
        if p.is_file() {
            Some(p.to_string_lossy().into_owned())
        } else {
            first_existing_font(fallbacks)
        }
    };

    let Some(display_font) = asset_or(
        "Anton-Regular.ttf",
        &[r"C:\Windows\Fonts\impact.ttf", r"C:\Windows\Fonts\arialbd.ttf"],
    ) else {
        return;
    };
    let Some(vf_font) = asset_or(
        "RobotoFlex-VF.ttf",
        &[r"C:\Windows\Fonts\bahnschrift.ttf", r"C:\Windows\Fonts\arial.ttf"],
    ) else {
        return;
    };
    let Some(serif_font) = asset_or(
        "InstrumentSerif-Regular.ttf",
        &[r"C:\Windows\Fonts\georgia.ttf", r"C:\Windows\Fonts\arial.ttf"],
    ) else {
        return;
    };
    let Some(jp_tategaki_font) = asset_or(
        "ShipporiMincho-Bold.ttf",
        &[r"C:\Windows\Fonts\YuGothB.ttc", r"C:\Windows\Fonts\msgothic.ttc"],
    ) else {
        return;
    };
    let Some(jp_anime_font) = asset_or(
        "DelaGothicOne-Regular.ttf",
        &[r"C:\Windows\Fonts\YuGothB.ttc", r"C:\Windows\Fonts\msgothic.ttc"],
    ) else {
        return;
    };
    let Some(wide_display_font) = asset_or(
        "Unbounded-VF.ttf",
        &[r"C:\Windows\Fonts\impact.ttf", r"C:\Windows\Fonts\arialbd.ttf"],
    ) else {
        return;
    };

    let pw = 640u32;
    let ph = 360u32;
    let mut engine = Engine::new_headless().expect("engine");

    let mut base_project = Project::golden_scene_preset();
    base_project.param_bindings.clear();
    for clip in &mut base_project.timeline.clips {
        clip.event_animations.clear();
    }
    base_project.render_profiles.draft.resolution = Resolution {
        width: pw,
        height: ph,
    };

    // Panel 1 (Top-Left): WGSL Slug + HDR Gradient + Double Anime Stroke + Knockout Gap + Glow + Shadow
    let mut p1 = base_project.clone();
    p1.graph = RenderGraphDef {
        output_node: NodeId::from("bloom_out"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("bg"),
                kind: NodeKind::Solid {
                    color_rgba: [0.025, 0.02, 0.06, 1.0],
                    color_space: ColorSpace::LinearRec709,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("slug_main"),
                kind: NodeKind::VectorText {
                    text: "SLUG GPU\nANIME FX".into(),
                    font_size_norm: 0.23,
                    color_rgba: [1.0, 0.98, 0.92, 1.0],
                    outline_rgba: [0.03, 0.02, 0.09, 1.0],
                    outline_width_norm: 0.015,
                    align_center: true,
                    y_offset_norm: 0.50,
                    font_family: None,
                    font_path: Some(display_font.clone()),
                    font_weight: 400,
                    align: Some(TextAlign2D::Center),
                    x_offset_norm: None,
                    tracking_em: Some(0.04),
                    line_height_em: Some(1.12),
                    opacity: 1.0,
                    fallback_fonts: Vec::new(),
                    writing_mode: WritingMode2D::HorizontalTb,
                    var_wght: None,
                    var_wdth: None,
                    var_slnt: None,
                    color_bottom_rgba: Some([1.0, 0.14, 0.48, 1.0]),
                    gradient_angle_deg: 8.0,
                    outer_outline_rgba: [0.10, 0.92, 1.0, 1.0],
                    outer_outline_width_norm: 0.010,
                    outline_gap_norm: 0.008,
                    shadow_offset_norm: [0.018, 0.022],
                    shadow_blur_norm: 0.014,
                    shadow_rgba: [0.01, 0.01, 0.04, 0.9],
                    glow_radius_norm: 0.038,
                    glow_rgba: [0.95, 0.18, 0.68, 0.65],
                    hdr_intensity: 1.35,
                    gpu_slug: true,
                    text_animator: None,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("comp"),
                kind: NodeKind::Blend {
                    mode: BlendMode::Normal,
                    opacity: 1.0,
                },
                inputs: vec![NodeId::from("bg"), NodeId::from("slug_main")],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("bloom_out"),
                kind: NodeKind::DeepGlow {
                    threshold: 0.9,
                    knee: 0.35,
                    intensity: 0.55,
                    radius: 1.4,
                    mip_levels: 4,
                    tint_rgb: [1.0, 0.65, 0.9],
                    scatter: 0.65,
                    anamorphic_ratio: 0.0,
                    chromatic_shift: 0.0,
                },
                inputs: vec![NodeId::from("comp")],
                enabled: true,
            },
        ],
    };
    let f1 = engine
        .render_frame_at(&p1, RenderProfileKind::Draft, Flicks::ZERO)
        .expect("panel 1");

    // Panel 2 (Top-Right): Variable Font wght=300 vs wght=750 + High-Contrast Editorial Serif
    let mut p2 = base_project.clone();
    p2.graph = RenderGraphDef {
        output_node: NodeId::from("comp_serif"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("bg"),
                kind: NodeKind::Solid {
                    color_rgba: [0.04, 0.04, 0.05, 1.0],
                    color_space: ColorSpace::LinearRec709,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("vf_light"),
                kind: NodeKind::VectorText {
                    text: "VARIABLE LIGHT 300".into(),
                    font_size_norm: 0.14,
                    color_rgba: [0.65, 0.85, 1.0, 1.0],
                    outline_rgba: [0.0; 4],
                    outline_width_norm: 0.0,
                    align_center: true,
                    y_offset_norm: 0.26,
                    font_family: None,
                    font_path: Some(vf_font.clone()),
                    font_weight: 400,
                    align: Some(TextAlign2D::Center),
                    x_offset_norm: None,
                    tracking_em: Some(0.06),
                    line_height_em: None,
                    opacity: 1.0,
                    fallback_fonts: Vec::new(),
                    writing_mode: WritingMode2D::HorizontalTb,
                    var_wght: Some(300.0),
                    var_wdth: Some(85.0),
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
                    hdr_intensity: 1.05,
                    gpu_slug: true,
                    text_animator: None,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("vf_bold"),
                kind: NodeKind::VectorText {
                    text: "VARIABLE BOLD 700".into(),
                    font_size_norm: 0.15,
                    color_rgba: [1.0, 0.92, 0.65, 1.0],
                    outline_rgba: [0.1, 0.05, 0.02, 1.0],
                    outline_width_norm: 0.008,
                    align_center: true,
                    y_offset_norm: 0.50,
                    font_family: None,
                    font_path: Some(vf_font),
                    font_weight: 400,
                    align: Some(TextAlign2D::Center),
                    x_offset_norm: None,
                    tracking_em: Some(0.02),
                    line_height_em: None,
                    opacity: 1.0,
                    fallback_fonts: Vec::new(),
                    writing_mode: WritingMode2D::HorizontalTb,
                    var_wght: Some(700.0),
                    var_wdth: Some(100.0),
                    var_slnt: None,
                    color_bottom_rgba: Some([1.0, 0.45, 0.12, 1.0]),
                    gradient_angle_deg: 0.0,
                    outer_outline_rgba: [0.0; 4],
                    outer_outline_width_norm: 0.0,
                    outline_gap_norm: 0.0,
                    shadow_offset_norm: [0.0, 0.0],
                    shadow_blur_norm: 0.0,
                    shadow_rgba: [0.0; 4],
                    glow_radius_norm: 0.0,
                    glow_rgba: [0.0; 4],
                    hdr_intensity: 1.15,
                    gpu_slug: true,
                    text_animator: None,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("serif_gpos"),
                kind: NodeKind::VectorText {
                    text: "Editorial AVATAR fi".into(),
                    font_size_norm: 0.15,
                    color_rgba: [0.98, 0.96, 0.94, 1.0],
                    outline_rgba: [0.0; 4],
                    outline_width_norm: 0.0,
                    align_center: true,
                    y_offset_norm: 0.76,
                    font_family: None,
                    font_path: Some(serif_font),
                    font_weight: 400,
                    align: Some(TextAlign2D::Center),
                    x_offset_norm: None,
                    tracking_em: None,
                    line_height_em: None,
                    opacity: 1.0,
                    fallback_fonts: Vec::new(),
                    writing_mode: WritingMode2D::HorizontalTb,
                    var_wght: None,
                    var_wdth: None,
                    var_slnt: None,
                    color_bottom_rgba: Some([0.78, 0.72, 0.88, 1.0]),
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
            },
            GraphNodeDef {
                id: NodeId::from("c1"),
                kind: NodeKind::Blend {
                    mode: BlendMode::Normal,
                    opacity: 1.0,
                },
                inputs: vec![NodeId::from("bg"), NodeId::from("vf_light")],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("c2"),
                kind: NodeKind::Blend {
                    mode: BlendMode::Normal,
                    opacity: 1.0,
                },
                inputs: vec![NodeId::from("c1"), NodeId::from("vf_bold")],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("comp_serif"),
                kind: NodeKind::Blend {
                    mode: BlendMode::Normal,
                    opacity: 1.0,
                },
                inputs: vec![NodeId::from("c2"), NodeId::from("serif_gpos")],
                enabled: true,
            },
        ],
    };
    let f2 = engine
        .render_frame_at(&p2, RenderProfileKind::Draft, Flicks::ZERO)
        .expect("panel 2");

    // Panel 3 (Bottom-Left): Vertical Japanese (Tategaki) + Latin/Kanji Fallback Chain
    let mut p3 = base_project.clone();
    p3.graph = RenderGraphDef {
        output_node: NodeId::from("comp_mix"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("bg"),
                kind: NodeKind::Solid {
                    color_rgba: [0.015, 0.035, 0.06, 1.0],
                    color_space: ColorSpace::LinearRec709,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("tategaki"),
                kind: NodeKind::VectorText {
                    text: "新世紀\n東京編".into(),
                    font_size_norm: 0.20,
                    color_rgba: [1.0, 0.96, 0.94, 1.0],
                    outline_rgba: [0.04, 0.06, 0.14, 1.0],
                    outline_width_norm: 0.012,
                    align_center: true,
                    y_offset_norm: 0.48,
                    font_family: None,
                    font_path: Some(jp_tategaki_font),
                    font_weight: 400,
                    align: Some(TextAlign2D::Center),
                    x_offset_norm: Some(0.72),
                    tracking_em: Some(0.05),
                    line_height_em: Some(1.35),
                    opacity: 1.0,
                    fallback_fonts: Vec::new(),
                    writing_mode: WritingMode2D::VerticalRl,
                    var_wght: None,
                    var_wdth: None,
                    var_slnt: None,
                    color_bottom_rgba: Some([0.2, 0.88, 1.0, 1.0]),
                    gradient_angle_deg: 0.0,
                    outer_outline_rgba: [1.0, 0.22, 0.55, 1.0],
                    outer_outline_width_norm: 0.008,
                    outline_gap_norm: 0.004,
                    shadow_offset_norm: [0.0, 0.0],
                    shadow_blur_norm: 0.0,
                    shadow_rgba: [0.0; 4],
                    glow_radius_norm: 0.025,
                    glow_rgba: [0.2, 0.8, 1.0, 0.5],
                    hdr_intensity: 1.25,
                    gpu_slug: true,
                    text_animator: None,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("mixed_fb"),
                kind: NodeKind::VectorText {
                    text: "FALLBACK\nSHIFT 東京".into(),
                    font_size_norm: 0.11,
                    color_rgba: [1.0, 0.86, 0.3, 1.0],
                    outline_rgba: [0.08, 0.03, 0.02, 1.0],
                    outline_width_norm: 0.009,
                    align_center: false,
                    y_offset_norm: 0.50,
                    font_family: None,
                    font_path: Some(wide_display_font),
                    font_weight: 700,
                    align: Some(TextAlign2D::Left),
                    x_offset_norm: Some(0.06),
                    tracking_em: Some(0.02),
                    line_height_em: Some(1.25),
                    opacity: 1.0,
                    fallback_fonts: vec![jp_anime_font],
                    writing_mode: WritingMode2D::HorizontalTb,
                    var_wght: Some(800.0),
                    var_wdth: None,
                    var_slnt: None,
                    color_bottom_rgba: Some([1.0, 0.3, 0.15, 1.0]),
                    gradient_angle_deg: 0.0,
                    outer_outline_rgba: [0.0; 4],
                    outer_outline_width_norm: 0.0,
                    outline_gap_norm: 0.0,
                    shadow_offset_norm: [0.0, 0.0],
                    shadow_blur_norm: 0.0,
                    shadow_rgba: [0.0; 4],
                    glow_radius_norm: 0.0,
                    glow_rgba: [0.0; 4],
                    hdr_intensity: 1.15,
                    gpu_slug: true,
                    text_animator: None,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("c_tat"),
                kind: NodeKind::Blend {
                    mode: BlendMode::Normal,
                    opacity: 1.0,
                },
                inputs: vec![NodeId::from("bg"), NodeId::from("tategaki")],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("comp_mix"),
                kind: NodeKind::Blend {
                    mode: BlendMode::Normal,
                    opacity: 1.0,
                },
                inputs: vec![NodeId::from("c_tat"), NodeId::from("mixed_fb")],
                enabled: true,
            },
        ],
    };
    let f3 = engine
        .render_frame_at(&p3, RenderProfileKind::Draft, Flicks::ZERO)
        .expect("panel 3");

    // Panel 4 (Bottom-Right): 3D Multi-line Text3D + Round Curved Bevel + Iridescent Chrome (mode=3)
    let mut p4 = base_project.clone();
    p4.graph = RenderGraphDef {
        output_node: NodeId::from("bloom3d"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("bg"),
                kind: NodeKind::Solid {
                    color_rgba: [0.02, 0.015, 0.045, 1.0],
                    color_space: ColorSpace::LinearRec709,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("t3d"),
                kind: NodeKind::Text3D {
                    text: "CHROME\nBEVEL 3D".into(),
                    font_path: display_font,
                    font_size: 0.42,
                    extrude_depth: 0.22,
                    bevel_size: 0.032,
                    color_top_rgba: [0.95, 0.98, 1.0, 1.0],
                    color_bottom_rgba: [0.35, 0.62, 1.0, 1.0],
                    side_color_rgba: [0.18, 0.14, 0.36, 1.0],
                    pos: [0.0, 0.0, 0.0],
                    rot_deg: [12.0, -22.0, 0.0],
                    camera_pos: [0.0, 0.0, 2.35],
                    camera_rot_deg: [0.0, 0.0, 0.0],
                    fov_deg: 50.0,
                    char_stagger_sec: 0.0,
                    char_bounce_amp: 0.0,
                    char_wave_freq: 0.0,
                    time_offset_sec: 0.0,
                    outline_width: 0.016,
                    outline_color_rgba: [0.1, 0.9, 1.0, 0.7],
                    material_mode: 3,
                    color_mode: 0,
                    palette_rgba: Vec::new(),
                    pop_duration_sec: 0.0,
                    pop_overshoot: 2.15,
                    spin_in_deg: 0.0,
                    char_jitter_deg: 0.0,
                    arc_curvature: 0.14,
                    arc_axis: 0,
                    echo_count: 3,
                    echo_offset: [0.0, 0.0, -0.22],
                    echo_alpha_decay: 0.52,
                    echo_scale_step: 0.95,
                    hdr_intensity: 1.35,
                    opacity: 1.0,
                    align: TextAlign2D::Center,
                    line_height_em: Some(1.1),
                    tracking_em: 0.03,
                    bevel_profile: BevelProfile3D::Round,
                    bevel_segments: 6,
                    fallback_fonts: Vec::new(),
                    var_wght: None,
                    var_wdth: None,
                    var_slnt: None,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("comp"),
                kind: NodeKind::Blend {
                    mode: BlendMode::Normal,
                    opacity: 1.0,
                },
                inputs: vec![NodeId::from("bg"), NodeId::from("t3d")],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("bloom3d"),
                kind: NodeKind::DeepGlow {
                    threshold: 0.85,
                    knee: 0.35,
                    intensity: 0.6,
                    radius: 1.3,
                    mip_levels: 4,
                    tint_rgb: [0.6, 0.85, 1.0],
                    scatter: 0.65,
                    anamorphic_ratio: 0.0,
                    chromatic_shift: 0.0,
                },
                inputs: vec![NodeId::from("comp")],
                enabled: true,
            },
        ],
    };
    let f4 = engine
        .render_frame_at(&p4, RenderProfileKind::Draft, Flicks::ZERO)
        .expect("panel 4");

    let full_w = pw * 2;
    let full_h = ph * 2;
    let mut canvas = vec![0u8; (full_w * full_h * 4) as usize];
    blit_panel(&mut canvas, full_w, &f1, 0, 0);
    blit_panel(&mut canvas, full_w, &f2, pw, 0);
    blit_panel(&mut canvas, full_w, &f3, 0, ph);
    blit_panel(&mut canvas, full_w, &f4, pw, ph);

    let out_frame = Rgba8Frame {
        width: full_w,
        height: full_h,
        pts: Flicks::ZERO,
        alpha_mode: AlphaMode::Straight,
        data: canvas,
    };
    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target")
        .join("verify_65");
    std::fs::create_dir_all(&out_dir).expect("create target/verify_65");
    let png_path = out_dir.join("typography_showcase_65.png");
    out_frame.save_png(&png_path).expect("write showcase PNG");
    assert!(png_path.is_file());
}
