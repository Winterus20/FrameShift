use crate::compositor::fnv1a_64;
use crate::error::RenderError;
use crate::pool::WORKING_TEXTURE_FORMAT;
use crate::uniforms::{
    AnimeImpactFrameUniform, AnimeSharpenUniform, BackdropScrimUniform, BlendUniform,
    BloomPassUniform, BlurMoCurvesUniform, ColorBasicUniform, CustomWgslUniform, DofBokehUniform,
    DofCocUniform, FilmHalationUniform, FilmOpticsUniform, FxaaUniform, GpuMeshVertex3D,
    GpuSlugVertex, HalftoneGlowUniform, Lut3DUniform, MaskMatteUniform, MotionBlurUniform,
    MotionTileShakeUniform, ParticleField3DUniform, RadialShockwaveUniform, RibbonTrail3DUniform,
    SlugUniform, Text2DFxUniform, Text2DJfaUniform, Text3DUniform, Transform3DUniform,
    TransformUniform, WhipTransitionUniform,
};
use crate::warp2d::{CornerPinUniform, GridWarpUniform};
use std::collections::HashMap;
use std::num::NonZeroU64;
use std::sync::Arc;

const SHADER_COMMON_FULLSCREEN: &str = include_str!("../shaders/common_fullscreen.wgsl");
const SHADER_TRANSFORM: &str = include_str!("../shaders/transform.wgsl");
const SHADER_MASK_MATTE: &str = include_str!("../shaders/mask_matte.wgsl");
const SHADER_BLEND: &str = include_str!("../shaders/blend.wgsl");
const SHADER_COLOR_BASIC: &str = include_str!("../shaders/color_basic.wgsl");
const SHADER_TRANSITION_WHIP: &str = include_str!("../shaders/transition_whip_chromatic.wgsl");
const SHADER_TRANSITION_CROSS_ZOOM: &str = include_str!("../shaders/transition_cross_zoom.wgsl");
const SHADER_TRANSITION_DIRECTIONAL_WARP: &str =
    include_str!("../shaders/transition_directional_warp.wgsl");
const SHADER_TRANSITION_GLITCH_DISPLACE: &str =
    include_str!("../shaders/transition_glitch_displace.wgsl");
const SHADER_TRANSITION_DREAMY_ZOOM: &str = include_str!("../shaders/transition_dreamy_zoom.wgsl");
const SHADER_TRANSITION_SWIRL: &str = include_str!("../shaders/transition_swirl.wgsl");
const SHADER_TRANSITION_LINEAR_BLUR: &str = include_str!("../shaders/transition_linear_blur.wgsl");
const SHADER_TRANSITION_BURN: &str = include_str!("../shaders/transition_burn.wgsl");
const SHADER_TRANSITION_RIPPLE: &str = include_str!("../shaders/transition_ripple.wgsl");
const SHADER_TRANSITION_KALEIDOSCOPE: &str =
    include_str!("../shaders/transition_kaleidoscope.wgsl");
const SHADER_FX_MOTION_TILE_SHAKE: &str = include_str!("../shaders/fx_motion_tile_shake.wgsl");
const SHADER_FX_BLUR_MOCURVES: &str = include_str!("../shaders/fx_blur_mocurves.wgsl");
const SHADER_FX_ANIME_IMPACT_FRAME: &str = include_str!("../shaders/fx_anime_impact_frame.wgsl");
const SHADER_FX_RADIAL_SHOCKWAVE: &str = include_str!("../shaders/fx_radial_shockwave.wgsl");
const SHADER_FX_HALFTONE_GLOW: &str = include_str!("../shaders/fx_halftone_glow.wgsl");
const SHADER_FX_BLOOM_DOWNSAMPLE: &str = include_str!("../shaders/fx_bloom_downsample.wgsl");
const SHADER_FX_BLOOM_UPSAMPLE: &str = include_str!("../shaders/fx_bloom_upsample.wgsl");
const SHADER_FX_ANIME_SHARPEN: &str = include_str!("../shaders/fx_anime_sharpen.wgsl");
const SHADER_FX_FILM_OPTICS: &str = include_str!("../shaders/fx_film_optics.wgsl");
const SHADER_FX_FILM_HALATION: &str = include_str!("../shaders/fx_film_halation.wgsl");
const SHADER_FX_AA: &str = include_str!("../shaders/fx_aa.wgsl");
const SHADER_MIPMAP_DOWNSAMPLE: &str = include_str!("../shaders/mipmap_downsample.wgsl");
const SHADER_MOTION_BLUR: &str = include_str!("../shaders/motion_blur_accumulate.wgsl");
const SHADER_LUT3D: &str = include_str!("../shaders/lut3d.wgsl");
const SHADER_TRANSFORM3D: &str = include_str!("../shaders/transform3d.wgsl");
const SHADER_TEXT3D_MESH: &str = include_str!("../shaders/text3d_mesh.wgsl");
const SHADER_PARTICLE_FIELD3D: &str = include_str!("../shaders/particle_field3d.wgsl");
const SHADER_RIBBON_TRAIL3D: &str = include_str!("../shaders/ribbon_trail3d.wgsl");
const SHADER_FX_DOF_COC: &str = include_str!("../shaders/fx_dof_coc.wgsl");
const SHADER_FX_DOF_COC_MSAA: &str = include_str!("../shaders/fx_dof_coc_msaa.wgsl");
const SHADER_FX_DOF_BOKEH: &str = include_str!("../shaders/fx_dof_bokeh.wgsl");
const SHADER_CORNER_PIN: &str = include_str!("../shaders/corner_pin.wgsl");
const SHADER_GRID_WARP: &str = include_str!("../shaders/grid_warp.wgsl");
const SHADER_TEXT_SLUG: &str = include_str!("../shaders/text_slug.wgsl");
const SHADER_TEXT2D_JFA_SEED: &str = include_str!("../shaders/text2d_jfa_seed.wgsl");
const SHADER_TEXT2D_JFA_STEP: &str = include_str!("../shaders/text2d_jfa_step.wgsl");
const SHADER_TEXT2D_FX: &str = include_str!("../shaders/text2d_fx.wgsl");
const SHADER_BACKDROP_SCRIM_BLUR: &str = include_str!("../shaders/backdrop_scrim_blur.wgsl");
const SHADER_BACKDROP_SCRIM_COMPOSITE: &str =
    include_str!("../shaders/backdrop_scrim_composite.wgsl");

