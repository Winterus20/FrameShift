//! P2 static/media project linter (`lint_project`) and frame diff comparator (`diff_frame`).
//!
//! Detects the 4 silent failure traps documented in `brain/lessons.md` (`LNT-MED-001`,
//! `LNT-TXT-001`, `LNT-CLR-001`, `LNT-CMP-001`) plus structural, timing, security,
//! performance, and matte/LUT ordering issues (`LNT-DAG-001/002`, `LNT-TIM-001`,
//! `LNT-SEC-001`, `LNT-PERF-001`, `LNT-MAT-001`).

use crate::error::CoreError;
use crate::resolver::{MediaResolver, ResolvedAssetSource};
use frameshift_media::{AlphaMode, Rgba8Frame, detect_black_intervals};
use frameshift_schema::{
    AssetKind, BlendMode, ColorSpace, GraphNodeDef, MAX_PRECOMP_DEPTH, NodeId, NodeKind, Project,
    RenderGraphDef, bindable_param_names,
};
use frameshift_time::Flicks;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;

/// Severity level for a [`LintFinding`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LintSeverity {
    Error,
    Warning,
    Info,
}

/// Single diagnostic finding produced by [`lint_project`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LintFinding {
    pub code: String,
    pub severity: LintSeverity,
    pub node_id: Option<String>,
    pub message: String,
    pub suggestion: String,
}

/// Configuration options for [`lint_project`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LintOptions {
    /// Opt-in: invoke `ffmpeg blackdetect` on on-disk video assets (`LNT-MED-001`).
    pub check_media: bool,
    /// Minimum black interval duration in seconds passed to `detect_black_intervals`.
    pub black_min_duration_sec: f64,
    /// Minimum fraction of black pixels per frame (`0.0..=1.0`) passed to `detect_black_intervals`.
    pub black_pic_th: f64,
    /// When `true`, nested `CustomWgsl` nodes (`LNT-SEC-001`) are promoted from `Warning` to `Error`.
    pub strict_sandbox: bool,
    /// `MotionBlurShutter.sub_samples` threshold above which `LNT-PERF-001` warns (default `16`).
    pub max_blur_sub_samples_warn: u32,
}

impl Default for LintOptions {
    fn default() -> Self {
        Self {
            check_media: false,
            black_min_duration_sec: 0.04,
            black_pic_th: 0.98,
            strict_sandbox: false,
            max_blur_sub_samples_warn: 16,
        }
    }
}

/// Aggregated lint result returned by [`lint_project`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LintReport {
    pub findings: Vec<LintFinding>,
    pub error_count: usize,
    pub warning_count: usize,
    pub info_count: usize,
    pub ok: bool,
}

impl LintReport {
    fn from_findings(findings: Vec<LintFinding>) -> Self {
        let mut error_count = 0usize;
        let mut warning_count = 0usize;
        let mut info_count = 0usize;
        for f in &findings {
            match f.severity {
                LintSeverity::Error => error_count += 1,
                LintSeverity::Warning => warning_count += 1,
                LintSeverity::Info => info_count += 1,
            }
        }
        Self {
            findings,
            error_count,
            warning_count,
            info_count,
            ok: error_count == 0,
        }
    }
}

/// Inspects `project` for silent visual traps, DAG anomalies, timing bugs, sandbox violations,
/// expensive GPU configurations, and optional on-disk black-frame intervals.
pub fn lint_project(project: &Project, options: &LintOptions) -> LintReport {
    let mut findings = Vec::new();

    if let Err(err) = project.validate() {
        findings.push(LintFinding {
            code: "LNT-DAG-000".into(),
            severity: LintSeverity::Error,
            node_id: None,
            message: format!("Schema validation failed: {err}"),
            suggestion: "Fix structural schema errors reported by Project::validate().".into(),
        });
    }

    lint_graph_recursive(project, &project.graph, "root", 0, options, &mut findings);
    lint_bindings_and_animations(project, &mut findings);
    lint_timing_and_assets(project, &mut findings);

    if options.check_media {
        lint_media_black_intervals(project, options, &mut findings);
    }

    LintReport::from_findings(findings)
}

fn scoped_path(prefix: &str, id: &NodeId) -> String {
    format!("{prefix}/{}", id.0)
}

