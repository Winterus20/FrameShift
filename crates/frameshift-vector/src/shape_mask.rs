//! Keyframeable Bézier/polygon `ShapeMask` morphing and deterministic rasterizer.
//!
//! Uses `lyon_tessellation` path construction & flattening, uniform arc-length
//! resampling + cyclic alignment for mismatched-vertex-count morphing, and
//! `VectorCanvas` 4x4 sub-pixel NonZero scanline coverage + SDF expand/choke.

use crate::canvas::{
    VectorCanvas, contour_slices_bounds, rasterize_contour_slices_coverage, sd_segment_sq,
    smoothstep,
};
use crate::error::VectorError;
use frameshift_media::{AlphaMode, Rgba16fFrame};
use frameshift_schema::{AnimationEasing, NodeKind, ShapeMaskKeyframe, ShapeMaskVertex};
use frameshift_time::Flicks;
use lyon_tessellation::math::point;
use lyon_tessellation::path::iterator::PathIterator;
use lyon_tessellation::path::{Event, Path as LyonPath};

/// Flattening tolerance in pixels for `lyon_tessellation` Bézier evaluation.
const FLATTEN_TOLERANCE_PX: f32 = 0.25;

/// Self-contained `ShapeMask` configuration for CPU/GPU evaluation.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeMaskConfig {
    pub vertices: Vec<ShapeMaskVertex>,
    pub keyframes: Vec<ShapeMaskKeyframe>,
    pub closed: bool,
    pub feather_px: f32,
    pub expand_px: f32,
    pub invert: bool,
    pub opacity: f32,
}

impl Default for ShapeMaskConfig {
    fn default() -> Self {
        Self {
            vertices: Vec::new(),
            keyframes: Vec::new(),
            closed: true,
            feather_px: 0.0,
            expand_px: 0.0,
            invert: false,
            opacity: 1.0,
        }
    }
}

impl ShapeMaskConfig {
    /// Extracts a [`ShapeMaskConfig`] from a schema [`NodeKind::ShapeMask`].
    pub fn from_node_kind(kind: &NodeKind) -> Result<Self, VectorError> {
        match kind {
            NodeKind::ShapeMask {
                vertices,
                keyframes,
                closed,
                feather_px,
                expand_px,
                invert,
                opacity,
            } => Ok(Self {
                vertices: vertices.clone(),
                keyframes: keyframes.clone(),
                closed: *closed,
                feather_px: *feather_px,
                expand_px: *expand_px,
                invert: *invert,
                opacity: *opacity,
            }),
            _ => Err(VectorError::InvalidParameter(
                "expected NodeKind::ShapeMask".to_owned(),
            )),
        }
    }
}

/// Evaluates `NodeKind::ShapeMask` directly to a premultiplied Linear Rec.709 `Rgba16fFrame` mask.
pub fn render_shape_mask_node_rgba16f(
    kind: &NodeKind,
    tick: Flicks,
    width: u32,
    height: u32,
) -> Result<Rgba16fFrame, VectorError> {
    let cfg = ShapeMaskConfig::from_node_kind(kind)?;
    render_shape_mask_rgba16f(&cfg, tick, width, height)
}

/// Rasterizes a [`ShapeMaskConfig`] at `tick` (including morphing, `expand_px`, `feather_px`,
/// `invert`, and `opacity`) into a premultiplied Linear Rec.709 `Rgba16fFrame` (`[m, m, m, m]`).
pub fn render_shape_mask_rgba16f(
    cfg: &ShapeMaskConfig,
    tick: Flicks,
    width: u32,
    height: u32,
) -> Result<Rgba16fFrame, VectorError> {
    render_shape_mask_inner(cfg, tick, width, height, true)
}

/// Rasterizes a [`ShapeMaskConfig`] at `tick` WITHOUT applying `feather_px` (useful when
/// downstream GPU passes run `fx_mask_feather.wgsl` separable Gaussian blur).
pub fn render_shape_mask_crisp_rgba16f(
    cfg: &ShapeMaskConfig,
    tick: Flicks,
    width: u32,
    height: u32,
) -> Result<Rgba16fFrame, VectorError> {
    render_shape_mask_inner(cfg, tick, width, height, false)
}

