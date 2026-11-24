// FrameShift WGSL Shader: fx_anime4k_darken_hq.wgsl
// Ported from bloc97/Anime4K (MIT) Anime4K_Darken_HQ.glsl
// 5-pass DoG (Difference-of-Gaussians) dark line reinforcement in sRGB straight space.

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

const MAX_HALF_K: i32 = 8;

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

fn comp_gaussian_x(center: vec2<i32>, max_c: vec2<i32>, sigma: f32, half_k: i32) -> f32 {
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
    return g / max(gn, 1e-6);
}

fn comp_gaussian_y(tex: texture_2d<f32>, center: vec2<i32>, max_c: vec2<i32>, sigma: f32, half_k: i32) -> f32 {
    var g = 0.0;
    var gn = 0.0;
    for (var i: i32 = -MAX_HALF_K; i <= MAX_HALF_K; i = i + 1) {
        if (abs(i) > half_k) {
            continue;
        }
        let gf = gaussian_weight(f32(i), sigma);
        let c = clamp(center + vec2<i32>(0, i), vec2<i32>(0), max_c);
        g = g + textureLoad(tex, c, 0).r * gf;
        gn = gn + gf;
    }
    return g / max(gn, 1e-6);
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
fn fs_diff_x(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = max(vec2<i32>(textureDimensions(tex0)), vec2<i32>(1, 1));
    let max_c = dims - vec2<i32>(1, 1);
    let center = clamp(vec2<i32>(in.position.xy), vec2<i32>(0), max_c);
    let sigma = max(f32(dims.y) / 1080.0, 0.5);
    let half_k = clamp(i32(ceil(sigma * 2.0)), 1, MAX_HALF_K);
    return vec4<f32>(comp_gaussian_x(center, max_c, sigma, half_k), 0.0, 0.0, 1.0);
}

@fragment
fn fs_diff_y(in: VertexOutput) -> @location(0) vec4<f32> {
    // tex0 = LINELUMA, tex1 = LINEKERNEL (from diff_x)
    let dims = max(vec2<i32>(textureDimensions(tex0)), vec2<i32>(1, 1));
    let max_c = dims - vec2<i32>(1, 1);
    let center = clamp(vec2<i32>(in.position.xy), vec2<i32>(0), max_c);
    let sigma = max(f32(dims.y) / 1080.0, 0.5);
    let half_k = clamp(i32(ceil(sigma * 2.0)), 1, MAX_HALF_K);

    let luma_c = textureLoad(tex0, center, 0).r;
    var g = 0.0;
    var gn = 0.0;
    for (var i: i32 = -MAX_HALF_K; i <= MAX_HALF_K; i = i + 1) {
        if (abs(i) > half_k) {
            continue;
        }
        let gf = gaussian_weight(f32(i), sigma);
        let c = clamp(center + vec2<i32>(0, i), vec2<i32>(0), max_c);
        g = g + textureLoad(tex1, c, 0).r * gf;
        gn = gn + gf;
    }
    let gy = g / max(gn, 1e-6);
    return vec4<f32>(min(luma_c - gy, 0.0), 0.0, 0.0, 1.0);
}

@fragment
fn fs_gauss_x(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = max(vec2<i32>(textureDimensions(tex0)), vec2<i32>(1, 1));
    let max_c = dims - vec2<i32>(1, 1);
    let center = clamp(vec2<i32>(in.position.xy), vec2<i32>(0), max_c);
    let sigma = max(f32(dims.y) / 1080.0, 0.5);
    let half_k = clamp(i32(ceil(sigma * 2.0)), 1, MAX_HALF_K);
    return vec4<f32>(comp_gaussian_x(center, max_c, sigma, half_k), 0.0, 0.0, 1.0);
}

@fragment
fn fs_composite(in: VertexOutput) -> @location(0) vec4<f32> {
    // tex0 = HOOKED, tex1 = LINEKERNEL (from gauss_x)
    let dims = max(vec2<i32>(textureDimensions(tex0)), vec2<i32>(1, 1));
    let max_c = dims - vec2<i32>(1, 1);
    let center = clamp(vec2<i32>(in.position.xy), vec2<i32>(0), max_c);
    let sigma = max(f32(dims.y) / 1080.0, 0.5);
    let half_k = clamp(i32(ceil(sigma * 2.0)), 1, MAX_HALF_K);

    let hooked = textureLoad(tex0, center, 0);
    var g = 0.0;
    var gn = 0.0;
    for (var i: i32 = -MAX_HALF_K; i <= MAX_HALF_K; i = i + 1) {
        if (abs(i) > half_k) {
            continue;
        }
        let gf = gaussian_weight(f32(i), sigma);
        let c = clamp(center + vec2<i32>(0, i), vec2<i32>(0), max_c);
        g = g + textureLoad(tex1, c, 0).r * gf;
        gn = gn + gf;
    }
    let gy = g / max(gn, 1e-6);
    let darken_strength = 1.5 * clamp(u_a4k.params.z, 0.0, 2.0);
    let rgb = clamp(hooked.rgb + vec3<f32>(gy * darken_strength), vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(rgb, hooked.a);
}
