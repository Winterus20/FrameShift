// FrameShift Faz 6.6 Shader: fx_dof_coc_msaa.wgsl
// 4x MSAA Depth32Float Pass 1 of Z-buffer Bokeh DoF (`fs_coc_h_msaa`):
// Resolves closest opaque sample across 4 MSAA depth subsamples via textureLoad,
// computes signed Circle of Confusion (px), and performs 1px-stepped horizontal
// foreground near-CoC dilation (subsequently completed by fx_dof_coc.wgsl::fs_coc_v).
// Output format: Rgba16Float (r = signed_coc_px, g = h_dilated_near_coc_px, b = linear_depth, a = center_alpha).

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

// 32 bytes, 16-byte aligned.
struct DofCocUniform {
    depth_params: vec4<f32>,
    screen_params: vec4<f32>,
}

@group(0) @binding(0)
var color_tex: texture_2d<f32>;

@group(0) @binding(1)
var depth_tex: texture_depth_multisampled_2d;

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

fn eval_signed_coc_msaa(coord: vec2<i32>, max_coord: vec2<i32>) -> vec4<f32> {
    let c = clamp(coord, vec2<i32>(0, 0), max_coord);
    let d0 = textureLoad(depth_tex, c, 0);
    let d1 = textureLoad(depth_tex, c, 1);
    let d2 = textureLoad(depth_tex, c, 2);
    let d3 = textureLoad(depth_tex, c, 3);
    let raw_d = min(min(d0, d1), min(d2, d3));
    let col = textureLoad(color_tex, c, 0);

    let near = max(u.depth_params.x, 1e-4);
    let far = max(u.depth_params.y, near + 0.01);
    let focal = max(u.depth_params.z, near);
    let strength = max(u.depth_params.w, 0.0);
    let max_rad = clamp(u.screen_params.x, 0.0, 48.0);
    let alpha = clamp(col.a, 0.0, 1.0);

    if (raw_d >= 0.99999 || alpha <= 1e-4) {
        return vec4<f32>(0.0, far, alpha, 1.0);
    }

    let z_view = linearize_depth_rh(raw_d, near, far);
    let raw_coc = ((z_view - focal) / max(z_view, 1e-3)) * strength * 24.0;
    let signed_coc = clamp(raw_coc, -max_rad, max_rad);
    return vec4<f32>(signed_coc, z_view, alpha, 0.0);
}

@fragment
fn fs_coc_h_msaa(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = vec2<i32>(textureDimensions(color_tex));
    let max_coord = max(dims - vec2<i32>(1, 1), vec2<i32>(0, 0));
    let center = vec2<i32>(in.position.xy);

    let info = eval_signed_coc_msaa(center, max_coord);
    let max_rad = clamp(u.screen_params.x, 0.0, 48.0);
    let rad_i = clamp(i32(ceil(max_rad)), 0, MAX_DILATION_PX);

    var h_max_near = max(-info.x, 0.0);
    for (var dx: i32 = -MAX_DILATION_PX; dx <= MAX_DILATION_PX; dx = dx + 1) {
        if (dx == 0 || abs(dx) > rad_i) {
            continue;
        }
        let n_info = eval_signed_coc_msaa(center + vec2<i32>(dx, 0), max_coord);
        let n_near = max(-n_info.x, 0.0);
        if (n_near > 0.0 && f32(abs(dx)) <= n_near + 1.0) {
            h_max_near = max(h_max_near, n_near);
        }
    }

    return vec4<f32>(info.x, h_max_near, info.y, info.z);
}
