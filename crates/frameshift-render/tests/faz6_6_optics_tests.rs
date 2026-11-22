use frameshift_render::{
    DitherMode, FnFrameProvider, GpuCompositor, MsaaTargetCache, NoopAnimator, OutputAlphaMode,
    RenderOptions, SourceFrame, TonemapMode, compute_mip_levels,
};
use frameshift_schema::{
    ColorSpace, GraphNodeDef, NodeId, NodeKind, RenderGraphDef, SampleFilter, SourceFit,
};
use frameshift_time::{FLICKS_PER_SECOND, FPS_30, Flicks};

fn hdr_glow_spot(width: u32, height: u32) -> SourceFrame {
    let mut data = vec![[0.0f32, 0.0, 0.0, 0.0]; (width * height) as usize];
    let cx0 = width / 2 - 4;
    let cx1 = width / 2 + 4;
    let cy0 = height / 2 - 4;
    let cy1 = height / 2 + 4;
    for y in cy0..cy1 {
        for x in cx0..cx1 {
            data[(y * width + x) as usize] = [6.0, 1.5, 0.4, 1.0];
        }
    }
    SourceFrame::rgba_f32_premul(width, height, data)
}

fn gradient_hdr_ramp(width: u32, height: u32) -> SourceFrame {
    let mut data = vec![[0.0f32, 0.0, 0.0, 1.0]; (width * height) as usize];
    for y in 0..height {
        for x in 0..width {
            let t = x as f32 / (width.max(2) - 1) as f32;
            let v = if y < 4 {
                0.0
            } else if y >= height - 4 {
                1.0
            } else {
                t * 0.25
            };
            data[(y * width + x) as usize] = [v, v, v, 1.0];
        }
    }
    SourceFrame::rgba_f32_premul(width, height, data)
}

fn diagonal_edge_plate(width: u32, height: u32) -> SourceFrame {
    let mut data = vec![[0.02f32, 0.02, 0.04, 1.0]; (width * height) as usize];
    for y in 0..height {
        for x in 0..width {
            if x > y {
                data[(y * width + x) as usize] = [2.5, 2.5, 2.8, 1.0];
            }
        }
    }
    SourceFrame::rgba_f32_premul(width, height, data)
}

fn checker_plate(width: u32, height: u32) -> SourceFrame {
    let mut data = vec![[0.0f32, 0.0, 0.0, 1.0]; (width * height) as usize];
    for y in 0..height {
        for x in 0..width {
            let on = ((x / 2) + (y / 2)) % 2 == 0;
            let v = if on { 0.9 } else { 0.05 };
            data[(y * width + x) as usize] = [v, v * 0.8, v * 0.6, 1.0];
        }
    }
    SourceFrame::rgba_f32_premul(width, height, data)
}

fn render_with(
    compositor: &mut GpuCompositor,
    graph: &RenderGraphDef,
    width: u32,
    height: u32,
    options: &RenderOptions,
) -> frameshift_render::RenderedFrame {
    let provider = FnFrameProvider(|asset_id: &str, _: Flicks, w: u32, h: u32| {
        Ok(match asset_id {
            "spot" => hdr_glow_spot(w, h),
            "ramp" => gradient_hdr_ramp(w, h),
            "diag" => diagonal_edge_plate(w, h),
            "checker" => checker_plate(w, h),
            _ => hdr_glow_spot(w, h),
        })
    });
    compositor
        .render_frame(
            graph,
            Flicks::ZERO,
            Flicks(FLICKS_PER_SECOND / 30),
            width,
            height,
            &provider,
            &NoopAnimator,
            options,
        )
        .expect("render_frame")
}

