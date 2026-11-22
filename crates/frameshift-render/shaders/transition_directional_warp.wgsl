// FrameShift WGSL Transition: transition_directional_warp.wgsl
// Adapted from gl-transitions/transitions/DirectionalWarp.glsl (MIT License, pschroen).
// Aspect-corrected zero-loop directional spatial wave warp with zero-vector guard,
// chromatic edge dispersion, midpoint impact flash, and premultiplied-alpha border clamping.
// Working format: Rgba16Float, Linear Rec.709, premultiplied alpha.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct WhipTransitionUniform {
    // x = progress [0..1]
    // y = direction_rad (radians)
    // z = whip_strength (spatial wave warp scale, floored to >= 1.0)
    // w = chromatic_aberration (0.0 = none, > 0 = RGB edge dispersion)
    motion: vec4<f32>,
    // x = impact_flash (0.0 = none, > 0 = midpoint premultiplied flash boost)
    // y = aspect_ratio, z, w = reserved
    flash_pad: vec4<f32>,
}

@group(0) @binding(0)
var tex_a: texture_2d<f32>;

@group(0) @binding(1)
var tex_b: texture_2d<f32>;

@group(0) @binding(2)
var s_linear: sampler;

@group(0) @binding(3)
var<uniform> u_trans: WhipTransitionUniform;

const PI: f32 = 3.14159265359;
const TAU: f32 = 6.28318530718;
const WARP_SMOOTHNESS: f32 = 0.14;

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VertexOutput {
    let x = f32((vid << 1u) & 2u);
    let y = f32(vid & 2u);
    var out: VertexOutput;
    out.position = vec4<f32>(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, 1.0 - y);
    return out;
}

fn sample_border_clamped(tex: texture_2d<f32>, uv: vec2<f32>) -> vec4<f32> {
    let in_bounds = uv.x >= 0.0 && uv.x <= 1.0 && uv.y >= 0.0 && uv.y <= 1.0;
    let clamped = clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0));
    let s = textureSampleLevel(tex, s_linear, clamped, 0.0);
    return select(vec4<f32>(0.0), s, in_bounds);
}

fn sample_border_ca(tex: texture_2d<f32>, uv: vec2<f32>, ca_offset: vec2<f32>, use_ca: bool) -> vec4<f32> {
    if (use_ca) {
        let sr = sample_border_clamped(tex, uv + ca_offset);
        let sg = sample_border_clamped(tex, uv);
        let sb = sample_border_clamped(tex, uv - ca_offset);
        let avg_a = (sr.a + sg.a + sb.a) * (1.0 / 3.0);
        return vec4<f32>(sr.r, sg.g, sb.b, avg_a);
    }
    return sample_border_clamped(tex, uv);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let p = clamp(u_trans.motion.x, 0.0, 1.0);
    let direction_rad = u_trans.motion.y;
    let warp_scale = max(u_trans.motion.z, 1.0);
    let ca_strength = max(u_trans.motion.w, 0.0);
    let impact_flash = max(u_trans.flash_pad.x, 0.0);

    if (p <= 0.0) {
        return textureSampleLevel(tex_a, s_linear, clamp(in.uv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0);
    }
    if (p >= 1.0) {
        return textureSampleLevel(tex_b, s_linear, clamp(in.uv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0);
    }

    let dims = max(vec2<f32>(textureDimensions(tex_a)), vec2<f32>(1.0));
    let aspect = select(u_trans.flash_pad.y, dims.x / dims.y, u_trans.flash_pad.y <= 1e-4);

    // Zero-vector guarded directional projection in isotropic aspect-corrected space.
    let raw_dir = vec2<f32>(cos(direction_rad), sin(direction_rad));
    let len_sq = dot(raw_dir, raw_dir);
    let safe_dir = select(vec2<f32>(-1.0, 1.0), raw_dir, len_sq > 1e-8);
    let v_iso = normalize(safe_dir);
    let perp_iso = vec2<f32>(-v_iso.y, v_iso.x);

    // Span across [-0.5 * aspect, 0.5 * aspect] x [-0.5, 0.5] exactly.
    let half_extent = 0.5 * (abs(v_iso.x) * aspect + abs(v_iso.y));
    let uv_iso = vec2<f32>((in.uv.x - 0.5) * aspect, in.uv.y - 0.5);
    let proj = dot(v_iso, uv_iso);
    let s = max(WARP_SMOOTHNESS * (2.0 * half_extent), 1e-4);
    let threshold = -half_extent + p * (2.0 * half_extent + s);
    let m = 1.0 - smoothstep(-s, 0.0, proj - threshold);

    // Spatial wave warp localized around the moving transition boundary (0 at m=0 and m=1).
    let edge_env = 4.0 * m * (1.0 - m);
    let wave_amp = (warp_scale - 1.0) * 0.035 * edge_env;
    let wave = sin(proj * 12.0 + p * TAU) * wave_amp;
    let pull = (warp_scale - 1.0) * 0.12;

    let v_uv = vec2<f32>(v_iso.x / aspect, v_iso.y);
    let perp_uv = vec2<f32>(perp_iso.x / aspect, perp_iso.y);

    let uv_a = (in.uv - vec2<f32>(0.5)) * (1.0 - m) + vec2<f32>(0.5)
        + v_uv * (m * pull) + perp_uv * wave;
    let uv_b = (in.uv - vec2<f32>(0.5)) * m + vec2<f32>(0.5)
        - v_uv * ((1.0 - m) * pull) + perp_uv * wave;

    let use_ca = ca_strength > 1e-5;
    let ca_offset = v_uv * (ca_strength * edge_env * 0.025);

    let col_a = sample_border_ca(tex_a, uv_a, ca_offset, use_ca);
    let col_b = sample_border_ca(tex_b, uv_b, ca_offset, use_ca);

    var res = mix(col_a, col_b, m);
    let bell_env = pow(max(sin(p * PI), 0.0), 1.25);
    let flash = impact_flash * bell_env;
    res = vec4<f32>(res.rgb + vec3<f32>(flash) * max(res.a, 0.0), res.a);

    return res;
}
