use crate::error::SchemaError;
use crate::graph::{
    BlendMode, ColorSpace, GraphNodeDef, NodeId, NodeKind, RenderGraphDef, SourceFit,
};
use crate::project::{
    AssetDef, ClipDef, CreativeDecisionRecord, EventAnchorDef, PreferenceEntry, Project,
};
use frameshift_time::{
    BezierSpeedKeyframe, Flicks, ImpactCurveStyle, ImpactSolveRequest, ImpactSolver, KeyframeEase,
    Rational32, SourceFitMode, SpeedCurve, SpeedCurveSpec, SpeedKeyframe, TimeError,
};
use serde::{Deserialize, Serialize};

/// One shot in a hard-cut list.
///
/// Output ranges are half-open `[timeline_start, timeline_end)` and every time
/// field is an integer `Flicks` count. `source_start` is the source tick shown
/// at `timeline_start`. Omit `speed_curve` for constant `1.0` playback.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HardCut {
    pub clip_id: String,
    pub asset_id: String,
    pub timeline_start: Flicks,
    pub timeline_end: Flicks,
    pub source_start: Flicks,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed_curve: Option<SpeedCurve>,
}

/// Discrete, validated mutation command applied through `ProjectSession`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum ProjectCommand {
    SetProjectName {
        name: String,
    },
    AddAsset(AssetDef),
    RemoveAsset {
        id: String,
    },
    AddClip(ClipDef),
    RemoveClip {
        id: String,
    },
    /// Updates the timeline bounds of a clip, advancing its source map when the
    /// head is trimmed so the same source moment remains at the new start.
    TrimClip {
        clip_id: String,
        new_timeline_start: Option<Flicks>,
        new_timeline_end: Option<Flicks>,
    },
    /// Splits one clip into left and right ranges and routes every source node
    /// that referenced the original clip through a transparent-aware blend.
    SplitClip {
        clip_id: String,
        split_tick: Flicks,
        right_clip_id: String,
    },
    /// Writes an ordered, non-overlapping hard-cut list as the picture.
    ///
    /// Each cut becomes a `ClipDef` and a `SourceVideo`. Every cut after the
    /// first is placed over the previous result with one `Blend` (normal,
    /// opacity 1). The chain tail becomes `graph.output_node`. A source
    /// outside its clip range stays transparent; this command does not change
    /// that sampling rule. `cuts` must already be sorted so each next
    /// `timeline_start` is greater than or equal to the previous `timeline_end`.
    AuthorHardCuts {
        cuts: Vec<HardCut>,
    },
    SetClipSpeedCurve {
        clip_id: String,
        speed_curve: SpeedCurve,
    },
    UpsertEventAnchor(EventAnchorDef),
    MoveEventTarget {
        event_id: String,
        new_target_output_tick: Flicks,
    },
    /// Moves an event target and, when the event has a linked source clip and
    /// source anchor, solves a new bounded speed curve that aligns the source
    /// impact with the requested output tick. The source bounds, speed range,
    /// curve style, and fit policy are explicit so headless clients never rely
    /// on implicit trim or speed choices. Anchors without a linked source keep
    /// the simple move behavior.
    RetargetEvent {
        event_id: String,
        new_target_output_tick: Flicks,
        source_bounds_start: Flicks,
        source_bounds_end: Flicks,
        min_speed: f64,
        max_speed: f64,
        curve_style: ImpactCurveStyle,
        source_fit: SourceFitMode,
    },
    /// Groups a node selection into one precomp graph node.
    Precompose {
        selected_node_ids: Vec<NodeId>,
        precomp_node_id: NodeId,
    },
    /// Expands one precomp graph node back into its parent graph.
    Unprecompose {
        precomp_node_id: NodeId,
    },
    UpsertGraphNode(GraphNodeDef),
    RemoveGraphNode {
        node_id: NodeId,
    },
    SetOutputNode {
        node_id: NodeId,
    },
    RecordCreativeDecision(CreativeDecisionRecord),
    SetPreference(PreferenceEntry),
    ReplaceProject {
        project: Project,
    },
}

/// Stateful editing session enforcing optimistic locking (`revision_id`),
/// atomic validation before commit, and undo/redo history.
#[derive(Debug, Clone)]
pub struct ProjectSession {
    project: Project,
    undo_stack: Vec<Project>,
    redo_stack: Vec<Project>,
}

impl ProjectSession {
    /// Creates a new validated `ProjectSession` around `project`.
    pub fn new(project: Project) -> Result<Self, SchemaError> {
        project.validate()?;
        Ok(Self {
            project,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        })
    }

    /// Returns the active project revision ID.
    #[inline]
    pub fn revision_id(&self) -> u64 {
        self.project.revision_id
    }

    /// Returns an immutable reference to the active project.
    #[inline]
    pub fn project(&self) -> &Project {
        &self.project
    }

    /// Validates the render graph DAG of the active project and returns topological order.
    pub fn validate_graph(&self) -> Result<Vec<NodeId>, SchemaError> {
        self.project.graph.validate_graph()
    }