fn render_shape_mask_inner(
    cfg: &ShapeMaskConfig,
    tick: Flicks,
    width: u32,
    height: u32,
    apply_feather: bool,
) -> Result<Rgba16fFrame, VectorError> {
    validate_shape_mask_config(cfg)?;
    if width == 0 || height == 0 {
        return Err(VectorError::InvalidDimensions { width, height });
    }
    let pixel_count = (width as usize)
        .checked_mul(height as usize)
        .ok_or(VectorError::InvalidDimensions { width, height })?;

    let w_f = width as f32;
    let h_f = height as f32;

    let pose = interpolate_shape_mask_vertices(
        &cfg.vertices,
        &cfg.keyframes,
        cfg.closed,
        tick,
        width,
        height,
    )?;
    let contour = flatten_shape_mask_contour(&pose, cfg.closed, w_f, h_f);

    let mut cov = vec![0.0_f32; pixel_count];
    if cfg.closed && contour.len() >= 3 {
        if cfg.expand_px.abs() <= 1e-4 {
            rasterize_closed_contour_into(&contour, width, height, &mut cov);
        } else {
            rasterize_expanded_contour_into(&contour, cfg.expand_px, width, height, &mut cov);
        }
    }

    if apply_feather && cfg.feather_px > 0.0 {
        apply_separable_feather(&mut cov, width as usize, height as usize, cfg.feather_px);
    }

    let opacity = cfg.opacity.clamp(0.0, 1.0);
    let mut data = Vec::with_capacity(pixel_count);
    for c in cov {
        let base = if cfg.invert { 1.0 - c } else { c };
        let m = (base * opacity).clamp(0.0, 1.0);
        data.push([m, m, m, m]);
    }

    Ok(Rgba16fFrame {
        width,
        height,
        pts: tick,
        alpha_mode: AlphaMode::Premultiplied,
        data,
    })
}

/// Evaluates and morph-interpolates `ShapeMask` vertices at `tick`.
///
/// - Same vertex count: direct Bézier control-point + handle interpolation.
/// - Different vertex count: flattens both poses via `lyon_tessellation`, resamples both
///   contours to equal arc-length vertex counts, aligns cyclic starting offset to minimize
///   twist (when `closed`), and linearly interpolates.
pub fn interpolate_shape_mask_vertices(
    static_vertices: &[ShapeMaskVertex],
    keyframes: &[ShapeMaskKeyframe],
    closed: bool,
    tick: Flicks,
    width: u32,
    height: u32,
) -> Result<Vec<ShapeMaskVertex>, VectorError> {
    if width == 0 || height == 0 {
        return Err(VectorError::InvalidDimensions { width, height });
    }
    if keyframes.is_empty() {
        return Ok(static_vertices.to_vec());
    }
    if keyframes.len() == 1 || tick <= keyframes[0].tick {
        return Ok(keyframes[0].vertices.clone());
    }
    let last_idx = keyframes.len() - 1;
    if tick >= keyframes[last_idx].tick {
        return Ok(keyframes[last_idx].vertices.clone());
    }

    let next_idx = keyframes.partition_point(|k| k.tick <= tick);
    let prev_idx = next_idx.saturating_sub(1);
    let kf_a = &keyframes[prev_idx];
    let kf_b = &keyframes[next_idx];

    let span = kf_b.tick.0 - kf_a.tick.0;
    if span <= 0 {
        return Ok(kf_b.vertices.clone());
    }
    let raw_t = ((tick.0 - kf_a.tick.0) as f64 / span as f64).clamp(0.0, 1.0) as f32;
    let eased_t = apply_animation_easing(raw_t, kf_a.easing);

    if kf_a.vertices.len() == kf_b.vertices.len() {
        return Ok(kf_a
            .vertices
            .iter()
            .zip(kf_b.vertices.iter())
            .map(|(va, vb)| ShapeMaskVertex {
                pos: lerp2(va.pos, vb.pos, eased_t),
                in_tangent: lerp2(va.in_tangent, vb.in_tangent, eased_t),
                out_tangent: lerp2(va.out_tangent, vb.out_tangent, eased_t),
            })
            .collect());
    }

    if kf_a.vertices.is_empty() || kf_b.vertices.is_empty() {
        return Ok(if eased_t < 0.5 {
            kf_a.vertices.clone()
        } else {
            kf_b.vertices.clone()
        });
    }

    let w_f = width as f32;
    let h_f = height as f32;
    let flat_a = flatten_shape_mask_contour(&kf_a.vertices, closed, w_f, h_f);
    let flat_b = flatten_shape_mask_contour(&kf_b.vertices, closed, w_f, h_f);
    if flat_a.len() < 2 || flat_b.len() < 2 {
        return Ok(if eased_t < 0.5 {
            kf_a.vertices.clone()
        } else {
            kf_b.vertices.clone()
        });
    }

    let sample_count = (flat_a.len().max(flat_b.len()) * 4).clamp(64, 512);
    let resampled_a = resample_contour_arc_length(&flat_a, closed, sample_count);
    let resampled_b = resample_contour_arc_length(&flat_b, closed, sample_count);

    let shift = if closed {
        best_cyclic_alignment(&resampled_a, &resampled_b)
    } else {
        0
    };

    let inv_w = 1.0 / w_f;
    let inv_h = 1.0 / h_f;
    let mut morphed = Vec::with_capacity(sample_count);
    for k in 0..sample_count {
        let pa = resampled_a[k];
        let pb = resampled_b[(k + shift) % sample_count];
        let p = lerp2(pa, pb, eased_t);
        morphed.push(ShapeMaskVertex {
            pos: [p[0] * inv_w, p[1] * inv_h],
            in_tangent: [0.0, 0.0],
            out_tangent: [0.0, 0.0],
        });
    }
    Ok(morphed)
}

