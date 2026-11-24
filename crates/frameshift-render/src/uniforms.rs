use bytemuck::{Pod, Zeroable};
use frameshift_schema::{BlendMode, MatteMode, SampleFilter};

/// Uniform buffer for `transform.wgsl` (64 bytes, 16-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct TransformUniform {
    /// `xy` = normalized translation, `zw` = scale (`[1.0, 1.0]` identity).
    pub translate_scale: [f32; 4],
    /// `xy` = anchor `[0..1]`, `z` = rotation in radians, `w` = opacity `[0..1]`.
    pub anchor_rot_opacity: [f32; 4],
    /// `x` = dst_aspect (`<= 0` -> 1.0), `y` = src_aspect (`<= 0` -> auto), `zw` = reserved.
    pub aspect_info: [f32; 4],
    /// `x` = filter_mode (`0` = Bilinear, `1` = Catmull-Rom), `yzw` = reserved.
    pub flags: [u32; 4],
}

impl TransformUniform {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        translate: [f32; 2],
        scale: [f32; 2],
        anchor: [f32; 2],
        rotation_rad: f32,
        opacity: f32,
        dst_aspect: f32,
        src_aspect: f32,
        filter: SampleFilter,
    ) -> Self {
        let filter_mode = match filter {
            SampleFilter::Bilinear => 0,
            SampleFilter::CatmullRom => 1,
        };
        Self {
            translate_scale: [translate[0], translate[1], scale[0], scale[1]],
            anchor_rot_opacity: [anchor[0], anchor[1], rotation_rad, opacity],
            aspect_info: [dst_aspect, src_aspect, 0.0, 0.0],
            flags: [filter_mode, 0, 0, 0],
        }
    }
}

/// Uniform buffer for `mask_matte.wgsl` (32 bytes, 16-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct MaskMatteUniform {
    /// `x` = matte_mode (`0`=Alpha, `1`=Luma Rec.709, `2`=Red, `3`=DepthMatte),
    /// `y` = invert (`0|1`), `z` = opaque_black_outside (`0|1`), `w` = spatial_feather_px bits.
    pub mode_flags: [u32; 4],
    /// `x` = edge0 (or DepthMatte near_depth), `y` = edge1 (or DepthMatte far_depth),
    /// `z` = mask_opacity `[0..1]`, `w` = choke_px (or DepthMatte feather).
    pub feather_opacity: [f32; 4],
}

impl MaskMatteUniform {
    pub fn from_params(mode: MatteMode, invert: bool, feather: f32, mask_opacity: f32) -> Self {
        let matte_mode = match mode {
            MatteMode::Alpha => 0,
            MatteMode::Luma => 1,
            MatteMode::Red => 2,
        };
        let (edge0, edge1) = if feather > 0.0 {
            let clamped = feather.clamp(0.0, 1.0);
            let half = 0.5 * clamped;
            ((0.5 - half).max(0.0), (0.5 + half).min(1.0))
        } else {
            (0.0, 0.0)
        };
        Self {
            mode_flags: [matte_mode, u32::from(invert), 0, 0],
            feather_opacity: [edge0, edge1, mask_opacity.clamp(0.0, 1.0), 0.0],
        }
    }

    pub fn with_opaque_black_outside(mut self, opaque_black_outside: bool) -> Self {
        self.mode_flags[2] = u32::from(opaque_black_outside);
        self
    }

    pub fn with_choke_and_spatial_feather(
        mut self,
        choke_px: f32,
        spatial_feather_px: f32,
    ) -> Self {
        let sf = spatial_feather_px.max(0.0);
        self.mode_flags[3] = if sf > 0.0 { sf.to_bits() } else { 0 };
        self.feather_opacity[3] = choke_px;
        self
    }

    pub fn depth_matte(near_depth: f32, far_depth: f32, feather: f32, invert: bool) -> Self {
        let near = near_depth.max(0.0);
        let far = far_depth.max(near);
        Self {
            mode_flags: [3, u32::from(invert), 0, 0],
            feather_opacity: [near, far, 1.0, feather.max(0.0)],
        }
    }
}

/// Uniform buffer for `blend.wgsl` (32 bytes, 16-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct BlendUniform {
    /// `x` = blend mode (`0`=Normal, `1`=Add, `2`=Screen, `3`=Multiply, `4`=Overlay,
    /// `5`=SoftLight, `6`=ColorDodge, `7`=HardLight, `8`=Lighten, `9`=Darken),
    /// `y` = blend space override (`0`=perceptual sRGB for non-linear modes, `1`=legacy linear), `zw` = reserved.
    pub mode_flags: [u32; 4],
    /// `x` = opacity `[0..1]` applied to top layer (`tex_b`), `yzw` = reserved.
    pub opacity_pad: [f32; 4],
}

impl BlendUniform {
    pub fn new(mode: BlendMode, opacity: f32) -> Self {
        let mode_id = match mode {
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
        };
        Self {
            mode_flags: [mode_id, 0, 0, 0],
            opacity_pad: [opacity.clamp(0.0, 1.0), 0.0, 0.0, 0.0],
        }
    }
}

