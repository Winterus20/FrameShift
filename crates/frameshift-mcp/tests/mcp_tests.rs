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
