//! Local, reviewed template packages with typed and validated input slots.
//!
//! A template contains an ordinary FrameShift project. The project remains the
//! authority for rendering; package metadata describes review, dependencies,
//! and the limited values that a template consumer may replace. This module
//! does not execute package code or fetch remote packages. Runtime `CustomWgsl`
//! nodes are rejected, including when nested inside precomps.

use crate::error::SchemaError;
use crate::graph::{NodeId, NodeKind, RenderGraphDef};
use crate::project::{AssetKind, ParamBindingDef, Project};
use frameshift_time::Flicks;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path};

/// Current local template package format.
pub const TEMPLATE_PACKAGE_VERSION: u32 = 1;

/// A reviewed, local package containing a renderable project and typed slots.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TemplatePackage {
    pub package_version: u32,
    pub template_id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub project: Project,
    #[serde(default)]
    pub slots: Vec<TemplateSlot>,
    #[serde(default)]
    pub manifest: TemplatePackageManifest,
    #[serde(default)]
    pub review: TemplateReview,
}

/// Declares the files that accompany a template and their redistribution terms.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TemplatePackageManifest {
    #[serde(default)]
    pub files: Vec<TemplatePackageFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style_card_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_video_path: Option<String>,
}

/// An inventoried template dependency. The path is relative to the package root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplatePackageFile {
    pub relative_path: String,
    pub role: TemplateFileRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default)]
    pub redistribution: RedistributionStatus,
}

/// What a packaged file is used for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateFileRole {
    Project,
    Media,
    Font,
    StyleCard,
    SampleVideo,
    Other,
}

/// Whether an inventoried file can be included when sharing the package.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RedistributionStatus {
    #[default]
    NotBundled,
    Redistributable,
    ReferenceOnly,
}

/// Review information for packages authored locally.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TemplateReview {
    #[serde(default)]
    pub reviewed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Input type a template consumer is allowed to replace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "asset_kind", rename_all = "snake_case")]
pub enum TemplateSlotKind {
    AssetPath(AssetKind),
    Text,
    Tick,
    Number,
    Boolean,
}

/// A typed value supplied while instantiating a template.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum TemplateSlotValue {
    AssetPath(String),
    Text(String),
    Tick(Flicks),
    Number(f64),
    Boolean(bool),
}

/// Supported, explicit destinations for template inputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "target", rename_all = "snake_case")]
pub enum TemplateSlotTarget {
    AssetPath { asset_id: String },
    NodeText { node_id: NodeId },
    EventTargetTick { event_id: String },
    NodeEnabled { node_id: NodeId },
    ParameterBinding { node_id: NodeId, param_name: String },
}

/// One named field that may be replaced when opening a template.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TemplateSlot {
    pub id: String,
    pub label: String,
    pub kind: TemplateSlotKind,
    pub target: TemplateSlotTarget,
    #[serde(default = "default_true")]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<TemplateSlotValue>,
}

fn default_true() -> bool {
    true
}

impl TemplatePackage {
    /// Loads JSON from a local path and validates package and project structure.
    pub fn load_local(path: impl AsRef<Path>) -> Result<Self, SchemaError> {
        let json = fs::read_to_string(path).map_err(|error| SchemaError::InvalidTemplate {
            template_id: String::new(),
            message: format!("could not read local package: {error}"),
        })?;
        let package: Self = serde_json::from_str(&json)?;
        package.validate()?;
        Ok(package)
    }

