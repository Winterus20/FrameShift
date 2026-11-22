use crate::audio::AudioBuffer;
use crate::frame::{AlphaMode, Rgba8Frame};
use frameshift_time::{FLICKS_PER_SECOND, Flicks, ticks_to_audio_samples_nearest};

/// Deterministic synthetic video and audio generator for automated Golden Scene
/// and seek-accuracy verification.
#[derive(Debug, Clone, Copy, Default)]
pub struct SyntheticMediaGenerator;

impl SyntheticMediaGenerator {
    /// Generates a high-contrast anime-style energy orb moving along a deterministic
    /// trajectory with dynamic speed-lines and an embedded 64-bit `source_tick` barcode
    /// in the top-left corner.
    pub fn generate_clip_a_frame(width: u32, height: u32, source_tick: Flicks) -> Rgba8Frame {
        assert!(width > 0 && height > 0, "frame dimensions must be non-zero");
        let t = source_tick.to_seconds_f64();
        let w = width as f64;
        let h = height as f64;
        let min_dim = w.min(h);

        // Smooth Lissajous-arc trajectory across the canvas.
        let orb_cx = w * (0.5 + 0.34 * (t * 2.1).sin());
        let orb_cy = h * (0.5 + 0.28 * (t * 3.1 + 0.7).cos());
        let core_radius = min_dim * (0.09 + 0.02 * (t * 8.0).sin());
        let aura_radius = core_radius * 2.4;

        let mut data = vec![0u8; (width as usize) * (height as usize) * 4];
        for y in 0..height {
            let yf = y as f64 + 0.5;
            let v_norm = yf / h;
            for x in 0..width {
                let xf = x as f64 + 0.5;
                let u_norm = xf / w;

                // Deep anime night-sky gradient background.
                let mut r = 10.0 + 18.0 * (1.0 - v_norm);
                let mut g = 14.0 + 12.0 * (u_norm * (1.0 - v_norm));
                let mut b = 34.0 + 46.0 * v_norm;

                // Diagonal speed lines scrolling deterministically with `t`.
                let diag = (xf * 0.7 + yf * 0.35 - t * min_dim * 1.8).rem_euclid(min_dim * 0.16);
                if diag < min_dim * 0.015 {
                    r += 24.0;
                    g += 38.0;
                    b += 72.0;
                }

                // Energy orb core + cyan/magenta plasma aura.
                let dx = xf - orb_cx;
                let dy = yf - orb_cy;
                let dist = (dx * dx + dy * dy).sqrt();
                if dist < aura_radius {
                    let aura = (1.0 - dist / aura_radius).powf(2.0);
                    let angle = dy.atan2(dx) + t * 5.0;
                    let flare = 0.7 + 0.3 * (angle * 6.0).sin();
                    r += 60.0 * aura + 190.0 * aura * flare;
                    g += 210.0 * aura * flare;
                    b += 255.0 * aura;
                }
                if dist < core_radius {
                    let core = 1.0 - (dist / core_radius).powf(2.5);
                    r += 255.0 * core;
                    g += 255.0 * core;
                    b += 255.0 * core;
                }

                let idx = ((y as usize) * (width as usize) + (x as usize)) * 4;
                data[idx] = clamp_u8(r);
                data[idx + 1] = clamp_u8(g);
                data[idx + 2] = clamp_u8(b);
                data[idx + 3] = 255;
            }
        }

        let mut frame = Rgba8Frame {
            width,
            height,
            pts: source_tick,
            alpha_mode: AlphaMode::Straight,
            data,
        };
        embed_tick_barcode(&mut frame, source_tick);
        frame
    }

