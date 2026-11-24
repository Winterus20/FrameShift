//! Manifest-driven ONNX AI plugin slot (`frameshift.ai_model.v1`).
//!
//! Discovers `local_media/models/<id>/model_manifest.json` descriptors, enforces
//! pure-Rust SHA-256 integrity verification (`Strict` by default, `allow_unverified` opt-in)
//! plus path-traversal bounds, and runs dynamic ONNX inference via `scripts/ai_infer_sidecar.py --manifest`.

use crate::ai_inference::resolve_ai_sidecar_script;
use crate::error::MediaError;
use crate::ffmpeg::FfmpegBridge;
use crate::frame::{AlphaMode, Rgba8Frame};
use crate::interpolator::find_executable;
use crate::streaming::spawn_stderr_drain;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

/// Current schema version identifier for `model_manifest.json`.
pub const AI_MODEL_MANIFEST_SCHEMA_V1: &str = "frameshift.ai_model.v1";

/// Conventional manifest filename inside each model directory.
pub const AI_MODEL_MANIFEST_FILENAME: &str = "model_manifest.json";

fn default_schema_version() -> String {
    AI_MODEL_MANIFEST_SCHEMA_V1.to_owned()
}

fn default_std() -> [f32; 3] {
    [1.0, 1.0, 1.0]
}

fn default_scale_factor() -> u32 {
    1
}

/// Model task declared by `model_manifest.json`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AiManifestTask {
    Matte,
    Depth,
    Upscale,
    Style,
    Custom(String),
}

impl AiManifestTask {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Matte => "matte",
            Self::Depth => "depth",
            Self::Upscale => "upscale",
            Self::Style => "style",
            Self::Custom(s) => s.as_str(),
        }
    }
}

impl Serialize for AiManifestTask {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for AiManifestTask {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        let normalized = raw.trim().to_ascii_lowercase();
        Ok(match normalized.as_str() {
            "matte" | "rotoscope" => Self::Matte,
            "depth" => Self::Depth,
            "upscale" => Self::Upscale,
            "style" => Self::Style,
            _ => Self::Custom(raw),
        })
    }
}

/// Input/output tensor memory layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiTensorLayout {
    #[default]
    Nchw,
    Nhwc,
}

/// Element data type expected by the ONNX input tensor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiTensorDtype {
    #[default]
    Float32,
    Float16,
    Uint8,
    Int64,
}

/// Input color channel convention expected by the ONNX model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiInputColorSpace {
    #[default]
    Rgb,
    Bgr,
    Rgba,
    Luma,
}

/// Spatial resizing policy applied before ONNX execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiResizeMode {
    #[default]
    Native,
    Fixed,
    AspectSnap,
}

/// Resize policy descriptor inside `model_manifest.json`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AiResizePolicy {
    #[serde(default)]
    pub mode: AiResizeMode,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub multiple_of: Option<u32>,
    #[serde(default)]
    pub max_side: Option<u32>,
    #[serde(default)]
    pub interp: Option<String>,
}

/// Optional tiled inference specification for high-resolution `upscale`/`style` models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiTilingConfig {
    pub tile_size: u32,
    #[serde(default)]
    pub overlap: u32,
}

/// Auxiliary input tensor binding (e.g. prompt points/labels for interactive segmentation).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiAuxInputSpec {
    pub name: String,
    pub source: String,
    #[serde(default)]
    pub dtype: AiTensorDtype,
    #[serde(default)]
    pub shape: Vec<i64>,
    #[serde(default)]
    pub default_value: Option<serde_json::Value>,
}

/// Activation applied to raw ONNX output tensors before RGBA packing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiOutputActivation {
    #[default]
    Auto,
    None,
    Sigmoid,
    Tanh,
    Clamp01,
}

/// Structured model license metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiLicenseInfo {
    pub spdx: String,
    #[serde(default)]
    pub commercial_ok: Option<bool>,
    #[serde(default)]
    pub source_url: Option<String>,
}