    /// Applies `cmd` iff `expected_rev == self.revision_id()`, validates the resulting
    /// project atomically, pushes the prior state onto the undo stack, clears redo,
    /// and returns the new `revision_id`.
    pub fn apply_command(
        &mut self,
        expected_rev: u64,
        cmd: ProjectCommand,
    ) -> Result<u64, SchemaError> {
        if expected_rev != self.project.revision_id {
            return Err(SchemaError::RevisionConflict {
                expected: expected_rev,
                actual: self.project.revision_id,
            });
        }

        let mut candidate = self.project.clone();
        apply_to_project(&mut candidate, cmd)?;
        candidate.revision_id = self.project.revision_id + 1;
        candidate.validate()?;

        let prev = std::mem::replace(&mut self.project, candidate);
        self.undo_stack.push(prev);
        self.redo_stack.clear();

        Ok(self.project.revision_id)
    }

    /// Undoes the most recent committed command if `expected_rev` matches.
    ///
    /// Restores the prior project snapshot while advancing `revision_id` monotonically
    /// so optimistic-locking clients always observe a strictly increasing revision.
    pub fn undo(&mut self, expected_rev: u64) -> Result<u64, SchemaError> {
        if expected_rev != self.project.revision_id {
            return Err(SchemaError::RevisionConflict {
                expected: expected_rev,
                actual: self.project.revision_id,
            });
        }
        let mut previous = self.undo_stack.pop().ok_or(SchemaError::EmptyUndoStack)?;
        let next_rev = self.project.revision_id + 1;
        previous.revision_id = next_rev;

        let undone = std::mem::replace(&mut self.project, previous);
        self.redo_stack.push(undone);
        Ok(next_rev)
    }

    /// Reapplies the most recently undone command if `expected_rev` matches.
    pub fn redo(&mut self, expected_rev: u64) -> Result<u64, SchemaError> {
        if expected_rev != self.project.revision_id {
            return Err(SchemaError::RevisionConflict {
                expected: expected_rev,
                actual: self.project.revision_id,
            });
        }
        let mut restored = self.redo_stack.pop().ok_or(SchemaError::EmptyRedoStack)?;
        let next_rev = self.project.revision_id + 1;
        restored.revision_id = next_rev;

        let current = std::mem::replace(&mut self.project, restored);
        self.undo_stack.push(current);
        Ok(next_rev)
    }

    #[inline]
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    #[inline]
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    #[inline]
    pub fn undo_len(&self) -> usize {
        self.undo_stack.len()
    }

    #[inline]
    pub fn redo_len(&self) -> usize {
        self.redo_stack.len()
    }
}

