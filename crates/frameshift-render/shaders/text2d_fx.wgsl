// FrameShift Faz 6.6 Shader: text2d_fx.wgsl
// Jump Flooding Algorithm (JFA) SDF 2D typography post-compositor over Slug HDR text fill:
// Soft Drop Shadow -> HDR Outer Glow -> Outer Anime Stroke (with optional Knockout Gap)
// -> Inner Anime Stroke -> Crisp Slug HDR Fill. Premultiplied Linear Rec.709 Rgba16Float.
// Uses ~4 texture reads/px (replacing the 352 tap/px raymarch) with glass-smooth corners.

struct Text2DFxUniform {
    // x = texel_x, y = texel_y, z = outline_px, w = outer_outline_px
    metrics0: vec4<f32>,
    // x = outline_gap_px, y = shadow_blur_px, z = glow_radius_px, w = opacity
    metrics1: vec4<f32>,
    // x = shadow_offset_x_px, y = shadow_offset_y_px, z = jfa_max_reach_px, w = reserved
    shadow_offset: vec4<f32>,
    outline_rgba: vec4<f32>,
    outer_outline_rgba: vec4<f32>,
    shadow_rgba: vec4<f32>,
    glow_rgba: vec4<f32>,
}

@group(0) @binding(0)
var t_text: texture_2d<f32>;
@group(0) @binding(1)
var t_sdf: texture_2d<f32>;
@group(0) @binding(2)
var s_linear: sampler;
@group(0) @binding(3)
var<uniform> u: Text2DFxUniform;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

const SENTINEL_THRESH: f32 = 8192.0;

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VertexOutput {
    let x = f32((vid << 1u) & 2u);
    let y = f32(vid & 2u);
    var out: VertexOutput;
    out.position = vec4<f32>(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, 1.0 - y);
    return out;
}

fn source_over(dst: vec4<f32>, src: vec4<f32>) -> vec4<f32> {
    let inv_a = 1.0 - clamp(src.a, 0.0, 1.0);
    return vec4<f32>(src.rgb + dst.rgb * inv_a, clamp(src.a + dst.a * inv_a, 0.0, 1.0));
}

fn premul_scaled(rgba: vec4<f32>, cov: f32) -> vec4<f32> {
    let a = clamp(rgba.a * cov, 0.0, 1.0);
    return vec4<f32>(max(rgba.rgb, vec3<f32>(0.0)) * a, a);
}

fn eval_ext_dist(jfa_val: vec4<f32>, fallback_dist: f32) -> f32 {
    if (abs(jfa_val.x) < SENTINEL_THRESH && abs(jfa_val.y) < SENTINEL_THRESH) {
        return length(jfa_val.xy);
    }
    return fallback_dist;
}

fn eval_signed_dist(jfa_val: vec4<f32>, fallback_dist: f32) -> f32 {
    let d_out = select(fallback_dist, length(jfa_val.xy), abs(jfa_val.x) < SENTINEL_THRESH && abs(jfa_val.y) < SENTINEL_THRESH);
    let d_in = select(fallback_dist, length(jfa_val.zw), abs(jfa_val.z) < SENTINEL_THRESH && abs(jfa_val.w) < SENTINEL_THRESH);
    return d_out - d_in;
}

