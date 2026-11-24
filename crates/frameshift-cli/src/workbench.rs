use crate::{
    Engine, Project, ProjectSession, RenderProfileKind, load_project, parse_time_spec,
    resolve_time_spec,
};
use anyhow::{Context, Result, bail};
use frameshift_app::{
    AgentActor, PreviewArtifactState, TimelineProjection, apply_agent_command, snap_tick,
    source_output_speed_graph,
};
use frameshift_core::export::{ExportJobState, ExportQueue, ExportTarget};
use frameshift_core::workbench::{
    PreflightReport, apply_effect, effect_presets, impact_cut_project, inspect_effect, preflight,
};
use frameshift_media::{
    AiInferenceConfig, AiTaskKind, DepthNormMode, DepthOutputFormat, ExportEncodingProfile,
    FfmpegBridge, InterpolateConfig, InterpolatorBackend, JapaneseTokenizer, LyricAlignBackend,
    LyricAlignRequest, LyricNodeStyle, LyricTimelineTemplateConfig, ProxyManager, ProxySpec,
    RealEsrganModel, StemBackend, StemSeparationRequest, UpscaleBackend, UpscaleConfig,
    align_audio_lyrics, build_lyric_timeline_nodes, detect_ai_audio_capabilities,
    detect_ai_capabilities, detect_realesrgan_capabilities, interpolate_video_file,
    run_ai_depth_png16_sequence, run_ai_video_inference, separate_audio_stems, upscale_video_file,
};
use frameshift_schema::portable::{ProjectMediaManifest, relink_missing_media_with_hash};
use frameshift_schema::template::{
    TEMPLATE_PACKAGE_VERSION, TemplatePackage, TemplateReview, TemplateSlot, TemplateSlotKind,
    TemplateSlotTarget, TemplateSlotValue,
};
use frameshift_schema::{AssetKind, NodeId, NodeKind, ProjectCommand, RenderGraphDef};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::{Duration, Instant};

pub fn dispatch(args: &[String]) -> Option<Result<()>> {
    let (command, args) = args.split_first()?;
    Some(match command.as_str() {
        "init-impact" => init_impact(args),
        "preflight" => check_project(args),
        "effect-presets" if args.is_empty() => print_json(&effect_presets()),
        "effect-inspect" => inspect(args),
        "effect-apply" => add(args),
        "effect-set-enabled" => set_enabled(args),
        "proxy" => proxy(args),
        "proxy-preview" => proxy_preview(args),
        "preview-benchmark" => benchmark(args),
        "timeline" => timeline(args),
        "speed-graph" => speed_graph(args),
        "snap" => snap(args),
        "edit-run" => edit_run(args),
        "mcp-stdio" if !args.is_empty() => mcp(args),
        "template-validate" => template_validate(args),
        "template-open" => template_open(args),
        "init-impact-template" => init_impact_template(args),
        "media-manifest" => media_manifest(args),
        "media-relink" => media_relink(args),
        "retarget" => retarget(args),
        "export" => export(args),
        "export-set" => export_set(args),
        "ai-status" if args.is_empty() => ai_status_cli(),
        "ai-stems" => ai_stems_cli(args),
        "ai-lyrics" => ai_lyrics_cli(args),
        "upscale" => upscale_cli(args),
        "interpolate" => interpolate_cli(args),
        "ai-matte" => ai_matte_cli(args),
        _ => return None,
    })
}

pub fn print_usage() {
    println!(
        "\
         \tframeshift-cli init-impact --out <project.json>\n\
         \tframeshift-cli preflight <project.json> [--skip-tools]\n\
         \tframeshift-cli effect-presets\n\
         \tframeshift-cli effect-inspect <project.json> <node-id> <time>\n\
         \tframeshift-cli effect-apply <project.json> <preset> <node-id> <revision> --out <project.json>\n\
         \tframeshift-cli effect-set-enabled <project.json> <node-id> <true|false> <revision> --out <project.json>\n\
         \tframeshift-cli proxy <status|build> <project.json> <asset-id> <cache-dir>\n\
         \tframeshift-cli proxy-preview <project.json> <asset-id> <time> <cache-dir> --out <frame.png>\n\
         \tframeshift-cli preview-benchmark <project.json> <time> [<time> ...]\n\
         \tframeshift-cli timeline <project.json>\n\
         \tframeshift-cli speed-graph <project.json> <clip-id> <sample-count>\n\
         \tframeshift-cli snap <project.json> <time> <tolerance>\n\
         \tframeshift-cli edit-run <project.json> <actions.json> --out <project.json>\n\
         \tframeshift-cli mcp-stdio <project.json> [--out <project.json>]\n\
         \tframeshift-cli template-validate <package.json>\n\
         \tframeshift-cli template-open <package.json> <slots.json> --out <project.json>\n\
         \tframeshift-cli init-impact-template --out <package.json>\n\
         \tframeshift-cli media-manifest <project.json>\n\
         \tframeshift-cli media-relink <project.json> <search-root> <revision> --out <project.json> [<manifest.json>]\n\
         \tframeshift-cli retarget <project.json> <event-id> <time> <revision> --out <project.json>\n\
         \tframeshift-cli export <project.json> <out.mp4|out.mov> [--profile draft|final] [--range <start>..<end>] [--platform <tiktok_1080p60|reels_1080p30|youtube_1080p60|anime_10bit_hevc|prores_422hq>]\n\
         \tframeshift-cli export-set <project.json> <targets.json> --manifest <export.json> [--retry]\n\
         \tframeshift-cli ai-status\n\
         \tframeshift-cli ai-stems <audio_or_video> --out-dir <dir> [--stems 2|4] [--backend auto|audio-separator|demucs|mdx-onnx] [--model <name>] [--device auto|cuda|dml|cpu] [--format wav|flac|mp3] [--offline]\n\
         \tframeshift-cli ai-lyrics <audio> [--lyrics <file.txt> | --lyrics-text <text>] [--language <iso>] [--backend auto|stable-ts|faster-whisper] [--model <name>] [--ja-tokenizer fugashi|char|raw] [--out <lyrics.json>] [--nodes-out <nodes.json>]\n\
         \tframeshift-cli upscale <in.mp4> --out <out.mp4> [--scale 2|3|4] [--model realesr-animevideov3|realesrgan-x4plus-anime|realesrgan-x4plus] [--backend auto|realesrgan|lanczos|bilinear] [--gpu <id>] [--tile <u32>] [--dedup <f32>] [--scd <f32>] [--tta]\n\
         \tframeshift-cli interpolate <in.mp4> --out <out.mp4> --fps <num[/den]> [--backend auto|rife|minterpolate|blend] [--dedup <f32>] [--scd <f32>] [--rife-model <str>]\n\
         \tframeshift-cli ai-matte <in.mp4> [--out <out.mp4>] [--model <model.onnx>] [--task rotoscope|depth] [--smooth <f32>] [--provider auto|dml|cuda|cpu] [--depth-format u8|rg16|hevc10|png16] [--norm-mode ema|per_frame|global_pass] [--png16-dir <dir>] [--synthetic-test]"
    );
}

fn print_json(value: &impl Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn save_project(path: &Path, project: &Project) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, project.to_json_pretty()?)
        .with_context(|| format!("failed to write '{}'", path.display()))
}

fn init_impact(args: &[String]) -> Result<()> {
    let [flag, path] = args else {
        bail!("usage: init-impact --out <project.json>")
    };
    if flag != "--out" {
        bail!("usage: init-impact --out <project.json>");
    }
    save_project(Path::new(path), &impact_cut_project())
}

fn check_project(args: &[String]) -> Result<()> {
    let (path, check_tools) = match args {
        [path] => (path, true),
        [path, flag] if flag == "--skip-tools" => (path, false),
        _ => bail!("usage: preflight <project.json> [--skip-tools]"),
    };
    let report = match load_project(Path::new(path)) {
        Ok(project) => preflight(&project, check_tools),
        Err(error) => PreflightReport::invalid_project(format!("{error:#}")),
    };
    print_json(&report)?;
    if !report.ok {
        bail!("project preflight failed");
    }
    Ok(())
}

fn inspect(args: &[String]) -> Result<()> {
    let [path, id, time] = args else {
        bail!("usage: effect-inspect <project.json> <node-id> <time>")
    };
    let project = load_project(Path::new(path))?;
    let tick = resolve_time_spec(parse_time_spec(time)?, project.timeline.fps)?;
    let report = inspect_effect(&project, &NodeId::new(id), tick)?
        .with_context(|| format!("unknown node '{id}'"))?;
    print_json(&report)
}

