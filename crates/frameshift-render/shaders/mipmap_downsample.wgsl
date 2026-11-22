// FrameShift WGSL Mipmap Generator: mipmap_downsample.wgsl
// 4-tap bilinear box downsample from mip k to mip k+1 in Linear Rec.709 Rgba16Float premultiplied alpha.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@group(0) @binding(0)
var src_mip: texture_2d<f32>;

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
    let dims = max(vec2<f32>(textureDimensions(src_mip)), vec2<f32>(1.0));
    let half_texel = 0.5 / dims;
    let uv = clamp(in.uv, vec2<f32>(0.0), vec2<f32>(1.0));
    let s0 = textureSampleLevel(src_mip, linear_sampler, clamp(uv + vec2<f32>(-half_texel.x, -half_texel.y), vec2<f32>(0.0), vec2<f32>(1.0)), 0.0);
    let s1 = textureSampleLevel(src_mip, linear_sampler, clamp(uv + vec2<f32>( half_texel.x, -half_texel.y), vec2<f32>(0.0), vec2<f32>(1.0)), 0.0);
    let s2 = textureSampleLevel(src_mip, linear_sampler, clamp(uv + vec2<f32>(-half_texel.x,  half_texel.y), vec2<f32>(0.0), vec2<f32>(1.0)), 0.0);
    let s3 = textureSampleLevel(src_mip, linear_sampler, clamp(uv + vec2<f32>( half_texel.x,  half_texel.y), vec2<f32>(0.0), vec2<f32>(1.0)), 0.0);
    return (s0 + s1 + s2 + s3) * 0.25;
}
