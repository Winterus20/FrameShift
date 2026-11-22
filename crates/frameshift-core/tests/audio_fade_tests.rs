use frameshift_core::resolver::audio_clip_fade_gain;
use frameshift_core::{MediaResolver, Project, slice_audio_buffer};
use frameshift_media::{AudioBuffer, DEFAULT_AUDIO_SAMPLE_RATE};
use frameshift_schema::SchemaError;
use frameshift_schema::project::{AudioFadeCurve, AudioFadeDef};
use frameshift_time::{FLICKS_PER_SECOND, Flicks, ticks_to_audio_samples_nearest};
use std::path::{Path, PathBuf};

struct TempDirGuard(PathBuf);

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn unique_temp_dir(label: &str) -> TempDirGuard {
    let dir = std::env::temp_dir().join(format!(
        "frameshift_audio_fade_{label}_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp test dir");
    TempDirGuard(dir)
}

fn fade(duration: Flicks, curve: AudioFadeCurve) -> AudioFadeDef {
    AudioFadeDef {
        duration_ticks: duration,
        curve,
    }
}

fn frame_sample(buffer: &AudioBuffer, frame: usize, channel: usize) -> f32 {
    buffer.samples[frame * buffer.channels as usize + channel]
}

/// Builds a one-second 48 kHz stereo DC clip written to disk, referenced by the golden audio asset.
fn dc_clip_project(dir: &Path, duration: Flicks, amplitude: f32) -> Project {
    let frames = ticks_to_audio_samples_nearest(duration, DEFAULT_AUDIO_SAMPLE_RATE)
        .expect("frame count")
        as usize;
    let mut samples = Vec::with_capacity(frames * 2);
    for _ in 0..frames {
        samples.push(amplitude);
        samples.push(amplitude);
    }
    let buffer =
        AudioBuffer::new(DEFAULT_AUDIO_SAMPLE_RATE, 2, samples).expect("valid dc buffer");
    let wav_path = dir.join("dc_stereo_1s.wav");
    buffer.write_wav(&wav_path).expect("write dc wav");

    let mut project = Project::golden_scene_preset();
    project.timeline.duration_ticks = duration;
    let asset = project
        .assets
        .iter_mut()
        .find(|a| a.id == "asset_bgm")
        .expect("golden bgm asset");
    asset.path = wav_path.to_string_lossy().into_owned();
    asset.duration_ticks = Some(duration);
    asset.sample_rate_hz = Some(DEFAULT_AUDIO_SAMPLE_RATE);

    let ac = &mut project.timeline.audio_clips[0];
    ac.timeline_start = Flicks::ZERO;
    ac.timeline_end = duration;
    ac.source_start = Flicks::ZERO;
    ac.gain_db = 0.0;
    ac.fade_in = None;
    ac.fade_out = None;
    project
}

#[test]
fn linear_fade_in_ramps_head_to_unity_and_holds() {
    let temp = unique_temp_dir("fade_in");
    let duration = Flicks(FLICKS_PER_SECOND);
    let mut project = dc_clip_project(&temp.0, duration, 0.5);
    project.timeline.audio_clips[0].fade_in = Some(fade(
        Flicks(FLICKS_PER_SECOND / 4),
        AudioFadeCurve::Linear,
    ));
    project.validate().expect("faded project validates");

    let mixed = MediaResolver::new()
        .mix_project_audio(&project)
        .expect("mix fade-in")
        .expect("non-empty mix");

    assert_eq!(mixed.samples.len(), 48_000 * 2);
    assert_eq!(mixed.sample_rate, DEFAULT_AUDIO_SAMPLE_RATE);
    assert!(mixed.samples.iter().all(|s| s.is_finite()));

    // First sample sits at gain 0, one eighth of a second is halfway up the 0.25 s ramp,
    // and everything from 0.25 s onward holds unity gain.
    assert!(frame_sample(&mixed, 0, 0).abs() < 1e-6);
    assert!((frame_sample(&mixed, 6_000, 0) - 0.25).abs() < 2e-3);
    assert!((frame_sample(&mixed, 24_000, 0) - 0.5).abs() < 1e-5);
    assert!((frame_sample(&mixed, 47_999, 0) - 0.5).abs() < 1e-5);
    assert!((frame_sample(&mixed, 24_000, 1) - 0.5).abs() < 1e-5);
}

#[test]
fn linear_fade_out_holds_unity_then_ramps_tail_to_zero() {
    let temp = unique_temp_dir("fade_out");
    let duration = Flicks(FLICKS_PER_SECOND);
    let mut project = dc_clip_project(&temp.0, duration, 0.5);
    project.timeline.audio_clips[0].fade_out = Some(fade(
        Flicks(FLICKS_PER_SECOND / 4),
        AudioFadeCurve::Linear,
    ));
    project.validate().expect("faded project validates");

    let mixed = MediaResolver::new()
        .mix_project_audio(&project)
        .expect("mix fade-out")
        .expect("non-empty mix");

    assert!(frame_sample(&mixed, 0, 0) > 0.499);
    assert!((frame_sample(&mixed, 24_000, 0) - 0.5).abs() < 1e-5);
    // 0.125 s before the end is halfway down the 0.25 s ramp.
    assert!((frame_sample(&mixed, 41_999, 0) - 0.25).abs() < 2e-3);
    assert!(frame_sample(&mixed, 47_999, 0).abs() < 1e-6);
    assert!(mixed.samples.iter().all(|s| s.is_finite()));
}

#[test]
fn equal_power_fade_in_is_half_power_at_ramp_midpoint() {
    let temp = unique_temp_dir("equal_power");
    let duration = Flicks(FLICKS_PER_SECOND);
    let mut project = dc_clip_project(&temp.0, duration, 0.5);
    project.timeline.audio_clips[0].fade_in = Some(fade(
        Flicks(FLICKS_PER_SECOND / 2),
        AudioFadeCurve::EqualPower,
    ));
    project.validate().expect("faded project validates");

    let mixed = MediaResolver::new()
        .mix_project_audio(&project)
        .expect("mix equal-power fade-in")
        .expect("non-empty mix");

    // 0.25 s is the midpoint of a 0.5 s equal-power ramp: sin(pi/4) ~ 0.7071.
    let expected = 0.5 * std::f64::consts::FRAC_1_SQRT_2;
    assert!((frame_sample(&mixed, 12_000, 0) as f64 - expected).abs() < 2e-3);
    assert!(frame_sample(&mixed, 0, 0).abs() < 1e-6);
    assert!((frame_sample(&mixed, 47_999, 0) - 0.5).abs() < 1e-5);
}

#[test]
fn overlapping_fades_multiply_their_multipliers() {
    let mut project = Project::golden_scene_preset();
    {
        let ac = &mut project.timeline.audio_clips[0];
        ac.timeline_start = Flicks::ZERO;
        ac.timeline_end = Flicks(FLICKS_PER_SECOND);
        ac.fade_in = Some(fade(
            Flicks(FLICKS_PER_SECOND * 3 / 4),
            AudioFadeCurve::Linear,
        ));
        ac.fade_out = Some(fade(
            Flicks(FLICKS_PER_SECOND * 3 / 4),
            AudioFadeCurve::Linear,
        ));
    }
    project.validate().expect("overlapping fades are legal");

    let ac = &project.timeline.audio_clips[0];
    let clip_duration = Flicks(FLICKS_PER_SECOND);
    assert_eq!(audio_clip_fade_gain(ac, clip_duration, 0.0), 0.0);
    assert_eq!(audio_clip_fade_gain(ac, clip_duration, 1.0), 0.0);
    assert!((audio_clip_fade_gain(ac, clip_duration, 0.5) - 4.0 / 9.0).abs() < 1e-12);
    assert!((audio_clip_fade_gain(ac, clip_duration, 0.25) - 1.0 / 3.0).abs() < 1e-12);
}

#[test]
fn legacy_project_mix_is_bit_identical_to_constant_gain_reference() {
    let project = Project::golden_scene_preset();
    let ac = &project.timeline.audio_clips[0];
    assert!(ac.fade_in.is_none());
    assert!(ac.fade_out.is_none());

    let resolver = MediaResolver::new();
    let mixed = resolver
        .mix_project_audio(&project)
        .expect("legacy mix")
        .expect("non-empty legacy mix");
    assert!(mixed.samples.iter().any(|s| *s != 0.0));

    // Reproduce the pre-fade resolver path exactly: slice, mix_in, clamp.
    let asset = project.asset(&ac.asset_id).expect("asset");
    let clip_duration = ac.timeline_end - ac.timeline_start;
    let full = resolver
        .load_audio_asset(asset, ac.source_start + clip_duration)
        .expect("load legacy audio");
    let sliced = slice_audio_buffer(&full, ac.source_start, clip_duration).expect("slice legacy");
    let mut reference = AudioBuffer::silence_for_duration(
        DEFAULT_AUDIO_SAMPLE_RATE,
        2,
        project.timeline.duration_ticks,
    )
    .expect("silence");
    reference
        .mix_in(&sliced, ac.timeline_start, ac.gain_db)
        .expect("constant gain mix");
    for sample in &mut reference.samples {
        *sample = sample.clamp(-1.0, 1.0);
    }

    let mixed_bits: Vec<u32> = mixed.samples.iter().map(|s| s.to_bits()).collect();
    let reference_bits: Vec<u32> = reference.samples.iter().map(|s| s.to_bits()).collect();
    assert_eq!(mixed_bits, reference_bits);

    // Legacy JSON omits fade fields entirely and round-trips to the same samples.
    let json = project.to_json_pretty().expect("serialize legacy project");
    assert!(!json.contains("fade_in"));
    assert!(!json.contains("fade_out"));
    let restored = Project::from_json_str(&json).expect("load legacy project json");
    assert!(restored.timeline.audio_clips[0].fade_in.is_none());
    let restored_mix = resolver
        .mix_project_audio(&restored)
        .expect("legacy remix")
        .expect("non-empty remix");
    let restored_bits: Vec<u32> = restored_mix.samples.iter().map(|s| s.to_bits()).collect();
    assert_eq!(restored_bits, reference_bits);
}

#[test]
fn fade_longer_than_clip_fails_validation() {
    let mut project = Project::golden_scene_preset();
    project.timeline.duration_ticks = Flicks(FLICKS_PER_SECOND);
    let ac = &mut project.timeline.audio_clips[0];
    ac.timeline_start = Flicks::ZERO;
    ac.timeline_end = Flicks(FLICKS_PER_SECOND);
    ac.fade_in = Some(fade(
        Flicks(FLICKS_PER_SECOND + 1),
        AudioFadeCurve::Linear,
    ));

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
}
