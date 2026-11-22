//! Headless product operations shared by command-line and programmatic callers.

use crate::{Engine, MediaResolver};
use frameshift_render::{NodeAnimator, ProjectAnimator};
use frameshift_schema::{
    AnimationEasing, AnimationKeyframe, AudioFadeDef, BlendMode, ColorSpace, EventBoundAnimation,
    GraphNodeDef, NodeId, NodeKind, ParamBindingDef, Project, ProjectCommand, RenderGraphDef,
    Resolution, SourceFit,
};
use frameshift_time::{FLICKS_PER_SECOND, FPS_24, Flicks, SpeedCurve};
use frameshift_vector::{FontQuery, resolve_font_chain};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

/// A self-contained three-shot edit using the built-in synthetic media generators.
pub fn impact_cut_project() -> Project {
    let seconds = |n| Flicks(n * FLICKS_PER_SECOND);
    let mut project = Project::golden_scene_preset();
    project.name = "Impact Cut".into();
    project.timeline.fps = FPS_24;
    project.timeline.duration_ticks = seconds(10);
    project.timeline.resolution = Resolution {
        width: 640,
        height: 360,
    };
    project.render_profiles.final_profile.resolution = project.timeline.resolution;
    project.render_profiles.draft.resolution = Resolution {
        width: 320,
        height: 180,
    };
    project.timeline.audio_clips[0].timeline_end = seconds(10);
    project.timeline.audio_clips[0].fade_out = Some(AudioFadeDef {
        duration_ticks: Flicks(FLICKS_PER_SECOND / 4),
        curve: Default::default(),
    });
    for (asset, path) in project.assets.iter_mut().zip([
        "synthetic://clip_a",
        "synthetic://clip_b",
        "synthetic://beat",
    ]) {
        asset.path = path.into();
    }

    let a = &mut project.timeline.clips[0];
    a.timeline_end = seconds(5);
    a.speed_curve = SpeedCurve::constant(1.0).expect("unit speed");
    let b = &mut project.timeline.clips[1];
    b.timeline_start = Flicks(7 * FLICKS_PER_SECOND / 2);
    b.timeline_end = seconds(7);
    let mut c = b.clone();
    c.id = "clip_c".into();
    c.asset_id = "asset_clip_a".into();
    c.timeline_start = seconds(7);
    c.timeline_end = seconds(10);
    c.source_start = seconds(5);
    c.linked_event_id = None;
    project.timeline.clips.push(c);

    let event = &mut project.events[0];
    event.target_output_tick = seconds(4);
    event.source_anchor_tick = Some(seconds(4));
    event.label = "Impact / beat".into();

    let old = project.graph;
    let take = |id: &str| {
        old.node(&NodeId::from(id))
            .expect("golden preset node")
            .clone()
    };
    let mut transition = take("whip_transition");
    transition.inputs[0] = NodeId::from("src_a");
    let mut impact_grade = take("grade_flash");
    impact_grade.inputs[0] = NodeId::from("impact_shake");
    if let NodeKind::ColorBasic { flash_amount, .. } = &mut impact_grade.kind {
        *flash_amount = 0.0;
    }
    let mut title = take("impact_title");
    if let NodeKind::VectorText { text, .. } = &mut title.kind {
        *text = "IMPACT CUT".into();
    }
    let node = |id, kind, inputs| GraphNodeDef {
        id: NodeId::from(id),
        kind,
        inputs,
        enabled: true,
    };
    project.graph = RenderGraphDef {
        output_node: NodeId::from("blend_title"),
        nodes: vec![
            take("src_a"),
            take("src_b"),
            transition,
            node(
                "src_c",
                NodeKind::SourceVideo {
                    asset_id: "asset_clip_a".into(),
                    clip_id: Some("clip_c".into()),
                    color_space: ColorSpace::DisplaySrgb,
                    fit: SourceFit::Stretch,
                },
                vec![],
            ),
            node(
                "picture",
                NodeKind::Blend {
                    mode: BlendMode::Normal,
                    opacity: 1.0,
                },
                vec![NodeId::from("whip_transition"), NodeId::from("src_c")],
            ),
            node(
                "impact_shake",
                NodeKind::MotionTileShake {
                    tile_scale: 1.0,
                    mirror_edges: true,
                    shake_amp_norm: [0.024, 0.015],
                    shake_rot_rad: 0.035,
                    shake_zoom: 0.04,
                    shake_freq_hz: 13.0,
                    shake_decay: 9.0,
                    event_delta_sec: -1.0,
                },
                vec![NodeId::from("picture")],
            ),
            impact_grade,
            title,
            take("blend_title"),
        ],
    };
    project.param_bindings = vec![
        ParamBindingDef {
            node_id: NodeId::from("whip_transition"),
            param_name: "progress".into(),
            expression: "smoothstep(-0.25, 0.25, event_delta_sec)".into(),
        },
        ParamBindingDef {
            node_id: NodeId::from("impact_shake"),
            param_name: "event_delta_sec".into(),
            expression: "event_delta_sec".into(),
        },
    ];
    project.timeline.clips[0]
        .event_animations
        .push(EventBoundAnimation {
            id: "impact_title_reveal".into(),
            event_id: "drop_impact".into(),
            target_node_id: NodeId::from("impact_title"),
            parameter: "opacity".into(),
            offset_from_event: Flicks::ZERO,
            duration: Flicks(FLICKS_PER_SECOND / 4),
            keyframes: vec![
                AnimationKeyframe {
                    progress: 0.0,
                    value: 0.0,
                    easing: AnimationEasing::EaseOut,
                },
                AnimationKeyframe {
                    progress: 1.0,
                    value: 1.0,
                    easing: AnimationEasing::Linear,
                },
            ],
            hold_after: true,
        });
    if let Some(title) = project
        .graph
        .nodes
        .iter_mut()
        .find(|node| node.id == NodeId::from("impact_title"))
        && let NodeKind::VectorText { opacity, .. } = &mut title.kind
    {
        *opacity = 0.0;
    }
    project.creative_decisions.clear();
    project
        .validate()
        .expect("Impact Cut fixture must validate");
    project
}

