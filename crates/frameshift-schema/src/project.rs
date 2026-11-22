use crate::error::SchemaError;
use crate::graph::{
    BlendMode, ColorSpace, GraphNodeDef, HalftoneColorMode, MatteMode, NodeId, NodeKind,
    RenderGraphDef, SampleFilter, ShutterPhase, SourceFit,
};
use crate::version::{CURRENT_SCHEMA_VERSION, SchemaVersion};
use frameshift_time::{
    AUDIO_48_KHZ, FLICKS_PER_SECOND, FPS_24, FPS_60, Flicks, Rational32, SafeExpr, SpeedCurve,
    SpeedKeyframe,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Media asset kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    Video,
    Audio,
    Image,
}

/// 2D pixel dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

/// External media asset registered in the project.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetDef {
    pub id: String,
    pub kind: AssetKind,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ticks: Option<Flicks>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fps: Option<Rational32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_rate_hz: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<Resolution>,
}

/// Easing mode between animation keyframes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnimationEasing {
    #[default]
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
    Step,
}

/// Normalized animation keyframe evaluated over an `EventBoundAnimation` duration.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AnimationKeyframe {
    /// Normalized time within `[0.0, 1.0]` across the animation's duration.
    pub progress: f64,
    pub value: f64,
    #[serde(default)]
    pub easing: AnimationEasing,
}

/// Parameter animation anchored relative to a semantic `event_id` (ADR-0003).
///
/// When the target event moves on the timeline, this animation shifts with it
/// without affecting unrelated keyframes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventBoundAnimation {
    pub id: String,
    pub event_id: String,
    pub target_node_id: NodeId,
    pub parameter: String,
    /// Offset from `event.target_output_tick` (can be negative for anticipation).
    pub offset_from_event: Flicks,
    pub duration: Flicks,
    pub keyframes: Vec<AnimationKeyframe>,
    #[serde(default)]
    pub hold_after: bool,
}

/// Timeline video clip referencing an `AssetDef` and a `SpeedCurve`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClipDef {
    pub id: String,
    pub asset_id: String,
    pub timeline_start: Flicks,
    pub timeline_end: Flicks,
    pub source_start: Flicks,
    pub speed_curve: SpeedCurve,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linked_event_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_animations: Vec<EventBoundAnimation>,
}

/// Fade curve applied to an audio clip's fade-in or fade-out ramp.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioFadeCurve {
    /// Linear ramp: `g(p) = p`.
    #[default]
    Linear,
    /// Equal-power ramp: `g(p) = sin(pi/2 * p)`; complementary ramps sum to unity power.
    EqualPower,
    /// Smoothstep ramp: `g(p) = p^2 * (3 - 2p)`; zero slope at both ends.
    Smoothstep,
}

impl AudioFadeCurve {
    /// Evaluates the normalized curve at `progress`, clamped to `[0.0, 1.0]`.
    pub fn gain_at(self, progress: f64) -> f64 {
        let p = progress.clamp(0.0, 1.0);
        match self {
            Self::Linear => p,
            Self::EqualPower => (std::f64::consts::FRAC_PI_2 * p).sin(),
            Self::Smoothstep => p * p * (3.0 - 2.0 * p),
        }
    }
}

/// Fade ramp attached to one edge of an [`AudioClipDef`].
///
/// The ramp is a multiplier in `[0.0, 1.0]` applied on top of the clip's `gain_db`
/// base level: fade-in ramps `0 -> 1` across the first `duration_ticks`, fade-out
/// ramps `1 -> 0` across the last `duration_ticks` of the clip.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AudioFadeDef {
    /// Fade length in Flicks; `0` is a no-op and must not exceed the clip duration.
    pub duration_ticks: Flicks,
    #[serde(default)]
    pub curve: AudioFadeCurve,
}

impl AudioFadeDef {
    /// Returns the fade multiplier at `elapsed_ticks` into the ramp.
    ///
    /// Callers pass elapsed time for a fade-in and remaining time for a fade-out.
    /// A zero-length ramp (or any non-positive duration) returns `1.0`.
    pub fn gain_at_elapsed(self, elapsed_ticks: f64) -> f64 {
        if self.duration_ticks <= Flicks::ZERO {
            return 1.0;
        }
        self.curve
            .gain_at(elapsed_ticks / self.duration_ticks.0 as f64)
    }
}

/// Timeline audio clip (e.g., music track or SFX).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioClipDef {
    pub id: String,
    pub asset_id: String,
    pub timeline_start: Flicks,
    pub timeline_end: Flicks,
    pub source_start: Flicks,
    #[serde(default)]
    pub gain_db: f32,
    /// Optional fade-in multiplier ramp at the clip head (`0 -> 1`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fade_in: Option<AudioFadeDef>,
    /// Optional fade-out multiplier ramp at the clip tail (`1 -> 0`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fade_out: Option<AudioFadeDef>,
}

/// Timeline definition containing fps, duration, resolution, and clips.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimelineDef {
    pub fps: Rational32,
    pub duration_ticks: Flicks,
    pub resolution: Resolution,
    #[serde(default)]
    pub clips: Vec<ClipDef>,
    #[serde(default)]
    pub audio_clips: Vec<AudioClipDef>,
}

/// Distinguishes human-marked anchors from algorithmically detected markers (ADR-0003).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventOrigin {
    #[default]
    UserMarked,
    Detected,
}

/// Semantic event anchor linking a source moment to a target output beat/drop tick.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventAnchorDef {
    pub event_id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_clip_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_anchor_tick: Option<Flicks>,
    pub target_output_tick: Flicks,
    #[serde(default)]
    pub origin: EventOrigin,
}

/// Named render quality profile kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderProfileKind {
    Draft,
    Final,
}

/// Working color/pixel format inside the GPU compositor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkingPixelFormat {
    #[default]
    Rgba16FloatPremul,
}

