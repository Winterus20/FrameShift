use std::sync::Arc;

use frameshift_schema::{
    TextAlign2D, TextAnimatorDef, TextRangeShape, TextSelectorUnit, WritingMode2D,
};
use harfrust::{Buffer, Direction, Feature, ShapeOptions, ShaperFont, Tag};
use lyon_tessellation::math::{Point, point};
use lyon_tessellation::path::Path as LyonPath;
use skrifa::font::{Blob, Font, Variation};
use skrifa::{DrawMemory, PathSink, ScalerFont};
use frameshift_time::Flicks;

use crate::error::VectorError;
use crate::font_locator::ResolvedFont;

/// Variable font axis overrides (`wght`, `wdth`, `slnt`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct VariationAxes {
    pub wght: Option<f32>,
    pub wdth: Option<f32>,
    pub slnt: Option<f32>,
}

impl VariationAxes {
    pub fn is_empty(&self) -> bool {
        self.wght.is_none() && self.wdth.is_none() && self.slnt.is_none()
    }

    fn to_variations(self) -> Vec<Variation> {
        let mut vars = Vec::with_capacity(3);
        if let Some(v) = self.wght {
            vars.push(Variation::new(Tag::new(b"wght"), v));
        }
        if let Some(v) = self.wdth {
            vars.push(Variation::new(Tag::new(b"wdth"), v));
        }
        if let Some(v) = self.slnt {
            vars.push(Variation::new(Tag::new(b"slnt"), v));
        }
        vars
    }
}

/// Single quadratic Bézier segment in Y-up layout space (`p0` -> `p1` -> `p2`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QuadraticCurve {
    pub p0: [f32; 2],
    pub p1: [f32; 2],
    pub p2: [f32; 2],
}

/// Shaped and outlined glyph in Y-up layout coordinates.
#[derive(Debug, Clone)]
pub struct ShapedGlyphOutline {
    pub char_index: u32,
    /// Logical grapheme range in the source string covered by this shaped glyph.
    /// A ligature can span more than one grapheme, so range selection expands to it.
    pub grapheme_start: u32,
    pub grapheme_end: u32,
    /// Logical whitespace-delimited word range covered by this shaped glyph.
    pub word_start: u32,
    pub word_end: u32,
    pub glyph_id: u32,
    pub font_index: usize,
    pub x: f32,
    pub y: f32,
    pub advance: f32,
    pub bounds_min: [f32; 2],
    pub bounds_max: [f32; 2],
    pub curves: Vec<QuadraticCurve>,
    pub lyon_path: LyonPath,
}

/// Complete shaped text block in Y-up coordinates (centered around anchor `(0, 0)`).
#[derive(Debug, Clone)]
pub struct ShapedTextBlock {
    pub glyphs: Vec<ShapedGlyphOutline>,
    pub bounds_min: [f32; 2],
    pub bounds_max: [f32; 2],
    pub line_count: usize,
    pub char_count: u32,
    pub grapheme_count: u32,
    pub word_count: u32,
    pub font_size: f32,
}

/// Layout options for `shape_text_block`.
#[derive(Debug, Clone, Copy)]
pub struct TextLayoutOptions {
    pub font_size: f32,
    pub align: TextAlign2D,
    pub writing_mode: WritingMode2D,
    pub tracking_em: f32,
    pub line_height_em: Option<f32>,
    pub variations: VariationAxes,
    pub enable_ligatures: bool,
}

/// Evaluated per-glyph transform produced by a `TextAnimatorDef`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphAnimationState {
    pub position_norm: [f32; 2],
    pub scale: f32,
    pub opacity: f32,
    pub rotation_deg: f32,
    pub tracking_em: f32,
}

impl Default for GlyphAnimationState {
    fn default() -> Self {
        Self {
            position_norm: [0.0, 0.0],
            scale: 1.0,
            opacity: 1.0,
            rotation_deg: 0.0,
            tracking_em: 0.0,
        }
    }
}

#[inline]
fn hash_animator_unit_01(seed: u64, unit_idx: u32) -> f32 {
    let s32 = (seed as u32) ^ ((seed >> 32) as u32);
    let mut x = unit_idx.wrapping_mul(0x9E37_79B9u32) ^ s32 ^ 0x85EB_CA6Bu32;
    x ^= x >> 16;
    x = x.wrapping_mul(0x7FEB_352Du32);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846C_A68Bu32);
    x ^= x >> 16;
    ((x >> 8) as f32) / 16_777_216.0
}

#[inline]
fn ease_animator_progress(t: f32, ease_high: f32, ease_low: f32) -> f32 {
    let u = t.clamp(0.0, 1.0);
    if ease_high == 0.0 && ease_low == 0.0 {
        return u;
    }
    let eh = (ease_high / 100.0).clamp(-1.0, 1.0);
    let el = (ease_low / 100.0).clamp(-1.0, 1.0);
    let m0 = (if el >= 0.0 { 1.0 - el } else { 1.0 - 0.5 * el }).clamp(0.0, 1.5);
    let m1 = (if eh >= 0.0 { 1.0 - eh } else { 1.0 - 0.5 * eh }).clamp(0.0, 1.5);
    let u2 = u * u;
    let u3 = u2 * u;
    ((u3 - 2.0 * u2 + u) * m0 + (-2.0 * u3 + 3.0 * u2) + (u3 - u2) * m1).clamp(0.0, 1.0)
}

#[inline]
fn evaluate_range_shape_weight(shape: TextRangeShape, u: f32) -> f32 {
    let x = u.clamp(0.0, 1.0);
    match shape {
        TextRangeShape::Square => 1.0,
        TextRangeShape::RampUp => x,
        TextRangeShape::RampDown => 1.0 - x,
        TextRangeShape::Triangle => (1.0 - (2.0 * x - 1.0).abs()).clamp(0.0, 1.0),
        TextRangeShape::Smooth => {
            0.5 * (1.0 - (std::f32::consts::TAU * x).cos()).clamp(0.0, 2.0)
        }
    }
}