fn add(args: &[String]) -> Result<()> {
    let [path, preset, id, revision, flag, out] = args else {
        bail!(
            "usage: effect-apply <project.json> <preset> <node-id> <revision> --out <project.json>"
        )
    };
    if flag != "--out" {
        bail!("expected --out <project.json>");
    }
    let project = load_project(Path::new(path))?;
    let command = apply_effect(&project, NodeId::new(id), preset)
        .with_context(|| format!("unknown effect preset '{preset}'"))?;
    commit(project, revision, command, out)
}

fn set_enabled(args: &[String]) -> Result<()> {
    let [path, id, enabled, revision, flag, out] = args else {
        bail!(
            "usage: effect-set-enabled <project.json> <node-id> <true|false> <revision> --out <project.json>"
        )
    };
    if flag != "--out" {
        bail!("expected --out <project.json>");
    }
    let project = load_project(Path::new(path))?;
    let mut node = project
        .graph
        .node(&NodeId::new(id))
        .with_context(|| format!("unknown node '{id}'"))?
        .clone();
    node.enabled = enabled.parse().context("enabled must be true or false")?;
    commit(
        project,
        revision,
        ProjectCommand::UpsertGraphNode(node),
        out,
    )
}

fn commit(project: Project, revision: &str, command: ProjectCommand, out: &str) -> Result<()> {
    let mut session = ProjectSession::new(project)?;
    let revision = session.apply_command(revision.parse()?, command)?;
    save_project(Path::new(out), session.project())?;
    print_json(&json!({ "revision_id": revision, "output": out }))
}

fn proxy(args: &[String]) -> Result<()> {
    let [action, path, id, cache] = args else {
        bail!("usage: proxy <status|build> <project.json> <asset-id> <cache-dir>")
    };
    let project = load_project(Path::new(path))?;
    let asset = project
        .asset(id)
        .with_context(|| format!("unknown asset '{id}'"))?;
    if asset.kind != AssetKind::Video {
        bail!("proxy management supports video assets only");
    }
    let source = Path::new(&asset.path);
    let spec = ProxySpec::new(
        project.proxy_config.scale_num,
        project.proxy_config.scale_den,
    )?;
    let manager = ProxyManager::new(cache);
    let artifact = match action.as_str() {
        "status" => manager.resolve_current_mp4_proxy(id, source, spec)?,
        "build" => Some(manager.resolve_or_generate_mp4_proxy(id, source, spec)?),
        _ => bail!("proxy action must be status or build"),
    };
    let exact_timestamps = artifact
        .as_ref()
        .map(|p| FfmpegBridge::probe_pts_index(&p.proxy_path).map(|pts| pts == p.pts_index))
        .transpose()?;
    print_json(&json!({
        "revision_id": project.revision_id,
        "asset_id": id,
        "configured_for_draft": project.proxy_config.enabled,
        "original_source": source,
        "proxy": artifact,
        "exact_pts_match": exact_timestamps,
        "default_render_source": "original",
    }))
}

/// Renders one explicit draft frame using a proxy after checking timing, aspect, opacity,
/// and matte use. The caller's project and normal preview/final paths remain untouched.
fn proxy_preview(args: &[String]) -> Result<()> {
    let [project_path, asset_id, at, cache_dir, flag, out] = args else {
        bail!("usage: proxy-preview <project.json> <asset-id> <time> <cache-dir> --out <frame.png>")
    };
    if flag != "--out" {
        bail!("expected --out <frame.png>");
    }

    let project = load_project(Path::new(project_path))?;
    let asset = project
        .asset(asset_id)
        .with_context(|| format!("unknown asset '{asset_id}'"))?;
    if asset.kind != AssetKind::Video {
        bail!("proxy preview supports video assets only");
    }
    if asset_is_matte_input(&project.graph, asset_id) {
        bail!("proxy preview is disabled for assets used as matte inputs");
    }
    let source_path = Path::new(&asset.path);
    if !source_path.is_file() {
        bail!(
            "proxy preview requires an on-disk video source: {}",
            source_path.display()
        );
    }

    // The MP4 proxy path is deliberately restricted to a format known to be opaque.
    // This avoids silently losing source alpha, which can affect compositing even when
    // the asset is not connected to an explicit MaskMatte node.
    let source_identity = FfmpegBridge::hash_file(source_path)?;
    let source_info = FfmpegBridge::probe_media_info(source_path)?;
    if source_info.pixel_format.as_deref() != Some("yuv420p") {
        bail!(
            "proxy preview requires an opaque yuv420p source; found {:?}",
            source_info.pixel_format
        );
    }

    let spec = ProxySpec::new(
        project.proxy_config.scale_num,
        project.proxy_config.scale_den,
    )?;
    let manager = ProxyManager::new(cache_dir);
    let artifact = manager.resolve_or_generate_mp4_proxy(asset_id, source_path, spec)?;
    if FfmpegBridge::hash_file(source_path)? != source_identity {
        bail!("proxy preview rejected: source changed during proxy resolution");
    }
    let proxy_path = artifact.proxy_path.canonicalize().with_context(|| {
        format!(
            "could not resolve proxy '{}'",
            artifact.proxy_path.display()
        )
    })?;
    let proxy_info = FfmpegBridge::probe_media_info(&proxy_path)?;
    if proxy_info.pixel_format.as_deref() != Some("yuv420p") {
        bail!(
            "proxy preview requires an opaque yuv420p proxy; found {:?}",
            proxy_info.pixel_format
        );
    }
    if source_info.pts_index != proxy_info.pts_index {
        bail!("proxy preview rejected: proxy PTS do not exactly match the source");
    }
    if source_info.width as u64 * proxy_info.height as u64
        != source_info.height as u64 * proxy_info.width as u64
    {
        bail!("proxy preview rejected: proxy aspect ratio differs from the source");
    }
    if artifact.width != proxy_info.width || artifact.height != proxy_info.height {
        bail!("proxy preview rejected: cached proxy dimensions do not match ffprobe");
    }
    let proxy_identity = FfmpegBridge::hash_file(&proxy_path)?;

    let mut proxy_project = project.clone();
    let pinned_asset = proxy_project
        .assets
        .iter_mut()
        .find(|asset| asset.id == *asset_id)
        .expect("asset was resolved from this project");
    pinned_asset.path = proxy_path.to_string_lossy().into_owned();

    let tick = resolve_time_spec(parse_time_spec(at)?, proxy_project.timeline.fps)?;
    let mut engine = Engine::new_headless()?;
    let frame = engine.render_frame_at(&proxy_project, RenderProfileKind::Draft, tick)?;
    if FfmpegBridge::hash_file(&proxy_path)? != proxy_identity {
        bail!("proxy preview rejected: proxy changed while rendering");
    }
    let out_path = Path::new(out);
    if let Some(parent) = out_path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    frame.save_png(out_path)?;
    print_json(&json!({
        "revision_id": project.revision_id,
        "asset_id": asset_id,
        "profile": "draft",
        "output_tick": tick,
        "render_source": proxy_path,
        "source_dimensions": [source_info.width, source_info.height],
        "proxy_dimensions": [proxy_info.width, proxy_info.height],
        "source_pixel_format": source_info.pixel_format,
        "proxy_pixel_format": proxy_info.pixel_format,
        "exact_pts_match": true,
        "aspect_match": true,
        "matte_input": false,
        "fnv1a_hash64": frame.fnv1a_hash64(),
        "output": out_path,
    }))
}

fn asset_is_matte_input(graph: &RenderGraphDef, asset_id: &str) -> bool {
    for node in &graph.nodes {
        if matches!(
            &node.kind,
            NodeKind::MaskMatte { .. } | NodeKind::DepthMatte { .. }
        )
            && let Some(matte_input) = node.inputs.get(1)
            && input_uses_asset(graph, matte_input, asset_id, &mut BTreeSet::new())
        {
            return true;
        }
        if let NodeKind::Precomp { sub_graph, .. } = &node.kind
            && asset_is_matte_input(sub_graph, asset_id)
        {
            return true;
        }
    }
    false
}

fn input_uses_asset(
    graph: &RenderGraphDef,
    input: &NodeId,
    asset_id: &str,
    seen: &mut BTreeSet<NodeId>,
) -> bool {
    if !seen.insert(input.clone()) {
        return false;
    }
    let Some(node) = graph.nodes.iter().find(|node| &node.id == input) else {
        return false;
    };
    match &node.kind {
        NodeKind::SourceVideo {
            asset_id: source_id,
            ..
        }
        | NodeKind::SourceImage {
            asset_id: source_id,
            ..
        } if source_id == asset_id => true,
        NodeKind::Precomp { sub_graph, .. } if graph_uses_asset(sub_graph, asset_id) => true,
        _ => node
            .inputs
            .iter()
            .any(|dependency| input_uses_asset(graph, dependency, asset_id, seen)),
    }
}