#[derive(Debug, Serialize)]
pub struct PreflightIssue {
    pub code: &'static str,
    pub object_id: Option<String>,
    pub references: Vec<String>,
    pub tick: Option<Flicks>,
    pub message: String,
    pub action: &'static str,
}

#[derive(Debug, Serialize)]
pub struct PreflightReport {
    pub project_revision: Option<u64>,
    pub ok: bool,
    pub tools_checked: bool,
    pub issues: Vec<PreflightIssue>,
}

impl PreflightReport {
    pub fn invalid_project(message: String) -> Self {
        Self {
            project_revision: None,
            ok: false,
            tools_checked: false,
            issues: vec![issue(
                "InvalidProject",
                message,
                "Check JSON and project schema.",
            )],
        }
    }
}

fn issue(code: &'static str, message: String, action: &'static str) -> PreflightIssue {
    PreflightIssue {
        code,
        object_id: None,
        references: Vec::new(),
        tick: None,
        message,
        action,
    }
}

pub fn preflight(project: &Project, check_tools: bool) -> PreflightReport {
    if let Err(error) = project.validate() {
        return PreflightReport::invalid_project(error.to_string());
    }
    let mut issues = Vec::new();
    let resolver = MediaResolver::new();
    let mut references: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    for clip in &project.timeline.clips {
        references
            .entry(&clip.asset_id)
            .or_default()
            .insert(clip.id.clone());
    }
    for clip in &project.timeline.audio_clips {
        references
            .entry(&clip.asset_id)
            .or_default()
            .insert(clip.id.clone());
    }
    inspect_graph(&project.graph, &mut references, &mut issues);
    for (id, refs) in references {
        if let Some(asset) = project.asset(id)
            && let Err(error) = resolver.resolve_asset_source(asset)
        {
            issues.push(PreflightIssue {
                object_id: Some(id.to_owned()),
                references: refs.into_iter().collect(),
                ..issue(
                    "MissingAsset",
                    error.to_string(),
                    "Relink this asset to its media file.",
                )
            });
        }
    }
    for event in &project.events {
        let target_valid = event.target_output_tick >= Flicks::ZERO
            && event.target_output_tick < project.timeline.duration_ticks;
        let valid = target_valid
            && match (&event.source_clip_id, event.source_anchor_tick) {
                (None, None) => true,
                (Some(id), Some(source)) => project.clip(id).is_some_and(|clip| {
                    let end = clip.source_start.saturating_add(
                        clip.speed_curve
                            .source_offset_at(clip.timeline_end - clip.timeline_start),
                    );
                    source >= clip.source_start.min(end)
                        && source <= clip.source_start.max(end)
                        && event.target_output_tick >= clip.timeline_start
                        && event.target_output_tick < clip.timeline_end
                }),
                _ => false,
            };
        if !valid {
            issues.push(PreflightIssue {
                object_id: Some(event.event_id.clone()),
                references: event.source_clip_id.iter().cloned().collect(),
                tick: Some(event.target_output_tick),
                ..issue(
                    "UnresolvedAnchor",
                    "Event has an invalid source/target pair.".into(),
                    "Place the target inside the clip and select a reachable source tick.",
                )
            });
        }
    }
    if check_tools {
        for (program, code) in [
            ("ffmpeg", "FfmpegUnavailable"),
            ("ffprobe", "FfprobeUnavailable"),
        ] {
            if !Command::new(program)
                .arg("-version")
                .output()
                .is_ok_and(|out| out.status.success())
            {
                issues.push(issue(
                    code,
                    format!("{program} is unavailable."),
                    "Install FFmpeg and add it to PATH.",
                ));
            }
        }
        if let Err(error) = Engine::new_headless() {
            issues.push(issue(
                "GpuUnavailable",
                error.to_string(),
                "Select a working wgpu adapter or software Vulkan driver.",
            ));
        }
    }
    PreflightReport {
        project_revision: Some(project.revision_id),
        ok: issues.is_empty(),
        tools_checked: check_tools,
        issues,
    }
}

