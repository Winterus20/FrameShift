//! FrameShift core orchestration, project session management, media resolution,
//! headless rendering, multi-surface inspection, and verification.

pub mod autocut;
pub mod dsl;
pub mod engine;
pub mod error;
pub mod export;
pub mod inspection;
pub mod linter;
pub mod progress;
pub mod range;
pub mod report;
pub mod resolver;
pub mod template_parametric;
pub mod tracker_bridge;
pub mod workbench;

pub use autocut::{AutocutParams, plan_autocut};
pub use dsl::{
    DslCompileOptions, DslCompileOutput, compile_motion_doc, compile_motion_str,
    parse_hex_linear_rgba,
};
pub use engine::Engine;
pub use error::CoreError;
pub use export::{
    ExportDependencyRecord, ExportJobId, ExportJobSnapshot, ExportJobState, ExportOutputReport,
    ExportOutputState, ExportProbeReport, ExportProgressSnapshot, ExportQueue, ExportTarget,
    ExportTicket,
};
pub use frameshift_app::{
    ActorKind, AppError, BackgroundJobSupervisor, ClipEditAction, JobId, JobKind, JobState,
    OptimisticLockGuard, WindowClosePolicy, WorkspaceUiController,
};
pub use frameshift_media::{
    AiDiscoveredModel, AiManifestRunConfig, AiManifestRunReport, AiManifestTask, AiModelManifest,
    AudioAnalysisReport, AudioBuffer, AudioRetimeMode, CacheTelemetry, DEFAULT_AUDIO_SAMPLE_RATE,
    EncodeBackend, HwAccelCapability, HwAccelDetector, LruFrameCache, ProxyArtifact, ProxyManager,
    ProxySpec, Rgba8Frame, discover_ai_models, load_ai_model_manifest, run_manifest_model,
    run_manifest_model_by_id, validate_manifest_and_resolve_onnx, verify_manifest_sha256,
};
pub use frameshift_render::{
    OutputAlphaMode, RenderOptions, ShutterOverride, SmartCacheConfig, SmartCacheMode,
    SmartCacheStats, TonemapMode, TrackPlanarOptions, TrackPointOptions, TrackerBindTarget,
};
pub use frameshift_schema::{
    CreativeDecisionRecord, HwAccelPolicy, KeyframeTrackDef, ParamBindingDef, PreferenceEntry,
    PreferenceScope, Project, ProjectCommand, ProjectSession, ProxyConfigDef, RenderProfileDef,
    RenderProfileKind, TrackKeyframe,
};
pub use frameshift_time::{
    ExprContext, Flicks, ImpactCurveStyle, ImpactSolution, ImpactSolveRequest, ImpactSolver,
    Rational32, SafeExpr, SpeedCurve,
};
pub use inspection::{
    EpistemicScopeReport, InspectionPackManifest, SampledFrameHash, VelocitySampleRecord,
};
pub use linter::{
    FrameDiffReport, LintFinding, LintOptions, LintReport, LintSeverity, diff_frame,
    diff_frame_files, diff_frame_with_threshold, lint_project,
};
pub use progress::{RenderProgress, RenderStage};
pub use range::RenderRange;
pub use report::{
    AudioImpactSyncReport, DeterminismCheckReport, M1VerificationReport, MotionBlurCheckReport,
    Phase5VerificationReport, SeekIndependenceCheckReport, VideoRenderSummary,
};
pub use resolver::{MediaResolver, ResolvedAssetSource, SyntheticAssetKind, slice_audio_buffer};
pub use template_parametric::{
    calculate_template_metadata, calculate_template_metadata_with_beats,
    instantiate_parametric_template, instantiate_parametric_template_with_beats,
};
pub use tracker_bridge::{
    NleExportOptions, NleImportOptions, NleMarker, NleMarkerKind, PlanarTrackProjectOptions,
    PointTrackProjectOptions, TrackerProjectReport, export_project_fcpxml_to_file,
    export_project_otio_to_file, export_project_to_fcpxml, export_project_to_otio_json,
    fcpxml_time_to_flicks, flicks_to_fcpxml_time, flicks_to_otio_rational_time,
    import_project_fcpxml_from_file, import_project_from_fcpxml, import_project_from_otio_json,
    import_project_otio_from_file, nle_markers_from_beats, otio_rational_time_to_flicks,
};
