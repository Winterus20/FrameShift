use frameshift_core::CoreError;
use frameshift_media::MediaError;
use frameshift_schema::SchemaError;
use frameshift_time::TimeError;
use thiserror::Error;

/// Errors returned by `frameshift-mcp` JSON-RPC 2.0 server and verification routines.
#[derive(Debug, Error)]
pub enum McpError {
    #[error("timebase error: {0}")]
    Time(#[from] TimeError),

    #[error("schema error: {0}")]
    Schema(#[from] SchemaError),

    #[error("media error: {0}")]
    Media(#[from] MediaError),

    #[error("core error: {0}")]
    Core(#[from] CoreError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("revision conflict: expected {expected}, actual {actual}")]
    RevisionConflict { expected: u64, actual: u64 },

    #[error("invalid parameters: {0}")]
    InvalidParams(String),

    #[error("verification failed: {0}")]
    VerificationFailed(String),
}