/// Configuration for a render profile (`Draft` or `Final`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderProfileDef {
    pub kind: RenderProfileKind,
    pub resolution: Resolution,
    pub motion_blur_sub_samples: u32,
    pub shutter_angle_deg: f32,
    #[serde(default)]
    pub pixel_format: WorkingPixelFormat,
}

/// Draft and Final render profile pair.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderProfilesDef {
    pub draft: RenderProfileDef,
    #[serde(rename = "final")]
    pub final_profile: RenderProfileDef,
}

/// Safe expression binding driving a render graph node parameter at evaluation time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParamBindingDef {
    pub node_id: NodeId,
    pub param_name: String,
    pub expression: String,
}

/// Names the evaluator writes for `kind`.
///
/// A binding whose name is absent here is not applied, so the node keeps its
/// authored base value. `center_u` / `center_uv_x` (and the `v` / `y` pair) are
/// both accepted because both spellings write the same channel.
fn bindable_param_names(kind: &NodeKind) -> &'static [&'static str] {
    match kind {
        NodeKind::Transform2D { .. } => &[
            "translate_x",
            "translate_y",
            "scale",
            "scale_x",
            "scale_y",
            "rotation_rad",
            "anchor_x",
            "anchor_y",
        ],
        NodeKind::MaskMatte { .. } => &["feather"],
        NodeKind::Blend { .. } => &["opacity"],
        NodeKind::ColorBasic { .. } => &["exposure", "contrast", "saturation", "flash_amount"],
        NodeKind::ShaderTransition { .. } => &[
            "progress",
            "direction_rad",
            "whip_strength",
            "chromatic_aberration",
            "impact_flash",
        ],
        NodeKind::Lut3D { .. } => &["intensity"],
        NodeKind::VectorText { .. } => &[
            "font_size_norm",
            "outline_width_norm",
            "y_offset_norm",
            "x_offset_norm",
            "tracking_em",
            "line_height_em",
            "opacity",
            "var_wght",
            "var_wdth",
            "var_slnt",
            "hdr_intensity",
            "outer_outline_width_norm",
            "outline_gap_norm",
            "glow_radius_norm",
            "shadow_blur_norm",
            "gradient_angle_deg",
        ],
        NodeKind::SourceImage { .. } => &["opacity"],
        NodeKind::Precomp { .. } => &["time_offset_ticks", "time_scale", "opacity"],
        NodeKind::VectorPathHud { .. } => &[
            "stroke_width_norm",
            "dash_len_norm",
            "gap_len_norm",
            "dash_offset_norm",
            "trim_start",
            "trim_end",
            "node_dot_radius_norm",
            "data_value",
            "opacity",
        ],
        NodeKind::CornerPin { .. } => &[
            "tl_x", "tl_y", "tr_x", "tr_y", "br_x", "br_y", "bl_x", "bl_y", "opacity",
        ],
        NodeKind::GridWarp { .. } => &[
            "wave_amp_x",
            "wave_amp_y",
            "wave_freq_x",
            "wave_freq_y",
            "wave_phase",
            "mix_amount",
            "opacity",
        ],
        NodeKind::MotionTileShake { .. } => &[
            "tile_scale",
            "mirror_edges",
            "shake_amp_norm_x",
            "shake_amp_norm_y",
            "shake_rot_rad",
            "shake_zoom",
            "shake_freq_hz",
            "shake_decay",
            "event_delta_sec",
        ],
        NodeKind::AnimeImpactFrame { .. } => &[
            "luma_threshold",
            "edge_weight",
            "invert_amount",
            "speed_line_density",
            "speed_line_intensity",
            "center_u",
            "center_uv_x",
            "center_v",
            "center_uv_y",
            "mix_amount",
        ],
        NodeKind::RadialShockwave { .. } => &[
            "center_u",
            "center_uv_x",
            "center_v",
            "center_uv_y",
            "radius",
            "ring_width",
            "refraction_amp",
            "zoom_blur_strength",
            "chromatic_aberration",
        ],
        NodeKind::HalftoneGlow { .. } => &[
            "dot_scale",
            "angle_rad",
            "halftone_mix",
            "glow_threshold",
            "glow_intensity",
            "glow_radius_px",
            "dot_softness",
        ],
        NodeKind::DeepGlow { .. } => &[
            "threshold",
            "knee",
            "intensity",
            "radius",
            "scatter",
            "anamorphic_ratio",
            "chromatic_shift",
        ],
        NodeKind::AnimeSharpen { .. } => &["line_strength", "sharpen_amount", "anti_ringing"],
        NodeKind::FilmOptics { .. } => &[
            "lens_k1",
            "lens_k2",
            "chromatic_edge",
            "grain_amount",
            "grain_seed",
            "halation_amount",
            "vignette_amount",
            "grain_size_px",
            "grain_chroma",
            "halation_threshold",
        ],
        NodeKind::CustomWgsl { .. } => &[
            "p0", "p1", "p2", "p3", "p4", "p5", "p6", "p7", "p8", "p9", "p10", "p11", "p12", "p13",
            "p14", "p15",
        ],
        NodeKind::Transform3D { .. } => &[
            "pos_x",
            "pos_y",
            "pos_z",
            "rot_x",
            "rot_y",
            "rot_z",
            "scale_x",
            "scale_y",
            "scale_z",
            "cam_x",
            "cam_y",
            "cam_z",
            "cam_rot_x",
            "cam_rot_y",
            "cam_rot_z",
            "fov_deg",
            "dof_focal_dist",
            "dof_blur_strength",
        ],
        NodeKind::Text3D { .. } => &[
            "pos_x",
            "pos_y",
            "pos_z",
            "rot_x",
            "rot_y",
            "rot_z",
            "cam_x",
            "cam_y",
            "cam_z",
            "cam_rot_x",
            "cam_rot_y",
            "cam_rot_z",
            "fov_deg",
            "extrude_depth",
            "char_bounce_amp",
            "char_stagger_sec",
            "char_wave_freq",
            "outline_width",
            "pop_duration_sec",
            "pop_overshoot",
            "spin_in_deg",
            "char_jitter_deg",
            "arc_curvature",
            "hdr_intensity",
            "time_offset_sec",
            "opacity",
            "var_wght",
            "var_wdth",
            "var_slnt",
            "tracking_em",
        ],
        NodeKind::Shape3D { .. } => &[
            "size",
            "aspect",
            "stroke_width",
            "corner_radius",
            "inner_ratio",
            "extrude_depth",
            "bevel_size",
            "pos_x",
            "pos_y",
            "pos_z",
            "rot_x",
            "rot_y",
            "rot_z",
            "scale_x",
            "scale_y",
            "scale_z",
            "cam_x",
            "cam_y",
            "cam_z",
            "cam_rot_x",
            "cam_rot_y",
            "cam_rot_z",
            "fov_deg",
            "pop_duration_sec",
            "pop_overshoot",
            "spin_in_deg",
            "hdr_intensity",
            "time_offset_sec",
            "opacity",
        ],
        NodeKind::ParticleField3D { .. } => &[
            "speed",
            "spread",
            "size",
            "cam_x",
            "cam_y",
            "cam_z",
            "cam_rot_x",
            "cam_rot_y",
            "cam_rot_z",
            "fov_deg",
            "time_offset_sec",
            "dof_focal_dist",
            "dof_blur_strength",
            "opacity",
        ],
        NodeKind::RibbonTrail3D { .. } => &[
            "width",
            "core_whiten",
            "core_intensity",
            "halo_intensity",
            "saturation",
            "min_width_px",
            "speed",
            "radius",
            "trim_start",
            "trim_end",
            "strand_spacing",
            "curve_bend",
            "pos_x",
            "pos_y",
            "pos_z",
            "rot_x",
            "rot_y",
            "rot_z",
            "scale_x",
            "scale_y",
            "scale_z",
            "cam_x",
            "cam_y",
            "cam_z",
            "cam_rot_x",
            "cam_rot_y",
            "cam_rot_z",
            "fov_deg",
            "time_offset_sec",
            "opacity",
        ],
        NodeKind::Scene3D { .. } => &[
            "cam_x",
            "cam_y",
            "cam_z",
            "cam_rot_x",
            "cam_rot_y",
            "cam_rot_z",
            "cam_roll",
            "cam_target_x",
            "cam_target_y",
            "cam_target_z",
            "fov_deg",
            "dof_focal_dist",
            "dof_blur_strength",
            "dof_max_radius_px",
            "dof_highlight_boost",
            "dof_blade_rotation_deg",
            "dof_blade_roundness",
            "dof_anamorphic_ratio",
            "dof_cats_eye_strength",
        ],
        NodeKind::BackdropScrim { .. } => &[
            "rect_center_x",
            "rect_center_y",
            "rect_size_x",
            "rect_size_y",
            "corner_radius_norm",
            "blur_radius_norm",
            "opacity",
            "auto_contrast",
        ],
        NodeKind::SourceVideo { .. }
        | NodeKind::Solid { .. }
        | NodeKind::MotionBlurShutter { .. }
        | NodeKind::PrecompInput { .. } => &[],
    }
}

