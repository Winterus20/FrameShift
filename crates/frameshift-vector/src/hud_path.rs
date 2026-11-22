use frameshift_media::{AlphaMode, Rgba16fFrame};
use frameshift_time::Flicks;
use lyon_tessellation::math::point;
use lyon_tessellation::path::iterator::PathIterator;
use lyon_tessellation::path::{Event, Path as LyonPath};
use serde::{Deserialize, Serialize};

use crate::canvas::{VectorCanvas, premultiply_rgba, sd_segment_sq, smoothstep};
use crate::error::VectorError;
use crate::text::{VectorTextConfig, draw_vector_text};

/// Cubic Bézier curve segment in normalized `[0, 1]` canvas coordinates for `VectorPathHud`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CubicBezierHudSegment {
    pub p0: [f32; 2],
    pub c1: [f32; 2],
    pub c2: [f32; 2],
    pub p1: [f32; 2],
}

/// Configuration for deterministic 2D HUD vector paths with trim-path, dashes, node dots, and data readout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VectorPathHudConfig {
    #[serde(default)]
    pub points_norm: Vec<[f32; 2]>,
    #[serde(default)]
    pub bezier_segments: Vec<CubicBezierHudSegment>,
    #[serde(default)]
    pub closed: bool,
    #[serde(default = "default_hud_stroke_width")]
    pub stroke_width_norm: f32,
    #[serde(default = "default_white_rgba")]
    pub stroke_rgba: [f32; 4],
    #[serde(default)]
    pub fill_rgba: [f32; 4],
    #[serde(default)]
    pub dash_len_norm: f32,
    #[serde(default)]
    pub gap_len_norm: f32,
    #[serde(default)]
    pub dash_offset_norm: f32,
    #[serde(default)]
    pub trim_start: f32,
    #[serde(default = "default_one")]
    pub trim_end: f32,
    #[serde(default)]
    pub node_dot_radius_norm: f32,
    #[serde(default)]
    pub data_value: f32,
    #[serde(default)]
    pub data_label: String,
    #[serde(default = "default_one")]
    pub opacity: f32,
}

impl Default for VectorPathHudConfig {
    fn default() -> Self {
        Self {
            points_norm: Vec::new(),
            bezier_segments: Vec::new(),
            closed: false,
            stroke_width_norm: default_hud_stroke_width(),
            stroke_rgba: default_white_rgba(),
            fill_rgba: [0.0, 0.0, 0.0, 0.0],
            dash_len_norm: 0.0,
            gap_len_norm: 0.0,
            dash_offset_norm: 0.0,
            trim_start: 0.0,
            trim_end: 1.0,
            node_dot_radius_norm: 0.0,
            data_value: 0.0,
            data_label: String::new(),
            opacity: 1.0,
        }
    }
}

fn default_hud_stroke_width() -> f32 {
    0.003
}

fn default_white_rgba() -> [f32; 4] {
    [1.0, 1.0, 1.0, 1.0]
}

fn default_one() -> f32 {
    1.0
}

/// Renders a `VectorPathHudConfig` into a premultiplied Linear Rec.709 `Rgba16fFrame`.
pub fn render_vector_path_hud_rgba16f(
    cfg: &VectorPathHudConfig,
    width: u32,
    height: u32,
    pts: Flicks,
) -> Result<Rgba16fFrame, VectorError> {
    let mut canvas = VectorCanvas::new(width, height)?;
    draw_vector_path_hud(&mut canvas, cfg)?;
    canvas.to_rgba16f_frame(pts, AlphaMode::Premultiplied)
}

/// Draws a `VectorPathHudConfig` onto `canvas` using `lyon_tessellation` flattening,
/// arc-length trim/dash slicing, single-pass analytical segment SDF stroke (`0.75px` AA),
/// junction node dots, and HUD label readout.
pub fn draw_vector_path_hud(
    canvas: &mut VectorCanvas,
    cfg: &VectorPathHudConfig,
) -> Result<(), VectorError> {
    validate_hud_config(cfg)?;

    let opacity = cfg.opacity.clamp(0.0, 1.0);
    if opacity <= 0.0 {
        return Ok(());
    }

    if (opacity - 1.0).abs() > 1e-6 {
        let mut scratch = VectorCanvas::new(canvas.width(), canvas.height())?;
        draw_vector_path_hud_opaque(&mut scratch, cfg)?;
        canvas.composite_canvas_with_opacity(&scratch, opacity);
        return Ok(());
    }

    draw_vector_path_hud_opaque(canvas, cfg)
}

