use crate::engine::{Engine, collect_timeline_frame_ticks};
use crate::error::CoreError;
use frameshift_media::{
    AlphaMode, AudioAnalysisReport, Rgba8Frame, downscale_frame_nearest_or_box, linear_to_srgb,
    srgb_to_linear,
};
use frameshift_render::{OutputAlphaMode, RenderOptions, TonemapMode};
use frameshift_schema::{
    BlendMode, ColorSpace, GraphNodeDef, HalftoneColorMode, MatteMode, NodeId, NodeKind, Project,
    RenderGraphDef, RenderProfileDef, RenderProfileKind, Resolution, SchemaVersion, Shape3DKind,
    SourceFit,
};
use frameshift_time::{FLICKS_PER_SECOND, Flicks};
use frameshift_vector::{
    ProceduralShapeKind, Shape3DParams, build_shape3d_mesh, render_label_badge,
    render_label_badge_scaled,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const SHEET_BG: [u8; 4] = [14, 17, 24, 255];
const HEADER_BG: [u8; 4] = [24, 30, 42, 255];
const CELL_BORDER: [u8; 4] = [56, 68, 92, 255];
const BADGE_FG: [u8; 4] = [245, 248, 255, 255];
const BADGE_BG: [u8; 4] = [12, 16, 24, 224];
const HEADER_FG: [u8; 4] = [56, 189, 248, 255];

/// Hash record for a single sampled frame in `inspection_manifest.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SampledFrameHash {
    pub tick: Flicks,
    pub frame_index: i64,
    pub fnv1a_hash64: u64,
    pub sha256_hex: String,
}

/// Velocity-strip sample tying output timing to source timing and instantaneous speed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VelocitySampleRecord {
    pub clip_id: String,
    pub output_tick: Flicks,
    pub source_tick: Flicks,
    pub instant_speed: f64,
    pub source_to_output_ratio: f64,
    pub frame_hash64: u64,
}

/// Epistemic scope declaration required by `inspection-contract.md` §4 and `PROJECT_NOTES.md` §5.2.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpistemicScopeReport {
    pub revision_id: u64,
    pub profile: RenderProfileKind,
    pub verified_by_code: Vec<String>,
    pub requires_visual_model_or_human: Vec<String>,
    pub not_proven_by_contact_sheet: Vec<String>,
    pub verified_surfaces: Vec<String>,
    pub unverified_aspects: Vec<String>,
    pub epistemic_note: String,
}

/// Complete manifest written to `inspection_manifest.json` by [`Engine::export_inspection_pack`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InspectionPackManifest {
    pub revision_id: u64,
    pub schema_version: SchemaVersion,
    pub project_name: String,
    pub profile: RenderProfileKind,
    pub out_dir: PathBuf,
    pub sampled_ticks: Vec<Flicks>,
    pub frame_hashes: Vec<SampledFrameHash>,
    pub velocity_samples: Vec<VelocitySampleRecord>,
    pub audio_analysis: Option<AudioAnalysisReport>,
    pub epistemic_scope: EpistemicScopeReport,
    pub contact_sheet_path: PathBuf,
    pub mask_pack_path: PathBuf,
    pub transition_pack_path: PathBuf,
    pub velocity_pack_path: PathBuf,
    pub crop_fullres_path: PathBuf,
    pub fx_showcase_pack_path: PathBuf,
    pub manifest_path: PathBuf,
}

