use frameshift_media::frame::fit_rgba8_frame;
use frameshift_media::{
    AlphaMode, AudioBuffer, AudioRetimeMode, DEFAULT_AUDIO_SAMPLE_RATE, DEFAULT_MAX_OPEN_STREAMS,
    DEFAULT_MAX_SEQUENTIAL_SKIP, DecoderPool, DecoderPoolConfig, EncodeBackend, FfmpegBridge,
    Flicks, HwAccelDetector, LruFrameCache, MediaError, ProxyManager, ProxySpec, PtsIndex,
    Rgba8Frame, Rgba16fFrame, SourceFit, StreamKey, StreamingVideoDecoder,
    SyntheticMediaGenerator, decode_tick_barcode, downscale_frame_nearest_or_box,
};
use frameshift_time::{
    AUDIO_44_1_KHZ, FLICKS_PER_MILLISECOND, FLICKS_PER_SECOND, FPS_24, SpeedCurve, SpeedKeyframe,
    ticks_to_audio_samples_nearest,
};
use std::path::PathBuf;

struct TempPathGuard(PathBuf);

impl Drop for TempPathGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

struct TempDirGuard(PathBuf);

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn rgba8_hash_color_conversion_and_png_roundtrip() {
    let tick = Flicks(123_456_789);
    let mut frame =
        Rgba8Frame::filled(16, 16, tick, [200, 120, 45, 255], AlphaMode::Straight).unwrap();
    frame.set_pixel(3, 4, [0, 255, 128, 128]);
    frame.set_pixel(7, 7, [255, 0, 0, 0]);

    let h1 = frame.fnv1a_hash64();
    let h2 = frame.fnv1a_hash64();
    let s1 = frame.sha256_hex();
    let s2 = frame.sha256_hex();
    assert_eq!(h1, h2);
    assert_eq!(s1, s2);
    assert_eq!(s1.len(), 64);

    // Mutating 1 byte changes both FNV-1a and SHA-256.
    let mut modified = frame.clone();
    modified.set_pixel(3, 4, [1, 255, 128, 128]);
    assert_ne!(frame.fnv1a_hash64(), modified.fnv1a_hash64());
    assert_ne!(frame.sha256(), modified.sha256());

    // sRGB -> Linear Rec.709 Premultiplied -> sRGB Straight round-trip.
    let linear: Rgba16fFrame = frame.to_linear_rgba16f().unwrap();
    assert_eq!(linear.alpha_mode, AlphaMode::Premultiplied);
    // Zero-alpha pixel stays zeroed when premultiplied.
    let zero_idx = 7 * 16 + 7;
    assert_eq!(linear.data[zero_idx], [0.0, 0.0, 0.0, 0.0]);

    let back = linear.to_srgb_rgba8().unwrap();
    assert_eq!(back.alpha_mode, AlphaMode::Straight);
    assert_eq!(back.get_pixel(0, 0), Some([200, 120, 45, 255]));
    assert_eq!(back.get_pixel(7, 7), Some([0, 0, 0, 0]));
    // Semi-transparent pixel survives round-trip within 1 LSB.
    let p = back.get_pixel(3, 4).unwrap();
    assert!((p[1] as i32 - 255).abs() <= 1);
    assert!((p[2] as i32 - 128).abs() <= 2);
    assert_eq!(p[3], 128);

    // PNG bytes and file round-trip.
    let png_bytes = frame.encode_png_bytes().unwrap();
    let decoded_mem = Rgba8Frame::decode_png_bytes(&png_bytes, tick).unwrap();
    assert_eq!(decoded_mem, frame);

    let png_path =
        std::env::temp_dir().join(format!("frameshift_media_test_{}.png", std::process::id()));
    let _guard = TempPathGuard(png_path.clone());
    frame.save_png(&png_path).unwrap();
    let decoded_disk = Rgba8Frame::load_png(&png_path, tick).unwrap();
    assert_eq!(decoded_disk, frame);
}

#[test]
fn lru_frame_cache_eviction_and_telemetry() {
    let mut cache = LruFrameCache::new(2);
    let f0 = SyntheticMediaGenerator::generate_clip_a_frame(32, 32, Flicks(0));
    let f1 = SyntheticMediaGenerator::generate_clip_a_frame(32, 32, Flicks(100));
    let f2 = SyntheticMediaGenerator::generate_clip_b_frame(32, 32, Flicks(200));

    assert!(cache.get("clip_a", 0).is_none()); // miss 1
    cache.insert("clip_a", 0, f0.clone());
    cache.insert("clip_a", 1, f1.clone());

    // Touch (clip_a, 0) so (clip_a, 1) becomes LRU.
    let hit0 = cache.get("clip_a", 0).expect("clip_a frame 0 cached");
    assert_eq!(hit0.fnv1a_hash64(), f0.fnv1a_hash64());

    // Insert (clip_b, 2) -> evicts (clip_a, 1).
    cache.insert("clip_b", 2, f2.clone());
    assert_eq!(cache.len(), 2);
    assert!(cache.peek("clip_a", 0).is_some());
    assert!(cache.peek("clip_a", 1).is_none());
    assert!(cache.peek("clip_b", 2).is_some());

    let tel = cache.telemetry();
    assert_eq!(tel.hits, 1);
    assert_eq!(tel.misses, 1);
    assert_eq!(tel.evictions, 1);
    assert_eq!(tel.current_entries, 2);
    assert_eq!(tel.capacity, 2);
    assert_eq!(tel.current_bytes, f0.data.len() + f2.data.len());

    // Asset invalidation drops only matching entries.
    cache.invalidate_asset("clip_a");
    assert_eq!(cache.len(), 1);
    assert!(cache.peek("clip_b", 2).is_some());

    // Byte-bounded cache enforces byte limit.
    let mut byte_cache = LruFrameCache::with_max_bytes(10, f0.data.len());
    byte_cache.insert("clip_a", 0, f0);
    byte_cache.insert("clip_a", 1, f1);
    assert_eq!(byte_cache.len(), 1);
    assert_eq!(byte_cache.telemetry().evictions, 1);
}

#[test]
fn synthetic_generator_determinism_and_barcode() {
    let t1 = Flicks(352_800_000);
    let t2 = Flicks(705_600_000);

    let a1 = SyntheticMediaGenerator::generate_clip_a_frame(128, 128, t1);
    let a1_repeat = SyntheticMediaGenerator::generate_clip_a_frame(128, 128, t1);
    let a2 = SyntheticMediaGenerator::generate_clip_a_frame(128, 128, t2);
    assert_eq!(a1.sha256(), a1_repeat.sha256());
    assert_ne!(a1.sha256(), a2.sha256());
    assert_eq!(decode_tick_barcode(&a1), Some(t1));
    assert_eq!(decode_tick_barcode(&a2), Some(t2));

    let b1 = SyntheticMediaGenerator::generate_clip_b_frame(128, 128, t1);
    let b1_repeat = SyntheticMediaGenerator::generate_clip_b_frame(128, 128, t1);
    assert_eq!(b1.fnv1a_hash64(), b1_repeat.fnv1a_hash64());
    assert_eq!(decode_tick_barcode(&b1), Some(t1));

    let m1 = SyntheticMediaGenerator::generate_moving_mask_frame(128, 128, t1);
    let m2 = SyntheticMediaGenerator::generate_moving_mask_frame(128, 128, t2);
    assert_ne!(m1.fnv1a_hash64(), m2.fnv1a_hash64());
    // Mask keeps R == G == B == A everywhere.
    for px in m1.data.as_chunks::<4>().0 {
        assert_eq!(px[0], px[1]);
        assert_eq!(px[1], px[2]);
        assert_eq!(px[2], px[3]);
    }
}

