//! Local, reviewed template packages and effect-chain presets with typed, validated slots.
//!
//! A template contains an ordinary FrameShift project. The project remains the
//! authority for rendering; package metadata describes review, dependencies,
//! and the limited values that a template consumer may replace. This module
//! does not execute package code or fetch remote packages. Runtime `CustomWgsl`
//! nodes are rejected, including when nested inside precomps.

use crate::error::SchemaError;
use crate::graph::{
    BevelProfile3D, BlendMode, Camera3DMode, GraphNodeDef, HalftoneColorMode, MatteMode, NodeId,
    NodeKind, RenderGraphDef, SampleFilter, Shape3DKind, ShutterPhase, SourceFit, TextAlign2D,
    WritingMode2D, node_parent, validate_dag_nodes,
};
use crate::project::{AssetKind, ParamBindingDef, Project, bindable_param_names};
use crate::version::SchemaVersion;
use frameshift_time::{Flicks, SafeExpr, SpeedCurve};
use serde::de::DeserializeOwned;
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

/// Reusable, data-only effect-chain preset fragment (`.fsfx.json`) that can be
/// validated standalone and spliced into a project render graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectChainPreset {
    pub schema_version: SchemaVersion,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub nodes: Vec<GraphNodeDef>,
    #[serde(default)]
    pub param_bindings: Vec<ParamBindingDef>,
    #[serde(default)]
    pub slots: Vec<TemplateSlot>,
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
    Color,
    Choice,
    SpeedCurve,
    Font,
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
    Color([f32; 4]),
    Choice(String),
    SpeedCurve(SpeedCurve),
    Font(String),
}

/// Supported, explicit destinations for template inputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "target", rename_all = "snake_case")]
pub enum TemplateSlotTarget {
    AssetPath {
        asset_id: String,
    },
    NodeText {
        node_id: NodeId,
    },
    EventTargetTick {
        event_id: String,
    },
    NodeEnabled {
        node_id: NodeId,
    },
    ParameterBinding {
        node_id: NodeId,
        param_name: String,
    },
    NodeColor {
        node_id: NodeId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        field: Option<String>,
    },
    NodeChoice {
        node_id: NodeId,
        field: String,
    },
    ClipSpeedCurve {
        clip_id: String,
    },
    NodeFont {
        node_id: NodeId,
    },
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

        validate_slot_list(&self.template_id, &self.slots, |slot| {
            self.validate_slot_target(slot)
        })?;

        let mut file_paths = BTreeSet::new();
        for file in &self.manifest.files {
            validate_package_relative_path(&file.relative_path).map_err(fail)?;
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
            validate_package_relative_path(path).map_err(fail)?;
        }
        validate_review(&self.template_id, &self.review, "packages")?;
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
                Ok(())
            }
            (TemplateSlotKind::Tick, TemplateSlotTarget::EventTargetTick { event_id }) => {
                if self.project.event(event_id).is_none() {
                    return Err(fail(format!("event '{event_id}' does not exist")));
                }
                Ok(())
            }
            (TemplateSlotKind::SpeedCurve, TemplateSlotTarget::ClipSpeedCurve { clip_id }) => {
                if self.project.clip(clip_id).is_none() {
                    return Err(fail(format!("clip '{clip_id}' does not exist")));
                }
                Ok(())
            }
            _ => validate_graph_slot_target(&self.project.graph.nodes, slot),
        }
    }
}

impl EffectChainPreset {
    /// Loads an effect-chain preset JSON file from disk and validates it.
    pub fn load_local(path: impl AsRef<Path>) -> Result<Self, SchemaError> {
        let json = fs::read_to_string(path).map_err(|error| SchemaError::InvalidTemplate {
            template_id: String::new(),
            message: format!("could not read local effect chain preset: {error}"),
        })?;
        let preset: Self = serde_json::from_str(&json)?;
        preset.validate()?;
        Ok(preset)
    }