/// Color transfer space mode for `color_basic.wgsl`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ColorSpaceMode {
    LinearToLinear = 0,
    SrgbToLinear = 1,
    LinearToSrgb = 2,
    SrgbRoundTrip = 3,
}

/// Uniform buffer for `color_basic.wgsl` (48 bytes, 16-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct ColorBasicUniform {
    /// `x` = exposure (EV), `y` = contrast, `z` = saturation, `w` = flash_amount.
    pub grade: [f32; 4],
    /// `x` = highlight_threshold, `y` = flash_softness, `zw` = reserved.
    pub highlight_pad: [f32; 4],
    /// `x` = space_mode (`0..=3`), `yzw` = reserved.
    pub space_flags: [u32; 4],
}

impl ColorBasicUniform {
    pub const DEFAULT_HIGHLIGHT_THRESHOLD: f32 = 0.35;
    pub const DEFAULT_FLASH_SOFTNESS: f32 = 0.65;

    pub fn new(
        exposure: f32,
        contrast: f32,
        saturation: f32,
        flash_amount: f32,
        space_mode: ColorSpaceMode,
    ) -> Self {
        Self {
            grade: [exposure, contrast, saturation, flash_amount],
            highlight_pad: [
                Self::DEFAULT_HIGHLIGHT_THRESHOLD,
                Self::DEFAULT_FLASH_SOFTNESS,
                0.0,
                0.0,
            ],
            space_flags: [space_mode as u32, 0, 0, 0],
        }
    }

    pub fn identity(space_mode: ColorSpaceMode) -> Self {
        Self::new(0.0, 1.0, 1.0, 0.0, space_mode)
    }
}

/// Uniform buffer for `transition_whip_chromatic.wgsl` (32 bytes, 16-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct WhipTransitionUniform {
    /// `x` = progress `[0..1]`, `y` = direction_rad, `z` = whip_strength, `w` = chromatic_aberration.
    pub motion: [f32; 4],
    /// `x` = impact_flash, `yzw` = reserved.
    pub flash_pad: [f32; 4],
}

impl WhipTransitionUniform {
    pub fn new(
        progress: f32,
        direction_rad: f32,
        whip_strength: f32,
        chromatic_aberration: f32,
        impact_flash: f32,
    ) -> Self {
        Self {
            motion: [
                progress.clamp(0.0, 1.0),
                direction_rad,
                whip_strength,
                chromatic_aberration,
            ],
            flash_pad: [impact_flash, 0.0, 0.0, 0.0],
        }
    }

    pub fn with_aspect(mut self, aspect_ratio: f32) -> Self {
        self.flash_pad[1] = aspect_ratio;
        self
    }
}

/// Pass mode for `motion_blur_accumulate.wgsl`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum MotionBlurPassMode {
    PingPongAccum = 0,
    WeightedSample = 1,
    TonemapToSrgb = 2,
}

/// Tonemap curve used when encoding final `Rgba8Unorm` output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u32)]
pub enum TonemapMode {
    None = 0,
    Reinhard = 1,
    #[default]
    AcesLite = 2,
    Agx = 3,
    PbrNeutral = 4,
    AgxPunchy = 5,
}

/// Dither mode applied before final `Rgba8Unorm` quantization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u32)]
pub enum DitherMode {
    #[default]
    Off = 0,
    TpdfIgn = 1,
}

/// Alpha encoding mode for `Rgba8Unorm` output readback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u32)]
pub enum OutputAlphaMode {
    #[default]
    Premultiplied = 0,
    Straight = 1,
}

/// Uniform buffer for `motion_blur_accumulate.wgsl` (32 bytes, 16-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct MotionBlurUniform {
    /// `x` = weight, `y` = accum_scale, `z` = exposure, `w` = dither_strength.
    pub weights: [f32; 4],
    /// `x` = pass_mode, `y` = tonemap_mode, `z` = output_alpha_mode, `w` = dither_mode.
    pub modes: [u32; 4],
}

impl MotionBlurUniform {
    pub fn accumulate(weight: f32, accum_scale: f32) -> Self {
        Self {
            weights: [weight, accum_scale, 1.0, 0.0],
            modes: [MotionBlurPassMode::PingPongAccum as u32, 0, 0, 0],
        }
    }

    pub fn tonemap_to_srgb(
        exposure: f32,
        tonemap_mode: TonemapMode,
        output_alpha_mode: OutputAlphaMode,
    ) -> Self {
        Self::tonemap_to_srgb_with_dither(
            exposure,
            tonemap_mode,
            output_alpha_mode,
            DitherMode::Off,
            0.0,
        )
    }

    pub fn tonemap_to_srgb_with_dither(
        exposure: f32,
        tonemap_mode: TonemapMode,
        output_alpha_mode: OutputAlphaMode,
        dither_mode: DitherMode,
        dither_strength: f32,
    ) -> Self {
        Self {
            weights: [1.0, 0.0, exposure, dither_strength.clamp(0.0, 2.0)],
            modes: [
                MotionBlurPassMode::TonemapToSrgb as u32,
                tonemap_mode as u32,
                output_alpha_mode as u32,
                dither_mode as u32,
            ],
        }
    }
}