fn graph_uses_asset(graph: &RenderGraphDef, asset_id: &str) -> bool {
    graph.nodes.iter().any(|node| match &node.kind {
        NodeKind::SourceVideo {
            asset_id: source_id,
            ..
        }
        | NodeKind::SourceImage {
            asset_id: source_id,
            ..
        } => source_id == asset_id,
        NodeKind::Precomp { sub_graph, .. } => graph_uses_asset(sub_graph, asset_id),
        _ => false,
    })
}

fn benchmark(args: &[String]) -> Result<()> {
    let [path, times @ ..] = args else {
        bail!("usage: preview-benchmark <project.json> <time> [<time> ...]")
    };
    if times.is_empty() {
        bail!("provide at least one preview time");
    }
    let project = load_project(Path::new(path))?;
    let ticks = times
        .iter()
        .map(|time| resolve_time_spec(parse_time_spec(time)?, project.timeline.fps))
        .collect::<Result<Vec<_>>>()?;
    let started = Instant::now();
    let mut engine = Engine::new_headless()?;
    let init = started.elapsed();
    let mut cold = Vec::new();
    let mut warm = Vec::new();
    for tick in &ticks {
        engine.clear_cache();
        let started = Instant::now();
        engine.render_frame_at(&project, RenderProfileKind::Draft, *tick)?;
        cold.push(started.elapsed());
        let started = Instant::now();
        engine.render_frame_at(&project, RenderProfileKind::Draft, *tick)?;
        warm.push(started.elapsed());
    }
    let cache = engine.cache_telemetry();
    print_json(&json!({
        "revision_id": project.revision_id,
        "output_ticks": ticks,
        "initialization_ms": init.as_secs_f64() * 1000.0,
        "first_cold_frame_ms": cold[0].as_secs_f64() * 1000.0,
        "cold": timings(cold),
        "warm": timings(warm),
        "last_frame_cache": {
            "hits": cache.hits, "misses": cache.misses, "evictions": cache.evictions,
            "entries": cache.current_entries, "capacity": cache.capacity, "bytes": cache.current_bytes,
        },
    }))
}

fn timings(mut samples: Vec<Duration>) -> serde_json::Value {
    samples.sort_unstable();
    let ms = |percentile: usize| {
        samples[(samples.len() * percentile).div_ceil(100) - 1].as_secs_f64() * 1000.0
    };
    json!({ "p50_ms": ms(50), "p95_ms": ms(95), "sample_count": samples.len() })
}

fn timeline(args: &[String]) -> Result<()> {
    let [path] = args else {
        bail!("usage: timeline <project.json>")
    };
    let project = load_project(Path::new(path))?;
    print_json(&TimelineProjection::from_project(&project))
}

fn speed_graph(args: &[String]) -> Result<()> {
    let [path, id, sample_count] = args else {
        bail!("usage: speed-graph <project.json> <clip-id> <sample-count>")
    };
    let project = load_project(Path::new(path))?;
    let clip = project
        .clip(id)
        .with_context(|| format!("unknown clip '{id}'"))?;
    let sample_count: usize = sample_count.parse().context("invalid sample count")?;
    if sample_count > 100_000 {
        bail!("sample count must be at most 100000");
    }
    print_json(&json!({
        "revision_id": project.revision_id, "clip_id": id, "event_id": clip.linked_event_id,
        "points": source_output_speed_graph(clip, sample_count)?,
    }))
}

fn snap(args: &[String]) -> Result<()> {
    let [path, time, tolerance] = args else {
        bail!("usage: snap <project.json> <time> <tolerance>")
    };
    let project = load_project(Path::new(path))?;
    let projection = TimelineProjection::from_project(&project);
    let tick = resolve_time_spec(parse_time_spec(time)?, projection.fps)?;
    let tolerance = resolve_time_spec(parse_time_spec(tolerance)?, projection.fps)?;
    let detected_beats = frameshift_core::MediaResolver::new()
        .mix_project_audio(&project)?
        .map(|audio| {
            audio.analyze_beats_and_energy(frameshift_time::Flicks(
                frameshift_time::FLICKS_PER_SECOND / 100,
            ))
        })
        .transpose()?
        .map_or_else(Vec::new, |report| report.detected_beats);
    print_json(&snap_tick(
        tick,
        projection.fps,
        &detected_beats,
        &projection.event_ticks(),
        tolerance,
    )?)
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum EditStep {
    Human {
        expected_revision_id: u64,
        command: ProjectCommand,
    },
    Agent {
        expected_revision_id: u64,
        agent_id: String,
        intent: String,
        command: ProjectCommand,
        preview_at: Option<String>,
        preview_range: Option<String>,
    },
    Undo {
        expected_revision_id: u64,
    },
    Redo {
        expected_revision_id: u64,
    },
    EnqueueExport {
        expected_revision_id: u64,
        targets: Vec<ExportTarget>,
    },
}

fn edit_run(args: &[String]) -> Result<()> {
    let [path, actions, flag, out] = args else {
        bail!("usage: edit-run <project.json> <actions.json> --out <project.json>")
    };
    if flag != "--out" {
        bail!("expected --out <project.json>");
    }
    let project = load_project(Path::new(path))?;
    let mut session = ProjectSession::new(project)?;
    let steps: Vec<EditStep> = serde_json::from_slice(&std::fs::read(actions)?)?;
    let mut history = Vec::with_capacity(steps.len());
    let mut exports = ExportQueue::default();
    for (index, step) in steps.into_iter().enumerate() {
        let result = (|| -> Result<_> {
            Ok(match step {
                EditStep::Human {
                    expected_revision_id,
                    command,
                } => {
                    let revision = session.apply_command(expected_revision_id, command)?;
                    json!({"operation":"human", "revision_id":revision})
                }
                EditStep::Agent {
                    expected_revision_id,
                    agent_id,
                    intent,
                    command,
                    preview_at,
                    preview_range,
                } => {
                    let needs_preview = preview_at.is_some() || preview_range.is_some();
                    let before = needs_preview.then(|| session.project().clone());
                    let mut receipt = apply_agent_command(
                        &mut session,
                        expected_revision_id,
                        AgentActor { agent_id },
                        intent,
                        command,
                    )?;
                    if let Some(before) = before {
                        let dir = Path::new(out)
                            .with_extension("edit-previews")
                            .join(format!("step-{:04}", index + 1));
                        let render = |at: &str, is_video| -> Result<PreviewArtifactState> {
                            let mut engine = Engine::new_headless()?;
                            std::fs::create_dir_all(&dir)?;
                            let ext = if is_video { "mp4" } else { "png" };
                            let before_path = dir.join(format!("before.{ext}"));
                            let after_path = dir.join(format!("after.{ext}"));
                            for (project, path) in
                                [(&before, &before_path), (session.project(), &after_path)]
                            {
                                let fps = project.timeline.fps;
                                if is_video {
                                    let range = crate::parse_range_spec(at)?;
                                    let start = resolve_time_spec(range.start, fps)?;
                                    let end = resolve_time_spec(range.end, fps)?;
                                    let range =
                                        frameshift_core::RenderRange::resolve(project, start, end)?;
                                    engine.render_video_mp4_range(
                                        project,
                                        RenderProfileKind::Draft,
                                        path,
                                        range,
                                    )?;
                                } else {
                                    let tick = resolve_time_spec(parse_time_spec(at)?, fps)?;
                                    engine
                                        .render_frame_at(project, RenderProfileKind::Draft, tick)?
                                        .save_png(path)?;
                                }
                                engine.clear_cache();
                            }
                            Ok(PreviewArtifactState::Generated {
                                before_path,
                                after_path,
                            })
                        };
                        if let Some(at) = preview_at {
                            receipt.preview.visual_artifact =
                                render(&at, false).unwrap_or_else(|error| {
                                    PreviewArtifactState::NotGenerated {
                                        reason: format!("{error:#}"),
                                    }
                                });
                        }
                        if let Some(range) = preview_range {
                            receipt.preview.audio_artifact =
                                render(&range, true).unwrap_or_else(|error| {
                                    PreviewArtifactState::NotGenerated {
                                        reason: format!("{error:#}"),
                                    }
                                });
                        }
                    }
                    json!({"operation":"agent", "receipt":receipt})
                }
                EditStep::Undo {
                    expected_revision_id,
                } => {
                    let revision = session.undo(expected_revision_id)?;
                    json!({"operation":"undo", "revision_id":revision})
                }
                EditStep::Redo {
                    expected_revision_id,
                } => {
                    let revision = session.redo(expected_revision_id)?;
                    json!({"operation":"redo", "revision_id":revision})
                }
                EditStep::EnqueueExport {
                    expected_revision_id,
                    targets,
                } => {
                    ensure_revision(expected_revision_id, session.revision_id())?;
                    let ticket = exports.enqueue(
                        session.project(),
                        project_dir(Path::new(path)),
                        targets,
                    )?;
                    json!({"operation":"enqueue_export", "job_id":ticket.job_id, "revision_id":expected_revision_id})
                }
            })
        })();
        history.push(result.with_context(|| format!("edit step {} failed", index + 1))?);
    }
    if !exports.list().is_empty() {
        let mut engine = Engine::new_headless()?;
        while let Some(job) = exports.run_next(&mut engine) {
            history.push(json!({"operation":"export_result", "job":job}));
        }
    }
    save_project(Path::new(out), session.project())?;
    let audit = Path::new(out).with_extension("edits.json");
    std::fs::write(&audit, serde_json::to_vec_pretty(&history)?)?;
    print_json(
        &json!({ "revision_id": session.revision_id(), "output": out, "edit_log": audit, "history": history }),
    )
}

fn mcp(args: &[String]) -> Result<()> {
    let (path, out) = match args {
        [path] => (path, None),
        [path, flag, out] if flag == "--out" => (path, Some(out)),
        _ => bail!("usage: mcp-stdio <project.json> [--out <project.json>]"),
    };
    let mut server = frameshift_mcp::McpServer::new(load_project(Path::new(path))?)?;
    server.run_stdio()?;
    if let Some(out) = out {
        save_project(Path::new(out), server.project())?;
        let audit = json!({
            "edit_receipts": server.list_edit_receipts(),
            "review_notes": server.list_review_notes(),
        });
        std::fs::write(
            Path::new(out).with_extension("edits.json"),
            serde_json::to_vec_pretty(&audit)?,
        )?;
    }
    Ok(())
}

fn template_validate(args: &[String]) -> Result<()> {
    let [path] = args else {
        bail!("usage: template-validate <package.json>")
    };
    let package = frameshift_schema::template::TemplatePackage::load_local(path)?;
    print_json(&json!({
        "template_id": package.template_id, "version": package.package_version,
        "review": package.review, "slots": package.slots, "manifest": package.manifest,
    }))
}

fn template_slot_kind_label(kind: TemplateSlotKind) -> &'static str {
    match kind {
        TemplateSlotKind::AssetPath(AssetKind::Video) => "video_asset",
        TemplateSlotKind::AssetPath(AssetKind::Image) => "image_asset",
        TemplateSlotKind::AssetPath(AssetKind::Audio) => "audio_asset",
        TemplateSlotKind::Text => "text",
        TemplateSlotKind::Tick => "tick",
        TemplateSlotKind::Number => "number",
        TemplateSlotKind::Boolean => "boolean",
        TemplateSlotKind::Color => "color",
        TemplateSlotKind::Choice => "choice",
        TemplateSlotKind::SpeedCurve => "speed_curve",
        TemplateSlotKind::Font => "font",
    }
}

