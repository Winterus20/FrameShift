use crate::audio::AudioBuffer;
use crate::error::MediaError;
use crate::frame::Sha256;
use crate::frame::{AlphaMode, Rgba8Frame, fit_rgba8_frame};
use frameshift_time::{FLICKS_PER_SECOND, Flicks, PtsIndex, Rational32};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::JoinHandle;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Content identity for a file, computed from its exact bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileContentIdentity {
    pub byte_len: u64,
    /// Lowercase SHA-256 digest.
    pub sha256: String,
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
        const PRESETS: &[&str] = &[
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
        if !PRESETS.contains(&self.preset.as_str()) || self.crf > 51 {
            return Err(MediaError::InvalidEncodingSettings(format!(
                "unsupported libx264 settings: preset='{}', crf={} (expected a standard preset and crf 0..=51)",
                self.preset, self.crf
            )));
        }
        Ok(())
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
#[derive(Debug, Clone, PartialEq)]
pub struct MediaProbeInfo {
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub pixel_format: Option<String>,
    pub width: u32,
    pub height: u32,
    pub fps: Option<Rational32>,
    pub duration_ticks: Option<Flicks>,
    pub pts_index: PtsIndex,
    pub has_audio: bool,
    pub audio_sample_rate: Option<u32>,
    pub audio_channels: Option<u16>,
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
                "stream=codec_type,codec_name,width,height,pix_fmt,time_base,avg_frame_rate,r_frame_rate,start_pts,duration_ts,sample_rate,channels",
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

        let size_arg = format!("{width}x{height}");
        let fps_arg = format!("{}/{}", fps.num, fps.den);
        let mut cmd = Command::new("ffmpeg");
        cmd.args([
            "-y", "-v", "error", "-nostdin", "-f", "rawvideo", "-pix_fmt", "rgba", "-s", &size_arg,
            "-r", &fps_arg, "-i", "pipe:0",
        ]);
        if let Some(ref wav_guard) = temp_wav_guard {
            cmd.arg("-i").arg(&wav_guard.0);
        }
        cmd.args(["-c:v", video_encoder, "-pix_fmt", "yuv420p"]);
        if video_encoder == "libx264" {
            cmd.arg("-preset").arg(&encoding.preset);
            cmd.arg("-crf").arg(encoding.crf.to_string());
        }
        if temp_wav_guard.is_some() {
            cmd.args(["-c:a", "aac", "-b:a", "192k"]);
        } else {
            cmd.arg("-an");
        }
        cmd.arg(out_path)
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
}