/// Evaluates a text animator for a shaped glyph's grapheme/word ranges.
pub fn evaluate_text_animator(
    animator: Option<&TextAnimatorDef>,
    grapheme_range: (u32, u32),
    word_range: (u32, u32),
    tick: Flicks,
) -> GlyphAnimationState {
    evaluate_text_animator_with_total(animator, grapheme_range, word_range, None, tick)
}

/// Evaluates a text animator with an optional known `total_units` count for open-ended
/// (`range_end: None`) range shapes and deterministic stagger randomization.
pub fn evaluate_text_animator_with_total(
    animator: Option<&TextAnimatorDef>,
    grapheme_range: (u32, u32),
    word_range: (u32, u32),
    total_units: Option<u32>,
    tick: Flicks,
) -> GlyphAnimationState {
    let Some(animator) = animator else {
        return GlyphAnimationState::default();
    };
    let (unit_start, unit_end) = match animator.unit {
        TextSelectorUnit::Grapheme => grapheme_range,
        TextSelectorUnit::Word => word_range,
    };
    let range_end = animator.range_end.unwrap_or(u32::MAX);
    if unit_end <= unit_start || unit_end <= animator.range_start || unit_start >= range_end {
        return GlyphAnimationState::default();
    }

    let selected_start = unit_start.max(animator.range_start);
    let rel_idx = selected_start.saturating_sub(animator.range_start);
    let effective_end = animator.range_end.or(total_units);
    let range_count = effective_end
        .map(|e| e.saturating_sub(animator.range_start).max(1));

    let stagger_f = if let Some(seed) = animator.randomize_seed {
        let h = hash_animator_unit_01(seed, selected_start) as f64;
        let span_slots = range_count
            .map(|c| c.saturating_sub(1) as f64)
            .unwrap_or(8.0);
        h * span_slots * (animator.delay_ticks.max(0) as f64)
    } else {
        (i128::from(rel_idx) * i128::from(animator.delay_ticks.max(0))) as f64
    };

    let elapsed = (i128::from(tick.0) - i128::from(animator.start_tick)) as f64 - stagger_f;
    let raw_progress = if animator.duration_ticks <= 0 {
        if elapsed >= 0.0 { 1.0 } else { 0.0 }
    } else {
        (elapsed / animator.duration_ticks as f64).clamp(0.0, 1.0) as f32
    };
    let progress = ease_animator_progress(raw_progress, animator.ease_high, animator.ease_low);

    let shape_weight = if animator.shape == TextRangeShape::Square {
        1.0
    } else {
        let u = if let Some(seed) = animator.randomize_seed {
            hash_animator_unit_01(seed ^ 0x517C_C1B7_2722_0A95u64, selected_start)
        } else if let Some(count) = range_count {
            (rel_idx as f32 + 0.5) / (count as f32)
        } else {
            (rel_idx as f32 + 0.5) / (rel_idx as f32 + 1.0)
        };
        evaluate_range_shape_weight(animator.shape, u)
    };

    let from_amount = (1.0 - progress) * shape_weight;
    GlyphAnimationState {
        position_norm: [
            animator.position_from_norm[0] * from_amount,
            animator.position_from_norm[1] * from_amount,
        ],
        scale: 1.0 + (animator.scale_from - 1.0) * from_amount,
        opacity: 1.0 + (animator.opacity_from - 1.0) * from_amount,
        rotation_deg: animator.rotation_deg_from * from_amount,
        tracking_em: animator.tracking_em_from * from_amount,
    }
}

impl Default for TextLayoutOptions {
    fn default() -> Self {
        Self {
            font_size: 48.0,
            align: TextAlign2D::Center,
            writing_mode: WritingMode2D::HorizontalTb,
            tracking_em: 0.0,
            line_height_em: None,
            variations: VariationAxes::default(),
            enable_ligatures: true,
        }
    }
}

struct PreparedFont {
    font: Font,
    scaler: ScalerFont,
    units_per_em: f32,
    ascender: f32,
    descender: f32,
    line_gap: f32,
    cap_height: f32,
}

fn prepare_font(
    resolved: &ResolvedFont,
    font_size: f32,
    variations: VariationAxes,
) -> Result<PreparedFont, VectorError> {
    let blob = Blob::from(Arc::clone(&resolved.bytes) as Arc<dyn AsRef<[u8]> + Send + Sync>);
    let base_font = Font::new(blob, resolved.face_index).ok_or_else(|| {
        VectorError::InvalidFont(format!(
            "skrifa failed to parse font '{}' (face {})",
            resolved.matched_family, resolved.face_index
        ))
    })?;

    let upem = base_font.units_per_em();
    if upem == 0 {
        return Err(VectorError::InvalidFont(
            "font has units_per_em = 0".to_string(),
        ));
    }

    let font = if variations.is_empty() {
        base_font
    } else {
        base_font
            .instance_builder()
            .variations(variations.to_variations())
            .build()
    };

    let scaler = ScalerFont::force_unhinted(font.clone(), Some(font_size));
    let metrics = scaler.metrics();
    let h_line = metrics.h_line().unwrap_or_default();
    let cap_height = metrics
        .cap_height()
        .filter(|&v| v > 0.0)
        .unwrap_or(h_line.ascender * 0.72);

    Ok(PreparedFont {
        font,
        scaler,
        units_per_em: f32::from(upem),
        ascender: h_line.ascender,
        descender: h_line.descender,
        line_gap: h_line.line_gap,
        cap_height,
    })
}

