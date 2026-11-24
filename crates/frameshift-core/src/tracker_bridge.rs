//! P5 Tracker -> `Project.keyframe_tracks`, NLE interchange (`.otio` / FCPXML),
//! and AI model manifest bridge helpers exposed cleanly for CLI and MCP surfaces.

use crate::engine::{Engine, resolve_disk_audio_envelope, select_profile};
use crate::error::CoreError;
use crate::resolver::ProjectFrameProvider;
use frameshift_media::MultibandBeatReport;
pub use frameshift_media::{
    AiDiscoveredModel, AiManifestRunConfig, AiManifestRunReport, AiManifestTask, AiModelManifest,
    discover_ai_models, load_ai_model_manifest, run_manifest_model, run_manifest_model_by_id,
    validate_manifest_and_resolve_onnx, verify_manifest_sha256,
};
use frameshift_render::{
    ProjectAnimator, TrackPlanarOptions, TrackPointOptions, TrackerBindTarget,
    planar_track_to_keyframes, point_track_to_keyframes, track_planar_sequence,
    track_point_sequence,
};
pub use frameshift_schema::nle_bridge::{
    NleExportOptions, NleImportOptions, NleMarker, NleMarkerKind, export_project_to_fcpxml,
    export_project_to_otio_json, fcpxml_time_to_flicks, flicks_to_fcpxml_time,
    flicks_to_otio_rational_time, import_project_from_fcpxml, import_project_from_otio_json,
    otio_rational_time_to_flicks,
};
use frameshift_schema::{
    Camera3DDef, KeyframeTrackDef, NodeId, NodeKind, Project, ProjectCommand, ProjectSession,
    RenderProfileKind, SchemaError,
};
use frameshift_time::Flicks;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Options for [`Engine::track_point_to_project`].
#[derive(Debug, Clone)]
pub struct PointTrackProjectOptions {
    pub source_node_id: NodeId,
    pub target_node_id: NodeId,
    pub bind_target: TrackerBindTarget,
    pub track_id_prefix: String,
    pub start_tick: Flicks,
    pub end_tick: Flicks,
    pub start_point_uv: [f32; 2],
    pub point_options: TrackPointOptions,
    pub profile: RenderProfileKind,
}

/// Options for [`Engine::track_planar_to_project`].
#[derive(Debug, Clone)]
pub struct PlanarTrackProjectOptions {
    pub source_node_id: NodeId,
    pub target_node_id: NodeId,
    pub bind_target: TrackerBindTarget,
    pub track_id_prefix: String,
    pub start_tick: Flicks,
    pub end_tick: Flicks,
    pub start_corners_uv: [[f32; 2]; 4],
    pub planar_options: TrackPlanarOptions,
    pub profile: RenderProfileKind,
}

/// Summary report emitted after tracking and writing [`KeyframeTrackDef`]s into a [`Project`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackerProjectReport {
    pub frames_tracked: usize,
    pub lost_ticks: Vec<Flicks>,
    pub mean_fb_error_px: f32,
    pub min_confidence: f32,
    pub generated_tracks: Vec<KeyframeTrackDef>,
    pub updated_revision_id: u64,
}

