use crate::error::VectorError;
use frameshift_media::{linear_to_srgb, srgb_to_linear, AlphaMode, Rgba16fFrame, Rgba8Frame};
use frameshift_time::Flicks;

/// 4x4 sub-pixel sample offsets for deterministic supersampled polygon rasterization.
const SUBPIXEL_OFFSETS: [f32; 4] = [0.125, 0.375, 0.625, 0.875];
const MAX_BEZIER_DEPTH: u8 = 10;

/// Deterministic CPU 2D vector canvas operating in premultiplied Linear Rec.709 `[f32; 4]`.
///
/// Pixel coordinates place `(0.0, 0.0)` at the top-left corner of the canvas, with pixel `(x, y)`
/// centered at `(x as f32 + 0.5, y as f32 + 0.5)`.
///
/// All color inputs accepted by drawing methods are **straight (unassociated) Linear Rec.709**
/// `[R, G, B, A]`. Internally, every pixel in `VectorCanvas` is stored in **premultiplied**
/// Linear Rec.709 `[R*A, G*A, B*A, A]` and composited using Porter-Duff `SourceOver`.
#[derive(Debug, Clone, PartialEq)]
pub struct VectorCanvas {
    width: u32,
    height: u32,
    data: Vec<[f32; 4]>,
}

impl VectorCanvas {
    /// Creates a transparent `[0.0, 0.0, 0.0, 0.0]` canvas with the given non-zero dimensions.
    pub fn new(width: u32, height: u32) -> Result<Self, VectorError> {
        let len = validated_pixel_count(width, height)?;
        Ok(Self {
            width,
            height,
            data: vec![[0.0, 0.0, 0.0, 0.0]; len],
        })
    }

    /// Creates a canvas filled uniformly with a straight Linear Rec.709 `color_rgba`.
    pub fn filled(width: u32, height: u32, color_rgba: [f32; 4]) -> Result<Self, VectorError> {
        let len = validated_pixel_count(width, height)?;
        let premul = premultiply_rgba(color_rgba);
        Ok(Self {
            width,
            height,
            data: vec![premul; len],
        })
    }

    /// Constructs a `VectorCanvas` by converting a Display sRGB `Rgba8Frame` to premultiplied Linear Rec.709.
    pub fn from_rgba8_frame(frame: &Rgba8Frame) -> Result<Self, VectorError> {
        let linear = Rgba16fFrame::from_srgb_rgba8(frame)?;
        Ok(Self {
            width: linear.width,
            height: linear.height,
            data: linear.data,
        })
    }

    /// Canvas width in pixels.
    #[inline]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Canvas height in pixels.
    #[inline]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Read-only slice of premultiplied Linear Rec.709 pixels in row-major order.
    #[inline]
    pub fn pixels(&self) -> &[[f32; 4]] {
        &self.data
    }

    /// Mutable slice of premultiplied Linear Rec.709 pixels in row-major order.
    #[inline]
    pub fn pixels_mut(&mut self) -> &mut [[f32; 4]] {
        &mut self.data
    }

    /// Clears the entire canvas to transparent black `[0.0, 0.0, 0.0, 0.0]`.
    pub fn clear(&mut self) {
        self.data.fill([0.0, 0.0, 0.0, 0.0]);
    }

    /// Returns the premultiplied Linear Rec.709 pixel at `(x, y)`, if in bounds.
    #[inline]
    pub fn get_pixel(&self, x: u32, y: u32) -> Option<[f32; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let idx = (y as usize) * (self.width as usize) + (x as usize);
        Some(self.data[idx])
    }

    /// Composites a premultiplied Linear Rec.709 color scaled by scalar `coverage in [0, 1]`
    /// onto pixel `(x, y)` using Porter-Duff `SourceOver`.
    #[inline]
    pub fn blend_pixel_premul(&mut self, x: u32, y: u32, src_premul: [f32; 4], coverage: f32) {
        if x >= self.width || y >= self.height || !coverage.is_finite() || coverage <= 0.0 {
            return;
        }
        let cov = coverage.clamp(0.0, 1.0);
        let src = [
            src_premul[0] * cov,
            src_premul[1] * cov,
            src_premul[2] * cov,
            src_premul[3] * cov,
        ];
        let idx = (y as usize) * (self.width as usize) + (x as usize);
        self.data[idx] = source_over_premul(self.data[idx], src);
    }

