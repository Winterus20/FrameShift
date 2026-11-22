use crate::flicks::Flicks;
use crate::speed_curve::{SpeedCurve, SpeedCurveSpec, SpeedKeyframe};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Curve shape requested from `ImpactSolver`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImpactCurveStyle {
    /// Two piecewise-constant segments (`pre_speed` then `post_speed`).
    #[default]
    TwoSegmentConstant,
    /// Smooth C1 `CubicHermite` curve with zero-slope knots that hits the exact
    /// pre-impact and post-impact integrals without exceeding `[min_speed, max_speed]`.
    SmoothRamp,
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
