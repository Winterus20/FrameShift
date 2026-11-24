//! Live ONNX Runtime AI inference sidecar bridge (`RotoscopeMatte` & `DepthMap`)
//! streaming raw RGBA8 buffers over stdin/stdout pipes with zero temporary PNG disk I/O.

use crate::analysis::InterpolatorBackend;
use crate::error::MediaError;
use crate::ffmpeg::{ExportEncodingProfile, FfmpegBridge, Mp4FrameEncoder};
use crate::frame::{AlphaMode, Rgba8Frame, Rgba16fFrame};
use crate::interpolator::find_executable;
use frameshift_time::Flicks;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Temporal normalization mode for monocular depth inference (`--task depth`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DepthNormMode {
    /// Streaming exponential moving average over `[p1, p99]` with scene-cut reset (anti-pumping).
    #[default]
    Ema,
    /// Independent per-frame `[p1, p99]` normalization.
    PerFrame,
    /// Two-pass global `[p0.5, p99.5]` normalization across all frames.
    GlobalPass,
}

impl DepthNormMode {
    pub fn cli_flag(self) -> &'static str {
        match self {
            Self::Ema => "ema",
            Self::PerFrame => "per_frame",
            Self::GlobalPass => "global_pass",
        }
    }
}

/// Output precision / container format for depth map inference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DepthOutputFormat {
    /// 8-bit grayscale RGBA pipe / MP4 (default for legacy callers).
    #[default]
    U8Mp4,
    /// 10-bit HEVC (`yuv420p10le`) MP4 encoding.
    Hevc10BitMp4,
    /// 16-bit grayscale PNG sequence (`depth_XXXXXX.png`) + RG16 pipe.
    Png16Sequence,
    /// 16-bit `RG16` packed (`R = hi, G = lo`) over raw pipe (`65,536` depth levels).
    Rg16Raw,
}

impl DepthOutputFormat {
    pub fn pipe_flag(self) -> &'static str {
        match self {
            Self::Rg16Raw | Self::Png16Sequence => "rg16",
            Self::U8Mp4 | Self::Hevc10BitMp4 => "u8",
        }
    }
}

/// AI matte/depth extraction task supported by the FrameShift ONNX sidecar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiTaskKind {
    /// Foreground character alpha matte (`MaskMatte` `Alpha`/`Luma`/`Red` compatible).
    #[default]
    RotoscopeMatte,
    /// Monocular depth map (`DepthMatte` compatible).
    DepthMap,
}

impl AiTaskKind {
    pub fn cli_flag(self) -> &'static str {
        match self {
            Self::RotoscopeMatte => "rotoscope",
            Self::DepthMap => "depth",
        }
    }
}

/// Host capability report for optical-flow interpolation and ONNX AI inference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiCapabilitiesReport {
    pub ffmpeg_available: bool,
    pub ffmpeg_path: Option<PathBuf>,
    pub rife_available: bool,
    pub rife_path: Option<PathBuf>,
    pub python_available: bool,
    pub python_path: Option<PathBuf>,
    pub sidecar_script_found: bool,
    pub sidecar_script_path: Option<PathBuf>,
    pub onnxruntime_version: Option<String>,
    pub onnx_providers: Vec<String>,
    pub recommended_interpolator: InterpolatorBackend,
}

/// Configuration for running AI rotoscope/depth inference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiInferenceConfig {
    pub task: AiTaskKind,
    /// Path to an `.onnx` model (e.g. `anime_segmentation.onnx`, `birefnet.onnx`, `depth_anything_v2_small.onnx`).
    pub model_path: Option<PathBuf>,
    /// Model spatial input size fallback when ONNX input shape is dynamic (`518` recommended for Depth Anything V2).
    pub input_size: u32,
    /// Temporal EMA smoothing factor in `0.0..=0.95` to reduce frame-to-frame flicker.
    pub temporal_smooth: f32,
    /// Execution provider preference (`"auto"`, `"dml"`, `"cuda"`, `"cpu"`).
    pub provider: String,
    /// Optional explicit Python interpreter path.
    pub python_binary: Option<PathBuf>,
    /// Optional explicit path to `scripts/ai_infer_sidecar.py`.
    pub sidecar_script: Option<PathBuf>,
    /// Runs deterministic synthetic matte/depth generation without requiring an `.onnx` file.
    pub synthetic_test: bool,
    /// Depth range normalization mode (`Ema`, `PerFrame`, `GlobalPass`).
    #[serde(default)]
    pub depth_norm_mode: DepthNormMode,
    /// Output precision/packing format for depth inference.
    #[serde(default)]
    pub depth_output_format: DepthOutputFormat,
    /// EMA decay for depth `[p1, p99]` normalization in `0.0..=0.99` (default `0.85`).
    #[serde(default = "default_depth_ema_decay")]
    pub depth_ema_decay: f32,
    /// Optional directory where 16-bit grayscale depth PNGs (`depth_XXXXXX.png`) are written.
    #[serde(default)]
    pub png16_dir: Option<PathBuf>,
}

