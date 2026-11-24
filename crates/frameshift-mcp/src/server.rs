use crate::error::McpError;
use frameshift_app::{
    AgentActor, AgentEditReceipt, AppError, NewReviewNote, PreviewArtifactState, ReviewNoteStore,
    ReviewNoteView, TickRange, apply_agent_command,
};
use frameshift_core::workbench::{
    PreflightReport, builtin_effect_chain_presets, effect_presets, find_effect_chain_preset,
    preflight, splice_effect_chain_preset,
};
use frameshift_core::{
    AutocutParams, Engine, InspectionPackManifest, MediaResolver, RenderOptions,
    VideoRenderSummary, plan_autocut,
};
use frameshift_media::{AudioAnalysisReport, ExportEncodingProfile};
use frameshift_schema::template::{EffectChainPreset, TemplateSlotValue};
use frameshift_schema::{
    CURRENT_SCHEMA_VERSION, NodeId, NodeKind, PreferenceEntry, PreferenceScope, Project,
    ProjectCommand, ProjectSession, RenderProfileKind, SchemaError, SchemaVersion,
};
use frameshift_time::{FLICKS_PER_SECOND, Flicks};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

/// Tool descriptor returned by `tools/list`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpToolDescriptor {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

/// Output of `describe_schema`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SchemaDescription {
    pub schema_version: SchemaVersion,
    pub revision_id: u64,
    pub project_name: String,
    pub fps_num: u32,
    pub fps_den: u32,
    pub duration_ticks: Flicks,
    pub node_kinds: Vec<String>,
    pub project_commands: Vec<String>,
    pub preference_scopes: Vec<String>,
    pub safe_expr_variables: Vec<String>,
    pub clip_ids: Vec<String>,
    pub event_ids: Vec<String>,
    pub node_ids: Vec<String>,
}

/// Output of `mutate_project`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutateProjectResult {
    pub revision_id: u64,
    pub project_name: String,
    pub creative_decisions_count: usize,
    pub preferences_count: usize,
    pub receipt: AgentEditReceipt,
}

/// Review notes pinned to the active project revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewNotesResult {
    pub current_revision_id: u64,
    pub notes: Vec<ReviewNoteView>,
}

/// Output of `query_preferences` preserving strict `Scene > Project > Global` precedence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreferenceQueryResult {
    pub key: String,
    pub scene_id: Option<String>,
    pub effective: Option<PreferenceEntry>,
    pub resolved_scope: Option<PreferenceScope>,
    pub scene_layer: Option<PreferenceEntry>,
    pub project_layer: Option<PreferenceEntry>,
    pub global_layer: Option<PreferenceEntry>,
}

/// Output of `render_project`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum RenderProjectResult {
    Frame {
        revision_id: u64,
        profile: RenderProfileKind,
        tick: Flicks,
        width: u32,
        height: u32,
        fnv1a_hash64: u64,
        out_path: PathBuf,
    },
    Video {
        revision_id: u64,
        summary: VideoRenderSummary,
    },
}

/// Output of `undo` and `redo`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndoRedoResult {
    pub operation: String,
    pub before_revision_id: u64,
    pub revision_id: u64,
    pub can_undo: bool,
    pub can_redo: bool,
    pub undo_len: usize,
    pub redo_len: usize,
    pub project_name: String,
}

/// Output of `plan_autocut`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanAutocutResult {
    pub start_tick: Flicks,
    pub duration_ticks: Flicks,
    pub min_segment_ticks: Flicks,
    pub include_start: bool,
    pub include_end: bool,
    pub detected_beats_count: usize,
    pub boundaries: Vec<Flicks>,
    pub segment_count: usize,
}

/// Optional parameters for [`McpServer::plan_autocut`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlanAutocutOptions {
    pub asset_id: Option<String>,
    pub window_ticks: Option<Flicks>,
    pub start_tick: Option<Flicks>,
    pub duration_ticks: Option<Flicks>,
    pub min_segment_ticks: Option<Flicks>,
    pub include_start: bool,
    pub include_end: bool,
}

/// Request payload for [`McpServer::apply_effect_preset`].
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ApplyEffectPresetRequest {
    pub expected_revision_id: u64,
    #[serde(default)]
    pub preset_id: Option<String>,
    #[serde(default)]
    pub preset: Option<EffectChainPreset>,
    #[serde(default)]
    pub target_node_id: Option<String>,
    #[serde(default)]
    pub node_id_prefix: Option<String>,
    #[serde(default)]
    pub slot_values: Option<BTreeMap<String, TemplateSlotValue>>,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub intent: Option<String>,
}

/// Stateful JSON-RPC 2.0 MCP Server wrapping a [`ProjectSession`] and lazy [`Engine`].
pub struct McpServer {
    session: ProjectSession,
    engine: Option<Engine>,
    resolver: MediaResolver,
    agent_edit_receipts: Vec<AgentEditReceipt>,
    review_notes: ReviewNoteStore,
}

impl McpServer {
    /// Creates an `McpServer` around `project` (validating it into a `ProjectSession`).
    pub fn new(project: Project) -> Result<Self, McpError> {
        let session = ProjectSession::new(project)?;
        Ok(Self {
            session,
            engine: None,
            resolver: MediaResolver::new(),
            agent_edit_receipts: Vec::new(),
            review_notes: ReviewNoteStore::default(),
        })
    }

    /// Creates an `McpServer` with an already-initialized [`Engine`].
    pub fn with_engine(project: Project, engine: Engine) -> Result<Self, McpError> {
        let session = ProjectSession::new(project)?;
        Ok(Self {
            session,
            engine: Some(engine),
            resolver: MediaResolver::new(),
            agent_edit_receipts: Vec::new(),
            review_notes: ReviewNoteStore::default(),
        })
    }

    /// Returns the active [`ProjectSession`].
    #[inline]
    pub fn session(&self) -> &ProjectSession {
        &self.session
    }

