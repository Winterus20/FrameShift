use crate::error::TimeError;
use crate::flicks::{Flicks, FLICKS_PER_SECOND};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Number of deterministic integration subdivisions per keyframe interval for the LUT.
const LUT_SUBDIVISIONS_PER_SEGMENT: usize = 32;

/// A single keyframe on a time-remapping speed curve.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SpeedKeyframe {
    /// Offset from the start of the clip on the output timeline.
    pub output_offset: Flicks,
    /// Instantaneous playback speed multiplier:
    /// `1.0` = normal forward, `< 1.0` = slow-mo, `> 1.0` = fast-forward,
    /// `0.0` = freeze frame, `< 0.0` = reverse.
    pub speed: f64,
    /// Optional slope `d(speed) / d(second)` for `CubicHermite` curves.
    /// If `None`, a deterministic Catmull-Rom / finite-difference tangent is computed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tangent: Option<f64>,
}

/// Serializable definition of a speed curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SpeedCurveSpec {
    /// Constant playback speed (including negative speeds for reverse).
    Constant { speed: f64 },
    /// Frozen frame (`speed == 0.0` everywhere, `source_offset == 0`).
    Freeze,
    /// Piecewise-linear speed interpolation between keyframes.
    PiecewiseLinear { keyframes: Vec<SpeedKeyframe> },
    /// Smooth C1 cubic Hermite speed interpolation between keyframes.
    CubicHermite { keyframes: Vec<SpeedKeyframe> },
}

/// Precomputed deterministic numerical-integration sample in the lookup table.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpeedLutEntry {
    pub output_offset: Flicks,
    pub source_offset_ticks: f64,
}

/// Keyframed speed curve with deterministic numerical integration lookup table
/// mapping `output_offset: Flicks -> source_offset: Flicks`.
#[derive(Debug, Clone, PartialEq)]
pub struct SpeedCurve {
    spec: SpeedCurveSpec,
    lut: Vec<SpeedLutEntry>,
    /// Auto-computed or explicit tangents (in speed/second) for `CubicHermite`.
    hermite_tangents: Vec<f64>,
}

impl Serialize for SpeedCurve {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.spec.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SpeedCurve {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let spec = SpeedCurveSpec::deserialize(deserializer)?;
        SpeedCurve::new(spec).map_err(serde::de::Error::custom)
    }
}

impl SpeedCurve {
    /// Validates `spec` and builds the deterministic numerical integration lookup table.
    pub fn new(spec: SpeedCurveSpec) -> Result<Self, TimeError> {
        match &spec {
            SpeedCurveSpec::Constant { speed } => {
                if !speed.is_finite() {
                    return Err(TimeError::InvalidSpeedCurve(
                        "constant speed must be finite".into(),
                    ));
                }
                Ok(Self {
                    spec,
                    lut: vec![SpeedLutEntry {
                        output_offset: Flicks::ZERO,
                        source_offset_ticks: 0.0,
                    }],
                    hermite_tangents: Vec::new(),
                })
            }
            SpeedCurveSpec::Freeze => Ok(Self {
                spec,
                lut: vec![SpeedLutEntry {
                    output_offset: Flicks::ZERO,
                    source_offset_ticks: 0.0,
                }],
                hermite_tangents: Vec::new(),
            }),
            SpeedCurveSpec::PiecewiseLinear { keyframes } => {
                validate_keyframes(keyframes)?;
                let lut = build_piecewise_linear_lut(keyframes);
                Ok(Self {
                    spec,
                    lut,
                    hermite_tangents: Vec::new(),
                })
            }
            SpeedCurveSpec::CubicHermite { keyframes } => {
                validate_keyframes(keyframes)?;
                let tangents = compute_hermite_tangents(keyframes);
                let lut = build_cubic_hermite_lut(keyframes, &tangents);
                Ok(Self {
                    spec,
                    lut,
                    hermite_tangents: tangents,
                })
            }
        }
    }

    /// Convenience constructor for constant speed (`1.0` = identity).
    pub fn constant(speed: f64) -> Result<Self, TimeError> {
        Self::new(SpeedCurveSpec::Constant { speed })
    }

    /// Convenience constructor for a freeze curve.
    pub fn freeze() -> Self {
        Self {
            spec: SpeedCurveSpec::Freeze,
            lut: vec![SpeedLutEntry {
                output_offset: Flicks::ZERO,
                source_offset_ticks: 0.0,
            }],
            hermite_tangents: Vec::new(),
        }
    }

