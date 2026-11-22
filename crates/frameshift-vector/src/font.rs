use crate::canvas::sd_segment;
use std::sync::OnceLock;

/// Native bitmap cell width in glyph grid units.
pub const GLYPH_CELL_WIDTH: usize = 8;
/// Native bitmap cell height in glyph grid units (cap rows 1..=8, descenders 9..=10).
pub const GLYPH_CELL_HEIGHT: usize = 12;
/// Nominal cap height in glyph grid units (rows 1..=8).
pub const GLYPH_CAP_HEIGHT: f32 = 8.0;
/// Cap top Y coordinate in glyph grid units.
pub const GLYPH_CAP_TOP: f32 = 1.0;
/// Baseline Y coordinate in glyph grid units (bottom of row 8).
pub const GLYPH_BASELINE: f32 = 9.0;

const FIRST_ASCII: u8 = 0x20;
const LAST_ASCII: u8 = 0x7E;
const GLYPH_COUNT: usize = (LAST_ASCII - FIRST_ASCII + 1) as usize;

/// Oversampled SDF grid parameters (`[-4, 12] x [-4, 16]` at 4x = `64 x 80`).
const SDF_PAD: f32 = 4.0;
const SDF_SCALE: usize = 4;
const SDF_WIDTH: usize = (GLYPH_CELL_WIDTH + 8) * SDF_SCALE;
const SDF_HEIGHT: usize = (GLYPH_CELL_HEIGHT + 8) * SDF_SCALE;

/// Raw 8x12 glyph definition with proportional horizontal metrics.
#[derive(Debug, Clone, Copy)]
pub struct BitmapGlyph {
    pub rows: [u8; GLYPH_CELL_HEIGHT],
    /// Leftmost active column in `[0..8]` (or 0 for space).
    pub left_col: u8,
    /// Tight ink width in columns (`0` for space).
    pub ink_width: u8,
    /// Proportional horizontal advance in glyph grid units (includes inter-glyph base gap).
    pub advance: f32,
}

impl BitmapGlyph {
    /// Returns true if pixel `(col, row)` is active in the 8x12 glyph mask.
    #[inline]
    pub fn pixel(&self, col: usize, row: usize) -> bool {
        if col >= GLYPH_CELL_WIDTH || row >= GLYPH_CELL_HEIGHT {
            return false;
        }
        (self.rows[row] & (0x80 >> col)) != 0
    }
}

#[derive(Debug, Clone, Copy)]
enum SdfPrim {
    Box {
        cx: f32,
        cy: f32,
        hx: f32,
        hy: f32,
    },
    RoundBox {
        cx: f32,
        cy: f32,
        hx: f32,
        hy: f32,
        r: f32,
    },
    Capsule {
        a: [f32; 2],
        b: [f32; 2],
        r: f32,
    },
}

impl SdfPrim {
    #[inline]
    fn distance(&self, x: f32, y: f32) -> f32 {
        match *self {
            SdfPrim::Box { cx, cy, hx, hy } => {
                let qx = (x - cx).abs() - hx;
                let qy = (y - cy).abs() - hy;
                let ox = qx.max(0.0);
                let oy = qy.max(0.0);
                (ox * ox + oy * oy).sqrt() + qx.max(qy).min(0.0)
            }
            SdfPrim::RoundBox { cx, cy, hx, hy, r } => {
                let rr = r.max(0.0).min(hx).min(hy);
                let qx = (x - cx).abs() - (hx - rr);
                let qy = (y - cy).abs() - (hy - rr);
                let ox = qx.max(0.0);
                let oy = qy.max(0.0);
                (ox * ox + oy * oy).sqrt() + qx.max(qy).min(0.0) - rr
            }
            SdfPrim::Capsule { a, b, r } => sd_segment([x, y], a, b) - r,
        }
    }
}

#[inline]
fn smin(a: f32, b: f32, k: f32) -> f32 {
    if k <= 0.0 {
        return a.min(b);
    }
    let h = (k - (a - b).abs()).max(0.0) / k;
    a.min(b) - h * h * k * 0.25
}

const SDF_CORNER_SMOOTH_K: f32 = 0.12;

/// Precomputed signed-distance-field tile for a single ASCII glyph.
#[derive(Debug, Clone)]
pub struct GlyphSdf {
    pub bitmap: BitmapGlyph,
    prims: Vec<SdfPrim>,
    sdf: Vec<f32>,
}

impl GlyphSdf {
    /// Evaluates the signed distance (in glyph grid units; `<= 0` inside ink, `> 0` outside)
    /// at continuous glyph-local coordinates `(gx, gy)`.
    #[inline]
    pub fn sample(&self, gx: f32, gy: f32) -> f32 {
        let sx = (gx + SDF_PAD) * (SDF_SCALE as f32) - 0.5;
        let sy = (gy + SDF_PAD) * (SDF_SCALE as f32) - 0.5;
        if sx < 0.0
            || sy < 0.0
            || sx >= (SDF_WIDTH - 1) as f32
            || sy >= (SDF_HEIGHT - 1) as f32
        {
            return self.eval_exact(gx, gy);
        }

        let x0 = sx.floor() as usize;
        let y0 = sy.floor() as usize;
        let tx = sx - (x0 as f32);
        let ty = sy - (y0 as f32);

        let row0 = y0 * SDF_WIDTH + x0;
        let row1 = (y0 + 1) * SDF_WIDTH + x0;
        let d00 = self.sdf[row0];
        let d10 = self.sdf[row0 + 1];
        let d01 = self.sdf[row1];
        let d11 = self.sdf[row1 + 1];

        let top = d00 + (d10 - d00) * tx;
        let bot = d01 + (d11 - d01) * tx;
        top + (bot - top) * ty
    }

    /// Evaluates the exact analytical SDF from vector primitives at `(gx, gy)` without grid quantization.
    #[inline]
    pub fn eval_exact(&self, gx: f32, gy: f32) -> f32 {
        eval_prims_exact(&self.prims, gx, gy)
    }
}

#[inline]
fn eval_prims_exact(prims: &[SdfPrim], gx: f32, gy: f32) -> f32 {
    if prims.is_empty() {
        return 100.0;
    }
    let mut d = f32::INFINITY;
    for prim in prims {
        let pd = prim.distance(gx, gy);
        d = if d.is_finite() {
            smin(d, pd, SDF_CORNER_SMOOTH_K)
        } else {
            pd
        };
    }
    d
}

static SDF_ATLAS: OnceLock<Vec<GlyphSdf>> = OnceLock::new();

