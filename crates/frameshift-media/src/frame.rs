use crate::error::MediaError;
use crate::ffmpeg::SourceFit;
use frameshift_time::Flicks;
use std::fs::File;
use std::io::{BufReader, BufWriter, Cursor};
use std::path::Path;

/// Alpha premultiplication convention of a pixel buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AlphaMode {
    /// Straight (unassociated) alpha: `[R, G, B, A]`.
    #[default]
    Straight,
    /// Premultiplied (associated) alpha: `[R*A, G*A, B*A, A]`.
    Premultiplied,
}

/// 8-bit per channel RGBA frame (`sRGB` display-referred by default).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgba8Frame {
    pub width: u32,
    pub height: u32,
    pub pts: Flicks,
    pub alpha_mode: AlphaMode,
    pub data: Vec<u8>,
}

impl Rgba8Frame {
    /// Constructs a validated `Rgba8Frame` from packed `width * height * 4` bytes.
    pub fn new(
        width: u32,
        height: u32,
        pts: Flicks,
        alpha_mode: AlphaMode,
        data: Vec<u8>,
    ) -> Result<Self, MediaError> {
        let expected = validated_byte_len(width, height)?;
        if data.len() != expected {
            return Err(MediaError::BufferLengthMismatch {
                expected,
                actual: data.len(),
            });
        }
        Ok(Self {
            width,
            height,
            pts,
            alpha_mode,
            data,
        })
    }

    /// Creates a solid-color `Rgba8Frame`.
    pub fn filled(
        width: u32,
        height: u32,
        pts: Flicks,
        rgba: [u8; 4],
        alpha_mode: AlphaMode,
    ) -> Result<Self, MediaError> {
        let expected = validated_byte_len(width, height)?;
        let pixel_count = expected / 4;
        let mut data = Vec::with_capacity(expected);
        for _ in 0..pixel_count {
            data.extend_from_slice(&rgba);
        }
        Ok(Self {
            width,
            height,
            pts,
            alpha_mode,
            data,
        })
    }

    /// Returns the total number of pixels (`width * height`).
    #[inline]
    pub fn pixel_count(&self) -> usize {
        (self.width as usize) * (self.height as usize)
    }

