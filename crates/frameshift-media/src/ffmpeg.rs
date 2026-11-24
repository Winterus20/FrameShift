use crate::audio::AudioBuffer;
use crate::error::MediaError;
use crate::frame::Sha256;
use crate::frame::{AlphaMode, DecodedVideoFrame, Nv12Frame, Rgba8Frame, YuvColorInfo, fit_rgba8_frame, validated_nv12_plane_lens};
use frameshift_time::{FLICKS_PER_SECOND, Flicks, PtsIndex, Rational32};
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::JoinHandle;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

const X26X_PRESETS: &[&str] = &[
    "ultrafast",
    "superfast",
    "veryfast",
    "faster",
    "fast",
    "medium",
    "slow",
    "slower",
    "veryslow",
];

fn default_true() -> bool {
    true
}

/// Content identity for a file, computed from its exact bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileContentIdentity {
    pub byte_len: u64,
    /// Lowercase SHA-256 digest.
    pub sha256: String,
}

/// Target video codec for FFmpeg export encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum VideoCodec {
    #[default]
    H264,
    Hevc,
    ProRes422Hq,
    ProRes4444,
}

impl VideoCodec {
    /// Returns the FFmpeg `-c:v` encoder identifier.
    pub fn ffmpeg_encoder(self) -> &'static str {
        match self {
            Self::H264 => "libx264",
            Self::Hevc => "libx265",
            Self::ProRes422Hq | Self::ProRes4444 => "prores_ks",
        }
    }

    /// Returns `true` for Apple ProRes mastering codecs targeting `.mov` containers.
    pub fn is_prores(self) -> bool {
        matches!(self, Self::ProRes422Hq | Self::ProRes4444)
    }

    /// Preferred file extension (`"mp4"` or `"mov"`).
    pub fn default_extension(self) -> &'static str {
        if self.is_prores() { "mov" } else { "mp4" }
    }
}

/// Output pixel format for encoded video streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum PixelFormat {
    #[default]
    Yuv420p,
    Yuv420p10le,
    Yuva444p10le,
}

impl PixelFormat {
    /// Returns the FFmpeg `-pix_fmt` name.
    pub fn ffmpeg_pix_fmt(self) -> &'static str {
        match self {
            Self::Yuv420p => "yuv420p",
            Self::Yuv420p10le => "yuv420p10le",
            Self::Yuva444p10le => "yuva444p10le",
        }
    }
}

/// Rate-control policy for H.264 / HEVC encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RateControl {
    Crf(u8),
    Vbr {
        target_kbps: u32,
        max_kbps: u32,
        bufsize_kbps: u32,
    },
}

impl Default for RateControl {
    fn default() -> Self {
        Self::Crf(16)
    }
}

/// Audio stream encoding parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AudioEncodingSettings {
    pub bitrate_kbps: u32,
    pub sample_rate_hz: u32,
}

impl Default for AudioEncodingSettings {
    fn default() -> Self {
        Self {
            bitrate_kbps: 192,
            sample_rate_hz: 48_000,
        }
    }
}

/// Optional FFmpeg post-encode `-vf` filter settings (debanding + subtle sharpening).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct ExportPostFilter {
    pub deband: bool,
    pub unsharp_amount: f32,
}

impl ExportPostFilter {
    /// Returns the combined `-vf` filtergraph string, if any post-filter is active.
    pub fn to_ffmpeg_vf(self) -> Option<String> {
        let mut parts = Vec::new();
        if self.deband {
            parts.push("deband=range=16:thr=0.02".to_owned());
        }
        if self.unsharp_amount.is_finite() && self.unsharp_amount > 0.0 {
            parts.push(format!("unsharp=3:3:{}", self.unsharp_amount));
        }
        if parts.is_empty() {
            None
        } else {
            Some(parts.join(","))
        }
    }
}

/// Full export encoding profile supporting H.264, 10-bit HEVC, ProRes mastering,
/// social platform presets, Rec.709 color tagging, faststart, and optional post-filtering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportEncodingProfile {
    pub codec: VideoCodec,
    pub pixel_format: PixelFormat,
    pub rate_control: RateControl,
    /// FFmpeg encoder preset for `libx264` / `libx265`.
    pub preset: String,
    #[serde(default)]
    pub audio: AudioEncodingSettings,
    #[serde(default)]
    pub post_filter: ExportPostFilter,
    #[serde(default = "default_true")]
    pub bt709_tags: bool,
    #[serde(default = "default_true")]
    pub faststart: bool,
}

impl Default for ExportEncodingProfile {
    fn default() -> Self {
        Self {
            codec: VideoCodec::H264,
            pixel_format: PixelFormat::Yuv420p,
            rate_control: RateControl::Crf(16),
            preset: "veryfast".into(),
            audio: AudioEncodingSettings::default(),
            post_filter: ExportPostFilter::default(),
            bt709_tags: true,
            faststart: true,
        }
    }
}

impl ExportEncodingProfile {
    /// TikTok 1080p60 social delivery profile (H.264 High 8-bit VBR + deband/sharpen + faststart).
    pub fn tiktok_1080p60() -> Self {
        Self {
            codec: VideoCodec::H264,
            pixel_format: PixelFormat::Yuv420p,
            rate_control: RateControl::Vbr {
                target_kbps: 18_000,
                max_kbps: 25_000,
                bufsize_kbps: 36_000,
            },
            preset: "slow".into(),
            audio: AudioEncodingSettings {
                bitrate_kbps: 256,
                sample_rate_hz: 48_000,
            },
            post_filter: ExportPostFilter {
                deband: true,
                unsharp_amount: 0.25,
            },
            bt709_tags: true,
            faststart: true,
        }
    }

    /// Instagram Reels 1080p30 delivery profile (H.264 High 8-bit VBR + deband/sharpen + faststart).
    pub fn reels_1080p30() -> Self {
        Self {
            codec: VideoCodec::H264,
            pixel_format: PixelFormat::Yuv420p,
            rate_control: RateControl::Vbr {
                target_kbps: 12_000,
                max_kbps: 18_000,
                bufsize_kbps: 24_000,
            },
            preset: "slow".into(),
            audio: AudioEncodingSettings {
                bitrate_kbps: 192,
                sample_rate_hz: 48_000,
            },
            post_filter: ExportPostFilter {
                deband: true,
                unsharp_amount: 0.2,
            },
            bt709_tags: true,
            faststart: true,
        }
    }

    /// YouTube 1080p60 high-bitrate H.264 High profile (320 kbps AAC, faststart).
    pub fn youtube_1080p60() -> Self {
        Self {
            codec: VideoCodec::H264,
            pixel_format: PixelFormat::Yuv420p,
            rate_control: RateControl::Vbr {
                target_kbps: 24_000,
                max_kbps: 35_000,
                bufsize_kbps: 48_000,
            },
            preset: "slow".into(),
            audio: AudioEncodingSettings {
                bitrate_kbps: 320,
                sample_rate_hz: 48_000,
            },
            post_filter: ExportPostFilter::default(),
            bt709_tags: true,
            faststart: true,
        }
    }

    /// 10-bit HEVC anime archival/AMV master profile (`yuv420p10le`, CRF 14, deband).
    pub fn anime_10bit_hevc() -> Self {
        Self {
            codec: VideoCodec::Hevc,
            pixel_format: PixelFormat::Yuv420p10le,
            rate_control: RateControl::Crf(14),
            preset: "slow".into(),
            audio: AudioEncodingSettings {
                bitrate_kbps: 320,
                sample_rate_hz: 48_000,
            },
            post_filter: ExportPostFilter {
                deband: true,
                unsharp_amount: 0.0,
            },
            bt709_tags: true,
            faststart: true,
        }
    }

    /// Apple ProRes 422 HQ mastering profile (`.mov` with `pcm_s16le` audio, emitted as `yuv422p10le`).
    pub fn prores_422hq() -> Self {
        Self {
            codec: VideoCodec::ProRes422Hq,
            pixel_format: PixelFormat::Yuv420p10le,
            rate_control: RateControl::Crf(0),
            preset: "medium".into(),
            audio: AudioEncodingSettings {
                bitrate_kbps: 1536,
                sample_rate_hz: 48_000,
            },
            post_filter: ExportPostFilter::default(),
            bt709_tags: true,
            faststart: false,
        }
    }

    /// Resolves a built-in export profile by platform or preset name (case-insensitive).
    pub fn from_platform_name(name: &str) -> Option<Self> {
        let normalized = name.trim().to_ascii_lowercase().replace([' ', '-'], "_");
        match normalized.as_str() {
            "tiktok" | "tiktok_1080p60" | "tiktok_60" => Some(Self::tiktok_1080p60()),
            "reels" | "instagram" | "instagram_reels" | "reels_1080p30" | "ig_reels" => {
                Some(Self::reels_1080p30())
            }
            "youtube" | "yt" | "youtube_1080p60" => Some(Self::youtube_1080p60()),
            "anime" | "anime_10bit" | "anime_10bit_hevc" | "hevc_10bit" | "hevc" => {
                Some(Self::anime_10bit_hevc())
            }
            "prores" | "prores_422hq" | "prores422hq" | "prores_hq" => Some(Self::prores_422hq()),
            _ => None,
        }
    }

