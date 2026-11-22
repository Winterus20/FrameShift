use frameshift_time::{
    audio_samples_to_ticks, ticks_to_audio_samples_exact, ticks_to_audio_samples_floor,
    ticks_to_audio_samples_nearest, Flicks, ImpactCurveStyle, ImpactSolveRequest, ImpactSolver,
    PtsIndex, Rational32, SourceFitMode, SpeedCurve, SpeedKeyframe, TimeError, AUDIO_44_1_KHZ,
    AUDIO_48_KHZ, AUDIO_96_KHZ, FLICKS_PER_SECOND, FPS_23_976, FPS_24, FPS_25, FPS_29_97, FPS_30,
    FPS_59_94, FPS_60,
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
