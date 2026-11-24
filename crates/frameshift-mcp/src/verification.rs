use crate::error::McpError;
use crate::server::{McpServer, MutateProjectResult, PreferenceQueryResult, SchemaDescription};
use frameshift_core::InspectionPackManifest;
use frameshift_media::AudioAnalysisReport;
use frameshift_schema::{
    CreativeDecisionRecord, PreferenceEntry, PreferenceScope, Project, ProjectCommand,
};
use frameshift_time::{
    FLICKS_PER_SECOND, Flicks, ImpactCurveStyle, ImpactSolveRequest, ImpactSolver, SourceFitMode,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// Summary produced by [`run_m3_agent_director_verification`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct M3VerificationSummary {
    pub all_gates_passed: bool,
    pub tools_count: usize,
    pub initial_revision_id: u64,
    pub final_revision_id: u64,
    pub detected_beats_count: usize,
    pub estimated_bpm: f64,
    pub aligned_beat_tick: Flicks,
    pub creative_decisions_recorded: usize,
    pub scene_preference_value: Option<String>,
    pub global_preference_leaked: bool,
    pub stale_revision_rejected: bool,
    pub inspection_revision_id: u64,
    pub inspection_manifest_path: PathBuf,
    pub report_path: PathBuf,
}

fn call_jsonrpc(
    server: &mut McpServer,
    id: u64,
    method: &str,
    params: Value,
) -> Result<Value, McpError> {
    let req = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    });
    let resp_str = server.handle_jsonrpc_str(&req.to_string());
    let resp: Value = serde_json::from_str(&resp_str)?;
    if let Some(err) = resp.get("error") {
        let code = err.get("code").and_then(Value::as_i64).unwrap_or(-32000);
        let msg = err
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown JSON-RPC error");
        if code == -32009 {
            let expected = err
                .pointer("/data/expected_revision_id")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let actual = err
                .pointer("/data/actual_revision_id")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            return Err(McpError::RevisionConflict { expected, actual });
        }
        return Err(McpError::VerificationFailed(format!(
            "JSON-RPC error ({code}): {msg}"
        )));
    }
    resp.get("result")
        .cloned()
        .ok_or_else(|| McpError::VerificationFailed("missing JSON-RPC result field".into()))
}

fn call_tool<T: for<'de> Deserialize<'de>>(
    server: &mut McpServer,
    id: u64,
    tool_name: &str,
    arguments: Value,
) -> Result<T, McpError> {
    let result = call_jsonrpc(
        server,
        id,
        "tools/call",
        json!({
            "name": tool_name,
            "arguments": arguments,
        }),
    )?;
    let structured = result.get("structuredContent").cloned().ok_or_else(|| {
        McpError::VerificationFailed("missing structuredContent in tools/call".into())
    })?;
    Ok(serde_json::from_value(structured)?)
}