pub(crate) fn export_inspection_pack_impl(
    engine: &mut Engine,
    project: &Project,
    out_dir: &Path,
) -> Result<InspectionPackManifest, CoreError> {
    project.validate()?;
    std::fs::create_dir_all(out_dir)?;

    let all_ticks = collect_timeline_frame_ticks(project)?;
    let frame_ticks = project.timeline.fps.frame_duration()?;
    let final_profile = project.render_profiles.final_profile.clone();
    let draft_profile = project.render_profiles.draft.clone();

    // 1. Contact Sheet (3x2 grid of 6 key frames across the timeline).
    let contact_ticks = select_six_key_ticks(project, &all_ticks);
    let cell_w = 426u32;
    let cell_h = 240u32;
    let pad = 16u32;
    let header_h = 52u32;
    let sheet_w = 3 * cell_w + 4 * pad;
    let sheet_h = header_h + 2 * cell_h + 3 * pad;

    let mut contact_sheet = Rgba8Frame::filled(
        sheet_w,
        sheet_h,
        Flicks::ZERO,
        SHEET_BG,
        AlphaMode::Straight,
    )?;
    fill_rect_u8(&mut contact_sheet, 0, 0, sheet_w, header_h, HEADER_BG);
    let header_text = format!(
        "REVISION #{} | {} | CONTACT SHEET 3X2",
        project.revision_id, project.name
    );
    render_label_badge_scaled(
        &mut contact_sheet,
        pad,
        10,
        &header_text,
        HEADER_FG,
        HEADER_BG,
        2,
    );

    let mut frame_hashes = Vec::with_capacity(contact_ticks.len());
    for (idx, &tick) in contact_ticks.iter().enumerate() {
        let rendered = engine.render_frame_at(project, RenderProfileKind::Final, tick)?;
        let frame_index = tick.0 / frame_ticks.0.max(1);
        frame_hashes.push(SampledFrameHash {
            tick,
            frame_index,
            fnv1a_hash64: rendered.fnv1a_hash64(),
            sha256_hex: rendered.sha256_hex(),
        });

        let thumb = downscale_frame_nearest_or_box(&rendered, cell_w, cell_h)?;
        let col = (idx % 3) as u32;
        let row = (idx / 3) as u32;
        let dx = pad + col * (cell_w + pad);
        let dy = header_h + pad + row * (cell_h + pad);
        blit_frame_with_border(&mut contact_sheet, &thumb, dx, dy, CELL_BORDER);

        let sec = tick.to_seconds_f64();
        let event_tag = project
            .events
            .iter()
            .find(|ev| (ev.target_output_tick.0 - tick.0).abs() <= frame_ticks.0)
            .map(|ev| format!(" [{}]", ev.event_id))
            .unwrap_or_default();
        let cell_label = format!(
            "REV #{} f:{} {:.2}s tick:{}{}",
            project.revision_id, frame_index, sec, tick.0, event_tag
        );
        render_label_badge(
            &mut contact_sheet,
            dx + 6,
            dy + cell_h - 22,
            &cell_label,
            BADGE_FG,
            BADGE_BG,
        );
    }

    let contact_sheet_path = out_dir.join("contact_sheet.png");
    contact_sheet.save_png(&contact_sheet_path)?;

    // 2. Mask Pack (2x2 quad panel at the mask composite tick).
    let mask_tick = choose_mask_tick(project, &all_ticks);
    let mask_cell_w = 480u32;
    let mask_cell_h = 270u32;
    let mask_sheet_w = 2 * mask_cell_w + 3 * pad;
    let mask_sheet_h = header_h + 2 * mask_cell_h + 3 * pad;
    let mut mask_pack = Rgba8Frame::filled(
        mask_sheet_w,
        mask_sheet_h,
        mask_tick,
        SHEET_BG,
        AlphaMode::Straight,
    )?;
    fill_rect_u8(&mut mask_pack, 0, 0, mask_sheet_w, header_h, HEADER_BG);
    render_label_badge_scaled(
        &mut mask_pack,
        pad,
        10,
        &format!(
            "REVISION #{} | MASK PACK | TICK {}",
            project.revision_id, mask_tick.0
        ),
        HEADER_FG,
        HEADER_BG,
        2,
    );

    let (src_node_id, matte_node_id, mask_node_id, matte_mode, matte_invert) =
        find_mask_nodes(project);
    let mask_profile = RenderProfileDef {
        resolution: frameshift_schema::Resolution {
            width: mask_cell_w,
            height: mask_cell_h,
        },
        motion_blur_sub_samples: 1,
        ..draft_profile.clone()
    };
    let straight_no_tonemap = RenderOptions {
        output_alpha_mode: OutputAlphaMode::Straight,
        tonemap_mode: Some(TonemapMode::None),
        ..Default::default()
    };

    let p1_src = render_subgraph_node(
        engine,
        project,
        &src_node_id,
        &mask_profile,
        mask_tick,
        &straight_no_tonemap,
    )?;
    let p2_raw_matte = render_subgraph_node(
        engine,
        project,
        &matte_node_id,
        &mask_profile,
        mask_tick,
        &straight_no_tonemap,
    )?;
    let p2_matte_vis = visualize_matte_frame(&p2_raw_matte, matte_mode, matte_invert)?;

    let p3_cutout_raw = render_subgraph_node(
        engine,
        project,
        &mask_node_id,
        &mask_profile,
        mask_tick,
        &straight_no_tonemap,
    )?;
    let p3_checker = composite_over_checkerboard(&p3_cutout_raw)?;

    let p4_final_full = engine.render_frame_at(project, RenderProfileKind::Final, mask_tick)?;
    let p4_final = downscale_frame_nearest_or_box(&p4_final_full, mask_cell_w, mask_cell_h)?;

    let mask_panels = [
        (0u32, 0u32, p1_src, "PANEL 1: SOURCE CLIP A"),
        (1u32, 0u32, p2_matte_vis, "PANEL 2: ALPHA MATTE"),
        (0u32, 1u32, p3_checker, "PANEL 3: CHECKERBOARD CUTOUT"),
        (1u32, 1u32, p4_final, "PANEL 4: FINAL COMPOSITE"),
    ];
    for (col, row, frame, label) in mask_panels {
        let dx = pad + col * (mask_cell_w + pad);
        let dy = header_h + pad + row * (mask_cell_h + pad);
        blit_frame_with_border(&mut mask_pack, &frame, dx, dy, CELL_BORDER);
        render_label_badge(&mut mask_pack, dx + 6, dy + 6, label, BADGE_FG, BADGE_BG);
    }
    let mask_pack_path = out_dir.join("mask_pack.png");
    mask_pack.save_png(&mask_pack_path)?;

    // 3. Transition Pack (3-panel strip at transition mid-point).
    let transition_tick = choose_transition_tick(project, &all_ticks);
    let tr_w = 426u32;
    let tr_h = 240u32;
    let tr_sheet_w = 3 * tr_w + 4 * pad;
    let tr_sheet_h = header_h + tr_h + 2 * pad;
    let mut transition_pack = Rgba8Frame::filled(
        tr_sheet_w,
        tr_sheet_h,
        transition_tick,
        SHEET_BG,
        AlphaMode::Straight,
    )?;
    fill_rect_u8(&mut transition_pack, 0, 0, tr_sheet_w, header_h, HEADER_BG);
    render_label_badge_scaled(
        &mut transition_pack,
        pad,
        10,
        &format!(
            "REVISION #{} | TRANSITION PACK | TICK {}",
            project.revision_id, transition_tick.0
        ),
        HEADER_FG,
        HEADER_BG,
        2,
    );

    let (tr_a_id, tr_b_id) = find_transition_input_nodes(project);
    let tr_profile = RenderProfileDef {
        resolution: frameshift_schema::Resolution {
            width: tr_w,
            height: tr_h,
        },
        motion_blur_sub_samples: 1,
        ..draft_profile.clone()
    };
    let tr_opts = RenderOptions {
        output_alpha_mode: OutputAlphaMode::Straight,
        ..Default::default()
    };
    let clip_a_raw = render_subgraph_node(
        engine,
        project,
        &tr_a_id,
        &tr_profile,
        transition_tick,
        &tr_opts,
    )?;
    let clip_b_raw = render_subgraph_node(
        engine,
        project,
        &tr_b_id,
        &tr_profile,
        transition_tick,
        &tr_opts,
    )?;
    let blended_full =
        engine.render_frame_at(project, RenderProfileKind::Final, transition_tick)?;
    let blended = downscale_frame_nearest_or_box(&blended_full, tr_w, tr_h)?;

    let tr_panels = [
        (0u32, clip_a_raw, "CLIP A"),
        (1u32, clip_b_raw, "CLIP B"),
        (2u32, blended, "WHIP + CHROMATIC + MOTION BLUR"),
    ];
    for (col, frame, label) in tr_panels {
        let dx = pad + col * (tr_w + pad);
        let dy = header_h + pad;
        blit_frame_with_border(&mut transition_pack, &frame, dx, dy, CELL_BORDER);
        render_label_badge(
            &mut transition_pack,
            dx + 6,
            dy + 6,
            label,
            BADGE_FG,
            BADGE_BG,
        );
    }
    let transition_pack_path = out_dir.join("transition_pack.png");
    transition_pack.save_png(&transition_pack_path)?;

    // 4. Velocity Pack (strip of 5 sampled speed-ramp frames + source/output tick ratio labels).
    let (vel_clip_id, vel_ticks) = select_velocity_ticks(project, &all_ticks, 5);
    let vel_w = 320u32;
    let vel_h = 180u32;
    let vel_pad = 12u32;
    let n_vel = vel_ticks.len().max(1) as u32;
    let vel_sheet_w = n_vel * vel_w + (n_vel + 1) * vel_pad;
    let vel_sheet_h = header_h + vel_h + 2 * vel_pad;
    let mut velocity_pack = Rgba8Frame::filled(
        vel_sheet_w,
        vel_sheet_h,
        Flicks::ZERO,
        SHEET_BG,
        AlphaMode::Straight,
    )?;
    fill_rect_u8(&mut velocity_pack, 0, 0, vel_sheet_w, header_h, HEADER_BG);
    render_label_badge_scaled(
        &mut velocity_pack,
        vel_pad,
        10,
        &format!(
            "REVISION #{} | VELOCITY PACK | CLIP {}",
            project.revision_id, vel_clip_id
        ),
        HEADER_FG,
        HEADER_BG,
        2,
    );

    let mut velocity_samples = Vec::with_capacity(vel_ticks.len());
    for (idx, &tick) in vel_ticks.iter().enumerate() {
        let frame_full = engine.render_frame_at(project, RenderProfileKind::Draft, tick)?;
        let hash64 = frame_full.fnv1a_hash64();
        let thumb = downscale_frame_nearest_or_box(&frame_full, vel_w, vel_h)?;

        let (src_tick, instant_speed, ratio) = if let Some(clip) = project.clip(&vel_clip_id) {
            let out_off = Flicks((tick.0 - clip.timeline_start.0).max(0));
            let src_off = clip.speed_curve.source_offset_at(out_off);
            let spd = clip.speed_curve.instant_speed_at(out_off);
            let r = if out_off.0 > 0 {
                src_off.0 as f64 / out_off.0 as f64
            } else {
                spd
            };
            (clip.source_start + src_off, spd, r)
        } else {
            (tick, 1.0, 1.0)
        };

        velocity_samples.push(VelocitySampleRecord {
            clip_id: vel_clip_id.clone(),
            output_tick: tick,
            source_tick: src_tick,
            instant_speed,
            source_to_output_ratio: ratio,
            frame_hash64: hash64,
        });

        let dx = vel_pad + (idx as u32) * (vel_w + vel_pad);
        let dy = header_h + vel_pad;
        blit_frame_with_border(&mut velocity_pack, &thumb, dx, dy, CELL_BORDER);
        let label = format!(
            "out:{:.2}s src:{:.2}s {:.2}x r:{:.2}",
            tick.to_seconds_f64(),
            src_tick.to_seconds_f64(),
            instant_speed,
            ratio
        );
        render_label_badge(
            &mut velocity_pack,
            dx + 4,
            dy + vel_h - 20,
            &label,
            BADGE_FG,
            BADGE_BG,
        );
    }
    let velocity_pack_path = out_dir.join("velocity_pack.png");
    velocity_pack.save_png(&velocity_pack_path)?;

    // 5. Full-Resolution Center Crop (1:1 640x360 around impact/title area at impact tick).
    let impact_tick = choose_impact_tick(project, &all_ticks);
    let full_impact = engine.render_frame_at(project, RenderProfileKind::Final, impact_tick)?;
    let crop = crop_impact_title_1to1(&full_impact, project, 640, 360)?;
    let crop_fullres_path = out_dir.join("crop_fullres.png");
    crop.save_png(&crop_fullres_path)?;

    // 5b. Anime FX & Transition Showcase Pack (4x2 8-panel comparison sheet).
    let fx_showcase_pack_path = out_dir.join("fx_showcase_pack.png");
    let _ = export_fx_showcase_pack_impl(engine, &fx_showcase_pack_path)?;

    // 6. Audio beat analysis + EpistemicScopeReport + inspection_manifest.json.
    let audio_analysis = match engine.mix_project_audio(project)? {
        Some(buf) => Some(buf.analyze_beats_and_energy(Flicks(FLICKS_PER_SECOND / 100))?),
        None => None,
    };

    let epistemic_scope = EpistemicScopeReport {
        revision_id: project.revision_id,
        profile: final_profile.kind,
        verified_by_code: vec![
            "schema_validation_and_acyclic_dag".into(),
            "flicks_deterministic_frame_hashes".into(),
            "speed_curve_source_offset_integration".into(),
            "audio_rms_envelope_and_beat_onset_detection".into(),
            "full_res_1to1_crop_dimensions".into(),
        ],
        requires_visual_model_or_human: vec![
            "character_face_and_typography_legibility".into(),
            "mask_edge_halos_and_fringing".into(),
            "color_grading_and_lut_aesthetics".into(),
            "whip_transition_composition".into(),
        ],
        not_proven_by_contact_sheet: vec![
            "inter_frame_flicker".into(),
            "motion_smoothness".into(),
            "audio_rhythm_feel".into(),
        ],
        verified_surfaces: vec![
            "visual_contact_sheet".into(),
            "visual_mask_pack".into(),
            "visual_transition_pack".into(),
            "visual_velocity_pack".into(),
            "visual_fullres_crop".into(),
            "visual_fx_showcase_pack".into(),
            "technical_json".into(),
        ],
        unverified_aspects: vec![
            "inter_frame_flicker".into(),
            "motion_smoothness".into(),
            "audio_rhythm_feel".into(),
        ],
        epistemic_note: "Contact sheet, packs, and algorithmic audio metrics verify technical invariants only; inter-frame flicker, motion smoothness, and audio rhythm feel require video playback or human/visual evaluation.".into(),
    };

    let manifest_path = out_dir.join("inspection_manifest.json");
    let manifest = InspectionPackManifest {
        revision_id: project.revision_id,
        schema_version: project.schema_version,
        project_name: project.name.clone(),
        profile: final_profile.kind,
        out_dir: out_dir.to_path_buf(),
        sampled_ticks: contact_ticks,
        frame_hashes,
        velocity_samples,
        audio_analysis,
        epistemic_scope,
        contact_sheet_path,
        mask_pack_path,
        transition_pack_path,
        velocity_pack_path,
        crop_fullres_path,
        fx_showcase_pack_path,
        manifest_path: manifest_path.clone(),
    };
    std::fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)?;

    Ok(manifest)
}

