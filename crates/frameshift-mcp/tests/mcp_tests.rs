use frameshift_mcp::{run_m3_agent_director_verification, McpServer};
use frameshift_schema::{PreferenceEntry, PreferenceScope, Project, ProjectCommand, Resolution};
use frameshift_time::{Flicks, FLICKS_PER_SECOND};
use serde_json::{json, Value};
use std::path::PathBuf;

struct TempDirGuard(PathBuf);

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn unique_temp_dir(label: &str) -> TempDirGuard {
    let dir = std::env::temp_dir().join(format!(
        "frameshift_mcp_test_{label}_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp test dir");
    TempDirGuard(dir)
}

#[test]
fn jsonrpc_tools_optimistic_lock_and_preference_precedence() {
    let mut project = Project::golden_scene_preset();
    project.render_profiles.draft.resolution = Resolution {
        width: 320,
        height: 180,
    };
    let mut server = McpServer::new(project).expect("valid server");

    // Malformed JSON -> -32700 parse error.
    let bad: Value = serde_json::from_str(&server.handle_jsonrpc_str("{not valid json")).unwrap();
    assert_eq!(bad["error"]["code"], -32700);

    // Unknown method -> -32601.
    let unknown: Value = serde_json::from_str(
        &server.handle_jsonrpc_str(r#"{"jsonrpc":"2.0","id":1,"method":"nope"}"#),
    )
    .unwrap();
    assert_eq!(unknown["error"]["code"], -32601);

    // Set Global, Project, and Scene preferences for the same key and verify precedence.
    let r1: Value = serde_json::from_str(
        &server.handle_jsonrpc_str(
            &json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "mutate_project",
                "params": {
                    "expected_revision_id": 1,
                    "command": ProjectCommand::SetPreference(PreferenceEntry {
                        key: "shake_mode".into(),
                        value: "global_gentle".into(),
                        scope: PreferenceScope::Global,
                        target_id: None,
                    })
                }
            })
            .to_string(),
        ),
    )
    .unwrap();
    assert_eq!(r1["result"]["revision_id"], 2);

    let r2: Value = serde_json::from_str(
        &server.handle_jsonrpc_str(
            &json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "mutate_project",
                "params": {
                    "expected_revision_id": 2,
                    "command": ProjectCommand::SetPreference(PreferenceEntry {
                        key: "shake_mode".into(),
                        value: "project_punchy".into(),
                        scope: PreferenceScope::Project,
                        target_id: None,
                    })
                }
            })
            .to_string(),
        ),
    )
    .unwrap();
    assert_eq!(r2["result"]["revision_id"], 3);

    let r3: Value = serde_json::from_str(
        &server.handle_jsonrpc_str(
            &json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "mutate_project",
                "params": {
                    "expected_revision_id": 3,
                    "command": ProjectCommand::SetPreference(PreferenceEntry {
                        key: "shake_mode".into(),
                        value: "scene_drop_only".into(),
                        scope: PreferenceScope::Scene,
                        target_id: Some("drop_scene".into()),
                    })
                }
            })
            .to_string(),
        ),
    )
    .unwrap();
    assert_eq!(r3["result"]["revision_id"], 4);

    // Query with scene_id="drop_scene" -> Scene wins.
    let q_scene: Value = serde_json::from_str(
        &server.handle_jsonrpc_str(
            &json!({
                "jsonrpc": "2.0",
                "id": 5,
                "method": "query_preferences",
                "params": { "key": "shake_mode", "scene_id": "drop_scene" }
            })
            .to_string(),
        ),
    )
    .unwrap();
    assert_eq!(q_scene["result"]["effective"]["value"], "scene_drop_only");
    assert_eq!(q_scene["result"]["resolved_scope"], "scene");
    assert_eq!(q_scene["result"]["global_layer"]["value"], "global_gentle");

    // Query without scene_id -> Project wins over Global, Scene does NOT leak.
    let q_proj: Value = serde_json::from_str(
        &server.handle_jsonrpc_str(
            &json!({
                "jsonrpc": "2.0",
                "id": 6,
                "method": "query_preferences",
                "params": { "key": "shake_mode" }
            })
            .to_string(),
        ),
    )
    .unwrap();
    assert_eq!(q_proj["result"]["effective"]["value"], "project_punchy");
    assert_eq!(q_proj["result"]["resolved_scope"], "project");
    assert!(q_proj["result"]["scene_layer"].is_null());

    // Stale expected_revision_id must return -32009 RevisionConflict.
    let stale: Value = serde_json::from_str(
        &server.handle_jsonrpc_str(
            &json!({
                "jsonrpc": "2.0",
                "id": 7,
                "method": "mutate_project",
                "params": {
                    "expected_revision_id": 1,
                    "command": ProjectCommand::SetProjectName { name: "stale".into() }
                }
            })
            .to_string(),
        ),
    )
    .unwrap();
    assert_eq!(stale["error"]["code"], -32009);
    assert_eq!(stale["error"]["data"]["actual_revision_id"], 4);

    // Single-frame render via tools/call.
    let temp = unique_temp_dir("render_frame");
    let frame_out = temp.0.join("impact.png");
    let render_res: Value = serde_json::from_str(
        &server.handle_jsonrpc_str(
            &json!({
                "jsonrpc": "2.0",
                "id": 8,
                "method": "tools/call",
                "params": {
                    "name": "render_project",
                    "arguments": {
                        "profile": "draft",
                        "out_path": frame_out.to_string_lossy(),
                        "tick": Flicks(2 * FLICKS_PER_SECOND + FLICKS_PER_SECOND / 2).0
                    }
                }
            })
            .to_string(),
        ),
    )
    .unwrap();
    assert_eq!(render_res["result"]["structuredContent"]["mode"], "frame");
    assert!(frame_out.is_file());
}