/// Shapes multi-line UTF-8 text with HarfRust (GPOS/GSUB), Skrifa (variable axes + outlines),
/// fallback font chain, and `HorizontalTb` or `VerticalRl` (Tategaki) layout.
pub fn shape_text_block(
    text: &str,
    primary_font: &ResolvedFont,
    fallback_fonts: &[ResolvedFont],
    options: &TextLayoutOptions,
) -> Result<ShapedTextBlock, VectorError> {
    if !options.font_size.is_finite() || options.font_size <= 0.0 {
        return Err(VectorError::InvalidParameter(format!(
            "font_size must be finite and > 0 (got {})",
            options.font_size
        )));
    }
    if !options.tracking_em.is_finite() {
        return Err(VectorError::InvalidParameter(
            "tracking_em must be finite".to_string(),
        ));
    }
    if options.line_height_em.is_some_and(|v| !v.is_finite() || v <= 0.0) {
        return Err(VectorError::InvalidParameter(
            "line_height_em must be finite and > 0".to_string(),
        ));
    }

    let mut prepared_fonts = Vec::with_capacity(1 + fallback_fonts.len());
    prepared_fonts.push(prepare_font(
        primary_font,
        options.font_size,
        options.variations,
    )?);
    for fb in fallback_fonts {
        prepared_fonts.push(prepare_font(fb, options.font_size, options.variations)?);
    }

    let normalized = text.replace("\r\n", "\n");
    let lines: Vec<&str> = normalized.split('\n').collect();
    let line_count = lines.len().max(1);

    if text.is_empty() {
        return Ok(ShapedTextBlock {
            glyphs: Vec::new(),
            bounds_min: [0.0, 0.0],
            bounds_max: [0.0, 0.0],
            line_count,
            char_count: 0,
            grapheme_count: 0,
            word_count: 0,
            font_size: options.font_size,
        });
    }

    let primary = &prepared_fonts[0];
    let natural_line_px = (primary.ascender - primary.descender + primary.line_gap.max(0.0))
        .max(options.font_size);
    let line_step = match options.line_height_em {
        Some(lh) => lh.max(0.25) * options.font_size,
        None => match options.writing_mode {
            WritingMode2D::HorizontalTb => natural_line_px,
            WritingMode2D::VerticalRl => (options.font_size * 1.25).max(natural_line_px),
        },
    };
    let tracking_px = options.tracking_em * options.font_size;

    let mut features = Vec::new();
    if !options.enable_ligatures {
        features.push(Feature::new(Tag::new(b"liga"), 0, ..));
        features.push(Feature::new(Tag::new(b"clig"), 0, ..));
    }
    if options.writing_mode == WritingMode2D::VerticalRl {
        features.push(Feature::new(Tag::new(b"vert"), 1, ..));
        features.push(Feature::new(Tag::new(b"vrt2"), 1, ..));
    }

    struct RawPlacedGlyph {
        char_index: u32,
        grapheme_start: u32,
        grapheme_end: u32,
        word_start: u32,
        word_end: u32,
        glyph_id: u32,
        font_idx: usize,
        x: f32,
        y: f32,
        advance: f32,
    }

    let mut placed_glyphs = Vec::new();
    let mut global_char_idx = 0u32;
    let unit_map = TextUnitMap::new(&normalized);

    match options.writing_mode {
        WritingMode2D::HorizontalTb => {
            let baseline_0 =
                ((line_count - 1) as f32 * 0.5) * line_step - primary.cap_height * 0.5;

            for (line_idx, line) in lines.iter().enumerate() {
                if line.is_empty() {
                    if line_idx + 1 < lines.len() {
                        global_char_idx = global_char_idx.saturating_add(1);
                    }
                    continue;
                }
                let runs = split_into_font_runs(line, &prepared_fonts);
                let mut line_placed: Vec<RawPlacedGlyph> = Vec::new();
                let mut pen_x = 0.0_f32;
                let mut pen_y = 0.0_f32;
                let run_count = runs.len();

                for (run_idx, (font_idx, run_text, char_len)) in runs.into_iter().enumerate() {
                    let pf = &prepared_fonts[font_idx];
                    let scale = options.font_size / pf.units_per_em;
                    let shaper_font = ShaperFont::new(&pf.font);
                    let mut buf = Buffer::new();
                    buf.push_str(run_text);
                    buf.set_direction(Direction::LeftToRight);
                    buf.guess_segment_properties();

                    let shape_opts = ShapeOptions::new().features(&features);
                    if harfrust::shape(&shaper_font, &mut buf, shape_opts).is_err() {
                        global_char_idx += char_len as u32;
                        continue;
                    }

                    let infos = buf.glyph_infos();
                    let positions = buf.glyph_positions();
                    let run_glyph_count = infos.len();
                    let clusters = cluster_char_ranges(run_text, infos);

                    for i in 0..run_glyph_count {
                        let info = infos[i];
                        let pos = positions[i];
                        let gx = pen_x + (pos.x_offset as f32) * scale;
                        let gy = pen_y + (pos.y_offset as f32) * scale;
                        let adv = (pos.x_advance as f32) * scale;
                        let (cluster_char_start, cluster_char_end) = clusters[i];
                        let (grapheme_start, grapheme_end) = unit_map.grapheme_range(
                            global_char_idx + cluster_char_start,
                            global_char_idx + cluster_char_end,
                        );
                        let (word_start, word_end) = unit_map.word_range(
                            global_char_idx + cluster_char_start,
                            global_char_idx + cluster_char_end,
                        );
                        let is_very_last = run_idx + 1 == run_count && i + 1 == run_glyph_count;
                        let extra_track = if is_very_last { 0.0 } else { tracking_px };

                        line_placed.push(RawPlacedGlyph {
                            char_index: global_char_idx + cluster_char_start,
                            grapheme_start,
                            grapheme_end,
                            word_start,
                            word_end,
                            glyph_id: info.glyph_id,
                            font_idx,
                            x: gx,
                            y: gy,
                            advance: adv + extra_track,
                        });
                        pen_x += adv + extra_track;
                        pen_y += (pos.y_advance as f32) * scale;
                    }
                    global_char_idx += char_len as u32;
                }

                let line_width = pen_x.max(0.0);
                let line_start_x = match options.align {
                    TextAlign2D::Left => 0.0,
                    TextAlign2D::Center => -line_width * 0.5,
                    TextAlign2D::Right => -line_width,
                };
                let baseline_y = baseline_0 - (line_idx as f32) * line_step;
                for mut g in line_placed {
                    g.x += line_start_x;
                    g.y += baseline_y;
                    placed_glyphs.push(g);
                }
                if line_idx + 1 < lines.len() {
                    global_char_idx = global_char_idx.saturating_add(1);
                }
            }
        }
        WritingMode2D::VerticalRl => {
            // Right-to-left vertical columns: column 0 is rightmost.
            let col_start_x = ((line_count - 1) as f32 * 0.5) * line_step;

            for (col_idx, line) in lines.iter().enumerate() {
                if line.is_empty() {
                    if col_idx + 1 < lines.len() {
                        global_char_idx = global_char_idx.saturating_add(1);
                    }
                    continue;
                }
                let runs = split_into_font_runs(line, &prepared_fonts);
                let mut col_placed: Vec<RawPlacedGlyph> = Vec::new();
                let mut pen_x = 0.0_f32;
                let mut pen_y = 0.0_f32;
                let run_count = runs.len();

                for (run_idx, (font_idx, run_text, char_len)) in runs.into_iter().enumerate() {
                    let pf = &prepared_fonts[font_idx];
                    let scale = options.font_size / pf.units_per_em;
                    let shaper_font = ShaperFont::new(&pf.font);
                    let mut buf = Buffer::new();
                    buf.push_str(run_text);
                    buf.set_direction(Direction::TopToBottom);
                    buf.guess_segment_properties();

                    let shape_opts = ShapeOptions::new().features(&features);
                    if harfrust::shape(&shaper_font, &mut buf, shape_opts).is_err() {
                        global_char_idx += char_len as u32;
                        continue;
                    }

                    let infos = buf.glyph_infos();
                    let positions = buf.glyph_positions();
                    let run_glyph_count = infos.len();
                    let clusters = cluster_char_ranges(run_text, infos);

                    for i in 0..run_glyph_count {
                        let info = infos[i];
                        let pos = positions[i];
                        let gx = pen_x + (pos.x_offset as f32) * scale;
                        let gy = pen_y + (pos.y_offset as f32) * scale;
                        let y_adv = (pos.y_advance as f32) * scale;
                        let step_y = if y_adv.abs() > 1e-3 {
                            y_adv
                        } else {
                            -options.font_size * 1.05
                        };
                        let (cluster_char_start, cluster_char_end) = clusters[i];
                        let (grapheme_start, grapheme_end) = unit_map.grapheme_range(
                            global_char_idx + cluster_char_start,
                            global_char_idx + cluster_char_end,
                        );
                        let (word_start, word_end) = unit_map.word_range(
                            global_char_idx + cluster_char_start,
                            global_char_idx + cluster_char_end,
                        );
                        let is_very_last = run_idx + 1 == run_count && i + 1 == run_glyph_count;
                        let extra_track = if is_very_last { 0.0 } else { tracking_px };

                        col_placed.push(RawPlacedGlyph {
                            char_index: global_char_idx + cluster_char_start,
                            grapheme_start,
                            grapheme_end,
                            word_start,
                            word_end,
                            glyph_id: info.glyph_id,
                            font_idx,
                            x: gx,
                            y: gy,
                            advance: step_y.abs() + extra_track,
                        });
                        pen_x += (pos.x_advance as f32) * scale;
                        pen_y += step_y - extra_track;
                    }
                    global_char_idx += char_len as u32;
                }

                let col_height = (-pen_y).max(0.0);
                let col_top_y = match options.align {
                    TextAlign2D::Left => 0.0,
                    TextAlign2D::Center => col_height * 0.5,
                    TextAlign2D::Right => col_height,
                };
                let col_x = col_start_x - (col_idx as f32) * line_step;
                for mut g in col_placed {
                    g.x += col_x;
                    g.y += col_top_y;
                    placed_glyphs.push(g);
                }
                if col_idx + 1 < lines.len() {
                    global_char_idx = global_char_idx.saturating_add(1);
                }
            }
        }
    }

    let mut glyphs = Vec::with_capacity(placed_glyphs.len());
    let mut block_min_x = f32::INFINITY;
    let mut block_min_y = f32::INFINITY;
    let mut block_max_x = f32::NEG_INFINITY;
    let mut block_max_y = f32::NEG_INFINITY;

    for pg in placed_glyphs {
        let pf = &prepared_fonts[pg.font_idx];
        let gid = skrifa::GlyphId::new(pg.glyph_id);
        let Some(outline) = pf.scaler.outline(gid) else {
            continue;
        };

        let mut sink = DualGlyphSink::new(pg.x, pg.y);
        if outline.draw(DrawMemory::Scratch, &mut sink).is_err() {
            continue;
        }
        let (curves, lyon_path, bounds_min, bounds_max) = sink.finish();
        if curves.is_empty() {
            continue;
        }

        block_min_x = block_min_x.min(bounds_min[0]);
        block_min_y = block_min_y.min(bounds_min[1]);
        block_max_x = block_max_x.max(bounds_max[0]);
        block_max_y = block_max_y.max(bounds_max[1]);

        glyphs.push(ShapedGlyphOutline {
            char_index: pg.char_index,
            grapheme_start: pg.grapheme_start,
            grapheme_end: pg.grapheme_end,
            word_start: pg.word_start,
            word_end: pg.word_end,
            glyph_id: pg.glyph_id,
            font_index: pg.font_idx,
            x: pg.x,
            y: pg.y,
            advance: pg.advance,
            bounds_min,
            bounds_max,
            curves,
            lyon_path,
        });
    }

    if glyphs.is_empty() || !block_min_x.is_finite() {
        block_min_x = 0.0;
        block_min_y = 0.0;
        block_max_x = 0.0;
        block_max_y = 0.0;
    }

    Ok(ShapedTextBlock {
        glyphs,
        bounds_min: [block_min_x, block_min_y],
        bounds_max: [block_max_x, block_max_y],
        line_count,
        char_count: global_char_idx,
        grapheme_count: unit_map.grapheme_count,
        word_count: unit_map.word_count,
        font_size: options.font_size,
    })
}

