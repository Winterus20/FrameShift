// FrameShift Faz 6.2 Shader: transform3d.wgsl
// 3D perspective quad transform with Depth-of-Field Circle-of-Confusion Vogel disk gather blur.
// Working format: Rgba16Float, Linear Rec.709, premultiplied alpha.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) view_dist: f32,
}

// 112 bytes, 16-byte aligned.
struct Transform3DUniform {
    mvp: mat4x4<f32>,
    model_view_z_row: vec4<f32>,
    // x = focal_dist, y = blur_strength, z = aspect, w = texel_size_x
    dof_params: vec4<f32>,
    // x = texel_size_y, yzw = reserved
    extra: vec4<f32>,
}

@group(0) @binding(0)
var src_tex: texture_2d<f32>;

@group(0) @binding(1)
var linear_sampler: sampler;

@group(0) @binding(2)
var<uniform> u: Transform3DUniform;

const GOLDEN_ANGLE: f32 = 2.39996323;
const VOGEL_TAPS: i32 = 9;

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VertexOutput {
    // 2 CCW triangles covering [-1, 1] x [-1, 1] in local XY:
    // 0: BL (-1, -1), 1: BR (1, -1), 2: TL (-1, 1)
    // 3: TL (-1,  1), 4: BR (1, -1), 5: TR ( 1, 1)
    var quad_xy = vec2<f32>(-1.0, -1.0);
    var quad_uv = vec2<f32>(0.0, 1.0);
    switch (vid) {
        case 0u: {
            quad_xy = vec2<f32>(-1.0, -1.0);
            quad_uv = vec2<f32>(0.0, 1.0);
        }
        case 1u: {
            quad_xy = vec2<f32>(1.0, -1.0);
            quad_uv = vec2<f32>(1.0, 1.0);
        }
        case 2u, 3u: {
            quad_xy = vec2<f32>(-1.0, 1.0);
            quad_uv = vec2<f32>(0.0, 0.0);
        }
        case 4u: {
            quad_xy = vec2<f32>(1.0, -1.0);
            quad_uv = vec2<f32>(1.0, 1.0);
        }
        default: {
            quad_xy = vec2<f32>(1.0, 1.0);
            quad_uv = vec2<f32>(1.0, 0.0);
        }
    }

    let aspect = select(u.dof_params.z, 1.0, u.dof_params.z <= 0.0);
    let local_pos = vec4<f32>(quad_xy.x * aspect, quad_xy.y, 0.0, 1.0);

    var out: VertexOutput;
    out.position = u.mvp * local_pos;
    out.uv = quad_uv;
    out.view_dist = abs(dot(u.model_view_z_row, local_pos));
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let focal_dist = u.dof_params.x;
    let blur_strength = max(u.dof_params.y, 0.0);
    let coc = abs(in.view_dist - focal_dist) * blur_strength;

    let base_sample = textureSampleLevel(src_tex, linear_sampler, in.uv, 0.0);
    if (coc <= 0.001) {
        let out_a = clamp(base_sample.a, 0.0, 1.0);
        if (out_a <= 0.001) {
            discard;
        }
        return vec4<f32>(max(base_sample.rgb, vec3<f32>(0.0)), out_a);
    }

    let dims = max(vec2<f32>(textureDimensions(src_tex)), vec2<f32>(1.0));
    let auto_texel = 1.0 / dims;
    let texel = vec2<f32>(
        select(u.dof_params.w, auto_texel.x, u.dof_params.w <= 1e-6),
        select(u.extra.x, auto_texel.y, u.extra.x <= 1e-6),
    );

    let blur_radius_px = clamp(coc * 18.0, 0.0, 40.0);
    var acc = vec4<f32>(0.0);
    for (var i: i32 = 0; i < VOGEL_TAPS; i = i + 1) {
        let fi = f32(i);
        let r = sqrt((fi + 0.5) / f32(VOGEL_TAPS));
        let theta = fi * GOLDEN_ANGLE;
        let offset_uv = vec2<f32>(cos(theta), sin(theta)) * (r * blur_radius_px) * texel;
        let sample_uv = clamp(in.uv + offset_uv, vec2<f32>(0.0), vec2<f32>(1.0));
        let tap = textureSampleLevel(src_tex, linear_sampler, sample_uv, 0.0);
        acc = acc + vec4<f32>(max(tap.rgb, vec3<f32>(0.0)), clamp(tap.a, 0.0, 1.0));
    }

    let avg = acc / f32(VOGEL_TAPS);
    let out_a = clamp(avg.a, 0.0, 1.0);
    if (out_a <= 0.001) {
        discard;
    }
    return vec4<f32>(max(avg.rgb, vec3<f32>(0.0)), out_a);
}
