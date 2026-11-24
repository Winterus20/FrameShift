//! GPU pyramidal Lucas-Kanade 2D Point & 4-Corner Planar Tracker.
//!
//! Reuses `fx_optical_flow_luma.wgsl` + `fx_optical_flow_step.wgsl` (`build_luma_pyramid`)
//! with bidirectional (Forward + Backward) consistency checking (`e_fb <= max_fb_error_px`),
//! `32x32` ROI `copy_texture_to_buffer` readback, keyframe template NCC drift protection,
//! and normalized DLT + Huber IRLS homography (`glam::Mat3`) estimation.

use crate::compositor::{GpuCompositor, NodeAnimator};
use crate::error::RenderError;
use crate::pool::PooledTexture;
use crate::provider::FrameProvider;
use crate::uniforms::OpticalFlowStepUniform;
use frameshift_schema::{
    AnimationEasing, Camera3DDef, KeyframeTrackDef, NodeId, RenderGraphDef, SchemaError,
    TrackKeyframe,
};
use frameshift_time::Flicks;
use glam::{Mat3, Vec3};
use half::f16;

/// Target node type when exporting tracker output to `KeyframeTrackDef`s.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackerBindTarget {
    /// Writes `anchor_x/y`, `translate_x/y`, and (when `point2` or planar) `scale_x/y`, `rotation_rad`.
    Transform2D,
    /// Camera-space inverse stabilization on `Transform2D`.
    Transform2DStabilize,
    /// Writes `pos_x`, `pos_y`, `rot_z`, `scale_x/y/z` on `Null3D` at `plane_z`.
    Null3D { plane_z_bits: u32 },
    /// Writes `tl_x/y`, `tr_x/y`, `br_x/y`, `bl_x/y` on `CornerPin`.
    CornerPin,
}

impl TrackerBindTarget {
    pub fn null3d(plane_z: f32) -> Self {
        Self::Null3D {
            plane_z_bits: plane_z.to_bits(),
        }
    }
}

/// Options for [`track_point_sequence`].
#[derive(Debug, Clone, PartialEq)]
pub struct TrackPointOptions {
    /// Optional secondary point `(u, v)` in `[0, 1]` for simultaneous translate + scale + rotation tracking.
    pub point2_uv: Option<[f32; 2]>,
    /// Patch radius in pixels for weighted flow aggregation (default `12`).
    pub patch_radius_px: u32,
    /// Maximum allowed forward-backward consistency error in pixels (default `1.5`).
    pub max_fb_error_px: f32,
    /// Minimum optical flow confidence `[0..1]` to accept a pixel sample (default `0.25`).
    pub min_confidence: f32,
    /// Optical flow smoothness `[0..1]` passed to `fx_optical_flow_step` (default `0.35`).
    pub flow_smoothness: f32,
    /// Optical flow scene-cut sensitivity (default `0.22`).
    pub sensitivity: f32,
    /// Maximum displacement in pixels at level 0 (default `48.0`).
    pub max_flow_px: f32,
    /// Minimum NCC threshold `[0..1]` to apply keyframe template drift correction (default `0.72`).
    pub ncc_min_score: f32,
    /// Re-anchor template interval in frames (`0` = keep initial frame template; default `15`).
    pub reanchor_interval: u32,
}

impl Default for TrackPointOptions {
    fn default() -> Self {
        Self {
            point2_uv: None,
            patch_radius_px: 12,
            max_fb_error_px: 1.5,
            min_confidence: 0.25,
            flow_smoothness: 0.35,
            sensitivity: 0.22,
            max_flow_px: 48.0,
            ncc_min_score: 0.72,
            reanchor_interval: 15,
        }
    }
}

/// Per-frame output sample from [`track_point_sequence`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointTrackSample {
    pub tick: Flicks,
    pub point_uv: [f32; 2],
    pub point2_uv: Option<[f32; 2]>,
    pub translate_uv: [f32; 2],
    pub scale: f32,
    pub rotation_rad: f32,
    pub fb_error_px: f32,
    pub confidence: f32,
    pub ncc_score: f32,
    pub lost: bool,
}

/// Complete output from [`track_point_sequence`].
#[derive(Debug, Clone, PartialEq)]
pub struct PointTrackResult {
    pub start_point_uv: [f32; 2],
    pub start_point2_uv: Option<[f32; 2]>,
    pub samples: Vec<PointTrackSample>,
    pub mean_fb_error_px: f32,
    pub min_confidence: f32,
    pub lost_ticks: Vec<Flicks>,
}

/// Options for [`track_planar_sequence`].
#[derive(Debug, Clone, PartialEq)]
pub struct TrackPlanarOptions {
    /// Grid sampling resolution inside the tracked quad (`N x N`, default `8`).
    pub grid_size: u32,
    /// Maximum allowed forward-backward consistency error in pixels (default `1.5`).
    pub max_fb_error_px: f32,
    /// Minimum optical flow confidence `[0..1]` to accept a sample (default `0.25`).
    pub min_confidence: f32,
    /// Optical flow smoothness `[0..1]` (default `0.35`).
    pub flow_smoothness: f32,
    /// Optical flow scene-cut sensitivity (default `0.22`).
    pub sensitivity: f32,
    /// Maximum displacement in pixels at level 0 (default `48.0`).
    pub max_flow_px: f32,
    /// Minimum NCC threshold for center patch drift correction (default `0.72`).
    pub ncc_min_score: f32,
    /// Re-anchor template interval in frames (default `15`).
    pub reanchor_interval: u32,
}

impl Default for TrackPlanarOptions {
    fn default() -> Self {
        Self {
            grid_size: 8,
            max_fb_error_px: 1.5,
            min_confidence: 0.25,
            flow_smoothness: 0.35,
            sensitivity: 0.22,
            max_flow_px: 48.0,
            ncc_min_score: 0.72,
            reanchor_interval: 15,
        }
    }
}

/// Per-frame output sample from [`track_planar_sequence`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlanarTrackSample {
    pub tick: Flicks,
    /// Tracked normalized corners `[tl, tr, br, bl]`.
    pub corners_uv: [[f32; 2]; 4],
    /// Cumulative homography `H_t` mapping initial `[0,1]` UV to current frame UV.
    pub homography: Mat3,
    pub center_uv: [f32; 2],
    pub scale: f32,
    pub rotation_rad: f32,
    pub inlier_count: usize,
    pub fb_error_px: f32,
    pub confidence: f32,
    pub lost: bool,
}

/// Complete output from [`track_planar_sequence`].
#[derive(Debug, Clone, PartialEq)]
pub struct PlanarTrackResult {
    pub start_corners_uv: [[f32; 2]; 4],
    pub samples: Vec<PlanarTrackSample>,
    pub mean_fb_error_px: f32,
    pub min_confidence: f32,
    pub lost_ticks: Vec<Flicks>,
}

struct RoiImage {
    origin_x: u32,
    origin_y: u32,
    width: u32,
    height: u32,
    pixels: Vec<[f32; 4]>,
}

impl RoiImage {
    fn sample_bilinear_px(&self, x_px: f32, y_px: f32) -> [f32; 4] {
        let lx = (x_px - self.origin_x as f32).clamp(0.0, (self.width.saturating_sub(1)) as f32);
        let ly = (y_px - self.origin_y as f32).clamp(0.0, (self.height.saturating_sub(1)) as f32);
        let x0 = lx.floor() as u32;
        let y0 = ly.floor() as u32;
        let x1 = (x0 + 1).min(self.width.saturating_sub(1));
        let y1 = (y0 + 1).min(self.height.saturating_sub(1));
        let fx = lx - x0 as f32;
        let fy = ly - y0 as f32;

        let p00 = self.pixels[(y0 * self.width + x0) as usize];
        let p10 = self.pixels[(y0 * self.width + x1) as usize];
        let p01 = self.pixels[(y1 * self.width + x0) as usize];
        let p11 = self.pixels[(y1 * self.width + x1) as usize];

        let mut out = [0.0f32; 4];
        for c in 0..4 {
            let top = p00[c] + (p10[c] - p00[c]) * fx;
            let bot = p01[c] + (p11[c] - p01[c]) * fx;
            out[c] = top + (bot - top) * fy;
        }
        out
    }
}