    /// Serializes the preset for storage or inspection.
    pub fn to_json_pretty(&self) -> Result<String, SchemaError> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Validates this preset by delegating to [`validate_effect_chain_preset`].
    pub fn validate(&self) -> Result<(), SchemaError> {
        validate_effect_chain_preset(self)
    }
}

/// Validates a standalone [`EffectChainPreset`] fragment:
/// - Compatible `schema_version` (`same major, minor <= current`)
/// - Non-empty `id`, `name`, and `nodes`
/// - Rejects `NodeKind::CustomWgsl` anywhere in `nodes` or nested `Precomp`s
/// - Validates node fields, arity, upstream edge existence, acyclicity, and single-sink connectivity
/// - Validates `param_bindings`, `slots`, and `review`
pub fn validate_effect_chain_preset(preset: &EffectChainPreset) -> Result<(), SchemaError> {
    let fail = |message: String| SchemaError::InvalidTemplate {
        template_id: preset.id.clone(),
        message,
    };
    if !preset.schema_version.is_compatible_with_current() {
        return Err(SchemaError::UnsupportedSchemaVersion(preset.schema_version));
    }
    if preset.id.trim().is_empty() || preset.name.trim().is_empty() {
        return Err(fail("preset id and name must be non-empty".into()));
    }
    if preset.nodes.is_empty() {
        return Err(fail(
            "effect chain preset must contain at least one node".into(),
        ));
    }
    if let Some(node_path) = custom_wgsl_node_path_in_slice(&preset.nodes, "preset") {
        return Err(fail(format!(
            "CustomWgsl node '{node_path}' is not allowed in effect chain presets; presets may not contain executable shader code"
        )));
    }

    validate_dag_nodes(&preset.nodes, None, 0, true)?;

    let mut consumed = BTreeSet::new();
    for node in &preset.nodes {
        for upstream in &node.inputs {
            consumed.insert(upstream);
        }
        if let NodeKind::Lut3D { input, .. } = &node.kind {
            consumed.insert(input);
        }
        if let Some(parent_id) = node_parent(&node.kind) {
            consumed.insert(parent_id);
        }
    }
    let sinks: Vec<&str> = preset
        .nodes
        .iter()
        .filter(|node| !consumed.contains(&node.id))
        .map(|node| node.id.0.as_str())
        .collect();
    if sinks.len() != 1 {
        return Err(fail(format!(
            "effect chain preset must converge to a single output node, found {} output(s): {}",
            sinks.len(),
            sinks.join(", ")
        )));
    }

    for binding in &preset.param_bindings {
        let Some(node) = find_node_in_slice(&preset.nodes, &binding.node_id) else {
            return Err(SchemaError::MissingNode(binding.node_id.clone()));
        };
        let allowed = bindable_param_names(&node.kind);
        if !allowed.contains(&binding.param_name.as_str()) {
            let allowed_list = if allowed.is_empty() {
                "none".to_owned()
            } else {
                allowed.join(", ")
            };
            return Err(SchemaError::InvalidParamBinding {
                node_id: binding.node_id.clone(),
                param_name: binding.param_name.clone(),
                message: format!(
                    "not a bindable parameter of this node kind (allowed: {allowed_list})"
                ),
            });
        }
        if let Err(err) = SafeExpr::parse(&binding.expression) {
            return Err(SchemaError::InvalidParamBinding {
                node_id: binding.node_id.clone(),
                param_name: binding.param_name.clone(),
                message: err.to_string(),
            });
        }
    }

    validate_slot_list(&preset.id, &preset.slots, |slot| {
        validate_graph_slot_target(&preset.nodes, slot)
    })?;
    validate_review(&preset.id, &preset.review, "presets")?;
    Ok(())
}

fn validate_review(
    template_id: &str,
    review: &TemplateReview,
    entity_plural: &str,
) -> Result<(), SchemaError> {
    if review.reviewed
        && review
            .reviewer
            .as_deref()
            .is_none_or(|reviewer| reviewer.trim().is_empty())
    {
        return Err(SchemaError::InvalidTemplate {
            template_id: template_id.to_owned(),
            message: format!("reviewed {entity_plural} must include a non-empty reviewer"),
        });
    }
    Ok(())
}

