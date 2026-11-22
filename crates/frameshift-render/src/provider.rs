use crate::error::RenderError;
use frameshift_schema::SourceFit;
use frameshift_time::Flicks;

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
