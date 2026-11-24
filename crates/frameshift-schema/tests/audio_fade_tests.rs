use frameshift_schema::project::{AudioFadeCurve, AudioFadeDef};
use frameshift_schema::{
    AudioBus, AudioClipDef, AudioDuckingRuleDef, MicroSilenceDef, Project, SchemaError,
};
use frameshift_time::{FLICKS_PER_SECOND, Flicks};
use serde_json::Value;

fn secs(value: f64) -> Flicks {
    Flicks((value * FLICKS_PER_SECOND as f64).round() as i64)
}

/// Golden preset reshaped into a one-second audio clip so fade bounds are easy to reason about.
fn one_second_audio_project() -> Project {
    let mut project = Project::golden_scene_preset();
    project.timeline.duration_ticks = Flicks(FLICKS_PER_SECOND);
    let ac = &mut project.timeline.audio_clips[0];
    ac.timeline_start = Flicks::ZERO;
    ac.timeline_end = Flicks(FLICKS_PER_SECOND);
    ac.source_start = Flicks::ZERO;
    ac.fade_in = None;
    ac.fade_out = None;
    project
}

fn fade(duration: Flicks, curve: AudioFadeCurve) -> AudioFadeDef {
    AudioFadeDef {
        duration_ticks: duration,
        curve,
    }
}

#[test]
fn audio_clip_fade_fields_default_to_none_and_are_omitted_from_json() {
    let project = one_second_audio_project();
    let ac = &project.timeline.audio_clips[0];
    assert!(ac.fade_in.is_none());
    assert!(ac.fade_out.is_none());
    assert_eq!(ac.gain_db, 0.0);

    let json = project.to_json_pretty().expect("serialize");
    assert!(!json.contains("fade_in"));
    assert!(!json.contains("fade_out"));

    let raw: Value = serde_json::from_str(&json).expect("parse json value");
    let clip = &raw["timeline"]["audio_clips"][0];
    assert!(clip.get("fade_in").is_none());
    assert!(clip.get("fade_out").is_none());

    let restored = Project::from_json_str(&json).expect("legacy json loads");
    assert!(restored.timeline.audio_clips[0].fade_in.is_none());
    assert!(restored.timeline.audio_clips[0].fade_out.is_none());
}

#[test]
fn legacy_audio_clip_json_without_fade_fields_uses_defaults() {
    let legacy = r#"{
        "id": "bgm_legacy",
        "asset_id": "asset_bgm",
        "timeline_start": 0,
        "timeline_end": 705600000,
        "source_start": 0
    }"#;
    let clip: AudioClipDef = serde_json::from_str(legacy).expect("legacy audio clip loads");
    assert_eq!(clip.gain_db, 0.0);
    assert!(clip.fade_in.is_none());
    assert!(clip.fade_out.is_none());
}

