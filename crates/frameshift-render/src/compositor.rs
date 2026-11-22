use crate::context::GpuContext;
use crate::error::RenderError;
use crate::lut::CubeLut3D;
use crate::pipelines::{ShaderPipelines, ShaderProgram};
use crate::pool::{
    MsaaTargetCache, PooledTexture, TexturePool, WORKING_TEXTURE_FORMAT, compute_mip_levels,
};
use crate::provider::{FrameProvider, SourceFrame, SourcePixelData};
use crate::uniforms::{
    AnimeImpactFrameUniform, AnimeSharpenUniform, BackdropScrimUniform, BlendUniform,
    BloomPassUniform,
    ColorBasicUniform, ColorSpaceMode, CustomWgslUniform, DitherMode, DofBokehUniform,
    DofCocUniform, FilmHalationUniform, FilmOpticsUniform, FxaaUniform, GpuMeshVertex3D,
    GpuSlugBand, GpuSlugCurve, GpuSlugGlyph, GpuSlugVertex, HalftoneGlowUniform, Lut3DUniform,
    MaskMatteUniform, MotionBlurUniform, MotionTileShakeUniform, OutputAlphaMode,
    ParticleField3DUniform, RadialShockwaveUniform, RibbonTrail3DUniform, SlugUniform,
    Text2DFxUniform, Text2DJfaUniform, Text3DUniform, TonemapMode, Transform3DUniform,
    TransformUniform, WhipTransitionUniform,
};
use crate::warp2d::{CornerPinUniform, GridWarpUniform};
use bytemuck::{Pod, cast_slice};
use frameshift_schema::{
    AnimationEasing, BevelProfile3D, BlendMode, Camera3DDef, Camera3DMode, ColorSpace,
    DisabledPassthrough, EventBoundAnimation, ExtendedColorSpace, GraphNodeDef, HalftoneColorMode,
    LutSourceDef, MAX_PRECOMP_DEPTH, NodeId, NodeKind, ParamBindingDef, Project, RenderGraphDef,
    RenderProfileDef, SchemaError, Shape3DKind, ShutterPhase, SourceFit, TextAlign2D,
    WritingMode2D,
};
use frameshift_time::{ExprContext, FLICKS_PER_SECOND, Flicks, SafeExpr};
use frameshift_vector::{
    FontQuery, ProceduralShapeKind, ResolvedFont, Shape3DParams, Text3DMeshOptions,
    TextLayoutOptions, VariationAxes, VectorError, build_slug_scene, resolve_font_chain,
    shape_text_block,
};
use half::f16;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use wgpu::util::DeviceExt;

/// Final rendered RGBA8 sRGB frame read back from the GPU compositor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedFrame {
    pub width: u32,
    pub height: u32,
    pub tick: Flicks,
    pub rgba8: Vec<u8>,
    pub fnv1a_hash: u64,
}

/// Explicit shutter control for an enabled root `MotionBlurShutter`.
///
/// `sub_samples` is a ceiling: it may only lower each enabled blur node's authored
/// count. Shutter angle and phase replace the root node. A quality profile does
/// not fill this in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShutterOverride {
    pub shutter_angle_deg: f32,
    pub sub_samples: u32,
    pub phase: Option<ShutterPhase>,
}

/// Options controlling final output encoding and optional profile overrides.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderOptions {
    /// Overrides the tonemap curve for final `Rgba8Unorm` encoding (`None` = auto from root `MotionBlurShutter`).
    pub tonemap_mode: Option<TonemapMode>,
    /// Output alpha convention for `RenderedFrame::rgba8`.
    pub output_alpha_mode: OutputAlphaMode,
    /// Linear pre-tonemap exposure multiplier (`1.0` = identity).
    pub exposure: f32,
    /// Explicit shutter ceiling and root angle/phase override.
    ///
    /// Absent means every enabled blur keeps its authored `sub_samples`.
    pub shutter_override: Option<ShutterOverride>,
    /// Dithering mode applied right before `Rgba8Unorm` quantization (defaults to `Off` to preserve M1 hashes).
    pub dither_mode: DitherMode,
    /// Optional full-screen HDR-safe FXAA pass before final tonemap (defaults to `false`).
    pub post_aa: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            tonemap_mode: None,
            output_alpha_mode: OutputAlphaMode::Premultiplied,
            exposure: 1.0,
            shutter_override: None,
            dither_mode: DitherMode::Off,
            post_aa: false,
        }
    }
}

/// Supplies per-sub-tick node parameter overrides and source-tick remapping during DAG evaluation.
pub trait NodeAnimator {
    /// Evaluates dynamic parameter overrides for `node_id` at `sub_tick`.
    ///
    /// A failed expression returns [`RenderError::Expression`] and does not keep
    /// the base parameter value.
    fn evaluate_node(
        &self,
        _node_id: &NodeId,
        base: &NodeKind,
        _sub_tick: Flicks,
    ) -> Result<NodeKind, RenderError> {
        Ok(base.clone())
    }

    /// Maps `(asset_id, clip_id, sub_tick)` to `Some(source_tick)`, or `None` if outside clip bounds.
    fn resolve_source_tick(
        &self,
        _asset_id: &str,
        _clip_id: Option<&str>,
        sub_tick: Flicks,
    ) -> Result<Option<Flicks>, RenderError> {
        Ok(Some(sub_tick))
    }
}

/// Default no-op animator for static `RenderGraphDef` evaluation.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopAnimator;

impl NodeAnimator for NoopAnimator {}

struct CompiledParamBinding<'a> {
    binding: &'a ParamBindingDef,
    expr: SafeExpr,
}

/// Uniformly sampled RMS audio envelope over timeline ticks (`[0.0, 1.0]`).
#[derive(Debug, Clone, PartialEq)]
pub struct AudioRmsEnvelope {
    pub window_ticks: Flicks,
    pub samples: Vec<f32>,
}

impl AudioRmsEnvelope {
    pub fn new(window_ticks: Flicks, samples: Vec<f32>) -> Self {
        Self {
            window_ticks,
            samples,
        }
    }

    /// Samples the RMS envelope at `tick` using linear interpolation between adjacent windows,
    /// clamped to `[0.0, 1.0]`. Returns `0.0` for negative ticks, non-positive `window_ticks`,
    /// or empty samples.
    pub fn sample_at(&self, tick: Flicks) -> f64 {
        if self.samples.is_empty() || self.window_ticks.0 <= 0 || tick.0 < 0 {
            return 0.0;
        }
        let pos = tick.0 as f64 / self.window_ticks.0 as f64;
        let idx = pos.floor() as usize;
        if idx >= self.samples.len() {
            return 0.0;
        }
        let frac = pos - (idx as f64);
        let s0 = f64::from(self.samples[idx]);
        let s1 = self
            .samples
            .get(idx + 1)
            .copied()
            .map(f64::from)
            .unwrap_or(s0);
        let interp = s0 + (s1 - s0) * frac;
        if interp.is_finite() {
            interp.clamp(0.0, 1.0)
        } else {
            0.0
        }
    }
}

/// Evaluates `Project` clip `SpeedCurve`s, `EventBoundAnimation`s, and `ParamBindingDef`
/// `SafeExpr` expressions at each sub-frame `sub_tick`.
pub struct ProjectAnimator<'a> {
    pub project: &'a Project,
    compiled_bindings: Vec<CompiledParamBinding<'a>>,
    audio_envelope: Option<&'a AudioRmsEnvelope>,
}

fn expression_failure(binding: &ParamBindingDef, err: impl std::fmt::Display) -> RenderError {
    RenderError::Expression {
        node_id: binding.node_id.clone(),
        param_name: binding.param_name.clone(),
        message: format!("'{}': {err}", binding.expression),
    }
}

/// Authored motion-blur sample count, lowered only by an explicit ceiling.
///
/// `None` keeps `node_sub_samples`. `Some(limit)` returns the minimum of the two.
/// A quality profile's sample count is not an input.
pub fn effective_blur_sub_samples(node_sub_samples: u32, ceiling: Option<u32>) -> u32 {
    match ceiling {
        Some(limit) => node_sub_samples.min(limit),
        None => node_sub_samples,
    }
}

impl<'a> ProjectAnimator<'a> {
    pub fn new(project: &'a Project) -> Result<Self, RenderError> {
        Self::with_audio_envelope(project, None)
    }

    pub fn with_audio_envelope(
        project: &'a Project,
        envelope: Option<&'a AudioRmsEnvelope>,
    ) -> Result<Self, RenderError> {
        let mut compiled_bindings = Vec::with_capacity(project.param_bindings.len());
        for binding in &project.param_bindings {
            let expr = SafeExpr::parse(&binding.expression)
                .map_err(|err| expression_failure(binding, err))?;
            compiled_bindings.push(CompiledParamBinding { binding, expr });
        }
        Ok(Self {
            project,
            compiled_bindings,
            audio_envelope: envelope,
        })
    }

    fn build_expr_context(&self, sub_tick: Flicks) -> ExprContext {
        let t_sec = sub_tick.0 as f64 / FLICKS_PER_SECOND as f64;
        let nearest_event = self
            .project
            .events
            .iter()
            .min_by_key(|ev| (sub_tick.0 - ev.target_output_tick.0).abs());
        let event_delta_sec = nearest_event
            .map(|ev| (sub_tick.0 - ev.target_output_tick.0) as f64 / FLICKS_PER_SECOND as f64)
            .unwrap_or(0.0);
        let beat_anchor = nearest_event
            .map(|ev| ev.target_output_tick)
            .unwrap_or(Flicks::ZERO);
        // 120 BPM (0.5s beat period) deterministic phase aligned to the nearest event anchor.
        let beat_period_ticks = (FLICKS_PER_SECOND / 2) as f64;
        let beat_phase = ((sub_tick.0 - beat_anchor.0) as f64 / beat_period_ticks).rem_euclid(1.0);

        let has_active_audio = self
            .project
            .timeline
            .audio_clips
            .iter()
            .any(|ac| sub_tick >= ac.timeline_start && sub_tick < ac.timeline_end);
        let audio_rms = if has_active_audio {
            self.audio_envelope
                .map(|env| env.sample_at(sub_tick))
                .unwrap_or_else(|| ((1.0 - beat_phase).powi(2) * 0.8 + 0.15).clamp(0.0, 1.0))
        } else {
            0.0
        };

        let duration = self.project.timeline.duration_ticks.0.max(1) as f64;
        let progress = (sub_tick.0 as f64 / duration).clamp(0.0, 1.0);

        ExprContext {
            t_sec,
            output_tick: sub_tick.0,
            beat_phase,
            audio_rms,
            event_delta_sec,
            progress,
        }
    }
}

impl NodeAnimator for ProjectAnimator<'_> {
    fn evaluate_node(
        &self,
        node_id: &NodeId,
        base: &NodeKind,
        sub_tick: Flicks,
    ) -> Result<NodeKind, RenderError> {
        let mut kind = base.clone();
        for clip in &self.project.timeline.clips {
            for anim in &clip.event_animations {
                if &anim.target_node_id != node_id {
                    continue;
                }
                let Some(event) = self.project.event(&anim.event_id) else {
                    continue;
                };
                if let Some(value) =
                    evaluate_event_animation(anim, event.target_output_tick, sub_tick)
                {
                    apply_animated_parameter(&mut kind, &anim.parameter, value as f32);
                }
            }
        }

        if !self.compiled_bindings.is_empty() {
            let mut ctx_opt: Option<ExprContext> = None;
            for compiled in &self.compiled_bindings {
                if &compiled.binding.node_id != node_id {
                    continue;
                }
                let ctx = ctx_opt.get_or_insert_with(|| self.build_expr_context(sub_tick));
                let value = compiled
                    .expr
                    .eval_default(ctx)
                    .map_err(|err| expression_failure(compiled.binding, err))?;
                apply_animated_parameter(&mut kind, &compiled.binding.param_name, value as f32);
            }
        }

        Ok(kind)
    }

    fn resolve_source_tick(
        &self,
        _asset_id: &str,
        clip_id: Option<&str>,
        sub_tick: Flicks,
    ) -> Result<Option<Flicks>, RenderError> {
        let Some(cid) = clip_id else {
            return Ok(Some(sub_tick));
        };
        let clip = self
            .project
            .clip(cid)
            .ok_or_else(|| frameshift_schema::SchemaError::MissingClip(cid.to_owned()))?;
        if sub_tick < clip.timeline_start || sub_tick >= clip.timeline_end {
            return Ok(None);
        }
        let output_offset = sub_tick - clip.timeline_start;
        let source_offset = clip.speed_curve.source_offset_at(output_offset);
        Ok(Some(clip.source_start + source_offset))
    }
}

/// Evaluates an [`EventBoundAnimation`] at `sample_tick`, returning `None` before its active window,
/// or holding the final value after its active window when `hold_after` is `true`.
pub fn evaluate_event_animation(
    anim: &EventBoundAnimation,
    event_tick: Flicks,
    sample_tick: Flicks,
) -> Option<f64> {
    if anim.keyframes.is_empty() {
        return None;
    }
    let start = event_tick + anim.offset_from_event;
    if anim.duration <= Flicks::ZERO {
        return if sample_tick >= start {
            anim.keyframes.last().map(|k| k.value)
        } else {
            None
        };
    }
    let end = start + anim.duration;
    if sample_tick < start {
        return None;
    }
    if sample_tick > end {
        return if anim.hold_after {
            Some(interpolate_keyframes(&anim.keyframes, 1.0))
        } else {
            None
        };
    }
    let progress = ((sample_tick.0 - start.0) as f64 / (anim.duration.0 as f64)).clamp(0.0, 1.0);
    Some(interpolate_keyframes(&anim.keyframes, progress))
}

fn interpolate_keyframes(keyframes: &[frameshift_schema::AnimationKeyframe], progress: f64) -> f64 {
    if keyframes.len() == 1 || progress <= keyframes[0].progress {
        return keyframes[0].value;
    }
    let last = keyframes.last().expect("non-empty keyframes");
    if progress >= last.progress {
        return last.value;
    }

    for pair in keyframes.windows(2) {
        let a = pair[0];
        let b = pair[1];
        if progress >= a.progress && progress <= b.progress {
            let span = b.progress - a.progress;
            if span <= 0.0 {
                return b.value;
            }
            let local_t = ((progress - a.progress) / span).clamp(0.0, 1.0);
            let eased = apply_easing(a.easing, local_t);
            return a.value + (b.value - a.value) * eased;
        }
    }
    last.value
}

fn apply_easing(easing: AnimationEasing, t: f64) -> f64 {
    let u = t.clamp(0.0, 1.0);
    match easing {
        AnimationEasing::Linear => u,
        AnimationEasing::EaseIn => u * u * u,
        AnimationEasing::EaseOut => 1.0 - (1.0 - u).powi(3),
        AnimationEasing::EaseInOut => {
            if u < 0.5 {
                4.0 * u * u * u
            } else {
                1.0 - (-2.0 * u + 2.0).powi(3) / 2.0
            }
        }
        AnimationEasing::Step => {
            if u >= 1.0 {
                1.0
            } else {
                0.0
            }
        }
    }
}