pub(crate) fn export_fx_showcase_pack_impl(
    engine: &mut Engine,
    out_path: &Path,
) -> Result<Rgba8Frame, CoreError> {
    if let Some(parent) = out_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }

    let mut base_project = Project::golden_scene_preset();
    base_project.param_bindings.clear();
    for clip in &mut base_project.timeline.clips {
        clip.event_animations.clear();
    }
    let sample_tick = base_project
        .event("drop_impact")
        .map(|ev| ev.target_output_tick)
        .unwrap_or(Flicks(2 * FLICKS_PER_SECOND + FLICKS_PER_SECOND / 2));

    let sheet_w = 1920u32;
    let sheet_h = 1080u32;
    let pad = 16u32;
    let banner_h = 68u32;
    let card_w = (sheet_w - 5 * pad) / 4; // 460
    let card_h = (sheet_h - banner_h - 3 * pad) / 2; // 482
    let card_header_h = 34u32;
    let card_footer_h = 50u32;
    let cell_w = card_w - 4; // 456
    let cell_h = card_h - card_header_h - card_footer_h - 4; // 394

    let mut sheet =
        Rgba8Frame::filled(sheet_w, sheet_h, sample_tick, SHEET_BG, AlphaMode::Straight)?;
    fill_rect_u8(&mut sheet, 0, 0, sheet_w, banner_h, HEADER_BG);
    fill_rect_u8(&mut sheet, 0, banner_h - 2, sheet_w, 2, HEADER_FG);
    render_label_badge_scaled(
        &mut sheet,
        pad,
        14,
        "FRAMESHIFT ANIME FX & TRANSITION LIBRARY // 15 WGSL SHADERS",
        HEADER_FG,
        HEADER_BG,
        3,
    );

    let cell_profile = RenderProfileDef {
        resolution: Resolution {
            width: cell_w,
            height: cell_h,
        },
        motion_blur_sub_samples: 1,
        shutter_angle_deg: 0.0,
        ..base_project.render_profiles.draft.clone()
    };
    let cell_opts = RenderOptions {
        tonemap_mode: Some(TonemapMode::AcesLite),
        output_alpha_mode: OutputAlphaMode::Straight,
        exposure: 1.0,
        ..Default::default()
    };

    let render_transition_panel = |engine: &mut Engine,
                                   shader_name: &str,
                                   direction_rad: f32,
                                   whip_strength: f32,
                                   chromatic_aberration: f32,
                                   impact_flash: f32|
     -> Result<Rgba8Frame, CoreError> {
        let mut proj = base_project.clone();
        proj.param_bindings.clear();
        proj.graph = RenderGraphDef {
            output_node: NodeId::from("tr_out"),
            nodes: vec![
                GraphNodeDef {
                    id: NodeId::from("src_a"),
                    kind: NodeKind::SourceVideo {
                        asset_id: "asset_clip_a".into(),
                        clip_id: Some("clip_a".into()),
                        color_space: ColorSpace::DisplaySrgb,
                        fit: SourceFit::Stretch,
                    },
                    inputs: vec![],
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("src_b"),
                    kind: NodeKind::SourceVideo {
                        asset_id: "asset_clip_b".into(),
                        clip_id: Some("clip_b".into()),
                        color_space: ColorSpace::DisplaySrgb,
                        fit: SourceFit::Stretch,
                    },
                    inputs: vec![],
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("tr_out"),
                    kind: NodeKind::ShaderTransition {
                        shader_name: shader_name.into(),
                        progress: 0.5,
                        direction_rad,
                        whip_strength,
                        chromatic_aberration,
                        impact_flash,
                    },
                    inputs: vec![NodeId::from("src_a"), NodeId::from("src_b")],
                    enabled: true,
                },
            ],
        };
        proj.validate()?;
        engine.render_frame_with_profile_def_pub(&proj, &cell_profile, sample_tick, &cell_opts)
    };

    let render_postfx_panel =
        |engine: &mut Engine, fx_kind: NodeKind| -> Result<Rgba8Frame, CoreError> {
            let mut proj = base_project.clone();
            proj.param_bindings.clear();
            proj.graph = RenderGraphDef {
                output_node: NodeId::from("fx_out"),
                nodes: vec![
                    GraphNodeDef {
                        id: NodeId::from("src_a"),
                        kind: NodeKind::SourceVideo {
                            asset_id: "asset_clip_a".into(),
                            clip_id: Some("clip_a".into()),
                            color_space: ColorSpace::DisplaySrgb,
                            fit: SourceFit::Stretch,
                        },
                        inputs: vec![],
                        enabled: true,
                    },
                    GraphNodeDef {
                        id: NodeId::from("src_b"),
                        kind: NodeKind::SourceVideo {
                            asset_id: "asset_clip_b".into(),
                            clip_id: Some("clip_b".into()),
                            color_space: ColorSpace::DisplaySrgb,
                            fit: SourceFit::Stretch,
                        },
                        inputs: vec![],
                        enabled: true,
                    },
                    GraphNodeDef {
                        id: NodeId::from("ab_comp"),
                        kind: NodeKind::Blend {
                            mode: BlendMode::Screen,
                            opacity: 0.72,
                        },
                        inputs: vec![NodeId::from("src_a"), NodeId::from("src_b")],
                        enabled: true,
                    },
                    GraphNodeDef {
                        id: NodeId::from("title_badge"),
                        kind: NodeKind::VectorText {
                            text: "IMPACT".into(),
                            font_size_norm: 0.14,
                            color_rgba: [1.0, 0.98, 0.92, 1.0],
                            outline_rgba: [0.05, 0.04, 0.12, 0.95],
                            outline_width_norm: 0.012,
                            align_center: true,
                            y_offset_norm: 0.52,
                            font_family: None,
                            font_path: None,
                            font_weight: 400,
                            align: None,
                            x_offset_norm: None,
                            tracking_em: None,
                            line_height_em: None,
                            opacity: 1.0,
                            fallback_fonts: Vec::new(),
                            writing_mode: Default::default(),
                            var_wght: None,
                            var_wdth: None,
                            var_slnt: None,
                            color_bottom_rgba: None,
                            gradient_angle_deg: 0.0,
                            outer_outline_rgba: [0.0; 4],
                            outer_outline_width_norm: 0.0,
                            outline_gap_norm: 0.0,
                            shadow_offset_norm: [0.0, 0.0],
                            shadow_blur_norm: 0.0,
                            shadow_rgba: [0.0; 4],
                            glow_radius_norm: 0.0,
                            glow_rgba: [0.0; 4],
                            hdr_intensity: 1.0,
                            gpu_slug: true,
                            text_animator: None,
                        },
                        inputs: vec![],
                        enabled: true,
                    },
                    GraphNodeDef {
                        id: NodeId::from("comp_with_title"),
                        kind: NodeKind::Blend {
                            mode: BlendMode::Normal,
                            opacity: 1.0,
                        },
                        inputs: vec![NodeId::from("ab_comp"), NodeId::from("title_badge")],
                        enabled: true,
                    },
                    GraphNodeDef {
                        id: NodeId::from("fx_out"),
                        kind: fx_kind,
                        inputs: vec![NodeId::from("comp_with_title")],
                        enabled: true,
                    },
                ],
            };
            proj.validate()?;
            engine.render_frame_with_profile_def_pub(&proj, &cell_profile, sample_tick, &cell_opts)
        };

    struct PanelSpec<'a> {
        col: u32,
        row: u32,
        title: &'a str,
        shader_line: &'a str,
        params_line: &'a str,
        frame: Rgba8Frame,
    }

    let panels = vec![
        PanelSpec {
            col: 0,
            row: 0,
            title: "1: WHIP CHROMATIC",
            shader_line: "WGSL: transition_whip_chromatic.wgsl",
            params_line: "P=0.50 WHIP=0.95 CA=0.045 FLASH=0.35",
            frame: render_transition_panel(engine, "whip_chromatic", 0.0, 0.95, 0.045, 0.35)?,
        },
        PanelSpec {
            col: 1,
            row: 0,
            title: "2: CROSS ZOOM (MIT)",
            shader_line: "WGSL: transition_cross_zoom.wgsl",
            params_line: "P=0.50 ZOOM=0.65 CA=0.030 24-TAP",
            frame: render_transition_panel(engine, "cross_zoom", 0.35, 0.65, 0.030, 0.20)?,
        },
        PanelSpec {
            col: 2,
            row: 0,
            title: "3: DIRECTIONAL WARP (MIT)",
            shader_line: "WGSL: transition_directional_warp.wgsl",
            params_line: "P=0.50 DIR=45DEG WARP=1.80 CA=0.040",
            frame: render_transition_panel(
                engine,
                "directional_warp",
                std::f32::consts::FRAC_PI_4,
                1.8,
                0.040,
                0.15,
            )?,
        },
        PanelSpec {
            col: 3,
            row: 0,
            title: "4: GLITCH DISPLACE (MIT)",
            shader_line: "WGSL: transition_glitch_displace.wgsl",
            params_line: "P=0.50 DISP=1.10 VORONOI-3X3 CA=0.05",
            frame: render_transition_panel(engine, "glitch_displace", 0.0, 1.10, 0.050, 0.25)?,
        },
        PanelSpec {
            col: 0,
            row: 1,
            title: "5: MOTION TILE + SHAKE",
            shader_line: "WGSL: fx_motion_tile_shake.wgsl",
            params_line: "TILE=1.35 MIRROR=1 ROT=0.14 ZOOM=0.12",
            frame: render_postfx_panel(
                engine,
                NodeKind::MotionTileShake {
                    tile_scale: 1.35,
                    mirror_edges: true,
                    shake_amp_norm: [0.05, 0.035],
                    shake_rot_rad: 0.14,
                    shake_zoom: 0.12,
                    shake_freq_hz: 14.0,
                    shake_decay: 3.0,
                    event_delta_sec: 0.04,
                },
            )?,
        },
        PanelSpec {
            col: 1,
            row: 1,
            title: "6: ANIME IMPACT FRAME",
            shader_line: "WGSL: fx_anime_impact_frame.wgsl",
            params_line: "SOBEL=1.40 INV=0.85 SPEED-LINES=0.85",
            frame: render_postfx_panel(
                engine,
                NodeKind::AnimeImpactFrame {
                    luma_threshold: 0.42,
                    edge_weight: 1.4,
                    invert_amount: 0.85,
                    speed_line_density: 0.72,
                    speed_line_intensity: 0.85,
                    center_uv: [0.5, 0.5],
                    mix_amount: 0.95,
                },
            )?,
        },
        PanelSpec {
            col: 2,
            row: 1,
            title: "7: RADIAL SHOCKWAVE",
            shader_line: "WGSL: fx_radial_shockwave.wgsl",
            params_line: "R=0.34 REFR=0.080 BLUR=0.28 CA=0.045",
            frame: render_postfx_panel(
                engine,
                NodeKind::RadialShockwave {
                    center_uv: [0.5, 0.5],
                    radius: 0.34,
                    ring_width: 0.14,
                    refraction_amp: 0.08,
                    zoom_blur_strength: 0.28,
                    chromatic_aberration: 0.045,
                },
            )?,
        },
        PanelSpec {
            col: 3,
            row: 1,
            title: "8: HALFTONE + DEEP GLOW",
            shader_line: "WGSL: fx_halftone_glow.wgsl",
            params_line: "DOTS=48 MIX=0.55 GLOW=1.15 RAD=16PX",
            frame: render_postfx_panel(
                engine,
                NodeKind::HalftoneGlow {
                    dot_scale: 48.0,
                    angle_rad: std::f32::consts::FRAC_PI_4,
                    halftone_mix: 0.55,
                    glow_threshold: 0.45,
                    glow_intensity: 1.15,
                    glow_radius_px: 16.0,
                    color_mode: HalftoneColorMode::Monochrome,
                    dot_softness: 0.0,
                },
            )?,
        },
    ];

    let card_footer_bg = [18, 22, 32, 255];
    let card_border_accent = [56, 189, 248, 255];
    for panel in panels {
        let cx = pad + panel.col * (card_w + pad);
        let cy = banner_h + pad + panel.row * (card_h + pad);

        // Card background + border
        fill_rect_u8(&mut sheet, cx, cy, card_w, card_h, CELL_BORDER);
        fill_rect_u8(
            &mut sheet,
            cx + 1,
            cy + 1,
            card_w - 2,
            card_h - 2,
            card_footer_bg,
        );

        // Card header bar
        fill_rect_u8(
            &mut sheet,
            cx + 1,
            cy + 1,
            card_w - 2,
            card_header_h,
            HEADER_BG,
        );
        fill_rect_u8(
            &mut sheet,
            cx + 1,
            cy + card_header_h,
            card_w - 2,
            1,
            card_border_accent,
        );
        render_label_badge_scaled(
            &mut sheet,
            cx + 8,
            cy + 8,
            panel.title,
            BADGE_FG,
            HEADER_BG,
            2,
        );

        // GPU-rendered FX viewport
        let img_x = cx + 2;
        let img_y = cy + card_header_h + 2;
        blit_frame_with_border(&mut sheet, &panel.frame, img_x, img_y, CELL_BORDER);

        // Bottom spec footer
        let foot_y = img_y + cell_h + 6;
        let hash_short = format!("HASH:0x{:016x}", panel.frame.fnv1a_hash64());
        render_label_badge(
            &mut sheet,
            cx + 8,
            foot_y,
            panel.shader_line,
            HEADER_FG,
            card_footer_bg,
        );
        render_label_badge(
            &mut sheet,
            cx + 8,
            foot_y + 18,
            &format!("{}  {}", panel.params_line, &hash_short[..15]),
            BADGE_FG,
            card_footer_bg,
        );
    }

    sheet.save_png(out_path)?;
    Ok(sheet)
}