/// License field accepting either an SPDX string or structured metadata object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AiModelLicense {
    Spdx(String),
    Detailed(AiLicenseInfo),
}

impl AiModelLicense {
    pub fn spdx_str(&self) -> &str {
        match self {
            Self::Spdx(s) => s.as_str(),
            Self::Detailed(info) => info.spdx.as_str(),
        }
    }
}

/// Declarative ONNX model manifest (`frameshift.ai_model.v1`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiModelManifest {
    #[serde(default = "default_schema_version")]
    pub schema_version: String,
    pub id: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    pub task: AiManifestTask,
    #[serde(alias = "entrypoint")]
    pub onnx_file: String,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub license: Option<AiModelLicense>,
    #[serde(default)]
    pub input_layout: AiTensorLayout,
    #[serde(default)]
    pub input_dtype: AiTensorDtype,
    #[serde(default)]
    pub mean: [f32; 3],
    #[serde(default = "default_std")]
    pub std: [f32; 3],
    #[serde(default)]
    pub color_space: AiInputColorSpace,
    #[serde(default)]
    pub resize: AiResizePolicy,
    #[serde(default = "default_scale_factor")]
    pub scale_factor: u32,
    #[serde(default)]
    pub tiling: Option<AiTilingConfig>,
    #[serde(default)]
    pub aux_inputs: Vec<AiAuxInputSpec>,
    #[serde(default)]
    pub activation: AiOutputActivation,
    #[serde(default)]
    pub output_index: i32,
    #[serde(default)]
    pub output_tensor_name: Option<String>,
    #[serde(default)]
    pub output_channels: Option<u32>,
    #[serde(default)]
    pub temporal_smooth: Option<f32>,
    #[serde(default)]
    pub preferred_provider: Option<String>,
}

/// SHA-256 verification policy when running manifest-backed models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sha256VerificationPolicy {
    /// Require `sha256` to be present and match the `.onnx` file digest.
    #[default]
    Strict,
    /// Allow missing or mismatched SHA-256 (explicit opt-in for local development).
    AllowUnverified,
}

/// Discovered model package under `local_media/models/<id>/`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiDiscoveredModel {
    pub manifest: AiModelManifest,
    pub manifest_path: PathBuf,
    pub model_dir: PathBuf,
    pub onnx_path: PathBuf,
    pub sha256_actual: Option<String>,
    pub sha256_verified: bool,
    pub verification_error: Option<String>,
}

/// Configuration for executing a manifest-backed AI model over a frame sequence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiManifestRunConfig {
    /// Execution provider preference (`"auto"`, `"dml"`, `"cuda"`, `"cpu"`).
    pub provider: String,
    /// Optional temporal EMA override (`0.0..=0.95`).
    pub temporal_smooth: Option<f32>,
    /// Optional auxiliary parameters JSON passed via `--params-json`.
    pub params_json: Option<serde_json::Value>,
    /// Explicit opt-in to run when `sha256` is missing or mismatched (`Strict` when `false`).
    pub allow_unverified: bool,
    /// Optional explicit Python interpreter path.
    pub python_binary: Option<PathBuf>,
    /// Optional explicit path to `scripts/ai_infer_sidecar.py`.
    pub sidecar_script: Option<PathBuf>,
    /// Runs deterministic synthetic output in sidecar without requiring an ONNX session.
    pub synthetic_test: bool,
}

impl Default for AiManifestRunConfig {
    fn default() -> Self {
        Self {
            provider: "auto".to_owned(),
            temporal_smooth: None,
            params_json: None,
            allow_unverified: false,
            python_binary: None,
            sidecar_script: None,
            synthetic_test: false,
        }
    }
}

/// Execution summary returned by [`run_manifest_model`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiManifestRunReport {
    pub model_id: String,
    pub task: AiManifestTask,
    pub frame_count: usize,
    pub input_width: u32,
    pub input_height: u32,
    pub output_width: u32,
    pub output_height: u32,
    pub scale_factor: u32,
    pub sha256_verified: bool,
    pub sha256_actual: Option<String>,
    pub used_synthetic_test: bool,
    pub python_binary: PathBuf,
    pub sidecar_script: PathBuf,
}

