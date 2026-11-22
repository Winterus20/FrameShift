// FrameShift WGSL Transition: transition_burn.wgsl
// Adapted from gl-transitions/transitions/BurnThrough.glsl & FilmBurn.glsl (MIT License).
// Aspect-corrected 4-octave rotated FBM organic film burn mask, hot ember rim,
// multi-lobe light-leak flares, subtle heat shimmer, optional CA, and impact_flash.
// Working format: Rgba16Float, Linear Rec.709, premultiplied alpha.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct WhipTransitionUniform {
    // x = progress [0..1]
    // y = direction_rad (radians; orients directional film burn sweep & light leaks)
    // z = whip_strength (burn exposure & light-leak intensity multiplier, floored to >= 0.35)
    // w = chromatic_aberration (0.0 = none, > 0 = directional RGB split)
    motion: vec4<f32>,
    // x = impact_flash (0.0 = default warm burn, > 0 = boosts HDR exposure & peak burn)
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
const BURN_CRIMSON_LINEAR: vec3<f32> = vec3<f32>(1.25, 0.16, 0.02);
const BURN_AMBER_LINEAR: vec3<f32> = vec3<f32>(2.40, 0.95, 0.22);

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VertexOutput {
    let x = f32((vid << 1u) & 2u);
    let y = f32(vid & 2u);
    var out: VertexOutput;
    out.position = vec4<f32>(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, 1.0 - y);
    return out;
}

// Dave Hoskins hash (MIT) - vendor-deterministic across GPUs without sin() precision drift.
fn hash12(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    p3 = p3 + vec3<f32>(dot(p3, vec3<f32>(p3.y, p3.z, p3.x) + vec3<f32>(33.33)));
    return fract((p3.x + p3.y) * p3.z);
}

fn value_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (vec2<f32>(3.0) - 2.0 * f);
    let a = hash12(i);
    let b = hash12(i + vec2<f32>(1.0, 0.0));
    let c = hash12(i + vec2<f32>(0.0, 1.0));
    let d = hash12(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn fbm4(p_in: vec2<f32>) -> f32 {
    var p = p_in;
    var val = 0.0;
    var amp = 0.5;
    let rot = mat2x2<f32>(vec2<f32>(0.8, 0.6), vec2<f32>(-0.6, 0.8));
    for (var i: i32 = 0; i < 4; i = i + 1) {
        val = val + amp * value_noise(p);
        p = rot * p * 2.03 + vec2<f32>(17.1, 31.7);
        amp = amp * 0.5;
    }
    return val;
}

fn sample_clip(tex: texture_2d<f32>, uv: vec2<f32>, ca_offset: vec2<f32>, use_ca: bool) -> vec4<f32> {
    if (use_ca) {
        let sr = textureSampleLevel(tex, s_linear, clamp(uv + ca_offset, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0);
        let sg = textureSampleLevel(tex, s_linear, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0);
        let sb = textureSampleLevel(tex, s_linear, clamp(uv - ca_offset, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0);
        let avg_a = (sr.a + sg.a + sb.a) * (1.0 / 3.0);
        return vec4<f32>(sr.r, sg.g, sb.b, avg_a);
    }
    return textureSampleLevel(tex, s_linear, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let p = clamp(u_trans.motion.x, 0.0, 1.0);
    let direction_rad = u_trans.motion.y;
    let strength = max(u_trans.motion.z, 0.35);
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

    let burn_env = 4.0 * p * (1.0 - p);
    let bell = sin(PI * p);

    let dir_iso = vec2<f32>(cos(direction_rad), sin(direction_rad));
    let dir_uv = vec2<f32>(dir_iso.x / aspect, dir_iso.y);
    let uv_iso = vec2<f32>((in.uv.x - 0.5) * aspect, in.uv.y - 0.5);

    // Organic FBM burn field combined with directional sweep.
    let n_burn = fbm4(uv_iso * 3.4 + dir_iso * (p * 0.85) + vec2<f32>(2.3, 5.1));
    let dir_sweep = clamp(0.5 + 0.5 * dot(uv_iso, dir_iso) / max(0.5 * aspect, 0.5), 0.0, 1.0);
    let burn_field = 0.65 * n_burn + 0.35 * dir_sweep;

    let thresh = p * 1.18 - 0.09;
    // mask_a = 1 keeps tex_a, 0 reveals tex_b.
    let mask_a = smoothstep(thresh - 0.08, thresh + 0.08, burn_field);
    let ember_band = 4.0 * mask_a * (1.0 - mask_a);

    // Organic multi-lobe film light leak + subtle heat shimmer distortion.
    let n_leak = fbm4(uv_iso * 1.65 - dir_iso * (p * 1.35) + vec2<f32>(7.9, 1.7));
    let leak_lobe = pow(clamp(n_leak * 1.25, 0.0, 1.5), 2.0) * burn_env;
    let heat_dx = value_noise(uv_iso * 9.0 + vec2<f32>(p * 6.0, 1.3)) - 0.5;
    let heat_dy = value_noise(uv_iso * 9.0 + vec2<f32>(4.1, p * 6.0)) - 0.5;
    let heat_warp = vec2<f32>(heat_dx / aspect, heat_dy) * (burn_env * 0.014 * min(strength, 2.5));

    let use_ca = ca_strength > 1e-5;
    let ca_offset = dir_uv * (ca_strength * (burn_env + ember_band * 0.6) * 0.02);

    let col_a = sample_clip(tex_a, in.uv + heat_warp, ca_offset, use_ca);
    let col_b = sample_clip(tex_b, in.uv - heat_warp * 0.5, ca_offset, use_ca);
    var res = mix(col_b, col_a, mask_a);

    let flash_mod = 1.0 + impact_flash * 1.4;
    let exposure = 1.0 + (burn_env * 0.45 + ember_band * 0.55) * strength * flash_mod;
    let ember_rgb = mix(BURN_CRIMSON_LINEAR, BURN_AMBER_LINEAR, ember_band) * (ember_band * strength * 1.1 * flash_mod);
    let leak_rgb = BURN_AMBER_LINEAR * (leak_lobe * strength * 0.65 * flash_mod);
    let white_flash = vec3<f32>(impact_flash * bell * 0.35);

    let burned_rgb = res.rgb * exposure + (ember_rgb + leak_rgb + white_flash) * max(res.a, 0.0);
    return vec4<f32>(burned_rgb, res.a);
}
