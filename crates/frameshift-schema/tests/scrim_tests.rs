//! BackdropScrim schema contract: serde defaults, arity, serde-name parity, and
//! bindable parameter names. Legacy project JSON must stay loadable.

use frameshift_schema::{
    ColorSpace, CURRENT_SCHEMA_VERSION, GraphNodeDef, NodeId, NodeKind, ParamBindingDef, Project,
    RenderGraphDef, RenderProfileDef, RenderProfileKind, RenderProfilesDef, Resolution, SchemaError,
    TimelineDef, WorkingPixelFormat,
};
use frameshift_time::{FLICKS_PER_SECOND, Flicks, FPS_60};

fn solid_node(id: &str) -> GraphNodeDef {
    GraphNodeDef {
        id: NodeId::from(id),
        kind: NodeKind::Solid {
            color_rgba: [0.0, 0.0, 0.0, 1.0],
            color_space: ColorSpace::LinearRec709,
        },
        inputs: vec![],
        enabled: true,
    }
}

fn scrim_node(inputs: Vec<&str>) -> GraphNodeDef {
    GraphNodeDef {
        id: NodeId::from("scrim"),
        kind: NodeKind::BackdropScrim {
            rect_center_norm: [0.5, 0.5],
            rect_size_norm: [0.5, 0.2],
            corner_radius_norm: 0.02,
            blur_radius_norm: 0.02,
            fill_rgba: [0.0, 0.0, 0.0, 0.55],
            opacity: 1.0,
            auto_contrast: 0.0,
        },
        inputs: inputs.into_iter().map(NodeId::from).collect(),
        enabled: true,
    }
}

fn project_with(graph: RenderGraphDef) -> Project {
    Project {
        schema_version: CURRENT_SCHEMA_VERSION,
        revision_id: 1,
        name: "scrim schema probe".into(),
        assets: vec![],
        timeline: TimelineDef {
            fps: FPS_60,
            duration_ticks: Flicks(FLICKS_PER_SECOND),
            resolution: Resolution {
                width: 64,
                height: 36,
            },
            clips: vec![],
            audio_clips: vec![],
            ducking_rules: vec![],
            micro_silences: vec![],
        },
        events: vec![],
        graph,
        render_profiles: RenderProfilesDef {
            draft: RenderProfileDef {
                kind: RenderProfileKind::Draft,
                resolution: Resolution {
                    width: 32,
                    height: 18,
                },
                motion_blur_sub_samples: 4,
                shutter_angle_deg: 180.0,
                pixel_format: WorkingPixelFormat::Rgba16FloatPremul,
            },
            final_profile: RenderProfileDef {
                kind: RenderProfileKind::Final,
                resolution: Resolution {
                    width: 64,
                    height: 36,
                },
                motion_blur_sub_samples: 16,
                shutter_angle_deg: 180.0,
                pixel_format: WorkingPixelFormat::Rgba16FloatPremul,
            },
        },
        param_bindings: vec![],
        keyframe_tracks: vec![],
        creative_decisions: vec![],
        preferences: vec![],
        proxy_config: Default::default(),
        hw_accel_policy: Default::default(),
    }
}

#[test]
fn minimal_backdrop_scrim_json_uses_documented_defaults() {
    let kind: NodeKind = serde_json::from_str(r#"{"type":"backdrop_scrim"}"#)
        .expect("minimal backdrop_scrim JSON must deserialize");

    match kind {
        NodeKind::BackdropScrim {
            rect_center_norm,
            rect_size_norm,
            corner_radius_norm,
            blur_radius_norm,
            fill_rgba,
            opacity,
            auto_contrast,
        } => {
            assert_eq!(rect_center_norm, [0.5, 0.5]);
            assert_eq!(rect_size_norm, [0.5, 0.2]);
            assert_eq!(corner_radius_norm, 0.02);
            assert_eq!(blur_radius_norm, 0.02);
            assert_eq!(fill_rgba, [0.0, 0.0, 0.0, 0.55]);
            assert_eq!(opacity, 1.0);
            assert_eq!(auto_contrast, 0.0);
        }
        other => panic!("expected BackdropScrim, got {other:?}"),
    }
}

#[test]
fn partial_backdrop_scrim_json_keeps_authored_fields() {
    let kind: NodeKind = serde_json::from_str(
        r#"{
            "type": "backdrop_scrim",
            "rect_center_norm": [0.25, 0.75],
            "rect_size_norm": [0.6, 0.3],
            "corner_radius_norm": 0.05,
            "blur_radius_norm": 0.08,
            "fill_rgba": [0.1, 0.2, 0.3, 0.4],
            "opacity": 0.5,
            "auto_contrast": 0.75
        }"#,
    )
    .expect("full backdrop_scrim JSON must deserialize");

    let round_trip = serde_json::to_value(&kind).expect("serialize backdrop_scrim");
    assert_eq!(round_trip["type"], "backdrop_scrim");
    let blur = round_trip["blur_radius_norm"].as_f64().unwrap();
    assert!((blur - 0.08).abs() < 1e-6, "blur radius round-trip: {blur}");
    let auto = round_trip["auto_contrast"].as_f64().unwrap();
    assert!((auto - 0.75).abs() < 1e-6, "auto_contrast round-trip: {auto}");
    let restored: NodeKind = serde_json::from_value(round_trip).expect("round-trip deserialize");
    assert_eq!(restored, kind);
}

