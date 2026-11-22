use std::path::Path;

use frameshift_media::{AlphaMode, Rgba16fFrame, Rgba8Frame};
use frameshift_schema::{CubicBezierSegmentDef, NodeKind, Project, TextAlign2D};
use frameshift_time::Flicks;
use frameshift_vector::{
    draw_vector_text, rasterize_ttf_text_rgba16f, rasterize_vector_text_node,
    rasterize_vector_text_rgba16f, render_anime_title_card, render_label_badge,
    render_vector_path_hud_node_rgba16f, render_vector_path_hud_rgba16f,
    render_vector_text_node_rgba16f, render_vector_text_rgba16f, render_vector_text_rgba8,
    AnimeTitleCardStyle, CubicBezierHudSegment, FontQuery, SystemFontIndex, TtfTextAlign,
    TtfTextConfig, VectorCanvas, VectorError, VectorPathHudConfig, VectorTextConfig,
};

fn alpha_bbox_rgba8(frame: &Rgba8Frame) -> Option<(u32, u32, u32, u32)> {
    let mut min_x = u32::MAX;
    let mut min_y = u32::MAX;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    let mut any = false;

    for y in 0..frame.height {
        for x in 0..frame.width {
            let px = frame.get_pixel(x, y).unwrap();
            if px[3] > 0 {
                any = true;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }

    if any {
        Some((min_x, min_y, max_x, max_y))
    } else {
        None
    }
}

#[test]
fn golden_vector_text_deterministic_and_outlined() {
    let f1 = rasterize_vector_text_node(
        640,
        360,
        "FRAMESHIFT // IMPACT",
        0.065,
        [1.0, 0.98, 0.95, 1.0],
        [0.06, 0.04, 0.12, 0.95],
        0.004,
        true,
        0.78,
    );
    let f2 = rasterize_vector_text_node(
        640,
        360,
        "FRAMESHIFT // IMPACT",
        0.065,
        [1.0, 0.98, 0.95, 1.0],
        [0.06, 0.04, 0.12, 0.95],
        0.004,
        true,
        0.78,
    );

    assert_eq!(f1.alpha_mode, AlphaMode::Straight);
    assert_eq!(f1.fnv1a_hash64(), f2.fnv1a_hash64());
    assert_eq!(f1.sha256(), f2.sha256());

    let r16_a = rasterize_vector_text_rgba16f(
        640,
        360,
        "FRAMESHIFT // IMPACT",
        0.065,
        [1.0, 0.98, 0.95, 1.0],
        [0.06, 0.04, 0.12, 0.95],
        0.004,
        true,
        0.78,
    );
    let r16_b = rasterize_vector_text_rgba16f(
        640,
        360,
        "FRAMESHIFT // IMPACT",
        0.065,
        [1.0, 0.98, 0.95, 1.0],
        [0.06, 0.04, 0.12, 0.95],
        0.004,
        true,
        0.78,
    );
    assert_eq!(r16_a.alpha_mode, AlphaMode::Premultiplied);
    assert_eq!(r16_a.fnv1a_hash64(), r16_b.fnv1a_hash64());

    let mut core_pixels = 0usize;
    let mut outline_pixels = 0usize;
    let mut outline_adjacent_to_core = 0usize;

    for y in 1..(f1.height - 1) {
        for x in 1..(f1.width - 1) {
            let px = f1.get_pixel(x, y).unwrap();
            if px[3] > 200 && px[0] > 230 && px[1] > 220 && px[2] > 210 {
                core_pixels += 1;
            } else if px[3] > 100 && px[0] < 130 && px[2] > px[0] {
                outline_pixels += 1;
                let has_core_neighbor = [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)]
                    .into_iter()
                    .any(|(nx, ny)| {
                        let np = f1.get_pixel(nx, ny).unwrap();
                        np[3] > 200 && np[0] > 200
                    });
                if has_core_neighbor {
                    outline_adjacent_to_core += 1;
                }
            }
        }
    }

    assert!(core_pixels > 400, "expected core glyph pixels, got {core_pixels}");
    assert!(
        outline_pixels > 200,
        "expected outline pixels, got {outline_pixels}"
    );
    assert!(
        outline_adjacent_to_core > 100,
        "expected outline pixels directly surrounding core pixels, got {outline_adjacent_to_core}"
    );
}

#[test]
fn vector_text_alignment_and_resolution_invariance() {
    let centered = rasterize_vector_text_node(
        640,
        360,
        "FRAMESHIFT // IMPACT",
        0.065,
        [1.0, 0.98, 0.95, 1.0],
        [0.06, 0.04, 0.12, 0.95],
        0.004,
        true,
        0.78,
    );
    let left_aligned = rasterize_vector_text_node(
        640,
        360,
        "FRAMESHIFT // IMPACT",
        0.065,
        [1.0, 0.98, 0.95, 1.0],
        [0.06, 0.04, 0.12, 0.95],
        0.004,
        false,
        0.78,
    );

    let c_box = alpha_bbox_rgba8(&centered).expect("centered text must render");
    let l_box = alpha_bbox_rgba8(&left_aligned).expect("left-aligned text must render");

    let c_mid_x = (c_box.0 + c_box.2) as f32 * 0.5;
    assert!(
        (c_mid_x - 320.0).abs() <= 4.0,
        "centered text midpoint {c_mid_x} should be near 320"
    );
    assert!(
        l_box.0 < c_box.0 && l_box.0 <= 32,
        "left-aligned text start {} should be near left margin and left of centered {}",
        l_box.0,
        c_box.0
    );

    // Resolution scaling invariance: 720p draft vs 1080p final (1.5x scale).
    let draft = rasterize_vector_text_node(
        1280,
        720,
        "FRAMESHIFT // IMPACT",
        0.065,
        [1.0, 0.98, 0.95, 1.0],
        [0.06, 0.04, 0.12, 0.95],
        0.004,
        true,
        0.78,
    );
    let final_frame = rasterize_vector_text_node(
        1920,
        1080,
        "FRAMESHIFT // IMPACT",
        0.065,
        [1.0, 0.98, 0.95, 1.0],
        [0.06, 0.04, 0.12, 0.95],
        0.004,
        true,
        0.78,
    );

    let d_box = alpha_bbox_rgba8(&draft).unwrap();
    let f_box = alpha_bbox_rgba8(&final_frame).unwrap();

    let d_center_y_norm = ((d_box.1 + d_box.3) as f32 * 0.5) / 720.0;
    let f_center_y_norm = ((f_box.1 + f_box.3) as f32 * 0.5) / 1080.0;
    assert!(
        (d_center_y_norm - 0.78).abs() < 0.02 && (f_center_y_norm - 0.78).abs() < 0.02,
        "vertical center norm mismatch: draft={d_center_y_norm}, final={f_center_y_norm}"
    );
    assert!(
        (d_center_y_norm - f_center_y_norm).abs() < 0.005,
        "resolution scaling broke vertical invariance: {d_center_y_norm} vs {f_center_y_norm}"
    );

    let d_w_norm = (d_box.2 - d_box.0) as f32 / 1280.0;
    let f_w_norm = (f_box.2 - f_box.0) as f32 / 1920.0;
    assert!(
        (d_w_norm - f_w_norm).abs() < 0.01,
        "resolution scaling broke horizontal width norm: {d_w_norm} vs {f_w_norm}"
    );
}

#[test]
fn golden_preset_node_conversion_and_all_ascii_glyphs() {
    let project = Project::golden_scene_preset();
    let text_node = project
        .graph
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::VectorText { .. }))
        .expect("golden preset must include VectorText node");

    let cfg = VectorTextConfig::from_node_kind(&text_node.kind).unwrap();
    let frame16 = render_vector_text_rgba16f(&cfg, 640, 360, Flicks::ZERO, AlphaMode::Premultiplied)
        .unwrap();
    assert_eq!(frame16.alpha_mode, AlphaMode::Premultiplied);
    assert!(frame16.data.iter().any(|px| px[3] > 0.9));

    // Verify every printable ASCII glyph renders cleanly and non-spaces have ink.
    for code in 0x20u8..=0x7Eu8 {
        let ch = code as char;
        let s = ch.to_string();
        let frame = rasterize_vector_text_node(
            64,
            64,
            &s,
            0.4,
            [1.0, 1.0, 1.0, 1.0],
            [0.0, 0.0, 0.0, 0.0],
            0.0,
            true,
            0.5,
        );
        let has_ink = frame.data.as_chunks::<4>().0.iter().any(|px| px[3] > 0);
        if ch == ' ' {
            assert!(!has_ink, "space must produce empty coverage");
        } else {
            assert!(has_ink, "printable char '{ch}' (0x{code:02x}) produced no ink");
        }
    }

    // Empty string must produce valid transparent output.
    let empty = rasterize_vector_text_node(
        64,
        64,
        "",
        0.1,
        [1.0, 1.0, 1.0, 1.0],
        [0.0, 0.0, 0.0, 0.0],
        0.0,
        true,
        0.5,
    );
    assert!(empty.data.iter().all(|&b| b == 0));
}