#[test]
fn beat_audio_transient_accuracy_and_retime() {
    let duration = Flicks(2 * FLICKS_PER_SECOND);
    let (beat_buf, beat_ticks) =
        SyntheticMediaGenerator::generate_beat_audio(duration, 120, DEFAULT_AUDIO_SAMPLE_RATE);

    assert_eq!(
        beat_ticks,
        vec![
            Flicks(0),
            Flicks(352_800_000),
            Flicks(705_600_000),
            Flicks(1_058_400_000),
        ]
    );

    let half_window = Flicks(FLICKS_PER_SECOND / 100); // +/- 10ms window
    for &beat_tick in &beat_ticks {
        let expected_sample =
            ticks_to_audio_samples_nearest(beat_tick, DEFAULT_AUDIO_SAMPLE_RATE).unwrap() as usize;
        let peak = beat_buf
            .detect_peak_in_window(beat_tick, half_window)
            .unwrap();
        let diff = peak.peak_sample_index.abs_diff(expected_sample);
        assert!(
            diff <= 1,
            "beat peak at sample {} differed from expected {} by {diff} samples",
            peak.peak_sample_index,
            expected_sample
        );
        assert!(peak.peak_amplitude > 0.9);
    }

    let transients = beat_buf
        .detect_transients(0.8, Flicks(FLICKS_PER_SECOND / 4))
        .unwrap();
    assert_eq!(transients.len(), beat_ticks.len());

    // Retime Mute -> silence.
    let speed_2x = SpeedCurve::constant(2.0).unwrap();
    let muted = beat_buf
        .retime(
            &speed_2x,
            Flicks(FLICKS_PER_SECOND),
            Flicks::ZERO,
            AudioRetimeMode::Mute,
        )
        .unwrap();
    assert!(muted.samples.iter().all(|&s| s == 0.0));

    // Retime Varispeed 2.0x -> beat at 0.5s moves to 0.25s.
    let fast = beat_buf
        .retime(
            &speed_2x,
            Flicks(FLICKS_PER_SECOND),
            Flicks::ZERO,
            AudioRetimeMode::Varispeed,
        )
        .unwrap();
    let fast_beat1 = fast
        .detect_peak_in_window(Flicks(176_400_000), half_window)
        .unwrap();
    assert!(fast_beat1.peak_amplitude > 0.8);

    // Retime Freeze -> silence.
    let frozen = beat_buf
        .retime(
            &SpeedCurve::freeze(),
            Flicks(FLICKS_PER_SECOND / 4),
            Flicks::ZERO,
            AudioRetimeMode::Varispeed,
        )
        .unwrap();
    assert!(frozen.samples.iter().all(|&s| s == 0.0));

    // Retime PitchPreserving -> WSOLA preserves duration scaling without muting.
    let pp_fast = beat_buf
        .retime(
            &speed_2x,
            Flicks(FLICKS_PER_SECOND),
            Flicks::ZERO,
            AudioRetimeMode::PitchPreserving,
        )
        .unwrap();
    assert_eq!(pp_fast.frame_count(), 48_000);
    assert!(pp_fast.samples.iter().any(|&s| s.abs() > 0.1));

    // Resampling 44.1 kHz -> 48 kHz + mixing preserves duration.
    let (buf_441, _) = SyntheticMediaGenerator::generate_beat_audio(
        Flicks(FLICKS_PER_SECOND),
        120,
        AUDIO_44_1_KHZ,
    );
    let mixed = AudioBuffer::mix_tracks(
        DEFAULT_AUDIO_SAMPLE_RATE,
        2,
        Flicks(FLICKS_PER_SECOND),
        &[(&buf_441, Flicks::ZERO, -3.0)],
    )
    .unwrap();
    assert_eq!(mixed.frame_count(), 48_000);
}

#[test]
fn ffmpeg_bridge_mp4_roundtrip_probe_and_seek() {
    let frame_count = 12usize;
    let width = 128u32;
    let height = 128u32;
    let fps = FPS_24;

    let mut frames = Vec::with_capacity(frame_count);
    let mut expected_pts = Vec::with_capacity(frame_count);
    for i in 0..frame_count {
        let tick = fps.frame_to_ticks(i as i64).unwrap();
        expected_pts.push(tick);
        frames.push(SyntheticMediaGenerator::generate_clip_a_frame(
            width, height, tick,
        ));
    }

    let duration = fps.frame_to_ticks(frame_count as i64).unwrap();
    let (audio, _) =
        SyntheticMediaGenerator::generate_beat_audio(duration, 120, DEFAULT_AUDIO_SAMPLE_RATE);

    let mp4_path = std::env::temp_dir().join(format!(
        "frameshift_media_roundtrip_{}.mp4",
        std::process::id()
    ));
    let _guard = TempPathGuard(mp4_path.clone());

    FfmpegBridge::encode_mp4(&frames, Some(&audio), fps, &mp4_path).unwrap();
    assert!(mp4_path.exists());

    let probe = FfmpegBridge::probe_media_info(&mp4_path).unwrap();
    assert_eq!(probe.width, width);
    assert_eq!(probe.height, height);
    assert_eq!(probe.video_codec.as_deref(), Some("h264"));
    assert_eq!(probe.pixel_format.as_deref(), Some("yuv420p"));
    assert!(probe.has_audio);
    assert_eq!(probe.pts_index.len(), frame_count);

    let pts_index = FfmpegBridge::probe_pts_index(&mp4_path).unwrap();
    assert_eq!(pts_index.len(), frame_count);
    for (idx, (&actual, &expected)) in pts_index
        .as_slice()
        .iter()
        .zip(expected_pts.iter())
        .enumerate()
    {
        assert_eq!(
            actual, expected,
            "frame {idx} probed PTS {actual} != expected {expected}"
        );
    }

    // Decode exact frames (start, middle, end) and verify barcode survives H.264 round-trip.
    for &idx in &[0usize, 5usize, 11usize] {
        let query_tick = expected_pts[idx];
        let decoded =
            FfmpegBridge::decode_video_frame(&mp4_path, &pts_index, query_tick, width, height)
                .unwrap();
        assert_eq!(decoded.width, width);
        assert_eq!(decoded.height, height);
        assert_eq!(decoded.pts, query_tick);
        assert_eq!(
            decode_tick_barcode(&decoded),
            Some(query_tick),
            "decoded frame {idx} barcode mismatch"
        );
    }

    // Querying halfway between frame 5 and frame 6 matches frame 5 (active PTS semantics).
    let mid_tick = Flicks((expected_pts[5].0 + expected_pts[6].0) / 2);
    let mid_decoded =
        FfmpegBridge::decode_video_frame(&mp4_path, &pts_index, mid_tick, width, height).unwrap();
    assert_eq!(mid_decoded.pts, expected_pts[5]);
    assert_eq!(decode_tick_barcode(&mid_decoded), Some(expected_pts[5]));

    // Decode audio back and verify non-empty 48 kHz stereo PCM.
    let decoded_audio =
        FfmpegBridge::decode_audio(&mp4_path, DEFAULT_AUDIO_SAMPLE_RATE, 2).unwrap();
    assert_eq!(decoded_audio.sample_rate, DEFAULT_AUDIO_SAMPLE_RATE);
    assert_eq!(decoded_audio.channels, 2);
    assert!(decoded_audio.frame_count() >= 20_000);
}

