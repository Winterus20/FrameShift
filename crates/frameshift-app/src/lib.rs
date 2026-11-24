//! Phase 4 Desktop UI & Hybrid Human-Agent Workspace crate for FrameShift.
//!
//! Provides:
//! - [`WorkspaceUiController`]: Zero-hidden-state wrapper around `ProjectSession` (ADR-0001).
//! - [`OptimisticLockGuard`]: Concurrent human-vs-agent revision conflict protection.
//! - [`BackgroundJobSupervisor`]: Deterministic render/agent job lifecycle across window close.
//! - [`render_desktop_ui_snapshot`]: Dark-mode desktop NLE UI visual snapshot renderer.

pub mod controller;
pub mod error;
pub mod jobs;
pub mod proposal;
pub mod snapshot;
pub mod timeline;

pub use controller::{
    ActorKind, ClipEditAction, OptimisticLockGuard, UiActivityEntry, WorkspaceUiController,
};
pub use error::AppError;
pub use jobs::{
    BackgroundJob, BackgroundJobSupervisor, JobId, JobKind, JobState, WindowClosePolicy,
    WindowCloseReport,
};
pub use proposal::{
    AgentActor, AgentEditReceipt, EventPosition, MovedEvent, NamedTickRange, NewReviewNote,
    PreviewArtifactState, ProjectPreviewSnapshot, ReceiptPreview, ReviewNote, ReviewNoteFreshness,
    ReviewNoteStore, ReviewNoteView, TickRange, apply_agent_command,
};
pub use snapshot::{
    node_kind_accent, node_kind_label, render_desktop_ui_snapshot, template_slot_kind_accent,
    template_slot_kind_label,
};
pub use timeline::{
    PreviewRequestGate, PreviewTicket, SnapKind, SnapResult, SpeedGraphPoint,
    TimelineAudioClipProjection, TimelineClipProjection, TimelineEventProjection,
    TimelineProjection, snap_tick, source_output_speed_graph,
};