fn sample_sd_bilinear(uv: vec2<f32>, dims: vec2<f32>, fallback_dist: f32) -> f32 {
    let fcoord = uv * dims - vec2<f32>(0.5);
    let p0 = vec2<i32>(floor(fcoord));
    let frac = fcoord - vec2<f32>(p0);
    let max_i = max(vec2<i32>(dims) - vec2<i32>(1, 1), vec2<i32>(0, 0));

    let c00 = clamp(p0, vec2<i32>(0), max_i);
    let c10 = clamp(p0 + vec2<i32>(1, 0), vec2<i32>(0), max_i);
    let c01 = clamp(p0 + vec2<i32>(0, 1), vec2<i32>(0), max_i);
    let c11 = clamp(p0 + vec2<i32>(1, 1), vec2<i32>(0), max_i);

    let d00 = eval_signed_dist(textureLoad(t_sdf, c00, 0), fallback_dist);
    let d10 = eval_signed_dist(textureLoad(t_sdf, c10, 0), fallback_dist);
    let d01 = eval_signed_dist(textureLoad(t_sdf, c01, 0), fallback_dist);
    let d11 = eval_signed_dist(textureLoad(t_sdf, c11, 0), fallback_dist);

    return mix(mix(d00, d10, frac.x), mix(d01, d11, frac.x), frac.y);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let texel = u.metrics0.xy;
    let dims = max(vec2<f32>(textureDimensions(t_sdf)), vec2<f32>(1.0));
    let max_i = max(vec2<i32>(dims) - vec2<i32>(1, 1), vec2<i32>(0, 0));
    let coord = clamp(vec2<i32>(floor(in.position.xy)), vec2<i32>(0), max_i);

    let outline_px = max(u.metrics0.z, 0.0);
    let outer_outline_px = max(u.metrics0.w, 0.0);
    let gap_px = max(u.metrics1.x, 0.0);
    let shadow_blur_px = max(u.metrics1.y, 0.0);
    let glow_radius_px = max(u.metrics1.z, 0.0);
    let op = clamp(u.metrics1.w, 0.0, 1.0);
    let max_reach = max(u.shadow_offset.z, 128.0);

    let fill = textureSampleLevel(t_text, s_linear, in.uv, 0.0);
    let jfa_here = textureLoad(t_sdf, coord, 0);
    let min_dist = eval_ext_dist(jfa_here, max_reach + 2.0);

    var acc = vec4<f32>(0.0);

    // 1. Soft Drop Shadow (1 lookup via SDF signed distance or crisp Slug alpha)
    if (u.shadow_rgba.a > 0.0001) {
        let sh_uv = in.uv - u.shadow_offset.xy * texel;
        let in_bounds = sh_uv.x >= 0.0 && sh_uv.x <= 1.0 && sh_uv.y >= 0.0 && sh_uv.y <= 1.0;
        var sh_cov = 0.0;
        if (in_bounds) {
            if (shadow_blur_px <= 0.5) {
                sh_cov = textureSampleLevel(t_text, s_linear, sh_uv, 0.0).a;
            } else {
                let sh_sd = sample_sd_bilinear(sh_uv, dims, max_reach + 2.0);
                let half_blur = max(shadow_blur_px * 0.65, 0.5);
                sh_cov = 1.0 - smoothstep(-half_blur, half_blur, sh_sd);
            }
        }
        if (sh_cov > 0.0005) {
            acc = source_over(acc, premul_scaled(u.shadow_rgba, sh_cov));
        }
    }

    // 2. HDR Outer Glow (0 extra taps: reuses JFA exterior distance)
    if (glow_radius_px > 0.05 && u.glow_rgba.a > 0.0001) {
        let sigma = max(glow_radius_px * 0.42, 0.5);
        let gauss_glow = exp(-(min_dist * min_dist) / (2.0 * sigma * sigma));
        let tail = 1.0 - smoothstep(glow_radius_px * 0.75, glow_radius_px * 1.35, min_dist);
        let glow_cov = clamp(gauss_glow * tail, 0.0, 1.0);
        if (glow_cov > 0.0005) {
            acc = source_over(acc, premul_scaled(u.glow_rgba, glow_cov));
        }
    }

    // 3. Multi-Layer Anime Outlines + Knockout Gap (glass-smooth JFA SDF circles/joins)
    let has_inner = outline_px > 0.05 && u.outline_rgba.a > 0.0001;
    let has_outer = outer_outline_px > 0.05 && u.outer_outline_rgba.a > 0.0001;
    if (has_outer) {
        let inner_edge = outline_px + gap_px;
        let outer_edge = inner_edge + outer_outline_px;
        let outer_band = clamp(outer_edge + 0.5 - min_dist, 0.0, 1.0);
        var outer_cov = outer_band;
        if (gap_px > 0.05) {
            let inner_cut = clamp(inner_edge + 0.5 - min_dist, 0.0, 1.0);
            outer_cov = outer_band * (1.0 - inner_cut);
        }
        if (outer_cov > 0.0005) {
            acc = source_over(acc, premul_scaled(u.outer_outline_rgba, outer_cov));
        }
    }

    if (has_inner) {
        let inner_cov = clamp(outline_px + 0.5 - min_dist, 0.0, 1.0);
        if (inner_cov > 0.0005) {
            acc = source_over(acc, premul_scaled(u.outline_rgba, inner_cov));
        }
    }

    // 4. Crisp Slug HDR Fill on top
    if (fill.a > 0.0001) {
        acc = source_over(acc, fill);
    }

    return vec4<f32>(acc.rgb * op, clamp(acc.a * op, 0.0, 1.0));
}
