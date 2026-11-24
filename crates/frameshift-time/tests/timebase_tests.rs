use frameshift_time::{
    anime_snap_in, anime_snap_out, apply_time_quantize, audio_samples_to_ticks,
    ticks_to_audio_samples_exact, ticks_to_audio_samples_floor, ticks_to_audio_samples_nearest,
    twixtor_ramp, BezierSpeedKeyframe, Flicks, ImpactCurveStyle, ImpactSolveRequest, ImpactSolver,
    KeyframeEase, PtsIndex, Rational32, SourceFitMode, SpeedCurve, SpeedCurveSpec, SpeedKeyframe,
    TimeError, TimeQuantizeSpec, AUDIO_44_1_KHZ, AUDIO_48_KHZ, AUDIO_96_KHZ, FLICKS_PER_SECOND,
    FPS_23_976, FPS_24, FPS_25, FPS_29_97, FPS_30, FPS_59_94, FPS_60,
};

struct SplitMix64(u64);

impl SplitMix64 {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn next_range_i64(&mut self, lo: i64, hi_inclusive: i64) -> i64 {
        let span = (hi_inclusive - lo) as u64 + 1;
        lo + (self.next_u64() % span) as i64
    }

    fn next_f64_unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / ((1u64 << 53) as f64)
    }
}

#[test]
fn standard_frame_rates_have_exact_flick_durations() {
    assert_eq!(FPS_24.exact_frame_duration().unwrap(), Some(Flicks(29_400_000)));
    assert_eq!(FPS_25.exact_frame_duration().unwrap(), Some(Flicks(28_224_000)));
    assert_eq!(FPS_30.exact_frame_duration().unwrap(), Some(Flicks(23_520_000)));
    assert_eq!(FPS_60.exact_frame_duration().unwrap(), Some(Flicks(11_760_000)));
    assert_eq!(
        FPS_23_976.exact_frame_duration().unwrap(),
        Some(Flicks(29_429_400))
    );
    assert_eq!(
        FPS_29_97.exact_frame_duration().unwrap(),
        Some(Flicks(23_543_520))
    );
    assert_eq!(
        FPS_59_94.exact_frame_duration().unwrap(),
        Some(Flicks(11_771_760))
    );
}

#[test]
fn ten_hour_zero_drift_video_frame_roundtrip_property() {
    let ten_hours_secs: i64 = 36_000;
    let rates = [FPS_23_976, FPS_29_97, FPS_60, FPS_24, FPS_25, FPS_30, FPS_59_94];
    let mut rng = SplitMix64(0x1234_5678_9ABC_DEF0);

    for fps in rates {
        // Exact number of frames in 10 hours (rounded up to cover >= 36,000s)
        let max_frame = (ten_hours_secs * fps.num as i64) / fps.den as i64;
        let end_ticks = fps.frame_to_ticks(max_frame).unwrap();
        assert_eq!(fps.ticks_to_frame_exact(end_ticks).unwrap(), Some(max_frame));
        assert_eq!(fps.ticks_to_frame_floor(end_ticks).unwrap(), max_frame);
        assert_eq!(fps.ticks_to_frame_nearest(end_ticks).unwrap(), max_frame);

        // Verify 5,000 pseudo-random frames across [0, 10h] round-trip with zero drift
        for _ in 0..5_000 {
            let f = rng.next_range_i64(0, max_frame);
            let ticks = fps.frame_to_ticks(f).unwrap();
            assert_eq!(fps.ticks_to_frame_exact(ticks).unwrap(), Some(f));
            assert_eq!(fps.ticks_to_frame_floor(ticks).unwrap(), f);
            assert_eq!(fps.ticks_to_frame_nearest(ticks).unwrap(), f);

            // Check half-frame minus 1 tick snaps back to `f`
            let frame_dur = fps.exact_frame_duration().unwrap().unwrap();
            let sub = Flicks(ticks.0 + frame_dur.0 / 2 - 1);
            assert_eq!(fps.ticks_to_frame_floor(sub).unwrap(), f);
            assert_eq!(fps.ticks_to_frame_nearest(sub).unwrap(), f);
        }
    }
}