#[derive(Debug)]
pub(crate) struct TextUnitMap {
    grapheme_by_char: Vec<u32>,
    word_by_char: Vec<Option<u32>>,
    pub(crate) grapheme_count: u32,
    pub(crate) word_count: u32,
}

impl TextUnitMap {
    pub(crate) fn new(text: &str) -> Self {
        let mut grapheme_by_char = Vec::new();
        let mut word_by_char = Vec::new();
        let mut grapheme_count = 0u32;
        let mut word_count = 0u32;
        let mut in_word = false;
        let mut join_next = false;
        let mut regional_run = 0u8;

        for ch in text.chars() {
            let is_line_break = ch == '\n';

            let extend = !is_line_break
                && (is_grapheme_extend(ch) || join_next || ch == '\u{200d}');
            if !grapheme_by_char.is_empty() && !extend {
                if is_regional_indicator(ch) && regional_run % 2 == 1 {
                    // Pair regional indicators into one flag grapheme.
                } else {
                    grapheme_count = grapheme_count.saturating_add(1);
                }
            }
            if grapheme_by_char.is_empty() {
                grapheme_count = 1;
            }
            grapheme_by_char.push(grapheme_count.saturating_sub(1));

            if is_line_break || ch.is_whitespace() {
                word_by_char.push(None);
                in_word = false;
            } else {
                if !in_word {
                    word_count = word_count.saturating_add(1);
                    in_word = true;
                }
                word_by_char.push(Some(word_count - 1));
            }

            if is_line_break {
                join_next = false;
                regional_run = 0;
                continue;
            }

            if is_regional_indicator(ch) {
                regional_run = regional_run.saturating_add(1);
            } else if !is_grapheme_extend(ch) && ch != '\u{200d}' {
                regional_run = 0;
            }
            join_next = ch == '\u{200d}';
        }

        Self {
            grapheme_by_char,
            word_by_char,
            grapheme_count,
            word_count,
        }
    }