/// Validates `manifest` fields and resolves `onnx_file` safely inside `manifest_dir`.
pub fn validate_manifest_and_resolve_onnx(
    manifest: &AiModelManifest,
    manifest_dir: &Path,
) -> Result<PathBuf, MediaError> {
    if manifest.schema_version != AI_MODEL_MANIFEST_SCHEMA_V1 {
        return Err(MediaError::ParseError(format!(
            "unsupported AI manifest schema_version '{}' (expected '{}')",
            manifest.schema_version, AI_MODEL_MANIFEST_SCHEMA_V1
        )));
    }
    let id_trimmed = manifest.id.trim();
    if id_trimmed.is_empty()
        || id_trimmed.contains('/')
        || id_trimmed.contains('\\')
        || id_trimmed.contains("..")
    {
        return Err(MediaError::ParseError(format!(
            "invalid AI manifest id '{}'",
            manifest.id
        )));
    }
    if manifest.scale_factor == 0 {
        return Err(MediaError::ParseError(
            "AI manifest scale_factor must be >= 1".into(),
        ));
    }
    if !manifest.mean.iter().all(|v| v.is_finite()) {
        return Err(MediaError::ParseError(
            "AI manifest mean contains non-finite value".into(),
        ));
    }
    if !manifest.std.iter().all(|v| v.is_finite() && *v > 0.0) {
        return Err(MediaError::ParseError(
            "AI manifest std values must be finite and > 0".into(),
        ));
    }
    if let Some(tiling) = manifest.tiling {
        if tiling.tile_size == 0 {
            return Err(MediaError::ParseError(
                "AI manifest tiling.tile_size must be > 0".into(),
            ));
        }
        if tiling.overlap * 2 >= tiling.tile_size {
            return Err(MediaError::ParseError(
                "AI manifest tiling.overlap must be < tile_size / 2".into(),
            ));
        }
    }

    let onnx_rel = Path::new(&manifest.onnx_file);
    if manifest.onnx_file.trim().is_empty() || onnx_rel.is_absolute() {
        return Err(MediaError::ParseError(format!(
            "AI manifest onnx_file '{}' must be a non-empty relative path",
            manifest.onnx_file
        )));
    }
    for comp in onnx_rel.components() {
        match comp {
            Component::Normal(_) => {}
            _ => {
                return Err(MediaError::ParseError(format!(
                    "AI manifest onnx_file '{}' escapes model directory",
                    manifest.onnx_file
                )));
            }
        }
    }

    let joined = manifest_dir.join(onnx_rel);
    if manifest_dir.exists()
        && joined.exists()
        && let (Ok(canon_dir), Ok(canon_file)) =
            (std::fs::canonicalize(manifest_dir), std::fs::canonicalize(&joined))
        && !canon_file.starts_with(&canon_dir)
    {
        return Err(MediaError::ParseError(format!(
            "AI manifest onnx_file '{}' resolves outside model directory via symlink",
            manifest.onnx_file
        )));
    }

    Ok(joined)
}

/// Loads and validates a `model_manifest.json` file, returning `(manifest, resolved_onnx_path)`.
pub fn load_ai_model_manifest(manifest_path: &Path) -> Result<(AiModelManifest, PathBuf), MediaError> {
    let raw = std::fs::read_to_string(manifest_path)?;
    let manifest: AiModelManifest = serde_json::from_str(&raw).map_err(|e| {
        MediaError::ParseError(format!(
            "invalid AI model manifest '{}': {e}",
            manifest_path.display()
        ))
    })?;
    let manifest_dir = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    let onnx_path = validate_manifest_and_resolve_onnx(&manifest, manifest_dir)?;
    Ok((manifest, onnx_path))
}