/// Compiled render pipeline paired with its `@group(0)` bind group layout.
pub struct ShaderProgram {
    pub bind_group_layout: wgpu::BindGroupLayout,
    pub pipeline: wgpu::RenderPipeline,
}

/// Compiled 4x MSAA `Scene3D` pipelines (available when `GpuContext::supports_msaa_4x == true`).
pub struct Scene3DMsaaPipelines {
    pub transform3d_scene: ShaderProgram,
    pub text3d_mesh: ShaderProgram,
    pub text3d_mesh_echo: ShaderProgram,
    pub particle_field3d_scene: ShaderProgram,
    pub ribbon_trail3d_scene: ShaderProgram,
    /// Opt-in premultiplied-alpha variant of `ribbon_trail3d_scene` (`normal_blend = true`).
    pub ribbon_trail3d_scene_normal: ShaderProgram,
    pub fx_dof_coc_msaa_h: ShaderProgram,
}

/// All built-in FrameShift WGSL pipelines compiled for the active `wgpu::Device`,
/// plus a runtime pipeline cache for sandboxed `CustomWgsl` fragment shaders.
pub struct ShaderPipelines {
    pub common_fullscreen: ShaderProgram,
    pub transform: ShaderProgram,
    pub mask_matte: ShaderProgram,
    pub blend: ShaderProgram,
    pub color_basic: ShaderProgram,
    pub transition_whip_chromatic: ShaderProgram,
    pub transition_cross_zoom: ShaderProgram,
    pub transition_directional_warp: ShaderProgram,
    pub transition_glitch_displace: ShaderProgram,
    pub transition_dreamy_zoom: ShaderProgram,
    pub transition_swirl: ShaderProgram,
    pub transition_linear_blur: ShaderProgram,
    pub transition_burn: ShaderProgram,
    pub transition_ripple: ShaderProgram,
    pub transition_kaleidoscope: ShaderProgram,
    pub fx_motion_tile_shake: ShaderProgram,
    pub fx_blur_mocurves: ShaderProgram,
    pub fx_anime_impact_frame: ShaderProgram,
    pub fx_radial_shockwave: ShaderProgram,
    pub fx_halftone_glow: ShaderProgram,
    pub fx_bloom_downsample: ShaderProgram,
    pub fx_bloom_upsample: ShaderProgram,
    pub fx_anime_sharpen: ShaderProgram,
    pub fx_film_optics: ShaderProgram,
    pub fx_film_optics_lens: ShaderProgram,
    pub fx_film_optics_composite: ShaderProgram,
    pub fx_film_halation: ShaderProgram,
    pub fx_aa: ShaderProgram,
    pub mipmap_downsample: ShaderProgram,
    pub motion_blur_accumulate: ShaderProgram,
    pub motion_blur_tonemap_srgb: ShaderProgram,
    pub lut3d: ShaderProgram,
    pub transform3d: ShaderProgram,
    pub transform3d_scene: ShaderProgram,
    pub text3d_mesh: ShaderProgram,
    pub text3d_mesh_echo: ShaderProgram,
    pub particle_field3d: ShaderProgram,
    pub particle_field3d_scene: ShaderProgram,
    pub ribbon_trail3d: ShaderProgram,
    pub ribbon_trail3d_scene: ShaderProgram,
    /// Opt-in premultiplied-alpha variants used when `normal_blend = true`.
    pub ribbon_trail3d_normal: ShaderProgram,
    pub ribbon_trail3d_scene_normal: ShaderProgram,
    pub fx_dof_coc: ShaderProgram,
    pub fx_dof_coc_v: ShaderProgram,
    pub fx_dof_bokeh: ShaderProgram,
    pub scene3d_msaa4x: Option<Scene3DMsaaPipelines>,
    pub corner_pin: ShaderProgram,
    pub grid_warp: ShaderProgram,
    pub text_slug: ShaderProgram,
    pub text2d_jfa_seed: ShaderProgram,
    pub text2d_jfa_step: ShaderProgram,
    pub text2d_fx: ShaderProgram,
    pub backdrop_scrim_blur: ShaderProgram,
    pub backdrop_scrim_composite: ShaderProgram,
    custom_wgsl_cache: HashMap<(u64, u8), Arc<ShaderProgram>>,
}

impl ShaderPipelines {
    /// Compiles and validates all built-in WGSL shader modules (1x sample count).
    pub fn compile_all(device: &wgpu::Device) -> Result<Self, RenderError> {
        Self::compile_all_with_msaa(device, false)
    }