/// Returns the deterministic `GlyphSdf` for an ASCII printable character (unsupported chars map to `'?'` or `' '`).
pub fn glyph_sdf(ch: char) -> &'static GlyphSdf {
    let atlas = SDF_ATLAS.get_or_init(build_sdf_atlas);
    let idx = ascii_index(ch);
    &atlas[idx]
}

/// Returns the deterministic `BitmapGlyph` for an ASCII printable character.
pub fn bitmap_glyph(ch: char) -> BitmapGlyph {
    glyph_sdf(ch).bitmap
}

fn ascii_index(ch: char) -> usize {
    let code = ch as u32;
    if (FIRST_ASCII as u32..=LAST_ASCII as u32).contains(&code) {
        (code - FIRST_ASCII as u32) as usize
    } else if ch == '\t' {
        0 // space
    } else {
        (b'?' - FIRST_ASCII) as usize
    }
}

fn build_sdf_atlas() -> Vec<GlyphSdf> {
    let mut out = Vec::with_capacity(GLYPH_COUNT);
    for (idx, raw) in RAW_ASCII_GLYPHS.into_iter().enumerate() {
        let ch = (FIRST_ASCII + idx as u8) as char;
        let bitmap = make_bitmap_glyph(raw);
        let prims = build_primitives(ch, &bitmap);
        let mut sdf = Vec::with_capacity(SDF_WIDTH * SDF_HEIGHT);
        let inv_scale = 1.0 / (SDF_SCALE as f32);
        for sy in 0..SDF_HEIGHT {
            let gy = (sy as f32 + 0.5) * inv_scale - SDF_PAD;
            for sx in 0..SDF_WIDTH {
                let gx = (sx as f32 + 0.5) * inv_scale - SDF_PAD;
                sdf.push(eval_prims_exact(&prims, gx, gy));
            }
        }
        out.push(GlyphSdf { bitmap, prims, sdf });
    }
    out
}

fn make_bitmap_glyph(rows: [u8; GLYPH_CELL_HEIGHT]) -> BitmapGlyph {
    let mut min_col = GLYPH_CELL_WIDTH;
    let mut max_col = 0usize;
    let mut any = false;

    for &row in &rows {
        for col in 0..GLYPH_CELL_WIDTH {
            if (row & (0x80 >> col)) != 0 {
                any = true;
                if col < min_col {
                    min_col = col;
                }
                if col > max_col {
                    max_col = col;
                }
            }
        }
    }

    if !any {
        return BitmapGlyph {
            rows,
            left_col: 0,
            ink_width: 0,
            advance: 4.0,
        };
    }

    let ink_w = (max_col - min_col + 1) as u8;
    BitmapGlyph {
        rows,
        left_col: min_col as u8,
        ink_width: ink_w,
        advance: (ink_w as f32) + 1.25,
    }
}

fn build_primitives(ch: char, g: &BitmapGlyph) -> Vec<SdfPrim> {
    if let Some(prims) = vector_glyph_prims(ch) {
        return prims;
    }
    extract_sdf_prims(g)
}

#[inline]
fn add_box(prims: &mut Vec<SdfPrim>, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) {
    prims.push(SdfPrim::RoundBox {
        cx: (x0 + x1) * 0.5,
        cy: (y0 + y1) * 0.5,
        hx: (x1 - x0) * 0.5,
        hy: (y1 - y0) * 0.5,
        r,
    });
}

#[inline]
fn add_cap(prims: &mut Vec<SdfPrim>, a: [f32; 2], b: [f32; 2], r: f32) {
    prims.push(SdfPrim::Capsule { a, b, r });
}

#[allow(clippy::too_many_arguments)]
fn add_arc(
    prims: &mut Vec<SdfPrim>,
    cx: f32,
    cy: f32,
    rx: f32,
    ry: f32,
    theta0: f32,
    theta1: f32,
    steps: usize,
    r: f32,
) {
    let steps = steps.max(1);
    let mut prev = [cx + rx * theta0.cos(), cy + ry * theta0.sin()];
    for i in 1..=steps {
        let t = (i as f32) / (steps as f32);
        let th = theta0 + (theta1 - theta0) * t;
        let next = [cx + rx * th.cos(), cy + ry * th.sin()];
        add_cap(prims, prev, next, r);
        prev = next;
    }
}