#[test]
fn vector_primitives_rounded_rect_ring_bezier_polygon() {
    let mut canvas = VectorCanvas::new(256, 256).unwrap();

    // Rounded rect fill + stroke
    canvas.fill_rounded_rect(20.0, 20.0, 90.0, 60.0, 14.0, [0.2, 0.6, 1.0, 1.0]);
    canvas.stroke_rounded_rect(20.0, 20.0, 90.0, 60.0, 14.0, 3.0, [1.0, 1.0, 1.0, 1.0]);
    // Interior center should be fill blue, corner exterior transparent.
    let center = canvas.get_pixel(65, 50).unwrap();
    assert!(center[2] > 0.9 && center[3] > 0.99);
    let outside_corner = canvas.get_pixel(21, 21).unwrap();
    assert!(outside_corner[3] < 0.1, "rounded corner should trim (21,21)");

    // Ring: center hollow, radius 40 stroked with width 6
    let mut ring_canvas = VectorCanvas::new(200, 200).unwrap();
    ring_canvas.stroke_ring(100.0, 100.0, 40.0, 6.0, [1.0, 0.3, 0.1, 1.0]);
    assert!(ring_canvas.get_pixel(100, 100).unwrap()[3] < 1e-4);
    assert!(ring_canvas.get_pixel(140, 100).unwrap()[3] > 0.95);
    assert!(ring_canvas.get_pixel(155, 100).unwrap()[3] < 1e-4);

    // Cubic Bézier curve
    let mut bez_canvas = VectorCanvas::new(200, 200).unwrap();
    bez_canvas.draw_cubic_bezier_aa(
        [20.0, 160.0],
        [60.0, 20.0],
        [140.0, 180.0],
        [180.0, 40.0],
        4.0,
        [0.1, 0.95, 0.4, 1.0],
    );
    assert!(bez_canvas.get_pixel(20, 160).unwrap()[3] > 0.9);
    assert!(bez_canvas.get_pixel(180, 40).unwrap()[3] > 0.9);
    let mid_curve_pixels = bez_canvas
        .pixels()
        .iter()
        .filter(|px| px[3] > 0.0 && px[3] < 1.0)
        .count();
    assert!(mid_curve_pixels > 50, "expected anti-aliased edge pixels on Bézier");

    // 4x4 supersampled polygon (diamond)
    let mut poly_canvas = VectorCanvas::new(128, 128).unwrap();
    poly_canvas.fill_polygon(
        &[[64.0, 16.0], [112.0, 64.0], [64.0, 112.0], [16.0, 64.0]],
        [0.9, 0.2, 0.8, 1.0],
    );
    assert!((poly_canvas.get_pixel(64, 64).unwrap()[3] - 1.0).abs() < 1e-5);
    assert!(poly_canvas.get_pixel(10, 10).unwrap()[3] < 1e-5);
    let aa_edges = poly_canvas
        .pixels()
        .iter()
        .filter(|px| px[3] > 0.0 && px[3] < 1.0)
        .count();
    assert!(aa_edges > 20, "expected 4x4 supersampled polygon edges");

    // Checkerboard + straight vs premultiplied export + rect/circle/line + roundtrip
    let mut chk = VectorCanvas::new(64, 64).unwrap();
    chk.draw_checkerboard(16, [0.2, 0.2, 0.2, 1.0], [0.5, 0.5, 0.5, 1.0]);
    assert_ne!(chk.get_pixel(0, 0), chk.get_pixel(16, 0));

    let mut basic = VectorCanvas::new(128, 128).unwrap();
    basic.fill_rect(10.0, 10.0, 30.0, 20.0, [0.8, 0.2, 0.2, 0.5]);
    basic.fill_circle(80.0, 80.0, 18.0, [0.2, 0.8, 0.3, 1.0]);
    basic.draw_line_aa(10.0, 100.0, 110.0, 100.0, 3.0, [1.0, 1.0, 1.0, 1.0]);
    assert!((basic.get_pixel(25, 20).unwrap()[3] - 0.5).abs() < 1e-3);
    assert!(basic.get_pixel(80, 80).unwrap()[3] > 0.99);
    assert!(basic.get_pixel(60, 100).unwrap()[3] > 0.99);

    let f16_straight = basic
        .to_rgba16f_frame(Flicks::ZERO, AlphaMode::Straight)
        .unwrap();
    assert_eq!(f16_straight.alpha_mode, AlphaMode::Straight);
    let idx_rect = 20 * 128 + 25;
    assert!((f16_straight.data[idx_rect][0] - 0.8).abs() < 1e-3);

    let f8_straight = poly_canvas
        .to_rgba8_frame(Flicks::ZERO, AlphaMode::Straight)
        .unwrap();
    let f8_premul = poly_canvas
        .to_rgba8_frame(Flicks::ZERO, AlphaMode::Premultiplied)
        .unwrap();
    assert_eq!(f8_straight.alpha_mode, AlphaMode::Straight);
    assert_eq!(f8_premul.alpha_mode, AlphaMode::Premultiplied);
    let roundtrip = VectorCanvas::from_rgba8_frame(&f8_straight).unwrap();
    assert!(roundtrip.get_pixel(64, 64).unwrap()[3] > 0.99);
}