// ---------------------------------------------------------------------------
// Material / shape swatch gallery (5 shapes x 4 material modes, 4x5 grid).
// ---------------------------------------------------------------------------

/// Stable tool identifier written to `materials_manifest.json`.
pub const MATERIAL_GALLERY_TOOL: &str = "frameshift-core/material-gallery";
/// Version of the material gallery manifest schema.
pub const MATERIAL_GALLERY_SCHEMA_VERSION: u32 = 1;
/// Sheet file name written into the gallery output directory.
pub const MATERIAL_GALLERY_SHEET_FILE: &str = "materials_sheet.png";
/// Manifest file name written into the gallery output directory.
pub const MATERIAL_GALLERY_MANIFEST_FILE: &str = "materials_manifest.json";

const GALLERY_COLS: u32 = 4;
const GALLERY_ROWS: u32 = 5;
const GALLERY_CELL_W: u32 = 320;
const GALLERY_CELL_H: u32 = 180;
const GALLERY_PAD: u32 = 16;
const GALLERY_HEADER_H: u32 = 68;
const GALLERY_RENDER_SCALE: u32 = 2;
const GALLERY_CAMERA_Z: f32 = 1.67;
const GALLERY_FOV_DEG: f32 = 50.0;
const GALLERY_SHAPE_SIZE: f32 = 1.05;
const GALLERY_INNER_RATIO: f32 = 0.382;
const GALLERY_EXTRUDE_DEPTH: f32 = 0.12;
const GALLERY_BEVEL_SIZE: f32 = 0.01;
const GALLERY_CELL_BG: [u8; 4] = [20, 24, 33, 255];
/// Summed `|RGB - background|` delta above which a pixel counts as shape coverage.
const GALLERY_NON_BG_DELTA: i32 = 24;

