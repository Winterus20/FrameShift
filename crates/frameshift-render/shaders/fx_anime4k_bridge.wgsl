// FrameShift WGSL Shader: fx_anime4k_bridge.wgsl
// Color-space & premultiplied-alpha bridge for bloc97/Anime4K (MIT) CNN passes:
// - `fs_to_srgb_straight`: Linear Rec.709 premultiplied -> sRGB straight [0, 1]
// - `fs_to_linear_premul`: sRGB straight (1x or 2x Catmull-Rom supersampled) -> Linear Rec.709 premultiplied

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

// 16 bytes, 16-byte aligned.
struct Anime4KUniform {
    // x = texel_x, y = texel_y, z = strength, w = mode_flag (e.g. >= 1.5 for 2x->1x Catmull-Rom)
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

// Single-texture variant uses bindings (0=tex0, 1=linear_sampler, 2=uniform) in fs_to_srgb_straight?
// Note: build_program binds 1-tex at (0=tex0, 1=sampler, 2=uniform) and 2-tex at (0=tex0, 1=tex1, 2=sampler, 3=uniform).
// So fs_to_srgb_straight lives in its own entry or uses a separate shader?
// Wait: in WGSL, a single module CANNOT bind two different types/indices at @binding(1) if both entry points share global declarations!
// Wait: Naga only includes statically-used globals per entry point, EXCEPT `@binding(1)` cannot be declared twice in one WGSL module!
// Therefore, `fs_to_srgb_straight` (1-tex) and `fs_to_linear_premul` (2-tex) must either be separate WGSL modules OR `fs_to_srgb_straight` can be invoked via `record_two_tex_pass` with dummy_zero_view at tex1!
// Using `record_two_tex_pass` with `tex1 = dummy_zero_view` keeps ONE clean WGSL module and 0 extra binding complexity!

const ALPHA_EPS: f32 = 1e-5;

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VertexOutput {
    let x = f32((vid << 1u) & 2u);
    let y = f32(vid & 2u);
    var out: VertexOutput;
    out.position = vec4<f32>(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, 1.0 - y);
    return out;
}

fn linear_to_srgb_ch(c: f32) -> f32 {
    let x = clamp(c, 0.0, 1.0);
    return select(1.055 * pow(x, 1.0 / 2.4) - 0.055, x * 12.92, x <= 0.0031308);
}

fn linear_to_srgb(rgb: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        linear_to_srgb_ch(rgb.r),
        linear_to_srgb_ch(rgb.g),
        linear_to_srgb_ch(rgb.b),
    );
}

fn srgb_to_linear_ch(c: f32) -> f32 {
    let x = max(c, 0.0);
    return select(pow((x + 0.055) / 1.055, 2.4), x / 12.92, x <= 0.04045);
}

fn srgb_to_linear(rgb: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        srgb_to_linear_ch(rgb.r),
        srgb_to_linear_ch(rgb.g),
        srgb_to_linear_ch(rgb.b),
    );
}

fn catmull_rom_weight(t: f32) -> vec4<f32> {
    let t2 = t * t;
    let t3 = t2 * t;
    return vec4<f32>(
        -0.5 * t + t2 - 0.5 * t3,
        1.0 - 2.5 * t2 + 1.5 * t3,
        0.5 * t + 2.0 * t2 - 1.5 * t3,
        -0.5 * t2 + 0.5 * t3,
    );
}

fn sample_fx_catmull_rom(uv: vec2<f32>) -> vec3<f32> {
    let dims_i = max(vec2<i32>(textureDimensions(tex1)), vec2<i32>(1, 1));
    let max_c = dims_i - vec2<i32>(1, 1);
    let dims_f = vec2<f32>(dims_i);
    let fcoord = uv * dims_f - vec2<f32>(0.5);
    let base = vec2<i32>(floor(fcoord));
    let frac = fcoord - vec2<f32>(base);
    let wx = catmull_rom_weight(frac.x);
    let wy = catmull_rom_weight(frac.y);

    var acc = vec3<f32>(0.0);
    for (var j: i32 = 0; j < 4; j = j + 1) {
        let cy = clamp(base.y + (j - 1), 0, max_c.y);
        var row = vec3<f32>(0.0);
        for (var i: i32 = 0; i < 4; i = i + 1) {
            let cx = clamp(base.x + (i - 1), 0, max_c.x);
            let s = textureLoad(tex1, vec2<i32>(cx, cy), 0).rgb;
            row = row + s * wx[i];
        }
        acc = acc + row * wy[j];
    }
    return clamp(acc, vec3<f32>(0.0), vec3<f32>(1.0));
}

@fragment
fn fs_to_srgb_straight(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims_i = max(vec2<i32>(textureDimensions(tex0)), vec2<i32>(1, 1));
    let coord = clamp(vec2<i32>(in.position.xy), vec2<i32>(0), dims_i - vec2<i32>(1));
    let src = textureLoad(tex0, coord, 0);
    let a = clamp(src.a, 0.0, 1.0);
    let max_rgb = max(src.r, max(src.g, src.b));
    if (a <= ALPHA_EPS && max_rgb <= ALPHA_EPS) {
        return vec4<f32>(0.0);
    }
    let denom = select(a, 1.0, a <= ALPHA_EPS);
    let lin = max(src.rgb / denom, vec3<f32>(0.0));
    let srgb = linear_to_srgb(lin);
    return vec4<f32>(srgb, a);
}

@fragment
fn fs_to_linear_premul(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims_i = max(vec2<i32>(textureDimensions(tex0)), vec2<i32>(1, 1));
    let coord = clamp(vec2<i32>(in.position.xy), vec2<i32>(0), dims_i - vec2<i32>(1));
    let orig = textureLoad(tex0, coord, 0);
    let a = clamp(orig.a, 0.0, 1.0);
    let max_rgb = max(orig.r, max(orig.g, orig.b));
    if (a <= ALPHA_EPS && max_rgb <= ALPHA_EPS) {
        return vec4<f32>(0.0);
    }

    let is_pure_additive = a <= ALPHA_EPS;
    let denom = select(a, 1.0, is_pure_additive);
    let orig_lin = max(orig.rgb / denom, vec3<f32>(0.0));

    let uv = clamp(in.uv, vec2<f32>(0.0), vec2<f32>(1.0));
    let fx_dims = vec2<i32>(textureDimensions(tex1));
    let use_supersample = (u_a4k.params.w >= 1.5) || (fx_dims.x > dims_i.x);
    let fx_srgb_1x = clamp(textureSampleLevel(tex1, linear_sampler, uv, 0.0).rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    let fx_srgb_2x = sample_fx_catmull_rom(uv);
    let fx_srgb = select(fx_srgb_1x, fx_srgb_2x, use_supersample);
    let fx_lin = srgb_to_linear(fx_srgb);

    let strength = clamp(u_a4k.params.z, 0.0, 2.0);
    let mixed_lin = max(mix(orig_lin, fx_lin, strength), vec3<f32>(0.0));
    let out_rgb = select(mixed_lin * orig.a, mixed_lin, is_pure_additive);
    return vec4<f32>(out_rgb, orig.a);
}
