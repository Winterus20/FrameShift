use crate::engine::collect_timeline_frame_ticks;
use crate::error::CoreError;
use frameshift_schema::Project;
use frameshift_time::Flicks;
use serde::{Deserialize, Serialize};

/// Half-open output frame range `[start_tick, end_tick)` snapped to the project frame grid.
///
/// `start_frame` is inclusive and `end_frame` is exclusive. Both ticks are exact frame
/// boundaries produced by `Rational32::frame_to_ticks`, so a range render always emits a
/// subset of the full timeline render's frames at the same ticks (seek independence).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderRange {
    /// First rendered frame start tick (frame-aligned).
    pub start_tick: Flicks,
    /// End tick, exclusive (frame-aligned).
    pub end_tick: Flicks,
    /// First rendered frame index (inclusive).
    pub start_frame: i64,
    /// One past the last rendered frame index (exclusive).
    pub end_frame: i64,
}

impl RenderRange {
    /// Resolves a requested `[start, end)` tick window against the project frame grid.
    ///
    /// Snapping is outward: `start` floors to its enclosing frame and `end` ceilings to the
    /// next frame boundary, so requested content is never dropped. `end` is clamped to the
    /// timeline duration; a `start` at or beyond the duration is an error.
    pub fn resolve(project: &Project, start: Flicks, end: Flicks) -> Result<Self, CoreError> {
        let duration = project.timeline.duration_ticks;
        if start.0 < 0 {
            return Err(CoreError::InvalidArgument(format!(
                "render range start must be non-negative, got {} flicks",
                start.0
            )));
        }
        if end <= start {
            return Err(CoreError::InvalidArgument(format!(
                "render range end ({} flicks) must be greater than start ({} flicks)",
                end.0, start.0
            )));
        }
        if start >= duration {
            return Err(CoreError::InvalidArgument(format!(
                "render range start ({} flicks) is at or beyond timeline duration ({} flicks)",
                start.0, duration.0
            )));
        }

        let fps = project.timeline.fps;
        let total_frames = i64::try_from(collect_timeline_frame_ticks(project)?.len())
            .map_err(|_| CoreError::InvalidArgument("timeline frame count exceeds i64".into()))?;

        let start_frame = fps.ticks_to_frame_floor(start)?;
        let clamped_end = if end > duration { duration } else { end };
        let end_frame = match fps.ticks_to_frame_exact(clamped_end)? {
            Some(index) => index,
            None => fps.ticks_to_frame_floor(clamped_end)? + 1,
        };
        let end_frame = end_frame.min(total_frames);
        if end_frame <= start_frame {
            return Err(CoreError::InvalidArgument(format!(
                "render range resolves to zero frames (start frame {start_frame}, end frame {end_frame}, timeline frames {total_frames})"
            )));
        }

        Ok(Self {
            start_tick: fps.frame_to_ticks(start_frame)?,
            end_tick: fps.frame_to_ticks(end_frame)?,
            start_frame,
            end_frame,
        })
    }

    /// Exact duration of the resolved range in Flicks.
    pub fn duration_ticks(&self) -> Flicks {
        Flicks(self.end_tick.0 - self.start_tick.0)
    }

    /// Number of frames in the resolved range.
    pub fn frame_count(&self) -> usize {
        usize::try_from((self.end_frame - self.start_frame).max(0)).unwrap_or(0)
    }

    /// True when the range covers every frame of a timeline with `total_frames` frames.
    pub fn covers_timeline(&self, total_frames: i64) -> bool {
        self.start_frame == 0 && self.end_frame >= total_frames
    }

