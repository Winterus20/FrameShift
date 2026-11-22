// FrameShift WGSL Post-FX: backdrop_scrim_composite.wgsl
// Rounded-rect SDF scrim for the 2D BackdropScrim node. tex_a is the sharp
// backdrop, tex_b the blurred backdrop. The plate is built from fill_rgba as
// straight alpha, premultiplied, optionally strengthened on bright backdrops,
// and composited source-over the blurred backdrop. Pixels outside the rect
// return tex_a unchanged so the node is a bit-exact no-op there.
// Working format: Rgba16Float, Linear Rec.709, premultiplied alpha.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct BackdropScrimUniform {
    // xy = rounded-rect center (normalized canvas), zw = half width/height
    rect: vec4<f32>,
    // x = corner radius, y = blur radius, z = opacity, w = auto_contrast
    shape: vec4<f32>,
    // straight-alpha plate fill
    fill: vec4<f32>,
    // z = aspect ratio (width / height); xy/zw are blur-pass only
    misc: vec4<f32>,
}

@group(0) @binding(0)
var tex_a: texture_2d<f32>;

@group(0) @binding(1)
var tex_b: texture_2d<f32>;

@group(0) @binding(2)
var s_linear: sampler;

@group(0) @binding(3)
var<uniform> params: BackdropScrimUniform;

const LUMA_REC709: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);
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

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let base = textureSampleLevel(tex_a, s_linear, in.uv, 0.0);
    let uv = clamp(in.uv, vec2<f32>(0.0), vec2<f32>(1.0));

    // Aspect-corrected space: x is measured in canvas-height units.
    let aspect = max(params.misc.z, 1e-5);
    let center = params.rect.xy;
    let half_extent = max(params.rect.zw, vec2<f32>(0.0)) * vec2<f32>(aspect, 1.0);
    let radius = min(max(params.shape.x, 0.0), min(half_extent.x, half_extent.y));

    let p = (uv - center) * vec2<f32>(aspect, 1.0);
    let q = abs(p) - (half_extent - vec2<f32>(radius));
    let dist = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius;

    // One-pixel analytic antialiasing; resolution independent by construction.
    let aa = max(fwidth(dist), 1e-5);
    let coverage = 1.0 - smoothstep(-aa, aa, dist);
    if (coverage <= 0.0) {
        return base;
    }

    let blurred = textureSampleLevel(tex_b, s_linear, uv, 0.0);
    let backdrop_rgb = blurred.rgb / max(blurred.a, ALPHA_EPS);
    let luma = clamp(dot(max(backdrop_rgb, vec3<f32>(0.0)), LUMA_REC709), 0.0, 1.0);
    // Bright backdrop -> stronger, slightly darker plate (single polarity, v1).
    let strengthen = params.shape.w * smoothstep(0.2, 0.9, luma);

    let fill = params.fill;
    let plate_alpha = clamp(fill.a * params.shape.z * (1.0 + strengthen), 0.0, 1.0);
    let plate_rgb = max(fill.rgb, vec3<f32>(0.0)) * (1.0 - 0.35 * strengthen);
    let plate = vec4<f32>(plate_rgb * plate_alpha, plate_alpha) * coverage;

    let out_a = clamp(plate.a + blurred.a * (1.0 - plate.a), 0.0, 1.0);
    let out_rgb = plate.rgb + blurred.rgb * (1.0 - plate.a);
    return vec4<f32>(out_rgb, out_a);
}