fn find_node_in_graphs<'a>(graph: &'a RenderGraphDef, id: &NodeId) -> Option<&'a GraphNodeDef> {
    graph.find_node_recursive(id)
}

/// Structured creative decision log entry recorded during editing/directing (Phase 3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreativeDecisionRecord {
    pub decision_id: String,
    pub approach: String,
    pub rationale: String,
    #[serde(default)]
    pub evidence_ticks: Vec<Flicks>,
    #[serde(default)]
    pub protected_invariants: Vec<String>,
    #[serde(default)]
    pub risks_to_verify: Vec<String>,
}

/// Scope hierarchy for 3-tier user/agent preferences (Phase 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreferenceScope {
    Global,
    #[default]
    Project,
    Scene,
}

/// Single preference entry scoped globally, per-project, or per-scene (Phase 3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreferenceEntry {
    pub key: String,
    pub value: String,
    pub scope: PreferenceScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
}

/// Scaled proxy media generation and resolution policy (Phase 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyConfigDef {
    pub enabled: bool,
    pub scale_num: u32,
    pub scale_den: u32,
}

impl Default for ProxyConfigDef {
    fn default() -> Self {
        Self {
            enabled: false,
            scale_num: 1,
            scale_den: 2,
        }
    }
}

/// Hardware acceleration policy for video decode/encode (Phase 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HwAccelPolicy {
    #[default]
    Auto,
    D3d11va,
    SoftwareFallback,
}

/// Top-level versioned FrameShift project model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub schema_version: SchemaVersion,
    pub revision_id: u64,
    pub name: String,
    pub assets: Vec<AssetDef>,
    pub timeline: TimelineDef,
    pub events: Vec<EventAnchorDef>,
    pub graph: RenderGraphDef,
    pub render_profiles: RenderProfilesDef,
    #[serde(default)]
    pub param_bindings: Vec<ParamBindingDef>,
    #[serde(default)]
    pub creative_decisions: Vec<CreativeDecisionRecord>,
    #[serde(default)]
    pub preferences: Vec<PreferenceEntry>,
    #[serde(default)]
    pub proxy_config: ProxyConfigDef,
    #[serde(default)]
    pub hw_accel_policy: HwAccelPolicy,
}

impl Project {
    /// Looks up an asset by ID.
    pub fn asset(&self, id: &str) -> Option<&AssetDef> {
        self.assets.iter().find(|a| a.id == id)
    }

    /// Looks up a timeline video clip by ID.
    pub fn clip(&self, id: &str) -> Option<&ClipDef> {
        self.timeline.clips.iter().find(|c| c.id == id)
    }

    /// Looks up an event anchor by `event_id`.
    pub fn event(&self, event_id: &str) -> Option<&EventAnchorDef> {
        self.events.iter().find(|e| e.event_id == event_id)
    }

