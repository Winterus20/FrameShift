//! Offline AI super-resolution upscaler (`realesrgan-ncnn-vulkan` batch CLI,
//! `ffmpeg` Lanczos fallback, and pure-Rust bilinear fallback) with `ClipAnalysis`
//! anime duplicate-frame deduplication and streaming MP4 chunked processing.

use crate::analysis::ClipAnalysis;
use crate::error::MediaError;
use crate::ffmpeg::{ExportEncodingProfile, FfmpegBridge, Mp4FrameEncoder};
use crate::frame::Rgba8Frame;
use crate::interpolator::find_executable;
use crate::streaming::spawn_stderr_drain;
use frameshift_time::Rational32;
use serde::{Deserialize, Serialize};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static UPSCALER_TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

const LOCAL_REALESRGAN_REL_DIR: &str = "local_media/tools/realesrgan-ncnn-vulkan";

/// Super-resolution backend selection for frame and video upscaling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpscaleBackend {
    /// External `realesrgan-ncnn-vulkan` CLI in single-process batch directory mode.
    #[default]
    RealEsrganNcnnCli,
    /// FFmpeg `scale=w:h:flags=lanczos` rawvideo pipe fallback.
    FfmpegLanczos,
    /// Pure-Rust deterministic bilinear upscaler fallback (zero external tools required).
    BilinearFallback,
}

/// Supported Real-ESRGAN NCNN models shipped in `realesrgan-ncnn-vulkan` v0.2.5.0+.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RealEsrganModel {
    /// `realesr-animevideov3` (`-s 2`, `3`, `4`; fast anime video model with temporal consistency).
    #[default]
    RealesrAnimeVideoV3,
    /// `realesrgan-x4plus-anime` (`-s 4` only; high-detail 2D anime illustration model).
    RealEsrganX4PlusAnime,
    /// `realesrgan-x4plus` (`-s 4` only; general/live-action model).
    RealEsrganX4Plus,
}

impl RealEsrganModel {
    /// Returns the exact CLI `-n` model name accepted by `realesrgan-ncnn-vulkan`.
    pub fn cli_name(self) -> &'static str {
        match self {
            Self::RealesrAnimeVideoV3 => "realesr-animevideov3",
            Self::RealEsrganX4PlusAnime => "realesrgan-x4plus-anime",
            Self::RealEsrganX4Plus => "realesrgan-x4plus",
        }
    }

    /// Parses a CLI or user-facing model identifier into [`RealEsrganModel`].
    pub fn from_name(name: &str) -> Option<Self> {
        let norm = name.trim().to_ascii_lowercase().replace('_', "-");
        match norm.as_str() {
            "realesr-animevideov3" | "realesr-anime-video-v3" | "animevideov3" => {
                Some(Self::RealesrAnimeVideoV3)
            }
            "realesrgan-x4plus-anime" | "x4plus-anime" => Some(Self::RealEsrganX4PlusAnime),
            "realesrgan-x4plus" | "x4plus" => Some(Self::RealEsrganX4Plus),
            _ => None,
        }
    }

    /// Supported integer scale factors (`-s`) for this model.
    pub fn supported_scales(self) -> &'static [u32] {
        match self {
            Self::RealesrAnimeVideoV3 => &[2, 3, 4],
            Self::RealEsrganX4PlusAnime | Self::RealEsrganX4Plus => &[4],
        }
    }

    /// Validates `scale` and returns the expected `.param`/`.bin` file stem in `models/`.
    pub fn model_file_stem(self, scale: u32) -> Result<String, MediaError> {
        if !self.supported_scales().contains(&scale) {
            return Err(MediaError::InvalidEncodingSettings(format!(
                "Real-ESRGAN model '{}' does not support scale {}x (supported: {:?})",
                self.cli_name(),
                scale,
                self.supported_scales()
            )));
        }
        Ok(match self {
            Self::RealesrAnimeVideoV3 => format!("realesr-animevideov3-x{scale}"),
            Self::RealEsrganX4PlusAnime => "realesrgan-x4plus-anime".to_owned(),
            Self::RealEsrganX4Plus => "realesrgan-x4plus".to_owned(),
        })
    }
}

/// Configuration for frame/video super-resolution upscaling.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpscaleConfig {
    /// Upscale factor (`2`, `3`, or `4`). Default: `2`.
    pub scale: u32,
    /// Real-ESRGAN model architecture. Default: [`RealEsrganModel::RealesrAnimeVideoV3`].
    pub model: RealEsrganModel,
    /// `None` auto-selects `RealEsrganNcnnCli` -> `FfmpegLanczos` -> `BilinearFallback`.
    pub backend: Option<UpscaleBackend>,
    /// Optional Vulkan GPU index (`-g`, e.g. `Some(1)` for discrete GPU). `None` = auto.
    pub gpu_id: Option<i32>,
    /// Tile size passed to `-t` (`0` = auto, or `>= 32`). Default: `0`.
    pub tile_size: u32,
    /// Luma MAD threshold in `0.0..=1.0` for anime duplicate-run deduplication (`0.008` default).
    pub dedup_threshold: f32,
    /// Adjacent luma MAD threshold in `0.0..=1.0` for hard scene-cut reporting (`0.25` default).
    pub scene_cut_threshold: f32,
    /// Maximum frames decoded and processed per chunk in [`upscale_video_file`] (`64` default).
    pub chunk_size: usize,
    /// Enables Test-Time Augmentation (`-x`, 8x slower) on `realesrgan-ncnn-vulkan`.
    pub tta: bool,
    /// Optional explicit path to `realesrgan-ncnn-vulkan` executable.
    pub realesrgan_binary: Option<PathBuf>,
    /// Optional explicit path to the Real-ESRGAN models folder.
    pub model_dir: Option<PathBuf>,
    /// Optional explicit path to `ffmpeg` executable (used by [`UpscaleBackend::FfmpegLanczos`]).
    pub ffmpeg_binary: Option<PathBuf>,
}