fn lint_graph_recursive(
    project: &Project,
    graph: &RenderGraphDef,
    scope_prefix: &str,
    depth: usize,
    options: &LintOptions,
    findings: &mut Vec<LintFinding>,
) {
    let node_map: BTreeMap<&str, &GraphNodeDef> =
        graph.nodes.iter().map(|n| (n.id.0.as_str(), n)).collect();

    // LNT-DAG-001: Reachability from graph.output_node.
    let reachable = collect_reachable_nodes(graph, &node_map);
    for node in &graph.nodes {
        if !reachable.contains(node.id.0.as_str()) {
            findings.push(LintFinding {
                code: "LNT-DAG-001".into(),
                severity: LintSeverity::Warning,
                node_id: Some(scoped_path(scope_prefix, &node.id)),
                message: format!(
                    "Node '{}' is unreachable from output_node '{}'.",
                    node.id.0, graph.output_node.0
                ),
                suggestion: "Connect the node into the active DAG or remove the dead node.".into(),
            });
        }
    }

    // Track whether any nested Precomp contains MotionBlurShutter.
    for node in &graph.nodes {
        let node_path = scoped_path(scope_prefix, &node.id);
        match &node.kind {
            NodeKind::VectorText {
                x_offset_norm,
                y_offset_norm,
                color_rgba,
                color_bottom_rgba,
                ..
            } => {
                check_text_coords(
                    &node_path,
                    &node.id,
                    *x_offset_norm,
                    *y_offset_norm,
                    project,
                    findings,
                );
                check_linear_text_color(&node_path, *color_rgba, *color_bottom_rgba, findings);
            }
            NodeKind::Text3D {
                color_top_rgba,
                color_bottom_rgba,
                ..
            }
            | NodeKind::Shape3D {
                color_top_rgba,
                color_bottom_rgba,
                ..
            } => {
                check_linear_text_color(
                    &node_path,
                    *color_top_rgba,
                    Some(*color_bottom_rgba),
                    findings,
                );
            }
            NodeKind::Solid {
                color_rgba,
                color_space,
            } => {
                if *color_space == ColorSpace::LinearRec709
                    && color_rgba[3] > 0.01
                    && color_rgba[..3]
                        .iter()
                        .all(|&c| c > 0.02 && c < 0.98)
                {
                    findings.push(LintFinding {
                        code: "LNT-CLR-001".into(),
                        severity: LintSeverity::Warning,
                        node_id: Some(node_path.clone()),
                        message: format!(
                            "Solid '{}' uses ColorSpace::LinearRec709 with mid-range RGB {:?}; raw sRGB values here appear washed out.",
                            node.id.0,
                            [color_rgba[0], color_rgba[1], color_rgba[2]]
                        ),
                        suggestion: "Use ColorSpace::DisplaySrgb on Solid or convert channels via srgb_to_linear.".into(),
                    });
                }
            }
            NodeKind::SourceVideo {
                asset_id,
                color_space,
                ..
            }
            | NodeKind::SourceImage {
                asset_id,
                color_space,
                ..
            } => {
                if *color_space == ColorSpace::LinearRec709
                    && let Some(asset) = project.asset(asset_id)
                {
                    let lower = asset.path.to_ascii_lowercase();
                    if lower.ends_with(".mp4")
                        || lower.ends_with(".mov")
                        || lower.ends_with(".png")
                        || lower.ends_with(".jpg")
                        || lower.ends_with(".jpeg")
                    {
                        findings.push(LintFinding {
                            code: "LNT-CLR-001".into(),
                            severity: LintSeverity::Warning,
                            node_id: Some(node_path.clone()),
                            message: format!(
                                "Source '{}' ('{}') is tagged LinearRec709; standard video/PNG/JPEG assets are DisplaySrgb.",
                                node.id.0, asset.path
                            ),
                            suggestion: "Set color_space to DisplaySrgb unless the media is genuinely scene-linear.".into(),
                        });
                    }
                }
            }
            NodeKind::Blend { mode, opacity } => {
                if *mode == BlendMode::Normal && *opacity >= 0.99 && node.inputs.len() >= 2 {
                    let base_has_source = subtree_contains_source(graph, &node_map, &node.inputs[0]);
                    let top_opaque_solid = subtree_has_opaque_solid_cover(&node_map, &node.inputs[1]);
                    if base_has_source && top_opaque_solid {
                        findings.push(LintFinding {
                            code: "LNT-CMP-001".into(),
                            severity: LintSeverity::Error,
                            node_id: Some(node_path.clone()),
                            message: format!(
                                "Blend '{}' composites an opaque Solid chain ('{}') over video/image input '{}', completely hiding the underlying footage.",
                                node.id.0, node.inputs[1].0, node.inputs[0].0
                            ),
                            suggestion: "Start text/overlay chains on a transparent Solid [0,0,0,0] or feed VectorText directly into Blend.inputs[1].".into(),
                        });
                    }
                }
            }
            NodeKind::AdjustmentLayer {
                start_tick,
                end_tick,
                ..
            } => {
                if start_tick >= end_tick {
                    findings.push(LintFinding {
                        code: "LNT-TIM-001".into(),
                        severity: LintSeverity::Error,
                        node_id: Some(node_path.clone()),
                        message: format!(
                            "AdjustmentLayer '{}' has zero/negative span [{}, {}).",
                            node.id.0, start_tick.0, end_tick.0
                        ),
                        suggestion: "Ensure start_tick < end_tick.".into(),
                    });
                }
            }
            NodeKind::CustomWgsl { .. } => {
                findings.push(LintFinding {
                    code: "LNT-SEC-001".into(),
                    severity: if options.strict_sandbox {
                        LintSeverity::Error
                    } else {
                        LintSeverity::Warning
                    },
                    node_id: Some(node_path.clone()),
                    message: format!(
                        "CustomWgsl node detected at '{}' (depth {}).",
                        node_path, depth
                    ),
                    suggestion: "Replace runtime CustomWgsl with built-in shader nodes for sandboxed templates.".into(),
                });
            }
            NodeKind::MotionBlurShutter { sub_samples, .. } => {
                if *sub_samples > options.max_blur_sub_samples_warn {
                    findings.push(LintFinding {
                        code: "LNT-PERF-001".into(),
                        severity: LintSeverity::Warning,
                        node_id: Some(node_path.clone()),
                        message: format!(
                            "MotionBlurShutter '{}' uses high sub_samples={} (>{}).",
                            node.id.0, sub_samples, options.max_blur_sub_samples_warn
                        ),
                        suggestion: "Keep sub_samples <= 16 or use OpticalFlowBlur for faster GPU blur.".into(),
                    });
                }
                if let Some(up) = node.inputs.first()
                    && subtree_contains_motion_blur(&node_map, up, 0)
                {
                    findings.push(LintFinding {
                        code: "LNT-PERF-001".into(),
                        severity: LintSeverity::Warning,
                        node_id: Some(node_path.clone()),
                        message: format!(
                            "Nested MotionBlurShutter detected upstream of '{}' (multiplicative sub-frame cost).",
                            node.id.0
                        ),
                        suggestion: "Keep only a single MotionBlurShutter at the root output.".into(),
                    });
                }
            }
            NodeKind::OpticalFlowBlur { samples, .. } if *samples > 24 => {
                findings.push(LintFinding {
                    code: "LNT-PERF-001".into(),
                    severity: LintSeverity::Warning,
                    node_id: Some(node_path.clone()),
                    message: format!(
                        "OpticalFlowBlur '{}' uses samples={samples} (> 24).",
                        node.id.0
                    ),
                    suggestion: "Reduce OpticalFlowBlur samples to 8..=16 for interactive speed.".into(),
                });
            }
            NodeKind::BlurMoCurves { samples, .. } if *samples > 32 => {
                findings.push(LintFinding {
                    code: "LNT-PERF-001".into(),
                    severity: LintSeverity::Warning,
                    node_id: Some(node_path.clone()),
                    message: format!(
                        "BlurMoCurves '{}' uses samples={samples} (> 32).",
                        node.id.0
                    ),
                    suggestion: "Reduce BlurMoCurves samples to <= 24.".into(),
                });
            }
            NodeKind::Lut3D { input, .. } => {
                check_grade_order(&node_path, &node.id, input, &node_map, findings);
            }
            NodeKind::ColorBasic { .. } => {
                if let Some(input) = node.inputs.first() {
                    check_grade_order(&node_path, &node.id, input, &node_map, findings);
                }
            }
            NodeKind::RadialShockwave { radius, .. } => {
                let has_radius_driver = node_param_has_driver(project, &node.id, "radius");
                if *radius <= 0.0 && !has_radius_driver {
                    findings.push(LintFinding {
                        code: "LNT-MAT-001".into(),
                        severity: LintSeverity::Warning,
                        node_id: Some(node_path.clone()),
                        message: format!(
                            "RadialShockwave '{}' has static radius={radius} <= 0 with no dynamic binding.",
                            node.id.0
                        ),
                        suggestion: "Animate radius > 0.0 via ParamBindingDef or EventBoundAnimation.".into(),
                    });
                }
            }
            NodeKind::Precomp { sub_graph, .. } if depth < MAX_PRECOMP_DEPTH => {
                lint_graph_recursive(
                    project,
                    sub_graph,
                    &node_path,
                    depth + 1,
                    options,
                    findings,
                );
            }
            _ => {}
        }
    }
}

