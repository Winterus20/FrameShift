//! Revision-pinned local export queue.
//!
//! Each job owns a serialized project snapshot and copies of every explicit media,
//! LUT, OCIO LUT, and font dependency. Output MP4s are rendered beside their final
//! destination, probed, and renamed into place only after the probe succeeds.

use crate::engine::{Engine, VideoRenderSettings};
use crate::error::CoreError;
use crate::progress::{RenderProgress, RenderStage};
use crate::range::RenderRange;
use crate::report::VideoRenderSummary;
use frameshift_media::{
    ExportEncodingProfile, FfmpegBridge, FileContentIdentity, MediaProbeInfo, VideoCodec,
    VideoEncodingSettings,
};
use frameshift_render::{OcioColorPipeline, OcioTransform, RenderOptions};
use frameshift_schema::{AssetKind, LutSourceDef, NodeKind, Project, RenderProfileKind};
use frameshift_vector::{FontQuery, ResolvedFont, resolve_font_chain};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static JOB_COUNTER: AtomicU64 = AtomicU64::new(0);
static STAGE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Opaque stable identifier assigned to one queued export.
pub type ExportJobId = String;

/// One requested output pass. All fields are copied into the job at enqueue time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportTarget {
    pub output_path: PathBuf,
    pub profile: RenderProfileKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<RenderRange>,
    #[serde(default)]
    pub encoding: VideoEncodingSettings,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform_preset: Option<String>,
}

impl ExportTarget {
    pub fn new(output_path: impl Into<PathBuf>, profile: RenderProfileKind) -> Self {
        Self {
            output_path: output_path.into(),
            profile,
            range: None,
            encoding: VideoEncodingSettings::default(),
            platform_preset: None,
        }
    }
}

/// Resolves and validates an optional `ExportTarget::platform_preset` into an [`ExportEncodingProfile`].
pub fn resolve_target_encoding_profile(
    target: &ExportTarget,
) -> Result<Option<ExportEncodingProfile>, CoreError> {
    let Some(ref name) = target.platform_preset else {
        return Ok(None);
    };
    let profile = ExportEncodingProfile::from_platform_name(name).ok_or_else(|| {
        CoreError::InvalidArgument(format!(
            "unknown platform preset '{name}'; expected tiktok_1080p60, reels_1080p30, youtube_1080p60, anime_10bit_hevc, or prores_422hq"
        ))
    })?;
    profile.validate()?;
    Ok(Some(profile))
}

/// A cloneable handle that can request cancellation while a queue worker renders.
#[derive(Debug, Clone)]
pub struct ExportTicket {
    pub job_id: ExportJobId,
    cancelled: Arc<AtomicBool>,
}

impl ExportTicket {
    /// Requests cancellation. The engine checks between frames and before publishing output.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportJobState {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportOutputState {
    Pending,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

/// Dependency copied or materialized into the job-owned snapshot directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportDependencyRecord {
    pub kind: String,
    pub source_path: String,
    pub pinned_path: PathBuf,
    pub identity: FileContentIdentity,
}

/// Progress for the currently running composition pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportProgressSnapshot {
    pub composition_pass: usize,
    pub stage: String,
    pub completed: usize,
    pub total: usize,
}

/// FFprobe facts and content identity for one successfully published output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportProbeReport {
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub width: u32,
    pub height: u32,
    pub frame_count: usize,
    pub fps: Option<frameshift_time::Rational32>,
    pub has_audio: bool,
    pub identity: FileContentIdentity,
    pub verified: bool,
}

/// Status for one requested output/composition pass.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportOutputReport {
    /// One-based pass order. Each output is reported as its own compositor pass.
    pub composition_pass: usize,
    pub output_path: PathBuf,
    pub profile: RenderProfileKind,
    pub range: Option<RenderRange>,
    pub encoding: VideoEncodingSettings,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform_preset: Option<String>,
    pub state: ExportOutputState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<VideoRenderSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub probe: Option<ExportProbeReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Serializable view of one queued job. This is the local queue's status and audit record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportJobSnapshot {
    pub job_id: ExportJobId,
    pub state: ExportJobState,
    pub revision_id: u64,
    pub source_project_sha256: String,
    pub pinned_project_path: PathBuf,
    pub pinned_project_sha256: String,
    pub renderer_identity: String,
    pub shader_identity_sha256: String,
    pub shader_identities: Vec<String>,
    pub dependencies: Vec<ExportDependencyRecord>,
    pub composition_passes_required: usize,
    pub composition_passes_completed: usize,
    pub current_progress: Option<ExportProgressSnapshot>,
    pub outputs: Vec<ExportOutputReport>,
    pub error: Option<String>,
}

struct ExportJob {
    snapshot: ExportJobSnapshot,
    pinned_project: Project,
    targets: Vec<ExportTarget>,
    snapshot_dir: PathBuf,
    cancelled: Arc<AtomicBool>,
}

