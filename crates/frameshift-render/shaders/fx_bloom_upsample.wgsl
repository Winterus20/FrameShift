// FrameShift WGSL Post-FX: fx_bloom_upsample.wgsl
// Jimenez 2014 9-tap 3x3 tent filter upsample with Unity HDRP exponential scatter falloff,
// optional anamorphic horizontal streak kernel, and mip-depth chromatic bloom tint.
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
    inv_sq_falloff: f32,
}

@group(0) @binding(0)
var base_tex: texture_2d<f32>;

@group(0) @binding(1)
var bloom_tex: texture_2d<f32>;

@group(0) @binding(2)
var linear_sampler: sampler;

@group(0) @binding(3)
var<uniform> params: BloomPassUniform;

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VertexOutput {
    let x = f32((vid << 1u) & 2u);
    let y = f32(vid & 2u);
    var out: VertexOutput;
    out.position = vec4<f32>(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, 1.0 - y);
    return out;
}

fn sample_bloom(uv: vec2<f32>) -> vec3<f32> {
    let clamped_uv = clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0));
    return max(textureSampleLevel(bloom_tex, linear_sampler, clamped_uv, 0.0).rgb, vec3<f32>(0.0));
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let bloom_dims = max(vec2<f32>(textureDimensions(bloom_tex)), vec2<f32>(1.0));
    let use_auto = params.texel_size.x <= 1e-6 || params.texel_size.y <= 1e-6;
    let texel = select(params.texel_size, 1.0 / bloom_dims, use_auto);
    let radius = max(params.radius, 0.0);
    let anamorphic = clamp(params.anamorphic_ratio, 0.0, 1.0);

    let x = texel.x * radius * (1.0 + anamorphic * 4.0);
    let y = texel.y * radius * max(1.0 - anamorphic * 0.80, 0.12);
    let uv = in.uv;

    // 9-tap 3x3 tent filter (weights sum to 1.0)
    let a = sample_bloom(uv + vec2<f32>(-x,  y));
    let b = sample_bloom(uv + vec2<f32>(0.0, y));
    let c = sample_bloom(uv + vec2<f32>( x,  y));

    let d = sample_bloom(uv + vec2<f32>(-x,  0.0));
    let e = sample_bloom(uv);
    let f = sample_bloom(uv + vec2<f32>( x,  0.0));

    let g = sample_bloom(uv + vec2<f32>(-x, -y));
    let h = sample_bloom(uv + vec2<f32>(0.0,-y));
    let i = sample_bloom(uv + vec2<f32>( x, -y));

    let tent9 = (e * 4.0 + (b + d + f + h) * 2.0 + (a + c + g + i)) * (1.0 / 16.0);
    var upsampled = tent9;

    let inv_sq_mix = clamp(params.inv_sq_falloff, 0.0, 1.0);
    if (inv_sq_mix > 1e-6) {
        // Inverse-square radial 3x3 weights: center r=0 -> 1.0, cardinals r=1 -> 0.5, corners r=sqrt(2) -> 1/3
        let inv_sq_kernel = (e + (b + d + f + h) * 0.5 + (a + c + g + i) * (1.0 / 3.0)) * (3.0 / 13.0);
        upsampled = mix(upsampled, inv_sq_kernel, inv_sq_mix);
    }

    // Smooth extended horizontal streak kernel when anamorphic_ratio > 0.05
    if (anamorphic > 0.05) {
        let s2l = sample_bloom(uv + vec2<f32>(-2.0 * x, 0.0));
        let s2r = sample_bloom(uv + vec2<f32>( 2.0 * x, 0.0));
        let s3l = sample_bloom(uv + vec2<f32>(-3.5 * x, 0.0));
        let s3r = sample_bloom(uv + vec2<f32>( 3.5 * x, 0.0));
        let streak_h = e * 0.28 + (d + f) * 0.18 + (s2l + s2r) * 0.12 + (s3l + s3r) * 0.06;
        upsampled = mix(tent9, streak_h, anamorphic * 0.75);
    }

    // Per-mip spectral chromatic bloom tint (finest -> coarsest)
    let shift = clamp(params.chromatic_shift, -1.0, 1.0);
    let chroma_outer = max(
        vec3<f32>(1.0 + shift * 0.45, 1.0 - abs(shift) * 0.10, 1.0 - shift * 0.45),
        vec3<f32>(0.05),
    );
    let mip_tint = mix(vec3<f32>(1.0), chroma_outer, clamp(params.mip_norm, 0.0, 1.0));
    upsampled = upsampled * mip_tint;

    let base = textureSampleLevel(base_tex, linear_sampler, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0);
    let base_rgb = max(base.rgb, vec3<f32>(0.0));

    if (params.is_final_composite > 0.5) {
        let tint = max(params.tint_rgb, vec3<f32>(0.0));
        let intensity = max(params.intensity, 0.0);
        // Scale by (1 + scatter) to keep visual peak bright under energy-conserving scatter
        let out_rgb = base_rgb + upsampled * tint * (intensity * (0.75 + clamp(params.scatter, 0.0, 1.0) * 0.75));
        return vec4<f32>(out_rgb, base.a);
    }

    // Exponential scatter falloff (Unity HDRP/URP energy-conserving lerp)
    var scatter = clamp(params.scatter, 0.0, 0.98);
    if (inv_sq_mix > 1e-6) {
        let mip_d = clamp(params.mip_norm, 0.0, 1.0);
        let inv_sq_mip = 1.0 / (1.0 + 3.0 * mip_d * mip_d);
        let target_scatter = clamp(scatter * (0.55 + 0.85 * inv_sq_mip), 0.0, 0.98);
        scatter = mix(scatter, target_scatter, inv_sq_mix);
    }
    return vec4<f32>(mix(base_rgb, upsampled, scatter), base.a);
}
