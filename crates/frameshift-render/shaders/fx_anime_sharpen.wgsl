// FrameShift WGSL Post-FX: fx_anime_sharpen.wgsl
// Inspired by bloc97/Anime4K (MIT): single-pass luma-gradient dark-line thinning
// and anti-ringing unsharp detail enhancement in Linear Rec.709 premultiplied alpha.
// Working format: Rgba16Float, Linear Rec.709, premultiplied alpha.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct AnimeSharpenUniform {
    texel_size: vec2<f32>,
    line_strength: f32,
    sharpen_amount: f32,
    anti_ringing: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
}

@group(0) @binding(0)
var src_tex: texture_2d<f32>;

@group(0) @binding(1)
var linear_sampler: sampler;

@group(0) @binding(2)
var<uniform> params: AnimeSharpenUniform;

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

fn sample_unpremul(uv: vec2<f32>) -> vec3<f32> {
    let clamped = clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0));
    let s = textureSampleLevel(src_tex, linear_sampler, clamped, 0.0);
    let max_rgb = max(s.r, max(s.g, s.b));
    if (s.a <= ALPHA_EPS && max_rgb <= ALPHA_EPS) {
        return vec3<f32>(0.0);
    }
    let denom = select(s.a, 1.0, s.a <= ALPHA_EPS);
    return max(s.rgb / denom, vec3<f32>(0.0));
}

fn perceptual_luma(rgb: vec3<f32>) -> f32 {
    return sqrt(max(dot(rgb, LUMA_REC709), 0.0));
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = clamp(in.uv, vec2<f32>(0.0), vec2<f32>(1.0));
    let center_raw = textureSampleLevel(src_tex, linear_sampler, uv, 0.0);
    let center_max = max(center_raw.r, max(center_raw.g, center_raw.b));
    if (center_raw.a <= ALPHA_EPS && center_max <= ALPHA_EPS) {
        return vec4<f32>(0.0);
    }

    let line_strength = clamp(params.line_strength, 0.0, 2.0);
    let sharpen_amount = clamp(params.sharpen_amount, 0.0, 4.0);
    let anti_ringing = clamp(params.anti_ringing, 0.0, 1.0);

    if (line_strength <= 1e-5 && sharpen_amount <= 1e-5) {
        return center_raw;
    }

    let is_pure_additive = center_raw.a <= ALPHA_EPS;
    let center_denom = select(center_raw.a, 1.0, is_pure_additive);

    let dims = max(vec2<f32>(textureDimensions(src_tex)), vec2<f32>(1.0));
    let use_auto = params.texel_size.x <= 1e-6 || params.texel_size.y <= 1e-6;
    let texel = select(params.texel_size, 1.0 / dims, use_auto);
    let dx = texel.x;
    let dy = texel.y;

    let tl = sample_unpremul(uv + vec2<f32>(-dx,  dy));
    let tc = sample_unpremul(uv + vec2<f32>(0.0,  dy));
    let tr = sample_unpremul(uv + vec2<f32>( dx,  dy));
    let ml = sample_unpremul(uv + vec2<f32>(-dx, 0.0));
    let mc = max(center_raw.rgb / center_denom, vec3<f32>(0.0));
    let mr = sample_unpremul(uv + vec2<f32>( dx, 0.0));
    let bl = sample_unpremul(uv + vec2<f32>(-dx, -dy));
    let bc = sample_unpremul(uv + vec2<f32>(0.0, -dy));
    let br = sample_unpremul(uv + vec2<f32>( dx, -dy));

    // Perceptual luma for Sobel gradient along dark anime ink edges.
    let l_tl = perceptual_luma(tl);
    let l_tc = perceptual_luma(tc);
    let l_tr = perceptual_luma(tr);
    let l_ml = perceptual_luma(ml);
    let l_mc = perceptual_luma(mc);
    let l_mr = perceptual_luma(mr);
    let l_bl = perceptual_luma(bl);
    let l_bc = perceptual_luma(bc);
    let l_br = perceptual_luma(br);

    let gx = (l_tr + 2.0 * l_mr + l_br) - (l_tl + 2.0 * l_ml + l_bl);
    let gy = (l_bl + 2.0 * l_bc + l_br) - (l_tl + 2.0 * l_tc + l_tr);
    let grad = vec2<f32>(gx, gy);
    let grad_mag = length(grad);

    var grad_dir = vec2<f32>(0.0);
    if (grad_mag > 1e-5) {
        grad_dir = grad / grad_mag;
    }

    // Sample along gradient direction (toward brighter side) and opposite (darker side).
    let c_pos = sample_unpremul(uv + grad_dir * texel);
    let c_neg = sample_unpremul(uv - grad_dir * texel);
    let l_pos = perceptual_luma(c_pos);
    let l_neg = perceptual_luma(c_neg);

    // Dark-line thinning: push dark edge pixels slightly toward the brighter gradient neighbor
    // only where a meaningful luma gradient exists, leaving flat fills untouched.
    let edge_mask = smoothstep(0.015, 0.18, grad_mag);
    let is_dark_slope = clamp((l_pos - l_mc) / max(l_pos - l_neg + 0.05, 0.05), 0.0, 1.0);
    let thin_weight = clamp(line_strength * 0.5 * edge_mask * is_dark_slope, 0.0, 0.85);
    let thinned = mix(mc, c_pos, thin_weight);

    // Anti-ringing unsharp detail enhancement clamped to local 3x3 neighborhood bounds.
    let cross_avg = (tc + ml + mr + bc) * 0.25;
    let detail = thinned - cross_avg;
    let sharpened = max(thinned + detail * sharpen_amount, vec3<f32>(0.0));

    var min_rgb = min(min(min(tl, tc), min(tr, ml)), min(min(mc, mr), min(bl, bc)));
    min_rgb = min(min_rgb, br);
    var max_rgb = max(max(max(tl, tc), max(tr, ml)), max(max(mc, mr), max(bl, bc)));
    max_rgb = max(max_rgb, br);

    let clamped_rgb = clamp(sharpened, min_rgb, max_rgb);
    let final_rgb = mix(sharpened, clamped_rgb, anti_ringing);
    let out_rgb = select(final_rgb * center_raw.a, final_rgb, is_pure_additive);

    return vec4<f32>(out_rgb, center_raw.a);
}
