// FrameShift WGSL Transition: transition_kaleidoscope.wgsl
// Adapted from gl-transitions/transitions/Kaleidoscope.glsl (MIT License, nwoeanhinnogaehr / gre).
// Aspect-corrected N-sector polar mirror + true-angle KIFS planar mirror folding +
// 10-tap IGN-jittered rotational/zoom motion blur, optional CA, and midpoint impact flash.
// Working format: Rgba16Float, Linear Rec.709, premultiplied alpha.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct WhipTransitionUniform {
    // x = progress [0..1]
    // y = direction_rad (radians; rotates kaleidoscope symmetry axes & fold angle)
    // z = whip_strength (kaleidoscope twist & fold scale, floored to >= 0.25)
    // w = chromatic_aberration (0.0 = none, > 0 = radial RGB split)
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
const KALEIDO_SECTORS: f32 = 8.0;
const KALEIDO_FOLDS: i32 = 3;
const BLUR_TAPS: i32 = 10;

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VertexOutput {
    let x = f32((vid << 1u) & 2u);
    let y = f32(vid & 2u);
    var out: VertexOutput;
    out.position = vec4<f32>(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, 1.0 - y);
    return out;
}

fn ign(frag_coord: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(frag_coord, vec2<f32>(0.06711056, 0.00583715))));
}

fn mirror_uv(uv: vec2<f32>) -> vec2<f32> {
    let t = abs(uv - 2.0 * floor(uv * 0.5));
    return select(2.0 - t, t, t <= vec2<f32>(1.0));
}

fn sample_clip(tex: texture_2d<f32>, uv: vec2<f32>, ca_offset: vec2<f32>, use_ca: bool) -> vec4<f32> {
    if (use_ca) {
        let sr = textureSampleLevel(tex, s_linear, mirror_uv(uv + ca_offset), 0.0);
        let sg = textureSampleLevel(tex, s_linear, mirror_uv(uv), 0.0);
        let sb = textureSampleLevel(tex, s_linear, mirror_uv(uv - ca_offset), 0.0);
        let avg_a = (sr.a + sg.a + sb.a) * (1.0 / 3.0);
        return vec4<f32>(sr.r, sg.g, sb.b, avg_a);
    }
    return textureSampleLevel(tex, s_linear, mirror_uv(uv), 0.0);
}

fn kaleidoscope_uv(
    uv: vec2<f32>,
    p: f32,
    env: f32,
    direction_rad: f32,
    strength: f32,
    aspect: f32,
) -> vec2<f32> {
    let p_iso = vec2<f32>((uv.x - 0.5) * aspect, uv.y - 0.5);
    let rot_spin = direction_rad + (p - 0.5) * 1.4 * strength;
    let c0 = cos(rot_spin);
    let s0 = sin(rot_spin);
    let rotated = vec2<f32>(p_iso.x * c0 - p_iso.y * s0, p_iso.x * s0 + p_iso.y * c0);

    let r = length(rotated);
    let theta = select(0.0, atan2(rotated.y, rotated.x), dot(rotated, rotated) > 1e-10);
    let sector = TAU / KALEIDO_SECTORS;
    let a_mod = theta - sector * floor(theta / sector);
    let a_fold = abs(a_mod - sector * 0.5);

    var k = vec2<f32>(cos(a_fold), sin(a_fold)) * r;

    // True-angle KIFS planar mirror folding across angled normal n.
    let fold_ang = 0.52359878 + 0.32 * env * min(strength, 2.5);
    let n = vec2<f32>(-sin(fold_ang), cos(fold_ang));
    let shift = vec2<f32>(cos(fold_ang), sin(fold_ang)) * (0.075 * env * min(strength, 2.5));
    for (var i: i32 = 0; i < KALEIDO_FOLDS; i = i + 1) {
        k = k - 2.0 * min(dot(k, n), 0.0) * n;
        k = k - shift;
    }

    // Rotate folded wedge across the frame so mirrored facets sample full image detail.
    let sample_spin = direction_rad * 0.5 + (p - 0.5) * 0.9 * strength;
    let cs = cos(sample_spin);
    let ss = sin(sample_spin);
    let k_rot = vec2<f32>(k.x * cs - k.y * ss, k.x * ss + k.y * cs);
    return vec2<f32>(k_rot.x / aspect, k_rot.y) + vec2<f32>(0.5);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let p = clamp(u_trans.motion.x, 0.0, 1.0);
    let direction_rad = u_trans.motion.y;
    let strength = max(u_trans.motion.z, 0.25);
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

    let dissolve = 0.5 * (1.0 - cos(PI * p));
    let env = sin(PI * p);
    let fold_blend = smoothstep(0.0, 0.35, env);

    let use_ca = ca_strength > 1e-5;
    let radial = in.uv - vec2<f32>(0.5);
    let ca_offset = radial * (ca_strength * env * 0.025);
    let jitter = ign(in.position.xy);
    let blur_span = env * 0.09 * min(strength, 2.5);

    var acc = vec4<f32>(0.0);
    var total_weight: f32 = 0.0;

    for (var i: i32 = 0; i < BLUR_TAPS; i = i + 1) {
        let t_norm = ((f32(i) + jitter) / f32(BLUR_TAPS)) - 0.5;
        let weight = max(1.0 - 1.5 * abs(t_norm), 0.2);
        let p_tap = clamp(p + t_norm * blur_span, 0.001, 0.999);
        let env_tap = sin(PI * p_tap);
        let k_uv = kaleidoscope_uv(in.uv, p_tap, env_tap, direction_rad, strength, aspect);

        let plain_a = sample_clip(tex_a, in.uv, ca_offset * 0.25, use_ca);
        let fold_a = sample_clip(tex_a, k_uv, ca_offset, use_ca);
        let col_a = mix(plain_a, fold_a, fold_blend);

        let plain_b = sample_clip(tex_b, in.uv, ca_offset * 0.25, use_ca);
        let fold_b = sample_clip(tex_b, k_uv, ca_offset, use_ca);
        let col_b = mix(plain_b, fold_b, fold_blend);

        acc = acc + mix(col_a, col_b, dissolve) * weight;
        total_weight = total_weight + weight;
    }

    var res = acc / max(total_weight, 1e-5);
    let flash = impact_flash * env;
    res = vec4<f32>(res.rgb + vec3<f32>(flash) * max(res.a, 0.0), res.a);

    return res;
}