    /// Generates a high-contrast dramatic silhouette over a moving perspective cyber-grid
    /// with crimson/amber rim lighting and an embedded 64-bit `source_tick` barcode.
    pub fn generate_clip_b_frame(width: u32, height: u32, source_tick: Flicks) -> Rgba8Frame {
        assert!(width > 0 && height > 0, "frame dimensions must be non-zero");
        let t = source_tick.to_seconds_f64();
        let w = width as f64;
        let h = height as f64;
        let horizon_y = h * 0.56;

        // Silhouette anchor drifts smoothly across the center.
        let sil_cx = w * (0.5 + 0.14 * (t * 1.4).sin());
        let sil_w = w * 0.16;
        let sil_top = h * 0.20;

        let mut data = vec![0u8; (width as usize) * (height as usize) * 4];
        for y in 0..height {
            let yf = y as f64 + 0.5;
            for x in 0..width {
                let xf = x as f64 + 0.5;
                let mut r: f64;
                let mut g: f64;
                let mut b: f64;

                if yf < horizon_y {
                    // Crimson dusk sky with pulsing horizontal scan accent.
                    let sky_t = yf / horizon_y;
                    r = 45.0 + 145.0 * sky_t.powf(1.5);
                    g = 12.0 + 42.0 * sky_t.powf(2.0);
                    b = 38.0 + 28.0 * (1.0 - sky_t);
                } else {
                    // Perspective cyber-grid scrolling forward with `t`.
                    let depth = ((yf - horizon_y) / (h - horizon_y)).max(0.02);
                    let world_z = 1.0 / depth + t * 4.5;
                    let world_x = ((xf - w * 0.5) / (w * 0.5)) / depth;
                    let gx = (world_x * 3.0).fract().abs();
                    let gz = (world_z * 2.0).fract().abs();
                    let on_grid = gx < 0.08 || gx > 0.92 || gz < 0.10 || gz > 0.90;
                    if on_grid {
                        let glow = (1.0 - depth * 0.3).max(0.2);
                        r = 250.0 * glow;
                        g = 64.0 * glow;
                        b = 110.0 * glow;
                    } else {
                        r = 16.0;
                        g = 8.0;
                        b = 24.0;
                    }
                }

                // Dramatic angular mecha/monolith silhouette with bright rim edge.
                let dx = (xf - sil_cx).abs();
                let body_half_w = if yf > sil_top && yf < h * 0.88 {
                    let frac = (yf - sil_top) / (h * 0.88 - sil_top);
                    sil_w * (0.28 + 0.72 * frac)
                } else {
                    -1.0
                };
                if body_half_w > 0.0 && dx < body_half_w {
                    let rim = (body_half_w - dx).min(yf - sil_top);
                    if rim < (w.min(h) * 0.015).max(2.0) {
                        r = 255.0;
                        g = 210.0;
                        b = 90.0;
                    } else {
                        r = 8.0;
                        g = 8.0;
                        b = 14.0;
                    }
                }

                let idx = ((y as usize) * (width as usize) + (x as usize)) * 4;
                data[idx] = clamp_u8(r);
                data[idx + 1] = clamp_u8(g);
                data[idx + 2] = clamp_u8(b);
                data[idx + 3] = 255;
            }
        }

        let mut frame = Rgba8Frame {
            width,
            height,
            pts: source_tick,
            alpha_mode: AlphaMode::Straight,
            data,
        };
        embed_tick_barcode(&mut frame, source_tick);
        frame
    }

    /// Generates an animated smooth circular/diamond alpha matte that expands and moves
    /// deterministically with `source_tick`. RGB equals alpha so both `Alpha` and `Luma`
    /// matte modes evaluate identically.
    pub fn generate_moving_mask_frame(width: u32, height: u32, source_tick: Flicks) -> Rgba8Frame {
        assert!(width > 0 && height > 0, "frame dimensions must be non-zero");
        let t = source_tick.to_seconds_f64();
        let w = width as f64;
        let h = height as f64;
        let min_dim = w.min(h);

        let cx = w * (0.5 + 0.25 * (t * 1.8).sin());
        let cy = h * (0.5 + 0.18 * (t * 2.4).cos());
        let radius = min_dim * (0.24 + 0.14 * (0.5 + 0.5 * (t * 3.0).sin()));
        let feather = (min_dim * 0.06).max(2.0);
        // Morph smoothly between circle and diamond.
        let diamond_mix = 0.5 + 0.5 * (t * 2.0).cos();

        let mut data = vec![0u8; (width as usize) * (height as usize) * 4];
        for y in 0..height {
            let dy = (y as f64 + 0.5 - cy).abs();
            for x in 0..width {
                let dx = (x as f64 + 0.5 - cx).abs();
                let euclidean = (dx * dx + dy * dy).sqrt();
                let manhattan = (dx + dy) * std::f64::consts::FRAC_1_SQRT_2;
                let dist = euclidean * (1.0 - diamond_mix) + manhattan * diamond_mix;

                let edge = smoothstep(radius + feather, radius - feather, dist);
                let v = clamp_u8(edge * 255.0);
                let idx = ((y as usize) * (width as usize) + (x as usize)) * 4;
                data[idx] = v;
                data[idx + 1] = v;
                data[idx + 2] = v;
                data[idx + 3] = v;
            }
        }

        Rgba8Frame {
            width,
            height,
            pts: source_tick,
            alpha_mode: AlphaMode::Straight,
            data,
        }
    }

