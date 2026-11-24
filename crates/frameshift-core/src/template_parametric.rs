//! P2 parametric template engine (`calculate_template_metadata` & `instantiate_parametric_template`).
//!
//! Resolves duration, resolution, beat-aligned cut boundaries via [`plan_autocut`],
//! and drop effect windows deterministically from a [`ParametricTemplateSpec`] and
//! typed JSON `props` without executing arbitrary template code.

use crate::autocut::{AutocutParams, plan_autocut};
use crate::dsl::{DslCompileOptions, compile_motion_doc, compile_motion_str};
use crate::error::CoreError;
use crate::linter::LintReport;
use crate::resolver::{MediaResolver, ResolvedAssetSource};
use frameshift_media::{
    AudioAnalysisReport, DEFAULT_AUDIO_SAMPLE_RATE, FfmpegBridge, MultibandBeatReport,
    MultibandFluxConfig, SyntheticMediaGenerator,
};
use frameshift_schema::dsl::{MotionDirective, MotionDoc, MotionMeta, MotionRangeExpr, MotionTimeExpr};
use frameshift_schema::template_parametric::{
    BeatSelectionSource, DurationPolicy, MotionBlueprintDef, ParametricTemplateSpec,
    TemplateClipAssignment, TemplateDropEvent, TemplateEffectWindow, TemplateMetadata,
    TemplatePropKind,
};
use frameshift_schema::{AssetKind, Project, Resolution, SourceFit};
use frameshift_time::{FLICKS_PER_SECOND, Flicks, Rational32};
use std::collections::{BTreeMap, BTreeSet};

/// Dry-run resolves duration, resolution, cuts, drop anchors, and effect windows from `spec` + `props`.
pub fn calculate_template_metadata(
    spec: &ParametricTemplateSpec,
    props: &serde_json::Value,
) -> Result<TemplateMetadata, CoreError> {
    calculate_template_metadata_with_beats(spec, props, None)
}

/// Same as [`calculate_template_metadata`] but accepts an optional precomputed [`MultibandBeatReport`].
pub fn calculate_template_metadata_with_beats(
    spec: &ParametricTemplateSpec,
    props: &serde_json::Value,
    beats_override: Option<&MultibandBeatReport>,
) -> Result<TemplateMetadata, CoreError> {
    let merged = validate_json_props(spec, props)?;
    let (metadata, _) = resolve_metadata_and_report(spec, &merged, beats_override)?;
    Ok(metadata)
}

/// Resolves template metadata and instantiates a validated + linted FrameShift `0.2.0` [`Project`].
pub fn instantiate_parametric_template(
    spec: &ParametricTemplateSpec,
    props: &serde_json::Value,
) -> Result<(Project, TemplateMetadata, LintReport), CoreError> {
    instantiate_parametric_template_with_beats(spec, props, None)
}

/// Same as [`instantiate_parametric_template`] with an optional precomputed [`MultibandBeatReport`].
pub fn instantiate_parametric_template_with_beats(
    spec: &ParametricTemplateSpec,
    props: &serde_json::Value,
    beats_override: Option<&MultibandBeatReport>,
) -> Result<(Project, TemplateMetadata, LintReport), CoreError> {
    let merged = validate_json_props(spec, props)?;
    let (metadata, beat_report) = resolve_metadata_and_report(spec, &merged, beats_override)?;

    let compile_opts = DslCompileOptions {
        beat_report: Some(beat_report),
        ..Default::default()
    };

    let compiled = match &spec.motion_blueprint {
        MotionBlueprintDef::MotionMarkdown { markdown_template } => {
            let rendered = substitute_markdown_template(markdown_template, &merged, &metadata);
            compile_motion_str(&rendered, &compile_opts)?
        }
        MotionBlueprintDef::AutoCut {
            include_title_prop,
            shutter_angle_deg,
            shutter_samples,
        } => {
            let doc = build_autocut_motion_doc(
                spec,
                &merged,
                &metadata,
                include_title_prop.as_deref(),
                *shutter_angle_deg,
                *shutter_samples,
            );
            compile_motion_doc(&doc, &compile_opts)?
        }
    };

    Ok((compiled.project, metadata, compiled.lint_report))
}