#[test]
fn audio_fade_def_defaults_curve_to_linear_and_requires_duration() {
    let parsed: AudioFadeDef =
        serde_json::from_str(r#"{ "duration_ticks": 176400000 }"#).expect("fade without curve");
    assert_eq!(parsed.curve, AudioFadeCurve::Linear);
    assert_eq!(parsed.duration_ticks, Flicks(FLICKS_PER_SECOND / 4));

    let missing_duration = serde_json::from_str::<AudioFadeDef>(r#"{ "curve": "smoothstep" }"#);
    assert!(missing_duration.is_err());
}

#[test]
fn audio_fade_curve_serde_uses_snake_case_and_rejects_unknown() {
    let cases = [
        (AudioFadeCurve::Linear, "\"linear\""),
        (AudioFadeCurve::EqualPower, "\"equal_power\""),
        (AudioFadeCurve::Smoothstep, "\"smoothstep\""),
    ];
    for (curve, expected) in cases {
        assert_eq!(serde_json::to_string(&curve).unwrap(), expected);
        let restored: AudioFadeCurve = serde_json::from_str(expected).unwrap();
        assert_eq!(restored, curve);
    }

    let unknown = serde_json::from_str::<AudioFadeCurve>("\"cubic\"");
    assert!(unknown.is_err());
    assert_eq!(AudioFadeCurve::default(), AudioFadeCurve::Linear);
}

#[test]
fn audio_fade_curve_math_matches_documented_shapes() {
    assert_eq!(AudioFadeCurve::Linear.gain_at(0.0), 0.0);
    assert_eq!(AudioFadeCurve::Linear.gain_at(0.25), 0.25);
    assert_eq!(AudioFadeCurve::Linear.gain_at(1.0), 1.0);
    // Progress outside [0, 1] clamps instead of extrapolating.
    assert_eq!(AudioFadeCurve::Linear.gain_at(-0.5), 0.0);
    assert_eq!(AudioFadeCurve::Linear.gain_at(1.5), 1.0);

    let mid = AudioFadeCurve::EqualPower.gain_at(0.5);
    assert!((mid - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-12);
    let quarter = AudioFadeCurve::EqualPower.gain_at(0.25);
    let three_quarter = AudioFadeCurve::EqualPower.gain_at(0.75);
    assert!((quarter * quarter + three_quarter * three_quarter - 1.0).abs() < 1e-12);
    assert_eq!(AudioFadeCurve::EqualPower.gain_at(0.0), 0.0);
    assert_eq!(AudioFadeCurve::EqualPower.gain_at(1.0), 1.0);

    assert_eq!(AudioFadeCurve::Smoothstep.gain_at(0.5), 0.5);
    assert_eq!(AudioFadeCurve::Smoothstep.gain_at(0.25), 0.15625);
    assert_eq!(AudioFadeCurve::Smoothstep.gain_at(0.0), 0.0);
    assert_eq!(AudioFadeCurve::Smoothstep.gain_at(1.0), 1.0);
}

#[test]
fn audio_fade_def_zero_duration_is_a_no_op() {
    let no_op = fade(Flicks::ZERO, AudioFadeCurve::Linear);
    assert_eq!(no_op.gain_at_elapsed(0.0), 1.0);
    assert_eq!(no_op.gain_at_elapsed(FLICKS_PER_SECOND as f64), 1.0);

    let quarter = fade(Flicks(FLICKS_PER_SECOND / 4), AudioFadeCurve::Linear);
    assert_eq!(quarter.gain_at_elapsed(0.0), 0.0);
    assert!((quarter.gain_at_elapsed((FLICKS_PER_SECOND / 8) as f64) - 0.5).abs() < 1e-12);
    assert_eq!(
        quarter.gain_at_elapsed((FLICKS_PER_SECOND / 2) as f64),
        1.0
    );
}

#[test]
fn validate_rejects_negative_fade_duration() {
    let mut project = one_second_audio_project();
    project.timeline.audio_clips[0].fade_in = Some(fade(Flicks(-1), AudioFadeCurve::Linear));
    match project.validate() {
        Err(SchemaError::InvalidTimeline(message)) => {
            assert!(message.contains("fade_in"), "unexpected message: {message}");
            assert!(message.contains(">= 0"), "unexpected message: {message}");
        }
        other => panic!("expected InvalidTimeline, got {other:?}"),
    }

    project.timeline.audio_clips[0].fade_in = None;
    project.timeline.audio_clips[0].fade_out = Some(fade(Flicks(-1), AudioFadeCurve::EqualPower));
    match project.validate() {
        Err(SchemaError::InvalidTimeline(message)) => {
            assert!(message.contains("fade_out"), "unexpected message: {message}");
        }
        other => panic!("expected InvalidTimeline, got {other:?}"),
    }
}

#[test]
fn validate_rejects_fade_longer_than_clip_but_accepts_exact_clip_length() {
    let mut project = one_second_audio_project();
    project.timeline.audio_clips[0].fade_in = Some(fade(secs(1.5), AudioFadeCurve::Linear));
    match project.validate() {
        Err(SchemaError::InvalidTimeline(message)) => {
            assert!(message.contains("fade_in"), "unexpected message: {message}");
            assert!(
                message.contains("must not exceed clip duration"),
                "unexpected message: {message}"
            );
        }
        other => panic!("expected InvalidTimeline, got {other:?}"),
    }

    project.timeline.audio_clips[0].fade_in = None;
    project.timeline.audio_clips[0].fade_out =
        Some(fade(Flicks(FLICKS_PER_SECOND + 1), AudioFadeCurve::Smoothstep));
    match project.validate() {
        Err(SchemaError::InvalidTimeline(message)) => {
            assert!(message.contains("fade_out"), "unexpected message: {message}");
        }
        other => panic!("expected InvalidTimeline, got {other:?}"),
    }

    // A full-clip fade-in is the longest legal ramp.
    project.timeline.audio_clips[0].fade_out = None;
    project.timeline.audio_clips[0].fade_in = Some(fade(secs(1.0), AudioFadeCurve::Linear));
    assert!(project.validate().is_ok());
}

#[test]
fn validate_rejects_non_finite_gain_db() {
    let mut project = one_second_audio_project();
    project.timeline.audio_clips[0].gain_db = f32::INFINITY;
    match project.validate() {
        Err(SchemaError::InvalidTimeline(message)) => {
            assert!(message.contains("gain_db"), "unexpected message: {message}");
        }
        other => panic!("expected InvalidTimeline, got {other:?}"),
    }

    project.timeline.audio_clips[0].gain_db = f32::NAN;
    assert!(project.validate().is_err());
}

#[test]
fn audio_fade_fields_roundtrip_through_json() {
    let mut project = one_second_audio_project();
    project.timeline.audio_clips[0].fade_in = Some(fade(secs(0.25), AudioFadeCurve::Linear));
    project.timeline.audio_clips[0].fade_out = Some(fade(secs(0.5), AudioFadeCurve::EqualPower));
    project.timeline.audio_clips[0].gain_db = -6.0;
    project.validate().expect("faded project validates");

    let json = project.to_json_pretty().expect("serialize faded project");
    assert!(json.contains("\"fade_in\""));
    assert!(json.contains("\"fade_out\""));
    assert!(json.contains("\"duration_ticks\": 176400000"));
    assert!(json.contains("\"curve\": \"equal_power\""));

    let restored = Project::from_json_str(&json).expect("deserialize faded project");
    assert_eq!(restored, project);
}

#[test]
fn audio_bus_pan_anchor_ducking_and_micro_silence_roundtrip_and_validate() {
    let mut project = one_second_audio_project();
    {
        let ac = &mut project.timeline.audio_clips[0];
        assert_eq!(ac.bus, AudioBus::Music);
        assert_eq!(ac.pan, 0.0);
        assert!(ac.anchor_event_id.is_none());
        assert!(ac.apex_offset_ticks.is_none());
    }

    // Legacy AudioClipDef JSON without new fields still deserializes with defaults.
    let legacy: AudioClipDef = serde_json::from_str(
        r#"{
            "id": "legacy_sfx",
            "asset_id": "asset_bgm",
            "timeline_start": 0,
            "timeline_end": 705600000,
            "source_start": 0
        }"#,
    )
    .expect("legacy audio clip loads");
    assert_eq!(legacy.bus, AudioBus::Music);
    assert_eq!(legacy.pan, 0.0);
    assert!(legacy.anchor_event_id.is_none());
    assert!(legacy.apex_offset_ticks.is_none());

    // Round-trip all AudioBus variants.
    for (bus, expected) in [
        (AudioBus::Music, "\"music\""),
        (AudioBus::Dialogue, "\"dialogue\""),
        (AudioBus::SfxImpact, "\"sfx_impact\""),
        (AudioBus::SfxWhoosh, "\"sfx_whoosh\""),
        (AudioBus::SfxRiser, "\"sfx_riser\""),
        (AudioBus::SubBass, "\"sub_bass\""),
    ] {
        assert_eq!(serde_json::to_string(&bus).unwrap(), expected);
        assert_eq!(serde_json::from_str::<AudioBus>(expected).unwrap(), bus);
    }

    // Populate SFX clip with bus, pan, anchor_event_id, apex_offset_ticks, ducking rule & micro silence.
    project.timeline.audio_clips.push(AudioClipDef {
        id: "whoosh_drop".into(),
        asset_id: "asset_bgm".into(),
        timeline_start: Flicks(FLICKS_PER_SECOND / 4),
        timeline_end: Flicks(FLICKS_PER_SECOND),
        source_start: Flicks::ZERO,
        gain_db: -3.0,
        fade_in: None,
        fade_out: None,
        bus: AudioBus::SfxWhoosh,
        pan: -0.65,
        anchor_event_id: Some("drop_impact".into()),
        apex_offset_ticks: Some(Flicks(FLICKS_PER_SECOND / 2)),
    });
    project.timeline.ducking_rules.push(AudioDuckingRuleDef {
        trigger_bus: AudioBus::Dialogue,
        target_bus: AudioBus::Music,
        threshold: 0.08,
        duck_gain: 0.3,
        attack_ms: 12.0,
        release_ms: 180.0,
    });
    project.timeline.micro_silences.push(MicroSilenceDef {
        start_tick: Flicks(FLICKS_PER_SECOND / 5),
        end_tick: Flicks(FLICKS_PER_SECOND / 4),
        fade_ms: 4.0,
        floor_gain: 0.02,
        target_bus: Some(AudioBus::Music),
    });

    project.validate().expect("audio buses/ducking/micro_silence validate");
    let json = project.to_json_pretty().unwrap();
    let restored = Project::from_json_str(&json).expect("round-trip succeeds");
    assert_eq!(restored, project);

    // Reject invalid pan (out of range / non-finite).
    for bad_pan in [-1.01_f32, 1.01, f32::NAN, f32::INFINITY] {
        let mut bad = project.clone();
        bad.timeline.audio_clips[1].pan = bad_pan;
        assert!(matches!(bad.validate(), Err(SchemaError::InvalidTimeline(_))));
    }

    // Reject missing anchor_event_id.
    let mut bad_event = project.clone();
    bad_event.timeline.audio_clips[1].anchor_event_id = Some("nonexistent_event".into());
    assert!(matches!(
        bad_event.validate(),
        Err(SchemaError::MissingEvent(id)) if id == "nonexistent_event"
    ));

    // Reject invalid ducking rules.
    let mut same_bus = project.clone();
    same_bus.timeline.ducking_rules[0].target_bus = AudioBus::Dialogue;
    assert!(matches!(same_bus.validate(), Err(SchemaError::InvalidTimeline(_))));

    let mut bad_threshold = project.clone();
    bad_threshold.timeline.ducking_rules[0].threshold = -0.1;
    assert!(matches!(
        bad_threshold.validate(),
        Err(SchemaError::InvalidTimeline(_))
    ));

    let mut bad_duck_gain = project.clone();
    bad_duck_gain.timeline.ducking_rules[0].duck_gain = 1.5;
    assert!(matches!(
        bad_duck_gain.validate(),
        Err(SchemaError::InvalidTimeline(_))
    ));

    let mut bad_attack = project.clone();
    bad_attack.timeline.ducking_rules[0].attack_ms = -1.0;
    assert!(matches!(
        bad_attack.validate(),
        Err(SchemaError::InvalidTimeline(_))
    ));

    // Reject invalid micro silences.
    let mut bad_ms_bounds = project.clone();
    bad_ms_bounds.timeline.micro_silences[0].start_tick = Flicks(FLICKS_PER_SECOND / 2);
    assert!(matches!(
        bad_ms_bounds.validate(),
        Err(SchemaError::InvalidTimeline(_))
    ));

    let mut bad_ms_floor = project;
    bad_ms_floor.timeline.micro_silences[0].floor_gain = -0.05;
    assert!(matches!(
        bad_ms_floor.validate(),
        Err(SchemaError::InvalidTimeline(_))
    ));
}
