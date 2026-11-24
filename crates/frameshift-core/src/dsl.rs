//! P2 deterministic `Motion.md` / `.fsmotion` DSL compiler.
//!
//! Compiles a parsed [`MotionDoc`] into a schema-validated `schema_version = "0.2.0"`
//! (`i64` Flicks) [`Project`], automatically converts hex sRGB text colors to Linear
//! Rec.709, composites text transparently (avoiding `LNT-CMP-001`), splices
//! [`EffectChainPreset`] fragments, resolves chained event constraints, and enforces the
//! [`lint_project`] gate.

use crate::error::CoreError;
use crate::linter::{LintOptions, LintReport, lint_project};
use crate::workbench::{find_effect_chain_preset, splice_effect_chain_preset};
use frameshift_media::{MultibandBeatReport, srgb_to_linear};
use frameshift_schema::dsl::{MotionDirective, MotionDoc, MotionTimeExpr};
use frameshift_schema::template::TemplateSlotValue;
use frameshift_schema::{
    AssetDef, AssetKind, AudioBus, AudioClipDef, AudioFadeCurve, AudioFadeDef, BlendMode,
    CURRENT_SCHEMA_VERSION, ClipDef, ColorSpace, EventAnchorDef, EventOrigin,
    EventTimingConstraint, GraphNodeDef, HwAccelPolicy, NodeId, NodeKind, ParamBindingDef,
    Project, ProjectCommand, ProxyConfigDef, RenderGraphDef, RenderProfileDef, RenderProfileKind,
    RenderProfilesDef, Resolution, SchemaError, ShutterPhase, SourceFit, TimelineDef,
    WorkingPixelFormat, WritingMode2D,
};
use frameshift_time::{FLICKS_PER_SECOND, Flicks, Rational32, SpeedCurve};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Compilation options for [`compile_motion_doc`] / [`compile_motion_str`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DslCompileOptions {
    /// Optional beat/downbeat analysis report used to resolve `beat:<n>` and `bar:<n>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beat_report: Option<MultibandBeatReport>,
    /// Options passed to the post-compile [`lint_project`] gate.
    #[serde(default)]
    pub lint_options: LintOptions,
    /// When `true`, `LintSeverity::Error` findings are returned in `lint_report` without rejecting compilation.
    #[serde(default)]
    pub skip_lint: bool,
}

/// Output produced by [`compile_motion_doc`] / [`compile_motion_str`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DslCompileOutput {
    pub project: Project,
    pub lint_report: LintReport,
}

/// Parses and compiles a `Motion.md` / `.fsmotion` string directly to a [`DslCompileOutput`].
pub fn compile_motion_str(
    input: &str,
    opts: &DslCompileOptions,
) -> Result<DslCompileOutput, CoreError> {
    let doc = MotionDoc::parse(input)?;
    compile_motion_doc(&doc, opts)
}

