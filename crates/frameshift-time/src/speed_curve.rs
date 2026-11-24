use crate::error::TimeError;
use crate::flicks::{Flicks, FLICKS_PER_SECOND};
use crate::rational::Rational32;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Number of deterministic integration subdivisions per keyframe interval for `PiecewiseLinear` and `CubicHermite` LUTs.
const LUT_SUBDIVISIONS_PER_SEGMENT: usize = 32;

/// Number of deterministic integration/sampling subdivisions per keyframe interval for `CubicBezier*` LUTs.
const BEZIER_LUT_SUBDIVISIONS_PER_SEGMENT: usize = 64;

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

/// Incoming or outgoing cubic Bézier handle ease for speed or value keyframes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct KeyframeEase {
    /// Tangent slope at the keyframe:
    /// - For `CubicBezierValue`: instantaneous playback speed `d(source) / d(output)` (dimensionless).
    /// - For `CubicBezierSpeed`: speed acceleration `d(speed) / d(second)`.
    pub speed: f64,
    /// Horizontal handle influence fraction in `[0.0, 1.0]` (default `0.333`).
    pub influence: f64,
}

impl Default for KeyframeEase {
    fn default() -> Self {
        Self {
            speed: 0.0,
            influence: 0.333,
        }
    }
}

/// A single keyframe on a `CubicBezierValue` or `CubicBezierSpeed` time-remapping curve.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BezierSpeedKeyframe {
    /// Offset from the start of the clip on the output timeline.
    pub output_offset: Flicks,
    /// Instantaneous playback speed multiplier (used by `CubicBezierSpeed`).
    pub speed: f64,
    /// Target source offset at `output_offset` (required by `CubicBezierValue`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_offset: Option<Flicks>,
    /// Incoming Bézier handle ease (arriving from the previous keyframe).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub in_ease: Option<KeyframeEase>,
    /// Outgoing Bézier handle ease (leaving toward the next keyframe).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub out_ease: Option<KeyframeEase>,
}

impl BezierSpeedKeyframe {
    /// Creates a `CubicBezierValue` keyframe mapping `output_offset` to `source_offset`.
    pub fn value(output_offset: Flicks, source_offset: Flicks) -> Self {
        Self {
            output_offset,
            speed: 0.0,
            source_offset: Some(source_offset),
            in_ease: None,
            out_ease: None,
        }
    }

    /// Creates a `CubicBezierSpeed` keyframe with instantaneous `speed` at `output_offset`.
    pub fn speed(output_offset: Flicks, speed: f64) -> Self {
        Self {
            output_offset,
            speed,
            source_offset: None,
            in_ease: None,
            out_ease: None,
        }
    }

    /// Attaches an incoming Bézier ease handle.
    pub fn with_in_ease(mut self, ease: KeyframeEase) -> Self {
        self.in_ease = Some(ease);
        self
    }

    /// Attaches an outgoing Bézier ease handle.
    pub fn with_out_ease(mut self, ease: KeyframeEase) -> Self {
        self.out_ease = Some(ease);
        self
    }
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
    /// After Effects-style Value Graph (`output_offset` -> `source_offset`) cubic Bézier curve
    /// solved analytically via Newton-Raphson inversion of `x(u) = t`.
    CubicBezierValue { keyframes: Vec<BezierSpeedKeyframe> },
    /// Speed Graph (`output_offset` -> `speed`) cubic Bézier curve supporting independent/broken
    /// `in_ease` / `out_ease` handles and integrated via a 64-subdivision GL4 LUT.
    CubicBezierSpeed { keyframes: Vec<BezierSpeedKeyframe> },
}

const fn default_stutter_step_or_hold() -> u32 {
    1
}

/// Temporal quantization / anime-style "on twos / on threes" posterize-time specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TimeQuantizeSpec {
    /// No temporal quantization (passes `tick` through unchanged).
    #[default]
    None,
    /// Quantizes `tick` onto the frame grid of `fps` (like After Effects `posterizeTime(fps)`).
    PosterizeFps { fps: Rational32 },
    /// Anime limited-animation cadence ("on twos", "on threes", or irregular step+hold):
    /// holds each sampled project frame for `hold_frames` frames, then steps by `step_frames`.
    StutterPattern {
        #[serde(default = "default_stutter_step_or_hold")]
        hold_frames: u32,
        #[serde(default = "default_stutter_step_or_hold")]
        step_frames: u32,
    },
}

