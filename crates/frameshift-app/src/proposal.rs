//! Headless agent edit receipts and revision-bound review notes.
//!
//! These types deliberately sit beside `ProjectSession`: desktop and MCP callers
//! can share the same checked `ProjectCommand` path without keeping a second edit
//! model or timeline state.

use crate::controller::OptimisticLockGuard;
use crate::error::AppError;
use frameshift_schema::{
    CreativeDecisionRecord, GraphNodeDef, NodeKind, Project, ProjectCommand, ProjectSession,
    RenderGraphDef,
};
use frameshift_time::Flicks;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Actor metadata recorded with an automatically applied agent edit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentActor {
    pub agent_id: String,
}

/// Half-open timeline span `[start_tick, end_tick)` used by receipts and review notes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TickRange {
    pub start_tick: Flicks,
    pub end_tick: Flicks,
}

impl TickRange {
    /// Creates a half-open range, returning `None` when the range is empty or reversed.
    pub fn new(start_tick: Flicks, end_tick: Flicks) -> Option<Self> {
        (start_tick < end_tick).then_some(Self {
            start_tick,
            end_tick,
        })
    }

    fn point(tick: Flicks) -> Self {
        Self {
            start_tick: tick,
            end_tick: Flicks(tick.0.saturating_add(1)),
        }
    }
}

/// A semantic event whose output anchor changed in a committed command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MovedEvent {
    pub event_id: String,
    pub from_tick: Flicks,
    pub to_tick: Flicks,
}

/// Minimal project structure shown in a headless before/after receipt preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectPreviewSnapshot {
    pub revision_id: u64,
    pub project_name: String,
    pub fps_num: u32,
    pub fps_den: u32,
    pub duration_ticks: Flicks,
    pub width: u32,
    pub height: u32,
    pub clip_ranges: Vec<NamedTickRange>,
    pub audio_clip_ranges: Vec<NamedTickRange>,
    pub event_positions: Vec<EventPosition>,
    pub graph_node_ids: Vec<String>,
    pub creative_decision_ids: Vec<String>,
}

/// A named half-open timeline range in a structural preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamedTickRange {
    pub id: String,
    pub range: TickRange,
}

/// Event target position shown in a structural preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventPosition {
    pub event_id: String,
    pub target_output_tick: Flicks,
}

/// Truthful status of a requested visual or audio preview artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PreviewArtifactState {
    NotGenerated {
        reason: String,
    },
    Generated {
        before_path: std::path::PathBuf,
        after_path: std::path::PathBuf,
    },
}

/// Preview evidence attached to each receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiptPreview {
    pub before: ProjectPreviewSnapshot,
    pub after: ProjectPreviewSnapshot,
    pub visual_artifact: PreviewArtifactState,
    pub audio_artifact: PreviewArtifactState,
}

/// Automatically generated audit receipt for one committed agent command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentEditReceipt {
    pub receipt_id: String,
    pub intent: String,
    pub actor: AgentActor,
    /// Stable snake-case name of the applied `ProjectCommand` variant.
    pub action: String,
    pub before_revision_id: u64,
    pub after_revision_id: u64,
    pub affected_entity_ids: Vec<String>,
    pub affected_ranges: Vec<TickRange>,
    pub moved_events: Vec<MovedEvent>,
    /// Existing decisions that are byte-for-byte semantically unchanged after the edit.
    pub preserved_creative_decisions: Vec<CreativeDecisionRecord>,
    pub preview: ReceiptPreview,
}

/// Applies one command against the requested revision and returns its edit receipt.
///
/// The session remains the only source of project truth. A successful call invokes
/// `ProjectSession::apply_command` exactly once, so the complete agent edit is one undo step.
pub fn apply_agent_command(
    session: &mut ProjectSession,
    expected_revision_id: u64,
    actor: AgentActor,
    intent: impl Into<String>,
    command: ProjectCommand,
) -> Result<AgentEditReceipt, AppError> {
    OptimisticLockGuard::new(expected_revision_id).verify(session.revision_id())?;

    let before = session.project().clone();
    let action = command_action(&command);
    let after_revision_id = session.apply_command(expected_revision_id, command)?;
    let after = session.project();

    let moved_events = moved_events(&before, after);
    let mut affected_ranges = changed_clip_ranges(&before, after);
    affected_ranges.extend(changed_audio_ranges(&before, after));
    affected_ranges.extend(changed_event_ranges(&before, after));
    affected_ranges = merge_ranges(affected_ranges);

    let preserved_creative_decisions = preserved_decisions(&before, after);
    let affected_entity_ids = changed_entity_ids(&before, after);
    let preview = ReceiptPreview {
        before: preview_snapshot(&before),
        after: preview_snapshot(after),
        visual_artifact: PreviewArtifactState::NotGenerated {
            reason: "apply_agent_command received no preview tick or artifact destination".into(),
        },
        audio_artifact: PreviewArtifactState::NotGenerated {
            reason: "apply_agent_command received no audio preview range or artifact destination"
                .into(),
        },
    };

    Ok(AgentEditReceipt {
        receipt_id: format!("agent-edit-{}-{}", before.revision_id, after_revision_id),
        intent: intent.into(),
        actor,
        action,
        before_revision_id: before.revision_id,
        after_revision_id,
        affected_entity_ids,
        affected_ranges,
        moved_events,
        preserved_creative_decisions,
        preview,
    })
}