impl Default for UpscaleConfig {
    fn default() -> Self {
        Self {
            scale: 2,
            model: RealEsrganModel::RealesrAnimeVideoV3,
            backend: None,
            gpu_id: None,
            tile_size: 0,
            dedup_threshold: 0.008,
            scene_cut_threshold: 0.25,
            chunk_size: 64,
            tta: false,
            realesrgan_binary: None,
            model_dir: None,
            ffmpeg_binary: None,
        }
    }
}

/// Host discovery report for `realesrgan-ncnn-vulkan` and its bundled NCNN models.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RealEsrganCapabilities {
    pub available: bool,
    pub binary_path: Option<PathBuf>,
    pub model_dir: Option<PathBuf>,
    pub models_found: Vec<String>,
    pub supports_animevideov3: bool,
    pub supports_x4plus_anime: bool,
    pub supports_x4plus: bool,
}

/// Telemetry and diagnostics returned after upscaling a frame sequence or video file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpscaleReport {
    pub input_frames: usize,
    pub unique_frames_upscaled: usize,
    pub duplicate_frames_reused: usize,
    pub scene_cuts: Vec<usize>,
    pub scale: u32,
    pub input_width: u32,
    pub input_height: u32,
    pub output_width: u32,
    pub output_height: u32,
    pub model_name: String,
    pub backend_used: UpscaleBackend,
    pub chunks_processed: usize,
}

struct TempDirGuard(PathBuf);

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn realesrgan_exe_names() -> &'static [&'static str] {
    &["realesrgan-ncnn-vulkan"]
}

fn candidate_local_tool_binaries() -> Vec<PathBuf> {
    let exe_name = if cfg!(windows) {
        "realesrgan-ncnn-vulkan.exe"
    } else {
        "realesrgan-ncnn-vulkan"
    };

    let mut candidates = Vec::new();
    candidates.push(Path::new(LOCAL_REALESRGAN_REL_DIR).join(exe_name));

    let manifest_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    candidates.push(manifest_root.join(LOCAL_REALESRGAN_REL_DIR).join(exe_name));

    if let Ok(exe) = std::env::current_exe() {
        for ancestor in exe.ancestors().skip(1).take(5) {
            candidates.push(ancestor.join(LOCAL_REALESRGAN_REL_DIR).join(exe_name));
        }
    }
    candidates
}

fn collect_model_stems(model_dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(model_dir) else {
        return Vec::new();
    };
    let mut stems = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("param")
            && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
        {
            let bin_path = model_dir.join(format!("{stem}.bin"));
            if bin_path.is_file() {
                stems.push(stem.to_owned());
            }
        }
    }
    stems.sort();
    stems.dedup();
    stems
}

fn resolve_model_dir_for_binary(binary: &Path, explicit_model_dir: Option<&Path>) -> Option<PathBuf> {
    if let Some(dir) = explicit_model_dir {
        if dir.is_dir() {
            let nested = dir.join("models");
            if nested.is_dir() && !collect_model_stems(&nested).is_empty() {
                return Some(nested);
            }
            return Some(dir.to_path_buf());
        }
        return None;
    }

    if let Some(parent) = binary.parent() {
        let adjacent_models = parent.join("models");
        if adjacent_models.is_dir() {
            return Some(adjacent_models);
        }
        let adjacent_models2 = parent.join("models2");
        if adjacent_models2.is_dir() {
            return Some(adjacent_models2);
        }
    }
    None
}

fn has_models_path_segment(path: &Path) -> bool {
    let text = path.to_string_lossy();
    text.contains("models") || text.contains("models2")
}

/// Probes `PATH` and workspace `local_media/tools/realesrgan-ncnn-vulkan/` for
/// `realesrgan-ncnn-vulkan` and its bundled NCNN `.param`/`.bin` models.
pub fn detect_realesrgan_capabilities() -> RealEsrganCapabilities {
    detect_realesrgan_capabilities_with_config(&UpscaleConfig::default())
}

/// Probes Real-ESRGAN availability using optional explicit binary/model paths from `config`.
pub fn detect_realesrgan_capabilities_with_config(
    config: &UpscaleConfig,
) -> RealEsrganCapabilities {
    if let Some(explicit) = config.realesrgan_binary.as_deref() {
        return capabilities_for_binary(
            if explicit.is_file() {
                Some(explicit.to_path_buf())
            } else {
                None
            },
            config.model_dir.as_deref(),
        );
    }

    let path_bin = find_executable(None, realesrgan_exe_names());
    let local_bin = candidate_local_tool_binaries()
        .into_iter()
        .find(|p| p.is_file());

    // Prefer a candidate that includes `realesr-animevideov3` models (Scoop v0.2.0 lacks it).
    match (path_bin, local_bin) {
        (Some(p_bin), Some(l_bin)) => {
            let p_caps = capabilities_for_binary(Some(p_bin), config.model_dir.as_deref());
            if p_caps.supports_animevideov3 {
                p_caps
            } else {
                let l_caps = capabilities_for_binary(Some(l_bin), config.model_dir.as_deref());
                if l_caps.supports_animevideov3 || !p_caps.available {
                    l_caps
                } else {
                    p_caps
                }
            }
        }
        (Some(p_bin), None) => capabilities_for_binary(Some(p_bin), config.model_dir.as_deref()),
        (None, Some(l_bin)) => capabilities_for_binary(Some(l_bin), config.model_dir.as_deref()),
        (None, None) => RealEsrganCapabilities::default(),
    }
}