/// In-memory FIFO queue for headless local export jobs.
///
/// Enqueueing snapshots immediately, so later edits to the caller's `Project` and later
/// edits to source media cannot change the queued job. Retry reuses the same pinned bytes.
#[derive(Default)]
pub struct ExportQueue {
    jobs: HashMap<ExportJobId, ExportJob>,
    pending: VecDeque<ExportJobId>,
    insertion_order: Vec<ExportJobId>,
}

impl ExportQueue {
    /// Pins a project revision and its dependencies, then appends the job to this local queue.
    pub fn enqueue(
        &mut self,
        project: &Project,
        project_dir: &Path,
        targets: Vec<ExportTarget>,
    ) -> Result<ExportTicket, CoreError> {
        project.validate()?;
        if targets.is_empty() {
            return Err(CoreError::InvalidArgument(
                "an export job needs at least one output target".into(),
            ));
        }

        let mut normalized_targets = targets;
        let cwd = std::env::current_dir()?;
        let mut unique_outputs = std::collections::HashSet::new();
        for target in &mut normalized_targets {
            let resolved_profile = resolve_target_encoding_profile(target)?;
            let expected_ext = resolved_profile
                .as_ref()
                .map_or("mp4", |p| p.codec.default_extension());
            if target
                .output_path
                .extension()
                .and_then(|v| v.to_str())
                .is_none_or(|ext| !ext.eq_ignore_ascii_case(expected_ext))
            {
                return Err(CoreError::InvalidArgument(format!(
                    "export output must use .{expected_ext}: {}",
                    target.output_path.display()
                )));
            }
            if target.output_path.is_relative() {
                target.output_path = cwd.join(&target.output_path);
            }
            if !unique_outputs.insert(target.output_path.clone()) {
                return Err(CoreError::InvalidArgument(format!(
                    "duplicate output path in export job: {}",
                    target.output_path.display()
                )));
            }
            target.encoding.validate()?;
            if let Some(range) = target.range {
                let resolved = RenderRange::resolve(project, range.start_tick, range.end_tick)?;
                if resolved != range {
                    return Err(CoreError::InvalidArgument(format!(
                        "export range must be frame-aligned and resolved for this project: {}..{}",
                        range.start_tick.0, range.end_tick.0
                    )));
                }
            }
        }

        let job_id = next_job_id();
        let snapshot_dir = std::env::temp_dir()
            .join("frameshift-export-jobs")
            .join(&job_id);
        fs::create_dir_all(&snapshot_dir)?;

        let source_bytes = serde_json::to_vec(project)?;
        let source_project_sha256 = FfmpegBridge::hash_bytes(&source_bytes).sha256;
        let mut pinned_project = project.clone();
        let mut dependencies = Vec::new();
        let pin_result = pin_project_dependencies(
            &mut pinned_project,
            project_dir,
            &snapshot_dir,
            &mut dependencies,
        );
        if let Err(error) = pin_result {
            let _ = fs::remove_dir_all(&snapshot_dir);
            return Err(error);
        }
        let project_json = serde_json::to_vec_pretty(&pinned_project)?;
        let pinned_project_sha256 = FfmpegBridge::hash_bytes(&project_json).sha256;
        let pinned_project_path = snapshot_dir.join("project.json");
        fs::write(&pinned_project_path, &project_json)?;

        let shader_identities = collect_shader_identities(&pinned_project);
        let shader_identity_bytes = serde_json::to_vec(&shader_identities)?;
        let shader_identity_sha256 = FfmpegBridge::hash_bytes(&shader_identity_bytes).sha256;
        let outputs = normalized_targets
            .iter()
            .enumerate()
            .map(|(index, target)| ExportOutputReport {
                composition_pass: index + 1,
                output_path: target.output_path.clone(),
                profile: target.profile,
                range: target.range,
                encoding: target.encoding.clone(),
                platform_preset: target.platform_preset.clone(),
                state: ExportOutputState::Pending,
                summary: None,
                probe: None,
                error: None,
            })
            .collect::<Vec<_>>();
        let ticket = ExportTicket {
            job_id: job_id.clone(),
            cancelled: Arc::new(AtomicBool::new(false)),
        };
        let snapshot = ExportJobSnapshot {
            job_id: job_id.clone(),
            state: ExportJobState::Queued,
            revision_id: project.revision_id,
            source_project_sha256,
            pinned_project_path,
            pinned_project_sha256,
            renderer_identity: format!("frameshift-render/{}", env!("CARGO_PKG_VERSION")),
            shader_identity_sha256,
            shader_identities,
            dependencies,
            composition_passes_required: normalized_targets.len(),
            composition_passes_completed: 0,
            current_progress: None,
            outputs,
            error: None,
        };
        self.jobs.insert(
            job_id.clone(),
            ExportJob {
                snapshot,
                pinned_project,
                targets: normalized_targets,
                snapshot_dir,
                cancelled: ticket.cancelled.clone(),
            },
        );
        self.pending.push_back(job_id.clone());
        self.insertion_order.push(job_id);
        Ok(ticket)
    }

    /// Returns a status/audit snapshot for a queued or completed job.
    pub fn get(&self, job_id: &str) -> Option<ExportJobSnapshot> {
        self.jobs.get(job_id).map(|job| job.snapshot.clone())
    }