    /// Convenience constructor for a piecewise-linear speed curve.
    pub fn piecewise_linear(keyframes: Vec<SpeedKeyframe>) -> Result<Self, TimeError> {
        Self::new(SpeedCurveSpec::PiecewiseLinear { keyframes })
    }

    /// Convenience constructor for a cubic Hermite speed curve.
    pub fn cubic_hermite(keyframes: Vec<SpeedKeyframe>) -> Result<Self, TimeError> {
        Self::new(SpeedCurveSpec::CubicHermite { keyframes })
    }

    /// Returns the underlying curve specification.
    #[inline]
    pub fn spec(&self) -> &SpeedCurveSpec {
        &self.spec
    }

    /// Returns the precomputed deterministic numerical integration lookup table.
    #[inline]
    pub fn lut(&self) -> &[SpeedLutEntry] {
        &self.lut
    }

    /// Maps `output_offset` (relative to clip output start) to `source_offset`
    /// (relative to clip source start), rounded to nearest `Flicks`.
    pub fn source_offset_at(&self, output_offset: Flicks) -> Flicks {
        match &self.spec {
            SpeedCurveSpec::Freeze => Flicks::ZERO,
            SpeedCurveSpec::Constant { speed } if *speed == 1.0 => output_offset,
            SpeedCurveSpec::Constant { speed } if *speed == -1.0 => -output_offset,
            SpeedCurveSpec::Constant { speed } if *speed == 0.0 => Flicks::ZERO,
            _ => {
                let raw = self.source_offset_f64(output_offset);
                Flicks(raw.round() as i64)
            }
        }
    }

    /// Maps `output_offset` to a sub-tick floating-point `source_offset` in Flicks.
    pub fn source_offset_f64(&self, output_offset: Flicks) -> f64 {
        match &self.spec {
            SpeedCurveSpec::Freeze => 0.0,
            SpeedCurveSpec::Constant { speed } => (output_offset.0 as f64) * (*speed),
            SpeedCurveSpec::PiecewiseLinear { keyframes } => {
                eval_lut_integral(&self.lut, keyframes, None, output_offset)
            }
            SpeedCurveSpec::CubicHermite { keyframes } => {
                eval_lut_integral(&self.lut, keyframes, Some(&self.hermite_tangents), output_offset)
            }
        }
    }

    /// Evaluates instantaneous playback speed multiplier at `output_offset`.
    pub fn instant_speed_at(&self, output_offset: Flicks) -> f64 {
        match &self.spec {
            SpeedCurveSpec::Freeze => 0.0,
            SpeedCurveSpec::Constant { speed } => *speed,
            SpeedCurveSpec::PiecewiseLinear { keyframes } => {
                eval_piecewise_linear_speed(keyframes, output_offset)
            }
            SpeedCurveSpec::CubicHermite { keyframes } => {
                eval_cubic_hermite_speed(keyframes, &self.hermite_tangents, output_offset)
            }
        }
    }

    /// Returns `(min_source_offset, max_source_offset)` reached across `[start_output, end_output]`.
    /// Useful for bounds verification when curves contain reverse or freeze sections.
    pub fn source_extrema(&self, start_output: Flicks, end_output: Flicks) -> (Flicks, Flicks) {
        let (lo, hi) = if start_output <= end_output {
            (start_output, end_output)
        } else {
            (end_output, start_output)
        };
        let mut min_val = self.source_offset_at(lo);
        let mut max_val = self.source_offset_at(hi);
        if max_val < min_val {
            std::mem::swap(&mut min_val, &mut max_val);
        }

        for entry in &self.lut {
            if entry.output_offset > lo && entry.output_offset < hi {
                let s = Flicks(entry.source_offset_ticks.round() as i64);
                if s < min_val {
                    min_val = s;
                }
                if s > max_val {
                    max_val = s;
                }
            }
        }
        (min_val, max_val)
    }
}

fn validate_keyframes(keyframes: &[SpeedKeyframe]) -> Result<(), TimeError> {
    if keyframes.is_empty() {
        return Err(TimeError::InvalidSpeedCurve(
            "keyframed speed curve requires at least one keyframe".into(),
        ));
    }
    for (i, kf) in keyframes.iter().enumerate() {
        if !kf.speed.is_finite() {
            return Err(TimeError::InvalidSpeedCurve(format!(
                "keyframe {i} has non-finite speed"
            )));
        }
        if let Some(t) = kf.tangent
            && !t.is_finite()
        {
            return Err(TimeError::InvalidSpeedCurve(format!(
                "keyframe {i} has non-finite tangent"
            )));
        }
        if i > 0 && kf.output_offset <= keyframes[i - 1].output_offset {
            return Err(TimeError::InvalidSpeedCurve(format!(
                "keyframe {i} output_offset ({}) must be strictly greater than previous ({})",
                kf.output_offset.0,
                keyframes[i - 1].output_offset.0
            )));
        }
    }
    Ok(())
}

