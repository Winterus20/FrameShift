use crate::error::SchemaError;
use crate::project::AnimationEasing;
use frameshift_time::Flicks;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

/// Maximum allowed nesting depth for `NodeKind::Precomp` sub-graphs.
pub const MAX_PRECOMP_DEPTH: usize = 8;

/// Maximum number of external inputs passed into a single `NodeKind::Precomp` node.
pub const MAX_PRECOMP_INPUTS: usize = 16;

/// Maximum control-mesh dimension (`cols` / `rows`) for `NodeKind::GridWarp`.
pub const MAX_GRID_WARP_DIM: u32 = 8;

/// Unique identifier for a node in the render DAG.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(transparent)]
pub struct NodeId(pub String);

impl NodeId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for NodeId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

/// Color space metadata for source inputs and color transforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorSpace {
    #[default]
    LinearRec709,
    DisplaySrgb,
}

/// Extended color space catalog for `Lut3D` / OCIO color adaptation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtendedColorSpace {
    #[default]
    LinearRec709,
    DisplaySrgb,
    Rec709,
    LinearRec2020,
    Rec2020,
    #[serde(rename = "aces2065_1", alias = "aces_2065_1")]
    Aces2065_1,
    AcesCg,
    AcesCct,
    ArriLogC3,
    SonySLog3,
}

impl From<ColorSpace> for ExtendedColorSpace {
    fn from(cs: ColorSpace) -> Self {
        match cs {
            ColorSpace::LinearRec709 => Self::LinearRec709,
            ColorSpace::DisplaySrgb => Self::DisplaySrgb,
        }
    }
}

/// External LUT or OCIO color transform source for `NodeKind::Lut3D`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LutSourceDef {
    /// Built-in preset name (`"identity"`, `"anime_cyber_contrast"`, `"warm_sunset"`, `"bleach_bypass"`).
    Preset { name: String },
    /// External `.cube` (1D, 3D, or combined 1D+3D) or `.spi1d` / `.spi3d` file.
    CubeFile {
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lut_size_override: Option<u32>,
    },
    /// OpenColorIO `.ocio` config-driven transform baked to 3D LUT.
    Ocio {
        config_path: String,
        src_space: String,
        dst_space: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        look: Option<String>,
        #[serde(default = "default_ocio_lut_size")]
        lut_size: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        shaper_space: Option<String>,
    },
    /// Built-in analytic space-to-space transform (no external file needed).
    BuiltinTransform {
        src_space: ExtendedColorSpace,
        dst_space: ExtendedColorSpace,
        #[serde(default = "default_ocio_lut_size")]
        lut_size: u32,
    },
}

/// Horizontal text alignment for 2D `NodeKind::VectorText` and 3D `NodeKind::Text3D`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextAlign2D {
    Left,
    #[default]
    Center,
    Right,
}

/// Writing direction for 2D `NodeKind::VectorText`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WritingMode2D {
    #[default]
    HorizontalTb,
    VerticalRl,
}

/// 3D bevel edge profile for `NodeKind::Text3D`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BevelProfile3D {
    #[default]
    Flat,
    Round,
    Concave,
    Stepped,
}

/// Cubic Bézier curve segment in normalized `[0, 1]` canvas coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CubicBezierSegmentDef {
    pub p0: [f32; 2],
    pub c1: [f32; 2],
    pub c2: [f32; 2],
    pub p1: [f32; 2],
}

/// Resampling filter used by `Transform2D`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SampleFilter {
    #[default]
    Bilinear,
    CatmullRom,
}

/// Matte interpretation mode for `MaskMatte`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatteMode {
    #[default]
    Alpha,
    Luma,
    Red,
}

/// Channel mapping mode for `NodeKind::DisplacementMap`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplacementMode {
    #[default]
    Rg,
    Luma,
    Polar,
    SignedRg,
}

/// Alias matching design-doc terminology for [`DisplacementMode`].
pub type DisplacementChannelMode = DisplacementMode;

/// Edge sampling policy for `NodeKind::DisplacementMap`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplacementWrap {
    #[default]
    Clamp,
    Mirror,
    Wrap,
    Transparent,
}

/// Alias matching design-doc terminology for [`DisplacementWrap`].
pub type DisplacementEdgeMode = DisplacementWrap;

/// Composite operator for `NodeKind::FrameEcho` ghost trail accumulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EchoOperator {
    #[default]
    CompositeUnder,
    CompositeOver,
    Add,
    Screen,
    Maximum,
}

/// Single control vertex (with relative Bézier handles) for `NodeKind::ShapeMask`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ShapeMaskVertex {
    pub pos: [f32; 2],
    #[serde(default)]
    pub in_tangent: [f32; 2],
    #[serde(default)]
    pub out_tangent: [f32; 2],
}

/// Keyframed shape pose for `NodeKind::ShapeMask` morphing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShapeMaskKeyframe {
    pub tick: Flicks,
    pub vertices: Vec<ShapeMaskVertex>,
    #[serde(default)]
    pub easing: AnimationEasing,
}

/// Text range unit used by the headless `VectorText` animator.
///
/// `Grapheme` follows Unicode combining marks and joiner sequences; when shaping
/// merges multiple units into a ligature, the selected range expands to the full
/// shaped cluster. `Word` groups non-whitespace graphemes in logical text order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextSelectorUnit {
    #[default]
    Grapheme,
    Word,
}

/// Range selector falloff shape for [`TextAnimatorDef`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextRangeShape {
    #[default]
    Square,
    RampUp,
    RampDown,
    Triangle,
    Smooth,
}

/// A deterministic in-animation applied to a logical range of shaped text units.
///
/// Tick values are integer FrameShift Flicks. Selected units start at `position_from_norm`,
/// `scale_from`, `opacity_from`, `rotation_deg_from`, and `tracking_em_from`; each advances
/// toward the normal text style over `duration_ticks`, with `delay_ticks` staggered between
/// adjacent units according to `shape`, `ease_high`, `ease_low`, and optional `randomize_seed`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextAnimatorDef {
    #[serde(default)]
    pub unit: TextSelectorUnit,
    #[serde(default)]
    pub range_start: u32,
    #[serde(default)]
    pub range_end: Option<u32>,
    #[serde(default)]
    pub start_tick: i64,
    #[serde(default)]
    pub duration_ticks: i64,
    #[serde(default)]
    pub delay_ticks: i64,
    #[serde(default)]
    pub position_from_norm: [f32; 2],
    #[serde(default = "default_one")]
    pub scale_from: f32,
    #[serde(default = "default_one")]
    pub opacity_from: f32,
    #[serde(default)]
    pub rotation_deg_from: f32,
    #[serde(default)]
    pub tracking_em_from: f32,
    #[serde(default)]
    pub shape: TextRangeShape,
    #[serde(default)]
    pub ease_high: f32,
    #[serde(default)]
    pub ease_low: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub randomize_seed: Option<u64>,
}

impl Default for TextAnimatorDef {
    fn default() -> Self {
        Self {
            unit: TextSelectorUnit::Grapheme,
            range_start: 0,
            range_end: None,
            start_tick: 0,
            duration_ticks: 0,
            delay_ticks: 0,
            position_from_norm: [0.0, 0.0],
            scale_from: 1.0,
            opacity_from: 1.0,
            rotation_deg_from: 0.0,
            tracking_em_from: 0.0,
            shape: TextRangeShape::Square,
            ease_high: 0.0,
            ease_low: 0.0,
            randomize_seed: None,
        }
    }
}

/// Compositing blend modes for `Blend`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlendMode {
    #[default]
    Normal,
    Add,
    Screen,
    Multiply,
    Overlay,
    SoftLight,
    ColorDodge,
    HardLight,
    Lighten,
    Darken,
}

/// Screentone / halftone color mode for `NodeKind::HalftoneGlow`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HalftoneColorMode {
    #[default]
    Monochrome,
    Cmyk,
    ColorDot,
}

/// Temporal motion blur shutter phase relative to the output frame timestamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShutterPhase {
    #[default]
    Centered,
    Trailing,
}

/// How a `SourceVideo` frame is mapped into the canvas.
///
/// `Stretch` is the default and fills the canvas without preserving aspect.
/// `Cover` fills the canvas and crops overflow. `Contain` fits inside the canvas.
/// Decode must read the stored mode through [`NodeKind::source_fit`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceFit {
    #[default]
    Stretch,
    Cover,
    Contain,
}

/// Image a disabled node copies instead of running its operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisabledPassthrough {
    /// The node has no image input. The compositor clears to transparent black.
    Clear,
    /// Copy this input. Index `0` is the only input, or the base of a 2-input node.
    Input(usize),
}

/// Camera orientation mode for unified `Scene3D` rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Camera3DMode {
    /// Uses `rot_deg` as XYZ Euler orientation (matches Faz 6.2 `build_camera_matrices`).
    #[default]
    Euler,
    /// Aims camera at `target` and applies `rot_deg[2]` as Dutch roll.
    LookAt,
}

/// Shared camera & Z-buffer Bokeh DoF configuration for `NodeKind::Scene3D`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SceneDepthPlateDef {
    /// Upstream 2D node producing the grayscale depth map (`1.0` = near, `0.0` = far).
    pub node_id: NodeId,
    /// Eye-space near distance corresponding to `depth = 1.0`.
    #[serde(default = "default_depth_plate_near")]
    pub near_z: f32,
    /// Eye-space far distance corresponding to `depth = 0.0`.
    #[serde(default = "default_depth_plate_far")]
    pub far_z: f32,
    /// Invert depth polarity (`true` if `0.0` = near, `1.0` = far).
    #[serde(default)]
    pub invert: bool,
    /// Non-linear gamma shaping applied to normalized depth (`pow(d, depth_gamma)`, default `1.0`).
    #[serde(default = "default_one")]
    pub depth_gamma: f32,
    /// Maps normalized depth in inverse-Z disparity space (`1 / mix(1/far_z, 1/near_z, d)`)
    /// matching Depth Anything V2 output when `true` (default), or linear eye-Z when `false`.
    #[serde(default = "default_true")]
    pub disparity_mode: bool,
    /// Decodes 16-bit RG16-packed depth (`R = hi, G = lo`) instead of 8-bit R/luma when `true`.
    #[serde(default)]
    pub rg16_packed: bool,
}

impl Default for SceneDepthPlateDef {
    fn default() -> Self {
        Self {
            node_id: NodeId::from(""),
            near_z: default_depth_plate_near(),
            far_z: default_depth_plate_far(),
            invert: false,
            depth_gamma: 1.0,
            disparity_mode: true,
            rg16_packed: false,
        }
    }
}

/// Anime4K v4.x FastS multi-pass preset chains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Anime4KPreset {
    /// `Clamp_Highlights` -> `Restore_CNN_S` -> `Upscale_CNN_x2_S` (2x->1x supersampling).
    #[default]
    ModeA,
    /// `Clamp_Highlights` -> `Restore_CNN_Soft_S` -> `Upscale_CNN_x2_S` (720p/aliased source).
    ModeB,
    /// `Clamp_Highlights` -> `Upscale_Denoise_CNN_x2_S` (480p/noisy source).
    ModeC,
    /// `Clamp_Highlights` -> `Restore_CNN_S` (1:1 restoration without 2x upscale).
    RestoreOnly,
    /// `Clamp_Highlights` -> `Darken_HQ` -> `Thin_HQ` (line-art darkening & thinning).
    LineArt,
}

fn default_depth_plate_near() -> f32 {
    0.5
}

fn default_depth_plate_far() -> f32 {
    6.0
}

