use crate::flicks::Flicks;
use crate::speed_curve::{
    BezierSpeedKeyframe, KeyframeEase, SpeedCurve, SpeedCurveSpec, SpeedKeyframe,
};
use serde::{Deserialize, Serialize};
use std::fmt;

const fn default_anime_snap_peak_bias() -> f64 {
    0.85
}

const fn default_anime_snap_influence() -> f64 {
    0.80
}

/// Curve shape requested from `ImpactSolver`.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImpactCurveStyle {
    /// Two piecewise-constant segments (`pre_speed` then `post_speed`).
    #[default]
    TwoSegmentConstant,
    /// Smooth C1 `CubicHermite` curve with zero-slope knots that hits the exact
    /// pre-impact and post-impact integrals without exceeding `[min_speed, max_speed]`.
    SmoothRamp,
    /// Anime-edit / AMV speed-ramp snap: builds tension before the impact and spikes
    /// into a high speed peak around the impact (`peak_bias`), then drops into slow-mo
    /// afterward while keeping pre- and post-impact integrals locked to `source_impact`
    /// and staying within `[min_speed, max_speed]`.
    AnimeSnap {
        #[serde(default = "default_anime_snap_peak_bias")]
        peak_bias: f64,
        #[serde(default = "default_anime_snap_influence")]
        influence: f64,
    },
}

impl ImpactCurveStyle {
    /// Builds an anime snap-in preset (slow anticipation followed by a steep whip-up peak into impact).
    pub const fn anime_snap_in() -> Self {
        Self::AnimeSnap {
            peak_bias: 0.85,
            influence: 0.80,
        }
    }

    /// Builds an anime snap-out preset (sharp speed burst coming out of impact before dropping to slow-mo).
    pub const fn anime_snap_out() -> Self {
        Self::AnimeSnap {
            peak_bias: 0.15,
            influence: 0.80,
        }
    }

    /// Builds a balanced high-influence Twixtor-style speed ramp centered on the impact.
    pub const fn twixtor_ramp() -> Self {
        Self::AnimeSnap {
            peak_bias: 0.50,
            influence: 0.90,
        }
    }
}

/// Preset helper producing an anime snap-in `ImpactCurveStyle`.
pub const fn anime_snap_in() -> ImpactCurveStyle {
    ImpactCurveStyle::anime_snap_in()
}

/// Preset helper producing an anime snap-out `ImpactCurveStyle`.
pub const fn anime_snap_out() -> ImpactCurveStyle {
    ImpactCurveStyle::anime_snap_out()
}

/// Preset helper producing a Twixtor-style speed ramp `ImpactCurveStyle`.
pub const fn twixtor_ramp() -> ImpactCurveStyle {
    ImpactCurveStyle::twixtor_ramp()
}

/// Controls whether the solver must consume the exact `[source_bounds_start, source_bounds_end]`
/// span or may trim unused head/tail within those source bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceFitMode {
    /// Consumes the exact range from `source_bounds_start` to `source_bounds_end`.
    #[default]
    ExactRange,
    /// Allows trimming excess source head/tail inside `[source_bounds_start, source_bounds_end]`
    /// if `max_speed` would otherwise be exceeded, while never expanding outside bounds.
    FitWithinBounds,
}

/// Input request for `ImpactSolver::solve` (ADR-0003).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImpactSolveRequest {
    pub source_bounds_start: Flicks,
    pub source_bounds_end: Flicks,
    pub source_impact: Flicks,
    pub output_start: Flicks,
    pub output_end: Flicks,
    pub target_output: Flicks,
    pub min_speed: f64,
    pub max_speed: f64,
    #[serde(default)]
    pub style: ImpactCurveStyle,
    #[serde(default)]
    pub fit_mode: SourceFitMode,
}

/// Which segment of the clip triggered a constraint violation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentSide {
    PreImpact,
    PostImpact,
    ClipBounds,
}

/// Specific constraint violated during impact alignment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConstraintFailure {
    InvalidSourceBounds {
        source_bounds_start: Flicks,
        source_impact: Flicks,
        source_bounds_end: Flicks,
    },
    InvalidOutputBounds {
        output_start: Flicks,
        target_output: Flicks,
        output_end: Flicks,
    },
    InvalidSpeedBounds {
        min_speed: f64,
        max_speed: f64,
    },
    /// Output segment has zero duration while corresponding source span is non-zero.
    ZeroOutputSpanWithNonZeroSource {
        segment: SegmentSide,
        required_source_span: Flicks,
    },
    /// Average speed required for the segment falls below `min_speed` (insufficient source frames).
    MinSpeedViolated {
        segment: SegmentSide,
        required_speed: f64,
        min_speed: f64,
        available_source: Flicks,
        required_source_at_min_speed: Flicks,
    },
    /// Average speed required for the segment exceeds `max_speed` (too much source for output span).
    MaxSpeedViolated {
        segment: SegmentSide,
        required_speed: f64,
        max_speed: f64,
        available_source: Flicks,
        max_consumable_source: Flicks,
    },
}

