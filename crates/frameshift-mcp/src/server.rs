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
    AiManifestRunConfig, AutocutParams, DslCompileOptions, Engine, InspectionPackManifest,
    LintOptions, MediaResolver, NleExportOptions, NleImportOptions, PlanarTrackProjectOptions,
    PointTrackProjectOptions, RenderOptions, TrackPlanarOptions, TrackPointOptions,
    TrackerBindTarget, VideoRenderSummary, calculate_template_metadata, compile_motion_str,
    diff_frame_files, discover_ai_models, export_project_to_fcpxml, export_project_to_otio_json,
    import_project_from_fcpxml, import_project_from_otio_json, instantiate_parametric_template,
    lint_project, nle_markers_from_beats, plan_autocut, run_manifest_model,
    run_manifest_model_by_id,
};
use frameshift_media::{
    AiAudioCapabilities, AiCapabilitiesReport, AudioAnalysisReport, DEFAULT_AUDIO_SAMPLE_RATE,
    ExportEncodingProfile, FfmpegBridge, JapaneseTokenizer, LyricAlignBackend, LyricAlignRequest,
    LyricAlignResult, LyricNodeStyle, LyricTimelineNodesTemplate, LyricTimelineTemplateConfig,
    MultibandBeatReport, MultibandFluxConfig, RealEsrganCapabilities, RealEsrganModel, Rgba8Frame,
    StemBackend, StemSeparationRequest, StemSeparationResult, UpscaleBackend, UpscaleConfig,
    UpscaleReport, WaveformChannelLayout, WaveformRenderOptions, WaveformStyle,
    align_audio_lyrics, build_lyric_timeline_nodes, detect_ai_audio_capabilities,
    detect_ai_capabilities, detect_realesrgan_capabilities, render_waveform_rgba,
    separate_audio_stems, upscale_video_file,
};
use frameshift_schema::template::{EffectChainPreset, TemplateSlotValue};
use frameshift_schema::template_parametric::ParametricTemplateSpec;
use frameshift_schema::{
    CURRENT_SCHEMA_VERSION, NodeId, NodeKind, PreferenceEntry, PreferenceScope, Project,
    ProjectCommand, ProjectSession, RenderProfileKind, SchemaError, SchemaVersion,
};
use frameshift_time::{FLICKS_PER_SECOND, FPS_24, Flicks};
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
    pub anime4k_presets: Vec<String>,
    pub scene_depth_plate_fields: Vec<String>,
    pub project_commands: Vec<String>,
    pub preference_scopes: Vec<String>,
    pub safe_expr_variables: Vec<String>,
    pub clip_ids: Vec<String>,
    pub event_ids: Vec<String>,
    pub node_ids: Vec<String>,
}

/// Combined host capabilities report returned by `ai_capabilities`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FullAiCapabilitiesReport {
    #[serde(flatten)]
    pub video_and_onnx: AiCapabilitiesReport,
    pub ai_audio: AiAudioCapabilities,
    pub realesrgan: RealEsrganCapabilities,
}

/// Output of `render_waveform`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderWaveformResult {
    pub out_path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub style: WaveformStyle,
    pub bpm: Option<f64>,
    pub beat_count: usize,
    pub downbeat_count: usize,
}

