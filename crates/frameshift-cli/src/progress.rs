use frameshift_core::{RenderProgress, RenderStage};
use std::io::{IsTerminal, Write};
use std::time::{Duration, Instant};

/// Number of leading frames ignored by the EMA rate estimate.
pub const WARMUP_FRAMES: usize = 10;
/// EMA smoothing factor applied to frame durations after warm-up.
pub const EMA_ALPHA: f64 = 0.1;
/// Minimum interval between two in-place redraws on a terminal.
pub const INTERACTIVE_INTERVAL: Duration = Duration::from_millis(100);

/// Controls whether render progress is written to stderr.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProgressMode {
    /// Write progress only when stderr is a terminal.
    #[default]
    Auto,
    /// Always write progress to stderr (`--progress`).
    Always,
    /// Never write progress (`--quiet`).
    Never,
}

impl ProgressMode {
    /// Resolves the mode against a terminal check.
    pub fn is_enabled(self, stderr_is_tty: bool) -> bool {
        match self {
            Self::Auto => stderr_is_tty,
            Self::Always => true,
            Self::Never => false,
        }
    }
}

/// Rate and remaining-time estimate derived from observed frame durations.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EtaEstimate {
    /// Smoothed frames per second.
    pub fps: f64,
    /// Estimated remaining render time.
    pub eta: Duration,
}

/// Warm-up + EMA estimator over cumulative frame completion times.
///
/// The first [`WARMUP_FRAMES`] frames only feed the initial average; after that each
/// frame duration is blended with [`EMA_ALPHA`]. A short render still reports its final
/// line using the overall average so the last frame is never silent.
#[derive(Debug, Clone)]
pub struct EtaEstimator {
    samples: usize,
    warmup_total_seconds: f64,
    ema_seconds_per_frame: f64,
    last_elapsed: Duration,
}

impl Default for EtaEstimator {
    fn default() -> Self {
        Self {
            samples: 0,
            warmup_total_seconds: 0.0,
            ema_seconds_per_frame: 0.0,
            last_elapsed: Duration::ZERO,
        }
    }
}

impl EtaEstimator {
    /// Creates a fresh estimator.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one completed frame at cumulative `elapsed`.
    ///
    /// Returns `None` during warm-up, except on the final frame, where the overall
    /// average is used so the caller can always print a closing line.
    pub fn record(
        &mut self,
        completed: usize,
        total: usize,
        elapsed: Duration,
    ) -> Option<EtaEstimate> {
        if total == 0 {
            return None;
        }
        let delta = elapsed.saturating_sub(self.last_elapsed);
        self.last_elapsed = elapsed;
        self.samples += 1;
        let delta_seconds = delta.as_secs_f64();
        if self.samples < WARMUP_FRAMES {
            self.warmup_total_seconds += delta_seconds;
            if completed < total {
                return None;
            }
        } else if self.samples == WARMUP_FRAMES {
            self.warmup_total_seconds += delta_seconds;
            self.ema_seconds_per_frame = self.warmup_total_seconds / WARMUP_FRAMES as f64;
        } else {
            self.ema_seconds_per_frame =
                EMA_ALPHA * delta_seconds + (1.0 - EMA_ALPHA) * self.ema_seconds_per_frame;
        }

        let seconds_per_frame = if self.samples >= WARMUP_FRAMES {
            self.ema_seconds_per_frame
        } else {
            elapsed.as_secs_f64() / completed.max(1) as f64
        };
        if !seconds_per_frame.is_finite() || seconds_per_frame <= 0.0 {
            return None;
        }
        let remaining = total.saturating_sub(completed) as f64;
        Some(EtaEstimate {
            fps: 1.0 / seconds_per_frame,
            eta: Duration::from_secs_f64(seconds_per_frame * remaining),
        })
    }
}

/// Formats a duration as `M:SS`, or `H:MM:SS` from one hour on; seconds round up.
pub fn format_duration(duration: Duration) -> String {
    let mut seconds = duration.as_secs();
    if duration.subsec_nanos() > 0 {
        seconds += 1;
    }
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes}:{secs:02}")
    }
}

/// Formats the canonical frame line: `frame N/M | X.Y fps | ETA M:SS`.
pub fn format_progress_line(completed: usize, total: usize, estimate: EtaEstimate) -> String {
    format!(
        "frame {completed}/{total} | {:.1} fps | ETA {}",
        estimate.fps,
        format_duration(estimate.eta)
    )
}

/// Formats a non-frame stage line, e.g. `audio mix 1/1 | 0:02`.
pub fn format_stage_line(
    stage: RenderStage,
    completed: usize,
    total: usize,
    elapsed: Duration,
) -> String {
    let label = match stage {
        RenderStage::Frames => "frames",
        RenderStage::AudioMix => "audio mix",
        RenderStage::Encode => "encode",
    };
    format!("{label} {completed}/{total} | {}", format_duration(elapsed))
}

