use frameshift_schema::{
    AssetDef, AssetKind, BlendMode, CURRENT_SCHEMA_VERSION, Camera3DDef, Camera3DMode, ColorSpace,
    CreativeDecisionRecord, CubicBezierSegmentDef, DisabledPassthrough, ExtendedColorSpace,
    GraphNodeDef, HardCut, HwAccelPolicy, LutSourceDef, MAX_PRECOMP_DEPTH, MatteMode, NodeId,
    NodeKind, ParamBindingDef, PreferenceEntry, PreferenceScope, Project, ProjectCommand,
    ProjectSession, ProxyConfigDef, RenderGraphDef, SampleFilter, SchemaError, SchemaVersion,
    Shape3DKind, SourceFit, TextAlign2D, TextAnimatorDef, TextRangeShape, TextSelectorUnit,
};
use frameshift_time::{FLICKS_PER_SECOND, Flicks, ImpactCurveStyle, SourceFitMode, SpeedCurve};

#[test]
fn golden_scene_preset_validates_toposorts_and_roundtrips_json() {
    let project = Project::golden_scene_preset();
    assert_eq!(project.schema_version, CURRENT_SCHEMA_VERSION);
    assert_eq!(project.revision_id, 1);

    let topo = project.validate().expect("golden preset must validate");
    assert_eq!(topo.len(), project.graph.nodes.len());

    // Verify topological invariant: every upstream input appears before its dependent
    for node in &project.graph.nodes {
        let node_pos = topo.iter().position(|id| id == &node.id).unwrap();
        for input in &node.inputs {
            let input_pos = topo.iter().position(|id| id == input).unwrap();
            assert!(
                input_pos < node_pos,
                "upstream {input} must precede {node_id}",
                node_id = node.id
            );
        }
    }

    let json = project.to_json_pretty().expect("serialize golden preset");
    assert!(json.contains("\"schema_version\": \"0.2.0\""));
    let restored = Project::from_json_str(&json).expect("deserialize golden preset");
    assert_eq!(project, restored);
}

fn shared_alpha_matte_project(
    matte_asset_id: &str,
    matte_duration: Option<Flicks>,
    matte_fit: SourceFit,
) -> Project {
    let mut project = Project::golden_scene_preset();
    let clip = project
        .timeline
        .clips
        .iter_mut()
        .find(|clip| clip.id == "clip_a")
        .expect("golden project clip_a");
    clip.timeline_end = clip.timeline_start + Flicks(3 * FLICKS_PER_SECOND);
    clip.source_start = Flicks(FLICKS_PER_SECOND);
    clip.speed_curve = SpeedCurve::constant(1.0).expect("unit speed");

    if matte_asset_id != "asset_clip_a" {
        project.assets.push(AssetDef {
            id: matte_asset_id.into(),
            kind: AssetKind::Video,
            path: "synthetic://matte_video".into(),
            duration_ticks: matte_duration,
            fps: Some(frameshift_time::FPS_24),
            sample_rate_hz: None,
            resolution: Some(frameshift_schema::Resolution {
                width: 1920,
                height: 1080,
            }),
        });
    }

    let source_id = NodeId::from("shared_alpha_source");
    let matte_id = NodeId::from("shared_alpha_video");
    let mask_id = NodeId::from("shared_alpha_mask");
    project.graph.nodes.extend([
        GraphNodeDef {
            id: source_id.clone(),
            kind: NodeKind::SourceVideo {
                asset_id: "asset_clip_a".into(),
                clip_id: Some("clip_a".into()),
                color_space: ColorSpace::DisplaySrgb,
                fit: SourceFit::Cover,
            },
            inputs: vec![],
            enabled: true,
        },
        GraphNodeDef {
            id: matte_id.clone(),
            kind: NodeKind::SourceVideo {
                asset_id: matte_asset_id.into(),
                clip_id: Some("clip_a".into()),
                color_space: ColorSpace::DisplaySrgb,
                fit: matte_fit,
            },
            inputs: vec![],
            enabled: true,
        },
        GraphNodeDef {
            id: mask_id.clone(),
            kind: NodeKind::MaskMatte {
                mode: MatteMode::Alpha,
                invert: false,
                feather: 0.0,
                opaque_black_outside: false,
                opacity: 1.0,
                choke_px: 0.0,
                spatial_feather_px: 0.0,
            },
            inputs: vec![source_id, matte_id],
            enabled: true,
        },
    ]);
    project.graph.output_node = mask_id;
    project
}

#[test]
fn shared_clip_alpha_matte_checks_crop_and_declared_source_time_coverage() {
    let enough = shared_alpha_matte_project(
        "asset_alpha_matte",
        Some(Flicks(4 * FLICKS_PER_SECOND)),
        SourceFit::Cover,
    );
    enough.validate().expect("matte covers the full source map");

    let too_short = shared_alpha_matte_project(
        "asset_short_matte",
        Some(Flicks(4 * FLICKS_PER_SECOND - 1)),
        SourceFit::Cover,
    );
    assert!(matches!(
        too_short.validate(),
        Err(SchemaError::InvalidTimeline(message))
            if message.contains("shared_alpha_mask") && message.contains("outside matte asset")
    ));

    let mismatched_crop = shared_alpha_matte_project(
        "asset_different_crop",
        Some(Flicks(5 * FLICKS_PER_SECOND)),
        SourceFit::Contain,
    );
    assert!(matches!(
        mismatched_crop.validate(),
        Err(SchemaError::InvalidTimeline(message))
            if message.contains("shared_alpha_mask") && message.contains("same crop")
    ));

    let unspecified_duration =
        shared_alpha_matte_project("asset_unknown_duration", None, SourceFit::Cover);
    unspecified_duration
        .validate()
        .expect("unknown duration leaves runtime source checks unchanged");

    let same_asset = shared_alpha_matte_project("asset_clip_a", None, SourceFit::Contain);
    same_asset
        .validate()
        .expect("self-matte behavior remains generic");
}

#[test]
fn retarget_event_solves_linked_clip_and_rolls_back_constraint_failures() {
    let mut session = ProjectSession::new(Project::golden_scene_preset()).unwrap();
    let target = Flicks(FLICKS_PER_SECOND + FLICKS_PER_SECOND / 2);
    let old_curve = session
        .project()
        .clip("clip_a")
        .unwrap()
        .speed_curve
        .clone();
    let revision = session
        .apply_command(
            1,
            ProjectCommand::RetargetEvent {
                event_id: "drop_impact".into(),
                new_target_output_tick: target,
                source_bounds_start: Flicks::ZERO,
                source_bounds_end: Flicks(4 * FLICKS_PER_SECOND),
                min_speed: 0.25,
                max_speed: 4.0,
                curve_style: ImpactCurveStyle::TwoSegmentConstant,
                source_fit: SourceFitMode::ExactRange,
            },
        )
        .expect("bounded retarget succeeds");
    assert_eq!(revision, 2);
    assert_eq!(
        session
            .project()
            .event("drop_impact")
            .unwrap()
            .target_output_tick,
        target
    );
    let solved_clip = session.project().clip("clip_a").unwrap();
    assert_ne!(solved_clip.speed_curve, old_curve);
    assert_eq!(
        solved_clip.source_start + solved_clip.speed_curve.source_offset_at(target),
        Flicks(2 * FLICKS_PER_SECOND)
    );

    let snapshot = session.project().clone();
    let invalid = session.apply_command(
        2,
        ProjectCommand::RetargetEvent {
            event_id: "drop_impact".into(),
            new_target_output_tick: Flicks(FLICKS_PER_SECOND),
            source_bounds_start: Flicks::ZERO,
            source_bounds_end: Flicks(4 * FLICKS_PER_SECOND),
            min_speed: 0.5,
            max_speed: 0.75,
            curve_style: ImpactCurveStyle::SmoothRamp,
            source_fit: SourceFitMode::ExactRange,
        },
    );
    assert!(matches!(invalid, Err(SchemaError::InvalidTimeline(_))));
    assert_eq!(session.revision_id(), 2);
    assert_eq!(session.project(), &snapshot);
    assert_eq!(session.undo_len(), 1);
}

#[test]
fn retarget_event_keeps_unlinked_anchor_moves_simple() {
    let mut project = Project::golden_scene_preset();
    let event = project
        .events
        .iter_mut()
        .find(|event| event.event_id == "drop_impact")
        .unwrap();
    event.source_clip_id = None;
    event.source_anchor_tick = None;
    let mut session = ProjectSession::new(project).unwrap();
    let target = Flicks(FLICKS_PER_SECOND / 3);
    session
        .apply_command(
            1,
            ProjectCommand::RetargetEvent {
                event_id: "drop_impact".into(),
                new_target_output_tick: target,
                source_bounds_start: Flicks(10),
                source_bounds_end: Flicks(5),
                min_speed: f64::NAN,
                max_speed: -1.0,
                curve_style: ImpactCurveStyle::SmoothRamp,
                source_fit: SourceFitMode::FitWithinBounds,
            },
        )
        .expect("unlinked anchor is a simple move");
    assert_eq!(
        session
            .project()
            .event("drop_impact")
            .unwrap()
            .target_output_tick,
        target
    );
}

#[test]
fn split_clip_routes_render_sources_and_moves_later_event_link() {
    let mut session = ProjectSession::new(Project::golden_scene_preset()).unwrap();
    let split_tick = Flicks(FLICKS_PER_SECOND);
    session
        .apply_command(
            1,
            ProjectCommand::SplitClip {
                clip_id: "clip_a".into(),
                split_tick,
                right_clip_id: "clip_a_tail".into(),
            },
        )
        .expect("split is valid");

    let graph = &session.project().graph;
    let routing_node = graph.node(&NodeId::from("src_a")).unwrap();
    assert!(matches!(&routing_node.kind, NodeKind::Blend { .. }));
    assert_eq!(routing_node.inputs.len(), 2);
    let branches: Vec<_> = routing_node
        .inputs
        .iter()
        .map(|id| graph.node(id).expect("split source exists"))
        .collect();
    assert!(branches.iter().any(|node| matches!(
        &node.kind,
        NodeKind::SourceVideo { clip_id: Some(id), .. } if id == "clip_a"
    )));
    assert!(branches.iter().any(|node| matches!(
        &node.kind,
        NodeKind::SourceVideo { clip_id: Some(id), .. } if id == "clip_a_tail"
    )));
    assert_eq!(
        session
            .project()
            .event("drop_impact")
            .unwrap()
            .source_clip_id
            .as_deref(),
        Some("clip_a_tail")
    );
    assert_eq!(
        session.project().clip("clip_a").unwrap().timeline_end,
        split_tick
    );
    assert_eq!(
        session
            .project()
            .clip("clip_a_tail")
            .unwrap()
            .timeline_start,
        split_tick
    );
    session
        .project()
        .validate()
        .expect("routed split remains valid");
}

#[test]
fn render_graph_rejects_cycles_missing_nodes_and_bad_arity() {
    // 3-node cycle: a -> b -> c -> a
    let cyclic = RenderGraphDef {
        output_node: NodeId::from("c"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("a"),
                kind: NodeKind::ColorBasic {
                    exposure: 0.0,
                    contrast: 1.0,
                    saturation: 1.0,
                    flash_amount: 0.0,
                },
                inputs: vec![NodeId::from("c")],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("b"),
                kind: NodeKind::ColorBasic {
                    exposure: 0.0,
                    contrast: 1.0,
                    saturation: 1.0,
                    flash_amount: 0.0,
                },
                inputs: vec![NodeId::from("a")],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("c"),
                kind: NodeKind::ColorBasic {
                    exposure: 0.0,
                    contrast: 1.0,
                    saturation: 1.0,
                    flash_amount: 0.0,
                },
                inputs: vec![NodeId::from("b")],
                enabled: true,
            },
        ],
    };

    match cyclic.validate_graph() {
        Err(SchemaError::CycleDetected { cycle }) => {
            assert!(cycle.len() >= 2, "cycle should report participating nodes");
        }
        other => panic!("expected CycleDetected, got {other:?}"),
    }

    // Wrong arity (Blend requires 2 inputs, given 1)
    let bad_arity = RenderGraphDef {
        output_node: NodeId::from("blend"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("solid"),
                kind: NodeKind::Solid {
                    color_rgba: [1.0, 1.0, 1.0, 1.0],
                    color_space: Default::default(),
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("blend"),
                kind: NodeKind::Blend {
                    mode: Default::default(),
                    opacity: 1.0,
                },
                inputs: vec![NodeId::from("solid")],
                enabled: true,
            },
        ],
    };
    assert!(matches!(
        bad_arity.validate_graph(),
        Err(SchemaError::InvalidNodeArity {
            expected: 2,
            actual: 1,
            ..
        })
    ));

    // Missing output node
    let missing_out = RenderGraphDef {
        output_node: NodeId::from("nonexistent"),
        nodes: vec![],
    };
    assert!(matches!(
        missing_out.validate_graph(),
        Err(SchemaError::MissingNode(_))
    ));
}

#[test]
fn schema_version_rule_accepts_older_minors_and_rejects_newer_or_other_majors() {
    // Same major, minor <= current: loadable (ADR-0004).
    assert!(SchemaVersion::new(0, 1, 0).is_compatible_with_current());
    assert!(SchemaVersion::new(0, 2, 0).is_compatible_with_current());
    // Patch never participates in the rule.
    assert!(SchemaVersion::new(0, 1, 9).is_compatible_with_current());
    assert!(SchemaVersion::new(0, 2, 7).is_compatible_with_current());

    // Newer minors and every other major are rejected.
    assert!(!SchemaVersion::new(0, 3, 0).is_compatible_with_current());
    assert!(!SchemaVersion::new(1, 0, 0).is_compatible_with_current());
    assert!(!SchemaVersion::new(1, 2, 0).is_compatible_with_current());
    assert!(!SchemaVersion::new(2, 0, 0).is_compatible_with_current());
}

#[test]
fn legacy_0_1_0_project_loads_with_serde_defaults() {
    let project = Project::golden_scene_preset();
    let mut value = serde_json::to_value(&project).expect("serialize golden preset");
    value["schema_version"] = serde_json::json!("0.1.0");
    // A real 0.1.0 file predates the Phase 2-5 top-level fields.
    let obj = value.as_object_mut().expect("project object");
    obj.remove("param_bindings");
    obj.remove("creative_decisions");
    obj.remove("preferences");
    obj.remove("proxy_config");
    obj.remove("hw_accel_policy");

    let loaded = Project::from_json_str(&serde_json::to_string(&value).expect("legacy json"))
        .expect("0.1.0 project must load");
    assert_eq!(loaded.schema_version, SchemaVersion::new(0, 1, 0));
    assert!(loaded.param_bindings.is_empty());
    assert!(loaded.creative_decisions.is_empty());
    assert!(loaded.preferences.is_empty());
}

#[test]
fn legacy_local_media_0_1_0_projects_load() {
    // Real-world fixtures authored before the 0.2.0 bump are checked when local_media is
    // available, but they are deliberately not compile-time includes so clean checkouts work.
    let fixtures = [
        "../../local_media/asta_amv_project.json",
        "../../local_media/asta_devil_union_project.json",
        "../../local_media/asta_v3/asta_v3_project.json",
        "../../local_media/dogfood/koleedits/koleedits_project.json",
        "../../local_media/fame_is_a_gun/fame_is_a_gun_project.json",
        "../../local_media/fame_is_a_gun/fame_is_a_gun_v3_project.json",
        "../../local_media/mushoku/mushoku_project.json",
    ];

    let mut checked = 0;
    for fixture in fixtures {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(fixture);
        if !path.is_file() {
            continue;
        }
        let json = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read legacy fixture '{}': {e}", path.display()));
        let project = Project::from_json_str(&json).unwrap_or_else(|e| {
            panic!(
                "0.1.0 local_media project '{}' must load: {e}",
                path.display()
            )
        });
        assert_eq!(
            project.schema_version,
            SchemaVersion::new(0, 1, 0),
            "{}",
            path.display()
        );
        assert!(!project.assets.is_empty(), "{}", path.display());
        checked += 1;
    }
    eprintln!("checked {checked} optional legacy local_media fixture(s)");
}