#[test]
fn label_badge_and_anime_title_card_and_shadow_glow() {
    let mut frame = Rgba8Frame::filled(320, 120, Flicks::ZERO, [20, 24, 32, 255], AlphaMode::Straight)
        .unwrap();
    let before = frame.fnv1a_hash64();

    render_label_badge(
        &mut frame,
        12,
        10,
        "T=00:02.500 | REV#7 [M2]",
        [255, 255, 255, 255],
        [18, 18, 28, 220],
    );
    assert_ne!(frame.fnv1a_hash64(), before);

    // Check foreground text pixels exist inside badge bounds and background was composited.
    let mut bright_fg = 0usize;
    for y in 10..26 {
        for x in 12..190 {
            let px = frame.get_pixel(x, y).unwrap();
            if px[0] > 230 && px[1] > 230 && px[2] > 230 {
                bright_fg += 1;
            }
        }
    }
    assert!(bright_fg > 40, "expected crisp foreground text pixels inside badge, got {bright_fg}");

    // Edge-clipping badge must not panic.
    render_label_badge(&mut frame, 300, 112, "CLIP_TEST", [255, 255, 0, 255], [0, 0, 0, 255]);

    // Anime title card + shadow + glow + multi-line
    let mut card_canvas = VectorCanvas::new(640, 360).unwrap();
    let cfg = VectorTextConfig {
        text: "SCENE 02\nIMPACT // DROP".into(),
        font_size_norm: 0.06,
        color_rgba: [1.0, 0.98, 0.95, 1.0],
        outline_rgba: [0.05, 0.03, 0.1, 0.95],
        outline_width_norm: 0.004,
        align_center: true,
        y_offset_norm: 0.5,
        shadow_offset_norm: [0.006, 0.008],
        shadow_blur_norm: 0.006,
        shadow_rgba: [0.0, 0.0, 0.0, 0.7],
        glow_radius_norm: 0.012,
        glow_rgba: [0.95, 0.25, 0.55, 0.45],
        ..VectorTextConfig::default()
    };
    render_anime_title_card(&mut card_canvas, &cfg, &AnimeTitleCardStyle::default()).unwrap();
    let card_out = render_vector_text_rgba8(&cfg, 640, 360, Flicks::ZERO, AlphaMode::Straight).unwrap();
    assert!(alpha_bbox_rgba8(&card_out).is_some());

    // Direct draw_vector_text also succeeds on non-empty canvas.
    draw_vector_text(&mut card_canvas, &cfg).unwrap();
}

