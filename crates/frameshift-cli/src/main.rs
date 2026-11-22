use anyhow::{Context, Result, bail};
use frameshift_app::{
    AppError, BackgroundJobSupervisor, JobKind, JobState, WindowClosePolicy, WorkspaceUiController,
};
use frameshift_core::{
    Engine, Flicks, M1VerificationReport, MediaResolver, OutputAlphaMode, Phase5VerificationReport,
    Project, ProjectCommand, RenderOptions, RenderProfileKind, RenderRange,
};
use frameshift_mcp::{M3VerificationSummary, McpServer, run_m3_agent_director_verification};
use frameshift_media::{
    AudioAnalysisReport, AudioBuffer, AudioPeakInfo, AudioRetimeMode, DEFAULT_AUDIO_SAMPLE_RATE,
    SyntheticMediaGenerator,
};
use frameshift_schema::{EventAnchorDef, EventOrigin, NodeKind, ProjectSession};
use frameshift_time::{
    ExprContext, FLICKS_PER_MILLISECOND, FLICKS_PER_SECOND, Rational32, SafeExpr, SpeedCurve,
};
use progress::{ProgressMode, ProgressReporter};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

mod progress;
mod workbench;

/// Human time token. Resolved ticks stay `i64` Flicks; `f:<index>` needs the project fps.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TimeSpec {
    Ticks(Flicks),
    FrameIndex(i64),
}