    /// Frame start ticks for every frame in `[start_frame, end_frame)`.
    pub fn frame_ticks(&self, project: &Project) -> Result<Vec<Flicks>, CoreError> {
        let fps = project.timeline.fps;
        let mut ticks = Vec::with_capacity(self.frame_count());
        for index in self.start_frame..self.end_frame {
            ticks.push(fps.frame_to_ticks(index)?);
        }
        Ok(ticks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::collect_timeline_frame_ticks;
    use frameshift_time::{FLICKS_PER_SECOND, FPS_24, FPS_60, Rational32};

    fn project_at(fps: Rational32, duration_seconds: i64) -> Project {
        let mut project = Project::golden_scene_preset();
        project.timeline.fps = fps;
        project.timeline.duration_ticks = Flicks(duration_seconds * FLICKS_PER_SECOND);
        project
    }

    #[test]
    fn resolves_drop_window_to_ninety_frames_at_60fps() {
        let project = project_at(FPS_60, 20);
        let range = RenderRange::resolve(
            &project,
            Flicks(5_292_000_000),
            Flicks(6_350_400_000),
        )
        .expect("7.5s..9.0s resolves");
        assert_eq!(range.start_frame, 450);
        assert_eq!(range.end_frame, 540);
        assert_eq!(range.start_tick, Flicks(5_292_000_000));
        assert_eq!(range.end_tick, Flicks(6_350_400_000));
        assert_eq!(range.frame_count(), 90);
        assert_eq!(range.duration_ticks(), Flicks(1_058_400_000));
        assert!(!range.covers_timeline(1200));
    }

    #[test]
    fn snaps_outward_to_the_frame_grid() {
        let project = project_at(FPS_24, 2);
        // 1 tick floors to frame 0; one tick past frame 1 ceilings to frame 2.
        let range = RenderRange::resolve(&project, Flicks(1), Flicks(29_400_001))
            .expect("off-grid window snaps outward");
        assert_eq!((range.start_frame, range.end_frame), (0, 2));
        assert_eq!(range.start_tick, Flicks::ZERO);
        assert_eq!(range.end_tick, Flicks(58_800_000));
        assert_eq!(range.frame_count(), 2);
    }

    #[test]
    fn clamps_end_to_the_timeline_end() {
        let project = project_at(FPS_60, 20);
        let range = RenderRange::resolve(
            &project,
            Flicks(14_041_440_000),
            Flicks(25 * FLICKS_PER_SECOND),
        )
        .expect("end beyond duration clamps");
        assert_eq!(range.start_frame, 1194);
        assert_eq!(range.end_frame, 1200);
        assert_eq!(range.end_tick, Flicks(14_112_000_000));
        assert_eq!(range.frame_count(), 6);
    }

    #[test]
    fn covers_timeline_for_a_full_grid_request() {
        let project = project_at(FPS_60, 20);
        let range = RenderRange::resolve(
            &project,
            Flicks::ZERO,
            Flicks(20 * FLICKS_PER_SECOND),
        )
        .expect("full timeline range resolves");
        assert_eq!((range.start_frame, range.end_frame), (0, 1200));
        assert!(range.covers_timeline(1200));
        assert_eq!(range.duration_ticks(), Flicks(14_112_000_000));
    }

    #[test]
    fn rejects_negative_reversed_and_out_of_timeline_starts() {
        let project = project_at(FPS_60, 20);
        let duration = project.timeline.duration_ticks;
        assert!(
            RenderRange::resolve(&project, Flicks(-1), Flicks(FLICKS_PER_SECOND)).is_err(),
            "negative start must error"
        );
        assert!(
            RenderRange::resolve(
                &project,
                Flicks(FLICKS_PER_SECOND),
                Flicks(FLICKS_PER_SECOND)
            )
            .is_err(),
            "empty range must error"
        );
        assert!(
            RenderRange::resolve(
                &project,
                Flicks(2 * FLICKS_PER_SECOND),
                Flicks(FLICKS_PER_SECOND)
            )
            .is_err(),
            "reversed range must error"
        );
        assert!(
            RenderRange::resolve(
                &project,
                duration,
                Flicks(duration.0 + FLICKS_PER_SECOND)
            )
            .is_err(),
            "start at the timeline end must error"
        );
    }

    #[test]
    fn frame_ticks_are_the_full_sequence_slice() {
        let project = project_at(FPS_60, 20);
        let all = collect_timeline_frame_ticks(&project).expect("full ticks");
        assert_eq!(all.len(), 1200);
        let range = RenderRange::resolve(
            &project,
            Flicks(5_292_000_000),
            Flicks(6_350_400_000),
        )
        .expect("range");
        let subset = range.frame_ticks(&project).expect("range ticks");
        assert_eq!(subset.len(), 90);
        assert_eq!(subset.as_slice(), &all[450..540]);
    }

    #[test]
    fn range_round_trips_through_json() {
        let project = project_at(FPS_60, 20);
        let range = RenderRange::resolve(
            &project,
            Flicks(5_292_000_000),
            Flicks(6_350_400_000),
        )
        .expect("range");
        let json = serde_json::to_string(&range).expect("serialize range");
        assert!(json.contains("\"start_frame\":450"));
        assert!(json.contains("\"end_frame\":540"));
        let back: RenderRange = serde_json::from_str(&json).expect("deserialize range");
        assert_eq!(back, range);
    }

    #[test]
    fn legacy_summary_json_without_range_deserializes() {
        let json = r#"{
            "out_path": "out.mp4",
            "profile": "draft",
            "width": 64,
            "height": 36,
            "frame_count": 1,
            "fps": { "num": 60, "den": 1 },
            "duration_ticks": 100,
            "has_audio": false,
            "frame_hashes": [1],
            "cache_hits": 0,
            "cache_misses": 0,
            "cache_evictions": 0
        }"#;
        let summary: crate::report::VideoRenderSummary =
            serde_json::from_str(json).expect("legacy summary");
        assert_eq!(summary.range, None);
        let encoded = serde_json::to_string(&summary).expect("serialize summary");
        assert!(!encoded.contains("\"range\""));
    }
}
