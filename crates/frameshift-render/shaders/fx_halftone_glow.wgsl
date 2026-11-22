// FrameShift WGSL Post-FX: fx_halftone_glow.wgsl
// Adapted from stackgl/glsl-halftone & glsl-aastep (MIT), Vidvox/ISF-Files/CMYK Halftone.fs (MIT).
// Rotated 2D SDF screentone (Monochrome, 4-plate CMYK Rosette, or ColorDot) +
// 28-tap multi-ring Vogel-disk + IGN jittered HDR soft-knee glow in Linear Rec.709 Rgba16Float.
// Working format: Rgba16Float, Linear Rec.709, premultiplied alpha.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct HalftoneGlowUniform {
    // x = dot_scale, y = angle_rad, z = halftone_mix, w = aspect_ratio
    halftone: vec4<f32>,
    // x = glow_threshold, y = glow_intensity, z = glow_radius_px, w = color_mode (0=Mono, 1=CMYK, 2=ColorDot)
    glow: vec4<f32>,
    // x = dot_softness [0..1], y, z, w = reserved
    pad: vec4<f32>,
}

@group(0) @binding(0)
var t_source: texture_2d<f32>;

@group(0) @binding(1)
var s_linear: sampler;

@group(0) @binding(2)
var<uniform> params: HalftoneGlowUniform;

const ALPHA_EPS: f32 = 1e-5;
const LUMA_REC709: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);
const GLOW_TAPS: i32 = 28;
const GOLDEN_ANGLE: f32 = 2.39996323;
const TAU: f32 = 6.28318530718;

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VertexOutput {
    let x = f32((vid << 1u) & 2u);
    let y = f32(vid & 2u);
    var out: VertexOutput;
    out.position = vec4<f32>(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, 1.0 - y);
    return out;
}

fn ign(frag_coord: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(frag_coord, vec2<f32>(0.06711056, 0.00583715))));
}