fn check_text_coords(
    node_path: &str,
    node_id: &NodeId,
    x_offset_norm: Option<f32>,
    y_offset_norm: f32,
    project: &Project,
    findings: &mut Vec<LintFinding>,
) {
    if !y_offset_norm.is_finite() || !(0.0..=1.0).contains(&y_offset_norm) {
        findings.push(LintFinding {
            code: "LNT-TXT-001".into(),
            severity: LintSeverity::Error,
            node_id: Some(node_path.to_owned()),
            message: format!(
                "VectorText '{}' y_offset_norm={y_offset_norm} is outside normalized [0.0, 1.0] frame bounds (off-screen).",
                node_id.0
            ),
            suggestion: "Use normalized frame coordinates: 0.5 is vertical center, 0.78..0.85 is lower-third.".into(),
        });
    } else if y_offset_norm == 0.0 || !(0.02..=0.98).contains(&y_offset_norm) {
        findings.push(LintFinding {
            code: "LNT-TXT-001".into(),
            severity: LintSeverity::Warning,
            node_id: Some(node_path.to_owned()),
            message: format!(
                "VectorText '{}' y_offset_norm={y_offset_norm} is at the frame edge (0.0 is top edge, 0.5 is screen center).",
                node_id.0
            ),
            suggestion: "Set y_offset_norm=0.5 to center vertically or 0.78..0.85 for subtitles.".into(),
        });
    }

    if let Some(x) = x_offset_norm {
        if !x.is_finite() || !(0.0..=1.0).contains(&x) {
            findings.push(LintFinding {
                code: "LNT-TXT-001".into(),
                severity: LintSeverity::Error,
                node_id: Some(node_path.to_owned()),
                message: format!(
                    "VectorText '{}' x_offset_norm={x} is outside normalized [0.0, 1.0] frame bounds.",
                    node_id.0
                ),
                suggestion: "Set x_offset_norm inside [0.05, 0.95] (0.5 is horizontal center).".into(),
            });
        } else if !(0.02..=0.98).contains(&x) {
            findings.push(LintFinding {
                code: "LNT-TXT-001".into(),
                severity: LintSeverity::Warning,
                node_id: Some(node_path.to_owned()),
                message: format!(
                    "VectorText '{}' x_offset_norm={x} sits near the horizontal frame edge.",
                    node_id.0
                ),
                suggestion: "Use x_offset_norm=0.5 for center placement.".into(),
            });
        }
    }

    for b in &project.param_bindings {
        if b.node_id == *node_id
            && (b.param_name == "x_offset_norm" || b.param_name == "y_offset_norm")
            && let Ok(val) = b.expression.trim().parse::<f64>()
            && (!val.is_finite() || !(0.0..=1.0).contains(&val))
        {
            findings.push(LintFinding {
                code: "LNT-TXT-001".into(),
                severity: LintSeverity::Error,
                node_id: Some(node_path.to_owned()),
                message: format!(
                    "ParamBinding on '{}.{}' evaluates to out-of-frame literal {val}.",
                    node_id.0, b.param_name
                ),
                suggestion: "Keep normalized text coordinate expressions inside [0.0, 1.0].".into(),
            });
        }
    }
}