    /// Returns all local jobs in enqueue order.
    pub fn list(&self) -> Vec<ExportJobSnapshot> {
        self.insertion_order
            .iter()
            .filter_map(|job_id| self.get(job_id))
            .collect()
    }

    /// Runs the next queued job synchronously and returns its final/current status.
    /// A failed output is recorded in the status snapshot; it does not abort later queue jobs.
    pub fn run_next(&mut self, engine: &mut Engine) -> Option<ExportJobSnapshot> {
        let job_id = self.pending.pop_front()?;
        if let Some(job) = self.jobs.get_mut(&job_id) {
            if job.snapshot.state == ExportJobState::Queued {
                run_job(job, engine);
            }
            Some(job.snapshot.clone())
        } else {
            None
        }
    }

    /// Schedules a failed or cancelled job for retry using the original pinned project/media.
    /// Outputs already published successfully are skipped.
    pub fn retry(&mut self, job_id: &str) -> Result<ExportTicket, CoreError> {
        let job = self
            .jobs
            .get_mut(job_id)
            .ok_or_else(|| CoreError::InvalidArgument(format!("unknown export job '{job_id}'")))?;
        if !matches!(
            job.snapshot.state,
            ExportJobState::Failed | ExportJobState::Cancelled
        ) {
            return Err(CoreError::InvalidArgument(format!(
                "export job '{job_id}' is not failed or cancelled"
            )));
        }
        job.snapshot.state = ExportJobState::Queued;
        job.snapshot.error = None;
        job.snapshot.current_progress = None;
        for output in &mut job.snapshot.outputs {
            if output.state != ExportOutputState::Succeeded {
                output.state = ExportOutputState::Pending;
                output.summary = None;
                output.probe = None;
                output.error = None;
            }
        }
        job.cancelled = Arc::new(AtomicBool::new(false));
        let ticket = ExportTicket {
            job_id: job_id.to_owned(),
            cancelled: job.cancelled.clone(),
        };
        self.pending.push_back(job_id.to_owned());
        Ok(ticket)
    }
}

impl Drop for ExportQueue {
    fn drop(&mut self) {
        for job in self.jobs.values() {
            let _ = fs::remove_dir_all(&job.snapshot_dir);
        }
    }
}

fn run_job(job: &mut ExportJob, engine: &mut Engine) {
    job.snapshot.state = ExportJobState::Running;
    job.snapshot.error = None;
    job.snapshot.current_progress = None;

    if let Err(error) = verify_pinned_snapshot(job) {
        if let Some(index) = job
            .snapshot
            .outputs
            .iter()
            .position(|output| output.state != ExportOutputState::Succeeded)
        {
            job.snapshot.outputs[index].state = ExportOutputState::Failed;
            job.snapshot.outputs[index].error = Some(error.to_string());
        }
        job.snapshot.state = ExportJobState::Failed;
        job.snapshot.error = Some(error.to_string());
        return;
    }

    for index in 0..job.targets.len() {
        if job.snapshot.outputs[index].state == ExportOutputState::Succeeded {
            continue;
        }
        if job.cancelled.load(Ordering::Relaxed) {
            job.snapshot.outputs[index].state = ExportOutputState::Cancelled;
            job.snapshot.state = ExportJobState::Cancelled;
            job.snapshot.error = Some("cancelled before composition pass".into());
            return;
        }
        let target = job.targets[index].clone();
        let output_path = target.output_path.clone();
        let composition_pass = index + 1;
        job.snapshot.outputs[index].state = ExportOutputState::Running;
        job.snapshot.current_progress = Some(ExportProgressSnapshot {
            composition_pass,
            stage: "preflight".into(),
            completed: 0,
            total: 1,
        });

        let result = render_and_publish(job, engine, index, &target, composition_pass);
        match result {
            Ok((mut summary, probe)) => {
                summary.out_path = output_path.clone();
                let output = &mut job.snapshot.outputs[index];
                output.state = ExportOutputState::Succeeded;
                output.summary = Some(summary);
                output.probe = Some(probe);
                output.error = None;
                job.snapshot.composition_passes_completed += 1;
                job.snapshot.current_progress = None;
            }
            Err(error) => {
                let cancelled =
                    matches!(error, CoreError::Cancelled) || job.cancelled.load(Ordering::Relaxed);
                let message = if cancelled {
                    None
                } else {
                    Some(error.to_string())
                };
                let output = &mut job.snapshot.outputs[index];
                output.state = if cancelled {
                    ExportOutputState::Cancelled
                } else {
                    ExportOutputState::Failed
                };
                output.error = message.clone();
                job.snapshot.state = if cancelled {
                    ExportJobState::Cancelled
                } else {
                    ExportJobState::Failed
                };
                job.snapshot.error = message;
                job.snapshot.current_progress = None;
                return;
            }
        }
    }
    job.snapshot.state = ExportJobState::Succeeded;
    job.snapshot.error = None;
    job.snapshot.current_progress = None;
}