#[test]
fn agx_pbr_neutral_and_tpdf_dither_work_and_guard_extremes() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let (w, h) = (64, 32);
    let graph = RenderGraphDef {
        output_node: NodeId::from("src"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("src"),
            kind: NodeKind::SourceVideo {
                asset_id: "ramp".into(),
                clip_id: None,
                color_space: ColorSpace::LinearRec709,
                fit: SourceFit::Stretch,
            },
            inputs: vec![],
            enabled: true,
        }],
    };

    for mode in [
        TonemapMode::Agx,
        TonemapMode::PbrNeutral,
        TonemapMode::AgxPunchy,
    ] {
        let f = render_with(
            &mut compositor,
            &graph,
            w,
            h,
            &RenderOptions {
                tonemap_mode: Some(mode),
                output_alpha_mode: OutputAlphaMode::Straight,
                ..Default::default()
            },
        );
        let mid_idx = (((h / 2) * w + (w / 2)) * 4) as usize;
        // Neutral grey must remain channel-balanced
        let r = f.rgba8[mid_idx];
        let g = f.rgba8[mid_idx + 1];
        let b = f.rgba8[mid_idx + 2];
        assert!(
            r.abs_diff(g) <= 2 && g.abs_diff(b) <= 2,
            "{mode:?} broke grey neutrality: [{r},{g},{b}]"
        );
    }

    let no_dither = render_with(
        &mut compositor,
        &graph,
        w,
        h,
        &RenderOptions {
            tonemap_mode: Some(TonemapMode::None),
            dither_mode: DitherMode::Off,
            ..Default::default()
        },
    );
    let with_dither = render_with(
        &mut compositor,
        &graph,
        w,
        h,
        &RenderOptions {
            tonemap_mode: Some(TonemapMode::None),
            dither_mode: DitherMode::TpdfIgn,
            ..Default::default()
        },
    );
    let with_dither_repeat = render_with(
        &mut compositor,
        &graph,
        w,
        h,
        &RenderOptions {
            tonemap_mode: Some(TonemapMode::None),
            dither_mode: DitherMode::TpdfIgn,
            ..Default::default()
        },
    );
    assert_eq!(with_dither.fnv1a_hash, with_dither_repeat.fnv1a_hash);
    assert_ne!(no_dither.fnv1a_hash, with_dither.fnv1a_hash);

    // Pure black rows (y < 4) must remain [0, 0, 0] and pure white rows (y >= h - 4) [255, 255, 255]
    for x in 0..w {
        let top = (x * 4) as usize;
        assert_eq!(&with_dither.rgba8[top..top + 3], &[0, 0, 0]);
        let bot = (((h - 1) * w + x) * 4) as usize;
        assert_eq!(&with_dither.rgba8[bot..bot + 3], &[255, 255, 255]);
    }
}