/// Deterministically lowers a [`MotionDoc`] AST to a `0.2.0` (`i64` Flicks) [`Project`]
/// and runs `recompute_event_constraints()`, `Project::validate()`, and `lint_project()`.
pub fn compile_motion_doc(
    doc: &MotionDoc,
    opts: &DslCompileOptions,
) -> Result<DslCompileOutput, CoreError> {
    let fps = doc.meta.fps;
    let frame_ticks = fps.frame_duration()?;
    let res = doc.meta.resolution;
    if res.width == 0 || res.height == 0 {
        return Err(CoreError::InvalidArgument(
            "MotionDoc resolution must be non-zero".into(),
        ));
    }

    let mut assets: Vec<AssetDef> = Vec::new();
    let mut asset_kinds: BTreeMap<String, AssetKind> = BTreeMap::new();
    let mut clips: Vec<ClipDef> = Vec::new();
    let mut audio_clips: Vec<AudioClipDef> = Vec::new();
    let mut events: Vec<EventAnchorDef> = Vec::new();
    let mut event_ticks: BTreeMap<String, Flicks> = BTreeMap::new();

    // First pass: collect assets and beat_drop anchors (while resolving shots/audio in order).
    let mut shot_nodes: BTreeMap<String, NodeId> = BTreeMap::new();
    let mut shot_order: Vec<(String, NodeId)> = Vec::new();
    let mut named_targets: BTreeMap<String, NodeId> = BTreeMap::new();
    let mut nodes: Vec<GraphNodeDef> = Vec::new();
    let mut param_bindings: Vec<ParamBindingDef> = Vec::new();
    let mut transitions: Vec<TransitionDecl> = Vec::new();
    let mut fx_decls: Vec<FxDecl> = Vec::new();
    let mut text_decls: Vec<TextDecl> = Vec::new();
    let mut shutter_cfg: Option<(f32, u32, ShutterPhase)> = None;

    for directive in &doc.directives {
        match directive {
            MotionDirective::Asset { id, kind, path } => {
                if asset_kinds.contains_key(id) {
                    return Err(CoreError::InvalidArgument(format!(
                        "duplicate asset id '{id}' in MotionDoc"
                    )));
                }
                asset_kinds.insert(id.clone(), *kind);
                assets.push(AssetDef {
                    id: id.clone(),
                    kind: *kind,
                    path: path.clone(),
                    duration_ticks: None,
                    fps: Some(fps),
                    sample_rate_hz: if *kind == AssetKind::Audio {
                        Some(48_000)
                    } else {
                        None
                    },
                    resolution: if *kind == AssetKind::Audio {
                        None
                    } else {
                        Some(res)
                    },
                });
            }
            MotionDirective::BeatDrop {
                id,
                at,
                clip_id,
                src,
                label,
            } => {
                let target_output_tick =
                    resolve_motion_time(at, fps, doc.meta.bpm, opts.beat_report.as_ref(), &event_ticks)?;
                let source_anchor_tick = match src {
                    Some(s) => Some(resolve_motion_time(
                        s,
                        fps,
                        doc.meta.bpm,
                        opts.beat_report.as_ref(),
                        &event_ticks,
                    )?),
                    None => None,
                };
                let timing = match at {
                    MotionTimeExpr::EventRef(parent_event_id) => {
                        EventTimingConstraint::RelativeToEvent {
                            parent_event_id: parent_event_id.clone(),
                            offset_ticks: Flicks::ZERO,
                        }
                    }
                    _ => EventTimingConstraint::Absolute,
                };
                event_ticks.insert(id.clone(), target_output_tick);
                events.push(EventAnchorDef {
                    event_id: id.clone(),
                    label: label.clone().unwrap_or_else(|| id.clone()),
                    source_clip_id: clip_id.clone(),
                    source_anchor_tick,
                    target_output_tick,
                    origin: EventOrigin::UserMarked,
                    timing,
                });
            }
            MotionDirective::Shot {
                id,
                asset_id,
                range,
                src,
                fit,
                speed,
                anchor,
            } => {
                let kind = asset_kinds.get(asset_id).copied().ok_or_else(|| {
                    CoreError::Schema(SchemaError::MissingAsset(asset_id.clone()))
                })?;
                let start = resolve_motion_time(
                    &range.start,
                    fps,
                    doc.meta.bpm,
                    opts.beat_report.as_ref(),
                    &event_ticks,
                )?;
                let end = resolve_motion_time(
                    &range.end,
                    fps,
                    doc.meta.bpm,
                    opts.beat_report.as_ref(),
                    &event_ticks,
                )?;
                if end <= start {
                    return Err(CoreError::InvalidArgument(format!(
                        "shot '{id}' range must satisfy start < end ({}..{})",
                        start.0, end.0
                    )));
                }
                let src_start = match src {
                    Some(s) => resolve_motion_time(
                        s,
                        fps,
                        doc.meta.bpm,
                        opts.beat_report.as_ref(),
                        &event_ticks,
                    )?,
                    None => Flicks::ZERO,
                };
                let speed_curve = parse_speed_expr(speed.as_deref())?;
                let fit_val = fit.unwrap_or(SourceFit::Stretch);
                let node_id = NodeId::new(format!("shot_{:02}_{}_src", shot_order.len(), id));

                if kind == AssetKind::Image {
                    nodes.push(GraphNodeDef {
                        id: node_id.clone(),
                        kind: NodeKind::SourceImage {
                            asset_id: asset_id.clone(),
                            color_space: ColorSpace::DisplaySrgb,
                            fit: fit_val,
                            opacity: 1.0,
                        },
                        inputs: Vec::new(),
                        enabled: true,
                    });
                } else {
                    clips.push(ClipDef {
                        id: id.clone(),
                        asset_id: asset_id.clone(),
                        timeline_start: start,
                        timeline_end: end,
                        source_start: src_start,
                        speed_curve,
                        linked_event_id: anchor.clone(),
                        event_animations: Vec::new(),
                    });
                    nodes.push(GraphNodeDef {
                        id: node_id.clone(),
                        kind: NodeKind::SourceVideo {
                            asset_id: asset_id.clone(),
                            clip_id: Some(id.clone()),
                            color_space: ColorSpace::DisplaySrgb,
                            fit: fit_val,
                        },
                        inputs: Vec::new(),
                        enabled: true,
                    });
                }
                shot_nodes.insert(id.clone(), node_id.clone());
                named_targets.insert(id.clone(), node_id.clone());
                named_targets.insert(node_id.0.clone(), node_id.clone());
                shot_order.push((id.clone(), node_id));
            }
            MotionDirective::Audio {
                id,
                asset_id,
                range,
                src,
                bus,
                gain_db,
                fade_in,
                fade_out,
            } => {
                if !asset_kinds.contains_key(asset_id) {
                    return Err(CoreError::Schema(SchemaError::MissingAsset(
                        asset_id.clone(),
                    )));
                }
                let start = resolve_motion_time(
                    &range.start,
                    fps,
                    doc.meta.bpm,
                    opts.beat_report.as_ref(),
                    &event_ticks,
                )?;
                let end = resolve_motion_time(
                    &range.end,
                    fps,
                    doc.meta.bpm,
                    opts.beat_report.as_ref(),
                    &event_ticks,
                )?;
                if end <= start {
                    return Err(CoreError::InvalidArgument(format!(
                        "audio '{id}' range must satisfy start < end"
                    )));
                }
                let source_start = match src {
                    Some(s) => resolve_motion_time(
                        s,
                        fps,
                        doc.meta.bpm,
                        opts.beat_report.as_ref(),
                        &event_ticks,
                    )?,
                    None => Flicks::ZERO,
                };
                let fade_in_def = match fade_in {
                    Some(f) => Some(AudioFadeDef {
                        duration_ticks: resolve_motion_time(
                            f,
                            fps,
                            doc.meta.bpm,
                            opts.beat_report.as_ref(),
                            &event_ticks,
                        )?,
                        curve: AudioFadeCurve::EqualPower,
                    }),
                    None => None,
                };
                let fade_out_def = match fade_out {
                    Some(f) => Some(AudioFadeDef {
                        duration_ticks: resolve_motion_time(
                            f,
                            fps,
                            doc.meta.bpm,
                            opts.beat_report.as_ref(),
                            &event_ticks,
                        )?,
                        curve: AudioFadeCurve::EqualPower,
                    }),
                    None => None,
                };
                audio_clips.push(AudioClipDef {
                    id: id.clone(),
                    asset_id: asset_id.clone(),
                    timeline_start: start,
                    timeline_end: end,
                    source_start,
                    gain_db: gain_db.unwrap_or(0.0),
                    fade_in: fade_in_def,
                    fade_out: fade_out_def,
                    bus: bus.unwrap_or(AudioBus::Music),
                    pan: 0.0,
                    anchor_event_id: None,
                    apex_offset_ticks: None,
                });
            }
            MotionDirective::Transition {
                shader,
                from_clip,
                to_clip,
                at,
                dur,
                params,
            } => {
                let at_ticks = resolve_motion_time(
                    at,
                    fps,
                    doc.meta.bpm,
                    opts.beat_report.as_ref(),
                    &event_ticks,
                )?;
                let dur_ticks = resolve_motion_time(
                    dur,
                    fps,
                    doc.meta.bpm,
                    opts.beat_report.as_ref(),
                    &event_ticks,
                )?;
                if dur_ticks.0 <= 0 {
                    return Err(CoreError::InvalidArgument(format!(
                        "transition '{shader}' duration must be > 0"
                    )));
                }
                transitions.push(TransitionDecl {
                    shader: shader.clone(),
                    from_clip: from_clip.clone(),
                    to_clip: to_clip.clone(),
                    at_ticks,
                    dur_ticks,
                    params: params.clone(),
                });
            }
            MotionDirective::Fx {
                preset_or_kind,
                on,
                at,
                dur,
                params,
            } => {
                let at_ticks = match at {
                    Some(t) => Some(resolve_motion_time(
                        t,
                        fps,
                        doc.meta.bpm,
                        opts.beat_report.as_ref(),
                        &event_ticks,
                    )?),
                    None => None,
                };
                let dur_ticks = match dur {
                    Some(d) => Some(resolve_motion_time(
                        d,
                        fps,
                        doc.meta.bpm,
                        opts.beat_report.as_ref(),
                        &event_ticks,
                    )?),
                    None => None,
                };
                fx_decls.push(FxDecl {
                    preset_or_kind: preset_or_kind.clone(),
                    on: on.clone(),
                    at_ticks,
                    dur_ticks,
                    params: params.clone(),
                });
            }
            MotionDirective::Text {
                text,
                range,
                pos,
                size,
                color,
                font,
                outline,
                anim,
            } => {
                let start = resolve_motion_time(
                    &range.start,
                    fps,
                    doc.meta.bpm,
                    opts.beat_report.as_ref(),
                    &event_ticks,
                )?;
                let end = resolve_motion_time(
                    &range.end,
                    fps,
                    doc.meta.bpm,
                    opts.beat_report.as_ref(),
                    &event_ticks,
                )?;
                if end <= start {
                    return Err(CoreError::InvalidArgument(format!(
                        "text '{text}' range must satisfy start < end"
                    )));
                }
                text_decls.push(TextDecl {
                    text: text.clone(),
                    start,
                    end,
                    pos: pos.unwrap_or([0.5, 0.5]),
                    size: size.unwrap_or(0.075),
                    color: color.clone(),
                    font: font.clone(),
                    outline: outline.clone(),
                    anim: anim.clone(),
                });
            }
            MotionDirective::Shutter {
                angle,
                samples,
                phase,
            } => {
                shutter_cfg = Some((
                    angle.unwrap_or(180.0),
                    samples.unwrap_or(8),
                    phase.unwrap_or(ShutterPhase::Centered),
                ));
            }
        }
    }

    // Extend clip ranges for transitions and build picture DAG.
    let mut current_chain: Option<NodeId> = None;

    if let Some(bg_hex) = doc.meta.bg_color.as_deref() {
        let bg_rgba = parse_hex_linear_rgba(bg_hex)?;
        let bg_id = NodeId::new("bg_solid");
        nodes.push(GraphNodeDef {
            id: bg_id.clone(),
            kind: NodeKind::Solid {
                color_rgba: bg_rgba,
                color_space: ColorSpace::LinearRec709,
            },
            inputs: Vec::new(),
            enabled: true,
        });
        current_chain = Some(bg_id);
    }

    let mut joined_to_clips: BTreeSet<String> = BTreeSet::new();
    let mut transition_by_pair: BTreeMap<(String, String), Vec<(usize, TransitionDecl)>> =
        BTreeMap::new();
    for (idx, tr) in transitions.iter().enumerate() {
        joined_to_clips.insert(tr.to_clip.clone());
        transition_by_pair
            .entry((tr.from_clip.clone(), tr.to_clip.clone()))
            .or_default()
            .push((idx + 1, tr.clone()));
    }

    // Adjust clip timeline boundaries to cover transitions smoothly.
    for tr in &transitions {
        let half = Flicks(tr.dur_ticks.0 / 2);
        let tr_start = Flicks((tr.at_ticks.0 - half.0).max(0));
        let tr_end = Flicks(tr.at_ticks.0 + half.0);
        if let Some(c_from) = clips.iter_mut().find(|c| c.id == tr.from_clip)
            && c_from.timeline_end < tr_end
        {
            c_from.timeline_end = tr_end;
        }
        if let Some(c_to) = clips.iter_mut().find(|c| c.id == tr.to_clip)
            && c_to.timeline_start > tr_start
        {
            c_to.timeline_start = tr_start;
        }
    }

    // Chain shots and transitions deterministically.
    for (shot_idx, (shot_id, shot_node)) in shot_order.iter().enumerate() {
        // Check if a transition starts from the previous shot to this shot.
        let prev_shot_id = if shot_idx > 0 {
            Some(shot_order[shot_idx - 1].0.as_str())
        } else {
            None
        };
        if let Some(prev_id) = prev_shot_id
            && let Some(trs) = transition_by_pair.remove(&(prev_id.to_owned(), shot_id.clone()))
            && let Some(prev_chain) = current_chain.take()
        {
            let mut chain_cursor = prev_chain;
            for (tr_num, tr) in trs {
                let tr_node_id = NodeId::new(format!("tr_{:02}_{}", tr_num, tr.shader));
                let half = Flicks(tr.dur_ticks.0 / 2);
                let t0 = (tr.at_ticks.0 - half.0).max(0) as f64 / FLICKS_PER_SECOND as f64;
                let t1 = (tr.at_ticks.0 + half.0).max(tr.at_ticks.0 - half.0 + 1) as f64
                    / FLICKS_PER_SECOND as f64;
                let whip_strength = parse_opt_f32(&tr.params, &["whip", "whip_strength"], 0.75)?;
                let chromatic_aberration =
                    parse_opt_f32(&tr.params, &["ca", "chromatic_aberration"], 0.025)?;
                let direction_rad = parse_opt_f32(&tr.params, &["dir", "direction_rad"], 0.0)?;
                let impact_flash = parse_opt_f32(&tr.params, &["flash", "impact_flash"], 0.0)?;

                nodes.push(GraphNodeDef {
                    id: tr_node_id.clone(),
                    kind: NodeKind::ShaderTransition {
                        shader_name: tr.shader.clone(),
                        progress: 0.0,
                        direction_rad,
                        whip_strength,
                        chromatic_aberration,
                        impact_flash,
                    },
                    inputs: vec![chain_cursor, shot_node.clone()],
                    enabled: true,
                });
                param_bindings.push(ParamBindingDef {
                    node_id: tr_node_id.clone(),
                    param_name: "progress".into(),
                    expression: format!("smoothstep({t0:.6}, {t1:.6}, t_sec)"),
                });
                named_targets.insert(format!("tr_{tr_num:02}"), tr_node_id.clone());
                named_targets.insert(tr_node_id.0.clone(), tr_node_id.clone());
                chain_cursor = tr_node_id;
            }
            current_chain = Some(chain_cursor);
            continue;
        }

        current_chain = Some(match current_chain {
            None => shot_node.clone(),
            Some(prev) => {
                let blend_id = NodeId::new(format!("cut_blend_{shot_id}"));
                nodes.push(GraphNodeDef {
                    id: blend_id.clone(),
                    kind: NodeKind::Blend {
                        mode: BlendMode::Normal,
                        opacity: 1.0,
                    },
                    inputs: vec![prev, shot_node.clone()],
                    enabled: true,
                });
                blend_id
            }
        });
    }

    // Any remaining transitions referencing non-adjacent shots:
    for ((from_id, to_id), trs) in transition_by_pair {
        let from_node = shot_nodes.get(&from_id).cloned().ok_or_else(|| {
            CoreError::InvalidArgument(format!("transition unknown clip '{from_id}'"))
        })?;
        let to_node = shot_nodes.get(&to_id).cloned().ok_or_else(|| {
            CoreError::InvalidArgument(format!("transition unknown clip '{to_id}'"))
        })?;
        for (tr_num, tr) in trs {
            let tr_node_id = NodeId::new(format!("tr_{:02}_{}", tr_num, tr.shader));
            let half = Flicks(tr.dur_ticks.0 / 2);
            let t0 = (tr.at_ticks.0 - half.0).max(0) as f64 / FLICKS_PER_SECOND as f64;
            let t1 = (tr.at_ticks.0 + half.0).max(tr.at_ticks.0 - half.0 + 1) as f64
                / FLICKS_PER_SECOND as f64;
            nodes.push(GraphNodeDef {
                id: tr_node_id.clone(),
                kind: NodeKind::ShaderTransition {
                    shader_name: tr.shader.clone(),
                    progress: 0.0,
                    direction_rad: parse_opt_f32(&tr.params, &["dir", "direction_rad"], 0.0)?,
                    whip_strength: parse_opt_f32(&tr.params, &["whip", "whip_strength"], 0.75)?,
                    chromatic_aberration: parse_opt_f32(
                        &tr.params,
                        &["ca", "chromatic_aberration"],
                        0.025,
                    )?,
                    impact_flash: parse_opt_f32(&tr.params, &["flash", "impact_flash"], 0.0)?,
                },
                inputs: vec![from_node.clone(), to_node.clone()],
                enabled: true,
            });
            param_bindings.push(ParamBindingDef {
                node_id: tr_node_id.clone(),
                param_name: "progress".into(),
                expression: format!("smoothstep({t0:.6}, {t1:.6}, t_sec)"),
            });
            named_targets.insert(format!("tr_{tr_num:02}"), tr_node_id.clone());
            named_targets.insert(tr_node_id.0.clone(), tr_node_id.clone());
            current_chain = Some(match current_chain {
                None => tr_node_id,
                Some(prev) => {
                    let blend_id = NodeId::new(format!("cut_blend_tr_{tr_num:02}"));
                    nodes.push(GraphNodeDef {
                        id: blend_id.clone(),
                        kind: NodeKind::Blend {
                            mode: BlendMode::Normal,
                            opacity: 1.0,
                        },
                        inputs: vec![prev, tr_node_id],
                        enabled: true,
                    });
                    blend_id
                }
            });
        }
    }

    let base_out = match current_chain {
        Some(id) => id,
        None => {
            let empty_id = NodeId::new("transparent_base");
            nodes.push(GraphNodeDef {
                id: empty_id.clone(),
                kind: NodeKind::Solid {
                    color_rgba: [0.0, 0.0, 0.0, 0.0],
                    color_space: ColorSpace::DisplaySrgb,
                },
                inputs: Vec::new(),
                enabled: true,
            });
            empty_id
        }
    };

    // Determine initial timeline duration.
    let mut duration_ticks = Flicks(frame_ticks.0);
    for c in &clips {
        duration_ticks = duration_ticks.max(c.timeline_end);
    }
    for ac in &audio_clips {
        duration_ticks = duration_ticks.max(ac.timeline_end);
    }
    for td in &text_decls {
        duration_ticks = duration_ticks.max(td.end);
    }
    for ev in &events {
        duration_ticks = duration_ticks.max(Flicks(ev.target_output_tick.0 + frame_ticks.0));
    }

    let draft_w = (res.width / 2).max(2) & !1;
    let draft_h = (res.height / 2).max(2) & !1;

    let mut project = Project {
        schema_version: CURRENT_SCHEMA_VERSION,
        revision_id: 1,
        name: doc.meta.name.clone(),
        assets,
        timeline: TimelineDef {
            fps,
            duration_ticks,
            resolution: res,
            clips,
            audio_clips,
            ducking_rules: Vec::new(),
            micro_silences: Vec::new(),
        },
        events,
        graph: RenderGraphDef {
            output_node: base_out,
            nodes,
        },
        render_profiles: RenderProfilesDef {
            draft: RenderProfileDef {
                kind: RenderProfileKind::Draft,
                resolution: Resolution {
                    width: draft_w.max(2),
                    height: draft_h.max(2),
                },
                motion_blur_sub_samples: 1,
                shutter_angle_deg: 180.0,
                pixel_format: WorkingPixelFormat::Rgba16FloatPremul,
            },
            final_profile: RenderProfileDef {
                kind: RenderProfileKind::Final,
                resolution: res,
                motion_blur_sub_samples: shutter_cfg.map_or(1, |s| s.1),
                shutter_angle_deg: shutter_cfg.map_or(180.0, |s| s.0),
                pixel_format: WorkingPixelFormat::Rgba16FloatPremul,
            },
        },
        param_bindings,
        keyframe_tracks: Vec::new(),
        creative_decisions: Vec::new(),
        preferences: Vec::new(),
        proxy_config: ProxyConfigDef::default(),
        hw_accel_policy: HwAccelPolicy::Auto,
    };

    // Apply FX directives (either EffectChainPreset splice or built-in single FX node).
    for (idx, fx) in fx_decls.iter().enumerate() {
        let target_node = if fx.on == "output" || fx.on == "master" {
            project.graph.output_node.clone()
        } else {
            named_targets
                .get(&fx.on)
                .cloned()
                .unwrap_or_else(|| NodeId::new(fx.on.clone()))
        };
        if project.graph.node(&target_node).is_none() {
            return Err(CoreError::Schema(SchemaError::MissingNode(target_node)));
        }

        let prefix = format!("fx_{:02}_{}", idx + 1, sanitize_id(&fx.preset_or_kind));
        if let Some(preset) = find_effect_chain_preset(&fx.preset_or_kind) {
            let mut slots = BTreeMap::new();
            for slot in &preset.slots {
                if let Some(raw_val) = fx.params.get(&slot.id)
                    && let Ok(num) = raw_val.parse::<f64>()
                {
                    slots.insert(slot.id.clone(), TemplateSlotValue::Number(num));
                }
            }
            let cmd = splice_effect_chain_preset(
                &project,
                Some(&target_node),
                &prefix,
                &preset,
                if slots.is_empty() { None } else { Some(&slots) },
            )?;
            if let ProjectCommand::ReplaceProject { project: updated } = cmd {
                project = updated;
            }
            // If `at` / `dur` window was requested, gate the primary parameter on the spliced nodes.
            if let Some(at_ticks) = fx.at_ticks {
                let dur_ticks = fx
                    .dur_ticks
                    .unwrap_or(Flicks(FLICKS_PER_SECOND / 2));
                let t0 = at_ticks.0 as f64 / FLICKS_PER_SECOND as f64;
                let t1 = (at_ticks.0 + dur_ticks.0) as f64 / FLICKS_PER_SECOND as f64;
                let gate = format!("(step({t0:.6}, t_sec) * (1.0 - step({t1:.6}, t_sec)))");
                if let Some(deep_glow_node) = project
                    .graph
                    .nodes
                    .iter()
                    .find(|n| n.id.0.starts_with(&prefix) && matches!(n.kind, NodeKind::DeepGlow { .. }))
                    .map(|n| n.id.clone())
                {
                    let base_int = fx
                        .params
                        .get("glow_intensity")
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(1.0);
                    let expr = format!("clamp(0.15 + {base_int:.4} * {gate}, 0.0, 4.0)");
                    upsert_param_binding(&mut project, deep_glow_node, "intensity", expr);
                } else if let Some(shake_node) = project
                    .graph
                    .nodes
                    .iter()
                    .find(|n| n.id.0.starts_with(&prefix) && matches!(n.kind, NodeKind::AnimeImpactFrame { .. }))
                    .map(|n| n.id.clone())
                {
                    let base_mix = fx
                        .params
                        .get("impact_mix")
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(0.65);
                    upsert_param_binding(
                        &mut project,
                        shake_node,
                        "mix_amount",
                        format!("{base_mix:.4} * {gate}"),
                    );
                }
            }
        } else {
            let (kind, primary_param, default_val) = build_single_fx_node(&fx.preset_or_kind, &fx.params)?;
            let fx_node_id = NodeId::new(prefix);
            splice_single_node_after(&mut project, &target_node, fx_node_id.clone(), kind)?;
            if let (Some(at_ticks), Some(param)) = (fx.at_ticks, primary_param) {
                let dur_ticks = fx.dur_ticks.unwrap_or(Flicks(FLICKS_PER_SECOND / 2));
                let t0 = at_ticks.0 as f64 / FLICKS_PER_SECOND as f64;
                let t1 = (at_ticks.0 + dur_ticks.0) as f64 / FLICKS_PER_SECOND as f64;
                upsert_param_binding(
                    &mut project,
                    fx_node_id,
                    param,
                    format!("{default_val:.4} * (step({t0:.6}, t_sec) * (1.0 - step({t1:.6}, t_sec)))"),
                );
            }
        }
    }

    // Apply master LUT before text overlays (prevents LNT-MAT-001 LUT-after-text color shift).
    if let Some(lut) = &doc.meta.lut {
        let prev_out = project.graph.output_node.clone();
        let lut_id = NodeId::new("lut_master");
        project.graph.nodes.push(GraphNodeDef {
            id: lut_id.clone(),
            kind: NodeKind::Lut3D {
                input: prev_out.clone(),
                lut_preset: lut.preset.clone(),
                intensity: lut.intensity,
                lut_source: None,
                input_space: None,
                output_space: None,
            },
            inputs: vec![prev_out],
            enabled: true,
        });
        project.graph.output_node = lut_id;
    }

    // Apply transparent VectorText overlays (automatic srgb_to_linear conversion).
    for (idx, td) in text_decls.iter().enumerate() {
        let color_rgba = match td.color.as_deref() {
            Some(hex) => parse_hex_linear_rgba(hex)?,
            None => [1.0, 1.0, 1.0, 1.0],
        };
        let outline_rgba = match td.outline.as_deref() {
            Some(hex) => parse_hex_linear_rgba(hex)?,
            None => [0.0, 0.0, 0.0, 1.0],
        };
        let outline_width_norm = if td.outline.is_some() { 0.004 } else { 0.0 };
        let txt_id = NodeId::new(format!("txt_{:02}", idx + 1));
        let blend_id = NodeId::new(format!("blend_txt_{:02}", idx + 1));

        let (font_family, font_path) = match &td.font {
            Some(f) if f.contains('/') || f.contains('\\') || f.ends_with(".ttf") || f.ends_with(".otf") => {
                (None, Some(f.clone()))
            }
            Some(f) => (Some(f.clone()), None),
            None => (None, None),
        };

        project.graph.nodes.push(GraphNodeDef {
            id: txt_id.clone(),
            kind: NodeKind::VectorText {
                text: td.text.clone(),
                font_size_norm: td.size,
                color_rgba,
                outline_rgba,
                outline_width_norm,
                align_center: true,
                y_offset_norm: td.pos[1],
                font_family,
                font_path,
                font_weight: 700,
                align: Some(frameshift_schema::TextAlign2D::Center),
                x_offset_norm: Some(td.pos[0]),
                tracking_em: None,
                line_height_em: None,
                opacity: 1.0,
                fallback_fonts: Vec::new(),
                writing_mode: WritingMode2D::HorizontalTb,
                var_wght: None,
                var_wdth: None,
                var_slnt: None,
                color_bottom_rgba: None,
                gradient_angle_deg: 0.0,
                outer_outline_rgba: [0.0, 0.0, 0.0, 0.0],
                outer_outline_width_norm: 0.0,
                outline_gap_norm: 0.0,
                shadow_offset_norm: [0.0, 0.0],
                shadow_blur_norm: 0.0,
                shadow_rgba: [0.0, 0.0, 0.0, 0.0],
                glow_radius_norm: 0.0,
                glow_rgba: [0.0, 0.0, 0.0, 0.0],
                hdr_intensity: 1.0,
                gpu_slug: true,
                text_animator: None,
            },
            inputs: Vec::new(),
            enabled: true,
        });

        let prev_out = project.graph.output_node.clone();
        project.graph.nodes.push(GraphNodeDef {
            id: blend_id.clone(),
            kind: NodeKind::Blend {
                mode: BlendMode::Normal,
                opacity: 1.0,
            },
            inputs: vec![prev_out, txt_id.clone()],
            enabled: true,
        });
        project.graph.output_node = blend_id;

        let t0 = td.start.0 as f64 / FLICKS_PER_SECOND as f64;
        let t1 = td.end.0 as f64 / FLICKS_PER_SECOND as f64;
        let opacity_expr = match td.anim.as_deref() {
            Some("fade") => {
                let fade_len = ((t1 - t0) * 0.2).clamp(0.04, 0.25);
                format!(
                    "smoothstep({t0:.6}, {:.6}, t_sec) * (1.0 - step({t1:.6}, t_sec))",
                    t0 + fade_len
                )
            }
            _ => format!("step({t0:.6}, t_sec) * (1.0 - step({t1:.6}, t_sec))"),
        };
        upsert_param_binding(&mut project, txt_id, "opacity", opacity_expr);
    }

    // Apply optional MotionBlurShutter as final output node.
    if let Some((angle, samples, phase)) = shutter_cfg {
        let prev_out = project.graph.output_node.clone();
        let shutter_id = NodeId::new("shutter_out");
        project.graph.nodes.push(GraphNodeDef {
            id: shutter_id.clone(),
            kind: NodeKind::MotionBlurShutter {
                shutter_angle_deg: angle,
                sub_samples: samples,
                phase,
                tonemap: true,
            },
            inputs: vec![prev_out],
            enabled: true,
        });
        project.graph.output_node = shutter_id;
    }

    project.recompute_event_constraints()?;
    project.validate()?;

    let lint_report = lint_project(&project, &opts.lint_options);
    if !opts.skip_lint && !lint_report.ok {
        let summary = lint_report
            .findings
            .iter()
            .filter(|f| f.severity == crate::linter::LintSeverity::Error)
            .map(|f| format!("[{}] {}", f.code, f.message))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(CoreError::InvalidArgument(format!(
            "Motion DSL compilation failed lint gate ({} error(s)): {summary}",
            lint_report.error_count
        )));
    }

    Ok(DslCompileOutput {
        project,
        lint_report,
    })
}

