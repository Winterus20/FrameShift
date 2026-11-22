//! FrameShift media I/O, deterministic synthetic media generation,
//! frame pixel/color-space buffers, audio mixing/retiming, LRU frame caching,
//! scaled proxy management, and raw-pipe FFmpeg/FFprobe bridge.

pub mod audio;
pub mod cache;
pub mod error;
pub mod ffmpeg;
pub mod frame;
pub mod proxy;
pub mod streaming;
pub mod synthetic;

pub use audio::{
    AudioAnalysisReport, AudioBuffer, AudioPeakInfo, AudioRetimeMode, DEFAULT_AUDIO_SAMPLE_RATE,
};
pub use cache::{AssetId, CacheTelemetry, FrameCacheKey, LruFrameCache};
pub use error::MediaError;
pub use ffmpeg::{
    EncodeBackend, FfmpegBridge, HwAccelCapability, HwAccelDetector, MediaProbeInfo,
    Mp4FrameEncoder, SourceFit,
};
pub use frame::{
    AlphaMode, Rgba8Frame, Rgba16fFrame, fit_rgba8_frame, linear_to_srgb, srgb_to_linear,
};
pub use frameshift_time::{Flicks, FrameMatch, PtsIndex, Rational32};
pub use proxy::{ProxyArtifact, ProxyManager, ProxySpec, downscale_frame_nearest_or_box};
pub use streaming::{
    DEFAULT_MAX_OPEN_STREAMS, DEFAULT_MAX_SEQUENTIAL_SKIP, DecoderPool, DecoderPoolConfig,
    DecoderPoolTelemetry, StreamKey, StreamingVideoDecoder,
};
pub use synthetic::{SyntheticMediaGenerator, decode_tick_barcode, embed_tick_barcode};
