use crate::jobs::JobId;
use frameshift_media::MediaError;
use frameshift_schema::SchemaError;
use frameshift_time::TimeError;
use thiserror::Error;

/// Errors produced by `frameshift-app` workspace controller, concurrency guards,
/// background job supervision, and UI snapshot rendering.
#[derive(Debug, Error)]
pub enum AppError {
    /// Concurrent agent edit attempted against a stale project revision.
    #[error("concurrency conflict: expected revision {expected}, actual revision {actual}")]
    ConcurrencyConflict { expected: u64, actual: u64 },

    /// Schema validation or session mutation error.
    #[error("schema error: {0}")]
    Schema(#[from] SchemaError),

    /// Media/frame buffer error.
    #[error("media error: {0}")]
    Media(#[from] MediaError),

    /// Timebase or speed-curve error.
    #[error("timebase error: {0}")]
    Time(#[from] TimeError),

    /// Invalid UI controller operation (e.g., out-of-bounds split or unknown selection).
    #[error("invalid workspace operation: {0}")]
    InvalidOperation(String),

    /// Referenced background job ID does not exist.
    #[error("background job {0:?} not found")]
    JobNotFound(JobId),

    /// Lifecycle transition is invalid for the current job state.
    #[error("invalid state transition for job {job_id:?}: {message}")]
    InvalidJobState { job_id: JobId, message: String },
}
