use frameshift_schema::{
    BlendMode, GraphNodeDef, NodeId, NodeKind, Project, ProjectMediaManifest, RenderGraphDef,
    TemplatePackage, TemplatePackageManifest, TemplateReview, TemplateSlot, TemplateSlotKind,
    TemplateSlotTarget, TemplateSlotValue, precompose, relink_missing_media, unprecompose,
};
use frameshift_time::Flicks;
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