#[test]
fn analyze_beats_and_energy_detects_beats_and_bpm() {
    let duration = Flicks(4 * FLICKS_PER_SECOND);
    let (beat_buf, expected_beats) =
        SyntheticMediaGenerator::generate_beat_audio(duration, 120, DEFAULT_AUDIO_SAMPLE_RATE);

    let window = Flicks(10 * FLICKS_PER_MILLISECOND);
    let report = beat_buf.analyze_beats_and_energy(window).unwrap();

    assert_eq!(report.window_ticks, window);
    assert_eq!(report.rms_envelope.len(), 400);
    assert_eq!(report.detected_beats.len(), expected_beats.len());
    for (detected, expected) in report.detected_beats.iter().zip(expected_beats.iter()) {
        let diff = (detected.0 - expected.0).abs();
        assert!(
            diff <= FLICKS_PER_MILLISECOND,
            "detected beat {detected} differed from expected {expected} by {diff} ticks"
        );
    }
    assert!(
        (report.estimated_bpm - 120.0).abs() <= 1.0,
        "expected ~120 BPM, got {}",
        report.estimated_bpm
    );

    // Also test 140 BPM and pure silence.
    let (buf_140, beats_140) =
        SyntheticMediaGenerator::generate_beat_audio(duration, 140, DEFAULT_AUDIO_SAMPLE_RATE);
    let report_140 = buf_140.analyze_beats_and_energy(window).unwrap();
    assert_eq!(report_140.detected_beats.len(), beats_140.len());
    assert!(
        (report_140.estimated_bpm - 140.0).abs() <= 2.0,
        "expected ~140 BPM, got {}",
        report_140.estimated_bpm
    );

    let silence = AudioBuffer::silence(DEFAULT_AUDIO_SAMPLE_RATE, 2, 48_000).unwrap();
    let silent_report = silence.analyze_beats_and_energy(window).unwrap();
    assert!(silent_report.detected_beats.is_empty());
    assert_eq!(silent_report.estimated_bpm, 0.0);

    // Invalid window_ticks returns explicit error.
    assert!(matches!(
        beat_buf.analyze_beats_and_energy(Flicks::ZERO),
        Err(MediaError::InvalidAudioFormat(_))
    ));
}

fn estimate_dominant_frequency_hz(buf: &AudioBuffer) -> f64 {
    let frames = buf.frame_count();
    let ch_count = buf.channels as usize;
    // Skip edges (first/last 5%) to measure steady-state interior frequency via zero-crossings
    // and confirm via DFT magnitude around the detected frequency.
    let start = frames / 20;
    let end = frames - frames / 20;
    let mut crossings = 0usize;
    let mut prev = buf.samples[start * ch_count];
    for f in (start + 1)..end {
        let cur = buf.samples[f * ch_count];
        if prev <= 0.0 && cur > 0.0 {
            crossings += 1;
        }
        prev = cur;
    }
    let dur_sec = (end - start) as f64 / buf.sample_rate as f64;
    let zcr_hz = crossings as f64 / dur_sec;

    // Fine DFT search around zcr_hz +- 30 Hz to pinpoint spectral peak.
    let mut best_freq = zcr_hz;
    let mut best_mag = -1.0f64;
    let lo_hz = (zcr_hz - 30.0).max(20.0);
    let hi_hz = zcr_hz + 30.0;
    let steps = 120;
    for step in 0..=steps {
        let freq = lo_hz + (hi_hz - lo_hz) * (step as f64 / steps as f64);
        let omega = 2.0 * std::f64::consts::PI * freq / buf.sample_rate as f64;
        let mut re = 0.0f64;
        let mut im = 0.0f64;
        for f in start..end {
            let s = buf.samples[f * ch_count] as f64;
            let angle = omega * (f as f64);
            re += s * angle.cos();
            im -= s * angle.sin();
        }
        let mag = re * re + im * im;
        if mag > best_mag {
            best_mag = mag;
            best_freq = freq;
        }
    }
    best_freq
}

#[test]
fn wsola_pitch_preserving_retime_maintains_sine_frequency() {
    let sample_rate = DEFAULT_AUDIO_SAMPLE_RATE;
    let channels = 2u16;
    let frames = sample_rate as usize; // 1.0 second
    let target_hz = 440.0f64;

    let mut samples = Vec::with_capacity(frames * channels as usize);
    for i in 0..frames {
        let t = i as f64 / sample_rate as f64;
        let v = (2.0 * std::f64::consts::PI * target_hz * t).sin() as f32 * 0.7;
        samples.push(v);
        samples.push(v * 0.5);
    }
    let sine_buf = AudioBuffer::new(sample_rate, channels, samples).unwrap();

    // Test slow-down (0.5x -> 2x length) and speed-up (1.5x, 2.0x).
    for &ratio in &[0.5_f64, 1.5_f64, 2.0_f64] {
        let stretched = sine_buf.retime_pitch_preserving(ratio).unwrap();
        let expected_frames = (frames as f64 / ratio).round() as usize;
        assert_eq!(
            stretched.frame_count(),
            expected_frames,
            "unexpected frame count for speed_ratio {ratio}"
        );
        let freq = estimate_dominant_frequency_hz(&stretched);
        assert!(
            (freq - target_hz).abs() <= 5.0,
            "speed_ratio {ratio}: expected pitch ~{target_hz} Hz, got {freq:.2} Hz"
        );

        // Determinism check: identical inputs produce bit-identical samples.
        let repeat = sine_buf.retime_pitch_preserving(ratio).unwrap();
        assert_eq!(stretched.samples, repeat.samples);
    }

    // Variable speed curve via AudioBuffer::retime(..., PitchPreserving).
    let ramp = SpeedCurve::piecewise_linear(vec![
        SpeedKeyframe {
            output_offset: Flicks::ZERO,
            speed: 0.75,
            tangent: None,
        },
        SpeedKeyframe {
            output_offset: Flicks(FLICKS_PER_SECOND),
            speed: 1.5,
            tangent: None,
        },
    ])
    .unwrap();
    let ramp_out = sine_buf
        .retime(
            &ramp,
            Flicks(FLICKS_PER_SECOND),
            Flicks::ZERO,
            AudioRetimeMode::PitchPreserving,
        )
        .unwrap();
    assert_eq!(ramp_out.frame_count(), 48_000);
    let ramp_freq = estimate_dominant_frequency_hz(&ramp_out);
    assert!(
        (ramp_freq - target_hz).abs() <= 8.0,
        "variable speed curve pitch drifted to {ramp_freq:.2} Hz"
    );

    // Out-of-range speed ratios error cleanly.
    assert!(sine_buf.retime_pitch_preserving(0.1).is_err());
    assert!(sine_buf.retime_pitch_preserving(4.5).is_err());
    assert!(sine_buf.retime_pitch_preserving(f64::NAN).is_err());
}

