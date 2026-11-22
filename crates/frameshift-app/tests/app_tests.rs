use frameshift_app::{
    ActorKind, AppError, BackgroundJobSupervisor, ClipEditAction, JobKind, JobState,
    OptimisticLockGuard, WindowClosePolicy, WorkspaceUiController, render_desktop_ui_snapshot,
};
use frameshift_media::{AlphaMode, Rgba8Frame};
use frameshift_schema::{NodeId, NodeKind, Project, ProjectCommand, RenderProfileKind};
use frameshift_time::{Flicks, FLICKS_PER_SECOND};

fn golden_controller() -> WorkspaceUiController {
    WorkspaceUiController::from_project(Project::golden_scene_preset())
        .expect("golden_scene_preset initializes controller")
}

#[test]
fn human_edit_rejects_stale_agent_and_supports_undo_redo() {
    let mut ctrl = golden_controller();
    let initial_rev = ctrl.revision_id();
    assert_eq!(initial_rev, 1);

    // Agent captures revision #1 before planning an edit.
    let agent_snapshot_rev = ctrl.revision_id();
    assert!(
        OptimisticLockGuard::new(agent_snapshot_rev)
            .verify(ctrl.revision_id())
            .is_ok()
    );

    // Human scrubs playhead (ephemeral UI state; does not advance revision) and moves event anchor.
    let target_tick = Flicks(2 * FLICKS_PER_SECOND + FLICKS_PER_SECOND / 4);
    ctrl.scrub_to(target_tick);
    assert_eq!(ctrl.playhead_tick, target_tick);
    assert_eq!(ctrl.revision_id(), 1);

    let rev_after_human = ctrl
        .human_move_event_anchor("drop_impact", target_tick)
        .expect("human event anchor move succeeds");
    assert_eq!(rev_after_human, 2);
    assert_eq!(
        ctrl.project()
            .event("drop_impact")
            .unwrap()
            .target_output_tick,
        target_tick
    );

    // Stale concurrent agent mutation using revision #1 must be rejected cleanly.
    let stale_err = ctrl
        .apply_agent_command(
            agent_snapshot_rev,
            "agent-director",
            ProjectCommand::SetProjectName {
                name: "Stale Agent Overwrite".into(),
            },
        )
        .unwrap_err();
    match stale_err {
        AppError::ConcurrencyConflict { expected, actual } => {
            assert_eq!(expected, 1);
            assert_eq!(actual, 2);
        }
        other => panic!("expected ConcurrencyConflict, got {other:?}"),
    }
    assert_eq!(ctrl.project().name, "Golden Scene — M1 Vertical Slice");
    assert_eq!(
        ctrl.project()
            .event("drop_impact")
            .unwrap()
            .target_output_tick,
        target_tick
    );

    // Fresh agent mutation at revision #2 succeeds.
    let rev_after_agent = ctrl
        .apply_agent_command(
            ctrl.revision_id(),
            "agent-director",
            ProjectCommand::SetProjectName {
                name: "Hybrid Human+Agent Edit".into(),
            },
        )
        .expect("fresh agent command succeeds");
    assert_eq!(rev_after_agent, 3);
    assert_eq!(ctrl.project().name, "Hybrid Human+Agent Edit");
    assert!(matches!(
        ctrl.activity_log.last().unwrap().actor,
        ActorKind::Agent { .. }
    ));

    // Human splits clip_a and updates a node, then verifies undo & redo.
    let rev_after_split = ctrl
        .human_split_or_trim_clip(
            "clip_a",
            ClipEditAction::Split {
                split_tick: Flicks(FLICKS_PER_SECOND),
                right_clip_id: "clip_a_tail".into(),
            },
        )
        .expect("split clip_a succeeds");
    assert_eq!(rev_after_split, 4);
    assert!(ctrl.project().clip("clip_a_tail").is_some());
    assert_eq!(
        ctrl.project().clip("clip_a").unwrap().timeline_end,
        Flicks(FLICKS_PER_SECOND)
    );

    let mut grade_node = ctrl
        .project()
        .graph
        .node(&NodeId::from("grade_flash"))
        .unwrap()
        .clone();
    if let NodeKind::ColorBasic {
        ref mut flash_amount,
        ..
    } = grade_node.kind
    {
        *flash_amount = 0.75;
    }
    let rev_after_node = ctrl
        .human_update_node(grade_node)
        .expect("human node update succeeds");
    assert_eq!(rev_after_node, 5);

    // Undo node update -> revision #6, flash_amount restored to 0.25
    let rev_undo_1 = ctrl.undo().expect("undo node update succeeds");
    assert_eq!(rev_undo_1, 6);
    match &ctrl
        .project()
        .graph
        .node(&NodeId::from("grade_flash"))
        .unwrap()
        .kind
    {
        NodeKind::ColorBasic { flash_amount, .. } => assert!((*flash_amount - 0.25).abs() < 1e-6),
        other => panic!("unexpected node kind {other:?}"),
    }

    // Undo split -> revision #7, clip_a_tail gone and clip_a end restored to 3s
    let rev_undo_2 = ctrl.undo().expect("undo split succeeds");
    assert_eq!(rev_undo_2, 7);
    assert!(ctrl.project().clip("clip_a_tail").is_none());
    assert_eq!(
        ctrl.project().clip("clip_a").unwrap().timeline_end,
        Flicks(3 * FLICKS_PER_SECOND)
    );

    // Redo split -> revision #8, clip_a_tail restored
    let rev_redo = ctrl.redo().expect("redo split succeeds");
    assert_eq!(rev_redo, 8);
    assert!(ctrl.project().clip("clip_a_tail").is_some());
}

