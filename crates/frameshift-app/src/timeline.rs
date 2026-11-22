//! Headless timeline projection and editing helpers.
//!
//! These types keep timeline display, snapping, and preview coordination usable
//! by both desktop and CLI callers without storing project truth in a UI layer.

use crate::error::AppError;
use frameshift_schema::{AudioFadeDef, ClipDef, EventOrigin, Project};
use frameshift_time::{Flicks, Rational32, SpeedCurve};
use serde::{Deserialize, Serialize};

/// Read-only projection of project timeline data for headless consumers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimelineProjection {
    pub revision_id: u64,
    pub fps: Rational32,
    pub duration_ticks: Flicks,
    pub video_clips: Vec<TimelineClipProjection>,
    pub audio_clips: Vec<TimelineAudioClipProjection>,
    pub events: Vec<TimelineEventProjection>,
}

impl TimelineProjection {
    /// Builds a deterministic, display-ready timeline view from a project.
    ///
    /// Video and audio clips are ordered by start tick and then ID. Events are
    /// ordered by target tick and then event ID, so callers get stable results
    /// regardless of the order in which project records were authored.
    pub fn from_project(project: &Project) -> Self {
        let mut video_clips: Vec<_> = project
            .timeline
            .clips
            .iter()
            .map(TimelineClipProjection::from)
            .collect();
        video_clips.sort_by(|a, b| {
            a.timeline_start
                .cmp(&b.timeline_start)
                .then_with(|| a.clip_id.cmp(&b.clip_id))
        });

        let mut audio_clips: Vec<_> = project
            .timeline
            .audio_clips
            .iter()
            .map(|clip| TimelineAudioClipProjection {
                clip_id: clip.id.clone(),
                asset_id: clip.asset_id.clone(),
                timeline_start: clip.timeline_start,
                timeline_end: clip.timeline_end,
                source_start: clip.source_start,
                gain_db: clip.gain_db,
                fade_in: clip.fade_in,
                fade_out: clip.fade_out,
            })
            .collect();
        audio_clips.sort_by(|a, b| {
            a.timeline_start
                .cmp(&b.timeline_start)
                .then_with(|| a.clip_id.cmp(&b.clip_id))
        });

        let mut events: Vec<_> = project
            .events
            .iter()
            .map(|event| TimelineEventProjection {
                event_id: event.event_id.clone(),
                label: event.label.clone(),
                source_clip_id: event.source_clip_id.clone(),
                source_anchor_tick: event.source_anchor_tick,
                target_output_tick: event.target_output_tick,
                origin: event.origin,
            })
            .collect();
        events.sort_by(|a, b| {
            a.target_output_tick
                .cmp(&b.target_output_tick)
                .then_with(|| a.event_id.cmp(&b.event_id))
        });

        Self {
            revision_id: project.revision_id,
            fps: project.timeline.fps,
            duration_ticks: project.timeline.duration_ticks,
            video_clips,
            audio_clips,
            events,
        }
    }

    /// Returns the event marker ticks in projection order.
    pub fn event_ticks(&self) -> Vec<Flicks> {
        self.events
            .iter()
            .map(|event| event.target_output_tick)
            .collect()
    }
}

/// One video clip row in a [`TimelineProjection`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimelineClipProjection {
    pub clip_id: String,
    pub asset_id: String,
    pub timeline_start: Flicks,
    pub timeline_end: Flicks,
    pub source_start: Flicks,
    pub speed_curve: SpeedCurve,
    pub linked_event_id: Option<String>,
}

impl From<&ClipDef> for TimelineClipProjection {
    fn from(clip: &ClipDef) -> Self {
        Self {
            clip_id: clip.id.clone(),
            asset_id: clip.asset_id.clone(),
            timeline_start: clip.timeline_start,
            timeline_end: clip.timeline_end,
            source_start: clip.source_start,
            speed_curve: clip.speed_curve.clone(),
            linked_event_id: clip.linked_event_id.clone(),
        }
    }
}

impl TimelineClipProjection {
    /// Maps an output tick within the clip (including its end boundary) to a
    /// source tick using the same integrated speed curve as the render engine.
    pub fn source_tick_at(&self, output_tick: Flicks) -> Option<Flicks> {
        if output_tick < self.timeline_start || output_tick > self.timeline_end {
            return None;
        }
        let offset = output_tick.checked_sub(self.timeline_start).ok()?;
        self.source_start
            .checked_add(self.speed_curve.source_offset_at(offset))
            .ok()
    }
}

