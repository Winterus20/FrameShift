// FrameShift WGSL Post-FX: fx_radial_shockwave.wgsl
// Adapted from Vidvox/ISF-Files (Shockwave.fs, Chroma Zoom.fs, Chromatic Aberration.fs - MIT),
// godotshaders.com/shader/positioned-shockwave (MIT), and evanw/glfx.js (zoomblur.js - MIT).
// Continuous C^inf Gaussian-derivative expanding ring refraction (zero sign(diff) seam) +
// 16-tap IGN-jittered radial zoom blur with blur-axis-integrated spectral CA in Linear Rec.709.
// Working format: Rgba16Float, Linear Rec.709, premultiplied alpha.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct RadialShockwaveUniform {
    // x = center_u, y = center_v, z = radius, w = ring_width
    wave: vec4<f32>,
    // x = refraction_amp, y = zoom_blur_strength, z = chromatic_aberration, w = aspect_ratio
    strengths: vec4<f32>,
    // x, y, z, w = reserved
    pad: vec4<f32>,
}

@group(0) @binding(0)
var t_source: texture_2d<f32>;

@group(0) @binding(1)
var s_linear: sampler;

@group(0) @binding(2)
var<uniform> params: RadialShockwaveUniform;

const ZOOM_TAPS: i32 = 16;

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
    let dims = max(vec2<f32>(textureDimensions(t_source)), vec2<f32>(1.0));
    let aspect = select(params.strengths.w, dims.x / dims.y, params.strengths.w <= 1e-4);

    let center = params.wave.xy;
    let radius_param = params.wave.z;
    let radius = max(radius_param, 0.0);
    let ring_width = max(params.wave.w, 1e-4);

    let refract_amp = params.strengths.x;
    let zoom_blur = max(params.strengths.y, 0.0);
    let ca_strength = max(params.strengths.z, 0.0);

    // Aspect-corrected expanding ring distance.
    let delta = in.uv - center;
    let delta_iso = vec2<f32>(delta.x * aspect, delta.y);
    let r = length(delta_iso);

    let half_w = max(ring_width * 0.5, 1e-4);
    let diff = r - radius;
    let u = diff / half_w;

    // A non-positive radius parks the ring on the center. Drop that mask so
    // refraction and ring chromatic aberration cannot draw a dot there.
    // Zoom blur reads zoom_blur_strength and keeps running.
    let is_active = select(1.0, 0.0, radius_param <= 0.0);
    let gauss = exp(-2.5 * u * u);
    let cutoff = 1.0 - smoothstep(0.9, 1.6, abs(u));
    let ring_mask = is_active * gauss * cutoff;

    // Smooth continuous wave derivative (-d/du exp(-2.5*u^2), normalized peak ~1.0 at |u|=1/sqrt(5))
    // eliminates the C0 discontinuity seam produced by sign(diff) at r == radius.
    let wave_deriv = is_active * (u * gauss * 3.6865) * cutoff;

    let dir_iso = select(vec2<f32>(0.0), delta_iso / max(r, 1e-5), r > 1e-5);
    let dir_uv = vec2<f32>(dir_iso.x / aspect, dir_iso.y);

    let refracted_uv = in.uv - dir_uv * (refract_amp * wave_deriv);
    let to_center = center - refracted_uv;

    // Interleaved gradient noise jitter (seek-independent) to break 16-tap radial banding.
    let jitter = fract(52.9829189 * fract(dot(in.position.xy, vec2<f32>(0.06711056, 0.00583715))));
    let ca_ring = ca_strength * 0.02 * ring_mask;
    let ca_zoom = ca_strength * (0.015 * clamp(r * 2.0, 0.0, 2.0) + 0.02 * ring_mask);

    var acc = vec4<f32>(0.0);
    var total_weight: f32 = 0.0;

    for (var i: i32 = 0; i < ZOOM_TAPS; i = i + 1) {
        let percent = (f32(i) + jitter) / f32(ZOOM_TAPS);
        let weight = 4.0 * (percent - percent * percent);

        // Spectral dispersion integrated along both the radial zoom-blur axis and ring refraction.
        let blur_g = to_center * (percent * zoom_blur);
        let blur_r = to_center * (percent * (zoom_blur + ca_zoom * 0.85))
            + dir_uv * (ca_ring * (0.5 + 0.5 * percent));
        let blur_b = to_center * (percent * max(zoom_blur - ca_zoom * 0.85, -ca_zoom * 0.5))
            - dir_uv * (ca_ring * (0.5 + 0.5 * percent));

        let uv_g = clamp(refracted_uv + blur_g, vec2<f32>(0.0), vec2<f32>(1.0));
        let uv_r = clamp(refracted_uv + blur_r, vec2<f32>(0.0), vec2<f32>(1.0));
        let uv_b = clamp(refracted_uv + blur_b, vec2<f32>(0.0), vec2<f32>(1.0));

        let sr = textureSampleLevel(t_source, s_linear, uv_r, 0.0);
        let sg = textureSampleLevel(t_source, s_linear, uv_g, 0.0);
        let sb = textureSampleLevel(t_source, s_linear, uv_b, 0.0);

        let avg_a = (sr.a + sg.a + sb.a) * (1.0 / 3.0);
        acc = acc + vec4<f32>(sr.r, sg.g, sb.b, avg_a) * weight;
        total_weight = total_weight + weight;
    }

    return acc / max(total_weight, 1e-5);
}