/// Verifies `onnx_path` against `manifest.sha256` using pure-Rust SHA-256.
///
/// In `Strict` mode (`allow_unverified == false`), missing or mismatched digests return an error.
pub fn verify_manifest_sha256(
    manifest: &AiModelManifest,
    onnx_path: &Path,
    allow_unverified: bool,
) -> Result<(bool, Option<String>), MediaError> {
    if !onnx_path.is_file() {
        return Err(MediaError::IoError {
            message: format!("ONNX model file '{}' not found", onnx_path.display()),
        });
    }
    let identity = FfmpegBridge::hash_file(onnx_path)?;
    let actual = identity.sha256;

    let Some(expected_raw) = manifest.sha256.as_deref().map(str::trim).filter(|s| !s.is_empty()) else {
        if allow_unverified {
            return Ok((false, Some(actual)));
        }
        return Err(MediaError::InvalidEncodingSettings(format!(
            "AI model '{}' is missing sha256 in manifest (Strict verification; pass allow_unverified=true to override)",
            manifest.id
        )));
    };

    let expected_lower = expected_raw.to_ascii_lowercase();
    if actual == expected_lower {
        Ok((true, Some(actual)))
    } else if allow_unverified {
        Ok((false, Some(actual)))
    } else {
        Err(MediaError::InvalidEncodingSettings(format!(
            "SHA-256 mismatch for AI model '{}': expected {}, got {}",
            manifest.id, expected_lower, actual
        )))
    }
}

/// Discovers all `model_manifest.json` packages inside subdirectories of `models_dir`.
pub fn discover_ai_models(models_dir: &Path) -> Result<Vec<AiDiscoveredModel>, MediaError> {
    if !models_dir.exists() {
        return Ok(Vec::new());
    }
    if !models_dir.is_dir() {
        return Err(MediaError::IoError {
            message: format!("models directory '{}' is not a directory", models_dir.display()),
        });
    }

    let mut subdirs = Vec::new();
    for entry in std::fs::read_dir(models_dir)? {
        let entry = entry?;
        let p = entry.path();
        if p.is_dir() {
            subdirs.push(p);
        }
    }
    subdirs.sort();

    let mut discovered = Vec::new();
    for dir in subdirs {
        let manifest_path = dir.join(AI_MODEL_MANIFEST_FILENAME);
        if !manifest_path.is_file() {
            continue;
        }
        let (manifest, onnx_path) = load_ai_model_manifest(&manifest_path)?;
        let (sha256_verified, sha256_actual, verification_error) = if onnx_path.is_file() {
            match verify_manifest_sha256(&manifest, &onnx_path, false) {
                Ok((verified, actual)) => (verified, actual, None),
                Err(err) => {
                    let actual = FfmpegBridge::hash_file(&onnx_path).ok().map(|i| i.sha256);
                    (false, actual, Some(err.to_string()))
                }
            }
        } else {
            (
                false,
                None,
                Some(format!("ONNX file '{}' not found", onnx_path.display())),
            )
        };

        discovered.push(AiDiscoveredModel {
            manifest,
            manifest_path,
            model_dir: dir,
            onnx_path,
            sha256_actual,
            sha256_verified,
            verification_error,
        });
    }

    discovered.sort_by(|a, b| a.manifest.id.cmp(&b.manifest.id));
    Ok(discovered)
}

/// Runs a manifest-declared ONNX model from `models_dir/<model_id>/model_manifest.json`.
pub fn run_manifest_model_by_id(
    frames: &[Rgba8Frame],
    models_dir: &Path,
    model_id: &str,
    config: &AiManifestRunConfig,
) -> Result<(Vec<Rgba8Frame>, AiManifestRunReport), MediaError> {
    let models = discover_ai_models(models_dir)?;
    let found = models
        .into_iter()
        .find(|m| m.manifest.id == model_id)
        .ok_or_else(|| MediaError::IoError {
            message: format!(
                "AI model manifest with id '{model_id}' not found under '{}'",
                models_dir.display()
            ),
        })?;
    run_manifest_model(frames, &found.manifest_path, config)
}