/// Shared camera & Z-buffer Bokeh DoF configuration for `NodeKind::Scene3D`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Camera3DDef {
    #[serde(default = "default_camera_pos")]
    pub pos: [f32; 3],
    /// Euler XYZ degrees (`Euler` mode), or `rot_deg[2]` = roll degrees (`LookAt` mode).
    #[serde(default)]
    pub rot_deg: [f32; 3],
    #[serde(default)]
    pub target: [f32; 3],
    #[serde(default)]
    pub mode: Camera3DMode,
    #[serde(default = "default_fov_deg")]
    pub fov_deg: f32,
    #[serde(default = "default_near_plane")]
    pub near: f32,
    #[serde(default = "default_far_plane")]
    pub far: f32,
    #[serde(default = "default_dof_focal_dist")]
    pub dof_focal_dist: f32,
    /// When true in `LookAt` mode, overrides `dof_focal_dist` with `length(target - pos)`.
    #[serde(default)]
    pub dof_autofocus_target: bool,
    #[serde(default)]
    pub dof_blur_strength: f32,
    #[serde(default = "default_dof_max_radius_px")]
    pub dof_max_radius_px: f32,
    #[serde(default = "default_dof_samples")]
    pub dof_samples: u32,
    #[serde(default = "default_dof_highlight_boost")]
    pub dof_highlight_boost: f32,
    /// Aperture blade count for polygonal bokeh (`0` = circular, `6` = hexagon, `8` = octagon).
    #[serde(default)]
    pub dof_aperture_blades: u32,
    /// Aperture blade rotation in degrees.
    #[serde(default)]
    pub dof_blade_rotation_deg: f32,
    /// Polygonal blade curvature `[0.0..1.0]` (`0.0` = straight blades, `1.0` = circular).
    #[serde(default = "default_dof_blade_roundness")]
    pub dof_blade_roundness: f32,
    /// Anamorphic bokeh aspect squeeze (`1.0` = spherical, `0.5` = 2:1 vertical oval).
    #[serde(default = "default_one")]
    pub dof_anamorphic_ratio: f32,
    /// Cat's-eye optical vignetting toward frame corners `[0.0..1.0]`.
    #[serde(default)]
    pub dof_cats_eye_strength: f32,
}

impl Default for Camera3DDef {
    fn default() -> Self {
        Self {
            pos: default_camera_pos(),
            rot_deg: [0.0, 0.0, 0.0],
            target: [0.0, 0.0, 0.0],
            mode: Camera3DMode::Euler,
            fov_deg: default_fov_deg(),
            near: default_near_plane(),
            far: default_far_plane(),
            dof_focal_dist: default_dof_focal_dist(),
            dof_autofocus_target: false,
            dof_blur_strength: 0.0,
            dof_max_radius_px: default_dof_max_radius_px(),
            dof_samples: default_dof_samples(),
            dof_highlight_boost: default_dof_highlight_boost(),
            dof_aperture_blades: 0,
            dof_blade_rotation_deg: 0.0,
            dof_blade_roundness: default_dof_blade_roundness(),
            dof_anamorphic_ratio: 1.0,
            dof_cats_eye_strength: 0.0,
        }
    }
}

/// Procedural 3D primitive kind generated by `NodeKind::Shape3D`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shape3DKind {
    NeonTile,
    #[serde(alias = "hollow_star_5")]
    HollowStar5,
    #[serde(alias = "heart_3d")]
    Heart3D,
    LightningBolt,
    #[serde(alias = "diamond_3d")]
    Diamond3D,
}