fn capabilities_for_binary(
    binary_path: Option<PathBuf>,
    explicit_model_dir: Option<&Path>,
) -> RealEsrganCapabilities {
    let Some(bin) = binary_path else {
        return RealEsrganCapabilities::default();
    };

    let model_dir = resolve_model_dir_for_binary(&bin, explicit_model_dir);
    let models_found = model_dir
        .as_deref()
        .map(collect_model_stems)
        .unwrap_or_default();

    let supports_animevideov3 = models_found
        .iter()
        .any(|m| m.starts_with("realesr-animevideov3-x"));
    let supports_x4plus_anime = models_found
        .iter()
        .any(|m| m == "realesrgan-x4plus-anime");
    let supports_x4plus = models_found.iter().any(|m| m == "realesrgan-x4plus");

    RealEsrganCapabilities {
        available: true,
        binary_path: Some(bin),
        model_dir,
        models_found,
        supports_animevideov3,
        supports_x4plus_anime,
        supports_x4plus,
    }
}

/// Resolves the active upscaling backend for `config`.
pub fn resolve_upscale_backend(config: &UpscaleConfig) -> UpscaleBackend {
    if let Some(explicit) = config.backend {
        return explicit;
    }
    if detect_realesrgan_capabilities_with_config(config).available {
        return UpscaleBackend::RealEsrganNcnnCli;
    }
    if find_executable(config.ffmpeg_binary.as_deref(), &["ffmpeg"]).is_some() {
        return UpscaleBackend::FfmpegLanczos;
    }
    UpscaleBackend::BilinearFallback
}

fn validate_upscale_config(
    config: &UpscaleConfig,
    backend: UpscaleBackend,
) -> Result<(), MediaError> {
    if !(2..=4).contains(&config.scale) {
        return Err(MediaError::InvalidEncodingSettings(format!(
            "upscale factor must be 2, 3, or 4 (got {})",
            config.scale
        )));
    }
    if config.tile_size != 0 && config.tile_size < 32 {
        return Err(MediaError::InvalidEncodingSettings(format!(
            "tile_size must be 0 (auto) or >= 32 (got {})",
            config.tile_size
        )));
    }
    if config.chunk_size == 0 {
        return Err(MediaError::InvalidEncodingSettings(
            "chunk_size must be >= 1".into(),
        ));
    }
    if backend == UpscaleBackend::RealEsrganNcnnCli {
        let _ = config.model.model_file_stem(config.scale)?;
    }
    Ok(())
}

/// Builds `realesrgan-ncnn-vulkan` CLI arguments for batch directory upscaling.
pub fn build_realesrgan_batch_args(
    config: &UpscaleConfig,
    in_dir: &Path,
    out_dir: &Path,
    model_dir: Option<&Path>,
) -> Result<Vec<String>, MediaError> {
    validate_upscale_config(config, UpscaleBackend::RealEsrganNcnnCli)?;

    let mut args = vec![
        "-i".to_owned(),
        in_dir.to_string_lossy().into_owned(),
        "-o".to_owned(),
        out_dir.to_string_lossy().into_owned(),
        "-n".to_owned(),
        config.model.cli_name().to_owned(),
        "-s".to_owned(),
        config.scale.to_string(),
        "-f".to_owned(),
        "png".to_owned(),
    ];

    if config.tile_size > 0 {
        args.push("-t".to_owned());
        args.push(config.tile_size.to_string());
    }
    if let Some(gpu_id) = config.gpu_id {
        args.push("-g".to_owned());
        args.push(gpu_id.to_string());
    }
    if let Some(dir) = model_dir {
        if !has_models_path_segment(dir) {
            return Err(MediaError::InvalidEncodingSettings(format!(
                "realesrgan-ncnn-vulkan requires model_dir path '{}' to contain 'models' or 'models2'",
                dir.display()
            )));
        }
        args.push("-m".to_owned());
        args.push(dir.to_string_lossy().into_owned());
    }
    if config.tta {
        args.push("-x".to_owned());
    }
    Ok(args)
}

