//! Headless `wgpu` + WGSL unified GPU compositor and sub-frame temporal motion blur engine.

pub mod compositor;
pub mod context;
pub mod error;
pub mod lut;
pub mod ocio;
pub mod pipelines;
pub mod pool;
pub mod provider;
pub mod smart_cache;
pub mod tracker;
pub mod uniforms;
pub mod vector_text;
pub mod warp2d;

pub use compositor::{
    AudioBandEnvelope, AudioRmsEnvelope, GpuCompositor, NodeAnimator, NoopAnimator,
    ProjectAnimator, RenderOptions, RenderedFrame, ShutterOverride, compute_shutter_samples,
    effective_blur_sub_samples, evaluate_event_animation, evaluate_keyframe_track, fnv1a_64,
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
    MsaaTargetCache, PooledTexture, SourceStagingPool, TexturePool, TexturePoolStats,
    WORKING_TEXTURE_FORMAT, compute_mip_levels, create_msaa_color_texture,
    create_msaa_depth_texture,
};
pub use provider::{
    ChromaLocation, FnFrameProvider, FrameProvider, SourceFrame, SourcePixelData, YuvColorInfo,
    YuvColorMatrix, YuvColorRange, YuvTransfer,
};
pub use smart_cache::{
    DEFAULT_CPU_NODE_CACHE_BYTES, DEFAULT_DISK_NODE_CACHE_BYTES, DEFAULT_GPU_NODE_CACHE_BYTES,
    NodeContentHash, Sha256, SmartCache, SmartCacheConfig, SmartCacheMode, SmartCacheStats,
};
pub use tracker::{
    PlanarTrackResult, PlanarTrackSample, PointTrackResult, PointTrackSample, TrackPlanarOptions,
    TrackPointOptions, TrackerBindTarget, planar_track_to_keyframes, point_track_to_keyframes,
    readback_rgba16f_roi, track_planar_sequence, track_point_sequence,
};
pub use uniforms::{
    Anime4KUniform, AnimeImpactFrameUniform, AnimeSharpenUniform, BackdropScrimUniform,
    BlendUniform, BloomPassUniform, BlurMoCurvesUniform, ColorBasicUniform, ColorSpaceMode,
    CustomWgslUniform, DisplacementMapUniform, DitherMode, DofBokehUniform, DofCocUniform,
    FilmHalationUniform, FilmOpticsUniform, FrameEchoUniform, FxaaUniform, GpuMeshVertex3D,
    HalftoneGlowUniform, Lut3DUniform, MaskMatteUniform, MotionBlurPassMode, MotionBlurUniform,
    MotionTileShakeUniform, OutputAlphaMode, ParticleField3DUniform, RadialShockwaveUniform,
    RibbonTrail3DUniform, Scene3DDepthSeedUniform, ShapeMaskFeatherUniform, Text2DFxUniform,
    Text2DJfaUniform, Text3DUniform, TonemapMode, Transform3DUniform, TransformUniform,
    WhipTransitionUniform, YuvToLinearUniform,
};
pub use vector_text::rasterize_vector_text;
pub use warp2d::{CornerPinUniform, GridWarpUniform, compute_corner_pin_inv_homography};

/// Returns the crate name marker.
pub fn crate_name() -> &'static str {
    "frameshift-render"
}
