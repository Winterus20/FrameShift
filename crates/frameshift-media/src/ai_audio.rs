//! AI stem separation (`audio-separator` MDX-Net / `htdemucs`) and word-level lyric alignment
//! (`stable-ts` forced alignment / `faster-whisper` + Japanese `fugashi`) sidecar bridge.
//!
//! Python dependencies live in an isolated Python 3.12 virtual environment at
//! `local_media/.venv-audio` so the default system Python 3.14 ONNX DirectML environment
//! remains untouched. Deterministic `Flicks(i64)` conversion happens strictly in Rust.

use crate::error::MediaError;
use crate::interpolator::find_executable;
use frameshift_time::{FLICKS_PER_SECOND, Flicks};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Host capability report for the isolated AI audio sidecar (`local_media/.venv-audio`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiAudioCapabilities {
    pub python_available: bool,
    pub python_path: Option<PathBuf>,
    pub venv_audio_found: bool,
    pub sidecar_script_found: bool,
    pub sidecar_script_path: Option<PathBuf>,
    pub model_cache_dir: PathBuf,
    pub python_version: Option<String>,
    pub torch_available: bool,
    pub torch_version: Option<String>,
    pub torch_cuda_available: bool,
    pub cuda_device_name: Option<String>,
    pub audio_separator_available: bool,
    pub audio_separator_version: Option<String>,
    pub demucs_available: bool,
    pub demucs_version: Option<String>,
    pub faster_whisper_available: bool,
    pub faster_whisper_version: Option<String>,
    pub stable_whisper_available: bool,
    pub stable_whisper_version: Option<String>,
    pub fugashi_available: bool,
    pub unidic_lite_available: bool,
    pub onnxruntime_available: bool,
    pub onnxruntime_version: Option<String>,
    pub onnx_providers: Vec<String>,
    pub ffmpeg_available: bool,
}

/// Alias matching the architecture specification naming.
pub type AiAudioCapabilitiesReport = AiAudioCapabilities;

/// Backend engine used for audio stem separation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StemBackend {
    /// Automatically prefers `audio-separator` (MDX-Net / htdemucs) then `demucs`.
    #[default]
    Auto,
    AudioSeparator,
    Demucs,
    MdxOnnx,
}

impl StemBackend {
    pub fn cli_flag(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::AudioSeparator => "audio-separator",
            Self::Demucs => "demucs",
            Self::MdxOnnx => "mdx-onnx",
        }
    }
}

/// Request parameters for AI stem separation (`separate`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StemSeparationRequest {
    pub input_path: PathBuf,
    pub out_dir: PathBuf,
    #[serde(default)]
    pub backend: StemBackend,
    /// Optional model name or filename (e.g. `"UVR-MDX-NET-Voc_FT.onnx"`, `"htdemucs.yaml"`).
    #[serde(default)]
    pub model: Option<String>,
    /// Number of stems: `2` (`vocals` + `instrumental`) or `4` (`vocals`, `drums`, `bass`, `other`).
    pub stems: u8,
    /// Execution device preference: `"auto"`, `"cuda"`, `"dml"`, or `"cpu"`.
    pub device: String,
    /// Output audio format (`"wav"`, `"flac"`, `"mp3"`).
    pub output_format: String,
    /// Optional model cache directory (defaults to `local_media/models`).
    pub model_dir: Option<PathBuf>,
    /// Disables remote HuggingFace / PyTorch model downloads.
    pub offline: bool,
    /// Optional explicit Python interpreter (defaults to `local_media/.venv-audio`).
    pub python_binary: Option<PathBuf>,
    /// Optional explicit path to `scripts/ai_audio_sidecar.py`.
    pub sidecar_script: Option<PathBuf>,
}

impl Default for StemSeparationRequest {
    fn default() -> Self {
        Self {
            input_path: PathBuf::new(),
            out_dir: PathBuf::new(),
            backend: StemBackend::Auto,
            model: None,
            stems: 2,
            device: "auto".to_owned(),
            output_format: "wav".to_owned(),
            model_dir: None,
            offline: false,
            python_binary: None,
            sidecar_script: None,
        }
    }
}

/// One separated stem WAV/FLAC artifact produced by the sidecar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeparatedStemFile {
    pub role: String,
    pub path: PathBuf,
    pub sample_rate_hz: u32,
    pub duration_ticks: Flicks,
}

/// Result returned by [`separate_audio_stems`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StemSeparationResult {
    pub input_path: PathBuf,
    pub backend: String,
    pub model: String,
    pub stems_requested: u8,
    pub device: String,
    pub model_cache_dir: PathBuf,
    pub stems: Vec<SeparatedStemFile>,
}