/// Uniform buffer for `lut3d.wgsl` (48 bytes, 16-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct Lut3DUniform {
    /// `x` = intensity `[0..1]`, `y` = lut_size (`>= 2`), `zw` = reserved.
    pub intensity_size: [f32; 4],
    /// `xyz` = domain_min, `w` = reserved.
    pub domain_min_pad: [f32; 4],
    /// `xyz` = `1.0 / (domain_max - domain_min)`, `w` = reserved.
    pub domain_inv_span_pad: [f32; 4],
}

impl Lut3DUniform {
    pub fn new(
        intensity: f32,
        lut_size: u32,
        domain_min: [f32; 3],
        domain_inv_span: [f32; 3],
    ) -> Self {
        Self {
            intensity_size: [intensity.clamp(0.0, 1.0), lut_size as f32, 0.0, 0.0],
            domain_min_pad: [domain_min[0], domain_min[1], domain_min[2], 0.0],
            domain_inv_span_pad: [
                domain_inv_span[0],
                domain_inv_span[1],
                domain_inv_span[2],
                0.0,
            ],
        }
    }
}

/// Uniform buffer for `fx_motion_tile_shake.wgsl` (48 bytes, 16-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct MotionTileShakeUniform {
    /// `x` = tile_scale, `y` = shake_amp_norm_x, `z` = shake_amp_norm_y, `w` = shake_rot_rad.
    pub tile_shake: [f32; 4],
    /// `x` = shake_zoom, `y` = shake_freq_hz, `z` = shake_decay, `w` = event_delta_sec.
    pub dynamics: [f32; 4],
    /// `x` = mirror_edges (`0` | `1`), `y` = aspect_correct_pad, `zw` = reserved.
    pub flags: [u32; 4],
}

impl MotionTileShakeUniform {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tile_scale: f32,
        mirror_edges: bool,
        shake_amp_norm: [f32; 2],
        shake_rot_rad: f32,
        shake_zoom: f32,
        shake_freq_hz: f32,
        shake_decay: f32,
        event_delta_sec: f32,
    ) -> Self {
        Self {
            tile_shake: [
                tile_scale,
                shake_amp_norm[0],
                shake_amp_norm[1],
                shake_rot_rad,
            ],
            dynamics: [shake_zoom, shake_freq_hz, shake_decay, event_delta_sec],
            flags: [u32::from(mirror_edges), 0, 0, 0],
        }
    }

    /// Sets damped 3D perspective tilt (pitch/yaw radians) and value-noise shake amplitude.
    /// Defaults to `0.0` (`0u` bits), preserving legacy output identically.
    pub fn with_tilt_and_noise(
        mut self,
        tilt_x_rad: f32,
        tilt_y_rad: f32,
        noise_amp: f32,
    ) -> Self {
        self.flags[1] = if tilt_x_rad == 0.0 { 0 } else { tilt_x_rad.to_bits() };
        self.flags[2] = if tilt_y_rad == 0.0 { 0 } else { tilt_y_rad.to_bits() };
        self.flags[3] = if noise_amp == 0.0 { 0 } else { noise_amp.to_bits() };
        self
    }
}

/// Uniform buffer for `fx_blur_mocurves.wgsl` (32 bytes, 16-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct BlurMoCurvesUniform {
    pub shift_x: f32,
    pub shift_y: f32,
    pub z_dist: f32,
    pub rotate_deg: f32,
    pub blur_strength: f32,
    pub samples: u32,
    pub wrap_mode: u32,
    pub aspect: f32,
}

impl BlurMoCurvesUniform {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        shift_x: f32,
        shift_y: f32,
        z_dist: f32,
        rotate_deg: f32,
        blur_strength: f32,
        samples: u32,
        wrap_mode: u32,
        aspect: f32,
    ) -> Self {
        Self {
            shift_x,
            shift_y,
            z_dist,
            rotate_deg,
            blur_strength: blur_strength.max(0.0),
            samples: samples.clamp(1, 32),
            wrap_mode: wrap_mode.min(2),
            aspect: if aspect.is_finite() && aspect > 0.0 {
                aspect
            } else {
                1.0
            },
        }
    }
}

/// Uniform buffer for `fx_anime_impact_frame.wgsl` (48 bytes, 16-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct AnimeImpactFrameUniform {
    /// `x` = luma_threshold, `y` = edge_weight, `z` = invert_amount, `w` = mix_amount.
    pub thresholds: [f32; 4],
    /// `x` = speed_line_density, `y` = speed_line_intensity, `z` = center_u, `w` = center_v.
    pub speed_lines: [f32; 4],
    /// `x` = aspect_ratio, `y` = seed, `zw` = reserved.
    pub extra: [f32; 4],
}

impl AnimeImpactFrameUniform {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        luma_threshold: f32,
        edge_weight: f32,
        invert_amount: f32,
        speed_line_density: f32,
        speed_line_intensity: f32,
        center_uv: [f32; 2],
        mix_amount: f32,
        aspect_ratio: f32,
        seed: f32,
    ) -> Self {
        Self {
            thresholds: [luma_threshold, edge_weight, invert_amount, mix_amount],
            speed_lines: [
                speed_line_density,
                speed_line_intensity,
                center_uv[0],
                center_uv[1],
            ],
            extra: [aspect_ratio, seed, 0.0, 0.0],
        }
    }
}

