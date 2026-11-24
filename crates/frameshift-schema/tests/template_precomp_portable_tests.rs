use frameshift_schema::{
    BlendMode, CURRENT_SCHEMA_VERSION, EffectChainPreset, GraphNodeDef, NodeId, NodeKind,
    ParamBindingDef, Project, ProjectMediaManifest, RenderGraphDef, SchemaError, TemplatePackage,
    TemplatePackageManifest, TemplateReview, TemplateSlot, TemplateSlotKind, TemplateSlotTarget,
    TemplateSlotValue, precompose, relink_missing_media, unprecompose,
    validate_effect_chain_preset,
};
use frameshift_time::{Flicks, SpeedCurve};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_TEMP_DIR: AtomicUsize = AtomicUsize::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let serial = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "frameshift-{label}-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create temp dir");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn reviewed_package(project: Project) -> TemplatePackage {
    let asset = project
        .assets
        .first()
        .expect("golden project has media asset");
    let asset_id = asset.id.clone();
    let asset_kind = asset.kind;
    let text_node = project
        .graph
        .nodes
        .iter()
        .find(|node| {
            matches!(
                node.kind,
                NodeKind::VectorText { .. } | NodeKind::Text3D { .. }
            )
        })
        .expect("golden project has a text node");
    let event = project.events.first().expect("golden project has event");
    let event_id = event.event_id.clone();
    let scalar_node = project
        .graph
        .nodes
        .iter()
        .find(|node| matches!(node.kind, NodeKind::ColorBasic { .. }))
        .expect("golden project has color node");
    let text_node_id = text_node.id.clone();
    let scalar_node_id = scalar_node.id.clone();

    TemplatePackage {
        package_version: 1,
        template_id: "impact-card".into(),
        name: "Impact Card".into(),
        description: "A reviewed editable impact title.".into(),
        project,
        slots: vec![
            TemplateSlot {
                id: "footage".into(),
                label: "Footage".into(),
                kind: TemplateSlotKind::AssetPath(asset_kind),
                target: TemplateSlotTarget::AssetPath { asset_id },
                required: true,
                default: None,
            },
            TemplateSlot {
                id: "headline".into(),
                label: "Headline".into(),
                kind: TemplateSlotKind::Text,
                target: TemplateSlotTarget::NodeText {
                    node_id: text_node_id,
                },
                required: true,
                default: Some(TemplateSlotValue::Text("IMPACT".into())),
            },
            TemplateSlot {
                id: "drop".into(),
                label: "Drop".into(),
                kind: TemplateSlotKind::Tick,
                target: TemplateSlotTarget::EventTargetTick { event_id },
                required: true,
                default: None,
            },
            TemplateSlot {
                id: "flash".into(),
                label: "Flash strength".into(),
                kind: TemplateSlotKind::Number,
                target: TemplateSlotTarget::ParameterBinding {
                    node_id: scalar_node_id,
                    param_name: "exposure".into(),
                },
                required: false,
                default: Some(TemplateSlotValue::Number(0.35)),
            },
        ],
        manifest: TemplatePackageManifest::default(),
        review: TemplateReview {
            reviewed: true,
            reviewer: Some("local-review".into()),
            note: Some("Local package inspected; no code execution.".into()),
        },
    }
}

fn simple_graph() -> RenderGraphDef {
    RenderGraphDef {
        output_node: NodeId::from("grade"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("plate"),
                kind: NodeKind::Solid {
                    color_rgba: [0.2, 0.3, 0.4, 1.0],
                    color_space: Default::default(),
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("grade"),
                kind: NodeKind::ColorBasic {
                    exposure: 0.2,
                    contrast: 1.0,
                    saturation: 1.0,
                    flash_amount: 0.0,
                },
                inputs: vec![NodeId::from("plate")],
                enabled: true,
            },
        ],
    }
}

