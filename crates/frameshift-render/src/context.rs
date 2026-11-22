use crate::error::RenderError;

/// Headless `wgpu` device, queue, adapter metadata, and shared linear/trilinear sampler.
pub struct GpuContext {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub linear_sampler: wgpu::Sampler,
    pub adapter_info: wgpu::AdapterInfo,
    pub supports_msaa_4x: bool,
}

impl GpuContext {
    /// Initializes a headless `wgpu` context without a window surface.
    ///
    /// Tries a high-performance hardware adapter first, then any available adapter,
    /// and finally falls back to a software/WARP adapter if needed.
    pub fn new_headless() -> Result<Self, RenderError> {
        let instance = wgpu::Instance::default();

        let adapter = pollster::block_on(request_headless_adapter(&instance))?;
        let adapter_info = adapter.get_info();

        let color_feats =
            adapter.get_texture_format_features(crate::pool::WORKING_TEXTURE_FORMAT);
        let depth_feats = adapter.get_texture_format_features(wgpu::TextureFormat::Depth32Float);
        let supports_msaa_4x = color_feats.flags.contains(
            wgpu::TextureFormatFeatureFlags::MULTISAMPLE_X4
                | wgpu::TextureFormatFeatureFlags::MULTISAMPLE_RESOLVE,
        ) && depth_feats
            .flags
            .contains(wgpu::TextureFormatFeatureFlags::MULTISAMPLE_X4);

        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("frameshift-headless-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                ..Default::default()
            }))?;

        let linear_sampler = create_linear_sampler(&device);

        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            linear_sampler,
            adapter_info,
            supports_msaa_4x,
        })
    }
}

async fn request_headless_adapter(instance: &wgpu::Instance) -> Result<wgpu::Adapter, RenderError> {
    let attempts = [
        (
            "high-performance",
            wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
                ..Default::default()
            },
        ),
        (
            "default",
            wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::None,
                compatible_surface: None,
                force_fallback_adapter: false,
                ..Default::default()
            },
        ),
        (
            "software/fallback",
            wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: None,
                force_fallback_adapter: true,
                ..Default::default()
            },
        ),
    ];

    let mut errors = Vec::new();
    for (label, opts) in attempts {
        match instance.request_adapter(&opts).await {
            Ok(adapter) => return Ok(adapter),
            Err(err) => errors.push(format!("{label}: {err}")),
        }
    }

    Err(RenderError::NoGpuAdapter(errors.join("; ")))
}

fn create_linear_sampler(device: &wgpu::Device) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("frameshift-linear-clamp"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        ..Default::default()
    })
}