/// Node operation and parameter payload inside the render DAG.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NodeKind {
    /// Decodes a video asset or timeline clip at the evaluated source tick (0 inputs).
    SourceVideo {
        asset_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        clip_id: Option<String>,
        #[serde(default)]
        color_space: ColorSpace,
        /// Omitted JSON stays [`SourceFit::Stretch`], which keeps the current full-frame scale.
        #[serde(default)]
        fit: SourceFit,
    },
    /// Decodes a still image asset (PNG/JPEG/WebP/TIFF/BMP) into Linear Rec.709 premultiplied RGBA (0 inputs).
    #[serde(alias = "source_image")]
    SourceImage {
        asset_id: String,
        #[serde(default = "default_display_srgb")]
        color_space: ColorSpace,
        #[serde(default)]
        fit: SourceFit,
        #[serde(default = "default_one")]
        opacity: f32,
    },
    /// Generates a uniform RGBA premultiplied solid color (0 inputs).
    Solid {
        color_rgba: [f32; 4],
        #[serde(default)]
        color_space: ColorSpace,
    },
    /// 2D affine spatial transformation (1 input).
    #[serde(alias = "transform_2d")]
    Transform2D {
        translate: [f32; 2],
        scale: [f32; 2],
        rotation_rad: f32,
        anchor: [f32; 2],
        #[serde(default)]
        filter: SampleFilter,
    },
    /// Applies a matte texture (`inputs[1]`) to a source texture (`inputs[0]`) (2 inputs).
    MaskMatte {
        #[serde(default)]
        mode: MatteMode,
        #[serde(default)]
        invert: bool,
        #[serde(default)]
        feather: f32,
        #[serde(default)]
        opaque_black_outside: bool,
        #[serde(default = "default_one")]
        opacity: f32,
        #[serde(default)]
        choke_px: f32,
        #[serde(default)]
        spatial_feather_px: f32,
    },
    /// Composites top (`inputs[1]`) over base (`inputs[0]`) in premultiplied alpha (2 inputs).
    Blend {
        #[serde(default)]
        mode: BlendMode,
        #[serde(default = "default_one")]
        opacity: f32,
    },
    /// Basic color grading and impact flash boost (1 input).
    ColorBasic {
        #[serde(default)]
        exposure: f32,
        #[serde(default = "default_one")]
        contrast: f32,
        #[serde(default = "default_one")]
        saturation: f32,
        #[serde(default)]
        flash_amount: f32,
    },
    /// Multi-input custom WGSL transition between `inputs[0]` (A) and `inputs[1]` (B) (2 inputs).
    ///
    /// Built-in `shader_name` values listed in [`NodeKind::BUILTIN_TRANSITION_SHADERS`]:
    /// `"whip_chromatic"`, `"cross_zoom"`, `"directional_warp"`, `"glitch_displace"`,
    /// `"dreamy_zoom"`, `"swirl"`, `"linear_blur"`, `"burn"`, `"ripple"`, and `"kaleidoscope"`.
    ShaderTransition {
        shader_name: String,
        progress: f32,
        #[serde(default)]
        direction_rad: f32,
        #[serde(default)]
        whip_strength: f32,
        #[serde(default)]
        chromatic_aberration: f32,
        #[serde(default)]
        impact_flash: f32,
    },
    /// Sub-frame temporal motion blur accumulator over the shutter interval (1 input).
    MotionBlurShutter {
        shutter_angle_deg: f32,
        sub_samples: u32,
        #[serde(default)]
        phase: ShutterPhase,
        #[serde(default = "default_true")]
        tonemap: bool,
    },
    /// Premultiplied-alpha vector text / typography generator (0 inputs).
    VectorText {
        text: String,
        font_size_norm: f32,
        color_rgba: [f32; 4],
        #[serde(default)]
        outline_rgba: [f32; 4],
        #[serde(default)]
        outline_width_norm: f32,
        #[serde(default = "default_true")]
        align_center: bool,
        #[serde(default)]
        y_offset_norm: f32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        font_family: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        font_path: Option<String>,
        #[serde(default = "default_font_weight")]
        font_weight: u16,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        align: Option<TextAlign2D>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        x_offset_norm: Option<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tracking_em: Option<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        line_height_em: Option<f32>,
        #[serde(default = "default_one")]
        opacity: f32,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        fallback_fonts: Vec<String>,
        #[serde(default)]
        writing_mode: WritingMode2D,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        var_wght: Option<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        var_wdth: Option<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        var_slnt: Option<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        color_bottom_rgba: Option<[f32; 4]>,
        #[serde(default)]
        gradient_angle_deg: f32,
        #[serde(default)]
        outer_outline_rgba: [f32; 4],
        #[serde(default)]
        outer_outline_width_norm: f32,
        #[serde(default)]
        outline_gap_norm: f32,
        #[serde(default)]
        shadow_offset_norm: [f32; 2],
        #[serde(default)]
        shadow_blur_norm: f32,
        #[serde(default)]
        shadow_rgba: [f32; 4],
        #[serde(default)]
        glow_radius_norm: f32,
        #[serde(default)]
        glow_rgba: [f32; 4],
        #[serde(default = "default_one")]
        hdr_intensity: f32,
        #[serde(default = "default_true")]
        gpu_slug: bool,
        /// Optional grapheme/word range animator, evaluated at the current output tick.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text_animator: Option<TextAnimatorDef>,
    },
    /// Anti-aliased 2D vector path & HUD overlay generator with trim-path, dashes,
    /// node junction dots, and numeric data readout (0 inputs).
    #[serde(alias = "vector_path_hud")]
    VectorPathHud {
        #[serde(default)]
        points_norm: Vec<[f32; 2]>,
        #[serde(default)]
        bezier_segments: Vec<CubicBezierSegmentDef>,
        #[serde(default)]
        closed: bool,
        #[serde(default = "default_hud_stroke_width")]
        stroke_width_norm: f32,
        #[serde(default = "default_white_rgba")]
        stroke_rgba: [f32; 4],
        #[serde(default)]
        fill_rgba: [f32; 4],
        #[serde(default)]
        dash_len_norm: f32,
        #[serde(default)]
        gap_len_norm: f32,
        #[serde(default)]
        dash_offset_norm: f32,
        #[serde(default)]
        trim_start: f32,
        #[serde(default = "default_one")]
        trim_end: f32,
        #[serde(default)]
        node_dot_radius_norm: f32,
        #[serde(default)]
        data_value: f32,
        #[serde(default)]
        data_label: String,
        #[serde(default = "default_one")]
        opacity: f32,
    },
    /// Trilinear 3D LUT color transform / OCIO-compatible node (1 input).
    #[serde(alias = "lut_3d")]
    Lut3D {
        input: NodeId,
        /// Legacy / quick preset or inline/path string (kept for 100% backward compatibility).
        #[serde(default)]
        lut_preset: String,
        #[serde(default = "default_one")]
        intensity: f32,
        /// Structured LUT / OCIO source; takes precedence over `lut_preset` when `Some`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lut_source: Option<LutSourceDef>,
        /// Optional explicit input color space declaration for DAG color adaptation.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        input_space: Option<ExtendedColorSpace>,
        /// Optional explicit output color space declaration for DAG color adaptation.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output_space: Option<ExtendedColorSpace>,
    },
    /// Motion-tile edge mirroring and damped harmonic camera shake post-FX (1 input).
    MotionTileShake {
        #[serde(default = "default_one")]
        tile_scale: f32,
        #[serde(default = "default_true")]
        mirror_edges: bool,
        #[serde(default)]
        shake_amp_norm: [f32; 2],
        #[serde(default)]
        shake_rot_rad: f32,
        #[serde(default)]
        shake_zoom: f32,
        #[serde(default = "default_shake_freq")]
        shake_freq_hz: f32,
        #[serde(default = "default_shake_decay")]
        shake_decay: f32,
        #[serde(default)]
        event_delta_sec: f32,
    },
    /// Anime impact frame with Sobel contours, luma threshold/invert, and polar speed lines (1 input).
    AnimeImpactFrame {
        #[serde(default = "default_half")]
        luma_threshold: f32,
        #[serde(default = "default_one")]
        edge_weight: f32,
        #[serde(default = "default_one")]
        invert_amount: f32,
        #[serde(default = "default_speed_density")]
        speed_line_density: f32,
        #[serde(default = "default_half")]
        speed_line_intensity: f32,
        #[serde(default = "default_center")]
        center_uv: [f32; 2],
        #[serde(default)]
        mix_amount: f32,
    },
    /// Aspect-corrected radial shockwave refraction, zoom blur, and RGB dispersion (1 input).
    RadialShockwave {
        #[serde(default = "default_center")]
        center_uv: [f32; 2],
        #[serde(default = "default_quarter")]
        radius: f32,
        #[serde(default = "default_tenth")]
        ring_width: f32,
        #[serde(default)]
        refraction_amp: f32,
        #[serde(default)]
        zoom_blur_strength: f32,
        #[serde(default)]
        chromatic_aberration: f32,
    },
    /// Rotated 2D SDF screentone/halftone (Monochrome, CMYK, or ColorDot) combined with
    /// Linear Rec.709 HDR soft-knee multi-ring glow (1 input).
    HalftoneGlow {
        #[serde(default = "default_dot_scale")]
        dot_scale: f32,
        #[serde(default)]
        angle_rad: f32,
        #[serde(default)]
        halftone_mix: f32,
        #[serde(default = "default_half")]
        glow_threshold: f32,
        #[serde(default)]
        glow_intensity: f32,
        #[serde(default = "default_glow_radius")]
        glow_radius_px: f32,
        #[serde(default)]
        color_mode: HalftoneColorMode,
        #[serde(default)]
        dot_softness: f32,
    },
    /// Jimenez 2014 multi-pass mip-pyramid HDR bloom / deep glow (1 input).
    DeepGlow {
        #[serde(default = "default_deep_glow_threshold")]
        threshold: f32,
        #[serde(default = "default_half")]
        knee: f32,
        #[serde(default = "default_deep_glow_intensity")]
        intensity: f32,
        #[serde(default = "default_one")]
        radius: f32,
        #[serde(default = "default_white_rgb")]
        tint_rgb: [f32; 3],
        #[serde(default = "default_mip_levels")]
        mip_levels: u32,
        #[serde(default = "default_bloom_scatter")]
        scatter: f32,
        #[serde(default)]
        anamorphic_ratio: f32,
        #[serde(default)]
        chromatic_shift: f32,
    },
    /// Anime4K-inspired dark-line thinning and anti-ringing sharpen (1 input).
    AnimeSharpen {
        #[serde(default = "default_anime_line_strength")]
        line_strength: f32,
        #[serde(default = "default_anime_sharpen_amount")]
        sharpen_amount: f32,
        #[serde(default = "default_anime_anti_ringing")]
        anti_ringing: f32,
    },
    /// Anime4K v4.x FastS multi-pass WGSL CNN restoration, 2x->1x supersampling, and line-art
    /// enhancement (`ModeA`, `ModeB`, `ModeC`, `RestoreOnly`, `LineArt`) (1 input).
    #[serde(rename = "anime4k", alias = "anime4_k", alias = "Anime4K")]
    Anime4K {
        #[serde(default)]
        preset: Anime4KPreset,
        #[serde(default = "default_one")]
        strength: f32,
        #[serde(default = "default_true")]
        clamp_highlights: bool,
    },
    /// Cinematic lens distortion, chromatic edge, film grain, halation, and vignette (1 input).
    FilmOptics {
        #[serde(default)]
        lens_k1: f32,
        #[serde(default)]
        lens_k2: f32,
        #[serde(default = "default_chromatic_edge")]
        chromatic_edge: f32,
        #[serde(default = "default_grain_amount")]
        grain_amount: f32,
        #[serde(default = "default_one")]
        grain_seed: f32,
        #[serde(default = "default_halation_amount")]
        halation_amount: f32,
        #[serde(default = "default_vignette_amount")]
        vignette_amount: f32,
        #[serde(default = "default_true")]
        auto_scale: bool,
        #[serde(default = "default_grain_size_px")]
        grain_size_px: f32,
        #[serde(default = "default_grain_chroma")]
        grain_chroma: f32,
        #[serde(default = "default_halation_threshold")]
        halation_threshold: f32,
    },
    /// User-authored WGSL fragment shader sandboxed with up to 2 inputs and 16 scalar params.
    CustomWgsl {
        #[serde(default)]
        wgsl_source: String,
        #[serde(default)]
        expected_inputs: u8,
        #[serde(default)]
        params: [f32; 16],
    },
    /// 3D perspective camera spatial transform with optional Depth-of-Field metadata (1 input).
    #[serde(alias = "transform_3d")]
    Transform3D {
        #[serde(default)]
        pos: [f32; 3],
        #[serde(default)]
        rot_deg: [f32; 3],
        #[serde(default = "default_white_rgb")]
        scale: [f32; 3],
        #[serde(default = "default_fov_deg")]
        fov_deg: f32,
        #[serde(default = "default_camera_pos")]
        camera_pos: [f32; 3],
        #[serde(default)]
        camera_rot_deg: [f32; 3],
        #[serde(default = "default_dof_focal_dist")]
        dof_focal_dist: f32,
        #[serde(default)]
        dof_blur_strength: f32,
        #[serde(default)]
        mirror_edges: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent: Option<NodeId>,
        #[serde(default)]
        pivot: [f32; 3],
    },
    /// Extruded 3D kinetic typography generator using an external TTF/OTF font (0 inputs).
    #[serde(alias = "text_3d")]
    Text3D {
        #[serde(default)]
        text: String,
        #[serde(default)]
        font_path: String,
        #[serde(default = "default_text3d_font_size")]
        font_size: f32,
        #[serde(default = "default_text3d_extrude_depth")]
        extrude_depth: f32,
        #[serde(default = "default_text3d_bevel_size")]
        bevel_size: f32,
        #[serde(default = "default_text3d_color_top")]
        color_top_rgba: [f32; 4],
        #[serde(default = "default_text3d_color_bottom")]
        color_bottom_rgba: [f32; 4],
        #[serde(default = "default_text3d_side_color")]
        side_color_rgba: [f32; 4],
        #[serde(default)]
        pos: [f32; 3],
        #[serde(default)]
        rot_deg: [f32; 3],
        #[serde(default = "default_camera_pos")]
        camera_pos: [f32; 3],
        #[serde(default)]
        camera_rot_deg: [f32; 3],
        #[serde(default = "default_fov_deg")]
        fov_deg: f32,
        #[serde(default = "default_char_stagger_sec")]
        char_stagger_sec: f32,
        #[serde(default = "default_char_bounce_amp")]
        char_bounce_amp: f32,
        #[serde(default = "default_char_wave_freq")]
        char_wave_freq: f32,
        #[serde(default)]
        time_offset_sec: f32,
        #[serde(default)]
        outline_width: f32,
        #[serde(default = "default_white_rgba")]
        outline_color_rgba: [f32; 4],
        #[serde(default)]
        material_mode: u32,
        #[serde(default)]
        color_mode: u32,
        #[serde(default)]
        palette_rgba: Vec<[f32; 4]>,
        #[serde(default)]
        pop_duration_sec: f32,
        #[serde(default = "default_pop_overshoot")]
        pop_overshoot: f32,
        #[serde(default)]
        spin_in_deg: f32,
        #[serde(default)]
        char_jitter_deg: f32,
        #[serde(default)]
        arc_curvature: f32,
        #[serde(default)]
        arc_axis: u32,
        #[serde(default)]
        echo_count: u32,
        #[serde(default = "default_echo_offset")]
        echo_offset: [f32; 3],
        #[serde(default = "default_echo_alpha_decay")]
        echo_alpha_decay: f32,
        #[serde(default = "default_echo_scale_step")]
        echo_scale_step: f32,
        #[serde(default = "default_one")]
        hdr_intensity: f32,
        #[serde(default = "default_one")]
        opacity: f32,
        #[serde(default)]
        align: TextAlign2D,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        line_height_em: Option<f32>,
        #[serde(default)]
        tracking_em: f32,
        #[serde(default)]
        bevel_profile: BevelProfile3D,
        #[serde(default = "default_bevel_segments")]
        bevel_segments: u32,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        fallback_fonts: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        var_wght: Option<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        var_wdth: Option<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        var_slnt: Option<f32>,
    },
    /// Procedural extruded 3D shape primitive (`NeonTile`, `HollowStar5`, `Heart3D`, `LightningBolt`, `Diamond3D`) (0 inputs).
    #[serde(alias = "shape_3d")]
    Shape3D {
        shape_kind: Shape3DKind,
        #[serde(default = "default_one")]
        size: f32,
        #[serde(default = "default_one")]
        aspect: f32,
        #[serde(default = "default_shape_stroke")]
        stroke_width: f32,
        #[serde(default)]
        corner_radius: f32,
        #[serde(default = "default_star_inner_ratio")]
        inner_ratio: f32,
        #[serde(default = "default_text3d_extrude_depth")]
        extrude_depth: f32,
        #[serde(default = "default_text3d_bevel_size")]
        bevel_size: f32,
        #[serde(default)]
        material_mode: u32,
        #[serde(default = "default_text3d_color_top")]
        color_top_rgba: [f32; 4],
        #[serde(default = "default_text3d_color_bottom")]
        color_bottom_rgba: [f32; 4],
        #[serde(default = "default_text3d_side_color")]
        side_color_rgba: [f32; 4],
        #[serde(default = "default_white_rgba")]
        outline_color_rgba: [f32; 4],
        #[serde(default)]
        pos: [f32; 3],
        #[serde(default)]
        rot_deg: [f32; 3],
        #[serde(default = "default_white_rgb")]
        scale: [f32; 3],
        #[serde(default = "default_camera_pos")]
        camera_pos: [f32; 3],
        #[serde(default)]
        camera_rot_deg: [f32; 3],
        #[serde(default = "default_fov_deg")]
        fov_deg: f32,
        #[serde(default)]
        pop_duration_sec: f32,
        #[serde(default = "default_pop_overshoot")]
        pop_overshoot: f32,
        #[serde(default)]
        spin_in_deg: f32,
        #[serde(default)]
        echo_count: u32,
        #[serde(default = "default_echo_offset")]
        echo_offset: [f32; 3],
        #[serde(default = "default_echo_alpha_decay")]
        echo_alpha_decay: f32,
        #[serde(default = "default_one")]
        hdr_intensity: f32,
        #[serde(default)]
        time_offset_sec: f32,
        #[serde(default = "default_one")]
        opacity: f32,
    },
    /// Seek-independent analytical 3D particle field (bokeh, stars, diamonds, hearts) (0 inputs).
    #[serde(alias = "particle_field_3d")]
    ParticleField3D {
        #[serde(default = "default_particle_count")]
        count: u32,
        #[serde(default = "default_one_u32")]
        seed: u32,
        #[serde(default = "default_one_u32")]
        shape: u32,
        #[serde(default = "default_particle_color_a")]
        color_a_rgba: [f32; 4],
        #[serde(default = "default_particle_color_b")]
        color_b_rgba: [f32; 4],
        #[serde(default = "default_particle_speed")]
        speed: f32,
        #[serde(default = "default_particle_spread")]
        spread: f32,
        #[serde(default = "default_particle_size")]
        size: f32,
        #[serde(default = "default_camera_pos")]
        camera_pos: [f32; 3],
        #[serde(default)]
        camera_rot_deg: [f32; 3],
        #[serde(default = "default_fov_deg")]
        fov_deg: f32,
        #[serde(default)]
        time_offset_sec: f32,
        #[serde(default = "default_dof_focal_dist")]
        dof_focal_dist: f32,
        #[serde(default = "default_particle_dof_blur")]
        dof_blur_strength: f32,
        #[serde(default = "default_one")]
        opacity: f32,
    },
    /// Seek-independent 3D neon spline ribbon trails with trim-path controls (0 inputs).
    #[serde(alias = "ribbon_trail_3d")]
    RibbonTrail3D {
        #[serde(default = "default_ribbon_count")]
        ribbon_count: u32,
        #[serde(default = "default_ribbon_segments")]
        segments: u32,
        #[serde(default = "default_one_u32")]
        seed: u32,
        #[serde(default = "default_ribbon_width")]
        width: f32,
        #[serde(default = "default_one")]
        speed: f32,
        #[serde(default = "default_ribbon_radius")]
        radius: f32,
        #[serde(default)]
        trim_start: f32,
        #[serde(default = "default_one")]
        trim_end: f32,
        #[serde(default = "default_ribbon_color_a")]
        color_a_rgba: [f32; 4],
        #[serde(default = "default_ribbon_color_b")]
        color_b_rgba: [f32; 4],
        #[serde(default = "default_camera_pos")]
        camera_pos: [f32; 3],
        #[serde(default)]
        camera_rot_deg: [f32; 3],
        #[serde(default = "default_fov_deg")]
        fov_deg: f32,
        #[serde(default)]
        time_offset_sec: f32,
        #[serde(default)]
        ribbon_style: u32,
        #[serde(default = "default_strand_spacing")]
        strand_spacing: f32,
        #[serde(default = "default_curve_bend")]
        curve_bend: f32,
        #[serde(default)]
        pos: [f32; 3],
        #[serde(default)]
        rot_deg: [f32; 3],
        #[serde(default = "default_white_rgb")]
        scale: [f32; 3],
        #[serde(default = "default_one")]
        opacity: f32,
        /// Linear blend toward white inside the HDR core (`0.0` keeps the authored hue,
        /// `0.6` reproduces the legacy white-hot core).
        #[serde(default)]
        core_whiten: f32,
        /// HDR multiplier on the core falloff (`1.0` = neutral, `0.0` = halo only).
        #[serde(default = "default_one")]
        core_intensity: f32,
        /// Multiplier on the wide halo falloff (`1.0` = neutral, `0.0` = core only).
        #[serde(default = "default_one")]
        halo_intensity: f32,
        /// Rec.709 luma saturation applied before HDR gain (`1.0` = identity, `0.0` = grayscale).
        #[serde(default = "default_one")]
        saturation: f32,
        /// Screen-space minimum ribbon width in pixels (`0.0` disables the clamp).
        #[serde(default)]
        min_width_px: f32,
        /// Opt into premultiplied-alpha blending instead of the default additive neon accumulation.
        #[serde(default)]
        normal_blend: bool,
    },
    /// Shared 3D scene pass combining 3D objects (`inputs[0..N]`) under one unified `Camera3D`,
    /// one shared `Depth32Float` buffer, and optional post-process Z-buffer Bokeh DoF.
    #[serde(alias = "scene_3d")]
    Scene3D {
        #[serde(default)]
        camera: Camera3DDef,
        /// Background clear color in Linear Rec.709 premultiplied alpha (default transparent).
        #[serde(default)]
        background_rgba: [f32; 4],
        /// Optional 2D depth plate (e.g. from `Depth-Anything-V2-Small`) seeded into the
        /// `Scene3D` `Depth32Float` buffer before drawing 3D objects.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        depth_plate: Option<SceneDepthPlateDef>,
    },
    /// 4-corner projective perspective warp in normalized `[0, 1]` canvas coordinates (1 input).
    #[serde(alias = "corner_pin")]
    CornerPin {
        #[serde(default = "default_corner_tl")]
        tl: [f32; 2],
        #[serde(default = "default_corner_tr")]
        tr: [f32; 2],
        #[serde(default = "default_corner_br")]
        br: [f32; 2],
        #[serde(default = "default_corner_bl")]
        bl: [f32; 2],
        #[serde(default)]
        filter: SampleFilter,
        #[serde(default = "default_one")]
        opacity: f32,
    },
    /// 2D control-mesh & harmonic wave UV deformation node (1 input).
    #[serde(alias = "grid_warp")]
    GridWarp {
        #[serde(default = "default_grid_dim")]
        cols: u32,
        #[serde(default = "default_grid_dim")]
        rows: u32,
        #[serde(default)]
        control_offsets: Vec<[f32; 2]>,
        #[serde(default)]
        wave_amp: [f32; 2],
        #[serde(default = "default_wave_freq")]
        wave_freq: [f32; 2],
        #[serde(default)]
        wave_phase: f32,
        #[serde(default = "default_one")]
        mix_amount: f32,
        #[serde(default)]
        filter: SampleFilter,
        #[serde(default = "default_one")]
        opacity: f32,
    },
    /// Single-input 2D backdrop scrim: separable blur of the backdrop plus an SDF rounded-rect
    /// plate composited over it (1 input). Works on any 2D source, including a `Scene3D` output.
    BackdropScrim {
        /// Rounded-rect center in normalized `[0, 1]` canvas coordinates.
        #[serde(default = "default_center")]
        rect_center_norm: [f32; 2],
        /// Rounded-rect full width/height in normalized canvas units.
        #[serde(default = "default_scrim_rect_size")]
        rect_size_norm: [f32; 2],
        /// Corner radius as a fraction of canvas height.
        #[serde(default = "default_scrim_corner_radius")]
        corner_radius_norm: f32,
        /// Blur radius as a fraction of canvas height (`0.0` skips the blur passes).
        #[serde(default = "default_scrim_blur_radius")]
        blur_radius_norm: f32,
        /// Straight-alpha plate fill; the shader premultiplies it before source-over.
        #[serde(default = "default_scrim_fill")]
        fill_rgba: [f32; 4],
        /// Plate alpha multiplier `[0, 1]`; the backdrop blur stays active while
        /// `blur_radius_norm > 0`, so `0.0` yields an untinted frosted plate.
        #[serde(default = "default_one")]
        opacity: f32,
        /// `0.0` = off; brightens the plate alpha where the blurred backdrop is bright.
        #[serde(default)]
        auto_contrast: f32,
    },
    /// Encapsulated nested render sub-graph with independent time offset/scale and opacity (0..=16 inputs).
    #[serde(alias = "precomp")]
    Precomp {
        sub_graph: RenderGraphDef,
        #[serde(default)]
        time_offset_ticks: i64,
        #[serde(default = "default_one")]
        time_scale: f32,
        #[serde(default = "default_one")]
        opacity: f32,
    },
    /// Reads an external input slot (`inputs[slot]`) passed into a `Precomp` node (0 inputs inside `sub_graph`).
    #[serde(alias = "precomp_input")]
    PrecompInput {
        #[serde(default)]
        slot: usize,
    },
    /// Time-bounded adjustment layer composited over its upstream input (1 input).
    #[serde(alias = "adjustment_layer")]
    AdjustmentLayer {
        start_tick: Flicks,
        end_tick: Flicks,
        #[serde(default = "default_one")]
        opacity: f32,
        #[serde(default)]
        blend_mode: BlendMode,
    },
    /// Directional, zoom, and rotational motion blur (`S_BlurMoCurves`-style) (1 input).
    #[serde(alias = "blur_mo_curves")]
    BlurMoCurves {
        #[serde(default)]
        shift_x: f32,
        #[serde(default)]
        shift_y: f32,
        #[serde(default)]
        z_dist: f32,
        #[serde(default)]
        rotate_deg: f32,
        #[serde(default = "default_one")]
        blur_strength: f32,
        #[serde(default = "default_blur_samples")]
        samples: u32,
        #[serde(default)]
        wrap_mode: u32,
    },
    /// Hierarchical 3D null transform parent node for `Transform3D` / `Null3D` rigs (0 inputs; allowed in `Scene3D`).
    #[serde(alias = "null_3d")]
    Null3D {
        #[serde(default)]
        pos: [f32; 3],
        #[serde(default)]
        rot_deg: [f32; 3],
        #[serde(default = "default_scale3")]
        scale: [f32; 3],
        #[serde(default)]
        pivot: [f32; 3],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent: Option<NodeId>,
    },
    /// Depth-map matte slicing `inputs[1]` (depth map) onto `inputs[0]` (base) (2 inputs).
    #[serde(alias = "depth_matte")]
    DepthMatte {
        #[serde(default)]
        near_depth: f32,
        #[serde(default = "default_one")]
        far_depth: f32,
        #[serde(default)]
        feather: f32,
        #[serde(default)]
        invert: bool,
    },
    /// Pyramidal GPU optical-flow motion blur (RSMB-style) (1 input).
    #[serde(rename = "optical_flow_blur", alias = "OpticalFlowBlur")]
    OpticalFlowBlur {
        #[serde(default = "default_one")]
        shutter_amount: f32,
        #[serde(default = "default_flow_samples")]
        samples: u32,
        #[serde(default = "default_flow_max_blur_px")]
        max_blur_px: f32,
        #[serde(default = "default_flow_sensitivity")]
        sensitivity: f32,
        #[serde(default = "default_flow_smoothness")]
        flow_smoothness: f32,
        #[serde(default)]
        visualize_flow: bool,
    },
    /// 2-input UV displacement map (`inputs[0]` = source, `inputs[1]` = displacement map).
    #[serde(alias = "displacement_map")]
    DisplacementMap {
        #[serde(default)]
        channel_mode: DisplacementMode,
        #[serde(default)]
        edge_mode: DisplacementWrap,
        #[serde(default = "default_disp_scale")]
        scale_x: f32,
        #[serde(default = "default_disp_scale")]
        scale_y: f32,
        #[serde(default = "default_half")]
        neutral_level: f32,
        #[serde(default)]
        angle_rad: f32,
        #[serde(default)]
        chromatic_split: f32,
        #[serde(default = "default_one")]
        opacity: f32,
    },
    /// Seek-independent temporal echo / ghost trail accumulator (1 input).
    #[serde(alias = "frame_echo")]
    FrameEcho {
        #[serde(default = "default_echo_step_ticks")]
        echo_step_ticks: i64,
        #[serde(default = "default_echo_count")]
        echo_count: u32,
        #[serde(default = "default_one")]
        starting_intensity: f32,
        #[serde(default = "default_echo_decay")]
        decay: f32,
        #[serde(default = "default_white_rgb")]
        color_decay_rgb: [f32; 3],
        #[serde(default)]
        hue_shift_deg: f32,
        #[serde(default)]
        operator: EchoOperator,
    },
    /// Keyframeable Bézier/polygon matte generator (0 inputs, feeds into `MaskMatte` or `Blend`).
    #[serde(alias = "shape_mask")]
    ShapeMask {
        #[serde(default)]
        vertices: Vec<ShapeMaskVertex>,
        #[serde(default)]
        keyframes: Vec<ShapeMaskKeyframe>,
        #[serde(default = "default_true")]
        closed: bool,
        #[serde(default)]
        feather_px: f32,
        #[serde(default)]
        expand_px: f32,
        #[serde(default)]
        invert: bool,
        #[serde(default = "default_one")]
        opacity: f32,
    },
}

