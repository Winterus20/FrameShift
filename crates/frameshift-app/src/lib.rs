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
pub mod snapshot;

pub use controller::{
    ActorKind, ClipEditAction, OptimisticLockGuard, UiActivityEntry, WorkspaceUiController,
};
pub use error::AppError;
pub use jobs::{
    BackgroundJob, BackgroundJobSupervisor, JobId, JobKind, JobState, WindowClosePolicy,
    WindowCloseReport,
};
pub use snapshot::render_desktop_ui_snapshot;
