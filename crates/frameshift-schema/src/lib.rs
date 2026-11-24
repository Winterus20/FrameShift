//! Versioned JSON project model, render DAG validation, optimistic-locking
//! `ProjectSession`, and undo/redo history for FrameShift.

pub mod error;
pub mod graph;
pub mod portable;
pub mod precomp;
pub mod project;
pub mod session;
pub mod template;
pub mod version;

pub use error::SchemaError;
pub use graph::{
    BevelProfile3D, BlendMode, Camera3DDef, Camera3DMode, ColorSpace, CubicBezierSegmentDef,
    DisabledPassthrough, ExtendedColorSpace, GraphNodeDef, HalftoneColorMode, LutSourceDef,
    MAX_GRID_WARP_DIM, MAX_PRECOMP_DEPTH, MAX_PRECOMP_INPUTS, MatteMode, NodeId, NodeKind,
    RenderGraphDef, SampleFilter, Shape3DKind, ShutterPhase, SourceFit, TextAlign2D,
    TextAnimatorDef, TextRangeShape, TextSelectorUnit, WritingMode2D,
};
pub use portable::{
    AmbiguousMediaRelink, MediaRelinkReport, PROJECT_MEDIA_MANIFEST_VERSION, ProjectMediaManifest,
    ProjectMediaManifestEntry, ProjectMediaStatus, RelinkedMediaAsset, inspect_project_media,
    missing_project_media, relink_missing_media, relink_missing_media_with_hash,
};
pub use precomp::{precompose, unprecompose};
pub use project::{
    AnimationEasing, AnimationKeyframe, AssetDef, AssetKind, AudioBus, AudioClipDef,
    AudioDuckingRuleDef, AudioFadeCurve, AudioFadeDef, ClipDef, CreativeDecisionRecord,
    EventAnchorDef, EventBoundAnimation, EventOrigin, HwAccelPolicy, MicroSilenceDef,
    ParamBindingDef, PreferenceEntry, PreferenceScope, Project, ProxyConfigDef, RenderProfileDef,
    RenderProfileKind, RenderProfilesDef, Resolution, TimelineDef, WorkingPixelFormat,
};
pub use session::{HardCut, ProjectCommand, ProjectSession};
pub use template::{
    EffectChainPreset, RedistributionStatus, TEMPLATE_PACKAGE_VERSION, TemplateFileRole,
    TemplatePackage, TemplatePackageFile, TemplatePackageManifest, TemplateReview, TemplateSlot,
    TemplateSlotKind, TemplateSlotTarget, TemplateSlotValue, validate_effect_chain_preset,
};
pub use version::{CURRENT_SCHEMA_VERSION, SchemaVersion};
