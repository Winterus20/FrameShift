// FrameShift WGSL Effect: fx_displacement_map.wgsl
// Two-input UV displacement map effect (Effect2D binding layout) with:
// - Channel modes: 0u = Rg, 1u = Luma, 2u = Polar, 3u = SignedRg
// - Edge wrap modes: 0u = Clamp, 1u = Mirror, 2u = Wrap, 3u = Transparent
// - Premultiplied-alpha map unpremultiplication + alpha weighting (prevents -neutral ghost shifts)
// - Optional RGB chromatic split + wet/dry opacity mix
// Working format: Rgba16Float, Linear Rec.709, premultiplied alpha.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

// 48 bytes, 16-byte aligned (3 x vec4).
struct DisplacementMapUniform {
    // x = channel_mode (0u=Rg, 1u=Luma, 2u=Polar, 3u=SignedRg)
    // y = edge_mode (0u=Clamp, 1u=Mirror, 2u=Wrap, 3u=Transparent)
    // z = reserved, w = reserved
    modes: vec4<u32>,
    // x = scale_x, y = scale_y, z = neutral_level, w = angle_rad
    scale_bias: vec4<f32>,
    // x = chromatic_split, y = opacity [0..1], z = aspect_ratio, w = reserved
    fx_params: vec4<f32>,
}

@group(0) @binding(0)
var t_source: texture_2d<f32>;

@group(0) @binding(1)
var t_map: texture_2d<f32>;

@group(0) @binding(2)
var s_linear: sampler;

@group(0) @binding(3)
var<uniform> params: DisplacementMapUniform;

const TAU: f32 = 6.28318530718;
const ALPHA_EPS: f32 = 1e-5;
const LUMA_REC709: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VertexOutput {
    let x = f32((vid << 1u) & 2u);
    let y = f32(vid & 2u);
    var out: VertexOutput;
    out.position = vec4<f32>(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, 1.0 - y);
    return out;
}

fn sample_source_edge(uv: vec2<f32>, edge_mode: u32) -> vec4<f32> {
    if (edge_mode == 1u) {
        let mirrored = 1.0 - abs(fract(uv * 0.5) * 2.0 - 1.0);
        return textureSampleLevel(t_source, s_linear, mirrored, 0.0);
    }
    if (edge_mode == 2u) {
        let wrapped = fract(uv);
        return textureSampleLevel(t_source, s_linear, wrapped, 0.0);
    }
    if (edge_mode == 3u) {
        let in_bounds = uv.x >= 0.0 && uv.x <= 1.0 && uv.y >= 0.0 && uv.y <= 1.0;
        let clamped = clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0));
        let sampled = textureSampleLevel(t_source, s_linear, clamped, 0.0);
        return select(vec4<f32>(0.0), sampled, in_bounds);
    }
    let clamped = clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0));
    return textureSampleLevel(t_source, s_linear, clamped, 0.0);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = clamp(in.uv, vec2<f32>(0.0), vec2<f32>(1.0));
    let orig = textureSampleLevel(t_source, s_linear, uv, 0.0);
    let opacity = clamp(params.fx_params.y, 0.0, 1.0);
    if (opacity <= 1e-6) {
        return orig;
    }

    let map_px = textureSampleLevel(t_map, s_linear, uv, 0.0);
    let map_a = clamp(map_px.a, 0.0, 1.0);
    let neutral = params.scale_bias.z;
    let straight_rgb = select(
        vec3<f32>(neutral),
        map_px.rgb / max(map_a, ALPHA_EPS),
        map_a > ALPHA_EPS,
    );

    let channel_mode = params.modes.x;
    let angle = params.scale_bias.w;
    var delta = vec2<f32>(0.0);

    if (channel_mode == 1u) {
        let luma = dot(straight_rgb, LUMA_REC709) - neutral;
        delta = vec2<f32>(cos(angle), sin(angle)) * (luma * map_a);
    } else if (channel_mode == 2u) {
        let theta = straight_rgb.r * TAU + angle;
        let rad = (straight_rgb.g - neutral) * map_a;
        delta = vec2<f32>(cos(theta), sin(theta)) * rad;
    } else if (channel_mode == 3u) {
        delta = select(vec2<f32>(0.0), map_px.rg * map_a, map_a > ALPHA_EPS);
    } else {
        delta = (straight_rgb.rg - vec2<f32>(neutral)) * map_a;
    }

    let aspect = max(params.fx_params.z, 1e-4);
    let offset = vec2<f32>(
        delta.x * params.scale_bias.x / aspect,
        delta.y * params.scale_bias.y,
    );

    let edge_mode = params.modes.y;
    let chromatic = params.fx_params.x;
    var warped: vec4<f32>;

    if (abs(chromatic) > 1e-5) {
        let sr = sample_source_edge(uv - offset * (1.0 + chromatic), edge_mode);
        let sg = sample_source_edge(uv - offset, edge_mode);
        let sb = sample_source_edge(uv - offset * (1.0 - chromatic), edge_mode);
        let out_a = clamp((sr.a + sg.a + sb.a) / 3.0, 0.0, 1.0);
        warped = vec4<f32>(sr.r, sg.g, sb.b, out_a);
    } else {
        warped = sample_source_edge(uv - offset, edge_mode);
    }

    return mix(orig, warped, opacity);
}