#[test]
fn ten_hour_zero_drift_audio_sample_roundtrip_property() {
    let ten_hours_secs: i64 = 36_000;
    let sample_rates = [AUDIO_44_1_KHZ, AUDIO_48_KHZ, AUDIO_96_KHZ];
    let mut rng = SplitMix64(0xCAFE_BABE_DEAD_BEEF);

    for sr in sample_rates {
        let total_samples = ten_hours_secs * sr as i64;
        let end_ticks = audio_samples_to_ticks(total_samples, sr).unwrap();
        assert_eq!(end_ticks, Flicks::from_seconds_i64(ten_hours_secs).unwrap());
        assert_eq!(
            ticks_to_audio_samples_exact(end_ticks, sr).unwrap(),
            Some(total_samples)
        );

        for _ in 0..5_000 {
            let s = rng.next_range_i64(0, total_samples);
            let ticks = audio_samples_to_ticks(s, sr).unwrap();
            assert_eq!(ticks_to_audio_samples_exact(ticks, sr).unwrap(), Some(s));
            assert_eq!(ticks_to_audio_samples_floor(ticks, sr).unwrap(), s);
            assert_eq!(ticks_to_audio_samples_nearest(ticks, sr).unwrap(), s);
        }
    }
}

#[test]
fn ntsc_and_48khz_audio_exact_cadence_over_ten_hours() {
    // 23.976 fps @ 48 kHz: every single frame is exactly 2,002 audio samples
    let f_dur_23976 = FPS_23_976.frame_to_ticks(1).unwrap();
    assert_eq!(
        ticks_to_audio_samples_exact(f_dur_23976, AUDIO_48_KHZ).unwrap(),
        Some(2_002)
    );

    // At 10 hours (863,136 frames @ 23.976), audio samples = 863,136 * 2,002 exact
    let frames_10h = 863_136_i64;
    let t_10h = FPS_23_976.frame_to_ticks(frames_10h).unwrap();
    assert_eq!(
        ticks_to_audio_samples_exact(t_10h, AUDIO_48_KHZ).unwrap(),
        Some(frames_10h * 2_002)
    );

    // 29.97 fps @ 48 kHz: 5 frames = 8,008 samples exact (1601.6 samples/frame cadence)
    let t_5_frames = FPS_29_97.frame_to_ticks(5).unwrap();
    assert_eq!(
        ticks_to_audio_samples_exact(t_5_frames, AUDIO_48_KHZ).unwrap(),
        Some(8_008)
    );
}

#[test]
fn i128_prevents_intermediate_overflow_and_detects_real_overflow() {
    // 100,000 hours at 23.976 fps: frame_index * 705_600_000 * 1001 > i64::MAX,
    // yet final tick result (~2.54e17) fits comfortably in i64!
    let frames_100k_hours = 100_000_i64 * 3_600 * 24_000 / 1_001;
    let ticks = FPS_23_976.frame_to_ticks(frames_100k_hours).unwrap();
    assert_eq!(
        FPS_23_976.ticks_to_frame_exact(ticks).unwrap(),
        Some(frames_100k_hours)
    );

    // Genuine i64 overflow is cleanly reported
    assert_eq!(
        FPS_24.frame_to_ticks(i64::MAX),
        Err(TimeError::Overflow)
    );
    assert_eq!(
        Rational32::new(0, 1),
        Err(TimeError::InvalidRational { num: 0, den: 1 })
    );
}

#[test]
fn pts_index_vfr_binary_search_and_property() {
    // Empty index
    let empty = PtsIndex::new(vec![]).unwrap();
    assert!(empty.is_empty());
    assert_eq!(empty.lookup_frame(Flicks::ZERO), None);

    // Unsorted rejected
    let unsorted = PtsIndex::new(vec![Flicks(100), Flicks(300), Flicks(250)]);
    assert!(matches!(unsorted, Err(TimeError::UnsortedPts { index: 2, .. })));

    // VFR stream with irregular intervals: 20ms, 40ms, 10ms, 80ms
    let ms = FLICKS_PER_SECOND / 1_000;
    let pts = vec![
        Flicks(0),
        Flicks(20 * ms),
        Flicks(60 * ms),
        Flicks(70 * ms),
        Flicks(150 * ms),
    ];
    let index = PtsIndex::new(pts.clone()).unwrap();

    // Before first frame clamps to frame 0
    let m0 = index.lookup_frame(Flicks(-10 * ms)).unwrap();
    assert_eq!(m0.frame_index, 0);
    assert_eq!(m0.pts, Flicks(0));
    assert_eq!(m0.next_pts, Some(Flicks(20 * ms)));
    assert_eq!(m0.sub_frame_alpha, 0.0);

    // Midpoint between frame 1 (20ms) and frame 2 (60ms) -> 40ms => alpha 0.5
    let m1 = index.lookup_frame(Flicks(40 * ms)).unwrap();
    assert_eq!(m1.frame_index, 1);
    assert_eq!(m1.pts, Flicks(20 * ms));
    assert_eq!(m1.next_pts, Some(Flicks(60 * ms)));
    assert!((m1.sub_frame_alpha - 0.5).abs() < 1e-12);

    // Beyond last frame
    let m_end = index.lookup_frame(Flicks(500 * ms)).unwrap();
    assert_eq!(m_end.frame_index, 4);
    assert_eq!(m_end.pts, Flicks(150 * ms));
    assert_eq!(m_end.next_pts, None);
    assert_eq!(m_end.sub_frame_alpha, 0.0);

    // Seeded property test over 2,000 queries
    let mut rng = SplitMix64(0xDEAD_C0DE_1122_3344);
    for _ in 0..2_000 {
        let q = Flicks(rng.next_range_i64(-5 * ms, 200 * ms));
        let m = index.lookup_frame(q).unwrap();
        if q < pts[0] {
            assert_eq!(m.frame_index, 0);
        } else if q >= *pts.last().unwrap() {
            assert_eq!(m.frame_index, pts.len() - 1);
            assert_eq!(m.next_pts, None);
        } else {
            assert!(m.pts <= q);
            assert!(q < m.next_pts.unwrap());
            assert!(m.sub_frame_alpha >= 0.0 && m.sub_frame_alpha < 1.0);
        }
    }
}

