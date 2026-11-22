//! Tests for the pure autocut planner (beat ticks plus assets to hard cuts).

use frameshift_schema::session::{plan_autocut, AutocutAsset, AutocutOptions};
use frameshift_schema::{HardCut, SchemaError};
use frameshift_time::{Flicks, Rational32, FLICKS_PER_SECOND};

const FRAME_60: i64 = FLICKS_PER_SECOND / 60;
const FRAME_30: i64 = FLICKS_PER_SECOND / 30;

fn sec(value: i64) -> Flicks {
    Flicks(FLICKS_PER_SECOND * value)
}

fn ticks(value: i64) -> Flicks {
    Flicks(value)
}

fn beat_list(count: usize, interval: Flicks) -> Vec<Flicks> {
    (0..count)
        .map(|index| Flicks(interval.as_i64() * index as i64))
        .collect()
}

fn asset(asset_id: &str, duration: Flicks, source_start: Flicks) -> AutocutAsset {
    AutocutAsset {
        asset_id: asset_id.into(),
        duration_ticks: duration,
        source_start,
    }
}

fn base_options() -> AutocutOptions {
    AutocutOptions {
        start_tick: Flicks::ZERO,
        end_tick: sec(4),
        cuts_per_beat: Rational32::new(1, 1).expect("1/1 is a valid ratio"),
        min_spacing_ticks: Flicks::ZERO,
        max_cuts: None,
        offset_ticks: Flicks::ZERO,
        fps: Rational32::new(60, 1).expect("60 fps is valid"),
    }
}

fn options_with(stage: usize, start: Flicks, end: Flicks) -> AutocutOptions {
    let mut options = base_options();
    options.start_tick = start;
    options.end_tick = end;
    options.cuts_per_beat = Rational32::new(stage as u32, 1).expect("integer ratio");
    options
}

fn assert_contiguous(cuts: &[HardCut], window_start: Flicks, window_end: Flicks) {
    assert!(!cuts.is_empty(), "plan must contain at least one cut");
    assert_eq!(cuts[0].timeline_start, window_start);
    assert_eq!(cuts[cuts.len() - 1].timeline_end, window_end);
    for pair in cuts.windows(2) {
        assert_eq!(
            pair[0].timeline_end, pair[1].timeline_start,
            "cuts must be ordered and non-overlapping"
        );
    }
    for cut in cuts {
        assert!(
            cut.timeline_start < cut.timeline_end,
            "cut '{}' must have positive length",
            cut.clip_id
        );
    }
}

fn assert_on_frame_grid(cuts: &[HardCut], fps: Rational32) {
    for cut in cuts {
        assert!(
            fps.ticks_to_frame_exact(cut.timeline_start)
                .expect("frame conversion")
                .is_some(),
            "cut '{}' start {} is off the frame grid",
            cut.clip_id,
            cut.timeline_start.as_i64()
        );
        assert!(
            fps.ticks_to_frame_exact(cut.timeline_end)
                .expect("frame conversion")
                .is_some(),
            "cut '{}' end {} is off the frame grid",
            cut.clip_id,
            cut.timeline_end.as_i64()
        );
    }
}

#[test]
fn one_cut_per_beat_yields_one_cut_per_beat() {
    let interval = Flicks(FLICKS_PER_SECOND / 2);
    let beats = beat_list(8, interval);
    let assets = vec![asset("asset_a", sec(30), Flicks::ZERO)];
    let options = base_options();

    let plan = plan_autocut(&beats, &assets, &options).expect("plan must succeed");

    assert_eq!(plan.cuts.len(), 8);
    assert_eq!(plan.dropped_beats, 0);
    assert_eq!(plan.warnings.len(), 0);
    assert_contiguous(&plan.cuts, Flicks::ZERO, sec(4));
    assert_on_frame_grid(&plan.cuts, options.fps);
    for cut in &plan.cuts {
        assert_eq!(cut.timeline_end.as_i64() - cut.timeline_start.as_i64(), interval.as_i64());
        assert_eq!(cut.asset_id, "asset_a");
        assert!(cut.speed_curve.is_none());
    }
    assert_eq!(plan.cuts[0].clip_id, "autocut_0000");
    assert_eq!(plan.cuts[7].clip_id, "autocut_0007");
}

