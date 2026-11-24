// FrameShift WGSL Optical Flow Motion Blur (RSMB-style): fx_optical_flow_blur.wgsl
// Gathers along estimated per-pixel optical flow with IGN jitter, confidence gating, and optional HSV debug view.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct OpticalFlowBlurUniform {
    // x, y = resolution; z = shutter_amount; w = max_blur_px
    res_and_shutter: vec4<f32>,
    // x = samples (as f32); y = visualize_flow (0.0 or 1.0); z = invert_sign (-1.0 or 1.0); w = reserved
    flags: vec4<f32>,
}

@group(0) @binding(0)
var curr_color_tex: texture_2d<f32>;

@group(0) @binding(1)
var flow_tex: texture_2d<f32>;

@group(0) @binding(2)
var linear_sampler: sampler;

@group(0) @binding(3)
var<uniform> u_blur: OpticalFlowBlurUniform;

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VertexOutput {
    let x = f32((vid << 1u) & 2u);
    let y = f32(vid & 2u);
    var out: VertexOutput;
    out.position = vec4<f32>(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, 1.0 - y);
    return out;
}

fn ign(pixel: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(pixel, vec2<f32>(0.06711056, 0.00583715))));
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> vec3<f32> {
    let k = vec3<f32>(1.0, 2.0 / 3.0, 1.0 / 3.0);
    let p = abs(fract(vec3<f32>(h) + k) * 6.0 - vec3<f32>(3.0));
    return v * mix(vec3<f32>(1.0), clamp(p - vec3<f32>(1.0), vec3<f32>(0.0), vec3<f32>(1.0)), s);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = clamp(in.uv, vec2<f32>(0.0), vec2<f32>(1.0));
    let res = max(u_blur.res_and_shutter.xy, vec2<f32>(1.0));
    let shutter_amount = max(u_blur.res_and_shutter.z, 0.0);
    let max_blur_px = max(u_blur.res_and_shutter.w, 0.0);
    let sample_count = clamp(u32(round(u_blur.flags.x)), 1u, 64u);
    let visualize = u_blur.flags.y >= 0.5;
    let flow_sign = select(1.0, -1.0, u_blur.flags.z < 0.0);

    let flow_data = textureSampleLevel(flow_tex, linear_sampler, uv, 0.0);
    let flow_uv = flow_data.xy * flow_sign;
    let confidence = clamp(flow_data.z, 0.0, 1.0);

    if visualize {
        let px_vel = flow_uv * res;
        let mag = length(px_vel);
        let angle = atan2(px_vel.y, px_vel.x);
        let hue = fract((angle + 3.14159265) / 6.2831853);
        let val = clamp(mag / max(max_blur_px * 0.5, 4.0), 0.0, 1.0) * confidence;
        let rgb = hsv_to_rgb(hue, 0.92, val);
        return vec4<f32>(rgb, 1.0);
    }

    let base_col = textureSampleLevel(curr_color_tex, linear_sampler, uv, 0.0);
    if shutter_amount <= 0.0001 || max_blur_px <= 0.01 || confidence <= 0.001 {
        return base_col;
    }

    var vel_px = flow_uv * res * (shutter_amount * 0.5) * confidence;
    let vel_len = length(vel_px);
    if vel_len < 0.2 {
        return base_col;
    }
    if vel_len > max_blur_px {
        vel_px = vel_px * (max_blur_px / vel_len);
    }
    let vel_uv = vel_px / res;
    let center_mag = length(flow_uv * res);

    var accum = base_col;
    var total_weight = 1.0;

    if sample_count > 1u {
        let jitter = (ign(uv * res) - 0.5) / f32(sample_count);
        for (var i: u32 = 0u; i < sample_count; i = i + 1u) {
            let t = ((f32(i) + 0.5) / f32(sample_count)) - 0.5 + jitter;
            let sample_uv = clamp(uv + vel_uv * t, vec2<f32>(0.0), vec2<f32>(1.0));
            let s_flow = textureSampleLevel(flow_tex, linear_sampler, sample_uv, 0.0);
            let s_mag = length(s_flow.xy * res);
            // Soft bleed-prevention weight: keep samples with compatible motion or within foreground smear.
            let motion_compat = smoothstep(0.0, max(center_mag * 0.35, 1.0), max(s_mag, center_mag * 0.5));
            let w = max(s_flow.z, 0.1) * motion_compat;
            accum += textureSampleLevel(curr_color_tex, linear_sampler, sample_uv, 0.0) * w;
            total_weight += w;
        }
    }

    return accum / max(total_weight, 1e-5);
}
