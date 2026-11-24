// FrameShift M1 Built-in Shader: mask_matte.wgsl
// Applies a moving Alpha, Luma (Rec.709), or Red-channel matte to a source texture
// with optional inversion, smoothstep feathering, and mask opacity.
// Preserves Rgba16Float premultiplied-alpha semantics.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct MaskMatteUniform {
    // x = matte_mode (0u = Alpha, 1u = Luma Rec.709, 2u = Red Channel, 3u = DepthMatte)
    // y = invert (0u = normal, 1u = invert matte)
    // z = opaque_black_outside (0u = transparent outside matte, 1u = opaque alpha=1.0)
    // w = spatial_feather_px bits (0u = off)
    mode_flags: vec4<u32>,
    // x = feather_low (or DepthMatte near_depth)
    // y = feather_high (or DepthMatte far_depth)
    // z = mask_opacity [0..1] (0.0 = unmasked source, 1.0 = full matte)
    // w = choke_px (or DepthMatte feather)
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

fn read_matte_channel(msk: vec4<f32>, mode: u32) -> f32 {
    if (mode == 1u) {
        return clamp(dot(msk.rgb, LUMA_REC709), 0.0, 1.0);
    }
    if (mode == 2u) {
        return clamp(msk.r, 0.0, 1.0);
    }
    if (mode == 3u) {
        // Depth map: use red channel when purely red-encoded, else Rec.709 luma (identical for grayscale).
        let is_red_only = msk.g <= 1e-5 && msk.b <= 1e-5;
        let d = select(dot(msk.rgb, LUMA_REC709), msk.r, is_red_only);
        return clamp(d, 0.0, 1.0);
    }
    return clamp(msk.a, 0.0, 1.0);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let src = textureSampleLevel(t_source, s_linear, in.uv, 0.0);
    let msk = textureSampleLevel(t_mask, s_linear, in.uv, 0.0);
    let mode = params.mode_flags.x;

    if (mode == 3u) {
        let d = read_matte_channel(msk, 3u);
        let near_d = params.feather_opacity.x;
        let far_d = max(params.feather_opacity.y, near_d);
        let f = max(params.feather_opacity.w, 0.0);
        let in_lo = select(
            select(0.0, 1.0, d >= near_d),
            smoothstep(near_d - f, near_d, d),
            f > 1e-6,
        );
        let in_hi = select(
            select(0.0, 1.0, d <= far_d),
            1.0 - smoothstep(far_d, far_d + f, d),
            f > 1e-6,
        );
        let raw_depth_matte = clamp(in_lo * in_hi, 0.0, 1.0);
        let final_matte = select(raw_depth_matte, 1.0 - raw_depth_matte, params.mode_flags.y != 0u);
        return src * final_matte;
    }

    let spatial_feather_px = max(bitcast<f32>(params.mode_flags.w), 0.0);
    let choke_px = params.feather_opacity.w;
    var raw_matte: f32 = read_matte_channel(msk, mode);

    if (abs(choke_px) > 1e-4 || spatial_feather_px > 1e-4) {
        let dims = max(vec2<f32>(textureDimensions(t_mask)), vec2<f32>(1.0));
        let texel = 1.0 / dims;
        let choke_r = clamp(abs(choke_px), 0.0, 8.0);
        let blur_r = clamp(spatial_feather_px, 0.0, 16.0);

        // Choke (erode when choke_px < 0, dilate when choke_px > 0)
        if (choke_r > 1e-4) {
            let erode = choke_px < 0.0;
            var extremum = raw_matte;
            for (var sy: i32 = -1; sy <= 1; sy = sy + 1) {
                for (var sx: i32 = -1; sx <= 1; sx = sx + 1) {
                    if (sx == 0 && sy == 0) {
                        continue;
                    }
                    let suv = clamp(in.uv + vec2<f32>(f32(sx), f32(sy)) * choke_r * texel, vec2<f32>(0.0), vec2<f32>(1.0));
                    let sv = read_matte_channel(textureSampleLevel(t_mask, s_linear, suv, 0.0), mode);
                    extremum = select(max(extremum, sv), min(extremum, sv), erode);
                }
            }
            raw_matte = extremum;
        }

        // Spatial Gaussian-weighted feathering
        if (blur_r > 1e-4) {
            var acc = raw_matte * 0.25;
            var wsum = 0.25;
            for (var sy: i32 = -1; sy <= 1; sy = sy + 1) {
                for (var sx: i32 = -1; sx <= 1; sx = sx + 1) {
                    if (sx == 0 && sy == 0) {
                        continue;
                    }
                    let d2 = f32(sx * sx + sy * sy);
                    let w = exp(-0.5 * d2);
                    let suv = clamp(in.uv + vec2<f32>(f32(sx), f32(sy)) * (blur_r * 0.65) * texel, vec2<f32>(0.0), vec2<f32>(1.0));
                    let sv = read_matte_channel(textureSampleLevel(t_mask, s_linear, suv, 0.0), mode);
                    acc = acc + sv * w;
                    wsum = wsum + w;
                }
            }
            raw_matte = clamp(acc / max(wsum, 1e-5), 0.0, 1.0);
        }
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
