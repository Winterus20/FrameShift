// FrameShift Faz 6.6 Shader: fx_dof_coc.wgsl
// Pass 1 of Z-buffer Bokeh DoF: 2-pass separable 1px-stepped H+V max CoC dilation.
// - `fs_coc_h`: reads (color_tex, depth_tex), linearizes glam::perspective_rh Depth32Float via
//   sampler-free textureLoad, treats raw_d >= 0.99999 unconditionally as void (0 CoC),
//   and performs bounded 1px horizontal foreground near-CoC dilation gated by each neighbor's own near-CoC.
// - `fs_coc_v`: reads coc_h_tex (binding 0), preserves center r/b/a, and performs bounded 1px vertical
//   foreground near-CoC dilation gated by neighbor's horizontal dilated near-CoC.
// Output format: Rgba16Float (r = signed_coc_px, g = dilated_near_coc_px, b = linear_depth, a = center_alpha).

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

// 32 bytes, 16-byte aligned.
struct DofCocUniform {
    // x = near, y = far, z = focal_dist, w = blur_strength
    depth_params: vec4<f32>,
    // x = max_radius_px, y = texel_x, z = texel_y, w = reserved
    screen_params: vec4<f32>,
}

@group(0) @binding(0)
var color_tex: texture_2d<f32>;

@group(0) @binding(1)
var depth_tex: texture_depth_2d;

@group(0) @binding(2)
var<uniform> u: DofCocUniform;

const MAX_DILATION_PX: i32 = 48;

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VertexOutput {
    let x = f32((vid << 1u) & 2u);
    let y = f32(vid & 2u);
    var out: VertexOutput;
    out.position = vec4<f32>(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, 1.0 - y);
    return out;
}

fn linearize_depth_rh(raw_d: f32, near: f32, far: f32) -> f32 {
    let d = clamp(raw_d, 0.0, 1.0);
    let denom = max(far - d * (far - near), 1e-6);
    return (near * far) / denom;
}

// Returns vec4(signed_coc_px, linear_depth, alpha, is_void).
fn eval_signed_coc(coord: vec2<i32>, max_coord: vec2<i32>) -> vec4<f32> {
    let c = clamp(coord, vec2<i32>(0, 0), max_coord);
    let raw_d = textureLoad(depth_tex, c, 0);
    let col = textureLoad(color_tex, c, 0);

    let near = max(u.depth_params.x, 1e-4);
    let far = max(u.depth_params.y, near + 0.01);
    let focal = max(u.depth_params.z, near);
    let strength = max(u.depth_params.w, 0.0);
    let max_rad = clamp(u.screen_params.x, 0.0, 48.0);
    let alpha = clamp(col.a, 0.0, 1.0);

    // Unwritten depth buffer (raw_d >= 0.99999) or transparent empty pixels (alpha <= 1e-4, e.g.
    // depth_plate-seeded background with no drawn 3D mesh) are always void (0 CoC) so empty
    // foreground areas never bleed near-CoC onto sharp in-focus 3D text/shapes.
    if (raw_d >= 0.99999 || alpha <= 1e-4) {
        return vec4<f32>(0.0, far, alpha, 1.0);
    }

    let z_view = linearize_depth_rh(raw_d, near, far);
    // Negative = foreground (closer than focal plane), positive = background (farther than focal plane).
    let raw_coc = ((z_view - focal) / max(z_view, 1e-3)) * strength * 24.0;
    let signed_coc = clamp(raw_coc, -max_rad, max_rad);
    return vec4<f32>(signed_coc, z_view, alpha, 0.0);
}

@fragment
fn fs_coc_h(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = vec2<i32>(textureDimensions(depth_tex));
    let max_coord = max(dims - vec2<i32>(1, 1), vec2<i32>(0, 0));
    let center = vec2<i32>(in.position.xy);

    let info = eval_signed_coc(center, max_coord);
    let max_rad = clamp(u.screen_params.x, 0.0, 48.0);
    let rad_i = clamp(i32(ceil(max_rad)), 0, MAX_DILATION_PX);

    var h_max_near = max(-info.x, 0.0);
    for (var dx: i32 = -MAX_DILATION_PX; dx <= MAX_DILATION_PX; dx = dx + 1) {
        if (dx == 0 || abs(dx) > rad_i) {
            continue;
        }
        let n_info = eval_signed_coc(center + vec2<i32>(dx, 0), max_coord);
        let n_near = max(-n_info.x, 0.0);
        if (n_near > 0.0 && f32(abs(dx)) <= n_near + 1.0) {
            h_max_near = max(h_max_near, n_near);
        }
    }

    return vec4<f32>(info.x, h_max_near, info.y, info.z);
}

@fragment
fn fs_coc_v(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = vec2<i32>(textureDimensions(color_tex));
    let max_coord = max(dims - vec2<i32>(1, 1), vec2<i32>(0, 0));
    let center = vec2<i32>(in.position.xy);

    let center_h = textureLoad(color_tex, clamp(center, vec2<i32>(0, 0), max_coord), 0);
    let max_rad = clamp(u.screen_params.x, 0.0, 48.0);
    let rad_i = clamp(i32(ceil(max_rad)), 0, MAX_DILATION_PX);

    var v_max_near = max(center_h.g, 0.0);
    for (var dy: i32 = -MAX_DILATION_PX; dy <= MAX_DILATION_PX; dy = dy + 1) {
        if (dy == 0 || abs(dy) > rad_i) {
            continue;
        }
        let c = clamp(center + vec2<i32>(0, dy), vec2<i32>(0, 0), max_coord);
        let n_near = max(textureLoad(color_tex, c, 0).g, 0.0);
        if (n_near > 0.0 && f32(abs(dy)) <= n_near + 1.0) {
            v_max_near = max(v_max_near, n_near);
        }
    }

    return vec4<f32>(center_h.r, v_max_near, center_h.b, center_h.a);
}
