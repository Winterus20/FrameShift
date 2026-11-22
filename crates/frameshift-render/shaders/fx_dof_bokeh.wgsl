// FrameShift Faz 6.6 Shader: fx_dof_bokeh.wgsl
// Pass 3 of Z-buffer Bokeh DoF: bounded <= 32-tap Polygonal (6/8-blade) + Anamorphic Oval +
// Cat's-Eye Optical Vignetting Vogel Disk with Jimenez (2014) IGN jitter,
// leak-guarded background gather, Porter-Duff foreground alpha compositing,
// and additive Scene3D atmosphere (RibbonTrail3D / ParticleField3D) layer merge.
// Working format: Rgba16Float, Linear Rec.709, premultiplied alpha.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

// 48 bytes, 16-byte aligned.
struct DofBokehUniform {
    // x = texel_x, y = texel_y, z = max_radius_px, w = tap_count
    params0: vec4<f32>,
    // x = highlight_boost, y = near_bleed_bias, z = jitter_seed, w = blade_roundness [0..1]
    params1: vec4<f32>,
    // x = blade_count (0=circle, 6=hex, 8=oct), y = blade_rotation_rad,
    // z = anamorphic_ratio (1.0=spherical, 0.5=2:1 vertical oval), w = cats_eye_strength [0..1]
    params2: vec4<f32>,
}

@group(0) @binding(0)
var color_tex: texture_2d<f32>;

@group(0) @binding(1)
var coc_tex: texture_2d<f32>;

@group(0) @binding(2)
var atmos_tex: texture_2d<f32>;

@group(0) @binding(3)
var s_linear: sampler;

@group(0) @binding(4)
var<uniform> u: DofBokehUniform;

const GOLDEN_ANGLE: f32 = 2.39996323;
const MAX_VOGEL_TAPS: i32 = 32;

// Jimenez (2014) Interleaved Gradient Noise for temporally stable per-pixel disk rotation.
fn interleaved_gradient_noise(pixel_xy: vec2<f32>) -> f32 {
    let magic = vec3<f32>(0.06711056, 0.00583715, 52.9829189);
    return fract(magic.z * fract(dot(pixel_xy, magic.xy)));
}