    /// Exports this canvas as a Linear Rec.709 `Rgba16fFrame` in the requested `AlphaMode`.
    pub fn to_rgba16f_frame(
        &self,
        pts: Flicks,
        alpha_mode: AlphaMode,
    ) -> Result<Rgba16fFrame, VectorError> {
        let data = match alpha_mode {
            AlphaMode::Premultiplied => self.data.clone(),
            AlphaMode::Straight => self.data.iter().copied().map(unpremultiply_rgba).collect(),
        };
        Ok(Rgba16fFrame::new(
            self.width,
            self.height,
            pts,
            alpha_mode,
            data,
        )?)
    }

    /// Exports this canvas as a Display sRGB `Rgba8Frame` in the requested `AlphaMode`.
    pub fn to_rgba8_frame(
        &self,
        pts: Flicks,
        alpha_mode: AlphaMode,
    ) -> Result<Rgba8Frame, VectorError> {
        let mut bytes = Vec::with_capacity(self.data.len() * 4);
        for &px in &self.data {
            let a = sanitize_unit(px[3]);
            let [lr, lg, lb, _] = unpremultiply_rgba(px);
            let sr = linear_to_srgb(lr);
            let sg = linear_to_srgb(lg);
            let sb = linear_to_srgb(lb);

            let (out_r, out_g, out_b) = match alpha_mode {
                AlphaMode::Straight => (sr, sg, sb),
                AlphaMode::Premultiplied => (sr * a, sg * a, sb * a),
            };

            bytes.push((out_r * 255.0).round().clamp(0.0, 255.0) as u8);
            bytes.push((out_g * 255.0).round().clamp(0.0, 255.0) as u8);
            bytes.push((out_b * 255.0).round().clamp(0.0, 255.0) as u8);
            bytes.push((a * 255.0).round().clamp(0.0, 255.0) as u8);
        }
        Ok(Rgba8Frame::new(
            self.width,
            self.height,
            pts,
            alpha_mode,
            bytes,
        )?)
    }

