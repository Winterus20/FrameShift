use frameshift_time::{ExprContext, ExprError, SafeExpr};

fn sample_ctx() -> ExprContext {
    ExprContext {
        t_sec: 1.5,
        output_tick: 1_058_400_000,
        beat_phase: 0.25,
        audio_rms: 0.8,
        event_delta_sec: -0.5,
        progress: 0.5,
    }
}

#[test]
fn safe_expr_evaluates_arithmetic_precedence_unary_and_variables() {
    let ctx = sample_ctx();

    let expr = SafeExpr::parse("1.0 + 2.0 * 3.0 - -4.0 / 2.0").unwrap();
    assert!((expr.eval_default(&ctx).unwrap() - 9.0).abs() < 1e-12);

    let vars = SafeExpr::parse(
        "t_sec + beat_phase * 2.0 + audio_rms - abs(event_delta_sec) + progress + output_tick * 1e-9",
    )
    .unwrap();
    let expected = 1.5 + 0.25 * 2.0 + 0.8 - 0.5 + 0.5 + (1_058_400_000.0 * 1e-9);
    assert!((vars.eval_default(&ctx).unwrap() - expected).abs() < 1e-12);
}

#[test]
fn safe_expr_supports_all_builtin_functions() {
    let ctx = sample_ctx();

    let sin_cos = SafeExpr::parse("sin(0.0) + cos(0.0)").unwrap();
    assert!((sin_cos.eval_default(&ctx).unwrap() - 1.0).abs() < 1e-12);

    let min_max = SafeExpr::parse("min(3.0, max(1.0, 2.5))").unwrap();
    assert!((min_max.eval_default(&ctx).unwrap() - 2.5).abs() < 1e-12);

    let clamped = SafeExpr::parse("clamp(audio_rms * 2.0, 0.0, 1.2)").unwrap();
    assert!((clamped.eval_default(&ctx).unwrap() - 1.2).abs() < 1e-12);

    let smooth = SafeExpr::parse("smoothstep(0.0, 1.0, progress)").unwrap();
    assert!((smooth.eval_default(&ctx).unwrap() - 0.5).abs() < 1e-12);

    let lerped = SafeExpr::parse("lerp(10.0, 20.0, beat_phase)").unwrap();
    assert!((lerped.eval_default(&ctx).unwrap() - 12.5).abs() < 1e-12);

    let stepped_low = SafeExpr::parse("step(0.6, progress)").unwrap();
    assert_eq!(stepped_low.eval_default(&ctx).unwrap(), 0.0);

    let stepped_high = SafeExpr::parse("step(0.5, progress)").unwrap();
    assert_eq!(stepped_high.eval_default(&ctx).unwrap(), 1.0);

    let powered = SafeExpr::parse("pow(2.0, 3.0)").unwrap();
    assert!((powered.eval_default(&ctx).unwrap() - 8.0).abs() < 1e-12);
}

#[test]
fn safe_expr_rejects_errors_deterministically_without_panicking() {
    let ctx = sample_ctx();

    // Division by zero (literal and dynamic)
    let div_zero = SafeExpr::parse("1.0 / (progress - 0.5)").unwrap();
    assert_eq!(div_zero.eval_default(&ctx), Err(ExprError::DivisionByZero));

    // NaN from fractional power of negative base
    let neg_sqrt = SafeExpr::parse("pow(-1.0, 0.5)").unwrap();
    assert_eq!(neg_sqrt.eval_default(&ctx), Err(ExprError::NonFiniteValue));

    // Infinity from overflow
    let inf_expr = SafeExpr::parse("pow(10.0, 400.0)").unwrap();
    assert_eq!(inf_expr.eval_default(&ctx), Err(ExprError::NonFiniteValue));

    // Non-finite context input
    let bad_ctx = ExprContext {
        audio_rms: f64::NAN,
        ..ctx
    };
    let rms_expr = SafeExpr::parse("audio_rms + 1.0").unwrap();
    assert_eq!(
        rms_expr.eval_default(&bad_ctx),
        Err(ExprError::NonFiniteValue)
    );

    // Inverted clamp bounds must return error, never panic
    let bad_clamp = SafeExpr::parse("clamp(0.5, 2.0, 1.0)").unwrap();
    assert!(matches!(
        bad_clamp.eval_default(&ctx),
        Err(ExprError::InvalidArgument(_))
    ));

    // smoothstep degenerate edges
    let bad_smooth_eq = SafeExpr::parse("smoothstep(1.0, 1.0, 0.5)").unwrap();
    assert_eq!(
        bad_smooth_eq.eval_default(&ctx),
        Err(ExprError::DivisionByZero)
    );
    let bad_smooth_inv = SafeExpr::parse("smoothstep(2.0, 1.0, 1.5)").unwrap();
    assert!(matches!(
        bad_smooth_inv.eval_default(&ctx),
        Err(ExprError::InvalidArgument(_))
    ));

    // Unknown identifier & function & bad arity & syntax
    assert_eq!(
        SafeExpr::parse("unknown_var + 1.0"),
        Err(ExprError::UnknownIdentifier("unknown_var".into()))
    );
    assert_eq!(
        SafeExpr::parse("sqrt(4.0)"),
        Err(ExprError::UnknownFunction("sqrt".into()))
    );
    assert_eq!(
        SafeExpr::parse("sin(1.0, 2.0)"),
        Err(ExprError::InvalidArity {
            func: "sin".into(),
            expected: 1,
            actual: 2,
        })
    );
    assert!(matches!(
        SafeExpr::parse(""),
        Err(ExprError::SyntaxError(_))
    ));
    assert!(matches!(
        SafeExpr::parse("1.0 + * 2.0"),
        Err(ExprError::SyntaxError(_))
    ));
    assert!(matches!(
        SafeExpr::parse("(1.0 + 2.0"),
        Err(ExprError::SyntaxError(_))
    ));
}