fn vector_glyph_prims(ch: char) -> Option<Vec<SdfPrim>> {
    use std::f32::consts::PI;
    let mut p = Vec::new();
    match ch {
        '-' => add_box(&mut p, 0.05, 4.18, 5.95, 5.82, 0.20),
        '/' => add_cap(&mut p, [6.15, 1.75], [0.85, 8.25], 0.74),
        '0' => {
            add_cap(&mut p, [0.76, 3.20], [0.76, 6.80], 0.72);
            add_cap(&mut p, [6.24, 3.20], [6.24, 6.80], 0.72);
            add_arc(&mut p, 3.50, 3.20, 2.74, 1.48, PI, 2.0 * PI, 6, 0.72);
            add_arc(&mut p, 3.50, 6.80, 2.74, 1.48, 0.0, PI, 6, 0.72);
            add_cap(&mut p, [1.85, 6.85], [5.15, 3.15], 0.44);
        }
        '1' => {
            add_box(&mut p, 2.05, 1.05, 3.65, 8.95, 0.14);
            add_cap(&mut p, [0.75, 2.95], [2.85, 1.68], 0.64);
            add_box(&mut p, 0.05, 7.68, 5.95, 8.95, 0.14);
        }
        '2' => {
            add_arc(&mut p, 3.50, 3.20, 2.68, 1.52, 1.08 * PI, 2.02 * PI, 7, 0.68);
            add_cap(&mut p, [6.12, 3.50], [0.85, 8.26], 0.68);
            add_box(&mut p, 0.06, 7.66, 6.94, 8.95, 0.14);
        }
        '3' => {
            add_arc(&mut p, 3.40, 3.25, 2.70, 1.57, 1.15 * PI, 2.50 * PI, 7, 0.68);
            add_cap(&mut p, [2.70, 4.82], [3.60, 4.82], 0.62);
            add_arc(&mut p, 3.40, 6.57, 2.80, 1.75, -0.48 * PI, 0.86 * PI, 7, 0.68);
        }
        '4' => {
            add_box(&mut p, 3.95, 1.05, 5.55, 8.95, 0.14);
            add_cap(&mut p, [4.65, 1.72], [0.74, 5.65], 0.68);
            add_box(&mut p, 0.06, 5.05, 6.94, 6.32, 0.14);
        }
        '5' => {
            add_box(&mut p, 0.06, 1.05, 6.94, 2.28, 0.14);
            add_box(&mut p, 0.06, 1.05, 1.46, 4.95, 0.14);
            add_cap(&mut p, [0.76, 4.85], [2.40, 4.85], 0.65);
            add_arc(&mut p, 3.35, 6.50, 2.85, 1.82, -0.68 * PI, 0.86 * PI, 8, 0.68);
        }
        '6' => {
            add_arc(&mut p, 3.80, 5.20, 3.04, 3.52, 1.66 * PI, PI, 6, 0.68);
            add_cap(&mut p, [0.76, 5.20], [0.76, 6.55], 0.68);
            add_arc(&mut p, 3.50, 6.55, 2.74, 1.77, 0.0, 2.0 * PI, 10, 0.68);
        }
        '7' => {
            add_box(&mut p, 0.05, 1.05, 6.95, 2.34, 0.14);
            add_cap(&mut p, [6.15, 1.82], [2.65, 8.26], 0.72);
        }
        '8' => {
            add_arc(&mut p, 3.50, 3.15, 2.55, 1.49, 0.0, 2.0 * PI, 10, 0.66);
            add_arc(&mut p, 3.50, 6.65, 2.80, 1.69, 0.0, 2.0 * PI, 10, 0.66);
        }
        '9' => {
            add_arc(&mut p, 3.50, 3.45, 2.74, 1.77, 0.0, 2.0 * PI, 10, 0.68);
            add_cap(&mut p, [6.24, 3.45], [6.24, 6.15], 0.68);
            add_arc(&mut p, 3.50, 6.15, 2.74, 2.17, 0.0, 0.82 * PI, 6, 0.68);
        }
        'A' => {
            add_cap(&mut p, [3.50, 1.78], [0.80, 8.22], 0.76);
            add_cap(&mut p, [3.50, 1.78], [6.20, 8.22], 0.76);
            add_cap(&mut p, [1.80, 5.75], [5.20, 5.75], 0.56);
        }
        'B' => {
            add_box(&mut p, 0.05, 1.05, 1.55, 8.95, 0.14);
            add_cap(&mut p, [0.80, 1.68], [4.10, 1.68], 0.68);
            add_arc(&mut p, 4.10, 3.26, 2.00, 1.58, -0.5 * PI, 0.5 * PI, 6, 0.68);
            add_cap(&mut p, [0.80, 4.84], [4.10, 4.84], 0.62);
            add_arc(&mut p, 4.10, 6.58, 2.18, 1.74, -0.5 * PI, 0.5 * PI, 6, 0.68);
            add_cap(&mut p, [0.80, 8.32], [4.10, 8.32], 0.68);
        }
        'C' => {
            add_cap(&mut p, [0.76, 3.20], [0.76, 6.80], 0.72);
            add_arc(&mut p, 3.50, 3.20, 2.74, 1.48, PI, 1.82 * PI, 5, 0.72);
            add_arc(&mut p, 3.50, 6.80, 2.74, 1.48, 0.18 * PI, PI, 5, 0.72);
        }
        'D' => {
            add_box(&mut p, 0.05, 1.05, 1.55, 8.95, 0.14);
            add_cap(&mut p, [0.80, 1.72], [3.50, 1.72], 0.72);
            add_cap(&mut p, [0.80, 8.28], [3.50, 8.28], 0.72);
            add_arc(&mut p, 3.50, 3.60, 2.74, 1.88, -0.5 * PI, 0.0, 5, 0.72);
            add_cap(&mut p, [6.24, 3.60], [6.24, 6.40], 0.72);
            add_arc(&mut p, 3.50, 6.40, 2.74, 1.88, 0.0, 0.5 * PI, 5, 0.72);
        }
        'E' => {
            add_box(&mut p, 0.05, 1.05, 1.65, 8.95, 0.14);
            add_box(&mut p, 0.05, 1.05, 6.95, 2.28, 0.14);
            add_box(&mut p, 0.05, 4.25, 5.95, 5.45, 0.14);
            add_box(&mut p, 0.05, 7.72, 6.95, 8.95, 0.14);
        }
        'F' => {
            add_box(&mut p, 0.05, 1.05, 1.65, 8.95, 0.14);
            add_box(&mut p, 0.05, 1.05, 6.95, 2.28, 0.14);
            add_box(&mut p, 0.05, 4.25, 5.95, 5.45, 0.14);
        }
        'G' => {
            add_cap(&mut p, [0.76, 3.20], [0.76, 6.80], 0.72);
            add_arc(&mut p, 3.50, 3.20, 2.74, 1.48, PI, 1.82 * PI, 5, 0.72);
            add_arc(&mut p, 3.50, 6.80, 2.74, 1.48, 0.0, PI, 6, 0.72);
            add_cap(&mut p, [6.24, 6.80], [6.24, 5.15], 0.72);
            add_cap(&mut p, [6.24, 5.15], [3.75, 5.15], 0.62);
        }
        'H' => {
            add_box(&mut p, 0.05, 1.05, 1.65, 8.95, 0.14);
            add_box(&mut p, 5.35, 1.05, 6.95, 8.95, 0.14);
            add_box(&mut p, 0.05, 4.35, 6.95, 5.65, 0.14);
        }
        'I' => {
            add_box(&mut p, 0.05, 1.05, 5.95, 2.25, 0.14);
            add_box(&mut p, 2.20, 1.05, 3.80, 8.95, 0.14);
            add_box(&mut p, 0.05, 7.75, 5.95, 8.95, 0.14);
        }
        'J' => {
            add_box(&mut p, 1.85, 1.05, 6.95, 2.25, 0.14);
            add_cap(&mut p, [5.15, 1.75], [5.15, 6.60], 0.70);
            add_arc(&mut p, 2.95, 6.60, 2.20, 1.70, 0.0, 0.85 * PI, 6, 0.70);
        }
        'K' => {
            add_box(&mut p, 0.05, 1.05, 1.65, 8.95, 0.14);
            add_cap(&mut p, [1.45, 5.00], [6.15, 1.75], 0.74);
            add_cap(&mut p, [2.45, 4.35], [6.18, 8.24], 0.74);
        }
        'L' => {
            add_box(&mut p, 0.05, 1.05, 1.65, 8.95, 0.14);
            add_box(&mut p, 0.05, 7.72, 6.95, 8.95, 0.14);
        }
        'M' => {
            add_box(&mut p, 0.05, 1.05, 1.55, 8.95, 0.14);
            add_box(&mut p, 6.45, 1.05, 7.95, 8.95, 0.14);
            add_cap(&mut p, [0.85, 1.78], [4.00, 5.55], 0.70);
            add_cap(&mut p, [7.15, 1.78], [4.00, 5.55], 0.70);
        }
        'N' => {
            add_box(&mut p, 0.05, 1.05, 1.55, 8.95, 0.14);
            add_box(&mut p, 5.45, 1.05, 6.95, 8.95, 0.14);
            add_cap(&mut p, [0.85, 1.75], [6.15, 8.25], 0.72);
        }
        'O' => {
            add_cap(&mut p, [0.76, 3.20], [0.76, 6.80], 0.72);
            add_cap(&mut p, [6.24, 3.20], [6.24, 6.80], 0.72);
            add_arc(&mut p, 3.50, 3.20, 2.74, 1.48, PI, 2.0 * PI, 6, 0.72);
            add_arc(&mut p, 3.50, 6.80, 2.74, 1.48, 0.0, PI, 6, 0.72);
        }
        'P' => {
            add_box(&mut p, 0.05, 1.05, 1.55, 8.95, 0.14);
            add_cap(&mut p, [0.80, 1.70], [4.00, 1.70], 0.70);
            add_arc(&mut p, 4.00, 3.45, 2.24, 1.75, -0.5 * PI, 0.5 * PI, 6, 0.70);
            add_cap(&mut p, [0.80, 5.20], [4.00, 5.20], 0.66);
        }
        'Q' => {
            add_cap(&mut p, [0.76, 3.20], [0.76, 6.80], 0.72);
            add_cap(&mut p, [6.24, 3.20], [6.24, 6.80], 0.72);
            add_arc(&mut p, 3.50, 3.20, 2.74, 1.48, PI, 2.0 * PI, 6, 0.72);
            add_arc(&mut p, 3.50, 6.80, 2.74, 1.48, 0.0, PI, 6, 0.72);
            add_cap(&mut p, [4.20, 6.20], [6.25, 8.28], 0.68);
        }
        'R' => {
            add_box(&mut p, 0.05, 1.05, 1.55, 8.95, 0.14);
            add_cap(&mut p, [0.80, 1.70], [4.00, 1.70], 0.70);
            add_arc(&mut p, 4.00, 3.40, 2.24, 1.70, -0.5 * PI, 0.5 * PI, 6, 0.70);
            add_cap(&mut p, [0.80, 5.10], [4.00, 5.10], 0.66);
            add_cap(&mut p, [3.65, 5.10], [6.18, 8.24], 0.74);
        }
        'S' => {
            add_arc(&mut p, 3.50, 3.25, 2.65, 1.57, 1.85 * PI, 0.56 * PI, 8, 0.68);
            add_cap(&mut p, [2.84, 4.77], [4.35, 5.15], 0.66);
            add_arc(&mut p, 3.50, 6.72, 2.70, 1.60, -0.36 * PI, 0.86 * PI, 8, 0.68);
        }
        'T' => {
            add_box(&mut p, 0.05, 1.05, 6.95, 2.28, 0.14);
            add_box(&mut p, 2.70, 1.05, 4.30, 8.95, 0.14);
        }
        'U' => {
            add_box(&mut p, 0.04, 1.04, 1.48, 6.45, 0.14);
            add_box(&mut p, 5.52, 1.04, 6.96, 6.45, 0.14);
            add_cap(&mut p, [0.76, 5.90], [0.76, 6.60], 0.72);
            add_cap(&mut p, [6.24, 5.90], [6.24, 6.60], 0.72);
            add_arc(&mut p, 3.50, 6.60, 2.74, 1.68, 0.0, PI, 6, 0.72);
        }
        'V' => {
            add_cap(&mut p, [0.82, 1.78], [3.50, 8.22], 0.76);
            add_cap(&mut p, [6.18, 1.78], [3.50, 8.22], 0.76);
        }
        'W' => {
            add_cap(&mut p, [0.78, 1.78], [2.20, 8.22], 0.72);
            add_cap(&mut p, [2.20, 8.22], [4.00, 3.60], 0.68);
            add_cap(&mut p, [4.00, 3.60], [5.80, 8.22], 0.68);
            add_cap(&mut p, [7.22, 1.78], [5.80, 8.22], 0.72);
        }
        'X' => {
            add_cap(&mut p, [0.82, 1.78], [6.18, 8.22], 0.76);
            add_cap(&mut p, [6.18, 1.78], [0.82, 8.22], 0.76);
        }
        'Y' => {
            add_cap(&mut p, [0.82, 1.78], [3.50, 5.10], 0.75);
            add_cap(&mut p, [6.18, 1.78], [3.50, 5.10], 0.75);
            add_box(&mut p, 2.74, 4.90, 4.26, 8.95, 0.14);
        }
        'Z' => {
            add_box(&mut p, 0.05, 1.05, 6.95, 2.28, 0.14);
            add_cap(&mut p, [6.12, 1.82], [0.88, 8.20], 0.72);
            add_box(&mut p, 0.05, 7.72, 6.95, 8.95, 0.14);
        }
        _ => return None,
    }
    Some(p)
}

