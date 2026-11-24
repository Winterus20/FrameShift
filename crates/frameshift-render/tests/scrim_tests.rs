//! BackdropScrim render contract: blur energy inside the plate, bit-exact passthrough
//! outside it, determinism, premultiplied alpha, auto_contrast polarity, and
//! draft/final scale invariance.

use frameshift_render::{
    FnFrameProvider, GpuCompositor, NoopAnimator, OutputAlphaMode, RenderOptions, RenderedFrame,
    SourceFrame, TonemapMode,
};
use frameshift_schema::{ColorSpace, GraphNodeDef, NodeId, NodeKind, RenderGraphDef, SourceFit};
use frameshift_time::{FLICKS_PER_SECOND, Flicks};

fn split_frame(width: u32, height: u32) -> SourceFrame {
    let mut data = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let idx = ((y * width + x) * 4) as usize;
            let value = if x < width / 2 { 0u8 } else { 255u8 };
            data[idx] = value;
            data[idx + 1] = value;
            data[idx + 2] = value;
            data[idx + 3] = 255;
        }
    }
    SourceFrame::rgba8_straight(width, height, data)
}

fn source_node() -> GraphNodeDef {
    GraphNodeDef {
        id: NodeId::from("src"),
        kind: NodeKind::SourceVideo {
            asset_id: "split".into(),
            clip_id: None,
            color_space: ColorSpace::LinearRec709,
            fit: SourceFit::Stretch,
        },
        inputs: vec![],
        enabled: true,
    }
}

fn solid_node(rgba: [f32; 4]) -> GraphNodeDef {
    GraphNodeDef {
        id: NodeId::from("src"),
        kind: NodeKind::Solid {
            color_rgba: rgba,
            color_space: ColorSpace::LinearRec709,
        },
        inputs: vec![],
        enabled: true,
    }
}

fn scrim_kind(
    rect_center_norm: [f32; 2],
    rect_size_norm: [f32; 2],
    blur_radius_norm: f32,
    fill_rgba: [f32; 4],
    auto_contrast: f32,
) -> NodeKind {
    NodeKind::BackdropScrim {
        rect_center_norm,
        rect_size_norm,
        corner_radius_norm: 0.01,
        blur_radius_norm,
        fill_rgba,
        opacity: 1.0,
        auto_contrast,
    }
}

fn scrim_graph(source: GraphNodeDef, kind: NodeKind) -> RenderGraphDef {
    RenderGraphDef {
        output_node: NodeId::from("scrim"),
        nodes: vec![
            source,
            GraphNodeDef {
                id: NodeId::from("scrim"),
                kind,
                inputs: vec![NodeId::from("src")],
                enabled: true,
            },
        ],
    }
}

fn source_only_graph() -> RenderGraphDef {
    RenderGraphDef {
        output_node: NodeId::from("src"),
        nodes: vec![source_node()],
    }
}

fn render(compositor: &mut GpuCompositor, graph: &RenderGraphDef, width: u32, height: u32) -> RenderedFrame {
    let provider =
        FnFrameProvider(|_: &str, _: Flicks, w: u32, h: u32| Ok(split_frame(w, h)));
    let options = RenderOptions {
        tonemap_mode: Some(TonemapMode::None),
        output_alpha_mode: OutputAlphaMode::Premultiplied,
        exposure: 1.0,
        ..Default::default()
    };
    compositor
        .render_frame(
            graph,
            Flicks::ZERO,
            Flicks(FLICKS_PER_SECOND / 24),
            width,
            height,
            &provider,
            &NoopAnimator,
            &options,
        )
        .expect("scrim render")
}

fn pixel(frame: &RenderedFrame, x: u32, y: u32) -> [u8; 4] {
    let idx = ((y * frame.width + x) * 4) as usize;
    [
        frame.rgba8[idx],
        frame.rgba8[idx + 1],
        frame.rgba8[idx + 2],
        frame.rgba8[idx + 3],
    ]
}

fn is_outside_rect(rect_center: [f32; 2], rect_size: [f32; 2], width: u32, height: u32, x: u32, y: u32, margin_px: f32) -> bool {
    let half_w = rect_size[0] * 0.5;
    let half_h = rect_size[1] * 0.5;
    let nx = (x as f32 + 0.5) / width as f32;
    let ny = (y as f32 + 0.5) / height as f32;
    let mx = margin_px / width as f32;
    let my = margin_px / height as f32;
    (nx - rect_center[0]).abs() > half_w + mx || (ny - rect_center[1]).abs() > half_h + my
}