fn draw_vector_path_hud_opaque(
    canvas: &mut VectorCanvas,
    cfg: &VectorPathHudConfig,
) -> Result<(), VectorError> {
    let w_f = canvas.width() as f32;
    let h_f = canvas.height() as f32;

    let (path, original_vertices_px) = build_hud_lyon_path(cfg, w_f, h_f);
    let subpaths = flatten_hud_subpaths(&path, cfg.closed, 0.25_f32);

    if cfg.closed && cfg.fill_rgba[3] > 1e-6 {
        let fill_contours: Vec<Vec<[f32; 2]>> = subpaths
            .iter()
            .filter(|s| s.points.len() >= 3)
            .map(|s| s.points.clone())
            .collect();
        if !fill_contours.is_empty() {
            let fill_premul = premultiply_rgba(cfg.fill_rgba);
            canvas.fill_contours_nonzero(&fill_contours, fill_premul);
        }
    }

    let parameterized = parameterize_subpaths(&subpaths);
    let total_len = parameterized.total_length;
    let t0 = cfg.trim_start.clamp(0.0, 1.0);
    let t1 = cfg.trim_end.clamp(0.0, 1.0);

    let mut trimmed_polylines: Vec<Vec<(f32, [f32; 2])>> = Vec::new();
    let mut active_endpoints: Vec<[f32; 2]> = Vec::new();
    let mut first_visible_pt: Option<[f32; 2]> = None;

    if t1 > t0 && total_len > 1e-4 {
        let s_min = t0 * total_len;
        let s_max = t1 * total_len;
        for sp in &parameterized.subpaths {
            let clipped = slice_polyline_arc_range(sp, s_min, s_max);
            if clipped.len() >= 2 {
                if first_visible_pt.is_none() {
                    first_visible_pt = Some(clipped[0].1);
                }
                active_endpoints.push(clipped[0].1);
                active_endpoints.push(clipped.last().unwrap().1);
                trimmed_polylines.push(clipped);
            }
        }
    } else if t1 > t0 && cfg.bezier_segments.is_empty() && cfg.points_norm.len() == 1 {
        let pt = [cfg.points_norm[0][0] * w_f, cfg.points_norm[0][1] * h_f];
        first_visible_pt = Some(pt);
        active_endpoints.push(pt);
    }

    let stroke_width_px = cfg.stroke_width_norm.max(0.0) * h_f;
    let stroke_premul = premultiply_rgba(cfg.stroke_rgba);
    if stroke_width_px > 0.0 && stroke_premul[3] > 0.0 && !trimmed_polylines.is_empty() {
        let dash_len_px = cfg.dash_len_norm.max(0.0) * h_f;
        let gap_len_px = cfg.gap_len_norm.max(0.0) * h_f;
        let dash_offset_px = cfg.dash_offset_norm * h_f;

        let stroke_segments = if dash_len_px > 0.5 && gap_len_px > 0.5 {
            slice_dashes(&trimmed_polylines, dash_len_px, gap_len_px, dash_offset_px)
        } else {
            collect_trimmed_segments(&trimmed_polylines)
        };

        if !stroke_segments.is_empty() {
            rasterize_sdf_stroke(canvas, &stroke_segments, stroke_width_px, stroke_premul);
        }
    }

    let dot_radius_px = cfg.node_dot_radius_norm.max(0.0) * h_f;
    if dot_radius_px > 0.0 && cfg.stroke_rgba[3] > 1e-6 && t1 > t0 {
        let mut dot_points = Vec::new();
        if total_len > 1e-4 {
            let s_min = t0 * total_len - 1e-3;
            let s_max = t1 * total_len + 1e-3;
            for &v in &original_vertices_px {
                if let Some(s) = closest_arc_position(&parameterized, v)
                    && s >= s_min
                    && s <= s_max
                {
                    push_unique_point(&mut dot_points, v);
                }
            }
        }
        for pt in active_endpoints {
            push_unique_point(&mut dot_points, pt);
        }
        for pt in dot_points {
            canvas.fill_circle(pt[0], pt[1], dot_radius_px, cfg.stroke_rgba);
        }
    }

    if !cfg.data_label.trim().is_empty() {
        let label_text = format_hud_label(&cfg.data_label, cfg.data_value);
        let anchor_px = first_visible_pt
            .or_else(|| original_vertices_px.first().copied())
            .unwrap_or([w_f * 0.5, h_f * 0.5]);
        let ax_norm = ((anchor_px[0] + 8.0) / w_f).clamp(0.02, 0.85);
        let ay_norm = ((anchor_px[1] - 12.0) / h_f).clamp(0.04, 0.96);
        let text_color = if cfg.stroke_rgba[3] > 1e-6 {
            cfg.stroke_rgba
        } else {
            [1.0, 1.0, 1.0, 1.0]
        };
        let label_cfg = VectorTextConfig {
            text: label_text,
            font_size_norm: (cfg.stroke_width_norm * 3.5).clamp(0.018, 0.04),
            color_rgba: text_color,
            outline_rgba: [0.0, 0.0, 0.0, 0.85],
            outline_width_norm: 0.0015,
            align_center: false,
            x_offset_norm: Some(ax_norm),
            y_offset_norm: ay_norm,
            ..VectorTextConfig::default()
        };
        draw_vector_text(canvas, &label_cfg)?;
    }

    Ok(())
}