fn render_and_publish(
    job: &mut ExportJob,
    engine: &mut Engine,
    index: usize,
    target: &ExportTarget,
    composition_pass: usize,
) -> Result<(VideoRenderSummary, ExportProbeReport), CoreError> {
    let output_path = &target.output_path;
    let parent = output_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty());
    if let Some(parent) = parent {
        fs::create_dir_all(parent)?;
    }
    let staging_path = staging_path(output_path, &job.snapshot.job_id, index);
    let mut stage_guard = StagingFileGuard::new(staging_path.clone());
    let progress = &mut job.snapshot.current_progress;
    let cancelled = job.cancelled.clone();
    let pinned_project = &job.pinned_project;
    let mut on_progress = |event: RenderProgress| {
        *progress = Some(ExportProgressSnapshot {
            composition_pass,
            stage: stage_name(event.stage).into(),
            completed: event.completed,
            total: event.total,
        });
    };
    let resolved_profile = resolve_target_encoding_profile(target)?;
    let expected_codec = resolved_profile
        .as_ref()
        .map_or(VideoCodec::H264, |p| p.codec);
    let render_options = RenderOptions {
        output_alpha_mode: frameshift_render::OutputAlphaMode::Straight,
        ..Default::default()
    };
    let render_settings = VideoRenderSettings {
        options: &render_options,
        encoding: &target.encoding,
        encoding_profile: resolved_profile.as_ref(),
        cancel: Some(cancelled.as_ref()),
    };
    let summary = engine.render_video_mp4_cancellable(
        pinned_project,
        target.profile,
        &staging_path,
        target.range,
        render_settings,
        &mut on_progress,
    )?;
    if cancelled.load(Ordering::Relaxed) {
        return Err(CoreError::Cancelled);
    }
    let probe_info = FfmpegBridge::probe_media_info(&staging_path)?;
    let expected_frames = summary.frame_count;
    let expected_audio = summary.has_audio;
    let verified = probe_matches_summary(
        &probe_info,
        &summary,
        expected_frames,
        expected_audio,
        expected_codec,
    );
    if !verified {
        return Err(CoreError::VerificationFailed(format!(
            "ffprobe output mismatch for {}: expected {}x{}, {} frames, audio={}, found {}x{}, {} frames, audio={}",
            output_path.display(),
            summary.width,
            summary.height,
            expected_frames,
            expected_audio,
            probe_info.width,
            probe_info.height,
            probe_info.pts_index.len(),
            probe_info.has_audio
        )));
    }
    let identity = FfmpegBridge::hash_file(&staging_path)?;
    publish_staged_output(&staging_path, output_path, &cancelled)?;
    stage_guard.disarm();

    Ok((summary, probe_report(probe_info, identity, verified)))
}

fn probe_matches_summary(
    probe: &MediaProbeInfo,
    summary: &VideoRenderSummary,
    expected_frames: usize,
    expected_audio: bool,
    expected_codec: VideoCodec,
) -> bool {
    probe.width == summary.width
        && probe.height == summary.height
        && probe.pts_index.len() == expected_frames
        && probe.has_audio == expected_audio
        && expected_video_codec_matches(expected_codec, probe.video_codec.as_deref())
        && (!expected_audio
            || expected_audio_codec_matches(expected_codec, probe.audio_codec.as_deref()))
        && probe.fps.is_none_or(|fps| fps == summary.fps)
}

fn expected_video_codec_matches(expected: VideoCodec, probed: Option<&str>) -> bool {
    let Some(codec) = probed else {
        return false;
    };
    match expected {
        VideoCodec::H264 => codec.eq_ignore_ascii_case("h264"),
        VideoCodec::Hevc => codec.eq_ignore_ascii_case("hevc") || codec.eq_ignore_ascii_case("h265"),
        VideoCodec::ProRes422Hq | VideoCodec::ProRes4444 => {
            codec.eq_ignore_ascii_case("prores") || codec.to_ascii_lowercase().starts_with("prores")
        }
    }
}

fn expected_audio_codec_matches(expected_video: VideoCodec, probed_audio: Option<&str>) -> bool {
    let Some(codec) = probed_audio else {
        return false;
    };
    if expected_video.is_prores() {
        codec.eq_ignore_ascii_case("pcm_s16le") || codec.to_ascii_lowercase().starts_with("pcm_")
    } else {
        codec.eq_ignore_ascii_case("aac")
    }
}

fn probe_report(
    probe: MediaProbeInfo,
    identity: FileContentIdentity,
    verified: bool,
) -> ExportProbeReport {
    ExportProbeReport {
        video_codec: probe.video_codec,
        audio_codec: probe.audio_codec,
        width: probe.width,
        height: probe.height,
        frame_count: probe.pts_index.len(),
        fps: probe.fps,
        has_audio: probe.has_audio,
        identity,
        verified,
    }
}

fn stage_name(stage: RenderStage) -> &'static str {
    match stage {
        RenderStage::Frames => "frames",
        RenderStage::AudioMix => "audio_mix",
        RenderStage::Encode => "encode",
    }
}

