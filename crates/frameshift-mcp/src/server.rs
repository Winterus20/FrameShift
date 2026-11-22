use crate::error::McpError;
use frameshift_core::{Engine, InspectionPackManifest, MediaResolver, VideoRenderSummary};
use frameshift_media::AudioAnalysisReport;
use frameshift_schema::{
    NodeKind, PreferenceEntry, PreferenceScope, Project, ProjectCommand, ProjectSession,
    RenderProfileKind, SchemaError, SchemaVersion, CURRENT_SCHEMA_VERSION,
};
use frameshift_time::{Flicks, FLICKS_PER_SECOND};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
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

/// Stateful JSON-RPC 2.0 MCP Server wrapping a [`ProjectSession`] and lazy [`Engine`].
pub struct McpServer {
    session: ProjectSession,
    engine: Option<Engine>,
    resolver: MediaResolver,
}

impl McpServer {
    /// Creates an `McpServer` around `project` (validating it into a `ProjectSession`).
    pub fn new(project: Project) -> Result<Self, McpError> {
        let session = ProjectSession::new(project)?;
        Ok(Self {
            session,
            engine: None,
            resolver: MediaResolver::new(),
        })
    }

    /// Creates an `McpServer` with an already-initialized [`Engine`].
    pub fn with_engine(project: Project, engine: Engine) -> Result<Self, McpError> {
        let session = ProjectSession::new(project)?;
        Ok(Self {
            session,
            engine: Some(engine),
            resolver: MediaResolver::new(),
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
                description: "Applies an atomic ProjectCommand enforcing expected_revision_id optimistic locking. Hard cuts are command author_hard_cuts: an ordered, non-overlapping cut list in integer Flicks. The session writes one SourceVideo per cut and one Blend chain.".into(),
                input_schema: json!({
                    "type": "object",
                    "required": ["expected_revision_id", "command"],
                    "properties": {
                        "expected_revision_id": { "type": "integer" },
                        "command": { "type": "object" }
                    }
                }),
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
                name: "render_project".into(),
                description: "Renders either a single PNG frame at tick or a full timeline MP4 video.".into(),
                input_schema: json!({
                    "type": "object",
                    "required": ["profile", "out_path"],
                    "properties": {
                        "profile": { "type": "string", "enum": ["draft", "final"] },
                        "out_path": { "type": "string" },
                        "tick": { "type": "integer" }
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
                "author_hard_cuts".into(),
                "set_clip_speed_curve".into(),
                "upsert_event_anchor".into(),
                "move_event_target".into(),
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
        let new_rev = match self.session.apply_command(expected_revision_id, command) {
            Ok(rev) => rev,
            Err(SchemaError::RevisionConflict { expected, actual }) => {
                return Err(McpError::RevisionConflict { expected, actual });
            }
            Err(other) => return Err(McpError::Schema(other)),
        };

        let p = self.session.project();
        Ok(MutateProjectResult {
            revision_id: new_rev,
            project_name: p.name.clone(),
            creative_decisions_count: p.creative_decisions.len(),
            preferences_count: p.preferences.len(),
        })
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

    /// Renders a single PNG frame (`tick = Some(..)`) or full MP4 (`tick = None`).
    pub fn render_project(
        &mut self,
        profile: RenderProfileKind,
        out_path: &Path,
        tick: Option<Flicks>,
    ) -> Result<RenderProjectResult, McpError> {
        let project = self.session.project().clone();
        let rev = project.revision_id;
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
            let summary = engine.render_video_mp4(&project, profile, out_path)?;
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
            "describe_schema" | "analyze_audio" | "inspect_project" | "mutate_project"
            | "query_preferences" | "render_project" => {
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
                    .mutate_project(p.expected_revision_id, p.command)
                    .map_err(RpcDispatchError::App)?;
                to_json_value(res)
            }
            "query_preferences" => {
                let p: QueryPreferencesParams = serde_json::from_value(args)
                    .map_err(|e| RpcDispatchError::InvalidParams(e.to_string()))?;
                let res = self.query_preferences(&p.key, p.scene_id.as_deref());
                to_json_value(res)
            }
            "render_project" => {
                let p: RenderProjectParams = serde_json::from_value(args)
                    .map_err(|e| RpcDispatchError::InvalidParams(e.to_string()))?;
                let res = self
                    .render_project(p.profile, Path::new(&p.out_path), p.tick.map(Flicks))
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
}

#[derive(Debug, Deserialize)]
struct QueryPreferencesParams {
    key: String,
    #[serde(default)]
    scene_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RenderProjectParams {
    profile: RenderProfileKind,
    out_path: String,
    #[serde(default)]
    tick: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