    pub(crate) fn grapheme_range(&self, start: u32, end: u32) -> (u32, u32) {
        let first = start as usize;
        let last = end.saturating_sub(1) as usize;
        let Some(&a) = self.grapheme_by_char.get(first) else {
            return (0, 0);
        };
        let b = self.grapheme_by_char.get(last).copied().unwrap_or(a);
        (a, b.saturating_add(1))
    }

    pub(crate) fn word_range(&self, start: u32, end: u32) -> (u32, u32) {
        let first = start as usize;
        let last = end.saturating_sub(1) as usize;
        let mut found = self.word_by_char.get(first).copied().flatten();
        let mut last_word = found;
        for value in self
            .word_by_char
            .get(first..=last.min(self.word_by_char.len().saturating_sub(1)))
            .into_iter()
            .flatten()
            .flatten()
        {
            found.get_or_insert(*value);
            last_word = Some(*value);
        }
        found.map_or((0, 0), |a| (a, last_word.unwrap_or(a).saturating_add(1)))
    }
}

fn cluster_char_ranges(text: &str, infos: &[harfrust::GlyphInfo]) -> Vec<(u32, u32)> {
    let mut byte_starts: Vec<usize> = infos
        .iter()
        .map(|info| (info.cluster as usize).min(text.len()))
        .collect();
    byte_starts.sort_unstable();
    byte_starts.dedup();

    infos
        .iter()
        .map(|info| {
            let start_byte = (info.cluster as usize).min(text.len());
            let end_byte = byte_starts
                .iter()
                .copied()
                .find(|&candidate| candidate > start_byte)
                .unwrap_or(text.len());
            let start_char = text[..start_byte].chars().count() as u32;
            let end_char = text[..end_byte].chars().count() as u32;
            (start_char, end_char.max(start_char.saturating_add(1)))
        })
        .collect()
}

fn is_regional_indicator(ch: char) -> bool {
    matches!(ch as u32, 0x1f1e6..=0x1f1ff)
}