fn compute_hermite_tangents(keyframes: &[SpeedKeyframe]) -> Vec<f64> {
    let n = keyframes.len();
    if n == 1 {
        return vec![keyframes[0].tangent.unwrap_or(0.0)];
    }

    let mut tangents = Vec::with_capacity(n);
    for i in 0..n {
        if let Some(explicit) = keyframes[i].tangent {
            tangents.push(explicit);
            continue;
        }
        let t = if i == 0 {
            let dt = (keyframes[1].output_offset.0 - keyframes[0].output_offset.0) as f64
                / FLICKS_PER_SECOND as f64;
            (keyframes[1].speed - keyframes[0].speed) / dt
        } else if i == n - 1 {
            let dt = (keyframes[n - 1].output_offset.0 - keyframes[n - 2].output_offset.0) as f64
                / FLICKS_PER_SECOND as f64;
            (keyframes[n - 1].speed - keyframes[n - 2].speed) / dt
        } else {
            let dt = (keyframes[i + 1].output_offset.0 - keyframes[i - 1].output_offset.0) as f64
                / FLICKS_PER_SECOND as f64;
            (keyframes[i + 1].speed - keyframes[i - 1].speed) / dt
        };
        tangents.push(t);
    }
    tangents
}

/// 4-point Gauss-Legendre quadrature nodes and weights on `[-1, 1]`.
/// Exact for all polynomials up to degree 7 (including `CubicHermite` degree 3).
const GL4_NODES: [f64; 4] = [
    -0.861_136_311_594_052_6,
    -0.339_981_043_584_856_3,
    0.339_981_043_584_856_3,
    0.861_136_311_594_052_6,
];
const GL4_WEIGHTS: [f64; 4] = [
    0.347_854_845_137_453_85,
    0.652_145_154_862_546_1,
    0.652_145_154_862_546_1,
    0.347_854_845_137_453_85,
];

/// Integrates a speed function `f(t_ticks)` over `[a, b]` (in ticks) using 4-point Gauss-Legendre.
#[inline]
fn integrate_gl4<F: Fn(f64) -> f64>(a: f64, b: f64, f: F) -> f64 {
    let half = 0.5 * (b - a);
    let mid = 0.5 * (a + b);
    let mut sum = 0.0;
    for i in 0..4 {
        sum += GL4_WEIGHTS[i] * f(mid + half * GL4_NODES[i]);
    }
    half * sum
}

fn build_piecewise_linear_lut(keyframes: &[SpeedKeyframe]) -> Vec<SpeedLutEntry> {
    let mut lut = Vec::new();
    let mut acc = 0.0_f64;

    if keyframes[0].output_offset > Flicks::ZERO {
        lut.push(SpeedLutEntry {
            output_offset: Flicks::ZERO,
            source_offset_ticks: 0.0,
        });
        acc += (keyframes[0].output_offset.0 as f64) * keyframes[0].speed;
    } else if keyframes[0].output_offset < Flicks::ZERO {
        // Normalize integration origin so output_offset == 0 has source_offset == 0.
        let origin_offset = integrate_piecewise_linear_from_zero(keyframes, Flicks::ZERO);
        acc = -origin_offset;
    }

    lut.push(SpeedLutEntry {
        output_offset: keyframes[0].output_offset,
        source_offset_ticks: acc,
    });

    for seg in keyframes.windows(2) {
        let a = seg[0];
        let b = seg[1];
        let t0 = a.output_offset.0;
        let t1 = b.output_offset.0;
        let span = t1 - t0;

        for step in 1..=LUT_SUBDIVISIONS_PER_SEGMENT {
            let t_prev = t0 + ((span as i128 * (step - 1) as i128) / LUT_SUBDIVISIONS_PER_SEGMENT as i128) as i64;
            let t_curr = if step == LUT_SUBDIVISIONS_PER_SEGMENT {
                t1
            } else {
                t0 + ((span as i128 * step as i128) / LUT_SUBDIVISIONS_PER_SEGMENT as i128) as i64
            };
            acc += integrate_gl4(t_prev as f64, t_curr as f64, |t| {
                let u = (t - t0 as f64) / (span as f64);
                a.speed + (b.speed - a.speed) * u
            });
            lut.push(SpeedLutEntry {
                output_offset: Flicks(t_curr),
                source_offset_ticks: acc,
            });
        }
    }

    lut
}