#[test]
fn speed_curve_forward_freeze_reverse_and_cubic_integral() {
    let sec = FLICKS_PER_SECOND;

    // Constant & Freeze
    let id = SpeedCurve::constant(1.0).unwrap();
    assert_eq!(id.source_offset_at(Flicks(3 * sec)), Flicks(3 * sec));

    let rev = SpeedCurve::constant(-1.5).unwrap();
    assert_eq!(rev.source_offset_at(Flicks(2 * sec)), Flicks(-3 * sec));

    let frz = SpeedCurve::freeze();
    assert_eq!(frz.source_offset_at(Flicks(10 * sec)), Flicks::ZERO);

    // PiecewiseLinear: [0s -> 1.0], [2s -> 3.0], [4s -> 0.0], [6s -> -1.0]
    // Area [0, 2s] = 0.5*(1+3)*2 = 4.0s
    // Area [2, 4s] = 0.5*(3+0)*2 = 3.0s -> total 7.0s
    // Area [4, 6s] = 0.5*(0-1)*2 = -1.0s -> total 6.0s
    let pwl = SpeedCurve::piecewise_linear(vec![
        SpeedKeyframe { output_offset: Flicks(0), speed: 1.0, tangent: None },
        SpeedKeyframe { output_offset: Flicks(2 * sec), speed: 3.0, tangent: None },
        SpeedKeyframe { output_offset: Flicks(4 * sec), speed: 0.0, tangent: None },
        SpeedKeyframe { output_offset: Flicks(6 * sec), speed: -1.0, tangent: None },
    ])
    .unwrap();

    assert!((pwl.source_offset_at(Flicks(2 * sec)).0 - 4 * sec).abs() <= 1);
    assert!((pwl.source_offset_at(Flicks(4 * sec)).0 - 7 * sec).abs() <= 1);
    assert!((pwl.source_offset_at(Flicks(6 * sec)).0 - 6 * sec).abs() <= 1);

    // Peak source offset was 7s at 4s even though end is 6s (because of reverse segment!)
    let (min_s, max_s) = pwl.source_extrema(Flicks::ZERO, Flicks(6 * sec));
    assert_eq!(min_s, Flicks::ZERO);
    assert!((max_s.0 - 7 * sec).abs() <= 1);

    // CubicHermite with zero tangents: exact integral of [0->1s] from 0.5 to 2.5 is 1.5s!
    let cubic = SpeedCurve::cubic_hermite(vec![
        SpeedKeyframe { output_offset: Flicks::ZERO, speed: 0.5, tangent: Some(0.0) },
        SpeedKeyframe { output_offset: Flicks(sec), speed: 2.5, tangent: Some(0.0) },
    ])
    .unwrap();
    assert!((cubic.source_offset_at(Flicks(sec)).0 - (3 * sec / 2)).abs() <= 1);
}