/// Reads back an arbitrary `[origin_x..origin_x+roi_w, origin_y..origin_y+roi_h]` window from an `Rgba16Float` texture.
#[allow(clippy::too_many_arguments)]
pub fn readback_rgba16f_roi(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    tex: &wgpu::Texture,
    tex_width: u32,
    tex_height: u32,
    origin_x: u32,
    origin_y: u32,
    roi_w: u32,
    roi_h: u32,
) -> Result<Vec<[f32; 4]>, RenderError> {
    if roi_w == 0 || roi_h == 0 || origin_x + roi_w > tex_width || origin_y + roi_h > tex_height {
        return Err(RenderError::InvalidDimensions {
            width: roi_w,
            height: roi_h,
        });
    }
    let unpadded = roi_w * 8;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded = unpadded.div_ceil(align) * align;
    let buffer_size = (padded as u64) * (roi_h as u64);

    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("frameshift-tracker-roi-readback"),
        size: buffer_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("frameshift-tracker-roi-copy"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: tex,
            mip_level: 0,
            origin: wgpu::Origin3d {
                x: origin_x,
                y: origin_y,
                z: 0,
            },
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &staging,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(roi_h),
            },
        },
        wgpu::Extent3d {
            width: roi_w,
            height: roi_h,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(std::iter::once(encoder.finish()));

    let slice = staging.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |res| {
        let _ = tx.send(res);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| RenderError::BufferMap(e.to_string()))?;
    rx.recv()
        .map_err(|e| RenderError::BufferMap(e.to_string()))?
        .map_err(|e| RenderError::BufferMap(e.to_string()))?;

    let mapped = slice
        .get_mapped_range()
        .map_err(|e| RenderError::BufferMap(e.to_string()))?;
    let mut pixels = Vec::with_capacity((roi_w * roi_h) as usize);
    for row in 0..roi_h as usize {
        let row_start = row * padded as usize;
        for col in 0..roi_w as usize {
            let px_off = row_start + col * 8;
            let r = f16::from_le_bytes([mapped[px_off], mapped[px_off + 1]]).to_f32();
            let g = f16::from_le_bytes([mapped[px_off + 2], mapped[px_off + 3]]).to_f32();
            let b = f16::from_le_bytes([mapped[px_off + 4], mapped[px_off + 5]]).to_f32();
            let a = f16::from_le_bytes([mapped[px_off + 6], mapped[px_off + 7]]).to_f32();
            pixels.push([r, g, b, a]);
        }
    }
    drop(mapped);
    staging.unmap();
    Ok(pixels)
}

fn compute_roi_bounds(
    center_px: [f32; 2],
    roi_size: u32,
    width: u32,
    height: u32,
) -> (u32, u32, u32, u32) {
    let rw = roi_size.min(width).max(1);
    let rh = roi_size.min(height).max(1);
    let half_w = (rw as f32) * 0.5;
    let half_h = (rh as f32) * 0.5;
    let ox = (center_px[0] - half_w)
        .round()
        .clamp(0.0, (width - rw) as f32) as u32;
    let oy = (center_px[1] - half_h)
        .round()
        .clamp(0.0, (height - rh) as f32) as u32;
    (ox, oy, rw, rh)
}

fn readback_roi_image(
    compositor: &GpuCompositor,
    tex: &PooledTexture,
    ox: u32,
    oy: u32,
    rw: u32,
    rh: u32,
) -> Result<RoiImage, RenderError> {
    let pixels = readback_rgba16f_roi(
        &compositor.context().device,
        &compositor.context().queue,
        &tex.texture,
        tex.width,
        tex.height,
        ox,
        oy,
        rw,
        rh,
    )?;
    Ok(RoiImage {
        origin_x: ox,
        origin_y: oy,
        width: rw,
        height: rh,
        pixels,
    })
}

#[allow(clippy::too_many_arguments)]
fn run_bidirectional_flow(
    compositor: &mut GpuCompositor,
    prev_pyr: &[PooledTexture],
    curr_pyr: &[PooledTexture],
    width: u32,
    height: u32,
    flow_smoothness: f32,
    sensitivity: f32,
    max_flow_px: f32,
) -> (PooledTexture, PooledTexture) {
    let fwd = run_single_flow(
        compositor,
        prev_pyr,
        curr_pyr,
        width,
        height,
        flow_smoothness,
        sensitivity,
        max_flow_px,
        "track-fwd",
    );
    let bwd = run_single_flow(
        compositor,
        curr_pyr,
        prev_pyr,
        width,
        height,
        flow_smoothness,
        sensitivity,
        max_flow_px,
        "track-bwd",
    );
    (fwd, bwd)
}

#[allow(clippy::too_many_arguments)]
fn run_single_flow(
    compositor: &mut GpuCompositor,
    prev_pyr: &[PooledTexture],
    curr_pyr: &[PooledTexture],
    width: u32,
    height: u32,
    flow_smoothness: f32,
    sensitivity: f32,
    max_flow_px: f32,
    label: &str,
) -> PooledTexture {
    let level_dims: [(u32, u32); 4] = [
        (width.max(1), height.max(1)),
        ((width / 2).max(1), (height / 2).max(1)),
        ((width / 4).max(1), (height / 4).max(1)),
        ((width / 8).max(1), (height / 8).max(1)),
    ];
    let max_flow_l0 = max_flow_px.max(8.0);
    let mut encoder =
        compositor
            .context()
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some(label),
            });
    let mut flow_tex: Option<PooledTexture> = None;
    for level_idx in (0..4usize).rev() {
        let (lw, lh) = level_dims[level_idx];
        let level_scale = (1u32 << level_idx) as f32;
        let level_max_flow = (max_flow_l0 / level_scale).max(4.0);
        for iter in 0..2 {
            let next_flow = compositor.acquire_pooled_texture(lw, lh);
            let has_coarse = flow_tex.is_some();
            let coarse_view = flow_tex
                .as_ref()
                .map(|t| &t.view)
                .unwrap_or_else(|| compositor.dummy_zero_view());
            let uniform = OpticalFlowStepUniform::new(
                lw,
                lh,
                flow_smoothness,
                sensitivity,
                level_max_flow,
                4.0,
                has_coarse,
            );
            compositor.record_optical_flow_step_pass(
                &mut encoder,
                &format!("{label}-l{level_idx}-i{iter}"),
                &prev_pyr[level_idx].view,
                &curr_pyr[level_idx].view,
                coarse_view,
                &uniform,
                &next_flow.view,
            );
            if let Some(old) = flow_tex.replace(next_flow) {
                compositor.release_pooled_texture(old);
            }
        }
    }
    compositor
        .context()
        .queue
        .submit(std::iter::once(encoder.finish()));
    flow_tex.expect("4-level optical flow produces level 0")
}

fn extract_luma_patch(luma: &RoiImage, center_px: [f32; 2], radius: i32) -> Vec<f32> {
    let side = (radius * 2 + 1) as usize;
    let mut patch = Vec::with_capacity(side * side);
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            let px = luma.sample_bilinear_px(center_px[0] + dx as f32, center_px[1] + dy as f32);
            patch.push(px[0]);
        }
    }
    patch
}