#[test]
fn reviewed_template_instantiates_independent_media_text_and_timing_copies() {
    let package = reviewed_package(Project::golden_scene_preset());
    package.validate().expect("valid reviewed package");

    let mut values_a = BTreeMap::new();
    values_a.insert(
        "footage".into(),
        TemplateSlotValue::AssetPath("/media/a/clip.mp4".into()),
    );
    values_a.insert("headline".into(), TemplateSlotValue::Text("ALPHA".into()));
    values_a.insert("drop".into(), TemplateSlotValue::Tick(Flicks(1234)));
    let project_a = package.instantiate(&values_a).expect("open first instance");

    let mut values_b = BTreeMap::new();
    values_b.insert(
        "footage".into(),
        TemplateSlotValue::AssetPath("/media/b/clip.mp4".into()),
    );
    values_b.insert("headline".into(), TemplateSlotValue::Text("BETA".into()));
    values_b.insert("drop".into(), TemplateSlotValue::Tick(Flicks(4321)));
    let project_b = package
        .instantiate(&values_b)
        .expect("open second instance");

    assert_eq!(project_a.assets[0].path, "/media/a/clip.mp4");
    assert_eq!(project_b.assets[0].path, "/media/b/clip.mp4");
    assert_eq!(project_a.events[0].target_output_tick, Flicks(1234));
    assert_eq!(project_b.events[0].target_output_tick, Flicks(4321));
    assert_eq!(package.project.assets[0].path, "local_media/clip_a.mp4");
    assert_ne!(project_a, project_b);

    let text_slot = package
        .slots
        .iter()
        .find(|slot| slot.id == "headline")
        .expect("headline slot");
    let TemplateSlotTarget::NodeText { node_id } = &text_slot.target else {
        panic!("headline target is text");
    };
    let read_text = |project: &Project| match &project.graph.node(node_id).unwrap().kind {
        NodeKind::VectorText { text, .. } | NodeKind::Text3D { text, .. } => text.clone(),
        _ => panic!("slot still targets text node"),
    };
    assert_eq!(read_text(&project_a), "ALPHA");
    assert_eq!(read_text(&project_b), "BETA");
    assert!(
        project_a
            .param_bindings
            .iter()
            .any(|binding| { binding.param_name == "exposure" && binding.expression == "0.35" })
    );
}

#[test]
fn template_rejects_custom_wgsl_at_root_and_inside_nested_precomp() {
    let mut root_package = reviewed_package(Project::golden_scene_preset());
    let previous_output = root_package.project.graph.output_node.clone();
    let custom_id = NodeId::from("template_custom_shader");
    root_package.project.graph.nodes.push(GraphNodeDef {
        id: custom_id.clone(),
        kind: NodeKind::CustomWgsl {
            wgsl_source: "fn main() {}".into(),
            expected_inputs: 1,
            params: [0.0; 16],
        },
        inputs: vec![previous_output],
        enabled: true,
    });
    root_package.project.graph.output_node = custom_id;
    assert!(matches!(
        root_package.validate(),
        Err(frameshift_schema::SchemaError::InvalidTemplate { message, .. })
            if message.contains("CustomWgsl") && message.contains("template_custom_shader")
    ));

    let mut nested_package = reviewed_package(Project::golden_scene_preset());
    let nested_custom_id = NodeId::from("nested_custom_shader");
    let precomp_id = NodeId::from("template_nested_precomp");
    nested_package.project.graph.nodes.push(GraphNodeDef {
        id: precomp_id.clone(),
        kind: NodeKind::Precomp {
            sub_graph: RenderGraphDef {
                output_node: nested_custom_id.clone(),
                nodes: vec![
                    GraphNodeDef {
                        id: NodeId::from("nested_shader_input"),
                        kind: NodeKind::Solid {
                            color_rgba: [0.0, 0.0, 0.0, 1.0],
                            color_space: Default::default(),
                        },
                        inputs: vec![],
                        enabled: true,
                    },
                    GraphNodeDef {
                        id: nested_custom_id,
                        kind: NodeKind::CustomWgsl {
                            wgsl_source: "fn main() {}".into(),
                            expected_inputs: 1,
                            params: [0.0; 16],
                        },
                        inputs: vec![NodeId::from("nested_shader_input")],
                        enabled: true,
                    },
                ],
            },
            time_offset_ticks: 0,
            time_scale: 1.0,
            opacity: 1.0,
        },
        inputs: vec![],
        enabled: true,
    });
    nested_package.project.graph.output_node = precomp_id;
    assert!(matches!(
        nested_package.validate(),
        Err(frameshift_schema::SchemaError::InvalidTemplate { message, .. })
            if message.contains("CustomWgsl")
                && message.contains("template_nested_precomp/nested_custom_shader")
    ));
}

