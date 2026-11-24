//! FrameShift media I/O, deterministic synthetic media generation,
//! frame pixel/color-space buffers, audio mixing/retiming, LRU frame caching,
//! scaled proxy management, clip duplicate/cut analysis, and raw-pipe FFmpeg/FFprobe bridge.

pub mod ai_audio;
pub mod ai_inference;
pub mod ai_manifest;
pub mod analysis;
pub mod audio;
pub mod blackdetect;
pub mod cache;
pub mod error;
pub mod ffmpeg;
pub mod frame;
pub mod interpolator;
pub mod proxy;
pub mod spectral;
pub mod streaming;
pub mod synthetic;
pub mod upscaler;
pub mod waveform;

pub use ai_audio::{
    AiAudioCapabilities, AiAudioCapabilitiesReport, JapaneseTokenizer, LyricAlignBackend,
    LyricAlignRequest, LyricAlignResult, LyricEventAnchorTemplate, LyricNodeStyle,
    LyricParamBindingTemplate, LyricSegmentTicks, LyricTimelineNodesTemplate,
    LyricTimelineTemplateConfig, SeparatedStemFile, StemBackend, StemSeparationRequest,
    StemSeparationResult, WordTimingTicks, align_audio_lyrics, build_lyric_timeline_nodes,
    detect_ai_audio_capabilities, detect_ai_audio_capabilities_with, is_isolated_audio_venv,
    parse_lyric_alignment_json, parse_stem_separation_json, resolve_ai_audio_sidecar_script,
    resolve_audio_model_cache_dir, resolve_audio_python, seconds_to_flicks, separate_audio_stems,
};
pub use ai_inference::{
    AiCapabilitiesReport, AiInferenceConfig, AiInferenceReport, AiTaskKind, DepthNormMode,
    DepthOutputFormat, detect_ai_capabilities, resolve_ai_sidecar_script,
    run_ai_depth_f32_inference, run_ai_depth_png16_sequence, run_ai_frame_inference,
    run_ai_video_inference, synthetic_ai_test_frames,
};
pub use ai_manifest::{
    AI_MODEL_MANIFEST_FILENAME, AI_MODEL_MANIFEST_SCHEMA_V1, AiAuxInputSpec, AiDiscoveredModel,
    AiInputColorSpace, AiLicenseInfo, AiManifestRunConfig, AiManifestRunReport, AiManifestTask,
    AiModelLicense, AiModelManifest, AiOutputActivation, AiResizeMode, AiResizePolicy,
    AiTensorDtype, AiTensorLayout, AiTilingConfig, Sha256VerificationPolicy, discover_ai_models,
    load_ai_model_manifest, run_manifest_model, run_manifest_model_by_id,
    validate_manifest_and_resolve_onnx, verify_manifest_sha256,
};
pub use analysis::{
    ANALYSIS_HEIGHT, ANALYSIS_WIDTH, ClipAnalysis, ClipAnalysisReport, DuplicateRun,
    InterpolatorBackend, analyze_frame_sequence, blend_frames, build_minterpolate_filter,
    build_rife_pair_args,
};
pub use blackdetect::{
    BlackDetectOptions, BlackInterval, detect_black_intervals, detect_black_intervals_with_options,
    parse_blackdetect_stderr,
};
pub use interpolator::{
    InterpolateConfig, InterpolationReport, interpolate_frame_sequence, interpolate_video_file,
    resolve_interpolator_backend, write_synthetic_motion_mp4,
};
pub use audio::{
    AudioAnalysisReport, AudioBuffer, AudioPeakInfo, AudioRetimeMode, BiquadFilterKind,
    DEFAULT_AUDIO_SAMPLE_RATE, apply_biquad_filter, apply_micro_silence, apply_sidechain_ducking,
    apply_soft_limiter, apply_stereo_pan,
};
pub use cache::{AssetId, CacheTelemetry, CachedVideoFrame, DEFAULT_CACHE_MAX_BYTES, FrameCacheKey, LruFrameCache};
pub use error::MediaError;
pub use ffmpeg::{
    AudioEncodingSettings, EncodeBackend, ExportEncodingProfile, ExportPostFilter, FfmpegBridge,
    FileContentIdentity, HwAccelCapability, HwAccelDetector, MediaProbeInfo, Mp4FrameEncoder,
    PixelFormat, RateControl, SourceFit, VideoCodec, VideoEncodingSettings,
    build_ffmpeg_encode_args, compute_contain_layout,
};
pub use frame::{
    AlphaMode, ChromaLocation, DecodedVideoFrame, Nv12Frame, Rgba8Frame, Rgba16fFrame,
    YuvColorInfo, YuvColorMatrix, YuvColorRange, YuvTransfer, fit_rgba8_frame, linear_to_srgb,
    srgb_to_linear,
};
pub use frameshift_time::{Flicks, FrameMatch, PtsIndex, Rational32};
pub use proxy::{ProxyArtifact, ProxyManager, ProxySpec, downscale_frame_nearest_or_box};
pub use spectral::{
    AudioBandEnvelope, BandOnsetSummary, MultibandBeatReport, MultibandFluxConfig, Radix2Fft,
};
pub use streaming::{
    DEFAULT_MAX_OPEN_STREAMS, DEFAULT_MAX_SEQUENTIAL_SKIP, DecoderPool, DecoderPoolConfig,
    DecoderPoolTelemetry, StreamKey, StreamingPixelFormat, StreamingVideoDecoder,
};
pub use synthetic::{SyntheticMediaGenerator, decode_tick_barcode, embed_tick_barcode};
pub use upscaler::{
    RealEsrganCapabilities, RealEsrganModel, UpscaleBackend, UpscaleConfig, UpscaleReport,
    build_realesrgan_batch_args, detect_realesrgan_capabilities,
    detect_realesrgan_capabilities_with_config, resolve_upscale_backend, upscale_frame_sequence,
    upscale_video_file,
};
pub use waveform::{
    DEFAULT_WAVEFORM_BASE_BUCKET_FRAMES, WaveformAmplitudeScale, WaveformChannelLayout,
    WaveformColumn, WaveformMarker, WaveformMarkerKind, WaveformPyramid, WaveformRenderOptions,
    WaveformRenderer, WaveformStyle, render_waveform_png_bytes, render_waveform_rgba,
};
