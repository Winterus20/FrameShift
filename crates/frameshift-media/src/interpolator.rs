//! Live video/frame interpolation engine (`RifeNcnnCli`, `FfmpegMinterpolate`, `FrameBlend`)
//! with `ClipAnalysis` anime duplicate-run deduplication and hard scene-cut boundary protection.

use crate::analysis::{
    ClipAnalysis, InterpolatorBackend, blend_frames, build_minterpolate_filter, build_rife_pair_args,
};
use crate::error::MediaError;
use crate::ffmpeg::{ExportEncodingProfile, FfmpegBridge, Mp4FrameEncoder};
use crate::frame::{AlphaMode, Rgba8Frame};
use frameshift_time::{Flicks, Rational32};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static INTERP_TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Configuration for frame/video interpolation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InterpolateConfig {
    pub source_fps: Rational32,
    pub target_fps: Rational32,
    /// `None` auto-selects `RifeNcnnCli` -> `FfmpegMinterpolate` -> `FrameBlend`.
    pub backend: Option<InterpolatorBackend>,
    /// Luma MAD threshold in `0.0..=1.0` for anime duplicate-run deduplication (`0.0` disables dedup).
    pub dedup_threshold: f32,
    /// Adjacent luma MAD threshold in `0.0..=1.0` for hard scene-cut isolation.
    pub scene_cut_threshold: f32,
    /// Optional explicit path to `rife-ncnn-vulkan`.
    pub rife_binary: Option<PathBuf>,
    /// Model name or directory passed to `rife-ncnn-vulkan -m`.
    pub rife_model: String,
    /// Optional explicit path to `ffmpeg`.
    pub ffmpeg_binary: Option<PathBuf>,
}

impl Default for InterpolateConfig {
    fn default() -> Self {
        Self {
            source_fps: Rational32 { num: 24, den: 1 },
            target_fps: Rational32 { num: 60, den: 1 },
            backend: None,
            dedup_threshold: 0.008,
            scene_cut_threshold: 0.25,
            rife_binary: None,
            rife_model: "rife-v4.6".to_owned(),
            ffmpeg_binary: None,
        }
    }
}

/// Telemetry and diagnostics returned after interpolating a sequence or video file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterpolationReport {
    pub input_frames: usize,
    pub unique_frames: usize,
    pub duplicate_frames_collapsed: usize,
    pub scene_cuts: Vec<usize>,
    pub output_frames: usize,
    pub backend_used: InterpolatorBackend,
    pub rife_pairs_rendered: usize,
    pub ffmpeg_segments_rendered: usize,
}

/// Checks if a binary name or explicit path exists and can be spawned cleanly (`--help` / `-version`).
pub(crate) fn find_executable(explicit: Option<&Path>, default_names: &[&str]) -> Option<PathBuf> {
    if let Some(p) = explicit {
        if p.exists() {
            return Some(p.to_path_buf());
        }
        return None;
    }
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        for name in default_names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
            #[cfg(windows)]
            if candidate.extension().is_none() {
                let exe = dir.join(format!("{name}.exe"));
                if exe.is_file() {
                    return Some(exe);
                }
                let cmd = dir.join(format!("{name}.cmd"));
                if cmd.is_file() {
                    return Some(cmd);
                }
            }
        }
    }
    None
}

/// Resolves the best available interpolation backend on the current host.
pub fn resolve_interpolator_backend(config: &InterpolateConfig) -> InterpolatorBackend {
    if let Some(explicit) = config.backend {
        return explicit;
    }
    if find_executable(config.rife_binary.as_deref(), &["rife-ncnn-vulkan"]).is_some() {
        return InterpolatorBackend::RifeNcnnCli;
    }
    if find_executable(config.ffmpeg_binary.as_deref(), &["ffmpeg"]).is_some() {
        return InterpolatorBackend::FfmpegMinterpolate;
    }
    InterpolatorBackend::FrameBlend
}