fn inspect_graph<'a>(
    graph: &'a RenderGraphDef,
    references: &mut BTreeMap<&'a str, BTreeSet<String>>,
    issues: &mut Vec<PreflightIssue>,
) {
    for node in &graph.nodes {
        match &node.kind {
            NodeKind::SourceVideo { asset_id, .. } | NodeKind::SourceImage { asset_id, .. } => {
                references
                    .entry(asset_id)
                    .or_default()
                    .insert(node.id.0.clone());
            }
            NodeKind::VectorText {
                font_path,
                font_family,
                font_weight,
                fallback_fonts,
                ..
            } if font_path.is_some() || font_family.is_some() || !fallback_fonts.is_empty() => {
                let query = FontQuery {
                    font_path: font_path.clone(),
                    font_family: font_family.clone(),
                    font_weight: *font_weight,
                };
                if let Err(error) = resolve_font_chain(&query, fallback_fonts, None) {
                    issues.push(PreflightIssue {
                        object_id: Some(node.id.0.clone()),
                        ..issue(
                            "MissingFont",
                            error.to_string(),
                            "Provide an available font family or font file.",
                        )
                    });
                }
            }
            NodeKind::Lut3D {
                lut_source: Some(source),
                ..
            } => {
                let path = match source {
                    frameshift_schema::LutSourceDef::CubeFile { path, .. } => Some(path),
                    frameshift_schema::LutSourceDef::Ocio { config_path, .. } => Some(config_path),
                    _ => None,
                };
                if let Some(path) = path.filter(|path| !Path::new(path).is_file()) {
                    issues.push(PreflightIssue {
                        object_id: Some(node.id.0.clone()),
                        ..issue(
                            "MissingLut",
                            path.clone(),
                            "Relink the LUT to its .cube file.",
                        )
                    });
                }
            }
            NodeKind::Precomp { sub_graph, .. } => inspect_graph(sub_graph, references, issues),
            _ => {}
        }
    }
}

#[derive(Debug, Serialize)]
pub struct EffectPreset {
    pub name: &'static str,
    pub description: &'static str,
    pub input_compatibility: &'static str,
    pub parameters: Vec<EffectParameterDescriptor>,
    pub node: NodeKind,
}

#[derive(Debug, Serialize)]
pub struct EffectParameterDescriptor {
    pub name: &'static str,
    pub unit: &'static str,
    pub suggested_min: f64,
    pub suggested_max: f64,
}

pub fn effect_presets() -> Vec<EffectPreset> {
    vec![
        EffectPreset {
            name: "impact-flash",
            description: "Short bright impact using the existing ColorBasic node.",
            input_compatibility: "One image input; can be appended to any image output chain.",
            parameters: vec![
                EffectParameterDescriptor {
                    name: "exposure",
                    unit: "stops (EV)",
                    suggested_min: -2.0,
                    suggested_max: 2.0,
                },
                EffectParameterDescriptor {
                    name: "contrast",
                    unit: "multiplier",
                    suggested_min: 0.5,
                    suggested_max: 2.0,
                },
                EffectParameterDescriptor {
                    name: "saturation",
                    unit: "multiplier",
                    suggested_min: 0.0,
                    suggested_max: 2.0,
                },
                EffectParameterDescriptor {
                    name: "flash_amount",
                    unit: "normalized (0–1)",
                    suggested_min: 0.0,
                    suggested_max: 1.0,
                },
            ],
            node: NodeKind::ColorBasic {
                exposure: 0.0,
                contrast: 1.04,
                saturation: 1.12,
                flash_amount: 0.35,
            },
        },
        EffectPreset {
            name: "cyber-contrast",
            description: "Built-in anime contrast LUT.",
            input_compatibility: "One image input; can be appended to any image output chain.",
            parameters: vec![EffectParameterDescriptor {
                name: "intensity",
                unit: "normalized (0–1)",
                suggested_min: 0.0,
                suggested_max: 1.0,
            }],
            node: NodeKind::Lut3D {
                input: NodeId::from("previous_output"),
                lut_preset: "anime_cyber_contrast".into(),
                intensity: 0.65,
                lut_source: None,
                input_space: None,
                output_space: None,
            },
        },
    ]
}