    /// Compiles and validates all built-in WGSL shader modules, plus 4x MSAA `Scene3D`
    /// pipelines when `supports_msaa_4x` is `true`.
    pub fn compile_all_with_msaa(
        device: &wgpu::Device,
        supports_msaa_4x: bool,
    ) -> Result<Self, RenderError> {
        let common_fullscreen = build_program(
            device,
            "common_fullscreen",
            SHADER_COMMON_FULLSCREEN,
            "fs_main",
            1,
            None,
            WORKING_TEXTURE_FORMAT,
        )?;

        let transform = build_program(
            device,
            "transform",
            SHADER_TRANSFORM,
            "fs_main",
            1,
            Some(size_non_zero::<TransformUniform>()),
            WORKING_TEXTURE_FORMAT,
        )?;

        let mask_matte = build_program(
            device,
            "mask_matte",
            SHADER_MASK_MATTE,
            "fs_main",
            2,
            Some(size_non_zero::<MaskMatteUniform>()),
            WORKING_TEXTURE_FORMAT,
        )?;

        let blend = build_program(
            device,
            "blend",
            SHADER_BLEND,
            "fs_main",
            2,
            Some(size_non_zero::<BlendUniform>()),
            WORKING_TEXTURE_FORMAT,
        )?;

        let color_basic = build_program(
            device,
            "color_basic",
            SHADER_COLOR_BASIC,
            "fs_main",
            1,
            Some(size_non_zero::<ColorBasicUniform>()),
            WORKING_TEXTURE_FORMAT,
        )?;

        let transition_whip_chromatic = build_program(
            device,
            "transition_whip_chromatic",
            SHADER_TRANSITION_WHIP,
            "fs_main",
            2,
            Some(size_non_zero::<WhipTransitionUniform>()),
            WORKING_TEXTURE_FORMAT,
        )?;

        let transition_cross_zoom = build_program(
            device,
            "transition_cross_zoom",
            SHADER_TRANSITION_CROSS_ZOOM,
            "fs_main",
            2,
            Some(size_non_zero::<WhipTransitionUniform>()),
            WORKING_TEXTURE_FORMAT,
        )?;

        let transition_directional_warp = build_program(
            device,
            "transition_directional_warp",
            SHADER_TRANSITION_DIRECTIONAL_WARP,
            "fs_main",
            2,
            Some(size_non_zero::<WhipTransitionUniform>()),
            WORKING_TEXTURE_FORMAT,
        )?;

        let transition_glitch_displace = build_program(
            device,
            "transition_glitch_displace",
            SHADER_TRANSITION_GLITCH_DISPLACE,
            "fs_main",
            2,
            Some(size_non_zero::<WhipTransitionUniform>()),
            WORKING_TEXTURE_FORMAT,
        )?;

        let transition_dreamy_zoom = build_program(
            device,
            "transition_dreamy_zoom",
            SHADER_TRANSITION_DREAMY_ZOOM,
            "fs_main",
            2,
            Some(size_non_zero::<WhipTransitionUniform>()),
            WORKING_TEXTURE_FORMAT,
        )?;

        let transition_swirl = build_program(
            device,
            "transition_swirl",
            SHADER_TRANSITION_SWIRL,
            "fs_main",
            2,
            Some(size_non_zero::<WhipTransitionUniform>()),
            WORKING_TEXTURE_FORMAT,
        )?;

        let transition_linear_blur = build_program(
            device,
            "transition_linear_blur",
            SHADER_TRANSITION_LINEAR_BLUR,
            "fs_main",
            2,
            Some(size_non_zero::<WhipTransitionUniform>()),
            WORKING_TEXTURE_FORMAT,
        )?;

        let transition_burn = build_program(
            device,
            "transition_burn",
            SHADER_TRANSITION_BURN,
            "fs_main",
            2,
            Some(size_non_zero::<WhipTransitionUniform>()),
            WORKING_TEXTURE_FORMAT,
        )?;

        let transition_ripple = build_program(
            device,
            "transition_ripple",
            SHADER_TRANSITION_RIPPLE,
            "fs_main",
            2,
            Some(size_non_zero::<WhipTransitionUniform>()),
            WORKING_TEXTURE_FORMAT,
        )?;

        let transition_kaleidoscope = build_program(
            device,
            "transition_kaleidoscope",
            SHADER_TRANSITION_KALEIDOSCOPE,
            "fs_main",
            2,
            Some(size_non_zero::<WhipTransitionUniform>()),
            WORKING_TEXTURE_FORMAT,
        )?;

        let fx_motion_tile_shake = build_program(
            device,
            "fx_motion_tile_shake",
            SHADER_FX_MOTION_TILE_SHAKE,
            "fs_main",
            1,
            Some(size_non_zero::<MotionTileShakeUniform>()),
            WORKING_TEXTURE_FORMAT,
        )?;

        let fx_blur_mocurves = build_program(
            device,
            "fx_blur_mocurves",
            SHADER_FX_BLUR_MOCURVES,
            "fs_main",
            1,
            Some(size_non_zero::<BlurMoCurvesUniform>()),
            WORKING_TEXTURE_FORMAT,
        )?;

        let fx_anime_impact_frame = build_program(
            device,
            "fx_anime_impact_frame",
            SHADER_FX_ANIME_IMPACT_FRAME,
            "fs_main",
            1,
            Some(size_non_zero::<AnimeImpactFrameUniform>()),
            WORKING_TEXTURE_FORMAT,
        )?;

        let fx_radial_shockwave = build_program(
            device,
            "fx_radial_shockwave",
            SHADER_FX_RADIAL_SHOCKWAVE,
            "fs_main",
            1,
            Some(size_non_zero::<RadialShockwaveUniform>()),
            WORKING_TEXTURE_FORMAT,
        )?;

        let fx_halftone_glow = build_program(
            device,
            "fx_halftone_glow",
            SHADER_FX_HALFTONE_GLOW,
            "fs_main",
            1,
            Some(size_non_zero::<HalftoneGlowUniform>()),
            WORKING_TEXTURE_FORMAT,
        )?;

        let fx_bloom_downsample = build_program(
            device,
            "fx_bloom_downsample",
            SHADER_FX_BLOOM_DOWNSAMPLE,
            "fs_main",
            1,
            Some(size_non_zero::<BloomPassUniform>()),
            WORKING_TEXTURE_FORMAT,
        )?;

        let fx_bloom_upsample = build_program(
            device,
            "fx_bloom_upsample",
            SHADER_FX_BLOOM_UPSAMPLE,
            "fs_main",
            2,
            Some(size_non_zero::<BloomPassUniform>()),
            WORKING_TEXTURE_FORMAT,
        )?;

        let fx_anime_sharpen = build_program(
            device,
            "fx_anime_sharpen",
            SHADER_FX_ANIME_SHARPEN,
            "fs_main",
            1,
            Some(size_non_zero::<AnimeSharpenUniform>()),
            WORKING_TEXTURE_FORMAT,
        )?;

        let fx_film_optics = build_program(
            device,
            "fx_film_optics",
            SHADER_FX_FILM_OPTICS,
            "fs_main",
            2,
            Some(size_non_zero::<FilmOpticsUniform>()),
            WORKING_TEXTURE_FORMAT,
        )?;

        let fx_film_optics_lens = build_program(
            device,
            "fx_film_optics_lens",
            SHADER_FX_FILM_OPTICS,
            "fs_lens",
            2,
            Some(size_non_zero::<FilmOpticsUniform>()),
            WORKING_TEXTURE_FORMAT,
        )?;

        let fx_film_optics_composite = build_program(
            device,
            "fx_film_optics_composite",
            SHADER_FX_FILM_OPTICS,
            "fs_composite",
            2,
            Some(size_non_zero::<FilmOpticsUniform>()),
            WORKING_TEXTURE_FORMAT,
        )?;

        let fx_film_halation = build_program(
            device,
            "fx_film_halation",
            SHADER_FX_FILM_HALATION,
            "fs_main",
            2,
            Some(size_non_zero::<FilmHalationUniform>()),
            WORKING_TEXTURE_FORMAT,
        )?;

        let fx_aa = build_program(
            device,
            "fx_aa",
            SHADER_FX_AA,
            "fs_main",
            1,
            Some(size_non_zero::<FxaaUniform>()),
            WORKING_TEXTURE_FORMAT,
        )?;

        let mipmap_downsample = build_program(
            device,
            "mipmap_downsample",
            SHADER_MIPMAP_DOWNSAMPLE,
            "fs_main",
            1,
            None,
            WORKING_TEXTURE_FORMAT,
        )?;

        let mb_module = compile_wgsl_module(device, "motion_blur_accumulate", SHADER_MOTION_BLUR)?;
        let mb_layout = create_bind_group_layout(
            device,
            "motion_blur_accumulate",
            2,
            Some(size_non_zero::<MotionBlurUniform>()),
        );
        let mb_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("motion_blur_accumulate-pipeline-layout"),
            bind_group_layouts: &[Some(&mb_layout)],
            immediate_size: 0,
        });

        let mb_accum_pipeline = create_fullscreen_pipeline(
            device,
            "motion_blur_accumulate",
            &mb_module,
            "fs_main",
            &mb_pipeline_layout,
            WORKING_TEXTURE_FORMAT,
        )?;

        let mb_tonemap_layout = create_bind_group_layout(
            device,
            "motion_blur_tonemap_srgb",
            2,
            Some(size_non_zero::<MotionBlurUniform>()),
        );
        let mb_tonemap_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("motion_blur_tonemap_srgb-pipeline-layout"),
                bind_group_layouts: &[Some(&mb_tonemap_layout)],
                immediate_size: 0,
            });
        let mb_tonemap_pipeline = create_fullscreen_pipeline(
            device,
            "motion_blur_tonemap_srgb",
            &mb_module,
            "fs_tonemap_to_srgb",
            &mb_tonemap_pipeline_layout,
            wgpu::TextureFormat::Rgba8Unorm,
        )?;

        let lut3d = build_program_with_dims(
            device,
            "lut3d",
            SHADER_LUT3D,
            "fs_main",
            &[
                wgpu::TextureViewDimension::D2,
                wgpu::TextureViewDimension::D3,
            ],
            Some(size_non_zero::<Lut3DUniform>()),
            WORKING_TEXTURE_FORMAT,
        )?;

        let scene_depth_write_state = Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        });

        let scene_depth_read_state = Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        });

        let transform3d = build_3d_program(
            device,
            "transform3d",
            SHADER_TRANSFORM3D,
            true,
            size_non_zero::<Transform3DUniform>(),
            &[],
            wgpu::PrimitiveTopology::TriangleList,
            None,
            None,
            1,
        )?;

        let transform3d_scene = build_3d_program(
            device,
            "transform3d_scene",
            SHADER_TRANSFORM3D,
            true,
            size_non_zero::<Transform3DUniform>(),
            &[],
            wgpu::PrimitiveTopology::TriangleList,
            scene_depth_write_state.clone(),
            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            1,
        )?;

        let text3d_mesh = build_3d_program(
            device,
            "text3d_mesh",
            SHADER_TEXT3D_MESH,
            false,
            size_non_zero::<Text3DUniform>(),
            &[Some(GpuMeshVertex3D::vertex_buffer_layout())],
            wgpu::PrimitiveTopology::TriangleList,
            scene_depth_write_state.clone(),
            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            1,
        )?;

        let text3d_mesh_echo = build_3d_program(
            device,
            "text3d_mesh_echo",
            SHADER_TEXT3D_MESH,
            false,
            size_non_zero::<Text3DUniform>(),
            &[Some(GpuMeshVertex3D::vertex_buffer_layout())],
            wgpu::PrimitiveTopology::TriangleList,
            scene_depth_read_state.clone(),
            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            1,
        )?;

        let hdr_additive_premul_blend = Some(wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
        });

        let particle_field3d = build_3d_program(
            device,
            "particle_field3d",
            SHADER_PARTICLE_FIELD3D,
            false,
            size_non_zero::<ParticleField3DUniform>(),
            &[],
            wgpu::PrimitiveTopology::TriangleList,
            None,
            hdr_additive_premul_blend,
            1,
        )?;

        let particle_field3d_scene = build_3d_program(
            device,
            "particle_field3d_scene",
            SHADER_PARTICLE_FIELD3D,
            false,
            size_non_zero::<ParticleField3DUniform>(),
            &[],
            wgpu::PrimitiveTopology::TriangleList,
            scene_depth_read_state.clone(),
            hdr_additive_premul_blend,
            1,
        )?;

        let ribbon_trail3d = build_3d_program(
            device,
            "ribbon_trail3d",
            SHADER_RIBBON_TRAIL3D,
            false,
            size_non_zero::<RibbonTrail3DUniform>(),
            &[],
            wgpu::PrimitiveTopology::TriangleStrip,
            None,
            hdr_additive_premul_blend,
            1,
        )?;

        let ribbon_trail3d_scene = build_3d_program(
            device,
            "ribbon_trail3d_scene",
            SHADER_RIBBON_TRAIL3D,
            false,
            size_non_zero::<RibbonTrail3DUniform>(),
            &[],
            wgpu::PrimitiveTopology::TriangleStrip,
            scene_depth_read_state.clone(),
            hdr_additive_premul_blend,
            1,
        )?;

        // Opt-in normal blending. Depth behaviour matches the additive variants: the
        // Scene3D ribbon reads the shared depth buffer but never writes it, so opaque
        // geometry still occludes trails while trails cannot occlude each other.
        let ribbon_trail3d_normal = build_3d_program(
            device,
            "ribbon_trail3d_normal",
            SHADER_RIBBON_TRAIL3D,
            false,
            size_non_zero::<RibbonTrail3DUniform>(),
            &[],
            wgpu::PrimitiveTopology::TriangleStrip,
            None,
            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            1,
        )?;

        let ribbon_trail3d_scene_normal = build_3d_program(
            device,
            "ribbon_trail3d_scene_normal",
            SHADER_RIBBON_TRAIL3D,
            false,
            size_non_zero::<RibbonTrail3DUniform>(),
            &[],
            wgpu::PrimitiveTopology::TriangleStrip,
            scene_depth_read_state.clone(),
            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            1,
        )?;

        let (fx_dof_coc, fx_dof_coc_v) = build_dof_coc_programs(device)?;
        let fx_dof_bokeh = build_program(
            device,
            "fx_dof_bokeh",
            SHADER_FX_DOF_BOKEH,
            "fs_main",
            3,
            Some(size_non_zero::<DofBokehUniform>()),
            WORKING_TEXTURE_FORMAT,
        )?;

        let scene3d_msaa4x = if supports_msaa_4x {
            Some(Scene3DMsaaPipelines {
                transform3d_scene: build_3d_program(
                    device,
                    "transform3d_scene_msaa4x",
                    SHADER_TRANSFORM3D,
                    true,
                    size_non_zero::<Transform3DUniform>(),
                    &[],
                    wgpu::PrimitiveTopology::TriangleList,
                    scene_depth_write_state.clone(),
                    Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    4,
                )?,
                text3d_mesh: build_3d_program(
                    device,
                    "text3d_mesh_msaa4x",
                    SHADER_TEXT3D_MESH,
                    false,
                    size_non_zero::<Text3DUniform>(),
                    &[Some(GpuMeshVertex3D::vertex_buffer_layout())],
                    wgpu::PrimitiveTopology::TriangleList,
                    scene_depth_write_state,
                    Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    4,
                )?,
                text3d_mesh_echo: build_3d_program(
                    device,
                    "text3d_mesh_echo_msaa4x",
                    SHADER_TEXT3D_MESH,
                    false,
                    size_non_zero::<Text3DUniform>(),
                    &[Some(GpuMeshVertex3D::vertex_buffer_layout())],
                    wgpu::PrimitiveTopology::TriangleList,
                    scene_depth_read_state.clone(),
                    Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    4,
                )?,
                particle_field3d_scene: build_3d_program(
                    device,
                    "particle_field3d_scene_msaa4x",
                    SHADER_PARTICLE_FIELD3D,
                    false,
                    size_non_zero::<ParticleField3DUniform>(),
                    &[],
                    wgpu::PrimitiveTopology::TriangleList,
                    scene_depth_read_state.clone(),
                    hdr_additive_premul_blend,
                    4,
                )?,
                ribbon_trail3d_scene: build_3d_program(
                    device,
                    "ribbon_trail3d_scene_msaa4x",
                    SHADER_RIBBON_TRAIL3D,
                    false,
                    size_non_zero::<RibbonTrail3DUniform>(),
                    &[],
                    wgpu::PrimitiveTopology::TriangleStrip,
                    scene_depth_read_state.clone(),
                    hdr_additive_premul_blend,
                    4,
                )?,
                ribbon_trail3d_scene_normal: build_3d_program(
                    device,
                    "ribbon_trail3d_scene_normal_msaa4x",
                    SHADER_RIBBON_TRAIL3D,
                    false,
                    size_non_zero::<RibbonTrail3DUniform>(),
                    &[],
                    wgpu::PrimitiveTopology::TriangleStrip,
                    scene_depth_read_state,
                    Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    4,
                )?,
                fx_dof_coc_msaa_h: build_dof_coc_msaa_program(device)?,
            })
        } else {
            None
        };

        let corner_pin = build_program(
            device,
            "corner_pin",
            SHADER_CORNER_PIN,
            "fs_main",
            1,
            Some(size_non_zero::<CornerPinUniform>()),
            WORKING_TEXTURE_FORMAT,
        )?;

        let grid_warp = build_program(
            device,
            "grid_warp",
            SHADER_GRID_WARP,
            "fs_main",
            1,
            Some(size_non_zero::<GridWarpUniform>()),
            WORKING_TEXTURE_FORMAT,
        )?;

        let text_slug = build_text_slug_program(device)?;
        let text2d_jfa_seed = build_program(
            device,
            "text2d_jfa_seed",
            SHADER_TEXT2D_JFA_SEED,
            "fs_main",
            1,
            Some(size_non_zero::<Text2DJfaUniform>()),
            WORKING_TEXTURE_FORMAT,
        )?;
        let text2d_jfa_step = build_program(
            device,
            "text2d_jfa_step",
            SHADER_TEXT2D_JFA_STEP,
            "fs_main",
            1,
            Some(size_non_zero::<Text2DJfaUniform>()),
            WORKING_TEXTURE_FORMAT,
        )?;
        let text2d_fx = build_program(
            device,
            "text2d_fx",
            SHADER_TEXT2D_FX,
            "fs_main",
            2,
            Some(size_non_zero::<Text2DFxUniform>()),
            WORKING_TEXTURE_FORMAT,
        )?;

        let backdrop_scrim_blur = build_program(
            device,
            "backdrop_scrim_blur",
            SHADER_BACKDROP_SCRIM_BLUR,
            "fs_main",
            1,
            Some(size_non_zero::<BackdropScrimUniform>()),
            WORKING_TEXTURE_FORMAT,
        )?;

        let backdrop_scrim_composite = build_program(
            device,
            "backdrop_scrim_composite",
            SHADER_BACKDROP_SCRIM_COMPOSITE,
            "fs_main",
            2,
            Some(size_non_zero::<BackdropScrimUniform>()),
            WORKING_TEXTURE_FORMAT,
        )?;

        Ok(Self {
            common_fullscreen,
            transform,
            mask_matte,
            blend,
            color_basic,
            transition_whip_chromatic,
            transition_cross_zoom,
            transition_directional_warp,
            transition_glitch_displace,
            transition_dreamy_zoom,
            transition_swirl,
            transition_linear_blur,
            transition_burn,
            transition_ripple,
            transition_kaleidoscope,
            fx_motion_tile_shake,
            fx_blur_mocurves,
            fx_anime_impact_frame,
            fx_radial_shockwave,
            fx_halftone_glow,
            fx_bloom_downsample,
            fx_bloom_upsample,
            fx_anime_sharpen,
            fx_film_optics,
            fx_film_optics_lens,
            fx_film_optics_composite,
            fx_film_halation,
            fx_aa,
            mipmap_downsample,
            motion_blur_accumulate: ShaderProgram {
                bind_group_layout: mb_layout,
                pipeline: mb_accum_pipeline,
            },
            motion_blur_tonemap_srgb: ShaderProgram {
                bind_group_layout: mb_tonemap_layout,
                pipeline: mb_tonemap_pipeline,
            },
            lut3d,
            transform3d,
            transform3d_scene,
            text3d_mesh,
            text3d_mesh_echo,
            particle_field3d,
            particle_field3d_scene,
            ribbon_trail3d,
            ribbon_trail3d_scene,
            ribbon_trail3d_normal,
            ribbon_trail3d_scene_normal,
            fx_dof_coc,
            fx_dof_coc_v,
            fx_dof_bokeh,
            scene3d_msaa4x,
            corner_pin,
            grid_warp,
            text_slug,
            text2d_jfa_seed,
            text2d_jfa_step,
            text2d_fx,
            backdrop_scrim_blur,
            backdrop_scrim_composite,
            custom_wgsl_cache: HashMap::new(),
        })
    }

    /// Returns the number of cached runtime `CustomWgsl` pipelines.
    pub fn custom_wgsl_cache_len(&self) -> usize {
        self.custom_wgsl_cache.len()
    }

    /// Wraps a user-supplied WGSL fragment body with standard `@group(0)` bindings
    /// (`0..=2` textures, `linear_sampler`, `CustomWgslUniform`) and fullscreen `vs_main`,
    /// statically rejects unbounded `loop` / `while` loops, validates via `wgpu` validation error scopes,
    /// and caches compiled pipelines by `(fnv1a_64(wgsl_source), input_count)`.
    pub fn compile_custom_wgsl(
        &mut self,
        device: &wgpu::Device,
        wgsl_source: &str,
        input_count: u8,
    ) -> Result<Arc<ShaderProgram>, RenderError> {
        if input_count > 2 {
            return Err(RenderError::ShaderCompilation {
                shader: "custom_wgsl".into(),
                message: format!("expected_inputs must be 0..=2, got {input_count}"),
            });
        }
        if wgsl_source.trim().is_empty() {
            return Err(RenderError::ShaderCompilation {
                shader: "custom_wgsl".into(),
                message: "wgsl_source must not be empty".into(),
            });
        }
        if contains_unbounded_loop(wgsl_source) {
            return Err(RenderError::ShaderCompilation {
                shader: "custom_wgsl".into(),
                message: "'loop' and 'while' are forbidden in CustomWgsl sandbox".into(),
            });
        }

        let cache_key = (fnv1a_64(wgsl_source.as_bytes()), input_count);
        if let Some(cached) = self.custom_wgsl_cache.get(&cache_key) {
            return Ok(Arc::clone(cached));
        }

        let wrapped = wrap_custom_wgsl(wgsl_source, input_count);
        let program = build_program(
            device,
            "custom_wgsl",
            &wrapped,
            "fs_main",
            u32::from(input_count),
            Some(size_non_zero::<CustomWgslUniform>()),
            WORKING_TEXTURE_FORMAT,
        )?;
        let arc = Arc::new(program);
        self.custom_wgsl_cache.insert(cache_key, Arc::clone(&arc));
        Ok(arc)
    }
}

