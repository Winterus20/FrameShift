// FrameShift WGSL Post-FX: fx_aa.wgsl
// Clean-room HDR-safe FXAA Quality pass in Rgba16Float Linear Rec.709 premultiplied space.
// Uses perceptual tone-compressed luma sqrt(Y / (1 + Y)) for edge contrast detection
// so bright HDR neon edges do not over-blur, and preserves premultiplied alpha.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct FxaaUniform {
    texel_size: vec2<f32>,
    subpixel_quality: f32,
    edge_threshold: f32,
}

@group(0) @binding(0)
var src_tex: texture_2d<f32>;

@group(0) @binding(1)
var linear_sampler: sampler;

@group(0) @binding(2)
var<uniform> params: FxaaUniform;

const LUMA_REC709: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);
const EDGE_THRESHOLD_MIN: f32 = 0.0312;

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VertexOutput {
    let x = f32((vid << 1u) & 2u);
    let y = f32(vid & 2u);
    var out: VertexOutput;
    out.position = vec4<f32>(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, 1.0 - y);
    return out;
}

fn hdr_perceptual_luma(rgba: vec4<f32>) -> f32 {
    let y = max(dot(max(rgba.rgb, vec3<f32>(0.0)), LUMA_REC709), 0.0);
    // Include alpha gradient contribution for transparent/premultiplied edges
    return sqrt(y / (1.0 + y)) * 0.85 + clamp(rgba.a, 0.0, 1.0) * 0.15;
}

fn sample_clamped(uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(src_tex, linear_sampler, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = max(vec2<f32>(textureDimensions(src_tex)), vec2<f32>(1.0));
    let use_auto = params.texel_size.x <= 1e-6 || params.texel_size.y <= 1e-6;
    let texel = select(params.texel_size, 1.0 / dims, use_auto);
    let uv = clamp(in.uv, vec2<f32>(0.0), vec2<f32>(1.0));

    let col_m = sample_clamped(uv);
    let col_n = sample_clamped(uv + vec2<f32>(0.0, -texel.y));
    let col_s = sample_clamped(uv + vec2<f32>(0.0,  texel.y));
    let col_w = sample_clamped(uv + vec2<f32>(-texel.x, 0.0));
    let col_e = sample_clamped(uv + vec2<f32>( texel.x, 0.0));

    let l_m = hdr_perceptual_luma(col_m);
    let l_n = hdr_perceptual_luma(col_n);
    let l_s = hdr_perceptual_luma(col_s);
    let l_w = hdr_perceptual_luma(col_w);
    let l_e = hdr_perceptual_luma(col_e);

    let l_min = min(l_m, min(min(l_n, l_s), min(l_w, l_e)));
    let l_max = max(l_m, max(max(l_n, l_s), max(l_w, l_e)));
    let l_range = l_max - l_min;

    let edge_thresh = max(params.edge_threshold, 0.0625);
    if (l_range < max(EDGE_THRESHOLD_MIN, l_max * edge_thresh)) {
        return col_m;
    }

    let l_nw = hdr_perceptual_luma(sample_clamped(uv + vec2<f32>(-texel.x, -texel.y)));
    let l_ne = hdr_perceptual_luma(sample_clamped(uv + vec2<f32>( texel.x, -texel.y)));
    let l_sw = hdr_perceptual_luma(sample_clamped(uv + vec2<f32>(-texel.x,  texel.y)));
    let l_se = hdr_perceptual_luma(sample_clamped(uv + vec2<f32>( texel.x,  texel.y)));

    let l_ns = l_n + l_s;
    let l_we = l_w + l_e;
    let subpix_luma = (2.0 * (l_ns + l_we) + (l_nw + l_ne + l_sw + l_se)) * (1.0 / 12.0);
    let subpix_offset1 = clamp(abs(subpix_luma - l_m) / max(l_range, 1e-5), 0.0, 1.0);
    let subpix_offset2 = (-2.0 * subpix_offset1 + 3.0) * subpix_offset1 * subpix_offset1;
    let subpix_blend = subpix_offset2 * subpix_offset2 * clamp(params.subpixel_quality, 0.0, 1.0);

    let edge_h = abs(-2.0 * l_w + l_nw + l_sw)
               + abs(-2.0 * l_m + l_n + l_s) * 2.0
               + abs(-2.0 * l_e + l_ne + l_se);
    let edge_v = abs(-2.0 * l_n + l_nw + l_ne)
               + abs(-2.0 * l_m + l_w + l_e) * 2.0
               + abs(-2.0 * l_s + l_sw + l_se);
    let is_horizontal = edge_h >= edge_v;

    let l1 = select(l_w, l_n, is_horizontal);
    let l2 = select(l_e, l_s, is_horizontal);
    let grad1 = l1 - l_m;
    let grad2 = l2 - l_m;
    let is_1_steepest = abs(grad1) >= abs(grad2);
    let grad_scaled = 0.25 * max(abs(grad1), abs(grad2));

    var step_len = select(texel.x, texel.y, is_horizontal);
    var l_local_avg = 0.0;
    if (is_1_steepest) {
        step_len = -step_len;
        l_local_avg = 0.5 * (l1 + l_m);
    } else {
        l_local_avg = 0.5 * (l2 + l_m);
    }

    var base_uv = uv;
    if (is_horizontal) {
        base_uv.y = base_uv.y + step_len * 0.5;
    } else {
        base_uv.x = base_uv.x + step_len * 0.5;
    }

    let offset = select(vec2<f32>(0.0, texel.y), vec2<f32>(texel.x, 0.0), is_horizontal);
    var uv1 = base_uv - offset;
    var uv2 = base_uv + offset;
    var l_end1 = hdr_perceptual_luma(sample_clamped(uv1)) - l_local_avg;
    var l_end2 = hdr_perceptual_luma(sample_clamped(uv2)) - l_local_avg;
    var reached1 = abs(l_end1) >= grad_scaled;
    var reached2 = abs(l_end2) >= grad_scaled;

    for (var i: i32 = 0; i < 8; i = i + 1) {
        if (reached1 && reached2) {
            break;
        }
        let step_mult = 1.0 + f32(i) * 0.5;
        if (!reached1) {
            uv1 = uv1 - offset * step_mult;
            l_end1 = hdr_perceptual_luma(sample_clamped(uv1)) - l_local_avg;
            reached1 = abs(l_end1) >= grad_scaled;
        }
        if (!reached2) {
            uv2 = uv2 + offset * step_mult;
            l_end2 = hdr_perceptual_luma(sample_clamped(uv2)) - l_local_avg;
            reached2 = abs(l_end2) >= grad_scaled;
        }
    }

    let dist1 = select(uv.y - uv1.y, uv.x - uv1.x, is_horizontal);
    let dist2 = select(uv2.y - uv.y, uv2.x - uv.x, is_horizontal);
    let is_dir1 = dist1 < dist2;
    let dist_final = min(dist1, dist2);
    let edge_len = max(dist1 + dist2, 1e-5);
    let px_offset = -dist_final / edge_len + 0.5;

    let is_m_smaller = l_m < l_local_avg;
    let correct_var = (select(l_end2, l_end1, is_dir1) < 0.0) != is_m_smaller;
    let edge_blend = select(0.0, max(px_offset, 0.0), correct_var);
    let final_offset = max(edge_blend, subpix_blend);

    var final_uv = uv;
    if (is_horizontal) {
        final_uv.y = final_uv.y + final_offset * step_len;
    } else {
        final_uv.x = final_uv.x + final_offset * step_len;
    }

    return sample_clamped(final_uv);
}
