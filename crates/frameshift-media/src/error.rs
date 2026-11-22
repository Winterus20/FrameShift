use crate::audio::AudioRetimeMode;
use frameshift_time::TimeError;
use thiserror::Error;

/// Errors returned by `frameshift-media` operations.
#[derive(Debug, Error)]
pub enum MediaError {
    #[error("timebase error: {0}")]
    Time(#[from] TimeError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid frame dimensions {width}x{height}: width and height must be non-zero")]
    InvalidDimensions { width: u32, height: u32 },

    #[error("pixel/sample buffer length mismatch: expected {expected} elements, got {actual}")]
    BufferLengthMismatch { expected: usize, actual: usize },

    #[error("PNG encoding error: {0}")]
    PngEncode(String),

    #[error("PNG decoding error: {0}")]
    PngDecode(String),

    #[error("invalid audio format: {0}")]
    InvalidAudioFormat(String),

    #[error("unsupported audio retime mode: {0:?}")]
    UnsupportedAudioRetimeMode(AudioRetimeMode),

    #[error("empty media stream: {0}")]
    EmptyMediaStream(String),

    #[error("invalid encoding settings: {0}")]
    InvalidEncodingSettings(String),

    #[error("invalid PTS index: {0}")]
    InvalidPtsIndex(String),

    #[error("ffmpeg/ffprobe binary not found or failed to spawn ({binary}): {reason}")]
    BinarySpawnFailed { binary: String, reason: String },

    #[error("ffprobe failed (exit {exit_code:?}): {stderr}")]
    FfprobeFailed {
        exit_code: Option<i32>,
        stderr: String,
    },

    #[error("ffmpeg failed (exit {exit_code:?}): {stderr}")]
    FfmpegFailed {
        exit_code: Option<i32>,
        stderr: String,
    },

    #[error("{command} failed (exit {exit_code:?}): {stderr}")]
    ProcessExit {
        command: String,
        exit_code: Option<i32>,
        stderr: String,
    },

    #[error("{message}")]
    IoError { message: String },

    #[error("failed to parse media metadata: {0}")]
    ParseError(String),
}
