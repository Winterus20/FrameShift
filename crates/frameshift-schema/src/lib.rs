//! Versioned JSON project model, render DAG validation, optimistic-locking
//! `ProjectSession`, and undo/redo history for FrameShift.

pub mod error;
pub mod graph;
pub mod project;
pub mod session;
pub mod version;

pub use error::SchemaError;
pub use graph::{
    BlendMode, Camera3DDef, Camera3DMode, ColorSpace, CubicBezierSegmentDef, DisabledPassthrough,
    ExtendedColorSpace, GraphNodeDef, HalftoneColorMode, LutSourceDef, MAX_GRID_WARP_DIM,
    MAX_PRECOMP_DEPTH, MAX_PRECOMP_INPUTS, MatteMode, NodeId, NodeKind, RenderGraphDef,
    SampleFilter, Shape3DKind, ShutterPhase, SourceFit, TextAlign2D, WritingMode2D,
    BevelProfile3D,
};
pub use project::{
    AnimationEasing, AnimationKeyframe, AssetDef, AssetKind, AudioClipDef, AudioFadeCurve,
    AudioFadeDef, ClipDef, CreativeDecisionRecord, EventAnchorDef, EventBoundAnimation,
    EventOrigin, HwAccelPolicy, ParamBindingDef, PreferenceEntry, PreferenceScope, Project,
    ProxyConfigDef, RenderProfileDef, RenderProfileKind, RenderProfilesDef, Resolution,
    TimelineDef, WorkingPixelFormat,
};
pub use session::{HardCut, ProjectCommand, ProjectSession};
pub use version::{CURRENT_SCHEMA_VERSION, SchemaVersion};