/// Writes render progress to a writer, in place on a terminal and line-by-line otherwise.
///
/// The reporter owns all formatting and timing; the engine only emits
/// [`RenderProgress`] events. stdout is never touched.
pub struct ProgressReporter<W: Write> {
    writer: W,
    enabled: bool,
    interactive: bool,
    estimator: EtaEstimator,
    started: Instant,
    last_emit: Option<Duration>,
    line_active: bool,
    line_len: usize,
}

impl ProgressReporter<std::io::Stderr> {
    /// Creates a reporter over stderr, resolving [`ProgressMode::Auto`] against the TTY check.
    pub fn stderr(mode: ProgressMode) -> Self {
        let stderr = std::io::stderr();
        let stderr_is_tty = stderr.is_terminal();
        Self::new(stderr, mode, stderr_is_tty)
    }
}

impl<W: Write> ProgressReporter<W> {
    /// Creates a reporter over `writer`.
    pub fn new(writer: W, mode: ProgressMode, stderr_is_tty: bool) -> Self {
        let enabled = mode.is_enabled(stderr_is_tty);
        Self {
            writer,
            enabled,
            interactive: enabled && stderr_is_tty,
            estimator: EtaEstimator::new(),
            started: Instant::now(),
            last_emit: None,
            line_active: false,
            line_len: 0,
        }
    }

    /// Handles one engine progress event.
    pub fn on_progress(&mut self, progress: RenderProgress) {
        let elapsed = self.started.elapsed();
        self.on_progress_at(progress, elapsed);
    }

    fn on_progress_at(&mut self, progress: RenderProgress, elapsed: Duration) {
        if !self.enabled {
            return;
        }
        match progress.stage {
            RenderStage::Frames => self.on_frame(progress, elapsed),
            RenderStage::AudioMix | RenderStage::Encode => {
                self.end_line();
                let line =
                    format_stage_line(progress.stage, progress.completed, progress.total, elapsed);
                let _ = writeln!(self.writer, "{line}");
                let _ = self.writer.flush();
            }
        }
    }

    fn on_frame(&mut self, progress: RenderProgress, elapsed: Duration) {
        let Some(estimate) = self
            .estimator
            .record(progress.completed, progress.total, elapsed)
        else {
            return;
        };
        let is_final = progress.completed >= progress.total;
        let line = format_progress_line(progress.completed, progress.total, estimate);
        if self.interactive {
            if !is_final && !self.should_redraw(elapsed) {
                return;
            }
            self.write_in_place(&line);
            self.last_emit = Some(elapsed);
        } else {
            let step = (progress.total / 100).max(1);
            if !is_final && !progress.completed.is_multiple_of(step) {
                return;
            }
            let _ = writeln!(self.writer, "{line}");
            let _ = self.writer.flush();
        }
    }

    fn should_redraw(&self, elapsed: Duration) -> bool {
        match self.last_emit {
            None => true,
            Some(last) => elapsed.saturating_sub(last) >= INTERACTIVE_INTERVAL,
        }
    }

    fn write_in_place(&mut self, line: &str) {
        let padding = self.line_len.saturating_sub(line.len());
        let _ = write!(self.writer, "\r{line}");
        if padding > 0 {
            let _ = write!(self.writer, "{:width$}", "", width = padding);
        }
        let _ = self.writer.flush();
        self.line_active = true;
        self.line_len = line.len();
    }

    fn end_line(&mut self) {
        if self.line_active {
            let _ = writeln!(self.writer);
            let _ = self.writer.flush();
            self.line_active = false;
            self.line_len = 0;
        }
    }

