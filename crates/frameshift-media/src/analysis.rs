//! Pure-Rust video clip analysis (160x90 luma subsampled anime duplicate detection
//! and hard scene-cut detection) and frame interpolation backend helpers.

use crate::error::MediaError;
use crate::frame::{AlphaMode, Rgba8Frame};
use frameshift_time::{Flicks, TimeError};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Subsampled horizontal resolution used by [`ClipAnalysis`] luma MAD metrics.
pub const ANALYSIS_WIDTH: usize = 160;
/// Subsampled vertical resolution used by [`ClipAnalysis`] luma MAD metrics.
pub const ANALYSIS_HEIGHT: usize = 90;

const LUMA_SAMPLE_COUNT: usize = ANALYSIS_WIDTH * ANALYSIS_HEIGHT;

/// A contiguous run of identical or near-identical frames starting at `start_frame`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DuplicateRun {
    /// Zero-based index of the first (representative) frame in the run.
    pub start_frame: usize,
    /// Number of consecutive frames covered by this run (`>= 1`).
    pub count: usize,
}

/// Summary of duplicate frame runs (e.g. anime 2s/3s holds) and hard scene cuts.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ClipAnalysisReport {
    /// Total number of frames analyzed.
    pub total_frames: usize,
    /// Partition of `[0, total_frames)` into consecutive duplicate runs.
    pub unique_runs: Vec<DuplicateRun>,
    /// Zero-based indices of frames where a hard scene cut occurs relative to the previous frame.
    pub scene_cuts: Vec<usize>,
}

impl ClipAnalysisReport {
    /// Number of unique representative frames (`unique_runs.len()`).
    pub fn unique_frame_count(&self) -> usize {
        self.unique_runs.len()
    }

    /// Number of redundant duplicate frames (`total_frames - unique_frame_count()`).
    pub fn duplicate_frame_count(&self) -> usize {
        self.total_frames.saturating_sub(self.unique_runs.len())
    }
}

/// Frame interpolation backend selection for slow-motion / high-FPS retiming.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum InterpolatorBackend {
    /// External `rife-ncnn-vulkan` CLI pairwise optical-flow neural interpolation.
    RifeNcnnCli,
    /// FFmpeg `minterpolate` motion-compensated filtergraph backend.
    FfmpegMinterpolate,
    /// Pure-Rust linear RGBA frame blending fallback.
    #[default]
    FrameBlend,
}

/// Pure-Rust clip sequence analyzer for anime duplicate runs and hard scene cuts.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClipAnalysis;

impl ClipAnalysis {
    /// Analyzes a slice of frame references using `160x90` Rec.709 luma subsampling and MAD.
    ///
    /// - Consecutive frames whose luma Mean Absolute Difference (MAD in `0.0..=1.0`) against the
    ///   current run's representative frame is `<= dedup_threshold` (and not a scene cut) are
    ///   grouped into the same [`DuplicateRun`].
    /// - Transitions where adjacent frame luma MAD is `>= scene_cut_threshold` are recorded in
    ///   [`ClipAnalysisReport::scene_cuts`] and always start a new run.
    pub fn analyze_frame_sequence(
        frames: &[&Rgba8Frame],
        dedup_threshold: f32,
        scene_cut_threshold: f32,
    ) -> ClipAnalysisReport {
        if frames.is_empty() {
            return ClipAnalysisReport::default();
        }

        let dedup_thr = if dedup_threshold.is_finite() {
            dedup_threshold.max(0.0)
        } else {
            0.0
        };
        let cut_thr = if scene_cut_threshold.is_finite() {
            scene_cut_threshold.max(0.0)
        } else {
            1.0
        };

        let mut anchor_luma = subsample_luma_160x90(frames[0]);
        let mut prev_luma = anchor_luma.clone();
        let mut unique_runs = vec![DuplicateRun {
            start_frame: 0,
            count: 1,
        }];
        let mut scene_cuts = Vec::new();

        for (idx, frame) in frames.iter().enumerate().skip(1) {
            let cur_luma = subsample_luma_160x90(frame);
            let adj_mad = luma_mad(&prev_luma, &cur_luma);
            let is_cut = adj_mad >= cut_thr;
            if is_cut {
                scene_cuts.push(idx);
            }

            let run_mad = luma_mad(&anchor_luma, &cur_luma);
            if !is_cut && run_mad <= dedup_thr {
                if let Some(last_run) = unique_runs.last_mut() {
                    last_run.count += 1;
                }
            } else {
                unique_runs.push(DuplicateRun {
                    start_frame: idx,
                    count: 1,
                });
                anchor_luma = cur_luma.clone();
            }
            prev_luma = cur_luma;
        }

        ClipAnalysisReport {
            total_frames: frames.len(),
            unique_runs,
            scene_cuts,
        }
    }
}

/// Convenience wrapper around [`ClipAnalysis::analyze_frame_sequence`].
pub fn analyze_frame_sequence(
    frames: &[&Rgba8Frame],
    dedup_threshold: f32,
    scene_cut_threshold: f32,
) -> ClipAnalysisReport {
    ClipAnalysis::analyze_frame_sequence(frames, dedup_threshold, scene_cut_threshold)
}

