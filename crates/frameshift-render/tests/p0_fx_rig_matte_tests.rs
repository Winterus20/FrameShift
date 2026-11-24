use std::collections::HashSet;
use std::path::Path;

use frameshift_render::{
    BloomPassUniform, BlurMoCurvesUniform, FnFrameProvider, GpuCompositor, MotionTileShakeUniform,
    NoopAnimator, OutputAlphaMode, ProjectAnimator, RenderError, RenderOptions, SourceFrame,
    TonemapMode, WORKING_TEXTURE_FORMAT,
};
use frameshift_schema::{
    BlendMode, Camera3DDef, Camera3DMode, ColorSpace, GraphNodeDef, MatteMode, NodeId, NodeKind,
    ParamBindingDef, Project, RenderGraphDef, RenderProfileDef, RenderProfileKind,
    RenderProfilesDef, Resolution, ShutterPhase, SourceFit, TextAlign2D, TextAnimatorDef,
    TextRangeShape, TextSelectorUnit, TimelineDef, WorkingPixelFormat, WritingMode2D,
    CURRENT_SCHEMA_VERSION,
};
use frameshift_time::{FLICKS_PER_SECOND, FPS_30, FPS_60, Flicks};
use wgpu::util::DeviceExt;

fn test_opts() -> RenderOptions {
    RenderOptions {
        tonemap_mode: Some(TonemapMode::None),
        output_alpha_mode: OutputAlphaMode::Straight,
        ..Default::default()
    }
}

fn make_project(
    name: &str,
    width: u32,
    height: u32,
    graph: RenderGraphDef,
    param_bindings: Vec<ParamBindingDef>,
) -> Project {
    let profile = |kind: RenderProfileKind| RenderProfileDef {
        kind,
        resolution: Resolution { width, height },
        motion_blur_sub_samples: 1,
        shutter_angle_deg: 180.0,
        pixel_format: WorkingPixelFormat::Rgba16FloatPremul,
    };
    Project {
        schema_version: CURRENT_SCHEMA_VERSION,
        revision_id: 1,
        name: name.into(),
        assets: vec![],
        timeline: TimelineDef {
            fps: FPS_60,
            duration_ticks: Flicks(2 * FLICKS_PER_SECOND),
            resolution: Resolution { width, height },
            clips: vec![],
            audio_clips: vec![],
            ducking_rules: vec![],
            micro_silences: vec![],
        },
        events: vec![],
        graph,
        render_profiles: RenderProfilesDef {
            draft: profile(RenderProfileKind::Draft),
            final_profile: profile(RenderProfileKind::Final),
        },
        param_bindings,
        creative_decisions: vec![],
        preferences: vec![],
        proxy_config: Default::default(),
        hw_accel_policy: Default::default(),
    }
}

fn no_override_provider() -> impl frameshift_render::FrameProvider {
    FnFrameProvider(|asset_id: &str, tick: Flicks, _w: u32, _h: u32| {
        Err(RenderError::FrameProvider {
            asset_id: asset_id.to_owned(),
            source_tick: tick.0,
            message: "no external frame".into(),
        })
    })
}

fn asymmetric_plate(w: u32, h: u32) -> SourceFrame {
    let mut data = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let idx = ((y * w + x) * 4) as usize;
            let checker = ((x / 8) + (y / 8)) % 2 == 0;
            // Left quarter bright red/yellow bar, right edge cyan stripe, dark center.
            if x < w / 4 {
                data[idx] = 240;
                data[idx + 1] = if checker { 210 } else { 40 };
                data[idx + 2] = 20;
            } else if x >= (3 * w) / 4 {
                data[idx] = 20;
                data[idx + 1] = if checker { 230 } else { 80 };
                data[idx + 2] = 250;
            } else {
                data[idx] = if checker { 120 } else { 15 };
                data[idx + 1] = 20;
                data[idx + 2] = 30;
            }
            data[idx + 3] = 255;
        }
    }
    SourceFrame::rgba8_straight(w, h, data)
}

fn center_dot_plate(w: u32, h: u32) -> SourceFrame {
    let mut data = vec![0u8; (w * h * 4) as usize];
    let cx = w as f32 * 0.35;
    let cy = h as f32 * 0.45;
    for y in 0..h {
        for x in 0..w {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            if dx * dx + dy * dy <= 36.0 {
                let idx = ((y * w + x) * 4) as usize;
                data[idx] = 255;
                data[idx + 1] = 200;
                data[idx + 2] = 60;
                data[idx + 3] = 255;
            }
        }
    }
    SourceFrame::rgba8_straight(w, h, data)
}

fn count_visible_alpha(rgba8: &[u8], threshold: u8) -> usize {
    rgba8
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|px| px[3] > threshold)
        .count()
}

fn mean_luma(rgba8: &[u8]) -> f64 {
    let pixels = rgba8.as_chunks::<4>().0;
    let total: f64 = pixels
        .iter()
        .map(|px| 0.2126 * px[0] as f64 + 0.7152 * px[1] as f64 + 0.0722 * px[2] as f64)
        .sum();
    total / pixels.len().max(1) as f64
}

