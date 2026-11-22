// FrameShift WGSL Post-FX: backdrop_scrim_blur.wgsl
// Unrolled 9-tap separable Gaussian blur for the 2D BackdropScrim node.
// The same program runs twice at half resolution: misc.xy selects the axis and
// misc.zw carries the blur radius in UV units. Weights sum to exactly 1.0, so
// flat regions keep their energy and the pass is a no-op when the radius is 0.
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
    // xy = direction, z = aspect ratio, w = blur radius (canvas-height fraction)
    misc: vec4<f32>,
}

@group(0) @binding(0)
var t_source: texture_2d<f32>;

@group(0) @binding(1)
var s_linear: sampler;

@group(0) @binding(2)
var<uniform> params: BackdropScrimUniform;

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VertexOutput {
    let x = f32((vid << 1u) & 2u);
    let y = f32(vid & 2u);
    var out: VertexOutput;
    out.position = vec4<f32>(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, 1.0 - y);
    return out;
}

fn tap_at(uv: vec2<f32>, step_uv: vec2<f32>, offset: f32) -> vec4<f32> {
    let coord = clamp(uv + step_uv * offset, vec2<f32>(0.0), vec2<f32>(1.0));
    return textureSampleLevel(t_source, s_linear, coord, 0.0);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = clamp(in.uv, vec2<f32>(0.0), vec2<f32>(1.0));
    let aspect = max(params.misc.z, 1e-5);
    let radius = max(params.misc.w, 0.0);
    // Same physical radius on both axes: x is a fraction of width, y of height.
    let radius_uv = vec2<f32>(params.misc.x * radius / aspect, params.misc.y * radius);
    // Taps span +/- 4 * step, so a quarter of the radius per tap.
    let step_uv = radius_uv * 0.25;

    var acc = tap_at(uv, step_uv, 0.0) * 0.2042;
    acc += (tap_at(uv, step_uv, 1.0) + tap_at(uv, step_uv, -1.0)) * 0.1802;
    acc += (tap_at(uv, step_uv, 2.0) + tap_at(uv, step_uv, -2.0)) * 0.1238;
    acc += (tap_at(uv, step_uv, 3.0) + tap_at(uv, step_uv, -3.0)) * 0.0663;
    acc += (tap_at(uv, step_uv, 4.0) + tap_at(uv, step_uv, -4.0)) * 0.0276;
    return acc;
}