impl StemSeparationResult {
    /// Returns the first separated stem matching `role` (e.g. `"vocals"`, `"drums"`).
    pub fn stem(&self, role: &str) -> Option<&SeparatedStemFile> {
        self.stems.iter().find(|s| s.role == role)
    }

    /// Convenience accessor for the `"vocals"` stem.
    pub fn vocals(&self) -> Option<&SeparatedStemFile> {
        self.stem("vocals")
    }
}

/// Backend engine used for word-level lyric alignment or transcription.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LyricAlignBackend {
    /// Uses `stable-ts` forced alignment when lyrics are provided, else `faster-whisper`.
    #[default]
    Auto,
    StableTs,
    FasterWhisper,
}

impl LyricAlignBackend {
    pub fn cli_flag(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::StableTs => "stable-ts",
            Self::FasterWhisper => "faster-whisper",
        }
    }
}

/// Japanese segmentation strategy for space-less Japanese lyrics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JapaneseTokenizer {
    #[default]
    Fugashi,
    Char,
    Raw,
}

impl JapaneseTokenizer {
    pub fn cli_flag(self) -> &'static str {
        match self {
            Self::Fugashi => "fugashi",
            Self::Char => "char",
            Self::Raw => "raw",
        }
    }
}

/// Request parameters for hybrid lyric alignment (`align`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LyricAlignRequest {
    pub input_audio: PathBuf,
    /// Known lyrics text passed inline via `--lyrics-text`.
    pub lyrics_text: Option<String>,
    /// Path to UTF-8 known lyrics file passed via `--lyrics`.
    pub lyrics_file: Option<PathBuf>,
    /// Optional ISO language code (`"en"`, `"ja"`, or `None` for `"auto"`).
    pub language: Option<String>,
    #[serde(default)]
    pub backend: LyricAlignBackend,
    pub model: String,
    pub compute_type: String,
    pub device: String,
    #[serde(default)]
    pub ja_tokenizer: JapaneseTokenizer,
    pub vad: bool,
    pub model_dir: Option<PathBuf>,
    pub offline: bool,
    pub python_binary: Option<PathBuf>,
    pub sidecar_script: Option<PathBuf>,
}

impl Default for LyricAlignRequest {
    fn default() -> Self {
        Self {
            input_audio: PathBuf::new(),
            lyrics_text: None,
            lyrics_file: None,
            language: None,
            backend: LyricAlignBackend::Auto,
            model: "large-v3-turbo".to_owned(),
            compute_type: "float16".to_owned(),
            device: "auto".to_owned(),
            ja_tokenizer: JapaneseTokenizer::Fugashi,
            vad: true,
            model_dir: None,
            offline: false,
            python_binary: None,
            sidecar_script: None,
        }
    }
}

/// Word-level timing converted deterministically to integer `Flicks(i64)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WordTimingTicks {
    pub text: String,
    pub start_tick: Flicks,
    pub end_tick: Flicks,
    pub confidence: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
}

/// Line/phrase segment containing deterministic word timings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LyricSegmentTicks {
    pub index: usize,
    pub text: String,
    pub start_tick: Flicks,
    pub end_tick: Flicks,
    pub words: Vec<WordTimingTicks>,
}

/// Deterministic `Flicks(i64)` lyric alignment output returned by [`align_audio_lyrics`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LyricAlignResult {
    pub source_audio: PathBuf,
    pub language: String,
    pub backend: String,
    pub model: String,
    pub duration_ticks: Flicks,
    pub segments: Vec<LyricSegmentTicks>,
}

impl LyricAlignResult {
    /// Flattens every aligned word across all segments in chronological order.
    pub fn all_words(&self) -> Vec<&WordTimingTicks> {
        self.segments
            .iter()
            .flat_map(|seg| seg.words.iter())
            .collect()
    }
}

/// Target node kind generated by [`build_lyric_timeline_nodes`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LyricNodeStyle {
    #[default]
    VectorText,
    Text3D,
}

/// Styling and layout template parameters for [`build_lyric_timeline_nodes`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LyricTimelineTemplateConfig {
    pub style: LyricNodeStyle,
    /// Existing render graph node ID onto which lyric overlays are blended (`BlendMode::Normal`).
    pub base_input_node_id: String,
    pub id_prefix: String,
    /// Uses `writing_mode: "vertical_rl"` for `VectorText` when Japanese text is detected/requested.
    pub vertical_japanese: bool,
    pub font_size_norm: f32,
    /// Linear Rec.709 `[r, g, b, a]` text color.
    pub color_rgba: [f32; 4],
    /// Linear Rec.709 `[r, g, b, a]` outline color.
    pub outline_rgba: [f32; 4],
    pub outline_width_norm: f32,
    /// Frame ratio `[0..1]` where `0.5` is horizontal center.
    pub x_offset_norm: f32,
    /// Frame ratio `[0..1]` where `0.5` is vertical center.
    pub y_offset_norm: f32,
    pub font_family: Option<String>,
    pub font_path: Option<String>,
    pub pop_duration_ticks: i64,
    pub char_delay_ticks: i64,
    pub text3d_font_size: f32,
    pub text3d_pop_duration_sec: f32,
    pub text3d_pop_overshoot: f32,
    pub text3d_char_stagger_sec: f32,
}

