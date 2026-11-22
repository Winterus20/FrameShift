// FrameShift WGSL Transition: transition_linear_blur.wgsl
// Adapted from gl-transitions/transitions/LinearBlur.glsl (MIT License, gre).
// Aspect-corrected 16-tap IGN-jittered directional blur along direction_rad peaking at progress = 0.5,
// with sinusoidal dissolve, optional RGB chromatic aberration, and midpoint impact flash.
// Working format: Rgba16Float, Linear Rec.709, premultiplied alpha.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct WhipTransitionUniform {
    // x = progress [0..1]
    // y = direction_rad (radians; blur axis direction)
    // z = whip_strength (directional blur span multiplier, floored to >= 0.25)
    // w = chromatic_aberration (0.0 = none, > 0 = directional RGB split)
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
const BLUR_TAPS: i32 = 16;

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
    let blur_env = 0.5 * (1.0 - cos(2.0 * PI * p));
    let dir_iso = vec2<f32>(cos(direction_rad), sin(direction_rad));
    let dir_uv = vec2<f32>(dir_iso.x / aspect, dir_iso.y);
    let iso_scale = mix(1.0, aspect, abs(dir_iso.x) * 0.45);
    let span = dir_uv * (strength * blur_env * 0.12 * iso_scale);

    let use_ca = ca_strength > 1e-5;
    let ca_offset = dir_uv * (ca_strength * blur_env * 0.028 * iso_scale);
    let jitter = ign(in.position.xy);

    var acc = vec4<f32>(0.0);
    var total_weight: f32 = 0.0;

    for (var i: i32 = 0; i < BLUR_TAPS; i = i + 1) {
        let t = (f32(i) + jitter) / f32(BLUR_TAPS) - 0.5;
        let weight = max(1.0 - 1.5 * abs(t), 0.2);
        let sample_uv = in.uv + span * t;

        let col_a = sample_clip(tex_a, sample_uv, ca_offset, use_ca);
        let col_b = sample_clip(tex_b, sample_uv, ca_offset, use_ca);
        let blended = mix(col_a, col_b, dissolve);

        acc = acc + blended * weight;
        total_weight = total_weight + weight;
    }

    var res = acc / max(total_weight, 1e-5);
    let flash = impact_flash * blur_env;
    res = vec4<f32>(res.rgb + vec3<f32>(flash) * max(res.a, 0.0), res.a);

    return res;
}