#[derive(Clone, Copy)]
struct HorizRun {
    y: usize,
    start: usize,
    end: usize,
    stepped: bool,
}

fn extract_sdf_prims(g: &BitmapGlyph) -> Vec<SdfPrim> {
    let mut prims = Vec::new();
    let inset = 0.04f32;

    // Collect horizontal runs across all scanlines to detect diagonal step runs.
    let mut runs: Vec<HorizRun> = Vec::new();
    for y in 0..GLYPH_CELL_HEIGHT {
        let mut x = 0usize;
        while x < GLYPH_CELL_WIDTH {
            if g.pixel(x, y) {
                let start = x;
                while x + 1 < GLYPH_CELL_WIDTH && g.pixel(x + 1, y) {
                    x += 1;
                }
                runs.push(HorizRun {
                    y,
                    start,
                    end: x,
                    stepped: false,
                });
            }
            x += 1;
        }
    }

    // 1. Detect diagonal 1x1 / 2x1 step runs across adjacent scanlines and emit angled Capsules.
    for i in 0..runs.len() {
        let a = runs[i];
        let a_w = a.end - a.start + 1;
        if a_w > 2 {
            continue;
        }
        for j in (i + 1)..runs.len() {
            let b = runs[j];
            if b.y != a.y + 1 {
                if b.y > a.y + 1 {
                    break;
                }
                continue;
            }
            let b_w = b.end - b.start + 1;
            if b_w > 2 {
                continue;
            }
            let shift_right = b.start == a.start + 1 && b.end <= a.end + 1;
            let shift_left = a.start == b.start + 1 && a.end <= b.end + 1;
            if shift_right || shift_left {
                runs[i].stepped = true;
                runs[j].stepped = true;
                let ax = (a.start + a.end + 1) as f32 * 0.5;
                let ay = a.y as f32 + 0.5;
                let bx = (b.start + b.end + 1) as f32 * 0.5;
                let by = b.y as f32 + 0.5;
                let r = ((a_w.min(b_w) as f32) * 0.44).clamp(0.44, 0.72);
                prims.push(SdfPrim::Capsule {
                    a: [ax, ay],
                    b: [bx, by],
                    r,
                });
            }
        }
    }

    // 2. Horizontal runs not superseded by diagonal step capsules
    for run in &runs {
        if run.stepped {
            continue;
        }
        let is_single = run.start == run.end;
        let has_vert = (run.y > 0 && g.pixel(run.start, run.y - 1))
            || (run.y + 1 < GLYPH_CELL_HEIGHT && g.pixel(run.start, run.y + 1));
        if !is_single || !has_vert {
            let x0 = run.start as f32 + inset;
            let x1 = (run.end + 1) as f32 - inset;
            let y0 = run.y as f32 + inset;
            let y1 = (run.y + 1) as f32 - inset;
            prims.push(SdfPrim::Box {
                cx: (x0 + x1) * 0.5,
                cy: (y0 + y1) * 0.5,
                hx: (x1 - x0) * 0.5,
                hy: (y1 - y0) * 0.5,
            });
        }
    }

    // 3. Straight vertical runs of length >= 2 (skipping single-step corners already bridged)
    for x in 0..GLYPH_CELL_WIDTH {
        let mut y = 0usize;
        while y < GLYPH_CELL_HEIGHT {
            if g.pixel(x, y) {
                let start = y;
                while y + 1 < GLYPH_CELL_HEIGHT && g.pixel(x, y + 1) {
                    y += 1;
                }
                let end = y;
                if end > start {
                    let x0 = x as f32 + inset;
                    let x1 = (x + 1) as f32 - inset;
                    let y0 = start as f32 + inset;
                    let y1 = (end + 1) as f32 - inset;
                    prims.push(SdfPrim::Box {
                        cx: (x0 + x1) * 0.5,
                        cy: (y0 + y1) * 0.5,
                        hx: (x1 - x0) * 0.5,
                        hy: (y1 - y0) * 0.5,
                    });
                }
            }
            y += 1;
        }
    }

    // 4. Diagonal 8-neighbor bridges where neither orthogonal intermediate pixel is active.
    for y in 0..GLYPH_CELL_HEIGHT.saturating_sub(1) {
        for x in 0..GLYPH_CELL_WIDTH {
            if !g.pixel(x, y) {
                continue;
            }
            if x + 1 < GLYPH_CELL_WIDTH
                && g.pixel(x + 1, y + 1)
                && !g.pixel(x + 1, y)
                && !g.pixel(x, y + 1)
            {
                prims.push(SdfPrim::Capsule {
                    a: [x as f32 + 0.5, y as f32 + 0.5],
                    b: [(x + 1) as f32 + 0.5, (y + 1) as f32 + 0.5],
                    r: 0.46,
                });
            }
            if x > 0
                && g.pixel(x - 1, y + 1)
                && !g.pixel(x - 1, y)
                && !g.pixel(x, y + 1)
            {
                prims.push(SdfPrim::Capsule {
                    a: [x as f32 + 0.5, y as f32 + 0.5],
                    b: [(x - 1) as f32 + 0.5, (y + 1) as f32 + 0.5],
                    r: 0.46,
                });
            }
        }
    }

    prims
}