    /// Validates all profile fields prior to spawning `ffmpeg`.
    pub fn validate(&self) -> Result<(), MediaError> {
        if !self.codec.is_prores() && !X26X_PRESETS.contains(&self.preset.as_str()) {
            return Err(MediaError::InvalidEncodingSettings(format!(
                "unsupported encoder preset '{}'",
                self.preset
            )));
        }
        match self.rate_control {
            RateControl::Crf(crf) => {
                if !self.codec.is_prores() && crf > 51 {
                    return Err(MediaError::InvalidEncodingSettings(format!(
                        "CRF {crf} exceeds valid 0..=51 range"
                    )));
                }
            }
            RateControl::Vbr {
                target_kbps,
                max_kbps,
                bufsize_kbps,
            } => {
                if target_kbps == 0 || max_kbps < target_kbps || bufsize_kbps == 0 {
                    return Err(MediaError::InvalidEncodingSettings(format!(
                        "invalid VBR settings: target={target_kbps}k, max={max_kbps}k, bufsize={bufsize_kbps}k"
                    )));
                }
            }
        }
        if self.audio.sample_rate_hz == 0 || self.audio.bitrate_kbps == 0 {
            return Err(MediaError::InvalidEncodingSettings(format!(
                "invalid audio encoding settings: {} Hz, {} kbps",
                self.audio.sample_rate_hz, self.audio.bitrate_kbps
            )));
        }
        if !self.post_filter.unsharp_amount.is_finite()
            || self.post_filter.unsharp_amount < 0.0
            || self.post_filter.unsharp_amount > 5.0
        {
            return Err(MediaError::InvalidEncodingSettings(format!(
                "unsharp_amount {} must be finite and within 0.0..=5.0",
                self.post_filter.unsharp_amount
            )));
        }
        if self.pixel_format == PixelFormat::Yuva444p10le && self.codec != VideoCodec::ProRes4444 {
            return Err(MediaError::InvalidEncodingSettings(
                "yuva444p10le alpha pixel format is only supported with ProRes4444".into(),
            ));
        }
        Ok(())
    }

    fn effective_ffmpeg_pix_fmt(&self) -> &'static str {
        match (self.codec, self.pixel_format) {
            (VideoCodec::ProRes422Hq, _) => "yuv422p10le",
            (VideoCodec::ProRes4444, PixelFormat::Yuva444p10le) => "yuva444p10le",
            (VideoCodec::ProRes4444, _) => "yuv444p10le",
            (_, pf) => pf.ffmpeg_pix_fmt(),
        }
    }
}

/// Reproducible software H.264 encoding settings for MP4 output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoEncodingSettings {
    /// FFmpeg libx264 preset (validated against its standard preset names).
    pub preset: String,
    /// Constant rate factor in `0..=51`.
    pub crf: u8,
}

impl Default for VideoEncodingSettings {
    fn default() -> Self {
        Self {
            preset: "veryfast".into(),
            crf: 16,
        }
    }
}

impl VideoEncodingSettings {
    pub fn validate(&self) -> Result<(), MediaError> {
        if !X26X_PRESETS.contains(&self.preset.as_str()) || self.crf > 51 {
            return Err(MediaError::InvalidEncodingSettings(format!(
                "unsupported libx264 settings: preset='{}', crf={} (expected a standard preset and crf 0..=51)",
                self.preset, self.crf
            )));
        }
        Ok(())
    }
}

impl From<&VideoEncodingSettings> for ExportEncodingProfile {
    fn from(settings: &VideoEncodingSettings) -> Self {
        Self {
            preset: settings.preset.clone(),
            rate_control: RateControl::Crf(settings.crf),
            ..Self::default()
        }
    }
}

impl From<VideoEncodingSettings> for ExportEncodingProfile {
    fn from(settings: VideoEncodingSettings) -> Self {
        Self::from(&settings)
    }
}

/// Detected FFmpeg hardware acceleration capabilities on the host.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct HwAccelCapability {
    pub available_hwaccels: Vec<String>,
    pub preferred_decoder: Option<String>,
    pub preferred_encoder: Option<String>,
}

/// Execution backend selected or fallen back to by policy-based I/O.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum EncodeBackend {
    Hardware,
    #[default]
    SoftwareFallback,
}

/// How a decoded frame is mapped into the requested width and height.
///
/// [`Stretch`](SourceFit::Stretch) is ffmpeg `-s` and keeps the historical full-frame scale.
/// [`Cover`](SourceFit::Cover) scales uniformly, then crops overflow.
/// [`Contain`](SourceFit::Contain) scales uniformly and letterboxes the remainder
/// with transparent black.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SourceFit {
    #[default]
    Stretch,
    Cover,
    Contain,
}

/// Probes and parses FFmpeg hardware decode/encode capabilities.
#[derive(Debug, Clone, Copy, Default)]
pub struct HwAccelDetector;

impl HwAccelDetector {
    const DECODER_PRIORITY: &[&str] = &[
        "cuda",
        "d3d11va",
        "d3d12va",
        "dxva2",
        "qsv",
        "videotoolbox",
        "vaapi",
        "vulkan",
        "amf",
    ];

    const ENCODER_PRIORITY: &[&str] = &[
        "h264_nvenc",
        "h264_qsv",
        "h264_amf",
        "h264_videotoolbox",
        "h264_vaapi",
        "h264_d3d12va",
        "h264_mf",
    ];

    /// Queries `ffmpeg -hide_banner -hwaccels` and `ffmpeg -hide_banner -encoders`.
    pub fn detect() -> Result<HwAccelCapability, MediaError> {
        let hw_out = Command::new("ffmpeg")
            .args(["-hide_banner", "-hwaccels"])
            .output()
            .map_err(|e| MediaError::BinarySpawnFailed {
                binary: "ffmpeg".into(),
                reason: e.to_string(),
            })?;
        if !hw_out.status.success() {
            return Err(MediaError::FfmpegFailed {
                exit_code: hw_out.status.code(),
                stderr: String::from_utf8_lossy(&hw_out.stderr).trim().to_owned(),
            });
        }

        let enc_out = Command::new("ffmpeg")
            .args(["-hide_banner", "-encoders"])
            .output()
            .map_err(|e| MediaError::BinarySpawnFailed {
                binary: "ffmpeg".into(),
                reason: e.to_string(),
            })?;
        if !enc_out.status.success() {
            return Err(MediaError::FfmpegFailed {
                exit_code: enc_out.status.code(),
                stderr: String::from_utf8_lossy(&enc_out.stderr).trim().to_owned(),
            });
        }

        let hw_text = String::from_utf8_lossy(&hw_out.stdout);
        let enc_text = String::from_utf8_lossy(&enc_out.stdout);
        Ok(Self::from_command_outputs(&hw_text, &enc_text))
    }

    /// Parses raw stdout text from `-hwaccels` and `-encoders` deterministically.
    pub fn from_command_outputs(hwaccels_stdout: &str, encoders_stdout: &str) -> HwAccelCapability {
        let available_hwaccels = Self::parse_hwaccels(hwaccels_stdout);
        let video_encoders = Self::parse_video_encoders(encoders_stdout);

        let preferred_decoder = Self::DECODER_PRIORITY
            .iter()
            .find(|&&cand| available_hwaccels.iter().any(|h| h == cand))
            .map(|&s| s.to_owned());

        let preferred_encoder = if available_hwaccels.is_empty() {
            None
        } else {
            Self::ENCODER_PRIORITY
                .iter()
                .find(|&&cand| video_encoders.iter().any(|e| e == cand))
                .map(|&s| s.to_owned())
        };

        HwAccelCapability {
            available_hwaccels,
            preferred_decoder,
            preferred_encoder,
        }
    }

    fn parse_hwaccels(stdout: &str) -> Vec<String> {
        let mut out = Vec::new();
        for line in stdout.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty()
                || trimmed.ends_with(':')
                || trimmed.contains(' ')
                || trimmed.starts_with('-')
            {
                continue;
            }
            let lower = trimmed.to_ascii_lowercase();
            if !out.contains(&lower) {
                out.push(lower);
            }
        }
        out
    }

    fn parse_video_encoders(stdout: &str) -> Vec<String> {
        let mut out = Vec::new();
        for line in stdout.lines() {
            let mut tokens = line.split_whitespace();
            if let (Some(flags), Some(name)) = (tokens.next(), tokens.next())
                && flags.len() == 6
                && flags.starts_with('V')
                && name != "="
            {
                let lower = name.to_ascii_lowercase();
                if !out.contains(&lower) {
                    out.push(lower);
                }
            }
        }
        out
    }
}

/// Metadata summary extracted from a media container via `ffprobe`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaProbeInfo {
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub pixel_format: Option<String>,
    #[serde(default)]
    pub color_space: Option<String>,
    #[serde(default)]
    pub color_transfer: Option<String>,
    #[serde(default)]
    pub color_primaries: Option<String>,
    #[serde(default)]
    pub color_range: Option<String>,
    #[serde(default)]
    pub chroma_location: Option<String>,
    pub width: u32,
    pub height: u32,
    pub fps: Option<Rational32>,
    pub duration_ticks: Option<Flicks>,
    pub pts_index: PtsIndex,
    pub has_audio: bool,
    pub audio_sample_rate: Option<u32>,
    pub audio_channels: Option<u16>,
}

impl MediaProbeInfo {
    /// Derives [`YuvColorInfo`] from the probed stream colorimetry tags.
    pub fn yuv_color_info(&self) -> YuvColorInfo {
        YuvColorInfo::from_probe_tags(
            self.width,
            self.height,
            self.color_space.as_deref(),
            self.color_transfer.as_deref(),
            self.color_range.as_deref(),
            self.chroma_location.as_deref(),
        )
    }

    /// Returns `true` when NV12 4:2:0 decoding is valid for `(target_width, target_height)`
    /// and the source pixel format carries no alpha channel.
    pub fn prefers_nv12_decode(&self, target_width: u32, target_height: u32) -> bool {
        !YuvColorInfo::should_fallback_to_rgba(
            target_width,
            target_height,
            self.pixel_format.as_deref(),
        )
    }
}

/// Subprocess raw-pipe bridge to `ffmpeg` and `ffprobe`.
///
/// Strictly handles raw pixel/PCM/PTS decoding and MP4 muxing/encoding;
/// creative effects and compositing live in the GPU compositor (`ADR-0001`).
#[derive(Debug, Clone, Copy, Default)]
pub struct FfmpegBridge;

impl FfmpegBridge {
    /// Computes a stable SHA-256 identity for an in-memory byte slice.
    pub fn hash_bytes(bytes: &[u8]) -> FileContentIdentity {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        FileContentIdentity {
            byte_len: bytes.len() as u64,
            sha256: hex_sha256(hasher.finalize()),
        }
    }