    /// Serializes the package for local review or storage.
    pub fn to_json_pretty(&self) -> Result<String, SchemaError> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Checks package metadata, its project, slots, and inventoried paths.
    pub fn validate(&self) -> Result<(), SchemaError> {
        let fail = |message: String| SchemaError::InvalidTemplate {
            template_id: self.template_id.clone(),
            message,
        };
        if self.package_version != TEMPLATE_PACKAGE_VERSION {
            return Err(fail(format!(
                "package_version {} is unsupported (expected {})",
                self.package_version, TEMPLATE_PACKAGE_VERSION
            )));
        }
        if self.template_id.trim().is_empty() || self.name.trim().is_empty() {
            return Err(fail("template_id and name must be non-empty".into()));
        }
        self.project.validate()?;
        if let Some(node_path) = custom_wgsl_node_path(&self.project.graph, "root") {
            return Err(fail(format!(
                "CustomWgsl node '{node_path}' is not allowed in template packages; packages may not contain executable shader code"
            )));
        }

        let mut slot_ids = BTreeSet::new();
        let mut slot_targets = BTreeSet::new();
        for slot in &self.slots {
            if slot.id.trim().is_empty() || slot.label.trim().is_empty() {
                return Err(fail("slot ids and labels must be non-empty".into()));
            }
            if !slot_ids.insert(slot.id.as_str()) {
                return Err(fail(format!("duplicate slot id '{}'", slot.id)));
            }
            if !slot_targets.insert(format!("{:?}", slot.target)) {
                return Err(fail(format!(
                    "multiple slots target the same field ({:?})",
                    slot.target
                )));
            }
            if let Some(value) = &slot.default {
                validate_slot_value_kind(&slot.id, slot.kind, value)?;
            }
            if slot.required && slot.default.is_none() {
                // This is valid: callers provide the value at instantiation.
            }
            self.validate_slot_target(slot)?;
        }

        let mut file_paths = BTreeSet::new();
        for file in &self.manifest.files {
            validate_package_relative_path(&file.relative_path).map_err(&fail)?;
            if !file_paths.insert(file.relative_path.as_str()) {
                return Err(fail(format!(
                    "duplicate manifest file path '{}'",
                    file.relative_path
                )));
            }
            if file.license.as_deref().is_some_and(str::is_empty) {
                return Err(fail(format!(
                    "license for '{}' must be omitted or non-empty",
                    file.relative_path
                )));
            }
        }
        for path in [
            self.manifest.style_card_path.as_deref(),
            self.manifest.sample_video_path.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            validate_package_relative_path(path).map_err(&fail)?;
        }
        if self.review.reviewed
            && self
                .review
                .reviewer
                .as_deref()
                .is_none_or(|reviewer| reviewer.trim().is_empty())
        {
            return Err(fail(
                "reviewed packages must include a non-empty reviewer".into(),
            ));
        }
        Ok(())
    }

    /// Applies values to an independent project copy and validates the result.
    ///
    /// Instantiation is restricted to packages carrying explicit local review
    /// metadata. The returned `Project` owns all of its data; later slot edits
    /// to another instance cannot affect it or this package.
    pub fn instantiate(
        &self,
        values: &BTreeMap<String, TemplateSlotValue>,
    ) -> Result<Project, SchemaError> {
        self.validate()?;
        if !self.review.reviewed {
            return Err(SchemaError::UnreviewedTemplate(self.template_id.clone()));
        }

        let slots_by_id: BTreeMap<&str, &TemplateSlot> = self
            .slots
            .iter()
            .map(|slot| (slot.id.as_str(), slot))
            .collect();
        for supplied in values.keys() {
            if !slots_by_id.contains_key(supplied.as_str()) {
                return Err(SchemaError::UnknownTemplateSlot(supplied.clone()));
            }
        }

        let mut project = self.project.clone();
        for slot in &self.slots {
            let value = values.get(&slot.id).or(slot.default.as_ref());
            let Some(value) = value else {
                if slot.required {
                    return Err(SchemaError::MissingTemplateSlot(slot.id.clone()));
                }
                continue;
            };
            validate_slot_value_kind(&slot.id, slot.kind, value)?;
            apply_slot_value(&mut project, slot, value)?;
        }
        project.validate()?;
        Ok(project)
    }