#[test]
fn m3_agent_director_end_to_end_verification_passes() {
    let temp = unique_temp_dir("m3_director");
    let summary =
        run_m3_agent_director_verification(&temp.0).expect("M3 verification should succeed");
    assert!(summary.all_gates_passed, "M3 failed: {summary:?}");
    assert!(!summary.global_preference_leaked);
    assert!(summary.stale_revision_rejected);
    assert!(summary.inspection_manifest_path.is_file());
    assert!(summary.report_path.is_file());
}

#[test]
fn new_mcp_tools_and_render_options_work_end_to_end() {
    let mut project = Project::golden_scene_preset();
    for (asset, synth) in project.assets.iter_mut().zip([
        "synthetic://clip_a",
        "synthetic://clip_b",
        "synthetic://beat",
    ]) {
        asset.path = synth.to_owned();
    }
    let mut server = McpServer::new(project).expect("valid server");

    // tools/list advertises all new tools and render_project platform/revision params.
    let tools_resp: Value = serde_json::from_str(
        &server.handle_jsonrpc_str(
            &json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {} })
                .to_string(),
        ),
    )
    .unwrap();
    let tools = tools_resp["result"]["tools"].as_array().unwrap();
    for required in [
        "undo",
        "redo",
        "preflight_project",
        "plan_autocut",
        "list_effect_presets",
        "apply_effect_preset",
    ] {
        assert!(
            tools.iter().any(|t| t["name"] == required),
            "missing tool {required}"
        );
    }
    let render_tool = tools.iter().find(|t| t["name"] == "render_project").unwrap();
    assert!(render_tool["inputSchema"]["properties"]["platform_preset"].is_object());
    assert!(render_tool["inputSchema"]["properties"]["expected_revision_id"].is_object());

    // preflight_project passes on synthetic project.
    let preflight_resp: Value = serde_json::from_str(
        &server.handle_jsonrpc_str(
            &json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {
                    "name": "preflight_project",
                    "arguments": { "check_tools": false, "expected_revision_id": 1 }
                }
            })
            .to_string(),
        ),
    )
    .unwrap();
    assert_eq!(preflight_resp["result"]["structuredContent"]["ok"], true);

    // plan_autocut plans beat-aligned boundaries on synthetic://beat.
    let autocut_resp: Value = serde_json::from_str(
        &server.handle_jsonrpc_str(
            &json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": {
                    "name": "plan_autocut",
                    "arguments": {
                        "start_tick": 0,
                        "duration_ticks": FLICKS_PER_SECOND * 4,
                        "min_segment_ticks": FLICKS_PER_SECOND / 2,
                        "include_start": true,
                        "include_end": true
                    }
                }
            })
            .to_string(),
        ),
    )
    .unwrap();
    let boundaries = autocut_resp["result"]["structuredContent"]["boundaries"]
        .as_array()
        .unwrap();
    assert!(boundaries.len() >= 3, "expected >=3 autocut boundaries");

    // list_effect_presets exposes built-in chains and filters by category.
    let presets_resp: Value = serde_json::from_str(
        &server.handle_jsonrpc_str(
            &json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "tools/call",
                "params": {
                    "name": "list_effect_presets",
                    "arguments": { "category": "impact" }
                }
            })
            .to_string(),
        ),
    )
    .unwrap();
    let chains = presets_resp["result"]["structuredContent"]["chain_presets"]
        .as_array()
        .unwrap();
    assert!(!chains.is_empty());
    assert!(chains.iter().all(|c| c["category"] == "impact"));

    // apply_effect_preset splices built-in preset with slot values and records a receipt.
    let rev_before = server.revision_id();
    let apply_resp: Value = serde_json::from_str(
        &server.handle_jsonrpc_str(
            &json!({
                "jsonrpc": "2.0",
                "id": 5,
                "method": "tools/call",
                "params": {
                    "name": "apply_effect_preset",
                    "arguments": {
                        "expected_revision_id": rev_before,
                        "preset_id": "xenoz_impact_glow",
                        "node_id_prefix": "mcp_glow",
                        "slot_values": {
                            "glow_intensity": { "type": "number", "value": 1.4 }
                        }
                    }
                }
            })
            .to_string(),
        ),
    )
    .unwrap();
    let rev_after_apply = apply_resp["result"]["structuredContent"]["revision_id"]
        .as_u64()
        .unwrap();
    assert_eq!(rev_after_apply, rev_before + 1);
    assert!(
        server
            .project()
            .graph
            .nodes
            .iter()
            .any(|n| n.id.0.starts_with("mcp_glow_"))
    );

    // undo and redo update revision monotonically while restoring/reapplying graph state.
    let undo_resp: Value = serde_json::from_str(
        &server.handle_jsonrpc_str(
            &json!({
                "jsonrpc": "2.0",
                "id": 6,
                "method": "tools/call",
                "params": {
                    "name": "undo",
                    "arguments": { "expected_revision_id": rev_after_apply }
                }
            })
            .to_string(),
        ),
    )
    .unwrap();
    let rev_after_undo = undo_resp["result"]["structuredContent"]["revision_id"]
        .as_u64()
        .unwrap();
    assert_eq!(rev_after_undo, rev_after_apply + 1);
    assert_eq!(undo_resp["result"]["structuredContent"]["can_redo"], true);
    assert!(
        !server
            .project()
            .graph
            .nodes
            .iter()
            .any(|n| n.id.0.starts_with("mcp_glow_"))
    );

    let redo_resp: Value = serde_json::from_str(
        &server.handle_jsonrpc_str(
            &json!({
                "jsonrpc": "2.0",
                "id": 7,
                "method": "tools/call",
                "params": {
                    "name": "redo",
                    "arguments": { "expected_revision_id": rev_after_undo }
                }
            })
            .to_string(),
        ),
    )
    .unwrap();
    let rev_after_redo = redo_resp["result"]["structuredContent"]["revision_id"]
        .as_u64()
        .unwrap();
    assert_eq!(rev_after_redo, rev_after_undo + 1);
    assert!(
        server
            .project()
            .graph
            .nodes
            .iter()
            .any(|n| n.id.0.starts_with("mcp_glow_"))
    );

    // render_project rejects stale expected_revision_id with -32009 and rejects unknown platform_preset.
    let stale_render: Value = serde_json::from_str(
        &server.handle_jsonrpc_str(
            &json!({
                "jsonrpc": "2.0",
                "id": 8,
                "method": "tools/call",
                "params": {
                    "name": "render_project",
                    "arguments": {
                        "profile": "draft",
                        "out_path": "unused.mp4",
                        "expected_revision_id": 1
                    }
                }
            })
            .to_string(),
        ),
    )
    .unwrap();
    assert_eq!(stale_render["error"]["code"], -32009);

    let bad_platform: Value = serde_json::from_str(
        &server.handle_jsonrpc_str(
            &json!({
                "jsonrpc": "2.0",
                "id": 9,
                "method": "tools/call",
                "params": {
                    "name": "render_project",
                    "arguments": {
                        "profile": "final",
                        "out_path": "unused.mp4",
                        "platform_preset": "unknown_platform"
                    }
                }
            })
            .to_string(),
        ),
    )
    .unwrap();
    assert!(bad_platform["error"]["message"]
        .as_str()
        .unwrap()
        .contains("unknown platform_preset"));
}