#[test]
fn impact_solver_valid_and_impossible_constraints() {
    let sec = FLICKS_PER_SECOND;

    // Feasible exact alignment: 4s source with impact at 1s -> 4s output with beat at 2s
    // Pre avg speed = 1/2 = 0.5x, Post avg speed = 3/2 = 1.5x (both inside [0.25, 3.0])
    for style in [ImpactCurveStyle::TwoSegmentConstant, ImpactCurveStyle::SmoothRamp] {
        let req = ImpactSolveRequest {
            source_bounds_start: Flicks::ZERO,
            source_bounds_end: Flicks(4 * sec),
            source_impact: Flicks(sec),
            output_start: Flicks(10 * sec),
            output_end: Flicks(14 * sec),
            target_output: Flicks(12 * sec),
            min_speed: 0.25,
            max_speed: 3.0,
            style,
            fit_mode: SourceFitMode::ExactRange,
        };

        let sol = ImpactSolver::solve(&req).expect("feasible request must solve");
        assert!((sol.pre_impact_avg_speed - 0.5).abs() < 1e-9);
        assert!((sol.post_impact_avg_speed - 1.5).abs() < 1e-9);
        assert!((sol.achieved_source_at_target.0 - req.source_impact.0).abs() <= 1);
    }

    // Impossible MinSpeed: only 0.2s of source before impact, but 2.0s of output before target,
    // and min_speed = 0.5x -> needs 1.0s of source. Must fail loudly without altering bounds!
    let impossible = ImpactSolveRequest {
        source_bounds_start: Flicks::ZERO,
        source_bounds_end: Flicks(3 * sec),
        source_impact: Flicks(sec / 5),
        output_start: Flicks::ZERO,
        output_end: Flicks(3 * sec),
        target_output: Flicks(2 * sec),
        min_speed: 0.5,
        max_speed: 4.0,
        style: ImpactCurveStyle::TwoSegmentConstant,
        fit_mode: SourceFitMode::ExactRange,
    };

    let err = ImpactSolver::solve(&impossible).expect_err("impossible constraint must error");
    assert!(!err.failures.is_empty());
    assert!(!err.suggestions.is_empty());
    assert!(err.suggestions[0].suggested_target_output.is_some());

    // Property test: 500 seeded random impact problems
    let mut rng = SplitMix64(0x5555_AAAA_1234_9999);
    for _ in 0..500 {
        let src_len = rng.next_range_i64(1, 20) * (sec / 4);
        let src_impact = rng.next_range_i64(1, (src_len / (sec / 4)).max(1)) * (sec / 4);
        let src_impact = src_impact.min(src_len);
        let out_len = rng.next_range_i64(1, 20) * (sec / 4);
        let target_off = rng.next_range_i64(0, (out_len / (sec / 4)).max(1)) * (sec / 4);
        let min_speed = 0.25 + rng.next_f64_unit() * 0.5;
        let max_speed = min_speed + 0.5 + rng.next_f64_unit() * 3.0;

        let req = ImpactSolveRequest {
            source_bounds_start: Flicks::ZERO,
            source_bounds_end: Flicks(src_len),
            source_impact: Flicks(src_impact),
            output_start: Flicks(sec),
            output_end: Flicks(sec + out_len),
            target_output: Flicks(sec + target_off),
            min_speed,
            max_speed,
            style: ImpactCurveStyle::TwoSegmentConstant,
            fit_mode: SourceFitMode::ExactRange,
        };

        match ImpactSolver::solve(&req) {
            Ok(sol) => {
                assert!(sol.pre_impact_avg_speed >= min_speed - 1e-9);
                assert!(sol.pre_impact_avg_speed <= max_speed + 1e-9);
                assert!(sol.post_impact_avg_speed >= min_speed - 1e-9);
                assert!(sol.post_impact_avg_speed <= max_speed + 1e-9);
                assert!((sol.achieved_source_at_target.0 - req.source_impact.0).abs() <= 1);
            }
            Err(violation) => {
                assert!(!violation.failures.is_empty());
                assert!(!violation.suggestions.is_empty());
            }
        }
    }
}