/// Concrete suggested alternative returned when constraints cannot be satisfied.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SuggestedAlternative {
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_target_output: Option<Flicks>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_min_speed: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_max_speed: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_output_start: Option<Flicks>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_output_end: Option<Flicks>,
}

/// Structured error returned by `ImpactSolver` when constraints are impossible to satisfy.
///
/// Per ADR-0003, the engine NEVER silently alters bounds or clamps speed outside `[min_speed, max_speed]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConstraintViolation {
    pub failures: Vec<ConstraintFailure>,
    pub suggestions: Vec<SuggestedAlternative>,
}

impl fmt::Display for ConstraintViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} constraint failure(s); {} suggested alternative(s)",
            self.failures.len(),
            self.suggestions.len()
        )?;
        if let Some(first) = self.suggestions.first() {
            write!(f, " (e.g. {})", first.description)?;
        }
        Ok(())
    }
}

impl std::error::Error for ConstraintViolation {}

/// Successful deterministic alignment result from `ImpactSolver`.
#[derive(Debug, Clone, PartialEq)]
pub struct ImpactSolution {
    pub speed_curve: SpeedCurve,
    pub effective_source_start: Flicks,
    pub effective_source_end: Flicks,
    pub pre_impact_avg_speed: f64,
    pub post_impact_avg_speed: f64,
    pub achieved_source_at_target: Flicks,
}

/// Deterministic solver for ADR-0003 semantic impact-to-beat alignment.
pub struct ImpactSolver;