fn compute_ncc(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let n = a.len() as f32;
    let mean_a = a.iter().sum::<f32>() / n;
    let mean_b = b.iter().sum::<f32>() / n;
    let mut num = 0.0f32;
    let mut den_a = 0.0f32;
    let mut den_b = 0.0f32;
    for (&va, &vb) in a.iter().zip(b.iter()) {
        let da = va - mean_a;
        let db = vb - mean_b;
        num += da * db;
        den_a += da * da;
        den_b += db * db;
    }
    let denom = (den_a * den_b).sqrt();
    if denom <= 1e-6 {
        return 0.0;
    }
    (num / denom).clamp(-1.0, 1.0)
}

fn refine_point_ncc(
    template: &[f32],
    curr_luma: &RoiImage,
    est_px: [f32; 2],
    radius: i32,
    min_score: f32,
) -> ([f32; 2], f32) {
    let mut scores = [[0.0f32; 3]; 3];
    let mut best_dx = 0i32;
    let mut best_dy = 0i32;
    let mut best_score = -1.0f32;

    for (iy, dy) in (-1..=1).enumerate() {
        for (ix, dx) in (-1..=1).enumerate() {
            let patch =
                extract_luma_patch(curr_luma, [est_px[0] + dx as f32, est_px[1] + dy as f32], radius);
            let s = compute_ncc(template, &patch);
            scores[iy][ix] = s;
            if s > best_score {
                best_score = s;
                best_dx = dx;
                best_dy = dy;
            }
        }
    }

    if best_score < min_score {
        return (est_px, scores[1][1]);
    }

    // Sub-pixel parabolic refinement around center when peak is interior.
    let sub_x = if best_dx == 0 {
        let denom = 2.0 * (scores[1][0] - 2.0 * scores[1][1] + scores[1][2]);
        if denom.abs() > 1e-5 {
            ((scores[1][0] - scores[1][2]) / denom).clamp(-0.5, 0.5)
        } else {
            0.0
        }
    } else {
        0.0
    };
    let sub_y = if best_dy == 0 {
        let denom = 2.0 * (scores[0][1] - 2.0 * scores[1][1] + scores[2][1]);
        if denom.abs() > 1e-5 {
            ((scores[0][1] - scores[2][1]) / denom).clamp(-0.5, 0.5)
        } else {
            0.0
        }
    } else {
        0.0
    };

    (
        [
            est_px[0] + best_dx as f32 + sub_x,
            est_px[1] + best_dy as f32 + sub_y,
        ],
        best_score,
    )
}

#[allow(clippy::too_many_arguments)]
fn track_single_point_step(
    prev_px: [f32; 2],
    flow_fwd_roi: &RoiImage,
    flow_bwd_roi: &RoiImage,
    curr_luma_roi: &RoiImage,
    template: &[f32],
    template_radius: i32,
    width: u32,
    height: u32,
    opts: &TrackPointOptions,
) -> ([f32; 2], f32, f32, f32, bool) {
    let w_f = width.max(1) as f32;
    let h_f = height.max(1) as f32;
    let rad = opts.patch_radius_px.clamp(2, 14) as i32;
    let sigma2 = ((rad as f32) * 0.5).powi(2).max(1.0);

    let mut sum_dx = 0.0f32;
    let mut sum_dy = 0.0f32;
    let mut sum_w = 0.0f32;
    let mut sum_fb = 0.0f32;
    let mut sum_conf = 0.0f32;

    for oy in -rad..=rad {
        for ox in -rad..=rad {
            let sx = prev_px[0] + ox as f32;
            let sy = prev_px[1] + oy as f32;
            if sx < 0.0 || sx >= w_f || sy < 0.0 || sy >= h_f {
                continue;
            }
            // flow_bwd = flow(curr -> prev) evaluated at prev: d = -flow_bwd(prev).
            let b_sample = flow_bwd_roi.sample_bilinear_px(sx, sy);
            let conf_b = b_sample[2];
            if conf_b < opts.min_confidence {
                continue;
            }
            let d_px = [-b_sample[0] * w_f, -b_sample[1] * h_f];
            let target_x = sx + d_px[0];
            let target_y = sy + d_px[1];
            if target_x < flow_fwd_roi.origin_x as f32
                || target_x >= (flow_fwd_roi.origin_x + flow_fwd_roi.width) as f32
                || target_y < flow_fwd_roi.origin_y as f32
                || target_y >= (flow_fwd_roi.origin_y + flow_fwd_roi.height) as f32
            {
                continue;
            }
            let f_sample = flow_fwd_roi.sample_bilinear_px(target_x, target_y);
            let conf_f = f_sample[2];
            if conf_f < opts.min_confidence {
                continue;
            }
            let f_px = [f_sample[0] * w_f, f_sample[1] * h_f];
            let fb_err = ((d_px[0] - f_px[0]).powi(2) + (d_px[1] - f_px[1]).powi(2)).sqrt();
            if fb_err > opts.max_fb_error_px {
                continue;
            }

            let dist2 = (ox * ox + oy * oy) as f32;
            let gauss = (-0.5 * dist2 / sigma2).exp();
            let fb_gate = 1.0 - (fb_err / opts.max_fb_error_px.max(1e-4)).clamp(0.0, 0.99);
            let w = gauss * conf_b * conf_f * fb_gate;
            sum_dx += d_px[0] * w;
            sum_dy += d_px[1] * w;
            sum_fb += fb_err * w;
            sum_conf += conf_b.min(conf_f) * w;
            sum_w += w;
        }
    }

    if sum_w <= 1e-5 {
        return (prev_px, opts.max_fb_error_px * 2.0, 0.0, 0.0, true);
    }

    let est = [
        (prev_px[0] + sum_dx / sum_w).clamp(0.0, w_f - 1.0),
        (prev_px[1] + sum_dy / sum_w).clamp(0.0, h_f - 1.0),
    ];
    let avg_fb = sum_fb / sum_w;
    let avg_conf = sum_conf / sum_w;

    let (refined, ncc) = refine_point_ncc(
        template,
        curr_luma_roi,
        est,
        template_radius,
        opts.ncc_min_score,
    );
    let final_px = [
        refined[0].clamp(0.0, w_f - 1.0),
        refined[1].clamp(0.0, h_f - 1.0),
    ];
    (final_px, avg_fb, avg_conf, ncc, false)
}

