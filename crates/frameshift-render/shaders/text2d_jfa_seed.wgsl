// FrameShift Faz 6.6 Shader: text2d_jfa_seed.wgsl
// Dual-channel sub-pixel boundary seeder for Jump Flooding Algorithm (JFA) in Rgba16Float.
// Stores relative pixel deltas: RG = exterior delta (outside -> nearest edge),
// BA = interior delta (inside -> nearest edge). Sentinel = 16384.0 (safe in f16).

struct Text2DJfaUniform {
    // x = step_px, y = max_reach_px, z = texel_x, w = texel_y
    params: vec4<f32>,
}

@group(0) @binding(0)
var t_source: texture_2d<f32>;

@group(0) @binding(1)
var s_linear: sampler;

@group(0) @binding(2)
var<uniform> u_jfa: Text2DJfaUniform;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

const SENTINEL: f32 = 16384.0;

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
    let dims = vec2<i32>(textureDimensions(t_source));
    let max_i = max(dims - vec2<i32>(1, 1), vec2<i32>(0, 0));
    let coord = clamp(vec2<i32>(floor(in.position.xy)), vec2<i32>(0, 0), max_i);

    let a = clamp(textureLoad(t_source, coord, 0).a, 0.0, 1.0);

    var ext_delta = vec2<f32>(SENTINEL, SENTINEL);
    var int_delta = vec2<f32>(SENTINEL, SENTINEL);

    if (a >= 0.999) {
        ext_delta = vec2<f32>(0.0, 0.0);
    } else if (a <= 0.001) {
        int_delta = vec2<f32>(0.0, 0.0);
    } else {
        // Y-down pixel coordinates: +x = Right, +y = Down (a_D - a_U points into higher alpha).
        let a_l = clamp(textureLoad(t_source, clamp(coord + vec2<i32>(-1, 0), vec2<i32>(0), max_i), 0).a, 0.0, 1.0);
        let a_r = clamp(textureLoad(t_source, clamp(coord + vec2<i32>(1, 0), vec2<i32>(0), max_i), 0).a, 0.0, 1.0);
        let a_u = clamp(textureLoad(t_source, clamp(coord + vec2<i32>(0, -1), vec2<i32>(0), max_i), 0).a, 0.0, 1.0);
        let a_d = clamp(textureLoad(t_source, clamp(coord + vec2<i32>(0, 1), vec2<i32>(0), max_i), 0).a, 0.0, 1.0);

        let grad = vec2<f32>(a_r - a_l, a_d - a_u);
        let grad_len = length(grad);
        if (grad_len > 1e-4) {
            let n_hat = grad / grad_len;
            if (a < 0.5) {
                ext_delta = n_hat * (0.5 - a);
                int_delta = vec2<f32>(0.0, 0.0);
            } else {
                ext_delta = vec2<f32>(0.0, 0.0);
                int_delta = -n_hat * (a - 0.5);
            }
        } else {
            // Thin hairline fallback without clear directional gradient.
            if (a >= 0.35) {
                ext_delta = vec2<f32>(0.0, 0.0);
            } else {
                int_delta = vec2<f32>(0.0, 0.0);
            }
        }
    }

    return vec4<f32>(ext_delta, int_delta);
}
