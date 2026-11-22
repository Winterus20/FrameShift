use crate::error::RenderError;
use crate::ocio::{
    OcioBakeOptions, OcioColorPipeline, OcioSpace, eval_builtin_transform_f64,
};
use frameshift_schema::{ColorSpace, ExtendedColorSpace, LutSourceDef};
use std::path::Path;

pub const MAX_LUT_1D_SIZE: u32 = 65_536;
pub const MAX_LUT_3D_SIZE: u32 = 128;
pub const DEFAULT_PRESET_LUT_SIZE: u32 = 17;
pub const DEFAULT_1D_TO_3D_BAKE_SIZE: u32 = 33;

/// Color space / LUT transform abstraction for `Lut3D` and OCIO integration.
pub trait ColorTransform {
    /// Human-readable transform name.
    fn name(&self) -> &str;

    /// Expected input color space.
    fn input_space(&self) -> ColorSpace;

    /// Output color space produced by this transform.
    fn output_space(&self) -> ColorSpace;

    /// Applies the transform to a straight (un-premultiplied) RGB triplet.
    fn apply_rgb(&self, rgb: [f32; 3]) -> [f32; 3];
}

/// Parsed 1D LUT (standalone `.cube`, `.spi1d`, or 1D shaper preceding a 3D LUT).
#[derive(Debug, Clone, PartialEq)]
pub struct CubeLut1D {
    pub title: String,
    pub size: u32,
    pub domain_min: [f32; 3],
    pub domain_max: [f32; 3],
    pub entries: Vec<[f32; 3]>,
}

impl CubeLut1D {
    /// Evaluates each channel independently via piecewise-linear interpolation (clamped to domain).
    pub fn sample_linear(&self, rgb: [f32; 3]) -> [f32; 3] {
        let n = self.entries.len();
        if n == 0 {
            return rgb;
        }
        if n == 1 {
            return self.entries[0];
        }
        let max_idx = (n - 1) as f32;
        let mut out = [0.0_f32; 3];
        for c in 0..3 {
            let span = (self.domain_max[c] - self.domain_min[c]).max(1e-6);
            let u = ((rgb[c] - self.domain_min[c]) / span).clamp(0.0, 1.0);
            let pos = u * max_idx;
            let i0 = (pos.floor() as usize).min(n - 1);
            let i1 = (i0 + 1).min(n - 1);
            let f = pos - i0 as f32;
            let v0 = self.entries[i0][c];
            let v1 = self.entries[i1][c];
            out[c] = v0 + (v1 - v0) * f;
        }
        out
    }

    /// Bakes this 1D LUT into a separable `grid_size^3` `CubeLut3D`.
    pub fn bake_to_3d(&self, grid_size: u32) -> Result<CubeLut3D, RenderError> {
        if !(2..=MAX_LUT_3D_SIZE).contains(&grid_size) {
            return Err(RenderError::InvalidLut3D(format!(
                "1D->3D bake grid_size must be in 2..={MAX_LUT_3D_SIZE}, got {grid_size}"
            )));
        }
        validate_domain(self.domain_min, self.domain_max)?;
        if self.size < 2 || self.entries.len() != self.size as usize {
            return Err(RenderError::InvalidLut3D(format!(
                "invalid 1D LUT entries: size={}, len={}",
                self.size,
                self.entries.len()
            )));
        }

        let n = grid_size as usize;
        let denom = (grid_size - 1) as f32;
        let span = [
            self.domain_max[0] - self.domain_min[0],
            self.domain_max[1] - self.domain_min[1],
            self.domain_max[2] - self.domain_min[2],
        ];
        let mut data = Vec::with_capacity(n * n * n);
        for b in 0..n {
            let fb = self.domain_min[2] + (b as f32 / denom) * span[2];
            for g in 0..n {
                let fg = self.domain_min[1] + (g as f32 / denom) * span[1];
                for r in 0..n {
                    let fr = self.domain_min[0] + (r as f32 / denom) * span[0];
                    let mapped = self.sample_linear([fr, fg, fb]);
                    data.push([mapped[0], mapped[1], mapped[2], 1.0]);
                }
            }
        }

        Ok(CubeLut3D {
            title: self.title.clone(),
            size: grid_size,
            domain_min: self.domain_min,
            domain_max: self.domain_max,
            data_rgba16f_or_f32: data,
            shaper_1d: None,
            input_space: ColorSpace::LinearRec709,
            output_space: ColorSpace::LinearRec709,
        })
    }

    /// Parses a Sony Pictures Imageworks `.spi1d` 1D LUT text payload.
    pub fn parse_spi1d(text: &str) -> Result<Self, RenderError> {
        let mut from_min = 0.0_f32;
        let mut from_max = 1.0_f32;
        let mut length: Option<u32> = None;
        let mut components: u32 = 1;
        let mut in_block = false;
        let mut closed_block = false;
        let mut raw_values: Vec<f32> = Vec::new();

        for (line_idx, raw_line) in text.lines().enumerate() {
            let line_no = line_idx + 1;
            let stripped = raw_line.split('#').next().unwrap_or("").trim();
            if stripped.is_empty() {
                continue;
            }

            if !in_block {
                if let Some(rest) = strip_keyword(stripped, "Version") {
                    let v: u32 = rest.trim().parse().map_err(|e| {
                        RenderError::InvalidLut3D(format!(
                            "line {line_no}: invalid .spi1d Version '{rest}': {e}"
                        ))
                    })?;
                    if v != 1 {
                        return Err(RenderError::InvalidLut3D(format!(
                            "line {line_no}: unsupported .spi1d Version {v} (expected 1)"
                        )));
                    }
                    continue;
                }
                if let Some(rest) = strip_keyword(stripped, "From") {
                    let mut parts = rest.split_whitespace();
                    let a: f32 = parts
                        .next()
                        .ok_or_else(|| {
                            RenderError::InvalidLut3D(format!(
                                "line {line_no}: .spi1d From missing min"
                            ))
                        })?
                        .parse()
                        .map_err(|e| {
                            RenderError::InvalidLut3D(format!(
                                "line {line_no}: invalid .spi1d From min: {e}"
                            ))
                        })?;
                    let b: f32 = parts
                        .next()
                        .ok_or_else(|| {
                            RenderError::InvalidLut3D(format!(
                                "line {line_no}: .spi1d From missing max"
                            ))
                        })?
                        .parse()
                        .map_err(|e| {
                            RenderError::InvalidLut3D(format!(
                                "line {line_no}: invalid .spi1d From max: {e}"
                            ))
                        })?;
                    if parts.next().is_some() {
                        return Err(RenderError::InvalidLut3D(format!(
                            "line {line_no}: unexpected trailing token in .spi1d From"
                        )));
                    }
                    from_min = a;
                    from_max = b;
                    continue;
                }
                if let Some(rest) = strip_keyword(stripped, "Length") {
                    let len: u32 = rest.trim().parse().map_err(|e| {
                        RenderError::InvalidLut3D(format!(
                            "line {line_no}: invalid .spi1d Length '{rest}': {e}"
                        ))
                    })?;
                    if !(2..=MAX_LUT_1D_SIZE).contains(&len) {
                        return Err(RenderError::InvalidLut3D(format!(
                            "line {line_no}: .spi1d Length must be in 2..={MAX_LUT_1D_SIZE}, got {len}"
                        )));
                    }
                    length = Some(len);
                    continue;
                }
                if let Some(rest) = strip_keyword(stripped, "Components") {
                    let c: u32 = rest.trim().parse().map_err(|e| {
                        RenderError::InvalidLut3D(format!(
                            "line {line_no}: invalid .spi1d Components '{rest}': {e}"
                        ))
                    })?;
                    if c != 1 && c != 3 {
                        return Err(RenderError::InvalidLut3D(format!(
                            "line {line_no}: .spi1d Components must be 1 or 3, got {c}"
                        )));
                    }
                    components = c;
                    continue;
                }
                if let Some(after_open) = stripped.strip_prefix('{') {
                    in_block = true;
                    let body = if let Some(before_close) = after_open.strip_suffix('}') {
                        in_block = false;
                        closed_block = true;
                        before_close
                    } else {
                        after_open
                    };
                    parse_spi1d_tokens(body, line_no, &mut raw_values)?;
                    continue;
                }
                return Err(RenderError::InvalidLut3D(format!(
                    "line {line_no}: unexpected .spi1d header token '{stripped}'"
                )));
            } else {
                let body = if let Some(idx) = stripped.find('}') {
                    in_block = false;
                    closed_block = true;
                    &stripped[..idx]
                } else {
                    stripped
                };
                parse_spi1d_tokens(body, line_no, &mut raw_values)?;
            }
        }

        if in_block || !closed_block {
            return Err(RenderError::InvalidLut3D(
                "unterminated or missing '{ ... }' block in .spi1d".to_owned(),
            ));
        }

        let size = length.ok_or_else(|| {
            RenderError::InvalidLut3D("missing required Length header in .spi1d".to_owned())
        })?;
        let domain_min = [from_min; 3];
        let domain_max = [from_max; 3];
        validate_domain(domain_min, domain_max)?;

        let expected = size as usize * components as usize;
        if raw_values.len() != expected {
            return Err(RenderError::InvalidLut3D(format!(
                "expected {expected} values in .spi1d ({size} x {components}), found {}",
                raw_values.len()
            )));
        }

        let mut entries = Vec::with_capacity(size as usize);
        if components == 1 {
            for v in raw_values {
                entries.push([v, v, v]);
            }
        } else {
            for &chunk in raw_values.as_chunks::<3>().0 {
                entries.push(chunk);
            }
        }

        Ok(Self {
            title: "spi1d".to_owned(),
            size,
            domain_min,
            domain_max,
            entries,
        })
    }
}

