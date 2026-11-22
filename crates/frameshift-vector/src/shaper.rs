use std::sync::Arc;

use frameshift_schema::{TextAlign2D, WritingMode2D};
use harfrust::{Buffer, Direction, Feature, ShapeOptions, ShaperFont, Tag};
use lyon_tessellation::math::{Point, point};
use lyon_tessellation::path::Path as LyonPath;
use skrifa::font::{Blob, Font, Variation};
use skrifa::{DrawMemory, PathSink, ScalerFont};

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
        glyph_id: u32,
        font_idx: usize,
        x: f32,
        y: f32,
        advance: f32,
    }

    let mut placed_glyphs = Vec::new();
    let mut global_char_idx = 0u32;

    match options.writing_mode {
        WritingMode2D::HorizontalTb => {
            let baseline_0 =
                ((line_count - 1) as f32 * 0.5) * line_step - primary.cap_height * 0.5;

            for (line_idx, line) in lines.iter().enumerate() {
                if line.is_empty() {
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

                    for i in 0..run_glyph_count {
                        let info = infos[i];
                        let pos = positions[i];
                        let gx = pen_x + (pos.x_offset as f32) * scale;
                        let gy = pen_y + (pos.y_offset as f32) * scale;
                        let adv = (pos.x_advance as f32) * scale;
                        let is_very_last = run_idx + 1 == run_count && i + 1 == run_glyph_count;
                        let extra_track = if is_very_last { 0.0 } else { tracking_px };

                        line_placed.push(RawPlacedGlyph {
                            char_index: global_char_idx + i as u32,
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
            }
        }
        WritingMode2D::VerticalRl => {
            // Right-to-left vertical columns: column 0 is rightmost.
            let col_start_x = ((line_count - 1) as f32 * 0.5) * line_step;

            for (col_idx, line) in lines.iter().enumerate() {
                if line.is_empty() {
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
                        let is_very_last = run_idx + 1 == run_count && i + 1 == run_glyph_count;
                        let extra_track = if is_very_last { 0.0 } else { tracking_px };

                        col_placed.push(RawPlacedGlyph {
                            char_index: global_char_idx + i as u32,
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
        font_size: options.font_size,
    })
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