    fn validate_slot_target(&self, slot: &TemplateSlot) -> Result<(), SchemaError> {
        let fail = |message: String| SchemaError::InvalidTemplateSlot {
            slot_id: slot.id.clone(),
            message,
        };
        match (&slot.kind, &slot.target) {
            (TemplateSlotKind::AssetPath(expected), TemplateSlotTarget::AssetPath { asset_id }) => {
                let asset = self
                    .project
                    .asset(asset_id)
                    .ok_or_else(|| fail(format!("asset '{asset_id}' does not exist")))?;
                if asset.kind != *expected {
                    return Err(fail(format!(
                        "asset '{asset_id}' is {:?}, slot expects {:?}",
                        asset.kind, expected
                    )));
                }
            }
            (TemplateSlotKind::Text, TemplateSlotTarget::NodeText { node_id }) => {
                match self.project.graph.find_node_recursive(node_id) {
                    Some(node)
                        if matches!(
                            &node.kind,
                            NodeKind::VectorText { .. } | NodeKind::Text3D { .. }
                        ) => {}
                    Some(_) => return Err(fail(format!("node '{node_id}' is not a text node"))),
                    None => return Err(fail(format!("node '{node_id}' does not exist"))),
                }
            }
            (TemplateSlotKind::Tick, TemplateSlotTarget::EventTargetTick { event_id }) => {
                if self.project.event(event_id).is_none() {
                    return Err(fail(format!("event '{event_id}' does not exist")));
                }
            }
            (TemplateSlotKind::Boolean, TemplateSlotTarget::NodeEnabled { node_id }) => {
                if self.project.graph.find_node_recursive(node_id).is_none() {
                    return Err(fail(format!("node '{node_id}' does not exist")));
                }
            }
            (
                TemplateSlotKind::Number,
                TemplateSlotTarget::ParameterBinding {
                    node_id,
                    param_name,
                },
            ) => {
                if self.project.graph.find_node_recursive(node_id).is_none() {
                    return Err(fail(format!("node '{node_id}' does not exist")));
                }
                if param_name.trim().is_empty() {
                    return Err(fail("parameter name must be non-empty".into()));
                }
                let mut candidate = self.project.clone();
                candidate.param_bindings.push(ParamBindingDef {
                    node_id: node_id.clone(),
                    param_name: param_name.clone(),
                    expression: "0".into(),
                });
                if let Err(error) = candidate.validate() {
                    return Err(fail(format!("parameter target is not bindable: {error}")));
                }
            }
            _ => {
                return Err(fail(format!(
                    "slot kind {:?} does not match target {:?}",
                    slot.kind, slot.target
                )));
            }
        }
        Ok(())
    }
}

/// Returns the first runtime custom shader node in a graph, including nested precomps.
/// Template packages are data-only and may use built-in effects, but may not carry
/// user-authored WGSL that is compiled and executed by the renderer.
fn custom_wgsl_node_path(graph: &RenderGraphDef, parent_path: &str) -> Option<String> {
    for node in &graph.nodes {
        let node_path = format!("{parent_path}/{}", node.id);
        if matches!(node.kind, NodeKind::CustomWgsl { .. }) {
            return Some(node_path);
        }
        if let NodeKind::Precomp { sub_graph, .. } = &node.kind
            && let Some(found) = custom_wgsl_node_path(sub_graph, &node_path)
        {
            return Some(found);
        }
    }
    None
}

fn validate_slot_value_kind(
    slot_id: &str,
    expected: TemplateSlotKind,
    value: &TemplateSlotValue,
) -> Result<(), SchemaError> {
    let valid = match (expected, value) {
        (TemplateSlotKind::AssetPath(_), TemplateSlotValue::AssetPath(path)) => {
            !path.trim().is_empty()
        }
        (TemplateSlotKind::Text, TemplateSlotValue::Text(_)) => true,
        (TemplateSlotKind::Tick, TemplateSlotValue::Tick(_)) => true,
        (TemplateSlotKind::Number, TemplateSlotValue::Number(number)) => number.is_finite(),
        (TemplateSlotKind::Boolean, TemplateSlotValue::Boolean(_)) => true,
        _ => false,
    };
    if !valid {
        return Err(SchemaError::InvalidTemplateSlot {
            slot_id: slot_id.to_owned(),
            message: format!("expected a value of kind {expected:?}, got {value:?}"),
        });
    }
    if let TemplateSlotValue::AssetPath(path) = value
        && path.trim().is_empty()
    {
        return Err(SchemaError::InvalidTemplateSlot {
            slot_id: slot_id.to_owned(),
            message: "asset path must be non-empty".into(),
        });
    }
    Ok(())
}