fn prepare_template_slot_values(
    package: &TemplatePackage,
    slots_path: &Path,
    values: &mut BTreeMap<String, TemplateSlotValue>,
) -> Result<()> {
    for (slot_id, value) in values.iter_mut() {
        let declared_kind = package
            .slots
            .iter()
            .find(|slot| &slot.id == slot_id)
            .map(|slot| template_slot_kind_label(slot.kind));
        match value {
            TemplateSlotValue::AssetPath(path) if !path.starts_with("synthetic://") => {
                let source = slots_path
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join(&*path);
                let absolute = source
                    .canonicalize()
                    .with_context(|| format!("missing slot asset '{path}'"))?;
                if !absolute.is_file() {
                    bail!("slot asset '{}' must be a file", absolute.display());
                }
                *path = absolute.to_string_lossy().into_owned();
            }
            TemplateSlotValue::AssetPath(_)
            | TemplateSlotValue::Text(_)
            | TemplateSlotValue::Tick(_)
            | TemplateSlotValue::Boolean(_)
            | TemplateSlotValue::SpeedCurve(_) => {}
            TemplateSlotValue::Number(num) if !num.is_finite() => {
                bail!(
                    "slot '{slot_id}' ({}) requires a finite number",
                    declared_kind.unwrap_or("number")
                );
            }
            TemplateSlotValue::Number(_) => {}
            TemplateSlotValue::Color(rgba)
                if !rgba.iter().all(|c| c.is_finite() && *c >= 0.0) || rgba[3] > 1.0 =>
            {
                bail!(
                    "slot '{slot_id}' ({}) color channels must be finite >= 0.0 with alpha <= 1.0",
                    declared_kind.unwrap_or("color")
                );
            }
            TemplateSlotValue::Color(_) => {}
            TemplateSlotValue::Choice(choice) if choice.trim().is_empty() => {
                bail!(
                    "slot '{slot_id}' ({}) choice must not be empty",
                    declared_kind.unwrap_or("choice")
                );
            }
            TemplateSlotValue::Choice(_) => {}
            TemplateSlotValue::Font(font) if font.trim().is_empty() => {
                bail!(
                    "slot '{slot_id}' ({}) font value must not be empty",
                    declared_kind.unwrap_or("font")
                );
            }
            TemplateSlotValue::Font(_) => {}
        }
    }
    Ok(())
}

fn template_open(args: &[String]) -> Result<()> {
    let [path, slots, flag, out] = args else {
        bail!("usage: template-open <package.json> <slots.json> --out <project.json>")
    };
    if flag != "--out" {
        bail!("expected --out <project.json>");
    }
    let package = frameshift_schema::template::TemplatePackage::load_local(path)?;
    let mut values: BTreeMap<String, TemplateSlotValue> =
        serde_json::from_slice(&std::fs::read(slots)?)?;
    prepare_template_slot_values(&package, Path::new(slots), &mut values)?;
    let project = package.instantiate(&values)?;
    let report = preflight(&project, false);
    if !report.ok {
        print_json(&report)?;
        bail!("template instance failed preflight");
    }
    save_project(Path::new(out), &project)?;
    print_json(
        &json!({ "template_id": package.template_id, "output": out, "revision_id": project.revision_id }),
    )
}

fn init_impact_template(args: &[String]) -> Result<()> {
    let [flag, out] = args else {
        bail!("usage: init-impact-template --out <package.json>")
    };
    if flag != "--out" {
        bail!("expected --out <package.json>");
    }
    let project = impact_cut_project();
    let mut slots: Vec<_> = project
        .assets
        .iter()
        .map(|asset| TemplateSlot {
            id: asset.id.clone(),
            label: asset.id.clone(),
            kind: TemplateSlotKind::AssetPath(asset.kind),
            target: TemplateSlotTarget::AssetPath {
                asset_id: asset.id.clone(),
            },
            required: true,
            default: Some(TemplateSlotValue::AssetPath(asset.path.clone())),
        })
        .collect();
    slots.push(TemplateSlot {
        id: "title".into(),
        label: "Impact title".into(),
        kind: TemplateSlotKind::Text,
        target: TemplateSlotTarget::NodeText {
            node_id: NodeId::from("impact_title"),
        },
        required: true,
        default: Some(TemplateSlotValue::Text("IMPACT CUT".into())),
    });
    let package = TemplatePackage {
        package_version: TEMPLATE_PACKAGE_VERSION,
        template_id: "impact-cut".into(),
        name: project.name.clone(),
        description: "Three-shot edit with synthetic source media.".into(),
        project,
        slots,
        manifest: Default::default(),
        review: TemplateReview {
            reviewed: true,
            reviewer: Some("FrameShift built-in".into()),
            note: None,
        },
    };
    package.validate()?;
    std::fs::write(out, package.to_json_pretty()?)?;
    Ok(())
}

fn media_manifest(args: &[String]) -> Result<()> {
    let [path] = args else {
        bail!("usage: media-manifest <project.json>")
    };
    let project = Project::from_json_str(&std::fs::read_to_string(path)?)?;
    print_json(&ProjectMediaManifest::from_project_with_hash(
        &project, path, hash_media,
    ))
}

