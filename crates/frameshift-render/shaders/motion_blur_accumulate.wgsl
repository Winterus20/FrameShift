// FrameShift M1 Built-in Shader: motion_blur_accumulate.wgsl
// Accumulates sub-frame temporal motion blur samples in Rgba16Float and performs
// Reinhard / ACES-lite / AgX / Khronos PBR Neutral highlight compression +
// optional TPDF/IGN dithering + Linear->sRGB encoding for Rgba8Unorm output readback.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct MotionBlurUniform {
    // x = weight (sub-frame sample weight, e.g. 1.0 / sub_samples)
    // y = accum_scale (1.0 for ping-pong accumulation, 0.0 for clear/first pass)
    // z = exposure (pre-tonemap linear multiplier; 1.0 = identity)
    // w = dither_strength (0.0 = off, 1.0 = +/-1 LSB TPDF)
    weights: vec4<f32>,
    // x = pass_mode:
    //     0u = accumulate ping-pong (tex_accum * accum_scale + tex_sample * weight)
    //     1u = weighted sample only (tex_sample * weight; for hardware additive blend)
    //     2u = tonemap + Linear->sRGB encoding (reads tex_sample -> Rgba8Unorm output)
    // y = tonemap_mode (0u = None/Clamp, 1u = Reinhard, 2u = ACES-lite, 3u = AgX, 4u = PBR Neutral, 5u = AgX Punchy)
    // z = output_alpha_mode (0u = Premultiplied sRGB, 1u = Straight sRGB)
    // w = dither_mode (0u = Off, 1u = TPDF + IGN)
    modes: vec4<u32>,
}

@group(0) @binding(0)
var tex_sample: texture_2d<f32>;

@group(0) @binding(1)
var tex_accum: texture_2d<f32>;

@group(0) @binding(2)
var s_linear: sampler;

@group(0) @binding(3)
var<uniform> params: MotionBlurUniform;

const ALPHA_EPS: f32 = 1e-5;
const LUMA_REC709: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);

// Linear Rec.709 (D65) -> Linear Rec.2020 (D65), column-major
const M_REC709_TO_REC2020: mat3x3<f32> = mat3x3<f32>(
    vec3<f32>(0.62740390, 0.06909729, 0.01639144),
    vec3<f32>(0.32928304, 0.91954039, 0.08801331),
    vec3<f32>(0.04331306, 0.01136231, 0.89559525),
);

// Linear Rec.2020 (D65) -> Linear Rec.709 (D65), column-major
const M_REC2020_TO_REC709: mat3x3<f32> = mat3x3<f32>(
    vec3<f32>( 1.66049100, -0.12455047, -0.01815076),
    vec3<f32>(-0.58764114,  1.13289990, -0.10057890),
    vec3<f32>(-0.07284986, -0.00834942,  1.11872966),
);

// Troy Sobotka / Blender / Filament AgX Inset Matrix (Rec.2020 primaries, column-major)
const AGX_INSET_MATRIX: mat3x3<f32> = mat3x3<f32>(
    vec3<f32>(0.8566271533, 0.1373189729, 0.1118982130),
    vec3<f32>(0.0951212405, 0.7612419906, 0.0767994186),
    vec3<f32>(0.0482516061, 0.1014390365, 0.8113023684),
);

// Troy Sobotka / Blender / Filament AgX Outset Matrix, column-major
const AGX_OUTSET_MATRIX: mat3x3<f32> = mat3x3<f32>(
    vec3<f32>( 1.1271005818, -0.1413297635, -0.1413297635),
    vec3<f32>(-0.1106066431,  1.1578237022, -0.1106066431),
    vec3<f32>(-0.0164939387, -0.0164939387,  1.2519364066),
);

const AGX_MIN_EV: f32 = -12.47393;
const AGX_MAX_EV: f32 =  4.026069;

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

fn linear_to_srgb_channel(c: f32) -> f32 {
    let x = clamp(c, 0.0, 1.0);
    return select(1.055 * pow(x, 1.0 / 2.4) - 0.055, x * 12.92, x <= 0.0031308);
}

fn linear_to_srgb(rgb: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        linear_to_srgb_channel(rgb.r),
        linear_to_srgb_channel(rgb.g),
        linear_to_srgb_channel(rgb.b),
    );
}

fn tonemap_reinhard(x: vec3<f32>) -> vec3<f32> {
    let v = max(x, vec3<f32>(0.0));
    return v / (vec3<f32>(1.0) + v);
}

fn tonemap_aces_lite(x: vec3<f32>) -> vec3<f32> {
    let v = max(x, vec3<f32>(0.0));
    let num = v * (2.51 * v + vec3<f32>(0.03));
    let den = v * (2.43 * v + vec3<f32>(0.59)) + vec3<f32>(0.14);
    return clamp(num / den, vec3<f32>(0.0), vec3<f32>(1.0));
}

fn agx_default_contrast_approx(x: vec3<f32>) -> vec3<f32> {
    let x2 = x * x;
    let x4 = x2 * x2;
    let x6 = x4 * x2;
    return -17.86 * x6 * x
        + 78.01 * x6
        - 126.7 * x4 * x
        + 92.06 * x4
        - 28.72 * x2 * x
        + 4.361 * x2
        - 0.1718 * x
        + vec3<f32>(0.002857);
}