#[derive(Clone)]
struct TransitionDecl {
    shader: String,
    from_clip: String,
    to_clip: String,
    at_ticks: Flicks,
    dur_ticks: Flicks,
    params: BTreeMap<String, String>,
}

struct FxDecl {
    preset_or_kind: String,
    on: String,
    at_ticks: Option<Flicks>,
    dur_ticks: Option<Flicks>,
    params: BTreeMap<String, String>,
}

struct TextDecl {
    text: String,
    start: Flicks,
    end: Flicks,
    pos: [f32; 2],
    size: f32,
    color: Option<String>,
    font: Option<String>,
    outline: Option<String>,
    anim: Option<String>,
}

fn resolve_motion_time(
    expr: &MotionTimeExpr,
    fps: Rational32,
    bpm: Option<f64>,
    beat_report: Option<&MultibandBeatReport>,
    events: &BTreeMap<String, Flicks>,
) -> Result<Flicks, CoreError> {
    let frame_ticks = fps.frame_duration()?.0;
    let quantize = |tick: Flicks| -> Flicks {
        let ft = frame_ticks as i128;
        let q = ((tick.0 as i128 + (ft / 2) * (tick.0 as i128).signum()) / ft) * ft;
        Flicks(q as i64)
    };
    match expr {
        MotionTimeExpr::Beat(idx) if *idx >= 0 => {
            if let Some(report) = beat_report
                && let Some(&t) = report.beat_grid.get(*idx as usize)
            {
                return Ok(quantize(t));
            }
            Ok(expr.to_ticks_static(fps, bpm, events)?)
        }
        MotionTimeExpr::Bar(idx) if *idx >= 0 => {
            if let Some(report) = beat_report
                && let Some(&t) = report.downbeats.get(*idx as usize)
            {
                return Ok(quantize(t));
            }
            Ok(expr.to_ticks_static(fps, bpm, events)?)
        }
        _ => Ok(expr.to_ticks_static(fps, bpm, events)?),
    }
}