fn validate_slot_list(
    template_id: &str,
    slots: &[TemplateSlot],
    mut validate_target: impl FnMut(&TemplateSlot) -> Result<(), SchemaError>,
) -> Result<(), SchemaError> {
    let fail = |message: String| SchemaError::InvalidTemplate {
        template_id: template_id.to_owned(),
        message,
    };
    let mut slot_ids = BTreeSet::new();
    let mut slot_targets = BTreeSet::new();
    for slot in slots {
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
        validate_target(slot)?;
    }
    Ok(())
}

fn validate_graph_slot_target(
    nodes: &[GraphNodeDef],
    slot: &TemplateSlot,
) -> Result<(), SchemaError> {
    let fail = |message: String| SchemaError::InvalidTemplateSlot {
        slot_id: slot.id.clone(),
        message,
    };
    match (&slot.kind, &slot.target) {
        (TemplateSlotKind::Text, TemplateSlotTarget::NodeText { node_id }) => {
            match find_node_in_slice(nodes, node_id) {
                Some(node)
                    if matches!(
                        &node.kind,
                        NodeKind::VectorText { .. } | NodeKind::Text3D { .. }
                    ) => {}
                Some(_) => return Err(fail(format!("node '{node_id}' is not a text node"))),
                None => return Err(fail(format!("node '{node_id}' does not exist"))),
            }
        }
        (TemplateSlotKind::Font, TemplateSlotTarget::NodeFont { node_id }) => {
            match find_node_in_slice(nodes, node_id) {
                Some(node)
                    if matches!(
                        &node.kind,
                        NodeKind::VectorText { .. } | NodeKind::Text3D { .. }
                    ) => {}
                Some(_) => return Err(fail(format!("node '{node_id}' is not a text node"))),
                None => return Err(fail(format!("node '{node_id}' does not exist"))),
            }
        }
        (TemplateSlotKind::Boolean, TemplateSlotTarget::NodeEnabled { node_id }) => {
            if find_node_in_slice(nodes, node_id).is_none() {
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
            let Some(node) = find_node_in_slice(nodes, node_id) else {
                return Err(fail(format!("node '{node_id}' does not exist")));
            };
            if param_name.trim().is_empty() {
                return Err(fail("parameter name must be non-empty".into()));
            }
            let allowed = bindable_param_names(&node.kind);
            if !allowed.contains(&param_name.as_str()) {
                return Err(fail(format!(
                    "parameter '{param_name}' is not bindable on node '{node_id}'"
                )));
            }
        }
        (TemplateSlotKind::Color, TemplateSlotTarget::NodeColor { node_id, field }) => {
            let Some(node) = find_node_in_slice(nodes, node_id) else {
                return Err(fail(format!("node '{node_id}' does not exist")));
            };
            let mut probe = node.kind.clone();
            apply_node_color(&mut probe, field.as_deref(), [1.0, 1.0, 1.0, 1.0])
                .map_err(|msg| fail(format!("node '{node_id}': {msg}")))?;
        }
        (TemplateSlotKind::Choice, TemplateSlotTarget::NodeChoice { node_id, field }) => {
            let Some(node) = find_node_in_slice(nodes, node_id) else {
                return Err(fail(format!("node '{node_id}' does not exist")));
            };
            if field.trim().is_empty() {
                return Err(fail("choice field name must be non-empty".into()));
            }
            let mut probe = node.kind.clone();
            if let Some(TemplateSlotValue::Choice(choice)) = &slot.default {
                apply_node_choice(&mut probe, field, choice)
                    .map_err(|msg| fail(format!("node '{node_id}': {msg}")))?;
            } else if !supports_choice_field(&probe, field) {
                return Err(fail(format!(
                    "node '{node_id}' does not support choice field '{field}'"
                )));
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

/// Returns the first runtime custom shader node in a graph, including nested precomps.
/// Template packages are data-only and may use built-in effects, but may not carry
/// user-authored WGSL that is compiled and executed by the renderer.
fn custom_wgsl_node_path(graph: &RenderGraphDef, parent_path: &str) -> Option<String> {
    custom_wgsl_node_path_in_slice(&graph.nodes, parent_path)
}

fn custom_wgsl_node_path_in_slice(nodes: &[GraphNodeDef], parent_path: &str) -> Option<String> {
    for node in nodes {
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
        (TemplateSlotKind::Color, TemplateSlotValue::Color(rgba)) => {
            rgba.iter().all(|c| c.is_finite() && *c >= 0.0) && rgba[3] <= 1.0
        }
        (TemplateSlotKind::Choice, TemplateSlotValue::Choice(choice)) => !choice.trim().is_empty(),
        (TemplateSlotKind::SpeedCurve, TemplateSlotValue::SpeedCurve(_)) => true,
        (TemplateSlotKind::Font, TemplateSlotValue::Font(font)) => !font.trim().is_empty(),
        _ => false,
    };
    if !valid {
        return Err(SchemaError::InvalidTemplateSlot {
            slot_id: slot_id.to_owned(),
            message: format!("expected a valid value of kind {expected:?}, got {value:?}"),
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
        (TemplateSlotTarget::NodeFont { node_id }, TemplateSlotValue::Font(font)) => {
            let node = find_node_mut_recursive(&mut project.graph, node_id)
                .ok_or_else(|| fail(format!("node '{node_id}' no longer exists")))?;
            match &mut node.kind {
                NodeKind::Text3D { font_path, .. } => {
                    *font_path = font.clone();
                }
                NodeKind::VectorText {
                    font_family,
                    font_path,
                    ..
                } => {
                    if looks_like_font_path(font) {
                        *font_path = Some(font.clone());
                        *font_family = None;
                    } else {
                        *font_family = Some(font.clone());
                        *font_path = None;
                    }
                }
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
        (
            TemplateSlotTarget::NodeColor { node_id, field },
            TemplateSlotValue::Color(rgba),
        ) => {
            let node = find_node_mut_recursive(&mut project.graph, node_id)
                .ok_or_else(|| fail(format!("node '{node_id}' no longer exists")))?;
            apply_node_color(&mut node.kind, field.as_deref(), *rgba).map_err(fail)?;
        }
        (
            TemplateSlotTarget::NodeChoice { node_id, field },
            TemplateSlotValue::Choice(choice),
        ) => {
            let node = find_node_mut_recursive(&mut project.graph, node_id)
                .ok_or_else(|| fail(format!("node '{node_id}' no longer exists")))?;
            apply_node_choice(&mut node.kind, field, choice).map_err(fail)?;
        }
        (
            TemplateSlotTarget::ClipSpeedCurve { clip_id },
            TemplateSlotValue::SpeedCurve(curve),
        ) => {
            let clip = project
                .timeline
                .clips
                .iter_mut()
                .find(|clip| clip.id == *clip_id)
                .ok_or_else(|| fail(format!("clip '{clip_id}' no longer exists")))?;
            clip.speed_curve = curve.clone();
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

fn looks_like_font_path(font: &str) -> bool {
    let lower = font.to_ascii_lowercase();
    font.contains('/')
        || font.contains('\\')
        || lower.ends_with(".ttf")
        || lower.ends_with(".otf")
        || lower.ends_with(".ttc")
        || lower.ends_with(".woff")
        || lower.ends_with(".woff2")
}

fn apply_node_color(
    kind: &mut NodeKind,
    field: Option<&str>,
    rgba: [f32; 4],
) -> Result<(), String> {
    match kind {
        NodeKind::Solid { color_rgba, .. } if matches!(field, None | Some("color_rgba")) => {
            *color_rgba = rgba;
        }
        NodeKind::VectorText {
            color_rgba,
            outline_rgba,
            color_bottom_rgba,
            outer_outline_rgba,
            shadow_rgba,
            glow_rgba,
            ..
        } => match field {
            None | Some("color_rgba") => *color_rgba = rgba,
            Some("outline_rgba") => *outline_rgba = rgba,
            Some("color_bottom_rgba") => *color_bottom_rgba = Some(rgba),
            Some("outer_outline_rgba") => *outer_outline_rgba = rgba,
            Some("shadow_rgba") => *shadow_rgba = rgba,
            Some("glow_rgba") => *glow_rgba = rgba,
            Some(other) => return Err(format!("unknown VectorText color field '{other}'")),
        },
        NodeKind::VectorPathHud {
            stroke_rgba,
            fill_rgba,
            ..
        } => match field {
            None | Some("stroke_rgba") => *stroke_rgba = rgba,
            Some("fill_rgba") => *fill_rgba = rgba,
            Some(other) => return Err(format!("unknown VectorPathHud color field '{other}'")),
        },
        NodeKind::DeepGlow { tint_rgb, .. } if matches!(field, None | Some("tint_rgb")) => {
            *tint_rgb = [rgba[0], rgba[1], rgba[2]];
        }
        NodeKind::Text3D {
            color_top_rgba,
            color_bottom_rgba,
            side_color_rgba,
            outline_color_rgba,
            ..
        }
        | NodeKind::Shape3D {
            color_top_rgba,
            color_bottom_rgba,
            side_color_rgba,
            outline_color_rgba,
            ..
        } => match field {
            None | Some("color_top_rgba") => *color_top_rgba = rgba,
            Some("color_bottom_rgba") => *color_bottom_rgba = rgba,
            Some("side_color_rgba") => *side_color_rgba = rgba,
            Some("outline_color_rgba") => *outline_color_rgba = rgba,
            Some(other) => return Err(format!("unknown 3D color field '{other}'")),
        },
        NodeKind::ParticleField3D {
            color_a_rgba,
            color_b_rgba,
            ..
        }
        | NodeKind::RibbonTrail3D {
            color_a_rgba,
            color_b_rgba,
            ..
        } => match field {
            None | Some("color_a_rgba") => *color_a_rgba = rgba,
            Some("color_b_rgba") => *color_b_rgba = rgba,
            Some(other) => return Err(format!("unknown particle/ribbon color field '{other}'")),
        },
        NodeKind::Scene3D {
            background_rgba, ..
        } if matches!(field, None | Some("background_rgba")) => {
            *background_rgba = rgba;
        }
        NodeKind::BackdropScrim { fill_rgba, .. }
            if matches!(field, None | Some("fill_rgba")) =>
        {
            *fill_rgba = rgba;
        }
        _ => {
            return Err(format!(
                "does not support color slot field '{}'",
                field.unwrap_or("<default>")
            ));
        }
    }
    Ok(())
}

fn supports_choice_field(kind: &NodeKind, field: &str) -> bool {
    matches!(
        (kind, field),
        (NodeKind::Blend { .. }, "mode" | "blend_mode")
            | (NodeKind::AdjustmentLayer { .. }, "mode" | "blend_mode")
            | (NodeKind::MaskMatte { .. }, "mode")
            | (NodeKind::ShaderTransition { .. }, "shader_name")
            | (NodeKind::Lut3D { .. }, "lut_preset")
            | (NodeKind::SourceVideo { .. } | NodeKind::SourceImage { .. }, "fit")
            | (
                NodeKind::Transform2D { .. }
                    | NodeKind::CornerPin { .. }
                    | NodeKind::GridWarp { .. },
                "filter"
            )
            | (NodeKind::HalftoneGlow { .. }, "color_mode")
            | (NodeKind::MotionBlurShutter { .. }, "phase")
            | (NodeKind::Shape3D { .. }, "shape_kind")
            | (NodeKind::VectorText { .. }, "writing_mode" | "align")
            | (NodeKind::Text3D { .. }, "align" | "bevel_profile")
            | (NodeKind::Scene3D { .. }, "mode" | "camera_mode")
    )
}

fn parse_snake_enum<T: DeserializeOwned>(raw: &str, field: &str) -> Result<T, String> {
    serde_json::from_value::<T>(serde_json::Value::String(raw.to_owned()))
        .map_err(|_| format!("invalid choice '{raw}' for field '{field}'"))
}

fn apply_node_choice(kind: &mut NodeKind, field: &str, choice: &str) -> Result<(), String> {
    match (kind, field) {
        (NodeKind::Blend { mode, .. }, "mode" | "blend_mode") => {
            *mode = parse_snake_enum::<BlendMode>(choice, field)?;
        }
        (NodeKind::AdjustmentLayer { blend_mode, .. }, "mode" | "blend_mode") => {
            *blend_mode = parse_snake_enum::<BlendMode>(choice, field)?;
        }
        (NodeKind::MaskMatte { mode, .. }, "mode") => {
            *mode = parse_snake_enum::<MatteMode>(choice, field)?;
        }
        (NodeKind::ShaderTransition { shader_name, .. }, "shader_name") => {
            if choice.trim().is_empty() {
                return Err("shader_name must not be empty".into());
            }
            *shader_name = choice.to_owned();
        }
        (NodeKind::Lut3D { lut_preset, .. }, "lut_preset") => {
            if choice.trim().is_empty() {
                return Err("lut_preset must not be empty".into());
            }
            *lut_preset = choice.to_owned();
        }
        (NodeKind::SourceVideo { fit, .. } | NodeKind::SourceImage { fit, .. }, "fit") => {
            *fit = parse_snake_enum::<SourceFit>(choice, field)?;
        }
        (
            NodeKind::Transform2D { filter, .. }
            | NodeKind::CornerPin { filter, .. }
            | NodeKind::GridWarp { filter, .. },
            "filter",
        ) => {
            *filter = parse_snake_enum::<SampleFilter>(choice, field)?;
        }
        (NodeKind::HalftoneGlow { color_mode, .. }, "color_mode") => {
            *color_mode = parse_snake_enum::<HalftoneColorMode>(choice, field)?;
        }
        (NodeKind::MotionBlurShutter { phase, .. }, "phase") => {
            *phase = parse_snake_enum::<ShutterPhase>(choice, field)?;
        }
        (NodeKind::Shape3D { shape_kind, .. }, "shape_kind") => {
            *shape_kind = parse_snake_enum::<Shape3DKind>(choice, field)?;
        }
        (NodeKind::VectorText { writing_mode, .. }, "writing_mode") => {
            *writing_mode = parse_snake_enum::<WritingMode2D>(choice, field)?;
        }
        (NodeKind::VectorText { align, .. }, "align") => {
            *align = Some(parse_snake_enum::<TextAlign2D>(choice, field)?);
        }
        (NodeKind::Text3D { align, .. }, "align") => {
            *align = parse_snake_enum::<TextAlign2D>(choice, field)?;
        }
        (NodeKind::Text3D { bevel_profile, .. }, "bevel_profile") => {
            *bevel_profile = parse_snake_enum::<BevelProfile3D>(choice, field)?;
        }
        (NodeKind::Scene3D { camera, .. }, "mode" | "camera_mode") => {
            camera.mode = parse_snake_enum::<Camera3DMode>(choice, field)?;
        }
        _ => return Err(format!("does not support choice field '{field}'")),
    }
    Ok(())
}

fn find_node_in_slice<'a>(nodes: &'a [GraphNodeDef], id: &NodeId) -> Option<&'a GraphNodeDef> {
    for node in nodes {
        if &node.id == id {
            return Some(node);
        }
        if let NodeKind::Precomp { sub_graph, .. } = &node.kind
            && let Some(found) = sub_graph.find_node_recursive(id)
        {
            return Some(found);
        }
    }
    None
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