fn alpha_bbox_rgba16f(frame: &Rgba16fFrame) -> Option<(u32, u32, u32, u32)> {
    let mut min_x = u32::MAX;
    let mut min_y = u32::MAX;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    let mut any = false;

    for y in 0..frame.height {
        for x in 0..frame.width {
            let idx = (y as usize) * (frame.width as usize) + (x as usize);
            if frame.data[idx][3] > 1e-3 {
                any = true;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }

    if any {
        Some((min_x, min_y, max_x, max_y))
    } else {
        None
    }
}

fn find_system_ttf_for_test() -> Option<String> {
    let candidates = [
        r"C:\Windows\Fonts\arial.ttf",
        r"C:\Windows\Fonts\impact.ttf",
        r"C:\Windows\Fonts\arialbd.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/System/Library/Fonts/Helvetica.ttc",
    ];
    for path in candidates {
        if Path::new(path).is_file() {
            return Some(path.to_string());
        }
    }
    None
}

#[test]
fn legacy_golden_vector_text_bit_exact_and_analytical_right_align_opacity() {
    let project = Project::golden_scene_preset();
    let text_node = project
        .graph
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::VectorText { .. }))
        .expect("golden preset must include VectorText node");

    let legacy_direct = rasterize_vector_text_rgba16f(
        640,
        360,
        "FRAMESHIFT // IMPACT",
        0.065,
        [1.0, 0.98, 0.95, 1.0],
        [0.06, 0.04, 0.12, 0.95],
        0.004,
        true,
        0.78,
    );
    let via_node_helper =
        render_vector_text_node_rgba16f(&text_node.kind, 640, 360, Flicks::ZERO).unwrap();
    let cfg_from_node = VectorTextConfig::from_node_kind(&text_node.kind).unwrap();
    let via_cfg = render_vector_text_rgba16f(
        &cfg_from_node,
        640,
        360,
        Flicks::ZERO,
        AlphaMode::Premultiplied,
    )
    .unwrap();

    assert_eq!(legacy_direct.fnv1a_hash64(), via_node_helper.fnv1a_hash64());
    assert_eq!(legacy_direct.fnv1a_hash64(), via_cfg.fnv1a_hash64());
    assert_eq!(legacy_direct.data, via_node_helper.data);

    // Analytical SDF path supports Right alignment, custom x_offset_norm, and opacity scaling.
    let right_cfg = VectorTextConfig {
        text: "RIGHT".into(),
        font_size_norm: 0.08,
        color_rgba: [1.0, 1.0, 1.0, 1.0],
        align: Some(TextAlign2D::Right),
        y_offset_norm: 0.5,
        ..VectorTextConfig::default()
    };
    let left_cfg = VectorTextConfig {
        align: Some(TextAlign2D::Left),
        ..right_cfg.clone()
    };
    let right_f =
        render_vector_text_rgba16f(&right_cfg, 320, 180, Flicks::ZERO, AlphaMode::Premultiplied)
            .unwrap();
    let left_f =
        render_vector_text_rgba16f(&left_cfg, 320, 180, Flicks::ZERO, AlphaMode::Premultiplied)
            .unwrap();
    let r_box = alpha_bbox_rgba16f(&right_f).unwrap();
    let l_box = alpha_bbox_rgba16f(&left_f).unwrap();
    assert!(
        r_box.0 > 180 && r_box.2 >= 295 && r_box.2 > l_box.2,
        "analytical right align should anchor near right margin: r_box={r_box:?}, l_box={l_box:?}"
    );

    let half_op_cfg = VectorTextConfig {
        opacity: 0.5,
        ..left_cfg
    };
    let half_f =
        render_vector_text_rgba16f(&half_op_cfg, 320, 180, Flicks::ZERO, AlphaMode::Premultiplied)
            .unwrap();
    let max_a_full = left_f.data.iter().map(|p| p[3]).fold(0.0_f32, f32::max);
    let max_a_half = half_f.data.iter().map(|p| p[3]).fold(0.0_f32, f32::max);
    assert!(max_a_full > 0.95);
    assert!(
        (max_a_half - 0.5).abs() < 0.05,
        "expected halved alpha near 0.5, got {max_a_half}"
    );
}