fn default_disp_scale() -> f32 {
    0.05
}

fn default_echo_step_ticks() -> i64 {
    -29_400_000
}

fn default_echo_count() -> u32 {
    5
}

fn default_echo_decay() -> f32 {
    0.65
}

fn default_one() -> f32 {
    1.0
}

fn default_half() -> f32 {
    0.5
}

fn default_quarter() -> f32 {
    0.25
}

fn default_tenth() -> f32 {
    0.1
}

fn default_center() -> [f32; 2] {
    [0.5, 0.5]
}

fn default_shake_freq() -> f32 {
    12.0
}

fn default_shake_decay() -> f32 {
    4.0
}

fn default_speed_density() -> f32 {
    0.5
}

fn default_dot_scale() -> f32 {
    60.0
}

fn default_glow_radius() -> f32 {
    12.0
}

fn default_deep_glow_threshold() -> f32 {
    0.8
}

fn default_deep_glow_intensity() -> f32 {
    0.65
}

fn default_white_rgb() -> [f32; 3] {
    [1.0, 1.0, 1.0]
}

fn default_mip_levels() -> u32 {
    5
}

fn default_anime_line_strength() -> f32 {
    0.35
}

fn default_anime_sharpen_amount() -> f32 {
    0.45
}

fn default_anime_anti_ringing() -> f32 {
    0.8
}

fn default_chromatic_edge() -> f32 {
    0.004
}

fn default_grain_amount() -> f32 {
    0.04
}

fn default_halation_amount() -> f32 {
    0.15
}

fn default_vignette_amount() -> f32 {
    0.2
}

fn default_one_u32() -> u32 {
    1
}

fn default_fov_deg() -> f32 {
    50.0
}

fn default_camera_pos() -> [f32; 3] {
    [0.0, 0.0, 2.5]
}

fn default_dof_focal_dist() -> f32 {
    2.5
}

fn default_text3d_font_size() -> f32 {
    0.45
}

fn default_text3d_extrude_depth() -> f32 {
    0.12
}

fn default_text3d_bevel_size() -> f32 {
    0.01
}

fn default_text3d_color_top() -> [f32; 4] {
    [1.0, 0.95, 1.0, 1.0]
}

fn default_text3d_color_bottom() -> [f32; 4] {
    [1.0, 0.2, 0.75, 1.0]
}

fn default_text3d_side_color() -> [f32; 4] {
    [0.25, 0.05, 0.45, 1.0]
}

fn default_char_stagger_sec() -> f32 {
    0.06
}

fn default_char_bounce_amp() -> f32 {
    0.08
}

fn default_char_wave_freq() -> f32 {
    4.0
}

fn default_particle_count() -> u32 {
    120
}

fn default_particle_color_a() -> [f32; 4] {
    [1.0, 0.3, 0.85, 1.0]
}

fn default_particle_color_b() -> [f32; 4] {
    [0.2, 0.9, 1.0, 1.0]
}

fn default_particle_speed() -> f32 {
    0.8
}

fn default_particle_spread() -> f32 {
    2.2
}

fn default_particle_size() -> f32 {
    0.06
}

fn default_particle_dof_blur() -> f32 {
    0.35
}

fn default_ribbon_count() -> u32 {
    4
}

fn default_ribbon_segments() -> u32 {
    48
}

fn default_ribbon_width() -> f32 {
    0.035
}

fn default_ribbon_radius() -> f32 {
    1.1
}

fn default_ribbon_color_a() -> [f32; 4] {
    [1.0, 0.15, 0.7, 1.0]
}

fn default_ribbon_color_b() -> [f32; 4] {
    [0.1, 0.95, 1.0, 1.0]
}

fn default_true() -> bool {
    true
}

fn default_near_plane() -> f32 {
    0.05
}