#[test]
fn additive_glow_with_zero_alpha_survives_downstream_color_lut_sharpen_and_tonemap() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let (w, h) = (64, 64);
    let graph = RenderGraphDef {
        output_node: NodeId::from("sharp"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("src"),
                kind: NodeKind::SourceVideo {
                    asset_id: "spot".into(),
                    clip_id: None,
                    color_space: ColorSpace::LinearRec709,
                    fit: SourceFit::Stretch,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("glow"),
                kind: NodeKind::DeepGlow {
                    threshold: 0.5,
                    knee: 0.25,
                    intensity: 2.0,
                    radius: 1.5,
                    tint_rgb: [1.0, 0.85, 0.7],
                    mip_levels: 5,
                    scatter: 0.7,
                    anamorphic_ratio: 0.0,
                    chromatic_shift: 0.2,
                },
                inputs: vec![NodeId::from("src")],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("grade"),
                kind: NodeKind::ColorBasic {
                    exposure: 0.2,
                    contrast: 1.1,
                    saturation: 1.05,
                    flash_amount: 0.0,
                },
                inputs: vec![NodeId::from("glow")],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("lut"),
                kind: NodeKind::Lut3D {
                    input: NodeId::from("grade"),
                    lut_preset: "identity".into(),
                    intensity: 1.0,
                    lut_source: None,
                    input_space: None,
                    output_space: None,
                },
                inputs: vec![NodeId::from("grade")],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("sharp"),
                kind: NodeKind::AnimeSharpen {
                    line_strength: 0.2,
                    sharpen_amount: 0.2,
                    anti_ringing: 0.8,
                },
                inputs: vec![NodeId::from("lut")],
                enabled: true,
            },
        ],
    };

    let out = render_with(
        &mut compositor,
        &graph,
        w,
        h,
        &RenderOptions {
            tonemap_mode: Some(TonemapMode::Agx),
            output_alpha_mode: OutputAlphaMode::Premultiplied,
            ..Default::default()
        },
    );
    // Neighbor pixel (20, 32) has alpha == 0.0 from source, only DeepGlow additive RGB!
    let idx = ((32 * w + 20) * 4) as usize;
    assert_eq!(out.rgba8[idx + 3], 0);
    assert!(
        out.rgba8[idx] > 0 || out.rgba8[idx + 1] > 0 || out.rgba8[idx + 2] > 0,
        "additive glow on alpha==0 must survive ColorBasic+Lut3D+AnimeSharpen+Tonemap, got {:?}",
        &out.rgba8[idx..idx + 4]
    );

    // Truly empty frame (alpha == 0 && rgb == 0) must stay [0, 0, 0, 0] through the exact same chain
    let empty_provider = FnFrameProvider(|_: &str, _: Flicks, ew: u32, eh: u32| {
        Ok(SourceFrame::rgba_f32_premul(
            ew,
            eh,
            vec![[0.0f32; 4]; (ew * eh) as usize],
        ))
    });
    let empty_out = compositor
        .render_frame(
            &graph,
            Flicks::ZERO,
            Flicks(FLICKS_PER_SECOND / 30),
            w,
            h,
            &empty_provider,
            &NoopAnimator,
            &RenderOptions {
                tonemap_mode: Some(TonemapMode::Agx),
                output_alpha_mode: OutputAlphaMode::Premultiplied,
                ..Default::default()
            },
        )
        .expect("empty render");
    assert_eq!(&empty_out.rgba8[0..4], &[0, 0, 0, 0]);
}

#[test]
fn deep_glow_anamorphic_stretches_horizontally_and_film_optics_auto_scales_and_halates() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let (w, h) = (64, 64);

    let anamorphic_graph = RenderGraphDef {
        output_node: NodeId::from("glow"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("src"),
                kind: NodeKind::SourceVideo {
                    asset_id: "spot".into(),
                    clip_id: None,
                    color_space: ColorSpace::LinearRec709,
                    fit: SourceFit::Stretch,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("glow"),
                kind: NodeKind::DeepGlow {
                    threshold: 0.5,
                    knee: 0.25,
                    intensity: 2.5,
                    radius: 1.8,
                    tint_rgb: [1.0, 1.0, 1.0],
                    mip_levels: 5,
                    scatter: 0.75,
                    anamorphic_ratio: 0.95,
                    chromatic_shift: -0.4,
                },
                inputs: vec![NodeId::from("src")],
                enabled: true,
            },
        ],
    };
    let anam = render_with(
        &mut compositor,
        &anamorphic_graph,
        w,
        h,
        &RenderOptions {
            tonemap_mode: Some(TonemapMode::PbrNeutral),
            ..Default::default()
        },
    );
    let h_idx = ((32 * w + 8) * 4) as usize;
    let v_idx = ((8 * w + 32) * 4) as usize;
    let h_energy = anam.rgba8[h_idx] as u32 + anam.rgba8[h_idx + 1] as u32 + anam.rgba8[h_idx + 2] as u32;
    let v_energy = anam.rgba8[v_idx] as u32 + anam.rgba8[v_idx + 1] as u32 + anam.rgba8[v_idx + 2] as u32;
    assert!(
        h_energy > v_energy,
        "anamorphic bloom should spread wider horizontally ({h_energy}) than vertically ({v_energy})"
    );

    // FilmOptics auto_scale=true with positive pincushion k1=0.35 must not leave black corners on opaque input
    let optics_graph = RenderGraphDef {
        output_node: NodeId::from("optics"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("src"),
                kind: NodeKind::SourceVideo {
                    asset_id: "diag".into(),
                    clip_id: None,
                    color_space: ColorSpace::LinearRec709,
                    fit: SourceFit::Stretch,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("optics"),
                kind: NodeKind::FilmOptics {
                    lens_k1: 0.35,
                    lens_k2: 0.05,
                    chromatic_edge: 0.02,
                    grain_amount: 0.12,
                    grain_seed: 3.0,
                    halation_amount: 0.6,
                    vignette_amount: 0.1,
                    auto_scale: true,
                    grain_size_px: 1.65,
                    grain_chroma: 0.25,
                    halation_threshold: 0.65,
                },
                inputs: vec![NodeId::from("src")],
                enabled: true,
            },
        ],
    };
    let opt_out = render_with(
        &mut compositor,
        &optics_graph,
        w,
        h,
        &RenderOptions {
            tonemap_mode: Some(TonemapMode::Agx),
            ..Default::default()
        },
    );
    // Top-right corner is bright in `diag`; auto_scale prevents black border clipping
    let tr_idx = ((w - 1) * 4) as usize;
    assert_eq!(opt_out.rgba8[tr_idx + 3], 255);
    assert!(
        opt_out.rgba8[tr_idx] > 20,
        "auto_scale FilmOptics corner should remain lit, got {:?}",
        &opt_out.rgba8[tr_idx..tr_idx + 4]
    );
}