fn apply_slot_value(
    project: &mut Project,
    slot: &TemplateSlot,
    value: &TemplateSlotValue,
) -> Result<(), SchemaError> {
    let fail = |message: String| SchemaError::InvalidTemplateSlot {
        slot_id: slot.id.clone(),
        message,
    };
    match (&slot.target, value) {
        (TemplateSlotTarget::AssetPath { asset_id }, TemplateSlotValue::AssetPath(path)) => {
            let asset = project
                .assets
                .iter_mut()
                .find(|asset| asset.id == *asset_id)
                .ok_or_else(|| fail(format!("asset '{asset_id}' no longer exists")))?;
            asset.path = path.clone();
        }
        (TemplateSlotTarget::NodeText { node_id }, TemplateSlotValue::Text(text)) => {
            let node = find_node_mut_recursive(&mut project.graph, node_id)
                .ok_or_else(|| fail(format!("node '{node_id}' no longer exists")))?;
            match &mut node.kind {
                NodeKind::VectorText {
                    text: node_text, ..
                }
                | NodeKind::Text3D {
                    text: node_text, ..
                } => *node_text = text.clone(),
                _ => return Err(fail(format!("node '{node_id}' is not a text node"))),
            }
        }
        (TemplateSlotTarget::EventTargetTick { event_id }, TemplateSlotValue::Tick(tick)) => {
            let event = project
                .events
                .iter_mut()
                .find(|event| event.event_id == *event_id)
                .ok_or_else(|| fail(format!("event '{event_id}' no longer exists")))?;
            event.target_output_tick = *tick;
        }
        (TemplateSlotTarget::NodeEnabled { node_id }, TemplateSlotValue::Boolean(enabled)) => {
            let node = find_node_mut_recursive(&mut project.graph, node_id)
                .ok_or_else(|| fail(format!("node '{node_id}' no longer exists")))?;
            node.enabled = *enabled;
        }
        (
            TemplateSlotTarget::ParameterBinding {
                node_id,
                param_name,
            },
            TemplateSlotValue::Number(number),
        ) => {
            let expression = number.to_string();
            if let Some(binding) = project
                .param_bindings
                .iter_mut()
                .find(|binding| binding.node_id == *node_id && binding.param_name == *param_name)
            {
                binding.expression = expression;
            } else {
                project.param_bindings.push(ParamBindingDef {
                    node_id: node_id.clone(),
                    param_name: param_name.clone(),
                    expression,
                });
            }
        }
        _ => {
            return Err(fail(format!(
                "value {value:?} does not match target {:?}",
                slot.target
            )));
        }
    }
    Ok(())
}

fn find_node_mut_recursive<'a>(
    graph: &'a mut RenderGraphDef,
    id: &NodeId,
) -> Option<&'a mut crate::graph::GraphNodeDef> {
    if let Some(index) = graph.nodes.iter().position(|node| &node.id == id) {
        return graph.nodes.get_mut(index);
    }
    for node in &mut graph.nodes {
        if let NodeKind::Precomp { sub_graph, .. } = &mut node.kind
            && let Some(found) = find_node_mut_recursive(sub_graph, id)
        {
            return Some(found);
        }
    }
    None
}

fn validate_package_relative_path(path: &str) -> Result<(), String> {
    let path_obj = Path::new(path);
    if path.trim().is_empty() || path_obj.is_absolute() {
        return Err(format!(
            "package path '{path}' must be a non-empty relative path"
        ));
    }
    if path_obj.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(format!(
            "package path '{path}' must not escape its local package directory"
        ));
    }
    Ok(())
}