    /// Draws an anti-aliased axis-aligned filled rectangle using analytical box SDF coverage.
    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color_rgba: [f32; 4]) {
        self.fill_rounded_rect(x, y, w, h, 0.0, color_rgba);
    }

    /// Draws an anti-aliased filled rounded rectangle using an exact rounded-box SDF.
    pub fn fill_rounded_rect(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radius: f32,
        color_rgba: [f32; 4],
    ) {
        if !are_finite(&[x, y, w, h, radius]) || w <= 0.0 || h <= 0.0 {
            return;
        }
        let src = premultiply_rgba(color_rgba);
        if src[3] <= 0.0 {
            return;
        }

        let r = radius.max(0.0).min(w * 0.5).min(h * 0.5);
        let cx = x + w * 0.5;
        let cy = y + h * 0.5;
        let half_w = w * 0.5;
        let half_h = h * 0.5;

        let Some((x0, y0, x1, y1)) = self.clipped_bounds(x - 1.5, y - 1.5, x + w + 1.5, y + h + 1.5)
        else {
            return;
        };

        for py in y0..=y1 {
            let p_y = py as f32 + 0.5;
            for px in x0..=x1 {
                let p_x = px as f32 + 0.5;
                let dist = sd_rounded_box(p_x - cx, p_y - cy, half_w, half_h, r);
                let cov = sdf_coverage(dist);
                if cov > 0.0 {
                    self.blend_pixel_premul(px, py, src, cov);
                }
            }
        }
    }

    /// Draws an anti-aliased stroked rounded rectangle centered on the perimeter with `stroke_width`.
    #[allow(clippy::too_many_arguments)]
    pub fn stroke_rounded_rect(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radius: f32,
        stroke_width: f32,
        color_rgba: [f32; 4],
    ) {
        if !are_finite(&[x, y, w, h, radius, stroke_width])
            || w <= 0.0
            || h <= 0.0
            || stroke_width <= 0.0
        {
            return;
        }
        let src = premultiply_rgba(color_rgba);
        if src[3] <= 0.0 {
            return;
        }

        let r = radius.max(0.0).min(w * 0.5).min(h * 0.5);
        let half_stroke = stroke_width * 0.5;
        let cx = x + w * 0.5;
        let cy = y + h * 0.5;
        let half_w = w * 0.5;
        let half_h = h * 0.5;
        let pad = half_stroke + 1.5;

        let Some((x0, y0, x1, y1)) = self.clipped_bounds(x - pad, y - pad, x + w + pad, y + h + pad)
        else {
            return;
        };

        for py in y0..=y1 {
            let p_y = py as f32 + 0.5;
            for px in x0..=x1 {
                let p_x = px as f32 + 0.5;
                let box_dist = sd_rounded_box(p_x - cx, p_y - cy, half_w, half_h, r);
                let ring_dist = box_dist.abs() - half_stroke;
                let cov = sdf_coverage(ring_dist);
                if cov > 0.0 {
                    self.blend_pixel_premul(px, py, src, cov);
                }
            }
        }
    }

    /// Draws an anti-aliased filled circle using an exact Euclidean circle SDF.
    pub fn fill_circle(&mut self, cx: f32, cy: f32, radius: f32, color_rgba: [f32; 4]) {
        if !are_finite(&[cx, cy, radius]) || radius <= 0.0 {
            return;
        }
        let src = premultiply_rgba(color_rgba);
        if src[3] <= 0.0 {
            return;
        }

        let pad = radius + 1.5;
        let Some((x0, y0, x1, y1)) = self.clipped_bounds(cx - pad, cy - pad, cx + pad, cy + pad)
        else {
            return;
        };

        for py in y0..=y1 {
            let dy = py as f32 + 0.5 - cy;
            for px in x0..=x1 {
                let dx = px as f32 + 0.5 - cx;
                let dist = (dx * dx + dy * dy).sqrt() - radius;
                let cov = sdf_coverage(dist);
                if cov > 0.0 {
                    self.blend_pixel_premul(px, py, src, cov);
                }
            }
        }
    }

    /// Draws an anti-aliased stroked ring centered at `(cx, cy)` with mean `radius` and `stroke_width`.
    pub fn stroke_ring(
        &mut self,
        cx: f32,
        cy: f32,
        radius: f32,
        stroke_width: f32,
        color_rgba: [f32; 4],
    ) {
        if !are_finite(&[cx, cy, radius, stroke_width]) || radius < 0.0 || stroke_width <= 0.0 {
            return;
        }
        let src = premultiply_rgba(color_rgba);
        if src[3] <= 0.0 {
            return;
        }

        let half_stroke = stroke_width * 0.5;
        let pad = radius + half_stroke + 1.5;
        let Some((x0, y0, x1, y1)) = self.clipped_bounds(cx - pad, cy - pad, cx + pad, cy + pad)
        else {
            return;
        };

        for py in y0..=y1 {
            let dy = py as f32 + 0.5 - cy;
            for px in x0..=x1 {
                let dx = px as f32 + 0.5 - cx;
                let dist = ((dx * dx + dy * dy).sqrt() - radius).abs() - half_stroke;
                let cov = sdf_coverage(dist);
                if cov > 0.0 {
                    self.blend_pixel_premul(px, py, src, cov);
                }
            }
        }
    }

    /// Draws an anti-aliased line segment with round caps using a capsule SDF.
    pub fn draw_line_aa(
        &mut self,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        stroke_width: f32,
        color_rgba: [f32; 4],
    ) {
        if !are_finite(&[x0, y0, x1, y1, stroke_width]) || stroke_width <= 0.0 {
            return;
        }
        let src = premultiply_rgba(color_rgba);
        if src[3] <= 0.0 {
            return;
        }

        let half_stroke = stroke_width * 0.5;
        let pad = half_stroke + 1.5;
        let min_x = x0.min(x1) - pad;
        let min_y = y0.min(y1) - pad;
        let max_x = x0.max(x1) + pad;
        let max_y = y0.max(y1) + pad;

        let Some((bx0, by0, bx1, by1)) = self.clipped_bounds(min_x, min_y, max_x, max_y) else {
            return;
        };

        for py in by0..=by1 {
            let p_y = py as f32 + 0.5;
            for px in bx0..=bx1 {
                let p_x = px as f32 + 0.5;
                let dist = sd_segment([p_x, p_y], [x0, y0], [x1, y1]) - half_stroke;
                let cov = sdf_coverage(dist);
                if cov > 0.0 {
                    self.blend_pixel_premul(px, py, src, cov);
                }
            }
        }
    }

    /// Draws an anti-aliased cubic Bézier curve using adaptive De Casteljau flattening
    /// and a joint-free minimum-distance polyline capsule SDF.
    pub fn draw_cubic_bezier_aa(
        &mut self,
        p0: [f32; 2],
        p1: [f32; 2],
        p2: [f32; 2],
        p3: [f32; 2],
        stroke_width: f32,
        color_rgba: [f32; 4],
    ) {
        if !are_finite(&[
            p0[0],
            p0[1],
            p1[0],
            p1[1],
            p2[0],
            p2[1],
            p3[0],
            p3[1],
            stroke_width,
        ]) || stroke_width <= 0.0
        {
            return;
        }
        let src = premultiply_rgba(color_rgba);
        if src[3] <= 0.0 {
            return;
        }

        let mut pts = Vec::with_capacity(32);
        pts.push(p0);
        flatten_cubic_bezier(p0, p1, p2, p3, 0.15, 0, &mut pts);
        if pts.len() < 2 {
            pts.push(p3);
        }

        let mut min_x = f32::INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut max_y = f32::NEG_INFINITY;
        for &[px, py] in &pts {
            min_x = min_x.min(px);
            min_y = min_y.min(py);
            max_x = max_x.max(px);
            max_y = max_y.max(py);
        }

        let half_stroke = stroke_width * 0.5;
        let pad = half_stroke + 1.5;
        let Some((bx0, by0, bx1, by1)) =
            self.clipped_bounds(min_x - pad, min_y - pad, max_x + pad, max_y + pad)
        else {
            return;
        };

        for py in by0..=by1 {
            let p_y = py as f32 + 0.5;
            for px in bx0..=bx1 {
                let p_x = px as f32 + 0.5;
                let mut min_dist = f32::INFINITY;
                for seg in pts.windows(2) {
                    let d = sd_segment([p_x, p_y], seg[0], seg[1]);
                    if d < min_dist {
                        min_dist = d;
                    }
                }
                let cov = sdf_coverage(min_dist - half_stroke);
                if cov > 0.0 {
                    self.blend_pixel_premul(px, py, src, cov);
                }
            }
        }
    }

    /// Validates and fills a closed polygon using 4x4 sub-pixel supersampling and non-zero winding.
    pub fn try_fill_polygon(
        &mut self,
        points: &[[f32; 2]],
        color_rgba: [f32; 4],
    ) -> Result<(), VectorError> {
        if points.len() < 3 || points.iter().any(|p| !p[0].is_finite() || !p[1].is_finite()) {
            return Err(VectorError::EmptyPolygon(points.len()));
        }
        self.fill_polygon(points, color_rgba);
        Ok(())
    }

    /// Fills a closed polygon using deterministic 4x4 sub-pixel supersampling and non-zero winding.
    pub fn fill_polygon(&mut self, points: &[[f32; 2]], color_rgba: [f32; 4]) {
        if points.len() < 3 || points.iter().any(|p| !p[0].is_finite() || !p[1].is_finite()) {
            return;
        }
        let src = premultiply_rgba(color_rgba);
        if src[3] <= 0.0 {
            return;
        }
        self.fill_contour_slices_with_rule(&[points], src, false);
    }

    /// Fills multiple closed contours (outer boundaries and holes such as `O`, `B`, `A`)
    /// using deterministic 4x4 sub-pixel supersampling and the `NonZero` winding rule.
    ///
    /// `color_premul` is premultiplied Linear Rec.709 `[R*A, G*A, B*A, A]`.
    pub fn fill_contours_nonzero(&mut self, contours: &[Vec<[f32; 2]>], color_premul: [f32; 4]) {
        if !are_finite(&color_premul) || color_premul[3] <= 0.0 {
            return;
        }
        let slices: Vec<&[[f32; 2]]> = contours
            .iter()
            .filter(|c| c.len() >= 3 && c.iter().all(|p| p[0].is_finite() && p[1].is_finite()))
            .map(|c| c.as_slice())
            .collect();
        if slices.is_empty() {
            return;
        }
        self.fill_contour_slices_with_rule(&slices, color_premul, false);
    }

    /// Fills multiple closed contours using deterministic 4x4 sub-pixel supersampling
    /// and the `EvenOdd` parity rule.
    ///
    /// `color_premul` is premultiplied Linear Rec.709 `[R*A, G*A, B*A, A]`.
    pub fn fill_contours_evenodd(&mut self, contours: &[Vec<[f32; 2]>], color_premul: [f32; 4]) {
        if !are_finite(&color_premul) || color_premul[3] <= 0.0 {
            return;
        }
        let slices: Vec<&[[f32; 2]]> = contours
            .iter()
            .filter(|c| c.len() >= 3 && c.iter().all(|p| p[0].is_finite() && p[1].is_finite()))
            .map(|c| c.as_slice())
            .collect();
        if slices.is_empty() {
            return;
        }
        self.fill_contour_slices_with_rule(&slices, color_premul, true);
    }

    /// Composites another same-sized premultiplied `VectorCanvas` onto `self`, scaled by `opacity`.
    pub fn composite_canvas_with_opacity(&mut self, overlay: &VectorCanvas, opacity: f32) {
        if self.width != overlay.width
            || self.height != overlay.height
            || !opacity.is_finite()
            || opacity <= 0.0
        {
            return;
        }
        let op = opacity.clamp(0.0, 1.0);
        if (op - 1.0).abs() <= f32::EPSILON {
            for (dst, &src) in self.data.iter_mut().zip(overlay.data.iter()) {
                if src[3] > 0.0 {
                    *dst = source_over_premul(*dst, src);
                }
            }
        } else {
            for (dst, &src) in self.data.iter_mut().zip(overlay.data.iter()) {
                if src[3] > 0.0 {
                    *dst = source_over_premul(*dst, scale_premul(src, op));
                }
            }
        }
    }

    fn fill_contour_slices_with_rule(
        &mut self,
        contours: &[&[[f32; 2]]],
        src_premul: [f32; 4],
        even_odd: bool,
    ) {
        let Some((min_x, min_y, max_x, max_y)) = contour_slices_bounds(contours) else {
            return;
        };
        let Some((bx0, by0, bx1, by1)) =
            self.clipped_bounds(min_x - 1.0, min_y - 1.0, max_x + 1.0, max_y + 1.0)
        else {
            return;
        };

        let rw = (bx1 - bx0 + 1) as usize;
        let rh = (by1 - by0 + 1) as usize;
        let mut cov_buf = vec![0.0_f32; rw * rh];
        rasterize_contour_slices_coverage(
            contours,
            bx0,
            by0,
            bx1,
            by1,
            even_odd,
            rw,
            &mut cov_buf,
        );

        for row in 0..rh {
            let py = by0 + row as u32;
            let row_off = row * rw;
            for col in 0..rw {
                let cov = cov_buf[row_off + col];
                if cov > 0.0 {
                    self.blend_pixel_premul(bx0 + col as u32, py, src_premul, cov);
                }
            }
        }
    }

    /// Draws an alternating deterministic checkerboard pattern (useful for alpha inspection).
    pub fn draw_checkerboard(
        &mut self,
        tile_size: u32,
        color_a: [f32; 4],
        color_b: [f32; 4],
    ) {
        let tile = tile_size.max(1);
        let premul_a = premultiply_rgba(color_a);
        let premul_b = premultiply_rgba(color_b);
        for y in 0..self.height {
            let ty = y / tile;
            let row_start = (y as usize) * (self.width as usize);
            for x in 0..self.width {
                let tx = x / tile;
                let src = if (tx + ty) & 1 == 0 {
                    premul_a
                } else {
                    premul_b
                };
                let idx = row_start + (x as usize);
                self.data[idx] = source_over_premul(self.data[idx], src);
            }
        }
    }

    pub(crate) fn clipped_bounds(
        &self,
        min_x: f32,
        min_y: f32,
        max_x: f32,
        max_y: f32,
    ) -> Option<(u32, u32, u32, u32)> {
        if self.width == 0 || self.height == 0 {
            return None;
        }
        let max_w = (self.width - 1) as i64;
        let max_h = (self.height - 1) as i64;
        let x0 = (min_x.floor() as i64).clamp(0, max_w);
        let y0 = (min_y.floor() as i64).clamp(0, max_h);
        let x1 = (max_x.ceil() as i64).clamp(0, max_w);
        let y1 = (max_y.ceil() as i64).clamp(0, max_h);
        if x1 < x0 || y1 < y0 || max_x < 0.0 || max_y < 0.0 {
            return None;
        }
        Some((x0 as u32, y0 as u32, x1 as u32, y1 as u32))
    }
}