#[test]
fn cubic_bezier_value_newton_raphson_extrema_and_serde() {
    let sec = FLICKS_PER_SECOND;

    // 1. Symmetric ease: 0 -> 2s output maps 0 -> 4s source, zero speed at both ends
    let ease = KeyframeEase {
        speed: 0.0,
        influence: 0.8,
    };
    let curve = SpeedCurve::cubic_bezier_value(vec![
        BezierSpeedKeyframe::value(Flicks::ZERO, Flicks::ZERO).with_out_ease(ease),
        BezierSpeedKeyframe::value(Flicks(2 * sec), Flicks(4 * sec)).with_in_ease(ease),
    ])
    .unwrap();

    assert_eq!(curve.source_offset_at(Flicks::ZERO), Flicks::ZERO);
    assert_eq!(curve.source_offset_at(Flicks(2 * sec)), Flicks(4 * sec));
    // Symmetry -> midpoint hits exact half (2s source at 1s output)
    assert!((curve.source_offset_at(Flicks(sec)).0 - 2 * sec).abs() <= 1);
    assert!(curve.instant_speed_at(Flicks::ZERO).abs() < 1e-9);
    assert!(curve.instant_speed_at(Flicks(2 * sec)).abs() < 1e-9);
    assert!(curve.instant_speed_at(Flicks(sec)) > 2.0);

    // Numerical finite-difference verification of instant_speed_derivative_at
    let t_probe = Flicks(sec / 2);
    let h = Flicks(1_000);
    let fd_deriv = (curve.instant_speed_at(t_probe + h) - curve.instant_speed_at(t_probe - h))
        / (2.0 * h.to_seconds_f64());
    let analytical_deriv = curve.instant_speed_derivative_at(t_probe);
    assert!(
        (fd_deriv - analytical_deriv).abs() < 1e-3,
        "fd={fd_deriv}, analytical={analytical_deriv}"
    );

    // 2. Extreme S-curve inflection (influence = 1.0 at both ends: dx/du == 0 at u=0.5)
    let extreme = SpeedCurve::cubic_bezier_value(vec![
        BezierSpeedKeyframe::value(Flicks::ZERO, Flicks::ZERO).with_out_ease(KeyframeEase {
            speed: 0.0,
            influence: 1.0,
        }),
        BezierSpeedKeyframe::value(Flicks(sec), Flicks(sec)).with_in_ease(KeyframeEase {
            speed: 0.0,
            influence: 1.0,
        }),
    ])
    .unwrap();
    assert!((extreme.source_offset_at(Flicks(sec / 2)).0 - sec / 2).abs() <= 2);
    assert!(extreme.instant_speed_at(Flicks(sec / 2)).is_finite());
    assert!(extreme.instant_speed_derivative_at(Flicks(sec / 2)).is_finite());

    // 3. Zero-influence corner uses L'Hopital cleanly without NaN
    let zero_inf = SpeedCurve::cubic_bezier_value(vec![
        BezierSpeedKeyframe::value(Flicks::ZERO, Flicks::ZERO).with_out_ease(KeyframeEase {
            speed: 1.0,
            influence: 0.0,
        }),
        BezierSpeedKeyframe::value(Flicks(sec), Flicks(sec)).with_in_ease(KeyframeEase {
            speed: 1.0,
            influence: 0.0,
        }),
    ])
    .unwrap();
    assert!((zero_inf.source_offset_at(Flicks(sec / 2)).0 - sec / 2).abs() <= 1);
    assert!((zero_inf.instant_speed_at(Flicks::ZERO) - 1.0).abs() < 1e-9);

    // 4. Reverse dip + overshoot extrema detection
    let overshoot = SpeedCurve::cubic_bezier_value(vec![
        BezierSpeedKeyframe::value(Flicks::ZERO, Flicks::ZERO).with_out_ease(KeyframeEase {
            speed: -2.0,
            influence: 0.5,
        }),
        BezierSpeedKeyframe::value(Flicks(2 * sec), Flicks(sec)).with_in_ease(KeyframeEase {
            speed: -2.0,
            influence: 0.5,
        }),
    ])
    .unwrap();
    let (min_s, max_s) = overshoot.source_extrema(Flicks::ZERO, Flicks(2 * sec));
    assert!(min_s < Flicks::ZERO, "expected negative minimum, got {min_s}");
    assert!(max_s > Flicks(sec), "expected overshoot > 1s, got {max_s}");

    // 5. Extrapolation outside keyframe span
    let single_extrap = SpeedCurve::cubic_bezier_value(vec![
        BezierSpeedKeyframe::value(Flicks(sec), Flicks(2 * sec))
            .with_in_ease(KeyframeEase {
                speed: 0.5,
                influence: 0.333,
            })
            .with_out_ease(KeyframeEase {
                speed: 2.0,
                influence: 0.333,
            }),
    ])
    .unwrap();
    assert_eq!(single_extrap.source_offset_at(Flicks::ZERO), Flicks(3 * sec / 2));
    assert_eq!(single_extrap.source_offset_at(Flicks(2 * sec)), Flicks(4 * sec));

    // 6. Default & #[serde(default)] partial-map deserialization checks
    assert_eq!(
        KeyframeEase::default(),
        KeyframeEase {
            speed: 0.0,
            influence: 0.333
        }
    );
    let empty_ease: KeyframeEase = serde::Deserialize::deserialize(
        serde::de::value::MapDeserializer::<_, serde::de::value::Error>::new(
            std::iter::empty::<(&str, f64)>(),
        ),
    )
    .unwrap();
    assert_eq!(empty_ease, KeyframeEase::default());

    let partial_ease: KeyframeEase = serde::Deserialize::deserialize(
        serde::de::value::MapDeserializer::<_, serde::de::value::Error>::new(
            [("speed", 2.5_f64)].into_iter(),
        ),
    )
    .unwrap();
    assert_eq!(
        partial_ease,
        KeyframeEase {
            speed: 2.5,
            influence: 0.333
        }
    );

    let partial_kf: BezierSpeedKeyframe = serde::Deserialize::deserialize(
        serde::de::value::MapDeserializer::<_, serde::de::value::Error>::new(
            [("speed", 1.75_f64)].into_iter(),
        ),
    )
    .unwrap();
    assert_eq!(partial_kf.output_offset, Flicks::ZERO);
    assert_eq!(partial_kf.speed, 1.75);
    assert_eq!(partial_kf.source_offset, None);
    assert_eq!(partial_kf.in_ease, None);
    assert_eq!(partial_kf.out_ease, None);

    // 7. Validation checks
    assert!(SpeedCurve::cubic_bezier_value(vec![
        BezierSpeedKeyframe::speed(Flicks::ZERO, 1.0),
    ])
    .is_err());
    assert!(SpeedCurve::cubic_bezier_value(vec![
        BezierSpeedKeyframe::value(Flicks::ZERO, Flicks::ZERO).with_out_ease(KeyframeEase {
            speed: 0.0,
            influence: 1.2,
        }),
        BezierSpeedKeyframe::value(Flicks(sec), Flicks(sec)),
    ])
    .is_err());
}

