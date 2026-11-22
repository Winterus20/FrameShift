//! Project media inventory, missing-file checks, and conservative relinking.
//!
//! Manifests are separate from project JSON so older project files remain
//! readable. Paths are interpreted relative to the project file when they are
//! relative. Optional SHA-256 values are supplied by a higher layer so this
//! crate does not acquire a media/hash dependency.

use crate::project::{AssetKind, Project};
use crate::version::SchemaVersion;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

/// Current sidecar project media manifest format.
pub const PROJECT_MEDIA_MANIFEST_VERSION: u32 = 1;

/// Portable inventory for media assets referenced by one project revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectMediaManifest {
    pub manifest_version: u32,
    pub schema_version: SchemaVersion,
    pub revision_id: u64,
    pub project_name: String,
    /// Project path as supplied by the authoring command, for user context only.
    pub project_path: String,
    #[serde(default)]
    pub assets: Vec<ProjectMediaManifestEntry>,
}

/// One registered media asset and its portable path metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectMediaManifestEntry {
    pub asset_id: String,
    pub kind: AssetKind,
    /// Path stored in `AssetDef` at manifest creation time.
    pub stored_path: String,
    /// Relative to the project directory when the source path is portable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_relative_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_size_bytes: Option<u64>,
    /// Optional lowercase SHA-256 supplied by a higher layer with a file hash API.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

/// Result of checking referenced media against a project directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectMediaStatus {
    pub asset_id: String,
    pub stored_path: String,
    pub resolved_path: PathBuf,
    pub exists: bool,
}

/// A file successfully matched and assigned to a missing media asset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelinkedMediaAsset {
    pub asset_id: String,
    pub previous_path: String,
    pub new_path: String,
}

/// A basename matched multiple files, so no path was changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AmbiguousMediaRelink {
    pub asset_id: String,
    pub candidates: Vec<PathBuf>,
}

/// Summary of a non-destructive relink pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MediaRelinkReport {
    pub relinked: Vec<RelinkedMediaAsset>,
    pub missing_asset_ids: Vec<String>,
    pub ambiguous: Vec<AmbiguousMediaRelink>,
    pub scan_errors: Vec<String>,
}

impl ProjectMediaManifest {
    /// Builds a portable media inventory beside the project file.
    pub fn from_project(project: &Project, project_path: impl AsRef<Path>) -> Self {
        Self::from_project_with_hash(project, project_path, |_| None)
    }

    /// Builds an inventory and asks a caller-provided file hasher for SHA-256.
    ///
    /// This keeps filesystem hashing out of the schema crate while allowing
    /// the CLI or media layer to record strong content matches.
    pub fn from_project_with_hash(
        project: &Project,
        project_path: impl AsRef<Path>,
        mut hash_file: impl FnMut(&Path) -> Option<String>,
    ) -> Self {
        let project_path = project_path.as_ref();
        let project_dir = project_path.parent().unwrap_or_else(|| Path::new("."));
        let assets = project
            .assets
            .iter()
            .map(|asset| {
                let absolute_path = resolve_asset_path(project_dir, &asset.path);
                let file_size_bytes = fs::metadata(&absolute_path)
                    .ok()
                    .filter(|metadata| metadata.is_file())
                    .map(|metadata| metadata.len());
                ProjectMediaManifestEntry {
                    asset_id: asset.id.clone(),
                    kind: asset.kind,
                    stored_path: asset.path.clone(),
                    project_relative_path: portable_relative_path(project_dir, &asset.path),
                    file_size_bytes,
                    sha256: file_size_bytes.and_then(|_| hash_file(&absolute_path)),
                }
            })
            .collect();
        Self {
            manifest_version: PROJECT_MEDIA_MANIFEST_VERSION,
            schema_version: project.schema_version,
            revision_id: project.revision_id,
            project_name: project.name.clone(),
            project_path: project_path.to_string_lossy().into_owned(),
            assets,
        }
    }

    /// Serializes the sidecar manifest to formatted JSON.
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Deserializes the manifest and verifies its format and unique asset ids.
    pub fn from_json_str(json: &str) -> Result<Self, String> {
        let manifest: Self = serde_json::from_str(json).map_err(|error| error.to_string())?;
        if manifest.manifest_version != PROJECT_MEDIA_MANIFEST_VERSION {
            return Err(format!(
                "manifest_version {} is unsupported (expected {})",
                manifest.manifest_version, PROJECT_MEDIA_MANIFEST_VERSION
            ));
        }
        let mut ids = std::collections::BTreeSet::new();
        for asset in &manifest.assets {
            if asset.asset_id.trim().is_empty() || !ids.insert(asset.asset_id.as_str()) {
                return Err(format!(
                    "manifest contains an empty or duplicate asset id '{}'",
                    asset.asset_id
                ));
            }
            if let Some(path) = &asset.project_relative_path
                && (Path::new(path).is_absolute()
                    || Path::new(path).components().any(|component| {
                        matches!(
                            component,
                            Component::ParentDir | Component::RootDir | Component::Prefix(_)
                        )
                    }))
            {
                return Err(format!(
                    "manifest relative path for asset '{}' is absolute or escapes the project directory",
                    asset.asset_id
                ));
            }
            if asset.sha256.as_deref().is_some_and(|sha| {
                sha.len() != 64 || !sha.bytes().all(|byte| byte.is_ascii_hexdigit())
            }) {
                return Err(format!(
                    "manifest SHA-256 for asset '{}' must be 64 hexadecimal characters",
                    asset.asset_id
                ));
            }
        }
        Ok(manifest)
    }
}