/// Requested end-exclusive render window, resolved against the project frame grid later.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RangeSpec {
    start: TimeSpec,
    end: TimeSpec,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CliCommand {
    InitGolden {
        out: PathBuf,
    },
    Render {
        project_path: PathBuf,
        out: PathBuf,
        profile: RenderProfileKind,
        range: Option<RangeSpec>,
        progress: ProgressMode,
    },
    Frame {
        project_path: PathBuf,
        time: TimeSpec,
        out: PathBuf,
        profile: RenderProfileKind,
    },
    VerifyM1 {
        out_dir: PathBuf,
    },
    Inspect {
        project_path: PathBuf,
        out_dir: PathBuf,
    },
    Materials {
        project_path: PathBuf,
        out_dir: PathBuf,
    },
    UiSnapshot {
        project_path: PathBuf,
        out: PathBuf,
        time: Option<TimeSpec>,
    },
    Peaks {
        project_path: PathBuf,
        asset_id: Option<String>,
        window: TimeSpec,
        at: Option<TimeSpec>,
        half_window: TimeSpec,
        write_events: bool,
    },
    Autocut {
        project_path: PathBuf,
        asset_id: Option<String>,
        window: TimeSpec,
        range: Option<RangeSpec>,
        at: Option<TimeSpec>,
        half_window: TimeSpec,
        min_segment: Option<TimeSpec>,
        no_start: bool,
        no_end: bool,
        write_events: bool,
    },
    McpStdio,
    VerifyAll {
        out_dir: PathBuf,
    },
    Help,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct M2VerificationSummary {
    all_gates_passed: bool,
    safe_expr_evaluated_value: f64,
    vector_text_non_zero_pixels: usize,
    lut3d_node_present: bool,
    wsola_pitch_preserving_frames: usize,
    audio_detected_beats_count: usize,
    audio_estimated_bpm: f64,
    inspection_revision_id: u64,
    contact_sheet_path: PathBuf,
    mask_pack_path: PathBuf,
    transition_pack_path: PathBuf,
    velocity_pack_path: PathBuf,
    crop_fullres_path: PathBuf,
    manifest_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct M4VerificationSummary {
    all_gates_passed: bool,
    concurrency_conflict_rejected: bool,
    undo_redo_verified: bool,
    window_close_detach_verified: bool,
    window_close_cancel_verified: bool,
    snapshot_width: u32,
    snapshot_height: u32,
    snapshot_hash64: u64,
    desktop_ui_snapshot_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct FullVerificationReport {
    all_gates_passed: bool,
    out_dir: PathBuf,
    m1: M1VerificationReport,
    m2: M2VerificationSummary,
    m3: M3VerificationSummary,
    m4: M4VerificationSummary,
    m5: Phase5VerificationReport,
}

impl FullVerificationReport {
    fn to_markdown(&self) -> String {
        let mut md = String::new();
        let _ = writeln!(md, "# FrameShift Full Phase 1-5 Verification Report\n");
        let _ = writeln!(
            md,
            "- **Overall Status:** {}",
            if self.all_gates_passed {
                "PASS"
            } else {
                "FAIL"
            }
        );
        let _ = writeln!(md, "- **Output Directory:** `{}`\n", self.out_dir.display());

        let _ = writeln!(md, "## Phase Summary\n");
        let _ = writeln!(md, "| Phase | Scope | Status | Key Evidence |");
        let _ = writeln!(md, "| :--- | :--- | :---: | :--- |");
        let _ = writeln!(
            md,
            "| M1 | Golden Vertical Slice (Determinism, Seek, Motion Blur, Audio Sync, MP4) | {} | {} frames, delta={} ticks |",
            pass_str(self.m1.all_gates_passed),
            self.m1.final_summary.frame_count,
            self.m1.audio_impact_sync.delta_ticks
        );
        let _ = writeln!(
            md,
            "| M2 | Multi-Surface Inspection Pack, SafeExpr, VectorText, Lut3D, WSOLA | {} | BPM={:.1}, beats={}, SafeExpr={:.3} |",
            pass_str(self.m2.all_gates_passed),
            self.m2.audio_estimated_bpm,
            self.m2.audio_detected_beats_count,
            self.m2.safe_expr_evaluated_value
        );
        let _ = writeln!(
            md,
            "| M3 | JSON-RPC 2.0 MCP Server & Agent Director | {} | {} tools, rev {}->{}, global_leak={} |",
            pass_str(self.m3.all_gates_passed),
            self.m3.tools_count,
            self.m3.initial_revision_id,
            self.m3.final_revision_id,
            self.m3.global_preference_leaked
        );
        let _ = writeln!(
            md,
            "| M4 | Desktop UI Controller, Concurrency Guard, Job Supervisor, UI Snapshot | {} | {}x{} snapshot, conflict_rejected={} |",
            pass_str(self.m4.all_gates_passed),
            self.m4.snapshot_width,
            self.m4.snapshot_height,
            self.m4.concurrency_conflict_rejected
        );
        let _ = writeln!(
            md,
            "| M5 | Proxy 1/2 PtsIndex Equivalence & HW Acceleration Policy | {} | proxy={}x{}, L1_diff={:.2}, auto_backend={} |\n",
            pass_str(self.m5.all_gates_passed),
            self.m5.proxy_width,
            self.m5.proxy_height,
            self.m5.draft_vs_final_mean_l1_diff,
            self.m5.auto_policy_backend
        );

        let _ = writeln!(md, "## Primary Artifacts\n");
        let _ = writeln!(
            md,
            "- Contact Sheet: `{}`",
            self.m2.contact_sheet_path.display()
        );
        let _ = writeln!(md, "- Mask Pack: `{}`", self.m2.mask_pack_path.display());
        let _ = writeln!(
            md,
            "- Transition Pack: `{}`",
            self.m2.transition_pack_path.display()
        );
        let _ = writeln!(
            md,
            "- Velocity Pack: `{}`",
            self.m2.velocity_pack_path.display()
        );
        let _ = writeln!(
            md,
            "- Full-Res 1:1 Crop: `{}`",
            self.m2.crop_fullres_path.display()
        );
        let _ = writeln!(
            md,
            "- Inspection Manifest: `{}`",
            self.m2.manifest_path.display()
        );
        let _ = writeln!(
            md,
            "- Desktop UI Snapshot: `{}`",
            self.m4.desktop_ui_snapshot_path.display()
        );
        let _ = writeln!(md, "- Proxy MP4: `{}`", self.m5.proxy_mp4_path.display());
        let _ = writeln!(
            md,
            "- M1 Final MP4: `{}`",
            self.m1.golden_final_mp4.display()
        );
        let _ = writeln!(md, "- M3 Report: `{}`", self.m3.report_path.display());
        md
    }
}

fn pass_str(ok: bool) -> &'static str {
    if ok { "PASS" } else { "FAIL" }
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(result) = workbench::dispatch(&args) {
        return result;
    }
    let cmd = parse_args(&args)?;

    match cmd {
        CliCommand::Help => {
            print_usage();
            Ok(())
        }
        CliCommand::InitGolden { out } => {
            let project = Project::golden_scene_preset();
            if let Some(parent) = out.parent()
                && !parent.as_os_str().is_empty()
            {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&out, project.to_json_pretty()?)
                .with_context(|| format!("failed to write '{}'", out.display()))?;
            println!("Wrote Golden Scene preset to {}", out.display());
            Ok(())
        }
        CliCommand::Render {
            project_path,
            out,
            profile,
            range,
            progress: progress_mode,
        } => {
            let project = load_project(&project_path)?;
            let fps = project.timeline.fps;
            let render_range = match range {
                Some(spec) => {
                    let start = resolve_time_spec(spec.start, fps)?;
                    let end = resolve_time_spec(spec.end, fps)?;
                    Some(RenderRange::resolve(&project, start, end).with_context(|| {
                        format!("invalid --range for '{}'", project_path.display())
                    })?)
                }
                None => None,
            };
            let options = RenderOptions {
                output_alpha_mode: OutputAlphaMode::Straight,
                ..Default::default()
            };
            let mut engine = Engine::new_headless()?;
            let mut reporter = ProgressReporter::stderr(progress_mode);
            let result = engine.render_video_mp4_with_progress(
                &project,
                profile,
                &out,
                render_range,
                &options,
                &mut |event| reporter.on_progress(event),
            );
            reporter.finish();
            let summary = result?;
            println!(
                "{}",
                format_render_success(
                    summary.width,
                    summary.height,
                    summary.frame_count,
                    summary.fps,
                    summary.has_audio,
                    summary.profile,
                    &summary.out_path,
                    &format_effective_blur_samples(&project, None),
                    summary.range.as_ref(),
                )
            );
            Ok(())
        }
        CliCommand::Frame {
            project_path,
            time,
            out,
            profile,
        } => {
            let project = load_project(&project_path)?;
            let tick = resolve_time_spec(time, project.timeline.fps)?;
            let mut engine = Engine::new_headless()?;
            let frame = engine.render_frame_at(&project, profile, tick)?;
            if let Some(parent) = out.parent()
                && !parent.as_os_str().is_empty()
            {
                std::fs::create_dir_all(parent)?;
            }
            frame.save_png(&out)?;
            println!(
                "{}",
                format_frame_success(
                    tick,
                    profile,
                    frame.width,
                    frame.height,
                    frame.fnv1a_hash64(),
                    &out,
                    &format_effective_blur_samples(&project, None),
                )
            );
            Ok(())
        }
        CliCommand::VerifyM1 { out_dir } => {
            let mut engine = Engine::new_headless()?;
            let report = engine.verify_m1_golden_scene(&out_dir)?;
            println!("{}", report.to_markdown());
            if !report.all_gates_passed {
                bail!(
                    "M1 verification failed; inspect {}",
                    out_dir.join("m1_verification_report.json").display()
                );
            }
            Ok(())
        }
        CliCommand::Inspect {
            project_path,
            out_dir,
        } => {
            let project = load_project(&project_path)?;
            let mut engine = Engine::new_headless()?;
            let manifest = engine.export_inspection_pack(&project, &out_dir)?;
            println!(
                "Exported inspection pack for revision #{} -> {}",
                manifest.revision_id,
                manifest.manifest_path.display()
            );
            Ok(())
        }
        CliCommand::UiSnapshot {
            project_path,
            out,
            time,
        } => {
            let project = load_project(&project_path)?;
            let target_tick = if let Some(spec) = time {
                resolve_time_spec(spec, project.timeline.fps)?
            } else {
                project
                    .event("drop_impact")
                    .or_else(|| project.events.first())
                    .map(|e| e.target_output_tick)
                    .unwrap_or(Flicks(project.timeline.duration_ticks.0 / 2))
            };
            let mut engine = Engine::new_headless()?;
            let viewport =
                engine.render_frame_at(&project, RenderProfileKind::Draft, target_tick)?;
            let mut controller = WorkspaceUiController::from_project(project)?;
            controller.scrub_to(target_tick);
            let snapshot = controller.render_desktop_ui_snapshot(&viewport, 1920, 1080);
            if let Some(parent) = out.parent()
                && !parent.as_os_str().is_empty()
            {
                std::fs::create_dir_all(parent)?;
            }
            snapshot.save_png(&out)?;
            println!(
                "Saved desktop UI snapshot ({}x{}, tick={}) -> {}",
                snapshot.width,
                snapshot.height,
                target_tick,
                out.display()
            );
            Ok(())
        }
        CliCommand::Peaks {
            project_path,
            asset_id,
            window,
            at,
            half_window,
            write_events,
        } => {
            let project = load_project(&project_path)?;
            let fps = project.timeline.fps;
            let window_ticks = resolve_time_spec(window, fps)?;
            if window_ticks.0 <= 0 {
                bail!("analysis window must be a positive tick count");
            }
            let audio = load_analysis_audio(&project, asset_id.as_deref())?;
            let report = audio.analyze_beats_and_energy(window_ticks)?;
            let cue = if let Some(at_spec) = at {
                let center = resolve_time_spec(at_spec, fps)?;
                let half = resolve_time_spec(half_window, fps)?;
                if half.0 < 0 {
                    bail!("cue half-window must be >= 0");
                }
                Some(audio.detect_peak_in_window(center, half)?)
            } else {
                None
            };
            if let Some(info) = &cue {
                println!("{}", format_cue_peak_stamp(info));
            }
            print!("{}", format_beat_report(&report));
            if write_events {
                let cue_tick = cue.map(|info| info.peak_tick);
                let written = write_detected_events(
                    &project,
                    &project_path,
                    &report.detected_beats,
                    cue_tick,
                )?;
                println!(
                    "Wrote {} detected event(s) -> {}",
                    written,
                    project_path.display()
                );
            }
            Ok(())
        }
        CliCommand::McpStdio => {
            let mut server = McpServer::new(Project::golden_scene_preset())?;
            server.run_stdio()?;
            Ok(())
        }
        CliCommand::VerifyAll { out_dir } => run_verify_all(&out_dir),
        CliCommand::Materials {
            project_path: _project_path,
            out_dir,
        } => {
            let mut engine = Engine::new_headless()?;
            let manifest = engine.export_material_gallery(&out_dir)?;
            println!(
                "Exported material gallery -> {} ({}x{}, {} cells)",
                manifest.sheet.file,
                manifest.sheet.width,
                manifest.sheet.height,
                manifest.cells.len()
            );
            println!(
                "  manifest: {}",
                out_dir.join(&manifest.sheet.file).display()
            );
            Ok(())
        }
        CliCommand::Autocut {
            project_path,
            asset_id,
            window,
            range,
            at,
            half_window,
            min_segment,
            no_start,
            no_end,
            write_events,
        } => {
            let project = load_project(&project_path)?;
            let fps = project.timeline.fps;
            let resolved_range = match range {
                Some(spec) => {
                    let start = resolve_time_spec(spec.start, fps)?;
                    let end = resolve_time_spec(spec.end, fps)?;
                    Some(RenderRange::resolve(&project, start, end).with_context(|| {
                        format!("invalid autocut --range for '{}'", project_path.display())
                    })?)
                }
                None => None,
            };
            let window_ticks = resolve_time_spec(window, fps)?;
            if window_ticks.0 <= 0 {
                bail!("analysis window must be a positive tick count");
            }
            let audio = load_analysis_audio(&project, asset_id.as_deref())?;
            let report = audio.analyze_beats_and_energy(window_ticks)?;
            let coordinate = asset_id
                .as_deref()
                .map_or_else(|| "project".to_owned(), |id| format!("source:{id}"));
            let cue = if let Some(at_spec) = at {
                let center = resolve_time_spec(at_spec, fps)?;
                let half = resolve_time_spec(half_window, fps)?;
                if half.0 < 0 {
                    bail!("cue half-window must be >= 0");
                }
                Some(audio.detect_peak_in_window(center, half)?)
            } else {
                None
            };
            if let Some(info) = &cue {
                println!("{}", format_cue_peak_stamp(info));
            }
            let min_seg = min_segment
                .map(|s| resolve_time_spec(s, fps))
                .transpose()?
                .unwrap_or(Flicks::ZERO);
            let params = frameshift_core::AutocutParams {
                start_tick: resolved_range
                    .as_ref()
                    .map_or(Flicks::ZERO, |range| range.start_tick),
                duration_ticks: Flicks(
                    resolved_range
                        .as_ref()
                        .map_or(project.timeline.duration_ticks.0, |range| {
                            range.end_tick.0 - range.start_tick.0
                        }),
                ),
                min_segment_ticks: min_seg,
                include_start: !no_start,
                include_end: !no_end,
            };
            let cuts = frameshift_core::plan_autocut(&report, params)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            print!(
                "{}",
                format_autocut_report(
                    &cuts,
                    &report,
                    &params,
                    fps,
                    resolved_range.as_ref(),
                    &coordinate,
                )?
            );
            if write_events {
                let written = write_autocut_events(&project, &project_path, &cuts)?;
                println!(
                    "Wrote {} autocut event(s) -> {}",
                    written,
                    project_path.display()
                );
            }
            Ok(())
        }
    }
}

fn run_verify_all(out_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(out_dir)?;
    let mut engine = Engine::new_headless()?;

    // Phase 1 (M1)
    let m1 = engine.verify_m1_golden_scene(&out_dir.join("m1"))?;

    // Phase 2 (M2): Inspection Pack + SafeExpr + VectorText + Lut3D + WSOLA + Beat Analysis
    let project = Project::golden_scene_preset();
    let manifest = engine.export_inspection_pack(&project, out_dir)?;

    let binding = project
        .param_bindings
        .first()
        .context("golden preset missing ParamBindingDef")?;
    let safe_expr = SafeExpr::parse(&binding.expression)?;
    let expr_val = safe_expr.eval_default(&ExprContext {
        t_sec: 2.5,
        output_tick: 2 * FLICKS_PER_SECOND + FLICKS_PER_SECOND / 2,
        beat_phase: 0.0,
        audio_rms: 0.75,
        event_delta_sec: 0.0,
        progress: 0.5,
    })?;

    let vtext_node = project
        .graph
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::VectorText { .. }))
        .context("golden preset missing VectorText node")?;
    let vtext_frame =
        MediaResolver::render_vector_text_node(&vtext_node.kind, 640, 360, Flicks::ZERO)?;
    let vector_text_non_zero_pixels = vtext_frame.data.iter().filter(|px| px[3] > 0.01).count();

    let lut3d_node_present = project
        .graph
        .nodes
        .iter()
        .any(|n| matches!(n.kind, NodeKind::Lut3D { .. }));

    let (beat_buf, _) = SyntheticMediaGenerator::generate_beat_audio(
        Flicks(2 * FLICKS_PER_SECOND),
        120,
        DEFAULT_AUDIO_SAMPLE_RATE,
    );
    let curve = SpeedCurve::constant(1.5)?;
    let wsola = MediaResolver::retime_audio_buffer(
        &beat_buf,
        &curve,
        Flicks(2 * FLICKS_PER_SECOND),
        Flicks::ZERO,
        AudioRetimeMode::PitchPreserving,
    )?;

    let (beats_count, est_bpm) = match &manifest.audio_analysis {
        Some(a) => (a.detected_beats.len(), a.estimated_bpm),
        None => (0, 0.0),
    };

    let m2_passed = manifest.contact_sheet_path.is_file()
        && manifest.mask_pack_path.is_file()
        && manifest.transition_pack_path.is_file()
        && manifest.velocity_pack_path.is_file()
        && manifest.crop_fullres_path.is_file()
        && manifest.manifest_path.is_file()
        && expr_val.is_finite()
        && vector_text_non_zero_pixels > 0
        && lut3d_node_present
        && wsola.frame_count() > 0
        && beats_count > 0;

    let m2 = M2VerificationSummary {
        all_gates_passed: m2_passed,
        safe_expr_evaluated_value: expr_val,
        vector_text_non_zero_pixels,
        lut3d_node_present,
        wsola_pitch_preserving_frames: wsola.frame_count(),
        audio_detected_beats_count: beats_count,
        audio_estimated_bpm: est_bpm,
        inspection_revision_id: manifest.revision_id,
        contact_sheet_path: manifest.contact_sheet_path,
        mask_pack_path: manifest.mask_pack_path,
        transition_pack_path: manifest.transition_pack_path,
        velocity_pack_path: manifest.velocity_pack_path,
        crop_fullres_path: manifest.crop_fullres_path,
        manifest_path: manifest.manifest_path,
    };

    // Phase 3 (M3): MCP Server & Agent Director
    let m3 = run_m3_agent_director_verification(&out_dir.join("m3"))?;

    // Phase 4 (M4): WorkspaceUiController concurrency guard + BackgroundJobSupervisor + UI snapshot
    let mut controller = WorkspaceUiController::from_project(project.clone())?;
    let initial_rev = controller.revision_id();
    let impact_tick = project
        .event("drop_impact")
        .map(|e| e.target_output_tick)
        .unwrap_or(Flicks(2 * FLICKS_PER_SECOND + FLICKS_PER_SECOND / 2));
    controller.scrub_to(impact_tick);

    let human_rev = controller.human_move_event_anchor(
        "drop_impact",
        Flicks(impact_tick.0 + FLICKS_PER_SECOND / 60),
    )?;
    let conflict_err = controller.apply_agent_command(
        initial_rev,
        "agent_director",
        ProjectCommand::SetProjectName {
            name: "STALE_AGENT_WRITE".into(),
        },
    );
    let concurrency_conflict_rejected = matches!(
        conflict_err,
        Err(AppError::ConcurrencyConflict { expected, actual })
            if expected == initial_rev && actual == human_rev
    );
    let undo_rev = controller.undo()?;
    let redo_rev = controller.redo()?;
    let undo_redo_verified = undo_rev > human_rev && redo_rev > undo_rev;
    // Undo back to original impact timing for clean visual snapshot.
    let _ = controller.undo()?;

    let mut supervisor = BackgroundJobSupervisor::new();
    let j1 = supervisor.enqueue(
        JobKind::FinalExportRender,
        "Final MP4 Export",
        controller.revision_id(),
    );
    let j2 = supervisor.enqueue(
        JobKind::AgentDirectorTask,
        "Agent Beat Sync",
        controller.revision_id(),
    );
    supervisor.start(j1)?;
    supervisor.update_progress(j1, 45)?;
    let close_detach = supervisor.on_window_close(WindowClosePolicy::DetachAndContinueBackground);
    let window_close_detach_verified = close_detach.detached_jobs.len() == 2
        && supervisor.job(j1).is_some_and(|j| {
            j.detached_to_background && j.state == JobState::Running { progress_pct: 45 }
        })
        && supervisor.job(j2).is_some_and(|j| j.detached_to_background);

    let mut cancel_sup = BackgroundJobSupervisor::new();
    let jc = cancel_sup.enqueue(JobKind::DraftPreviewRender, "Draft Preview", 1);
    cancel_sup.start(jc)?;
    let close_cancel = cancel_sup.on_window_close(WindowClosePolicy::CancelAll);
    let window_close_cancel_verified = close_cancel.cancelled_jobs == vec![jc]
        && cancel_sup
            .job(jc)
            .is_some_and(|j| j.state == JobState::Cancelled);

    let viewport = engine.render_frame_at(
        controller.project(),
        RenderProfileKind::Draft,
        controller.playhead_tick,
    )?;
    let ui_snapshot = controller.render_desktop_ui_snapshot(&viewport, 1920, 1080);
    let desktop_ui_snapshot_path = out_dir.join("desktop_ui_snapshot.png");
    ui_snapshot.save_png(&desktop_ui_snapshot_path)?;

    let m4_passed = concurrency_conflict_rejected
        && undo_redo_verified
        && window_close_detach_verified
        && window_close_cancel_verified
        && ui_snapshot.width == 1920
        && ui_snapshot.height == 1080
        && desktop_ui_snapshot_path.is_file();

    let m4 = M4VerificationSummary {
        all_gates_passed: m4_passed,
        concurrency_conflict_rejected,
        undo_redo_verified,
        window_close_detach_verified,
        window_close_cancel_verified,
        snapshot_width: ui_snapshot.width,
        snapshot_height: ui_snapshot.height,
        snapshot_hash64: ui_snapshot.fnv1a_hash64(),
        desktop_ui_snapshot_path,
    };

    // Phase 5 (M5): Proxy Semantic Equivalence + HwAccel
    let m5 = engine.verify_phase5_proxy_and_hw(&out_dir.join("m5"))?;

    let all_gates_passed = m1.all_gates_passed
        && m2.all_gates_passed
        && m3.all_gates_passed
        && m4.all_gates_passed
        && m5.all_gates_passed;

    let report = FullVerificationReport {
        all_gates_passed,
        out_dir: out_dir.to_path_buf(),
        m1,
        m2,
        m3,
        m4,
        m5,
    };

    let json_path = out_dir.join("full_verification_report.json");
    let md_path = out_dir.join("full_verification_report.md");
    std::fs::write(&json_path, serde_json::to_string_pretty(&report)?)?;
    let md = report.to_markdown();
    std::fs::write(&md_path, &md)?;
    println!("{md}");

    if !report.all_gates_passed {
        bail!("verify-all failed; inspect {}", json_path.display());
    }

    Ok(())
}