    /// Computes a stable SHA-256 identity for a file without loading it all into memory.
    pub fn hash_file(path: &Path) -> Result<FileContentIdentity, MediaError> {
        let mut file = std::fs::File::open(path)?;
        let mut hasher = Sha256::new();
        let mut byte_len = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            byte_len = byte_len.saturating_add(read as u64);
            hasher.update(&buffer[..read]);
        }
        let sha256 = hex_sha256(hasher.finalize());
        Ok(FileContentIdentity { byte_len, sha256 })
    }

    /// Probes a video file with `ffprobe` to build a deterministic `PtsIndex`
    /// using `i128` rational arithmetic over stream `time_base` and frame PTS values.
    pub fn probe_pts_index(path: &Path) -> Result<PtsIndex, MediaError> {
        Ok(Self::probe_media_info(path)?.pts_index)
    }

    /// Probes stream metadata and builds the video `PtsIndex` via `ffprobe`.
    pub fn probe_media_info(path: &Path) -> Result<MediaProbeInfo, MediaError> {
        let output = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=codec_type,codec_name,width,height,pix_fmt,color_space,color_transfer,color_primaries,color_range,chroma_location,time_base,avg_frame_rate,r_frame_rate,start_pts,duration_ts,sample_rate,channels",
                "-show_entries",
                "frame=media_type,pts,best_effort_timestamp,pts_time",
                "-of",
                "json",
            ])
            .arg(path)
            .output()
            .map_err(|e| MediaError::BinarySpawnFailed {
                binary: "ffprobe".into(),
                reason: e.to_string(),
            })?;

        if !output.status.success() {
            return Err(MediaError::FfprobeFailed {
                exit_code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            });
        }

        let parsed: FfprobeOutput = serde_json::from_slice(&output.stdout)
            .map_err(|e| MediaError::ParseError(format!("invalid ffprobe JSON: {e}")))?;

        let video_stream = parsed
            .streams
            .iter()
            .find(|s| s.codec_type.as_deref() == Some("video"))
            .ok_or_else(|| {
                MediaError::EmptyMediaStream(format!(
                    "no video stream found in '{}'",
                    path.display()
                ))
            })?;

        let audio_stream = parsed
            .streams
            .iter()
            .find(|s| s.codec_type.as_deref() == Some("audio"));

        let width = video_stream.width.unwrap_or(0);
        let height = video_stream.height.unwrap_or(0);
        if width == 0 || height == 0 {
            return Err(MediaError::InvalidDimensions { width, height });
        }

        let (tb_num, tb_den) = parse_rational_pair(
            video_stream
                .time_base
                .as_deref()
                .ok_or_else(|| MediaError::ParseError("missing video stream time_base".into()))?,
        )?;

        let fps = video_stream
            .avg_frame_rate
            .as_deref()
            .and_then(|s| parse_rational_pair(s).ok())
            .filter(|&(n, d)| n > 0 && d > 0)
            .or_else(|| {
                video_stream
                    .r_frame_rate
                    .as_deref()
                    .and_then(|s| parse_rational_pair(s).ok())
                    .filter(|&(n, d)| n > 0 && d > 0)
            })
            .and_then(|(n, d)| Rational32::new(n, d).ok());

        let video_frames: Vec<&FfprobeFrame> = parsed
            .frames
            .iter()
            .filter(|f| f.media_type.as_deref().is_none_or(|m| m == "video"))
            .collect();

        if video_frames.is_empty() {
            return Err(MediaError::EmptyMediaStream(format!(
                "video stream in '{}' contains zero frames",
                path.display()
            )));
        }

        let base_pts = video_stream.start_pts.or_else(|| {
            video_frames
                .first()
                .and_then(|f| f.pts.or(f.best_effort_timestamp))
        });

        let mut pts_list = Vec::with_capacity(video_frames.len());
        for frame in &video_frames {
            let ticks = if let Some(raw_pts) = frame.pts.or(frame.best_effort_timestamp) {
                let rel_pts = raw_pts.saturating_sub(base_pts.unwrap_or(0));
                raw_pts_to_flicks(rel_pts, tb_num, tb_den)?
            } else if let Some(pts_time_str) = frame.pts_time.as_deref() {
                let sec: f64 = pts_time_str.parse().map_err(|e| {
                    MediaError::ParseError(format!("invalid pts_time '{pts_time_str}': {e}"))
                })?;
                Flicks::from_seconds_f64_approx(sec)?
            } else {
                return Err(MediaError::InvalidPtsIndex(
                    "frame is missing pts, best_effort_timestamp, and pts_time".into(),
                ));
            };
            pts_list.push(ticks);
        }

        let pts_index = PtsIndex::new(pts_list)?;

        let duration_ticks = video_stream
            .duration_ts
            .map(|dur| raw_pts_to_flicks(dur, tb_num, tb_den))
            .transpose()?;

        let audio_sample_rate = audio_stream
            .and_then(|s| s.sample_rate.as_deref())
            .and_then(|s| s.parse::<u32>().ok());
        let audio_channels = audio_stream.and_then(|s| s.channels);

        Ok(MediaProbeInfo {
            video_codec: video_stream.codec_name.clone(),
            audio_codec: audio_stream.and_then(|s| s.codec_name.clone()),
            pixel_format: video_stream.pix_fmt.clone(),
            color_space: video_stream.color_space.clone(),
            color_transfer: video_stream.color_transfer.clone(),
            color_primaries: video_stream.color_primaries.clone(),
            color_range: video_stream.color_range.clone(),
            chroma_location: video_stream.chroma_location.clone(),
            width,
            height,
            fps,
            duration_ticks,
            pts_index,
            has_audio: audio_stream.is_some(),
            audio_sample_rate,
            audio_channels,
        })
    }

    /// Probes host FFmpeg hardware acceleration methods and H.264 hardware encoders.
    pub fn detect_hwaccel_capabilities() -> Result<HwAccelCapability, MediaError> {
        HwAccelDetector::detect()
    }

    /// Seeks to the matched PTS for `source_tick` and decodes a single raw `Rgba8Frame`
    /// at `(width, height)` via `ffmpeg`.
    ///
    /// This is [`SourceFit::Stretch`]: the delivered frame is scaled with `-s`.
    pub fn decode_video_frame(
        path: &Path,
        pts_index: &PtsIndex,
        source_tick: Flicks,
        width: u32,
        height: u32,
    ) -> Result<Rgba8Frame, MediaError> {
        Self::decode_video_frame_with_fit(
            path,
            pts_index,
            source_tick,
            width,
            height,
            SourceFit::Stretch,
        )
    }

    /// Decodes one frame into `(width, height)` using `fit`.
    ///
    /// [`SourceFit::Stretch`] calls the same `-s` scale as [`Self::decode_video_frame`].
    pub fn decode_video_frame_with_fit(
        path: &Path,
        pts_index: &PtsIndex,
        source_tick: Flicks,
        width: u32,
        height: u32,
        fit: SourceFit,
    ) -> Result<Rgba8Frame, MediaError> {
        Self::decode_video_frame_internal(path, pts_index, source_tick, width, height, fit, None)
    }

    /// Seeks once to `start_frame_index` and decodes up to `window_len` consecutive
    /// raw `Rgba8Frame`s at `(width, height)` in a single `ffmpeg` rawvideo pipe pass.
    ///
    /// This is [`SourceFit::Stretch`].
    pub fn decode_video_window(
        path: &Path,
        pts_index: &PtsIndex,
        start_frame_index: usize,
        window_len: usize,
        width: u32,
        height: u32,
    ) -> Result<Vec<(usize, Rgba8Frame)>, MediaError> {
        Self::decode_video_window_with_fit(
            path,
            pts_index,
            start_frame_index,
            window_len,
            width,
            height,
            SourceFit::Stretch,
        )
    }

    /// Decodes a single frame at `source_tick`, preferring NV12 planar output when
    /// `(width, height)` are even and `probe.pixel_format` has no alpha channel, and
    /// falling back automatically to `Rgba8` otherwise.
    pub fn decode_video_frame_decoded_with_fit(
        path: &Path,
        probe: &MediaProbeInfo,
        source_tick: Flicks,
        width: u32,
        height: u32,
        fit: SourceFit,
    ) -> Result<DecodedVideoFrame, MediaError> {
        if width == 0 || height == 0 {
            return Err(MediaError::InvalidDimensions { width, height });
        }
        if !probe.prefers_nv12_decode(width, height) {
            return Self::decode_video_frame_with_fit(
                path,
                &probe.pts_index,
                source_tick,
                width,
                height,
                fit,
            )
            .map(DecodedVideoFrame::Rgba8);
        }

        let matched = probe.pts_index.lookup_frame(source_tick).ok_or_else(|| {
            MediaError::EmptyMediaStream(format!(
                "cannot decode from empty PtsIndex for '{}'",
                path.display()
            ))
        })?;

        let mut cmd = Command::new("ffmpeg");
        cmd.args(["-v", "error", "-nostdin"]);
        if let Some(seek_sec) = midpoint_seek_seconds(&probe.pts_index, matched.frame_index) {
            cmd.arg("-ss").arg(format!("{seek_sec:.9}"));
        }
        cmd.arg("-i")
            .arg(path)
            .args(["-fps_mode", "passthrough", "-frames:v", "1"]);
        let (decode_w, decode_h, content_uv_rect) = append_nv12_fitted_output(
            &mut cmd,
            Some((probe.width, probe.height)),
            width,
            height,
            fit,
        );
        cmd.args(["-pix_fmt", "nv12", "-f", "rawvideo", "pipe:1"]);

        let output = cmd.output().map_err(|e| MediaError::BinarySpawnFailed {
            binary: "ffmpeg".into(),
            reason: e.to_string(),
        })?;
        if !output.status.success() {
            return Err(MediaError::FfmpegFailed {
                exit_code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            });
        }

        let (y_len, uv_len) = validated_nv12_plane_lens(decode_w, decode_h)?;
        let expected = y_len + uv_len;
        if output.stdout.len() != expected {
            return Err(MediaError::BufferLengthMismatch {
                expected,
                actual: output.stdout.len(),
            });
        }

        Nv12Frame::from_contiguous(
            decode_w,
            decode_h,
            matched.pts,
            &output.stdout,
            probe.yuv_color_info(),
            content_uv_rect,
        )
        .map(DecodedVideoFrame::Nv12)
    }

    /// Decodes a window of frames using NV12 when supported or automatic `Rgba8` fallback.
    pub fn decode_video_window_decoded_with_fit(
        path: &Path,
        probe: &MediaProbeInfo,
        start_frame_index: usize,
        window_len: usize,
        width: u32,
        height: u32,
        fit: SourceFit,
    ) -> Result<Vec<(usize, DecodedVideoFrame)>, MediaError> {
        if width == 0 || height == 0 {
            return Err(MediaError::InvalidDimensions { width, height });
        }
        if probe.pts_index.is_empty() {
            return Err(MediaError::EmptyMediaStream(format!(
                "cannot decode from empty PtsIndex for '{}'",
                path.display()
            )));
        }
        if start_frame_index >= probe.pts_index.len() || window_len == 0 {
            return Ok(Vec::new());
        }
        if !probe.prefers_nv12_decode(width, height) {
            return Self::decode_video_window_with_fit(
                path,
                &probe.pts_index,
                start_frame_index,
                window_len,
                width,
                height,
                fit,
            )
            .map(|frames| {
                frames
                    .into_iter()
                    .map(|(i, f)| (i, DecodedVideoFrame::Rgba8(f)))
                    .collect()
            });
        }

        let count = window_len.min(probe.pts_index.len() - start_frame_index);
        let mut cmd = Command::new("ffmpeg");
        cmd.args(["-v", "error", "-nostdin"]);
        if let Some(seek_sec) = midpoint_seek_seconds(&probe.pts_index, start_frame_index) {
            cmd.arg("-ss").arg(format!("{seek_sec:.9}"));
        }
        let count_arg = count.to_string();
        cmd.arg("-i")
            .arg(path)
            .args(["-fps_mode", "passthrough", "-frames:v", &count_arg]);
        let (decode_w, decode_h, content_uv_rect) = append_nv12_fitted_output(
            &mut cmd,
            Some((probe.width, probe.height)),
            width,
            height,
            fit,
        );
        cmd.args(["-pix_fmt", "nv12", "-f", "rawvideo", "pipe:1"]);

        let output = cmd.output().map_err(|e| MediaError::BinarySpawnFailed {
            binary: "ffmpeg".into(),
            reason: e.to_string(),
        })?;
        if !output.status.success() {
            return Err(MediaError::FfmpegFailed {
                exit_code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            });
        }

        let (y_len, uv_len) = validated_nv12_plane_lens(decode_w, decode_h)?;
        let expected_bytes = y_len + uv_len;
        if output.stdout.is_empty() || !output.stdout.len().is_multiple_of(expected_bytes) {
            return Err(MediaError::BufferLengthMismatch {
                expected: expected_bytes,
                actual: output.stdout.len(),
            });
        }
        let actual_frames = output.stdout.len() / expected_bytes;
        if actual_frames > count {
            return Err(MediaError::BufferLengthMismatch {
                expected: count * expected_bytes,
                actual: output.stdout.len(),
            });
        }

        let pts_slice = probe.pts_index.as_slice();
        let color_info = probe.yuv_color_info();
        let mut decoded = Vec::with_capacity(actual_frames);
        for (i, chunk) in output.stdout.chunks_exact(expected_bytes).enumerate() {
            let frame_idx = start_frame_index + i;
            let pts = pts_slice[frame_idx];
            let frame = Nv12Frame::from_contiguous(
                decode_w,
                decode_h,
                pts,
                chunk,
                color_info,
                content_uv_rect,
            )?;
            decoded.push((frame_idx, DecodedVideoFrame::Nv12(frame)));
        }
        Ok(decoded)
    }

    /// Decodes a window of frames into `(width, height)` using `fit`.
    ///
    /// [`SourceFit::Stretch`] uses the same `-s` scale as [`Self::decode_video_window`].
    pub fn decode_video_window_with_fit(
        path: &Path,
        pts_index: &PtsIndex,
        start_frame_index: usize,
        window_len: usize,
        width: u32,
        height: u32,
        fit: SourceFit,
    ) -> Result<Vec<(usize, Rgba8Frame)>, MediaError> {
        if width == 0 || height == 0 {
            return Err(MediaError::InvalidDimensions { width, height });
        }
        if pts_index.is_empty() {
            return Err(MediaError::EmptyMediaStream(format!(
                "cannot decode from empty PtsIndex for '{}'",
                path.display()
            )));
        }
        if start_frame_index >= pts_index.len() || window_len == 0 {
            return Ok(Vec::new());
        }

        let expected_bytes = (width as usize)
            .checked_mul(height as usize)
            .and_then(|v| v.checked_mul(4))
            .ok_or(MediaError::InvalidDimensions { width, height })?;
        let count = window_len.min(pts_index.len() - start_frame_index);

        let mut cmd = Command::new("ffmpeg");
        cmd.args(["-v", "error", "-nostdin"]);
        if let Some(seek_sec) = midpoint_seek_seconds(pts_index, start_frame_index) {
            cmd.arg("-ss").arg(format!("{seek_sec:.9}"));
        }

        let count_arg = count.to_string();
        cmd.arg("-i")
            .arg(path)
            .args(["-fps_mode", "passthrough", "-frames:v", &count_arg]);
        append_fitted_output(&mut cmd, width, height, fit);
        cmd.args(["-pix_fmt", "rgba", "-f", "rawvideo", "pipe:1"]);

        let output = cmd.output().map_err(|e| MediaError::BinarySpawnFailed {
            binary: "ffmpeg".into(),
            reason: e.to_string(),
        })?;

        if !output.status.success() {
            return Err(MediaError::FfmpegFailed {
                exit_code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            });
        }

        if output.stdout.is_empty() || !output.stdout.len().is_multiple_of(expected_bytes) {
            return Err(MediaError::BufferLengthMismatch {
                expected: expected_bytes,
                actual: output.stdout.len(),
            });
        }

        let actual_frames = output.stdout.len() / expected_bytes;
        if actual_frames > count {
            return Err(MediaError::BufferLengthMismatch {
                expected: count * expected_bytes,
                actual: output.stdout.len(),
            });
        }

        let pts_slice = pts_index.as_slice();
        let mut decoded = Vec::with_capacity(actual_frames);
        for (i, chunk) in output.stdout.chunks_exact(expected_bytes).enumerate() {
            let frame_idx = start_frame_index + i;
            let pts = pts_slice[frame_idx];
            let frame = Rgba8Frame::new(width, height, pts, AlphaMode::Straight, chunk.to_vec())?;
            decoded.push((frame_idx, frame));
        }
        Ok(decoded)
    }

    /// Decodes a single video frame using hardware acceleration when `use_hw_if_available`
    /// is true and available, falling back automatically to software decoding on failure.
    pub fn decode_video_frame_with_policy(
        path: &Path,
        pts_index: &PtsIndex,
        source_tick: Flicks,
        width: u32,
        height: u32,
        use_hw_if_available: bool,
    ) -> Result<(Rgba8Frame, EncodeBackend), MediaError> {
        if use_hw_if_available
            && let Ok(cap) = Self::detect_hwaccel_capabilities()
            && let Some(ref hw) = cap.preferred_decoder
            && let Ok(frame) = Self::decode_video_frame_internal(
                path,
                pts_index,
                source_tick,
                width,
                height,
                SourceFit::Stretch,
                Some(hw.as_str()),
            )
        {
            return Ok((frame, EncodeBackend::Hardware));
        }

        let frame = Self::decode_video_frame_internal(
            path,
            pts_index,
            source_tick,
            width,
            height,
            SourceFit::Stretch,
            None,
        )?;
        Ok((frame, EncodeBackend::SoftwareFallback))
    }

    fn decode_video_frame_internal(
        path: &Path,
        pts_index: &PtsIndex,
        source_tick: Flicks,
        width: u32,
        height: u32,
        fit: SourceFit,
        hwaccel: Option<&str>,
    ) -> Result<Rgba8Frame, MediaError> {
        if width == 0 || height == 0 {
            return Err(MediaError::InvalidDimensions { width, height });
        }
        let matched = pts_index.lookup_frame(source_tick).ok_or_else(|| {
            MediaError::EmptyMediaStream(format!(
                "cannot decode from empty PtsIndex for '{}'",
                path.display()
            ))
        })?;

        let mut cmd = Command::new("ffmpeg");
        cmd.args(["-v", "error", "-nostdin"]);
        if let Some(hw) = hwaccel {
            cmd.args(["-hwaccel", hw]);
        }

        if let Some(seek_sec) = midpoint_seek_seconds(pts_index, matched.frame_index) {
            cmd.arg("-ss").arg(format!("{seek_sec:.9}"));
        }

        cmd.arg("-i")
            .arg(path)
            .args(["-fps_mode", "passthrough", "-frames:v", "1"]);
        append_fitted_output(&mut cmd, width, height, fit);
        cmd.args(["-pix_fmt", "rgba", "-f", "rawvideo", "pipe:1"]);

        let output = cmd.output().map_err(|e| MediaError::BinarySpawnFailed {
            binary: "ffmpeg".into(),
            reason: e.to_string(),
        })?;

        if !output.status.success() {
            return Err(MediaError::FfmpegFailed {
                exit_code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            });
        }

        let expected_bytes = (width as usize)
            .checked_mul(height as usize)
            .and_then(|v| v.checked_mul(4))
            .ok_or(MediaError::InvalidDimensions { width, height })?;
        if output.stdout.len() != expected_bytes {
            return Err(MediaError::BufferLengthMismatch {
                expected: expected_bytes,
                actual: output.stdout.len(),
            });
        }

        Rgba8Frame::new(
            width,
            height,
            matched.pts,
            AlphaMode::Straight,
            output.stdout,
        )
    }

    /// Decodes a static image file into `(width, height)` using `ffmpeg` without `ffprobe`/`PtsIndex`.
    pub fn decode_image_frame_with_fit(
        path: &Path,
        width: u32,
        height: u32,
        fit: SourceFit,
    ) -> Result<Rgba8Frame, MediaError> {
        if width == 0 || height == 0 {
            return Err(MediaError::InvalidDimensions { width, height });
        }
        let expected_bytes = (width as usize)
            .checked_mul(height as usize)
            .and_then(|v| v.checked_mul(4))
            .ok_or(MediaError::InvalidDimensions { width, height })?;

        let mut cmd = Command::new("ffmpeg");
        cmd.args(["-v", "error", "-nostdin", "-i"])
            .arg(path)
            .args(["-frames:v", "1"]);
        append_fitted_output(&mut cmd, width, height, fit);
        cmd.args(["-pix_fmt", "rgba", "-f", "rawvideo", "pipe:1"])
            .stdin(Stdio::null());

        let output = cmd.output().map_err(|e| MediaError::BinarySpawnFailed {
            binary: "ffmpeg".into(),
            reason: e.to_string(),
        })?;

        if !output.status.success() {
            return Err(MediaError::FfmpegFailed {
                exit_code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            });
        }

        if output.stdout.len() != expected_bytes {
            return Err(MediaError::BufferLengthMismatch {
                expected: expected_bytes,
                actual: output.stdout.len(),
            });
        }

        Rgba8Frame::new(
            width,
            height,
            Flicks::ZERO,
            AlphaMode::Straight,
            output.stdout,
        )
    }

    /// Loads a static image into `(width, height)` using `fit`.
    ///
    /// PNG files (detected by the 8-byte PNG signature) are decoded in pure Rust via
    /// [`Rgba8Frame::load_png`] and resized with [`fit_rgba8_frame`] without spawning `ffmpeg`.
    /// Other image formats (JPEG, WebP, TIFF, BMP, etc.) fall back to
    /// [`Self::decode_image_frame_with_fit`].
    pub fn load_image_frame_with_fit(
        path: &Path,
        width: u32,
        height: u32,
        fit: SourceFit,
    ) -> Result<Rgba8Frame, MediaError> {
        const PNG_MAGIC: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

        if width == 0 || height == 0 {
            return Err(MediaError::InvalidDimensions { width, height });
        }

        let mut header = [0u8; 8];
        let is_png = {
            let mut file = std::fs::File::open(path)?;
            let mut read = 0usize;
            while read < header.len() {
                let n = file.read(&mut header[read..])?;
                if n == 0 {
                    break;
                }
                read += n;
            }
            read == header.len() && header == PNG_MAGIC
        };

        if is_png {
            let decoded = Rgba8Frame::load_png(path, Flicks::ZERO)?;
            fit_rgba8_frame(&decoded, width, height, fit)
        } else {
            Self::decode_image_frame_with_fit(path, width, height, fit)
        }
    }

    /// Decodes the primary audio stream into interleaved `f32` PCM at `(sample_rate, channels)`.
    pub fn decode_audio(
        path: &Path,
        sample_rate: u32,
        channels: u16,
    ) -> Result<AudioBuffer, MediaError> {
        if sample_rate == 0 || channels == 0 {
            return Err(MediaError::InvalidAudioFormat(
                "sample_rate and channels must be non-zero".into(),
            ));
        }
        let output = Command::new("ffmpeg")
            .args(["-v", "error", "-nostdin", "-i"])
            .arg(path)
            .args([
                "-vn",
                "-ac",
                &channels.to_string(),
                "-ar",
                &sample_rate.to_string(),
                "-f",
                "f32le",
                "pipe:1",
            ])
            .output()
            .map_err(|e| MediaError::BinarySpawnFailed {
                binary: "ffmpeg".into(),
                reason: e.to_string(),
            })?;

        if !output.status.success() {
            return Err(MediaError::FfmpegFailed {
                exit_code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            });
        }

        if !output.stdout.len().is_multiple_of(4) {
            return Err(MediaError::InvalidAudioFormat(format!(
                "decoded f32le byte count {} is not a multiple of 4",
                output.stdout.len()
            )));
        }

        let samples: Vec<f32> = output
            .stdout
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&c| f32::from_le_bytes(c))
            .collect();
        AudioBuffer::new(sample_rate, channels, samples)
    }

    /// Pipes raw RGBA8 frames (and optional audio) to `ffmpeg` to produce an H.264 + AAC `.mp4`.
    pub fn encode_mp4(
        frames: &[Rgba8Frame],
        audio: Option<&AudioBuffer>,
        fps: Rational32,
        out_path: &Path,
    ) -> Result<(), MediaError> {
        Self::encode_mp4_internal(frames, audio, fps, out_path, "libx264")
    }

    /// Pipes raw RGBA8 frames (and optional audio) to `ffmpeg` using hardware H.264 encoding
    /// when `use_hw_if_available` is true and available, falling back automatically to
    /// `libx264` software encoding if hardware encoding is unavailable or fails.
    pub fn encode_mp4_with_policy(
        frames: &[Rgba8Frame],
        audio: Option<&AudioBuffer>,
        fps: Rational32,
        out_path: &Path,
        use_hw_if_available: bool,
    ) -> Result<EncodeBackend, MediaError> {
        if use_hw_if_available
            && let Ok(cap) = Self::detect_hwaccel_capabilities()
            && let Some(ref hw_enc) = cap.preferred_encoder
        {
            if Self::encode_mp4_internal(frames, audio, fps, out_path, hw_enc).is_ok() {
                return Ok(EncodeBackend::Hardware);
            }
            let _ = std::fs::remove_file(out_path);
        }

        Self::encode_mp4_internal(frames, audio, fps, out_path, "libx264")?;
        Ok(EncodeBackend::SoftwareFallback)
    }

    fn encode_mp4_internal(
        frames: &[Rgba8Frame],
        audio: Option<&AudioBuffer>,
        fps: Rational32,
        out_path: &Path,
        video_encoder: &str,
    ) -> Result<(), MediaError> {
        let first = frames.first().ok_or_else(|| {
            MediaError::EmptyMediaStream("cannot encode MP4 from zero video frames".into())
        })?;
        if fps.num == 0 || fps.den == 0 {
            return Err(frameshift_time::TimeError::InvalidRational {
                num: fps.num,
                den: fps.den,
            }
            .into());
        }
        let width = first.width;
        let height = first.height;
        if width == 0 || height == 0 {
            return Err(MediaError::InvalidDimensions { width, height });
        }
        let expected_bytes = (width as usize)
            .checked_mul(height as usize)
            .and_then(|v| v.checked_mul(4))
            .ok_or(MediaError::InvalidDimensions { width, height })?;

        for frame in frames {
            if frame.width != width || frame.height != height {
                return Err(MediaError::InvalidDimensions {
                    width: frame.width,
                    height: frame.height,
                });
            }
            if frame.data.len() != expected_bytes {
                return Err(MediaError::BufferLengthMismatch {
                    expected: expected_bytes,
                    actual: frame.data.len(),
                });
            }
        }

        let mut encoder = Mp4FrameEncoder::new(width, height, audio, fps, out_path, video_encoder)?;
        for frame in frames {
            encoder.write_frame(frame)?;
        }
        encoder.finish()
    }
}