fn hash_media(path: &Path) -> Option<String> {
    FfmpegBridge::hash_file(path)
        .ok()
        .map(|identity| identity.sha256)
}

fn media_relink(args: &[String]) -> Result<()> {
    let (path, root, revision, out, manifest) = match args {
        [path, root, revision, flag, out] if flag == "--out" => (path, root, revision, out, None),
        [path, root, revision, flag, out, manifest] if flag == "--out" => {
            let manifest = ProjectMediaManifest::from_json_str(&std::fs::read_to_string(manifest)?)
                .map_err(anyhow::Error::msg)?;
            (path, root, revision, out, Some(manifest))
        }
        _ => bail!(
            "usage: media-relink <project.json> <search-root> <revision> --out <project.json> [<manifest.json>]"
        ),
    };
    let project = load_project(Path::new(path))?;
    let mut candidate = project.clone();
    let project_dir = Path::new(path).parent().unwrap_or_else(|| Path::new("."));
    let report = relink_missing_media_with_hash(
        &mut candidate,
        project_dir,
        &[Path::new(root).canonicalize()?],
        manifest.as_ref(),
        hash_media,
    );
    if report.relinked.is_empty() {
        print_json(&report)?;
        return Ok(());
    }
    let mut session = ProjectSession::new(project)?;
    let revision_id = session.apply_command(
        revision.parse()?,
        ProjectCommand::ReplaceProject { project: candidate },
    )?;
    save_project(Path::new(out), session.project())?;
    print_json(&json!({ "output": out, "revision_id": revision_id, "relink": report }))
}

fn retarget(args: &[String]) -> Result<()> {
    let [path, event_id, at, revision, flag, out] = args else {
        bail!("usage: retarget <project.json> <event-id> <time> <revision> --out <project.json>")
    };
    if flag != "--out" {
        bail!("expected --out <project.json>");
    }
    let project = load_project(Path::new(path))?;
    let event = project
        .event(event_id)
        .with_context(|| format!("unknown event '{event_id}'"))?;
    let tick = resolve_time_spec(parse_time_spec(at)?, project.timeline.fps)?;
    let (source_bounds_start, source_bounds_end) = event
        .source_clip_id
        .as_deref()
        .and_then(|id| project.clip(id))
        .map_or(
            (frameshift_time::Flicks::ZERO, frameshift_time::Flicks::ZERO),
            |clip| {
                (
                    clip.source_start,
                    clip.source_start.saturating_add(
                        clip.speed_curve
                            .source_offset_at(clip.timeline_end - clip.timeline_start),
                    ),
                )
            },
        );
    let command = ProjectCommand::RetargetEvent {
        event_id: event_id.clone(),
        new_target_output_tick: tick,
        source_bounds_start,
        source_bounds_end,
        min_speed: 0.05,
        max_speed: 8.0,
        curve_style: frameshift_time::ImpactCurveStyle::TwoSegmentConstant,
        source_fit: frameshift_time::SourceFitMode::ExactRange,
    };
    commit(project, revision, command, out)
}

fn project_dir(path: &Path) -> &Path {
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn ensure_revision(expected: u64, actual: u64) -> Result<()> {
    if expected != actual {
        bail!("revision conflict: expected {expected}, actual {actual}");
    }
    Ok(())
}

fn parse_export_target(project: &Project, out: &str, flags: &[String]) -> Result<ExportTarget> {
    let mut target = ExportTarget::new(out, RenderProfileKind::Final);
    let mut iter = flags.iter();
    while let Some(flag) = iter.next() {
        let (name, inline_val) = match flag.split_once('=') {
            Some((k, v)) => (k, Some(v)),
            None => (flag.as_str(), None),
        };
        let value = match inline_val {
            Some(v) => v,
            None => iter
                .next()
                .with_context(|| format!("{name} needs a value"))?
                .as_str(),
        };
        match name {
            "--profile" => target.profile = crate::parse_profile(value)?,
            "--range" => {
                let spec = crate::parse_range_spec(value)?;
                let fps = project.timeline.fps;
                target.range = Some(frameshift_core::RenderRange::resolve(
                    project,
                    resolve_time_spec(spec.start, fps)?,
                    resolve_time_spec(spec.end, fps)?,
                )?);
            }
            "--platform" => {
                if ExportEncodingProfile::from_platform_name(value).is_none() {
                    bail!(
                        "unknown export platform '{value}'; expected one of tiktok_1080p60, reels_1080p30, youtube_1080p60, anime_10bit_hevc, prores_422hq"
                    );
                }
                target.platform_preset = Some(value.to_owned());
            }
            _ => bail!("unknown export flag {flag}"),
        }
    }
    Ok(target)
}

fn export(args: &[String]) -> Result<()> {
    let [project_path, out, flags @ ..] = args else {
        bail!(
            "usage: export <project.json> <out.mp4|out.mov> [--profile draft|final] [--range <start>..<end>] [--platform <tiktok_1080p60|reels_1080p30|youtube_1080p60|anime_10bit_hevc|prores_422hq>]"
        )
    };
    let project = load_project(Path::new(project_path))?;
    let target = parse_export_target(&project, out, flags)?;
    let manifest = Path::new(out).with_extension("export.json");
    run_export(
        &project,
        project_dir(Path::new(project_path)),
        vec![target],
        &manifest,
        false,
    )
}

fn export_set(args: &[String]) -> Result<()> {
    let (project_path, targets, manifest, retry) = match args {
        [path, targets, flag, manifest] if flag == "--manifest" => (path, targets, manifest, false),
        [path, targets, flag, manifest, retry] if flag == "--manifest" && retry == "--retry" => {
            (path, targets, manifest, true)
        }
        _ => bail!(
            "usage: export-set <project.json> <targets.json> --manifest <export.json> [--retry]"
        ),
    };
    let project = load_project(Path::new(project_path))?;
    let targets = serde_json::from_slice(&std::fs::read(targets)?)?;
    run_export(
        &project,
        project_dir(Path::new(project_path)),
        targets,
        Path::new(manifest),
        retry,
    )
}

fn run_export(
    project: &Project,
    source_dir: &Path,
    targets: Vec<ExportTarget>,
    manifest: &Path,
    retry: bool,
) -> Result<()> {
    let mut queue = ExportQueue::default();
    let ticket = queue.enqueue(project, source_dir, targets)?;
    let mut engine = Engine::new_headless()?;
    let mut job = queue
        .run_next(&mut engine)
        .context("export queue has no pending job")?;
    if retry && job.state == ExportJobState::Failed {
        queue.retry(&ticket.job_id)?;
        job = queue
            .run_next(&mut engine)
            .context("export retry did not queue a job")?;
    }
    std::fs::create_dir_all(project_dir(manifest))?;
    std::fs::write(manifest, serde_json::to_vec_pretty(&job)?)?;
    print_json(&job)?;
    if job.state != ExportJobState::Succeeded {
        bail!("export {} failed; see {}", job.job_id, manifest.display());
    }
    Ok(())
}

fn parse_fps_arg(raw: &str) -> Result<frameshift_time::Rational32> {
    if let Some((n, d)) = raw.split_once('/') {
        let num: u32 = n.trim().parse().context("invalid fps numerator")?;
        let den: u32 = d.trim().parse().context("invalid fps denominator")?;
        if num == 0 || den == 0 {
            bail!("fps numerator and denominator must be > 0");
        }
        Ok(frameshift_time::Rational32 { num, den })
    } else {
        let num: u32 = raw.trim().parse().context("invalid fps integer")?;
        if num == 0 {
            bail!("fps must be > 0");
        }
        Ok(frameshift_time::Rational32 { num, den: 1 })
    }
}

fn interpolate_cli(args: &[String]) -> Result<()> {
    let Some((in_path, flags)) = args.split_first() else {
        bail!(
            "usage: interpolate <in.mp4> --out <out.mp4> --fps <num[/den]> [--backend auto|rife|minterpolate|blend] [--dedup <f32>] [--scd <f32>] [--rife-model <str>]"
        );
    };
    let mut out_path: Option<String> = None;
    let mut cfg = InterpolateConfig::default();
    let mut iter = flags.iter();
    while let Some(flag) = iter.next() {
        match flag.as_str() {
            "--out" => {
                out_path = Some(iter.next().context("missing value for --out")?.clone());
            }
            "--fps" => {
                let raw = iter.next().context("missing value for --fps")?;
                cfg.target_fps = parse_fps_arg(raw)?;
            }
            "--backend" => {
                let raw = iter.next().context("missing value for --backend")?;
                cfg.backend = match raw.to_ascii_lowercase().as_str() {
                    "auto" => None,
                    "rife" | "rife_ncnn_cli" => Some(InterpolatorBackend::RifeNcnnCli),
                    "minterpolate" | "ffmpeg" | "ffmpeg_minterpolate" => {
                        Some(InterpolatorBackend::FfmpegMinterpolate)
                    }
                    "blend" | "frame_blend" => Some(InterpolatorBackend::FrameBlend),
                    other => bail!("unknown --backend '{other}'"),
                };
            }
            "--dedup" => {
                let raw = iter.next().context("missing value for --dedup")?;
                cfg.dedup_threshold = raw.parse().context("invalid --dedup threshold")?;
            }
            "--scd" => {
                let raw = iter.next().context("missing value for --scd")?;
                cfg.scene_cut_threshold = raw.parse().context("invalid --scd threshold")?;
            }
            "--rife-model" => {
                cfg.rife_model = iter.next().context("missing value for --rife-model")?.clone();
            }
            other => bail!("unknown interpolate flag '{other}'"),
        }
    }
    let out = out_path.context("missing required --out <out.mp4>")?;
    let report = interpolate_video_file(
        Path::new(in_path),
        Path::new(&out),
        &cfg,
        &ExportEncodingProfile::default(),
    )?;
    print_json(&report)
}

fn build_ai_status_report() -> serde_json::Value {
    let video_onnx = detect_ai_capabilities();
    let ai_audio = detect_ai_audio_capabilities();
    let realesrgan = detect_realesrgan_capabilities();
    let depth_model = Path::new("local_media/models/depth_anything_v2_small.onnx");
    let isnet_model = Path::new("local_media/models/isnetis.onnx");
    json!({
        "video_and_onnx": video_onnx,
        "ai_audio": ai_audio,
        "realesrgan": realesrgan,
        "local_models": {
            "depth_anything_v2_small": depth_model.is_file(),
            "depth_anything_v2_small_path": depth_model,
            "isnetis": isnet_model.is_file(),
            "isnetis_path": isnet_model,
        }
    })
}

fn ai_status_cli() -> Result<()> {
    print_json(&build_ai_status_report())
}

fn parse_ai_stems_args(args: &[String]) -> Result<StemSeparationRequest> {
    let Some((in_path, flags)) = args.split_first() else {
        bail!(
            "usage: ai-stems <audio_or_video> --out-dir <dir> [--stems 2|4] [--backend auto|audio-separator|demucs|mdx-onnx] [--model <name>] [--device auto|cuda|dml|cpu] [--format wav|flac|mp3] [--model-dir <dir>] [--offline]"
        );
    };
    let mut req = StemSeparationRequest {
        input_path: Path::new(in_path).to_path_buf(),
        ..StemSeparationRequest::default()
    };
    let mut out_dir_set = false;
    let mut iter = flags.iter();
    while let Some(flag) = iter.next() {
        let (name, inline_val) = match flag.split_once('=') {
            Some((k, v)) => (k, Some(v)),
            None => (flag.as_str(), None),
        };
        let next_val = |iter: &mut std::slice::Iter<'_, String>| -> Result<String> {
            match inline_val {
                Some(v) => Ok(v.to_owned()),
                None => iter
                    .next()
                    .cloned()
                    .with_context(|| format!("missing value for {name}")),
            }
        };
        match name {
            "--out-dir" => {
                req.out_dir = Path::new(&next_val(&mut iter)?).to_path_buf();
                out_dir_set = true;
            }
            "--stems" => {
                let n: u8 = next_val(&mut iter)?.parse().context("invalid --stems count")?;
                if n != 2 && n != 4 {
                    bail!("--stems must be 2 or 4 (got {n})");
                }
                req.stems = n;
            }
            "--backend" => {
                let raw = next_val(&mut iter)?;
                req.backend = match raw.to_ascii_lowercase().as_str() {
                    "auto" => StemBackend::Auto,
                    "audio-separator" | "audio_separator" => StemBackend::AudioSeparator,
                    "demucs" => StemBackend::Demucs,
                    "mdx-onnx" | "mdx_onnx" | "mdx" => StemBackend::MdxOnnx,
                    other => bail!("unknown ai-stems --backend '{other}'"),
                };
            }
            "--model" => {
                req.model = Some(next_val(&mut iter)?);
            }
            "--device" => {
                req.device = next_val(&mut iter)?;
            }
            "--format" => {
                req.output_format = next_val(&mut iter)?;
            }
            "--model-dir" => {
                req.model_dir = Some(Path::new(&next_val(&mut iter)?).to_path_buf());
            }
            "--offline" => {
                req.offline = true;
            }
            "--python" => {
                req.python_binary = Some(Path::new(&next_val(&mut iter)?).to_path_buf());
            }
            "--sidecar" => {
                req.sidecar_script = Some(Path::new(&next_val(&mut iter)?).to_path_buf());
            }
            other => bail!("unknown ai-stems flag '{other}'"),
        }
    }
    if !out_dir_set {
        bail!("ai-stems requires --out-dir <dir>");
    }
    Ok(req)
}