fn is_grapheme_extend(ch: char) -> bool {
    matches!(ch as u32,
        0x0300..=0x036f | 0x0483..=0x0489 | 0x0591..=0x05bd | 0x05bf..=0x05bf |
        0x05c1..=0x05c2 | 0x05c4..=0x05c5 | 0x0610..=0x061a | 0x064b..=0x065f |
        0x0670..=0x0670 | 0x06d6..=0x06ed | 0x0711..=0x0711 | 0x0730..=0x074a |
        0x07a6..=0x07b0 | 0x07eb..=0x07f3 | 0x0816..=0x082d | 0x0859..=0x085b |
        0x08d3..=0x0903 | 0x093a..=0x093c | 0x093e..=0x094f | 0x0951..=0x0957 |
        0x0962..=0x0963 | 0x0981..=0x0983 | 0x09bc..=0x09bc | 0x09be..=0x09cd |
        0x0a01..=0x0a03 | 0x0a3c..=0x0a4d | 0x0abc..=0x0acd | 0x0b01..=0x0b03 |
        0x0b3c..=0x0b4d | 0x0c00..=0x0c04 | 0x0c3e..=0x0c56 | 0x0d00..=0x0d03 |
        0x0d3b..=0x0d4d | 0x0e31..=0x0e4e | 0x0f18..=0x0fbc | 0x102b..=0x103e |
        0x135d..=0x135f | 0x1712..=0x1734 | 0x17b4..=0x17d3 | 0x180b..=0x180d |
        0x1ab0..=0x1aff | 0x1dc0..=0x1dff | 0x20d0..=0x20ff | 0xfe00..=0xfe0f |
        0xfe20..=0xfe2f | 0x1f3fb..=0x1f3ff | 0xe0100..=0xe01ef)
}

fn split_into_font_runs<'a>(
    line: &'a str,
    fonts: &[PreparedFont],
) -> Vec<(usize, &'a str, usize)> {
    let mut runs = Vec::new();
    let mut cur_font = None;
    let mut run_start = 0usize;
    let mut char_count = 0usize;

    for (byte_idx, ch) in line.char_indices() {
        let chosen = pick_font_for_char(ch, fonts);
        match cur_font {
            None => {
                cur_font = Some(chosen);
                run_start = byte_idx;
                char_count = 1;
            }
            Some(prev) if prev == chosen => {
                char_count += 1;
            }
            Some(prev) => {
                runs.push((prev, &line[run_start..byte_idx], char_count));
                cur_font = Some(chosen);
                run_start = byte_idx;
                char_count = 1;
            }
        }
    }

    if let Some(last_font) = cur_font {
        runs.push((last_font, &line[run_start..], char_count));
    }
    runs
}

fn pick_font_for_char(ch: char, fonts: &[PreparedFont]) -> usize {
    if ch.is_whitespace() {
        return 0;
    }
    for (idx, pf) in fonts.iter().enumerate() {
        if pf.font.charmap().map_unicode(ch).is_some() {
            return idx;
        }
    }
    0
}

struct DualGlyphSink {
    offset_x: f32,
    offset_y: f32,
    curves: Vec<QuadraticCurve>,
    builder: lyon_tessellation::path::path::Builder,
    in_subpath: bool,
    subpath_start: [f32; 2],
    current_pt: [f32; 2],
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
}

impl DualGlyphSink {
    fn new(offset_x: f32, offset_y: f32) -> Self {
        Self {
            offset_x,
            offset_y,
            curves: Vec::new(),
            builder: LyonPath::builder(),
            in_subpath: false,
            subpath_start: [offset_x, offset_y],
            current_pt: [offset_x, offset_y],
            min_x: f32::INFINITY,
            min_y: f32::INFINITY,
            max_x: f32::NEG_INFINITY,
            max_y: f32::NEG_INFINITY,
        }
    }

    #[inline]
    fn tr(&self, x: f32, y: f32) -> [f32; 2] {
        [self.offset_x + x, self.offset_y + y]
    }

    #[inline]
    fn to_point(p: [f32; 2]) -> Point {
        point(p[0], p[1])
    }

    #[inline]
    fn expand_bounds(&mut self, p: [f32; 2]) {
        self.min_x = self.min_x.min(p[0]);
        self.min_y = self.min_y.min(p[1]);
        self.max_x = self.max_x.max(p[0]);
        self.max_y = self.max_y.max(p[1]);
    }

    fn push_line_curve(&mut self, p0: [f32; 2], p2: [f32; 2]) {
        let dx = p2[0] - p0[0];
        let dy = p2[1] - p0[1];
        if dx * dx + dy * dy <= 1e-10 {
            return;
        }
        let p1 = [0.5 * (p0[0] + p2[0]), 0.5 * (p0[1] + p2[1])];
        self.expand_bounds(p0);
        self.expand_bounds(p2);
        self.curves.push(QuadraticCurve { p0, p1, p2 });
    }

    fn push_quad_curve(&mut self, p0: [f32; 2], p1: [f32; 2], p2: [f32; 2]) {
        self.expand_bounds(p0);
        self.expand_bounds(p1);
        self.expand_bounds(p2);
        self.curves.push(QuadraticCurve { p0, p1, p2 });
    }

    fn finish(mut self) -> (Vec<QuadraticCurve>, LyonPath, [f32; 2], [f32; 2]) {
        if self.in_subpath {
            self.push_line_curve(self.current_pt, self.subpath_start);
            self.builder.close();
            self.in_subpath = false;
        }
        let bounds = if self.min_x.is_finite() {
            ([self.min_x, self.min_y], [self.max_x, self.max_y])
        } else {
            ([0.0, 0.0], [0.0, 0.0])
        };
        (self.curves, self.builder.build(), bounds.0, bounds.1)
    }
}