    /// Returns a reference to the pixel `[R, G, B, A]` at `(x, y)`, if in bounds.
    #[inline]
    pub fn get_pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let idx = ((y as usize) * (self.width as usize) + (x as usize)) * 4;
        Some([
            self.data[idx],
            self.data[idx + 1],
            self.data[idx + 2],
            self.data[idx + 3],
        ])
    }

    /// Sets the pixel `[R, G, B, A]` at `(x, y)` if in bounds.
    #[inline]
    pub fn set_pixel(&mut self, x: u32, y: u32, rgba: [u8; 4]) {
        if x >= self.width || y >= self.height {
            return;
        }
        let idx = ((y as usize) * (self.width as usize) + (x as usize)) * 4;
        self.data[idx..idx + 4].copy_from_slice(&rgba);
    }

    /// Computes a deterministic 64-bit FNV-1a hash over dimensions, alpha mode, and pixel bytes.
    pub fn fnv1a_hash64(&self) -> u64 {
        let mut h = Fnv1a64::new();
        h.update(&self.width.to_le_bytes());
        h.update(&self.height.to_le_bytes());
        h.update(&[match self.alpha_mode {
            AlphaMode::Straight => 0,
            AlphaMode::Premultiplied => 1,
        }]);
        h.update(&self.data);
        h.finish()
    }

    /// Computes a deterministic 64-bit FNV-1a hash over raw pixel bytes only.
    pub fn pixel_fnv1a_hash64(&self) -> u64 {
        let mut h = Fnv1a64::new();
        h.update(&self.data);
        h.finish()
    }

    /// Computes a deterministic SHA-256 digest over dimensions, alpha mode, and pixel bytes.
    pub fn sha256(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(&self.width.to_le_bytes());
        hasher.update(&self.height.to_le_bytes());
        hasher.update(&[match self.alpha_mode {
            AlphaMode::Straight => 0,
            AlphaMode::Premultiplied => 1,
        }]);
        hasher.update(&self.data);
        hasher.finalize()
    }

    /// Computes a lowercase hex SHA-256 digest string.
    pub fn sha256_hex(&self) -> String {
        bytes_to_hex(&self.sha256())
    }

    /// Converts this 8-bit sRGB frame into a `LinearRec709` premultiplied `Rgba16fFrame`.
    pub fn to_linear_rgba16f(&self) -> Result<Rgba16fFrame, MediaError> {
        Rgba16fFrame::from_srgb_rgba8(self)
    }

    /// Encodes this frame as PNG bytes in memory.
    pub fn encode_png_bytes(&self) -> Result<Vec<u8>, MediaError> {
        let expected = validated_byte_len(self.width, self.height)?;
        if self.data.len() != expected {
            return Err(MediaError::BufferLengthMismatch {
                expected,
                actual: self.data.len(),
            });
        }
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, self.width, self.height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder
                .write_header()
                .map_err(|e| MediaError::PngEncode(e.to_string()))?;
            writer
                .write_image_data(&self.data)
                .map_err(|e| MediaError::PngEncode(e.to_string()))?;
        }
        Ok(out)
    }

    /// Saves this frame to a PNG file on disk.
    pub fn save_png(&self, path: &Path) -> Result<(), MediaError> {
        let expected = validated_byte_len(self.width, self.height)?;
        if self.data.len() != expected {
            return Err(MediaError::BufferLengthMismatch {
                expected,
                actual: self.data.len(),
            });
        }
        let file = File::create(path)?;
        let writer = BufWriter::new(file);
        let mut encoder = png::Encoder::new(writer, self.width, self.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut png_writer = encoder
            .write_header()
            .map_err(|e| MediaError::PngEncode(e.to_string()))?;
        png_writer
            .write_image_data(&self.data)
            .map_err(|e| MediaError::PngEncode(e.to_string()))?;
        Ok(())
    }

    /// Decodes PNG bytes into an `Rgba8Frame` (`AlphaMode::Straight`).
    pub fn decode_png_bytes(bytes: &[u8], pts: Flicks) -> Result<Self, MediaError> {
        decode_png_reader(Cursor::new(bytes), pts)
    }

    /// Loads a PNG file from disk into an `Rgba8Frame` (`AlphaMode::Straight`).
    pub fn load_png(path: &Path, pts: Flicks) -> Result<Self, MediaError> {
        let file = File::open(path)?;
        decode_png_reader(BufReader::new(file), pts)
    }
}

fn decode_png_reader<R: std::io::BufRead + std::io::Seek>(
    reader: R,
    pts: Flicks,
) -> Result<Rgba8Frame, MediaError> {
    let mut decoder = png::Decoder::new(reader);
    decoder.set_transformations(
        png::Transformations::normalize_to_color8() | png::Transformations::ALPHA,
    );
    let mut png_reader = decoder
        .read_info()
        .map_err(|e| MediaError::PngDecode(e.to_string()))?;
    let buf_size = png_reader
        .output_buffer_size()
        .ok_or_else(|| MediaError::PngDecode("PNG output buffer size overflow".into()))?;
    let mut buf = vec![0u8; buf_size];
    let info = png_reader
        .next_frame(&mut buf)
        .map_err(|e| MediaError::PngDecode(e.to_string()))?;
    let raw = &buf[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => raw.to_vec(),
        png::ColorType::Rgb => {
            let mut out = Vec::with_capacity((info.width as usize) * (info.height as usize) * 4);
            for chunk in raw.as_chunks::<3>().0 {
                out.extend_from_slice(&[chunk[0], chunk[1], chunk[2], 255]);
            }
            out
        }
        png::ColorType::GrayscaleAlpha => {
            let mut out = Vec::with_capacity((info.width as usize) * (info.height as usize) * 4);
            for chunk in raw.as_chunks::<2>().0 {
                out.extend_from_slice(&[chunk[0], chunk[0], chunk[0], chunk[1]]);
            }
            out
        }
        png::ColorType::Grayscale => {
            let mut out = Vec::with_capacity((info.width as usize) * (info.height as usize) * 4);
            for &g in raw {
                out.extend_from_slice(&[g, g, g, 255]);
            }
            out
        }
        other => {
            return Err(MediaError::PngDecode(format!(
                "unsupported decoded PNG color type: {other:?}"
            )));
        }
    };
    Rgba8Frame::new(info.width, info.height, pts, AlphaMode::Straight, rgba)
}

