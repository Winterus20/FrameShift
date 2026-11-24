// FrameShift WGSL Pyramidal Optical Flow Step: fx_optical_flow_step.wgsl
// Coarse-to-fine warped Lucas-Kanade + Horn-Schunck regularization with scene-cut sensitivity guard.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct OpticalFlowStepUniform {
    // x, y = 1.0 / level_resolution; z = smoothness [0, 1]; w = sensitivity [0.001, 2.0]
    texel_and_params: vec4<f32>,
    // x = max_flow_px (at current level); y = max_step_px; z = invert_sign (0 or 1); w = has_coarse (0 or 1)
    limits_and_flags: vec4<f32>,
}

@group(0) @binding(0)
var prev_luma_tex: texture_2d<f32>;

@group(0) @binding(1)
var curr_luma_tex: texture_2d<f32>;

@group(0) @binding(2)
var coarse_flow_tex: texture_2d<f32>;

@group(0) @binding(3)
var linear_sampler: sampler;

@group(0) @binding(4)
var<uniform> u_flow: OpticalFlowStepUniform;

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VertexOutput {
    let x = f32((vid << 1u) & 2u);
    let y = f32(vid & 2u);
    var out: VertexOutput;
    out.position = vec4<f32>(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, 1.0 - y);
    return out;
}

fn sample_luma(tex: texture_2d<f32>, uv: vec2<f32>) -> f32 {
    return textureSampleLevel(tex, linear_sampler, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).r;
}

fn sample_smoothed_coarse_flow(uv: vec2<f32>, texel: vec2<f32>, smoothness: f32) -> vec2<f32> {
    if u_flow.limits_and_flags.w < 0.5 {
        return vec2<f32>(0.0);
    }
    let center = textureSampleLevel(coarse_flow_tex, linear_sampler, uv, 0.0).xy;
    var sum = vec2<f32>(0.0);
    var w_sum = 0.0;
    for (var oy: i32 = -1; oy <= 1; oy = oy + 1) {
        for (var ox: i32 = -1; ox <= 1; ox = ox + 1) {
            let s_uv = clamp(uv + vec2<f32>(f32(ox), f32(oy)) * texel, vec2<f32>(0.0), vec2<f32>(1.0));
            let s = textureSampleLevel(coarse_flow_tex, linear_sampler, s_uv, 0.0);
            let w = select(1.0, 2.0, ox == 0 && oy == 0) * max(s.z, 0.15);
            sum += s.xy * w;
            w_sum += w;
        }
    }
    let avg = sum / max(w_sum, 1e-5);
    return mix(center, avg, clamp(smoothness, 0.0, 1.0));
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let texel = u_flow.texel_and_params.xy;
    let smoothness = clamp(u_flow.texel_and_params.z, 0.0, 1.0);
    let sensitivity = max(u_flow.texel_and_params.w, 0.001);
    let max_flow_px = max(u_flow.limits_and_flags.x, 1.0);
    let max_step_px = max(u_flow.limits_and_flags.y, 0.5);

    let uv = clamp(in.uv, vec2<f32>(0.0), vec2<f32>(1.0));
    let u0 = sample_smoothed_coarse_flow(uv, texel, smoothness);

    var sum_ix2 = 0.0;
    var sum_iy2 = 0.0;
    var sum_ixy = 0.0;
    var sum_ixt = 0.0;
    var sum_iyt = 0.0;
    var sum_abs_it = 0.0;
    var w_total = 0.0;

    let dx_uv = vec2<f32>(texel.x, 0.0);
    let dy_uv = vec2<f32>(0.0, texel.y);

    for (var oy: i32 = -2; oy <= 2; oy = oy + 1) {
        for (var ox: i32 = -2; ox <= 2; ox = ox + 1) {
            let offset = vec2<f32>(f32(ox), f32(oy));
            let w = exp(-0.5 * dot(offset, offset) / 2.25);
            let c_uv = clamp(uv + offset * texel, vec2<f32>(0.0), vec2<f32>(1.0));
            let p_uv = clamp(c_uv - u0, vec2<f32>(0.0), vec2<f32>(1.0));

            let curr_c = sample_luma(curr_luma_tex, c_uv);
            let prev_c = sample_luma(prev_luma_tex, p_uv);

            let curr_dx = (sample_luma(curr_luma_tex, c_uv + dx_uv) - sample_luma(curr_luma_tex, c_uv - dx_uv)) * 0.5;
            let curr_dy = (sample_luma(curr_luma_tex, c_uv + dy_uv) - sample_luma(curr_luma_tex, c_uv - dy_uv)) * 0.5;
            let prev_dx = (sample_luma(prev_luma_tex, p_uv + dx_uv) - sample_luma(prev_luma_tex, p_uv - dx_uv)) * 0.5;
            let prev_dy = (sample_luma(prev_luma_tex, p_uv + dy_uv) - sample_luma(prev_luma_tex, p_uv - dy_uv)) * 0.5;

            let ix = 0.5 * (curr_dx + prev_dx);
            let iy = 0.5 * (curr_dy + prev_dy);
            let it = curr_c - prev_c;

            sum_ix2 += w * ix * ix;
            sum_iy2 += w * iy * iy;
            sum_ixy += w * ix * iy;
            sum_ixt += w * ix * it;
            sum_iyt += w * iy * it;
            sum_abs_it += w * abs(it);
            w_total += w;
        }
    }

    // Horn-Schunck / Tikhonov regularization prevents aperture instability on flat anime fills.
    let alpha = mix(0.002, 0.05, smoothness) * w_total;
    let a00 = sum_ix2 + alpha;
    let a11 = sum_iy2 + alpha;
    let a01 = sum_ixy;
    let det = a00 * a11 - a01 * a01;

    var du_px = vec2<f32>(0.0);
    if det > 1e-6 {
        let rhs_x = -sum_ixt;
        let rhs_y = -sum_iyt;
        du_px = vec2<f32>(
            (a11 * rhs_x - a01 * rhs_y) / det,
            (a00 * rhs_y - a01 * rhs_x) / det,
        );
    }

    let step_len = length(du_px);
    if step_len > max_step_px {
        du_px = du_px * (max_step_px / step_len);
    }

    var flow_uv = u0 + du_px * texel;
    let max_uv_x = max_flow_px * texel.x;
    let max_uv_y = max_flow_px * texel.y;
    flow_uv = clamp(flow_uv, vec2<f32>(-max_uv_x, -max_uv_y), vec2<f32>(max_uv_x, max_uv_y));

    let mean_diff = sum_abs_it / max(w_total, 1e-5);
    let confidence = smoothstep(sensitivity * 1.6, sensitivity * 0.35, mean_diff);

    return vec4<f32>(flow_uv.x, flow_uv.y, confidence, clamp(mean_diff, 0.0, 1.0));
}
