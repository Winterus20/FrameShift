use frameshift_media::MediaError;
use thiserror::Error;

/// Errors returned by `frameshift-vector` rasterization and canvas operations.
#[derive(Debug, Error)]
pub enum VectorError {
    #[error("invalid canvas dimensions {width}x{height}: width and height must be non-zero")]
    InvalidDimensions { width: u32, height: u32 },

    #[error("invalid vector parameter: {0}")]
    InvalidParameter(String),

    #[error("polygon must contain at least 3 finite vertices (got {0})")]
    EmptyPolygon(usize),

    #[error("media buffer error: {0}")]
    Media(#[from] MediaError),

    #[error("font file not found: {0}")]
    FontNotFound(String),

    #[error("invalid font data: {0}")]
    InvalidFont(String),
}