/// One audio clip row in a [`TimelineProjection`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimelineAudioClipProjection {
    pub clip_id: String,
    pub asset_id: String,
    pub timeline_start: Flicks,
    pub timeline_end: Flicks,
    pub source_start: Flicks,
    pub gain_db: f32,
    pub fade_in: Option<AudioFadeDef>,
    pub fade_out: Option<AudioFadeDef>,
}

/// One event marker in a [`TimelineProjection`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelineEventProjection {
    pub event_id: String,
    pub label: String,
    pub source_clip_id: Option<String>,
    pub source_anchor_tick: Option<Flicks>,
    pub target_output_tick: Flicks,
    pub origin: EventOrigin,
}

/// A source/output sample suitable for plotting a clip's speed graph.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SpeedGraphPoint {
    pub output_tick: Flicks,
    pub source_tick: Flicks,
    pub speed: f64,
}

/// Samples a clip's integrated source-time map and instantaneous speed curve.
///
/// The first and last samples sit at the clip's output boundaries. At least two
/// samples are required so the returned graph spans the complete clip.
pub fn source_output_speed_graph(
    clip: &ClipDef,
    sample_count: usize,
) -> Result<Vec<SpeedGraphPoint>, AppError> {
    if sample_count < 2 {
        return Err(AppError::InvalidOperation(
            "speed graph requires at least two samples".into(),
        ));
    }
    let duration = clip.timeline_end.checked_sub(clip.timeline_start)?.as_i64();
    if duration <= 0 {
        return Err(AppError::InvalidOperation(format!(
            "speed graph clip '{}' must have a positive duration",
            clip.id
        )));
    }
    let segments = (sample_count - 1) as i128;
    let mut points = Vec::with_capacity(sample_count);
    for index in 0..sample_count {
        let offset = if index + 1 == sample_count {
            duration
        } else {
            ((duration as i128) * (index as i128) / segments) as i64
        };
        let output_tick = clip.timeline_start.checked_add(Flicks(offset))?;
        let source_tick = clip
            .source_start
            .checked_add(clip.speed_curve.source_offset_at(Flicks(offset)))?;
        points.push(SpeedGraphPoint {
            output_tick,
            source_tick,
            speed: clip.speed_curve.instant_speed_at(Flicks(offset)),
        });
    }
    Ok(points)
}

/// Kind of timeline location selected by snapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapKind {
    Frame,
    Beat,
    Event,
}

/// Result of snapping a requested tick to the nearest timeline landmark.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapResult {
    pub requested_tick: Flicks,
    pub tick: Flicks,
    /// `None` means no landmark was within tolerance and the input was retained.
    pub snapped_to: Option<SnapKind>,
}

/// Snaps to the nearest frame, beat, or event when it falls within tolerance.
///
/// Equal-distance ties prefer event markers, then beat markers, then frames.
/// For duplicate beat/event locations, the caller's first matching tick wins.
pub fn snap_tick(
    tick: Flicks,
    fps: Rational32,
    beat_ticks: &[Flicks],
    event_ticks: &[Flicks],
    tolerance_ticks: Flicks,
) -> Result<SnapResult, AppError> {
    if tolerance_ticks < Flicks::ZERO {
        return Err(AppError::InvalidOperation(
            "snap tolerance must not be negative".into(),
        ));
    }

    let frame_index = fps.ticks_to_frame_nearest(tick)?;
    let frame_tick = fps.frame_to_ticks(frame_index)?;
    let mut best: Option<(Flicks, SnapKind, i128, u8)> = None;
    consider_snap_candidate(&mut best, tick, frame_tick, SnapKind::Frame, 2);
    for &beat_tick in beat_ticks {
        consider_snap_candidate(&mut best, tick, beat_tick, SnapKind::Beat, 1);
    }
    for &event_tick in event_ticks {
        consider_snap_candidate(&mut best, tick, event_tick, SnapKind::Event, 0);
    }

    match best {
        Some((snapped_tick, kind, distance, _)) if distance <= tolerance_ticks.as_i64() as i128 => {
            Ok(SnapResult {
                requested_tick: tick,
                tick: snapped_tick,
                snapped_to: Some(kind),
            })
        }
        _ => Ok(SnapResult {
            requested_tick: tick,
            tick,
            snapped_to: None,
        }),
    }
}

fn consider_snap_candidate(
    best: &mut Option<(Flicks, SnapKind, i128, u8)>,
    requested: Flicks,
    candidate: Flicks,
    kind: SnapKind,
    priority: u8,
) {
    let distance = (candidate.as_i64() as i128 - requested.as_i64() as i128).abs();
    let replace = match best {
        None => true,
        Some((_, _, best_distance, best_priority)) => {
            distance < *best_distance || (distance == *best_distance && priority < *best_priority)
        }
    };
    if replace {
        *best = Some((candidate, kind, distance, priority));
    }
}