/// Builds `rife-ncnn-vulkan` CLI arguments (`-m`, `-0`, `-1`, `-o`, `-s`) for pairwise frame interpolation.
pub fn build_rife_pair_args(
    model: &str,
    frame0: &Path,
    frame1: &Path,
    output: &Path,
    timestep: f32,
) -> Result<Vec<String>, MediaError> {
    if model.trim().is_empty() {
        return Err(MediaError::InvalidEncodingSettings(
            "RIFE model name/path cannot be empty".into(),
        ));
    }
    if !timestep.is_finite() || !(0.0..=1.0).contains(&timestep) {
        return Err(MediaError::InvalidEncodingSettings(format!(
            "RIFE timestep {timestep} must be finite and within 0.0..=1.0"
        )));
    }
    Ok(vec![
        "-m".into(),
        model.into(),
        "-0".into(),
        frame0.to_string_lossy().into_owned(),
        "-1".into(),
        frame1.to_string_lossy().into_owned(),
        "-o".into(),
        output.to_string_lossy().into_owned(),
        "-s".into(),
        format!("{timestep}"),
    ])
}

/// Builds an FFmpeg `minterpolate` filter string for motion-compensated optical-flow FPS conversion.
pub fn build_minterpolate_filter(fps_num: u32, fps_den: u32) -> Result<String, MediaError> {
    if fps_num == 0 || fps_den == 0 {
        return Err(TimeError::InvalidRational {
            num: fps_num,
            den: fps_den,
        }
        .into());
    }
    Ok(format!(
        "minterpolate=fps={fps_num}/{fps_den}:mi_mode=mci:mc_mode=aobmc:me_mode=bidir:vsbmc=1"
    ))
}