fn apply_animated_parameter(kind: &mut NodeKind, param: &str, value: f32) {
    match kind {
        NodeKind::Transform2D {
            translate,
            scale,
            rotation_rad,
            anchor,
            ..
        } => match param {
            "translate_x" => translate[0] = value,
            "translate_y" => translate[1] = value,
            "scale" => {
                scale[0] = value;
                scale[1] = value;
            }
            "scale_x" => scale[0] = value,
            "scale_y" => scale[1] = value,
            "rotation_rad" => *rotation_rad = value,
            "anchor_x" => anchor[0] = value,
            "anchor_y" => anchor[1] = value,
            _ => {}
        },
        NodeKind::MaskMatte { feather, .. } => {
            if param == "feather" {
                *feather = value;
            }
        }
        NodeKind::Blend { opacity, .. } => {
            if param == "opacity" {
                *opacity = value;
            }
        }
        NodeKind::ColorBasic {
            exposure,
            contrast,
            saturation,
            flash_amount,
        } => match param {
            "exposure" => *exposure = value,
            "contrast" => *contrast = value,
            "saturation" => *saturation = value,
            "flash_amount" => *flash_amount = value,
            _ => {}
        },
        NodeKind::ShaderTransition {
            progress,
            direction_rad,
            whip_strength,
            chromatic_aberration,
            impact_flash,
            ..
        } => match param {
            "progress" => *progress = value,
            "direction_rad" => *direction_rad = value,
            "whip_strength" => *whip_strength = value,
            "chromatic_aberration" => *chromatic_aberration = value,
            "impact_flash" => *impact_flash = value,
            _ => {}
        },
        NodeKind::Lut3D { intensity, .. } => {
            if param == "intensity" {
                *intensity = value;
            }
        }
        NodeKind::VectorText {
            font_size_norm,
            outline_width_norm,
            y_offset_norm,
            x_offset_norm,
            tracking_em,
            line_height_em,
            opacity,
            var_wght,
            var_wdth,
            var_slnt,
            gradient_angle_deg,
            outer_outline_width_norm,
            outline_gap_norm,
            shadow_blur_norm,
            glow_radius_norm,
            hdr_intensity,
            ..
        } => match param {
            "font_size_norm" => *font_size_norm = value,
            "outline_width_norm" => *outline_width_norm = value,
            "y_offset_norm" => *y_offset_norm = value,
            "x_offset_norm" => *x_offset_norm = Some(value),
            "tracking_em" => *tracking_em = Some(value),
            "line_height_em" => *line_height_em = Some(value),
            "opacity" => *opacity = value.clamp(0.0, 1.0),
            "var_wght" => *var_wght = Some(value),
            "var_wdth" => *var_wdth = Some(value),
            "var_slnt" => *var_slnt = Some(value),
            "gradient_angle_deg" => *gradient_angle_deg = value,
            "outer_outline_width_norm" => *outer_outline_width_norm = value,
            "outline_gap_norm" => *outline_gap_norm = value,
            "shadow_blur_norm" => *shadow_blur_norm = value,
            "glow_radius_norm" => *glow_radius_norm = value,
            "hdr_intensity" => *hdr_intensity = value.max(0.0),
            _ => {}
        },
        NodeKind::MotionTileShake {
            tile_scale,
            mirror_edges,
            shake_amp_norm,
            shake_rot_rad,
            shake_zoom,
            shake_freq_hz,
            shake_decay,
            event_delta_sec,
        } => match param {
            "tile_scale" => *tile_scale = value,
            "mirror_edges" => *mirror_edges = value >= 0.5,
            "shake_amp_norm_x" => shake_amp_norm[0] = value,
            "shake_amp_norm_y" => shake_amp_norm[1] = value,
            "shake_rot_rad" => *shake_rot_rad = value,
            "shake_zoom" => *shake_zoom = value,
            "shake_freq_hz" => *shake_freq_hz = value,
            "shake_decay" => *shake_decay = value,
            "event_delta_sec" => *event_delta_sec = value,
            _ => {}
        },
        NodeKind::AnimeImpactFrame {
            luma_threshold,
            edge_weight,
            invert_amount,
            speed_line_density,
            speed_line_intensity,
            center_uv,
            mix_amount,
        } => match param {
            "luma_threshold" => *luma_threshold = value,
            "edge_weight" => *edge_weight = value,
            "invert_amount" => *invert_amount = value,
            "speed_line_density" => *speed_line_density = value,
            "speed_line_intensity" => *speed_line_intensity = value,
            "center_u" | "center_uv_x" => center_uv[0] = value,
            "center_v" | "center_uv_y" => center_uv[1] = value,
            "mix_amount" => *mix_amount = value,
            _ => {}
        },
        NodeKind::RadialShockwave {
            center_uv,
            radius,
            ring_width,
            refraction_amp,
            zoom_blur_strength,
            chromatic_aberration,
        } => match param {
            "center_u" | "center_uv_x" => center_uv[0] = value,
            "center_v" | "center_uv_y" => center_uv[1] = value,
            "radius" => *radius = value,
            "ring_width" => *ring_width = value,
            "refraction_amp" => *refraction_amp = value,
            "zoom_blur_strength" => *zoom_blur_strength = value,
            "chromatic_aberration" => *chromatic_aberration = value,
            _ => {}
        },
        NodeKind::HalftoneGlow {
            dot_scale,
            angle_rad,
            halftone_mix,
            glow_threshold,
            glow_intensity,
            glow_radius_px,
            dot_softness,
            ..
        } => match param {
            "dot_scale" => *dot_scale = value,
            "angle_rad" => *angle_rad = value,
            "halftone_mix" => *halftone_mix = value,
            "glow_threshold" => *glow_threshold = value,
            "glow_intensity" => *glow_intensity = value,
            "glow_radius_px" => *glow_radius_px = value,
            "dot_softness" => *dot_softness = value,
            _ => {}
        },
        NodeKind::DeepGlow {
            threshold,
            knee,
            intensity,
            radius,
            scatter,
            anamorphic_ratio,
            chromatic_shift,
            ..
        } => match param {
            "threshold" => *threshold = value,
            "knee" => *knee = value,
            "intensity" => *intensity = value,
            "radius" => *radius = value,
            "scatter" => *scatter = value,
            "anamorphic_ratio" => *anamorphic_ratio = value,
            "chromatic_shift" => *chromatic_shift = value,
            _ => {}
        },
        NodeKind::AnimeSharpen {
            line_strength,
            sharpen_amount,
            anti_ringing,
        } => match param {
            "line_strength" => *line_strength = value,
            "sharpen_amount" => *sharpen_amount = value,
            "anti_ringing" => *anti_ringing = value,
            _ => {}
        },
        NodeKind::FilmOptics {
            lens_k1,
            lens_k2,
            chromatic_edge,
            grain_amount,
            grain_seed,
            halation_amount,
            vignette_amount,
            grain_size_px,
            grain_chroma,
            halation_threshold,
            ..
        } => match param {
            "lens_k1" => *lens_k1 = value,
            "lens_k2" => *lens_k2 = value,
            "chromatic_edge" => *chromatic_edge = value,
            "grain_amount" => *grain_amount = value,
            "grain_seed" => *grain_seed = value,
            "halation_amount" => *halation_amount = value,
            "vignette_amount" => *vignette_amount = value,
            "grain_size_px" => *grain_size_px = value,
            "grain_chroma" => *grain_chroma = value,
            "halation_threshold" => *halation_threshold = value,
            _ => {}
        },
        NodeKind::CustomWgsl { params, .. } => match param {
            "p0" => params[0] = value,
            "p1" => params[1] = value,
            "p2" => params[2] = value,
            "p3" => params[3] = value,
            "p4" => params[4] = value,
            "p5" => params[5] = value,
            "p6" => params[6] = value,
            "p7" => params[7] = value,
            "p8" => params[8] = value,
            "p9" => params[9] = value,
            "p10" => params[10] = value,
            "p11" => params[11] = value,
            "p12" => params[12] = value,
            "p13" => params[13] = value,
            "p14" => params[14] = value,
            "p15" => params[15] = value,
            _ => {}
        },
        NodeKind::Transform3D {
            pos,
            rot_deg,
            scale,
            fov_deg,
            camera_pos,
            camera_rot_deg,
            dof_focal_dist,
            dof_blur_strength,
        } => match param {
            "pos_x" => pos[0] = value,
            "pos_y" => pos[1] = value,
            "pos_z" => pos[2] = value,
            "rot_x" => rot_deg[0] = value,
            "rot_y" => rot_deg[1] = value,
            "rot_z" => rot_deg[2] = value,
            "scale_x" => scale[0] = value,
            "scale_y" => scale[1] = value,
            "scale_z" => scale[2] = value,
            "cam_x" => camera_pos[0] = value,
            "cam_y" => camera_pos[1] = value,
            "cam_z" => camera_pos[2] = value,
            "cam_rot_x" => camera_rot_deg[0] = value,
            "cam_rot_y" => camera_rot_deg[1] = value,
            "cam_rot_z" => camera_rot_deg[2] = value,
            "fov_deg" => *fov_deg = value,
            "dof_focal_dist" => *dof_focal_dist = value,
            "dof_blur_strength" => *dof_blur_strength = value,
            _ => {}
        },
        NodeKind::Text3D {
            extrude_depth,
            pos,
            rot_deg,
            camera_pos,
            camera_rot_deg,
            fov_deg,
            char_stagger_sec,
            char_bounce_amp,
            char_wave_freq,
            time_offset_sec,
            outline_width,
            pop_duration_sec,
            pop_overshoot,
            spin_in_deg,
            char_jitter_deg,
            arc_curvature,
            hdr_intensity,
            opacity,
            tracking_em,
            var_wght,
            var_wdth,
            var_slnt,
            ..
        } => match param {
            "pos_x" => pos[0] = value,
            "pos_y" => pos[1] = value,
            "pos_z" => pos[2] = value,
            "rot_x" => rot_deg[0] = value,
            "rot_y" => rot_deg[1] = value,
            "rot_z" => rot_deg[2] = value,
            "cam_x" => camera_pos[0] = value,
            "cam_y" => camera_pos[1] = value,
            "cam_z" => camera_pos[2] = value,
            "cam_rot_x" => camera_rot_deg[0] = value,
            "cam_rot_y" => camera_rot_deg[1] = value,
            "cam_rot_z" => camera_rot_deg[2] = value,
            "fov_deg" => *fov_deg = value,
            "extrude_depth" => *extrude_depth = value,
            "char_bounce_amp" => *char_bounce_amp = value,
            "char_stagger_sec" => *char_stagger_sec = value,
            "char_wave_freq" => *char_wave_freq = value,
            "outline_width" => *outline_width = value,
            "pop_duration_sec" => *pop_duration_sec = value,
            "pop_overshoot" => *pop_overshoot = value,
            "spin_in_deg" => *spin_in_deg = value,
            "char_jitter_deg" => *char_jitter_deg = value,
            "arc_curvature" => *arc_curvature = value,
            "hdr_intensity" => *hdr_intensity = value,
            "time_offset_sec" => *time_offset_sec = value,
            "opacity" => *opacity = value,
            "tracking_em" => *tracking_em = value,
            "var_wght" => *var_wght = Some(value),
            "var_wdth" => *var_wdth = Some(value),
            "var_slnt" => *var_slnt = Some(value),
            _ => {}
        },
        NodeKind::Shape3D {
            size,
            aspect,
            stroke_width,
            corner_radius,
            inner_ratio,
            extrude_depth,
            bevel_size,
            pos,
            rot_deg,
            scale,
            camera_pos,
            camera_rot_deg,
            fov_deg,
            pop_duration_sec,
            pop_overshoot,
            spin_in_deg,
            hdr_intensity,
            time_offset_sec,
            opacity,
            ..
        } => match param {
            "size" => *size = value,
            "aspect" => *aspect = value,
            "stroke_width" => *stroke_width = value,
            "corner_radius" => *corner_radius = value,
            "inner_ratio" => *inner_ratio = value,
            "extrude_depth" => *extrude_depth = value,
            "bevel_size" => *bevel_size = value,
            "pos_x" => pos[0] = value,
            "pos_y" => pos[1] = value,
            "pos_z" => pos[2] = value,
            "rot_x" => rot_deg[0] = value,
            "rot_y" => rot_deg[1] = value,
            "rot_z" => rot_deg[2] = value,
            "scale_x" => scale[0] = value,
            "scale_y" => scale[1] = value,
            "scale_z" => scale[2] = value,
            "cam_x" => camera_pos[0] = value,
            "cam_y" => camera_pos[1] = value,
            "cam_z" => camera_pos[2] = value,
            "cam_rot_x" => camera_rot_deg[0] = value,
            "cam_rot_y" => camera_rot_deg[1] = value,
            "cam_rot_z" => camera_rot_deg[2] = value,
            "fov_deg" => *fov_deg = value,
            "pop_duration_sec" => *pop_duration_sec = value,
            "pop_overshoot" => *pop_overshoot = value,
            "spin_in_deg" => *spin_in_deg = value,
            "hdr_intensity" => *hdr_intensity = value,
            "time_offset_sec" => *time_offset_sec = value,
            "opacity" => *opacity = value,
            _ => {}
        },
        NodeKind::ParticleField3D {
            speed,
            spread,
            size,
            camera_pos,
            camera_rot_deg,
            fov_deg,
            time_offset_sec,
            dof_focal_dist,
            dof_blur_strength,
            opacity,
            ..
        } => match param {
            "speed" => *speed = value,
            "spread" => *spread = value,
            "size" => *size = value,
            "cam_x" => camera_pos[0] = value,
            "cam_y" => camera_pos[1] = value,
            "cam_z" => camera_pos[2] = value,
            "cam_rot_x" => camera_rot_deg[0] = value,
            "cam_rot_y" => camera_rot_deg[1] = value,
            "cam_rot_z" => camera_rot_deg[2] = value,
            "fov_deg" => *fov_deg = value,
            "time_offset_sec" => *time_offset_sec = value,
            "dof_focal_dist" => *dof_focal_dist = value,
            "dof_blur_strength" => *dof_blur_strength = value,
            "opacity" => *opacity = value,
            _ => {}
        },
        NodeKind::RibbonTrail3D {
            width,
            speed,
            radius,
            trim_start,
            trim_end,
            camera_pos,
            camera_rot_deg,
            fov_deg,
            time_offset_sec,
            strand_spacing,
            curve_bend,
            pos,
            rot_deg,
            scale,
            opacity,
            core_whiten,
            core_intensity,
            halo_intensity,
            saturation,
            min_width_px,
            ..
        } => match param {
            "width" => *width = value,
            "core_whiten" => *core_whiten = value.clamp(0.0, 1.0),
            "core_intensity" => *core_intensity = value.max(0.0),
            "halo_intensity" => *halo_intensity = value.max(0.0),
            "saturation" => *saturation = value.max(0.0),
            "min_width_px" => *min_width_px = value.max(0.0),
            "speed" => *speed = value,
            "radius" => *radius = value,
            "trim_start" => *trim_start = value,
            "trim_end" => *trim_end = value,
            "strand_spacing" => *strand_spacing = value,
            "curve_bend" => *curve_bend = value,
            "pos_x" => pos[0] = value,
            "pos_y" => pos[1] = value,
            "pos_z" => pos[2] = value,
            "rot_x" => rot_deg[0] = value,
            "rot_y" => rot_deg[1] = value,
            "rot_z" => rot_deg[2] = value,
            "scale_x" => scale[0] = value,
            "scale_y" => scale[1] = value,
            "scale_z" => scale[2] = value,
            "cam_x" => camera_pos[0] = value,
            "cam_y" => camera_pos[1] = value,
            "cam_z" => camera_pos[2] = value,
            "cam_rot_x" => camera_rot_deg[0] = value,
            "cam_rot_y" => camera_rot_deg[1] = value,
            "cam_rot_z" => camera_rot_deg[2] = value,
            "fov_deg" => *fov_deg = value,
            "time_offset_sec" => *time_offset_sec = value,
            "opacity" => *opacity = value,
            _ => {}
        },
        NodeKind::Scene3D { camera, .. } => match param {
            "cam_x" => camera.pos[0] = value,
            "cam_y" => camera.pos[1] = value,
            "cam_z" => camera.pos[2] = value,
            "cam_rot_x" => camera.rot_deg[0] = value,
            "cam_rot_y" => camera.rot_deg[1] = value,
            "cam_rot_z" | "cam_roll" => camera.rot_deg[2] = value,
            "cam_target_x" => camera.target[0] = value,
            "cam_target_y" => camera.target[1] = value,
            "cam_target_z" => camera.target[2] = value,
            "fov_deg" => camera.fov_deg = value,
            "dof_focal_dist" => camera.dof_focal_dist = value,
            "dof_blur_strength" => camera.dof_blur_strength = value,
            "dof_max_radius_px" => camera.dof_max_radius_px = value,
            "dof_highlight_boost" => camera.dof_highlight_boost = value,
            "dof_blade_rotation_deg" => camera.dof_blade_rotation_deg = value,
            "dof_blade_roundness" => camera.dof_blade_roundness = value,
            "dof_anamorphic_ratio" => camera.dof_anamorphic_ratio = value,
            "dof_cats_eye_strength" => camera.dof_cats_eye_strength = value,
            _ => {}
        },
        NodeKind::SourceImage { opacity, .. } => {
            if param == "opacity" {
                *opacity = value.clamp(0.0, 1.0);
            }
        }
        NodeKind::Precomp {
            time_offset_ticks,
            time_scale,
            opacity,
            ..
        } => match param {
            "time_offset_ticks" => {
                if value.is_finite() {
                    *time_offset_ticks = value.round() as i64;
                }
            }
            "time_scale" => *time_scale = value,
            "opacity" => *opacity = value.clamp(0.0, 1.0),
            _ => {}
        },
        NodeKind::VectorPathHud {
            stroke_width_norm,
            dash_len_norm,
            gap_len_norm,
            dash_offset_norm,
            trim_start,
            trim_end,
            node_dot_radius_norm,
            data_value,
            opacity,
            ..
        } => match param {
            "stroke_width_norm" => *stroke_width_norm = value,
            "dash_len_norm" => *dash_len_norm = value,
            "gap_len_norm" => *gap_len_norm = value,
            "dash_offset_norm" => *dash_offset_norm = value,
            "trim_start" => *trim_start = value,
            "trim_end" => *trim_end = value,
            "node_dot_radius_norm" => *node_dot_radius_norm = value,
            "data_value" => *data_value = value,
            "opacity" => *opacity = value.clamp(0.0, 1.0),
            _ => {}
        },
        NodeKind::CornerPin {
            tl,
            tr,
            br,
            bl,
            opacity,
            ..
        } => match param {
            "tl_x" => tl[0] = value,
            "tl_y" => tl[1] = value,
            "tr_x" => tr[0] = value,
            "tr_y" => tr[1] = value,
            "br_x" => br[0] = value,
            "br_y" => br[1] = value,
            "bl_x" => bl[0] = value,
            "bl_y" => bl[1] = value,
            "opacity" => *opacity = value.clamp(0.0, 1.0),
            _ => {}
        },
        NodeKind::GridWarp {
            wave_amp,
            wave_freq,
            wave_phase,
            mix_amount,
            opacity,
            ..
        } => match param {
            "wave_amp_x" => wave_amp[0] = value,
            "wave_amp_y" => wave_amp[1] = value,
            "wave_freq_x" => wave_freq[0] = value,
            "wave_freq_y" => wave_freq[1] = value,
            "wave_phase" => *wave_phase = value,
            "mix_amount" => *mix_amount = value,
            "opacity" => *opacity = value.clamp(0.0, 1.0),
            _ => {}
        },
        NodeKind::BackdropScrim {
            rect_center_norm,
            rect_size_norm,
            corner_radius_norm,
            blur_radius_norm,
            opacity,
            auto_contrast,
            ..
        } => match param {
            "rect_center_x" => rect_center_norm[0] = value,
            "rect_center_y" => rect_center_norm[1] = value,
            "rect_size_x" => rect_size_norm[0] = value.max(0.0),
            "rect_size_y" => rect_size_norm[1] = value.max(0.0),
            "corner_radius_norm" => *corner_radius_norm = value.max(0.0),
            "blur_radius_norm" => *blur_radius_norm = value.max(0.0),
            "opacity" => *opacity = value.clamp(0.0, 1.0),
            "auto_contrast" => *auto_contrast = value.clamp(0.0, 1.0),
            _ => {}
        },
        NodeKind::SourceVideo { .. }
        | NodeKind::Solid { .. }
        | NodeKind::MotionBlurShutter { .. }
        | NodeKind::PrecompInput { .. } => {}
    }
}

/// Computes deterministic midpoint sample ticks inside the shutter window for `MotionBlurShutter`.
pub fn compute_shutter_samples(
    tick: Flicks,
    frame_ticks: Flicks,
    shutter_angle_deg: f32,
    sub_samples: u32,
    phase: ShutterPhase,
) -> Result<Vec<Flicks>, RenderError> {
    if !shutter_angle_deg.is_finite() || !(0.0..=720.0).contains(&shutter_angle_deg) {
        return Err(RenderError::InvalidMotionBlurParams(format!(
            "shutter_angle_deg must be within 0.0..=720.0, got {shutter_angle_deg}"
        )));
    }
    if sub_samples == 0 {
        return Err(RenderError::InvalidMotionBlurParams(
            "sub_samples must be >= 1".into(),
        ));
    }
    if sub_samples == 1 || shutter_angle_deg == 0.0 {
        return Ok(vec![tick]);
    }
    if frame_ticks.0 <= 0 {
        return Err(RenderError::InvalidMotionBlurParams(format!(
            "frame_ticks must be > 0 when motion blur is active, got {}",
            frame_ticks.0
        )));
    }

    let window_ticks =
        ((frame_ticks.0 as f64) * (shutter_angle_deg as f64) / 360.0).round() as i128;
    if window_ticks <= 0 {
        return Ok(vec![tick; sub_samples as usize]);
    }

    let (t_start, t_end) = match phase {
        ShutterPhase::Centered => {
            let start = (tick.0 as i128) - window_ticks / 2;
            (start, start + window_ticks)
        }
        ShutterPhase::Trailing => {
            let end = tick.0 as i128;
            (end - window_ticks, end)
        }
    };

    let span = (t_end - t_start) as f64;
    let n = sub_samples as usize;
    let mut samples = Vec::with_capacity(n);
    for i in 0..n {
        let frac = (i as f64 + 0.5) / (n as f64);
        let sample_tick = (t_start as f64 + frac * span).round() as i64;
        samples.push(Flicks(sample_tick));
    }
    Ok(samples)
}

/// Maps a parent `sub_tick` into a `Precomp` sub-graph's local time using `time_scale`
/// and `time_offset_ticks`.
pub fn remap_precomp_tick(sub_tick: Flicks, time_offset_ticks: i64, time_scale: f32) -> Flicks {
    if (time_scale - 1.0).abs() <= f32::EPSILON {
        return Flicks(sub_tick.0.saturating_add(time_offset_ticks));
    }
    let scaled = ((sub_tick.0 as f64) * f64::from(time_scale)).round();
    let scaled_i64 = if scaled.is_finite() {
        scaled.clamp(i64::MIN as f64, i64::MAX as f64) as i64
    } else {
        0
    };
    Flicks(scaled_i64.saturating_add(time_offset_ticks))
}