// Fixed signature palette: identical in all 20 cells so only the material changes.
const GALLERY_COLOR_TOP: [f32; 4] = [1.20, 0.62, 0.08, 1.0];
const GALLERY_COLOR_BOTTOM: [f32; 4] = [1.00, 0.15, 0.02, 1.0];
const GALLERY_SIDE_COLOR: [f32; 4] = [0.10, 0.02, 0.00, 1.0];
const GALLERY_OUTLINE_COLOR: [f32; 4] = [0.35, 0.10, 0.02, 1.0];

const GALLERY_DIAMOND_NOTE: &str =
    "8-point star at default inner_ratio 0.382 (use inner_ratio >= 0.99 for a 4-point rhombus)";

const GALLERY_NOTES: [&str; 3] = [
    "diamond_3d renders as an 8-point star at default inner_ratio 0.382; inner_ratio >= 0.99 yields a 4-point rhombus.",
    "material_mode 0 (neon_3d) is an emissive Cook-Torrance body, not a flat fill; material_mode 2 (flat_2d_stroked) is the unlit flat fill.",
    "mean_rgb is the mean of non-background pixels in linear Rec.709; non_background_ratio counts pixels whose summed |RGB - background| delta exceeds 24.",
];

struct GalleryShapeSpec {
    kind: Shape3DKind,
    display_name: &'static str,
    stroke_width: f32,
    note: Option<&'static str>,
}

const GALLERY_SHAPES: [GalleryShapeSpec; 5] = [
    GalleryShapeSpec {
        kind: Shape3DKind::NeonTile,
        display_name: "NEON TILE",
        stroke_width: 0.06,
        note: None,
    },
    GalleryShapeSpec {
        kind: Shape3DKind::HollowStar5,
        display_name: "HOLLOW STAR 5",
        // A 0.06 band would cover < 6% of the cell; 0.12 keeps the star legible at 320x180.
        stroke_width: 0.12,
        note: None,
    },
    GalleryShapeSpec {
        kind: Shape3DKind::Heart3D,
        display_name: "HEART 3D",
        stroke_width: 0.06,
        note: None,
    },
    GalleryShapeSpec {
        kind: Shape3DKind::LightningBolt,
        display_name: "LIGHTNING BOLT",
        stroke_width: 0.06,
        note: None,
    },
    GalleryShapeSpec {
        kind: Shape3DKind::Diamond3D,
        display_name: "DIAMOND 3D",
        stroke_width: 0.06,
        note: Some(GALLERY_DIAMOND_NOTE),
    },
];

struct GalleryMaterialSpec {
    mode: u32,
    name: &'static str,
    display_name: &'static str,
}

const GALLERY_MATERIALS: [GalleryMaterialSpec; 4] = [
    GalleryMaterialSpec {
        mode: 0,
        name: "neon_3d",
        display_name: "NEON 3D",
    },
    GalleryMaterialSpec {
        mode: 1,
        name: "metallic_chrome",
        display_name: "METALLIC CHROME",
    },
    GalleryMaterialSpec {
        mode: 2,
        name: "flat_2d_stroked",
        display_name: "FLAT 2D STROKED",
    },
    GalleryMaterialSpec {
        mode: 3,
        name: "iridescent",
        display_name: "IRIDESCENT",
    },
];

/// Pixel rectangle of one material gallery cell inside the sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialCellRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Grid geometry of the material gallery sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialGalleryGrid {
    pub cols: u32,
    pub rows: u32,
    pub cell_width: u32,
    pub cell_height: u32,
    pub pad: u32,
    pub header_height: u32,
}

/// Dimensions and content hashes of `materials_sheet.png`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialGallerySheet {
    pub file: String,
    pub width: u32,
    pub height: u32,
    pub fnv1a_hash64: u64,
    pub sha256_hex: String,
}

/// Fixed camera shared by every gallery cell.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MaterialGalleryCamera {
    pub pos: [f32; 3],
    pub fov_deg: f32,
    pub shape_size: f32,
}

/// Signature palette shared by every gallery cell (linear Rec.709 unless noted).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MaterialGalleryPalette {
    pub color_top_rgba: [f32; 4],
    pub color_bottom_rgba: [f32; 4],
    pub side_color_rgba: [f32; 4],
    pub outline_color_rgba: [f32; 4],
    pub cell_background_rgba8: [u8; 4],
}

/// One shape x material swatch cell of the material gallery.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterialCellRecord {
    /// Row-major index in `0..20`: `row * 4 + col`.
    pub cell_index: u32,
    pub row: u32,
    pub col: u32,
    pub shape_kind: Shape3DKind,
    pub material_mode: u32,
    pub material_name: String,
    pub cell_rect: MaterialCellRect,
    /// Mean of non-background pixels in linear Rec.709.
    pub mean_rgb: [f32; 3],
    pub non_background_ratio: f32,
    /// Mesh height divided by the camera's visible height at `z = 0`.
    pub height_fraction: f32,
    pub stroke_width: f32,
    pub inner_ratio: f32,
    pub pixel_hash64: u64,
    pub sha256_hex: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Complete manifest written to `materials_manifest.json`.
///
/// The manifest deliberately carries no wall-clock field so two runs over the same
/// engine produce byte-identical JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterialGalleryManifest {
    pub schema_version: u32,
    pub tool: String,
    pub grid: MaterialGalleryGrid,
    pub sheet: MaterialGallerySheet,
    pub camera: MaterialGalleryCamera,
    pub palette: MaterialGalleryPalette,
    pub color_space: String,
    pub tonemap: String,
    pub sample_tick: Flicks,
    pub cells: Vec<MaterialCellRecord>,
    pub notes: Vec<String>,
}

