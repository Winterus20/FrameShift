// FrameShift Faz 6.2 Shader: particle_field3d.wgsl
// Stateless, seek-independent analytical instanced 3D particles with DoF softness & 4 SDF shapes.
// Working format: Rgba16Float, Linear Rec.709, premultiplied alpha.

struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) quad_uv: vec2<f32>,
    @location(1) @interpolate(flat) particle_color: vec4<f32>,
    @location(2) @interpolate(flat) coc: f32,
    @location(3) @interpolate(flat) shape_id: u32,
}

// 176 bytes, 16-byte aligned.
struct ParticleField3DUniform {
    view_proj: mat4x4<f32>,
    cam_right: vec4<f32>,
    cam_up: vec4<f32>,
    // xyz = cam_pos, w = time_sec
    cam_pos_time: vec4<f32>,
    color_a: vec4<f32>,
    color_b: vec4<f32>,
    // x = speed, y = spread, z = size, w = seed_f32
    params0: vec4<f32>,
    // x = shape_f32 (0=bokeh, 1=star4, 2=diamond, 3=heart), y = dof_focal_dist, z = dof_blur_strength, w = count_f32
    params1: vec4<f32>,
}

@group(0) @binding(0)
var<uniform> u: ParticleField3DUniform;

fn pcg_hash(v_in: u32) -> u32 {
    let state = v_in * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

fn hash_f32(seed: u32, idx: u32, salt: u32) -> f32 {
    let h = pcg_hash(seed ^ (idx * 1664525u + salt * 1013904223u));
    return f32(h & 0x00FFFFFFu) / 16777216.0;
}

@vertex
fn vs_main(
    @builtin(vertex_index) vid: u32,
    @builtin(instance_index) iid: u32,
) -> VertexOutput {
    var quad_xy = vec2<f32>(-1.0, -1.0);
    switch (vid) {
        case 0u: { quad_xy = vec2<f32>(-1.0, -1.0); }
        case 1u: { quad_xy = vec2<f32>( 1.0, -1.0); }
        case 2u, 3u: { quad_xy = vec2<f32>(-1.0,  1.0); }
        case 4u: { quad_xy = vec2<f32>( 1.0, -1.0); }
        default: { quad_xy = vec2<f32>( 1.0,  1.0); }
    }

    let seed = u32(max(u.params0.w, 0.0));
    let speed = u.params0.x;
    let spread = max(u.params0.y, 0.05);
    let base_size = max(u.params0.z, 0.001);
    let time_sec = u.cam_pos_time.w;

    // Deterministic per-particle attributes in [0, 1).
    let hx = hash_f32(seed, iid, 1u) * 2.0 - 1.0;
    let hy = hash_f32(seed, iid, 2u) * 2.0 - 1.0;
    let hz = hash_f32(seed, iid, 3u) * 2.0 - 1.0;
    let h_phase = hash_f32(seed, iid, 4u) * 6.2831853;
    let h_size = 0.55 + hash_f32(seed, iid, 5u) * 0.90;
    let h_col = hash_f32(seed, iid, 6u);
    let h_spin = (hash_f32(seed, iid, 7u) - 0.5) * 2.4;

    // Analytical seek-independent 3D trajectory (wrapped drift + orbital curl).
    let t = time_sec * speed;
    let drift_y = fract(hy * 0.5 + 0.5 + t * 0.14) * 2.0 - 1.0;
    let pos_base = vec3<f32>(hx, drift_y, hz) * spread;
    let swirl = vec3<f32>(
        sin(t * 1.15 + h_phase) * 0.18,
        cos(t * 0.85 + h_phase * 1.3) * 0.12,
        cos(t * 1.05 + h_phase * 0.7) * 0.18,
    ) * spread;
    let world_center = pos_base + swirl;

    // Automatic Depth-of-Field softness based on distance to camera.
    let view_dist = length(world_center - u.cam_pos_time.xyz);
    let coc = abs(view_dist - u.params1.y) * max(u.params1.z, 0.0);

    // Subtle spin for star/diamond glints while keeping hearts upright.
    let shape_id = u32(max(round(u.params1.x), 0.0)) % 4u;
    let spin_angle = select(t * h_spin + h_phase, 0.0, shape_id == 3u);
    let cs = cos(spin_angle);
    let sn = sin(spin_angle);
    let rot_xy = vec2<f32>(
        quad_xy.x * cs - quad_xy.y * sn,
        quad_xy.x * sn + quad_xy.y * cs,
    );

    let dof_expand = 1.0 + clamp(coc, 0.0, 1.5) * 1.15;
    let radius = base_size * h_size * dof_expand;
    let world_pos = world_center
        + u.cam_right.xyz * (rot_xy.x * radius)
        + u.cam_up.xyz * (rot_xy.y * radius);

    // Edge fade near vertical wrap boundaries so particles never pop abruptly.
    let wrap_fade = smoothstep(1.0, 0.78, abs(drift_y));
    let p_col = mix(u.color_a, u.color_b, h_col);

    var out: VertexOutput;
    out.clip_pos = u.view_proj * vec4<f32>(world_pos, 1.0);
    out.quad_uv = quad_xy;
    out.particle_color = vec4<f32>(max(p_col.rgb, vec3<f32>(0.0)), clamp(p_col.a, 0.0, 1.0) * wrap_fade);
    out.coc = coc;
    out.shape_id = shape_id;
    return out;
}

fn sdf_bokeh_circle(p: vec2<f32>) -> f32 {
    return length(p) - 0.72;
}

fn sdf_star4(p_in: vec2<f32>) -> f32 {
    // 4-point anime glint star using rotated hyperbola-like pinched diamond.
    let p = abs(p_in);
    let d_cross = min(
        length(vec2<f32>(p.x * 3.4, p.y * 0.85)),
        length(vec2<f32>(p.x * 0.85, p.y * 3.4)),
    );
    let d_core = length(p) * 1.75;
    return min(d_cross, d_core) - 0.72;
}

fn sdf_diamond(p: vec2<f32>) -> f32 {
    let q = abs(p);
    return (q.x + q.y) * 0.78 - 0.62;
}

fn sdf_heart(p_in: vec2<f32>) -> f32 {
    // Flip Y so +Y billboard top renders the heart upright.
    var p = vec2<f32>(abs(p_in.x), p_in.y * 0.95 + 0.35) * 1.15;
    if (p.y + p.x > 1.0) {
        let d = p - vec2<f32>(0.25, 0.75);
        return sqrt(dot(d, d)) - 0.35355339;
    }
    let d1 = p - vec2<f32>(0.0, 1.0);
    let d2 = p - 0.5 * max(p.x + p.y, 0.0);
    return sqrt(min(dot(d1, d1), dot(d2, d2))) * sign(p.x - p.y);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    if (in.particle_color.a <= 0.001) {
        discard;
    }

    var d: f32 = 0.0;
    switch (in.shape_id) {
        case 0u: { d = sdf_bokeh_circle(in.quad_uv); }
        case 1u: { d = sdf_star4(in.quad_uv); }
        case 2u: { d = sdf_diamond(in.quad_uv); }
        default: { d = sdf_heart(in.quad_uv); }
    }

    let edge_soft = clamp(0.08 + in.coc * 0.38, 0.05, 0.65);
    let shape_mask = 1.0 - smoothstep(-edge_soft, edge_soft, d);
    if (shape_mask <= 0.001) {
        discard;
    }

    // Bokeh ring accent for shape 0, crisp HDR core glow (> 1.4 intensity) for all shapes.
    let radial = length(in.quad_uv);
    let core_glow = exp(-radial * radial * 5.2) * 1.65;
    let ring_boost = select(0.0, smoothstep(0.32, 0.62, radial) * 0.45, in.shape_id == 0u);
    let dof_atten = 1.0 / (1.0 + clamp(in.coc, 0.0, 2.0) * 0.45);

    let alpha = clamp(in.particle_color.a * shape_mask * dof_atten, 0.0, 1.0);
    let base_rgb = in.particle_color.rgb;
    let hdr_rgb = base_rgb * (0.95 + ring_boost) + mix(base_rgb, vec3<f32>(1.0), 0.55) * core_glow;

    return vec4<f32>(max(hdr_rgb, vec3<f32>(0.0)) * alpha, alpha);
}