#[test]
fn project_rejects_newer_minor_and_other_major_schema_versions() {
    for (major, minor) in [(0u16, 3u16), (1, 0), (1, 2)] {
        let mut project = Project::golden_scene_preset();
        project.schema_version = SchemaVersion::new(major, minor, 0);
        let json = project.to_json_pretty().expect("serialize project");
        match Project::from_json_str(&json) {
            Err(SchemaError::UnsupportedSchemaVersion(version)) => {
                assert_eq!(version, SchemaVersion::new(major, minor, 0));
            }
            other => panic!(
                "expected UnsupportedSchemaVersion for {major}.{minor}.0, got {:?}",
                other.map(|_| ())
            ),
        }
    }
}

#[test]
fn malformed_schema_version_is_rejected() {
    for raw in ["", "0.1", "0.1.0.0", "v0.1.0", "0.three.0", "-1.0.0"] {
        assert!(
            raw.parse::<SchemaVersion>().is_err(),
            "malformed version '{raw}' must fail to parse"
        );
    }

    // A truncated version inside project JSON surfaces as a load error.
    let project = Project::golden_scene_preset();
    let mut value = serde_json::to_value(&project).expect("serialize golden preset");
    value["schema_version"] = serde_json::json!("0.3");
    let json = serde_json::to_string(&value).expect("encode json");
    assert!(Project::from_json_str(&json).is_err());
}

#[test]
fn project_session_optimistic_locking_atomic_validation_and_undo_redo() {
    let project = Project::golden_scene_preset();
    let mut session = ProjectSession::new(project).expect("session init");
    assert_eq!(session.revision_id(), 1);
    assert!(!session.can_undo());
    assert!(!session.can_redo());

    // Stale expected_rev must be rejected without mutating state
    let conflict = session.apply_command(
        99,
        ProjectCommand::SetProjectName {
            name: "Should Fail".into(),
        },
    );
    assert!(matches!(
        conflict,
        Err(SchemaError::RevisionConflict {
            expected: 99,
            actual: 1
        })
    ));
    assert_eq!(session.revision_id(), 1);

    // Invalid command (introducing a graph cycle) must roll back atomically
    let invalid = session.apply_command(
        1,
        ProjectCommand::UpsertGraphNode(GraphNodeDef {
            id: NodeId::from("grade_flash"),
            kind: NodeKind::ColorBasic {
                exposure: 0.2,
                contrast: 1.0,
                saturation: 1.0,
                flash_amount: 0.5,
            },
            inputs: vec![NodeId::from("motion_blur_out")],
            enabled: true,
        }),
    );
    assert!(matches!(invalid, Err(SchemaError::CycleDetected { .. })));
    assert_eq!(session.revision_id(), 1);
    assert!(!session.can_undo());

    // Valid command 1: move event target
    let new_tick = Flicks(3 * FLICKS_PER_SECOND);
    let rev2 = session
        .apply_command(
            1,
            ProjectCommand::MoveEventTarget {
                event_id: "drop_impact".into(),
                new_target_output_tick: new_tick,
            },
        )
        .expect("move event target");
    assert_eq!(rev2, 2);
    assert_eq!(
        session
            .project()
            .event("drop_impact")
            .unwrap()
            .target_output_tick,
        new_tick
    );

    // Valid command 2: change clip speed curve
    let rev3 = session
        .apply_command(
            2,
            ProjectCommand::SetClipSpeedCurve {
                clip_id: "clip_b".into(),
                speed_curve: SpeedCurve::constant(2.0).unwrap(),
            },
        )
        .expect("set speed curve");
    assert_eq!(rev3, 3);
    assert_eq!(session.undo_len(), 2);

    // Undo command 2 -> revision advances to 4, clip_b speed restored to 1.0
    let rev4 = session.undo(3).expect("undo");
    assert_eq!(rev4, 4);
    assert!(session.can_redo());
    assert_eq!(
        session
            .project()
            .clip("clip_b")
            .unwrap()
            .speed_curve
            .source_offset_at(Flicks(FLICKS_PER_SECOND)),
        Flicks(FLICKS_PER_SECOND)
    );

    // Redo -> revision advances to 5, clip_b speed is 2.0 again
    let rev5 = session.redo(4).expect("redo");
    assert_eq!(rev5, 5);
    assert!(!session.can_redo());
    assert_eq!(
        session
            .project()
            .clip("clip_b")
            .unwrap()
            .speed_curve
            .source_offset_at(Flicks(FLICKS_PER_SECOND)),
        Flicks(2 * FLICKS_PER_SECOND)
    );

    // Undo then apply new command -> redo stack cleared
    let rev6 = session.undo(5).expect("undo");
    assert!(session.can_redo());
    let rev7 = session
        .apply_command(
            rev6,
            ProjectCommand::SetProjectName {
                name: "Revised Golden Scene".into(),
            },
        )
        .expect("rename");
    assert_eq!(rev7, 7);
    assert!(!session.can_redo());
    assert_eq!(session.project().name, "Revised Golden Scene");
    session.validate_graph().expect("graph remains valid");
}

#[test]
fn phase_2_to_5_nodes_bindings_decisions_preferences_and_backward_compat() {
    let preset = Project::golden_scene_preset();

    // Verify VectorText, Lut3D, ParamBindingDef, CreativeDecisionRecord, PreferenceEntry in golden preset
    assert!(preset.graph.nodes.iter().any(|n| matches!(
        &n.kind,
        NodeKind::VectorText { text, .. } if text == "FRAMESHIFT // IMPACT"
    )));
    assert!(preset.graph.nodes.iter().any(|n| matches!(
        &n.kind,
        NodeKind::Lut3D { lut_preset, .. } if lut_preset == "anime_cyber_contrast"
    )));
    assert_eq!(preset.param_bindings.len(), 1);
    assert_eq!(preset.creative_decisions.len(), 1);
    assert_eq!(preset.preferences.len(), 1);
    assert_eq!(preset.hw_accel_policy, HwAccelPolicy::Auto);
    assert_eq!(
        preset.proxy_config,
        ProxyConfigDef {
            enabled: false,
            scale_num: 1,
            scale_den: 2
        }
    );

    // Backward compatibility: legacy JSON omitting Phase 2-5 top-level fields deserializes with defaults
    let mut val: serde_json::Value = serde_json::to_value(&preset).unwrap();
    let obj = val.as_object_mut().unwrap();
    obj.remove("param_bindings");
    obj.remove("creative_decisions");
    obj.remove("preferences");
    obj.remove("proxy_config");
    obj.remove("hw_accel_policy");
    let legacy_json = serde_json::to_string(&val).unwrap();
    let legacy_project = Project::from_json_str(&legacy_json).expect("legacy JSON must validate");
    assert!(legacy_project.param_bindings.is_empty());
    assert!(legacy_project.creative_decisions.is_empty());
    assert!(legacy_project.preferences.is_empty());
    assert_eq!(legacy_project.proxy_config.scale_den, 2);
    assert_eq!(legacy_project.hw_accel_policy, HwAccelPolicy::Auto);

    // Lut3D arity and dependency checks
    let bad_lut_arity = RenderGraphDef {
        output_node: NodeId::from("lut"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("lut"),
            kind: NodeKind::Lut3D {
                input: NodeId::from("src"),
                lut_preset: "anime_cyber_contrast".into(),
                intensity: 1.0,
                lut_source: None,
                input_space: None,
                output_space: None,
            },
            inputs: vec![],
            enabled: true,
        }],
    };
    assert!(matches!(
        bad_lut_arity.validate_graph(),
        Err(SchemaError::InvalidNodeArity {
            expected: 1,
            actual: 0,
            ..
        })
    ));

    let bad_lut_dep = RenderGraphDef {
        output_node: NodeId::from("lut"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("txt"),
                kind: NodeKind::VectorText {
                    text: "HI".into(),
                    font_size_norm: 0.05,
                    color_rgba: [1.0; 4],
                    outline_rgba: [0.0; 4],
                    outline_width_norm: 0.0,
                    align_center: true,
                    y_offset_norm: 0.5,
                    font_family: None,
                    font_path: None,
                    font_weight: 400,
                    align: None,
                    x_offset_norm: None,
                    tracking_em: None,
                    line_height_em: None,
                    opacity: 1.0,
                    fallback_fonts: Vec::new(),
                    writing_mode: Default::default(),
                    var_wght: None,
                    var_wdth: None,
                    var_slnt: None,
                    color_bottom_rgba: None,
                    gradient_angle_deg: 0.0,
                    outer_outline_rgba: [0.0; 4],
                    outer_outline_width_norm: 0.0,
                    outline_gap_norm: 0.0,
                    shadow_offset_norm: [0.0, 0.0],
                    shadow_blur_norm: 0.0,
                    shadow_rgba: [0.0; 4],
                    glow_radius_norm: 0.0,
                    glow_rgba: [0.0; 4],
                    hdr_intensity: 1.0,
                    gpu_slug: true,
                    text_animator: None,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("lut"),
                kind: NodeKind::Lut3D {
                    input: NodeId::from("missing_node"),
                    lut_preset: "anime_cyber_contrast".into(),
                    intensity: 1.0,
                    lut_source: None,
                    input_space: None,
                    output_space: None,
                },
                inputs: vec![NodeId::from("txt")],
                enabled: true,
            },
        ],
    };
    assert!(matches!(
        bad_lut_dep.validate_graph(),
        Err(SchemaError::MissingNode(id)) if id == NodeId::from("missing_node")
    ));

    // Invalid ParamBindingDef expression rejected
    let mut bad_expr_proj = preset.clone();
    bad_expr_proj.param_bindings.push(ParamBindingDef {
        node_id: NodeId::from("grade_flash"),
        param_name: "exposure".into(),
        expression: "unknown_symbol + 1.0".into(),
    });
    assert!(matches!(
        bad_expr_proj.validate(),
        Err(SchemaError::InvalidParamBinding { .. })
    ));

    // Session commands: RecordCreativeDecision & SetPreference (including 3-tier upsert)
    let mut session = ProjectSession::new(preset).unwrap();
    let rev2 = session
        .apply_command(
            1,
            ProjectCommand::RecordCreativeDecision(CreativeDecisionRecord {
                decision_id: "dec_002".into(),
                approach: "Add subtle glow ramp".into(),
                rationale: "Accentuate beat impact".into(),
                evidence_ticks: vec![Flicks(FLICKS_PER_SECOND)],
                protected_invariants: vec!["No clipping".into()],
                risks_to_verify: vec!["Check highlight roll-off".into()],
            }),
        )
        .unwrap();
    assert_eq!(rev2, 2);
    assert_eq!(session.project().creative_decisions.len(), 2);

    let rev3 = session
        .apply_command(
            2,
            ProjectCommand::SetPreference(PreferenceEntry {
                key: "impact_flash_style".into(),
                value: "soft_bloom".into(),
                scope: PreferenceScope::Project,
                target_id: None,
            }),
        )
        .unwrap();
    assert_eq!(rev3, 3);
    assert_eq!(session.project().preferences.len(), 1);
    assert_eq!(session.project().preferences[0].value, "soft_bloom");

    let rev4 = session
        .apply_command(
            3,
            ProjectCommand::SetPreference(PreferenceEntry {
                key: "impact_flash_style".into(),
                value: "scene_hard_cut".into(),
                scope: PreferenceScope::Scene,
                target_id: Some("scene_01".into()),
            }),
        )
        .unwrap();
    assert_eq!(rev4, 4);
    assert_eq!(session.project().preferences.len(), 2);
}

#[test]
fn anime_fx_showcase_preset_and_postfx_nodes_validate_toposort_and_roundtrip() {
    assert_eq!(
        NodeKind::BUILTIN_TRANSITION_SHADERS,
        &[
            "whip_chromatic",
            "cross_zoom",
            "directional_warp",
            "glitch_displace",
            "dreamy_zoom",
            "swirl",
            "linear_blur",
            "burn",
            "ripple",
            "kaleidoscope",
        ]
    );

    let project = Project::anime_fx_showcase_preset();
    let topo = project.validate().expect("anime fx showcase must validate");
    assert_eq!(topo.len(), project.graph.nodes.len());

    // Verify topological order of chained post-FX nodes:
    // MotionTileShake -> AnimeImpactFrame -> RadialShockwave -> HalftoneGlow
    let pos_shake = topo
        .iter()
        .position(|id| id == &NodeId::from("fx_motion_tile_shake"))
        .unwrap();
    let pos_impact = topo
        .iter()
        .position(|id| id == &NodeId::from("fx_anime_impact"))
        .unwrap();
    let pos_shockwave = topo
        .iter()
        .position(|id| id == &NodeId::from("fx_radial_shockwave"))
        .unwrap();
    let pos_halftone = topo
        .iter()
        .position(|id| id == &NodeId::from("fx_halftone_glow"))
        .unwrap();
    assert!(pos_shake < pos_impact);
    assert!(pos_impact < pos_shockwave);
    assert!(pos_shockwave < pos_halftone);

    // Verify transitions are present in graph
    for shader in ["cross_zoom", "directional_warp", "glitch_displace"] {
        assert!(
            project.graph.nodes.iter().any(|n| matches!(
                &n.kind,
                NodeKind::ShaderTransition { shader_name, .. } if shader_name == shader
            )),
            "missing transition shader '{shader}'"
        );
    }

    // Roundtrip JSON
    let json = project.to_json_pretty().expect("serialize anime fx preset");
    let restored = Project::from_json_str(&json).expect("deserialize anime fx preset");
    assert_eq!(project, restored);
}

#[test]
fn anime_postfx_node_kinds_deserialize_defaults_and_enforce_single_input_arity() {
    let minimal_nodes_json = r#"[
        {"id": "n_shake", "kind": {"type": "motion_tile_shake"}, "inputs": ["src"]},
        {"id": "n_impact", "kind": {"type": "anime_impact_frame"}, "inputs": ["n_shake"]},
        {"id": "n_shock", "kind": {"type": "radial_shockwave"}, "inputs": ["n_impact"]},
        {"id": "n_half", "kind": {"type": "halftone_glow"}, "inputs": ["n_shock"]}
    ]"#;
    let parsed: Vec<GraphNodeDef> =
        serde_json::from_str(minimal_nodes_json).expect("minimal postfx json must deserialize");

    assert_eq!(parsed.len(), 4);
    for node in &parsed {
        assert_eq!(node.kind.expected_input_count(), 1);
    }

    assert!(matches!(
        parsed[0].kind,
        NodeKind::MotionTileShake {
            tile_scale,
            mirror_edges: true,
            ..
        } if (tile_scale - 1.0).abs() < 1e-6
    ));
    assert!(matches!(
        parsed[1].kind,
        NodeKind::AnimeImpactFrame {
            luma_threshold,
            edge_weight,
            invert_amount,
            center_uv,
            ..
        } if (luma_threshold - 0.5).abs() < 1e-6
            && (edge_weight - 1.0).abs() < 1e-6
            && (invert_amount - 1.0).abs() < 1e-6
            && center_uv == [0.5, 0.5]
    ));
    assert!(matches!(
        parsed[2].kind,
        NodeKind::RadialShockwave {
            center_uv,
            radius,
            ring_width,
            ..
        } if center_uv == [0.5, 0.5]
            && (radius - 0.25).abs() < 1e-6
            && (ring_width - 0.1).abs() < 1e-6
    ));
    assert!(matches!(
        parsed[3].kind,
        NodeKind::HalftoneGlow {
            glow_threshold,
            ..
        } if (glow_threshold - 0.5).abs() < 1e-6
    ));

    // Verify arity enforcement when input is missing
    let bad_arity = RenderGraphDef {
        output_node: NodeId::from("n_shake"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("n_shake"),
            kind: parsed[0].kind.clone(),
            inputs: vec![],
            enabled: true,
        }],
    };
    assert!(matches!(
        bad_arity.validate_graph(),
        Err(SchemaError::InvalidNodeArity {
            expected: 1,
            actual: 0,
            ..
        })
    ));
}

