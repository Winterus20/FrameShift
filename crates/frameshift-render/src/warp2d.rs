use bytemuck::{Pod, Zeroable};
use frameshift_schema::SampleFilter;
use glam::{Mat3, Vec3};

/// Maximum control-mesh dimension supported by `grid_warp.wgsl` along either axis.
pub const MAX_GRID_WARP_DIM: u32 = 8;

/// Maximum number of control-mesh vertices packed into `GridWarpUniform::offsets`.
pub const MAX_GRID_WARP_POINTS: usize = 64;

const HOMOG_DET_EPSILON: f32 = 1e-8;

/// Uniform buffer for `corner_pin.wgsl` (64 bytes, 16-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct CornerPinUniform {
    /// Columns 0..2 of inverse homography `H_inv` stored as `[m0j, m1j, m2j, 0.0]`.
    pub inv_cols: [[f32; 4]; 3],
    /// `x` = opacity `[0..1]`, `y` = filter_mode (`0.0` = Bilinear, `1.0` = CatmullRom),
    /// `z` = valid (`1.0` or `0.0`), `w` = reserved.
    pub params: [f32; 4],
}

impl Default for CornerPinUniform {
    fn default() -> Self {
        Self::from_corners(
            [0.0, 0.0],
            [1.0, 0.0],
            [1.0, 1.0],
            [0.0, 1.0],
            SampleFilter::Bilinear,
            1.0,
        )
    }
}

impl CornerPinUniform {
    /// Builds a `CornerPinUniform` from normalized `[0, 1]` destination corner coordinates
    /// using Paul Heckbert's closed-form unit-square-to-quad homography and `glam::Mat3` inversion.
    pub fn from_corners(
        tl: [f32; 2],
        tr: [f32; 2],
        br: [f32; 2],
        bl: [f32; 2],
        filter: SampleFilter,
        opacity: f32,
    ) -> Self {
        let (inv_cols, valid) = compute_corner_pin_inv_homography(tl, tr, br, bl);
        Self {
            inv_cols,
            params: [
                sanitize_unit_clamp(opacity),
                sample_filter_f32(filter),
                if valid { 1.0 } else { 0.0 },
                0.0,
            ],
        }
    }
}

/// Uniform buffer for `grid_warp.wgsl` (1072 bytes, 16-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct GridWarpUniform {
    /// `x` = cols (`2..=8`), `y` = rows (`2..=8`), `z` = wave_phase, `w` = mix_amount `[0..1]`.
    pub grid_info: [f32; 4],
    /// `xy` = wave_amp, `zw` = wave_freq.
    pub wave_params: [f32; 4],
    /// `x` = opacity `[0..1]`, `y` = filter_mode (`0.0` = Bilinear, `1.0` = CatmullRom),
    /// `zw` = reserved.
    pub render_flags: [f32; 4],
    /// Up to `8 x 8 = 64` control point offsets `(dx, dy, 0.0, 0.0)`.
    pub offsets: [[f32; 4]; MAX_GRID_WARP_POINTS],
}

impl Default for GridWarpUniform {
    fn default() -> Self {
        Self::from_params(
            4,
            4,
            &[],
            [0.0, 0.0],
            [2.0, 2.0],
            0.0,
            1.0,
            SampleFilter::Bilinear,
            1.0,
        )
    }
}