fn check_linear_text_color(
    node_path: &str,
    color_rgba: [f32; 4],
    color_bottom_rgba: Option<[f32; 4]>,
    findings: &mut Vec<LintFinding>,
) {
    let check_rgba = |c: [f32; 4]| {
        c[3] > 0.01 && c[..3].iter().all(|&v| v > 0.25 && v < 0.95)
    };
    if check_rgba(color_rgba) || color_bottom_rgba.is_some_and(check_rgba) {
        findings.push(LintFinding {
            code: "LNT-CLR-001".into(),
            severity: LintSeverity::Warning,
            node_id: Some(node_path.to_owned()),
            message: format!(
                "Node '{}' uses mid-tone RGBA {:?} directly in Linear Rec.709 (if copied from sRGB hex/255, colors render washed out).",
                node_path,
                [color_rgba[0], color_rgba[1], color_rgba[2]]
            ),
            suggestion: "Convert sRGB hex values with frameshift_media::srgb_to_linear before writing Linear Rec.709 node colors.".into(),
        });
    }
}

fn check_grade_order(
    node_path: &str,
    node_id: &NodeId,
    input_id: &NodeId,
    node_map: &BTreeMap<&str, &GraphNodeDef>,
    findings: &mut Vec<LintFinding>,
) {
    let mut visited = BTreeSet::new();
    let mut stack = vec![input_id.0.as_str()];
    while let Some(curr) = stack.pop() {
        if !visited.insert(curr) {
            continue;
        }
        let Some(up) = node_map.get(curr) else {
            continue;
        };
        match &up.kind {
            NodeKind::MaskMatte {
                opaque_black_outside: true,
                ..
            } => {
                findings.push(LintFinding {
                    code: "LNT-MAT-001".into(),
                    severity: LintSeverity::Warning,
                    node_id: Some(node_path.to_owned()),
                    message: format!(
                        "Grade/LUT node '{}' sits after opaque-black MaskMatte '{}' (letterbox/black edges will be color-tinted).",
                        node_id.0, up.id.0
                    ),
                    suggestion: "Place Lut3D / ColorBasic before MaskMatte(opaque_black_outside: true).".into(),
                });
                return;
            }
            NodeKind::VectorText { .. } => {
                findings.push(LintFinding {
                    code: "LNT-MAT-001".into(),
                    severity: LintSeverity::Warning,
                    node_id: Some(node_path.to_owned()),
                    message: format!(
                        "Grade/LUT node '{}' sits after VectorText '{}' (title colors will be shifted by the LUT/grade).",
                        node_id.0, up.id.0
                    ),
                    suggestion: "Composite VectorText after the master Lut3D / ColorBasic grade.".into(),
                });
                return;
            }
            _ => {}
        }
        for inp in &up.inputs {
            stack.push(inp.0.as_str());
        }
        if let NodeKind::Lut3D { input, .. } = &up.kind {
            stack.push(input.0.as_str());
        }
    }
}

fn collect_reachable_nodes<'a>(
    graph: &'a RenderGraphDef,
    node_map: &BTreeMap<&'a str, &'a GraphNodeDef>,
) -> BTreeSet<&'a str> {
    let mut visited = BTreeSet::new();
    let mut queue = VecDeque::new();
    queue.push_back(graph.output_node.0.as_str());

    while let Some(id) = queue.pop_front() {
        if !visited.insert(id) {
            continue;
        }
        let Some(node) = node_map.get(id) else {
            continue;
        };
        for inp in &node.inputs {
            queue.push_back(inp.0.as_str());
        }
        match &node.kind {
            NodeKind::Lut3D { input, .. } => queue.push_back(input.0.as_str()),
            NodeKind::Transform3D { parent, .. } | NodeKind::Null3D { parent, .. } => {
                if let Some(p) = parent {
                    queue.push_back(p.0.as_str());
                }
            }
            NodeKind::Scene3D {
                depth_plate: Some(dp),
                ..
            } => {
                queue.push_back(dp.node_id.0.as_str());
            }
            _ => {}
        }
    }
    visited
}

fn subtree_contains_source(
    graph: &RenderGraphDef,
    node_map: &BTreeMap<&str, &GraphNodeDef>,
    root: &NodeId,
) -> bool {
    let mut visited = BTreeSet::new();
    let mut stack = vec![root.0.as_str()];
    while let Some(id) = stack.pop() {
        if !visited.insert(id) {
            continue;
        }
        let Some(node) = node_map.get(id) else {
            continue;
        };
        if !node.enabled {
            continue;
        }
        match &node.kind {
            NodeKind::SourceVideo { .. } | NodeKind::SourceImage { .. } => return true,
            NodeKind::Precomp { sub_graph, .. } => {
                let sub_map: BTreeMap<&str, &GraphNodeDef> = sub_graph
                    .nodes
                    .iter()
                    .map(|n| (n.id.0.as_str(), n))
                    .collect();
                if subtree_contains_source(sub_graph, &sub_map, &sub_graph.output_node) {
                    return true;
                }
                for inp in &node.inputs {
                    stack.push(inp.0.as_str());
                }
            }
            NodeKind::Lut3D { input, .. } => stack.push(input.0.as_str()),
            _ => {
                for inp in &node.inputs {
                    stack.push(inp.0.as_str());
                }
            }
        }
    }
    let _ = graph;
    false
}