impl ImpactSolver {
    /// Aligns `source_impact` to `target_output` within `[min_speed, max_speed]` and source bounds.
    ///
    /// Returns `Err(ConstraintViolation)` without silently altering bounds if no valid curve exists.
    pub fn solve(req: &ImpactSolveRequest) -> Result<ImpactSolution, ConstraintViolation> {
        let mut failures = Vec::new();
        let mut suggestions = Vec::new();

        if !req.min_speed.is_finite()
            || !req.max_speed.is_finite()
            || req.min_speed <= 0.0
            || req.min_speed > req.max_speed
        {
            failures.push(ConstraintFailure::InvalidSpeedBounds {
                min_speed: req.min_speed,
                max_speed: req.max_speed,
            });
            suggestions.push(SuggestedAlternative {
                description: "Provide finite positive speeds with 0 < min_speed <= max_speed".into(),
                suggested_target_output: None,
                suggested_min_speed: Some(0.25),
                suggested_max_speed: Some(4.0),
                suggested_output_start: None,
                suggested_output_end: None,
            });
        }

        if req.source_impact < req.source_bounds_start || req.source_impact > req.source_bounds_end {
            failures.push(ConstraintFailure::InvalidSourceBounds {
                source_bounds_start: req.source_bounds_start,
                source_impact: req.source_impact,
                source_bounds_end: req.source_bounds_end,
            });
            suggestions.push(SuggestedAlternative {
                description: "Ensure source_bounds_start <= source_impact <= source_bounds_end".into(),
                suggested_target_output: None,
                suggested_min_speed: None,
                suggested_max_speed: None,
                suggested_output_start: None,
                suggested_output_end: None,
            });
        }

        if req.target_output < req.output_start
            || req.target_output > req.output_end
            || req.output_end <= req.output_start
        {
            failures.push(ConstraintFailure::InvalidOutputBounds {
                output_start: req.output_start,
                target_output: req.target_output,
                output_end: req.output_end,
            });
            suggestions.push(SuggestedAlternative {
                description: "Ensure output_start <= target_output <= output_end with non-zero clip duration".into(),
                suggested_target_output: None,
                suggested_min_speed: None,
                suggested_max_speed: None,
                suggested_output_start: None,
                suggested_output_end: None,
            });
        }

        if let ImpactCurveStyle::AnimeSnap {
            peak_bias,
            influence,
        } = req.style
            && (!peak_bias.is_finite()
                || !(0.0..=1.0).contains(&peak_bias)
                || !influence.is_finite()
                || !(0.0..=1.0).contains(&influence))
        {
            failures.push(ConstraintFailure::InvalidSpeedBounds {
                min_speed: req.min_speed,
                max_speed: req.max_speed,
            });
            suggestions.push(SuggestedAlternative {
                description:
                    "AnimeSnap requires finite peak_bias and influence in [0.0, 1.0]".into(),
                suggested_target_output: None,
                suggested_min_speed: None,
                suggested_max_speed: None,
                suggested_output_start: None,
                suggested_output_end: None,
            });
        }

        if !failures.is_empty() {
            return Err(ConstraintViolation {
                failures,
                suggestions,
            });
        }

        let dt_pre = req.target_output - req.output_start;
        let dt_post = req.output_end - req.target_output;
        let ds_pre_avail = req.source_impact - req.source_bounds_start;
        let ds_post_avail = req.source_bounds_end - req.source_impact;

        let (effective_source_start, v_pre) = check_segment(
            SegmentSide::PreImpact,
            dt_pre,
            ds_pre_avail,
            req,
            true,
            &mut failures,
            &mut suggestions,
        );

        let (effective_source_end, v_post) = check_segment(
            SegmentSide::PostImpact,
            dt_post,
            ds_post_avail,
            req,
            false,
            &mut failures,
            &mut suggestions,
        );

        if let ImpactCurveStyle::AnimeSnap {
            peak_bias,
            influence,
        } = req.style
            && (!peak_bias.is_finite()
                || !(0.0..=1.0).contains(&peak_bias)
                || !influence.is_finite()
                || !(0.0..=1.0).contains(&influence))
        {
            failures.push(ConstraintFailure::InvalidSpeedBounds {
                min_speed: req.min_speed,
                max_speed: req.max_speed,
            });
            suggestions.push(SuggestedAlternative {
                description:
                    "AnimeSnap requires finite peak_bias and influence in [0.0, 1.0]".into(),
                suggested_target_output: None,
                suggested_min_speed: None,
                suggested_max_speed: None,
                suggested_output_start: None,
                suggested_output_end: None,
            });
        }

        if !failures.is_empty() {
            return Err(ConstraintViolation {
                failures,
                suggestions,
            });
        }

        let speed_curve = build_solution_curve(dt_pre, dt_post, v_pre, v_post, req)?;
        let achieved_offset = speed_curve.source_offset_at(dt_pre);
        let achieved_source_at_target = effective_source_start + achieved_offset;

        Ok(ImpactSolution {
            speed_curve,
            effective_source_start,
            effective_source_end,
            pre_impact_avg_speed: v_pre,
            post_impact_avg_speed: v_post,
            achieved_source_at_target,
        })
    }
}