/// Upscales an in-memory frame sequence while collapsing anime duplicate runs via
/// [`ClipAnalysis`] (`dedup_threshold`) and executing unique frames in a single batch pass.
pub fn upscale_frame_sequence(
    frames: &[Rgba8Frame],
    config: &UpscaleConfig,
) -> Result<(Vec<Rgba8Frame>, UpscaleReport), MediaError> {
    let backend_used = resolve_upscale_backend(config);
    validate_upscale_config(config, backend_used)?;

    if frames.is_empty() {
        return Ok((
            Vec::new(),
            UpscaleReport {
                input_frames: 0,
                unique_frames_upscaled: 0,
                duplicate_frames_reused: 0,
                scene_cuts: Vec::new(),
                scale: config.scale,
                input_width: 0,
                input_height: 0,
                output_width: 0,
                output_height: 0,
                model_name: model_label(backend_used, config.model),
                backend_used,
                chunks_processed: 0,
            },
        ));
    }

    let in_w = frames[0].width;
    let in_h = frames[0].height;
    if in_w == 0 || in_h == 0 {
        return Err(MediaError::InvalidDimensions {
            width: in_w,
            height: in_h,
        });
    }
    for f in frames {
        if f.width != in_w || f.height != in_h {
            return Err(MediaError::InvalidDimensions {
                width: f.width,
                height: f.height,
            });
        }
    }

    let out_w = in_w
        .checked_mul(config.scale)
        .ok_or(MediaError::InvalidDimensions {
            width: in_w,
            height: in_h,
        })?;
    let out_h = in_h
        .checked_mul(config.scale)
        .ok_or(MediaError::InvalidDimensions {
            width: in_w,
            height: in_h,
        })?;

    let refs: Vec<&Rgba8Frame> = frames.iter().collect();
    let analysis = ClipAnalysis::analyze_frame_sequence(
        &refs,
        config.dedup_threshold,
        config.scene_cut_threshold,
    );

    let unique_frames: Vec<&Rgba8Frame> = analysis
        .unique_runs
        .iter()
        .map(|run| &frames[run.start_frame])
        .collect();

    let upscaled_uniques = match backend_used {
        UpscaleBackend::RealEsrganNcnnCli => {
            run_realesrgan_batch(&unique_frames, config, out_w, out_h)?
        }
        UpscaleBackend::FfmpegLanczos => {
            run_ffmpeg_lanczos_batch(&unique_frames, config, in_w, in_h, out_w, out_h)?
        }
        UpscaleBackend::BilinearFallback => unique_frames
            .iter()
            .map(|f| upscale_frame_bilinear(f, config.scale))
            .collect::<Result<Vec<_>, _>>()?,
    };

    if upscaled_uniques.len() != analysis.unique_runs.len() {
        return Err(MediaError::BufferLengthMismatch {
            expected: analysis.unique_runs.len(),
            actual: upscaled_uniques.len(),
        });
    }

    // Re-expand deduplicated runs while preserving every original frame's PTS and alpha mode.
    let mut out_frames = Vec::with_capacity(frames.len());
    for (run_idx, run) in analysis.unique_runs.iter().enumerate() {
        let upscaled = &upscaled_uniques[run_idx];
        for offset in 0..run.count {
            let orig = &frames[run.start_frame + offset];
            out_frames.push(Rgba8Frame::new(
                upscaled.width,
                upscaled.height,
                orig.pts,
                orig.alpha_mode,
                upscaled.data.clone(),
            )?);
        }
    }

    let report = UpscaleReport {
        input_frames: frames.len(),
        unique_frames_upscaled: analysis.unique_frame_count(),
        duplicate_frames_reused: analysis.duplicate_frame_count(),
        scene_cuts: analysis.scene_cuts,
        scale: config.scale,
        input_width: in_w,
        input_height: in_h,
        output_width: out_w,
        output_height: out_h,
        model_name: model_label(backend_used, config.model),
        backend_used,
        chunks_processed: 1,
    };
    Ok((out_frames, report))
}

/// Streams an on-disk video file in `config.chunk_size` frame windows, upscales each chunk
/// with anime duplicate deduplication, preserves audio if present, and encodes an MP4.
pub fn upscale_video_file(
    input: &Path,
    output: &Path,
    config: &UpscaleConfig,
    encode_profile: &ExportEncodingProfile,
) -> Result<UpscaleReport, MediaError> {
    let backend_used = resolve_upscale_backend(config);
    validate_upscale_config(config, backend_used)?;

    let probe = FfmpegBridge::probe_media_info(input)?;
    let total_frames = probe.pts_index.len();
    if total_frames == 0 {
        return Err(MediaError::EmptyMediaStream(format!(
            "input video '{}' has zero frames",
            input.display()
        )));
    }

    let fps = probe.fps.unwrap_or(Rational32 { num: 24, den: 1 });
    if fps.num == 0 || fps.den == 0 {
        return Err(MediaError::InvalidEncodingSettings(
            "input video FPS must have non-zero numerator and denominator".into(),
        ));
    }

    let out_w = probe
        .width
        .checked_mul(config.scale)
        .ok_or(MediaError::InvalidDimensions {
            width: probe.width,
            height: probe.height,
        })?;
    let out_h = probe
        .height
        .checked_mul(config.scale)
        .ok_or(MediaError::InvalidDimensions {
            width: probe.width,
            height: probe.height,
        })?;

    let audio = if probe.has_audio {
        let sr = probe
            .audio_sample_rate
            .unwrap_or(crate::audio::DEFAULT_AUDIO_SAMPLE_RATE);
        let ch = probe.audio_channels.unwrap_or(2);
        Some(FfmpegBridge::decode_audio(input, sr, ch)?)
    } else {
        None
    };

    if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }

    let mut encoder = Mp4FrameEncoder::new_with_profile(
        output,
        out_w,
        out_h,
        fps,
        audio.as_ref(),
        encode_profile,
    )?;

    let mut input_frames_total = 0usize;
    let mut unique_frames_total = 0usize;
    let mut duplicate_frames_total = 0usize;
    let mut scene_cuts_total = Vec::new();
    let mut chunks_processed = 0usize;

    let mut start_idx = 0usize;
    while start_idx < total_frames {
        let window_len = (total_frames - start_idx).min(config.chunk_size);
        let decoded = FfmpegBridge::decode_video_window(
            input,
            &probe.pts_index,
            start_idx,
            window_len,
            probe.width,
            probe.height,
        )?;
        if decoded.is_empty() {
            break;
        }
        let chunk_frames: Vec<Rgba8Frame> = decoded.into_iter().map(|(_, f)| f).collect();
        let chunk_len = chunk_frames.len();
        let (upscaled_chunk, chunk_report) = upscale_frame_sequence(&chunk_frames, config)?;

        for frame in &upscaled_chunk {
            encoder.write_frame(frame)?;
        }

        input_frames_total += chunk_report.input_frames;
        unique_frames_total += chunk_report.unique_frames_upscaled;
        duplicate_frames_total += chunk_report.duplicate_frames_reused;
        scene_cuts_total.extend(chunk_report.scene_cuts.into_iter().map(|c| c + start_idx));
        chunks_processed += 1;
        start_idx += chunk_len;
    }

    encoder.finish()?;

    Ok(UpscaleReport {
        input_frames: input_frames_total,
        unique_frames_upscaled: unique_frames_total,
        duplicate_frames_reused: duplicate_frames_total,
        scene_cuts: scene_cuts_total,
        scale: config.scale,
        input_width: probe.width,
        input_height: probe.height,
        output_width: out_w,
        output_height: out_h,
        model_name: model_label(backend_used, config.model),
        backend_used,
        chunks_processed,
    })
}

