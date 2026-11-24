//! FrameShift media I/O, deterministic synthetic media generation,
//! frame pixel/color-space buffers, audio mixing/retiming, LRU frame caching,
//! scaled proxy management, clip duplicate/cut analysis, and raw-pipe FFmpeg/FFprobe bridge.

pub mod analysis;
pub mod audio;
pub mod cache;
pub mod error;
pub mod ffmpeg;
pub mod frame;
pub mod proxy;
pub mod streaming;
pub mod synthetic;

pub use analysis::{
    ANALYSIS_HEIGHT, ANALYSIS_WIDTH, ClipAnalysis, ClipAnalysisReport, DuplicateRun,
    InterpolatorBackend, analyze_frame_sequence, blend_frames, build_minterpolate_filter,
    build_rife_pair_args,
};
pub use audio::{
    AudioAnalysisReport, AudioBuffer, AudioPeakInfo, AudioRetimeMode, BiquadFilterKind,
    DEFAULT_AUDIO_SAMPLE_RATE, apply_biquad_filter, apply_micro_silence, apply_sidechain_ducking,
    apply_soft_limiter, apply_stereo_pan,
};
pub use cache::{AssetId, CacheTelemetry, FrameCacheKey, LruFrameCache};
pub use error::MediaError;
pub use ffmpeg::{
    AudioEncodingSettings, EncodeBackend, ExportEncodingProfile, ExportPostFilter, FfmpegBridge,
    FileContentIdentity, HwAccelCapability, HwAccelDetector, MediaProbeInfo, Mp4FrameEncoder,
    PixelFormat, RateControl, SourceFit, VideoCodec, VideoEncodingSettings,
    build_ffmpeg_encode_args,
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