fn parse_spi1d_tokens(
    body: &str,
    line_no: usize,
    out: &mut Vec<f32>,
) -> Result<(), RenderError> {
    for tok in body.split_whitespace() {
        let val: f32 = tok.parse().map_err(|e| {
            RenderError::InvalidLut3D(format!(
                "line {line_no}: invalid float '{tok}' in .spi1d data: {e}"
            ))
        })?;
        if !val.is_finite() {
            return Err(RenderError::InvalidLut3D(format!(
                "line {line_no}: non-finite value '{tok}' in .spi1d data"
            )));
        }
        out.push(val);
    }
    Ok(())
}

/// Parsed or synthesized 3D LUT ready for `Rgba16Float` `texture_3d<f32>` upload.
///
/// Data ordering follows the Adobe/Iridas `.cube` specification: R varies fastest,
/// then G, then B (`index = r + g * size + b * size * size`).
#[derive(Debug, Clone, PartialEq)]
pub struct CubeLut3D {
    pub title: String,
    pub size: u32,
    pub domain_min: [f32; 3],
    pub domain_max: [f32; 3],
    pub data_rgba16f_or_f32: Vec<[f32; 4]>,
    /// Optional 1D shaper evaluated before the 3D grid (combined 1D+3D `.cube` files).
    pub shaper_1d: Option<CubeLut1D>,
    pub input_space: ColorSpace,
    pub output_space: ColorSpace,
}

impl CubeLut3D {
    /// Parses an Adobe/Iridas `.cube` payload supporting 3D-only, 1D-only, and combined 1D+3D shaper LUTs.
    pub fn parse_cube(cube_text: &str) -> Result<Self, RenderError> {
        let mut title = String::from("untitled");
        let mut lut_1d_size: Option<u32> = None;
        let mut lut_3d_size: Option<u32> = None;
        let mut domain_min_hdr: Option<[f32; 3]> = None;
        let mut domain_max_hdr: Option<[f32; 3]> = None;
        let mut range_1d: Option<([f32; 3], [f32; 3])> = None;
        let mut range_3d: Option<([f32; 3], [f32; 3])> = None;
        let mut entries: Vec<[f32; 3]> = Vec::new();

        for (line_idx, raw_line) in cube_text.lines().enumerate() {
            let line_no = line_idx + 1;
            let stripped = raw_line.split('#').next().unwrap_or("").trim();
            if stripped.is_empty() {
                continue;
            }

            if let Some(rest) = strip_keyword(stripped, "TITLE") {
                title = rest.trim().trim_matches('"').to_owned();
                continue;
            }
            if let Some(rest) = strip_keyword(stripped, "LUT_1D_SIZE") {
                let size: u32 = rest.trim().parse().map_err(|e| {
                    RenderError::InvalidLut3D(format!(
                        "line {line_no}: invalid LUT_1D_SIZE '{rest}': {e}"
                    ))
                })?;
                if !(2..=MAX_LUT_1D_SIZE).contains(&size) {
                    return Err(RenderError::InvalidLut3D(format!(
                        "line {line_no}: LUT_1D_SIZE must be in 2..={MAX_LUT_1D_SIZE}, got {size}"
                    )));
                }
                lut_1d_size = Some(size);
                continue;
            }
            if let Some(rest) = strip_keyword(stripped, "LUT_3D_SIZE") {
                let size: u32 = rest.trim().parse().map_err(|e| {
                    RenderError::InvalidLut3D(format!(
                        "line {line_no}: invalid LUT_3D_SIZE '{rest}': {e}"
                    ))
                })?;
                if !(2..=MAX_LUT_3D_SIZE).contains(&size) {
                    return Err(RenderError::InvalidLut3D(format!(
                        "line {line_no}: LUT_3D_SIZE must be in 2..={MAX_LUT_3D_SIZE}, got {size}"
                    )));
                }
                lut_3d_size = Some(size);
                continue;
            }
            if let Some(rest) = strip_keyword(stripped, "DOMAIN_MIN") {
                domain_min_hdr = Some(parse_vec3(rest, line_no, "DOMAIN_MIN")?);
                continue;
            }
            if let Some(rest) = strip_keyword(stripped, "DOMAIN_MAX") {
                domain_max_hdr = Some(parse_vec3(rest, line_no, "DOMAIN_MAX")?);
                continue;
            }
            if let Some(rest) = strip_keyword(stripped, "LUT_1D_INPUT_RANGE") {
                range_1d = Some(parse_range2(rest, line_no, "LUT_1D_INPUT_RANGE")?);
                continue;
            }
            if let Some(rest) = strip_keyword(stripped, "LUT_3D_INPUT_RANGE") {
                range_3d = Some(parse_range2(rest, line_no, "LUT_3D_INPUT_RANGE")?);
                continue;
            }
            if strip_keyword(stripped, "LUT_IN_VIDEO_RANGE").is_some()
                || strip_keyword(stripped, "LUT_OUT_VIDEO_RANGE").is_some()
            {
                continue;
            }

            let rgb = parse_vec3(stripped, line_no, "RGB triplet")?;
            entries.push(rgb);
        }

        match (lut_1d_size, lut_3d_size) {
            (None, None) => Err(RenderError::InvalidLut3D(
                "missing required LUT_3D_SIZE or LUT_1D_SIZE header".into(),
            )),
            (Some(m), None) => {
                let dmin = range_1d
                    .map(|r| r.0)
                    .or(domain_min_hdr)
                    .unwrap_or([0.0, 0.0, 0.0]);
                let dmax = range_1d
                    .map(|r| r.1)
                    .or(domain_max_hdr)
                    .unwrap_or([1.0, 1.0, 1.0]);
                validate_domain(dmin, dmax)?;
                if entries.len() != m as usize {
                    return Err(RenderError::InvalidLut3D(format!(
                        "expected {m} RGB entries for 1D LUT, found {}",
                        entries.len()
                    )));
                }
                let lut1d = CubeLut1D {
                    title,
                    size: m,
                    domain_min: dmin,
                    domain_max: dmax,
                    entries,
                };
                lut1d.bake_to_3d(DEFAULT_1D_TO_3D_BAKE_SIZE)
            }
            (None, Some(n)) => {
                let dmin = range_3d
                    .map(|r| r.0)
                    .or(domain_min_hdr)
                    .unwrap_or([0.0, 0.0, 0.0]);
                let dmax = range_3d
                    .map(|r| r.1)
                    .or(domain_max_hdr)
                    .unwrap_or([1.0, 1.0, 1.0]);
                validate_domain(dmin, dmax)?;
                let expected = (n as usize) * (n as usize) * (n as usize);
                if entries.len() != expected {
                    return Err(RenderError::InvalidLut3D(format!(
                        "expected {expected} RGB entries for {n}^3 LUT, found {}",
                        entries.len()
                    )));
                }
                let data = entries
                    .into_iter()
                    .map(|[r, g, b]| [r, g, b, 1.0])
                    .collect();
                Ok(Self {
                    title,
                    size: n,
                    domain_min: dmin,
                    domain_max: dmax,
                    data_rgba16f_or_f32: data,
                    shaper_1d: None,
                    input_space: ColorSpace::LinearRec709,
                    output_space: ColorSpace::LinearRec709,
                })
            }
            (Some(m), Some(n)) => {
                let d1_min = range_1d
                    .map(|r| r.0)
                    .or(domain_min_hdr)
                    .unwrap_or([0.0, 0.0, 0.0]);
                let d1_max = range_1d
                    .map(|r| r.1)
                    .or(domain_max_hdr)
                    .unwrap_or([1.0, 1.0, 1.0]);
                validate_domain(d1_min, d1_max)?;

                let d3_min = range_3d
                    .map(|r| r.0)
                    .or_else(|| if range_1d.is_some() { domain_min_hdr } else { None })
                    .unwrap_or([0.0, 0.0, 0.0]);
                let d3_max = range_3d
                    .map(|r| r.1)
                    .or_else(|| if range_1d.is_some() { domain_max_hdr } else { None })
                    .unwrap_or([1.0, 1.0, 1.0]);
                validate_domain(d3_min, d3_max)?;

                let m_usize = m as usize;
                let expected_3d = (n as usize) * (n as usize) * (n as usize);
                let expected_total = m_usize + expected_3d;
                if entries.len() != expected_total {
                    return Err(RenderError::InvalidLut3D(format!(
                        "expected {expected_total} RGB entries ({m} 1D shaper + {n}^3 3D LUT), found {}",
                        entries.len()
                    )));
                }

                let mut entries_1d = entries;
                let entries_3d = entries_1d.split_off(m_usize);
                let shaper = CubeLut1D {
                    title: format!("{title} (1D shaper)"),
                    size: m,
                    domain_min: d1_min,
                    domain_max: d1_max,
                    entries: entries_1d,
                };
                let data = entries_3d
                    .into_iter()
                    .map(|[r, g, b]| [r, g, b, 1.0])
                    .collect();

                Ok(Self {
                    title,
                    size: n,
                    domain_min: d3_min,
                    domain_max: d3_max,
                    data_rgba16f_or_f32: data,
                    shaper_1d: Some(shaper),
                    input_space: ColorSpace::LinearRec709,
                    output_space: ColorSpace::LinearRec709,
                })
            }
        }
    }

