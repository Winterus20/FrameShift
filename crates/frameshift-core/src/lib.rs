//! FrameShift core orchestration, project session management, media resolution,
//! headless rendering, multi-surface inspection, and verification.

pub mod autocut;
pub mod engine;
pub mod error;
pub mod export;
pub mod inspection;
pub mod progress;
pub mod range;
pub mod report;
pub mod resolver;
pub mod workbench;

pub use autocut::{AutocutParams, plan_autocut};
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
    AudioAnalysisReport, AudioBuffer, AudioRetimeMode, CacheTelemetry, DEFAULT_AUDIO_SAMPLE_RATE,
    EncodeBackend, HwAccelCapability, HwAccelDetector, LruFrameCache, ProxyArtifact, ProxyManager,
    ProxySpec, Rgba8Frame,
};
pub use frameshift_render::{OutputAlphaMode, RenderOptions, ShutterOverride, TonemapMode};
pub use frameshift_schema::{
    CreativeDecisionRecord, HwAccelPolicy, ParamBindingDef, PreferenceEntry, PreferenceScope,
    Project, ProjectCommand, ProjectSession, ProxyConfigDef, RenderProfileDef, RenderProfileKind,
};
pub use frameshift_time::{
    ExprContext, Flicks, ImpactCurveStyle, ImpactSolution, ImpactSolveRequest, ImpactSolver,
    Rational32, SafeExpr, SpeedCurve,
};
pub use inspection::{
    EpistemicScopeReport, InspectionPackManifest, SampledFrameHash, VelocitySampleRecord,
};
pub use progress::{RenderProgress, RenderStage};
pub use range::RenderRange;
pub use report::{
    AudioImpactSyncReport, DeterminismCheckReport, M1VerificationReport, MotionBlurCheckReport,
    Phase5VerificationReport, SeekIndependenceCheckReport, VideoRenderSummary,
};
pub use resolver::{MediaResolver, ResolvedAssetSource, SyntheticAssetKind, slice_audio_buffer};