/// Parses `#RRGGBB` or `#RRGGBBAA` sRGB hex into Linear Rec.709 `[f32; 4]`.
pub fn parse_hex_linear_rgba(raw: &str) -> Result<[f32; 4], CoreError> {
    let hex = raw.trim().strip_prefix('#').ok_or_else(|| {
        CoreError::InvalidArgument(format!("expected hex color '#RRGGBB[AA]', got '{raw}'"))
    })?;
    if !(hex.len() == 6 || hex.len() == 8) {
        return Err(CoreError::InvalidArgument(format!(
            "expected 6 or 8 hex digits in '{raw}'"
        )));
    }
    let byte_at = |i: usize| -> Result<u8, CoreError> {
        u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| {
            CoreError::InvalidArgument(format!("invalid hex byte in color '{raw}'"))
        })
    };
    let r = srgb_to_linear(f32::from(byte_at(0)?) / 255.0);
    let g = srgb_to_linear(f32::from(byte_at(2)?) / 255.0);
    let b = srgb_to_linear(f32::from(byte_at(4)?) / 255.0);
    let a = if hex.len() == 8 {
        f32::from(byte_at(6)?) / 255.0
    } else {
        1.0
    };
    Ok([r, g, b, a])
}

fn parse_speed_expr(raw: Option<&str>) -> Result<SpeedCurve, CoreError> {
    let Some(s) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(SpeedCurve::constant(1.0)?);
    };
    let stripped = s.strip_suffix('x').unwrap_or(s);
    let factor: f64 = stripped.parse().map_err(|_| {
        CoreError::InvalidArgument(format!("invalid shot speed expression '{s}'"))
    })?;
    Ok(SpeedCurve::constant(factor)?)
}