fn check_segment(
    side: SegmentSide,
    dt: Flicks,
    ds_avail: Flicks,
    req: &ImpactSolveRequest,
    is_pre: bool,
    failures: &mut Vec<ConstraintFailure>,
    suggestions: &mut Vec<SuggestedAlternative>,
) -> (Flicks, f64) {
    if dt == Flicks::ZERO {
        if ds_avail == Flicks::ZERO || req.fit_mode == SourceFitMode::FitWithinBounds {
            let eff = req.source_impact;
            let default_speed = req.min_speed.max(1.0).min(req.max_speed);
            return (eff, default_speed);
        }
        failures.push(ConstraintFailure::ZeroOutputSpanWithNonZeroSource {
            segment: side,
            required_source_span: ds_avail,
        });
        let suggested_target = if is_pre {
            Flicks(req.output_start.0 + (ds_avail.0 as f64 / req.max_speed).ceil() as i64)
        } else {
            Flicks(req.output_end.0 - (ds_avail.0 as f64 / req.max_speed).ceil() as i64)
        };
        suggestions.push(SuggestedAlternative {
            description: format!(
                "{side:?} output span is zero while {ds_avail} of source remains; move target_output to {suggested_target}"
            ),
            suggested_target_output: Some(suggested_target),
            suggested_min_speed: None,
            suggested_max_speed: None,
            suggested_output_start: None,
            suggested_output_end: None,
        });
        return (req.source_impact, 0.0);
    }

    let required_speed = ds_avail.0 as f64 / dt.0 as f64;
    let min_source_needed = Flicks((dt.0 as f64 * req.min_speed).ceil() as i64);
    let max_source_consumable = Flicks((dt.0 as f64 * req.max_speed).floor() as i64);

    if required_speed < req.min_speed {
        failures.push(ConstraintFailure::MinSpeedViolated {
            segment: side,
            required_speed,
            min_speed: req.min_speed,
            available_source: ds_avail,
            required_source_at_min_speed: min_source_needed,
        });

        let max_dt_at_min_speed = (ds_avail.0 as f64 / req.min_speed).floor() as i64;
        if is_pre {
            let suggested_target = Flicks(req.output_start.0 + max_dt_at_min_speed);
            let suggested_out_start = Flicks(req.target_output.0 - max_dt_at_min_speed);
            suggestions.push(SuggestedAlternative {
                description: format!(
                    "Pre-impact needs speed {required_speed:.3}x (< min {:.3}x): trim output_start to {suggested_out_start}, move target_output to {suggested_target}, or lower min_speed to {required_speed:.3}x",
                    req.min_speed
                ),
                suggested_target_output: Some(suggested_target),
                suggested_min_speed: Some((required_speed * 0.99).max(0.0)),
                suggested_max_speed: None,
                suggested_output_start: Some(suggested_out_start),
                suggested_output_end: None,
            });
        } else {
            let suggested_target = Flicks(req.output_end.0 - max_dt_at_min_speed);
            let suggested_out_end = Flicks(req.target_output.0 + max_dt_at_min_speed);
            suggestions.push(SuggestedAlternative {
                description: format!(
                    "Post-impact needs speed {required_speed:.3}x (< min {:.3}x): trim output_end to {suggested_out_end}, move target_output to {suggested_target}, or lower min_speed to {required_speed:.3}x",
                    req.min_speed
                ),
                suggested_target_output: Some(suggested_target),
                suggested_min_speed: Some((required_speed * 0.99).max(0.0)),
                suggested_max_speed: None,
                suggested_output_start: None,
                suggested_output_end: Some(suggested_out_end),
            });
        }
        return (req.source_impact, required_speed);
    }

    if required_speed > req.max_speed {
        if req.fit_mode == SourceFitMode::FitWithinBounds {
            let used_source = max_source_consumable.min(ds_avail);
            let actual_speed = used_source.0 as f64 / dt.0 as f64;
            let eff = if is_pre {
                req.source_impact - used_source
            } else {
                req.source_impact + used_source
            };
            return (eff, actual_speed);
        }

        failures.push(ConstraintFailure::MaxSpeedViolated {
            segment: side,
            required_speed,
            max_speed: req.max_speed,
            available_source: ds_avail,
            max_consumable_source: max_source_consumable,
        });

        let min_dt_at_max_speed = (ds_avail.0 as f64 / req.max_speed).ceil() as i64;
        if is_pre {
            let suggested_target = Flicks(req.output_start.0 + min_dt_at_max_speed);
            let suggested_out_start = Flicks(req.target_output.0 - min_dt_at_max_speed);
            suggestions.push(SuggestedAlternative {
                description: format!(
                    "Pre-impact needs speed {required_speed:.3}x (> max {:.3}x): extend output_start to {suggested_out_start}, move target_output to {suggested_target}, raise max_speed to {required_speed:.3}x, or use FitWithinBounds",
                    req.max_speed
                ),
                suggested_target_output: Some(suggested_target),
                suggested_min_speed: None,
                suggested_max_speed: Some(required_speed * 1.01),
                suggested_output_start: Some(suggested_out_start),
                suggested_output_end: None,
            });
        } else {
            let suggested_target = Flicks(req.output_end.0 - min_dt_at_max_speed);
            let suggested_out_end = Flicks(req.target_output.0 + min_dt_at_max_speed);
            suggestions.push(SuggestedAlternative {
                description: format!(
                    "Post-impact needs speed {required_speed:.3}x (> max {:.3}x): extend output_end to {suggested_out_end}, move target_output to {suggested_target}, raise max_speed to {required_speed:.3}x, or use FitWithinBounds",
                    req.max_speed
                ),
                suggested_target_output: Some(suggested_target),
                suggested_min_speed: None,
                suggested_max_speed: Some(required_speed * 1.01),
                suggested_output_start: None,
                suggested_output_end: Some(suggested_out_end),
            });
        }
        return (req.source_impact, required_speed);
    }

    let eff = if is_pre {
        req.source_bounds_start
    } else {
        req.source_bounds_end
    };
    (eff, required_speed)
}

