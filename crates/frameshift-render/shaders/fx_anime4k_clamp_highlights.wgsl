// FrameShift WGSL Shader: fx_anime4k_clamp_highlights.wgsl
// Ported from bloc97/Anime4K (MIT) Anime4K_Clamp_Highlights.glsl
// 3-pass separable 5x5 local max luma highlight clamping in sRGB straight space.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct Anime4KUniform {
    params: vec4<f32>,
}

@group(0) @binding(0)
var tex0: texture_2d<f32>;

@group(0) @binding(1)
var tex1: texture_2d<f32>;

@group(0) @binding(2)
var linear_sampler: sampler;

@group(0) @binding(3)
var<uniform> u_a4k: Anime4KUniform;

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VertexOutput {
    let x = f32((vid << 1u) & 2u);
    let y = f32(vid & 2u);
    var out: VertexOutput;
    out.position = vec4<f32>(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, 1.0 - y);
    return out;
}

fn get_luma(rgb: vec3<f32>) -> f32 {
    return dot(vec3<f32>(0.299, 0.587, 0.114), rgb);
}

@fragment
fn fs_stats_x(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = max(vec2<i32>(textureDimensions(tex0)), vec2<i32>(1, 1));
    let max_c = dims - vec2<i32>(1, 1);
    let center = clamp(vec2<i32>(in.position.xy), vec2<i32>(0), max_c);

    var gmax = 0.0;
    for (var i: i32 = -2; i <= 2; i = i + 1) {
        let c = clamp(center + vec2<i32>(i, 0), vec2<i32>(0), max_c);
        let g = get_luma(textureLoad(tex0, c, 0).rgb);
        gmax = max(g, gmax);
    }
    return vec4<f32>(gmax, 0.0, 0.0, 1.0);
}

@fragment
fn fs_stats_y(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = max(vec2<i32>(textureDimensions(tex0)), vec2<i32>(1, 1));
    let max_c = dims - vec2<i32>(1, 1);
    let center = clamp(vec2<i32>(in.position.xy), vec2<i32>(0), max_c);

    var gmax = 0.0;
    for (var i: i32 = -2; i <= 2; i = i + 1) {
        let c = clamp(center + vec2<i32>(0, i), vec2<i32>(0), max_c);
        let g = textureLoad(tex0, c, 0).r;
        gmax = max(g, gmax);
    }
    return vec4<f32>(gmax, 0.0, 0.0, 1.0);
}

@fragment
fn fs_clamp(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = max(vec2<i32>(textureDimensions(tex0)), vec2<i32>(1, 1));
    let max_c = dims - vec2<i32>(1, 1);
    let center = clamp(vec2<i32>(in.position.xy), vec2<i32>(0), max_c);

    let hooked = textureLoad(tex0, center, 0);
    let current_luma = get_luma(hooked.rgb);
    let max_luma = textureLoad(tex1, center, 0).r;
    let new_luma = min(current_luma, max_luma);
    let clamped_rgb = max(hooked.rgb - vec3<f32>(current_luma - new_luma), vec3<f32>(0.0));
    return vec4<f32>(clamped_rgb, hooked.a);
}
