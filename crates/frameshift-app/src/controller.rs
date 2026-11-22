use crate::error::AppError;
use crate::snapshot::render_desktop_ui_snapshot;
use frameshift_media::Rgba8Frame;
use frameshift_schema::{
    ClipDef, GraphNodeDef, NodeId, Project, ProjectCommand, ProjectSession, RenderProfileKind,
};
use frameshift_time::{Flicks, SpeedCurve, SpeedCurveSpec, SpeedKeyframe};
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
        let mut candidate = self.session.project().clone();
        let idx = candidate
            .timeline
            .clips
            .iter()
            .position(|c| c.id == clip_id)
            .ok_or_else(|| AppError::InvalidOperation(format!("missing clip '{clip_id}'")))?;

        let summary = match action {
            ClipEditAction::Trim {
                new_timeline_start,
                new_timeline_end,
            } => {
                let clip = &mut candidate.timeline.clips[idx];
                let old_start = clip.timeline_start;
                let target_start = new_timeline_start.unwrap_or(clip.timeline_start);
                let target_end = new_timeline_end.unwrap_or(clip.timeline_end);
                if target_start < Flicks::ZERO || target_start >= target_end {
                    return Err(AppError::InvalidOperation(format!(
                        "invalid trim bounds [{target_start}, {target_end}) for clip '{clip_id}'"
                    )));
                }
                if target_start > old_start {
                    let delta = target_start - old_start;
                    let src_advance = clip.speed_curve.source_offset_at(delta);
                    clip.source_start = clip.source_start.saturating_add(src_advance);
                    clip.speed_curve = shift_speed_curve(&clip.speed_curve, delta)?;
                }
                clip.timeline_start = target_start;
                clip.timeline_end = target_end;
                format!("Trimmed {clip_id} to [{}, {}]", target_start.0, target_end.0)
            }
            ClipEditAction::Split {
                split_tick,
                right_clip_id,
            } => {
                if candidate
                    .timeline
                    .clips
                    .iter()
                    .any(|c| c.id == right_clip_id)
                {
                    return Err(AppError::InvalidOperation(format!(
                        "right_clip_id '{right_clip_id}' already exists"
                    )));
                }
                let orig = candidate.timeline.clips[idx].clone();
                if split_tick <= orig.timeline_start || split_tick >= orig.timeline_end {
                    return Err(AppError::InvalidOperation(format!(
                        "split_tick {split_tick} must lie strictly inside [{}, {})",
                        orig.timeline_start, orig.timeline_end
                    )));
                }

                let split_offset = split_tick - orig.timeline_start;
                let right_source_start = orig
                    .source_start
                    .saturating_add(orig.speed_curve.source_offset_at(split_offset));
                let right_curve = shift_speed_curve(&orig.speed_curve, split_offset)?;

                candidate.timeline.clips[idx].timeline_end = split_tick;
                let right_clip = ClipDef {
                    id: right_clip_id.clone(),
                    asset_id: orig.asset_id,
                    timeline_start: split_tick,
                    timeline_end: orig.timeline_end,
                    source_start: right_source_start,
                    speed_curve: right_curve,
                    linked_event_id: orig.linked_event_id,
                    event_animations: Vec::new(),
                };
                candidate.timeline.clips.insert(idx + 1, right_clip);
                format!(
                    "Split {clip_id} at {} into {clip_id} + {right_clip_id}",
                    split_tick.0
                )
            }
        };

        self.human_apply_command(
            "split_or_trim_clip",
            summary,
            ProjectCommand::ReplaceProject { project: candidate },
        )
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
        ProjectCommand::AuthorHardCuts { cuts } => format!("AuthorHardCuts({})", cuts.len()),
        ProjectCommand::SetClipSpeedCurve { clip_id, .. } => {
            format!("SetClipSpeedCurve({clip_id})")
        }
        ProjectCommand::UpsertEventAnchor(e) => format!("UpsertEventAnchor({})", e.event_id),
        ProjectCommand::MoveEventTarget {
            event_id,
            new_target_output_tick,
        } => format!("MoveEventTarget({event_id} -> {})", new_target_output_tick.0),
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

fn shift_speed_curve(curve: &SpeedCurve, delta: Flicks) -> Result<SpeedCurve, AppError> {
    match curve.spec() {
        SpeedCurveSpec::Freeze => Ok(SpeedCurve::freeze()),
        SpeedCurveSpec::Constant { speed } => Ok(SpeedCurve::constant(*speed)?),
        SpeedCurveSpec::PiecewiseLinear { keyframes } => {
            let shifted = shift_keyframes(curve, keyframes, delta);
            Ok(SpeedCurve::piecewise_linear(shifted)?)
        }
        SpeedCurveSpec::CubicHermite { keyframes } => {
            let shifted = shift_keyframes(curve, keyframes, delta);
            Ok(SpeedCurve::cubic_hermite(shifted)?)
        }
    }
}

fn shift_keyframes(
    curve: &SpeedCurve,
    keyframes: &[SpeedKeyframe],
    delta: Flicks,
) -> Vec<SpeedKeyframe> {
    let mut shifted: Vec<SpeedKeyframe> = Vec::new();
    let speed_at_zero = curve.instant_speed_at(delta);
    shifted.push(SpeedKeyframe {
        output_offset: Flicks::ZERO,
        speed: speed_at_zero,
        tangent: None,
    });
    for kf in keyframes {
        if kf.output_offset > delta {
            shifted.push(SpeedKeyframe {
                output_offset: kf.output_offset - delta,
                speed: kf.speed,
                tangent: kf.tangent,
            });
        }
    }
    shifted
}