/// Output of `align_lyrics`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlignLyricsToolResult {
    #[serde(flatten)]
    pub alignment: LyricAlignResult,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeline_nodes: Option<LyricTimelineNodesTemplate>,
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
            McpToolDescriptor {
                name: "ai_capabilities".into(),
                description: "Probes host optical-flow interpolation (rife-ncnn-vulkan, FFmpeg minterpolate), Python ONNX Runtime providers (DirectML/CUDA/CPU), isolated .venv-audio stem/lyric capabilities, and Real-ESRGAN NCNN Vulkan availability.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {}
                }),
            },
            McpToolDescriptor {
                name: "analyze_multiband_beats".into(),
                description: "Analyzes 3-band (sub/mid/high) spectral flux onsets, Ellis tempo/beat grid, and 4/4 downbeats in Flicks.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "asset_id": { "type": "string" },
                        "window_ticks": { "type": "integer" },
                        "fft_size": { "type": "integer" }
                    }
                }),
            },
            McpToolDescriptor {
                name: "render_waveform".into(),
                description: "Renders a DualEnvelope or SpectralRgb waveform PNG with beat/downbeat overlays from project audio or an input audio file.".into(),
                input_schema: json!({
                    "type": "object",
                    "required": ["out_path"],
                    "properties": {
                        "out_path": { "type": "string" },
                        "asset_id": { "type": "string" },
                        "input_path": { "type": "string" },
                        "start_tick": { "type": "integer" },
                        "end_tick": { "type": "integer" },
                        "style": { "type": "string", "enum": ["dual", "spectral"] },
                        "channel_layout": { "type": "string", "enum": ["mono", "stereo"] },
                        "width": { "type": "integer" },
                        "height": { "type": "integer" }
                    }
                }),
            },
            McpToolDescriptor {
                name: "separate_stems".into(),
                description: "Separates vocals/instrumental or 4 stems via the isolated Python 3.12 AI audio sidecar.".into(),
                input_schema: json!({
                    "type": "object",
                    "required": ["input_path", "out_dir"],
                    "properties": {
                        "input_path": { "type": "string" },
                        "out_dir": { "type": "string" },
                        "stems": { "type": "integer", "enum": [2, 4] },
                        "backend": { "type": "string", "enum": ["auto", "audio-separator", "demucs", "mdx-onnx"] },
                        "model": { "type": "string" },
                        "device": { "type": "string" },
                        "output_format": { "type": "string" },
                        "model_dir": { "type": "string" },
                        "offline": { "type": "boolean" }
                    }
                }),
            },
            McpToolDescriptor {
                name: "align_lyrics".into(),
                description: "Performs word-level lyric alignment/transcription (stable-ts / faster-whisper + fugashi) returning Flicks timings and optional VectorText/Text3D graph nodes.".into(),
                input_schema: json!({
                    "type": "object",
                    "required": ["input_audio"],
                    "properties": {
                        "input_audio": { "type": "string" },
                        "lyrics_text": { "type": "string" },
                        "lyrics_file": { "type": "string" },
                        "language": { "type": "string" },
                        "backend": { "type": "string", "enum": ["auto", "stable-ts", "faster-whisper"] },
                        "model": { "type": "string" },
                        "compute_type": { "type": "string" },
                        "device": { "type": "string" },
                        "ja_tokenizer": { "type": "string", "enum": ["fugashi", "char", "raw"] },
                        "vad": { "type": "boolean" },
                        "model_dir": { "type": "string" },
                        "offline": { "type": "boolean" },
                        "build_nodes": { "type": "boolean" },
                        "style": { "type": "string", "enum": ["vector_text", "text3d"] },
                        "base_input_node_id": { "type": "string" },
                        "vertical_japanese": { "type": "boolean" }
                    }
                }),
            },
            McpToolDescriptor {
                name: "upscale_media".into(),
                description: "Upscales a video file 2x/3x/4x using Real-ESRGAN NCNN Vulkan (animevideov3/x4plus), FFmpeg Lanczos, or bilinear fallback with anime duplicate-frame deduplication.".into(),
                input_schema: json!({
                    "type": "object",
                    "required": ["input_path", "out_path"],
                    "properties": {
                        "input_path": { "type": "string" },
                        "out_path": { "type": "string" },
                        "scale": { "type": "integer", "enum": [2, 3, 4] },
                        "model": { "type": "string", "enum": ["realesr-animevideov3", "realesrgan-x4plus-anime", "realesrgan-x4plus"] },
                        "backend": { "type": "string", "enum": ["auto", "realesrgan", "lanczos", "bilinear"] },
                        "gpu_id": { "type": "integer" },
                        "tile_size": { "type": "integer" },
                        "dedup_threshold": { "type": "number" },
                        "scene_cut_threshold": { "type": "number" },
                        "chunk_size": { "type": "integer" },
                        "tta": { "type": "boolean" },
                        "platform_preset": { "type": "string" }
                    }
                }),
            },
            McpToolDescriptor {
                name: "lint_project".into(),
                description: "Runs static and optional media-level diagnostics on the active project (off-screen text, non-linear text color, opaque solid blending, DAG reachability, timing, sandbox, performance, and optional ffmpeg blackdetect).".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "check_media": { "type": "boolean" },
                        "strict_sandbox": { "type": "boolean" },
                        "black_min_duration_sec": { "type": "number" },
                        "black_pic_th": { "type": "number" },
                        "max_blur_sub_samples_warn": { "type": "integer" },
                        "expected_revision_id": { "type": "integer" }
                    }
                }),
            },
            McpToolDescriptor {
                name: "diff_frame".into(),
                description: "Computes quantitative pixel metrics (MAE, RMSE, PSNR, changed bbox, quadrant MAE) between two PNG files and optionally writes a 3-panel heatmap diff PNG.".into(),
                input_schema: json!({
                    "type": "object",
                    "required": ["path_a", "path_b"],
                    "properties": {
                        "path_a": { "type": "string" },
                        "path_b": { "type": "string" },
                        "threshold": { "type": "integer", "minimum": 0, "maximum": 255 },
                        "out_diff_png": { "type": "string" }
                    }
                }),
            },
            McpToolDescriptor {
                name: "compile_motion_dsl".into(),
                description: "Compiles a Motion.md / .fsmotion DSL script into a validated 0.2.0 Project with sRGB->Linear Rec.709 color conversion, lint checks, and optional replacement of the active MCP project.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "input": { "type": "string" },
                        "input_path": { "type": "string" },
                        "skip_lint": { "type": "boolean" },
                        "apply_to_session": { "type": "boolean" },
                        "expected_revision_id": { "type": "integer" }
                    }
                }),
            },
            McpToolDescriptor {
                name: "template_metadata".into(),
                description: "Dry-run resolves duration, resolution, beat-aligned cuts, drop anchors, and effect windows from a ParametricTemplateSpec and props JSON.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "spec": { "type": "object" },
                        "spec_path": { "type": "string" },
                        "props": { "type": "object" },
                        "props_path": { "type": "string" }
                    }
                }),
            },
            McpToolDescriptor {
                name: "template_instantiate".into(),
                description: "Instantiates a ParametricTemplateSpec + props into a validated and linted Project, optionally saving to disk or replacing the active MCP session project.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "spec": { "type": "object" },
                        "spec_path": { "type": "string" },
                        "props": { "type": "object" },
                        "props_path": { "type": "string" },
                        "out_path": { "type": "string" },
                        "apply_to_session": { "type": "boolean" },
                        "expected_revision_id": { "type": "integer" }
                    }
                }),
            },
            McpToolDescriptor {
                name: "track_point".into(),
                description: "Runs GPU pyramidal Lucas-Kanade point tracking on a source node/asset and writes KeyframeTrackDefs onto target_node_id in the active project.".into(),
                input_schema: json!({
                    "type": "object",
                    "required": ["target_node_id", "x", "y"],
                    "properties": {
                        "source_node_id": { "type": "string" },
                        "asset_id": { "type": "string" },
                        "target_node_id": { "type": "string" },
                        "x": { "type": "number" },
                        "y": { "type": "number" },
                        "x2": { "type": "number" },
                        "y2": { "type": "number" },
                        "start_tick": { "type": "integer" },
                        "end_tick": { "type": "integer" },
                        "bind_mode": { "type": "string", "enum": ["transform2d", "stabilize", "null3d"] },
                        "plane_z": { "type": "number" },
                        "track_id_prefix": { "type": "string" },
                        "profile": { "type": "string", "enum": ["draft", "final"] },
                        "expected_revision_id": { "type": "integer" }
                    }
                }),
            },
            McpToolDescriptor {
                name: "track_planar".into(),
                description: "Runs GPU pyramidal 4-corner planar homography tracking on a source node/asset and writes KeyframeTrackDefs onto target_node_id in the active project.".into(),
                input_schema: json!({
                    "type": "object",
                    "required": ["target_node_id", "corners"],
                    "properties": {
                        "source_node_id": { "type": "string" },
                        "asset_id": { "type": "string" },
                        "target_node_id": { "type": "string" },
                        "corners": {
                            "type": "array",
                            "items": { "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2 },
                            "minItems": 4,
                            "maxItems": 4
                        },
                        "start_tick": { "type": "integer" },
                        "end_tick": { "type": "integer" },
                        "bind_mode": { "type": "string", "enum": ["corner_pin", "transform2d", "null3d"] },
                        "plane_z": { "type": "number" },
                        "track_id_prefix": { "type": "string" },
                        "profile": { "type": "string", "enum": ["draft", "final"] },
                        "expected_revision_id": { "type": "integer" }
                    }
                }),
            },
            McpToolDescriptor {
                name: "export_nle".into(),
                description: "Exports the active project to OpenTimelineIO (.otio) JSON or Final Cut Pro XML (FCPXML 1.11) with optional beat markers and optional file write.".into(),
                input_schema: json!({
                    "type": "object",
                    "required": ["format"],
                    "properties": {
                        "format": { "type": "string", "enum": ["otio", "fcpxml"] },
                        "out_path": { "type": "string" },
                        "include_beat_markers": { "type": "boolean" },
                        "expected_revision_id": { "type": "integer" }
                    }
                }),
            },
            McpToolDescriptor {
                name: "import_nle".into(),
                description: "Imports an OpenTimelineIO (.otio) JSON or FCPXML document into a validated FrameShift Project, optionally replacing the active session project.".into(),
                input_schema: json!({
                    "type": "object",
                    "required": ["format"],
                    "properties": {
                        "format": { "type": "string", "enum": ["otio", "fcpxml"] },
                        "input": { "type": "string" },
                        "input_path": { "type": "string" },
                        "project_name": { "type": "string" },
                        "apply_to_session": { "type": "boolean" },
                        "expected_revision_id": { "type": "integer" }
                    }
                }),
            },
            McpToolDescriptor {
                name: "ai_models".into(),
                description: "Discovers manifest-driven ONNX AI model packages (frameshift.ai_model.v1) under models_dir and reports their SHA-256 verification status.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "models_dir": { "type": "string" }
                    }
                }),
            },
            McpToolDescriptor {
                name: "ai_run_manifest".into(),
                description: "Runs a manifest-driven ONNX AI model (by model_id under models_dir or explicit manifest_path) over a PNG or video file with strict SHA-256 verification by default.".into(),
                input_schema: json!({
                    "type": "object",
                    "required": ["input_path", "out_path"],
                    "properties": {
                        "model_id": { "type": "string" },
                        "manifest_path": { "type": "string" },
                        "models_dir": { "type": "string" },
                        "input_path": { "type": "string" },
                        "out_path": { "type": "string" },
                        "provider": { "type": "string", "enum": ["auto", "dml", "cuda", "cpu"] },
                        "temporal_smooth": { "type": "number" },
                        "params_json": { "type": "object" },
                        "allow_unverified": { "type": "boolean" },
                        "synthetic_test": { "type": "boolean" }
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
            anime4k_presets: vec![
                "mode_a".into(),
                "mode_b".into(),
                "mode_c".into(),
                "restore_only".into(),
                "line_art".into(),
            ],
            scene_depth_plate_fields: vec![
                "node_id".into(),
                "near_z".into(),
                "far_z".into(),
                "depth_gamma".into(),
                "disparity_mode".into(),
                "rg16_packed".into(),
            ],
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
                "upsert_keyframe_track".into(),
                "remove_keyframe_track".into(),
                "replace_project".into(),
            ],
            preference_scopes: vec!["global".into(), "project".into(), "scene".into()],
            safe_expr_variables: vec![
                "t_sec".into(),
                "output_tick".into(),
                "beat_phase".into(),
                "downbeat_phase".into(),
                "audio_rms".into(),
                "audio_sub".into(),
                "audio_mid".into(),
                "audio_high".into(),
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

    /// Returns host optical-flow interpolation, ONNX AI sidecar, isolated `.venv-audio`, and Real-ESRGAN capabilities.
    pub fn ai_capabilities(&self) -> FullAiCapabilitiesReport {
        FullAiCapabilitiesReport {
            video_and_onnx: detect_ai_capabilities(),
            ai_audio: detect_ai_audio_capabilities(),
            realesrgan: detect_realesrgan_capabilities(),
        }
    }

    /// Analyzes 3-band spectral flux onsets (`sub`/`mid`/`high`), tempo/beat grid, and 4/4 downbeats.
    pub fn analyze_multiband_beats(
        &self,
        asset_id: Option<&str>,
        window_ticks: Option<Flicks>,
        fft_size: Option<usize>,
    ) -> Result<MultibandBeatReport, McpError> {
        let mut cfg = MultibandFluxConfig::default();
        if let Some(w) = window_ticks {
            cfg.window_ticks = w;
        }
        if let Some(fft) = fft_size {
            cfg.fft_size = fft;
        }
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
        Ok(audio.analyze_multiband_beats(cfg)?)
    }

    /// Renders a waveform PNG from a project audio asset/mix or an explicit audio file.
    #[allow(clippy::too_many_arguments)]
    pub fn render_waveform(
        &self,
        out_path: &Path,
        asset_id: Option<&str>,
        input_path: Option<&Path>,
        range_ticks: Option<(Flicks, Flicks)>,
        style: WaveformStyle,
        channel_layout: WaveformChannelLayout,
        width: u32,
        height: u32,
    ) -> Result<RenderWaveformResult, McpError> {
        let project = self.session.project();
        let (audio, title, revision_id) = if let Some(in_path) = input_path {
            let buf = FfmpegBridge::decode_audio(in_path, DEFAULT_AUDIO_SAMPLE_RATE, 2)?;
            let name = in_path
                .file_name()
                .and_then(|s| s.to_str())
                .map(str::to_owned);
            (buf, name, None)
        } else if let Some(aid) = asset_id {
            let asset = project
                .asset(aid)
                .ok_or_else(|| SchemaError::MissingAsset(aid.to_owned()))?;
            let buf = self
                .resolver
                .load_audio_asset(asset, project.timeline.duration_ticks)?;
            (buf, Some(aid.to_owned()), Some(project.revision_id))
        } else {
            let buf = self.resolver.mix_project_audio(project)?.ok_or_else(|| {
                McpError::InvalidParams("project has no audio clips and no input_path was provided".into())
            })?;
            (buf, Some(project.name.clone()), Some(project.revision_id))
        };

        let mut opts = WaveformRenderOptions::new(width, height);
        opts.range_ticks = range_ticks;
        opts.style = style;
        opts.channel_layout = channel_layout;
        opts.title = title;
        opts.revision_id = revision_id;
        if audio.frame_count() > 0
            && let Ok(mb) = audio.analyze_multiband_beats(MultibandFluxConfig::default())
        {
            opts.bpm = Some(mb.estimated_bpm);
            opts.beat_ticks = mb.beat_grid;
            opts.downbeat_ticks = mb.downbeats;
        }
        let beat_count = opts.beat_ticks.len();
        let downbeat_count = opts.downbeat_ticks.len();
        let bpm = opts.bpm;

        let frame = render_waveform_rgba(&audio, &opts)?;
        if let Some(parent) = out_path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        frame.save_png(out_path)?;
        Ok(RenderWaveformResult {
            out_path: out_path.to_path_buf(),
            width: frame.width,
            height: frame.height,
            style,
            bpm,
            beat_count,
            downbeat_count,
        })
    }

    /// Separates audio stems using the isolated Python 3.12 AI audio sidecar.
    pub fn separate_stems(&self, req: &StemSeparationRequest) -> Result<StemSeparationResult, McpError> {
        Ok(separate_audio_stems(req)?)
    }

    /// Aligns or transcribes lyrics to deterministic `Flicks(i64)` timings and optional graph nodes.
    pub fn align_lyrics(
        &self,
        req: &LyricAlignRequest,
        build_nodes: bool,
        template_config: &LyricTimelineTemplateConfig,
    ) -> Result<AlignLyricsToolResult, McpError> {
        let alignment = align_audio_lyrics(req)?;
        let timeline_nodes = if build_nodes {
            let words: Vec<_> = alignment.all_words().into_iter().cloned().collect();
            Some(build_lyric_timeline_nodes(
                &words,
                &alignment.language,
                template_config,
            ))
        } else {
            None
        };
        Ok(AlignLyricsToolResult {
            alignment,
            timeline_nodes,
        })
    }

    /// Upscales a video file using Real-ESRGAN NCNN Vulkan, FFmpeg Lanczos, or bilinear fallback.
    pub fn upscale_media(
        &self,
        input_path: &Path,
        out_path: &Path,
        config: &UpscaleConfig,
        platform_preset: Option<&str>,
    ) -> Result<UpscaleReport, McpError> {
        let profile = if let Some(name) = platform_preset {
            ExportEncodingProfile::from_platform_name(name).ok_or_else(|| {
                McpError::InvalidParams(format!(
                    "unknown platform_preset '{name}'; expected one of tiktok_1080p60, reels_1080p30, youtube_1080p60, anime_10bit_hevc, prores_422hq"
                ))
            })?
        } else {
            ExportEncodingProfile::default()
        };
        Ok(upscale_video_file(input_path, out_path, config, &profile)?)
    }

    /// Runs static and optional media-level lint checks on the active project.
    pub fn lint_active_project(
        &self,
        opts: &LintOptions,
        expected_revision_id: Option<u64>,
    ) -> Result<frameshift_core::LintReport, McpError> {
        self.check_expected_revision(expected_revision_id)?;
        Ok(lint_project(self.session.project(), opts))
    }

    /// Compiles a Motion.md / .fsmotion DSL string into a validated Project + LintReport,
    /// optionally replacing the active MCP project.
    pub fn compile_motion_dsl(
        &mut self,
        input: &str,
        skip_lint: bool,
        apply_to_session: bool,
        expected_revision_id: Option<u64>,
    ) -> Result<Value, McpError> {
        if apply_to_session {
            self.check_expected_revision(expected_revision_id)?;
        }
        let compiled = compile_motion_str(
            input,
            &DslCompileOptions {
                skip_lint,
                ..DslCompileOptions::default()
            },
        )?;
        let applied_revision_id = if apply_to_session {
            let rev = self.revision_id();
            let res = self.mutate_project_as_agent(
                rev,
                "mcp-agent".into(),
                "Replace active project from compiled Motion DSL".into(),
                ProjectCommand::ReplaceProject {
                    project: compiled.project.clone(),
                },
            )?;
            Some(res.revision_id)
        } else {
            None
        };
        Ok(json!({
            "project": compiled.project,
            "lint_report": compiled.lint_report,
            "applied_revision_id": applied_revision_id,
        }))
    }

    fn resolve_source_node_id(
        project: &Project,
        source_node_id: Option<&str>,
        asset_id: Option<&str>,
    ) -> Result<NodeId, McpError> {
        if let Some(node) = source_node_id {
            return Ok(NodeId::new(node));
        }
        let aid = asset_id.ok_or_else(|| {
            McpError::InvalidParams("either 'source_node_id' or 'asset_id' is required".into())
        })?;
        let find_matching = |only_enabled: bool| -> Option<NodeId> {
            project.graph.nodes.iter().find_map(|n| {
                if only_enabled && !n.enabled {
                    return None;
                }
                match &n.kind {
                    NodeKind::SourceVideo { asset_id: a, .. }
                    | NodeKind::SourceImage { asset_id: a, .. }
                        if a == aid =>
                    {
                        Some(n.id.clone())
                    }
                    _ => None,
                }
            })
        };
        if let Some(id) = find_matching(true).or_else(|| find_matching(false)) {
            return Ok(id);
        }
        let as_node = NodeId::new(aid);
        if project.graph.node(&as_node).is_some() {
            return Ok(as_node);
        }
        Err(McpError::InvalidParams(format!(
            "no SourceVideo/SourceImage node references asset '{aid}'"
        )))
    }

    fn apply_tracked_keyframes(
        &mut self,
        tracks: &[frameshift_schema::KeyframeTrackDef],
        intent: &str,
    ) -> Result<u64, McpError> {
        for track in tracks {
            let rev = self.revision_id();
            self.mutate_project_as_agent(
                rev,
                "mcp-tracker".into(),
                intent.to_owned(),
                ProjectCommand::UpsertKeyframeTrack(track.clone()),
            )?;
        }
        Ok(self.revision_id())
    }

    /// Tracks 1 or 2 points in the active project and persists KeyframeTrackDefs via session commands.
    #[allow(clippy::too_many_arguments)]
    pub fn track_point(
        &mut self,
        source_node_id: Option<&str>,
        asset_id: Option<&str>,
        target_node_id: &str,
        start_point_uv: [f32; 2],
        point2_uv: Option<[f32; 2]>,
        start_tick: Option<Flicks>,
        end_tick: Option<Flicks>,
        bind_target: TrackerBindTarget,
        track_id_prefix: Option<&str>,
        profile: RenderProfileKind,
        expected_revision_id: Option<u64>,
    ) -> Result<frameshift_core::TrackerProjectReport, McpError> {
        self.check_expected_revision(expected_revision_id)?;
        let mut proj = self.session.project().clone();
        let src_node = Self::resolve_source_node_id(&proj, source_node_id, asset_id)?;
        let s_tick = start_tick.unwrap_or(Flicks::ZERO);
        let e_tick = end_tick.unwrap_or(proj.timeline.duration_ticks);
        let prefix = track_id_prefix
            .map(str::to_owned)
            .unwrap_or_else(|| format!("track_pt_{target_node_id}"));
        let opts = PointTrackProjectOptions {
            source_node_id: src_node,
            target_node_id: NodeId::new(target_node_id),
            bind_target,
            track_id_prefix: prefix,
            start_tick: s_tick,
            end_tick: e_tick,
            start_point_uv,
            point_options: TrackPointOptions {
                point2_uv,
                ..TrackPointOptions::default()
            },
            profile,
        };
        let mut report = {
            let engine = self.ensure_engine()?;
            engine.track_point_to_project(&mut proj, &opts)?
        };
        report.updated_revision_id = self.apply_tracked_keyframes(
            &report.generated_tracks,
            &format!("Apply point tracker keyframes to '{target_node_id}'"),
        )?;
        Ok(report)
    }

    /// Tracks a 4-corner planar quad in the active project and persists KeyframeTrackDefs.
    #[allow(clippy::too_many_arguments)]
    pub fn track_planar(
        &mut self,
        source_node_id: Option<&str>,
        asset_id: Option<&str>,
        target_node_id: &str,
        start_corners_uv: [[f32; 2]; 4],
        start_tick: Option<Flicks>,
        end_tick: Option<Flicks>,
        bind_target: TrackerBindTarget,
        track_id_prefix: Option<&str>,
        profile: RenderProfileKind,
        expected_revision_id: Option<u64>,
    ) -> Result<frameshift_core::TrackerProjectReport, McpError> {
        self.check_expected_revision(expected_revision_id)?;
        let mut proj = self.session.project().clone();
        let src_node = Self::resolve_source_node_id(&proj, source_node_id, asset_id)?;
        let s_tick = start_tick.unwrap_or(Flicks::ZERO);
        let e_tick = end_tick.unwrap_or(proj.timeline.duration_ticks);
        let prefix = track_id_prefix
            .map(str::to_owned)
            .unwrap_or_else(|| format!("track_planar_{target_node_id}"));
        let opts = PlanarTrackProjectOptions {
            source_node_id: src_node,
            target_node_id: NodeId::new(target_node_id),
            bind_target,
            track_id_prefix: prefix,
            start_tick: s_tick,
            end_tick: e_tick,
            start_corners_uv,
            planar_options: TrackPlanarOptions::default(),
            profile,
        };
        let mut report = {
            let engine = self.ensure_engine()?;
            engine.track_planar_to_project(&mut proj, &opts)?
        };
        report.updated_revision_id = self.apply_tracked_keyframes(
            &report.generated_tracks,
            &format!("Apply planar tracker keyframes to '{target_node_id}'"),
        )?;
        Ok(report)
    }

    /// Exports the active project to OTIO JSON or FCPXML 1.11.
    pub fn export_nle(
        &self,
        format: &str,
        out_path: Option<&Path>,
        include_beat_markers: bool,
        expected_revision_id: Option<u64>,
    ) -> Result<Value, McpError> {
        let rev = self.check_expected_revision(expected_revision_id)?;
        let project = self.session.project();
        let mut nle_opts = NleExportOptions::default();
        if include_beat_markers
            && let Ok(Some(mixed)) = self.resolver.mix_project_audio(project)
            && let Ok(mb) = mixed.analyze_multiband_beats(MultibandFluxConfig::default())
            && let Ok(frame_ticks) = project.timeline.fps.frame_duration()
        {
            nle_opts.extra_markers = nle_markers_from_beats(&mb, frame_ticks);
        }
        let content = match format.trim().to_ascii_lowercase().as_str() {
            "otio" => export_project_to_otio_json(project, &nle_opts)?,
            "fcpxml" => export_project_to_fcpxml(project, &nle_opts)?,
            other => {
                return Err(McpError::InvalidParams(format!(
                    "unknown NLE format '{other}'; expected 'otio' or 'fcpxml'"
                )));
            }
        };
        if let Some(path) = out_path {
            if let Some(parent) = path.parent()
                && !parent.as_os_str().is_empty()
            {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, &content)?;
        }
        Ok(json!({
            "format": format.trim().to_ascii_lowercase(),
            "revision_id": rev,
            "out_path": out_path,
            "content": content,
        }))
    }

    /// Imports an OTIO JSON or FCPXML document into a FrameShift Project, optionally replacing the session project.
    pub fn import_nle(
        &mut self,
        format: &str,
        content: &str,
        project_name: Option<String>,
        apply_to_session: bool,
        expected_revision_id: Option<u64>,
    ) -> Result<Value, McpError> {
        if apply_to_session {
            self.check_expected_revision(expected_revision_id)?;
        }
        let opts = NleImportOptions {
            project_name,
            ..NleImportOptions::default()
        };
        let project = match format.trim().to_ascii_lowercase().as_str() {
            "otio" => import_project_from_otio_json(content, &opts)?,
            "fcpxml" => import_project_from_fcpxml(content, &opts)?,
            other => {
                return Err(McpError::InvalidParams(format!(
                    "unknown NLE format '{other}'; expected 'otio' or 'fcpxml'"
                )));
            }
        };
        let applied_revision_id = if apply_to_session {
            let rev = self.revision_id();
            let res = self.mutate_project_as_agent(
                rev,
                "mcp-agent".into(),
                format!("Replace active project from imported {format} NLE"),
                ProjectCommand::ReplaceProject {
                    project: project.clone(),
                },
            )?;
            Some(res.revision_id)
        } else {
            None
        };
        Ok(json!({
            "format": format.trim().to_ascii_lowercase(),
            "project": project,
            "applied_revision_id": applied_revision_id,
        }))
    }

    /// Runs a manifest-driven ONNX AI model over a PNG or video file.
    #[allow(clippy::too_many_arguments)]
    pub fn ai_run_manifest(
        &self,
        model_id: Option<&str>,
        manifest_path: Option<&Path>,
        models_dir: &Path,
        input_path: &Path,
        out_path: &Path,
        provider: Option<&str>,
        temporal_smooth: Option<f32>,
        params_json: Option<Value>,
        allow_unverified: bool,
        synthetic_test: bool,
    ) -> Result<Value, McpError> {
        let is_png = |p: &Path| {
            p.extension()
                .and_then(|s| s.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("png"))
        };
        let (frames, fps) = if is_png(input_path) {
            (vec![Rgba8Frame::load_png(input_path, Flicks::ZERO)?], FPS_24)
        } else {
            let probe = FfmpegBridge::probe_media_info(input_path)?;
            let decoded = FfmpegBridge::decode_video_window(
                input_path,
                &probe.pts_index,
                0,
                probe.pts_index.len(),
                probe.width,
                probe.height,
            )?;
            (
                decoded.into_iter().map(|(_, f)| f).collect(),
                probe.fps.unwrap_or(FPS_24),
            )
        };
        let cfg = AiManifestRunConfig {
            provider: provider.unwrap_or("auto").to_owned(),
            temporal_smooth,
            params_json,
            allow_unverified,
            synthetic_test,
            ..AiManifestRunConfig::default()
        };
        let (out_frames, report) = match (manifest_path, model_id) {
            (Some(mp), _) => run_manifest_model(&frames, mp, &cfg)?,
            (None, Some(id)) => run_manifest_model_by_id(&frames, models_dir, id, &cfg)?,
            (None, None) => {
                return Err(McpError::InvalidParams(
                    "either 'model_id' or 'manifest_path' is required".into(),
                ));
            }
        };
        if let Some(parent) = out_path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        if out_frames.len() == 1 && is_png(out_path) {
            out_frames[0].save_png(out_path)?;
        } else {
            FfmpegBridge::encode_mp4(&out_frames, None, fps, out_path)?;
        }
        Ok(json!({
            "out_path": out_path,
            "report": report,
        }))
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
            | "render_project"
            | "ai_capabilities"
            | "analyze_multiband_beats"
            | "render_waveform"
            | "separate_stems"
            | "align_lyrics"
            | "upscale_media"
            | "lint_project"
            | "diff_frame"
            | "compile_motion_dsl"
            | "template_metadata"
            | "template_instantiate"
            | "track_point"
            | "track_planar"
            | "export_nle"
            | "import_nle"
            | "ai_models"
            | "ai_run_manifest" => {
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
            "ai_capabilities" => to_json_value(self.ai_capabilities()),
            "analyze_multiband_beats" => {
                let p: AnalyzeMultibandParams = serde_json::from_value(args)
                    .map_err(|e| RpcDispatchError::InvalidParams(e.to_string()))?;
                let report = self
                    .analyze_multiband_beats(
                        p.asset_id.as_deref(),
                        p.window_ticks.map(Flicks),
                        p.fft_size,
                    )
                    .map_err(RpcDispatchError::App)?;
                to_json_value(report)
            }
            "render_waveform" => {
                let p: RenderWaveformParams = serde_json::from_value(args)
                    .map_err(|e| RpcDispatchError::InvalidParams(e.to_string()))?;
                let range_ticks = match (p.start_tick, p.end_tick) {
                    (Some(s), Some(e)) => Some((Flicks(s), Flicks(e))),
                    (None, None) => None,
                    _ => {
                        return Err(RpcDispatchError::InvalidParams(
                            "start_tick and end_tick must be supplied together".into(),
                        ));
                    }
                };
                let style = match p.style.as_deref().unwrap_or("dual") {
                    "dual" | "dual_envelope" => WaveformStyle::DualEnvelope,
                    "spectral" | "spectral_rgb" => WaveformStyle::SpectralRgb,
                    other => {
                        return Err(RpcDispatchError::InvalidParams(format!(
                            "unknown waveform style '{other}'; expected dual or spectral"
                        )));
                    }
                };
                let layout = match p.channel_layout.as_deref().unwrap_or("mono") {
                    "mono" | "mono_mix" => WaveformChannelLayout::MonoMix,
                    "stereo" | "stereo_split" => WaveformChannelLayout::StereoSplit,
                    other => {
                        return Err(RpcDispatchError::InvalidParams(format!(
                            "unknown channel_layout '{other}'; expected mono or stereo"
                        )));
                    }
                };
                let res = self
                    .render_waveform(
                        Path::new(&p.out_path),
                        p.asset_id.as_deref(),
                        p.input_path.as_deref().map(Path::new),
                        range_ticks,
                        style,
                        layout,
                        p.width.unwrap_or(1600),
                        p.height.unwrap_or(400),
                    )
                    .map_err(RpcDispatchError::App)?;
                to_json_value(res)
            }
            "separate_stems" => {
                let p: SeparateStemsParams = serde_json::from_value(args)
                    .map_err(|e| RpcDispatchError::InvalidParams(e.to_string()))?;
                let backend = match p.backend.as_deref().unwrap_or("auto") {
                    "auto" => StemBackend::Auto,
                    "audio-separator" | "audio_separator" => StemBackend::AudioSeparator,
                    "demucs" => StemBackend::Demucs,
                    "mdx-onnx" | "mdx_onnx" | "mdx" => StemBackend::MdxOnnx,
                    other => {
                        return Err(RpcDispatchError::InvalidParams(format!(
                            "unknown stem backend '{other}'"
                        )));
                    }
                };
                let req = StemSeparationRequest {
                    input_path: PathBuf::from(p.input_path),
                    out_dir: PathBuf::from(p.out_dir),
                    backend,
                    model: p.model,
                    stems: p.stems.unwrap_or(2),
                    device: p.device.unwrap_or_else(|| "auto".into()),
                    output_format: p.output_format.unwrap_or_else(|| "wav".into()),
                    model_dir: p.model_dir.map(PathBuf::from),
                    offline: p.offline,
                    python_binary: None,
                    sidecar_script: None,
                };
                let res = self.separate_stems(&req).map_err(RpcDispatchError::App)?;
                to_json_value(res)
            }
            "align_lyrics" => {
                let p: AlignLyricsParams = serde_json::from_value(args)
                    .map_err(|e| RpcDispatchError::InvalidParams(e.to_string()))?;
                let backend = match p.backend.as_deref().unwrap_or("auto") {
                    "auto" => LyricAlignBackend::Auto,
                    "stable-ts" | "stable_ts" => LyricAlignBackend::StableTs,
                    "faster-whisper" | "faster_whisper" => LyricAlignBackend::FasterWhisper,
                    other => {
                        return Err(RpcDispatchError::InvalidParams(format!(
                            "unknown lyric align backend '{other}'"
                        )));
                    }
                };
                let ja_tokenizer = match p.ja_tokenizer.as_deref().unwrap_or("fugashi") {
                    "fugashi" => JapaneseTokenizer::Fugashi,
                    "char" => JapaneseTokenizer::Char,
                    "raw" => JapaneseTokenizer::Raw,
                    other => {
                        return Err(RpcDispatchError::InvalidParams(format!(
                            "unknown ja_tokenizer '{other}'"
                        )));
                    }
                };
                let base_req = LyricAlignRequest::default();
                let req = LyricAlignRequest {
                    input_audio: PathBuf::from(p.input_audio),
                    lyrics_text: p.lyrics_text,
                    lyrics_file: p.lyrics_file.map(PathBuf::from),
                    language: p.language,
                    backend,
                    model: p.model.unwrap_or(base_req.model),
                    compute_type: p.compute_type.unwrap_or(base_req.compute_type),
                    device: p.device.unwrap_or(base_req.device),
                    ja_tokenizer,
                    vad: p.vad.unwrap_or(base_req.vad),
                    model_dir: p.model_dir.map(PathBuf::from),
                    offline: p.offline,
                    python_binary: None,
                    sidecar_script: None,
                };

                let base_tmpl = LyricTimelineTemplateConfig::default();
                let tmpl_cfg = LyricTimelineTemplateConfig {
                    style: match p.style.as_deref() {
                        Some("vector_text" | "vectortext" | "vector") | None => {
                            LyricNodeStyle::VectorText
                        }
                        Some("text3d" | "text_3d" | "3d") => LyricNodeStyle::Text3D,
                        Some(other) => {
                            return Err(RpcDispatchError::InvalidParams(format!(
                                "unknown lyric node style '{other}'"
                            )));
                        }
                    },
                    base_input_node_id: p
                        .base_input_node_id
                        .unwrap_or(base_tmpl.base_input_node_id),
                    vertical_japanese: p.vertical_japanese,
                    ..base_tmpl
                };

                let res = self
                    .align_lyrics(&req, p.build_nodes, &tmpl_cfg)
                    .map_err(RpcDispatchError::App)?;
                to_json_value(res)
            }
            "upscale_media" => {
                let p: UpscaleMediaParams = serde_json::from_value(args)
                    .map_err(|e| RpcDispatchError::InvalidParams(e.to_string()))?;
                let mut cfg = UpscaleConfig::default();
                if let Some(s) = p.scale {
                    cfg.scale = s;
                }
                if let Some(m) = p.model.as_deref() {
                    cfg.model = RealEsrganModel::from_name(m).ok_or_else(|| {
                        RpcDispatchError::InvalidParams(format!("unknown upscale model '{m}'"))
                    })?;
                }
                if let Some(b) = p.backend.as_deref() {
                    cfg.backend = match b {
                        "auto" => None,
                        "realesrgan" | "realesrgan-ncnn-vulkan" => {
                            Some(UpscaleBackend::RealEsrganNcnnCli)
                        }
                        "lanczos" | "ffmpeg" | "ffmpeg-lanczos" => {
                            Some(UpscaleBackend::FfmpegLanczos)
                        }
                        "bilinear" => Some(UpscaleBackend::BilinearFallback),
                        other => {
                            return Err(RpcDispatchError::InvalidParams(format!(
                                "unknown upscale backend '{other}'"
                            )));
                        }
                    };
                }
                cfg.gpu_id = p.gpu_id;
                if let Some(t) = p.tile_size {
                    cfg.tile_size = t;
                }
                if let Some(d) = p.dedup_threshold {
                    cfg.dedup_threshold = d;
                }
                if let Some(s) = p.scene_cut_threshold {
                    cfg.scene_cut_threshold = s;
                }
                if let Some(c) = p.chunk_size {
                    cfg.chunk_size = c;
                }
                cfg.tta = p.tta;
                let res = self
                    .upscale_media(
                        Path::new(&p.input_path),
                        Path::new(&p.out_path),
                        &cfg,
                        p.platform_preset.as_deref(),
                    )
                    .map_err(RpcDispatchError::App)?;
                to_json_value(res)
            }
            "lint_project" => {
                let p: LintProjectParams = serde_json::from_value(args)
                    .map_err(|e| RpcDispatchError::InvalidParams(e.to_string()))?;
                let defaults = LintOptions::default();
                let opts = LintOptions {
                    check_media: p.check_media,
                    black_min_duration_sec: p
                        .black_min_duration_sec
                        .unwrap_or(defaults.black_min_duration_sec),
                    black_pic_th: p.black_pic_th.unwrap_or(defaults.black_pic_th),
                    strict_sandbox: p.strict_sandbox,
                    max_blur_sub_samples_warn: p
                        .max_blur_sub_samples_warn
                        .unwrap_or(defaults.max_blur_sub_samples_warn),
                };
                let report = self
                    .lint_active_project(&opts, p.expected_revision_id)
                    .map_err(RpcDispatchError::App)?;
                to_json_value(report)
            }
            "diff_frame" => {
                let p: DiffFrameParams = serde_json::from_value(args)
                    .map_err(|e| RpcDispatchError::InvalidParams(e.to_string()))?;
                let report = diff_frame_files(
                    Path::new(&p.path_a),
                    Path::new(&p.path_b),
                    p.threshold.unwrap_or(2),
                    p.out_diff_png.as_deref().map(Path::new),
                )
                .map_err(|e| RpcDispatchError::App(e.into()))?;
                to_json_value(report)
            }
            "compile_motion_dsl" => {
                let p: CompileMotionDslParams = serde_json::from_value(args)
                    .map_err(|e| RpcDispatchError::InvalidParams(e.to_string()))?;
                let source = match (p.input, p.input_path) {
                    (Some(s), _) => s,
                    (None, Some(path)) => std::fs::read_to_string(&path)
                        .map_err(|e| RpcDispatchError::App(e.into()))?,
                    (None, None) => {
                        return Err(RpcDispatchError::InvalidParams(
                            "either 'input' or 'input_path' is required".into(),
                        ));
                    }
                };
                self.compile_motion_dsl(
                    &source,
                    p.skip_lint,
                    p.apply_to_session,
                    p.expected_revision_id,
                )
                .map_err(RpcDispatchError::App)
            }
            "template_metadata" => {
                let p: TemplateParams = serde_json::from_value(args)
                    .map_err(|e| RpcDispatchError::InvalidParams(e.to_string()))?;
                let (spec, props) = resolve_spec_and_props(
                    p.spec,
                    p.spec_path.as_deref(),
                    p.props,
                    p.props_path.as_deref(),
                )?;
                let meta = calculate_template_metadata(&spec, &props)
                    .map_err(|e| RpcDispatchError::App(e.into()))?;
                to_json_value(meta)
            }
            "template_instantiate" => {
                let p: TemplateParams = serde_json::from_value(args)
                    .map_err(|e| RpcDispatchError::InvalidParams(e.to_string()))?;
                if p.apply_to_session {
                    self.check_expected_revision(p.expected_revision_id)
                        .map_err(RpcDispatchError::App)?;
                }
                let (spec, props) = resolve_spec_and_props(
                    p.spec,
                    p.spec_path.as_deref(),
                    p.props,
                    p.props_path.as_deref(),
                )?;
                let (project, metadata, lint_report) = instantiate_parametric_template(&spec, &props)
                    .map_err(|e| RpcDispatchError::App(e.into()))?;
                if let Some(ref out_path) = p.out_path {
                    let path = Path::new(out_path);
                    if let Some(parent) = path.parent()
                        && !parent.as_os_str().is_empty()
                    {
                        std::fs::create_dir_all(parent)
                            .map_err(|e| RpcDispatchError::App(e.into()))?;
                    }
                    let json_str = project
                        .to_json_pretty()
                        .map_err(|e| RpcDispatchError::App(e.into()))?;
                    std::fs::write(path, json_str).map_err(|e| RpcDispatchError::App(e.into()))?;
                }
                let applied_revision_id = if p.apply_to_session {
                    let rev = self.revision_id();
                    let res = self
                        .mutate_project_as_agent(
                            rev,
                            "mcp-agent".into(),
                            format!("Replace active project from template '{}'", metadata.template_id),
                            ProjectCommand::ReplaceProject {
                                project: project.clone(),
                            },
                        )
                        .map_err(RpcDispatchError::App)?;
                    Some(res.revision_id)
                } else {
                    None
                };
                Ok(json!({
                    "project": project,
                    "metadata": metadata,
                    "lint_report": lint_report,
                    "out_path": p.out_path,
                    "applied_revision_id": applied_revision_id,
                }))
            }
            "track_point" => {
                let p: TrackPointParams = serde_json::from_value(args)
                    .map_err(|e| RpcDispatchError::InvalidParams(e.to_string()))?;
                let bind_target = match p.bind_mode.as_deref().unwrap_or("transform2d") {
                    "transform2d" | "transform_2d" => TrackerBindTarget::Transform2D,
                    "stabilize" | "transform2d_stabilize" => {
                        TrackerBindTarget::Transform2DStabilize
                    }
                    "null3d" | "null_3d" => {
                        TrackerBindTarget::null3d(p.plane_z.unwrap_or(0.0))
                    }
                    other => {
                        return Err(RpcDispatchError::InvalidParams(format!(
                            "unknown track_point bind_mode '{other}'"
                        )));
                    }
                };
                let point2 = match (p.x2, p.y2) {
                    (Some(x2), Some(y2)) => Some([x2, y2]),
                    (None, None) => None,
                    _ => {
                        return Err(RpcDispatchError::InvalidParams(
                            "x2 and y2 must be provided together".into(),
                        ));
                    }
                };
                let report = self
                    .track_point(
                        p.source_node_id.as_deref(),
                        p.asset_id.as_deref(),
                        &p.target_node_id,
                        [p.x, p.y],
                        point2,
                        p.start_tick.map(Flicks),
                        p.end_tick.map(Flicks),
                        bind_target,
                        p.track_id_prefix.as_deref(),
                        p.profile.unwrap_or(RenderProfileKind::Draft),
                        p.expected_revision_id,
                    )
                    .map_err(RpcDispatchError::App)?;
                to_json_value(report)
            }
            "track_planar" => {
                let p: TrackPlanarParams = serde_json::from_value(args)
                    .map_err(|e| RpcDispatchError::InvalidParams(e.to_string()))?;
                let bind_target = match p.bind_mode.as_deref().unwrap_or("corner_pin") {
                    "corner_pin" | "cornerpin" => TrackerBindTarget::CornerPin,
                    "transform2d" | "transform_2d" => TrackerBindTarget::Transform2D,
                    "null3d" | "null_3d" => {
                        TrackerBindTarget::null3d(p.plane_z.unwrap_or(0.0))
                    }
                    other => {
                        return Err(RpcDispatchError::InvalidParams(format!(
                            "unknown track_planar bind_mode '{other}'"
                        )));
                    }
                };
                let report = self
                    .track_planar(
                        p.source_node_id.as_deref(),
                        p.asset_id.as_deref(),
                        &p.target_node_id,
                        p.corners,
                        p.start_tick.map(Flicks),
                        p.end_tick.map(Flicks),
                        bind_target,
                        p.track_id_prefix.as_deref(),
                        p.profile.unwrap_or(RenderProfileKind::Draft),
                        p.expected_revision_id,
                    )
                    .map_err(RpcDispatchError::App)?;
                to_json_value(report)
            }
            "export_nle" => {
                let p: ExportNleParams = serde_json::from_value(args)
                    .map_err(|e| RpcDispatchError::InvalidParams(e.to_string()))?;
                self.export_nle(
                    &p.format,
                    p.out_path.as_deref().map(Path::new),
                    p.include_beat_markers,
                    p.expected_revision_id,
                )
                .map_err(RpcDispatchError::App)
            }
            "import_nle" => {
                let p: ImportNleParams = serde_json::from_value(args)
                    .map_err(|e| RpcDispatchError::InvalidParams(e.to_string()))?;
                let content = match (p.input, p.input_path) {
                    (Some(s), _) => s,
                    (None, Some(path)) => std::fs::read_to_string(&path)
                        .map_err(|e| RpcDispatchError::App(e.into()))?,
                    (None, None) => {
                        return Err(RpcDispatchError::InvalidParams(
                            "either 'input' or 'input_path' is required".into(),
                        ));
                    }
                };
                self.import_nle(
                    &p.format,
                    &content,
                    p.project_name,
                    p.apply_to_session,
                    p.expected_revision_id,
                )
                .map_err(RpcDispatchError::App)
            }
            "ai_models" => {
                let p: AiModelsParams = serde_json::from_value(args)
                    .map_err(|e| RpcDispatchError::InvalidParams(e.to_string()))?;
                let dir = p
                    .models_dir
                    .unwrap_or_else(|| "local_media/models".into());
                let models = discover_ai_models(Path::new(&dir))
                    .map_err(|e| RpcDispatchError::App(e.into()))?;
                Ok(json!({
                    "models_dir": dir,
                    "count": models.len(),
                    "models": models,
                }))
            }
            "ai_run_manifest" => {
                let p: AiRunManifestParams = serde_json::from_value(args)
                    .map_err(|e| RpcDispatchError::InvalidParams(e.to_string()))?;
                let dir = p
                    .models_dir
                    .unwrap_or_else(|| "local_media/models".into());
                self.ai_run_manifest(
                    p.model_id.as_deref(),
                    p.manifest_path.as_deref().map(Path::new),
                    Path::new(&dir),
                    Path::new(&p.input_path),
                    Path::new(&p.out_path),
                    p.provider.as_deref(),
                    p.temporal_smooth,
                    p.params_json,
                    p.allow_unverified,
                    p.synthetic_test,
                )
                .map_err(RpcDispatchError::App)
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

#[derive(Debug, Default, Deserialize)]
struct AnalyzeMultibandParams {
    #[serde(default)]
    asset_id: Option<String>,
    #[serde(default)]
    window_ticks: Option<i64>,
    #[serde(default)]
    fft_size: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct RenderWaveformParams {
    out_path: String,
    #[serde(default)]
    asset_id: Option<String>,
    #[serde(default)]
    input_path: Option<String>,
    #[serde(default)]
    start_tick: Option<i64>,
    #[serde(default)]
    end_tick: Option<i64>,
    #[serde(default)]
    style: Option<String>,
    #[serde(default)]
    channel_layout: Option<String>,
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct SeparateStemsParams {
    input_path: String,
    out_dir: String,
    #[serde(default)]
    stems: Option<u8>,
    #[serde(default)]
    backend: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    device: Option<String>,
    #[serde(default)]
    output_format: Option<String>,
    #[serde(default)]
    model_dir: Option<String>,
    #[serde(default)]
    offline: bool,
}

#[derive(Debug, Deserialize)]
struct AlignLyricsParams {
    input_audio: String,
    #[serde(default)]
    lyrics_text: Option<String>,
    #[serde(default)]
    lyrics_file: Option<String>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    backend: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    compute_type: Option<String>,
    #[serde(default)]
    device: Option<String>,
    #[serde(default)]
    ja_tokenizer: Option<String>,
    #[serde(default)]
    vad: Option<bool>,
    #[serde(default)]
    model_dir: Option<String>,
    #[serde(default)]
    offline: bool,
    #[serde(default)]
    build_nodes: bool,
    #[serde(default)]
    style: Option<String>,
    #[serde(default)]
    base_input_node_id: Option<String>,
    #[serde(default)]
    vertical_japanese: bool,
}

#[derive(Debug, Deserialize)]
struct UpscaleMediaParams {
    input_path: String,
    out_path: String,
    #[serde(default)]
    scale: Option<u32>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    backend: Option<String>,
    #[serde(default)]
    gpu_id: Option<i32>,
    #[serde(default)]
    tile_size: Option<u32>,
    #[serde(default)]
    dedup_threshold: Option<f32>,
    #[serde(default)]
    scene_cut_threshold: Option<f32>,
    #[serde(default)]
    chunk_size: Option<usize>,
    #[serde(default)]
    tta: bool,
    #[serde(default)]
    platform_preset: Option<String>,
}

fn resolve_spec_and_props(
    spec: Option<Value>,
    spec_path: Option<&str>,
    props: Option<Value>,
    props_path: Option<&str>,
) -> Result<(ParametricTemplateSpec, Value), RpcDispatchError> {
    let spec_val = match (spec, spec_path) {
        (Some(v), _) => v,
        (None, Some(path)) => {
            let raw = std::fs::read_to_string(path).map_err(|e| RpcDispatchError::App(e.into()))?;
            serde_json::from_str(&raw).map_err(|e| RpcDispatchError::InvalidParams(e.to_string()))?
        }
        (None, None) => {
            return Err(RpcDispatchError::InvalidParams(
                "either 'spec' or 'spec_path' is required".into(),
            ));
        }
    };
    let parsed_spec: ParametricTemplateSpec = serde_json::from_value(spec_val)
        .map_err(|e| RpcDispatchError::InvalidParams(e.to_string()))?;

    let parsed_props = match (props, props_path) {
        (Some(v), _) => v,
        (None, Some(path)) => {
            let raw = std::fs::read_to_string(path).map_err(|e| RpcDispatchError::App(e.into()))?;
            serde_json::from_str(&raw).map_err(|e| RpcDispatchError::InvalidParams(e.to_string()))?
        }
        (None, None) => json!({}),
    };
    Ok((parsed_spec, parsed_props))
}

#[derive(Debug, Default, Deserialize)]
struct LintProjectParams {
    #[serde(default)]
    check_media: bool,
    #[serde(default)]
    strict_sandbox: bool,
    #[serde(default)]
    black_min_duration_sec: Option<f64>,
    #[serde(default)]
    black_pic_th: Option<f64>,
    #[serde(default)]
    max_blur_sub_samples_warn: Option<u32>,
    #[serde(default)]
    expected_revision_id: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct DiffFrameParams {
    #[serde(alias = "a")]
    path_a: String,
    #[serde(alias = "b")]
    path_b: String,
    #[serde(default)]
    threshold: Option<u8>,
    #[serde(default, alias = "out")]
    out_diff_png: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct CompileMotionDslParams {
    #[serde(default, alias = "markdown")]
    input: Option<String>,
    #[serde(default)]
    input_path: Option<String>,
    #[serde(default)]
    skip_lint: bool,
    #[serde(default)]
    apply_to_session: bool,
    #[serde(default)]
    expected_revision_id: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
struct TemplateParams {
    #[serde(default)]
    spec: Option<Value>,
    #[serde(default)]
    spec_path: Option<String>,
    #[serde(default)]
    props: Option<Value>,
    #[serde(default)]
    props_path: Option<String>,
    #[serde(default)]
    out_path: Option<String>,
    #[serde(default)]
    apply_to_session: bool,
    #[serde(default)]
    expected_revision_id: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct TrackPointParams {
    #[serde(default)]
    source_node_id: Option<String>,
    #[serde(default, alias = "asset")]
    asset_id: Option<String>,
    #[serde(alias = "target_node")]
    target_node_id: String,
    x: f32,
    y: f32,
    #[serde(default)]
    x2: Option<f32>,
    #[serde(default)]
    y2: Option<f32>,
    #[serde(default)]
    start_tick: Option<i64>,
    #[serde(default)]
    end_tick: Option<i64>,
    #[serde(default)]
    bind_mode: Option<String>,
    #[serde(default)]
    plane_z: Option<f32>,
    #[serde(default)]
    track_id_prefix: Option<String>,
    #[serde(default)]
    profile: Option<RenderProfileKind>,
    #[serde(default)]
    expected_revision_id: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct TrackPlanarParams {
    #[serde(default)]
    source_node_id: Option<String>,
    #[serde(default, alias = "asset")]
    asset_id: Option<String>,
    #[serde(alias = "target_node")]
    target_node_id: String,
    corners: [[f32; 2]; 4],
    #[serde(default)]
    start_tick: Option<i64>,
    #[serde(default)]
    end_tick: Option<i64>,
    #[serde(default)]
    bind_mode: Option<String>,
    #[serde(default)]
    plane_z: Option<f32>,
    #[serde(default)]
    track_id_prefix: Option<String>,
    #[serde(default)]
    profile: Option<RenderProfileKind>,
    #[serde(default)]
    expected_revision_id: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ExportNleParams {
    format: String,
    #[serde(default, alias = "out")]
    out_path: Option<String>,
    #[serde(default)]
    include_beat_markers: bool,
    #[serde(default)]
    expected_revision_id: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ImportNleParams {
    format: String,
    #[serde(default)]
    input: Option<String>,
    #[serde(default)]
    input_path: Option<String>,
    #[serde(default)]
    project_name: Option<String>,
    #[serde(default)]
    apply_to_session: bool,
    #[serde(default)]
    expected_revision_id: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
struct AiModelsParams {
    #[serde(default)]
    models_dir: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AiRunManifestParams {
    #[serde(default, alias = "model")]
    model_id: Option<String>,
    #[serde(default)]
    manifest_path: Option<String>,
    #[serde(default)]
    models_dir: Option<String>,
    #[serde(alias = "input")]
    input_path: String,
    #[serde(alias = "out")]
    out_path: String,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    temporal_smooth: Option<f32>,
    #[serde(default)]
    params_json: Option<Value>,
    #[serde(default)]
    allow_unverified: bool,
    #[serde(default)]
    synthetic_test: bool,
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

        let ai_caps_raw = server.handle_jsonrpc_str(
            &json!({
                "jsonrpc": "2.0",
                "id": 99,
                "method": "tools/call",
                "params": {
                    "name": "ai_capabilities",
                    "arguments": {}
                }
            })
            .to_string(),
        );
        let ai_caps: Value = serde_json::from_str(&ai_caps_raw).unwrap();
        assert!(ai_caps["result"]["structuredContent"]["sidecar_script_found"].as_bool().unwrap());
        assert!(ai_caps["result"]["structuredContent"]["ai_audio"]["python_available"].is_boolean());
        assert!(ai_caps["result"]["structuredContent"]["realesrgan"]["available"].is_boolean());
    }

    #[test]
    fn describe_schema_and_wave2_mcp_tools_work() {
        let mut server = McpServer::new(Project::golden_scene_preset()).expect("valid server");
        let desc = server.describe_schema();
        for expected_var in ["audio_sub", "audio_mid", "audio_high", "downbeat_phase"] {
            assert!(
                desc.safe_expr_variables.iter().any(|v| v == expected_var),
                "missing SafeExpr var '{expected_var}'"
            );
        }
        assert!(desc.node_kinds.iter().any(|k| k == "anime4k"));
        assert!(desc.anime4k_presets.iter().any(|p| p == "mode_a"));
        assert!(desc.scene_depth_plate_fields.iter().any(|f| f == "rg16_packed"));

        let tools = server.list_tools();
        for required in [
            "analyze_multiband_beats",
            "render_waveform",
            "separate_stems",
            "align_lyrics",
            "upscale_media",
        ] {
            assert!(
                tools.iter().any(|t| t.name == required),
                "missing MCP tool '{required}'"
            );
        }

        let mb_raw = server.handle_jsonrpc_str(
            &json!({
                "jsonrpc": "2.0",
                "id": 100,
                "method": "tools/call",
                "params": {
                    "name": "analyze_multiband_beats",
                    "arguments": {}
                }
            })
            .to_string(),
        );
        let mb_val: Value = serde_json::from_str(&mb_raw).unwrap();
        assert!(!mb_val["result"]["isError"].as_bool().unwrap());
        assert!(
            mb_val["result"]["structuredContent"]["estimated_bpm"]
                .as_f64()
                .unwrap()
                > 0.0
        );

        let wave_path = std::env::temp_dir().join(format!(
            "frameshift_mcp_wave_{}.png",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&wave_path);
        let wave_raw = server.handle_jsonrpc_str(
            &json!({
                "jsonrpc": "2.0",
                "id": 101,
                "method": "tools/call",
                "params": {
                    "name": "render_waveform",
                    "arguments": {
                        "out_path": wave_path,
                        "style": "spectral",
                        "width": 640,
                        "height": 180
                    }
                }
            })
            .to_string(),
        );
        let wave_val: Value = serde_json::from_str(&wave_raw).unwrap();
        assert_eq!(wave_val["result"]["structuredContent"]["width"], 640);
        assert!(wave_path.is_file());
        let _ = std::fs::remove_file(&wave_path);
    }
}