#[test]
fn fxaa_mipmap_minification_and_msaa_pool_helpers_work() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let (w, h) = (64, 64);

    let diag_graph = RenderGraphDef {
        output_node: NodeId::from("src"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("src"),
            kind: NodeKind::SourceVideo {
                asset_id: "diag".into(),
                clip_id: None,
                color_space: ColorSpace::LinearRec709,
                fit: SourceFit::Stretch,
            },
            inputs: vec![],
            enabled: true,
        }],
    };
    let raw = render_with(
        &mut compositor,
        &diag_graph,
        w,
        h,
        &RenderOptions {
            tonemap_mode: Some(TonemapMode::AcesLite),
            post_aa: false,
            ..Default::default()
        },
    );
    let aa = render_with(
        &mut compositor,
        &diag_graph,
        w,
        h,
        &RenderOptions {
            tonemap_mode: Some(TonemapMode::AcesLite),
            post_aa: true,
            ..Default::default()
        },
    );
    assert_ne!(raw.fnv1a_hash, aa.fnv1a_hash);

    // Minified Transform2D exercises mipmap_downsample + textureSampleGrad
    let min_graph = RenderGraphDef {
        output_node: NodeId::from("xform"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("src"),
                kind: NodeKind::SourceVideo {
                    asset_id: "checker".into(),
                    clip_id: None,
                    color_space: ColorSpace::LinearRec709,
                    fit: SourceFit::Stretch,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("xform"),
                kind: NodeKind::Transform2D {
                    translate: [0.0, 0.0],
                    scale: [0.25, 0.25],
                    rotation_rad: 0.2,
                    anchor: [0.5, 0.5],
                    filter: SampleFilter::Bilinear,
                },
                inputs: vec![NodeId::from("src")],
                enabled: true,
            },
        ],
    };
    let min_out = render_with(
        &mut compositor,
        &min_graph,
        w,
        h,
        &RenderOptions {
            tonemap_mode: Some(TonemapMode::None),
            ..Default::default()
        },
    );
    let center_idx = ((32 * w + 32) * 4) as usize;
    assert_eq!(min_out.rgba8[center_idx + 3], 255);
    assert_eq!(compute_mip_levels(64, 64), 7);

    // Pool MSAA target helpers work when supports_msaa_4x is true
    if compositor.context().supports_msaa_4x {
        let mut cache = MsaaTargetCache::default();
        let _c = cache.ensure_color(&compositor.context().device, 64, 64, 4);
        let _d = cache.ensure_depth(&compositor.context().device, 64, 64, 4);
        cache.clear();
    }
    let _ = FPS_30;
}