fn load_project(path: &Path) -> Result<Project> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read project JSON '{}'", path.display()))?;
    let mut project = Project::from_json_str(&raw)
        .with_context(|| format!("failed to validate project JSON '{}'", path.display()))?;
    if let Some(project_dir) = path.parent() {
        for asset in &mut project.assets {
            let path = Path::new(&asset.path);
            if path.is_relative()
                && !asset.path.starts_with("synthetic://")
                && let Ok(resolved) = project_dir.join(path).canonicalize()
                && resolved.is_file()
            {
                asset.path = resolved.to_string_lossy().into_owned();
            }
        }
    }
    Ok(project)
}

fn profile_label(profile: RenderProfileKind) -> &'static str {
    match profile {
        RenderProfileKind::Draft => "draft",
        RenderProfileKind::Final => "final",
    }
}

/// `Rational32`'s `Display` already includes `fps`. Callers must not append it.
fn format_effective_blur_samples(project: &Project, ceiling: Option<u32>) -> String {
    project
        .graph
        .nodes
        .iter()
        .filter(|node| node.disabled_passthrough().is_none())
        .filter_map(|node| {
            let NodeKind::MotionBlurShutter { sub_samples, .. } = node.kind else {
                return None;
            };
            let effective = match ceiling {
                Some(limit) => sub_samples.min(limit),
                None => sub_samples,
            };
            Some(format!(
                "{}: node {sub_samples}, effective {effective}",
                node.id
            ))
        })
        .collect::<Vec<_>>()
        .join("; ")
}