#[test]
fn vector_text_missing_font_path_and_family_return_font_not_found_without_fallback() {
    let missing_path_cfg = VectorTextConfig {
        text: "NO FALLBACK".into(),
        font_path: Some(r"C:\nonexistent_frameshift_fonts\missing_font_999.ttf".into()),
        ..VectorTextConfig::default()
    };
    let err_path = render_vector_text_rgba16f(
        &missing_path_cfg,
        256,
        128,
        Flicks::ZERO,
        AlphaMode::Premultiplied,
    )
    .unwrap_err();
    assert!(
        matches!(err_path, VectorError::FontNotFound(_)),
        "expected FontNotFound for missing font_path, got {err_path:?}"
    );

    let missing_family_cfg = VectorTextConfig {
        text: "NO FALLBACK".into(),
        font_family: Some("FrameshiftNonExistentFontFamilyXYZ_98765".into()),
        font_weight: 700,
        ..VectorTextConfig::default()
    };
    let err_family = render_vector_text_rgba16f(
        &missing_family_cfg,
        256,
        128,
        Flicks::ZERO,
        AlphaMode::Premultiplied,
    )
    .unwrap_err();
    assert!(
        matches!(err_family, VectorError::FontNotFound(_)),
        "expected FontNotFound for missing font_family, got {err_family:?}"
    );

    let node_missing = NodeKind::VectorText {
        text: "TEST".into(),
        font_size_norm: 0.08,
        color_rgba: [1.0, 1.0, 1.0, 1.0],
        outline_rgba: [0.0; 4],
        outline_width_norm: 0.0,
        align_center: true,
        y_offset_norm: 0.5,
        font_family: Some("FrameshiftNonExistentFontFamilyXYZ_98765".into()),
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
        text_animator: None,
    };
    let err_node = render_vector_text_node_rgba16f(&node_missing, 256, 128, Flicks::ZERO).unwrap_err();
    assert!(matches!(err_node, VectorError::FontNotFound(_)));
}