fn wrap_custom_wgsl(fragment_body: &str, input_count: u8) -> String {
    let bindings = match input_count {
        0 => "\
@group(0) @binding(0)
var linear_sampler: sampler;
@group(0) @binding(1)
var<uniform> u_custom: CustomWgslUniform;",
        1 => "\
@group(0) @binding(0)
var tex0: texture_2d<f32>;
@group(0) @binding(1)
var linear_sampler: sampler;
@group(0) @binding(2)
var<uniform> u_custom: CustomWgslUniform;",
        _ => "\
@group(0) @binding(0)
var tex0: texture_2d<f32>;
@group(0) @binding(1)
var tex1: texture_2d<f32>;
@group(0) @binding(2)
var linear_sampler: sampler;
@group(0) @binding(3)
var<uniform> u_custom: CustomWgslUniform;",
    };

    format!(
        r#"struct VertexOutput {{
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}}

struct CustomWgslUniform {{
    resolution_time_tick: vec4<f32>,
    params: array<vec4<f32>, 4>,
}}

{bindings}

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VertexOutput {{
    let x = f32((vid << 1u) & 2u);
    let y = f32(vid & 2u);
    var out: VertexOutput;
    out.position = vec4<f32>(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, 1.0 - y);
    return out;
}}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {{
    let uv = in.uv;
    let resolution = u_custom.resolution_time_tick.xy;
    let time_sec = u_custom.resolution_time_tick.z;
    let tick = u_custom.resolution_time_tick.w;
    let params = u_custom.params;
    {{
{fragment_body}
    }}
}}
"#
    )
}

