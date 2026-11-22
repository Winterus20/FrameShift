// FrameShift Faz 6.6 Shader: ribbon_trail3d.wgsl
// Stateless analytical 3D Trim-Path neon spline ribbons (TriangleStrip, hue-preserving HDR core)
// with analytical Scene3D Depth-of-Field expansion and softening (immune to fx_dof_coc void artifacts).
// The core keeps the authored hue by default (core_whiten = 0); core/halo gain, saturation and a
// screen-space minimum width are uniform-driven. Additive blending is the default; normal
// premultiplied blending is opt-in via a separate pipeline (see pipelines.rs).
// Supports legacy Lissajous mode (ribbon_style=0), Triple Parallel Trim-Path Bundle
// (ribbon_style=1, monotonic cubic Bezier S-curve + Bishop-safe twist-free frame),
// and Triple Parallel Helix Arc (ribbon_style=2).
// Working format: Rgba16Float, Linear Rec.709, premultiplied alpha.

struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) ribbon_uv: vec2<f32>,
    @location(1) curve_s: f32,
    @location(2) @interpolate(flat) trim_span: f32,
    @location(3) coc: f32,
}

// 272 bytes, 16-byte aligned (first 240 bytes identical to Faz 6.3).
struct RibbonTrail3DUniform {
    view_proj: mat4x4<f32>,  //   0..64
    // xyz = cam_pos, w = time_sec
    cam_pos_time: vec4<f32>, //  64..80
    color_a: vec4<f32>,      //  80..96
    color_b: vec4<f32>,      //  96..112
    // x = width, y = speed, z = radius, w = seed_f32
    params0: vec4<f32>,      // 112..128
    // x = trim_start, y = trim_end, z = segments_f32, w = ribbon_count_f32
    params1: vec4<f32>,      // 128..144
    model: mat4x4<f32>,      // 144..208
    // x = ribbon_style (0=lissajous_legacy, 1=parallel_triple_bundle, 2=parallel_triple_helix),
    // y = strand_spacing, z = curve_bend, w = opacity
    params2: vec4<f32>,      // 208..224
    // x = dof_focal_dist, y = dof_blur_strength, z = max_world_expand, w = reserved
    dof_params: vec4<f32>,   // 224..240
    // x = min_width_px, y = viewport_w, z = viewport_h, w = saturation
    params3: vec4<f32>,      // 240..256
    // x = core_intensity, y = halo_intensity, z = core_whiten, w = reserved
    params4: vec4<f32>,      // 256..272
}

@group(0) @binding(0)
var<uniform> u: RibbonTrail3DUniform;