/// Deterministically quantizes `tick` according to `spec` and `project_fps` using exact `i128` tick arithmetic.
///
/// Invalid rationals or `hold_frames == 0` fall back safely to returning `tick` unchanged.
pub fn apply_time_quantize(tick: Flicks, project_fps: Rational32, spec: &TimeQuantizeSpec) -> Flicks {
    match *spec {
        TimeQuantizeSpec::None => tick,
        TimeQuantizeSpec::PosterizeFps { fps } => {
            if fps.num == 0 || fps.den == 0 {
                return tick;
            }
            let Ok(frame) = fps.ticks_to_frame_floor(tick) else {
                return tick;
            };
            frame_start_tick_ceil(fps, frame).unwrap_or(tick)
        }
        TimeQuantizeSpec::StutterPattern {
            hold_frames,
            step_frames,
        } => {
            if hold_frames == 0 || project_fps.num == 0 || project_fps.den == 0 {
                return tick;
            }
            let Ok(frame) = project_fps.ticks_to_frame_floor(tick) else {
                return tick;
            };
            let cycle = (frame as i128).div_euclid(hold_frames as i128);
            let Ok(quantized_frame) = i64::try_from(cycle * (step_frames as i128)) else {
                return tick;
            };
            frame_start_tick_ceil(project_fps, quantized_frame).unwrap_or(tick)
        }
    }
}

#[inline]
fn frame_start_tick_ceil(fps: Rational32, frame_index: i64) -> Result<Flicks, TimeError> {
    let numerator = (frame_index as i128) * (FLICKS_PER_SECOND as i128) * (fps.den as i128);
    let denominator = fps.num as i128;
    let q = numerator.div_euclid(denominator);
    let r = numerator.rem_euclid(denominator);
    let val = if r == 0 { q } else { q + 1 };
    let ticks = i64::try_from(val).map_err(|_| TimeError::Overflow)?;
    Ok(Flicks(ticks))
}

/// Precomputed deterministic numerical-integration sample in the lookup table.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpeedLutEntry {
    pub output_offset: Flicks,
    pub source_offset_ticks: f64,
}

/// Precomputed 2D cubic Bézier segment `(x(u), y(u))` with normalized `x in [0, 1]`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct CubicBezierSegment2D {
    t0: f64,
    span_ticks: f64,
    /// Scale factor converting normalized `x` units into slope units (`span_ticks` for Value Graph,
    /// `span_seconds` for Speed Graph).
    slope_scale: f64,
    x1: f64,
    x2: f64,
    ax: f64,
    bx: f64,
    cx: f64,
    y0: f64,
    y1_val: f64,
    c1_y: f64,
    c2_y: f64,
    ay: f64,
    by: f64,
    cy: f64,
}

