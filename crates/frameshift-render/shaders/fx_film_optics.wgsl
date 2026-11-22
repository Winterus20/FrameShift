// FrameShift WGSL Post-FX: fx_film_optics.wgsl
// Cinematic 35mm film & lens optics pass:
// - Auto-scaled Brown-Conrady radial distortion
// - 7-tap energy-conserving continuous spectral chromatic aberration
// - Multi-pass red-orange emulsion halation composite (via fx_film_halation.wgsl)
// - Spatially correlated 2-octave 35mm organic film grain with chroma variation
// - Smooth aspect-corrected optical vignette
// Working format: Rgba16Float, Linear Rec.709, premultiplied alpha.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct FilmOpticsUniform {
    lens_k1: f32,
    lens_k2: f32,
    chromatic_edge: f32,
    grain_amount: f32,
    grain_seed: f32,
    halation_amount: f32,
    vignette_amount: f32,
    aspect: f32,
    grain_size_px: f32,
    grain_chroma: f32,
    halation_threshold: f32,
    auto_scale_flags: u32,
}

@group(0) @binding(0)
var src_tex: texture_2d<f32>;

@group(0) @binding(1)
var halation_tex: texture_2d<f32>;

@group(0) @binding(2)
var linear_sampler: sampler;

@group(0) @binding(3)
var<uniform> params: FilmOpticsUniform;

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

fn max3(v: vec3<f32>) -> f32 {
    return max(v.r, max(v.g, v.b));
}

fn sample_bounded(uv: vec2<f32>) -> vec4<f32> {
    let in_bounds = uv.x >= 0.0 && uv.x <= 1.0 && uv.y >= 0.0 && uv.y <= 1.0;
    let clamped = clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0));
    let s = textureSampleLevel(src_tex, linear_sampler, clamped, 0.0);
    return select(vec4<f32>(0.0), s, in_bounds);
}