fn validate_json_props(
    spec: &ParametricTemplateSpec,
    props: &serde_json::Value,
) -> Result<BTreeMap<String, serde_json::Value>, CoreError> {
    let obj = props.as_object().ok_or_else(|| {
        CoreError::InvalidArgument("parametric template props must be a JSON object".into())
    })?;
    let map: BTreeMap<String, serde_json::Value> =
        obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    Ok(spec.validate_props(&map)?)
}

fn resolve_metadata_and_report(
    spec: &ParametricTemplateSpec,
    merged: &BTreeMap<String, serde_json::Value>,
    beats_override: Option<&MultibandBeatReport>,
) -> Result<(TemplateMetadata, MultibandBeatReport), CoreError> {
    let rules = &spec.metadata_rules;
    let fps = rules.default_fps;
    let frame_ticks = fps.frame_duration()?.0;
    let mut warnings = Vec::new();

    // Resolution selection from choice props matching `aspect_presets`.
    let resolved_resolution = pick_resolution(spec, merged);

    // Clip list & audio path lookup.
    let clip_paths = extract_clip_list(spec, merged)?;
    let audio_path = extract_audio_path(spec, merged);

    let (beat_report, audio_duration_ticks) =
        resolve_audio_and_beats(audio_path.as_deref(), beats_override, &mut warnings)?;

    let estimated_bpm = if beat_report.estimated_bpm.is_finite() && beat_report.estimated_bpm > 0.0 {
        Some(beat_report.estimated_bpm)
    } else {
        None
    };

    let raw_duration = compute_policy_duration(
        &rules.duration_policy,
        audio_duration_ticks,
        &beat_report,
    );
    let resolved_duration_ticks = quantize_floor(raw_duration.0.max(frame_ticks), frame_ticks);

    // Build beat series and resample according to `cuts_per_beat`.
    let base_series: &[Flicks] = match rules.beat_source {
        BeatSelectionSource::Downbeats => {
            if beat_report.downbeats.is_empty() {
                &beat_report.beat_grid
            } else {
                &beat_report.downbeats
            }
        }
        BeatSelectionSource::SubOnsets => {
            if beat_report.sub.onsets.is_empty() {
                &beat_report.beat_grid
            } else {
                &beat_report.sub.onsets
            }
        }
        BeatSelectionSource::BeatGrid => &beat_report.beat_grid,
    };

    let candidate_beats = apply_cuts_per_beat(base_series, rules.cuts_per_beat, resolved_duration_ticks);
    let analysis = AudioAnalysisReport {
        window_ticks: beat_report.window_ticks,
        rms_envelope: Vec::new(),
        detected_beats: candidate_beats,
        estimated_bpm: beat_report.estimated_bpm,
    };
    let min_seg_ticks = Flicks(
        ((rules.min_segment_ms as i128 * FLICKS_PER_SECOND as i128) / 1000) as i64,
    );
    let cuts = if min_seg_ticks.0 > resolved_duration_ticks.0 {
        vec![Flicks::ZERO, resolved_duration_ticks]
    } else {
        plan_autocut(
            &analysis,
            AutocutParams {
                start_tick: Flicks::ZERO,
                duration_ticks: resolved_duration_ticks,
                min_segment_ticks: min_seg_ticks,
                include_start: true,
                include_end: true,
            },
        )?
    };

    let quantized_cuts: Vec<Flicks> = cuts
        .into_iter()
        .map(|t| quantize_floor(t.0, frame_ticks))
        .collect();
    let mut clean_cuts = Vec::with_capacity(quantized_cuts.len());
    for c in quantized_cuts {
        if clean_cuts.last().copied() != Some(c) {
            clean_cuts.push(c);
        }
    }
    if clean_cuts.is_empty() {
        clean_cuts.push(Flicks::ZERO);
    }
    if *clean_cuts.last().expect("non-empty") < resolved_duration_ticks {
        clean_cuts.push(resolved_duration_ticks);
    }

    let mut clip_assignments = Vec::new();
    let resolver = MediaResolver::new();
    for seg_idx in 0..clean_cuts.len().saturating_sub(1) {
        let start = clean_cuts[seg_idx];
        let end = clean_cuts[seg_idx + 1];
        if end <= start {
            continue;
        }
        let asset_path = if clip_paths.is_empty() {
            "synthetic://clip_a".to_owned()
        } else {
            clip_paths[seg_idx % clip_paths.len()].clone()
        };
        let source_start = if asset_path.starts_with("synthetic://") {
            Flicks::ZERO
        } else if let Ok(probe) = FfmpegBridge::probe_media_info(std::path::Path::new(&asset_path))
            && let Some(dur) = probe.duration_ticks
        {
            let seg_len = end - start;
            if dur > seg_len {
                let offset = ((seg_idx as i64 * frame_ticks * 12) % (dur.0 - seg_len.0).max(1))
                    .max(0);
                quantize_floor(offset, frame_ticks)
            } else {
                Flicks::ZERO
            }
        } else {
            let _ = &resolver;
            Flicks::ZERO
        };

        clip_assignments.push(TemplateClipAssignment {
            clip_id: format!("shot_{:02}", seg_idx + 1),
            asset_path,
            timeline_start: start,
            timeline_end: end,
            source_start,
        });
    }

    // Drop events from downbeats inside `(0, resolved_duration_ticks)`.
    let mut drop_events = Vec::new();
    let drop_source = if !beat_report.downbeats.is_empty() {
        &beat_report.downbeats
    } else {
        &beat_report.beat_grid
    };
    for &tick in drop_source {
        let q = quantize_floor(tick.0, frame_ticks);
        if q.0 > 0 && q < resolved_duration_ticks && drop_events.len() < 8 {
            let idx = drop_events.len() + 1;
            drop_events.push(TemplateDropEvent {
                event_id: format!("drop_{idx}"),
                label: format!("Drop {idx}"),
                target_output_tick: q,
            });
        }
    }
    if drop_events.is_empty() && clip_assignments.len() >= 2 {
        drop_events.push(TemplateDropEvent {
            event_id: "drop_1".into(),
            label: "Drop 1".into(),
            target_output_tick: clip_assignments[1].timeline_start,
        });
    }

    let mut effect_windows = Vec::new();
    if let Some(preset_id) = &rules.drop_fx_preset_id {
        let pre_ticks = (rules.drop_fx_window_ms[0] as i128 * FLICKS_PER_SECOND as i128 / 1000) as i64;
        let post_ticks =
            (rules.drop_fx_window_ms[1] as i128 * FLICKS_PER_SECOND as i128 / 1000) as i64;
        for ev in &drop_events {
            let s = Flicks((ev.target_output_tick.0 + pre_ticks).max(0));
            let e = Flicks((ev.target_output_tick.0 + post_ticks).min(resolved_duration_ticks.0));
            if e > s {
                effect_windows.push(TemplateEffectWindow {
                    event_id: ev.event_id.clone(),
                    preset_id: preset_id.clone(),
                    start_tick: s,
                    end_tick: e,
                });
            }
        }
    }

    let metadata = TemplateMetadata {
        template_id: spec.template_id.clone(),
        resolved_duration_ticks,
        resolved_duration_sec: resolved_duration_ticks.0 as f64 / FLICKS_PER_SECOND as f64,
        resolved_fps: fps,
        resolved_resolution,
        estimated_bpm,
        cut_count: clip_assignments.len(),
        drop_events,
        effect_windows,
        clip_assignments,
        warnings,
    };
    Ok((metadata, beat_report))
}