/// Resizes and/or crops/pads an `Rgba8Frame` to `(dst_w, dst_h)` according to `fit`.
///
/// Resampling uses bilinear interpolation in premultiplied-alpha space so transparent
/// edges never bleed dark halos into semi-transparent boundaries.
pub fn fit_rgba8_frame(
    src: &Rgba8Frame,
    dst_w: u32,
    dst_h: u32,
    fit: SourceFit,
) -> Result<Rgba8Frame, MediaError> {
    let src_expected = validated_byte_len(src.width, src.height)?;
    if src.data.len() != src_expected {
        return Err(MediaError::BufferLengthMismatch {
            expected: src_expected,
            actual: src.data.len(),
        });
    }
    let dst_expected = validated_byte_len(dst_w, dst_h)?;
    if src.width == dst_w && src.height == dst_h {
        return Ok(src.clone());
    }

    let src_w_f = src.width as f64;
    let src_h_f = src.height as f64;
    let dst_w_f = dst_w as f64;
    let dst_h_f = dst_h as f64;

    let src_pixels = (src.width as usize) * (src.height as usize);
    let mut premul = Vec::with_capacity(src_pixels);
    for px in src.data.as_chunks::<4>().0 {
        let a = (px[3] as f64) / 255.0;
        let (pr, pg, pb) = match src.alpha_mode {
            AlphaMode::Straight => (
                ((px[0] as f64) / 255.0) * a,
                ((px[1] as f64) / 255.0) * a,
                ((px[2] as f64) / 255.0) * a,
            ),
            AlphaMode::Premultiplied => (
                (px[0] as f64) / 255.0,
                (px[1] as f64) / 255.0,
                (px[2] as f64) / 255.0,
            ),
        };
        premul.push([pr, pg, pb, a]);
    }

    let mut out = vec![0u8; dst_expected];
    let max_sx = (src.width - 1) as f64;
    let max_sy = (src.height - 1) as f64;
    let src_w_usize = src.width as usize;
    let src_h_usize = src.height as usize;

    let sample_bilinear = |sx: f64, sy: f64| -> [u8; 4] {
        let sx = sx.clamp(0.0, max_sx);
        let sy = sy.clamp(0.0, max_sy);
        let x0 = sx.floor() as usize;
        let y0 = sy.floor() as usize;
        let x1 = (x0 + 1).min(src_w_usize - 1);
        let y1 = (y0 + 1).min(src_h_usize - 1);
        let fx = sx - (x0 as f64);
        let fy = sy - (y0 as f64);
        let w00 = (1.0 - fx) * (1.0 - fy);
        let w10 = fx * (1.0 - fy);
        let w01 = (1.0 - fx) * fy;
        let w11 = fx * fy;

        let p00 = premul[y0 * src_w_usize + x0];
        let p10 = premul[y0 * src_w_usize + x1];
        let p01 = premul[y1 * src_w_usize + x0];
        let p11 = premul[y1 * src_w_usize + x1];

        let pr = p00[0] * w00 + p10[0] * w10 + p01[0] * w01 + p11[0] * w11;
        let pg = p00[1] * w00 + p10[1] * w10 + p01[1] * w01 + p11[1] * w11;
        let pb = p00[2] * w00 + p10[2] * w10 + p01[2] * w01 + p11[2] * w11;
        let pa = (p00[3] * w00 + p10[3] * w10 + p01[3] * w01 + p11[3] * w11).clamp(0.0, 1.0);

        match src.alpha_mode {
            AlphaMode::Straight => {
                if pa <= 1e-9 {
                    [0, 0, 0, 0]
                } else {
                    let r = (pr / pa).clamp(0.0, 1.0);
                    let g = (pg / pa).clamp(0.0, 1.0);
                    let b = (pb / pa).clamp(0.0, 1.0);
                    [
                        (r * 255.0).round().clamp(0.0, 255.0) as u8,
                        (g * 255.0).round().clamp(0.0, 255.0) as u8,
                        (b * 255.0).round().clamp(0.0, 255.0) as u8,
                        (pa * 255.0).round().clamp(0.0, 255.0) as u8,
                    ]
                }
            }
            AlphaMode::Premultiplied => [
                (pr * 255.0).round().clamp(0.0, 255.0) as u8,
                (pg * 255.0).round().clamp(0.0, 255.0) as u8,
                (pb * 255.0).round().clamp(0.0, 255.0) as u8,
                (pa * 255.0).round().clamp(0.0, 255.0) as u8,
            ],
        }
    };

    let dst_w_usize = dst_w as usize;
    match fit {
        SourceFit::Stretch => {
            let scale_x = src_w_f / dst_w_f;
            let scale_y = src_h_f / dst_h_f;
            for dy in 0..dst_h {
                let sy = ((dy as f64) + 0.5) * scale_y - 0.5;
                let row_off = (dy as usize) * dst_w_usize * 4;
                for dx in 0..dst_w {
                    let sx = ((dx as f64) + 0.5) * scale_x - 0.5;
                    let px = sample_bilinear(sx, sy);
                    let idx = row_off + (dx as usize) * 4;
                    out[idx..idx + 4].copy_from_slice(&px);
                }
            }
        }
        SourceFit::Cover => {
            let scale = (dst_w_f / src_w_f).max(dst_h_f / src_h_f);
            let scaled_w = src_w_f * scale;
            let scaled_h = src_h_f * scale;
            let offset_x = (scaled_w - dst_w_f) * 0.5;
            let offset_y = (scaled_h - dst_h_f) * 0.5;
            let inv_scale = 1.0 / scale;
            for dy in 0..dst_h {
                let sy = ((dy as f64) + 0.5 + offset_y) * inv_scale - 0.5;
                let row_off = (dy as usize) * dst_w_usize * 4;
                for dx in 0..dst_w {
                    let sx = ((dx as f64) + 0.5 + offset_x) * inv_scale - 0.5;
                    let px = sample_bilinear(sx, sy);
                    let idx = row_off + (dx as usize) * 4;
                    out[idx..idx + 4].copy_from_slice(&px);
                }
            }
        }
        SourceFit::Contain => {
            let scale = (dst_w_f / src_w_f).min(dst_h_f / src_h_f);
            let scaled_w = src_w_f * scale;
            let scaled_h = src_h_f * scale;
            let pad_x = (dst_w_f - scaled_w) * 0.5;
            let pad_y = (dst_h_f - scaled_h) * 0.5;
            let end_x = pad_x + scaled_w;
            let end_y = pad_y + scaled_h;
            let inv_scale = 1.0 / scale;
            for dy in 0..dst_h {
                let cy = (dy as f64) + 0.5;
                if cy < pad_y || cy >= end_y {
                    continue;
                }
                let sy = (cy - pad_y) * inv_scale - 0.5;
                let row_off = (dy as usize) * dst_w_usize * 4;
                for dx in 0..dst_w {
                    let cx = (dx as f64) + 0.5;
                    if cx < pad_x || cx >= end_x {
                        continue;
                    }
                    let sx = (cx - pad_x) * inv_scale - 0.5;
                    let px = sample_bilinear(sx, sy);
                    let idx = row_off + (dx as usize) * 4;
                    out[idx..idx + 4].copy_from_slice(&px);
                }
            }
        }
    }

    Rgba8Frame::new(dst_w, dst_h, src.pts, src.alpha_mode, out)
}

