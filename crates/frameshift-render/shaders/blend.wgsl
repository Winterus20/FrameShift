// FrameShift Faz 6.6 Shader: blend.wgsl
// Premultiplied-alpha two-layer compositor supporting:
// 0u=Normal, 1u=Add, 2u=Screen, 3u=Multiply (Linear Rec.709, M1-identical),
// 4u=Overlay, 5u=SoftLight, 6u=ColorDodge, 7u=HardLight (Perceptual IEC 61966-2-1 sRGB W3C),
// 8u=Lighten, 9u=Darken (HDR-safe Linear Rec.709) + pure additive glow protection.
// Working format: Rgba16Float, Linear Rec.709, premultiplied alpha.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct BlendUniform {
    // x = mode (0u..=9u), y = blend_space_override (0u = perceptual sRGB for 4..7, 1u = legacy linear), z, w = reserved
    mode_flags: vec4<u32>,
    // x = opacity [0..1] applied to top layer (tex_b), y, z, w = reserved
    opacity_pad: vec4<f32>,
}

@group(0) @binding(0)
var tex_a: texture_2d<f32>;

@group(0) @binding(1)
var tex_b: texture_2d<f32>;

@group(0) @binding(2)
var s_linear: sampler;

@group(0) @binding(3)
var<uniform> params: BlendUniform;

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

fn safe_unpremultiply(c: vec4<f32>) -> vec3<f32> {
    let max_c = max(c.r, max(c.g, c.b));
    if (c.a <= ALPHA_EPS && max_c <= ALPHA_EPS) {
        return vec3<f32>(0.0);
    }
    let denom = select(c.a, 1.0, c.a <= ALPHA_EPS);
    return max(c.rgb / denom, vec3<f32>(0.0));
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
    let x = clamp(c, 0.0, 1.0);
    return select(pow((x + 0.055) / 1.055, 2.4), x / 12.92, x <= 0.04045);
}

fn srgb_to_linear(rgb: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        srgb_to_linear_ch(rgb.r),
        srgb_to_linear_ch(rgb.g),
        srgb_to_linear_ch(rgb.b),
    );
}

fn overlay_channel(cb: f32, cs: f32) -> f32 {
    let b = clamp(cb, 0.0, 1.0);
    let s = clamp(cs, 0.0, 1.0);
    let dark = 2.0 * b * s;
    let light = 1.0 - 2.0 * (1.0 - b) * (1.0 - s);
    return select(light, dark, b <= 0.5);
}

fn soft_light_channel(cb: f32, cs: f32) -> f32 {
    let b = clamp(cb, 0.0, 1.0);
    let s = clamp(cs, 0.0, 1.0);
    if (s <= 0.5) {
        return b - (1.0 - 2.0 * s) * b * (1.0 - b);
    }
    let d_b = select(sqrt(b), ((16.0 * b - 12.0) * b + 4.0) * b, b <= 0.25);
    return b + (2.0 * s - 1.0) * (d_b - b);
}

fn color_dodge_channel(cb: f32, cs: f32) -> f32 {
    let b = clamp(cb, 0.0, 1.0);
    let s = clamp(cs, 0.0, 1.0);
    if (b <= 0.0) {
        return 0.0;
    }
    if (s >= 1.0 - 1e-5) {
        return 1.0;
    }
    return min(1.0, b / (1.0 - s));
}

fn overlay_rgb(cb: vec3<f32>, cs: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        overlay_channel(cb.r, cs.r),
        overlay_channel(cb.g, cs.g),
        overlay_channel(cb.b, cs.b),
    );
}

fn soft_light_rgb(cb: vec3<f32>, cs: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        soft_light_channel(cb.r, cs.r),
        soft_light_channel(cb.g, cs.g),
        soft_light_channel(cb.b, cs.b),
    );
}

fn color_dodge_rgb(cb: vec3<f32>, cs: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        color_dodge_channel(cb.r, cs.r),
        color_dodge_channel(cb.g, cs.g),
        color_dodge_channel(cb.b, cs.b),
    );
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let base = textureSampleLevel(tex_a, s_linear, in.uv, 0.0);
    let top_raw = textureSampleLevel(tex_b, s_linear, in.uv, 0.0);
    let opacity = clamp(params.opacity_pad.x, 0.0, 1.0);
    let top = top_raw * opacity;

    let ab = clamp(base.a, 0.0, 1.0);
    let as_top = clamp(top.a, 0.0, 1.0);
    let out_a = clamp(as_top + ab * (1.0 - as_top), 0.0, 1.0);

    let mode = params.mode_flags.x;

    // 0u = Normal (Porter-Duff Source-Over, M1-preserved)
    var out_rgb = top.rgb + base.rgb * (1.0 - as_top);

    if (mode == 1u) {
        // 1u = Add (Linear Dodge, M1-preserved)
        out_rgb = base.rgb + top.rgb;
    } else if (mode == 2u) {
        // 2u = Screen (HDR-safe clamp on the subtracted product term, M1-preserved)
        let prod = min(base.rgb, vec3<f32>(1.0)) * min(top.rgb, vec3<f32>(1.0));
        out_rgb = base.rgb + top.rgb - prod;
    } else if (mode >= 3u) {
        let max_top = max(top.r, max(top.g, top.b));
        let top_is_glow = as_top <= ALPHA_EPS && max_top > ALPHA_EPS;

        if (mode == 3u) {
            // 3u = Multiply (Linear Rec.709, M1-preserved)
            let cb = safe_unpremultiply(base);
            let cs = safe_unpremultiply(top);
            let blended = cb * cs;
            out_rgb = (1.0 - ab) * top.rgb + (1.0 - as_top) * base.rgb + (ab * as_top) * blended;
        } else if (mode == 8u) {
            // 8u = Lighten (HDR-preserving Linear Rec.709 max)
            if (top_is_glow) {
                out_rgb = max(base.rgb, top.rgb);
            } else {
                let cb = safe_unpremultiply(base);
                let cs = safe_unpremultiply(top);
                let blended = max(cb, cs);
                out_rgb = (1.0 - ab) * top.rgb + (1.0 - as_top) * base.rgb + (ab * as_top) * blended;
            }
        } else if (mode == 9u) {
            // 9u = Darken (HDR-preserving Linear Rec.709 min)
            let cb = safe_unpremultiply(base);
            let cs = safe_unpremultiply(top);
            let blended = min(cb, cs);
            out_rgb = (1.0 - ab) * top.rgb + (1.0 - as_top) * base.rgb + (ab * as_top) * blended;
        } else {
            // 4u=Overlay, 5u=SoftLight, 6u=ColorDodge, 7u=HardLight (Perceptual sRGB W3C)
            if (top_is_glow) {
                out_rgb = base.rgb + top.rgb;
            } else {
                let cb = safe_unpremultiply(base);
                let cs = safe_unpremultiply(top);
                let use_srgb = params.mode_flags.y == 0u;
                let sb = select(cb, linear_to_srgb(cb), use_srgb);
                let ss = select(cs, linear_to_srgb(cs), use_srgb);

                var res = overlay_rgb(sb, ss);
                if (mode == 5u) {
                    res = soft_light_rgb(sb, ss);
                } else if (mode == 6u) {
                    res = color_dodge_rgb(sb, ss);
                } else if (mode == 7u) {
                    res = overlay_rgb(ss, sb);
                }

                let blended = select(res, srgb_to_linear(res), use_srgb);
                out_rgb = (1.0 - ab) * top.rgb + (1.0 - as_top) * base.rgb + (ab * as_top) * blended;
            }
        }
    }

    return vec4<f32>(out_rgb, out_a);
}