    /// Terminates any in-place line and flushes the writer.
    pub fn finish(&mut self) {
        if !self.enabled {
            return;
        }
        self.end_line();
        let _ = self.writer.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames(completed: usize, total: usize) -> RenderProgress {
        RenderProgress::frames(completed, total)
    }

    #[test]
    fn format_duration_rounds_up_and_switches_at_one_hour() {
        assert_eq!(format_duration(Duration::from_secs(0)), "0:00");
        assert_eq!(format_duration(Duration::from_secs(59)), "0:59");
        assert_eq!(format_duration(Duration::from_millis(59_900)), "1:00");
        assert_eq!(format_duration(Duration::from_secs(3_599)), "59:59");
        assert_eq!(format_duration(Duration::from_secs(3_600)), "1:00:00");
        assert_eq!(format_duration(Duration::from_secs(3_661)), "1:01:01");
    }

    #[test]
    fn format_progress_line_matches_documented_shape() {
        let line = format_progress_line(
            600,
            1200,
            EtaEstimate {
                fps: 3.2,
                eta: Duration::from_secs(188),
            },
        );
        assert_eq!(line, "frame 600/1200 | 3.2 fps | ETA 3:08");
        let regex_like = line.starts_with("frame 600/1200 | 3.2 fps | ETA 3:");
        assert!(regex_like);
    }

    #[test]
    fn format_stage_line_names_each_stage() {
        assert_eq!(
            format_stage_line(RenderStage::AudioMix, 1, 1, Duration::from_millis(250)),
            "audio mix 1/1 | 0:01"
        );
        assert_eq!(
            format_stage_line(RenderStage::Encode, 0, 1, Duration::from_secs(5)),
            "encode 0/1 | 0:05"
        );
    }

    #[test]
    fn eta_estimator_warms_up_then_reports_ema_rate() {
        let mut estimator = EtaEstimator::new();
        let mut last = None;
        for index in 1..=600 {
            last = estimator.record(index, 1200, Duration::from_millis(100 * index as u64));
            if index < WARMUP_FRAMES {
                assert!(last.is_none(), "warm-up frame {index} must stay silent");
            }
        }
        let estimate = last.expect("post-warm-up frame reports a rate");
        assert!((estimate.fps - 10.0).abs() < 1e-9);
        assert_eq!(estimate.eta, Duration::from_secs(60));
        assert_eq!(
            format_progress_line(600, 1200, estimate),
            "frame 600/1200 | 10.0 fps | ETA 1:00"
        );
    }

    #[test]
    fn eta_estimator_reports_final_line_before_warmup_completes() {
        let mut estimator = EtaEstimator::new();
        let mut last = None;
        for index in 1..=5 {
            last = estimator.record(index, 5, Duration::from_millis(100 * index as u64));
        }
        let estimate = last.expect("final frame always reports");
        assert!((estimate.fps - 10.0).abs() < 1e-9);
        assert_eq!(estimate.eta, Duration::ZERO);
    }

    #[test]
    fn quiet_mode_writes_nothing() {
        let mut reporter = ProgressReporter::new(Vec::new(), ProgressMode::Never, true);
        assert!(!reporter.enabled);
        reporter.on_progress_at(frames(0, 4), Duration::ZERO);
        reporter.on_progress_at(frames(4, 4), Duration::from_secs(1));
        reporter.on_progress_at(RenderProgress::encode(1, 1), Duration::from_secs(1));
        reporter.finish();
        assert!(reporter.writer.is_empty());
    }

    #[test]
    fn auto_mode_requires_a_terminal() {
        assert!(!ProgressMode::Auto.is_enabled(false));
        assert!(ProgressMode::Auto.is_enabled(true));
        assert!(ProgressMode::Always.is_enabled(false));
        assert!(!ProgressMode::Never.is_enabled(true));
    }

    #[test]
    fn non_interactive_reporter_writes_one_line_per_step() {
        let mut reporter = ProgressReporter::new(Vec::new(), ProgressMode::Always, false);
        for index in 1..=1200 {
            reporter.on_progress_at(frames(index, 1200), Duration::from_millis(10 * index as u64));
        }
        reporter.finish();
        let text = String::from_utf8(reporter.writer).expect("utf-8 progress");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 100, "1200 frames at step 12 -> 100 lines");
        assert!(lines[0].starts_with("frame 12/1200 |"));
        assert!(lines[99].starts_with("frame 1200/1200 |"));
        assert!(!text.contains('\r'));
    }

    #[test]
    fn interactive_reporter_throttles_redraws_to_100ms() {
        let mut reporter = ProgressReporter::new(Vec::new(), ProgressMode::Always, true);
        for index in 1..=10 {
            reporter.on_progress_at(frames(index, 200), Duration::from_millis(10 * index as u64));
        }
        reporter.on_progress_at(frames(11, 200), Duration::from_millis(160));
        reporter.on_progress_at(frames(12, 200), Duration::from_millis(260));
        reporter.finish();
        let text = String::from_utf8(reporter.writer).expect("utf-8 progress");
        assert_eq!(text.matches("frame ").count(), 2, "10th and 12th frame only");
        assert!(text.starts_with('\r'));
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn interactive_reporter_pads_shorter_lines() {
        let mut reporter = ProgressReporter::new(Vec::new(), ProgressMode::Always, true);
        for index in 1..=10 {
            reporter.on_progress_at(frames(index, 20), Duration::from_millis(10 * index as u64));
        }
        for index in 11..=20 {
            reporter.on_progress_at(frames(index, 20), Duration::from_millis(100 * index as u64));
        }
        reporter.finish();
        let text = String::from_utf8(reporter.writer).expect("utf-8 progress");
        assert!(text.contains("frame 10/20"));
        assert!(text.contains("frame 20/20"));
        assert!(text.ends_with('\n'));
    }
}