fn command_action(command: &ProjectCommand) -> String {
    serde_json::to_value(command)
        .ok()
        .and_then(|value| value.get("command")?.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown_command".into())
}

fn preview_snapshot(project: &Project) -> ProjectPreviewSnapshot {
    let mut graph_node_ids = Vec::new();
    collect_node_ids(&project.graph, &mut graph_node_ids);
    graph_node_ids.sort();
    ProjectPreviewSnapshot {
        revision_id: project.revision_id,
        project_name: project.name.clone(),
        fps_num: project.timeline.fps.num,
        fps_den: project.timeline.fps.den,
        duration_ticks: project.timeline.duration_ticks,
        width: project.timeline.resolution.width,
        height: project.timeline.resolution.height,
        clip_ranges: project
            .timeline
            .clips
            .iter()
            .map(|clip| NamedTickRange {
                id: clip.id.clone(),
                range: TickRange {
                    start_tick: clip.timeline_start,
                    end_tick: clip.timeline_end,
                },
            })
            .collect(),
        audio_clip_ranges: project
            .timeline
            .audio_clips
            .iter()
            .map(|clip| NamedTickRange {
                id: clip.id.clone(),
                range: TickRange {
                    start_tick: clip.timeline_start,
                    end_tick: clip.timeline_end,
                },
            })
            .collect(),
        event_positions: project
            .events
            .iter()
            .map(|event| EventPosition {
                event_id: event.event_id.clone(),
                target_output_tick: event.target_output_tick,
            })
            .collect(),
        graph_node_ids,
        creative_decision_ids: project
            .creative_decisions
            .iter()
            .map(|decision| decision.decision_id.clone())
            .collect(),
    }
}

fn collect_node_ids(graph: &RenderGraphDef, out: &mut Vec<String>) {
    for node in &graph.nodes {
        out.push(node.id.0.clone());
        if let NodeKind::Precomp { sub_graph, .. } = &node.kind {
            collect_node_ids(sub_graph, out);
        }
    }
}

fn moved_events(before: &Project, after: &Project) -> Vec<MovedEvent> {
    let old: BTreeMap<_, _> = before
        .events
        .iter()
        .map(|event| (event.event_id.as_str(), event.target_output_tick))
        .collect();
    let mut moved = after
        .events
        .iter()
        .filter_map(|event| {
            old.get(event.event_id.as_str())
                .filter(|old_tick| **old_tick != event.target_output_tick)
                .map(|old_tick| MovedEvent {
                    event_id: event.event_id.clone(),
                    from_tick: *old_tick,
                    to_tick: event.target_output_tick,
                })
        })
        .collect::<Vec<_>>();
    moved.sort_by(|a, b| a.event_id.cmp(&b.event_id));
    moved
}

fn changed_clip_ranges(before: &Project, after: &Project) -> Vec<TickRange> {
    changed_spans(&before.timeline.clips, &after.timeline.clips, |clip| {
        (&clip.id, clip.timeline_start, clip.timeline_end)
    })
}

fn changed_audio_ranges(before: &Project, after: &Project) -> Vec<TickRange> {
    changed_spans(
        &before.timeline.audio_clips,
        &after.timeline.audio_clips,
        |clip| (&clip.id, clip.timeline_start, clip.timeline_end),
    )
}

fn changed_spans<T: PartialEq>(
    before: &[T],
    after: &[T],
    fields: impl Fn(&T) -> (&String, Flicks, Flicks),
) -> Vec<TickRange> {
    let old: BTreeMap<_, _> = before.iter().map(|item| (fields(item).0, item)).collect();
    let new: BTreeMap<_, _> = after.iter().map(|item| (fields(item).0, item)).collect();
    let ids: BTreeSet<_> = old.keys().chain(new.keys()).copied().collect();
    let mut ranges = Vec::new();
    for id in ids {
        if old.get(id) != new.get(id) {
            for item in [old.get(id), new.get(id)].into_iter().flatten() {
                let (_, start, end) = fields(item);
                if let Some(range) = TickRange::new(start, end) {
                    ranges.push(range);
                }
            }
        }
    }
    ranges
}

fn changed_event_ranges(before: &Project, after: &Project) -> Vec<TickRange> {
    let old: BTreeMap<_, _> = before
        .events
        .iter()
        .map(|event| (event.event_id.as_str(), event))
        .collect();
    let new: BTreeMap<_, _> = after
        .events
        .iter()
        .map(|event| (event.event_id.as_str(), event))
        .collect();
    let ids: BTreeSet<_> = old.keys().chain(new.keys()).copied().collect();
    let mut ranges = Vec::new();
    for id in ids {
        if old.get(id) != new.get(id) {
            for event in [old.get(id), new.get(id)].into_iter().flatten() {
                ranges.push(TickRange::point(event.target_output_tick));
            }
        }
    }
    ranges
}

fn merge_ranges(mut ranges: Vec<TickRange>) -> Vec<TickRange> {
    ranges.sort_by_key(|range| (range.start_tick, range.end_tick));
    let mut merged: Vec<TickRange> = Vec::new();
    for range in ranges {
        if let Some(last) = merged.last_mut()
            && range.start_tick <= last.end_tick
        {
            last.end_tick = last.end_tick.max(range.end_tick);
        } else {
            merged.push(range);
        }
    }
    merged
}

fn preserved_decisions(before: &Project, after: &Project) -> Vec<CreativeDecisionRecord> {
    let new: BTreeMap<_, _> = after
        .creative_decisions
        .iter()
        .map(|decision| (decision.decision_id.as_str(), decision))
        .collect();
    let mut preserved = before
        .creative_decisions
        .iter()
        .filter(|decision| {
            new.get(decision.decision_id.as_str())
                .is_some_and(|new_decision| *new_decision == *decision)
        })
        .cloned()
        .collect::<Vec<_>>();
    preserved.sort_by(|a, b| a.decision_id.cmp(&b.decision_id));
    preserved
}

fn changed_entity_ids(before: &Project, after: &Project) -> Vec<String> {
    let mut ids = Vec::new();
    changed_id_list(
        &before.assets,
        &after.assets,
        |asset| format!("asset:{}", asset.id),
        &mut ids,
    );
    changed_id_list(
        &before.timeline.clips,
        &after.timeline.clips,
        |clip| format!("clip:{}", clip.id),
        &mut ids,
    );
    changed_id_list(
        &before.timeline.audio_clips,
        &after.timeline.audio_clips,
        |clip| format!("audio_clip:{}", clip.id),
        &mut ids,
    );
    changed_id_list(
        &before.events,
        &after.events,
        |event| format!("event:{}", event.event_id),
        &mut ids,
    );
    let old_nodes = flattened_nodes(&before.graph);
    let new_nodes = flattened_nodes(&after.graph);
    changed_id_list(
        &old_nodes,
        &new_nodes,
        |node| format!("node:{}", node.id.0),
        &mut ids,
    );
    changed_id_list(
        &before.creative_decisions,
        &after.creative_decisions,
        |decision| format!("creative_decision:{}", decision.decision_id),
        &mut ids,
    );
    changed_id_list(
        &before.preferences,
        &after.preferences,
        |preference| {
            format!(
                "preference:{:?}:{}:{}",
                preference.scope,
                preference.target_id.as_deref().unwrap_or(""),
                preference.key
            )
        },
        &mut ids,
    );
    changed_id_list(
        &before.param_bindings,
        &after.param_bindings,
        |binding| format!("binding:{}:{}", binding.node_id.0, binding.param_name),
        &mut ids,
    );
    if before.name != after.name {
        ids.push("project:name".into());
    }
    if before.schema_version != after.schema_version {
        ids.push("project:schema_version".into());
    }
    if before.timeline.fps != after.timeline.fps {
        ids.push("timeline:fps".into());
    }
    if before.timeline.duration_ticks != after.timeline.duration_ticks {
        ids.push("timeline:duration".into());
    }
    if before.timeline.resolution != after.timeline.resolution {
        ids.push("timeline:resolution".into());
    }
    if before.graph.output_node != after.graph.output_node {
        ids.push("graph:output".into());
    }
    if before.render_profiles != after.render_profiles {
        ids.push("project:render_profiles".into());
    }
    if before.proxy_config != after.proxy_config {
        ids.push("project:proxy_config".into());
    }
    if before.hw_accel_policy != after.hw_accel_policy {
        ids.push("project:hw_accel_policy".into());
    }
    ids.sort();
    ids.dedup();
    ids
}

fn changed_id_list<T: PartialEq>(
    before: &[T],
    after: &[T],
    key: impl Fn(&T) -> String,
    out: &mut Vec<String>,
) {
    let old: BTreeMap<_, _> = before.iter().map(|item| (key(item), item)).collect();
    let new: BTreeMap<_, _> = after.iter().map(|item| (key(item), item)).collect();
    let ids: BTreeSet<_> = old.keys().chain(new.keys()).cloned().collect();
    out.extend(
        ids.into_iter()
            .filter(|id| old.get(id) != new.get(id))
            .collect::<Vec<_>>(),
    );
}

fn flattened_nodes(graph: &RenderGraphDef) -> Vec<GraphNodeDef> {
    fn visit(graph: &RenderGraphDef, out: &mut Vec<GraphNodeDef>) {
        for node in &graph.nodes {
            out.push(node.clone());
            if let NodeKind::Precomp { sub_graph, .. } = &node.kind {
                visit(sub_graph, out);
            }
        }
    }
    let mut nodes = Vec::new();
    visit(graph, &mut nodes);
    nodes
}

/// Input for adding a revision-bound note to the in-memory review surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewReviewNote {
    pub expected_revision_id: u64,
    pub author: String,
    pub body: String,
    #[serde(default)]
    pub range: Option<TickRange>,
}