impl Engine {
    /// Tracks 1 or 2 points across `[start_tick, end_tick)` on `source_node_id`, converts the
    /// result into [`KeyframeTrackDef`]s targeting `target_node_id`, and persists them into
    /// `project.keyframe_tracks` via [`ProjectCommand::UpsertKeyframeTrack`].
    pub fn track_point_to_project(
        &mut self,
        project: &mut Project,
        opts: &PointTrackProjectOptions,
    ) -> Result<TrackerProjectReport, CoreError> {
        project.validate()?;
        if project.graph.node(&opts.source_node_id).is_none() {
            return Err(SchemaError::MissingNode(opts.source_node_id.clone()).into());
        }
        if project.graph.find_node_recursive(&opts.target_node_id).is_none() {
            return Err(SchemaError::MissingNode(opts.target_node_id.clone()).into());
        }
        let frame_ticks = project.timeline.fps.frame_duration()?;
        let ticks = collect_range_ticks(
            opts.start_tick,
            opts.end_tick,
            project.timeline.duration_ticks,
            frame_ticks,
        )?;
        let profile_def = select_profile(project, opts.profile).clone();
        let width = profile_def.resolution.width;
        let height = profile_def.resolution.height;

        let track_result = {
            let (compositor, resolver, cache, audio_cache) = self.split_parts_mut();
            let audio_envelope = resolve_disk_audio_envelope(audio_cache, resolver, project)?;
            let provider = ProjectFrameProvider::new(project, resolver, cache);
            let animator = ProjectAnimator::with_audio_envelope(project, audio_envelope)?;
            track_point_sequence(
                compositor,
                &project.graph,
                &opts.source_node_id,
                &ticks,
                frame_ticks,
                width,
                height,
                &provider,
                &animator,
                opts.start_point_uv,
                &opts.point_options,
            )?
        };

        let camera = find_scene_camera(project);
        let generated_tracks = point_track_to_keyframes(
            &opts.track_id_prefix,
            &opts.target_node_id,
            opts.bind_target,
            &track_result,
            width,
            height,
            camera.as_ref(),
        );
        let updated_revision_id = apply_keyframe_tracks(project, &generated_tracks)?;

        Ok(TrackerProjectReport {
            frames_tracked: track_result.samples.len(),
            lost_ticks: track_result.lost_ticks,
            mean_fb_error_px: track_result.mean_fb_error_px,
            min_confidence: track_result.min_confidence,
            generated_tracks,
            updated_revision_id,
        })
    }

    /// Tracks a 4-corner planar quad across `[start_tick, end_tick)` on `source_node_id`,
    /// converts it into [`KeyframeTrackDef`]s targeting `target_node_id`, and persists them
    /// into `project.keyframe_tracks` via [`ProjectCommand::UpsertKeyframeTrack`].
    pub fn track_planar_to_project(
        &mut self,
        project: &mut Project,
        opts: &PlanarTrackProjectOptions,
    ) -> Result<TrackerProjectReport, CoreError> {
        project.validate()?;
        if project.graph.node(&opts.source_node_id).is_none() {
            return Err(SchemaError::MissingNode(opts.source_node_id.clone()).into());
        }
        if project.graph.find_node_recursive(&opts.target_node_id).is_none() {
            return Err(SchemaError::MissingNode(opts.target_node_id.clone()).into());
        }
        let frame_ticks = project.timeline.fps.frame_duration()?;
        let ticks = collect_range_ticks(
            opts.start_tick,
            opts.end_tick,
            project.timeline.duration_ticks,
            frame_ticks,
        )?;
        let profile_def = select_profile(project, opts.profile).clone();
        let width = profile_def.resolution.width;
        let height = profile_def.resolution.height;

        let track_result = {
            let (compositor, resolver, cache, audio_cache) = self.split_parts_mut();
            let audio_envelope = resolve_disk_audio_envelope(audio_cache, resolver, project)?;
            let provider = ProjectFrameProvider::new(project, resolver, cache);
            let animator = ProjectAnimator::with_audio_envelope(project, audio_envelope)?;
            track_planar_sequence(
                compositor,
                &project.graph,
                &opts.source_node_id,
                &ticks,
                frame_ticks,
                width,
                height,
                &provider,
                &animator,
                opts.start_corners_uv,
                &opts.planar_options,
            )?
        };

        let camera = find_scene_camera(project);
        let generated_tracks = planar_track_to_keyframes(
            &opts.track_id_prefix,
            &opts.target_node_id,
            opts.bind_target,
            &track_result,
            width,
            height,
            camera.as_ref(),
        );
        let updated_revision_id = apply_keyframe_tracks(project, &generated_tracks)?;

        Ok(TrackerProjectReport {
            frames_tracked: track_result.samples.len(),
            lost_ticks: track_result.lost_ticks,
            mean_fb_error_px: track_result.mean_fb_error_px,
            min_confidence: track_result.min_confidence,
            generated_tracks,
            updated_revision_id,
        })
    }
}

