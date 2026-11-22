use crate::error::MediaError;
use crate::ffmpeg::FfmpegBridge;
use crate::frame::Rgba8Frame;
use frameshift_time::{PtsIndex, Rational32};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Rational scaling specification for proxy generation (`scale_num / scale_den`, e.g. `1/2` or `1/4`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProxySpec {
    pub scale_num: u32,
    pub scale_den: u32,
}

impl ProxySpec {
    /// Creates a validated `ProxySpec` with `0 < scale_num <= scale_den`.
    pub fn new(scale_num: u32, scale_den: u32) -> Result<Self, MediaError> {
        if scale_num == 0 || scale_den == 0 || scale_num > scale_den {
            return Err(MediaError::InvalidDimensions {
                width: scale_num,
                height: scale_den,
            });
        }
        Ok(Self {
            scale_num,
            scale_den,
        })
    }

    /// Half-resolution proxy (`1/2`).
    #[inline]
    pub const fn half() -> Self {
        Self {
            scale_num: 1,
            scale_den: 2,
        }
    }

    /// Quarter-resolution proxy (`1/4`).
    #[inline]
    pub const fn quarter() -> Self {
        Self {
            scale_num: 1,
            scale_den: 4,
        }
    }

    /// Computes exact scaled dimensions `(width, height)` clamped to at least `1x1`.
    pub fn scaled_dimensions(
        self,
        src_width: u32,
        src_height: u32,
    ) -> Result<(u32, u32), MediaError> {
        if self.scale_num == 0 || self.scale_den == 0 || self.scale_num > self.scale_den {
            return Err(MediaError::InvalidDimensions {
                width: self.scale_num,
                height: self.scale_den,
            });
        }
        if src_width == 0 || src_height == 0 {
            return Err(MediaError::InvalidDimensions {
                width: src_width,
                height: src_height,
            });
        }
        let w = ((src_width as u64 * self.scale_num as u64) / self.scale_den as u64).max(1) as u32;
        let h = ((src_height as u64 * self.scale_num as u64) / self.scale_den as u64).max(1) as u32;
        Ok((w, h))
    }

    /// Computes scaled dimensions rounded to even pixel counts (`>= 2`) for `yuv420p` H.264 MP4 output.
    pub fn scaled_even_dimensions(
        self,
        src_width: u32,
        src_height: u32,
    ) -> Result<(u32, u32), MediaError> {
        let (w, h) = self.scaled_dimensions(src_width, src_height)?;
        let ew = if w < 2 { 2 } else { w & !1 };
        let eh = if h < 2 { 2 } else { h & !1 };
        Ok((ew, eh))
    }
}

impl Default for ProxySpec {
    fn default() -> Self {
        Self::half()
    }
}

/// Generated or resolved proxy video artifact paired with its exact source `PtsIndex`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyArtifact {
    pub proxy_path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub pts_index: PtsIndex,
}

