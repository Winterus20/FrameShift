use crate::error::AppError;
use crate::snapshot::render_desktop_ui_snapshot;
use frameshift_media::Rgba8Frame;
use frameshift_schema::{
    GraphNodeDef, NodeId, Project, ProjectCommand, ProjectSession, RenderProfileKind,
};
use frameshift_time::{Flicks, ImpactCurveStyle, SourceFitMode};
use serde::{Deserialize, Serialize};

/// Optimistic-locking guard protecting human timeline edits from stale concurrent agent mutations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OptimisticLockGuard {
    pub expected_revision_id: u64,
}

impl OptimisticLockGuard {
    /// Creates a guard pinned to `expected_revision_id`.
    #[inline]
    pub const fn new(expected_revision_id: u64) -> Self {
        Self {
            expected_revision_id,
        }
    }

    /// Verifies that `actual_revision_id` still matches `self.expected_revision_id`.
    #[inline]
    pub fn verify(&self, actual_revision_id: u64) -> Result<(), AppError> {
        if self.expected_revision_id != actual_revision_id {
            return Err(AppError::ConcurrencyConflict {
                expected: self.expected_revision_id,
                actual: actual_revision_id,
            });
        }
        Ok(())
    }
}

/// Origin of a workspace mutation recorded in `WorkspaceUiController::activity_log`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActorKind {
    Human,
    Agent { agent_id: String },
}

/// Audit entry capturing a committed UI/agent action against the active `ProjectSession`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiActivityEntry {
    pub revision_id: u64,
    pub actor: ActorKind,
    pub action: String,
    pub summary: String,
    pub playhead_tick: Flicks,
}

/// Discrete human timeline clip edit routed through `WorkspaceUiController::human_split_or_trim_clip`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ClipEditAction {
    /// Trims `timeline_start` and/or `timeline_end` of an existing clip.
    Trim {
        new_timeline_start: Option<Flicks>,
        new_timeline_end: Option<Flicks>,
    },
    /// Splits an existing clip at `split_tick` into `[start, split_tick)` and `[split_tick, end)`.
    Split {
        split_tick: Flicks,
        right_clip_id: String,
    },
}

/// Desktop workspace controller with zero hidden UI state (ADR-0001).
///
/// All persistent project truth (timeline clips, event anchors, render graph DAG,
/// creative decisions, preferences, and undo/redo history) lives inside `ProjectSession`.
/// Only ephemeral view state (`playhead_tick`, selection, active preview profile,
/// and the activity feed) lives on the controller.
#[derive(Debug, Clone)]
pub struct WorkspaceUiController {
    session: ProjectSession,
    pub playhead_tick: Flicks,
    pub selected_node: Option<NodeId>,
    pub selected_clip: Option<String>,
    pub active_profile: RenderProfileKind,
    pub activity_log: Vec<UiActivityEntry>,
}

impl WorkspaceUiController {
    /// Wraps an existing validated `ProjectSession`.
    pub fn new(session: ProjectSession) -> Self {
        let selected_node = Some(session.project().graph.output_node.clone());
        let selected_clip = session
            .project()
            .timeline
            .clips
            .first()
            .map(|c| c.id.clone());
        Self {
            session,
            playhead_tick: Flicks::ZERO,
            selected_node,
            selected_clip,
            active_profile: RenderProfileKind::Draft,
            activity_log: Vec::new(),
        }
    }

    /// Validates `project` into a new `ProjectSession` and wraps it in a controller.
    pub fn from_project(project: Project) -> Result<Self, AppError> {
        let session = ProjectSession::new(project)?;
        Ok(Self::new(session))
    }

    /// Returns the underlying `ProjectSession`.
    #[inline]
    pub fn session(&self) -> &ProjectSession {
        &self.session
    }

    /// Returns an immutable reference to the committed `Project`.
    #[inline]
    pub fn project(&self) -> &Project {
        self.session.project()
    }

    /// Returns the current monotonic project `revision_id`.
    #[inline]
    pub fn revision_id(&self) -> u64 {
        self.session.revision_id()
    }

    /// Returns an `OptimisticLockGuard` pinned to the current `revision_id`.
    #[inline]
    pub fn optimistic_guard(&self) -> OptimisticLockGuard {
        OptimisticLockGuard::new(self.session.revision_id())
    }

    /// Scrubs the UI playhead within `[0, timeline.duration_ticks]`. Does not mutate project revision.
    pub fn scrub_to(&mut self, tick: Flicks) -> Flicks {
        let clamped = tick.clamp(Flicks::ZERO, self.session.project().timeline.duration_ticks);
        self.playhead_tick = clamped;
        clamped
    }

    /// Updates the selected render graph node after verifying it exists in the active graph.
    pub fn select_node(&mut self, node_id: Option<NodeId>) -> Result<(), AppError> {
        if let Some(ref id) = node_id
            && self.session.project().graph.node(id).is_none()
        {
            return Err(AppError::InvalidOperation(format!(
                "cannot select missing graph node '{id}'"
            )));
        }
        self.selected_node = node_id;
        Ok(())
    }