#[test]
fn two_cuts_per_beat_doubles_the_plan() {
    let interval = Flicks(FLICKS_PER_SECOND / 2);
    let beats = beat_list(8, interval);
    let assets = vec![asset("asset_a", sec(30), Flicks::ZERO)];
    let plan = plan_autocut(&beats, &assets, &options_with(2, Flicks::ZERO, sec(4)))
        .expect("plan must succeed");

    assert_eq!(plan.cuts.len(), 16);
    assert_eq!(plan.dropped_beats, 0);
    assert_contiguous(&plan.cuts, Flicks::ZERO, sec(4));
    assert_on_frame_grid(&plan.cuts, Rational32::new(60, 1).unwrap());
    for (index, cut) in plan.cuts.iter().enumerate() {
        let expected = Flicks(interval.as_i64() * index as i64 / 2);
        assert_eq!(cut.timeline_start, expected, "cut {index}");
    }
}

#[test]
fn half_and_quarter_cut_rates_stretch_the_grid() {
    let interval = Flicks(FLICKS_PER_SECOND / 2);
    let beats = beat_list(8, interval);
    let assets = vec![asset("asset_a", sec(30), Flicks::ZERO)];

    let mut half = base_options();
    half.cuts_per_beat = Rational32::new(1, 2).unwrap();
    let half_plan = plan_autocut(&beats, &assets, &half).expect("1/2 plan");
    assert_eq!(half_plan.cuts.len(), 4);
    assert_eq!(half_plan.cuts[0].timeline_start, Flicks::ZERO);
    assert_eq!(half_plan.cuts[0].timeline_end, sec(1));
    assert_eq!(half_plan.cuts[3].timeline_start, sec(3));
    assert_eq!(half_plan.cuts[3].timeline_end, sec(4));

    let mut quarter = base_options();
    quarter.cuts_per_beat = Rational32::new(1, 4).unwrap();
    let quarter_plan = plan_autocut(&beats, &assets, &quarter).expect("1/4 plan");
    assert_eq!(quarter_plan.cuts.len(), 2);
    assert_eq!(quarter_plan.cuts[0].timeline_end, sec(2));
    assert_eq!(quarter_plan.cuts[1].timeline_start, sec(2));
    assert_eq!(quarter_plan.cuts[1].timeline_end, sec(4));
}

#[test]
fn boundaries_are_quantized_to_the_frame_grid() {
    let fps = Rational32::new(60, 1).unwrap();
    let interval = Flicks(FLICKS_PER_SECOND / 2 + 1);
    let beats = beat_list(6, interval);
    let assets = vec![asset("asset_a", sec(30), Flicks::ZERO)];

    let start_tick = ticks(1);
    let end_tick = ticks(interval.as_i64() * 6 + 7);
    let options = options_with(1, start_tick, end_tick);
    let plan = plan_autocut(&beats, &assets, &options).expect("plan must succeed");

    assert_eq!(plan.cuts.len(), 6);
    assert_eq!(plan.dropped_beats, 0);
    assert_on_frame_grid(&plan.cuts, fps);
    assert_eq!(plan.cuts[0].timeline_start, ticks(FRAME_60));
    assert_eq!(
        plan.cuts[plan.cuts.len() - 1].timeline_end,
        ticks(FRAME_60 * 180)
    );
    for cut in &plan.cuts {
        assert!(
            cut.timeline_start.as_i64() % FRAME_60 == 0,
            "boundary {} is not a frame multiple",
            cut.timeline_start.as_i64()
        );
    }
    for (index, beat) in beats.iter().skip(1).enumerate() {
        let frame = fps.ticks_to_frame_nearest(*beat).unwrap();
        let expected = fps.frame_to_ticks(frame).unwrap();
        assert_eq!(plan.cuts[index + 1].timeline_start, expected, "beat {index}");
    }
    assert!(plan.cuts[0].timeline_start >= start_tick);
    assert!(plan.cuts[plan.cuts.len() - 1].timeline_end <= end_tick);
}

#[test]
fn min_spacing_drops_crowded_boundaries() {
    let interval = Flicks(FLICKS_PER_SECOND / 2);
    let beats = beat_list(8, interval);
    let assets = vec![asset("asset_a", sec(30), Flicks::ZERO)];
    let mut options = base_options();
    options.min_spacing_ticks = ticks(FLICKS_PER_SECOND * 12 / 10);

    let plan = plan_autocut(&beats, &assets, &options).expect("plan must succeed");

    assert_eq!(plan.cuts.len(), 3);
    assert_eq!(plan.dropped_beats, 5);
    assert_eq!(plan.cuts[0].timeline_start, Flicks::ZERO);
    assert_eq!(plan.cuts[0].timeline_end, sec(1) + interval);
    assert_eq!(plan.cuts[1].timeline_start, sec(1) + interval);
    assert_eq!(plan.cuts[1].timeline_end, sec(3));
    assert_eq!(plan.cuts[2].timeline_end, sec(4));
    assert_contiguous(&plan.cuts, Flicks::ZERO, sec(4));
    assert!(
        plan.warnings.iter().any(|warning| warning.contains("min_spacing")),
        "drops must be reported: {:?}",
        plan.warnings
    );
}