fn hex_sha256(bytes: [u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Streaming MP4 encoder that writes raw RGBA8 frames to one ffmpeg child process.
///
/// Unlike [`FfmpegBridge::encode_mp4`], this type does not require every frame to
/// be resident in memory. Callers render one frame, call [`Self::write_frame`],
/// and drop it. [`Self::finish`] flushes stdin and checks the encoder exit status.
/// Dropping an unfinished encoder kills the child process.
pub struct Mp4FrameEncoder {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    stderr_drain: Option<JoinHandle<String>>,
    width: u32,
    height: u32,
    expected_bytes: usize,
    temp_wav_guard: Option<TempFileGuard>,
}

impl Mp4FrameEncoder {
    /// Starts an ffmpeg H.264 (+ optional AAC) encoder writing to `out_path`.
    ///
    /// When `audio` is present it is written to a temporary IEEE-float WAV and
    /// muxed as the audio stream. The temporary file is removed by
    /// [`Self::finish`] or by the drop guard.
    pub fn new(
        width: u32,
        height: u32,
        audio: Option<&AudioBuffer>,
        fps: Rational32,
        out_path: &Path,
        video_encoder: &str,
    ) -> Result<Self, MediaError> {
        Self::new_with_settings(
            width,
            height,
            audio,
            fps,
            out_path,
            video_encoder,
            &VideoEncodingSettings::default(),
        )
    }

    /// Starts an encoder using explicit libx264 quality settings.
    pub fn new_with_settings(
        width: u32,
        height: u32,
        audio: Option<&AudioBuffer>,
        fps: Rational32,
        out_path: &Path,
        video_encoder: &str,
        encoding: &VideoEncodingSettings,
    ) -> Result<Self, MediaError> {
        encoding.validate()?;
        let profile = ExportEncodingProfile::from(encoding);
        Self::spawn_with_profile(
            out_path,
            width,
            height,
            fps,
            audio,
            &profile,
            Some(video_encoder),
        )
    }

    /// Starts an encoder using a full [`ExportEncodingProfile`] (H.264, 10-bit HEVC, ProRes,
    /// platform presets, Rec.709 color tags, faststart, and optional post-filtering).
    pub fn new_with_profile(
        out_path: &Path,
        width: u32,
        height: u32,
        fps: Rational32,
        audio: Option<&AudioBuffer>,
        profile: &ExportEncodingProfile,
    ) -> Result<Self, MediaError> {
        Self::spawn_with_profile(out_path, width, height, fps, audio, profile, None)
    }

    fn spawn_with_profile(
        out_path: &Path,
        width: u32,
        height: u32,
        fps: Rational32,
        audio: Option<&AudioBuffer>,
        profile: &ExportEncodingProfile,
        video_encoder_override: Option<&str>,
    ) -> Result<Self, MediaError> {
        profile.validate()?;
        if fps.num == 0 || fps.den == 0 {
            return Err(frameshift_time::TimeError::InvalidRational {
                num: fps.num,
                den: fps.den,
            }
            .into());
        }
        if width == 0 || height == 0 {
            return Err(MediaError::InvalidDimensions { width, height });
        }
        let expected_bytes = (width as usize)
            .checked_mul(height as usize)
            .and_then(|v| v.checked_mul(4))
            .ok_or(MediaError::InvalidDimensions { width, height })?;

        let temp_wav_guard = if let Some(audio_buf) = audio {
            let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
            let wav_path = std::env::temp_dir()
                .join(format!("frameshift_audio_{}_{seq}.wav", std::process::id()));
            // Own the path before writing so a failed/partial write cannot leak the file.
            let guard = TempFileGuard(wav_path);
            audio_buf.write_wav(&guard.0)?;
            Some(guard)
        } else {
            None
        };

        let args = build_ffmpeg_encode_os_args(
            width,
            height,
            fps,
            temp_wav_guard.as_ref().map(|g| g.0.as_path()),
            out_path,
            profile,
            video_encoder_override,
        )?;

        let mut cmd = Command::new("ffmpeg");
        cmd.args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| MediaError::BinarySpawnFailed {
            binary: "ffmpeg".into(),
            reason: e.to_string(),
        })?;
        let stdin = match child.stdin.take() {
            Some(stdin) => stdin,
            None => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(MediaError::FfmpegFailed {
                    exit_code: None,
                    stderr: "failed to acquire ffmpeg stdin pipe".into(),
                });
            }
        };
        // Drain stderr concurrently so a verbose ffmpeg can never fill the pipe and
        // block while we are still writing frames to stdin.
        let stderr_drain = crate::streaming::spawn_stderr_drain(child.stderr.take());
        Ok(Self {
            child: Some(child),
            stdin: Some(stdin),
            stderr_drain,
            width,
            height,
            expected_bytes,
            temp_wav_guard,
        })
    }

    /// Writes one full-resolution RGBA8 frame to the encoder.
    pub fn write_frame(&mut self, frame: &Rgba8Frame) -> Result<(), MediaError> {
        if frame.width != self.width || frame.height != self.height {
            return Err(MediaError::InvalidDimensions {
                width: frame.width,
                height: frame.height,
            });
        }
        if frame.data.len() != self.expected_bytes {
            return Err(MediaError::BufferLengthMismatch {
                expected: self.expected_bytes,
                actual: frame.data.len(),
            });
        }
        let Some(stdin) = self.stdin.as_mut() else {
            return Err(MediaError::FfmpegFailed {
                exit_code: None,
                stderr: "MP4 encoder is already finished".into(),
            });
        };
        let write_result = stdin.write_all(&frame.data);
        if let Err(write_error) = write_result {
            // ffmpeg died before accepting the frame. Reap it and surface its real
            // stderr instead of a bare broken-pipe I/O error.
            let (wait_result, stderr) = self.reap();
            let exit_code = wait_result
                .and_then(|result| result.ok())
                .and_then(|status| status.code());
            return Err(MediaError::FfmpegFailed {
                exit_code,
                stderr: io_failure_message(
                    "failed to write frame to ffmpeg stdin",
                    &write_error,
                    &stderr,
                ),
            });
        }
        Ok(())
    }

    /// Flushes stdin, waits for ffmpeg, and checks the exit status.
    pub fn finish(mut self) -> Result<(), MediaError> {
        let flush_error = match self.stdin.take() {
            Some(mut stdin) => stdin.flush().err(),
            None => None,
        };
        let (wait_result, stderr) = self.reap();
        self.temp_wav_guard.take();

        let Some(wait_result) = wait_result else {
            return Err(MediaError::FfmpegFailed {
                exit_code: None,
                stderr: "MP4 encoder is already finished".into(),
            });
        };
        let status = wait_result.map_err(|io_err| MediaError::IoError {
            message: io_failure_message(
                "ffmpeg encode: failed to wait on child process",
                &io_err,
                &stderr,
            ),
        })?;

        if let Some(flush_error) = flush_error {
            return Err(MediaError::FfmpegFailed {
                exit_code: status.code(),
                stderr: io_failure_message("failed to flush ffmpeg stdin", &flush_error, &stderr),
            });
        }
        if !status.success() {
            return Err(MediaError::ProcessExit {
                command: "ffmpeg encode".into(),
                exit_code: status.code(),
                stderr,
            });
        }
        Ok(())
    }

    /// Finalizes the encoder by dropping the temporary WAV guard and reaping the
    /// ffmpeg child process.
    pub fn finalize(self) -> Result<(), MediaError> {
        self.finish()
    }

    fn reap(&mut self) -> (Option<std::io::Result<std::process::ExitStatus>>, String) {
        self.stdin.take();
        let wait_result = self.child.take().map(|mut child| child.wait());
        let stderr = self
            .stderr_drain
            .take()
            .and_then(|handle| handle.join().ok())
            .unwrap_or_default();
        (wait_result, stderr)
    }
}