fn integrate_piecewise_linear_from_zero(keyframes: &[SpeedKeyframe], target: Flicks) -> f64 {
    // Only used if keyframes[0].output_offset < 0 to anchor source_offset(0) = 0.
    let mut acc = 0.0;
    for seg in keyframes.windows(2) {
        let a = seg[0];
        let b = seg[1];
        if target <= a.output_offset {
            break;
        }
        let seg_end = target.min(b.output_offset);
        let span = (b.output_offset.0 - a.output_offset.0) as f64;
        acc += integrate_gl4(a.output_offset.0 as f64, seg_end.0 as f64, |t| {
            let u = (t - a.output_offset.0 as f64) / span;
            a.speed + (b.speed - a.speed) * u
        });
    }
    if target > keyframes.last().unwrap().output_offset {
        let last = keyframes.last().unwrap();
        acc += (target.0 - last.output_offset.0) as f64 * last.speed;
    }
    acc
}

fn build_cubic_hermite_lut(keyframes: &[SpeedKeyframe], tangents: &[f64]) -> Vec<SpeedLutEntry> {
    let mut lut = Vec::new();
    let mut acc = 0.0_f64;

    if keyframes[0].output_offset > Flicks::ZERO {
        lut.push(SpeedLutEntry {
            output_offset: Flicks::ZERO,
            source_offset_ticks: 0.0,
        });
        acc += (keyframes[0].output_offset.0 as f64) * keyframes[0].speed;
    } else if keyframes[0].output_offset < Flicks::ZERO {
        let origin_offset = integrate_cubic_hermite_raw(keyframes, tangents, Flicks::ZERO);
        acc = -origin_offset;
    }

    lut.push(SpeedLutEntry {
        output_offset: keyframes[0].output_offset,
        source_offset_ticks: acc,
    });

    for (idx, seg) in keyframes.windows(2).enumerate() {
        let a = seg[0];
        let b = seg[1];
        let m0 = tangents[idx];
        let m1 = tangents[idx + 1];
        let t0 = a.output_offset.0;
        let t1 = b.output_offset.0;
        let span = t1 - t0;
        let dt_sec = span as f64 / FLICKS_PER_SECOND as f64;

        for step in 1..=LUT_SUBDIVISIONS_PER_SEGMENT {
            let t_prev = t0 + ((span as i128 * (step - 1) as i128) / LUT_SUBDIVISIONS_PER_SEGMENT as i128) as i64;
            let t_curr = if step == LUT_SUBDIVISIONS_PER_SEGMENT {
                t1
            } else {
                t0 + ((span as i128 * step as i128) / LUT_SUBDIVISIONS_PER_SEGMENT as i128) as i64
            };
            acc += integrate_gl4(t_prev as f64, t_curr as f64, |t| {
                let u = (t - t0 as f64) / (span as f64);
                eval_hermite_poly(a.speed, b.speed, m0, m1, dt_sec, u)
            });
            lut.push(SpeedLutEntry {
                output_offset: Flicks(t_curr),
                source_offset_ticks: acc,
            });
        }
    }

    lut
}

fn integrate_cubic_hermite_raw(
    keyframes: &[SpeedKeyframe],
    tangents: &[f64],
    target: Flicks,
) -> f64 {
    let mut acc = 0.0;
    for (idx, seg) in keyframes.windows(2).enumerate() {
        let a = seg[0];
        let b = seg[1];
        if target <= a.output_offset {
            break;
        }
        let seg_end = target.min(b.output_offset);
        let span = (b.output_offset.0 - a.output_offset.0) as f64;
        let dt_sec = span / FLICKS_PER_SECOND as f64;
        let m0 = tangents[idx];
        let m1 = tangents[idx + 1];
        acc += integrate_gl4(a.output_offset.0 as f64, seg_end.0 as f64, |t| {
            let u = (t - a.output_offset.0 as f64) / span;
            eval_hermite_poly(a.speed, b.speed, m0, m1, dt_sec, u)
        });
    }
    if target > keyframes.last().unwrap().output_offset {
        let last = keyframes.last().unwrap();
        acc += (target.0 - last.output_offset.0) as f64 * last.speed;
    }
    acc
}