/// Uniform buffer for `fx_radial_shockwave.wgsl` (48 bytes, 16-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct RadialShockwaveUniform {
    /// `x` = center_u, `y` = center_v, `z` = radius, `w` = ring_width.
    pub wave: [f32; 4],
    /// `x` = refraction_amp, `y` = zoom_blur_strength, `z` = chromatic_aberration, `w` = aspect_ratio.
    pub strengths: [f32; 4],
    /// Reserved padding.
    pub pad: [f32; 4],
}

impl RadialShockwaveUniform {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        center_uv: [f32; 2],
        radius: f32,
        ring_width: f32,
        refraction_amp: f32,
        zoom_blur_strength: f32,
        chromatic_aberration: f32,
        aspect_ratio: f32,
    ) -> Self {
        Self {
            wave: [center_uv[0], center_uv[1], radius, ring_width],
            strengths: [
                refraction_amp,
                zoom_blur_strength,
                chromatic_aberration,
                aspect_ratio,
            ],
            pad: [0.0; 4],
        }
    }
}

/// Uniform buffer for `fx_halftone_glow.wgsl` (48 bytes, 16-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct HalftoneGlowUniform {
    /// `x` = dot_scale, `y` = angle_rad, `z` = halftone_mix, `w` = aspect_ratio.
    pub halftone: [f32; 4],
    /// `x` = glow_threshold, `y` = glow_intensity, `z` = glow_radius_px, `w` = reserved.
    pub glow: [f32; 4],
    /// Reserved padding.
    pub pad: [f32; 4],
}

impl HalftoneGlowUniform {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        dot_scale: f32,
        angle_rad: f32,
        halftone_mix: f32,
        glow_threshold: f32,
        glow_intensity: f32,
        glow_radius_px: f32,
        aspect_ratio: f32,
    ) -> Self {
        Self {
            halftone: [dot_scale, angle_rad, halftone_mix, aspect_ratio],
            glow: [glow_threshold, glow_intensity, glow_radius_px, 0.0],
            pad: [0.0; 4],
        }
    }

    pub fn with_style(mut self, color_mode: u32, dot_softness: f32) -> Self {
        self.glow[3] = color_mode.min(2) as f32;
        self.pad[0] = dot_softness.clamp(0.0, 1.0);
        self
    }
}

/// Uniform buffer for `fx_bloom_downsample.wgsl` and `fx_bloom_upsample.wgsl` (64 bytes, 16-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct BloomPassUniform {
    pub texel_size: [f32; 2],
    pub threshold: f32,
    pub knee: f32,
    pub intensity: f32,
    pub radius: f32,
    pub is_first_pass: f32,
    pub is_final_composite: f32,
    pub tint_rgb: [f32; 3],
    pub scatter: f32,
    pub anamorphic_ratio: f32,
    pub chromatic_shift: f32,
    pub mip_norm: f32,
    pub _pad: f32,
}

impl BloomPassUniform {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        texel_size: [f32; 2],
        threshold: f32,
        knee: f32,
        intensity: f32,
        radius: f32,
        is_first_pass: bool,
        is_final_composite: bool,
        tint_rgb: [f32; 3],
    ) -> Self {
        Self::with_optics(
            texel_size,
            threshold,
            knee,
            intensity,
            radius,
            is_first_pass,
            is_final_composite,
            tint_rgb,
            0.65,
            0.0,
            0.0,
            0.0,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_optics(
        texel_size: [f32; 2],
        threshold: f32,
        knee: f32,
        intensity: f32,
        radius: f32,
        is_first_pass: bool,
        is_final_composite: bool,
        tint_rgb: [f32; 3],
        scatter: f32,
        anamorphic_ratio: f32,
        chromatic_shift: f32,
        mip_norm: f32,
    ) -> Self {
        Self {
            texel_size,
            threshold,
            knee,
            intensity,
            radius,
            is_first_pass: if is_first_pass { 1.0 } else { 0.0 },
            is_final_composite: if is_final_composite { 1.0 } else { 0.0 },
            tint_rgb,
            scatter,
            anamorphic_ratio,
            chromatic_shift,
            mip_norm,
            _pad: 0.0,
        }
    }

    /// Enables inverse-square mip & kernel falloff (`0.0` = legacy Jimenez tent/scatter, `1.0` = inverse-square).
    pub fn with_inverse_square_falloff(mut self, strength: f32) -> Self {
        self._pad = strength.clamp(0.0, 1.0);
        self
    }
}

/// Uniform buffer for `fx_anime_sharpen.wgsl` (32 bytes, 16-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct AnimeSharpenUniform {
    pub texel_size: [f32; 2],
    pub line_strength: f32,
    pub sharpen_amount: f32,
    pub anti_ringing: f32,
    pub _pad: [f32; 3],
}

impl AnimeSharpenUniform {
    pub fn new(
        texel_size: [f32; 2],
        line_strength: f32,
        sharpen_amount: f32,
        anti_ringing: f32,
    ) -> Self {
        Self {
            texel_size,
            line_strength,
            sharpen_amount,
            anti_ringing,
            _pad: [0.0; 3],
        }
    }
}