/// Renders the 5 shapes x 4 `material_mode` swatch gallery into `out_dir`.
///
/// Writes `materials_sheet.png` (4 columns x 5 rows of 320x180 cells) and
/// `materials_manifest.json`. Everything is fixed (camera, palette, tick), so the
/// output is deterministic for a given GPU adapter. The `inspect` pack is not
/// touched: this is a separate, project-independent reference surface.
pub fn export_material_gallery_impl(
    engine: &mut Engine,
    out_dir: &Path,
) -> Result<MaterialGalleryManifest, CoreError> {
    std::fs::create_dir_all(out_dir)?;

    let mut base_project = Project::golden_scene_preset();
    base_project.param_bindings.clear();
    for clip in &mut base_project.timeline.clips {
        clip.event_animations.clear();
    }

    let sheet_w = GALLERY_COLS * GALLERY_CELL_W + (GALLERY_COLS + 1) * GALLERY_PAD;
    let sheet_h =
        GALLERY_HEADER_H + GALLERY_ROWS * GALLERY_CELL_H + (GALLERY_ROWS + 1) * GALLERY_PAD;
    let mut sheet = Rgba8Frame::filled(
        sheet_w,
        sheet_h,
        Flicks::ZERO,
        SHEET_BG,
        AlphaMode::Straight,
    )?;
    fill_rect_u8(&mut sheet, 0, 0, sheet_w, GALLERY_HEADER_H, HEADER_BG);
    fill_rect_u8(&mut sheet, 0, GALLERY_HEADER_H - 2, sheet_w, 2, HEADER_FG);
    render_label_badge_scaled(
        &mut sheet,
        GALLERY_PAD,
        12,
        "FRAMESHIFT MATERIAL / SHAPE SWATCH GALLERY // 5 SHAPES X 4 MATERIAL MODES",
        HEADER_FG,
        HEADER_BG,
        2,
    );
    render_label_badge(
        &mut sheet,
        GALLERY_PAD,
        44,
        &format!(
            "CAM Z={:.2} FOV={:.0} | CELL {}X{} | LINEAR REC.709 | ACES-LITE | TICK 0",
            GALLERY_CAMERA_Z, GALLERY_FOV_DEG, GALLERY_CELL_W, GALLERY_CELL_H
        ),
        BADGE_FG,
        HEADER_BG,
    );

    let cell_profile = RenderProfileDef {
        resolution: Resolution {
            width: GALLERY_CELL_W * GALLERY_RENDER_SCALE,
            height: GALLERY_CELL_H * GALLERY_RENDER_SCALE,
        },
        motion_blur_sub_samples: 1,
        shutter_angle_deg: 0.0,
        ..base_project.render_profiles.draft.clone()
    };
    let cell_opts = RenderOptions {
        tonemap_mode: Some(TonemapMode::AcesLite),
        output_alpha_mode: OutputAlphaMode::Straight,
        exposure: 1.0,
        ..Default::default()
    };

    let mut cells = Vec::with_capacity((GALLERY_COLS * GALLERY_ROWS) as usize);
    for (row, shape) in GALLERY_SHAPES.iter().enumerate() {
        let height_fraction = gallery_height_fraction(shape)?;
        for (col, material) in GALLERY_MATERIALS.iter().enumerate() {
            let project = gallery_cell_project(&base_project, shape, material.mode);
            project.validate()?;
            let rendered = engine.render_frame_with_profile_def_pub(
                &project,
                &cell_profile,
                Flicks::ZERO,
                &cell_opts,
            )?;
            let opaque = composite_over_solid(&rendered, GALLERY_CELL_BG)?;
            let thumb = downscale_frame_nearest_or_box(&opaque, GALLERY_CELL_W, GALLERY_CELL_H)?;

            let cell_rect = MaterialCellRect {
                x: GALLERY_PAD + (col as u32) * (GALLERY_CELL_W + GALLERY_PAD),
                y: GALLERY_HEADER_H + GALLERY_PAD + (row as u32) * (GALLERY_CELL_H + GALLERY_PAD),
                width: GALLERY_CELL_W,
                height: GALLERY_CELL_H,
            };
            blit_frame_with_border(&mut sheet, &thumb, cell_rect.x, cell_rect.y, CELL_BORDER);
            draw_gallery_color_chips(&mut sheet, cell_rect.x, cell_rect.y);
            let label = format!("{} / {}", shape.display_name, material.display_name);
            render_label_badge(
                &mut sheet,
                cell_rect.x + 6,
                cell_rect.y + GALLERY_CELL_H - 22,
                &label,
                BADGE_FG,
                BADGE_BG,
            );

            let stats = gallery_cell_stats(&thumb);
            cells.push(MaterialCellRecord {
                cell_index: (row as u32) * GALLERY_COLS + col as u32,
                row: row as u32,
                col: col as u32,
                shape_kind: shape.kind,
                material_mode: material.mode,
                material_name: material.name.to_string(),
                cell_rect,
                mean_rgb: stats.mean_rgb,
                non_background_ratio: stats.non_background_ratio,
                height_fraction,
                stroke_width: shape.stroke_width,
                inner_ratio: GALLERY_INNER_RATIO,
                pixel_hash64: thumb.pixel_fnv1a_hash64(),
                sha256_hex: thumb.sha256_hex(),
                note: shape.note.map(str::to_string),
            });
        }
    }

    sheet.save_png(&out_dir.join(MATERIAL_GALLERY_SHEET_FILE))?;

    let manifest = MaterialGalleryManifest {
        schema_version: MATERIAL_GALLERY_SCHEMA_VERSION,
        tool: MATERIAL_GALLERY_TOOL.to_string(),
        grid: MaterialGalleryGrid {
            cols: GALLERY_COLS,
            rows: GALLERY_ROWS,
            cell_width: GALLERY_CELL_W,
            cell_height: GALLERY_CELL_H,
            pad: GALLERY_PAD,
            header_height: GALLERY_HEADER_H,
        },
        sheet: MaterialGallerySheet {
            file: MATERIAL_GALLERY_SHEET_FILE.to_string(),
            width: sheet.width,
            height: sheet.height,
            fnv1a_hash64: sheet.fnv1a_hash64(),
            sha256_hex: sheet.sha256_hex(),
        },
        camera: MaterialGalleryCamera {
            pos: [0.0, 0.0, GALLERY_CAMERA_Z],
            fov_deg: GALLERY_FOV_DEG,
            shape_size: GALLERY_SHAPE_SIZE,
        },
        palette: MaterialGalleryPalette {
            color_top_rgba: GALLERY_COLOR_TOP,
            color_bottom_rgba: GALLERY_COLOR_BOTTOM,
            side_color_rgba: GALLERY_SIDE_COLOR,
            outline_color_rgba: GALLERY_OUTLINE_COLOR,
            cell_background_rgba8: GALLERY_CELL_BG,
        },
        color_space: "linear_rec709".to_string(),
        tonemap: "aces_lite".to_string(),
        sample_tick: Flicks::ZERO,
        cells,
        notes: GALLERY_NOTES
            .iter()
            .map(|note| (*note).to_string())
            .collect(),
    };
    std::fs::write(
        out_dir.join(MATERIAL_GALLERY_MANIFEST_FILE),
        serde_json::to_string_pretty(&manifest)?,
    )?;

    Ok(manifest)
}

fn gallery_cell_project(
    base_project: &Project,
    shape: &GalleryShapeSpec,
    material_mode: u32,
) -> Project {
    let mut project = base_project.clone();
    project.param_bindings.clear();
    project.graph = RenderGraphDef {
        output_node: NodeId::from("gallery_shape"),
        nodes: vec![GraphNodeDef {
            id: NodeId::from("gallery_shape"),
            kind: NodeKind::Shape3D {
                shape_kind: shape.kind,
                size: GALLERY_SHAPE_SIZE,
                aspect: 1.0,
                stroke_width: shape.stroke_width,
                corner_radius: 0.0,
                inner_ratio: GALLERY_INNER_RATIO,
                extrude_depth: GALLERY_EXTRUDE_DEPTH,
                bevel_size: GALLERY_BEVEL_SIZE,
                material_mode,
                color_top_rgba: GALLERY_COLOR_TOP,
                color_bottom_rgba: GALLERY_COLOR_BOTTOM,
                side_color_rgba: GALLERY_SIDE_COLOR,
                outline_color_rgba: GALLERY_OUTLINE_COLOR,
                pos: [0.0, 0.0, 0.0],
                rot_deg: [0.0, 0.0, 0.0],
                scale: [1.0, 1.0, 1.0],
                camera_pos: [0.0, 0.0, GALLERY_CAMERA_Z],
                camera_rot_deg: [0.0, 0.0, 0.0],
                fov_deg: GALLERY_FOV_DEG,
                pop_duration_sec: 0.0,
                pop_overshoot: 1.0,
                spin_in_deg: 0.0,
                echo_count: 0,
                echo_offset: [0.0, 0.0, 0.0],
                echo_alpha_decay: 1.0,
                hdr_intensity: 1.0,
                time_offset_sec: 0.0,
                opacity: 1.0,
            },
            inputs: vec![],
            enabled: true,
        }],
    };
    project
}