// Bijective N-gon radial scaling keeping polygon inscribed inside unit circle.
fn polygon_radial_scale(theta: f32, blades_f: f32, rot_rad: f32, roundness: f32) -> f32 {
    if (blades_f < 3.0) {
        return 1.0;
    }
    let n = clamp(round(blades_f), 3.0, 12.0);
    let sector = 6.2831853 / n;
    let half_sector = 0.5 * sector;
    let a = theta - rot_rad;
    let d_theta = abs(a - floor((a + half_sector) / sector) * sector);
    let poly_r = cos(half_sector) / max(cos(d_theta), 1e-4);
    return mix(poly_r, 1.0, clamp(roundness, 0.0, 1.0));
}

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
    let center_raw = textureSampleLevel(color_tex, s_linear, in.uv, 0.0);
    let center_col = vec4<f32>(max(center_raw.rgb, vec3<f32>(0.0)), clamp(center_raw.a, 0.0, 1.0));
    let atmos_raw = textureSampleLevel(atmos_tex, s_linear, in.uv, 0.0);
    let atmos_col = vec4<f32>(max(atmos_raw.rgb, vec3<f32>(0.0)), clamp(atmos_raw.a, 0.0, 1.0));

    let center_coc_data = textureSampleLevel(coc_tex, s_linear, in.uv, 0.0);
    let center_coc = center_coc_data.r;
    let dilated_near = center_coc_data.g;
    let center_z = center_coc_data.b;
    let max_rad = clamp(u.params0.z, 0.0, 48.0);
    let gather_radius = min(max(abs(center_coc), dilated_near), max_rad);

    // Early-out preserves razor sharpness on in-focus pixels while compositing additive atmosphere.
    if (gather_radius <= 0.35) {
        return vec4<f32>(
            center_col.rgb + atmos_col.rgb,
            clamp(center_col.a + atmos_col.a * (1.0 - center_col.a), 0.0, 1.0),
        );
    }

    let dims = max(vec2<f32>(textureDimensions(color_tex)), vec2<f32>(1.0));
    let auto_texel = 1.0 / dims;
    let texel = vec2<f32>(
        select(u.params0.x, auto_texel.x, u.params0.x <= 1e-6),
        select(u.params0.y, auto_texel.y, u.params0.y <= 1e-6),
    );

    let taps = clamp(i32(round(u.params0.w)), 1, MAX_VOGEL_TAPS);
    let highlight_boost = max(u.params1.x, 0.0);
    let near_bleed_bias = max(u.params1.y, 0.0);
    let rot_offset = interleaved_gradient_noise(in.position.xy + vec2<f32>(u.params1.z)) * 6.2831853;

    let blade_roundness = clamp(u.params1.w, 0.0, 1.0);
    let blade_count = u.params2.x;
    let blade_rot_rad = u.params2.y;
    let anam = clamp(select(u.params2.z, 1.0, u.params2.z <= 0.0), 0.25, 4.0);
    let anam_scale = select(vec2<f32>(1.0, 1.0 / anam), vec2<f32>(anam, 1.0), anam <= 1.0);

    // Cat's-eye optical vignetting toward frame corners.
    let aspect = dims.x / max(dims.y, 1.0);
    let p_screen = (in.uv - vec2<f32>(0.5)) * vec2<f32>(aspect, 1.0);
    let max_corner = length(vec2<f32>(0.5 * aspect, 0.5));
    let ecc = clamp(length(p_screen) / max(max_corner, 1e-4), 0.0, 1.0);
    let radial_dir = select(vec2<f32>(0.0), normalize(p_screen), length(p_screen) > 1e-5);
    let cat_shift = clamp(u.params2.w, 0.0, 1.0) * ecc * 0.72;

    var bg_acc = vec4<f32>(0.0);
    var bg_w = 0.0;
    var fg_acc = vec4<f32>(0.0);
    var fg_w = 0.0;
    var fg_cov_sum = 0.0;
    var cat_w_sum = 0.0;

    for (var i: i32 = 0; i < MAX_VOGEL_TAPS; i = i + 1) {
        if (i >= taps) {
            break;
        }
        let fi = f32(i);
        let r_norm = sqrt((fi + 0.5) / f32(taps));
        let theta = fi * GOLDEN_ANGLE + rot_offset;
        let poly_scale = polygon_radial_scale(theta, blade_count, blade_rot_rad, blade_roundness);
        let disk_dir = vec2<f32>(cos(theta), sin(theta)) * poly_scale;

        let q = disk_dir * r_norm;
        let cat_mask = smoothstep(1.02, 0.85, length(q - radial_dir * cat_shift));
        cat_w_sum = cat_w_sum + cat_mask;

        let r_px = r_norm * gather_radius;
        let offset_uv = disk_dir * anam_scale * r_px * texel;
        let suv = clamp(in.uv + offset_uv, vec2<f32>(0.0), vec2<f32>(1.0));

        let s_col_raw = textureSampleLevel(color_tex, s_linear, suv, 0.0);
        let s_col = vec4<f32>(max(s_col_raw.rgb, vec3<f32>(0.0)), clamp(s_col_raw.a, 0.0, 1.0));
        let s_coc_data = textureSampleLevel(coc_tex, s_linear, suv, 0.0);
        let s_coc = s_coc_data.r;
        let s_z = s_coc_data.b;

        // HDR bokeh highlight boost (> 0.85 Linear Rec.709 luminance).
        let luma = dot(s_col.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
        let hdr_w = 1.0 + max(luma - 0.85, 0.0) * highlight_boost;

        // Foreground scatter-as-gather (s_coc < 0): blurred foreground bleeds over sharp background.
        let fg_coc_abs = max(-s_coc, 0.0);
        let fg_cov = smoothstep(r_px - 1.0, r_px + 1.0, fg_coc_abs) * cat_mask;
        if (fg_cov > 0.001) {
            let w_fg = fg_cov * hdr_w;
            fg_acc = fg_acc + s_col * w_fg;
            fg_w = fg_w + w_fg;
            fg_cov_sum = fg_cov_sum + fg_cov;
        }

        // Background gather: reject foreground taps (s_coc < 0) when center is on/behind focal plane.
        let s_is_fg = s_coc < 0.0;
        let depth_gate = smoothstep(center_z - 0.15, center_z + 0.05, s_z);
        let bg_allow = select(depth_gate, 0.0, s_is_fg && center_coc >= 0.0);
        let bg_rad = max(max(s_coc, center_coc), 0.0);
        let bg_cov = smoothstep(r_px - 1.0, r_px + 1.0, bg_rad) * bg_allow * cat_mask;
        if (bg_cov > 0.001) {
            let w_bg = bg_cov * hdr_w;
            bg_acc = bg_acc + s_col * w_bg;
            bg_w = bg_w + w_bg;
        }
    }

    let bg_color = select(center_col, bg_acc / max(bg_w, 1e-5), bg_w > 1e-4);
    let fg_avg = select(vec4<f32>(0.0), fg_acc / max(fg_w, 1e-5), fg_w > 1e-4);
    let fg_spatial_cov = clamp((fg_cov_sum / max(cat_w_sum, 1e-4)) * (1.0 + near_bleed_bias), 0.0, 1.0);
    let fg_layer = fg_avg * fg_spatial_cov;

    // Additive atmosphere (Phase B ribbons/particles) over background bokeh, under foreground bokeh halo.
    let bg_with_atmos = vec4<f32>(
        bg_color.rgb + atmos_col.rgb,
        clamp(bg_color.a + atmos_col.a * (1.0 - bg_color.a), 0.0, 1.0),
    );

    // Porter-Duff Source-Over preserves semi-transparent foreground alpha cleanly.
    let out_rgba = fg_layer + bg_with_atmos * (1.0 - clamp(fg_layer.a, 0.0, 1.0));
    return vec4<f32>(max(out_rgba.rgb, vec3<f32>(0.0)), clamp(out_rgba.a, 0.0, 1.0));
}
