use crate::shaper::ShapedTextBlock;

/// GPU quadratic Bézier curve (32 bytes, 16-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct SlugCurveGpu {
    /// `[p0.x, p0.y, p1.x, p1.y]` in Y-up layout space.
    pub p0_p1: [f32; 4],
    /// `[p2.x, p2.y, 0.0, 0.0]` in Y-up layout space.
    pub p2_pad: [f32; 4],
}

/// GPU band descriptor (16 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct SlugBandGpu {
    pub curve_offset: u32,
    pub curve_count: u32,
    pub _pad: [u32; 2],
}

/// GPU glyph metadata and band mapping (48 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct SlugGlyphGpu {
    /// `[min_x, min_y, max_x, max_y]` in Y-up layout space.
    pub bounds: [f32; 4],
    /// `[vband_scale, hband_scale, vband_offset, hband_offset]`.
    pub band_scale_offset: [f32; 4],
    pub hband_base: u32,
    pub vband_base: u32,
    pub hband_count: u32,
    pub vband_count: u32,
}

/// GPU vertex for a dilated glyph quad (40 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct SlugVertex {
    /// Layout space corner `[x, y]` (Y-up relative to anchor).
    pub pos: [f32; 2],
    /// Layout space em-coord `[x, y]` (identical to curve coordinate space).
    pub em_pos: [f32; 2],
    /// Outward corner direction `(±1, ±1)` for dynamic screen-space dilation.
    pub normal_dir: [f32; 2],
    /// Normalized `[0, 1]` UV over the entire text block bounds (for gradients).
    pub uv: [f32; 2],
    pub glyph_index: u32,
    pub _pad: u32,
}

/// Complete CPU-packed Slug GPU buffers for a `ShapedTextBlock`.
#[derive(Debug, Clone, Default)]
pub struct SlugSceneGpu {
    pub vertices: Vec<SlugVertex>,
    pub indices: Vec<u32>,
    pub glyphs: Vec<SlugGlyphGpu>,
    pub hbands: Vec<SlugBandGpu>,
    pub vbands: Vec<SlugBandGpu>,
    pub band_indices: Vec<u32>,
    pub curves: Vec<SlugCurveGpu>,
    pub block_bounds: [f32; 4],
}

impl SlugSceneGpu {
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty() || self.curves.is_empty()
    }
}

