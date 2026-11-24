//! Content-addressed multi-tier node output cache (`GpuNodeCache` -> `CpuNodeCache` -> optional `DiskNodeCache`).
//!
//! Uses a pure-Rust SHA-256 Merkle DAG `NodeContentHash` (independent of `Project.revision_id`)
//! so unchanged upstream sub-graphs and repeated frames (`24fps -> 60fps`, `OpticalFlowBlur`
//! `prev_tex`, `MotionBlurShutter` / `FrameEcho` sub-samples) reuse cached `Rgba16Float` results
//! without mid-frame GPU stalls.

use crate::compositor::fnv1a_64;
use crate::pool::WORKING_TEXTURE_FORMAT;
use frameshift_schema::{
    Anime4KPreset, BevelProfile3D, BlendMode, Camera3DDef, Camera3DMode, ColorSpace,
    DisplacementMode, DisplacementWrap, EchoOperator, ExtendedColorSpace, HalftoneColorMode,
    LutSourceDef, MatteMode, NodeKind, SampleFilter, SceneDepthPlateDef, Shape3DKind,
    ShapeMaskKeyframe, ShapeMaskVertex, ShutterPhase, SourceFit, TextAlign2D, TextAnimatorDef,
    TextRangeShape, WritingMode2D,
};
use frameshift_time::Flicks;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Arc;

/// Default Tier 1 GPU VRAM cache budget (192 MiB).
pub const DEFAULT_GPU_NODE_CACHE_BYTES: usize = 192 * 1024 * 1024;
/// Default Tier 2 CPU RAM cache budget (128 MiB).
pub const DEFAULT_CPU_NODE_CACHE_BYTES: usize = 128 * 1024 * 1024;
/// Default Tier 3 Disk LRU cache budget (2 GiB).
pub const DEFAULT_DISK_NODE_CACHE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

const DISK_MAGIC: &[u8; 8] = b"FSCACHE1";
const DISK_HEADER_SIZE: usize = 64;

/// 128-bit truncated SHA-256 content hash for a render graph node at a given resolution and parameter state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeContentHash(pub [u8; 16]);

impl NodeContentHash {
    /// Formats the 128-bit hash as a 32-character lowercase hex string.
    pub fn to_hex(self) -> String {
        let mut s = String::with_capacity(32);
        for b in self.0 {
            use std::fmt::Write;
            let _ = write!(&mut s, "{b:02x}");
        }
        s
    }
}

/// Operating mode for [`SmartCache`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SmartCacheMode {
    Disabled,
    /// Tier 1 GPU VRAM + Tier 2 CPU RAM (default).
    #[default]
    MemoryOnly,
    /// Tier 1 GPU VRAM + Tier 2 CPU RAM + Tier 3 Disk LRU (requires `cache_dir`).
    MemoryAndDisk,
}

/// Configuration for [`SmartCache`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmartCacheConfig {
    pub mode: SmartCacheMode,
    pub gpu_max_bytes: usize,
    pub cpu_max_bytes: usize,
    pub disk_max_bytes: u64,
    pub cache_dir: Option<PathBuf>,
    pub shader_fingerprint: u64,
    pub adapter_fingerprint: u64,
}

impl Default for SmartCacheConfig {
    fn default() -> Self {
        Self {
            mode: SmartCacheMode::MemoryOnly,
            gpu_max_bytes: DEFAULT_GPU_NODE_CACHE_BYTES,
            cpu_max_bytes: DEFAULT_CPU_NODE_CACHE_BYTES,
            disk_max_bytes: DEFAULT_DISK_NODE_CACHE_BYTES,
            cache_dir: None,
            shader_fingerprint: 0x4653_5F53_4844_5231,
            adapter_fingerprint: 0,
        }
    }
}

/// Telemetry counters for [`SmartCache`].
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SmartCacheStats {
    pub gpu_hits: u64,
    pub gpu_misses: u64,
    pub gpu_evictions: u64,
    pub cpu_hits: u64,
    pub cpu_misses: u64,
    pub cpu_evictions: u64,
    pub disk_hits: u64,
    pub disk_misses: u64,
    pub disk_evictions: u64,
    pub gpu_bytes: usize,
    pub cpu_bytes: usize,
    pub disk_bytes: u64,
}

/// Returns `true` when `kind` runs multiple expensive GPU passes and is eligible for Tier 2/3 RAM/Disk caching.
pub fn is_heavy_node(kind: &NodeKind) -> bool {
    match kind {
        NodeKind::Anime4K { .. }
        | NodeKind::OpticalFlowBlur { .. }
        | NodeKind::Scene3D { .. }
        | NodeKind::DeepGlow { .. }
        | NodeKind::Precomp { .. }
        | NodeKind::CustomWgsl { .. }
        | NodeKind::FrameEcho { .. }
        | NodeKind::MotionBlurShutter { .. }
        | NodeKind::FilmOptics { .. } => true,
        NodeKind::ShapeMask { feather_px, .. } => *feather_px > 1e-4,
        _ => false,
    }
}

/// Pure-Rust SHA-256 implementation (0 new Cargo crates).
#[derive(Clone)]
pub struct Sha256 {
    state: [u32; 8],
    bit_len: u64,
    buf: [u8; 64],
    buf_len: usize,
}

impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha256 {
    const K: [u32; 64] = [
        0x428a_2f98, 0x7137_4491, 0xb5c0_fbcf, 0xe9b5_dba5, 0x3956_c25b, 0x59f1_11f1, 0x923f_82a4,
        0xab1c_5ed5, 0xd807_aa98, 0x1283_5b01, 0x2431_85be, 0x550c_7dc3, 0x72be_5d74, 0x80de_b1fe,
        0x9bdc_06a7, 0xc19b_f174, 0xe49b_69c1, 0xefbe_4786, 0x0fc1_9dc6, 0x240c_a1cc, 0x2de9_2c6f,
        0x4a74_84aa, 0x5cb0_a9dc, 0x76f9_88da, 0x983e_5152, 0xa831_c66d, 0xb003_27c8, 0xbf59_7fc7,
        0xc6e0_0bf3, 0xd5a7_9147, 0x06ca_6351, 0x1429_2967, 0x27b7_0a85, 0x2e1b_2138, 0x4d2c_6dfc,
        0x5338_0d13, 0x650a_7354, 0x766a_0abb, 0x81c2_c92e, 0x9272_2c85, 0xa2bf_e8a1, 0xa81a_664b,
        0xc24b_8b70, 0xc76c_51a3, 0xd192_e819, 0xd699_0624, 0xf40e_3585, 0x106a_a070, 0x19a4_c116,
        0x1e37_6c08, 0x2748_774c, 0x34b0_bcb5, 0x391c_0cb3, 0x4ed8_aa4a, 0x5b9c_ca4f, 0x682e_6ff3,
        0x748f_82ee, 0x78a5_636f, 0x84c8_7814, 0x8cc7_0208, 0x90be_fffa, 0xa450_6ceb, 0xbef9_a3f7,
        0xc671_78f2,
    ];

    pub fn new() -> Self {
        Self {
            state: [
                0x6a09_e667, 0xbb67_ae85, 0x3c6e_f372, 0xa54f_f53a, 0x510e_527f, 0x9b05_688c,
                0x1f83_d9ab, 0x5be0_cd19,
            ],
            bit_len: 0,
            buf: [0u8; 64],
            buf_len: 0,
        }
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.bit_len = self.bit_len.wrapping_add((data.len() as u64) * 8);
        if self.buf_len > 0 {
            let take = (64 - self.buf_len).min(data.len());
            self.buf[self.buf_len..self.buf_len + take].copy_from_slice(&data[..take]);
            self.buf_len += take;
            data = &data[take..];
            if self.buf_len == 64 {
                let block = self.buf;
                self.compress(&block);
                self.buf_len = 0;
            }
        }
        while data.len() >= 64 {
            let mut block = [0u8; 64];
            block.copy_from_slice(&data[..64]);
            self.compress(&block);
            data = &data[64..];
        }
        if !data.is_empty() {
            self.buf[..data.len()].copy_from_slice(data);
            self.buf_len = data.len();
        }
    }

    pub fn finalize(mut self) -> [u8; 32] {
        let len = self.buf_len;
        self.buf[len] = 0x80;
        self.buf_len += 1;
        if self.buf_len > 56 {
            self.buf[self.buf_len..64].fill(0);
            let block = self.buf;
            self.compress(&block);
            self.buf_len = 0;
        }
        self.buf[self.buf_len..56].fill(0);
        self.buf[56..64].copy_from_slice(&self.bit_len.to_be_bytes());
        let block = self.buf;
        self.compress(&block);

        let mut out = [0u8; 32];
        for (i, word) in self.state.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        out
    }

    pub fn finalize_128(self) -> NodeContentHash {
        let digest = self.finalize();
        let mut h = [0u8; 16];
        h.copy_from_slice(&digest[..16]);
        NodeContentHash(h)
    }

    fn compress(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for (dst, &chunk) in w[..16].iter_mut().zip(block.as_chunks::<4>().0.iter()) {
            *dst = u32::from_be_bytes(chunk);
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
        for (&k_val, &w_val) in Self::K.iter().zip(w.iter()) {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(k_val)
                .wrapping_add(w_val);
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

/// Canonical content-hash builder that normalizes `-0.0` floats and length-prefixes strings/slices.
pub struct ContentHashBuilder(Sha256);

impl Default for ContentHashBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl ContentHashBuilder {
    pub fn new() -> Self {
        Self(Sha256::new())
    }

    pub fn u8(&mut self, v: u8) {
        self.0.update(&[v]);
    }

    pub fn bool(&mut self, v: bool) {
        self.0.update(&[u8::from(v)]);
    }

    pub fn u32(&mut self, v: u32) {
        self.0.update(&v.to_le_bytes());
    }

    pub fn u64(&mut self, v: u64) {
        self.0.update(&v.to_le_bytes());
    }

    pub fn i64(&mut self, v: i64) {
        self.0.update(&v.to_le_bytes());
    }

    pub fn f32(&mut self, v: f32) {
        let canon = if v == 0.0 {
            0.0f32
        } else if v.is_nan() {
            f32::NAN
        } else {
            v
        };
        self.0.update(&canon.to_le_bytes());
    }

    pub fn f32_slice(&mut self, s: &[f32]) {
        self.u32(s.len() as u32);
        for &v in s {
            self.f32(v);
        }
    }

    pub fn str(&mut self, s: &str) {
        self.u32(s.len() as u32);
        self.0.update(s.as_bytes());
    }

    pub fn bytes(&mut self, b: &[u8]) {
        self.u64(b.len() as u64);
        self.0.update(b);
    }

    pub fn hash(&mut self, h: NodeContentHash) {
        self.0.update(&h.0);
    }

    pub fn finish(self) -> NodeContentHash {
        self.0.finalize_128()
    }
}

/// Computes a deterministic hash for a `SourceVideo` leaf node from its pixel/identity hash and output parameters.
pub fn hash_source_video_leaf(
    asset_id: &str,
    frame_hash: Option<u64>,
    source_tick: Option<Flicks>,
    color_space: ColorSpace,
    fit: SourceFit,
    width: u32,
    height: u32,
) -> NodeContentHash {
    let mut b = ContentHashBuilder::new();
    b.str("SourceVideo");
    b.u32(width);
    b.u32(height);
    match source_tick {
        None => b.u8(0),
        Some(t) => {
            b.u8(1);
            b.str(asset_id);
            b.u8(color_space_code(color_space));
            b.u8(source_fit_code(fit));
            match frame_hash {
                Some(h) => {
                    b.u8(1);
                    b.u64(h);
                }
                None => {
                    b.u8(0);
                    b.i64(t.0);
                }
            }
        }
    }
    b.finish()
}

/// Computes a deterministic hash for a `NodeKind` + upstream content hashes + dimensions.
///
/// Time-invariant nodes omit `sub_tick` so identical inputs & evaluated parameters reuse cached outputs
/// across frames and sub-samples. Time-dependent nodes include `sub_tick` (or active window flags).
#[allow(clippy::too_many_arguments)]
pub fn compute_node_content_hash(
    kind: &NodeKind,
    upstream_hashes: &[NodeContentHash],
    sub_tick: Flicks,
    width: u32,
    height: u32,
    extra_seed: Option<NodeContentHash>,
) -> NodeContentHash {
    let mut b = ContentHashBuilder::new();
    b.u32(width);
    b.u32(height);
    for &uh in upstream_hashes {
        b.hash(uh);
    }
    if let Some(extra) = extra_seed {
        b.u8(1);
        b.hash(extra);
    } else {
        b.u8(0);
    }

    match kind {
        NodeKind::SourceVideo {
            asset_id,
            color_space,
            fit,
            ..
        } => {
            b.str("SourceVideo");
            b.str(asset_id);
            b.u8(color_space_code(*color_space));
            b.u8(source_fit_code(*fit));
            b.i64(sub_tick.0);
        }
        NodeKind::SourceImage {
            asset_id,
            color_space,
            fit,
            opacity,
        } => {
            b.str("SourceImage");
            b.str(asset_id);
            b.u8(color_space_code(*color_space));
            b.u8(source_fit_code(*fit));
            b.f32(*opacity);
        }
        NodeKind::PrecompInput { slot } => {
            b.str("PrecompInput");
            b.u32(*slot as u32);
        }
        NodeKind::Solid {
            color_rgba,
            color_space,
        } => {
            b.str("Solid");
            b.f32_slice(color_rgba);
            b.u8(color_space_code(*color_space));
        }
        NodeKind::Transform2D {
            translate,
            scale,
            rotation_rad,
            anchor,
            filter,
        } => {
            b.str("Transform2D");
            b.f32_slice(translate);
            b.f32_slice(scale);
            b.f32(*rotation_rad);
            b.f32_slice(anchor);
            b.u8(sample_filter_code(*filter));
        }
        NodeKind::MaskMatte {
            mode,
            invert,
            feather,
            opaque_black_outside,
            opacity,
            choke_px,
            spatial_feather_px,
        } => {
            b.str("MaskMatte");
            b.u8(matte_mode_code(*mode));
            b.bool(*invert);
            b.f32(*feather);
            b.bool(*opaque_black_outside);
            b.f32(*opacity);
            b.f32(*choke_px);
            b.f32(*spatial_feather_px);
        }
        NodeKind::DepthMatte {
            near_depth,
            far_depth,
            feather,
            invert,
        } => {
            b.str("DepthMatte");
            b.f32(*near_depth);
            b.f32(*far_depth);
            b.f32(*feather);
            b.bool(*invert);
        }
        NodeKind::AdjustmentLayer {
            start_tick,
            end_tick,
            opacity,
            blend_mode,
        } => {
            b.str("AdjustmentLayer");
            let is_active = sub_tick >= *start_tick && sub_tick < *end_tick;
            b.bool(is_active);
            b.f32(*opacity);
            b.u8(blend_mode_code(*blend_mode));
        }
        NodeKind::Blend { mode, opacity } => {
            b.str("Blend");
            b.u8(blend_mode_code(*mode));
            b.f32(*opacity);
        }
        NodeKind::ColorBasic {
            exposure,
            contrast,
            saturation,
            flash_amount,
        } => {
            b.str("ColorBasic");
            b.f32(*exposure);
            b.f32(*contrast);
            b.f32(*saturation);
            b.f32(*flash_amount);
        }
        NodeKind::ShaderTransition {
            shader_name,
            progress,
            direction_rad,
            whip_strength,
            chromatic_aberration,
            impact_flash,
        } => {
            b.str("ShaderTransition");
            b.str(shader_name);
            b.f32(*progress);
            b.f32(*direction_rad);
            b.f32(*whip_strength);
            b.f32(*chromatic_aberration);
            b.f32(*impact_flash);
        }
        NodeKind::Lut3D {
            input,
            lut_preset,
            intensity,
            lut_source,
            input_space,
            output_space,
        } => {
            b.str("Lut3D");
            b.str(&input.0);
            b.str(lut_preset);
            hash_file_stamp(&mut b, lut_preset);
            b.f32(*intensity);
            hash_opt_lut_source(&mut b, lut_source.as_ref());
            hash_opt_ext_cs(&mut b, *input_space);
            hash_opt_ext_cs(&mut b, *output_space);
        }
        NodeKind::MotionTileShake {
            tile_scale,
            mirror_edges,
            shake_amp_norm,
            shake_rot_rad,
            shake_zoom,
            shake_freq_hz,
            shake_decay,
            event_delta_sec,
        } => {
            b.str("MotionTileShake");
            b.f32(*tile_scale);
            b.bool(*mirror_edges);
            b.f32_slice(shake_amp_norm);
            b.f32(*shake_rot_rad);
            b.f32(*shake_zoom);
            b.f32(*shake_freq_hz);
            b.f32(*shake_decay);
            b.f32(*event_delta_sec);
        }
        NodeKind::AnimeImpactFrame {
            luma_threshold,
            edge_weight,
            invert_amount,
            speed_line_density,
            speed_line_intensity,
            center_uv,
            mix_amount,
        } => {
            b.str("AnimeImpactFrame");
            b.f32(*luma_threshold);
            b.f32(*edge_weight);
            b.f32(*invert_amount);
            b.f32(*speed_line_density);
            b.f32(*speed_line_intensity);
            b.f32_slice(center_uv);
            b.f32(*mix_amount);
            b.i64(sub_tick.0);
        }
        NodeKind::RadialShockwave {
            center_uv,
            radius,
            ring_width,
            refraction_amp,
            zoom_blur_strength,
            chromatic_aberration,
        } => {
            b.str("RadialShockwave");
            b.f32_slice(center_uv);
            b.f32(*radius);
            b.f32(*ring_width);
            b.f32(*refraction_amp);
            b.f32(*zoom_blur_strength);
            b.f32(*chromatic_aberration);
        }
        NodeKind::HalftoneGlow {
            dot_scale,
            angle_rad,
            halftone_mix,
            glow_threshold,
            glow_intensity,
            glow_radius_px,
            color_mode,
            dot_softness,
        } => {
            b.str("HalftoneGlow");
            b.f32(*dot_scale);
            b.f32(*angle_rad);
            b.f32(*halftone_mix);
            b.f32(*glow_threshold);
            b.f32(*glow_intensity);
            b.f32(*glow_radius_px);
            b.u8(match color_mode {
                HalftoneColorMode::Monochrome => 0,
                HalftoneColorMode::Cmyk => 1,
                HalftoneColorMode::ColorDot => 2,
            });
            b.f32(*dot_softness);
        }
        NodeKind::DeepGlow {
            threshold,
            knee,
            intensity,
            radius,
            tint_rgb,
            mip_levels,
            scatter,
            anamorphic_ratio,
            chromatic_shift,
        } => {
            b.str("DeepGlow");
            b.f32(*threshold);
            b.f32(*knee);
            b.f32(*intensity);
            b.f32(*radius);
            b.f32_slice(tint_rgb);
            b.u32(*mip_levels);
            b.f32(*scatter);
            b.f32(*anamorphic_ratio);
            b.f32(*chromatic_shift);
        }
        NodeKind::AnimeSharpen {
            line_strength,
            sharpen_amount,
            anti_ringing,
        } => {
            b.str("AnimeSharpen");
            b.f32(*line_strength);
            b.f32(*sharpen_amount);
            b.f32(*anti_ringing);
        }
        NodeKind::Anime4K {
            preset,
            strength,
            clamp_highlights,
        } => {
            b.str("Anime4K");
            b.u8(match preset {
                Anime4KPreset::ModeA => 0,
                Anime4KPreset::ModeB => 1,
                Anime4KPreset::ModeC => 2,
                Anime4KPreset::RestoreOnly => 3,
                Anime4KPreset::LineArt => 4,
            });
            b.f32(*strength);
            b.bool(*clamp_highlights);
        }
        NodeKind::FilmOptics {
            lens_k1,
            lens_k2,
            chromatic_edge,
            grain_amount,
            grain_seed,
            halation_amount,
            vignette_amount,
            auto_scale,
            grain_size_px,
            grain_chroma,
            halation_threshold,
        } => {
            b.str("FilmOptics");
            b.f32(*lens_k1);
            b.f32(*lens_k2);
            b.f32(*chromatic_edge);
            b.f32(*grain_amount);
            b.f32(*grain_seed);
            b.f32(*halation_amount);
            b.f32(*vignette_amount);
            b.bool(*auto_scale);
            b.f32(*grain_size_px);
            b.f32(*grain_chroma);
            b.f32(*halation_threshold);
        }
        NodeKind::BlurMoCurves {
            shift_x,
            shift_y,
            z_dist,
            rotate_deg,
            blur_strength,
            samples,
            wrap_mode,
        } => {
            b.str("BlurMoCurves");
            b.f32(*shift_x);
            b.f32(*shift_y);
            b.f32(*z_dist);
            b.f32(*rotate_deg);
            b.f32(*blur_strength);
            b.u32(*samples);
            b.u32(*wrap_mode);
        }
        NodeKind::CustomWgsl {
            wgsl_source,
            expected_inputs,
            params,
        } => {
            b.str("CustomWgsl");
            b.str(wgsl_source);
            b.u8(*expected_inputs);
            b.f32_slice(params);
            b.i64(sub_tick.0);
        }
        NodeKind::VectorText {
            text,
            font_size_norm,
            color_rgba,
            outline_rgba,
            outline_width_norm,
            align_center,
            y_offset_norm,
            font_family,
            font_path,
            font_weight,
            align,
            x_offset_norm,
            tracking_em,
            line_height_em,
            opacity,
            fallback_fonts,
            writing_mode,
            var_wght,
            var_wdth,
            var_slnt,
            color_bottom_rgba,
            gradient_angle_deg,
            outer_outline_rgba,
            outer_outline_width_norm,
            outline_gap_norm,
            shadow_offset_norm,
            shadow_blur_norm,
            shadow_rgba,
            glow_radius_norm,
            glow_rgba,
            hdr_intensity,
            gpu_slug,
            text_animator,
        } => {
            b.str("VectorText");
            b.str(text);
            b.f32(*font_size_norm);
            b.f32_slice(color_rgba);
            b.f32_slice(outline_rgba);
            b.f32(*outline_width_norm);
            b.bool(*align_center);
            b.f32(*y_offset_norm);
            hash_opt_str(&mut b, font_family.as_deref());
            hash_opt_str(&mut b, font_path.as_deref());
            if let Some(fp) = font_path.as_deref() {
                hash_file_stamp(&mut b, fp);
            }
            b.u32(u32::from(*font_weight));
            b.u8(match align {
                None => 0,
                Some(TextAlign2D::Left) => 1,
                Some(TextAlign2D::Center) => 2,
                Some(TextAlign2D::Right) => 3,
            });
            hash_opt_f32(&mut b, *x_offset_norm);
            hash_opt_f32(&mut b, *tracking_em);
            hash_opt_f32(&mut b, *line_height_em);
            b.f32(*opacity);
            b.u32(fallback_fonts.len() as u32);
            for ff in fallback_fonts {
                b.str(ff);
            }
            b.u8(match writing_mode {
                WritingMode2D::HorizontalTb => 0,
                WritingMode2D::VerticalRl => 1,
            });
            hash_opt_f32(&mut b, *var_wght);
            hash_opt_f32(&mut b, *var_wdth);
            hash_opt_f32(&mut b, *var_slnt);
            hash_opt_f32_arr(&mut b, color_bottom_rgba.as_ref().map(|v| v.as_slice()));
            b.f32(*gradient_angle_deg);
            b.f32_slice(outer_outline_rgba);
            b.f32(*outer_outline_width_norm);
            b.f32(*outline_gap_norm);
            b.f32_slice(shadow_offset_norm);
            b.f32(*shadow_blur_norm);
            b.f32_slice(shadow_rgba);
            b.f32(*glow_radius_norm);
            b.f32_slice(glow_rgba);
            b.f32(*hdr_intensity);
            b.bool(*gpu_slug);
            if let Some(anim) = text_animator {
                b.u8(1);
                hash_text_animator(&mut b, anim);
                b.i64(sub_tick.0);
            } else {
                b.u8(0);
            }
        }
        NodeKind::VectorPathHud {
            points_norm,
            bezier_segments,
            closed,
            stroke_width_norm,
            stroke_rgba,
            fill_rgba,
            dash_len_norm,
            gap_len_norm,
            dash_offset_norm,
            trim_start,
            trim_end,
            node_dot_radius_norm,
            data_value,
            data_label,
            opacity,
        } => {
            b.str("VectorPathHud");
            b.u32(points_norm.len() as u32);
            for pt in points_norm {
                b.f32_slice(pt);
            }
            b.u32(bezier_segments.len() as u32);
            for seg in bezier_segments {
                b.f32_slice(&seg.p0);
                b.f32_slice(&seg.c1);
                b.f32_slice(&seg.c2);
                b.f32_slice(&seg.p1);
            }
            b.bool(*closed);
            b.f32(*stroke_width_norm);
            b.f32_slice(stroke_rgba);
            b.f32_slice(fill_rgba);
            b.f32(*dash_len_norm);
            b.f32(*gap_len_norm);
            b.f32(*dash_offset_norm);
            b.f32(*trim_start);
            b.f32(*trim_end);
            b.f32(*node_dot_radius_norm);
            b.f32(*data_value);
            b.str(data_label);
            b.f32(*opacity);
        }
        NodeKind::Transform3D {
            pos,
            rot_deg,
            scale,
            fov_deg,
            camera_pos,
            camera_rot_deg,
            dof_focal_dist,
            dof_blur_strength,
            mirror_edges,
            parent,
            pivot,
        } => {
            b.str("Transform3D");
            b.f32_slice(pos);
            b.f32_slice(rot_deg);
            b.f32_slice(scale);
            b.f32(*fov_deg);
            b.f32_slice(camera_pos);
            b.f32_slice(camera_rot_deg);
            b.f32(*dof_focal_dist);
            b.f32(*dof_blur_strength);
            b.bool(*mirror_edges);
            hash_opt_str(&mut b, parent.as_ref().map(|p| p.0.as_str()));
            b.f32_slice(pivot);
        }
        NodeKind::Null3D {
            pos,
            rot_deg,
            scale,
            pivot,
            parent,
        } => {
            b.str("Null3D");
            b.f32_slice(pos);
            b.f32_slice(rot_deg);
            b.f32_slice(scale);
            b.f32_slice(pivot);
            hash_opt_str(&mut b, parent.as_ref().map(|p| p.0.as_str()));
        }
        NodeKind::Text3D {
            text,
            font_path,
            font_size,
            extrude_depth,
            bevel_size,
            color_top_rgba,
            color_bottom_rgba,
            side_color_rgba,
            pos,
            rot_deg,
            camera_pos,
            camera_rot_deg,
            fov_deg,
            char_stagger_sec,
            char_bounce_amp,
            char_wave_freq,
            time_offset_sec,
            outline_width,
            outline_color_rgba,
            material_mode,
            color_mode,
            palette_rgba,
            pop_duration_sec,
            pop_overshoot,
            spin_in_deg,
            char_jitter_deg,
            arc_curvature,
            arc_axis,
            echo_count,
            echo_offset,
            echo_alpha_decay,
            echo_scale_step,
            hdr_intensity,
            opacity,
            align,
            line_height_em,
            tracking_em,
            bevel_profile,
            bevel_segments,
            fallback_fonts,
            var_wght,
            var_wdth,
            var_slnt,
        } => {
            b.str("Text3D");
            b.str(text);
            b.str(font_path);
            hash_file_stamp(&mut b, font_path);
            b.f32(*font_size);
            b.f32(*extrude_depth);
            b.f32(*bevel_size);
            b.f32_slice(color_top_rgba);
            b.f32_slice(color_bottom_rgba);
            b.f32_slice(side_color_rgba);
            b.f32_slice(pos);
            b.f32_slice(rot_deg);
            b.f32_slice(camera_pos);
            b.f32_slice(camera_rot_deg);
            b.f32(*fov_deg);
            b.f32(*char_stagger_sec);
            b.f32(*char_bounce_amp);
            b.f32(*char_wave_freq);
            b.f32(*time_offset_sec);
            b.f32(*outline_width);
            b.f32_slice(outline_color_rgba);
            b.u32(*material_mode);
            b.u32(*color_mode);
            b.u32(palette_rgba.len() as u32);
            for p in palette_rgba {
                b.f32_slice(p);
            }
            b.f32(*pop_duration_sec);
            b.f32(*pop_overshoot);
            b.f32(*spin_in_deg);
            b.f32(*char_jitter_deg);
            b.f32(*arc_curvature);
            b.u32(*arc_axis);
            b.u32(*echo_count);
            b.f32_slice(echo_offset);
            b.f32(*echo_alpha_decay);
            b.f32(*echo_scale_step);
            b.f32(*hdr_intensity);
            b.f32(*opacity);
            b.u8(match align {
                TextAlign2D::Left => 0,
                TextAlign2D::Center => 1,
                TextAlign2D::Right => 2,
            });
            hash_opt_f32(&mut b, *line_height_em);
            b.f32(*tracking_em);
            b.u8(match bevel_profile {
                BevelProfile3D::Flat => 0,
                BevelProfile3D::Round => 1,
                BevelProfile3D::Concave => 2,
                BevelProfile3D::Stepped => 3,
            });
            b.u32(*bevel_segments);
            b.u32(fallback_fonts.len() as u32);
            for ff in fallback_fonts {
                b.str(ff);
            }
            hash_opt_f32(&mut b, *var_wght);
            hash_opt_f32(&mut b, *var_wdth);
            hash_opt_f32(&mut b, *var_slnt);
            b.i64(sub_tick.0);
        }
        NodeKind::Shape3D {
            shape_kind,
            size,
            aspect,
            stroke_width,
            corner_radius,
            inner_ratio,
            extrude_depth,
            bevel_size,
            material_mode,
            color_top_rgba,
            color_bottom_rgba,
            side_color_rgba,
            outline_color_rgba,
            pos,
            rot_deg,
            scale,
            camera_pos,
            camera_rot_deg,
            fov_deg,
            pop_duration_sec,
            pop_overshoot,
            spin_in_deg,
            echo_count,
            echo_offset,
            echo_alpha_decay,
            hdr_intensity,
            time_offset_sec,
            opacity,
        } => {
            b.str("Shape3D");
            b.u8(match shape_kind {
                Shape3DKind::NeonTile => 0,
                Shape3DKind::HollowStar5 => 1,
                Shape3DKind::Heart3D => 2,
                Shape3DKind::LightningBolt => 3,
                Shape3DKind::Diamond3D => 4,
            });
            b.f32(*size);
            b.f32(*aspect);
            b.f32(*stroke_width);
            b.f32(*corner_radius);
            b.f32(*inner_ratio);
            b.f32(*extrude_depth);
            b.f32(*bevel_size);
            b.u32(*material_mode);
            b.f32_slice(color_top_rgba);
            b.f32_slice(color_bottom_rgba);
            b.f32_slice(side_color_rgba);
            b.f32_slice(outline_color_rgba);
            b.f32_slice(pos);
            b.f32_slice(rot_deg);
            b.f32_slice(scale);
            b.f32_slice(camera_pos);
            b.f32_slice(camera_rot_deg);
            b.f32(*fov_deg);
            b.f32(*pop_duration_sec);
            b.f32(*pop_overshoot);
            b.f32(*spin_in_deg);
            b.u32(*echo_count);
            b.f32_slice(echo_offset);
            b.f32(*echo_alpha_decay);
            b.f32(*hdr_intensity);
            b.f32(*time_offset_sec);
            b.f32(*opacity);
            b.i64(sub_tick.0);
        }
        NodeKind::ParticleField3D {
            count,
            seed,
            shape,
            color_a_rgba,
            color_b_rgba,
            speed,
            spread,
            size,
            camera_pos,
            camera_rot_deg,
            fov_deg,
            time_offset_sec,
            dof_focal_dist,
            dof_blur_strength,
            opacity,
        } => {
            b.str("ParticleField3D");
            b.u32(*count);
            b.u32(*seed);
            b.u32(*shape);
            b.f32_slice(color_a_rgba);
            b.f32_slice(color_b_rgba);
            b.f32(*speed);
            b.f32(*spread);
            b.f32(*size);
            b.f32_slice(camera_pos);
            b.f32_slice(camera_rot_deg);
            b.f32(*fov_deg);
            b.f32(*time_offset_sec);
            b.f32(*dof_focal_dist);
            b.f32(*dof_blur_strength);
            b.f32(*opacity);
            b.i64(sub_tick.0);
        }
        NodeKind::RibbonTrail3D {
            ribbon_count,
            segments,
            seed,
            width,
            speed,
            radius,
            trim_start,
            trim_end,
            color_a_rgba,
            color_b_rgba,
            camera_pos,
            camera_rot_deg,
            fov_deg,
            time_offset_sec,
            ribbon_style,
            strand_spacing,
            curve_bend,
            pos,
            rot_deg,
            scale,
            opacity,
            core_whiten,
            core_intensity,
            halo_intensity,
            saturation,
            min_width_px,
            normal_blend,
        } => {
            b.str("RibbonTrail3D");
            b.u32(*ribbon_count);
            b.u32(*segments);
            b.u32(*seed);
            b.f32(*width);
            b.f32(*speed);
            b.f32(*radius);
            b.f32(*trim_start);
            b.f32(*trim_end);
            b.f32_slice(color_a_rgba);
            b.f32_slice(color_b_rgba);
            b.f32_slice(camera_pos);
            b.f32_slice(camera_rot_deg);
            b.f32(*fov_deg);
            b.f32(*time_offset_sec);
            b.u32(*ribbon_style);
            b.f32(*strand_spacing);
            b.f32(*curve_bend);
            b.f32_slice(pos);
            b.f32_slice(rot_deg);
            b.f32_slice(scale);
            b.f32(*opacity);
            b.f32(*core_whiten);
            b.f32(*core_intensity);
            b.f32(*halo_intensity);
            b.f32(*saturation);
            b.f32(*min_width_px);
            b.bool(*normal_blend);
            b.i64(sub_tick.0);
        }
        NodeKind::Scene3D {
            camera,
            background_rgba,
            depth_plate,
        } => {
            b.str("Scene3D");
            hash_camera3d(&mut b, camera);
            b.f32_slice(background_rgba);
            if let Some(dp) = depth_plate {
                b.u8(1);
                hash_depth_plate(&mut b, dp);
            } else {
                b.u8(0);
            }
            b.i64(sub_tick.0);
        }
        NodeKind::CornerPin {
            tl,
            tr,
            br,
            bl,
            filter,
            opacity,
        } => {
            b.str("CornerPin");
            b.f32_slice(tl);
            b.f32_slice(tr);
            b.f32_slice(br);
            b.f32_slice(bl);
            b.u8(sample_filter_code(*filter));
            b.f32(*opacity);
        }
        NodeKind::GridWarp {
            cols,
            rows,
            control_offsets,
            wave_amp,
            wave_freq,
            wave_phase,
            mix_amount,
            filter,
            opacity,
        } => {
            b.str("GridWarp");
            b.u32(*cols);
            b.u32(*rows);
            b.u32(control_offsets.len() as u32);
            for pt in control_offsets {
                b.f32_slice(pt);
            }
            b.f32_slice(wave_amp);
            b.f32_slice(wave_freq);
            b.f32(*wave_phase);
            b.f32(*mix_amount);
            b.u8(sample_filter_code(*filter));
            b.f32(*opacity);
        }
        NodeKind::BackdropScrim {
            rect_center_norm,
            rect_size_norm,
            corner_radius_norm,
            blur_radius_norm,
            fill_rgba,
            opacity,
            auto_contrast,
        } => {
            b.str("BackdropScrim");
            b.f32_slice(rect_center_norm);
            b.f32_slice(rect_size_norm);
            b.f32(*corner_radius_norm);
            b.f32(*blur_radius_norm);
            b.f32_slice(fill_rgba);
            b.f32(*opacity);
            b.f32(*auto_contrast);
        }
        NodeKind::MotionBlurShutter {
            shutter_angle_deg,
            sub_samples,
            phase,
            tonemap,
        } => {
            b.str("MotionBlurShutter");
            b.f32(*shutter_angle_deg);
            b.u32(*sub_samples);
            b.u8(match phase {
                ShutterPhase::Centered => 0,
                ShutterPhase::Trailing => 1,
            });
            b.bool(*tonemap);
        }
        NodeKind::OpticalFlowBlur {
            shutter_amount,
            samples,
            max_blur_px,
            sensitivity,
            flow_smoothness,
            visualize_flow,
        } => {
            b.str("OpticalFlowBlur");
            b.f32(*shutter_amount);
            b.u32(*samples);
            b.f32(*max_blur_px);
            b.f32(*sensitivity);
            b.f32(*flow_smoothness);
            b.bool(*visualize_flow);
        }
        NodeKind::Precomp {
            time_offset_ticks,
            time_scale,
            opacity,
            ..
        } => {
            b.str("Precomp");
            b.i64(*time_offset_ticks);
            b.f32(*time_scale);
            b.f32(*opacity);
        }
        NodeKind::DisplacementMap {
            channel_mode,
            edge_mode,
            scale_x,
            scale_y,
            neutral_level,
            angle_rad,
            chromatic_split,
            opacity,
        } => {
            b.str("DisplacementMap");
            b.u8(match channel_mode {
                DisplacementMode::Rg => 0,
                DisplacementMode::Luma => 1,
                DisplacementMode::Polar => 2,
                DisplacementMode::SignedRg => 3,
            });
            b.u8(match edge_mode {
                DisplacementWrap::Clamp => 0,
                DisplacementWrap::Mirror => 1,
                DisplacementWrap::Wrap => 2,
                DisplacementWrap::Transparent => 3,
            });
            b.f32(*scale_x);
            b.f32(*scale_y);
            b.f32(*neutral_level);
            b.f32(*angle_rad);
            b.f32(*chromatic_split);
            b.f32(*opacity);
        }
        NodeKind::FrameEcho {
            echo_step_ticks,
            echo_count,
            starting_intensity,
            decay,
            color_decay_rgb,
            hue_shift_deg,
            operator,
        } => {
            b.str("FrameEcho");
            b.i64(*echo_step_ticks);
            b.u32(*echo_count);
            b.f32(*starting_intensity);
            b.f32(*decay);
            b.f32_slice(color_decay_rgb);
            b.f32(*hue_shift_deg);
            b.u8(match operator {
                EchoOperator::CompositeUnder => 0,
                EchoOperator::CompositeOver => 1,
                EchoOperator::Add => 2,
                EchoOperator::Screen => 3,
                EchoOperator::Maximum => 4,
            });
        }
        NodeKind::ShapeMask {
            vertices,
            keyframes,
            closed,
            feather_px,
            expand_px,
            invert,
            opacity,
        } => {
            b.str("ShapeMask");
            b.bool(*closed);
            b.f32(*feather_px);
            b.f32(*expand_px);
            b.bool(*invert);
            b.f32(*opacity);
            if keyframes.is_empty() {
                b.u8(0);
                hash_shape_mask_vertices(&mut b, vertices);
            } else {
                b.u8(1);
                b.u32(keyframes.len() as u32);
                for kf in keyframes {
                    hash_shape_mask_keyframe(&mut b, kf);
                }
                let clamped_tick = if sub_tick <= keyframes[0].tick {
                    keyframes[0].tick
                } else if sub_tick >= keyframes[keyframes.len() - 1].tick {
                    keyframes[keyframes.len() - 1].tick
                } else {
                    sub_tick
                };
                b.i64(clamped_tick.0);
            }
        }
    }
    b.finish()
}

fn color_space_code(cs: ColorSpace) -> u8 {
    match cs {
        ColorSpace::LinearRec709 => 0,
        ColorSpace::DisplaySrgb => 1,
    }
}

fn source_fit_code(fit: SourceFit) -> u8 {
    match fit {
        SourceFit::Stretch => 0,
        SourceFit::Cover => 1,
        SourceFit::Contain => 2,
    }
}

fn sample_filter_code(f: SampleFilter) -> u8 {
    match f {
        SampleFilter::Bilinear => 0,
        SampleFilter::CatmullRom => 1,
    }
}

fn matte_mode_code(m: MatteMode) -> u8 {
    match m {
        MatteMode::Alpha => 0,
        MatteMode::Luma => 1,
        MatteMode::Red => 2,
    }
}

fn blend_mode_code(m: BlendMode) -> u8 {
    match m {
        BlendMode::Normal => 0,
        BlendMode::Add => 1,
        BlendMode::Screen => 2,
        BlendMode::Multiply => 3,
        BlendMode::Overlay => 4,
        BlendMode::SoftLight => 5,
        BlendMode::ColorDodge => 6,
        BlendMode::HardLight => 7,
        BlendMode::Lighten => 8,
        BlendMode::Darken => 9,
    }
}

fn hash_opt_str(b: &mut ContentHashBuilder, s: Option<&str>) {
    match s {
        None => b.u8(0),
        Some(val) => {
            b.u8(1);
            b.str(val);
        }
    }
}

fn hash_opt_f32(b: &mut ContentHashBuilder, v: Option<f32>) {
    match v {
        None => b.u8(0),
        Some(val) => {
            b.u8(1);
            b.f32(val);
        }
    }
}

fn hash_opt_f32_arr(b: &mut ContentHashBuilder, v: Option<&[f32]>) {
    match v {
        None => b.u8(0),
        Some(val) => {
            b.u8(1);
            b.f32_slice(val);
        }
    }
}

fn hash_opt_ext_cs(b: &mut ContentHashBuilder, cs: Option<ExtendedColorSpace>) {
    match cs {
        None => b.u8(0),
        Some(val) => {
            b.u8(1);
            b.u8(val as u8);
        }
    }
}

fn hash_file_stamp(b: &mut ContentHashBuilder, path: &str) {
    match fs::metadata(path) {
        Ok(meta) => {
            b.u8(1);
            b.u64(meta.len());
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            b.u64(mtime as u64);
            b.u64((mtime >> 64) as u64);
        }
        Err(_) => b.u8(0),
    }
}

fn hash_opt_lut_source(b: &mut ContentHashBuilder, src: Option<&LutSourceDef>) {
    match src {
        None => b.u8(0),
        Some(LutSourceDef::Preset { name }) => {
            b.u8(1);
            b.str(name);
        }
        Some(LutSourceDef::CubeFile {
            path,
            lut_size_override,
        }) => {
            b.u8(2);
            b.str(path);
            hash_file_stamp(b, path);
            b.u32(lut_size_override.unwrap_or(0));
        }
        Some(LutSourceDef::Ocio {
            config_path,
            src_space,
            dst_space,
            look,
            lut_size,
            shaper_space,
        }) => {
            b.u8(3);
            b.str(config_path);
            hash_file_stamp(b, config_path);
            b.str(src_space);
            b.str(dst_space);
            hash_opt_str(b, look.as_deref());
            b.u32(*lut_size);
            hash_opt_str(b, shaper_space.as_deref());
        }
        Some(LutSourceDef::BuiltinTransform {
            src_space,
            dst_space,
            lut_size,
        }) => {
            b.u8(4);
            b.u8(*src_space as u8);
            b.u8(*dst_space as u8);
            b.u32(*lut_size);
        }
    }
}

fn hash_camera3d(b: &mut ContentHashBuilder, cam: &Camera3DDef) {
    b.f32_slice(&cam.pos);
    b.f32_slice(&cam.rot_deg);
    b.f32_slice(&cam.target);
    b.u8(match cam.mode {
        Camera3DMode::Euler => 0,
        Camera3DMode::LookAt => 1,
    });
    b.f32(cam.fov_deg);
    b.f32(cam.near);
    b.f32(cam.far);
    b.f32(cam.dof_focal_dist);
    b.bool(cam.dof_autofocus_target);
    b.f32(cam.dof_blur_strength);
    b.f32(cam.dof_max_radius_px);
    b.u32(cam.dof_samples);
    b.f32(cam.dof_highlight_boost);
    b.u32(cam.dof_aperture_blades);
    b.f32(cam.dof_blade_rotation_deg);
    b.f32(cam.dof_blade_roundness);
    b.f32(cam.dof_anamorphic_ratio);
    b.f32(cam.dof_cats_eye_strength);
}

fn hash_depth_plate(b: &mut ContentHashBuilder, dp: &SceneDepthPlateDef) {
    b.str(&dp.node_id.0);
    b.f32(dp.near_z);
    b.f32(dp.far_z);
    b.bool(dp.invert);
    b.f32(dp.depth_gamma);
    b.bool(dp.disparity_mode);
    b.bool(dp.rg16_packed);
}

fn hash_text_animator(b: &mut ContentHashBuilder, anim: &TextAnimatorDef) {
    b.u8(anim.unit as u8);
    b.i64(anim.start_tick);
    b.i64(anim.duration_ticks);
    b.i64(anim.delay_ticks);
    b.u32(anim.range_start);
    b.u32(anim.range_end.unwrap_or(u32::MAX));
    b.u8(match anim.shape {
        TextRangeShape::Square => 0,
        TextRangeShape::RampUp => 1,
        TextRangeShape::RampDown => 2,
        TextRangeShape::Triangle => 3,
        TextRangeShape::Smooth => 4,
    });
    b.f32_slice(&anim.position_from_norm);
    b.f32(anim.scale_from);
    b.f32(anim.opacity_from);
    b.f32(anim.rotation_deg_from);
    b.f32(anim.tracking_em_from);
    b.f32(anim.ease_high);
    b.f32(anim.ease_low);
    b.u64(anim.randomize_seed.unwrap_or(0));
}

fn hash_shape_mask_vertices(b: &mut ContentHashBuilder, verts: &[ShapeMaskVertex]) {
    b.u32(verts.len() as u32);
    for v in verts {
        b.f32_slice(&v.pos);
        b.f32_slice(&v.in_tangent);
        b.f32_slice(&v.out_tangent);
    }
}

fn hash_shape_mask_keyframe(b: &mut ContentHashBuilder, kf: &ShapeMaskKeyframe) {
    b.i64(kf.tick.0);
    b.u8(kf.easing as u8);
    hash_shape_mask_vertices(b, &kf.vertices);
}

struct GpuCacheEntry {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    width: u32,
    height: u32,
    bytes: usize,
    last_use: u64,
}

/// Tier 1 GPU VRAM LRU cache for `Rgba16Float` node textures.
pub struct GpuNodeCache {
    max_bytes: usize,
    current_bytes: usize,
    clock: u64,
    entries: HashMap<NodeContentHash, GpuCacheEntry>,
    lru: BTreeMap<u64, NodeContentHash>,
    hits: u64,
    misses: u64,
    evictions: u64,
}

impl GpuNodeCache {
    pub fn new(max_bytes: usize) -> Self {
        Self {
            max_bytes,
            current_bytes: 0,
            clock: 0,
            entries: HashMap::new(),
            lru: BTreeMap::new(),
            hits: 0,
            misses: 0,
            evictions: 0,
        }
    }

    pub fn set_max_bytes(&mut self, max_bytes: usize) {
        self.max_bytes = max_bytes;
        self.evict_to_fit();
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.lru.clear();
        self.current_bytes = 0;
    }

    pub fn get_for_copy(&mut self, hash: NodeContentHash, width: u32, height: u32) -> Option<&wgpu::Texture> {
        let matches = self
            .entries
            .get(&hash)
            .is_some_and(|e| e.width == width && e.height == height);
        if !matches {
            self.misses += 1;
            return None;
        }
        self.clock += 1;
        let tick = self.clock;
        if let Some(entry) = self.entries.get_mut(&hash) {
            self.lru.remove(&entry.last_use);
            entry.last_use = tick;
            self.lru.insert(tick, hash);
            self.hits += 1;
        }
        self.entries.get(&hash).map(|e| &e.texture)
    }

    pub fn contains_dim(&self, hash: NodeContentHash, width: u32, height: u32) -> bool {
        self.entries
            .get(&hash)
            .is_some_and(|e| e.width == width && e.height == height)
    }

    pub fn insert_copy(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        hash: NodeContentHash,
        src_tex: &wgpu::Texture,
        width: u32,
        height: u32,
    ) {
        let bytes = (width as usize) * (height as usize) * 8;
        if self.max_bytes == 0 || bytes > self.max_bytes {
            return;
        }
        if let Some(existing) = self.entries.get_mut(&hash)
            && existing.width == width
            && existing.height == height
        {
            self.clock += 1;
            self.lru.remove(&existing.last_use);
            existing.last_use = self.clock;
            self.lru.insert(self.clock, hash);
            return;
        }
        if let Some(old) = self.entries.remove(&hash) {
            self.lru.remove(&old.last_use);
            self.current_bytes = self.current_bytes.saturating_sub(old.bytes);
        }

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("frameshift-gpu-node-cache"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: WORKING_TEXTURE_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: src_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.clock += 1;
        let tick = self.clock;
        self.lru.insert(tick, hash);
        self.current_bytes += bytes;
        self.entries.insert(
            hash,
            GpuCacheEntry {
                texture,
                view,
                width,
                height,
                bytes,
                last_use: tick,
            },
        );
        self.evict_to_fit();
    }

    pub fn insert_from_cpu_bytes(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        hash: NodeContentHash,
        width: u32,
        height: u32,
        bytes_rgba16f: &[u8],
    ) -> Option<&wgpu::TextureView> {
        let byte_len = (width as usize) * (height as usize) * 8;
        if bytes_rgba16f.len() != byte_len || self.max_bytes == 0 || byte_len > self.max_bytes {
            return None;
        }
        if let Some(old) = self.entries.remove(&hash) {
            self.lru.remove(&old.last_use);
            self.current_bytes = self.current_bytes.saturating_sub(old.bytes);
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("frameshift-gpu-node-cache-promoted"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: WORKING_TEXTURE_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytes_rgba16f,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 8),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.clock += 1;
        let tick = self.clock;
        self.lru.insert(tick, hash);
        self.current_bytes += byte_len;
        self.entries.insert(
            hash,
            GpuCacheEntry {
                texture,
                view,
                width,
                height,
                bytes: byte_len,
                last_use: tick,
            },
        );
        self.evict_to_fit();
        self.entries.get(&hash).map(|e| &e.view)
    }

    fn evict_to_fit(&mut self) {
        while self.current_bytes > self.max_bytes {
            let Some((&oldest_tick, &oldest_hash)) = self.lru.iter().next() else {
                break;
            };
            self.lru.remove(&oldest_tick);
            if let Some(removed) = self.entries.remove(&oldest_hash) {
                self.current_bytes = self.current_bytes.saturating_sub(removed.bytes);
                self.evictions += 1;
            }
        }
    }
}

struct CpuCacheEntry {
    width: u32,
    height: u32,
    bytes: Arc<Vec<u8>>,
    last_use: u64,
}

/// Tier 2 CPU RAM LRU cache for heavy node `Rgba16Float` pixel buffers.
pub struct CpuNodeCache {
    max_bytes: usize,
    current_bytes: usize,
    clock: u64,
    entries: HashMap<NodeContentHash, CpuCacheEntry>,
    lru: BTreeMap<u64, NodeContentHash>,
    hits: u64,
    misses: u64,
    evictions: u64,
}

impl CpuNodeCache {
    pub fn new(max_bytes: usize) -> Self {
        Self {
            max_bytes,
            current_bytes: 0,
            clock: 0,
            entries: HashMap::new(),
            lru: BTreeMap::new(),
            hits: 0,
            misses: 0,
            evictions: 0,
        }
    }

    pub fn set_max_bytes(&mut self, max_bytes: usize) {
        self.max_bytes = max_bytes;
        self.evict_to_fit();
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.lru.clear();
        self.current_bytes = 0;
    }

    pub fn get(&mut self, hash: NodeContentHash, width: u32, height: u32) -> Option<Arc<Vec<u8>>> {
        let matches = self
            .entries
            .get(&hash)
            .is_some_and(|e| e.width == width && e.height == height);
        if !matches {
            self.misses += 1;
            return None;
        }
        self.clock += 1;
        let tick = self.clock;
        let entry = self.entries.get_mut(&hash)?;
        self.lru.remove(&entry.last_use);
        entry.last_use = tick;
        self.lru.insert(tick, hash);
        self.hits += 1;
        Some(Arc::clone(&entry.bytes))
    }

    pub fn insert(&mut self, hash: NodeContentHash, width: u32, height: u32, data: Vec<u8>) {
        let len = data.len();
        let expected = (width as usize) * (height as usize) * 8;
        if len != expected || self.max_bytes == 0 || len > self.max_bytes {
            return;
        }
        if let Some(old) = self.entries.remove(&hash) {
            self.lru.remove(&old.last_use);
            self.current_bytes = self.current_bytes.saturating_sub(old.bytes.len());
        }
        self.clock += 1;
        let tick = self.clock;
        self.lru.insert(tick, hash);
        self.current_bytes += len;
        self.entries.insert(
            hash,
            CpuCacheEntry {
                width,
                height,
                bytes: Arc::new(data),
                last_use: tick,
            },
        );
        self.evict_to_fit();
    }

    fn evict_to_fit(&mut self) {
        while self.current_bytes > self.max_bytes {
            let Some((&oldest_tick, &oldest_hash)) = self.lru.iter().next() else {
                break;
            };
            self.lru.remove(&oldest_tick);
            if let Some(removed) = self.entries.remove(&oldest_hash) {
                self.current_bytes = self.current_bytes.saturating_sub(removed.bytes.len());
                self.evictions += 1;
            }
        }
    }
}

struct DiskIndexEntry {
    path: PathBuf,
    size_bytes: u64,
    last_use: u64,
}

/// Tier 3 opt-in disk LRU cache (`cache_dir/nodes/<hh>/<hash>.fsc`) with header & FNV-1a verification.
pub struct DiskNodeCache {
    cache_dir: Option<PathBuf>,
    max_bytes: u64,
    current_bytes: u64,
    clock: u64,
    shader_fingerprint: u64,
    adapter_fingerprint: u64,
    entries: HashMap<NodeContentHash, DiskIndexEntry>,
    lru: BTreeMap<u64, NodeContentHash>,
    hits: u64,
    misses: u64,
    evictions: u64,
}

impl DiskNodeCache {
    pub fn new(
        cache_dir: Option<PathBuf>,
        max_bytes: u64,
        shader_fingerprint: u64,
        adapter_fingerprint: u64,
    ) -> Self {
        Self {
            cache_dir,
            max_bytes,
            current_bytes: 0,
            clock: 0,
            shader_fingerprint,
            adapter_fingerprint,
            entries: HashMap::new(),
            lru: BTreeMap::new(),
            hits: 0,
            misses: 0,
            evictions: 0,
        }
    }

    pub fn configure(
        &mut self,
        cache_dir: Option<PathBuf>,
        max_bytes: u64,
        shader_fingerprint: u64,
        adapter_fingerprint: u64,
    ) {
        if self.cache_dir != cache_dir {
            self.entries.clear();
            self.lru.clear();
            self.current_bytes = 0;
        }
        self.cache_dir = cache_dir;
        self.max_bytes = max_bytes;
        self.shader_fingerprint = shader_fingerprint;
        self.adapter_fingerprint = adapter_fingerprint;
        self.evict_to_fit();
    }

    pub fn clear(&mut self) {
        if let Some(dir) = &self.cache_dir {
            let nodes_dir = dir.join("nodes");
            let _ = fs::remove_dir_all(&nodes_dir);
        }
        self.entries.clear();
        self.lru.clear();
        self.current_bytes = 0;
    }

    fn entry_path(&self, hash: NodeContentHash) -> Option<PathBuf> {
        let dir = self.cache_dir.as_ref()?;
        let hex = hash.to_hex();
        Some(dir.join("nodes").join(&hex[..2]).join(format!("{hex}.fsc")))
    }

    pub fn get(&mut self, hash: NodeContentHash, width: u32, height: u32) -> Option<Vec<u8>> {
        let path = self.entry_path(hash)?;
        let Ok(mut file) = fs::File::open(&path) else {
            self.misses += 1;
            return None;
        };
        let mut header = [0u8; DISK_HEADER_SIZE];
        if file.read_exact(&mut header).is_err() {
            self.misses += 1;
            return None;
        }
        if &header[0..8] != DISK_MAGIC {
            self.misses += 1;
            return None;
        }
        let format_code = u32::from_le_bytes(header[8..12].try_into().ok()?);
        let file_w = u32::from_le_bytes(header[12..16].try_into().ok()?);
        let file_h = u32::from_le_bytes(header[16..20].try_into().ok()?);
        let payload_len = u64::from_le_bytes(header[20..28].try_into().ok()?);
        let shader_fp = u64::from_le_bytes(header[28..36].try_into().ok()?);
        let adapter_fp = u64::from_le_bytes(header[36..44].try_into().ok()?);
        let expected_fnv = u64::from_le_bytes(header[44..52].try_into().ok()?);

        let expected_len = (width as u64) * (height as u64) * 8;
        if format_code != 1
            || file_w != width
            || file_h != height
            || payload_len != expected_len
            || shader_fp != self.shader_fingerprint
            || adapter_fp != self.adapter_fingerprint
        {
            self.misses += 1;
            return None;
        }
        let mut payload = vec![0u8; payload_len as usize];
        if file.read_exact(&mut payload).is_err() {
            self.misses += 1;
            return None;
        }
        if fnv1a_64(&payload) != expected_fnv {
            self.misses += 1;
            return None;
        }

        let total_bytes = (DISK_HEADER_SIZE as u64) + payload_len;
        self.clock += 1;
        let tick = self.clock;
        if let Some(existing) = self.entries.get_mut(&hash) {
            self.lru.remove(&existing.last_use);
            existing.last_use = tick;
            self.lru.insert(tick, hash);
        } else {
            self.current_bytes += total_bytes;
            self.lru.insert(tick, hash);
            self.entries.insert(
                hash,
                DiskIndexEntry {
                    path,
                    size_bytes: total_bytes,
                    last_use: tick,
                },
            );
        }
        self.hits += 1;
        Some(payload)
    }

    pub fn insert(&mut self, hash: NodeContentHash, width: u32, height: u32, payload: &[u8]) {
        let Some(path) = self.entry_path(hash) else {
            return;
        };
        let total_bytes = (DISK_HEADER_SIZE + payload.len()) as u64;
        if self.max_bytes == 0 || total_bytes > self.max_bytes {
            return;
        }
        if let Some(parent) = path.parent()
            && fs::create_dir_all(parent).is_err()
        {
            return;
        }
        let mut header = [0u8; DISK_HEADER_SIZE];
        header[0..8].copy_from_slice(DISK_MAGIC);
        header[8..12].copy_from_slice(&1u32.to_le_bytes());
        header[12..16].copy_from_slice(&width.to_le_bytes());
        header[16..20].copy_from_slice(&height.to_le_bytes());
        header[20..28].copy_from_slice(&(payload.len() as u64).to_le_bytes());
        header[28..36].copy_from_slice(&self.shader_fingerprint.to_le_bytes());
        header[36..44].copy_from_slice(&self.adapter_fingerprint.to_le_bytes());
        header[44..52].copy_from_slice(&fnv1a_64(payload).to_le_bytes());

        let tmp_path = path.with_extension("fsc.tmp");
        let Ok(mut file) = fs::File::create(&tmp_path) else {
            return;
        };
        if file.write_all(&header).is_err() || file.write_all(payload).is_err() {
            let _ = fs::remove_file(&tmp_path);
            return;
        }
        drop(file);
        if fs::rename(&tmp_path, &path).is_err() {
            let _ = fs::remove_file(&tmp_path);
            return;
        }

        if let Some(old) = self.entries.remove(&hash) {
            self.lru.remove(&old.last_use);
            self.current_bytes = self.current_bytes.saturating_sub(old.size_bytes);
        }
        self.clock += 1;
        let tick = self.clock;
        self.lru.insert(tick, hash);
        self.current_bytes += total_bytes;
        self.entries.insert(
            hash,
            DiskIndexEntry {
                path,
                size_bytes: total_bytes,
                last_use: tick,
            },
        );
        self.evict_to_fit();
    }

    fn evict_to_fit(&mut self) {
        while self.current_bytes > self.max_bytes {
            let Some((&oldest_tick, &oldest_hash)) = self.lru.iter().next() else {
                break;
            };
            self.lru.remove(&oldest_tick);
            if let Some(removed) = self.entries.remove(&oldest_hash) {
                let _ = fs::remove_file(&removed.path);
                self.current_bytes = self.current_bytes.saturating_sub(removed.size_bytes);
                self.evictions += 1;
            }
        }
    }
}

/// Pending deferred end-of-frame GPU->CPU readback for populating [`CpuNodeCache`] & [`DiskNodeCache`].
pub struct PendingCpuCacheReadback {
    pub hash: NodeContentHash,
    pub width: u32,
    pub height: u32,
    pub padded_bytes_per_row: u32,
    pub buffer: wgpu::Buffer,
}

/// Unified 3-tier content-addressed Smart Cache manager embedded in [`crate::GpuCompositor`].
pub struct SmartCache {
    config: SmartCacheConfig,
    gpu: GpuNodeCache,
    cpu: CpuNodeCache,
    disk: DiskNodeCache,
}

impl SmartCache {
    pub fn new(config: SmartCacheConfig) -> Self {
        let gpu = GpuNodeCache::new(config.gpu_max_bytes);
        let cpu = CpuNodeCache::new(config.cpu_max_bytes);
        let disk = DiskNodeCache::new(
            config.cache_dir.clone(),
            config.disk_max_bytes,
            config.shader_fingerprint,
            config.adapter_fingerprint,
        );
        Self {
            config,
            gpu,
            cpu,
            disk,
        }
    }

    pub fn config(&self) -> &SmartCacheConfig {
        &self.config
    }

    pub fn set_config(&mut self, config: SmartCacheConfig) {
        self.gpu.set_max_bytes(config.gpu_max_bytes);
        self.cpu.set_max_bytes(config.cpu_max_bytes);
        self.disk.configure(
            config.cache_dir.clone(),
            config.disk_max_bytes,
            config.shader_fingerprint,
            config.adapter_fingerprint,
        );
        self.config = config;
    }

    pub fn clear(&mut self) {
        self.gpu.clear();
        self.cpu.clear();
        self.disk.clear();
    }

    pub fn stats(&self) -> SmartCacheStats {
        SmartCacheStats {
            gpu_hits: self.gpu.hits,
            gpu_misses: self.gpu.misses,
            gpu_evictions: self.gpu.evictions,
            cpu_hits: self.cpu.hits,
            cpu_misses: self.cpu.misses,
            cpu_evictions: self.cpu.evictions,
            disk_hits: self.disk.hits,
            disk_misses: self.disk.misses,
            disk_evictions: self.disk.evictions,
            gpu_bytes: self.gpu.current_bytes,
            cpu_bytes: self.cpu.current_bytes,
            disk_bytes: self.disk.current_bytes,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.config.mode != SmartCacheMode::Disabled
    }

    /// Attempts to populate `dst_tex` from Tier 1 (GPU), Tier 2 (CPU), or Tier 3 (Disk).
    /// Returns `true` on hit.
    #[allow(clippy::too_many_arguments)]
    pub fn try_restore_into(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        hash: NodeContentHash,
        heavy: bool,
        width: u32,
        height: u32,
        dst_tex: &wgpu::Texture,
    ) -> bool {
        if !self.is_enabled() {
            return false;
        }
        if let Some(cached_gpu) = self.gpu.get_for_copy(hash, width, height) {
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: cached_gpu,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: dst_tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
            return true;
        }

        if !heavy {
            return false;
        }

        if let Some(cpu_bytes) = self.cpu.get(hash, width, height) {
            self.upload_and_copy(device, queue, encoder, hash, width, height, &cpu_bytes, dst_tex);
            return true;
        }

        if self.config.mode == SmartCacheMode::MemoryAndDisk
            && let Some(disk_bytes) = self.disk.get(hash, width, height)
        {
            self.upload_and_copy(device, queue, encoder, hash, width, height, &disk_bytes, dst_tex);
            self.cpu.insert(hash, width, height, disk_bytes);
            return true;
        }

        false
    }

    #[allow(clippy::too_many_arguments)]
    fn upload_and_copy(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        hash: NodeContentHash,
        width: u32,
        height: u32,
        bytes: &[u8],
        dst_tex: &wgpu::Texture,
    ) {
        if let Some(_view) = self
            .gpu
            .insert_from_cpu_bytes(device, queue, hash, width, height, bytes)
            && let Some(entry) = self.gpu.entries.get(&hash)
        {
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &entry.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyTextureInfo {
                    texture: dst_tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
            return;
        }

        // Fallback when Tier 1 GPU cache is disabled (gpu_max_bytes == 0):
        // upload into a fresh dedicated staging texture before encoder copy so pooled dst_tex never aliases.
        let temp_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("frameshift-smart-cache-cpu-restore"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: WORKING_TEXTURE_FORMAT,
            usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.write_rgba16f_to_texture(queue, &temp_tex, width, height, bytes);
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &temp_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: dst_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
    }

    fn write_rgba16f_to_texture(
        &self,
        queue: &wgpu::Queue,
        dst_tex: &wgpu::Texture,
        width: u32,
        height: u32,
        bytes: &[u8],
    ) {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: dst_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 8),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Stores a newly computed node texture into Tier 1 (`GpuNodeCache`) and optionally queues
    /// a deferred end-of-frame CPU readback if `heavy` and `queue_cpu_readback == true`.
    #[allow(clippy::too_many_arguments)]
    pub fn store_computed(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        hash: NodeContentHash,
        heavy: bool,
        width: u32,
        height: u32,
        src_tex: &wgpu::Texture,
        queue_cpu_readback: bool,
        pending_readbacks: &mut Vec<PendingCpuCacheReadback>,
    ) {
        if !self.is_enabled() {
            return;
        }
        self.gpu
            .insert_copy(device, encoder, hash, src_tex, width, height);

        if queue_cpu_readback
            && heavy
            && self.config.cpu_max_bytes > 0
            && !self.cpu.entries.contains_key(&hash)
            && !pending_readbacks.iter().any(|p| p.hash == hash)
        {
            let unpadded = width * 8;
            let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
            let padded = unpadded.div_ceil(align) * align;
            let byte_size = (padded as u64) * (height as u64);
            if (unpadded as usize) * (height as usize) <= self.config.cpu_max_bytes {
                let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("frameshift-smart-cache-readback"),
                    size: byte_size,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                });
                encoder.copy_texture_to_buffer(
                    wgpu::TexelCopyTextureInfo {
                        texture: src_tex,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::TexelCopyBufferInfo {
                        buffer: &buffer,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(padded),
                            rows_per_image: Some(height),
                        },
                    },
                    wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                );
                pending_readbacks.push(PendingCpuCacheReadback {
                    hash,
                    width,
                    height,
                    padded_bytes_per_row: padded,
                    buffer,
                });
            }
        }
    }

    /// Maps and stores all pending CPU/Disk readbacks (single batched `device.poll(Wait)`).
    pub fn finish_pending_readbacks(
        &mut self,
        device: &wgpu::Device,
        pending: Vec<PendingCpuCacheReadback>,
    ) {
        if pending.is_empty() {
            return;
        }
        let mut receivers = Vec::with_capacity(pending.len());
        for item in &pending {
            let slice = item.buffer.slice(..);
            let (tx, rx) = std::sync::mpsc::channel();
            slice.map_async(wgpu::MapMode::Read, move |res| {
                let _ = tx.send(res);
            });
            receivers.push(rx);
        }
        if device.poll(wgpu::PollType::wait_indefinitely()).is_err() {
            return;
        }
        let write_disk = self.config.mode == SmartCacheMode::MemoryAndDisk;
        for (item, rx) in pending.into_iter().zip(receivers) {
            if rx.recv().ok().and_then(|r| r.ok()).is_none() {
                continue;
            }
            let slice = item.buffer.slice(..);
            let Ok(mapped) = slice.get_mapped_range() else {
                continue;
            };
            let unpadded = (item.width * 8) as usize;
            let padded = item.padded_bytes_per_row as usize;
            let mut bytes = Vec::with_capacity(unpadded * (item.height as usize));
            for row in 0..item.height as usize {
                let start = row * padded;
                bytes.extend_from_slice(&mapped[start..start + unpadded]);
            }
            drop(mapped);
            item.buffer.unmap();

            if write_disk {
                self.disk
                    .insert(item.hash, item.width, item.height, &bytes);
            }
            self.cpu.insert(item.hash, item.width, item.height, bytes);
        }
    }
}