#[test]
fn template_requires_review_and_all_required_slots() {
    let mut package = reviewed_package(Project::golden_scene_preset());
    package.review.reviewed = false;
    package.review.reviewer = None;
    assert!(matches!(
        package.instantiate(&BTreeMap::new()),
        Err(frameshift_schema::SchemaError::UnreviewedTemplate(_))
    ));

    package.review.reviewed = true;
    package.review.reviewer = Some("reviewer".into());
    assert!(matches!(
        package.instantiate(&BTreeMap::new()),
        Err(frameshift_schema::SchemaError::MissingTemplateSlot(slot)) if slot == "footage"
    ));
    assert!(matches!(
        package.instantiate(&BTreeMap::from([(
            "unknown".into(),
            TemplateSlotValue::Text("value".into())
        )])),
        Err(frameshift_schema::SchemaError::UnknownTemplateSlot(slot)) if slot == "unknown"
    ));
}

#[test]
fn precompose_and_unprecompose_roundtrip_flat_graph() {
    let original = simple_graph();
    let mut graph = original.clone();
    precompose(&mut graph, &[NodeId::from("grade")], NodeId::from("group"))
        .expect("flat grade group can be precomposed");
    assert_eq!(graph.output_node, NodeId::from("group"));
    let group = graph.node(&NodeId::from("group")).unwrap();
    assert!(matches!(group.kind, NodeKind::Precomp { .. }));
    assert_eq!(group.inputs, vec![NodeId::from("plate")]);
    graph.validate_graph().expect("precomposed DAG validates");

    unprecompose(&mut graph, &NodeId::from("group")).expect("precomp opens cleanly");
    assert_eq!(graph, original);
}

#[test]
fn precompose_rejects_blend_boundary_transactionally() {
    let graph = RenderGraphDef {
        output_node: NodeId::from("blend"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("base"),
                kind: NodeKind::Solid {
                    color_rgba: [0.0, 0.0, 0.0, 1.0],
                    color_space: Default::default(),
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("top"),
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
                    mode: BlendMode::Normal,
                    opacity: 1.0,
                },
                inputs: vec![NodeId::from("base"), NodeId::from("top")],
                enabled: true,
            },
        ],
    };
    let mut candidate = graph.clone();
    assert!(matches!(
        precompose(
            &mut candidate,
            &[NodeId::from("blend")],
            NodeId::from("group")
        ),
        Err(frameshift_schema::SchemaError::InvalidPrecompOperation { .. })
    ));
    assert_eq!(
        candidate, graph,
        "a rejected edit must not mutate the graph"
    );
}

#[test]
fn portable_manifest_reports_missing_files_and_relinks_unique_match() {
    let temp = TempDir::new("relink");
    let project_dir = temp.path().join("project");
    let search_dir = temp.path().join("recovered");
    fs::create_dir_all(project_dir.join("media")).unwrap();
    fs::create_dir_all(&search_dir).unwrap();

    let mut project = Project::golden_scene_preset();
    project.assets[0].path = "media/original.bin".into();
    let original = project_dir.join("media/original.bin");
    fs::write(&original, b"sample media payload").unwrap();
    let manifest = ProjectMediaManifest::from_project(&project, project_dir.join("edit.json"));
    let json = manifest.to_json_pretty().unwrap();
    let restored = ProjectMediaManifest::from_json_str(&json).expect("manifest roundtrips");
    assert_eq!(restored, manifest);
    assert_eq!(
        manifest.assets[0].project_relative_path.as_deref(),
        Some("media/original.bin")
    );
    fs::remove_file(&original).unwrap();
    let recovered = search_dir.join("original.bin");
    fs::write(&recovered, b"sample media payload").unwrap();

    let missing = frameshift_schema::missing_project_media(&project, &project_dir);
    assert!(
        missing
            .iter()
            .any(|asset| asset.asset_id == project.assets[0].id)
    );
    let report = relink_missing_media(
        &mut project,
        &project_dir,
        std::slice::from_ref(&search_dir),
        Some(&manifest),
    );
    assert_eq!(report.relinked.len(), 1);
    assert!(report.missing_asset_ids.contains(&project.assets[1].id));
    assert_eq!(project.assets[0].path, recovered.to_string_lossy());
}