/// A review note is permanently pinned to the revision where it was written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewNote {
    pub note_id: String,
    pub revision_id: u64,
    pub author: String,
    pub body: String,
    pub range: Option<TickRange>,
}

/// Freshness reported when a review note is read against the active project revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ReviewNoteFreshness {
    Current,
    Stale { current_revision_id: u64 },
}

/// Review note paired with its current/stale status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewNoteView {
    pub note: ReviewNote,
    pub freshness: ReviewNoteFreshness,
}

/// In-memory store for review notes, suitable for an MCP or desktop headless host.
#[derive(Debug, Clone, Default)]
pub struct ReviewNoteStore {
    next_id: u64,
    notes: Vec<ReviewNote>,
}

impl ReviewNoteStore {
    /// Adds a note only if its expected revision is still active.
    pub fn add(
        &mut self,
        session: &ProjectSession,
        input: NewReviewNote,
    ) -> Result<ReviewNoteView, AppError> {
        OptimisticLockGuard::new(input.expected_revision_id).verify(session.revision_id())?;
        if input.body.trim().is_empty() {
            return Err(AppError::InvalidOperation(
                "review note body must not be empty".into(),
            ));
        }
        if let Some(range) = input.range
            && (range.start_tick < Flicks::ZERO
                || range.end_tick <= range.start_tick
                || range.end_tick > session.project().timeline.duration_ticks)
        {
            return Err(AppError::InvalidOperation(format!(
                "review note range [{}, {}) must be inside [0, {})",
                range.start_tick.0,
                range.end_tick.0,
                session.project().timeline.duration_ticks.0
            )));
        }
        self.next_id += 1;
        let note = ReviewNote {
            note_id: format!("review-note-{}", self.next_id),
            revision_id: session.revision_id(),
            author: input.author,
            body: input.body,
            range: input.range,
        };
        self.notes.push(note.clone());
        Ok(ReviewNoteView {
            note,
            freshness: ReviewNoteFreshness::Current,
        })
    }

