use crate::provider::SourceFrame;

#[derive(Clone, Copy)]
struct Segment {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
}

const fn seg(x0: f32, y0: f32, x1: f32, y1: f32) -> Segment {
    Segment { x0, y0, x1, y1 }
}

/// Self-contained analytical capsule-SDF vector text rasterizer.
///
/// Produces a premultiplied-alpha Linear Rec.709 `SourceFrame` (`RgbaF32`)
/// using resolution-independent normalized metrics so `draft` and `final`
/// profiles preserve identical layout proportions and outline ratios.
#[allow(clippy::too_many_arguments)]
pub fn rasterize_vector_text(
    text: &str,
    font_size_norm: f32,
    color_rgba: [f32; 4],
    outline_rgba: [f32; 4],
    outline_width_norm: f32,
    align_center: bool,
    y_offset_norm: f32,
    width: u32,
    height: u32,
) -> SourceFrame {
    let pixel_count = (width as usize) * (height as usize);
    let mut pixels = vec![[0.0_f32; 4]; pixel_count];

    if text.is_empty() || width == 0 || height == 0 {
        return SourceFrame::rgba_f32_premul(width, height, pixels);
    }

    let w_f = width as f32;
    let h_f = height as f32;
    let font_px = (font_size_norm.max(1e-4) * h_f).max(2.0);
    let stroke_r = (0.085 * font_px).max(0.6);
    let outline_px = (outline_width_norm.max(0.0) * h_f).max(0.0);
    let aa = 0.75_f32;

    let glyph_w = 0.54 * font_px;
    let advance = 0.68 * font_px;

    let chars: Vec<char> = text.chars().collect();
    let total_w = if chars.is_empty() {
        0.0
    } else {
        (chars.len() as f32 - 1.0) * advance + glyph_w
    };

    let start_x = if align_center {
        0.5 * w_f - 0.5 * total_w
    } else {
        0.06 * w_f
    };
    let center_y = y_offset_norm * h_f;
    let top_y = center_y - 0.5 * font_px;

    let fill_a_base = color_rgba[3].clamp(0.0, 1.0);
    let out_a_base = outline_rgba[3].clamp(0.0, 1.0);
    let has_outline = outline_px > 0.0 && out_a_base > 1e-6;
    let max_radius = stroke_r + if has_outline { outline_px } else { 0.0 } + aa + 1.0;

    for (idx, &ch) in chars.iter().enumerate() {
        if ch == ' ' || ch == '\t' || ch == '\n' || ch == '\r' {
            continue;
        }
        let is_lower = ch.is_ascii_lowercase();
        let (segs, scale_y, offset_y) = if is_lower {
            (glyph_segments(ch.to_ascii_uppercase()), 0.76_f32, 0.24_f32)
        } else {
            (glyph_segments(ch), 1.0_f32, 0.0_f32)
        };
        if segs.is_empty() {
            continue;
        }

        let gx0 = start_x + (idx as f32) * advance;
        let gy0 = top_y + offset_y * font_px;
        let gw = glyph_w;
        let gh = font_px * scale_y;

        let min_px = ((gx0 - max_radius).floor() as i32).max(0);
        let max_px = ((gx0 + gw + max_radius).ceil() as i32).min(width as i32 - 1);
        let min_py = ((gy0 - max_radius).floor() as i32).max(0);
        let max_py = ((gy0 + gh + max_radius).ceil() as i32).min(height as i32 - 1);

        if min_px > max_px || min_py > max_py {
            continue;
        }

        for py in min_py..=max_py {
            let fy = py as f32 + 0.5;
            let row_off = (py as usize) * (width as usize);
            for px in min_px..=max_px {
                let fx = px as f32 + 0.5;

                let mut min_dist_sq = f32::INFINITY;
                for s in segs {
                    let ax = gx0 + s.x0 * gw;
                    let ay = gy0 + s.y0 * gh;
                    let bx = gx0 + s.x1 * gw;
                    let by = gy0 + s.y1 * gh;
                    let d2 = dist_sq_to_segment(fx, fy, ax, ay, bx, by);
                    if d2 < min_dist_sq {
                        min_dist_sq = d2;
                    }
                }

                let dist = min_dist_sq.sqrt();
                let fill_cov = smoothstep(stroke_r + aa, stroke_r - aa, dist);
                let out_cov = if has_outline {
                    let r_out = stroke_r + outline_px;
                    smoothstep(r_out + aa, r_out - aa, dist)
                } else {
                    0.0
                };

                if fill_cov <= 1e-6 && out_cov <= 1e-6 {
                    continue;
                }

                let fa = fill_a_base * fill_cov;
                let oa = out_a_base * out_cov;

                let fr = color_rgba[0].max(0.0) * fa;
                let fg = color_rgba[1].max(0.0) * fa;
                let fb = color_rgba[2].max(0.0) * fa;

                let or = outline_rgba[0].max(0.0) * oa;
                let og = outline_rgba[1].max(0.0) * oa;
                let ob = outline_rgba[2].max(0.0) * oa;

                // Porter-Duff Over: fill over outline (both premultiplied)
                let inv_fa = 1.0 - fa;
                let src = [
                    fr + or * inv_fa,
                    fg + og * inv_fa,
                    fb + ob * inv_fa,
                    fa + oa * inv_fa,
                ];

                let dst = &mut pixels[row_off + (px as usize)];
                let inv_sa = 1.0 - src[3].clamp(0.0, 1.0);
                dst[0] = src[0] + dst[0] * inv_sa;
                dst[1] = src[1] + dst[1] * inv_sa;
                dst[2] = src[2] + dst[2] * inv_sa;
                dst[3] = (src[3] + dst[3] * inv_sa).clamp(0.0, 1.0);
            }
        }
    }

    SourceFrame::rgba_f32_premul(width, height, pixels)
}