#[test]
fn ttf_vector_text_multiline_alignment_tracking_outline_and_opacity() {
    let resolved_path = find_system_ttf_for_test();
    let family_query = FontQuery {
        font_path: None,
        font_family: Some("Arial".into()),
        font_weight: 400,
    };
    let resolved_via_family = SystemFontIndex::global().resolve(&family_query, None).is_ok();
    if resolved_path.is_none() && !resolved_via_family {
        return;
    }

    let (font_family, font_path) = if resolved_via_family {
        (Some("Arial".to_string()), None)
    } else {
        (None, resolved_path.clone())
    };

    // 1. Left | Center | Right alignment with multi-line `\n`
    let make_align_node = |align: TextAlign2D| NodeKind::VectorText {
        text: "SHIFT\nSYNC".into(),
        font_size_norm: 0.12,
        color_rgba: [1.0, 0.98, 0.92, 1.0],
        outline_rgba: [0.0, 0.0, 0.0, 0.0],
        outline_width_norm: 0.0,
        align_center: true,
        y_offset_norm: 0.5,
        font_family: font_family.clone(),
        font_path: font_path.clone(),
        font_weight: 400,
        align: Some(align),
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
        text_animator: None,
    };

    let left_f =
        render_vector_text_node_rgba16f(&make_align_node(TextAlign2D::Left), 480, 270, Flicks::ZERO)
            .unwrap();
    let center_f = render_vector_text_node_rgba16f(
        &make_align_node(TextAlign2D::Center),
        480,
        270,
        Flicks::ZERO,
    )
    .unwrap();
    let right_f = render_vector_text_node_rgba16f(
        &make_align_node(TextAlign2D::Right),
        480,
        270,
        Flicks::ZERO,
    )
    .unwrap();

    let l_box = alpha_bbox_rgba16f(&left_f).expect("TTF left text should render");
    let c_box = alpha_bbox_rgba16f(&center_f).expect("TTF center text should render");
    let r_box = alpha_bbox_rgba16f(&right_f).expect("TTF right text should render");

    assert!(l_box.0 <= 30 && l_box.0 < c_box.0, "left={l_box:?}, center={c_box:?}");
    let c_mid = (c_box.0 + c_box.2) as f32 * 0.5;
    assert!((c_mid - 240.0).abs() <= 6.0, "center midpoint {c_mid} should be near 240");
    assert!(r_box.2 >= 445 && r_box.2 > c_box.2, "right={r_box:?}, center={c_box:?}");

    // 2. tracking_em & line_height_em effects
    let tight_cfg = VectorTextConfig {
        text: "AB\nCD".into(),
        font_size_norm: 0.14,
        font_family: font_family.clone(),
        font_path: font_path.clone(),
        tracking_em_override: Some(0.0),
        line_height_em_override: Some(1.0),
        ..VectorTextConfig::default()
    };
    let wide_cfg = VectorTextConfig {
        tracking_em_override: Some(0.45),
        line_height_em_override: Some(2.0),
        ..tight_cfg.clone()
    };
    let tight_f =
        render_vector_text_rgba16f(&tight_cfg, 480, 270, Flicks::ZERO, AlphaMode::Premultiplied)
            .unwrap();
    let wide_f =
        render_vector_text_rgba16f(&wide_cfg, 480, 270, Flicks::ZERO, AlphaMode::Premultiplied)
            .unwrap();
    let t_box = alpha_bbox_rgba16f(&tight_f).unwrap();
    let w_box = alpha_bbox_rgba16f(&wide_f).unwrap();
    assert!(
        (w_box.2 - w_box.0) > (t_box.2 - t_box.0) + 10,
        "tracking_em should widen text bbox: tight={t_box:?}, wide={w_box:?}"
    );
    assert!(
        (w_box.3 - w_box.1) > (t_box.3 - t_box.1) + 15,
        "line_height_em should increase multi-line height: tight={t_box:?}, wide={w_box:?}"
    );

    // 3. outline_width_norm + opacity=0.5 + direct rasterize_ttf_text_rgba16f
    let ttf_outlined = TtfTextConfig {
        text: "OBAD".into(),
        font_family,
        font_path,
        font_weight: 400,
        font_size_norm: 0.22,
        color_rgba: [1.0, 0.95, 0.9, 1.0],
        outline_rgba: [0.1, 0.3, 0.95, 1.0],
        outline_width_norm: 0.015,
        align: TtfTextAlign::Center,
        opacity: 1.0,
        ..TtfTextConfig::default()
    };
    let full_outlined = rasterize_ttf_text_rgba16f(
        &ttf_outlined,
        400,
        200,
        Flicks::ZERO,
        AlphaMode::Premultiplied,
        None,
    )
    .unwrap();
    let half_outlined = rasterize_ttf_text_rgba16f(
        &TtfTextConfig {
            opacity: 0.5,
            ..ttf_outlined.clone()
        },
        400,
        200,
        Flicks::ZERO,
        AlphaMode::Premultiplied,
        None,
    )
    .unwrap();

    let outline_blue_pixels = full_outlined
        .data
        .iter()
        .filter(|px| px[3] > 0.5 && px[2] > px[0] * 1.5)
        .count();
    assert!(
        outline_blue_pixels > 80,
        "expected blue outline pixels around TTF glyphs, got {outline_blue_pixels}"
    );

    let max_a_full = full_outlined.data.iter().map(|p| p[3]).fold(0.0_f32, f32::max);
    let max_a_half = half_outlined.data.iter().map(|p| p[3]).fold(0.0_f32, f32::max);
    assert!(max_a_full > 0.95);
    assert!(
        (max_a_half - 0.5).abs() < 0.03,
        "TTF opacity=0.5 should cap premultiplied alpha near 0.5, got {max_a_half}"
    );
}