/// Uniform buffer for `fx_film_optics.wgsl` (48 bytes, 16-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct FilmOpticsUniform {
    pub lens_k1: f32,
    pub lens_k2: f32,
    pub chromatic_edge: f32,
    pub grain_amount: f32,
    pub grain_seed: f32,
    pub halation_amount: f32,
    pub vignette_amount: f32,
    pub aspect: f32,
    pub grain_size_px: f32,
    pub grain_chroma: f32,
    pub halation_threshold: f32,
    pub auto_scale_flags: u32,
}

impl FilmOpticsUniform {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        lens_k1: f32,
        lens_k2: f32,
        chromatic_edge: f32,
        grain_amount: f32,
        grain_seed: f32,
        halation_amount: f32,
        vignette_amount: f32,
        aspect: f32,
    ) -> Self {
        Self::new_ex(
            lens_k1,
            lens_k2,
            chromatic_edge,
            grain_amount,
            grain_seed,
            halation_amount,
            vignette_amount,
            aspect,
            true,
            1.65,
            0.25,
            0.65,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_ex(
        lens_k1: f32,
        lens_k2: f32,
        chromatic_edge: f32,
        grain_amount: f32,
        grain_seed: f32,
        halation_amount: f32,
        vignette_amount: f32,
        aspect: f32,
        auto_scale: bool,
        grain_size_px: f32,
        grain_chroma: f32,
        halation_threshold: f32,
    ) -> Self {
        Self {
            lens_k1,
            lens_k2,
            chromatic_edge,
            grain_amount,
            grain_seed,
            halation_amount,
            vignette_amount,
            aspect,
            grain_size_px: grain_size_px.max(0.5),
            grain_chroma: grain_chroma.clamp(0.0, 1.0),
            halation_threshold: halation_threshold.max(0.0),
            auto_scale_flags: u32::from(auto_scale),
        }
    }
}

/// Uniform buffer for `fx_film_halation.wgsl` (32 bytes, 16-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct FilmHalationUniform {
    pub texel_size: [f32; 2],
    pub threshold: f32,
    pub knee: f32,
    pub mode: u32,
    pub dir_x: f32,
    pub dir_y: f32,
    pub _pad: f32,
}

impl FilmHalationUniform {
    pub fn extract(texel_size: [f32; 2], threshold: f32, knee: f32) -> Self {
        Self {
            texel_size,
            threshold,
            knee,
            mode: 0,
            dir_x: 0.0,
            dir_y: 0.0,
            _pad: 0.0,
        }
    }

    pub fn blur(texel_size: [f32; 2], dir_x: f32, dir_y: f32) -> Self {
        Self {
            texel_size,
            threshold: 0.0,
            knee: 0.0,
            mode: 1,
            dir_x,
            dir_y,
            _pad: 0.0,
        }
    }

    pub fn combine() -> Self {
        Self {
            texel_size: [0.0, 0.0],
            threshold: 0.0,
            knee: 0.0,
            mode: 2,
            dir_x: 0.0,
            dir_y: 0.0,
            _pad: 0.0,
        }
    }
}

/// Uniform buffer for `backdrop_scrim_blur.wgsl` and `backdrop_scrim_composite.wgsl`
/// (64 bytes, 16-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct BackdropScrimUniform {
    /// `xy` = rounded-rect center in normalized `[0, 1]` canvas coordinates,
    /// `zw` = half width/height in the same normalized canvas units.
    pub rect: [f32; 4],
    /// `x` = corner radius (canvas-height units), `y` = blur radius (canvas-height units),
    /// `z` = opacity `[0..1]`, `w` = auto_contrast `[0..1]`.
    pub shape: [f32; 4],
    /// Straight-alpha plate fill; the composite shader premultiplies it.
    pub fill: [f32; 4],
    /// `xy` = blur direction, `z` = canvas aspect ratio (width / height),
    /// `w` = blur radius as a fraction of canvas height.
    pub misc: [f32; 4],
}

impl BackdropScrimUniform {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        rect_center_norm: [f32; 2],
        rect_size_norm: [f32; 2],
        corner_radius_norm: f32,
        blur_radius_norm: f32,
        fill_rgba: [f32; 4],
        opacity: f32,
        auto_contrast: f32,
        aspect_ratio: f32,
    ) -> Self {
        let aspect = if aspect_ratio.is_finite() && aspect_ratio > 0.0 {
            aspect_ratio
        } else {
            1.0
        };
        let blur = if blur_radius_norm.is_finite() {
            blur_radius_norm.max(0.0)
        } else {
            0.0
        };
        Self {
            rect: [
                rect_center_norm[0],
                rect_center_norm[1],
                (rect_size_norm[0] * 0.5).max(0.0),
                (rect_size_norm[1] * 0.5).max(0.0),
            ],
            shape: [
                corner_radius_norm.max(0.0),
                blur,
                opacity.clamp(0.0, 1.0),
                auto_contrast.clamp(0.0, 1.0),
            ],
            fill: fill_rgba,
            misc: [1.0, 0.0, aspect, blur],
        }
    }

    /// Sets the blur pass direction (`[1, 0]` = horizontal, `[0, 1]` = vertical).
    pub fn with_blur_direction(mut self, dir_x: f32, dir_y: f32) -> Self {
        self.misc[0] = dir_x;
        self.misc[1] = dir_y;
        self
    }
}