#[inline]
fn dist_sq_to_segment(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let abx = bx - ax;
    let aby = by - ay;
    let apx = px - ax;
    let apy = py - ay;
    let denom = abx * abx + aby * aby;
    if denom <= 1e-8 {
        return apx * apx + apy * apy;
    }
    let t = ((apx * abx + apy * aby) / denom).clamp(0.0, 1.0);
    let dx = apx - abx * t;
    let dy = apy - aby * t;
    dx * dx + dy * dy
}

#[inline]
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    if (edge1 - edge0).abs() < 1e-6 {
        return if x >= edge1 { 1.0 } else { 0.0 };
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

macro_rules! segs {
    ($($x0:expr, $y0:expr, $x1:expr, $y1:expr);* $(;)?) => {{
        const DATA: &[Segment] = &[$(seg($x0, $y0, $x1, $y1)),*];
        DATA
    }};
}

fn glyph_segments(ch: char) -> &'static [Segment] {
    match ch {
        'A' => segs!(0.05, 1.0, 0.5, 0.0; 0.5, 0.0, 0.95, 1.0; 0.22, 0.64, 0.78, 0.64),
        'B' => segs!(
            0.1, 0.0, 0.1, 1.0; 0.1, 0.0, 0.72, 0.0; 0.72, 0.0, 0.88, 0.14;
            0.88, 0.14, 0.88, 0.34; 0.88, 0.34, 0.72, 0.48; 0.1, 0.48, 0.75, 0.48;
            0.75, 0.48, 0.92, 0.62; 0.92, 0.62, 0.92, 0.86; 0.92, 0.86, 0.75, 1.0;
            0.1, 1.0, 0.75, 1.0
        ),
        'C' => segs!(
            0.9, 0.16, 0.72, 0.0; 0.72, 0.0, 0.28, 0.0; 0.28, 0.0, 0.1, 0.18;
            0.1, 0.18, 0.1, 0.82; 0.1, 0.82, 0.28, 1.0; 0.28, 1.0, 0.72, 1.0;
            0.72, 1.0, 0.9, 0.84
        ),
        'D' => segs!(
            0.1, 0.0, 0.1, 1.0; 0.1, 0.0, 0.66, 0.0; 0.66, 0.0, 0.9, 0.22;
            0.9, 0.22, 0.9, 0.78; 0.9, 0.78, 0.66, 1.0; 0.1, 1.0, 0.66, 1.0
        ),
        'E' => segs!(
            0.12, 0.0, 0.12, 1.0; 0.12, 0.0, 0.9, 0.0;
            0.12, 0.5, 0.76, 0.5; 0.12, 1.0, 0.9, 1.0
        ),
        'F' => segs!(0.12, 0.0, 0.12, 1.0; 0.12, 0.0, 0.9, 0.0; 0.12, 0.5, 0.76, 0.5),
        'G' => segs!(
            0.88, 0.16, 0.7, 0.0; 0.7, 0.0, 0.28, 0.0; 0.28, 0.0, 0.1, 0.18;
            0.1, 0.18, 0.1, 0.82; 0.1, 0.82, 0.28, 1.0; 0.28, 1.0, 0.72, 1.0;
            0.72, 1.0, 0.9, 0.82; 0.9, 0.82, 0.9, 0.52; 0.54, 0.52, 0.9, 0.52
        ),
        'H' => segs!(0.12, 0.0, 0.12, 1.0; 0.88, 0.0, 0.88, 1.0; 0.12, 0.5, 0.88, 0.5),
        'I' => segs!(0.22, 0.0, 0.78, 0.0; 0.5, 0.0, 0.5, 1.0; 0.22, 1.0, 0.78, 1.0),
        'J' => segs!(
            0.26, 0.0, 0.86, 0.0; 0.76, 0.0, 0.76, 0.8; 0.76, 0.8, 0.58, 1.0;
            0.58, 1.0, 0.28, 1.0; 0.28, 1.0, 0.12, 0.82
        ),
        'K' => segs!(0.12, 0.0, 0.12, 1.0; 0.88, 0.0, 0.14, 0.54; 0.32, 0.42, 0.9, 1.0),
        'L' => segs!(0.14, 0.0, 0.14, 1.0; 0.14, 1.0, 0.88, 1.0),
        'M' => segs!(
            0.08, 1.0, 0.08, 0.0; 0.08, 0.0, 0.5, 0.56;
            0.5, 0.56, 0.92, 0.0; 0.92, 0.0, 0.92, 1.0
        ),
        'N' => segs!(0.12, 1.0, 0.12, 0.0; 0.12, 0.0, 0.88, 1.0; 0.88, 1.0, 0.88, 0.0),
        'O' => segs!(
            0.28, 0.0, 0.72, 0.0; 0.72, 0.0, 0.9, 0.18; 0.9, 0.18, 0.9, 0.82;
            0.9, 0.82, 0.72, 1.0; 0.72, 1.0, 0.28, 1.0; 0.28, 1.0, 0.1, 0.82;
            0.1, 0.82, 0.1, 0.18; 0.1, 0.18, 0.28, 0.0
        ),
        'P' => segs!(
            0.12, 0.0, 0.12, 1.0; 0.12, 0.0, 0.72, 0.0; 0.72, 0.0, 0.9, 0.14;
            0.9, 0.14, 0.9, 0.38; 0.9, 0.38, 0.72, 0.52; 0.12, 0.52, 0.72, 0.52
        ),
        'Q' => segs!(
            0.28, 0.0, 0.72, 0.0; 0.72, 0.0, 0.9, 0.18; 0.9, 0.18, 0.9, 0.78;
            0.9, 0.78, 0.7, 1.0; 0.7, 1.0, 0.28, 1.0; 0.28, 1.0, 0.1, 0.82;
            0.1, 0.82, 0.1, 0.18; 0.1, 0.18, 0.28, 0.0; 0.58, 0.68, 0.94, 1.02
        ),
        'R' => segs!(
            0.12, 0.0, 0.12, 1.0; 0.12, 0.0, 0.72, 0.0; 0.72, 0.0, 0.9, 0.14;
            0.9, 0.14, 0.9, 0.36; 0.9, 0.36, 0.72, 0.5; 0.12, 0.5, 0.72, 0.5;
            0.5, 0.5, 0.9, 1.0
        ),
        'S' => segs!(
            0.88, 0.14, 0.7, 0.0; 0.7, 0.0, 0.26, 0.0; 0.26, 0.0, 0.1, 0.16;
            0.1, 0.16, 0.1, 0.34; 0.1, 0.34, 0.28, 0.48; 0.28, 0.48, 0.72, 0.52;
            0.72, 0.52, 0.9, 0.66; 0.9, 0.66, 0.9, 0.84; 0.9, 0.84, 0.72, 1.0;
            0.72, 1.0, 0.26, 1.0; 0.26, 1.0, 0.1, 0.86
        ),
        'T' => segs!(0.06, 0.0, 0.94, 0.0; 0.5, 0.0, 0.5, 1.0),
        'U' => segs!(
            0.12, 0.0, 0.12, 0.82; 0.12, 0.82, 0.3, 1.0; 0.3, 1.0, 0.7, 1.0;
            0.7, 1.0, 0.88, 0.82; 0.88, 0.82, 0.88, 0.0
        ),
        'V' => segs!(0.08, 0.0, 0.5, 1.0; 0.5, 1.0, 0.92, 0.0),
        'W' => segs!(
            0.05, 0.0, 0.26, 1.0; 0.26, 1.0, 0.5, 0.38;
            0.5, 0.38, 0.74, 1.0; 0.74, 1.0, 0.95, 0.0
        ),
        'X' => segs!(0.1, 0.0, 0.9, 1.0; 0.9, 0.0, 0.1, 1.0),
        'Y' => segs!(0.08, 0.0, 0.5, 0.52; 0.92, 0.0, 0.5, 0.52; 0.5, 0.52, 0.5, 1.0),
        'Z' => segs!(0.1, 0.0, 0.9, 0.0; 0.9, 0.0, 0.1, 1.0; 0.1, 1.0, 0.9, 1.0),
        '0' => segs!(
            0.28, 0.0, 0.72, 0.0; 0.72, 0.0, 0.9, 0.18; 0.9, 0.18, 0.9, 0.82;
            0.9, 0.82, 0.72, 1.0; 0.72, 1.0, 0.28, 1.0; 0.28, 1.0, 0.1, 0.82;
            0.1, 0.82, 0.1, 0.18; 0.1, 0.18, 0.28, 0.0; 0.16, 0.84, 0.84, 0.16
        ),
        '1' => segs!(0.28, 0.18, 0.52, 0.0; 0.52, 0.0, 0.52, 1.0; 0.22, 1.0, 0.82, 1.0),
        '2' => segs!(
            0.12, 0.2, 0.3, 0.0; 0.3, 0.0, 0.72, 0.0; 0.72, 0.0, 0.88, 0.16;
            0.88, 0.16, 0.88, 0.36; 0.88, 0.36, 0.1, 1.0; 0.1, 1.0, 0.9, 1.0
        ),
        '3' => segs!(
            0.14, 0.0, 0.74, 0.0; 0.74, 0.0, 0.9, 0.16; 0.9, 0.16, 0.9, 0.34;
            0.9, 0.34, 0.72, 0.48; 0.36, 0.48, 0.72, 0.48; 0.72, 0.48, 0.9, 0.64;
            0.9, 0.64, 0.9, 0.84; 0.9, 0.84, 0.72, 1.0; 0.72, 1.0, 0.14, 1.0
        ),
        '4' => segs!(0.72, 0.0, 0.1, 0.66; 0.1, 0.66, 0.92, 0.66; 0.72, 0.0, 0.72, 1.0),
        '5' => segs!(
            0.86, 0.0, 0.16, 0.0; 0.16, 0.0, 0.14, 0.46; 0.14, 0.46, 0.72, 0.46;
            0.72, 0.46, 0.9, 0.62; 0.9, 0.62, 0.9, 0.84; 0.9, 0.84, 0.72, 1.0;
            0.72, 1.0, 0.14, 1.0
        ),
        '6' => segs!(
            0.8, 0.0, 0.32, 0.0; 0.32, 0.0, 0.12, 0.2; 0.12, 0.2, 0.12, 0.84;
            0.12, 0.84, 0.3, 1.0; 0.3, 1.0, 0.72, 1.0; 0.72, 1.0, 0.9, 0.84;
            0.9, 0.84, 0.9, 0.62; 0.9, 0.62, 0.72, 0.48; 0.72, 0.48, 0.12, 0.48
        ),
        '7' => segs!(0.1, 0.0, 0.9, 0.0; 0.9, 0.0, 0.34, 1.0),
        '8' => segs!(
            0.28, 0.0, 0.72, 0.0; 0.72, 0.0, 0.88, 0.16; 0.88, 0.16, 0.88, 0.34;
            0.88, 0.34, 0.72, 0.48; 0.28, 0.48, 0.72, 0.48; 0.28, 0.48, 0.12, 0.34;
            0.12, 0.34, 0.12, 0.16; 0.12, 0.16, 0.28, 0.0; 0.28, 0.48, 0.1, 0.64;
            0.1, 0.64, 0.1, 0.84; 0.1, 0.84, 0.28, 1.0; 0.28, 1.0, 0.72, 1.0;
            0.72, 1.0, 0.9, 0.84; 0.9, 0.84, 0.9, 0.64; 0.9, 0.64, 0.72, 0.48
        ),
        '9' => segs!(
            0.88, 0.52, 0.28, 0.52; 0.28, 0.52, 0.1, 0.38; 0.1, 0.38, 0.1, 0.16;
            0.1, 0.16, 0.28, 0.0; 0.28, 0.0, 0.72, 0.0; 0.72, 0.0, 0.88, 0.16;
            0.88, 0.16, 0.88, 0.82; 0.88, 0.82, 0.7, 1.0; 0.7, 1.0, 0.22, 1.0
        ),
        '/' => segs!(0.16, 1.0, 0.84, 0.0),
        '\\' => segs!(0.16, 0.0, 0.84, 1.0),
        '-' => segs!(0.16, 0.5, 0.84, 0.5),
        '_' => segs!(0.08, 1.0, 0.92, 1.0),
        '+' => segs!(0.5, 0.2, 0.5, 0.8; 0.18, 0.5, 0.82, 0.5),
        '=' => segs!(0.16, 0.38, 0.84, 0.38; 0.16, 0.62, 0.84, 0.62),
        '.' => segs!(0.48, 0.94, 0.52, 0.94),
        ',' => segs!(0.54, 0.86, 0.42, 1.04),
        ':' => segs!(0.48, 0.28, 0.52, 0.28; 0.48, 0.76, 0.52, 0.76),
        ';' => segs!(0.48, 0.28, 0.52, 0.28; 0.54, 0.76, 0.42, 0.98),
        '!' => segs!(0.5, 0.0, 0.5, 0.66; 0.49, 0.92, 0.51, 0.92),
        '?' => segs!(
            0.18, 0.2, 0.34, 0.0; 0.34, 0.0, 0.72, 0.0; 0.72, 0.0, 0.86, 0.16;
            0.86, 0.16, 0.86, 0.34; 0.86, 0.34, 0.5, 0.54; 0.5, 0.54, 0.5, 0.68;
            0.49, 0.92, 0.51, 0.92
        ),
        '(' => segs!(0.72, 0.0, 0.42, 0.24; 0.42, 0.24, 0.42, 0.76; 0.42, 0.76, 0.72, 1.0),
        ')' => segs!(0.28, 0.0, 0.58, 0.24; 0.58, 0.24, 0.58, 0.76; 0.58, 0.76, 0.28, 1.0),
        '[' => segs!(0.68, 0.0, 0.34, 0.0; 0.34, 0.0, 0.34, 1.0; 0.34, 1.0, 0.68, 1.0),
        ']' => segs!(0.32, 0.0, 0.66, 0.0; 0.66, 0.0, 0.66, 1.0; 0.66, 1.0, 0.32, 1.0),
        '%' => segs!(0.16, 1.0, 0.84, 0.0; 0.2, 0.16, 0.3, 0.16; 0.7, 0.84, 0.8, 0.84),
        '#' => {
            segs!(0.34, 0.08, 0.26, 0.92; 0.74, 0.08, 0.66, 0.92; 0.1, 0.36, 0.9, 0.36; 0.1, 0.66, 0.9, 0.66)
        }
        '*' => segs!(0.5, 0.18, 0.5, 0.78; 0.22, 0.32, 0.78, 0.64; 0.78, 0.32, 0.22, 0.64),
        '\'' => segs!(0.5, 0.0, 0.46, 0.24),
        '"' => segs!(0.36, 0.0, 0.34, 0.24; 0.66, 0.0, 0.64, 0.24),
        _ => segs!(
            0.14, 0.06, 0.86, 0.06; 0.86, 0.06, 0.86, 0.94;
            0.86, 0.94, 0.14, 0.94; 0.14, 0.94, 0.14, 0.06
        ),
    }
}