#[test]
fn safe_expr_enforces_fuel_and_depth_limits() {
    let ctx = sample_ctx();

    // `1.0 + 2.0 * 3.0` has 5 AST nodes: Literal(1), Literal(2), Literal(3), Mul, Add.
    let expr = SafeExpr::parse("1.0 + 2.0 * 3.0").unwrap();
    assert_eq!(expr.node_count(), 5);
    assert_eq!(
        expr.eval(&ctx, 4),
        Err(ExprError::FuelExhausted { max_fuel: 4 })
    );
    assert!((expr.eval(&ctx, 5).unwrap() - 7.0).abs() < 1e-12);

    // Deeply nested expression exceeds MAX_EXPR_DEPTH cleanly
    let deep = format!("{}1.0{}", "(".repeat(80), ")".repeat(80));
    assert!(matches!(
        SafeExpr::parse(&deep),
        Err(ExprError::MaxDepthExceeded { .. })
    ));
}

#[test]
fn safe_expr_kinetic_easings_and_exp_endpoints_overshoot_safety_and_fuel() {
    let ctx = sample_ctx();

    // exp(x)
    let exp0 = SafeExpr::parse("exp(0.0)").unwrap();
    assert!((exp0.eval_default(&ctx).unwrap() - 1.0).abs() < 1e-12);
    let exp1 = SafeExpr::parse("exp(1.0)").unwrap();
    assert!((exp1.eval_default(&ctx).unwrap() - std::f64::consts::E).abs() < 1e-12);
    let exp_neg = SafeExpr::parse("exp(-2.0)").unwrap();
    assert!((exp_neg.eval_default(&ctx).unwrap() - (-2.0_f64).exp()).abs() < 1e-12);

    // Overflow in exp(x) -> NonFiniteValue
    let exp_overflow = SafeExpr::parse("exp(800.0)").unwrap();
    assert_eq!(
        exp_overflow.eval_default(&ctx),
        Err(ExprError::NonFiniteValue)
    );

    // Clamped endpoints & monotonicity for expo easings
    for func in ["ease_out_expo", "ease_in_expo", "ease_in_out_expo"] {
        let at_neg = SafeExpr::parse(&format!("{func}(-0.5)")).unwrap();
        let at_zero = SafeExpr::parse(&format!("{func}(0.0)")).unwrap();
        let at_quarter = SafeExpr::parse(&format!("{func}(0.25)")).unwrap();
        let at_half = SafeExpr::parse(&format!("{func}(0.5)")).unwrap();
        let at_three_quarter = SafeExpr::parse(&format!("{func}(0.75)")).unwrap();
        let at_one = SafeExpr::parse(&format!("{func}(1.0)")).unwrap();
        let at_over = SafeExpr::parse(&format!("{func}(1.5)")).unwrap();

        let v_neg = at_neg.eval_default(&ctx).unwrap();
        let v0 = at_zero.eval_default(&ctx).unwrap();
        let v25 = at_quarter.eval_default(&ctx).unwrap();
        let v50 = at_half.eval_default(&ctx).unwrap();
        let v75 = at_three_quarter.eval_default(&ctx).unwrap();
        let v1 = at_one.eval_default(&ctx).unwrap();
        let v_over = at_over.eval_default(&ctx).unwrap();

        assert_eq!(v_neg, 0.0, "{func}(-0.5) must clamp to 0.0");
        assert_eq!(v0, 0.0, "{func}(0.0) must be 0.0");
        assert_eq!(v1, 1.0, "{func}(1.0) must be 1.0");
        assert_eq!(v_over, 1.0, "{func}(1.5) must clamp to 1.0");
        assert!(
            v0 < v25 && v25 < v50 && v50 < v75 && v75 < v1,
            "{func} must be strictly monotonic on [0, 1]: {v0}, {v25}, {v50}, {v75}, {v1}"
        );
    }

    // Specific midpoint / curve checks
    let in_out_half = SafeExpr::parse("ease_in_out_expo(0.5)").unwrap();
    assert!((in_out_half.eval_default(&ctx).unwrap() - 0.5).abs() < 1e-12);
    let out_expo_half = SafeExpr::parse("ease_out_expo(0.5)").unwrap();
    assert!((out_expo_half.eval_default(&ctx).unwrap() - (1.0 - 2.0_f64.powf(-5.0))).abs() < 1e-12);
    let in_expo_half = SafeExpr::parse("ease_in_expo(0.5)").unwrap();
    assert!((in_expo_half.eval_default(&ctx).unwrap() - 2.0_f64.powf(-5.0)).abs() < 1e-12);

    // ease_out_back: endpoints clamped, natural overshoot > 1.0 around t=0.5..0.7
    assert_eq!(
        SafeExpr::parse("ease_out_back(-0.2)").unwrap().eval_default(&ctx).unwrap(),
        0.0
    );
    assert_eq!(
        SafeExpr::parse("ease_out_back(0.0)").unwrap().eval_default(&ctx).unwrap(),
        0.0
    );
    assert_eq!(
        SafeExpr::parse("ease_out_back(1.0)").unwrap().eval_default(&ctx).unwrap(),
        1.0
    );
    assert_eq!(
        SafeExpr::parse("ease_out_back(1.8)").unwrap().eval_default(&ctx).unwrap(),
        1.0
    );
    let back_mid = SafeExpr::parse("ease_out_back(0.6)").unwrap().eval_default(&ctx).unwrap();
    assert!(
        back_mid > 1.05 && back_mid < 1.12,
        "ease_out_back(0.6) should overshoot ~1.10, got {back_mid}"
    );

    // NaN / Inf rejection via context
    let nan_ctx = ExprContext {
        t_sec: f64::NAN,
        ..ctx
    };
    let inf_ctx = ExprContext {
        t_sec: f64::INFINITY,
        ..ctx
    };
    for func in [
        "exp",
        "ease_out_expo",
        "ease_in_expo",
        "ease_in_out_expo",
        "ease_out_back",
    ] {
        let e = SafeExpr::parse(&format!("{func}(t_sec)")).unwrap();
        assert_eq!(e.eval_default(&nan_ctx), Err(ExprError::NonFiniteValue));
        assert_eq!(e.eval_default(&inf_ctx), Err(ExprError::NonFiniteValue));

        // Arity check
        assert_eq!(
            SafeExpr::parse(&format!("{func}(1.0, 2.0)")),
            Err(ExprError::InvalidArity {
                func: func.into(),
                expected: 1,
                actual: 2,
            })
        );
    }

    // Fuel accounting: Call + Literal = 2 AST nodes
    let fuel_expr = SafeExpr::parse("ease_out_back(0.5)").unwrap();
    assert_eq!(fuel_expr.node_count(), 2);
    assert_eq!(
        fuel_expr.eval(&ctx, 1),
        Err(ExprError::FuelExhausted { max_fuel: 1 })
    );
    assert!(fuel_expr.eval(&ctx, 2).is_ok());
}

