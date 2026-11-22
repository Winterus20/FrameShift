use frameshift_render::{
    FnFrameProvider, GpuCompositor, NoopAnimator, OutputAlphaMode, RenderOptions, SourceFrame,
    TonemapMode,
};
use frameshift_schema::{
    BevelProfile3D, BlendMode, Camera3DDef, Camera3DMode, ColorSpace, GraphNodeDef, NodeId,
    NodeKind, RenderGraphDef, Shape3DKind,
};
use frameshift_time::{FLICKS_PER_SECOND, Flicks};
use std::collections::HashSet;
use std::path::Path;

fn test_opts() -> RenderOptions {
    RenderOptions {
        tonemap_mode: Some(TonemapMode::None),
        output_alpha_mode: OutputAlphaMode::Straight,
        ..Default::default()
    }
}

fn find_windows_ttf() -> String {
    for candidate in [
        r"C:\Windows\Fonts\arialbd.ttf",
        r"C:\Windows\Fonts\arial.ttf",
        r"C:\Windows\Fonts\impact.ttf",
    ] {
        if Path::new(candidate).exists() {
            return candidate.to_string();
        }
    }
    panic!("expected at least one standard Windows TTF font in C:\\Windows\\Fonts");
}

fn render_graph(
    compositor: &mut GpuCompositor,
    graph: &RenderGraphDef,
    width: u32,
    height: u32,
) -> frameshift_render::RenderedFrame {
    let provider = FnFrameProvider(|asset_id: &str, _: Flicks, w: u32, h: u32| {
        if asset_id == "blue_bg" {
            return Ok(SourceFrame::rgba_f32_premul(
                w,
                h,
                vec![[0.05, 0.15, 0.85, 1.0]; (w * h) as usize],
            ));
        }
        Ok(SourceFrame::rgba8_straight(
            w,
            h,
            vec![255; (w * h * 4) as usize],
        ))
    });
    compositor
        .render_frame(
            graph,
            Flicks(FLICKS_PER_SECOND / 2),
            Flicks(FLICKS_PER_SECOND / 30),
            width,
            height,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("render_frame must succeed")
}

fn count_non_zero_alpha(rgba8: &[u8]) -> usize {
    rgba8.as_chunks::<4>().0.iter().filter(|px| px[3] > 0).count()
}

fn max_rgb_luma(rgba8: &[u8]) -> u8 {
    rgba8
        .as_chunks::<4>()
        .0
        .iter()
        .map(|px| px[0].max(px[1]).max(px[2]))
        .max()
        .unwrap_or(0)
}

fn mean_visible_luma(rgba8: &[u8]) -> f32 {
    let mut sum = 0.0f64;
    let mut count = 0usize;
    for px in rgba8.as_chunks::<4>().0 {
        if px[3] > 16 {
            sum += f64::from(px[0].max(px[1]).max(px[2]));
            count += 1;
        }
    }
    if count == 0 {
        0.0
    } else {
        (sum / count as f64) as f32
    }
}

fn center_pixel(rgba8: &[u8], width: u32, height: u32) -> [u8; 4] {
    let x = width / 2;
    let y = height / 2;
    let idx = ((y * width + x) * 4) as usize;
    [rgba8[idx], rgba8[idx + 1], rgba8[idx + 2], rgba8[idx + 3]]
}

#[test]
fn text3d_pbr_camera_relative_ibl_90deg_turn_and_zecho_depth_separation() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let (w, h) = (192, 108);
    let font_path = find_windows_ttf();

    // Rotate Text3D +90 deg around Y (with slight tilt so bevel walls are visible)
    // and place camera near +X axis looking at origin (90-deg side turn).
    let make_side_text = |material_mode: u32, bevel_profile: BevelProfile3D| RenderGraphDef {
        output_node: NodeId::from("scene"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("txt"),
                kind: NodeKind::Text3D {
                    text: "HI".into(),
                    font_path: font_path.clone(),
                    font_size: 0.72,
                    extrude_depth: 0.20,
                    bevel_size: 0.055,
                    color_top_rgba: [0.95, 0.98, 1.0, 1.0],
                    color_bottom_rgba: [0.75, 0.85, 1.0, 1.0],
                    side_color_rgba: [0.35, 0.45, 0.75, 1.0],
                    pos: [0.0, 0.0, 0.0],
                    rot_deg: [14.0, 76.0, 0.0],
                    camera_pos: [2.4, 0.0, 0.0],
                    camera_rot_deg: [0.0, 90.0, 0.0],
                    fov_deg: 50.0,
                    char_stagger_sec: 0.0,
                    char_bounce_amp: 0.0,
                    char_wave_freq: 0.0,
                    time_offset_sec: 0.0,
                    outline_width: 0.0,
                    outline_color_rgba: [1.0; 4],
                    material_mode,
                    color_mode: 0,
                    palette_rgba: vec![],
                    pop_duration_sec: 0.0,
                    pop_overshoot: 2.15,
                    spin_in_deg: 0.0,
                    char_jitter_deg: 0.0,
                    arc_curvature: 0.0,
                    arc_axis: 0,
                    echo_count: 0,
                    echo_offset: [0.0, 0.0, -0.25],
                    echo_alpha_decay: 0.55,
                    echo_scale_step: 0.94,
                    hdr_intensity: 1.0,
                    opacity: 1.0,
                    align: Default::default(),
                    line_height_em: None,
                    tracking_em: 0.0,
                    bevel_profile,
                    bevel_segments: 4,
                    fallback_fonts: vec![],
                    var_wght: None,
                    var_wdth: None,
                    var_slnt: None,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("scene"),
                kind: NodeKind::Scene3D {
                    camera: Camera3DDef {
                        pos: [2.4, 0.0, 0.0],
                        target: [0.0, 0.0, 0.0],
                        mode: Camera3DMode::LookAt,
                        fov_deg: 50.0,
                        ..Default::default()
                    },
                    background_rgba: [0.0; 4],
                },
                inputs: vec![NodeId::from("txt")],
                enabled: true,
            },
        ],
    };

    let chrome_round = render_graph(
        &mut compositor,
        &make_side_text(1, BevelProfile3D::Round),
        w,
        h,
    );
    let chrome_concave = render_graph(
        &mut compositor,
        &make_side_text(1, BevelProfile3D::Concave),
        w,
        h,
    );
    let neon_round = render_graph(
        &mut compositor,
        &make_side_text(0, BevelProfile3D::Round),
        w,
        h,
    );

    assert!(
        mean_visible_luma(&chrome_round.rgba8) > 95.0,
        "MetallicChrome must remain brightly lit by camera-relative IBL at 90-deg turn (got {})",
        mean_visible_luma(&chrome_round.rgba8)
    );
    assert!(
        mean_visible_luma(&neon_round.rgba8) > 90.0,
        "Neon3D must remain brightly lit at 90-deg turn (got {})",
        mean_visible_luma(&neon_round.rgba8)
    );
    assert_ne!(
        chrome_round.fnv1a_hash, chrome_concave.fnv1a_hash,
        "Different bevel profiles with cavity AO must produce distinct shading"
    );

    // Z-echo depth-write separation: a semi-transparent Z-echo placed in front of a ribbon
    // must NOT write depth and occlude the ribbon behind it.
    let echo_with_ribbon = |echo_count: u32| RenderGraphDef {
        output_node: NodeId::from("scene"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("shape"),
                kind: NodeKind::Shape3D {
                    shape_kind: Shape3DKind::NeonTile,
                    size: 0.9,
                    aspect: 1.0,
                    stroke_width: 0.08,
                    corner_radius: 0.05,
                    inner_ratio: 0.382,
                    extrude_depth: 0.1,
                    bevel_size: 0.01,
                    material_mode: 2,
                    color_top_rgba: [0.1, 0.1, 0.2, 1.0],
                    color_bottom_rgba: [0.1, 0.1, 0.2, 1.0],
                    side_color_rgba: [0.1, 0.1, 0.2, 1.0],
                    outline_color_rgba: [0.1, 0.1, 0.2, 1.0],
                    pos: [0.0, 0.0, -2.0], // hero behind
                    rot_deg: [0.0, 0.0, 0.0],
                    scale: [1.0, 1.0, 1.0],
                    camera_pos: [0.0, 0.0, 2.5],
                    camera_rot_deg: [0.0, 0.0, 0.0],
                    fov_deg: 50.0,
                    pop_duration_sec: 0.0,
                    pop_overshoot: 2.15,
                    spin_in_deg: 0.0,
                    echo_count,
                    echo_offset: [0.0, 0.0, 1.2], // echo moves forward to z = -0.8 / +0.4
                    echo_alpha_decay: 0.35,
                    hdr_intensity: 1.0,
                    time_offset_sec: 0.0,
                    opacity: 1.0,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("ribbon"),
                kind: NodeKind::RibbonTrail3D {
                    ribbon_count: 3,
                    segments: 36,
                    seed: 7,
                    width: 0.08,
                    speed: 0.0,
                    radius: 0.85,
                    trim_start: 0.0,
                    trim_end: 1.0,
                    color_a_rgba: [1.0, 0.2, 0.1, 1.0],
                    color_b_rgba: [1.0, 0.9, 0.1, 1.0],
                    camera_pos: [0.0, 0.0, 2.5],
                    camera_rot_deg: [0.0, 0.0, 0.0],
                    fov_deg: 50.0,
                    time_offset_sec: 0.0,
                    ribbon_style: 1,
                    strand_spacing: 0.1,
                    curve_bend: 0.3,
                    pos: [0.0, 0.0, -0.2], // ribbon between hero and front echo
                    rot_deg: [0.0, 0.0, 0.0],
                    scale: [1.0, 1.0, 1.0],
                    opacity: 1.0,
                    core_whiten: 0.0,
                    core_intensity: 1.0,
                    halo_intensity: 1.0,
                    saturation: 1.0,
                    min_width_px: 0.0,
                    normal_blend: false,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("scene"),
                kind: NodeKind::Scene3D {
                    camera: Camera3DDef {
                        pos: [0.0, 0.0, 2.5],
                        target: [0.0, 0.0, 0.0],
                        mode: Camera3DMode::LookAt,
                        fov_deg: 50.0,
                        ..Default::default()
                    },
                    background_rgba: [0.0; 4],
                },
                inputs: vec![NodeId::from("shape"), NodeId::from("ribbon")],
                enabled: true,
            },
        ],
    };

    let no_echo = render_graph(&mut compositor, &echo_with_ribbon(0), w, h);
    let with_echo = render_graph(&mut compositor, &echo_with_ribbon(2), w, h);
    assert!(
        max_rgb_luma(&with_echo.rgba8) >= 200 && max_rgb_luma(&no_echo.rgba8) >= 200,
        "Ribbon must remain bright and unoccluded behind semi-transparent Z-echoes"
    );
}

#[test]
fn scene3d_ribbon_and_particle_isolated_from_fx_dof_coc_void_corruption() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let (w, h) = (160, 90);

    // Ribbon + Particles at focal plane (z = 0, dist = 2.5 == focal_dist) over empty background,
    // with strong Scene3D DoF enabled. Before Faz 6.6, fx_dof_coc saw raw_d=1.0 -> z=200 and
    // smeared focal-plane ribbons/particles at max_radius_px!
    let make_scene = |blur_strength: f32, with_occluder: bool| {
        let mut nodes = vec![
            GraphNodeDef {
                id: NodeId::from("ribbon"),
                kind: NodeKind::RibbonTrail3D {
                    ribbon_count: 3,
                    segments: 40,
                    seed: 5,
                    width: 0.04,
                    speed: 0.0,
                    radius: 0.85,
                    trim_start: 0.0,
                    trim_end: 1.0,
                    color_a_rgba: [0.1, 0.95, 1.0, 1.0],
                    color_b_rgba: [1.0, 0.2, 0.9, 1.0],
                    camera_pos: [0.0, 0.0, 2.5],
                    camera_rot_deg: [0.0, 0.0, 0.0],
                    fov_deg: 50.0,
                    time_offset_sec: 0.0,
                    ribbon_style: 1,
                    strand_spacing: 0.09,
                    curve_bend: 0.4,
                    pos: [0.0, 0.0, 0.0],
                    rot_deg: [0.0, 0.0, 0.0],
                    scale: [1.0, 1.0, 1.0],
                    opacity: 1.0,
                    core_whiten: 0.0,
                    core_intensity: 1.0,
                    halo_intensity: 1.0,
                    saturation: 1.0,
                    min_width_px: 0.0,
                    normal_blend: false,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("particles"),
                kind: NodeKind::ParticleField3D {
                    count: 16,
                    seed: 12,
                    shape: 1,
                    color_a_rgba: [1.0, 0.9, 0.3, 1.0],
                    color_b_rgba: [1.0, 0.4, 0.8, 1.0],
                    speed: 0.0,
                    spread: 0.9,
                    size: 0.06,
                    camera_pos: [0.0, 0.0, 2.5],
                    camera_rot_deg: [0.0, 0.0, 0.0],
                    fov_deg: 50.0,
                    time_offset_sec: 0.0,
                    dof_focal_dist: 2.5,
                    dof_blur_strength: 0.0,
                    opacity: 1.0,
                },
                inputs: vec![],
                enabled: true,
            },
        ];
        let mut inputs = vec![NodeId::from("ribbon"), NodeId::from("particles")];
        if with_occluder {
            nodes.push(GraphNodeDef {
                id: NodeId::from("occluder"),
                kind: NodeKind::Shape3D {
                    shape_kind: Shape3DKind::Diamond3D,
                    size: 1.2,
                    aspect: 1.0,
                    stroke_width: 0.08,
                    corner_radius: 0.05,
                    inner_ratio: 0.382,
                    extrude_depth: 0.1,
                    bevel_size: 0.0,
                    material_mode: 2,
                    color_top_rgba: [0.05, 0.2, 0.05, 1.0],
                    color_bottom_rgba: [0.05, 0.2, 0.05, 1.0],
                    side_color_rgba: [0.05, 0.2, 0.05, 1.0],
                    outline_color_rgba: [0.05, 0.2, 0.05, 1.0],
                    pos: [0.0, 0.0, 0.7], // in front of ribbon/particles
                    rot_deg: [0.0, 0.0, 0.0],
                    scale: [1.0, 1.0, 1.0],
                    camera_pos: [0.0, 0.0, 2.5],
                    camera_rot_deg: [0.0, 0.0, 0.0],
                    fov_deg: 50.0,
                    pop_duration_sec: 0.0,
                    pop_overshoot: 2.15,
                    spin_in_deg: 0.0,
                    echo_count: 0,
                    echo_offset: [0.0, 0.0, -0.3],
                    echo_alpha_decay: 0.5,
                    hdr_intensity: 1.0,
                    time_offset_sec: 0.0,
                    opacity: 1.0,
                },
                inputs: vec![],
                enabled: true,
            });
            inputs.push(NodeId::from("occluder"));
        }
        nodes.push(GraphNodeDef {
            id: NodeId::from("scene"),
            kind: NodeKind::Scene3D {
                camera: Camera3DDef {
                    pos: [0.0, 0.0, 2.5],
                    target: [0.0, 0.0, 0.0],
                    mode: Camera3DMode::LookAt,
                    fov_deg: 50.0,
                    dof_focal_dist: 2.5,
                    dof_autofocus_target: true,
                    dof_blur_strength: blur_strength,
                    dof_max_radius_px: 28.0,
                    dof_samples: 24,
                    ..Default::default()
                },
                background_rgba: [0.0; 4],
            },
            inputs,
            enabled: true,
        });
        RenderGraphDef {
            output_node: NodeId::from("scene"),
            nodes,
        }
    };

    let sharp = render_graph(&mut compositor, &make_scene(0.0, false), w, h);
    let with_dof = render_graph(&mut compositor, &make_scene(1.8, false), w, h);
    let sharp_px = count_non_zero_alpha(&sharp.rgba8);
    let dof_px = count_non_zero_alpha(&with_dof.rgba8);

    assert!(
        max_rgb_luma(&with_dof.rgba8) > 200,
        "In-focus ribbon core must remain bright under Scene3D DoF"
    );
    // Analytical DoF expands out-of-focus z!=0 particles/ends modestly while keeping focal-plane
    // pixels sharp (without 28px fx_dof_coc void smearing across the entire canvas).
    assert!(
        dof_px < (w * h / 4) as usize && dof_px < sharp_px * 5,
        "Ribbon/particles must not be smeared across empty void by fx_dof_coc (sharp={sharp_px}, dof={dof_px})"
    );

    // Center pixel occluded by front green Shape3D must NOT be overwritten by ribbon behind it.
    let occluded = render_graph(&mut compositor, &make_scene(1.8, true), w, h);
    let cp = center_pixel(&occluded.rgba8, w, h);
    assert!(
        cp[1] >= cp[0] && cp[2] < 100,
        "Front Shape3D must still Z-occlude ribbon/particles behind it under DoF (center={cp:?})"
    );
}

#[test]
fn dof_2pass_separable_coc_dilation_and_polygonal_anamorphic_catseye_bokeh() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let (w, h) = (160, 90);

    // Thin foreground stroke (~2-3px wide) over an opaque blue Transform3D background card.
    let make_dof_graph =
        |blur_strength: f32,
         fg_opacity: f32,
         blades: u32,
         anamorphic: f32,
         cats_eye: f32| {
            RenderGraphDef {
                output_node: NodeId::from("scene"),
                nodes: vec![
                    GraphNodeDef {
                        id: NodeId::from("bg_src"),
                        kind: NodeKind::Solid {
                            color_rgba: [0.05, 0.15, 0.85, 1.0],
                            color_space: ColorSpace::LinearRec709,
                        },
                        inputs: vec![],
                        enabled: true,
                    },
                    GraphNodeDef {
                        id: NodeId::from("bg_card"),
                        kind: NodeKind::Transform3D {
                            pos: [0.0, 0.0, -1.0],
                            rot_deg: [0.0, 0.0, 0.0],
                            scale: [4.0, 4.0, 1.0],
                            fov_deg: 50.0,
                            camera_pos: [0.0, 0.0, 3.0],
                            camera_rot_deg: [0.0, 0.0, 0.0],
                            dof_focal_dist: 4.0,
                            dof_blur_strength: 0.0,
                        },
                        inputs: vec![NodeId::from("bg_src")],
                        enabled: true,
                    },
                    GraphNodeDef {
                        id: NodeId::from("thin_fg"),
                        kind: NodeKind::Shape3D {
                            shape_kind: Shape3DKind::NeonTile,
                            size: 0.7,
                            aspect: 1.0,
                            stroke_width: 0.025, // very thin stroke that sparse 5x5 missed
                            corner_radius: 0.02,
                            inner_ratio: 0.382,
                            extrude_depth: 0.04,
                            bevel_size: 0.0,
                            material_mode: 2,
                            color_top_rgba: [1.0, 0.1, 0.1, 1.0],
                            color_bottom_rgba: [1.0, 0.1, 0.1, 1.0],
                            side_color_rgba: [1.0, 0.1, 0.1, 1.0],
                            outline_color_rgba: [1.0, 0.1, 0.1, 1.0],
                            pos: [-0.85, 0.0, 1.4], // close to camera (near foreground)
                            rot_deg: [0.0, 0.0, 0.0],
                            scale: [1.0, 1.0, 1.0],
                            camera_pos: [0.0, 0.0, 3.0],
                            camera_rot_deg: [0.0, 0.0, 0.0],
                            fov_deg: 50.0,
                            pop_duration_sec: 0.0,
                            pop_overshoot: 2.15,
                            spin_in_deg: 0.0,
                            echo_count: 0,
                            echo_offset: [0.0, 0.0, -0.3],
                            echo_alpha_decay: 0.5,
                            hdr_intensity: 1.8,
                            time_offset_sec: 0.0,
                            opacity: fg_opacity,
                        },
                        inputs: vec![],
                        enabled: true,
                    },
                    GraphNodeDef {
                        id: NodeId::from("scene"),
                        kind: NodeKind::Scene3D {
                            camera: Camera3DDef {
                                pos: [0.0, 0.0, 3.0],
                                target: [0.0, 0.0, -1.0],
                                mode: Camera3DMode::LookAt,
                                fov_deg: 50.0,
                                dof_focal_dist: 4.0,
                                dof_autofocus_target: true,
                                dof_blur_strength: blur_strength,
                                dof_max_radius_px: 28.0,
                                dof_samples: 24,
                                dof_aperture_blades: blades,
                                dof_blade_rotation_deg: 15.0,
                                dof_blade_roundness: 0.05,
                                dof_anamorphic_ratio: anamorphic,
                                dof_cats_eye_strength: cats_eye,
                                ..Default::default()
                            },
                            background_rgba: [0.0; 4],
                        },
                        inputs: vec![NodeId::from("bg_card"), NodeId::from("thin_fg")],
                        enabled: true,
                    },
                ],
            }
        };

    let count_red_pixels = |rgba8: &[u8]| -> usize {
        rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|px| px[0] > px[2] && px[0] > 60)
            .count()
    };

    let sharp = render_graph(&mut compositor, &make_dof_graph(0.0, 1.0, 0, 1.0, 0.0), w, h);
    let circ = render_graph(&mut compositor, &make_dof_graph(2.0, 1.0, 0, 1.0, 0.0), w, h);
    let hex = render_graph(&mut compositor, &make_dof_graph(2.0, 1.0, 6, 1.0, 0.0), w, h);
    let anam = render_graph(&mut compositor, &make_dof_graph(2.0, 1.0, 6, 0.45, 0.0), w, h);
    let cat = render_graph(&mut compositor, &make_dof_graph(2.0, 1.0, 6, 1.0, 0.9), w, h);

    let sharp_red = count_red_pixels(&sharp.rgba8);
    let dilated_red = count_red_pixels(&circ.rgba8);
    assert!(
        dilated_red > sharp_red + 80,
        "2-pass separable CoC dilation must bleed thin foreground stroke outward (sharp={sharp_red}, dilated={dilated_red})"
    );

    let mut hashes = HashSet::new();
    hashes.insert(circ.fnv1a_hash);
    hashes.insert(hex.fnv1a_hash);
    hashes.insert(anam.fnv1a_hash);
    hashes.insert(cat.fnv1a_hash);
    assert_eq!(
        hashes.len(),
        4,
        "Circular, 6-blade polygonal, anamorphic, and cat's-eye bokeh must each produce distinct outputs"
    );

    // Semi-transparent foreground (opacity = 0.45) over opaque blue background must preserve blue channel.
    let semi_fg = render_graph(&mut compositor, &make_dof_graph(2.0, 0.45, 0, 1.0, 0.0), w, h);
    let mut min_blue_on_red = u8::MAX;
    for px in semi_fg.rgba8.as_chunks::<4>().0 {
        if px[0] > 70 {
            min_blue_on_red = min_blue_on_red.min(px[2]);
        }
    }
    assert!(
        min_blue_on_red > 100 && min_blue_on_red < u8::MAX,
        "Porter-Duff foreground bokeh must let opaque blue background show through semi-transparent foreground (min_blue={min_blue_on_red})"
    );
}

