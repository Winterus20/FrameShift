//! Pure-Rust deterministic 2D vector graphics and typography rasterizer for FrameShift.
//!
//! Operates in premultiplied Linear Rec.709 `[f32; 4]` with analytical SDF / 4x4 sub-pixel
//! coverage anti-aliasing and exports straight or premultiplied `Rgba8Frame` and `Rgba16fFrame`.

pub mod canvas;
pub mod error;
pub mod font;
pub mod font_locator;
pub mod hud_path;
pub mod mesh3d;
pub mod shape3d;
pub mod shaper;
pub mod slug;
pub mod text;

pub use canvas::{
    premultiply_rgba, source_over_premul, srgb_u8_to_linear_rgba, unpremultiply_rgba, VectorCanvas,
};
pub use error::VectorError;
pub use font::{
    bitmap_glyph, glyph_sdf, BitmapGlyph, GlyphSdf, GLYPH_BASELINE, GLYPH_CAP_HEIGHT,
    GLYPH_CAP_TOP, GLYPH_CELL_HEIGHT, GLYPH_CELL_WIDTH,
};
pub use font_locator::{
    rasterize_ttf_text_rgba16f, resolve_font, resolve_font_chain, FontQuery, ResolvedFont,
    SystemFontIndex, TtfTextAlign, TtfTextConfig,
};
pub use shaper::{
    evaluate_text_animator, evaluate_text_animator_with_total, shape_text_block,
    GlyphAnimationState, QuadraticCurve, ShapedGlyphOutline, ShapedTextBlock, TextLayoutOptions,
    VariationAxes,
};
pub use slug::{
    build_slug_scene, SlugBandGpu, SlugCurveGpu, SlugGlyphGpu, SlugSceneGpu, SlugVertex,
};
pub use hud_path::{
    draw_vector_path_hud, render_vector_path_hud_rgba16f, CubicBezierHudSegment,
    VectorPathHudConfig,
};
pub use mesh3d::{
    extrude_text_to_mesh, extrude_text_to_mesh_ex, extrude_text_to_mesh_with_bevel,
    extrude_text_to_mesh_with_outline, load_font_bytes, ExtrudedTextMesh, MeshVertex3D,
    Text3DMeshOptions,
};
pub use shape3d::{build_shape3d_mesh, ProceduralShapeKind, Shape3DParams};
pub use text::{
    draw_vector_text, pair_kerning_units, rasterize_vector_text_node,
    rasterize_vector_text_rgba16f, render_anime_title_card, render_label_badge,
    render_label_badge_scaled, render_vector_path_hud_node_rgba16f,
    render_vector_text_node_rgba16f, render_vector_text_rgba16f, render_vector_text_rgba8,
    AnimeTitleCardStyle, VectorTextConfig,
};

/// Returns the crate identifier string.
pub fn crate_name() -> &'static str {
    "frameshift-vector"
}