fn default_depth_ema_decay() -> f32 {
    0.85
}

impl Default for AiInferenceConfig {
    fn default() -> Self {
        Self {
            task: AiTaskKind::RotoscopeMatte,
            model_path: None,
            input_size: 512,
            temporal_smooth: 0.25,
            provider: "auto".to_owned(),
            python_binary: None,
            sidecar_script: None,
            synthetic_test: false,
            depth_norm_mode: DepthNormMode::Ema,
            depth_output_format: DepthOutputFormat::U8Mp4,
            depth_ema_decay: 0.85,
            png16_dir: None,
        }
    }
}

/// Summary returned after AI inference over a frame sequence or video file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiInferenceReport {
    pub task: AiTaskKind,
    pub frame_count: usize,
    pub width: u32,
    pub height: u32,
    pub used_synthetic_test: bool,
    pub python_binary: PathBuf,
    pub sidecar_script: PathBuf,
}

#[derive(Debug, Deserialize)]
struct SidecarProbeJson {
    #[serde(default)]
    onnxruntime_available: bool,
    onnxruntime_version: Option<String>,
    #[serde(default)]
    providers: Vec<String>,
}

/// Resolves the sidecar Python script path relative to the workspace or current executable.
pub fn resolve_ai_sidecar_script(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = explicit {
        return if p.is_file() {
            Some(p.to_path_buf())
        } else {
            None
        };
    }

    let relative = Path::new("scripts").join("ai_infer_sidecar.py");
    if relative.is_file() {
        return Some(relative);
    }

    let manifest_candidate = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("scripts")
        .join("ai_infer_sidecar.py");
    if manifest_candidate.is_file() {
        return Some(manifest_candidate);
    }
    None
}

/// Probes local optical-flow interpolation and ONNX AI runtime capabilities.
pub fn detect_ai_capabilities() -> AiCapabilitiesReport {
    let ffmpeg_path = find_executable(None, &["ffmpeg"]);
    let rife_path = find_executable(None, &["rife-ncnn-vulkan"]);
    let python_path = find_executable(None, &["python", "python3"]);
    let sidecar_script_path = resolve_ai_sidecar_script(None);

    let mut onnxruntime_version = None;
    let mut onnx_providers = Vec::new();
    if let (Some(py), Some(script)) = (&python_path, &sidecar_script_path)
        && let Ok(out) = Command::new(py).arg(script).arg("--probe").output()
        && out.status.success()
        && let Ok(parsed) = serde_json::from_slice::<SidecarProbeJson>(&out.stdout)
        && parsed.onnxruntime_available
    {
        onnxruntime_version = parsed.onnxruntime_version;
        onnx_providers = parsed.providers;
    }

    let recommended_interpolator = if rife_path.is_some() {
        InterpolatorBackend::RifeNcnnCli
    } else if ffmpeg_path.is_some() {
        InterpolatorBackend::FfmpegMinterpolate
    } else {
        InterpolatorBackend::FrameBlend
    };

    AiCapabilitiesReport {
        ffmpeg_available: ffmpeg_path.is_some(),
        ffmpeg_path,
        rife_available: rife_path.is_some(),
        rife_path,
        python_available: python_path.is_some(),
        python_path,
        sidecar_script_found: sidecar_script_path.is_some(),
        sidecar_script_path,
        onnxruntime_version,
        onnx_providers,
        recommended_interpolator,
    }
}

