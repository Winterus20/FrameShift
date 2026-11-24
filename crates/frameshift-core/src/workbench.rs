//! Headless product operations shared by command-line and programmatic callers.

use crate::{CoreError, Engine, MediaResolver};
use frameshift_render::{NodeAnimator, ProjectAnimator};
use frameshift_schema::template::{
    EffectChainPreset, TemplateReview, TemplateSlot, TemplateSlotKind, TemplateSlotTarget,
    TemplateSlotValue, validate_effect_chain_preset,
};
use frameshift_schema::{
    AnimationEasing, AnimationKeyframe, AudioFadeDef, BevelProfile3D, BlendMode,
    CURRENT_SCHEMA_VERSION, Camera3DMode, ColorSpace, EventBoundAnimation, GraphNodeDef,
    HalftoneColorMode, MatteMode, NodeId, NodeKind, ParamBindingDef, Project, ProjectCommand,
    RenderGraphDef, Resolution, SampleFilter, SchemaError, Shape3DKind, ShutterPhase, SourceFit,
    TextAlign2D, WritingMode2D,
};
use frameshift_time::{FLICKS_PER_SECOND, FPS_24, Flicks, SpeedCurve};
use frameshift_vector::{FontQuery, resolve_font_chain};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::collections::{BTreeMap, BTreeSet, HashMap};
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