#[test]
fn author_hard_cuts_emits_nonoverlapping_clips_and_blend_chain() {
    let sec = FLICKS_PER_SECOND;
    let mut session = ProjectSession::new(Project::golden_scene_preset()).expect("session init");
    let before_output = session.project().graph.output_node.clone();
    let before_clips = session.project().timeline.clips.len();
    let before_nodes = session.project().graph.nodes.len();

    let cmd = ProjectCommand::AuthorHardCuts {
        cuts: vec![
            HardCut {
                clip_id: "cut_0".into(),
                asset_id: "asset_clip_a".into(),
                timeline_start: Flicks::ZERO,
                timeline_end: Flicks(sec),
                source_start: Flicks::ZERO,
                speed_curve: None,
            },
            HardCut {
                clip_id: "cut_1".into(),
                asset_id: "asset_clip_b".into(),
                timeline_start: Flicks(sec),
                timeline_end: Flicks(2 * sec),
                source_start: Flicks(sec / 2),
                speed_curve: Some(SpeedCurve::constant(1.0).unwrap()),
            },
            HardCut {
                clip_id: "cut_2".into(),
                asset_id: "asset_clip_a".into(),
                timeline_start: Flicks(2 * sec),
                timeline_end: Flicks(3 * sec),
                source_start: Flicks(4 * sec),
                speed_curve: None,
            },
        ],
    };

    let json = serde_json::to_value(&cmd).expect("command json");
    assert_eq!(json["command"], "author_hard_cuts");
    assert!(json["cuts"][0]["timeline_start"].is_i64());
    assert_eq!(json["cuts"][0]["timeline_end"].as_i64(), Some(sec));
    assert_eq!(json["cuts"][1]["timeline_start"].as_i64(), Some(sec));
    assert_eq!(json["cuts"][2]["source_start"].as_i64(), Some(4 * sec));
    assert!(json["cuts"][0].get("speed_curve").is_none());
    let parsed: ProjectCommand = serde_json::from_value(json).expect("roundtrip");

    let rev = session
        .apply_command(session.revision_id(), parsed)
        .expect("author hard cuts");
    let authored = session.project();
    assert_eq!(authored.timeline.clips.len(), before_clips + 3);

    let cut_0 = authored.clip("cut_0").expect("cut_0");
    assert_eq!(cut_0.timeline_start, Flicks::ZERO);
    assert_eq!(cut_0.timeline_end, Flicks(sec));
    assert_eq!(cut_0.source_start, Flicks::ZERO);
    assert_eq!(cut_0.speed_curve.source_offset_at(Flicks(sec)), Flicks(sec));
    let cut_1 = authored.clip("cut_1").expect("cut_1");
    assert_eq!(cut_1.timeline_start, Flicks(sec));
    assert_eq!(cut_1.source_start, Flicks(sec / 2));
    let cut_2 = authored.clip("cut_2").expect("cut_2");
    assert_eq!(cut_2.timeline_end, Flicks(3 * sec));
    assert_eq!(cut_2.source_start, Flicks(4 * sec));
    assert!(authored.clip("clip_a").is_some());

    let src0 = authored
        .graph
        .node(&NodeId::from("cut_src_cut_0"))
        .expect("source for first cut");
    assert!(matches!(
        &src0.kind,
        NodeKind::SourceVideo {
            clip_id: Some(id),
            asset_id,
            ..
        } if id == "cut_0" && asset_id == "asset_clip_a"
    ));
    assert!(src0.inputs.is_empty());

    let blend1 = authored
        .graph
        .node(&NodeId::from("cut_blend_cut_1"))
        .expect("first blend");
    assert!(matches!(
        blend1.kind,
        NodeKind::Blend {
            mode: BlendMode::Normal,
            opacity,
        } if opacity == 1.0
    ));
    assert_eq!(
        blend1.inputs,
        vec![NodeId::from("cut_src_cut_0"), NodeId::from("cut_src_cut_1")]
    );
    let blend2 = authored
        .graph
        .node(&NodeId::from("cut_blend_cut_2"))
        .expect("second blend");
    assert_eq!(
        blend2.inputs,
        vec![
            NodeId::from("cut_blend_cut_1"),
            NodeId::from("cut_src_cut_2")
        ]
    );
    assert_eq!(authored.graph.output_node, NodeId::from("cut_blend_cut_2"));
    assert_eq!(authored.graph.nodes.len(), before_nodes + 5);

    let topo = session.validate_graph().expect("blend chain is a DAG");
    let pos = |id: &str| {
        topo.iter()
            .position(|node| node == &NodeId::from(id))
            .expect(id)
    };
    assert!(pos("cut_src_cut_0") < pos("cut_blend_cut_1"));
    assert!(pos("cut_src_cut_1") < pos("cut_blend_cut_1"));
    assert!(pos("cut_blend_cut_1") < pos("cut_blend_cut_2"));
    assert!(pos("cut_src_cut_2") < pos("cut_blend_cut_2"));

    let undone = session.undo(rev).expect("undo hard cuts");
    assert_eq!(session.project().timeline.clips.len(), before_clips);
    assert_eq!(session.project().graph.nodes.len(), before_nodes);
    assert_eq!(session.project().graph.output_node, before_output);
    assert!(session.project().clip("cut_0").is_none());

    session
        .apply_command(
            undone,
            ProjectCommand::AuthorHardCuts {
                cuts: vec![HardCut {
                    clip_id: "solo".into(),
                    asset_id: "asset_clip_a".into(),
                    timeline_start: Flicks::ZERO,
                    timeline_end: Flicks(sec),
                    source_start: Flicks(10),
                    speed_curve: None,
                }],
            },
        )
        .expect("single cut");
    assert_eq!(
        session.project().graph.output_node,
        NodeId::from("cut_src_solo")
    );
    assert!(
        session
            .project()
            .graph
            .nodes
            .iter()
            .all(|node| node.id.0 != "cut_blend_solo")
    );
    assert_eq!(
        session.project().clip("solo").unwrap().source_start,
        Flicks(10)
    );

    let mut gapped = ProjectSession::new(Project::golden_scene_preset()).unwrap();
    gapped
        .apply_command(
            1,
            ProjectCommand::AuthorHardCuts {
                cuts: vec![
                    HardCut {
                        clip_id: "gap_a".into(),
                        asset_id: "asset_clip_a".into(),
                        timeline_start: Flicks::ZERO,
                        timeline_end: Flicks(sec),
                        source_start: Flicks::ZERO,
                        speed_curve: None,
                    },
                    HardCut {
                        clip_id: "gap_b".into(),
                        asset_id: "asset_clip_b".into(),
                        timeline_start: Flicks(2 * sec),
                        timeline_end: Flicks(3 * sec),
                        source_start: Flicks::ZERO,
                        speed_curve: None,
                    },
                ],
            },
        )
        .expect("gap is non-overlapping");
    assert_eq!(
        gapped.project().graph.output_node,
        NodeId::from("cut_blend_gap_b")
    );
    gapped.validate_graph().expect("gapped chain validates");

    let mut rejected = ProjectSession::new(Project::golden_scene_preset()).unwrap();
    let overlap = rejected.apply_command(
        1,
        ProjectCommand::AuthorHardCuts {
            cuts: vec![
                HardCut {
                    clip_id: "ov_a".into(),
                    asset_id: "asset_clip_a".into(),
                    timeline_start: Flicks::ZERO,
                    timeline_end: Flicks(2 * sec),
                    source_start: Flicks::ZERO,
                    speed_curve: None,
                },
                HardCut {
                    clip_id: "ov_b".into(),
                    asset_id: "asset_clip_b".into(),
                    timeline_start: Flicks(sec),
                    timeline_end: Flicks(3 * sec),
                    source_start: Flicks::ZERO,
                    speed_curve: None,
                },
            ],
        },
    );
    assert!(matches!(
        overlap,
        Err(SchemaError::InvalidTimeline(msg))
            if msg.contains("ov_b") && msg.contains("non-overlapping")
    ));
    assert_eq!(rejected.revision_id(), 1);
    assert!(rejected.project().clip("ov_a").is_none());
    assert_eq!(rejected.project().graph.output_node, before_output);

    let reversed = rejected.apply_command(
        1,
        ProjectCommand::AuthorHardCuts {
            cuts: vec![
                HardCut {
                    clip_id: "late".into(),
                    asset_id: "asset_clip_a".into(),
                    timeline_start: Flicks(2 * sec),
                    timeline_end: Flicks(3 * sec),
                    source_start: Flicks::ZERO,
                    speed_curve: None,
                },
                HardCut {
                    clip_id: "early".into(),
                    asset_id: "asset_clip_b".into(),
                    timeline_start: Flicks::ZERO,
                    timeline_end: Flicks(sec),
                    source_start: Flicks::ZERO,
                    speed_curve: None,
                },
            ],
        },
    );
    assert!(matches!(reversed, Err(SchemaError::InvalidTimeline(_))));
    assert_eq!(rejected.revision_id(), 1);

    let missing = rejected.apply_command(
        1,
        ProjectCommand::AuthorHardCuts {
            cuts: vec![HardCut {
                clip_id: "ghost".into(),
                asset_id: "no_such_asset".into(),
                timeline_start: Flicks::ZERO,
                timeline_end: Flicks(sec),
                source_start: Flicks::ZERO,
                speed_curve: None,
            }],
        },
    );
    assert!(matches!(
        missing,
        Err(SchemaError::MissingAsset(id)) if id == "no_such_asset"
    ));

    let empty = rejected.apply_command(1, ProjectCommand::AuthorHardCuts { cuts: vec![] });
    assert!(matches!(empty, Err(SchemaError::InvalidTimeline(_))));
    assert_eq!(rejected.revision_id(), 1);
}