    /// Returns the active [`Project`].
    #[inline]
    pub fn project(&self) -> &Project {
        self.session.project()
    }

    /// Returns the active monotonic `revision_id`.
    #[inline]
    pub fn revision_id(&self) -> u64 {
        self.session.revision_id()
    }

    fn ensure_engine(&mut self) -> Result<&mut Engine, McpError> {
        if self.engine.is_none() {
            self.engine = Some(Engine::new_headless()?);
        }
        Ok(self.engine.as_mut().expect("engine initialized"))
    }

    /// Lists all MCP tools exposed by this server.
    pub fn list_tools(&self) -> Vec<McpToolDescriptor> {
        vec![
            McpToolDescriptor {
                name: "describe_schema".into(),
                description: "Describes FrameShift schema version, node kinds, commands, preference scopes, SafeExpr variables, and current project summary.".into(),
                input_schema: json!({ "type": "object", "properties": {} }),
            },
            McpToolDescriptor {
                name: "analyze_audio".into(),
                description: "Computes windowed RMS energy envelope, detected beat/onset ticks, and estimated BPM for a project audio asset or mix.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "asset_id": { "type": "string" },
                        "window_ticks": { "type": "integer" }
                    }
                }),
            },
            McpToolDescriptor {
                name: "inspect_project".into(),
                description: "Exports the multi-surface inspection pack (contact sheet, mask pack, transition pack, velocity pack, 1:1 fullres crop, epistemic manifest).".into(),
                input_schema: json!({
                    "type": "object",
                    "required": ["out_dir"],
                    "properties": {
                        "out_dir": { "type": "string" },
                        "expected_revision_id": { "type": "integer" }
                    }
                }),
            },
            McpToolDescriptor {
                name: "mutate_project".into(),
                description: "Applies an atomic ProjectCommand enforcing expected_revision_id optimistic locking and returns an automatic agent edit receipt with affected IDs/ranges, moved events, preserved creative decisions, and truthful preview status. Hard cuts are command author_hard_cuts: an ordered, non-overlapping cut list in integer Flicks. The session writes one SourceVideo per cut and one Blend chain.".into(),
                input_schema: json!({
                    "type": "object",
                    "required": ["expected_revision_id", "command"],
                    "properties": {
                        "expected_revision_id": { "type": "integer" },
                        "command": { "type": "object" },
                        "intent": { "type": "string" },
                        "agent_id": { "type": "string" }
                    }
                }),
            },
            McpToolDescriptor {
                name: "list_edit_receipts".into(),
                description: "Lists automatic agent edit receipts from this headless session, including before/after revision snapshots and preview artifact state.".into(),
                input_schema: json!({ "type": "object", "properties": {} }),
            },
            McpToolDescriptor {
                name: "add_review_note".into(),
                description: "Adds a review note only when expected_revision_id matches the active project; optional tick ranges are half-open Flicks ranges.".into(),
                input_schema: json!({
                    "type": "object",
                    "required": ["expected_revision_id", "body"],
                    "properties": {
                        "expected_revision_id": { "type": "integer" },
                        "author": { "type": "string" },
                        "body": { "type": "string" },
                        "start_tick": { "type": "integer" },
                        "end_tick": { "type": "integer" }
                    }
                }),
            },
            McpToolDescriptor {
                name: "list_review_notes".into(),
                description: "Lists review notes pinned to project revisions and reports each note as current or stale against the active revision.".into(),
                input_schema: json!({ "type": "object", "properties": {} }),
            },
            McpToolDescriptor {
                name: "query_preferences".into(),
                description: "Queries user/agent preferences with strict Scene > Project > Global precedence without leaking Scene preferences into Global.".into(),
                input_schema: json!({
                    "type": "object",
                    "required": ["key"],
                    "properties": {
                        "key": { "type": "string" },
                        "scene_id": { "type": "string" }
                    }
                }),
            },
            McpToolDescriptor {
                name: "undo".into(),
                description: "Undoes the most recent committed project command while advancing revision_id monotonically.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "expected_revision_id": { "type": "integer" }
                    }
                }),
            },
            McpToolDescriptor {
                name: "redo".into(),
                description: "Reapplies the most recently undone project command while advancing revision_id monotonically.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "expected_revision_id": { "type": "integer" }
                    }
                }),
            },
            McpToolDescriptor {
                name: "preflight_project".into(),
                description: "Runs schema validation, media asset checks, LUT path checks, and optional tool availability checks.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "check_tools": { "type": "boolean" },
                        "expected_revision_id": { "type": "integer" }
                    }
                }),
            },
            McpToolDescriptor {
                name: "plan_autocut".into(),
                description: "Plans beat-aligned cut boundaries over an explicit timeline window using project audio analysis.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "asset_id": { "type": "string" },
                        "window_ticks": { "type": "integer" },
                        "start_tick": { "type": "integer" },
                        "duration_ticks": { "type": "integer" },
                        "min_segment_ticks": { "type": "integer" },
                        "include_start": { "type": "boolean" },
                        "include_end": { "type": "boolean" }
                    }
                }),
            },
            McpToolDescriptor {
                name: "list_effect_presets".into(),
                description: "Lists single-node effect presets and multi-node anime/AMV effect chain presets with typed slots.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "category": { "type": "string" }
                    }
                }),
            },
            McpToolDescriptor {
                name: "apply_effect_preset".into(),
                description: "Validates and splices a built-in or inline EffectChainPreset into the render graph with typed slot values and optimistic locking.".into(),
                input_schema: json!({
                    "type": "object",
                    "required": ["expected_revision_id"],
                    "properties": {
                        "expected_revision_id": { "type": "integer" },
                        "preset_id": { "type": "string" },
                        "preset": { "type": "object" },
                        "target_node_id": { "type": "string" },
                        "node_id_prefix": { "type": "string" },
                        "slot_values": { "type": "object" },
                        "agent_id": { "type": "string" },
                        "intent": { "type": "string" }
                    }
                }),
            },
            McpToolDescriptor {
                name: "render_project".into(),
                description: "Renders either a single PNG frame at tick or a full timeline video (optionally with a named platform encoding preset).".into(),
                input_schema: json!({
                    "type": "object",
                    "required": ["profile", "out_path"],
                    "properties": {
                        "profile": { "type": "string", "enum": ["draft", "final"] },
                        "out_path": { "type": "string" },
                        "tick": { "type": "integer" },
                        "platform_preset": {
                            "type": "string",
                            "enum": [
                                "tiktok_1080p60",
                                "reels_1080p30",
                                "youtube_1080p60",
                                "anime_10bit_hevc",
                                "prores_422hq"
                            ]
                        },
                        "expected_revision_id": { "type": "integer" }
                    }
                }),
            },
        ]
    }

    /// Describes the schema capabilities and current project topology.
    pub fn describe_schema(&self) -> SchemaDescription {
        let p = self.session.project();
        SchemaDescription {
            schema_version: CURRENT_SCHEMA_VERSION,
            revision_id: p.revision_id,
            project_name: p.name.clone(),
            fps_num: p.timeline.fps.num,
            fps_den: p.timeline.fps.den,
            duration_ticks: p.timeline.duration_ticks,
            node_kinds: NodeKind::ALL_SERDE_NAMES
                .iter()
                .map(|&name| name.to_owned())
                .collect(),
            project_commands: vec![
                "set_project_name".into(),
                "add_asset".into(),
                "remove_asset".into(),
                "add_clip".into(),
                "remove_clip".into(),
                "trim_clip".into(),
                "split_clip".into(),
                "author_hard_cuts".into(),
                "set_clip_speed_curve".into(),
                "upsert_event_anchor".into(),
                "move_event_target".into(),
                "retarget_event".into(),
                "precompose".into(),
                "unprecompose".into(),
                "upsert_graph_node".into(),
                "remove_graph_node".into(),
                "set_output_node".into(),
                "record_creative_decision".into(),
                "set_preference".into(),
                "replace_project".into(),
            ],
            preference_scopes: vec!["global".into(), "project".into(), "scene".into()],
            safe_expr_variables: vec![
                "t_sec".into(),
                "output_tick".into(),
                "beat_phase".into(),
                "audio_rms".into(),
                "event_delta_sec".into(),
                "progress".into(),
            ],
            clip_ids: p.timeline.clips.iter().map(|c| c.id.clone()).collect(),
            event_ids: p.events.iter().map(|e| e.event_id.clone()).collect(),
            node_ids: p.graph.nodes.iter().map(|n| n.id.0.clone()).collect(),
        }
    }

    /// Analyzes beats, onset peaks, and RMS energy envelope on the specified audio asset or mixed project audio.
    pub fn analyze_audio(
        &self,
        asset_id: Option<&str>,
        window_ticks: Option<Flicks>,
    ) -> Result<AudioAnalysisReport, McpError> {
        let win = window_ticks.unwrap_or(Flicks(FLICKS_PER_SECOND / 100));
        let project = self.session.project();

        let audio = if let Some(aid) = asset_id {
            let asset = project
                .asset(aid)
                .ok_or_else(|| SchemaError::MissingAsset(aid.to_owned()))?;
            self.resolver
                .load_audio_asset(asset, project.timeline.duration_ticks)?
        } else {
            self.resolver.mix_project_audio(project)?.ok_or_else(|| {
                McpError::InvalidParams("project has no audio clips to analyze".into())
            })?
        };

        Ok(audio.analyze_beats_and_energy(win)?)
    }

    /// Exports the Phase 2 Multi-Surface Inspection Pack to `out_dir`.
    pub fn inspect_project(
        &mut self,
        out_dir: &Path,
        expected_revision_id: Option<u64>,
    ) -> Result<InspectionPackManifest, McpError> {
        if let Some(expected) = expected_revision_id {
            let actual = self.session.revision_id();
            if expected != actual {
                return Err(McpError::RevisionConflict { expected, actual });
            }
        }
        let project = self.session.project().clone();
        let engine = self.ensure_engine()?;
        Ok(engine.export_inspection_pack(&project, out_dir)?)
    }

    /// Mutates the project atomically with `expected_revision_id` optimistic locking.
    pub fn mutate_project(
        &mut self,
        expected_revision_id: u64,
        command: ProjectCommand,
    ) -> Result<MutateProjectResult, McpError> {
        self.mutate_project_as_agent(
            expected_revision_id,
            "mcp-agent".into(),
            "Apply an MCP project command".into(),
            command,
        )
    }

    fn mutate_project_as_agent(
        &mut self,
        expected_revision_id: u64,
        agent_id: String,
        intent: String,
        command: ProjectCommand,
    ) -> Result<MutateProjectResult, McpError> {
        let receipt = apply_agent_command(
            &mut self.session,
            expected_revision_id,
            AgentActor { agent_id },
            intent,
            command,
        )
        .map_err(map_app_error)?;

        let p = self.session.project();
        let result = MutateProjectResult {
            revision_id: receipt.after_revision_id,
            project_name: p.name.clone(),
            creative_decisions_count: p.creative_decisions.len(),
            preferences_count: p.preferences.len(),
            receipt: receipt.clone(),
        };
        self.agent_edit_receipts.push(receipt);
        Ok(result)
    }

    /// Returns receipts produced by successful agent edits in this MCP session.
    pub fn list_edit_receipts(&self) -> &[AgentEditReceipt] {
        &self.agent_edit_receipts
    }

    /// Updates truthful preview artifact status after an optional headless preview pass.
    /// Returns `false` when `receipt_id` is not present in this session.
    pub fn set_receipt_preview_artifacts(
        &mut self,
        receipt_id: &str,
        visual: PreviewArtifactState,
        audio: PreviewArtifactState,
    ) -> bool {
        let Some(receipt) = self
            .agent_edit_receipts
            .iter_mut()
            .find(|receipt| receipt.receipt_id == receipt_id)
        else {
            return false;
        };
        receipt.preview.visual_artifact = visual;
        receipt.preview.audio_artifact = audio;
        true
    }

    /// Adds a review note pinned to the active revision.
    pub fn add_review_note(
        &mut self,
        expected_revision_id: u64,
        author: String,
        body: String,
        range: Option<TickRange>,
    ) -> Result<ReviewNoteView, McpError> {
        self.review_notes
            .add(
                &self.session,
                NewReviewNote {
                    expected_revision_id,
                    author,
                    body,
                    range,
                },
            )
            .map_err(map_app_error)
    }

    /// Lists review notes and marks notes from older revisions as stale.
    pub fn list_review_notes(&self) -> ReviewNotesResult {
        ReviewNotesResult {
            current_revision_id: self.session.revision_id(),
            notes: self.review_notes.list(self.session.revision_id()),
        }
    }

    /// Queries preferences for `key` under strict `Scene > Project > Global` hierarchy.
    ///
    /// A `Scene`-scoped preference only matches when `scene_id` matches `target_id` and
    /// never leaks into `project_layer` or `global_layer`.
    pub fn query_preferences(&self, key: &str, scene_id: Option<&str>) -> PreferenceQueryResult {
        let prefs = &self.session.project().preferences;

        let scene_layer = scene_id.and_then(|sid| {
            prefs
                .iter()
                .rev()
                .find(|e| {
                    e.key == key
                        && e.scope == PreferenceScope::Scene
                        && e.target_id.as_deref() == Some(sid)
                })
                .cloned()
        });

        let project_layer = prefs
            .iter()
            .rev()
            .find(|e| e.key == key && e.scope == PreferenceScope::Project)
            .cloned();

        let global_layer = prefs
            .iter()
            .rev()
            .find(|e| e.key == key && e.scope == PreferenceScope::Global)
            .cloned();

        let effective = scene_layer
            .clone()
            .or_else(|| project_layer.clone())
            .or_else(|| global_layer.clone());
        let resolved_scope = effective.as_ref().map(|e| e.scope);

        PreferenceQueryResult {
            key: key.to_owned(),
            scene_id: scene_id.map(str::to_owned),
            effective,
            resolved_scope,
            scene_layer,
            project_layer,
            global_layer,
        }
    }

    fn check_expected_revision(&self, expected_revision_id: Option<u64>) -> Result<u64, McpError> {
        let actual = self.session.revision_id();
        if let Some(expected) = expected_revision_id
            && expected != actual
        {
            return Err(McpError::RevisionConflict { expected, actual });
        }
        Ok(actual)
    }

    fn apply_undo_or_redo(
        &mut self,
        is_undo: bool,
        expected_revision_id: Option<u64>,
    ) -> Result<UndoRedoResult, McpError> {
        let before_rev = self.check_expected_revision(expected_revision_id)?;
        let (operation, next_rev) = if is_undo {
            ("undo", self.session.undo(before_rev)?)
        } else {
            ("redo", self.session.redo(before_rev)?)
        };
        Ok(UndoRedoResult {
            operation: operation.to_owned(),
            before_revision_id: before_rev,
            revision_id: next_rev,
            can_undo: self.session.can_undo(),
            can_redo: self.session.can_redo(),
            undo_len: self.session.undo_len(),
            redo_len: self.session.redo_len(),
            project_name: self.session.project().name.clone(),
        })
    }

    /// Undoes the most recent committed project command.
    pub fn undo(&mut self, expected_revision_id: Option<u64>) -> Result<UndoRedoResult, McpError> {
        self.apply_undo_or_redo(true, expected_revision_id)
    }

    /// Reapplies the most recently undone project command.
    pub fn redo(&mut self, expected_revision_id: Option<u64>) -> Result<UndoRedoResult, McpError> {
        self.apply_undo_or_redo(false, expected_revision_id)
    }

    /// Runs schema, asset, LUT, and optional tool preflight checks against the active project.
    pub fn preflight_project(
        &self,
        check_tools: bool,
        expected_revision_id: Option<u64>,
    ) -> Result<PreflightReport, McpError> {
        self.check_expected_revision(expected_revision_id)?;
        Ok(preflight(self.session.project(), check_tools))
    }

    /// Plans beat-aligned cut boundaries over an explicit timeline window.
    pub fn plan_autocut(
        &self,
        options: PlanAutocutOptions,
    ) -> Result<PlanAutocutResult, McpError> {
        let report = self.analyze_audio(options.asset_id.as_deref(), options.window_ticks)?;
        let start_tick = options.start_tick.unwrap_or(Flicks::ZERO);
        let duration_ticks = options.duration_ticks.unwrap_or_else(|| {
            Flicks(
                self.session
                    .project()
                    .timeline
                    .duration_ticks
                    .0
                    .saturating_sub(start_tick.0),
            )
        });
        let min_segment_ticks = options.min_segment_ticks.unwrap_or(Flicks::ZERO);
        let params = AutocutParams {
            start_tick,
            duration_ticks,
            min_segment_ticks,
            include_start: options.include_start,
            include_end: options.include_end,
        };
        let boundaries = plan_autocut(&report, params)?;
        Ok(PlanAutocutResult {
            start_tick,
            duration_ticks,
            min_segment_ticks,
            include_start: options.include_start,
            include_end: options.include_end,
            detected_beats_count: report.detected_beats.len(),
            segment_count: boundaries.len().saturating_sub(1),
            boundaries,
        })
    }

    /// Lists single-node presets and built-in multi-node effect chain presets.
    pub fn list_effect_presets(&self, category: Option<&str>) -> Value {
        let single_node_presets = effect_presets();
        let mut chain_presets = builtin_effect_chain_presets();
        if let Some(cat) = category.map(str::trim).filter(|c| !c.is_empty()) {
            chain_presets.retain(|preset| preset.category.eq_ignore_ascii_case(cat));
        }
        json!({
            "single_node_presets": single_node_presets,
            "chain_presets": chain_presets,
        })
    }

    /// Validates and splices an [`EffectChainPreset`] into the render graph with optimistic locking.
    pub fn apply_effect_preset(
        &mut self,
        req: ApplyEffectPresetRequest,
    ) -> Result<MutateProjectResult, McpError> {
        self.check_expected_revision(Some(req.expected_revision_id))?;
        let preset = match (req.preset_id.as_deref(), req.preset) {
            (Some(_), Some(_)) => {
                return Err(McpError::InvalidParams(
                    "provide either 'preset_id' or 'preset', not both".into(),
                ));
            }
            (None, None) => {
                return Err(McpError::InvalidParams(
                    "either 'preset_id' or 'preset' is required".into(),
                ));
            }
            (Some(preset_id), None) => find_effect_chain_preset(preset_id).ok_or_else(|| {
                McpError::InvalidParams(format!("unknown effect chain preset '{preset_id}'"))
            })?,
            (None, Some(inline)) => inline,
        };

        let target = req.target_node_id.map(NodeId);
        let prefix = req.node_id_prefix.unwrap_or_else(|| preset.id.clone());
        let cmd = splice_effect_chain_preset(
            self.session.project(),
            target.as_ref(),
            &prefix,
            &preset,
            req.slot_values.as_ref(),
        )?;
        let agent_id = req.agent_id.unwrap_or_else(|| "mcp-agent".into());
        let intent = req
            .intent
            .unwrap_or_else(|| format!("Apply effect chain preset '{}'", preset.id));
        self.mutate_project_as_agent(req.expected_revision_id, agent_id, intent, cmd)
    }

    /// Renders a single PNG frame (`tick = Some(..)`) or full MP4 (`tick = None`).
    pub fn render_project(
        &mut self,
        profile: RenderProfileKind,
        out_path: &Path,
        tick: Option<Flicks>,
    ) -> Result<RenderProjectResult, McpError> {
        self.render_project_with_options(profile, out_path, tick, None, None)
    }

    /// Renders a frame or video with optional platform encoding preset and optimistic revision lock.
    pub fn render_project_with_options(
        &mut self,
        profile: RenderProfileKind,
        out_path: &Path,
        tick: Option<Flicks>,
        platform_preset: Option<&str>,
        expected_revision_id: Option<u64>,
    ) -> Result<RenderProjectResult, McpError> {
        let rev = self.check_expected_revision(expected_revision_id)?;
        let encoding_profile = if let Some(preset_name) = platform_preset {
            if tick.is_some() {
                return Err(McpError::InvalidParams(
                    "platform_preset is only supported for video renders (when tick is omitted)"
                        .into(),
                ));
            }
            Some(
                ExportEncodingProfile::from_platform_name(preset_name).ok_or_else(|| {
                    McpError::InvalidParams(format!(
                        "unknown platform_preset '{preset_name}'; expected one of tiktok_1080p60, reels_1080p30, youtube_1080p60, anime_10bit_hevc, prores_422hq"
                    ))
                })?,
            )
        } else {
            None
        };

        let project = self.session.project().clone();
        let engine = self.ensure_engine()?;

        if let Some(t) = tick {
            let frame = engine.render_frame_at(&project, profile, t)?;
            if let Some(parent) = out_path.parent()
                && !parent.as_os_str().is_empty()
            {
                std::fs::create_dir_all(parent)?;
            }
            frame.save_png(out_path)?;
            Ok(RenderProjectResult::Frame {
                revision_id: rev,
                profile,
                tick: t,
                width: frame.width,
                height: frame.height,
                fnv1a_hash64: frame.fnv1a_hash64(),
                out_path: out_path.to_path_buf(),
            })
        } else {
            let summary = if let Some(enc) = encoding_profile.as_ref() {
                engine.render_video_with_profile(
                    &project,
                    profile,
                    out_path,
                    None,
                    &RenderOptions::default(),
                    enc,
                    &mut |_| {},
                )?
            } else {
                engine.render_video_mp4(&project, profile, out_path)?
            };
            Ok(RenderProjectResult::Video {
                revision_id: rev,
                summary,
            })
        }
    }

    /// Processes a single JSON-RPC 2.0 request line and returns a serialized JSON-RPC 2.0 response line.
    pub fn handle_jsonrpc_str(&mut self, line: &str) -> String {
        let req: JsonRpcRequest = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(err) => {
                return jsonrpc_error(Value::Null, -32700, format!("parse error: {err}"), None);
            }
        };

        if req.jsonrpc != "2.0" {
            return jsonrpc_error(
                req.id,
                -32600,
                format!("unsupported jsonrpc version '{}'", req.jsonrpc),
                None,
            );
        }

        let id = req.id.clone();
        match self.dispatch_jsonrpc(&req.method, req.params) {
            Ok(result) => serde_json::to_string(&json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": result,
            }))
            .unwrap_or_else(|e| {
                jsonrpc_error(id, -32603, format!("serialization error: {e}"), None)
            }),
            Err(err) => match err {
                RpcDispatchError::MethodNotFound(m) => {
                    jsonrpc_error(id, -32601, format!("method not found: {m}"), None)
                }
                RpcDispatchError::InvalidParams(msg) => jsonrpc_error(id, -32602, msg, None),
                RpcDispatchError::App(McpError::RevisionConflict { expected, actual }) => {
                    jsonrpc_error(
                        id,
                        -32009,
                        format!("revision conflict: expected {expected}, actual {actual}"),
                        Some(json!({
                            "kind": "revision_conflict",
                            "expected_revision_id": expected,
                            "actual_revision_id": actual,
                        })),
                    )
                }
                RpcDispatchError::App(other) => jsonrpc_error(id, -32000, other.to_string(), None),
            },
        }
    }

    /// Runs a blocking line-delimited JSON-RPC 2.0 stdio server loop until EOF.
    pub fn run_stdio(&mut self) -> Result<(), McpError> {
        let stdin = io::stdin();
        let mut stdout = io::stdout();
        for line_res in stdin.lock().lines() {
            let line = line_res?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let resp = self.handle_jsonrpc_str(trimmed);
            writeln!(stdout, "{resp}")?;
            stdout.flush()?;
        }
        Ok(())
    }

    fn dispatch_jsonrpc(
        &mut self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, RpcDispatchError> {
        match method {
            "initialize" => Ok(json!({
                "protocolVersion": "2024-11-05",
                "serverInfo": {
                    "name": "frameshift-mcp",
                    "version": env!("CARGO_PKG_VERSION"),
                },
                "capabilities": {
                    "tools": {}
                }
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": self.list_tools() })),
            "tools/call" => {
                let call: ToolCallParams = parse_params(params)?;
                let res = self.dispatch_tool(&call.name, call.arguments)?;
                Ok(json!({
                    "content": [{
                        "type": "text",
                        "text": serde_json::to_string(&res).map_err(|e| RpcDispatchError::App(e.into()))?,
                    }],
                    "structuredContent": res,
                    "isError": false,
                }))
            }
            "describe_schema"
            | "analyze_audio"
            | "inspect_project"
            | "mutate_project"
            | "list_edit_receipts"
            | "add_review_note"
            | "list_review_notes"
            | "query_preferences"
            | "undo"
            | "redo"
            | "preflight_project"
            | "plan_autocut"
            | "list_effect_presets"
            | "apply_effect_preset"
            | "render_project" => {
                self.dispatch_tool(method, params.unwrap_or_else(|| json!({})))
            }
            other => Err(RpcDispatchError::MethodNotFound(other.to_owned())),
        }
    }

    fn dispatch_tool(&mut self, name: &str, args: Value) -> Result<Value, RpcDispatchError> {
        match name {
            "describe_schema" => {
                let desc = self.describe_schema();
                to_json_value(desc)
            }
            "analyze_audio" => {
                let p: AnalyzeAudioParams = serde_json::from_value(args)
                    .map_err(|e| RpcDispatchError::InvalidParams(e.to_string()))?;
                let report = self
                    .analyze_audio(p.asset_id.as_deref(), p.window_ticks.map(Flicks))
                    .map_err(RpcDispatchError::App)?;
                to_json_value(report)
            }
            "inspect_project" => {
                let p: InspectProjectParams = serde_json::from_value(args)
                    .map_err(|e| RpcDispatchError::InvalidParams(e.to_string()))?;
                let manifest = self
                    .inspect_project(Path::new(&p.out_dir), p.expected_revision_id)
                    .map_err(RpcDispatchError::App)?;
                to_json_value(manifest)
            }
            "mutate_project" => {
                let p: MutateProjectParams = serde_json::from_value(args)
                    .map_err(|e| RpcDispatchError::InvalidParams(e.to_string()))?;
                let res = self
                    .mutate_project_as_agent(
                        p.expected_revision_id,
                        p.agent_id.unwrap_or_else(|| "mcp-agent".into()),
                        p.intent
                            .unwrap_or_else(|| "Apply an MCP project command".into()),
                        p.command,
                    )
                    .map_err(RpcDispatchError::App)?;
                to_json_value(res)
            }
            "list_edit_receipts" => to_json_value(self.list_edit_receipts()),
            "add_review_note" => {
                let p: AddReviewNoteParams = serde_json::from_value(args)
                    .map_err(|e| RpcDispatchError::InvalidParams(e.to_string()))?;
                let range = match (p.start_tick, p.end_tick) {
                    (Some(start), Some(end)) => Some(TickRange {
                        start_tick: Flicks(start),
                        end_tick: Flicks(end),
                    }),
                    (None, None) => None,
                    _ => {
                        return Err(RpcDispatchError::InvalidParams(
                            "start_tick and end_tick must be supplied together".into(),
                        ));
                    }
                };
                let note = self
                    .add_review_note(
                        p.expected_revision_id,
                        p.author.unwrap_or_else(|| "mcp-reviewer".into()),
                        p.body,
                        range,
                    )
                    .map_err(RpcDispatchError::App)?;
                to_json_value(note)
            }
            "list_review_notes" => to_json_value(self.list_review_notes()),
            "query_preferences" => {
                let p: QueryPreferencesParams = serde_json::from_value(args)
                    .map_err(|e| RpcDispatchError::InvalidParams(e.to_string()))?;
                let res = self.query_preferences(&p.key, p.scene_id.as_deref());
                to_json_value(res)
            }
            "undo" => {
                let p: RevisionCheckParams = serde_json::from_value(args)
                    .map_err(|e| RpcDispatchError::InvalidParams(e.to_string()))?;
                let res = self
                    .undo(p.expected_revision_id)
                    .map_err(RpcDispatchError::App)?;
                to_json_value(res)
            }
            "redo" => {
                let p: RevisionCheckParams = serde_json::from_value(args)
                    .map_err(|e| RpcDispatchError::InvalidParams(e.to_string()))?;
                let res = self
                    .redo(p.expected_revision_id)
                    .map_err(RpcDispatchError::App)?;
                to_json_value(res)
            }
            "preflight_project" => {
                let p: PreflightProjectParams = serde_json::from_value(args)
                    .map_err(|e| RpcDispatchError::InvalidParams(e.to_string()))?;
                let res = self
                    .preflight_project(p.check_tools, p.expected_revision_id)
                    .map_err(RpcDispatchError::App)?;
                to_json_value(res)
            }
            "plan_autocut" => {
                let p: PlanAutocutParams = serde_json::from_value(args)
                    .map_err(|e| RpcDispatchError::InvalidParams(e.to_string()))?;
                let res = self
                    .plan_autocut(PlanAutocutOptions {
                        asset_id: p.asset_id,
                        window_ticks: p.window_ticks.map(Flicks),
                        start_tick: p.start_tick.map(Flicks),
                        duration_ticks: p.duration_ticks.map(Flicks),
                        min_segment_ticks: p.min_segment_ticks.map(Flicks),
                        include_start: p.include_start,
                        include_end: p.include_end,
                    })
                    .map_err(RpcDispatchError::App)?;
                to_json_value(res)
            }
            "list_effect_presets" => {
                let p: ListEffectPresetsParams = serde_json::from_value(args)
                    .map_err(|e| RpcDispatchError::InvalidParams(e.to_string()))?;
                Ok(self.list_effect_presets(p.category.as_deref()))
            }
            "apply_effect_preset" => {
                let p: ApplyEffectPresetRequest = serde_json::from_value(args)
                    .map_err(|e| RpcDispatchError::InvalidParams(e.to_string()))?;
                let res = self
                    .apply_effect_preset(p)
                    .map_err(RpcDispatchError::App)?;
                to_json_value(res)
            }
            "render_project" => {
                let p: RenderProjectParams = serde_json::from_value(args)
                    .map_err(|e| RpcDispatchError::InvalidParams(e.to_string()))?;
                let res = self
                    .render_project_with_options(
                        p.profile,
                        Path::new(&p.out_path),
                        p.tick.map(Flicks),
                        p.platform_preset.as_deref(),
                        p.expected_revision_id,
                    )
                    .map_err(RpcDispatchError::App)?;
                to_json_value(res)
            }
            other => Err(RpcDispatchError::MethodNotFound(other.to_owned())),
        }
    }
}

fn to_json_value<T: Serialize>(value: T) -> Result<Value, RpcDispatchError> {
    serde_json::to_value(value).map_err(|e| RpcDispatchError::App(McpError::Json(e)))
}

fn parse_params<T: for<'de> Deserialize<'de>>(
    params: Option<Value>,
) -> Result<T, RpcDispatchError> {
    let val = params.unwrap_or_else(|| json!({}));
    serde_json::from_value(val).map_err(|e| RpcDispatchError::InvalidParams(e.to_string()))
}

fn jsonrpc_error(id: Value, code: i64, message: String, data: Option<Value>) -> String {
    let err_obj = if let Some(d) = data {
        json!({ "code": code, "message": message, "data": d })
    } else {
        json!({ "code": code, "message": message })
    };
    serde_json::to_string(&json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": err_obj,
    }))
    .unwrap_or_else(|_| r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"internal serialization failure"}}"#.to_owned())
}

