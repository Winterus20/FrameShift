// FrameShift Faz 6.5 Shader: text_slug.wgsl
// GPU-native quadratic Bezier glyph rendering (Eric Lengyel Slug algorithm, MIT / Public Domain)
// with dynamic screen-space dilation, horizontal/vertical band acceleration, and HDR Linear Rec.709 gradient fill.

struct SlugUniform {
    // x = canvas_w, y = canvas_h, z = anchor_x_px, w = block_center_y_px
    canvas_anchor: vec4<f32>,
    // xy = block_min (Y-up), zw = block_max (Y-up)
    block_bounds: vec4<f32>,
    color_top: vec4<f32>,
    color_bottom: vec4<f32>,
    // x = gradient_angle_rad, y = hdr_intensity, z = opacity, w = dilate_px
    style_params: vec4<f32>,
    // x = enabled, y = unit (0 grapheme, 1 word), z = range start, w = end-exclusive
    selector: vec4<u32>,
    // x = current time, y = start, z = duration, w = per-unit delay (seconds)
    timing_seconds: vec4<f32>,
    // x/y = initial position (canvas-height units), z = initial scale, w = initial opacity
    from_style: vec4<f32>,
}

struct SlugGlyph {
    bounds: vec4<f32>,
    band_scale_offset: vec4<f32>,
    hband_base: u32,
    vband_base: u32,
    hband_count: u32,
    vband_count: u32,
    unit_ranges: vec4<u32>,
}

struct SlugBand {
    curve_offset: u32,
    curve_count: u32,
    _pad0: u32,
    _pad1: u32,
}

struct SlugCurve {
    p0_p1: vec4<f32>,
    p2_pad: vec4<f32>,
}

@group(0) @binding(0)
var<uniform> u: SlugUniform;
@group(0) @binding(1)
var<storage, read> glyphs: array<SlugGlyph>;
@group(0) @binding(2)
var<storage, read> hbands: array<SlugBand>;
@group(0) @binding(3)
var<storage, read> vbands: array<SlugBand>;
@group(0) @binding(4)
var<storage, read> band_indices: array<u32>;
@group(0) @binding(5)
var<storage, read> curves: array<SlugCurve>;

struct VertexInput {
    @location(0) pos: vec2<f32>,
    @location(1) em_pos: vec2<f32>,
    @location(2) normal_dir: vec2<f32>,
    @location(3) uv: vec2<f32>,
    @location(4) glyph_index: u32,
}

struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) em_pos: vec2<f32>,
    @location(1) block_uv: vec2<f32>,
    @location(2) @interpolate(flat) glyph_index: u32,
    @location(3) @interpolate(flat) animation_opacity: f32,
}