#[test]
fn cubic_bezier_speed_broken_handles_and_64_subdivision_gl4_lut() {
    let sec = FLICKS_PER_SECOND;

    // 1. Broken handles at t = 1s (arriving flat from 0.5x, jumping down steeply toward 3.0x? Wait: slope jump!)
    let curve = SpeedCurve::cubic_bezier_speed(vec![
        BezierSpeedKeyframe::speed(Flicks::ZERO, 0.5).with_out_ease(KeyframeEase {
            speed: 0.0,
            influence: 0.8,
        }),
        BezierSpeedKeyframe::speed(Flicks(sec), 2.5)
            .with_in_ease(KeyframeEase {
                speed: 0.0,
                influence: 0.2,
            })
            .with_out_ease(KeyframeEase {
                speed: -4.0,
                influence: 0.6,
            }),
        BezierSpeedKeyframe::speed(Flicks(2 * sec), 0.5).with_in_ease(KeyframeEase {
            speed: 0.0,
            influence: 0.7,
        }),
    ])
    .unwrap();

    // 2 segments * 64 subdivisions + start knot = 129 entries
    assert_eq!(curve.lut().len(), 1 + 2 * 64);

    // Broken handle: speed at 1s is 2.5, right-hand derivative is -4.0 (while incoming left slope was 0.0)
    assert!((curve.instant_speed_at(Flicks(sec)) - 2.5).abs() < 1e-12);
    assert!((curve.instant_speed_derivative_at(Flicks(sec)) - (-4.0)).abs() < 1e-6);

    // Closed-form integral check on [0, 1s]:
    // W(out=0.8, in=0.2) = 0.5 + 0.3 * (0.2 - 0.8) = 0.32
    // avg_speed_01 = 0.5 + (2.5 - 0.5) * 0.32 = 1.14 -> 1.14s in ticks
    let expected_at_1s = 1.14 * sec as f64;
    assert!(
        (curve.source_offset_f64(Flicks(sec)) - expected_at_1s).abs() < 1e-4,
        "got {}, expected {expected_at_1s}",
        curve.source_offset_f64(Flicks(sec))
    );

    // Extreme influence = 1.0 integration exactness (W = 0.5 + 0.3 * (0 - 1) = 0.2 -> avg = 1.0 + 3.0*0.2 = 1.6)
    let extreme_speed = SpeedCurve::cubic_bezier_speed(vec![
        BezierSpeedKeyframe::speed(Flicks::ZERO, 1.0).with_out_ease(KeyframeEase {
            speed: 0.0,
            influence: 1.0,
        }),
        BezierSpeedKeyframe::speed(Flicks(sec), 4.0).with_in_ease(KeyframeEase {
            speed: 0.0,
            influence: 0.0,
        }),
    ])
    .unwrap();
    assert!(
        (extreme_speed.source_offset_f64(Flicks(sec)) - 1.6 * sec as f64).abs() < 1e-4,
        "extreme integral mismatch: {}",
        extreme_speed.source_offset_f64(Flicks(sec)) / sec as f64
    );

    // Anchoring when first keyframe starts at negative output_offset
    let shifted = SpeedCurve::cubic_bezier_speed(vec![
        BezierSpeedKeyframe::speed(Flicks(-sec), 1.0),
        BezierSpeedKeyframe::speed(Flicks(sec), 3.0),
    ])
    .unwrap();
    assert_eq!(shifted.source_offset_at(Flicks::ZERO), Flicks::ZERO);

    // Check linear equivalence when ease handles are omitted
    let linear_ramp = SpeedCurve::cubic_bezier_speed(vec![
        BezierSpeedKeyframe::speed(Flicks::ZERO, 1.0),
        BezierSpeedKeyframe::speed(Flicks(sec), 3.0),
    ])
    .unwrap();
    assert!(matches!(linear_ramp.spec(), SpeedCurveSpec::CubicBezierSpeed { .. }));
    assert!((linear_ramp.source_offset_f64(Flicks(sec)) - 2.0 * sec as f64).abs() < 1e-4);

    // Reverse + forward source_extrema check on CubicBezierSpeed
    let rev_fwd = SpeedCurve::cubic_bezier_speed(vec![
        BezierSpeedKeyframe::speed(Flicks::ZERO, -2.0),
        BezierSpeedKeyframe::speed(Flicks(sec), 2.0),
    ])
    .unwrap();
    let (rev_min, rev_max) = rev_fwd.source_extrema(Flicks::ZERO, Flicks(sec));
    assert!(rev_min < Flicks::ZERO, "expected dip below 0, got {rev_min}");
    assert_eq!(rev_max, Flicks::ZERO);
}