/// Tracks 1 or 2 points across `ticks` on `node_id` using 4-level pyramidal optical flow,
/// forward-backward consistency, `32x32` ROI readback, and NCC drift protection.
#[allow(clippy::too_many_arguments)]
pub fn track_point_sequence(
    compositor: &mut GpuCompositor,
    graph: &RenderGraphDef,
    node_id: &NodeId,
    ticks: &[Flicks],
    frame_ticks: Flicks,
    width: u32,
    height: u32,
    provider: &dyn FrameProvider,
    animator: &dyn NodeAnimator,
    start_point_uv: [f32; 2],
    opts: &TrackPointOptions,
) -> Result<PointTrackResult, RenderError> {
    if width < 2 || height < 2 {
        return Err(RenderError::InvalidDimensions { width, height });
    }
    if ticks.is_empty() {
        return Ok(PointTrackResult {
            start_point_uv,
            start_point2_uv: opts.point2_uv,
            samples: Vec::new(),
            mean_fb_error_px: 0.0,
            min_confidence: 1.0,
            lost_ticks: Vec::new(),
        });
    }
    if graph.node(node_id).is_none() {
        return Err(RenderError::Schema(SchemaError::MissingNode(
            node_id.clone(),
        )));
    }
    let full_topo = graph.validate_graph()?;
    let w_f = width as f32;
    let h_f = height as f32;

    let mut cur_p1_px = [
        (start_point_uv[0] * w_f).clamp(0.0, w_f - 1.0),
        (start_point_uv[1] * h_f).clamp(0.0, h_f - 1.0),
    ];
    let mut cur_p2_px = opts.point2_uv.map(|p2| {
        [
            (p2[0] * w_f).clamp(0.0, w_f - 1.0),
            (p2[1] * h_f).clamp(0.0, h_f - 1.0),
        ]
    });
    let base_dist = opts.point2_uv.map(|p2| {
        let dx = (p2[0] - start_point_uv[0]) * w_f;
        let dy = (p2[1] - start_point_uv[1]) * h_f;
        (dx * dx + dy * dy).sqrt().max(1e-4)
    });
    let base_angle = opts
        .point2_uv
        .map(|p2| ((p2[1] - start_point_uv[1]) * h_f).atan2((p2[0] - start_point_uv[0]) * w_f));

    let template_radius = 6i32;

    let tex0 = compositor.evaluate_subgraph_to_pooled(
        graph,
        &full_topo,
        node_id,
        ticks[0],
        frame_ticks,
        width,
        height,
        provider,
        animator,
    )?;
    let mut prev_pyr = compositor.build_luma_pyramid_standalone("track-p0", &tex0, width, height);
    compositor.release_pooled_texture(tex0);

    let (ox0, oy0, rw0, rh0) = compute_roi_bounds(cur_p1_px, 32, width, height);
    let luma0 = readback_roi_image(compositor, &prev_pyr[0], ox0, oy0, rw0, rh0)?;
    let mut template1 = extract_luma_patch(&luma0, cur_p1_px, template_radius);
    let mut template2 = cur_p2_px
        .map(|p2| {
            let (ox, oy, rw, rh) = compute_roi_bounds(p2, 32, width, height);
            let l2 = readback_roi_image(compositor, &prev_pyr[0], ox, oy, rw, rh)?;
            Ok::<_, RenderError>(extract_luma_patch(&l2, p2, template_radius))
        })
        .transpose()?;

    let mut samples = Vec::with_capacity(ticks.len());
    samples.push(PointTrackSample {
        tick: ticks[0],
        point_uv: start_point_uv,
        point2_uv: opts.point2_uv,
        translate_uv: [0.0, 0.0],
        scale: 1.0,
        rotation_rad: 0.0,
        fb_error_px: 0.0,
        confidence: 1.0,
        ncc_score: 1.0,
        lost: false,
    });

    let mut fb_sum = 0.0f32;
    let mut fb_count = 0usize;
    let mut min_conf = 1.0f32;
    let mut lost_ticks = Vec::new();

    for (frame_idx, &tick) in ticks.iter().enumerate().skip(1) {
        let curr_tex = compositor.evaluate_subgraph_to_pooled(
            graph,
            &full_topo,
            node_id,
            tick,
            frame_ticks,
            width,
            height,
            provider,
            animator,
        )?;
        let curr_pyr = compositor.build_luma_pyramid_standalone(
            &format!("track-f{frame_idx}"),
            &curr_tex,
            width,
            height,
        );
        compositor.release_pooled_texture(curr_tex);

        let (fwd_flow, bwd_flow) = run_bidirectional_flow(
            compositor,
            &prev_pyr,
            &curr_pyr,
            width,
            height,
            opts.flow_smoothness,
            opts.sensitivity,
            opts.max_flow_px,
        );

        let (ox1, oy1, rw1, rh1) = compute_roi_bounds(cur_p1_px, 32, width, height);
        let bwd1 = readback_roi_image(compositor, &bwd_flow, ox1, oy1, rw1, rh1)?;
        let raw_b1 = bwd1.sample_bilinear_px(cur_p1_px[0], cur_p1_px[1]);
        let est1 = [
            (cur_p1_px[0] - raw_b1[0] * w_f).clamp(0.0, w_f - 1.0),
            (cur_p1_px[1] - raw_b1[1] * h_f).clamp(0.0, h_f - 1.0),
        ];
        let (fox1, foy1, frw1, frh1) = compute_roi_bounds(est1, 32, width, height);
        let fwd1 = readback_roi_image(compositor, &fwd_flow, fox1, foy1, frw1, frh1)?;
        let luma1 = readback_roi_image(compositor, &curr_pyr[0], fox1, foy1, frw1, frh1)?;

        let (next_p1_px, fb1, conf1, ncc1, lost1) = track_single_point_step(
            cur_p1_px,
            &fwd1,
            &bwd1,
            &luma1,
            &template1,
            template_radius,
            width,
            height,
            opts,
        );
        cur_p1_px = next_p1_px;

        let mut next_p2_uv = None;
        let mut combined_fb = fb1;
        let mut combined_conf = conf1;
        let mut lost = lost1;
        let mut scale = 1.0f32;
        let mut rotation_rad = 0.0f32;

        if let (Some(p2_px), Some(t2)) = (cur_p2_px, template2.as_ref()) {
            let (ox2, oy2, rw2, rh2) = compute_roi_bounds(p2_px, 32, width, height);
            let bwd2 = readback_roi_image(compositor, &bwd_flow, ox2, oy2, rw2, rh2)?;
            let raw_b2 = bwd2.sample_bilinear_px(p2_px[0], p2_px[1]);
            let est2 = [
                (p2_px[0] - raw_b2[0] * w_f).clamp(0.0, w_f - 1.0),
                (p2_px[1] - raw_b2[1] * h_f).clamp(0.0, h_f - 1.0),
            ];
            let (fox2, foy2, frw2, frh2) = compute_roi_bounds(est2, 32, width, height);
            let fwd2 = readback_roi_image(compositor, &fwd_flow, fox2, foy2, frw2, frh2)?;
            let luma2 = readback_roi_image(compositor, &curr_pyr[0], fox2, foy2, frw2, frh2)?;

            let (next_p2_px, fb2, conf2, _ncc2, lost2) = track_single_point_step(
                p2_px,
                &fwd2,
                &bwd2,
                &luma2,
                t2,
                template_radius,
                width,
                height,
                opts,
            );
            cur_p2_px = Some(next_p2_px);
            let p2_uv = [next_p2_px[0] / w_f, next_p2_px[1] / h_f];
            next_p2_uv = Some(p2_uv);
            combined_fb = (fb1 + fb2) * 0.5;
            combined_conf = conf1.min(conf2);
            lost = lost1 || lost2;

            if let (Some(d0), Some(a0)) = (base_dist, base_angle) {
                let dx = next_p2_px[0] - cur_p1_px[0];
                let dy = next_p2_px[1] - cur_p1_px[1];
                let dist = (dx * dx + dy * dy).sqrt();
                scale = (dist / d0).clamp(0.05, 20.0);
                rotation_rad = wrap_angle_rad(dy.atan2(dx) - a0);
            }

            if opts.reanchor_interval > 0
                && frame_idx % (opts.reanchor_interval as usize) == 0
                && !lost2
                && fb2 < 0.4
            {
                template2 = Some(extract_luma_patch(&luma2, next_p2_px, template_radius));
            }
        }

        if opts.reanchor_interval > 0
            && frame_idx % (opts.reanchor_interval as usize) == 0
            && !lost1
            && fb1 < 0.4
        {
            template1 = extract_luma_patch(&luma1, cur_p1_px, template_radius);
        }

        compositor.release_pooled_texture(fwd_flow);
        compositor.release_pooled_texture(bwd_flow);
        for tex in prev_pyr {
            compositor.release_pooled_texture(tex);
        }
        prev_pyr = curr_pyr;

        let p1_uv = [cur_p1_px[0] / w_f, cur_p1_px[1] / h_f];
        let translate_uv = if let (Some(p2_uv), Some(s2)) = (next_p2_uv, opts.point2_uv) {
            let mid_cur = [(p1_uv[0] + p2_uv[0]) * 0.5, (p1_uv[1] + p2_uv[1]) * 0.5];
            let mid_0 = [
                (start_point_uv[0] + s2[0]) * 0.5,
                (start_point_uv[1] + s2[1]) * 0.5,
            ];
            [mid_cur[0] - mid_0[0], mid_cur[1] - mid_0[1]]
        } else {
            [p1_uv[0] - start_point_uv[0], p1_uv[1] - start_point_uv[1]]
        };

        fb_sum += combined_fb;
        fb_count += 1;
        min_conf = min_conf.min(combined_conf);
        if lost {
            lost_ticks.push(tick);
        }

        samples.push(PointTrackSample {
            tick,
            point_uv: p1_uv,
            point2_uv: next_p2_uv,
            translate_uv,
            scale,
            rotation_rad,
            fb_error_px: combined_fb,
            confidence: combined_conf,
            ncc_score: ncc1,
            lost,
        });
    }

    for tex in prev_pyr {
        compositor.release_pooled_texture(tex);
    }

    Ok(PointTrackResult {
        start_point_uv,
        start_point2_uv: opts.point2_uv,
        samples,
        mean_fb_error_px: if fb_count > 0 {
            fb_sum / fb_count as f32
        } else {
            0.0
        },
        min_confidence: min_conf,
        lost_ticks,
    })
}

