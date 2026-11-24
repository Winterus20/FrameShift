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