#[test]
fn blur_keeps_pixels_outside_the_rect_and_softens_inside() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let width = 128;
    let height = 72;
    let rect_center = [0.5, 0.5];
    let rect_size = [0.5, 0.4];
    let transparent_fill = [0.0, 0.0, 0.0, 0.0];

    let plain = render(&mut compositor, &source_only_graph(), width, height);
    let no_blur = render(
        &mut compositor,
        &scrim_graph(
            source_node(),
            scrim_kind(rect_center, rect_size, 0.0, transparent_fill, 0.0),
        ),
        width,
        height,
    );
    let with_blur = render(
        &mut compositor,
        &scrim_graph(
            source_node(),
            scrim_kind(rect_center, rect_size, 0.08, transparent_fill, 0.0),
        ),
        width,
        height,
    );

    // A zero-radius, transparent-fill scrim is a bit-exact passthrough of its input.
    assert_eq!(
        no_blur.rgba8, plain.rgba8,
        "blur_radius=0 with a transparent fill must not change any pixel"
    );

    // The blur must never leak outside the plate rect (2 px margin for analytic AA).
    let mut checked_outside = 0usize;
    for y in 0..height {
        for x in 0..width {
            if is_outside_rect(rect_center, rect_size, width, height, x, y, 2.0) {
                checked_outside += 1;
                assert_eq!(
                    pixel(&with_blur, x, y),
                    pixel(&no_blur, x, y),
                    "pixel ({x}, {y}) outside the rect must stay bit-identical"
                );
            }
        }
    }
    assert!(
        checked_outside > (width * height) as usize / 3,
        "test must actually inspect a meaningful outside region"
    );

    // The hard vertical edge inside the rect is softened to a mid tone.
    let split_x = width / 2;
    let center_y = height / 2;
    assert_eq!(pixel(&no_blur, split_x, center_y)[0], 255);
    let blurred = pixel(&with_blur, split_x, center_y)[0];
    assert!(
        (40..=215).contains(&blurred),
        "edge pixel must be a mix of both halves after blur, got {blurred}"
    );
}

#[test]
fn blur_is_deterministic_across_repeated_renders() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let graph = scrim_graph(
        source_node(),
        scrim_kind([0.5, 0.5], [0.6, 0.5], 0.06, [0.0, 0.0, 0.0, 0.4], 0.5),
    );

    let first = render(&mut compositor, &graph, 96, 54);
    let second = render(&mut compositor, &graph, 96, 54);

    assert_eq!(first.fnv1a_hash, second.fnv1a_hash);
    assert_eq!(first.rgba8, second.rgba8);
}

#[test]
fn plate_is_premultiplied_without_a_dark_halo() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let width = 128;
    let height = 72;
    let fill = [0.2, 0.3, 0.9, 0.5];
    let graph = scrim_graph(
        solid_node([0.0, 0.0, 0.0, 0.0]),
        scrim_kind([0.5, 0.5], [0.5, 0.5], 0.0, fill, 0.0),
    );
    let frame = render(&mut compositor, &graph, width, height);

    // Expected straight-alpha fill encoded to sRGB, the value premultiplied output divides out.
    let expected_srgb = [0.485, 0.584, 0.955];

    let center = pixel(&frame, width / 2, height / 2);
    assert!(
        (120..=136).contains(&center[3]),
        "plate alpha must be ~128, got {}",
        center[3]
    );

    let mut partial_edge_pixels = 0usize;
    for y in 0..height {
        for x in 0..width {
            let px = pixel(&frame, x, y);
            for channel in 0..3 {
                assert!(
                    px[channel] as i32 <= px[3] as i32 + 2,
                    "premultiplied invariant broken at ({x}, {y}): {px:?}"
                );
            }
            if (16..=100).contains(&px[3]) {
                partial_edge_pixels += 1;
                for channel in 0..3 {
                    let ratio = px[channel] as f32 / px[3] as f32;
                    assert!(
                        (ratio - expected_srgb[channel]).abs() < 0.06,
                        "coverage-edged pixel must keep the fill hue (no dark halo) at ({x}, {y}): {px:?}"
                    );
                }
            }
        }
    }
    assert!(
        partial_edge_pixels > 0,
        "expected antialiased plate-edge pixels with partial coverage"
    );
}