/// Tracks a 4-corner quad `[tl, tr, br, bl]` in normalized `[0, 1]` UV coordinates across `ticks`
/// using bidirectional optical flow, weighted normalized DLT + Huber IRLS homography, and affine fallback.
#[allow(clippy::too_many_arguments)]
pub fn track_planar_sequence(
    compositor: &mut GpuCompositor,
    graph: &RenderGraphDef,
    node_id: &NodeId,
    ticks: &[Flicks],
    frame_ticks: Flicks,
    width: u32,
    height: u32,
    provider: &dyn FrameProvider,
    animator: &dyn NodeAnimator,
    start_corners_uv: [[f32; 2]; 4],
    opts: &TrackPlanarOptions,
) -> Result<PlanarTrackResult, RenderError> {
    if width < 2 || height < 2 {
        return Err(RenderError::InvalidDimensions { width, height });
    }
    if ticks.is_empty() {
        return Ok(PlanarTrackResult {
            start_corners_uv,
            samples: Vec::new(),
            mean_fb_error_px: 0.0,
            min_confidence: 1.0,
            lost_ticks: Vec::new(),
        });
    }
    if graph.node(node_id).is_none() {
        return Err(RenderError::Schema(SchemaError::MissingNode(
            node_id.clone(),
        )));
    }
    let full_topo = graph.validate_graph()?;
    let w_f = width as f32;
    let h_f = height as f32;

    let tex0 = compositor.evaluate_subgraph_to_pooled(
        graph,
        &full_topo,
        node_id,
        ticks[0],
        frame_ticks,
        width,
        height,
        provider,
        animator,
    )?;
    let mut prev_pyr = compositor.build_luma_pyramid_standalone("planar-p0", &tex0, width, height);
    compositor.release_pooled_texture(tex0);

    let mut cum_h = Mat3::IDENTITY;
    let mut cur_corners = start_corners_uv;
    let center0 = quad_center(start_corners_uv);
    let top_vec0 = [
        (start_corners_uv[1][0] - start_corners_uv[0][0]) * w_f,
        (start_corners_uv[1][1] - start_corners_uv[0][1]) * h_f,
    ];
    let base_edge_len = (top_vec0[0] * top_vec0[0] + top_vec0[1] * top_vec0[1])
        .sqrt()
        .max(1e-4);
    let base_angle = top_vec0[1].atan2(top_vec0[0]);

    let template_radius = 6i32;
    let c0_px = [center0[0] * w_f, center0[1] * h_f];
    let (ox0, oy0, rw0, rh0) = compute_roi_bounds(c0_px, 32, width, height);
    let luma0 = readback_roi_image(compositor, &prev_pyr[0], ox0, oy0, rw0, rh0)?;
    let mut center_template = extract_luma_patch(&luma0, c0_px, template_radius);

    let mut samples = Vec::with_capacity(ticks.len());
    samples.push(PlanarTrackSample {
        tick: ticks[0],
        corners_uv: start_corners_uv,
        homography: Mat3::IDENTITY,
        center_uv: center0,
        scale: 1.0,
        rotation_rad: 0.0,
        inlier_count: (opts.grid_size.clamp(4, 16) as usize).pow(2),
        fb_error_px: 0.0,
        confidence: 1.0,
        lost: false,
    });

    let mut fb_sum = 0.0f32;
    let mut fb_count = 0usize;
    let mut min_conf = 1.0f32;
    let mut lost_ticks = Vec::new();
    let grid_n = opts.grid_size.clamp(4, 16);

    for (frame_idx, &tick) in ticks.iter().enumerate().skip(1) {
        let curr_tex = compositor.evaluate_subgraph_to_pooled(
            graph,
            &full_topo,
            node_id,
            tick,
            frame_ticks,
            width,
            height,
            provider,
            animator,
        )?;
        let curr_pyr = compositor.build_luma_pyramid_standalone(
            &format!("planar-f{frame_idx}"),
            &curr_tex,
            width,
            height,
        );
        compositor.release_pooled_texture(curr_tex);

        let (fwd_flow, bwd_flow) = run_bidirectional_flow(
            compositor,
            &prev_pyr,
            &curr_pyr,
            width,
            height,
            opts.flow_smoothness,
            opts.sensitivity,
            opts.max_flow_px,
        );

        let (bx, by, bw, bh) = quad_roi_bounds(cur_corners, 24.0, width, height);
        let bwd_roi = readback_roi_image(compositor, &bwd_flow, bx, by, bw, bh)?;
        let fwd_roi = readback_roi_image(compositor, &fwd_flow, bx, by, bw, bh)?;

        let mut corr_src: Vec<[f64; 2]> = Vec::new();
        let mut corr_dst: Vec<[f64; 2]> = Vec::new();
        let mut corr_w: Vec<f64> = Vec::new();
        let mut local_fb_sum = 0.0f32;
        let mut local_conf_sum = 0.0f32;

        for gy in 0..grid_n {
            let v = (gy as f32 + 0.5) / (grid_n as f32);
            for gx in 0..grid_n {
                let u = (gx as f32 + 0.5) / (grid_n as f32);
                let q_uv = bilinear_quad(cur_corners, u, v);
                let q_px = [q_uv[0] * w_f, q_uv[1] * h_f];
                if q_px[0] < bx as f32
                    || q_px[0] >= (bx + bw) as f32
                    || q_px[1] < by as f32
                    || q_px[1] >= (by + bh) as f32
                {
                    continue;
                }
                let b_s = bwd_roi.sample_bilinear_px(q_px[0], q_px[1]);
                let conf_b = b_s[2];
                if conf_b < opts.min_confidence {
                    continue;
                }
                let d_px = [-b_s[0] * w_f, -b_s[1] * h_f];
                let dst_px = [q_px[0] + d_px[0], q_px[1] + d_px[1]];
                if dst_px[0] < bx as f32
                    || dst_px[0] >= (bx + bw) as f32
                    || dst_px[1] < by as f32
                    || dst_px[1] >= (by + bh) as f32
                {
                    continue;
                }
                let f_s = fwd_roi.sample_bilinear_px(dst_px[0], dst_px[1]);
                let conf_f = f_s[2];
                if conf_f < opts.min_confidence {
                    continue;
                }
                let f_px = [f_s[0] * w_f, f_s[1] * h_f];
                let fb_err = ((d_px[0] - f_px[0]).powi(2) + (d_px[1] - f_px[1]).powi(2)).sqrt();
                if fb_err > opts.max_fb_error_px {
                    continue;
                }
                let w = (conf_b * conf_f) as f64
                    * (1.0 - (fb_err / opts.max_fb_error_px.max(1e-4)) as f64).clamp(0.05, 1.0);
                corr_src.push([q_uv[0] as f64, q_uv[1] as f64]);
                corr_dst.push([(dst_px[0] / w_f) as f64, (dst_px[1] / h_f) as f64]);
                corr_w.push(w);
                local_fb_sum += fb_err;
                local_conf_sum += conf_b.min(conf_f);
            }
        }

        let inlier_count = corr_src.len();
        let (delta_h, lost, avg_fb, avg_conf) = if inlier_count >= 6 {
            let dh = solve_weighted_homography_irls(&corr_src, &corr_dst, &corr_w)
                .or_else(|| solve_weighted_affine(&corr_src, &corr_dst, &corr_w))
                .unwrap_or(Mat3::IDENTITY);
            (
                dh,
                false,
                local_fb_sum / inlier_count as f32,
                local_conf_sum / inlier_count as f32,
            )
        } else if inlier_count >= 3 {
            let dh = solve_weighted_affine(&corr_src, &corr_dst, &corr_w).unwrap_or(Mat3::IDENTITY);
            (
                dh,
                false,
                local_fb_sum / inlier_count as f32,
                local_conf_sum / inlier_count as f32,
            )
        } else {
            (Mat3::IDENTITY, true, opts.max_fb_error_px * 2.0, 0.0)
        };

        if !lost {
            cum_h = delta_h * cum_h;
            let z_norm = cum_h.z_axis.z;
            if z_norm.abs() > 1e-6 {
                cum_h /= z_norm;
            }
            for i in 0..4 {
                cur_corners[i] = project_uv(cum_h, start_corners_uv[i]);
            }

            // Keyframe center-patch NCC drift refinement.
            let c_uv = quad_center(cur_corners);
            let c_px = [
                (c_uv[0] * w_f).clamp(0.0, w_f - 1.0),
                (c_uv[1] * h_f).clamp(0.0, h_f - 1.0),
            ];
            let (cox, coy, crw, crh) = compute_roi_bounds(c_px, 32, width, height);
            let curr_luma_roi = readback_roi_image(compositor, &curr_pyr[0], cox, coy, crw, crh)?;
            let (refined_c_px, ncc_score) = refine_point_ncc(
                &center_template,
                &curr_luma_roi,
                c_px,
                template_radius,
                opts.ncc_min_score,
            );
            if ncc_score >= opts.ncc_min_score {
                let corr_dx = (refined_c_px[0] - c_px[0]) / w_f;
                let corr_dy = (refined_c_px[1] - c_px[1]) / h_f;
                if corr_dx.abs() * w_f <= 1.25 && corr_dy.abs() * h_f <= 1.25 {
                    let shift = Mat3::from_cols(
                        Vec3::new(1.0, 0.0, 0.0),
                        Vec3::new(0.0, 1.0, 0.0),
                        Vec3::new(corr_dx, corr_dy, 1.0),
                    );
                    cum_h = shift * cum_h;
                    for i in 0..4 {
                        cur_corners[i] = project_uv(cum_h, start_corners_uv[i]);
                    }
                }
            }
            if opts.reanchor_interval > 0
                && frame_idx % (opts.reanchor_interval as usize) == 0
                && avg_fb < 0.4
            {
                let nc = quad_center(cur_corners);
                let nc_px = [
                    (nc[0] * w_f).clamp(0.0, w_f - 1.0),
                    (nc[1] * h_f).clamp(0.0, h_f - 1.0),
                ];
                center_template = extract_luma_patch(&curr_luma_roi, nc_px, template_radius);
            }
        } else {
            lost_ticks.push(tick);
        }

        compositor.release_pooled_texture(fwd_flow);
        compositor.release_pooled_texture(bwd_flow);
        for tex in prev_pyr {
            compositor.release_pooled_texture(tex);
        }
        prev_pyr = curr_pyr;

        let center_uv = quad_center(cur_corners);
        let top_vec = [
            (cur_corners[1][0] - cur_corners[0][0]) * w_f,
            (cur_corners[1][1] - cur_corners[0][1]) * h_f,
        ];
        let edge_len = (top_vec[0] * top_vec[0] + top_vec[1] * top_vec[1]).sqrt();
        let scale = (edge_len / base_edge_len).clamp(0.05, 20.0);
        let rotation_rad = wrap_angle_rad(top_vec[1].atan2(top_vec[0]) - base_angle);

        fb_sum += avg_fb;
        fb_count += 1;
        min_conf = min_conf.min(avg_conf);

        samples.push(PlanarTrackSample {
            tick,
            corners_uv: cur_corners,
            homography: cum_h,
            center_uv,
            scale,
            rotation_rad,
            inlier_count,
            fb_error_px: avg_fb,
            confidence: avg_conf,
            lost,
        });
    }

    for tex in prev_pyr {
        compositor.release_pooled_texture(tex);
    }

    Ok(PlanarTrackResult {
        start_corners_uv,
        samples,
        mean_fb_error_px: if fb_count > 0 {
            fb_sum / fb_count as f32
        } else {
            0.0
        },
        min_confidence: min_conf,
        lost_ticks,
    })
}

