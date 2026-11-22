use crate::error::RenderError;
use crate::lut::{CubeLut1D, CubeLut3D, MAX_LUT_3D_SIZE};
use frameshift_schema::{ColorSpace, ExtendedColorSpace};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Built-in analytic color spaces supported by `OcioColorPipeline`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OcioSpace {
    LinearRec709,
    DisplaySrgb,
    Rec709,
    LinearRec2020,
    Rec2020,
    Aces2065_1,
    AcesCg,
    AcesCct,
    ArriLogC3,
    SonySLog3,
}

impl From<ColorSpace> for OcioSpace {
    fn from(value: ColorSpace) -> Self {
        match value {
            ColorSpace::LinearRec709 => Self::LinearRec709,
            ColorSpace::DisplaySrgb => Self::DisplaySrgb,
        }
    }
}

impl From<ExtendedColorSpace> for OcioSpace {
    fn from(value: ExtendedColorSpace) -> Self {
        match value {
            ExtendedColorSpace::LinearRec709 => Self::LinearRec709,
            ExtendedColorSpace::DisplaySrgb => Self::DisplaySrgb,
            ExtendedColorSpace::Rec709 => Self::Rec709,
            ExtendedColorSpace::LinearRec2020 => Self::LinearRec2020,
            ExtendedColorSpace::Rec2020 => Self::Rec2020,
            ExtendedColorSpace::Aces2065_1 => Self::Aces2065_1,
            ExtendedColorSpace::AcesCg => Self::AcesCg,
            ExtendedColorSpace::AcesCct => Self::AcesCct,
            ExtendedColorSpace::ArriLogC3 => Self::ArriLogC3,
            ExtendedColorSpace::SonySLog3 => Self::SonySLog3,
        }
    }
}

impl From<OcioSpace> for ExtendedColorSpace {
    fn from(value: OcioSpace) -> Self {
        match value {
            OcioSpace::LinearRec709 => Self::LinearRec709,
            OcioSpace::DisplaySrgb => Self::DisplaySrgb,
            OcioSpace::Rec709 => Self::Rec709,
            OcioSpace::LinearRec2020 => Self::LinearRec2020,
            OcioSpace::Rec2020 => Self::Rec2020,
            OcioSpace::Aces2065_1 => Self::Aces2065_1,
            OcioSpace::AcesCg => Self::AcesCg,
            OcioSpace::AcesCct => Self::AcesCct,
            OcioSpace::ArriLogC3 => Self::ArriLogC3,
            OcioSpace::SonySLog3 => Self::SonySLog3,
        }
    }
}

impl OcioSpace {
    /// Parses common OCIO / FrameShift color space names and aliases.
    pub fn from_name(name: &str) -> Option<Self> {
        let key = name.trim().to_ascii_lowercase().replace(['-', ' '], "_");
        match key.as_str() {
            "linear_rec709" | "lin_rec709" | "linear_srgb" | "lin_srgb" | "linear" => {
                Some(Self::LinearRec709)
            }
            "display_srgb" | "srgb" | "srgb_texture" => Some(Self::DisplaySrgb),
            "rec709" | "bt709" | "rec_709" | "bt_709" => Some(Self::Rec709),
            "linear_rec2020" | "lin_rec2020" | "linear_bt2020" => Some(Self::LinearRec2020),
            "rec2020" | "bt2020" | "rec_2020" | "bt_2020" => Some(Self::Rec2020),
            "aces2065_1" | "aces_2065_1" | "aces" | "ap0" => Some(Self::Aces2065_1),
            "acescg" | "aces_cg" | "ap1" => Some(Self::AcesCg),
            "acescct" | "aces_cct" => Some(Self::AcesCct),
            "arri_logc3" | "logc3" | "arri_logc" | "alexa_logc3" | "awg3_logc3" => {
                Some(Self::ArriLogC3)
            }
            "sony_slog3" | "slog3" | "s_log3" | "sgamut3_slog3" => Some(Self::SonySLog3),
            _ => None,
        }
    }
}

// -----------------------------------------------------------------------------
// Verified f64 color-science matrices (row-major) and transfer functions
// -----------------------------------------------------------------------------

/// ACES2065-1 (AP0, D60) -> ACEScg (AP1, D60)
pub const AP0_TO_AP1_MAT: [[f64; 3]; 3] = [
    [1.4514393161, -0.2365107469, -0.2149285693],
    [-0.0765537734, 1.1762296998, -0.0996759264],
    [0.0083161484, -0.0060324498, 0.9977163014],
];

/// ACEScg (AP1, D60) -> ACES2065-1 (AP0, D60)
pub const AP1_TO_AP0_MAT: [[f64; 3]; 3] = [
    [0.6954522414, 0.1406786965, 0.1638690622],
    [0.0447945634, 0.8596711185, 0.0955343182],
    [-0.0055258826, 0.0040252103, 1.0015006723],
];

/// ACEScg (AP1, D60) -> Linear Rec.709 (D65) with Bradford chromatic adaptation
pub const AP1_TO_LINEAR_REC709_MAT: [[f64; 3]; 3] = [
    [1.70505099, -0.62179212, -0.08325887],
    [-0.13025642, 1.14080474, -0.01054832],
    [-0.02400336, -0.12896898, 1.15297233],
];

/// Linear Rec.709 (D65) -> ACEScg (AP1, D60) with Bradford chromatic adaptation
pub const LINEAR_REC709_TO_AP1_MAT: [[f64; 3]; 3] = [
    [0.61309740, 0.33952315, 0.04737945],
    [0.07019372, 0.91635388, 0.01345240],
    [0.02061559, 0.10956977, 0.86981463],
];

/// Linear Rec.2020 (D65) -> Linear Rec.709 (D65)
pub const LINEAR_REC2020_TO_LINEAR_REC709_MAT: [[f64; 3]; 3] = [
    [1.66049100, -0.58764114, -0.07284986],
    [-0.12455047, 1.13289990, -0.00834942],
    [-0.01815076, -0.10057890, 1.11872966],
];

/// ARRI Wide Gamut 3 (D65) -> Linear Rec.709 (D65)
pub const AWG3_TO_LINEAR_REC709_MAT: [[f64; 3]; 3] = [
    [1.61752344, -0.53728662, -0.08023681],
    [-0.07057274, 1.33461306, -0.26404032],
    [-0.02110173, -0.22695388, 1.24805560],
];

/// Sony S-Gamut3 (D65) -> Linear Rec.709 (D65)
pub const SGAMUT3_TO_LINEAR_REC709_MAT: [[f64; 3]; 3] = [
    [1.87791513, -0.79416876, -0.08374637],
    [-0.17680698, 1.35099962, -0.17419264],
    [-0.02620113, -0.14842226, 1.17462339],
];