fn pick_resolution(
    spec: &ParametricTemplateSpec,
    merged: &BTreeMap<String, serde_json::Value>,
) -> Resolution {
    for prop in &spec.props_schema {
        if matches!(prop.kind, TemplatePropKind::Choice { .. })
            && let Some(val) = merged.get(&prop.id).and_then(|v| v.as_str())
            && let Some(&res) = spec.metadata_rules.aspect_presets.get(val)
        {
            return res;
        }
    }
    spec.metadata_rules
        .aspect_presets
        .values()
        .next()
        .copied()
        .unwrap_or(Resolution {
            width: 1920,
            height: 1080,
        })
}

fn extract_clip_list(
    spec: &ParametricTemplateSpec,
    merged: &BTreeMap<String, serde_json::Value>,
) -> Result<Vec<String>, CoreError> {
    for prop in &spec.props_schema {
        if matches!(prop.kind, TemplatePropKind::VideoClipList { .. })
            && let Some(arr) = merged.get(&prop.id).and_then(|v| v.as_array())
        {
            return Ok(arr
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect());
        }
    }
    Ok(Vec::new())
}

fn extract_audio_path(
    spec: &ParametricTemplateSpec,
    merged: &BTreeMap<String, serde_json::Value>,
) -> Option<String> {
    for prop in &spec.props_schema {
        if matches!(prop.kind, TemplatePropKind::AudioFile)
            && let Some(s) = merged.get(&prop.id).and_then(|v| v.as_str())
        {
            return Some(s.to_owned());
        }
    }
    None
}