/// Builds a `lyon_tessellation::path::Path` from `vertices` in pixel coordinates and
/// flattens all cubic Bézier segments at `0.25 px` tolerance.
pub fn flatten_shape_mask_contour(
    vertices: &[ShapeMaskVertex],
    closed: bool,
    w_f: f32,
    h_f: f32,
) -> Vec<[f32; 2]> {
    if vertices.len() < 2 {
        return vertices
            .iter()
            .map(|v| [v.pos[0] * w_f, v.pos[1] * h_f])
            .collect();
    }

    let mut builder = LyonPath::builder();
    let first = [vertices[0].pos[0] * w_f, vertices[0].pos[1] * h_f];
    builder.begin(point(first[0], first[1]));

    let seg_count = if closed {
        vertices.len()
    } else {
        vertices.len() - 1
    };

    for i in 0..seg_count {
        let curr = &vertices[i];
        let next = &vertices[(i + 1) % vertices.len()];
        let has_bezier = curr.out_tangent[0].abs() > 1e-6
            || curr.out_tangent[1].abs() > 1e-6
            || next.in_tangent[0].abs() > 1e-6
            || next.in_tangent[1].abs() > 1e-6;
        let p1 = [next.pos[0] * w_f, next.pos[1] * h_f];
        if has_bezier {
            let c1 = [
                (curr.pos[0] + curr.out_tangent[0]) * w_f,
                (curr.pos[1] + curr.out_tangent[1]) * h_f,
            ];
            let c2 = [
                (next.pos[0] + next.in_tangent[0]) * w_f,
                (next.pos[1] + next.in_tangent[1]) * h_f,
            ];
            builder.cubic_bezier_to(point(c1[0], c1[1]), point(c2[0], c2[1]), point(p1[0], p1[1]));
        } else {
            builder.line_to(point(p1[0], p1[1]));
        }
    }
    builder.end(closed);
    let path = builder.build();

    let mut pts = Vec::new();
    for ev in path.iter().flattened(FLATTEN_TOLERANCE_PX) {
        match ev {
            Event::Begin { at } => {
                pts.push([at.x, at.y]);
            }
            Event::Line { to, .. } => {
                let pt = [to.x, to.y];
                if pts.last().is_none_or(|prev| dist_sq(*prev, pt) > 1e-6) {
                    pts.push(pt);
                }
            }
            Event::End { close: true, .. } => {
                if pts.len() >= 2
                    && dist_sq(*pts.first().unwrap(), *pts.last().unwrap()) <= 1e-6
                {
                    pts.pop();
                }
            }
            Event::End { .. } => {}
            Event::Quadratic { .. } | Event::Cubic { .. } => {}
        }
    }
    pts
}

