// FrameShift 2D Mesh & Harmonic Wave Warp Shader: grid_warp.wgsl
// Up to 8x8 control-mesh UV displacement (bilinear or clamped 4x4 Catmull-Rom)
// combined with analytic harmonic wave displacement and premultiplied-alpha border clamping.
// Working format: Rgba16Float, Linear Rec.709, premultiplied alpha.

const TAU: f32 = 6.28318530718;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct GridWarpUniform {
    // x = cols (2..=8), y = rows (2..=8), z = wave_phase, w = mix_amount [0..1].
    grid_info: vec4<f32>,
    // xy = wave_amp, zw = wave_freq.
    wave_params: vec4<f32>,
    // x = opacity [0..1], y = filter_mode (0.0 = Bilinear, 1.0 = Catmull-Rom), zw = reserved.
    render_flags: vec4<f32>,
    // Up to 8 x 8 = 64 control point offsets (dx, dy, 0.0, 0.0).
    offsets: array<vec4<f32>, 64>,
}

@group(0) @binding(0)
var t_source: texture_2d<f32>;

@group(0) @binding(1)
var s_linear: sampler;

@group(0) @binding(2)
var<uniform> u: GridWarpUniform;

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

fn grid_offset(col: i32, row: i32, cols_i: i32, rows_i: i32) -> vec2<f32> {
    let cx = clamp(col, 0, cols_i - 1);
    let cy = clamp(row, 0, rows_i - 1);
    let idx = clamp(cy * cols_i + cx, 0, 63);
    return u.offsets[idx].xy;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let cols_i = clamp(i32(round(u.grid_info.x)), 2, 8);
    let rows_i = clamp(i32(round(u.grid_info.y)), 2, 8);
    let gx = clamp(in.uv.x, 0.0, 1.0) * f32(cols_i - 1);
    let gy = clamp(in.uv.y, 0.0, 1.0) * f32(rows_i - 1);
    let ix = clamp(i32(floor(gx)), 0, cols_i - 2);
    let iy = clamp(i32(floor(gy)), 0, rows_i - 2);
    let fx = gx - f32(ix);
    let fy = gy - f32(iy);

    let use_catmull = u.render_flags.y >= 0.5;
    var disp = vec2<f32>(0.0);
    if (!use_catmull) {
        let o00 = grid_offset(ix, iy, cols_i, rows_i);
        let o10 = grid_offset(ix + 1, iy, cols_i, rows_i);
        let o01 = grid_offset(ix, iy + 1, cols_i, rows_i);
        let o11 = grid_offset(ix + 1, iy + 1, cols_i, rows_i);
        let top = mix(o00, o10, fx);
        let bot = mix(o01, o11, fx);
        disp = mix(top, bot, fy);
    } else {
        let wx = catmull_rom_weights(fx);
        let wy = catmull_rom_weights(fy);
        let weights_x = array<f32, 4>(wx.x, wx.y, wx.z, wx.w);
        let weights_y = array<f32, 4>(wy.x, wy.y, wy.z, wy.w);
        var mesh_acc = vec2<f32>(0.0);
        for (var j: i32 = 0; j < 4; j = j + 1) {
            var row_acc = vec2<f32>(0.0);
            for (var i: i32 = 0; i < 4; i = i + 1) {
                let pt = grid_offset(ix + i - 1, iy + j - 1, cols_i, rows_i);
                row_acc = row_acc + pt * weights_x[i];
            }
            mesh_acc = mesh_acc + row_acc * weights_y[j];
        }
        disp = mesh_acc;
    }

    let wave_amp = u.wave_params.xy;
    let wave_freq = u.wave_params.zw;
    let wave_phase = u.grid_info.z;
    let wave_disp = vec2<f32>(
        wave_amp.x * sin(in.uv.y * wave_freq.x * TAU + wave_phase),
        wave_amp.y * cos(in.uv.x * wave_freq.y * TAU + wave_phase),
    );

    let mix_amount = clamp(u.grid_info.w, 0.0, 1.0);
    let src_uv = in.uv - (disp + wave_disp) * mix_amount;

    let in_bounds = src_uv.x >= 0.0
        && src_uv.x <= 1.0
        && src_uv.y >= 0.0
        && src_uv.y <= 1.0;

    let clamped_uv = clamp(src_uv, vec2<f32>(0.0), vec2<f32>(1.0));
    let duv_dx = dpdx(src_uv);
    let duv_dy = dpdy(src_uv);
    let bilinear_sample = textureSampleGrad(t_source, s_linear, clamped_uv, duv_dx, duv_dy);

    var sampled = bilinear_sample;
    if (use_catmull) {
        let tex_dims = vec2<f32>(textureDimensions(t_source));
        let safe_tex_dims = max(tex_dims, vec2<f32>(1.0));
        sampled = sample_catmull_rom(clamped_uv, safe_tex_dims);
    }

    let opacity = clamp(u.render_flags.x, 0.0, 1.0);
    return select(vec4<f32>(0.0), sampled * opacity, in_bounds);
}