fn resolve_audio_and_beats(
    audio_path: Option<&str>,
    beats_override: Option<&MultibandBeatReport>,
    warnings: &mut Vec<String>,
) -> Result<(MultibandBeatReport, Flicks), CoreError> {
    if let Some(override_report) = beats_override {
        let inferred_dur = override_report
            .beat_grid
            .last()
            .copied()
            .map(|last| {
                let period = if override_report.estimated_bpm > 0.0 {
                    Flicks((60.0 / override_report.estimated_bpm * FLICKS_PER_SECOND as f64) as i64)
                } else {
                    Flicks(FLICKS_PER_SECOND / 2)
                };
                Flicks(last.0 + period.0)
            })
            .unwrap_or(Flicks(8 * FLICKS_PER_SECOND));
        return Ok((override_report.clone(), inferred_dur));
    }

    let Some(path_str) = audio_path else {
        let default_dur = Flicks(8 * FLICKS_PER_SECOND);
        let (buf, _) =
            SyntheticMediaGenerator::generate_beat_audio(default_dur, 120, DEFAULT_AUDIO_SAMPLE_RATE);
        let report = buf.analyze_multiband_beats(MultibandFluxConfig::default())?;
        warnings.push("No AudioFile prop supplied; using synthetic 120 BPM 8s beat grid.".into());
        return Ok((report, default_dur));
    };

    let resolver = MediaResolver::new();
    let asset = frameshift_schema::AssetDef {
        id: "template_bgm".into(),
        kind: AssetKind::Audio,
        path: path_str.to_owned(),
        duration_ticks: None,
        fps: None,
        sample_rate_hz: Some(DEFAULT_AUDIO_SAMPLE_RATE),
        resolution: None,
    };
    let buf = match resolver.resolve_asset_source(&asset) {
        Ok(ResolvedAssetSource::Disk(path)) => {
            FfmpegBridge::decode_audio(&path, DEFAULT_AUDIO_SAMPLE_RATE, 2)?
        }
        Ok(ResolvedAssetSource::Synthetic(_)) => {
            let dur = Flicks(8 * FLICKS_PER_SECOND);
            SyntheticMediaGenerator::generate_beat_audio(dur, 120, DEFAULT_AUDIO_SAMPLE_RATE).0
        }
        Err(err) => return Err(err),
    };
    let dur_ticks = Flicks(
        ((buf.frame_count() as i128 * FLICKS_PER_SECOND as i128)
            / i128::from(buf.sample_rate.max(1))) as i64,
    );
    let report = buf.analyze_multiband_beats(MultibandFluxConfig::default())?;
    Ok((report, dur_ticks.max(Flicks(FLICKS_PER_SECOND))))
}

fn compute_policy_duration(
    policy: &DurationPolicy,
    audio_dur: Flicks,
    report: &MultibandBeatReport,
) -> Flicks {
    let beat_period = if report.estimated_bpm.is_finite() && report.estimated_bpm > 0.0 {
        ((60.0 / report.estimated_bpm) * FLICKS_PER_SECOND as f64).round() as i64
    } else {
        FLICKS_PER_SECOND / 2
    };
    match policy {
        DurationPolicy::AudioLength => audio_dur,
        DurationPolicy::BeatCount { beats } => {
            let b = (*beats).max(1) as usize;
            if let Some(&t) = report.beat_grid.get(b - 1) {
                Flicks(t.0 + beat_period)
            } else {
                Flicks(beat_period * (b as i64))
            }
        }
        DurationPolicy::DownbeatBars { bars } => {
            let n = (*bars).max(1) as usize;
            if let Some(&t) = report.downbeats.get(n - 1) {
                Flicks(t.0 + beat_period * i64::from(report.beats_per_bar.max(1)))
            } else {
                Flicks(beat_period * i64::from(report.beats_per_bar.max(1)) * (n as i64))
            }
        }
        DurationPolicy::ClampMs { min_ms, max_ms } => {
            let min_t = (*min_ms as i128 * FLICKS_PER_SECOND as i128 / 1000) as i64;
            let max_t = (*max_ms as i128 * FLICKS_PER_SECOND as i128 / 1000) as i64;
            Flicks(audio_dur.0.clamp(min_t, max_t))
        }
    }
}