/// Returns `true` when `root` unconditionally outputs an opaque Solid-backed frame
/// (either a `Solid { alpha >= 0.99 }` directly or a `Blend(Normal)` chain whose base is an opaque `Solid`).
fn subtree_has_opaque_solid_cover(
    node_map: &BTreeMap<&str, &GraphNodeDef>,
    root: &NodeId,
) -> bool {
    let mut curr = root.0.as_str();
    let mut steps = 0usize;
    while steps < 64 {
        steps += 1;
        let Some(node) = node_map.get(curr) else {
            return false;
        };
        if !node.enabled {
            return false;
        }
        match &node.kind {
            NodeKind::Solid { color_rgba, .. } => return color_rgba[3] >= 0.99,
            NodeKind::Blend { mode, opacity } => {
                if *mode == BlendMode::Normal && *opacity >= 0.99 && node.inputs.len() >= 2 {
                    if subtree_has_opaque_solid_cover(node_map, &node.inputs[1]) {
                        return true;
                    }
                    curr = node.inputs[0].0.as_str();
                    continue;
                }
                if let Some(base) = node.inputs.first() {
                    curr = base.0.as_str();
                    continue;
                }
                return false;
            }
            NodeKind::Lut3D { input, .. } => curr = input.0.as_str(),
            NodeKind::Transform2D { .. }
            | NodeKind::ColorBasic { .. }
            | NodeKind::DeepGlow { .. }
            | NodeKind::BlurMoCurves { .. }
            | NodeKind::AnimeSharpen { .. } => {
                let Some(first) = node.inputs.first() else {
                    return false;
                };
                curr = first.0.as_str();
            }
            _ => return false,
        }
    }
    false
}

fn subtree_contains_motion_blur(
    node_map: &BTreeMap<&str, &GraphNodeDef>,
    root: &NodeId,
    depth: usize,
) -> bool {
    if depth > MAX_PRECOMP_DEPTH {
        return false;
    }
    let mut visited = BTreeSet::new();
    let mut stack = vec![root.0.as_str()];
    while let Some(id) = stack.pop() {
        if !visited.insert(id) {
            continue;
        }
        let Some(node) = node_map.get(id) else {
            continue;
        };
        if !node.enabled {
            continue;
        }
        match &node.kind {
            NodeKind::MotionBlurShutter { .. } => return true,
            NodeKind::Precomp { sub_graph, .. } => {
                let sub_map: BTreeMap<&str, &GraphNodeDef> = sub_graph
                    .nodes
                    .iter()
                    .map(|n| (n.id.0.as_str(), n))
                    .collect();
                if subtree_contains_motion_blur(&sub_map, &sub_graph.output_node, depth + 1) {
                    return true;
                }
                for inp in &node.inputs {
                    stack.push(inp.0.as_str());
                }
            }
            NodeKind::Lut3D { input, .. } => stack.push(input.0.as_str()),
            _ => {
                for inp in &node.inputs {
                    stack.push(inp.0.as_str());
                }
            }
        }
    }
    false
}

fn node_param_has_driver(project: &Project, node_id: &NodeId, param: &str) -> bool {
    project
        .param_bindings
        .iter()
        .any(|b| b.node_id == *node_id && b.param_name == param)
        || project
            .keyframe_tracks
            .iter()
            .any(|t| t.target_node == *node_id && t.target_param == param)
        || project
            .timeline
            .clips
            .iter()
            .flat_map(|c| c.event_animations.iter())
            .any(|a| a.target_node_id == *node_id && a.parameter == param)
}