/// Pure-Rust linear RGBA8 blend helper used by [`InterpolatorBackend::FrameBlend`].
pub fn blend_frames(
    a: &Rgba8Frame,
    b: &Rgba8Frame,
    t: f32,
    pts: Flicks,
) -> Result<Rgba8Frame, MediaError> {
    if a.width == 0 || a.height == 0 {
        return Err(MediaError::InvalidDimensions {
            width: a.width,
            height: a.height,
        });
    }
    if a.width != b.width || a.height != b.height {
        return Err(MediaError::InvalidDimensions {
            width: b.width,
            height: b.height,
        });
    }
    if a.data.len() != b.data.len() {
        return Err(MediaError::BufferLengthMismatch {
            expected: a.data.len(),
            actual: b.data.len(),
        });
    }
    let alpha = if t.is_finite() {
        t.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let inv = 1.0 - alpha;
    let mut data = Vec::with_capacity(a.data.len());
    for (&pa, &pb) in a.data.iter().zip(b.data.iter()) {
        let blended = (pa as f32) * inv + (pb as f32) * alpha;
        data.push(blended.round().clamp(0.0, 255.0) as u8);
    }
    Rgba8Frame::new(a.width, a.height, pts, a.alpha_mode, data)
}

fn subsample_luma_160x90(frame: &Rgba8Frame) -> Vec<f32> {
    let w = frame.width as usize;
    let h = frame.height as usize;
    let mut out = vec![0.0f32; LUMA_SAMPLE_COUNT];
    if w == 0 || h == 0 || frame.data.len() < w.saturating_mul(h).saturating_mul(4) {
        return out;
    }

    for gy in 0..ANALYSIS_HEIGHT {
        let sy = (((2 * gy + 1) * h) / (2 * ANALYSIS_HEIGHT)).min(h - 1);
        let row_offset = sy * w * 4;
        let out_row = gy * ANALYSIS_WIDTH;
        for gx in 0..ANALYSIS_WIDTH {
            let sx = (((2 * gx + 1) * w) / (2 * ANALYSIS_WIDTH)).min(w - 1);
            let idx = row_offset + sx * 4;
            let r = frame.data[idx] as f32;
            let g = frame.data[idx + 1] as f32;
            let b = frame.data[idx + 2] as f32;
            let a_scale = match frame.alpha_mode {
                AlphaMode::Straight if frame.data[idx + 3] != 255 => {
                    frame.data[idx + 3] as f32 / 255.0
                }
                _ => 1.0,
            };
            out[out_row + gx] = ((0.2126 * r + 0.7152 * g + 0.0722 * b) / 255.0) * a_scale;
        }
    }
    out
}

fn luma_mad(a: &[f32], b: &[f32]) -> f32 {
    if a.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    let mut sum = 0.0f64;
    for (&va, &vb) in a.iter().zip(b.iter()) {
        sum += (va as f64 - vb as f64).abs();
    }
    (sum / a.len() as f64) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient_frame(width: u32, height: u32, bias: u8, invert: bool) -> Rgba8Frame {
        let mut data = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let base = (((x * 3 + y * 5) % 200) as u8).saturating_add(bias);
                let (r, g, b) = if invert {
                    (255 - base, (255 - base) / 2, base)
                } else {
                    (base, base.saturating_div(2), 255 - base)
                };
                data.extend_from_slice(&[r, g, b, 255]);
            }
        }
        Rgba8Frame::new(width, height, Flicks::ZERO, AlphaMode::Straight, data).unwrap()
    }

    #[test]
    fn clip_analysis_detects_anime_twos_threes_and_hard_cuts() {
        let black = Rgba8Frame::filled(320, 180, Flicks::ZERO, [0, 0, 0, 255], AlphaMode::Straight)
            .unwrap();
        let black_noise =
            Rgba8Frame::filled(320, 180, Flicks::ZERO, [1, 1, 1, 255], AlphaMode::Straight)
                .unwrap();
        let bright = Rgba8Frame::filled(
            320,
            180,
            Flicks::ZERO,
            [240, 240, 240, 255],
            AlphaMode::Straight,
        )
        .unwrap();
        let scene_c = gradient_frame(320, 180, 10, false);

        // Pattern: [black, black_noise, black] (run of 3), hard cut -> [bright, bright] (run of 2),
        // hard cut -> [scene_c] (run of 1)
        let seq = [&black, &black_noise, &black, &bright, &bright, &scene_c];
        let report = analyze_frame_sequence(&seq, 0.01, 0.25);

        assert_eq!(report.total_frames, 6);
        assert_eq!(
            report.unique_runs,
            vec![
                DuplicateRun {
                    start_frame: 0,
                    count: 3,
                },
                DuplicateRun {
                    start_frame: 3,
                    count: 2,
                },
                DuplicateRun {
                    start_frame: 5,
                    count: 1,
                },
            ]
        );
        assert_eq!(report.scene_cuts, vec![3, 5]);
        assert_eq!(report.unique_frame_count(), 3);
        assert_eq!(report.duplicate_frame_count(), 3);
        assert_eq!(
            report,
            ClipAnalysis::analyze_frame_sequence(&seq, 0.01, 0.25)
        );
    }

    #[test]
    fn clip_analysis_handles_empty_single_and_zero_dedup_threshold() {
        let empty = analyze_frame_sequence(&[], 0.005, 0.25);
        assert_eq!(empty.total_frames, 0);
        assert!(empty.unique_runs.is_empty());
        assert!(empty.scene_cuts.is_empty());

        let f0 = gradient_frame(64, 36, 0, false);
        let f1 = gradient_frame(64, 36, 2, false);
        let single = analyze_frame_sequence(&[&f0], 0.0, 0.25);
        assert_eq!(single.total_frames, 1);
        assert_eq!(
            single.unique_runs,
            vec![DuplicateRun {
                start_frame: 0,
                count: 1,
            }]
        );
        assert!(single.scene_cuts.is_empty());

        // Exact duplicates still merge at threshold 0.0, slight change splits
        let exact_and_diff = analyze_frame_sequence(&[&f0, &f0, &f1], 0.0, 0.5);
        assert_eq!(
            exact_and_diff.unique_runs,
            vec![
                DuplicateRun {
                    start_frame: 0,
                    count: 2,
                },
                DuplicateRun {
                    start_frame: 2,
                    count: 1,
                },
            ]
        );
        assert!(exact_and_diff.scene_cuts.is_empty());
    }

    #[test]
    fn rife_and_minterpolate_builders_format_and_validate() {
        let args = build_rife_pair_args(
            "rife-v4.6",
            Path::new("in0.png"),
            Path::new("in1.png"),
            Path::new("out.png"),
            0.5,
        )
        .unwrap();
        assert_eq!(
            args,
            vec![
                "-m",
                "rife-v4.6",
                "-0",
                "in0.png",
                "-1",
                "in1.png",
                "-o",
                "out.png",
                "-s",
                "0.5",
            ]
        );
        assert!(
            build_rife_pair_args(
                " ",
                Path::new("0.png"),
                Path::new("1.png"),
                Path::new("o.png"),
                0.5
            )
            .is_err()
        );
        assert!(
            build_rife_pair_args(
                "rife",
                Path::new("0.png"),
                Path::new("1.png"),
                Path::new("o.png"),
                1.5
            )
            .is_err()
        );

        let filter = build_minterpolate_filter(60, 1).unwrap();
        assert_eq!(
            filter,
            "minterpolate=fps=60/1:mi_mode=mci:mc_mode=aobmc:me_mode=bidir:vsbmc=1"
        );
        assert!(build_minterpolate_filter(60, 0).is_err());
        assert!(build_minterpolate_filter(0, 1).is_err());
    }

    #[test]
    fn frame_blend_interpolates_midpoint_cleanly() {
        let a = Rgba8Frame::filled(4, 4, Flicks::ZERO, [0, 100, 200, 255], AlphaMode::Straight)
            .unwrap();
        let b = Rgba8Frame::filled(4, 4, Flicks(100), [100, 200, 0, 255], AlphaMode::Straight)
            .unwrap();
        let mid = blend_frames(&a, &b, 0.5, Flicks(50)).unwrap();
        assert_eq!(mid.pts, Flicks(50));
        assert_eq!(mid.get_pixel(0, 0), Some([50, 150, 100, 255]));
    }
}