/// Resolves all project asset paths and marks files that are missing.
pub fn inspect_project_media(
    project: &Project,
    project_dir: impl AsRef<Path>,
) -> Vec<ProjectMediaStatus> {
    let project_dir = project_dir.as_ref();
    project
        .assets
        .iter()
        .map(|asset| {
            let resolved_path = resolve_asset_path(project_dir, &asset.path);
            let exists = resolved_path.is_file();
            ProjectMediaStatus {
                asset_id: asset.id.clone(),
                stored_path: asset.path.clone(),
                resolved_path,
                exists,
            }
        })
        .collect()
}

/// Reports missing registered assets without changing the project.
pub fn missing_project_media(
    project: &Project,
    project_dir: impl AsRef<Path>,
) -> Vec<ProjectMediaStatus> {
    inspect_project_media(project, project_dir)
        .into_iter()
        .filter(|status| !status.exists)
        .collect()
}

/// Relinks missing assets by basename, narrowing candidates with a manifest fingerprint.
///
/// Only unique matches are assigned. Ambiguous or absent candidates stay
/// missing and are reported to the caller. `search_roots` are walked without
/// following symlinks; there is no network lookup or implicit project write.
pub fn relink_missing_media(
    project: &mut Project,
    project_dir: impl AsRef<Path>,
    search_roots: &[PathBuf],
    manifest: Option<&ProjectMediaManifest>,
) -> MediaRelinkReport {
    relink_missing_media_with_hash(project, project_dir, search_roots, manifest, |_| None)
}

/// Relinks missing assets with a caller-provided SHA-256 function.
///
/// If an entry has a SHA-256 value, candidates must match it. Entries without
/// a hash still require a unique basename and matching byte size when present.
pub fn relink_missing_media_with_hash(
    project: &mut Project,
    project_dir: impl AsRef<Path>,
    search_roots: &[PathBuf],
    manifest: Option<&ProjectMediaManifest>,
    mut hash_file: impl FnMut(&Path) -> Option<String>,
) -> MediaRelinkReport {
    let project_dir = project_dir.as_ref();
    let mut report = MediaRelinkReport::default();
    let mut files = Vec::new();
    for root in search_roots {
        collect_files(root, &mut files, &mut report.scan_errors);
    }
    files.sort();
    files.dedup();

    let manifest_entries: BTreeMap<&str, &ProjectMediaManifestEntry> = manifest
        .into_iter()
        .flat_map(|manifest| manifest.assets.iter())
        .map(|entry| (entry.asset_id.as_str(), entry))
        .collect();

    for asset in &mut project.assets {
        let old_resolved = resolve_asset_path(project_dir, &asset.path);
        if old_resolved.is_file() {
            continue;
        }
        let Some(basename) = Path::new(&asset.path).file_name() else {
            report.missing_asset_ids.push(asset.id.clone());
            continue;
        };
        let mut candidates: Vec<PathBuf> = files
            .iter()
            .filter(|candidate| candidate.file_name() == Some(basename))
            .cloned()
            .collect();
        if let Some(entry) = manifest_entries.get(asset.id.as_str()) {
            let expected_size = entry.file_size_bytes;
            let expected_hash = entry.sha256.as_deref();
            candidates.retain(|candidate| {
                let Ok(metadata) = fs::metadata(candidate) else {
                    return false;
                };
                if expected_size.is_some_and(|size| metadata.len() != size) {
                    return false;
                }
                expected_hash.is_none_or(|expected| {
                    hash_file(candidate).is_some_and(|actual| actual == expected)
                })
            });
        }
        match candidates.len() {
            0 => report.missing_asset_ids.push(asset.id.clone()),
            1 => {
                let new_path = candidates.remove(0);
                let previous_path = asset.path.clone();
                asset.path = new_path.to_string_lossy().into_owned();
                report.relinked.push(RelinkedMediaAsset {
                    asset_id: asset.id.clone(),
                    previous_path,
                    new_path: asset.path.clone(),
                });
            }
            _ => report.ambiguous.push(AmbiguousMediaRelink {
                asset_id: asset.id.clone(),
                candidates,
            }),
        }
    }
    report
}

fn resolve_asset_path(project_dir: &Path, path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        project_dir.join(path)
    }
}

fn portable_relative_path(project_dir: &Path, asset_path: &str) -> Option<String> {
    let path = Path::new(asset_path);
    let relative = if path.is_absolute() {
        path.strip_prefix(project_dir).ok()?
    } else {
        path
    };
    if relative.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return None;
    }
    Some(relative.to_string_lossy().replace('\\', "/"))
}

fn collect_files(root: &Path, files: &mut Vec<PathBuf>, errors: &mut Vec<String>) {
    let metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) => {
            errors.push(format!("{}: {error}", root.display()));
            return;
        }
    };
    if metadata.file_type().is_symlink() {
        return;
    }
    if metadata.is_file() {
        files.push(root.to_path_buf());
        return;
    }
    if !metadata.is_dir() {
        return;
    }
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) => {
            errors.push(format!("{}: {error}", root.display()));
            return;
        }
    };
    let mut paths = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) => paths.push(entry.path()),
            Err(error) => errors.push(format!("{}: {error}", root.display())),
        }
    }
    paths.sort();
    for path in paths {
        collect_files(&path, files, errors);
    }
}