fn lint_bindings_and_animations(project: &Project, findings: &mut Vec<LintFinding>) {
    let mut binding_targets: BTreeSet<(&str, &str)> = BTreeSet::new();
    for b in &project.param_bindings {
        binding_targets.insert((b.node_id.0.as_str(), b.param_name.as_str()));
        match project.graph.find_node_recursive(&b.node_id) {
            None => {
                findings.push(LintFinding {
                    code: "LNT-DAG-002".into(),
                    severity: LintSeverity::Warning,
                    node_id: Some(b.node_id.0.clone()),
                    message: format!(
                        "ParamBinding targets missing node '{}' (param '{}').",
                        b.node_id.0, b.param_name
                    ),
                    suggestion: "Remove stale ParamBinding or update target node_id.".into(),
                });
            }
            Some(node) => {
                if !node.enabled {
                    findings.push(LintFinding {
                        code: "LNT-DAG-002".into(),
                        severity: LintSeverity::Warning,
                        node_id: Some(node.id.0.clone()),
                        message: format!(
                            "ParamBinding targets disabled node '{}' (param '{}').",
                            node.id.0, b.param_name
                        ),
                        suggestion: "Enable the node or remove the unused ParamBinding.".into(),
                    });
                }
                let allowed = bindable_param_names(&node.kind);
                if !allowed.contains(&b.param_name.as_str()) {
                    findings.push(LintFinding {
                        code: "LNT-DAG-002".into(),
                        severity: LintSeverity::Warning,
                        node_id: Some(node.id.0.clone()),
                        message: format!(
                            "ParamBinding param '{}' is not in bindable_param_names for '{}'.",
                            b.param_name, node.id.0
                        ),
                        suggestion: format!("Use one of {:?}.", allowed),
                    });
                }
            }
        }
    }

    for clip in &project.timeline.clips {
        for anim in &clip.event_animations {
            if anim.duration.0 <= 0 {
                findings.push(LintFinding {
                    code: "LNT-TIM-001".into(),
                    severity: LintSeverity::Error,
                    node_id: Some(anim.target_node_id.0.clone()),
                    message: format!(
                        "EventBoundAnimation '{}' on clip '{}' has non-positive duration ({}).",
                        anim.id, clip.id, anim.duration.0
                    ),
                    suggestion: "Set animation duration > 0 Flicks.".into(),
                });
            }
            if anim.keyframes.is_empty()
                || anim
                    .keyframes
                    .iter()
                    .any(|kf| !kf.progress.is_finite() || !(0.0..=1.0).contains(&kf.progress))
            {
                findings.push(LintFinding {
                    code: "LNT-DAG-002".into(),
                    severity: LintSeverity::Error,
                    node_id: Some(anim.target_node_id.0.clone()),
                    message: format!(
                        "EventBoundAnimation '{}' has empty keyframes or progress outside [0.0, 1.0].",
                        anim.id
                    ),
                    suggestion: "Provide at least one keyframe with progress in [0.0, 1.0].".into(),
                });
            }
            if let Some(node) = project.graph.find_node_recursive(&anim.target_node_id)
                && !node.enabled
            {
                findings.push(LintFinding {
                    code: "LNT-DAG-002".into(),
                    severity: LintSeverity::Warning,
                    node_id: Some(node.id.0.clone()),
                    message: format!(
                        "EventBoundAnimation '{}' targets disabled node '{}'.",
                        anim.id, node.id.0
                    ),
                    suggestion: "Enable the target node or remove the animation.".into(),
                });
            }
            if binding_targets.contains(&(anim.target_node_id.0.as_str(), anim.parameter.as_str()))
            {
                findings.push(LintFinding {
                    code: "LNT-DAG-002".into(),
                    severity: LintSeverity::Warning,
                    node_id: Some(anim.target_node_id.0.clone()),
                    message: format!(
                        "Conflicting writers for '{}.{}': both ParamBindingDef and EventBoundAnimation '{}' write this parameter.",
                        anim.target_node_id.0, anim.parameter, anim.id
                    ),
                    suggestion: "Keep a single parameter driver (either ParamBindingDef or EventBoundAnimation).".into(),
                });
            }
        }
    }

    for track in &project.keyframe_tracks {
        if let Some(node) = project.graph.find_node_recursive(&track.target_node)
            && !node.enabled
        {
            findings.push(LintFinding {
                code: "LNT-DAG-002".into(),
                severity: LintSeverity::Warning,
                node_id: Some(node.id.0.clone()),
                message: format!(
                    "KeyframeTrack '{}' targets disabled node '{}'.",
                    track.id, node.id.0
                ),
                suggestion: "Enable the target node or remove the unused keyframe track.".into(),
            });
        }
        if binding_targets.contains(&(track.target_node.0.as_str(), track.target_param.as_str())) {
            findings.push(LintFinding {
                code: "LNT-DAG-002".into(),
                severity: LintSeverity::Info,
                node_id: Some(track.target_node.0.clone()),
                message: format!(
                    "ParamBinding overrides KeyframeTrack '{}' on '{}.{}'.",
                    track.id, track.target_node.0, track.target_param
                ),
                suggestion: "Ensure the ParamBinding intentionally layers on top of the tracker keyframe track.".into(),
            });
        }
    }
}

fn lint_timing_and_assets(project: &Project, findings: &mut Vec<LintFinding>) {
    for ac in &project.timeline.audio_clips {
        if ac.anchor_event_id.is_none()
            && ac.timeline_end > project.timeline.duration_ticks
        {
            findings.push(LintFinding {
                code: "LNT-TIM-001".into(),
                severity: LintSeverity::Warning,
                node_id: None,
                message: format!(
                    "AudioClip '{}' ends at {} after timeline.duration_ticks ({}).",
                    ac.id, ac.timeline_end.0, project.timeline.duration_ticks.0
                ),
                suggestion: "Extend timeline.duration_ticks or trim AudioClip.timeline_end.".into(),
            });
        }
    }

    if project.timeline.clips.len() >= 2 {
        let mut sorted: Vec<_> = project.timeline.clips.iter().collect();
        sorted.sort_by_key(|c| (c.timeline_start.0, c.timeline_end.0));
        let mut covered_end = sorted[0].timeline_end;
        for clip in &sorted[1..] {
            if clip.timeline_start > covered_end {
                findings.push(LintFinding {
                    code: "LNT-TIM-001".into(),
                    severity: LintSeverity::Warning,
                    node_id: None,
                    message: format!(
                        "Uncovered timeline gap [{}, {}) before clip '{}' (risk of unintended black frames).",
                        covered_end.0, clip.timeline_start.0, clip.id
                    ),
                    suggestion: "Extend the preceding clip or shift timeline_start to close the gap.".into(),
                });
            }
            if clip.timeline_end > covered_end {
                covered_end = clip.timeline_end;
            }
        }
    }

    let resolver = MediaResolver::new();
    for asset in &project.assets {
        if matches!(asset.kind, AssetKind::Video | AssetKind::Audio)
            && asset.duration_ticks.is_none()
            && matches!(
                resolver.resolve_asset_source(asset),
                Ok(ResolvedAssetSource::Disk(_))
            )
        {
            findings.push(LintFinding {
                code: "LNT-MAT-001".into(),
                severity: LintSeverity::Warning,
                node_id: None,
                message: format!(
                    "Disk asset '{}' ('{}') has duration_ticks=None; clip bounds cannot be verified statically.",
                    asset.id, asset.path
                ),
                suggestion: "Populate AssetDef.duration_ticks from ffprobe metadata.".into(),
            });
        }
    }
}