fn pcg_hash(v_in: u32) -> u32 {
    let state = v_in * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

fn hash_f32(seed: u32, idx: u32, salt: u32) -> f32 {
    let h = pcg_hash(seed ^ (idx * 1664525u + salt * 1013904223u));
    return f32(h & 0x00FFFFFFu) / 16777216.0;
}

fn safe_normalize(v: vec3<f32>, fallback: vec3<f32>) -> vec3<f32> {
    let len_sq = dot(v, v);
    if (len_sq <= 1e-10) {
        return fallback;
    }
    return v * inverseSqrt(len_sq);
}

fn eval_ribbon_point(s: f32, iid: u32, time_sec: f32, seed: u32, speed: f32, radius: f32) -> vec3<f32> {
    let h0 = hash_f32(seed, iid, 1u);
    let h1 = hash_f32(seed, iid, 2u);
    let h2 = hash_f32(seed, iid, 3u);
    let h3 = hash_f32(seed, iid, 4u);

    let freq_x = 1.0 + floor(h0 * 2.0);
    let freq_y = 2.0 + floor(h1 * 2.0);
    let freq_z = 1.0 + floor(h2 * 2.0);

    let phase_x = h0 * 6.2831853;
    let phase_y = h1 * 6.2831853;
    let phase_z = h2 * 6.2831853;
    let dir_sign = select(-1.0, 1.0, h3 >= 0.5);

    let theta = (s * 6.2831853 + time_sec * speed * 1.15 * dir_sign);
    let x = sin(theta * freq_x + phase_x) * radius * 1.25;
    let y = cos(theta * freq_y + phase_y) * radius * 0.65 + sin(theta + phase_z) * radius * 0.22;
    let z = cos(theta * freq_z + phase_z) * radius * 0.85;

    // Gentle per-ribbon tilt around X and Y so ribbons weave cleanly in 3D.
    let tilt = (h3 - 0.5) * 1.1;
    let ct = cos(tilt);
    let st = sin(tilt);
    let yr = y * ct - z * st;
    let zr = y * st + z * ct;

    return vec3<f32>(x, yr, zr);
}

fn eval_cubic_bezier(p0: vec3<f32>, p1: vec3<f32>, p2: vec3<f32>, p3: vec3<f32>, s: f32) -> vec3<f32> {
    let u1 = 1.0 - s;
    let u2 = u1 * u1;
    let s2 = s * s;
    return p0 * (u2 * u1) + p1 * (3.0 * u2 * s) + p2 * (3.0 * u1 * s2) + p3 * (s2 * s);
}

fn eval_cubic_bezier_deriv(p0: vec3<f32>, p1: vec3<f32>, p2: vec3<f32>, p3: vec3<f32>, s: f32) -> vec3<f32> {
    let u1 = 1.0 - s;
    return 3.0 * u1 * u1 * (p1 - p0) + 6.0 * u1 * s * (p2 - p1) + 3.0 * s * s * (p3 - p2);
}

fn rotate_x(v: vec3<f32>, angle: f32) -> vec3<f32> {
    let c = cos(angle);
    let s = sin(angle);
    return vec3<f32>(v.x, v.y * c - v.z * s, v.y * s + v.z * c);
}

@vertex
fn vs_main(
    @builtin(vertex_index) vid: u32,
    @builtin(instance_index) iid: u32,
) -> VertexOutput {
    let seg_idx = vid / 2u;
    let side = select(-1.0, 1.0, (vid & 1u) == 1u);
    let segments = max(u.params1.z, 1.0);
    let t_seg = clamp(f32(seg_idx) / segments, 0.0, 1.0);

    let trim_start = clamp(u.params1.x, 0.0, 1.0);
    let trim_end = clamp(u.params1.y, 0.0, 1.0);
    let trim_span = max(trim_end - trim_start, 0.0);

    // Map strip segments across [trim_start, trim_end] for crisp curve resolution.
    let s = mix(trim_start, trim_end, t_seg);

    let seed = u32(max(u.params0.w, 0.0));
    let width = max(u.params0.x, 0.0005);
    let speed = u.params0.y;
    let radius = max(u.params0.z, 0.05);
    let time_sec = u.cam_pos_time.w;
    let style = u32(clamp(round(u.params2.x), 0.0, 2.0));

    var pos = vec3<f32>(0.0);
    var tangent = vec3<f32>(1.0, 0.0, 0.0);

    if (style == 0u) {
        let ds = 0.0025;
        let local_pos = eval_ribbon_point(s, iid, time_sec, seed, speed, radius);
        let local_prev = eval_ribbon_point(s - ds, iid, time_sec, seed, speed, radius);
        let local_next = eval_ribbon_point(s + ds, iid, time_sec, seed, speed, radius);
        pos = (u.model * vec4<f32>(local_pos, 1.0)).xyz;
        let pos_prev = (u.model * vec4<f32>(local_prev, 1.0)).xyz;
        let pos_next = (u.model * vec4<f32>(local_next, 1.0)).xyz;
        tangent = safe_normalize(pos_next - pos_prev, vec3<f32>(1.0, 0.0, 0.0));
    } else {
        // Triple Parallel Trim-Path Bundle (style 1: monotonic cubic Bezier S-curve; style 2: helix arc).
        let strand_idx = iid % 3u;
        let bundle_idx = iid / 3u;
        let lane = f32(strand_idx) - 1.0;
        let spacing = select(max(u.params2.y, width * 1.35), width * 2.2, u.params2.y <= 0.0);
        let bend = clamp(u.params2.z, -2.5, 2.5);

        let bh0 = hash_f32(seed, bundle_idx, 10u);
        let bh1 = hash_f32(seed, bundle_idx, 11u);
        let bh2 = hash_f32(seed, bundle_idx, 12u);
        let bh3 = hash_f32(seed, bundle_idx, 13u);
        let t_flow = time_sec * speed;

        var center_local = vec3<f32>(0.0);
        var deriv_local = vec3<f32>(1.0, 0.0, 0.0);
        var u_ref = vec3<f32>(0.0, 0.0, 1.0);

        if (style == 1u) {
            // Strictly monotonic X progression guarantees B_x'(s) > 0 everywhere (zero Bishop flip).
            let sgn = select(-1.0, 1.0, bh0 >= 0.5);
            let amp_y = radius * (0.45 + 0.65 * bend);
            let amp_z = radius * (0.30 + 0.45 * bend);
            let wave1 = sin(t_flow * 1.1 + bh1 * 6.2831853) * 0.22 * radius;
            let wave2 = cos(t_flow * 0.9 + bh2 * 6.2831853) * 0.22 * radius;

            let p0 = vec3<f32>(-radius * 1.45, -amp_y * 0.45 * sgn, -amp_z * 0.35);
            let p1 = vec3<f32>(-radius * 0.48,  amp_y * sgn + wave1,  amp_z * 0.75 + wave2 * 0.5);
            let p2 = vec3<f32>( radius * 0.48, -amp_y * sgn - wave2, -amp_z * 0.65 + wave1 * 0.5);
            let p3 = vec3<f32>( radius * 1.45,  amp_y * 0.45 * sgn,  amp_z * 0.35);

            center_local = eval_cubic_bezier(p0, p1, p2, p3, s);
            deriv_local = eval_cubic_bezier_deriv(p0, p1, p2, p3, s);

            if (bundle_idx > 0u) {
                let b_tilt = (bh3 - 0.5) * 1.4;
                let b_shift = vec3<f32>(0.0, (bh0 - 0.5) * radius * 0.6, (bh2 - 0.5) * radius * 0.4);
                center_local = rotate_x(center_local, b_tilt) + b_shift;
                deriv_local = rotate_x(deriv_local, b_tilt);
                u_ref = rotate_x(u_ref, b_tilt);
            }
        } else {
            let arc_span = 4.4;
            let theta = mix(-2.2, 2.2, s) + t_flow * 0.85 + bh1 * 6.2831853;
            let rise = radius * (0.9 + bend);
            center_local = vec3<f32>(
                cos(theta) * radius,
                (s - 0.5) * rise + (bh0 - 0.5) * radius * 0.35,
                sin(theta) * radius,
            );
            deriv_local = vec3<f32>(
                -sin(theta) * arc_span * radius,
                rise,
                cos(theta) * arc_span * radius,
            );
            u_ref = vec3<f32>(0.0, 1.0, 0.0);
        }

        let t_local = safe_normalize(deriv_local, vec3<f32>(1.0, 0.0, 0.0));
        let b_local = safe_normalize(cross(t_local, u_ref), vec3<f32>(0.0, 1.0, 0.0));
        let n_local = safe_normalize(cross(b_local, t_local), vec3<f32>(0.0, 1.0, 0.0));

        let strand_local = center_local + n_local * (lane * spacing);
        pos = (u.model * vec4<f32>(strand_local, 1.0)).xyz;
        tangent = safe_normalize((u.model * vec4<f32>(t_local, 0.0)).xyz, vec3<f32>(1.0, 0.0, 0.0));
    }

    let view_vec = safe_normalize(u.cam_pos_time.xyz - pos, vec3<f32>(0.0, 0.0, 1.0));
    var side_dir = cross(tangent, view_vec);
    if (dot(side_dir, side_dir) <= 1e-8) {
        side_dir = cross(tangent, vec3<f32>(0.0, 1.0, 0.0));
    }
    side_dir = safe_normalize(side_dir, vec3<f32>(0.0, 1.0, 0.0));

    // Analytical Scene3D DoF expansion along view distance (0 when dof_blur_strength <= 0).
    let view_dist = length(pos - u.cam_pos_time.xyz);
    let max_expand = select(u.dof_params.z, 1.5, u.dof_params.z <= 0.0);
    let ribbon_coc = clamp(abs(view_dist - u.dof_params.x) * max(u.dof_params.y, 0.0), 0.0, max_expand);

    // Taper smoothly toward trim endpoints so neon trails have sleek tips.
    let tip_taper = pow(max(sin(t_seg * 3.14159265), 0.0), 0.35);
    let half_w = width * 0.5 * tip_taper * (1.0 + ribbon_coc * 1.35);
    var world_pos = pos + side_dir * (side * half_w);

    // Screen-space floor: project the center and the offset edge, measure the pixel
    // width, and widen ribbons that would otherwise collapse into a 1px wire.
    let min_width_px = max(u.params3.x, 0.0);
    let viewport = u.params3.yz;
    if (min_width_px > 0.0 && viewport.x > 0.0 && viewport.y > 0.0) {
        let clip_center = u.view_proj * vec4<f32>(pos, 1.0);
        let clip_edge = u.view_proj * vec4<f32>(pos + side_dir * half_w, 1.0);
        if (clip_center.w > 1e-4 && clip_edge.w > 1e-4) {
            let ndc_center = clip_center.xy / clip_center.w;
            let ndc_edge = clip_edge.xy / clip_edge.w;
            // NDC spans 2 units over the viewport, so the delta of a half-width
            // offset scaled by viewport size is the full ribbon width in pixels.
            let width_px = length((ndc_edge - ndc_center) * viewport);
            if (width_px > 1e-5 && width_px < min_width_px) {
                world_pos = pos + side_dir * (side * half_w * (min_width_px / width_px));
            }
        }
    }

    var out: VertexOutput;
    out.clip_pos = u.view_proj * vec4<f32>(world_pos, 1.0);
    out.ribbon_uv = vec2<f32>(t_seg, side);
    out.curve_s = s;
    out.trim_span = trim_span;
    out.coc = ribbon_coc;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    if (in.trim_span <= 1e-4) {
        discard;
    }

    let trim_start = clamp(u.params1.x, 0.0, 1.0);
    let trim_end = clamp(u.params1.y, 0.0, 1.0);
    if (in.curve_s < trim_start || in.curve_s > trim_end) {
        discard;
    }

    let t_seg = clamp(in.ribbon_uv.x, 0.0, 1.0);
    let cross_u = clamp(in.ribbon_uv.y, -1.0, 1.0);
    let dof_soft = clamp(in.coc * 0.65, 0.0, 1.0);

    // Smooth end fade along the trimmed arc + bright neon HDR core (> 1.5 intensity).
    let end_fade = smoothstep(0.0, 0.08, t_seg) * smoothstep(1.0, 0.92, t_seg);
    let halo = pow(max(1.0 - abs(cross_u), 0.0), mix(1.6, 0.85, dof_soft));
    let core = exp(-cross_u * cross_u * mix(7.5, 2.2, dof_soft));
    let dof_atten = 1.0 / (1.0 + clamp(in.coc, 0.0, 2.0) * 0.65);

    let base_col = mix(u.color_a, u.color_b, t_seg);
    let opacity = clamp(u.params2.w, 0.0, 1.0);
    let alpha = clamp(base_col.a * halo * end_fade * opacity * dof_atten, 0.0, 1.0);
    if (alpha <= 0.001) {
        discard;
    }

    // Saturation is applied to the authored color before any HDR gain: desaturating
    // after clipping would converge channels and whiten the core instead.
    let base_linear = max(base_col.rgb, vec3<f32>(0.0));
    let luma = dot(base_linear, vec3<f32>(0.2126, 0.7152, 0.0722));
    let saturation = clamp(u.params3.w, 0.0, 4.0);
    let base_rgb = mix(vec3<f32>(luma), base_linear, saturation);

    let core_intensity = max(u.params4.x, 0.0);
    let halo_intensity = max(u.params4.y, 0.0);
    let core_whiten = clamp(u.params4.z, 0.0, 1.0);

    // Hue-preserving HDR core: white is mixed in only when core_whiten > 0.
    let halo_rgb = base_rgb * (halo * halo_intensity * 0.45);
    let core_rgb = mix(base_rgb, vec3<f32>(1.0), core_whiten) * (core * core_intensity);
    let hdr_rgb = base_rgb * 1.15 + halo_rgb + core_rgb;
    return vec4<f32>(max(hdr_rgb, vec3<f32>(0.0)) * alpha, alpha);
}