/// Complete ASCII printable (`0x20..=0x7E`, 95 chars) crisp 8x12 bitmap glyph table.
/// Rows `1..=8` form cap-height (`8` rows); rows `3..=8` form x-height; rows `9..=10` form descenders.
#[rustfmt::skip]
const RAW_ASCII_GLYPHS: [[u8; GLYPH_CELL_HEIGHT]; GLYPH_COUNT] = [
    // 0x20 ' '
    [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    // 0x21 '!'
    [
        0b00000000,
        0b00110000,
        0b00110000,
        0b00110000,
        0b00110000,
        0b00110000,
        0b00000000,
        0b00110000,
        0b00110000,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x22 '"'
    [
        0b00000000,
        0b01101100,
        0b01101100,
        0b01101100,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x23 '#'
    [
        0b00000000,
        0b01001000,
        0b01001000,
        0b11111110,
        0b01001000,
        0b01001000,
        0b11111110,
        0b01001000,
        0b01001000,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x24 '$'
    [
        0b00010000,
        0b01111100,
        0b11010110,
        0b11010000,
        0b01111100,
        0b00010110,
        0b00010110,
        0b11010110,
        0b01111100,
        0b00010000,
        0b00000000,
        0b00000000,
    ],
    // 0x25 '%'
    [
        0b00000000,
        0b11000010,
        0b11000110,
        0b00001100,
        0b00011000,
        0b00110000,
        0b01100000,
        0b11000110,
        0b10000110,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x26 '&'
    [
        0b00000000,
        0b01111000,
        0b11001100,
        0b11001100,
        0b01111000,
        0b11011000,
        0b11001110,
        0b11001100,
        0b01110110,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x27 '\''
    [
        0b00000000,
        0b00110000,
        0b00110000,
        0b00100000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x28 '('
    [
        0b00000000,
        0b00011000,
        0b00110000,
        0b01100000,
        0b01100000,
        0b01100000,
        0b01100000,
        0b01100000,
        0b00110000,
        0b00011000,
        0b00000000,
        0b00000000,
    ],
    // 0x29 ')'
    [
        0b00000000,
        0b01100000,
        0b00110000,
        0b00011000,
        0b00011000,
        0b00011000,
        0b00011000,
        0b00011000,
        0b00110000,
        0b01100000,
        0b00000000,
        0b00000000,
    ],
    // 0x2A '*'
    [
        0b00000000,
        0b00000000,
        0b00100100,
        0b01111110,
        0b00111100,
        0b01111110,
        0b00100100,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x2B '+'
    [
        0b00000000,
        0b00000000,
        0b00011000,
        0b00011000,
        0b11111110,
        0b11111110,
        0b00011000,
        0b00011000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x2C ','
    [
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00110000,
        0b00110000,
        0b00100000,
        0b01000000,
        0b00000000,
    ],
    // 0x2D '-'
    [
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b11111100,
        0b11111100,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x2E '.'
    [
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b01100000,
        0b01100000,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x2F '/'
    [
        0b00000000,
        0b00000110,
        0b00001100,
        0b00001100,
        0b00011000,
        0b00110000,
        0b00110000,
        0b01100000,
        0b11000000,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x30 '0'
    [
        0b00000000,
        0b01111100,
        0b11000110,
        0b11001110,
        0b11011110,
        0b11110110,
        0b11100110,
        0b11000110,
        0b01111100,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x31 '1'
    [
        0b00000000,
        0b00110000,
        0b01110000,
        0b11110000,
        0b00110000,
        0b00110000,
        0b00110000,
        0b00110000,
        0b11111100,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x32 '2'
    [
        0b00000000,
        0b01111100,
        0b11000110,
        0b00000110,
        0b00001100,
        0b00111000,
        0b01100000,
        0b11000000,
        0b11111110,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x33 '3'
    [
        0b00000000,
        0b01111100,
        0b11000110,
        0b00000110,
        0b00111100,
        0b00000110,
        0b00000110,
        0b11000110,
        0b01111100,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x34 '4'
    [
        0b00000000,
        0b00011100,
        0b00111100,
        0b01101100,
        0b11001100,
        0b11111110,
        0b00001100,
        0b00001100,
        0b00001100,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x35 '5'
    [
        0b00000000,
        0b11111110,
        0b11000000,
        0b11000000,
        0b11111100,
        0b00000110,
        0b00000110,
        0b11000110,
        0b01111100,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x36 '6'
    [
        0b00000000,
        0b01111100,
        0b11000110,
        0b11000000,
        0b11111100,
        0b11000110,
        0b11000110,
        0b11000110,
        0b01111100,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x37 '7'
    [
        0b00000000,
        0b11111110,
        0b00000110,
        0b00001100,
        0b00011000,
        0b00110000,
        0b00110000,
        0b00110000,
        0b00110000,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x38 '8'
    [
        0b00000000,
        0b01111100,
        0b11000110,
        0b11000110,
        0b01111100,
        0b11000110,
        0b11000110,
        0b11000110,
        0b01111100,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x39 '9'
    [
        0b00000000,
        0b01111100,
        0b11000110,
        0b11000110,
        0b01111110,
        0b00000110,
        0b00000110,
        0b11000110,
        0b01111100,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x3A ':'
    [
        0b00000000,
        0b00000000,
        0b00000000,
        0b01100000,
        0b01100000,
        0b00000000,
        0b00000000,
        0b01100000,
        0b01100000,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x3B ';'
    [
        0b00000000,
        0b00000000,
        0b00000000,
        0b00110000,
        0b00110000,
        0b00000000,
        0b00000000,
        0b00110000,
        0b00110000,
        0b00100000,
        0b01000000,
        0b00000000,
    ],
    // 0x3C '<'
    [
        0b00000000,
        0b00001100,
        0b00011000,
        0b00110000,
        0b01100000,
        0b01100000,
        0b00110000,
        0b00011000,
        0b00001100,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x3D '='
    [
        0b00000000,
        0b00000000,
        0b00000000,
        0b11111100,
        0b00000000,
        0b00000000,
        0b11111100,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x3E '>'
    [
        0b00000000,
        0b01100000,
        0b00110000,
        0b00011000,
        0b00001100,
        0b00001100,
        0b00011000,
        0b00110000,
        0b01100000,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x3F '?'
    [
        0b00000000,
        0b01111100,
        0b11000110,
        0b00000110,
        0b00011100,
        0b00110000,
        0b00000000,
        0b00110000,
        0b00110000,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x40 '@'
    [
        0b00000000,
        0b01111100,
        0b11000110,
        0b11011110,
        0b11010110,
        0b11011110,
        0b11000000,
        0b11000110,
        0b01111100,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x41 'A'
    [
        0b00000000,
        0b00111000,
        0b01101100,
        0b11000110,
        0b11000110,
        0b11111110,
        0b11000110,
        0b11000110,
        0b11000110,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x42 'B'
    [
        0b00000000,
        0b11111100,
        0b11000110,
        0b11000110,
        0b11111100,
        0b11000110,
        0b11000110,
        0b11000110,
        0b11111100,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x43 'C'
    [
        0b00000000,
        0b01111100,
        0b11000110,
        0b11000000,
        0b11000000,
        0b11000000,
        0b11000000,
        0b11000110,
        0b01111100,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x44 'D'
    [
        0b00000000,
        0b11111000,
        0b11001100,
        0b11000110,
        0b11000110,
        0b11000110,
        0b11000110,
        0b11001100,
        0b11111000,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x45 'E'
    [
        0b00000000,
        0b11111110,
        0b11000000,
        0b11000000,
        0b11111100,
        0b11000000,
        0b11000000,
        0b11000000,
        0b11111110,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x46 'F'
    [
        0b00000000,
        0b11111110,
        0b11000000,
        0b11000000,
        0b11111100,
        0b11000000,
        0b11000000,
        0b11000000,
        0b11000000,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x47 'G'
    [
        0b00000000,
        0b01111100,
        0b11000110,
        0b11000000,
        0b11000000,
        0b11011110,
        0b11000110,
        0b11000110,
        0b01111100,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x48 'H'
    [
        0b00000000,
        0b11000110,
        0b11000110,
        0b11000110,
        0b11111110,
        0b11000110,
        0b11000110,
        0b11000110,
        0b11000110,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x49 'I'
    [
        0b00000000,
        0b11111100,
        0b00110000,
        0b00110000,
        0b00110000,
        0b00110000,
        0b00110000,
        0b00110000,
        0b11111100,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x4A 'J'
    [
        0b00000000,
        0b00111110,
        0b00001100,
        0b00001100,
        0b00001100,
        0b00001100,
        0b11001100,
        0b11001100,
        0b01111000,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x4B 'K'
    [
        0b00000000,
        0b11000110,
        0b11001100,
        0b11011000,
        0b11110000,
        0b11011000,
        0b11001100,
        0b11000110,
        0b11000110,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x4C 'L'
    [
        0b00000000,
        0b11000000,
        0b11000000,
        0b11000000,
        0b11000000,
        0b11000000,
        0b11000000,
        0b11000000,
        0b11111110,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x4D 'M'
    [
        0b00000000,
        0b11000011,
        0b11100111,
        0b11111111,
        0b11011011,
        0b11000011,
        0b11000011,
        0b11000011,
        0b11000011,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x4E 'N'
    [
        0b00000000,
        0b11000110,
        0b11100110,
        0b11110110,
        0b11011110,
        0b11001110,
        0b11000110,
        0b11000110,
        0b11000110,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x4F 'O'
    [
        0b00000000,
        0b01111100,
        0b11000110,
        0b11000110,
        0b11000110,
        0b11000110,
        0b11000110,
        0b11000110,
        0b01111100,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x50 'P'
    [
        0b00000000,
        0b11111100,
        0b11000110,
        0b11000110,
        0b11111100,
        0b11000000,
        0b11000000,
        0b11000000,
        0b11000000,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x51 'Q'
    [
        0b00000000,
        0b01111100,
        0b11000110,
        0b11000110,
        0b11000110,
        0b11000110,
        0b11011110,
        0b11001100,
        0b01110110,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x52 'R'
    [
        0b00000000,
        0b11111100,
        0b11000110,
        0b11000110,
        0b11111100,
        0b11011000,
        0b11001100,
        0b11000110,
        0b11000110,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x53 'S'
    [
        0b00000000,
        0b01111100,
        0b11000110,
        0b11000000,
        0b01111100,
        0b00000110,
        0b00000110,
        0b11000110,
        0b01111100,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x54 'T'
    [
        0b00000000,
        0b11111110,
        0b00011000,
        0b00011000,
        0b00011000,
        0b00011000,
        0b00011000,
        0b00011000,
        0b00011000,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x55 'U'
    [
        0b00000000,
        0b11000110,
        0b11000110,
        0b11000110,
        0b11000110,
        0b11000110,
        0b11000110,
        0b11000110,
        0b01111100,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x56 'V'
    [
        0b00000000,
        0b11000110,
        0b11000110,
        0b11000110,
        0b11000110,
        0b11000110,
        0b01101100,
        0b00111000,
        0b00111000,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x57 'W'
    [
        0b00000000,
        0b11000011,
        0b11000011,
        0b11000011,
        0b11011011,
        0b11011011,
        0b11111111,
        0b01100110,
        0b01100110,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x58 'X'
    [
        0b00000000,
        0b11000110,
        0b11000110,
        0b01101100,
        0b00111000,
        0b00111000,
        0b01101100,
        0b11000110,
        0b11000110,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x59 'Y'
    [
        0b00000000,
        0b11000110,
        0b11000110,
        0b01101100,
        0b00111000,
        0b00111000,
        0b00111000,
        0b00111000,
        0b00111000,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x5A 'Z'
    [
        0b00000000,
        0b11111110,
        0b00000110,
        0b00001100,
        0b00111000,
        0b01100000,
        0b11000000,
        0b11000000,
        0b11111110,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x5B '['
    [
        0b00000000,
        0b11110000,
        0b11000000,
        0b11000000,
        0b11000000,
        0b11000000,
        0b11000000,
        0b11000000,
        0b11000000,
        0b11110000,
        0b00000000,
        0b00000000,
    ],
    // 0x5C '\\'
    [
        0b00000000,
        0b11000000,
        0b01100000,
        0b01100000,
        0b00110000,
        0b00011000,
        0b00011000,
        0b00001100,
        0b00000110,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x5D ']'
    [
        0b00000000,
        0b11110000,
        0b00110000,
        0b00110000,
        0b00110000,
        0b00110000,
        0b00110000,
        0b00110000,
        0b00110000,
        0b11110000,
        0b00000000,
        0b00000000,
    ],
    // 0x5E '^'
    [
        0b00111000,
        0b01101100,
        0b11000110,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x5F '_'
    [
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b11111110,
        0b00000000,
        0b00000000,
    ],
    // 0x60 '`'
    [
        0b00000000,
        0b01100000,
        0b00110000,
        0b00011000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x61 'a'
    [
        0b00000000,
        0b00000000,
        0b00000000,
        0b01111000,
        0b00001100,
        0b01111100,
        0b11001100,
        0b11001100,
        0b01110110,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x62 'b'
    [
        0b00000000,
        0b11000000,
        0b11000000,
        0b11111000,
        0b11001100,
        0b11001100,
        0b11001100,
        0b11001100,
        0b11111000,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x63 'c'
    [
        0b00000000,
        0b00000000,
        0b00000000,
        0b01111100,
        0b11000110,
        0b11000000,
        0b11000000,
        0b11000110,
        0b01111100,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x64 'd'
    [
        0b00000000,
        0b00001100,
        0b00001100,
        0b01111100,
        0b11001100,
        0b11001100,
        0b11001100,
        0b11001100,
        0b01111100,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x65 'e'
    [
        0b00000000,
        0b00000000,
        0b00000000,
        0b01111100,
        0b11000110,
        0b11111110,
        0b11000000,
        0b11000110,
        0b01111100,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x66 'f'
    [
        0b00000000,
        0b00111100,
        0b01100110,
        0b01100000,
        0b11111000,
        0b01100000,
        0b01100000,
        0b01100000,
        0b01100000,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x67 'g'
    [
        0b00000000,
        0b00000000,
        0b00000000,
        0b01111100,
        0b11001100,
        0b11001100,
        0b11001100,
        0b11001100,
        0b01111100,
        0b00001100,
        0b11111000,
        0b00000000,
    ],
    // 0x68 'h'
    [
        0b00000000,
        0b11000000,
        0b11000000,
        0b11111000,
        0b11001100,
        0b11001100,
        0b11001100,
        0b11001100,
        0b11001100,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x69 'i'
    [
        0b00000000,
        0b01100000,
        0b00000000,
        0b11100000,
        0b01100000,
        0b01100000,
        0b01100000,
        0b01100000,
        0b11110000,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x6A 'j'
    [
        0b00000000,
        0b00011000,
        0b00000000,
        0b00111000,
        0b00011000,
        0b00011000,
        0b00011000,
        0b00011000,
        0b00011000,
        0b11011000,
        0b01110000,
        0b00000000,
    ],
    // 0x6B 'k'
    [
        0b00000000,
        0b11000000,
        0b11000000,
        0b11001100,
        0b11011000,
        0b11110000,
        0b11011000,
        0b11001100,
        0b11000110,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x6C 'l'
    [
        0b00000000,
        0b11100000,
        0b01100000,
        0b01100000,
        0b01100000,
        0b01100000,
        0b01100000,
        0b01100000,
        0b11110000,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x6D 'm'
    [
        0b00000000,
        0b00000000,
        0b00000000,
        0b11101100,
        0b11111110,
        0b11011010,
        0b11011010,
        0b11011010,
        0b11011010,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x6E 'n'
    [
        0b00000000,
        0b00000000,
        0b00000000,
        0b11111000,
        0b11001100,
        0b11001100,
        0b11001100,
        0b11001100,
        0b11001100,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x6F 'o'
    [
        0b00000000,
        0b00000000,
        0b00000000,
        0b01111000,
        0b11001100,
        0b11001100,
        0b11001100,
        0b11001100,
        0b01111000,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x70 'p'
    [
        0b00000000,
        0b00000000,
        0b00000000,
        0b11111000,
        0b11001100,
        0b11001100,
        0b11001100,
        0b11001100,
        0b11111000,
        0b11000000,
        0b11000000,
        0b00000000,
    ],
    // 0x71 'q'
    [
        0b00000000,
        0b00000000,
        0b00000000,
        0b01111100,
        0b11001100,
        0b11001100,
        0b11001100,
        0b11001100,
        0b01111100,
        0b00001100,
        0b00001100,
        0b00000000,
    ],
    // 0x72 'r'
    [
        0b00000000,
        0b00000000,
        0b00000000,
        0b11011100,
        0b11100110,
        0b11000000,
        0b11000000,
        0b11000000,
        0b11000000,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x73 's'
    [
        0b00000000,
        0b00000000,
        0b00000000,
        0b01111100,
        0b11000000,
        0b01111000,
        0b00001100,
        0b00001100,
        0b11111000,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x74 't'
    [
        0b00000000,
        0b01100000,
        0b01100000,
        0b11111000,
        0b01100000,
        0b01100000,
        0b01100000,
        0b01100100,
        0b00111000,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x75 'u'
    [
        0b00000000,
        0b00000000,
        0b00000000,
        0b11001100,
        0b11001100,
        0b11001100,
        0b11001100,
        0b11001100,
        0b01111100,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x76 'v'
    [
        0b00000000,
        0b00000000,
        0b00000000,
        0b11001100,
        0b11001100,
        0b11001100,
        0b11001100,
        0b01111000,
        0b00110000,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x77 'w'
    [
        0b00000000,
        0b00000000,
        0b00000000,
        0b11000010,
        0b11011010,
        0b11011010,
        0b11111110,
        0b01100100,
        0b01100100,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x78 'x'
    [
        0b00000000,
        0b00000000,
        0b00000000,
        0b11001100,
        0b01111000,
        0b00110000,
        0b00110000,
        0b01111000,
        0b11001100,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x79 'y'
    [
        0b00000000,
        0b00000000,
        0b00000000,
        0b11001100,
        0b11001100,
        0b11001100,
        0b11001100,
        0b11001100,
        0b01111100,
        0b00001100,
        0b11111000,
        0b00000000,
    ],
    // 0x7A 'z'
    [
        0b00000000,
        0b00000000,
        0b00000000,
        0b11111100,
        0b00001100,
        0b00111000,
        0b01100000,
        0b11000000,
        0b11111100,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
    // 0x7B '{'
    [
        0b00000000,
        0b00111000,
        0b01100000,
        0b01100000,
        0b11000000,
        0b01100000,
        0b01100000,
        0b01100000,
        0b01100000,
        0b00111000,
        0b00000000,
        0b00000000,
    ],
    // 0x7C '|'
    [
        0b00000000,
        0b01100000,
        0b01100000,
        0b01100000,
        0b01100000,
        0b01100000,
        0b01100000,
        0b01100000,
        0b01100000,
        0b01100000,
        0b00000000,
        0b00000000,
    ],
    // 0x7D '}'
    [
        0b00000000,
        0b11100000,
        0b00110000,
        0b00110000,
        0b00011000,
        0b00110000,
        0b00110000,
        0b00110000,
        0b00110000,
        0b11100000,
        0b00000000,
        0b00000000,
    ],
    // 0x7E '~'
    [
        0b00000000,
        0b00000000,
        0b00000000,
        0b01100110,
        0b11111100,
        0b11001100,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
        0b00000000,
    ],
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analytical_vector_glyphs_respect_bounds_and_smooth_edges() {
        let vector_chars: Vec<char> = ('0'..='9')
            .chain('A'..='Z')
            .chain(['/', '-'])
            .collect();

        for ch in vector_chars {
            let sdf = glyph_sdf(ch);
            let bmp = sdf.bitmap;
            let min_x = bmp.left_col as f32;
            let max_x = (bmp.left_col + bmp.ink_width) as f32;
            let min_y = if ch == '-' { 4.0 } else { GLYPH_CAP_TOP };
            let max_y = if ch == '-' { 6.0 } else { GLYPH_BASELINE };

            let mut inside_ink_count = 0usize;
            for sy in 0..96 {
                let gy = -1.0 + (sy as f32) * 0.125;
                for sx in 0..80 {
                    let gx = -1.0 + (sx as f32) * 0.125;
                    let d = sdf.eval_exact(gx, gy);
                    if d <= 0.0 {
                        inside_ink_count += 1;
                        assert!(
                            gx >= min_x - 0.15
                                && gx <= max_x + 0.15
                                && gy >= min_y - 0.15
                                && gy <= max_y + 0.15,
                            "glyph '{ch}' ink at ({gx:.2}, {gy:.2}) escaped [{min_x}..{max_x}] x [{min_y}..{max_y}]"
                        );
                    }
                }
            }
            assert!(
                inside_ink_count > 10,
                "glyph '{ch}' must have positive interior ink area"
            );
        }

        // Verify '/' diagonal edge is smooth and continuous at high scale (1080p-like)
        let slash = glyph_sdf('/');
        let mut prev_center_x: Option<f32> = None;
        for row in 20..70 {
            let gy = (row as f32) * 0.1;
            // Find x where eval_exact crosses 0 along scanline
            let mut min_x = f32::INFINITY;
            for col in 0..800 {
                let gx = (col as f32) * 0.01;
                if slash.eval_exact(gx, gy) <= 0.0 {
                    min_x = min_x.min(gx);
                }
            }
            if let Some(prev) = prev_center_x {
                let step = (prev - min_x).abs();
                assert!(
                    step < 0.15,
                    "slash left edge jumped by {step:.3} at y={gy:.1} (staircase detected)"
                );
            }
            prev_center_x = Some(min_x);
        }
    }
}
