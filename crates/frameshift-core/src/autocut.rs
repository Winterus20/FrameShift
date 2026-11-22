//! Autocut module for beat-synchronized segment planning.

use crate::error::CoreError;
use frameshift_time::Flicks;
use serde::{Deserialize, Serialize};

/// Parameters for autocut planning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AutocutParams {
    pub min_segment_ticks: Flicks,
    pub include_start: bool,
    pub include_end: bool,
}

/// Plan autocut points from beat analysis.
pub fn plan_autocut(report: &frameshift_media::AudioAnalysisReport, params: AutocutParams) -> Result<Vec<Flicks>, CoreError> {
    let beats = &report.detected_beats;
    let window_ticks = report.window_ticks;

    if beats.is_empty() {
        if params.include_end {
            Ok(vec![window_ticks])
        } else {
            Ok(Vec::new())
        }
    } else {
        let mut cuts: Vec<Flicks> = Vec::new();
        let mut last_tick = Flicks(0);

        if params.include_start {
            if beats.first().map_or(true, |b| b.0 != 0) {
                cuts.push(Flicks(0));
                last_tick = Flicks(0);
            }
        }

        for &beat_tick in beats {
            if beat_tick.0 <= last_tick.0 {
                continue;
            }
            if params.min_segment_ticks.0 > 0 && (beat_tick.0 - last_tick.0) < params.min_segment_ticks.0 {
                continue;
            }
            cuts.push(beat_tick);
            last_tick = beat_tick;
        }

        if params.include_end {
            if cuts.last().map_or(true, |last| last.0 < window_ticks.0) {
                cuts.push(window_ticks);
            }
        }

        cuts.sort_by(|a, b| a.0.cmp(&b.0));
        cuts.dedup();
        Ok(cuts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use frameshift_media::AudioAnalysisReport;
    use frameshift_time::Flicks;

    #[test]
    fn test_plan_autocut_with_beats() {
        let report = AudioAnalysisReport {
            window_ticks: Flicks(7620),
            rms_envelope: vec![],
            detected_beats: vec![Flicks(0), Flicks(1905), Flicks(3810), Flicks(5715)],
            estimated_bpm: 120.0,
        };
        let params = AutocutParams::default();
        let segments = plan_autocut(&report, params).unwrap();
        assert_eq!(segments.len(), 4);
    }

    #[test]
    fn test_plan_autocut_no_beats() {
        let report = AudioAnalysisReport {
            window_ticks: Flicks(7620),
            rms_envelope: vec![],
            detected_beats: vec![],
            estimated_bpm: 0.0,
        };
        let params = AutocutParams::default();
        let segments = plan_autocut(&report, params).unwrap();
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0], Flicks(7620));
    }
}