/// Converts a [`PointTrackResult`] into dense [`KeyframeTrackDef`]s targeting `Transform2D` or `Null3D`.
pub fn point_track_to_keyframes(
    track_id_prefix: &str,
    target_node: &NodeId,
    target_kind: TrackerBindTarget,
    result: &PointTrackResult,
    width: u32,
    height: u32,
    camera: Option<&Camera3DDef>,
) -> Vec<KeyframeTrackDef> {
    let aspect = (width as f32 / height.max(1) as f32).max(1e-4);
    match target_kind {
        TrackerBindTarget::Transform2D | TrackerBindTarget::Transform2DStabilize => {
            let stabilize = matches!(target_kind, TrackerBindTarget::Transform2DStabilize);
            let sign = if stabilize { -1.0 } else { 1.0 };
            let mut tx = Vec::with_capacity(result.samples.len());
            let mut ty = Vec::with_capacity(result.samples.len());
            let mut sx = Vec::with_capacity(result.samples.len());
            let mut sy = Vec::with_capacity(result.samples.len());
            let mut rot = Vec::with_capacity(result.samples.len());

            for s in &result.samples {
                tx.push(TrackKeyframe {
                    tick: s.tick,
                    value: f64::from(s.translate_uv[0] * sign),
                    easing: AnimationEasing::Linear,
                });
                ty.push(TrackKeyframe {
                    tick: s.tick,
                    value: f64::from(s.translate_uv[1] * sign),
                    easing: AnimationEasing::Linear,
                });
                let sc = if stabilize {
                    1.0 / s.scale.max(1e-4)
                } else {
                    s.scale
                };
                sx.push(TrackKeyframe {
                    tick: s.tick,
                    value: f64::from(sc),
                    easing: AnimationEasing::Linear,
                });
                sy.push(TrackKeyframe {
                    tick: s.tick,
                    value: f64::from(sc),
                    easing: AnimationEasing::Linear,
                });
                rot.push(TrackKeyframe {
                    tick: s.tick,
                    value: f64::from(s.rotation_rad * sign),
                    easing: AnimationEasing::Linear,
                });
            }

            let mut out = vec![
                make_track(track_id_prefix, target_node, "translate_x", tx),
                make_track(track_id_prefix, target_node, "translate_y", ty),
            ];
            if result.start_point2_uv.is_some() {
                out.push(make_track(track_id_prefix, target_node, "scale_x", sx));
                out.push(make_track(track_id_prefix, target_node, "scale_y", sy));
                out.push(make_track(track_id_prefix, target_node, "rotation_rad", rot));
            }
            out
        }
        TrackerBindTarget::Null3D { plane_z_bits } => {
            let plane_z = f32::from_bits(plane_z_bits);
            let default_cam = Camera3DDef::default();
            let cam = camera.unwrap_or(&default_cam);
            let dist = (cam.pos[2] - plane_z).abs().max(0.05);
            let half_h = dist * (cam.fov_deg.clamp(5.0, 175.0).to_radians() * 0.5).tan();
            let half_w = half_h * aspect;

            let mut px = Vec::with_capacity(result.samples.len());
            let mut py = Vec::with_capacity(result.samples.len());
            let mut rz = Vec::with_capacity(result.samples.len());
            let mut sx = Vec::with_capacity(result.samples.len());
            let mut sy = Vec::with_capacity(result.samples.len());
            let mut sz = Vec::with_capacity(result.samples.len());

            for s in &result.samples {
                let pos_x = (2.0 * s.point_uv[0] - 1.0) * half_w;
                let pos_y = (1.0 - 2.0 * s.point_uv[1]) * half_h;
                px.push(TrackKeyframe {
                    tick: s.tick,
                    value: f64::from(pos_x),
                    easing: AnimationEasing::Linear,
                });
                py.push(TrackKeyframe {
                    tick: s.tick,
                    value: f64::from(pos_y),
                    easing: AnimationEasing::Linear,
                });
                rz.push(TrackKeyframe {
                    tick: s.tick,
                    value: f64::from(-s.rotation_rad.to_degrees()),
                    easing: AnimationEasing::Linear,
                });
                for vec in [&mut sx, &mut sy, &mut sz] {
                    vec.push(TrackKeyframe {
                        tick: s.tick,
                        value: f64::from(s.scale),
                        easing: AnimationEasing::Linear,
                    });
                }
            }

            let mut out = vec![
                make_track(track_id_prefix, target_node, "pos_x", px),
                make_track(track_id_prefix, target_node, "pos_y", py),
            ];
            if result.start_point2_uv.is_some() {
                out.push(make_track(track_id_prefix, target_node, "rot_z", rz));
                out.push(make_track(track_id_prefix, target_node, "scale_x", sx));
                out.push(make_track(track_id_prefix, target_node, "scale_y", sy));
                out.push(make_track(track_id_prefix, target_node, "scale_z", sz));
            }
            out
        }
        TrackerBindTarget::CornerPin => Vec::new(),
    }
}