pub(crate) fn contour_slices_bounds(contours: &[&[[f32; 2]]]) -> Option<(f32, f32, f32, f32)> {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    let mut any = false;
    for contour in contours {
        for &[px, py] in *contour {
            any = true;
            min_x = min_x.min(px);
            min_y = min_y.min(py);
            max_x = max_x.max(px);
            max_y = max_y.max(py);
        }
    }
    if any {
        Some((min_x, min_y, max_x, max_y))
    } else {
        None
    }
}

/// Rasterizes multiple closed contour slices into a pre-allocated row-major `[f32]` coverage
/// buffer covering `[bx0..=bx1] x [by0..=by1]` using deterministic 4x4 SSAA and `max` accumulation.
#[allow(clippy::too_many_arguments)]
pub(crate) fn rasterize_contour_slices_coverage(
    contours: &[&[[f32; 2]]],
    bx0: u32,
    by0: u32,
    bx1: u32,
    by1: u32,
    even_odd: bool,
    out_stride: usize,
    out_max_cov: &mut [f32],
) {
    if bx1 < bx0 || by1 < by0 {
        return;
    }
    let row_len = (bx1 - bx0 + 1) as usize;
    let row_count = (by1 - by0 + 1) as usize;
    if out_stride < row_len || out_max_cov.len() < (row_count - 1) * out_stride + row_len {
        return;
    }

    let edge_cap: usize = contours.iter().map(|c| c.len()).sum();
    let mut row_hits = vec![0u8; row_len];
    let mut intersections: Vec<(f32, i32)> = Vec::with_capacity(edge_cap);

    for (row_idx, py) in (by0..=by1).enumerate() {
        row_hits.fill(0);
        for &sy_off in &SUBPIXEL_OFFSETS {
            let sy = py as f32 + sy_off;
            intersections.clear();
            for contour in contours {
                let n = contour.len();
                if n < 3 {
                    continue;
                }
                for i in 0..n {
                    let a = contour[i];
                    let b = contour[(i + 1) % n];
                    let (y0, y1) = (a[1], b[1]);
                    if (y0 <= sy && y1 > sy) || (y1 <= sy && y0 > sy) {
                        let t = (sy - y0) / (y1 - y0);
                        let ix = a[0] + t * (b[0] - a[0]);
                        let dir = if y1 > y0 { 1 } else { -1 };
                        intersections.push((ix, dir));
                    }
                }
            }
            if intersections.is_empty() {
                continue;
            }
            intersections.sort_by(|a, b| a.0.total_cmp(&b.0));

            let mut k = 0usize;
            let mut winding = 0i32;
            for (col_idx, px) in (bx0..=bx1).enumerate() {
                let px_f = px as f32;
                let mut hit_count = 0u8;
                for &sx_off in &SUBPIXEL_OFFSETS {
                    let sx = px_f + sx_off;
                    while k < intersections.len() && intersections[k].0 <= sx {
                        winding += intersections[k].1;
                        k += 1;
                    }
                    let inside = if even_odd {
                        (winding & 1) != 0
                    } else {
                        winding != 0
                    };
                    if inside {
                        hit_count += 1;
                    }
                }
                row_hits[col_idx] += hit_count;
            }
        }

        let row_off = row_idx * out_stride;
        for (col_idx, &hits) in row_hits.iter().enumerate() {
            if hits > 0 {
                let cov = hits as f32 / 16.0;
                let slot = &mut out_max_cov[row_off + col_idx];
                if cov > *slot {
                    *slot = cov;
                }
            }
        }
    }
}

