use crate::canvas::{
    premultiply_rgba, sdf_coverage, source_over_premul, VectorCanvas,
};
use crate::error::VectorError;
use crate::font::{
    bitmap_glyph, glyph_sdf, GLYPH_CAP_HEIGHT, GLYPH_CAP_TOP, GLYPH_CELL_HEIGHT,
    GLYPH_CELL_WIDTH,
};
use crate::font_locator::{
    draw_ttf_text_with_fallbacks, rasterize_ttf_text_rgba16f as rasterize_ttf_rgba16f_impl,
    resolve_font_chain, FontQuery, TtfTextAlign, TtfTextConfig,
};
use crate::hud_path::{
    render_vector_path_hud_rgba16f, CubicBezierHudSegment, VectorPathHudConfig,
};
use crate::shaper::VariationAxes;
use frameshift_media::{AlphaMode, Rgba16fFrame, Rgba8Frame};
use frameshift_schema::{NodeKind, TextAlign2D, WritingMode2D};
use frameshift_time::Flicks;
use serde::{Deserialize, Serialize};

const DEFAULT_LEFT_MARGIN_NORM: f32 = 0.04;
const SDF_REGION_MARGIN_UNITS: f32 = 1.5;

/// Full configuration for deterministic vector/SDF typography and anime title cards.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VectorTextConfig {
    pub text: String,
    /// Normalized cap height relative to canvas height (`0.065` -> ~70px at 1080p).
    pub font_size_norm: f32,
    /// Straight Linear Rec.709 core fill RGBA `[0..1]`.
    pub color_rgba: [f32; 4],
    /// Straight Linear Rec.709 radial outline RGBA `[0..1]`.
    #[serde(default)]
    pub outline_rgba: [f32; 4],
    /// Normalized radial outline thickness relative to canvas height.
    #[serde(default)]
    pub outline_width_norm: f32,
    /// When true, centers each line horizontally around `x_offset_norm` (default `0.5`).
    #[serde(default = "default_true")]
    pub align_center: bool,
    /// Normalized vertical center position (`0.0` top .. `1.0` bottom).
    #[serde(default = "default_half")]
    pub y_offset_norm: f32,
    /// Optional normalized horizontal anchor (`0.5` when centered, `0.04` when left-aligned).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x_offset_norm: Option<f32>,
    /// Extra tracking in em (cap-height) units.
    #[serde(default = "default_tracking")]
    pub tracking_em: f32,
    /// Enables deterministic optical pair kerning.
    #[serde(default = "default_true")]
    pub kerning_enabled: bool,
    /// Line spacing multiplier in em (cap-height) units.
    #[serde(default = "default_line_height")]
    pub line_height_em: f32,
    /// Optional system/project font family name (when set, uses TrueType/OpenType rasterizer).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    /// Optional direct path to a `.ttf`/`.otf`/`.ttc` file (overrides `font_family`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_path: Option<String>,
    /// Target OS/2 font weight class (`1..=1000`, default `400`).
    #[serde(default = "default_font_weight")]
    pub font_weight: u16,
    /// Optional explicit horizontal alignment (`left`, `center`, `right`). Overrides `align_center`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<TextAlign2D>,
    /// Optional tracking override in em units (`None` -> `0.05` on analytical SDF, `0.0` on TTF).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tracking_em_override: Option<f32>,
    /// Optional line-height override in em units (`None` -> `1.3` on analytical SDF, font metrics on TTF).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_height_em_override: Option<f32>,
    /// Layer opacity multiplier in `[0.0, 1.0]` (default `1.0`).
    #[serde(default = "default_one")]
    pub opacity: f32,
    /// Drop shadow offset normalized to canvas height `[dx, dy]`.
    #[serde(default)]
    pub shadow_offset_norm: [f32; 2],
    /// Drop shadow softness normalized to canvas height.
    #[serde(default)]
    pub shadow_blur_norm: f32,
    /// Straight Linear Rec.709 drop shadow RGBA.
    #[serde(default)]
    pub shadow_rgba: [f32; 4],
    /// Outer glow radius normalized to canvas height.
    #[serde(default)]
    pub glow_radius_norm: f32,
    /// Straight Linear Rec.709 outer glow RGBA.
    #[serde(default)]
    pub glow_rgba: [f32; 4],
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallback_fonts: Vec<String>,
    #[serde(default)]
    pub writing_mode: WritingMode2D,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub var_wght: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub var_wdth: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub var_slnt: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_bottom_rgba: Option<[f32; 4]>,
    #[serde(default)]
    pub gradient_angle_deg: f32,
    #[serde(default)]
    pub outer_outline_rgba: [f32; 4],
    #[serde(default)]
    pub outer_outline_width_norm: f32,
    #[serde(default)]
    pub outline_gap_norm: f32,
    #[serde(default = "default_one")]
    pub hdr_intensity: f32,
}