/// Uniform buffer for `fx_aa.wgsl` (16 bytes, 16-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct FxaaUniform {
    pub texel_size: [f32; 2],
    pub subpixel_quality: f32,
    pub edge_threshold: f32,
}

impl FxaaUniform {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            texel_size: [1.0 / width.max(1) as f32, 1.0 / height.max(1) as f32],
            subpixel_quality: 0.75,
            edge_threshold: 0.125,
        }
    }
}

/// Uniform buffer for sandboxed `CustomWgsl` fragment shaders (80 bytes, 16-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct CustomWgslUniform {
    /// `[width, height, time_sec, tick_f32]`.
    pub resolution_time_tick: [f32; 4],
    /// `p0..p15` packed into four `vec4<f32>` slots.
    pub params: [[f32; 4]; 4],
}

impl CustomWgslUniform {
    pub fn new(width: u32, height: u32, time_sec: f32, tick: f32, flat_params: &[f32; 16]) -> Self {
        Self {
            resolution_time_tick: [width as f32, height as f32, time_sec, tick],
            params: [
                [
                    flat_params[0],
                    flat_params[1],
                    flat_params[2],
                    flat_params[3],
                ],
                [
                    flat_params[4],
                    flat_params[5],
                    flat_params[6],
                    flat_params[7],
                ],
                [
                    flat_params[8],
                    flat_params[9],
                    flat_params[10],
                    flat_params[11],
                ],
                [
                    flat_params[12],
                    flat_params[13],
                    flat_params[14],
                    flat_params[15],
                ],
            ],
        }
    }
}

/// Uniform buffer for `transform3d.wgsl` (128 bytes, 16-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct Transform3DUniform {
    pub mvp: [[f32; 4]; 4],
    pub model_view_z_row: [f32; 4],
    /// `x` = focal_dist, `y` = blur_strength, `z` = aspect, `w` = texel_size_x.
    pub dof_params: [f32; 4],
    /// `x` = texel_size_y, `yzw` = reserved.
    pub extra: [f32; 4],
    /// `x` = mirror_edges (`0` | `1`), `yzw` = reserved.
    pub flags: [u32; 4],
}

/// GPU vertex buffer layout for `text3d_mesh.wgsl` (48 bytes per vertex).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct GpuMeshVertex3D {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub char_center: [f32; 2],
    pub char_index: u32,
    pub face_kind: u32,
}

impl From<&frameshift_vector::MeshVertex3D> for GpuMeshVertex3D {
    fn from(v: &frameshift_vector::MeshVertex3D) -> Self {
        Self {
            position: v.position,
            normal: v.normal,
            uv: v.uv,
            char_center: v.char_center,
            char_index: v.char_index,
            face_kind: v.face_kind,
        }
    }
}

impl GpuMeshVertex3D {
    const ATTRS: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
        0 => Float32x3,
        1 => Float32x3,
        2 => Float32x2,
        3 => Float32x2,
        4 => Uint32,
        5 => Uint32
    ];

    pub fn vertex_buffer_layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRS,
        }
    }
}

/// Uniform buffer for `text3d_mesh.wgsl` (432 bytes, 16-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct Text3DUniform {
    pub mvp: [[f32; 4]; 4],
    pub model: [[f32; 4]; 4],
    pub view: [[f32; 4]; 4],
    /// `xyz` = camera_world_pos, `w` = material_mode (`0`=Neon3D, `1`=MetallicChrome, `2`=Flat2DStroked, `3`=IridescentAnisotropic).
    pub cam_material: [f32; 4],
    pub color_top: [f32; 4],
    pub color_bottom: [f32; 4],
    pub side_color: [f32; 4],
    pub outline_color: [f32; 4],
    /// `x` = time_sec, `y` = char_stagger_sec, `z` = char_bounce_amp, `w` = char_wave_freq.
    pub anim: [f32; 4],
    /// `x` = pop_duration_sec, `y` = pop_overshoot_c1, `z` = spin_in_deg, `w` = char_jitter_deg.
    pub kinetic: [f32; 4],
    /// `x` = arc_curvature, `y` = arc_axis (`0`=XY, `1`=XZ), `z` = color_mode (`0`=grad, `1`=palette, `2`=rainbow), `w` = palette_count.
    pub arc_palette: [f32; 4],
    /// `xyz` = echo_offset, `w` = echo_count (`0..=16`).
    pub echo_params: [f32; 4],
    /// `x` = echo_alpha_decay, `y` = echo_scale_step, `z` = hue_shift_per_char, `w` = hdr_intensity.
    pub echo_decay: [f32; 4],
    pub palette: [[f32; 4]; 4],
    /// `x` = roughness (`<0` -> material_mode default), `y` = metallic (`<0` -> material_mode default),
    /// `z` = ao_strength (`0..1`), `w` = echo_pass_flag (`0.0` = hero layer 0, `1.0` = echo layers `total_echoes..1`).
    pub pbr_params: [f32; 4],
}