impl PathSink for DualGlyphSink {
    fn move_to(&mut self, x: f32, y: f32) {
        if self.in_subpath {
            self.push_line_curve(self.current_pt, self.subpath_start);
            self.builder.close();
        }
        let p = self.tr(x, y);
        self.builder.begin(Self::to_point(p));
        self.in_subpath = true;
        self.subpath_start = p;
        self.current_pt = p;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.tr(x, y);
        if !self.in_subpath {
            self.builder.begin(Self::to_point(p));
            self.in_subpath = true;
            self.subpath_start = p;
            self.current_pt = p;
            return;
        }
        self.push_line_curve(self.current_pt, p);
        self.builder.line_to(Self::to_point(p));
        self.current_pt = p;
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        let c = self.tr(cx0, cy0);
        let p = self.tr(x, y);
        if !self.in_subpath {
            self.builder.begin(Self::to_point(p));
            self.in_subpath = true;
            self.subpath_start = p;
            self.current_pt = p;
            return;
        }
        self.push_quad_curve(self.current_pt, c, p);
        self.builder
            .quadratic_bezier_to(Self::to_point(c), Self::to_point(p));
        self.current_pt = p;
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let c1 = self.tr(cx0, cy0);
        let c2 = self.tr(cx1, cy1);
        let p3 = self.tr(x, y);
        if !self.in_subpath {
            self.builder.begin(Self::to_point(p3));
            self.in_subpath = true;
            self.subpath_start = p3;
            self.current_pt = p3;
            return;
        }
        let p0 = self.current_pt;
        self.builder
            .cubic_bezier_to(Self::to_point(c1), Self::to_point(c2), Self::to_point(p3));

        // Subdivide cubic Bézier into 4 quadratic Bézier segments for Slug GPU evaluation.
        const SEGMENTS: usize = 4;
        let mut prev = p0;
        for i in 1..=SEGMENTS {
            let t0 = (i - 1) as f32 / SEGMENTS as f32;
            let t1 = i as f32 / SEGMENTS as f32;
            let sub_c1 = eval_cubic_deriv_ctrl(p0, c1, c2, p3, t0, t1, true);
            let sub_c2 = eval_cubic_deriv_ctrl(p0, c1, c2, p3, t0, t1, false);
            let end = eval_cubic(p0, c1, c2, p3, t1);
            let q_ctrl = [
                0.25 * (3.0 * (sub_c1[0] + sub_c2[0]) - (prev[0] + end[0])),
                0.25 * (3.0 * (sub_c1[1] + sub_c2[1]) - (prev[1] + end[1])),
            ];
            self.push_quad_curve(prev, q_ctrl, end);
            prev = end;
        }
        self.current_pt = p3;
    }

    fn close(&mut self) {
        if self.in_subpath {
            self.push_line_curve(self.current_pt, self.subpath_start);
            self.builder.close();
            self.in_subpath = false;
            self.current_pt = self.subpath_start;
        }
    }
}

#[inline]
fn eval_cubic(p0: [f32; 2], c1: [f32; 2], c2: [f32; 2], p3: [f32; 2], t: f32) -> [f32; 2] {
    let u = 1.0 - t;
    let u2 = u * u;
    let t2 = t * t;
    let w0 = u2 * u;
    let w1 = 3.0 * u2 * t;
    let w2 = 3.0 * u * t2;
    let w3 = t2 * t;
    [
        w0 * p0[0] + w1 * c1[0] + w2 * c2[0] + w3 * p3[0],
        w0 * p0[1] + w1 * c1[1] + w2 * c2[1] + w3 * p3[1],
    ]
}

#[inline]
fn eval_cubic_deriv_ctrl(
    p0: [f32; 2],
    c1: [f32; 2],
    c2: [f32; 2],
    p3: [f32; 2],
    t0: f32,
    t1: f32,
    first: bool,
) -> [f32; 2] {
    let dt = (t1 - t0) / 3.0;
    let t = if first { t0 } else { t1 };
    let u = 1.0 - t;
    let deriv = [
        3.0 * (u * u * (c1[0] - p0[0]) + 2.0 * u * t * (c2[0] - c1[0]) + t * t * (p3[0] - c2[0])),
        3.0 * (u * u * (c1[1] - p0[1]) + 2.0 * u * t * (c2[1] - c1[1]) + t * t * (p3[1] - c2[1])),
    ];
    let pt = eval_cubic(p0, c1, c2, p3, t);
    if first {
        [pt[0] + deriv[0] * dt, pt[1] + deriv[1] * dt]
    } else {
        [pt[0] - deriv[0] * dt, pt[1] - deriv[1] * dt]
    }
}

#[cfg(test)]
mod selector_tests {
    use super::{TextUnitMap, evaluate_text_animator, evaluate_text_animator_with_total};
    use frameshift_schema::{TextAnimatorDef, TextRangeShape, TextSelectorUnit};
    use frameshift_time::Flicks;

    #[test]
    fn text_units_cover_combining_emoji_and_line_breaks() {
        let units = TextUnitMap::new("A\u{301} B\n🇺🇸👩‍🚀");

        assert_eq!(units.grapheme_count, 6);
        assert_eq!(units.word_count, 3);
        assert_eq!(units.grapheme_range(0, 2), (0, 1));
        assert_eq!(units.word_range(0, 2), (0, 1));
        assert_eq!(units.grapheme_range(4, 5), (3, 4));
        assert_eq!(units.word_range(4, 5), (0, 0));
        assert_eq!(units.grapheme_range(5, 7), (4, 5));
        assert_eq!(units.word_range(7, 10), (2, 3));

        let multiline = TextUnitMap::new("A\nB");
        assert_eq!(multiline.grapheme_range(2, 3), (2, 3));
        assert_eq!(multiline.word_range(2, 3), (1, 2));
    }