#[test]
fn max_cuts_truncates_the_plan_and_keeps_the_window_covered() {
    let interval = Flicks(FLICKS_PER_SECOND / 2);
    let beats = beat_list(8, interval);
    let assets = vec![asset("asset_a", sec(30), Flicks::ZERO)];
    let mut options = base_options();
    options.max_cuts = Some(3);

    let plan = plan_autocut(&beats, &assets, &options).expect("plan must succeed");

    assert_eq!(plan.cuts.len(), 3);
    assert_eq!(plan.dropped_beats, 5);
    assert_eq!(plan.cuts[0].timeline_start, Flicks::ZERO);
    assert_eq!(plan.cuts[1].timeline_start, interval);
    assert_eq!(plan.cuts[2].timeline_start, sec(1));
    assert_eq!(plan.cuts[2].timeline_end, sec(4));
    assert!(
        plan.warnings.iter().any(|warning| warning.contains("max_cuts")),
        "truncation must be reported: {:?}",
        plan.warnings
    );
}

#[test]
fn assets_advance_round_robin_and_wrap_cursors() {
    let beats = beat_list(6, sec(1));
    let assets = vec![
        asset("asset_a", sec(10), Flicks::ZERO),
        asset("asset_b", sec(3), sec(1)),
    ];
    let mut options = base_options();
    options.end_tick = sec(6);
    let plan = plan_autocut(&beats, &assets, &options).expect("plan must succeed");

    assert_eq!(plan.cuts.len(), 6);
    let asset_ids: Vec<&str> = plan.cuts.iter().map(|cut| cut.asset_id.as_str()).collect();
    assert_eq!(
        asset_ids,
        ["asset_a", "asset_b", "asset_a", "asset_b", "asset_a", "asset_b"]
    );
    let sources: Vec<i64> = plan
        .cuts
        .iter()
        .map(|cut| cut.source_start.as_i64())
        .collect();
    assert_eq!(
        sources,
        [
            0,
            sec(1).as_i64(),
            sec(1).as_i64(),
            sec(2).as_i64(),
            sec(2).as_i64(),
            sec(1).as_i64()
        ],
        "cursor must advance per asset and wrap back to source_start"
    );
}

#[test]
fn offset_shifts_every_beat_boundary() {
    let interval = Flicks(FLICKS_PER_SECOND / 2);
    let beats = beat_list(8, interval);
    let assets = vec![asset("asset_a", sec(30), Flicks::ZERO)];
    let mut options = base_options();
    options.offset_ticks = ticks(FLICKS_PER_SECOND / 10);

    let plan = plan_autocut(&beats, &assets, &options).expect("plan must succeed");

    assert_eq!(plan.cuts.len(), 9);
    assert_eq!(plan.cuts[0].timeline_start, Flicks::ZERO);
    assert_eq!(plan.cuts[0].timeline_end, ticks(FLICKS_PER_SECOND / 10));
    assert_eq!(plan.cuts[1].timeline_start, ticks(FLICKS_PER_SECOND / 10));
    assert_eq!(
        plan.cuts[2].timeline_start,
        ticks(FLICKS_PER_SECOND * 6 / 10)
    );
    assert_eq!(plan.cuts[8].timeline_start, ticks(FLICKS_PER_SECOND * 36 / 10));
    assert_eq!(plan.cuts[8].timeline_end, sec(4));
}

#[test]
fn plan_stays_ordered_inside_the_window_and_respects_frame_minimum() {
    let interval = Flicks(FLICKS_PER_SECOND * 3 / 10);
    let beats = beat_list(20, interval);
    let assets = vec![asset("asset_a", sec(60), Flicks::ZERO)];
    let mut options = options_with(2, sec(1), sec(5));
    options.fps = Rational32::new(30, 1).unwrap();

    let plan = plan_autocut(&beats, &assets, &options).expect("plan must succeed");

    assert!(!plan.cuts.is_empty());
    assert_contiguous(&plan.cuts, sec(1), sec(5));
    assert_on_frame_grid(&plan.cuts, options.fps);
    for cut in &plan.cuts {
        assert!(cut.timeline_start >= sec(1));
        assert!(cut.timeline_end <= sec(5));
        assert!(
            cut.timeline_end.as_i64() - cut.timeline_start.as_i64() >= FRAME_30,
            "cut '{}' is shorter than one frame",
            cut.clip_id
        );
    }
}