/// Uniform buffer for `particle_field3d.wgsl` (176 bytes, 16-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct ParticleField3DUniform {
    pub view_proj: [[f32; 4]; 4],
    pub cam_right: [f32; 4],
    pub cam_up: [f32; 4],
    /// `xyz` = cam_pos, `w` = time_sec.
    pub cam_pos_time: [f32; 4],
    pub color_a: [f32; 4],
    pub color_b: [f32; 4],
    /// `x` = speed, `y` = spread, `z` = size, `w` = seed_f32.
    pub params0: [f32; 4],
    /// `x` = shape_f32, `y` = dof_focal_dist, `z` = dof_blur_strength, `w` = count_f32.
    pub params1: [f32; 4],
}

/// Uniform buffer for `ribbon_trail3d.wgsl` (272 bytes, 16-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct RibbonTrail3DUniform {
    pub view_proj: [[f32; 4]; 4],
    /// `xyz` = cam_pos, `w` = time_sec.
    pub cam_pos_time: [f32; 4],
    pub color_a: [f32; 4],
    pub color_b: [f32; 4],
    /// `x` = width, `y` = speed, `z` = radius, `w` = seed_f32.
    pub params0: [f32; 4],
    /// `x` = trim_start, `y` = trim_end, `z` = segments_f32, `w` = ribbon_count_f32.
    pub params1: [f32; 4],
    pub model: [[f32; 4]; 4],
    /// `x` = ribbon_style, `y` = strand_spacing, `z` = curve_bend, `w` = opacity.
    pub params2: [f32; 4],
    /// `x` = dof_focal_dist, `y` = dof_blur_strength, `z` = max_world_expand, `w` = reserved.
    pub dof_params: [f32; 4],
    /// `x` = min_width_px, `y` = viewport_w, `z` = viewport_h, `w` = saturation.
    pub params3: [f32; 4],
    /// `x` = core_intensity, `y` = halo_intensity, `z` = core_whiten, `w` = reserved.
    pub params4: [f32; 4],
}

/// Uniform buffer for `fx_dof_coc.wgsl` (32 bytes, 16-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct DofCocUniform {
    /// `x` = near, `y` = far, `z` = focal_dist, `w` = blur_strength.
    pub depth_params: [f32; 4],
    /// `x` = max_radius_px, `y` = texel_x, `z` = texel_y, `w` = reserved.
    pub screen_params: [f32; 4],
}

/// Uniform buffer for `fx_dof_bokeh.wgsl` (48 bytes, 16-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct DofBokehUniform {
    /// `x` = texel_x, `y` = texel_y, `z` = max_radius_px, `w` = tap_count.
    pub params0: [f32; 4],
    /// `x` = highlight_boost, `y` = near_bleed_bias, `z` = jitter_seed, `w` = blade_roundness `[0..1]`.
    pub params1: [f32; 4],
    /// `x` = blade_count (`0`=circle), `y` = blade_rotation_rad, `z` = anamorphic_ratio, `w` = cats_eye_strength `[0..1]`.
    pub params2: [f32; 4],
}

/// GPU vertex for `text_slug.wgsl` (40 bytes per vertex).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct GpuSlugVertex {
    pub pos: [f32; 2],
    pub em_pos: [f32; 2],
    pub normal_dir: [f32; 2],
    pub uv: [f32; 2],
    pub glyph_index: u32,
    pub _pad: u32,
}

impl From<&frameshift_vector::SlugVertex> for GpuSlugVertex {
    fn from(v: &frameshift_vector::SlugVertex) -> Self {
        Self {
            pos: v.pos,
            em_pos: v.em_pos,
            normal_dir: v.normal_dir,
            uv: v.uv,
            glyph_index: v.glyph_index,
            _pad: v._pad,
        }
    }
}

impl GpuSlugVertex {
    const ATTRS: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![
        0 => Float32x2,
        1 => Float32x2,
        2 => Float32x2,
        3 => Float32x2,
        4 => Uint32
    ];

    pub fn vertex_buffer_layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRS,
        }
    }
}

/// Pod wrapper for `SlugGlyphGpu` (64 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct GpuSlugGlyph {
    pub bounds: [f32; 4],
    pub band_scale_offset: [f32; 4],
    pub hband_base: u32,
    pub vband_base: u32,
    pub hband_count: u32,
    pub vband_count: u32,
    pub unit_ranges: [u32; 4],
}

impl From<&frameshift_vector::SlugGlyphGpu> for GpuSlugGlyph {
    fn from(g: &frameshift_vector::SlugGlyphGpu) -> Self {
        Self {
            bounds: g.bounds,
            band_scale_offset: g.band_scale_offset,
            hband_base: g.hband_base,
            vband_base: g.vband_base,
            hband_count: g.hband_count,
            vband_count: g.vband_count,
            unit_ranges: g.unit_ranges,
        }
    }
}

/// Pod wrapper for `SlugBandGpu` (16 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct GpuSlugBand {
    pub curve_offset: u32,
    pub curve_count: u32,
    pub _pad: [u32; 2],
}

