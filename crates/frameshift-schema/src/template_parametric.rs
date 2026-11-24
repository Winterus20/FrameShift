//! Parametric template schema (`ParametricTemplateSpec`) with typed props,
//! declarative metadata rules, and `TemplateMetadata` output contract.

use crate::error::SchemaError;
use crate::project::Resolution;
use crate::template::TemplateReview;
use frameshift_time::{Flicks, Rational32};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Current specification version for [`ParametricTemplateSpec`].
pub const PARAMETRIC_TEMPLATE_SPEC_VERSION: u32 = 1;

/// Duration calculation strategy driven by input audio/beats.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DurationPolicy {
    #[default]
    AudioLength,
    BeatCount {
        beats: u32,
    },
    DownbeatBars {
        bars: u32,
    },
    ClampMs {
        min_ms: u64,
        max_ms: u64,
    },
}

/// Which rhythmic onset series drives automatic cut placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BeatSelectionSource {
    #[default]
    Downbeats,
    SubOnsets,
    BeatGrid,
}

/// Typed schema for one input prop accepted by a [`ParametricTemplateSpec`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TemplatePropKind {
    VideoClipList {
        min_items: usize,
        max_items: usize,
    },
    AudioFile,
    Text {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_len: Option<usize>,
    },
    Number {
        min: f64,
        max: f64,
    },
    ColorHex,
    Choice {
        options: Vec<String>,
    },
    Boolean,
}

/// One typed prop definition on a [`ParametricTemplateSpec`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TemplatePropDef {
    pub id: String,
    pub label: String,
    pub kind: TemplatePropKind,
    #[serde(default = "default_true")]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<serde_json::Value>,
}

fn default_true() -> bool {
    true
}

/// Declarative timing, aspect-ratio, and effect-window rules for `calculate_metadata`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetadataRulesDef {
    pub default_fps: Rational32,
    #[serde(default)]
    pub aspect_presets: BTreeMap<String, Resolution>,
    #[serde(default)]
    pub duration_policy: DurationPolicy,
    #[serde(default)]
    pub beat_source: BeatSelectionSource,
    pub cuts_per_beat: Rational32,
    #[serde(default = "default_min_segment_ms")]
    pub min_segment_ms: u64,
    #[serde(default = "default_transition_shader")]
    pub transition_shader: String,
    #[serde(default = "default_transition_dur_ms")]
    pub transition_dur_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drop_fx_preset_id: Option<String>,
    #[serde(default = "default_drop_fx_window_ms")]
    pub drop_fx_window_ms: [i64; 2],
}

fn default_min_segment_ms() -> u64 {
    280
}

fn default_transition_shader() -> String {
    "whip_chromatic".to_owned()
}

fn default_transition_dur_ms() -> u64 {
    250
}

fn default_drop_fx_window_ms() -> [i64; 2] {
    [-120, 380]
}

/// How the parametric template materializes its `Project` graph once metadata is resolved.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MotionBlueprintDef {
    AutoCut {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        include_title_prop: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        shutter_angle_deg: Option<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        shutter_samples: Option<u32>,
    },
    MotionMarkdown {
        markdown_template: String,
    },
}

impl Default for MotionBlueprintDef {
    fn default() -> Self {
        Self::AutoCut {
            include_title_prop: None,
            shutter_angle_deg: Some(180.0),
            shutter_samples: Some(8),
        }
    }
}

/// Declarative, data-only parametric template specification (Remotion `calculateMetadata` model).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParametricTemplateSpec {
    pub spec_version: u32,
    pub template_id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub props_schema: Vec<TemplatePropDef>,
    pub metadata_rules: MetadataRulesDef,
    #[serde(default)]
    pub motion_blueprint: MotionBlueprintDef,
    #[serde(default)]
    pub review: TemplateReview,
}

/// Resolved drop anchor emitted inside [`TemplateMetadata`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateDropEvent {
    pub event_id: String,
    pub label: String,
    pub target_output_tick: Flicks,
}

/// Resolved drop effect window emitted inside [`TemplateMetadata`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateEffectWindow {
    pub event_id: String,
    pub preset_id: String,
    pub start_tick: Flicks,
    pub end_tick: Flicks,
}

