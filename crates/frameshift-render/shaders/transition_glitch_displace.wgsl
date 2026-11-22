// FrameShift WGSL Transition: transition_glitch_displace.wgsl
// Adapted from gl-transitions/transitions/GlitchDisplace.glsl (MIT License, Matt DesLauriers).
// Fixes upstream smoothstep(1.0, 0.5, p) UB via 1.0 - smoothstep(0.5, 1.0, p),
// uses aspect-corrected time-stepped dynamic 3x3 Voronoi cellular displacement + scanline tear,
// Linear Rec.709 luminance shift, RGB chromatic split, and midpoint impact flash.
// Working format: Rgba16Float, Linear Rec.709, premultiplied alpha.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct WhipTransitionUniform {
    // x = progress [0..1]
    // y = direction_rad (radians; orients RGB chromatic split & glitch seed)
    // z = whip_strength (displacement intensity multiplier; 1.0 = standard glitch)
    // w = chromatic_aberration (0.0 = none, > 0 = RGB channel split)
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
const ALPHA_EPS: f32 = 1e-5;
const LUMA_REC709: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);

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
    let t = abs(uv - 2.0 * floor(uv * 0.5));
    return select(2.0 - t, t, t <= vec2<f32>(1.0));
}

// Dave Hoskins hashes (MIT) for deterministic cross-GPU Voronoi jitter.
fn hash12(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    p3 = p3 + vec3<f32>(dot(p3, vec3<f32>(p3.y, p3.z, p3.x) + vec3<f32>(33.33)));
    return fract((p3.x + p3.y) * p3.z);
}

fn hash21(p: f32) -> vec2<f32> {
    var p3 = fract(vec3<f32>(p) * vec3<f32>(0.1031, 0.1030, 0.0973));
    p3 = p3 + vec3<f32>(dot(p3, vec3<f32>(p3.y, p3.z, p3.x) + vec3<f32>(33.33)));
    return fract((vec2<f32>(p3.x, p3.x) + vec2<f32>(p3.y, p3.z)) * vec2<f32>(p3.z, p3.y));
}

fn random2f(p: vec2<f32>) -> vec2<f32> {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * vec3<f32>(0.1031, 0.1030, 0.0973));
    p3 = p3 + vec3<f32>(dot(p3, vec3<f32>(p3.y, p3.z, p3.x) + vec3<f32>(33.33)));
    return fract((vec2<f32>(p3.x, p3.x) + vec2<f32>(p3.y, p3.z)) * vec2<f32>(p3.z, p3.y));
}

fn voronoi(x: vec2<f32>) -> vec3<f32> {
    let n = floor(x);
    let f = fract(x);
    var mr = vec2<f32>(0.0);
    var md: f32 = 8.0;

    for (var j: i32 = -1; j <= 1; j = j + 1) {
        for (var i: i32 = -1; i <= 1; i = i + 1) {
            let g = vec2<f32>(f32(i), f32(j));
            let o = random2f(n + g);
            let r = g + o - f;
            let d = dot(r, r);
            if (d < md) {
                md = d;
                mr = r;
            }
        }
    }
    return vec3<f32>(md, mr);
}

fn ease1(t: f32) -> f32 {
    if (t <= 0.0) {
        return 0.0;
    }
    if (t >= 1.0) {
        return 1.0;
    }
    if (t < 0.5) {
        return 0.5 * pow(2.0, 20.0 * t - 10.0);
    }
    return -0.5 * pow(2.0, -20.0 * t + 10.0) + 1.0;
}

fn ease2(t: f32) -> f32 {
    if (t <= 0.0) {
        return 0.0;
    }
    if (t >= 1.0) {
        return 1.0;
    }
    return 1.0 - pow(2.0, -10.0 * t);
}