fn apply_to_project(project: &mut Project, cmd: ProjectCommand) -> Result<(), SchemaError> {
    match cmd {
        ProjectCommand::SetProjectName { name } => {
            project.name = name;
        }
        ProjectCommand::AddAsset(asset) => {
            if project.assets.iter().any(|a| a.id == asset.id) {
                return Err(SchemaError::DuplicateId {
                    entity: "asset",
                    id: asset.id,
                });
            }
            project.assets.push(asset);
        }
        ProjectCommand::RemoveAsset { id } => {
            if project.timeline.clips.iter().any(|c| c.asset_id == id)
                || project
                    .timeline
                    .audio_clips
                    .iter()
                    .any(|c| c.asset_id == id)
            {
                return Err(SchemaError::AssetInUse(id));
            }
            let pos = project
                .assets
                .iter()
                .position(|a| a.id == id)
                .ok_or_else(|| SchemaError::MissingAsset(id.clone()))?;
            project.assets.remove(pos);
        }
        ProjectCommand::AddClip(clip) => {
            if project.timeline.clips.iter().any(|c| c.id == clip.id) {
                return Err(SchemaError::DuplicateId {
                    entity: "clip",
                    id: clip.id,
                });
            }
            project.timeline.clips.push(clip);
        }
        ProjectCommand::RemoveClip { id } => {
            let pos = project
                .timeline
                .clips
                .iter()
                .position(|c| c.id == id)
                .ok_or_else(|| SchemaError::MissingClip(id.clone()))?;
            project.timeline.clips.remove(pos);
        }
        ProjectCommand::TrimClip {
            clip_id,
            new_timeline_start,
            new_timeline_end,
        } => {
            trim_clip(project, &clip_id, new_timeline_start, new_timeline_end)?;
        }
        ProjectCommand::SplitClip {
            clip_id,
            split_tick,
            right_clip_id,
        } => {
            split_clip(project, &clip_id, split_tick, &right_clip_id)?;
        }
        ProjectCommand::AuthorHardCuts { cuts } => {
            author_hard_cuts(project, &cuts)?;
        }
        ProjectCommand::SetClipSpeedCurve {
            clip_id,
            speed_curve,
        } => {
            let clip = project
                .timeline
                .clips
                .iter_mut()
                .find(|c| c.id == clip_id)
                .ok_or_else(|| SchemaError::MissingClip(clip_id.clone()))?;
            clip.speed_curve = speed_curve;
        }
        ProjectCommand::UpsertEventAnchor(anchor) => {
            if let Some(existing) = project
                .events
                .iter_mut()
                .find(|e| e.event_id == anchor.event_id)
            {
                *existing = anchor;
            } else {
                project.events.push(anchor);
            }
        }
        ProjectCommand::MoveEventTarget {
            event_id,
            new_target_output_tick,
        } => {
            let ev = project
                .events
                .iter_mut()
                .find(|e| e.event_id == event_id)
                .ok_or_else(|| SchemaError::MissingEvent(event_id.clone()))?;
            ev.target_output_tick = new_target_output_tick;
        }
        ProjectCommand::RetargetEvent {
            event_id,
            new_target_output_tick,
            source_bounds_start,
            source_bounds_end,
            min_speed,
            max_speed,
            curve_style,
            source_fit,
        } => {
            let event_index = project
                .events
                .iter()
                .position(|event| event.event_id == event_id)
                .ok_or_else(|| SchemaError::MissingEvent(event_id.clone()))?;

            let linked_source = project.events[event_index]
                .source_clip_id
                .clone()
                .zip(project.events[event_index].source_anchor_tick);
            if let Some((source_clip_id, source_anchor_tick)) = linked_source {
                let clip = project
                    .timeline
                    .clips
                    .iter_mut()
                    .find(|clip| clip.id == source_clip_id)
                    .ok_or_else(|| SchemaError::MissingClip(source_clip_id.clone()))?;
                let request = ImpactSolveRequest {
                    source_bounds_start,
                    source_bounds_end,
                    source_impact: source_anchor_tick,
                    output_start: clip.timeline_start,
                    output_end: clip.timeline_end,
                    target_output: new_target_output_tick,
                    min_speed,
                    max_speed,
                    style: curve_style,
                    fit_mode: source_fit,
                };
                let solution = ImpactSolver::solve(&request).map_err(|violation| {
                    SchemaError::InvalidTimeline(format!(
                        "cannot retarget event '{event_id}': {violation}"
                    ))
                })?;
                clip.source_start = solution.effective_source_start;
                clip.speed_curve = solution.speed_curve;
            }

            project.events[event_index].target_output_tick = new_target_output_tick;
        }
        ProjectCommand::Precompose {
            selected_node_ids,
            precomp_node_id,
        } => {
            crate::precomp::precompose(&mut project.graph, &selected_node_ids, precomp_node_id)?;
        }
        ProjectCommand::Unprecompose { precomp_node_id } => {
            crate::precomp::unprecompose(&mut project.graph, &precomp_node_id)?;
        }
        ProjectCommand::UpsertGraphNode(node) => {
            if let Some(existing) = project.graph.nodes.iter_mut().find(|n| n.id == node.id) {
                *existing = node;
            } else {
                project.graph.nodes.push(node);
            }
        }
        ProjectCommand::RemoveGraphNode { node_id } => {
            if project.graph.output_node == node_id
                || project
                    .graph
                    .nodes
                    .iter()
                    .any(|n| n.inputs.contains(&node_id))
            {
                return Err(SchemaError::NodeInUse(node_id));
            }
            let pos = project
                .graph
                .nodes
                .iter()
                .position(|n| n.id == node_id)
                .ok_or_else(|| SchemaError::MissingNode(node_id.clone()))?;
            project.graph.nodes.remove(pos);
        }
        ProjectCommand::SetOutputNode { node_id } => {
            if project.graph.node(&node_id).is_none() {
                return Err(SchemaError::MissingNode(node_id));
            }
            project.graph.output_node = node_id;
        }
        ProjectCommand::RecordCreativeDecision(record) => {
            if project
                .creative_decisions
                .iter()
                .any(|d| d.decision_id == record.decision_id)
            {
                return Err(SchemaError::DuplicateId {
                    entity: "creative_decision",
                    id: record.decision_id,
                });
            }
            project.creative_decisions.push(record);
        }
        ProjectCommand::SetPreference(entry) => {
            if let Some(existing) = project.preferences.iter_mut().find(|p| {
                p.key == entry.key && p.scope == entry.scope && p.target_id == entry.target_id
            }) {
                *existing = entry;
            } else {
                project.preferences.push(entry);
            }
        }
        ProjectCommand::ReplaceProject {
            project: replacement,
        } => {
            *project = replacement;
        }
    }
    Ok(())
}

fn trim_clip(
    project: &mut Project,
    clip_id: &str,
    new_timeline_start: Option<Flicks>,
    new_timeline_end: Option<Flicks>,
) -> Result<(), SchemaError> {
    let clip = project
        .timeline
        .clips
        .iter_mut()
        .find(|clip| clip.id == clip_id)
        .ok_or_else(|| SchemaError::MissingClip(clip_id.to_owned()))?;
    let target_start = new_timeline_start.unwrap_or(clip.timeline_start);
    let target_end = new_timeline_end.unwrap_or(clip.timeline_end);
    if target_start < Flicks::ZERO || target_start >= target_end {
        return Err(SchemaError::InvalidClipBounds {
            clip_id: clip_id.to_owned(),
            start: target_start,
            end: target_end,
        });
    }
    if target_start > clip.timeline_start {
        let delta = target_start - clip.timeline_start;
        clip.source_start = clip
            .source_start
            .saturating_add(clip.speed_curve.source_offset_at(delta));
        clip.speed_curve = shift_speed_curve(&clip.speed_curve, delta)?;
    }
    clip.timeline_start = target_start;
    clip.timeline_end = target_end;
    Ok(())
}

