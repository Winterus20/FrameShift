use crate::error::RenderError;
use frameshift_schema::SourceFit;
use frameshift_time::Flicks;

/// YUV color matrix standard for GPU [`YuvToLinearUniform`] conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum YuvColorMatrix {
    #[default]
    Bt709,
    Bt601,
    Bt2020Ncl,
}

impl YuvColorMatrix {
    /// Returns the uniform discriminant expected by `yuv_to_linear.wgsl`.
    #[inline]
    pub const fn shader_mode(self) -> u32 {
        match self {
            Self::Bt709 => 0,
            Self::Bt601 => 1,
            Self::Bt2020Ncl => 2,
        }
    }
}

/// YUV signal quantization range (`tv`/limited vs `pc`/full).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum YuvColorRange {
    #[default]
    Limited,
    Full,
}

impl YuvColorRange {
    /// Returns the uniform discriminant expected by `yuv_to_linear.wgsl`.
    #[inline]
    pub const fn shader_mode(self) -> u32 {
        match self {
            Self::Limited => 0,
            Self::Full => 1,
        }
    }
}

/// Chroma sample siting convention for 4:2:0 subsampled video.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ChromaLocation {
    #[default]
    Left,
    Center,
    TopLeft,
}

impl ChromaLocation {
    /// Returns the uniform discriminant expected by `yuv_to_linear.wgsl`.
    #[inline]
    pub const fn shader_mode(self) -> u32 {
        match self {
            Self::Left => 0,
            Self::Center => 1,
            Self::TopLeft => 2,
        }
    }
}

/// Transfer function applied after YUV->RGB conversion in `yuv_to_linear.wgsl`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum YuvTransfer {
    Linear,
    #[default]
    Srgb,
}

impl YuvTransfer {
    /// Returns the uniform discriminant expected by `yuv_to_linear.wgsl`.
    #[inline]
    pub const fn shader_mode(self) -> u32 {
        match self {
            Self::Linear => 0,
            Self::Srgb => 1,
        }
    }
}

/// Complete YUV colorimetry metadata for GPU NV12 -> Linear Rec.709 conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct YuvColorInfo {
    pub matrix: YuvColorMatrix,
    pub range: YuvColorRange,
    pub chroma_location: ChromaLocation,
    pub transfer: YuvTransfer,
}

/// Raw pixel payload supplied by a [`FrameProvider`].
#[derive(Debug, Clone)]
pub enum SourcePixelData {
    /// 8-bit RGBA pixels (`width * height * 4` bytes).
    Rgba8 { data: Vec<u8>, premultiplied: bool },
    /// 32-bit float RGBA pixels (`width * height` elements).
    RgbaF32 {
        data: Vec<[f32; 4]>,
        premultiplied: bool,
    },
    /// 8-bit 4:2:0 NV12 planar Y + interleaved UV (plus optional R8 alpha plane).
    Nv12 {
        y_plane: Vec<u8>,
        uv_plane: Vec<u8>,
        alpha_plane: Option<Vec<u8>>,
        color_info: YuvColorInfo,
        content_uv_rect: [f32; 4],
    },
}

/// Decoded or synthesized source video/image frame ready for GPU upload.
#[derive(Debug, Clone)]
pub struct SourceFrame {
    pub width: u32,
    pub height: u32,
    pub pixels: SourcePixelData,
}

impl SourceFrame {
    /// Constructs an 8-bit straight-alpha RGBA frame.
    pub fn rgba8_straight(width: u32, height: u32, data: Vec<u8>) -> Self {
        Self {
            width,
            height,
            pixels: SourcePixelData::Rgba8 {
                data,
                premultiplied: false,
            },
        }
    }

    /// Constructs an 8-bit premultiplied-alpha RGBA frame.
    pub fn rgba8_premul(width: u32, height: u32, data: Vec<u8>) -> Self {
        Self {
            width,
            height,
            pixels: SourcePixelData::Rgba8 {
                data,
                premultiplied: true,
            },
        }
    }

