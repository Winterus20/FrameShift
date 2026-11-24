// FrameShift WGSL Pyramidal Optical Flow: fx_optical_flow_luma.wgsl
// Extracts perceptual Rec.709 luma sqrt(Y) for balanced gradient estimation in dark anime tones.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@group(0) @binding(0)
var src_tex: texture_2d<f32>;

@group(0) @binding(1)
var linear_sampler: sampler;

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
    let c = textureSampleLevel(src_tex, linear_sampler, uv, 0.0);
    let lin_y = max(dot(max(c.rgb, vec3<f32>(0.0)), vec3<f32>(0.2126, 0.7152, 0.0722)), 0.0);
    let perc_luma = sqrt(lin_y);
    return vec4<f32>(perc_luma, perc_luma, perc_luma, clamp(c.a, 0.0, 1.0));
}
