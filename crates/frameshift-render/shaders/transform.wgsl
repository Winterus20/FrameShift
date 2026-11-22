// FrameShift M1 Built-in Shader: transform.wgsl
// Aspect-aware 2D affine transform (translate, scale, rotation, anchor, opacity)
// with bilinear or 4x4 Catmull-Rom bicubic sampling and premultiplied-alpha border clamping.
// Working format: Rgba16Float, Linear Rec.709, premultiplied alpha.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct TransformUniform {
    // xy = translation in normalized canvas units (0,0 = no offset)
    // zw = scale (1,1 = identity; supports negative values for mirror flip)
    translate_scale: vec4<f32>,
    // xy = anchor in normalized source UV [0..1] (default 0.5, 0.5)
    // z  = rotation in radians
    // w  = opacity [0..1]
    anchor_rot_opacity: vec4<f32>,
    // x = dst_aspect (dst_width / dst_height; <= 0 defaults to 1.0)
    // y = src_aspect (src_width / src_height; <= 0 auto-derived from textureDimensions)
    // z = reserved
    // w = reserved
    aspect_info: vec4<f32>,
    // x = filter_mode (0u = Bilinear, 1u = Catmull-Rom 4x4 bicubic)
    // y, z, w = reserved
    flags: vec4<u32>,
}

@group(0) @binding(0)
var t_source: texture_2d<f32>;

@group(0) @binding(1)
var s_linear: sampler;

@group(0) @binding(2)
var<uniform> params: TransformUniform;

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

    // Clamp negative ringing from cubic weights while preserving HDR > 1.0 RGB.
    let safe_a = clamp(acc.a, 0.0, 1.0);
    let safe_rgb = max(acc.rgb, vec3<f32>(0.0));
    return vec4<f32>(safe_rgb, safe_a);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let tex_dims = vec2<f32>(textureDimensions(t_source));
    let safe_tex_dims = max(tex_dims, vec2<f32>(1.0));

    let translate = params.translate_scale.xy;
    let scale = params.translate_scale.zw;
    let anchor = params.anchor_rot_opacity.xy;
    let rotation = params.anchor_rot_opacity.z;
    let opacity = clamp(params.anchor_rot_opacity.w, 0.0, 1.0);

    let dst_aspect = select(params.aspect_info.x, 1.0, params.aspect_info.x <= 0.0);
    let auto_src_aspect = safe_tex_dims.x / safe_tex_dims.y;
    let src_aspect = select(params.aspect_info.y, auto_src_aspect, params.aspect_info.y <= 0.0);

    let scale_valid = abs(scale.x) >= 1e-6 && abs(scale.y) >= 1e-6;
    let safe_scale = vec2<f32>(
        select(1.0, scale.x, abs(scale.x) >= 1e-6),
        select(1.0, scale.y, abs(scale.y) >= 1e-6),
    );

    // Map destination UV into aspect-isotropic space around pivot (anchor + translate),
    // apply inverse rotation & scale, then return to source UV space.
    let delta_dst = in.uv - (anchor + translate);
    let delta_iso = vec2<f32>(delta_dst.x * dst_aspect, delta_dst.y);

    let c = cos(-rotation);
    let s = sin(-rotation);
    let delta_rot = vec2<f32>(
        delta_iso.x * c - delta_iso.y * s,
        delta_iso.x * s + delta_iso.y * c,
    );

    let delta_scaled = delta_rot / safe_scale;
    let src_uv = vec2<f32>(delta_scaled.x / src_aspect, delta_scaled.y) + anchor;

    let in_bounds = scale_valid
        && src_uv.x >= 0.0
        && src_uv.x <= 1.0
        && src_uv.y >= 0.0
        && src_uv.y <= 1.0;

    let clamped_uv = clamp(src_uv, vec2<f32>(0.0), vec2<f32>(1.0));
    let duv_dx = dpdx(src_uv);
    let duv_dy = dpdy(src_uv);
    let bilinear_sample = textureSampleGrad(t_source, s_linear, clamped_uv, duv_dx, duv_dy);

    var sampled = bilinear_sample;
    if (params.flags.x == 1u) {
        sampled = sample_catmull_rom(clamped_uv, safe_tex_dims);
    }

    return select(vec4<f32>(0.0), sampled * opacity, in_bounds);
}
