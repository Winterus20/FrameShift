// FrameShift WGSL Media Decode: yuv_to_linear.wgsl
// GPU NV12 (R8Unorm Y + Rg8Unorm interleaved UV + optional R8Unorm alpha) -> Linear Rec.709 Rgba16Float.
// Supports BT.709 / BT.601 / BT.2020 NCL color matrices, limited/full range,
// exact 8-bit neutral chroma center (128.0 / 255.0), MPEG-2 Left (+0.5 / width) chroma siting,
// IEC 61966-2-1 sRGB EOTF, and SourceFit::Contain transparent (0,0,0,0) letterbox padding.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

// 80 bytes, 16-byte aligned (2 x vec4<u32> + 3 x vec4<f32>).
struct YuvToLinearUniform {
    // x = matrix_mode (0u = BT.709, 1u = BT.601, 2u = BT.2020 NCL)
    // y = range_mode (0u = Limited [Y: 16..235, UV: 16..240], 1u = Full [0..255], 2u = Custom offsets_chroma.xy)
    // z = chroma_siting (0u = MPEG-2/H.264 Left +0.5/w, 1u = Center 0.0, 2u = TopLeft +0.5/w,+0.5/h, 3u = Custom)
    // w = transfer_mode (0u = Linear passthrough, 1u = IEC 61966-2-1 sRGB EOTF)
    color_modes: vec4<u32>,
    // x = use_alpha_tex (0u = opaque 1.0, 1u = sample t_alpha.r), y = src_width, z = src_height, w = reserved
    alpha_dims: vec4<u32>,
    // SourceFit::Contain letterbox rectangle: (u_min, v_min, u_max, v_max); default (0, 0, 1, 1)
    content_uv_rect: vec4<f32>,
    // xy = custom (y_offset, uv_offset) when range_mode == 2u
    // zw = custom (chroma_dx, chroma_dy) in UV units when chroma_siting == 3u
    offsets_chroma: vec4<f32>,
    // Reserved for future color/metadata extensions (keep zeroed)
    reserved: vec4<f32>,
}

@group(0) @binding(0)
var t_y: texture_2d<f32>;

@group(0) @binding(1)
var t_uv: texture_2d<f32>;

@group(0) @binding(2)
var t_alpha: texture_2d<f32>;

@group(0) @binding(3)
var s_linear: sampler;

@group(0) @binding(4)
var<uniform> params: YuvToLinearUniform;

const CHROMA_CENTER_8BIT: f32 = 128.0 / 255.0;
const LIMITED_Y_OFFSET_8BIT: f32 = 16.0 / 255.0;
const LIMITED_Y_SCALE_8BIT: f32 = 255.0 / 219.0;
const LIMITED_C_SCALE_8BIT: f32 = 255.0 / 224.0;

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VertexOutput {
    let x = f32((vid << 1u) & 2u);
    let y = f32(vid & 2u);
    var out: VertexOutput;
    out.position = vec4<f32>(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, 1.0 - y);
    return out;
}

fn srgb_to_linear_channel(c: f32) -> f32 {
    let x = max(c, 0.0);
    return select(pow((x + 0.055) / 1.055, 2.4), x / 12.92, x <= 0.04045);
}