fn eval_sdf_dot(st: vec2<f32>, dot_scale: f32, angle: f32) -> f32 {
    let ca = cos(angle);
    let sa = sin(angle);
    let rot_uv = vec2<f32>(ca * st.x - sa * st.y, sa * st.x + ca * st.y);
    let cell_uv = 2.0 * fract(rot_uv * dot_scale) - 1.0;
    return length(cell_uv);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = max(vec2<f32>(textureDimensions(t_source)), vec2<f32>(1.0));
    let texel = 1.0 / dims;
    let aspect = select(params.halftone.w, dims.x / dims.y, params.halftone.w <= 1e-4);

    // Compute rotated 2D SDF screentone distances and screen-space derivatives in uniform control flow.
    let dot_scale = select(params.halftone.x, 60.0, params.halftone.x <= 1e-4);
    let angle = params.halftone.y;
    let halftone_mix = clamp(params.halftone.z, 0.0, 1.0);
    let color_mode = u32(clamp(round(params.glow.w), 0.0, 2.0));
    let dot_softness = clamp(params.pad.x, 0.0, 1.0);

    let st = vec2<f32>(in.uv.x * aspect, in.uv.y);
    let sdf_k = eval_sdf_dot(st, dot_scale, angle);
    let aa_base = max(length(vec2<f32>(dpdx(sdf_k), dpdy(sdf_k))) * 0.70710678, 1e-4);
    let aa_width = aa_base + dot_softness * 0.35;

    // Precompute CMYK rosette plates in uniform control flow (15 deg, 75 deg, 0 deg, 45 deg offsets).
    let sdf_c = eval_sdf_dot(st, dot_scale, angle + 0.26179939);
    let sdf_m = eval_sdf_dot(st, dot_scale, angle + 1.30899694);
    let sdf_y = eval_sdf_dot(st, dot_scale, angle);
    let sdf_k_plate = eval_sdf_dot(st, dot_scale, angle + 0.78539816);

    let src = textureSampleLevel(t_source, s_linear, in.uv, 0.0);

    // 28-tap multi-ring Vogel disk + IGN angular jitter for smooth, polka-dot-free HDR bloom.
    let glow_thresh = max(params.glow.x, 0.0);
    let glow_intensity = max(params.glow.y, 0.0);
    let glow_radius_px = max(params.glow.z, 0.0);
    let knee = max(glow_thresh * 0.35, 0.05);
    let step_uv = texel * glow_radius_px;
    let base_rot = ign(in.position.xy) * TAU;

    var glow_acc = vec3<f32>(0.0);
    var glow_w_sum = 0.0;
    if (glow_intensity > 1e-5 && glow_radius_px > 1e-4) {
        for (var i: i32 = 0; i < GLOW_TAPS; i = i + 1) {
            let t = (f32(i) + 0.5) / f32(GLOW_TAPS);
            let r_norm = pow(t, 0.72) * 1.95;
            let ang = f32(i) * GOLDEN_ANGLE + base_rot;
            let ofs = vec2<f32>(cos(ang), sin(ang)) * r_norm * step_uv;
            let tap_uv = clamp(in.uv + ofs, vec2<f32>(0.0), vec2<f32>(1.0));
            let tap = textureSampleLevel(t_source, s_linear, tap_uv, 0.0);
            let tap_rgb = max(tap.rgb, vec3<f32>(0.0));
            let luma = dot(tap_rgb, LUMA_REC709);
            let soft = clamp(luma - glow_thresh + knee, 0.0, 2.0 * knee);
            let soft_term = (soft * soft) / (4.0 * knee + 1e-5);
            let contrib = max(luma - glow_thresh, soft_term) / max(luma, 1e-5);
            let w = exp(-1.65 * r_norm * r_norm);
            glow_acc = glow_acc + tap_rgb * (contrib * w);
            glow_w_sum = glow_w_sum + w;
        }
        glow_acc = glow_acc / max(glow_w_sum, 1e-5);
    }

    var base_premul = vec3<f32>(0.0);
    if (src.a > ALPHA_EPS) {
        let base_rgb = max(src.rgb / src.a, vec3<f32>(0.0));
        var screentone_rgb = base_rgb;

        if (color_mode == 1u) {
            // 4-plate CMYK subtractive screentone in perceptual space, preserving HDR excess.
            let tone = clamp(base_rgb / (vec3<f32>(1.0) + base_rgb * 0.25), vec3<f32>(0.0), vec3<f32>(1.0));
            let s = pow(tone, vec3<f32>(1.0 / 2.2));
            let k_ink = 1.0 - max(s.r, max(s.g, s.b));
            let cmy_ink = clamp((vec3<f32>(1.0) - s - vec3<f32>(k_ink)) / max(1.0 - k_ink, 1e-4), vec3<f32>(0.0), vec3<f32>(1.0));

            let r_c = sqrt(cmy_ink.x) * 1.05;
            let r_m = sqrt(cmy_ink.y) * 1.05;
            let r_y = sqrt(cmy_ink.z) * 1.05;
            let r_k = sqrt(k_ink) * 1.05;

            let dot_c = 1.0 - smoothstep(r_c - aa_width, r_c + aa_width, sdf_c);
            let dot_m = 1.0 - smoothstep(r_m - aa_width, r_m + aa_width, sdf_m);
            let dot_y = 1.0 - smoothstep(r_y - aa_width, r_y + aa_width, sdf_y);
            let dot_k = 1.0 - smoothstep(r_k - aa_width, r_k + aa_width, sdf_k_plate);

            let paper = vec3<f32>(0.97);
            let s_out = (paper - vec3<f32>(dot_c, dot_m, dot_y) * 0.92) * (1.0 - 0.90 * dot_k);
            let hdr_excess = max(base_rgb - vec3<f32>(1.0), vec3<f32>(0.0));
            screentone_rgb = pow(clamp(s_out, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(2.2)) + hdr_excess * 0.5;
        } else if (color_mode == 2u) {
            // ColorDot pop-art mode: preserves hue while carving halftone dots by tone luma.
            let base_luma = dot(base_rgb, LUMA_REC709);
            let tone_luma = clamp(base_luma / (1.0 + base_luma * 0.25), 0.0, 1.0);
            let r_dot = sqrt(tone_luma) * 1.08;
            let dot_mask = 1.0 - smoothstep(r_dot - aa_width, r_dot + aa_width, sdf_k);
            screentone_rgb = mix(base_rgb * 0.08, base_rgb, dot_mask);
        } else {
            // Default Monochrome manga screentone.
            let base_luma = dot(base_rgb, LUMA_REC709);
            let tone_luma = clamp(base_luma / (1.0 + base_luma * 0.25), 0.0, 1.0);
            let ink = clamp(1.0 - tone_luma, 0.0, 1.0);
            let r_dot = sqrt(ink) * 1.05;
            let dot_mask = 1.0 - smoothstep(r_dot - aa_width, r_dot + aa_width, sdf_k);
            screentone_rgb = mix(vec3<f32>(0.96), vec3<f32>(0.02), dot_mask);
        }

        let blended_rgb = mix(base_rgb, screentone_rgb, halftone_mix);
        base_premul = blended_rgb * src.a;
    }

    let out_rgb = base_premul + glow_acc * glow_intensity;
    // Additive glow guard: keep empty transparent pixels exactly zero.
    if (src.a <= ALPHA_EPS && max(out_rgb.r, max(out_rgb.g, out_rgb.b)) <= ALPHA_EPS) {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(out_rgb, src.a);
}