fn validate_hud_config(cfg: &VectorPathHudConfig) -> Result<(), VectorError> {
    let scalars_ok = cfg.stroke_width_norm.is_finite()
        && cfg.stroke_width_norm >= 0.0
        && cfg.dash_len_norm.is_finite()
        && cfg.dash_len_norm >= 0.0
        && cfg.gap_len_norm.is_finite()
        && cfg.gap_len_norm >= 0.0
        && cfg.dash_offset_norm.is_finite()
        && cfg.trim_start.is_finite()
        && cfg.trim_end.is_finite()
        && cfg.node_dot_radius_norm.is_finite()
        && cfg.node_dot_radius_norm >= 0.0
        && cfg.data_value.is_finite()
        && cfg.opacity.is_finite()
        && cfg.opacity >= 0.0
        && cfg.stroke_rgba.iter().all(|v| v.is_finite())
        && cfg.fill_rgba.iter().all(|v| v.is_finite());

    let points_ok = cfg
        .points_norm
        .iter()
        .all(|p| p[0].is_finite() && p[1].is_finite());
    let beziers_ok = cfg.bezier_segments.iter().all(|s| {
        s.p0[0].is_finite()
            && s.p0[1].is_finite()
            && s.c1[0].is_finite()
            && s.c1[1].is_finite()
            && s.c2[0].is_finite()
            && s.c2[1].is_finite()
            && s.p1[0].is_finite()
            && s.p1[1].is_finite()
    });

    if !scalars_ok || !points_ok || !beziers_ok {
        return Err(VectorError::InvalidParameter(
            "VectorPathHudConfig contains invalid or non-finite parameters".to_string(),
        ));
    }
    Ok(())
}

fn build_hud_lyon_path(cfg: &VectorPathHudConfig, w_f: f32, h_f: f32) -> (LyonPath, Vec<[f32; 2]>) {
    let mut builder = LyonPath::builder();
    let mut vertices = Vec::new();

    if !cfg.bezier_segments.is_empty() {
        let mut in_subpath = false;
        let mut prev_end: Option<[f32; 2]> = None;

        for seg in &cfg.bezier_segments {
            let p0 = [seg.p0[0] * w_f, seg.p0[1] * h_f];
            let c1 = [seg.c1[0] * w_f, seg.c1[1] * h_f];
            let c2 = [seg.c2[0] * w_f, seg.c2[1] * h_f];
            let p1 = [seg.p1[0] * w_f, seg.p1[1] * h_f];

            let connected = prev_end.is_some_and(|pe| {
                let dx = pe[0] - p0[0];
                let dy = pe[1] - p0[1];
                dx * dx + dy * dy <= 1e-4
            });

            if !in_subpath || !connected {
                if in_subpath {
                    builder.end(false);
                }
                builder.begin(point(p0[0], p0[1]));
                in_subpath = true;
                vertices.push(p0);
            }
            builder.cubic_bezier_to(
                point(c1[0], c1[1]),
                point(c2[0], c2[1]),
                point(p1[0], p1[1]),
            );
            vertices.push(p1);
            prev_end = Some(p1);
        }

        if in_subpath {
            builder.end(cfg.closed);
        }
    } else if cfg.points_norm.len() >= 2 {
        let first = [cfg.points_norm[0][0] * w_f, cfg.points_norm[0][1] * h_f];
        builder.begin(point(first[0], first[1]));
        vertices.push(first);
        for p in &cfg.points_norm[1..] {
            let pt = [p[0] * w_f, p[1] * h_f];
            builder.line_to(point(pt[0], pt[1]));
            vertices.push(pt);
        }
        builder.end(cfg.closed);
    }

    (builder.build(), vertices)
}

