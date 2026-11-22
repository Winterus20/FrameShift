// FrameShift M1 Built-in Shader: transition_whip_chromatic.wgsl
// Two-input anime-edit directional whip-pan transition with aspect-corrected direction,
// 16-tap IGN-jittered directional motion blur, RGB chromatic aberration, and midpoint impact flash.
// Working format: Rgba16Float, Linear Rec.709, premultiplied alpha.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct WhipTransitionUniform {
    // x = progress [0..1]
    // y = direction (radians)
    // z = whip_strength (0.0 = static dissolve, 1.0 = standard whip)
    // w = chromatic_aberration (0.0 = none, 0.5..1.5 = strong RGB split)
    motion: vec4<f32>,
    // x = impact_flash (0.0 = none, > 0 = peak midpoint flash boost)
    // y = aspect_ratio (<= 0 falls back to textureDimensions(tex_a)), zw = reserved
    flash_pad: vec4<f32>,
}

@group(0) @binding(0)
var tex_a: texture_2d<f32>;

@group(0) @binding(1)
var tex_b: texture_2d<f32>;

@group(0) @binding(2)
var s_linear: sampler;

@group(0) @binding(3)
var<uniform> params: WhipTransitionUniform;

const PI: f32 = 3.14159265359;
const BLUR_SAMPLES: i32 = 16;

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

fn sample_ca(tex: texture_2d<f32>, uv: vec2<f32>, ca_offset: vec2<f32>, use_ca: bool) -> vec4<f32> {
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
    let p = clamp(params.motion.x, 0.0, 1.0);
    let direction = params.motion.y;
    let whip_strength = max(params.motion.z, 0.0);
    let ca_strength = max(params.motion.w, 0.0);
    let impact_flash = max(params.flash_pad.x, 0.0);

    if (p <= 0.0) {
        return textureSampleLevel(tex_a, s_linear, clamp(in.uv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0);
    }
    if (p >= 1.0) {
        return textureSampleLevel(tex_b, s_linear, clamp(in.uv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0);
    }

    let dims = max(vec2<f32>(textureDimensions(tex_a)), vec2<f32>(1.0));
    let aspect = select(params.flash_pad.y, dims.x / dims.y, params.flash_pad.y <= 1e-4);

    // Quintic ease for snappy whip-pan cutover (exact 0 at p=0, exact 1 at p=1).
    let ease = p * p * p * (p * (p * 6.0 - 15.0) + 10.0);
    // Mid-transition bell envelope (exact 0 at p=0 and p=1).
    let whip_env = pow(max(sin(p * PI), 0.0), 1.25);

    // Isotropic screen direction mapped into UV space so diagonal angles and blur lengths match on 16:9.
    let dir_iso = vec2<f32>(cos(direction), sin(direction));
    let dir_uv = vec2<f32>(dir_iso.x / aspect, dir_iso.y);
    let slide_scale = mix(1.0, aspect, abs(dir_iso.x) * 0.45);
    let slide_a = dir_uv * (ease * whip_strength * 0.6 * slide_scale);
    let slide_b = -dir_uv * ((1.0 - ease) * whip_strength * 0.6 * slide_scale);

    let base_uv_a = in.uv - slide_a;
    let base_uv_b = in.uv - slide_b;

    let blur_span = dir_uv * (whip_strength * whip_env * 0.095 * slide_scale);
    let use_ca = ca_strength > 1e-5;
    let ca_offset = dir_uv * (ca_strength * whip_env * 0.028 * slide_scale);
    let jitter = ign(in.position.xy);

    var acc_a = vec4<f32>(0.0);
    var acc_b = vec4<f32>(0.0);
    var total_weight: f32 = 0.0;

    for (var i: i32 = 0; i < BLUR_SAMPLES; i = i + 1) {
        let t = ((f32(i) + jitter) / f32(BLUR_SAMPLES)) - 0.5;
        let weight = max(1.0 - 1.4 * abs(t), 0.2);
        let offset = blur_span * t;
        acc_a = acc_a + sample_ca(tex_a, base_uv_a + offset, ca_offset, use_ca) * weight;
        acc_b = acc_b + sample_ca(tex_b, base_uv_b + offset, ca_offset, use_ca) * weight;
        total_weight = total_weight + weight;
    }

    let inv_w = 1.0 / max(total_weight, 1e-5);
    acc_a = acc_a * inv_w;
    acc_b = acc_b * inv_w;

    var res = mix(acc_a, acc_b, ease);
    let flash = impact_flash * whip_env;
    res = vec4<f32>(res.rgb + vec3<f32>(flash) * max(res.a, 0.0), res.a);

    return res;
}