fn lint_media_black_intervals(
    project: &Project,
    options: &LintOptions,
    findings: &mut Vec<LintFinding>,
) {
    let resolver = MediaResolver::new();
    for asset in &project.assets {
        if asset.kind != AssetKind::Video {
            continue;
        }
        let Ok(ResolvedAssetSource::Disk(path)) = resolver.resolve_asset_source(asset) else {
            continue;
        };
        if !path.is_file() {
            continue;
        }
        match detect_black_intervals(&path, options.black_min_duration_sec, options.black_pic_th) {
            Ok(intervals) => {
                if intervals.is_empty() {
                    continue;
                }
                let asset_clips: Vec<_> = project
                    .timeline
                    .clips
                    .iter()
                    .filter(|c| c.asset_id == asset.id)
                    .collect();
                for iv in &intervals {
                    let mut matched_clip = false;
                    for clip in &asset_clips {
                        let out_span = clip.timeline_end - clip.timeline_start;
                        let src_delta = clip.speed_curve.source_offset_at(out_span);
                        let src_start = clip.source_start.min(clip.source_start + src_delta);
                        let src_end = clip.source_start.max(clip.source_start + src_delta);
                        let overlap_start = iv.start_ticks.max(src_start);
                        let overlap_end = iv.end_ticks.min(src_end);
                        if overlap_end > overlap_start {
                            matched_clip = true;
                            let clip_len = (src_end.0 - src_start.0).max(1) as f64;
                            let overlap_len = (overlap_end.0 - overlap_start.0) as f64;
                            let sev = if overlap_len / clip_len >= 0.99 {
                                LintSeverity::Error
                            } else {
                                LintSeverity::Warning
                            };
                            findings.push(LintFinding {
                                code: "LNT-MED-001".into(),
                                severity: sev,
                                node_id: None,
                                message: format!(
                                    "Black frame interval [{:.3}s..{:.3}s] in '{}' overlaps clip '{}' source window.",
                                    iv.start_sec, iv.end_sec, asset.id, clip.id
                                ),
                                suggestion: format!(
                                    "Trim clip '{}' source_start past {:.3}s or split around the black interval.",
                                    clip.id, iv.end_sec
                                ),
                            });
                        }
                    }
                    if !matched_clip && asset_clips.is_empty() {
                        findings.push(LintFinding {
                            code: "LNT-MED-001".into(),
                            severity: LintSeverity::Info,
                            node_id: None,
                            message: format!(
                                "Asset '{}' contains black interval [{:.3}s..{:.3}s].",
                                asset.id, iv.start_sec, iv.end_sec
                            ),
                            suggestion: "Avoid sampling the black interval in new clips.".into(),
                        });
                    }
                }
            }
            Err(err) => {
                findings.push(LintFinding {
                    code: "LNT-MED-001".into(),
                    severity: LintSeverity::Warning,
                    node_id: None,
                    message: format!(
                        "blackdetect check failed for '{}' ({}): {err}",
                        asset.id,
                        path.display()
                    ),
                    suggestion: "Ensure ffmpeg is installed and the video stream is readable.".into(),
                });
            }
        }
    }
}

/// Quantitative & spatial pixel difference report returned by [`diff_frame`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrameDiffReport {
    pub width: u32,
    pub height: u32,
    pub pixel_count: u64,
    pub identical: bool,
    pub mae: f64,
    pub mae_rgba: [f64; 4],
    pub rmse: f64,
    pub psnr_db: Option<f64>,
    pub max_abs_diff: u8,
    pub threshold: u8,
    pub changed_pixels: u64,
    pub changed_pixel_ratio: f64,
    pub changed_bbox: Option<[u32; 4]>,
    pub quadrant_mae: [f64; 4],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub out_diff_png: Option<String>,
}

/// Compares two `Rgba8Frame` images with default per-channel threshold `2` and optionally writes
/// a 3-panel `[before | after | heatmap]` diagnostic PNG to `out_diff_png`.
pub fn diff_frame(
    a_rgba: &Rgba8Frame,
    b_rgba: &Rgba8Frame,
    out_diff_png: Option<&Path>,
) -> Result<FrameDiffReport, CoreError> {
    diff_frame_with_threshold(a_rgba, b_rgba, 2, out_diff_png)
}

/// Compares two PNG files on disk using [`diff_frame_with_threshold`].
pub fn diff_frame_files(
    path_a: &Path,
    path_b: &Path,
    threshold: u8,
    out_diff_png: Option<&Path>,
) -> Result<FrameDiffReport, CoreError> {
    let a = Rgba8Frame::load_png(path_a, Flicks::ZERO)?;
    let b = Rgba8Frame::load_png(path_b, Flicks::ZERO)?;
    diff_frame_with_threshold(&a, &b, threshold, out_diff_png)
}