fn apply_cuts_per_beat(
    base: &[Flicks],
    cuts_per_beat: Rational32,
    duration_ticks: Flicks,
) -> Vec<Flicks> {
    let mut sorted: Vec<Flicks> = base
        .iter()
        .copied()
        .filter(|t| t.0 >= 0 && t.0 < duration_ticks.0)
        .collect();
    sorted.sort_unstable_by_key(|t| t.0);
    sorted.dedup();
    if sorted.is_empty() {
        return sorted;
    }

    let num = cuts_per_beat.num.max(1);
    let den = cuts_per_beat.den.max(1);
    if num == den {
        return sorted;
    }
    if num > den {
        // Subdivide each interval into `sub = ceil(num/den)` segments.
        let sub = num.div_ceil(den) as i64;
        let mut out = Vec::new();
        for win in sorted.windows(2) {
            let a = win[0].0;
            let b = win[1].0;
            out.push(Flicks(a));
            for k in 1..sub {
                let interp = a + ((b - a) as i128 * k as i128 / sub as i128) as i64;
                out.push(Flicks(interp));
            }
        }
        if let Some(&last) = sorted.last() {
            out.push(last);
        }
        out
    } else {
        // Stride by `step = ceil(den/num)`.
        let step = den.div_ceil(num) as usize;
        sorted.into_iter().step_by(step).collect()
    }
}

fn quantize_floor(ticks: i64, frame_ticks: i64) -> Flicks {
    if frame_ticks <= 1 {
        return Flicks(ticks);
    }
    Flicks((ticks / frame_ticks) * frame_ticks)
}