/// Runs AI rotoscope matte or depth-map inference on an in-memory frame sequence over a
/// single rawvideo stdin/stdout pipe to `scripts/ai_infer_sidecar.py`.
pub fn run_ai_frame_inference(
    frames: &[Rgba8Frame],
    config: &AiInferenceConfig,
) -> Result<(Vec<Rgba8Frame>, AiInferenceReport), MediaError> {
    let python_exe = find_executable(config.python_binary.as_deref(), &["python", "python3"])
        .ok_or_else(|| MediaError::BinarySpawnFailed {
            binary: "python".into(),
            reason: "python interpreter not found in PATH".into(),
        })?;
    let script_path =
        resolve_ai_sidecar_script(config.sidecar_script.as_deref()).ok_or_else(|| {
            MediaError::IoError {
                message: "could not locate scripts/ai_infer_sidecar.py".into(),
            }
        })?;

    if !config.synthetic_test && config.model_path.is_none() {
        return Err(MediaError::InvalidEncodingSettings(
            "AI inference requires model_path unless synthetic_test is enabled".into(),
        ));
    }
    if let Some(model) = &config.model_path
        && !config.synthetic_test
        && !model.is_file()
    {
        return Err(MediaError::IoError {
            message: format!("ONNX model file '{}' not found", model.display()),
        });
    }

    if frames.is_empty() {
        return Ok((
            Vec::new(),
            AiInferenceReport {
                task: config.task,
                frame_count: 0,
                width: 0,
                height: 0,
                used_synthetic_test: config.synthetic_test,
                python_binary: python_exe,
                sidecar_script: script_path,
            },
        ));
    }

    let w = frames[0].width;
    let h = frames[0].height;
    if w == 0 || h == 0 {
        return Err(MediaError::InvalidDimensions {
            width: w,
            height: h,
        });
    }
    for f in frames {
        if f.width != w || f.height != h {
            return Err(MediaError::InvalidDimensions {
                width: f.width,
                height: f.height,
            });
        }
    }

    let mut cmd = Command::new(&python_exe);
    cmd.arg(&script_path)
        .args(["--task", config.task.cli_flag()])
        .args(["--width", &w.to_string()])
        .args(["--height", &h.to_string()])
        .args(["--frames", &frames.len().to_string()])
        .args(["--input-size", &config.input_size.max(32).to_string()])
        .args([
            "--temporal-smooth",
            &format!("{}", config.temporal_smooth.clamp(0.0, 0.95)),
        ])
        .args(["--norm-mode", config.depth_norm_mode.cli_flag()])
        .args([
            "--ema-decay",
            &format!("{}", config.depth_ema_decay.clamp(0.0, 0.99)),
        ])
        .args(["--depth-format", config.depth_output_format.pipe_flag()])
        .args(["--provider", &config.provider]);

    if let Some(png16_dir) = &config.png16_dir {
        cmd.arg("--png16-dir").arg(png16_dir);
    }
    if let Some(model) = &config.model_path {
        cmd.arg("--model").arg(model);
    }
    if config.synthetic_test {
        cmd.arg("--synthetic-test");
    }

    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| MediaError::BinarySpawnFailed {
        binary: python_exe.to_string_lossy().into_owned(),
        reason: e.to_string(),
    })?;

    let mut stdin = child.stdin.take().ok_or_else(|| MediaError::FfmpegFailed {
        exit_code: None,
        stderr: "failed to acquire python sidecar stdin pipe".into(),
    })?;
    let mut stdout = child.stdout.take().ok_or_else(|| MediaError::FfmpegFailed {
        exit_code: None,
        stderr: "failed to acquire python sidecar stdout pipe".into(),
    })?;
    let stderr_drain = crate::streaming::spawn_stderr_drain(child.stderr.take());

    let payload: Vec<u8> = frames.iter().flat_map(|f| f.data.iter().copied()).collect();
    let writer_handle = std::thread::spawn(move || -> Result<(), std::io::Error> {
        stdin.write_all(&payload)?;
        stdin.flush()?;
        drop(stdin);
        Ok(())
    });

    let frame_bytes = (w as usize) * (h as usize) * 4;
    let expected_total = frame_bytes * frames.len();
    let mut raw_out = Vec::with_capacity(expected_total);
    let read_res = stdout.read_to_end(&mut raw_out);
    let write_res = writer_handle.join().map_err(|_| MediaError::IoError {
        message: "AI sidecar writer thread panicked".into(),
    })?;
    let status = child.wait().map_err(|e| MediaError::IoError {
        message: format!("AI sidecar wait failed: {e}"),
    })?;
    let stderr_text = stderr_drain
        .and_then(|h| h.join().ok())
        .unwrap_or_default();

    if let Err(e) = write_res {
        return Err(MediaError::FfmpegFailed {
            exit_code: status.code(),
            stderr: format!("AI sidecar stdin write failed ({e}): {stderr_text}"),
        });
    }
    if let Err(e) = read_res {
        return Err(MediaError::FfmpegFailed {
            exit_code: status.code(),
            stderr: format!("AI sidecar stdout read failed ({e}): {stderr_text}"),
        });
    }
    if !status.success() {
        return Err(MediaError::FfmpegFailed {
            exit_code: status.code(),
            stderr: stderr_text,
        });
    }
    if raw_out.len() != expected_total {
        return Err(MediaError::BufferLengthMismatch {
            expected: expected_total,
            actual: raw_out.len(),
        });
    }

    let mut out_frames = Vec::with_capacity(frames.len());
    for (idx, src) in frames.iter().enumerate() {
        let start = idx * frame_bytes;
        let end = start + frame_bytes;
        out_frames.push(Rgba8Frame::new(
            w,
            h,
            src.pts,
            AlphaMode::Straight,
            raw_out[start..end].to_vec(),
        )?);
    }

    Ok((
        out_frames,
        AiInferenceReport {
            task: config.task,
            frame_count: frames.len(),
            width: w,
            height: h,
            used_synthetic_test: config.synthetic_test,
            python_binary: python_exe,
            sidecar_script: script_path,
        },
    ))
}

