use frameshift_schema::{
    AnimationEasing, ClipTimingEdge, DisplacementMode, DisplacementWrap, EchoOperator,
    EventAnchorDef, EventOrigin, EventTimingConstraint, GraphNodeDef, KeyframeTrackDef, MatteMode,
    NodeId, NodeKind, ParamBindingDef, Project, ProjectCommand, ProjectSession, SchemaError,
    ShapeMaskKeyframe, ShapeMaskVertex, TrackKeyframe,
};
use frameshift_time::{FLICKS_PER_SECOND, Flicks};

#[test]
fn displacement_map_frame_echo_shape_mask_validate_and_roundtrip() {
    let mut project = Project::golden_scene_preset();
    let shape_id = NodeId::from("shape_mask_01");
    let matte_id = NodeId::from("shape_matte_01");
    let disp_id = NodeId::from("disp_map_01");
    let echo_id = NodeId::from("frame_echo_01");

    project.graph.nodes.push(GraphNodeDef {
        id: shape_id.clone(),
        kind: NodeKind::ShapeMask {
            vertices: vec![
                ShapeMaskVertex {
                    pos: [0.2, 0.2],
                    in_tangent: [0.0, 0.0],
                    out_tangent: [0.1, 0.0],
                },
                ShapeMaskVertex {
                    pos: [0.8, 0.2],
                    in_tangent: [-0.1, 0.0],
                    out_tangent: [0.0, 0.1],
                },
                ShapeMaskVertex {
                    pos: [0.5, 0.85],
                    in_tangent: [0.0, 0.0],
                    out_tangent: [0.0, 0.0],
                },
            ],
            keyframes: vec![ShapeMaskKeyframe {
                tick: Flicks(FLICKS_PER_SECOND),
                vertices: vec![
                    ShapeMaskVertex {
                        pos: [0.3, 0.3],
                        in_tangent: [0.0, 0.0],
                        out_tangent: [0.0, 0.0],
                    },
                    ShapeMaskVertex {
                        pos: [0.7, 0.3],
                        in_tangent: [0.0, 0.0],
                        out_tangent: [0.0, 0.0],
                    },
                    ShapeMaskVertex {
                        pos: [0.5, 0.7],
                        in_tangent: [0.0, 0.0],
                        out_tangent: [0.0, 0.0],
                    },
                ],
                easing: AnimationEasing::EaseInOut,
            }],
            closed: true,
            feather_px: 12.0,
            expand_px: 2.0,
            invert: false,
            opacity: 0.9,
        },
        inputs: vec![],
        enabled: true,
    });
    project.graph.nodes.push(GraphNodeDef {
        id: matte_id.clone(),
        kind: NodeKind::MaskMatte {
            mode: MatteMode::Alpha,
            invert: false,
            feather: 0.0,
            opaque_black_outside: false,
            opacity: 1.0,
            choke_px: 0.0,
            spatial_feather_px: 0.0,
        },
        inputs: vec![NodeId::from("src_a"), shape_id.clone()],
        enabled: true,
    });
    project.graph.nodes.push(GraphNodeDef {
        id: disp_id.clone(),
        kind: NodeKind::DisplacementMap {
            channel_mode: DisplacementMode::Luma,
            edge_mode: DisplacementWrap::Mirror,
            scale_x: 0.08,
            scale_y: 0.04,
            neutral_level: 0.5,
            angle_rad: 0.25,
            chromatic_split: 0.15,
            opacity: 1.0,
        },
        inputs: vec![matte_id, shape_id],
        enabled: true,
    });
    project.graph.nodes.push(GraphNodeDef {
        id: echo_id.clone(),
        kind: NodeKind::FrameEcho {
            echo_step_ticks: -29_400_000,
            echo_count: 6,
            starting_intensity: 0.8,
            decay: 0.65,
            color_decay_rgb: [0.85, 0.6, 1.0],
            hue_shift_deg: 12.0,
            operator: EchoOperator::CompositeUnder,
        },
        inputs: vec![disp_id.clone()],
        enabled: true,
    });
    project.graph.output_node = echo_id.clone();

    project.param_bindings.push(ParamBindingDef {
        node_id: disp_id,
        param_name: "scale_x".into(),
        expression: "0.05 + audio_sub * 0.1".into(),
    });
    project.param_bindings.push(ParamBindingDef {
        node_id: echo_id.clone(),
        param_name: "decay".into(),
        expression: "0.5 + 0.2 * progress".into(),
    });
    project.keyframe_tracks.push(KeyframeTrackDef {
        id: "echo_intensity_track".into(),
        target_node: echo_id,
        target_param: "starting_intensity".into(),
        keyframes: vec![
            TrackKeyframe {
                tick: Flicks::ZERO,
                value: 0.5,
                easing: AnimationEasing::Linear,
            },
            TrackKeyframe {
                tick: Flicks(FLICKS_PER_SECOND * 2),
                value: 1.2,
                easing: AnimationEasing::EaseOut,
            },
        ],
    });

    let json = project.to_json_pretty().expect("serialize");
    let restored = Project::from_json_str(&json).expect("deserialize & validate");
    assert_eq!(project, restored);
    assert!(NodeKind::ALL_SERDE_NAMES.contains(&"displacement_map"));
    assert!(NodeKind::ALL_SERDE_NAMES.contains(&"frame_echo"));
    assert!(NodeKind::ALL_SERDE_NAMES.contains(&"shape_mask"));
}