#[allow(clippy::too_many_arguments)]
fn format_render_success(
    width: u32,
    height: u32,
    frame_count: usize,
    fps: Rational32,
    has_audio: bool,
    profile: RenderProfileKind,
    out: &Path,
    blur_samples: &str,
    range: Option<&RenderRange>,
) -> String {
    let range = match range {
        Some(range) => format!(", range f:{}..f:{}", range.start_frame, range.end_frame),
        None => String::new(),
    };
    let blur = if blur_samples.is_empty() {
        String::new()
    } else {
        format!(", {blur_samples}")
    };
    format!(
        "Rendered {}x{} (profile {}, {} frames @ {}, audio={}{}{}) -> {}",
        width,
        height,
        profile_label(profile),
        frame_count,
        fps,
        has_audio,
        range,
        blur,
        out.display()
    )
}

fn format_frame_success(
    tick: Flicks,
    profile: RenderProfileKind,
    width: u32,
    height: u32,
    hash: u64,
    out: &Path,
    blur_samples: &str,
) -> String {
    let blur = if blur_samples.is_empty() {
        String::new()
    } else {
        format!(", {blur_samples}")
    };
    format!(
        "Rendered frame at tick {} (profile {}, {}x{}, fnv1a=0x{:016x}{}) -> {}",
        tick,
        profile_label(profile),
        width,
        height,
        hash,
        blur,
        out.display()
    )
}

fn default_peak_window() -> TimeSpec {
    TimeSpec::Ticks(Flicks(FLICKS_PER_SECOND / 100))
}

fn default_cue_half_window() -> TimeSpec {
    TimeSpec::Ticks(Flicks(100 * FLICKS_PER_MILLISECOND))
}

fn assign_time(slot: &mut Option<TimeSpec>, spec: TimeSpec) -> Result<()> {
    if slot.replace(spec).is_some() {
        bail!("time was specified more than once");
    }
    Ok(())
}

/// Parses `--range <start>..<end>`; the end is exclusive and `..=` is rejected.
fn parse_range_spec(raw: &str) -> Result<RangeSpec> {
    let raw = raw.trim();
    if raw.contains("..=") {
        bail!("invalid range '{raw}'; '..=' is not supported, use end-exclusive '<start>..<end>'");
    }
    let Some((start, end)) = raw.split_once("..") else {
        bail!("invalid range '{raw}'; expected '<start>..<end>'");
    };
    if start.is_empty() || end.is_empty() {
        bail!("invalid range '{raw}'; both sides of '..' are required");
    }
    if end.contains("..") {
        bail!("invalid range '{raw}'; expected exactly one '..' separator");
    }
    Ok(RangeSpec {
        start: parse_time_spec(start).with_context(|| format!("invalid range start '{start}'"))?,
        end: parse_time_spec(end).with_context(|| format!("invalid range end '{end}'"))?,
    })
}

/// Integer Flicks, `<seconds>s`, `<num>/<den>s`, or `f:<frame>` at the project fps.
///
/// At most three decimal places are an integer millisecond count (`ms * 705_600`).
/// Longer decimals and explicit ratios scale with integer arithmetic. `f64` seconds
/// are not a timebase and are not used here.
fn parse_time_spec(raw: &str) -> Result<TimeSpec> {
    let raw = raw.trim();
    if raw.is_empty() {
        bail!("time value is empty");
    }
    if let Some(index) = raw.strip_prefix("f:").or_else(|| raw.strip_prefix("F:")) {
        let frame_index: i64 = index
            .parse()
            .with_context(|| format!("invalid frame index '{raw}'"))?;
        return Ok(TimeSpec::FrameIndex(frame_index));
    }
    if let Some(body) = raw.strip_suffix('s').or_else(|| raw.strip_suffix('S')) {
        let ticks = parse_seconds_body(body).with_context(|| format!("invalid seconds '{raw}'"))?;
        return Ok(TimeSpec::Ticks(ticks));
    }
    let ticks: i64 = raw.parse().with_context(|| {
        format!(
            "invalid time '{raw}'; expected integer Flicks, <seconds>s, <num>/<den>s, or f:<frame>"
        )
    })?;
    Ok(TimeSpec::Ticks(Flicks(ticks)))
}

fn parse_seconds_body(body: &str) -> Result<Flicks> {
    if body.is_empty() {
        bail!("seconds value is empty");
    }
    if let Some((num_text, den_text)) = body.split_once('/') {
        if den_text.contains('/') {
            bail!("invalid second ratio '{body}'");
        }
        let numer = parse_i128_token(num_text)?;
        let denom = parse_i128_token(den_text)?;
        return seconds_ratio_to_ticks(numer, denom);
    }
    parse_decimal_seconds(body)
}

fn parse_decimal_seconds(body: &str) -> Result<Flicks> {
    let (neg, unsigned) = split_ascii_sign(body)?;
    if unsigned.is_empty() {
        bail!("seconds value is empty");
    }
    let (int_digits, frac_digits) = if let Some((int_part, frac_part)) = unsigned.split_once('.') {
        if int_part.is_empty() || frac_part.is_empty() || frac_part.contains('.') {
            bail!("invalid seconds '{body}'");
        }
        (int_part, frac_part)
    } else {
        (unsigned, "")
    };
    if !is_ascii_digits(int_digits) || frac_digits.bytes().any(|byte| !byte.is_ascii_digit()) {
        bail!("invalid seconds '{body}'");
    }
    if frac_digits.len() <= 3 {
        let millis = integer_millis(int_digits, frac_digits)?;
        let millis = if neg {
            millis.checked_neg().context("seconds overflow Flicks")?
        } else {
            millis
        };
        return Flicks::from_millis_i64(millis).context("seconds overflow Flicks");
    }
    let mut numer: i128 = format!("{}{}", int_digits, frac_digits)
        .parse()
        .context("seconds overflow Flicks")?;
    if neg {
        numer = numer.checked_neg().context("seconds overflow Flicks")?;
    }
    let mut denom: i128 = 1;
    for _ in 0..frac_digits.len() {
        denom = denom.checked_mul(10).context("seconds overflow Flicks")?;
    }
    seconds_ratio_to_ticks(numer, denom)
}

fn integer_millis(int_digits: &str, frac_digits: &str) -> Result<i64> {
    let whole: i128 = int_digits.parse().context("seconds overflow Flicks")?;
    let mut frac = frac_digits.to_owned();
    while frac.len() < 3 {
        frac.push('0');
    }
    let frac_ms: i128 = frac.parse().context("seconds overflow Flicks")?;
    let millis = whole
        .checked_mul(1_000)
        .and_then(|value| value.checked_add(frac_ms))
        .context("seconds overflow Flicks")?;
    i64::try_from(millis).context("seconds overflow Flicks")
}

fn seconds_ratio_to_ticks(numer: i128, denom: i128) -> Result<Flicks> {
    if denom == 0 {
        bail!("second ratio denominator must be non-zero");
    }
    let (numer, denom) = if denom < 0 {
        (
            numer.checked_neg().context("seconds overflow Flicks")?,
            denom.checked_neg().context("seconds overflow Flicks")?,
        )
    } else {
        (numer, denom)
    };
    let scaled = numer
        .checked_mul(FLICKS_PER_SECOND as i128)
        .context("seconds overflow Flicks")?;
    let ticks = div_round_nearest_i128(scaled, denom);
    let ticks = i64::try_from(ticks).context("seconds overflow Flicks")?;
    Ok(Flicks(ticks))
}

fn div_round_nearest_i128(num: i128, den: i128) -> i128 {
    debug_assert!(den > 0);
    let quotient = num.div_euclid(den);
    let remainder = num.rem_euclid(den);
    if remainder.saturating_mul(2) >= den {
        quotient + 1
    } else {
        quotient
    }
}

fn split_ascii_sign(text: &str) -> Result<(bool, &str)> {
    match text.as_bytes().first() {
        Some(b'+') => Ok((false, &text[1..])),
        Some(b'-') => Ok((true, &text[1..])),
        Some(_) => Ok((false, text)),
        None => bail!("seconds value is empty"),
    }
}

fn parse_i128_token(text: &str) -> Result<i128> {
    let (neg, digits) = split_ascii_sign(text)?;
    if !is_ascii_digits(digits) {
        bail!("invalid integer '{text}'");
    }
    let value: i128 = digits
        .parse()
        .with_context(|| format!("integer '{text}' overflows"))?;
    if neg {
        value.checked_neg().context("integer overflow")
    } else {
        Ok(value)
    }
}