#[test]
fn omitted_fit_and_enabled_default_and_disabled_passthrough_is_base() {
    let source: GraphNodeDef = serde_json::from_str(
        r#"{"id":"src","kind":{"type":"source_video","asset_id":"a"},"inputs":[]}"#,
    )
    .expect("legacy source json");
    assert!(source.enabled);
    assert_eq!(source.disabled_passthrough(), None);
    assert_eq!(source.kind.source_fit(), Some(SourceFit::Stretch));

    let grade: GraphNodeDef =
        serde_json::from_str(r#"{"id":"grade","kind":{"type":"color_basic"},"inputs":["src"]}"#)
            .expect("legacy grade json");
    assert!(grade.enabled);
    assert_eq!(grade.disabled_passthrough(), None);
    assert!(grade.kind.source_fit().is_none());

    let mut off_grade = grade.clone();
    off_grade.enabled = false;
    assert_eq!(
        off_grade.disabled_passthrough(),
        Some(DisabledPassthrough::Input(0))
    );

    let blend: GraphNodeDef = serde_json::from_str(
        r#"{"id":"over","kind":{"type":"blend"},"enabled":false,"inputs":["base","top"]}"#,
    )
    .expect("disabled blend");
    assert!(!blend.enabled);
    assert_eq!(
        blend.disabled_passthrough(),
        Some(DisabledPassthrough::Input(0))
    );
    let blend_again: GraphNodeDef =
        serde_json::from_str(&serde_json::to_string(&blend).expect("serialize blend"))
            .expect("roundtrip blend");
    assert_eq!(blend_again, blend);

    let cover: NodeKind =
        serde_json::from_str(r#"{"type":"source_video","asset_id":"a","fit":"cover"}"#)
            .expect("cover");
    assert_eq!(cover.source_fit(), Some(SourceFit::Cover));
    let contain: NodeKind =
        serde_json::from_str(r#"{"type":"source_video","asset_id":"a","fit":"contain"}"#)
            .expect("contain");
    assert_eq!(contain.source_fit(), Some(SourceFit::Contain));
    let stretch_again: NodeKind =
        serde_json::from_str(&serde_json::to_string(&contain).expect("serialize contain"))
            .expect("roundtrip contain");
    assert_eq!(stretch_again.source_fit(), Some(SourceFit::Contain));

    let off_solid: GraphNodeDef = serde_json::from_str(
        r#"{"id":"solid","kind":{"type":"solid","color_rgba":[1.0,0.0,0.0,1.0]},"enabled":false,"inputs":[]}"#,
    )
    .expect("disabled solid");
    assert_eq!(
        off_solid.disabled_passthrough(),
        Some(DisabledPassthrough::Clear)
    );

    let golden = Project::golden_scene_preset();
    assert!(golden.graph.nodes.iter().all(|node| node.enabled));
    assert!(
        golden
            .graph
            .nodes
            .iter()
            .all(|node| { matches!(node.kind.source_fit(), None | Some(SourceFit::Stretch)) })
    );
}

#[test]
fn phase_6_1_postfx_and_custom_wgsl_defaults_arity_and_validation() {
    let minimal_nodes_json = r#"[
        {"id": "n_glow", "kind": {"type": "deep_glow"}, "inputs": ["src"]},
        {"id": "n_sharp", "kind": {"type": "anime_sharpen"}, "inputs": ["n_glow"]},
        {"id": "n_optics", "kind": {"type": "film_optics"}, "inputs": ["n_sharp"]},
        {"id": "n_wgsl", "kind": {"type": "custom_wgsl", "wgsl_source": "return vec4<f32>(1.0);", "expected_inputs": 1}, "inputs": ["n_optics"]}
    ]"#;
    let parsed: Vec<GraphNodeDef> =
        serde_json::from_str(minimal_nodes_json).expect("minimal phase 6.1 json must deserialize");
    assert_eq!(parsed.len(), 4);
    for node in &parsed {
        assert_eq!(node.kind.expected_input_count(), 1);
    }

    assert!(matches!(
        parsed[0].kind,
        NodeKind::DeepGlow {
            threshold,
            knee,
            intensity,
            radius,
            tint_rgb,
            mip_levels: 5,
            scatter,
            anamorphic_ratio,
            chromatic_shift,
        } if (threshold - 0.8).abs() < 1e-6
            && (knee - 0.5).abs() < 1e-6
            && (intensity - 0.65).abs() < 1e-6
            && (radius - 1.0).abs() < 1e-6
            && tint_rgb == [1.0, 1.0, 1.0]
            && (scatter - 0.65).abs() < 1e-6
            && anamorphic_ratio.abs() < 1e-6
            && chromatic_shift.abs() < 1e-6
    ));
    assert!(matches!(
        parsed[1].kind,
        NodeKind::AnimeSharpen {
            line_strength,
            sharpen_amount,
            anti_ringing,
        } if (line_strength - 0.35).abs() < 1e-6
            && (sharpen_amount - 0.45).abs() < 1e-6
            && (anti_ringing - 0.8).abs() < 1e-6
    ));
    assert!(matches!(
        parsed[2].kind,
        NodeKind::FilmOptics {
            lens_k1,
            lens_k2,
            chromatic_edge,
            grain_amount,
            grain_seed,
            halation_amount,
            vignette_amount,
            auto_scale: true,
            grain_size_px,
            grain_chroma,
            halation_threshold,
        } if lens_k1.abs() < 1e-6
            && lens_k2.abs() < 1e-6
            && (chromatic_edge - 0.004).abs() < 1e-6
            && (grain_amount - 0.04).abs() < 1e-6
            && (grain_seed - 1.0).abs() < 1e-6
            && (halation_amount - 0.15).abs() < 1e-6
            && (vignette_amount - 0.2).abs() < 1e-6
            && (grain_size_px - 1.65).abs() < 1e-6
            && (grain_chroma - 0.25).abs() < 1e-6
            && (halation_threshold - 0.65).abs() < 1e-6
    ));
    assert!(matches!(
        &parsed[3].kind,
        NodeKind::CustomWgsl {
            wgsl_source,
            expected_inputs: 1,
            params,
        } if wgsl_source == "return vec4<f32>(1.0);" && *params == [0.0; 16]
    ));

    // expected_input_count clamps values > 2 to 2, while validate_graph rejects expected_inputs > 2.
    let clamped = NodeKind::CustomWgsl {
        wgsl_source: "return vec4<f32>(1.0);".into(),
        expected_inputs: 5,
        params: [0.0; 16],
    };
    assert_eq!(clamped.expected_input_count(), 2);

    let bad_inputs_graph = RenderGraphDef {
        output_node: NodeId::from("n_wgsl"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("s0"),
                kind: NodeKind::Solid {
                    color_rgba: [1.0; 4],
                    color_space: Default::default(),
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("s1"),
                kind: NodeKind::Solid {
                    color_rgba: [0.5; 4],
                    color_space: Default::default(),
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("n_wgsl"),
                kind: clamped,
                inputs: vec![NodeId::from("s0"), NodeId::from("s1")],
                enabled: true,
            },
        ],
    };
    assert!(matches!(
        bad_inputs_graph.validate_graph(),
        Err(SchemaError::InvalidCustomWgsl { node_id, message })
            if node_id == NodeId::from("n_wgsl") && message.contains("expected_inputs")
    ));

    let empty_wgsl_graph = RenderGraphDef {
        output_node: NodeId::from("n_wgsl"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("n_wgsl"),
            kind: NodeKind::CustomWgsl {
                wgsl_source: "   ".into(),
                expected_inputs: 0,
                params: [0.0; 16],
            },
            inputs: vec![],
            enabled: true,
        }],
    };
    assert!(matches!(
        empty_wgsl_graph.validate_graph(),
        Err(SchemaError::InvalidCustomWgsl { node_id, message })
            if node_id == NodeId::from("n_wgsl") && message.contains("wgsl_source")
    ));

    let valid_two_input_wgsl = RenderGraphDef {
        output_node: NodeId::from("n_wgsl"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("s0"),
                kind: NodeKind::Solid {
                    color_rgba: [1.0; 4],
                    color_space: Default::default(),
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("s1"),
                kind: NodeKind::Solid {
                    color_rgba: [0.5; 4],
                    color_space: Default::default(),
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("n_wgsl"),
                kind: NodeKind::CustomWgsl {
                    wgsl_source: "return textureSample(tex_a, linear_sampler, uv);".into(),
                    expected_inputs: 2,
                    params: [0.0; 16],
                },
                inputs: vec![NodeId::from("s0"), NodeId::from("s1")],
                enabled: true,
            },
        ],
    };
    let topo = valid_two_input_wgsl
        .validate_graph()
        .expect("2-input custom_wgsl graph must validate");
    assert_eq!(topo.len(), 3);
}

#[test]
fn phase_6_1_safe_expr_param_bindings_whitelist_and_rejections() {
    let mut project = Project::golden_scene_preset();
    project.graph = RenderGraphDef {
        output_node: NodeId::from("n_wgsl"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("src"),
                kind: NodeKind::Solid {
                    color_rgba: [1.0; 4],
                    color_space: Default::default(),
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("n_glow"),
                kind: NodeKind::DeepGlow {
                    threshold: 0.8,
                    knee: 0.5,
                    intensity: 0.65,
                    radius: 1.0,
                    tint_rgb: [1.0, 1.0, 1.0],
                    mip_levels: 5,
                    scatter: 0.65,
                    anamorphic_ratio: 0.0,
                    chromatic_shift: 0.0,
                },
                inputs: vec![NodeId::from("src")],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("n_sharp"),
                kind: NodeKind::AnimeSharpen {
                    line_strength: 0.35,
                    sharpen_amount: 0.45,
                    anti_ringing: 0.8,
                },
                inputs: vec![NodeId::from("n_glow")],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("n_optics"),
                kind: NodeKind::FilmOptics {
                    lens_k1: 0.0,
                    lens_k2: 0.0,
                    chromatic_edge: 0.004,
                    grain_amount: 0.04,
                    grain_seed: 1.0,
                    halation_amount: 0.15,
                    vignette_amount: 0.2,
                    auto_scale: true,
                    grain_size_px: 1.65,
                    grain_chroma: 0.25,
                    halation_threshold: 0.65,
                },
                inputs: vec![NodeId::from("n_sharp")],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("n_wgsl"),
                kind: NodeKind::CustomWgsl {
                    wgsl_source: "return vec4<f32>(u.params[0].x, 0.0, 0.0, 1.0);".into(),
                    expected_inputs: 1,
                    params: [0.0; 16],
                },
                inputs: vec![NodeId::from("n_optics")],
                enabled: true,
            },
        ],
    };
    project.timeline.clips[0].event_animations.clear();
    project.param_bindings.clear();

    for param in ["threshold", "knee", "intensity", "radius"] {
        project.param_bindings.push(ParamBindingDef {
            node_id: NodeId::from("n_glow"),
            param_name: param.into(),
            expression: "0.5 + 0.1 * sin(t_sec)".into(),
        });
    }
    for param in ["line_strength", "sharpen_amount", "anti_ringing"] {
        project.param_bindings.push(ParamBindingDef {
            node_id: NodeId::from("n_sharp"),
            param_name: param.into(),
            expression: "clamp(audio_rms, 0.0, 1.0)".into(),
        });
    }
    for param in [
        "lens_k1",
        "lens_k2",
        "chromatic_edge",
        "grain_amount",
        "grain_seed",
        "halation_amount",
        "vignette_amount",
    ] {
        project.param_bindings.push(ParamBindingDef {
            node_id: NodeId::from("n_optics"),
            param_name: param.into(),
            expression: "0.25".into(),
        });
    }
    for idx in 0..16 {
        project.param_bindings.push(ParamBindingDef {
            node_id: NodeId::from("n_wgsl"),
            param_name: format!("p{idx}"),
            expression: "beat_phase".into(),
        });
    }

    project
        .validate()
        .expect("all Phase 6.1 whitelisted params must validate");

    // Roundtrip JSON
    let json = project.to_json_pretty().expect("serialize");
    let restored = Project::from_json_str(&json).expect("roundtrip");
    assert_eq!(project, restored);

    // Reject unknown param on CustomWgsl (p16)
    let mut bad_p16 = project.clone();
    bad_p16.param_bindings.push(ParamBindingDef {
        node_id: NodeId::from("n_wgsl"),
        param_name: "p16".into(),
        expression: "1.0".into(),
    });
    assert!(matches!(
        bad_p16.validate(),
        Err(SchemaError::InvalidParamBinding { param_name, .. }) if param_name == "p16"
    ));

    // Reject non-whitelisted field on DeepGlow (mip_levels)
    let mut bad_mip = project;
    bad_mip.param_bindings.push(ParamBindingDef {
        node_id: NodeId::from("n_glow"),
        param_name: "mip_levels".into(),
        expression: "4.0".into(),
    });
    assert!(matches!(
        bad_mip.validate(),
        Err(SchemaError::InvalidParamBinding { param_name, .. }) if param_name == "mip_levels"
    ));
}

#[test]
fn phase_6_2_3d_nodes_defaults_arity_and_validation() {
    let minimal_nodes_json = r#"[
        {"id": "n_text3d", "kind": {"type": "text3_d", "text": "BANG!", "font_path": "C:/Windows/Fonts/arial.ttf"}, "inputs": []},
        {"id": "n_xform3d", "kind": {"type": "transform3_d"}, "inputs": ["n_text3d"]},
        {"id": "n_particles", "kind": {"type": "particle_field3_d"}, "inputs": []},
        {"id": "n_ribbons", "kind": {"type": "ribbon_trail3_d"}, "inputs": []}
    ]"#;
    let parsed: Vec<GraphNodeDef> =
        serde_json::from_str(minimal_nodes_json).expect("minimal phase 6.2 json must deserialize");
    assert_eq!(parsed.len(), 4);
    assert_eq!(parsed[0].kind.expected_input_count(), 0);
    assert_eq!(parsed[1].kind.expected_input_count(), 1);
    assert_eq!(parsed[2].kind.expected_input_count(), 0);
    assert_eq!(parsed[3].kind.expected_input_count(), 0);

    assert!(matches!(
        &parsed[0].kind,
        NodeKind::Text3D {
            text,
            font_path,
            font_size,
            extrude_depth,
            bevel_size,
            color_top_rgba,
            color_bottom_rgba,
            side_color_rgba,
            pos,
            rot_deg,
            camera_pos,
            camera_rot_deg,
            fov_deg,
            char_stagger_sec,
            char_bounce_amp,
            char_wave_freq,
            time_offset_sec,
            ..
        } if text == "BANG!"
            && font_path == "C:/Windows/Fonts/arial.ttf"
            && (*font_size - 0.45).abs() < 1e-6
            && (*extrude_depth - 0.12).abs() < 1e-6
            && (*bevel_size - 0.01).abs() < 1e-6
            && *color_top_rgba == [1.0, 0.95, 1.0, 1.0]
            && *color_bottom_rgba == [1.0, 0.2, 0.75, 1.0]
            && *side_color_rgba == [0.25, 0.05, 0.45, 1.0]
            && *pos == [0.0, 0.0, 0.0]
            && *rot_deg == [0.0, 0.0, 0.0]
            && *camera_pos == [0.0, 0.0, 2.5]
            && *camera_rot_deg == [0.0, 0.0, 0.0]
            && (*fov_deg - 50.0).abs() < 1e-6
            && (*char_stagger_sec - 0.06).abs() < 1e-6
            && (*char_bounce_amp - 0.08).abs() < 1e-6
            && (*char_wave_freq - 4.0).abs() < 1e-6
            && time_offset_sec.abs() < 1e-6
    ));

    assert!(matches!(
        &parsed[1].kind,
        NodeKind::Transform3D {
            pos,
            rot_deg,
            scale,
            fov_deg,
            camera_pos,
            camera_rot_deg,
            dof_focal_dist,
            dof_blur_strength,
            mirror_edges: false,
            parent: None,
            pivot,
        } if *pos == [0.0, 0.0, 0.0]
            && *rot_deg == [0.0, 0.0, 0.0]
            && *scale == [1.0, 1.0, 1.0]
            && (*fov_deg - 50.0).abs() < 1e-6
            && *camera_pos == [0.0, 0.0, 2.5]
            && *camera_rot_deg == [0.0, 0.0, 0.0]
            && (*dof_focal_dist - 2.5).abs() < 1e-6
            && dof_blur_strength.abs() < 1e-6
            && *pivot == [0.0, 0.0, 0.0]
    ));

    assert!(matches!(
        parsed[2].kind,
        NodeKind::ParticleField3D {
            count: 120,
            seed: 1,
            shape: 1,
            color_a_rgba,
            color_b_rgba,
            speed,
            spread,
            size,
            camera_pos,
            camera_rot_deg,
            fov_deg,
            time_offset_sec,
            dof_focal_dist,
            dof_blur_strength,
            ..
        } if color_a_rgba == [1.0, 0.3, 0.85, 1.0]
            && color_b_rgba == [0.2, 0.9, 1.0, 1.0]
            && (speed - 0.8).abs() < 1e-6
            && (spread - 2.2).abs() < 1e-6
            && (size - 0.06).abs() < 1e-6
            && camera_pos == [0.0, 0.0, 2.5]
            && camera_rot_deg == [0.0, 0.0, 0.0]
            && (fov_deg - 50.0).abs() < 1e-6
            && time_offset_sec.abs() < 1e-6
            && (dof_focal_dist - 2.5).abs() < 1e-6
            && (dof_blur_strength - 0.35).abs() < 1e-6
    ));

    assert!(matches!(
        parsed[3].kind,
        NodeKind::RibbonTrail3D {
            ribbon_count: 4,
            segments: 48,
            seed: 1,
            width,
            speed,
            radius,
            trim_start,
            trim_end,
            color_a_rgba,
            color_b_rgba,
            camera_pos,
            camera_rot_deg,
            fov_deg,
            time_offset_sec,
            ..
        } if (width - 0.035).abs() < 1e-6
            && (speed - 1.0).abs() < 1e-6
            && (radius - 1.1).abs() < 1e-6
            && trim_start.abs() < 1e-6
            && (trim_end - 1.0).abs() < 1e-6
            && color_a_rgba == [1.0, 0.15, 0.7, 1.0]
            && color_b_rgba == [0.1, 0.95, 1.0, 1.0]
            && camera_pos == [0.0, 0.0, 2.5]
            && camera_rot_deg == [0.0, 0.0, 0.0]
            && (fov_deg - 50.0).abs() < 1e-6
            && time_offset_sec.abs() < 1e-6
    ));

    // Empty or whitespace-only font_path on Text3D must fail graph validation
    let empty_font_graph = RenderGraphDef {
        output_node: NodeId::from("n_text3d"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("n_text3d"),
            kind: serde_json::from_str(r#"{"type": "text3_d", "text": "HI", "font_path": "  "}"#)
                .unwrap(),
            inputs: vec![],
            enabled: true,
        }],
    };
    assert!(matches!(
        empty_font_graph.validate_graph(),
        Err(SchemaError::InvalidText3D { node_id, message })
            if node_id == NodeId::from("n_text3d") && message.contains("font_path")
    ));

    // Transform3D requires 1 input
    let bad_xform_arity = RenderGraphDef {
        output_node: NodeId::from("n_xform3d"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("n_xform3d"),
            kind: parsed[1].kind.clone(),
            inputs: vec![],
            enabled: true,
        }],
    };
    assert!(matches!(
        bad_xform_arity.validate_graph(),
        Err(SchemaError::InvalidNodeArity {
            expected: 1,
            actual: 0,
            ..
        })
    ));

    // Text3D, ParticleField3D, RibbonTrail3D require 0 inputs
    for (idx, kind) in [0usize, 2, 3].into_iter().map(|i| (i, &parsed[i].kind)) {
        let bad_gen_arity = RenderGraphDef {
            output_node: NodeId::from("n_gen"),
            nodes: vec![
                GraphNodeDef {
                    id: NodeId::from("s"),
                    kind: NodeKind::Solid {
                        color_rgba: [1.0; 4],
                        color_space: Default::default(),
                    },
                    inputs: vec![],
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("n_gen"),
                    kind: kind.clone(),
                    inputs: vec![NodeId::from("s")],
                    enabled: true,
                },
            ],
        };
        assert!(
            matches!(
                bad_gen_arity.validate_graph(),
                Err(SchemaError::InvalidNodeArity {
                    expected: 0,
                    actual: 1,
                    ..
                })
            ),
            "node index {idx} must reject non-zero inputs"
        );
    }
}

#[test]
fn phase_6_2_safe_expr_param_bindings_whitelist_and_rejections() {
    let mut project = Project::golden_scene_preset();
    let parsed: Vec<GraphNodeDef> = serde_json::from_str(
        r#"[
            {"id": "n_text3d", "kind": {"type": "text3_d", "text": "BANG!", "font_path": "C:/Windows/Fonts/arial.ttf"}, "inputs": []},
            {"id": "n_xform3d", "kind": {"type": "transform3_d"}, "inputs": ["n_text3d"]},
            {"id": "n_particles", "kind": {"type": "particle_field3_d"}, "inputs": []},
            {"id": "n_ribbons", "kind": {"type": "ribbon_trail3_d"}, "inputs": []}
        ]"#,
    )
    .unwrap();

    let mut nodes = parsed;
    nodes.push(GraphNodeDef {
        id: NodeId::from("blend_pr"),
        kind: NodeKind::Blend {
            mode: BlendMode::Add,
            opacity: 1.0,
        },
        inputs: vec![NodeId::from("n_particles"), NodeId::from("n_ribbons")],
        enabled: true,
    });
    nodes.push(GraphNodeDef {
        id: NodeId::from("out_blend"),
        kind: NodeKind::Blend {
            mode: BlendMode::Normal,
            opacity: 1.0,
        },
        inputs: vec![NodeId::from("n_xform3d"), NodeId::from("blend_pr")],
        enabled: true,
    });

    project.graph = RenderGraphDef {
        output_node: NodeId::from("out_blend"),
        nodes,
    };
    project.timeline.clips[0].event_animations.clear();
    project.param_bindings.clear();

    for param in [
        "pos_x",
        "pos_y",
        "pos_z",
        "rot_x",
        "rot_y",
        "rot_z",
        "scale_x",
        "scale_y",
        "scale_z",
        "cam_x",
        "cam_y",
        "cam_z",
        "cam_rot_x",
        "cam_rot_y",
        "cam_rot_z",
        "fov_deg",
        "dof_focal_dist",
        "dof_blur_strength",
    ] {
        project.param_bindings.push(ParamBindingDef {
            node_id: NodeId::from("n_xform3d"),
            param_name: param.into(),
            expression: "0.5 + 0.1 * sin(t_sec)".into(),
        });
    }

    for param in [
        "pos_x",
        "pos_y",
        "pos_z",
        "rot_x",
        "rot_y",
        "rot_z",
        "cam_x",
        "cam_y",
        "cam_z",
        "cam_rot_x",
        "cam_rot_y",
        "cam_rot_z",
        "fov_deg",
        "extrude_depth",
        "char_bounce_amp",
        "time_offset_sec",
    ] {
        project.param_bindings.push(ParamBindingDef {
            node_id: NodeId::from("n_text3d"),
            param_name: param.into(),
            expression: "t_sec".into(),
        });
    }

    for param in [
        "speed",
        "spread",
        "size",
        "cam_x",
        "cam_y",
        "cam_z",
        "cam_rot_x",
        "cam_rot_y",
        "cam_rot_z",
        "fov_deg",
        "time_offset_sec",
        "dof_focal_dist",
        "dof_blur_strength",
    ] {
        project.param_bindings.push(ParamBindingDef {
            node_id: NodeId::from("n_particles"),
            param_name: param.into(),
            expression: "1.0 + beat_phase".into(),
        });
    }

    for param in [
        "width",
        "speed",
        "radius",
        "trim_start",
        "trim_end",
        "cam_x",
        "cam_y",
        "cam_z",
        "cam_rot_x",
        "cam_rot_y",
        "cam_rot_z",
        "fov_deg",
        "time_offset_sec",
    ] {
        project.param_bindings.push(ParamBindingDef {
            node_id: NodeId::from("n_ribbons"),
            param_name: param.into(),
            expression: "clamp(t_sec, 0.0, 1.0)".into(),
        });
    }

    project
        .validate()
        .expect("all Phase 6.2 whitelisted params must validate");

    let json = project.to_json_pretty().expect("serialize phase 6.2");
    let restored = Project::from_json_str(&json).expect("roundtrip phase 6.2");
    assert_eq!(project, restored);

    // Reject non-whitelisted params
    let mut bad_particle_count = project.clone();
    bad_particle_count.param_bindings.push(ParamBindingDef {
        node_id: NodeId::from("n_particles"),
        param_name: "count".into(),
        expression: "100.0".into(),
    });
    assert!(matches!(
        bad_particle_count.validate(),
        Err(SchemaError::InvalidParamBinding { param_name, .. }) if param_name == "count"
    ));

    let mut bad_xform_extrude = project;
    bad_xform_extrude.param_bindings.push(ParamBindingDef {
        node_id: NodeId::from("n_xform3d"),
        param_name: "extrude_depth".into(),
        expression: "0.2".into(),
    });
    assert!(matches!(
        bad_xform_extrude.validate(),
        Err(SchemaError::InvalidParamBinding { param_name, .. }) if param_name == "extrude_depth"
    ));
}

