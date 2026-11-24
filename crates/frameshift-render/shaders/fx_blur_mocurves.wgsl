// FrameShift WGSL Post-FX: fx_blur_mocurves.wgsl
// Directional, radial (z_dist), and rotational N-tap analytical motion blur (S_BlurMoCurves-style)
// with selectable UV wrap mode: 0=Clamp, 1=Tile, 2=Reflect.
// Working format: Rgba16Float, Linear Rec.709, premultiplied alpha.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

// 32 bytes, 16-byte aligned.
struct BlurMoCurvesUniform {
    shift_x: f32,
    shift_y: f32,
    z_dist: f32,
    rotate_deg: f32,
    blur_strength: f32,
    samples: u32,
    wrap_mode: u32,
    aspect: f32,
}

@group(0) @binding(0)
var src_tex: texture_2d<f32>;

@group(0) @binding(1)
var linear_sampler: sampler;

@group(0) @binding(2)
var<uniform> params: BlurMoCurvesUniform;

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VertexOutput {
    let x = f32((vid << 1u) & 2u);
    let y = f32(vid & 2u);
    var out: VertexOutput;
    out.position = vec4<f32>(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, 1.0 - y);
    return out;
}

fn wrap_uv(uv: vec2<f32>, mode: u32) -> vec2<f32> {
    if (mode == 1u) {
        return fract(uv);
    }
    if (mode == 2u) {
        return 1.0 - abs(fract(uv * 0.5) * 2.0 - 1.0);
    }
    return clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0));
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let n = clamp(params.samples, 1u, 32u);
    let strength = max(params.blur_strength, 0.0);
    let aspect = select(params.aspect, 1.0, params.aspect <= 0.0);
    let motion_mag = abs(params.shift_x) + abs(params.shift_y) + abs(params.z_dist) + abs(params.rotate_deg);

    if (n <= 1u || strength <= 1e-6 || motion_mag <= 1e-6) {
        let uv0 = wrap_uv(in.uv, params.wrap_mode);
        return textureSampleLevel(src_tex, linear_sampler, uv0, 0.0);
    }

    let inv_span = 1.0 / f32(n - 1u);
    let base_rot = radians(params.rotate_deg);
    let shift = vec2<f32>(params.shift_x, params.shift_y);
    let centered = in.uv - vec2<f32>(0.5, 0.5);
    let p_asp = vec2<f32>(centered.x * aspect, centered.y);

    var acc = vec4<f32>(0.0);
    for (var i: u32 = 0u; i < n; i = i + 1u) {
        let t = (f32(i) * inv_span - 0.5) * strength;
        let angle = base_rot * t;
        let c = cos(angle);
        let s = sin(angle);
        let p_rot = vec2<f32>(p_asp.x * c - p_asp.y * s, p_asp.x * s + p_asp.y * c);
        let zoom_scale = max(1.0 + params.z_dist * t, 0.05);
        let p_zoom = p_rot * zoom_scale;
        let uv_t = vec2<f32>(p_zoom.x / aspect, p_zoom.y) + vec2<f32>(0.5, 0.5) + shift * t;
        let folded = wrap_uv(uv_t, params.wrap_mode);
        acc = acc + textureSampleLevel(src_tex, linear_sampler, folded, 0.0);
    }

    return acc / f32(n);
}