/// Compares two `Rgba8Frame` images with an explicit changed-pixel threshold (`0..=255`).
pub fn diff_frame_with_threshold(
    a_rgba: &Rgba8Frame,
    b_rgba: &Rgba8Frame,
    threshold: u8,
    out_diff_png: Option<&Path>,
) -> Result<FrameDiffReport, CoreError> {
    if a_rgba.width != b_rgba.width || a_rgba.height != b_rgba.height {
        return Err(CoreError::InvalidArgument(format!(
            "diff_frame dimension mismatch: {}x{} vs {}x{}",
            a_rgba.width, a_rgba.height, b_rgba.width, b_rgba.height
        )));
    }
    let w = a_rgba.width;
    let h = a_rgba.height;
    let pixel_count = u64::from(w) * u64::from(h);
    if pixel_count == 0 {
        return Err(CoreError::InvalidArgument(
            "diff_frame requires non-zero frame dimensions".into(),
        ));
    }

    let mut sum_abs = [0u64; 4];
    let mut sum_sq_rgb = 0u64;
    let mut max_abs_diff = 0u8;
    let mut changed_pixels = 0u64;
    let mut min_x = w;
    let mut min_y = h;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    let mut quad_sum = [0u64; 4];
    let mut quad_cnt = [0u64; 4];
    let half_w = w / 2;
    let half_h = h / 2;

    for y in 0..h {
        for x in 0..w {
            let idx = ((y as usize) * (w as usize) + (x as usize)) * 4;
            let pa = &a_rgba.data[idx..idx + 4];
            let pb = &b_rgba.data[idx..idx + 4];
            let mut px_max = 0u8;
            let mut px_abs_sum = 0u64;
            for c in 0..4 {
                let d = pa[c].abs_diff(pb[c]);
                sum_abs[c] += u64::from(d);
                px_abs_sum += u64::from(d);
                px_max = px_max.max(d);
                if c < 3 {
                    let df = u64::from(d);
                    sum_sq_rgb += df * df;
                }
            }
            max_abs_diff = max_abs_diff.max(px_max);
            if px_max > threshold {
                changed_pixels += 1;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
            let q_idx = usize::from(y >= half_h) * 2 + usize::from(x >= half_w);
            quad_sum[q_idx] += px_abs_sum;
            quad_cnt[q_idx] += 4;
        }
    }

    let mae_rgba = sum_abs.map(|s| s as f64 / pixel_count as f64);
    let mae = (sum_abs.iter().sum::<u64>() as f64) / (4.0 * pixel_count as f64);
    let mse_rgb = sum_sq_rgb as f64 / (3.0 * pixel_count as f64);
    let rmse = mse_rgb.sqrt();
    let psnr_db = if mse_rgb == 0.0 {
        None
    } else {
        Some(10.0 * ((255.0_f64 * 255.0) / mse_rgb).log10())
    };
    let changed_pixel_ratio = changed_pixels as f64 / pixel_count as f64;
    let changed_bbox = if changed_pixels > 0 {
        Some([min_x, min_y, max_x - min_x + 1, max_y - min_y + 1])
    } else {
        None
    };
    let quadrant_mae = [0, 1, 2, 3].map(|i| {
        if quad_cnt[i] == 0 {
            0.0
        } else {
            quad_sum[i] as f64 / quad_cnt[i] as f64
        }
    });

    let out_diff_str = if let Some(path) = out_diff_png {
        let strip_w = w.checked_mul(3).ok_or_else(|| {
            CoreError::InvalidArgument("diff_frame strip width overflow".into())
        })?;
        let mut strip = Rgba8Frame::filled(
            strip_w,
            h,
            a_rgba.pts,
            [0, 0, 0, 255],
            AlphaMode::Straight,
        )?;
        for y in 0..h {
            for x in 0..w {
                let idx = ((y as usize) * (w as usize) + (x as usize)) * 4;
                let pa = [
                    a_rgba.data[idx],
                    a_rgba.data[idx + 1],
                    a_rgba.data[idx + 2],
                    255,
                ];
                let pb = [
                    b_rgba.data[idx],
                    b_rgba.data[idx + 1],
                    b_rgba.data[idx + 2],
                    255,
                ];
                strip.set_pixel(x, y, pa);
                strip.set_pixel(w + x, y, pb);

                let d_max = (0..4)
                    .map(|c| a_rgba.data[idx + c].abs_diff(b_rgba.data[idx + c]))
                    .max()
                    .unwrap_or(0);
                let amp = (u16::from(d_max) * 4).min(255) as u8;
                let heat = heatmap_rgb(amp);
                strip.set_pixel(2 * w + x, y, [heat[0], heat[1], heat[2], 255]);
            }
        }
        if let Some([bx, by, bw, bh]) = changed_bbox {
            let x0 = bx;
            let y0 = by;
            let x1 = (bx + bw).saturating_sub(1).min(w.saturating_sub(1));
            let y1 = (by + bh).saturating_sub(1).min(h.saturating_sub(1));
            for x in x0..=x1 {
                strip.set_pixel(2 * w + x, y0, [0, 255, 128, 255]);
                strip.set_pixel(2 * w + x, y1, [0, 255, 128, 255]);
            }
            for y in y0..=y1 {
                strip.set_pixel(2 * w + x0, y, [0, 255, 128, 255]);
                strip.set_pixel(2 * w + x1, y, [0, 255, 128, 255]);
            }
        }
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        strip.save_png(path)?;
        Some(path.to_string_lossy().into_owned())
    } else {
        None
    };

    Ok(FrameDiffReport {
        width: w,
        height: h,
        pixel_count,
        identical: max_abs_diff == 0,
        mae,
        mae_rgba,
        rmse,
        psnr_db,
        max_abs_diff,
        threshold,
        changed_pixels,
        changed_pixel_ratio,
        changed_bbox,
        quadrant_mae,
        out_diff_png: out_diff_str,
    })
}

fn heatmap_rgb(intensity: u8) -> [u8; 3] {
    let t = f32::from(intensity) / 255.0;
    if t <= 0.001 {
        [8, 10, 20]
    } else if t < 0.33 {
        let u = t / 0.33;
        [(30.0 * u) as u8, (140.0 * u) as u8, (180.0 + 75.0 * u) as u8]
    } else if t < 0.66 {
        let u = (t - 0.33) / 0.33;
        [(30.0 + 225.0 * u) as u8, (140.0 + 100.0 * u) as u8, (255.0 * (1.0 - u)) as u8]
    } else {
        let u = (t - 0.66) / 0.34;
        [255, (240.0 * (1.0 - 0.8 * u)) as u8, 20]
    }
}