struct FlattenedSubpath {
    points: Vec<[f32; 2]>,
    closed: bool,
}

fn flatten_hud_subpaths(
    path: &LyonPath,
    fallback_closed: bool,
    tolerance: f32,
) -> Vec<FlattenedSubpath> {
    let mut subpaths = Vec::new();
    let mut current: Option<Vec<[f32; 2]>> = None;

    for event in path.iter().flattened(tolerance) {
        match event {
            Event::Begin { at } => {
                if let Some(pts) = current.take()
                    && pts.len() >= 2
                {
                    subpaths.push(FlattenedSubpath {
                        points: pts,
                        closed: false,
                    });
                }
                current = Some(vec![[at.x, at.y]]);
            }
            Event::Line { to, .. } => {
                if let Some(pts) = current.as_mut() {
                    let next = [to.x, to.y];
                    if let Some(&prev) = pts.last() {
                        let dx = next[0] - prev[0];
                        let dy = next[1] - prev[1];
                        if dx * dx + dy * dy > 1e-10 {
                            pts.push(next);
                        }
                    } else {
                        pts.push(next);
                    }
                }
            }
            Event::End { close, .. } => {
                if let Some(mut pts) = current.take() {
                    let is_closed = close || fallback_closed;
                    if is_closed && pts.len() >= 2 {
                        let first = pts[0];
                        let last = *pts.last().unwrap();
                        let dx = first[0] - last[0];
                        let dy = first[1] - last[1];
                        if dx * dx + dy * dy <= 1e-10 {
                            pts.pop();
                        }
                    }
                    if pts.len() >= 2 {
                        subpaths.push(FlattenedSubpath {
                            points: pts,
                            closed: is_closed,
                        });
                    }
                }
            }
            Event::Quadratic { .. } | Event::Cubic { .. } => {}
        }
    }

    if let Some(pts) = current.take()
        && pts.len() >= 2
    {
        subpaths.push(FlattenedSubpath {
            points: pts,
            closed: false,
        });
    }

    subpaths
}

struct ParameterizedPath {
    subpaths: Vec<Vec<(f32, [f32; 2])>>,
    total_length: f32,
}

fn parameterize_subpaths(subpaths: &[FlattenedSubpath]) -> ParameterizedPath {
    let mut out = Vec::with_capacity(subpaths.len());
    let mut s = 0.0_f32;

    for sp in subpaths {
        if sp.points.len() < 2 {
            continue;
        }
        let mut poly = Vec::with_capacity(sp.points.len() + usize::from(sp.closed));
        let mut prev = sp.points[0];
        poly.push((s, prev));
        for &pt in &sp.points[1..] {
            let dx = pt[0] - prev[0];
            let dy = pt[1] - prev[1];
            s += (dx * dx + dy * dy).sqrt();
            poly.push((s, pt));
            prev = pt;
        }
        if sp.closed {
            let first = sp.points[0];
            let dx = first[0] - prev[0];
            let dy = first[1] - prev[1];
            let d = (dx * dx + dy * dy).sqrt();
            if d > 1e-6 {
                s += d;
                poly.push((s, first));
            }
        }
        out.push(poly);
    }

    ParameterizedPath {
        subpaths: out,
        total_length: s,
    }
}

