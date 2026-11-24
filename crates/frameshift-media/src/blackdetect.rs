//! FFmpeg `blackdetect` video interval detector for P2 media linting & QA.

use crate::error::MediaError;
use frameshift_time::Flicks;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::{Command, Stdio};

/// A contiguous black-frame interval detected by `ffmpeg -vf blackdetect`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BlackInterval {
    pub start_sec: f64,
    pub end_sec: f64,
    pub duration_sec: f64,
    pub start_ticks: Flicks,
    pub end_ticks: Flicks,
    pub duration_ticks: Flicks,
}

/// Parameters controlling FFmpeg `blackdetect` filtering.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BlackDetectOptions {
    /// Minimum contiguous black duration in seconds (`d`, default `0.04`).
    pub min_duration_sec: f64,
    /// Fraction of pixels in a frame required to be below threshold (`pic_th` in `0.0..=1.0`, default `0.98`).
    pub pic_th: f64,
    /// Optional per-pixel luma threshold (`pix_th` in `0.0..=1.0`; `None` uses FFmpeg default `0.10`).
    pub pix_th: Option<f64>,
    /// Check alpha channel instead of luma (`alpha=1`).
    pub alpha: bool,
}

impl Default for BlackDetectOptions {
    fn default() -> Self {
        Self {
            min_duration_sec: 0.04,
            pic_th: 0.98,
            pix_th: None,
            alpha: false,
        }
    }
}

/// Detects contiguous black intervals in `path` via `ffmpeg -vf blackdetect`.
///
/// `min_duration_sec` maps to `d` and `pic_th` maps to `pic_th` (`0.0..=1.0`).
pub fn detect_black_intervals(
    path: &Path,
    min_duration_sec: f64,
    pic_th: f64,
) -> Result<Vec<BlackInterval>, MediaError> {
    detect_black_intervals_with_options(
        path,
        BlackDetectOptions {
            min_duration_sec,
            pic_th,
            ..Default::default()
        },
    )
}

/// Detects contiguous black intervals in `path` with full [`BlackDetectOptions`].
pub fn detect_black_intervals_with_options(
    path: &Path,
    options: BlackDetectOptions,
) -> Result<Vec<BlackInterval>, MediaError> {
    if !options.min_duration_sec.is_finite() || options.min_duration_sec < 0.0 {
        return Err(MediaError::InvalidEncodingSettings(format!(
            "invalid blackdetect min_duration_sec: {}",
            options.min_duration_sec
        )));
    }
    if !options.pic_th.is_finite() || !(0.0..=1.0).contains(&options.pic_th) {
        return Err(MediaError::InvalidEncodingSettings(format!(
            "invalid blackdetect pic_th (expected 0.0..=1.0): {}",
            options.pic_th
        )));
    }
    if let Some(pix_th) = options.pix_th
        && (!pix_th.is_finite() || !(0.0..=1.0).contains(&pix_th))
    {
        return Err(MediaError::InvalidEncodingSettings(format!(
            "invalid blackdetect pix_th (expected 0.0..=1.0): {pix_th}"
        )));
    }
    if !path.is_file() {
        return Err(MediaError::IoError {
            message: format!("media file '{}' not found", path.display()),
        });
    }

    let mut filter = format!(
        "blackdetect=d={:.6}:pic_th={:.6}",
        options.min_duration_sec, options.pic_th
    );
    if let Some(pix_th) = options.pix_th {
        filter.push_str(&format!(":pix_th={pix_th:.6}"));
    }
    if options.alpha {
        filter.push_str(":alpha=1");
    }

    let output = Command::new("ffmpeg")
        .args(["-hide_banner", "-nostdin", "-v", "info", "-i"])
        .arg(path)
        .args(["-vf", &filter, "-an", "-f", "null", "-"])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| MediaError::BinarySpawnFailed {
            binary: "ffmpeg".into(),
            reason: e.to_string(),
        })?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        return Err(MediaError::FfmpegFailed {
            exit_code: output.status.code(),
            stderr: stderr.trim().to_owned(),
        });
    }

    Ok(parse_blackdetect_stderr(&stderr))
}