/// Returns built-in, schema-validated [`EffectChainPreset`] fragments for anime/AMV workflows.
pub fn builtin_effect_chain_presets() -> Vec<EffectChainPreset> {
    vec![
        EffectChainPreset {
            schema_version: CURRENT_SCHEMA_VERSION,
            id: "xenoz_impact_glow".into(),
            name: "Xenoz Impact Glow".into(),
            category: "impact".into(),
            nodes: vec![
                GraphNodeDef {
                    id: NodeId::from("in_0"),
                    kind: NodeKind::PrecompInput { slot: 0 },
                    inputs: Vec::new(),
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("blur_mocurves"),
                    kind: NodeKind::BlurMoCurves {
                        shift_x: 0.04,
                        shift_y: 0.0,
                        z_dist: -0.08,
                        rotate_deg: 2.0,
                        blur_strength: 1.0,
                        samples: 16,
                        wrap_mode: 2,
                    },
                    inputs: vec![NodeId::from("in_0")],
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("deep_glow"),
                    kind: NodeKind::DeepGlow {
                        threshold: 0.72,
                        knee: 0.5,
                        intensity: 0.85,
                        radius: 1.2,
                        tint_rgb: [1.0, 0.75, 0.9],
                        mip_levels: 5,
                        scatter: 0.65,
                        anamorphic_ratio: 0.0,
                        chromatic_shift: 0.015,
                    },
                    inputs: vec![NodeId::from("blur_mocurves")],
                    enabled: true,
                },
            ],
            param_bindings: vec![ParamBindingDef {
                node_id: NodeId::from("deep_glow"),
                param_name: "intensity".into(),
                expression: "clamp(0.45 + audio_rms * 0.9, 0.0, 2.0)".into(),
            }],
            slots: vec![
                TemplateSlot {
                    id: "glow_tint".into(),
                    label: "Glow Tint".into(),
                    kind: TemplateSlotKind::Color,
                    target: TemplateSlotTarget::NodeColor {
                        node_id: NodeId::from("deep_glow"),
                        field: Some("tint_rgb".into()),
                    },
                    required: false,
                    default: Some(TemplateSlotValue::Color([1.0, 0.75, 0.9, 1.0])),
                },
                TemplateSlot {
                    id: "glow_intensity".into(),
                    label: "Glow Intensity".into(),
                    kind: TemplateSlotKind::Number,
                    target: TemplateSlotTarget::ParameterBinding {
                        node_id: NodeId::from("deep_glow"),
                        param_name: "intensity".into(),
                    },
                    required: false,
                    default: None,
                },
            ],
            review: TemplateReview {
                reviewed: true,
                reviewer: Some("FrameShift built-in".into()),
                note: Some("Directional BlurMoCurves + HDR DeepGlow impact chain".into()),
            },
        },
        EffectChainPreset {
            schema_version: CURRENT_SCHEMA_VERSION,
            id: "anime_impact_shake".into(),
            name: "Anime Impact Shake".into(),
            category: "impact".into(),
            nodes: vec![
                GraphNodeDef {
                    id: NodeId::from("in_0"),
                    kind: NodeKind::PrecompInput { slot: 0 },
                    inputs: Vec::new(),
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("shake"),
                    kind: NodeKind::MotionTileShake {
                        tile_scale: 1.0,
                        mirror_edges: true,
                        shake_amp_norm: [0.025, 0.018],
                        shake_rot_rad: 0.03,
                        shake_zoom: 0.04,
                        shake_freq_hz: 14.0,
                        shake_decay: 8.0,
                        event_delta_sec: 0.0,
                    },
                    inputs: vec![NodeId::from("in_0")],
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("impact_frame"),
                    kind: NodeKind::AnimeImpactFrame {
                        luma_threshold: 0.5,
                        edge_weight: 1.0,
                        invert_amount: 0.85,
                        speed_line_density: 0.6,
                        speed_line_intensity: 0.5,
                        center_uv: [0.5, 0.5],
                        mix_amount: 0.45,
                    },
                    inputs: vec![NodeId::from("shake")],
                    enabled: true,
                },
            ],
            param_bindings: vec![ParamBindingDef {
                node_id: NodeId::from("shake"),
                param_name: "event_delta_sec".into(),
                expression: "event_delta_sec".into(),
            }],
            slots: vec![TemplateSlot {
                id: "impact_mix".into(),
                label: "Impact Mix".into(),
                kind: TemplateSlotKind::Number,
                target: TemplateSlotTarget::ParameterBinding {
                    node_id: NodeId::from("impact_frame"),
                    param_name: "mix_amount".into(),
                },
                required: false,
                default: None,
            }],
            review: TemplateReview {
                reviewed: true,
                reviewer: Some("FrameShift built-in".into()),
                note: Some("MotionTileShake + AnimeImpactFrame punch".into()),
            },
        },
        EffectChainPreset {
            schema_version: CURRENT_SCHEMA_VERSION,
            id: "impact_flash".into(),
            name: "Impact Flash".into(),
            category: "color".into(),
            nodes: vec![
                GraphNodeDef {
                    id: NodeId::from("in_0"),
                    kind: NodeKind::PrecompInput { slot: 0 },
                    inputs: Vec::new(),
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("flash"),
                    kind: NodeKind::ColorBasic {
                        exposure: 0.0,
                        contrast: 1.04,
                        saturation: 1.12,
                        flash_amount: 0.35,
                    },
                    inputs: vec![NodeId::from("in_0")],
                    enabled: true,
                },
            ],
            param_bindings: Vec::new(),
            slots: vec![TemplateSlot {
                id: "flash_amount".into(),
                label: "Flash Amount".into(),
                kind: TemplateSlotKind::Number,
                target: TemplateSlotTarget::ParameterBinding {
                    node_id: NodeId::from("flash"),
                    param_name: "flash_amount".into(),
                },
                required: false,
                default: None,
            }],
            review: TemplateReview {
                reviewed: true,
                reviewer: Some("FrameShift built-in".into()),
                note: None,
            },
        },
        EffectChainPreset {
            schema_version: CURRENT_SCHEMA_VERSION,
            id: "cyber_contrast".into(),
            name: "Cyber Contrast LUT".into(),
            category: "color".into(),
            nodes: vec![
                GraphNodeDef {
                    id: NodeId::from("in_0"),
                    kind: NodeKind::PrecompInput { slot: 0 },
                    inputs: Vec::new(),
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("lut"),
                    kind: NodeKind::Lut3D {
                        input: NodeId::from("in_0"),
                        lut_preset: "anime_cyber_contrast".into(),
                        intensity: 0.65,
                        lut_source: None,
                        input_space: None,
                        output_space: None,
                    },
                    inputs: vec![NodeId::from("in_0")],
                    enabled: true,
                },
            ],
            param_bindings: Vec::new(),
            slots: vec![TemplateSlot {
                id: "intensity".into(),
                label: "LUT Intensity".into(),
                kind: TemplateSlotKind::Number,
                target: TemplateSlotTarget::ParameterBinding {
                    node_id: NodeId::from("lut"),
                    param_name: "intensity".into(),
                },
                required: false,
                default: None,
            }],
            review: TemplateReview {
                reviewed: true,
                reviewer: Some("FrameShift built-in".into()),
                note: None,
            },
        },
        EffectChainPreset {
            schema_version: CURRENT_SCHEMA_VERSION,
            id: "rsmb_anime_motion".into(),
            name: "RSMB Anime Optical Flow Blur".into(),
            category: "motion".into(),
            nodes: vec![
                GraphNodeDef {
                    id: NodeId::from("in_0"),
                    kind: NodeKind::PrecompInput { slot: 0 },
                    inputs: Vec::new(),
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("of_blur"),
                    kind: NodeKind::OpticalFlowBlur {
                        shutter_amount: 1.25,
                        samples: 12,
                        max_blur_px: 48.0,
                        sensitivity: 0.35,
                        flow_smoothness: 0.25,
                        visualize_flow: false,
                    },
                    inputs: vec![NodeId::from("in_0")],
                    enabled: true,
                },
            ],
            param_bindings: Vec::new(),
            slots: vec![TemplateSlot {
                id: "shutter_amount".into(),
                label: "Shutter Amount".into(),
                kind: TemplateSlotKind::Number,
                target: TemplateSlotTarget::ParameterBinding {
                    node_id: NodeId::from("of_blur"),
                    param_name: "shutter_amount".into(),
                },
                required: false,
                default: None,
            }],
            review: TemplateReview {
                reviewed: true,
                reviewer: Some("FrameShift built-in".into()),
                note: Some("Pyramidal GPU optical flow motion blur".into()),
            },
        },
    ]
}