fn is_ascii_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

fn resolve_time_spec(spec: TimeSpec, fps: Rational32) -> Result<Flicks> {
    match spec {
        TimeSpec::Ticks(ticks) => Ok(ticks),
        TimeSpec::FrameIndex(index) => fps.frame_to_ticks(index).with_context(|| {
            format!(
                "failed to convert frame index {} at {} into Flicks",
                index, fps
            )
        }),
    }
}

fn format_beat_stamp(index: usize, tick: Flicks) -> String {
    format!("beat {} {}", index, tick)
}

fn format_cue_peak_stamp(info: &AudioPeakInfo) -> String {
    format!(
        "cue {} amp={:.6} rms={:.6}",
        info.peak_tick, info.peak_amplitude, info.rms
    )
}

fn format_beat_report(report: &AudioAnalysisReport) -> String {
    let mut text = format!(
        "beats={} bpm={:.3}\n",
        report.detected_beats.len(),
        report.estimated_bpm
    );
    for (index, tick) in report.detected_beats.iter().enumerate() {
        text.push_str(&format_beat_stamp(index, *tick));
        text.push('\n');
    }
    text
}

fn detected_anchor(event_id: &str, tick: Flicks) -> EventAnchorDef {
    EventAnchorDef {
        event_id: event_id.to_owned(),
        label: event_id.to_owned(),
        source_clip_id: None,
        source_anchor_tick: None,
        target_output_tick: tick,
        origin: EventOrigin::Detected,
    }
}

fn detected_event_anchors(beats: &[Flicks], cue: Option<Flicks>) -> Vec<EventAnchorDef> {
    let mut events = Vec::new();
    if let Some(tick) = cue {
        events.push(detected_anchor("detected_cue", tick));
    }
    for (index, tick) in beats.iter().enumerate() {
        events.push(detected_anchor(&format!("detected_beat_{}", index), *tick));
    }
    events
}

fn load_analysis_audio(project: &Project, asset_id: Option<&str>) -> Result<AudioBuffer> {
    let resolver = MediaResolver::new();
    if let Some(id) = asset_id {
        let asset = project
            .asset(id)
            .with_context(|| format!("project has no asset '{id}'"))?;
        return Ok(resolver.load_audio_asset(asset, project.timeline.duration_ticks)?);
    }
    resolver
        .mix_project_audio(project)?
        .context("project has no audio clips to analyze")
}

fn write_detected_events(
    project: &Project,
    path: &Path,
    beats: &[Flicks],
    cue: Option<Flicks>,
) -> Result<usize> {
    let events = detected_event_anchors(beats, cue);
    if events.is_empty() {
        bail!("--write-events found no peaks to stamp");
    }
    let count = events.len();
    let mut session = ProjectSession::new(project.clone())
        .context("project failed validation before writing detected events")?;
    let mut revision = session.revision_id();
    for event in events {
        revision = session
            .apply_command(revision, ProjectCommand::UpsertEventAnchor(event))
            .context("failed to stamp detected event")?;
    }
    let json = session.project().to_json_pretty()?;
    std::fs::write(path, json).with_context(|| format!("failed to write '{}'", path.display()))?;
    Ok(count)
}

fn format_autocut_report(
    cuts: &[Flicks],
    report: &AudioAnalysisReport,
    params: &frameshift_core::AutocutParams,
    fps: Rational32,
    range: Option<&RenderRange>,
    coordinate: &str,
) -> Result<String> {
    let mut text = format!(
        "autocut cuts={} beats={} bpm={:.3} fps={} coordinate={} min_segment={} start={} end={}\n",
        cuts.len(),
        report.detected_beats.len(),
        report.estimated_bpm,
        fps,
        coordinate,
        params.min_segment_ticks.0,
        params.include_start,
        params.include_end,
    );
    if let Some(range) = range {
        let _ = writeln!(
            text,
            "  range=[{}, {}) frames=[{}, {})",
            range.start_tick.0, range.end_tick.0, range.start_frame, range.end_frame
        );
    }
    for (index, tick) in cuts.iter().enumerate() {
        let ms = tick.0 as f64 / FLICKS_PER_MILLISECOND as f64;
        let frame = fps.ticks_to_frame_nearest(*tick)?;
        let _ = writeln!(text, "  [{index}] tick={} ms={ms:.3} ~f={frame}", tick.0);
    }
    Ok(text)
}

fn write_autocut_events(project: &Project, path: &Path, cuts: &[Flicks]) -> Result<usize> {
    if cuts.is_empty() {
        bail!("--write-events found no autocut points to stamp");
    }
    let mut session = ProjectSession::new(project.clone())
        .context("project failed validation before writing autocut events")?;
    let mut revision = session.revision_id();
    for (index, tick) in cuts.iter().enumerate() {
        let event = detected_anchor(&format!("autocut_{index:03}"), *tick);
        revision = session
            .apply_command(revision, ProjectCommand::UpsertEventAnchor(event))
            .context("failed to stamp autocut event")?;
    }
    let json = session.project().to_json_pretty()?;
    std::fs::write(path, json).with_context(|| format!("failed to write '{}'", path.display()))?;
    Ok(cuts.len())
}