/// Packs a `ShapedTextBlock` into Eric Lengyel Slug GPU buffers (`8x8` horizontal + vertical bands,
/// `1/1024` epsilon boundary padding, straight-axis curve culling, and descending `max_x`/`max_y` sort).
pub fn build_slug_scene(block: &ShapedTextBlock) -> SlugSceneGpu {
    let mut scene = SlugSceneGpu {
        block_bounds: [
            block.bounds_min[0],
            block.bounds_min[1],
            block.bounds_max[0],
            block.bounds_max[1],
        ],
        ..SlugSceneGpu::default()
    };

    if block.glyphs.is_empty() {
        return scene;
    }

    let block_w = (block.bounds_max[0] - block.bounds_min[0]).max(1e-4);
    let block_h = (block.bounds_max[1] - block.bounds_min[1]).max(1e-4);

    for glyph in &block.glyphs {
        if glyph.curves.is_empty() {
            continue;
        }
        let min_x = glyph.bounds_min[0];
        let min_y = glyph.bounds_min[1];
        let max_x = glyph.bounds_max[0];
        let max_y = glyph.bounds_max[1];
        let gw = (max_x - min_x).max(1e-4);
        let gh = (max_y - min_y).max(1e-4);

        let curve_base = scene.curves.len() as u32;
        for c in &glyph.curves {
            scene.curves.push(SlugCurveGpu {
                p0_p1: [c.p0[0], c.p0[1], c.p1[0], c.p1[1]],
                p2_pad: [c.p2[0], c.p2[1], 0.0, 0.0],
            });
        }

        let hband_count = (glyph.curves.len().clamp(1, 8)) as u32;
        let vband_count = (glyph.curves.len().clamp(1, 8)) as u32;
        let eps = (gw.max(gh) / 1024.0).max(1e-5);

        let hband_base = scene.hbands.len() as u32;
        for hb in 0..hband_count {
            let y0 = min_y + gh * (hb as f32 / hband_count as f32) - eps;
            let y1 = min_y + gh * ((hb + 1) as f32 / hband_count as f32) + eps;
            let mut entries: Vec<(u32, f32)> = Vec::new();

            for (local_idx, c) in glyph.curves.iter().enumerate() {
                // Skip horizontal straight lines in horizontal bands.
                if (c.p0[1] - c.p1[1]).abs() <= 1e-6 && (c.p1[1] - c.p2[1]).abs() <= 1e-6 {
                    continue;
                }
                let cy_min = c.p0[1].min(c.p1[1]).min(c.p2[1]);
                let cy_max = c.p0[1].max(c.p1[1]).max(c.p2[1]);
                if cy_max >= y0 && cy_min <= y1 {
                    let cx_max = c.p0[0].max(c.p1[0]).max(c.p2[0]);
                    entries.push((curve_base + local_idx as u32, cx_max));
                }
            }
            entries.sort_by(|a, b| b.1.total_cmp(&a.1));
            let offset = scene.band_indices.len() as u32;
            let count = entries.len() as u32;
            scene.band_indices.extend(entries.into_iter().map(|(id, _)| id));
            scene.hbands.push(SlugBandGpu {
                curve_offset: offset,
                curve_count: count,
                _pad: [0, 0],
            });
        }

        let vband_base = scene.vbands.len() as u32;
        for vb in 0..vband_count {
            let x0 = min_x + gw * (vb as f32 / vband_count as f32) - eps;
            let x1 = min_x + gw * ((vb + 1) as f32 / vband_count as f32) + eps;
            let mut entries: Vec<(u32, f32)> = Vec::new();

            for (local_idx, c) in glyph.curves.iter().enumerate() {
                // Skip vertical straight lines in vertical bands.
                if (c.p0[0] - c.p1[0]).abs() <= 1e-6 && (c.p1[0] - c.p2[0]).abs() <= 1e-6 {
                    continue;
                }
                let cx_min = c.p0[0].min(c.p1[0]).min(c.p2[0]);
                let cx_max = c.p0[0].max(c.p1[0]).max(c.p2[0]);
                if cx_max >= x0 && cx_min <= x1 {
                    let cy_max = c.p0[1].max(c.p1[1]).max(c.p2[1]);
                    entries.push((curve_base + local_idx as u32, cy_max));
                }
            }
            entries.sort_by(|a, b| b.1.total_cmp(&a.1));
            let offset = scene.band_indices.len() as u32;
            let count = entries.len() as u32;
            scene.band_indices.extend(entries.into_iter().map(|(id, _)| id));
            scene.vbands.push(SlugBandGpu {
                curve_offset: offset,
                curve_count: count,
                _pad: [0, 0],
            });
        }

        let vband_scale = (vband_count as f32) / gw;
        let hband_scale = (hband_count as f32) / gh;
        let vband_offset = -min_x * vband_scale;
        let hband_offset = -min_y * hband_scale;

        let glyph_idx = scene.glyphs.len() as u32;
        scene.glyphs.push(SlugGlyphGpu {
            bounds: [min_x, min_y, max_x, max_y],
            band_scale_offset: [vband_scale, hband_scale, vband_offset, hband_offset],
            hband_base,
            vband_base,
            hband_count,
            vband_count,
        });

        let u0 = ((min_x - block.bounds_min[0]) / block_w).clamp(0.0, 1.0);
        let u1 = ((max_x - block.bounds_min[0]) / block_w).clamp(0.0, 1.0);
        let v0 = ((min_y - block.bounds_min[1]) / block_h).clamp(0.0, 1.0);
        let v1 = ((max_y - block.bounds_min[1]) / block_h).clamp(0.0, 1.0);

        let v_base = scene.vertices.len() as u32;
        scene.vertices.extend_from_slice(&[
            SlugVertex {
                pos: [min_x, min_y],
                em_pos: [min_x, min_y],
                normal_dir: [-1.0, -1.0],
                uv: [u0, v0],
                glyph_index: glyph_idx,
                _pad: 0,
            },
            SlugVertex {
                pos: [max_x, min_y],
                em_pos: [max_x, min_y],
                normal_dir: [1.0, -1.0],
                uv: [u1, v0],
                glyph_index: glyph_idx,
                _pad: 0,
            },
            SlugVertex {
                pos: [max_x, max_y],
                em_pos: [max_x, max_y],
                normal_dir: [1.0, 1.0],
                uv: [u1, v1],
                glyph_index: glyph_idx,
                _pad: 0,
            },
            SlugVertex {
                pos: [min_x, max_y],
                em_pos: [min_x, max_y],
                normal_dir: [-1.0, 1.0],
                uv: [u0, v1],
                glyph_index: glyph_idx,
                _pad: 0,
            },
        ]);
        scene
            .indices
            .extend_from_slice(&[v_base, v_base + 1, v_base + 2, v_base, v_base + 2, v_base + 3]);
    }

    // Ensure storage buffers are non-empty for wgpu bind groups if needed.
    if scene.band_indices.is_empty() && !scene.glyphs.is_empty() {
        scene.band_indices.push(0);
    }

    scene
}