/// Converts a [`PlanarTrackResult`] into dense [`KeyframeTrackDef`]s targeting `CornerPin`, `Transform2D`, or `Null3D`.
pub fn planar_track_to_keyframes(
    track_id_prefix: &str,
    target_node: &NodeId,
    target_kind: TrackerBindTarget,
    result: &PlanarTrackResult,
    width: u32,
    height: u32,
    camera: Option<&Camera3DDef>,
) -> Vec<KeyframeTrackDef> {
    match target_kind {
        TrackerBindTarget::CornerPin => {
            let names = [
                "tl_x", "tl_y", "tr_x", "tr_y", "br_x", "br_y", "bl_x", "bl_y",
            ];
            let mut series: [Vec<TrackKeyframe>; 8] = std::array::from_fn(|_| Vec::new());
            for s in &result.samples {
                for corner in 0..4 {
                    series[corner * 2].push(TrackKeyframe {
                        tick: s.tick,
                        value: f64::from(s.corners_uv[corner][0]),
                        easing: AnimationEasing::Linear,
                    });
                    series[corner * 2 + 1].push(TrackKeyframe {
                        tick: s.tick,
                        value: f64::from(s.corners_uv[corner][1]),
                        easing: AnimationEasing::Linear,
                    });
                }
            }
            names
                .into_iter()
                .zip(series)
                .map(|(param, kfs)| make_track(track_id_prefix, target_node, param, kfs))
                .collect()
        }
        TrackerBindTarget::Transform2D | TrackerBindTarget::Transform2DStabilize | TrackerBindTarget::Null3D { .. } => {
            let c0 = quad_center(result.start_corners_uv);
            let point_equiv = PointTrackResult {
                start_point_uv: c0,
                start_point2_uv: Some(result.start_corners_uv[1]),
                samples: result
                    .samples
                    .iter()
                    .map(|s| PointTrackSample {
                        tick: s.tick,
                        point_uv: s.center_uv,
                        point2_uv: Some(s.corners_uv[1]),
                        translate_uv: [s.center_uv[0] - c0[0], s.center_uv[1] - c0[1]],
                        scale: s.scale,
                        rotation_rad: s.rotation_rad,
                        fb_error_px: s.fb_error_px,
                        confidence: s.confidence,
                        ncc_score: 1.0,
                        lost: s.lost,
                    })
                    .collect(),
                mean_fb_error_px: result.mean_fb_error_px,
                min_confidence: result.min_confidence,
                lost_ticks: result.lost_ticks.clone(),
            };
            point_track_to_keyframes(
                track_id_prefix,
                target_node,
                target_kind,
                &point_equiv,
                width,
                height,
                camera,
            )
        }
    }
}

fn make_track(
    prefix: &str,
    target_node: &NodeId,
    param: &str,
    keyframes: Vec<TrackKeyframe>,
) -> KeyframeTrackDef {
    KeyframeTrackDef {
        id: format!("{prefix}_{param}"),
        target_node: target_node.clone(),
        target_param: param.to_owned(),
        keyframes,
    }
}

fn wrap_angle_rad(a: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    let mut r = (a + std::f32::consts::PI).rem_euclid(tau) - std::f32::consts::PI;
    if r <= -std::f32::consts::PI {
        r += tau;
    }
    r
}

fn quad_center(c: [[f32; 2]; 4]) -> [f32; 2] {
    [
        (c[0][0] + c[1][0] + c[2][0] + c[3][0]) * 0.25,
        (c[0][1] + c[1][1] + c[2][1] + c[3][1]) * 0.25,
    ]
}

fn bilinear_quad(c: [[f32; 2]; 4], u: f32, v: f32) -> [f32; 2] {
    let w00 = (1.0 - u) * (1.0 - v);
    let w10 = u * (1.0 - v);
    let w11 = u * v;
    let w01 = (1.0 - u) * v;
    [
        c[0][0] * w00 + c[1][0] * w10 + c[2][0] * w11 + c[3][0] * w01,
        c[0][1] * w00 + c[1][1] * w10 + c[2][1] * w11 + c[3][1] * w01,
    ]
}

fn quad_roi_bounds(c: [[f32; 2]; 4], pad_px: f32, width: u32, height: u32) -> (u32, u32, u32, u32) {
    let w_f = width as f32;
    let h_f = height as f32;
    let mut min_x = w_f;
    let mut min_y = h_f;
    let mut max_x = 0.0f32;
    let mut max_y = 0.0f32;
    for pt in c {
        min_x = min_x.min(pt[0] * w_f);
        min_y = min_y.min(pt[1] * h_f);
        max_x = max_x.max(pt[0] * w_f);
        max_y = max_y.max(pt[1] * h_f);
    }
    let ox = (min_x - pad_px).floor().clamp(0.0, (width - 1) as f32) as u32;
    let oy = (min_y - pad_px).floor().clamp(0.0, (height - 1) as f32) as u32;
    let x1 = (max_x + pad_px).ceil().clamp((ox + 1) as f32, w_f) as u32;
    let y1 = (max_y + pad_px).ceil().clamp((oy + 1) as f32, h_f) as u32;
    (ox, oy, (x1 - ox).max(1), (y1 - oy).max(1))
}

fn project_uv(h: Mat3, uv: [f32; 2]) -> [f32; 2] {
    let v = h * Vec3::new(uv[0], uv[1], 1.0);
    let w = if v.z.abs() > 1e-6 { v.z } else { 1.0 };
    [v.x / w, v.y / w]
}

