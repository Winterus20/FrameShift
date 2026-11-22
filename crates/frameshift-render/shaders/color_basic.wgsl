// FrameShift M1 Built-in Shader: color_basic.wgsl
// sRGB <-> Linear Rec.709 color space conversion, exposure, contrast, saturation,
// and highlight glow/flash boost while preserving Rgba16Float premultiplied alpha.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct ColorBasicUniform {
    // x = exposure (EV stops; 0.0 = identity)
    // y = contrast (1.0 = identity; pivots around 0.18 linear mid-grey)
    // z = saturation (1.0 = identity; 0.0 = monochrome Rec.709 luma)
    // w = flash_amount (0.0 = none; > 0 boosts highlights and adds impact flash)
    grade: vec4<f32>,
    // x = highlight_threshold (default ~0.35 in linear space)
    // y = flash_softness (knee width above threshold, default ~0.65)
    // z, w = reserved
    highlight_pad: vec4<f32>,
    // x = space_mode:
    //     0u = Linear -> Linear (no transfer conversion)
    //     1u = sRGB   -> Linear (decode input to Linear Rec.709)
    //     2u = Linear -> sRGB   (encode output to sRGB)
    //     3u = sRGB   -> Linear -> sRGB (round-trip grading in linear space)
    // y, z, w = reserved
    space_flags: vec4<u32>,
}

@group(0) @binding(0)
var t_source: texture_2d<f32>;

@group(0) @binding(1)
var s_linear: sampler;

@group(0) @binding(2)
var<uniform> params: ColorBasicUniform;

const ALPHA_EPS: f32 = 1e-5;
const LUMA_REC709: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);
const LINEAR_MID_GREY: f32 = 0.18;

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

fn srgb_to_linear(rgb: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        srgb_to_linear_channel(rgb.r),
        srgb_to_linear_channel(rgb.g),
        srgb_to_linear_channel(rgb.b),
    );
}

fn linear_to_srgb_channel(c: f32) -> f32 {
    let x = max(c, 0.0);
    return select(1.055 * pow(x, 1.0 / 2.4) - 0.055, x * 12.92, x <= 0.0031308);
}

fn linear_to_srgb(rgb: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        linear_to_srgb_channel(rgb.r),
        linear_to_srgb_channel(rgb.g),
        linear_to_srgb_channel(rgb.b),
    );
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let src = textureSampleLevel(t_source, s_linear, in.uv, 0.0);
    let a = clamp(src.a, 0.0, 1.0);
    let max_rgb = max(src.r, max(src.g, src.b));
    if (a <= ALPHA_EPS && max_rgb <= ALPHA_EPS) {
        return vec4<f32>(0.0);
    }

    let is_pure_additive = a <= ALPHA_EPS;
    let denom = select(a, 1.0, is_pure_additive);
    var rgb = max(src.rgb / denom, vec3<f32>(0.0));
    let space_mode = params.space_flags.x;

    // Decode sRGB input to Linear Rec.709 when requested.
    if (space_mode == 1u || space_mode == 3u) {
        rgb = srgb_to_linear(rgb);
    }

    // Exposure (EV stops).
    let exposure = params.grade.x;
    rgb = rgb * exp2(exposure);

    // Contrast around linear mid-grey (0.18) for occluded pixels; scale-only on pure additive glow.
    let contrast = params.grade.y;
    if (!is_pure_additive) {
        rgb = max((rgb - vec3<f32>(LINEAR_MID_GREY)) * contrast + vec3<f32>(LINEAR_MID_GREY), vec3<f32>(0.0));
    }

    // Saturation using Linear Rec.709 luminance.
    let saturation = params.grade.z;
    let luma = dot(rgb, LUMA_REC709);
    rgb = max(mix(vec3<f32>(luma), rgb, saturation), vec3<f32>(0.0));

    // Highlight glow / impact flash boost (0.0 = no-op).
    let flash_amount = max(params.grade.w, 0.0);
    if (flash_amount > 0.0) {
        let thresh = max(params.highlight_pad.x, 0.0);
        let knee = max(params.highlight_pad.y, 1e-4);
        let highlight_mask = smoothstep(thresh, thresh + knee, luma);
        let add_bias = select(flash_amount * 0.35 * (0.25 + 0.75 * highlight_mask), 0.0, is_pure_additive);
        rgb = rgb * (1.0 + flash_amount * (0.5 + 1.5 * highlight_mask)) + vec3<f32>(add_bias);
    }

    // Encode Linear Rec.709 output to sRGB when requested.
    if (space_mode == 2u || space_mode == 3u) {
        rgb = linear_to_srgb(rgb);
    }

    let out_rgb = select(rgb * src.a, rgb, is_pure_additive);
    return vec4<f32>(out_rgb, src.a);
}
