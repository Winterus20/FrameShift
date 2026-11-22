// FrameShift 2D Perspective Warp Shader: corner_pin.wgsl
// Projective 4-corner inverse homography warp with bilinear or 4x4 Catmull-Rom sampling
// and premultiplied-alpha border clamping.
// Working format: Rgba16Float, Linear Rec.709, premultiplied alpha.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct CornerPinUniform {
    // Columns 0..2 of inverse homography H_inv stored as [m0j, m1j, m2j, 0.0].
    inv_cols: array<vec4<f32>, 3>,
    // x = opacity [0..1], y = filter_mode (0.0 = Bilinear, 1.0 = Catmull-Rom),
    // z = valid (1.0 or 0.0), w = reserved.
    params: vec4<f32>,
}

@group(0) @binding(0)
var t_source: texture_2d<f32>;

@group(0) @binding(1)
var s_linear: sampler;

@group(0) @binding(2)
var<uniform> u: CornerPinUniform;

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VertexOutput {
    let x = f32((vid << 1u) & 2u);
    let y = f32(vid & 2u);
    var out: VertexOutput;
    out.position = vec4<f32>(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, 1.0 - y);
    return out;
}

fn catmull_rom_weights(t: f32) -> vec4<f32> {
    let t2 = t * t;
    let t3 = t2 * t;
    return vec4<f32>(
        -0.5 * t + t2 - 0.5 * t3,
        1.0 - 2.5 * t2 + 1.5 * t3,
        0.5 * t + 2.0 * t2 - 1.5 * t3,
        -0.5 * t2 + 0.5 * t3,
    );
}

fn sample_catmull_rom(uv: vec2<f32>, tex_size: vec2<f32>) -> vec4<f32> {
    let sample_pos = uv * tex_size - vec2<f32>(0.5);
    let base_coord = vec2<i32>(floor(sample_pos));
    let frac = sample_pos - floor(sample_pos);

    let wx = catmull_rom_weights(frac.x);
    let wy = catmull_rom_weights(frac.y);
    let max_coord = vec2<i32>(tex_size) - vec2<i32>(1);

    let weights_x = array<f32, 4>(wx.x, wx.y, wx.z, wx.w);
    let weights_y = array<f32, 4>(wy.x, wy.y, wy.z, wy.w);

    var acc = vec4<f32>(0.0);
    for (var j: i32 = 0; j < 4; j = j + 1) {
        let cy = clamp(base_coord.y + j - 1, 0, max_coord.y);
        var row = vec4<f32>(0.0);
        for (var i: i32 = 0; i < 4; i = i + 1) {
            let cx = clamp(base_coord.x + i - 1, 0, max_coord.x);
            row = row + textureLoad(t_source, vec2<i32>(cx, cy), 0) * weights_x[i];
        }
        acc = acc + row * weights_y[j];
    }

    // Clamp negative ringing from cubic weights and preserve HDR > 1.0 RGB.
    let safe_a = clamp(acc.a, 0.0, 1.0);
    let safe_rgb = max(acc.rgb, vec3<f32>(0.0));
    return vec4<f32>(safe_rgb, safe_a);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let h_inv = mat3x3<f32>(
        u.inv_cols[0].xyz,
        u.inv_cols[1].xyz,
        u.inv_cols[2].xyz,
    );
    let p = h_inv * vec3<f32>(in.uv, 1.0);
    let valid = u.params.z > 0.5 && p.z > 1e-6;
    let safe_w = select(1.0, p.z, p.z > 1e-6);
    let src_uv = p.xy / safe_w;

    let in_bounds = valid
        && src_uv.x >= 0.0
        && src_uv.x <= 1.0
        && src_uv.y >= 0.0
        && src_uv.y <= 1.0;

    let clamped_uv = clamp(src_uv, vec2<f32>(0.0), vec2<f32>(1.0));
    let duv_dx = dpdx(src_uv);
    let duv_dy = dpdy(src_uv);
    let bilinear_sample = textureSampleGrad(t_source, s_linear, clamped_uv, duv_dx, duv_dy);

    var sampled = bilinear_sample;
    if (u.params.y >= 0.5) {
        let tex_dims = vec2<f32>(textureDimensions(t_source));
        let safe_tex_dims = max(tex_dims, vec2<f32>(1.0));
        sampled = sample_catmull_rom(clamped_uv, safe_tex_dims);
    }

    let opacity = clamp(u.params.x, 0.0, 1.0);
    return select(vec4<f32>(0.0), sampled * opacity, in_bounds);
}