fn ai_stems_cli(args: &[String]) -> Result<()> {
    let req = parse_ai_stems_args(args)?;
    let result = separate_audio_stems(&req)?;
    print_json(&result)
}

struct AiLyricsCliOptions {
    request: LyricAlignRequest,
    out_json: Option<std::path::PathBuf>,
    nodes_out_json: Option<std::path::PathBuf>,
    template_config: LyricTimelineTemplateConfig,
}

fn parse_ai_lyrics_args(args: &[String]) -> Result<AiLyricsCliOptions> {
    let Some((in_path, flags)) = args.split_first() else {
        bail!(
            "usage: ai-lyrics <audio> [--lyrics <file.txt> | --lyrics-text <text>] [--language <iso>] [--backend auto|stable-ts|faster-whisper] [--model <name>] [--compute-type <str>] [--device auto|cuda|cpu] [--ja-tokenizer fugashi|char|raw] [--no-vad] [--offline] [--out <lyrics.json>] [--nodes-out <nodes.json>]"
        );
    };
    let mut req = LyricAlignRequest {
        input_audio: Path::new(in_path).to_path_buf(),
        ..LyricAlignRequest::default()
    };
    let mut out_json = None;
    let mut nodes_out_json = None;
    let mut template_config = LyricTimelineTemplateConfig::default();

    let mut iter = flags.iter();
    while let Some(flag) = iter.next() {
        let (name, inline_val) = match flag.split_once('=') {
            Some((k, v)) => (k, Some(v)),
            None => (flag.as_str(), None),
        };
        let next_val = |iter: &mut std::slice::Iter<'_, String>| -> Result<String> {
            match inline_val {
                Some(v) => Ok(v.to_owned()),
                None => iter
                    .next()
                    .cloned()
                    .with_context(|| format!("missing value for {name}")),
            }
        };
        match name {
            "--lyrics" => {
                req.lyrics_file = Some(Path::new(&next_val(&mut iter)?).to_path_buf());
            }
            "--lyrics-text" => {
                req.lyrics_text = Some(next_val(&mut iter)?);
            }
            "--language" => {
                req.language = Some(next_val(&mut iter)?);
            }
            "--backend" => {
                let raw = next_val(&mut iter)?;
                req.backend = match raw.to_ascii_lowercase().as_str() {
                    "auto" => LyricAlignBackend::Auto,
                    "stable-ts" | "stable_ts" => LyricAlignBackend::StableTs,
                    "faster-whisper" | "faster_whisper" => LyricAlignBackend::FasterWhisper,
                    other => bail!("unknown ai-lyrics --backend '{other}'"),
                };
            }
            "--model" => {
                req.model = next_val(&mut iter)?;
            }
            "--compute-type" => {
                req.compute_type = next_val(&mut iter)?;
            }
            "--device" => {
                req.device = next_val(&mut iter)?;
            }
            "--ja-tokenizer" => {
                let raw = next_val(&mut iter)?;
                req.ja_tokenizer = match raw.to_ascii_lowercase().as_str() {
                    "fugashi" => JapaneseTokenizer::Fugashi,
                    "char" => JapaneseTokenizer::Char,
                    "raw" => JapaneseTokenizer::Raw,
                    other => bail!("unknown --ja-tokenizer '{other}'"),
                };
            }
            "--no-vad" => {
                req.vad = false;
            }
            "--model-dir" => {
                req.model_dir = Some(Path::new(&next_val(&mut iter)?).to_path_buf());
            }
            "--offline" => {
                req.offline = true;
            }
            "--python" => {
                req.python_binary = Some(Path::new(&next_val(&mut iter)?).to_path_buf());
            }
            "--sidecar" => {
                req.sidecar_script = Some(Path::new(&next_val(&mut iter)?).to_path_buf());
            }
            "--out" => {
                out_json = Some(Path::new(&next_val(&mut iter)?).to_path_buf());
            }
            "--nodes-out" => {
                nodes_out_json = Some(Path::new(&next_val(&mut iter)?).to_path_buf());
            }
            "--style" => {
                let raw = next_val(&mut iter)?;
                template_config.style = match raw.to_ascii_lowercase().as_str() {
                    "vector" | "vector_text" | "vectortext" => LyricNodeStyle::VectorText,
                    "3d" | "text3d" | "text_3d" => LyricNodeStyle::Text3D,
                    other => bail!("unknown ai-lyrics --style '{other}'"),
                };
            }
            "--base-node" => {
                template_config.base_input_node_id = next_val(&mut iter)?;
            }
            "--vertical-ja" => {
                template_config.vertical_japanese = true;
            }
            other => bail!("unknown ai-lyrics flag '{other}'"),
        }
    }
    Ok(AiLyricsCliOptions {
        request: req,
        out_json,
        nodes_out_json,
        template_config,
    })
}