fn parse_args(args: &[String]) -> Result<CliCommand> {
    let Some((sub, rest)) = args.split_first() else {
        return Ok(CliCommand::Help);
    };

    if matches!(sub.as_str(), "-h" | "--help" | "help") {
        return Ok(CliCommand::Help);
    }

    match sub.as_str() {
        "init-golden" => {
            let mut out: Option<PathBuf> = None;
            let mut i = 0;
            while i < rest.len() {
                if let Some(val) = take_flag_value(rest, &mut i, "--out")? {
                    out = Some(PathBuf::from(val));
                } else {
                    bail!(
                        "unknown or unexpected argument '{}' for init-golden",
                        rest[i]
                    );
                }
            }
            let out = out.context("init-golden requires --out <project.json>")?;
            Ok(CliCommand::InitGolden { out })
        }
        "render" => {
            let mut project_path: Option<PathBuf> = None;
            let mut out: Option<PathBuf> = None;
            let mut profile = RenderProfileKind::Final;
            let mut range: Option<RangeSpec> = None;
            let mut start: Option<TimeSpec> = None;
            let mut end: Option<TimeSpec> = None;
            let mut force_progress = false;
            let mut quiet = false;
            let mut i = 0;
            while i < rest.len() {
                if rest[i] == "--progress" {
                    force_progress = true;
                    i += 1;
                } else if rest[i] == "--quiet" {
                    quiet = true;
                    i += 1;
                } else if let Some(val) = take_flag_value(rest, &mut i, "--out")? {
                    out = Some(PathBuf::from(val));
                } else if let Some(val) = take_flag_value(rest, &mut i, "--profile")? {
                    profile = parse_profile(&val)?;
                } else if let Some(val) = take_flag_value(rest, &mut i, "--range")? {
                    if range.is_some() {
                        bail!("--range specified more than once");
                    }
                    range = Some(parse_range_spec(&val)?);
                } else if let Some(val) = take_flag_value(rest, &mut i, "--start")? {
                    assign_time(&mut start, parse_time_spec(&val)?)?;
                } else if let Some(val) = take_flag_value(rest, &mut i, "--end")? {
                    assign_time(&mut end, parse_time_spec(&val)?)?;
                } else if !rest[i].starts_with('-') && project_path.is_none() {
                    project_path = Some(PathBuf::from(&rest[i]));
                    i += 1;
                } else {
                    bail!("unknown or unexpected argument '{}' for render", rest[i]);
                }
            }
            let progress = match (force_progress, quiet) {
                (true, true) => bail!("--progress and --quiet cannot be combined"),
                (true, false) => ProgressMode::Always,
                (false, true) => ProgressMode::Never,
                (false, false) => ProgressMode::Auto,
            };
            let range = match (range, start, end) {
                (Some(spec), None, None) => Some(spec),
                (None, Some(start), Some(end)) => Some(RangeSpec { start, end }),
                (None, None, None) => None,
                (Some(_), _, _) => bail!("--range cannot be combined with --start/--end"),
                (None, Some(_), None) => bail!("--end is required when --start is given"),
                (None, None, Some(_)) => bail!("--start is required when --end is given"),
            };
            Ok(CliCommand::Render {
                project_path: project_path.context("render requires <project.json>")?,
                out: out.context("render requires --out <out.mp4>")?,
                profile,
                range,
                progress,
            })
        }
        "frame" => {
            let mut project_path: Option<PathBuf> = None;
            let mut time: Option<TimeSpec> = None;
            let mut out: Option<PathBuf> = None;
            let mut profile = RenderProfileKind::Final;
            let mut i = 0;
            while i < rest.len() {
                if let Some(val) = take_flag_value(rest, &mut i, "--tick")? {
                    assign_time(&mut time, parse_time_spec(&val)?)?;
                } else if let Some(val) = take_flag_value(rest, &mut i, "--at")? {
                    assign_time(&mut time, parse_time_spec(&val)?)?;
                } else if let Some(val) = take_flag_value(rest, &mut i, "--out")? {
                    out = Some(PathBuf::from(val));
                } else if let Some(val) = take_flag_value(rest, &mut i, "--profile")? {
                    profile = parse_profile(&val)?;
                } else if !rest[i].starts_with('-') && project_path.is_none() {
                    project_path = Some(PathBuf::from(&rest[i]));
                    i += 1;
                } else {
                    bail!("unknown or unexpected argument '{}' for frame", rest[i]);
                }
            }
            Ok(CliCommand::Frame {
                project_path: project_path.context("frame requires <project.json>")?,
                time: time.context("frame requires --tick <time> or --at <time>")?,
                out: out.context("frame requires --out <out.png>")?,
                profile,
            })
        }
        "verify-m1" => {
            let mut out_dir: Option<PathBuf> = None;
            let mut i = 0;
            while i < rest.len() {
                if let Some(val) = take_flag_value(rest, &mut i, "--out-dir")? {
                    out_dir = Some(PathBuf::from(val));
                } else {
                    bail!("unknown or unexpected argument '{}' for verify-m1", rest[i]);
                }
            }
            Ok(CliCommand::VerifyM1 {
                out_dir: out_dir.context("verify-m1 requires --out-dir <dir>")?,
            })
        }
        "inspect" => {
            let mut project_path: Option<PathBuf> = None;
            let mut out_dir: Option<PathBuf> = None;
            let mut i = 0;
            while i < rest.len() {
                if let Some(val) = take_flag_value(rest, &mut i, "--out-dir")? {
                    out_dir = Some(PathBuf::from(val));
                } else if !rest[i].starts_with('-') && project_path.is_none() {
                    project_path = Some(PathBuf::from(&rest[i]));
                    i += 1;
                } else {
                    bail!("unknown or unexpected argument '{}' for inspect", rest[i]);
                }
            }
            Ok(CliCommand::Inspect {
                project_path: project_path.context("inspect requires <project.json>")?,
                out_dir: out_dir.context("inspect requires --out-dir <dir>")?,
            })
        }
        "materials" => {
            let mut project_path: Option<PathBuf> = None;
            let mut out_dir: Option<PathBuf> = None;
            let mut i = 0;
            while i < rest.len() {
                if let Some(val) = take_flag_value(rest, &mut i, "--out-dir")? {
                    out_dir = Some(PathBuf::from(val));
                } else if !rest[i].starts_with('-') && project_path.is_none() {
                    project_path = Some(PathBuf::from(&rest[i]));
                    i += 1;
                } else {
                    bail!("unknown or unexpected argument '{}' for materials", rest[i]);
                }
            }
            Ok(CliCommand::Materials {
                project_path: project_path.context("materials requires <project.json>")?,
                out_dir: out_dir.context("materials requires --out-dir <dir>")?,
            })
        }
        "ui-snapshot" => {
            let mut project_path: Option<PathBuf> = None;
            let mut out: Option<PathBuf> = None;
            let mut time: Option<TimeSpec> = None;
            let mut i = 0;
            while i < rest.len() {
                if let Some(val) = take_flag_value(rest, &mut i, "--out")? {
                    out = Some(PathBuf::from(val));
                } else if let Some(val) = take_flag_value(rest, &mut i, "--tick")? {
                    assign_time(&mut time, parse_time_spec(&val)?)?;
                } else if !rest[i].starts_with('-') && project_path.is_none() {
                    project_path = Some(PathBuf::from(&rest[i]));
                    i += 1;
                } else {
                    bail!(
                        "unknown or unexpected argument '{}' for ui-snapshot",
                        rest[i]
                    );
                }
            }
            Ok(CliCommand::UiSnapshot {
                project_path: project_path.context("ui-snapshot requires <project.json>")?,
                out: out.context("ui-snapshot requires --out <out.png>")?,
                time,
            })
        }
        "peaks" => {
            let mut project_path: Option<PathBuf> = None;
            let mut asset_id: Option<String> = None;
            let mut window: Option<TimeSpec> = None;
            let mut at: Option<TimeSpec> = None;
            let mut half_window: Option<TimeSpec> = None;
            let mut write_events = false;
            let mut i = 0;
            while i < rest.len() {
                if rest[i] == "--write-events" {
                    write_events = true;
                    i += 1;
                } else if let Some(val) = take_flag_value(rest, &mut i, "--asset")? {
                    if asset_id.is_some() {
                        bail!("--asset specified more than once");
                    }
                    asset_id = Some(val);
                } else if let Some(val) = take_flag_value(rest, &mut i, "--window")? {
                    assign_time(&mut window, parse_time_spec(&val)?)?;
                } else if let Some(val) = take_flag_value(rest, &mut i, "--at")? {
                    assign_time(&mut at, parse_time_spec(&val)?)?;
                } else if let Some(val) = take_flag_value(rest, &mut i, "--half-window")? {
                    assign_time(&mut half_window, parse_time_spec(&val)?)?;
                } else if !rest[i].starts_with('-') && project_path.is_none() {
                    project_path = Some(PathBuf::from(&rest[i]));
                    i += 1;
                } else {
                    bail!("unknown or unexpected argument '{}' for peaks", rest[i]);
                }
            }
            Ok(CliCommand::Peaks {
                project_path: project_path.context("peaks requires <project.json>")?,
                asset_id,
                window: window.unwrap_or_else(default_peak_window),
                at,
                half_window: half_window.unwrap_or_else(default_cue_half_window),
                write_events,
            })
        }
        "autocut" => {
            let mut project_path: Option<PathBuf> = None;
            let mut asset_id: Option<String> = None;
            let mut window: Option<TimeSpec> = None;
            let mut range: Option<RangeSpec> = None;
            let mut at: Option<TimeSpec> = None;
            let mut half_window: Option<TimeSpec> = None;
            let mut min_segment: Option<TimeSpec> = None;
            let mut no_start = false;
            let mut no_end = false;
            let mut write_events = false;
            let mut i = 0;
            while i < rest.len() {
                if rest[i] == "--write-events" {
                    write_events = true;
                    i += 1;
                } else if rest[i] == "--no-start" {
                    no_start = true;
                    i += 1;
                } else if rest[i] == "--no-end" {
                    no_end = true;
                    i += 1;
                } else if let Some(val) = take_flag_value(rest, &mut i, "--asset")? {
                    asset_id = Some(val);
                } else if let Some(val) = take_flag_value(rest, &mut i, "--window")? {
                    assign_time(&mut window, parse_time_spec(&val)?)?;
                } else if let Some(val) = take_flag_value(rest, &mut i, "--range")? {
                    if range.is_some() {
                        bail!("--range specified more than once");
                    }
                    range = Some(parse_range_spec(&val)?);
                } else if let Some(val) = take_flag_value(rest, &mut i, "--at")? {
                    assign_time(&mut at, parse_time_spec(&val)?)?;
                } else if let Some(val) = take_flag_value(rest, &mut i, "--half-window")? {
                    assign_time(&mut half_window, parse_time_spec(&val)?)?;
                } else if let Some(val) = take_flag_value(rest, &mut i, "--min-segment")? {
                    assign_time(&mut min_segment, parse_time_spec(&val)?)?;
                } else if !rest[i].starts_with('-') && project_path.is_none() {
                    project_path = Some(PathBuf::from(&rest[i]));
                    i += 1;
                } else {
                    bail!("unknown or unexpected argument '{}' for autocut", rest[i]);
                }
            }
            Ok(CliCommand::Autocut {
                project_path: project_path.context("autocut requires <project.json>")?,
                asset_id,
                window: window.unwrap_or_else(default_peak_window),
                range,
                at,
                half_window: half_window.unwrap_or_else(default_cue_half_window),
                min_segment,
                no_start,
                no_end,
                write_events,
            })
        }
        "mcp-stdio" => {
            if !rest.is_empty() {
                bail!("unexpected argument '{}' for mcp-stdio", rest[0]);
            }
            Ok(CliCommand::McpStdio)
        }
        "verify-all" => {
            let mut out_dir: Option<PathBuf> = None;
            let mut i = 0;
            while i < rest.len() {
                if let Some(val) = take_flag_value(rest, &mut i, "--out-dir")? {
                    out_dir = Some(PathBuf::from(val));
                } else {
                    bail!(
                        "unknown or unexpected argument '{}' for verify-all",
                        rest[i]
                    );
                }
            }
            Ok(CliCommand::VerifyAll {
                out_dir: out_dir.context("verify-all requires --out-dir <dir>")?,
            })
        }
        other => bail!("unknown subcommand '{other}'. Run with --help for usage."),
    }
}