#[test]
fn event_timing_constraints_resolve_dag_fraction_edge_and_detect_cycles() {
    let mut project = Project::golden_scene_preset();
    // clip_a is [0, 3s]
    let clip_a_mid = Flicks(3 * FLICKS_PER_SECOND / 2); // 1.5s exact

    project.events.push(EventAnchorDef {
        event_id: "clip_mid".into(),
        label: "Clip A Midpoint".into(),
        source_clip_id: None,
        source_anchor_tick: None,
        target_output_tick: Flicks::ZERO,
        origin: EventOrigin::Detected,
        timing: EventTimingConstraint::RelativeToClipFraction {
            clip_id: "clip_a".into(),
            fraction_num: 1,
            fraction_den: 2,
            offset_ticks: Flicks::ZERO,
        },
    });
    project.events.push(EventAnchorDef {
        event_id: "anticipation".into(),
        label: "Pre-Mid Anticipation".into(),
        source_clip_id: None,
        source_anchor_tick: None,
        target_output_tick: Flicks::ZERO,
        origin: EventOrigin::UserMarked,
        timing: EventTimingConstraint::RelativeToEvent {
            parent_event_id: "clip_mid".into(),
            offset_ticks: Flicks(-29_400_000),
        },
    });
    project.events.push(EventAnchorDef {
        event_id: "clip_end_hit".into(),
        label: "Clip A End".into(),
        source_clip_id: None,
        source_anchor_tick: None,
        target_output_tick: Flicks::ZERO,
        origin: EventOrigin::UserMarked,
        timing: EventTimingConstraint::RelativeToClipEdge {
            clip_id: "clip_a".into(),
            edge: ClipTimingEdge::End,
            offset_ticks: Flicks(100),
        },
    });

    // Before recompute, validation detects out-of-sync target_output_tick
    assert!(matches!(
        project.validate(),
        Err(SchemaError::InvalidEventConstraint { .. })
    ));

    project.recompute_event_constraints().expect("resolve DAG");
    project.validate().expect("valid after recompute");
    assert_eq!(project.event("clip_mid").unwrap().target_output_tick, clip_a_mid);
    assert_eq!(
        project.event("anticipation").unwrap().target_output_tick,
        Flicks(clip_a_mid.0 - 29_400_000)
    );
    assert_eq!(
        project.event("clip_end_hit").unwrap().target_output_tick,
        Flicks(3 * FLICKS_PER_SECOND + 100)
    );

    // Session propagates chained events when TrimClip changes clip_a
    let mut session = ProjectSession::new(project.clone()).expect("session");
    session
        .apply_command(
            1,
            ProjectCommand::TrimClip {
                clip_id: "clip_a".into(),
                new_timeline_start: None,
                new_timeline_end: Some(Flicks(2 * FLICKS_PER_SECOND)),
            },
        )
        .expect("trim clip propagates");
    assert_eq!(
        session.project().event("clip_mid").unwrap().target_output_tick,
        Flicks(FLICKS_PER_SECOND)
    );
    assert_eq!(
        session.project().event("anticipation").unwrap().target_output_tick,
        Flicks(FLICKS_PER_SECOND - 29_400_000)
    );

    // Cycle detection: A -> B -> A
    let mut cyclic = project;
    cyclic.events.push(EventAnchorDef {
        event_id: "cyc_a".into(),
        label: "A".into(),
        source_clip_id: None,
        source_anchor_tick: None,
        target_output_tick: Flicks::ZERO,
        origin: EventOrigin::UserMarked,
        timing: EventTimingConstraint::RelativeToEvent {
            parent_event_id: "cyc_b".into(),
            offset_ticks: Flicks::ZERO,
        },
    });
    cyclic.events.push(EventAnchorDef {
        event_id: "cyc_b".into(),
        label: "B".into(),
        source_clip_id: None,
        source_anchor_tick: None,
        target_output_tick: Flicks::ZERO,
        origin: EventOrigin::UserMarked,
        timing: EventTimingConstraint::RelativeToEvent {
            parent_event_id: "cyc_a".into(),
            offset_ticks: Flicks::ZERO,
        },
    });
    assert!(matches!(
        cyclic.recompute_event_constraints(),
        Err(SchemaError::CyclicEventConstraint { .. })
    ));
}

#[test]
fn keyframe_tracks_and_legacy_020_json_compatibility() {
    let mut project = Project::golden_scene_preset();
    let legacy_json = project.to_json_pretty().unwrap();
    assert!(!legacy_json.contains("keyframe_tracks"));
    assert!(!legacy_json.contains("\"timing\""));

    let parsed = Project::from_json_str(&legacy_json).expect("legacy 0.2.0 loads");
    assert!(parsed.keyframe_tracks.is_empty());

    // Reject unsorted keyframe track
    project.keyframe_tracks.push(KeyframeTrackDef {
        id: "unsorted".into(),
        target_node: NodeId::from("grade_flash"),
        target_param: "flash_amount".into(),
        keyframes: vec![
            TrackKeyframe {
                tick: Flicks(100),
                value: 1.0,
                easing: AnimationEasing::Linear,
            },
            TrackKeyframe {
                tick: Flicks(50),
                value: 0.5,
                easing: AnimationEasing::Linear,
            },
        ],
    });
    assert!(matches!(
        project.validate(),
        Err(SchemaError::InvalidKeyframeTrack { .. })
    ));
}