    /// Updates the selected timeline clip after verifying it exists in the active timeline.
    pub fn select_clip(&mut self, clip_id: Option<String>) -> Result<(), AppError> {
        if let Some(ref id) = clip_id
            && self.session.project().clip(id).is_none()
        {
            return Err(AppError::InvalidOperation(format!(
                "cannot select missing timeline clip '{id}'"
            )));
        }
        self.selected_clip = clip_id;
        Ok(())
    }

    /// Switches the active preview render profile (`Draft` or `Final`).
    pub fn set_active_profile(&mut self, profile: RenderProfileKind) {
        self.active_profile = profile;
    }

    /// Applies an arbitrary `ProjectCommand` on behalf of the human editor.
    pub fn human_apply_command(
        &mut self,
        action: impl Into<String>,
        summary: impl Into<String>,
        cmd: ProjectCommand,
    ) -> Result<u64, AppError> {
        let expected = self.session.revision_id();
        let new_rev = self.session.apply_command(expected, cmd)?;
        self.reconcile_selection();
        self.activity_log.push(UiActivityEntry {
            revision_id: new_rev,
            actor: ActorKind::Human,
            action: action.into(),
            summary: summary.into(),
            playhead_tick: self.playhead_tick,
        });
        Ok(new_rev)
    }

    /// Trims or splits a clip on the timeline through `ProjectSession::apply_command`.
    pub fn human_split_or_trim_clip(
        &mut self,
        clip_id: &str,
        action: ClipEditAction,
    ) -> Result<u64, AppError> {
        let (summary, command) = match action {
            ClipEditAction::Trim {
                new_timeline_start,
                new_timeline_end,
            } => (
                format!("Trimmed {clip_id}"),
                ProjectCommand::TrimClip {
                    clip_id: clip_id.to_owned(),
                    new_timeline_start,
                    new_timeline_end,
                },
            ),
            ClipEditAction::Split {
                split_tick,
                right_clip_id,
            } => (
                format!(
                    "Split {clip_id} at {} into {clip_id} + {right_clip_id}",
                    split_tick.0
                ),
                ProjectCommand::SplitClip {
                    clip_id: clip_id.to_owned(),
                    split_tick,
                    right_clip_id,
                },
            ),
        };

        self.human_apply_command("split_or_trim_clip", summary, command)
    }

    /// Moves a semantic `event_id` anchor's `target_output_tick` via `ProjectSession::apply_command`.
    pub fn human_move_event_anchor(
        &mut self,
        event_id: &str,
        new_target_output_tick: Flicks,
    ) -> Result<u64, AppError> {
        self.human_apply_command(
            "move_event_anchor",
            format!("Moved event '{event_id}' to {}", new_target_output_tick.0),
            ProjectCommand::MoveEventTarget {
                event_id: event_id.to_owned(),
                new_target_output_tick,
            },
        )
    }

    /// Retargets a semantic event, solving its linked source clip when bounds
    /// and a source anchor are present. Source/speed bounds and fit policy are
    /// explicit to keep the edit deterministic for headless and desktop users.
    #[allow(clippy::too_many_arguments)]
    pub fn human_retarget_event(
        &mut self,
        event_id: &str,
        new_target_output_tick: Flicks,
        source_bounds_start: Flicks,
        source_bounds_end: Flicks,
        min_speed: f64,
        max_speed: f64,
        curve_style: ImpactCurveStyle,
        source_fit: SourceFitMode,
    ) -> Result<u64, AppError> {
        self.human_apply_command(
            "retarget_event",
            format!(
                "Retargeted event '{event_id}' to {}",
                new_target_output_tick.0
            ),
            ProjectCommand::RetargetEvent {
                event_id: event_id.to_owned(),
                new_target_output_tick,
                source_bounds_start,
                source_bounds_end,
                min_speed,
                max_speed,
                curve_style,
                source_fit,
            },
        )
    }

    /// Updates or inserts a render graph node via `ProjectSession::apply_command`.
    pub fn human_update_node(&mut self, node: GraphNodeDef) -> Result<u64, AppError> {
        let node_id = node.id.clone();
        self.human_apply_command(
            "update_node",
            format!("Updated node '{node_id}'"),
            ProjectCommand::UpsertGraphNode(node),
        )
    }

    /// Undoes the latest committed command through `ProjectSession::undo`.
    pub fn undo(&mut self) -> Result<u64, AppError> {
        let rev = self.session.revision_id();
        let new_rev = self.session.undo(rev)?;
        self.reconcile_selection();
        self.activity_log.push(UiActivityEntry {
            revision_id: new_rev,
            actor: ActorKind::Human,
            action: "undo".into(),
            summary: format!("Undo to revision #{new_rev}"),
            playhead_tick: self.playhead_tick,
        });
        Ok(new_rev)
    }

