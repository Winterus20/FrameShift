// FrameShift Faz 6.6 Shader: text3d_mesh.wgsl
// 3D extruded TTF/OTF typography & Shape3D primitives with Filament Cook-Torrance GGX PBR,
// camera-relative studio IBL (zero 90-deg turn darkening), bevel/crease cavity AO,
// stateless per-char kinetic pop-in, 3-axis spin-in, XY/XZ arc curvature,
// and depth-write-separated back-to-front Z-echo instancing.
// Working format: Rgba16Float, Linear Rec.709, premultiplied alpha.

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) char_center: vec2<f32>,
    @location(4) char_index: u32,
    @location(5) face_kind: u32,
}

struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) @interpolate(flat) face_kind: u32,
    @location(4) @interpolate(flat) char_index: u32,
    @location(5) @interpolate(flat) inst_alpha: f32,
    @location(6) local_bevel_factor: f32,
}

// 432 bytes, 16-byte aligned.
struct Text3DUniform {
    mvp: mat4x4<f32>,                 //   0..64
    model: mat4x4<f32>,               //  64..128
    view: mat4x4<f32>,                // 128..192
    // xyz = camera_world_pos, w = material_mode (0=Neon3D, 1=MetallicChrome, 2=Flat2DStroked, 3=IridescentAnisotropic)
    cam_material: vec4<f32>,          // 192..208
    color_top: vec4<f32>,             // 208..224
    color_bottom: vec4<f32>,          // 224..240
    side_color: vec4<f32>,            // 240..256
    outline_color: vec4<f32>,         // 256..272
    // x = time_sec, y = char_stagger_sec, z = char_bounce_amp, w = char_wave_freq
    anim: vec4<f32>,                  // 272..288
    // x = pop_duration_sec, y = pop_overshoot_c1, z = spin_in_deg, w = char_jitter_deg
    kinetic: vec4<f32>,               // 288..304
    // x = arc_curvature, y = arc_axis (0=XY, 1=XZ), z = color_mode (0=grad, 1=palette, 2=rainbow), w = palette_count
    arc_palette: vec4<f32>,           // 304..320
    // xyz = echo_offset, w = echo_count (0..16)
    echo_params: vec4<f32>,           // 320..336
    // x = echo_alpha_decay, y = echo_scale_step, z = hue_shift_per_char, w = hdr_intensity
    echo_decay: vec4<f32>,            // 336..352
    palette: array<vec4<f32>, 4>,     // 352..416
    // x = roughness (<0 -> material_mode default), y = metallic (<0 -> material_mode default),
    // z = ao_strength [0..1], w = echo_pass_flag (0 = hero layer 0, 1 = echo layers total_echoes..1)
    pbr_params: vec4<f32>,            // 416..432
}

@group(0) @binding(0)
var<uniform> u: Text3DUniform;

const PI: f32 = 3.14159265359;

fn safe_normalize(v: vec3<f32>, fallback: vec3<f32>) -> vec3<f32> {
    let len_sq = dot(v, v);
    if (len_sq <= 1e-10) {
        return fallback;
    }
    return v * inverseSqrt(len_sq);
}

