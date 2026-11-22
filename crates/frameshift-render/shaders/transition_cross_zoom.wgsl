// FrameShift WGSL Transition: transition_cross_zoom.wgsl
// Adapted from gl-transitions/transitions/CrossZoom.glsl (MIT License, rectalogic / gre).
// Two-input aspect-corrected radial cross-zoom blur with IGN jitter, sinusoidal dissolve,
// directional center sweep, optional RGB chromatic aberration, and midpoint impact flash.
// Working format: Rgba16Float, Linear Rec.709, premultiplied alpha.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct WhipTransitionUniform {
    // x = progress [0..1]
    // y = direction_rad (radians; sweeps zoom focus along cos/sin direction)
    // z = whip_strength (clamped >= 0.35 for cross-zoom blur intensity)
    // w = chromatic_aberration (0.0 = none, > 0 = radial RGB split)
    motion: vec4<f32>,
    // x = impact_flash (0.0 = none, > 0 = midpoint premultiplied flash boost)
    // y = aspect_ratio, z, w = reserved
    flash_pad: vec4<f32>,
}

@group(0) @binding(0)
var tex_a: texture_2d<f32>;

@group(0) @binding(1)
var tex_b: texture_2d<f32>;

@group(0) @binding(2)
var s_linear: sampler;

@group(0) @binding(3)
var<uniform> u_trans: WhipTransitionUniform;

const PI: f32 = 3.14159265359;
const CROSS_ZOOM_TAPS: i32 = 24;

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VertexOutput {
    let x = f32((vid << 1u) & 2u);
    let y = f32(vid & 2u);
    var out: VertexOutput;
    out.position = vec4<f32>(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, 1.0 - y);
    return out;
}

fn mirror_uv(uv: vec2<f32>) -> vec2<f32> {
    let t = abs(uv - 2.0 * floor(uv * 0.5));
    return select(2.0 - t, t, t <= vec2<f32>(1.0));
}

fn ign(frag_coord: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(frag_coord, vec2<f32>(0.06711056, 0.00583715))));
}

fn sample_clip(tex: texture_2d<f32>, uv: vec2<f32>, ca_offset: vec2<f32>, use_ca: bool) -> vec4<f32> {
    if (use_ca) {
        let sr = textureSampleLevel(tex, s_linear, mirror_uv(uv + ca_offset), 0.0);
        let sg = textureSampleLevel(tex, s_linear, mirror_uv(uv), 0.0);
        let sb = textureSampleLevel(tex, s_linear, mirror_uv(uv - ca_offset), 0.0);
        let avg_a = (sr.a + sg.a + sb.a) * (1.0 / 3.0);
        return vec4<f32>(sr.r, sg.g, sb.b, avg_a);
    }
    return textureSampleLevel(tex, s_linear, mirror_uv(uv), 0.0);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let p = clamp(u_trans.motion.x, 0.0, 1.0);
    let direction_rad = u_trans.motion.y;
    let strength = max(u_trans.motion.z, 0.35);
    let ca_strength = max(u_trans.motion.w, 0.0);
    let impact_flash = max(u_trans.flash_pad.x, 0.0);

    // Exact endpoint pass-through at p == 0.0 or p == 1.0.
    if (p <= 0.0) {
        return textureSampleLevel(tex_a, s_linear, clamp(in.uv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0);
    }
    if (p >= 1.0) {
        return textureSampleLevel(tex_b, s_linear, clamp(in.uv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0);
    }

    let dims = max(vec2<f32>(textureDimensions(tex_a)), vec2<f32>(1.0));
    let aspect = select(u_trans.flash_pad.y, dims.x / dims.y, u_trans.flash_pad.y <= 1e-4);

    // Sinusoidal dissolve and symmetric sinusoidal blur envelope (0 -> peak at 0.5 -> 0).
    let dissolve = 0.5 * (1.0 - cos(PI * p));
    let blur_env = 0.5 * (1.0 - cos(2.0 * PI * p));
    let zoom_strength = strength * blur_env;

    // Sweep focus center across frame along aspect-corrected direction_rad.
    let dir_uv = vec2<f32>(cos(direction_rad) / aspect, sin(direction_rad));
    let center = vec2<f32>(0.5, 0.5) + dir_uv * ((p - 0.5) * 0.5);
    let to_center = center - in.uv;

    let jitter = ign(in.position.xy);
    let use_ca = ca_strength > 1e-5;
    let ca_offset = to_center * (ca_strength * blur_env * 0.03);

    var acc = vec4<f32>(0.0);
    var total_weight: f32 = 0.0;

    for (var i: i32 = 0; i < CROSS_ZOOM_TAPS; i = i + 1) {
        let percent = (f32(i) + jitter) / f32(CROSS_ZOOM_TAPS);
        let weight = 4.0 * (percent - percent * percent);
        let sample_uv = in.uv + to_center * (percent * zoom_strength);

        let col_a = sample_clip(tex_a, sample_uv, ca_offset, use_ca);
        let col_b = sample_clip(tex_b, sample_uv, ca_offset, use_ca);
        let blended = mix(col_a, col_b, dissolve);

        acc = acc + blended * weight;
        total_weight = total_weight + weight;
    }

    var res = acc / max(total_weight, 1e-5);
    let flash = impact_flash * blur_env;
    res = vec4<f32>(res.rgb + vec3<f32>(flash) * max(res.a, 0.0), res.a);

    return res;
}