enum RpcDispatchError {
    MethodNotFound(String),
    InvalidParams(String),
    App(McpError),
}

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    #[serde(default)]
    id: Value,
    method: String,
    #[serde(default)]
    params: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct ToolCallParams {
    name: String,
    #[serde(default)]
    arguments: Value,
}

#[derive(Debug, Deserialize)]
struct AnalyzeAudioParams {
    #[serde(default)]
    asset_id: Option<String>,
    #[serde(default)]
    window_ticks: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct InspectProjectParams {
    out_dir: String,
    #[serde(default)]
    expected_revision_id: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct MutateProjectParams {
    expected_revision_id: u64,
    command: ProjectCommand,
    #[serde(default)]
    intent: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AddReviewNoteParams {
    expected_revision_id: u64,
    body: String,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    start_tick: Option<i64>,
    #[serde(default)]
    end_tick: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct QueryPreferencesParams {
    key: String,
    #[serde(default)]
    scene_id: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RevisionCheckParams {
    #[serde(default)]
    expected_revision_id: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
struct PreflightProjectParams {
    #[serde(default)]
    check_tools: bool,
    #[serde(default)]
    expected_revision_id: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
struct PlanAutocutParams {
    #[serde(default)]
    asset_id: Option<String>,
    #[serde(default)]
    window_ticks: Option<i64>,
    #[serde(default)]
    start_tick: Option<i64>,
    #[serde(default)]
    duration_ticks: Option<i64>,
    #[serde(default)]
    min_segment_ticks: Option<i64>,
    #[serde(default)]
    include_start: bool,
    #[serde(default)]
    include_end: bool,
}

#[derive(Debug, Default, Deserialize)]
struct ListEffectPresetsParams {
    #[serde(default)]
    category: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RenderProjectParams {
    profile: RenderProfileKind,
    out_path: String,
    #[serde(default)]
    tick: Option<i64>,
    #[serde(default)]
    platform_preset: Option<String>,
    #[serde(default)]
    expected_revision_id: Option<u64>,
}

fn map_app_error(error: AppError) -> McpError {
    match error {
        AppError::ConcurrencyConflict { expected, actual } => {
            McpError::RevisionConflict { expected, actual }
        }
        AppError::Schema(schema_error) => McpError::Schema(schema_error),
        other => McpError::InvalidParams(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use frameshift_app::{ReviewNoteFreshness, TickRange};

    #[test]
    fn describe_schema_lists_all_node_kinds() {
        let server = McpServer::new(Project::golden_scene_preset()).expect("valid server");
        let desc = server.describe_schema();
        for &name in NodeKind::ALL_SERDE_NAMES {
            assert!(
                desc.node_kinds.iter().any(|k| k == name),
                "describe_schema missing node kind '{name}'"
            );
        }
        assert_eq!(desc.node_kinds.len(), NodeKind::ALL_SERDE_NAMES.len());
    }

    #[test]
    fn mutate_project_returns_and_retains_one_revision_checked_receipt() {
        let mut server = McpServer::new(Project::golden_scene_preset()).expect("valid server");
        let initial_revision = server.revision_id();
        let result = server
            .mutate_project(
                initial_revision,
                ProjectCommand::SetProjectName {
                    name: "Agent edit".into(),
                },
            )
            .expect("mutation applied");

        assert_eq!(result.revision_id, initial_revision + 1);
        assert_eq!(result.receipt.before_revision_id, initial_revision);
        assert_eq!(result.receipt.after_revision_id, initial_revision + 1);
        assert_eq!(result.receipt.action, "set_project_name");
        assert_eq!(result.receipt.actor.agent_id, "mcp-agent");
        assert_eq!(server.list_edit_receipts(), &[result.receipt]);
        assert_eq!(server.session().undo_len(), 1);

        let stale = server.mutate_project(
            initial_revision,
            ProjectCommand::SetProjectName {
                name: "must not apply".into(),
            },
        );
        assert!(matches!(
            stale,
            Err(McpError::RevisionConflict {
                expected,
                actual
            })
            if expected == initial_revision && actual == initial_revision + 1
        ));
        assert_eq!(server.project().name, "Agent edit");
        assert_eq!(server.list_edit_receipts().len(), 1);
    }

    #[test]
    fn review_note_surface_marks_notes_stale_after_revision_changes() {
        let mut server = McpServer::new(Project::golden_scene_preset()).expect("valid server");
        let initial_revision = server.revision_id();
        let note = server
            .add_review_note(
                initial_revision,
                "reviewer".into(),
                "Check this cut".into(),
                Some(TickRange {
                    start_tick: Flicks(0),
                    end_tick: Flicks(1),
                }),
            )
            .expect("note added");
        assert_eq!(note.freshness, ReviewNoteFreshness::Current);

        server
            .mutate_project(
                initial_revision,
                ProjectCommand::SetProjectName {
                    name: "New revision".into(),
                },
            )
            .expect("revision advanced");
        let listed = server.list_review_notes();
        assert_eq!(listed.current_revision_id, initial_revision + 1);
        assert_eq!(
            listed.notes[0].freshness,
            ReviewNoteFreshness::Stale {
                current_revision_id: initial_revision + 1
            }
        );
    }

    #[test]
    fn jsonrpc_tool_calls_expose_receipts_and_revision_bound_notes() {
        let mut server = McpServer::new(Project::golden_scene_preset()).expect("valid server");
        let revision = server.revision_id();
        let mutate = server.handle_jsonrpc_str(
            &json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {
                    "name": "mutate_project",
                    "arguments": {
                        "expected_revision_id": revision,
                        "intent": "rename the project for delivery",
                        "agent_id": "director",
                        "command": {
                            "command": "set_project_name",
                            "name": "RPC edited"
                        }
                    }
                }
            })
            .to_string(),
        );
        let mutate: Value = serde_json::from_str(&mutate).unwrap();
        let receipt = &mutate["result"]["structuredContent"]["receipt"];
        assert_eq!(receipt["intent"], "rename the project for delivery");
        assert_eq!(receipt["actor"]["agent_id"], "director");
        assert_eq!(receipt["action"], "set_project_name");
        assert_eq!(receipt["before_revision_id"], revision);
        assert_eq!(receipt["after_revision_id"], revision + 1);

        let add_note = server.handle_jsonrpc_str(
            &json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {
                    "name": "add_review_note",
                    "arguments": {
                        "expected_revision_id": revision + 1,
                        "author": "human-reviewer",
                        "body": "Review the new title card",
                        "start_tick": 100,
                        "end_tick": 200
                    }
                }
            })
            .to_string(),
        );
        let add_note: Value = serde_json::from_str(&add_note).unwrap();
        assert_eq!(
            add_note["result"]["structuredContent"]["note"]["revision_id"],
            revision + 1
        );
        assert_eq!(
            add_note["result"]["structuredContent"]["freshness"]["status"],
            "current"
        );

        let listed = server.handle_jsonrpc_str(
            &json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": { "name": "list_review_notes", "arguments": {} }
            })
            .to_string(),
        );
        let listed: Value = serde_json::from_str(&listed).unwrap();
        assert_eq!(
            listed["result"]["structuredContent"]["notes"][0]["note"]["body"],
            "Review the new title card"
        );
    }

    #[test]
    fn apply_effect_preset_rejects_custom_wgsl_and_undo_checks_stack_and_revision() {
        let project = Project::golden_scene_preset();
        let mut server = McpServer::new(project).expect("valid server");
        let rev = server.revision_id();

        let err = server.undo(Some(rev)).expect_err("empty undo stack must fail");
        assert!(matches!(err, McpError::Schema(SchemaError::EmptyUndoStack)));

        let err = server
            .undo(Some(rev + 99))
            .expect_err("wrong revision must return RevisionConflict");
        assert!(matches!(err, McpError::RevisionConflict { .. }));

        let wgsl_preset = EffectChainPreset {
            schema_version: CURRENT_SCHEMA_VERSION,
            id: "bad_custom".into(),
            name: "Bad Custom WGSL".into(),
            category: "custom".into(),
            nodes: vec![
                frameshift_schema::GraphNodeDef {
                    id: NodeId::from("in_0"),
                    kind: NodeKind::PrecompInput { slot: 0 },
                    inputs: Vec::new(),
                    enabled: true,
                },
                frameshift_schema::GraphNodeDef {
                    id: NodeId::from("wgsl"),
                    kind: NodeKind::CustomWgsl {
                        wgsl_source: "// custom".into(),
                        expected_inputs: 1,
                        params: [0.0; 16],
                    },
                    inputs: vec![NodeId::from("in_0")],
                    enabled: true,
                },
            ],
            param_bindings: Vec::new(),
            slots: Vec::new(),
            review: frameshift_schema::template::TemplateReview::default(),
        };
        let wgsl_err = server
            .apply_effect_preset(ApplyEffectPresetRequest {
                expected_revision_id: rev,
                preset_id: None,
                preset: Some(wgsl_preset),
                target_node_id: None,
                node_id_prefix: None,
                slot_values: None,
                agent_id: None,
                intent: None,
            })
            .expect_err("CustomWgsl in EffectChainPreset must be rejected");
        assert!(wgsl_err.to_string().contains("CustomWgsl") || wgsl_err.to_string().contains("shader"));

        let render_err = server
            .render_project_with_options(
                RenderProfileKind::Draft,
                Path::new("frame.png"),
                Some(Flicks::ZERO),
                Some("tiktok_1080p60"),
                Some(rev),
            )
            .expect_err("platform_preset with tick=Some must be rejected");
        assert!(matches!(render_err, McpError::InvalidParams(_)));
    }
}
