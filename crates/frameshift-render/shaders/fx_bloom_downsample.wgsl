// FrameShift WGSL Post-FX: fx_bloom_downsample.wgsl
// Jimenez 2014 13-tap box downsample ("Next Generation Post Processing in Call of Duty: Advanced Warfare")
// with Karis-average luminance weighting, soft-knee HDR threshold on the first pass,
// and optional anamorphic horizontal step scaling.
// Working format: Rgba16Float, Linear Rec.709, premultiplied alpha.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct BloomPassUniform {
    texel_size: vec2<f32>,
    threshold: f32,
    knee: f32,
    intensity: f32,
    radius: f32,
    is_first_pass: f32,
    is_final_composite: f32,
    tint_rgb: vec3<f32>,
    scatter: f32,
    anamorphic_ratio: f32,
    chromatic_shift: f32,
    mip_norm: f32,
    _pad: f32,
}

@group(0) @binding(0)
var src_tex: texture_2d<f32>;

@group(0) @binding(1)
var linear_sampler: sampler;

@group(0) @binding(2)
var<uniform> params: BloomPassUniform;

const LUMA_REC709: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);
const EPS: f32 = 1e-5;

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VertexOutput {
    let x = f32((vid << 1u) & 2u);
    let y = f32(vid & 2u);
    var out: VertexOutput;
    out.position = vec4<f32>(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, 1.0 - y);
    return out;
}

fn luma_rec709(rgb: vec3<f32>) -> f32 {
    return dot(max(rgb, vec3<f32>(0.0)), LUMA_REC709);
}

fn soft_knee_threshold(rgb: vec3<f32>, threshold: f32, knee: f32) -> vec3<f32> {
    let c = max(rgb, vec3<f32>(0.0));
    let br = max(c.r, max(c.g, c.b));
    let thresh = max(threshold, 0.0);
    let k = max(knee, 0.0);
    let soft = clamp(br - thresh + k, 0.0, 2.0 * k);
    let soft_term = (soft * soft) / (4.0 * k + EPS);
    let contrib = max(br - thresh, soft_term) / max(br, EPS);
    return c * max(contrib, 0.0);
}

fn sample_tap(uv: vec2<f32>, first_pass: bool) -> vec3<f32> {
    let clamped_uv = clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0));
    let rgb = max(textureSampleLevel(src_tex, linear_sampler, clamped_uv, 0.0).rgb, vec3<f32>(0.0));
    if (first_pass) {
        return soft_knee_threshold(rgb, params.threshold, params.knee);
    }
    return rgb;
}

fn karis_weight(group_rgb: vec3<f32>) -> f32 {
    return 1.0 / (1.0 + luma_rec709(group_rgb));
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = max(vec2<f32>(textureDimensions(src_tex)), vec2<f32>(1.0));
    let use_auto = params.texel_size.x <= 1e-6 || params.texel_size.y <= 1e-6;
    let texel = select(params.texel_size, 1.0 / dims, use_auto);
    let anamorphic = clamp(params.anamorphic_ratio, 0.0, 1.0);
    let x = texel.x * (1.0 + anamorphic * 1.25);
    let y = texel.y * max(1.0 - anamorphic * 0.35, 0.4);

    let first_pass = params.is_first_pass > 0.5;
    let uv = in.uv;

    // 13-tap Jimenez pattern:
    // a - b - c
    // - j - k -
    // d - e - f
    // - l - m -
    // g - h - i
    let a = sample_tap(uv + vec2<f32>(-2.0 * x,  2.0 * y), first_pass);
    let b = sample_tap(uv + vec2<f32>( 0.0,      2.0 * y), first_pass);
    let c = sample_tap(uv + vec2<f32>( 2.0 * x,  2.0 * y), first_pass);

    let d = sample_tap(uv + vec2<f32>(-2.0 * x,  0.0), first_pass);
    let e = sample_tap(uv, first_pass);
    let f = sample_tap(uv + vec2<f32>( 2.0 * x,  0.0), first_pass);

    let g = sample_tap(uv + vec2<f32>(-2.0 * x, -2.0 * y), first_pass);
    let h = sample_tap(uv + vec2<f32>( 0.0,     -2.0 * y), first_pass);
    let i = sample_tap(uv + vec2<f32>( 2.0 * x, -2.0 * y), first_pass);

    let j = sample_tap(uv + vec2<f32>(-x,  y), first_pass);
    let k = sample_tap(uv + vec2<f32>( x,  y), first_pass);
    let l = sample_tap(uv + vec2<f32>(-x, -y), first_pass);
    let m = sample_tap(uv + vec2<f32>( x, -y), first_pass);

    let g0 = (a + b + d + e) * 0.25;
    let g1 = (b + c + e + f) * 0.25;
    let g2 = (d + e + g + h) * 0.25;
    let g3 = (e + f + h + i) * 0.25;
    let g4 = (j + k + l + m) * 0.25;

    var out_rgb: vec3<f32>;
    if (first_pass) {
        let w0 = 0.125 * karis_weight(g0);
        let w1 = 0.125 * karis_weight(g1);
        let w2 = 0.125 * karis_weight(g2);
        let w3 = 0.125 * karis_weight(g3);
        let w4 = 0.5 * karis_weight(g4);
        let w_sum = max(w0 + w1 + w2 + w3 + w4, EPS);
        out_rgb = (g0 * w0 + g1 * w1 + g2 * w2 + g3 * w3 + g4 * w4) / w_sum;
    } else {
        out_rgb = g4 * 0.5 + (g0 + g1 + g2 + g3) * 0.125;
    }

    return vec4<f32>(max(out_rgb, vec3<f32>(0.0)), 1.0);
}