#[test]
fn wave4_mcp_tools_cover_lint_diff_dsl_template_tracker_nle_and_ai_manifest() {
    use frameshift_media::{AlphaMode, FfmpegBridge, Rgba8Frame};
    use frameshift_schema::{
        BlendMode, ColorSpace, GraphNodeDef, NodeId, NodeKind, RenderGraphDef, Resolution,
        SourceFit,
    };
    use frameshift_time::{FLICKS_PER_SECOND, FPS_24, SpeedCurve};

    let tmp = unique_temp_dir("wave4");

    let mut project = Project::golden_scene_preset();
    project.param_bindings.clear();
    project.timeline.clips[0].event_animations.clear();
    project.timeline.fps = FPS_24;
    project.timeline.duration_ticks = Flicks(FLICKS_PER_SECOND / 6); // 4 frames
    project.timeline.clips.truncate(1);
    project.timeline.clips[0].timeline_end = project.timeline.duration_ticks;
    project.timeline.clips[0].speed_curve = SpeedCurve::constant(1.0).unwrap();
    project.render_profiles.draft.resolution = Resolution {
        width: 96,
        height: 64,
    };
    project.render_profiles.draft.motion_blur_sub_samples = 1;
    project.graph = RenderGraphDef {
        output_node: NodeId::from("corner_out"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("src_a"),
                kind: NodeKind::SourceVideo {
                    asset_id: "asset_clip_a".into(),
                    clip_id: Some("clip_a".into()),
                    color_space: ColorSpace::DisplaySrgb,
                    fit: SourceFit::Stretch,
                },
                inputs: Vec::new(),
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("follow_2d"),
                kind: NodeKind::Transform2D {
                    translate: [0.0, 0.0],
                    scale: [1.0, 1.0],
                    rotation_rad: 0.0,
                    anchor: [0.5, 0.5],
                    filter: Default::default(),
                },
                inputs: vec![NodeId::from("src_a")],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("corner_out"),
                kind: NodeKind::CornerPin {
                    tl: [0.0, 0.0],
                    tr: [1.0, 0.0],
                    br: [1.0, 1.0],
                    bl: [0.0, 1.0],
                    opacity: 1.0,
                    filter: Default::default(),
                },
                inputs: vec![NodeId::from("follow_2d")],
                enabled: true,
            },
        ],
    };

    let mut server = McpServer::new(project).expect("valid server");

    // 1. tools/list & describe_schema coverage.
    let tools_res: Value = serde_json::from_str(
        &server.handle_jsonrpc_str(
            &json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {} })
                .to_string(),
        ),
    )
    .unwrap();
    let tools = tools_res["result"]["tools"].as_array().unwrap();
    for expected in [
        "lint_project",
        "diff_frame",
        "compile_motion_dsl",
        "template_metadata",
        "template_instantiate",
        "track_point",
        "track_planar",
        "export_nle",
        "import_nle",
        "ai_models",
        "ai_run_manifest",
    ] {
        assert!(
            tools.iter().any(|t| t["name"] == expected),
            "tools/list missing '{expected}'"
        );
    }

    let desc = server.describe_schema();
    for kind in ["displacement_map", "frame_echo", "shape_mask"] {
        assert!(desc.node_kinds.iter().any(|k| k == kind));
    }
    for cmd in ["upsert_keyframe_track", "remove_keyframe_track"] {
        assert!(desc.project_commands.iter().any(|c| c == cmd));
    }

    // 2. lint_project (clean -> ok=true, then inject off-screen text -> LNT-TXT-001).
    let lint_ok: Value = serde_json::from_str(
        &server.handle_jsonrpc_str(
            &json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": { "name": "lint_project", "arguments": { "check_media": false } }
            })
            .to_string(),
        ),
    )
    .unwrap();
    assert!(lint_ok["result"]["structuredContent"]["ok"].as_bool().unwrap());

    // 3. diff_frame on two PNG files + 3-panel heatmap output.
    let png_a = tmp.0.join("a.png");
    let png_b = tmp.0.join("b.png");
    let png_diff = tmp.0.join("diff.png");
    let frame_a =
        Rgba8Frame::filled(32, 32, Flicks::ZERO, [10, 20, 30, 255], AlphaMode::Straight).unwrap();
    let mut frame_b = frame_a.clone();
    frame_b.set_pixel(5, 6, [240, 200, 100, 255]);
    frame_a.save_png(&png_a).unwrap();
    frame_b.save_png(&png_b).unwrap();

    let diff_res: Value = serde_json::from_str(
        &server.handle_jsonrpc_str(
            &json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": {
                    "name": "diff_frame",
                    "arguments": {
                        "path_a": png_a,
                        "path_b": png_b,
                        "threshold": 2,
                        "out_diff_png": png_diff
                    }
                }
            })
            .to_string(),
        ),
    )
    .unwrap();
    let diff_sc = &diff_res["result"]["structuredContent"];
    assert_eq!(diff_sc["changed_pixels"], 1);
    assert_eq!(diff_sc["changed_bbox"], json!([5, 6, 1, 1]));
    assert!(png_diff.is_file());

    // 4. compile_motion_dsl.
    let dsl_script = r#"