/// Deterministic FNV-1a 64-bit hash over pixel bytes.
pub fn fnv1a_64(bytes: &[u8]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0100_0000_01b3;
    let mut hash = FNV_OFFSET_BASIS;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum LutSourceCacheKey {
    Legacy {
        spec: String,
        file_len: Option<u64>,
        modified_nanos: Option<u128>,
    },
    Preset(String),
    CubeFile {
        path: String,
        lut_size_override: Option<u32>,
        file_len: Option<u64>,
        modified_nanos: Option<u128>,
    },
    Ocio {
        config_path: String,
        src_space: String,
        dst_space: String,
        look: Option<String>,
        lut_size: u32,
        shaper_space: Option<String>,
        config_len: Option<u64>,
        config_modified_nanos: Option<u128>,
    },
    BuiltinTransform {
        src_space: ExtendedColorSpace,
        dst_space: ExtendedColorSpace,
        lut_size: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct LutCacheKey {
    source: LutSourceCacheKey,
    input_space: Option<ExtendedColorSpace>,
    output_space: Option<ExtendedColorSpace>,
}

struct CachedLut3D {
    size: u32,
    domain_min: [f32; 3],
    domain_inv_span: [f32; 3],
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
}

type ImageGpuKey = (String, u32, u32, u8, u8);

struct CachedImageTexture {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Text3DMeshKey {
    font_path: String,
    text: String,
    font_size_bits: u32,
    extrude_depth_bits: u32,
    bevel_size_bits: u32,
    outline_width_bits: u32,
    bevel_profile: u8,
    bevel_segments: u32,
    align: u8,
    tracking_bits: u32,
    line_height_bits: Option<u32>,
    fallback_fonts: Vec<String>,
    var_wght_bits: Option<u32>,
    var_wdth_bits: Option<u32>,
    var_slnt_bits: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SlugSceneCacheKey {
    text: String,
    font_family: Option<String>,
    font_path: Option<String>,
    font_weight: u16,
    fallback_fonts: Vec<String>,
    writing_mode: u8,
    align: u8,
    font_px_bits: u32,
    tracking_bits: Option<u32>,
    line_height_bits: Option<u32>,
    var_wght_bits: Option<u32>,
    var_wdth_bits: Option<u32>,
    var_slnt_bits: Option<u32>,
}

struct CachedSlugScene {
    vertex_buffer: Option<wgpu::Buffer>,
    index_buffer: Option<wgpu::Buffer>,
    glyphs_buffer: Option<wgpu::Buffer>,
    hbands_buffer: Option<wgpu::Buffer>,
    vbands_buffer: Option<wgpu::Buffer>,
    band_indices_buffer: Option<wgpu::Buffer>,
    curves_buffer: Option<wgpu::Buffer>,
    index_count: u32,
    block_bounds: [f32; 4],
}

type Shape3DMeshKey = (u8, u32, u32, u32, u32, u32, u32, u32);

struct CachedMesh3D {
    vertex_buffer: Option<wgpu::Buffer>,
    index_buffer: Option<wgpu::Buffer>,
    index_count: u32,
}

/// Headless `wgpu` + WGSL unified GPU compositor (`Rgba16Float` premultiplied-alpha working space).
pub struct GpuCompositor {
    ctx: GpuContext,
    pipelines: ShaderPipelines,
    pool: TexturePool,
    lut_cache: HashMap<LutCacheKey, CachedLut3D>,
    image_gpu_cache: HashMap<ImageGpuKey, CachedImageTexture>,
    font_cache: HashMap<String, Arc<Vec<u8>>>,
    text3d_mesh_cache: HashMap<Text3DMeshKey, CachedMesh3D>,
    slug_cache: HashMap<SlugSceneCacheKey, CachedSlugScene>,
    shape3d_mesh_cache: HashMap<Shape3DMeshKey, CachedMesh3D>,
    depth_cache: HashMap<(u32, u32), (wgpu::Texture, wgpu::TextureView)>,
    msaa_cache: MsaaTargetCache,
    _dummy_zero_tex: wgpu::Texture,
    dummy_zero_view: wgpu::TextureView,
}

impl GpuCompositor {
    /// Initializes a headless [`GpuContext`], compiles all built-in WGSL pipelines, and creates a [`TexturePool`].
    pub fn new_headless() -> Result<Self, RenderError> {
        let ctx = GpuContext::new_headless()?;
        let pipelines = ShaderPipelines::compile_all_with_msaa(&ctx.device, ctx.supports_msaa_4x)?;
        let (_dummy_zero_tex, dummy_zero_view) = create_dummy_zero_texture(&ctx.device, &ctx.queue);

        Ok(Self {
            ctx,
            pipelines,
            pool: TexturePool::new(),
            lut_cache: HashMap::new(),
            image_gpu_cache: HashMap::new(),
            font_cache: HashMap::new(),
            text3d_mesh_cache: HashMap::new(),
            slug_cache: HashMap::new(),
            shape3d_mesh_cache: HashMap::new(),
            depth_cache: HashMap::new(),
            msaa_cache: MsaaTargetCache::default(),
            _dummy_zero_tex,
            dummy_zero_view,
        })
    }

    /// Returns the underlying [`GpuContext`].
    pub fn context(&self) -> &GpuContext {
        &self.ctx
    }

    /// Returns the compiled [`ShaderPipelines`].
    pub fn pipelines(&self) -> &ShaderPipelines {
        &self.pipelines
    }

    /// Returns the [`TexturePool`].
    pub fn texture_pool(&self) -> &TexturePool {
        &self.pool
    }

    /// Returns a mutable reference to the [`TexturePool`].
    pub fn texture_pool_mut(&mut self) -> &mut TexturePool {
        &mut self.pool
    }

    /// Evaluates `graph` at a specific `sub_tick` into a pooled `Rgba16Float` Linear Rec.709 texture.
    #[allow(clippy::too_many_arguments)]
    pub fn evaluate_subframe(
        &mut self,
        graph: &RenderGraphDef,
        sub_tick: Flicks,
        frame_ticks: Flicks,
        width: u32,
        height: u32,
        provider: &dyn FrameProvider,
        animator: &dyn NodeAnimator,
    ) -> Result<PooledTexture, RenderError> {
        validate_dimensions(width, height)?;
        let full_topo = graph.validate_graph()?;
        let mut encoder = self
            .ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frameshift-evaluate-subframe"),
            });

        let tex = self.evaluate_subgraph(
            graph,
            &full_topo,
            &graph.output_node,
            sub_tick,
            frame_ticks,
            width,
            height,
            provider,
            animator,
            None,
            None,
            &mut encoder,
        )?;

        self.ctx.queue.submit(std::iter::once(encoder.finish()));
        Ok(tex)
    }

    /// Renders a complete output frame at `tick` (including sub-frame temporal motion blur if present),
    /// converts Linear `Rgba16Float` -> sRGB `Rgba8Unorm`, and reads pixels back to CPU.
    #[allow(clippy::too_many_arguments)]
    pub fn render_frame(
        &mut self,
        graph: &RenderGraphDef,
        tick: Flicks,
        frame_ticks: Flicks,
        width: u32,
        height: u32,
        provider: &dyn FrameProvider,
        animator: &dyn NodeAnimator,
        options: &RenderOptions,
    ) -> Result<RenderedFrame, RenderError> {
        validate_dimensions(width, height)?;
        let full_topo = graph.validate_graph()?;

        let root_node = graph.node(&graph.output_node).ok_or_else(|| {
            frameshift_schema::SchemaError::MissingNode(graph.output_node.clone())
        })?;

        let root_tonemap = matches!(
            root_node.kind,
            NodeKind::MotionBlurShutter { tonemap: true, .. }
        );
        let tonemap_mode = options.tonemap_mode.unwrap_or(if root_tonemap {
            TonemapMode::AcesLite
        } else {
            TonemapMode::None
        });

        let mut encoder = self
            .ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frameshift-render-frame"),
            });

        let sample_ceiling = options.shutter_override.map(|ov| ov.sub_samples);
        let linear_result = self.evaluate_subgraph(
            graph,
            &full_topo,
            &graph.output_node,
            tick,
            frame_ticks,
            width,
            height,
            provider,
            animator,
            options.shutter_override,
            sample_ceiling,
            &mut encoder,
        )?;

        let final_linear = if options.post_aa {
            let aa_tex = self.pool.acquire(&self.ctx.device, width, height);
            let aa_uniform = FxaaUniform::new(width, height);
            self.record_one_tex_pass(
                &mut encoder,
                "post-fxaa",
                &self.pipelines.fx_aa,
                &linear_result.view,
                &aa_uniform,
                &aa_tex.view,
            );
            self.pool.release(linear_result);
            aa_tex
        } else {
            linear_result
        };

        let srgb_texture = self.ctx.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("frameshift-output-rgba8"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let srgb_view = srgb_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let dither_strength = match options.dither_mode {
            DitherMode::Off => 0.0,
            DitherMode::TpdfIgn => 1.0,
        };
        let tonemap_uniform = MotionBlurUniform::tonemap_to_srgb_with_dither(
            options.exposure,
            tonemap_mode,
            options.output_alpha_mode,
            options.dither_mode,
            dither_strength,
        );
        self.record_two_tex_pass(
            &mut encoder,
            "tonemap-to-srgb",
            &self.pipelines.motion_blur_tonemap_srgb,
            &final_linear.view,
            &self.dummy_zero_view,
            &tonemap_uniform,
            &srgb_view,
        );

        self.pool.release(final_linear);

        let rgba8 = self.readback_rgba8(encoder, &srgb_texture, width, height)?;
        let fnv1a_hash = fnv1a_64(&rgba8);

        Ok(RenderedFrame {
            width,
            height,
            tick,
            rgba8,
            fnv1a_hash,
        })
    }

    /// Renders a frame from a [`Project`] using the specified [`RenderProfileDef`] and [`FrameProvider`].
    pub fn render_project_frame(
        &mut self,
        project: &Project,
        tick: Flicks,
        profile: &RenderProfileDef,
        provider: &dyn FrameProvider,
        options: &RenderOptions,
    ) -> Result<RenderedFrame, RenderError> {
        self.render_project_frame_with_audio(project, tick, profile, provider, options, None)
    }

    /// Renders a frame from a [`Project`] with an optional precomputed [`AudioRmsEnvelope`].
    pub fn render_project_frame_with_audio(
        &mut self,
        project: &Project,
        output_tick: Flicks,
        profile: &RenderProfileDef,
        provider: &dyn FrameProvider,
        options: &RenderOptions,
        audio_envelope: Option<&AudioRmsEnvelope>,
    ) -> Result<RenderedFrame, RenderError> {
        project.validate()?;
        let frame_ticks = project.timeline.fps.frame_duration()?;
        let animator = ProjectAnimator::with_audio_envelope(project, audio_envelope)?;
        self.render_frame(
            &project.graph,
            output_tick,
            frame_ticks,
            profile.resolution.width,
            profile.resolution.height,
            provider,
            &animator,
            options,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn evaluate_subgraph(
        &mut self,
        graph: &RenderGraphDef,
        full_topo: &[NodeId],
        target_id: &NodeId,
        sub_tick: Flicks,
        frame_ticks: Flicks,
        width: u32,
        height: u32,
        provider: &dyn FrameProvider,
        animator: &dyn NodeAnimator,
        root_shutter_override: Option<ShutterOverride>,
        sample_ceiling: Option<u32>,
        encoder: &mut wgpu::CommandEncoder,
    ) -> Result<PooledTexture, RenderError> {
        self.evaluate_subgraph_inner(
            graph,
            full_topo,
            target_id,
            sub_tick,
            frame_ticks,
            width,
            height,
            provider,
            animator,
            root_shutter_override,
            sample_ceiling,
            &[],
            0,
            encoder,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn evaluate_subgraph_inner(
        &mut self,
        graph: &RenderGraphDef,
        full_topo: &[NodeId],
        target_id: &NodeId,
        sub_tick: Flicks,
        frame_ticks: Flicks,
        width: u32,
        height: u32,
        provider: &dyn FrameProvider,
        animator: &dyn NodeAnimator,
        root_shutter_override: Option<ShutterOverride>,
        sample_ceiling: Option<u32>,
        precomp_inputs: &[&PooledTexture],
        depth: usize,
        encoder: &mut wgpu::CommandEncoder,
    ) -> Result<PooledTexture, RenderError> {
        if depth > MAX_PRECOMP_DEPTH {
            return Err(RenderError::Schema(SchemaError::PrecompDepthExceeded {
                node_id: target_id.clone(),
                depth,
                max_depth: MAX_PRECOMP_DEPTH,
            }));
        }

        let target_node = graph
            .node(target_id)
            .ok_or_else(|| SchemaError::MissingNode(target_id.clone()))?;

        // Disabled nodes copy an input. The shutter override below stays on the enabled path.
        if target_node.disabled_passthrough().is_none()
            && let NodeKind::MotionBlurShutter {
                shutter_angle_deg,
                sub_samples,
                phase,
                ..
            } = target_node.kind
        {
            if target_node.inputs.len() != 1 {
                return Err(RenderError::MissingGraphInput {
                    node_id: target_node.id.clone(),
                    expected: 1,
                    actual: target_node.inputs.len(),
                });
            }
            let samples = effective_blur_sub_samples(sub_samples, sample_ceiling);
            let (angle, phase_used) = match root_shutter_override {
                Some(ov) => (ov.shutter_angle_deg, ov.phase.unwrap_or(phase)),
                None => (shutter_angle_deg, phase),
            };
            return self.evaluate_motion_blur(
                graph,
                full_topo,
                &target_node.inputs[0],
                sub_tick,
                frame_ticks,
                angle,
                samples,
                phase_used,
                width,
                height,
                provider,
                animator,
                sample_ceiling,
                precomp_inputs,
                depth,
                encoder,
            );
        }

        let cone = collect_direct_cone(graph, target_id)?;
        let sub_order: Vec<&GraphNodeDef> = full_topo
            .iter()
            .filter(|id| cone.contains(*id))
            .filter_map(|id| graph.node(id))
            .collect();

        let skip_standalone = collect_scene3d_skip_standalone(&sub_order, target_id);

        let mut remaining_uses: HashMap<NodeId, usize> = HashMap::with_capacity(sub_order.len());
        for node in &sub_order {
            if skip_standalone.contains(&node.id) {
                continue;
            }
            for upstream in &node.inputs {
                *remaining_uses.entry(upstream.clone()).or_insert(0) += 1;
            }
            if node.disabled_passthrough().is_none()
                && matches!(node.kind, NodeKind::Scene3D { .. })
            {
                for child_id in &node.inputs {
                    if let Some(child) = graph.node(child_id)
                        && child.enabled
                        && matches!(child.kind, NodeKind::Transform3D { .. })
                        && let Some(tex_in) = child.inputs.first()
                    {
                        *remaining_uses.entry(tex_in.clone()).or_insert(0) += 1;
                    }
                }
            }
        }
        *remaining_uses.entry(target_id.clone()).or_insert(0) += 1;

        let mut textures: HashMap<NodeId, PooledTexture> = HashMap::with_capacity(sub_order.len());

        for node in sub_order {
            if skip_standalone.contains(&node.id) {
                continue;
            }
            let (out_tex, release_inputs, release_scene_xform_inputs) = if let Some(pass) =
                node.disabled_passthrough()
            {
                let release_inputs = match pass {
                    DisabledPassthrough::Clear => true,
                    DisabledPassthrough::Input(index) => node
                        .inputs
                        .get(index)
                        .is_some_and(|id| textures.contains_key(id)),
                };
                let out_tex = self.evaluate_disabled_node(
                    graph,
                    full_topo,
                    node,
                    pass,
                    sub_tick,
                    frame_ticks,
                    width,
                    height,
                    provider,
                    animator,
                    &textures,
                    sample_ceiling,
                    precomp_inputs,
                    depth,
                    encoder,
                )?;
                (out_tex, release_inputs, false)
            } else {
                let evaluated_kind = animator.evaluate_node(&node.id, &node.kind, sub_tick)?;
                let (out_tex, is_scene3d) = if let NodeKind::MotionBlurShutter {
                    shutter_angle_deg,
                    sub_samples,
                    phase,
                    ..
                } = evaluated_kind
                {
                    if node.inputs.len() != 1 {
                        return Err(RenderError::MissingGraphInput {
                            node_id: node.id.clone(),
                            expected: 1,
                            actual: node.inputs.len(),
                        });
                    }
                    let samples = effective_blur_sub_samples(sub_samples, sample_ceiling);
                    let tex = self.evaluate_motion_blur(
                        graph,
                        full_topo,
                        &node.inputs[0],
                        sub_tick,
                        frame_ticks,
                        shutter_angle_deg,
                        samples,
                        phase,
                        width,
                        height,
                        provider,
                        animator,
                        sample_ceiling,
                        precomp_inputs,
                        depth,
                        encoder,
                    )?;
                    (tex, false)
                } else if let NodeKind::Precomp {
                    ref sub_graph,
                    time_offset_ticks,
                    time_scale,
                    opacity,
                } = evaluated_kind
                {
                    let child_depth = depth + 1;
                    if child_depth > MAX_PRECOMP_DEPTH {
                        return Err(RenderError::Schema(SchemaError::PrecompDepthExceeded {
                            node_id: node.id.clone(),
                            depth: child_depth,
                            max_depth: MAX_PRECOMP_DEPTH,
                        }));
                    }
                    if opacity <= 0.001 {
                        let out = self.pool.acquire(&self.ctx.device, width, height);
                        clear_texture(encoder, &out.view, [0.0, 0.0, 0.0, 0.0]);
                        (out, false)
                    } else {
                        let sub_topo = sub_graph.validate_graph_with_depth(child_depth)?;
                        let mut child_inputs: Vec<&PooledTexture> =
                            Vec::with_capacity(node.inputs.len());
                        for idx in 0..node.inputs.len() {
                            child_inputs.push(require_input(node, &textures, idx)?);
                        }
                        let local_tick =
                            remap_precomp_tick(sub_tick, time_offset_ticks, time_scale);
                        let sub_tex = self.evaluate_subgraph_inner(
                            sub_graph,
                            &sub_topo,
                            &sub_graph.output_node,
                            local_tick,
                            frame_ticks,
                            width,
                            height,
                            provider,
                            animator,
                            None,
                            sample_ceiling,
                            &child_inputs,
                            child_depth,
                            encoder,
                        )?;
                        let op = opacity.clamp(0.0, 1.0);
                        let final_tex = if op < 0.999 {
                            let scaled = self.pool.acquire(&self.ctx.device, width, height);
                            let uniform = BlendUniform::new(BlendMode::Normal, op);
                            self.record_two_tex_pass(
                                encoder,
                                &format!("{}-precomp-opacity", node.id.0),
                                &self.pipelines.blend,
                                &self.dummy_zero_view,
                                &sub_tex.view,
                                &uniform,
                                &scaled.view,
                            );
                            self.pool.release(sub_tex);
                            scaled
                        } else {
                            sub_tex
                        };
                        (final_tex, false)
                    }
                } else if let NodeKind::Scene3D {
                    ref camera,
                    background_rgba,
                } = evaluated_kind
                {
                    let tex = self.evaluate_scene3d(
                        graph,
                        node,
                        camera,
                        background_rgba,
                        sub_tick,
                        width,
                        height,
                        animator,
                        &textures,
                        encoder,
                    )?;
                    (tex, true)
                } else {
                    let tex = self.evaluate_single_node(
                        node,
                        &evaluated_kind,
                        sub_tick,
                        width,
                        height,
                        provider,
                        animator,
                        &textures,
                        precomp_inputs,
                        encoder,
                    )?;
                    (tex, false)
                };
                let release_inputs = !matches!(evaluated_kind, NodeKind::MotionBlurShutter { .. });
                (out_tex, release_inputs, is_scene3d)
            };

            textures.insert(node.id.clone(), out_tex);

            if release_inputs {
                for upstream in &node.inputs {
                    release_if_last_use(&mut self.pool, &mut remaining_uses, &mut textures, upstream);
                }
                if release_scene_xform_inputs {
                    for child_id in &node.inputs {
                        if let Some(child) = graph.node(child_id)
                            && child.enabled
                            && matches!(child.kind, NodeKind::Transform3D { .. })
                            && let Some(tex_in) = child.inputs.first()
                        {
                            release_if_last_use(
                                &mut self.pool,
                                &mut remaining_uses,
                                &mut textures,
                                tex_in,
                            );
                        }
                    }
                }
            }
        }

        textures
            .remove(target_id)
            .ok_or_else(|| RenderError::Schema(SchemaError::MissingNode(target_id.clone())))
    }

    #[allow(clippy::too_many_arguments)]
    fn evaluate_disabled_node(
        &mut self,
        graph: &RenderGraphDef,
        full_topo: &[NodeId],
        node: &GraphNodeDef,
        pass: DisabledPassthrough,
        sub_tick: Flicks,
        frame_ticks: Flicks,
        width: u32,
        height: u32,
        provider: &dyn FrameProvider,
        animator: &dyn NodeAnimator,
        textures: &HashMap<NodeId, PooledTexture>,
        sample_ceiling: Option<u32>,
        precomp_inputs: &[&PooledTexture],
        depth: usize,
        encoder: &mut wgpu::CommandEncoder,
    ) -> Result<PooledTexture, RenderError> {
        let DisabledPassthrough::Input(index) = pass else {
            let out = self.pool.acquire(&self.ctx.device, width, height);
            clear_texture(encoder, &out.view, [0.0, 0.0, 0.0, 0.0]);
            return Ok(out);
        };
        let input_id = node
            .inputs
            .get(index)
            .ok_or_else(|| RenderError::MissingGraphInput {
                node_id: node.id.clone(),
                expected: node.kind.expected_input_count(),
                actual: node.inputs.len(),
            })?;
        if let Some(src) = textures.get(input_id) {
            let out = self.pool.acquire(&self.ctx.device, width, height);
            debug_assert_eq!((src.width, src.height), (width, height));
            copy_texture_2d(encoder, &src.texture, &out.texture, width, height);
            return Ok(out);
        }
        // Nested motion blur is not part of this cone; its input is rendered on its own.
        if matches!(node.kind, NodeKind::MotionBlurShutter { .. }) {
            return self.evaluate_subgraph_inner(
                graph,
                full_topo,
                input_id,
                sub_tick,
                frame_ticks,
                width,
                height,
                provider,
                animator,
                None,
                sample_ceiling,
                precomp_inputs,
                depth,
                encoder,
            );
        }
        Err(RenderError::Schema(SchemaError::MissingNode(
            input_id.clone(),
        )))
    }

    #[allow(clippy::too_many_arguments)]
    fn evaluate_motion_blur(
        &mut self,
        graph: &RenderGraphDef,
        full_topo: &[NodeId],
        input_id: &NodeId,
        tick: Flicks,
        frame_ticks: Flicks,
        shutter_angle_deg: f32,
        sub_samples: u32,
        phase: ShutterPhase,
        width: u32,
        height: u32,
        provider: &dyn FrameProvider,
        animator: &dyn NodeAnimator,
        sample_ceiling: Option<u32>,
        precomp_inputs: &[&PooledTexture],
        depth: usize,
        encoder: &mut wgpu::CommandEncoder,
    ) -> Result<PooledTexture, RenderError> {
        let sample_ticks =
            compute_shutter_samples(tick, frame_ticks, shutter_angle_deg, sub_samples, phase)?;

        if sample_ticks.len() == 1 {
            return self.evaluate_subgraph_inner(
                graph,
                full_topo,
                input_id,
                sample_ticks[0],
                frame_ticks,
                width,
                height,
                provider,
                animator,
                None,
                sample_ceiling,
                precomp_inputs,
                depth,
                encoder,
            );
        }

        let weight = 1.0 / (sample_ticks.len() as f32);
        let mut accum_tex: Option<PooledTexture> = None;

        for t_i in sample_ticks {
            let sample_tex = self.evaluate_subgraph_inner(
                graph,
                full_topo,
                input_id,
                t_i,
                frame_ticks,
                width,
                height,
                provider,
                animator,
                None,
                sample_ceiling,
                precomp_inputs,
                depth,
                encoder,
            )?;

            let next_accum = self.pool.acquire(&self.ctx.device, width, height);
            let uniform = if accum_tex.is_some() {
                MotionBlurUniform::accumulate(weight, 1.0)
            } else {
                MotionBlurUniform::accumulate(weight, 0.0)
            };
            let prev_view = accum_tex
                .as_ref()
                .map(|t| &t.view)
                .unwrap_or(&self.dummy_zero_view);

            self.record_two_tex_pass(
                encoder,
                "motion-blur-accum",
                &self.pipelines.motion_blur_accumulate,
                &sample_tex.view,
                prev_view,
                &uniform,
                &next_accum.view,
            );

            self.pool.release(sample_tex);
            if let Some(prev) = accum_tex.replace(next_accum) {
                self.pool.release(prev);
            }
        }

        Ok(accum_tex.expect("sample_ticks is guaranteed non-empty"))
    }

    #[allow(clippy::too_many_arguments)]
    fn evaluate_single_node(
        &mut self,
        node: &GraphNodeDef,
        kind: &NodeKind,
        sub_tick: Flicks,
        width: u32,
        height: u32,
        provider: &dyn FrameProvider,
        animator: &dyn NodeAnimator,
        textures: &HashMap<NodeId, PooledTexture>,
        precomp_inputs: &[&PooledTexture],
        encoder: &mut wgpu::CommandEncoder,
    ) -> Result<PooledTexture, RenderError> {
        match kind {
            NodeKind::SourceVideo {
                asset_id,
                clip_id,
                color_space,
                ..
            } => {
                let maybe_src_tick =
                    animator.resolve_source_tick(asset_id, clip_id.as_deref(), sub_tick)?;
                let out = self.pool.acquire(&self.ctx.device, width, height);
                let Some(src_tick) = maybe_src_tick else {
                    clear_texture(encoder, &out.view, [0.0, 0.0, 0.0, 0.0]);
                    return Ok(out);
                };

                let fit = kind.source_fit().unwrap_or(SourceFit::Stretch);
                let frame =
                    provider.provide_source_frame(asset_id, src_tick, width, height, fit)?;
                self.upload_source_frame(encoder, asset_id, &frame, *color_space, &out.view)?;
                Ok(out)
            }
            NodeKind::SourceImage {
                asset_id,
                color_space,
                fit,
                opacity,
            } => {
                let out = self.pool.acquire(&self.ctx.device, width, height);
                let op = opacity.clamp(0.0, 1.0);
                if op <= 0.001 {
                    clear_texture(encoder, &out.view, [0.0, 0.0, 0.0, 0.0]);
                    return Ok(out);
                }

                let key: ImageGpuKey = (
                    asset_id.clone(),
                    width,
                    height,
                    source_fit_code(*fit),
                    color_space_code(*color_space),
                );
                if !self.image_gpu_cache.contains_key(&key) {
                    let frame =
                        provider.provide_source_frame(asset_id, Flicks::ZERO, width, height, *fit)?;
                    let cached_tex = self.ctx.device.create_texture(&wgpu::TextureDescriptor {
                        label: Some("frameshift-source-image-cache"),
                        size: wgpu::Extent3d {
                            width,
                            height,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: WORKING_TEXTURE_FORMAT,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                            | wgpu::TextureUsages::TEXTURE_BINDING
                            | wgpu::TextureUsages::COPY_SRC
                            | wgpu::TextureUsages::COPY_DST,
                        view_formats: &[],
                    });
                    let cached_view =
                        cached_tex.create_view(&wgpu::TextureViewDescriptor::default());
                    self.upload_source_frame(
                        encoder,
                        asset_id,
                        &frame,
                        *color_space,
                        &cached_view,
                    )?;
                    self.image_gpu_cache.insert(
                        key.clone(),
                        CachedImageTexture {
                            texture: cached_tex,
                            view: cached_view,
                        },
                    );
                }

                let cached = &self.image_gpu_cache[&key];
                if op >= 0.999 {
                    copy_texture_2d(encoder, &cached.texture, &out.texture, width, height);
                } else {
                    let uniform = BlendUniform::new(BlendMode::Normal, op);
                    self.record_two_tex_pass(
                        encoder,
                        &node.id.0,
                        &self.pipelines.blend,
                        &self.dummy_zero_view,
                        &cached.view,
                        &uniform,
                        &out.view,
                    );
                }
                Ok(out)
            }
            NodeKind::PrecompInput { slot } => {
                let src = precomp_inputs.get(*slot).ok_or_else(|| {
                    RenderError::Schema(SchemaError::InvalidPrecomp {
                        node_id: node.id.clone(),
                        message: format!(
                            "precomp_input slot {slot} out of bounds (available: {})",
                            precomp_inputs.len()
                        ),
                    })
                })?;
                let out = self.pool.acquire(&self.ctx.device, width, height);
                copy_texture_2d(encoder, &src.texture, &out.texture, width, height);
                Ok(out)
            }
            NodeKind::Solid {
                color_rgba,
                color_space,
            } => {
                let out = self.pool.acquire(&self.ctx.device, width, height);
                let linear = match color_space {
                    ColorSpace::LinearRec709 => *color_rgba,
                    ColorSpace::DisplaySrgb => premul_srgb_to_linear(*color_rgba),
                };
                clear_texture(encoder, &out.view, linear);
                Ok(out)
            }
            NodeKind::Transform2D {
                translate,
                scale,
                rotation_rad,
                anchor,
                filter,
            } => {
                let src = require_input(node, textures, 0)?;
                let out = self.pool.acquire(&self.ctx.device, width, height);
                let dst_aspect = width as f32 / height as f32;
                let uniform = TransformUniform::new(
                    *translate,
                    *scale,
                    *anchor,
                    *rotation_rad,
                    1.0,
                    dst_aspect,
                    0.0,
                    *filter,
                );
                let needs_mips = *filter == frameshift_schema::SampleFilter::Bilinear
                    && (scale[0].abs() < 0.95 || scale[1].abs() < 0.95);
                if needs_mips && let Some(mip_src) = self.build_mipmapped_copy(encoder, src) {
                    self.record_one_tex_pass(
                        encoder,
                        &node.id.0,
                        &self.pipelines.transform,
                        &mip_src.view,
                        &uniform,
                        &out.view,
                    );
                    self.pool.release(mip_src);
                } else {
                    self.record_one_tex_pass(
                        encoder,
                        &node.id.0,
                        &self.pipelines.transform,
                        &src.view,
                        &uniform,
                        &out.view,
                    );
                }
                Ok(out)
            }
            NodeKind::MaskMatte {
                mode,
                invert,
                feather,
                opaque_black_outside,
            } => {
                let src = require_input(node, textures, 0)?;
                let mask = require_input(node, textures, 1)?;
                let out = self.pool.acquire(&self.ctx.device, width, height);
                let uniform = MaskMatteUniform::from_params(*mode, *invert, *feather, 1.0)
                    .with_opaque_black_outside(*opaque_black_outside);
                self.record_two_tex_pass(
                    encoder,
                    &node.id.0,
                    &self.pipelines.mask_matte,
                    &src.view,
                    &mask.view,
                    &uniform,
                    &out.view,
                );
                Ok(out)
            }
            NodeKind::Blend { mode, opacity } => {
                let base = require_input(node, textures, 0)?;
                let top = require_input(node, textures, 1)?;
                let out = self.pool.acquire(&self.ctx.device, width, height);
                let uniform = BlendUniform::new(*mode, *opacity);
                self.record_two_tex_pass(
                    encoder,
                    &node.id.0,
                    &self.pipelines.blend,
                    &base.view,
                    &top.view,
                    &uniform,
                    &out.view,
                );
                Ok(out)
            }
            NodeKind::ColorBasic {
                exposure,
                contrast,
                saturation,
                flash_amount,
            } => {
                let src = require_input(node, textures, 0)?;
                let out = self.pool.acquire(&self.ctx.device, width, height);
                let uniform = ColorBasicUniform::new(
                    *exposure,
                    *contrast,
                    *saturation,
                    *flash_amount,
                    ColorSpaceMode::LinearToLinear,
                );
                self.record_one_tex_pass(
                    encoder,
                    &node.id.0,
                    &self.pipelines.color_basic,
                    &src.view,
                    &uniform,
                    &out.view,
                );
                Ok(out)
            }
            NodeKind::ShaderTransition {
                shader_name,
                progress,
                direction_rad,
                whip_strength,
                chromatic_aberration,
                impact_flash,
            } => {
                let program = match shader_name.as_str() {
                    "whip_chromatic"
                    | "transition_whip_chromatic"
                    | "transition_whip_chromatic.wgsl" => &self.pipelines.transition_whip_chromatic,
                    "cross_zoom" | "transition_cross_zoom" | "transition_cross_zoom.wgsl" => {
                        &self.pipelines.transition_cross_zoom
                    }
                    "directional_warp"
                    | "transition_directional_warp"
                    | "transition_directional_warp.wgsl" => {
                        &self.pipelines.transition_directional_warp
                    }
                    "glitch_displace"
                    | "transition_glitch_displace"
                    | "transition_glitch_displace.wgsl" => {
                        &self.pipelines.transition_glitch_displace
                    }
                    "dreamy_zoom" | "transition_dreamy_zoom" | "transition_dreamy_zoom.wgsl" => {
                        &self.pipelines.transition_dreamy_zoom
                    }
                    "swirl" | "transition_swirl" | "transition_swirl.wgsl" => {
                        &self.pipelines.transition_swirl
                    }
                    "linear_blur" | "transition_linear_blur" | "transition_linear_blur.wgsl" => {
                        &self.pipelines.transition_linear_blur
                    }
                    "burn" | "transition_burn" | "transition_burn.wgsl" => {
                        &self.pipelines.transition_burn
                    }
                    "ripple" | "transition_ripple" | "transition_ripple.wgsl" => {
                        &self.pipelines.transition_ripple
                    }
                    "kaleidoscope"
                    | "transition_kaleidoscope"
                    | "transition_kaleidoscope.wgsl" => &self.pipelines.transition_kaleidoscope,
                    other => {
                        return Err(RenderError::UnsupportedShaderTransition(other.to_owned()));
                    }
                };
                let tex_a = require_input(node, textures, 0)?;
                let tex_b = require_input(node, textures, 1)?;
                let out = self.pool.acquire(&self.ctx.device, width, height);
                let aspect_ratio = width as f32 / height.max(1) as f32;
                let uniform = WhipTransitionUniform::new(
                    *progress,
                    *direction_rad,
                    *whip_strength,
                    *chromatic_aberration,
                    *impact_flash,
                )
                .with_aspect(aspect_ratio);
                self.record_two_tex_pass(
                    encoder,
                    &node.id.0,
                    program,
                    &tex_a.view,
                    &tex_b.view,
                    &uniform,
                    &out.view,
                );
                Ok(out)
            }
            NodeKind::VectorText {
                font_family,
                font_path,
                gpu_slug,
                ..
            } => {
                let override_key = format!("vector_text://{}", node.id.0);
                let frame = match provider.provide_frame(&override_key, sub_tick, width, height) {
                    Ok(frame) => Some(frame),
                    Err(RenderError::FrameProvider {
                        asset_id,
                        source_tick,
                        message,
                    }) => {
                        if let Some(rest) = message.strip_prefix("vector error: font file not found: ")
                        {
                            return Err(RenderError::Vector(VectorError::FontNotFound(
                                rest.to_owned(),
                            )));
                        }
                        if let Some(rest) = message.strip_prefix("vector error: invalid font data: ")
                        {
                            return Err(RenderError::Vector(VectorError::InvalidFont(
                                rest.to_owned(),
                            )));
                        }
                        if !is_vector_override_absent_message(&message) {
                            return Err(RenderError::FrameProvider {
                                asset_id,
                                source_tick,
                                message,
                            });
                        }
                        None
                    }
                    Err(other) => return Err(other),
                };

                if let Some(frame) = frame {
                    let out = self.pool.acquire(&self.ctx.device, width, height);
                    self.upload_source_frame(
                        encoder,
                        &override_key,
                        &frame,
                        ColorSpace::LinearRec709,
                        &out.view,
                    )?;
                    Ok(out)
                } else if *gpu_slug && (font_family.is_some() || font_path.is_some()) {
                    self.render_vector_text_slug_gpu(encoder, &node.id.0, kind, width, height)
                } else {
                    let rendered = frameshift_vector::render_vector_text_node_rgba16f(
                        kind, width, height, sub_tick,
                    )?;
                    let frame = SourceFrame::rgba_f32_premul(
                        rendered.width,
                        rendered.height,
                        rendered.data,
                    );
                    let out = self.pool.acquire(&self.ctx.device, width, height);
                    self.upload_source_frame(
                        encoder,
                        &override_key,
                        &frame,
                        ColorSpace::LinearRec709,
                        &out.view,
                    )?;
                    Ok(out)
                }
            }
            NodeKind::VectorPathHud { .. } => {
                let override_key = format!("vector_path_hud://{}", node.id.0);
                let frame = match provider.provide_frame(&override_key, sub_tick, width, height) {
                    Ok(frame) => frame,
                    Err(RenderError::Schema(SchemaError::MissingAsset(ref missing)))
                        if missing == &override_key =>
                    {
                        let rendered = frameshift_vector::render_vector_path_hud_node_rgba16f(
                            kind, width, height, sub_tick,
                        )?;
                        SourceFrame::rgba_f32_premul(rendered.width, rendered.height, rendered.data)
                    }
                    Err(RenderError::FrameProvider {
                        asset_id,
                        source_tick,
                        message,
                    }) => {
                        if !is_vector_override_absent_message(&message) {
                            return Err(RenderError::FrameProvider {
                                asset_id,
                                source_tick,
                                message,
                            });
                        }
                        let rendered = frameshift_vector::render_vector_path_hud_node_rgba16f(
                            kind, width, height, sub_tick,
                        )?;
                        SourceFrame::rgba_f32_premul(rendered.width, rendered.height, rendered.data)
                    }
                    Err(other) => return Err(other),
                };
                let out = self.pool.acquire(&self.ctx.device, width, height);
                self.upload_source_frame(
                    encoder,
                    &override_key,
                    &frame,
                    ColorSpace::LinearRec709,
                    &out.view,
                )?;
                Ok(out)
            }
            NodeKind::Lut3D {
                input: _,
                lut_preset,
                intensity,
                lut_source,
                input_space,
                output_space,
            } => {
                let src = require_input(node, textures, 0)?;
                let out = self.pool.acquire(&self.ctx.device, width, height);
                let key = build_lut_cache_key(
                    lut_source.as_ref(),
                    lut_preset,
                    *input_space,
                    *output_space,
                );
                self.ensure_lut3d_cached(
                    &key,
                    lut_source.as_ref(),
                    lut_preset,
                    *input_space,
                    *output_space,
                )?;
                let cached = &self.lut_cache[&key];
                let uniform = Lut3DUniform::new(
                    *intensity,
                    cached.size,
                    cached.domain_min,
                    cached.domain_inv_span,
                );
                self.record_two_tex_pass(
                    encoder,
                    &node.id.0,
                    &self.pipelines.lut3d,
                    &src.view,
                    &cached.view,
                    &uniform,
                    &out.view,
                );
                Ok(out)
            }
            NodeKind::MotionTileShake {
                tile_scale,
                mirror_edges,
                shake_amp_norm,
                shake_rot_rad,
                shake_zoom,
                shake_freq_hz,
                shake_decay,
                event_delta_sec,
            } => {
                let src = require_input(node, textures, 0)?;
                let out = self.pool.acquire(&self.ctx.device, width, height);
                let uniform = MotionTileShakeUniform::new(
                    *tile_scale,
                    *mirror_edges,
                    *shake_amp_norm,
                    *shake_rot_rad,
                    *shake_zoom,
                    *shake_freq_hz,
                    *shake_decay,
                    *event_delta_sec,
                );
                self.record_one_tex_pass(
                    encoder,
                    &node.id.0,
                    &self.pipelines.fx_motion_tile_shake,
                    &src.view,
                    &uniform,
                    &out.view,
                );
                Ok(out)
            }
            NodeKind::AnimeImpactFrame {
                luma_threshold,
                edge_weight,
                invert_amount,
                speed_line_density,
                speed_line_intensity,
                center_uv,
                mix_amount,
            } => {
                let src = require_input(node, textures, 0)?;
                let out = self.pool.acquire(&self.ctx.device, width, height);
                let aspect_ratio = width as f32 / height.max(1) as f32;
                let seed = (sub_tick.0 as f64 / FLICKS_PER_SECOND as f64) as f32;
                let uniform = AnimeImpactFrameUniform::new(
                    *luma_threshold,
                    *edge_weight,
                    *invert_amount,
                    *speed_line_density,
                    *speed_line_intensity,
                    *center_uv,
                    *mix_amount,
                    aspect_ratio,
                    seed,
                );
                self.record_one_tex_pass(
                    encoder,
                    &node.id.0,
                    &self.pipelines.fx_anime_impact_frame,
                    &src.view,
                    &uniform,
                    &out.view,
                );
                Ok(out)
            }
            NodeKind::RadialShockwave {
                center_uv,
                radius,
                ring_width,
                refraction_amp,
                zoom_blur_strength,
                chromatic_aberration,
            } => {
                let src = require_input(node, textures, 0)?;
                let out = self.pool.acquire(&self.ctx.device, width, height);
                let aspect_ratio = width as f32 / height.max(1) as f32;
                let uniform = RadialShockwaveUniform::new(
                    *center_uv,
                    *radius,
                    *ring_width,
                    *refraction_amp,
                    *zoom_blur_strength,
                    *chromatic_aberration,
                    aspect_ratio,
                );
                self.record_one_tex_pass(
                    encoder,
                    &node.id.0,
                    &self.pipelines.fx_radial_shockwave,
                    &src.view,
                    &uniform,
                    &out.view,
                );
                Ok(out)
            }
            NodeKind::HalftoneGlow {
                dot_scale,
                angle_rad,
                halftone_mix,
                glow_threshold,
                glow_intensity,
                glow_radius_px,
                color_mode,
                dot_softness,
            } => {
                let src = require_input(node, textures, 0)?;
                let out = self.pool.acquire(&self.ctx.device, width, height);
                let aspect_ratio = width as f32 / height.max(1) as f32;
                let mode_u32 = match color_mode {
                    HalftoneColorMode::Monochrome => 0u32,
                    HalftoneColorMode::Cmyk => 1u32,
                    HalftoneColorMode::ColorDot => 2u32,
                };
                let uniform = HalftoneGlowUniform::new(
                    *dot_scale,
                    *angle_rad,
                    *halftone_mix,
                    *glow_threshold,
                    *glow_intensity,
                    *glow_radius_px,
                    aspect_ratio,
                )
                .with_style(mode_u32, *dot_softness);
                self.record_one_tex_pass(
                    encoder,
                    &node.id.0,
                    &self.pipelines.fx_halftone_glow,
                    &src.view,
                    &uniform,
                    &out.view,
                );
                Ok(out)
            }
            NodeKind::DeepGlow {
                threshold,
                knee,
                intensity,
                radius,
                tint_rgb,
                mip_levels,
                scatter,
                anamorphic_ratio,
                chromatic_shift,
            } => {
                let src = require_input(node, textures, 0)?;
                let levels = (*mip_levels).clamp(2, 6) as usize;

                // Downsample pyramid: mip 0..levels
                let mut down_mips: Vec<PooledTexture> = Vec::with_capacity(levels);
                let mut prev_w = width;
                let mut prev_h = height;
                for i in 0..levels {
                    let next_w = (prev_w / 2).max(1);
                    let next_h = (prev_h / 2).max(1);
                    let target = self.pool.acquire(&self.ctx.device, next_w, next_h);
                    let uniform = BloomPassUniform::with_optics(
                        [1.0 / prev_w as f32, 1.0 / prev_h as f32],
                        *threshold,
                        *knee,
                        *intensity,
                        *radius,
                        i == 0,
                        false,
                        *tint_rgb,
                        *scatter,
                        *anamorphic_ratio,
                        *chromatic_shift,
                        0.0,
                    );
                    let src_view = if i == 0 {
                        &src.view
                    } else {
                        &down_mips[i - 1].view
                    };
                    self.record_one_tex_pass(
                        encoder,
                        &format!("{}-down-{i}", node.id.0),
                        &self.pipelines.fx_bloom_downsample,
                        src_view,
                        &uniform,
                        &target.view,
                    );
                    down_mips.push(target);
                    prev_w = next_w;
                    prev_h = next_h;
                }

                // Upsample pyramid: merge lower mip into higher mip via scatter-weighted 3x3 tent filter
                let mut current_bloom = down_mips.pop().expect("levels >= 2 guarantees non-empty");
                let mut up_idx = 0usize;
                let up_total = levels.saturating_sub(1).max(1) as f32;
                while let Some(base_mip) = down_mips.pop() {
                    let up_target = self
                        .pool
                        .acquire(&self.ctx.device, base_mip.width, base_mip.height);
                    let mip_norm = 1.0 - (up_idx as f32 / up_total);
                    let uniform = BloomPassUniform::with_optics(
                        [
                            1.0 / current_bloom.width as f32,
                            1.0 / current_bloom.height as f32,
                        ],
                        *threshold,
                        *knee,
                        *intensity,
                        *radius,
                        false,
                        false,
                        *tint_rgb,
                        *scatter,
                        *anamorphic_ratio,
                        *chromatic_shift,
                        mip_norm,
                    );
                    self.record_two_tex_pass(
                        encoder,
                        &format!("{}-up-{up_idx}", node.id.0),
                        &self.pipelines.fx_bloom_upsample,
                        &base_mip.view,
                        &current_bloom.view,
                        &uniform,
                        &up_target.view,
                    );
                    self.pool.release(base_mip);
                    self.pool.release(current_bloom);
                    current_bloom = up_target;
                    up_idx += 1;
                }

                // Final composite over original full-res input
                let out = self.pool.acquire(&self.ctx.device, width, height);
                let final_uniform = BloomPassUniform::with_optics(
                    [
                        1.0 / current_bloom.width as f32,
                        1.0 / current_bloom.height as f32,
                    ],
                    *threshold,
                    *knee,
                    *intensity,
                    *radius,
                    false,
                    true,
                    *tint_rgb,
                    *scatter,
                    *anamorphic_ratio,
                    *chromatic_shift,
                    0.0,
                );
                self.record_two_tex_pass(
                    encoder,
                    &format!("{}-composite", node.id.0),
                    &self.pipelines.fx_bloom_upsample,
                    &src.view,
                    &current_bloom.view,
                    &final_uniform,
                    &out.view,
                );
                self.pool.release(current_bloom);
                Ok(out)
            }
            NodeKind::AnimeSharpen {
                line_strength,
                sharpen_amount,
                anti_ringing,
            } => {
                let src = require_input(node, textures, 0)?;
                let out = self.pool.acquire(&self.ctx.device, width, height);
                let uniform = AnimeSharpenUniform::new(
                    [1.0 / width as f32, 1.0 / height as f32],
                    *line_strength,
                    *sharpen_amount,
                    *anti_ringing,
                );
                self.record_one_tex_pass(
                    encoder,
                    &node.id.0,
                    &self.pipelines.fx_anime_sharpen,
                    &src.view,
                    &uniform,
                    &out.view,
                );
                Ok(out)
            }
            NodeKind::FilmOptics {
                lens_k1,
                lens_k2,
                chromatic_edge,
                grain_amount,
                grain_seed,
                halation_amount,
                vignette_amount,
                auto_scale,
                grain_size_px,
                grain_chroma,
                halation_threshold,
            } => {
                let src = require_input(node, textures, 0)?;
                let out = self.pool.acquire(&self.ctx.device, width, height);
                let aspect = width as f32 / height.max(1) as f32;
                let uniform = FilmOpticsUniform::new_ex(
                    *lens_k1,
                    *lens_k2,
                    *chromatic_edge,
                    *grain_amount,
                    *grain_seed,
                    *halation_amount,
                    *vignette_amount,
                    aspect,
                    *auto_scale,
                    *grain_size_px,
                    *grain_chroma,
                    *halation_threshold,
                );

                if *halation_amount <= 1e-5 {
                    self.record_two_tex_pass(
                        encoder,
                        &node.id.0,
                        &self.pipelines.fx_film_optics,
                        &src.view,
                        &self.dummy_zero_view,
                        &uniform,
                        &out.view,
                    );
                    return Ok(out);
                }

                // Pass A: Lens distortion + 7-tap spectral CA (if active)
                let needs_lens = lens_k1.abs() > 1e-6
                    || lens_k2.abs() > 1e-6
                    || chromatic_edge.abs() > 1e-5;
                let lens_tex_opt = if needs_lens {
                    let lt = self.pool.acquire(&self.ctx.device, width, height);
                    self.record_two_tex_pass(
                        encoder,
                        &format!("{}-lens", node.id.0),
                        &self.pipelines.fx_film_optics_lens,
                        &src.view,
                        &self.dummy_zero_view,
                        &uniform,
                        &lt.view,
                    );
                    Some(lt)
                } else {
                    None
                };
                let lens_view = lens_tex_opt.as_ref().map(|t| &t.view).unwrap_or(&src.view);

                // Pass B: Two-scale separable bilinear Gaussian red-orange halation
                let hw = (width / 2).max(1);
                let hh = (height / 2).max(1);
                let qw = (hw / 2).max(1);
                let qh = (hh / 2).max(1);

                let hal_half_a = self.pool.acquire(&self.ctx.device, hw, hh);
                let hal_half_b = self.pool.acquire(&self.ctx.device, hw, hh);
                let hal_qtr_a = self.pool.acquire(&self.ctx.device, qw, qh);
                let hal_qtr_b = self.pool.acquire(&self.ctx.device, qw, qh);
                let hal_merged = self.pool.acquire(&self.ctx.device, hw, hh);

                let extract_u = FilmHalationUniform::extract(
                    [1.0 / width as f32, 1.0 / height as f32],
                    *halation_threshold,
                    0.25,
                );
                self.record_two_tex_pass(
                    encoder,
                    &format!("{}-hal-extract", node.id.0),
                    &self.pipelines.fx_film_halation,
                    lens_view,
                    &self.dummy_zero_view,
                    &extract_u,
                    &hal_half_a.view,
                );

                // Downsample half -> quarter (threshold=0)
                let down_u = FilmHalationUniform::extract(
                    [1.0 / hw as f32, 1.0 / hh as f32],
                    0.0,
                    0.05,
                );
                self.record_two_tex_pass(
                    encoder,
                    &format!("{}-hal-down", node.id.0),
                    &self.pipelines.fx_film_halation,
                    &hal_half_a.view,
                    &self.dummy_zero_view,
                    &down_u,
                    &hal_qtr_a.view,
                );

                // Tight separable H+V blur on half-res
                let half_texel = [1.0 / hw as f32, 1.0 / hh as f32];
                self.record_two_tex_pass(
                    encoder,
                    &format!("{}-hal-half-h", node.id.0),
                    &self.pipelines.fx_film_halation,
                    &hal_half_a.view,
                    &self.dummy_zero_view,
                    &FilmHalationUniform::blur(half_texel, 1.0, 0.0),
                    &hal_half_b.view,
                );
                self.record_two_tex_pass(
                    encoder,
                    &format!("{}-hal-half-v", node.id.0),
                    &self.pipelines.fx_film_halation,
                    &hal_half_b.view,
                    &self.dummy_zero_view,
                    &FilmHalationUniform::blur(half_texel, 0.0, 1.0),
                    &hal_half_a.view,
                );

                // Wide separable H+V blur on quarter-res
                let qtr_texel = [1.0 / qw as f32, 1.0 / qh as f32];
                self.record_two_tex_pass(
                    encoder,
                    &format!("{}-hal-qtr-h", node.id.0),
                    &self.pipelines.fx_film_halation,
                    &hal_qtr_a.view,
                    &self.dummy_zero_view,
                    &FilmHalationUniform::blur(qtr_texel, 1.35, 0.0),
                    &hal_qtr_b.view,
                );
                self.record_two_tex_pass(
                    encoder,
                    &format!("{}-hal-qtr-v", node.id.0),
                    &self.pipelines.fx_film_halation,
                    &hal_qtr_b.view,
                    &self.dummy_zero_view,
                    &FilmHalationUniform::blur(qtr_texel, 0.0, 1.35),
                    &hal_qtr_a.view,
                );

                // Merge tight + wide halation
                self.record_two_tex_pass(
                    encoder,
                    &format!("{}-hal-merge", node.id.0),
                    &self.pipelines.fx_film_halation,
                    &hal_half_a.view,
                    &hal_qtr_a.view,
                    &FilmHalationUniform::combine(),
                    &hal_merged.view,
                );

                // Pass C: Composite lens + halation + 35mm organic grain + vignette
                self.record_two_tex_pass(
                    encoder,
                    &node.id.0,
                    &self.pipelines.fx_film_optics_composite,
                    lens_view,
                    &hal_merged.view,
                    &uniform,
                    &out.view,
                );

                self.pool.release(hal_half_a);
                self.pool.release(hal_half_b);
                self.pool.release(hal_qtr_a);
                self.pool.release(hal_qtr_b);
                self.pool.release(hal_merged);
                if let Some(lt) = lens_tex_opt {
                    self.pool.release(lt);
                }
                Ok(out)
            }
            NodeKind::CustomWgsl {
                wgsl_source,
                expected_inputs,
                params,
            } => {
                let program = self.pipelines.compile_custom_wgsl(
                    &self.ctx.device,
                    wgsl_source,
                    *expected_inputs,
                )?;
                let time_sec = (sub_tick.0 as f64 / FLICKS_PER_SECOND as f64) as f32;
                let uniform =
                    CustomWgslUniform::new(width, height, time_sec, sub_tick.0 as f32, params);
                let out = self.pool.acquire(&self.ctx.device, width, height);
                match *expected_inputs {
                    0 => {
                        self.record_zero_tex_pass(
                            encoder,
                            &node.id.0,
                            &program,
                            &uniform,
                            &out.view,
                        );
                    }
                    1 => {
                        let tex0 = require_input(node, textures, 0)?;
                        self.record_one_tex_pass(
                            encoder,
                            &node.id.0,
                            &program,
                            &tex0.view,
                            &uniform,
                            &out.view,
                        );
                    }
                    _ => {
                        let tex0 = require_input(node, textures, 0)?;
                        let tex1 = require_input(node, textures, 1)?;
                        self.record_two_tex_pass(
                            encoder,
                            &node.id.0,
                            &program,
                            &tex0.view,
                            &tex1.view,
                            &uniform,
                            &out.view,
                        );
                    }
                }
                Ok(out)
            }
            NodeKind::Transform3D {
                pos,
                rot_deg,
                scale,
                fov_deg,
                camera_pos,
                camera_rot_deg,
                dof_focal_dist,
                dof_blur_strength,
            } => {
                let src = require_input(node, textures, 0)?;
                let out = self.pool.acquire(&self.ctx.device, width, height);
                let cam =
                    build_camera_matrices(*camera_pos, *camera_rot_deg, *fov_deg, width, height);
                let uniform = build_transform3d_uniform(
                    &cam,
                    *pos,
                    *rot_deg,
                    *scale,
                    *dof_focal_dist,
                    *dof_blur_strength,
                    width,
                    height,
                );
                self.record_transform3d_pass(encoder, &node.id.0, &src.view, &uniform, &out.view);
                Ok(out)
            }
            NodeKind::Text3D {
                text,
                font_path,
                font_size,
                extrude_depth,
                bevel_size,
                outline_width,
                camera_pos,
                camera_rot_deg,
                fov_deg,
                echo_count,
                opacity,
                align,
                line_height_em,
                tracking_em,
                bevel_profile,
                bevel_segments,
                fallback_fonts,
                var_wght,
                var_wdth,
                var_slnt,
                ..
            } => {
                let out = self.pool.acquire(&self.ctx.device, width, height);
                if *opacity <= 0.001 {
                    clear_texture(encoder, &out.view, [0.0, 0.0, 0.0, 0.0]);
                    return Ok(out);
                }
                let mesh_key = self.ensure_text3d_mesh_cached(
                    font_path,
                    text,
                    *font_size,
                    *extrude_depth,
                    *bevel_size,
                    *outline_width,
                    *bevel_profile,
                    *bevel_segments,
                    *align,
                    *tracking_em,
                    *line_height_em,
                    fallback_fonts,
                    VariationAxes {
                        wght: *var_wght,
                        wdth: *var_wdth,
                        slnt: *var_slnt,
                    },
                )?;
                self.ensure_depth_texture_cached(width, height);
                let cached = &self.text3d_mesh_cache[&mesh_key];
                if cached.index_count == 0 {
                    clear_texture(encoder, &out.view, [0.0, 0.0, 0.0, 0.0]);
                    return Ok(out);
                }
                let cam =
                    build_camera_matrices(*camera_pos, *camera_rot_deg, *fov_deg, width, height);
                let t_sec = (sub_tick.0 as f64 / FLICKS_PER_SECOND as f64) as f32;
                let uniform = build_text3d_uniform(kind, &cam, t_sec);
                let echo_cnt = (*echo_count).min(16);
                let depth_view = &self.depth_cache[&(width, height)].1;
                self.record_mesh3d_pass(
                    encoder,
                    &node.id.0,
                    cached,
                    &uniform,
                    echo_cnt,
                    *opacity,
                    &out.view,
                    depth_view,
                );
                Ok(out)
            }
            NodeKind::Shape3D {
                shape_kind,
                size,
                aspect,
                stroke_width,
                corner_radius,
                inner_ratio,
                extrude_depth,
                bevel_size,
                camera_pos,
                camera_rot_deg,
                fov_deg,
                echo_count,
                opacity,
                ..
            } => {
                let out = self.pool.acquire(&self.ctx.device, width, height);
                if *opacity <= 0.001 {
                    clear_texture(encoder, &out.view, [0.0, 0.0, 0.0, 0.0]);
                    return Ok(out);
                }
                let mesh_key = self.ensure_shape3d_mesh_cached(
                    *shape_kind,
                    *size,
                    *aspect,
                    *stroke_width,
                    *corner_radius,
                    *inner_ratio,
                    *extrude_depth,
                    *bevel_size,
                )?;
                self.ensure_depth_texture_cached(width, height);
                let cached = &self.shape3d_mesh_cache[&mesh_key];
                if cached.index_count == 0 {
                    clear_texture(encoder, &out.view, [0.0, 0.0, 0.0, 0.0]);
                    return Ok(out);
                }
                let cam =
                    build_camera_matrices(*camera_pos, *camera_rot_deg, *fov_deg, width, height);
                let t_sec = (sub_tick.0 as f64 / FLICKS_PER_SECOND as f64) as f32;
                let uniform = build_shape3d_uniform(kind, &cam, t_sec);
                let echo_cnt = (*echo_count).min(16);
                let depth_view = &self.depth_cache[&(width, height)].1;
                self.record_mesh3d_pass(
                    encoder,
                    &node.id.0,
                    cached,
                    &uniform,
                    echo_cnt,
                    *opacity,
                    &out.view,
                    depth_view,
                );
                Ok(out)
            }
            NodeKind::ParticleField3D {
                count,
                camera_pos,
                camera_rot_deg,
                fov_deg,
                opacity,
                ..
            } => {
                let out = self.pool.acquire(&self.ctx.device, width, height);
                if *count == 0 || *opacity <= 0.001 {
                    clear_texture(encoder, &out.view, [0.0, 0.0, 0.0, 0.0]);
                    return Ok(out);
                }
                let cam =
                    build_camera_matrices(*camera_pos, *camera_rot_deg, *fov_deg, width, height);
                let t_sec = (sub_tick.0 as f64 / FLICKS_PER_SECOND as f64) as f32;
                let uniform = build_particle_field3d_uniform(kind, &cam, t_sec, None);
                self.record_uniform_draw_pass(
                    encoder,
                    &node.id.0,
                    &self.pipelines.particle_field3d,
                    &uniform,
                    0..6,
                    0..*count,
                    &out.view,
                );
                Ok(out)
            }
            NodeKind::RibbonTrail3D {
                ribbon_count,
                segments,
                camera_pos,
                camera_rot_deg,
                fov_deg,
                opacity,
                normal_blend,
                ..
            } => {
                let out = self.pool.acquire(&self.ctx.device, width, height);
                if *ribbon_count == 0 || *segments == 0 || *opacity <= 0.001 {
                    clear_texture(encoder, &out.view, [0.0, 0.0, 0.0, 0.0]);
                    return Ok(out);
                }
                let cam =
                    build_camera_matrices(*camera_pos, *camera_rot_deg, *fov_deg, width, height);
                let t_sec = (sub_tick.0 as f64 / FLICKS_PER_SECOND as f64) as f32;
                let segs = (*segments).max(1);
                let uniform =
                    build_ribbon_trail3d_uniform(kind, &cam, t_sec, None, width, height);
                // Standalone ribbons have no depth attachment, so normal blending only
                // changes how the trail composites over transparent black.
                let program = if *normal_blend {
                    &self.pipelines.ribbon_trail3d_normal
                } else {
                    &self.pipelines.ribbon_trail3d
                };
                self.record_uniform_draw_pass(
                    encoder,
                    &node.id.0,
                    program,
                    &uniform,
                    0..(segs * 2 + 2),
                    0..*ribbon_count,
                    &out.view,
                );
                Ok(out)
            }
            NodeKind::CornerPin {
                tl,
                tr,
                br,
                bl,
                filter,
                opacity,
            } => {
                let src = require_input(node, textures, 0)?;
                let out = self.pool.acquire(&self.ctx.device, width, height);
                let uniform =
                    CornerPinUniform::from_corners(*tl, *tr, *br, *bl, *filter, *opacity);
                if *filter == frameshift_schema::SampleFilter::Bilinear
                    && let Some(mip_src) = self.build_mipmapped_copy(encoder, src)
                {
                    self.record_one_tex_pass(
                        encoder,
                        &node.id.0,
                        &self.pipelines.corner_pin,
                        &mip_src.view,
                        &uniform,
                        &out.view,
                    );
                    self.pool.release(mip_src);
                } else {
                    self.record_one_tex_pass(
                        encoder,
                        &node.id.0,
                        &self.pipelines.corner_pin,
                        &src.view,
                        &uniform,
                        &out.view,
                    );
                }
                Ok(out)
            }
            NodeKind::GridWarp {
                cols,
                rows,
                control_offsets,
                wave_amp,
                wave_freq,
                wave_phase,
                mix_amount,
                filter,
                opacity,
            } => {
                let src = require_input(node, textures, 0)?;
                let out = self.pool.acquire(&self.ctx.device, width, height);
                let uniform = GridWarpUniform::from_params(
                    *cols,
                    *rows,
                    control_offsets,
                    *wave_amp,
                    *wave_freq,
                    *wave_phase,
                    *mix_amount,
                    *filter,
                    *opacity,
                );
                if *filter == frameshift_schema::SampleFilter::Bilinear
                    && let Some(mip_src) = self.build_mipmapped_copy(encoder, src)
                {
                    self.record_one_tex_pass(
                        encoder,
                        &node.id.0,
                        &self.pipelines.grid_warp,
                        &mip_src.view,
                        &uniform,
                        &out.view,
                    );
                    self.pool.release(mip_src);
                } else {
                    self.record_one_tex_pass(
                        encoder,
                        &node.id.0,
                        &self.pipelines.grid_warp,
                        &src.view,
                        &uniform,
                        &out.view,
                    );
                }
                Ok(out)
            }
            NodeKind::BackdropScrim {
                rect_center_norm,
                rect_size_norm,
                corner_radius_norm,
                blur_radius_norm,
                fill_rgba,
                opacity,
                auto_contrast,
            } => {
                let src = require_input(node, textures, 0)?;
                let out = self.pool.acquire(&self.ctx.device, width, height);
                let aspect = width as f32 / height.max(1) as f32;
                let uniform = BackdropScrimUniform::new(
                    *rect_center_norm,
                    *rect_size_norm,
                    *corner_radius_norm,
                    *blur_radius_norm,
                    *fill_rgba,
                    *opacity,
                    *auto_contrast,
                    aspect,
                );

                // No blur radius: a single full-resolution composite pass over the sharp source.
                if uniform.shape[1] <= 1e-6 {
                    self.record_two_tex_pass(
                        encoder,
                        &node.id.0,
                        &self.pipelines.backdrop_scrim_composite,
                        &src.view,
                        &src.view,
                        &uniform,
                        &out.view,
                    );
                    return Ok(out);
                }

                // Half-resolution separable blur, then full-resolution composite.
                let hw = (width / 2).max(1);
                let hh = (height / 2).max(1);
                let blur_h = self.pool.acquire(&self.ctx.device, hw, hh);
                let blur_v = self.pool.acquire(&self.ctx.device, hw, hh);
                self.record_one_tex_pass(
                    encoder,
                    &format!("{}-blur-h", node.id.0),
                    &self.pipelines.backdrop_scrim_blur,
                    &src.view,
                    &uniform.with_blur_direction(1.0, 0.0),
                    &blur_h.view,
                );
                self.record_one_tex_pass(
                    encoder,
                    &format!("{}-blur-v", node.id.0),
                    &self.pipelines.backdrop_scrim_blur,
                    &blur_h.view,
                    &uniform.with_blur_direction(0.0, 1.0),
                    &blur_v.view,
                );
                self.record_two_tex_pass(
                    encoder,
                    &node.id.0,
                    &self.pipelines.backdrop_scrim_composite,
                    &src.view,
                    &blur_v.view,
                    &uniform,
                    &out.view,
                );
                self.pool.release(blur_h);
                self.pool.release(blur_v);
                Ok(out)
            }
            NodeKind::MotionBlurShutter { .. }
            | NodeKind::Scene3D { .. }
            | NodeKind::Precomp { .. } => {
                unreachable!("handled in evaluate_subgraph_inner")
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn evaluate_scene3d(
        &mut self,
        graph: &RenderGraphDef,
        node: &GraphNodeDef,
        camera: &Camera3DDef,
        background_rgba: [f32; 4],
        sub_tick: Flicks,
        width: u32,
        height: u32,
        animator: &dyn NodeAnimator,
        textures: &HashMap<NodeId, PooledTexture>,
        encoder: &mut wgpu::CommandEncoder,
    ) -> Result<PooledTexture, RenderError> {
        enum MeshKind3D {
            Text(Text3DMeshKey),
            Shape(Shape3DMeshKey),
        }

        enum PhaseADraw {
            Mesh {
                mesh: MeshKind3D,
                bind_group: wgpu::BindGroup,
            },
            TransformCard {
                bind_group: wgpu::BindGroup,
            },
        }

        struct PhaseEchoDraw {
            mesh: MeshKind3D,
            bind_group: wgpu::BindGroup,
            instance_count: u32,
        }

        enum PhaseBDraw {
            Ribbon {
                bind_group: wgpu::BindGroup,
                vertex_count: u32,
                instance_count: u32,
                normal_blend: bool,
            },
            Particles {
                bind_group: wgpu::BindGroup,
                instance_count: u32,
            },
        }

        let cam = build_scene_camera_matrices(camera, width, height);
        let t_sec = (sub_tick.0 as f64 / FLICKS_PER_SECOND as f64) as f32;
        let dof_enabled = camera.dof_blur_strength > 1e-4 && camera.dof_max_radius_px > 0.5;
        let focal_dist = if camera.dof_autofocus_target && camera.mode == Camera3DMode::LookAt {
            let target_v = glam::Vec3::from_array(camera.target);
            let pos_v = glam::Vec3::from_array(camera.pos);
            target_v.distance(pos_v).max(cam.near)
        } else {
            camera.dof_focal_dist.max(cam.near)
        };
        let scene_dof_override = dof_enabled.then_some((focal_dist, camera.dof_blur_strength));

        let mut phase_a: Vec<PhaseADraw> = Vec::new();
        let mut phase_echo: Vec<PhaseEchoDraw> = Vec::new();
        let mut phase_b: Vec<PhaseBDraw> = Vec::new();

        for child_id in &node.inputs {
            let child_node = graph.node(child_id).ok_or_else(|| {
                RenderError::Schema(frameshift_schema::SchemaError::MissingNode(
                    child_id.clone(),
                ))
            })?;
            if !child_node.enabled {
                continue;
            }
            let child_kind = animator.evaluate_node(child_id, &child_node.kind, sub_tick)?;
            match &child_kind {
                NodeKind::Text3D {
                    text,
                    font_path,
                    font_size,
                    extrude_depth,
                    bevel_size,
                    outline_width,
                    echo_count,
                    opacity,
                    align,
                    line_height_em,
                    tracking_em,
                    bevel_profile,
                    bevel_segments,
                    fallback_fonts,
                    var_wght,
                    var_wdth,
                    var_slnt,
                    ..
                } => {
                    if *opacity <= 0.001 {
                        continue;
                    }
                    let key = self.ensure_text3d_mesh_cached(
                        font_path,
                        text,
                        *font_size,
                        *extrude_depth,
                        *bevel_size,
                        *outline_width,
                        *bevel_profile,
                        *bevel_segments,
                        *align,
                        *tracking_em,
                        *line_height_em,
                        fallback_fonts,
                        VariationAxes {
                            wght: *var_wght,
                            wdth: *var_wdth,
                            slnt: *var_slnt,
                        },
                    )?;
                    if self.text3d_mesh_cache[&key].index_count == 0 {
                        continue;
                    }
                    let uniform = build_text3d_uniform(&child_kind, &cam, t_sec);
                    let hero_bg = create_uniform_bind_group(
                        &self.ctx.device,
                        &child_id.0,
                        &self.pipelines.text3d_mesh.bind_group_layout,
                        &uniform,
                    );
                    if *opacity >= 0.999 {
                        phase_a.push(PhaseADraw::Mesh {
                            mesh: MeshKind3D::Text(key.clone()),
                            bind_group: hero_bg,
                        });
                    } else {
                        phase_echo.push(PhaseEchoDraw {
                            mesh: MeshKind3D::Text(key.clone()),
                            bind_group: hero_bg,
                            instance_count: 1,
                        });
                    }
                    let echo_cnt = (*echo_count).min(16);
                    if echo_cnt > 0 {
                        let mut echo_uniform = uniform;
                        echo_uniform.pbr_params[3] = 1.0;
                        let echo_bg = create_uniform_bind_group(
                            &self.ctx.device,
                            &format!("{}-echo", child_id.0),
                            &self.pipelines.text3d_mesh_echo.bind_group_layout,
                            &echo_uniform,
                        );
                        phase_echo.push(PhaseEchoDraw {
                            mesh: MeshKind3D::Text(key),
                            bind_group: echo_bg,
                            instance_count: echo_cnt,
                        });
                    }
                }
                NodeKind::Shape3D {
                    shape_kind,
                    size,
                    aspect,
                    stroke_width,
                    corner_radius,
                    inner_ratio,
                    extrude_depth,
                    bevel_size,
                    echo_count,
                    opacity,
                    ..
                } => {
                    if *opacity <= 0.001 {
                        continue;
                    }
                    let key = self.ensure_shape3d_mesh_cached(
                        *shape_kind,
                        *size,
                        *aspect,
                        *stroke_width,
                        *corner_radius,
                        *inner_ratio,
                        *extrude_depth,
                        *bevel_size,
                    )?;
                    if self.shape3d_mesh_cache[&key].index_count == 0 {
                        continue;
                    }
                    let uniform = build_shape3d_uniform(&child_kind, &cam, t_sec);
                    let hero_bg = create_uniform_bind_group(
                        &self.ctx.device,
                        &child_id.0,
                        &self.pipelines.text3d_mesh.bind_group_layout,
                        &uniform,
                    );
                    if *opacity >= 0.999 {
                        phase_a.push(PhaseADraw::Mesh {
                            mesh: MeshKind3D::Shape(key),
                            bind_group: hero_bg,
                        });
                    } else {
                        phase_echo.push(PhaseEchoDraw {
                            mesh: MeshKind3D::Shape(key),
                            bind_group: hero_bg,
                            instance_count: 1,
                        });
                    }
                    let echo_cnt = (*echo_count).min(16);
                    if echo_cnt > 0 {
                        let mut echo_uniform = uniform;
                        echo_uniform.pbr_params[3] = 1.0;
                        let echo_bg = create_uniform_bind_group(
                            &self.ctx.device,
                            &format!("{}-echo", child_id.0),
                            &self.pipelines.text3d_mesh_echo.bind_group_layout,
                            &echo_uniform,
                        );
                        phase_echo.push(PhaseEchoDraw {
                            mesh: MeshKind3D::Shape(key),
                            bind_group: echo_bg,
                            instance_count: echo_cnt,
                        });
                    }
                }
                NodeKind::Transform3D {
                    pos,
                    rot_deg,
                    scale,
                    dof_focal_dist,
                    dof_blur_strength,
                    ..
                } => {
                    let src_tex = require_input(child_node, textures, 0)?;
                    let uniform = build_transform3d_uniform(
                        &cam,
                        *pos,
                        *rot_deg,
                        *scale,
                        *dof_focal_dist,
                        *dof_blur_strength,
                        width,
                        height,
                    );
                    let uniform_buf =
                        self.ctx
                            .device
                            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                                label: Some(&child_id.0),
                                contents: bytemuck::bytes_of(&uniform),
                                usage: wgpu::BufferUsages::UNIFORM,
                            });
                    let bind_group =
                        self.ctx
                            .device
                            .create_bind_group(&wgpu::BindGroupDescriptor {
                                label: Some(&child_id.0),
                                layout: &self.pipelines.transform3d_scene.bind_group_layout,
                                entries: &[
                                    wgpu::BindGroupEntry {
                                        binding: 0,
                                        resource: wgpu::BindingResource::TextureView(&src_tex.view),
                                    },
                                    wgpu::BindGroupEntry {
                                        binding: 1,
                                        resource: wgpu::BindingResource::Sampler(
                                            &self.ctx.linear_sampler,
                                        ),
                                    },
                                    wgpu::BindGroupEntry {
                                        binding: 2,
                                        resource: uniform_buf.as_entire_binding(),
                                    },
                                ],
                            });
                    phase_a.push(PhaseADraw::TransformCard { bind_group });
                }
                NodeKind::RibbonTrail3D {
                    ribbon_count,
                    segments,
                    opacity,
                    normal_blend,
                    ..
                } => {
                    if *ribbon_count == 0 || *segments == 0 || *opacity <= 0.001 {
                        continue;
                    }
                    let segs = (*segments).max(1);
                    let uniform = build_ribbon_trail3d_uniform(
                        &child_kind,
                        &cam,
                        t_sec,
                        scene_dof_override,
                        width,
                        height,
                    );
                    let bind_group = create_uniform_bind_group(
                        &self.ctx.device,
                        &child_id.0,
                        &self.pipelines.ribbon_trail3d_scene.bind_group_layout,
                        &uniform,
                    );
                    phase_b.push(PhaseBDraw::Ribbon {
                        bind_group,
                        vertex_count: segs * 2 + 2,
                        instance_count: *ribbon_count,
                        normal_blend: *normal_blend,
                    });
                }
                NodeKind::ParticleField3D { count, opacity, .. } => {
                    if *count == 0 || *opacity <= 0.001 {
                        continue;
                    }
                    let uniform = build_particle_field3d_uniform(
                        &child_kind,
                        &cam,
                        t_sec,
                        scene_dof_override,
                    );
                    let bind_group = create_uniform_bind_group(
                        &self.ctx.device,
                        &child_id.0,
                        &self.pipelines.particle_field3d_scene.bind_group_layout,
                        &uniform,
                    );
                    phase_b.push(PhaseBDraw::Particles {
                        bind_group,
                        instance_count: *count,
                    });
                }
                other => {
                    return Err(RenderError::Schema(
                        frameshift_schema::SchemaError::InvalidScene3D {
                            node_id: node.id.clone(),
                            message: format!(
                                "unsupported Scene3D child node kind for '{child_id}': {other:?}"
                            ),
                        },
                    ));
                }
            }
        }

        let bg_clear = wgpu::Color {
            r: background_rgba[0] as f64,
            g: background_rgba[1] as f64,
            b: background_rgba[2] as f64,
            a: background_rgba[3] as f64,
        };

        let use_msaa = self.pipelines.scene3d_msaa4x.is_some();
        if !use_msaa {
            self.ensure_depth_texture_cached(width, height);
        }
        let color_tex = self.pool.acquire(&self.ctx.device, width, height);

        let text_cache = &self.text3d_mesh_cache;
        let shape_cache = &self.shape3d_mesh_cache;
        let draw_opaque_and_echo =
            |pass: &mut wgpu::RenderPass<'_>,
             text_pipe: &wgpu::RenderPipeline,
             echo_pipe: &wgpu::RenderPipeline,
             xform_pipe: &wgpu::RenderPipeline| {
                for item in &phase_a {
                    match item {
                        PhaseADraw::Mesh { mesh, bind_group } => {
                            let cached = match mesh {
                                MeshKind3D::Text(k) => &text_cache[k],
                                MeshKind3D::Shape(k) => &shape_cache[k],
                            };
                            let vb = cached
                                .vertex_buffer
                                .as_ref()
                                .expect("non-zero index_count guarantees vertex_buffer");
                            let ib = cached
                                .index_buffer
                                .as_ref()
                                .expect("non-zero index_count guarantees index_buffer");
                            pass.set_pipeline(text_pipe);
                            pass.set_bind_group(0, bind_group, &[]);
                            pass.set_vertex_buffer(0, vb.slice(..));
                            pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                            pass.draw_indexed(0..cached.index_count, 0, 0..1);
                        }
                        PhaseADraw::TransformCard { bind_group } => {
                            pass.set_pipeline(xform_pipe);
                            pass.set_bind_group(0, bind_group, &[]);
                            pass.draw(0..6, 0..1);
                        }
                    }
                }
                for item in &phase_echo {
                    let cached = match &item.mesh {
                        MeshKind3D::Text(k) => &text_cache[k],
                        MeshKind3D::Shape(k) => &shape_cache[k],
                    };
                    let vb = cached
                        .vertex_buffer
                        .as_ref()
                        .expect("non-zero index_count guarantees vertex_buffer");
                    let ib = cached
                        .index_buffer
                        .as_ref()
                        .expect("non-zero index_count guarantees index_buffer");
                    pass.set_pipeline(echo_pipe);
                    pass.set_bind_group(0, &item.bind_group, &[]);
                    pass.set_vertex_buffer(0, vb.slice(..));
                    pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..cached.index_count, 0, 0..item.instance_count);
                }
            };

        let draw_phase_b = |pass: &mut wgpu::RenderPass<'_>,
                            ribbon_pipe: &wgpu::RenderPipeline,
                            ribbon_normal_pipe: &wgpu::RenderPipeline,
                            particle_pipe: &wgpu::RenderPipeline| {
            // Additive draws first: they are order-independent, so the accumulated
            // neon glow is stable. Normal-blend ribbons are drawn afterwards so their
            // authored color composites over that glow instead of being hidden by it.
            for item in &phase_b {
                match item {
                    PhaseBDraw::Ribbon {
                        bind_group,
                        vertex_count,
                        instance_count,
                        normal_blend: false,
                    } => {
                        pass.set_pipeline(ribbon_pipe);
                        pass.set_bind_group(0, bind_group, &[]);
                        pass.draw(0..*vertex_count, 0..*instance_count);
                    }
                    PhaseBDraw::Ribbon {
                        normal_blend: true, ..
                    } => {}
                    PhaseBDraw::Particles {
                        bind_group,
                        instance_count,
                    } => {
                        pass.set_pipeline(particle_pipe);
                        pass.set_bind_group(0, bind_group, &[]);
                        pass.draw(0..6, 0..*instance_count);
                    }
                }
            }
            for item in &phase_b {
                if let PhaseBDraw::Ribbon {
                    bind_group,
                    vertex_count,
                    instance_count,
                    normal_blend: true,
                } = item
                {
                    pass.set_pipeline(ribbon_normal_pipe);
                    pass.set_bind_group(0, bind_group, &[]);
                    pass.draw(0..*vertex_count, 0..*instance_count);
                }
            }
        };

        let (atmos_tex_opt, depth_view_ref): (Option<PooledTexture>, &wgpu::TextureView) =
            if let Some(msaa_pipes) = &self.pipelines.scene3d_msaa4x {
                let (msaa_color_view, msaa_depth_view) =
                    self.msaa_cache.ensure_pair(&self.ctx.device, width, height, 4);
                {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some(&node.id.0),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: msaa_color_view,
                            depth_slice: None,
                            resolve_target: Some(&color_tex.view),
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(bg_clear),
                                store: wgpu::StoreOp::Discard,
                            },
                        })],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: msaa_depth_view,
                            depth_ops: Some(wgpu::Operations {
                                load: wgpu::LoadOp::Clear(1.0),
                                store: wgpu::StoreOp::Store,
                            }),
                            stencil_ops: None,
                        }),
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    draw_opaque_and_echo(
                        &mut pass,
                        &msaa_pipes.text3d_mesh.pipeline,
                        &msaa_pipes.text3d_mesh_echo.pipeline,
                        &msaa_pipes.transform3d_scene.pipeline,
                    );
                    if !dof_enabled {
                        draw_phase_b(
                            &mut pass,
                            &msaa_pipes.ribbon_trail3d_scene.pipeline,
                            &msaa_pipes.ribbon_trail3d_scene_normal.pipeline,
                            &msaa_pipes.particle_field3d_scene.pipeline,
                        );
                    }
                }
                let atmos_opt = if dof_enabled && !phase_b.is_empty() {
                    let atmos_tex = self.pool.acquire(&self.ctx.device, width, height);
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some(&format!("{}-atmos", node.id.0)),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: msaa_color_view,
                            depth_slice: None,
                            resolve_target: Some(&atmos_tex.view),
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                store: wgpu::StoreOp::Discard,
                            },
                        })],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: msaa_depth_view,
                            depth_ops: Some(wgpu::Operations {
                                load: wgpu::LoadOp::Load,
                                store: wgpu::StoreOp::Store,
                            }),
                            stencil_ops: None,
                        }),
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    draw_phase_b(
                        &mut pass,
                        &msaa_pipes.ribbon_trail3d_scene.pipeline,
                        &msaa_pipes.ribbon_trail3d_scene_normal.pipeline,
                        &msaa_pipes.particle_field3d_scene.pipeline,
                    );
                    Some(atmos_tex)
                } else {
                    None
                };
                (atmos_opt, msaa_depth_view)
            } else {
                let depth_view = &self.depth_cache[&(width, height)].1;
                {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some(&node.id.0),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &color_tex.view,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(bg_clear),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: depth_view,
                            depth_ops: Some(wgpu::Operations {
                                load: wgpu::LoadOp::Clear(1.0),
                                store: wgpu::StoreOp::Store,
                            }),
                            stencil_ops: None,
                        }),
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    draw_opaque_and_echo(
                        &mut pass,
                        &self.pipelines.text3d_mesh.pipeline,
                        &self.pipelines.text3d_mesh_echo.pipeline,
                        &self.pipelines.transform3d_scene.pipeline,
                    );
                    if !dof_enabled {
                        draw_phase_b(
                            &mut pass,
                            &self.pipelines.ribbon_trail3d_scene.pipeline,
                            &self.pipelines.ribbon_trail3d_scene_normal.pipeline,
                            &self.pipelines.particle_field3d_scene.pipeline,
                        );
                    }
                }
                let atmos_opt = if dof_enabled && !phase_b.is_empty() {
                    let atmos_tex = self.pool.acquire(&self.ctx.device, width, height);
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some(&format!("{}-atmos", node.id.0)),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &atmos_tex.view,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: depth_view,
                            depth_ops: Some(wgpu::Operations {
                                load: wgpu::LoadOp::Load,
                                store: wgpu::StoreOp::Store,
                            }),
                            stencil_ops: None,
                        }),
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    draw_phase_b(
                        &mut pass,
                        &self.pipelines.ribbon_trail3d_scene.pipeline,
                        &self.pipelines.ribbon_trail3d_scene_normal.pipeline,
                        &self.pipelines.particle_field3d_scene.pipeline,
                    );
                    Some(atmos_tex)
                } else {
                    None
                };
                (atmos_opt, depth_view)
            };

        if dof_enabled {
            let texel_x = 1.0 / width.max(1) as f32;
            let texel_y = 1.0 / height.max(1) as f32;
            let max_radius_px = camera.dof_max_radius_px.clamp(0.0, 48.0);

            let coc_h_tex = self.pool.acquire(&self.ctx.device, width, height);
            let coc_tex = self.pool.acquire(&self.ctx.device, width, height);
            let coc_uniform = DofCocUniform {
                depth_params: [cam.near, cam.far, focal_dist, camera.dof_blur_strength],
                screen_params: [max_radius_px, texel_x, texel_y, 0.0],
            };
            let coc_uniform_buf =
                self.ctx
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some(&format!("{}-dof-coc", node.id.0)),
                        contents: bytemuck::bytes_of(&coc_uniform),
                        usage: wgpu::BufferUsages::UNIFORM,
                    });

            let coc_h_program = self
                .pipelines
                .scene3d_msaa4x
                .as_ref()
                .map(|m| &m.fx_dof_coc_msaa_h)
                .unwrap_or(&self.pipelines.fx_dof_coc);
            let coc_h_bg = self.ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(&format!("{}-dof-coc-h", node.id.0)),
                layout: &coc_h_program.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&color_tex.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(depth_view_ref),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: coc_uniform_buf.as_entire_binding(),
                    },
                ],
            });
            draw_fullscreen_pass(
                encoder,
                &format!("{}-dof-coc-h", node.id.0),
                &coc_h_program.pipeline,
                &coc_h_bg,
                &coc_h_tex.view,
            );

            let coc_v_bg = self.ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(&format!("{}-dof-coc-v", node.id.0)),
                layout: &self.pipelines.fx_dof_coc_v.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&coc_h_tex.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: coc_uniform_buf.as_entire_binding(),
                    },
                ],
            });
            draw_fullscreen_pass(
                encoder,
                &format!("{}-dof-coc-v", node.id.0),
                &self.pipelines.fx_dof_coc_v.pipeline,
                &coc_v_bg,
                &coc_tex.view,
            );

            let out_tex = self.pool.acquire(&self.ctx.device, width, height);
            let anam_ratio = if camera.dof_anamorphic_ratio.is_finite()
                && camera.dof_anamorphic_ratio > 0.0
            {
                camera.dof_anamorphic_ratio.clamp(0.25, 4.0)
            } else {
                1.0
            };
            let bokeh_uniform = DofBokehUniform {
                params0: [
                    texel_x,
                    texel_y,
                    max_radius_px,
                    camera.dof_samples.clamp(8, 32) as f32,
                ],
                params1: [
                    camera.dof_highlight_boost,
                    0.35,
                    t_sec,
                    camera.dof_blade_roundness.clamp(0.0, 1.0),
                ],
                params2: [
                    camera.dof_aperture_blades as f32,
                    camera.dof_blade_rotation_deg.to_radians(),
                    anam_ratio,
                    camera.dof_cats_eye_strength.clamp(0.0, 1.0),
                ],
            };
            let bokeh_uniform_buf =
                self.ctx
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some(&format!("{}-dof-bokeh", node.id.0)),
                        contents: bytemuck::bytes_of(&bokeh_uniform),
                        usage: wgpu::BufferUsages::UNIFORM,
                    });
            let atmos_view = atmos_tex_opt
                .as_ref()
                .map(|t| &t.view)
                .unwrap_or(&self.dummy_zero_view);
            let bokeh_bg = self.ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(&format!("{}-dof-bokeh", node.id.0)),
                layout: &self.pipelines.fx_dof_bokeh.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&color_tex.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&coc_tex.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(atmos_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::Sampler(&self.ctx.linear_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: bokeh_uniform_buf.as_entire_binding(),
                    },
                ],
            });
            draw_fullscreen_pass(
                encoder,
                &format!("{}-dof-bokeh", node.id.0),
                &self.pipelines.fx_dof_bokeh.pipeline,
                &bokeh_bg,
                &out_tex.view,
            );

            self.pool.release(coc_h_tex);
            self.pool.release(coc_tex);
            self.pool.release(color_tex);
            if let Some(atmos_tex) = atmos_tex_opt {
                self.pool.release(atmos_tex);
            }
            return Ok(out_tex);
        }

        if let Some(atmos_tex) = atmos_tex_opt {
            self.pool.release(atmos_tex);
        }
        Ok(color_tex)
    }

    #[allow(clippy::too_many_arguments)]
    fn record_mesh3d_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        label: &str,
        cached: &CachedMesh3D,
        uniform: &Text3DUniform,
        echo_count: u32,
        opacity: f32,
        target_view: &wgpu::TextureView,
        depth_view: &wgpu::TextureView,
    ) {
        let vb = cached
            .vertex_buffer
            .as_ref()
            .expect("non-zero index_count guarantees vertex_buffer");
        let ib = cached
            .index_buffer
            .as_ref()
            .expect("non-zero index_count guarantees index_buffer");
        let hero_bg = create_uniform_bind_group(
            &self.ctx.device,
            label,
            &self.pipelines.text3d_mesh.bind_group_layout,
            uniform,
        );
        let echo_bg = (echo_count > 0).then(|| {
            let mut echo_u = *uniform;
            echo_u.pbr_params[3] = 1.0;
            create_uniform_bind_group(
                &self.ctx.device,
                &format!("{label}-echo"),
                &self.pipelines.text3d_mesh_echo.bind_group_layout,
                &echo_u,
            )
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let hero_pipe = if opacity >= 0.999 {
            &self.pipelines.text3d_mesh.pipeline
        } else {
            &self.pipelines.text3d_mesh_echo.pipeline
        };
        pass.set_pipeline(hero_pipe);
        pass.set_bind_group(0, &hero_bg, &[]);
        pass.set_vertex_buffer(0, vb.slice(..));
        pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..cached.index_count, 0, 0..1);

        if let Some(bg) = echo_bg.as_ref() {
            pass.set_pipeline(&self.pipelines.text3d_mesh_echo.pipeline);
            pass.set_bind_group(0, bg, &[]);
            pass.set_vertex_buffer(0, vb.slice(..));
            pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..cached.index_count, 0, 0..echo_count);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_vector_text_slug_gpu(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        node_label: &str,
        kind: &NodeKind,
        width: u32,
        height: u32,
    ) -> Result<PooledTexture, RenderError> {
        let NodeKind::VectorText {
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
        } = kind
        else {
            unreachable!("expected VectorText");
        };

        let out = self.pool.acquire(&self.ctx.device, width, height);
        let op = opacity.clamp(0.0, 1.0);
        if text.is_empty() || *font_size_norm <= 0.0 || op <= 0.0001 {
            clear_texture(encoder, &out.view, [0.0, 0.0, 0.0, 0.0]);
            return Ok(out);
        }

        let effective_align = align.unwrap_or(if *align_center {
            TextAlign2D::Center
        } else {
            TextAlign2D::Left
        });
        let w_f = width as f32;
        let h_f = height as f32;
        let font_px = (*font_size_norm) * h_f;
        if !font_px.is_finite() || font_px <= 0.0 {
            clear_texture(encoder, &out.view, [0.0, 0.0, 0.0, 0.0]);
            return Ok(out);
        }

        let align_tag = match effective_align {
            TextAlign2D::Left => 0u8,
            TextAlign2D::Center => 1u8,
            TextAlign2D::Right => 2u8,
        };
        let wm_tag = match writing_mode {
            WritingMode2D::HorizontalTb => 0u8,
            WritingMode2D::VerticalRl => 1u8,
        };
        let slug_key = SlugSceneCacheKey {
            text: text.clone(),
            font_family: font_family.clone(),
            font_path: font_path.clone(),
            font_weight: *font_weight,
            fallback_fonts: fallback_fonts.clone(),
            writing_mode: wm_tag,
            align: align_tag,
            font_px_bits: font_px.to_bits(),
            tracking_bits: tracking_em.map(f32::to_bits),
            line_height_bits: line_height_em.map(f32::to_bits),
            var_wght_bits: var_wght.map(f32::to_bits),
            var_wdth_bits: var_wdth.map(f32::to_bits),
            var_slnt_bits: var_slnt.map(f32::to_bits),
        };

        if !self.slug_cache.contains_key(&slug_key) {
            let query = FontQuery {
                font_path: font_path.clone(),
                font_family: font_family.clone(),
                font_weight: *font_weight,
            };
            let (primary, fallbacks) = resolve_font_chain(&query, fallback_fonts, None)?;
            let shaped = shape_text_block(
                text,
                &primary,
                &fallbacks,
                &TextLayoutOptions {
                    font_size: font_px,
                    align: effective_align,
                    writing_mode: *writing_mode,
                    tracking_em: tracking_em.unwrap_or(0.0),
                    line_height_em: *line_height_em,
                    variations: VariationAxes {
                        wght: *var_wght,
                        wdth: *var_wdth,
                        slnt: *var_slnt,
                    },
                    enable_ligatures: true,
                },
            )?;
            let scene = build_slug_scene(&shaped);
            let cached_slug = self.upload_slug_scene(&scene);
            self.slug_cache.insert(slug_key.clone(), cached_slug);
        }

        let cached = &self.slug_cache[&slug_key];
        if cached.index_count == 0 {
            clear_texture(encoder, &out.view, [0.0, 0.0, 0.0, 0.0]);
            return Ok(out);
        }

        let outline_px = outline_width_norm.max(0.0) * h_f;
        let outer_outline_px = outer_outline_width_norm.max(0.0) * h_f;
        let gap_px = outline_gap_norm.max(0.0) * h_f;
        let shadow_blur_px = shadow_blur_norm.max(0.0) * h_f;
        let glow_radius_px = glow_radius_norm.max(0.0) * h_f;

        let needs_fx = (outline_px > 0.05 && outline_rgba[3] > 0.0001)
            || (outer_outline_px > 0.05 && outer_outline_rgba[3] > 0.0001)
            || shadow_rgba[3] > 0.0001
            || (glow_radius_px > 0.05 && glow_rgba[3] > 0.0001);

        let anchor_x_px = x_offset_norm.unwrap_or(match effective_align {
            TextAlign2D::Left => 0.04,
            TextAlign2D::Center => 0.5,
            TextAlign2D::Right => 0.96,
        }) * w_f;
        let block_center_y_px = (*y_offset_norm) * h_f;
        let bottom_rgba = color_bottom_rgba.unwrap_or(*color_rgba);

        let slug_uniform = SlugUniform {
            canvas_anchor: [w_f, h_f, anchor_x_px, block_center_y_px],
            block_bounds: cached.block_bounds,
            color_top: *color_rgba,
            color_bottom: bottom_rgba,
            style_params: [
                gradient_angle_deg.to_radians(),
                hdr_intensity.max(0.0),
                if needs_fx { 1.0 } else { op },
                1.0,
            ],
        };

        let slug_bind_group = self.create_slug_bind_group(node_label, cached, &slug_uniform);

        if !needs_fx {
            self.record_slug_draw(encoder, node_label, cached, &slug_bind_group, &out.view);
            return Ok(out);
        }

        let fill_tex = self.pool.acquire(&self.ctx.device, width, height);
        self.record_slug_draw(
            encoder,
            node_label,
            cached,
            &slug_bind_group,
            &fill_tex.view,
        );

        let outline_total = if (outline_px > 0.05 && outline_rgba[3] > 0.0001)
            || (outer_outline_px > 0.05 && outer_outline_rgba[3] > 0.0001)
        {
            outline_px
                + if outer_outline_px > 0.05 && outer_outline_rgba[3] > 0.0001 {
                    gap_px + outer_outline_px
                } else {
                    0.0
                }
        } else {
            0.0
        };
        let shadow_reach = if shadow_rgba[3] > 0.0001 && shadow_blur_px > 0.5 {
            shadow_blur_px
        } else {
            0.0
        };
        let glow_reach = if glow_radius_px > 0.05 && glow_rgba[3] > 0.0001 {
            glow_radius_px * 1.4
        } else {
            0.0
        };
        let max_reach_px = outline_total
            .max(shadow_reach)
            .max(glow_reach)
            .ceil()
            .clamp(2.0, 128.0);
        let texel_x = 1.0 / w_f;
        let texel_y = 1.0 / h_f;

        let mut jfa_src = self.pool.acquire(&self.ctx.device, width, height);
        let seed_uniform = Text2DJfaUniform {
            params: [0.0, max_reach_px, texel_x, texel_y],
        };
        self.record_one_tex_pass(
            encoder,
            &format!("{node_label}-jfa-seed"),
            &self.pipelines.text2d_jfa_seed,
            &fill_tex.view,
            &seed_uniform,
            &jfa_src.view,
        );

        let mut jfa_dst = self.pool.acquire(&self.ctx.device, width, height);
        let mut step_px = ((max_reach_px as u32).next_power_of_two() / 2).max(1) as i32;
        while step_px >= 1 {
            let step_uniform = Text2DJfaUniform {
                params: [step_px as f32, max_reach_px, texel_x, texel_y],
            };
            self.record_one_tex_pass(
                encoder,
                &format!("{node_label}-jfa-{step_px}"),
                &self.pipelines.text2d_jfa_step,
                &jfa_src.view,
                &step_uniform,
                &jfa_dst.view,
            );
            std::mem::swap(&mut jfa_src, &mut jfa_dst);
            step_px /= 2;
        }

        let fx_uniform = Text2DFxUniform {
            metrics0: [texel_x, texel_y, outline_px, outer_outline_px],
            metrics1: [gap_px, shadow_blur_px, glow_radius_px, op],
            shadow_offset: [
                shadow_offset_norm[0] * h_f,
                shadow_offset_norm[1] * h_f,
                max_reach_px,
                0.0,
            ],
            outline_rgba: *outline_rgba,
            outer_outline_rgba: *outer_outline_rgba,
            shadow_rgba: *shadow_rgba,
            glow_rgba: *glow_rgba,
        };
        self.record_two_tex_pass(
            encoder,
            &format!("{node_label}-text2d-fx"),
            &self.pipelines.text2d_fx,
            &fill_tex.view,
            &jfa_src.view,
            &fx_uniform,
            &out.view,
        );
        self.pool.release(jfa_dst);
        self.pool.release(jfa_src);
        self.pool.release(fill_tex);
        Ok(out)
    }

    fn upload_slug_scene(&self, scene: &frameshift_vector::SlugSceneGpu) -> CachedSlugScene {
        if scene.is_empty() {
            return CachedSlugScene {
                vertex_buffer: None,
                index_buffer: None,
                glyphs_buffer: None,
                hbands_buffer: None,
                vbands_buffer: None,
                band_indices_buffer: None,
                curves_buffer: None,
                index_count: 0,
                block_bounds: scene.block_bounds,
            };
        }
        let gpu_verts: Vec<GpuSlugVertex> =
            scene.vertices.iter().map(GpuSlugVertex::from).collect();
        let gpu_glyphs: Vec<GpuSlugGlyph> = scene.glyphs.iter().map(GpuSlugGlyph::from).collect();
        let gpu_hbands: Vec<GpuSlugBand> = scene.hbands.iter().map(GpuSlugBand::from).collect();
        let gpu_vbands: Vec<GpuSlugBand> = scene.vbands.iter().map(GpuSlugBand::from).collect();
        let gpu_curves: Vec<GpuSlugCurve> = scene.curves.iter().map(GpuSlugCurve::from).collect();

        let make_storage = |label: &str, bytes: &[u8]| {
            self.ctx
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(label),
                    contents: bytes,
                    usage: wgpu::BufferUsages::STORAGE,
                })
        };

        CachedSlugScene {
            vertex_buffer: Some(self.ctx.device.create_buffer_init(
                &wgpu::util::BufferInitDescriptor {
                    label: Some("frameshift-slug-vb"),
                    contents: cast_slice(&gpu_verts),
                    usage: wgpu::BufferUsages::VERTEX,
                },
            )),
            index_buffer: Some(self.ctx.device.create_buffer_init(
                &wgpu::util::BufferInitDescriptor {
                    label: Some("frameshift-slug-ib"),
                    contents: cast_slice(&scene.indices),
                    usage: wgpu::BufferUsages::INDEX,
                },
            )),
            glyphs_buffer: Some(make_storage("frameshift-slug-glyphs", cast_slice(&gpu_glyphs))),
            hbands_buffer: Some(make_storage("frameshift-slug-hbands", cast_slice(&gpu_hbands))),
            vbands_buffer: Some(make_storage("frameshift-slug-vbands", cast_slice(&gpu_vbands))),
            band_indices_buffer: Some(make_storage(
                "frameshift-slug-band-indices",
                cast_slice(&scene.band_indices),
            )),
            curves_buffer: Some(make_storage("frameshift-slug-curves", cast_slice(&gpu_curves))),
            index_count: scene.indices.len() as u32,
            block_bounds: scene.block_bounds,
        }
    }

    fn create_slug_bind_group(
        &self,
        label: &str,
        cached: &CachedSlugScene,
        uniform: &SlugUniform,
    ) -> wgpu::BindGroup {
        let u_buf = self
            .ctx
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytemuck::bytes_of(uniform),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        self.ctx
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &self.pipelines.text_slug.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: u_buf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: cached.glyphs_buffer.as_ref().unwrap().as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: cached.hbands_buffer.as_ref().unwrap().as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: cached.vbands_buffer.as_ref().unwrap().as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: cached
                            .band_indices_buffer
                            .as_ref()
                            .unwrap()
                            .as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: cached.curves_buffer.as_ref().unwrap().as_entire_binding(),
                    },
                ],
            })
    }

    fn record_slug_draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        label: &str,
        cached: &CachedSlugScene,
        bind_group: &wgpu::BindGroup,
        target_view: &wgpu::TextureView,
    ) {
        let vb = cached.vertex_buffer.as_ref().unwrap();
        let ib = cached.index_buffer.as_ref().unwrap();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipelines.text_slug.pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.set_vertex_buffer(0, vb.slice(..));
        pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..cached.index_count, 0, 0..1);
    }

    #[allow(clippy::too_many_arguments)]
    fn ensure_text3d_mesh_cached(
        &mut self,
        font_path: &str,
        text: &str,
        font_size: f32,
        extrude_depth: f32,
        bevel_size: f32,
        outline_width: f32,
        bevel_profile: BevelProfile3D,
        bevel_segments: u32,
        align: TextAlign2D,
        tracking_em: f32,
        line_height_em: Option<f32>,
        fallback_fonts: &[String],
        variations: VariationAxes,
    ) -> Result<Text3DMeshKey, RenderError> {
        let profile_tag = match bevel_profile {
            BevelProfile3D::Flat => 0u8,
            BevelProfile3D::Round => 1u8,
            BevelProfile3D::Concave => 2u8,
            BevelProfile3D::Stepped => 3u8,
        };
        let align_tag = match align {
            TextAlign2D::Left => 0u8,
            TextAlign2D::Center => 1u8,
            TextAlign2D::Right => 2u8,
        };
        let key = Text3DMeshKey {
            font_path: font_path.to_owned(),
            text: text.to_owned(),
            font_size_bits: font_size.to_bits(),
            extrude_depth_bits: extrude_depth.to_bits(),
            bevel_size_bits: bevel_size.to_bits(),
            outline_width_bits: outline_width.to_bits(),
            bevel_profile: profile_tag,
            bevel_segments,
            align: align_tag,
            tracking_bits: tracking_em.to_bits(),
            line_height_bits: line_height_em.map(f32::to_bits),
            fallback_fonts: fallback_fonts.to_vec(),
            var_wght_bits: variations.wght.map(f32::to_bits),
            var_wdth_bits: variations.wdth.map(f32::to_bits),
            var_slnt_bits: variations.slnt.map(f32::to_bits),
        };
        if self.text3d_mesh_cache.contains_key(&key) {
            return Ok(key);
        }

        let font_bytes = if let Some(cached) = self.font_cache.get(font_path) {
            Arc::clone(cached)
        } else {
            let bytes = Arc::new(frameshift_vector::load_font_bytes(Path::new(font_path))?);
            self.font_cache
                .insert(font_path.to_owned(), Arc::clone(&bytes));
            bytes
        };

        let mut resolved_fallbacks: Vec<ResolvedFont> = Vec::with_capacity(fallback_fonts.len());
        for fb in fallback_fonts {
            let trimmed = fb.trim();
            let lower = trimmed.to_ascii_lowercase();
            let is_path = trimmed.contains('/')
                || trimmed.contains('\\')
                || lower.ends_with(".ttf")
                || lower.ends_with(".otf")
                || lower.ends_with(".ttc")
                || lower.ends_with(".otc");
            let q = if is_path {
                FontQuery {
                    font_path: Some(trimmed.to_string()),
                    font_family: None,
                    font_weight: 400,
                }
            } else {
                FontQuery {
                    font_path: None,
                    font_family: Some(trimmed.to_string()),
                    font_weight: 400,
                }
            };
            resolved_fallbacks.push(frameshift_vector::resolve_font(&q, None)?);
        }

        let mesh = frameshift_vector::extrude_text_to_mesh_ex(
            &font_bytes,
            text,
            &Text3DMeshOptions {
                font_size,
                extrude_depth,
                bevel_size,
                bevel_profile,
                bevel_segments,
                outline_width,
                align,
                tracking_em,
                line_height_em,
                variations,
                fallback_fonts: &resolved_fallbacks,
            },
        )?;
        let cached_mesh = self.upload_extruded_mesh(&mesh);
        self.text3d_mesh_cache.insert(key.clone(), cached_mesh);
        Ok(key)
    }

    #[allow(clippy::too_many_arguments)]
    fn ensure_shape3d_mesh_cached(
        &mut self,
        shape_kind: Shape3DKind,
        size: f32,
        aspect: f32,
        stroke_width: f32,
        corner_radius: f32,
        inner_ratio: f32,
        extrude_depth: f32,
        bevel_size: f32,
    ) -> Result<Shape3DMeshKey, RenderError> {
        let (kind_id, proc_kind) = match shape_kind {
            Shape3DKind::NeonTile => (0u8, ProceduralShapeKind::NeonTile),
            Shape3DKind::HollowStar5 => (1u8, ProceduralShapeKind::HollowStar5),
            Shape3DKind::Heart3D => (2u8, ProceduralShapeKind::Heart3D),
            Shape3DKind::LightningBolt => (3u8, ProceduralShapeKind::LightningBolt),
            Shape3DKind::Diamond3D => (4u8, ProceduralShapeKind::Diamond3D),
        };
        let key = (
            kind_id,
            size.to_bits(),
            aspect.to_bits(),
            stroke_width.to_bits(),
            corner_radius.to_bits(),
            inner_ratio.to_bits(),
            extrude_depth.to_bits(),
            bevel_size.to_bits(),
        );
        if self.shape3d_mesh_cache.contains_key(&key) {
            return Ok(key);
        }

        let mesh = frameshift_vector::build_shape3d_mesh(Shape3DParams {
            kind: proc_kind,
            size,
            aspect,
            stroke_width,
            corner_radius,
            inner_ratio,
            extrude_depth,
            bevel_size,
        })?;
        let cached_mesh = self.upload_extruded_mesh(&mesh);
        self.shape3d_mesh_cache.insert(key, cached_mesh);
        Ok(key)
    }

    fn upload_extruded_mesh(&self, mesh: &frameshift_vector::ExtrudedTextMesh) -> CachedMesh3D {
        if mesh.indices.is_empty() || mesh.vertices.is_empty() {
            return CachedMesh3D {
                vertex_buffer: None,
                index_buffer: None,
                index_count: 0,
            };
        }
        let gpu_verts: Vec<GpuMeshVertex3D> =
            mesh.vertices.iter().map(GpuMeshVertex3D::from).collect();
        let vb = self
            .ctx
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("frameshift-mesh3d-vb"),
                contents: cast_slice(&gpu_verts),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let ib = self
            .ctx
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("frameshift-mesh3d-ib"),
                contents: cast_slice(&mesh.indices),
                usage: wgpu::BufferUsages::INDEX,
            });
        CachedMesh3D {
            vertex_buffer: Some(vb),
            index_buffer: Some(ib),
            index_count: mesh.indices.len() as u32,
        }
    }

    fn ensure_depth_texture_cached(&mut self, width: u32, height: u32) {
        if self.depth_cache.contains_key(&(width, height)) {
            return;
        }
        let texture = self.ctx.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("frameshift-depth32f"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            aspect: wgpu::TextureAspect::DepthOnly,
            ..Default::default()
        });
        self.depth_cache.insert((width, height), (texture, view));
    }

    fn ensure_lut3d_cached(
        &mut self,
        key: &LutCacheKey,
        lut_source: Option<&LutSourceDef>,
        lut_preset: &str,
        input_space: Option<ExtendedColorSpace>,
        output_space: Option<ExtendedColorSpace>,
    ) -> Result<(), RenderError> {
        if self.lut_cache.contains_key(key) {
            return Ok(());
        }

        let lut = CubeLut3D::resolve_source(lut_source, lut_preset, input_space, output_space)?;
        let size = lut.size;
        let domain_min = lut.domain_min;
        let domain_inv_span = lut.domain_inv_span();

        let mut f16_texels: Vec<[u16; 4]> = Vec::with_capacity(lut.data_rgba16f_or_f32.len());
        for &[r, g, b, a] in &lut.data_rgba16f_or_f32 {
            f16_texels.push([
                f16::from_f32(r).to_bits(),
                f16::from_f32(g).to_bits(),
                f16::from_f32(b).to_bits(),
                f16::from_f32(a).to_bits(),
            ]);
        }

        let texture = self.ctx.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("frameshift-lut3d-rgba16f"),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: size,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            format: WORKING_TEXTURE_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        self.ctx.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            cast_slice(&f16_texels),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size * 8),
                rows_per_image: Some(size),
            },
            wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: size,
            },
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D3),
            ..Default::default()
        });

        self.lut_cache.insert(
            key.clone(),
            CachedLut3D {
                size,
                domain_min,
                domain_inv_span,
                _texture: texture,
                view,
            },
        );
        Ok(())
    }

    fn upload_source_frame(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        asset_id: &str,
        frame: &crate::provider::SourceFrame,
        color_space: ColorSpace,
        dst_view: &wgpu::TextureView,
    ) -> Result<(), RenderError> {
        validate_dimensions(frame.width, frame.height)?;
        let pixel_count = (frame.width as usize) * (frame.height as usize);
        let space_mode = match color_space {
            ColorSpace::DisplaySrgb => ColorSpaceMode::SrgbToLinear,
            ColorSpace::LinearRec709 => ColorSpaceMode::LinearToLinear,
        };

        let staging_tex = match &frame.pixels {
            SourcePixelData::Rgba8 {
                data,
                premultiplied,
            } => {
                let expected = pixel_count * 4;
                if data.len() != expected {
                    return Err(RenderError::InvalidFrameBuffer {
                        asset_id: asset_id.to_owned(),
                        expected,
                        actual: data.len(),
                    });
                }

                let tex = self.ctx.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("frameshift-src-rgba8"),
                    size: wgpu::Extent3d {
                        width: frame.width,
                        height: frame.height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });

                let upload_buf;
                let upload_slice: &[u8] = if *premultiplied {
                    data.as_slice()
                } else {
                    upload_buf = premultiply_rgba8(data);
                    upload_buf.as_slice()
                };

                self.ctx.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &tex,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    upload_slice,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(frame.width * 4),
                        rows_per_image: Some(frame.height),
                    },
                    wgpu::Extent3d {
                        width: frame.width,
                        height: frame.height,
                        depth_or_array_layers: 1,
                    },
                );
                tex
            }
            SourcePixelData::RgbaF32 {
                data,
                premultiplied,
            } => {
                if data.len() != pixel_count {
                    return Err(RenderError::InvalidFrameBuffer {
                        asset_id: asset_id.to_owned(),
                        expected: pixel_count,
                        actual: data.len(),
                    });
                }

                let mut f16_pixels: Vec<[u16; 4]> = Vec::with_capacity(pixel_count);
                for &[r, g, b, a] in data {
                    let (pr, pg, pb) = if *premultiplied {
                        (r, g, b)
                    } else {
                        (r * a, g * a, b * a)
                    };
                    f16_pixels.push([
                        f16::from_f32(pr).to_bits(),
                        f16::from_f32(pg).to_bits(),
                        f16::from_f32(pb).to_bits(),
                        f16::from_f32(a).to_bits(),
                    ]);
                }

                let tex = self.ctx.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("frameshift-src-rgba16f"),
                    size: wgpu::Extent3d {
                        width: frame.width,
                        height: frame.height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: WORKING_TEXTURE_FORMAT,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });

                self.ctx.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &tex,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    cast_slice(&f16_pixels),
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(frame.width * 8),
                        rows_per_image: Some(frame.height),
                    },
                    wgpu::Extent3d {
                        width: frame.width,
                        height: frame.height,
                        depth_or_array_layers: 1,
                    },
                );
                tex
            }
        };

        let staging_view = staging_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let uniform = ColorBasicUniform::identity(space_mode);
        self.record_one_tex_pass(
            encoder,
            "source-upload-convert",
            &self.pipelines.color_basic,
            &staging_view,
            &uniform,
            dst_view,
        );
        Ok(())
    }

    fn build_mipmapped_copy(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        src: &PooledTexture,
    ) -> Option<PooledTexture> {
        let mips = compute_mip_levels(src.width, src.height);
        if mips <= 1 {
            return None;
        }
        let mip_tex = self
            .pool
            .acquire_mipmapped(&self.ctx.device, src.width, src.height);
        copy_texture_2d(encoder, &src.texture, &mip_tex.texture, src.width, src.height);
        self.generate_mipmaps(encoder, &mip_tex.texture, mip_tex.mip_level_count);
        Some(mip_tex)
    }

    fn generate_mipmaps(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        texture: &wgpu::Texture,
        mip_level_count: u32,
    ) {
        for level in 0..mip_level_count.saturating_sub(1) {
            let src_view = texture.create_view(&wgpu::TextureViewDescriptor {
                label: Some("frameshift-mip-src"),
                base_mip_level: level,
                mip_level_count: Some(1),
                ..Default::default()
            });
            let dst_view = texture.create_view(&wgpu::TextureViewDescriptor {
                label: Some("frameshift-mip-dst"),
                base_mip_level: level + 1,
                mip_level_count: Some(1),
                ..Default::default()
            });
            let bind_group = self
                .ctx
                .device
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("frameshift-mip-bg"),
                    layout: &self.pipelines.mipmap_downsample.bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&src_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&self.ctx.linear_sampler),
                        },
                    ],
                });
            draw_fullscreen_pass(
                encoder,
                "frameshift-mip-downsample",
                &self.pipelines.mipmap_downsample.pipeline,
                &bind_group,
                &dst_view,
            );
        }
    }

    fn record_transform3d_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        label: &str,
        tex0: &wgpu::TextureView,
        uniform: &Transform3DUniform,
        target_view: &wgpu::TextureView,
    ) {
        let uniform_buf = self
            .ctx
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytemuck::bytes_of(uniform),
                usage: wgpu::BufferUsages::UNIFORM,
            });

        let bind_group = self
            .ctx
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &self.pipelines.transform3d.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(tex0),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.ctx.linear_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: uniform_buf.as_entire_binding(),
                    },
                ],
            });

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipelines.transform3d.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(0..6, 0..1);
    }

    #[allow(clippy::too_many_arguments)]
    fn record_uniform_draw_pass<U: Pod>(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        label: &str,
        program: &ShaderProgram,
        uniform: &U,
        vertices: std::ops::Range<u32>,
        instances: std::ops::Range<u32>,
        target_view: &wgpu::TextureView,
    ) {
        let uniform_buf = self
            .ctx
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytemuck::bytes_of(uniform),
                usage: wgpu::BufferUsages::UNIFORM,
            });

        let bind_group = self
            .ctx
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &program.bind_group_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buf.as_entire_binding(),
                }],
            });

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&program.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.draw(vertices, instances);
    }

    fn record_zero_tex_pass<U: Pod>(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        label: &str,
        program: &ShaderProgram,
        uniform: &U,
        target_view: &wgpu::TextureView,
    ) {
        let uniform_buf = self
            .ctx
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytemuck::bytes_of(uniform),
                usage: wgpu::BufferUsages::UNIFORM,
            });

        let bind_group = self
            .ctx
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &program.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Sampler(&self.ctx.linear_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: uniform_buf.as_entire_binding(),
                    },
                ],
            });

        draw_fullscreen_pass(encoder, label, &program.pipeline, &bind_group, target_view);
    }

    fn record_one_tex_pass<U: Pod>(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        label: &str,
        program: &ShaderProgram,
        tex0: &wgpu::TextureView,
        uniform: &U,
        target_view: &wgpu::TextureView,
    ) {
        let uniform_buf = self
            .ctx
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytemuck::bytes_of(uniform),
                usage: wgpu::BufferUsages::UNIFORM,
            });

        let bind_group = self
            .ctx
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &program.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(tex0),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.ctx.linear_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: uniform_buf.as_entire_binding(),
                    },
                ],
            });

        draw_fullscreen_pass(encoder, label, &program.pipeline, &bind_group, target_view);
    }

    #[allow(clippy::too_many_arguments)]
    fn record_two_tex_pass<U: Pod>(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        label: &str,
        program: &ShaderProgram,
        tex0: &wgpu::TextureView,
        tex1: &wgpu::TextureView,
        uniform: &U,
        target_view: &wgpu::TextureView,
    ) {
        let uniform_buf = self
            .ctx
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytemuck::bytes_of(uniform),
                usage: wgpu::BufferUsages::UNIFORM,
            });

        let bind_group = self
            .ctx
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &program.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(tex0),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(tex1),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.ctx.linear_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: uniform_buf.as_entire_binding(),
                    },
                ],
            });

        draw_fullscreen_pass(encoder, label, &program.pipeline, &bind_group, target_view);
    }

    fn readback_rgba8(
        &self,
        mut encoder: wgpu::CommandEncoder,
        srgb_texture: &wgpu::Texture,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>, RenderError> {
        let unpadded_bytes_per_row = width * 4;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;
        let buffer_size = (padded_bytes_per_row as u64) * (height as u64);

        let staging_buffer = self.ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frameshift-readback-staging"),
            size: buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: srgb_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        self.ctx.queue.submit(std::iter::once(encoder.finish()));

        let slice = staging_buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |res| {
            let _ = tx.send(res);
        });
        self.ctx
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| RenderError::BufferMap(e.to_string()))?;

        rx.recv()
            .map_err(|e| RenderError::BufferMap(e.to_string()))?
            .map_err(|e| RenderError::BufferMap(e.to_string()))?;

        let mapped = slice
            .get_mapped_range()
            .map_err(|e| RenderError::BufferMap(e.to_string()))?;
        let mut rgba8 = Vec::with_capacity((unpadded_bytes_per_row * height) as usize);
        for row in 0..height as usize {
            let start = row * (padded_bytes_per_row as usize);
            let end = start + (unpadded_bytes_per_row as usize);
            rgba8.extend_from_slice(&mapped[start..end]);
        }
        drop(mapped);
        staging_buffer.unmap();

        Ok(rgba8)
    }
}