/// Runs AI rotoscope/depth inference on an on-disk video file and encodes the resulting
/// matte/depth stream into a lossless/high-bitrate MP4 compatible with `MaskMatte` and `DepthMatte`.
pub fn run_ai_video_inference(
    input: &Path,
    output: &Path,
    config: &AiInferenceConfig,
    encode_profile: &ExportEncodingProfile,
) -> Result<AiInferenceReport, MediaError> {
    let probe = FfmpegBridge::probe_media_info(input)?;
    let frame_count = probe.pts_index.len();
    if frame_count == 0 {
        return Err(MediaError::EmptyMediaStream(format!(
            "input video '{}' has zero frames",
            input.display()
        )));
    }
    let decoded = FfmpegBridge::decode_video_window(
        input,
        &probe.pts_index,
        0,
        frame_count,
        probe.width,
        probe.height,
    )?;
    let in_frames: Vec<Rgba8Frame> = decoded.into_iter().map(|(_, f)| f).collect();
    let (out_frames, report) = run_ai_frame_inference(&in_frames, config)?;

    if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let fps = probe.fps.unwrap_or(frameshift_time::FPS_24);
    let mut encoder = Mp4FrameEncoder::new_with_profile(
        output,
        probe.width,
        probe.height,
        fps,
        None,
        encode_profile,
    )?;
    for frame in &out_frames {
        encoder.write_frame(frame)?;
    }
    encoder.finish()?;
    Ok(report)
}

/// Runs monocular depth inference in 16-bit `RG16` pipe precision and decodes the output
/// directly into linear float32 `Rgba16fFrame`s (`[d, d, d, 1.0]`, 65,536 discrete depth levels).
pub fn run_ai_depth_f32_inference(
    frames: &[Rgba8Frame],
    config: &AiInferenceConfig,
) -> Result<(Vec<Rgba16fFrame>, AiInferenceReport), MediaError> {
    let mut cfg = config.clone();
    cfg.task = AiTaskKind::DepthMap;
    cfg.depth_output_format = DepthOutputFormat::Rg16Raw;
    if cfg.input_size == 512 {
        cfg.input_size = 518;
    }
    let (packed, report) = run_ai_frame_inference(frames, &cfg)?;
    let mut out = Vec::with_capacity(packed.len());
    for f in &packed {
        out.push(Rgba16fFrame::from_rg16_packed(f)?);
    }
    Ok((out, report))
}

/// Runs monocular depth inference and writes a 16-bit grayscale PNG sequence (`depth_XXXXXX.png`)
/// into `out_dir`, returning the ordered PNG file paths.
pub fn run_ai_depth_png16_sequence(
    frames: &[Rgba8Frame],
    out_dir: &Path,
    config: &AiInferenceConfig,
) -> Result<(Vec<PathBuf>, AiInferenceReport), MediaError> {
    std::fs::create_dir_all(out_dir)?;
    let mut cfg = config.clone();
    cfg.task = AiTaskKind::DepthMap;
    cfg.depth_output_format = DepthOutputFormat::Png16Sequence;
    cfg.png16_dir = Some(out_dir.to_path_buf());
    if cfg.input_size == 512 {
        cfg.input_size = 518;
    }
    let (_, report) = run_ai_frame_inference(frames, &cfg)?;
    let paths = (0..frames.len())
        .map(|i| out_dir.join(format!("depth_{i:06}.png")))
        .collect();
    Ok((paths, report))
}