impl Default for LyricTimelineTemplateConfig {
    fn default() -> Self {
        Self {
            style: LyricNodeStyle::VectorText,
            base_input_node_id: "grade_main".to_owned(),
            id_prefix: "lyric".to_owned(),
            vertical_japanese: false,
            font_size_norm: 0.068,
            color_rgba: [1.0, 0.98, 0.94, 1.0],
            outline_rgba: [0.02, 0.02, 0.04, 0.95],
            outline_width_norm: 0.005,
            x_offset_norm: 0.5,
            y_offset_norm: 0.78,
            font_family: None,
            font_path: None,
            pop_duration_ticks: 84_672_000, // 120 ms in Flicks
            char_delay_ticks: 14_112_000,   // 20 ms in Flicks
            text3d_font_size: 0.35,
            text3d_pop_duration_sec: 0.14,
            text3d_pop_overshoot: 1.22,
            text3d_char_stagger_sec: 0.025,
        }
    }
}

/// Parameter binding descriptor compatible with `frameshift_schema::ParamBindingDef`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LyricParamBindingTemplate {
    pub node_id: String,
    pub param_name: String,
    pub expression: String,
}

/// Event anchor descriptor compatible with `frameshift_schema::EventAnchorDef`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LyricEventAnchorTemplate {
    pub event_id: String,
    pub label: String,
    pub target_output_tick: Flicks,
    pub duration_ticks: Flicks,
    pub confidence: f32,
}

/// Generated render-graph node JSON definitions (`GraphNodeDef`-compatible), visibility
/// expressions, and event anchors returned by [`build_lyric_timeline_nodes`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LyricTimelineNodesTemplate {
    /// Serialized `GraphNodeDef` JSON objects ready for `serde_json::from_value` in core/MCP.
    pub nodes: Vec<serde_json::Value>,
    pub param_bindings: Vec<LyricParamBindingTemplate>,
    pub event_anchors: Vec<LyricEventAnchorTemplate>,
    pub output_node_id: String,
}

#[derive(Debug, Deserialize)]
struct SidecarAudioProbeJson {
    python_version: Option<String>,
    #[serde(default)]
    torch_available: bool,
    torch_version: Option<String>,
    #[serde(default)]
    torch_cuda_available: bool,
    cuda_device_name: Option<String>,
    #[serde(default)]
    audio_separator_available: bool,
    audio_separator_version: Option<String>,
    #[serde(default)]
    demucs_available: bool,
    demucs_version: Option<String>,
    #[serde(default)]
    faster_whisper_available: bool,
    faster_whisper_version: Option<String>,
    #[serde(default)]
    stable_whisper_available: bool,
    stable_whisper_version: Option<String>,
    #[serde(default)]
    fugashi_available: bool,
    #[serde(default)]
    unidic_lite_available: bool,
    #[serde(default)]
    onnxruntime_available: bool,
    onnxruntime_version: Option<String>,
    #[serde(default)]
    onnx_providers: Vec<String>,
    #[serde(default)]
    ffmpeg_available: bool,
    model_cache_dir: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct SidecarStemFileJson {
    role: String,
    path: PathBuf,
    sample_rate_hz: u32,
    duration_sec: f64,
}

#[derive(Debug, Deserialize)]
struct SidecarStemResultJson {
    input_path: PathBuf,
    backend: String,
    model: String,
    stems_requested: u8,
    device: String,
    model_cache_dir: PathBuf,
    stems: Vec<SidecarStemFileJson>,
}

#[derive(Debug, Deserialize)]
struct SidecarWordJson {
    text: String,
    start_sec: f64,
    end_sec: f64,
    #[serde(default = "default_confidence")]
    confidence: f32,
    #[serde(default)]
    speaker: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SidecarSegmentJson {
    index: usize,
    text: String,
    start_sec: f64,
    end_sec: f64,
    #[serde(default)]
    words: Vec<SidecarWordJson>,
}

#[derive(Debug, Deserialize)]
struct SidecarLyricResultJson {
    source_audio: PathBuf,
    language: String,
    backend: String,
    model: String,
    duration_sec: f64,
    #[serde(default)]
    segments: Vec<SidecarSegmentJson>,
}

fn default_confidence() -> f32 {
    1.0
}

fn venv_python_candidates_under(root: &Path) -> [PathBuf; 2] {
    [
        root.join("local_media")
            .join(".venv-audio")
            .join("Scripts")
            .join("python.exe"),
        root.join("local_media")
            .join(".venv-audio")
            .join("bin")
            .join("python"),
    ]
}

fn repo_root_candidates() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        for ancestor in cwd.ancestors() {
            roots.push(ancestor.to_path_buf());
        }
    }
    let manifest_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    roots.push(manifest_root);
    if let Ok(exe) = std::env::current_exe() {
        for ancestor in exe.ancestors() {
            roots.push(ancestor.to_path_buf());
        }
    }
    roots
}