    /// Generates stereo `f32` audio with crisp drum/click transients at exact beat ticks
    /// (`beat_tick = round(k * 60 * FLICKS_PER_SECOND / bpm)`). Returns `(AudioBuffer, beat_ticks)`.
    pub fn generate_beat_audio(
        duration: Flicks,
        bpm: u32,
        sample_rate: u32,
    ) -> (AudioBuffer, Vec<Flicks>) {
        assert!(bpm > 0, "bpm must be non-zero");
        assert!(sample_rate > 0, "sample_rate must be non-zero");
        assert!(duration.0 >= 0, "duration cannot be negative");

        let channels: u16 = 2;
        let mut buffer = AudioBuffer::silence_for_duration(sample_rate, channels, duration)
            .expect("valid beat audio duration");
        let total_frames = buffer.frame_count();
        let mut beat_ticks = Vec::new();

        // Exact beat interval using i128 arithmetic.
        let numer_per_beat = 60i128 * FLICKS_PER_SECOND as i128;
        let bpm_i128 = bpm as i128;
        let transient_len = ((sample_rate as f64) * 0.035).round().max(16.0) as usize;

        let mut beat_idx: i128 = 0;
        loop {
            let tick_val = (beat_idx * numer_per_beat + bpm_i128 / 2) / bpm_i128;
            let tick = Flicks(i64::try_from(tick_val).expect("beat tick fits in i64"));
            if tick >= duration {
                break;
            }
            beat_ticks.push(tick);

            let sample_idx = ticks_to_audio_samples_nearest(tick, sample_rate)
                .expect("beat sample index fits in i64") as usize;

            if sample_idx < total_frames {
                // Sample `0` of the transient is the crisp peak (amplitude 0.98) so
                // peak/transient detection lands within <= 1 sample of `tick`.
                let end = (sample_idx + transient_len).min(total_frames);
                for offset in 0..(end - sample_idx) {
                    let s = sample_idx + offset;
                    let env = if offset == 0 {
                        0.98f32
                    } else {
                        let dt = offset as f32 / sample_rate as f32;
                        let decay = (-dt * 120.0).exp();
                        let tone = (2.0 * std::f32::consts::PI * 880.0 * dt).cos();
                        0.85 * decay * tone
                    };
                    let l = (buffer.samples[s * 2] + env).clamp(-1.0, 1.0);
                    let r = (buffer.samples[s * 2 + 1] + env).clamp(-1.0, 1.0);
                    buffer.samples[s * 2] = l;
                    buffer.samples[s * 2 + 1] = r;
                }
            }
            beat_idx += 1;
        }

        (buffer, beat_ticks)
    }
}

/// Encodes the 64-bit `Flicks` value as an 8x8 high-contrast binary grid in the top-left corner.
pub fn embed_tick_barcode(frame: &mut Rgba8Frame, tick: Flicks) {
    let cell = barcode_cell_size(frame.width, frame.height);
    if frame.width < cell * 8 || frame.height < cell * 8 {
        return;
    }
    let bits = tick.0 as u64;
    for bit_idx in 0..64u32 {
        let row = bit_idx / 8;
        let col = bit_idx % 8;
        let is_one = ((bits >> bit_idx) & 1) == 1;
        let val = if is_one { 255u8 } else { 0u8 };
        let rgba = [val, val, val, 255u8];
        let x0 = col * cell;
        let y0 = row * cell;
        for dy in 0..cell {
            for dx in 0..cell {
                frame.set_pixel(x0 + dx, y0 + dy, rgba);
            }
        }
    }
}

/// Decodes the 64-bit `Flicks` barcode from the top-left corner of an `Rgba8Frame`.
/// Uses inner-pixel averaging for `cell >= 4` so lossy `yuv420p` round-trips decode cleanly.
pub fn decode_tick_barcode(frame: &Rgba8Frame) -> Option<Flicks> {
    let cell = barcode_cell_size(frame.width, frame.height);
    if frame.width < cell * 8 || frame.height < cell * 8 {
        return None;
    }
    let mut bits = 0u64;
    for bit_idx in 0..64u32 {
        let row = bit_idx / 8;
        let col = bit_idx % 8;
        let x0 = col * cell;
        let y0 = row * cell;
        let luma_sum: u32 = if cell >= 4 {
            let mut sum = 0u32;
            for dy in 1..3 {
                for dx in 1..3 {
                    let px = frame.get_pixel(x0 + dx, y0 + dy)?;
                    sum += ((px[0] as u32) + (px[1] as u32) + (px[2] as u32)) / 3;
                }
            }
            sum / 4
        } else {
            let px = frame.get_pixel(x0 + cell / 2, y0 + cell / 2)?;
            ((px[0] as u32) + (px[1] as u32) + (px[2] as u32)) / 3
        };
        if luma_sum >= 128 {
            bits |= 1u64 << bit_idx;
        }
    }
    Some(Flicks(bits as i64))
}

#[inline]
fn barcode_cell_size(width: u32, height: u32) -> u32 {
    if width >= 64 && height >= 64 {
        4
    } else if width >= 16 && height >= 16 {
        2
    } else {
        1
    }
}

#[inline]
fn clamp_u8(v: f64) -> u8 {
    v.round().clamp(0.0, 255.0) as u8
}

#[inline]
fn smoothstep(edge0: f64, edge1: f64, x: f64) -> f64 {
    if (edge1 - edge0).abs() < 1e-12 {
        return if x >= edge1 { 1.0 } else { 0.0 };
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