#[test]
fn vector_path_hud_bezier_trim_dash_dots_and_data_readout() {
    // 1. Solid vs dashed vs trimmed cubic Bézier via VectorPathHudConfig & NodeKind::VectorPathHud
    let solid_cfg = VectorPathHudConfig {
        bezier_segments: vec![CubicBezierHudSegment {
            p0: [0.1, 0.7],
            c1: [0.3, 0.2],
            c2: [0.7, 0.2],
            p1: [0.9, 0.7],
        }],
        stroke_width_norm: 0.012,
        stroke_rgba: [0.1, 0.9, 1.0, 1.0],
        trim_start: 0.0,
        trim_end: 1.0,
        ..VectorPathHudConfig::default()
    };
    let solid_f = render_vector_path_hud_rgba16f(&solid_cfg, 320, 180, Flicks::ZERO).unwrap();

    let dashed_cfg = VectorPathHudConfig {
        dash_len_norm: 0.06,
        gap_len_norm: 0.06,
        dash_offset_norm: 0.01,
        ..solid_cfg.clone()
    };
    let dashed_f = render_vector_path_hud_rgba16f(&dashed_cfg, 320, 180, Flicks::ZERO).unwrap();

    let trimmed_cfg = VectorPathHudConfig {
        trim_start: 0.15,
        trim_end: 0.55,
        ..solid_cfg.clone()
    };
    let trimmed_f = render_vector_path_hud_rgba16f(&trimmed_cfg, 320, 180, Flicks::ZERO).unwrap();

    let count_lit = |f: &Rgba16fFrame| f.data.iter().filter(|p| p[3] > 0.5).count();
    let solid_lit = count_lit(&solid_f);
    let dashed_lit = count_lit(&dashed_f);
    let trimmed_lit = count_lit(&trimmed_f);

    assert!(solid_lit > 200, "expected solid Bézier HUD pixels, got {solid_lit}");
    assert!(
        dashed_lit > 50 && dashed_lit < solid_lit,
        "dashed HUD should have fewer lit pixels than solid: dashed={dashed_lit}, solid={solid_lit}"
    );
    assert!(
        trimmed_lit > 50 && trimmed_lit < solid_lit,
        "trimmed HUD should have fewer lit pixels than full: trimmed={trimmed_lit}, solid={solid_lit}"
    );

    let trim_box = alpha_bbox_rgba16f(&trimmed_f).unwrap();
    let solid_box = alpha_bbox_rgba16f(&solid_f).unwrap();
    assert!(
        trim_box.0 > solid_box.0 && trim_box.2 < solid_box.2,
        "trim_start=0.15..trim_end=0.55 should clip both ends: trim={trim_box:?}, solid={solid_box:?}"
    );

    // 2. Node dots + data_label + data_value + closed fill via NodeKind::VectorPathHud
    let make_hud_node = |dot_radius: f32, val: f32, label: &str, opacity: f32| NodeKind::VectorPathHud {
        points_norm: vec![[0.15, 0.55], [0.5, 0.35], [0.85, 0.55]],
        bezier_segments: vec![CubicBezierSegmentDef {
            p0: [0.15, 0.55],
            c1: [0.3, 0.25],
            c2: [0.7, 0.25],
            p1: [0.85, 0.55],
        }],
        closed: true,
        stroke_width_norm: 0.008,
        stroke_rgba: [0.2, 1.0, 0.7, 1.0],
        fill_rgba: [0.05, 0.25, 0.3, 0.4],
        dash_len_norm: 0.0,
        gap_len_norm: 0.0,
        dash_offset_norm: 0.0,
        trim_start: 0.0,
        trim_end: 1.0,
        node_dot_radius_norm: dot_radius,
        data_value: val,
        data_label: label.to_string(),
        opacity,
    };
    let bare_hud_node = make_hud_node(0.0, 0.0, "", 1.0);
    let bare_node_f =
        render_vector_path_hud_node_rgba16f(&bare_hud_node, 320, 180, Flicks::ZERO).unwrap();

    let annotated_hud_node = make_hud_node(0.03, 88.5, "PWR:", 0.8);
    let annotated_f =
        render_vector_path_hud_node_rgba16f(&annotated_hud_node, 320, 180, Flicks::ZERO).unwrap();

    // Verify endpoints have dot coverage even in annotated HUD, and data_label added pixels above anchor.
    let idx_p0 = (0.55_f32 * 180.0) as usize * 320 + (0.15_f32 * 320.0) as usize;
    assert!(
        annotated_f.data[idx_p0][3] > 0.7,
        "node dot at p0 should be solidly covered"
    );
    let max_alpha_annotated = annotated_f
        .data
        .iter()
        .map(|p| p[3])
        .fold(0.0_f32, f32::max);
    assert!(
        max_alpha_annotated <= 0.81,
        "opacity=0.8 should scale HUD alpha <= 0.8, got {max_alpha_annotated}"
    );
    assert!(
        annotated_f.data.iter().filter(|p| p[3] > 0.1).count()
            > bare_node_f.data.iter().filter(|p| p[3] > 0.1).count(),
        "node dots + data_label should add visible pixels over bare path"
    );
}