impl Drop for Mp4FrameEncoder {
    fn drop(&mut self) {
        // Close the pipe first; killing ffmpeg while stdin is open can leave it
        // blocked on a write that never completes.
        self.stdin.take();
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        // The drain thread sees EOF once the killed child releases its stderr pipe.
        let _ = self
            .stderr_drain
            .take()
            .and_then(|handle| handle.join().ok());
    }
}

/// Builds the complete `ffmpeg` CLI argument list for encoding raw RGBA8 frames (and optional
/// WAV audio) according to `profile`.
///
/// Exposed as a pure helper so callers and unit tests can validate Rec.709 colorimetry tags,
/// codec/pixel-format selection, H.264 High profile, MP4 faststart, audio settings, and `-vf`
/// post-filters without spawning `ffmpeg`.
pub fn build_ffmpeg_encode_args(
    width: u32,
    height: u32,
    fps: Rational32,
    audio_wav_path: Option<&Path>,
    out_path: &Path,
    profile: &ExportEncodingProfile,
) -> Result<Vec<String>, MediaError> {
    let os_args = build_ffmpeg_encode_os_args(
        width,
        height,
        fps,
        audio_wav_path,
        out_path,
        profile,
        None,
    )?;
    Ok(os_args
        .into_iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect())
}

