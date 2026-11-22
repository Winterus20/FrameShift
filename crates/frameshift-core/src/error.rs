use frameshift_app::AppError;
use frameshift_media::MediaError;
use frameshift_render::RenderError;
use frameshift_schema::SchemaError;
use frameshift_time::{ConstraintViolation, TimeError};
use frameshift_vector::VectorError;
use thiserror::Error;

/// Errors returned by `frameshift-core` orchestration, media resolution, rendering, and verification.
#[derive(Debug, Error)]
pub enum CoreError {
    #[error("timebase error: {0}")]
    Time(#[from] TimeError),

    #[error("schema error: {0}")]
    Schema(#[from] SchemaError),

    #[error("media error: {0}")]
    Media(#[from] MediaError),

    #[error("render error: {0}")]
    Render(#[from] RenderError),

    #[error("vector error: {0}")]
    Vector(#[from] VectorError),

    #[error("app controller error: {0}")]
    App(#[from] AppError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("impact constraint violation: {0}")]
    ImpactConstraint(#[from] ConstraintViolation),

    #[error("missing media asset '{asset_id}' at path '{path}'")]
    MissingAssetFile { asset_id: String, path: String },

    #[error("audio-impact sync verification failed: {0}")]
    AudioSyncFailure(String),

    #[error("verification failed: {0}")]
    VerificationFailed(String),

    #[error("inspection target not found: {0}")]
    InspectionTargetNotFound(String),

    #[error("invalid inspection range: {0}")]
    InvalidInspectionRange(String),

    #[error("invalid crop region: {0}")]
    InvalidCropRegion(String),

    #[error("invalid argument: {0}")]
    InvalidArgument(String),
}