/// Parses `ffmpeg -vf blackdetect` stderr lines (`black_start:... black_end:... black_duration:...`).
pub fn parse_blackdetect_stderr(stderr: &str) -> Vec<BlackInterval> {
    let mut intervals = Vec::new();
    for line in stderr.lines() {
        if !line.contains("black_start:") {
            continue;
        }
        let mut start_sec = None;
        let mut end_sec = None;
        let mut duration_sec = None;

        for token in line.split_whitespace() {
            if let Some(val) = token.strip_prefix("black_start:") {
                start_sec = val.parse::<f64>().ok().filter(|v| v.is_finite());
            } else if let Some(val) = token.strip_prefix("black_end:") {
                end_sec = val.parse::<f64>().ok().filter(|v| v.is_finite());
            } else if let Some(val) = token.strip_prefix("black_duration:") {
                duration_sec = val.parse::<f64>().ok().filter(|v| v.is_finite());
            }
        }

        if let (Some(start), Some(end)) = (start_sec, end_sec) {
            let dur = duration_sec.unwrap_or_else(|| (end - start).max(0.0));
            let start_ticks = Flicks::from_seconds_f64_approx(start).unwrap_or(Flicks::ZERO);
            let end_ticks = Flicks::from_seconds_f64_approx(end).unwrap_or(start_ticks);
            let duration_ticks = Flicks::from_seconds_f64_approx(dur).unwrap_or(Flicks::ZERO);
            intervals.push(BlackInterval {
                start_sec: start,
                end_sec: end,
                duration_sec: dur,
                start_ticks,
                end_ticks,
                duration_ticks,
            });
        }
    }
    intervals
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffmpeg::FfmpegBridge;
    use crate::frame::{AlphaMode, Rgba8Frame};
    use frameshift_time::FPS_24;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TempGuard(PathBuf);
    impl Drop for TempGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn parse_blackdetect_stderr_extracts_all_intervals() {
        let sample = "\
[Parsed_blackdetect_0 @ 000001e3ad8102c0] black_start:0 black_end:0.375 black_duration:0.375
[info] unrelated line
[Parsed_blackdetect_0 @ 000001e3ad8102c0] black_start:1.500 black_end:2.250 black_duration:0.750
";
        let intervals = parse_blackdetect_stderr(sample);
        assert_eq!(intervals.len(), 2);
        assert!((intervals[0].start_sec - 0.0).abs() < 1e-6);
        assert!((intervals[0].end_sec - 0.375).abs() < 1e-6);
        assert!((intervals[0].duration_sec - 0.375).abs() < 1e-6);
        assert_eq!(intervals[0].start_ticks, Flicks::ZERO);
        assert_eq!(
            intervals[0].end_ticks,
            Flicks::from_seconds_f64_approx(0.375).unwrap()
        );
        assert!((intervals[1].start_sec - 1.5).abs() < 1e-6);
        assert!((intervals[1].end_sec - 2.25).abs() < 1e-6);
    }

    #[test]
    fn detect_black_intervals_validates_inputs_and_detects_live_black_frames() {
        let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let mp4_path = std::env::temp_dir().join(format!(
            "frameshift_blackdetect_{}_{}.mp4",
            std::process::id(),
            id
        ));
        let _guard = TempGuard(mp4_path.clone());

        let mut frames = Vec::with_capacity(24);
        for i in 0..24 {
            let pts = FPS_24.frame_to_ticks(i as i64).unwrap();
            let rgba = if i < 12 {
                [0, 0, 0, 255]
            } else {
                [32, 220, 48, 255]
            };
            frames.push(Rgba8Frame::filled(64, 64, pts, rgba, AlphaMode::Straight).unwrap());
        }
        FfmpegBridge::encode_mp4(&frames, None, FPS_24, &mp4_path).unwrap();

        let intervals = detect_black_intervals(&mp4_path, 0.10, 0.98).unwrap();
        assert_eq!(
            intervals.len(),
            1,
            "expected exactly 1 black interval, got {intervals:?}"
        );
        assert!(intervals[0].start_sec <= 0.05);
        assert!(
            intervals[0].duration_sec >= 0.40 && intervals[0].duration_sec <= 0.56,
            "unexpected black duration: {}",
            intervals[0].duration_sec
        );

        assert!(matches!(
            detect_black_intervals(&mp4_path, -0.1, 0.98),
            Err(MediaError::InvalidEncodingSettings(_))
        ));
        assert!(matches!(
            detect_black_intervals(&mp4_path, 0.1, 1.5),
            Err(MediaError::InvalidEncodingSettings(_))
        ));
    }
}