struct CameraMatrices {
    view: glam::Mat4,
    proj: glam::Mat4,
    view_proj: glam::Mat4,
    cam_pos: glam::Vec3,
    cam_right: glam::Vec3,
    cam_up: glam::Vec3,
    near: f32,
    far: f32,
}

fn build_camera_matrices(
    camera_pos: [f32; 3],
    camera_rot_deg: [f32; 3],
    fov_deg: f32,
    width: u32,
    height: u32,
) -> CameraMatrices {
    let camera = Camera3DDef {
        pos: camera_pos,
        rot_deg: camera_rot_deg,
        fov_deg,
        mode: Camera3DMode::Euler,
        ..Default::default()
    };
    build_scene_camera_matrices(&camera, width, height)
}

fn build_scene_camera_matrices(camera: &Camera3DDef, width: u32, height: u32) -> CameraMatrices {
    let near = camera.near.max(1e-4);
    let far = camera.far.max(near + 0.01);
    let aspect = (width as f32 / height.max(1) as f32).max(1e-4);
    let fov_y_rad = camera.fov_deg.clamp(5.0, 175.0).to_radians();
    let proj = glam::Mat4::perspective_rh(fov_y_rad, aspect, near, far);

    let cam_pos = glam::Vec3::from_array(camera.pos);
    let (view, cam_right, cam_up) = match camera.mode {
        Camera3DMode::Euler => {
            let cam_rot = glam::Quat::from_euler(
                glam::EulerRot::XYZ,
                camera.rot_deg[0].to_radians(),
                camera.rot_deg[1].to_radians(),
                camera.rot_deg[2].to_radians(),
            );
            let cam_world = glam::Mat4::from_rotation_translation(cam_rot, cam_pos);
            (
                cam_world.inverse(),
                cam_rot * glam::Vec3::X,
                cam_rot * glam::Vec3::Y,
            )
        }
        Camera3DMode::LookAt => {
            let target_v = glam::Vec3::from_array(camera.target);
            let forward = (target_v - cam_pos)
                .try_normalize()
                .unwrap_or(glam::Vec3::NEG_Z);
            let ref_up = if forward.dot(glam::Vec3::Y).abs() > 0.999 {
                glam::Vec3::Z
            } else {
                glam::Vec3::Y
            };
            let roll_quat = glam::Quat::from_axis_angle(forward, camera.rot_deg[2].to_radians());
            let up_v = roll_quat * ref_up;
            let view = glam::Mat4::look_at_rh(cam_pos, target_v, up_v);
            let cam_world = view.inverse();
            let cam_right = cam_world.x_axis.truncate().normalize_or(glam::Vec3::X);
            let cam_up = cam_world.y_axis.truncate().normalize_or(glam::Vec3::Y);
            (view, cam_right, cam_up)
        }
    };

    CameraMatrices {
        view,
        proj,
        view_proj: proj * view,
        cam_pos,
        cam_right,
        cam_up,
        near,
        far,
    }
}

