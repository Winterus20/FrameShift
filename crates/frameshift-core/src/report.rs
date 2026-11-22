use crate::range::RenderRange;
use frameshift_media::CacheTelemetry;
use frameshift_schema::{Project, RenderProfileDef, RenderProfileKind};
use frameshift_time::{Flicks, Rational32};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::path::PathBuf;

/// Summary returned after rendering a full timeline to MP4.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoRenderSummary {
    pub out_path: PathBuf,
    pub profile: RenderProfileKind,
    pub width: u32,
    pub height: u32,
    pub frame_count: usize,
    pub fps: Rational32,
    pub duration_ticks: Flicks,
    pub has_audio: bool,
    /// Resolved range when only a timeline slice was rendered; `None` for a full render.
    ///
    /// `duration_ticks` always describes the whole timeline, never the rendered slice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<RenderRange>,
    pub frame_hashes: Vec<u64>,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub cache_evictions: u64,
}

impl VideoRenderSummary {
    pub(crate) fn from_project(
        project: &Project,
        profile_def: &RenderProfileDef,
        out_path: PathBuf,
        has_audio: bool,
        frame_hashes: Vec<u64>,
        telemetry: CacheTelemetry,
    ) -> Self {
        Self::from_project_range(
            project,
            profile_def,
            out_path,
            has_audio,
            frame_hashes,
            telemetry,
            None,
        )
    }

    pub(crate) fn from_project_range(
        project: &Project,
        profile_def: &RenderProfileDef,
        out_path: PathBuf,
        has_audio: bool,
        frame_hashes: Vec<u64>,
        telemetry: CacheTelemetry,
        range: Option<RenderRange>,
    ) -> Self {
        Self {
            out_path,
            profile: profile_def.kind,
            width: profile_def.resolution.width,
            height: profile_def.resolution.height,
            frame_count: frame_hashes.len(),
            fps: project.timeline.fps,
            duration_ticks: project.timeline.duration_ticks,
            has_audio,
            range,
            frame_hashes,
            cache_hits: telemetry.hits,
            cache_misses: telemetry.misses,
            cache_evictions: telemetry.evictions,
        }
    }
}

/// Determinism verification gate: compares two independent full-sequence renders frame-by-frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeterminismCheckReport {
    pub passed: bool,
    pub frame_count: usize,
    pub mismatched_frames: Vec<usize>,
}

/// Seek-independence verification gate: compares random/reverse single-frame seeks vs sequential render.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeekIndependenceCheckReport {
    pub passed: bool,
    pub tested_ticks: Vec<Flicks>,
    pub max_channel_diff: u8,
    pub mismatched_ticks: Vec<Flicks>,
}

/// Temporal motion blur verification gate: compares `sub_samples > 1` (`Final`) vs `sub_samples == 1` (`Draft`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MotionBlurCheckReport {
    pub passed: bool,
    pub inspected_tick: Flicks,
    pub final_sub_samples: u32,
    pub draft_sub_samples: u32,
    pub final_hash: u64,
    pub draft_hash: u64,
    pub mean_l1_channel_diff: f64,
    pub timing_anchors_identical: bool,
}

/// Audio-impact synchronization & ADR-0003 `ImpactSolver` verification gate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioImpactSyncReport {
    pub passed: bool,
    pub target_beat_tick: Flicks,
    pub visual_impact_flash_tick: Flicks,
    pub audio_transient_peak_tick: Flicks,
    pub delta_ticks: i64,
    pub max_allowed_delta_ticks: i64,
    pub solver_achieved_source_tick: Flicks,
    pub solver_expected_source_tick: Flicks,
    pub solver_error_ticks: i64,
    pub impossible_constraint_rejected: bool,
    pub impossible_suggestion_count: usize,
}

/// Complete M1 Golden Vertical Slice verification report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct M1VerificationReport {
    pub all_gates_passed: bool,
    pub out_dir: PathBuf,
    pub golden_project_json: PathBuf,
    pub golden_final_mp4: PathBuf,
    pub golden_draft_mp4: PathBuf,
    pub key_frames: Vec<PathBuf>,
    pub final_summary: VideoRenderSummary,
    pub draft_summary: VideoRenderSummary,
    pub determinism: DeterminismCheckReport,
    pub seek_independence: SeekIndependenceCheckReport,
    pub motion_blur: MotionBlurCheckReport,
    pub audio_impact_sync: AudioImpactSyncReport,
    pub mp4_probe_verified: bool,
}