fn verify_pinned_snapshot(job: &ExportJob) -> Result<(), CoreError> {
    let project_identity = FfmpegBridge::hash_file(&job.snapshot.pinned_project_path)?;
    if project_identity.sha256 != job.snapshot.pinned_project_sha256 {
        return Err(CoreError::VerificationFailed(
            "pinned project snapshot changed after enqueue".into(),
        ));
    }
    for dependency in &job.snapshot.dependencies {
        let identity = FfmpegBridge::hash_file(&dependency.pinned_path)?;
        if identity != dependency.identity {
            return Err(CoreError::VerificationFailed(format!(
                "pinned {} dependency changed after enqueue: {}",
                dependency.kind,
                dependency.pinned_path.display()
            )));
        }
    }
    Ok(())
}

fn publish_staged_output(
    staging_path: &Path,
    output_path: &Path,
    cancelled: &AtomicBool,
) -> Result<(), CoreError> {
    if cancelled.load(Ordering::Relaxed) {
        return Err(CoreError::Cancelled);
    }
    // The staging file is in the destination directory, so rename is an atomic publish on
    // supported local filesystems. If rename fails, the previous destination remains intact.
    fs::rename(staging_path, output_path)?;
    Ok(())
}

struct StagingFileGuard {
    path: PathBuf,
    armed: bool,
}