fn glyph_animation(glyph: SlugGlyph) -> vec4<f32> {
    if (u.selector.x == 0u) {
        return vec4<f32>(0.0, 0.0, 1.0, 1.0);
    }

    var unit_start = glyph.unit_ranges.x;
    var unit_end = glyph.unit_ranges.y;
    if (u.selector.y == 1u) {
        unit_start = glyph.unit_ranges.z;
        unit_end = glyph.unit_ranges.w;
    }
    let selected = unit_end > unit_start
        && unit_end > u.selector.z
        && unit_start < u.selector.w;
    if (!selected) {
        return vec4<f32>(0.0, 0.0, 1.0, 1.0);
    }

    let selected_start = max(unit_start, u.selector.z);
    let stagger = f32(selected_start - u.selector.z) * max(u.timing_seconds.w, 0.0);
    let elapsed = u.timing_seconds.x - (u.timing_seconds.y + stagger);
    var progress = 0.0;
    if (u.timing_seconds.z > 0.0) {
        progress = clamp(elapsed / u.timing_seconds.z, 0.0, 1.0);
    } else if (elapsed >= 0.0) {
        progress = 1.0;
    }
    let from_amount = 1.0 - progress;
    return vec4<f32>(
        u.from_style.x * from_amount,
        u.from_style.y * from_amount,
        mix(1.0, u.from_style.z, from_amount),
        mix(1.0, u.from_style.w, from_amount),
    );
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    let dilate = max(u.style_params.w, 0.75);
    let glyph = glyphs[in.glyph_index];
    let animation = glyph_animation(glyph);
    let center = (glyph.bounds.xy + glyph.bounds.zw) * 0.5;
    let shift = vec2<f32>(animation.x, -animation.y) * u.canvas_anchor.y;
    let dilated = center + (in.pos - center) * animation.z + shift + in.normal_dir * dilate;

    let canvas_x = u.canvas_anchor.z + dilated.x;
    let canvas_y_down = u.canvas_anchor.w - dilated.y;
    let ndc_x = (canvas_x / max(u.canvas_anchor.x, 1.0)) * 2.0 - 1.0;
    let ndc_y = 1.0 - (canvas_y_down / max(u.canvas_anchor.y, 1.0)) * 2.0;

    let b_span = max(u.block_bounds.zw - u.block_bounds.xy, vec2<f32>(1e-4));
    let block_uv = (dilated - u.block_bounds.xy) / b_span;

    var out: VertexOutput;
    out.clip_pos = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    out.em_pos = in.em_pos + in.normal_dir * dilate;
    out.block_uv = block_uv;
    out.glyph_index = in.glyph_index;
    out.animation_opacity = animation.w;
    return out;
}

fn calc_root_code(y0: f32, y1: f32, y2: f32) -> u32 {
    let i0 = select(0u, 2u, y0 > 0.0);
    let i1 = select(0u, 4u, y1 > 0.0);
    let i2 = select(0u, 8u, y2 > 0.0);
    let shift = i0 + i1 + i2;
    return (0x2E74u >> shift) & 3u;
}

fn solve_horiz_poly(p0: vec2<f32>, p1: vec2<f32>, p2: vec2<f32>) -> vec2<f32> {
    let a = p0 - 2.0 * p1 + p2;
    let b = p0 - p1;
    let d = sqrt(max(b.y * b.y - a.y * p0.y, 0.0));
    var t1: f32;
    var t2: f32;
    if (abs(a.y) < 1e-4) {
        let t = p0.y / select(2.0 * b.y, 1e-6, abs(b.y) < 1e-7);
        t1 = t;
        t2 = t;
    } else {
        let ra = 1.0 / a.y;
        t1 = (b.y - d) * ra;
        t2 = (b.y + d) * ra;
    }
    return vec2<f32>(
        (a.x * t1 - 2.0 * b.x) * t1 + p0.x,
        (a.x * t2 - 2.0 * b.x) * t2 + p0.x,
    );
}

