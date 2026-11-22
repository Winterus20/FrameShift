// FrameShift Phase 2 Built-in Shader: lut3d.wgsl
// Trilinear 3D LUT color transform in Rgba16Float Linear Rec.709 premultiplied-alpha space.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct Lut3DUniform {
    // x = intensity [0..1], y = lut_size (e.g. 17.0 or 33.0), z/w = reserved
    intensity_size: vec4<f32>,
    // xyz = domain_min, w = reserved
    domain_min_pad: vec4<f32>,
    // xyz = 1.0 / (domain_max - domain_min), w = reserved
    domain_inv_span_pad: vec4<f32>,
}

@group(0) @binding(0)
var t_source: texture_2d<f32>;

@group(0) @binding(1)
var t_lut: texture_3d<f32>;

@group(0) @binding(2)
var s_linear: sampler;

@group(0) @binding(3)
var<uniform> u_lut: Lut3DUniform;

const ALPHA_EPS: f32 = 1e-5;

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VertexOutput {
    let x = f32((vid << 1u) & 2u);
    let y = f32(vid & 2u);
    var out: VertexOutput;
    out.position = vec4<f32>(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, 1.0 - y);
    return out;
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
    let rgb_in = max(src.rgb / denom, vec3<f32>(0.0));
    let domain_min = u_lut.domain_min_pad.xyz;
    let domain_inv_span = u_lut.domain_inv_span_pad.xyz;
    let norm_rgb = clamp((rgb_in - domain_min) * domain_inv_span, vec3<f32>(0.0), vec3<f32>(1.0));

    let lut_size = max(u_lut.intensity_size.y, 2.0);
    let half_texel = 0.5 / lut_size;
    let scale = (lut_size - 1.0) / lut_size;
    let uvw = norm_rgb * scale + vec3<f32>(half_texel);

    let lut_rgb = textureSampleLevel(t_lut, s_linear, uvw, 0.0).rgb;
    let intensity = clamp(u_lut.intensity_size.x, 0.0, 1.0);
    var rgb_out = mix(rgb_in, lut_rgb, intensity);
    if (is_pure_additive) {
        // Subtract any LUT black-floor lift on pure additive glow so faint halos stay clean.
        let black_floor = textureSampleLevel(t_lut, s_linear, vec3<f32>(half_texel), 0.0).rgb;
        rgb_out = max(mix(rgb_in, max(lut_rgb - black_floor, vec3<f32>(0.0)), intensity), vec3<f32>(0.0));
        return vec4<f32>(rgb_out, src.a);
    }

    return vec4<f32>(rgb_out * src.a, src.a);
}