#[test]
fn package_manifest_rejects_paths_that_escape_local_package() {
    let mut package = reviewed_package(Project::golden_scene_preset());
    package
        .manifest
        .files
        .push(frameshift_schema::TemplatePackageFile {
            relative_path: "../outside.ttf".into(),
            role: frameshift_schema::TemplateFileRole::Font,
            license: Some("OFL-1.1".into()),
            redistribution: frameshift_schema::RedistributionStatus::Redistributable,
        });
    assert!(matches!(
        package.validate(),
        Err(frameshift_schema::SchemaError::InvalidTemplate { .. })
    ));
}

#[test]
fn template_extended_slot_kinds_serde_validation_and_instantiation() {
    let mut project = Project::golden_scene_preset();
    let text_node_id = NodeId::from("impact_title");
    let blend_node_id = NodeId::from("blend_atmosphere");
    let clip_id = project.timeline.clips[0].id.clone();

    if let NodeKind::VectorText { font_family, .. } =
        &mut project.graph.nodes.iter_mut().find(|n| n.id == text_node_id).unwrap().kind
    {
        *font_family = Some("Impact".into());
    }

    let mut package = reviewed_package(project);
    package.slots.extend([
        TemplateSlot {
            id: "title_color".into(),
            label: "Title Color".into(),
            kind: TemplateSlotKind::Color,
            target: TemplateSlotTarget::NodeColor {
                node_id: text_node_id.clone(),
                field: Some("color_rgba".into()),
            },
            required: true,
            default: Some(TemplateSlotValue::Color([1.0, 1.0, 1.0, 1.0])),
        },
        TemplateSlot {
            id: "atmo_blend".into(),
            label: "Atmosphere Blend".into(),
            kind: TemplateSlotKind::Choice,
            target: TemplateSlotTarget::NodeChoice {
                node_id: blend_node_id.clone(),
                field: "mode".into(),
            },
            required: true,
            default: Some(TemplateSlotValue::Choice("screen".into())),
        },
        TemplateSlot {
            id: "main_curve".into(),
            label: "Main Clip Speed".into(),
            kind: TemplateSlotKind::SpeedCurve,
            target: TemplateSlotTarget::ClipSpeedCurve {
                clip_id: clip_id.clone(),
            },
            required: false,
            default: Some(TemplateSlotValue::SpeedCurve(
                SpeedCurve::constant(1.5).unwrap(),
            )),
        },
        TemplateSlot {
            id: "title_font".into(),
            label: "Title Font".into(),
            kind: TemplateSlotKind::Font,
            target: TemplateSlotTarget::NodeFont {
                node_id: text_node_id.clone(),
            },
            required: false,
            default: Some(TemplateSlotValue::Font("Anton".into())),
        },
    ]);

    let json = package.to_json_pretty().expect("serialize template package");
    let restored: TemplatePackage = serde_json::from_str(&json).expect("deserialize package");
    assert_eq!(restored, package);
    restored.validate().expect("extended slots validate");

    let custom_curve = SpeedCurve::constant(2.0).unwrap();
    let values = BTreeMap::from([
        (
            "footage".into(),
            TemplateSlotValue::AssetPath("/media/new.mp4".into()),
        ),
        ("drop".into(), TemplateSlotValue::Tick(Flicks(5000))),
        (
            "title_color".into(),
            TemplateSlotValue::Color([0.2, 0.8, 1.0, 1.0]),
        ),
        (
            "atmo_blend".into(),
            TemplateSlotValue::Choice("overlay".into()),
        ),
        (
            "main_curve".into(),
            TemplateSlotValue::SpeedCurve(custom_curve.clone()),
        ),
        (
            "title_font".into(),
            TemplateSlotValue::Font("assets/fonts/BebasNeue.ttf".into()),
        ),
    ]);

    let instance = restored.instantiate(&values).expect("instantiate with extended slots");
    let text_node = instance.graph.node(&text_node_id).unwrap();
    assert!(matches!(
        &text_node.kind,
        NodeKind::VectorText {
            color_rgba,
            font_path: Some(fp),
            font_family: None,
            ..
        } if *color_rgba == [0.2, 0.8, 1.0, 1.0] && fp == "assets/fonts/BebasNeue.ttf"
    ));
    assert!(matches!(
        instance.graph.node(&blend_node_id).unwrap().kind,
        NodeKind::Blend {
            mode: BlendMode::Overlay,
            ..
        }
    ));
    assert_eq!(instance.clip(&clip_id).unwrap().speed_curve, custom_curve);

    // Reject invalid choice value or mismatched slot kind/target.
    let mut bad_choice = values.clone();
    bad_choice.insert(
        "atmo_blend".into(),
        TemplateSlotValue::Choice("not_a_blend_mode".into()),
    );
    assert!(matches!(
        restored.instantiate(&bad_choice),
        Err(SchemaError::InvalidTemplateSlot { slot_id, .. }) if slot_id == "atmo_blend"
    ));

    let mut bad_color = values;
    bad_color.insert(
        "title_color".into(),
        TemplateSlotValue::Color([-0.1, 0.0, 0.0, 1.0]),
    );
    assert!(matches!(
        restored.instantiate(&bad_color),
        Err(SchemaError::InvalidTemplateSlot { slot_id, .. }) if slot_id == "title_color"
    ));
}

