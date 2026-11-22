// FrameShift WGSL Post-FX: fx_film_halation.wgsl
// Two-scale separable bilinear Gaussian red-orange emulsion halation pass:
// mode 0u: soft-knee highlight extraction + film emulsion R/G/B scatter weighting (1/2 or 1/4 downsample)
// mode 1u: 5-fetch 9-tap hardware-bilinear separable Gaussian blur along (dir_x, dir_y)
// mode 2u: dual-scale tight + wide halation merge
// Working format: Rgba16Float, Linear Rec.709, premultiplied alpha.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct FilmHalationUniform {
    texel_size: vec2<f32>,
    threshold: f32,
    knee: f32,
    mode: u32,
    dir_x: f32,
    dir_y: f32,
    _pad: f32,
}

@group(0) @binding(0)
var tex0: texture_2d<f32>;

@group(0) @binding(1)
var tex1: texture_2d<f32>;

@group(0) @binding(2)
var linear_sampler: sampler;

@group(0) @binding(3)
var<uniform> params: FilmHalationUniform;

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

fn emulsion_extract(rgb_in: vec3<f32>, thresh: f32, knee: f32) -> vec3<f32> {
    let rgb = max(rgb_in, vec3<f32>(0.0));
    let luma = dot(rgb, LUMA_REC709);
    let soft = clamp(luma - thresh + knee, 0.0, 2.0 * knee);
    let soft_term = (soft * soft) / (4.0 * knee + EPS);
    let contrib = max(luma - thresh, soft_term) / max(luma, EPS);
    let excess = max(contrib, 0.0);
    // Kodak Vision3 red-deepest / green-spill emulsion response
    let amber_spill = clamp((luma - thresh) * 0.45, 0.0, 1.0);
    let emulsion_tint = mix(vec3<f32>(1.0, 0.22, 0.04), vec3<f32>(1.0, 0.45, 0.10), amber_spill);
    return rgb * excess * emulsion_tint;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = max(vec2<f32>(textureDimensions(tex0)), vec2<f32>(1.0));
    let use_auto = params.texel_size.x <= 1e-6 || params.texel_size.y <= 1e-6;
    let texel = select(params.texel_size, 1.0 / dims, use_auto);
    let uv = clamp(in.uv, vec2<f32>(0.0), vec2<f32>(1.0));

    if (params.mode == 0u) {
        let thresh = max(params.threshold, 0.0);
        let knee = max(params.knee, 0.05);
        let d = texel * 0.5;
        let s0 = emulsion_extract(textureSampleLevel(tex0, linear_sampler, clamp(uv + vec2<f32>(-d.x, -d.y), vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).rgb, thresh, knee);
        let s1 = emulsion_extract(textureSampleLevel(tex0, linear_sampler, clamp(uv + vec2<f32>( d.x, -d.y), vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).rgb, thresh, knee);
        let s2 = emulsion_extract(textureSampleLevel(tex0, linear_sampler, clamp(uv + vec2<f32>(-d.x,  d.y), vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).rgb, thresh, knee);
        let s3 = emulsion_extract(textureSampleLevel(tex0, linear_sampler, clamp(uv + vec2<f32>( d.x,  d.y), vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).rgb, thresh, knee);
        return vec4<f32>((s0 + s1 + s2 + s3) * 0.25, 1.0);
    }

    if (params.mode == 2u) {
        let tight = max(textureSampleLevel(tex0, linear_sampler, uv, 0.0).rgb, vec3<f32>(0.0));
        let wide = max(textureSampleLevel(tex1, linear_sampler, uv, 0.0).rgb, vec3<f32>(0.0));
        return vec4<f32>(tight * 0.45 + wide * 0.55, 1.0);
    }

    // 5-fetch 9-tap hardware-bilinear separable Gaussian
    let step_uv = vec2<f32>(params.dir_x, params.dir_y) * texel;
    let c0 = max(textureSampleLevel(tex0, linear_sampler, uv, 0.0).rgb, vec3<f32>(0.0)) * 0.22702703;
    let c1p = max(textureSampleLevel(tex0, linear_sampler, clamp(uv + step_uv * 1.38461538, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).rgb, vec3<f32>(0.0)) * 0.31621622;
    let c1n = max(textureSampleLevel(tex0, linear_sampler, clamp(uv - step_uv * 1.38461538, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).rgb, vec3<f32>(0.0)) * 0.31621622;
    let c2p = max(textureSampleLevel(tex0, linear_sampler, clamp(uv + step_uv * 3.23076923, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).rgb, vec3<f32>(0.0)) * 0.07027027;
    let c2n = max(textureSampleLevel(tex0, linear_sampler, clamp(uv - step_uv * 3.23076923, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).rgb, vec3<f32>(0.0)) * 0.07027027;
    return vec4<f32>(c0 + c1p + c1n + c2p + c2n, 1.0);
}