fn run_single_pass_rgba16f(
    compositor: &GpuCompositor,
    program: &frameshift_render::ShaderProgram,
    width: u32,
    height: u32,
    input_a: &[[f32; 4]],
    input_b: Option<&[[f32; 4]]>,
    uniform_bytes: &[u8],
) -> Vec<[f32; 4]> {
    assert_eq!(width % 32, 0, "width must keep row stride 256-byte aligned");
    let ctx = compositor.context();
    let upload = |label: &str, pixels: &[[f32; 4]]| {
        let tex = ctx.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: WORKING_TEXTURE_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let f16_bytes: Vec<u8> = pixels
            .iter()
            .flat_map(|px| px.iter().flat_map(|&c| half::f16::from_f32(c).to_le_bytes()))
            .collect();
        ctx.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &f16_bytes,
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
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        (tex, view)
    };

    let (_tex_a, view_a) = upload("in_a", input_a);
    let tex_b_holder = input_b.map(|b| upload("in_b", b));

    let out_tex = ctx.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("out"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: WORKING_TEXTURE_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let out_view = out_tex.create_view(&wgpu::TextureViewDescriptor::default());

    let ubuf = ctx
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("uniform"),
            contents: uniform_bytes,
            usage: wgpu::BufferUsages::UNIFORM,
        });

    let bg = if let Some((_, ref view_b)) = tex_b_holder {
        ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("two_tex_bg"),
            layout: &program.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view_a),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(view_b),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&ctx.linear_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: ubuf.as_entire_binding(),
                },
            ],
        })
    } else {
        ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("one_tex_bg"),
            layout: &program.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view_a),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&ctx.linear_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: ubuf.as_entire_binding(),
                },
            ],
        })
    };

    let bytes_per_row = width * 8;
    let read_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("read_buf"),
        size: u64::from(bytes_per_row * height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("pass"),
        });
    {
        let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("shader_test_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &out_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
            multiview_mask: None,
        });
        rpass.set_pipeline(&program.pipeline);
        rpass.set_bind_group(0, &bg, &[]);
        rpass.draw(0..3, 0..1);
    }
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &out_tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &read_buf,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    ctx.queue.submit(Some(encoder.finish()));

    let slice = read_buf.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |res| {
        let _ = tx.send(res);
    });
    let _ = ctx.device.poll(wgpu::PollType::wait_indefinitely());
    rx.recv().unwrap().unwrap();
    let mapped = slice.get_mapped_range().unwrap();
    let out: Vec<[f32; 4]> = mapped
        .as_chunks::<8>()
        .0
        .iter()
        .map(|px| {
            [
                half::f16::from_le_bytes([px[0], px[1]]).to_f32(),
                half::f16::from_le_bytes([px[2], px[3]]).to_f32(),
                half::f16::from_le_bytes([px[4], px[5]]).to_f32(),
                half::f16::from_le_bytes([px[6], px[7]]).to_f32(),
            ]
        })
        .collect();
    drop(mapped);
    read_buf.unmap();
    out
}

