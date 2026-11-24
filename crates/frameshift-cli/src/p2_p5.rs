use anyhow::{Context, Result, bail};
use frameshift_core::{
    AiManifestRunConfig, DslCompileOptions, Engine, LintOptions, NleExportOptions,
    NleImportOptions, PlanarTrackProjectOptions, PointTrackProjectOptions, SmartCacheConfig,
    SmartCacheMode, TrackPlanarOptions, TrackPointOptions, TrackerBindTarget,
    calculate_template_metadata, compile_motion_str, diff_frame_files, discover_ai_models,
    export_project_fcpxml_to_file, export_project_otio_to_file, import_project_fcpxml_from_file,
    import_project_otio_from_file, instantiate_parametric_template, lint_project,
    run_manifest_model, run_manifest_model_by_id,
};
use frameshift_media::{FfmpegBridge, Rgba8Frame};
use frameshift_schema::template_parametric::ParametricTemplateSpec;
use frameshift_schema::{NodeId, NodeKind, Project, RenderProfileKind};
use frameshift_time::{FPS_24, Flicks};
use serde::Serialize;
use serde_json::json;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NleFormat {
    Otio,
    Fcpxml,
}

impl NleFormat {
    pub(crate) fn parse(raw: &str) -> Result<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "otio" => Ok(Self::Otio),
            "fcpxml" => Ok(Self::Fcpxml),
            other => bail!("invalid NLE --format '{other}'; expected 'otio' or 'fcpxml'"),
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Otio => "otio",
            Self::Fcpxml => "fcpxml",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrackBindMode {
    Transform2D,
    Stabilize,
    Null3D,
    CornerPin,
}

impl TrackBindMode {
    pub(crate) fn parse_point(raw: &str) -> Result<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "transform2d" | "transform_2d" => Ok(Self::Transform2D),
            "stabilize" | "transform2d_stabilize" => Ok(Self::Stabilize),
            "null3d" | "null_3d" => Ok(Self::Null3D),
            other => bail!(
                "invalid track-point --bind-mode '{other}'; expected transform2d|stabilize|null3d"
            ),
        }
    }

    pub(crate) fn parse_planar(raw: &str) -> Result<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "corner_pin" | "cornerpin" => Ok(Self::CornerPin),
            "transform2d" | "transform_2d" => Ok(Self::Transform2D),
            "null3d" | "null_3d" => Ok(Self::Null3D),
            other => bail!(
                "invalid track-planar --bind-mode '{other}'; expected corner_pin|transform2d|null3d"
            ),
        }
    }

    pub(crate) fn to_tracker_target(self) -> TrackerBindTarget {
        match self {
            Self::Transform2D => TrackerBindTarget::Transform2D,
            Self::Stabilize => TrackerBindTarget::Transform2DStabilize,
            Self::Null3D => TrackerBindTarget::null3d(0.0),
            Self::CornerPin => TrackerBindTarget::CornerPin,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TrackPointCliOpts {
    pub(crate) project_path: PathBuf,
    pub(crate) asset: String,
    pub(crate) target_node: String,
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) start_tick: Option<crate::TimeSpec>,
    pub(crate) end_tick: Option<crate::TimeSpec>,
    pub(crate) bind_mode: TrackBindMode,
    pub(crate) out: Option<PathBuf>,
    pub(crate) json: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TrackPlanarCliOpts {
    pub(crate) project_path: PathBuf,
    pub(crate) asset: String,
    pub(crate) target_node: String,
    pub(crate) corners: [[f32; 2]; 4],
    pub(crate) start_tick: Option<crate::TimeSpec>,
    pub(crate) end_tick: Option<crate::TimeSpec>,
    pub(crate) bind_mode: TrackBindMode,
    pub(crate) out: Option<PathBuf>,
    pub(crate) json: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AiRunCliOpts {
    pub(crate) model: String,
    pub(crate) input: PathBuf,
    pub(crate) out: PathBuf,
    pub(crate) models_dir: PathBuf,
    pub(crate) allow_unverified: bool,
    pub(crate) synthetic_test: bool,
    pub(crate) json: bool,
}

pub(crate) fn build_engine(cache_dir: Option<&Path>) -> Result<Engine> {
    if let Some(dir) = cache_dir {
        let cfg = SmartCacheConfig {
            mode: SmartCacheMode::MemoryAndDisk,
            cache_dir: Some(dir.to_path_buf()),
            ..SmartCacheConfig::default()
        };
        Ok(Engine::new_headless_with_smart_cache(cfg)?)
    } else {
        Ok(Engine::new_headless()?)
    }
}

pub(crate) fn parse_finite_f32(raw: &str, flag: &str) -> Result<f32> {
    let v: f32 = raw
        .trim()
        .parse()
        .with_context(|| format!("invalid float '{raw}' for {flag}"))?;
    if !v.is_finite() {
        bail!("{flag} must be finite, got '{raw}'");
    }
    Ok(v)
}

pub(crate) fn parse_threshold_f32_to_u8(raw: &str) -> Result<u8> {
    let v = parse_finite_f32(raw, "--threshold")?;
    if !(0.0..=255.0).contains(&v) {
        bail!("--threshold must be in 0..=255, got {v}");
    }
    Ok(v.round() as u8)
}

pub(crate) fn parse_corners_arg(raw: &str) -> Result<[[f32; 2]; 4]> {
    let parts: Vec<&str> = raw.split(',').map(str::trim).collect();
    if parts.len() != 8 {
        bail!(
            "invalid --corners '{raw}'; expected 8 comma-separated numbers x0,y0,x1,y1,x2,y2,x3,y3"
        );
    }
    let mut vals = [0.0f32; 8];
    for (idx, part) in parts.iter().enumerate() {
        vals[idx] = parse_finite_f32(part, "--corners")?;
    }
    Ok([
        [vals[0], vals[1]],
        [vals[2], vals[3]],
        [vals[4], vals[5]],
        [vals[6], vals[7]],
    ])
}

fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn write_project_json(path: &Path, project: &Project) -> Result<()> {
    ensure_parent_dir(path)?;
    std::fs::write(path, project.to_json_pretty()?)
        .with_context(|| format!("failed to write '{}'", path.display()))
}

fn print_json(value: &impl Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn resolve_source_node_for_asset(project: &Project, asset_or_node: &str) -> Result<NodeId> {
    let find_matching = |only_enabled: bool| -> Option<NodeId> {
        project.graph.nodes.iter().find_map(|n| {
            if only_enabled && !n.enabled {
                return None;
            }
            match &n.kind {
                NodeKind::SourceVideo { asset_id, .. }
                | NodeKind::SourceImage { asset_id, .. }
                    if asset_id == asset_or_node =>
                {
                    Some(n.id.clone())
                }
                _ => None,
            }
        })
    };
    if let Some(id) = find_matching(true).or_else(|| find_matching(false)) {
        return Ok(id);
    }
    let as_node = NodeId::new(asset_or_node);
    if project.graph.node(&as_node).is_some() {
        return Ok(as_node);
    }
    bail!(
        "no SourceVideo/SourceImage node references asset '{asset_or_node}', and no root graph node has id '{asset_or_node}'"
    )
}

fn resolve_track_window(
    project: &Project,
    start: Option<crate::TimeSpec>,
    end: Option<crate::TimeSpec>,
) -> Result<(Flicks, Flicks)> {
    let fps = project.timeline.fps;
    let start_tick = match start {
        Some(spec) => crate::resolve_time_spec(spec, fps)?,
        None => Flicks::ZERO,
    };
    let end_tick = match end {
        Some(spec) => crate::resolve_time_spec(spec, fps)?,
        None => project.timeline.duration_ticks,
    };
    Ok((start_tick, end_tick))
}

pub(crate) fn run_lint(project_path: &Path, check_media: bool, json_out: bool) -> Result<()> {
    let project = crate::load_project(project_path)?;
    let report = lint_project(
        &project,
        &LintOptions {
            check_media,
            ..LintOptions::default()
        },
    );
    if json_out {
        print_json(&report)?;
    } else {
        println!(
            "Lint: ok={} errors={} warnings={} info={}",
            report.ok, report.error_count, report.warning_count, report.info_count
        );
        for f in &report.findings {
            let loc = f.node_id.as_deref().unwrap_or("-");
            println!(
                "  [{:?}] {} ({}): {} -> {}",
                f.severity, f.code, loc, f.message, f.suggestion
            );
        }
    }
    if !report.ok {
        bail!("lint failed with {} error(s)", report.error_count);
    }
    Ok(())
}

pub(crate) fn run_diff_frame(
    a: &Path,
    b: &Path,
    out: Option<&Path>,
    threshold: u8,
    json_out: bool,
) -> Result<()> {
    let report = diff_frame_files(a, b, threshold, out)?;
    if json_out {
        print_json(&report)?;
    } else {
        println!(
            "diff-frame ({}x{}): identical={} mae={:.4} rmse={:.4} psnr_db={:?} changed_pixels={} max_abs_diff={} bbox={:?}",
            report.width,
            report.height,
            report.identical,
            report.mae,
            report.rmse,
            report.psnr_db,
            report.changed_pixels,
            report.max_abs_diff,
            report.changed_bbox
        );
        if let Some(ref diff_path) = report.out_diff_png {
            println!("  wrote diff strip -> {diff_path}");
        }
    }
    Ok(())
}

pub(crate) fn run_compile_dsl(
    input: &Path,
    out: &Path,
    skip_lint: bool,
    json_out: bool,
) -> Result<()> {
    let raw = std::fs::read_to_string(input)
        .with_context(|| format!("failed to read Motion DSL '{}'", input.display()))?;
    let compiled = compile_motion_str(
        &raw,
        &DslCompileOptions {
            skip_lint,
            ..DslCompileOptions::default()
        },
    )?;
    write_project_json(out, &compiled.project)?;
    if json_out {
        print_json(&json!({
            "out": out,
            "project_name": compiled.project.name,
            "revision_id": compiled.project.revision_id,
            "nodes": compiled.project.graph.nodes.len(),
            "clips": compiled.project.timeline.clips.len(),
            "lint_report": compiled.lint_report,
        }))?;
    } else {
        println!(
            "Compiled Motion DSL '{}' -> {} (nodes={}, clips={}, lint_ok={}, warnings={})",
            input.display(),
            out.display(),
            compiled.project.graph.nodes.len(),
            compiled.project.timeline.clips.len(),
            compiled.lint_report.ok,
            compiled.lint_report.warning_count
        );
    }
    Ok(())
}

fn load_spec_and_props(
    spec_path: &Path,
    props_path: &Path,
) -> Result<(ParametricTemplateSpec, serde_json::Value)> {
    let spec_raw = std::fs::read_to_string(spec_path)
        .with_context(|| format!("failed to read template spec '{}'", spec_path.display()))?;
    let spec: ParametricTemplateSpec = serde_json::from_str(&spec_raw)
        .with_context(|| format!("invalid template spec JSON '{}'", spec_path.display()))?;
    let props_raw = std::fs::read_to_string(props_path)
        .with_context(|| format!("failed to read template props '{}'", props_path.display()))?;
    let props: serde_json::Value = serde_json::from_str(&props_raw)
        .with_context(|| format!("invalid template props JSON '{}'", props_path.display()))?;
    Ok((spec, props))
}

pub(crate) fn run_template_metadata(
    spec_path: &Path,
    props_path: &Path,
    json_out: bool,
) -> Result<()> {
    let (spec, props) = load_spec_and_props(spec_path, props_path)?;
    let meta = calculate_template_metadata(&spec, &props)?;
    if json_out {
        print_json(&meta)?;
    } else {
        println!(
            "Template '{}': {}x{} @ {} fps, duration={} flicks, cuts={}, drop_events={}",
            meta.template_id,
            meta.resolved_resolution.width,
            meta.resolved_resolution.height,
            meta.resolved_fps,
            meta.resolved_duration_ticks.0,
            meta.cut_count,
            meta.drop_events.len()
        );
    }
    Ok(())
}

pub(crate) fn run_template_instantiate(
    spec_path: &Path,
    props_path: &Path,
    out: &Path,
    json_out: bool,
) -> Result<()> {
    let (spec, props) = load_spec_and_props(spec_path, props_path)?;
    let (project, metadata, lint_report) = instantiate_parametric_template(&spec, &props)?;
    write_project_json(out, &project)?;
    if json_out {
        print_json(&json!({
            "out": out,
            "metadata": metadata,
            "lint_report": lint_report,
        }))?;
    } else {
        println!(
            "Instantiated template '{}' -> {} ({}x{}, cuts={}, lint_ok={})",
            metadata.template_id,
            out.display(),
            metadata.resolved_resolution.width,
            metadata.resolved_resolution.height,
            metadata.cut_count,
            lint_report.ok
        );
    }
    Ok(())
}

pub(crate) fn run_track_point(opts: TrackPointCliOpts) -> Result<()> {
    let mut project = crate::load_project(&opts.project_path)?;
    let source_node_id = resolve_source_node_for_asset(&project, &opts.asset)?;
    let (start_tick, end_tick) = resolve_track_window(&project, opts.start_tick, opts.end_tick)?;
    let mut engine = Engine::new_headless()?;
    let report = engine.track_point_to_project(
        &mut project,
        &PointTrackProjectOptions {
            source_node_id,
            target_node_id: NodeId::new(&opts.target_node),
            bind_target: opts.bind_mode.to_tracker_target(),
            track_id_prefix: format!("track_pt_{}", opts.target_node),
            start_tick,
            end_tick,
            start_point_uv: [opts.x, opts.y],
            point_options: TrackPointOptions::default(),
            profile: RenderProfileKind::Draft,
        },
    )?;
    let out_path = opts.out.as_deref().unwrap_or(&opts.project_path);
    write_project_json(out_path, &project)?;
    if opts.json {
        print_json(&json!({
            "out": out_path,
            "report": report,
        }))?;
    } else {
        println!(
            "Tracked point ({:.3}, {:.3}) across {} frames (mean_fb_err={:.3}px, min_conf={:.3}, tracks={}, rev={}) -> {}",
            opts.x,
            opts.y,
            report.frames_tracked,
            report.mean_fb_error_px,
            report.min_confidence,
            report.generated_tracks.len(),
            report.updated_revision_id,
            out_path.display()
        );
    }
    Ok(())
}

pub(crate) fn run_track_planar(opts: TrackPlanarCliOpts) -> Result<()> {
    let mut project = crate::load_project(&opts.project_path)?;
    let source_node_id = resolve_source_node_for_asset(&project, &opts.asset)?;
    let (start_tick, end_tick) = resolve_track_window(&project, opts.start_tick, opts.end_tick)?;
    let mut engine = Engine::new_headless()?;
    let report = engine.track_planar_to_project(
        &mut project,
        &PlanarTrackProjectOptions {
            source_node_id,
            target_node_id: NodeId::new(&opts.target_node),
            bind_target: opts.bind_mode.to_tracker_target(),
            track_id_prefix: format!("track_planar_{}", opts.target_node),
            start_tick,
            end_tick,
            start_corners_uv: opts.corners,
            planar_options: TrackPlanarOptions::default(),
            profile: RenderProfileKind::Draft,
        },
    )?;
    let out_path = opts.out.as_deref().unwrap_or(&opts.project_path);
    write_project_json(out_path, &project)?;
    if opts.json {
        print_json(&json!({
            "out": out_path,
            "report": report,
        }))?;
    } else {
        println!(
            "Tracked planar quad across {} frames (mean_fb_err={:.3}px, min_conf={:.3}, tracks={}, rev={}) -> {}",
            report.frames_tracked,
            report.mean_fb_error_px,
            report.min_confidence,
            report.generated_tracks.len(),
            report.updated_revision_id,
            out_path.display()
        );
    }
    Ok(())
}

pub(crate) fn run_export_nle(
    project_path: &Path,
    format: NleFormat,
    out: &Path,
    json_out: bool,
) -> Result<()> {
    let project = crate::load_project(project_path)?;
    let opts = NleExportOptions::default();
    match format {
        NleFormat::Otio => export_project_otio_to_file(&project, out, &opts)?,
        NleFormat::Fcpxml => export_project_fcpxml_to_file(&project, out, &opts)?,
    }
    if json_out {
        print_json(&json!({
            "format": format.as_str(),
            "out": out,
            "project_name": project.name,
            "clips": project.timeline.clips.len(),
            "events": project.events.len(),
        }))?;
    } else {
        println!(
            "Exported {} ({} clips) -> {}",
            format.as_str(),
            project.timeline.clips.len(),
            out.display()
        );
    }
    Ok(())
}

pub(crate) fn run_import_nle(
    input: &Path,
    format: NleFormat,
    out: &Path,
    json_out: bool,
) -> Result<()> {
    let opts = NleImportOptions::default();
    let project = match format {
        NleFormat::Otio => import_project_otio_from_file(input, &opts)?,
        NleFormat::Fcpxml => import_project_fcpxml_from_file(input, &opts)?,
    };
    write_project_json(out, &project)?;
    if json_out {
        print_json(&json!({
            "format": format.as_str(),
            "input": input,
            "out": out,
            "project_name": project.name,
            "clips": project.timeline.clips.len(),
            "duration_ticks": project.timeline.duration_ticks,
            "revision_id": project.revision_id,
        }))?;
    } else {
        println!(
            "Imported {} '{}' ({} clips) -> {}",
            format.as_str(),
            input.display(),
            project.timeline.clips.len(),
            out.display()
        );
    }
    Ok(())
}

pub(crate) fn run_ai_models(models_dir: &Path, json_out: bool) -> Result<()> {
    let models = discover_ai_models(models_dir)?;
    if json_out {
        print_json(&json!({
            "models_dir": models_dir,
            "count": models.len(),
            "models": models,
        }))?;
    } else if models.is_empty() {
        println!("No AI model manifests found under {}", models_dir.display());
    } else {
        println!("Discovered {} AI model manifest(s) under {}:", models.len(), models_dir.display());
        for m in &models {
            println!(
                "  - {} [task={}] verified={} onnx={}",
                m.manifest.id,
                m.manifest.task.as_str(),
                m.sha256_verified,
                m.onnx_path.display()
            );
            if let Some(err) = &m.verification_error {
                println!("    verification_error: {err}");
            }
        }
    }
    Ok(())
}

fn is_png_path(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
}

fn is_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "webp" | "bmp" | "tif" | "tiff"
            )
        })
}