impl From<&frameshift_vector::SlugBandGpu> for GpuSlugBand {
    fn from(b: &frameshift_vector::SlugBandGpu) -> Self {
        Self {
            curve_offset: b.curve_offset,
            curve_count: b.curve_count,
            _pad: b._pad,
        }
    }
}

/// Pod wrapper for `SlugCurveGpu` (32 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct GpuSlugCurve {
    pub p0_p1: [f32; 4],
    pub p2_pad: [f32; 4],
}

impl From<&frameshift_vector::SlugCurveGpu> for GpuSlugCurve {
    fn from(c: &frameshift_vector::SlugCurveGpu) -> Self {
        Self {
            p0_p1: c.p0_p1,
            p2_pad: c.p2_pad,
        }
    }
}

/// Uniform buffer for `text_slug.wgsl` (160 bytes, 16-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct SlugUniform {
    pub canvas_anchor: [f32; 4],
    pub block_bounds: [f32; 4],
    pub color_top: [f32; 4],
    pub color_bottom: [f32; 4],
    pub style_params: [f32; 4],
    /// x = enabled, y = unit (0 grapheme, 1 word), z = range start, w = end-exclusive.
    pub selector: [u32; 4],
    /// x = current time, y = animation start, z = duration, w = per-unit stagger in seconds.
    pub timing_seconds: [f32; 4],
    /// x/y = initial position in canvas-height units, z = initial scale, w = initial opacity.
    pub from_style: [f32; 4],
    /// x = rotation_deg_from, y = tracking_px_from, z = ease_high, w = ease_low.
    pub anim_extra: [f32; 4],
    /// x = shape (`0..=4`), y = has_seed (`0|1`), z = seed32, w = range_count.
    pub anim_shape: [u32; 4],
}

/// Uniform buffer for `text2d_fx.wgsl` (112 bytes, 16-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct Text2DFxUniform {
    pub metrics0: [f32; 4],
    pub metrics1: [f32; 4],
    pub shadow_offset: [f32; 4],
    pub outline_rgba: [f32; 4],
    pub outer_outline_rgba: [f32; 4],
    pub shadow_rgba: [f32; 4],
    pub glow_rgba: [f32; 4],
}

/// Uniform buffer for `text2d_jfa_seed.wgsl` and `text2d_jfa_step.wgsl` (16 bytes, 16-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct Text2DJfaUniform {
    /// `x` = step_px, `y` = max_reach_px, `z` = texel_x, `w` = texel_y.
    pub params: [f32; 4],
}

const _: () = {
    assert!(std::mem::size_of::<TransformUniform>() == 64);
    assert!(std::mem::size_of::<MaskMatteUniform>() == 32);
    assert!(std::mem::size_of::<BlendUniform>() == 32);
    assert!(std::mem::size_of::<ColorBasicUniform>() == 48);
    assert!(std::mem::size_of::<WhipTransitionUniform>() == 32);
    assert!(std::mem::size_of::<MotionBlurUniform>() == 32);
    assert!(std::mem::size_of::<Lut3DUniform>() == 48);
    assert!(std::mem::size_of::<MotionTileShakeUniform>() == 48);
    assert!(std::mem::size_of::<BlurMoCurvesUniform>() == 32);
    assert!(std::mem::size_of::<AnimeImpactFrameUniform>() == 48);
    assert!(std::mem::size_of::<RadialShockwaveUniform>() == 48);
    assert!(std::mem::size_of::<HalftoneGlowUniform>() == 48);
    assert!(std::mem::size_of::<BloomPassUniform>() == 64);
    assert!(std::mem::size_of::<AnimeSharpenUniform>() == 32);
    assert!(std::mem::size_of::<FilmOpticsUniform>() == 48);
    assert!(std::mem::size_of::<FilmHalationUniform>() == 32);
    assert!(std::mem::size_of::<BackdropScrimUniform>() == 64);
    assert!(std::mem::size_of::<FxaaUniform>() == 16);
    assert!(std::mem::size_of::<CustomWgslUniform>() == 80);
    assert!(std::mem::size_of::<Transform3DUniform>() == 128);
    assert!(std::mem::size_of::<GpuMeshVertex3D>() == 48);
    assert!(std::mem::size_of::<Text3DUniform>() == 432);
    assert!(std::mem::size_of::<ParticleField3DUniform>() == 176);
    assert!(std::mem::size_of::<RibbonTrail3DUniform>() == 272);
    assert!(std::mem::size_of::<DofCocUniform>() == 32);
    assert!(std::mem::size_of::<DofBokehUniform>() == 48);
    assert!(std::mem::size_of::<GpuSlugVertex>() == 40);
    assert!(std::mem::size_of::<GpuSlugGlyph>() == 64);
    assert!(std::mem::size_of::<GpuSlugBand>() == 16);
    assert!(std::mem::size_of::<GpuSlugCurve>() == 32);
    assert!(std::mem::size_of::<SlugUniform>() == 160);
    assert!(std::mem::size_of::<Text2DFxUniform>() == 112);
    assert!(std::mem::size_of::<Text2DJfaUniform>() == 16);
};