fn create_uniform_bind_group<U: Pod>(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::BindGroupLayout,
    uniform: &U,
) -> wgpu::BindGroup {
    let uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: bytemuck::bytes_of(uniform),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform_buf.as_entire_binding(),
        }],
    })
}

fn mul_alpha(rgba: [f32; 4], opacity: f32) -> [f32; 4] {
    [rgba[0], rgba[1], rgba[2], rgba[3] * opacity]
}

#[allow(clippy::too_many_arguments)]
fn build_transform3d_uniform(
    cam: &CameraMatrices,
    pos: [f32; 3],
    rot_deg: [f32; 3],
    scale: [f32; 3],
    dof_focal_dist: f32,
    dof_blur_strength: f32,
    width: u32,
    height: u32,
) -> Transform3DUniform {
    let model = build_model_matrix(pos, rot_deg, scale);
    let mv = cam.view * model;
    let mvp = cam.proj * mv;
    let aspect = width as f32 / height.max(1) as f32;
    Transform3DUniform {
        mvp: mvp.to_cols_array_2d(),
        model_view_z_row: mv.row(2).to_array(),
        dof_params: [
            dof_focal_dist,
            dof_blur_strength,
            aspect,
            1.0 / width.max(1) as f32,
        ],
        extra: [1.0 / height.max(1) as f32, 0.0, 0.0, 0.0],
    }
}