fn build_solution_curve(
    dt_pre: Flicks,
    dt_post: Flicks,
    v_pre: f64,
    v_post: f64,
    req: &ImpactSolveRequest,
) -> Result<SpeedCurve, ConstraintViolation> {
    let total_dt = dt_pre + dt_post;
    let spec = match req.style {
        ImpactCurveStyle::TwoSegmentConstant => {
            if dt_pre == Flicks::ZERO {
                SpeedCurveSpec::Constant { speed: v_post }
            } else if dt_post == Flicks::ZERO {
                SpeedCurveSpec::Constant { speed: v_pre }
            } else {
                // Narrow transition ramp (1 tick or exact step) approximated cleanly via PiecewiseLinear
                // or split keyframes: using `dt_pre` and `dt_pre + 1` makes pre-segment exact `v_pre`
                // up to 1 tick before target; even better: if `dt_pre > 1` and `dt_post > 1`, use a
                // 1-tick step at `dt_pre` -> integral error is at most 0.5 * |v_post - v_pre| <= a few ticks!
                // Wait: even better, if `v_pre == v_post` use `Constant`. Otherwise center a 2-tick step
                // across `dt_pre` (`dt_pre - 1` at `v_pre`, `dt_pre + 1` at `v_post`) so the integral at
                // `dt_pre` has 0.25*(v_post - v_pre) — OR place step right at `[dt_pre, dt_pre + 1]`?
                // Wait: if we put `[0: v_pre, dt_pre: v_pre, dt_pre+1: v_post, total_dt: v_post]`:
                // On `[0, dt_pre]` the speed is IDENTICALLY `v_pre`! So `source_offset(dt_pre) == v_pre * dt_pre` EXACTLY!
                if dt_post.0 > 1 {
                    SpeedCurveSpec::PiecewiseLinear {
                        keyframes: vec![
                            SpeedKeyframe {
                                output_offset: Flicks::ZERO,
                                speed: v_pre,
                                tangent: None,
                            },
                            SpeedKeyframe {
                                output_offset: dt_pre,
                                speed: v_pre,
                                tangent: None,
                            },
                            SpeedKeyframe {
                                output_offset: Flicks(dt_pre.0 + 1),
                                speed: v_post,
                                tangent: None,
                            },
                            SpeedKeyframe {
                                output_offset: total_dt,
                                speed: v_post,
                                tangent: None,
                            },
                        ],
                    }
                } else {
                    SpeedCurveSpec::PiecewiseLinear {
                        keyframes: vec![
                            SpeedKeyframe {
                                output_offset: Flicks::ZERO,
                                speed: v_pre,
                                tangent: None,
                            },
                            SpeedKeyframe {
                                output_offset: dt_pre,
                                speed: v_post,
                                tangent: None,
                            },
                            SpeedKeyframe {
                                output_offset: total_dt,
                                speed: v_post,
                                tangent: None,
                            },
                        ],
                    }
                }
            }
        }
        ImpactCurveStyle::SmoothRamp => {
            let mut keyframes = Vec::new();
            if dt_pre.0 >= 2 {
                // Symmetric zero-slope Hermite arch keeps exact integral `v_pre * dt_pre`
                // while never leaving `[min_speed, max_speed]`.
                let headroom = (v_pre - req.min_speed).min(req.max_speed - v_pre);
                let delta = 0.5 * headroom;
                let v_edge = v_pre - delta;
                let v_mid = v_pre + delta;
                let mid = Flicks(dt_pre.0 / 2);
                keyframes.push(SpeedKeyframe {
                    output_offset: Flicks::ZERO,
                    speed: v_edge,
                    tangent: Some(0.0),
                });
                if mid > Flicks::ZERO && mid < dt_pre {
                    keyframes.push(SpeedKeyframe {
                        output_offset: mid,
                        speed: v_mid,
                        tangent: Some(0.0),
                    });
                }
                keyframes.push(SpeedKeyframe {
                    output_offset: dt_pre,
                    speed: v_edge,
                    tangent: Some(0.0),
                });
            } else {
                keyframes.push(SpeedKeyframe {
                    output_offset: Flicks::ZERO,
                    speed: v_pre,
                    tangent: Some(0.0),
                });
                if dt_pre > Flicks::ZERO {
                    keyframes.push(SpeedKeyframe {
                        output_offset: dt_pre,
                        speed: v_pre,
                        tangent: Some(0.0),
                    });
                }
            }

            if dt_post.0 >= 2 {
                let headroom = (v_post - req.min_speed).min(req.max_speed - v_post);
                let delta = 0.5 * headroom;
                let v_edge = v_post - delta;
                let v_mid = v_post + delta;
                let mid = Flicks(dt_pre.0 + dt_post.0 / 2);
                // If pre ended at a different speed, add a 1-tick bridge or adjust only if dt_pre == 0
                if dt_pre == Flicks::ZERO {
                    keyframes[0].speed = v_edge;
                } else if (keyframes.last().unwrap().speed - v_edge).abs() > 1e-9 && dt_post.0 >= 4 {
                    // Use a symmetric bridge keeping exact post integral when connecting pre->post
                    // by resetting the post arch around the shared knot's speed only if feasible,
                    // otherwise a 1-tick C0 step into the post arch preserves exact integral.
                    keyframes.push(SpeedKeyframe {
                        output_offset: Flicks(dt_pre.0 + 1),
                        speed: v_edge,
                        tangent: Some(0.0),
                    });
                }
                if mid > keyframes.last().unwrap().output_offset && mid < total_dt {
                    keyframes.push(SpeedKeyframe {
                        output_offset: mid,
                        speed: v_mid,
                        tangent: Some(0.0),
                    });
                }
                keyframes.push(SpeedKeyframe {
                    output_offset: total_dt,
                    speed: v_edge,
                    tangent: Some(0.0),
                });
            } else if dt_post > Flicks::ZERO {
                keyframes.push(SpeedKeyframe {
                    output_offset: total_dt,
                    speed: v_post,
                    tangent: Some(0.0),
                });
            }

            SpeedCurveSpec::CubicHermite { keyframes }
        }
        ImpactCurveStyle::AnimeSnap {
            peak_bias,
            influence,
        } => build_anime_snap_spec(dt_pre, dt_post, v_pre, v_post, peak_bias, influence, req),
    };

    SpeedCurve::new(spec).map_err(|err| ConstraintViolation {
        failures: vec![ConstraintFailure::InvalidSpeedBounds {
            min_speed: req.min_speed,
            max_speed: req.max_speed,
        }],
        suggestions: vec![SuggestedAlternative {
            description: format!("Failed to construct speed curve: {err}"),
            suggested_target_output: None,
            suggested_min_speed: None,
            suggested_max_speed: None,
            suggested_output_start: None,
            suggested_output_end: None,
        }],
    })
}