#[test]
fn time_quantize_posterize_and_stutter_determinism() {
    let f24 = FPS_24.exact_frame_duration().unwrap().unwrap();

    // None is identity
    assert_eq!(
        apply_time_quantize(Flicks(123_456), FPS_24, &TimeQuantizeSpec::None),
        Flicks(123_456)
    );

    // Posterize 12 fps inside 24 fps project (on twos)
    let p12 = TimeQuantizeSpec::PosterizeFps {
        fps: Rational32::new(12, 1).unwrap(),
    };
    assert_eq!(apply_time_quantize(Flicks::ZERO, FPS_24, &p12), Flicks::ZERO);
    assert_eq!(apply_time_quantize(f24, FPS_24, &p12), Flicks::ZERO);
    assert_eq!(
        apply_time_quantize(Flicks(2 * f24.0), FPS_24, &p12),
        Flicks(2 * f24.0)
    );
    assert_eq!(
        apply_time_quantize(Flicks(3 * f24.0), FPS_24, &p12),
        Flicks(2 * f24.0)
    );
    // Negative ticks use Euclidean floor
    assert_eq!(
        apply_time_quantize(Flicks(-f24.0), FPS_24, &p12),
        Flicks(-2 * f24.0)
    );

    // Idempotency even for non-exact rate (11 fps)
    let p11 = TimeQuantizeSpec::PosterizeFps {
        fps: Rational32::new(11, 1).unwrap(),
    };
    for i in -5..30 {
        let t = Flicks(i * f24.0);
        let q1 = apply_time_quantize(t, FPS_24, &p11);
        let q2 = apply_time_quantize(q1, FPS_24, &p11);
        assert_eq!(q1, q2, "PosterizeFps must be idempotent for frame {i}");
    }

    // StutterPattern: anime "on threes" (hold 3, step 3)
    let on_threes = TimeQuantizeSpec::StutterPattern {
        hold_frames: 3,
        step_frames: 3,
    };
    let expected_threes = [0, 0, 0, 3, 3, 3, 6, 6, 6];
    for (idx, &exp_frame) in expected_threes.iter().enumerate() {
        let t = Flicks(idx as i64 * f24.0 + 100);
        assert_eq!(
            apply_time_quantize(t, FPS_24, &on_threes),
            Flicks(exp_frame * f24.0)
        );
    }

    // Asymmetric StutterPattern (hold 3, step 2)
    let slow_stutter = TimeQuantizeSpec::StutterPattern {
        hold_frames: 3,
        step_frames: 2,
    };
    let expected_asym = [0, 0, 0, 2, 2, 2, 4, 4, 4];
    for (idx, &exp_frame) in expected_asym.iter().enumerate() {
        let t = Flicks(idx as i64 * f24.0);
        assert_eq!(
            apply_time_quantize(t, FPS_24, &slow_stutter),
            Flicks(exp_frame * f24.0)
        );
    }

    // Safe fallback on degenerate hold_frames = 0 or invalid rational
    assert_eq!(
        apply_time_quantize(
            Flicks(42_000),
            FPS_24,
            &TimeQuantizeSpec::StutterPattern {
                hold_frames: 0,
                step_frames: 2
            }
        ),
        Flicks(42_000)
    );
    assert_eq!(
        apply_time_quantize(
            Flicks(42_000),
            FPS_24,
            &TimeQuantizeSpec::PosterizeFps {
                fps: Rational32 { num: 0, den: 1 }
            }
        ),
        Flicks(42_000)
    );

    // Default & #[serde(default)] check for TimeQuantizeSpec::StutterPattern
    assert_eq!(TimeQuantizeSpec::default(), TimeQuantizeSpec::None);
    let default_stutter: TimeQuantizeSpec = serde::Deserialize::deserialize(
        serde::de::value::MapDeserializer::<_, serde::de::value::Error>::new(
            [("kind", "stutter_pattern")].into_iter(),
        ),
    )
    .unwrap();
    assert_eq!(
        default_stutter,
        TimeQuantizeSpec::StutterPattern {
            hold_frames: 1,
            step_frames: 1
        }
    );
}