pub(crate) fn run_ai_run(opts: AiRunCliOpts) -> Result<()> {
    let (frames, fps) = if is_png_path(&opts.input) {
        (vec![Rgba8Frame::load_png(&opts.input, Flicks::ZERO)?], FPS_24)
    } else if is_image_path(&opts.input) {
        let frame = FfmpegBridge::load_image_frame_with_fit(
            &opts.input,
            0,
            0,
            frameshift_media::SourceFit::Stretch,
        )?;
        (vec![frame], FPS_24)
    } else {
        let probe = FfmpegBridge::probe_media_info(&opts.input)?;
        let decoded = FfmpegBridge::decode_video_window(
            &opts.input,
            &probe.pts_index,
            0,
            probe.pts_index.len(),
            probe.width,
            probe.height,
        )?;
        let frames: Vec<Rgba8Frame> = decoded.into_iter().map(|(_, f)| f).collect();
        (frames, probe.fps.unwrap_or(FPS_24))
    };

    let cfg = AiManifestRunConfig {
        allow_unverified: opts.allow_unverified,
        synthetic_test: opts.synthetic_test,
        ..AiManifestRunConfig::default()
    };

    let model_path = Path::new(&opts.model);
    let (out_frames, report) = if model_path.is_file()
        && model_path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("json"))
    {
        run_manifest_model(&frames, model_path, &cfg)?
    } else {
        run_manifest_model_by_id(&frames, &opts.models_dir, &opts.model, &cfg)?
    };

    ensure_parent_dir(&opts.out)?;
    if out_frames.len() == 1 && is_png_path(&opts.out) {
        out_frames[0].save_png(&opts.out)?;
    } else {
        FfmpegBridge::encode_mp4(&out_frames, None, fps, &opts.out)?;
    }

    if opts.json {
        print_json(&json!({
            "out_path": opts.out,
            "report": report,
        }))?;
    } else {
        println!(
            "ai-run '{}' [{}]: {} frames ({}x{} -> {}x{}, sha256_verified={}) -> {}",
            report.model_id,
            report.task.as_str(),
            report.frame_count,
            report.input_width,
            report.input_height,
            report.output_width,
            report.output_height,
            report.sha256_verified,
            opts.out.display()
        );
    }
    Ok(())
}