/// Linear Rec.709 floating-point RGBA working frame (maps to GPU `Rgba16Float`).
#[derive(Debug, Clone, PartialEq)]
pub struct Rgba16fFrame {
    pub width: u32,
    pub height: u32,
    pub pts: Flicks,
    pub alpha_mode: AlphaMode,
    pub data: Vec<[f32; 4]>,
}

impl Rgba16fFrame {
    /// Constructs a validated `Rgba16fFrame` from `width * height` `[R, G, B, A]` pixels.
    pub fn new(
        width: u32,
        height: u32,
        pts: Flicks,
        alpha_mode: AlphaMode,
        data: Vec<[f32; 4]>,
    ) -> Result<Self, MediaError> {
        let expected = validated_pixel_len(width, height)?;
        if data.len() != expected {
            return Err(MediaError::BufferLengthMismatch {
                expected,
                actual: data.len(),
            });
        }
        Ok(Self {
            width,
            height,
            pts,
            alpha_mode,
            data,
        })
    }

    /// Creates a solid-color `Rgba16fFrame`.
    pub fn filled(
        width: u32,
        height: u32,
        pts: Flicks,
        rgba: [f32; 4],
        alpha_mode: AlphaMode,
    ) -> Result<Self, MediaError> {
        let expected = validated_pixel_len(width, height)?;
        Ok(Self {
            width,
            height,
            pts,
            alpha_mode,
            data: vec![rgba; expected],
        })
    }