fn tonemap_agx(x: vec3<f32>, punchy: bool) -> vec3<f32> {
    var val = M_REC709_TO_REC2020 * max(x, vec3<f32>(0.0));
    val = AGX_INSET_MATRIX * val;
    val = max(val, vec3<f32>(1e-10));
    val = log2(val);
    val = (val - AGX_MIN_EV) / (AGX_MAX_EV - AGX_MIN_EV);
    val = clamp(val, vec3<f32>(0.0), vec3<f32>(1.0));
    val = agx_default_contrast_approx(val);
    val = AGX_OUTSET_MATRIX * val;
    val = max(val, vec3<f32>(0.0));
    if (punchy) {
        let luma = dot(val, LUMA_REC709);
        val = pow(val, vec3<f32>(1.35));
        val = max(mix(vec3<f32>(luma), val, 1.4), vec3<f32>(0.0));
    }
    val = pow(val, vec3<f32>(2.2));
    val = M_REC2020_TO_REC709 * val;
    return clamp(val, vec3<f32>(0.0), vec3<f32>(1.0));
}

fn tonemap_pbr_neutral(color_in: vec3<f32>) -> vec3<f32> {
    let start_compression = 0.8 - 0.04;
    let desaturation = 0.15;
    var color = max(color_in, vec3<f32>(0.0));

    let x = min(color.r, min(color.g, color.b));
    let offset = select(0.04, x - 6.25 * x * x, x < 0.08);
    color = color - vec3<f32>(offset);

    let peak = max3(color);
    if (peak < start_compression) {
        return clamp(color, vec3<f32>(0.0), vec3<f32>(1.0));
    }
    let d = 1.0 - start_compression;
    let new_peak = 1.0 - (d * d) / (peak + d - start_compression);
    color = color * (new_peak / peak);

    let g = 1.0 - 1.0 / (desaturation * (peak - new_peak) + 1.0);
    return clamp(mix(color, vec3<f32>(new_peak), g), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn ign(pixel_xy: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(pixel_xy, vec2<f32>(0.06711056, 0.00583715))));
}

fn dither_srgb_tpdf(srgb: vec3<f32>, frag_xy: vec2<f32>, strength: f32) -> vec3<f32> {
    let u1 = ign(frag_xy);
    let u2 = ign(frag_xy + vec2<f32>(17.0, 59.0));
    let tpdf = ((u1 + u2 - 1.0) / 255.0) * strength;
    let boundary_guard = smoothstep(vec3<f32>(0.0), vec3<f32>(1.0 / 255.0), srgb)
                       * smoothstep(vec3<f32>(1.0), vec3<f32>(254.0 / 255.0), srgb);
    return clamp(srgb + vec3<f32>(tpdf) * boundary_guard, vec3<f32>(0.0), vec3<f32>(1.0));
}

fn evaluate_tonemap_to_srgb(uv: vec2<f32>, frag_xy: vec2<f32>) -> vec4<f32> {
    let col = textureSampleLevel(tex_sample, s_linear, uv, 0.0);
    let a = clamp(col.a, 0.0, 1.0);
    let max_rgb = max3(max(col.rgb, vec3<f32>(0.0)));
    if (a <= ALPHA_EPS && max_rgb <= ALPHA_EPS) {
        return vec4<f32>(0.0);
    }

    let is_pure_additive = a <= ALPHA_EPS;
    let denom = select(a, 1.0, is_pure_additive);
    let exposure = max(params.weights.z, 0.0);
    var rgb = max(col.rgb / denom, vec3<f32>(0.0)) * exposure;

    let tonemap_mode = params.modes.y;
    if (tonemap_mode == 1u) {
        rgb = tonemap_reinhard(rgb);
    } else if (tonemap_mode == 2u) {
        rgb = tonemap_aces_lite(rgb);
    } else if (tonemap_mode == 3u) {
        rgb = tonemap_agx(rgb, false);
    } else if (tonemap_mode == 4u) {
        rgb = tonemap_pbr_neutral(rgb);
    } else if (tonemap_mode == 5u) {
        rgb = tonemap_agx(rgb, true);
    }

    var srgb = linear_to_srgb(rgb);
    if (params.modes.w != 0u) {
        srgb = dither_srgb_tpdf(srgb, frag_xy, max(params.weights.w, 0.0));
    }

    if (params.modes.z == 1u) {
        return vec4<f32>(srgb, a);
    }
    let out_rgb = select(srgb * a, srgb, is_pure_additive);
    return vec4<f32>(out_rgb, a);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let pass_mode = params.modes.x;
    if (pass_mode == 2u) {
        return evaluate_tonemap_to_srgb(in.uv, in.position.xy);
    }

    let sample_col = textureSampleLevel(tex_sample, s_linear, in.uv, 0.0);
    let weight = params.weights.x;

    if (pass_mode == 1u) {
        return sample_col * weight;
    }

    let accum_col = textureSampleLevel(tex_accum, s_linear, in.uv, 0.0);
    let accum_scale = params.weights.y;
    return accum_col * accum_scale + sample_col * weight;
}

@fragment
fn fs_tonemap_to_srgb(in: VertexOutput) -> @location(0) vec4<f32> {
    return evaluate_tonemap_to_srgb(in.uv, in.position.xy);
}