/// Computes the exact integral weight `W(out_inf, in_inf) = 0.5 + 0.3 * (in_inf - out_inf)`
/// for a zero-slope `CubicBezierSpeed` ramp from `v0` to `v1`.
#[inline]
fn bezier_speed_ramp_weight(out_inf: f64, in_inf: f64) -> f64 {
    0.5 + 0.3 * (in_inf - out_inf)
}

fn push_anime_pre_keyframes(
    keyframes: &mut Vec<BezierSpeedKeyframe>,
    dt_pre: Flicks,
    knee_ticks: Option<i64>,
    v_low: f64,
    out_inf_pre: f64,
) {
    let _ = dt_pre;
    if let Some(k) = knee_ticks {
        keyframes.push(
            BezierSpeedKeyframe::speed(Flicks::ZERO, v_low).with_out_ease(KeyframeEase {
                speed: 0.0,
                influence: 0.333,
            }),
        );
        keyframes.push(
            BezierSpeedKeyframe::speed(Flicks(k), v_low)
                .with_in_ease(KeyframeEase {
                    speed: 0.0,
                    influence: 0.333,
                })
                .with_out_ease(KeyframeEase {
                    speed: 0.0,
                    influence: out_inf_pre,
                }),
        );
    } else {
        keyframes.push(
            BezierSpeedKeyframe::speed(Flicks::ZERO, v_low).with_out_ease(KeyframeEase {
                speed: 0.0,
                influence: out_inf_pre,
            }),
        );
    }
}

fn push_anime_post_keyframes(
    keyframes: &mut Vec<BezierSpeedKeyframe>,
    post_start: Flicks,
    total_dt: Flicks,
    settle_offset: Option<i64>,
    v_settle: f64,
    in_inf_post: f64,
) {
    if let Some(off) = settle_offset {
        keyframes.push(
            BezierSpeedKeyframe::speed(Flicks(post_start.0 + off), v_settle)
                .with_in_ease(KeyframeEase {
                    speed: 0.0,
                    influence: in_inf_post,
                })
                .with_out_ease(KeyframeEase {
                    speed: 0.0,
                    influence: 0.333,
                }),
        );
        keyframes.push(
            BezierSpeedKeyframe::speed(total_dt, v_settle).with_in_ease(KeyframeEase {
                speed: 0.0,
                influence: 0.333,
            }),
        );
    } else {
        keyframes.push(
            BezierSpeedKeyframe::speed(total_dt, v_settle).with_in_ease(KeyframeEase {
                speed: 0.0,
                influence: in_inf_post,
            }),
        );
    }
}