    /// Converts an 8-bit `DisplaySrgb` frame into `LinearRec709` `AlphaMode::Premultiplied`.
    pub fn from_srgb_rgba8(frame: &Rgba8Frame) -> Result<Self, MediaError> {
        let pixel_count = validated_pixel_len(frame.width, frame.height)?;
        let expected_bytes = pixel_count
            .checked_mul(4)
            .ok_or(MediaError::InvalidDimensions {
                width: frame.width,
                height: frame.height,
            })?;
        if frame.data.len() != expected_bytes {
            return Err(MediaError::BufferLengthMismatch {
                expected: expected_bytes,
                actual: frame.data.len(),
            });
        }

        let mut data = Vec::with_capacity(pixel_count);
        for px in frame.data.as_chunks::<4>().0 {
            let a = px[3] as f32 / 255.0;
            let (sr, sg, sb) = match frame.alpha_mode {
                AlphaMode::Straight => (
                    px[0] as f32 / 255.0,
                    px[1] as f32 / 255.0,
                    px[2] as f32 / 255.0,
                ),
                AlphaMode::Premultiplied => {
                    if a > 1e-6 {
                        (
                            (px[0] as f32 / 255.0 / a).clamp(0.0, 1.0),
                            (px[1] as f32 / 255.0 / a).clamp(0.0, 1.0),
                            (px[2] as f32 / 255.0 / a).clamp(0.0, 1.0),
                        )
                    } else {
                        (0.0, 0.0, 0.0)
                    }
                }
            };
            let lr = srgb_to_linear(sr) * a;
            let lg = srgb_to_linear(sg) * a;
            let lb = srgb_to_linear(sb) * a;
            data.push([lr, lg, lb, a]);
        }

        Ok(Self {
            width: frame.width,
            height: frame.height,
            pts: frame.pts,
            alpha_mode: AlphaMode::Premultiplied,
            data,
        })
    }

    /// Converts this `LinearRec709` frame into an 8-bit `DisplaySrgb` `Rgba8Frame` (`AlphaMode::Straight`).
    pub fn to_srgb_rgba8(&self) -> Result<Rgba8Frame, MediaError> {
        let pixel_count = validated_pixel_len(self.width, self.height)?;
        if self.data.len() != pixel_count {
            return Err(MediaError::BufferLengthMismatch {
                expected: pixel_count,
                actual: self.data.len(),
            });
        }

        let mut out = Vec::with_capacity(pixel_count * 4);
        for &[r, g, b, a] in &self.data {
            let a_clamped = sanitize_unit(a);
            let (lr, lg, lb) = match self.alpha_mode {
                AlphaMode::Straight => (sanitize_unit(r), sanitize_unit(g), sanitize_unit(b)),
                AlphaMode::Premultiplied => {
                    if a_clamped > 1e-6 {
                        (
                            sanitize_unit(r / a_clamped),
                            sanitize_unit(g / a_clamped),
                            sanitize_unit(b / a_clamped),
                        )
                    } else {
                        (0.0, 0.0, 0.0)
                    }
                }
            };
            out.push((linear_to_srgb(lr) * 255.0).round().clamp(0.0, 255.0) as u8);
            out.push((linear_to_srgb(lg) * 255.0).round().clamp(0.0, 255.0) as u8);
            out.push((linear_to_srgb(lb) * 255.0).round().clamp(0.0, 255.0) as u8);
            out.push((a_clamped * 255.0).round().clamp(0.0, 255.0) as u8);
        }

        Rgba8Frame::new(self.width, self.height, self.pts, AlphaMode::Straight, out)
    }