fn take_flag_value(args: &[String], idx: &mut usize, flag: &str) -> Result<Option<String>> {
    let cur = &args[*idx];
    if cur == flag {
        let next = args
            .get(*idx + 1)
            .with_context(|| format!("flag '{flag}' requires a value"))?;
        *idx += 2;
        return Ok(Some(next.clone()));
    }
    if let Some(val) = cur.strip_prefix(&format!("{flag}=")) {
        *idx += 1;
        return Ok(Some(val.to_owned()));
    }
    Ok(None)
}

fn parse_profile(s: &str) -> Result<RenderProfileKind> {
    match s.to_ascii_lowercase().as_str() {
        "draft" => Ok(RenderProfileKind::Draft),
        "final" => Ok(RenderProfileKind::Final),
        other => bail!("invalid profile '{other}'; expected 'draft' or 'final'"),
    }
}

fn print_usage() {
    println!(
        "FrameShift Headless CLI\n\
         \n\
         USAGE:\n\
         \tframeshift-cli init-golden --out <project.json>\n\
         \tframeshift-cli render <project.json> --out <out.mp4> [--profile draft|final] [--range <start>..<end> | --start <time> --end <time>] [--progress | --quiet]\n\
         \tframeshift-cli frame <project.json> (--tick <time> | --at <time>) --out <out.png> [--profile draft|final]\n\
         \tframeshift-cli verify-m1 --out-dir <dir>\n\
         \tframeshift-cli inspect <project.json> --out-dir <dir>\n\
         \tframeshift-cli ui-snapshot <project.json> --out <out.png> [--tick <time>]\n\
         \tframeshift-cli materials <project.json> --out-dir <dir>\n\
         \tframeshift-cli peaks <project.json> [--asset <id>] [--window <time>] [--at <time>] [--half-window <time>] [--write-events]\n\
         \tframeshift-cli mcp-stdio\n\
         \tframeshift-cli autocut <project.json> [--asset <id>] [--window <time>] [--range <start>..<end>] [--at <time>] [--half-window <time>] [--min-segment <time>] [--no-start] [--no-end] [--write-events]\n\
         \tframeshift-cli verify-all --out-dir <dir>\n\
         \n\
         Time is integer Flicks, <seconds>s (1.905s = 1905 ms), <num>/<den>s, or f:<frame> at the project fps.\n\
         Render profile defaults to final. --range is end-exclusive and snaps outward to the frame grid.\n\
         Autocut --range is end-exclusive; ticks stay in project coordinates by default and source coordinates with --asset.\n\
         Progress goes to stderr (auto on a TTY, --progress forces it, --quiet disables it).\n\
         --write-events stamps Detected anchors into the project JSON."
    );
    workbench::print_usage();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn parses_all_subcommands() {
        assert_eq!(
            parse_args(&args(&["init-golden", "--out", "p.json"])).unwrap(),
            CliCommand::InitGolden {
                out: PathBuf::from("p.json")
            }
        );
        assert_eq!(
            parse_args(&args(&[
                "render",
                "p.json",
                "--out=out.mp4",
                "--profile",
                "draft"
            ]))
            .unwrap(),
            CliCommand::Render {
                project_path: PathBuf::from("p.json"),
                out: PathBuf::from("out.mp4"),
                profile: RenderProfileKind::Draft,
                range: None,
                progress: ProgressMode::Auto,
            }
        );
        assert_eq!(
            parse_args(&args(&[
                "frame",
                "p.json",
                "--tick",
                "1764000000",
                "--out",
                "f.png",
            ]))
            .unwrap(),
            CliCommand::Frame {
                project_path: PathBuf::from("p.json"),
                time: TimeSpec::Ticks(Flicks(1_764_000_000)),
                out: PathBuf::from("f.png"),
                profile: RenderProfileKind::Final,
            }
        );
        assert_eq!(
            parse_args(&args(&["verify-m1", "--out-dir", "target/m1"])).unwrap(),
            CliCommand::VerifyM1 {
                out_dir: PathBuf::from("target/m1")
            }
        );
        assert_eq!(
            parse_args(&args(&["inspect", "p.json", "--out-dir", "target/insp"])).unwrap(),
            CliCommand::Inspect {
                project_path: PathBuf::from("p.json"),
                out_dir: PathBuf::from("target/insp")
            }
        );
        assert_eq!(
            parse_args(&args(&[
                "ui-snapshot",
                "p.json",
                "--out",
                "ui.png",
                "--tick",
                "1764000000"
            ]))
            .unwrap(),
            CliCommand::UiSnapshot {
                project_path: PathBuf::from("p.json"),
                out: PathBuf::from("ui.png"),
                time: Some(TimeSpec::Ticks(Flicks(1_764_000_000)))
            }
        );
        assert_eq!(
            parse_args(&args(&["peaks", "p.json"])).unwrap(),
            CliCommand::Peaks {
                project_path: PathBuf::from("p.json"),
                asset_id: None,
                window: default_peak_window(),
                at: None,
                half_window: default_cue_half_window(),
                write_events: false,
            }
        );
        assert_eq!(
            parse_args(&args(&["mcp-stdio"])).unwrap(),
            CliCommand::McpStdio
        );
        assert_eq!(
            parse_args(&args(&["verify-all", "--out-dir", "target/full_verify"])).unwrap(),
            CliCommand::VerifyAll {
                out_dir: PathBuf::from("target/full_verify")
            }
        );
    }

    #[test]
    fn rejects_invalid_profile_and_missing_args() {
        assert!(
            parse_args(&args(&[
                "render",
                "p.json",
                "--out",
                "o.mp4",
                "--profile",
                "ultra"
            ]))
            .is_err()
        );
        assert!(parse_args(&args(&["frame", "p.json", "--out", "o.png"])).is_err());
        assert!(parse_args(&args(&["verify-m1"])).is_err());
        assert!(parse_args(&args(&["inspect", "p.json"])).is_err());
        assert!(parse_args(&args(&["ui-snapshot", "p.json"])).is_err());
        assert!(parse_args(&args(&["verify-all"])).is_err());
        assert!(parse_args(&args(&["peaks"])).is_err());
        assert!(
            parse_args(&args(&[
                "frame", "p.json", "--tick", "1", "--at", "1s", "--out", "f.png"
            ]))
            .is_err()
        );
    }

    #[test]
    fn render_success_prints_fps_once_and_profile() {
        let line = format_render_success(
            1920,
            1080,
            24,
            frameshift_time::FPS_24,
            true,
            RenderProfileKind::Final,
            Path::new("out.mp4"),
            "",
            None,
        );
        assert_eq!(line.matches("fps").count(), 1);
        assert!(line.contains("24 fps"));
        assert!(line.contains("profile final"));
        assert!(!line.contains("fps fps"));

        let ntsc = format_render_success(
            320,
            180,
            1,
            frameshift_time::FPS_23_976,
            false,
            RenderProfileKind::Draft,
            Path::new("draft.mp4"),
            "",
            None,
        );
        assert_eq!(ntsc.matches("fps").count(), 1);
        assert!(ntsc.contains("profile draft"));
        assert!(!ntsc.contains("fps fps"));
    }

    #[test]
    fn seconds_and_frame_at_resolve_to_integer_flicks() {
        assert_eq!(
            parse_time_spec("1.905s").unwrap(),
            TimeSpec::Ticks(Flicks(1_344_168_000))
        );
        assert_eq!(
            parse_time_spec("1905/1000s").unwrap(),
            TimeSpec::Ticks(Flicks(1_344_168_000))
        );
        assert_eq!(
            parse_time_spec("0.0005s").unwrap(),
            TimeSpec::Ticks(Flicks(352_800))
        );
        assert_eq!(parse_time_spec("10").unwrap(), TimeSpec::Ticks(Flicks(10)));
        assert_eq!(
            parse_time_spec("10s").unwrap(),
            TimeSpec::Ticks(Flicks(10 * FLICKS_PER_SECOND))
        );
        assert!(parse_time_spec("1.905").is_err());

        let human = parse_args(&args(&[
            "frame", "p.json", "--at", "1.905s", "--out", "f.png",
        ]))
        .unwrap();
        let raw = parse_args(&args(&[
            "frame",
            "p.json",
            "--tick",
            "1344168000",
            "--out",
            "f.png",
        ]))
        .unwrap();
        assert_eq!(human, raw);
        match human {
            CliCommand::Frame { time, profile, .. } => {
                assert_eq!(time, TimeSpec::Ticks(Flicks(1_344_168_000)));
                assert_eq!(profile, RenderProfileKind::Final);
            }
            other => panic!("expected frame command, got {other:?}"),
        }

        assert_eq!(
            resolve_time_spec(TimeSpec::FrameIndex(10), frameshift_time::FPS_24).unwrap(),
            Flicks(294_000_000)
        );
        assert_eq!(
            parse_args(&args(&[
                "frame", "p.json", "--at", "f:10", "--out", "f.png"
            ]))
            .unwrap(),
            CliCommand::Frame {
                project_path: PathBuf::from("p.json"),
                time: TimeSpec::FrameIndex(10),
                out: PathBuf::from("f.png"),
                profile: RenderProfileKind::Final,
            }
        );
    }

    #[test]
    fn cue_peak_stamp_prints_integer_ticks() {
        let (buf, _) = SyntheticMediaGenerator::generate_beat_audio(
            Flicks(2 * FLICKS_PER_SECOND),
            120,
            DEFAULT_AUDIO_SAMPLE_RATE,
        );
        let report = buf
            .analyze_beats_and_energy(Flicks(FLICKS_PER_SECOND / 100))
            .unwrap();
        assert!(!report.detected_beats.is_empty());
        let text = format_beat_report(&report);
        for beat in &report.detected_beats {
            assert!(text.contains(&format!("{} flicks", beat.0)));
            assert_eq!(beat.0, beat.as_i64());
        }
        let cue = buf
            .detect_peak_in_window(
                report.detected_beats[0],
                Flicks(100 * FLICKS_PER_MILLISECOND),
            )
            .unwrap();
        let stamp = format_cue_peak_stamp(&cue);
        assert!(stamp.contains(&format!("{} flicks", cue.peak_tick.0)));
        assert_eq!(stamp.matches("fps").count(), 0);

        let events = detected_event_anchors(&[cue.peak_tick], Some(cue.peak_tick));
        assert!(
            events
                .iter()
                .all(|event| event.origin == EventOrigin::Detected)
        );
        assert_eq!(events[0].event_id, "detected_cue");
        assert_eq!(events[0].target_output_tick, cue.peak_tick);
        assert_eq!(events[1].target_output_tick.0, cue.peak_tick.0);
    }

    #[test]
    fn frame_success_keeps_tick_display_and_names_profile() {
        let tick = Flicks(1_344_168_000);
        let line = format_frame_success(
            tick,
            RenderProfileKind::Final,
            64,
            64,
            0xabc,
            Path::new("f.png"),
            "",
        );
        assert!(line.contains(&tick.to_string()));
        assert!(line.contains("profile final"));
        assert!(!line.contains("fps fps"));
    }

    #[test]
    fn success_line_prints_authored_blur_samples_unless_ceiling_is_set() {
        let project = Project::golden_scene_preset();
        assert_eq!(project.render_profiles.draft.motion_blur_sub_samples, 4);
        let authored = format_effective_blur_samples(&project, None);
        assert_eq!(authored, "motion_blur_out: node 8, effective 8");
        let capped = format_effective_blur_samples(&project, Some(1));
        assert_eq!(capped, "motion_blur_out: node 8, effective 1");
        let line = format_render_success(
            1280,
            720,
            1,
            frameshift_time::FPS_24,
            false,
            RenderProfileKind::Draft,
            Path::new("draft.mp4"),
            &authored,
            None,
        );
        assert!(line.contains("profile draft"));
        assert!(line.contains("node 8, effective 8"));
        assert!(!line.contains("effective 4"));
    }

    #[test]
    fn parses_render_range_and_progress_flags() {
        assert_eq!(
            parse_args(&args(&[
                "render",
                "p.json",
                "--out",
                "o.mp4",
                "--range",
                "7.5s..9.0s",
            ]))
            .unwrap(),
            CliCommand::Render {
                project_path: PathBuf::from("p.json"),
                out: PathBuf::from("o.mp4"),
                profile: RenderProfileKind::Final,
                range: Some(RangeSpec {
                    start: TimeSpec::Ticks(Flicks(5_292_000_000)),
                    end: TimeSpec::Ticks(Flicks(6_350_400_000)),
                }),
                progress: ProgressMode::Auto,
            }
        );

        let frames = parse_args(&args(&[
            "render",
            "p.json",
            "--out=o.mp4",
            "--range=f:450..f:540",
            "--progress",
        ]))
        .unwrap();
        match frames {
            CliCommand::Render {
                range, progress, ..
            } => {
                assert_eq!(
                    range,
                    Some(RangeSpec {
                        start: TimeSpec::FrameIndex(450),
                        end: TimeSpec::FrameIndex(540),
                    })
                );
                assert_eq!(progress, ProgressMode::Always);
            }
            other => panic!("expected render, got {other:?}"),
        }

        assert_eq!(
            parse_args(&args(&[
                "render", "p.json", "--out", "o.mp4", "--start", "f:450", "--end", "f:540",
                "--quiet",
            ]))
            .unwrap(),
            CliCommand::Render {
                project_path: PathBuf::from("p.json"),
                out: PathBuf::from("o.mp4"),
                profile: RenderProfileKind::Final,
                range: Some(RangeSpec {
                    start: TimeSpec::FrameIndex(450),
                    end: TimeSpec::FrameIndex(540),
                }),
                progress: ProgressMode::Never,
            }
        );
    }

    #[test]
    fn rejects_invalid_range_and_progress_combinations() {
        for bad in [
            "7.5s..",
            "..9.0s",
            "7.5s..9.0s..10s",
            "7.5s..=9.0s",
            "nonsense..9.0s",
            "1s..nonsense",
        ] {
            assert!(
                parse_args(&args(&[
                    "render", "p.json", "--out", "o.mp4", "--range", bad
                ]))
                .is_err(),
                "range '{bad}' must be rejected"
            );
        }
        assert!(
            parse_args(&args(&[
                "render", "p.json", "--out", "o.mp4", "--range", "1s..2s", "--start", "1s",
            ]))
            .is_err()
        );
        assert!(
            parse_args(&args(&[
                "render", "p.json", "--out", "o.mp4", "--start", "1s"
            ]))
            .is_err()
        );
        assert!(
            parse_args(&args(&[
                "render", "p.json", "--out", "o.mp4", "--end", "2s"
            ]))
            .is_err()
        );
        assert!(
            parse_args(&args(&[
                "render",
                "p.json",
                "--out",
                "o.mp4",
                "--progress",
                "--quiet",
            ]))
            .is_err()
        );
        assert!(
            parse_args(&args(&[
                "render", "p.json", "--out", "o.mp4", "--range", "1s..2s", "--range", "3s..4s",
            ]))
            .is_err()
        );
    }

    #[test]
    fn autocut_parses_an_absolute_range_and_rejects_duplicates() {
        assert_eq!(
            parse_args(&args(&[
                "autocut",
                "p.json",
                "--range",
                "f:120..f:240",
                "--no-start",
            ]))
            .unwrap(),
            CliCommand::Autocut {
                project_path: PathBuf::from("p.json"),
                asset_id: None,
                window: default_peak_window(),
                range: Some(RangeSpec {
                    start: TimeSpec::FrameIndex(120),
                    end: TimeSpec::FrameIndex(240),
                }),
                at: None,
                half_window: default_cue_half_window(),
                min_segment: None,
                no_start: true,
                no_end: false,
                write_events: false,
            }
        );
        assert!(
            parse_args(&args(&[
                "autocut", "p.json", "--range", "1s..2s", "--range", "3s..4s",
            ]))
            .is_err()
        );
    }

    #[test]
    fn autocut_report_uses_project_fps_and_keeps_range_ticks_absolute() {
        let report = AudioAnalysisReport {
            window_ticks: Flicks(70),
            rms_envelope: vec![],
            detected_beats: vec![Flicks(705_600_000)],
            estimated_bpm: 120.0,
        };
        let params = frameshift_core::AutocutParams {
            start_tick: Flicks(705_600_000),
            duration_ticks: Flicks(705_600_000),
            min_segment_ticks: Flicks::ZERO,
            include_start: true,
            include_end: true,
        };
        let range = RenderRange {
            start_tick: Flicks(705_600_000),
            end_tick: Flicks(1_411_200_000),
            start_frame: 60,
            end_frame: 120,
        };
        let text = format_autocut_report(
            &[Flicks(705_600_000), Flicks(1_411_200_000)],
            &report,
            &params,
            frameshift_time::FPS_60,
            Some(&range),
            "project",
        )
        .unwrap();
        assert!(text.contains("fps=60 fps coordinate=project"));
        assert!(text.contains("range=[705600000, 1411200000) frames=[60, 120)"));
        assert!(text.contains("tick=705600000 ms=1000.000 ~f=60"));
        assert!(text.contains("tick=1411200000 ms=2000.000 ~f=120"));
    }

    #[test]
    fn render_success_appends_resolved_range_only_when_present() {
        let range = RenderRange {
            start_tick: Flicks(5_292_000_000),
            end_tick: Flicks(6_350_400_000),
            start_frame: 450,
            end_frame: 540,
        };
        let line = format_render_success(
            1920,
            1080,
            90,
            frameshift_time::FPS_60,
            true,
            RenderProfileKind::Draft,
            Path::new("range.mp4"),
            "",
            Some(&range),
        );
        assert!(line.contains("90 frames"));
        assert!(line.contains("range f:450..f:540"));
        assert_eq!(line.matches("fps").count(), 1);

        let full = format_render_success(
            1920,
            1080,
            1200,
            frameshift_time::FPS_60,
            true,
            RenderProfileKind::Draft,
            Path::new("full.mp4"),
            "",
            None,
        );
        assert!(!full.contains("range"));
    }
}
