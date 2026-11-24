// FrameShift WGSL Shader: fx_anime4k_thin_hq.wgsl
// Ported from bloc97/Anime4K (MIT) Anime4K_Thin_HQ.glsl
// 8-pass Sobel + Gaussian + forward gradient UV warp dark-line thinning in sRGB straight space.

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

const MAX_HALF_K: i32 = 12;

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VertexOutput {
    let x = f32((vid << 1u) & 2u);
    let y = f32(vid & 2u);
    var out: VertexOutput;
    out.position = vec4<f32>(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, 1.0 - y);
    return out;
}

fn gaussian_weight(x: f32, sigma: f32) -> f32 {
    let scaled = x / max(sigma, 1e-4);
    return exp(-0.5 * scaled * scaled);
}

@fragment
fn fs_luma(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = max(vec2<i32>(textureDimensions(tex0)), vec2<i32>(1, 1));
    let center = clamp(vec2<i32>(in.position.xy), vec2<i32>(0), dims - vec2<i32>(1));
    let rgb = textureLoad(tex0, center, 0).rgb;
    let y = dot(vec3<f32>(0.299, 0.587, 0.114), rgb);
    return vec4<f32>(y, 0.0, 0.0, 1.0);
}

@fragment
fn fs_sobel_x(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = max(vec2<i32>(textureDimensions(tex0)), vec2<i32>(1, 1));
    let max_c = dims - vec2<i32>(1, 1);
    let center = clamp(vec2<i32>(in.position.xy), vec2<i32>(0), max_c);
    let l = textureLoad(tex0, clamp(center + vec2<i32>(-1, 0), vec2<i32>(0), max_c), 0).r;
    let c = textureLoad(tex0, center, 0).r;
    let r = textureLoad(tex0, clamp(center + vec2<i32>(1, 0), vec2<i32>(0), max_c), 0).r;
    return vec4<f32>(-l + r, l + c + c + r, 0.0, 1.0);
}

@fragment
fn fs_sobel_y(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = max(vec2<i32>(textureDimensions(tex0)), vec2<i32>(1, 1));
    let max_c = dims - vec2<i32>(1, 1);
    let center = clamp(vec2<i32>(in.position.xy), vec2<i32>(0), max_c);

    let top = textureLoad(tex0, clamp(center + vec2<i32>(0, -1), vec2<i32>(0), max_c), 0);
    let mid = textureLoad(tex0, center, 0);
    let bot = textureLoad(tex0, clamp(center + vec2<i32>(0, 1), vec2<i32>(0), max_c), 0);

    let xgrad = (top.x + mid.x + mid.x + bot.x) / 8.0;
    let ygrad = (-top.y + bot.y) / 8.0;
    let norm = sqrt(xgrad * xgrad + ygrad * ygrad);
    let p = pow(max(norm, 0.0), 0.7);
    return vec4<f32>(p, p, p, 1.0);
}

@fragment
fn fs_gauss_x(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = max(vec2<i32>(textureDimensions(tex0)), vec2<i32>(1, 1));
    let max_c = dims - vec2<i32>(1, 1);
    let center = clamp(vec2<i32>(in.position.xy), vec2<i32>(0), max_c);
    let sigma = max(2.0 * f32(dims.y) / 1080.0, 0.5);
    let half_k = clamp(i32(ceil(sigma * 2.0)), 1, MAX_HALF_K);

    var g = 0.0;
    var gn = 0.0;
    for (var i: i32 = -MAX_HALF_K; i <= MAX_HALF_K; i = i + 1) {
        if (abs(i) > half_k) {
            continue;
        }
        let gf = gaussian_weight(f32(i), sigma);
        let c = clamp(center + vec2<i32>(i, 0), vec2<i32>(0), max_c);
        g = g + textureLoad(tex0, c, 0).r * gf;
        gn = gn + gf;
    }
    return vec4<f32>(g / max(gn, 1e-6), 0.0, 0.0, 1.0);
}