/// Checks whether `path` resides inside the isolated `.venv-audio` virtual environment.
pub fn is_isolated_audio_venv(path: &Path) -> bool {
    path.components()
        .any(|c| c.as_os_str().to_string_lossy() == ".venv-audio")
}

/// Resolves the isolated Python 3.12 audio interpreter (`local_media/.venv-audio`),
/// falling back to `FRAMESHIFT_AUDIO_PYTHON` or `PATH` only if the venv is absent.
pub fn resolve_audio_python(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = explicit {
        return p.is_file().then(|| p.to_path_buf());
    }
    if let Some(env_py) = std::env::var_os("FRAMESHIFT_AUDIO_PYTHON") {
        let p = PathBuf::from(env_py);
        if p.is_file() {
            return Some(p);
        }
    }
    for root in repo_root_candidates() {
        for candidate in venv_python_candidates_under(&root) {
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    find_executable(None, &["python", "python3"])
}

/// Resolves `scripts/ai_audio_sidecar.py` relative to cwd, crate manifest, or executable ancestors.
pub fn resolve_ai_audio_sidecar_script(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = explicit {
        return p.is_file().then(|| p.to_path_buf());
    }
    for root in repo_root_candidates() {
        let candidate = root.join("scripts").join("ai_audio_sidecar.py");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Resolves the model cache directory (`local_media/models`).
pub fn resolve_audio_model_cache_dir(explicit: Option<&Path>) -> PathBuf {
    if let Some(p) = explicit {
        return p.to_path_buf();
    }
    for root in repo_root_candidates() {
        let candidate = root.join("local_media").join("models");
        if candidate.is_dir() || root.join("Cargo.toml").is_file() {
            return candidate;
        }
    }
    PathBuf::from("local_media").join("models")
}

/// Converts finite non-negative seconds into deterministic integer [`Flicks`].
pub fn seconds_to_flicks(seconds: f64) -> Result<Flicks, MediaError> {
    if !seconds.is_finite() || seconds < 0.0 {
        return Err(MediaError::ParseError(format!(
            "timestamp seconds must be finite and non-negative, got {seconds}"
        )));
    }
    let scaled = (seconds * (FLICKS_PER_SECOND as f64)).round();
    if !scaled.is_finite() || scaled > (i64::MAX as f64) {
        return Err(MediaError::ParseError(format!(
            "timestamp {seconds}s overflows Flicks(i64)"
        )));
    }
    Ok(Flicks(scaled as i64))
}

/// Probes the isolated AI audio sidecar environment (`local_media/.venv-audio`).
pub fn detect_ai_audio_capabilities() -> AiAudioCapabilities {
    detect_ai_audio_capabilities_with(None, None)
}

/// Probes AI audio sidecar capabilities with optional explicit Python/script overrides.
pub fn detect_ai_audio_capabilities_with(
    python_binary: Option<&Path>,
    sidecar_script: Option<&Path>,
) -> AiAudioCapabilities {
    let python_path = resolve_audio_python(python_binary);
    let sidecar_script_path = resolve_ai_audio_sidecar_script(sidecar_script);
    let default_cache = resolve_audio_model_cache_dir(None);
    let venv_audio_found = python_path
        .as_deref()
        .map(is_isolated_audio_venv)
        .unwrap_or(false);

    let mut report = AiAudioCapabilities {
        python_available: python_path.is_some(),
        python_path: python_path.clone(),
        venv_audio_found,
        sidecar_script_found: sidecar_script_path.is_some(),
        sidecar_script_path: sidecar_script_path.clone(),
        model_cache_dir: default_cache,
        python_version: None,
        torch_available: false,
        torch_version: None,
        torch_cuda_available: false,
        cuda_device_name: None,
        audio_separator_available: false,
        audio_separator_version: None,
        demucs_available: false,
        demucs_version: None,
        faster_whisper_available: false,
        faster_whisper_version: None,
        stable_whisper_available: false,
        stable_whisper_version: None,
        fugashi_available: false,
        unidic_lite_available: false,
        onnxruntime_available: false,
        onnxruntime_version: None,
        onnx_providers: Vec::new(),
        ffmpeg_available: find_executable(None, &["ffmpeg"]).is_some(),
    };

    if let (Some(py), Some(script)) = (&python_path, &sidecar_script_path)
        && let Ok(out) = Command::new(py).arg(script).arg("--probe").output()
        && out.status.success()
        && let Ok(parsed) = serde_json::from_slice::<SidecarAudioProbeJson>(&out.stdout)
    {
        report.python_version = parsed.python_version;
        report.torch_available = parsed.torch_available;
        report.torch_version = parsed.torch_version;
        report.torch_cuda_available = parsed.torch_cuda_available;
        report.cuda_device_name = parsed.cuda_device_name;
        report.audio_separator_available = parsed.audio_separator_available;
        report.audio_separator_version = parsed.audio_separator_version;
        report.demucs_available = parsed.demucs_available;
        report.demucs_version = parsed.demucs_version;
        report.faster_whisper_available = parsed.faster_whisper_available;
        report.faster_whisper_version = parsed.faster_whisper_version;
        report.stable_whisper_available = parsed.stable_whisper_available;
        report.stable_whisper_version = parsed.stable_whisper_version;
        report.fugashi_available = parsed.fugashi_available;
        report.unidic_lite_available = parsed.unidic_lite_available;
        report.onnxruntime_available = parsed.onnxruntime_available;
        report.onnxruntime_version = parsed.onnxruntime_version;
        report.onnx_providers = parsed.onnx_providers;
        report.ffmpeg_available = report.ffmpeg_available || parsed.ffmpeg_available;
        if let Some(cache) = parsed.model_cache_dir {
            report.model_cache_dir = cache;
        }
    }

    report
}

fn resolve_python_and_script(
    python_binary: Option<&Path>,
    sidecar_script: Option<&Path>,
) -> Result<(PathBuf, PathBuf), MediaError> {
    let py = resolve_audio_python(python_binary).ok_or_else(|| MediaError::BinarySpawnFailed {
        binary: "local_media/.venv-audio/Scripts/python.exe".into(),
        reason: "isolated audio Python interpreter not found".into(),
    })?;
    let script =
        resolve_ai_audio_sidecar_script(sidecar_script).ok_or_else(|| MediaError::IoError {
            message: "could not locate scripts/ai_audio_sidecar.py".into(),
        })?;
    Ok((py, script))
}

fn run_json_sidecar(mut cmd: Command, action: &str) -> Result<String, MediaError> {
    let out = cmd.output().map_err(|e| MediaError::BinarySpawnFailed {
        binary: action.into(),
        reason: e.to_string(),
    })?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_owned();
        return Err(MediaError::ProcessExit {
            command: action.into(),
            exit_code: out.status.code(),
            stderr,
        });
    }
    String::from_utf8(out.stdout)
        .map_err(|e| MediaError::ParseError(format!("{action} output is not valid UTF-8: {e}")))
}

/// Parses `scripts/ai_audio_sidecar.py separate` stdout JSON and converts stem durations to [`Flicks`].
pub fn parse_stem_separation_json(json_str: &str) -> Result<StemSeparationResult, MediaError> {
    let raw: SidecarStemResultJson = serde_json::from_str(json_str)
        .map_err(|e| MediaError::ParseError(format!("invalid stem separation JSON: {e}")))?;
    let mut stems = Vec::with_capacity(raw.stems.len());
    for stem in raw.stems {
        stems.push(SeparatedStemFile {
            role: stem.role,
            path: stem.path,
            sample_rate_hz: stem.sample_rate_hz,
            duration_ticks: seconds_to_flicks(stem.duration_sec)?,
        });
    }
    Ok(StemSeparationResult {
        input_path: raw.input_path,
        backend: raw.backend,
        model: raw.model,
        stems_requested: raw.stems_requested,
        device: raw.device,
        model_cache_dir: raw.model_cache_dir,
        stems,
    })
}

/// Runs AI stem separation via `scripts/ai_audio_sidecar.py separate`.
pub fn separate_audio_stems(req: &StemSeparationRequest) -> Result<StemSeparationResult, MediaError> {
    if req.stems != 2 && req.stems != 4 {
        return Err(MediaError::InvalidAudioFormat(format!(
            "stem separation supports 2 or 4 stems, got {}",
            req.stems
        )));
    }
    if !req.input_path.is_file() {
        return Err(MediaError::IoError {
            message: format!("input media file '{}' not found", req.input_path.display()),
        });
    }

    let (py, script) =
        resolve_python_and_script(req.python_binary.as_deref(), req.sidecar_script.as_deref())?;
    let model_dir = resolve_audio_model_cache_dir(req.model_dir.as_deref());

    let mut cmd = Command::new(&py);
    cmd.arg(&script)
        .arg("separate")
        .arg("--input")
        .arg(&req.input_path)
        .arg("--out-dir")
        .arg(&req.out_dir)
        .args(["--backend", req.backend.cli_flag()])
        .args(["--stems", &req.stems.to_string()])
        .args(["--device", &req.device])
        .args(["--output-format", &req.output_format])
        .arg("--model-dir")
        .arg(&model_dir);

    if let Some(model) = &req.model {
        cmd.args(["--model", model]);
    }
    if req.offline {
        cmd.arg("--offline");
    }

    let stdout = run_json_sidecar(cmd, "ai_audio_sidecar separate")?;
    parse_stem_separation_json(&stdout)
}

/// Parses `scripts/ai_audio_sidecar.py align` stdout JSON and converts all word/segment timestamps
/// deterministically into integer [`Flicks`].
pub fn parse_lyric_alignment_json(json_str: &str) -> Result<LyricAlignResult, MediaError> {
    let raw: SidecarLyricResultJson = serde_json::from_str(json_str)
        .map_err(|e| MediaError::ParseError(format!("invalid lyric alignment JSON: {e}")))?;

    let mut segments = Vec::with_capacity(raw.segments.len());
    for seg in raw.segments {
        let seg_start = seconds_to_flicks(seg.start_sec)?;
        let seg_end = seconds_to_flicks(seg.end_sec.max(seg.start_sec))?;

        let mut words = Vec::with_capacity(seg.words.len());
        for w in seg.words {
            let start_tick = seconds_to_flicks(w.start_sec)?;
            let end_tick = seconds_to_flicks(w.end_sec.max(w.start_sec))?;
            words.push(WordTimingTicks {
                text: w.text,
                start_tick,
                end_tick,
                confidence: w.confidence.clamp(0.0, 1.0),
                speaker: w.speaker,
            });
        }

        segments.push(LyricSegmentTicks {
            index: seg.index,
            text: seg.text,
            start_tick: seg_start,
            end_tick: seg_end,
            words,
        });
    }

    Ok(LyricAlignResult {
        source_audio: raw.source_audio,
        language: raw.language,
        backend: raw.backend,
        model: raw.model,
        duration_ticks: seconds_to_flicks(raw.duration_sec)?,
        segments,
    })
}

/// Runs hybrid word-level lyric alignment via `scripts/ai_audio_sidecar.py align`.
pub fn align_audio_lyrics(req: &LyricAlignRequest) -> Result<LyricAlignResult, MediaError> {
    if !req.input_audio.is_file() {
        return Err(MediaError::IoError {
            message: format!("input audio file '{}' not found", req.input_audio.display()),
        });
    }
    if let Some(lyrics_path) = &req.lyrics_file
        && !lyrics_path.is_file()
    {
        return Err(MediaError::IoError {
            message: format!("lyrics file '{}' not found", lyrics_path.display()),
        });
    }

    let (py, script) =
        resolve_python_and_script(req.python_binary.as_deref(), req.sidecar_script.as_deref())?;
    let model_dir = resolve_audio_model_cache_dir(req.model_dir.as_deref());

    let mut cmd = Command::new(&py);
    cmd.arg(&script)
        .arg("align")
        .arg("--input")
        .arg(&req.input_audio)
        .args(["--backend", req.backend.cli_flag()])
        .args(["--model", &req.model])
        .args(["--compute-type", &req.compute_type])
        .args(["--device", &req.device])
        .args(["--ja-tokenizer", req.ja_tokenizer.cli_flag()])
        .arg("--model-dir")
        .arg(&model_dir);

    if let Some(lang) = &req.language {
        cmd.args(["--language", lang]);
    }
    if let Some(lyrics_path) = &req.lyrics_file {
        cmd.arg("--lyrics").arg(lyrics_path);
    } else if let Some(text) = &req.lyrics_text {
        cmd.args(["--lyrics-text", text]);
    }
    if req.vad {
        cmd.arg("--vad");
    }
    if req.offline {
        cmd.arg("--offline");
    }

    let stdout = run_json_sidecar(cmd, "ai_audio_sidecar align")?;
    parse_lyric_alignment_json(&stdout)
}

/// Builds `GraphNodeDef`-compatible JSON nodes (`VectorText` or `Text3D` + `Blend` `Normal` chain),
/// `step(t_start, t_sec) * (1.0 - step(t_end, t_sec))` `opacity` bindings, and event anchors
/// for a sequence of aligned words.
pub fn build_lyric_timeline_nodes(
    words: &[WordTimingTicks],
    language: &str,
    config: &LyricTimelineTemplateConfig,
) -> LyricTimelineNodesTemplate {
    let mut nodes = Vec::new();
    let mut param_bindings = Vec::new();
    let mut event_anchors = Vec::new();
    let mut current_base = config.base_input_node_id.clone();

    let use_vertical_ja = config.vertical_japanese && language.eq_ignore_ascii_case("ja");
    let writing_mode = if use_vertical_ja {
        "vertical_rl"
    } else {
        "horizontal_tb"
    };

    for (idx, word) in words.iter().enumerate() {
        let trimmed = word.text.trim();
        if trimmed.is_empty() {
            continue;
        }
        let end_ticks = word.end_tick.0.max(word.start_tick.0 + 1);
        let t_start = (word.start_tick.0 as f64) / (FLICKS_PER_SECOND as f64);
        let t_end = (end_ticks as f64) / (FLICKS_PER_SECOND as f64);

        let text_node_id = format!("{}_word_{idx:04}", config.id_prefix);
        let blend_node_id = format!("{}_blend_{idx:04}", config.id_prefix);

        let text_node = match config.style {
            LyricNodeStyle::VectorText => json!({
                "id": text_node_id,
                "kind": {
                    "type": "vector_text",
                    "text": trimmed,
                    "font_size_norm": config.font_size_norm,
                    "color_rgba": config.color_rgba,
                    "outline_rgba": config.outline_rgba,
                    "outline_width_norm": config.outline_width_norm,
                    "align_center": true,
                    "x_offset_norm": config.x_offset_norm,
                    "y_offset_norm": config.y_offset_norm,
                    "font_family": config.font_family,
                    "font_path": config.font_path,
                    "opacity": 1.0,
                    "writing_mode": writing_mode,
                    "gpu_slug": true,
                    "text_animator": {
                        "unit": "grapheme",
                        "start_tick": word.start_tick.0,
                        "duration_ticks": config.pop_duration_ticks,
                        "delay_ticks": config.char_delay_ticks,
                        "scale_from": 1.35,
                        "opacity_from": 0.0,
                        "shape": "smooth"
                    }
                },
                "inputs": [],
                "enabled": true
            }),
            LyricNodeStyle::Text3D => json!({
                "id": text_node_id,
                "kind": {
                    "type": "text3_d",
                    "text": trimmed,
                    "font_path": config.font_path.clone().unwrap_or_default(),
                    "font_size": config.text3d_font_size,
                    "color_top_rgba": config.color_rgba,
                    "color_bottom_rgba": config.color_rgba,
                    "outline_color_rgba": config.outline_rgba,
                    "time_offset_sec": -(t_start as f32),
                    "pop_duration_sec": config.text3d_pop_duration_sec,
                    "pop_overshoot": config.text3d_pop_overshoot,
                    "char_stagger_sec": config.text3d_char_stagger_sec,
                    "opacity": 1.0
                },
                "inputs": [],
                "enabled": true
            }),
        };

        let blend_node = json!({
            "id": blend_node_id,
            "kind": {
                "type": "blend",
                "mode": "normal",
                "opacity": 1.0
            },
            "inputs": [current_base, text_node_id],
            "enabled": true
        });

        nodes.push(text_node);
        nodes.push(blend_node);

        param_bindings.push(LyricParamBindingTemplate {
            node_id: text_node_id,
            param_name: "opacity".to_owned(),
            expression: format!("step({t_start:.6}, t_sec) * (1.0 - step({t_end:.6}, t_sec))"),
        });

        event_anchors.push(LyricEventAnchorTemplate {
            event_id: format!("{}_w_{idx:04}", config.id_prefix),
            label: trimmed.to_owned(),
            target_output_tick: word.start_tick,
            duration_ticks: Flicks(end_ticks - word.start_tick.0),
            confidence: word.confidence,
        });

        current_base = blend_node_id;
    }

    LyricTimelineNodesTemplate {
        nodes,
        param_bindings,
        event_anchors,
        output_node_id: current_base,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seconds_to_flicks_is_deterministic_at_audio_and_video_rates() {
        assert_eq!(seconds_to_flicks(0.0).unwrap(), Flicks(0));
        assert_eq!(seconds_to_flicks(1.0).unwrap(), Flicks(705_600_000));
        assert_eq!(seconds_to_flicks(0.5).unwrap(), Flicks(352_800_000));
        assert!(seconds_to_flicks(-0.1).is_err());
        assert!(seconds_to_flicks(f64::NAN).is_err());
    }

    #[test]
    fn parse_lyric_alignment_json_converts_words_to_flicks() {
        let sample = r#"{
            "schema": "frameshift.ai_audio.lyrics.v1",
            "source_audio": "vocals.wav",
            "language": "ja",
            "backend": "stable-ts",
            "model": "large-v3-turbo",
            "duration_sec": 4.0,
            "segments": [
                {
                    "index": 0,
                    "text": "世界が",
                    "start_sec": 1.0,
                    "end_sec": 2.5,
                    "words": [
                        { "text": "世界", "start_sec": 1.0, "end_sec": 1.5, "confidence": 0.94, "speaker": null },
                        { "text": "が", "start_sec": 1.5, "end_sec": 2.0, "confidence": 0.89, "speaker": null }
                    ]
                }
            ]
        }"#;

        let parsed = parse_lyric_alignment_json(sample).unwrap();
        assert_eq!(parsed.language, "ja");
        assert_eq!(parsed.duration_ticks, Flicks(4 * 705_600_000));
        assert_eq!(parsed.segments.len(), 1);
        let words = parsed.all_words();
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].text, "世界");
        assert_eq!(words[0].start_tick, Flicks(705_600_000));
        assert_eq!(words[0].end_tick, Flicks(1_058_400_000));
        assert_eq!(words[1].start_tick, Flicks(1_058_400_000));
    }

    #[test]
    fn parse_stem_separation_json_maps_roles_and_ticks() {
        let sample = r#"{
            "schema": "frameshift.ai_audio.stems.v1",
            "input_path": "song.wav",
            "backend": "audio-separator",
            "model": "UVR-MDX-NET-Voc_FT.onnx",
            "stems_requested": 2,
            "device": "cuda",
            "model_cache_dir": "local_media/models",
            "stems": [
                { "role": "vocals", "path": "out/vocals.wav", "sample_rate_hz": 44100, "duration_sec": 2.0 },
                { "role": "instrumental", "path": "out/inst.wav", "sample_rate_hz": 44100, "duration_sec": 2.0 }
            ]
        }"#;

        let parsed = parse_stem_separation_json(sample).unwrap();
        assert_eq!(parsed.vocals().unwrap().duration_ticks, Flicks(1_411_200_000));
        assert_eq!(parsed.stem("instrumental").unwrap().sample_rate_hz, 44100);
    }

    #[test]
    fn build_lyric_timeline_nodes_creates_vector_and_text3d_chains() {
        let words = vec![
            WordTimingTicks {
                text: "世界".to_owned(),
                start_tick: Flicks(705_600_000),
                end_tick: Flicks(1_058_400_000),
                confidence: 0.95,
                speaker: None,
            },
            WordTimingTicks {
                text: "が".to_owned(),
                start_tick: Flicks(1_058_400_000),
                end_tick: Flicks(1_411_200_000),
                confidence: 0.90,
                speaker: None,
            },
        ];

        let cfg = LyricTimelineTemplateConfig {
            vertical_japanese: true,
            ..Default::default()
        };
        let tpl = build_lyric_timeline_nodes(&words, "ja", &cfg);
        assert_eq!(tpl.nodes.len(), 4);
        assert_eq!(tpl.param_bindings.len(), 2);
        assert_eq!(tpl.event_anchors.len(), 2);
        assert_eq!(tpl.output_node_id, "lyric_blend_0001");
        assert_eq!(tpl.nodes[0]["kind"]["type"], "vector_text");
        assert_eq!(tpl.nodes[0]["kind"]["writing_mode"], "vertical_rl");
        assert!(tpl.param_bindings[0].expression.contains("step(1.000000, t_sec)"));

        let cfg_3d = LyricTimelineTemplateConfig {
            style: LyricNodeStyle::Text3D,
            ..Default::default()
        };
        let tpl_3d = build_lyric_timeline_nodes(&words, "en", &cfg_3d);
        assert_eq!(tpl_3d.nodes[0]["kind"]["type"], "text3_d");
        assert_eq!(tpl_3d.nodes[1]["kind"]["type"], "blend");
    }

    #[test]
    fn detect_ai_audio_capabilities_discovers_isolated_venv_and_script() {
        let caps = detect_ai_audio_capabilities();
        assert!(caps.sidecar_script_found);
        assert!(caps.python_available);
        assert!(caps.venv_audio_found);
        assert!(caps.torch_available);
        assert!(caps.torch_cuda_available);
        assert!(caps.audio_separator_available);
        assert!(caps.faster_whisper_available);
        assert!(caps.stable_whisper_available);
        assert!(caps.fugashi_available);
        assert!(caps.unidic_lite_available);
    }

    #[test]
    fn separation_and_alignment_validate_inputs_before_spawning() {
        let bad_stems = StemSeparationRequest {
            stems: 3,
            ..Default::default()
        };
        assert!(matches!(
            separate_audio_stems(&bad_stems),
            Err(MediaError::InvalidAudioFormat(_))
        ));

        let missing_audio = LyricAlignRequest {
            input_audio: PathBuf::from("nonexistent_vocals_12345.wav"),
            ..Default::default()
        };
        assert!(matches!(
            align_audio_lyrics(&missing_audio),
            Err(MediaError::IoError { .. })
        ));
    }
}