    /// Validates schema version, asset/clip/event references, timeline bounds,
    /// parameter-binding names, expression parse, and DAG acyclicity.
    ///
    /// On success, returns the topological node execution order of `self.graph`.
    pub fn validate(&self) -> Result<Vec<NodeId>, SchemaError> {
        if !self.schema_version.is_compatible_with_current() {
            return Err(SchemaError::UnsupportedSchemaVersion(self.schema_version));
        }

        if self.timeline.duration_ticks <= Flicks::ZERO {
            return Err(SchemaError::InvalidTimeline(
                "timeline duration_ticks must be > 0".into(),
            ));
        }
        if self.timeline.resolution.width == 0 || self.timeline.resolution.height == 0 {
            return Err(SchemaError::InvalidTimeline(
                "timeline resolution width and height must be non-zero".into(),
            ));
        }
        if self.timeline.fps.num == 0 || self.timeline.fps.den == 0 {
            return Err(SchemaError::InvalidTimeline(
                "timeline fps must be non-zero".into(),
            ));
        }

        let mut asset_ids = HashSet::new();
        for asset in &self.assets {
            if !asset_ids.insert(asset.id.as_str()) {
                return Err(SchemaError::DuplicateId {
                    entity: "asset",
                    id: asset.id.clone(),
                });
            }
        }

        let mut event_ids = HashSet::new();
        for ev in &self.events {
            if !event_ids.insert(ev.event_id.as_str()) {
                return Err(SchemaError::DuplicateId {
                    entity: "event",
                    id: ev.event_id.clone(),
                });
            }
        }

        let topo_order = self.graph.validate_graph()?;
        self.validate_graph_assets(&self.graph)?;

        let mut clip_ids = HashSet::new();
        for clip in &self.timeline.clips {
            if !clip_ids.insert(clip.id.as_str()) {
                return Err(SchemaError::DuplicateId {
                    entity: "clip",
                    id: clip.id.clone(),
                });
            }
            if !asset_ids.contains(clip.asset_id.as_str()) {
                return Err(SchemaError::MissingAsset(clip.asset_id.clone()));
            }
            if clip.timeline_start >= clip.timeline_end {
                return Err(SchemaError::InvalidClipBounds {
                    clip_id: clip.id.clone(),
                    start: clip.timeline_start,
                    end: clip.timeline_end,
                });
            }
            if let Some(ev_id) = &clip.linked_event_id
                && !event_ids.contains(ev_id.as_str())
            {
                return Err(SchemaError::MissingEvent(ev_id.clone()));
            }
            for anim in &clip.event_animations {
                if !event_ids.contains(anim.event_id.as_str()) {
                    return Err(SchemaError::MissingEvent(anim.event_id.clone()));
                }
                if find_node_in_graphs(&self.graph, &anim.target_node_id).is_none() {
                    return Err(SchemaError::MissingNode(anim.target_node_id.clone()));
                }
            }
        }

        for ac in &self.timeline.audio_clips {
            if !clip_ids.insert(ac.id.as_str()) {
                return Err(SchemaError::DuplicateId {
                    entity: "audio clip",
                    id: ac.id.clone(),
                });
            }
            if !asset_ids.contains(ac.asset_id.as_str()) {
                return Err(SchemaError::MissingAsset(ac.asset_id.clone()));
            }
            if ac.timeline_start >= ac.timeline_end {
                return Err(SchemaError::InvalidClipBounds {
                    clip_id: ac.id.clone(),
                    start: ac.timeline_start,
                    end: ac.timeline_end,
                });
            }
            if !ac.gain_db.is_finite() {
                return Err(SchemaError::InvalidTimeline(format!(
                    "audio clip '{}' gain_db must be finite, found {}",
                    ac.id, ac.gain_db
                )));
            }
            let clip_duration = ac.timeline_end - ac.timeline_start;
            for (edge, fade) in [("fade_in", ac.fade_in), ("fade_out", ac.fade_out)] {
                let Some(fade) = fade else { continue };
                // `Flicks` is an `i64`, so a non-finite duration is unrepresentable.
                if fade.duration_ticks < Flicks::ZERO {
                    return Err(SchemaError::InvalidTimeline(format!(
                        "audio clip '{}' {edge} duration_ticks must be >= 0, found {}",
                        ac.id, fade.duration_ticks
                    )));
                }
                if fade.duration_ticks > clip_duration {
                    return Err(SchemaError::InvalidTimeline(format!(
                        "audio clip '{}' {edge} duration_ticks ({}) must not exceed clip duration ({})",
                        ac.id, fade.duration_ticks, clip_duration
                    )));
                }
            }
        }

        for ev in &self.events {
            if let Some(clip_id) = &ev.source_clip_id
                && !clip_ids.contains(clip_id.as_str())
            {
                return Err(SchemaError::MissingClip(clip_id.clone()));
            }
        }

        for binding in &self.param_bindings {
            let Some(node) = find_node_in_graphs(&self.graph, &binding.node_id) else {
                return Err(SchemaError::MissingNode(binding.node_id.clone()));
            };
            let allowed = bindable_param_names(&node.kind);
            if !allowed.contains(&binding.param_name.as_str()) {
                let allowed_list = if allowed.is_empty() {
                    "none".to_owned()
                } else {
                    allowed.join(", ")
                };
                return Err(SchemaError::InvalidParamBinding {
                    node_id: binding.node_id.clone(),
                    param_name: binding.param_name.clone(),
                    message: format!(
                        "not a bindable parameter of this node kind (allowed: {allowed_list})"
                    ),
                });
            }
            if let Err(err) = SafeExpr::parse(&binding.expression) {
                return Err(SchemaError::InvalidParamBinding {
                    node_id: binding.node_id.clone(),
                    param_name: binding.param_name.clone(),
                    message: err.to_string(),
                });
            }
        }

        let mut decision_ids = HashSet::new();
        for decision in &self.creative_decisions {
            if !decision_ids.insert(decision.decision_id.as_str()) {
                return Err(SchemaError::DuplicateId {
                    entity: "creative_decision",
                    id: decision.decision_id.clone(),
                });
            }
        }

        if self.proxy_config.scale_num == 0 || self.proxy_config.scale_den == 0 {
            return Err(SchemaError::InvalidProxyConfig(
                "proxy scale_num and scale_den must be non-zero".into(),
            ));
        }

        Ok(topo_order)
    }

