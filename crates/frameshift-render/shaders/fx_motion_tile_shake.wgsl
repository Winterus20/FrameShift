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
    // x = mirror_edges (0u | 1u), y = aspect_correct_pad, z = 0u, w = 0u
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

    // Damped harmonic oscillation active for event_delta_sec >= 0.
    let is_active = select(0.0, 1.0, dt >= 0.0);
    let safe_dt = max(dt, 0.0);
    let env = is_active * exp(-decay * safe_dt);

    let osc_x = env * sin(TAU * freq_hz * safe_dt);
    let osc_y = env * cos(TAU * freq_hz * 1.13 * safe_dt + 0.7);
    let osc_r = env * sin(TAU * freq_hz * 0.87 * safe_dt + 1.9);

    let dx = amp_x * osc_x;
    let dy = amp_y * osc_y;
    let theta = rot_amp * osc_r;
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
    p = (p * tile_scale) / zoom + vec2<f32>(0.5, 0.5) + vec2<f32>(dx, dy);

    if (params.flags.x != 0u) {
        return textureSampleLevel(t_source, s_linear, mirror_uv(p), 0.0);
    }

    let in_bounds = p.x >= 0.0 && p.x <= 1.0 && p.y >= 0.0 && p.y <= 1.0;
    let s_col = textureSampleLevel(t_source, s_linear, clamp(p, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0);
    return select(vec4<f32>(0.0), s_col, in_bounds);
}