fn build_text3d_uniform(kind: &NodeKind, cam: &CameraMatrices, t_sec: f32) -> Text3DUniform {
    let NodeKind::Text3D {
        color_top_rgba,
        color_bottom_rgba,
        side_color_rgba,
        pos,
        rot_deg,
        char_stagger_sec,
        char_bounce_amp,
        char_wave_freq,
        time_offset_sec,
        outline_color_rgba,
        material_mode,
        color_mode,
        palette_rgba,
        pop_duration_sec,
        pop_overshoot,
        spin_in_deg,
        char_jitter_deg,
        arc_curvature,
        arc_axis,
        echo_count,
        echo_offset,
        echo_alpha_decay,
        echo_scale_step,
        hdr_intensity,
        opacity,
        ..
    } = kind
    else {
        unreachable!("expected Text3D");
    };

    let op = opacity.clamp(0.0, 1.0);
    let model = build_model_matrix(*pos, *rot_deg, [1.0, 1.0, 1.0]);
    let mvp = cam.view_proj * model;
    let mut palette = [[0.0f32; 4]; 4];
    let p_count = palette_rgba.len().min(4);
    for (dst, src) in palette.iter_mut().zip(palette_rgba.iter().take(p_count)) {
        *dst = mul_alpha(*src, op);
    }

    Text3DUniform {
        mvp: mvp.to_cols_array_2d(),
        model: model.to_cols_array_2d(),
        view: cam.view.to_cols_array_2d(),
        cam_material: [
            cam.cam_pos.x,
            cam.cam_pos.y,
            cam.cam_pos.z,
            *material_mode as f32,
        ],
        color_top: mul_alpha(*color_top_rgba, op),
        color_bottom: mul_alpha(*color_bottom_rgba, op),
        side_color: mul_alpha(*side_color_rgba, op),
        outline_color: mul_alpha(*outline_color_rgba, op),
        anim: [
            t_sec + *time_offset_sec,
            *char_stagger_sec,
            *char_bounce_amp,
            *char_wave_freq,
        ],
        kinetic: [
            *pop_duration_sec,
            *pop_overshoot,
            *spin_in_deg,
            *char_jitter_deg,
        ],
        arc_palette: [
            *arc_curvature,
            *arc_axis as f32,
            *color_mode as f32,
            p_count as f32,
        ],
        echo_params: [
            echo_offset[0],
            echo_offset[1],
            echo_offset[2],
            (*echo_count).min(16) as f32,
        ],
        echo_decay: [*echo_alpha_decay, *echo_scale_step, 0.0, *hdr_intensity],
        palette,
        pbr_params: [-1.0, -1.0, 0.65, 0.0],
    }
}