/// Identity for one asynchronous preview render request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewTicket {
    pub revision_id: u64,
    pub output_tick: Flicks,
    pub request_id: u64,
}

/// Last-request-wins gate for asynchronous preview results.
///
/// A completed render is current only if it is still the newest ticket and its
/// revision/playhead still match the caller's active project state.
#[derive(Debug, Clone, Default)]
pub struct PreviewRequestGate {
    request_id: u64,
    current: Option<PreviewTicket>,
}

impl PreviewRequestGate {
    /// Starts a new request and makes every earlier ticket stale.
    pub fn begin(&mut self, revision_id: u64, output_tick: Flicks) -> PreviewTicket {
        self.request_id = self.request_id.wrapping_add(1);
        let ticket = PreviewTicket {
            revision_id,
            output_tick,
            request_id: self.request_id,
        };
        self.current = Some(ticket);
        ticket
    }

    /// Invalidates any in-flight requests, e.g. after a project or playhead change.
    pub fn invalidate(&mut self) {
        self.current = None;
    }

    /// Returns whether a completion may update the visible preview.
    pub fn accepts(
        &self,
        ticket: PreviewTicket,
        active_revision_id: u64,
        active_output_tick: Flicks,
    ) -> bool {
        self.current == Some(ticket)
            && ticket.revision_id == active_revision_id
            && ticket.output_tick == active_output_tick
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use frameshift_schema::Project;
    use frameshift_time::{FLICKS_PER_SECOND, FPS_60};

    #[test]
    fn projection_is_stable_and_maps_source_ticks_with_engine_curve() {
        let project = Project::golden_scene_preset();
        let projection = TimelineProjection::from_project(&project);
        assert_eq!(projection.revision_id, project.revision_id);
        assert_eq!(projection.video_clips[0].clip_id, "clip_a");
        assert_eq!(projection.events[0].event_id, "drop_impact");
        let clip = &projection.video_clips[0];
        let mapped = clip
            .source_tick_at(Flicks(FLICKS_PER_SECOND))
            .expect("tick is within clip");
        assert_eq!(
            mapped,
            clip.source_start + clip.speed_curve.source_offset_at(Flicks(FLICKS_PER_SECOND))
        );
    }

    #[test]
    fn snap_prefers_event_then_beat_then_frame_on_ties() {
        let frame_tick = Flicks(FLICKS_PER_SECOND / 60);
        let requested = Flicks(frame_tick.as_i64() / 2);
        let snapped = snap_tick(
            requested,
            FPS_60,
            &[Flicks::ZERO],
            &[frame_tick],
            Flicks(frame_tick.as_i64()),
        )
        .expect("valid snap request");
        assert_eq!(snapped.tick, frame_tick);
        assert_eq!(snapped.snapped_to, Some(SnapKind::Event));

        let outside = snap_tick(requested, FPS_60, &[], &[], Flicks::ZERO)
            .expect("valid zero-tolerance snap");
        assert_eq!(outside.tick, requested);
        assert_eq!(outside.snapped_to, None);
    }

    #[test]
    fn preview_gate_discards_superseded_revision_and_playhead_results() {
        let mut gate = PreviewRequestGate::default();
        let old = gate.begin(4, Flicks(100));
        let latest = gate.begin(4, Flicks(200));
        assert!(!gate.accepts(old, 4, Flicks(100)));
        assert!(!gate.accepts(latest, 5, Flicks(200)));
        assert!(!gate.accepts(latest, 4, Flicks(100)));
        assert!(gate.accepts(latest, 4, Flicks(200)));
        gate.invalidate();
        assert!(!gate.accepts(latest, 4, Flicks(200)));
    }

    #[test]
    fn speed_graph_spans_clip_and_uses_integrated_source_mapping() {
        let clip = Project::golden_scene_preset().timeline.clips[0].clone();
        let points = source_output_speed_graph(&clip, 3).expect("sample graph");
        assert_eq!(points.len(), 3);
        assert_eq!(points[0].output_tick, clip.timeline_start);
        assert_eq!(points[2].output_tick, clip.timeline_end);
        assert_eq!(
            points[1].source_tick,
            clip.source_start
                + clip
                    .speed_curve
                    .source_offset_at(points[1].output_tick - clip.timeline_start)
        );
    }
}