    /// Constructs a float32 premultiplied-alpha RGBA frame.
    pub fn rgba_f32_premul(width: u32, height: u32, data: Vec<[f32; 4]>) -> Self {
        Self {
            width,
            height,
            pixels: SourcePixelData::RgbaF32 {
                data,
                premultiplied: true,
            },
        }
    }

    /// Constructs a float32 straight-alpha RGBA frame.
    pub fn rgba_f32_straight(width: u32, height: u32, data: Vec<[f32; 4]>) -> Self {
        Self {
            width,
            height,
            pixels: SourcePixelData::RgbaF32 {
                data,
                premultiplied: false,
            },
        }
    }

    /// Constructs an 8-bit 4:2:0 NV12 frame (`Y` plane + interleaved `UV` plane).
    pub fn nv12(
        width: u32,
        height: u32,
        y_plane: Vec<u8>,
        uv_plane: Vec<u8>,
        color_info: YuvColorInfo,
        content_uv_rect: [f32; 4],
    ) -> Self {
        Self {
            width,
            height,
            pixels: SourcePixelData::Nv12 {
                y_plane,
                uv_plane,
                alpha_plane: None,
                color_info,
                content_uv_rect,
            },
        }
    }

    /// Constructs an 8-bit 4:2:0 NV12 frame with an additional `R8Unorm` alpha plane.
    #[allow(clippy::too_many_arguments)]
    pub fn nv12_with_alpha(
        width: u32,
        height: u32,
        y_plane: Vec<u8>,
        uv_plane: Vec<u8>,
        alpha_plane: Vec<u8>,
        color_info: YuvColorInfo,
        content_uv_rect: [f32; 4],
    ) -> Self {
        Self {
            width,
            height,
            pixels: SourcePixelData::Nv12 {
                y_plane,
                uv_plane,
                alpha_plane: Some(alpha_plane),
                color_info,
                content_uv_rect,
            },
        }
    }
}

/// Decouples video decoding / synthetic frame generation from the GPU compositor.
pub trait FrameProvider {
    /// Provides pixels for `asset_id` at `source_tick` targeting `width x height`.
    fn provide_frame(
        &self,
        asset_id: &str,
        source_tick: Flicks,
        width: u32,
        height: u32,
    ) -> Result<SourceFrame, RenderError>;

    /// Provides pixels for the source node that asked, using that node's fit.
    ///
    /// `fit` is [`frameshift_schema::NodeKind::source_fit`]. Stretch and cover of
    /// the same asset must not share a decoded buffer. The default ignores `fit`
    /// and calls [`Self::provide_frame`], so canvas-sized test providers stay put.
    fn provide_source_frame(
        &self,
        asset_id: &str,
        source_tick: Flicks,
        width: u32,
        height: u32,
        fit: SourceFit,
    ) -> Result<SourceFrame, RenderError> {
        let _ = fit;
        self.provide_frame(asset_id, source_tick, width, height)
    }

    /// Optional stable content identity hash for `(asset_id, source_tick, width, height, fit)`
    /// allowing `SmartCache` to bypass decoding when a `GpuNodeCache` hit occurs.
    fn source_frame_hash(
        &self,
        _asset_id: &str,
        _source_tick: Flicks,
        _width: u32,
        _height: u32,
        _fit: SourceFit,
    ) -> Option<u64> {
        None
    }
}

/// Adapter allowing closures to act as a [`FrameProvider`].
pub struct FnFrameProvider<F>(pub F);

impl<F> FrameProvider for FnFrameProvider<F>
where
    F: Fn(&str, Flicks, u32, u32) -> Result<SourceFrame, RenderError>,
{
    fn provide_frame(
        &self,
        asset_id: &str,
        source_tick: Flicks,
        width: u32,
        height: u32,
    ) -> Result<SourceFrame, RenderError> {
        (self.0)(asset_id, source_tick, width, height)
    }
}