fn parse_opt_f32(
    params: &BTreeMap<String, String>,
    keys: &[&str],
    default: f32,
) -> Result<f32, CoreError> {
    for k in keys {
        if let Some(raw) = params.get(*k) {
            let v: f32 = raw.parse().map_err(|_| {
                CoreError::InvalidArgument(format!("invalid float '{raw}' for '{k}'"))
            })?;
            if !v.is_finite() {
                return Err(CoreError::InvalidArgument(format!(
                    "non-finite float for '{k}'"
                )));
            }
            return Ok(v);
        }
    }
    Ok(default)
}

fn sanitize_id(raw: &str) -> String {
    raw.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
        .collect()
}

fn upsert_param_binding(
    project: &mut Project,
    node_id: NodeId,
    param_name: &str,
    expression: String,
) {
    if let Some(existing) = project
        .param_bindings
        .iter_mut()
        .find(|b| b.node_id == node_id && b.param_name == param_name)
    {
        existing.expression = expression;
    } else {
        project.param_bindings.push(ParamBindingDef {
            node_id,
            param_name: param_name.to_owned(),
            expression,
        });
    }
}

fn splice_single_node_after(
    project: &mut Project,
    target_id: &NodeId,
    new_id: NodeId,
    kind: NodeKind,
) -> Result<(), CoreError> {
    for existing in &mut project.graph.nodes {
        for input in &mut existing.inputs {
            if existing.id != new_id && *input == *target_id {
                *input = new_id.clone();
            }
        }
        if let NodeKind::Lut3D { input, .. } = &mut existing.kind
            && *input == *target_id
        {
            *input = new_id.clone();
        }
    }
    if project.graph.output_node == *target_id {
        project.graph.output_node = new_id.clone();
    }
    project.graph.nodes.push(GraphNodeDef {
        id: new_id,
        kind,
        inputs: vec![target_id.clone()],
        enabled: true,
    });
    Ok(())
}