fn gallery_procedural_kind(kind: Shape3DKind) -> ProceduralShapeKind {
    match kind {
        Shape3DKind::NeonTile => ProceduralShapeKind::NeonTile,
        Shape3DKind::HollowStar5 => ProceduralShapeKind::HollowStar5,
        Shape3DKind::Heart3D => ProceduralShapeKind::Heart3D,
        Shape3DKind::LightningBolt => ProceduralShapeKind::LightningBolt,
        Shape3DKind::Diamond3D => ProceduralShapeKind::Diamond3D,
    }
}

/// Height of the generated mesh divided by the visible height of the fixed camera at `z = 0`.
fn gallery_height_fraction(shape: &GalleryShapeSpec) -> Result<f32, CoreError> {
    let mesh = build_shape3d_mesh(Shape3DParams {
        kind: gallery_procedural_kind(shape.kind),
        size: GALLERY_SHAPE_SIZE,
        aspect: 1.0,
        stroke_width: shape.stroke_width,
        corner_radius: 0.0,
        inner_ratio: GALLERY_INNER_RATIO,
        extrude_depth: GALLERY_EXTRUDE_DEPTH,
        bevel_size: GALLERY_BEVEL_SIZE,
    })?;
    let visible_height = 2.0 * GALLERY_CAMERA_Z * (0.5 * GALLERY_FOV_DEG.to_radians()).tan();
    Ok((mesh.bounds_max[1] - mesh.bounds_min[1]) / visible_height)
}

/// Alpha-composites a straight-alpha `frame` over a solid background, yielding opaque pixels.
fn composite_over_solid(frame: &Rgba8Frame, bg: [u8; 4]) -> Result<Rgba8Frame, CoreError> {
    let mut out = Rgba8Frame::new(
        frame.width,
        frame.height,
        frame.pts,
        AlphaMode::Straight,
        vec![0u8; frame.data.len()],
    )?;
    for idx in (0..frame.data.len()).step_by(4) {
        let alpha = frame.data[idx + 3] as u32;
        let inv = 255 - alpha;
        out.data[idx] = ((frame.data[idx] as u32 * alpha + bg[0] as u32 * inv + 127) / 255) as u8;
        out.data[idx + 1] =
            ((frame.data[idx + 1] as u32 * alpha + bg[1] as u32 * inv + 127) / 255) as u8;
        out.data[idx + 2] =
            ((frame.data[idx + 2] as u32 * alpha + bg[2] as u32 * inv + 127) / 255) as u8;
        out.data[idx + 3] = 255;
    }
    Ok(out)
}

struct GalleryCellStats {
    mean_rgb: [f32; 3],
    non_background_ratio: f32,
}

fn gallery_cell_stats(frame: &Rgba8Frame) -> GalleryCellStats {
    let mut sum = [0.0f64; 3];
    let mut count = 0u64;
    for idx in (0..frame.data.len()).step_by(4) {
        let dr = (frame.data[idx] as i32 - GALLERY_CELL_BG[0] as i32).abs();
        let dg = (frame.data[idx + 1] as i32 - GALLERY_CELL_BG[1] as i32).abs();
        let db = (frame.data[idx + 2] as i32 - GALLERY_CELL_BG[2] as i32).abs();
        if dr + dg + db > GALLERY_NON_BG_DELTA {
            sum[0] += srgb_to_linear(frame.data[idx] as f32 / 255.0) as f64;
            sum[1] += srgb_to_linear(frame.data[idx + 1] as f32 / 255.0) as f64;
            sum[2] += srgb_to_linear(frame.data[idx + 2] as f32 / 255.0) as f64;
            count += 1;
        }
    }
    let divisor = count.max(1) as f64;
    GalleryCellStats {
        mean_rgb: [
            (sum[0] / divisor) as f32,
            (sum[1] / divisor) as f32,
            (sum[2] / divisor) as f32,
        ],
        non_background_ratio: count as f32 / frame.pixel_count() as f32,
    }
}

/// Draws the three fixed palette chips (top, bottom, side) in the cell's top-right corner.
fn draw_gallery_color_chips(canvas: &mut Rgba8Frame, cell_x: u32, cell_y: u32) {
    const CHIP: u32 = 16;
    const GAP: u32 = 4;
    const CHIP_BORDER: [u8; 4] = [8, 10, 14, 255];
    let colors = [GALLERY_COLOR_TOP, GALLERY_COLOR_BOTTOM, GALLERY_SIDE_COLOR];
    let total_w = 3 * CHIP + 2 * GAP;
    let x0 = cell_x + GALLERY_CELL_W - 6 - total_w;
    let y0 = cell_y + 6;
    for (index, linear) in colors.iter().enumerate() {
        let chip_x = x0 + (index as u32) * (CHIP + GAP);
        fill_rect_u8(canvas, chip_x - 1, y0 - 1, CHIP + 2, CHIP + 2, CHIP_BORDER);
        fill_rect_u8(
            canvas,
            chip_x,
            y0,
            CHIP,
            CHIP,
            linear_rgba_to_srgb_u8(*linear),
        );
    }
}

fn linear_rgba_to_srgb_u8(rgba: [f32; 4]) -> [u8; 4] {
    [
        (linear_to_srgb(rgba[0]) * 255.0).round() as u8,
        (linear_to_srgb(rgba[1]) * 255.0).round() as u8,
        (linear_to_srgb(rgba[2]) * 255.0).round() as u8,
        (rgba[3].clamp(0.0, 1.0) * 255.0).round() as u8,
    ]
}

fn render_subgraph_node(
    engine: &mut Engine,
    project: &Project,
    target_node_id: &NodeId,
    profile_def: &RenderProfileDef,
    tick: Flicks,
    options: &RenderOptions,
) -> Result<Rgba8Frame, CoreError> {
    let mut sub_project = project.clone();
    sub_project.graph.output_node = target_node_id.clone();
    // Keep param_bindings only for nodes reachable by target_node_id so validation passes cleanly.
    engine.render_frame_with_profile_def_pub(&sub_project, profile_def, tick, options)
}

fn select_six_key_ticks(project: &Project, all_ticks: &[Flicks]) -> Vec<Flicks> {
    if all_ticks.len() <= 6 {
        return all_ticks.to_vec();
    }
    let mut chosen = Vec::with_capacity(6);
    // Include the primary event tick if present, plus evenly spaced timeline samples.
    let mut candidates = vec![
        all_ticks[0],
        all_ticks[(all_ticks.len() * 20) / 100],
        all_ticks[(all_ticks.len() * 44) / 100],
        all_ticks[(all_ticks.len() * 48) / 100],
        all_ticks[(all_ticks.len() * 50) / 100],
        all_ticks[(all_ticks.len() * 82) / 100],
    ];
    if let Some(ev) = project.events.first()
        && let Some(&nearest) = all_ticks
            .iter()
            .min_by_key(|t| (t.0 - ev.target_output_tick.0).abs())
    {
        candidates[4] = nearest;
    }
    for t in candidates {
        if !chosen.contains(&t) {
            chosen.push(t);
        }
    }
    for &t in all_ticks {
        if chosen.len() >= 6 {
            break;
        }
        if !chosen.contains(&t) {
            chosen.push(t);
        }
    }
    chosen.sort_by_key(|t| t.0);
    chosen.truncate(6);
    chosen
}

fn choose_mask_tick(project: &Project, all_ticks: &[Flicks]) -> Flicks {
    let target = if let Some(clip) = project.timeline.clips.first() {
        let span = clip.timeline_end.0 - clip.timeline_start.0;
        Flicks(clip.timeline_start.0 + (span * 73) / 100)
    } else {
        all_ticks[all_ticks.len() / 2]
    };
    nearest_tick(all_ticks, target)
}

fn choose_transition_tick(project: &Project, all_ticks: &[Flicks]) -> Flicks {
    let target = if project.timeline.clips.len() >= 2 {
        let a = &project.timeline.clips[0];
        let b = &project.timeline.clips[1];
        let ov_start = a.timeline_start.0.max(b.timeline_start.0);
        let ov_end = a.timeline_end.0.min(b.timeline_end.0);
        if ov_end > ov_start {
            Flicks(ov_start + (ov_end - ov_start) * 40 / 100)
        } else {
            all_ticks[all_ticks.len() / 2]
        }
    } else {
        all_ticks[all_ticks.len() / 2]
    };
    nearest_tick(all_ticks, target)
}