/// Deterministically resizes an `Rgba8Frame` to `(target_width, target_height)`.
///
/// Uses exact integer box-averaging when downscaling (`target_width <= src_width` and
/// `target_height <= src_height`), falling back to nearest-neighbor sampling if either
/// axis enlarges. Preserves `pts` and `alpha_mode`.
pub fn downscale_frame_nearest_or_box(
    frame: &Rgba8Frame,
    target_width: u32,
    target_height: u32,
) -> Result<Rgba8Frame, MediaError> {
    if frame.width == 0 || frame.height == 0 {
        return Err(MediaError::InvalidDimensions {
            width: frame.width,
            height: frame.height,
        });
    }
    if target_width == 0 || target_height == 0 {
        return Err(MediaError::InvalidDimensions {
            width: target_width,
            height: target_height,
        });
    }
    let expected = (frame.width as usize)
        .checked_mul(frame.height as usize)
        .and_then(|p| p.checked_mul(4))
        .ok_or(MediaError::InvalidDimensions {
            width: frame.width,
            height: frame.height,
        })?;
    if frame.data.len() != expected {
        return Err(MediaError::BufferLengthMismatch {
            expected,
            actual: frame.data.len(),
        });
    }

    if target_width == frame.width && target_height == frame.height {
        return Ok(frame.clone());
    }

    let out_len = (target_width as usize)
        .checked_mul(target_height as usize)
        .and_then(|p| p.checked_mul(4))
        .ok_or(MediaError::InvalidDimensions {
            width: target_width,
            height: target_height,
        })?;
    let mut out = vec![0u8; out_len];

    let src_w = frame.width as usize;
    let src_h = frame.height as usize;
    let dst_w = target_width as usize;
    let dst_h = target_height as usize;

    if dst_w <= src_w && dst_h <= src_h {
        // Exact integer box filter over each destination pixel's source coverage rectangle.
        for dy in 0..dst_h {
            let y0 = (dy * src_h) / dst_h;
            let y1 = (((dy + 1) * src_h) / dst_h).max(y0 + 1).min(src_h);
            for dx in 0..dst_w {
                let x0 = (dx * src_w) / dst_w;
                let x1 = (((dx + 1) * src_w) / dst_w).max(x0 + 1).min(src_w);

                let mut sum_r = 0u64;
                let mut sum_g = 0u64;
                let mut sum_b = 0u64;
                let mut sum_a = 0u64;
                let mut count = 0u64;

                for sy in y0..y1 {
                    let row_base = sy * src_w * 4;
                    for sx in x0..x1 {
                        let idx = row_base + sx * 4;
                        sum_r += frame.data[idx] as u64;
                        sum_g += frame.data[idx + 1] as u64;
                        sum_b += frame.data[idx + 2] as u64;
                        sum_a += frame.data[idx + 3] as u64;
                        count += 1;
                    }
                }

                let half = count / 2;
                let out_idx = (dy * dst_w + dx) * 4;
                out[out_idx] = ((sum_r + half) / count) as u8;
                out[out_idx + 1] = ((sum_g + half) / count) as u8;
                out[out_idx + 2] = ((sum_b + half) / count) as u8;
                out[out_idx + 3] = ((sum_a + half) / count) as u8;
            }
        }
    } else {
        for dy in 0..dst_h {
            let sy = ((dy * src_h) / dst_h).min(src_h - 1);
            for dx in 0..dst_w {
                let sx = ((dx * src_w) / dst_w).min(src_w - 1);
                let src_idx = (sy * src_w + sx) * 4;
                let out_idx = (dy * dst_w + dx) * 4;
                out[out_idx..out_idx + 4].copy_from_slice(&frame.data[src_idx..src_idx + 4]);
            }
        }
    }

    Rgba8Frame::new(
        target_width,
        target_height,
        frame.pts,
        frame.alpha_mode,
        out,
    )
}

/// Manages deterministic scaled proxy MP4 generation and cache resolution while
/// preserving exact source `PtsIndex` timestamps.
#[derive(Debug, Clone, Default)]
pub struct ProxyManager {
    proxy_dir: Option<PathBuf>,
}

impl ProxyManager {
    /// Creates a `ProxyManager` storing generated proxies under `proxy_dir`.
    pub fn new(proxy_dir: impl Into<PathBuf>) -> Self {
        Self {
            proxy_dir: Some(proxy_dir.into()),
        }
    }

    /// Returns the configured proxy root directory, if any.
    pub fn proxy_dir(&self) -> Option<&Path> {
        self.proxy_dir.as_deref()
    }

    /// Deterministically downscales `frame` to `(target_width, target_height)`.
    pub fn downscale_frame_nearest_or_box(
        frame: &Rgba8Frame,
        target_width: u32,
        target_height: u32,
    ) -> Result<Rgba8Frame, MediaError> {
        downscale_frame_nearest_or_box(frame, target_width, target_height)
    }

    /// Deterministically downscales `frame` according to `spec`.
    pub fn downscale_frame(frame: &Rgba8Frame, spec: ProxySpec) -> Result<Rgba8Frame, MediaError> {
        let (w, h) = spec.scaled_dimensions(frame.width, frame.height)?;
        downscale_frame_nearest_or_box(frame, w, h)
    }

    /// Computes the deterministic cache file path for `(asset_id, spec)`.
    pub fn proxy_path_for(&self, asset_id: &str, spec: ProxySpec) -> Result<PathBuf, MediaError> {
        if spec.scale_num == 0 || spec.scale_den == 0 || spec.scale_num > spec.scale_den {
            return Err(MediaError::InvalidDimensions {
                width: spec.scale_num,
                height: spec.scale_den,
            });
        }
        let dir = self
            .proxy_dir
            .clone()
            .unwrap_or_else(|| std::env::temp_dir().join("frameshift_proxies"));
        let safe_id: String = asset_id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let safe_id = if safe_id.is_empty() {
            "proxy".to_owned()
        } else {
            safe_id
        };
        Ok(dir.join(format!(
            "{safe_id}_{}of{}.mp4",
            spec.scale_num, spec.scale_den
        )))
    }