pub fn apply_effect(project: &Project, id: NodeId, preset: &str) -> Option<ProjectCommand> {
    let mut node = effect_presets()
        .into_iter()
        .find(|effect| effect.name == preset)?
        .node;
    let mut candidate = project.clone();
    let previous = std::mem::replace(&mut candidate.graph.output_node, id.clone());
    if let NodeKind::Lut3D { input, .. } = &mut node {
        *input = previous.clone();
    }
    candidate.graph.nodes.push(GraphNodeDef {
        id,
        kind: node,
        inputs: vec![previous],
        enabled: true,
    });
    Some(ProjectCommand::ReplaceProject { project: candidate })
}

/// Evaluates a node with the same animation and audio envelope logic as picture rendering.
pub fn inspect_effect(
    project: &Project,
    node_id: &NodeId,
    tick: Flicks,
) -> Result<Option<serde_json::Value>, crate::CoreError> {
    let Some(node) = project.graph.find_node_recursive(node_id) else {
        return Ok(None);
    };
    let mut cache = None;
    let resolver = MediaResolver::new();
    let envelope = crate::engine::resolve_disk_audio_envelope(&mut cache, &resolver, project)?;
    let animated = ProjectAnimator::with_audio_envelope(project, envelope)?
        .evaluate_node(node_id, &node.kind, tick)?;
    let bindings: Vec<_> = project
        .param_bindings
        .iter()
        .filter(|b| &b.node_id == node_id)
        .collect();
    let target_event_ids: BTreeSet<_> = project
        .timeline
        .clips
        .iter()
        .flat_map(|clip| &clip.event_animations)
        .filter(|animation| &animation.target_node_id == node_id)
        .map(|animation| animation.event_id.clone())
        .collect();
    let preset_metadata = effect_presets()
        .into_iter()
        .find(|preset| match (&preset.node, &node.kind) {
            (NodeKind::ColorBasic { .. }, NodeKind::ColorBasic { .. }) => true,
            (
                NodeKind::Lut3D {
                    lut_preset: preset_name,
                    ..
                },
                NodeKind::Lut3D {
                    lut_preset: node_name,
                    ..
                },
            ) => preset_name == node_name,
            _ => false,
        })
        .map(|preset| {
            serde_json::json!({
                "name": preset.name,
                "description": preset.description,
                "input_compatibility": preset.input_compatibility,
                "parameters": preset.parameters,
            })
        });
    Ok(Some(serde_json::json!({
        "revision_id": project.revision_id,
        "output_tick": tick,
        "node_id": node_id,
        "enabled": node.enabled,
        "base": node.kind,
        "evaluated": animated,
        "bindings": bindings,
        "preset_metadata": preset_metadata,
        "target_event_ids": target_event_ids,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn impact_cut_is_self_contained() {
        let project = impact_cut_project();
        assert_eq!(project.timeline.clips.len(), 3);
        assert_eq!(project.timeline.audio_clips.len(), 1);
        assert_eq!(project.events.len(), 1);
        assert!(preflight(&project, false).ok);
    }

    #[test]
    fn missing_media_and_anchor_have_separate_codes() {
        let mut project = impact_cut_project();
        let mut missing = project.assets[0].clone();
        missing.id = "missing_video".into();
        missing.path = "unavailable.mp4".into();
        project.assets.push(missing);
        project.timeline.clips[0].asset_id = "missing_video".into();
        project.events[0].source_anchor_tick = None;
        let report = preflight(&project, false);
        assert!(
            report
                .issues
                .iter()
                .any(|i| i.code == "MissingAsset" && i.references == ["clip_a"])
        );
        assert!(report.issues.iter().any(|i| i.code == "UnresolvedAnchor"));
    }

    #[test]
    fn effect_presets_serialize_parameter_and_input_guidance() {
        let descriptors = serde_json::to_value(effect_presets()).expect("serialize presets");
        assert_eq!(
            descriptors[0]["input_compatibility"],
            "One image input; can be appended to any image output chain."
        );
        assert_eq!(descriptors[0]["parameters"][0]["name"], "exposure");
        assert_eq!(descriptors[0]["parameters"][0]["unit"], "stops (EV)");
        assert_eq!(descriptors[1]["parameters"][0]["suggested_max"], 1.0);
    }

    #[test]
    fn effect_inspection_includes_matching_preset_and_target_events() {
        let project = impact_cut_project();
        let report = inspect_effect(
            &project,
            &NodeId::from("grade_flash"),
            Flicks(4 * FLICKS_PER_SECOND),
        )
        .expect("inspect effect")
        .expect("known node");
        assert_eq!(report["preset_metadata"]["name"], "impact-flash");
        assert_eq!(
            report["target_event_ids"],
            serde_json::json!(["drop_impact"])
        );
    }
}