    /// Returns every note with status evaluated against `current_revision_id`.
    pub fn list(&self, current_revision_id: u64) -> Vec<ReviewNoteView> {
        self.notes
            .iter()
            .cloned()
            .map(|note| ReviewNoteView {
                freshness: if note.revision_id == current_revision_id {
                    ReviewNoteFreshness::Current
                } else {
                    ReviewNoteFreshness::Stale {
                        current_revision_id,
                    }
                },
                note,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use frameshift_schema::Project;

    #[test]
    fn agent_receipt_tracks_moved_event_preservation_preview_and_one_undo() {
        let mut session = ProjectSession::new(Project::golden_scene_preset()).unwrap();
        let before_revision = session.revision_id();
        let event = session.project().events[0].clone();
        let decision_id = session.project().creative_decisions[0].decision_id.clone();
        let target = Flicks(event.target_output_tick.0 + 20);

        let receipt = apply_agent_command(
            &mut session,
            before_revision,
            AgentActor {
                agent_id: "test-agent".into(),
            },
            "align impact to the music drop",
            ProjectCommand::MoveEventTarget {
                event_id: event.event_id.clone(),
                new_target_output_tick: target,
            },
        )
        .unwrap();

        assert_eq!(receipt.before_revision_id, before_revision);
        assert_eq!(receipt.after_revision_id, before_revision + 1);
        assert_eq!(receipt.action, "move_event_target");
        assert_eq!(receipt.moved_events.len(), 1);
        assert_eq!(receipt.moved_events[0].event_id, event.event_id);
        assert_eq!(
            receipt
                .preserved_creative_decisions
                .iter()
                .map(|decision| decision.decision_id.clone())
                .collect::<Vec<_>>(),
            vec![decision_id]
        );
        assert!(!receipt.affected_ranges.is_empty());
        assert!(matches!(
            receipt.preview.visual_artifact,
            PreviewArtifactState::NotGenerated { .. }
        ));
        assert_eq!(session.undo_len(), 1);

        let undone_revision = session.undo(before_revision + 1).unwrap();
        assert_eq!(undone_revision, before_revision + 2);
        assert_eq!(
            session
                .project()
                .event(&event.event_id)
                .unwrap()
                .target_output_tick,
            event.target_output_tick
        );
    }

    #[test]
    fn stale_agent_edit_is_rejected_without_a_commit() {
        let mut session = ProjectSession::new(Project::golden_scene_preset()).unwrap();
        let initial_revision = session.revision_id();
        session
            .apply_command(
                initial_revision,
                ProjectCommand::SetProjectName { name: "new".into() },
            )
            .unwrap();
        let err = apply_agent_command(
            &mut session,
            initial_revision,
            AgentActor {
                agent_id: "test-agent".into(),
            },
            "stale edit",
            ProjectCommand::SetProjectName {
                name: "stale".into(),
            },
        )
        .unwrap_err();
        assert!(matches!(
            err,
            AppError::ConcurrencyConflict {
                expected,
                actual
            }
            if expected == initial_revision && actual == initial_revision + 1
        ));
        assert_eq!(session.project().name, "new");
        assert_eq!(session.undo_len(), 1);
    }

    #[test]
    fn review_notes_are_revision_bound_and_become_stale_after_an_edit() {
        let mut session = ProjectSession::new(Project::golden_scene_preset()).unwrap();
        let initial_revision = session.revision_id();
        let mut store = ReviewNoteStore::default();
        let note = store
            .add(
                &session,
                NewReviewNote {
                    expected_revision_id: initial_revision,
                    author: "reviewer".into(),
                    body: "Check the impact frame".into(),
                    range: Some(TickRange::new(Flicks(10), Flicks(20)).unwrap()),
                },
            )
            .unwrap();
        assert_eq!(note.freshness, ReviewNoteFreshness::Current);

        session
            .apply_command(
                initial_revision,
                ProjectCommand::SetProjectName { name: "new".into() },
            )
            .unwrap();
        assert_eq!(
            store.list(session.revision_id())[0].freshness,
            ReviewNoteFreshness::Stale {
                current_revision_id: initial_revision + 1
            }
        );
    }

    #[test]
    fn review_note_rejects_wrong_revision_and_out_of_bounds_range() {
        let session = ProjectSession::new(Project::golden_scene_preset()).unwrap();
        let current_revision = session.revision_id();
        let mut store = ReviewNoteStore::default();
        let stale = store.add(
            &session,
            NewReviewNote {
                expected_revision_id: current_revision + 1,
                author: "reviewer".into(),
                body: "stale".into(),
                range: None,
            },
        );
        assert!(matches!(stale, Err(AppError::ConcurrencyConflict { .. })));
        let invalid = store.add(
            &session,
            NewReviewNote {
                expected_revision_id: current_revision,
                author: "reviewer".into(),
                body: "out of range".into(),
                range: Some(TickRange {
                    start_tick: Flicks(10),
                    end_tick: Flicks(session.project().timeline.duration_ticks.0 + 1),
                }),
            },
        );
        assert!(matches!(invalid, Err(AppError::InvalidOperation(_))));
    }
}