impl StagingFileGuard {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for StagingFileGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn staging_path(output_path: &Path, job_id: &str, target_index: usize) -> PathBuf {
    let parent = output_path.parent().unwrap_or_else(|| Path::new("."));
    let stage_index = STAGE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let ext = output_path
        .extension()
        .and_then(|e| e.to_str())
        .filter(|e| !e.is_empty())
        .unwrap_or("mp4");
    parent.join(format!(
        ".frameshift-{job_id}-{target_index}-{stage_index}.{ext}"
    ))
}

fn next_job_id() -> ExportJobId {
    let seq = JOB_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("export-{}-{seq}", std::process::id())
}

fn pin_project_dependencies(
    project: &mut Project,
    project_dir: &Path,
    snapshot_dir: &Path,
    manifest: &mut Vec<ExportDependencyRecord>,
) -> Result<(), CoreError> {
    let deps_dir = snapshot_dir.join("dependencies");
    fs::create_dir_all(&deps_dir)?;

    for asset in &mut project.assets {
        let source_path = resolve_path(&asset.path, project_dir);
        if source_path.is_file() {
            let (pinned, identity) = copy_file_pinned(&source_path, &deps_dir)?;
            asset.path = pinned.to_string_lossy().into_owned();
            manifest.push(ExportDependencyRecord {
                kind: format!("asset_{:?}", asset.kind).to_ascii_lowercase(),
                source_path: source_path.to_string_lossy().into_owned(),
                pinned_path: pinned,
                identity,
            });
        } else if !is_synthetic_asset(&asset.id, asset.kind, &asset.path) {
            return Err(CoreError::MissingAssetFile {
                asset_id: asset.id.clone(),
                path: asset.path.clone(),
            });
        }
    }

    pin_graph_dependencies(&mut project.graph, project_dir, &deps_dir, manifest)?;
    Ok(())
}

fn pin_graph_dependencies(
    graph: &mut frameshift_schema::RenderGraphDef,
    project_dir: &Path,
    deps_dir: &Path,
    manifest: &mut Vec<ExportDependencyRecord>,
) -> Result<(), CoreError> {
    for node in &mut graph.nodes {
        match &mut node.kind {
            NodeKind::Lut3D {
                lut_source: Some(LutSourceDef::CubeFile { path, .. }),
                ..
            } => {
                *path = pin_explicit_path("lut", path, project_dir, deps_dir, manifest)?;
            }
            NodeKind::Lut3D {
                lut_source: Some(LutSourceDef::Ocio { config_path, .. }),
                ..
            } => {
                *config_path = pin_ocio_config(config_path, project_dir, deps_dir, manifest)?;
            }
            NodeKind::VectorText {
                font_family,
                font_path,
                font_weight,
                fallback_fonts,
                ..
            } => {
                if font_family.is_some() || font_path.is_some() || !fallback_fonts.is_empty() {
                    let query = FontQuery {
                        font_path: font_path.clone(),
                        font_family: font_family.clone(),
                        font_weight: *font_weight,
                    };
                    let (primary, fallbacks) =
                        resolve_font_chain(&query, fallback_fonts, Some(project_dir))?;
                    *font_path = Some(pin_resolved_font(&primary, deps_dir, manifest)?);
                    *fallback_fonts = fallbacks
                        .iter()
                        .map(|font| pin_resolved_font(font, deps_dir, manifest))
                        .collect::<Result<Vec<_>, _>>()?;
                }
            }
            NodeKind::Text3D {
                font_path,
                fallback_fonts,
                ..
            } => {
                let query = FontQuery {
                    font_path: Some(font_path.clone()),
                    font_family: None,
                    font_weight: 400,
                };
                let (primary, fallbacks) =
                    resolve_font_chain(&query, fallback_fonts, Some(project_dir))?;
                *font_path = pin_resolved_font(&primary, deps_dir, manifest)?;
                *fallback_fonts = fallbacks
                    .iter()
                    .map(|font| pin_resolved_font(font, deps_dir, manifest))
                    .collect::<Result<Vec<_>, _>>()?;
            }
            NodeKind::Precomp { sub_graph, .. } => {
                pin_graph_dependencies(sub_graph, project_dir, deps_dir, manifest)?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn pin_explicit_path(
    kind: &str,
    path: &str,
    project_dir: &Path,
    deps_dir: &Path,
    manifest: &mut Vec<ExportDependencyRecord>,
) -> Result<String, CoreError> {
    let source = resolve_path(path, project_dir);
    if !source.is_file() {
        return Err(CoreError::VerificationFailed(format!(
            "missing {kind} dependency '{}': {}",
            path,
            source.display()
        )));
    }
    let (pinned, identity) = copy_file_pinned(&source, deps_dir)?;
    manifest.push(ExportDependencyRecord {
        kind: kind.into(),
        source_path: source.to_string_lossy().into_owned(),
        pinned_path: pinned.clone(),
        identity,
    });
    Ok(pinned.to_string_lossy().into_owned())
}

fn pin_resolved_font(
    font: &ResolvedFont,
    deps_dir: &Path,
    manifest: &mut Vec<ExportDependencyRecord>,
) -> Result<String, CoreError> {
    let bytes = font.bytes.as_slice();
    let source_identity = FfmpegBridge::hash_bytes(bytes);
    let (pinned, identity) = write_bytes_pinned(bytes, "font", "ttf", deps_dir)?;
    manifest.push(ExportDependencyRecord {
        kind: "font".into(),
        source_path: format!("resolved-font:{}:{}", font.matched_family, font.face_index),
        pinned_path: pinned.clone(),
        identity: identity.clone(),
    });
    if identity != source_identity {
        return Err(CoreError::VerificationFailed(
            "font bytes changed while the export snapshot was written".into(),
        ));
    }
    Ok(pinned.to_string_lossy().into_owned())
}

fn pin_ocio_config(
    config_path: &str,
    project_dir: &Path,
    deps_dir: &Path,
    manifest: &mut Vec<ExportDependencyRecord>,
) -> Result<String, CoreError> {
    let source = resolve_path(config_path, project_dir);
    if !source.is_file() {
        return Err(CoreError::VerificationFailed(format!(
            "missing OCIO config dependency '{config_path}': {}",
            source.display()
        )));
    }
    let pipeline = OcioColorPipeline::from_file(&source)?;
    let config = pipeline.config();
    let mut text = fs::read_to_string(&source)?;
    let mut file_sources = Vec::new();
    for colorspace in config.colorspaces.values() {
        if let Some(transform) = &colorspace.to_reference {
            collect_ocio_file_sources(transform, config, &mut file_sources)?;
        }
        if let Some(transform) = &colorspace.from_reference {
            collect_ocio_file_sources(transform, config, &mut file_sources)?;
        }
    }
    file_sources.sort();
    file_sources.dedup();
    for (source_name, resolved_path) in file_sources {
        let (pinned, identity) = copy_file_pinned(&resolved_path, deps_dir)?;
        let replacement = pinned.to_string_lossy().into_owned();
        text = text.replace(&source_name, &replacement);
        manifest.push(ExportDependencyRecord {
            kind: "ocio_lut".into(),
            source_path: resolved_path.to_string_lossy().into_owned(),
            pinned_path: pinned,
            identity,
        });
    }
    let original_identity = FfmpegBridge::hash_file(&source)?;
    let mut config_path_pinned = deps_dir.join(format!("ocio-{}.ocio", original_identity.sha256));
    if config_path_pinned.exists() {
        config_path_pinned = deps_dir.join(format!(
            "ocio-{}-{}.ocio",
            original_identity.sha256,
            STAGE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
    }
    fs::write(&config_path_pinned, text.as_bytes())?;
    let identity = FfmpegBridge::hash_file(&config_path_pinned)?;
    manifest.push(ExportDependencyRecord {
        kind: "ocio_config".into(),
        source_path: source.to_string_lossy().into_owned(),
        pinned_path: config_path_pinned.clone(),
        identity,
    });
    Ok(config_path_pinned.to_string_lossy().into_owned())
}

fn collect_ocio_file_sources(
    transform: &OcioTransform,
    config: &frameshift_render::OcioConfig,
    files: &mut Vec<(String, PathBuf)>,
) -> Result<(), CoreError> {
    match transform {
        OcioTransform::File { src, .. } => files.push((src.clone(), config.resolve_lut_path(src)?)),
        OcioTransform::Group { children, .. } => {
            for child in children {
                collect_ocio_file_sources(child, config, files)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn copy_file_pinned(
    source: &Path,
    deps_dir: &Path,
) -> Result<(PathBuf, FileContentIdentity), CoreError> {
    fs::create_dir_all(deps_dir)?;
    let stage = deps_dir.join(format!(
        ".incoming-{}",
        STAGE_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    fs::copy(source, &stage)?;
    let identity = FfmpegBridge::hash_file(&stage)?;
    let extension = source
        .extension()
        .and_then(|v| v.to_str())
        .filter(|v| !v.is_empty())
        .unwrap_or("bin");
    let pinned = deps_dir.join(format!("{}.{}", identity.sha256, extension));
    if pinned.exists() {
        fs::remove_file(&stage)?;
    } else if let Err(error) = fs::rename(&stage, &pinned) {
        let _ = fs::remove_file(&stage);
        return Err(error.into());
    }
    Ok((pinned, identity))
}

fn write_bytes_pinned(
    bytes: &[u8],
    _kind: &str,
    extension: &str,
    deps_dir: &Path,
) -> Result<(PathBuf, FileContentIdentity), CoreError> {
    fs::create_dir_all(deps_dir)?;
    let identity = FfmpegBridge::hash_bytes(bytes);
    let pinned = deps_dir.join(format!("{}.{}", identity.sha256, extension));
    if !pinned.exists() {
        let stage = deps_dir.join(format!(
            ".incoming-{}",
            STAGE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(&stage, bytes)?;
        if let Err(error) = fs::rename(&stage, &pinned) {
            let _ = fs::remove_file(&stage);
            return Err(error.into());
        }
    }
    Ok((pinned, identity))
}

fn resolve_path(raw: &str, project_dir: &Path) -> PathBuf {
    let path = Path::new(raw);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    let project_relative = project_dir.join(path);
    if project_relative.is_file() {
        project_relative
    } else if path.is_file() {
        path.to_path_buf()
    } else {
        project_relative
    }
}

fn is_synthetic_asset(id: &str, kind: AssetKind, raw_path: &str) -> bool {
    let id = id.to_ascii_lowercase();
    let path = raw_path.replace('\\', "/").to_ascii_lowercase();
    match kind {
        AssetKind::Audio => {
            id == "asset_bgm"
                || id.contains("beat_track")
                || path == "local_media/beat_track.wav"
                || path.starts_with("synthetic://beat")
        }
        AssetKind::Video | AssetKind::Image => {
            id == "asset_clip_a"
                || id == "asset_clip_b"
                || id == "asset_mask"
                || id == "asset_moving_mask"
                || id.contains("moving_mask")
                || path == "local_media/clip_a.mp4"
                || path == "local_media/clip_b.mp4"
                || path == "local_media/moving_mask.mp4"
                || path.starts_with("synthetic://clip_a")
                || path.starts_with("synthetic://clip_b")
                || path.starts_with("synthetic://moving_mask")
        }
    }
}

fn collect_shader_identities(project: &Project) -> Vec<String> {
    let mut identities = Vec::new();
    collect_graph_shader_identities(&project.graph, &mut identities);
    identities.sort();
    identities.dedup();
    identities
}

fn collect_graph_shader_identities(
    graph: &frameshift_schema::RenderGraphDef,
    identities: &mut Vec<String>,
) {
    for node in &graph.nodes {
        match &node.kind {
            NodeKind::ShaderTransition { shader_name, .. } => identities.push(format!(
                "builtin-transition:{}@frameshift-render/{}",
                shader_name,
                env!("CARGO_PKG_VERSION")
            )),
            NodeKind::CustomWgsl { wgsl_source, .. } => {
                let hash = FfmpegBridge::hash_bytes(wgsl_source.as_bytes()).sha256;
                identities.push(format!("custom-wgsl:{}:{hash}", node.id));
            }
            NodeKind::Precomp { sub_graph, .. } => {
                collect_graph_shader_identities(sub_graph, identities);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use frameshift_schema::Project;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "frameshift-export-{label}-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("temp dir");
        path
    }

    #[test]
    fn enqueue_pins_revision_and_dependency_bytes() {
        let root = temp_dir("pin");
        let media = root.join("clip.bin");
        fs::write(&media, b"asset revision one").expect("write source");
        let mut project = Project::golden_scene_preset();
        project.assets[0].path = "clip.bin".into();
        let mut queue = ExportQueue::default();
        let ticket = queue
            .enqueue(
                &project,
                &root,
                vec![ExportTarget::new("output.mp4", RenderProfileKind::Draft)],
            )
            .expect("enqueue");
        let snapshot = queue.get(&ticket.job_id).expect("job snapshot");
        assert_eq!(snapshot.revision_id, project.revision_id);
        assert_eq!(snapshot.dependencies.len(), 1);
        let pinned = &snapshot.dependencies[0].pinned_path;
        fs::write(&media, b"asset revision two").expect("mutate source");
        assert_eq!(
            fs::read(pinned).expect("read pinned"),
            b"asset revision one"
        );

        project.revision_id += 1;
        let job = queue.jobs.get(&ticket.job_id).expect("stored job");
        assert_eq!(job.pinned_project.revision_id, snapshot.revision_id);
        assert_eq!(
            job.snapshot.pinned_project_sha256,
            snapshot.pinned_project_sha256
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cancelled_publish_preserves_existing_destination_and_cleans_stage() {
        let root = temp_dir("cancel");
        let destination = root.join("existing.mp4");
        let staged = root.join("staged.mp4");
        fs::write(&destination, b"old output").expect("old output");
        fs::write(&staged, b"new output").expect("staged output");
        let cancelled = AtomicBool::new(true);
        {
            let _guard = StagingFileGuard::new(staged.clone());
            assert!(matches!(
                publish_staged_output(&staged, &destination, &cancelled),
                Err(CoreError::Cancelled)
            ));
        }
        assert_eq!(fs::read(&destination).expect("destination"), b"old output");
        assert!(!staged.exists(), "cancelled staging file must be removed");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn pinned_dependency_mutation_is_detected_before_retry() {
        let root = temp_dir("mismatch");
        let media = root.join("clip.bin");
        fs::write(&media, b"stable bytes").expect("source");
        let mut project = Project::golden_scene_preset();
        project.assets[0].path = "clip.bin".into();
        let mut queue = ExportQueue::default();
        let ticket = queue
            .enqueue(
                &project,
                &root,
                vec![ExportTarget::new("output.mp4", RenderProfileKind::Draft)],
            )
            .expect("enqueue");
        let job = queue.jobs.get(&ticket.job_id).expect("stored job");
        fs::write(&job.snapshot.dependencies[0].pinned_path, b"tampered bytes")
            .expect("mutate snapshot");
        assert!(matches!(
            verify_pinned_snapshot(job),
            Err(CoreError::VerificationFailed(_))
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn export_target_platform_presets_and_probe_codec_matching() {
        use frameshift_time::PtsIndex;

        // Backward-compatible deserialization without platform_preset.
        let legacy: ExportTarget = serde_json::from_str(
            r#"{"output_path":"out.mp4","profile":"final"}"#,
        )
        .expect("legacy ExportTarget deserializes");
        assert_eq!(legacy.platform_preset, None);
        assert!(resolve_target_encoding_profile(&legacy).unwrap().is_none());

        // Valid platform presets resolve to expected VideoCodec.
        let mut tiktok = ExportTarget::new("tiktok.mp4", RenderProfileKind::Final);
        tiktok.platform_preset = Some("tiktok_1080p60".into());
        assert_eq!(
            resolve_target_encoding_profile(&tiktok).unwrap().unwrap().codec,
            VideoCodec::H264
        );

        let mut hevc = ExportTarget::new("anime.mp4", RenderProfileKind::Final);
        hevc.platform_preset = Some("anime_10bit_hevc".into());
        assert_eq!(
            resolve_target_encoding_profile(&hevc).unwrap().unwrap().codec,
            VideoCodec::Hevc
        );

        let mut prores = ExportTarget::new("master.mov", RenderProfileKind::Final);
        prores.platform_preset = Some("prores_422hq".into());
        assert_eq!(
            resolve_target_encoding_profile(&prores).unwrap().unwrap().codec,
            VideoCodec::ProRes422Hq
        );

        // Unknown platform preset and mismatched container extension are rejected cleanly.
        let mut bad_preset = ExportTarget::new("out.mp4", RenderProfileKind::Final);
        bad_preset.platform_preset = Some("unknown_platform".into());
        assert!(resolve_target_encoding_profile(&bad_preset).is_err());

        let project = Project::golden_scene_preset();
        let mut queue = ExportQueue::default();
        let mut bad_ext = ExportTarget::new("master.mp4", RenderProfileKind::Final);
        bad_ext.platform_preset = Some("prores_422hq".into());
        assert!(queue.enqueue(&project, Path::new("."), vec![bad_ext]).is_err());

        // Flexible probe matching across H264, HEVC, and ProRes + PCM.
        let summary = VideoRenderSummary {
            out_path: PathBuf::from("test.mp4"),
            profile: RenderProfileKind::Final,
            width: 1920,
            height: 1080,
            frame_count: 1,
            fps: project.timeline.fps,
            duration_ticks: project.timeline.duration_ticks,
            has_audio: true,
            range: None,
            frame_hashes: vec![1],
            cache_hits: 0,
            cache_misses: 0,
            cache_evictions: 0,
        };
        let pts = PtsIndex::new(vec![frameshift_time::Flicks::ZERO]).unwrap();
        let hevc_probe = MediaProbeInfo {
            video_codec: Some("hevc".into()),
            audio_codec: Some("aac".into()),
            pixel_format: Some("yuv420p10le".into()),
            width: 1920,
            height: 1080,
            fps: Some(project.timeline.fps),
            duration_ticks: Some(project.timeline.duration_ticks),
            pts_index: pts.clone(),
            has_audio: true,
            audio_sample_rate: Some(48_000),
            audio_channels: Some(2),
        };
        assert!(probe_matches_summary(&hevc_probe, &summary, 1, true, VideoCodec::Hevc));
        assert!(!probe_matches_summary(&hevc_probe, &summary, 1, true, VideoCodec::H264));

        let prores_probe = MediaProbeInfo {
            video_codec: Some("prores".into()),
            audio_codec: Some("pcm_s16le".into()),
            pixel_format: Some("yuv422p10le".into()),
            ..hevc_probe
        };
        assert!(probe_matches_summary(
            &prores_probe,
            &summary,
            1,
            true,
            VideoCodec::ProRes422Hq
        ));
    }
}