/// Resolved cut assignment emitted inside [`TemplateMetadata`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateClipAssignment {
    pub clip_id: String,
    pub asset_path: String,
    pub timeline_start: Flicks,
    pub timeline_end: Flicks,
    pub source_start: Flicks,
}

/// Dry-run resolved metadata produced before template instantiation / rendering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TemplateMetadata {
    pub template_id: String,
    pub resolved_duration_ticks: Flicks,
    pub resolved_duration_sec: f64,
    pub resolved_fps: Rational32,
    pub resolved_resolution: Resolution,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_bpm: Option<f64>,
    pub cut_count: usize,
    #[serde(default)]
    pub drop_events: Vec<TemplateDropEvent>,
    #[serde(default)]
    pub effect_windows: Vec<TemplateEffectWindow>,
    #[serde(default)]
    pub clip_assignments: Vec<TemplateClipAssignment>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// Alias matching design-doc terminology for [`TemplateMetadata`].
pub type CalculatedTemplateMetadata = TemplateMetadata;

impl ParametricTemplateSpec {
    /// Validates structural invariants of the parametric template specification.
    pub fn validate(&self) -> Result<(), SchemaError> {
        let fail = |msg: String| SchemaError::InvalidTemplate {
            template_id: self.template_id.clone(),
            message: msg,
        };
        if self.spec_version != PARAMETRIC_TEMPLATE_SPEC_VERSION {
            return Err(fail(format!(
                "unsupported parametric spec_version {} (expected {})",
                self.spec_version, PARAMETRIC_TEMPLATE_SPEC_VERSION
            )));
        }
        if self.template_id.trim().is_empty() || self.name.trim().is_empty() {
            return Err(fail("template_id and name must be non-empty".into()));
        }
        if self.metadata_rules.default_fps.num == 0 || self.metadata_rules.default_fps.den == 0 {
            return Err(fail("metadata_rules.default_fps must be non-zero".into()));
        }
        if self.metadata_rules.cuts_per_beat.num == 0 || self.metadata_rules.cuts_per_beat.den == 0
        {
            return Err(fail("metadata_rules.cuts_per_beat must be non-zero".into()));
        }
        if self.metadata_rules.drop_fx_window_ms[0] >= self.metadata_rules.drop_fx_window_ms[1] {
            return Err(fail(
                "metadata_rules.drop_fx_window_ms[0] must be < drop_fx_window_ms[1]".into(),
            ));
        }
        for (preset, res) in &self.metadata_rules.aspect_presets {
            if res.width == 0 || res.height == 0 {
                return Err(fail(format!(
                    "aspect_preset '{preset}' resolution must be non-zero"
                )));
            }
        }
        if let DurationPolicy::ClampMs { min_ms, max_ms } = &self.metadata_rules.duration_policy
            && (min_ms >= max_ms || *min_ms == 0)
        {
            return Err(fail(
                "duration_policy ClampMs requires 0 < min_ms < max_ms".into(),
            ));
        }

        let mut seen_props = BTreeSet::new();
        for prop in &self.props_schema {
            if prop.id.trim().is_empty() {
                return Err(fail("prop id must not be empty".into()));
            }
            if !seen_props.insert(prop.id.as_str()) {
                return Err(fail(format!("duplicate prop id '{}'", prop.id)));
            }
            match &prop.kind {
                TemplatePropKind::VideoClipList {
                    min_items,
                    max_items,
                } => {
                    if *min_items > *max_items || *max_items == 0 {
                        return Err(fail(format!(
                            "prop '{}' VideoClipList requires min_items <= max_items and max_items > 0",
                            prop.id
                        )));
                    }
                }
                TemplatePropKind::Number { min, max } => {
                    if !min.is_finite() || !max.is_finite() || min > max {
                        return Err(fail(format!(
                            "prop '{}' Number requires finite min <= max",
                            prop.id
                        )));
                    }
                }
                TemplatePropKind::Choice { options } if options.is_empty() => {
                    return Err(fail(format!(
                        "prop '{}' Choice must define at least one option",
                        prop.id
                    )));
                }
                _ => {}
            }
            if let Some(default_val) = &prop.default_value {
                validate_prop_value(&prop.id, &prop.kind, default_val)
                    .map_err(|msg| fail(format!("invalid default_value: {msg}")))?;
            }
        }
        Ok(())
    }