fn validate_shape_mask_config(cfg: &ShapeMaskConfig) -> Result<(), VectorError> {
    if !cfg.feather_px.is_finite()
        || cfg.feather_px < 0.0
        || !cfg.expand_px.is_finite()
        || !cfg.opacity.is_finite()
        || !(0.0..=1.0).contains(&cfg.opacity)
    {
        return Err(VectorError::InvalidParameter(
            "ShapeMaskConfig requires feather_px >= 0, finite expand_px, and opacity in [0, 1]"
                .to_owned(),
        ));
    }
    let check_vert = |v: &ShapeMaskVertex| {
        v.pos[0].is_finite()
            && v.pos[1].is_finite()
            && v.in_tangent[0].is_finite()
            && v.in_tangent[1].is_finite()
            && v.out_tangent[0].is_finite()
            && v.out_tangent[1].is_finite()
    };
    if cfg.vertices.iter().any(|v| !check_vert(v))
        || cfg
            .keyframes
            .iter()
            .any(|kf| kf.vertices.iter().any(|v| !check_vert(v)))
    {
        return Err(VectorError::InvalidParameter(
            "ShapeMaskConfig contains non-finite vertex coordinates".to_owned(),
        ));
    }
    Ok(())
}

fn rasterize_closed_contour_into(contour: &[[f32; 2]], width: u32, height: u32, cov: &mut [f32]) {
    let Ok(canvas) = VectorCanvas::new(width, height) else {
        return;
    };
    let slices = [contour];
    let Some((min_x, min_y, max_x, max_y)) = contour_slices_bounds(&slices) else {
        return;
    };
    let Some((bx0, by0, bx1, by1)) =
        canvas.clipped_bounds(min_x - 1.0, min_y - 1.0, max_x + 1.0, max_y + 1.0)
    else {
        return;
    };
    let rw = (bx1 - bx0 + 1) as usize;
    let rh = (by1 - by0 + 1) as usize;
    let mut box_cov = vec![0.0_f32; rw * rh];
    rasterize_contour_slices_coverage(&slices, bx0, by0, bx1, by1, false, rw, &mut box_cov);

    let w_usize = width as usize;
    for row in 0..rh {
        let py = (by0 + row as u32) as usize;
        let dst_off = py * w_usize + bx0 as usize;
        let src_off = row * rw;
        cov[dst_off..dst_off + rw].copy_from_slice(&box_cov[src_off..src_off + rw]);
    }
}

fn rasterize_expanded_contour_into(
    contour: &[[f32; 2]],
    expand_px: f32,
    width: u32,
    height: u32,
    cov: &mut [f32],
) {
    let Ok(canvas) = VectorCanvas::new(width, height) else {
        return;
    };
    let slices = [contour];
    let Some((min_x, min_y, max_x, max_y)) = contour_slices_bounds(&slices) else {
        return;
    };
    let pad = expand_px.max(0.0) + 2.0;
    let Some((bx0, by0, bx1, by1)) =
        canvas.clipped_bounds(min_x - pad, min_y - pad, max_x + pad, max_y + pad)
    else {
        return;
    };

    let rw = (bx1 - bx0 + 1) as usize;
    let rh = (by1 - by0 + 1) as usize;
    let mut inside_cov = vec![0.0_f32; rw * rh];
    rasterize_contour_slices_coverage(&slices, bx0, by0, bx1, by1, false, rw, &mut inside_cov);

    let n = contour.len();
    let mut seg_bboxes = Vec::with_capacity(n);
    let max_reach = expand_px.abs() + 2.0;
    for i in 0..n {
        let a = contour[i];
        let b = contour[(i + 1) % n];
        seg_bboxes.push((
            a,
            b,
            a[0].min(b[0]) - max_reach,
            a[1].min(b[1]) - max_reach,
            a[0].max(b[0]) + max_reach,
            a[1].max(b[1]) + max_reach,
        ));
    }

    let w_usize = width as usize;
    for row in 0..rh {
        let py = (by0 + row as u32) as f32 + 0.5;
        let y_idx = (by0 + row as u32) as usize;
        for col in 0..rw {
            let px = (bx0 + col as u32) as f32 + 0.5;
            let x_idx = (bx0 + col as u32) as usize;
            let inside = inside_cov[row * rw + col] >= 0.5;

            let mut min_d2 = f32::INFINITY;
            for &(a, b, smin_x, smin_y, smax_x, smax_y) in &seg_bboxes {
                if px >= smin_x && px <= smax_x && py >= smin_y && py <= smax_y {
                    let d2 = sd_segment_sq([px, py], a, b);
                    if d2 < min_d2 {
                        min_d2 = d2;
                    }
                }
            }

            let c = if !min_d2.is_finite() {
                if inside { 1.0 } else { 0.0 }
            } else {
                let d = min_d2.sqrt();
                let signed = if inside { -d } else { d };
                smoothstep(expand_px + 0.5, expand_px - 0.5, signed)
            };
            cov[y_idx * w_usize + x_idx] = c;
        }
    }
}