#[test]
fn auto_contrast_strengthens_the_plate_on_a_bright_backdrop() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let width = 64;
    let height = 36;
    let rect_center = [0.5, 0.5];
    let rect_size = [0.6, 0.6];
    let fill = [0.0, 0.0, 0.0, 0.35];

    let dark = [0.02, 0.02, 0.02, 1.0];
    let bright = [0.9, 0.9, 0.9, 1.0];

    let dark_off = render(
        &mut compositor,
        &scrim_graph(solid_node(dark), scrim_kind(rect_center, rect_size, 0.0, fill, 0.0)),
        width,
        height,
    );
    let dark_on = render(
        &mut compositor,
        &scrim_graph(solid_node(dark), scrim_kind(rect_center, rect_size, 0.0, fill, 1.0)),
        width,
        height,
    );
    let bright_off = render(
        &mut compositor,
        &scrim_graph(solid_node(bright), scrim_kind(rect_center, rect_size, 0.0, fill, 0.0)),
        width,
        height,
    );
    let bright_on = render(
        &mut compositor,
        &scrim_graph(solid_node(bright), scrim_kind(rect_center, rect_size, 0.0, fill, 1.0)),
        width,
        height,
    );

    let center = (width / 2, height / 2);
    let dark_off_luma = pixel(&dark_off, center.0, center.1)[0] as i32;
    let dark_on_luma = pixel(&dark_on, center.0, center.1)[0] as i32;
    let bright_off_luma = pixel(&bright_off, center.0, center.1)[0] as i32;
    let bright_on_luma = pixel(&bright_on, center.0, center.1)[0] as i32;

    // Dark backdrop: auto_contrast has nothing to strengthen.
    assert!(
        (dark_off_luma - dark_on_luma).abs() <= 2,
        "dark backdrop plate must not change: off={dark_off_luma} on={dark_on_luma}"
    );
    // Bright backdrop: the plate gets materially stronger.
    assert!(
        bright_on_luma + 30 < bright_off_luma,
        "bright backdrop plate must get stronger: off={bright_off_luma} on={bright_on_luma}"
    );
}

#[test]
fn scrim_over_a_transparent_scene3d_backdrop_is_fill_only() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let width = 64;
    let height = 36;
    // Scene3D clears to transparent by default, so both the backdrop blur and
    // auto_contrast have no energy to work with and the plate is fill-only.
    let graph = RenderGraphDef {
        output_node: NodeId::from("scrim"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("scene"),
                kind: NodeKind::Scene3D {
                    camera: Default::default(),
                    background_rgba: [0.0, 0.0, 0.0, 0.0],
        depth_plate: None,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("scrim"),
                kind: scrim_kind([0.5, 0.5], [0.5, 0.5], 0.05, [0.0, 0.0, 0.0, 0.5], 1.0),
                inputs: vec![NodeId::from("scene")],
                enabled: true,
            },
        ],
    };

    let frame = render(&mut compositor, &graph, width, height);
    let center = pixel(&frame, width / 2, height / 2);
    assert!(
        (124..=132).contains(&center[3]),
        "transparent backdrop plate alpha must stay at the fill alpha, got {}",
        center[3]
    );
    assert!(
        center[0] <= 2 && center[1] <= 2 && center[2] <= 2,
        "transparent backdrop plate must stay the fill color, got {center:?}"
    );
}

#[test]
fn plate_geometry_is_scale_invariant_between_draft_and_final() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let rect_center = [0.5, 0.5];
    let rect_size = [0.5, 0.5];
    let fill = [0.0, 0.0, 0.0, 0.5];

    let mut edges = Vec::new();
    for (width, height) in [(96u32, 54u32), (192u32, 108u32)] {
        let graph = scrim_graph(
            solid_node([0.0, 0.0, 0.0, 0.0]),
            scrim_kind(rect_center, rect_size, 0.05, fill, 0.0),
        );
        let frame = render(&mut compositor, &graph, width, height);
        let row = height / 2;
        let left = (0..width)
            .find(|&x| pixel(&frame, x, row)[3] >= 64)
            .expect("left plate edge must be visible");
        let right = (0..width)
            .rev()
            .find(|&x| pixel(&frame, x, row)[3] >= 64)
            .expect("right plate edge must be visible");
        edges.push((left as f32 / width as f32, right as f32 / width as f32));
    }

    let (left_draft, right_draft) = edges[0];
    let (left_final, right_final) = edges[1];
    assert!(
        (left_draft - left_final).abs() <= 2.0 / 96.0,
        "left plate edge moved between scales: {left_draft} vs {left_final}"
    );
    assert!(
        (right_draft - right_final).abs() <= 2.0 / 96.0,
        "right plate edge moved between scales: {right_draft} vs {right_final}"
    );
    assert!((left_draft - 0.25).abs() <= 2.0 / 96.0);
    assert!((right_draft - 0.75).abs() <= 2.0 / 96.0);
}
