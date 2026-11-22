// FrameShift Faz 6.6 Shader: text2d_jfa_step.wgsl
// 3x3 Jump Flooding Algorithm (JFA) propagation pass over relative pixel deltas in Rgba16Float.
// Reads exact texels via textureLoad (never bilinear filtered) to preserve sub-pixel precision.

struct Text2DJfaUniform {
    // x = step_px, y = max_reach_px, z = texel_x, w = texel_y
    params: vec4<f32>,
}

@group(0) @binding(0)
var t_jfa: texture_2d<f32>;

@group(0) @binding(1)
var s_linear: sampler;

@group(0) @binding(2)
var<uniform> u_jfa: Text2DJfaUniform;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

const SENTINEL_THRESH: f32 = 8192.0;

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
    let dims = vec2<i32>(textureDimensions(t_jfa));
    let max_i = max(dims - vec2<i32>(1, 1), vec2<i32>(0, 0));
    let coord = clamp(vec2<i32>(floor(in.position.xy)), vec2<i32>(0, 0), max_i);
    let step_px = max(i32(round(u_jfa.params.x)), 1);

    let center = textureLoad(t_jfa, coord, 0);
    var best_ext = center.xy;
    var best_ext_d2 = dot(best_ext, best_ext);
    var best_int = center.zw;
    var best_int_d2 = dot(best_int, best_int);

    for (var dy: i32 = -1; dy <= 1; dy = dy + 1) {
        for (var dx: i32 = -1; dx <= 1; dx = dx + 1) {
            if (dx == 0 && dy == 0) {
                continue;
            }
            let ofs_i = vec2<i32>(dx * step_px, dy * step_px);
            let q = coord + ofs_i;
            if (q.x < 0 || q.y < 0 || q.x >= dims.x || q.y >= dims.y) {
                continue;
            }
            let jfa_tap = textureLoad(t_jfa, q, 0);
            let ofs_f = vec2<f32>(f32(ofs_i.x), f32(ofs_i.y));

            if (abs(jfa_tap.x) < SENTINEL_THRESH && abs(jfa_tap.y) < SENTINEL_THRESH) {
                let cand_ext = jfa_tap.xy + ofs_f;
                let d2 = dot(cand_ext, cand_ext);
                if (d2 < best_ext_d2) {
                    best_ext_d2 = d2;
                    best_ext = cand_ext;
                }
            }

            if (abs(jfa_tap.z) < SENTINEL_THRESH && abs(jfa_tap.w) < SENTINEL_THRESH) {
                let cand_int = jfa_tap.zw + ofs_f;
                let d2 = dot(cand_int, cand_int);
                if (d2 < best_int_d2) {
                    best_int_d2 = d2;
                    best_int = cand_int;
                }
            }
        }
    }

    return vec4<f32>(best_ext, best_int);
}