    /// Validates user-supplied `props` against `self.props_schema`, returning merged
    /// effective props (with `default_value` applied for omitted optional props).
    pub fn validate_props(
        &self,
        props: &BTreeMap<String, serde_json::Value>,
    ) -> Result<BTreeMap<String, serde_json::Value>, SchemaError> {
        self.validate()?;
        let schema_by_id: BTreeMap<&str, &TemplatePropDef> =
            self.props_schema.iter().map(|p| (p.id.as_str(), p)).collect();
        for supplied in props.keys() {
            if !schema_by_id.contains_key(supplied.as_str()) {
                return Err(SchemaError::UnknownTemplateSlot(supplied.clone()));
            }
        }

        let mut merged = BTreeMap::new();
        for prop in &self.props_schema {
            if let Some(val) = props.get(&prop.id) {
                validate_prop_value(&prop.id, &prop.kind, val).map_err(|message| {
                    SchemaError::InvalidTemplateSlot {
                        slot_id: prop.id.clone(),
                        message,
                    }
                })?;
                merged.insert(prop.id.clone(), val.clone());
            } else if let Some(def) = &prop.default_value {
                merged.insert(prop.id.clone(), def.clone());
            } else if prop.required {
                return Err(SchemaError::MissingTemplateSlot(prop.id.clone()));
            }
        }
        Ok(merged)
    }
}