@fragment
fn fs_gauss_y(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = max(vec2<i32>(textureDimensions(tex0)), vec2<i32>(1, 1));
    let max_c = dims - vec2<i32>(1, 1);
    let center = clamp(vec2<i32>(in.position.xy), vec2<i32>(0), max_c);
    let sigma = max(2.0 * f32(dims.y) / 1080.0, 0.5);
    let half_k = clamp(i32(ceil(sigma * 2.0)), 1, MAX_HALF_K);

    var g = 0.0;
    var gn = 0.0;
    for (var i: i32 = -MAX_HALF_K; i <= MAX_HALF_K; i = i + 1) {
        if (abs(i) > half_k) {
            continue;
        }
        let gf = gaussian_weight(f32(i), sigma);
        let c = clamp(center + vec2<i32>(0, i), vec2<i32>(0), max_c);
        g = g + textureLoad(tex0, c, 0).r * gf;
        gn = gn + gf;
    }
    return vec4<f32>(g / max(gn, 1e-6), 0.0, 0.0, 1.0);
}

@fragment
fn fs_kernel_x(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = max(vec2<i32>(textureDimensions(tex0)), vec2<i32>(1, 1));
    let max_c = dims - vec2<i32>(1, 1);
    let center = clamp(vec2<i32>(in.position.xy), vec2<i32>(0), max_c);
    let l = textureLoad(tex0, clamp(center + vec2<i32>(-1, 0), vec2<i32>(0), max_c), 0).r;
    let c = textureLoad(tex0, center, 0).r;
    let r = textureLoad(tex0, clamp(center + vec2<i32>(1, 0), vec2<i32>(0), max_c), 0).r;
    return vec4<f32>(-l + r, l + c + c + r, 0.0, 1.0);
}

@fragment
fn fs_kernel_y(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = max(vec2<i32>(textureDimensions(tex0)), vec2<i32>(1, 1));
    let max_c = dims - vec2<i32>(1, 1);
    let center = clamp(vec2<i32>(in.position.xy), vec2<i32>(0), max_c);

    let top = textureLoad(tex0, clamp(center + vec2<i32>(0, -1), vec2<i32>(0), max_c), 0);
    let mid = textureLoad(tex0, center, 0);
    let bot = textureLoad(tex0, clamp(center + vec2<i32>(0, 1), vec2<i32>(0), max_c), 0);

    let xgrad = (top.x + mid.x + mid.x + bot.x) / 8.0;
    let ygrad = (-top.y + bot.y) / 8.0;
    return vec4<f32>(xgrad, ygrad, 0.0, 1.0);
}

@fragment
fn fs_warp(in: VertexOutput) -> @location(0) vec4<f32> {
    // tex0 = HOOKED, tex1 = LINESOBEL
    let dims_f = max(vec2<f32>(textureDimensions(tex0)), vec2<f32>(1.0, 1.0));
    let d = 1.0 / dims_f;
    let relstr = (dims_f.y / 1080.0) * 0.6 * clamp(u_a4k.params.z, 0.0, 2.0);

    var pos = clamp(in.uv, vec2<f32>(0.0), vec2<f32>(1.0));
    for (var i: i32 = 0; i < 2; i = i + 1) {
        let dn = textureSampleLevel(tex1, linear_sampler, pos, 0.0).xy;
        let dd = (dn / (length(dn) + 0.01)) * d * relstr * 0.5;
        pos = clamp(pos - dd, vec2<f32>(0.0), vec2<f32>(1.0));
    }

    let center_a = textureSampleLevel(tex0, linear_sampler, clamp(in.uv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0).a;
    let warped_rgb = textureSampleLevel(tex0, linear_sampler, pos, 0.0).rgb;
    return vec4<f32>(clamp(warped_rgb, vec3<f32>(0.0), vec3<f32>(1.0)), center_a);
}