/// Fast 3-pass separable box blur approximating a Gaussian feather in `O(W * H)` time.
fn apply_separable_feather(buf: &mut [f32], width: usize, height: usize, feather_px: f32) {
    let radius_f = (feather_px * 0.5).clamp(0.0, 256.0);
    if radius_f <= 1e-4 || width == 0 || height == 0 {
        return;
    }
    let radius = (radius_f.ceil() as usize).max(1);
    let mut tmp = vec![0.0_f32; buf.len()];
    let orig = if radius_f < 1.0 {
        Some(buf.to_vec())
    } else {
        None
    };

    for _ in 0..3 {
        // Horizontal pass: buf -> tmp
        let inv_w = 1.0 / ((2 * radius + 1) as f32);
        for y in 0..height {
            let row = &buf[y * width..(y + 1) * width];
            let out_row = &mut tmp[y * width..(y + 1) * width];
            let mut acc = 0.0_f32;
            for dx in -(radius as isize)..=(radius as isize) {
                let sx = dx.clamp(0, (width - 1) as isize) as usize;
                acc += row[sx];
            }
            for (x, slot) in out_row.iter_mut().enumerate().take(width) {
                *slot = acc * inv_w;
                let remove_x = (x as isize - radius as isize).clamp(0, (width - 1) as isize) as usize;
                let add_x = (x as isize + radius as isize + 1).clamp(0, (width - 1) as isize) as usize;
                acc += row[add_x] - row[remove_x];
            }
        }
        // Vertical pass: tmp -> buf
        for x in 0..width {
            let mut acc = 0.0_f32;
            for dy in -(radius as isize)..=(radius as isize) {
                let sy = dy.clamp(0, (height - 1) as isize) as usize;
                acc += tmp[sy * width + x];
            }
            for y in 0..height {
                buf[y * width + x] = acc * inv_w;
                let remove_y = (y as isize - radius as isize).clamp(0, (height - 1) as isize) as usize;
                let add_y = (y as isize + radius as isize + 1).clamp(0, (height - 1) as isize) as usize;
                acc += tmp[add_y * width + x] - tmp[remove_y * width + x];
            }
        }
    }

    if let Some(crisp) = orig {
        let t = radius_f;
        for (b, c) in buf.iter_mut().zip(crisp.iter()) {
            *b = *c + (*b - *c) * t;
        }
    }
}