fn validate_prop_value(
    prop_id: &str,
    kind: &TemplatePropKind,
    val: &serde_json::Value,
) -> Result<(), String> {
    match kind {
        TemplatePropKind::VideoClipList {
            min_items,
            max_items,
        } => {
            let arr = val
                .as_array()
                .ok_or_else(|| format!("prop '{prop_id}' expected array of paths"))?;
            if arr.len() < *min_items || arr.len() > *max_items {
                return Err(format!(
                    "prop '{prop_id}' expects {min_items}..={max_items} clips, got {}",
                    arr.len()
                ));
            }
            if arr
                .iter()
                .any(|item| item.as_str().is_none_or(|s| s.trim().is_empty()))
            {
                return Err(format!(
                    "prop '{prop_id}' entries must be non-empty strings"
                ));
            }
        }
        TemplatePropKind::AudioFile => {
            let s = val
                .as_str()
                .ok_or_else(|| format!("prop '{prop_id}' expected audio path string"))?;
            if s.trim().is_empty() {
                return Err(format!("prop '{prop_id}' audio path must not be empty"));
            }
        }
        TemplatePropKind::Text { max_len } => {
            let s = val
                .as_str()
                .ok_or_else(|| format!("prop '{prop_id}' expected string"))?;
            if let Some(limit) = max_len
                && s.chars().count() > *limit
            {
                return Err(format!(
                    "prop '{prop_id}' text length exceeds max_len ({limit})"
                ));
            }
        }
        TemplatePropKind::Number { min, max } => {
            let n = val
                .as_f64()
                .ok_or_else(|| format!("prop '{prop_id}' expected finite number"))?;
            if !n.is_finite() || n < *min || n > *max {
                return Err(format!(
                    "prop '{prop_id}' value {n} out of range [{min}, {max}]"
                ));
            }
        }
        TemplatePropKind::ColorHex => {
            let s = val
                .as_str()
                .ok_or_else(|| format!("prop '{prop_id}' expected '#RRGGBB' hex string"))?;
            let hex = s.strip_prefix('#').unwrap_or("");
            if !(hex.len() == 6 || hex.len() == 8) || !hex.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err(format!(
                    "prop '{prop_id}' invalid ColorHex '{s}' (expected #RRGGBB or #RRGGBBAA)"
                ));
            }
        }
        TemplatePropKind::Choice { options } => {
            let s = val
                .as_str()
                .ok_or_else(|| format!("prop '{prop_id}' expected choice string"))?;
            if !options.iter().any(|opt| opt == s) {
                return Err(format!(
                    "prop '{prop_id}' value '{s}' not in allowed options {options:?}"
                ));
            }
        }
        TemplatePropKind::Boolean => {
            if !val.is_boolean() {
                return Err(format!("prop '{prop_id}' expected boolean"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_spec() -> ParametricTemplateSpec {
        ParametricTemplateSpec {
            spec_version: PARAMETRIC_TEMPLATE_SPEC_VERSION,
            template_id: "anime_drop_v1".into(),
            name: "Anime Drop V1".into(),
            description: "Beat-synced anime montage".into(),
            props_schema: vec![
                TemplatePropDef {
                    id: "clips".into(),
                    label: "Video Clips".into(),
                    kind: TemplatePropKind::VideoClipList {
                        min_items: 2,
                        max_items: 16,
                    },
                    required: true,
                    default_value: None,
                },
                TemplatePropDef {
                    id: "bgm".into(),
                    label: "Music".into(),
                    kind: TemplatePropKind::AudioFile,
                    required: true,
                    default_value: None,
                },
                TemplatePropDef {
                    id: "accent".into(),
                    label: "Accent Color".into(),
                    kind: TemplatePropKind::ColorHex,
                    required: false,
                    default_value: Some(serde_json::json!("#FF44AA")),
                },
                TemplatePropDef {
                    id: "aspect".into(),
                    label: "Aspect".into(),
                    kind: TemplatePropKind::Choice {
                        options: vec!["16:9".into(), "9:16".into()],
                    },
                    required: false,
                    default_value: Some(serde_json::json!("16:9")),
                },
            ],
            metadata_rules: MetadataRulesDef {
                default_fps: Rational32 { num: 24, den: 1 },
                aspect_presets: BTreeMap::from([
                    (
                        "16:9".into(),
                        Resolution {
                            width: 1920,
                            height: 1080,
                        },
                    ),
                    (
                        "9:16".into(),
                        Resolution {
                            width: 1080,
                            height: 1920,
                        },
                    ),
                ]),
                duration_policy: DurationPolicy::AudioLength,
                beat_source: BeatSelectionSource::Downbeats,
                cuts_per_beat: Rational32 { num: 1, den: 1 },
                min_segment_ms: 280,
                transition_shader: "whip_chromatic".into(),
                transition_dur_ms: 250,
                drop_fx_preset_id: Some("xenoz_impact_glow".into()),
                drop_fx_window_ms: [-120, 380],
            },
            motion_blueprint: MotionBlueprintDef::default(),
            review: TemplateReview::default(),
        }
    }

    #[test]
    fn parametric_template_spec_roundtrips_and_validates_props() {
        let spec = sample_spec();
        let json = serde_json::to_string_pretty(&spec).unwrap();
        let parsed: ParametricTemplateSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(spec, parsed);

        let supplied = BTreeMap::from([
            (
                "clips".into(),
                serde_json::json!(["synthetic://a", "synthetic://b"]),
            ),
            ("bgm".into(), serde_json::json!("synthetic://beat")),
        ]);
        let merged = parsed.validate_props(&supplied).expect("valid props");
        assert_eq!(merged["accent"], serde_json::json!("#FF44AA"));
        assert_eq!(merged["aspect"], serde_json::json!("16:9"));
    }

    #[test]
    fn parametric_template_rejects_invalid_props_and_specs() {
        let spec = sample_spec();
        let too_few = BTreeMap::from([
            ("clips".into(), serde_json::json!(["synthetic://a"])),
            ("bgm".into(), serde_json::json!("synthetic://beat")),
        ]);
        assert!(matches!(
            spec.validate_props(&too_few),
            Err(SchemaError::InvalidTemplateSlot { .. })
        ));

        let bad_hex = BTreeMap::from([
            (
                "clips".into(),
                serde_json::json!(["synthetic://a", "synthetic://b"]),
            ),
            ("bgm".into(), serde_json::json!("synthetic://beat")),
            ("accent".into(), serde_json::json!("red")),
        ]);
        assert!(matches!(
            spec.validate_props(&bad_hex),
            Err(SchemaError::InvalidTemplateSlot { .. })
        ));
    }
}