/// Builds a deterministic test frame sequence for AI sidecar verification.
pub fn synthetic_ai_test_frames(width: u32, height: u32, count: usize) -> Vec<Rgba8Frame> {
    (0..count)
        .map(|i| {
            let mut data = vec![0u8; (width * height * 4) as usize];
            for y in 0..height {
                for x in 0..width {
                    let idx = ((y * width + x) * 4) as usize;
                    let fg = x > width / 4 && x < (3 * width) / 4 && y > height / 4;
                    if fg {
                        data[idx..idx + 4].copy_from_slice(&[240, (60 + i * 20) as u8, 40, 255]);
                    } else {
                        data[idx..idx + 4].copy_from_slice(&[15, 20, 25, 255]);
                    }
                }
            }
            Rgba8Frame::new(
                width,
                height,
                Flicks(i as i64 * 29_400_000),
                AlphaMode::Straight,
                data,
            )
            .unwrap()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_ai_capabilities_finds_local_ffmpeg_python_and_onnxruntime() {
        let report = detect_ai_capabilities();
        assert!(report.sidecar_script_found);
        assert!(report.python_available);
        assert!(report.onnxruntime_version.is_some());
        assert!(!report.onnx_providers.is_empty());
    }

    #[test]
    fn ai_sidecar_raw_pipe_roundtrip_works_for_rotoscope_and_depth() {
        let frames = synthetic_ai_test_frames(64, 36, 3);
        for task in [AiTaskKind::RotoscopeMatte, AiTaskKind::DepthMap] {
            let cfg = AiInferenceConfig {
                task,
                synthetic_test: true,
                temporal_smooth: 0.2,
                ..Default::default()
            };
            let (out, report) = run_ai_frame_inference(&frames, &cfg).unwrap();
            assert_eq!(report.frame_count, 3);
            assert_eq!(out.len(), 3);
            assert_eq!(out[0].width, 64);
            assert_eq!(out[0].height, 36);
            if task == AiTaskKind::DepthMap {
                assert_eq!(out[0].get_pixel(0, 0).unwrap()[3], 255);
            } else {
                let fg_alpha = out[0].get_pixel(32, 20).unwrap()[3];
                let bg_alpha = out[0].get_pixel(2, 2).unwrap()[3];
                assert!(fg_alpha > bg_alpha, "expected fg {fg_alpha} > bg {bg_alpha}");
            }
        }
    }

    #[test]
    fn ai_depth_rg16_and_png16_preserve_16bit_precision() {
        let frames = synthetic_ai_test_frames(64, 36, 2);
        let cfg = AiInferenceConfig {
            task: AiTaskKind::DepthMap,
            synthetic_test: true,
            depth_norm_mode: DepthNormMode::Ema,
            ..Default::default()
        };
        let (f32_frames, report) = run_ai_depth_f32_inference(&frames, &cfg).unwrap();
        assert_eq!(report.frame_count, 2);
        assert_eq!(f32_frames.len(), 2);
        let top_d = f32_frames[0].data[0][0];
        let bot_d = f32_frames[0].data[(35 * 64) + 32][0];
        assert!(
            bot_d > top_d,
            "expected bottom depth {bot_d} > top depth {top_d}"
        );

        let unique_dir = std::env::temp_dir().join(format!(
            "frameshift_png16_test_{}",
            std::process::id()
        ));
        let (paths, _) = run_ai_depth_png16_sequence(&frames, &unique_dir, &cfg).unwrap();
        assert_eq!(paths.len(), 2);
        let loaded = Rgba16fFrame::load_png_linear_f32(&paths[0], Flicks::ZERO).unwrap();
        assert_eq!(loaded.width, 64);
        assert_eq!(loaded.height, 36);
        assert!((loaded.data[0][0] - f32_frames[0].data[0][0]).abs() < 1e-4);
        assert!((loaded.data[(35 * 64) + 32][0] - bot_d).abs() < 1e-4);
        let _ = std::fs::remove_dir_all(&unique_dir);
    }

    #[test]
    fn live_depth_anything_v2_small_onnx_runs_aspect_preserving_rg16() {
        let model_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("local_media")
            .join("models")
            .join("depth_anything_v2_small.onnx");
        if !model_path.is_file() {
            return;
        }
        let frames = synthetic_ai_test_frames(160, 90, 2);
        let cfg = AiInferenceConfig {
            task: AiTaskKind::DepthMap,
            model_path: Some(model_path),
            input_size: 518,
            temporal_smooth: 0.25,
            depth_norm_mode: DepthNormMode::Ema,
            ..Default::default()
        };
        let (f32_frames, report) = run_ai_depth_f32_inference(&frames, &cfg).unwrap();
        assert_eq!(report.frame_count, 2);
        assert_eq!(f32_frames[0].width, 160);
        assert_eq!(f32_frames[0].height, 90);
        let mut distinct = std::collections::BTreeSet::new();
        for px in &f32_frames[0].data {
            distinct.insert((px[0] * 65535.0).round() as u32);
        }
        assert!(
            distinct.len() > 256,
            "expected >256 16-bit depth levels from live Depth-Anything-V2-Small, got {}",
            distinct.len()
        );
    }
}