#[test]
fn safe_expr_floor_fract_mod_and_cubic_bezier() {
    let ctx = sample_ctx();

    // floor(x)
    assert_eq!(
        SafeExpr::parse("floor(1.9)").unwrap().eval_default(&ctx).unwrap(),
        1.0
    );
    assert_eq!(
        SafeExpr::parse("floor(-1.1)").unwrap().eval_default(&ctx).unwrap(),
        -2.0
    );
    assert_eq!(
        SafeExpr::parse("floor(0.0)").unwrap().eval_default(&ctx).unwrap(),
        0.0
    );

    // fract(x) = x - floor(x) (periodic [0, 1))
    assert!(
        (SafeExpr::parse("fract(1.25)").unwrap().eval_default(&ctx).unwrap() - 0.25).abs() < 1e-12
    );
    assert!(
        (SafeExpr::parse("fract(-0.25)").unwrap().eval_default(&ctx).unwrap() - 0.75).abs() < 1e-12
    );
    assert_eq!(
        SafeExpr::parse("fract(2.0)").unwrap().eval_default(&ctx).unwrap(),
        0.0
    );

    // mod(a, b) = a - b * floor(a / b)
    assert!(
        (SafeExpr::parse("mod(5.5, 2.0)").unwrap().eval_default(&ctx).unwrap() - 1.5).abs() < 1e-12
    );
    assert!(
        (SafeExpr::parse("mod(-0.25, 1.0)").unwrap().eval_default(&ctx).unwrap() - 0.75).abs()
            < 1e-12
    );
    assert_eq!(
        SafeExpr::parse("mod(1.0, 0.0)").unwrap().eval_default(&ctx),
        Err(ExprError::DivisionByZero)
    );

    // cubic_bezier(t, x1, y1, x2, y2)
    assert_eq!(
        SafeExpr::parse("cubic_bezier(-0.2, 0.42, 0.0, 0.58, 1.0)")
            .unwrap()
            .eval_default(&ctx)
            .unwrap(),
        0.0
    );
    assert_eq!(
        SafeExpr::parse("cubic_bezier(0.0, 0.42, 0.0, 0.58, 1.0)")
            .unwrap()
            .eval_default(&ctx)
            .unwrap(),
        0.0
    );
    assert_eq!(
        SafeExpr::parse("cubic_bezier(1.0, 0.42, 0.0, 0.58, 1.0)")
            .unwrap()
            .eval_default(&ctx)
            .unwrap(),
        1.0
    );
    assert_eq!(
        SafeExpr::parse("cubic_bezier(1.4, 0.42, 0.0, 0.58, 1.0)")
            .unwrap()
            .eval_default(&ctx)
            .unwrap(),
        1.0
    );
    // Linear identity
    assert!(
        (SafeExpr::parse("cubic_bezier(0.37, 0.0, 0.0, 1.0, 1.0)")
            .unwrap()
            .eval_default(&ctx)
            .unwrap()
            - 0.37)
            .abs()
            < 1e-12
    );
    // Symmetric CSS ease-in-out hits exact 0.5 at t = 0.5
    assert!(
        (SafeExpr::parse("cubic_bezier(0.5, 0.42, 0.0, 0.58, 1.0)")
            .unwrap()
            .eval_default(&ctx)
            .unwrap()
            - 0.5)
            .abs()
            < 1e-12
    );
    // Extreme S-curve inflection (x1=1, x2=0 -> dx/du=0 at u=0.5) converges cleanly
    let steep = SafeExpr::parse("cubic_bezier(0.5, 1.0, 0.0, 0.0, 1.0)")
        .unwrap()
        .eval_default(&ctx)
        .unwrap();
    assert!((steep - 0.5).abs() < 1e-9);
    // Overshoot allowed on y1/y2
    let overshoot = SafeExpr::parse("cubic_bezier(0.5, 0.34, 1.6, 0.64, 1.0)")
        .unwrap()
        .eval_default(&ctx)
        .unwrap();
    assert!(overshoot > 1.05, "expected overshoot > 1.05, got {overshoot}");

    // Domain, arity, non-finite, and fuel checks
    assert!(matches!(
        SafeExpr::parse("cubic_bezier(0.5, -0.1, 0.0, 0.5, 1.0)")
            .unwrap()
            .eval_default(&ctx),
        Err(ExprError::InvalidArgument(_))
    ));
    assert!(matches!(
        SafeExpr::parse("cubic_bezier(0.5, 0.2, 0.0, 1.2, 1.0)")
            .unwrap()
            .eval_default(&ctx),
        Err(ExprError::InvalidArgument(_))
    ));
    assert_eq!(
        SafeExpr::parse("cubic_bezier(0.5, 0.0, 0.0, 1.0)"),
        Err(ExprError::InvalidArity {
            func: "cubic_bezier".into(),
            expected: 5,
            actual: 4,
        })
    );
    assert_eq!(
        SafeExpr::parse("mod(1.0)"),
        Err(ExprError::InvalidArity {
            func: "mod".into(),
            expected: 2,
            actual: 1,
        })
    );
    assert_eq!(
        SafeExpr::parse("floor(1.0, 2.0)"),
        Err(ExprError::InvalidArity {
            func: "floor".into(),
            expected: 1,
            actual: 2,
        })
    );

    let cb_fuel = SafeExpr::parse("cubic_bezier(0.5, 0.42, 0.0, 0.58, 1.0)").unwrap();
    assert_eq!(cb_fuel.node_count(), 6);
    assert_eq!(
        cb_fuel.eval(&ctx, 5),
        Err(ExprError::FuelExhausted { max_fuel: 5 })
    );
    assert!(cb_fuel.eval(&ctx, 6).is_ok());

    let nan_ctx = ExprContext {
        t_sec: f64::NAN,
        ..ctx
    };
    for s in [
        "floor(t_sec)",
        "fract(t_sec)",
        "mod(t_sec, 1.0)",
        "cubic_bezier(t_sec, 0.42, 0.0, 0.58, 1.0)",
    ] {
        assert_eq!(
            SafeExpr::parse(s).unwrap().eval_default(&nan_ctx),
            Err(ExprError::NonFiniteValue)
        );
    }
}