    fn validate_graph_assets(&self, graph: &RenderGraphDef) -> Result<(), SchemaError> {
        for node in &graph.nodes {
            match &node.kind {
                NodeKind::SourceImage { asset_id, .. } => {
                    let Some(asset) = self.asset(asset_id) else {
                        return Err(SchemaError::MissingAsset(asset_id.clone()));
                    };
                    if asset.kind == AssetKind::Audio {
                        return Err(SchemaError::MissingAsset(asset_id.clone()));
                    }
                }
                NodeKind::Precomp { sub_graph, .. } => {
                    self.validate_graph_assets(sub_graph)?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Serializes the project to formatted JSON.
    pub fn to_json_pretty(&self) -> Result<String, SchemaError> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Deserializes and validates a project from JSON.
    pub fn from_json_str(json: &str) -> Result<Self, SchemaError> {
        let project: Self = serde_json::from_str(json)?;
        project.validate()?;
        Ok(project)
    }

    /// Builds a valid M1 "Golden Scene" preset project containing:
    /// - Speed-ramped anime clip A & clip B
    /// - Moving matte mask (`Transform2D` + `MaskMatte`)
    /// - Multi-input shader transition (`ShaderTransition`: whip + chromatic aberration)
    /// - Event-bound flash (`ColorBasic`) anchored to a musical beat/drop
    /// - Sub-frame temporal motion blur (`MotionBlurShutter`)
    /// - 48 kHz audio beat track
    pub fn golden_scene_preset() -> Self {
        let sec = FLICKS_PER_SECOND;
        let beat_tick = Flicks(2 * sec + sec / 2); // 2.5s drop impact
        let total_duration = Flicks(5 * sec);

        let speed_ramp_a = SpeedCurve::cubic_hermite(vec![
            SpeedKeyframe {
                output_offset: Flicks::ZERO,
                speed: 0.5,
                tangent: Some(0.0),
            },
            SpeedKeyframe {
                output_offset: beat_tick,
                speed: 2.5,
                tangent: Some(0.0),
            },
            SpeedKeyframe {
                output_offset: Flicks(3 * sec),
                speed: 1.0,
                tangent: Some(0.0),
            },
        ])
        .expect("golden preset speed curve is valid");

        let speed_ramp_b = SpeedCurve::constant(1.0).expect("constant speed 1.0 is valid");

        let project = Project {
            schema_version: CURRENT_SCHEMA_VERSION,
            revision_id: 1,
            name: "Golden Scene — M1 Vertical Slice".into(),
            assets: vec![
                AssetDef {
                    id: "asset_clip_a".into(),
                    kind: AssetKind::Video,
                    path: "local_media/clip_a.mp4".into(),
                    duration_ticks: Some(Flicks(8 * sec)),
                    fps: Some(FPS_24),
                    sample_rate_hz: None,
                    resolution: Some(Resolution {
                        width: 1920,
                        height: 1080,
                    }),
                },
                AssetDef {
                    id: "asset_clip_b".into(),
                    kind: AssetKind::Video,
                    path: "local_media/clip_b.mp4".into(),
                    duration_ticks: Some(Flicks(8 * sec)),
                    fps: Some(FPS_24),
                    sample_rate_hz: None,
                    resolution: Some(Resolution {
                        width: 1920,
                        height: 1080,
                    }),
                },
                AssetDef {
                    id: "asset_bgm".into(),
                    kind: AssetKind::Audio,
                    path: "local_media/beat_track.wav".into(),
                    duration_ticks: Some(Flicks(10 * sec)),
                    fps: None,
                    sample_rate_hz: Some(AUDIO_48_KHZ),
                    resolution: None,
                },
            ],
            timeline: TimelineDef {
                fps: FPS_60,
                duration_ticks: total_duration,
                resolution: Resolution {
                    width: 1920,
                    height: 1080,
                },
                clips: vec![
                    ClipDef {
                        id: "clip_a".into(),
                        asset_id: "asset_clip_a".into(),
                        timeline_start: Flicks::ZERO,
                        timeline_end: Flicks(3 * sec),
                        source_start: Flicks::ZERO,
                        speed_curve: speed_ramp_a,
                        linked_event_id: Some("drop_impact".into()),
                        event_animations: vec![EventBoundAnimation {
                            id: "anim_impact_flash".into(),
                            event_id: "drop_impact".into(),
                            target_node_id: NodeId::from("grade_flash"),
                            parameter: "flash_amount".into(),
                            offset_from_event: Flicks::ZERO,
                            duration: Flicks(sec / 4),
                            keyframes: vec![
                                AnimationKeyframe {
                                    progress: 0.0,
                                    value: 1.0,
                                    easing: AnimationEasing::EaseOut,
                                },
                                AnimationKeyframe {
                                    progress: 1.0,
                                    value: 0.0,
                                    easing: AnimationEasing::Linear,
                                },
                            ],
                            hold_after: false,
                        }],
                    },
                    ClipDef {
                        id: "clip_b".into(),
                        asset_id: "asset_clip_b".into(),
                        timeline_start: Flicks(2 * sec),
                        timeline_end: total_duration,
                        source_start: Flicks::ZERO,
                        speed_curve: speed_ramp_b,
                        linked_event_id: Some("drop_impact".into()),
                        event_animations: Vec::new(),
                    },
                ],
                audio_clips: vec![AudioClipDef {
                    id: "bgm_main".into(),
                    asset_id: "asset_bgm".into(),
                    timeline_start: Flicks::ZERO,
                    timeline_end: total_duration,
                    source_start: Flicks::ZERO,
                    gain_db: 0.0,
                    fade_in: None,
                    fade_out: None,
                }],
            },
            events: vec![EventAnchorDef {
                event_id: "drop_impact".into(),
                label: "Main Beat Drop / Impact".into(),
                source_clip_id: Some("clip_a".into()),
                source_anchor_tick: Some(Flicks(2 * sec)),
                target_output_tick: beat_tick,
                origin: EventOrigin::UserMarked,
            }],
            graph: RenderGraphDef {
                output_node: NodeId::from("motion_blur_out"),
                nodes: vec![
                    GraphNodeDef {
                        id: NodeId::from("src_a"),
                        kind: NodeKind::SourceVideo {
                            asset_id: "asset_clip_a".into(),
                            clip_id: Some("clip_a".into()),
                            color_space: ColorSpace::DisplaySrgb,
                            fit: SourceFit::Stretch,
                        },
                        inputs: vec![],
                        enabled: true,
                    },
                    GraphNodeDef {
                        id: NodeId::from("src_b"),
                        kind: NodeKind::SourceVideo {
                            asset_id: "asset_clip_b".into(),
                            clip_id: Some("clip_b".into()),
                            color_space: ColorSpace::DisplaySrgb,
                            fit: SourceFit::Stretch,
                        },
                        inputs: vec![],
                        enabled: true,
                    },
                    GraphNodeDef {
                        id: NodeId::from("mask_solid"),
                        kind: NodeKind::Solid {
                            color_rgba: [1.0, 1.0, 1.0, 1.0],
                            color_space: ColorSpace::LinearRec709,
                        },
                        inputs: vec![],
                        enabled: true,
                    },
                    GraphNodeDef {
                        id: NodeId::from("moving_mask_xform"),
                        kind: NodeKind::Transform2D {
                            translate: [0.15, 0.0],
                            scale: [0.65, 1.0],
                            rotation_rad: 0.12,
                            anchor: [0.5, 0.5],
                            filter: SampleFilter::Bilinear,
                        },
                        inputs: vec![NodeId::from("mask_solid")],
                        enabled: true,
                    },
                    GraphNodeDef {
                        id: NodeId::from("masked_a"),
                        kind: NodeKind::MaskMatte {
                            mode: MatteMode::Alpha,
                            invert: false,
                            feather: 0.04,
                            opaque_black_outside: false,
                        },
                        inputs: vec![NodeId::from("src_a"), NodeId::from("moving_mask_xform")],
                        enabled: true,
                    },
                    GraphNodeDef {
                        id: NodeId::from("whip_transition"),
                        kind: NodeKind::ShaderTransition {
                            shader_name: "transition_whip_chromatic".into(),
                            progress: 0.5,
                            direction_rad: 0.0,
                            whip_strength: 0.85,
                            chromatic_aberration: 0.035,
                            impact_flash: 0.4,
                        },
                        inputs: vec![NodeId::from("masked_a"), NodeId::from("src_b")],
                        enabled: true,
                    },
                    GraphNodeDef {
                        id: NodeId::from("overlay_tint"),
                        kind: NodeKind::Solid {
                            color_rgba: [0.08, 0.02, 0.12, 0.25],
                            color_space: ColorSpace::LinearRec709,
                        },
                        inputs: vec![],
                        enabled: true,
                    },
                    GraphNodeDef {
                        id: NodeId::from("blend_atmosphere"),
                        kind: NodeKind::Blend {
                            mode: BlendMode::Screen,
                            opacity: 0.35,
                        },
                        inputs: vec![NodeId::from("whip_transition"), NodeId::from("overlay_tint")],
                        enabled: true,
                    },
                    GraphNodeDef {
                        id: NodeId::from("grade_flash"),
                        kind: NodeKind::ColorBasic {
                            exposure: 0.1,
                            contrast: 1.08,
                            saturation: 1.15,
                            flash_amount: 0.25,
                        },
                        inputs: vec![NodeId::from("blend_atmosphere")],
                        enabled: true,
                    },
                    GraphNodeDef {
                        id: NodeId::from("impact_title"),
                        kind: NodeKind::VectorText {
                            text: "FRAMESHIFT // IMPACT".into(),
                            font_size_norm: 0.065,
                            color_rgba: [1.0, 0.98, 0.95, 1.0],
                            outline_rgba: [0.06, 0.04, 0.12, 0.95],
                            outline_width_norm: 0.004,
                            align_center: true,
                            y_offset_norm: 0.78,
                            font_family: None,
                            font_path: None,
                            font_weight: 400,
                            align: None,
                            x_offset_norm: None,
                            tracking_em: None,
                            line_height_em: None,
                            opacity: 1.0,
                            fallback_fonts: Vec::new(),
                            writing_mode: crate::WritingMode2D::HorizontalTb,
                            var_wght: None,
                            var_wdth: None,
                            var_slnt: None,
                            color_bottom_rgba: None,
                            gradient_angle_deg: 0.0,
                            outer_outline_rgba: [0.0; 4],
                            outer_outline_width_norm: 0.0,
                            outline_gap_norm: 0.0,
                            shadow_offset_norm: [0.0, 0.0],
                            shadow_blur_norm: 0.0,
                            shadow_rgba: [0.0; 4],
                            glow_radius_norm: 0.0,
                            glow_rgba: [0.0; 4],
                            hdr_intensity: 1.0,
                            gpu_slug: true,
                        },
                        inputs: vec![],
                        enabled: true,
                    },
                    GraphNodeDef {
                        id: NodeId::from("blend_title"),
                        kind: NodeKind::Blend {
                            mode: BlendMode::Normal,
                            opacity: 1.0,
                        },
                        inputs: vec![NodeId::from("grade_flash"), NodeId::from("impact_title")],
                        enabled: true,
                    },
                    GraphNodeDef {
                        id: NodeId::from("cyber_lut"),
                        kind: NodeKind::Lut3D {
                            input: NodeId::from("blend_title"),
                            lut_preset: "anime_cyber_contrast".into(),
                            intensity: 0.65,
                            lut_source: None,
                            input_space: None,
                            output_space: None,
                        },
                        inputs: vec![NodeId::from("blend_title")],
                        enabled: true,
                    },
                    GraphNodeDef {
                        id: NodeId::from("motion_blur_out"),
                        kind: NodeKind::MotionBlurShutter {
                            shutter_angle_deg: 180.0,
                            sub_samples: 8,
                            phase: ShutterPhase::Centered,
                            tonemap: true,
                        },
                        inputs: vec![NodeId::from("cyber_lut")],
                        enabled: true,
                    },
                ],
            },
            render_profiles: RenderProfilesDef {
                draft: RenderProfileDef {
                    kind: RenderProfileKind::Draft,
                    resolution: Resolution {
                        width: 1280,
                        height: 720,
                    },
                    motion_blur_sub_samples: 4,
                    shutter_angle_deg: 180.0,
                    pixel_format: WorkingPixelFormat::Rgba16FloatPremul,
                },
                final_profile: RenderProfileDef {
                    kind: RenderProfileKind::Final,
                    resolution: Resolution {
                        width: 1920,
                        height: 1080,
                    },
                    motion_blur_sub_samples: 16,
                    shutter_angle_deg: 180.0,
                    pixel_format: WorkingPixelFormat::Rgba16FloatPremul,
                },
            },
            param_bindings: vec![ParamBindingDef {
                node_id: NodeId::from("grade_flash"),
                param_name: "flash_amount".into(),
                expression: "clamp(0.15 + audio_rms * 0.65 + (1.0 - beat_phase) * 0.2, 0.0, 1.5)"
                    .into(),
            }],
            creative_decisions: vec![CreativeDecisionRecord {
                decision_id: "dec_golden_impact_pacing".into(),
                approach: "Whip-chromatic transition synced to 2.5s beat drop with cyber contrast LUT and VectorText impact title".into(),
                rationale: "Keeps motion blur shutter downstream of grading and typography so fast camera whips streak title and highlights cohesively".into(),
                evidence_ticks: vec![beat_tick],
                protected_invariants: vec![
                    "drop_impact aligned to 2.5s output tick".into(),
                    "Rgba16Float premultiplied alpha pipeline".into(),
                ],
                risks_to_verify: vec![
                    "Verify title legibility during peak whip_chromatic blur".into(),
                ],
            }],
            preferences: vec![PreferenceEntry {
                key: "impact_flash_style".into(),
                value: "crisp_anime_bloom".into(),
                scope: PreferenceScope::Project,
                target_id: None,
            }],
            proxy_config: ProxyConfigDef::default(),
            hw_accel_policy: HwAccelPolicy::Auto,
        };

        project
            .validate()
            .expect("golden_scene_preset must always be valid");
        project
    }

    /// Builds a valid Anime/AMV Post-FX showcase preset demonstrating:
    /// - Built-in shader transitions (`"cross_zoom"`, `"directional_warp"`, `"glitch_displace"`)
    /// - Chained post-FX DAG (`MotionTileShake -> AnimeImpactFrame -> RadialShockwave -> HalftoneGlow`)
    pub fn anime_fx_showcase_preset() -> Self {
        let mut project = Self::golden_scene_preset();
        project.name = "Anime FX Showcase — Post-FX & Transitions".into();

        project.graph = RenderGraphDef {
            output_node: NodeId::from("fx_halftone_glow"),
            nodes: vec![
                GraphNodeDef {
                    id: NodeId::from("src_a"),
                    kind: NodeKind::SourceVideo {
                        asset_id: "asset_clip_a".into(),
                        clip_id: Some("clip_a".into()),
                        color_space: ColorSpace::DisplaySrgb,
                        fit: SourceFit::Stretch,
                    },
                    inputs: vec![],
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("src_b"),
                    kind: NodeKind::SourceVideo {
                        asset_id: "asset_clip_b".into(),
                        clip_id: Some("clip_b".into()),
                        color_space: ColorSpace::DisplaySrgb,
                        fit: SourceFit::Stretch,
                    },
                    inputs: vec![],
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("flash_solid"),
                    kind: NodeKind::Solid {
                        color_rgba: [1.0, 0.96, 0.92, 1.0],
                        color_space: ColorSpace::LinearRec709,
                    },
                    inputs: vec![],
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("trans_cross_zoom"),
                    kind: NodeKind::ShaderTransition {
                        shader_name: "cross_zoom".into(),
                        progress: 0.45,
                        direction_rad: 0.0,
                        whip_strength: 0.4,
                        chromatic_aberration: 0.02,
                        impact_flash: 0.1,
                    },
                    inputs: vec![NodeId::from("src_a"), NodeId::from("src_b")],
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("trans_directional_warp"),
                    kind: NodeKind::ShaderTransition {
                        shader_name: "directional_warp".into(),
                        progress: 0.35,
                        direction_rad: std::f32::consts::FRAC_PI_4,
                        whip_strength: 0.6,
                        chromatic_aberration: 0.0,
                        impact_flash: 0.0,
                    },
                    inputs: vec![
                        NodeId::from("trans_cross_zoom"),
                        NodeId::from("flash_solid"),
                    ],
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("trans_glitch_displace"),
                    kind: NodeKind::ShaderTransition {
                        shader_name: "glitch_displace".into(),
                        progress: 0.5,
                        direction_rad: 0.0,
                        whip_strength: 0.8,
                        chromatic_aberration: 0.04,
                        impact_flash: 0.25,
                    },
                    inputs: vec![
                        NodeId::from("trans_directional_warp"),
                        NodeId::from("src_b"),
                    ],
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("grade_flash"),
                    kind: NodeKind::ColorBasic {
                        exposure: 0.05,
                        contrast: 1.05,
                        saturation: 1.1,
                        flash_amount: 0.2,
                    },
                    inputs: vec![NodeId::from("trans_glitch_displace")],
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("fx_motion_tile_shake"),
                    kind: NodeKind::MotionTileShake {
                        tile_scale: 1.0,
                        mirror_edges: true,
                        shake_amp_norm: [0.025, 0.018],
                        shake_rot_rad: 0.035,
                        shake_zoom: 0.08,
                        shake_freq_hz: 14.0,
                        shake_decay: 4.5,
                        event_delta_sec: 0.05,
                    },
                    inputs: vec![NodeId::from("grade_flash")],
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("fx_anime_impact"),
                    kind: NodeKind::AnimeImpactFrame {
                        luma_threshold: 0.48,
                        edge_weight: 1.2,
                        invert_amount: 0.85,
                        speed_line_density: 0.65,
                        speed_line_intensity: 0.75,
                        center_uv: [0.5, 0.5],
                        mix_amount: 0.9,
                    },
                    inputs: vec![NodeId::from("fx_motion_tile_shake")],
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("fx_radial_shockwave"),
                    kind: NodeKind::RadialShockwave {
                        center_uv: [0.5, 0.5],
                        radius: 0.35,
                        ring_width: 0.12,
                        refraction_amp: 0.06,
                        zoom_blur_strength: 0.22,
                        chromatic_aberration: 0.03,
                    },
                    inputs: vec![NodeId::from("fx_anime_impact")],
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("fx_halftone_glow"),
                    kind: NodeKind::HalftoneGlow {
                        dot_scale: 64.0,
                        angle_rad: std::f32::consts::FRAC_PI_4,
                        halftone_mix: 0.35,
                        glow_threshold: 0.6,
                        glow_intensity: 0.85,
                        glow_radius_px: 14.0,
                        color_mode: HalftoneColorMode::Monochrome,
                        dot_softness: 0.0,
                    },
                    inputs: vec![NodeId::from("fx_radial_shockwave")],
                    enabled: true,
                },
            ],
        };

        project
            .validate()
            .expect("anime_fx_showcase_preset must always be valid");
        project
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hold_after_and_opaque_black_outside_default_false_on_legacy_json() {
        let mut project = Project::golden_scene_preset();
        let mut value = serde_json::to_value(&project).expect("serialize");
        // Strip the new optional fields to simulate legacy JSON
        if let Some(anims) = value["timeline"]["clips"][0]["event_animations"].as_array_mut() {
            for anim in anims {
                anim.as_object_mut().unwrap().remove("hold_after");
            }
        }
        if let Some(nodes) = value["graph"]["nodes"].as_array_mut() {
            for node in nodes {
                if node["kind"]["type"] == "mask_matte" {
                    node["kind"]
                        .as_object_mut()
                        .unwrap()
                        .remove("opaque_black_outside");
                }
            }
        }
        let json = serde_json::to_string(&value).unwrap();
        let decoded = Project::from_json_str(&json).expect("legacy json must deserialize");
        assert!(!decoded.timeline.clips[0].event_animations[0].hold_after);
        let NodeKind::MaskMatte {
            opaque_black_outside,
            ..
        } = decoded.graph.node(&NodeId::from("masked_a")).unwrap().kind
        else {
            panic!("expected MaskMatte");
        };
        assert!(!opaque_black_outside);

        project.timeline.clips[0].event_animations[0].hold_after = true;
        let roundtrip = Project::from_json_str(&project.to_json_pretty().unwrap()).unwrap();
        assert!(roundtrip.timeline.clips[0].event_animations[0].hold_after);
    }

    #[test]
    fn unknown_param_binding_name_is_rejected() {
        let mut project = Project::golden_scene_preset();
        project.param_bindings.push(ParamBindingDef {
            node_id: NodeId::from("grade_flash"),
            param_name: "opacity".into(),
            expression: "1.0".into(),
        });
        match project.validate() {
            Err(SchemaError::InvalidParamBinding {
                node_id,
                param_name,
                message,
            }) => {
                assert_eq!(node_id, NodeId::from("grade_flash"));
                assert_eq!(param_name, "opacity");
                assert!(
                    message.contains("not a bindable parameter"),
                    "unexpected message: {message}"
                );
                assert!(
                    message.contains("flash_amount"),
                    "allowed list should name ColorBasic parameters, got: {message}"
                );
            }
            other => panic!("expected InvalidParamBinding, got {other:?}"),
        }

        let mut shutter = Project::golden_scene_preset();
        shutter.param_bindings.push(ParamBindingDef {
            node_id: NodeId::from("motion_blur_out"),
            param_name: "sub_samples".into(),
            expression: "1.0".into(),
        });
        match shutter.validate() {
            Err(SchemaError::InvalidParamBinding {
                node_id,
                param_name,
                message,
            }) => {
                assert_eq!(node_id, NodeId::from("motion_blur_out"));
                assert_eq!(param_name, "sub_samples");
                assert!(
                    message.contains("allowed: none"),
                    "motion blur has no bindable parameters, got: {message}"
                );
            }
            other => panic!("expected InvalidParamBinding, got {other:?}"),
        }
    }

    #[test]
    fn known_param_binding_names_still_validate() {
        let mut project = Project::golden_scene_preset();
        project.param_bindings.push(ParamBindingDef {
            node_id: NodeId::from("blend_title"),
            param_name: "opacity".into(),
            expression: "step(1.905, t_sec)".into(),
        });
        project.validate().expect("opacity is bindable on Blend");

        let mut showcase = Project::anime_fx_showcase_preset();
        showcase.param_bindings.push(ParamBindingDef {
            node_id: NodeId::from("fx_radial_shockwave"),
            param_name: "center_uv_x".into(),
            expression: "0.5".into(),
        });
        showcase
            .validate()
            .expect("center_uv_x is a bindable shockwave alias");
    }
}