fn default_far_plane() -> f32 {
    200.0
}

fn default_dof_max_radius_px() -> f32 {
    24.0
}

fn default_dof_samples() -> u32 {
    24
}

fn default_dof_highlight_boost() -> f32 {
    0.5
}

fn default_dof_blade_roundness() -> f32 {
    0.25
}

fn default_white_rgba() -> [f32; 4] {
    [1.0, 1.0, 1.0, 1.0]
}

fn default_shape_stroke() -> f32 {
    0.06
}

fn default_star_inner_ratio() -> f32 {
    0.382
}

fn default_pop_overshoot() -> f32 {
    2.15
}

fn default_echo_offset() -> [f32; 3] {
    [0.0, 0.0, -0.35]
}

fn default_echo_alpha_decay() -> f32 {
    0.55
}

fn default_echo_scale_step() -> f32 {
    0.94
}

fn default_strand_spacing() -> f32 {
    0.06
}

fn default_curve_bend() -> f32 {
    0.45
}

fn default_display_srgb() -> ColorSpace {
    ColorSpace::DisplaySrgb
}

fn default_font_weight() -> u16 {
    400
}

fn default_ocio_lut_size() -> u32 {
    33
}

fn default_hud_stroke_width() -> f32 {
    0.003
}

fn default_corner_tl() -> [f32; 2] {
    [0.0, 0.0]
}

fn default_corner_tr() -> [f32; 2] {
    [1.0, 0.0]
}

fn default_corner_br() -> [f32; 2] {
    [1.0, 1.0]
}

fn default_corner_bl() -> [f32; 2] {
    [0.0, 1.0]
}

fn default_grid_dim() -> u32 {
    4
}

fn default_wave_freq() -> [f32; 2] {
    [2.0, 2.0]
}

fn default_bevel_segments() -> u32 {
    4
}

fn default_bloom_scatter() -> f32 {
    0.65
}

fn default_grain_size_px() -> f32 {
    1.65
}

fn default_grain_chroma() -> f32 {
    0.25
}

fn default_halation_threshold() -> f32 {
    0.65
}

fn default_scrim_rect_size() -> [f32; 2] {
    [0.5, 0.2]
}

fn default_scrim_corner_radius() -> f32 {
    0.02
}

fn default_scrim_blur_radius() -> f32 {
    0.02
}

fn default_scrim_fill() -> [f32; 4] {
    [0.0, 0.0, 0.0, 0.55]
}

fn default_blur_samples() -> u32 {
    16
}

fn default_flow_samples() -> u32 {
    12
}

fn default_flow_max_blur_px() -> f32 {
    48.0
}

fn default_flow_sensitivity() -> f32 {
    0.35
}

fn default_flow_smoothness() -> f32 {
    0.25
}

fn default_scale3() -> [f32; 3] {
    [1.0, 1.0, 1.0]
}

impl NodeKind {
    /// Canonical serde `"type"` tag names for every `NodeKind` variant.
    pub const ALL_SERDE_NAMES: &'static [&'static str] = &[
        "source_video",
        "source_image",
        "solid",
        "transform2_d",
        "mask_matte",
        "blend",
        "color_basic",
        "shader_transition",
        "motion_blur_shutter",
        "vector_text",
        "vector_path_hud",
        "lut3_d",
        "motion_tile_shake",
        "anime_impact_frame",
        "radial_shockwave",
        "halftone_glow",
        "deep_glow",
        "anime_sharpen",
        "anime4k",
        "film_optics",
        "custom_wgsl",
        "transform3_d",
        "text3_d",
        "shape3_d",
        "particle_field3_d",
        "ribbon_trail3_d",
        "scene3_d",
        "corner_pin",
        "grid_warp",
        "backdrop_scrim",
        "precomp",
        "precomp_input",
        "adjustment_layer",
        "blur_mo_curves",
        "null3_d",
        "depth_matte",
        "optical_flow_blur",
        "displacement_map",
        "frame_echo",
        "shape_mask",
    ];

    /// Built-in WGSL transition shader names supported by `ShaderTransition`.
    pub const BUILTIN_TRANSITION_SHADERS: &'static [&'static str] = &[
        "whip_chromatic",
        "cross_zoom",
        "directional_warp",
        "glitch_displace",
        "dreamy_zoom",
        "swirl",
        "linear_blur",
        "burn",
        "ripple",
        "kaleidoscope",
    ];

    /// Expected number of upstream input edges for this node kind.
    ///
    /// Note: `Scene3D` (`0..=64`) and `Precomp` (`0..=16`) are variadic; callers validating
    /// input arity should use [`Self::accepts_input_count`].
    pub fn expected_input_count(&self) -> usize {
        match self {
            NodeKind::SourceVideo { .. }
            | NodeKind::SourceImage { .. }
            | NodeKind::Solid { .. }
            | NodeKind::VectorText { .. }
            | NodeKind::VectorPathHud { .. }
            | NodeKind::Precomp { .. }
            | NodeKind::PrecompInput { .. }
            | NodeKind::Text3D { .. }
            | NodeKind::Shape3D { .. }
            | NodeKind::ParticleField3D { .. }
            | NodeKind::RibbonTrail3D { .. }
            | NodeKind::Scene3D { .. }
            | NodeKind::Null3D { .. }
            | NodeKind::ShapeMask { .. } => 0,
            NodeKind::Transform2D { .. }
            | NodeKind::Transform3D { .. }
            | NodeKind::ColorBasic { .. }
            | NodeKind::MotionBlurShutter { .. }
            | NodeKind::Lut3D { .. }
            | NodeKind::MotionTileShake { .. }
            | NodeKind::AnimeImpactFrame { .. }
            | NodeKind::RadialShockwave { .. }
            | NodeKind::HalftoneGlow { .. }
            | NodeKind::DeepGlow { .. }
            | NodeKind::AnimeSharpen { .. }
            | NodeKind::Anime4K { .. }
            | NodeKind::FilmOptics { .. }
            | NodeKind::CornerPin { .. }
            | NodeKind::GridWarp { .. }
            | NodeKind::BackdropScrim { .. }
            | NodeKind::AdjustmentLayer { .. }
            | NodeKind::BlurMoCurves { .. }
            | NodeKind::OpticalFlowBlur { .. }
            | NodeKind::FrameEcho { .. } => 1,
            NodeKind::MaskMatte { .. }
            | NodeKind::DepthMatte { .. }
            | NodeKind::Blend { .. }
            | NodeKind::ShaderTransition { .. }
            | NodeKind::DisplacementMap { .. } => 2,
            NodeKind::CustomWgsl {
                expected_inputs, ..
            } => (*expected_inputs).min(2) as usize,
        }
    }

    /// Returns `true` if `count` upstream inputs is valid for this node kind.
    pub fn accepts_input_count(&self, count: usize) -> bool {
        match self {
            NodeKind::Scene3D { .. } => count <= 64,
            NodeKind::Precomp { .. } => count <= MAX_PRECOMP_INPUTS,
            _ => count == self.expected_input_count(),
        }
    }

    /// Returns `true` if this node kind is a valid direct child of `NodeKind::Scene3D`.
    pub fn is_scene3d_child(&self) -> bool {
        matches!(
            self,
            NodeKind::Text3D { .. }
                | NodeKind::Shape3D { .. }
                | NodeKind::ParticleField3D { .. }
                | NodeKind::RibbonTrail3D { .. }
                | NodeKind::Transform3D { .. }
                | NodeKind::Null3D { .. }
        )
    }

    /// Fit mode stored on a source node.
    ///
    /// Decode and the resolver must call this. Adding another fit field will diverge
    /// from project JSON. Other node kinds return `None`.
    pub fn source_fit(&self) -> Option<SourceFit> {
        match self {
            NodeKind::SourceVideo { fit, .. } | NodeKind::SourceImage { fit, .. } => Some(*fit),
            _ => None,
        }
    }
}

/// Single node inside `RenderGraphDef`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphNodeDef {
    pub id: NodeId,
    pub kind: NodeKind,
    #[serde(default)]
    pub inputs: Vec<NodeId>,
    /// `false` copies [`Self::disabled_passthrough`] instead of running the node.
    /// Omitted JSON stays enabled. There is no separate solo flag.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl GraphNodeDef {
    /// Passthrough used when this node is disabled.
    ///
    /// `None` means the node runs. Compositor bypass must call this rather than
    /// inventing a second enable flag. One-input nodes and the base of two-input
    /// nodes are [`DisabledPassthrough::Input`]`(0)`. Disabled `Scene3D` clears to
    /// transparent because its inputs are 3D scene children, not a 2D base image.
    pub fn disabled_passthrough(&self) -> Option<DisabledPassthrough> {
        if self.enabled {
            return None;
        }
        if matches!(self.kind, NodeKind::Scene3D { .. }) {
            return Some(DisabledPassthrough::Clear);
        }
        if matches!(self.kind, NodeKind::Precomp { .. }) {
            return Some(if self.inputs.is_empty() {
                DisabledPassthrough::Clear
            } else {
                DisabledPassthrough::Input(0)
            });
        }
        Some(match self.kind.expected_input_count() {
            0 => DisabledPassthrough::Clear,
            _ => DisabledPassthrough::Input(0),
        })
    }
}

/// Directed acyclic render graph describing how frames are composited.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderGraphDef {
    pub output_node: NodeId,
    pub nodes: Vec<GraphNodeDef>,
}

impl RenderGraphDef {
    /// Looks up a node by `NodeId`.
    pub fn node(&self, id: &NodeId) -> Option<&GraphNodeDef> {
        self.nodes.iter().find(|n| &n.id == id)
    }

    /// Looks up a node by `NodeId` recursively across nested `Precomp` sub-graphs.
    pub fn find_node_recursive(&self, id: &NodeId) -> Option<&GraphNodeDef> {
        if let Some(node) = self.node(id) {
            return Some(node);
        }
        for node in &self.nodes {
            if let NodeKind::Precomp { sub_graph, .. } = &node.kind
                && let Some(found) = sub_graph.find_node_recursive(id)
            {
                return Some(found);
            }
        }
        None
    }

    /// Validates node uniqueness, edge existence, arity, and acyclicity, returning a
    /// deterministic topological execution order (upstream inputs before dependents).
    pub fn validate_graph(&self) -> Result<Vec<NodeId>, SchemaError> {
        self.validate_graph_with_depth(0)
    }

    /// Validates this graph (and any nested `Precomp` sub-graphs) at `depth`.
    pub fn validate_graph_with_depth(&self, depth: usize) -> Result<Vec<NodeId>, SchemaError> {
        validate_dag_nodes(&self.nodes, Some(&self.output_node), depth, false)
    }
}

pub(crate) fn node_parent(kind: &NodeKind) -> Option<&NodeId> {
    match kind {
        NodeKind::Transform3D { parent, .. } | NodeKind::Null3D { parent, .. } => parent.as_ref(),
        _ => None,
    }
}