    /// Reapplies the most recently undone command through `ProjectSession::redo`.
    pub fn redo(&mut self) -> Result<u64, AppError> {
        let rev = self.session.revision_id();
        let new_rev = self.session.redo(rev)?;
        self.reconcile_selection();
        self.activity_log.push(UiActivityEntry {
            revision_id: new_rev,
            actor: ActorKind::Human,
            action: "redo".into(),
            summary: format!("Redo to revision #{new_rev}"),
            playhead_tick: self.playhead_tick,
        });
        Ok(new_rev)
    }

    /// Applies an agent-submitted `ProjectCommand` guarded by `OptimisticLockGuard`.
    ///
    /// If the human editor modified the timeline or graph after the agent read
    /// `expected_revision_id`, this immediately rejects with
    /// `Err(AppError::ConcurrencyConflict { expected, actual })` without touching `session`.
    pub fn apply_agent_command(
        &mut self,
        expected_revision_id: u64,
        agent_id: impl Into<String>,
        cmd: ProjectCommand,
    ) -> Result<u64, AppError> {
        let guard = OptimisticLockGuard::new(expected_revision_id);
        guard.verify(self.session.revision_id())?;

        let agent_id = agent_id.into();
        let summary = describe_command(&cmd);
        let new_rev = self.session.apply_command(expected_revision_id, cmd)?;
        self.reconcile_selection();
        self.activity_log.push(UiActivityEntry {
            revision_id: new_rev,
            actor: ActorKind::Agent {
                agent_id: agent_id.clone(),
            },
            action: "agent_command".into(),
            summary: format!("[{agent_id}] {summary}"),
            playhead_tick: self.playhead_tick,
        });
        Ok(new_rev)
    }

    /// Renders a visual dark-mode desktop editor UI snapshot embedding `viewport_frame`.
    pub fn render_desktop_ui_snapshot(
        &self,
        viewport_frame: &Rgba8Frame,
        width: u32,
        height: u32,
    ) -> Rgba8Frame {
        render_desktop_ui_snapshot(self, viewport_frame, width, height)
    }

    fn reconcile_selection(&mut self) {
        let project = self.session.project();
        if let Some(ref node_id) = self.selected_node
            && project.graph.node(node_id).is_none()
        {
            self.selected_node = Some(project.graph.output_node.clone());
        }
        if let Some(ref clip_id) = self.selected_clip
            && project.clip(clip_id).is_none()
        {
            self.selected_clip = project.timeline.clips.first().map(|c| c.id.clone());
        }
        if self.playhead_tick > project.timeline.duration_ticks {
            self.playhead_tick = project.timeline.duration_ticks;
        }
    }
}

fn describe_command(cmd: &ProjectCommand) -> String {
    match cmd {
        ProjectCommand::SetProjectName { name } => format!("SetProjectName({name})"),
        ProjectCommand::AddAsset(a) => format!("AddAsset({})", a.id),
        ProjectCommand::RemoveAsset { id } => format!("RemoveAsset({id})"),
        ProjectCommand::AddClip(c) => format!("AddClip({})", c.id),
        ProjectCommand::RemoveClip { id } => format!("RemoveClip({id})"),
        ProjectCommand::TrimClip { clip_id, .. } => format!("TrimClip({clip_id})"),
        ProjectCommand::SplitClip {
            clip_id,
            right_clip_id,
            ..
        } => format!("SplitClip({clip_id} -> {right_clip_id})"),
        ProjectCommand::AuthorHardCuts { cuts } => format!("AuthorHardCuts({})", cuts.len()),
        ProjectCommand::SetClipSpeedCurve { clip_id, .. } => {
            format!("SetClipSpeedCurve({clip_id})")
        }
        ProjectCommand::UpsertEventAnchor(e) => format!("UpsertEventAnchor({})", e.event_id),
        ProjectCommand::MoveEventTarget {
            event_id,
            new_target_output_tick,
        } => format!(
            "MoveEventTarget({event_id} -> {})",
            new_target_output_tick.0
        ),
        ProjectCommand::RetargetEvent {
            event_id,
            new_target_output_tick,
            ..
        } => format!("RetargetEvent({event_id} -> {})", new_target_output_tick.0),
        ProjectCommand::Precompose {
            precomp_node_id, ..
        } => format!("Precompose({precomp_node_id})"),
        ProjectCommand::Unprecompose { precomp_node_id } => {
            format!("Unprecompose({precomp_node_id})")
        }
        ProjectCommand::UpsertGraphNode(n) => format!("UpsertGraphNode({})", n.id),
        ProjectCommand::RemoveGraphNode { node_id } => format!("RemoveGraphNode({node_id})"),
        ProjectCommand::SetOutputNode { node_id } => format!("SetOutputNode({node_id})"),
        ProjectCommand::RecordCreativeDecision(d) => {
            format!("RecordCreativeDecision({})", d.decision_id)
        }
        ProjectCommand::SetPreference(p) => format!("SetPreference({})", p.key),
        ProjectCommand::ReplaceProject { .. } => "ReplaceProject".into(),
    }
}