fn build_ffmpeg_encode_os_args(
    width: u32,
    height: u32,
    fps: Rational32,
    audio_wav_path: Option<&Path>,
    out_path: &Path,
    profile: &ExportEncodingProfile,
    video_encoder_override: Option<&str>,
) -> Result<Vec<OsString>, MediaError> {
    profile.validate()?;
    if fps.num == 0 || fps.den == 0 {
        return Err(frameshift_time::TimeError::InvalidRational {
            num: fps.num,
            den: fps.den,
        }
        .into());
    }
    if width == 0 || height == 0 {
        return Err(MediaError::InvalidDimensions { width, height });
    }

    let mut args: Vec<OsString> = Vec::new();
    let push_str = |args: &mut Vec<OsString>, s: &str| args.push(OsString::from(s));

    let size_arg = format!("{width}x{height}");
    let fps_arg = format!("{}/{}", fps.num, fps.den);
    for flag in [
        "-y", "-v", "error", "-nostdin", "-f", "rawvideo", "-pix_fmt", "rgba", "-s", &size_arg,
        "-r", &fps_arg, "-i", "pipe:0",
    ] {
        push_str(&mut args, flag);
    }

    if let Some(wav_path) = audio_wav_path {
        push_str(&mut args, "-i");
        args.push(wav_path.as_os_str().to_os_string());
    }

    if let Some(vf) = profile.post_filter.to_ffmpeg_vf() {
        push_str(&mut args, "-vf");
        push_str(&mut args, &vf);
    }

    let encoder = video_encoder_override.unwrap_or_else(|| profile.codec.ffmpeg_encoder());
    let is_prores = encoder == "prores_ks" || (video_encoder_override.is_none() && profile.codec.is_prores());
    let is_mov_ext = out_path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("mov"));

    push_str(&mut args, "-c:v");
    push_str(&mut args, encoder);
    push_str(&mut args, "-pix_fmt");
    push_str(&mut args, profile.effective_ffmpeg_pix_fmt());

    if encoder == "libx264" && profile.pixel_format == PixelFormat::Yuv420p {
        push_str(&mut args, "-profile:v");
        push_str(&mut args, "high");
    }

    if is_prores {
        let prores_profile = match profile.codec {
            VideoCodec::ProRes4444 => "4",
            _ => "3",
        };
        push_str(&mut args, "-profile:v");
        push_str(&mut args, prores_profile);
    } else if encoder == "libx264" || encoder == "libx265" {
        push_str(&mut args, "-preset");
        push_str(&mut args, &profile.preset);
        match profile.rate_control {
            RateControl::Crf(crf) => {
                push_str(&mut args, "-crf");
                push_str(&mut args, &crf.to_string());
            }
            RateControl::Vbr {
                target_kbps,
                max_kbps,
                bufsize_kbps,
            } => {
                push_str(&mut args, "-b:v");
                push_str(&mut args, &format!("{target_kbps}k"));
                push_str(&mut args, "-maxrate");
                push_str(&mut args, &format!("{max_kbps}k"));
                push_str(&mut args, "-bufsize");
                push_str(&mut args, &format!("{bufsize_kbps}k"));
            }
        }
        if encoder == "libx265" && !is_mov_ext {
            push_str(&mut args, "-tag:v");
            push_str(&mut args, "hvc1");
        }
    }

    if profile.bt709_tags {
        for flag in [
            "-colorspace",
            "bt709",
            "-color_primaries",
            "bt709",
            "-color_trc",
            "bt709",
            "-color_range",
            "tv",
        ] {
            push_str(&mut args, flag);
        }
    }

    if audio_wav_path.is_some() {
        let sample_rate = profile.audio.sample_rate_hz.to_string();
        if is_prores {
            push_str(&mut args, "-c:a");
            push_str(&mut args, "pcm_s16le");
            push_str(&mut args, "-ar");
            push_str(&mut args, &sample_rate);
        } else {
            let bitrate = format!("{}k", profile.audio.bitrate_kbps);
            push_str(&mut args, "-c:a");
            push_str(&mut args, "aac");
            push_str(&mut args, "-ar");
            push_str(&mut args, &sample_rate);
            push_str(&mut args, "-b:a");
            push_str(&mut args, &bitrate);
        }
    } else {
        push_str(&mut args, "-an");
    }

    if profile.faststart && !is_prores && !is_mov_ext {
        push_str(&mut args, "-movflags");
        push_str(&mut args, "+faststart");
    }

    args.push(out_path.as_os_str().to_os_string());
    Ok(args)
}