fn slice_polyline_arc_range(
    poly: &[(f32, [f32; 2])],
    s_min: f32,
    s_max: f32,
) -> Vec<(f32, [f32; 2])> {
    if poly.len() < 2 || s_max <= s_min {
        return Vec::new();
    }
    let sp_start = poly.first().unwrap().0;
    let sp_end = poly.last().unwrap().0;
    if sp_end <= s_min || sp_start >= s_max {
        return Vec::new();
    }

    let mut out = Vec::new();
    for seg in poly.windows(2) {
        let (s0, p0) = seg[0];
        let (s1, p1) = seg[1];
        let seg_len = s1 - s0;
        if seg_len <= 1e-8 || s1 <= s_min || s0 >= s_max {
            continue;
        }
        let a_s = s0.max(s_min);
        let b_s = s1.min(s_max);
        if b_s <= a_s + 1e-6 {
            continue;
        }
        let ta = ((a_s - s0) / seg_len).clamp(0.0, 1.0);
        let tb = ((b_s - s0) / seg_len).clamp(0.0, 1.0);
        let pa = lerp_pt(p0, p1, ta);
        let pb = lerp_pt(p0, p1, tb);
        if out.is_empty() {
            out.push((a_s, pa));
        } else {
            let last_pt = out.last().unwrap().1;
            let dx = last_pt[0] - pa[0];
            let dy = last_pt[1] - pa[1];
            if dx * dx + dy * dy > 1e-6 {
                out.push((a_s, pa));
            }
        }
        out.push((b_s, pb));
    }
    out
}

struct StrokeSegment {
    a: [f32; 2],
    b: [f32; 2],
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
}

impl StrokeSegment {
    #[inline]
    fn new(a: [f32; 2], b: [f32; 2]) -> Self {
        Self {
            a,
            b,
            min_x: a[0].min(b[0]),
            min_y: a[1].min(b[1]),
            max_x: a[0].max(b[0]),
            max_y: a[1].max(b[1]),
        }
    }
}

fn collect_trimmed_segments(polylines: &[Vec<(f32, [f32; 2])>]) -> Vec<StrokeSegment> {
    let mut segs = Vec::new();
    for poly in polylines {
        for win in poly.windows(2) {
            segs.push(StrokeSegment::new(win[0].1, win[1].1));
        }
    }
    segs
}

fn slice_dashes(
    polylines: &[Vec<(f32, [f32; 2])>],
    dash_len: f32,
    gap_len: f32,
    dash_offset: f32,
) -> Vec<StrokeSegment> {
    let period = dash_len + gap_len;
    if period <= 1e-4 {
        return collect_trimmed_segments(polylines);
    }

    let mut segs = Vec::new();
    for poly in polylines {
        for win in poly.windows(2) {
            let (s0, p0) = win[0];
            let (s1, p1) = win[1];
            let seg_len = s1 - s0;
            if seg_len <= 1e-6 {
                continue;
            }

            let shifted_start = s0 - dash_offset;
            let shifted_end = s1 - dash_offset;
            let first_k = (shifted_start / period).floor() as i64;
            let last_k = (shifted_end / period).ceil() as i64;

            for k in first_k..=last_k {
                let cycle_start = (k as f32) * period + dash_offset;
                let dash_end = cycle_start + dash_len;
                let a_s = s0.max(cycle_start);
                let b_s = s1.min(dash_end);
                if b_s > a_s + 1e-5 {
                    let ta = ((a_s - s0) / seg_len).clamp(0.0, 1.0);
                    let tb = ((b_s - s0) / seg_len).clamp(0.0, 1.0);
                    segs.push(StrokeSegment::new(lerp_pt(p0, p1, ta), lerp_pt(p0, p1, tb)));
                }
            }
        }
    }
    segs
}

