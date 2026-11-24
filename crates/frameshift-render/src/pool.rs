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

enum StagingKind {
    Y,
    Uv,
    Alpha,
    Rgba8,
    Rgba16f,
}

struct InFlightStaging {
    kind: StagingKind,
    dims: (u32, u32),
    entry: (wgpu::Texture, wgpu::TextureView),
}

/// Lease-based pool of reusable GPU staging textures (`R8Unorm`, `Rg8Unorm`, `Rgba8Unorm`, `Rgba16Float`).
///
/// Each upload within an unsubmitted `CommandEncoder` leases a distinct staging texture so
/// `queue.write_texture` never overwrites an earlier upload in the same batch; calling
/// [`SourceStagingPool::reclaim`] after `queue.submit` returns all leased textures for reuse.
pub struct SourceStagingPool {
    free_y: HashMap<(u32, u32), Vec<(wgpu::Texture, wgpu::TextureView)>>,
    free_uv: HashMap<(u32, u32), Vec<(wgpu::Texture, wgpu::TextureView)>>,
    free_alpha: HashMap<(u32, u32), Vec<(wgpu::Texture, wgpu::TextureView)>>,
    free_rgba8: HashMap<(u32, u32), Vec<(wgpu::Texture, wgpu::TextureView)>>,
    free_rgba16f: HashMap<(u32, u32), Vec<(wgpu::Texture, wgpu::TextureView)>>,
    in_flight: Vec<InFlightStaging>,
    _dummy_r8_white_tex: wgpu::Texture,
    dummy_r8_white_view: wgpu::TextureView,
}

impl std::fmt::Debug for SourceStagingPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourceStagingPool")
            .field("in_flight", &self.in_flight.len())
            .finish()
    }
}

impl SourceStagingPool {
    /// Creates a new staging pool and initializes the 1x1 white `R8Unorm` dummy alpha texture.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let dummy_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("frameshift-dummy-r8-white"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &dummy_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &[255u8],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(1),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        let dummy_view = dummy_tex.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            free_y: HashMap::new(),
            free_uv: HashMap::new(),
            free_alpha: HashMap::new(),
            free_rgba8: HashMap::new(),
            free_rgba16f: HashMap::new(),
            in_flight: Vec::new(),
            _dummy_r8_white_tex: dummy_tex,
            dummy_r8_white_view: dummy_view,
        }
    }

    /// Returns the 1x1 opaque `R8Unorm` dummy alpha texture view (`value = 1.0`).
    pub fn dummy_alpha_view(&self) -> &wgpu::TextureView {
        &self.dummy_r8_white_view
    }

    /// Returns all in-flight staging textures to the free pool (call after `queue.submit`).
    pub fn reclaim(&mut self) {
        for item in self.in_flight.drain(..) {
            let bucket = match item.kind {
                StagingKind::Y => &mut self.free_y,
                StagingKind::Uv => &mut self.free_uv,
                StagingKind::Alpha => &mut self.free_alpha,
                StagingKind::Rgba8 => &mut self.free_rgba8,
                StagingKind::Rgba16f => &mut self.free_rgba16f,
            };
            bucket.entry(item.dims).or_default().push(item.entry);
        }
    }

    /// Acquires a distinct `(width, height)` `Rgba8Unorm` staging texture for the current encoder batch.
    pub fn acquire_rgba8(
        &mut self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
    ) -> &(wgpu::Texture, wgpu::TextureView) {
        let entry = pop_or_create_staging(
            &mut self.free_rgba8,
            device,
            width,
            height,
            wgpu::TextureFormat::Rgba8Unorm,
            "frameshift-staging-rgba8",
        );
        self.in_flight.push(InFlightStaging {
            kind: StagingKind::Rgba8,
            dims: (width, height),
            entry,
        });
        &self.in_flight.last().unwrap().entry
    }

    /// Acquires a distinct `(width, height)` `Rgba16Float` staging texture for the current encoder batch.
    pub fn acquire_rgba16f(
        &mut self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
    ) -> &(wgpu::Texture, wgpu::TextureView) {
        let entry = pop_or_create_staging(
            &mut self.free_rgba16f,
            device,
            width,
            height,
            WORKING_TEXTURE_FORMAT,
            "frameshift-staging-rgba16f",
        );
        self.in_flight.push(InFlightStaging {
            kind: StagingKind::Rgba16f,
            dims: (width, height),
            entry,
        });
        &self.in_flight.last().unwrap().entry
    }

    /// Acquires distinct NV12 staging textures `(y, uv)` (and optional `alpha`) and uploads plane bytes.
    #[allow(clippy::too_many_arguments)]
    pub fn upload_nv12_planes(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        y_plane: &[u8],
        uv_plane: &[u8],
        alpha_plane: Option<&[u8]>,
    ) -> (&wgpu::TextureView, &wgpu::TextureView, &wgpu::TextureView) {
        let uv_w = width / 2;
        let uv_h = height / 2;

        let y_entry = pop_or_create_staging(
            &mut self.free_y,
            device,
            width,
            height,
            wgpu::TextureFormat::R8Unorm,
            "frameshift-staging-nv12-y",
        );
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &y_entry.0,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            y_plane,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.in_flight.push(InFlightStaging {
            kind: StagingKind::Y,
            dims: (width, height),
            entry: y_entry,
        });
        let y_idx = self.in_flight.len() - 1;

        let uv_entry = pop_or_create_staging(
            &mut self.free_uv,
            device,
            uv_w,
            uv_h,
            wgpu::TextureFormat::Rg8Unorm,
            "frameshift-staging-nv12-uv",
        );
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &uv_entry.0,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            uv_plane,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(uv_w * 2),
                rows_per_image: Some(uv_h),
            },
            wgpu::Extent3d {
                width: uv_w,
                height: uv_h,
                depth_or_array_layers: 1,
            },
        );
        self.in_flight.push(InFlightStaging {
            kind: StagingKind::Uv,
            dims: (uv_w, uv_h),
            entry: uv_entry,
        });
        let uv_idx = self.in_flight.len() - 1;

        let a_idx = if let Some(a_bytes) = alpha_plane {
            let a_entry = pop_or_create_staging(
                &mut self.free_alpha,
                device,
                width,
                height,
                wgpu::TextureFormat::R8Unorm,
                "frameshift-staging-nv12-alpha",
            );
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &a_entry.0,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                a_bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width),
                    rows_per_image: Some(height),
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
            self.in_flight.push(InFlightStaging {
                kind: StagingKind::Alpha,
                dims: (width, height),
                entry: a_entry,
            });
            Some(self.in_flight.len() - 1)
        } else {
            None
        };

        let y_view = &self.in_flight[y_idx].entry.1;
        let uv_view = &self.in_flight[uv_idx].entry.1;
        let alpha_view = match a_idx {
            Some(idx) => &self.in_flight[idx].entry.1,
            None => &self.dummy_r8_white_view,
        };
        (y_view, uv_view, alpha_view)
    }

    /// Drops all cached staging textures.
    pub fn clear(&mut self) {
        self.free_y.clear();
        self.free_uv.clear();
        self.free_alpha.clear();
        self.free_rgba8.clear();
        self.free_rgba16f.clear();
        self.in_flight.clear();
    }
}

fn pop_or_create_staging(
    map: &mut HashMap<(u32, u32), Vec<(wgpu::Texture, wgpu::TextureView)>>,
    device: &wgpu::Device,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    label: &str,
) -> (wgpu::Texture, wgpu::TextureView) {
    if let Some(bucket) = map.get_mut(&(width, height))
        && let Some(entry) = bucket.pop()
    {
        return entry;
    }
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    (tex, view)
}
