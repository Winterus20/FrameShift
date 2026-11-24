// FrameShift WGSL Effect: fx_shape_mask_feather.wgsl
// 2-pass separable Gaussian blur feather + invert + opacity pass for ShapeMask (Effect1D layout).
// Pass 1 (H): direction = (1, 0), apply_post = 0.0
// Pass 2 (V): direction = (0, 1), apply_post = 1.0 (applies invert + opacity)
// Outputs premultiplied white Rgba16Float mask (m, m, m, m) compatible with Alpha/Luma/Red mattes.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

// 32 bytes, 16-byte aligned (2 x vec4<f32>).
struct ShapeMaskFeatherUniform {
    // xy = blur direction (e.g. (1,0) or (0,1)), z = radius_px (>= 0.0), w = apply_post (0.0 | 1.0)
    dir_radius: vec4<f32>,
    // x = invert (0.0 | 1.0), y = opacity [0..1], z = texel_x (0 = auto), w = texel_y (0 = auto)
    post_texel: vec4<f32>,
}

@group(0) @binding(0)
var t_source: texture_2d<f32>;

@group(0) @binding(1)
var s_linear: sampler;

@group(0) @binding(2)
var<uniform> params: ShapeMaskFeatherUniform;

const MAX_HALF_TAPS: i32 = 48;

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
    let uv = clamp(in.uv, vec2<f32>(0.0), vec2<f32>(1.0));
    let dims = max(vec2<f32>(textureDimensions(t_source)), vec2<f32>(1.0));
    let use_auto = params.post_texel.z <= 1e-6 || params.post_texel.w <= 1e-6;
    let texel = select(params.post_texel.zw, 1.0 / dims, use_auto);

    let radius_px = max(params.dir_radius.z, 0.0);
    let center_a = clamp(textureSampleLevel(t_source, s_linear, uv, 0.0).a, 0.0, 1.0);
    var matte = center_a;

    if (radius_px > 1e-4) {
        let raw_dir = params.dir_radius.xy;
        let dir_len = length(raw_dir);
        let dir = select(vec2<f32>(1.0, 0.0), raw_dir / max(dir_len, 1e-6), dir_len > 1e-6);

        let steps = clamp(i32(ceil(radius_px)), 1, MAX_HALF_TAPS);
        let step_px = radius_px / f32(steps);
        let sigma = max(radius_px * 0.45, 0.35);
        let inv_two_sigma2 = 0.5 / (sigma * sigma);

        var acc = center_a;
        var wsum = 1.0;

        for (var i: i32 = 1; i <= MAX_HALF_TAPS; i = i + 1) {
            if (i > steps) {
                break;
            }
            let dist_px = f32(i) * step_px;
            let w = exp(-(dist_px * dist_px) * inv_two_sigma2);
            let duv = dir * (dist_px * texel);
            let sp = clamp(
                textureSampleLevel(t_source, s_linear, clamp(uv + duv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).a,
                0.0,
                1.0,
            );
            let sn = clamp(
                textureSampleLevel(t_source, s_linear, clamp(uv - duv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).a,
                0.0,
                1.0,
            );
            acc = acc + (sp + sn) * w;
            wsum = wsum + 2.0 * w;
        }

        matte = clamp(acc / max(wsum, 1e-5), 0.0, 1.0);
    }

    if (params.dir_radius.w >= 0.5) {
        if (params.post_texel.x >= 0.5) {
            matte = 1.0 - matte;
        }
        matte = matte * clamp(params.post_texel.y, 0.0, 1.0);
    }

    return vec4<f32>(matte, matte, matte, matte);
}