fn pcg_hash(input: u32) -> u32 {
    let state = input * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

fn hash_unit_i(ix: i32, iy: i32, seed: u32, salt: u32) -> f32 {
    let ux = bitcast<u32>(ix);
    let uy = bitcast<u32>(iy);
    let h = pcg_hash(ux ^ (uy * 1664525u) ^ (seed * 1013904223u) ^ salt);
    return f32(h & 0x00ffffffu) / f32(0x00ffffffu);
}

// Smooth zero-mean quintic lattice noise in [-1, 1] for spatially correlated 35mm grain clumps
fn value_noise_2d(p: vec2<f32>, seed: u32, salt: u32) -> f32 {
    let ip = vec2<i32>(floor(p));
    let f = fract(p);
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let n00 = hash_unit_i(ip.x,     ip.y,     seed, salt);
    let n10 = hash_unit_i(ip.x + 1, ip.y,     seed, salt);
    let n01 = hash_unit_i(ip.x,     ip.y + 1, seed, salt);
    let n11 = hash_unit_i(ip.x + 1, ip.y + 1, seed, salt);
    let nx0 = mix(n00, n10, u.x);
    let nx1 = mix(n01, n11, u.x);
    return mix(nx0, nx1, u.y) * 2.0 - 1.0;
}

fn organic_grain_sample(frag_xy: vec2<f32>, grain_size_px: f32, seed: u32, salt: u32) -> f32 {
    let p = frag_xy / max(grain_size_px, 0.5);
    let oct1 = value_noise_2d(p, seed, salt);
    let oct2 = value_noise_2d(p * 2.13 + vec2<f32>(31.7, 17.3), seed, salt ^ 0x9e3779b9u);
    return oct1 * 0.68 + oct2 * 0.32;
}

fn eval_distorted_uv(uv: vec2<f32>, aspect: f32) -> vec3<f32> {
    let centered = uv - vec2<f32>(0.5, 0.5);
    let p = vec2<f32>(centered.x * aspect, centered.y);
    let r2 = dot(p, p);
    let r4 = r2 * r2;
    let radial_scale = 1.0 + params.lens_k1 * r2 + params.lens_k2 * r4;

    var auto_zoom = 1.0;
    if ((params.auto_scale_flags & 1u) != 0u) {
        let r2_ex = 0.25 * aspect * aspect;
        let r2_ey = 0.25;
        let r2_co = r2_ex + r2_ey;
        let s_ex = 1.0 + params.lens_k1 * r2_ex + params.lens_k2 * r2_ex * r2_ex;
        let s_ey = 1.0 + params.lens_k1 * r2_ey + params.lens_k2 * r2_ey * r2_ey;
        let s_co = 1.0 + params.lens_k1 * r2_co + params.lens_k2 * r2_co * r2_co;
        let s_bound = max(max(s_ex, s_ey), s_co);
        auto_zoom = clamp(max(s_bound, 1.0) * (1.0 + abs(params.chromatic_edge) * 1.5), 0.35, 4.0);
    }

    let p_dist = (p * radial_scale) / auto_zoom;
    let uv_dist = vec2<f32>(p_dist.x / aspect, p_dist.y) + vec2<f32>(0.5, 0.5);
    return vec3<f32>(uv_dist, r2);
}

fn eval_lens_and_ca(uv: vec2<f32>, aspect: f32) -> vec4<f32> {
    let dist_info = eval_distorted_uv(uv, aspect);
    let uv_dist = dist_info.xy;
    let r2 = dist_info.z;

    let ca_amt = params.chromatic_edge;
    if (abs(ca_amt) <= 1e-5) {
        return sample_bounded(uv_dist);
    }

    // 7-tap continuous spectral CA with normalized channel weights (sum_R = sum_G = sum_B = 1.0)
    let ca_shift = (uv_dist - vec2<f32>(0.5, 0.5)) * ca_amt * (0.5 + r2 * 2.0);
    var spectral_weights = array<vec3<f32>, 7>(
        vec3<f32>(0.42, 0.00, 0.00),
        vec3<f32>(0.36, 0.12, 0.00),
        vec3<f32>(0.18, 0.30, 0.02),
        vec3<f32>(0.04, 0.38, 0.08),
        vec3<f32>(0.00, 0.16, 0.30),
        vec3<f32>(0.00, 0.04, 0.38),
        vec3<f32>(0.00, 0.00, 0.22),
    );

    var rgb_acc = vec3<f32>(0.0);
    var center_a = 0.0;
    for (var i: i32 = 0; i < 7; i = i + 1) {
        let t = (f32(i) - 3.0) / 3.0;
        let tap = sample_bounded(uv_dist + ca_shift * t);
        rgb_acc = rgb_acc + max(tap.rgb, vec3<f32>(0.0)) * spectral_weights[i];
        if (i == 3) {
            center_a = tap.a;
        }
    }
    return vec4<f32>(rgb_acc, center_a);
}

fn apply_grain_and_vignette(col_in: vec4<f32>, uv: vec2<f32>, frag_xy: vec2<f32>, aspect: f32) -> vec4<f32> {
    var out_rgb = max(col_in.rgb, vec3<f32>(0.0));
    let out_a = clamp(col_in.a, 0.0, 1.0);
    let max_rgb = max3(out_rgb);

    let grain_amt = max(params.grain_amount, 0.0);
    if (grain_amt > 1e-5 && (out_a > ALPHA_EPS || max_rgb > ALPHA_EPS)) {
        let is_pure_additive = out_a <= ALPHA_EPS;
        let denom = select(out_a, 1.0, is_pure_additive);
        let seed_u = u32(max(params.grain_seed, 0.0) * 1024.0 + 1.0);
        let grain_size = max(params.grain_size_px, 0.5);
        let chroma_mix = clamp(params.grain_chroma, 0.0, 1.0);

        let n_luma = organic_grain_sample(frag_xy, grain_size, seed_u, 0x9e3779b9u);
        var grain_vec = vec3<f32>(n_luma);
        if (chroma_mix > 1e-4) {
            let n_r = organic_grain_sample(frag_xy, grain_size, seed_u, 0x85ebca6bu);
            let n_g = organic_grain_sample(frag_xy, grain_size, seed_u, 0xc2b2ae35u);
            let n_b = organic_grain_sample(frag_xy, grain_size, seed_u, 0x27d4eb2fu);
            grain_vec = mix(grain_vec, vec3<f32>(n_r, n_g, n_b), chroma_mix);
        }

        let straight_rgb = max(out_rgb / denom, vec3<f32>(0.0));
        let luma = dot(straight_rgb, LUMA_REC709);
        let mid_weight = smoothstep(0.01, 0.12, luma) * (1.0 - 0.60 * smoothstep(0.55, 1.8, luma));
        let delta = grain_vec * (grain_amt * 0.18 * mid_weight) * (0.35 + 0.65 * straight_rgb);
        out_rgb = max(out_rgb + delta * denom, vec3<f32>(0.0));
    }

    let vig_amt = clamp(params.vignette_amount, 0.0, 1.0);
    if (vig_amt > 1e-5) {
        let centered = uv - vec2<f32>(0.5, 0.5);
        let p = vec2<f32>(centered.x * aspect, centered.y);
        let half_diag = max(length(vec2<f32>(0.5 * aspect, 0.5)), 1e-4);
        let r_norm = length(p) / half_diag;
        let falloff = smoothstep(0.35, 1.15, r_norm);
        let vig_factor = 1.0 - vig_amt * falloff * 0.85;
        out_rgb = out_rgb * vig_factor;
    }

    return vec4<f32>(out_rgb, out_a);
}

// Pass A (used when halation_amount > 1e-5): lens distortion + 7-tap spectral CA only
@fragment
fn fs_lens(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = max(vec2<f32>(textureDimensions(src_tex)), vec2<f32>(1.0));
    let aspect = select(params.aspect, dims.x / dims.y, params.aspect <= 1e-4);
    return eval_lens_and_ca(in.uv, aspect);
}

// Pass C (used when halation_amount > 1e-5): merge pre-distorted src_tex + blurred halation_tex + grain + vignette
@fragment
fn fs_composite(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = max(vec2<f32>(textureDimensions(src_tex)), vec2<f32>(1.0));
    let aspect = select(params.aspect, dims.x / dims.y, params.aspect <= 1e-4);
    let base_col = textureSampleLevel(src_tex, linear_sampler, clamp(in.uv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0);
    let hal_rgb = max(textureSampleLevel(halation_tex, linear_sampler, clamp(in.uv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).rgb, vec3<f32>(0.0));
    let hal_amt = max(params.halation_amount, 0.0);
    let combined = vec4<f32>(max(base_col.rgb, vec3<f32>(0.0)) + hal_rgb * (hal_amt * 0.65), base_col.a);
    return apply_grain_and_vignette(combined, in.uv, in.position.xy, aspect);
}

// Single-pass fast path (used when halation_amount <= 1e-5)
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = max(vec2<f32>(textureDimensions(src_tex)), vec2<f32>(1.0));
    let aspect = select(params.aspect, dims.x / dims.y, params.aspect <= 1e-4);
    let lens_col = eval_lens_and_ca(in.uv, aspect);
    return apply_grain_and_vignette(lens_col, in.uv, in.position.xy, aspect);
}