impl M1VerificationReport {
    /// Formats a concise human-readable Markdown summary of all verification gates.
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();
        let _ = writeln!(
            md,
            "# FrameShift M1 Golden Vertical Slice Verification Report\n"
        );
        let _ = writeln!(
            md,
            "- **Overall Status:** {}",
            if self.all_gates_passed {
                "PASS"
            } else {
                "FAIL"
            }
        );
        let _ = writeln!(md, "- **Output Directory:** `{}`", self.out_dir.display());
        let _ = writeln!(
            md,
            "- **Project Preset:** `{}`",
            self.golden_project_json.display()
        );
        let _ = writeln!(
            md,
            "- **Final MP4:** `{}` ({}x{}, {} frames, audio={})",
            self.golden_final_mp4.display(),
            self.final_summary.width,
            self.final_summary.height,
            self.final_summary.frame_count,
            self.final_summary.has_audio
        );
        let _ = writeln!(
            md,
            "- **Draft MP4:** `{}` ({}x{}, {} frames, audio={})\n",
            self.golden_draft_mp4.display(),
            self.draft_summary.width,
            self.draft_summary.height,
            self.draft_summary.frame_count,
            self.draft_summary.has_audio
        );

        let _ = writeln!(md, "## Inspection PNG Frames\n");
        for path in &self.key_frames {
            let _ = writeln!(md, "- `{}`", path.display());
        }

        let _ = writeln!(md, "\n## Verification Gates\n");
        let _ = writeln!(md, "| Gate | Status | Details |\n| :--- | :---: | :--- |");
        let _ = writeln!(
            md,
            "| Determinism (2x full render) | {} | {} frames checked, {} mismatches |",
            pass_str(self.determinism.passed),
            self.determinism.frame_count,
            self.determinism.mismatched_frames.len()
        );
        let _ = writeln!(
            md,
            "| Seek Independence (5 scrambled seeks) | {} | max channel diff = {}, mismatches = {} |",
            pass_str(self.seek_independence.passed),
            self.seek_independence.max_channel_diff,
            self.seek_independence.mismatched_ticks.len()
        );
        let _ = writeln!(
            md,
            "| Temporal Motion Blur (Final {} vs Draft {}) | {} | mean L1 diff = {:.4}, anchors identical = {} |",
            self.motion_blur.final_sub_samples,
            self.motion_blur.draft_sub_samples,
            pass_str(self.motion_blur.passed),
            self.motion_blur.mean_l1_channel_diff,
            self.motion_blur.timing_anchors_identical
        );
        let _ = writeln!(
            md,
            "| Audio-Impact Sync & ADR-0003 | {} | delta = {} ticks (limit {}), solver error = {} ticks, impossible rejected = {} ({} suggestions) |",
            pass_str(self.audio_impact_sync.passed),
            self.audio_impact_sync.delta_ticks,
            self.audio_impact_sync.max_allowed_delta_ticks,
            self.audio_impact_sync.solver_error_ticks,
            self.audio_impact_sync.impossible_constraint_rejected,
            self.audio_impact_sync.impossible_suggestion_count
        );
        let _ = writeln!(
            md,
            "| Muxed MP4 Container Probe (`ffprobe`) | {} | verified video + audio streams in final MP4 |",
            pass_str(self.mp4_probe_verified)
        );

        md
    }
}

/// Phase 5 Proxy Semantic Equivalence and Hardware Acceleration verification report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Phase5VerificationReport {
    pub all_gates_passed: bool,
    pub proxy_width: u32,
    pub proxy_height: u32,
    pub full_width: u32,
    pub full_height: u32,
    pub pts_index_preserved: bool,
    pub proxy_frame_count: usize,
    pub semantic_timing_identical: bool,
    pub draft_vs_final_mean_l1_diff: f64,
    pub impact_tick: Flicks,
    pub available_hwaccels: Vec<String>,
    pub preferred_decoder: Option<String>,
    pub preferred_encoder: Option<String>,
    pub software_policy_backend: String,
    pub auto_policy_backend: String,
    pub hw_fallback_mp4_verified: bool,
    pub proxy_mp4_path: PathBuf,
}

fn pass_str(passed: bool) -> &'static str {
    if passed { "PASS" } else { "FAIL" }
}