fn build_single_fx_node(
    kind_name: &str,
    params: &BTreeMap<String, String>,
) -> Result<(NodeKind, Option<&'static str>, f64), CoreError> {
    let norm = kind_name.trim().to_ascii_lowercase().replace('-', "_");
    match norm.as_str() {
        "color_basic" => {
            let flash = parse_opt_f32(params, &["flash", "flash_amount"], 0.35)?;
            Ok((
                NodeKind::ColorBasic {
                    exposure: parse_opt_f32(params, &["exposure"], 0.0)?,
                    contrast: parse_opt_f32(params, &["contrast"], 1.0)?,
                    saturation: parse_opt_f32(params, &["saturation"], 1.0)?,
                    flash_amount: flash,
                },
                Some("flash_amount"),
                f64::from(flash),
            ))
        }
        "deep_glow" => {
            let int = parse_opt_f32(params, &["intensity", "glow_intensity"], 0.9)?;
            Ok((
                NodeKind::DeepGlow {
                    threshold: parse_opt_f32(params, &["threshold"], 0.7)?,
                    knee: 0.5,
                    intensity: int,
                    radius: parse_opt_f32(params, &["radius"], 1.1)?,
                    tint_rgb: [1.0, 0.85, 0.95],
                    mip_levels: 5,
                    scatter: 0.6,
                    anamorphic_ratio: 0.0,
                    chromatic_shift: 0.01,
                },
                Some("intensity"),
                f64::from(int),
            ))
        }
        "anime_sharpen" => Ok((
            NodeKind::AnimeSharpen {
                line_strength: parse_opt_f32(params, &["line_strength"], 0.5)?,
                sharpen_amount: parse_opt_f32(params, &["sharpen_amount"], 0.6)?,
                anti_ringing: 0.8,
            },
            Some("sharpen_amount"),
            0.6,
        )),
        "radial_shockwave" => {
            let radius = parse_opt_f32(params, &["radius"], 0.25)?;
            Ok((
                NodeKind::RadialShockwave {
                    center_uv: [0.5, 0.5],
                    radius,
                    ring_width: parse_opt_f32(params, &["ring_width", "band_width"], 0.12)?,
                    refraction_amp: parse_opt_f32(params, &["refraction_amp"], 0.04)?,
                    chromatic_aberration: parse_opt_f32(
                        params,
                        &["ca", "chromatic_aberration", "chromatic_split"],
                        0.015,
                    )?,
                    zoom_blur_strength: parse_opt_f32(params, &["zoom_blur_strength"], 0.2)?,
                },
                Some("radius"),
                f64::from(radius),
            ))
        }
        "blur_mocurves" => {
            let bs = parse_opt_f32(params, &["blur_strength"], 0.85)?;
            Ok((
                NodeKind::BlurMoCurves {
                    shift_x: parse_opt_f32(params, &["shift_x"], 0.03)?,
                    shift_y: parse_opt_f32(params, &["shift_y"], 0.0)?,
                    z_dist: parse_opt_f32(params, &["z_dist"], -0.06)?,
                    rotate_deg: parse_opt_f32(params, &["rotate_deg"], 1.5)?,
                    blur_strength: bs,
                    samples: 12,
                    wrap_mode: 2,
                },
                Some("blur_strength"),
                f64::from(bs),
            ))
        }
        "optical_flow_blur" => {
            let sa = parse_opt_f32(params, &["shutter_amount"], 1.0)?;
            Ok((
                NodeKind::OpticalFlowBlur {
                    shutter_amount: sa,
                    samples: 12,
                    max_blur_px: 48.0,
                    sensitivity: 0.35,
                    flow_smoothness: 0.25,
                    visualize_flow: false,
                },
                Some("shutter_amount"),
                f64::from(sa),
            ))
        }
        other => Err(CoreError::InvalidArgument(format!(
            "unknown fx preset or node kind '{other}'"
        ))),
    }
}