fn pcg_hash(v_in: u32) -> u32 {
    let state = v_in * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

fn hash_f32(idx: u32, salt: u32) -> f32 {
    let h = pcg_hash(idx * 1664525u + salt * 1013904223u + 0x9E3779B9u);
    return f32(h & 0x00FFFFFFu) / 16777216.0;
}

fn ease_out_back(x: f32, c1: f32) -> f32 {
    let c3 = c1 + 1.0;
    let xm1 = x - 1.0;
    return 1.0 + c3 * (xm1 * xm1 * xm1) + c1 * (xm1 * xm1);
}

fn rotate_xyz(v: vec3<f32>, ax: f32, ay: f32, az: f32) -> vec3<f32> {
    let cx = cos(ax);
    let sx = sin(ax);
    let y1 = v.y * cx - v.z * sx;
    let z1 = v.y * sx + v.z * cx;

    let cy = cos(ay);
    let sy = sin(ay);
    let x2 = v.x * cy + z1 * sy;
    let z2 = -v.x * sy + z1 * cy;

    let cz = cos(az);
    let sz = sin(az);
    let x3 = x2 * cz - y1 * sz;
    let y3 = x2 * sz + y1 * cz;

    return vec3<f32>(x3, y3, z2);
}

fn rotate_z(v: vec3<f32>, angle: f32) -> vec3<f32> {
    let c = cos(angle);
    let s = sin(angle);
    return vec3<f32>(v.x * c - v.y * s, v.x * s + v.y * c, v.z);
}

fn rotate_y(v: vec3<f32>, angle: f32) -> vec3<f32> {
    let c = cos(angle);
    let s = sin(angle);
    return vec3<f32>(v.x * c + v.z * s, v.y, -v.x * s + v.z * c);
}

fn hsv_to_linear_rgb(h: f32, s: f32, v: f32) -> vec3<f32> {
    let k = vec3<f32>(1.0, 2.0 / 3.0, 1.0 / 3.0);
    let p = abs(fract(vec3<f32>(h) + k) * 6.0 - vec3<f32>(3.0));
    let srgb = clamp(v * mix(vec3<f32>(1.0), clamp(p - vec3<f32>(1.0), vec3<f32>(0.0), vec3<f32>(1.0)), s), vec3<f32>(0.0), vec3<f32>(1.0));
    return srgb * (srgb * (srgb * 0.305306011 + 0.682171111) + 0.012522878);
}

// Filament Cook-Torrance GGX Microfacet BRDF
fn d_ggx(no_h: f32, a: f32) -> f32 {
    let a2 = a * a;
    let f = (no_h * a2 - no_h) * no_h + 1.0;
    return a2 / (PI * max(f * f, 1e-6));
}

fn v_smith_ggx_correlated(no_v: f32, no_l: f32, a: f32) -> f32 {
    let a2 = a * a;
    let ggxv = no_l * sqrt((-no_v * a2 + no_v) * no_v + a2);
    let ggxl = no_v * sqrt((-no_l * a2 + no_l) * no_l + a2);
    return 0.5 / max(ggxv + ggxl, 1e-6);
}

fn f_schlick(u_cos: f32, f0: vec3<f32>) -> vec3<f32> {
    let f = pow(clamp(1.0 - u_cos, 0.0, 1.0), 5.0);
    return f0 + (vec3<f32>(1.0) - f0) * f;
}

fn f_schlick_roughness(no_v: f32, f0: vec3<f32>, roughness: f32) -> vec3<f32> {
    let f = pow(clamp(1.0 - no_v, 0.0, 1.0), 5.0);
    return f0 + (max(vec3<f32>(1.0 - roughness), f0) - f0) * f;
}

// Karis (2014) / Lazarov (2013) Analytic Split-Sum EnvBRDFApprox
fn env_brdf_approx(f0: vec3<f32>, roughness: f32, no_v: f32) -> vec3<f32> {
    let c0 = vec4<f32>(-1.0, -0.0275, -0.572, 0.022);
    let c1 = vec4<f32>(1.0, 0.0425, 1.04, -0.04);
    let r = roughness * c0 + c1;
    let a004 = min(r.x * r.x, exp2(-9.28 * no_v)) * r.x + r.y;
    let ab = vec2<f32>(-1.04, 1.04) * a004 + r.zw;
    return clamp(f0 * ab.x + vec3<f32>(ab.y), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn eval_direct_pbr(
    n: vec3<f32>,
    v: vec3<f32>,
    l: vec3<f32>,
    light_rgb: vec3<f32>,
    diffuse_col: vec3<f32>,
    f0: vec3<f32>,
    a: f32,
) -> vec3<f32> {
    let no_l = max(dot(n, l), 0.0);
    if (no_l <= 1e-5) {
        return vec3<f32>(0.0);
    }
    let no_v = max(dot(n, v), 1e-4);
    let h = safe_normalize(v + l, n);
    let no_h = clamp(dot(n, h), 0.0, 1.0);
    let lo_h = clamp(dot(l, h), 0.0, 1.0);

    let d = d_ggx(no_h, a);
    let vis = v_smith_ggx_correlated(no_v, no_l, a);
    let f = f_schlick(lo_h, f0);

    let fr = (d * vis) * f;
    let kd = (vec3<f32>(1.0) - f);
    let fd = kd * diffuse_col * (1.0 / PI);
    return (fd + fr) * light_rgb * no_l;
}

// Camera-relative + world-blended analytic studio IBL (never turns black on 90 deg turns).
fn sample_studio_ibl(r_world: vec3<f32>, roughness: f32) -> vec3<f32> {
    let r_view = safe_normalize((u.view * vec4<f32>(r_world, 0.0)).xyz, vec3<f32>(0.0, 0.0, 1.0));
    let r_env = safe_normalize(mix(r_view, r_world, 0.25), r_view);

    let band_soft = roughness * 0.35;
    let sky_col = mix(
        vec3<f32>(0.24, 0.32, 0.48),
        vec3<f32>(0.96, 0.99, 1.08),
        smoothstep(0.02, 0.78, r_env.y),
    );
    let ground_col = mix(
        vec3<f32>(0.05, 0.05, 0.08),
        vec3<f32>(0.22, 0.20, 0.28),
        smoothstep(-0.85, -0.02, r_env.y),
    );
    let dome = mix(ground_col, sky_col, smoothstep(-0.18 - band_soft, 0.12 + band_soft, r_env.y));

    let glint_sharp = mix(16.0, 2.8, roughness);
    let horizon_glint = exp(-abs(r_env.y) * glint_sharp) * (1.0 - roughness * 0.55) * vec3<f32>(1.35, 1.38, 1.48);

    // Camera-attached softbox strips so facing caps and bevels stay richly lit at any yaw/pitch.
    let sb_power_a = mix(14.0, 2.5, roughness);
    let sb_power_b = mix(10.0, 2.0, roughness);
    let sb_front_top = pow(max(dot(r_view, normalize(vec3<f32>(0.28, 0.72, 0.63))), 0.0), sb_power_a) * mix(1.35, 0.45, roughness);
    let sb_left_cool = pow(max(dot(r_view, normalize(vec3<f32>(-0.82, 0.32, 0.48))), 0.0), sb_power_b) * mix(0.75, 0.28, roughness) * vec3<f32>(0.78, 0.92, 1.15);
    let sb_right_warm = pow(max(dot(r_view, normalize(vec3<f32>(0.78, -0.18, 0.60))), 0.0), sb_power_b) * mix(0.55, 0.22, roughness) * vec3<f32>(1.12, 0.86, 0.96);

    return dome + horizon_glint + vec3<f32>(sb_front_top) + sb_left_cool + sb_right_warm;
}

@vertex
fn vs_main(
    in: VertexInput,
    @builtin(instance_index) instance_id: u32,
) -> VertexOutput {
    let local_t = u.anim.x - f32(in.char_index) * u.anim.y;
    let phase = local_t * u.anim.w;
    let amp = u.anim.z;

    let y_bounce = sin(phase) * amp;
    let z_tilt = sin(phase * 0.75) * clamp(amp, -1.0, 1.0) * 0.65;

    let pop_dur = u.kinetic.x;
    var pop_scale = 1.0;
    var spin_decay = 0.0;
    var char_alpha = 1.0;
    if (pop_dur > 0.0) {
        if (local_t <= 0.0) {
            pop_scale = 0.0;
            spin_decay = 1.0;
            char_alpha = 0.0;
        } else {
            let p = clamp(local_t / pop_dur, 0.0, 1.0);
            let c1 = max(u.kinetic.y, 0.0);
            pop_scale = max(ease_out_back(p, c1), 0.0);
            spin_decay = pow(1.0 - p, 3.0);
            char_alpha = 1.0;
        }
    }

    let h0 = hash_f32(in.char_index, 1u);
    let h1 = hash_f32(in.char_index, 2u);
    let h2 = hash_f32(in.char_index, 3u);
    let sign_z = select(-1.0, 1.0, h0 >= 0.5);
    let sign_y = select(-1.0, 1.0, h1 >= 0.5);
    let jitter_z = radians(u.kinetic.w) * (h2 * 2.0 - 1.0);

    let spin_rad = radians(u.kinetic.z);
    let theta_z = spin_rad * spin_decay * sign_z + z_tilt + jitter_z;
    let theta_y = (spin_rad * 0.65) * spin_decay * sign_y;
    let theta_x = (spin_rad * 0.35) * spin_decay;

    let pivot = vec3<f32>(in.char_center, 0.0);
    let p_rel = in.position - pivot;
    let p_scaled = p_rel * pop_scale;
    let p_rot = rotate_xyz(p_scaled, theta_x, theta_y, theta_z);
    let p_local = pivot + p_rot + vec3<f32>(0.0, y_bounce, 0.0);
    let n_rot = rotate_xyz(in.normal, theta_x, theta_y, theta_z);

    // Optional XY rainbow/smile arc or XZ cylindrical wrap around camera.
    var p_curved = p_local;
    var n_curved = n_rot;
    let k = u.arc_palette.x;
    if (abs(k) > 1e-4) {
        let radius = 1.0 / k;
        let phi = clamp(p_local.x * k, -3.14159265, 3.14159265);
        let half_sin = sin(0.5 * phi);
        let one_minus_cos = 2.0 * half_sin * half_sin;
        let arc_axis = u32(clamp(round(u.arc_palette.y), 0.0, 1.0));
        if (arc_axis == 0u) {
            p_curved = vec3<f32>(
                (radius + p_local.y) * sin(phi),
                p_local.y * cos(phi) - radius * one_minus_cos,
                p_local.z,
            );
            n_curved = rotate_z(n_rot, -phi);
        } else {
            p_curved = vec3<f32>(
                (radius + p_local.z) * sin(phi),
                p_local.y,
                p_local.z * cos(phi) - radius * one_minus_cos,
            );
            n_curved = rotate_y(n_rot, phi);
        }
    }

    // Separated hero (pbr_params.w < 0.5 -> layer 0) vs back-to-front Z-echo pass (pbr_params.w >= 0.5 -> layer total_echoes..1).
    let total_echoes = u32(clamp(round(u.echo_params.w), 0.0, 16.0));
    let is_echo_pass = u.pbr_params.w > 0.5;
    var echo_layer = 0u;
    if (is_echo_pass && total_echoes > 0u) {
        echo_layer = total_echoes - min(instance_id, total_echoes);
    }

    var p_final = p_curved;
    var echo_alpha = 1.0;
    if (echo_layer >= 1u) {
        let fl = f32(echo_layer);
        let scale_step = max(u.echo_decay.y, 0.0);
        let echo_scale = pow(scale_step, fl);
        p_final = p_curved * echo_scale + u.echo_params.xyz * fl;
        echo_alpha = pow(clamp(u.echo_decay.x, 0.0, 1.0), fl);
    }

    let anim_normal = safe_normalize(n_curved, vec3<f32>(0.0, 0.0, 1.0));
    let world_pos4 = u.model * vec4<f32>(p_final, 1.0);
    let world_n_raw = (u.model * vec4<f32>(anim_normal, 0.0)).xyz;

    var out: VertexOutput;
    out.clip_pos = u.mvp * vec4<f32>(p_final, 1.0);
    out.world_pos = world_pos4.xyz;
    out.world_normal = safe_normalize(world_n_raw, vec3<f32>(0.0, 0.0, 1.0));
    out.uv = in.uv;
    out.face_kind = in.face_kind;
    out.char_index = in.char_index;
    out.inst_alpha = char_alpha * echo_alpha;
    out.local_bevel_factor = abs(in.normal.z);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Compute screen-space normal gradient in uniform control flow BEFORE any early return or discard.
    let n = safe_normalize(in.world_normal, vec3<f32>(0.0, 0.0, 1.0));
    let n_grad = length(fwidth(n));

    let grad_t = clamp(in.uv.y, 0.0, 1.0);
    let color_mode = u32(clamp(round(u.arc_palette.z), 0.0, 2.0));

    var cap_color = mix(u.color_bottom, u.color_top, grad_t);
    if (color_mode == 1u) {
        let p_count = clamp(u32(round(u.arc_palette.w)), 1u, 4u);
        let slot = in.char_index % p_count;
        let base_p = u.palette[slot];
        cap_color = vec4<f32>(max(base_p.rgb, vec3<f32>(0.0)) * mix(0.82, 1.18, grad_t), base_p.a);
    } else if (color_mode == 2u) {
        let hue_step = select(u.echo_decay.z, 0.16, abs(u.echo_decay.z) <= 1e-5);
        let hue = fract(f32(in.char_index) * hue_step + u.anim.x * 0.15);
        let rain_rgb = hsv_to_linear_rgb(hue, 0.92, 1.0);
        let cap_a = mix(u.color_bottom.a, u.color_top.a, grad_t);
        cap_color = vec4<f32>(rain_rgb * mix(0.82, 1.18, grad_t), cap_a);
    }

    var base_color = cap_color;
    if (in.face_kind == 3u) {
        base_color = u.outline_color;
    } else if (in.face_kind == 1u) {
        base_color = u.side_color;
    }

    let alpha = clamp(base_color.a * in.inst_alpha, 0.0, 1.0);
    if (alpha <= 0.001) {
        discard;
    }

    let base_rgb = max(base_color.rgb, vec3<f32>(0.0));
    let hdr_intensity = max(u.echo_decay.w, 0.0);
    let material_mode = u32(clamp(round(u.cam_material.w), 0.0, 3.0));

    // Crisp unshadowed HDR emission for outer stroke/border faces (face_kind == 3u)
    // and Flat2DStroked front/back caps.
    if (in.face_kind == 3u) {
        let out_rgb = base_rgb * hdr_intensity;
        return vec4<f32>(max(out_rgb, vec3<f32>(0.0)) * alpha, alpha);
    }

    if (material_mode == 2u) {
        let side_shade = select(1.0, 0.82, in.face_kind == 1u);
        let flat_rgb = base_rgb * side_shade * hdr_intensity;
        return vec4<f32>(max(flat_rgb, vec3<f32>(0.0)) * alpha, alpha);
    }

    let view_dir = safe_normalize(u.cam_material.xyz - in.world_pos, vec3<f32>(0.0, 0.0, 1.0));
    let no_v = max(dot(n, view_dir), 1e-4);

    // Extract camera basis in world space from view matrix columns so lighting follows 90-deg turns.
    let cam_right = safe_normalize(vec3<f32>(u.view[0].x, u.view[1].x, u.view[2].x), vec3<f32>(1.0, 0.0, 0.0));
    let cam_up = safe_normalize(vec3<f32>(u.view[0].y, u.view[1].y, u.view[2].y), vec3<f32>(0.0, 1.0, 0.0));
    let cam_back = safe_normalize(vec3<f32>(u.view[0].z, u.view[1].z, u.view[2].z), vec3<f32>(0.0, 0.0, 1.0));

    let key_dir = safe_normalize(cam_right * 0.42 + cam_up * 0.62 + cam_back * 0.66, cam_back);
    let fill_dir = safe_normalize(-cam_right * 0.72 + cam_up * 0.18 + cam_back * 0.67, cam_back);
    let rim_dir = safe_normalize(-cam_right * 0.30 + cam_up * 0.65 - cam_back * 0.70, -cam_back);

    // Bevel & Crease Cavity AO (wall recess + fwidth curvature + Filament SpecularAO).
    let wall_ao = select(1.0, mix(0.62, 0.96, smoothstep(0.05, 0.75, in.local_bevel_factor)), in.face_kind == 1u);
    let crease_ao = 1.0 - clamp(n_grad * 0.42, 0.0, 0.35);
    let ao_strength = clamp(u.pbr_params.z, 0.0, 1.0);
    let ao = mix(1.0, wall_ao * crease_ao, ao_strength);

    // Resolve roughness & metallic defaults per material_mode.
    var default_rough = 0.26;
    var default_metal = 0.18;
    if (material_mode == 1u) {
        default_rough = 0.12;
        default_metal = 0.98;
    } else if (material_mode == 3u) {
        default_rough = 0.18;
        default_metal = 0.92;
    }
    let roughness = clamp(select(u.pbr_params.x, default_rough, u.pbr_params.x < 0.0), 0.045, 1.0);
    let metallic = clamp(select(u.pbr_params.y, default_metal, u.pbr_params.y < 0.0), 0.0, 1.0);
    let alpha_r = roughness * roughness;
    let spec_ao = clamp(pow(no_v + ao, exp2(-16.0 * roughness - 1.0)) - 1.0 + ao, 0.0, 1.0);

    if (material_mode == 1u) {
        // MetallicChrome: Cook-Torrance GGX + Camera-Relative Studio IBL + SpecularAO.
        let albedo = mix(base_rgb, mix(vec3<f32>(0.88), base_rgb, 0.58), metallic);
        let f0 = mix(vec3<f32>(0.04), albedo, metallic);
        let diff_col = albedo * (1.0 - metallic);

        let direct = eval_direct_pbr(n, view_dir, key_dir, vec3<f32>(2.4, 2.38, 2.45), diff_col, f0, alpha_r)
            + eval_direct_pbr(n, view_dir, fill_dir, vec3<f32>(0.75, 0.85, 1.05), diff_col, f0, alpha_r)
            + eval_direct_pbr(n, view_dir, rim_dir, vec3<f32>(1.15, 1.20, 1.35), diff_col, f0, alpha_r);

        let r_vec = reflect(-view_dir, n);
        let env_spec = sample_studio_ibl(r_vec, roughness) * mix(vec3<f32>(1.0), base_rgb, 0.42);
        let env_diff = sample_studio_ibl(n, 0.95);
        let k_s = f_schlick_roughness(no_v, f0, roughness);
        let k_d = (vec3<f32>(1.0) - k_s) * (1.0 - metallic);
        let brdf_lut = env_brdf_approx(f0, roughness, no_v);

        let ibl = k_d * diff_col * env_diff * ao + env_spec * brdf_lut * spec_ao;
        let chrome_rgb = (direct + ibl) * hdr_intensity;
        return vec4<f32>(max(chrome_rgb, vec3<f32>(0.0)) * alpha, alpha);
    }

    if (material_mode == 3u) {
        // IridescentAnisotropic: Thin-film spectral Fresnel + GGX + brushed anisotropic studio rim.
        let thin_phase = (1.0 - no_v) * 2.2 + in.uv.x * 0.45 + in.uv.y * 0.25;
        let irid = 0.5 + 0.5 * cos(6.2831853 * (thin_phase + vec3<f32>(0.0, 0.33, 0.67)));
        let irid_lin = irid * irid;
        let f0 = mix(vec3<f32>(0.04), mix(base_rgb, irid_lin, 0.55 + 0.35 * (1.0 - no_v)), metallic);
        let diff_col = base_rgb * (1.0 - metallic);

        let up_ref = select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(n.y) > 0.98);
        let tangent = safe_normalize(cross(up_ref, n), cam_right);
        let half_key = safe_normalize(key_dir + view_dir, key_dir);
        let tdoth = dot(tangent, half_key);
        let aniso_band = pow(max(1.0 - tdoth * tdoth, 0.0), 22.0) * max(dot(n, key_dir), 0.0);

        let direct = eval_direct_pbr(n, view_dir, key_dir, vec3<f32>(2.1, 2.05, 2.2), diff_col, f0, alpha_r)
            + eval_direct_pbr(n, view_dir, fill_dir, vec3<f32>(0.7, 0.8, 1.0), diff_col, f0, alpha_r);

        let r_vec = reflect(-view_dir, n);
        let env_spec = sample_studio_ibl(r_vec, roughness);
        let env_diff = sample_studio_ibl(n, 0.95);
        let k_s = f_schlick_roughness(no_v, f0, roughness);
        let k_d = (vec3<f32>(1.0) - k_s) * (1.0 - metallic);
        let brdf_lut = env_brdf_approx(f0, roughness, no_v);

        let rim_fresnel = pow(1.0 - no_v, 3.0);
        let rim_hdr = irid_lin * rim_fresnel * 1.45 * spec_ao;
        let aniso_hdr = mix(vec3<f32>(1.0), irid_lin, 0.5) * aniso_band * 1.4;
        let ibl = k_d * diff_col * env_diff * ao + env_spec * brdf_lut * spec_ao;

        let final_rgb = (direct + ibl + aniso_hdr + rim_hdr) * hdr_intensity;
        return vec4<f32>(max(final_rgb, vec3<f32>(0.0)) * alpha, alpha);
    }

    // Neon3D (material_mode == 0u): Cook-Torrance GGX studio PBR + neon self-luminous body.
    let f0 = mix(vec3<f32>(0.04), mix(base_rgb, vec3<f32>(1.0), 0.35), metallic);
    let diff_col = base_rgb * (1.0 - metallic);

    let direct = eval_direct_pbr(n, view_dir, key_dir, vec3<f32>(1.95, 1.90, 1.85), diff_col, f0, alpha_r)
        + eval_direct_pbr(n, view_dir, fill_dir, vec3<f32>(0.65, 0.78, 1.05), diff_col, f0, alpha_r)
        + eval_direct_pbr(n, view_dir, rim_dir, vec3<f32>(1.10, 1.05, 1.25), diff_col, f0, alpha_r);

    let r_vec = reflect(-view_dir, n);
    let env_spec = sample_studio_ibl(r_vec, roughness);
    let env_diff = sample_studio_ibl(n, 0.95);
    let k_s = f_schlick_roughness(no_v, f0, roughness);
    let k_d = (vec3<f32>(1.0) - k_s) * (1.0 - metallic);
    let brdf_lut = env_brdf_approx(f0, roughness, no_v);

    let is_side = in.face_kind == 1u;
    let neon_emissive = base_rgb * select(0.42, 0.24, is_side) * ao;
    let fresnel_rim = mix(base_rgb, vec3<f32>(1.0, 0.96, 1.0), 0.65)
        * pow(1.0 - no_v, 2.6)
        * select(0.55, 0.85, is_side)
        * spec_ao;
    let ibl = k_d * diff_col * env_diff * ao + env_spec * brdf_lut * spec_ao * 0.65;

    let lit_rgb = max(neon_emissive + direct + ibl + fresnel_rim, vec3<f32>(0.0)) * hdr_intensity;
    return vec4<f32>(lit_rgb * alpha, alpha);
}