fn split_clip(
    project: &mut Project,
    clip_id: &str,
    split_tick: Flicks,
    right_clip_id: &str,
) -> Result<(), SchemaError> {
    if right_clip_id.is_empty() {
        return Err(SchemaError::InvalidTimeline(
            "right_clip_id must not be empty".into(),
        ));
    }
    if project
        .timeline
        .clips
        .iter()
        .any(|clip| clip.id == right_clip_id)
        || project
            .timeline
            .audio_clips
            .iter()
            .any(|clip| clip.id == right_clip_id)
    {
        return Err(SchemaError::DuplicateId {
            entity: "clip",
            id: right_clip_id.to_owned(),
        });
    }
    let index = project
        .timeline
        .clips
        .iter()
        .position(|clip| clip.id == clip_id)
        .ok_or_else(|| SchemaError::MissingClip(clip_id.to_owned()))?;
    let original = project.timeline.clips[index].clone();
    if split_tick <= original.timeline_start || split_tick >= original.timeline_end {
        return Err(SchemaError::InvalidTimeline(format!(
            "split_tick {split_tick} must lie strictly inside [{}, {})",
            original.timeline_start, original.timeline_end
        )));
    }

    let split_offset = split_tick - original.timeline_start;
    let right_source_start = original
        .source_start
        .saturating_add(original.speed_curve.source_offset_at(split_offset));
    let right_curve = shift_speed_curve(&original.speed_curve, split_offset)?;
    project.timeline.clips[index].timeline_end = split_tick;
    project.timeline.clips.insert(
        index + 1,
        ClipDef {
            id: right_clip_id.to_owned(),
            asset_id: original.asset_id,
            timeline_start: split_tick,
            timeline_end: original.timeline_end,
            source_start: right_source_start,
            speed_curve: right_curve,
            linked_event_id: original.linked_event_id,
            event_animations: Vec::new(),
        },
    );

    for event in &mut project.events {
        if event.source_clip_id.as_deref() == Some(clip_id)
            && event.target_output_tick >= split_tick
        {
            event.source_clip_id = Some(right_clip_id.to_owned());
        }
    }

    split_graph_source_routes(&mut project.graph, clip_id, right_clip_id)?;
    Ok(())
}

fn split_graph_source_routes(
    graph: &mut RenderGraphDef,
    left_clip_id: &str,
    right_clip_id: &str,
) -> Result<(), SchemaError> {
    // Only visit the nodes that existed on entry. The left SourceVideo clones
    // retain `left_clip_id` and must not be recursively split a second time.
    let original_len = graph.nodes.len();
    for index in 0..original_len {
        if let NodeKind::Precomp { sub_graph, .. } = &mut graph.nodes[index].kind {
            split_graph_source_routes(sub_graph, left_clip_id, right_clip_id)?;
        }

        let original_node = graph.nodes[index].clone();
        let NodeKind::SourceVideo {
            asset_id,
            clip_id: Some(source_clip_id),
            color_space,
            fit,
        } = &original_node.kind
        else {
            continue;
        };
        if source_clip_id != left_clip_id {
            continue;
        }

        let left_source_id = fresh_split_source_id(
            graph,
            &format!("{}_split_left_{}", original_node.id.0, right_clip_id),
        );
        let right_source_id = fresh_split_source_id(
            graph,
            &format!("{}_split_right_{}", original_node.id.0, right_clip_id),
        );
        let source_node = |id: NodeId, clip_id: &str| GraphNodeDef {
            id,
            kind: NodeKind::SourceVideo {
                asset_id: asset_id.clone(),
                clip_id: Some(clip_id.to_owned()),
                color_space: *color_space,
                fit: *fit,
            },
            inputs: Vec::new(),
            // A disabled source clears to transparency. Keep both branches
            // disabled and the routing blend enabled to preserve that result.
            enabled: original_node.enabled,
        };
        graph.nodes[index].kind = NodeKind::Blend {
            mode: BlendMode::Normal,
            opacity: 1.0,
        };
        graph.nodes[index].inputs = vec![left_source_id.clone(), right_source_id.clone()];
        graph.nodes[index].enabled = true;
        graph.nodes.push(source_node(left_source_id, left_clip_id));
        graph
            .nodes
            .push(source_node(right_source_id, right_clip_id));
    }
    Ok(())
}

fn fresh_split_source_id(graph: &RenderGraphDef, base: &str) -> NodeId {
    let mut candidate = NodeId::new(base);
    let mut suffix = 1u64;
    while graph.find_node_recursive(&candidate).is_some() {
        candidate = NodeId::new(format!("{base}_{suffix}"));
        suffix += 1;
    }
    candidate
}

fn shift_speed_curve(curve: &SpeedCurve, delta: Flicks) -> Result<SpeedCurve, SchemaError> {
    let map_error = |error: TimeError| {
        SchemaError::InvalidTimeline(format!("could not shift clip speed curve: {error}"))
    };
    match curve.spec() {
        SpeedCurveSpec::Freeze => Ok(SpeedCurve::freeze()),
        SpeedCurveSpec::Constant { speed } => SpeedCurve::constant(*speed).map_err(map_error),
        SpeedCurveSpec::PiecewiseLinear { keyframes } => {
            SpeedCurve::piecewise_linear(shift_keyframes(curve, keyframes, delta))
                .map_err(map_error)
        }
        SpeedCurveSpec::CubicHermite { keyframes } => {
            SpeedCurve::cubic_hermite(shift_keyframes(curve, keyframes, delta)).map_err(map_error)
        }
        SpeedCurveSpec::CubicBezierValue { keyframes } => {
            SpeedCurve::cubic_bezier_value(shift_bezier_value_keyframes(curve, keyframes, delta))
                .map_err(map_error)
        }
        SpeedCurveSpec::CubicBezierSpeed { keyframes } => {
            SpeedCurve::cubic_bezier_speed(shift_bezier_speed_keyframes(curve, keyframes, delta))
                .map_err(map_error)
        }
    }
}