fn normalize_points_2d(pts: &[[f64; 2]]) -> ([[f64; 3]; 3], Vec<[f64; 2]>) {
    let n = pts.len().max(1) as f64;
    let cx = pts.iter().map(|p| p[0]).sum::<f64>() / n;
    let cy = pts.iter().map(|p| p[1]).sum::<f64>() / n;
    let mean_dist = (pts
        .iter()
        .map(|p| ((p[0] - cx).powi(2) + (p[1] - cy).powi(2)).sqrt())
        .sum::<f64>()
        / n)
        .max(1e-9);
    let s = std::f64::consts::SQRT_2 / mean_dist;
    let t = [
        [s, 0.0, -s * cx],
        [0.0, s, -s * cy],
        [0.0, 0.0, 1.0],
    ];
    let norm = pts
        .iter()
        .map(|p| [s * (p[0] - cx), s * (p[1] - cy)])
        .collect();
    (t, norm)
}

fn solve_linear_system<const N: usize>(mut a: [[f64; N]; N], mut b: [f64; N]) -> Option<[f64; N]> {
    for col in 0..N {
        let mut pivot_row = col;
        let mut best = a[col][col].abs();
        for (r, row) in a.iter().enumerate().skip(col + 1) {
            if row[col].abs() > best {
                best = row[col].abs();
                pivot_row = r;
            }
        }
        if !best.is_finite() || best < 1e-12 {
            return None;
        }
        if pivot_row != col {
            a.swap(col, pivot_row);
            b.swap(col, pivot_row);
        }
        let inv = 1.0 / a[col][col];
        for cell in &mut a[col][col..N] {
            *cell *= inv;
        }
        b[col] *= inv;
        let col_row = a[col];
        for r in 0..N {
            if r == col {
                continue;
            }
            let factor = a[r][col];
            if factor.abs() <= 1e-15 {
                continue;
            }
            for (dst, &src) in a[r][col..N].iter_mut().zip(&col_row[col..N]) {
                *dst -= factor * src;
            }
            b[r] -= factor * b[col];
        }
    }
    Some(b)
}

fn mat3_mul_f64(a: [[f64; 3]; 3], b: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let mut out = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            out[i][j] = a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j];
        }
    }
    out
}

/// Solves `dst ~ H * src` (with `h22 = 1`) via normalized DLT normal equations + 3 Huber IRLS iterations.
fn solve_weighted_homography_irls(
    src: &[[f64; 2]],
    dst: &[[f64; 2]],
    base_weights: &[f64],
) -> Option<Mat3> {
    if src.len() < 6 || src.len() != dst.len() || src.len() != base_weights.len() {
        return None;
    }
    let (t_src, n_src) = normalize_points_2d(src);
    let (t_dst, n_dst) = normalize_points_2d(dst);
    let t_dst_inv = invert_3x3_f64(t_dst)?;

    let mut irls_w = base_weights.to_vec();
    let mut h_norm = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

    for iter in 0..4 {
        let mut ata = [[0.0f64; 8]; 8];
        let mut atb = [0.0f64; 8];

        for i in 0..n_src.len() {
            let [x, y] = n_src[i];
            let [u, v] = n_dst[i];
            let w = irls_w[i].max(1e-6);

            let r1 = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y];
            let r2 = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y];

            for (row, rhs) in [(r1, u), (r2, v)] {
                for r in 0..8 {
                    atb[r] += w * row[r] * rhs;
                    for c in 0..8 {
                        ata[r][c] += w * row[r] * row[c];
                    }
                }
            }
        }
        // Small ridge for numerical stability on near-planar degenerate patches.
        for (i, row) in ata.iter_mut().enumerate() {
            row[i] += 1e-10;
        }
        let h_vec = solve_linear_system(ata, atb)?;
        h_norm = [
            [h_vec[0], h_vec[1], h_vec[2]],
            [h_vec[3], h_vec[4], h_vec[5]],
            [h_vec[6], h_vec[7], 1.0],
        ];

        if iter < 3 {
            let huber_delta = 0.025;
            for i in 0..n_src.len() {
                let [x, y] = n_src[i];
                let [u, v] = n_dst[i];
                let px = h_norm[0][0] * x + h_norm[0][1] * y + h_norm[0][2];
                let py = h_norm[1][0] * x + h_norm[1][1] * y + h_norm[1][2];
                let pw = h_norm[2][0] * x + h_norm[2][1] * y + h_norm[2][2];
                if pw.abs() < 1e-6 {
                    irls_w[i] = 1e-6;
                    continue;
                }
                let err = ((px / pw - u).powi(2) + (py / pw - v).powi(2)).sqrt();
                let huber = if err <= huber_delta {
                    1.0
                } else {
                    huber_delta / err
                };
                irls_w[i] = base_weights[i] * huber;
            }
        }
    }

    let h_full = mat3_mul_f64(t_dst_inv, mat3_mul_f64(h_norm, t_src));
    let scale = if h_full[2][2].abs() > 1e-9 {
        1.0 / h_full[2][2]
    } else {
        1.0
    };
    Some(Mat3::from_cols(
        Vec3::new(
            (h_full[0][0] * scale) as f32,
            (h_full[1][0] * scale) as f32,
            (h_full[2][0] * scale) as f32,
        ),
        Vec3::new(
            (h_full[0][1] * scale) as f32,
            (h_full[1][1] * scale) as f32,
            (h_full[2][1] * scale) as f32,
        ),
        Vec3::new(
            (h_full[0][2] * scale) as f32,
            (h_full[1][2] * scale) as f32,
            (h_full[2][2] * scale) as f32,
        ),
    ))
}

fn solve_weighted_affine(src: &[[f64; 2]], dst: &[[f64; 2]], weights: &[f64]) -> Option<Mat3> {
    if src.len() < 3 || src.len() != dst.len() || src.len() != weights.len() {
        return None;
    }
    let mut atbx = [0.0f64; 3];
    let mut atby = [0.0f64; 3];
    let mut a3 = [[0.0f64; 3]; 3];

    for i in 0..src.len() {
        let w = weights[i].max(1e-6);
        let row = [src[i][0], src[i][1], 1.0];
        for r in 0..3 {
            atbx[r] += w * row[r] * dst[i][0];
            atby[r] += w * row[r] * dst[i][1];
            for c in 0..3 {
                a3[r][c] += w * row[r] * row[c];
            }
        }
    }
    for (i, row) in a3.iter_mut().enumerate() {
        row[i] += 1e-10;
    }
    let px = solve_linear_system(a3, atbx)?;
    let py = solve_linear_system(a3, atby)?;
    Some(Mat3::from_cols(
        Vec3::new(px[0] as f32, py[0] as f32, 0.0),
        Vec3::new(px[1] as f32, py[1] as f32, 0.0),
        Vec3::new(px[2] as f32, py[2] as f32, 1.0),
    ))
}

fn invert_3x3_f64(m: [[f64; 3]; 3]) -> Option<[[f64; 3]; 3]> {
    let c00 = m[1][1] * m[2][2] - m[1][2] * m[2][1];
    let c01 = -(m[1][0] * m[2][2] - m[1][2] * m[2][0]);
    let c02 = m[1][0] * m[2][1] - m[1][1] * m[2][0];
    let det = m[0][0] * c00 + m[0][1] * c01 + m[0][2] * c02;
    if !det.is_finite() || det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    let c10 = -(m[0][1] * m[2][2] - m[0][2] * m[2][1]);
    let c11 = m[0][0] * m[2][2] - m[0][2] * m[2][0];
    let c12 = -(m[0][0] * m[2][1] - m[0][1] * m[2][0]);
    let c20 = m[0][1] * m[1][2] - m[0][2] * m[1][1];
    let c21 = -(m[0][0] * m[1][2] - m[0][2] * m[1][0]);
    let c22 = m[0][0] * m[1][1] - m[0][1] * m[1][0];
    Some([
        [c00 * inv, c10 * inv, c20 * inv],
        [c01 * inv, c11 * inv, c21 * inv],
        [c02 * inv, c12 * inv, c22 * inv],
    ])
}
