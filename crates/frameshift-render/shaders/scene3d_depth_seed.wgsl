// FrameShift WGSL Scene3D Depth Plate Seeder: scene3d_depth_seed.wgsl
// Seeds the hardware Depth32Float buffer from a 2D monocular depth map (1.0 = near, 0.0 = far)
// so 3D Text3D, Shape3D, particles, ribbons, and DoF interact with real scene depth.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct Scene3DDepthSeedUniform {
    // x = cam_near, y = cam_far, z = plate_near_z, w = plate_far_z
    ranges: vec4<f32>,
    // x = invert (0/1), y = depth_gamma (0.05..=8.0, <=0 -> 1.0),
    // z = disparity_mode (1.0 = inverse-Z disparity, 0.0 = linear eye-Z),
    // w = decode_mode (0.0 = R/luma, 1.0 = RG16 packed)
    flags: vec4<f32>,
}

@group(0) @binding(0)
var depth_plate_tex: texture_2d<f32>;

@group(0) @binding(1)
var linear_sampler: sampler;

@group(0) @binding(2)
var<uniform> u_seed: Scene3DDepthSeedUniform;

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VertexOutput {
    let x = f32((vid << 1u) & 2u);
    let y = f32(vid & 2u);
    var out: VertexOutput;
    out.position = vec4<f32>(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, 1.0 - y);
    return out;
}

fn decode_rg16_texel(coord: vec2<i32>) -> f32 {
    let t = textureLoad(depth_plate_tex, coord, 0);
    let hi = round(clamp(t.r, 0.0, 1.0) * 255.0);
    let lo = round(clamp(t.g, 0.0, 1.0) * 255.0);
    return clamp((hi * 256.0 + lo) / 65535.0, 0.0, 1.0);
}

fn sample_rg16_bilinear(uv: vec2<f32>) -> f32 {
    let dims_i = max(vec2<i32>(textureDimensions(depth_plate_tex)), vec2<i32>(1, 1));
    let max_c = dims_i - vec2<i32>(1, 1);
    let dims_f = vec2<f32>(dims_i);
    let fcoord = clamp(uv * dims_f - vec2<f32>(0.5), vec2<f32>(0.0), dims_f - vec2<f32>(1.0));
    let c0 = vec2<i32>(floor(fcoord));
    let c1 = min(c0 + vec2<i32>(1, 1), max_c);
    let frac = fcoord - vec2<f32>(c0);
    let d00 = decode_rg16_texel(clamp(c0, vec2<i32>(0), max_c));
    let d10 = decode_rg16_texel(clamp(vec2<i32>(c1.x, c0.y), vec2<i32>(0), max_c));
    let d01 = decode_rg16_texel(clamp(vec2<i32>(c0.x, c1.y), vec2<i32>(0), max_c));
    let d11 = decode_rg16_texel(c1);
    return mix(mix(d00, d10, frac.x), mix(d01, d11, frac.x), frac.y);
}

@fragment
fn fs_main(in: VertexOutput) -> @builtin(frag_depth) f32 {
    let uv = clamp(in.uv, vec2<f32>(0.0), vec2<f32>(1.0));
    let raw_luma = clamp(textureSampleLevel(depth_plate_tex, linear_sampler, uv, 0.0).r, 0.0, 1.0);
    let raw_rg16 = sample_rg16_bilinear(uv);
    let raw = select(raw_luma, raw_rg16, u_seed.flags.w >= 0.5);

    let d0 = select(raw, 1.0 - raw, u_seed.flags.x >= 0.5);
    let gamma = select(clamp(u_seed.flags.y, 0.05, 8.0), 1.0, u_seed.flags.y <= 0.0);
    let d = pow(clamp(d0, 0.0, 1.0), gamma);

    let cam_near = max(u_seed.ranges.x, 1e-4);
    let cam_far = max(u_seed.ranges.y, cam_near + 0.01);
    let plate_near = max(u_seed.ranges.z, cam_near);
    let plate_far = max(u_seed.ranges.w, plate_near + 0.001);

    let inv_z = mix(1.0 / plate_far, 1.0 / plate_near, d);
    let eye_z_disp = 1.0 / max(inv_z, 1e-5);
    let eye_z_lin = mix(plate_far, plate_near, d);
    let eye_z = select(eye_z_lin, eye_z_disp, u_seed.flags.z >= 0.5);
    // Perspective RH NDC Z in [0, 1] matching glam::Mat4::perspective_rh.
    let ndc_z = (cam_far * (eye_z - cam_near)) / (eye_z * (cam_far - cam_near));
    return clamp(ndc_z, 0.0, 1.0);
}
