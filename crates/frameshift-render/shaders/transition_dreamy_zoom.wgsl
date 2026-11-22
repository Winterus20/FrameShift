// FrameShift WGSL Transition: transition_dreamy_zoom.wgsl
// Adapted from gl-transitions/transitions/DreamyZoom.glsl (MIT License, Zeh Fernando).
// Aspect-corrected rotation & zoom (zero 16:9 shear) + 12-tap IGN-jittered zoom/rotation
// motion blur, gentle peak tint, optional RGB chromatic aberration, and midpoint impact flash.
// Working format: Rgba16Float, Linear Rec.709, premultiplied alpha.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct WhipTransitionUniform {
    // x = progress [0..1]
    // y = direction_rad (radians; sets rotation sign and subtle directional drift)
    // z = whip_strength (zoom and rotation amplitude scale, floored to >= 0.25)
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
const BLUR_TAPS: i32 = 12;
const DREAMY_TINT_LINEAR: vec3<f32> = vec3<f32>(0.09, 0.055, 0.11);

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

fn rotate_zoom_uv(
    uv: vec2<f32>,
    angle: f32,
    inv_scale: f32,
    drift_uv: vec2<f32>,
    aspect: f32,
) -> vec2<f32> {
    let c = cos(angle);
    let s = sin(angle);
    let p_iso = vec2<f32>((uv.x - 0.5) * aspect, uv.y - 0.5) * inv_scale;
    let r_iso = vec2<f32>(p_iso.x * c - p_iso.y * s, p_iso.x * s + p_iso.y * c);
    return vec2<f32>(r_iso.x / aspect, r_iso.y) + vec2<f32>(0.5) + drift_uv;
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
    let strength = clamp(max(u_trans.motion.z, 0.25), 0.25, 3.0);
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
    let bell = sin(PI * p);
    let rot_sign = select(-1.0, 1.0, cos(direction_rad) >= 0.0);
    let dir_uv = vec2<f32>(cos(direction_rad) / aspect, sin(direction_rad));

    let base_angle = bell * 0.11 * strength * rot_sign;
    let inv_scale_a = 1.0 / (1.0 + 0.24 * strength * sin(PI * 0.5 * p));
    let inv_scale_b = 1.0 / (1.0 + 0.24 * strength * sin(PI * 0.5 * (1.0 - p)));
    let drift = dir_uv * (bell * 0.015 * strength);

    let use_ca = ca_strength > 1e-5;
    let radial = in.uv - vec2<f32>(0.5);
    let ca_offset = radial * (ca_strength * bell * 0.025);
    let jitter = ign(in.position.xy);
    let blur_amp = bell * min(strength, 2.0);

    var acc_a = vec4<f32>(0.0);
    var acc_b = vec4<f32>(0.0);
    var total_weight: f32 = 0.0;

    for (var i: i32 = 0; i < BLUR_TAPS; i = i + 1) {
        let t_norm = ((f32(i) + jitter) / f32(BLUR_TAPS)) - 0.5;
        let weight = max(1.0 - 1.5 * abs(t_norm), 0.2);
        let s = t_norm * blur_amp;

        let uv_a = rotate_zoom_uv(
            in.uv,
            base_angle + s * 0.05 * rot_sign,
            inv_scale_a * (1.0 + s * 0.12),
            drift,
            aspect,
        );
        let uv_b = rotate_zoom_uv(
            in.uv,
            base_angle * 0.85 - s * 0.05 * rot_sign,
            inv_scale_b * (1.0 - s * 0.12),
            -drift * 0.5,
            aspect,
        );

        acc_a = acc_a + sample_clip(tex_a, uv_a, ca_offset, use_ca) * weight;
        acc_b = acc_b + sample_clip(tex_b, uv_b, ca_offset, use_ca) * weight;
        total_weight = total_weight + weight;
    }

    let inv_w = 1.0 / max(total_weight, 1e-5);
    var res = mix(acc_a * inv_w, acc_b * inv_w, dissolve);
    let tint_env = bell * bell;
    let peak_tint = DREAMY_TINT_LINEAR * tint_env * min(strength, 2.0);
    let flash = vec3<f32>(impact_flash * bell);
    res = vec4<f32>(res.rgb + (peak_tint + flash) * max(res.a, 0.0), res.a);

    return res;
}