    /// Downscales in-memory/synthetic frames and encodes them into a proxy MP4 at `out_path`,
    /// preserving the exact frame timestamps in `ProxyArtifact::pts_index`.
    pub fn generate_proxy_from_frames(
        frames: &[Rgba8Frame],
        fps: Rational32,
        spec: ProxySpec,
        out_path: &Path,
    ) -> Result<ProxyArtifact, MediaError> {
        let first = frames.first().ok_or_else(|| {
            MediaError::EmptyMediaStream("cannot generate proxy from zero frames".into())
        })?;
        let (pw, ph) = spec.scaled_even_dimensions(first.width, first.height)?;

        let mut pts_list = Vec::with_capacity(frames.len());
        let mut scaled = Vec::with_capacity(frames.len());
        for frame in frames {
            if frame.width != first.width || frame.height != first.height {
                return Err(MediaError::InvalidDimensions {
                    width: frame.width,
                    height: frame.height,
                });
            }
            pts_list.push(frame.pts);
            scaled.push(downscale_frame_nearest_or_box(frame, pw, ph)?);
        }
        let pts_index = PtsIndex::new(pts_list)?;

        if let Some(parent) = out_path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }

        FfmpegBridge::encode_mp4(&scaled, None, fps, out_path)?;

        Ok(ProxyArtifact {
            proxy_path: out_path.to_path_buf(),
            width: pw,
            height: ph,
            pts_index,
        })
    }

    /// Transcodes an on-disk video file at `source_path` into a scaled proxy MP4 at `out_path`,
    /// preserving the exact probed source `PtsIndex`.
    pub fn generate_proxy_mp4(
        source_path: &Path,
        spec: ProxySpec,
        out_path: &Path,
    ) -> Result<ProxyArtifact, MediaError> {
        let probe = FfmpegBridge::probe_media_info(source_path)?;
        let (pw, ph) = spec.scaled_even_dimensions(probe.width, probe.height)?;

        if let Some(parent) = out_path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }

        let vf_scale = format!("scale={pw}:{ph}:flags=area");
        let output = Command::new("ffmpeg")
            .args(["-y", "-v", "error", "-nostdin", "-i"])
            .arg(source_path)
            .args([
                "-vf",
                &vf_scale,
                "-fps_mode",
                "passthrough",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-preset",
                "veryfast",
                "-crf",
                "23",
                "-an",
            ])
            .arg(out_path)
            .output()
            .map_err(|e| MediaError::BinarySpawnFailed {
                binary: "ffmpeg".into(),
                reason: e.to_string(),
            })?;

        if !output.status.success() {
            return Err(MediaError::FfmpegFailed {
                exit_code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            });
        }

        Ok(ProxyArtifact {
            proxy_path: out_path.to_path_buf(),
            width: pw,
            height: ph,
            pts_index: probe.pts_index,
        })
    }

    /// Resolves an existing proxy MP4 on disk for `(asset_id, spec)` if it exists and matches
    /// `expected_pts.len()`, preserving `expected_pts`.
    pub fn resolve_proxy(
        &self,
        asset_id: &str,
        spec: ProxySpec,
        expected_pts: &PtsIndex,
    ) -> Result<Option<ProxyArtifact>, MediaError> {
        let path = self.proxy_path_for(asset_id, spec)?;
        if !path.is_file() {
            return Ok(None);
        }
        let probe = match FfmpegBridge::probe_media_info(&path) {
            Ok(info) => info,
            Err(_) => return Ok(None),
        };
        if probe.pts_index.len() != expected_pts.len() {
            return Ok(None);
        }
        Ok(Some(ProxyArtifact {
            proxy_path: path,
            width: probe.width,
            height: probe.height,
            pts_index: expected_pts.clone(),
        }))
    }

    /// Resolves an existing synthetic proxy or generates one from `frames`.
    pub fn resolve_or_generate_synthetic_proxy(
        &self,
        asset_id: &str,
        frames: &[Rgba8Frame],
        fps: Rational32,
        spec: ProxySpec,
    ) -> Result<ProxyArtifact, MediaError> {
        let pts_list: Vec<_> = frames.iter().map(|f| f.pts).collect();
        let expected_pts = PtsIndex::new(pts_list)?;
        if let Some(existing) = self.resolve_proxy(asset_id, spec, &expected_pts)? {
            return Ok(existing);
        }
        let path = self.proxy_path_for(asset_id, spec)?;
        Self::generate_proxy_from_frames(frames, fps, spec, &path)
    }

    /// Resolves an existing proxy MP4 or generates one from `source_path`.
    pub fn resolve_or_generate_mp4_proxy(
        &self,
        asset_id: &str,
        source_path: &Path,
        spec: ProxySpec,
    ) -> Result<ProxyArtifact, MediaError> {
        let probe = FfmpegBridge::probe_media_info(source_path)?;
        if let Some(existing) = self.resolve_proxy(asset_id, spec, &probe.pts_index)? {
            return Ok(existing);
        }
        let path = self.proxy_path_for(asset_id, spec)?;
        Self::generate_proxy_mp4(source_path, spec, &path)
    }
}