fn shift_bezier_value_keyframes(
    curve: &SpeedCurve,
    keyframes: &[BezierSpeedKeyframe],
    delta: Flicks,
) -> Vec<BezierSpeedKeyframe> {
    let base_source = curve.source_offset_at(delta);
    let head = if let Some(exact) = keyframes.iter().find(|kf| kf.output_offset == delta) {
        BezierSpeedKeyframe {
            output_offset: Flicks::ZERO,
            speed: exact.speed,
            source_offset: Some(Flicks::ZERO),
            in_ease: None,
            out_ease: exact.out_ease,
        }
    } else {
        BezierSpeedKeyframe {
            output_offset: Flicks::ZERO,
            speed: 0.0,
            source_offset: Some(Flicks::ZERO),
            in_ease: None,
            out_ease: Some(KeyframeEase {
                speed: curve.instant_speed_at(delta),
                influence: 0.333,
            }),
        }
    };
    let mut shifted = vec![head];
    for keyframe in keyframes {
        if keyframe.output_offset > delta {
            shifted.push(BezierSpeedKeyframe {
                output_offset: keyframe.output_offset - delta,
                speed: keyframe.speed,
                source_offset: Some(keyframe.source_offset.unwrap_or(Flicks::ZERO) - base_source),
                in_ease: keyframe.in_ease,
                out_ease: keyframe.out_ease,
            });
        }
    }
    shifted
}

fn shift_bezier_speed_keyframes(
    curve: &SpeedCurve,
    keyframes: &[BezierSpeedKeyframe],
    delta: Flicks,
) -> Vec<BezierSpeedKeyframe> {
    let head = if let Some(exact) = keyframes.iter().find(|kf| kf.output_offset == delta) {
        BezierSpeedKeyframe {
            output_offset: Flicks::ZERO,
            speed: exact.speed,
            source_offset: None,
            in_ease: None,
            out_ease: exact.out_ease,
        }
    } else {
        BezierSpeedKeyframe {
            output_offset: Flicks::ZERO,
            speed: curve.instant_speed_at(delta),
            source_offset: None,
            in_ease: None,
            out_ease: Some(KeyframeEase {
                speed: curve.instant_speed_derivative_at(delta),
                influence: 0.333,
            }),
        }
    };
    let mut shifted = vec![head];
    for keyframe in keyframes {
        if keyframe.output_offset > delta {
            shifted.push(BezierSpeedKeyframe {
                output_offset: keyframe.output_offset - delta,
                speed: keyframe.speed,
                source_offset: None,
                in_ease: keyframe.in_ease,
                out_ease: keyframe.out_ease,
            });
        }
    }
    shifted
}

fn shift_keyframes(
    curve: &SpeedCurve,
    keyframes: &[SpeedKeyframe],
    delta: Flicks,
) -> Vec<SpeedKeyframe> {
    let tangent = matches!(curve.spec(), SpeedCurveSpec::CubicHermite { .. })
        .then_some(curve.instant_speed_derivative_at(delta));
    let mut shifted = vec![SpeedKeyframe {
        output_offset: Flicks::ZERO,
        speed: curve.instant_speed_at(delta),
        tangent,
    }];
    for keyframe in keyframes {
        if keyframe.output_offset > delta {
            shifted.push(SpeedKeyframe {
                output_offset: keyframe.output_offset - delta,
                speed: keyframe.speed,
                tangent: keyframe.tangent,
            });
        }
    }
    shifted
}

fn author_hard_cuts(project: &mut Project, cuts: &[HardCut]) -> Result<(), SchemaError> {
    if cuts.is_empty() {
        return Err(SchemaError::InvalidTimeline(
            "author_hard_cuts requires at least one cut".into(),
        ));
    }
    validate_hard_cut_list(project, cuts)?;

    let mut nodes = Vec::with_capacity(cuts.len() * 2 - 1);
    let first_src = cut_source_id(&cuts[0].clip_id);
    ensure_fresh_node(project, &nodes, &first_src)?;
    nodes.push(source_node(&cuts[0], first_src.clone()));
    let mut tail = first_src;

    for cut in cuts.iter().skip(1) {
        let src_id = cut_source_id(&cut.clip_id);
        let blend_id = cut_blend_id(&cut.clip_id);
        ensure_fresh_node(project, &nodes, &src_id)?;
        ensure_fresh_node(project, &nodes, &blend_id)?;
        nodes.push(source_node(cut, src_id.clone()));
        nodes.push(GraphNodeDef {
            id: blend_id.clone(),
            kind: NodeKind::Blend {
                mode: BlendMode::Normal,
                opacity: 1.0,
            },
            inputs: vec![tail, src_id],
            enabled: true,
        });
        tail = blend_id;
    }

    for cut in cuts {
        project.timeline.clips.push(clip_from_cut(cut));
    }
    project.graph.nodes.extend(nodes);
    project.graph.output_node = tail;
    Ok(())
}

