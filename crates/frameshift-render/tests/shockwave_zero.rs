use frameshift_render::{
    FnFrameProvider, GpuCompositor, NoopAnimator, RenderOptions, RenderedFrame, SourceFrame,
};
use frameshift_schema::{ColorSpace, GraphNodeDef, NodeId, NodeKind, RenderGraphDef, SourceFit};
use frameshift_time::{FLICKS_PER_SECOND, Flicks};

const SIZE: u32 = 96;

#[derive(Clone, Copy)]
struct Shock {
    radius: f32,
    ring_width: f32,
    refraction_amp: f32,
    zoom_blur_strength: f32,
    chromatic_aberration: f32,
}

fn checkerboard(width: u32, height: u32) -> SourceFrame {
    let mut data = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let idx = ((y * width + x) * 4) as usize;
            let on = ((x / 4) + (y / 4)) % 2 == 0;
            let v = if on { 255 } else { 0 };
            data[idx] = v;
            data[idx + 1] = v;
            data[idx + 2] = v;
            data[idx + 3] = 255;
        }
    }
    SourceFrame::rgba8_straight(width, height, data)
}

fn render(compositor: &mut GpuCompositor, shock: Shock) -> RenderedFrame {
    let graph = RenderGraphDef {
        output_node: NodeId::from("fx"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("src"),
                kind: NodeKind::SourceVideo {
                    asset_id: "plate".into(),
                    clip_id: None,
                    color_space: ColorSpace::LinearRec709,
                    fit: SourceFit::Stretch,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("fx"),
                kind: NodeKind::RadialShockwave {
                    center_uv: [0.5, 0.5],
                    radius: shock.radius,
                    ring_width: shock.ring_width,
                    refraction_amp: shock.refraction_amp,
                    zoom_blur_strength: shock.zoom_blur_strength,
                    chromatic_aberration: shock.chromatic_aberration,
                },
                inputs: vec![NodeId::from("src")],
                enabled: true,
            },
        ],
    };
    let provider = FnFrameProvider(|_: &str, _: Flicks, w: u32, h: u32| Ok(checkerboard(w, h)));
    compositor
        .render_frame(
            &graph,
            Flicks::ZERO,
            Flicks(FLICKS_PER_SECOND / 24),
            SIZE,
            SIZE,
            &provider,
            &NoopAnimator,
            &RenderOptions::default(),
        )
        .expect("shockwave render")
}

fn assert_frames_eq(a: &RenderedFrame, b: &RenderedFrame, label: &str) {
    assert_eq!(a.rgba8.len(), b.rgba8.len(), "{label}");
    if a.rgba8 != b.rgba8 {
        let idx = a
            .rgba8
            .iter()
            .zip(b.rgba8.iter())
            .position(|(p, q)| p != q)
            .unwrap();
        let pixel = idx / 4;
        let start = pixel * 4;
        let x = (pixel as u32) % a.width;
        let y = (pixel as u32) / a.width;
        panic!(
            "{label}: readback mismatch at ({x},{y}) {:?} vs {:?}",
            &a.rgba8[start..start + 4],
            &b.rgba8[start..start + 4],
        );
    }
}

fn assert_frames_ne(a: &RenderedFrame, b: &RenderedFrame, label: &str) {
    assert_ne!(a.rgba8, b.rgba8, "{label}");
}

fn assert_contrast(frame: &RenderedFrame, label: &str) {
    let mut min_l = 255u8;
    let mut max_l = 0u8;
    for px in frame.rgba8.as_chunks::<4>().0 {
        let l = px[0].max(px[1]).max(px[2]);
        min_l = min_l.min(l);
        max_l = max_l.max(l);
    }
    assert!(
        max_l > min_l + 40,
        "{label}: expected spatial contrast, luma {min_l}..{max_l}"
    );
}

#[test]
fn non_positive_radius_zeros_ring_contribution() {
    let mut compositor = GpuCompositor::new_headless().expect("headless GPU compositor");
    for radius in [0.0, -0.4] {
        let hot_ring = render(
            &mut compositor,
            Shock {
                radius,
                ring_width: 0.22,
                refraction_amp: 0.28,
                zoom_blur_strength: 0.0,
                chromatic_aberration: 2.0,
            },
        );
        let cold_ring = render(
            &mut compositor,
            Shock {
                radius,
                ring_width: 0.01,
                refraction_amp: 0.0,
                zoom_blur_strength: 0.0,
                chromatic_aberration: 2.0,
            },
        );
        assert_contrast(&hot_ring, "non-positive radius still shades the plate");
        assert_frames_eq(
            &hot_ring,
            &cold_ring,
            "radius <= 0 drops refraction and the chromatic ring, including the center disk",
        );
    }
}

#[test]
fn non_positive_radius_keeps_zoom_blur() {
    let mut compositor = GpuCompositor::new_headless().expect("headless GPU compositor");
    let zoom = Shock {
        radius: 0.0,
        ring_width: 0.07,
        refraction_amp: 0.0,
        zoom_blur_strength: 0.55,
        chromatic_aberration: 0.0,
    };
    let still = Shock {
        zoom_blur_strength: 0.0,
        ..zoom
    };
    let zoom_with_live_ring = Shock {
        radius: 0.34,
        ..zoom
    };

    let zoomed = render(&mut compositor, zoom);
    let unzoomed = render(&mut compositor, still);
    let zoomed_positive = render(&mut compositor, zoom_with_live_ring);

    assert_frames_ne(
        &zoomed,
        &unzoomed,
        "zoom_blur_strength must still move pixels when radius is 0",
    );
    assert_frames_eq(
        &zoomed,
        &zoomed_positive,
        "with refraction and chromatic aberration at 0, zoom does not depend on the radius gate",
    );
}

#[test]
fn positive_radius_still_draws_the_ring() {
    let mut compositor = GpuCompositor::new_headless().expect("headless GPU compositor");
    let live = Shock {
        radius: 0.32,
        ring_width: 0.12,
        refraction_amp: 0.25,
        zoom_blur_strength: 0.0,
        chromatic_aberration: 2.0,
    };
    let collapsed = Shock {
        radius: 0.0,
        ..live
    };
    let drawn = render(&mut compositor, live);
    let hidden = render(&mut compositor, collapsed);
    assert_frames_ne(
        &drawn,
        &hidden,
        "a positive radius must still refract and disperse; only radius <= 0 drops the ring",
    );
}