fn ai_lyrics_cli(args: &[String]) -> Result<()> {
    let opts = parse_ai_lyrics_args(args)?;
    let result = align_audio_lyrics(&opts.request)?;
    if let Some(out) = &opts.out_json {
        if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(out, serde_json::to_vec_pretty(&result)?)?;
    }
    if let Some(nodes_out) = &opts.nodes_out_json {
        let words: Vec<_> = result.all_words().into_iter().cloned().collect();
        let nodes = build_lyric_timeline_nodes(&words, &result.language, &opts.template_config);
        if let Some(parent) = nodes_out.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(nodes_out, serde_json::to_vec_pretty(&nodes)?)?;
    }
    print_json(&result)
}

fn parse_upscale_args(args: &[String]) -> Result<(std::path::PathBuf, std::path::PathBuf, UpscaleConfig, ExportEncodingProfile)> {
    let Some((in_path, flags)) = args.split_first() else {
        bail!(
            "usage: upscale <in.mp4> --out <out.mp4> [--scale 2|3|4] [--model realesr-animevideov3|realesrgan-x4plus-anime|realesrgan-x4plus] [--backend auto|realesrgan|lanczos|bilinear] [--gpu <id>] [--tile <u32>] [--dedup <f32>] [--scd <f32>] [--chunk <usize>] [--tta] [--platform <preset>]"
        );
    };
    let mut out_path: Option<std::path::PathBuf> = None;
    let mut cfg = UpscaleConfig::default();
    let mut profile = ExportEncodingProfile::default();
    let mut iter = flags.iter();
    while let Some(flag) = iter.next() {
        let (name, inline_val) = match flag.split_once('=') {
            Some((k, v)) => (k, Some(v)),
            None => (flag.as_str(), None),
        };
        let next_val = |iter: &mut std::slice::Iter<'_, String>| -> Result<String> {
            match inline_val {
                Some(v) => Ok(v.to_owned()),
                None => iter
                    .next()
                    .cloned()
                    .with_context(|| format!("missing value for {name}")),
            }
        };
        match name {
            "--out" => {
                out_path = Some(Path::new(&next_val(&mut iter)?).to_path_buf());
            }
            "--scale" => {
                cfg.scale = next_val(&mut iter)?.parse().context("invalid --scale integer")?;
            }
            "--model" => {
                let raw = next_val(&mut iter)?;
                cfg.model = RealEsrganModel::from_name(&raw)
                    .with_context(|| format!("unknown Real-ESRGAN --model '{raw}'"))?;
            }
            "--backend" => {
                let raw = next_val(&mut iter)?;
                cfg.backend = match raw.to_ascii_lowercase().as_str() {
                    "auto" => None,
                    "realesrgan" | "realesrgan-ncnn-vulkan" => {
                        Some(UpscaleBackend::RealEsrganNcnnCli)
                    }
                    "lanczos" | "ffmpeg" | "ffmpeg-lanczos" => Some(UpscaleBackend::FfmpegLanczos),
                    "bilinear" => Some(UpscaleBackend::BilinearFallback),
                    other => bail!("unknown upscale --backend '{other}'"),
                };
            }
            "--gpu" => {
                cfg.gpu_id = Some(next_val(&mut iter)?.parse().context("invalid --gpu id")?);
            }
            "--tile" => {
                cfg.tile_size = next_val(&mut iter)?.parse().context("invalid --tile integer")?;
            }
            "--dedup" => {
                cfg.dedup_threshold = next_val(&mut iter)?.parse().context("invalid --dedup float")?;
            }
            "--scd" => {
                cfg.scene_cut_threshold = next_val(&mut iter)?.parse().context("invalid --scd float")?;
            }
            "--chunk" => {
                cfg.chunk_size = next_val(&mut iter)?.parse().context("invalid --chunk integer")?;
            }
            "--tta" => {
                cfg.tta = true;
            }
            "--platform" => {
                let raw = next_val(&mut iter)?;
                profile = ExportEncodingProfile::from_platform_name(&raw)
                    .with_context(|| format!("unknown export --platform '{raw}'"))?;
            }
            other => bail!("unknown upscale flag '{other}'"),
        }
    }
    let out = out_path.context("upscale requires --out <out.mp4>")?;
    Ok((Path::new(in_path).to_path_buf(), out, cfg, profile))
}

fn upscale_cli(args: &[String]) -> Result<()> {
    let (in_path, out_path, cfg, profile) = parse_upscale_args(args)?;
    let report = upscale_video_file(&in_path, &out_path, &cfg, &profile)?;
    print_json(&report)
}

fn parse_ai_matte_args(args: &[String]) -> Result<(std::path::PathBuf, Option<std::path::PathBuf>, AiInferenceConfig)> {
    let Some((in_path, flags)) = args.split_first() else {
        bail!(
            "usage: ai-matte <in.mp4> [--out <out.mp4>] [--model <model.onnx>] [--task rotoscope|depth] [--smooth <f32>] [--provider auto|dml|cuda|cpu] [--depth-format u8|rg16|hevc10|png16] [--norm-mode ema|per_frame|global_pass] [--png16-dir <dir>] [--synthetic-test]"
        );
    };
    let mut out_path: Option<std::path::PathBuf> = None;
    let mut cfg = AiInferenceConfig::default();
    let mut iter = flags.iter();
    while let Some(flag) = iter.next() {
        let (name, inline_val) = match flag.split_once('=') {
            Some((k, v)) => (k, Some(v)),
            None => (flag.as_str(), None),
        };
        let next_val = |iter: &mut std::slice::Iter<'_, String>| -> Result<String> {
            match inline_val {
                Some(v) => Ok(v.to_owned()),
                None => iter
                    .next()
                    .cloned()
                    .with_context(|| format!("missing value for {name}")),
            }
        };
        match name {
            "--out" => {
                out_path = Some(Path::new(&next_val(&mut iter)?).to_path_buf());
            }
            "--model" => {
                cfg.model_path = Some(Path::new(&next_val(&mut iter)?).to_path_buf());
            }
            "--task" => {
                let raw = next_val(&mut iter)?;
                cfg.task = match raw.to_ascii_lowercase().as_str() {
                    "rotoscope" | "matte" => AiTaskKind::RotoscopeMatte,
                    "depth" => AiTaskKind::DepthMap,
                    other => bail!("unknown --task '{other}'"),
                };
            }
            "--smooth" => {
                cfg.temporal_smooth = next_val(&mut iter)?.parse().context("invalid --smooth float")?;
            }
            "--provider" => {
                cfg.provider = next_val(&mut iter)?;
            }
            "--depth-format" => {
                let raw = next_val(&mut iter)?;
                cfg.depth_output_format = match raw.to_ascii_lowercase().as_str() {
                    "u8" | "u8_mp4" => DepthOutputFormat::U8Mp4,
                    "rg16" | "rg16_raw" => DepthOutputFormat::Rg16Raw,
                    "hevc10" | "hevc_10bit_mp4" => DepthOutputFormat::Hevc10BitMp4,
                    "png16" | "png16_sequence" => DepthOutputFormat::Png16Sequence,
                    other => bail!(
                        "unknown --depth-format '{other}'; expected u8, rg16, hevc10, or png16"
                    ),
                };
            }
            "--norm-mode" => {
                let raw = next_val(&mut iter)?;
                cfg.depth_norm_mode = match raw.to_ascii_lowercase().as_str() {
                    "ema" => DepthNormMode::Ema,
                    "per_frame" | "per-frame" => DepthNormMode::PerFrame,
                    "global_pass" | "global-pass" | "global" => DepthNormMode::GlobalPass,
                    other => bail!(
                        "unknown --norm-mode '{other}'; expected ema, per_frame, or global_pass"
                    ),
                };
            }
            "--png16-dir" => {
                cfg.png16_dir = Some(Path::new(&next_val(&mut iter)?).to_path_buf());
            }
            "--synthetic-test" => {
                cfg.synthetic_test = true;
            }
            other => bail!("unknown ai-matte flag '{other}'"),
        }
    }
    if out_path.is_none() && !(cfg.task == AiTaskKind::DepthMap && cfg.png16_dir.is_some()) {
        bail!("missing required --out <out.mp4> (or --png16-dir with --task depth)");
    }
    Ok((Path::new(in_path).to_path_buf(), out_path, cfg))
}