pub(crate) fn validate_dag_nodes(
    nodes: &[GraphNodeDef],
    output_node: Option<&NodeId>,
    depth: usize,
    allow_root_precomp_input: bool,
) -> Result<Vec<NodeId>, SchemaError> {
    let mut by_id: BTreeMap<NodeId, &GraphNodeDef> = BTreeMap::new();
    for node in nodes {
        if by_id.insert(node.id.clone(), node).is_some() {
            return Err(SchemaError::DuplicateId {
                entity: "graph node",
                id: node.id.0.clone(),
            });
        }
    }

    if let Some(output_id) = output_node
        && !by_id.contains_key(output_id)
    {
        return Err(SchemaError::MissingNode(output_id.clone()));
    }

    let mut indegree: BTreeMap<NodeId, usize> = BTreeMap::new();
    let mut dependents: BTreeMap<NodeId, Vec<NodeId>> = BTreeMap::new();

    for id in by_id.keys() {
        indegree.insert(id.clone(), 0);
        dependents.insert(id.clone(), Vec::new());
    }

    for node in nodes {
        if let NodeKind::CustomWgsl {
            wgsl_source,
            expected_inputs,
            ..
        } = &node.kind
        {
            if *expected_inputs > 2 {
                return Err(SchemaError::InvalidCustomWgsl {
                    node_id: node.id.clone(),
                    message: format!("expected_inputs ({expected_inputs}) must be <= 2"),
                });
            }
            if wgsl_source.trim().is_empty() {
                return Err(SchemaError::InvalidCustomWgsl {
                    node_id: node.id.clone(),
                    message: "wgsl_source must not be empty".into(),
                });
            }
        }

        if let NodeKind::MaskMatte {
            feather,
            opacity,
            choke_px,
            spatial_feather_px,
            ..
        } = &node.kind
            && (!feather.is_finite()
                || !opacity.is_finite()
                || !(0.0..=1.0).contains(opacity)
                || !choke_px.is_finite()
                || !spatial_feather_px.is_finite()
                || *spatial_feather_px < 0.0)
        {
            return Err(SchemaError::InvalidMaskMatte {
                node_id: node.id.clone(),
                message: "feather and choke_px must be finite; opacity must be in [0, 1]; spatial_feather_px must be finite and >= 0.0".into(),
            });
        }

        if let NodeKind::AdjustmentLayer {
            start_tick,
            end_tick,
            opacity,
            ..
        } = &node.kind
        {
            if *start_tick < Flicks::ZERO || *start_tick >= *end_tick {
                return Err(SchemaError::InvalidAdjustmentLayer {
                    node_id: node.id.clone(),
                    message: format!(
                        "requires 0 <= start_tick ({start_tick}) < end_tick ({end_tick})"
                    ),
                });
            }
            if !opacity.is_finite() || !(0.0..=1.0).contains(opacity) {
                return Err(SchemaError::InvalidAdjustmentLayer {
                    node_id: node.id.clone(),
                    message: format!("opacity ({opacity}) must be finite and in [0.0, 1.0]"),
                });
            }
        }

        if let NodeKind::BlurMoCurves {
            shift_x,
            shift_y,
            z_dist,
            rotate_deg,
            blur_strength,
            samples,
            wrap_mode,
        } = &node.kind
        {
            if !(1..=32).contains(samples) {
                return Err(SchemaError::InvalidBlurMoCurves {
                    node_id: node.id.clone(),
                    message: format!("samples ({samples}) must be in 1..=32"),
                });
            }
            if *wrap_mode > 2 {
                return Err(SchemaError::InvalidBlurMoCurves {
                    node_id: node.id.clone(),
                    message: format!("wrap_mode ({wrap_mode}) must be in 0..=2"),
                });
            }
            if !shift_x.is_finite()
                || !shift_y.is_finite()
                || !z_dist.is_finite()
                || !rotate_deg.is_finite()
                || !blur_strength.is_finite()
                || *blur_strength < 0.0
            {
                return Err(SchemaError::InvalidBlurMoCurves {
                    node_id: node.id.clone(),
                    message: "shift/z_dist/rotate_deg must be finite; blur_strength must be finite and >= 0.0".into(),
                });
            }
        }

        if let NodeKind::DepthMatte {
            near_depth,
            far_depth,
            feather,
            ..
        } = &node.kind
            && (!near_depth.is_finite()
                || !far_depth.is_finite()
                || !feather.is_finite()
                || *near_depth < 0.0
                || *far_depth < *near_depth
                || *feather < 0.0)
        {
            return Err(SchemaError::InvalidDepthMatte {
                node_id: node.id.clone(),
                message: format!(
                    "requires finite 0.0 <= near_depth ({near_depth}) <= far_depth ({far_depth}) and feather ({feather}) >= 0.0"
                ),
            });
        }

        if let NodeKind::OpticalFlowBlur {
            shutter_amount,
            samples,
            max_blur_px,
            sensitivity,
            flow_smoothness,
            ..
        } = &node.kind
        {
            if !shutter_amount.is_finite() || !(0.0..=8.0).contains(shutter_amount) {
                return Err(SchemaError::InvalidOpticalFlowBlur {
                    node_id: node.id.clone(),
                    message: format!(
                        "shutter_amount ({shutter_amount}) must be finite and in 0.0..=8.0"
                    ),
                });
            }
            if !(1..=64).contains(samples) {
                return Err(SchemaError::InvalidOpticalFlowBlur {
                    node_id: node.id.clone(),
                    message: format!("samples ({samples}) must be in 1..=64"),
                });
            }
            if !max_blur_px.is_finite() || !(0.0..=512.0).contains(max_blur_px) {
                return Err(SchemaError::InvalidOpticalFlowBlur {
                    node_id: node.id.clone(),
                    message: format!(
                        "max_blur_px ({max_blur_px}) must be finite and in 0.0..=512.0"
                    ),
                });
            }
            if !sensitivity.is_finite() || !(0.001..=2.0).contains(sensitivity) {
                return Err(SchemaError::InvalidOpticalFlowBlur {
                    node_id: node.id.clone(),
                    message: format!(
                        "sensitivity ({sensitivity}) must be finite and in 0.001..=2.0"
                    ),
                });
            }
            if !flow_smoothness.is_finite() || !(0.0..=1.0).contains(flow_smoothness) {
                return Err(SchemaError::InvalidOpticalFlowBlur {
                    node_id: node.id.clone(),
                    message: format!(
                        "flow_smoothness ({flow_smoothness}) must be finite and in 0.0..=1.0"
                    ),
                });
            }
        }

        if let NodeKind::Anime4K { strength, .. } = &node.kind
            && (!strength.is_finite() || !(0.0..=2.0).contains(strength))
        {
            return Err(SchemaError::InvalidAnime4K {
                node_id: node.id.clone(),
                message: format!("strength ({strength}) must be finite and in 0.0..=2.0"),
            });
        }

        if let NodeKind::DisplacementMap {
            scale_x,
            scale_y,
            neutral_level,
            angle_rad,
            chromatic_split,
            opacity,
            ..
        } = &node.kind
            && (!scale_x.is_finite()
                || !scale_y.is_finite()
                || !neutral_level.is_finite()
                || !angle_rad.is_finite()
                || !chromatic_split.is_finite()
                || !opacity.is_finite()
                || !(0.0..=1.0).contains(opacity)
                || !(0.0..=1.0).contains(chromatic_split))
        {
            return Err(SchemaError::InvalidDisplacementMap {
                node_id: node.id.clone(),
                message: "scale_x/scale_y/neutral_level/angle_rad must be finite; opacity and chromatic_split must be in [0.0, 1.0]".into(),
            });
        }

        if let NodeKind::FrameEcho {
            echo_count,
            starting_intensity,
            decay,
            color_decay_rgb,
            hue_shift_deg,
            ..
        } = &node.kind
        {
            if *echo_count > 24 {
                return Err(SchemaError::InvalidFrameEcho {
                    node_id: node.id.clone(),
                    message: format!("echo_count ({echo_count}) must be <= 24"),
                });
            }
            if !starting_intensity.is_finite()
                || *starting_intensity < 0.0
                || !decay.is_finite()
                || !(0.0..=1.0).contains(decay)
                || color_decay_rgb.iter().any(|c| !c.is_finite() || *c < 0.0)
                || !hue_shift_deg.is_finite()
            {
                return Err(SchemaError::InvalidFrameEcho {
                    node_id: node.id.clone(),
                    message: "starting_intensity >= 0, decay in [0, 1], color_decay_rgb >= 0, and hue_shift_deg must be finite".into(),
                });
            }
        }

        if let NodeKind::ShapeMask {
            vertices,
            keyframes,
            feather_px,
            expand_px,
            opacity,
            ..
        } = &node.kind
        {
            if !feather_px.is_finite()
                || *feather_px < 0.0
                || !expand_px.is_finite()
                || !opacity.is_finite()
                || !(0.0..=1.0).contains(opacity)
            {
                return Err(SchemaError::InvalidShapeMask {
                    node_id: node.id.clone(),
                    message: "feather_px >= 0, expand_px finite, and opacity in [0.0, 1.0] required".into(),
                });
            }
            let vertex_finite = |v: &ShapeMaskVertex| {
                v.pos[0].is_finite()
                    && v.pos[1].is_finite()
                    && v.in_tangent[0].is_finite()
                    && v.in_tangent[1].is_finite()
                    && v.out_tangent[0].is_finite()
                    && v.out_tangent[1].is_finite()
            };
            if vertices.iter().any(|v| !vertex_finite(v)) {
                return Err(SchemaError::InvalidShapeMask {
                    node_id: node.id.clone(),
                    message: "vertices must contain only finite coordinates".into(),
                });
            }
            let mut prev_tick: Option<Flicks> = None;
            for kf in keyframes {
                if let Some(pt) = prev_tick
                    && kf.tick < pt
                {
                    return Err(SchemaError::InvalidShapeMask {
                        node_id: node.id.clone(),
                        message: format!(
                            "keyframes must be ordered by non-decreasing tick ({} < {})",
                            kf.tick, pt
                        ),
                    });
                }
                prev_tick = Some(kf.tick);
                if kf.vertices.iter().any(|v| !vertex_finite(v)) {
                    return Err(SchemaError::InvalidShapeMask {
                        node_id: node.id.clone(),
                        message: "keyframe vertices must contain only finite coordinates".into(),
                    });
                }
            }
        }

        if let NodeKind::Transform3D {
            pos,
            rot_deg,
            scale,
            fov_deg,
            camera_pos,
            camera_rot_deg,
            dof_focal_dist,
            dof_blur_strength,
            pivot,
            ..
        } = &node.kind
            && (pos.iter().any(|v| !v.is_finite())
                || rot_deg.iter().any(|v| !v.is_finite())
                || scale.iter().any(|v| !v.is_finite())
                || camera_pos.iter().any(|v| !v.is_finite())
                || camera_rot_deg.iter().any(|v| !v.is_finite())
                || pivot.iter().any(|v| !v.is_finite())
                || !fov_deg.is_finite()
                || !dof_focal_dist.is_finite()
                || !dof_blur_strength.is_finite())
        {
            return Err(SchemaError::InvalidTransform3D {
                node_id: node.id.clone(),
                message: "transform3_d coordinates, angles, scale, pivot, and camera parameters must be finite".into(),
            });
        }

        if let NodeKind::Null3D {
            pos,
            rot_deg,
            scale,
            pivot,
            ..
        } = &node.kind
            && (pos.iter().any(|v| !v.is_finite())
                || rot_deg.iter().any(|v| !v.is_finite())
                || scale.iter().any(|v| !v.is_finite())
                || pivot.iter().any(|v| !v.is_finite()))
        {
            return Err(SchemaError::InvalidNull3D {
                node_id: node.id.clone(),
                message: "null3_d pos, rot_deg, scale, and pivot must be finite".into(),
            });
        }

        if let NodeKind::Text3D {
                font_path,
                line_height_em,
                tracking_em,
                bevel_segments,
                fallback_fonts,
                var_wght,
                var_wdth,
                var_slnt,
                ..
            } = &node.kind
            {
                if font_path.trim().is_empty() {
                    return Err(SchemaError::InvalidText3D {
                        node_id: node.id.clone(),
                        message: "font_path must not be empty".into(),
                    });
                }
                if !tracking_em.is_finite() {
                    return Err(SchemaError::InvalidText3D {
                        node_id: node.id.clone(),
                        message: format!("tracking_em ({tracking_em}) must be finite"),
                    });
                }
                if let Some(lh) = line_height_em
                    && (!lh.is_finite() || *lh <= 0.0)
                {
                    return Err(SchemaError::InvalidText3D {
                        node_id: node.id.clone(),
                        message: format!("line_height_em ({lh}) must be finite and > 0.0"),
                    });
                }
                if *bevel_segments == 0 || *bevel_segments > 16 {
                    return Err(SchemaError::InvalidText3D {
                        node_id: node.id.clone(),
                        message: format!("bevel_segments ({bevel_segments}) must be in 1..=16"),
                    });
                }
                for fb in fallback_fonts {
                    if fb.trim().is_empty() {
                        return Err(SchemaError::InvalidText3D {
                            node_id: node.id.clone(),
                            message: "fallback_fonts entries must not be empty".into(),
                        });
                    }
                }
                for (name, val) in [
                    ("var_wght", *var_wght),
                    ("var_wdth", *var_wdth),
                    ("var_slnt", *var_slnt),
                ] {
                    if let Some(v) = val
                        && !v.is_finite()
                    {
                        return Err(SchemaError::InvalidText3D {
                            node_id: node.id.clone(),
                            message: format!("{name} ({v}) must be finite"),
                        });
                    }
                }
            }

            if let NodeKind::Shape3D { size, .. } = &node.kind
                && (!size.is_finite() || *size <= 0.0)
            {
                return Err(SchemaError::InvalidShape3D {
                    node_id: node.id.clone(),
                    message: format!("size ({size}) must be finite and > 0.0"),
                });
            }

            if let NodeKind::VectorText {
                font_size_norm,
                outline_width_norm,
                y_offset_norm,
                font_family,
                font_path,
                font_weight,
                x_offset_norm,
                tracking_em,
                line_height_em,
                opacity,
                fallback_fonts,
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
                text_animator,
                ..
            } = &node.kind
            {
                if !font_size_norm.is_finite() || *font_size_norm < 0.0 {
                    return Err(SchemaError::InvalidVectorText {
                        node_id: node.id.clone(),
                        message: format!(
                            "font_size_norm ({font_size_norm}) must be finite and >= 0.0"
                        ),
                    });
                }
                if !outline_width_norm.is_finite() || *outline_width_norm < 0.0 {
                    return Err(SchemaError::InvalidVectorText {
                        node_id: node.id.clone(),
                        message: format!(
                            "outline_width_norm ({outline_width_norm}) must be finite and >= 0.0"
                        ),
                    });
                }
                if !y_offset_norm.is_finite() {
                    return Err(SchemaError::InvalidVectorText {
                        node_id: node.id.clone(),
                        message: format!("y_offset_norm ({y_offset_norm}) must be finite"),
                    });
                }
                if !opacity.is_finite() || *opacity < 0.0 {
                    return Err(SchemaError::InvalidVectorText {
                        node_id: node.id.clone(),
                        message: format!("opacity ({opacity}) must be finite and >= 0.0"),
                    });
                }
                if let Some(x) = x_offset_norm
                    && !x.is_finite()
                {
                    return Err(SchemaError::InvalidVectorText {
                        node_id: node.id.clone(),
                        message: format!("x_offset_norm ({x}) must be finite"),
                    });
                }
                if let Some(t) = tracking_em
                    && !t.is_finite()
                {
                    return Err(SchemaError::InvalidVectorText {
                        node_id: node.id.clone(),
                        message: format!("tracking_em ({t}) must be finite"),
                    });
                }
                if let Some(lh) = line_height_em
                    && !lh.is_finite()
                {
                    return Err(SchemaError::InvalidVectorText {
                        node_id: node.id.clone(),
                        message: format!("line_height_em ({lh}) must be finite"),
                    });
                }
                if *font_weight == 0 || *font_weight > 1000 {
                    return Err(SchemaError::InvalidVectorText {
                        node_id: node.id.clone(),
                        message: format!("font_weight ({font_weight}) must be in 1..=1000"),
                    });
                }
                if let Some(path) = font_path
                    && path.trim().is_empty()
                {
                    return Err(SchemaError::InvalidVectorText {
                        node_id: node.id.clone(),
                        message: "font_path must not be empty when provided".into(),
                    });
                }
                if let Some(family) = font_family
                    && family.trim().is_empty()
                {
                    return Err(SchemaError::InvalidVectorText {
                        node_id: node.id.clone(),
                        message: "font_family must not be empty when provided".into(),
                    });
                }
                for fb in fallback_fonts {
                    if fb.trim().is_empty() {
                        return Err(SchemaError::InvalidVectorText {
                            node_id: node.id.clone(),
                            message: "fallback_fonts entries must not be empty".into(),
                        });
                    }
                }
                for (name, val) in [
                    ("var_wght", *var_wght),
                    ("var_wdth", *var_wdth),
                    ("var_slnt", *var_slnt),
                ] {
                    if let Some(v) = val
                        && !v.is_finite()
                    {
                        return Err(SchemaError::InvalidVectorText {
                            node_id: node.id.clone(),
                            message: format!("{name} ({v}) must be finite"),
                        });
                    }
                }
                if let Some(cb) = color_bottom_rgba
                    && cb.iter().any(|v| !v.is_finite())
                {
                    return Err(SchemaError::InvalidVectorText {
                        node_id: node.id.clone(),
                        message: "color_bottom_rgba components must be finite".into(),
                    });
                }
                if !gradient_angle_deg.is_finite() {
                    return Err(SchemaError::InvalidVectorText {
                        node_id: node.id.clone(),
                        message: format!(
                            "gradient_angle_deg ({gradient_angle_deg}) must be finite"
                        ),
                    });
                }
                if !outer_outline_width_norm.is_finite() || *outer_outline_width_norm < 0.0 {
                    return Err(SchemaError::InvalidVectorText {
                        node_id: node.id.clone(),
                        message: format!(
                            "outer_outline_width_norm ({outer_outline_width_norm}) must be finite and >= 0.0"
                        ),
                    });
                }
                if !outline_gap_norm.is_finite() || *outline_gap_norm < 0.0 {
                    return Err(SchemaError::InvalidVectorText {
                        node_id: node.id.clone(),
                        message: format!(
                            "outline_gap_norm ({outline_gap_norm}) must be finite and >= 0.0"
                        ),
                    });
                }
                if shadow_offset_norm.iter().any(|v| !v.is_finite()) {
                    return Err(SchemaError::InvalidVectorText {
                        node_id: node.id.clone(),
                        message: "shadow_offset_norm components must be finite".into(),
                    });
                }
                if !shadow_blur_norm.is_finite() || *shadow_blur_norm < 0.0 {
                    return Err(SchemaError::InvalidVectorText {
                        node_id: node.id.clone(),
                        message: format!(
                            "shadow_blur_norm ({shadow_blur_norm}) must be finite and >= 0.0"
                        ),
                    });
                }
                if !glow_radius_norm.is_finite() || *glow_radius_norm < 0.0 {
                    return Err(SchemaError::InvalidVectorText {
                        node_id: node.id.clone(),
                        message: format!(
                            "glow_radius_norm ({glow_radius_norm}) must be finite and >= 0.0"
                        ),
                    });
                }
                if !hdr_intensity.is_finite() || *hdr_intensity < 0.0 {
                    return Err(SchemaError::InvalidVectorText {
                        node_id: node.id.clone(),
                        message: format!(
                            "hdr_intensity ({hdr_intensity}) must be finite and >= 0.0"
                        ),
                    });
                }
                if let Some(animator) = text_animator {
                    if font_family.is_none() && font_path.is_none() {
                        return Err(SchemaError::InvalidVectorText {
                            node_id: node.id.clone(),
                            message: "text_animator requires a TTF/OTF font_family or font_path for shaped cluster mapping".into(),
                        });
                    }
                    if animator
                        .range_end
                        .is_some_and(|end| end < animator.range_start)
                    {
                        return Err(SchemaError::InvalidVectorText {
                            node_id: node.id.clone(),
                            message: "text_animator range_end must be >= range_start".into(),
                        });
                    }
                    if animator.duration_ticks < 0 || animator.delay_ticks < 0 {
                        return Err(SchemaError::InvalidVectorText {
                            node_id: node.id.clone(),
                            message: "text_animator duration_ticks and delay_ticks must be >= 0"
                                .into(),
                        });
                    }
                    if animator.position_from_norm.iter().any(|v| !v.is_finite())
                        || !animator.scale_from.is_finite()
                        || animator.scale_from < 0.0
                        || !animator.opacity_from.is_finite()
                        || !(0.0..=1.0).contains(&animator.opacity_from)
                        || !animator.rotation_deg_from.is_finite()
                        || !animator.tracking_em_from.is_finite()
                        || !animator.ease_high.is_finite()
                        || !(-100.0..=100.0).contains(&animator.ease_high)
                        || !animator.ease_low.is_finite()
                        || !(-100.0..=100.0).contains(&animator.ease_low)
                    {
                        return Err(SchemaError::InvalidVectorText {
                            node_id: node.id.clone(),
                            message: "text_animator position, scale, opacity, rotation, tracking, and ease must be finite; scale >= 0, opacity in [0, 1], ease in [-100, 100]".into(),
                        });
                    }
                }
                if outer_outline_rgba.iter().any(|v| !v.is_finite())
                    || shadow_rgba.iter().any(|v| !v.is_finite())
                    || glow_rgba.iter().any(|v| !v.is_finite())
                {
                    return Err(SchemaError::InvalidVectorText {
                        node_id: node.id.clone(),
                        message: "outline/shadow/glow RGBA components must be finite".into(),
                    });
                }
            }

            if let NodeKind::Lut3D {
                lut_preset,
                intensity,
                lut_source,
                ..
            } = &node.kind
            {
                if !intensity.is_finite() || *intensity < 0.0 {
                    return Err(SchemaError::InvalidLut3D {
                        node_id: node.id.clone(),
                        message: format!("intensity ({intensity}) must be finite and >= 0.0"),
                    });
                }
                match lut_source {
                    None => {
                        if lut_preset.trim().is_empty() {
                            return Err(SchemaError::InvalidLut3D {
                                node_id: node.id.clone(),
                                message: "lut_preset must not be empty when lut_source is omitted"
                                    .into(),
                            });
                        }
                    }
                    Some(LutSourceDef::Preset { name }) => {
                        if name.trim().is_empty() {
                            return Err(SchemaError::InvalidLut3D {
                                node_id: node.id.clone(),
                                message: "lut_source.name must not be empty".into(),
                            });
                        }
                    }
                    Some(LutSourceDef::CubeFile {
                        path,
                        lut_size_override,
                    }) => {
                        if path.trim().is_empty() {
                            return Err(SchemaError::InvalidLut3D {
                                node_id: node.id.clone(),
                                message: "lut_source.path must not be empty".into(),
                            });
                        }
                        if let Some(sz) = lut_size_override
                            && !(2..=128).contains(sz)
                        {
                            return Err(SchemaError::InvalidLut3D {
                                node_id: node.id.clone(),
                                message: format!("lut_size_override ({sz}) must be in 2..=128"),
                            });
                        }
                    }
                    Some(LutSourceDef::Ocio {
                        config_path,
                        src_space,
                        dst_space,
                        lut_size,
                        ..
                    }) => {
                        if config_path.trim().is_empty() {
                            return Err(SchemaError::InvalidLut3D {
                                node_id: node.id.clone(),
                                message: "lut_source.config_path must not be empty".into(),
                            });
                        }
                        if src_space.trim().is_empty() || dst_space.trim().is_empty() {
                            return Err(SchemaError::InvalidLut3D {
                                node_id: node.id.clone(),
                                message: "lut_source.src_space and dst_space must not be empty"
                                    .into(),
                            });
                        }
                        if !(2..=128).contains(lut_size) {
                            return Err(SchemaError::InvalidLut3D {
                                node_id: node.id.clone(),
                                message: format!(
                                    "lut_source.lut_size ({lut_size}) must be in 2..=128"
                                ),
                            });
                        }
                    }
                    Some(LutSourceDef::BuiltinTransform { lut_size, .. }) => {
                        if !(2..=128).contains(lut_size) {
                            return Err(SchemaError::InvalidLut3D {
                                node_id: node.id.clone(),
                                message: format!(
                                    "lut_source.lut_size ({lut_size}) must be in 2..=128"
                                ),
                            });
                        }
                    }
                }
            }

            if let NodeKind::GridWarp {
                cols,
                rows,
                control_offsets,
                wave_amp,
                wave_freq,
                wave_phase,
                mix_amount,
                opacity,
                ..
            } = &node.kind
            {
                if !(2..=MAX_GRID_WARP_DIM).contains(cols)
                    || !(2..=MAX_GRID_WARP_DIM).contains(rows)
                {
                    return Err(SchemaError::InvalidGridWarp {
                        node_id: node.id.clone(),
                        message: format!(
                            "cols ({cols}) and rows ({rows}) must be in 2..={MAX_GRID_WARP_DIM}"
                        ),
                    });
                }
                if control_offsets
                    .iter()
                    .any(|[dx, dy]| !dx.is_finite() || !dy.is_finite())
                {
                    return Err(SchemaError::InvalidGridWarp {
                        node_id: node.id.clone(),
                        message: "control_offsets must contain only finite coordinates".into(),
                    });
                }
                if !wave_amp[0].is_finite()
                    || !wave_amp[1].is_finite()
                    || !wave_freq[0].is_finite()
                    || !wave_freq[1].is_finite()
                    || !wave_phase.is_finite()
                    || !mix_amount.is_finite()
                    || !opacity.is_finite()
                    || *opacity < 0.0
                {
                    return Err(SchemaError::InvalidGridWarp {
                        node_id: node.id.clone(),
                        message: "wave parameters, mix_amount, and opacity (>= 0.0) must be finite"
                            .into(),
                    });
                }
            }

            if let NodeKind::CornerPin {
                tl,
                tr,
                br,
                bl,
                opacity,
                ..
            } = &node.kind
            {
                if [tl, tr, br, bl]
                    .into_iter()
                    .any(|pt| !pt[0].is_finite() || !pt[1].is_finite())
                {
                    return Err(SchemaError::InvalidCornerPin {
                        node_id: node.id.clone(),
                        message: "corner coordinates must be finite".into(),
                    });
                }
                if !opacity.is_finite() || *opacity < 0.0 {
                    return Err(SchemaError::InvalidCornerPin {
                        node_id: node.id.clone(),
                        message: format!("opacity ({opacity}) must be finite and >= 0.0"),
                    });
                }
            }

            if let NodeKind::VectorPathHud {
                points_norm,
                bezier_segments,
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
            } = &node.kind
            {
                if points_norm
                    .iter()
                    .any(|p| !p[0].is_finite() || !p[1].is_finite())
                    || bezier_segments.iter().any(|seg| {
                        [seg.p0, seg.c1, seg.c2, seg.p1]
                            .into_iter()
                            .any(|p| !p[0].is_finite() || !p[1].is_finite())
                    })
                {
                    return Err(SchemaError::InvalidVectorPathHud {
                        node_id: node.id.clone(),
                        message: "path points and bezier segments must be finite".into(),
                    });
                }
                if !stroke_width_norm.is_finite()
                    || *stroke_width_norm < 0.0
                    || !dash_len_norm.is_finite()
                    || *dash_len_norm < 0.0
                    || !gap_len_norm.is_finite()
                    || *gap_len_norm < 0.0
                    || !dash_offset_norm.is_finite()
                    || !trim_start.is_finite()
                    || !trim_end.is_finite()
                    || !node_dot_radius_norm.is_finite()
                    || *node_dot_radius_norm < 0.0
                    || !data_value.is_finite()
                    || !opacity.is_finite()
                    || *opacity < 0.0
                {
                    return Err(SchemaError::InvalidVectorPathHud {
                        node_id: node.id.clone(),
                        message: "stroke/dash/trim/dot/opacity parameters must be finite and non-negative where applicable".into(),
                    });
                }
            }

            if let NodeKind::PrecompInput { slot } = &node.kind {
                if depth == 0 && !allow_root_precomp_input {
                    return Err(SchemaError::InvalidPrecomp {
                        node_id: node.id.clone(),
                        message: "precomp_input is forbidden in the root render graph".into(),
                    });
                }
                if allow_root_precomp_input && *slot >= MAX_PRECOMP_INPUTS {
                    return Err(SchemaError::InvalidPrecomp {
                        node_id: node.id.clone(),
                        message: format!(
                            "precomp_input slot ({slot}) must be < MAX_PRECOMP_INPUTS ({MAX_PRECOMP_INPUTS})"
                        ),
                    });
                }
            }

            if let NodeKind::Precomp {
                sub_graph,
                time_scale,
                opacity,
                ..
            } = &node.kind
            {
                let child_depth = depth + 1;
                if child_depth > MAX_PRECOMP_DEPTH {
                    return Err(SchemaError::PrecompDepthExceeded {
                        node_id: node.id.clone(),
                        depth: child_depth,
                        max_depth: MAX_PRECOMP_DEPTH,
                    });
                }
                if !time_scale.is_finite() {
                    return Err(SchemaError::InvalidPrecomp {
                        node_id: node.id.clone(),
                        message: format!("time_scale ({time_scale}) must be finite"),
                    });
                }
                if !opacity.is_finite() || *opacity < 0.0 {
                    return Err(SchemaError::InvalidPrecomp {
                        node_id: node.id.clone(),
                        message: format!("opacity ({opacity}) must be finite and >= 0.0"),
                    });
                }
                sub_graph.validate_graph_with_depth(child_depth)?;
                for sub_node in &sub_graph.nodes {
                    if let NodeKind::PrecompInput { slot } = &sub_node.kind
                        && *slot >= node.inputs.len()
                    {
                        return Err(SchemaError::InvalidPrecomp {
                            node_id: node.id.clone(),
                            message: format!(
                                "sub_graph node '{}' requests precomp_input slot {} but Precomp has {} input(s)",
                                sub_node.id,
                                slot,
                                node.inputs.len()
                            ),
                        });
                    }
                }
            }

            if !node.kind.accepts_input_count(node.inputs.len()) {
                let expected = match &node.kind {
                    NodeKind::Scene3D { .. } => 64,
                    NodeKind::Precomp { .. } => MAX_PRECOMP_INPUTS,
                    _ => node.kind.expected_input_count(),
                };
                return Err(SchemaError::InvalidNodeArity {
                    node_id: node.id.clone(),
                    expected,
                    actual: node.inputs.len(),
                });
            }

            if let NodeKind::Scene3D {
                camera,
                depth_plate,
                ..
            } = &node.kind
            {
                if let Some(dp) = depth_plate {
                    if !by_id.contains_key(&dp.node_id) {
                        return Err(SchemaError::MissingNode(dp.node_id.clone()));
                    }
                    if !dp.near_z.is_finite()
                        || !dp.far_z.is_finite()
                        || dp.near_z <= 0.0
                        || dp.far_z <= dp.near_z
                    {
                        return Err(SchemaError::InvalidScene3D {
                            node_id: node.id.clone(),
                            message: format!(
                                "depth_plate requires finite 0.0 < near_z ({}) < far_z ({})",
                                dp.near_z, dp.far_z
                            ),
                        });
                    }
                    if !dp.depth_gamma.is_finite() || !(0.05..=8.0).contains(&dp.depth_gamma) {
                        return Err(SchemaError::InvalidScene3D {
                            node_id: node.id.clone(),
                            message: format!(
                                "depth_plate.depth_gamma ({}) must be finite and in 0.05..=8.0",
                                dp.depth_gamma
                            ),
                        });
                    }
                    *indegree.get_mut(&node.id).unwrap() += 1;
                    dependents
                        .get_mut(&dp.node_id)
                        .unwrap()
                        .push(node.id.clone());
                }
                if !camera.near.is_finite() || camera.near <= 0.0 {
                    return Err(SchemaError::InvalidScene3D {
                        node_id: node.id.clone(),
                        message: format!("camera.near ({}) must be finite and > 0.0", camera.near),
                    });
                }
                if !camera.far.is_finite() || camera.far <= camera.near {
                    return Err(SchemaError::InvalidScene3D {
                        node_id: node.id.clone(),
                        message: format!(
                            "camera.far ({}) must be finite and > camera.near ({})",
                            camera.far, camera.near
                        ),
                    });
                }
                if !camera.fov_deg.is_finite() || !(5.0..=175.0).contains(&camera.fov_deg) {
                    return Err(SchemaError::InvalidScene3D {
                        node_id: node.id.clone(),
                        message: format!(
                            "camera.fov_deg ({}) must be finite and in 5.0..=175.0",
                            camera.fov_deg
                        ),
                    });
                }
                if !(8..=32).contains(&camera.dof_samples) {
                    return Err(SchemaError::InvalidScene3D {
                        node_id: node.id.clone(),
                        message: format!(
                            "camera.dof_samples ({}) must be in 8..=32",
                            camera.dof_samples
                        ),
                    });
                }
                if !camera.dof_anamorphic_ratio.is_finite() || camera.dof_anamorphic_ratio <= 0.0 {
                    return Err(SchemaError::InvalidScene3D {
                        node_id: node.id.clone(),
                        message: format!(
                            "camera.dof_anamorphic_ratio ({}) must be finite and > 0.0",
                            camera.dof_anamorphic_ratio
                        ),
                    });
                }
                for upstream in &node.inputs {
                    if let Some(child) = by_id.get(upstream)
                        && !child.kind.is_scene3d_child()
                    {
                        return Err(SchemaError::InvalidScene3D {
                            node_id: node.id.clone(),
                            message: format!(
                                "input '{upstream}' is not a valid 3D scene child (expected Text3D, Shape3D, ParticleField3D, RibbonTrail3D, Transform3D, or Null3D)"
                            ),
                        });
                    }
                }
            }

            if let Some(parent_id) = node_parent(&node.kind) {
                let Some(parent_node) = by_id.get(parent_id) else {
                    return Err(SchemaError::MissingNode(parent_id.clone()));
                };
                if !matches!(
                    parent_node.kind,
                    NodeKind::Null3D { .. } | NodeKind::Transform3D { .. }
                ) {
                    let message = format!(
                        "parent '{parent_id}' must be a Null3D or Transform3D node"
                    );
                    return Err(match node.kind {
                        NodeKind::Null3D { .. } => SchemaError::InvalidNull3D {
                            node_id: node.id.clone(),
                            message,
                        },
                        _ => SchemaError::InvalidTransform3D {
                            node_id: node.id.clone(),
                            message,
                        },
                    });
                }
                *indegree.get_mut(&node.id).unwrap() += 1;
                dependents.get_mut(parent_id).unwrap().push(node.id.clone());
            }

            for upstream in &node.inputs {
                if !by_id.contains_key(upstream) {
                    return Err(SchemaError::MissingNode(upstream.clone()));
                }
                *indegree.get_mut(&node.id).unwrap() += 1;
                dependents.get_mut(upstream).unwrap().push(node.id.clone());
            }

            if let NodeKind::Lut3D { input, .. } = &node.kind
                && !by_id.contains_key(input)
            {
                return Err(SchemaError::MissingNode(input.clone()));
            }
        }

        // Deterministic Kahn's algorithm: BTreeSet keeps zero-indegree frontier sorted by NodeId.
        let mut ready: BTreeSet<NodeId> = indegree
            .iter()
            .filter_map(|(id, &deg)| if deg == 0 { Some(id.clone()) } else { None })
            .collect();

        let mut order = Vec::with_capacity(by_id.len());
        while let Some(curr) = ready.pop_first() {
            order.push(curr.clone());
            for dep in &dependents[&curr] {
                let deg = indegree.get_mut(dep).unwrap();
                *deg -= 1;
                if *deg == 0 {
                    ready.insert(dep.clone());
                }
            }
        }

        if order.len() != by_id.len() {
            let cycle = extract_cycle_nodes(&by_id);
            return Err(SchemaError::CycleDetected { cycle });
        }

        Ok(order)
}