fn displace_offset(
    tex: texture_2d<f32>,
    uv: vec2<f32>,
    uv_iso: vec2<f32>,
    aspect: f32,
    step_slow: f32,
    step_fast: f32,
    dot_depth: f32,
    texture_depth: f32,
) -> vec2<f32> {
    let seed1 = hash21(step_slow) * 19.0 + vec2<f32>(1.0);
    let seed2 = hash21(step_fast) * 31.0 + vec2<f32>(4.0);
    let seed3 = hash21(step_fast + 11.0) * 47.0 + vec2<f32>(17.0);

    let vor1 = voronoi(uv_iso * 2.5 + seed1);
    let vor2 = voronoi(uv_iso * 14.0 + seed2);
    let vor3 = voronoi(uv_iso * 52.0 + seed3);

    let b = vor1.x * 0.5 + vor2.x * 0.3 + vor3.x * 0.2;
    let raw = textureSampleLevel(tex, s_linear, mirror_uv(uv), 0.0);
    if (raw.a <= ALPHA_EPS) {
        return vec2<f32>(0.0);
    }
    let rgb = clamp(raw.rgb / raw.a, vec3<f32>(0.0), vec3<f32>(4.0));
    let luma = dot(rgb, LUMA_REC709);
    let dx = (vor2.y * dot_depth) + (rgb.r - luma) * texture_depth;
    let dy = (vor3.z * dot_depth) + (rgb.b - luma + b * 0.15) * texture_depth;
    return vec2<f32>(dx / aspect, dy);
}

fn sample_split(tex: texture_2d<f32>, uv: vec2<f32>, ca_offset: vec2<f32>, use_ca: bool) -> vec4<f32> {
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
    let strength = max(u_trans.motion.z, 0.0);
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

    // Upstream GLSL bug fix: smoothstep(1.0, 0.5, p) replaced with 1.0 - smoothstep(0.5, 1.0, p).
    let p_in = smoothstep(0.0, 0.5, p);
    let p_out = 1.0 - smoothstep(0.5, 1.0, p);

    let dir_uv = vec2<f32>(cos(direction_rad) / aspect, sin(direction_rad));
    let bell_env = pow(max(sin(p * PI), 0.0), 1.25);
    let use_ca = ca_strength > 1e-5;
    let ca_offset = dir_uv * (ca_strength * bell_env * 0.032);

    // Time-stepped discrete glitch cadence + horizontal scanline slice tear.
    let step_slow = floor(p * 12.0 + direction_rad * 3.17);
    let step_fast = floor(p * 28.0 + direction_rad * 7.43);
    let row_band = floor(in.uv.y * 26.0);
    let row_gate = step(0.68, hash12(vec2<f32>(row_band + 19.0, step_slow)));
    let row_tear = (hash12(vec2<f32>(row_band, step_fast)) - 0.5) * 0.08 * bell_env * strength * row_gate;

    let uv_iso = vec2<f32>(in.uv.x * aspect + row_tear * aspect, in.uv.y);

    let disp1 = displace_offset(tex_a, in.uv, uv_iso, aspect, step_slow, step_fast, 0.33, 0.7) * strength;
    let disp2 = displace_offset(tex_b, in.uv, uv_iso, aspect, step_slow + 3.0, step_fast + 5.0, 0.33, 0.5) * strength;

    let uv_a = in.uv + disp1 * p_in * 0.38 + vec2<f32>(row_tear, 0.0);
    let uv_b = in.uv + disp2 * p_out * 0.38 - vec2<f32>(row_tear * 0.7, 0.0);

    let d_color1 = sample_split(tex_a, uv_a, ca_offset, use_ca);
    let d_color2 = sample_split(tex_b, uv_b, ca_offset, use_ca);

    // Linear Rec.709 luminance glitch highlight layer (premultiplied alpha safe).
    let luma = dot(min(d_color2.rgb, d_color1.rgb), LUMA_REC709);
    let gray_rgb = vec3<f32>(luma * 2.0) * d_color2.a;
    let d_color_gray = vec4<f32>(gray_rgb, d_color2.a);

    let mix_a = mix(d_color1, d_color_gray, ease1(p));
    let mix_b = mix(mix_a, d_color2, ease2(p));
    var res = mix_b;

    let flash = impact_flash * bell_env;
    res = vec4<f32>(res.rgb + vec3<f32>(flash) * max(res.a, 0.0), res.a);

    return res;
}