fn validate_hard_cut_list(project: &Project, cuts: &[HardCut]) -> Result<(), SchemaError> {
    for (index, cut) in cuts.iter().enumerate() {
        if cut.clip_id.is_empty() {
            return Err(SchemaError::InvalidTimeline(
                "hard cut clip_id must be non-empty".into(),
            ));
        }
        if cuts[..index].iter().any(|prev| prev.clip_id == cut.clip_id) {
            return Err(SchemaError::DuplicateId {
                entity: "clip",
                id: cut.clip_id.clone(),
            });
        }
        if project.timeline.clips.iter().any(|c| c.id == cut.clip_id)
            || project
                .timeline
                .audio_clips
                .iter()
                .any(|c| c.id == cut.clip_id)
        {
            return Err(SchemaError::DuplicateId {
                entity: "clip",
                id: cut.clip_id.clone(),
            });
        }
        if project.asset(&cut.asset_id).is_none() {
            return Err(SchemaError::MissingAsset(cut.asset_id.clone()));
        }
        if cut.timeline_start >= cut.timeline_end {
            return Err(SchemaError::InvalidClipBounds {
                clip_id: cut.clip_id.clone(),
                start: cut.timeline_start,
                end: cut.timeline_end,
            });
        }
        if index > 0 {
            let prev = &cuts[index - 1];
            if cut.timeline_start < prev.timeline_end {
                return Err(SchemaError::InvalidTimeline(format!(
                    "hard cut '{}' starts at {} before previous cut '{}' ends at {}; cuts must be ordered and non-overlapping",
                    cut.clip_id,
                    cut.timeline_start.as_i64(),
                    prev.clip_id,
                    prev.timeline_end.as_i64(),
                )));
            }
        }
    }
    Ok(())
}

fn clip_from_cut(cut: &HardCut) -> ClipDef {
    let speed_curve = cut
        .speed_curve
        .clone()
        .unwrap_or_else(|| SpeedCurve::constant(1.0).expect("constant speed 1.0 is finite"));
    ClipDef {
        id: cut.clip_id.clone(),
        asset_id: cut.asset_id.clone(),
        timeline_start: cut.timeline_start,
        timeline_end: cut.timeline_end,
        source_start: cut.source_start,
        speed_curve,
        linked_event_id: None,
        event_animations: Vec::new(),
    }
}

fn source_node(cut: &HardCut, id: NodeId) -> GraphNodeDef {
    GraphNodeDef {
        id,
        kind: NodeKind::SourceVideo {
            asset_id: cut.asset_id.clone(),
            clip_id: Some(cut.clip_id.clone()),
            color_space: ColorSpace::DisplaySrgb,
            fit: SourceFit::Stretch,
        },
        inputs: Vec::new(),
        enabled: true,
    }
}

fn cut_source_id(clip_id: &str) -> NodeId {
    NodeId::new(format!("cut_src_{clip_id}"))
}

fn cut_blend_id(clip_id: &str) -> NodeId {
    NodeId::new(format!("cut_blend_{clip_id}"))
}

fn ensure_fresh_node(
    project: &Project,
    pending: &[GraphNodeDef],
    id: &NodeId,
) -> Result<(), SchemaError> {
    if project.graph.node(id).is_some() || pending.iter().any(|node| &node.id == id) {
        return Err(SchemaError::DuplicateId {
            entity: "graph node",
            id: id.0.clone(),
        });
    }
    Ok(())
}

/// One source asset available to [`plan_autocut`].
///
/// `duration_ticks` is the full asset length and `source_start` is the in-point
/// used the first time the asset is read. A per-asset cursor advances by the
/// length of each assigned window and wraps back to `source_start` when the
/// next window would pass `duration_ticks`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutocutAsset {
    pub asset_id: String,
    pub duration_ticks: Flicks,
    pub source_start: Flicks,
}

/// Deterministic knobs for [`plan_autocut`].
///
/// `cuts_per_beat` accepts `1/1`, `2/1`, `1/2` and `1/4`: one cut per beat,
/// two cuts per beat (an extra boundary halfway to the next beat), one cut
/// every two beats and one cut every four beats. `offset_ticks` shifts every
/// beat-derived boundary by a constant amount. `min_spacing_ticks` is the
/// smallest gap between consecutive cut starts (`0` disables the filter) and
/// `max_cuts` of `None` means "unlimited".
#[derive(Debug, Clone, PartialEq)]
pub struct AutocutOptions {
    pub start_tick: Flicks,
    pub end_tick: Flicks,
    pub cuts_per_beat: Rational32,
    pub min_spacing_ticks: Flicks,
    pub max_cuts: Option<usize>,
    pub offset_ticks: Flicks,
    pub fps: Rational32,
}

/// Ordered, non-overlapping hard cuts produced by [`plan_autocut`].
///
/// The cuts are pure data: planning never writes event anchors, never splits
/// an existing clip and never touches a project. `dropped_beats` counts
/// beat-derived boundaries removed by frame quantization, `min_spacing_ticks`
/// or `max_cuts`; `warnings` carries human-readable notes about those drops
/// and about assets shorter than one window.
#[derive(Debug, Clone, PartialEq)]
pub struct AutocutPlan {
    pub cuts: Vec<HardCut>,
    pub dropped_beats: usize,
    pub warnings: Vec<String>,
}