    /// Computes a deterministic 64-bit FNV-1a hash over dimensions, alpha mode, and canonical float bits.
    pub fn fnv1a_hash64(&self) -> u64 {
        let mut h = Fnv1a64::new();
        h.update(&self.width.to_le_bytes());
        h.update(&self.height.to_le_bytes());
        h.update(&[match self.alpha_mode {
            AlphaMode::Straight => 0,
            AlphaMode::Premultiplied => 1,
        }]);
        for px in &self.data {
            for &c in px {
                let normalized = if c == 0.0 { 0.0f32 } else { c };
                h.update(&normalized.to_bits().to_le_bytes());
            }
        }
        h.finish()
    }
}

/// IEC 61966-2-1 piecewise sRGB EOTF (`sRGB [0,1] -> Linear Rec.709 [0,1]`).
#[inline]
pub fn srgb_to_linear(c: f32) -> f32 {
    let c = sanitize_unit(c);
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// IEC 61966-2-1 piecewise sRGB OETF (`Linear Rec.709 [0,1] -> sRGB [0,1]`).
#[inline]
pub fn linear_to_srgb(c: f32) -> f32 {
    let c = sanitize_unit(c);
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

#[inline]
fn sanitize_unit(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn validated_pixel_len(width: u32, height: u32) -> Result<usize, MediaError> {
    if width == 0 || height == 0 {
        return Err(MediaError::InvalidDimensions { width, height });
    }
    (width as usize)
        .checked_mul(height as usize)
        .ok_or(MediaError::InvalidDimensions { width, height })
}

fn validated_byte_len(width: u32, height: u32) -> Result<usize, MediaError> {
    validated_pixel_len(width, height)?
        .checked_mul(4)
        .ok_or(MediaError::InvalidDimensions { width, height })
}

/// Deterministic 64-bit FNV-1a hasher.
#[derive(Debug, Clone)]
pub(crate) struct Fnv1a64(u64);

impl Fnv1a64 {
    const OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x100000001b3;

    pub fn new() -> Self {
        Self(Self::OFFSET_BASIS)
    }

    pub fn update(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= b as u64;
            self.0 = self.0.wrapping_mul(Self::PRIME);
        }
    }

    pub fn finish(self) -> u64 {
        self.0
    }
}

/// Pure-Rust deterministic SHA-256 implementation.
pub(crate) struct Sha256 {
    state: [u32; 8],
    buffer: [u8; 64],
    buffer_len: usize,
    total_bits: u64,
}

impl Sha256 {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    pub fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            buffer: [0u8; 64],
            buffer_len: 0,
            total_bits: 0,
        }
    }

    pub fn update(&mut self, mut input: &[u8]) {
        self.total_bits = self.total_bits.wrapping_add((input.len() as u64) * 8);
        if self.buffer_len > 0 {
            let take = (64 - self.buffer_len).min(input.len());
            self.buffer[self.buffer_len..self.buffer_len + take].copy_from_slice(&input[..take]);
            self.buffer_len += take;
            input = &input[take..];
            if self.buffer_len == 64 {
                let block = self.buffer;
                self.compress(&block);
                self.buffer_len = 0;
            }
        }
        while input.len() >= 64 {
            let mut block = [0u8; 64];
            block.copy_from_slice(&input[..64]);
            self.compress(&block);
            input = &input[64..];
        }
        if !input.is_empty() {
            self.buffer[..input.len()].copy_from_slice(input);
            self.buffer_len = input.len();
        }
    }

    pub fn finalize(mut self) -> [u8; 32] {
        self.buffer[self.buffer_len] = 0x80;
        self.buffer_len += 1;
        if self.buffer_len > 56 {
            self.buffer[self.buffer_len..].fill(0);
            let block = self.buffer;
            self.compress(&block);
            self.buffer_len = 0;
        }
        self.buffer[self.buffer_len..56].fill(0);
        self.buffer[56..64].copy_from_slice(&self.total_bits.to_be_bytes());
        let block = self.buffer;
        self.compress(&block);

        let mut out = [0u8; 32];
        for (i, word) in self.state.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        out
    }

    fn compress(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
        for (&w_i, &k_i) in w.iter().zip(Self::K.iter()) {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(k_i)
                .wrapping_add(w_i);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
        self.state[5] = self.state[5].wrapping_add(f);
        self.state[6] = self.state[6].wrapping_add(g);
        self.state[7] = self.state[7].wrapping_add(h);
    }
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0f) as usize] as char);
    }
    s
}