impl GridWarpUniform {
    /// Packs `GridWarp` parameters and up to `8x8` control vertex offsets into 16-byte aligned slots.
    #[allow(clippy::too_many_arguments)]
    pub fn from_params(
        cols: u32,
        rows: u32,
        control_offsets: &[[f32; 2]],
        wave_amp: [f32; 2],
        wave_freq: [f32; 2],
        wave_phase: f32,
        mix_amount: f32,
        filter: SampleFilter,
        opacity: f32,
    ) -> Self {
        let clamped_cols = cols.clamp(2, MAX_GRID_WARP_DIM);
        let clamped_rows = rows.clamp(2, MAX_GRID_WARP_DIM);
        let active_points =
            (clamped_cols as usize * clamped_rows as usize).min(MAX_GRID_WARP_POINTS);

        let mut offsets = [[0.0_f32; 4]; MAX_GRID_WARP_POINTS];
        for (dst, src) in offsets
            .iter_mut()
            .zip(control_offsets.iter().take(active_points))
        {
            dst[0] = sanitize_finite(src[0]);
            dst[1] = sanitize_finite(src[1]);
        }

        Self {
            grid_info: [
                clamped_cols as f32,
                clamped_rows as f32,
                sanitize_finite(wave_phase),
                sanitize_unit_clamp(mix_amount),
            ],
            wave_params: [
                sanitize_finite(wave_amp[0]),
                sanitize_finite(wave_amp[1]),
                sanitize_finite(wave_freq[0]),
                sanitize_finite(wave_freq[1]),
            ],
            render_flags: [
                sanitize_unit_clamp(opacity),
                sample_filter_f32(filter),
                0.0,
                0.0,
            ],
            offsets,
        }
    }
}

/// Computes the inverse projective homography columns for mapping normalized destination UV
/// coordinates back to unit-square `[0, 1]^2` source UV coordinates.
///
/// Uses Paul Heckbert's closed-form unit-square-to-quad mapping (`tl`=(0,0), `tr`=(1,0),
/// `br`=(1,1), `bl`=(0,1)) and guards against singular, collinear, or non-finite quads.
pub fn compute_corner_pin_inv_homography(
    tl: [f32; 2],
    tr: [f32; 2],
    br: [f32; 2],
    bl: [f32; 2],
) -> ([[f32; 4]; 3], bool) {
    let coords = [tl[0], tl[1], tr[0], tr[1], br[0], br[1], bl[0], bl[1]];
    if !coords.into_iter().all(f32::is_finite) {
        return ([[0.0; 4]; 3], false);
    }

    let [x0, y0] = tl;
    let [x1, y1] = tr;
    let [x2, y2] = br;
    let [x3, y3] = bl;

    let dx1 = x1 - x2;
    let dx2 = x3 - x2;
    let sx = x0 - x1 + x2 - x3;

    let dy1 = y1 - y2;
    let dy2 = y3 - y2;
    let sy = y0 - y1 + y2 - y3;

    let det = dx1 * dy2 - dx2 * dy1;
    if !det.is_finite() || det.abs() <= HOMOG_DET_EPSILON {
        return ([[0.0; 4]; 3], false);
    }

    let g = (sx * dy2 - sy * dx2) / det;
    let h = (dx1 * sy - dy1 * sx) / det;
    if !g.is_finite() || !h.is_finite() {
        return ([[0.0; 4]; 3], false);
    }

    let a = x1 - x0 + g * x1;
    let d = y1 - y0 + g * y1;
    let b = x3 - x0 + h * x3;
    let e = y3 - y0 + h * y3;
    let c = x0;
    let f = y0;

    let h_fwd = Mat3::from_cols(
        Vec3::new(a, d, g),
        Vec3::new(b, e, h),
        Vec3::new(c, f, 1.0),
    );

    let Some(h_inv) = try_inverse_mat3(h_fwd) else {
        return ([[0.0; 4]; 3], false);
    };

    let cols = [
        [h_inv.x_axis.x, h_inv.x_axis.y, h_inv.x_axis.z, 0.0],
        [h_inv.y_axis.x, h_inv.y_axis.y, h_inv.y_axis.z, 0.0],
        [h_inv.z_axis.x, h_inv.z_axis.y, h_inv.z_axis.z, 0.0],
    ];
    (cols, true)
}

fn try_inverse_mat3(m: Mat3) -> Option<Mat3> {
    let det = m.determinant();
    if !det.is_finite() || det.abs() <= HOMOG_DET_EPSILON {
        return None;
    }
    let inv = m.inverse();
    if inv.is_finite() {
        Some(inv)
    } else {
        None
    }
}