/// Looks up a built-in [`EffectChainPreset`] by ID (accepting `-` or `_` separators).
pub fn find_effect_chain_preset(preset_id: &str) -> Option<EffectChainPreset> {
    let normalized = preset_id.trim().to_ascii_lowercase().replace(['-', ' '], "_");
    builtin_effect_chain_presets()
        .into_iter()
        .find(|p| p.id == preset_id || p.id.to_ascii_lowercase() == normalized)
}

/// Validates an [`EffectChainPreset`], applies optional typed `slot_values`, and splices
/// its nodes and bindings into `project.graph` after `target_node_id` (default `output_node`).
pub fn splice_effect_chain_preset(
    project: &Project,
    target_node_id: Option<&NodeId>,
    node_id_prefix: &str,
    preset: &EffectChainPreset,
    slot_values: Option<&BTreeMap<String, TemplateSlotValue>>,
) -> Result<ProjectCommand, CoreError> {
    validate_effect_chain_preset(preset)?;
    let mut preset = preset.clone();
    apply_preset_slot_values(&mut preset, slot_values)?;

    let target_id = target_node_id
        .cloned()
        .unwrap_or_else(|| project.graph.output_node.clone());
    if project.graph.node(&target_id).is_none() {
        return Err(SchemaError::MissingNode(target_id).into());
    }

    let mut precomp_inputs = BTreeSet::new();
    let mut consumed = BTreeSet::new();
    for node in &preset.nodes {
        if let NodeKind::PrecompInput { slot } = node.kind {
            if slot != 0 {
                return Err(CoreError::InvalidArgument(format!(
                    "effect chain preset '{}' uses PrecompInput slot {slot}; only slot 0 is supported",
                    preset.id
                )));
            }
            precomp_inputs.insert(node.id.clone());
        }
        for up in &node.inputs {
            consumed.insert(up.clone());
        }
        if let NodeKind::Lut3D { input, .. } = &node.kind {
            consumed.insert(input.clone());
        }
    }
    if precomp_inputs.is_empty() && target_node_id.is_some() {
        return Err(CoreError::InvalidArgument(format!(
            "effect chain preset '{}' must contain a PrecompInput(0) node to splice after '{target_id}'",
            preset.id
        )));
    }

    let sink_id = preset
        .nodes
        .iter()
        .find(|n| !consumed.contains(&n.id))
        .map(|n| n.id.clone())
        .ok_or_else(|| {
            CoreError::InvalidArgument(format!(
                "effect chain preset '{}' has no unique output node",
                preset.id
            ))
        })?;
    if precomp_inputs.contains(&sink_id) {
        return Err(CoreError::InvalidArgument(format!(
            "effect chain preset '{}' cannot have PrecompInput as its sole output",
            preset.id
        )));
    }

    let mut candidate = project.clone();
    let clean_prefix = if node_id_prefix.trim().is_empty() {
        preset.id.as_str()
    } else {
        node_id_prefix.trim()
    };

    let mut id_map: HashMap<NodeId, NodeId> = HashMap::new();
    let mut reserved: BTreeSet<NodeId> = candidate.graph.nodes.iter().map(|n| n.id.clone()).collect();
    for node in &preset.nodes {
        if precomp_inputs.contains(&node.id) {
            continue;
        }
        let base = format!("{clean_prefix}_{}", node.id.0);
        let mapped = unique_node_id(&reserved, &base);
        reserved.insert(mapped.clone());
        id_map.insert(node.id.clone(), mapped);
    }

    let mapped_sink = id_map.get(&sink_id).cloned().ok_or_else(|| {
        CoreError::InvalidArgument("preset output node missing from mapped graph".into())
    })?;

    // Rewire any existing downstream consumers of `target_id` before appending the spliced chain.
    for existing in &mut candidate.graph.nodes {
        for input in &mut existing.inputs {
            if *input == target_id {
                *input = mapped_sink.clone();
            }
        }
        if let NodeKind::Lut3D { input, .. } = &mut existing.kind
            && *input == target_id
        {
            *input = mapped_sink.clone();
        }
    }
    if candidate.graph.output_node == target_id {
        candidate.graph.output_node = mapped_sink;
    }

    let rewrite_ref = |id: &NodeId| -> NodeId {
        if precomp_inputs.contains(id) {
            target_id.clone()
        } else {
            id_map.get(id).cloned().unwrap_or_else(|| id.clone())
        }
    };

    for mut node in preset.nodes {
        if precomp_inputs.contains(&node.id) {
            continue;
        }
        node.id = id_map[&node.id].clone();
        for input in &mut node.inputs {
            *input = rewrite_ref(input);
        }
        match &mut node.kind {
            NodeKind::Lut3D { input, .. } => *input = rewrite_ref(input),
            NodeKind::Transform3D { parent, .. } | NodeKind::Null3D { parent, .. } => {
                if let Some(p) = parent {
                    *p = rewrite_ref(p);
                }
            }
            _ => {}
        }
        candidate.graph.nodes.push(node);
    }

    for mut binding in preset.param_bindings {
        if precomp_inputs.contains(&binding.node_id) {
            continue;
        }
        binding.node_id = id_map[&binding.node_id].clone();
        if let Some(existing) = candidate
            .param_bindings
            .iter_mut()
            .find(|b| b.node_id == binding.node_id && b.param_name == binding.param_name)
        {
            existing.expression = binding.expression;
        } else {
            candidate.param_bindings.push(binding);
        }
    }

    candidate.validate()?;
    Ok(ProjectCommand::ReplaceProject { project: candidate })
}