fn choose_impact_tick(project: &Project, all_ticks: &[Flicks]) -> Flicks {
    let target = project
        .event("drop_impact")
        .or_else(|| project.events.first())
        .map(|ev| ev.target_output_tick)
        .unwrap_or_else(|| all_ticks[all_ticks.len() / 2]);
    nearest_tick(all_ticks, target)
}

fn nearest_tick(all_ticks: &[Flicks], target: Flicks) -> Flicks {
    all_ticks
        .iter()
        .copied()
        .min_by_key(|t| (t.0 - target.0).abs())
        .unwrap_or(Flicks::ZERO)
}

fn find_mask_nodes(project: &Project) -> (NodeId, NodeId, NodeId, MatteMode, bool) {
    for node in &project.graph.nodes {
        if let NodeKind::MaskMatte { mode, invert, .. } = node.kind
            && node.inputs.len() >= 2
        {
            return (
                node.inputs[0].clone(),
                node.inputs[1].clone(),
                node.id.clone(),
                mode,
                invert,
            );
        }
    }
    let out = project.graph.output_node.clone();
    (out.clone(), out.clone(), out, MatteMode::Alpha, false)
}

fn find_transition_input_nodes(project: &Project) -> (NodeId, NodeId) {
    for node in &project.graph.nodes {
        if matches!(node.kind, NodeKind::ShaderTransition { .. }) && node.inputs.len() >= 2 {
            return (node.inputs[0].clone(), node.inputs[1].clone());
        }
    }
    for node in &project.graph.nodes {
        if matches!(node.kind, NodeKind::Blend { .. }) && node.inputs.len() >= 2 {
            return (node.inputs[0].clone(), node.inputs[1].clone());
        }
    }
    let out = project.graph.output_node.clone();
    (out.clone(), out)
}

fn select_velocity_ticks(
    project: &Project,
    all_ticks: &[Flicks],
    count: usize,
) -> (String, Vec<Flicks>) {
    let Some(clip) = project.timeline.clips.first() else {
        let mut out = Vec::new();
        for i in 0..count.min(all_ticks.len()) {
            let idx = (i * (all_ticks.len() - 1)) / count.saturating_sub(1).max(1);
            out.push(all_ticks[idx]);
        }
        return ("timeline".into(), out);
    };

    let clip_ticks: Vec<Flicks> = all_ticks
        .iter()
        .copied()
        .filter(|&t| t >= clip.timeline_start && t < clip.timeline_end)
        .collect();
    if clip_ticks.is_empty() {
        return (clip.id.clone(), vec![all_ticks[0]]);
    }
    let mut out = Vec::new();
    let n = count.min(clip_ticks.len()).max(1);
    for i in 0..n {
        let idx = if n == 1 {
            0
        } else {
            (i * (clip_ticks.len() - 1)) / (n - 1)
        };
        let t = clip_ticks[idx];
        if !out.contains(&t) {
            out.push(t);
        }
    }
    (clip.id.clone(), out)
}

fn visualize_matte_frame(
    matte: &Rgba8Frame,
    mode: MatteMode,
    invert: bool,
) -> Result<Rgba8Frame, CoreError> {
    let mut out = matte.clone();
    out.alpha_mode = AlphaMode::Straight;
    for px in out.data.as_chunks_mut::<4>().0 {
        let raw = match mode {
            MatteMode::Alpha => px[3],
            MatteMode::Luma => {
                let l = 0.2126 * (px[0] as f32) + 0.7152 * (px[1] as f32) + 0.0722 * (px[2] as f32);
                l.round().clamp(0.0, 255.0) as u8
            }
        };
        let v = if invert { 255 - raw } else { raw };
        px[0] = v;
        px[1] = v;
        px[2] = v;
        px[3] = 255;
    }
    Ok(out)
}

fn composite_over_checkerboard(cutout: &Rgba8Frame) -> Result<Rgba8Frame, CoreError> {
    let mut out = Rgba8Frame::new(
        cutout.width,
        cutout.height,
        cutout.pts,
        AlphaMode::Straight,
        vec![0u8; cutout.data.len()],
    )?;
    let sq = 16u32;
    for y in 0..cutout.height {
        for x in 0..cutout.width {
            let light = ((x / sq) + (y / sq)).is_multiple_of(2);
            let bg = if light {
                [196u8, 202, 212]
            } else {
                [52u8, 58, 68]
            };
            let idx = ((y as usize) * (cutout.width as usize) + (x as usize)) * 4;
            let sr = cutout.data[idx] as u32;
            let sg = cutout.data[idx + 1] as u32;
            let sb = cutout.data[idx + 2] as u32;
            let sa = cutout.data[idx + 3] as u32;
            let inv = 255 - sa;
            out.data[idx] = ((sr * sa + (bg[0] as u32) * inv + 127) / 255) as u8;
            out.data[idx + 1] = ((sg * sa + (bg[1] as u32) * inv + 127) / 255) as u8;
            out.data[idx + 2] = ((sb * sa + (bg[2] as u32) * inv + 127) / 255) as u8;
            out.data[idx + 3] = 255;
        }
    }
    Ok(out)
}

fn crop_impact_title_1to1(
    frame: &Rgba8Frame,
    project: &Project,
    target_w: u32,
    target_h: u32,
) -> Result<Rgba8Frame, CoreError> {
    let crop_w = target_w.min(frame.width).max(1);
    let crop_h = target_h.min(frame.height).max(1);
    let x0 = (frame.width - crop_w) / 2;

    // Position vertically to cover both the center impact area and any VectorText title card.
    let title_y_norm = project.graph.nodes.iter().find_map(|n| match n.kind {
        NodeKind::VectorText { y_offset_norm, .. } => Some(y_offset_norm),
        _ => None,
    });
    let y_center = match title_y_norm {
        Some(yn) if yn > 0.55 => ((frame.height as f32) * ((0.5 + yn) * 0.5)).round() as u32,
        Some(yn) if yn < 0.45 => ((frame.height as f32) * ((0.5 + yn) * 0.5)).round() as u32,
        _ => frame.height / 2,
    };
    let y0 = y_center
        .saturating_sub(crop_h / 2)
        .min(frame.height.saturating_sub(crop_h));

    let mut data = Vec::with_capacity((crop_w as usize) * (crop_h as usize) * 4);
    let row_stride = (frame.width as usize) * 4;
    let crop_row_bytes = (crop_w as usize) * 4;
    let x_byte = (x0 as usize) * 4;

    for row in 0..crop_h {
        let src_start = ((y0 + row) as usize) * row_stride + x_byte;
        data.extend_from_slice(&frame.data[src_start..src_start + crop_row_bytes]);
    }

    Ok(Rgba8Frame::new(
        crop_w,
        crop_h,
        frame.pts,
        frame.alpha_mode,
        data,
    )?)
}

fn fill_rect_u8(canvas: &mut Rgba8Frame, x0: u32, y0: u32, w: u32, h: u32, rgba: [u8; 4]) {
    let x1 = x0.saturating_add(w).min(canvas.width);
    let y1 = y0.saturating_add(h).min(canvas.height);
    for y in y0..y1 {
        for x in x0..x1 {
            canvas.set_pixel(x, y, rgba);
        }
    }
}

fn blit_frame_with_border(
    canvas: &mut Rgba8Frame,
    src: &Rgba8Frame,
    dst_x: u32,
    dst_y: u32,
    border_rgba: [u8; 4],
) {
    if dst_x > 0 && dst_y > 0 {
        fill_rect_u8(
            canvas,
            dst_x - 1,
            dst_y - 1,
            src.width + 2,
            src.height + 2,
            border_rgba,
        );
    }
    let copy_w = src.width.min(canvas.width.saturating_sub(dst_x));
    let copy_h = src.height.min(canvas.height.saturating_sub(dst_y));
    let dst_stride = (canvas.width as usize) * 4;
    let src_stride = (src.width as usize) * 4;
    let row_bytes = (copy_w as usize) * 4;

    for y in 0..copy_h {
        let s_off = (y as usize) * src_stride;
        let d_off = ((dst_y + y) as usize) * dst_stride + (dst_x as usize) * 4;
        canvas.data[d_off..d_off + row_bytes].copy_from_slice(&src.data[s_off..s_off + row_bytes]);
    }
}