#[test]
fn node_kind_natural_2d_and_3d_serde_aliases_deserialize() {
    let text_3d: NodeKind = serde_json::from_str(
        r#"{"type": "text_3d", "text": "ALIAS", "font_path": "C:/Windows/Fonts/arial.ttf"}"#,
    )
    .expect("text_3d alias should deserialize");
    assert!(matches!(text_3d, NodeKind::Text3D { ref text, .. } if text == "ALIAS"));
    assert!(
        serde_json::to_string(&text_3d)
            .expect("serialize")
            .contains(r#""type":"text3_d""#),
        "canonical serialization tag must remain unchanged"
    );

    let transform_3d: NodeKind =
        serde_json::from_str(r#"{"type": "transform_3d"}"#).expect("transform_3d alias");
    assert!(matches!(transform_3d, NodeKind::Transform3D { .. }));

    let particle_field_3d: NodeKind =
        serde_json::from_str(r#"{"type": "particle_field_3d"}"#).expect("particle_field_3d alias");
    assert!(matches!(
        particle_field_3d,
        NodeKind::ParticleField3D { .. }
    ));

    let ribbon_trail_3d: NodeKind =
        serde_json::from_str(r#"{"type": "ribbon_trail_3d"}"#).expect("ribbon_trail_3d alias");
    assert!(matches!(ribbon_trail_3d, NodeKind::RibbonTrail3D { .. }));

    let transform_2d: NodeKind = serde_json::from_str(
        r#"{"type": "transform_2d", "translate": [0.0, 0.0], "scale": [1.0, 1.0], "rotation_rad": 0.0, "anchor": [0.5, 0.5]}"#,
    )
    .expect("transform_2d alias");
    assert!(matches!(transform_2d, NodeKind::Transform2D { .. }));

    let lut_3d: NodeKind =
        serde_json::from_str(r#"{"type": "lut_3d", "input": "src", "lut_preset": "identity"}"#)
            .expect("lut_3d alias");
    assert!(matches!(lut_3d, NodeKind::Lut3D { .. }));
}

#[test]
fn phase_6_3_scene3d_and_shape3d_serde_validation_and_disabled_passthrough() {
    // Shape3D serde for all 5 Shape3DKind variants + snake_case alias
    for (kind_json, expected_kind) in [
        ("neon_tile", Shape3DKind::NeonTile),
        ("hollow_star5", Shape3DKind::HollowStar5),
        ("hollow_star_5", Shape3DKind::HollowStar5),
        ("heart3_d", Shape3DKind::Heart3D),
        ("heart_3d", Shape3DKind::Heart3D),
        ("lightning_bolt", Shape3DKind::LightningBolt),
        ("diamond3_d", Shape3DKind::Diamond3D),
        ("diamond_3d", Shape3DKind::Diamond3D),
    ] {
        let json = format!(r#"{{"type": "shape_3d", "shape_kind": "{kind_json}"}}"#);
        let parsed: NodeKind = serde_json::from_str(&json).expect("shape_3d must deserialize");
        assert!(matches!(
            parsed,
            NodeKind::Shape3D {
                shape_kind,
                size,
                aspect,
                inner_ratio,
                opacity,
                ..
            } if shape_kind == expected_kind
                && (size - 1.0).abs() < 1e-6
                && (aspect - 1.0).abs() < 1e-6
                && (inner_ratio - 0.382).abs() < 1e-6
                && (opacity - 1.0).abs() < 1e-6
        ));
    }

    // Invalid Shape3D size (<= 0 or NaN) rejected
    let bad_shape = RenderGraphDef {
        output_node: NodeId::from("s"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("s"),
            kind: serde_json::from_str(
                r#"{"type": "shape_3d", "shape_kind": "neon_tile", "size": 0.0}"#,
            )
            .unwrap(),
            inputs: vec![],
            enabled: true,
        }],
    };
    assert!(matches!(
        bad_shape.validate_graph(),
        Err(SchemaError::InvalidShape3D { node_id, .. }) if node_id == NodeId::from("s")
    ));

    // Valid Scene3D with all 5 allowed 3D child kinds
    let valid_scene_nodes: Vec<GraphNodeDef> = serde_json::from_str(
        r#"[
            {"id": "bg_2d", "kind": {"type": "solid", "color_rgba": [0.1, 0.1, 0.2, 1.0]}, "inputs": []},
            {"id": "c_xform", "kind": {"type": "transform_3d"}, "inputs": ["bg_2d"]},
            {"id": "c_text", "kind": {"type": "text_3d", "text": "RIVAL", "font_path": "C:/Windows/Fonts/arial.ttf"}, "inputs": []},
            {"id": "c_shape", "kind": {"type": "shape_3d", "shape_kind": "hollow_star5"}, "inputs": []},
            {"id": "c_part", "kind": {"type": "particle_field_3d"}, "inputs": []},
            {"id": "c_ribbon", "kind": {"type": "ribbon_trail_3d", "ribbon_style": 1}, "inputs": []},
            {
                "id": "scene",
                "kind": {
                    "type": "scene_3d",
                    "camera": {"mode": "look_at", "target": [0.0, 0.0, 0.0], "dof_autofocus_target": true, "dof_samples": 24}
                },
                "inputs": ["c_xform", "c_text", "c_shape", "c_part", "c_ribbon"]
            }
        ]"#,
    )
    .unwrap();

    let valid_graph = RenderGraphDef {
        output_node: NodeId::from("scene"),
        nodes: valid_scene_nodes.clone(),
    };
    let topo = valid_graph.validate_graph().expect("valid Scene3D DAG");
    assert_eq!(topo.len(), 7);
    assert_eq!(topo.last(), Some(&NodeId::from("scene")));

    // Disabled Scene3D clears to transparent even with multiple 3D child inputs
    let mut disabled_scene = valid_scene_nodes.last().unwrap().clone();
    disabled_scene.enabled = false;
    assert_eq!(
        disabled_scene.disabled_passthrough(),
        Some(DisabledPassthrough::Clear)
    );
    assert!(disabled_scene.kind.accepts_input_count(0));
    assert!(disabled_scene.kind.accepts_input_count(64));
    assert!(!disabled_scene.kind.accepts_input_count(65));

    // Reject non-3D child in Scene3D.inputs
    let bad_child_graph = RenderGraphDef {
        output_node: NodeId::from("scene"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("bg_2d"),
                kind: NodeKind::Solid {
                    color_rgba: [1.0; 4],
                    color_space: Default::default(),
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("scene"),
                kind: NodeKind::Scene3D {
                    camera: Camera3DDef::default(),
                    background_rgba: [0.0; 4],
        depth_plate: None,
                },
                inputs: vec![NodeId::from("bg_2d")],
                enabled: true,
            },
        ],
    };
    assert!(matches!(
        bad_child_graph.validate_graph(),
        Err(SchemaError::InvalidScene3D { node_id, .. }) if node_id == NodeId::from("scene")
    ));

    // Reject invalid camera bounds (near <= 0, far <= near, fov out of range, dof_samples out of 8..=32)
    for bad_cam in [
        Camera3DDef {
            near: 0.0,
            ..Default::default()
        },
        Camera3DDef {
            near: 10.0,
            far: 5.0,
            ..Default::default()
        },
        Camera3DDef {
            fov_deg: 2.0,
            ..Default::default()
        },
        Camera3DDef {
            fov_deg: 179.0,
            ..Default::default()
        },
        Camera3DDef {
            dof_samples: 4,
            ..Default::default()
        },
        Camera3DDef {
            dof_samples: 64,
            ..Default::default()
        },
    ] {
        let g = RenderGraphDef {
            output_node: NodeId::from("scene"),
            nodes: vec![GraphNodeDef {
                id: NodeId::from("scene"),
                kind: NodeKind::Scene3D {
                    camera: bad_cam,
                    background_rgba: [0.0; 4],
        depth_plate: None,
                },
                inputs: vec![],
                enabled: true,
            }],
        };
        assert!(matches!(
            g.validate_graph(),
            Err(SchemaError::InvalidScene3D { .. })
        ));
    }

    // Reject > 64 inputs on Scene3D
    let mut nodes_65 = Vec::new();
    let mut inputs_65 = Vec::new();
    for i in 0..65 {
        let id = NodeId::from(format!("s_{i}").as_str());
        inputs_65.push(id.clone());
        nodes_65.push(GraphNodeDef {
            id,
            kind: NodeKind::ParticleField3D {
                count: 10,
                seed: 1,
                shape: 1,
                color_a_rgba: [1.0; 4],
                color_b_rgba: [1.0; 4],
                speed: 1.0,
                spread: 1.0,
                size: 0.05,
                camera_pos: [0.0, 0.0, 2.5],
                camera_rot_deg: [0.0; 3],
                fov_deg: 50.0,
                time_offset_sec: 0.0,
                dof_focal_dist: 2.5,
                dof_blur_strength: 0.0,
                opacity: 1.0,
            },
            inputs: vec![],
            enabled: true,
        });
    }
    nodes_65.push(GraphNodeDef {
        id: NodeId::from("scene"),
        kind: NodeKind::Scene3D {
            camera: Camera3DDef::default(),
            background_rgba: [0.0; 4],
        depth_plate: None,
        },
        inputs: inputs_65,
        enabled: true,
    });
    let over_arity = RenderGraphDef {
        output_node: NodeId::from("scene"),
        nodes: nodes_65,
    };
    assert!(matches!(
        over_arity.validate_graph(),
        Err(SchemaError::InvalidNodeArity {
            expected: 64,
            actual: 65,
            ..
        })
    ));
}

#[test]
fn phase_6_3_legacy_faz6_2_backward_compat_and_safe_expr_bindings() {
    // Minimal Faz 6.2 JSON without any Faz 6.3 fields must deserialize with exact defaults
    let legacy_text: NodeKind = serde_json::from_str(
        r#"{"type": "text3_d", "text": "BANG!", "font_path": "C:/Windows/Fonts/arial.ttf"}"#,
    )
    .expect("legacy Text3D JSON must deserialize");
    assert!(matches!(
        legacy_text,
        NodeKind::Text3D {
            outline_width,
            outline_color_rgba,
            material_mode: 0,
            color_mode: 0,
            ref palette_rgba,
            pop_duration_sec,
            pop_overshoot,
            spin_in_deg,
            char_jitter_deg,
            arc_curvature,
            arc_axis: 0,
            echo_count: 0,
            echo_offset,
            echo_alpha_decay,
            echo_scale_step,
            hdr_intensity,
            opacity,
            ..
        } if outline_width == 0.0
            && outline_color_rgba == [1.0, 1.0, 1.0, 1.0]
            && palette_rgba.is_empty()
            && pop_duration_sec == 0.0
            && (pop_overshoot - 2.15).abs() < 1e-6
            && spin_in_deg == 0.0
            && char_jitter_deg == 0.0
            && arc_curvature == 0.0
            && echo_offset == [0.0, 0.0, -0.35]
            && (echo_alpha_decay - 0.55).abs() < 1e-6
            && (echo_scale_step - 0.94).abs() < 1e-6
            && (hdr_intensity - 1.0).abs() < 1e-6
            && (opacity - 1.0).abs() < 1e-6
    ));

    let legacy_ribbon: NodeKind =
        serde_json::from_str(r#"{"type": "ribbon_trail3_d"}"#).expect("legacy RibbonTrail3D JSON");
    assert!(matches!(
        legacy_ribbon,
        NodeKind::RibbonTrail3D {
            ribbon_style: 0,
            strand_spacing,
            curve_bend,
            pos,
            rot_deg,
            scale,
            opacity,
            ..
        } if (strand_spacing - 0.06).abs() < 1e-6
            && (curve_bend - 0.45).abs() < 1e-6
            && pos == [0.0; 3]
            && rot_deg == [0.0; 3]
            && scale == [1.0; 3]
            && (opacity - 1.0).abs() < 1e-6
    ));

    let legacy_particle: NodeKind = serde_json::from_str(r#"{"type": "particle_field3_d"}"#)
        .expect("legacy ParticleField3D JSON");
    assert!(matches!(
        legacy_particle,
        NodeKind::ParticleField3D { opacity, .. } if (opacity - 1.0).abs() < 1e-6
    ));

    // Validate Faz 6.3 SafeExpr param bindings across Scene3D, Shape3D, Text3D, RibbonTrail3D, ParticleField3D
    let mut project = Project::golden_scene_preset();
    project.timeline.clips[0].event_animations.clear();
    project.param_bindings.clear();
    project.graph = RenderGraphDef {
        output_node: NodeId::from("scene"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("txt"),
                kind: legacy_text,
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("shp"),
                kind: serde_json::from_str(r#"{"type": "shape_3d", "shape_kind": "diamond_3d"}"#)
                    .unwrap(),
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("rib"),
                kind: legacy_ribbon,
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("prt"),
                kind: legacy_particle,
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("scene"),
                kind: NodeKind::Scene3D {
                    camera: Camera3DDef {
                        mode: Camera3DMode::LookAt,
                        ..Default::default()
                    },
                    background_rgba: [0.0; 4],
                    depth_plate: None,
                },
                inputs: vec![
                    NodeId::from("txt"),
                    NodeId::from("shp"),
                    NodeId::from("rib"),
                    NodeId::from("prt"),
                ],
                enabled: true,
            },
        ],
    };

    for param in [
        "cam_x",
        "cam_y",
        "cam_z",
        "cam_rot_x",
        "cam_rot_y",
        "cam_rot_z",
        "cam_roll",
        "cam_target_x",
        "cam_target_y",
        "cam_target_z",
        "fov_deg",
        "dof_focal_dist",
        "dof_blur_strength",
        "dof_max_radius_px",
        "dof_highlight_boost",
    ] {
        project.param_bindings.push(ParamBindingDef {
            node_id: NodeId::from("scene"),
            param_name: param.into(),
            expression: "ease_out_expo(progress)".into(),
        });
    }

    for param in [
        "size",
        "aspect",
        "stroke_width",
        "scale_x",
        "scale_y",
        "scale_z",
        "hdr_intensity",
        "opacity",
    ] {
        project.param_bindings.push(ParamBindingDef {
            node_id: NodeId::from("shp"),
            param_name: param.into(),
            expression: "ease_out_back(progress)".into(),
        });
    }

    for param in [
        "arc_curvature",
        "hdr_intensity",
        "pop_duration_sec",
        "spin_in_deg",
        "opacity",
    ] {
        project.param_bindings.push(ParamBindingDef {
            node_id: NodeId::from("txt"),
            param_name: param.into(),
            expression: "exp(-t_sec)".into(),
        });
    }

    for param in [
        "strand_spacing",
        "curve_bend",
        "pos_x",
        "pos_y",
        "pos_z",
        "rot_x",
        "rot_y",
        "rot_z",
        "scale_x",
        "scale_y",
        "scale_z",
        "opacity",
    ] {
        project.param_bindings.push(ParamBindingDef {
            node_id: NodeId::from("rib"),
            param_name: param.into(),
            expression: "ease_in_out_expo(progress)".into(),
        });
    }

    project.param_bindings.push(ParamBindingDef {
        node_id: NodeId::from("prt"),
        param_name: "opacity".into(),
        expression: "ease_in_expo(progress)".into(),
    });

    project
        .validate()
        .expect("all Faz 6.3 param bindings and easing expressions must validate");

    // Reject non-bindable param on Scene3D
    let mut bad_scene_param = project.clone();
    bad_scene_param.param_bindings.push(ParamBindingDef {
        node_id: NodeId::from("scene"),
        param_name: "opacity".into(),
        expression: "1.0".into(),
    });
    assert!(matches!(
        bad_scene_param.validate(),
        Err(SchemaError::InvalidParamBinding { param_name, .. }) if param_name == "opacity"
    ));
}

#[test]
fn wave1_all_serde_names_match_serialized_node_kind_tags() {
    let variants: Vec<NodeKind> = vec![
        NodeKind::SourceVideo {
            asset_id: "a".into(),
            clip_id: None,
            color_space: ColorSpace::LinearRec709,
            fit: SourceFit::Stretch,
        },
        NodeKind::SourceImage {
            asset_id: "img".into(),
            color_space: ColorSpace::DisplaySrgb,
            fit: SourceFit::Cover,
            opacity: 1.0,
        },
        NodeKind::Solid {
            color_rgba: [1.0; 4],
            color_space: ColorSpace::LinearRec709,
        },
        NodeKind::Transform2D {
            translate: [0.0; 2],
            scale: [1.0; 2],
            rotation_rad: 0.0,
            anchor: [0.5; 2],
            filter: SampleFilter::Bilinear,
        },
        NodeKind::MaskMatte {
            mode: Default::default(),
            invert: false,
            feather: 0.0,
            opaque_black_outside: false,
            opacity: 1.0,
            choke_px: 0.0,
            spatial_feather_px: 0.0,
        },
        NodeKind::Blend {
            mode: BlendMode::Normal,
            opacity: 1.0,
        },
        NodeKind::ColorBasic {
            exposure: 0.0,
            contrast: 1.0,
            saturation: 1.0,
            flash_amount: 0.0,
        },
        NodeKind::ShaderTransition {
            shader_name: "cross_zoom".into(),
            progress: 0.5,
            direction_rad: 0.0,
            whip_strength: 0.0,
            chromatic_aberration: 0.0,
            impact_flash: 0.0,
        },
        NodeKind::MotionBlurShutter {
            shutter_angle_deg: 180.0,
            sub_samples: 4,
            phase: Default::default(),
            tonemap: true,
        },
        NodeKind::VectorText {
            text: "T".into(),
            font_size_norm: 0.1,
            color_rgba: [1.0; 4],
            outline_rgba: [0.0; 4],
            outline_width_norm: 0.0,
            align_center: true,
            y_offset_norm: 0.5,
            font_family: None,
            font_path: None,
            font_weight: 400,
            align: None,
            x_offset_norm: None,
            tracking_em: None,
            line_height_em: None,
            opacity: 1.0,
            fallback_fonts: Vec::new(),
            writing_mode: Default::default(),
            var_wght: None,
            var_wdth: None,
            var_slnt: None,
            color_bottom_rgba: None,
            gradient_angle_deg: 0.0,
            outer_outline_rgba: [0.0; 4],
            outer_outline_width_norm: 0.0,
            outline_gap_norm: 0.0,
            shadow_offset_norm: [0.0, 0.0],
            shadow_blur_norm: 0.0,
            shadow_rgba: [0.0; 4],
            glow_radius_norm: 0.0,
            glow_rgba: [0.0; 4],
            hdr_intensity: 1.0,
            gpu_slug: true,
            text_animator: None,
        },
        NodeKind::VectorPathHud {
            points_norm: vec![[0.1, 0.1], [0.9, 0.9]],
            bezier_segments: vec![],
            closed: false,
            stroke_width_norm: 0.003,
            stroke_rgba: [1.0; 4],
            fill_rgba: [0.0; 4],
            dash_len_norm: 0.0,
            gap_len_norm: 0.0,
            dash_offset_norm: 0.0,
            trim_start: 0.0,
            trim_end: 1.0,
            node_dot_radius_norm: 0.0,
            data_value: 0.0,
            data_label: String::new(),
            opacity: 1.0,
        },
        NodeKind::Lut3D {
            input: NodeId::from("s"),
            lut_preset: "identity".into(),
            intensity: 1.0,
            lut_source: None,
            input_space: None,
            output_space: None,
        },
        serde_json::from_str(r#"{"type":"motion_tile_shake"}"#).unwrap(),
        serde_json::from_str(r#"{"type":"anime_impact_frame"}"#).unwrap(),
        serde_json::from_str(r#"{"type":"radial_shockwave"}"#).unwrap(),
        serde_json::from_str(r#"{"type":"halftone_glow"}"#).unwrap(),
        serde_json::from_str(r#"{"type":"deep_glow"}"#).unwrap(),
        serde_json::from_str(r#"{"type":"anime_sharpen"}"#).unwrap(),
        serde_json::from_str(r#"{"type":"anime4k"}"#).unwrap(),
        serde_json::from_str(r#"{"type":"film_optics"}"#).unwrap(),
        serde_json::from_str(r#"{"type":"custom_wgsl"}"#).unwrap(),
        serde_json::from_str(r#"{"type":"transform3_d"}"#).unwrap(),
        serde_json::from_str(r#"{"type":"text3_d"}"#).unwrap(),
        serde_json::from_str(r#"{"type":"shape3_d","shape_kind":"neon_tile"}"#).unwrap(),
        serde_json::from_str(r#"{"type":"particle_field3_d"}"#).unwrap(),
        serde_json::from_str(r#"{"type":"ribbon_trail3_d"}"#).unwrap(),
        serde_json::from_str(r#"{"type":"scene3_d"}"#).unwrap(),
        serde_json::from_str(r#"{"type":"corner_pin"}"#).unwrap(),
        serde_json::from_str(r#"{"type":"grid_warp"}"#).unwrap(),
        serde_json::from_str(r#"{"type":"backdrop_scrim"}"#).unwrap(),
        NodeKind::Precomp {
            sub_graph: RenderGraphDef {
                output_node: NodeId::from("sub"),
                nodes: vec![],
            },
            time_offset_ticks: 0,
            time_scale: 1.0,
            opacity: 1.0,
        },
        NodeKind::PrecompInput { slot: 0 },
        NodeKind::AdjustmentLayer {
            start_tick: Flicks::ZERO,
            end_tick: Flicks(FLICKS_PER_SECOND),
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
        },
        serde_json::from_str(r#"{"type":"blur_mo_curves"}"#).unwrap(),
        serde_json::from_str(r#"{"type":"null3_d"}"#).unwrap(),
        serde_json::from_str(r#"{"type":"depth_matte"}"#).unwrap(),
        serde_json::from_str(r#"{"type":"optical_flow_blur"}"#).unwrap(),
        serde_json::from_str(r#"{"type":"displacement_map"}"#).unwrap(),
        serde_json::from_str(r#"{"type":"frame_echo"}"#).unwrap(),
        serde_json::from_str(r#"{"type":"shape_mask"}"#).unwrap(),
    ];

    let serialized_tags: Vec<String> = variants
        .iter()
        .map(|k| {
            serde_json::to_value(k)
                .unwrap()
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap()
                .to_owned()
        })
        .collect();

    assert_eq!(serialized_tags.len(), NodeKind::ALL_SERDE_NAMES.len());
    for expected in NodeKind::ALL_SERDE_NAMES {
        assert!(
            serialized_tags.iter().any(|t| t == expected),
            "missing '{expected}' in serialized NodeKind tags"
        );
    }
}

#[test]
fn wave1_legacy_vectortext_and_lut3d_backward_compat_and_validation() {
    let legacy_vtext: NodeKind = serde_json::from_str(
        r#"{
            "type": "vector_text",
            "text": "HELLO",
            "font_size_norm": 0.08,
            "color_rgba": [1.0, 1.0, 1.0, 1.0]
        }"#,
    )
    .expect("legacy vector_text JSON must deserialize");
    assert!(matches!(
        &legacy_vtext,
        NodeKind::VectorText {
            font_family: None,
            font_path: None,
            font_weight: 400,
            align: None,
            x_offset_norm: None,
            tracking_em: None,
            line_height_em: None,
            opacity,
            ..
        } if (*opacity - 1.0).abs() < 1e-6
    ));

    let full_vtext: NodeKind = serde_json::from_str(
        r#"{
            "type": "vector_text",
            "text": "LINE 1\nLINE 2",
            "font_size_norm": 0.06,
            "color_rgba": [1.0, 0.9, 0.8, 1.0],
            "font_family": "Impact",
            "font_weight": 700,
            "align": "right",
            "x_offset_norm": 0.92,
            "tracking_em": 0.04,
            "line_height_em": 1.25,
            "opacity": 0.85
        }"#,
    )
    .expect("full vector_text JSON must deserialize");
    assert!(matches!(
        &full_vtext,
        NodeKind::VectorText {
            font_family: Some(fam),
            font_weight: 700,
            align: Some(TextAlign2D::Right),
            ..
        } if fam == "Impact"
    ));

    // Invalid VectorText checks
    for bad_json in [
        r#"{"type":"vector_text","text":"X","font_size_norm":0.1,"color_rgba":[1,1,1,1],"font_weight":0}"#,
        r#"{"type":"vector_text","text":"X","font_size_norm":0.1,"color_rgba":[1,1,1,1],"font_weight":1001}"#,
        r#"{"type":"vector_text","text":"X","font_size_norm":-0.1,"color_rgba":[1,1,1,1]}"#,
        r#"{"type":"vector_text","text":"X","font_size_norm":0.1,"color_rgba":[1,1,1,1],"font_path":"  "}"#,
        r#"{"type":"vector_text","text":"X","font_size_norm":0.1,"color_rgba":[1,1,1,1],"font_family":""}"#,
    ] {
        let kind: NodeKind = serde_json::from_str(bad_json).unwrap();
        let g = RenderGraphDef {
            output_node: NodeId::from("vt"),
            nodes: vec![GraphNodeDef {
                id: NodeId::from("vt"),
                kind,
                inputs: vec![],
                enabled: true,
            }],
        };
        assert!(
            matches!(
                g.validate_graph(),
                Err(SchemaError::InvalidVectorText { .. })
            ),
            "expected InvalidVectorText for {bad_json}"
        );
    }

    // Legacy Lut3D without lut_source / spaces
    let legacy_lut: NodeKind =
        serde_json::from_str(r#"{"type": "lut3_d", "input": "src", "lut_preset": "warm_sunset"}"#)
            .expect("legacy lut3_d must deserialize");
    assert!(matches!(
        &legacy_lut,
        NodeKind::Lut3D {
            lut_preset,
            lut_source: None,
            input_space: None,
            output_space: None,
            ..
        } if lut_preset == "warm_sunset"
    ));

    // OCIO Lut3D without lut_preset
    let ocio_lut: NodeKind = serde_json::from_str(
        r#"{
            "type": "lut_3d",
            "input": "src",
            "lut_source": {
                "kind": "ocio",
                "config_path": "luts/config.ocio",
                "src_space": "ACEScg",
                "dst_space": "Output - sRGB"
            },
            "input_space": "aces_cg",
            "output_space": "display_srgb"
        }"#,
    )
    .expect("ocio lut3_d must deserialize");
    assert!(matches!(
        &ocio_lut,
        NodeKind::Lut3D {
            lut_source: Some(LutSourceDef::Ocio { lut_size: 33, .. }),
            input_space: Some(ExtendedColorSpace::AcesCg),
            output_space: Some(ExtendedColorSpace::DisplaySrgb),
            ..
        }
    ));

    let bad_lut = RenderGraphDef {
        output_node: NodeId::from("lut"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("src"),
                kind: NodeKind::Solid {
                    color_rgba: [1.0; 4],
                    color_space: ColorSpace::LinearRec709,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("lut"),
                kind: NodeKind::Lut3D {
                    input: NodeId::from("src"),
                    lut_preset: "".into(),
                    intensity: 1.0,
                    lut_source: Some(LutSourceDef::BuiltinTransform {
                        src_space: ExtendedColorSpace::Aces2065_1,
                        dst_space: ExtendedColorSpace::LinearRec709,
                        lut_size: 256,
                    }),
                    input_space: None,
                    output_space: None,
                },
                inputs: vec![NodeId::from("src")],
                enabled: true,
            },
        ],
    };
    assert!(matches!(
        bad_lut.validate_graph(),
        Err(SchemaError::InvalidLut3D { .. })
    ));
}

#[test]
fn wave1_source_image_hud_warp_precomp_roundtrip_bindings_and_errors() {
    let mut project = Project::golden_scene_preset();
    project.timeline.clips[0].event_animations.clear();
    project.param_bindings.clear();

    project.assets.push(AssetDef {
        id: "img_panel".into(),
        kind: AssetKind::Image,
        path: "local_media/panel.png".into(),
        duration_ticks: None,
        fps: None,
        sample_rate_hz: None,
        resolution: Some(frameshift_schema::Resolution {
            width: 512,
            height: 512,
        }),
    });

    let sub_graph = RenderGraphDef {
        output_node: NodeId::from("sub_warp"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("sub_in"),
                kind: NodeKind::PrecompInput { slot: 0 },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("sub_hud"),
                kind: NodeKind::VectorPathHud {
                    points_norm: vec![[0.1, 0.2], [0.9, 0.8]],
                    bezier_segments: vec![CubicBezierSegmentDef {
                        p0: [0.1, 0.2],
                        c1: [0.4, 0.1],
                        c2: [0.6, 0.9],
                        p1: [0.9, 0.8],
                    }],
                    closed: false,
                    stroke_width_norm: 0.004,
                    stroke_rgba: [0.2, 0.9, 1.0, 1.0],
                    fill_rgba: [0.0; 4],
                    dash_len_norm: 0.02,
                    gap_len_norm: 0.01,
                    dash_offset_norm: 0.0,
                    trim_start: 0.0,
                    trim_end: 1.0,
                    node_dot_radius_norm: 0.006,
                    data_value: 42.0,
                    data_label: "VEL:".into(),
                    opacity: 1.0,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("sub_blend"),
                kind: NodeKind::Blend {
                    mode: BlendMode::Normal,
                    opacity: 1.0,
                },
                inputs: vec![NodeId::from("sub_in"), NodeId::from("sub_hud")],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("sub_warp"),
                kind: NodeKind::GridWarp {
                    cols: 4,
                    rows: 4,
                    control_offsets: vec![[0.01, -0.01]],
                    wave_amp: [0.02, 0.01],
                    wave_freq: [2.0, 3.0],
                    wave_phase: 0.0,
                    mix_amount: 1.0,
                    filter: SampleFilter::CatmullRom,
                    opacity: 1.0,
                },
                inputs: vec![NodeId::from("sub_blend")],
                enabled: true,
            },
        ],
    };

    project.graph = RenderGraphDef {
        output_node: NodeId::from("pin_out"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("img_src"),
                kind: NodeKind::SourceImage {
                    asset_id: "img_panel".into(),
                    color_space: ColorSpace::DisplaySrgb,
                    fit: SourceFit::Contain,
                    opacity: 0.9,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("precomp_card"),
                kind: NodeKind::Precomp {
                    sub_graph,
                    time_offset_ticks: 0,
                    time_scale: 1.0,
                    opacity: 1.0,
                },
                inputs: vec![NodeId::from("img_src")],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("pin_out"),
                kind: NodeKind::CornerPin {
                    tl: [0.05, 0.05],
                    tr: [0.95, 0.1],
                    br: [0.9, 0.95],
                    bl: [0.1, 0.9],
                    filter: SampleFilter::Bilinear,
                    opacity: 1.0,
                },
                inputs: vec![NodeId::from("precomp_card")],
                enabled: true,
            },
        ],
    };

    assert_eq!(
        project.graph.nodes[0].kind.source_fit(),
        Some(SourceFit::Contain)
    );
    let mut disabled_precomp = project.graph.nodes[1].clone();
    disabled_precomp.enabled = false;
    assert_eq!(
        disabled_precomp.disabled_passthrough(),
        Some(DisabledPassthrough::Input(0))
    );

    // Bindings targeting root and nested precomp nodes
    for (node, param) in [
        ("img_src", "opacity"),
        ("precomp_card", "time_offset_ticks"),
        ("precomp_card", "time_scale"),
        ("precomp_card", "opacity"),
        ("sub_hud", "trim_start"),
        ("sub_hud", "trim_end"),
        ("sub_hud", "data_value"),
        ("sub_warp", "wave_amp_x"),
        ("sub_warp", "wave_phase"),
        ("sub_warp", "mix_amount"),
        ("pin_out", "tl_x"),
        ("pin_out", "br_y"),
        ("pin_out", "opacity"),
    ] {
        project.param_bindings.push(ParamBindingDef {
            node_id: NodeId::from(node),
            param_name: param.into(),
            expression: "progress".into(),
        });
    }

    project.validate().expect("wave1 project must validate");
    let json = project.to_json_pretty().unwrap();
    let restored = Project::from_json_str(&json).unwrap();
    assert_eq!(project, restored);

    // SourceImage referencing audio asset must fail Project::validate()
    let mut bad_img_proj = project.clone();
    bad_img_proj.graph.nodes[0].kind = NodeKind::SourceImage {
        asset_id: "asset_bgm".into(),
        color_space: ColorSpace::DisplaySrgb,
        fit: SourceFit::Stretch,
        opacity: 1.0,
    };
    assert!(matches!(
        bad_img_proj.validate(),
        Err(SchemaError::MissingAsset(id)) if id == "asset_bgm"
    ));

    // PrecompInput in root graph is forbidden
    let root_precomp_in = RenderGraphDef {
        output_node: NodeId::from("pi"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("pi"),
            kind: NodeKind::PrecompInput { slot: 0 },
            inputs: vec![],
            enabled: true,
        }],
    };
    assert!(matches!(
        root_precomp_in.validate_graph(),
        Err(SchemaError::InvalidPrecomp { .. })
    ));

    // Precomp depth > MAX_PRECOMP_DEPTH (8) rejected
    let mut nested = RenderGraphDef {
        output_node: NodeId::from("leaf"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("leaf"),
            kind: NodeKind::Solid {
                color_rgba: [1.0; 4],
                color_space: ColorSpace::LinearRec709,
            },
            inputs: vec![],
            enabled: true,
        }],
    };
    for lvl in 0..=MAX_PRECOMP_DEPTH {
        nested = RenderGraphDef {
            output_node: NodeId::from(format!("p_{lvl}").as_str()),
            nodes: vec![GraphNodeDef {
                id: NodeId::from(format!("p_{lvl}").as_str()),
                kind: NodeKind::Precomp {
                    sub_graph: nested,
                    time_offset_ticks: 0,
                    time_scale: 1.0,
                    opacity: 1.0,
                },
                inputs: vec![],
                enabled: true,
            }],
        };
    }
    assert!(matches!(
        nested.validate_graph(),
        Err(SchemaError::PrecompDepthExceeded {
            depth: 9,
            max_depth: 8,
            ..
        })
    ));

    // GridWarp out-of-range dimensions rejected
    for dim in [1u32, 9u32] {
        let g = RenderGraphDef {
            output_node: NodeId::from("gw"),
            nodes: vec![
                GraphNodeDef {
                    id: NodeId::from("s"),
                    kind: NodeKind::Solid {
                        color_rgba: [1.0; 4],
                        color_space: ColorSpace::LinearRec709,
                    },
                    inputs: vec![],
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("gw"),
                    kind: NodeKind::GridWarp {
                        cols: dim,
                        rows: 4,
                        control_offsets: vec![],
                        wave_amp: [0.0; 2],
                        wave_freq: [2.0; 2],
                        wave_phase: 0.0,
                        mix_amount: 1.0,
                        filter: SampleFilter::Bilinear,
                        opacity: 1.0,
                    },
                    inputs: vec![NodeId::from("s")],
                    enabled: true,
                },
            ],
        };
        assert!(matches!(
            g.validate_graph(),
            Err(SchemaError::InvalidGridWarp { .. })
        ));
    }

    // CornerPin non-finite coordinate rejected
    let bad_pin = RenderGraphDef {
        output_node: NodeId::from("cp"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("s"),
                kind: NodeKind::Solid {
                    color_rgba: [1.0; 4],
                    color_space: ColorSpace::LinearRec709,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("cp"),
                kind: NodeKind::CornerPin {
                    tl: [f32::NAN, 0.0],
                    tr: [1.0, 0.0],
                    br: [1.0, 1.0],
                    bl: [0.0, 1.0],
                    filter: SampleFilter::Bilinear,
                    opacity: 1.0,
                },
                inputs: vec![NodeId::from("s")],
                enabled: true,
            },
        ],
    };
    assert!(matches!(
        bad_pin.validate_graph(),
        Err(SchemaError::InvalidCornerPin { .. })
    ));

    // VectorPathHud negative stroke_width_norm rejected
    let bad_hud = RenderGraphDef {
        output_node: NodeId::from("hud"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("hud"),
            kind: NodeKind::VectorPathHud {
                points_norm: vec![],
                bezier_segments: vec![],
                closed: false,
                stroke_width_norm: -0.01,
                stroke_rgba: [1.0; 4],
                fill_rgba: [0.0; 4],
                dash_len_norm: 0.0,
                gap_len_norm: 0.0,
                dash_offset_norm: 0.0,
                trim_start: 0.0,
                trim_end: 1.0,
                node_dot_radius_norm: 0.0,
                data_value: 0.0,
                data_label: String::new(),
                opacity: 1.0,
            },
            inputs: vec![],
            enabled: true,
        }],
    };
    assert!(matches!(
        bad_hud.validate_graph(),
        Err(SchemaError::InvalidVectorPathHud { .. })
    ));
}

#[test]
fn anime_workflow_nodes_null3d_rig_matte_and_text_animator_v2_roundtrip_and_validate() {
    // 1. MatteMode::Red + MaskMatte opacity/choke_px/spatial_feather_px defaults and roundtrip
    let matte: NodeKind = serde_json::from_str(r#"{"type":"mask_matte","mode":"red"}"#)
        .expect("mask_matte red mode deserializes");
    assert!(matches!(
        matte,
        NodeKind::MaskMatte {
            mode: MatteMode::Red,
            opacity,
            choke_px,
            spatial_feather_px,
            ..
        } if (opacity - 1.0).abs() < 1e-6 && choke_px == 0.0 && spatial_feather_px == 0.0
    ));

    // 2. Transform3D mirror_edges, parent, pivot & Null3D alias + parenting rig in Scene3D
    let xform_alias: NodeKind = serde_json::from_str(
        r#"{"type":"transform_3d","mirror_edges":true,"parent":"cam_null","pivot":[0.1,-0.2,0.3]}"#,
    )
    .expect("transform_3d new fields deserialize");
    assert!(matches!(
        &xform_alias,
        NodeKind::Transform3D {
            mirror_edges: true,
            parent: Some(p),
            pivot,
            ..
        } if p == &NodeId::from("cam_null") && *pivot == [0.1, -0.2, 0.3]
    ));

    let null_alias: NodeKind =
        serde_json::from_str(r#"{"type":"null_3d"}"#).expect("null_3d alias deserializes");
    assert!(matches!(
        &null_alias,
        NodeKind::Null3D {
            pos,
            rot_deg,
            scale,
            pivot,
            parent: None,
        } if *pos == [0.0; 3] && *rot_deg == [0.0; 3] && *scale == [1.0; 3] && *pivot == [0.0; 3]
    ));
    assert!(null_alias.is_scene3d_child());
    assert_eq!(null_alias.expected_input_count(), 0);

    // 3. TextRangeShape & TextAnimatorDef v2 defaults & roundtrip
    for (shape, expected) in [
        (TextRangeShape::Square, "\"square\""),
        (TextRangeShape::RampUp, "\"ramp_up\""),
        (TextRangeShape::RampDown, "\"ramp_down\""),
        (TextRangeShape::Triangle, "\"triangle\""),
        (TextRangeShape::Smooth, "\"smooth\""),
    ] {
        assert_eq!(serde_json::to_string(&shape).unwrap(), expected);
        assert_eq!(
            serde_json::from_str::<TextRangeShape>(expected).unwrap(),
            shape
        );
    }

    let legacy_anim: TextAnimatorDef =
        serde_json::from_str(r#"{"unit":"word","range_start":0}"#).expect("legacy animator");
    assert_eq!(legacy_anim.shape, TextRangeShape::Square);
    assert_eq!(legacy_anim.rotation_deg_from, 0.0);
    assert_eq!(legacy_anim.tracking_em_from, 0.0);
    assert_eq!(legacy_anim.ease_high, 0.0);
    assert_eq!(legacy_anim.ease_low, 0.0);
    assert!(legacy_anim.randomize_seed.is_none());

    let mut project = Project::golden_scene_preset();
    project.timeline.clips[0].event_animations.clear();
    project.param_bindings.clear();
    project.graph = RenderGraphDef {
        output_node: NodeId::from("depth_matte_out"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("bg"),
                kind: NodeKind::Solid {
                    color_rgba: [0.1, 0.2, 0.3, 1.0],
                    color_space: ColorSpace::LinearRec709,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("title"),
                kind: NodeKind::VectorText {
                    text: "KINETIC".into(),
                    font_size_norm: 0.08,
                    color_rgba: [1.0; 4],
                    outline_rgba: [0.0; 4],
                    outline_width_norm: 0.0,
                    align_center: true,
                    y_offset_norm: 0.5,
                    font_family: Some("Impact".into()),
                    font_path: None,
                    font_weight: 400,
                    align: None,
                    x_offset_norm: None,
                    tracking_em: None,
                    line_height_em: None,
                    opacity: 1.0,
                    fallback_fonts: vec![],
                    writing_mode: Default::default(),
                    var_wght: None,
                    var_wdth: None,
                    var_slnt: None,
                    color_bottom_rgba: None,
                    gradient_angle_deg: 0.0,
                    outer_outline_rgba: [0.0; 4],
                    outer_outline_width_norm: 0.0,
                    outline_gap_norm: 0.0,
                    shadow_offset_norm: [0.0; 2],
                    shadow_blur_norm: 0.0,
                    shadow_rgba: [0.0; 4],
                    glow_radius_norm: 0.0,
                    glow_rgba: [0.0; 4],
                    hdr_intensity: 1.0,
                    gpu_slug: true,
                    text_animator: Some(TextAnimatorDef {
                        unit: TextSelectorUnit::Grapheme,
                        range_start: 0,
                        range_end: Some(7),
                        start_tick: 0,
                        duration_ticks: FLICKS_PER_SECOND / 2,
                        delay_ticks: FLICKS_PER_SECOND / 30,
                        position_from_norm: [0.0, 0.1],
                        scale_from: 1.2,
                        opacity_from: 0.0,
                        rotation_deg_from: -18.0,
                        tracking_em_from: 0.35,
                        shape: TextRangeShape::Triangle,
                        ease_high: 75.0,
                        ease_low: 25.0,
                        randomize_seed: Some(42),
                    }),
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("cam_null"),
                kind: NodeKind::Null3D {
                    pos: [0.0, 0.1, 0.0],
                    rot_deg: [0.0, 5.0, 0.0],
                    scale: [1.0, 1.0, 1.0],
                    pivot: [0.0, 0.0, 0.0],
                    parent: None,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("layer_xform"),
                kind: xform_alias,
                inputs: vec![NodeId::from("bg")],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("scene"),
                kind: NodeKind::Scene3D {
                    camera: Camera3DDef::default(),
                    background_rgba: [0.0; 4],
        depth_plate: None,
                },
                inputs: vec![NodeId::from("cam_null"), NodeId::from("layer_xform")],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("adj"),
                kind: NodeKind::AdjustmentLayer {
                    start_tick: Flicks::ZERO,
                    end_tick: Flicks(FLICKS_PER_SECOND * 2),
                    opacity: 0.85,
                    blend_mode: BlendMode::Screen,
                },
                inputs: vec![NodeId::from("scene")],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("moblur"),
                kind: NodeKind::BlurMoCurves {
                    shift_x: 0.04,
                    shift_y: -0.02,
                    z_dist: -0.15,
                    rotate_deg: 3.0,
                    blur_strength: 1.0,
                    samples: 16,
                    wrap_mode: 2,
                },
                inputs: vec![NodeId::from("adj")],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("depth_map"),
                kind: NodeKind::Solid {
                    color_rgba: [0.4, 0.4, 0.4, 1.0],
                    color_space: ColorSpace::LinearRec709,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("red_matte"),
                kind: NodeKind::MaskMatte {
                    mode: MatteMode::Red,
                    invert: false,
                    feather: 0.05,
                    opaque_black_outside: false,
                    opacity: 0.9,
                    choke_px: -2.0,
                    spatial_feather_px: 4.0,
                },
                inputs: vec![NodeId::from("moblur"), NodeId::from("title")],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("depth_matte_out"),
                kind: NodeKind::DepthMatte {
                    near_depth: 0.1,
                    far_depth: 0.8,
                    feather: 0.05,
                    invert: false,
                },
                inputs: vec![NodeId::from("red_matte"), NodeId::from("depth_map")],
                enabled: true,
            },
        ],
    };

    for (node, param) in [
        ("cam_null", "pos_x"),
        ("cam_null", "pivot_y"),
        ("layer_xform", "pivot_z"),
        ("adj", "opacity"),
        ("moblur", "shift_x"),
        ("moblur", "z_dist"),
        ("moblur", "rotate_deg"),
        ("moblur", "blur_strength"),
        ("red_matte", "opacity"),
        ("red_matte", "choke_px"),
        ("red_matte", "spatial_feather_px"),
        ("depth_matte_out", "near_depth"),
        ("depth_matte_out", "far_depth"),
        ("depth_matte_out", "feather"),
    ] {
        project.param_bindings.push(ParamBindingDef {
            node_id: NodeId::from(node),
            param_name: param.into(),
            expression: "0.5".into(),
        });
    }

    let topo = project.validate().expect("new workflow graph validates");
    let null_pos = topo.iter().position(|id| id == &NodeId::from("cam_null")).unwrap();
    let xform_pos = topo.iter().position(|id| id == &NodeId::from("layer_xform")).unwrap();
    assert!(null_pos < xform_pos, "parent Null3D must precede child Transform3D");

    let json = project.to_json_pretty().unwrap();
    let restored = Project::from_json_str(&json).expect("roundtrip validates");
    assert_eq!(restored, project);

    // Validation rejections:
    // AdjustmentLayer start >= end
    let bad_adj = RenderGraphDef {
        output_node: NodeId::from("adj"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("s"),
                kind: NodeKind::Solid {
                    color_rgba: [1.0; 4],
                    color_space: ColorSpace::LinearRec709,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("adj"),
                kind: NodeKind::AdjustmentLayer {
                    start_tick: Flicks(100),
                    end_tick: Flicks(100),
                    opacity: 1.0,
                    blend_mode: BlendMode::Normal,
                },
                inputs: vec![NodeId::from("s")],
                enabled: true,
            },
        ],
    };
    assert!(matches!(
        bad_adj.validate_graph(),
        Err(SchemaError::InvalidAdjustmentLayer { .. })
    ));

    // BlurMoCurves invalid samples (0 or 33) and wrap_mode (3)
    for bad_json in [
        r#"{"type":"blur_mo_curves","samples":0}"#,
        r#"{"type":"blur_mo_curves","samples":33}"#,
        r#"{"type":"blur_mo_curves","wrap_mode":3}"#,
    ] {
        let g = RenderGraphDef {
            output_node: NodeId::from("b"),
            nodes: vec![
                GraphNodeDef {
                    id: NodeId::from("s"),
                    kind: NodeKind::Solid {
                        color_rgba: [1.0; 4],
                        color_space: ColorSpace::LinearRec709,
                    },
                    inputs: vec![],
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("b"),
                    kind: serde_json::from_str(bad_json).unwrap(),
                    inputs: vec![NodeId::from("s")],
                    enabled: true,
                },
            ],
        };
        assert!(matches!(
            g.validate_graph(),
            Err(SchemaError::InvalidBlurMoCurves { .. })
        ));
    }

    // Null3D parent cycle detection & missing parent
    let cyclic_nulls = RenderGraphDef {
        output_node: NodeId::from("n1"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("n1"),
                kind: NodeKind::Null3D {
                    pos: [0.0; 3],
                    rot_deg: [0.0; 3],
                    scale: [1.0; 3],
                    pivot: [0.0; 3],
                    parent: Some(NodeId::from("n2")),
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("n2"),
                kind: NodeKind::Null3D {
                    pos: [0.0; 3],
                    rot_deg: [0.0; 3],
                    scale: [1.0; 3],
                    pivot: [0.0; 3],
                    parent: Some(NodeId::from("n1")),
                },
                inputs: vec![],
                enabled: true,
            },
        ],
    };
    assert!(matches!(
        cyclic_nulls.validate_graph(),
        Err(SchemaError::CycleDetected { .. })
    ));

    // DepthMatte invalid range (far_depth < near_depth) and arity
    let bad_depth = RenderGraphDef {
        output_node: NodeId::from("dm"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("a"),
                kind: NodeKind::Solid {
                    color_rgba: [1.0; 4],
                    color_space: ColorSpace::LinearRec709,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("b"),
                kind: NodeKind::Solid {
                    color_rgba: [0.5; 4],
                    color_space: ColorSpace::LinearRec709,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("dm"),
                kind: NodeKind::DepthMatte {
                    near_depth: 0.8,
                    far_depth: 0.2,
                    feather: 0.0,
                    invert: false,
                },
                inputs: vec![NodeId::from("a"), NodeId::from("b")],
                enabled: true,
            },
        ],
    };
    assert!(matches!(
        bad_depth.validate_graph(),
        Err(SchemaError::InvalidDepthMatte { .. })
    ));

    // MaskMatte invalid opacity
    let bad_matte = RenderGraphDef {
        output_node: NodeId::from("mm"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("a"),
                kind: NodeKind::Solid {
                    color_rgba: [1.0; 4],
                    color_space: ColorSpace::LinearRec709,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("b"),
                kind: NodeKind::Solid {
                    color_rgba: [1.0; 4],
                    color_space: ColorSpace::LinearRec709,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("mm"),
                kind: NodeKind::MaskMatte {
                    mode: MatteMode::Red,
                    invert: false,
                    feather: 0.0,
                    opaque_black_outside: false,
                    opacity: 1.5,
                    choke_px: 0.0,
                    spatial_feather_px: 0.0,
                },
                inputs: vec![NodeId::from("a"), NodeId::from("b")],
                enabled: true,
            },
        ],
    };
    assert!(matches!(
        bad_matte.validate_graph(),
        Err(SchemaError::InvalidMaskMatte { .. })
    ));
}

#[test]
fn trim_and_split_shift_cubic_bezier_value_and_speed_curves() {
    use frameshift_time::{BezierSpeedKeyframe, KeyframeEase};

    let sec = FLICKS_PER_SECOND;
    let mut project = Project::golden_scene_preset();

    // CubicBezierValue clip
    project.timeline.clips[0].speed_curve = SpeedCurve::cubic_bezier_value(vec![
        BezierSpeedKeyframe::value(Flicks::ZERO, Flicks::ZERO)
            .with_out_ease(KeyframeEase {
                speed: 1.5,
                influence: 0.4,
            }),
        BezierSpeedKeyframe::value(Flicks(2 * sec), Flicks(3 * sec))
            .with_in_ease(KeyframeEase {
                speed: 1.0,
                influence: 0.4,
            }),
    ])
    .unwrap();

    // CubicBezierSpeed clip
    project.timeline.clips[1].speed_curve = SpeedCurve::cubic_bezier_speed(vec![
        BezierSpeedKeyframe::speed(Flicks::ZERO, 0.5).with_out_ease(KeyframeEase {
            speed: 0.5,
            influence: 0.35,
        }),
        BezierSpeedKeyframe::speed(Flicks(2 * sec), 2.0).with_in_ease(KeyframeEase {
            speed: 0.0,
            influence: 0.35,
        }),
    ])
    .unwrap();

    let orig_a_src_at_15 = project.clip("clip_a").unwrap().source_start
        + project
            .clip("clip_a")
            .unwrap()
            .speed_curve
            .source_offset_at(Flicks(sec + sec / 2));

    let mut session = ProjectSession::new(project).expect("valid bezier curves");
    let rev = session
        .apply_command(
            1,
            ProjectCommand::TrimClip {
                clip_id: "clip_a".into(),
                new_timeline_start: Some(Flicks(sec / 2)),
                new_timeline_end: None,
            },
        )
        .expect("trim CubicBezierValue succeeds");
    let trimmed_a = session.project().clip("clip_a").unwrap();
    assert_eq!(trimmed_a.timeline_start, Flicks(sec / 2));
    // At offset 1s into trimmed clip (= 1.5s original), source time at endpoint (2s - 0.5s = 1.5s) matches original 2s endpoint.
    let orig_a_end_expected = orig_a_src_at_15;
    assert!(orig_a_end_expected > Flicks::ZERO);
    assert_eq!(
        trimmed_a.source_start
            + trimmed_a
                .speed_curve
                .source_offset_at(Flicks(sec + sec / 2)),
        Flicks(3 * sec)
    );

    session
        .apply_command(
            rev,
            ProjectCommand::SplitClip {
                clip_id: "clip_b".into(),
                split_tick: Flicks(3 * sec),
                right_clip_id: "clip_b_right".into(),
            },
        )
        .expect("split CubicBezierSpeed succeeds");
    let right_b = session.project().clip("clip_b_right").unwrap();
    assert_eq!(right_b.timeline_start, Flicks(3 * sec));
    assert_eq!(right_b.speed_curve.instant_speed_at(Flicks(sec)), 2.0);
}

#[test]
fn optical_flow_blur_serde_defaults_validation_and_params() {
    let parsed: NodeKind = serde_json::from_str(r#"{"type":"optical_flow_blur"}"#).unwrap();
    assert_eq!(parsed.expected_input_count(), 1);
    assert!(NodeKind::ALL_SERDE_NAMES.contains(&"optical_flow_blur"));
    assert!(matches!(
        parsed,
        NodeKind::OpticalFlowBlur {
            shutter_amount,
            samples: 12,
            max_blur_px,
            sensitivity,
            flow_smoothness,
            visualize_flow: false,
        } if (shutter_amount - 1.0).abs() < 1e-6
            && (max_blur_px - 48.0).abs() < 1e-6
            && (sensitivity - 0.35).abs() < 1e-6
            && (flow_smoothness - 0.25).abs() < 1e-6
    ));

    let valid_graph = RenderGraphDef {
        output_node: NodeId::from("ofb"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("src"),
                kind: NodeKind::Solid {
                    color_rgba: [1.0; 4],
                    color_space: ColorSpace::LinearRec709,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("ofb"),
                kind: parsed,
                inputs: vec![NodeId::from("src")],
                enabled: true,
            },
        ],
    };
    valid_graph.validate_graph().unwrap();

    let bad_graph = RenderGraphDef {
        output_node: NodeId::from("ofb"),
        nodes: vec![
            valid_graph.nodes[0].clone(),
            GraphNodeDef {
                id: NodeId::from("ofb"),
                kind: NodeKind::OpticalFlowBlur {
                    shutter_amount: 1.0,
                    samples: 0,
                    max_blur_px: 48.0,
                    sensitivity: 0.35,
                    flow_smoothness: 0.25,
                    visualize_flow: false,
                },
                inputs: vec![NodeId::from("src")],
                enabled: true,
            },
        ],
    };
    assert!(matches!(
        bad_graph.validate_graph(),
        Err(SchemaError::InvalidOpticalFlowBlur { .. })
    ));
}

#[test]
fn anime4k_serde_defaults_validation_and_depth_plate_cycle_precomp() {
    use frameshift_schema::{Anime4KPreset, SceneDepthPlateDef, precompose};

    let parsed: NodeKind = serde_json::from_str(r#"{"type":"anime4k"}"#).unwrap();
    assert_eq!(parsed.expected_input_count(), 1);
    assert!(NodeKind::ALL_SERDE_NAMES.contains(&"anime4k"));
    assert!(matches!(
        parsed,
        NodeKind::Anime4K {
            preset: Anime4KPreset::ModeA,
            strength,
            clamp_highlights: true,
        } if (strength - 1.0).abs() < 1e-6
    ));

    let bad_a4k = RenderGraphDef {
        output_node: NodeId::from("a4k"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("src"),
                kind: NodeKind::Solid {
                    color_rgba: [1.0; 4],
                    color_space: ColorSpace::LinearRec709,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("a4k"),
                kind: NodeKind::Anime4K {
                    preset: Anime4KPreset::LineArt,
                    strength: 2.5,
                    clamp_highlights: true,
                },
                inputs: vec![NodeId::from("src")],
                enabled: true,
            },
        ],
    };
    assert!(matches!(
        bad_a4k.validate_graph(),
        Err(SchemaError::InvalidAnime4K { .. })
    ));

    // Legacy SceneDepthPlateDef JSON without depth_gamma/disparity_mode/rg16_packed deserializes cleanly.
    let legacy_dp: SceneDepthPlateDef =
        serde_json::from_str(r#"{"node_id":"depth_src","near_z":0.8,"far_z":4.5}"#).unwrap();
    assert!((legacy_dp.depth_gamma - 1.0).abs() < 1e-6);
    assert!(legacy_dp.disparity_mode);
    assert!(!legacy_dp.rg16_packed);

    // Cycle through Scene3D.depth_plate.node_id must report non-empty cycle nodes.
    let cycle_graph = RenderGraphDef {
        output_node: NodeId::from("scene"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("depth_fx"),
                kind: serde_json::from_str(r#"{"type":"color_basic"}"#).unwrap(),
                inputs: vec![NodeId::from("scene")],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("scene"),
                kind: NodeKind::Scene3D {
                    camera: Default::default(),
                    background_rgba: [0.0; 4],
                    depth_plate: Some(SceneDepthPlateDef {
                        node_id: NodeId::from("depth_fx"),
                        ..Default::default()
                    }),
                },
                inputs: vec![],
                enabled: true,
            },
        ],
    };
    let err = cycle_graph.validate_graph().unwrap_err();
    match err {
        SchemaError::CycleDetected { cycle } => {
            assert!(cycle.contains(&NodeId::from("scene")));
            assert!(cycle.contains(&NodeId::from("depth_fx")));
        }
        other => panic!("expected CycleDetected, got {other:?}"),
    }

    // precompose must reject splitting across outside Scene3D.depth_plate.node_id.
    let mut precomp_graph = RenderGraphDef {
        output_node: NodeId::from("scene"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("bg_src"),
                kind: NodeKind::Solid {
                    color_rgba: [0.5; 4],
                    color_space: ColorSpace::LinearRec709,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("depth_fx"),
                kind: serde_json::from_str(r#"{"type":"color_basic"}"#).unwrap(),
                inputs: vec![NodeId::from("bg_src")],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("scene"),
                kind: NodeKind::Scene3D {
                    camera: Default::default(),
                    background_rgba: [0.0; 4],
                    depth_plate: Some(SceneDepthPlateDef {
                        node_id: NodeId::from("depth_fx"),
                        ..Default::default()
                    }),
                },
                inputs: vec![],
                enabled: true,
            },
        ],
    };
    assert!(precompose(&mut precomp_graph, &[NodeId::from("depth_fx")], NodeId::from("sub")).is_err());

    // Verify bindable_param_names includes depth_plate_near_z/far_z/gamma and Anime4K strength.
    let mut proj = Project::golden_scene_preset();
    for clip in &mut proj.timeline.clips {
        clip.event_animations.clear();
    }
    proj.graph = RenderGraphDef {
        output_node: NodeId::from("scene"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("src"),
                kind: NodeKind::Solid {
                    color_rgba: [0.5; 4],
                    color_space: ColorSpace::LinearRec709,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("a4k"),
                kind: NodeKind::Anime4K {
                    preset: Anime4KPreset::ModeB,
                    strength: 1.0,
                    clamp_highlights: true,
                },
                inputs: vec![NodeId::from("src")],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("scene"),
                kind: NodeKind::Scene3D {
                    camera: Default::default(),
                    background_rgba: [0.0; 4],
                    depth_plate: Some(SceneDepthPlateDef {
                        node_id: NodeId::from("a4k"),
                        ..Default::default()
                    }),
                },
                inputs: vec![],
                enabled: true,
            },
        ],
    };
    for param in ["depth_plate_near_z", "depth_plate_far_z", "depth_plate_gamma"] {
        proj.param_bindings = vec![ParamBindingDef {
            node_id: NodeId::from("scene"),
            param_name: param.into(),
            expression: "1.0".into(),
        }];
        proj.validate().unwrap();
    }
    proj.param_bindings = vec![ParamBindingDef {
        node_id: NodeId::from("a4k"),
        param_name: "strength".into(),
        expression: "0.75".into(),
    }];
    proj.validate().unwrap();
}