fn io_failure_message(context: &str, io_err: &std::io::Error, stderr: &str) -> String {
    let mut message = format!("{context}: {io_err}");
    let stderr = stderr.trim();
    if !stderr.is_empty() {
        message.push_str("\nstderr: ");
        message.push_str(stderr);
    }
    message
}

struct TempFileGuard(std::path::PathBuf);

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn parse_rational_pair(s: &str) -> Result<(u32, u32), MediaError> {
    let (num_s, den_s) = s
        .split_once('/')
        .ok_or_else(|| MediaError::ParseError(format!("expected rational 'num/den', got '{s}'")))?;
    let num: u32 = num_s.trim().parse().map_err(|e| {
        MediaError::ParseError(format!("invalid rational numerator '{num_s}': {e}"))
    })?;
    let den: u32 = den_s.trim().parse().map_err(|e| {
        MediaError::ParseError(format!("invalid rational denominator '{den_s}': {e}"))
    })?;
    if den == 0 {
        return Err(MediaError::ParseError(format!(
            "rational '{s}' has zero denominator"
        )));
    }
    Ok((num, den))
}

fn raw_pts_to_flicks(raw_pts: i64, tb_num: u32, tb_den: u32) -> Result<Flicks, MediaError> {
    let num = (raw_pts as i128) * (FLICKS_PER_SECOND as i128) * (tb_num as i128);
    let den = tb_den as i128;
    let q = num.div_euclid(den);
    let r = num.rem_euclid(den);
    let rounded = if r.saturating_mul(2) >= den { q + 1 } else { q };
    let ticks = i64::try_from(rounded)
        .map_err(|_| MediaError::Time(frameshift_time::TimeError::Overflow))?;
    Ok(Flicks(ticks))
}

#[derive(Debug, Deserialize)]
struct FfprobeOutput {
    #[serde(default)]
    streams: Vec<FfprobeStream>,
    #[serde(default)]
    frames: Vec<FfprobeFrame>,
}

#[derive(Debug, Deserialize)]
struct FfprobeStream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    pix_fmt: Option<String>,
    color_space: Option<String>,
    color_transfer: Option<String>,
    color_primaries: Option<String>,
    color_range: Option<String>,
    chroma_location: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    time_base: Option<String>,
    avg_frame_rate: Option<String>,
    r_frame_rate: Option<String>,
    start_pts: Option<i64>,
    duration_ts: Option<i64>,
    sample_rate: Option<String>,
    channels: Option<u16>,
}

#[derive(Debug, Deserialize)]
struct FfprobeFrame {
    media_type: Option<String>,
    pts: Option<i64>,
    best_effort_timestamp: Option<i64>,
    pts_time: Option<String>,
}