#[test]
fn blur_mocurves_directional_radial_rotational_wrap_modes_and_animation() {
    assert_eq!(std::mem::size_of::<BlurMoCurvesUniform>(), 32);

    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let _ = &compositor.pipelines().fx_blur_mocurves;
    let frame_ticks = FPS_60.frame_duration().unwrap();
    let (w, h) = (96, 64);

    let provider = FnFrameProvider(|asset_id: &str, _: Flicks, pw: u32, ph: u32| {
        if asset_id == "dot" {
            Ok(center_dot_plate(pw, ph))
        } else {
            Ok(asymmetric_plate(pw, ph))
        }
    });

    let make_blur = |asset: &str,
                     shift_x: f32,
                     shift_y: f32,
                     z_dist: f32,
                     rotate_deg: f32,
                     blur_strength: f32,
                     samples: u32,
                     wrap_mode: u32| RenderGraphDef {
        output_node: NodeId::from("bmc"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("src"),
                kind: NodeKind::SourceVideo {
                    asset_id: asset.into(),
                    clip_id: None,
                    color_space: ColorSpace::LinearRec709,
                    fit: SourceFit::Stretch,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("bmc"),
                kind: NodeKind::BlurMoCurves {
                    shift_x,
                    shift_y,
                    z_dist,
                    rotate_deg,
                    blur_strength,
                    samples,
                    wrap_mode,
                },
                inputs: vec![NodeId::from("src")],
                enabled: true,
            },
        ],
    };

    let sharp = compositor
        .render_frame(
            &make_blur("dot", 0.0, 0.0, 0.0, 0.0, 0.0, 16, 0),
            Flicks::ZERO,
            frame_ticks,
            w,
            h,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("zero-blur dot");
    let dir = compositor
        .render_frame(
            &make_blur("dot", 0.25, 0.0, 0.0, 0.0, 1.0, 16, 0),
            Flicks::ZERO,
            frame_ticks,
            w,
            h,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("directional blur");
    let radial = compositor
        .render_frame(
            &make_blur("dot", 0.0, 0.0, 0.35, 0.0, 1.0, 16, 0),
            Flicks::ZERO,
            frame_ticks,
            w,
            h,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("radial z_dist blur");
    let rot = compositor
        .render_frame(
            &make_blur("dot", 0.0, 0.0, 0.0, 32.0, 1.0, 16, 0),
            Flicks::ZERO,
            frame_ticks,
            w,
            h,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("rotational blur");

    let sharp_px = count_visible_alpha(&sharp.rgba8, 8);
    assert!(count_visible_alpha(&dir.rgba8, 8) > sharp_px);
    assert!(count_visible_alpha(&radial.rgba8, 8) > sharp_px);
    assert!(count_visible_alpha(&rot.rgba8, 8) > sharp_px);

    let hashes: HashSet<u64> = [
        sharp.fnv1a_hash,
        dir.fnv1a_hash,
        radial.fnv1a_hash,
        rot.fnv1a_hash,
    ]
    .into_iter()
    .collect();
    assert_eq!(hashes.len(), 4, "all BlurMoCurves motion modes must produce distinct outputs");

    // Large off-screen shift exercises wrap_mode 0=Clamp, 1=Tile, 2=Reflect differently.
    let mut wrap_hashes = HashSet::new();
    for mode in 0..=2u32 {
        let f = compositor
            .render_frame(
                &make_blur("asym", 0.9, 0.0, 0.0, 0.0, 1.8, 16, mode),
                Flicks::ZERO,
                frame_ticks,
                w,
                h,
                &provider,
                &NoopAnimator,
                &test_opts(),
            )
            .expect("wrap mode render");
        wrap_hashes.insert(f.fnv1a_hash);
    }
    assert_eq!(wrap_hashes.len(), 3, "Clamp, Tile, and Reflect wrap modes must differ");

    // Animated BlurMoCurves via ProjectAnimator
    let project = make_project(
        "blur_mocurves_anim",
        w,
        h,
        make_blur("dot", 0.0, 0.0, 0.0, 0.0, 1.0, 16, 2),
        vec![
            ParamBindingDef {
                node_id: NodeId::from("bmc"),
                param_name: "shift_x".into(),
                expression: "0.3 * t_sec".into(),
            },
            ParamBindingDef {
                node_id: NodeId::from("bmc"),
                param_name: "blur_strength".into(),
                expression: "0.5 + t_sec".into(),
            },
        ],
    );
    let animator = ProjectAnimator::new(&project).expect("animator");
    let anim_t0 = compositor
        .render_frame(&project.graph, Flicks::ZERO, frame_ticks, w, h, &provider, &animator, &test_opts())
        .expect("t0");
    let anim_t1 = compositor
        .render_frame(
            &project.graph,
            Flicks(FLICKS_PER_SECOND / 2),
            frame_ticks,
            w,
            h,
            &provider,
            &animator,
            &test_opts(),
        )
        .expect("t0.5");
    assert_ne!(anim_t0.fnv1a_hash, anim_t1.fnv1a_hash);
}

#[test]
fn adjustment_layer_respects_tick_window_blend_mode_and_animated_opacity() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let frame_ticks = FPS_60.frame_duration().unwrap();
    let (w, h) = (32, 32);
    let provider = FnFrameProvider(|_: &str, _: Flicks, pw: u32, ph: u32| {
        Ok(SourceFrame::rgba_f32_premul(
            pw,
            ph,
            vec![[0.35, 0.20, 0.10, 1.0]; (pw * ph) as usize],
        ))
    });

    let start_tick = Flicks(FLICKS_PER_SECOND / 2);
    let end_tick = Flicks(FLICKS_PER_SECOND);
    let graph = RenderGraphDef {
        output_node: NodeId::from("adj"),
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
                id: NodeId::from("adj"),
                kind: NodeKind::AdjustmentLayer {
                    start_tick,
                    end_tick,
                    opacity: 1.0,
                    blend_mode: BlendMode::Screen,
                },
                inputs: vec![NodeId::from("src")],
                enabled: true,
            },
        ],
    };

    let before = compositor
        .render_frame(
            &graph,
            Flicks(start_tick.0 - 1),
            frame_ticks,
            w,
            h,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("before start");
    let at_start = compositor
        .render_frame(&graph, start_tick, frame_ticks, w, h, &provider, &NoopAnimator, &test_opts())
        .expect("at start");
    let inside = compositor
        .render_frame(
            &graph,
            Flicks((start_tick.0 + end_tick.0) / 2),
            frame_ticks,
            w,
            h,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("inside");
    let at_end = compositor
        .render_frame(&graph, end_tick, frame_ticks, w, h, &provider, &NoopAnimator, &test_opts())
        .expect("at end");

    assert_eq!(before.fnv1a_hash, at_end.fnv1a_hash, "outside [start, end) must passthrough");
    assert_eq!(at_start.fnv1a_hash, inside.fnv1a_hash);
    assert!(
        mean_luma(&inside.rgba8) > mean_luma(&before.rgba8) + 20.0,
        "Screen AdjustmentLayer inside window must brighten over passthrough"
    );

    let project = make_project(
        "adj_anim",
        w,
        h,
        graph,
        vec![ParamBindingDef {
            node_id: NodeId::from("adj"),
            param_name: "opacity".into(),
            expression: "0.4".into(),
        }],
    );
    let animator = ProjectAnimator::new(&project).expect("animator");
    let half_op = compositor
        .render_frame(
            &project.graph,
            Flicks((start_tick.0 + end_tick.0) / 2),
            frame_ticks,
            w,
            h,
            &provider,
            &animator,
            &test_opts(),
        )
        .expect("animated opacity");
    assert!(mean_luma(&half_op.rgba8) > mean_luma(&before.rgba8) + 5.0);
    assert!(mean_luma(&half_op.rgba8) < mean_luma(&inside.rgba8) - 5.0);
}

#[test]
fn null3d_parent_chain_pivot_and_mirror_edges_in_scene3d() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let frame_ticks = FPS_60.frame_duration().unwrap();
    let (w, h) = (96, 64);

    let provider = FnFrameProvider(|_: &str, _: Flicks, pw: u32, ph: u32| Ok(asymmetric_plate(pw, ph)));

    let make_scene = |null_pos: [f32; 3],
                      null_rot_z: f32,
                      child_rot_z: f32,
                      child_scale: [f32; 3],
                      child_pivot: [f32; 3],
                      mirror_edges: bool| RenderGraphDef {
        output_node: NodeId::from("scene"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("plate"),
                kind: NodeKind::SourceVideo {
                    asset_id: "asym".into(),
                    clip_id: None,
                    color_space: ColorSpace::LinearRec709,
                    fit: SourceFit::Stretch,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("null_rig"),
                kind: NodeKind::Null3D {
                    pos: null_pos,
                    rot_deg: [0.0, 0.0, null_rot_z],
                    scale: [1.0, 1.0, 1.0],
                    pivot: [0.0, 0.0, 0.0],
                    parent: None,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("card"),
                kind: NodeKind::Transform3D {
                    pos: [0.0, 0.0, 0.0],
                    rot_deg: [0.0, 0.0, child_rot_z],
                    scale: child_scale,
                    fov_deg: 50.0,
                    camera_pos: [0.0, 0.0, 3.0],
                    camera_rot_deg: [0.0, 0.0, 0.0],
                    dof_focal_dist: 3.0,
                    dof_blur_strength: 0.0,
                    mirror_edges,
                    parent: Some(NodeId::from("null_rig")),
                    pivot: child_pivot,
                },
                inputs: vec![NodeId::from("plate")],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("scene"),
                kind: NodeKind::Scene3D {
                    camera: Camera3DDef {
                        pos: [0.0, 0.0, 3.0],
                        target: [0.0, 0.0, 0.0],
                        mode: Camera3DMode::LookAt,
                        fov_deg: 50.0,
                        ..Default::default()
                    },
                    background_rgba: [0.0, 0.0, 0.0, 0.0],
                },
                inputs: vec![NodeId::from("null_rig"), NodeId::from("card")],
                enabled: true,
            },
        ],
    };

    let base = compositor
        .render_frame(
            &make_scene([0.0, 0.0, 0.0], 0.0, 0.0, [0.35, 0.35, 1.0], [0.0; 3], false),
            Flicks::ZERO,
            frame_ticks,
            w,
            h,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("base rig");
    let shifted_null = compositor
        .render_frame(
            &make_scene([0.5, 0.25, 0.0], 0.0, 0.0, [0.35, 0.35, 1.0], [0.0; 3], false),
            Flicks::ZERO,
            frame_ticks,
            w,
            h,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("shifted null");
    assert_ne!(base.fnv1a_hash, shifted_null.fnv1a_hash, "Null3D pos must move child Transform3D");

    // Offset pivot rotates around pivot rather than center
    let pivot_zero = compositor
        .render_frame(
            &make_scene([0.0; 3], 0.0, 90.0, [0.4, 0.4, 1.0], [0.0, 0.0, 0.0], false),
            Flicks::ZERO,
            frame_ticks,
            w,
            h,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("pivot zero");
    let pivot_offset = compositor
        .render_frame(
            &make_scene([0.0; 3], 0.0, 90.0, [0.4, 0.4, 1.0], [0.6, 0.0, 0.0], false),
            Flicks::ZERO,
            frame_ticks,
            w,
            h,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("pivot offset");
    assert_ne!(pivot_zero.fnv1a_hash, pivot_offset.fnv1a_hash);

    // mirror_edges=false leaves corners transparent; mirror_edges=true fills canvas via UV mirror
    let mirrored = compositor
        .render_frame(
            &make_scene([0.0; 3], 0.0, 0.0, [0.35, 0.35, 1.0], [0.0; 3], true),
            Flicks::ZERO,
            frame_ticks,
            w,
            h,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("mirrored");
    assert_eq!(base.rgba8[3], 0, "unmirrored small card corner must be transparent");
    assert!(mirrored.rgba8[3] > 200, "mirror_edges=true must fill corner via UV folding");
    assert!(count_visible_alpha(&mirrored.rgba8, 128) > count_visible_alpha(&base.rgba8, 128));

    // Animate Null3D parent via ProjectAnimator
    let project = make_project(
        "null3d_anim",
        w,
        h,
        make_scene([0.0; 3], 0.0, 0.0, [0.35, 0.35, 1.0], [0.0; 3], false),
        vec![ParamBindingDef {
            node_id: NodeId::from("null_rig"),
            param_name: "rot_z".into(),
            expression: "90.0 * t_sec".into(),
        }],
    );
    let animator = ProjectAnimator::new(&project).expect("animator");
    let t0 = compositor
        .render_frame(&project.graph, Flicks::ZERO, frame_ticks, w, h, &provider, &animator, &test_opts())
        .expect("t0");
    let t1 = compositor
        .render_frame(
            &project.graph,
            Flicks(FLICKS_PER_SECOND / 2),
            frame_ticks,
            w,
            h,
            &provider,
            &animator,
            &test_opts(),
        )
        .expect("t0.5");
    assert_ne!(t0.fnv1a_hash, t1.fnv1a_hash);
}

#[test]
fn depth_matte_slices_near_far_feather_invert_and_animates() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let frame_ticks = FPS_60.frame_duration().unwrap();
    let (w, h) = (64, 16);

    let provider = FnFrameProvider(|asset_id: &str, _: Flicks, pw: u32, ph: u32| {
        if asset_id == "depth" {
            let mut pixels = Vec::with_capacity((pw * ph) as usize);
            for _y in 0..ph {
                for x in 0..pw {
                    let d = (x as f32 + 0.5) / pw as f32;
                    pixels.push([d, d, d, 1.0]);
                }
            }
            Ok(SourceFrame::rgba_f32_premul(pw, ph, pixels))
        } else {
            Ok(SourceFrame::rgba_f32_premul(
                pw,
                ph,
                vec![[0.9, 0.7, 0.2, 1.0]; (pw * ph) as usize],
            ))
        }
    });

    let make_dm = |near_depth: f32, far_depth: f32, feather: f32, invert: bool| RenderGraphDef {
        output_node: NodeId::from("dm"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("base"),
                kind: NodeKind::SourceVideo {
                    asset_id: "base".into(),
                    clip_id: None,
                    color_space: ColorSpace::LinearRec709,
                    fit: SourceFit::Stretch,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("depth"),
                kind: NodeKind::SourceVideo {
                    asset_id: "depth".into(),
                    clip_id: None,
                    color_space: ColorSpace::LinearRec709,
                    fit: SourceFit::Stretch,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("dm"),
                kind: NodeKind::DepthMatte {
                    near_depth,
                    far_depth,
                    feather,
                    invert,
                },
                inputs: vec![NodeId::from("base"), NodeId::from("depth")],
                enabled: true,
            },
        ],
    };

    let band = compositor
        .render_frame(
            &make_dm(0.35, 0.65, 0.05, false),
            Flicks::ZERO,
            frame_ticks,
            w,
            h,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("depth matte band");
    let px_at = |frame: &frameshift_render::RenderedFrame, x: u32| -> u8 {
        frame.rgba8[((x + w * 4) * 4 + 3) as usize]
    };
    assert_eq!(px_at(&band, 2), 0, "left outside near_depth must be transparent");
    assert_eq!(px_at(&band, w / 2), 255, "center inside depth range must stay opaque");
    assert_eq!(px_at(&band, w - 3), 0, "right outside far_depth must be transparent");

    let inverted = compositor
        .render_frame(
            &make_dm(0.35, 0.65, 0.05, true),
            Flicks::ZERO,
            frame_ticks,
            w,
            h,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("depth matte invert");
    assert_eq!(px_at(&inverted, 2), 255);
    assert_eq!(px_at(&inverted, w / 2), 0);
    assert_eq!(px_at(&inverted, w - 3), 255);

    let project = make_project(
        "depth_anim",
        w,
        h,
        make_dm(0.2, 0.4, 0.05, false),
        vec![
            ParamBindingDef {
                node_id: NodeId::from("dm"),
                param_name: "near_depth".into(),
                expression: "0.2 + 0.5 * t_sec".into(),
            },
            ParamBindingDef {
                node_id: NodeId::from("dm"),
                param_name: "far_depth".into(),
                expression: "0.4 + 0.5 * t_sec".into(),
            },
        ],
    );
    let animator = ProjectAnimator::new(&project).expect("animator");
    let dm_t0 = compositor
        .render_frame(&project.graph, Flicks::ZERO, frame_ticks, w, h, &provider, &animator, &test_opts())
        .expect("t0");
    let dm_t1 = compositor
        .render_frame(
            &project.graph,
            Flicks(FLICKS_PER_SECOND / 2),
            frame_ticks,
            w,
            h,
            &provider,
            &animator,
            &test_opts(),
        )
        .expect("t0.5");
    assert_ne!(dm_t0.fnv1a_hash, dm_t1.fnv1a_hash);
}

#[test]
fn mask_matte_red_mode_opacity_choke_spatial_feather_and_animation() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let frame_ticks = FPS_60.frame_duration().unwrap();
    let (w, h) = (32, 32);

    let provider = FnFrameProvider(|asset_id: &str, _: Flicks, pw: u32, ph: u32| {
        if asset_id == "red_vs_luma" {
            // Left half: R=0, G=1, B=1 (bright cyan -> high luma, zero red!). Right half: R=1, G=0, B=0.
            let mut px = Vec::with_capacity((pw * ph) as usize);
            for _y in 0..ph {
                for x in 0..pw {
                    if x < pw / 2 {
                        px.push([0.0, 1.0, 1.0, 1.0]);
                    } else {
                        px.push([1.0, 0.0, 0.0, 1.0]);
                    }
                }
            }
            Ok(SourceFrame::rgba_f32_premul(pw, ph, px))
        } else if asset_id == "square" {
            let mut px = vec![[0.0, 0.0, 0.0, 0.0]; (pw * ph) as usize];
            for y in 8..24 {
                for x in 8..24 {
                    px[(y * pw + x) as usize] = [1.0, 1.0, 1.0, 1.0];
                }
            }
            Ok(SourceFrame::rgba_f32_premul(pw, ph, px))
        } else {
            Ok(SourceFrame::rgba_f32_premul(
                pw,
                ph,
                vec![[0.9, 0.8, 0.7, 1.0]; (pw * ph) as usize],
            ))
        }
    });

    let make_mm = |matte_asset: &str,
                   mode: MatteMode,
                   opacity: f32,
                   choke_px: f32,
                   spatial_feather_px: f32| RenderGraphDef {
        output_node: NodeId::from("mm"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("src"),
                kind: NodeKind::SourceVideo {
                    asset_id: "base".into(),
                    clip_id: None,
                    color_space: ColorSpace::LinearRec709,
                    fit: SourceFit::Stretch,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("msk"),
                kind: NodeKind::SourceVideo {
                    asset_id: matte_asset.into(),
                    clip_id: None,
                    color_space: ColorSpace::LinearRec709,
                    fit: SourceFit::Stretch,
                },
                inputs: vec![],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("mm"),
                kind: NodeKind::MaskMatte {
                    mode,
                    invert: false,
                    feather: 0.0,
                    opaque_black_outside: false,
                    opacity,
                    choke_px,
                    spatial_feather_px,
                },
                inputs: vec![NodeId::from("src"), NodeId::from("msk")],
                enabled: true,
            },
        ],
    };

    let red_out = compositor
        .render_frame(
            &make_mm("red_vs_luma", MatteMode::Red, 1.0, 0.0, 0.0),
            Flicks::ZERO,
            frame_ticks,
            w,
            h,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("red matte");
    let left_alpha = red_out.rgba8[(16 * w + 4) as usize * 4 + 3];
    let right_alpha = red_out.rgba8[(16 * w + 28) as usize * 4 + 3];
    assert_eq!(left_alpha, 0, "MatteMode::Red must ignore G/B/A on left half where R=0");
    assert_eq!(right_alpha, 255, "MatteMode::Red must preserve right half where R=1");

    // Partial mask opacity (~0.4) leaves unmasked alpha around (1 - 0.4)*255 = 153 on left half
    let partial_op = compositor
        .render_frame(
            &make_mm("red_vs_luma", MatteMode::Red, 0.4, 0.0, 0.0),
            Flicks::ZERO,
            frame_ticks,
            w,
            h,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("partial opacity");
    let left_partial = partial_op.rgba8[(16 * w + 4) as usize * 4 + 3];
    assert!(
        (140..=165).contains(&left_partial),
        "opacity=0.4 should blend mask intensity, got alpha={left_partial}"
    );

    // Choke & spatial feather on square matte
    let sq_base = compositor
        .render_frame(
            &make_mm("square", MatteMode::Alpha, 1.0, 0.0, 0.0),
            Flicks::ZERO,
            frame_ticks,
            w,
            h,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("square base");
    let sq_erode = compositor
        .render_frame(
            &make_mm("square", MatteMode::Alpha, 1.0, -2.0, 0.0),
            Flicks::ZERO,
            frame_ticks,
            w,
            h,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("square choke erode");
    let sq_dilate = compositor
        .render_frame(
            &make_mm("square", MatteMode::Alpha, 1.0, 2.0, 0.0),
            Flicks::ZERO,
            frame_ticks,
            w,
            h,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("square choke dilate");
    let sq_feather = compositor
        .render_frame(
            &make_mm("square", MatteMode::Alpha, 1.0, 0.0, 3.0),
            Flicks::ZERO,
            frame_ticks,
            w,
            h,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("square spatial feather");

    let base_px = count_visible_alpha(&sq_base.rgba8, 128);
    assert!(count_visible_alpha(&sq_erode.rgba8, 128) < base_px);
    assert!(count_visible_alpha(&sq_dilate.rgba8, 128) > base_px);
    let soft_edges = sq_feather
        .rgba8
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|px| px[3] > 5 && px[3] < 250)
        .count();
    assert!(soft_edges > 16, "spatial_feather_px must create smooth edge falloff pixels, got {soft_edges}");
}

#[test]
fn gpu_slug_text_range_shape_easing_rotation_tracking_and_randomize_seed() {
    let font_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fonts/Anton-Regular.ttf");
    assert!(Path::new(font_path).is_file(), "bundled Anton-Regular.ttf must exist");

    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let provider = no_override_provider();
    let frame_ticks = FPS_60.frame_duration().unwrap();
    let (w, h) = (200, 96);

    let make_slug_graph = |shape: TextRangeShape,
                           ease_high: f32,
                           ease_low: f32,
                           randomize_seed: Option<u64>,
                           rotation_deg_from: f32,
                           tracking_em_from: f32| RenderGraphDef {
        output_node: NodeId::from("txt"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("txt"),
            kind: NodeKind::VectorText {
                text: "ANIME".into(),
                font_size_norm: 0.38,
                color_rgba: [1.0, 1.0, 1.0, 1.0],
                outline_rgba: [0.0; 4],
                outline_width_norm: 0.0,
                align_center: true,
                y_offset_norm: 0.5,
                font_family: None,
                font_path: Some(font_path.into()),
                font_weight: 400,
                align: Some(TextAlign2D::Center),
                x_offset_norm: None,
                tracking_em: Some(0.0),
                line_height_em: None,
                opacity: 1.0,
                fallback_fonts: Vec::new(),
                writing_mode: WritingMode2D::HorizontalTb,
                var_wght: None,
                var_wdth: None,
                var_slnt: None,
                color_bottom_rgba: None,
                gradient_angle_deg: 0.0,
                outer_outline_rgba: [0.0; 4],
                outer_outline_width_norm: 0.0,
                outline_gap_norm: 0.0,
                shadow_offset_norm: [0.0; 2],
                shadow_blur_norm: 0.0,
                shadow_rgba: [0.0; 4],
                glow_radius_norm: 0.0,
                glow_rgba: [0.0; 4],
                hdr_intensity: 1.0,
                gpu_slug: true,
                text_animator: Some(TextAnimatorDef {
                    unit: TextSelectorUnit::Grapheme,
                    range_start: 0,
                    range_end: Some(5),
                    start_tick: 0,
                    duration_ticks: FLICKS_PER_SECOND,
                    delay_ticks: FLICKS_PER_SECOND / 10,
                    position_from_norm: [0.0, 0.08],
                    scale_from: 1.0,
                    opacity_from: 0.1,
                    rotation_deg_from,
                    tracking_em_from,
                    shape,
                    ease_high,
                    ease_low,
                    randomize_seed,
                }),
            },
            inputs: vec![],
            enabled: true,
        }],
    };

    let mid_tick = Flicks(FLICKS_PER_SECOND / 4);
    let mut shape_hashes = HashSet::new();
    for shape in [
        TextRangeShape::Square,
        TextRangeShape::RampUp,
        TextRangeShape::RampDown,
        TextRangeShape::Triangle,
        TextRangeShape::Smooth,
    ] {
        let f = compositor
            .render_frame(
                &make_slug_graph(shape, 75.0, 25.0, None, 35.0, 0.45),
                mid_tick,
                frame_ticks,
                w,
                h,
                &provider,
                &NoopAnimator,
                &test_opts(),
            )
            .expect("gpu slug text animator render");
        assert!(count_visible_alpha(&f.rgba8, 16) > 40);
        shape_hashes.insert(f.fnv1a_hash);
    }
    assert_eq!(shape_hashes.len(), 5, "all 5 TextRangeShapes must yield distinct GPU Slug renders");

    // Easing changes mid-animation frame
    let linear_sq = compositor
        .render_frame(
            &make_slug_graph(TextRangeShape::Square, 0.0, 0.0, None, 35.0, 0.45),
            mid_tick,
            frame_ticks,
            w,
            h,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("linear sq");
    let eased_sq = compositor
        .render_frame(
            &make_slug_graph(TextRangeShape::Square, 90.0, 10.0, None, 35.0, 0.45),
            mid_tick,
            frame_ticks,
            w,
            h,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("eased sq");
    assert_ne!(linear_sq.fnv1a_hash, eased_sq.fnv1a_hash);

    // Zero vs non-zero rotation/tracking alters output
    let no_rot_track = compositor
        .render_frame(
            &make_slug_graph(TextRangeShape::Square, 0.0, 0.0, None, 0.0, 0.0),
            mid_tick,
            frame_ticks,
            w,
            h,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("no rot track");
    assert_ne!(no_rot_track.fnv1a_hash, linear_sq.fnv1a_hash);

    // randomize_seed determinism & distinct seed ordering
    let seed77_a = compositor
        .render_frame(
            &make_slug_graph(TextRangeShape::Square, 50.0, 50.0, Some(77), 30.0, 0.2),
            mid_tick,
            frame_ticks,
            w,
            h,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("seed 77 a");
    let seed77_b = compositor
        .render_frame(
            &make_slug_graph(TextRangeShape::Square, 50.0, 50.0, Some(77), 30.0, 0.2),
            mid_tick,
            frame_ticks,
            w,
            h,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("seed 77 b");
    let seed123 = compositor
        .render_frame(
            &make_slug_graph(TextRangeShape::Square, 50.0, 50.0, Some(123), 30.0, 0.2),
            mid_tick,
            frame_ticks,
            w,
            h,
            &provider,
            &NoopAnimator,
            &test_opts(),
        )
        .expect("seed 123");
    assert_eq!(seed77_a.fnv1a_hash, seed77_b.fnv1a_hash);
    assert_ne!(seed77_a.fnv1a_hash, seed123.fnv1a_hash);
}

#[test]
fn bloom_inverse_square_and_motion_tile_shake_tilt_noise_and_shutter_anim() {
    let mut compositor = GpuCompositor::new_headless().expect("headless init");
    let (w, h) = (64, 64);

    // 1. BloomPassUniform: 0.0 == legacy with_optics, 1.0 == inverse-square falloff
    let mut base_px = vec![[0.05f32, 0.05, 0.05, 1.0]; (w * h) as usize];
    let mut bloom_px = vec![[0.0f32; 4]; (w * h) as usize];
    for y in 24..40 {
        for x in 24..40 {
            bloom_px[(y * w + x) as usize] = [2.0, 1.2, 0.4, 1.0];
        }
    }
    base_px[(32 * w + 32) as usize] = [0.2, 0.2, 0.2, 1.0];

    let u_legacy = BloomPassUniform::with_optics(
        [1.0 / w as f32, 1.0 / h as f32],
        0.5,
        0.2,
        1.2,
        2.5,
        false,
        false,
        [1.0, 1.0, 1.0],
        0.75,
        0.0,
        0.0,
        0.8,
    );
    let u_zero = u_legacy.with_inverse_square_falloff(0.0);
    let u_inv_sq = u_legacy.with_inverse_square_falloff(1.0);

    let out_legacy = run_single_pass_rgba16f(
        &compositor,
        &compositor.pipelines().fx_bloom_upsample,
        w,
        h,
        &base_px,
        Some(&bloom_px),
        bytemuck::bytes_of(&u_legacy),
    );
    let out_zero = run_single_pass_rgba16f(
        &compositor,
        &compositor.pipelines().fx_bloom_upsample,
        w,
        h,
        &base_px,
        Some(&bloom_px),
        bytemuck::bytes_of(&u_zero),
    );
    let out_inv_sq = run_single_pass_rgba16f(
        &compositor,
        &compositor.pipelines().fx_bloom_upsample,
        w,
        h,
        &base_px,
        Some(&bloom_px),
        bytemuck::bytes_of(&u_inv_sq),
    );
    assert_eq!(out_legacy, out_zero, "inv_sq_falloff=0.0 must match legacy bloom upsample exactly");
    assert_ne!(out_legacy, out_inv_sq, "inv_sq_falloff=1.0 must modify mip/kernel falloff");

    // 2. MotionTileShakeUniform: 0.0 tilt/noise == legacy; non-zero perspective tilt + noise shifts output
    let mut tile_px = vec![[0.0f32; 4]; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            let chk = ((x / 8) + (y / 8)) % 2 == 0;
            tile_px[(y * w + x) as usize] = if chk {
                [0.9, 0.2, 0.2, 1.0]
            } else {
                [0.1, 0.4, 0.9, 1.0]
            };
        }
    }
    let m_legacy = MotionTileShakeUniform::new(1.0, true, [0.02, 0.02], 0.05, 0.0, 6.0, 2.0, 0.1);
    let m_zero = m_legacy.with_tilt_and_noise(0.0, 0.0, 0.0);
    let m_tilt = m_legacy.with_tilt_and_noise(0.25, -0.18, 0.0);
    let m_noise = m_legacy.with_tilt_and_noise(0.0, 0.0, 0.8);

    let t_legacy = run_single_pass_rgba16f(
        &compositor,
        &compositor.pipelines().fx_motion_tile_shake,
        w,
        h,
        &tile_px,
        None,
        bytemuck::bytes_of(&m_legacy),
    );
    let t_zero = run_single_pass_rgba16f(
        &compositor,
        &compositor.pipelines().fx_motion_tile_shake,
        w,
        h,
        &tile_px,
        None,
        bytemuck::bytes_of(&m_zero),
    );
    let t_tilt = run_single_pass_rgba16f(
        &compositor,
        &compositor.pipelines().fx_motion_tile_shake,
        w,
        h,
        &tile_px,
        None,
        bytemuck::bytes_of(&m_tilt),
    );
    let t_noise = run_single_pass_rgba16f(
        &compositor,
        &compositor.pipelines().fx_motion_tile_shake,
        w,
        h,
        &tile_px,
        None,
        bytemuck::bytes_of(&m_noise),
    );
    assert_eq!(t_legacy, t_zero, "tilt/noise=0 must match legacy motion tile shake identically");
    assert_ne!(t_legacy, t_tilt, "non-zero 3D tilt must warp perspective");
    assert_ne!(t_legacy, t_noise, "non-zero noise_amp must perturb shake");

    // 3. MotionBlurShutter shutter_angle_deg animation via ProjectAnimator
    let frame_ticks = FPS_30.frame_duration().unwrap();
    let src_provider = FnFrameProvider(|_: &str, _: Flicks, pw: u32, ph: u32| Ok(center_dot_plate(pw, ph)));
    let graph = RenderGraphDef {
        output_node: NodeId::from("shutter"),
        nodes: vec![
            GraphNodeDef {
                id: NodeId::from("src"),
                kind: NodeKind::SourceVideo {
                    asset_id: "dot".into(),
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
                    scale: [1.0, 1.0],
                    rotation_rad: 0.0,
                    anchor: [0.5, 0.5],
                    filter: frameshift_schema::SampleFilter::Bilinear,
                },
                inputs: vec![NodeId::from("src")],
                enabled: true,
            },
            GraphNodeDef {
                id: NodeId::from("shutter"),
                kind: NodeKind::MotionBlurShutter {
                    shutter_angle_deg: 0.0,
                    sub_samples: 8,
                    phase: ShutterPhase::Centered,
                    tonemap: false,
                },
                inputs: vec![NodeId::from("xform")],
                enabled: true,
            },
        ],
    };
    let project = make_project(
        "shutter_anim",
        w,
        h,
        graph,
        vec![
            ParamBindingDef {
                node_id: NodeId::from("xform"),
                param_name: "translate_x".into(),
                expression: "0.22 * sin(25.13274 * (t_sec - 1.0))".into(),
            },
            ParamBindingDef {
                node_id: NodeId::from("shutter"),
                param_name: "shutter_angle_deg".into(),
                expression: "step(0.5, t_sec) * 360.0".into(),
            },
        ],
    );
    let animator = ProjectAnimator::new(&project).expect("animator");
    let no_blur = compositor
        .render_frame(
            &project.graph,
            Flicks(FLICKS_PER_SECOND / 4),
            frame_ticks,
            w,
            h,
            &src_provider,
            &animator,
            &test_opts(),
        )
        .expect("shutter 0 deg");
    let full_blur = compositor
        .render_frame(
            &project.graph,
            Flicks(FLICKS_PER_SECOND),
            frame_ticks,
            w,
            h,
            &src_provider,
            &animator,
            &test_opts(),
        )
        .expect("shutter 360 deg");
    // Animated shutter_angle=360 smears across more alpha pixels than 0 deg (translating dot has same sharp footprint at any t)
    assert!(
        count_visible_alpha(&full_blur.rgba8, 8) > count_visible_alpha(&no_blur.rgba8, 8) + 20,
        "animated MotionBlurShutter shutter_angle_deg=360 must smear moving dot vs 0 deg"
    );
}
