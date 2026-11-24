// FrameShift WGSL Post-FX: fx_motion_tile_shake.wgsl
// Adapted from Vidvox/ISF-Files (Quad Tile.fs, Mirror Edge.fs, Shake.fs - MIT) and
// godotshaders.com/shader/screen-shake (MIT).
// Damped harmonic camera shake + zoom punch + aspect-corrected rotation with mirrored UV folding
// so shook edges never expose black bars.
// Working format: Rgba16Float, Linear Rec.709, premultiplied alpha.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct MotionTileShakeUniform {
    // x = tile_scale, y = shake_amp_norm_x, z = shake_amp_norm_y, w = shake_rot_rad
    tile_shake: vec4<f32>,
    // x = shake_zoom, y = shake_freq_hz, z = shake_decay, w = event_delta_sec
    dynamics: vec4<f32>,
    // x = mirror_edges (0u | 1u), y = tilt_x_rad bits, z = tilt_y_rad bits, w = noise_amp bits
    flags: vec4<u32>,
}

@group(0) @binding(0)
var t_source: texture_2d<f32>;

@group(0) @binding(1)
var s_linear: sampler;

@group(0) @binding(2)
var<uniform> params: MotionTileShakeUniform;

const TAU: f32 = 6.28318530718;

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VertexOutput {
    let x = f32((vid << 1u) & 2u);
    let y = f32(vid & 2u);
    var out: VertexOutput;
    out.position = vec4<f32>(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, 1.0 - y);
    return out;
}

fn mirror_uv(uv: vec2<f32>) -> vec2<f32> {
    return 1.0 - abs(fract(uv * 0.5) * 2.0 - 1.0);
}

fn hash11(p: f32) -> f32 {
    return fract(sin(p * 127.1 + 311.7) * 43758.5453123) * 2.0 - 1.0;
}

fn value_noise_1d(x: f32) -> f32 {
    let i = floor(x);
    let f = fract(x);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(hash11(i), hash11(i + 1.0), u);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let tile_scale = select(params.tile_shake.x, 1.0, params.tile_shake.x <= 1e-4);
    let amp_x = params.tile_shake.y;
    let amp_y = params.tile_shake.z;
    let rot_amp = params.tile_shake.w;

    let shake_zoom = params.dynamics.x;
    let freq_hz = max(params.dynamics.y, 0.0);
    let decay = max(params.dynamics.z, 0.0);
    let dt = params.dynamics.w;

    let tilt_x_amp = bitcast<f32>(params.flags.y);
    let tilt_y_amp = bitcast<f32>(params.flags.z);
    let noise_amp = bitcast<f32>(params.flags.w);

    // Damped harmonic oscillation active for event_delta_sec >= 0.
    let is_active = select(0.0, 1.0, dt >= 0.0);
    let safe_dt = max(dt, 0.0);
    let env = is_active * exp(-decay * safe_dt);

    let osc_x = env * sin(TAU * freq_hz * safe_dt);
    let osc_y = env * cos(TAU * freq_hz * 1.13 * safe_dt + 0.7);
    let osc_r = env * sin(TAU * freq_hz * 0.87 * safe_dt + 1.9);

    var dx = amp_x * osc_x;
    var dy = amp_y * osc_y;
    var theta = rot_amp * osc_r;
    if (abs(noise_amp) > 1e-6) {
        let n_phase = safe_dt * max(freq_hz, 1.0);
        dx = dx + env * noise_amp * 0.1 * value_noise_1d(n_phase * 1.7 + 13.7);
        dy = dy + env * noise_amp * 0.1 * value_noise_1d(n_phase * 1.9 + 47.3);
        theta = theta + env * noise_amp * 0.25 * value_noise_1d(n_phase * 1.3 + 91.1);
    }
    let zoom = max(1.0 + shake_zoom * env, 1e-4);

    // Aspect-corrected rotation around frame center (0.5, 0.5).
    let dims = max(vec2<f32>(textureDimensions(t_source)), vec2<f32>(1.0));
    let aspect = dims.x / dims.y;

    var p = in.uv - vec2<f32>(0.5, 0.5);
    p = vec2<f32>(p.x * aspect, p.y);

    let c = cos(-theta);
    let s = sin(-theta);
    let rotated = vec2<f32>(p.x * c - p.y * s, p.x * s + p.y * c);

    p = vec2<f32>(rotated.x / aspect, rotated.y);
    if (abs(tilt_x_amp) > 1e-6 || abs(tilt_y_amp) > 1e-6) {
        let tx = tilt_x_amp * env * cos(TAU * freq_hz * 0.71 * safe_dt + 0.3);
        let ty = tilt_y_amp * env * sin(TAU * freq_hz * 0.63 * safe_dt + 1.1);
        let denom = max(1.0 + p.x * sin(ty) * 0.6 - p.y * sin(tx) * 0.6, 0.15);
        p = vec2<f32>(p.x * cos(ty), p.y * cos(tx)) / denom;
    }
    p = (p * tile_scale) / zoom + vec2<f32>(0.5, 0.5) + vec2<f32>(dx, dy);

    if (params.flags.x != 0u) {
        return textureSampleLevel(t_source, s_linear, mirror_uv(p), 0.0);
    }

    let in_bounds = p.x >= 0.0 && p.x <= 1.0 && p.y >= 0.0 && p.y <= 1.0;
    let s_col = textureSampleLevel(t_source, s_linear, clamp(p, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0);
    return select(vec4<f32>(0.0), s_col, in_bounds);
}