fn build_shape3d_uniform(kind: &NodeKind, cam: &CameraMatrices, t_sec: f32) -> Text3DUniform {
    let NodeKind::Shape3D {
        material_mode,
        color_top_rgba,
        color_bottom_rgba,
        side_color_rgba,
        outline_color_rgba,
        pos,
        rot_deg,
        scale,
        pop_duration_sec,
        pop_overshoot,
        spin_in_deg,
        echo_count,
        echo_offset,
        echo_alpha_decay,
        hdr_intensity,
        time_offset_sec,
        opacity,
        ..
    } = kind
    else {
        unreachable!("expected Shape3D");
    };

    let op = opacity.clamp(0.0, 1.0);
    let model = build_model_matrix(*pos, *rot_deg, *scale);
    let mvp = cam.view_proj * model;

    Text3DUniform {
        mvp: mvp.to_cols_array_2d(),
        model: model.to_cols_array_2d(),
        view: cam.view.to_cols_array_2d(),
        cam_material: [
            cam.cam_pos.x,
            cam.cam_pos.y,
            cam.cam_pos.z,
            *material_mode as f32,
        ],
        color_top: mul_alpha(*color_top_rgba, op),
        color_bottom: mul_alpha(*color_bottom_rgba, op),
        side_color: mul_alpha(*side_color_rgba, op),
        outline_color: mul_alpha(*outline_color_rgba, op),
        anim: [t_sec + *time_offset_sec, 0.0, 0.0, 0.0],
        kinetic: [*pop_duration_sec, *pop_overshoot, *spin_in_deg, 0.0],
        arc_palette: [0.0, 0.0, 0.0, 0.0],
        echo_params: [
            echo_offset[0],
            echo_offset[1],
            echo_offset[2],
            (*echo_count).min(16) as f32,
        ],
        echo_decay: [*echo_alpha_decay, 0.94, 0.0, *hdr_intensity],
        palette: [[0.0; 4]; 4],
        pbr_params: [-1.0, -1.0, 0.65, 0.0],
    }
}