    /// Parses a Sony Pictures Imageworks `.spi3d` 3D LUT payload.
    pub fn parse_spi3d(text: &str) -> Result<Self, RenderError> {
        let mut lines = text
            .lines()
            .enumerate()
            .filter_map(|(idx, raw)| {
                let clean = raw.split('#').next().unwrap_or("").trim();
                if clean.is_empty() {
                    None
                } else {
                    Some((idx + 1, clean))
                }
            });

        let (l1, h1) = lines.next().ok_or_else(|| {
            RenderError::InvalidLut3D("empty .spi3d payload".to_owned())
        })?;
        let mut h1_tokens = h1.split_whitespace();
        match (h1_tokens.next(), h1_tokens.next(), h1_tokens.next()) {
            (Some("SPILUT"), Some("1.0"), None) => {}
            _ => {
                return Err(RenderError::InvalidLut3D(format!(
                    "line {l1}: expected 'SPILUT 1.0' header, got '{h1}'"
                )));
            }
        }

        let (l2, h2) = lines.next().ok_or_else(|| {
            RenderError::InvalidLut3D("missing .spi3d dimension count".to_owned())
        })?;
        if h2.trim() != "3" {
            return Err(RenderError::InvalidLut3D(format!(
                "line {l2}: expected .spi3d dimension '3', got '{h2}'"
            )));
        }

        let (l3, h3) = lines.next().ok_or_else(|| {
            RenderError::InvalidLut3D("missing .spi3d grid dimensions".to_owned())
        })?;
        let dims: Vec<u32> = h3
            .split_whitespace()
            .map(|tok| {
                tok.parse::<u32>().map_err(|e| {
                    RenderError::InvalidLut3D(format!(
                        "line {l3}: invalid .spi3d grid dimension '{tok}': {e}"
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if dims.len() != 3 {
            return Err(RenderError::InvalidLut3D(format!(
                "line {l3}: expected 3 grid dimensions in .spi3d, got {}",
                dims.len()
            )));
        }
        let (nx, ny, nz) = (dims[0], dims[1], dims[2]);
        for (axis, &d) in ["R", "G", "B"].iter().zip(dims.iter()) {
            if !(2..=MAX_LUT_3D_SIZE).contains(&d) {
                return Err(RenderError::InvalidLut3D(format!(
                    "line {l3}: .spi3d {axis} size must be in 2..={MAX_LUT_3D_SIZE}, got {d}"
                )));
            }
        }

        let total = (nx as usize) * (ny as usize) * (nz as usize);
        let mut grid = vec![[0.0_f32; 4]; total];
        let mut seen = vec![false; total];
        let mut count = 0usize;

        for (line_no, row) in lines {
            let mut parts = row.split_whitespace();
            let parse_idx = |tok: Option<&str>, axis: &str, max_v: u32| -> Result<usize, RenderError> {
                let t = tok.ok_or_else(|| {
                    RenderError::InvalidLut3D(format!(
                        "line {line_no}: missing {axis} index in .spi3d"
                    ))
                })?;
                let v: u32 = t.parse().map_err(|e| {
                    RenderError::InvalidLut3D(format!(
                        "line {line_no}: invalid {axis} index '{t}': {e}"
                    ))
                })?;
                if v >= max_v {
                    return Err(RenderError::InvalidLut3D(format!(
                        "line {line_no}: .spi3d {axis} index {v} out of bounds 0..{max_v}"
                    )));
                }
                Ok(v as usize)
            };
            let ix = parse_idx(parts.next(), "R", nx)?;
            let iy = parse_idx(parts.next(), "G", ny)?;
            let iz = parse_idx(parts.next(), "B", nz)?;

            let mut out_rgb = [0.0_f32; 3];
            for (c, slot) in out_rgb.iter_mut().enumerate() {
                let tok = parts.next().ok_or_else(|| {
                    RenderError::InvalidLut3D(format!(
                        "line {line_no}: missing RGB channel {c} in .spi3d"
                    ))
                })?;
                let val: f32 = tok.parse().map_err(|e| {
                    RenderError::InvalidLut3D(format!(
                        "line {line_no}: invalid float '{tok}' in .spi3d: {e}"
                    ))
                })?;
                if !val.is_finite() {
                    return Err(RenderError::InvalidLut3D(format!(
                        "line {line_no}: non-finite float '{tok}' in .spi3d"
                    )));
                }
                *slot = val;
            }
            if let Some(extra) = parts.next() {
                return Err(RenderError::InvalidLut3D(format!(
                    "line {line_no}: unexpected trailing token '{extra}' in .spi3d"
                )));
            }

            let flat = ix + iy * (nx as usize) + iz * (nx as usize) * (ny as usize);
            if seen[flat] {
                return Err(RenderError::InvalidLut3D(format!(
                    "line {line_no}: duplicate .spi3d grid coordinate ({ix}, {iy}, {iz})"
                )));
            }
            seen[flat] = true;
            grid[flat] = [out_rgb[0], out_rgb[1], out_rgb[2], 1.0];
            count += 1;
        }

        if count != total {
            return Err(RenderError::InvalidLut3D(format!(
                "expected {total} entries in .spi3d ({nx}x{ny}x{nz}), found {count}"
            )));
        }

        if nx == ny && ny == nz {
            return Ok(Self {
                title: "spi3d".to_owned(),
                size: nx,
                domain_min: [0.0, 0.0, 0.0],
                domain_max: [1.0, 1.0, 1.0],
                data_rgba16f_or_f32: grid,
                shaper_1d: None,
                input_space: ColorSpace::LinearRec709,
                output_space: ColorSpace::LinearRec709,
            });
        }

        // Resample non-cubic .spi3d onto a cubic grid of size max(nx, ny, nz).
        let target = nx.max(ny).max(nz).min(MAX_LUT_3D_SIZE);
        let t_usize = target as usize;
        let denom = (target - 1) as f32;
        let mut cubic = Vec::with_capacity(t_usize * t_usize * t_usize);
        for b in 0..t_usize {
            let ub = b as f32 / denom;
            for g in 0..t_usize {
                let ug = g as f32 / denom;
                for r in 0..t_usize {
                    let ur = r as f32 / denom;
                    let sampled = sample_rect_grid(
                        &grid,
                        nx as usize,
                        ny as usize,
                        nz as usize,
                        [ur, ug, ub],
                    );
                    cubic.push([sampled[0], sampled[1], sampled[2], 1.0]);
                }
            }
        }

        Ok(Self {
            title: "spi3d".to_owned(),
            size: target,
            domain_min: [0.0, 0.0, 0.0],
            domain_max: [1.0, 1.0, 1.0],
            data_rgba16f_or_f32: cubic,
            shaper_1d: None,
            input_space: ColorSpace::LinearRec709,
            output_space: ColorSpace::LinearRec709,
        })
    }

    /// Generates one of the built-in 3D LUT presets:
    /// `"identity"`, `"anime_cyber_contrast"`, `"warm_sunset"`, `"bleach_bypass"`.
    pub fn from_preset(name: &str) -> Result<Self, RenderError> {
        let size = DEFAULT_PRESET_LUT_SIZE;
        match name {
            "identity" => Self::build_analytic(name, size, |r, g, b| [r, g, b]),
            "anime_cyber_contrast" => Self::build_analytic(name, size, cyber_contrast_grade),
            "warm_sunset" => Self::build_analytic(name, size, warm_sunset_grade),
            "bleach_bypass" => Self::build_analytic(name, size, bleach_bypass_grade),
            other => Err(RenderError::InvalidLut3D(format!(
                "unknown built-in LUT preset '{other}'"
            ))),
        }
    }

    /// Generates an exact identity 3D LUT of dimension `size^3`.
    pub fn identity(size: u32) -> Result<Self, RenderError> {
        if !(2..=MAX_LUT_3D_SIZE).contains(&size) {
            return Err(RenderError::InvalidLut3D(format!(
                "LUT size must be in 2..={MAX_LUT_3D_SIZE}, got {size}"
            )));
        }
        Self::build_analytic("identity", size, |r, g, b| [r, g, b])
    }

    /// Loads a `.cube`, `.spi1d`, or `.spi3d` file from disk based on file extension.
    pub fn resolve_file(path: &Path) -> Result<Self, RenderError> {
        let text = std::fs::read_to_string(path).map_err(|e| {
            RenderError::InvalidLut3D(format!(
                "failed to read LUT file '{}': {e}",
                path.display()
            ))
        })?;
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        match ext.as_str() {
            "spi1d" => CubeLut1D::parse_spi1d(&text)?.bake_to_3d(DEFAULT_1D_TO_3D_BAKE_SIZE),
            "spi3d" => Self::parse_spi3d(&text),
            _ => Self::parse_cube(&text),
        }
    }

    /// Resolves a `lut_preset` string from `NodeKind::Lut3D`:
    /// accepts a built-in preset name, inline `.cube`/`.spi3d` text, or an existing LUT file path.
    /// Combined 1D+3D shaper LUTs are baked into a GPU-compatible 3D grid automatically.
    pub fn resolve(spec: &str) -> Result<Self, RenderError> {
        if matches!(
            spec,
            "identity" | "anime_cyber_contrast" | "warm_sunset" | "bleach_bypass"
        ) {
            return Self::from_preset(spec);
        }
        if spec.contains("LUT_3D_SIZE") || spec.contains("LUT_1D_SIZE") {
            let parsed = Self::parse_cube(spec)?;
            return if parsed.shaper_1d.is_some() {
                parsed.bake_composed(parsed.size.clamp(65, MAX_LUT_3D_SIZE))
            } else {
                Ok(parsed)
            };
        }
        if spec.trim_start().starts_with("SPILUT") {
            return Self::parse_spi3d(spec);
        }
        let path = Path::new(spec);
        if path.is_file() {
            let parsed = Self::resolve_file(path)?;
            return if parsed.shaper_1d.is_some() {
                parsed.bake_composed(parsed.size.clamp(65, MAX_LUT_3D_SIZE))
            } else {
                Ok(parsed)
            };
        }
        Err(RenderError::InvalidLut3D(format!(
            "unknown LUT preset or missing .cube file '{spec}'"
        )))
    }

    /// Bakes a built-in analytic `src_space -> dst_space` color transform into a `lut_size^3` 3D LUT.
    pub fn from_builtin_transform(
        src_space: OcioSpace,
        dst_space: OcioSpace,
        lut_size: u32,
    ) -> Result<Self, RenderError> {
        if !(2..=MAX_LUT_3D_SIZE).contains(&lut_size) {
            return Err(RenderError::InvalidLut3D(format!(
                "BuiltinTransform lut_size must be in 2..={MAX_LUT_3D_SIZE}, got {lut_size}"
            )));
        }
        let n = lut_size as usize;
        let denom = (lut_size - 1) as f64;
        let mut data = Vec::with_capacity(n * n * n);
        for b in 0..n {
            let fb = b as f64 / denom;
            for g in 0..n {
                let fg = g as f64 / denom;
                for r in 0..n {
                    let fr = r as f64 / denom;
                    let mapped = eval_builtin_transform_f64(src_space, dst_space, [fr, fg, fb]);
                    data.push([mapped[0] as f32, mapped[1] as f32, mapped[2] as f32, 1.0]);
                }
            }
        }
        Ok(Self {
            title: format!("{src_space:?} -> {dst_space:?}"),
            size: lut_size,
            domain_min: [0.0, 0.0, 0.0],
            domain_max: [1.0, 1.0, 1.0],
            data_rgba16f_or_f32: data,
            shaper_1d: None,
            input_space: ColorSpace::LinearRec709,
            output_space: ColorSpace::LinearRec709,
        })
    }

    /// Resolves a `NodeKind::Lut3D` source (`lut_source` takes precedence over `lut_preset`)
    /// and optionally bridges `(input_space -> output_space)` into FrameShift's `LinearRec709` space.
    pub fn resolve_source(
        lut_source: Option<&LutSourceDef>,
        lut_preset: &str,
        input_space: Option<ExtendedColorSpace>,
        output_space: Option<ExtendedColorSpace>,
    ) -> Result<Self, RenderError> {
        let base_lut = match lut_source {
            None => Self::resolve(lut_preset)?,
            Some(LutSourceDef::Preset { name }) => Self::from_preset(name)?,
            Some(LutSourceDef::CubeFile {
                path,
                lut_size_override,
            }) => {
                let parsed = Self::resolve_file(Path::new(path))?;
                if let Some(target_size) = lut_size_override {
                    parsed.bake_composed(*target_size)?
                } else if parsed.shaper_1d.is_some() {
                    parsed.bake_composed(parsed.size.clamp(65, MAX_LUT_3D_SIZE))?
                } else {
                    parsed
                }
            }
            Some(LutSourceDef::Ocio {
                config_path,
                src_space,
                dst_space,
                look,
                lut_size,
                shaper_space,
            }) => {
                if let Some(look_name) = look
                    && !look_name.trim().is_empty()
                {
                    return Err(RenderError::InvalidLut3D(format!(
                        "OCIO look '{look_name}' is not supported by OcioColorPipeline"
                    )));
                }
                let pipeline = OcioColorPipeline::from_file(Path::new(config_path))?;
                let opts = OcioBakeOptions {
                    lut_size: *lut_size,
                    shaper_space: shaper_space.clone(),
                    ..Default::default()
                };
                pipeline.bake_to_cube_lut(src_space, dst_space, &opts)?
            }
            Some(LutSourceDef::BuiltinTransform {
                src_space,
                dst_space,
                lut_size,
            }) => {
                Self::from_builtin_transform((*src_space).into(), (*dst_space).into(), *lut_size)?
            }
        };

        let in_space = input_space
            .map(OcioSpace::from)
            .unwrap_or(OcioSpace::LinearRec709);
        let out_space = output_space
            .map(OcioSpace::from)
            .unwrap_or(OcioSpace::LinearRec709);
        compose_color_space_bridge(base_lut, in_space, out_space)
    }

    /// Bakes any attached `shaper_1d` + 3D LUT into a single GPU-uploadable `CubeLut3D`.
    pub fn bake_composed(&self, grid_size: u32) -> Result<Self, RenderError> {
        if !(2..=MAX_LUT_3D_SIZE).contains(&grid_size) {
            return Err(RenderError::InvalidLut3D(format!(
                "bake_composed grid_size must be in 2..={MAX_LUT_3D_SIZE}, got {grid_size}"
            )));
        }
        if self.shaper_1d.is_none() && grid_size == self.size {
            return Ok(self.clone());
        }

        let (dmin, dmax) = match &self.shaper_1d {
            Some(shaper) => (shaper.domain_min, shaper.domain_max),
            None => (self.domain_min, self.domain_max),
        };
        validate_domain(dmin, dmax)?;

        let n = grid_size as usize;
        let denom = (grid_size - 1) as f32;
        let span = [dmax[0] - dmin[0], dmax[1] - dmin[1], dmax[2] - dmin[2]];
        let mut data = Vec::with_capacity(n * n * n);

        for b in 0..n {
            let fb = dmin[2] + (b as f32 / denom) * span[2];
            for g in 0..n {
                let fg = dmin[1] + (g as f32 / denom) * span[1];
                for r in 0..n {
                    let fr = dmin[0] + (r as f32 / denom) * span[0];
                    let mapped = self.sample_trilinear([fr, fg, fb]);
                    data.push([mapped[0], mapped[1], mapped[2], 1.0]);
                }
            }
        }

        Ok(Self {
            title: self.title.clone(),
            size: grid_size,
            domain_min: dmin,
            domain_max: dmax,
            data_rgba16f_or_f32: data,
            shaper_1d: None,
            input_space: self.input_space,
            output_space: self.output_space,
        })
    }

    /// Evaluates the LUT on the CPU (`shaper_1d` first if present, then trilinear 3D lookup).
    pub fn sample_trilinear(&self, rgb: [f32; 3]) -> [f32; 3] {
        let shaped = match &self.shaper_1d {
            Some(shaper) => shaper.sample_linear(rgb),
            None => rgb,
        };
        self.sample_3d_only(shaped)
    }

    fn sample_3d_only(&self, rgb: [f32; 3]) -> [f32; 3] {
        let n = self.size as usize;
        let max_idx = (n - 1) as f32;
        let mut coords = [0.0_f32; 3];
        for c in 0..3 {
            let span = (self.domain_max[c] - self.domain_min[c]).max(1e-6);
            let u = ((rgb[c] - self.domain_min[c]) / span).clamp(0.0, 1.0);
            coords[c] = u * max_idx;
        }

        let x0 = (coords[0].floor() as usize).min(n - 1);
        let y0 = (coords[1].floor() as usize).min(n - 1);
        let z0 = (coords[2].floor() as usize).min(n - 1);
        let x1 = (x0 + 1).min(n - 1);
        let y1 = (y0 + 1).min(n - 1);
        let z1 = (z0 + 1).min(n - 1);

        let fx = coords[0] - x0 as f32;
        let fy = coords[1] - y0 as f32;
        let fz = coords[2] - z0 as f32;

        let fetch = |x: usize, y: usize, z: usize| -> [f32; 4] {
            self.data_rgba16f_or_f32[x + y * n + z * n * n]
        };

        let c000 = fetch(x0, y0, z0);
        let c100 = fetch(x1, y0, z0);
        let c010 = fetch(x0, y1, z0);
        let c110 = fetch(x1, y1, z0);
        let c001 = fetch(x0, y0, z1);
        let c101 = fetch(x1, y0, z1);
        let c011 = fetch(x0, y1, z1);
        let c111 = fetch(x1, y1, z1);

        let mut out = [0.0_f32; 3];
        for i in 0..3 {
            let c00 = c000[i] + (c100[i] - c000[i]) * fx;
            let c10 = c010[i] + (c110[i] - c010[i]) * fx;
            let c01 = c001[i] + (c101[i] - c001[i]) * fx;
            let c11 = c011[i] + (c111[i] - c011[i]) * fx;
            let c0 = c00 + (c10 - c00) * fy;
            let c1 = c01 + (c11 - c01) * fy;
            out[i] = c0 + (c1 - c0) * fz;
        }
        out
    }

    /// Returns `1.0 / (domain_max - domain_min)` per channel.
    pub fn domain_inv_span(&self) -> [f32; 3] {
        [
            1.0 / (self.domain_max[0] - self.domain_min[0]).max(1e-6),
            1.0 / (self.domain_max[1] - self.domain_min[1]).max(1e-6),
            1.0 / (self.domain_max[2] - self.domain_min[2]).max(1e-6),
        ]
    }

    fn build_analytic<F>(title: &str, size: u32, f: F) -> Result<Self, RenderError>
    where
        F: Fn(f32, f32, f32) -> [f32; 3],
    {
        let n = size as usize;
        let denom = (size - 1) as f32;
        let mut data = Vec::with_capacity(n * n * n);
        for b in 0..n {
            let fb = b as f32 / denom;
            for g in 0..n {
                let fg = g as f32 / denom;
                for r in 0..n {
                    let fr = r as f32 / denom;
                    let mapped = f(fr, fg, fb);
                    data.push([
                        mapped[0].max(0.0),
                        mapped[1].max(0.0),
                        mapped[2].max(0.0),
                        1.0,
                    ]);
                }
            }
        }
        Ok(Self {
            title: title.to_owned(),
            size,
            domain_min: [0.0, 0.0, 0.0],
            domain_max: [1.0, 1.0, 1.0],
            data_rgba16f_or_f32: data,
            shaper_1d: None,
            input_space: ColorSpace::LinearRec709,
            output_space: ColorSpace::LinearRec709,
        })
    }
}

impl ColorTransform for CubeLut3D {
    fn name(&self) -> &str {
        &self.title
    }

    fn input_space(&self) -> ColorSpace {
        self.input_space
    }

    fn output_space(&self) -> ColorSpace {
        self.output_space
    }

    fn apply_rgb(&self, rgb: [f32; 3]) -> [f32; 3] {
        self.sample_trilinear(rgb)
    }
}

/// Resolves a `NodeKind::Lut3D` source (`lut_source` or fallback `lut_preset`) into a GPU-ready `CubeLut3D`.
pub fn resolve_lut_source(
    lut_source: Option<&LutSourceDef>,
    lut_preset: &str,
    input_space: Option<ExtendedColorSpace>,
    output_space: Option<ExtendedColorSpace>,
) -> Result<CubeLut3D, RenderError> {
    CubeLut3D::resolve_source(lut_source, lut_preset, input_space, output_space)
}

/// Bridges a `CubeLut3D` authored for `(in_space -> out_space)` into FrameShift's
/// `LinearRec709` DAG working space by composing `LinearRec709 -> in_space -> LUT -> out_space -> LinearRec709`.
pub fn compose_color_space_bridge(
    lut: CubeLut3D,
    in_space: OcioSpace,
    out_space: OcioSpace,
) -> Result<CubeLut3D, RenderError> {
    if in_space == OcioSpace::LinearRec709 && out_space == OcioSpace::LinearRec709 {
        return Ok(lut);
    }

    let grid_size = lut.size.clamp(33, MAX_LUT_3D_SIZE);
    let (dmin, dmax) = if in_space == OcioSpace::LinearRec709 {
        match &lut.shaper_1d {
            Some(shaper) => (shaper.domain_min, shaper.domain_max),
            None => (lut.domain_min, lut.domain_max),
        }
    } else {
        ([0.0_f32, 0.0, 0.0], [1.0_f32, 1.0, 1.0])
    };
    validate_domain(dmin, dmax)?;

    let n = grid_size as usize;
    let denom = (grid_size - 1) as f64;
    let span = [
        (dmax[0] - dmin[0]) as f64,
        (dmax[1] - dmin[1]) as f64,
        (dmax[2] - dmin[2]) as f64,
    ];
    let mut data = Vec::with_capacity(n * n * n);

    for b in 0..n {
        let fb = dmin[2] as f64 + (b as f64 / denom) * span[2];
        for g in 0..n {
            let fg = dmin[1] as f64 + (g as f64 / denom) * span[1];
            for r in 0..n {
                let fr = dmin[0] as f64 + (r as f64 / denom) * span[0];
                let adapted_in =
                    eval_builtin_transform_f64(OcioSpace::LinearRec709, in_space, [fr, fg, fb]);
                let lut_out = lut.sample_trilinear([
                    adapted_in[0] as f32,
                    adapted_in[1] as f32,
                    adapted_in[2] as f32,
                ]);
                let adapted_out = eval_builtin_transform_f64(
                    out_space,
                    OcioSpace::LinearRec709,
                    [lut_out[0] as f64, lut_out[1] as f64, lut_out[2] as f64],
                );
                data.push([
                    adapted_out[0] as f32,
                    adapted_out[1] as f32,
                    adapted_out[2] as f32,
                    1.0,
                ]);
            }
        }
    }

    Ok(CubeLut3D {
        title: lut.title,
        size: grid_size,
        domain_min: dmin,
        domain_max: dmax,
        data_rgba16f_or_f32: data,
        shaper_1d: None,
        input_space: ColorSpace::LinearRec709,
        output_space: ColorSpace::LinearRec709,
    })
}

fn validate_domain(domain_min: [f32; 3], domain_max: [f32; 3]) -> Result<(), RenderError> {
    for c in 0..3 {
        if !domain_min[c].is_finite()
            || !domain_max[c].is_finite()
            || domain_max[c] <= domain_min[c]
        {
            return Err(RenderError::InvalidLut3D(format!(
                "invalid domain on channel {c}: min={}, max={}",
                domain_min[c], domain_max[c]
            )));
        }
    }
    Ok(())
}

fn sample_rect_grid(
    grid: &[[f32; 4]],
    nx: usize,
    ny: usize,
    nz: usize,
    uvw: [f32; 3],
) -> [f32; 3] {
    let cx = uvw[0].clamp(0.0, 1.0) * (nx - 1) as f32;
    let cy = uvw[1].clamp(0.0, 1.0) * (ny - 1) as f32;
    let cz = uvw[2].clamp(0.0, 1.0) * (nz - 1) as f32;

    let x0 = (cx.floor() as usize).min(nx - 1);
    let y0 = (cy.floor() as usize).min(ny - 1);
    let z0 = (cz.floor() as usize).min(nz - 1);
    let x1 = (x0 + 1).min(nx - 1);
    let y1 = (y0 + 1).min(ny - 1);
    let z1 = (z0 + 1).min(nz - 1);

    let fx = cx - x0 as f32;
    let fy = cy - y0 as f32;
    let fz = cz - z0 as f32;

    let fetch = |x: usize, y: usize, z: usize| -> [f32; 4] { grid[x + y * nx + z * nx * ny] };
    let c000 = fetch(x0, y0, z0);
    let c100 = fetch(x1, y0, z0);
    let c010 = fetch(x0, y1, z0);
    let c110 = fetch(x1, y1, z0);
    let c001 = fetch(x0, y0, z1);
    let c101 = fetch(x1, y0, z1);
    let c011 = fetch(x0, y1, z1);
    let c111 = fetch(x1, y1, z1);

    let mut out = [0.0_f32; 3];
    for i in 0..3 {
        let c00 = c000[i] + (c100[i] - c000[i]) * fx;
        let c10 = c010[i] + (c110[i] - c010[i]) * fx;
        let c01 = c001[i] + (c101[i] - c001[i]) * fx;
        let c11 = c011[i] + (c111[i] - c011[i]) * fx;
        let c0 = c00 + (c10 - c00) * fy;
        let c1 = c01 + (c11 - c01) * fy;
        out[i] = c0 + (c1 - c0) * fz;
    }
    out
}

fn strip_keyword<'a>(line: &'a str, keyword: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(keyword)?;
    if rest.is_empty() || rest.starts_with(char::is_whitespace) {
        Some(rest)
    } else {
        None
    }
}

fn parse_range2(
    text: &str,
    line_no: usize,
    label: &str,
) -> Result<([f32; 3], [f32; 3]), RenderError> {
    let mut parts = text.split_whitespace();
    let min_tok = parts.next().ok_or_else(|| {
        RenderError::InvalidLut3D(format!("line {line_no}: expected 2 floats for {label}"))
    })?;
    let max_tok = parts.next().ok_or_else(|| {
        RenderError::InvalidLut3D(format!("line {line_no}: expected 2 floats for {label}"))
    })?;
    if let Some(extra) = parts.next() {
        return Err(RenderError::InvalidLut3D(format!(
            "line {line_no}: unexpected trailing token '{extra}' in {label}"
        )));
    }
    let lo: f32 = min_tok.parse().map_err(|e| {
        RenderError::InvalidLut3D(format!(
            "line {line_no}: invalid float '{min_tok}' in {label}: {e}"
        ))
    })?;
    let hi: f32 = max_tok.parse().map_err(|e| {
        RenderError::InvalidLut3D(format!(
            "line {line_no}: invalid float '{max_tok}' in {label}: {e}"
        ))
    })?;
    if !lo.is_finite() || !hi.is_finite() {
        return Err(RenderError::InvalidLut3D(format!(
            "line {line_no}: non-finite value in {label}"
        )));
    }
    Ok(([lo, lo, lo], [hi, hi, hi]))
}

fn parse_vec3(text: &str, line_no: usize, label: &str) -> Result<[f32; 3], RenderError> {
    let mut parts = text.split_whitespace();
    let mut out = [0.0_f32; 3];
    for (idx, slot) in out.iter_mut().enumerate() {
        let token = parts.next().ok_or_else(|| {
            RenderError::InvalidLut3D(format!(
                "line {line_no}: expected 3 floats for {label}, found {idx}"
            ))
        })?;
        let val: f32 = token.parse().map_err(|e| {
            RenderError::InvalidLut3D(format!(
                "line {line_no}: invalid float '{token}' in {label}: {e}"
            ))
        })?;
        if !val.is_finite() {
            return Err(RenderError::InvalidLut3D(format!(
                "line {line_no}: non-finite value '{token}' in {label}"
            )));
        }
        *slot = val;
    }
    if let Some(extra) = parts.next() {
        return Err(RenderError::InvalidLut3D(format!(
            "line {line_no}: unexpected trailing token '{extra}' in {label}"
        )));
    }
    Ok(out)
}

fn s_curve(x: f32, strength: f32) -> f32 {
    let clamped = x.clamp(0.0, 1.0);
    let smooth = clamped * clamped * (3.0 - 2.0 * clamped);
    clamped + (smooth - clamped) * strength
}

fn luma_rec709(r: f32, g: f32, b: f32) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

fn cyber_contrast_grade(r: f32, g: f32, b: f32) -> [f32; 3] {
    // S-curve contrast + cyan shadows / magenta-warm highlights in linear space
    let cr = s_curve(r, 0.55);
    let cg = s_curve(g, 0.50);
    let cb = s_curve(b, 0.55);

    let l = luma_rec709(cr, cg, cb);
    let shadow_w = (1.0 - (l * 2.2).clamp(0.0, 1.0)).powf(1.5);
    let hi_w = ((l - 0.35) / 0.65).clamp(0.0, 1.0).powf(1.3);

    // Subtle vibrancy boost around luma
    let sat = 1.18;
    let mut nr = l + (cr - l) * sat;
    let mut ng = l + (cg - l) * sat;
    let mut nb = l + (cb - l) * sat;

    // Cyan shadow push (-R, +G, +B) + warm magenta highlight push (+R, -G*0.02, +B)
    nr = nr * (1.0 - 0.14 * shadow_w) + 0.08 * hi_w;
    ng = ng + 0.045 * shadow_w * (1.0 - ng) - 0.025 * hi_w * ng;
    nb = nb + 0.095 * shadow_w * (1.0 - nb) + 0.06 * hi_w;

    [nr.clamp(0.0, 1.0), ng.clamp(0.0, 1.0), nb.clamp(0.0, 1.0)]
}

fn warm_sunset_grade(r: f32, g: f32, b: f32) -> [f32; 3] {
    let cr = s_curve(r, 0.30);
    let cg = s_curve(g, 0.25);
    let cb = s_curve(b, 0.25);
    let l = luma_rec709(cr, cg, cb);
    let hi_w = l.clamp(0.0, 1.0);
    let shadow_w = 1.0 - hi_w;

    let nr = (cr * (1.06 + 0.12 * hi_w) + 0.02 * shadow_w).clamp(0.0, 1.0);
    let ng = (cg * (0.98 + 0.04 * hi_w)).clamp(0.0, 1.0);
    let nb = (cb * (0.82 - 0.10 * hi_w) + 0.03 * shadow_w).clamp(0.0, 1.0);
    [nr, ng, nb]
}

fn bleach_bypass_grade(r: f32, g: f32, b: f32) -> [f32; 3] {
    let l = luma_rec709(r, g, b);
    let cl = s_curve(l, 0.70);
    let mix_amt = 0.62;
    let nr = (r + (cl - r) * mix_amt) * 0.98;
    let ng = (g + (cl - g) * mix_amt) * 0.99;
    let nb = (b + (cl - b) * mix_amt) * 1.03;
    [
        s_curve(nr, 0.25).clamp(0.0, 1.0),
        s_curve(ng, 0.25).clamp(0.0, 1.0),
        s_curve(nb, 0.25).clamp(0.0, 1.0),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cube_1d_only_and_truncated_1d_behave_correctly() {
        // Truncated 1D LUT must return InvalidLut3D
        assert!(matches!(
            CubeLut3D::parse_cube("LUT_1D_SIZE 16\n"),
            Err(RenderError::InvalidLut3D(_))
        ));

        // Valid 1D LUT (invert curve)
        let mut cube = String::from("TITLE \"Invert1D\"\nLUT_1D_SIZE 5\nDOMAIN_MIN 0 0 0\nDOMAIN_MAX 1 1 1\n");
        for i in 0..5 {
            let v = 1.0 - (i as f32 / 4.0);
            cube.push_str(&format!("{v} {v} {v}\n"));
        }
        let lut = CubeLut3D::parse_cube(&cube).expect("valid 1D .cube");
        let out = lut.sample_trilinear([0.25, 0.5, 0.75]);
        assert!((out[0] - 0.75).abs() < 1e-4);
        assert!((out[1] - 0.50).abs() < 1e-4);
        assert!((out[2] - 0.25).abs() < 1e-4);
    }

    #[test]
    fn combined_1d_shaper_and_3d_cube_preserves_shaper_and_bakes() {
        let mut cube = String::from(
            "TITLE \"ShaperPlus3D\"\n\
             LUT_1D_SIZE 5\n\
             LUT_1D_INPUT_RANGE -0.5 1.5\n\
             LUT_3D_SIZE 2\n\
             LUT_3D_INPUT_RANGE 0.0 1.0\n",
        );
        // 1D shaper maps [-0.5, 1.5] linearly to [0.0, 1.0]
        for i in 0..5 {
            let v = i as f32 / 4.0;
            cube.push_str(&format!("{v} {v} {v}\n"));
        }
        // 3D LUT scales [0,1]^3 by (0.8, 0.6, 0.4)
        for b in 0..2 {
            for g in 0..2 {
                for r in 0..2 {
                    cube.push_str(&format!(
                        "{} {} {}\n",
                        r as f32 * 0.8,
                        g as f32 * 0.6,
                        b as f32 * 0.4
                    ));
                }
            }
        }

        let parsed = CubeLut3D::parse_cube(&cube).expect("combined 1D+3D .cube");
        assert!(parsed.shaper_1d.is_some());
        // Input 0.5 in [-0.5, 1.5] -> shaped 0.5 -> 3D output [0.4, 0.3, 0.2]
        let exact = parsed.sample_trilinear([0.5, 0.5, 0.5]);
        assert!((exact[0] - 0.4).abs() < 1e-4);
        assert!((exact[1] - 0.3).abs() < 1e-4);
        assert!((exact[2] - 0.2).abs() < 1e-4);

        // Input -0.25 -> shaped 0.125 -> 3D output [0.1, 0.075, 0.05]
        let neg_in = parsed.sample_trilinear([-0.25, -0.25, -0.25]);
        assert!((neg_in[0] - 0.1).abs() < 1e-4);
        assert!((neg_in[1] - 0.075).abs() < 1e-4);
        assert!((neg_in[2] - 0.05).abs() < 1e-4);

        let baked = parsed.bake_composed(17).expect("bake_composed");
        assert!(baked.shaper_1d.is_none());
        assert_eq!(baked.domain_min, [-0.5, -0.5, -0.5]);
        assert_eq!(baked.domain_max, [1.5, 1.5, 1.5]);
        let baked_neg = baked.sample_trilinear([-0.25, -0.25, -0.25]);
        assert!((baked_neg[0] - 0.1).abs() < 1e-3);
        assert!((baked_neg[1] - 0.075).abs() < 1e-3);
        assert!((baked_neg[2] - 0.05).abs() < 1e-3);
    }

    #[test]
    fn spi1d_and_spi3d_parsing() {
        let spi1d = "Version 1\nFrom -0.2 1.2\nLength 3\nComponents 3\n{\n0.0 0.0 0.0\n0.5 0.25 0.75\n1.0 1.0 1.0\n}\n";
        let lut1 = CubeLut1D::parse_spi1d(spi1d).expect("parse spi1d");
        let mid = lut1.sample_linear([0.5, 0.5, 0.5]);
        assert!((mid[0] - 0.5).abs() < 1e-5);
        assert!((mid[1] - 0.25).abs() < 1e-5);
        assert!((mid[2] - 0.75).abs() < 1e-5);

        // Non-cubic 2x3x2 .spi3d
        let mut spi3d = String::from("SPILUT 1.0\n3\n2 3 2\n");
        for iz in 0..2 {
            for iy in 0..3 {
                for ix in 0..2 {
                    spi3d.push_str(&format!(
                        "{ix} {iy} {iz} {} {} {}\n",
                        ix as f32,
                        iy as f32 / 2.0,
                        iz as f32
                    ));
                }
            }
        }
        let lut3 = CubeLut3D::parse_spi3d(&spi3d).expect("parse spi3d");
        assert_eq!(lut3.size, 3);
        let sample = lut3.sample_trilinear([0.3, 0.6, 0.9]);
        assert!((sample[0] - 0.3).abs() < 1e-4);
        assert!((sample[1] - 0.6).abs() < 1e-4);
        assert!((sample[2] - 0.9).abs() < 1e-4);
    }

    #[test]
    fn color_space_bridge_preserves_identity_across_srgb_and_logc3() {
        let id = CubeLut3D::identity(17).expect("id");
        let bridged_srgb =
            compose_color_space_bridge(id.clone(), OcioSpace::DisplaySrgb, OcioSpace::DisplaySrgb)
                .expect("bridge srgb");
        let out = bridged_srgb.sample_trilinear([0.18, 0.5, 0.82]);
        assert!((out[0] - 0.18).abs() < 1e-3);
        assert!((out[1] - 0.50).abs() < 1e-3);
        assert!((out[2] - 0.82).abs() < 1e-3);

        let bridged_logc =
            compose_color_space_bridge(id, OcioSpace::ArriLogC3, OcioSpace::ArriLogC3)
                .expect("bridge logc3");
        let out_logc = bridged_logc.sample_trilinear([0.18, 0.5, 0.8]);
        assert!((out_logc[0] - 0.18).abs() < 5e-3);
        assert!((out_logc[1] - 0.50).abs() < 5e-3);
        assert!((out_logc[2] - 0.80).abs() < 5e-3);
    }

    #[test]
    fn resolve_lut_source_handles_preset_builtin_cubefile_and_ocio() {
        // Legacy fallback when lut_source is None
        let legacy = resolve_lut_source(None, "identity", None, None).expect("legacy preset");
        assert_eq!(legacy.size, DEFAULT_PRESET_LUT_SIZE);

        // Structured Preset with ExtendedColorSpace bridge
        let preset = CubeLut3D::resolve_source(
            Some(&LutSourceDef::Preset {
                name: "identity".to_owned(),
            }),
            "",
            Some(ExtendedColorSpace::DisplaySrgb),
            Some(ExtendedColorSpace::DisplaySrgb),
        )
        .expect("structured preset");
        let bridged = preset.sample_trilinear([0.22, 0.45, 0.78]);
        assert!((bridged[0] - 0.22).abs() < 2e-3);
        assert!((bridged[1] - 0.45).abs() < 2e-3);
        assert!((bridged[2] - 0.78).abs() < 2e-3);

        // BuiltinTransform
        let builtin = resolve_lut_source(
            Some(&LutSourceDef::BuiltinTransform {
                src_space: ExtendedColorSpace::DisplaySrgb,
                dst_space: ExtendedColorSpace::LinearRec709,
                lut_size: 33,
            }),
            "",
            None,
            None,
        )
        .expect("builtin transform");
        assert_eq!(builtin.size, 33);
        let lin = builtin.sample_trilinear([0.5, 0.5, 0.5]);
        assert!((lin[0] - 0.214041).abs() < 2e-3);

        // CubeFile with lut_size_override + Ocio config
        let temp_dir = std::env::temp_dir().join("frameshift_lut_source_test");
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");
        let cube_path = temp_dir.join("id2.cube");
        let mut cube = String::from("LUT_3D_SIZE 2\n");
        for b in 0..2 {
            for g in 0..2 {
                for r in 0..2 {
                    cube.push_str(&format!("{r}.0 {g}.0 {b}.0\n"));
                }
            }
        }
        std::fs::write(&cube_path, cube).expect("write cube");

        let resized = resolve_lut_source(
            Some(&LutSourceDef::CubeFile {
                path: cube_path.to_string_lossy().into_owned(),
                lut_size_override: Some(9),
            }),
            "",
            None,
            None,
        )
        .expect("cube file with size override");
        assert_eq!(resized.size, 9);
        let out_cube = resized.sample_trilinear([0.25, 0.5, 0.75]);
        assert!((out_cube[0] - 0.25).abs() < 1e-4);
        assert!((out_cube[1] - 0.50).abs() < 1e-4);
        assert!((out_cube[2] - 0.75).abs() < 1e-4);

        let ocio_path = temp_dir.join("test.ocio");
        let ocio_yaml = r#"
ocio_profile_version: 1
colorspaces:
  - !<ColorSpace>
    name: lin
  - !<ColorSpace>
    name: half
    from_reference: !<MatrixTransform> {matrix: [0.5, 0, 0, 0,  0, 0.5, 0, 0,  0, 0, 0.5, 0,  0, 0, 0, 1]}
"#;
        std::fs::write(&ocio_path, ocio_yaml).expect("write ocio");
        let ocio_lut = resolve_lut_source(
            Some(&LutSourceDef::Ocio {
                config_path: ocio_path.to_string_lossy().into_owned(),
                src_space: "lin".to_owned(),
                dst_space: "half".to_owned(),
                look: None,
                lut_size: 9,
                shaper_space: None,
            }),
            "",
            None,
            None,
        )
        .expect("ocio source");
        assert_eq!(ocio_lut.size, 9);
        let scaled = ocio_lut.sample_trilinear([0.8, 0.6, 0.4]);
        assert!((scaled[0] - 0.4).abs() < 1e-3);
        assert!((scaled[1] - 0.3).abs() < 1e-3);
        assert!((scaled[2] - 0.2).abs() < 1e-3);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