#[test]
fn impact_solver_anime_snap_presets_and_property_bounds() {
    let sec = FLICKS_PER_SECOND;

    // 1. Test all three preset helpers on standard alignment scenario
    for style in [anime_snap_in(), anime_snap_out(), twixtor_ramp()] {
        let req = ImpactSolveRequest {
            source_bounds_start: Flicks::ZERO,
            source_bounds_end: Flicks(4 * sec),
            source_impact: Flicks(sec),
            output_start: Flicks(10 * sec),
            output_end: Flicks(14 * sec),
            target_output: Flicks(12 * sec),
            min_speed: 0.25,
            max_speed: 3.0,
            style,
            fit_mode: SourceFitMode::ExactRange,
        };

        let sol = ImpactSolver::solve(&req).expect("AnimeSnap preset must solve feasible problem");
        assert!((sol.achieved_source_at_target.0 - req.source_impact.0).abs() <= 1);
        assert!(
            (sol.speed_curve.source_offset_at(Flicks(4 * sec)).0 - 4 * sec).abs() <= 2,
            "end integral drifted"
        );

        // Peak at impact must exceed pre-impact average and drop toward settle afterward
        let v_start = sol.speed_curve.instant_speed_at(Flicks::ZERO);
        let v_impact = sol.speed_curve.instant_speed_at(Flicks(2 * sec));
        let v_end = sol.speed_curve.instant_speed_at(Flicks(4 * sec));
        assert!(
            v_impact > sol.pre_impact_avg_speed && v_impact > v_start && v_impact >= v_end,
            "expected peak at impact: start={v_start}, impact={v_impact}, end={v_end}"
        );
    }

    // 2. Reject invalid AnimeSnap parameters cleanly
    let bad_bias = ImpactSolveRequest {
        source_bounds_start: Flicks::ZERO,
        source_bounds_end: Flicks(2 * sec),
        source_impact: Flicks(sec),
        output_start: Flicks::ZERO,
        output_end: Flicks(2 * sec),
        target_output: Flicks(sec),
        min_speed: 0.25,
        max_speed: 3.0,
        style: ImpactCurveStyle::AnimeSnap {
            peak_bias: 1.5,
            influence: 0.8,
        },
        fit_mode: SourceFitMode::ExactRange,
    };
    assert!(ImpactSolver::solve(&bad_bias).is_err());

    // 3. Property test across 500 seeded random problems for AnimeSnap presets
    let styles = [anime_snap_in(), anime_snap_out(), twixtor_ramp()];
    let mut rng = SplitMix64(0xABCD_1234_9876_5432);
    for idx in 0..500 {
        let src_len = rng.next_range_i64(1, 20) * (sec / 4);
        let src_impact = rng.next_range_i64(0, (src_len / (sec / 4)).max(1)) * (sec / 4);
        let src_impact = src_impact.min(src_len);
        let out_len = rng.next_range_i64(1, 20) * (sec / 4);
        let target_off = rng.next_range_i64(0, (out_len / (sec / 4)).max(1)) * (sec / 4);
        let min_speed = 0.25 + rng.next_f64_unit() * 0.5;
        let max_speed = min_speed + 0.5 + rng.next_f64_unit() * 3.0;
        let style = styles[idx % styles.len()];

        let req = ImpactSolveRequest {
            source_bounds_start: Flicks::ZERO,
            source_bounds_end: Flicks(src_len),
            source_impact: Flicks(src_impact),
            output_start: Flicks(sec),
            output_end: Flicks(sec + out_len),
            target_output: Flicks(sec + target_off),
            min_speed,
            max_speed,
            style,
            fit_mode: SourceFitMode::ExactRange,
        };

        if let Ok(sol) = ImpactSolver::solve(&req) {
            assert!(
                (sol.achieved_source_at_target.0 - req.source_impact.0).abs() <= 1,
                "impact integral error exceeded 1 flick at iter {idx}"
            );
            let total_src = sol.effective_source_end.0 - sol.effective_source_start.0;
            assert!(
                (sol.speed_curve.source_offset_at(Flicks(out_len)).0 - total_src).abs() <= 3,
                "end integral error exceeded 3 flicks at iter {idx}"
            );
            for sample in 0..=16 {
                let t = Flicks((out_len as i128 * sample as i128 / 16) as i64);
                let spd = sol.speed_curve.instant_speed_at(t);
                assert!(
                    spd >= min_speed - 1e-9 && spd <= max_speed + 1e-9,
                    "speed {spd} out of [{min_speed}, {max_speed}] at sample {sample} iter {idx}"
                );
            }
        }
    }
}
