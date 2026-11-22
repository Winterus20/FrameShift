use std::path::Path;

use frameshift_schema::{BevelProfile3D, TextAlign2D, WritingMode2D};
use frameshift_vector::{
    build_slug_scene, extrude_text_to_mesh_ex, load_font_bytes, resolve_font,
    resolve_font_chain, shape_text_block, FontQuery, SystemFontIndex, Text3DMeshOptions,
    TextLayoutOptions, VariationAxes, VectorError,
};

fn first_existing_font(candidates: &[&str]) -> Option<String> {
    candidates
        .iter()
        .find(|p| Path::new(p).is_file())
        .map(|s| (*s).to_string())
}

#[test]
fn harfrust_gpos_kerning_and_slug_bands_work_on_system_font() {
    let Some(font_path) = first_existing_font(&[
        r"C:\Windows\Fonts\arial.ttf",
        r"C:\Windows\Fonts\georgia.ttf",
        r"C:\Windows\Fonts\impact.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ]) else {
        return;
    };

    let primary = resolve_font(
        &FontQuery {
            font_path: Some(font_path),
            font_family: None,
            font_weight: 400,
        },
        None,
    )
    .expect("resolve primary font");

    let shaped = shape_text_block(
        "AVATAR fi",
        &primary,
        &[],
        &TextLayoutOptions {
            font_size: 96.0,
            align: TextAlign2D::Center,
            writing_mode: WritingMode2D::HorizontalTb,
            tracking_em: 0.0,
            line_height_em: None,
            variations: VariationAxes::default(),
            enable_ligatures: true,
        },
    )
    .expect("shape text block");

    assert!(!shaped.glyphs.is_empty());
    let slug = build_slug_scene(&shaped);
    assert!(!slug.is_empty(), "SlugSceneGpu must contain vertices and curves");
    assert_eq!(slug.glyphs.len(), shaped.glyphs.len());
    assert!(!slug.hbands.is_empty() && !slug.vbands.is_empty());
    assert!(!slug.band_indices.is_empty());
}

#[test]
fn variable_font_wght_changes_shaped_outline_and_slug_scene() {
    let Some(vf_path) = first_existing_font(&[
        r"C:\Windows\Fonts\bahnschrift.ttf",
        r"C:\Windows\Fonts\segoeuivf.ttf",
    ]) else {
        return;
    };

    let primary = resolve_font(
        &FontQuery {
            font_path: Some(vf_path),
            font_family: None,
            font_weight: 400,
        },
        None,
    )
    .expect("resolve variable font");

    let light = shape_text_block(
        "FRAMESHIFT",
        &primary,
        &[],
        &TextLayoutOptions {
            font_size: 120.0,
            variations: VariationAxes {
                wght: Some(300.0),
                wdth: Some(85.0),
                slnt: None,
            },
            ..TextLayoutOptions::default()
        },
    )
    .expect("shape light variable instance");

    let heavy = shape_text_block(
        "FRAMESHIFT",
        &primary,
        &[],
        &TextLayoutOptions {
            font_size: 120.0,
            variations: VariationAxes {
                wght: Some(700.0),
                wdth: Some(100.0),
                slnt: None,
            },
            ..TextLayoutOptions::default()
        },
    )
    .expect("shape heavy variable instance");

    assert_ne!(
        light.bounds_max[0] - light.bounds_min[0],
        heavy.bounds_max[0] - heavy.bounds_min[0],
        "variable font wght/wdth must alter glyph metrics & bounds"
    );
    let slug_light = build_slug_scene(&light);
    let slug_heavy = build_slug_scene(&heavy);
    assert_ne!(
        slug_light.curves[0].p0_p1, slug_heavy.curves[0].p0_p1,
        "variable font wght must alter Bezier curve coordinates"
    );
}

#[test]
fn vertical_rl_tategaki_flows_right_to_left_and_top_to_bottom() {
    let Some(jp_path) = first_existing_font(&[
        r"C:\Windows\Fonts\YuGothB.ttc",
        r"C:\Windows\Fonts\YuGothR.ttc",
        r"C:\Windows\Fonts\msgothic.ttc",
    ]) else {
        return;
    };

    let primary = resolve_font(
        &FontQuery {
            font_path: Some(jp_path),
            font_family: None,
            font_weight: 400,
        },
        None,
    )
    .expect("resolve japanese font");

    let shaped = shape_text_block(
        "東京\n渋谷",
        &primary,
        &[],
        &TextLayoutOptions {
            font_size: 64.0,
            align: TextAlign2D::Center,
            writing_mode: WritingMode2D::VerticalRl,
            tracking_em: 0.0,
            line_height_em: Some(1.3),
            variations: VariationAxes::default(),
            enable_ligatures: true,
        },
    )
    .expect("shape vertical Japanese");

    assert_eq!(shaped.glyphs.len(), 4);
    // Column 0 ("東京") is to the RIGHT of Column 1 ("渋谷").
    assert!(
        shaped.glyphs[0].x > shaped.glyphs[2].x + 10.0,
        "VerticalRl column 0 ({}) must be to the right of column 1 ({})",
        shaped.glyphs[0].x,
        shaped.glyphs[2].x
    );
    // Within Column 0, '東' (glyph 0) is ABOVE '京' (glyph 1) in Y-up coordinates.
    assert!(
        shaped.glyphs[0].y > shaped.glyphs[1].y + 10.0,
        "glyph 0 ({}) must be above glyph 1 ({}) in Y-up space",
        shaped.glyphs[0].y,
        shaped.glyphs[1].y
    );
}

#[test]
fn fallback_font_chain_selects_kanji_font_and_rejects_missing_fallback() {
    let Some(latin_path) = first_existing_font(&[
        r"C:\Windows\Fonts\impact.ttf",
        r"C:\Windows\Fonts\arial.ttf",
    ]) else {
        return;
    };
    let Some(jp_path) = first_existing_font(&[
        r"C:\Windows\Fonts\YuGothB.ttc",
        r"C:\Windows\Fonts\msgothic.ttc",
    ]) else {
        return;
    };

    let (primary, fallbacks) = resolve_font_chain(
        &FontQuery {
            font_path: Some(latin_path.clone()),
            font_family: None,
            font_weight: 400,
        },
        &[jp_path],
        None,
    )
    .expect("resolve latin + jp fallback chain");

    let shaped = shape_text_block(
        "SHIFT東京",
        &primary,
        &fallbacks,
        &TextLayoutOptions::default(),
    )
    .expect("shape mixed latin + kanji");

    assert!(shaped.glyphs.iter().any(|g| g.font_index == 0));
    assert!(
        shaped.glyphs.iter().any(|g| g.font_index == 1),
        "Kanji characters must route to fallback font_index 1"
    );

    let err = resolve_font_chain(
        &FontQuery {
            font_path: Some(latin_path),
            font_family: None,
            font_weight: 400,
        },
        &["DefinitelyMissingFallbackFont_FrameShift_XYZ".to_string()],
        None,
    )
    .expect_err("missing fallback font must fail with FontNotFound");
    assert!(matches!(err, VectorError::FontNotFound(_)));
}

#[test]
fn text3d_multiline_and_curved_bevel_profiles_preserve_unit_normals() {
    let Some(font_path) = first_existing_font(&[
        r"C:\Windows\Fonts\arialbd.ttf",
        r"C:\Windows\Fonts\arial.ttf",
    ]) else {
        return;
    };
    let bytes = load_font_bytes(Path::new(&font_path)).expect("load font");

    let mut counts = Vec::new();
    for profile in [
        BevelProfile3D::Flat,
        BevelProfile3D::Round,
        BevelProfile3D::Concave,
        BevelProfile3D::Stepped,
    ] {
        let mesh = extrude_text_to_mesh_ex(
            &bytes,
            "FRAME\nSHIFT",
            &Text3DMeshOptions {
                font_size: 48.0,
                extrude_depth: 12.0,
                bevel_size: 2.5,
                bevel_profile: profile,
                bevel_segments: 6,
                outline_width: 0.0,
                align: TextAlign2D::Center,
                tracking_em: 0.04,
                line_height_em: Some(1.15),
                variations: VariationAxes::default(),
                fallback_fonts: &[],
            },
        )
        .expect("extrude multiline 3d mesh");

        assert!(!mesh.vertices.is_empty());
        assert_eq!(mesh.indices.len() % 3, 0);
        for v in &mesh.vertices {
            let nlen =
                (v.normal[0] * v.normal[0] + v.normal[1] * v.normal[1] + v.normal[2] * v.normal[2])
                    .sqrt();
            assert!((nlen - 1.0).abs() < 1e-3, "non-unit normal {nlen} for {profile:?}");
        }
        counts.push((profile, mesh.vertices.len()));
    }

    assert!(
        counts[1].1 > counts[0].1 && counts[2].1 > counts[0].1 && counts[3].1 > counts[0].1,
        "curved/stepped multi-segment bevels must emit more vertices than flat chamfer: {counts:?}"
    );
}

#[test]
fn curated_assets_fonts_pack_resolves_and_shapes_via_system_font_index() {
    let assets_fonts = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("assets")
        .join("fonts");
    if !assets_fonts.is_dir() {
        return;
    }

    let index = SystemFontIndex::build_for_dirs(&[assets_fonts]);
    for family in [
        "Roboto Flex",
        "Recursive",
        "Unbounded",
        "Bricolage Grotesque",
        "Syne",
        "Instrument Serif",
        "Anton",
        "M PLUS 1",
        "Zen Kaku Gothic New",
        "Dela Gothic One",
        "Shippori Mincho",
        "DotGothic16",
        "Yuji Syuku",
    ] {
        let resolved = index
            .resolve(
                &FontQuery {
                    font_path: None,
                    font_family: Some(family.to_string()),
                    font_weight: 400,
                },
                None,
            )
            .unwrap_or_else(|e| panic!("curated SIL OFL family '{family}' must resolve: {e}"));
        let sample = if matches!(
            family,
            "M PLUS 1" | "Zen Kaku Gothic New" | "Dela Gothic One" | "Shippori Mincho" | "DotGothic16" | "Yuji Syuku"
        ) {
            "東京"
        } else {
            "SHIFT"
        };
        let shaped = shape_text_block(sample, &resolved, &[], &TextLayoutOptions::default())
            .unwrap_or_else(|e| panic!("shape '{family}' failed: {e}"));
        assert!(
            !shaped.glyphs.is_empty(),
            "curated family '{family}' produced zero shaped glyphs"
        );
    }
}