fn extract_cycle_nodes(by_id: &BTreeMap<NodeId, &GraphNodeDef>) -> Vec<NodeId> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Color {
        White,
        Gray,
        Black,
    }

    let mut color: BTreeMap<NodeId, Color> =
        by_id.keys().map(|id| (id.clone(), Color::White)).collect();
    let mut stack: Vec<NodeId> = Vec::new();

    fn dfs(
        curr: &NodeId,
        by_id: &BTreeMap<NodeId, &GraphNodeDef>,
        color: &mut BTreeMap<NodeId, Color>,
        stack: &mut Vec<NodeId>,
    ) -> Option<Vec<NodeId>> {
        color.insert(curr.clone(), Color::Gray);
        stack.push(curr.clone());

        if let Some(node) = by_id.get(curr) {
            let depth_plate_id = if let NodeKind::Scene3D {
                depth_plate: Some(dp),
                ..
            } = &node.kind
            {
                Some(&dp.node_id)
            } else {
                None
            };
            for upstream in node
                .inputs
                .iter()
                .chain(node_parent(&node.kind))
                .chain(depth_plate_id)
            {
                match color.get(upstream).copied().unwrap_or(Color::White) {
                    Color::Gray => {
                        let start_idx = stack.iter().position(|id| id == upstream).unwrap_or(0);
                        let mut cycle: Vec<NodeId> = stack[start_idx..].to_vec();
                        cycle.push(upstream.clone());
                        return Some(cycle);
                    }
                    Color::White => {
                        if let Some(found) = dfs(upstream, by_id, color, stack) {
                            return Some(found);
                        }
                    }
                    Color::Black => {}
                }
            }
        }

        stack.pop();
        color.insert(curr.clone(), Color::Black);
        None
    }

    // Check remaining nodes deterministically.
    let keys: VecDeque<NodeId> = by_id.keys().cloned().collect();
    for id in keys {
        if color[&id] == Color::White
            && let Some(cycle) = dfs(&id, by_id, &mut color, &mut stack)
        {
            return cycle;
        }
    }
    Vec::new()
}