fn build_particle_field3d_uniform(
    kind: &NodeKind,
    cam: &CameraMatrices,
    t_sec: f32,
    scene_dof_override: Option<(f32, f32)>,
) -> ParticleField3DUniform {
    let NodeKind::ParticleField3D {
        count,
        seed,
        shape,
        color_a_rgba,
        color_b_rgba,
        speed,
        spread,
        size,
        time_offset_sec,
        dof_focal_dist,
        dof_blur_strength,
        opacity,
        ..
    } = kind
    else {
        unreachable!("expected ParticleField3D");
    };

    let (eff_focal, eff_blur) = scene_dof_override.unwrap_or((*dof_focal_dist, *dof_blur_strength));
    let op = opacity.clamp(0.0, 1.0);
    ParticleField3DUniform {
        view_proj: cam.view_proj.to_cols_array_2d(),
        cam_right: [cam.cam_right.x, cam.cam_right.y, cam.cam_right.z, 0.0],
        cam_up: [cam.cam_up.x, cam.cam_up.y, cam.cam_up.z, 0.0],
        cam_pos_time: [
            cam.cam_pos.x,
            cam.cam_pos.y,
            cam.cam_pos.z,
            t_sec + *time_offset_sec,
        ],
        color_a: mul_alpha(*color_a_rgba, op),
        color_b: mul_alpha(*color_b_rgba, op),
        params0: [*speed, *spread, *size, *seed as f32],
        params1: [*shape as f32, eff_focal, eff_blur, *count as f32],
    }
}

fn build_ribbon_trail3d_uniform(
    kind: &NodeKind,
    cam: &CameraMatrices,
    t_sec: f32,
    scene_dof_override: Option<(f32, f32)>,
    viewport_w: u32,
    viewport_h: u32,
) -> RibbonTrail3DUniform {
    let NodeKind::RibbonTrail3D {
        ribbon_count,
        segments,
        seed,
        width: ribbon_width,
        speed,
        radius,
        trim_start,
        trim_end,
        color_a_rgba,
        color_b_rgba,
        time_offset_sec,
        ribbon_style,
        strand_spacing,
        curve_bend,
        pos,
        rot_deg,
        scale,
        opacity,
        core_whiten,
        core_intensity,
        halo_intensity,
        saturation,
        min_width_px,
        ..
    } = kind
    else {
        unreachable!("expected RibbonTrail3D");
    };

    let (eff_focal, eff_blur) = scene_dof_override.unwrap_or((2.5, 0.0));
    let segs = (*segments).max(1);
    let model = build_model_matrix(*pos, *rot_deg, *scale);
    RibbonTrail3DUniform {
        view_proj: cam.view_proj.to_cols_array_2d(),
        cam_pos_time: [
            cam.cam_pos.x,
            cam.cam_pos.y,
            cam.cam_pos.z,
            t_sec + *time_offset_sec,
        ],
        color_a: *color_a_rgba,
        color_b: *color_b_rgba,
        params0: [*ribbon_width, *speed, *radius, *seed as f32],
        params1: [*trim_start, *trim_end, segs as f32, *ribbon_count as f32],
        model: model.to_cols_array_2d(),
        params2: [
            *ribbon_style as f32,
            *strand_spacing,
            *curve_bend,
            opacity.clamp(0.0, 1.0),
        ],
        dof_params: [eff_focal, eff_blur, 1.5, 0.0],
        params3: [
            min_width_px.max(0.0),
            viewport_w.max(1) as f32,
            viewport_h.max(1) as f32,
            saturation.max(0.0),
        ],
        params4: [
            core_intensity.max(0.0),
            halo_intensity.max(0.0),
            core_whiten.clamp(0.0, 1.0),
            0.0,
        ],
    }
}

fn collect_scene3d_skip_standalone(
    sub_order: &[&GraphNodeDef],
    target_id: &NodeId,
) -> HashSet<NodeId> {
    let mut enabled_scene3d_children: HashSet<NodeId> = HashSet::new();
    let mut non_scene3d_consumers: HashSet<NodeId> = HashSet::new();

    for node in sub_order {
        if node.disabled_passthrough().is_none() && matches!(node.kind, NodeKind::Scene3D { .. })
        {
            for upstream in &node.inputs {
                enabled_scene3d_children.insert(upstream.clone());
            }
        } else if let Some(DisabledPassthrough::Input(index)) = node.disabled_passthrough() {
            if let Some(upstream) = node.inputs.get(index) {
                non_scene3d_consumers.insert(upstream.clone());
            }
        } else if node.disabled_passthrough().is_none() {
            for upstream in &node.inputs {
                non_scene3d_consumers.insert(upstream.clone());
            }
        }
    }

    let mut skip = HashSet::new();
    for node in sub_order {
        if &node.id != target_id
            && node.kind.is_scene3d_child()
            && (node.enabled || node.inputs.is_empty())
            && enabled_scene3d_children.contains(&node.id)
            && !non_scene3d_consumers.contains(&node.id)
        {
            skip.insert(node.id.clone());
        }
    }
    skip
}

fn release_if_last_use(
    pool: &mut TexturePool,
    remaining_uses: &mut HashMap<NodeId, usize>,
    textures: &mut HashMap<NodeId, PooledTexture>,
    id: &NodeId,
) {
    if let Some(count) = remaining_uses.get_mut(id) {
        *count -= 1;
        if *count == 0
            && let Some(tex) = textures.remove(id)
        {
            pool.release(tex);
        }
    }
}

fn build_model_matrix(pos: [f32; 3], rot_deg: [f32; 3], scale: [f32; 3]) -> glam::Mat4 {
    let rot = glam::Quat::from_euler(
        glam::EulerRot::XYZ,
        rot_deg[0].to_radians(),
        rot_deg[1].to_radians(),
        rot_deg[2].to_radians(),
    );
    glam::Mat4::from_scale_rotation_translation(
        glam::Vec3::from_array(scale),
        rot,
        glam::Vec3::from_array(pos),
    )
}

fn validate_dimensions(width: u32, height: u32) -> Result<(), RenderError> {
    if width == 0 || height == 0 {
        return Err(RenderError::InvalidDimensions { width, height });
    }
    Ok(())
}

fn copy_texture_2d(
    encoder: &mut wgpu::CommandEncoder,
    src: &wgpu::Texture,
    dst: &wgpu::Texture,
    width: u32,
    height: u32,
) {
    encoder.copy_texture_to_texture(
        wgpu::TexelCopyTextureInfo {
            texture: src,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyTextureInfo {
            texture: dst,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
}

fn source_fit_code(fit: SourceFit) -> u8 {
    match fit {
        SourceFit::Stretch => 0,
        SourceFit::Cover => 1,
        SourceFit::Contain => 2,
    }
}

fn color_space_code(space: ColorSpace) -> u8 {
    match space {
        ColorSpace::LinearRec709 => 0,
        ColorSpace::DisplaySrgb => 1,
    }
}

fn file_fingerprint(path: &Path) -> (Option<u64>, Option<u128>) {
    let Ok(meta) = std::fs::metadata(path) else {
        return (None, None);
    };
    let len = Some(meta.len());
    let modified_nanos = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos());
    (len, modified_nanos)
}

fn build_lut_cache_key(
    lut_source: Option<&LutSourceDef>,
    lut_preset: &str,
    input_space: Option<ExtendedColorSpace>,
    output_space: Option<ExtendedColorSpace>,
) -> LutCacheKey {
    let source = match lut_source {
        None => {
            let path = Path::new(lut_preset);
            let (file_len, modified_nanos) = if path.is_file() {
                file_fingerprint(path)
            } else {
                (None, None)
            };
            LutSourceCacheKey::Legacy {
                spec: lut_preset.to_owned(),
                file_len,
                modified_nanos,
            }
        }
        Some(LutSourceDef::Preset { name }) => LutSourceCacheKey::Preset(name.clone()),
        Some(LutSourceDef::CubeFile {
            path,
            lut_size_override,
        }) => {
            let (file_len, modified_nanos) = file_fingerprint(Path::new(path));
            LutSourceCacheKey::CubeFile {
                path: path.clone(),
                lut_size_override: *lut_size_override,
                file_len,
                modified_nanos,
            }
        }
        Some(LutSourceDef::Ocio {
            config_path,
            src_space,
            dst_space,
            look,
            lut_size,
            shaper_space,
        }) => {
            let (config_len, config_modified_nanos) = file_fingerprint(Path::new(config_path));
            LutSourceCacheKey::Ocio {
                config_path: config_path.clone(),
                src_space: src_space.clone(),
                dst_space: dst_space.clone(),
                look: look.clone(),
                lut_size: *lut_size,
                shaper_space: shaper_space.clone(),
                config_len,
                config_modified_nanos,
            }
        }
        Some(LutSourceDef::BuiltinTransform {
            src_space,
            dst_space,
            lut_size,
        }) => LutSourceCacheKey::BuiltinTransform {
            src_space: *src_space,
            dst_space: *dst_space,
            lut_size: *lut_size,
        },
    };
    LutCacheKey {
        source,
        input_space,
        output_space,
    }
}

fn is_vector_override_absent_message(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    if lower.contains("vector error") || lower.contains("font") {
        return false;
    }
    lower.contains("dynamic")
        || lower.contains("built-in")
        || lower.contains("builtin")
        || lower.contains("no external")
        || lower.contains("override")
        || lower.contains("unused")
        || lower.contains("fallback")
        || lower.contains("not provided")
        || lower.contains("no frame")
        || lower.contains("missing asset")
        || lower.contains("unknown asset")
        || lower == "not found"
}

fn collect_direct_cone(
    graph: &RenderGraphDef,
    target_id: &NodeId,
) -> Result<HashSet<NodeId>, RenderError> {
    let mut visited = HashSet::new();
    let mut stack = vec![target_id.clone()];
    while let Some(id) = stack.pop() {
        if !visited.insert(id.clone()) {
            continue;
        }
        let node = graph
            .node(&id)
            .ok_or_else(|| frameshift_schema::SchemaError::MissingNode(id.clone()))?;
        if &id != target_id && matches!(node.kind, NodeKind::MotionBlurShutter { .. }) {
            continue;
        }
        for upstream in &node.inputs {
            stack.push(upstream.clone());
        }
    }
    Ok(visited)
}

fn require_input<'a>(
    node: &GraphNodeDef,
    textures: &'a HashMap<NodeId, PooledTexture>,
    index: usize,
) -> Result<&'a PooledTexture, RenderError> {
    let input_id = node
        .inputs
        .get(index)
        .ok_or_else(|| RenderError::MissingGraphInput {
            node_id: node.id.clone(),
            expected: node.kind.expected_input_count(),
            actual: node.inputs.len(),
        })?;
    textures.get(input_id).ok_or_else(|| {
        RenderError::Schema(frameshift_schema::SchemaError::MissingNode(
            input_id.clone(),
        ))
    })
}

fn clear_texture(encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, rgba: [f32; 4]) {
    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("frameshift-clear-solid"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color {
                    r: rgba[0] as f64,
                    g: rgba[1] as f64,
                    b: rgba[2] as f64,
                    a: rgba[3] as f64,
                }),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
}

fn draw_fullscreen_pass(
    encoder: &mut wgpu::CommandEncoder,
    label: &str,
    pipeline: &wgpu::RenderPipeline,
    bind_group: &wgpu::BindGroup,
    target_view: &wgpu::TextureView,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target_view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, &[]);
    pass.draw(0..3, 0..1);
}

fn create_dummy_zero_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> (wgpu::Texture, wgpu::TextureView) {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("frameshift-dummy-zero-rgba16f"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: WORKING_TEXTURE_FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let zero = [0u8; 8];
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &zero,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(8),
            rows_per_image: Some(1),
        },
        wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    (tex, view)
}

fn premultiply_rgba8(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let (chunks, _) = data.as_chunks::<4>();
    for px in chunks {
        let a = px[3];
        if a == 255 {
            out.extend_from_slice(px);
        } else if a == 0 {
            out.extend_from_slice(&[0, 0, 0, 0]);
        } else {
            let a16 = a as u16;
            out.push(((px[0] as u16 * a16 + 127) / 255) as u8);
            out.push(((px[1] as u16 * a16 + 127) / 255) as u8);
            out.push(((px[2] as u16 * a16 + 127) / 255) as u8);
            out.push(a);
        }
    }
    out
}

fn srgb_to_linear_channel(c: f32) -> f32 {
    let x = c.max(0.0);
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

fn premul_srgb_to_linear(rgba: [f32; 4]) -> [f32; 4] {
    let a = rgba[3].clamp(0.0, 1.0);
    if a <= 1e-6 {
        return [0.0, 0.0, 0.0, 0.0];
    }
    let sr = (rgba[0] / a).max(0.0);
    let sg = (rgba[1] / a).max(0.0);
    let sb = (rgba[2] / a).max(0.0);
    [
        srgb_to_linear_channel(sr) * a,
        srgb_to_linear_channel(sg) * a,
        srgb_to_linear_channel(sb) * a,
        a,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::FnFrameProvider;
    use frameshift_schema::{
        AnimationEasing, AnimationKeyframe, BlendMode, MatteMode, RenderProfileKind, Resolution,
        WorkingPixelFormat,
    };

    #[test]
    fn audio_rms_envelope_interpolates_clamps_and_handles_out_of_range() {
        let empty = AudioRmsEnvelope::new(Flicks(100), vec![]);
        assert_eq!(empty.sample_at(Flicks(0)), 0.0);

        let env = AudioRmsEnvelope::new(Flicks(100), vec![0.2, 0.8, 1.5]);
        assert_eq!(env.sample_at(Flicks(-1)), 0.0);
        assert!((env.sample_at(Flicks(0)) - 0.2).abs() < 1e-6);
        assert!((env.sample_at(Flicks(50)) - 0.5).abs() < 1e-6);
        assert!((env.sample_at(Flicks(100)) - 0.8).abs() < 1e-6);
        assert!((env.sample_at(Flicks(150)) - 1.0).abs() < 1e-6);
        assert_eq!(env.sample_at(Flicks(300)), 0.0);
    }

    #[test]
    fn evaluate_event_animation_respects_hold_after() {
        let mut anim = EventBoundAnimation {
            id: "a1".into(),
            event_id: "ev".into(),
            target_node_id: NodeId::from("n"),
            parameter: "progress".into(),
            offset_from_event: Flicks::ZERO,
            duration: Flicks(1000),
            keyframes: vec![
                AnimationKeyframe {
                    progress: 0.0,
                    value: 0.0,
                    easing: AnimationEasing::Linear,
                },
                AnimationKeyframe {
                    progress: 1.0,
                    value: 1.0,
                    easing: AnimationEasing::Linear,
                },
            ],
            hold_after: false,
        };
        assert_eq!(
            evaluate_event_animation(&anim, Flicks(500), Flicks(400)),
            None
        );
        assert_eq!(
            evaluate_event_animation(&anim, Flicks(500), Flicks(1000)),
            Some(0.5)
        );
        assert_eq!(
            evaluate_event_animation(&anim, Flicks(500), Flicks(1501)),
            None
        );

        anim.hold_after = true;
        assert_eq!(
            evaluate_event_animation(&anim, Flicks(500), Flicks(1501)),
            Some(1.0)
        );
    }

    #[test]
    fn project_animator_uses_real_audio_envelope_when_provided() {
        let mut project = Project::golden_scene_preset();
        project.param_bindings = vec![ParamBindingDef {
            node_id: NodeId::from("grade_flash"),
            param_name: "flash_amount".into(),
            expression: "audio_rms".into(),
        }];
        let tick = Flicks(FLICKS_PER_SECOND);
        let env = AudioRmsEnvelope::new(Flicks(FLICKS_PER_SECOND), vec![0.12, 0.12, 0.12]);

        let synth_anim = ProjectAnimator::new(&project).expect("compile synth bindings");
        let real_anim =
            ProjectAnimator::with_audio_envelope(&project, Some(&env)).expect("compile envelope");
        let base = project
            .graph
            .node(&NodeId::from("grade_flash"))
            .unwrap()
            .kind
            .clone();

        let NodeKind::ColorBasic {
            flash_amount: synth_flash,
            ..
        } = synth_anim
            .evaluate_node(&NodeId::from("grade_flash"), &base, tick)
            .expect("synth eval")
        else {
            panic!("expected ColorBasic");
        };
        let NodeKind::ColorBasic {
            flash_amount: real_flash,
            ..
        } = real_anim
            .evaluate_node(&NodeId::from("grade_flash"), &base, tick)
            .expect("envelope eval")
        else {
            panic!("expected ColorBasic");
        };

        assert!((real_flash - 0.12).abs() < 1e-4);
        assert!((synth_flash - real_flash).abs() > 0.1);
    }

    fn blend_opacity(kind: &NodeKind) -> f32 {
        match kind {
            NodeKind::Blend { opacity, .. } => *opacity,
            other => panic!("expected Blend, got {other:?}"),
        }
    }

    #[test]
    fn bad_expression_does_not_keep_the_base_value() {
        let mut project = Project::golden_scene_preset();
        project.param_bindings.push(ParamBindingDef {
            node_id: NodeId::from("blend_title"),
            param_name: "opacity".into(),
            expression: "step(".into(),
        });
        let Err(parse_err) = ProjectAnimator::new(&project) else {
            panic!("unclosed expression compiled");
        };
        let parse_text = parse_err.to_string();
        assert!(
            parse_text.contains("blend_title"),
            "parse error must name the node: {parse_text}"
        );
        assert!(
            parse_text.contains("opacity"),
            "parse error must name the parameter: {parse_text}"
        );

        project.param_bindings.last_mut().unwrap().expression = "1 / (t_sec - t_sec)".into();
        project
            .validate()
            .expect("division by zero is an evaluation error, not a parse error");
        let animator = ProjectAnimator::new(&project).expect("expression parses");
        let base = NodeKind::Blend {
            mode: BlendMode::Normal,
            opacity: 0.37,
        };
        match animator.evaluate_node(&NodeId::from("blend_title"), &base, Flicks::ZERO) {
            Ok(kind) => panic!(
                "bad expression kept a value {} (base was 0.37)",
                blend_opacity(&kind)
            ),
            Err(err) => {
                let text = err.to_string();
                assert!(text.contains("blend_title"), "{text}");
                assert!(text.contains("opacity"), "{text}");
                assert!(text.contains("division by zero"), "{text}");
                assert!(text.contains("1 / (t_sec - t_sec)"), "{text}");
            }
        }
    }

    #[test]
    fn t_sec_step_window_replaces_base_opacity() {
        let mut project = Project::golden_scene_preset();
        project.param_bindings.push(ParamBindingDef {
            node_id: NodeId::from("blend_title"),
            param_name: "opacity".into(),
            expression: "step(1.905, t_sec) * (1 - step(1.980, t_sec))".into(),
        });
        let animator = ProjectAnimator::new(&project).expect("flash expression parses");
        let base = NodeKind::Blend {
            mode: BlendMode::Normal,
            opacity: 0.37,
        };
        let inside = Flicks(1940 * frameshift_time::FLICKS_PER_MILLISECOND);
        let after = Flicks(2000 * frameshift_time::FLICKS_PER_MILLISECOND);
        let opacity_at = |tick: Flicks| {
            let kind = animator
                .evaluate_node(&NodeId::from("blend_title"), &base, tick)
                .expect("window expression evaluates");
            blend_opacity(&kind)
        };
        assert!((opacity_at(Flicks::ZERO) - 0.0).abs() < 1e-5);
        assert!((opacity_at(inside) - 1.0).abs() < 1e-5);
        assert!((opacity_at(after) - 0.0).abs() < 1e-5);
        assert!((opacity_at(inside) - 0.37).abs() > 0.5);
    }

    #[test]
    fn explicit_blur_ceiling_only_lowers_authored_samples() {
        assert_eq!(effective_blur_sub_samples(8, None), 8);
        assert_eq!(effective_blur_sub_samples(8, Some(4)), 4);
        assert_eq!(effective_blur_sub_samples(8, Some(1)), 1);
        assert_eq!(effective_blur_sub_samples(8, Some(16)), 8);
    }

    #[test]
    fn mask_matte_opaque_black_outside_produces_opaque_alpha() {
        let mut compositor = GpuCompositor::new_headless().expect("headless init");
        let make_graph = |opaque_black_outside: bool| RenderGraphDef {
            output_node: NodeId::from("masked"),
            nodes: vec![
                GraphNodeDef {
                    id: NodeId::from("src"),
                    kind: NodeKind::Solid {
                        color_rgba: [0.8, 0.4, 0.2, 1.0],
                        color_space: ColorSpace::LinearRec709,
                    },
                    inputs: vec![],
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("matte"),
                    kind: NodeKind::Solid {
                        color_rgba: [0.0, 0.0, 0.0, 0.0],
                        color_space: ColorSpace::LinearRec709,
                    },
                    inputs: vec![],
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("masked"),
                    kind: NodeKind::MaskMatte {
                        mode: MatteMode::Alpha,
                        invert: false,
                        feather: 0.0,
                        opaque_black_outside,
                    },
                    inputs: vec![NodeId::from("src"), NodeId::from("matte")],
                    enabled: true,
                },
            ],
        };

        let provider = FnFrameProvider(
            |_: &str,
             _: Flicks,
             _: u32,
             _: u32|
             -> Result<crate::provider::SourceFrame, RenderError> {
                unreachable!("no source video")
            },
        );
        let opts = RenderOptions {
            tonemap_mode: Some(TonemapMode::None),
            output_alpha_mode: OutputAlphaMode::Straight,
            ..Default::default()
        };
        let frame_ticks = Flicks(FLICKS_PER_SECOND / 24);

        let transparent_out = compositor
            .render_frame(
                &make_graph(false),
                Flicks::ZERO,
                frame_ticks,
                16,
                16,
                &provider,
                &NoopAnimator,
                &opts,
            )
            .expect("transparent mask render");
        assert_eq!(&transparent_out.rgba8[0..4], &[0, 0, 0, 0]);

        let opaque_out = compositor
            .render_frame(
                &make_graph(true),
                Flicks::ZERO,
                frame_ticks,
                16,
                16,
                &provider,
                &NoopAnimator,
                &opts,
            )
            .expect("opaque black outside mask render");
        assert_eq!(&opaque_out.rgba8[0..4], &[0, 0, 0, 255]);

        // Also smoke-test render_project_frame_with_audio
        let project = Project::golden_scene_preset();
        let profile = RenderProfileDef {
            kind: RenderProfileKind::Draft,
            resolution: Resolution {
                width: 64,
                height: 36,
            },
            motion_blur_sub_samples: 1,
            shutter_angle_deg: 0.0,
            pixel_format: WorkingPixelFormat::Rgba16FloatPremul,
        };
        let env = AudioRmsEnvelope::new(Flicks(FLICKS_PER_SECOND), vec![0.5; 6]);
        let dummy_provider = FnFrameProvider(|asset_id: &str, _: Flicks, w: u32, h: u32| {
            if asset_id.starts_with("vector_text://") {
                return Err(RenderError::FrameProvider {
                    asset_id: asset_id.to_owned(),
                    source_tick: 0,
                    message: "use built-in".into(),
                });
            }
            Ok(crate::provider::SourceFrame::rgba8_straight(
                w,
                h,
                vec![128; (w * h * 4) as usize],
            ))
        });
        let r = compositor
            .render_project_frame_with_audio(
                &project,
                Flicks(FLICKS_PER_SECOND),
                &profile,
                &dummy_provider,
                &RenderOptions::default(),
                Some(&env),
            )
            .expect("render_project_frame_with_audio");
        assert_eq!(r.width, 64);
    }
}