---
name: "MCP DSL Scene"
fps: 24
res: 160x90
---
asset clip_a video path="synthetic://clip_a"
shot c1 asset=clip_a range=0s..1s src=0s fit=cover
text "OK" at=0s..1s pos=(0.5, 0.5) size=0.08 color=#FFFFFF
"#;
    let dsl_res: Value = serde_json::from_str(
        &server.handle_jsonrpc_str(
            &json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "tools/call",
                "params": {
                    "name": "compile_motion_dsl",
                    "arguments": { "input": dsl_script }
                }
            })
            .to_string(),
        ),
    )
    .unwrap();
    assert_eq!(
        dsl_res["result"]["structuredContent"]["project"]["name"],
        "MCP DSL Scene"
    );
    assert!(
        dsl_res["result"]["structuredContent"]["lint_report"]["ok"]
            .as_bool()
            .unwrap()
    );

    // 5. template_metadata + template_instantiate.
    let spec = json!({
        "spec_version": 1,
        "template_id": "mcp_montage",
        "name": "MCP Montage",
        "props_schema": [
            { "id": "clips", "label": "Clips", "kind": { "type": "video_clip_list", "min_items": 1, "max_items": 4 }, "required": true },
            { "id": "bgm", "label": "BGM", "kind": { "type": "audio_file" }, "required": true }
        ],
        "metadata_rules": {
            "default_fps": { "num": 24, "den": 1 },
            "aspect_presets": { "16:9": { "width": 160, "height": 90 } },
            "duration_policy": { "kind": "beat_count", "beats": 4 },
            "beat_source": "beat_grid",
            "cuts_per_beat": { "num": 1, "den": 1 },
            "min_segment_ms": 200
        }
    });
    let props = json!({
        "clips": ["synthetic://clip_a"],
        "bgm": "synthetic://beat"
    });
    let tmpl_meta: Value = serde_json::from_str(
        &server.handle_jsonrpc_str(
            &json!({
                "jsonrpc": "2.0",
                "id": 5,
                "method": "tools/call",
                "params": {
                    "name": "template_metadata",
                    "arguments": { "spec": spec, "props": props }
                }
            })
            .to_string(),
        ),
    )
    .unwrap();
    assert_eq!(
        tmpl_meta["result"]["structuredContent"]["template_id"],
        "mcp_montage"
    );

    let tmpl_out = tmp.0.join("tmpl_proj.json");
    let tmpl_inst: Value = serde_json::from_str(
        &server.handle_jsonrpc_str(
            &json!({
                "jsonrpc": "2.0",
                "id": 6,
                "method": "tools/call",
                "params": {
                    "name": "template_instantiate",
                    "arguments": {
                        "spec": spec,
                        "props": props,
                        "out_path": tmpl_out
                    }
                }
            })
            .to_string(),
        ),
    )
    .unwrap();
    assert!(
        tmpl_inst["result"]["structuredContent"]["lint_report"]["ok"]
            .as_bool()
            .unwrap()
    );
    assert!(tmpl_out.is_file());

    // 6. track_point + track_planar on active session.
    let pt_res: Value = serde_json::from_str(
        &server.handle_jsonrpc_str(
            &json!({
                "jsonrpc": "2.0",
                "id": 7,
                "method": "tools/call",
                "params": {
                    "name": "track_point",
                    "arguments": {
                        "asset_id": "asset_clip_a",
                        "target_node_id": "follow_2d",
                        "x": 0.5,
                        "y": 0.5,
                        "bind_mode": "transform2d"
                    }
                }
            })
            .to_string(),
        ),
    )
    .unwrap();
    assert_eq!(pt_res["result"]["structuredContent"]["frames_tracked"], 4);
    assert!(!server.project().keyframe_tracks.is_empty());

    let pl_res: Value = serde_json::from_str(
        &server.handle_jsonrpc_str(
            &json!({
                "jsonrpc": "2.0",
                "id": 8,
                "method": "tools/call",
                "params": {
                    "name": "track_planar",
                    "arguments": {
                        "source_node_id": "src_a",
                        "target_node_id": "corner_out",
                        "corners": [[0.2, 0.2], [0.8, 0.2], [0.8, 0.8], [0.2, 0.8]],
                        "bind_mode": "corner_pin"
                    }
                }
            })
            .to_string(),
        ),
    )
    .unwrap();
    assert_eq!(pl_res["result"]["structuredContent"]["frames_tracked"], 4);

    // 7. export_nle + import_nle (OTIO & FCPXML).
    let otio_out = tmp.0.join("out.otio");
    let exp_otio: Value = serde_json::from_str(
        &server.handle_jsonrpc_str(
            &json!({
                "jsonrpc": "2.0",
                "id": 9,
                "method": "tools/call",
                "params": {
                    "name": "export_nle",
                    "arguments": { "format": "otio", "out_path": otio_out }
                }
            })
            .to_string(),
        ),
    )
    .unwrap();
    assert!(otio_out.is_file());
    let otio_text = exp_otio["result"]["structuredContent"]["content"]
        .as_str()
        .unwrap()
        .to_owned();

    let imp_otio: Value = serde_json::from_str(
        &server.handle_jsonrpc_str(
            &json!({
                "jsonrpc": "2.0",
                "id": 10,
                "method": "tools/call",
                "params": {
                    "name": "import_nle",
                    "arguments": { "format": "otio", "input": otio_text }
                }
            })
            .to_string(),
        ),
    )
    .unwrap();
    assert_eq!(
        imp_otio["result"]["structuredContent"]["format"],
        "otio"
    );

    let exp_fcpxml: Value = serde_json::from_str(
        &server.handle_jsonrpc_str(
            &json!({
                "jsonrpc": "2.0",
                "id": 11,
                "method": "tools/call",
                "params": { "name": "export_nle", "arguments": { "format": "fcpxml" } }
            })
            .to_string(),
        ),
    )
    .unwrap();
    let fcpxml_text = exp_fcpxml["result"]["structuredContent"]["content"]
        .as_str()
        .unwrap()
        .to_owned();
    let imp_fcpxml: Value = serde_json::from_str(
        &server.handle_jsonrpc_str(
            &json!({
                "jsonrpc": "2.0",
                "id": 12,
                "method": "tools/call",
                "params": {
                    "name": "import_nle",
                    "arguments": { "format": "fcpxml", "input": fcpxml_text }
                }
            })
            .to_string(),
        ),
    )
    .unwrap();
    assert_eq!(
        imp_fcpxml["result"]["structuredContent"]["format"],
        "fcpxml"
    );

    // 8. ai_models + ai_run_manifest (synthetic_test mode with verified SHA-256).
    let models_root = tmp.0.join("models");
    let model_dir = models_root.join("demo_matte");
    std::fs::create_dir_all(&model_dir).unwrap();
    let onnx_path = model_dir.join("demo.onnx");
    let onnx_bytes = b"mcp-demo-onnx";
    std::fs::write(&onnx_path, onnx_bytes).unwrap();
    let sha = FfmpegBridge::hash_bytes(onnx_bytes).sha256;
    std::fs::write(
        model_dir.join("model_manifest.json"),
        serde_json::to_string_pretty(&json!({
            "schema_version": "frameshift.ai_model.v1",
            "id": "demo_matte",
            "task": "matte",
            "onnx_file": "demo.onnx",
            "sha256": sha,
            "license": "MIT"
        }))
        .unwrap(),
    )
    .unwrap();

    let ai_list: Value = serde_json::from_str(
        &server.handle_jsonrpc_str(
            &json!({
                "jsonrpc": "2.0",
                "id": 13,
                "method": "tools/call",
                "params": {
                    "name": "ai_models",
                    "arguments": { "models_dir": models_root }
                }
            })
            .to_string(),
        ),
    )
    .unwrap();
    assert_eq!(ai_list["result"]["structuredContent"]["count"], 1);
    assert_eq!(
        ai_list["result"]["structuredContent"]["models"][0]["manifest"]["id"],
        "demo_matte"
    );

    let ai_out_png = tmp.0.join("ai_out.png");
    let ai_run_res: Value = serde_json::from_str(
        &server.handle_jsonrpc_str(
            &json!({
                "jsonrpc": "2.0",
                "id": 14,
                "method": "tools/call",
                "params": {
                    "name": "ai_run_manifest",
                    "arguments": {
                        "model_id": "demo_matte",
                        "models_dir": models_root,
                        "input_path": png_a,
                        "out_path": ai_out_png,
                        "synthetic_test": true
                    }
                }
            })
            .to_string(),
        ),
    )
    .unwrap();
    assert_eq!(
        ai_run_res["result"]["structuredContent"]["report"]["model_id"],
        "demo_matte"
    );
    assert!(
        ai_run_res["result"]["structuredContent"]["report"]["sha256_verified"]
            .as_bool()
            .unwrap()
    );
    assert!(ai_out_png.is_file());
    let _ = BlendMode::Normal;
}