fn model_label(backend: UpscaleBackend, model: RealEsrganModel) -> String {
    match backend {
        UpscaleBackend::RealEsrganNcnnCli => model.cli_name().to_owned(),
        UpscaleBackend::FfmpegLanczos => "ffmpeg-lanczos".to_owned(),
        UpscaleBackend::BilinearFallback => "bilinear-fallback".to_owned(),
    }
}

fn run_realesrgan_batch(
    unique_frames: &[&Rgba8Frame],
    config: &UpscaleConfig,
    expected_w: u32,
    expected_h: u32,
) -> Result<Vec<Rgba8Frame>, MediaError> {
    let caps = detect_realesrgan_capabilities_with_config(config);
    let exe = caps.binary_path.ok_or_else(|| MediaError::BinarySpawnFailed {
        binary: "realesrgan-ncnn-vulkan".into(),
        reason: "realesrgan-ncnn-vulkan executable not found in PATH or local_media/tools/realesrgan-ncnn-vulkan; install Real-ESRGAN v0.2.5.0".into(),
    })?;

    let stem = config.model.model_file_stem(config.scale)?;
    if let Some(ref model_dir) = caps.model_dir
        && !caps.models_found.iter().any(|m| m == &stem)
    {
        return Err(MediaError::InvalidEncodingSettings(format!(
            "Real-ESRGAN model files '{stem}.param/.bin' not found in '{}' (found: {:?})",
            model_dir.display(),
            caps.models_found
        )));
    }

    let seq = UPSCALER_TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let root = std::env::temp_dir().join(format!("fs_esrgan_{pid}_{seq}"));
    let in_dir = root.join("in");
    let out_dir = root.join("out");
    let _guard = TempDirGuard(root);
    std::fs::create_dir_all(&in_dir)?;
    std::fs::create_dir_all(&out_dir)?;

    for (idx, frame) in unique_frames.iter().enumerate() {
        let in_path = in_dir.join(format!("f_{idx:08}.png"));
        write_upscaler_input_png(frame, &in_path)?;
    }

    let args = build_realesrgan_batch_args(
        config,
        &in_dir,
        &out_dir,
        caps.model_dir.as_deref(),
    )?;

    let output = Command::new(&exe)
        .args(&args)
        .output()
        .map_err(|e| MediaError::BinarySpawnFailed {
            binary: exe.to_string_lossy().into_owned(),
            reason: e.to_string(),
        })?;

    if !output.status.success() {
        return Err(MediaError::ProcessExit {
            command: exe.to_string_lossy().into_owned(),
            exit_code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }

    let mut result = Vec::with_capacity(unique_frames.len());
    for (idx, src) in unique_frames.iter().enumerate() {
        let out_path = out_dir.join(format!("f_{idx:08}.png"));
        let mut loaded = Rgba8Frame::load_png(&out_path, src.pts)?;
        loaded.alpha_mode = src.alpha_mode;
        if loaded.width != expected_w || loaded.height != expected_h {
            return Err(MediaError::InvalidDimensions {
                width: loaded.width,
                height: loaded.height,
            });
        }
        result.push(loaded);
    }
    Ok(result)
}

/// Writes 3-channel RGB PNG for opaque frames (25% less disk I/O) or 4-channel RGBA PNG
/// when straight alpha transparency is present.
fn write_upscaler_input_png(frame: &Rgba8Frame, path: &Path) -> Result<(), MediaError> {
    let has_alpha = frame.data.as_chunks::<4>().0.iter().any(|px| px[3] != 255);
    if has_alpha {
        let bytes = frame.encode_png_bytes()?;
        std::fs::write(path, bytes)?;
        return Ok(());
    }

    let file = std::fs::File::create(path)?;
    let writer = BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, frame.width, frame.height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut png_writer = encoder
        .write_header()
        .map_err(|e| MediaError::PngEncode(e.to_string()))?;
    let mut rgb = Vec::with_capacity((frame.width as usize) * (frame.height as usize) * 3);
    for px in frame.data.as_chunks::<4>().0 {
        rgb.extend_from_slice(&px[..3]);
    }
    png_writer
        .write_image_data(&rgb)
        .map_err(|e| MediaError::PngEncode(e.to_string()))?;
    Ok(())
}

fn run_ffmpeg_lanczos_batch(
    unique_frames: &[&Rgba8Frame],
    config: &UpscaleConfig,
    in_w: u32,
    in_h: u32,
    out_w: u32,
    out_h: u32,
) -> Result<Vec<Rgba8Frame>, MediaError> {
    let ffmpeg_exe = find_executable(config.ffmpeg_binary.as_deref(), &["ffmpeg"]).ok_or_else(|| {
        MediaError::BinarySpawnFailed {
            binary: "ffmpeg".into(),
            reason: "ffmpeg executable not found in PATH".into(),
        }
    })?;

    let in_size = format!("{in_w}x{in_h}");
    let vf = format!("scale={out_w}:{out_h}:flags=lanczos");
    let out_frame_bytes = (out_w as usize) * (out_h as usize) * 4;

    let mut cmd = Command::new(&ffmpeg_exe);
    cmd.args([
        "-v",
        "error",
        "-nostdin",
        "-f",
        "rawvideo",
        "-pixel_format",
        "rgba",
        "-video_size",
        &in_size,
        "-framerate",
        "24",
        "-i",
        "pipe:0",
        "-vf",
        &vf,
        "-f",
        "rawvideo",
        "-pixel_format",
        "rgba",
        "pipe:1",
    ])
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| MediaError::BinarySpawnFailed {
        binary: ffmpeg_exe.to_string_lossy().into_owned(),
        reason: e.to_string(),
    })?;

    let mut stdin = child.stdin.take().ok_or_else(|| MediaError::FfmpegFailed {
        exit_code: None,
        stderr: "failed to acquire ffmpeg stdin pipe for lanczos upscale".into(),
    })?;
    let mut stdout = child.stdout.take().ok_or_else(|| MediaError::FfmpegFailed {
        exit_code: None,
        stderr: "failed to acquire ffmpeg stdout pipe for lanczos upscale".into(),
    })?;
    let stderr_drain = spawn_stderr_drain(child.stderr.take());

    let payload: Vec<u8> = unique_frames
        .iter()
        .flat_map(|f| f.data.iter().copied())
        .collect();
    let writer_handle = std::thread::spawn(move || -> Result<(), std::io::Error> {
        stdin.write_all(&payload)?;
        stdin.flush()?;
        drop(stdin);
        Ok(())
    });

    let mut raw_out = Vec::with_capacity(unique_frames.len() * out_frame_bytes);
    let read_res = stdout.read_to_end(&mut raw_out);
    let write_res = writer_handle
        .join()
        .map_err(|_| MediaError::IoError {
            message: "lanczos upscale writer thread panicked".into(),
        })?;
    let status = child.wait().map_err(|e| MediaError::IoError {
        message: format!("lanczos upscale wait failed: {e}"),
    })?;
    let stderr_text = stderr_drain
        .and_then(|h| h.join().ok())
        .unwrap_or_default();

    if let Err(e) = write_res {
        return Err(MediaError::FfmpegFailed {
            exit_code: status.code(),
            stderr: format!("stdin write failed ({e}): {stderr_text}"),
        });
    }
    if let Err(e) = read_res {
        return Err(MediaError::FfmpegFailed {
            exit_code: status.code(),
            stderr: format!("stdout read failed ({e}): {stderr_text}"),
        });
    }
    if !status.success() {
        return Err(MediaError::FfmpegFailed {
            exit_code: status.code(),
            stderr: stderr_text,
        });
    }

    let expected_total = unique_frames.len() * out_frame_bytes;
    if raw_out.len() != expected_total {
        return Err(MediaError::BufferLengthMismatch {
            expected: expected_total,
            actual: raw_out.len(),
        });
    }

    let mut result = Vec::with_capacity(unique_frames.len());
    for (idx, chunk) in raw_out.chunks_exact(out_frame_bytes).enumerate() {
        result.push(Rgba8Frame::new(
            out_w,
            out_h,
            unique_frames[idx].pts,
            unique_frames[idx].alpha_mode,
            chunk.to_vec(),
        )?);
    }
    Ok(result)
}

fn upscale_frame_bilinear(src: &Rgba8Frame, scale: u32) -> Result<Rgba8Frame, MediaError> {
    let out_w = src.width * scale;
    let out_h = src.height * scale;
    let mut out = vec![0u8; (out_w as usize) * (out_h as usize) * 4];
    let sw = src.width as usize;
    let sh = src.height as usize;

    for y in 0..out_h {
        let fy = ((y as f32 + 0.5) / scale as f32) - 0.5;
        let fy_clamped = fy.clamp(0.0, (sh - 1) as f32);
        let y0 = fy_clamped.floor() as usize;
        let y1 = (y0 + 1).min(sh - 1);
        let wy = fy_clamped - (y0 as f32);

        for x in 0..out_w {
            let fx = ((x as f32 + 0.5) / scale as f32) - 0.5;
            let fx_clamped = fx.clamp(0.0, (sw - 1) as f32);
            let x0 = fx_clamped.floor() as usize;
            let x1 = (x0 + 1).min(sw - 1);
            let wx = fx_clamped - (x0 as f32);

            let i00 = (y0 * sw + x0) * 4;
            let i10 = (y0 * sw + x1) * 4;
            let i01 = (y1 * sw + x0) * 4;
            let i11 = (y1 * sw + x1) * 4;
            let dst = ((y as usize) * (out_w as usize) + (x as usize)) * 4;

            for c in 0..4 {
                let top = (src.data[i00 + c] as f32) * (1.0 - wx)
                    + (src.data[i10 + c] as f32) * wx;
                let bot = (src.data[i01 + c] as f32) * (1.0 - wx)
                    + (src.data[i11 + c] as f32) * wx;
                let val = top * (1.0 - wy) + bot * wy;
                out[dst + c] = val.round().clamp(0.0, 255.0) as u8;
            }
        }
    }

    Rgba8Frame::new(out_w, out_h, src.pts, src.alpha_mode, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::AlphaMode;
    use crate::interpolator::write_synthetic_motion_mp4;
    use frameshift_time::Flicks;

    fn pattern_frame(w: u32, h: u32, block_x: u32, pts: Flicks) -> Rgba8Frame {
        let mut data = vec![0u8; (w as usize) * (h as usize) * 4];
        for y in 0..h {
            for x in 0..w {
                let idx = ((y as usize) * (w as usize) + (x as usize)) * 4;
                let in_box = x >= block_x && x < block_x + 12 && y >= h / 4 && y < (3 * h) / 4;
                if in_box {
                    data[idx..idx + 4].copy_from_slice(&[240, 200, 60, 255]);
                } else {
                    data[idx..idx + 4].copy_from_slice(&[18, 24, 38, 255]);
                }
            }
        }
        Rgba8Frame::new(w, h, pts, AlphaMode::Straight, data).unwrap()
    }

    #[test]
    fn realesrgan_model_parses_names_and_validates_scales() {
        assert_eq!(
            RealEsrganModel::from_name("realesr-animevideov3"),
            Some(RealEsrganModel::RealesrAnimeVideoV3)
        );
        assert_eq!(
            RealEsrganModel::from_name("realesrgan-x4plus-anime"),
            Some(RealEsrganModel::RealEsrganX4PlusAnime)
        );
        assert_eq!(
            RealEsrganModel::from_name("realesrgan-x4plus"),
            Some(RealEsrganModel::RealEsrganX4Plus)
        );
        assert_eq!(
            RealEsrganModel::RealesrAnimeVideoV3
                .model_file_stem(2)
                .unwrap(),
            "realesr-animevideov3-x2"
        );
        assert_eq!(
            RealEsrganModel::RealesrAnimeVideoV3
                .model_file_stem(3)
                .unwrap(),
            "realesr-animevideov3-x3"
        );
        assert!(RealEsrganModel::RealEsrganX4PlusAnime.model_file_stem(2).is_err());
        assert_eq!(
            RealEsrganModel::RealEsrganX4PlusAnime
                .model_file_stem(4)
                .unwrap(),
            "realesrgan-x4plus-anime"
        );
    }

    #[test]
    fn build_realesrgan_batch_args_validates_scale_tile_and_gpu_flags() {
        let cfg = UpscaleConfig {
            scale: 2,
            model: RealEsrganModel::RealesrAnimeVideoV3,
            gpu_id: Some(1),
            tile_size: 128,
            tta: true,
            ..Default::default()
        };
        let args = build_realesrgan_batch_args(
            &cfg,
            Path::new("tmp/in"),
            Path::new("tmp/out"),
            Some(Path::new("tools/models")),
        )
        .unwrap();
        assert_eq!(
            args,
            vec![
                "-i",
                "tmp/in",
                "-o",
                "tmp/out",
                "-n",
                "realesr-animevideov3",
                "-s",
                "2",
                "-f",
                "png",
                "-t",
                "128",
                "-g",
                "1",
                "-m",
                "tools/models",
                "-x",
            ]
        );

        let bad_scale = UpscaleConfig {
            scale: 5,
            ..Default::default()
        };
        assert!(build_realesrgan_batch_args(&bad_scale, Path::new("a"), Path::new("b"), None).is_err());

        let bad_tile = UpscaleConfig {
            tile_size: 16,
            ..Default::default()
        };
        assert!(build_realesrgan_batch_args(&bad_tile, Path::new("a"), Path::new("b"), None).is_err());

        assert!(
            build_realesrgan_batch_args(
                &cfg,
                Path::new("a"),
                Path::new("b"),
                Some(Path::new("tools/weights"))
            )
            .is_err()
        );
    }

    #[test]
    fn detect_realesrgan_capabilities_finds_local_tool_and_animevideov3() {
        let local_exe = candidate_local_tool_binaries()
            .into_iter()
            .find(|p| p.is_file());
        if local_exe.is_none() {
            return;
        }
        let caps = detect_realesrgan_capabilities();
        assert!(caps.available);
        assert!(caps.binary_path.is_some());
        assert!(caps.supports_animevideov3);
        assert!(caps.supports_x4plus_anime);
        assert!(
            caps.models_found
                .iter()
                .any(|m| m == "realesr-animevideov3-x2")
        );
    }

    #[test]
    fn bilinear_upscale_dedups_anime_twos_and_preserves_pts() {
        let tick_step = 29_400_000i64;
        let frames = vec![
            pattern_frame(64, 36, 4, Flicks(0)),
            pattern_frame(64, 36, 4, Flicks(tick_step)),
            pattern_frame(64, 36, 20, Flicks(tick_step * 2)),
            pattern_frame(64, 36, 20, Flicks(tick_step * 3)),
            pattern_frame(64, 36, 20, Flicks(tick_step * 4)),
            pattern_frame(64, 36, 40, Flicks(tick_step * 5)),
        ];

        let cfg = UpscaleConfig {
            scale: 2,
            backend: Some(UpscaleBackend::BilinearFallback),
            dedup_threshold: 0.008,
            ..Default::default()
        };
        let (out, report) = upscale_frame_sequence(&frames, &cfg).unwrap();
        assert_eq!(report.input_frames, 6);
        assert_eq!(report.unique_frames_upscaled, 3);
        assert_eq!(report.duplicate_frames_reused, 3);
        assert_eq!(report.input_width, 64);
        assert_eq!(report.input_height, 36);
        assert_eq!(report.output_width, 128);
        assert_eq!(report.output_height, 72);
        assert_eq!(report.backend_used, UpscaleBackend::BilinearFallback);
        assert_eq!(out.len(), 6);
        for (i, f) in out.iter().enumerate() {
            assert_eq!(f.width, 128);
            assert_eq!(f.height, 72);
            assert_eq!(f.pts, Flicks((i as i64) * tick_step));
        }
        assert_eq!(out[0].data, out[1].data);
        assert_eq!(out[2].data, out[3].data);
        assert_eq!(out[3].data, out[4].data);
        assert_ne!(out[0].data, out[2].data);
    }

    #[test]
    fn live_realesrgan_cli_batch_upscales_deduped_frames_on_rtx() {
        let caps = detect_realesrgan_capabilities();
        if !caps.available || !caps.supports_animevideov3 {
            return;
        }
        let tick_step = 29_400_000i64;
        let frames = vec![
            pattern_frame(64, 64, 8, Flicks(0)),
            pattern_frame(64, 64, 8, Flicks(tick_step)),
            pattern_frame(64, 64, 36, Flicks(tick_step * 2)),
            pattern_frame(64, 64, 36, Flicks(tick_step * 3)),
        ];
        let cfg = UpscaleConfig {
            scale: 2,
            model: RealEsrganModel::RealesrAnimeVideoV3,
            backend: Some(UpscaleBackend::RealEsrganNcnnCli),
            gpu_id: Some(1),
            dedup_threshold: 0.008,
            ..Default::default()
        };
        let (out, report) = upscale_frame_sequence(&frames, &cfg).unwrap();
        assert_eq!(report.input_frames, 4);
        assert_eq!(report.unique_frames_upscaled, 2);
        assert_eq!(report.duplicate_frames_reused, 2);
        assert_eq!(report.output_width, 128);
        assert_eq!(report.output_height, 128);
        assert_eq!(out.len(), 4);
        assert_eq!(out[0].pts, Flicks(0));
        assert_eq!(out[1].pts, Flicks(tick_step));
        assert_eq!(out[2].pts, Flicks(tick_step * 2));
        assert_eq!(out[3].pts, Flicks(tick_step * 3));
        assert_eq!(out[0].data, out[1].data);
        assert_eq!(out[2].data, out[3].data);
        assert_ne!(out[0].data, out[2].data);
    }

    #[test]
    fn live_upscale_video_file_mp4_to_mp4_chunked() {
        if find_executable(None, &["ffmpeg"]).is_none() {
            return;
        }
        let caps = detect_realesrgan_capabilities();
        let pid = std::process::id();
        let seq = UPSCALER_TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let in_mp4 = std::env::temp_dir().join(format!("fs_upscale_in_{pid}_{seq}.mp4"));
        let out_mp4 = std::env::temp_dir().join(format!("fs_upscale_out_{pid}_{seq}.mp4"));
        let _ = std::fs::remove_file(&in_mp4);
        let _ = std::fs::remove_file(&out_mp4);

        write_synthetic_motion_mp4(&in_mp4, 64, 64, Rational32 { num: 24, den: 1 }, 6).unwrap();

        let cfg = UpscaleConfig {
            scale: 2,
            model: RealEsrganModel::RealesrAnimeVideoV3,
            backend: Some(if caps.supports_animevideov3 {
                UpscaleBackend::RealEsrganNcnnCli
            } else {
                UpscaleBackend::FfmpegLanczos
            }),
            gpu_id: if caps.supports_animevideov3 {
                Some(1)
            } else {
                None
            },
            chunk_size: 4,
            dedup_threshold: 0.008,
            ..Default::default()
        };

        let report =
            upscale_video_file(&in_mp4, &out_mp4, &cfg, &ExportEncodingProfile::default())
                .unwrap();
        assert_eq!(report.input_frames, 6);
        assert_eq!(report.chunks_processed, 2);
        assert_eq!(report.output_width, 128);
        assert_eq!(report.output_height, 128);

        let probe = FfmpegBridge::probe_media_info(&out_mp4).unwrap();
        assert_eq!(probe.width, 128);
        assert_eq!(probe.height, 128);
        assert_eq!(probe.pts_index.len(), 6);

        let _ = std::fs::remove_file(&in_mp4);
        let _ = std::fs::remove_file(&out_mp4);
    }
}