#[test]
fn proxy_manager_downscales_deterministically_and_preserves_pts_index() {
    let temp_dir =
        std::env::temp_dir().join(format!("frameshift_proxy_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();
    let _dir_guard = TempDirGuard(temp_dir.clone());

    let frame_count = 8usize;
    let width = 128u32;
    let height = 72u32;
    let fps = FPS_24;

    let mut frames = Vec::with_capacity(frame_count);
    let mut pts_list = Vec::with_capacity(frame_count);
    for i in 0..frame_count {
        let tick = fps.frame_to_ticks(i as i64).unwrap();
        pts_list.push(tick);
        frames.push(SyntheticMediaGenerator::generate_clip_a_frame(
            width, height, tick,
        ));
    }
    let expected_pts = PtsIndex::new(pts_list).unwrap();

    // Deterministic box downscale check.
    let small_a = downscale_frame_nearest_or_box(&frames[0], 64, 36).unwrap();
    let small_b = ProxyManager::downscale_frame(&frames[0], ProxySpec::half()).unwrap();
    assert_eq!(small_a, small_b);
    assert_eq!(small_a.width, 64);
    assert_eq!(small_a.height, 36);
    assert_eq!(small_a.pts, frames[0].pts);

    // Synthetic proxy generation & resolution preserving exact PtsIndex.
    let manager = ProxyManager::new(&temp_dir);
    let spec = ProxySpec::half();
    let synth_artifact = manager
        .resolve_or_generate_synthetic_proxy("clip_a", &frames, fps, spec)
        .unwrap();
    assert!(synth_artifact.proxy_path.is_file());
    assert_eq!(synth_artifact.width, 64);
    assert_eq!(synth_artifact.height, 36);
    assert_eq!(synth_artifact.pts_index, expected_pts);

    // Second call resolves existing proxy without failing.
    let resolved = manager
        .resolve_proxy("clip_a", spec, &expected_pts)
        .unwrap()
        .expect("proxy should resolve from cache");
    assert_eq!(resolved, synth_artifact);
    let shifted_pts = PtsIndex::new(
        expected_pts
            .as_slice()
            .iter()
            .map(|tick| Flicks(tick.0 + 1))
            .collect(),
    )
    .unwrap();
    assert!(manager
        .resolve_proxy("clip_a", spec, &shifted_pts)
        .unwrap()
        .is_none());

    // On-disk MP4 proxy generation preserves exact source PtsIndex.
    let source_mp4 = temp_dir.join("source.mp4");
    FfmpegBridge::encode_mp4(&frames, None, fps, &source_mp4).unwrap();
    let quarter_mp4 = temp_dir.join("source_quarter.mp4");
    let mp4_artifact =
        ProxyManager::generate_proxy_mp4(&source_mp4, ProxySpec::quarter(), &quarter_mp4).unwrap();
    assert!(mp4_artifact.proxy_path.is_file());
    assert_eq!(mp4_artifact.width, 32);
    assert_eq!(mp4_artifact.height, 18);
    assert_eq!(mp4_artifact.pts_index, expected_pts);

    let probed_proxy = FfmpegBridge::probe_media_info(&quarter_mp4).unwrap();
    assert_eq!(probed_proxy.width, 32);
    assert_eq!(probed_proxy.height, 18);
    assert_eq!(probed_proxy.pts_index, expected_pts);
}

#[test]
fn hwaccel_detector_and_policy_encode_with_software_fallback() {
    // Unit test parser on synthetic ffmpeg output with no hardware acceleration.
    let empty_cap = HwAccelDetector::from_command_outputs(
        "Hardware acceleration methods:\n",
        "Encoders:\n V....D libx264              libx264\n",
    );
    assert!(empty_cap.available_hwaccels.is_empty());
    assert_eq!(empty_cap.preferred_decoder, None);
    assert_eq!(empty_cap.preferred_encoder, None);

    // Unit test parser on synthetic ffmpeg output with nvenc + cuda.
    let nv_cap = HwAccelDetector::from_command_outputs(
        "Hardware acceleration methods:\ncuda\nd3d11va\n",
        "Encoders:\n ------\n V....D libx264              libx264\n V....D h264_nvenc           NVENC\n",
    );
    assert_eq!(nv_cap.available_hwaccels, vec!["cuda", "d3d11va"]);
    assert_eq!(nv_cap.preferred_decoder.as_deref(), Some("cuda"));
    assert_eq!(nv_cap.preferred_encoder.as_deref(), Some("h264_nvenc"));

    // Live host probe.
    let host_cap = FfmpegBridge::detect_hwaccel_capabilities().unwrap();
    // Should succeed cleanly on host with ffmpeg installed.
    if host_cap.available_hwaccels.is_empty() {
        assert!(host_cap.preferred_decoder.is_none());
    }

    let fps = FPS_24;
    let frames: Vec<Rgba8Frame> = (0..6)
        .map(|i| {
            let tick = fps.frame_to_ticks(i).unwrap();
            SyntheticMediaGenerator::generate_clip_a_frame(64, 64, tick)
        })
        .collect();

    // Policy with use_hw_if_available = false MUST use SoftwareFallback.
    let sw_mp4 =
        std::env::temp_dir().join(format!("frameshift_policy_sw_{}.mp4", std::process::id()));
    let _sw_guard = TempPathGuard(sw_mp4.clone());
    let sw_backend =
        FfmpegBridge::encode_mp4_with_policy(&frames, None, fps, &sw_mp4, false).unwrap();
    assert_eq!(sw_backend, EncodeBackend::SoftwareFallback);
    assert!(sw_mp4.is_file());
    let sw_probe = FfmpegBridge::probe_media_info(&sw_mp4).unwrap();
    assert_eq!(sw_probe.pts_index.len(), 6);

    // Policy with use_hw_if_available = true succeeds (either hardware or clean SoftwareFallback).
    let hw_mp4 =
        std::env::temp_dir().join(format!("frameshift_policy_hw_{}.mp4", std::process::id()));
    let _hw_guard = TempPathGuard(hw_mp4.clone());
    let _hw_backend =
        FfmpegBridge::encode_mp4_with_policy(&frames, None, fps, &hw_mp4, true).unwrap();
    assert!(hw_mp4.is_file());
    let hw_probe = FfmpegBridge::probe_media_info(&hw_mp4).unwrap();
    assert_eq!(hw_probe.pts_index.len(), 6);

    // Policy-based frame decode (software & auto-hw) preserves barcode.
    let target_tick = sw_probe.pts_index.as_slice()[2];
    let (dec_sw, dec_sw_backend) = FfmpegBridge::decode_video_frame_with_policy(
        &sw_mp4,
        &sw_probe.pts_index,
        target_tick,
        64,
        64,
        false,
    )
    .unwrap();
    assert_eq!(dec_sw_backend, EncodeBackend::SoftwareFallback);
    assert_eq!(decode_tick_barcode(&dec_sw), Some(target_tick));

    let (dec_hw, _) = FfmpegBridge::decode_video_frame_with_policy(
        &sw_mp4,
        &sw_probe.pts_index,
        target_tick,
        64,
        64,
        true,
    )
    .unwrap();
    assert_eq!(decode_tick_barcode(&dec_hw), Some(target_tick));
}

#[test]
fn source_fit_stretch_matches_old_scale_cover_crops_contain_keeps_source() {
    let fps = FPS_24;
    let src_w = 320u32;
    let src_h = 180u32;
    let dst_w = 180u32;
    let dst_h = 220u32;
    let frames: Vec<Rgba8Frame> = (0..4)
        .map(|i| wide_edge_bars(src_w, src_h, fps.frame_to_ticks(i).unwrap()))
        .collect();

    let mp4_path =
        std::env::temp_dir().join(format!("frameshift_source_fit_{}.mp4", std::process::id()));
    let _guard = TempPathGuard(mp4_path.clone());
    FfmpegBridge::encode_mp4(&frames, None, fps, &mp4_path).unwrap();
    let probe = FfmpegBridge::probe_media_info(&mp4_path).unwrap();
    let tick = probe.pts_index.as_slice()[0];

    let legacy =
        FfmpegBridge::decode_video_frame(&mp4_path, &probe.pts_index, tick, dst_w, dst_h).unwrap();
    let stretch = FfmpegBridge::decode_video_frame_with_fit(
        &mp4_path,
        &probe.pts_index,
        tick,
        dst_w,
        dst_h,
        SourceFit::Stretch,
    )
    .unwrap();
    let cover = FfmpegBridge::decode_video_frame_with_fit(
        &mp4_path,
        &probe.pts_index,
        tick,
        dst_w,
        dst_h,
        SourceFit::Cover,
    )
    .unwrap();
    let contain = FfmpegBridge::decode_video_frame_with_fit(
        &mp4_path,
        &probe.pts_index,
        tick,
        dst_w,
        dst_h,
        SourceFit::Contain,
    )
    .unwrap();

    assert_eq!(stretch.width, dst_w);
    assert_eq!(stretch.height, dst_h);
    assert_eq!(
        stretch.data, legacy.data,
        "stretch must match the historical -s scale"
    );
    assert!(is_red(stretch.get_pixel(2, dst_h / 2).unwrap()));
    assert!(is_blue(stretch.get_pixel(dst_w - 3, dst_h / 2).unwrap()));
    assert!(stretch.get_pixel(dst_w / 2, 2).unwrap()[3] > 200);

    assert_eq!(cover.width, dst_w);
    assert_eq!(cover.height, dst_h);
    assert_ne!(cover.data, stretch.data);
    assert!(
        is_green(cover.get_pixel(2, dst_h / 2).unwrap()),
        "cover must crop the left overflow, pixel {:?}",
        cover.get_pixel(2, dst_h / 2)
    );
    assert!(
        is_green(cover.get_pixel(dst_w - 3, dst_h / 2).unwrap()),
        "cover must crop the right overflow, pixel {:?}",
        cover.get_pixel(dst_w - 3, dst_h / 2)
    );
    assert!(is_green(cover.get_pixel(dst_w / 2, dst_h / 2).unwrap()));
    assert!(cover.data.as_chunks::<4>().0.iter().all(|px| px[3] > 200));

    assert_eq!(contain.width, dst_w);
    assert_eq!(contain.height, dst_h);
    assert!(
        is_clear(contain.get_pixel(dst_w / 2, 2).unwrap()),
        "contain letterbox must be transparent black, pixel {:?}",
        contain.get_pixel(dst_w / 2, 2)
    );
    assert!(is_clear(contain.get_pixel(dst_w / 2, dst_h - 3).unwrap()));
    let content_rows = (0..dst_h)
        .filter(|&y| contain.get_pixel(dst_w / 2, y).unwrap()[3] > 200)
        .count() as u32;
    let expected_content_h = dst_w * src_h / src_w;
    assert!(
        content_rows + 8 >= expected_content_h && content_rows <= expected_content_h + 8,
        "contain must keep the full source height, rows {content_rows} vs {expected_content_h}"
    );
    let content_y = dst_h / 2;
    assert!(
        is_red(contain.get_pixel(2, content_y).unwrap()),
        "contain must keep the source left edge, pixel {:?}",
        contain.get_pixel(2, content_y)
    );
    assert!(
        is_blue(contain.get_pixel(dst_w - 3, content_y).unwrap()),
        "contain must keep the source right edge, pixel {:?}",
        contain.get_pixel(dst_w - 3, content_y)
    );

    let cover_window = FfmpegBridge::decode_video_window_with_fit(
        &mp4_path,
        &probe.pts_index,
        0,
        2,
        dst_w,
        dst_h,
        SourceFit::Cover,
    )
    .unwrap();
    assert_eq!(cover_window[0].1.data, cover.data);
}

fn wide_edge_bars(width: u32, height: u32, pts: Flicks) -> Rgba8Frame {
    let mut frame =
        Rgba8Frame::filled(width, height, pts, [0, 255, 0, 255], AlphaMode::Straight).unwrap();
    let edge = width / 5;
    for y in 0..height {
        for x in 0..edge {
            frame.set_pixel(x, y, [255, 0, 0, 255]);
            frame.set_pixel(width - 1 - x, y, [0, 0, 255, 255]);
        }
    }
    frame
}

fn is_red(px: [u8; 4]) -> bool {
    px[0] > 180 && px[1] < 80 && px[2] < 80 && px[3] > 200
}

fn is_blue(px: [u8; 4]) -> bool {
    px[2] > 180 && px[0] < 80 && px[1] < 80 && px[3] > 200
}

fn is_green(px: [u8; 4]) -> bool {
    px[1] > 180 && px[0] < 80 && px[2] < 80 && px[3] > 200
}

fn is_clear(px: [u8; 4]) -> bool {
    px[0] < 16 && px[1] < 16 && px[2] < 16 && px[3] < 16
}

fn encode_synthetic_clip_mp4(
    path: &std::path::Path,
    frame_count: usize,
    width: u32,
    height: u32,
) -> frameshift_media::MediaProbeInfo {
    let fps = FPS_24;
    let frames: Vec<Rgba8Frame> = (0..frame_count)
        .map(|i| {
            let tick = fps.frame_to_ticks(i as i64).unwrap();
            SyntheticMediaGenerator::generate_clip_a_frame(width, height, tick)
        })
        .collect();
    FfmpegBridge::encode_mp4(&frames, None, fps, path).unwrap();
    FfmpegBridge::probe_media_info(path).unwrap()
}

#[test]
fn decoder_pool_sequential_read_small_skip_seek_and_bit_exact_match() {
    assert_eq!(DEFAULT_MAX_OPEN_STREAMS, 8);
    assert_eq!(DEFAULT_MAX_SEQUENTIAL_SKIP, 32);

    let frame_count = 44usize;
    let width = 96u32;
    let height = 72u32;
    let mp4_path = std::env::temp_dir().join(format!(
        "frameshift_streaming_seq_skip_seek_{}.mp4",
        std::process::id()
    ));
    let _guard = TempPathGuard(mp4_path.clone());
    let probe = encode_synthetic_clip_mp4(&mp4_path, frame_count, width, height);
    assert_eq!(probe.pts_index.len(), frame_count);

    let mut pool = DecoderPool::default();

    // 1. Sequential read frames 0..6 -> spawns == 1, sequential_reads == 5, seek_reopens == 0.
    for idx in 0..6 {
        let frame = pool
            .decode_frame(
                &mp4_path,
                &probe.pts_index,
                idx,
                width,
                height,
                SourceFit::Stretch,
            )
            .unwrap();
        let expected_pts = probe.pts_index.as_slice()[idx];
        assert_eq!(frame.pts, expected_pts);
        assert_eq!(decode_tick_barcode(&frame), Some(expected_pts));

        let single = FfmpegBridge::decode_video_frame_with_fit(
            &mp4_path,
            &probe.pts_index,
            expected_pts,
            width,
            height,
            SourceFit::Stretch,
        )
        .unwrap();
        assert_eq!(
            frame.data, single.data,
            "sequential frame {idx} must match decode_video_frame_with_fit bit-for-bit"
        );
    }
    let tel = pool.telemetry();
    assert_eq!(tel.active_streams, 1);
    assert_eq!(tel.spawns, 1);
    assert_eq!(tel.sequential_reads, 5);
    assert_eq!(tel.skipped_frames, 0);
    assert_eq!(tel.seek_reopens, 0);

    // 2. Small forward skip (cursor is at 6; request 9 => delta = 3 <= 32) -> 0 new spawns/reopens.
    let f9 = pool
        .decode_frame(
            &mp4_path,
            &probe.pts_index,
            9,
            width,
            height,
            SourceFit::Stretch,
        )
        .unwrap();
    let single9 = FfmpegBridge::decode_video_frame_with_fit(
        &mp4_path,
        &probe.pts_index,
        probe.pts_index.as_slice()[9],
        width,
        height,
        SourceFit::Stretch,
    )
    .unwrap();
    assert_eq!(f9.data, single9.data);
    assert_eq!(
        decode_tick_barcode(&f9),
        Some(probe.pts_index.as_slice()[9])
    );

    // Boundary skip: cursor is at 10; request 42 => delta = 32 == DEFAULT_MAX_SEQUENTIAL_SKIP.
    let f42 = pool
        .decode_frame(
            &mp4_path,
            &probe.pts_index,
            42,
            width,
            height,
            SourceFit::Stretch,
        )
        .unwrap();
    let single42 = FfmpegBridge::decode_video_frame_with_fit(
        &mp4_path,
        &probe.pts_index,
        probe.pts_index.as_slice()[42],
        width,
        height,
        SourceFit::Stretch,
    )
    .unwrap();
    assert_eq!(f42.data, single42.data);

    let tel_after_skip = pool.telemetry();
    assert_eq!(tel_after_skip.spawns, 1);
    assert_eq!(tel_after_skip.seek_reopens, 0);
    assert_eq!(tel_after_skip.skipped_frames, 3 + 32);

    // 3. Rewind (cursor is 43; request 2) -> seek_reopens == 1.
    let f2 = pool
        .decode_frame(
            &mp4_path,
            &probe.pts_index,
            2,
            width,
            height,
            SourceFit::Stretch,
        )
        .unwrap();
    let single2 = FfmpegBridge::decode_video_frame_with_fit(
        &mp4_path,
        &probe.pts_index,
        probe.pts_index.as_slice()[2],
        width,
        height,
        SourceFit::Stretch,
    )
    .unwrap();
    assert_eq!(f2.data, single2.data);
    assert_eq!(pool.telemetry().seek_reopens, 1);
    assert_eq!(pool.telemetry().spawns, 1);

    // 4. Large forward jump (cursor is 3; request 36 => delta = 33 > 32) -> seek_reopens == 2.
    let f36 = pool
        .decode_frame(
            &mp4_path,
            &probe.pts_index,
            36,
            width,
            height,
            SourceFit::Stretch,
        )
        .unwrap();
    let single36 = FfmpegBridge::decode_video_frame_with_fit(
        &mp4_path,
        &probe.pts_index,
        probe.pts_index.as_slice()[36],
        width,
        height,
        SourceFit::Stretch,
    )
    .unwrap();
    assert_eq!(f36.data, single36.data);
    assert_eq!(pool.telemetry().seek_reopens, 2);
    assert_eq!(pool.telemetry().spawns, 1);

    pool.close_all();
    assert_eq!(pool.telemetry().active_streams, 0);
}

#[test]
fn decoder_pool_separates_source_fits_and_evicts_lru_streams() {
    let fps = FPS_24;
    let src_w = 160u32;
    let src_h = 90u32;
    let dst_w = 90u32;
    let dst_h = 110u32;
    let frames: Vec<Rgba8Frame> = (0..6)
        .map(|i| wide_edge_bars(src_w, src_h, fps.frame_to_ticks(i).unwrap()))
        .collect();
    let mp4_path = std::env::temp_dir().join(format!(
        "frameshift_streaming_fit_lru_{}.mp4",
        std::process::id()
    ));
    let _guard = TempPathGuard(mp4_path.clone());
    FfmpegBridge::encode_mp4(&frames, None, fps, &mp4_path).unwrap();
    let probe = FfmpegBridge::probe_media_info(&mp4_path).unwrap();

    // Verify SourceFit separation and bit-for-bit parity with decode_video_frame_with_fit.
    let mut fit_pool = DecoderPool::default();
    for fit in [SourceFit::Stretch, SourceFit::Cover, SourceFit::Contain] {
        let streamed = fit_pool
            .decode_frame(&mp4_path, &probe.pts_index, 1, dst_w, dst_h, fit)
            .unwrap();
        let direct = FfmpegBridge::decode_video_frame_with_fit(
            &mp4_path,
            &probe.pts_index,
            probe.pts_index.as_slice()[1],
            dst_w,
            dst_h,
            fit,
        )
        .unwrap();
        assert_eq!(
            streamed.data, direct.data,
            "SourceFit::{fit:?} must match single-frame decode bit-for-bit"
        );
    }
    assert_eq!(fit_pool.telemetry().active_streams, 3);
    assert_eq!(fit_pool.telemetry().spawns, 3);
    fit_pool.close_all();

    // Verify LRU eviction with max_open_streams = 2.
    let mut lru_pool = DecoderPool::new(DecoderPoolConfig {
        max_open_streams: 2,
        max_sequential_skip: 32,
    });
    lru_pool
        .decode_frame(
            &mp4_path,
            &probe.pts_index,
            0,
            dst_w,
            dst_h,
            SourceFit::Stretch,
        )
        .unwrap();
    lru_pool
        .decode_frame(
            &mp4_path,
            &probe.pts_index,
            0,
            dst_w,
            dst_h,
            SourceFit::Cover,
        )
        .unwrap();
    assert_eq!(lru_pool.telemetry().active_streams, 2);

    // Touch Stretch at frame 1 so Cover becomes LRU.
    lru_pool
        .decode_frame(
            &mp4_path,
            &probe.pts_index,
            1,
            dst_w,
            dst_h,
            SourceFit::Stretch,
        )
        .unwrap();
    assert_eq!(lru_pool.telemetry().sequential_reads, 1);

    // Opening Contain must evict Cover while preserving Stretch.
    lru_pool
        .decode_frame(
            &mp4_path,
            &probe.pts_index,
            0,
            dst_w,
            dst_h,
            SourceFit::Contain,
        )
        .unwrap();
    assert_eq!(lru_pool.telemetry().active_streams, 2);
    assert_eq!(lru_pool.telemetry().lru_evictions, 1);
    assert_eq!(lru_pool.telemetry().spawns, 3);

    // Stretch must still be open and serve frame 2 sequentially with 0 new spawns.
    lru_pool
        .decode_frame(
            &mp4_path,
            &probe.pts_index,
            2,
            dst_w,
            dst_h,
            SourceFit::Stretch,
        )
        .unwrap();
    assert_eq!(lru_pool.telemetry().spawns, 3);
    assert_eq!(lru_pool.telemetry().sequential_reads, 2);

    // Cover was evicted, so requesting frame 1 on Cover causes a new spawn.
    lru_pool
        .decode_frame(
            &mp4_path,
            &probe.pts_index,
            1,
            dst_w,
            dst_h,
            SourceFit::Cover,
        )
        .unwrap();
    assert_eq!(lru_pool.telemetry().spawns, 4);
    assert_eq!(lru_pool.telemetry().lru_evictions, 2);
}

#[test]
fn decoder_pool_releases_windows_file_locks_on_invalidate_close_and_drop() {
    let pid = std::process::id();
    let mp4_invalidate =
        std::env::temp_dir().join(format!("frameshift_streaming_lock_inv_{pid}.mp4"));
    let mp4_close = std::env::temp_dir().join(format!("frameshift_streaming_lock_cls_{pid}.mp4"));
    let mp4_drop = std::env::temp_dir().join(format!("frameshift_streaming_lock_drp_{pid}.mp4"));
    let _g1 = TempPathGuard(mp4_invalidate.clone());
    let _g2 = TempPathGuard(mp4_close.clone());
    let _g3 = TempPathGuard(mp4_drop.clone());

    let probe1 = encode_synthetic_clip_mp4(&mp4_invalidate, 6, 64, 64);
    let probe2 = encode_synthetic_clip_mp4(&mp4_close, 6, 64, 64);
    let probe3 = encode_synthetic_clip_mp4(&mp4_drop, 6, 64, 64);

    {
        let mut pool = DecoderPool::default();
        pool.decode_frame(&mp4_invalidate, &probe1.pts_index, 0, 64, 64, SourceFit::Stretch)
            .unwrap();
        pool.decode_frame(&mp4_close, &probe2.pts_index, 0, 64, 64, SourceFit::Stretch)
            .unwrap();
        assert_eq!(pool.telemetry().active_streams, 2);

        // 1. invalidate_path releases file lock for mp4_invalidate while keeping mp4_close open.
        pool.invalidate_path(&mp4_invalidate);
        assert_eq!(pool.telemetry().active_streams, 1);
        std::fs::remove_file(&mp4_invalidate)
            .expect("temp mp4 must be deletable after invalidate_path");

        // 2. close_all releases remaining stream locks.
        pool.close_all();
        assert_eq!(pool.telemetry().active_streams, 0);
        std::fs::remove_file(&mp4_close).expect("temp mp4 must be deletable after close_all");

        // 3. Leave mp4_drop open when `pool` drops at end of scope.
        pool.decode_frame(&mp4_drop, &probe3.pts_index, 0, 64, 64, SourceFit::Stretch)
            .unwrap();
        assert_eq!(pool.telemetry().active_streams, 1);
    }

    std::fs::remove_file(&mp4_drop).expect("temp mp4 must be deletable after DecoderPool::drop");
}

#[test]
fn streaming_video_decoder_reads_until_clean_eof_and_validates_inputs() {
    let mp4_path = std::env::temp_dir().join(format!(
        "frameshift_streaming_direct_eof_{}.mp4",
        std::process::id()
    ));
    let _guard = TempPathGuard(mp4_path.clone());
    let probe = encode_synthetic_clip_mp4(&mp4_path, 6, 64, 64);

    let key = StreamKey::new(&mp4_path, 64, 64, SourceFit::Stretch);
    let mut decoder = StreamingVideoDecoder::open(key.clone(), &probe.pts_index, 0).unwrap();
    assert_eq!(decoder.next_cursor(), 0);

    let mut read_count = 0usize;
    while let Some(frame) = decoder.read_next_frame(&probe.pts_index).unwrap() {
        assert_eq!(
            decode_tick_barcode(&frame),
            Some(probe.pts_index.as_slice()[read_count])
        );
        read_count += 1;
    }
    assert_eq!(read_count, 6);
    assert_eq!(decoder.next_cursor(), 6);
    // Repeated read after EOF returns Ok(None).
    assert!(decoder.read_next_frame(&probe.pts_index).unwrap().is_none());

    let bad_dim_key = StreamKey::new(&mp4_path, 0, 64, SourceFit::Stretch);
    assert!(matches!(
        StreamingVideoDecoder::open(bad_dim_key, &probe.pts_index, 0),
        Err(MediaError::InvalidDimensions { .. })
    ));

    let empty_pts = PtsIndex::new(Vec::new()).unwrap();
    assert!(matches!(
        StreamingVideoDecoder::open(key, &empty_pts, 0),
        Err(MediaError::EmptyMediaStream(_))
    ));
}

#[test]
fn fit_rgba8_frame_stretch_cover_contain_and_halo_free_alpha() {
    let pts = Flicks(42_000);

    // 1. Exact dimensions -> direct clone preserving pts, alpha_mode, and bytes.
    let mut exact_src =
        Rgba8Frame::filled(8, 6, pts, [120, 200, 40, 180], AlphaMode::Straight).unwrap();
    exact_src.set_pixel(2, 3, [10, 20, 30, 0]);
    let cloned = fit_rgba8_frame(&exact_src, 8, 6, SourceFit::Cover).unwrap();
    assert_eq!(cloned, exact_src);

    // 2. Stretch: premultiplied-space bilinear resize must avoid dark halos on transparent edges
    // and must not bleed hidden RGB from alpha == 0 pixels.
    let mut edge_src =
        Rgba8Frame::filled(4, 2, pts, [0, 0, 255, 0], AlphaMode::Straight).unwrap();
    for y in 0..2 {
        for x in 0..2 {
            edge_src.set_pixel(x, y, [255, 255, 255, 255]);
        }
    }
    let stretched = fit_rgba8_frame(&edge_src, 8, 2, SourceFit::Stretch).unwrap();
    let stretched_repeat = fit_rgba8_frame(&edge_src, 8, 2, SourceFit::Stretch).unwrap();
    assert_eq!(stretched.sha256(), stretched_repeat.sha256());
    assert_eq!(stretched.alpha_mode, AlphaMode::Straight);
    assert_eq!(stretched.pts, pts);

    let mut saw_semi_transparent = false;
    for y in 0..2 {
        for x in 0..8 {
            let px = stretched.get_pixel(x, y).unwrap();
            if px[3] > 0 && px[3] < 255 {
                saw_semi_transparent = true;
                // Halo-free white edge must remain pure white (~255), never darkened or blue-tinted.
                assert!(
                    px[0] >= 253 && px[1] >= 253 && px[2] >= 253,
                    "dark/color halo detected at ({x}, {y}): {px:?}"
                );
            } else if px[3] == 0 {
                assert_eq!(px, [0, 0, 0, 0]);
            }
        }
    }
    assert!(
        saw_semi_transparent,
        "expected semi-transparent boundary pixels during 4->8 resize"
    );

    // 3. Cover: wide 16x8 source (4 red | 8 green | 4 blue) fit to 8x8 -> center-cropped green.
    let mut wide_src =
        Rgba8Frame::filled(16, 8, pts, [0, 255, 0, 255], AlphaMode::Straight).unwrap();
    for y in 0..8 {
        for x in 0..4 {
            wide_src.set_pixel(x, y, [255, 0, 0, 255]);
        }
        for x in 12..16 {
            wide_src.set_pixel(x, y, [0, 0, 255, 255]);
        }
    }
    let covered_wide = fit_rgba8_frame(&wide_src, 8, 8, SourceFit::Cover).unwrap();
    for y in 0..8 {
        for x in 0..8 {
            let px = covered_wide.get_pixel(x, y).unwrap();
            assert!(
                px[1] >= 250 && px[0] <= 5 && px[2] <= 5 && px[3] == 255,
                "Cover center crop expected green at ({x}, {y}), got {px:?}"
            );
        }
    }

    // Cover: tall 8x16 source (4 red top | 8 green middle | 4 blue bottom) fit to 8x8 -> green.
    let mut tall_src =
        Rgba8Frame::filled(8, 16, pts, [0, 255, 0, 255], AlphaMode::Straight).unwrap();
    for y in 0..4 {
        for x in 0..8 {
            tall_src.set_pixel(x, y, [255, 0, 0, 255]);
        }
    }
    for y in 12..16 {
        for x in 0..8 {
            tall_src.set_pixel(x, y, [0, 0, 255, 255]);
        }
    }
    let covered_tall = fit_rgba8_frame(&tall_src, 8, 8, SourceFit::Cover).unwrap();
    for y in 0..8 {
        for x in 0..8 {
            let px = covered_tall.get_pixel(x, y).unwrap();
            assert!(
                px[1] >= 250 && px[0] <= 5 && px[2] <= 5 && px[3] == 255,
                "Cover tall center crop expected green at ({x}, {y}), got {px:?}"
            );
        }
    }

    // 4. Contain: wide 16x8 solid red fit to 16x16 -> top/bottom 4 rows transparent black.
    let solid_red =
        Rgba8Frame::filled(16, 8, pts, [255, 0, 0, 255], AlphaMode::Straight).unwrap();
    let contained_h = fit_rgba8_frame(&solid_red, 16, 16, SourceFit::Contain).unwrap();
    for y in 0..16 {
        for x in 0..16 {
            let px = contained_h.get_pixel(x, y).unwrap();
            if !(4..12).contains(&y) {
                assert_eq!(px, [0, 0, 0, 0], "expected transparent letterbox at ({x},{y})");
            } else {
                assert_eq!(px, [255, 0, 0, 255], "expected red content at ({x},{y})");
            }
        }
    }

    // Contain: tall 8x16 solid blue fit to 16x16 -> left/right 4 cols transparent black.
    let solid_blue =
        Rgba8Frame::filled(8, 16, pts, [0, 0, 255, 255], AlphaMode::Straight).unwrap();
    let contained_v = fit_rgba8_frame(&solid_blue, 16, 16, SourceFit::Contain).unwrap();
    for y in 0..16 {
        for x in 0..16 {
            let px = contained_v.get_pixel(x, y).unwrap();
            if !(4..12).contains(&x) {
                assert_eq!(px, [0, 0, 0, 0], "expected transparent pillarbox at ({x},{y})");
            } else {
                assert_eq!(px, [0, 0, 255, 255], "expected blue content at ({x},{y})");
            }
        }
    }

    // Invalid zero dimensions return explicit error.
    assert!(matches!(
        fit_rgba8_frame(&solid_red, 0, 16, SourceFit::Stretch),
        Err(MediaError::InvalidDimensions { .. })
    ));
}

#[test]
fn load_image_frame_with_fit_png_fast_path_and_ffmpeg_fallbacks() {
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("frameshift_image_decode_{pid}"));
    std::fs::create_dir_all(&dir).unwrap();
    let _dir_guard = TempDirGuard(dir.clone());

    // Build a 32x16 source image with alpha edge (left half white opaque, right half transparent)
    // plus an opaque 32x16 test pattern (8 red | 16 green | 8 blue) for format conversions.
    let mut alpha_src =
        Rgba8Frame::filled(32, 16, Flicks::ZERO, [0, 0, 0, 0], AlphaMode::Straight).unwrap();
    for y in 0..16 {
        for x in 0..16 {
            alpha_src.set_pixel(x, y, [255, 255, 255, 255]);
        }
    }
    let png_alpha_path = dir.join("alpha_edge.png");
    alpha_src.save_png(&png_alpha_path).unwrap();

    // PNG magic detection must work even when extension is non-.png (pure Rust path).
    let renamed_png_path = dir.join("magic_only.bin");
    std::fs::copy(&png_alpha_path, &renamed_png_path).unwrap();

    for path in [&png_alpha_path, &renamed_png_path] {
        let stretch =
            FfmpegBridge::load_image_frame_with_fit(path, 48, 16, SourceFit::Stretch).unwrap();
        assert_eq!(stretch.width, 48);
        assert_eq!(stretch.height, 16);
        assert_eq!(stretch.pts, Flicks::ZERO);
        assert_eq!(stretch.alpha_mode, AlphaMode::Straight);
        for y in 0..16 {
            for x in 0..48 {
                let px = stretch.get_pixel(x, y).unwrap();
                if px[3] > 0 && px[3] < 255 {
                    assert!(
                        px[0] >= 253 && px[1] >= 253 && px[2] >= 253,
                        "PNG Stretch halo at ({x},{y}): {px:?}"
                    );
                }
            }
        }

        let cover =
            FfmpegBridge::load_image_frame_with_fit(path, 16, 16, SourceFit::Cover).unwrap();
        assert_eq!(cover.width, 16);
        assert_eq!(cover.height, 16);
        // Left half of center 16 cols (x=8..16 of 32) is white opaque; right half (x=16..24) is transparent.
        assert_eq!(cover.get_pixel(2, 8), Some([255, 255, 255, 255]));
        assert_eq!(cover.get_pixel(13, 8), Some([0, 0, 0, 0]));

        let contain =
            FfmpegBridge::load_image_frame_with_fit(path, 32, 32, SourceFit::Contain).unwrap();
        assert_eq!(contain.width, 32);
        assert_eq!(contain.height, 32);
        assert_eq!(contain.get_pixel(4, 2), Some([0, 0, 0, 0]));
        assert_eq!(contain.get_pixel(4, 29), Some([0, 0, 0, 0]));
        assert_eq!(contain.get_pixel(4, 16), Some([255, 255, 255, 255]));
        assert_eq!(contain.get_pixel(27, 16), Some([0, 0, 0, 0]));
    }

    // Create opaque 32x16 tricolor PNG (8 red | 16 green | 8 blue) and convert to JPEG, BMP, TIFF via ffmpeg.
    let mut rgb_src =
        Rgba8Frame::filled(32, 16, Flicks::ZERO, [0, 255, 0, 255], AlphaMode::Straight).unwrap();
    for y in 0..16 {
        for x in 0..8 {
            rgb_src.set_pixel(x, y, [255, 0, 0, 255]);
        }
        for x in 24..32 {
            rgb_src.set_pixel(x, y, [0, 0, 255, 255]);
        }
    }
    let rgb_png_path = dir.join("tricolor.png");
    rgb_src.save_png(&rgb_png_path).unwrap();

    for ext in ["jpg", "bmp", "tiff"] {
        let out_img = dir.join(format!("tricolor.{ext}"));
        let status = std::process::Command::new("ffmpeg")
            .args(["-y", "-v", "error", "-nostdin", "-i"])
            .arg(&rgb_png_path)
            .arg(&out_img)
            .stdin(std::process::Stdio::null())
            .status()
            .expect("ffmpeg must be available for image format test generation");
        assert!(status.success(), "ffmpeg failed generating {ext}");

        for fit in [SourceFit::Stretch, SourceFit::Cover, SourceFit::Contain] {
            let (target_w, target_h) = match fit {
                SourceFit::Stretch => (24, 12),
                SourceFit::Cover => (16, 16),
                SourceFit::Contain => (32, 32),
            };
            let loaded =
                FfmpegBridge::load_image_frame_with_fit(&out_img, target_w, target_h, fit)
                    .unwrap_or_else(|e| panic!("failed loading {ext} with {fit:?}: {e}"));
            let direct =
                FfmpegBridge::decode_image_frame_with_fit(&out_img, target_w, target_h, fit)
                    .unwrap_or_else(|e| panic!("failed decoding {ext} with {fit:?}: {e}"));
            assert_eq!(loaded, direct, "{ext} {fit:?} load vs decode mismatch");
            assert_eq!(loaded.width, target_w);
            assert_eq!(loaded.height, target_h);
            assert_eq!(loaded.pts, Flicks::ZERO);
            assert_eq!(loaded.alpha_mode, AlphaMode::Straight);

            match fit {
                SourceFit::Stretch => {
                    // Center pixel should be predominantly green.
                    let c = loaded.get_pixel(12, 6).unwrap();
                    assert!(c[1] > 180 && c[0] < 60 && c[2] < 60, "{ext} Stretch center: {c:?}");
                }
                SourceFit::Cover => {
                    // Center 16x16 crop of 32x16 keeps the middle green stripe.
                    let c = loaded.get_pixel(8, 8).unwrap();
                    assert!(c[1] > 180 && c[0] < 60 && c[2] < 60, "{ext} Cover center: {c:?}");
                }
                SourceFit::Contain => {
                    // Top/bottom padding rows must be transparent black [0, 0, 0, 0].
                    assert_eq!(loaded.get_pixel(16, 2), Some([0, 0, 0, 0]));
                    assert_eq!(loaded.get_pixel(16, 29), Some([0, 0, 0, 0]));
                    let c = loaded.get_pixel(16, 16).unwrap();
                    assert!(c[1] > 180 && c[0] < 60 && c[2] < 60, "{ext} Contain center: {c:?}");
                }
            }
        }
    }

    // Error cases: zero dimension & missing path.
    assert!(matches!(
        FfmpegBridge::load_image_frame_with_fit(&png_alpha_path, 0, 16, SourceFit::Stretch),
        Err(MediaError::InvalidDimensions { .. })
    ));
    assert!(matches!(
        FfmpegBridge::decode_image_frame_with_fit(&png_alpha_path, 16, 0, SourceFit::Stretch),
        Err(MediaError::InvalidDimensions { .. })
    ));
    let missing = dir.join("does_not_exist.jpg");
    assert!(matches!(
        FfmpegBridge::load_image_frame_with_fit(&missing, 16, 16, SourceFit::Stretch),
        Err(MediaError::Io(_))
    ));
    assert!(matches!(
        FfmpegBridge::decode_image_frame_with_fit(&missing, 16, 16, SourceFit::Stretch),
        Err(MediaError::FfmpegFailed { .. })
    ));
}