/// Advances past a WGSL `//` line comment or nested `/* ... */` block comment starting at `idx`.
fn skip_wgsl_comment(bytes: &[u8], idx: usize) -> Option<usize> {
    let len = bytes.len();
    if idx + 1 >= len || bytes[idx] != b'/' {
        return None;
    }
    if bytes[idx + 1] == b'/' {
        let mut i = idx + 2;
        while i < len && bytes[i] != b'\n' {
            i += 1;
        }
        return Some(i);
    }
    if bytes[idx + 1] == b'*' {
        let mut i = idx + 2;
        let mut depth = 1usize;
        while i < len && depth > 0 {
            if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'*' {
                depth += 1;
                i += 2;
            } else if i + 1 < len && bytes[i] == b'*' && bytes[i + 1] == b'/' {
                depth -= 1;
                i += 2;
            } else {
                i += 1;
            }
        }
        return Some(i);
    }
    None
}

/// Comment-aware check for WGSL `loop {` and `while` loop keywords (which can cause TDR hangs).
fn contains_unbounded_loop(src: &str) -> bool {
    let bytes = src.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        if let Some(next_i) = skip_wgsl_comment(bytes, i) {
            i = next_i;
            continue;
        }

        if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            let start = i;
            i += 1;
            while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let ident = &bytes[start..i];
            if ident == b"while" {
                return true;
            }
            if ident == b"loop" {
                let mut j = i;
                while j < len {
                    if bytes[j].is_ascii_whitespace() {
                        j += 1;
                    } else if let Some(next_j) = skip_wgsl_comment(bytes, j) {
                        j = next_j;
                    } else {
                        break;
                    }
                }
                if j < len && bytes[j] == b'{' {
                    return true;
                }
            }
            continue;
        }
        i += 1;
    }

    false
}