/// Scales a premultiplied RGBA color by scalar `s`.
#[inline]
#[allow(dead_code)]
pub(crate) fn scale_premul(c: [f32; 4], s: f32) -> [f32; 4] {
    [c[0] * s, c[1] * s, c[2] * s, c[3] * s]
}

/// Smooth Hermite interpolation in `[0, 1]` between `edge0` and `edge1`.
#[inline]
#[allow(dead_code)]
pub(crate) fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    if (edge1 - edge0).abs() <= 1e-6 {
        return if x >= edge1 { 1.0 } else { 0.0 };
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Computes squared distance from point `p` to segment `ab` (faster than `sd_segment` inside tight loops).
#[inline]
#[allow(dead_code)]
pub(crate) fn sd_segment_sq(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let pax = p[0] - a[0];
    let pay = p[1] - a[1];
    let bax = b[0] - a[0];
    let bay = b[1] - a[1];
    let len_sq = bax * bax + bay * bay;
    let h = if len_sq > 1e-12 {
        ((pax * bax + pay * bay) / len_sq).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let dx = pax - bax * h;
    let dy = pay - bay * h;
    dx * dx + dy * dy
}

/// Converts a straight `[R, G, B, A]` color into premultiplied `[R*A, G*A, B*A, A]`.
#[inline]
pub fn premultiply_rgba(rgba: [f32; 4]) -> [f32; 4] {
    let a = sanitize_unit(rgba[3]);
    if a <= 0.0 {
        return [0.0, 0.0, 0.0, 0.0];
    }
    [
        sanitize_nonneg(rgba[0]) * a,
        sanitize_nonneg(rgba[1]) * a,
        sanitize_nonneg(rgba[2]) * a,
        a,
    ]
}

/// Converts a premultiplied `[R*A, G*A, B*A, A]` pixel back to straight `[R, G, B, A]`.
#[inline]
pub fn unpremultiply_rgba(premul: [f32; 4]) -> [f32; 4] {
    let a = sanitize_unit(premul[3]);
    if a <= 1e-6 {
        return [0.0, 0.0, 0.0, 0.0];
    }
    [
        sanitize_nonneg(premul[0] / a),
        sanitize_nonneg(premul[1] / a),
        sanitize_nonneg(premul[2] / a),
        a,
    ]
}

/// Converts an 8-bit straight sRGB `[u8; 4]` color into straight Linear Rec.709 `[f32; 4]`.
#[inline]
pub fn srgb_u8_to_linear_rgba(rgba: [u8; 4]) -> [f32; 4] {
    [
        srgb_to_linear(rgba[0] as f32 / 255.0),
        srgb_to_linear(rgba[1] as f32 / 255.0),
        srgb_to_linear(rgba[2] as f32 / 255.0),
        rgba[3] as f32 / 255.0,
    ]
}

/// Porter-Duff `SourceOver` compositing in premultiplied RGBA space.
#[inline]
pub fn source_over_premul(dst: [f32; 4], src: [f32; 4]) -> [f32; 4] {
    let inv_a = (1.0 - sanitize_unit(src[3])).max(0.0);
    [
        src[0] + dst[0] * inv_a,
        src[1] + dst[1] * inv_a,
        src[2] + dst[2] * inv_a,
        (src[3] + dst[3] * inv_a).clamp(0.0, 1.0),
    ]
}

/// Maps a signed distance (`<= 0` inside, `> 0` outside) to 1px anti-aliased coverage `[0, 1]`.
#[inline]
pub(crate) fn sdf_coverage(signed_distance: f32) -> f32 {
    if !signed_distance.is_finite() {
        return 0.0;
    }
    (0.5 - signed_distance).clamp(0.0, 1.0)
}

#[inline]
fn sd_rounded_box(px: f32, py: f32, half_w: f32, half_h: f32, radius: f32) -> f32 {
    let qx = px.abs() - (half_w - radius);
    let qy = py.abs() - (half_h - radius);
    let out_x = qx.max(0.0);
    let out_y = qy.max(0.0);
    let outside = (out_x * out_x + out_y * out_y).sqrt();
    let inside = qx.max(qy).min(0.0);
    outside + inside - radius
}

#[inline]
pub(crate) fn sd_segment(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let pax = p[0] - a[0];
    let pay = p[1] - a[1];
    let bax = b[0] - a[0];
    let bay = b[1] - a[1];
    let len_sq = bax * bax + bay * bay;
    let h = if len_sq > 1e-12 {
        ((pax * bax + pay * bay) / len_sq).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let dx = pax - bax * h;
    let dy = pay - bay * h;
    (dx * dx + dy * dy).sqrt()
}

fn flatten_cubic_bezier(
    p0: [f32; 2],
    p1: [f32; 2],
    p2: [f32; 2],
    p3: [f32; 2],
    tolerance: f32,
    depth: u8,
    out: &mut Vec<[f32; 2]>,
) {
    if depth >= MAX_BEZIER_DEPTH || cubic_flatness_error(p0, p1, p2, p3) <= tolerance {
        out.push(p3);
        return;
    }

    let p01 = midpoint(p0, p1);
    let p12 = midpoint(p1, p2);
    let p23 = midpoint(p2, p3);
    let p012 = midpoint(p01, p12);
    let p123 = midpoint(p12, p23);
    let p0123 = midpoint(p012, p123);

    flatten_cubic_bezier(p0, p01, p012, p0123, tolerance, depth + 1, out);
    flatten_cubic_bezier(p0123, p123, p23, p3, tolerance, depth + 1, out);
}

#[inline]
fn midpoint(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5]
}

#[inline]
fn cubic_flatness_error(p0: [f32; 2], p1: [f32; 2], p2: [f32; 2], p3: [f32; 2]) -> f32 {
    let ux = 3.0 * p1[0] - 2.0 * p0[0] - p3[0];
    let uy = 3.0 * p1[1] - 2.0 * p0[1] - p3[1];
    let vx = 3.0 * p2[0] - 2.0 * p3[0] - p0[0];
    let vy = 3.0 * p2[1] - 2.0 * p3[1] - p0[1];
    ((ux * ux).max(vx * vx) + (uy * uy).max(vy * vy)).sqrt() / 4.0
}

#[inline]
fn sanitize_unit(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

#[inline]
fn sanitize_nonneg(v: f32) -> f32 {
    if v.is_finite() {
        v.max(0.0)
    } else {
        0.0
    }
}

#[inline]
fn are_finite(vals: &[f32]) -> bool {
    vals.iter().all(|v| v.is_finite())
}

fn validated_pixel_count(width: u32, height: u32) -> Result<usize, VectorError> {
    if width == 0 || height == 0 {
        return Err(VectorError::InvalidDimensions { width, height });
    }
    (width as usize)
        .checked_mul(height as usize)
        .ok_or(VectorError::InvalidDimensions { width, height })
}
