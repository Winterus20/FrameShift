// FrameShift WGSL Effect: fx_frame_echo.wgsl
// Seek-independent accumulative ping-pong frame echo / ghost trail shader (Effect2D layout).
// Supports linear RGB color decay tint, Rodrigues hue rotation around (1,1,1)/sqrt(3),
// intensity decay weighting, and premultiplied Linear Rec.709 compositing operators:
// 0u = Normal (Porter-Duff Source-Over), 1u = Add, 2u = Screen, 3u = Maximum.
// Working format: Rgba16Float, Linear Rec.709, premultiplied alpha.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

// 32 bytes, 16-byte aligned (2 x vec4<f32>).
struct FrameEchoUniform {
    // xyz = linear RGB tint multiplier (e.g. color_decay_rgb^i), w = intensity/decay weight
    tint_weight: vec4<f32>,
    // x = operator (0.0=Normal/SourceOver, 1.0=Add, 2.0=Screen, 3.0=Maximum)
    // y = is_first (0.0 = composite onto tex_accum, 1.0 = first pass / ignore tex_accum)
    // z = hue_rad (Rodrigues hue shift in radians)
    // w = reserved
    params: vec4<f32>,
}

@group(0) @binding(0)
var tex_sample: texture_2d<f32>;

@group(0) @binding(1)
var tex_accum: texture_2d<f32>;

@group(0) @binding(2)
var s_linear: sampler;

@group(0) @binding(3)
var<uniform> u_echo: FrameEchoUniform;

const INV_SQRT3: f32 = 0.57735026919;

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VertexOutput {
    let x = f32((vid << 1u) & 2u);
    let y = f32(vid & 2u);
    var out: VertexOutput;
    out.position = vec4<f32>(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, 1.0 - y);
    return out;
}

// Preserves premultiplied alpha linearity because Rodrigues rotation is homogeneous in RGB.
fn rotate_hue_linear(rgb: vec3<f32>, angle_rad: f32) -> vec3<f32> {
    if (abs(angle_rad) <= 1e-6) {
        return max(rgb, vec3<f32>(0.0));
    }
    let k = vec3<f32>(INV_SQRT3);
    let cos_a = cos(angle_rad);
    let sin_a = sin(angle_rad);
    let rotated = rgb * cos_a + cross(k, rgb) * sin_a + k * dot(k, rgb) * (1.0 - cos_a);
    return max(rotated, vec3<f32>(0.0));
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = clamp(in.uv, vec2<f32>(0.0), vec2<f32>(1.0));
    let sample_col = textureSampleLevel(tex_sample, s_linear, uv, 0.0);

    let weight = max(u_echo.tint_weight.w, 0.0);
    let tint = max(u_echo.tint_weight.xyz, vec3<f32>(0.0));
    let shifted_rgb = rotate_hue_linear(sample_col.rgb, u_echo.params.z);
    let s_tinted = vec4<f32>(
        shifted_rgb * tint * weight,
        clamp(sample_col.a * weight, 0.0, 1.0),
    );

    if (u_echo.params.y >= 0.5) {
        return s_tinted;
    }

    let accum = textureSampleLevel(tex_accum, s_linear, uv, 0.0);
    let op = u32(round(max(u_echo.params.x, 0.0)));
    let a_top = clamp(s_tinted.a, 0.0, 1.0);
    let a_bot = clamp(accum.a, 0.0, 1.0);

    if (op == 1u) {
        // Add (Linear Dodge)
        let out_a = clamp(a_top + a_bot * (1.0 - a_top), 0.0, 1.0);
        return vec4<f32>(accum.rgb + s_tinted.rgb, out_a);
    }
    if (op == 2u) {
        // Screen (HDR-safe clamp on subtracted product)
        let prod = min(accum.rgb, vec3<f32>(1.0)) * min(s_tinted.rgb, vec3<f32>(1.0));
        let out_a = clamp(a_top + a_bot * (1.0 - a_top), 0.0, 1.0);
        return vec4<f32>(accum.rgb + s_tinted.rgb - prod, out_a);
    }
    if (op == 3u) {
        // Maximum
        return vec4<f32>(max(accum.rgb, s_tinted.rgb), clamp(max(a_bot, a_top), 0.0, 1.0));
    }

    // 0u = Normal (Porter-Duff Source-Over: s_tinted over accum)
    let out_rgb = s_tinted.rgb + accum.rgb * (1.0 - a_top);
    let out_a = clamp(a_top + a_bot * (1.0 - a_top), 0.0, 1.0);
    return vec4<f32>(out_rgb, out_a);
}