impl Default for VectorTextConfig {
    fn default() -> Self {
        Self {
            text: String::new(),
            font_size_norm: 0.065,
            color_rgba: [1.0, 1.0, 1.0, 1.0],
            outline_rgba: [0.0, 0.0, 0.0, 0.0],
            outline_width_norm: 0.0,
            align_center: true,
            y_offset_norm: 0.5,
            x_offset_norm: None,
            tracking_em: default_tracking(),
            kerning_enabled: true,
            line_height_em: default_line_height(),
            font_family: None,
            font_path: None,
            font_weight: default_font_weight(),
            align: None,
            tracking_em_override: None,
            line_height_em_override: None,
            opacity: default_one(),
            shadow_offset_norm: [0.0, 0.0],
            shadow_blur_norm: 0.0,
            shadow_rgba: [0.0, 0.0, 0.0, 0.0],
            glow_radius_norm: 0.0,
            glow_rgba: [0.0, 0.0, 0.0, 0.0],
            fallback_fonts: Vec::new(),
            writing_mode: WritingMode2D::HorizontalTb,
            var_wght: None,
            var_wdth: None,
            var_slnt: None,
            color_bottom_rgba: None,
            gradient_angle_deg: 0.0,
            outer_outline_rgba: [0.0, 0.0, 0.0, 0.0],
            outer_outline_width_norm: 0.0,
            outline_gap_norm: 0.0,
            hdr_intensity: 1.0,
        }
    }
}

impl VectorTextConfig {
    /// Builds a `VectorTextConfig` from `NodeKind::VectorText`, returning `None` for other node kinds.
    pub fn from_node_kind(kind: &NodeKind) -> Option<Self> {
        match kind {
            NodeKind::VectorText {
                text,
                font_size_norm,
                color_rgba,
                outline_rgba,
                outline_width_norm,
                align_center,
                y_offset_norm,
                font_family,
                font_path,
                font_weight,
                align,
                x_offset_norm,
                tracking_em,
                line_height_em,
                opacity,
                fallback_fonts,
                writing_mode,
                var_wght,
                var_wdth,
                var_slnt,
                color_bottom_rgba,
                gradient_angle_deg,
                outer_outline_rgba,
                outer_outline_width_norm,
                outline_gap_norm,
                shadow_offset_norm,
                shadow_blur_norm,
                shadow_rgba,
                glow_radius_norm,
                glow_rgba,
                hdr_intensity,
                gpu_slug: _,
                text_animator: _,
            } => Some(Self {
                text: text.clone(),
                font_size_norm: *font_size_norm,
                color_rgba: *color_rgba,
                outline_rgba: *outline_rgba,
                outline_width_norm: *outline_width_norm,
                align_center: *align_center,
                y_offset_norm: *y_offset_norm,
                x_offset_norm: *x_offset_norm,
                tracking_em: tracking_em.unwrap_or_else(default_tracking),
                line_height_em: line_height_em.unwrap_or_else(default_line_height),
                font_family: font_family.clone(),
                font_path: font_path.clone(),
                font_weight: *font_weight,
                align: *align,
                tracking_em_override: *tracking_em,
                line_height_em_override: *line_height_em,
                opacity: *opacity,
                shadow_offset_norm: *shadow_offset_norm,
                shadow_blur_norm: *shadow_blur_norm,
                shadow_rgba: *shadow_rgba,
                glow_radius_norm: *glow_radius_norm,
                glow_rgba: *glow_rgba,
                fallback_fonts: fallback_fonts.clone(),
                writing_mode: *writing_mode,
                var_wght: *var_wght,
                var_wdth: *var_wdth,
                var_slnt: *var_slnt,
                color_bottom_rgba: *color_bottom_rgba,
                gradient_angle_deg: *gradient_angle_deg,
                outer_outline_rgba: *outer_outline_rgba,
                outer_outline_width_norm: *outer_outline_width_norm,
                outline_gap_norm: *outline_gap_norm,
                hdr_intensity: *hdr_intensity,
                ..Self::default()
            }),
            _ => None,
        }
    }

    /// Returns the effective horizontal alignment (`align` if present, otherwise derived from `align_center`).
    pub fn effective_align(&self) -> TextAlign2D {
        self.align.unwrap_or(if self.align_center {
            TextAlign2D::Center
        } else {
            TextAlign2D::Left
        })
    }

    /// Returns `true` when TrueType/OpenType font resolution should be used instead of the analytical SDF font.
    #[inline]
    pub fn uses_ttf_font(&self) -> bool {
        self.font_family.is_some() || self.font_path.is_some()
    }