#[inline]
fn eval_hermite_poly(v0: f64, v1: f64, m0: f64, m1: f64, dt_sec: f64, u: f64) -> f64 {
    let u2 = u * u;
    let u3 = u2 * u;
    let h00 = 2.0 * u3 - 3.0 * u2 + 1.0;
    let h10 = u3 - 2.0 * u2 + u;
    let h01 = -2.0 * u3 + 3.0 * u2;
    let h11 = u3 - u2;
    h00 * v0 + h10 * dt_sec * m0 + h01 * v1 + h11 * dt_sec * m1
}

fn eval_lut_integral(
    lut: &[SpeedLutEntry],
    keyframes: &[SpeedKeyframe],
    tangents: Option<&[f64]>,
    output_offset: Flicks,
) -> f64 {
    let first = lut.first().expect("LUT is non-empty");
    let last = lut.last().expect("LUT is non-empty");

    if output_offset <= first.output_offset {
        let dt = (output_offset.0 - first.output_offset.0) as f64;
        return first.source_offset_ticks + dt * keyframes[0].speed;
    }
    if output_offset >= last.output_offset {
        let dt = (output_offset.0 - last.output_offset.0) as f64;
        return last.source_offset_ticks + dt * keyframes.last().unwrap().speed;
    }

    let pos = lut.partition_point(|e| e.output_offset <= output_offset);
    let idx = pos - 1;
    let base = lut[idx];
    if base.output_offset == output_offset {
        return base.source_offset_ticks;
    }

    // Find enclosing keyframe segment for exact polynomial evaluation between LUT nodes.
    let kf_pos = keyframes.partition_point(|k| k.output_offset <= output_offset);
    if kf_pos == 0 {
        let dt = (output_offset.0 - base.output_offset.0) as f64;
        return base.source_offset_ticks + dt * keyframes[0].speed;
    }
    let seg_idx = kf_pos - 1;
    let a = keyframes[seg_idx];
    let b = keyframes[seg_idx + 1];
    let span = (b.output_offset.0 - a.output_offset.0) as f64;
    let t0 = a.output_offset.0 as f64;

    let delta = match tangents {
        None => integrate_gl4(base.output_offset.0 as f64, output_offset.0 as f64, |t| {
            let u = (t - t0) / span;
            a.speed + (b.speed - a.speed) * u
        }),
        Some(ts) => {
            let dt_sec = span / FLICKS_PER_SECOND as f64;
            let m0 = ts[seg_idx];
            let m1 = ts[seg_idx + 1];
            integrate_gl4(base.output_offset.0 as f64, output_offset.0 as f64, |t| {
                let u = (t - t0) / span;
                eval_hermite_poly(a.speed, b.speed, m0, m1, dt_sec, u)
            })
        }
    };

    base.source_offset_ticks + delta
}

fn eval_piecewise_linear_speed(keyframes: &[SpeedKeyframe], output_offset: Flicks) -> f64 {
    if output_offset <= keyframes[0].output_offset {
        return keyframes[0].speed;
    }
    let last = keyframes.last().unwrap();
    if output_offset >= last.output_offset {
        return last.speed;
    }
    let pos = keyframes.partition_point(|k| k.output_offset <= output_offset);
    let a = keyframes[pos - 1];
    let b = keyframes[pos];
    let u = (output_offset.0 - a.output_offset.0) as f64
        / (b.output_offset.0 - a.output_offset.0) as f64;
    a.speed + (b.speed - a.speed) * u
}

fn eval_cubic_hermite_speed(
    keyframes: &[SpeedKeyframe],
    tangents: &[f64],
    output_offset: Flicks,
) -> f64 {
    if output_offset <= keyframes[0].output_offset {
        return keyframes[0].speed;
    }
    let last = keyframes.last().unwrap();
    if output_offset >= last.output_offset {
        return last.speed;
    }
    let pos = keyframes.partition_point(|k| k.output_offset <= output_offset);
    let seg_idx = pos - 1;
    let a = keyframes[seg_idx];
    let b = keyframes[seg_idx + 1];
    let span = (b.output_offset.0 - a.output_offset.0) as f64;
    let dt_sec = span / FLICKS_PER_SECOND as f64;
    let u = (output_offset.0 - a.output_offset.0) as f64 / span;
    eval_hermite_poly(a.speed, b.speed, tangents[seg_idx], tangents[seg_idx + 1], dt_sec, u)
}