fn unique_node_id(reserved: &BTreeSet<NodeId>, base: &str) -> NodeId {
    let mut cand = NodeId::new(base);
    let mut suffix = 1u64;
    while reserved.contains(&cand) {
        cand = NodeId::new(format!("{base}_{suffix}"));
        suffix += 1;
    }
    cand
}

fn apply_preset_slot_values(
    preset: &mut EffectChainPreset,
    slot_values: Option<&BTreeMap<String, TemplateSlotValue>>,
) -> Result<(), CoreError> {
    if let Some(values) = slot_values {
        for supplied in values.keys() {
            if !preset.slots.iter().any(|s| s.id == *supplied) {
                return Err(SchemaError::UnknownTemplateSlot(supplied.clone()).into());
            }
        }
    }

    let slots = preset.slots.clone();
    for slot in &slots {
        let value = slot_values
            .and_then(|values| values.get(&slot.id))
            .or(slot.default.as_ref());
        let Some(value) = value else {
            if slot.required {
                return Err(SchemaError::MissingTemplateSlot(slot.id.clone()).into());
            }
            continue;
        };
        apply_one_preset_slot(preset, slot, value)?;
    }
    Ok(())
}

fn apply_one_preset_slot(
    preset: &mut EffectChainPreset,
    slot: &TemplateSlot,
    value: &TemplateSlotValue,
) -> Result<(), CoreError> {
    let fail = |message: String| -> CoreError {
        SchemaError::InvalidTemplateSlot {
            slot_id: slot.id.clone(),
            message,
        }
        .into()
    };
    match (slot.kind, &slot.target, value) {
        (
            TemplateSlotKind::Text,
            TemplateSlotTarget::NodeText { node_id },
            TemplateSlotValue::Text(text),
        ) => {
            let node = find_node_mut_in_slice(&mut preset.nodes, node_id)
                .ok_or_else(|| fail(format!("node '{node_id}' does not exist")))?;
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
        (
            TemplateSlotKind::Font,
            TemplateSlotTarget::NodeFont { node_id },
            TemplateSlotValue::Font(font),
        ) if !font.trim().is_empty() => {
            let node = find_node_mut_in_slice(&mut preset.nodes, node_id)
                .ok_or_else(|| fail(format!("node '{node_id}' does not exist")))?;
            match &mut node.kind {
                NodeKind::Text3D { font_path, .. } => *font_path = font.clone(),
                NodeKind::VectorText {
                    font_family,
                    font_path,
                    ..
                } => {
                    let lower = font.to_ascii_lowercase();
                    if font.contains('/')
                        || font.contains('\\')
                        || lower.ends_with(".ttf")
                        || lower.ends_with(".otf")
                        || lower.ends_with(".ttc")
                    {
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
        (
            TemplateSlotKind::Boolean,
            TemplateSlotTarget::NodeEnabled { node_id },
            TemplateSlotValue::Boolean(enabled),
        ) => {
            let node = find_node_mut_in_slice(&mut preset.nodes, node_id)
                .ok_or_else(|| fail(format!("node '{node_id}' does not exist")))?;
            node.enabled = *enabled;
        }
        (
            TemplateSlotKind::Number,
            TemplateSlotTarget::ParameterBinding {
                node_id,
                param_name,
            },
            TemplateSlotValue::Number(num),
        ) if num.is_finite() => {
            let expression = num.to_string();
            if let Some(binding) = preset
                .param_bindings
                .iter_mut()
                .find(|b| b.node_id == *node_id && b.param_name == *param_name)
            {
                binding.expression = expression;
            } else {
                preset.param_bindings.push(ParamBindingDef {
                    node_id: node_id.clone(),
                    param_name: param_name.clone(),
                    expression,
                });
            }
        }
        (
            TemplateSlotKind::Color,
            TemplateSlotTarget::NodeColor { node_id, field },
            TemplateSlotValue::Color(rgba),
        ) if rgba.iter().all(|c| c.is_finite() && *c >= 0.0) && rgba[3] <= 1.0 => {
            let node = find_node_mut_in_slice(&mut preset.nodes, node_id)
                .ok_or_else(|| fail(format!("node '{node_id}' does not exist")))?;
            apply_preset_node_color(&mut node.kind, field.as_deref(), *rgba).map_err(fail)?;
        }
        (
            TemplateSlotKind::Choice,
            TemplateSlotTarget::NodeChoice { node_id, field },
            TemplateSlotValue::Choice(choice),
        ) if !choice.trim().is_empty() => {
            let node = find_node_mut_in_slice(&mut preset.nodes, node_id)
                .ok_or_else(|| fail(format!("node '{node_id}' does not exist")))?;
            apply_preset_node_choice(&mut node.kind, field, choice).map_err(fail)?;
        }
        _ => {
            return Err(fail(format!(
                "slot value {value:?} is incompatible with kind {:?} / target {:?}",
                slot.kind, slot.target
            )));
        }
    }
    Ok(())
}

fn find_node_mut_in_slice<'a>(
    nodes: &'a mut [GraphNodeDef],
    id: &NodeId,
) -> Option<&'a mut GraphNodeDef> {
    for node in nodes {
        if &node.id == id {
            return Some(node);
        }
        if let NodeKind::Precomp { sub_graph, .. } = &mut node.kind
            && let Some(found) = find_node_mut_in_slice(&mut sub_graph.nodes, id)
        {
            return Some(found);
        }
    }
    None
}

fn apply_preset_node_color(
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

fn parse_snake_enum<T: DeserializeOwned>(raw: &str, field: &str) -> Result<T, String> {
    serde_json::from_value::<T>(serde_json::Value::String(raw.to_owned()))
        .map_err(|_| format!("invalid choice '{raw}' for field '{field}'"))
}

fn apply_preset_node_choice(kind: &mut NodeKind, field: &str, choice: &str) -> Result<(), String> {
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
            *shader_name = choice.to_owned();
        }
        (NodeKind::Lut3D { lut_preset, .. }, "lut_preset") => {
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

    #[test]
    fn builtin_effect_chain_presets_validate_and_splice_cleanly() {
        let presets = builtin_effect_chain_presets();
        assert!(presets.len() >= 4);
        for preset in &presets {
            validate_effect_chain_preset(preset)
                .unwrap_or_else(|e| panic!("preset '{}' failed validation: {e}", preset.id));
        }

        let project = impact_cut_project();
        let xenoz = find_effect_chain_preset("xenoz-impact-glow").expect("xenoz preset");
        let slots = BTreeMap::from([
            (
                "glow_tint".into(),
                TemplateSlotValue::Color([0.2, 0.9, 1.0, 1.0]),
            ),
            ("glow_intensity".into(), TemplateSlotValue::Number(1.25)),
        ]);
        let cmd = splice_effect_chain_preset(&project, None, "fx1", &xenoz, Some(&slots))
            .expect("splice preset");
        let mut session = frameshift_schema::ProjectSession::new(project).unwrap();
        let rev = session.revision_id();
        session.apply_command(rev, cmd).expect("commit spliced preset");
        let updated = session.project();
        assert_eq!(updated.graph.output_node, NodeId::from("fx1_deep_glow"));
        assert!(
            updated
                .param_bindings
                .iter()
                .any(|b| b.node_id == NodeId::from("fx1_deep_glow")
                    && b.param_name == "intensity"
                    && b.expression == "1.25")
        );

        // Intermediate node splice preserves downstream consumer chain.
        let flash = find_effect_chain_preset("impact_flash").unwrap();
        let intermediate = splice_effect_chain_preset(
            updated,
            Some(&NodeId::from("fx1_blur_mocurves")),
            "mid",
            &flash,
            None,
        )
        .expect("splice intermediate");
        if let ProjectCommand::ReplaceProject { project: mid_proj } = intermediate {
            let glow = mid_proj.graph.node(&NodeId::from("fx1_deep_glow")).unwrap();
            assert_eq!(glow.inputs, vec![NodeId::from("mid_flash")]);
            assert_eq!(mid_proj.graph.output_node, NodeId::from("fx1_deep_glow"));
        } else {
            panic!("expected ReplaceProject");
        }
    }
}