    fn to_ttf_config(&self) -> TtfTextConfig {
        let align = match self.effective_align() {
            TextAlign2D::Left => TtfTextAlign::Left,
            TextAlign2D::Center => TtfTextAlign::Center,
            TextAlign2D::Right => TtfTextAlign::Right,
        };
        let tracking_em = self.tracking_em_override.or_else(|| {
            if (self.tracking_em - default_tracking()).abs() > f32::EPSILON {
                Some(self.tracking_em)
            } else {
                None
            }
        });
        let line_height_em = self.line_height_em_override.or_else(|| {
            if (self.line_height_em - default_line_height()).abs() > f32::EPSILON {
                Some(self.line_height_em)
            } else {
                None
            }
        });
        TtfTextConfig {
            text: self.text.clone(),
            font_path: self.font_path.clone(),
            font_family: self.font_family.clone(),
            font_weight: self.font_weight,
            font_size_norm: self.font_size_norm,
            color_rgba: self.color_rgba,
            outline_rgba: self.outline_rgba,
            outline_width_norm: self.outline_width_norm,
            align,
            x_offset_norm: self.x_offset_norm,
            y_offset_norm: self.y_offset_norm,
            tracking_em,
            line_height_em,
            opacity: self.opacity,
            fallback_fonts: self.fallback_fonts.clone(),
            writing_mode: self.writing_mode,
            variations: VariationAxes {
                wght: self.var_wght,
                wdth: self.var_wdth,
                slnt: self.var_slnt,
            },
            color_bottom_rgba: self.color_bottom_rgba,
            outer_outline_rgba: self.outer_outline_rgba,
            outer_outline_width_norm: self.outer_outline_width_norm,
            outline_gap_norm: self.outline_gap_norm,
            hdr_intensity: self.hdr_intensity,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_half() -> f32 {
    0.5
}

fn default_tracking() -> f32 {
    0.05
}

fn default_line_height() -> f32 {
    1.3
}

fn default_font_weight() -> u16 {
    400
}

fn default_one() -> f32 {
    1.0
}

/// Optional styling for anime HUD / title card framing drawn behind `VectorTextConfig`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnimeTitleCardStyle {
    pub backdrop_rgba: [f32; 4],
    pub accent_rgba: [f32; 4],
    pub border_rgba: [f32; 4],
    pub corner_radius_norm: f32,
    pub padding_x_norm: f32,
    pub padding_y_norm: f32,
}

impl Default for AnimeTitleCardStyle {
    fn default() -> Self {
        Self {
            backdrop_rgba: [0.03, 0.03, 0.06, 0.72],
            accent_rgba: [0.95, 0.22, 0.42, 0.95],
            border_rgba: [1.0, 1.0, 1.0, 0.25],
            corner_radius_norm: 0.012,
            padding_x_norm: 0.035,
            padding_y_norm: 0.022,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct PlacedGlyph {
    ch: char,
    /// Top-left of the 8x12 glyph cell in canvas pixels.
    origin_x: f32,
    origin_y: f32,
}

/// Deterministic kerning adjustment in glyph grid units (`8` units = 1 em cap height).
pub fn pair_kerning_units(prev: char, next: char) -> f32 {
    match (prev, next) {
        ('A', 'V') | ('A', 'W') | ('A', 'Y') | ('A', 'T') => -0.75,
        ('V', 'A') | ('W', 'A') | ('Y', 'A') | ('T', 'A') => -0.75,
        ('F', 'A') | ('P', 'A') | ('L', 'T') | ('L', 'V') | ('L', 'W') | ('L', 'Y') => -0.65,
        ('T', 'O') | ('T', 'o') | ('T', 'e') | ('T', 'a') => -0.45,
        ('F', 'R') | ('R', 'A') | ('P', 'C') | ('A', 'C') => -0.25,
        ('/', '/') => -0.65,
        ('T', '.') | ('F', '.') | ('V', '.') | ('W', '.') | ('Y', '.') | ('P', '.') => -0.8,
        _ => 0.0,
    }
}

/// Rasterizes a `NodeKind::VectorText` configuration into an sRGB `Rgba8Frame` (`AlphaMode::Straight`).
///
/// # Panics
/// Panics if `width == 0` or `height == 0`.
#[allow(clippy::too_many_arguments)]
pub fn rasterize_vector_text_node(
    width: u32,
    height: u32,
    text: &str,
    font_size_norm: f32,
    color_rgba: [f32; 4],
    outline_rgba: [f32; 4],
    outline_width_norm: f32,
    align_center: bool,
    y_offset_norm: f32,
) -> Rgba8Frame {
    let config = VectorTextConfig {
        text: text.to_owned(),
        font_size_norm,
        color_rgba,
        outline_rgba,
        outline_width_norm,
        align_center,
        y_offset_norm,
        ..VectorTextConfig::default()
    };
    render_vector_text_rgba8(&config, width, height, Flicks::ZERO, AlphaMode::Straight)
        .expect("width and height must be non-zero")
}

/// Rasterizes a `NodeKind::VectorText` configuration into a Linear Rec.709 `Rgba16fFrame`
/// (`AlphaMode::Premultiplied`), matching FrameShift's GPU working texture format.
///
/// # Panics
/// Panics if `width == 0` or `height == 0`.
#[allow(clippy::too_many_arguments)]
pub fn rasterize_vector_text_rgba16f(
    width: u32,
    height: u32,
    text: &str,
    font_size_norm: f32,
    color_rgba: [f32; 4],
    outline_rgba: [f32; 4],
    outline_width_norm: f32,
    align_center: bool,
    y_offset_norm: f32,
) -> Rgba16fFrame {
    let config = VectorTextConfig {
        text: text.to_owned(),
        font_size_norm,
        color_rgba,
        outline_rgba,
        outline_width_norm,
        align_center,
        y_offset_norm,
        ..VectorTextConfig::default()
    };
    render_vector_text_rgba16f(&config, width, height, Flicks::ZERO, AlphaMode::Premultiplied)
        .expect("width and height must be non-zero")
}

/// Renders a `VectorTextConfig` into an `Rgba8Frame` with the chosen `AlphaMode`.
pub fn render_vector_text_rgba8(
    config: &VectorTextConfig,
    width: u32,
    height: u32,
    pts: Flicks,
    alpha_mode: AlphaMode,
) -> Result<Rgba8Frame, VectorError> {
    let mut canvas = VectorCanvas::new(width, height)?;
    draw_vector_text(&mut canvas, config)?;
    canvas.to_rgba8_frame(pts, alpha_mode)
}

/// Renders a `VectorTextConfig` into a Linear Rec.709 `Rgba16fFrame` with the chosen `AlphaMode`.
pub fn render_vector_text_rgba16f(
    config: &VectorTextConfig,
    width: u32,
    height: u32,
    pts: Flicks,
    alpha_mode: AlphaMode,
) -> Result<Rgba16fFrame, VectorError> {
    if config.uses_ttf_font() {
        let ttf_cfg = config.to_ttf_config();
        return rasterize_ttf_rgba16f_impl(&ttf_cfg, width, height, pts, alpha_mode, None);
    }
    let mut canvas = VectorCanvas::new(width, height)?;
    draw_vector_text(&mut canvas, config)?;
    canvas.to_rgba16f_frame(pts, alpha_mode)
}

/// Converts a `NodeKind::VectorText` node into a [`VectorTextConfig`] and renders it into
/// a premultiplied Linear Rec.709 `Rgba16fFrame`.
pub fn render_vector_text_node_rgba16f(
    kind: &NodeKind,
    width: u32,
    height: u32,
    pts: Flicks,
) -> Result<Rgba16fFrame, VectorError> {
    let config = VectorTextConfig::from_node_kind(kind).ok_or_else(|| {
        VectorError::InvalidParameter(
            "render_vector_text_node_rgba16f requires NodeKind::VectorText".into(),
        )
    })?;
    render_vector_text_rgba16f(&config, width, height, pts, AlphaMode::Premultiplied)
}

/// Converts a `NodeKind::VectorPathHud` node into a [`VectorPathHudConfig`] and renders it into
/// a premultiplied Linear Rec.709 `Rgba16fFrame`.
pub fn render_vector_path_hud_node_rgba16f(
    kind: &NodeKind,
    width: u32,
    height: u32,
    pts: Flicks,
) -> Result<Rgba16fFrame, VectorError> {
    let cfg = match kind {
        NodeKind::VectorPathHud {
            points_norm,
            bezier_segments,
            closed,
            stroke_width_norm,
            stroke_rgba,
            fill_rgba,
            dash_len_norm,
            gap_len_norm,
            dash_offset_norm,
            trim_start,
            trim_end,
            node_dot_radius_norm,
            data_value,
            data_label,
            opacity,
        } => VectorPathHudConfig {
            points_norm: points_norm.clone(),
            bezier_segments: bezier_segments
                .iter()
                .map(|s| CubicBezierHudSegment {
                    p0: s.p0,
                    c1: s.c1,
                    c2: s.c2,
                    p1: s.p1,
                })
                .collect(),
            closed: *closed,
            stroke_width_norm: *stroke_width_norm,
            stroke_rgba: *stroke_rgba,
            fill_rgba: *fill_rgba,
            dash_len_norm: *dash_len_norm,
            gap_len_norm: *gap_len_norm,
            dash_offset_norm: *dash_offset_norm,
            trim_start: *trim_start,
            trim_end: *trim_end,
            node_dot_radius_norm: *node_dot_radius_norm,
            data_value: *data_value,
            data_label: data_label.clone(),
            opacity: *opacity,
        },
        _ => {
            return Err(VectorError::InvalidParameter(
                "render_vector_path_hud_node_rgba16f requires NodeKind::VectorPathHud".into(),
            ));
        }
    };
    render_vector_path_hud_rgba16f(&cfg, width, height, pts)
}

/// Renders an anime HUD / title card (backdrop plate, left accent bar, border, and crisp SDF title).
pub fn render_anime_title_card(
    canvas: &mut VectorCanvas,
    config: &VectorTextConfig,
    style: &AnimeTitleCardStyle,
) -> Result<(), VectorError> {
    let layout = layout_text(config, canvas.width(), canvas.height());
    if let Some((min_x, min_y, max_x, max_y)) = layout.bounds {
        let h = canvas.height() as f32;
        let pad_x = style.padding_x_norm.max(0.0) * h;
        let pad_y = style.padding_y_norm.max(0.0) * h;
        let radius = style.corner_radius_norm.max(0.0) * h;
        let rx = min_x - pad_x;
        let ry = min_y - pad_y;
        let rw = (max_x - min_x) + pad_x * 2.0;
        let rh = (max_y - min_y) + pad_y * 2.0;

        canvas.fill_rounded_rect(rx, ry, rw, rh, radius, style.backdrop_rgba);
        canvas.stroke_rounded_rect(
            rx,
            ry,
            rw,
            rh,
            radius,
            (0.002 * h).max(1.0),
            style.border_rgba,
        );
        let accent_w = (0.006 * h).max(2.0);
        canvas.fill_rounded_rect(
            rx + pad_x * 0.25,
            ry + pad_y * 0.35,
            accent_w,
            (rh - pad_y * 0.7).max(2.0),
            accent_w * 0.5,
            style.accent_rgba,
        );
    }
    draw_vector_text(canvas, config)
}

/// Draws SDF-antialiased, kerning-aware, multi-layer outlined/shadowed typography onto `canvas`.
pub fn draw_vector_text(
    canvas: &mut VectorCanvas,
    config: &VectorTextConfig,
) -> Result<(), VectorError> {
    if config.uses_ttf_font() {
        let ttf_cfg = config.to_ttf_config();
        let query = FontQuery {
            font_path: ttf_cfg.font_path.clone(),
            font_family: ttf_cfg.font_family.clone(),
            font_weight: ttf_cfg.font_weight,
        };
        let (resolved, fallbacks) = resolve_font_chain(&query, &ttf_cfg.fallback_fonts, None)?;
        return draw_ttf_text_with_fallbacks(canvas, &ttf_cfg, &resolved, &fallbacks);
    }

    if !config.font_size_norm.is_finite()
        || !config.outline_width_norm.is_finite()
        || !config.y_offset_norm.is_finite()
        || !config.tracking_em.is_finite()
        || !config.line_height_em.is_finite()
        || !config.opacity.is_finite()
        || config.opacity < 0.0
        || config.x_offset_norm.is_some_and(|v| !v.is_finite())
        || config.tracking_em_override.is_some_and(|v| !v.is_finite())
        || config.line_height_em_override.is_some_and(|v| !v.is_finite())
    {
        return Err(VectorError::InvalidParameter(
            "VectorTextConfig contains non-finite layout parameters".into(),
        ));
    }

    let op = config.opacity.clamp(0.0, 1.0);
    if config.text.is_empty() || config.font_size_norm <= 0.0 || op <= 0.0 {
        return Ok(());
    }

    let width = canvas.width();
    let height = canvas.height();
    let layout = layout_text(config, width, height);
    if layout.glyphs.is_empty() {
        return Ok(());
    }

    let h_f = height as f32;
    let scale = layout.scale;
    let outline_px = (config.outline_width_norm.max(0.0)) * h_f;
    let shadow_dx = sanitize_finite(config.shadow_offset_norm[0]) * h_f;
    let shadow_dy = sanitize_finite(config.shadow_offset_norm[1]) * h_f;
    let shadow_blur_px = sanitize_finite(config.shadow_blur_norm).max(0.0) * h_f;
    let glow_radius_px = sanitize_finite(config.glow_radius_norm).max(0.0) * h_f;

    let has_outline = outline_px > 0.0 && config.outline_rgba[3] > 0.0;
    let has_shadow = config.shadow_rgba[3] > 0.0
        && (shadow_dx.abs() > 0.0 || shadow_dy.abs() > 0.0 || shadow_blur_px > 0.0);
    let has_glow = glow_radius_px > 0.0 && config.glow_rgba[3] > 0.0;

    let extra_pad = outline_px
        .max(glow_radius_px)
        .max(shadow_blur_px + shadow_dx.abs().max(shadow_dy.abs()))
        + SDF_REGION_MARGIN_UNITS * scale
        + 2.0;

    let Some((b_min_x, b_min_y, b_max_x, b_max_y)) = layout.bounds else {
        return Ok(());
    };

    let region_x0 = ((b_min_x - extra_pad).floor() as i64).clamp(0, (width - 1) as i64) as u32;
    let region_y0 = ((b_min_y - extra_pad).floor() as i64).clamp(0, (height - 1) as i64) as u32;
    let region_x1 = ((b_max_x + extra_pad).ceil() as i64).clamp(0, (width - 1) as i64) as u32;
    let region_y1 = ((b_max_y + extra_pad).ceil() as i64).clamp(0, (height - 1) as i64) as u32;

    if region_x1 < region_x0 || region_y1 < region_y0 {
        return Ok(());
    }

    let rw = (region_x1 - region_x0 + 1) as usize;
    let rh = (region_y1 - region_y0 + 1) as usize;
    let mut unified_sdf_px = vec![1.0e6f32; rw * rh];

    let glyph_pad_px = outline_px
        .max(glow_radius_px)
        .max(shadow_blur_px)
        + 2.5 * scale
        + 2.0;

    for placed in &layout.glyphs {
        if placed.ch == ' ' || placed.ch == '\t' {
            continue;
        }
        let sdf_tile = glyph_sdf(placed.ch);
        let cell_w_px = (GLYPH_CELL_WIDTH as f32) * scale;
        let cell_h_px = (GLYPH_CELL_HEIGHT as f32) * scale;

        let gx0 = ((placed.origin_x - glyph_pad_px).floor() as i64)
            .clamp(region_x0 as i64, region_x1 as i64) as u32;
        let gy0 = ((placed.origin_y - glyph_pad_px).floor() as i64)
            .clamp(region_y0 as i64, region_y1 as i64) as u32;
        let gx1 = ((placed.origin_x + cell_w_px + glyph_pad_px).ceil() as i64)
            .clamp(region_x0 as i64, region_x1 as i64) as u32;
        let gy1 = ((placed.origin_y + cell_h_px + glyph_pad_px).ceil() as i64)
            .clamp(region_y0 as i64, region_y1 as i64) as u32;

        if gx1 < gx0 || gy1 < gy0 {
            continue;
        }

        let inv_scale = 1.0 / scale;
        let use_exact = scale >= 3.0;
        for py in gy0..=gy1 {
            let local_y = (py as f32 + 0.5 - placed.origin_y) * inv_scale;
            let row_off = ((py - region_y0) as usize) * rw;
            for px in gx0..=gx1 {
                let local_x = (px as f32 + 0.5 - placed.origin_x) * inv_scale;
                let dist = if use_exact {
                    sdf_tile.eval_exact(local_x, local_y)
                } else {
                    sdf_tile.sample(local_x, local_y)
                };
                let d_px = dist * scale;
                let idx = row_off + ((px - region_x0) as usize);
                if d_px < unified_sdf_px[idx] {
                    unified_sdf_px[idx] = d_px;
                }
            }
        }
    }

    let fill_premul = premultiply_rgba(config.color_rgba);
    let outline_premul = premultiply_rgba(config.outline_rgba);
    let shadow_premul = premultiply_rgba(config.shadow_rgba);
    let glow_premul = premultiply_rgba(config.glow_rgba);
    let apply_opacity = (op - 1.0).abs() > 0.0;

    for py in region_y0..=region_y1 {
        let ry = (py - region_y0) as usize;
        for px in region_x0..=region_x1 {
            let rx = (px - region_x0) as usize;
            let d_px = unified_sdf_px[ry * rw + rx];

            let mut acc = [0.0f32; 4];

            if has_shadow {
                let sx = (px as f32 - shadow_dx).round() as i64 - (region_x0 as i64);
                let sy = (py as f32 - shadow_dy).round() as i64 - (region_y0 as i64);
                if sx >= 0 && sx < (rw as i64) && sy >= 0 && sy < (rh as i64) {
                    let sd = unified_sdf_px[(sy as usize) * rw + (sx as usize)] - outline_px;
                    let cov = if shadow_blur_px > 0.0 {
                        let half_b = shadow_blur_px * 0.5;
                        smoothstep(half_b, -half_b, sd)
                    } else {
                        sdf_coverage(sd)
                    };
                    if cov > 0.0 {
                        let src = scale_premul(shadow_premul, cov);
                        acc = source_over_premul(acc, src);
                    }
                }
            }

            if has_glow {
                let d_outer = (d_px - outline_px).max(0.0);
                if d_outer < glow_radius_px {
                    let t = 1.0 - (d_outer / glow_radius_px);
                    let cov = t * t;
                    let src = scale_premul(glow_premul, cov);
                    acc = source_over_premul(acc, src);
                }
            }

            if has_outline {
                let outer_cov = sdf_coverage(d_px - outline_px);
                if outer_cov > 0.0 {
                    let src = scale_premul(outline_premul, outer_cov);
                    acc = source_over_premul(acc, src);
                }
            }

            if fill_premul[3] > 0.0 {
                let fill_cov = sdf_coverage(d_px);
                if fill_cov > 0.0 {
                    let src = scale_premul(fill_premul, fill_cov);
                    acc = source_over_premul(acc, src);
                }
            }

            if acc[3] > 0.0 {
                if apply_opacity {
                    acc = scale_premul(acc, op);
                }
                canvas.blend_pixel_premul(px, py, acc, 1.0);
            }
        }
    }

    Ok(())
}

/// Burns a crisp, high-contrast label badge (timestamps, revision IDs, node names) onto `canvas`.
///
/// Safe against out-of-bounds `(x, y)` or overflow near frame edges; preserves `canvas.alpha_mode`.
pub fn render_label_badge(
    canvas: &mut Rgba8Frame,
    x: u32,
    y: u32,
    text: &str,
    fg: [u8; 4],
    bg: [u8; 4],
) {
    render_label_badge_scaled(canvas, x, y, text, fg, bg, 1);
}

/// Scaled variant of [`render_label_badge`] with integer pixel magnification (`scale >= 1`).
pub fn render_label_badge_scaled(
    canvas: &mut Rgba8Frame,
    x: u32,
    y: u32,
    text: &str,
    fg: [u8; 4],
    bg: [u8; 4],
    scale: u32,
) {
    let s = scale.max(1);
    if x >= canvas.width || y >= canvas.height {
        return;
    }

    let pad_x = 4 * s;
    let pad_y = 2 * s;

    let mut text_units_w: u32 = 0;
    let chars: Vec<char> = text.chars().collect();
    for (idx, &ch) in chars.iter().enumerate() {
        let g = bitmap_glyph(ch);
        let w = if g.ink_width == 0 {
            3
        } else {
            g.ink_width as u32
        };
        text_units_w = text_units_w.saturating_add(w);
        if idx + 1 < chars.len() {
            text_units_w = text_units_w.saturating_add(1);
        }
    }

    let badge_w = pad_x * 2 + text_units_w * s;
    let badge_h = pad_y * 2 + (GLYPH_CELL_HEIGHT as u32) * s;

    let end_x = x.saturating_add(badge_w).min(canvas.width);
    let end_y = y.saturating_add(badge_h).min(canvas.height);

    for py in y..end_y {
        for px in x..end_x {
            blend_u8_pixel(canvas, px, py, bg);
        }
    }

    let mut cursor_x = x + pad_x;
    let glyph_y0 = y + pad_y;

    for &ch in &chars {
        let g = bitmap_glyph(ch);
        if g.ink_width == 0 {
            cursor_x = cursor_x.saturating_add(4 * s);
            continue;
        }
        let left = g.left_col as usize;
        let w = g.ink_width as usize;
        for row in 0..GLYPH_CELL_HEIGHT {
            for col_off in 0..w {
                if g.pixel(left + col_off, row) {
                    let bx = cursor_x + (col_off as u32) * s;
                    let by = glyph_y0 + (row as u32) * s;
                    for dy in 0..s {
                        for dx in 0..s {
                            let px = bx + dx;
                            let py = by + dy;
                            if px < end_x && py < end_y {
                                blend_u8_pixel(canvas, px, py, fg);
                            }
                        }
                    }
                }
            }
        }
        cursor_x = cursor_x.saturating_add(((w as u32) + 1) * s);
    }
}

struct TextLayout {
    scale: f32,
    glyphs: Vec<PlacedGlyph>,
    bounds: Option<(f32, f32, f32, f32)>,
}

fn layout_text(config: &VectorTextConfig, width: u32, height: u32) -> TextLayout {
    let w_f = width as f32;
    let h_f = height as f32;
    let font_px = config.font_size_norm.max(0.0) * h_f;
    let scale = font_px / GLYPH_CAP_HEIGHT;
    if scale <= 0.0 {
        return TextLayout {
            scale: 0.0,
            glyphs: Vec::new(),
            bounds: None,
        };
    }

    let tracking_em = config.tracking_em_override.unwrap_or(config.tracking_em);
    let line_height_em = config
        .line_height_em_override
        .unwrap_or(config.line_height_em);
    let tracking_units = tracking_em * GLYPH_CAP_HEIGHT;
    let line_step_px = line_height_em.max(0.5) * font_px;

    let lines: Vec<&str> = config
        .text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect();
    let total_block_h = if lines.len() <= 1 {
        font_px
    } else {
        font_px + ((lines.len() - 1) as f32) * line_step_px
    };

    let block_center_y = config.y_offset_norm * h_f;
    let first_cap_top_y = block_center_y - total_block_h * 0.5;

    let effective_align = config.effective_align();
    let anchor_x_norm = config.x_offset_norm.unwrap_or(match effective_align {
        TextAlign2D::Left => DEFAULT_LEFT_MARGIN_NORM,
        TextAlign2D::Center => 0.5,
        TextAlign2D::Right => 1.0 - DEFAULT_LEFT_MARGIN_NORM,
    });
    let anchor_x = anchor_x_norm * w_f;

    let mut glyphs = Vec::new();
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;

    for (line_idx, line) in lines.iter().enumerate() {
        let chars: Vec<char> = line.chars().collect();
        if chars.is_empty() {
            continue;
        }

        let mut line_advances_units = Vec::with_capacity(chars.len());
        let mut cursor_units = 0.0f32;
        for (i, &ch) in chars.iter().enumerate() {
            let sdf = glyph_sdf(ch);
            let kern = if config.kerning_enabled && i > 0 {
                pair_kerning_units(chars[i - 1], ch)
            } else {
                0.0
            };
            cursor_units += kern;
            let start_u = cursor_units;
            let extra_track = if i + 1 < chars.len() {
                tracking_units
            } else {
                0.0
            };
            cursor_units += sdf.bitmap.advance + extra_track;
            line_advances_units.push((ch, start_u, sdf.bitmap.left_col as f32, sdf.bitmap.ink_width as f32));
        }

        // Trim trailing inter-glyph space from visual line width so centering is exact.
        let last_ch = *chars.last().unwrap();
        let last_bmp = bitmap_glyph(last_ch);
        let visual_end_units = if last_bmp.ink_width > 0 {
            let (_, start_u, _, ink_w) = line_advances_units.last().copied().unwrap();
            start_u + ink_w
        } else {
            cursor_units
        };
        let line_width_px = visual_end_units * scale;

        let line_start_x = match effective_align {
            TextAlign2D::Left => anchor_x,
            TextAlign2D::Center => anchor_x - line_width_px * 0.5,
            TextAlign2D::Right => anchor_x - line_width_px,
        };
        let cap_top_y = first_cap_top_y + (line_idx as f32) * line_step_px;
        let cell_origin_y = cap_top_y - GLYPH_CAP_TOP * scale;

        for (ch, start_u, left_col_u, ink_w_u) in line_advances_units {
            let origin_x = line_start_x + (start_u - left_col_u) * scale;
            glyphs.push(PlacedGlyph {
                ch,
                origin_x,
                origin_y: cell_origin_y,
            });
            if ink_w_u > 0.0 {
                let gx0 = line_start_x + start_u * scale;
                let gx1 = gx0 + ink_w_u * scale;
                let gy0 = cell_origin_y;
                let gy1 = cell_origin_y + (GLYPH_CELL_HEIGHT as f32) * scale;
                min_x = min_x.min(gx0);
                min_y = min_y.min(gy0);
                max_x = max_x.max(gx1);
                max_y = max_y.max(gy1);
            }
        }
    }

    let bounds = if min_x.is_finite() {
        Some((min_x, min_y, max_x, max_y))
    } else {
        None
    };

    TextLayout {
        scale,
        glyphs,
        bounds,
    }
}

fn blend_u8_pixel(canvas: &mut Rgba8Frame, x: u32, y: u32, src_straight: [u8; 4]) {
    let sa = src_straight[3] as f32 / 255.0;
    if sa <= 0.0 {
        return;
    }
    let Some(dst) = canvas.get_pixel(x, y) else {
        return;
    };

    let sr = src_straight[0] as f32 / 255.0;
    let sg = src_straight[1] as f32 / 255.0;
    let sb = src_straight[2] as f32 / 255.0;

    let da = dst[3] as f32 / 255.0;
    let (dr_pre, dg_pre, db_pre) = match canvas.alpha_mode {
        AlphaMode::Straight => (
            (dst[0] as f32 / 255.0) * da,
            (dst[1] as f32 / 255.0) * da,
            (dst[2] as f32 / 255.0) * da,
        ),
        AlphaMode::Premultiplied => (
            dst[0] as f32 / 255.0,
            dst[1] as f32 / 255.0,
            dst[2] as f32 / 255.0,
        ),
    };

    let inv_sa = 1.0 - sa;
    let out_a = (sa + da * inv_sa).clamp(0.0, 1.0);
    let out_r_pre = sr * sa + dr_pre * inv_sa;
    let out_g_pre = sg * sa + dg_pre * inv_sa;
    let out_b_pre = sb * sa + db_pre * inv_sa;

    let (or, og, ob) = match canvas.alpha_mode {
        AlphaMode::Straight => {
            if out_a > 1e-6 {
                (
                    (out_r_pre / out_a).clamp(0.0, 1.0),
                    (out_g_pre / out_a).clamp(0.0, 1.0),
                    (out_b_pre / out_a).clamp(0.0, 1.0),
                )
            } else {
                (0.0, 0.0, 0.0)
            }
        }
        AlphaMode::Premultiplied => (
            out_r_pre.clamp(0.0, 1.0),
            out_g_pre.clamp(0.0, 1.0),
            out_b_pre.clamp(0.0, 1.0),
        ),
    };

    canvas.set_pixel(
        x,
        y,
        [
            (or * 255.0).round() as u8,
            (og * 255.0).round() as u8,
            (ob * 255.0).round() as u8,
            (out_a * 255.0).round() as u8,
        ],
    );
}

#[inline]
fn scale_premul(c: [f32; 4], s: f32) -> [f32; 4] {
    [c[0] * s, c[1] * s, c[2] * s, c[3] * s]
}

#[inline]
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    if (edge1 - edge0).abs() <= 1e-6 {
        return if x >= edge1 { 1.0 } else { 0.0 };
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[inline]
fn sanitize_finite(v: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        0.0
    }
}