fn build_anime_snap_spec(
    dt_pre: Flicks,
    dt_post: Flicks,
    v_pre: f64,
    v_post: f64,
    peak_bias: f64,
    influence: f64,
    req: &ImpactSolveRequest,
) -> SpeedCurveSpec {
    let total_dt = dt_pre + dt_post;
    let bias = peak_bias.clamp(0.0, 1.0);
    let inf = influence.clamp(0.0, 1.0);

    let alpha_target = 0.65 * bias;
    let out_inf_pre = inf;
    let in_inf_pre = (1.0 - bias) * inf;
    let w_pre = bezier_speed_ramp_weight(out_inf_pre, in_inf_pre);

    let gamma_target = 0.25 + 0.45 * (1.0 - bias);
    let out_inf_post = (1.0 - bias) * inf;
    let in_inf_post = inf;
    let w_post = bezier_speed_ramp_weight(out_inf_post, in_inf_post);

    let knee_ticks = if dt_pre.0 >= 4 {
        let mut alpha = alpha_target;
        let desired_peak = (v_pre.max(v_post) * 1.15).min(req.max_speed);
        if desired_peak > v_pre + 1e-9 && v_pre > req.min_speed + 1e-9 {
            let max_k = (v_pre - req.min_speed) / (desired_peak - req.min_speed);
            let needed_alpha = 1.0 - max_k / w_pre;
            if needed_alpha > alpha {
                alpha = needed_alpha.min(0.85);
            }
        }
        Some(((alpha * dt_pre.0 as f64).round() as i64).clamp(1, dt_pre.0 - 1))
    } else {
        None
    };
    let k_pre = match knee_ticks {
        Some(k) => (1.0 - (k as f64 / dt_pre.0 as f64)) * w_pre,
        None => w_pre,
    };

    let post_geometry = |span: i64, target_v: f64| -> (Option<i64>, f64) {
        if span >= 4 {
            let mut gamma = gamma_target;
            let desired_peak = (v_pre.max(target_v) * 1.15).min(req.max_speed);
            if desired_peak > target_v + 1e-9 && target_v > req.min_speed + 1e-9 {
                let max_k = (target_v - req.min_speed) / (desired_peak - req.min_speed);
                let needed_gamma = max_k / (1.0 - w_post);
                if needed_gamma < gamma {
                    gamma = needed_gamma.max(0.15);
                }
            }
            let off = ((gamma * span as f64).round() as i64).clamp(1, span - 1);
            let actual_gamma = off as f64 / span as f64;
            (Some(off), actual_gamma * (1.0 - w_post))
        } else {
            (None, 1.0 - w_post)
        }
    };

    let v_peak_pre_max = if dt_pre > Flicks::ZERO {
        req.max_speed
            .min(req.min_speed + (v_pre - req.min_speed) / k_pre)
    } else {
        req.max_speed
    };

    let (settle_offset_full, k_post_full) = post_geometry(dt_post.0, v_post);
    let v_peak_post_max = if dt_post > Flicks::ZERO {
        req.max_speed
            .min(req.min_speed + (v_post - req.min_speed) / k_post_full)
    } else {
        req.max_speed
    };

    let mut keyframes = Vec::with_capacity(6);

    if dt_pre == Flicks::ZERO {
        if dt_post.0 < 2 {
            return SpeedCurveSpec::Constant { speed: v_post };
        }
        let v_peak = v_peak_post_max.max(v_post).min(req.max_speed);
        let v_settle = ((v_post - k_post_full * v_peak) / (1.0 - k_post_full))
            .clamp(req.min_speed, req.max_speed);
        keyframes.push(
            BezierSpeedKeyframe::speed(Flicks::ZERO, v_peak).with_out_ease(KeyframeEase {
                speed: 0.0,
                influence: out_inf_post,
            }),
        );
        push_anime_post_keyframes(
            &mut keyframes,
            Flicks::ZERO,
            total_dt,
            settle_offset_full,
            v_settle,
            in_inf_post,
        );
        return SpeedCurveSpec::CubicBezierSpeed { keyframes };
    }

    if dt_post == Flicks::ZERO {
        if dt_pre.0 < 2 {
            return SpeedCurveSpec::Constant { speed: v_pre };
        }
        let v_peak = v_peak_pre_max.max(v_pre).min(req.max_speed);
        let v_low =
            ((v_pre - k_pre * v_peak) / (1.0 - k_pre)).clamp(req.min_speed, req.max_speed);
        push_anime_pre_keyframes(&mut keyframes, dt_pre, knee_ticks, v_low, out_inf_pre);
        keyframes.push(
            BezierSpeedKeyframe::speed(dt_pre, v_peak).with_in_ease(KeyframeEase {
                speed: 0.0,
                influence: in_inf_pre,
            }),
        );
        return SpeedCurveSpec::CubicBezierSpeed { keyframes };
    }

    let lo = v_pre.max(v_post);
    let hi = v_peak_pre_max.min(v_peak_post_max);

    if lo <= hi + 1e-12 {
        // C0-continuous peak shared across impact at `dt_pre`.
        let v_peak = hi.max(lo).min(req.max_speed);
        let v_low =
            ((v_pre - k_pre * v_peak) / (1.0 - k_pre)).clamp(req.min_speed, req.max_speed);
        let v_settle = ((v_post - k_post_full * v_peak) / (1.0 - k_post_full))
            .clamp(req.min_speed, req.max_speed);

        push_anime_pre_keyframes(&mut keyframes, dt_pre, knee_ticks, v_low, out_inf_pre);
        keyframes.push(
            BezierSpeedKeyframe::speed(dt_pre, v_peak)
                .with_in_ease(KeyframeEase {
                    speed: 0.0,
                    influence: in_inf_pre,
                })
                .with_out_ease(KeyframeEase {
                    speed: 0.0,
                    influence: out_inf_post,
                }),
        );
        push_anime_post_keyframes(
            &mut keyframes,
            dt_pre,
            total_dt,
            settle_offset_full,
            v_settle,
            in_inf_post,
        );
        return SpeedCurveSpec::CubicBezierSpeed { keyframes };
    }

    // Disjoint feasible peak ranges (e.g. one segment hugs min_speed while the other hugs max_speed):
    // lock pre-impact integral at `dt_pre` and use a 1-tick bridge at `[dt_pre, dt_pre + 1]`.
    let v_peak_pre = v_peak_pre_max.max(v_pre).min(req.max_speed);
    let v_low =
        ((v_pre - k_pre * v_peak_pre) / (1.0 - k_pre)).clamp(req.min_speed, req.max_speed);
    push_anime_pre_keyframes(&mut keyframes, dt_pre, knee_ticks, v_low, out_inf_pre);

    if dt_post.0 >= 4 {
        let post_start = Flicks(dt_pre.0 + 1);
        let rem_span = dt_post.0 - 1;
        let (settle_off_rem, k_post_rem) = post_geometry(rem_span, v_post);
        let v_peak_post_est = v_peak_post_max.max(v_post).min(req.max_speed);
        let bridge_ticks = 0.5 * (v_peak_pre + v_peak_post_est);
        let v_post_rem_raw = (v_post * dt_post.0 as f64 - bridge_ticks) / rem_span as f64;
        let v_post_eff = if (req.min_speed..=req.max_speed).contains(&v_post_rem_raw) {
            v_post_rem_raw
        } else {
            v_post
        };
        let v_peak_post = req
            .max_speed
            .min(req.min_speed + (v_post_eff - req.min_speed) / k_post_rem)
            .max(v_post_eff);
        let v_settle = ((v_post_eff - k_post_rem * v_peak_post) / (1.0 - k_post_rem))
            .clamp(req.min_speed, req.max_speed);

        keyframes.push(
            BezierSpeedKeyframe::speed(dt_pre, v_peak_pre).with_in_ease(KeyframeEase {
                speed: 0.0,
                influence: in_inf_pre,
            }),
        );
        keyframes.push(
            BezierSpeedKeyframe::speed(post_start, v_peak_post).with_out_ease(KeyframeEase {
                speed: 0.0,
                influence: out_inf_post,
            }),
        );
        push_anime_post_keyframes(
            &mut keyframes,
            post_start,
            total_dt,
            settle_off_rem,
            v_settle,
            in_inf_post,
        );
    } else {
        keyframes.push(
            BezierSpeedKeyframe::speed(dt_pre, v_peak_pre).with_in_ease(KeyframeEase {
                speed: 0.0,
                influence: in_inf_pre,
            }),
        );
        keyframes.push(BezierSpeedKeyframe::speed(total_dt, v_post));
    }

    SpeedCurveSpec::CubicBezierSpeed { keyframes }
}