#[test]
fn human_trim_clip_advances_source_start_and_revision() {
    let mut ctrl = golden_controller();
    let trim_start = Flicks(FLICKS_PER_SECOND / 2);
    let trim_end = Flicks(2 * FLICKS_PER_SECOND + FLICKS_PER_SECOND / 2);
    let rev = ctrl
        .human_split_or_trim_clip(
            "clip_a",
            ClipEditAction::Trim {
                new_timeline_start: Some(trim_start),
                new_timeline_end: Some(trim_end),
            },
        )
        .expect("trim succeeds");
    assert_eq!(rev, 2);
    let clip = ctrl.project().clip("clip_a").unwrap();
    assert_eq!(clip.timeline_start, trim_start);
    assert_eq!(clip.timeline_end, trim_end);
    assert!(clip.source_start > Flicks::ZERO);
}

#[test]
fn background_job_supervisor_window_close_policies() {
    // 1. DetachAndContinueBackground leaves running/queued jobs active and marks them detached.
    let mut sup = BackgroundJobSupervisor::new();
    let j_render = sup.enqueue(JobKind::FinalExportRender, "Final 1080p MP4", 3);
    let j_agent = sup.enqueue(JobKind::AgentDirectorTask, "Beat-sync pass", 3);
    let j_done = sup.enqueue(JobKind::DraftPreviewRender, "Draft scrub cache", 2);

    sup.start(j_render).unwrap();
    sup.update_progress(j_render, 42).unwrap();
    sup.start(j_done).unwrap();
    sup.complete(j_done).unwrap();

    let detach_report = sup.on_window_close(WindowClosePolicy::DetachAndContinueBackground);
    assert!(!sup.is_window_open());
    assert_eq!(detach_report.detached_jobs, vec![j_render, j_agent]);
    assert_eq!(detach_report.untouched_terminal_jobs, vec![j_done]);
    assert!(detach_report.cancelled_jobs.is_empty());

    assert_eq!(
        sup.job(j_render).unwrap().state,
        JobState::Running { progress_pct: 42 }
    );
    assert!(sup.job(j_render).unwrap().detached_to_background);
    assert_eq!(sup.job(j_agent).unwrap().state, JobState::Queued);
    assert!(sup.job(j_agent).unwrap().detached_to_background);
    assert_eq!(sup.job(j_done).unwrap().state, JobState::Completed);
    assert_eq!(sup.jobs_for_revision(3).len(), 2);

    // Reattach clears detached flags for active jobs.
    sup.reattach_window();
    assert!(sup.is_window_open());
    assert!(!sup.job(j_render).unwrap().detached_to_background);

    // 2. PauseGracefully transitions active jobs to Paused while preserving progress.
    let pause_report = sup.on_window_close(WindowClosePolicy::PauseGracefully);
    assert_eq!(pause_report.paused_jobs, vec![j_render, j_agent]);
    assert_eq!(
        sup.job(j_render).unwrap().state,
        JobState::Paused { progress_pct: 42 }
    );
    assert_eq!(
        sup.job(j_agent).unwrap().state,
        JobState::Paused { progress_pct: 0 }
    );

    // Resume and then test CancelAll.
    sup.reattach_window();
    sup.start(j_render).unwrap();
    assert_eq!(
        sup.job(j_render).unwrap().state,
        JobState::Running { progress_pct: 42 }
    );

    let cancel_report = sup.on_window_close(WindowClosePolicy::CancelAll);
    assert_eq!(cancel_report.cancelled_jobs, vec![j_render, j_agent]);
    assert_eq!(cancel_report.untouched_terminal_jobs, vec![j_done]);
    assert_eq!(sup.job(j_render).unwrap().state, JobState::Cancelled);
    assert_eq!(sup.job(j_agent).unwrap().state, JobState::Cancelled);
    assert_eq!(sup.job(j_done).unwrap().state, JobState::Completed);
    assert!(sup.active_jobs().is_empty());
}