#[test]
fn blend_perceptual_srgb_overlay_and_new_blend_modes() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let (w, h) = (16, 16);

    let make_blend = |base: [f32; 4], top: [f32; 4], mode: BlendMode| RenderGraphDef {
        output_node: NodeId::from("out"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("a"),
                kind: NodeKind::Solid {
                    color_rgba: base,
                    color_space: ColorSpace::LinearRec709,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("b"),
                kind: NodeKind::Solid {
                    color_rgba: top,
                    color_space: ColorSpace::LinearRec709,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("out"),
                kind: NodeKind::Blend { mode, opacity: 1.0 },
                inputs: vec![NodeId::from("a"), NodeId::from("b")],
                enabled: true,
            },
        ],
    };

    // Perceptual mid-gray in Linear Rec.709 (sRGB 0.5 == ~0.214041 linear).
    let lin_mid = [0.214041f32, 0.214041, 0.214041, 1.0];
    let overlay = render_graph(
        &mut compositor,
        &make_blend(lin_mid, lin_mid, BlendMode::Overlay),
        w,
        h,
    );
    let px_ov = center_pixel(&overlay.rgba8, w, h);
    // W3C perceptual Overlay(0.5 sRGB, 0.5 sRGB) == 0.5 sRGB (~128). Linear bug produced ~0.0916 -> ~86!
    assert!(
        (i16::from(px_ov[0]) - 128).abs() <= 2,
        "Perceptual sRGB Overlay(0.5, 0.5) must be ~128 sRGB, got {:?}",
        px_ov
    );

    let soft = render_graph(
        &mut compositor,
        &make_blend(lin_mid, lin_mid, BlendMode::SoftLight),
        w,
        h,
    );
    let px_soft = center_pixel(&soft.rgba8, w, h);
    assert!(
        (i16::from(px_soft[0]) - 128).abs() <= 2,
        "SoftLight(0.5, 0.5) must be ~128 sRGB, got {:?}",
        px_soft
    );

    let dodge = render_graph(
        &mut compositor,
        &make_blend(lin_mid, lin_mid, BlendMode::ColorDodge),
        w,
        h,
    );
    let px_dodge = center_pixel(&dodge.rgba8, w, h);
    // ColorDodge(0.5, 0.5) = 1.0 sRGB (255).
    assert!(
        px_dodge[0] >= 253,
        "ColorDodge(0.5, 0.5) must be ~255 sRGB, got {:?}",
        px_dodge
    );

    let hard = render_graph(
        &mut compositor,
        &make_blend(lin_mid, [0.65, 0.65, 0.65, 1.0], BlendMode::HardLight),
        w,
        h,
    );
    let px_hard = center_pixel(&hard.rgba8, w, h);
    assert!(px_hard[0] > 180, "HardLight bright top must brighten base, got {:?}", px_hard);

    let lighten = render_graph(
        &mut compositor,
        &make_blend([0.1, 0.8, 0.2, 1.0], [0.7, 0.2, 0.6, 1.0], BlendMode::Lighten),
        w,
        h,
    );
    let px_l = center_pixel(&lighten.rgba8, w, h);
    assert!(px_l[0] > 200 && px_l[1] > 220 && px_l[2] > 190, "Lighten failed: {px_l:?}");

    let darken = render_graph(
        &mut compositor,
        &make_blend([0.1, 0.8, 0.2, 1.0], [0.7, 0.2, 0.6, 1.0], BlendMode::Darken),
        w,
        h,
    );
    let px_d = center_pixel(&darken.rgba8, w, h);
    assert!(px_d[0] < 100 && px_d[1] < 130 && px_d[2] < 130, "Darken failed: {px_d:?}");

    // Pure additive glow (alpha = 0, rgb > 0) over opaque base must not be discarded by Overlay/Lighten.
    let glow_overlay = render_graph(
        &mut compositor,
        &make_blend([0.05, 0.05, 0.1, 1.0], [1.2, 0.2, 0.0, 0.0], BlendMode::Overlay),
        w,
        h,
    );
    let px_glow = center_pixel(&glow_overlay.rgba8, w, h);
    assert!(
        px_glow[0] >= 250,
        "Additive glow guard must preserve top RGB when top.a==0, got {:?}",
        px_glow
    );
}