fn size_non_zero<T>() -> NonZeroU64 {
    NonZeroU64::new(std::mem::size_of::<T>() as u64).expect("uniform struct must be non-empty")
}

fn build_program(
    device: &wgpu::Device,
    name: &str,
    wgsl_source: &str,
    fs_entry: &str,
    texture_count: u32,
    uniform_size: Option<NonZeroU64>,
    target_format: wgpu::TextureFormat,
) -> Result<ShaderProgram, RenderError> {
    let dims = vec![wgpu::TextureViewDimension::D2; texture_count as usize];
    build_program_with_dims(
        device,
        name,
        wgsl_source,
        fs_entry,
        &dims,
        uniform_size,
        target_format,
    )
}

fn build_program_with_dims(
    device: &wgpu::Device,
    name: &str,
    wgsl_source: &str,
    fs_entry: &str,
    texture_dims: &[wgpu::TextureViewDimension],
    uniform_size: Option<NonZeroU64>,
    target_format: wgpu::TextureFormat,
) -> Result<ShaderProgram, RenderError> {
    let module = compile_wgsl_module(device, name, wgsl_source)?;
    let bind_group_layout =
        create_bind_group_layout_with_dims(device, name, texture_dims, uniform_size);
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(&format!("{name}-pipeline-layout")),
        bind_group_layouts: &[Some(&bind_group_layout)],
        immediate_size: 0,
    });
    let pipeline = create_fullscreen_pipeline(
        device,
        name,
        &module,
        fs_entry,
        &pipeline_layout,
        target_format,
    )?;
    Ok(ShaderProgram {
        bind_group_layout,
        pipeline,
    })
}