/// Keyframed speed curve with deterministic numerical integration lookup table
/// mapping `output_offset: Flicks -> source_offset: Flicks`.
#[derive(Debug, Clone, PartialEq)]
pub struct SpeedCurve {
    spec: SpeedCurveSpec,
    lut: Vec<SpeedLutEntry>,
    /// Auto-computed or explicit tangents (in speed/second) for `CubicHermite`.
    hermite_tangents: Vec<f64>,
    /// Precomputed segments for `CubicBezierValue` and `CubicBezierSpeed`.
    bezier_segments: Vec<CubicBezierSegment2D>,
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
                    bezier_segments: Vec::new(),
                })
            }
            SpeedCurveSpec::Freeze => Ok(Self {
                spec,
                lut: vec![SpeedLutEntry {
                    output_offset: Flicks::ZERO,
                    source_offset_ticks: 0.0,
                }],
                hermite_tangents: Vec::new(),
                bezier_segments: Vec::new(),
            }),
            SpeedCurveSpec::PiecewiseLinear { keyframes } => {
                validate_keyframes(keyframes)?;
                let lut = build_piecewise_linear_lut(keyframes);
                Ok(Self {
                    spec,
                    lut,
                    hermite_tangents: Vec::new(),
                    bezier_segments: Vec::new(),
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
                    bezier_segments: Vec::new(),
                })
            }
            SpeedCurveSpec::CubicBezierValue { keyframes } => {
                validate_bezier_keyframes(keyframes, true)?;
                let segments = build_bezier_value_segments(keyframes);
                let lut = build_bezier_value_lut(keyframes, &segments);
                Ok(Self {
                    spec,
                    lut,
                    hermite_tangents: Vec::new(),
                    bezier_segments: segments,
                })
            }
            SpeedCurveSpec::CubicBezierSpeed { keyframes } => {
                validate_bezier_keyframes(keyframes, false)?;
                let segments = build_bezier_speed_segments(keyframes);
                let lut = build_bezier_speed_lut(keyframes, &segments);
                Ok(Self {
                    spec,
                    lut,
                    hermite_tangents: Vec::new(),
                    bezier_segments: segments,
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
            bezier_segments: Vec::new(),
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

    /// Convenience constructor for an AE-style cubic Bézier Value Graph curve.
    pub fn cubic_bezier_value(keyframes: Vec<BezierSpeedKeyframe>) -> Result<Self, TimeError> {
        Self::new(SpeedCurveSpec::CubicBezierValue { keyframes })
    }

    /// Convenience constructor for a cubic Bézier Speed Graph curve with broken handles.
    pub fn cubic_bezier_speed(keyframes: Vec<BezierSpeedKeyframe>) -> Result<Self, TimeError> {
        Self::new(SpeedCurveSpec::CubicBezierSpeed { keyframes })
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
            SpeedCurveSpec::CubicBezierValue { keyframes } => {
                eval_bezier_value_source(keyframes, &self.bezier_segments, output_offset)
            }
            SpeedCurveSpec::CubicBezierSpeed { keyframes } => {
                eval_bezier_speed_integral(&self.lut, keyframes, &self.bezier_segments, output_offset)
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
            SpeedCurveSpec::CubicBezierValue { keyframes } => {
                eval_bezier_value_speed(keyframes, &self.bezier_segments, output_offset)
            }
            SpeedCurveSpec::CubicBezierSpeed { keyframes } => {
                eval_bezier_speed_value(keyframes, &self.bezier_segments, output_offset)
            }
        }
    }

    /// Evaluates the right-hand derivative of playback speed at `output_offset`.
    ///
    /// The result is in speed units per second. This is useful when cropping a
    /// curve: the derivative at the new head preserves the original Hermite
    /// segment instead of recomputing its tangent from the shortened key list.
    pub fn instant_speed_derivative_at(&self, output_offset: Flicks) -> f64 {
        match &self.spec {
            SpeedCurveSpec::Freeze | SpeedCurveSpec::Constant { .. } => 0.0,
            SpeedCurveSpec::PiecewiseLinear { keyframes } => {
                if output_offset < keyframes[0].output_offset
                    || output_offset >= keyframes.last().unwrap().output_offset
                {
                    return 0.0;
                }
                let pos = keyframes.partition_point(|kf| kf.output_offset <= output_offset);
                let index = pos.saturating_sub(1).min(keyframes.len() - 2);
                let a = keyframes[index];
                let b = keyframes[index + 1];
                (b.speed - a.speed) * FLICKS_PER_SECOND as f64
                    / (b.output_offset.0 - a.output_offset.0) as f64
            }
            SpeedCurveSpec::CubicHermite { keyframes } => {
                let first = keyframes[0];
                let last = keyframes.last().unwrap();
                if output_offset < first.output_offset || output_offset >= last.output_offset {
                    return 0.0;
                }
                let pos = keyframes.partition_point(|kf| kf.output_offset <= output_offset);
                let seg_idx = pos.saturating_sub(1).min(keyframes.len() - 2);
                let a = keyframes[seg_idx];
                let b = keyframes[seg_idx + 1];
                let span_ticks = (b.output_offset.0 - a.output_offset.0) as f64;
                let span_seconds = span_ticks / FLICKS_PER_SECOND as f64;
                let u = (output_offset.0 - a.output_offset.0) as f64 / span_ticks;
                eval_cubic_hermite_speed_derivative(
                    a.speed,
                    b.speed,
                    self.hermite_tangents[seg_idx],
                    self.hermite_tangents[seg_idx + 1],
                    span_seconds,
                    u,
                )
            }
            SpeedCurveSpec::CubicBezierValue { keyframes } => {
                let first = keyframes[0];
                let last = keyframes.last().unwrap();
                if output_offset < first.output_offset || output_offset >= last.output_offset {
                    return 0.0;
                }
                let pos = keyframes.partition_point(|kf| kf.output_offset <= output_offset);
                let seg_idx = pos.saturating_sub(1).min(keyframes.len() - 2);
                let seg = &self.bezier_segments[seg_idx];
                let u = seg.solve_u_at_tick(output_offset);
                seg.eval_second_derivative_per_sec(u)
            }
            SpeedCurveSpec::CubicBezierSpeed { keyframes } => {
                let first = keyframes[0];
                let last = keyframes.last().unwrap();
                if output_offset < first.output_offset || output_offset >= last.output_offset {
                    return 0.0;
                }
                let pos = keyframes.partition_point(|kf| kf.output_offset <= output_offset);
                let seg_idx = pos.saturating_sub(1).min(keyframes.len() - 2);
                let seg = &self.bezier_segments[seg_idx];
                let u = seg.solve_u_at_tick(output_offset);
                seg.eval_dy_dx(u)
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

        if matches!(self.spec, SpeedCurveSpec::CubicBezierValue { .. }) {
            for seg in &self.bezier_segments {
                let mut roots = [0.0_f64; 2];
                let count = seg.y_extrema_u(&mut roots);
                for &u in &roots[..count] {
                    let t_root = Flicks((seg.t0 + seg.eval_x(u) * seg.span_ticks).round() as i64);
                    if t_root > lo && t_root < hi {
                        let s = Flicks(seg.eval_y(u).round() as i64);
                        if s < min_val {
                            min_val = s;
                        }
                        if s > max_val {
                            max_val = s;
                        }
                    }
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

fn eval_cubic_hermite_speed_derivative(
    v0: f64,
    v1: f64,
    m0: f64,
    m1: f64,
    span_seconds: f64,
    u: f64,
) -> f64 {
    let u2 = u * u;
    let dv0 = 6.0 * u2 - 6.0 * u;
    let dm0 = 3.0 * u2 - 4.0 * u + 1.0;
    let dv1 = -6.0 * u2 + 6.0 * u;
    let dm1 = 3.0 * u2 - 2.0 * u;
    (dv0 * v0 + dv1 * v1) / span_seconds + dm0 * m0 + dm1 * m1
}

/// Evaluates a 1D cubic Bézier `B(u)` for control values `(p0, p1, p2, p3)` at `u in [0, 1]`.
#[inline]
pub(crate) fn eval_cubic_bezier_1d(p0: f64, p1: f64, p2: f64, p3: f64, u: f64) -> f64 {
    let inv = 1.0 - u;
    let inv2 = inv * inv;
    let u2 = u * u;
    inv2 * inv * p0 + 3.0 * inv2 * u * p1 + 3.0 * inv * u2 * p2 + u2 * u * p3
}

/// Solves `x(u) = x_target` for `u in [0, 1]` on a unit cubic Bézier with `x0 = 0`, `x3 = 1`
/// and `x1, x2 in [0, 1]`, using bracketed Newton-Raphson with bisection fallback.
pub(crate) fn solve_cubic_bezier_x(x1: f64, x2: f64, x_target: f64) -> f64 {
    if x_target <= 0.0 {
        return 0.0;
    }
    if x_target >= 1.0 {
        return 1.0;
    }

    let cx = 3.0 * x1;
    let bx = 3.0 * x2 - 6.0 * x1;
    let ax = 1.0 - 3.0 * x2 + 3.0 * x1;

    let mut lo = 0.0_f64;
    let mut hi = 1.0_f64;
    let mut u = x_target;

    for _ in 0..16 {
        let x_val = ((ax * u + bx) * u + cx) * u;
        let err = x_val - x_target;
        if err.abs() < 1e-12 {
            return u;
        }
        if err < 0.0 {
            lo = u;
        } else {
            hi = u;
        }

        let dx = (3.0 * ax * u + 2.0 * bx) * u + cx;
        if dx.abs() > 1e-12 {
            let next_u = u - err / dx;
            if next_u > lo && next_u < hi {
                u = next_u;
            } else {
                u = 0.5 * (lo + hi);
            }
        } else {
            u = 0.5 * (lo + hi);
        }
    }

    for _ in 0..52 {
        if (hi - lo) < 1e-13 {
            break;
        }
        let x_val = ((ax * u + bx) * u + cx) * u;
        let err = x_val - x_target;
        if err.abs() < 1e-12 {
            return u;
        }
        if err < 0.0 {
            lo = u;
        } else {
            hi = u;
        }
        u = 0.5 * (lo + hi);
    }

    u
}

impl CubicBezierSegment2D {
    fn new(
        t0: f64,
        span_ticks: f64,
        slope_scale: f64,
        y0: f64,
        y1_val: f64,
        out_ease: Option<KeyframeEase>,
        in_ease: Option<KeyframeEase>,
    ) -> Self {
        let dy = y1_val - y0;
        let (x1, c1_y) = match out_ease {
            Some(ease) => {
                let inf = ease.influence;
                (inf, y0 + ease.speed * inf * slope_scale)
            }
            None => (1.0 / 3.0, y0 + dy / 3.0),
        };
        let (x2, c2_y) = match in_ease {
            Some(ease) => {
                let inf = ease.influence;
                (1.0 - inf, y1_val - ease.speed * inf * slope_scale)
            }
            None => (2.0 / 3.0, y1_val - dy / 3.0),
        };

        let cx = 3.0 * x1;
        let bx = 3.0 * x2 - 6.0 * x1;
        let ax = 1.0 - 3.0 * x2 + 3.0 * x1;

        let dy1 = c1_y - y0;
        let dy2 = c2_y - y0;
        let cy = 3.0 * dy1;
        let by = 3.0 * dy2 - 6.0 * dy1;
        let ay = dy - 3.0 * dy2 + 3.0 * dy1;

        Self {
            t0,
            span_ticks,
            slope_scale,
            x1,
            x2,
            ax,
            bx,
            cx,
            y0,
            y1_val,
            c1_y,
            c2_y,
            ay,
            by,
            cy,
        }
    }

    #[inline]
    fn eval_x(&self, u: f64) -> f64 {
        ((self.ax * u + self.bx) * u + self.cx) * u
    }

    #[inline]
    fn dx_du(&self, u: f64) -> f64 {
        (3.0 * self.ax * u + 2.0 * self.bx) * u + self.cx
    }

    #[inline]
    fn d2x_du2(&self, u: f64) -> f64 {
        6.0 * self.ax * u + 2.0 * self.bx
    }

    #[inline]
    fn eval_y(&self, u: f64) -> f64 {
        self.y0 + ((self.ay * u + self.by) * u + self.cy) * u
    }

    #[inline]
    fn dy_du(&self, u: f64) -> f64 {
        (3.0 * self.ay * u + 2.0 * self.by) * u + self.cy
    }

    #[inline]
    fn d2y_du2(&self, u: f64) -> f64 {
        6.0 * self.ay * u + 2.0 * self.by
    }

    #[inline]
    fn solve_u(&self, x_target: f64) -> f64 {
        solve_cubic_bezier_x(self.x1, self.x2, x_target)
    }

    #[inline]
    fn solve_u_at_tick(&self, output_offset: Flicks) -> f64 {
        let x_target = (output_offset.0 as f64 - self.t0) / self.span_ticks;
        self.solve_u(x_target)
    }

    fn eval_dy_dx(&self, u: f64) -> f64 {
        let dx = self.dx_du(u);
        if dx.abs() > 1e-12 {
            return self.dy_du(u) / (self.slope_scale * dx);
        }
        // L'Hôpital fallback for zero-influence endpoints (x1 == 0 or x2 == 1)
        let d2x = self.d2x_du2(u);
        if d2x.abs() > 1e-12 {
            return self.d2y_du2(u) / (self.slope_scale * d2x);
        }
        // Third-order fallback / clamped vertical inflection
        if (u <= 1e-9 || u >= 1.0 - 1e-9) && self.slope_scale.abs() > 1e-12 {
            return (self.y1_val - self.y0) / self.slope_scale;
        }
        let safe_dx = if dx >= 0.0 { 1e-9 } else { -1e-9 };
        self.dy_du(u) / (self.slope_scale * safe_dx)
    }

    fn eval_second_derivative_per_sec(&self, u: f64) -> f64 {
        let dx = self.dx_du(u);
        if dx.abs() <= 1e-6 {
            return 0.0;
        }
        let dy = self.dy_du(u);
        let d2x = self.d2x_du2(u);
        let d2y = self.d2y_du2(u);
        let num = d2y * dx - dy * d2x;
        let den = self.span_ticks * self.span_ticks * dx * dx * dx;
        (num / den) * FLICKS_PER_SECOND as f64
    }

    fn y_extrema_u(&self, out: &mut [f64; 2]) -> usize {
        // Solve 3*ay*u^2 + 2*by*u + cy = 0 for roots strictly inside (0, 1).
        let a = 3.0 * self.ay;
        let b = 2.0 * self.by;
        let c = self.cy;
        let mut count = 0;

        if a.abs() <= 1e-12 {
            if b.abs() > 1e-12 {
                let r = -c / b;
                if r > 0.0 && r < 1.0 {
                    out[count] = r;
                    count += 1;
                }
            }
            return count;
        }

        let disc = b * b - 4.0 * a * c;
        if disc < 0.0 {
            return 0;
        }
        let sqrt_d = disc.sqrt();
        for r in [(-b - sqrt_d) / (2.0 * a), (-b + sqrt_d) / (2.0 * a)] {
            if r > 0.0 && r < 1.0 && (count == 0 || (r - out[0]).abs() > 1e-12) {
                out[count] = r;
                count += 1;
            }
        }
        count
    }
}

fn validate_ease(idx: usize, label: &str, ease: Option<KeyframeEase>) -> Result<(), TimeError> {
    if let Some(e) = ease {
        if !e.speed.is_finite() {
            return Err(TimeError::InvalidSpeedCurve(format!(
                "bezier keyframe {idx} {label}.speed must be finite"
            )));
        }
        if !e.influence.is_finite() || !(0.0..=1.0).contains(&e.influence) {
            return Err(TimeError::InvalidSpeedCurve(format!(
                "bezier keyframe {idx} {label}.influence ({}) must be finite and within [0.0, 1.0]",
                e.influence
            )));
        }
    }
    Ok(())
}

fn validate_bezier_keyframes(
    keyframes: &[BezierSpeedKeyframe],
    require_source_offset: bool,
) -> Result<(), TimeError> {
    if keyframes.is_empty() {
        return Err(TimeError::InvalidSpeedCurve(
            "keyframed speed curve requires at least one keyframe".into(),
        ));
    }
    for (i, kf) in keyframes.iter().enumerate() {
        if !kf.speed.is_finite() {
            return Err(TimeError::InvalidSpeedCurve(format!(
                "bezier keyframe {i} has non-finite speed"
            )));
        }
        if require_source_offset && kf.source_offset.is_none() {
            return Err(TimeError::InvalidSpeedCurve(format!(
                "CubicBezierValue keyframe {i} requires source_offset"
            )));
        }
        validate_ease(i, "in_ease", kf.in_ease)?;
        validate_ease(i, "out_ease", kf.out_ease)?;
        if i > 0 && kf.output_offset <= keyframes[i - 1].output_offset {
            return Err(TimeError::InvalidSpeedCurve(format!(
                "bezier keyframe {i} output_offset ({}) must be strictly greater than previous ({})",
                kf.output_offset.0,
                keyframes[i - 1].output_offset.0
            )));
        }
    }
    Ok(())
}

fn build_bezier_value_segments(keyframes: &[BezierSpeedKeyframe]) -> Vec<CubicBezierSegment2D> {
    keyframes
        .windows(2)
        .map(|seg| {
            let a = seg[0];
            let b = seg[1];
            let t0 = a.output_offset.0 as f64;
            let span_ticks = (b.output_offset.0 - a.output_offset.0) as f64;
            let y0 = a.source_offset.expect("validated").0 as f64;
            let y1 = b.source_offset.expect("validated").0 as f64;
            CubicBezierSegment2D::new(t0, span_ticks, span_ticks, y0, y1, a.out_ease, b.in_ease)
        })
        .collect()
}

fn build_bezier_speed_segments(keyframes: &[BezierSpeedKeyframe]) -> Vec<CubicBezierSegment2D> {
    keyframes
        .windows(2)
        .map(|seg| {
            let a = seg[0];
            let b = seg[1];
            let t0 = a.output_offset.0 as f64;
            let span_ticks = (b.output_offset.0 - a.output_offset.0) as f64;
            let span_seconds = span_ticks / FLICKS_PER_SECOND as f64;
            CubicBezierSegment2D::new(
                t0,
                span_ticks,
                span_seconds,
                a.speed,
                b.speed,
                a.out_ease,
                b.in_ease,
            )
        })
        .collect()
}

fn build_bezier_value_lut(
    keyframes: &[BezierSpeedKeyframe],
    segments: &[CubicBezierSegment2D],
) -> Vec<SpeedLutEntry> {
    let mut lut = Vec::with_capacity(1 + segments.len() * BEZIER_LUT_SUBDIVISIONS_PER_SEGMENT);
    let first_s = keyframes[0].source_offset.expect("validated").0 as f64;
    lut.push(SpeedLutEntry {
        output_offset: keyframes[0].output_offset,
        source_offset_ticks: first_s,
    });

    for (idx, seg_kf) in keyframes.windows(2).enumerate() {
        let a = seg_kf[0];
        let b = seg_kf[1];
        let t0 = a.output_offset.0;
        let t1 = b.output_offset.0;
        let span = t1 - t0;
        let seg = &segments[idx];

        for step in 1..=BEZIER_LUT_SUBDIVISIONS_PER_SEGMENT {
            let t_curr = if step == BEZIER_LUT_SUBDIVISIONS_PER_SEGMENT {
                t1
            } else {
                t0 + ((span as i128 * step as i128) / BEZIER_LUT_SUBDIVISIONS_PER_SEGMENT as i128)
                    as i64
            };
            let u = if step == BEZIER_LUT_SUBDIVISIONS_PER_SEGMENT {
                1.0
            } else {
                seg.solve_u_at_tick(Flicks(t_curr))
            };
            lut.push(SpeedLutEntry {
                output_offset: Flicks(t_curr),
                source_offset_ticks: seg.eval_y(u),
            });
        }
    }

    lut
}

fn build_bezier_speed_lut(
    keyframes: &[BezierSpeedKeyframe],
    segments: &[CubicBezierSegment2D],
) -> Vec<SpeedLutEntry> {
    let mut lut = Vec::with_capacity(2 + segments.len() * BEZIER_LUT_SUBDIVISIONS_PER_SEGMENT);
    let mut acc = 0.0_f64;

    if keyframes[0].output_offset > Flicks::ZERO {
        lut.push(SpeedLutEntry {
            output_offset: Flicks::ZERO,
            source_offset_ticks: 0.0,
        });
        acc += (keyframes[0].output_offset.0 as f64) * keyframes[0].speed;
    } else if keyframes[0].output_offset < Flicks::ZERO {
        let origin_offset = integrate_bezier_speed_raw(keyframes, segments, Flicks::ZERO);
        acc = -origin_offset;
    }

    lut.push(SpeedLutEntry {
        output_offset: keyframes[0].output_offset,
        source_offset_ticks: acc,
    });

    for (idx, seg_kf) in keyframes.windows(2).enumerate() {
        let a = seg_kf[0];
        let b = seg_kf[1];
        let t0 = a.output_offset.0;
        let t1 = b.output_offset.0;
        let span = t1 - t0;
        let seg = &segments[idx];

        let mut u_prev = 0.0_f64;
        for step in 1..=BEZIER_LUT_SUBDIVISIONS_PER_SEGMENT {
            let t_curr = if step == BEZIER_LUT_SUBDIVISIONS_PER_SEGMENT {
                t1
            } else {
                t0 + ((span as i128 * step as i128) / BEZIER_LUT_SUBDIVISIONS_PER_SEGMENT as i128)
                    as i64
            };
            let u_curr = if step == BEZIER_LUT_SUBDIVISIONS_PER_SEGMENT {
                1.0
            } else {
                seg.solve_u_at_tick(Flicks(t_curr))
            };
            acc += seg.span_ticks
                * integrate_gl4(u_prev, u_curr, |u| seg.eval_y(u) * seg.dx_du(u));
            lut.push(SpeedLutEntry {
                output_offset: Flicks(t_curr),
                source_offset_ticks: acc,
            });
            u_prev = u_curr;
        }
    }

    lut
}

fn integrate_bezier_speed_raw(
    keyframes: &[BezierSpeedKeyframe],
    segments: &[CubicBezierSegment2D],
    target: Flicks,
) -> f64 {
    let mut acc = 0.0;
    for (idx, seg_kf) in keyframes.windows(2).enumerate() {
        let a = seg_kf[0];
        let b = seg_kf[1];
        if target <= a.output_offset {
            break;
        }
        let seg_end = target.min(b.output_offset);
        let seg = &segments[idx];
        let u_end = if seg_end == b.output_offset {
            1.0
        } else {
            seg.solve_u_at_tick(seg_end)
        };
        acc += seg.span_ticks * integrate_gl4(0.0, u_end, |u| seg.eval_y(u) * seg.dx_du(u));
    }
    let last = keyframes.last().unwrap();
    if target > last.output_offset {
        acc += (target.0 - last.output_offset.0) as f64 * last.speed;
    }
    acc
}

fn eval_bezier_value_source(
    keyframes: &[BezierSpeedKeyframe],
    segments: &[CubicBezierSegment2D],
    output_offset: Flicks,
) -> f64 {
    let first = keyframes[0];
    let first_s = first.source_offset.expect("validated").0 as f64;
    if output_offset < first.output_offset {
        let pre_slope = first.in_ease.map_or(0.0, |e| e.speed);
        return first_s + (output_offset.0 - first.output_offset.0) as f64 * pre_slope;
    }
    let last = keyframes.last().unwrap();
    let last_s = last.source_offset.expect("validated").0 as f64;
    if output_offset > last.output_offset {
        let post_slope = last.out_ease.map_or(0.0, |e| e.speed);
        return last_s + (output_offset.0 - last.output_offset.0) as f64 * post_slope;
    }
    if output_offset == last.output_offset || segments.is_empty() {
        return last_s;
    }
    let pos = keyframes.partition_point(|k| k.output_offset <= output_offset);
    let seg = &segments[pos - 1];
    let u = seg.solve_u_at_tick(output_offset);
    seg.eval_y(u)
}

fn eval_bezier_value_speed(
    keyframes: &[BezierSpeedKeyframe],
    segments: &[CubicBezierSegment2D],
    output_offset: Flicks,
) -> f64 {
    let first = keyframes[0];
    if output_offset < first.output_offset {
        return first.in_ease.map_or(0.0, |e| e.speed);
    }
    let last = keyframes.last().unwrap();
    if output_offset > last.output_offset {
        return last.out_ease.map_or(0.0, |e| e.speed);
    }
    if segments.is_empty() {
        return first.out_ease.map_or(0.0, |e| e.speed);
    }
    if output_offset == last.output_offset {
        return segments.last().unwrap().eval_dy_dx(1.0);
    }
    let pos = keyframes.partition_point(|k| k.output_offset <= output_offset);
    let seg = &segments[pos - 1];
    let u = seg.solve_u_at_tick(output_offset);
    seg.eval_dy_dx(u)
}

fn eval_bezier_speed_integral(
    lut: &[SpeedLutEntry],
    keyframes: &[BezierSpeedKeyframe],
    segments: &[CubicBezierSegment2D],
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
    let base = lut[pos - 1];
    if base.output_offset == output_offset {
        return base.source_offset_ticks;
    }

    let kf_pos = keyframes.partition_point(|k| k.output_offset <= output_offset);
    if kf_pos == 0 {
        let dt = (output_offset.0 - base.output_offset.0) as f64;
        return base.source_offset_ticks + dt * keyframes[0].speed;
    }
    let seg = &segments[kf_pos - 1];
    let u_base = seg.solve_u_at_tick(base.output_offset);
    let u_target = seg.solve_u_at_tick(output_offset);
    let delta =
        seg.span_ticks * integrate_gl4(u_base, u_target, |u| seg.eval_y(u) * seg.dx_du(u));
    base.source_offset_ticks + delta
}

fn eval_bezier_speed_value(
    keyframes: &[BezierSpeedKeyframe],
    segments: &[CubicBezierSegment2D],
    output_offset: Flicks,
) -> f64 {
    if output_offset <= keyframes[0].output_offset {
        return keyframes[0].speed;
    }
    let last = keyframes.last().unwrap();
    if output_offset >= last.output_offset || segments.is_empty() {
        return last.speed;
    }
    let pos = keyframes.partition_point(|k| k.output_offset <= output_offset);
    let seg = &segments[pos - 1];
    let u = seg.solve_u_at_tick(output_offset);
    seg.eval_y(u)
}