    #[test]
    fn word_selector_applies_range_and_per_word_stagger() {
        let animator = TextAnimatorDef {
            unit: TextSelectorUnit::Word,
            range_start: 1,
            range_end: Some(3),
            start_tick: 0,
            duration_ticks: 100,
            delay_ticks: 20,
            position_from_norm: [0.2, -0.1],
            scale_from: 0.5,
            opacity_from: 0.0,
            ..Default::default()
        };

        let unselected = evaluate_text_animator(
            Some(&animator),
            (0, 1),
            (0, 1),
            Flicks(0),
        );
        assert_eq!(unselected.opacity, 1.0);
        assert_eq!(unselected.rotation_deg, 0.0);
        assert_eq!(unselected.tracking_em, 0.0);

        let selected_first = evaluate_text_animator(
            Some(&animator),
            (1, 2),
            (1, 2),
            Flicks(0),
        );
        assert_eq!(selected_first.opacity, 0.0);

        let selected_second = evaluate_text_animator(
            Some(&animator),
            (2, 3),
            (2, 3),
            Flicks(70),
        );
        assert!((selected_second.opacity - 0.5).abs() < 1e-6);
        assert!((selected_second.scale - 0.75).abs() < 1e-6);
        assert!((selected_second.position_norm[0] - 0.1).abs() < 1e-6);
        assert_eq!(selected_second.rotation_deg, 0.0);
        assert_eq!(selected_second.tracking_em, 0.0);
    }

    #[test]
    fn text_range_shapes_easing_rotation_tracking_and_randomize_work() {
        let base = TextAnimatorDef {
            unit: TextSelectorUnit::Grapheme,
            range_start: 0,
            range_end: Some(5),
            start_tick: 0,
            duration_ticks: 100,
            delay_ticks: 0,
            position_from_norm: [0.0, 0.2],
            scale_from: 2.0,
            opacity_from: 0.0,
            rotation_deg_from: -30.0,
            tracking_em_from: 0.4,
            ..Default::default()
        };

        let eval_shape = |shape: TextRangeShape, idx: u32, tick: i64| {
            let mut anim = base.clone();
            anim.shape = shape;
            evaluate_text_animator(Some(&anim), (idx, idx + 1), (0, 1), Flicks(tick))
        };

        // Square at t=0 has full rotation & tracking from-values
        let sq0 = eval_shape(TextRangeShape::Square, 0, 0);
        assert!((sq0.rotation_deg - (-30.0)).abs() < 1e-6);
        assert!((sq0.tracking_em - 0.4).abs() < 1e-6);
        assert!((sq0.opacity - 0.0).abs() < 1e-6);

        // RampUp increases weight from start to end
        let ru_first = eval_shape(TextRangeShape::RampUp, 0, 0);
        let ru_last = eval_shape(TextRangeShape::RampUp, 4, 0);
        assert!(ru_last.rotation_deg.abs() > ru_first.rotation_deg.abs());
        assert!(ru_last.tracking_em > ru_first.tracking_em);

        // RampDown decreases weight from start to end
        let rd_first = eval_shape(TextRangeShape::RampDown, 0, 0);
        let rd_last = eval_shape(TextRangeShape::RampDown, 4, 0);
        assert!(rd_first.rotation_deg.abs() > rd_last.rotation_deg.abs());

        // Triangle and Smooth peak at center unit (idx = 2 of 0..5)
        let tri_edge = eval_shape(TextRangeShape::Triangle, 0, 0);
        let tri_mid = eval_shape(TextRangeShape::Triangle, 2, 0);
        assert!(tri_mid.rotation_deg.abs() > tri_edge.rotation_deg.abs());
        assert!((tri_mid.rotation_deg - (-30.0)).abs() < 1e-5);

        let sm_edge0 = eval_shape(TextRangeShape::Smooth, 0, 0);
        let sm_off = eval_shape(TextRangeShape::Smooth, 1, 0);
        let sm_mid = eval_shape(TextRangeShape::Smooth, 2, 0);
        assert!(sm_mid.rotation_deg.abs() > sm_off.rotation_deg.abs());
        assert!((sm_edge0.rotation_deg - tri_edge.rotation_deg).abs() < 5.0);
        // Smooth vs Triangle differ off-center (idx 1 -> u=0.3 -> Tri=0.6, Smooth=sin^2(0.3*pi)~0.6545)
        let tri_off = eval_shape(TextRangeShape::Triangle, 1, 0);
        assert!((sm_off.rotation_deg - tri_off.rotation_deg).abs() > 0.5);

        // Easing shifts mid-animation values while preserving endpoints (t=0 and t=100)
        let mut eased = base.clone();
        eased.ease_high = 85.0;
        eased.ease_low = 0.0;
        let lin_mid = eval_shape(TextRangeShape::Square, 0, 25);
        let eased_mid = evaluate_text_animator(Some(&eased), (0, 1), (0, 1), Flicks(25));
        assert_ne!(lin_mid.opacity, eased_mid.opacity);
        let eased_end = evaluate_text_animator(Some(&eased), (0, 1), (0, 1), Flicks(100));
        assert!((eased_end.opacity - 1.0).abs() < 1e-6);
        assert!(eased_end.rotation_deg.abs() < 1e-6);

        // Deterministic randomize_seed reorders stagger reproducibly
        let mut rand_anim = base;
        rand_anim.delay_ticks = 25;
        rand_anim.randomize_seed = Some(42);
        let r0_a = evaluate_text_animator_with_total(
            Some(&rand_anim),
            (0, 1),
            (0, 1),
            Some(5),
            Flicks(60),
        );
        let r0_b = evaluate_text_animator_with_total(
            Some(&rand_anim),
            (0, 1),
            (0, 1),
            Some(5),
            Flicks(60),
        );
        assert_eq!(r0_a, r0_b);
        rand_anim.randomize_seed = Some(99);
        let r0_diff = evaluate_text_animator_with_total(
            Some(&rand_anim),
            (0, 1),
            (0, 1),
            Some(5),
            Flicks(60),
        );
        assert_ne!(r0_a, r0_diff);
    }
}