/// Plans a deterministic hard-cut list from beat ticks and repeating assets.
///
/// Boundaries start at `start_tick`, follow the beat grid scaled by
/// `cuts_per_beat` (plus `offset_ticks`) and end at `end_tick`. Every boundary
/// is snapped to the frame grid of `options.fps`: ceiling for the first
/// boundary, nearest frame start for interior boundaries and floor for the
/// last one, so the plan never leaves `[start_tick, end_tick]`.
///
/// Each cut reads its window length from the next asset in round-robin order
/// (`cut_index % assets.len()`). Per-asset cursors advance by the window
/// length and wrap back to `source_start` when the asset would run out, so the
/// same input always yields the same asset and source assignment.
///
/// Returns [`SchemaError::InvalidTimeline`] for empty beats or assets, a
/// reversed or empty window (also after frame quantization), unsupported
/// `cuts_per_beat`, negative spacing, `max_cuts` of zero, unsorted beats or an
/// asset whose `duration_ticks`/`source_start` cannot satisfy one read.
pub fn plan_autocut(
    beats: &[Flicks],
    assets: &[AutocutAsset],
    options: &AutocutOptions,
) -> Result<AutocutPlan, SchemaError> {
    let ratio = validate_autocut_request(beats, assets, options)?;

    let start = frame_ceil(options.fps, options.start_tick)?;
    let end = frame_floor(options.fps, options.end_tick)?;
    if start >= end {
        return Err(SchemaError::InvalidTimeline(format!(
            "autocut window [{}, {}) is empty after quantization to {}",
            options.start_tick.as_i64(),
            options.end_tick.as_i64(),
            options.fps
        )));
    }

    let mut warnings = Vec::new();
    let mut quantize_dropped = 0usize;
    let mut spacing_dropped = 0usize;
    let mut kept = vec![start];
    let mut last_kept = start;
    let mut last_quantized: Option<Flicks> = None;

    for candidate in beat_boundaries(beats, options, ratio, start, end)? {
        let quantized = frame_nearest(options.fps, candidate)?;
        if last_quantized == Some(quantized) {
            quantize_dropped += 1;
            continue;
        }
        last_quantized = Some(quantized);
        if quantized <= start || quantized >= end {
            quantize_dropped += 1;
            continue;
        }
        let spacing = quantized.as_i64() as i128 - last_kept.as_i64() as i128;
        if spacing < options.min_spacing_ticks.as_i64() as i128 {
            spacing_dropped += 1;
            continue;
        }
        kept.push(quantized);
        last_kept = quantized;
    }

    if spacing_dropped > 0 {
        warnings.push(format!(
            "autocut dropped {spacing_dropped} boundary(ies) closer than min_spacing_ticks ({})",
            options.min_spacing_ticks.as_i64()
        ));
    }

    let mut max_cuts_dropped = 0usize;
    if let Some(max_cuts) = options.max_cuts.filter(|max_cuts| kept.len() > *max_cuts) {
        max_cuts_dropped = kept.len() - max_cuts;
        kept.truncate(max_cuts);
        warnings.push(format!(
            "autocut truncated {max_cuts_dropped} boundary(ies) to max_cuts ({max_cuts})"
        ));
    }

    let mut boundaries = kept;
    boundaries.push(end);

    let mut cursors: Vec<Flicks> = assets.iter().map(|asset| asset.source_start).collect();
    let mut cuts = Vec::with_capacity(boundaries.len() - 1);
    for (index, window) in boundaries.windows(2).enumerate() {
        let timeline_start = window[0];
        let timeline_end = window[1];
        let duration = Flicks(timeline_end.as_i64() - timeline_start.as_i64());
        let asset_index = index % assets.len();
        let source_start =
            advance_asset_cursor(assets, &mut cursors, asset_index, duration, &mut warnings);
        cuts.push(HardCut {
            clip_id: format!("autocut_{index:04}"),
            asset_id: assets[asset_index].asset_id.clone(),
            timeline_start,
            timeline_end,
            source_start,
            speed_curve: None,
        });
    }

    Ok(AutocutPlan {
        cuts,
        dropped_beats: quantize_dropped + spacing_dropped + max_cuts_dropped,
        warnings,
    })
}

fn validate_autocut_request(
    beats: &[Flicks],
    assets: &[AutocutAsset],
    options: &AutocutOptions,
) -> Result<Rational32, SchemaError> {
    if beats.is_empty() {
        return Err(SchemaError::InvalidTimeline(
            "plan_autocut requires at least one beat".into(),
        ));
    }
    if assets.is_empty() {
        return Err(SchemaError::InvalidTimeline(
            "plan_autocut requires at least one asset".into(),
        ));
    }
    if options.start_tick >= options.end_tick {
        return Err(SchemaError::InvalidTimeline(format!(
            "autocut window start ({}) must be < end ({})",
            options.start_tick.as_i64(),
            options.end_tick.as_i64()
        )));
    }
    if options.min_spacing_ticks < Flicks::ZERO {
        return Err(SchemaError::InvalidTimeline(format!(
            "autocut min_spacing_ticks must be >= 0, got {}",
            options.min_spacing_ticks.as_i64()
        )));
    }
    if options.max_cuts == Some(0) {
        return Err(SchemaError::InvalidTimeline(
            "autocut max_cuts must be at least 1".into(),
        ));
    }
    for asset in assets {
        if asset.asset_id.is_empty() {
            return Err(SchemaError::InvalidTimeline(
                "autocut asset_id must be non-empty".into(),
            ));
        }
        if asset.duration_ticks <= Flicks::ZERO {
            return Err(SchemaError::InvalidTimeline(format!(
                "autocut asset '{}' duration_ticks must be > 0",
                asset.asset_id
            )));
        }
        if asset.source_start < Flicks::ZERO || asset.source_start >= asset.duration_ticks {
            return Err(SchemaError::InvalidTimeline(format!(
                "autocut asset '{}' source_start ({}) must be in [0, {})",
                asset.asset_id,
                asset.source_start.as_i64(),
                asset.duration_ticks.as_i64()
            )));
        }
    }
    for pair in beats.windows(2) {
        if pair[1] < pair[0] {
            return Err(SchemaError::InvalidTimeline(
                "autocut beats must be sorted in non-decreasing order".into(),
            ));
        }
    }
    let ratio = options.cuts_per_beat.reduced().map_err(|err| {
        SchemaError::InvalidTimeline(format!("autocut cuts_per_beat is invalid: {err}"))
    })?;
    if !matches!((ratio.num, ratio.den), (1, 1) | (2, 1) | (1, 2) | (1, 4)) {
        return Err(SchemaError::InvalidTimeline(format!(
            "autocut cuts_per_beat {}/{} is unsupported; use 1, 2, 1/2 or 1/4",
            ratio.num, ratio.den
        )));
    }
    options
        .fps
        .reduced()
        .map_err(|err| SchemaError::InvalidTimeline(format!("autocut fps is invalid: {err}")))?;
    Ok(ratio)
}