fn ycbcr_to_rgb(y: f32, u: f32, v: f32, matrix_mode: u32) -> vec3<f32> {
    if (matrix_mode == 1u) {
        // ITU-R BT.601 (Kr = 0.299, Kb = 0.114)
        return vec3<f32>(
            y + 1.402 * v,
            y - 0.34413629 * u - 0.71413629 * v,
            y + 1.772 * u,
        );
    }
    if (matrix_mode == 2u) {
        // ITU-R BT.2020 NCL (Kr = 0.2627, Kb = 0.0593)
        return vec3<f32>(
            y + 1.4746 * v,
            y - 0.16455313 * u - 0.57135313 * v,
            y + 1.8814 * u,
        );
    }
    // 0u = ITU-R BT.709 (Kr = 0.2126, Kb = 0.0722)
    return vec3<f32>(
        y + 1.5748 * v,
        y - 0.18732427 * u - 0.46812427 * v,
        y + 1.8556 * u,
    );
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let rect = params.content_uv_rect;
    if (in.uv.x < rect.x || in.uv.x > rect.z || in.uv.y < rect.y || in.uv.y > rect.w) {
        return vec4<f32>(0.0);
    }
    let span = max(rect.zw - rect.xy, vec2<f32>(1e-5));
    let local_uv = clamp((in.uv - rect.xy) / span, vec2<f32>(0.0), vec2<f32>(1.0));

    let dims_auto = vec2<f32>(textureDimensions(t_y));
    let dims_param = vec2<f32>(f32(params.alpha_dims.y), f32(params.alpha_dims.z));
    let use_auto_dims = params.alpha_dims.y == 0u || params.alpha_dims.z == 0u;
    let src_dims = max(select(dims_param, dims_auto, use_auto_dims), vec2<f32>(2.0));

    let siting = params.color_modes.z;
    var chroma_offset = vec2<f32>(0.5 / src_dims.x, 0.0);
    if (siting == 1u) {
        chroma_offset = vec2<f32>(0.0, 0.0);
    } else if (siting == 2u) {
        chroma_offset = vec2<f32>(0.5 / src_dims.x, 0.5 / src_dims.y);
    } else if (siting == 3u) {
        chroma_offset = params.offsets_chroma.zw;
    }

    let y_sample = textureSampleLevel(t_y, s_linear, local_uv, 0.0).r;
    let uv_coord = clamp(local_uv + chroma_offset, vec2<f32>(0.0), vec2<f32>(1.0));
    let cbcr = textureSampleLevel(t_uv, s_linear, uv_coord, 0.0).rg;

    let range_mode = params.color_modes.y;
    var y_val: f32;
    var u_val: f32;
    var v_val: f32;
    if (range_mode == 1u) {
        // Full range (pc / jpeg)
        y_val = y_sample;
        u_val = cbcr.x - CHROMA_CENTER_8BIT;
        v_val = cbcr.y - CHROMA_CENTER_8BIT;
    } else if (range_mode == 2u) {
        // Custom offsets (limited-scale slopes with custom bias)
        y_val = (y_sample - params.offsets_chroma.x) * LIMITED_Y_SCALE_8BIT;
        u_val = (cbcr.x - params.offsets_chroma.y) * LIMITED_C_SCALE_8BIT;
        v_val = (cbcr.y - params.offsets_chroma.y) * LIMITED_C_SCALE_8BIT;
    } else {
        // 0u = Limited range (tv / mpeg)
        y_val = (y_sample - LIMITED_Y_OFFSET_8BIT) * LIMITED_Y_SCALE_8BIT;
        u_val = (cbcr.x - CHROMA_CENTER_8BIT) * LIMITED_C_SCALE_8BIT;
        v_val = (cbcr.y - CHROMA_CENTER_8BIT) * LIMITED_C_SCALE_8BIT;
    }

    var rgb = clamp(
        ycbcr_to_rgb(y_val, u_val, v_val, params.color_modes.x),
        vec3<f32>(0.0),
        vec3<f32>(1.0),
    );

    if (params.color_modes.w == 1u) {
        rgb = vec3<f32>(
            srgb_to_linear_channel(rgb.r),
            srgb_to_linear_channel(rgb.g),
            srgb_to_linear_channel(rgb.b),
        );
    }

    let alpha = select(
        1.0,
        clamp(textureSampleLevel(t_alpha, s_linear, local_uv, 0.0).r, 0.0, 1.0),
        params.alpha_dims.x != 0u,
    );

    return vec4<f32>(rgb * alpha, alpha);
}