fn sample_filter_f32(filter: SampleFilter) -> f32 {
    match filter {
        SampleFilter::Bilinear => 0.0,
        SampleFilter::CatmullRom => 1.0,
    }
}

fn sanitize_finite(v: f32) -> f32 {
    if v.is_finite() { v } else { 0.0 }
}

fn sanitize_unit_clamp(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

const _: () = {
    assert!(std::mem::size_of::<CornerPinUniform>() == 64);
    assert!(std::mem::size_of::<CornerPinUniform>().is_multiple_of(16));
    assert!(std::mem::size_of::<GridWarpUniform>() == 1072);
    assert!(std::mem::size_of::<GridWarpUniform>().is_multiple_of(16));
};

#[cfg(test)]
mod tests {
    use super::*;
    use wgpu::naga::front::wgsl;
    use wgpu::naga::valid::{Capabilities, ValidationFlags, Validator};

    const SHADER_CORNER_PIN: &str = include_str!("../shaders/corner_pin.wgsl");
    const SHADER_GRID_WARP: &str = include_str!("../shaders/grid_warp.wgsl");

    fn project_with_cols(cols: [[f32; 4]; 3], uv: [f32; 2]) -> [f32; 2] {
        let m = Mat3::from_cols(
            Vec3::new(cols[0][0], cols[0][1], cols[0][2]),
            Vec3::new(cols[1][0], cols[1][1], cols[1][2]),
            Vec3::new(cols[2][0], cols[2][1], cols[2][2]),
        );
        let p = m * Vec3::new(uv[0], uv[1], 1.0);
        assert!(p.z > 1e-6, "expected positive projective w, got {}", p.z);
        [p.x / p.z, p.y / p.z]
    }

    fn has_forbidden_loop_keyword(src: &str) -> bool {
        src.split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .any(|tok| tok == "loop" || tok == "while")
    }

    #[test]
    fn uniform_sizes_and_field_offsets_match_wgsl() {
        assert_eq!(std::mem::size_of::<CornerPinUniform>(), 64);
        assert_eq!(std::mem::offset_of!(CornerPinUniform, inv_cols), 0);
        assert_eq!(std::mem::offset_of!(CornerPinUniform, params), 48);

        assert_eq!(std::mem::size_of::<GridWarpUniform>(), 1072);
        assert_eq!(std::mem::offset_of!(GridWarpUniform, grid_info), 0);
        assert_eq!(std::mem::offset_of!(GridWarpUniform, wave_params), 16);
        assert_eq!(std::mem::offset_of!(GridWarpUniform, render_flags), 32);
        assert_eq!(std::mem::offset_of!(GridWarpUniform, offsets), 48);
    }

    #[test]
    fn corner_pin_identity_quad_yields_identity_homography() {
        let uniform = CornerPinUniform::default();
        assert_eq!(uniform.params, [1.0, 0.0, 1.0, 0.0]);

        let expected = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ];
        for (col_got, col_exp) in uniform.inv_cols.iter().zip(expected.iter()) {
            for (a, b) in col_got.iter().zip(col_exp.iter()) {
                assert!((a - b).abs() < 1e-6, "got {a}, expected {b}");
            }
        }
    }

    #[test]
    fn corner_pin_perspective_trapezoid_inverts_all_four_corners() {
        let tl = [0.18, 0.12];
        let tr = [0.84, 0.22];
        let br = [0.92, 0.88];
        let bl = [0.10, 0.78];

        let uniform = CornerPinUniform::from_corners(tl, tr, br, bl, SampleFilter::CatmullRom, 0.75);
        assert_eq!(uniform.params, [0.75, 1.0, 1.0, 0.0]);

        let mapped_tl = project_with_cols(uniform.inv_cols, tl);
        let mapped_tr = project_with_cols(uniform.inv_cols, tr);
        let mapped_br = project_with_cols(uniform.inv_cols, br);
        let mapped_bl = project_with_cols(uniform.inv_cols, bl);

        for (got, expected) in [
            (mapped_tl, [0.0_f32, 0.0]),
            (mapped_tr, [1.0, 0.0]),
            (mapped_br, [1.0, 1.0]),
            (mapped_bl, [0.0, 1.0]),
        ] {
            assert!(
                (got[0] - expected[0]).abs() < 1e-5 && (got[1] - expected[1]).abs() < 1e-5,
                "mapped {got:?} != {expected:?}"
            );
        }
    }

    #[test]
    fn corner_pin_degenerate_and_non_finite_quads_return_invalid() {
        // Collinear horizontal line
        let (_, valid_line) = compute_corner_pin_inv_homography(
            [0.0, 0.5],
            [0.3, 0.5],
            [0.7, 0.5],
            [1.0, 0.5],
        );
        assert!(!valid_line);

        // Coincident opposing corners (br == tl)
        let (_, valid_coincident) = compute_corner_pin_inv_homography(
            [0.0, 0.0],
            [1.0, 0.0],
            [0.0, 0.0],
            [0.0, 1.0],
        );
        assert!(!valid_coincident);

        // Non-finite corner
        let (_, valid_nan) = compute_corner_pin_inv_homography(
            [f32::NAN, 0.0],
            [1.0, 0.0],
            [1.0, 1.0],
            [0.0, 1.0],
        );
        assert!(!valid_nan);

        let invalid_uniform = CornerPinUniform::from_corners(
            [0.2, 0.2],
            [0.2, 0.2],
            [0.8, 0.8],
            [0.8, 0.8],
            SampleFilter::Bilinear,
            1.0,
        );
        assert_eq!(invalid_uniform.params[2], 0.0);
    }

    #[test]
    fn grid_warp_uniform_clamps_dims_pads_offsets_and_sanitizes() {
        let offsets = [
            [0.05, -0.02],
            [f32::NAN, 0.03],
            [-0.04, 0.01],
            [0.02, 0.02],
        ];
        let u = GridWarpUniform::from_params(
            1,
            12,
            &offsets,
            [0.02, -0.01],
            [3.0, 1.5],
            0.75,
            1.5,
            SampleFilter::CatmullRom,
            0.85,
        );

        assert_eq!(u.grid_info, [2.0, 8.0, 0.75, 1.0]);
        assert_eq!(u.wave_params, [0.02, -0.01, 3.0, 1.5]);
        assert_eq!(u.render_flags, [0.85, 1.0, 0.0, 0.0]);
        assert_eq!(u.offsets[0], [0.05, -0.02, 0.0, 0.0]);
        assert_eq!(u.offsets[1], [0.0, 0.03, 0.0, 0.0]);
        assert_eq!(u.offsets[2], [-0.04, 0.01, 0.0, 0.0]);
        assert_eq!(u.offsets[3], [0.02, 0.02, 0.0, 0.0]);
        assert_eq!(u.offsets[4], [0.0, 0.0, 0.0, 0.0]);
        assert_eq!(u.offsets[63], [0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn warp2d_shaders_pass_naga_parse_validation_and_loop_guard() {
        for (name, src) in [
            ("corner_pin.wgsl", SHADER_CORNER_PIN),
            ("grid_warp.wgsl", SHADER_GRID_WARP),
        ] {
            assert!(
                !has_forbidden_loop_keyword(src),
                "{name} must not contain unbounded loop/while keywords"
            );
            let module = wgsl::parse_str(src)
                .unwrap_or_else(|err| panic!("{name} failed WGSL parse: {}", err.emit_to_string(src)));
            let mut validator = Validator::new(ValidationFlags::all(), Capabilities::empty());
            validator
                .validate(&module)
                .unwrap_or_else(|err| panic!("{name} failed Naga validation: {err:?}"));
        }
    }
}