#[inline]
fn mul_mat3_f64(m: &[[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

fn invert_mat3_f64(m: &[[f64; 3]; 3]) -> Option<[[f64; 3]; 3]> {
    let c00 = m[1][1] * m[2][2] - m[1][2] * m[2][1];
    let c01 = -(m[1][0] * m[2][2] - m[1][2] * m[2][0]);
    let c02 = m[1][0] * m[2][1] - m[1][1] * m[2][0];
    let det = m[0][0] * c00 + m[0][1] * c01 + m[0][2] * c02;
    if det.abs() < 1e-15 {
        return None;
    }
    let inv_det = 1.0 / det;
    let c10 = -(m[0][1] * m[2][2] - m[0][2] * m[2][1]);
    let c11 = m[0][0] * m[2][2] - m[0][2] * m[2][0];
    let c12 = -(m[0][0] * m[2][1] - m[0][1] * m[2][0]);
    let c20 = m[0][1] * m[1][2] - m[0][2] * m[1][1];
    let c21 = -(m[0][0] * m[1][2] - m[0][2] * m[1][0]);
    let c22 = m[0][0] * m[1][1] - m[0][1] * m[1][0];
    Some([
        [c00 * inv_det, c10 * inv_det, c20 * inv_det],
        [c01 * inv_det, c11 * inv_det, c21 * inv_det],
        [c02 * inv_det, c12 * inv_det, c22 * inv_det],
    ])
}

fn invert_mat4_f64(m: &[f64; 16]) -> Option<[f64; 16]> {
    // Gauss-Jordan elimination with partial pivoting on 4x4 row-major matrix
    let mut a = [[0.0_f64; 8]; 4];
    for r in 0..4 {
        for c in 0..4 {
            a[r][c] = m[r * 4 + c];
            a[r][c + 4] = if r == c { 1.0 } else { 0.0 };
        }
    }
    for col in 0..4 {
        let mut pivot_row = col;
        let mut max_val = a[col][col].abs();
        for (r, row) in a.iter().enumerate().skip(col + 1) {
            if row[col].abs() > max_val {
                max_val = row[col].abs();
                pivot_row = r;
            }
        }
        if max_val < 1e-15 {
            return None;
        }
        if pivot_row != col {
            a.swap(col, pivot_row);
        }
        let diag = a[col][col];
        for cell in &mut a[col] {
            *cell /= diag;
        }
        for r in 0..4 {
            if r == col {
                continue;
            }
            let factor = a[r][col];
            if factor != 0.0 {
                let pivot_row_vals = a[col];
                for (dst, &src) in a[r].iter_mut().zip(pivot_row_vals.iter()) {
                    *dst -= factor * src;
                }
            }
        }
    }
    let mut out = [0.0_f64; 16];
    for r in 0..4 {
        for c in 0..4 {
            out[r * 4 + c] = a[r][c + 4];
        }
    }
    Some(out)
}

/// IEC 61966-2-1 sRGB EOTF (`sRGB -> Linear`).
#[inline]
pub fn srgb_to_linear_f64(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// IEC 61966-2-1 sRGB OETF (`Linear -> sRGB`).
#[inline]
pub fn linear_to_srgb_f64(x: f64) -> f64 {
    if x <= 0.0031308 {
        12.92 * x
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    }
}

/// ITU-R BT.709 OETF (`Linear -> Rec.709`).
#[inline]
pub fn linear_to_rec709_f64(x: f64) -> f64 {
    if x < 0.018 {
        4.5 * x
    } else {
        1.099 * x.powf(0.45) - 0.099
    }
}

/// ITU-R BT.709 inverse OETF (`Rec.709 -> Linear`).
#[inline]
pub fn rec709_to_linear_f64(v: f64) -> f64 {
    if v < 0.081 {
        v / 4.5
    } else {
        ((v + 0.099) / 1.099).powf(1.0 / 0.45)
    }
}

const BT2020_ALPHA: f64 = 1.09929682680944;
const BT2020_BETA: f64 = 0.018053968510807;

/// ITU-R BT.2020 SDR OETF (`Linear -> Rec.2020`).
#[inline]
pub fn linear_to_rec2020_f64(x: f64) -> f64 {
    if x < BT2020_BETA {
        4.5 * x
    } else {
        BT2020_ALPHA * x.powf(0.45) - (BT2020_ALPHA - 1.0)
    }
}

/// ITU-R BT.2020 SDR inverse OETF (`Rec.2020 -> Linear`).
#[inline]
pub fn rec2020_to_linear_f64(v: f64) -> f64 {
    if v < 4.5 * BT2020_BETA {
        v / 4.5
    } else {
        ((v + (BT2020_ALPHA - 1.0)) / BT2020_ALPHA).powf(1.0 / 0.45)
    }
}

const ACESCCT_X_BREAK: f64 = 0.0078125;
const ACESCCT_A: f64 = 10.5402377416545;
const ACESCCT_B: f64 = 0.0729055341958355;
const ACESCCT_Y_BREAK: f64 = ACESCCT_A * ACESCCT_X_BREAK + ACESCCT_B;

/// Academy S-2016-001 (`Linear AP1 -> ACEScct`).
#[inline]
pub fn linear_ap1_to_acescct_f64(x: f64) -> f64 {
    if x <= ACESCCT_X_BREAK {
        ACESCCT_A * x + ACESCCT_B
    } else {
        (x.log2() + 9.72) / 17.52
    }
}

/// Academy S-2016-001 inverse (`ACEScct -> Linear AP1`).
#[inline]
pub fn acescct_to_linear_ap1_f64(y: f64) -> f64 {
    if y <= ACESCCT_Y_BREAK {
        (y - ACESCCT_B) / ACESCCT_A
    } else {
        (17.52 * y - 9.72).exp2()
    }
}

const LOGC3_CUT: f64 = 0.010591;
const LOGC3_A: f64 = 5.555556;
const LOGC3_B: f64 = 0.052272;
const LOGC3_C: f64 = 0.247190;
const LOGC3_D: f64 = 0.385537;
const LOGC3_E: f64 = 5.367655;
const LOGC3_F: f64 = 0.092809;
const LOGC3_CUT_Y: f64 = LOGC3_E * LOGC3_CUT + LOGC3_F;

/// ARRI ALEXA LogC3 EI 800 (`Linear AWG3 -> LogC3`).
#[inline]
pub fn linear_awg3_to_logc3_f64(x: f64) -> f64 {
    if x > LOGC3_CUT {
        LOGC3_C * (LOGC3_A * x + LOGC3_B).log10() + LOGC3_D
    } else {
        LOGC3_E * x + LOGC3_F
    }
}

/// ARRI ALEXA LogC3 EI 800 inverse (`LogC3 -> Linear AWG3`).
#[inline]
pub fn logc3_to_linear_awg3_f64(y: f64) -> f64 {
    if y > LOGC3_CUT_Y {
        (10.0_f64.powf((y - LOGC3_D) / LOGC3_C) - LOGC3_B) / LOGC3_A
    } else {
        (y - LOGC3_F) / LOGC3_E
    }
}

const SLOG3_X_BREAK: f64 = 0.01125000;
const SLOG3_C_LOW: f64 = 171.2102946929;
const SLOG3_Y_BREAK: f64 = SLOG3_C_LOW / 1023.0;

/// Sony S-Log3 (`Linear S-Gamut3 -> normalized S-Log3`).
#[inline]
pub fn linear_sgamut3_to_slog3_f64(x: f64) -> f64 {
    if x >= SLOG3_X_BREAK {
        (420.0 + ((x + 0.01) / (0.18 + 0.01)).log10() * 261.5) / 1023.0
    } else {
        (x * ((SLOG3_C_LOW - 95.0) / SLOG3_X_BREAK) + 95.0) / 1023.0
    }
}

/// Sony S-Log3 inverse (`normalized S-Log3 -> Linear S-Gamut3`).
#[inline]
pub fn slog3_to_linear_sgamut3_f64(y: f64) -> f64 {
    if y >= SLOG3_Y_BREAK {
        10.0_f64.powf((y * 1023.0 - 420.0) / 261.5) * (0.18 + 0.01) - 0.01
    } else {
        (y * 1023.0 - 95.0) * SLOG3_X_BREAK / (SLOG3_C_LOW - 95.0)
    }
}

#[inline]
fn map_rgb(rgb: [f64; 3], f: fn(f64) -> f64) -> [f64; 3] {
    [f(rgb[0]), f(rgb[1]), f(rgb[2])]
}

/// Converts `rgb` from `space` into `LinearRec709` (D65) in `f64` precision.
pub fn to_linear_rec709_f64(space: OcioSpace, rgb: [f64; 3]) -> [f64; 3] {
    match space {
        OcioSpace::LinearRec709 => rgb,
        OcioSpace::DisplaySrgb => map_rgb(rgb, srgb_to_linear_f64),
        OcioSpace::Rec709 => map_rgb(rgb, rec709_to_linear_f64),
        OcioSpace::LinearRec2020 => mul_mat3_f64(&LINEAR_REC2020_TO_LINEAR_REC709_MAT, rgb),
        OcioSpace::Rec2020 => {
            let lin2020 = map_rgb(rgb, rec2020_to_linear_f64);
            mul_mat3_f64(&LINEAR_REC2020_TO_LINEAR_REC709_MAT, lin2020)
        }
        OcioSpace::Aces2065_1 => {
            let ap1 = mul_mat3_f64(&AP0_TO_AP1_MAT, rgb);
            mul_mat3_f64(&AP1_TO_LINEAR_REC709_MAT, ap1)
        }
        OcioSpace::AcesCg => mul_mat3_f64(&AP1_TO_LINEAR_REC709_MAT, rgb),
        OcioSpace::AcesCct => {
            let ap1 = map_rgb(rgb, acescct_to_linear_ap1_f64);
            mul_mat3_f64(&AP1_TO_LINEAR_REC709_MAT, ap1)
        }
        OcioSpace::ArriLogC3 => {
            let awg3 = map_rgb(rgb, logc3_to_linear_awg3_f64);
            mul_mat3_f64(&AWG3_TO_LINEAR_REC709_MAT, awg3)
        }
        OcioSpace::SonySLog3 => {
            let sgamut3 = map_rgb(rgb, slog3_to_linear_sgamut3_f64);
            mul_mat3_f64(&SGAMUT3_TO_LINEAR_REC709_MAT, sgamut3)
        }
    }
}

/// Converts `rgb` from `LinearRec709` (D65) into `space` in `f64` precision.
pub fn from_linear_rec709_f64(space: OcioSpace, rgb: [f64; 3]) -> [f64; 3] {
    match space {
        OcioSpace::LinearRec709 => rgb,
        OcioSpace::DisplaySrgb => map_rgb(rgb, linear_to_srgb_f64),
        OcioSpace::Rec709 => map_rgb(rgb, linear_to_rec709_f64),
        OcioSpace::LinearRec2020 => {
            let inv = invert_mat3_f64(&LINEAR_REC2020_TO_LINEAR_REC709_MAT)
                .expect("Rec2020 matrix invertible");
            mul_mat3_f64(&inv, rgb)
        }
        OcioSpace::Rec2020 => {
            let inv = invert_mat3_f64(&LINEAR_REC2020_TO_LINEAR_REC709_MAT)
                .expect("Rec2020 matrix invertible");
            let lin2020 = mul_mat3_f64(&inv, rgb);
            map_rgb(lin2020, linear_to_rec2020_f64)
        }
        OcioSpace::Aces2065_1 => {
            let ap1 = mul_mat3_f64(&LINEAR_REC709_TO_AP1_MAT, rgb);
            mul_mat3_f64(&AP1_TO_AP0_MAT, ap1)
        }
        OcioSpace::AcesCg => mul_mat3_f64(&LINEAR_REC709_TO_AP1_MAT, rgb),
        OcioSpace::AcesCct => {
            let ap1 = mul_mat3_f64(&LINEAR_REC709_TO_AP1_MAT, rgb);
            map_rgb(ap1, linear_ap1_to_acescct_f64)
        }
        OcioSpace::ArriLogC3 => {
            let inv = invert_mat3_f64(&AWG3_TO_LINEAR_REC709_MAT).expect("AWG3 matrix invertible");
            let awg3 = mul_mat3_f64(&inv, rgb);
            map_rgb(awg3, linear_awg3_to_logc3_f64)
        }
        OcioSpace::SonySLog3 => {
            let inv = invert_mat3_f64(&SGAMUT3_TO_LINEAR_REC709_MAT)
                .expect("SGamut3 matrix invertible");
            let sgamut3 = mul_mat3_f64(&inv, rgb);
            map_rgb(sgamut3, linear_sgamut3_to_slog3_f64)
        }
    }
}

/// Evaluates an analytic `src -> dst` transform directly in `f64`, bypassing unnecessary
/// chromatic-adaptation round-trips for same-family spaces (ACES / Rec.2020).
pub fn eval_builtin_transform_f64(src: OcioSpace, dst: OcioSpace, rgb: [f64; 3]) -> [f64; 3] {
    if src == dst {
        return rgb;
    }
    // Direct ACES D60 family paths (no D60 <-> D65 round-trip)
    let is_aces = |s: OcioSpace| {
        matches!(
            s,
            OcioSpace::Aces2065_1 | OcioSpace::AcesCg | OcioSpace::AcesCct
        )
    };
    if is_aces(src) && is_aces(dst) {
        let ap1 = match src {
            OcioSpace::AcesCg => rgb,
            OcioSpace::Aces2065_1 => mul_mat3_f64(&AP0_TO_AP1_MAT, rgb),
            OcioSpace::AcesCct => map_rgb(rgb, acescct_to_linear_ap1_f64),
            _ => unreachable!(),
        };
        return match dst {
            OcioSpace::AcesCg => ap1,
            OcioSpace::Aces2065_1 => mul_mat3_f64(&AP1_TO_AP0_MAT, ap1),
            OcioSpace::AcesCct => map_rgb(ap1, linear_ap1_to_acescct_f64),
            _ => unreachable!(),
        };
    }
    // Direct Rec.2020 OETF path (same primaries, no 3x3 matrix needed)
    match (src, dst) {
        (OcioSpace::Rec2020, OcioSpace::LinearRec2020) => {
            return map_rgb(rgb, rec2020_to_linear_f64);
        }
        (OcioSpace::LinearRec2020, OcioSpace::Rec2020) => {
            return map_rgb(rgb, linear_to_rec2020_f64);
        }
        _ => {}
    }

    let lin709 = to_linear_rec709_f64(src, rgb);
    from_linear_rec709_f64(dst, lin709)
}

// -----------------------------------------------------------------------------
// OCIO Config AST, YAML/Text Parser & Pure-Rust Chain Compiler
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OcioInterpolation {
    Nearest,
    #[default]
    Linear,
    Tetrahedral,
    Best,
}

impl OcioInterpolation {
    fn parse(s: &str) -> Result<Self, RenderError> {
        match s.trim().to_ascii_lowercase().as_str() {
            "nearest" => Ok(Self::Nearest),
            "linear" => Ok(Self::Linear),
            "tetrahedral" => Ok(Self::Tetrahedral),
            "best" => Ok(Self::Best),
            other => Err(RenderError::InvalidLut3D(format!(
                "unsupported OCIO interpolation '{other}'"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NegativeStyle {
    #[default]
    Clamp,
    Mirror,
    PassThru,
    Linear,
}

impl NegativeStyle {
    fn parse(s: &str) -> Result<Self, RenderError> {
        match s.trim().to_ascii_lowercase().as_str() {
            "clamp" => Ok(Self::Clamp),
            "mirror" => Ok(Self::Mirror),
            "pass_thru" | "passthru" => Ok(Self::PassThru),
            "linear" => Ok(Self::Linear),
            other => Err(RenderError::InvalidLut3D(format!(
                "unsupported OCIO ExponentTransform negative_style '{other}'"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum OcioTransform {
    Matrix {
        matrix: [f64; 16],
        offset: [f64; 4],
        inverse: bool,
    },
    File {
        src: String,
        interpolation: OcioInterpolation,
        inverse: bool,
    },
    Exponent {
        value: [f64; 4],
        negative_style: NegativeStyle,
        inverse: bool,
    },
    Log {
        base: f64,
        inverse: bool,
    },
    Group {
        children: Vec<OcioTransform>,
        inverse: bool,
    },
    Unsupported {
        tag: String,
    },
}

/// Automatically inverts any `OcioTransform` tree (used when `from_reference` is
/// omitted or when a transform runs in `direction: inverse`).
pub fn invert_transform(transform: &OcioTransform) -> OcioTransform {
    match transform {
        OcioTransform::Matrix {
            matrix,
            offset,
            inverse,
        } => OcioTransform::Matrix {
            matrix: *matrix,
            offset: *offset,
            inverse: !*inverse,
        },
        OcioTransform::File {
            src,
            interpolation,
            inverse,
        } => OcioTransform::File {
            src: src.clone(),
            interpolation: *interpolation,
            inverse: !*inverse,
        },
        OcioTransform::Exponent {
            value,
            negative_style,
            inverse,
        } => OcioTransform::Exponent {
            value: *value,
            negative_style: *negative_style,
            inverse: !*inverse,
        },
        OcioTransform::Log { base, inverse } => OcioTransform::Log {
            base: *base,
            inverse: !*inverse,
        },
        OcioTransform::Group { children, inverse } => OcioTransform::Group {
            children: children.clone(),
            inverse: !*inverse,
        },
        OcioTransform::Unsupported { tag } => OcioTransform::Unsupported { tag: tag.clone() },
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OcioColorSpaceDef {
    pub name: String,
    pub aliases: Vec<String>,
    pub family: String,
    pub is_data: bool,
    pub to_reference: Option<OcioTransform>,
    pub from_reference: Option<OcioTransform>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OcioConfig {
    pub profile_version: u32,
    pub config_path: Option<PathBuf>,
    pub config_dir: PathBuf,
    pub search_paths: Vec<PathBuf>,
    pub roles: HashMap<String, String>,
    pub colorspaces: HashMap<String, OcioColorSpaceDef>,
}

impl OcioConfig {
    /// Parses a `.ocio` YAML/text config from disk.
    pub fn from_file(config_path: &Path) -> Result<Self, RenderError> {
        let text = std::fs::read_to_string(config_path).map_err(|e| {
            RenderError::InvalidLut3D(format!(
                "failed to read OCIO config '{}': {e}",
                config_path.display()
            ))
        })?;
        let base_dir = config_path.parent().unwrap_or_else(|| Path::new("."));
        let mut cfg = Self::from_str(&text, base_dir)?;
        cfg.config_path = Some(config_path.to_path_buf());
        Ok(cfg)
    }

    /// Parses a `.ocio` YAML/text config from an in-memory string.
    pub fn from_str(yaml_text: &str, base_dir: &Path) -> Result<Self, RenderError> {
        let logical_lines = preprocess_yaml_lines(yaml_text)?;
        let mut profile_version: u32 = 1;
        let mut raw_search_paths: Vec<String> = Vec::new();
        let mut roles: HashMap<String, String> = HashMap::new();
        let mut colorspaces: HashMap<String, OcioColorSpaceDef> = HashMap::new();

        let mut i = 0;
        while i < logical_lines.len() {
            let line = &logical_lines[i];
            if line.indent > 0 {
                i += 1;
                continue;
            }
            if let Some((key, val)) = split_key_value(&line.text) {
                match key {
                    "ocio_profile_version" => {
                        let cleaned = strip_quotes(val.trim());
                        profile_version = cleaned.parse::<u32>().map_err(|e| {
                            RenderError::InvalidLut3D(format!(
                                "invalid ocio_profile_version '{cleaned}': {e}"
                            ))
                        })?;
                        i += 1;
                    }
                    "search_path" => {
                        i += 1;
                        if !val.trim().is_empty() {
                            extend_search_paths(val.trim(), &mut raw_search_paths);
                        } else {
                            while i < logical_lines.len() && logical_lines[i].indent > line.indent {
                                let item = logical_lines[i].text.trim();
                                if let Some(rest) = item.strip_prefix('-') {
                                    extend_search_paths(rest.trim(), &mut raw_search_paths);
                                }
                                i += 1;
                            }
                        }
                    }
                    "roles" => {
                        i += 1;
                        while i < logical_lines.len() && logical_lines[i].indent > line.indent {
                            let sub = &logical_lines[i];
                            if let Some((rk, rv)) = split_key_value(&sub.text) {
                                roles.insert(
                                    rk.to_ascii_lowercase(),
                                    strip_quotes(rv.trim()).to_owned(),
                                );
                            }
                            i += 1;
                        }
                    }
                    "colorspaces" => {
                        i += 1;
                        while i < logical_lines.len() && logical_lines[i].indent > line.indent {
                            let sub = &logical_lines[i];
                            let trimmed = sub.text.trim();
                            if trimmed.starts_with('-')
                                && (trimmed.contains("!<ColorSpace>")
                                    || trimmed == "-"
                                    || trimmed.starts_with("- "))
                            {
                                let (cs, next_idx) = parse_colorspace_entry(&logical_lines, i)?;
                                colorspaces.insert(cs.name.clone(), cs);
                                i = next_idx;
                            } else {
                                i += 1;
                            }
                        }
                    }
                    _ => {
                        i += 1;
                    }
                }
            } else {
                i += 1;
            }
        }

        let search_paths = raw_search_paths
            .into_iter()
            .map(|p| {
                let path = PathBuf::from(&p);
                if path.is_absolute() {
                    path
                } else {
                    base_dir.join(path)
                }
            })
            .collect();

        Ok(Self {
            profile_version,
            config_path: None,
            config_dir: base_dir.to_path_buf(),
            search_paths,
            roles,
            colorspaces,
        })
    }

    /// Resolves a colorspace name, role, or alias (case-insensitive fallback).
    pub fn resolve_colorspace(&self, name_or_role: &str) -> Result<&OcioColorSpaceDef, RenderError> {
        let trimmed = name_or_role.trim();
        if let Some(cs) = self.colorspaces.get(trimmed) {
            return Ok(cs);
        }
        let lower = trimmed.to_ascii_lowercase();
        if let Some(role_target) = self.roles.get(&lower) {
            if let Some(cs) = self.colorspaces.get(role_target) {
                return Ok(cs);
            }
            for cs in self.colorspaces.values() {
                if cs.name.eq_ignore_ascii_case(role_target) {
                    return Ok(cs);
                }
            }
        }
        for cs in self.colorspaces.values() {
            if cs.name.eq_ignore_ascii_case(trimmed)
                || cs.aliases.iter().any(|a| a.eq_ignore_ascii_case(trimmed))
            {
                return Ok(cs);
            }
        }
        Err(RenderError::InvalidLut3D(format!(
            "OCIO colorspace, role, or alias '{name_or_role}' not found in config"
        )))
    }

    /// Resolves a `FileTransform` path against `config_dir` and `search_paths`.
    pub fn resolve_lut_path(&self, src: &str) -> Result<PathBuf, RenderError> {
        let candidate = Path::new(src);
        if candidate.is_absolute() {
            if candidate.is_file() {
                return Ok(candidate.to_path_buf());
            }
            return Err(RenderError::InvalidLut3D(format!(
                "OCIO FileTransform source '{}' does not exist",
                candidate.display()
            )));
        }

        let direct = self.config_dir.join(candidate);
        if direct.is_file() {
            return Ok(direct);
        }

        for dir in &self.search_paths {
            let in_search = dir.join(candidate);
            if in_search.is_file() {
                return Ok(in_search);
            }
        }

        Err(RenderError::InvalidLut3D(format!(
            "OCIO FileTransform source '{src}' not found in '{}' or search_paths",
            self.config_dir.display()
        )))
    }
}

/// Options for baking an `OcioColorPipeline` chain into a 3D LUT.
#[derive(Debug, Clone, PartialEq)]
pub struct OcioBakeOptions {
    pub lut_size: u32,
    pub domain_min: [f32; 3],
    pub domain_max: [f32; 3],
    pub shaper_space: Option<String>,
    pub allow_cli_fallback: bool,
}

impl Default for OcioBakeOptions {
    fn default() -> Self {
        Self {
            lut_size: 33,
            domain_min: [0.0, 0.0, 0.0],
            domain_max: [1.0, 1.0, 1.0],
            shaper_space: None,
            allow_cli_fallback: true,
        }
    }
}

#[derive(Debug, Clone)]
enum CompiledStep {
    Matrix {
        mat: [f64; 16],
        offset: [f64; 4],
    },
    MatrixInverse {
        inv_mat: [f64; 16],
        offset: [f64; 4],
    },
    Lut1D {
        lut: CubeLut1D,
        inverse: bool,
    },
    Lut3D {
        lut: CubeLut3D,
        inverse: bool,
    },
    Exponent {
        exp: [f64; 4],
        negative_style: NegativeStyle,
    },
    Log {
        base: f64,
        inverse: bool,
    },
}

/// Compiled pure-Rust `f64` OCIO transform chain (`src_space -> reference -> dst_space`).
#[derive(Debug, Clone)]
pub struct CompiledOcioChain {
    pub src_space: String,
    pub dst_space: String,
    steps: Vec<CompiledStep>,
}

impl CompiledOcioChain {
    /// Evaluates the full compiled OCIO chain on an RGB triplet in `f64`.
    pub fn eval_rgb(&self, rgb: [f64; 3]) -> [f64; 3] {
        let mut cur = rgb;
        for step in &self.steps {
            cur = eval_compiled_step(step, cur);
        }
        cur
    }
}

/// Pure-Rust OpenColorIO config pipeline with optional `ociobakelut` CLI fallback.
#[derive(Debug, Clone)]
pub struct OcioColorPipeline {
    config: OcioConfig,
}

impl OcioColorPipeline {
    /// Loads and parses an `.ocio` config file from disk.
    pub fn from_file(config_path: &Path) -> Result<Self, RenderError> {
        let config = OcioConfig::from_file(config_path)?;
        Ok(Self { config })
    }

    /// Parses an `.ocio` config from an in-memory string and base directory.
    pub fn from_str(yaml_text: &str, base_dir: &Path) -> Result<Self, RenderError> {
        let config = OcioConfig::from_str(yaml_text, base_dir)?;
        Ok(Self { config })
    }

    /// Returns a reference to the parsed `OcioConfig`.
    pub fn config(&self) -> &OcioConfig {
        &self.config
    }

    /// Compiles `src_space -> reference -> dst_space` into a deterministic `f64` CPU chain.
    pub fn build_chain(
        &self,
        src_space: &str,
        dst_space: &str,
    ) -> Result<CompiledOcioChain, RenderError> {
        let src_cs = self.config.resolve_colorspace(src_space)?;
        let dst_cs = self.config.resolve_colorspace(dst_space)?;

        let mut steps = Vec::new();
        if !src_cs.name.eq_ignore_ascii_case(&dst_cs.name) && !src_cs.is_data && !dst_cs.is_data {
            if let Some(to_ref) = &src_cs.to_reference {
                compile_transform(&self.config, to_ref, &mut steps)?;
            } else if let Some(from_ref) = &src_cs.from_reference {
                let inv = invert_transform(from_ref);
                compile_transform(&self.config, &inv, &mut steps)?;
            }

            if let Some(from_ref) = &dst_cs.from_reference {
                compile_transform(&self.config, from_ref, &mut steps)?;
            } else if let Some(to_ref) = &dst_cs.to_reference {
                let inv = invert_transform(to_ref);
                compile_transform(&self.config, &inv, &mut steps)?;
            }
        }

        Ok(CompiledOcioChain {
            src_space: src_cs.name.clone(),
            dst_space: dst_cs.name.clone(),
            steps,
        })
    }

    /// Bakes `src_space -> dst_space` into a GPU-ready `CubeLut3D`.
    pub fn bake_to_cube_lut(
        &self,
        src_space: &str,
        dst_space: &str,
        opts: &OcioBakeOptions,
    ) -> Result<CubeLut3D, RenderError> {
        if !(2..=MAX_LUT_3D_SIZE).contains(&opts.lut_size) {
            return Err(RenderError::InvalidLut3D(format!(
                "OCIO bake lut_size must be in 2..={MAX_LUT_3D_SIZE}, got {}",
                opts.lut_size
            )));
        }
        for c in 0..3 {
            if !opts.domain_min[c].is_finite()
                || !opts.domain_max[c].is_finite()
                || opts.domain_max[c] <= opts.domain_min[c]
            {
                return Err(RenderError::InvalidLut3D(format!(
                    "invalid OCIO bake domain on channel {c}: min={}, max={}",
                    opts.domain_min[c], opts.domain_max[c]
                )));
            }
        }

        match self.build_chain(src_space, dst_space) {
            Ok(chain) => {
                let n = opts.lut_size as usize;
                let denom = (opts.lut_size - 1) as f64;
                let mut data = Vec::with_capacity(n * n * n);
                let span = [
                    (opts.domain_max[0] - opts.domain_min[0]) as f64,
                    (opts.domain_max[1] - opts.domain_min[1]) as f64,
                    (opts.domain_max[2] - opts.domain_min[2]) as f64,
                ];
                for b in 0..n {
                    let fb = opts.domain_min[2] as f64 + (b as f64 / denom) * span[2];
                    for g in 0..n {
                        let fg = opts.domain_min[1] as f64 + (g as f64 / denom) * span[1];
                        for r in 0..n {
                            let fr = opts.domain_min[0] as f64 + (r as f64 / denom) * span[0];
                            let out = chain.eval_rgb([fr, fg, fb]);
                            data.push([out[0] as f32, out[1] as f32, out[2] as f32, 1.0]);
                        }
                    }
                }
                Ok(CubeLut3D {
                    title: format!("{} -> {}", chain.src_space, chain.dst_space),
                    size: opts.lut_size,
                    domain_min: opts.domain_min,
                    domain_max: opts.domain_max,
                    data_rgba16f_or_f32: data,
                    shaper_1d: None,
                    input_space: ColorSpace::LinearRec709,
                    output_space: ColorSpace::LinearRec709,
                })
            }
            Err(err) => {
                if opts.allow_cli_fallback
                    && let Some(cfg_path) = &self.config.config_path
                    && let Some(baked) =
                        try_ociobakelut_cli(cfg_path, src_space, dst_space, opts)?
                {
                    return Ok(baked);
                }
                Err(err)
            }
        }
    }
}

fn try_ociobakelut_cli(
    config_path: &Path,
    src_space: &str,
    dst_space: &str,
    opts: &OcioBakeOptions,
) -> Result<Option<CubeLut3D>, RenderError> {
    let probe = Command::new("ociobakelut").arg("--help").output();
    match probe {
        Ok(out) if out.status.success() => {}
        _ => return Ok(None),
    }

    let mut cmd = Command::new("ociobakelut");
    cmd.arg("--iconfig")
        .arg(config_path)
        .arg("--inputspace")
        .arg(src_space)
        .arg("--outputspace")
        .arg(dst_space)
        .arg("--cubesize")
        .arg(opts.lut_size.to_string())
        .arg("--format")
        .arg("iridas_cube")
        .arg("--stdout");

    if let Some(shaper) = &opts.shaper_space {
        cmd.arg("--shaperspace").arg(shaper);
    }

    let output = cmd.output().map_err(|e| {
        RenderError::InvalidLut3D(format!("failed to execute 'ociobakelut': {e}"))
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(RenderError::InvalidLut3D(format!(
            "ociobakelut failed for '{src_space}' -> '{dst_space}': {stderr}"
        )));
    }
    let stdout_cube = String::from_utf8(output.stdout).map_err(|e| {
        RenderError::InvalidLut3D(format!("ociobakelut output is not valid UTF-8: {e}"))
    })?;
    let lut = CubeLut3D::parse_cube(&stdout_cube)?;
    Ok(Some(lut))
}

fn compile_transform(
    config: &OcioConfig,
    transform: &OcioTransform,
    out: &mut Vec<CompiledStep>,
) -> Result<(), RenderError> {
    match transform {
        OcioTransform::Matrix {
            matrix,
            offset,
            inverse,
        } => {
            if *inverse {
                let inv_mat = invert_mat4_f64(matrix).ok_or_else(|| {
                    RenderError::InvalidLut3D(
                        "singular MatrixTransform cannot be inverted".to_owned(),
                    )
                })?;
                out.push(CompiledStep::MatrixInverse {
                    inv_mat,
                    offset: *offset,
                });
            } else {
                out.push(CompiledStep::Matrix {
                    mat: *matrix,
                    offset: *offset,
                });
            }
        }
        OcioTransform::File {
            src,
            interpolation: _,
            inverse,
        } => {
            let resolved = config.resolve_lut_path(src)?;
            let ext = resolved
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_default();
            if ext == "spi1d" {
                let text = std::fs::read_to_string(&resolved).map_err(|e| {
                    RenderError::InvalidLut3D(format!(
                        "failed to read '{}': {e}",
                        resolved.display()
                    ))
                })?;
                let lut1d = CubeLut1D::parse_spi1d(&text)?;
                out.push(CompiledStep::Lut1D {
                    lut: lut1d,
                    inverse: *inverse,
                });
            } else {
                let lut3d = CubeLut3D::resolve_file(&resolved)?;
                out.push(CompiledStep::Lut3D {
                    lut: lut3d,
                    inverse: *inverse,
                });
            }
        }
        OcioTransform::Exponent {
            value,
            negative_style,
            inverse,
        } => {
            let mut exp = *value;
            if *inverse {
                for (c, slot) in exp.iter_mut().enumerate() {
                    if slot.abs() < 1e-12 {
                        return Err(RenderError::InvalidLut3D(format!(
                            "ExponentTransform value[{c}]=0 cannot be inverted"
                        )));
                    }
                    *slot = 1.0 / *slot;
                }
            }
            out.push(CompiledStep::Exponent {
                exp,
                negative_style: *negative_style,
            });
        }
        OcioTransform::Log { base, inverse } => {
            if *base <= 0.0 || (*base - 1.0).abs() < 1e-12 {
                return Err(RenderError::InvalidLut3D(format!(
                    "invalid LogTransform base {base}"
                )));
            }
            out.push(CompiledStep::Log {
                base: *base,
                inverse: *inverse,
            });
        }
        OcioTransform::Group { children, inverse } => {
            if *inverse {
                for child in children.iter().rev() {
                    let inv_child = invert_transform(child);
                    compile_transform(config, &inv_child, out)?;
                }
            } else {
                for child in children {
                    compile_transform(config, child, out)?;
                }
            }
        }
        OcioTransform::Unsupported { tag } => {
            return Err(RenderError::InvalidLut3D(format!(
                "unsupported OCIO transform '{tag}' and 'ociobakelut' CLI not found on PATH"
            )));
        }
    }
    Ok(())
}

fn eval_compiled_step(step: &CompiledStep, rgb: [f64; 3]) -> [f64; 3] {
    match step {
        CompiledStep::Matrix { mat, offset } => {
            let v = [rgb[0], rgb[1], rgb[2], 1.0];
            [
                mat[0] * v[0] + mat[1] * v[1] + mat[2] * v[2] + mat[3] * v[3] + offset[0],
                mat[4] * v[0] + mat[5] * v[1] + mat[6] * v[2] + mat[7] * v[3] + offset[1],
                mat[8] * v[0] + mat[9] * v[1] + mat[10] * v[2] + mat[11] * v[3] + offset[2],
            ]
        }
        CompiledStep::MatrixInverse { inv_mat, offset } => {
            let v = [
                rgb[0] - offset[0],
                rgb[1] - offset[1],
                rgb[2] - offset[2],
                1.0 - offset[3],
            ];
            [
                inv_mat[0] * v[0] + inv_mat[1] * v[1] + inv_mat[2] * v[2] + inv_mat[3] * v[3],
                inv_mat[4] * v[0] + inv_mat[5] * v[1] + inv_mat[6] * v[2] + inv_mat[7] * v[3],
                inv_mat[8] * v[0] + inv_mat[9] * v[1] + inv_mat[10] * v[2] + inv_mat[11] * v[3],
            ]
        }
        CompiledStep::Lut1D { lut, inverse } => {
            if *inverse {
                sample_1d_inverse(lut, rgb)
            } else {
                let r = lut.sample_linear([rgb[0] as f32, rgb[1] as f32, rgb[2] as f32]);
                [r[0] as f64, r[1] as f64, r[2] as f64]
            }
        }
        CompiledStep::Lut3D { lut, inverse } => {
            if *inverse {
                sample_3d_inverse(lut, rgb)
            } else {
                let r = lut.sample_trilinear([rgb[0] as f32, rgb[1] as f32, rgb[2] as f32]);
                [r[0] as f64, r[1] as f64, r[2] as f64]
            }
        }
        CompiledStep::Exponent {
            exp,
            negative_style,
        } => {
            let mut out = [0.0_f64; 3];
            for c in 0..3 {
                let x = rgb[c];
                let e = exp[c];
                out[c] = match negative_style {
                    NegativeStyle::Clamp => x.max(0.0).powf(e),
                    NegativeStyle::Mirror => {
                        if x < 0.0 {
                            -((-x).powf(e))
                        } else {
                            x.powf(e)
                        }
                    }
                    NegativeStyle::PassThru | NegativeStyle::Linear => {
                        if x <= 0.0 {
                            x
                        } else {
                            x.powf(e)
                        }
                    }
                };
            }
            out
        }
        CompiledStep::Log { base, inverse } => {
            let mut out = [0.0_f64; 3];
            for c in 0..3 {
                out[c] = if *inverse {
                    base.powf(rgb[c])
                } else {
                    rgb[c].max(1e-10).log(*base)
                };
            }
            out
        }
    }
}

fn sample_1d_inverse(lut: &CubeLut1D, rgb: [f64; 3]) -> [f64; 3] {
    let mut out = [0.0_f64; 3];
    let n = lut.entries.len();
    if n < 2 {
        return rgb;
    }
    for c in 0..3 {
        let target = rgb[c] as f32;
        let span = lut.domain_max[c] - lut.domain_min[c];
        let mut found = lut.domain_min[c];
        if target <= lut.entries[0][c] {
            found = lut.domain_min[c];
        } else if target >= lut.entries[n - 1][c] {
            found = lut.domain_max[c];
        } else {
            for i in 0..(n - 1) {
                let y0 = lut.entries[i][c];
                let y1 = lut.entries[i + 1][c];
                let lo = y0.min(y1);
                let hi = y0.max(y1);
                if target >= lo && target <= hi {
                    let denom = (y1 - y0).abs();
                    let frac = if denom > 1e-8 {
                        (target - y0) / (y1 - y0)
                    } else {
                        0.0
                    };
                    let u = (i as f32 + frac.clamp(0.0, 1.0)) / (n - 1) as f32;
                    found = lut.domain_min[c] + u * span;
                    break;
                }
            }
        }
        out[c] = found as f64;
    }
    out
}

fn sample_3d_inverse(lut: &CubeLut3D, target: [f64; 3]) -> [f64; 3] {
    // Damped fixed-point iteration for smooth invertible 3D LUTs
    let mut x = [target[0] as f32, target[1] as f32, target[2] as f32];
    for _ in 0..16 {
        let fx = lut.sample_trilinear(x);
        for c in 0..3 {
            let err = target[c] as f32 - fx[c];
            x[c] = (x[c] + 0.65 * err).clamp(lut.domain_min[c], lut.domain_max[c]);
        }
    }
    [x[0] as f64, x[1] as f64, x[2] as f64]
}

// -----------------------------------------------------------------------------
// Pure-Rust YAML/Text Parsing Helpers for `.ocio`
// -----------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct LogicalLine {
    indent: usize,
    text: String,
}

fn preprocess_yaml_lines(yaml_text: &str) -> Result<Vec<LogicalLine>, RenderError> {
    let mut out = Vec::new();
    let mut acc_text = String::new();
    let mut acc_indent = 0usize;
    let mut bracket_depth: i32 = 0;
    let mut brace_depth: i32 = 0;

    for raw in yaml_text.lines() {
        let cleaned = strip_yaml_comment(raw);
        let trimmed = cleaned.trim();
        if trimmed.is_empty() && bracket_depth == 0 && brace_depth == 0 {
            continue;
        }
        let indent = cleaned.bytes().take_while(|b| *b == b' ').count();

        if bracket_depth == 0 && brace_depth == 0 {
            acc_indent = indent;
            acc_text.clear();
            acc_text.push_str(trimmed);
        } else if !trimmed.is_empty() {
            acc_text.push(' ');
            acc_text.push_str(trimmed);
        }

        update_delimiters(trimmed, &mut bracket_depth, &mut brace_depth);
        if bracket_depth < 0 || brace_depth < 0 {
            return Err(RenderError::InvalidLut3D(
                "unbalanced brackets/braces in .ocio config".to_owned(),
            ));
        }

        if bracket_depth == 0 && brace_depth == 0 && !acc_text.is_empty() {
            out.push(LogicalLine {
                indent: acc_indent,
                text: std::mem::take(&mut acc_text),
            });
        }
    }

    if bracket_depth != 0 || brace_depth != 0 {
        return Err(RenderError::InvalidLut3D(
            "unclosed '[' or '{' in .ocio config".to_owned(),
        ));
    }

    Ok(out)
}

fn strip_yaml_comment(line: &str) -> &str {
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    for (idx, ch) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' if in_double => escaped = true,
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '#' if !in_single && !in_double => return &line[..idx],
            _ => {}
        }
    }
    line
}

fn update_delimiters(text: &str, bracket_depth: &mut i32, brace_depth: &mut i32) {
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    for ch in text.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' if in_double => escaped = true,
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '[' if !in_single && !in_double => *bracket_depth += 1,
            ']' if !in_single && !in_double => *bracket_depth -= 1,
            '{' if !in_single && !in_double => *brace_depth += 1,
            '}' if !in_single && !in_double => *brace_depth -= 1,
            _ => {}
        }
    }
}

fn split_key_value(text: &str) -> Option<(&str, &str)> {
    let mut in_single = false;
    let mut in_double = false;
    let mut depth = 0i32;
    for (idx, ch) in text.char_indices() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '[' | '{' if !in_single && !in_double => depth += 1,
            ']' | '}' if !in_single && !in_double => depth -= 1,
            ':' if !in_single && !in_double && depth == 0 => {
                let key = text[..idx].trim();
                let val = text[idx + 1..].trim();
                if !key.is_empty() {
                    return Some((key, val));
                }
                return None;
            }
            _ => {}
        }
    }
    None
}

fn strip_quotes(s: &str) -> &str {
    let t = s.trim();
    if (t.starts_with('"') && t.ends_with('"') && t.len() >= 2)
        || (t.starts_with('\'') && t.ends_with('\'') && t.len() >= 2)
    {
        &t[1..t.len() - 1]
    } else {
        t
    }
}

fn extend_search_paths(val: &str, out: &mut Vec<String>) {
    let trimmed = val.trim();
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        for item in split_top_level(&trimmed[1..trimmed.len() - 1], ',') {
            let clean = strip_quotes(item.trim());
            if !clean.is_empty() {
                out.push(clean.to_owned());
            }
        }
        return;
    }

    let clean = strip_quotes(trimmed);
    for seg in clean.split(';') {
        // Split on ':' except Windows drive-letter prefixes like `C:\` or `C:/`
        let bytes = seg.as_bytes();
        let mut start = 0usize;
        let mut idx = 0usize;
        while idx < bytes.len() {
            if bytes[idx] == b':' {
                let is_drive = idx == start + 1
                    && bytes[start].is_ascii_alphabetic()
                    && matches!(bytes.get(idx + 1), Some(b'\\' | b'/'));
                if !is_drive {
                    let part = seg[start..idx].trim();
                    if !part.is_empty() {
                        out.push(part.to_owned());
                    }
                    start = idx + 1;
                }
            }
            idx += 1;
        }
        let tail = seg[start..].trim();
        if !tail.is_empty() {
            out.push(tail.to_owned());
        }
    }
}

fn split_top_level(text: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut bracket = 0i32;
    let mut brace = 0i32;
    let mut start = 0usize;
    for (idx, ch) in text.char_indices() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '[' if !in_single && !in_double => bracket += 1,
            ']' if !in_single && !in_double => bracket -= 1,
            '{' if !in_single && !in_double => brace += 1,
            '}' if !in_single && !in_double => brace -= 1,
            c if c == sep && !in_single && !in_double && bracket == 0 && brace == 0 => {
                parts.push(text[start..idx].trim());
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }
    let tail = text[start..].trim();
    if !tail.is_empty() || !parts.is_empty() {
        parts.push(tail);
    }
    parts
}

fn parse_colorspace_entry(
    lines: &[LogicalLine],
    start: usize,
) -> Result<(OcioColorSpaceDef, usize), RenderError> {
    let entry_indent = lines[start].indent;
    let mut name = String::new();
    let mut family = String::new();
    let mut aliases: Vec<String> = Vec::new();
    let mut is_data = false;
    let mut to_reference: Option<OcioTransform> = None;
    let mut from_reference: Option<OcioTransform> = None;

    let mut i = start + 1;
    // Handle inline `- !<ColorSpace> {name: ...}` if ever present
    if let Some(brace_start) = lines[start].text.find('{')
        && let Some(brace_end) = lines[start].text.rfind('}')
        && brace_start < brace_end
    {
        for pair in split_top_level(&lines[start].text[brace_start + 1..brace_end], ',') {
            if let Some((k, v)) = split_key_value(pair) {
                apply_colorspace_kv(
                    k,
                    v,
                    &mut name,
                    &mut family,
                    &mut aliases,
                    &mut is_data,
                );
            }
        }
    }

    while i < lines.len() {
        let line = &lines[i];
        if line.indent < entry_indent {
            break;
        }
        if line.indent == entry_indent && line.text.trim_start().starts_with('-') {
            break;
        }

        if let Some((key, val)) = split_key_value(&line.text) {
            match key {
                "aliases" => {
                    i += 1;
                    if !val.is_empty() {
                        parse_inline_string_list(val, &mut aliases);
                    } else {
                        while i < lines.len() && lines[i].indent > line.indent {
                            let item = lines[i].text.trim();
                            if let Some(rest) = item.strip_prefix('-') {
                                let clean = strip_quotes(rest.trim());
                                if !clean.is_empty() {
                                    aliases.push(clean.to_owned());
                                }
                            }
                            i += 1;
                        }
                    }
                }
                "to_reference" | "to_scene_reference" => {
                    let (tr, next) = parse_transform_block(lines, i, val)?;
                    to_reference = Some(tr);
                    i = next;
                }
                "from_reference" | "from_scene_reference" => {
                    let (tr, next) = parse_transform_block(lines, i, val)?;
                    from_reference = Some(tr);
                    i = next;
                }
                _ => {
                    apply_colorspace_kv(
                        key,
                        val,
                        &mut name,
                        &mut family,
                        &mut aliases,
                        &mut is_data,
                    );
                    i += 1;
                }
            }
        } else {
            i += 1;
        }
    }

    if name.is_empty() {
        return Err(RenderError::InvalidLut3D(
            "OCIO ColorSpace entry missing 'name'".to_owned(),
        ));
    }

    Ok((
        OcioColorSpaceDef {
            name,
            aliases,
            family,
            is_data,
            to_reference,
            from_reference,
        },
        i,
    ))
}

fn apply_colorspace_kv(
    key: &str,
    val: &str,
    name: &mut String,
    family: &mut String,
    aliases: &mut Vec<String>,
    is_data: &mut bool,
) {
    match key {
        "name" => *name = strip_quotes(val).to_owned(),
        "family" => *family = strip_quotes(val).to_owned(),
        "aliases" => parse_inline_string_list(val, aliases),
        "isdata" => *is_data = val.trim().eq_ignore_ascii_case("true"),
        _ => {}
    }
}

fn parse_inline_string_list(val: &str, out: &mut Vec<String>) {
    let trimmed = val.trim();
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        for item in split_top_level(&trimmed[1..trimmed.len() - 1], ',') {
            let clean = strip_quotes(item);
            if !clean.is_empty() {
                out.push(clean.to_owned());
            }
        }
    } else {
        let clean = strip_quotes(trimmed);
        if !clean.is_empty() {
            out.push(clean.to_owned());
        }
    }
}

fn parse_transform_block(
    lines: &[LogicalLine],
    key_line_idx: usize,
    inline_val: &str,
) -> Result<(OcioTransform, usize), RenderError> {
    let parent_indent = lines[key_line_idx].indent;
    let val_trimmed = inline_val.trim();
    if !val_trimmed.is_empty() {
        return parse_transform_from_head(lines, key_line_idx, parent_indent, val_trimmed);
    }
    // Transform starts on the next indented line
    let next_idx = key_line_idx + 1;
    if next_idx >= lines.len() || lines[next_idx].indent <= parent_indent {
        return Err(RenderError::InvalidLut3D(
            "empty transform block in .ocio config".to_owned(),
        ));
    }
    let head = lines[next_idx].text.trim();
    parse_transform_from_head(lines, next_idx, lines[next_idx].indent, head)
}

fn parse_transform_from_head(
    lines: &[LogicalLine],
    head_line_idx: usize,
    block_indent: usize,
    head_expr: &str,
) -> Result<(OcioTransform, usize), RenderError> {
    let (tag, inline_body) = extract_tag_and_brace(head_expr).ok_or_else(|| {
        RenderError::InvalidLut3D(format!(
            "expected OCIO transform tag !<...>, found '{head_expr}'"
        ))
    })?;

    let mut fields: Vec<(String, String)> = Vec::new();
    let mut children: Vec<OcioTransform> = Vec::new();

    if let Some(body) = inline_body {
        for pair in split_top_level(body, ',') {
            if let Some((k, v)) = split_key_value(pair) {
                if k == "children" {
                    parse_inline_children(v, &mut children)?;
                } else {
                    fields.push((k.to_owned(), v.to_owned()));
                }
            }
        }
    }

    let mut i = head_line_idx + 1;
    while i < lines.len() && lines[i].indent > block_indent {
        let sub = &lines[i];
        if let Some((k, v)) = split_key_value(&sub.text) {
            if k == "children" {
                i += 1;
                if !v.trim().is_empty() {
                    parse_inline_children(v.trim(), &mut children)?;
                } else {
                    while i < lines.len() && lines[i].indent > sub.indent {
                        let item = lines[i].text.trim();
                        if let Some(rest) = item.strip_prefix('-') {
                            let child_head = rest.trim();
                            let (child_tr, next_i) = parse_transform_from_head(
                                lines,
                                i,
                                lines[i].indent,
                                child_head,
                            )?;
                            children.push(child_tr);
                            i = next_i;
                        } else {
                            i += 1;
                        }
                    }
                }
            } else {
                fields.push((k.to_owned(), v.to_owned()));
                i += 1;
            }
        } else {
            i += 1;
        }
    }

    let tr = build_transform_from_fields(tag, &fields, children)?;
    Ok((tr, i))
}

fn parse_inline_children(val: &str, out: &mut Vec<OcioTransform>) -> Result<(), RenderError> {
    let trimmed = val.trim();
    if !(trimmed.starts_with('[') && trimmed.ends_with(']')) {
        return Err(RenderError::InvalidLut3D(format!(
            "expected inline children [...] list, got '{trimmed}'"
        )));
    }
    let inner = &trimmed[1..trimmed.len() - 1];
    for item in split_top_level(inner, ',') {
        if item.is_empty() {
            continue;
        }
        let dummy = [LogicalLine {
            indent: 0,
            text: item.to_owned(),
        }];
        let (tr, _) = parse_transform_from_head(&dummy, 0, 0, item)?;
        out.push(tr);
    }
    Ok(())
}

fn extract_tag_and_brace(expr: &str) -> Option<(&str, Option<&str>)> {
    let trimmed = expr.trim();
    let rest = trimmed.strip_prefix("!<")?;
    let end = rest.find('>')?;
    let tag = &rest[..end];
    let after = rest[end + 1..].trim();
    if let Some(b_start) = after.find('{')
        && let Some(b_end) = after.rfind('}')
        && b_start < b_end
    {
        return Some((tag, Some(&after[b_start + 1..b_end])));
    }
    Some((tag, None))
}

fn parse_direction_inverse(fields: &[(String, String)]) -> Result<bool, RenderError> {
    for (k, v) in fields {
        if k == "direction" {
            return match strip_quotes(v).to_ascii_lowercase().as_str() {
                "forward" => Ok(false),
                "inverse" => Ok(true),
                other => Err(RenderError::InvalidLut3D(format!(
                    "invalid OCIO direction '{other}'"
                ))),
            };
        }
    }
    Ok(false)
}

fn parse_f64_list(val: &str) -> Result<Vec<f64>, RenderError> {
    let trimmed = val.trim();
    let inner = if trimmed.starts_with('[') && trimmed.ends_with(']') {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };
    let mut out = Vec::new();
    for tok in split_top_level(inner, ',') {
        let t = tok.trim();
        if t.is_empty() {
            continue;
        }
        let v: f64 = t.parse().map_err(|e| {
            RenderError::InvalidLut3D(format!("invalid float '{t}' in OCIO transform: {e}"))
        })?;
        if !v.is_finite() {
            return Err(RenderError::InvalidLut3D(format!(
                "non-finite float '{t}' in OCIO transform"
            )));
        }
        out.push(v);
    }
    Ok(out)
}

fn build_transform_from_fields(
    tag: &str,
    fields: &[(String, String)],
    children: Vec<OcioTransform>,
) -> Result<OcioTransform, RenderError> {
    let inverse = parse_direction_inverse(fields)?;
    match tag {
        "MatrixTransform" => {
            let mut matrix = [
                1.0, 0.0, 0.0, 0.0, //
                0.0, 1.0, 0.0, 0.0, //
                0.0, 0.0, 1.0, 0.0, //
                0.0, 0.0, 0.0, 1.0,
            ];
            let mut offset = [0.0_f64; 4];
            for (k, v) in fields {
                if k == "matrix" {
                    let list = parse_f64_list(v)?;
                    if list.len() != 16 {
                        return Err(RenderError::InvalidLut3D(format!(
                            "MatrixTransform expects 16 floats, got {}",
                            list.len()
                        )));
                    }
                    matrix.copy_from_slice(&list);
                } else if k == "offset" {
                    let list = parse_f64_list(v)?;
                    if list.len() != 4 {
                        return Err(RenderError::InvalidLut3D(format!(
                            "MatrixTransform offset expects 4 floats, got {}",
                            list.len()
                        )));
                    }
                    offset.copy_from_slice(&list);
                }
            }
            Ok(OcioTransform::Matrix {
                matrix,
                offset,
                inverse,
            })
        }
        "FileTransform" => {
            let mut src = None;
            let mut interpolation = OcioInterpolation::Linear;
            for (k, v) in fields {
                if k == "src" {
                    src = Some(strip_quotes(v).to_owned());
                } else if k == "interpolation" {
                    interpolation = OcioInterpolation::parse(strip_quotes(v))?;
                }
            }
            let src = src.ok_or_else(|| {
                RenderError::InvalidLut3D("FileTransform missing required 'src' field".to_owned())
            })?;
            Ok(OcioTransform::File {
                src,
                interpolation,
                inverse,
            })
        }
        "ExponentTransform" => {
            let mut value = [1.0_f64; 4];
            let mut negative_style = NegativeStyle::Clamp;
            for (k, v) in fields {
                if k == "value" {
                    let list = parse_f64_list(v)?;
                    match list.len() {
                        1 => {
                            value = [list[0], list[0], list[0], 1.0];
                        }
                        3 => {
                            value = [list[0], list[1], list[2], 1.0];
                        }
                        4 => {
                            value.copy_from_slice(&list);
                        }
                        n => {
                            return Err(RenderError::InvalidLut3D(format!(
                                "ExponentTransform value expects 1, 3, or 4 floats, got {n}"
                            )));
                        }
                    }
                } else if k == "negative_style" {
                    negative_style = NegativeStyle::parse(strip_quotes(v))?;
                }
            }
            Ok(OcioTransform::Exponent {
                value,
                negative_style,
                inverse,
            })
        }
        "LogTransform" => {
            let mut base = 2.0_f64;
            for (k, v) in fields {
                if k == "base" {
                    base = strip_quotes(v).parse::<f64>().map_err(|e| {
                        RenderError::InvalidLut3D(format!("invalid LogTransform base '{v}': {e}"))
                    })?;
                }
            }
            Ok(OcioTransform::Log { base, inverse })
        }
        "GroupTransform" => Ok(OcioTransform::Group { children, inverse }),
        other => Ok(OcioTransform::Unsupported {
            tag: other.to_owned(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq_3(a: [f64; 3], b: [f64; 3], tol: f64) {
        for c in 0..3 {
            assert!(
                (a[c] - b[c]).abs() <= tol,
                "channel {c} mismatch: got {}, expected {} (diff={})",
                a[c],
                b[c],
                (a[c] - b[c]).abs()
            );
        }
    }

    #[test]
    fn acescg_and_linear_rec709_matrices_and_white_point() {
        // Equal-energy / neutral grey (1,1,1) maps to (1,1,1) across Bradford D60 <-> D65
        let rec709 = eval_builtin_transform_f64(
            OcioSpace::AcesCg,
            OcioSpace::LinearRec709,
            [1.0, 1.0, 1.0],
        );
        approx_eq_3(rec709, [1.0, 1.0, 1.0], 1e-6);

        let back = eval_builtin_transform_f64(
            OcioSpace::LinearRec709,
            OcioSpace::AcesCg,
            rec709,
        );
        approx_eq_3(back, [1.0, 1.0, 1.0], 1e-6);

        // AP1 <-> AP0 round-trip
        let sample = [0.42, 0.18, 0.77];
        let ap0 = eval_builtin_transform_f64(OcioSpace::AcesCg, OcioSpace::Aces2065_1, sample);
        let ap1 = eval_builtin_transform_f64(OcioSpace::Aces2065_1, OcioSpace::AcesCg, ap0);
        approx_eq_3(ap1, sample, 1e-7);
    }

    #[test]
    fn analytic_transfer_round_trips() {
        let samples = [
            [0.0, 0.002, 0.18],
            [0.18, 0.5, 1.0],
            [0.008, 0.015, 2.5],
        ];
        let spaces = [
            OcioSpace::DisplaySrgb,
            OcioSpace::Rec709,
            OcioSpace::LinearRec2020,
            OcioSpace::Rec2020,
            OcioSpace::Aces2065_1,
            OcioSpace::AcesCg,
            OcioSpace::AcesCct,
            OcioSpace::ArriLogC3,
            OcioSpace::SonySLog3,
        ];

        for &space in &spaces {
            for &lin_in in &samples {
                let encoded =
                    eval_builtin_transform_f64(OcioSpace::LinearRec709, space, lin_in);
                let decoded =
                    eval_builtin_transform_f64(space, OcioSpace::LinearRec709, encoded);
                approx_eq_3(decoded, lin_in, 1e-5);
            }
        }

        // Known 18% grey midpoint sanity checks
        let logc3_grey = linear_awg3_to_logc3_f64(0.18);
        assert!((logc3_grey - 0.3910068).abs() < 1e-4);
        let slog3_grey = linear_sgamut3_to_slog3_f64(0.18);
        assert!((slog3_grey - (420.0 / 1023.0)).abs() < 1e-4);
        let cct_grey = linear_ap1_to_acescct_f64(0.18);
        assert!((cct_grey - 0.4135884).abs() < 1e-4);
    }

    #[test]
    fn ocio_yaml_config_parse_group_file_matrix_inverse_and_bake() {
        let temp_dir = std::env::temp_dir().join("frameshift_ocio_unit_test");
        let luts_dir = temp_dir.join("luts");
        std::fs::create_dir_all(&luts_dir).expect("create temp ocio luts dir");

        // Write a .spi1d gamma 2.0 shaper LUT into luts/gamma2.spi1d
        let mut spi1d = String::from("Version 1\nFrom 0.0 1.0\nLength 17\nComponents 1\n{\n");
        for i in 0..17 {
            let x = i as f32 / 16.0;
            spi1d.push_str(&format!("  {}\n", x * x));
        }
        spi1d.push_str("}\n");
        std::fs::write(luts_dir.join("gamma2.spi1d"), spi1d).expect("write spi1d");

        let ocio_yaml = r#"
ocio_profile_version: 1
search_path: luts

roles:
  scene_linear: linear_709
  default: sRGB_Gamma

colorspaces:
  - !<ColorSpace>
    name: linear_709
    family: Linear
    aliases: [lin709]
    isdata: false

  - !<ColorSpace>
    name: sRGB_Gamma
    family: Display
    from_reference: !<ExponentTransform> {value: [0.4545454545, 0.4545454545, 0.4545454545, 1.0]}

  - !<ColorSpace>
    name: CustomLook
    family: Look
    to_reference: !<GroupTransform>
      children:
        - !<FileTransform>
          src: gamma2.spi1d
          interpolation: linear
        - !<MatrixTransform>
          matrix: [1.0, 0.0, 0.0, 0.0,
                   0.0, 0.5, 0.0, 0.0,
                   0.0, 0.0, 1.0, 0.0,
                   0.0, 0.0, 0.0, 1.0]
          offset: [0.1, 0.0, 0.0, 0.0]
"#;

        let pipeline = OcioColorPipeline::from_str(ocio_yaml, &temp_dir).expect("parse ocio");
        assert_eq!(pipeline.config().profile_version, 1);

        // Role + Alias + Auto-Inverse test: sRGB_Gamma only defines `from_reference`,
        // so `default` -> `scene_linear` automatically inverts ExponentTransform(1/2.2).
        let inv_chain = pipeline
            .build_chain("default", "lin709")
            .expect("auto-inverted chain");
        let lin = inv_chain.eval_rgb([0.5, 0.5, 0.5]);
        let expected_lin = 0.5_f64.powf(2.2);
        approx_eq_3(lin, [expected_lin, expected_lin, expected_lin], 1e-5);

        // GroupTransform (FileTransform .spi1d + MatrixTransform with offset)
        let look_chain = pipeline
            .build_chain("CustomLook", "scene_linear")
            .expect("look chain");
        let mapped = look_chain.eval_rgb([0.5, 0.5, 0.5]);
        // 0.5^2 = 0.25 -> matrix*0.25 + offset -> [0.35, 0.125, 0.25]
        approx_eq_3(mapped, [0.35, 0.125, 0.25], 1e-4);

        // Inverse of GroupTransform (when CustomLook is destination without from_reference)
        let inv_look = pipeline
            .build_chain("scene_linear", "CustomLook")
            .expect("inverse look chain");
        let roundtrip = inv_look.eval_rgb(mapped);
        approx_eq_3(roundtrip, [0.5, 0.5, 0.5], 1e-3);

        // Bake to 3D LUT
        let baked = pipeline
            .bake_to_cube_lut(
                "CustomLook",
                "scene_linear",
                &OcioBakeOptions {
                    lut_size: 17,
                    allow_cli_fallback: false,
                    ..Default::default()
                },
            )
            .expect("bake_to_cube_lut");
        assert_eq!(baked.size, 17);
        let lut_sample = baked.sample_trilinear([0.5, 0.5, 0.5]);
        assert!((lut_sample[0] - 0.35).abs() < 5e-3);
        assert!((lut_sample[1] - 0.125).abs() < 5e-3);
        assert!((lut_sample[2] - 0.25).abs() < 5e-3);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn unsupported_transform_fails_explicitly_without_cli() {
        let yaml = r#"
ocio_profile_version: 2
colorspaces:
  - !<ColorSpace>
    name: ref
  - !<ColorSpace>
    name: exotic
    to_reference: !<GradingToneTransform> {style: log}
"#;
        let pipeline = OcioColorPipeline::from_str(yaml, Path::new(".")).expect("parse ast");
        let err = pipeline
            .bake_to_cube_lut(
                "exotic",
                "ref",
                &OcioBakeOptions {
                    allow_cli_fallback: false,
                    ..Default::default()
                },
            )
            .expect_err("unsupported transform must fail explicitly");
        let msg = err.to_string();
        assert!(msg.contains("GradingToneTransform"), "got: {msg}");
    }

    #[test]
    fn ocio_space_and_extended_color_space_roundtrip() {
        let all = [
            ExtendedColorSpace::LinearRec709,
            ExtendedColorSpace::DisplaySrgb,
            ExtendedColorSpace::Rec709,
            ExtendedColorSpace::LinearRec2020,
            ExtendedColorSpace::Rec2020,
            ExtendedColorSpace::Aces2065_1,
            ExtendedColorSpace::AcesCg,
            ExtendedColorSpace::AcesCct,
            ExtendedColorSpace::ArriLogC3,
            ExtendedColorSpace::SonySLog3,
        ];
        for ext in all {
            let ocio: OcioSpace = ext.into();
            let back: ExtendedColorSpace = ocio.into();
            assert_eq!(back, ext);
        }
    }
}