fn beat_boundaries(
    beats: &[Flicks],
    options: &AutocutOptions,
    ratio: Rational32,
    start: Flicks,
    end: Flicks,
) -> Result<Vec<Flicks>, SchemaError> {
    let mut points = Vec::new();
    if ratio.den > 1 {
        let stride = ratio.den as usize;
        for beat in beats.iter().step_by(stride) {
            let tick = autocut_add(*beat, options.offset_ticks)?;
            push_interior(&mut points, tick, start, end);
        }
        return Ok(points);
    }

    for (index, beat) in beats.iter().enumerate() {
        let tick = autocut_add(*beat, options.offset_ticks)?;
        push_interior(&mut points, tick, start, end);
        if ratio.num < 2 {
            continue;
        }
        let half = match beats.get(index + 1) {
            Some(next) => autocut_delta(*next, *beat)? / 2,
            None if index > 0 => autocut_delta(*beat, beats[index - 1])? / 2,
            None => 0,
        };
        let midpoint = autocut_add(autocut_add(*beat, Flicks(half))?, options.offset_ticks)?;
        push_interior(&mut points, midpoint, start, end);
    }
    Ok(points)
}

fn push_interior(points: &mut Vec<Flicks>, tick: Flicks, start: Flicks, end: Flicks) {
    if tick > start && tick < end {
        points.push(tick);
    }
}

fn advance_asset_cursor(
    assets: &[AutocutAsset],
    cursors: &mut [Flicks],
    asset_index: usize,
    duration: Flicks,
    warnings: &mut Vec<String>,
) -> Flicks {
    let asset = &assets[asset_index];
    let capacity = asset.duration_ticks.as_i64() - asset.source_start.as_i64();
    if duration.as_i64() > capacity {
        let message = format!(
            "autocut asset '{}' is shorter than one cut window; the source will hold its last frame",
            asset.asset_id
        );
        if !warnings.contains(&message) {
            warnings.push(message);
        }
        cursors[asset_index] = asset.source_start;
        return asset.source_start;
    }

    let mut cursor = cursors[asset_index];
    if cursor.as_i64() + duration.as_i64() > asset.duration_ticks.as_i64() {
        cursor = asset.source_start;
    }
    let next = cursor.as_i64() + duration.as_i64();
    cursors[asset_index] = if next >= asset.duration_ticks.as_i64() {
        asset.source_start
    } else {
        Flicks(next)
    };
    cursor
}

fn frame_ceil(fps: Rational32, tick: Flicks) -> Result<Flicks, SchemaError> {
    let frame = match fps.ticks_to_frame_exact(tick).map_err(autocut_time_error)? {
        Some(frame) => frame,
        None => fps
            .ticks_to_frame_floor(tick)
            .map_err(autocut_time_error)?
            .checked_add(1)
            .ok_or_else(|| SchemaError::InvalidTimeline("autocut frame index overflow".into()))?,
    };
    fps.frame_to_ticks(frame).map_err(autocut_time_error)
}

fn frame_floor(fps: Rational32, tick: Flicks) -> Result<Flicks, SchemaError> {
    let frame = fps.ticks_to_frame_floor(tick).map_err(autocut_time_error)?;
    fps.frame_to_ticks(frame).map_err(autocut_time_error)
}

fn frame_nearest(fps: Rational32, tick: Flicks) -> Result<Flicks, SchemaError> {
    let frame = fps
        .ticks_to_frame_nearest(tick)
        .map_err(autocut_time_error)?;
    fps.frame_to_ticks(frame).map_err(autocut_time_error)
}

fn autocut_time_error(err: TimeError) -> SchemaError {
    SchemaError::InvalidTimeline(format!("autocut time math failed: {err}"))
}

fn autocut_add(left: Flicks, right: Flicks) -> Result<Flicks, SchemaError> {
    left.checked_add(right).map_err(autocut_time_error)
}

fn autocut_delta(later: Flicks, earlier: Flicks) -> Result<i64, SchemaError> {
    later
        .as_i64()
        .checked_sub(earlier.as_i64())
        .ok_or_else(|| SchemaError::InvalidTimeline("autocut time math overflow".into()))
}
