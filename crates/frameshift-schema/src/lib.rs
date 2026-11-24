//! Versioned JSON project model, render DAG validation, optimistic-locking
//! `ProjectSession`, and undo/redo history for FrameShift.

pub mod dsl;
pub mod error;
pub mod graph;
pub mod nle_bridge;
pub mod portable;
pub mod precomp;
pub mod project;
pub mod session;
pub mod template;
pub mod template_parametric;
pub mod version;

pub use dsl::{
    MotionDirective, MotionDoc, MotionLutSpec, MotionMeta, MotionRangeExpr, MotionTimeExpr,
    parse_motion_doc,
};
pub use error::SchemaError;
pub use graph::{
    Anime4KPreset, BevelProfile3D, BlendMode, Camera3DDef, Camera3DMode, ColorSpace,
    CubicBezierSegmentDef, DisabledPassthrough, DisplacementChannelMode, DisplacementEdgeMode,
    DisplacementMode, DisplacementWrap, EchoOperator, ExtendedColorSpace, GraphNodeDef,
    HalftoneColorMode, LutSourceDef, MAX_GRID_WARP_DIM, MAX_PRECOMP_DEPTH, MAX_PRECOMP_INPUTS,
    MatteMode, NodeId, NodeKind, RenderGraphDef, SampleFilter, SceneDepthPlateDef, Shape3DKind,
    ShapeMaskKeyframe, ShapeMaskVertex, ShutterPhase, SourceFit, TextAlign2D, TextAnimatorDef,
    TextRangeShape, TextSelectorUnit, WritingMode2D,
};
pub use nle_bridge::{
    NleExportOptions, NleImportOptions, NleMarker, NleMarkerKind, OtioRationalTime, OtioTimeRange,
    export_project_to_fcpxml, export_project_to_otio_json, fcpxml_time_to_flicks,
    flicks_to_fcpxml_time, flicks_to_otio_rational_time, import_project_from_fcpxml,
    import_project_from_otio_json, otio_rational_time_to_flicks,
};
pub use portable::{
    AmbiguousMediaRelink, MediaRelinkReport, PROJECT_MEDIA_MANIFEST_VERSION, ProjectMediaManifest,
    ProjectMediaManifestEntry, ProjectMediaStatus, RelinkedMediaAsset, inspect_project_media,
    missing_project_media, relink_missing_media, relink_missing_media_with_hash,
};
pub use precomp::{precompose, unprecompose};
pub use project::{
    AnimationEasing, AnimationKeyframe, AssetDef, AssetKind, AudioBus, AudioClipDef,
    AudioDuckingRuleDef, AudioFadeCurve, AudioFadeDef, ClipDef, ClipTimingEdge,
    CreativeDecisionRecord, EventAnchorDef, EventBoundAnimation, EventOrigin,
    EventTimingConstraint, HwAccelPolicy, KeyframeTrackDef, MicroSilenceDef, ParamBindingDef,
    PreferenceEntry, PreferenceScope, Project, ProxyConfigDef, RenderProfileDef,
    RenderProfileKind, RenderProfilesDef, Resolution, TimelineDef, TrackKeyframe,
    WorkingPixelFormat, bindable_param_names, resolve_event_constraints,
    validate_event_constraints,
};
pub use session::{HardCut, ProjectCommand, ProjectSession};
pub use template::{
    EffectChainPreset, RedistributionStatus, TEMPLATE_PACKAGE_VERSION, TemplateFileRole,
    TemplatePackage, TemplatePackageFile, TemplatePackageManifest, TemplateReview, TemplateSlot,
    TemplateSlotKind, TemplateSlotTarget, TemplateSlotValue, validate_effect_chain_preset,
};
pub use template_parametric::{
    BeatSelectionSource, CalculatedTemplateMetadata, DurationPolicy, MetadataRulesDef,
    MotionBlueprintDef, PARAMETRIC_TEMPLATE_SPEC_VERSION, ParametricTemplateSpec,
    TemplateClipAssignment, TemplateDropEvent, TemplateEffectWindow, TemplateMetadata,
    TemplatePropDef, TemplatePropKind,
};
pub use version::{CURRENT_SCHEMA_VERSION, SchemaVersion};
