use std::collections::HashMap;

/// Working texture format for all intermediate DAG passes (`Linear Rec.709`, premultiplied alpha).
pub const WORKING_TEXTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Computes the full mipmap chain level count for a 2D texture of `(width, height)`.
pub fn compute_mip_levels(width: u32, height: u32) -> u32 {
    32 - width.max(height).max(1).leading_zeros()
}

/// Reusable GPU texture leased from `TexturePool`.
#[derive(Debug)]
pub struct PooledTexture {
    pub width: u32,
    pub height: u32,
    pub mip_level_count: u32,
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
}

/// Statistics for `TexturePool` allocations vs cache hits.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TexturePoolStats {
    pub allocations: u64,
    pub reuses: u64,
}

/// Pool of reusable `Rgba16Float` render/sample textures keyed by `(width, height, mip_level_count)`.
#[derive(Debug, Default)]
pub struct TexturePool {
    free: HashMap<(u32, u32, u32), Vec<PooledTexture>>,
    stats: TexturePoolStats,
}

impl TexturePool {
    pub fn new() -> Self {
        Self::default()
    }

    /// Acquires a single-mip `(width, height)` `Rgba16Float` texture usable as both a render attachment
    /// and sampled texture binding.
    pub fn acquire(&mut self, device: &wgpu::Device, width: u32, height: u32) -> PooledTexture {
        self.acquire_with_mips(device, width, height, 1)
    }

    /// Acquires a full-mip-chain `(width, height)` `Rgba16Float` texture usable for trilinear filtering.
    pub fn acquire_mipmapped(
        &mut self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
    ) -> PooledTexture {
        let mips = compute_mip_levels(width, height);
        self.acquire_with_mips(device, width, height, mips)
    }

    /// Acquires a `(width, height, mip_level_count)` `Rgba16Float` texture.
    pub fn acquire_with_mips(
        &mut self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
        mip_level_count: u32,
    ) -> PooledTexture {
        let mips = mip_level_count.max(1);
        if let Some(bucket) = self.free.get_mut(&(width, height, mips))
            && let Some(tex) = bucket.pop()
        {
            self.stats.reuses += 1;
            return tex;
        }

        self.stats.allocations += 1;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("frameshift-pool-rgba16f"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: mips,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: WORKING_TEXTURE_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        PooledTexture {
            width,
            height,
            mip_level_count: mips,
            texture,
            view,
        }
    }

    /// Returns a texture to the pool for reuse in subsequent passes or frames.
    pub fn release(&mut self, tex: PooledTexture) {
        self.free
            .entry((tex.width, tex.height, tex.mip_level_count))
            .or_default()
            .push(tex);
    }

    /// Drops all cached textures for dimensions other than `(width, height)`.
    pub fn retain_resolution(&mut self, width: u32, height: u32) {
        self.free.retain(|&(w, h, _), _| w == width && h == height);
    }

    /// Clears all pooled textures.
    pub fn clear(&mut self) {
        self.free.clear();
    }

    /// Returns pool allocation and reuse counters.
    pub fn stats(&self) -> TexturePoolStats {
        self.stats
    }

    /// Returns number of currently idle textures stored in the pool.
    pub fn idle_count(&self) -> usize {
        self.free.values().map(Vec::len).sum()
    }
}

/// Creates a multisampled `Rgba16Float` color render attachment for `Scene3D` / `Text3D` MSAA.
pub fn create_msaa_color_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    sample_count: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("frameshift-msaa-rgba16f"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: sample_count.max(1),
        dimension: wgpu::TextureDimension::D2,
        format: WORKING_TEXTURE_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    (tex, view)
}

/// Creates a multisampled `Depth32Float` depth attachment readable in WGSL via `texture_depth_multisampled_2d`.
pub fn create_msaa_depth_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    sample_count: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("frameshift-msaa-depth32f"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: sample_count.max(1),
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = tex.create_view(&wgpu::TextureViewDescriptor {
        aspect: wgpu::TextureAspect::DepthOnly,
        ..Default::default()
    });
    (tex, view)
}

/// Reusable cache of MSAA color and `Depth32Float` targets keyed by `(width, height, sample_count)`.
#[derive(Debug, Default)]
pub struct MsaaTargetCache {
    color_targets: HashMap<(u32, u32, u32), (wgpu::Texture, wgpu::TextureView)>,
    depth_targets: HashMap<(u32, u32, u32), (wgpu::Texture, wgpu::TextureView)>,
}

impl MsaaTargetCache {
    pub fn ensure_color(
        &mut self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
        sample_count: u32,
    ) -> &wgpu::TextureView {
        let entry = self
            .color_targets
            .entry((width, height, sample_count))
            .or_insert_with(|| create_msaa_color_texture(device, width, height, sample_count));
        &entry.1
    }

    pub fn ensure_depth(
        &mut self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
        sample_count: u32,
    ) -> &wgpu::TextureView {
        let entry = self
            .depth_targets
            .entry((width, height, sample_count))
            .or_insert_with(|| create_msaa_depth_texture(device, width, height, sample_count));
        &entry.1
    }

    pub fn ensure_pair(
        &mut self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
        sample_count: u32,
    ) -> (&wgpu::TextureView, &wgpu::TextureView) {
        let color = self
            .color_targets
            .entry((width, height, sample_count))
            .or_insert_with(|| create_msaa_color_texture(device, width, height, sample_count));
        let depth = self
            .depth_targets
            .entry((width, height, sample_count))
            .or_insert_with(|| create_msaa_depth_texture(device, width, height, sample_count));
        (&color.1, &depth.1)
    }

    pub fn clear(&mut self) {
        self.color_targets.clear();
        self.depth_targets.clear();
    }
}