fn resample_contour_arc_length(
    pts: &[[f32; 2]],
    closed: bool,
    target_count: usize,
) -> Vec<[f32; 2]> {
    if pts.is_empty() || target_count == 0 {
        return Vec::new();
    }
    if pts.len() == 1 {
        return vec![pts[0]; target_count];
    }

    let seg_count = if closed { pts.len() } else { pts.len() - 1 };
    let mut cum_len = Vec::with_capacity(seg_count + 1);
    cum_len.push(0.0_f32);
    let mut total = 0.0_f32;
    for i in 0..seg_count {
        let a = pts[i];
        let b = pts[(i + 1) % pts.len()];
        total += dist_sq(a, b).sqrt();
        cum_len.push(total);
    }

    if total <= 1e-6 {
        return vec![pts[0]; target_count];
    }

    let mut out = Vec::with_capacity(target_count);
    let denom = if closed {
        target_count as f32
    } else {
        (target_count - 1).max(1) as f32
    };
    let mut seg_idx = 0usize;
    for k in 0..target_count {
        let target_d = (k as f32 / denom) * total;
        while seg_idx + 1 < seg_count && cum_len[seg_idx + 1] < target_d {
            seg_idx += 1;
        }
        let d0 = cum_len[seg_idx];
        let d1 = cum_len[seg_idx + 1];
        let t = if (d1 - d0) > 1e-6 {
            ((target_d - d0) / (d1 - d0)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let a = pts[seg_idx];
        let b = pts[(seg_idx + 1) % pts.len()];
        out.push(lerp2(a, b, t));
    }
    out
}

fn best_cyclic_alignment(a: &[[f32; 2]], b: &[[f32; 2]]) -> usize {
    let n = a.len().min(b.len());
    if n == 0 {
        return 0;
    }
    let mut best_shift = 0usize;
    let mut best_cost = f32::INFINITY;
    for shift in 0..n {
        let mut cost = 0.0_f32;
        for k in 0..n {
            cost += dist_sq(a[k], b[(k + shift) % n]);
            if cost >= best_cost {
                break;
            }
        }
        if cost < best_cost {
            best_cost = cost;
            best_shift = shift;
        }
    }
    best_shift
}

#[inline]
fn apply_animation_easing(t: f32, easing: AnimationEasing) -> f32 {
    let t = t.clamp(0.0, 1.0);
    match easing {
        AnimationEasing::Linear => t,
        AnimationEasing::EaseIn => t * t,
        AnimationEasing::EaseOut => 1.0 - (1.0 - t) * (1.0 - t),
        AnimationEasing::EaseInOut => t * t * (3.0 - 2.0 * t),
        AnimationEasing::Step => {
            if t < 1.0 {
                0.0
            } else {
                1.0
            }
        }
    }
}

#[inline]
fn lerp2(a: [f32; 2], b: [f32; 2], t: f32) -> [f32; 2] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

#[inline]
fn dist_sq(a: [f32; 2], b: [f32; 2]) -> f32 {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    dx * dx + dy * dy
}

#[cfg(test)]
mod tests {
    use super::*;
    use frameshift_time::FLICKS_PER_SECOND;

    fn rect_vertices(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<ShapeMaskVertex> {
        vec![
            ShapeMaskVertex {
                pos: [x0, y0],
                in_tangent: [0.0, 0.0],
                out_tangent: [0.0, 0.0],
            },
            ShapeMaskVertex {
                pos: [x1, y0],
                in_tangent: [0.0, 0.0],
                out_tangent: [0.0, 0.0],
            },
            ShapeMaskVertex {
                pos: [x1, y1],
                in_tangent: [0.0, 0.0],
                out_tangent: [0.0, 0.0],
            },
            ShapeMaskVertex {
                pos: [x0, y1],
                in_tangent: [0.0, 0.0],
                out_tangent: [0.0, 0.0],
            },
        ]
    }

    #[test]
    fn shape_mask_same_length_morph_expand_invert_and_feather_work() {
        let cfg = ShapeMaskConfig {
            vertices: Vec::new(),
            keyframes: vec![
                ShapeMaskKeyframe {
                    tick: Flicks::ZERO,
                    vertices: rect_vertices(0.25, 0.25, 0.75, 0.75),
                    easing: AnimationEasing::Linear,
                },
                ShapeMaskKeyframe {
                    tick: Flicks(FLICKS_PER_SECOND),
                    vertices: rect_vertices(0.1, 0.1, 0.9, 0.9),
                    easing: AnimationEasing::Linear,
                },
            ],
            closed: true,
            feather_px: 0.0,
            expand_px: 0.0,
            invert: false,
            opacity: 1.0,
        };

        let mid = render_shape_mask_rgba16f(&cfg, Flicks(FLICKS_PER_SECOND / 2), 64, 64)
            .expect("render mid morph");
        // Center (32, 32) is inside
        assert!((mid.data[32 * 64 + 32][3] - 1.0).abs() < 1e-5);
        // Corner (2, 2) is outside
        assert!(mid.data[2 * 64 + 2][3] < 1e-5);

        // Inverted + half opacity: corner becomes 0.5, center becomes 0.0
        let inv_cfg = ShapeMaskConfig {
            invert: true,
            opacity: 0.5,
            ..cfg.clone()
        };
        let inv = render_shape_mask_rgba16f(&inv_cfg, Flicks(FLICKS_PER_SECOND / 2), 64, 64)
            .expect("render inverted");
        assert!((inv.data[2 * 64 + 2][3] - 0.5).abs() < 1e-5);
        assert!(inv.data[32 * 64 + 32][3] < 1e-5);

        // Expand grows coverage area; choke shrinks coverage area
        let expanded = render_shape_mask_rgba16f(
            &ShapeMaskConfig {
                expand_px: 4.0,
                ..cfg.clone()
            },
            Flicks::ZERO,
            64,
            64,
        )
        .unwrap();
        let choked = render_shape_mask_rgba16f(
            &ShapeMaskConfig {
                expand_px: -4.0,
                ..cfg.clone()
            },
            Flicks::ZERO,
            64,
            64,
        )
        .unwrap();
        let exp_sum: f32 = expanded.data.iter().map(|p| p[3]).sum();
        let choke_sum: f32 = choked.data.iter().map(|p| p[3]).sum();
        assert!(exp_sum > choke_sum + 200.0);

        // Feather produces smooth intermediate coverage near boundary
        let feathered = render_shape_mask_rgba16f(
            &ShapeMaskConfig {
                feather_px: 8.0,
                ..cfg
            },
            Flicks::ZERO,
            64,
            64,
        )
        .unwrap();
        let edge_px = feathered.data[16 * 64 + 32][3];
        assert!(edge_px > 0.05 && edge_px < 0.95, "expected soft feathered edge, got {edge_px}");
    }

    #[test]
    fn shape_mask_mismatched_vertex_count_arc_length_morph_is_deterministic() {
        let triangle = vec![
            ShapeMaskVertex {
                pos: [0.5, 0.2],
                in_tangent: [0.0, 0.0],
                out_tangent: [0.0, 0.0],
            },
            ShapeMaskVertex {
                pos: [0.8, 0.8],
                in_tangent: [0.0, 0.0],
                out_tangent: [0.0, 0.0],
            },
            ShapeMaskVertex {
                pos: [0.2, 0.8],
                in_tangent: [0.0, 0.0],
                out_tangent: [0.0, 0.0],
            },
        ];
        // Circle-like 4-vertex cubic Bézier pose
        let k = 0.18;
        let bezier_quad = vec![
            ShapeMaskVertex {
                pos: [0.5, 0.15],
                in_tangent: [-k, 0.0],
                out_tangent: [k, 0.0],
            },
            ShapeMaskVertex {
                pos: [0.85, 0.5],
                in_tangent: [0.0, -k],
                out_tangent: [0.0, k],
            },
            ShapeMaskVertex {
                pos: [0.5, 0.85],
                in_tangent: [k, 0.0],
                out_tangent: [-k, 0.0],
            },
            ShapeMaskVertex {
                pos: [0.15, 0.5],
                in_tangent: [0.0, k],
                out_tangent: [0.0, -k],
            },
        ];

        let kfs = vec![
            ShapeMaskKeyframe {
                tick: Flicks::ZERO,
                vertices: triangle,
                easing: AnimationEasing::EaseInOut,
            },
            ShapeMaskKeyframe {
                tick: Flicks(FLICKS_PER_SECOND),
                vertices: bezier_quad,
                easing: AnimationEasing::Linear,
            },
        ];

        let p1 = interpolate_shape_mask_vertices(
            &[],
            &kfs,
            true,
            Flicks(FLICKS_PER_SECOND / 2),
            128,
            128,
        )
        .expect("arc-length morph");
        let p2 = interpolate_shape_mask_vertices(
            &[],
            &kfs,
            true,
            Flicks(FLICKS_PER_SECOND / 2),
            128,
            128,
        )
        .expect("deterministic repeat");
        assert_eq!(p1, p2);
        assert!(p1.len() >= 64);

        let cfg = ShapeMaskConfig {
            vertices: Vec::new(),
            keyframes: kfs,
            closed: true,
            feather_px: 0.0,
            expand_px: 0.0,
            invert: false,
            opacity: 1.0,
        };
        let frame = render_shape_mask_rgba16f(&cfg, Flicks(FLICKS_PER_SECOND / 2), 64, 64)
            .expect("render mismatched morph");
        assert!((frame.data[32 * 64 + 32][3] - 1.0).abs() < 1e-5);
    }
}