/// Runs a manifest-declared ONNX model over `frames` via `scripts/ai_infer_sidecar.py --manifest`.
pub fn run_manifest_model(
    frames: &[Rgba8Frame],
    manifest_path: &Path,
    config: &AiManifestRunConfig,
) -> Result<(Vec<Rgba8Frame>, AiManifestRunReport), MediaError> {
    let (manifest, onnx_path) = load_ai_model_manifest(manifest_path)?;

    let (sha256_verified, sha256_actual) = if config.synthetic_test && !onnx_path.is_file() {
        if config.allow_unverified {
            (false, None)
        } else {
            return Err(MediaError::IoError {
                message: format!(
                    "ONNX model file '{}' not found for Strict verification",
                    onnx_path.display()
                ),
            });
        }
    } else {
        verify_manifest_sha256(&manifest, &onnx_path, config.allow_unverified)?
    };

    let python_exe = find_executable(config.python_binary.as_deref(), &["python", "python3"])
        .ok_or_else(|| MediaError::BinarySpawnFailed {
            binary: "python".into(),
            reason: "python interpreter not found in PATH".into(),
        })?;
    let script_path =
        resolve_ai_sidecar_script(config.sidecar_script.as_deref()).ok_or_else(|| {
            MediaError::IoError {
                message: "could not locate scripts/ai_infer_sidecar.py".into(),
            }
        })?;

    let scale_factor = manifest.scale_factor.max(1);
    if frames.is_empty() {
        return Ok((
            Vec::new(),
            AiManifestRunReport {
                model_id: manifest.id,
                task: manifest.task,
                frame_count: 0,
                input_width: 0,
                input_height: 0,
                output_width: 0,
                output_height: 0,
                scale_factor,
                sha256_verified,
                sha256_actual,
                used_synthetic_test: config.synthetic_test,
                python_binary: python_exe,
                sidecar_script: script_path,
            },
        ));
    }

    let in_w = frames[0].width;
    let in_h = frames[0].height;
    if in_w == 0 || in_h == 0 {
        return Err(MediaError::InvalidDimensions {
            width: in_w,
            height: in_h,
        });
    }
    for f in frames {
        if f.width != in_w || f.height != in_h {
            return Err(MediaError::InvalidDimensions {
                width: f.width,
                height: f.height,
            });
        }
    }

    let out_w = in_w
        .checked_mul(scale_factor)
        .ok_or(MediaError::InvalidDimensions {
            width: in_w,
            height: in_h,
        })?;
    let out_h = in_h
        .checked_mul(scale_factor)
        .ok_or(MediaError::InvalidDimensions {
            width: in_w,
            height: in_h,
        })?;

    let effective_provider = if config.provider != "auto" {
        config.provider.as_str()
    } else {
        manifest.preferred_provider.as_deref().unwrap_or("auto")
    };
    let smooth = config
        .temporal_smooth
        .or(manifest.temporal_smooth)
        .unwrap_or(0.0)
        .clamp(0.0, 0.95);

    let mut cmd = Command::new(&python_exe);
    cmd.arg(&script_path)
        .arg("--manifest")
        .arg(manifest_path)
        .args(["--width", &in_w.to_string()])
        .args(["--height", &in_h.to_string()])
        .args(["--frames", &frames.len().to_string()])
        .args(["--temporal-smooth", &format!("{smooth}")])
        .args(["--provider", effective_provider]);

    if let Some(params) = &config.params_json {
        cmd.args(["--params-json", &params.to_string()]);
    }
    if config.synthetic_test {
        cmd.arg("--synthetic-test");
    }

    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| MediaError::BinarySpawnFailed {
        binary: python_exe.to_string_lossy().into_owned(),
        reason: e.to_string(),
    })?;

    let mut stdin = child.stdin.take().ok_or_else(|| MediaError::FfmpegFailed {
        exit_code: None,
        stderr: "failed to acquire python sidecar stdin pipe".into(),
    })?;
    let mut stdout = child.stdout.take().ok_or_else(|| MediaError::FfmpegFailed {
        exit_code: None,
        stderr: "failed to acquire python sidecar stdout pipe".into(),
    })?;
    let stderr_drain = spawn_stderr_drain(child.stderr.take());

    let payload: Vec<u8> = frames.iter().flat_map(|f| f.data.iter().copied()).collect();
    let writer_handle = std::thread::spawn(move || -> Result<(), std::io::Error> {
        stdin.write_all(&payload)?;
        stdin.flush()?;
        Ok(())
    });

    let out_frame_bytes = (out_w as usize)
        .checked_mul(out_h as usize)
        .and_then(|v| v.checked_mul(4))
        .ok_or(MediaError::InvalidDimensions {
            width: out_w,
            height: out_h,
        })?;
    let expected_total = out_frame_bytes
        .checked_mul(frames.len())
        .ok_or(MediaError::InvalidDimensions {
            width: out_w,
            height: out_h,
        })?;

    let mut raw_out = Vec::with_capacity(expected_total);
    stdout.read_to_end(&mut raw_out)?;

    let write_res = writer_handle.join().map_err(|_| MediaError::IoError {
        message: "python sidecar stdin writer thread panicked".into(),
    })?;
    let status = child.wait().map_err(|e| MediaError::IoError {
        message: format!("failed waiting on python sidecar: {e}"),
    })?;
    let stderr = stderr_drain
        .and_then(|h| h.join().ok())
        .unwrap_or_default();

    if !status.success() {
        return Err(MediaError::FfmpegFailed {
            exit_code: status.code(),
            stderr,
        });
    }
    write_res?;

    if raw_out.len() != expected_total {
        return Err(MediaError::BufferLengthMismatch {
            expected: expected_total,
            actual: raw_out.len(),
        });
    }

    let mut out_frames = Vec::with_capacity(frames.len());
    for (idx, src) in frames.iter().enumerate() {
        let start = idx * out_frame_bytes;
        let end = start + out_frame_bytes;
        out_frames.push(Rgba8Frame::new(
            out_w,
            out_h,
            src.pts,
            AlphaMode::Straight,
            raw_out[start..end].to_vec(),
        )?);
    }

    Ok((
        out_frames,
        AiManifestRunReport {
            model_id: manifest.id,
            task: manifest.task,
            frame_count: frames.len(),
            input_width: in_w,
            input_height: in_h,
            output_width: out_w,
            output_height: out_h,
            scale_factor,
            sha256_verified,
            sha256_actual,
            used_synthetic_test: config.synthetic_test,
            python_binary: python_exe,
            sidecar_script: script_path,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_inference::synthetic_ai_test_frames;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TempDirGuard(PathBuf);
    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn make_temp_dir(tag: &str) -> PathBuf {
        let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "frameshift_ai_manifest_{tag}_{}_{}",
            std::process::id(),
            id
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn manifest_discovery_strict_sha256_and_path_traversal_checks() {
        let root = make_temp_dir("discover");
        let _guard = TempDirGuard(root.clone());

        let model_dir = root.join("upscale_2x");
        std::fs::create_dir_all(&model_dir).unwrap();
        let onnx_path = model_dir.join("dummy.onnx");
        std::fs::write(&onnx_path, b"dummy-onnx-weights-v1").unwrap();
        let actual_sha = FfmpegBridge::hash_file(&onnx_path).unwrap().sha256;

        let manifest_json = serde_json::json!({
            "schema_version": "frameshift.ai_model.v1",
            "id": "upscale_2x",
            "task": "upscale",
            "onnx_file": "dummy.onnx",
            "sha256": actual_sha,
            "license": "MIT",
            "input_layout": "nchw",
            "input_dtype": "float32",
            "mean": [0.0, 0.0, 0.0],
            "std": [1.0, 1.0, 1.0],
            "color_space": "rgb",
            "resize": { "mode": "native", "multiple_of": 8 },
            "scale_factor": 2,
            "tiling": { "tile_size": 256, "overlap": 16 },
            "aux_inputs": []
        });
        let manifest_path = model_dir.join("model_manifest.json");
        std::fs::write(&manifest_path, serde_json::to_string_pretty(&manifest_json).unwrap()).unwrap();

        let models = discover_ai_models(&root).unwrap();
        assert_eq!(models.len(), 1);
        assert!(models[0].sha256_verified);
        assert_eq!(models[0].manifest.scale_factor, 2);
        assert_eq!(models[0].manifest.task, AiManifestTask::Upscale);

        let mut bad_manifest = models[0].manifest.clone();
        bad_manifest.onnx_file = "../outside.onnx".into();
        assert!(validate_manifest_and_resolve_onnx(&bad_manifest, &model_dir).is_err());

        bad_manifest.onnx_file = "dummy.onnx".into();
        bad_manifest.sha256 = Some("0000".repeat(16));
        assert!(verify_manifest_sha256(&bad_manifest, &onnx_path, false).is_err());
        let (verified, actual) = verify_manifest_sha256(&bad_manifest, &onnx_path, true).unwrap();
        assert!(!verified);
        assert_eq!(actual, Some(actual_sha));
    }

    #[test]
    fn manifest_sidecar_synthetic_roundtrip_respects_scale_factor_and_strict_hash() {
        let root = make_temp_dir("run");
        let _guard = TempDirGuard(root.clone());

        let model_dir = root.join("anime_upscale_2x");
        std::fs::create_dir_all(&model_dir).unwrap();
        let onnx_path = model_dir.join("model.onnx");
        std::fs::write(&onnx_path, b"onnx-placeholder").unwrap();
        let actual_sha = FfmpegBridge::hash_file(&onnx_path).unwrap().sha256;

        let manifest_path = model_dir.join("model_manifest.json");
        let manifest_json = serde_json::json!({
            "schema_version": "frameshift.ai_model.v1",
            "id": "anime_upscale_2x",
            "task": "upscale",
            "onnx_file": "model.onnx",
            "sha256": actual_sha,
            "license": { "spdx": "Apache-2.0", "commercial_ok": true },
            "input_layout": "nchw",
            "input_dtype": "float32",
            "mean": [0.0, 0.0, 0.0],
            "std": [1.0, 1.0, 1.0],
            "color_space": "rgb",
            "resize": { "mode": "native" },
            "scale_factor": 2,
            "tiling": { "tile_size": 32, "overlap": 4 },
            "aux_inputs": []
        });
        std::fs::write(&manifest_path, serde_json::to_string_pretty(&manifest_json).unwrap()).unwrap();

        let frames = synthetic_ai_test_frames(32, 24, 2);
        let cfg = AiManifestRunConfig {
            synthetic_test: true,
            ..Default::default()
        };
        let (out, report) = run_manifest_model(&frames, &manifest_path, &cfg).unwrap();
        assert!(report.sha256_verified);
        assert_eq!(report.scale_factor, 2);
        assert_eq!(report.output_width, 64);
        assert_eq!(report.output_height, 48);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].width, 64);
        assert_eq!(out[0].height, 48);

        let mut corrupt = manifest_json.clone();
        corrupt["sha256"] = serde_json::json!("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff");
        std::fs::write(&manifest_path, serde_json::to_string_pretty(&corrupt).unwrap()).unwrap();
        assert!(run_manifest_model(&frames, &manifest_path, &cfg).is_err());

        let opt_in_cfg = AiManifestRunConfig {
            allow_unverified: true,
            synthetic_test: true,
            ..Default::default()
        };
        let (out2, report2) = run_manifest_model(&frames, &manifest_path, &opt_in_cfg).unwrap();
        assert!(!report2.sha256_verified);
        assert_eq!(out2.len(), 2);
    }
}
