//! Autocut boundary planning from beat analysis.

use crate::error::CoreError;
use frameshift_time::Flicks;
use serde::{Deserialize, Serialize};

/// Parameters for autocut planning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AutocutParams {
    /// Absolute timeline tick at which the planned window begins.
    pub start_tick: Flicks,
    /// Duration of the planned window. This is explicit because the analysis window is only
    /// the RMS sampling step and does not describe the analyzed audio or timeline duration.
    pub duration_ticks: Flicks,
    /// Minimum spacing between emitted boundaries, including the window start/end when enabled.
    pub min_segment_ticks: Flicks,
    pub include_start: bool,
    pub include_end: bool,
}

/// Plan autocut boundaries from beat analysis, within an explicit absolute timeline window.
///
/// Beat ticks are kept in their original time coordinate. Ticks outside
/// `[start_tick, start_tick + duration_ticks)` are ignored. Duplicate and unsorted beat ticks
/// are normalized before spacing is applied. Requested start and end boundaries are retained;
/// beat boundaries too close to a requested end boundary are dropped so the final segment can
/// satisfy `min_segment_ticks`.
pub fn plan_autocut(
    report: &frameshift_media::AudioAnalysisReport,
    params: AutocutParams,
) -> Result<Vec<Flicks>, CoreError> {
    let end_tick = params.start_tick.checked_add(params.duration_ticks)?;

    if params.start_tick.0 < 0 {
        return Err(CoreError::InvalidArgument(format!(
            "autocut start_tick must be non-negative, got {}",
            params.start_tick.0
        )));
    }
    if params.duration_ticks.0 <= 0 {
        return Err(CoreError::InvalidArgument(format!(
            "autocut duration_ticks must be positive, got {}",
            params.duration_ticks.0
        )));
    }
    if params.min_segment_ticks.0 < 0 {
        return Err(CoreError::InvalidArgument(format!(
            "autocut min_segment_ticks must be non-negative, got {}",
            params.min_segment_ticks.0
        )));
    }
    if params.include_start
        && params.include_end
        && params.min_segment_ticks.0 > params.duration_ticks.0
    {
        return Err(CoreError::InvalidArgument(format!(
            "autocut window duration ({}) is shorter than min_segment_ticks ({}) with both endpoints included",
            params.duration_ticks.0, params.min_segment_ticks.0
        )));
    }

    let mut beats: Vec<Flicks> = report
        .detected_beats
        .iter()
        .copied()
        .filter(|beat| beat.0 >= params.start_tick.0 && beat.0 < end_tick.0)
        .collect();
    beats.sort_unstable_by_key(|beat| beat.0);
    beats.dedup();

    let mut cuts = Vec::with_capacity(beats.len() + 2);
    if params.include_start {
        cuts.push(params.start_tick);
    }

    // The timeline start is the lower spacing bound even when it is not emitted. A beat exactly
    // at the start remains eligible when no positive minimum spacing is requested.
    let mut last_tick = params.start_tick;
    for beat in beats {
        if beat == params.start_tick && params.include_start {
            continue;
        }
        if beat.0 - last_tick.0 < params.min_segment_ticks.0 {
            continue;
        }
        if params.include_end && end_tick.0 - beat.0 < params.min_segment_ticks.0 {
            continue;
        }
        cuts.push(beat);
        last_tick = beat;
    }

    if params.include_end {
        cuts.push(end_tick);
    }

    cuts.sort_unstable_by_key(|tick| tick.0);
    cuts.dedup();
    Ok(cuts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use frameshift_media::AudioAnalysisReport;

    fn report(beats: &[i64]) -> AudioAnalysisReport {
        AudioAnalysisReport {
            // Analysis step is intentionally unrelated to the planner horizon.
            window_ticks: Flicks(7),
            rms_envelope: vec![],
            detected_beats: beats.iter().copied().map(Flicks).collect(),
            estimated_bpm: 120.0,
        }
    }

    fn params() -> AutocutParams {
        AutocutParams {
            start_tick: Flicks(0),
            duration_ticks: Flicks(100),
            min_segment_ticks: Flicks(0),
            include_start: false,
            include_end: false,
        }
    }

    #[test]
    fn includes_zero_beat_and_uses_explicit_horizon() {
        let cuts = plan_autocut(&report(&[0, 25, 75, 100, 125]), params()).unwrap();
        assert_eq!(cuts, vec![Flicks(0), Flicks(25), Flicks(75)]);
    }

    #[test]
    fn empty_beats_follow_start_and_end_flags() {
        assert_eq!(plan_autocut(&report(&[]), params()).unwrap(), vec![]);

        let mut options = params();
        options.include_start = true;
        assert_eq!(
            plan_autocut(&report(&[]), options).unwrap(),
            vec![Flicks(0)]
        );

        options.include_start = false;
        options.include_end = true;
        assert_eq!(
            plan_autocut(&report(&[]), options).unwrap(),
            vec![Flicks(100)]
        );

        options.include_start = true;
        assert_eq!(
            plan_autocut(&report(&[]), options).unwrap(),
            vec![Flicks(0), Flicks(100)]
        );
    }

    #[test]
    fn sorts_deduplicates_and_applies_spacing_around_endpoints() {
        let mut options = params();
        options.include_start = true;
        options.include_end = true;
        options.min_segment_ticks = Flicks(20);

        let cuts = plan_autocut(&report(&[70, 10, 20, 60, 30, 30, 0, 95]), options).unwrap();
        assert_eq!(cuts, vec![Flicks(0), Flicks(20), Flicks(60), Flicks(100)]);
    }

    #[test]
    fn plans_an_absolute_subrange_without_rebasing_beats() {
        let mut options = params();
        options.start_tick = Flicks(50);
        options.duration_ticks = Flicks(100);
        options.include_start = true;
        options.include_end = true;
        options.min_segment_ticks = Flicks(2);
        let cuts = plan_autocut(&report(&[0, 50, 75, 149, 150]), options).unwrap();
        assert_eq!(cuts, vec![Flicks(50), Flicks(75), Flicks(150)]);
    }

    #[test]
    fn rejects_nonpositive_horizon_and_impossible_endpoint_spacing() {
        let mut options = params();
        options.duration_ticks = Flicks(0);
        assert!(plan_autocut(&report(&[]), options).is_err());

        options.duration_ticks = Flicks(10);
        options.include_start = true;
        options.include_end = true;
        options.min_segment_ticks = Flicks(11);
        assert!(plan_autocut(&report(&[]), options).is_err());
    }
}