/// Computes the even-aligned inner `(width, height)` and GPU `content_uv_rect`
/// (`[u_min, v_min, u_max, v_max]`) for [`SourceFit::Contain`] without CPU RGBA padding.
pub fn compute_contain_layout(src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> (u32, u32, [f32; 4]) {
    if src_w == 0 || src_h == 0 || dst_w < 2 || dst_h < 2 {
        return (dst_w, dst_h, [0.0, 0.0, 1.0, 1.0]);
    }
    let (inner_w, inner_h) = if (src_w as u64) * (dst_h as u64) >= (dst_w as u64) * (src_h as u64) {
        let w = (dst_w / 2) * 2;
        let scaled_h = ((w as f64) * (src_h as f64) / (src_w as f64)).round() as u32;
        let even_h = ((scaled_h / 2) * 2).clamp(2, (dst_h / 2) * 2);
        (w.max(2), even_h)
    } else {
        let h = (dst_h / 2) * 2;
        let scaled_w = ((h as f64) * (src_w as f64) / (src_h as f64)).round() as u32;
        let even_w = ((scaled_w / 2) * 2).clamp(2, (dst_w / 2) * 2);
        (even_w, h.max(2))
    };
    let pad_x = (dst_w.saturating_sub(inner_w)) as f32 * 0.5;
    let pad_y = (dst_h.saturating_sub(inner_h)) as f32 * 0.5;
    let inv_w = 1.0 / (dst_w as f32);
    let inv_h = 1.0 / (dst_h as f32);
    let rect = [
        pad_x * inv_w,
        pad_y * inv_h,
        (pad_x + inner_w as f32) * inv_w,
        (pad_y + inner_h as f32) * inv_h,
    ];
    (inner_w, inner_h, rect)
}

pub(crate) fn append_nv12_fitted_output(
    cmd: &mut Command,
    src_dims: Option<(u32, u32)>,
    dst_w: u32,
    dst_h: u32,
    fit: SourceFit,
) -> (u32, u32, [f32; 4]) {
    match fit {
        SourceFit::Stretch => {
            cmd.arg("-s").arg(format!("{dst_w}x{dst_h}"));
            (dst_w, dst_h, [0.0, 0.0, 1.0, 1.0])
        }
        SourceFit::Cover => {
            cmd.arg("-vf").arg(format!(
                "scale={dst_w}:{dst_h}:force_original_aspect_ratio=increase:force_divisible_by=2,crop={dst_w}:{dst_h}:(in_w-{dst_w})/2:(in_h-{dst_h})/2"
            ));
            (dst_w, dst_h, [0.0, 0.0, 1.0, 1.0])
        }
        SourceFit::Contain => {
            if let Some((sw, sh)) = src_dims {
                let (inner_w, inner_h, rect) = compute_contain_layout(sw, sh, dst_w, dst_h);
                cmd.arg("-vf").arg(format!("scale={inner_w}:{inner_h}"));
                (inner_w, inner_h, rect)
            } else {
                cmd.arg("-s").arg(format!("{dst_w}x{dst_h}"));
                (dst_w, dst_h, [0.0, 0.0, 1.0, 1.0])
            }
        }
    }
}

pub(crate) fn append_fitted_output(cmd: &mut Command, width: u32, height: u32, fit: SourceFit) {
    match fit {
        SourceFit::Stretch => {
            cmd.arg("-s").arg(format!("{width}x{height}"));
        }
        SourceFit::Cover => {
            cmd.arg("-vf").arg(format!(
                "scale={width}:{height}:force_original_aspect_ratio=increase:force_divisible_by=2,crop={width}:{height}:(in_w-{width})/2:(in_h-{height})/2"
            ));
        }
        SourceFit::Contain => {
            cmd.arg("-vf").arg(format!(
                "scale={width}:{height}:force_original_aspect_ratio=decrease:force_divisible_by=2,format=rgba,pad={width}:{height}:(ow-iw)/2:(oh-ih)/2:color=0x00000000"
            ));
        }
    }
}

pub(crate) fn midpoint_seek_seconds(pts_index: &PtsIndex, frame_index: usize) -> Option<f64> {
    let target_pts = *pts_index.as_slice().get(frame_index)?;
    if frame_index > 0 && target_pts > Flicks::ZERO {
        let prev_pts = pts_index.as_slice()[frame_index - 1];
        let gap = (target_pts.0 - prev_pts.0).max(0);
        // Step slightly behind `target_pts` so float rounding never jumps to the next frame.
        let half_ms = FLICKS_PER_SECOND / 2_000;
        let back_offset = if gap > 1 {
            (gap / 2).min(half_ms).max(1)
        } else {
            0
        };
        let seek_ticks = (target_pts.0 - back_offset).max(0);
        Some(seek_ticks as f64 / FLICKS_PER_SECOND as f64)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synthetic::{SyntheticMediaGenerator, decode_tick_barcode};
    use frameshift_time::FPS_24;
    use std::path::PathBuf;

    struct TempGuard(PathBuf);

    impl Drop for TempGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn file_content_identity_uses_sha256_and_byte_length() {
        let path = std::env::temp_dir().join(format!(
            "frameshift_file_identity_{}.bin",
            std::process::id()
        ));
        let _guard = TempGuard(path.clone());
        std::fs::write(&path, b"abc").expect("write identity fixture");
        let expected = FfmpegBridge::hash_bytes(b"abc");
        let actual = FfmpegBridge::hash_file(&path).expect("hash file");
        assert_eq!(actual.byte_len, 3);
        assert_eq!(
            actual.sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(actual, expected);
    }

    #[test]
    fn streaming_encoder_writes_frames_and_finishes() {
        let fps = FPS_24;
        let width = 64u32;
        let height = 64u32;
        let frames: Vec<Rgba8Frame> = (0..3)
            .map(|i| {
                let tick = fps.frame_to_ticks(i).unwrap();
                SyntheticMediaGenerator::generate_clip_a_frame(width, height, tick)
            })
            .collect();
        let mp4_path = std::env::temp_dir().join(format!(
            "frameshift_stream_encode_test_{}.mp4",
            std::process::id()
        ));
        let _guard = TempGuard(mp4_path.clone());

        let mut encoder =
            Mp4FrameEncoder::new(width, height, None, fps, &mp4_path, "libx264").unwrap();
        for frame in &frames {
            encoder.write_frame(frame).unwrap();
        }
        encoder.finish().unwrap();

        let probe = FfmpegBridge::probe_media_info(&mp4_path).unwrap();
        assert_eq!(probe.width, width);
        assert_eq!(probe.height, height);
        assert_eq!(probe.pts_index.len(), 3);
        assert!(!probe.has_audio);
    }

    #[test]
    fn decode_video_window_matches_single_frame_bit_for_bit() {
        let fps = FPS_24;
        let width = 64u32;
        let height = 64u32;
        let frames: Vec<Rgba8Frame> = (0..24)
            .map(|i| {
                let tick = fps.frame_to_ticks(i).unwrap();
                SyntheticMediaGenerator::generate_clip_a_frame(width, height, tick)
            })
            .collect();

        let mp4_path = std::env::temp_dir().join(format!(
            "frameshift_window_decode_test_{}.mp4",
            std::process::id()
        ));
        let _guard = TempGuard(mp4_path.clone());
        FfmpegBridge::encode_mp4(&frames, None, fps, &mp4_path).unwrap();

        let probe = FfmpegBridge::probe_media_info(&mp4_path).unwrap();
        assert_eq!(probe.pts_index.len(), 24);

        // Window starting at 0 covering frames 0..8
        let win0 =
            FfmpegBridge::decode_video_window(&mp4_path, &probe.pts_index, 0, 8, width, height)
                .unwrap();
        assert_eq!(win0.len(), 8);
        assert_eq!(win0[0].0, 0);
        assert_eq!(win0[5].0, 5);

        let single0 = FfmpegBridge::decode_video_frame(
            &mp4_path,
            &probe.pts_index,
            probe.pts_index.as_slice()[0],
            width,
            height,
        )
        .unwrap();
        let single5 = FfmpegBridge::decode_video_frame(
            &mp4_path,
            &probe.pts_index,
            probe.pts_index.as_slice()[5],
            width,
            height,
        )
        .unwrap();
        assert_eq!(win0[0].1.data, single0.data);
        assert_eq!(win0[0].1.pts, single0.pts);
        assert_eq!(win0[5].1.data, single5.data);
        assert_eq!(win0[5].1.pts, single5.pts);
        assert_eq!(
            decode_tick_barcode(&win0[5].1),
            Some(probe.pts_index.as_slice()[5])
        );

        // Window starting mid-stream at frame 10 covering frames 10..18 (checks frame 12)
        let win10 =
            FfmpegBridge::decode_video_window(&mp4_path, &probe.pts_index, 10, 8, width, height)
                .unwrap();
        assert_eq!(win10.len(), 8);
        assert_eq!(win10[2].0, 12);
        let single12 = FfmpegBridge::decode_video_frame(
            &mp4_path,
            &probe.pts_index,
            probe.pts_index.as_slice()[12],
            width,
            height,
        )
        .unwrap();
        assert_eq!(win10[2].1.data, single12.data);
        assert_eq!(win10[2].1.pts, single12.pts);
        assert_eq!(
            decode_tick_barcode(&win10[2].1),
            Some(probe.pts_index.as_slice()[12])
        );

        // EOF clamp: request 10 frames starting at index 20 -> returns 4 frames (20..24)
        let win_eof =
            FfmpegBridge::decode_video_window(&mp4_path, &probe.pts_index, 20, 10, width, height)
                .unwrap();
        assert_eq!(win_eof.len(), 4);
        assert_eq!(win_eof[0].0, 20);
        assert_eq!(win_eof[3].0, 23);

        // Edge cases: window_len == 0 or start >= len returns empty Ok(vec![])
        assert!(
            FfmpegBridge::decode_video_window(&mp4_path, &probe.pts_index, 0, 0, width, height)
                .unwrap()
                .is_empty()
        );
        assert!(
            FfmpegBridge::decode_video_window(&mp4_path, &probe.pts_index, 24, 4, width, height)
                .unwrap()
                .is_empty()
        );

        // Error cases: zero dimensions, empty PtsIndex
        assert!(matches!(
            FfmpegBridge::decode_video_window(&mp4_path, &probe.pts_index, 0, 4, 0, height),
            Err(MediaError::InvalidDimensions { .. })
        ));
        let empty_pts = PtsIndex::new(Vec::new()).unwrap();
        assert!(matches!(
            FfmpegBridge::decode_video_window(&mp4_path, &empty_pts, 0, 4, width, height),
            Err(MediaError::EmptyMediaStream(_))
        ));
    }

    fn has_flag_pair(args: &[String], key: &str, value: &str) -> bool {
        args.windows(2).any(|w| w[0] == key && w[1] == value)
    }

    #[test]
    fn ffmpeg_arg_builder_applies_bt709_high_profile_faststart_and_audio_for_default() {
        let legacy = VideoEncodingSettings {
            preset: "medium".into(),
            crf: 18,
        };
        let profile = ExportEncodingProfile::from(&legacy);
        let args = build_ffmpeg_encode_args(
            1920,
            1080,
            Rational32::new(60, 1).unwrap(),
            Some(Path::new("audio.wav")),
            Path::new("out.mp4"),
            &profile,
        )
        .unwrap();

        assert!(has_flag_pair(&args, "-c:v", "libx264"));
        assert!(has_flag_pair(&args, "-pix_fmt", "yuv420p"));
        assert!(has_flag_pair(&args, "-profile:v", "high"));
        assert!(has_flag_pair(&args, "-preset", "medium"));
        assert!(has_flag_pair(&args, "-crf", "18"));
        assert!(has_flag_pair(&args, "-colorspace", "bt709"));
        assert!(has_flag_pair(&args, "-color_primaries", "bt709"));
        assert!(has_flag_pair(&args, "-color_trc", "bt709"));
        assert!(has_flag_pair(&args, "-color_range", "tv"));
        assert!(has_flag_pair(&args, "-c:a", "aac"));
        assert!(has_flag_pair(&args, "-ar", "48000"));
        assert!(has_flag_pair(&args, "-b:a", "192k"));
        assert!(has_flag_pair(&args, "-movflags", "+faststart"));
        assert!(!args.iter().any(|a| a == "-vf"));
        assert_eq!(args.last().map(String::as_str), Some("out.mp4"));
    }

    #[test]
    fn platform_presets_resolve_and_build_expected_ffmpeg_arguments() {
        let tiktok = ExportEncodingProfile::from_platform_name("TikTok").unwrap();
        assert_eq!(tiktok, ExportEncodingProfile::tiktok_1080p60());
        let tiktok_args = build_ffmpeg_encode_args(
            1080,
            1920,
            Rational32::new(60, 1).unwrap(),
            Some(Path::new("bgm.wav")),
            Path::new("tiktok.mp4"),
            &tiktok,
        )
        .unwrap();
        assert!(has_flag_pair(&tiktok_args, "-b:v", "18000k"));
        assert!(has_flag_pair(&tiktok_args, "-maxrate", "25000k"));
        assert!(has_flag_pair(&tiktok_args, "-bufsize", "36000k"));
        assert!(has_flag_pair(&tiktok_args, "-profile:v", "high"));
        assert!(has_flag_pair(&tiktok_args, "-ar", "48000"));
        assert!(has_flag_pair(&tiktok_args, "-b:a", "256k"));
        assert!(has_flag_pair(
            &tiktok_args,
            "-vf",
            "deband=range=16:thr=0.02,unsharp=3:3:0.25"
        ));
        assert!(has_flag_pair(&tiktok_args, "-movflags", "+faststart"));

        let reels = ExportEncodingProfile::from_platform_name("instagram-reels").unwrap();
        assert_eq!(reels, ExportEncodingProfile::reels_1080p30());

        let yt = ExportEncodingProfile::from_platform_name("YouTube").unwrap();
        assert_eq!(yt, ExportEncodingProfile::youtube_1080p60());

        let anime_hevc = ExportEncodingProfile::from_platform_name("anime_10bit_hevc").unwrap();
        assert_eq!(anime_hevc, ExportEncodingProfile::anime_10bit_hevc());
        let hevc_args = build_ffmpeg_encode_args(
            1920,
            1080,
            Rational32::new(24, 1).unwrap(),
            None,
            Path::new("anime.mp4"),
            &anime_hevc,
        )
        .unwrap();
        assert!(has_flag_pair(&hevc_args, "-c:v", "libx265"));
        assert!(has_flag_pair(&hevc_args, "-pix_fmt", "yuv420p10le"));
        assert!(has_flag_pair(&hevc_args, "-crf", "14"));
        assert!(has_flag_pair(&hevc_args, "-vf", "deband=range=16:thr=0.02"));
        assert!(!has_flag_pair(&hevc_args, "-profile:v", "high"));
        assert!(hevc_args.iter().any(|a| a == "-an"));

        let prores = ExportEncodingProfile::from_platform_name("prores_422hq").unwrap();
        assert_eq!(prores, ExportEncodingProfile::prores_422hq());
        let prores_args = build_ffmpeg_encode_args(
            1920,
            1080,
            Rational32::new(60, 1).unwrap(),
            Some(Path::new("master.wav")),
            Path::new("master.mov"),
            &prores,
        )
        .unwrap();
        assert!(has_flag_pair(&prores_args, "-c:v", "prores_ks"));
        assert!(has_flag_pair(&prores_args, "-pix_fmt", "yuv422p10le"));
        assert!(has_flag_pair(&prores_args, "-profile:v", "3"));
        assert!(has_flag_pair(&prores_args, "-c:a", "pcm_s16le"));
        assert!(has_flag_pair(&prores_args, "-ar", "48000"));
        assert!(!prores_args.iter().any(|a| a == "-movflags"));
        assert!(!prores_args.iter().any(|a| a == "-b:a"));

        assert!(ExportEncodingProfile::from_platform_name("unknown").is_none());
    }

    #[test]
    fn export_profile_validation_rejects_invalid_configurations() {
        let bad_vbr = ExportEncodingProfile {
            rate_control: RateControl::Vbr {
                target_kbps: 10_000,
                max_kbps: 5_000,
                bufsize_kbps: 20_000,
            },
            ..Default::default()
        };
        assert!(bad_vbr.validate().is_err());

        let bad_alpha = ExportEncodingProfile {
            pixel_format: PixelFormat::Yuva444p10le,
            ..Default::default()
        };
        assert!(bad_alpha.validate().is_err());

        let mut prores4444 = ExportEncodingProfile::prores_422hq();
        prores4444.codec = VideoCodec::ProRes4444;
        prores4444.pixel_format = PixelFormat::Yuva444p10le;
        assert!(prores4444.validate().is_ok());
        let args = build_ffmpeg_encode_args(
            1920,
            1080,
            FPS_24,
            None,
            Path::new("alpha.mov"),
            &prores4444,
        )
        .unwrap();
        assert!(has_flag_pair(&args, "-pix_fmt", "yuva444p10le"));
        assert!(has_flag_pair(&args, "-profile:v", "4"));
    }
}
