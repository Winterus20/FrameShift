// FrameShift WGSL Post-FX: fx_anime_impact_frame.wgsl
// Adapted from Vidvox/ISF-Files (Edges.fs, Edge Trace.fs, RGB Invert.fs - MIT) and
// godotshaders.com/shader/speed-lines (CC0).
// 3x3 Linear Rec.709 Sobel edge detection, high-contrast ink threshold,
// deterministic polar radial speed lines, and negative/invert impact flash.
// Working format: Rgba16Float, Linear Rec.709, premultiplied alpha.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct AnimeImpactFrameUniform {
    // x = luma_threshold, y = edge_weight, z = invert_amount, w = mix_amount
    thresholds: vec4<f32>,
    // x = speed_line_density, y = speed_line_intensity, z = center_u, w = center_v
    speed_lines: vec4<f32>,
    // x = aspect_ratio, y = seed, z = 0.0, w = 0.0
    extra: vec4<f32>,
}

@group(0) @binding(0)
var t_source: texture_2d<f32>;

@group(0) @binding(1)
var s_linear: sampler;

@group(0) @binding(2)
var<uniform> params: AnimeImpactFrameUniform;

const TAU: f32 = 6.28318530718;
const ALPHA_EPS: f32 = 1e-5;
const LUMA_REC709: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);
const SPEED_LINE_SLOTS: f32 = 96.0;

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VertexOutput {
    let x = f32((vid << 1u) & 2u);
    let y = f32(vid & 2u);
    var out: VertexOutput;
    out.position = vec4<f32>(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, 1.0 - y);
    return out;
}

fn sample_luma(uv: vec2<f32>) -> f32 {
    let s = textureSampleLevel(t_source, s_linear, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0);
    let max_rgb = max(s.r, max(s.g, s.b));
    if (s.a <= ALPHA_EPS && max_rgb <= ALPHA_EPS) {
        return 0.0;
    }
    let denom = select(s.a, 1.0, s.a <= ALPHA_EPS);
    let rgb = max(s.rgb / denom, vec3<f32>(0.0));
    return dot(rgb, LUMA_REC709);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let src = textureSampleLevel(t_source, s_linear, in.uv, 0.0);
    let max_rgb = max(src.r, max(src.g, src.b));
    if (src.a <= ALPHA_EPS && max_rgb <= ALPHA_EPS) {
        return vec4<f32>(0.0);
    }

    let is_pure_additive = src.a <= ALPHA_EPS;
    let denom = select(src.a, 1.0, is_pure_additive);
    let base_rgb = max(src.rgb / denom, vec3<f32>(0.0));
    let center_luma = dot(base_rgb, LUMA_REC709);

    let dims = max(vec2<f32>(textureDimensions(t_source)), vec2<f32>(1.0));
    let texel = 1.0 / dims;

    // 3x3 Linear Rec.709 Sobel edge detection.
    let l_la = sample_luma(in.uv + vec2<f32>(-texel.x, -texel.y));
    let l_a  = sample_luma(in.uv + vec2<f32>(0.0,      -texel.y));
    let l_ra = sample_luma(in.uv + vec2<f32>(texel.x,  -texel.y));
    let l_l  = sample_luma(in.uv + vec2<f32>(-texel.x, 0.0));
    let l_r  = sample_luma(in.uv + vec2<f32>(texel.x,  0.0));
    let l_lb = sample_luma(in.uv + vec2<f32>(-texel.x, texel.y));
    let l_b  = sample_luma(in.uv + vec2<f32>(0.0,      texel.y));
    let l_rb = sample_luma(in.uv + vec2<f32>(texel.x,  texel.y));

    let gx = (-l_la - 2.0 * l_l - l_lb) + (l_ra + 2.0 * l_r + l_rb);
    let gy = (l_la + 2.0 * l_a + l_ra) + (-l_lb - 2.0 * l_b - l_rb);
    let edge_mag = sqrt(gx * gx + gy * gy);

    // High-contrast ink threshold + Sobel contour emphasis.
    let luma_thresh = max(params.thresholds.x, 0.0);
    let edge_weight = max(params.thresholds.y, 0.0);
    let invert_amount = clamp(params.thresholds.z, 0.0, 1.0);
    let mix_amount = clamp(params.thresholds.w, 0.0, 1.0);

    let bw_low = max(luma_thresh - 0.03, 0.0);
    let bw_high = bw_low + 0.06;
    let bw_mask = smoothstep(bw_low, bw_high, center_luma);
    var impact_rgb = vec3<f32>(bw_mask);

    let edge_mask = smoothstep(0.15, 0.45, edge_mag * edge_weight);
    impact_rgb = mix(impact_rgb, vec3<f32>(0.0), edge_mask * clamp(edge_weight, 0.0, 1.0));

    // Deterministic polar radial speed lines around (center_u, center_v).
    let density = clamp(params.speed_lines.x, 0.0, 1.0);
    let line_intensity = clamp(params.speed_lines.y, 0.0, 1.0);
    let center = params.speed_lines.zw;
    let aspect = select(params.extra.x, dims.x / dims.y, params.extra.x <= 1e-4);
    let seed = params.extra.y;

    let d = vec2<f32>((in.uv.x - center.x) * aspect, in.uv.y - center.y);
    let r = length(d);
    let safe_d = select(vec2<f32>(1.0, 0.0), d, r > 1e-6);
    let theta = atan2(safe_d.y, safe_d.x) / TAU + 0.5;

    let slot_coord = theta * SPEED_LINE_SLOTS;
    let slot_idx = floor(slot_coord);
    let slot_hash = fract(sin(slot_idx * 127.1 + seed * 311.7) * 43758.5453);
    let local_a = fract(slot_coord) * 2.0 - 1.0;

    let line_hit = step(1.0 - density, slot_hash)
        * (1.0 - smoothstep(0.0, max(1.0 - slot_hash * 0.35, 0.1), abs(local_a)));
    let radial_mask = smoothstep(0.12, 0.48, r);
    let line_mask = line_hit * radial_mask * line_intensity;

    // Contrast-inverting speed lines pop cleanly on both black and white impact fields.
    impact_rgb = mix(impact_rgb, vec3<f32>(1.0) - impact_rgb, line_mask);

    // Negative / invert impact flash.
    let inverted_rgb = max(vec3<f32>(1.0) - clamp(impact_rgb, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(0.0));
    impact_rgb = mix(impact_rgb, inverted_rgb, invert_amount);

    let out_rgb = mix(base_rgb, impact_rgb, mix_amount);
    let premul_rgb = select(out_rgb * src.a, out_rgb, is_pure_additive);
    return vec4<f32>(premul_rgb, src.a);
}