fn substitute_markdown_template(
    template: &str,
    merged: &BTreeMap<String, serde_json::Value>,
    meta: &TemplateMetadata,
) -> String {
    let mut out = template.to_owned();
    for (k, v) in merged {
        let placeholder = format!("{{{{{k}}}}}");
        let rep = match v {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        out = out.replace(&placeholder, &rep);
    }
    out = out
        .replace(
            "{{duration_sec}}",
            &format!("{:.6}", meta.resolved_duration_sec),
        )
        .replace("{{cut_count}}", &meta.cut_count.to_string())
        .replace(
            "{{bpm}}",
            &meta
                .estimated_bpm
                .map_or_else(|| "120".into(), |b| format!("{b:.2}")),
        );
    out
}

fn build_autocut_motion_doc(
    spec: &ParametricTemplateSpec,
    merged: &BTreeMap<String, serde_json::Value>,
    meta: &TemplateMetadata,
    include_title_prop: Option<&str>,
    shutter_angle_deg: Option<f32>,
    shutter_samples: Option<u32>,
) -> MotionDoc {
    let mut directives = Vec::new();
    let mut path_to_asset_id = BTreeMap::new();
    let mut seen_assets = BTreeSet::new();

    for (idx, assign) in meta.clip_assignments.iter().enumerate() {
        if !path_to_asset_id.contains_key(&assign.asset_path) {
            let asset_id = format!("v_asset_{idx}");
            path_to_asset_id.insert(assign.asset_path.clone(), asset_id.clone());
            if seen_assets.insert(asset_id.clone()) {
                directives.push(MotionDirective::Asset {
                    id: asset_id,
                    kind: AssetKind::Video,
                    path: assign.asset_path.clone(),
                });
            }
        }
    }

    if let Some(audio_path) = extract_audio_path(spec, merged) {
        directives.push(MotionDirective::Asset {
            id: "bgm".into(),
            kind: AssetKind::Audio,
            path: audio_path,
        });
        directives.push(MotionDirective::Audio {
            id: "a_bgm".into(),
            asset_id: "bgm".into(),
            range: MotionRangeExpr {
                start: MotionTimeExpr::Ticks(0),
                end: MotionTimeExpr::Ticks(meta.resolved_duration_ticks.0),
            },
            src: Some(MotionTimeExpr::Ticks(0)),
            bus: None,
            gain_db: None,
            fade_in: None,
            fade_out: None,
        });
    }

    for assign in &meta.clip_assignments {
        let asset_id = path_to_asset_id[&assign.asset_path].clone();
        directives.push(MotionDirective::Shot {
            id: assign.clip_id.clone(),
            asset_id,
            range: MotionRangeExpr {
                start: MotionTimeExpr::Ticks(assign.timeline_start.0),
                end: MotionTimeExpr::Ticks(assign.timeline_end.0),
            },
            src: Some(MotionTimeExpr::Ticks(assign.source_start.0)),
            fit: Some(SourceFit::Cover),
            speed: None,
            anchor: None,
        });
    }

    for ev in &meta.drop_events {
        directives.push(MotionDirective::BeatDrop {
            id: ev.event_id.clone(),
            at: MotionTimeExpr::Ticks(ev.target_output_tick.0),
            clip_id: None,
            src: None,
            label: Some(ev.label.clone()),
        });
    }

    let tr_dur_ticks =
        (spec.metadata_rules.transition_dur_ms as i128 * FLICKS_PER_SECOND as i128 / 1000) as i64;
    if tr_dur_ticks > 0 && !spec.metadata_rules.transition_shader.trim().is_empty() {
        for win in meta.clip_assignments.windows(2) {
            let a = &win[0];
            let b = &win[1];
            let a_len = a.timeline_end.0 - a.timeline_start.0;
            let b_len = b.timeline_end.0 - b.timeline_start.0;
            let max_dur = (a_len.min(b_len) / 2).max(1);
            let actual_dur = tr_dur_ticks.min(max_dur);
            if actual_dur > 0 {
                directives.push(MotionDirective::Transition {
                    shader: spec.metadata_rules.transition_shader.clone(),
                    from_clip: a.clip_id.clone(),
                    to_clip: b.clip_id.clone(),
                    at: MotionTimeExpr::Ticks(a.timeline_end.0),
                    dur: MotionTimeExpr::Ticks(actual_dur),
                    params: BTreeMap::new(),
                });
            }
        }
    }

    if let Some(first_fx) = meta.effect_windows.first() {
        directives.push(MotionDirective::Fx {
            preset_or_kind: first_fx.preset_id.clone(),
            on: "output".into(),
            at: Some(MotionTimeExpr::Ticks(first_fx.start_tick.0)),
            dur: Some(MotionTimeExpr::Ticks(
                (first_fx.end_tick.0 - first_fx.start_tick.0).max(1),
            )),
            params: BTreeMap::new(),
        });
    }

    if let Some(title_key) = include_title_prop
        && let Some(title) = merged.get(title_key).and_then(|v| v.as_str())
        && !title.trim().is_empty()
    {
        let accent_hex = spec
            .props_schema
            .iter()
            .find(|p| matches!(p.kind, TemplatePropKind::ColorHex))
            .and_then(|p| merged.get(&p.id))
            .and_then(|v| v.as_str())
            .map(str::to_owned);
        directives.push(MotionDirective::Text {
            text: title.to_owned(),
            range: MotionRangeExpr {
                start: MotionTimeExpr::Ticks(0),
                end: MotionTimeExpr::Ticks(meta.resolved_duration_ticks.0),
            },
            pos: Some([0.5, 0.78]),
            size: Some(0.07),
            color: accent_hex.or_else(|| Some("#FFFFFF".into())),
            font: None,
            outline: Some("#000000".into()),
            anim: Some("fade".into()),
        });
    }

    if shutter_angle_deg.is_some() || shutter_samples.is_some() {
        directives.push(MotionDirective::Shutter {
            angle: shutter_angle_deg,
            samples: shutter_samples,
            phase: None,
        });
    }

    MotionDoc {
        meta: MotionMeta {
            name: spec.name.clone(),
            fps: meta.resolved_fps,
            resolution: meta.resolved_resolution,
            bpm: meta.estimated_bpm,
            lut: None,
            bg_color: None,
            extra: BTreeMap::new(),
        },
        directives,
    }
}