fn ai_matte_cli(args: &[String]) -> Result<()> {
    let (in_path, out_path, cfg) = parse_ai_matte_args(args)?;
    if let Some(out) = out_path {
        let report = run_ai_video_inference(
            &in_path,
            &out,
            &cfg,
            &ExportEncodingProfile::default(),
        )?;
        print_json(&report)
    } else {
        let png16_dir = cfg
            .png16_dir
            .as_deref()
            .context("--png16-dir required when --out is omitted")?;
        let probe = FfmpegBridge::probe_media_info(&in_path)?;
        let decoded = FfmpegBridge::decode_video_window(
            &in_path,
            &probe.pts_index,
            0,
            probe.pts_index.len(),
            probe.width,
            probe.height,
        )?;
        let frames: Vec<_> = decoded.into_iter().map(|(_, f)| f).collect();
        let (paths, report) = run_ai_depth_png16_sequence(&frames, png16_dir, &cfg)?;
        print_json(&json!({
            "report": report,
            "png16_files": paths,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use frameshift_schema::{ColorSpace, GraphNodeDef};
    use frameshift_time::{FLICKS_PER_SECOND, Flicks};

    #[test]
    fn parse_export_target_supports_platform_profile_and_range() {
        let project = impact_cut_project();
        let target = parse_export_target(
            &project,
            "out.mov",
            &[
                "--profile".into(),
                "draft".into(),
                "--range".into(),
                "0..2s".into(),
                "--platform".into(),
                "prores_422hq".into(),
            ],
        )
        .expect("valid export flags should parse");
        assert_eq!(target.profile, RenderProfileKind::Draft);
        assert_eq!(target.platform_preset.as_deref(), Some("prores_422hq"));
        let range = target.range.expect("range should be resolved");
        assert_eq!(range.start_tick, Flicks::ZERO);
        assert_eq!(range.end_tick, Flicks(2 * FLICKS_PER_SECOND));

        let inline = parse_export_target(
            &project,
            "out.mp4",
            &["--platform=tiktok_1080p60".into()],
        )
        .expect("inline --platform= should parse");
        assert_eq!(inline.platform_preset.as_deref(), Some("tiktok_1080p60"));

        let err = parse_export_target(
            &project,
            "out.mp4",
            &["--platform".into(), "unknown_target".into()],
        )
        .expect_err("unknown platform preset must fail");
        assert!(err.to_string().contains("unknown export platform"));
    }

    #[test]
    fn asset_is_matte_input_detects_mask_and_depth_matte_nodes() {
        let graph = RenderGraphDef {
            output_node: NodeId::from("depth_matte"),
            nodes: vec![
                GraphNodeDef {
                    id: NodeId::from("fg"),
                    kind: NodeKind::SourceVideo {
                        asset_id: "asset_fg".into(),
                        clip_id: None,
                        color_space: ColorSpace::DisplaySrgb,
                        fit: frameshift_schema::SourceFit::Contain,
                    },
                    inputs: Vec::new(),
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("matte_src"),
                    kind: NodeKind::SourceImage {
                        asset_id: "asset_depth".into(),
                        color_space: ColorSpace::DisplaySrgb,
                        fit: frameshift_schema::SourceFit::Stretch,
                        opacity: 1.0,
                    },
                    inputs: Vec::new(),
                    enabled: true,
                },
                GraphNodeDef {
                    id: NodeId::from("depth_matte"),
                    kind: NodeKind::DepthMatte {
                        near_depth: 0.2,
                        far_depth: 0.8,
                        feather: 0.05,
                        invert: false,
                    },
                    inputs: vec![NodeId::from("fg"), NodeId::from("matte_src")],
                    enabled: true,
                },
            ],
        };
        assert!(asset_is_matte_input(&graph, "asset_depth"));
        assert!(!asset_is_matte_input(&graph, "asset_fg"));
    }

    #[test]
    fn template_slot_kind_label_covers_new_variants() {
        assert_eq!(template_slot_kind_label(TemplateSlotKind::Color), "color");
        assert_eq!(template_slot_kind_label(TemplateSlotKind::Choice), "choice");
        assert_eq!(
            template_slot_kind_label(TemplateSlotKind::SpeedCurve),
            "speed_curve"
        );
        assert_eq!(template_slot_kind_label(TemplateSlotKind::Font), "font");
        assert_eq!(
            template_slot_kind_label(TemplateSlotKind::AssetPath(AssetKind::Video)),
            "video_asset"
        );
    }

    #[test]
    fn parses_wave2_workbench_subcommands_and_flags() {
        let to_args = |items: &[&str]| -> Vec<String> { items.iter().map(|s| (*s).to_owned()).collect() };

        let stems = parse_ai_stems_args(&to_args(&[
            "bgm.wav",
            "--out-dir",
            "stems_out",
            "--stems",
            "4",
            "--backend",
            "audio-separator",
            "--offline",
        ]))
        .expect("ai-stems args");
        assert_eq!(stems.stems, 4);
        assert_eq!(stems.backend, StemBackend::AudioSeparator);
        assert!(stems.offline);

        let lyrics = parse_ai_lyrics_args(&to_args(&[
            "vocals.wav",
            "--lyrics-text",
            "世界が終わる",
            "--language",
            "ja",
            "--ja-tokenizer",
            "fugashi",
            "--style",
            "text3d",
            "--vertical-ja",
        ]))
        .expect("ai-lyrics args");
        assert_eq!(lyrics.request.ja_tokenizer, JapaneseTokenizer::Fugashi);
        assert_eq!(lyrics.template_config.style, LyricNodeStyle::Text3D);
        assert!(lyrics.template_config.vertical_japanese);

        let (_, _, up_cfg, _) = parse_upscale_args(&to_args(&[
            "in.mp4",
            "--out",
            "out.mp4",
            "--scale",
            "2",
            "--model",
            "realesr-animevideov3",
            "--backend",
            "realesrgan",
            "--gpu",
            "1",
            "--tta",
        ]))
        .expect("upscale args");
        assert_eq!(up_cfg.scale, 2);
        assert_eq!(up_cfg.model, RealEsrganModel::RealesrAnimeVideoV3);
        assert_eq!(up_cfg.backend, Some(UpscaleBackend::RealEsrganNcnnCli));
        assert_eq!(up_cfg.gpu_id, Some(1));
        assert!(up_cfg.tta);

        let (_, out_opt, matte_cfg) = parse_ai_matte_args(&to_args(&[
            "clip.mp4",
            "--task",
            "depth",
            "--depth-format",
            "rg16",
            "--norm-mode",
            "global_pass",
            "--png16-dir",
            "depth16",
            "--synthetic-test",
        ]))
        .expect("ai-matte depth args");
        assert!(out_opt.is_none());
        assert_eq!(matte_cfg.task, AiTaskKind::DepthMap);
        assert_eq!(matte_cfg.depth_output_format, DepthOutputFormat::Rg16Raw);
        assert_eq!(matte_cfg.depth_norm_mode, DepthNormMode::GlobalPass);
        assert_eq!(
            matte_cfg.png16_dir.as_deref(),
            Some(Path::new("depth16"))
        );

        let status = build_ai_status_report();
        assert!(status.get("video_and_onnx").is_some());
        assert!(status.get("ai_audio").is_some());
        assert!(status.get("realesrgan").is_some());
        assert!(status.get("local_models").is_some());
    }
}
