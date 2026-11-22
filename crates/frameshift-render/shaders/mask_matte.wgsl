// FrameShift M1 Built-in Shader: mask_matte.wgsl
// Applies a moving Alpha, Luma (Rec.709), or Red-channel matte to a source texture
// with optional inversion, smoothstep feathering, and mask opacity.
// Preserves Rgba16Float premultiplied-alpha semantics.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct MaskMatteUniform {
    // x = matte_mode (0u = Alpha, 1u = Luma Rec.709, 2u = Red Channel)
    // y = invert (0u = normal, 1u = invert matte)
    // z = opaque_black_outside (0u = transparent outside matte, 1u = opaque alpha=1.0)
    // w = reserved
    mode_flags: vec4<u32>,
    // x = feather_low (smoothstep edge0)
    // y = feather_high (smoothstep edge1; if both 0.0 -> linear passthrough; if <= edge0 -> step)
    // z = mask_opacity [0..1] (0.0 = unmasked source, 1.0 = full matte)
    // w = reserved
    feather_opacity: vec4<f32>,
}

@group(0) @binding(0)
var t_source: texture_2d<f32>;

@group(0) @binding(1)
var t_mask: texture_2d<f32>;

@group(0) @binding(2)
var s_linear: sampler;

@group(0) @binding(3)
var<uniform> params: MaskMatteUniform;

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

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let src = textureSampleLevel(t_source, s_linear, in.uv, 0.0);
    let msk = textureSampleLevel(t_mask, s_linear, in.uv, 0.0);

    var raw_matte: f32 = clamp(msk.a, 0.0, 1.0);
    if (params.mode_flags.x == 1u) {
        raw_matte = clamp(dot(msk.rgb, LUMA_REC709), 0.0, 1.0);
    } else if (params.mode_flags.x == 2u) {
        raw_matte = clamp(msk.r, 0.0, 1.0);
    }

    var matte = select(raw_matte, 1.0 - raw_matte, params.mode_flags.y != 0u);

    let edge0 = params.feather_opacity.x;
    let edge1 = params.feather_opacity.y;
    if (edge1 > edge0) {
        matte = smoothstep(edge0, edge1, matte);
    } else if (edge0 > 0.0 || edge1 > 0.0) {
        matte = select(0.0, 1.0, matte >= edge0);
    }

    let mask_opacity = clamp(params.feather_opacity.z, 0.0, 1.0);
    let final_matte = mix(1.0, matte, mask_opacity);

    let masked = src * final_matte;
    let out_a = select(masked.a, 1.0, params.mode_flags.z != 0u);
    return vec4<f32>(masked.rgb, out_a);
}