#[test]
fn effect_chain_preset_serde_and_validation_rules() {
    let valid_preset = EffectChainPreset {
        schema_version: CURRENT_SCHEMA_VERSION,
        id: "xenoz_impact_glow".into(),
        name: "Xenoz Impact Glow".into(),
        category: "impact".into(),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("in_0"),
                kind: NodeKind::PrecompInput { slot: 0 },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("blur"),
                kind: NodeKind::BlurMoCurves {
                    shift_x: 0.05,
                    shift_y: 0.0,
                    z_dist: -0.1,
                    rotate_deg: 2.5,
                    blur_strength: 1.0,
                    samples: 16,
                    wrap_mode: 2,
                },
                inputs: vec![NodeId::from("in_0")],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("glow"),
                kind: serde_json::from_str(r#"{"type":"deep_glow"}"#).unwrap(),
                inputs: vec![NodeId::from("blur")],
                enabled: true,
            },
        ],
        param_bindings: vec![ParamBindingDef {
            node_id: NodeId::from("glow"),
            param_name: "intensity".into(),
            expression: "clamp(0.5 + audio_rms, 0.0, 2.0)".into(),
        }],
        slots: vec![
            TemplateSlot {
                id: "glow_tint".into(),
                label: "Glow Tint".into(),
                kind: TemplateSlotKind::Color,
                target: TemplateSlotTarget::NodeColor {
                    node_id: NodeId::from("glow"),
                    field: Some("tint_rgb".into()),
                },
                required: false,
                default: Some(TemplateSlotValue::Color([1.0, 0.4, 0.8, 1.0])),
            },
            TemplateSlot {
                id: "glow_strength".into(),
                label: "Glow Strength".into(),
                kind: TemplateSlotKind::Number,
                target: TemplateSlotTarget::ParameterBinding {
                    node_id: NodeId::from("glow"),
                    param_name: "intensity".into(),
                },
                required: false,
                default: Some(TemplateSlotValue::Number(0.8)),
            },
        ],
        review: TemplateReview {
            reviewed: true,
            reviewer: Some("fx-lead".into()),
            note: Some("Data-only preset".into()),
        },
    };

    validate_effect_chain_preset(&valid_preset).expect("valid preset passes");
    let json = valid_preset.to_json_pretty().expect("serialize preset");
    let restored: EffectChainPreset = serde_json::from_str(&json).expect("deserialize preset");
    assert_eq!(restored, valid_preset);
    restored.validate().expect("restored preset validates");

    // Reject CustomWgsl at root and inside nested Precomp.
    let mut wgsl_preset = valid_preset.clone();
    wgsl_preset.nodes.push(GraphNodeDef {
        id: NodeId::from("custom"),
        kind: NodeKind::CustomWgsl {
            wgsl_source: "return vec4<f32>(1.0);".into(),
            expected_inputs: 1,
            params: [0.0; 16],
        },
        inputs: vec![NodeId::from("glow")],
        enabled: true,
    });
    assert!(matches!(
        validate_effect_chain_preset(&wgsl_preset),
        Err(SchemaError::InvalidTemplate { message, .. }) if message.contains("CustomWgsl")
    ));

    let mut nested_wgsl = valid_preset.clone();
    nested_wgsl.nodes.push(GraphNodeDef {
        id: NodeId::from("sub"),
        kind: NodeKind::Precomp {
            sub_graph: RenderGraphDef {
                output_node: NodeId::from("nested_custom"),
                nodes: vec![
                    GraphNodeDef {
                        id: NodeId::from("sub_in"),
                        kind: NodeKind::PrecompInput { slot: 0 },
                        inputs: vec![],
                        enabled: true,
                    },
                    GraphNodeDef {
                        id: NodeId::from("nested_custom"),
                        kind: NodeKind::CustomWgsl {
                            wgsl_source: "return vec4<f32>(1.0);".into(),
                            expected_inputs: 1,
                            params: [0.0; 16],
                        },
                        inputs: vec![NodeId::from("sub_in")],
                        enabled: true,
                    },
                ],
            },
            time_offset_ticks: 0,
            time_scale: 1.0,
            opacity: 1.0,
        },
        inputs: vec![NodeId::from("glow")],
        enabled: true,
    });
    assert!(matches!(
        validate_effect_chain_preset(&nested_wgsl),
        Err(SchemaError::InvalidTemplate { message, .. }) if message.contains("CustomWgsl")
    ));

    // Reject cycle in preset.
    let mut cyclic = valid_preset.clone();
    cyclic.nodes[0].inputs = vec![NodeId::from("glow")];
    cyclic.nodes[0].kind = serde_json::from_str(r#"{"type":"color_basic"}"#).unwrap();
    assert!(matches!(
        validate_effect_chain_preset(&cyclic),
        Err(SchemaError::CycleDetected { .. })
    ));

    // Reject missing upstream connection.
    let mut broken_edge = valid_preset.clone();
    broken_edge.nodes[1].inputs = vec![NodeId::from("missing_node")];
    assert!(matches!(
        validate_effect_chain_preset(&broken_edge),
        Err(SchemaError::MissingNode(id)) if id == NodeId::from("missing_node")
    ));

    // Reject disconnected multiple sinks.
    let mut disconnected = valid_preset;
    disconnected.nodes.push(GraphNodeDef {
        id: NodeId::from("extra_solid"),
        kind: NodeKind::Solid {
            color_rgba: [1.0; 4],
            color_space: Default::default(),
        },
        inputs: vec![],
        enabled: true,
    });
    assert!(matches!(
        validate_effect_chain_preset(&disconnected),
        Err(SchemaError::InvalidTemplate { message, .. })
            if message.contains("single output node")
    ));

    // Reject incompatible schema_version or timeline-only slot target in EffectChainPreset.
    let mut bad_ver = valid_preset_copy_for_err();
    bad_ver.schema_version = frameshift_schema::SchemaVersion::new(0, 3, 0);
    assert!(matches!(
        validate_effect_chain_preset(&bad_ver),
        Err(SchemaError::UnsupportedSchemaVersion(_))
    ));
}

fn valid_preset_copy_for_err() -> EffectChainPreset {
    EffectChainPreset {
        schema_version: CURRENT_SCHEMA_VERSION,
        id: "err_probe".into(),
        name: "Err Probe".into(),
        category: "test".into(),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("s"),
            kind: NodeKind::Solid {
                color_rgba: [1.0; 4],
                color_space: Default::default(),
            },
            inputs: vec![],
            enabled: true,
        }],
        param_bindings: vec![],
        slots: vec![],
        review: TemplateReview::default(),
    }
}