fn collect_range_ticks(
    start_tick: Flicks,
    end_tick: Flicks,
    timeline_duration: Flicks,
    frame_ticks: Flicks,
) -> Result<Vec<Flicks>, CoreError> {
    if frame_ticks.0 <= 0 {
        return Err(CoreError::InvalidArgument(
            "frame_ticks must be positive".into(),
        ));
    }
    let start = start_tick.0.max(0);
    let end = end_tick.0.min(timeline_duration.0);
    if end <= start {
        return Err(CoreError::InvalidArgument(format!(
            "tracker range [{start}, {end}) must be non-empty"
        )));
    }
    let mut ticks = Vec::new();
    let mut t = start;
    while t < end {
        ticks.push(Flicks(t));
        t = t.saturating_add(frame_ticks.0);
    }
    Ok(ticks)
}

fn find_scene_camera(project: &Project) -> Option<Camera3DDef> {
    project.graph.nodes.iter().find_map(|n| match &n.kind {
        NodeKind::Scene3D { camera, .. } => Some(camera.clone()),
        _ => None,
    })
}

fn apply_keyframe_tracks(
    project: &mut Project,
    tracks: &[KeyframeTrackDef],
) -> Result<u64, CoreError> {
    let mut session = ProjectSession::new(project.clone())?;
    for track in tracks {
        let rev = session.revision_id();
        session.apply_command(rev, ProjectCommand::UpsertKeyframeTrack(track.clone()))?;
    }
    let rev = session.project().revision_id;
    *project = session.project().clone();
    Ok(rev)
}

/// Converts a [`MultibandBeatReport`] into NLE markers (`Downbeat` + `Beat`).
pub fn nle_markers_from_beats(report: &MultibandBeatReport, frame_ticks: Flicks) -> Vec<NleMarker> {
    let mut markers = Vec::new();
    for (idx, &tick) in report.downbeats.iter().enumerate() {
        markers.push(NleMarker {
            label: format!("Downbeat {}", idx + 1),
            start_tick: tick,
            duration_ticks: frame_ticks.max(Flicks(1)),
            kind: NleMarkerKind::Downbeat,
        });
    }
    for (idx, &tick) in report.beat_grid.iter().enumerate() {
        if report.downbeats.contains(&tick) {
            continue;
        }
        markers.push(NleMarker {
            label: format!("Beat {}", idx + 1),
            start_tick: tick,
            duration_ticks: frame_ticks.max(Flicks(1)),
            kind: NleMarkerKind::Beat,
        });
    }
    markers
}

/// Exports `project` to an OpenTimelineIO (`.otio`) JSON file on disk.
pub fn export_project_otio_to_file(
    project: &Project,
    out_path: &Path,
    opts: &NleExportOptions,
) -> Result<(), CoreError> {
    let json = export_project_to_otio_json(project, opts)?;
    if let Some(parent) = out_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out_path, json)?;
    Ok(())
}

/// Exports `project` to a Final Cut Pro XML (`FCPXML 1.11`) file on disk.
pub fn export_project_fcpxml_to_file(
    project: &Project,
    out_path: &Path,
    opts: &NleExportOptions,
) -> Result<(), CoreError> {
    let xml = export_project_to_fcpxml(project, opts)?;
    if let Some(parent) = out_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out_path, xml)?;
    Ok(())
}

/// Imports an OpenTimelineIO (`.otio`) JSON file into a validated FrameShift [`Project`].
pub fn import_project_otio_from_file(
    in_path: &Path,
    opts: &NleImportOptions,
) -> Result<Project, CoreError> {
    let json = std::fs::read_to_string(in_path)?;
    Ok(import_project_from_otio_json(&json, opts)?)
}

/// Imports an `FCPXML` file into a validated FrameShift [`Project`].
pub fn import_project_fcpxml_from_file(
    in_path: &Path,
    opts: &NleImportOptions,
) -> Result<Project, CoreError> {
    let xml = std::fs::read_to_string(in_path)?;
    Ok(import_project_from_fcpxml(&xml, opts)?)
}