fn compile_wgsl_module(
    device: &wgpu::Device,
    name: &str,
    wgsl_source: &str,
) -> Result<wgpu::ShaderModule, RenderError> {
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(name),
        source: wgpu::ShaderSource::Wgsl(wgsl_source.into()),
    });
    if let Some(err) = pollster::block_on(scope.pop()) {
        return Err(RenderError::ShaderCompilation {
            shader: name.to_owned(),
            message: err.to_string(),
        });
    }
    Ok(module)
}

fn create_bind_group_layout(
    device: &wgpu::Device,
    name: &str,
    texture_count: u32,
    uniform_size: Option<NonZeroU64>,
) -> wgpu::BindGroupLayout {
    let dims = vec![wgpu::TextureViewDimension::D2; texture_count as usize];
    create_bind_group_layout_with_dims(device, name, &dims, uniform_size)
}

fn create_bind_group_layout_with_dims(
    device: &wgpu::Device,
    name: &str,
    texture_dims: &[wgpu::TextureViewDimension],
    uniform_size: Option<NonZeroU64>,
) -> wgpu::BindGroupLayout {
    let texture_count = texture_dims.len() as u32;
    let mut entries = Vec::with_capacity(texture_dims.len() + 2);
    for (idx, &dim) in texture_dims.iter().enumerate() {
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: idx as u32,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: dim,
                multisampled: false,
            },
            count: None,
        });
    }
    entries.push(wgpu::BindGroupLayoutEntry {
        binding: texture_count,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    });
    if let Some(min_size) = uniform_size {
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: texture_count + 1,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: Some(min_size),
            },
            count: None,
        });
    }

    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(&format!("{name}-bgl")),
        entries: &entries,
    })
}