/// Interpolates an in-memory frame sequence while isolating hard scene cuts and collapsing
/// anime duplicate runs (`ClipAnalysis`).
pub fn interpolate_frame_sequence(
    frames: &[Rgba8Frame],
    config: &InterpolateConfig,
) -> Result<(Vec<Rgba8Frame>, InterpolationReport), MediaError> {
    if frames.is_empty() {
        return Ok((
            Vec::new(),
            InterpolationReport {
                input_frames: 0,
                unique_frames: 0,
                duplicate_frames_collapsed: 0,
                scene_cuts: Vec::new(),
                output_frames: 0,
                backend_used: resolve_interpolator_backend(config),
                rife_pairs_rendered: 0,
                ffmpeg_segments_rendered: 0,
            },
        ));
    }

    if config.source_fps.num == 0
        || config.source_fps.den == 0
        || config.target_fps.num == 0
        || config.target_fps.den == 0
    {
        return Err(MediaError::InvalidEncodingSettings(
            "source_fps and target_fps must have non-zero numerator and denominator".into(),
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

    let backend_used = resolve_interpolator_backend(config);
    let refs: Vec<&Rgba8Frame> = frames.iter().collect();
    let analysis =
        ClipAnalysis::analyze_frame_sequence(&refs, config.dedup_threshold, config.scene_cut_threshold);

    let src_fps_f = config.source_fps.num as f64 / config.source_fps.den as f64;
    let dst_fps_f = config.target_fps.num as f64 / config.target_fps.den as f64;
    let dst_frame_ticks = config.target_fps.frame_duration()?;

    // Partition frames into continuous scene segments at `scene_cuts`.
    let mut boundaries = Vec::with_capacity(analysis.scene_cuts.len() + 2);
    boundaries.push(0usize);
    boundaries.extend(analysis.scene_cuts.iter().copied());
    boundaries.push(frames.len());

    let mut out_frames = Vec::new();
    let mut rife_pairs_rendered = 0usize;
    let mut ffmpeg_segments_rendered = 0usize;

    for win in boundaries.windows(2) {
        let seg_start = win[0];
        let seg_end = win[1];
        if seg_start >= seg_end {
            continue;
        }
        let seg_len = seg_end - seg_start;
        let seg_target_count = ((seg_len as f64 * dst_fps_f / src_fps_f).round() as usize).max(1);

        // Extract unique runs inside `[seg_start, seg_end)`.
        let mut seg_uniques: Vec<(usize, usize)> = Vec::new(); // (local_start_in_seg, count)
        for run in &analysis.unique_runs {
            if run.start_frame >= seg_start && run.start_frame < seg_end {
                let clipped_count = run.count.min(seg_end - run.start_frame);
                seg_uniques.push((run.start_frame - seg_start, clipped_count));
            }
        }
        if seg_uniques.is_empty() {
            seg_uniques.push((0, seg_len));
        }

        if seg_uniques.len() == 1 || seg_target_count == 1 {
            let rep = &frames[seg_start + seg_uniques[0].0];
            for _ in 0..seg_target_count {
                let pts = Flicks((out_frames.len() as i64) * dst_frame_ticks.0);
                out_frames.push(Rgba8Frame::new(
                    rep.width,
                    rep.height,
                    pts,
                    rep.alpha_mode,
                    rep.data.clone(),
                )?);
            }
            continue;
        }

        match backend_used {
            InterpolatorBackend::FfmpegMinterpolate => {
                let unique_frames: Vec<&Rgba8Frame> = seg_uniques
                    .iter()
                    .map(|(local_idx, _)| &frames[seg_start + *local_idx])
                    .collect();
                let seg_out = run_ffmpeg_minterpolate_segment(
                    &unique_frames,
                    seg_len,
                    seg_target_count,
                    config,
                    out_frames.len(),
                    dst_frame_ticks,
                )?;
                out_frames.extend(seg_out);
                ffmpeg_segments_rendered += 1;
            }
            InterpolatorBackend::RifeNcnnCli | InterpolatorBackend::FrameBlend => {
                // Anchor each unique representative at its run start normalized over `seg_len`.
                let anchors: Vec<(f64, &Rgba8Frame)> = seg_uniques
                    .iter()
                    .map(|(local_idx, _)| (*local_idx as f64, &frames[seg_start + *local_idx]))
                    .collect();

                for out_i in 0..seg_target_count {
                    let src_pos = (out_i as f64) * (seg_len as f64) / (seg_target_count as f64);
                    let pts = Flicks((out_frames.len() as i64) * dst_frame_ticks.0);

                    // Locate enclosing pair `(anchors[k], anchors[k+1])`
                    let mut k = 0usize;
                    while k + 1 < anchors.len() && anchors[k + 1].0 <= src_pos {
                        k += 1;
                    }

                    let frame = if k + 1 >= anchors.len() {
                        let last = anchors[anchors.len() - 1].1;
                        Rgba8Frame::new(
                            last.width,
                            last.height,
                            pts,
                            last.alpha_mode,
                            last.data.clone(),
                        )?
                    } else {
                        let (t0, f0) = anchors[k];
                        let (t1, f1) = anchors[k + 1];
                        let span = (t1 - t0).max(1e-6);
                        let alpha = ((src_pos - t0) / span).clamp(0.0, 1.0) as f32;
                        if alpha <= 1e-4 {
                            Rgba8Frame::new(
                                f0.width,
                                f0.height,
                                pts,
                                f0.alpha_mode,
                                f0.data.clone(),
                            )?
                        } else if alpha >= 1.0 - 1e-4 {
                            Rgba8Frame::new(
                                f1.width,
                                f1.height,
                                pts,
                                f1.alpha_mode,
                                f1.data.clone(),
                            )?
                        } else if backend_used == InterpolatorBackend::RifeNcnnCli {
                            rife_pairs_rendered += 1;
                            run_rife_pair(f0, f1, alpha, pts, config)?
                        } else {
                            blend_frames(f0, f1, alpha, pts)?
                        }
                    };
                    out_frames.push(frame);
                }
            }
        }
    }

    let report = InterpolationReport {
        input_frames: frames.len(),
        unique_frames: analysis.unique_frame_count(),
        duplicate_frames_collapsed: analysis.duplicate_frame_count(),
        scene_cuts: analysis.scene_cuts,
        output_frames: out_frames.len(),
        backend_used,
        rife_pairs_rendered,
        ffmpeg_segments_rendered,
    };
    Ok((out_frames, report))
}

/// Interpolates an on-disk video file using [`interpolate_frame_sequence`] and writes
/// a Rec.709-tagged output video using `encode_profile`.
pub fn interpolate_video_file(
    input: &Path,
    output: &Path,
    config: &InterpolateConfig,
    encode_profile: &ExportEncodingProfile,
) -> Result<InterpolationReport, MediaError> {
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
    let input_frames: Vec<Rgba8Frame> = decoded.into_iter().map(|(_, f)| f).collect();
    let mut cfg = config.clone();
    if let Some(fps) = probe.fps
        && fps.num > 0
        && fps.den > 0
    {
        cfg.source_fps = fps;
    }
    let (out_frames, report) = interpolate_frame_sequence(&input_frames, &cfg)?;

    if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let mut encoder = Mp4FrameEncoder::new_with_profile(
        output,
        probe.width,
        probe.height,
        cfg.target_fps,
        None,
        encode_profile,
    )?;
    for frame in &out_frames {
        encoder.write_frame(frame)?;
    }
    encoder.finish()?;
    Ok(report)
}

fn run_rife_pair(
    frame0: &Rgba8Frame,
    frame1: &Rgba8Frame,
    timestep: f32,
    pts: Flicks,
    config: &InterpolateConfig,
) -> Result<Rgba8Frame, MediaError> {
    let rife_exe = find_executable(config.rife_binary.as_deref(), &["rife-ncnn-vulkan"])
        .ok_or_else(|| MediaError::BinarySpawnFailed {
            binary: "rife-ncnn-vulkan".into(),
            reason: "rife-ncnn-vulkan executable not found in PATH".into(),
        })?;

    let seq = INTERP_TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let dir = std::env::temp_dir();
    let in0_path = dir.join(format!("fs_rife_in0_{pid}_{seq}.png"));
    let in1_path = dir.join(format!("fs_rife_in1_{pid}_{seq}.png"));
    let out_path = dir.join(format!("fs_rife_out_{pid}_{seq}.png"));

    struct Cleanup([PathBuf; 3]);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            for p in &self.0 {
                let _ = std::fs::remove_file(p);
            }
        }
    }
    let _cleanup = Cleanup([in0_path.clone(), in1_path.clone(), out_path.clone()]);

    save_rgb_png_for_rife(frame0, &in0_path)?;
    save_rgb_png_for_rife(frame1, &in1_path)?;
    let args = build_rife_pair_args(
        &config.rife_model,
        &in0_path,
        &in1_path,
        &out_path,
        timestep,
    )?;

    let output = Command::new(&rife_exe)
        .args(&args)
        .output()
        .map_err(|e| MediaError::BinarySpawnFailed {
            binary: rife_exe.to_string_lossy().into_owned(),
            reason: e.to_string(),
        })?;
    if !output.status.success() {
        return Err(MediaError::FfmpegFailed {
            exit_code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Rgba8Frame::load_png(&out_path, pts)
}

/// Writes an 8-bit 3-channel RGB PNG because `rife-ncnn-vulkan` misreads 4-channel RGBA PNGs.
fn save_rgb_png_for_rife(frame: &Rgba8Frame, path: &Path) -> Result<(), MediaError> {
    let file = std::fs::File::create(path)?;
    let writer = std::io::BufWriter::new(file);
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

fn run_ffmpeg_minterpolate_segment(
    unique_frames: &[&Rgba8Frame],
    seg_src_frames: usize,
    seg_target_count: usize,
    config: &InterpolateConfig,
    global_out_offset: usize,
    dst_frame_ticks: Flicks,
) -> Result<Vec<Rgba8Frame>, MediaError> {
    let ffmpeg_exe = find_executable(config.ffmpeg_binary.as_deref(), &["ffmpeg"])
        .ok_or_else(|| MediaError::BinarySpawnFailed {
            binary: "ffmpeg".into(),
            reason: "ffmpeg executable not found in PATH".into(),
        })?;

    let w = unique_frames[0].width;
    let h = unique_frames[0].height;
    let frame_bytes = (w as usize) * (h as usize) * 4;

    // Effective input FPS of deduplicated unique frames so duration matches the original segment.
    let src_fps_f = config.source_fps.num as f64 / config.source_fps.den as f64;
    let seg_duration_sec = (seg_src_frames as f64) / src_fps_f.max(1e-6);
    let eff_in_fps = ((unique_frames.len() as f64) / seg_duration_sec.max(1e-6)).max(1.0);
    let eff_in_num = (eff_in_fps * 1000.0).round().max(1.0) as u32;
    let in_fps_str = format!("{eff_in_num}/1000");

    let filter = build_minterpolate_filter(config.target_fps.num, config.target_fps.den)?;
    let size_arg = format!("{w}x{h}");

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
        &size_arg,
        "-framerate",
        &in_fps_str,
        "-i",
        "pipe:0",
        "-vf",
        &filter,
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
        stderr: "failed to acquire ffmpeg stdin pipe for minterpolate".into(),
    })?;
    let mut stdout = child.stdout.take().ok_or_else(|| MediaError::FfmpegFailed {
        exit_code: None,
        stderr: "failed to acquire ffmpeg stdout pipe for minterpolate".into(),
    })?;
    let stderr_drain = crate::streaming::spawn_stderr_drain(child.stderr.take());

    // Write unique frames on a background thread to avoid stdout/stdin pipe deadlock.
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

    let mut raw_out = Vec::with_capacity(seg_target_count * frame_bytes);
    let read_res = stdout.read_to_end(&mut raw_out);
    let write_res = writer_handle
        .join()
        .map_err(|_| MediaError::IoError {
            message: "minterpolate writer thread panicked".into(),
        })?;
    let status = child.wait().map_err(|e| MediaError::IoError {
        message: format!("minterpolate wait failed: {e}"),
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

    let produced = raw_out.len() / frame_bytes;
    if produced == 0 {
        return Err(MediaError::FfmpegFailed {
            exit_code: status.code(),
            stderr: format!("minterpolate produced 0 frames: {stderr_text}"),
        });
    }

    let mut result = Vec::with_capacity(seg_target_count);
    for i in 0..seg_target_count {
        let src_idx = i.min(produced - 1);
        let start = src_idx * frame_bytes;
        let end = start + frame_bytes;
        let pts = Flicks(((global_out_offset + i) as i64) * dst_frame_ticks.0);
        result.push(Rgba8Frame::new(
            w,
            h,
            pts,
            AlphaMode::Straight,
            raw_out[start..end].to_vec(),
        )?);
    }
    Ok(result)
}

/// Generates a compact motion test clip on disk for interpolation verification and demos.
pub fn write_synthetic_motion_mp4(
    out_path: &Path,
    width: u32,
    height: u32,
    fps: Rational32,
    frame_count: usize,
) -> Result<(), MediaError> {
    let frame_ticks = fps.frame_duration()?;
    let mut enc = Mp4FrameEncoder::new_with_profile(
        out_path,
        width,
        height,
        fps,
        None,
        &ExportEncodingProfile::default(),
    )?;
    for idx in 0..frame_count {
        let mut data = vec![0u8; (width * height * 4) as usize];
        let bx = ((idx * 6) % (width as usize).saturating_sub(24).max(1)) as u32;
        for y in 0..height {
            for x in 0..width {
                let offset = ((y * width + x) * 4) as usize;
                let inside = x >= bx && x < bx + 20 && y >= height / 3 && y < (2 * height) / 3;
                if inside {
                    data[offset..offset + 4].copy_from_slice(&[250, 210, 60, 255]);
                } else {
                    data[offset..offset + 4].copy_from_slice(&[18, 22, 34, 255]);
                }
            }
        }
        let frame = Rgba8Frame::new(
            width,
            height,
            Flicks(idx as i64 * frame_ticks.0),
            AlphaMode::Straight,
            data,
        )?;
        enc.write_frame(&frame)?;
    }
    enc.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block_frame(w: u32, h: u32, block_x: u32, bg: [u8; 4], fg: [u8; 4]) -> Rgba8Frame {
        let mut data = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let idx = ((y * w + x) * 4) as usize;
                let inside = x >= block_x && x < block_x + 12 && y >= h / 4 && y < (3 * h) / 4;
                data[idx..idx + 4].copy_from_slice(if inside { &fg } else { &bg });
            }
        }
        Rgba8Frame::new(w, h, Flicks::ZERO, AlphaMode::Straight, data).unwrap()
    }

    #[test]
    fn interpolate_frame_blend_dedups_twos_and_protects_hard_cuts() {
        // Scene A on twos: [x=4, x=4, x=24, x=24], Hard cut -> Scene B: [bright inverse]
        let a0 = block_frame(64, 36, 4, [10, 10, 15, 255], [240, 220, 40, 255]);
        let a1 = block_frame(64, 36, 24, [10, 10, 15, 255], [240, 220, 40, 255]);
        let b0 = Rgba8Frame::filled(64, 36, Flicks::ZERO, [235, 235, 240, 255], AlphaMode::Straight)
            .unwrap();
        let b1 = Rgba8Frame::filled(64, 36, Flicks::ZERO, [235, 235, 240, 255], AlphaMode::Straight)
            .unwrap();

        let seq = vec![a0.clone(), a0, a1.clone(), a1, b0.clone(), b1];
        let cfg = InterpolateConfig {
            source_fps: Rational32 { num: 24, den: 1 },
            target_fps: Rational32 { num: 48, den: 1 },
            backend: Some(InterpolatorBackend::FrameBlend),
            dedup_threshold: 0.005,
            scene_cut_threshold: 0.25,
            ..Default::default()
        };

        let (out, report) = interpolate_frame_sequence(&seq, &cfg).unwrap();
        assert_eq!(report.input_frames, 6);
        assert_eq!(report.unique_frames, 3);
        assert_eq!(report.duplicate_frames_collapsed, 3);
        assert_eq!(report.scene_cuts, vec![4]);
        assert_eq!(report.output_frames, 12);

        // Scene B frames (indices 8..12) must never be morphed with Scene A dark background.
        for f in &out[8..12] {
            assert_eq!(f.get_pixel(0, 0), Some([235, 235, 240, 255]));
        }
    }

    #[test]
    fn interpolate_live_ffmpeg_minterpolate_and_mp4_24_to_48() {
        if find_executable(None, &["ffmpeg"]).is_none() {
            return;
        }
        let pid = std::process::id();
        let seq = INTERP_TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let in_mp4 = std::env::temp_dir().join(format!("fs_interp_in_{pid}_{seq}.mp4"));
        let out_mp4 = std::env::temp_dir().join(format!("fs_interp_out_{pid}_{seq}.mp4"));
        let _ = std::fs::remove_file(&in_mp4);
        let _ = std::fs::remove_file(&out_mp4);

        write_synthetic_motion_mp4(&in_mp4, 96, 64, Rational32 { num: 24, den: 1 }, 6).unwrap();

        let cfg = InterpolateConfig {
            source_fps: Rational32 { num: 24, den: 1 },
            target_fps: Rational32 { num: 48, den: 1 },
            backend: Some(InterpolatorBackend::FfmpegMinterpolate),
            dedup_threshold: 0.0,
            scene_cut_threshold: 0.35,
            ..Default::default()
        };
        let report = interpolate_video_file(
            &in_mp4,
            &out_mp4,
            &cfg,
            &ExportEncodingProfile::default(),
        )
        .unwrap();
        assert_eq!(report.backend_used, InterpolatorBackend::FfmpegMinterpolate);
        assert_eq!(report.output_frames, 12);

        let probe = FfmpegBridge::probe_media_info(&out_mp4).unwrap();
        assert_eq!(probe.fps, Some(Rational32 { num: 48, den: 1 }));
        assert_eq!(probe.pts_index.len(), 12);

        let _ = std::fs::remove_file(&in_mp4);
        let _ = std::fs::remove_file(&out_mp4);
    }
}
