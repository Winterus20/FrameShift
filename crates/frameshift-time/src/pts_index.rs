use crate::error::TimeError;
use crate::flicks::Flicks;
use crate::rational::Rational32;
use serde::{Deserialize, Serialize};

/// Result of looking up a `source_tick` in a `PtsIndex`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameMatch {
    /// Zero-based index of the active frame in the PTS table.
    pub frame_index: usize,
    /// Presentation timestamp of the matched frame.
    pub pts: Flicks,
    /// Presentation timestamp of the subsequent frame, if one exists.
    pub next_pts: Option<Flicks>,
    /// Normalized position within `[pts, next_pts)` in `[0.0, 1.0)`.
    /// Always `0.0` at or before the first frame, at or beyond the last frame,
    /// or when `next_pts == Some(pts)`.
    pub sub_frame_alpha: f64,
}

/// Deterministic presentation-timestamp (PTS) index for CFR and VFR media streams.
///
/// Built once during media probe/import without transcoding VFR streams.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PtsIndex {
    pts: Vec<Flicks>,
}

impl PtsIndex {
    /// Constructs a `PtsIndex` from a monotonically non-decreasing sequence of `Flicks` PTS values.
    pub fn new(pts: Vec<Flicks>) -> Result<Self, TimeError> {
        for i in 1..pts.len() {
            if pts[i] < pts[i - 1] {
                return Err(TimeError::UnsortedPts {
                    index: i,
                    prev: pts[i - 1],
                    current: pts[i],
                });
            }
        }
        Ok(Self { pts })
    }

    /// Generates a synthetic constant-frame-rate (CFR) `PtsIndex`.
    pub fn from_cfr(
        frame_count: usize,
        fps: Rational32,
        start_pts: Flicks,
    ) -> Result<Self, TimeError> {
        let mut pts = Vec::with_capacity(frame_count);
        for idx in 0..frame_count {
            let idx_i64 = i64::try_from(idx).map_err(|_| TimeError::Overflow)?;
            let offset = fps.frame_to_ticks(idx_i64)?;
            pts.push(start_pts.checked_add(offset)?);
        }
        Ok(Self { pts })
    }

    /// Looks up the active frame for `source_tick` using deterministic binary search.
    ///
    /// - Returns `None` if the index is empty.
    /// - For `source_tick <= first_pts`, clamps to frame `0` with `sub_frame_alpha = 0.0`.
    /// - For `pts[i] <= source_tick < pts[i + 1]`, returns frame `i` with
    ///   `sub_frame_alpha = (source_tick - pts[i]) / (pts[i + 1] - pts[i])`.
    /// - For `source_tick >= last_pts`, returns the final frame with `next_pts = None`
    ///   and `sub_frame_alpha = 0.0`.
    pub fn lookup_frame(&self, source_tick: Flicks) -> Option<FrameMatch> {
        if self.pts.is_empty() {
            return None;
        }
        if source_tick <= self.pts[0] {
            let pts = self.pts[0];
            let next_pts = self.pts.get(1).copied();
            return Some(FrameMatch {
                frame_index: 0,
                pts,
                next_pts,
                sub_frame_alpha: 0.0,
            });
        }

        // `partition_point(|&p| p <= source_tick)` returns the first index `pos`
        // where `pts[pos] > source_tick`. Since `source_tick > pts[0]`, `pos >= 1`.
        let pos = self.pts.partition_point(|&p| p <= source_tick);
        let frame_index = pos - 1;
        let pts = self.pts[frame_index];
        let next_pts = self.pts.get(frame_index + 1).copied();

        let sub_frame_alpha = match next_pts {
            Some(next) if next > pts => {
                let span = (next.0 - pts.0) as f64;
                let elapsed = (source_tick.0 - pts.0) as f64;
                (elapsed / span).clamp(0.0, 1.0)
            }
            _ => 0.0,
        };

        Some(FrameMatch {
            frame_index,
            pts,
            next_pts,
            sub_frame_alpha,
        })
    }

    /// Returns the number of indexed frames.
    #[inline]
    pub fn len(&self) -> usize {
        self.pts.len()
    }

    /// Returns `true` if no frames are indexed.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.pts.is_empty()
    }

    /// Returns the underlying sorted PTS slice.
    #[inline]
    pub fn as_slice(&self) -> &[Flicks] {
        &self.pts
    }

    /// Returns the first PTS, if present.
    #[inline]
    pub fn first_pts(&self) -> Option<Flicks> {
        self.pts.first().copied()
    }

    /// Returns the last PTS, if present.
    #[inline]
    pub fn last_pts(&self) -> Option<Flicks> {
        self.pts.last().copied()
    }
}