fn solve_vert_poly(p0: vec2<f32>, p1: vec2<f32>, p2: vec2<f32>) -> vec2<f32> {
    let a = p0 - 2.0 * p1 + p2;
    let b = p0 - p1;
    let d = sqrt(max(b.x * b.x - a.x * p0.x, 0.0));
    var t1: f32;
    var t2: f32;
    if (abs(a.x) < 1e-4) {
        let t = p0.x / select(2.0 * b.x, 1e-6, abs(b.x) < 1e-7);
        t1 = t;
        t2 = t;
    } else {
        let ra = 1.0 / a.x;
        t1 = (b.x - d) * ra;
        t2 = (b.x + d) * ra;
    }
    return vec2<f32>(
        (a.y * t1 - 2.0 * b.y) * t1 + p0.y,
        (a.y * t2 - 2.0 * b.y) * t2 + p0.y,
    );
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let em_coord = in.em_pos;
    let pixels_per_em = vec2<f32>(1.0) / max(fwidth(em_coord), vec2<f32>(1e-5));
    let glyph = glyphs[in.glyph_index];

    let vband_idx = u32(clamp(
        floor(em_coord.x * glyph.band_scale_offset.x + glyph.band_scale_offset.z),
        0.0,
        f32(max(glyph.vband_count, 1u) - 1u),
    ));
    let hband_idx = u32(clamp(
        floor(em_coord.y * glyph.band_scale_offset.y + glyph.band_scale_offset.w),
        0.0,
        f32(max(glyph.hband_count, 1u) - 1u),
    ));

    let hband = hbands[glyph.hband_base + hband_idx];
    let vband = vbands[glyph.vband_base + vband_idx];

    var xcov = 0.0;
    var xwgt = 0.0;
    for (var i = 0u; i < hband.curve_count; i = i + 1u) {
        let c_idx = band_indices[hband.curve_offset + i];
        let c = curves[c_idx];
        let p0 = c.p0_p1.xy - em_coord;
        let p1 = c.p0_p1.zw - em_coord;
        let p2 = c.p2_pad.xy - em_coord;

        if (max(max(p0.x, p1.x), p2.x) * pixels_per_em.x < -0.5) {
            break;
        }
        let code = calc_root_code(p0.y, p1.y, p2.y);
        if (code != 0u) {
            let r = solve_horiz_poly(p0, p1, p2) * pixels_per_em.x;
            if ((code & 1u) != 0u) {
                xcov += clamp(r.x + 0.5, 0.0, 1.0);
                xwgt = max(xwgt, clamp(1.0 - abs(r.x) * 2.0, 0.0, 1.0));
            }
            if ((code & 2u) != 0u) {
                xcov -= clamp(r.y + 0.5, 0.0, 1.0);
                xwgt = max(xwgt, clamp(1.0 - abs(r.y) * 2.0, 0.0, 1.0));
            }
        }
    }

    var ycov = 0.0;
    var ywgt = 0.0;
    for (var i = 0u; i < vband.curve_count; i = i + 1u) {
        let c_idx = band_indices[vband.curve_offset + i];
        let c = curves[c_idx];
        let p0 = c.p0_p1.xy - em_coord;
        let p1 = c.p0_p1.zw - em_coord;
        let p2 = c.p2_pad.xy - em_coord;

        if (max(max(p0.y, p1.y), p2.y) * pixels_per_em.y < -0.5) {
            break;
        }
        let code = calc_root_code(p0.x, p1.x, p2.x);
        if (code != 0u) {
            let r = solve_vert_poly(p0, p1, p2) * pixels_per_em.y;
            if ((code & 1u) != 0u) {
                ycov -= clamp(r.x + 0.5, 0.0, 1.0);
                ywgt = max(ywgt, clamp(1.0 - abs(r.x) * 2.0, 0.0, 1.0));
            }
            if ((code & 2u) != 0u) {
                ycov += clamp(r.y + 0.5, 0.0, 1.0);
                ywgt = max(ywgt, clamp(1.0 - abs(r.y) * 2.0, 0.0, 1.0));
            }
        }
    }

    let w_sum = xwgt + ywgt;
    var cov = 0.0;
    if (w_sum > 1e-5) {
        cov = clamp(abs(xcov * xwgt + ycov * ywgt) / w_sum, 0.0, 1.0);
    } else {
        cov = clamp(abs(xcov + ycov) * 0.5, 0.0, 1.0);
    }

    if (cov <= 1e-4) {
        discard;
    }

    // Angled HDR gradient in block_uv space (Y-up: block_uv.y = 1.0 at top, 0.0 at bottom).
    let ang = u.style_params.x;
    let dir = vec2<f32>(sin(ang), cos(ang));
    let centered_uv = in.block_uv - vec2<f32>(0.5, 0.5);
    let grad_t = clamp(dot(centered_uv, dir) + 0.5, 0.0, 1.0);
    let fill = mix(u.color_bottom, u.color_top, grad_t);
    let hdr = max(u.style_params.y, 0.0);
    let op = clamp(u.style_params.z, 0.0, 1.0);

    let alpha = clamp(fill.a * cov * op * in.animation_opacity, 0.0, 1.0);
    let rgb = max(fill.rgb, vec3<f32>(0.0)) * hdr * alpha;
    return vec4<f32>(rgb, alpha);
}