fn create_fullscreen_pipeline(
    device: &wgpu::Device,
    name: &str,
    module: &wgpu::ShaderModule,
    fs_entry: &str,
    layout: &wgpu::PipelineLayout,
    target_format: wgpu::TextureFormat,
) -> Result<wgpu::RenderPipeline, RenderError> {
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(&format!("{name}-pipeline")),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(fs_entry),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: target_format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });
    if let Some(err) = pollster::block_on(scope.pop()) {
        return Err(RenderError::ShaderCompilation {
            shader: format!("{name}::{fs_entry}"),
            message: err.to_string(),
        });
    }
    Ok(pipeline)
}

#[allow(clippy::too_many_arguments)]
fn build_3d_program(
    device: &wgpu::Device,
    name: &str,
    wgsl_source: &str,
    has_src_texture: bool,
    uniform_size: NonZeroU64,
    vertex_buffers: &[Option<wgpu::VertexBufferLayout<'_>>],
    topology: wgpu::PrimitiveTopology,
    depth_stencil: Option<wgpu::DepthStencilState>,
    blend: Option<wgpu::BlendState>,
    sample_count: u32,
) -> Result<ShaderProgram, RenderError> {
    let module = compile_wgsl_module(device, name, wgsl_source)?;
    let mut entries = Vec::with_capacity(if has_src_texture { 3 } else { 1 });
    let uniform_binding = if has_src_texture {
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        });
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        });
        2
    } else {
        0
    };
    entries.push(wgpu::BindGroupLayoutEntry {
        binding: uniform_binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: Some(uniform_size),
        },
        count: None,
    });

    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(&format!("{name}-bgl")),
        entries: &entries,
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(&format!("{name}-pipeline-layout")),
        bind_group_layouts: &[Some(&bind_group_layout)],
        immediate_size: 0,
    });

    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(&format!("{name}-pipeline")),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: vertex_buffers,
        },
        primitive: wgpu::PrimitiveState {
            topology,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil,
        multisample: wgpu::MultisampleState {
            count: sample_count.max(1),
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: WORKING_TEXTURE_FORMAT,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });
    if let Some(err) = pollster::block_on(scope.pop()) {
        return Err(RenderError::ShaderCompilation {
            shader: format!("{name}::fs_main"),
            message: err.to_string(),
        });
    }

    Ok(ShaderProgram {
        bind_group_layout,
        pipeline,
    })
}

fn build_dof_coc_programs(device: &wgpu::Device) -> Result<(ShaderProgram, ShaderProgram), RenderError> {
    let module = compile_wgsl_module(device, "fx_dof_coc", SHADER_FX_DOF_COC)?;
    let bind_group_layout_h = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("fx_dof_coc-bgl"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: Some(size_non_zero::<DofCocUniform>()),
                },
                count: None,
            },
        ],
    });
    let pipeline_layout_h = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("fx_dof_coc_h-pipeline-layout"),
        bind_group_layouts: &[Some(&bind_group_layout_h)],
        immediate_size: 0,
    });
    let pipeline_h = create_fullscreen_pipeline(
        device,
        "fx_dof_coc_h",
        &module,
        "fs_coc_h",
        &pipeline_layout_h,
        WORKING_TEXTURE_FORMAT,
    )?;

    let bind_group_layout_v = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("fx_dof_coc_v-bgl"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: Some(size_non_zero::<DofCocUniform>()),
                },
                count: None,
            },
        ],
    });
    let pipeline_layout_v = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("fx_dof_coc_v-pipeline-layout"),
        bind_group_layouts: &[Some(&bind_group_layout_v)],
        immediate_size: 0,
    });
    let pipeline_v = create_fullscreen_pipeline(
        device,
        "fx_dof_coc_v",
        &module,
        "fs_coc_v",
        &pipeline_layout_v,
        WORKING_TEXTURE_FORMAT,
    )?;

    Ok((
        ShaderProgram {
            bind_group_layout: bind_group_layout_h,
            pipeline: pipeline_h,
        },
        ShaderProgram {
            bind_group_layout: bind_group_layout_v,
            pipeline: pipeline_v,
        },
    ))
}

fn build_dof_coc_msaa_program(device: &wgpu::Device) -> Result<ShaderProgram, RenderError> {
    let module = compile_wgsl_module(device, "fx_dof_coc_msaa", SHADER_FX_DOF_COC_MSAA)?;
    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("fx_dof_coc_msaa-bgl"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: true,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: Some(size_non_zero::<DofCocUniform>()),
                },
                count: None,
            },
        ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("fx_dof_coc_msaa-pipeline-layout"),
        bind_group_layouts: &[Some(&bind_group_layout)],
        immediate_size: 0,
    });
    let pipeline = create_fullscreen_pipeline(
        device,
        "fx_dof_coc_msaa",
        &module,
        "fs_coc_h_msaa",
        &pipeline_layout,
        WORKING_TEXTURE_FORMAT,
    )?;
    Ok(ShaderProgram {
        bind_group_layout,
        pipeline,
    })
}

fn build_text_slug_program(device: &wgpu::Device) -> Result<ShaderProgram, RenderError> {
    let module = compile_wgsl_module(device, "text_slug", SHADER_TEXT_SLUG)?;
    let storage_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: if binding == 1 {
            wgpu::ShaderStages::VERTEX_FRAGMENT
        } else {
            wgpu::ShaderStages::FRAGMENT
        },
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };

    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("text_slug-bgl"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: Some(size_non_zero::<SlugUniform>()),
                },
                count: None,
            },
            storage_entry(1),
            storage_entry(2),
            storage_entry(3),
            storage_entry(4),
            storage_entry(5),
        ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("text_slug-pipeline-layout"),
        bind_group_layouts: &[Some(&bind_group_layout)],
        immediate_size: 0,
    });

    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("text_slug-pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Some(GpuSlugVertex::vertex_buffer_layout())],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: WORKING_TEXTURE_FORMAT,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });
    if let Some(err) = pollster::block_on(scope.pop()) {
        return Err(RenderError::ShaderCompilation {
            shader: "text_slug::fs_main".to_owned(),
            message: err.to_string(),
        });
    }

    Ok(ShaderProgram {
        bind_group_layout,
        pipeline,
    })
}
