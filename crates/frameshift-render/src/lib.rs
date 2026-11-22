//! Headless `wgpu` + WGSL unified GPU compositor and sub-frame temporal motion blur engine.

pub mod compositor;
pub mod context;
pub mod error;
pub mod lut;
pub mod ocio;
pub mod pipelines;
pub mod pool;
pub mod provider;
pub mod uniforms;
pub mod vector_text;
pub mod warp2d;

pub use compositor::{
    AudioRmsEnvelope, GpuCompositor, NodeAnimator, NoopAnimator, ProjectAnimator, RenderOptions,
    RenderedFrame, ShutterOverride, compute_shutter_samples, effective_blur_sub_samples,
    evaluate_event_animation, fnv1a_64,
};
pub use context::GpuContext;
pub use error::RenderError;
pub use frameshift_schema::{NodeId, RenderGraphDef};
pub use lut::{
    ColorTransform, CubeLut1D, CubeLut3D, compose_color_space_bridge, resolve_lut_source,
};
pub use ocio::{
    CompiledOcioChain, NegativeStyle, OcioBakeOptions, OcioColorPipeline, OcioColorSpaceDef,
    OcioConfig, OcioInterpolation, OcioSpace, OcioTransform, eval_builtin_transform_f64,
    from_linear_rec709_f64, invert_transform, to_linear_rec709_f64,
};
pub use pipelines::{ShaderPipelines, ShaderProgram};
pub use pool::{
    MsaaTargetCache, PooledTexture, TexturePool, TexturePoolStats, WORKING_TEXTURE_FORMAT,
    compute_mip_levels, create_msaa_color_texture, create_msaa_depth_texture,
};
pub use provider::{FnFrameProvider, FrameProvider, SourceFrame, SourcePixelData};
pub use uniforms::{
    AnimeImpactFrameUniform, AnimeSharpenUniform, BackdropScrimUniform, BlendUniform,
    BloomPassUniform,
    ColorBasicUniform, ColorSpaceMode, CustomWgslUniform, DitherMode, DofBokehUniform,
    DofCocUniform, FilmHalationUniform, FilmOpticsUniform, FxaaUniform, GpuMeshVertex3D,
    HalftoneGlowUniform, Lut3DUniform, MaskMatteUniform, MotionBlurPassMode, MotionBlurUniform,
    MotionTileShakeUniform, OutputAlphaMode, ParticleField3DUniform, RadialShockwaveUniform,
    RibbonTrail3DUniform, Text2DFxUniform, Text2DJfaUniform, Text3DUniform, TonemapMode,
    Transform3DUniform, TransformUniform, WhipTransitionUniform,
};
pub use vector_text::rasterize_vector_text;
pub use warp2d::{CornerPinUniform, GridWarpUniform, compute_corner_pin_inv_homography};

/// Returns the crate name marker.
pub fn crate_name() -> &'static str {
    "frameshift-render"
}