/// Simulates an autonomous Agent Director building and revising an edit strictly
/// through JSON-RPC 2.0 `McpServer` calls, verifying schema discovery, beat analysis,
/// impact alignment, creative decision recording, preference scope isolation, optimistic-lock
/// conflict rejection, and inspection pack generation.
pub fn run_m3_agent_director_verification(
    out_dir: &Path,
) -> Result<M3VerificationSummary, McpError> {
    std::fs::create_dir_all(out_dir)?;

    let mut project = Project::golden_scene_preset();
    // Use 960x540 for fast deterministic M3 verification while keeping crop_fullres at 640x360.
    project.render_profiles.final_profile.resolution = frameshift_schema::Resolution {
        width: 960,
        height: 540,
    };
    project
        .render_profiles
        .final_profile
        .motion_blur_sub_samples = 4;

    let mut server = McpServer::new(project)?;
    let mut rpc_id = 1u64;

    // 1. Tool discovery via `tools/list`.
    let tools_val = call_jsonrpc(&mut server, rpc_id, "tools/list", json!({}))?;
    rpc_id += 1;
    let tools_arr = tools_val
        .get("tools")
        .and_then(Value::as_array)
        .ok_or_else(|| McpError::VerificationFailed("tools/list missing tools array".into()))?;
    let tools_count = tools_arr.len();
    let required_tools = [
        "describe_schema",
        "analyze_audio",
        "inspect_project",
        "mutate_project",
        "undo",
        "redo",
        "preflight_project",
        "plan_autocut",
        "list_effect_presets",
        "apply_effect_preset",
        "query_preferences",
        "render_project",
    ];
    for req_tool in required_tools {
        if !tools_arr
            .iter()
            .any(|t| t.get("name").and_then(Value::as_str) == Some(req_tool))
        {
            return Err(McpError::VerificationFailed(format!(
                "tools/list missing required tool '{req_tool}'"
            )));
        }
    }

    // 2. Schema discovery via `describe_schema`.
    let schema_desc: SchemaDescription =
        call_tool(&mut server, rpc_id, "describe_schema", json!({}))?;
    rpc_id += 1;
    let initial_revision_id = schema_desc.revision_id;
    let mut rev = initial_revision_id;

    // 3. Audio beat analysis via `analyze_audio`.
    let audio_report: AudioAnalysisReport = call_tool(
        &mut server,
        rpc_id,
        "analyze_audio",
        json!({ "window_ticks": FLICKS_PER_SECOND / 100 }),
    )?;
    rpc_id += 1;
    if audio_report.detected_beats.is_empty() {
        return Err(McpError::VerificationFailed(
            "analyze_audio returned zero detected beats".into(),
        ));
    }

    // 4. Align `drop_impact` & `clip_a` speed curve to the detected beat nearest 2.5s.
    let target_2_5s = Flicks(2 * FLICKS_PER_SECOND + FLICKS_PER_SECOND / 2);
    let aligned_beat_tick = *audio_report
        .detected_beats
        .iter()
        .min_by_key(|b| (b.0 - target_2_5s.0).abs())
        .ok_or_else(|| McpError::VerificationFailed("missing beat".into()))?;

    let clip_a = server
        .project()
        .clip("clip_a")
        .ok_or_else(|| McpError::VerificationFailed("missing clip_a".into()))?
        .clone();
    let event = server
        .project()
        .event("drop_impact")
        .ok_or_else(|| McpError::VerificationFailed("missing drop_impact".into()))?
        .clone();
    let source_impact = event
        .source_anchor_tick
        .unwrap_or(Flicks(2 * FLICKS_PER_SECOND));

    let solve = ImpactSolver::solve(&ImpactSolveRequest {
        source_bounds_start: clip_a.source_start,
        source_bounds_end: clip_a.source_start + Flicks(3 * FLICKS_PER_SECOND),
        source_impact,
        output_start: clip_a.timeline_start,
        output_end: clip_a.timeline_end,
        target_output: aligned_beat_tick,
        min_speed: 0.25,
        max_speed: 4.0,
        style: ImpactCurveStyle::SmoothRamp,
        fit_mode: SourceFitMode::ExactRange,
    })
    .map_err(|v| McpError::VerificationFailed(format!("ImpactSolver failed: {v}")))?;

    let m1: MutateProjectResult = call_tool(
        &mut server,
        rpc_id,
        "mutate_project",
        json!({
            "expected_revision_id": rev,
            "command": ProjectCommand::MoveEventTarget {
                event_id: "drop_impact".into(),
                new_target_output_tick: aligned_beat_tick,
            }
        }),
    )?;
    rpc_id += 1;
    rev = m1.revision_id;

    let m2: MutateProjectResult = call_tool(
        &mut server,
        rpc_id,
        "mutate_project",
        json!({
            "expected_revision_id": rev,
            "command": ProjectCommand::SetClipSpeedCurve {
                clip_id: "clip_a".into(),
                speed_curve: solve.speed_curve,
            }
        }),
    )?;
    rpc_id += 1;
    rev = m2.revision_id;

    // 5. Record a structured CreativeDecisionRecord.
    let m3: MutateProjectResult = call_tool(
        &mut server,
        rpc_id,
        "mutate_project",
        json!({
            "expected_revision_id": rev,
            "command": ProjectCommand::RecordCreativeDecision(CreativeDecisionRecord {
                decision_id: "dec_m3_agent_beat_lock".into(),
                approach: format!(
                    "Locked drop_impact to detected audio onset tick {} ({:.3}s) via ImpactSolver SmoothRamp",
                    aligned_beat_tick.0,
                    aligned_beat_tick.to_seconds_f64()
                ),
                rationale: "Ensures visual impact flash and speed-ramp inflection coincide with detected transient peak".into(),
                evidence_ticks: vec![aligned_beat_tick],
                protected_invariants: vec![
                    "clip_a bounds [0s, 3s) preserved".into(),
                    "Scene preference isolated from Global".into(),
                ],
                risks_to_verify: vec![
                    "Inspect crop_fullres.png and contact_sheet.png for title readability".into(),
                ],
            })
        }),
    )?;
    rpc_id += 1;
    rev = m3.revision_id;

    // 6. Set a Scene-scoped preference and verify it does NOT leak into Global.
    let m4: MutateProjectResult = call_tool(
        &mut server,
        rpc_id,
        "mutate_project",
        json!({
            "expected_revision_id": rev,
            "command": ProjectCommand::SetPreference(PreferenceEntry {
                key: "camera_shake_intensity".into(),
                value: "0.85_drop_shake".into(),
                scope: PreferenceScope::Scene,
                target_id: Some("scene_drop".into()),
            })
        }),
    )?;
    rpc_id += 1;
    rev = m4.revision_id;

    let pref_scene: PreferenceQueryResult = call_tool(
        &mut server,
        rpc_id,
        "query_preferences",
        json!({ "key": "camera_shake_intensity", "scene_id": "scene_drop" }),
    )?;
    rpc_id += 1;

    let pref_global: PreferenceQueryResult = call_tool(
        &mut server,
        rpc_id,
        "query_preferences",
        json!({ "key": "camera_shake_intensity" }),
    )?;
    rpc_id += 1;

    let scene_preference_value = pref_scene.effective.as_ref().map(|e| e.value.clone());
    let global_preference_leaked =
        pref_global.effective.is_some() || pref_global.global_layer.is_some();

    // 7. Optimistic-locking negative check: stale revision must be rejected cleanly.
    let stale_res: Result<MutateProjectResult, McpError> = call_tool(
        &mut server,
        rpc_id,
        "mutate_project",
        json!({
            "expected_revision_id": initial_revision_id,
            "command": ProjectCommand::SetProjectName {
                name: "SHOULD_NOT_COMMIT".into(),
            }
        }),
    );
    rpc_id += 1;
    let stale_revision_rejected = matches!(stale_res, Err(McpError::RevisionConflict { .. }));

    // 8. Export multi-surface inspection pack via `inspect_project`.
    let inspect_dir = out_dir.join("inspection_pack");
    let manifest: InspectionPackManifest = call_tool(
        &mut server,
        rpc_id,
        "inspect_project",
        json!({
            "out_dir": inspect_dir.to_string_lossy(),
            "expected_revision_id": rev,
        }),
    )?;

    let inspection_files_ok = manifest.contact_sheet_path.is_file()
        && manifest.mask_pack_path.is_file()
        && manifest.transition_pack_path.is_file()
        && manifest.velocity_pack_path.is_file()
        && manifest.crop_fullres_path.is_file()
        && manifest.manifest_path.is_file();

    let all_gates_passed = tools_count >= 6
        && rev > initial_revision_id
        && !audio_report.detected_beats.is_empty()
        && audio_report.estimated_bpm > 0.0
        && m3.creative_decisions_count >= 2
        && scene_preference_value.as_deref() == Some("0.85_drop_shake")
        && pref_scene.resolved_scope == Some(PreferenceScope::Scene)
        && !global_preference_leaked
        && stale_revision_rejected
        && manifest.revision_id == rev
        && inspection_files_ok;

    let report_path = out_dir.join("m3_verification_report.json");
    let summary = M3VerificationSummary {
        all_gates_passed,
        tools_count,
        initial_revision_id,
        final_revision_id: rev,
        detected_beats_count: audio_report.detected_beats.len(),
        estimated_bpm: audio_report.estimated_bpm,
        aligned_beat_tick,
        creative_decisions_recorded: m3.creative_decisions_count,
        scene_preference_value,
        global_preference_leaked,
        stale_revision_rejected,
        inspection_revision_id: manifest.revision_id,
        inspection_manifest_path: manifest.manifest_path,
        report_path: report_path.clone(),
    };

    std::fs::write(&report_path, serde_json::to_string_pretty(&summary)?)?;
    Ok(summary)
}