#[test]
fn identical_inputs_produce_identical_plans() {
    let interval = Flicks(FLICKS_PER_SECOND / 2);
    let beats = beat_list(8, interval);
    let assets = vec![
        asset("asset_a", sec(10), Flicks::ZERO),
        asset("asset_b", sec(3), sec(1)),
    ];
    let mut options = base_options();
    options.cuts_per_beat = Rational32::new(2, 1).unwrap();
    options.max_cuts = Some(10);
    options.offset_ticks = ticks(FLICKS_PER_SECOND / 20);

    let first = plan_autocut(&beats, &assets, &options).expect("first plan");
    let second = plan_autocut(&beats, &assets, &options).expect("second plan");

    assert_eq!(first, second);
    assert_eq!(
        serde_json::to_string(&first.cuts).unwrap(),
        serde_json::to_string(&second.cuts).unwrap()
    );
}

#[test]
fn empty_inputs_reversed_windows_and_bad_options_are_rejected() {
    let beats = beat_list(4, sec(1));
    let assets = vec![asset("asset_a", sec(4), Flicks::ZERO)];
    let good = base_options();

    let empty_beats = plan_autocut(&[], &assets, &good);
    assert!(matches!(empty_beats, Err(SchemaError::InvalidTimeline(_))));

    let empty_assets = plan_autocut(&beats, &[], &good);
    assert!(matches!(empty_assets, Err(SchemaError::InvalidTimeline(_))));

    let mut equal_window = good.clone();
    equal_window.start_tick = sec(2);
    equal_window.end_tick = sec(2);
    assert!(matches!(
        plan_autocut(&beats, &assets, &equal_window),
        Err(SchemaError::InvalidTimeline(_))
    ));

    let mut reversed = good.clone();
    reversed.start_tick = sec(4);
    reversed.end_tick = Flicks::ZERO;
    assert!(matches!(
        plan_autocut(&beats, &assets, &reversed),
        Err(SchemaError::InvalidTimeline(_))
    ));

    let mut too_narrow = good.clone();
    too_narrow.start_tick = Flicks::ZERO;
    too_narrow.end_tick = ticks(1);
    assert!(
        matches!(
            plan_autocut(&beats, &assets, &too_narrow),
            Err(SchemaError::InvalidTimeline(_))
        ),
        "a window narrower than one frame must fail"
    );

    let mut unsupported = good.clone();
    unsupported.cuts_per_beat = Rational32::new(3, 1).unwrap();
    assert!(matches!(
        plan_autocut(&beats, &assets, &unsupported),
        Err(SchemaError::InvalidTimeline(_))
    ));

    let mut zero_ratio = good.clone();
    zero_ratio.cuts_per_beat = Rational32 { num: 0, den: 1 };
    assert!(matches!(
        plan_autocut(&beats, &assets, &zero_ratio),
        Err(SchemaError::InvalidTimeline(_))
    ));

    let mut negative_spacing = good.clone();
    negative_spacing.min_spacing_ticks = ticks(-1);
    assert!(matches!(
        plan_autocut(&beats, &assets, &negative_spacing),
        Err(SchemaError::InvalidTimeline(_))
    ));

    let mut zero_cuts = good.clone();
    zero_cuts.max_cuts = Some(0);
    assert!(matches!(
        plan_autocut(&beats, &assets, &zero_cuts),
        Err(SchemaError::InvalidTimeline(_))
    ));

    let short_assets = vec![asset("asset_a", Flicks::ZERO, Flicks::ZERO)];
    assert!(matches!(
        plan_autocut(&beats, &short_assets, &good),
        Err(SchemaError::InvalidTimeline(_))
    ));

    let bad_in_point = vec![asset("asset_a", sec(4), sec(4))];
    assert!(matches!(
        plan_autocut(&beats, &bad_in_point, &good),
        Err(SchemaError::InvalidTimeline(_))
    ));

    let unsorted_beats = vec![Flicks::ZERO, sec(2), sec(1)];
    assert!(matches!(
        plan_autocut(&unsorted_beats, &assets, &good),
        Err(SchemaError::InvalidTimeline(_))
    ));
}