fn rasterize_sdf_stroke(
    canvas: &mut VectorCanvas,
    segments: &[StrokeSegment],
    stroke_width_px: f32,
    stroke_premul: [f32; 4],
) {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for seg in segments {
        min_x = min_x.min(seg.min_x);
        min_y = min_y.min(seg.min_y);
        max_x = max_x.max(seg.max_x);
        max_y = max_y.max(seg.max_y);
    }

    let half_w = stroke_width_px * 0.5;
    let edge0 = half_w + 0.75;
    let edge1 = half_w - 0.75;
    let pad = edge0 + 1.0;

    let Some((bx0, by0, bx1, by1)) =
        canvas.clipped_bounds(min_x - pad, min_y - pad, max_x + pad, max_y + pad)
    else {
        return;
    };

    let max_r = edge0 + 0.5;
    let max_r_sq = max_r * max_r;

    for py in by0..=by1 {
        let p_y = py as f32 + 0.5;
        for px in bx0..=bx1 {
            let p_x = px as f32 + 0.5;
            let mut min_dist_sq = max_r_sq;
            for seg in segments {
                if p_x < seg.min_x - max_r
                    || p_x > seg.max_x + max_r
                    || p_y < seg.min_y - max_r
                    || p_y > seg.max_y + max_r
                {
                    continue;
                }
                let d_sq = sd_segment_sq([p_x, p_y], seg.a, seg.b);
                if d_sq < min_dist_sq {
                    min_dist_sq = d_sq;
                }
            }
            if min_dist_sq < max_r_sq {
                let cov = smoothstep(edge0, edge1, min_dist_sq.sqrt());
                if cov > 0.0 {
                    canvas.blend_pixel_premul(px, py, stroke_premul, cov);
                }
            }
        }
    }
}

fn closest_arc_position(param: &ParameterizedPath, pt: [f32; 2]) -> Option<f32> {
    let mut best_dist_sq = f32::INFINITY;
    let mut best_s = None;
    for sp in &param.subpaths {
        for &(s, p) in sp {
            let dx = p[0] - pt[0];
            let dy = p[1] - pt[1];
            let d_sq = dx * dx + dy * dy;
            if d_sq < best_dist_sq {
                best_dist_sq = d_sq;
                best_s = Some(s);
            }
        }
    }
    best_s
}

fn push_unique_point(out: &mut Vec<[f32; 2]>, pt: [f32; 2]) {
    let exists = out.iter().any(|p| {
        let dx = p[0] - pt[0];
        let dy = p[1] - pt[1];
        dx * dx + dy * dy <= 0.25
    });
    if !exists {
        out.push(pt);
    }
}

fn format_hud_label(label: &str, value: f32) -> String {
    let trimmed = label.trim();
    if trimmed.contains("{val}") {
        trimmed.replace("{val}", &format!("{value:.2}"))
    } else if trimmed.contains("{value}") {
        trimmed.replace("{value}", &format!("{value:.2}"))
    } else if value.abs() > 1e-6 || label.ends_with(':') || label.ends_with(' ') {
        format!("{trimmed} {value:.2}")
    } else {
        trimmed.to_string()
    }
}

#[inline]
fn lerp_pt(a: [f32; 2], b: [f32; 2], t: f32) -> [f32; 2] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hud_trim_zero_produces_empty_frame() {
        let cfg = VectorPathHudConfig {
            points_norm: vec![[0.1, 0.5], [0.9, 0.5]],
            stroke_width_norm: 0.01,
            trim_start: 0.5,
            trim_end: 0.5,
            ..VectorPathHudConfig::default()
        };
        let frame = render_vector_path_hud_rgba16f(&cfg, 128, 128, Flicks::ZERO).unwrap();
        assert!(frame.data.iter().all(|px| px[3] == 0.0));
    }

    #[test]
    fn hud_bezier_dash_and_dots_render_deterministically() {
        let cfg = VectorPathHudConfig {
            bezier_segments: vec![CubicBezierHudSegment {
                p0: [0.1, 0.8],
                c1: [0.3, 0.2],
                c2: [0.7, 0.2],
                p1: [0.9, 0.8],
            }],
            stroke_width_norm: 0.008,
            stroke_rgba: [0.2, 0.9, 1.0, 0.8],
            dash_len_norm: 0.06,
            gap_len_norm: 0.04,
            trim_start: 0.1,
            trim_end: 0.9,
            node_dot_radius_norm: 0.015,
            data_label: "VEL:".to_string(),
            data_value: 42.5,
            opacity: 0.9,
            ..VectorPathHudConfig::default()
        };
        let a = render_vector_path_hud_rgba16f(&cfg, 320, 180, Flicks::ZERO).unwrap();
        let b = render_vector_path_hud_rgba16f(&cfg, 320, 180, Flicks::ZERO).unwrap();
        assert_eq!(a.data, b.data);
        let non_empty = a.data.iter().filter(|px| px[3] > 0.01).count();
        assert!(
            non_empty > 50,
            "expected rendered HUD pixels, got {non_empty}"
        );
    }
}