#[test]
fn desktop_ui_snapshot_is_deterministic_1920x1080_and_embeds_viewport_and_tracks() {
    let mut ctrl = golden_controller();
    ctrl.scrub_to(Flicks(2 * FLICKS_PER_SECOND + FLICKS_PER_SECOND / 2));
    ctrl.set_active_profile(RenderProfileKind::Final);
    ctrl.select_node(Some(NodeId::from("whip_transition")))
        .unwrap();

    // Distinct bright magenta/teal viewport frame to verify real embedding.
    let mut viewport_a = Rgba8Frame::filled(
        320,
        180,
        ctrl.playhead_tick,
        [24, 180, 220, 255],
        AlphaMode::Straight,
    )
    .unwrap();
    for y in 40..140 {
        for x in 80..240 {
            viewport_a.set_pixel(x, y, [245, 70, 140, 255]);
        }
    }

    let snap_1 = ctrl.render_desktop_ui_snapshot(&viewport_a, 1920, 1080);
    let snap_2 = render_desktop_ui_snapshot(&ctrl, &viewport_a, 1920, 1080);

    assert_eq!(snap_1.width, 1920);
    assert_eq!(snap_1.height, 1080);
    assert_eq!(snap_1.pts, ctrl.playhead_tick);
    assert_eq!(snap_1.fnv1a_hash64(), snap_2.fnv1a_hash64());

    // Confirm center viewport area actually contains embedded magenta pixels from viewport_a.
    let center_px = snap_1.get_pixel(932, 350).unwrap();
    assert_eq!(center_px, [245, 70, 140, 255]);

    // Confirm changing viewport content changes the rendered snapshot.
    let viewport_b = Rgba8Frame::filled(
        320,
        180,
        ctrl.playhead_tick,
        [40, 220, 90, 255],
        AlphaMode::Straight,
    )
    .unwrap();
    let snap_b = ctrl.render_desktop_ui_snapshot(&viewport_b, 1920, 1080);
    assert_ne!(snap_1.fnv1a_hash64(), snap_b.fnv1a_hash64());

    // Confirm scrubbing playhead updates the timeline playhead & timecode in the snapshot.
    ctrl.scrub_to(Flicks(FLICKS_PER_SECOND));
    let snap_scrubbed = ctrl.render_desktop_ui_snapshot(&viewport_a, 1920, 1080);
    assert_ne!(snap_1.fnv1a_hash64(), snap_scrubbed.fnv1a_hash64());

    // Confirm non-blank timeline track pixels exist in bottom panel (e.g. clip_a blue bar).
    let mut found_blue_clip = false;
    for y in 700..900 {
        for x in 150..1200 {
            if snap_1.get_pixel(x, y) == Some([37, 99, 235, 255]) {
                found_blue_clip = true;
                break;
            }
        }
        if found_blue_clip {
            break;
        }
    }
    assert!(
        found_blue_clip,
        "expected video clip bar rendered in timeline"
    );
}