#[test]
fn backdrop_scrim_serde_name_is_listed_exactly_once() {
    let matches = NodeKind::ALL_SERDE_NAMES
        .iter()
        .filter(|name| **name == "backdrop_scrim")
        .count();
    assert_eq!(
        matches, 1,
        "ALL_SERDE_NAMES must list backdrop_scrim exactly once"
    );

    let serialized = serde_json::to_value(scrim_node(vec!["bg"]).kind).expect("serialize");
    assert_eq!(serialized["type"], "backdrop_scrim");
    let from_tag: NodeKind = serde_json::from_str(r#"{"type":"backdrop_scrim"}"#).unwrap();
    assert!(matches!(from_tag, NodeKind::BackdropScrim { .. }));

    let mut sorted = NodeKind::ALL_SERDE_NAMES.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        NodeKind::ALL_SERDE_NAMES.len(),
        "ALL_SERDE_NAMES must not contain duplicate tags"
    );
}

#[test]
fn backdrop_scrim_requires_exactly_one_input() {
    let zero = RenderGraphDef {
        output_node: NodeId::from("scrim"),
        nodes: vec![scrim_node(vec![])],
    };
    assert!(matches!(
        zero.validate_graph(),
        Err(SchemaError::InvalidNodeArity {
            expected: 1,
            actual: 0,
            ..
        })
    ));

    let two = RenderGraphDef {
        output_node: NodeId::from("scrim"),
        nodes: vec![
            solid_node("a"),
            solid_node("b"),
            scrim_node(vec!["a", "b"]),
        ],
    };
    assert!(matches!(
        two.validate_graph(),
        Err(SchemaError::InvalidNodeArity {
            expected: 1,
            actual: 2,
            ..
        })
    ));

    let one = RenderGraphDef {
        output_node: NodeId::from("scrim"),
        nodes: vec![solid_node("bg"), scrim_node(vec!["bg"])],
    };
    assert!(one.validate_graph().is_ok());
    assert_eq!(
        scrim_node(vec!["bg"]).kind.expected_input_count(),
        1
    );
}

#[test]
fn backdrop_scrim_bindable_params_validate_and_reject_unknown_names() {
    let graph = RenderGraphDef {
        output_node: NodeId::from("scrim"),
        nodes: vec![solid_node("bg"), scrim_node(vec!["bg"])],
    };
    let bindable = [
        "rect_center_x",
        "rect_center_y",
        "rect_size_x",
        "rect_size_y",
        "corner_radius_norm",
        "blur_radius_norm",
        "opacity",
        "auto_contrast",
    ];

    for name in bindable {
        let mut project = project_with(graph.clone());
        project.param_bindings = vec![ParamBindingDef {
            node_id: NodeId::from("scrim"),
            param_name: name.into(),
            expression: "0.25".into(),
        }];
        assert!(
            project.validate().is_ok(),
            "'{name}' must be a bindable BackdropScrim parameter"
        );
    }

    let mut project = project_with(graph);
    project.param_bindings = vec![ParamBindingDef {
        node_id: NodeId::from("scrim"),
        param_name: "fill_rgba".into(),
        expression: "0.25".into(),
    }];
    assert!(matches!(
        project.validate(),
        Err(SchemaError::InvalidParamBinding { .. })
    ));
}

#[test]
fn legacy_project_json_without_scrim_still_loads() {
    let legacy = r#"{
        "schema_version": "0.1.0",
        "revision_id": 3,
        "name": "legacy pre-scrim project",
        "assets": [],
        "timeline": {
            "fps": { "num": 60, "den": 1 },
            "duration_ticks": 705600000,
            "resolution": { "width": 64, "height": 36 },
            "clips": [],
            "audio_clips": []
        },
        "events": [],
        "graph": {
            "output_node": "out",
            "nodes": [
                { "id": "bg", "kind": { "type": "solid", "color_rgba": [0.0, 0.0, 0.0, 1.0] } },
                { "id": "out", "kind": { "type": "film_optics", "grain_amount": 0.4 }, "inputs": ["bg"] }
            ]
        },
        "render_profiles": {
            "draft": {
                "kind": "draft",
                "resolution": { "width": 32, "height": 18 },
                "motion_blur_sub_samples": 4,
                "shutter_angle_deg": 180.0
            },
            "final": {
                "kind": "final",
                "resolution": { "width": 64, "height": 36 },
                "motion_blur_sub_samples": 16,
                "shutter_angle_deg": 180.0
            }
        }
    }"#;

    let project = Project::from_json_str(legacy).expect("legacy 0.1.0 project must load");
    assert!(project.param_bindings.is_empty());
    assert_eq!(project.graph.nodes.len(), 2);
    assert!(project.validate().is_ok(), "legacy project must validate");

    let film_optics = &project.graph.nodes[1].kind;
    assert!(matches!(
        film_optics,
        NodeKind::FilmOptics {
            grain_amount,
            halation_amount,
            ..
        } if (*grain_amount - 0.4).abs() < 1e-6 && *halation_amount > 0.0
    ));
}
