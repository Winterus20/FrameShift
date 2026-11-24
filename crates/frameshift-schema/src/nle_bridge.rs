//! Zero-dependency NLE interchange bridge for OpenTimelineIO (`.otio` JSON) and
//! Final Cut Pro XML (`FCPXML 1.10 / 1.11`).
//!
//! Uses `serde_json` for `.otio` and a pure-Rust XML writer/reader (rejecting external
//! `<!ENTITY>` / `SYSTEM` declarations) with lossless `Flicks` <-> `RationalTime` / `"N/Ds"`
//! integer-rational conversions.

use crate::error::SchemaError;
use crate::graph::{BlendMode, ColorSpace, GraphNodeDef, NodeId, NodeKind, RenderGraphDef, SourceFit};
use crate::project::{
    AssetDef, AssetKind, AudioBus, AudioClipDef, AudioFadeDef, ClipDef, EventAnchorDef,
    EventOrigin, EventTimingConstraint, HwAccelPolicy, Project, ProxyConfigDef, RenderProfileDef,
    RenderProfileKind, RenderProfilesDef, Resolution, TimelineDef, WorkingPixelFormat,
};
use crate::version::CURRENT_SCHEMA_VERSION;
use frameshift_time::{FLICKS_PER_SECOND, FPS_24, Flicks, Rational32, SpeedCurve};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

/// Marker category carried in NLE exports/imports (events, beats, downbeats, lyrics).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NleMarkerKind {
    #[default]
    UserEvent,
    Beat,
    Downbeat,
    Lyric,
}

/// Optional extra timeline marker passed to [`export_project_to_otio_json`] or [`export_project_to_fcpxml`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NleMarker {
    pub label: String,
    pub start_tick: Flicks,
    pub duration_ticks: Flicks,
    #[serde(default)]
    pub kind: NleMarkerKind,
}

/// Optional settings for NLE export.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NleExportOptions {
    pub extra_markers: Vec<NleMarker>,
}

/// Optional overrides for NLE import.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NleImportOptions {
    pub project_name: Option<String>,
    pub fps_override: Option<Rational32>,
    pub resolution_override: Option<Resolution>,
}

/// OpenTimelineIO `RationalTime.1`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct OtioRationalTime {
    #[serde(rename = "OTIO_SCHEMA")]
    pub schema: OtioSchemaTag<OtioRationalTimeTag>,
    pub value: f64,
    pub rate: f64,
}

impl OtioRationalTime {
    pub fn new(value: f64, rate: f64) -> Self {
        Self {
            schema: OtioSchemaTag::default(),
            value,
            rate,
        }
    }
}

/// OpenTimelineIO `TimeRange.1`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct OtioTimeRange {
    #[serde(rename = "OTIO_SCHEMA")]
    pub schema: OtioSchemaTag<OtioTimeRangeTag>,
    pub start_time: OtioRationalTime,
    pub duration: OtioRationalTime,
}

impl OtioTimeRange {
    pub fn new(start_time: OtioRationalTime, duration: OtioRationalTime) -> Self {
        Self {
            schema: OtioSchemaTag::default(),
            start_time,
            duration,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OtioSchemaTag<T>(std::marker::PhantomData<T>);

pub trait OtioSchemaName {
    const SCHEMA: &'static str;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OtioRationalTimeTag;
impl OtioSchemaName for OtioRationalTimeTag {
    const SCHEMA: &'static str = "RationalTime.1";
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OtioTimeRangeTag;
impl OtioSchemaName for OtioTimeRangeTag {
    const SCHEMA: &'static str = "TimeRange.1";
}

impl<T: OtioSchemaName> Serialize for OtioSchemaTag<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(T::SCHEMA)
    }
}

impl<'de, T: OtioSchemaName> Deserialize<'de> for OtioSchemaTag<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        if s == T::SCHEMA {
            Ok(Self(std::marker::PhantomData))
        } else {
            Err(serde::de::Error::custom(format!(
                "expected OTIO_SCHEMA '{}', found '{s}'",
                T::SCHEMA
            )))
        }
    }
}

#[inline]
fn gcd_u128(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        let r = a % b;
        a = b;
        b = r;
    }
    a.max(1)
}

#[inline]
fn div_round_i128(num: i128, den: i128) -> i128 {
    let q = num / den;
    let r = num % den;
    if r.abs() * 2 >= den {
        q + num.signum()
    } else {
        q
    }
}

/// Converts `Flicks` to OpenTimelineIO `RationalTime.1` at the given rational `fps`.
///
/// For frame-aligned ticks at standard integer/NTSC frame rates, `value` is exact integer frames.
pub fn flicks_to_otio_rational_time(tick: Flicks, fps: Rational32) -> OtioRationalTime {
    let rate = fps.num as f64 / fps.den as f64;
    let num = (tick.0 as i128) * (fps.num as i128);
    let den = (FLICKS_PER_SECOND as i128) * (fps.den as i128);
    let value = if den != 0 && num % den == 0 {
        (num / den) as f64
    } else {
        (tick.0 as f64 * rate) / FLICKS_PER_SECOND as f64
    };
    OtioRationalTime::new(value, rate)
}

/// Converts OpenTimelineIO `RationalTime.1` back to integer `Flicks`.
pub fn otio_rational_time_to_flicks(rt: &OtioRationalTime) -> Result<Flicks, SchemaError> {
    if !rt.value.is_finite() || !rt.rate.is_finite() || rt.rate <= 0.0 {
        return Err(SchemaError::InvalidNleDocument(format!(
            "invalid RationalTime value={} rate={}",
            rt.value, rt.rate
        )));
    }
    // Check exact rational match for common NTSC rates (24000/1001, 30000/1001, 60000/1001)
    let (rate_num, rate_den): (i128, i128) = if (rt.rate - 24000.0 / 1001.0).abs() < 1e-6 {
        (24000, 1001)
    } else if (rt.rate - 30000.0 / 1001.0).abs() < 1e-6 {
        (30000, 1001)
    } else if (rt.rate - 60000.0 / 1001.0).abs() < 1e-6 {
        (60000, 1001)
    } else if (rt.rate - rt.rate.round()).abs() < 1e-9 && rt.rate.round() >= 1.0 {
        (rt.rate.round() as i128, 1)
    } else {
        let secs = rt.value / rt.rate;
        let ticks_f = (secs * FLICKS_PER_SECOND as f64).round();
        if !ticks_f.is_finite() || ticks_f < i64::MIN as f64 || ticks_f > i64::MAX as f64 {
            return Err(SchemaError::InvalidNleDocument(
                "RationalTime overflows Flicks".into(),
            ));
        }
        return Ok(Flicks(ticks_f as i64));
    };

    if (rt.value - rt.value.round()).abs() < 1e-9 {
        let val_i = rt.value.round() as i128;
        let num = val_i
            .checked_mul(FLICKS_PER_SECOND as i128)
            .and_then(|v| v.checked_mul(rate_den))
            .ok_or_else(|| SchemaError::InvalidNleDocument("RationalTime integer overflow".into()))?;
        let ticks = div_round_i128(num, rate_num);
        return i64::try_from(ticks)
            .map(Flicks)
            .map_err(|_| SchemaError::InvalidNleDocument("RationalTime exceeds i64 Flicks".into()));
    }

    let ticks_f = ((rt.value * rate_den as f64 * FLICKS_PER_SECOND as f64) / rate_num as f64).round();
    if !ticks_f.is_finite() || ticks_f < i64::MIN as f64 || ticks_f > i64::MAX as f64 {
        return Err(SchemaError::InvalidNleDocument(
            "RationalTime overflows Flicks".into(),
        ));
    }
    Ok(Flicks(ticks_f as i64))
}

/// Losslessly formats `Flicks` as an FCPXML rational time string (`"0s"`, `"1001/24000s"`, `"1/60s"`, `"1/48000s"`).
pub fn flicks_to_fcpxml_time(tick: Flicks) -> String {
    if tick.0 == 0 {
        return "0s".to_owned();
    }
    let g = gcd_u128(tick.0.unsigned_abs() as u128, FLICKS_PER_SECOND as u128) as i128;
    let num = (tick.0 as i128) / g;
    let den = (FLICKS_PER_SECOND as i128) / g;
    if den == 1 {
        format!("{num}s")
    } else {
        format!("{num}/{den}s")
    }
}

/// Parses an FCPXML rational or decimal time string (`"0s"`, `"1001/24000s"`, `"1.5s"`) into `Flicks`.
pub fn fcpxml_time_to_flicks(raw: &str) -> Result<Flicks, SchemaError> {
    let s = raw.trim();
    if s == "0" || s == "0s" {
        return Ok(Flicks::ZERO);
    }
    let body = s.strip_suffix('s').ok_or_else(|| {
        SchemaError::InvalidNleDocument(format!("FCPXML time '{s}' must end with 's'"))
    })?;
    if let Some((num_s, den_s)) = body.split_once('/') {
        let num: i128 = num_s.trim().parse().map_err(|_| {
            SchemaError::InvalidNleDocument(format!("invalid FCPXML time numerator '{s}'"))
        })?;
        let den: i128 = den_s.trim().parse().map_err(|_| {
            SchemaError::InvalidNleDocument(format!("invalid FCPXML time denominator '{s}'"))
        })?;
        if den <= 0 {
            return Err(SchemaError::InvalidNleDocument(format!(
                "FCPXML time denominator must be > 0 in '{s}'"
            )));
        }
        let scaled = num
            .checked_mul(FLICKS_PER_SECOND as i128)
            .ok_or_else(|| SchemaError::InvalidNleDocument(format!("FCPXML time overflow '{s}'")))?;
        let ticks = div_round_i128(scaled, den);
        return i64::try_from(ticks).map(Flicks).map_err(|_| {
            SchemaError::InvalidNleDocument(format!("FCPXML time '{s}' exceeds i64 Flicks"))
        });
    }
    if let Ok(whole) = body.parse::<i128>() {
        let ticks = whole
            .checked_mul(FLICKS_PER_SECOND as i128)
            .ok_or_else(|| SchemaError::InvalidNleDocument(format!("FCPXML time overflow '{s}'")))?;
        return i64::try_from(ticks).map(Flicks).map_err(|_| {
            SchemaError::InvalidNleDocument(format!("FCPXML time '{s}' exceeds i64 Flicks"))
        });
    }
    let secs: f64 = body
        .parse()
        .map_err(|_| SchemaError::InvalidNleDocument(format!("invalid FCPXML time '{s}'")))?;
    let ticks_f = (secs * FLICKS_PER_SECOND as f64).round();
    if !ticks_f.is_finite() || ticks_f < i64::MIN as f64 || ticks_f > i64::MAX as f64 {
        return Err(SchemaError::InvalidNleDocument(format!(
            "FCPXML time '{s}' overflows Flicks"
        )));
    }
    Ok(Flicks(ticks_f as i64))
}

fn path_to_file_url(path: &str) -> String {
    if path.starts_with("file://") || path.starts_with("synthetic://") {
        return path.to_owned();
    }
    let normalized = path.replace('\\', "/");
    if normalized.starts_with('/') {
        format!("file://{normalized}")
    } else if normalized.len() >= 2 && normalized.as_bytes()[1] == b':' {
        format!("file:///{normalized}")
    } else {
        normalized
    }
}

fn file_url_to_path(url: &str) -> String {
    if let Some(rest) = url.strip_prefix("file:///") {
        // Windows drive letter `C:/...` vs Unix `/...`
        if rest.len() >= 2 && rest.as_bytes()[1] == b':' {
            rest.to_owned()
        } else {
            format!("/{rest}")
        }
    } else if let Some(rest) = url.strip_prefix("file://") {
        rest.to_owned()
    } else {
        url.to_owned()
    }
}

fn marker_color_for_kind(kind: NleMarkerKind) -> &'static str {
    match kind {
        NleMarkerKind::Downbeat => "RED",
        NleMarkerKind::Beat => "ORANGE",
        NleMarkerKind::Lyric => "CYAN",
        NleMarkerKind::UserEvent => "MAGENTA",
    }
}

fn audio_bus_name(bus: AudioBus) -> &'static str {
    match bus {
        AudioBus::Music => "music",
        AudioBus::Dialogue => "dialogue",
        AudioBus::SfxImpact => "sfx_impact",
        AudioBus::SfxWhoosh => "sfx_whoosh",
        AudioBus::SfxRiser => "sfx_riser",
        AudioBus::SubBass => "sub_bass",
    }
}

fn parse_audio_bus_name(name: &str) -> AudioBus {
    match name.to_ascii_lowercase().as_str() {
        "dialogue" => AudioBus::Dialogue,
        "sfx_impact" => AudioBus::SfxImpact,
        "sfx_whoosh" => AudioBus::SfxWhoosh,
        "sfx_riser" => AudioBus::SfxRiser,
        "sub_bass" => AudioBus::SubBass,
        _ => AudioBus::Music,
    }
}

fn collect_all_markers(project: &Project, opts: &NleExportOptions) -> Vec<NleMarker> {
    let frame_dur = project
        .timeline
        .fps
        .frame_duration()
        .unwrap_or(Flicks(FLICKS_PER_SECOND / 24));
    let mut markers: Vec<NleMarker> = project
        .events
        .iter()
        .map(|ev| NleMarker {
            label: if ev.label.trim().is_empty() {
                ev.event_id.clone()
            } else {
                ev.label.clone()
            },
            start_tick: ev.target_output_tick,
            duration_ticks: frame_dur,
            kind: match ev.origin {
                EventOrigin::UserMarked => NleMarkerKind::UserEvent,
                EventOrigin::Detected => NleMarkerKind::Beat,
            },
        })
        .collect();
    markers.extend(opts.extra_markers.iter().cloned());
    markers.sort_by_key(|m| m.start_tick);
    markers
}

fn build_video_lanes(clips: &[ClipDef]) -> Vec<Vec<&ClipDef>> {
    let mut sorted: Vec<&ClipDef> = clips.iter().collect();
    sorted.sort_by_key(|c| (c.timeline_start, c.timeline_end));
    let mut lanes: Vec<Vec<&ClipDef>> = Vec::new();
    for clip in sorted {
        if let Some(lane) = lanes
            .iter_mut()
            .find(|l| l.last().is_none_or(|prev| prev.timeline_end <= clip.timeline_start))
        {
            lane.push(clip);
        } else {
            lanes.push(vec![clip]);
        }
    }
    if lanes.is_empty() {
        lanes.push(Vec::new());
    }
    lanes
}

/// Exports a FrameShift [`Project`] to OpenTimelineIO (`.otio` JSON) string.
pub fn export_project_to_otio_json(
    project: &Project,
    opts: &NleExportOptions,
) -> Result<String, SchemaError> {
    let fps = project.timeline.fps;
    let rate = fps.num as f64 / fps.den as f64;
    let asset_map: BTreeMap<&str, &AssetDef> =
        project.assets.iter().map(|a| (a.id.as_str(), a)).collect();

    let markers = collect_all_markers(project, opts);
    let otio_markers: Vec<serde_json::Value> = markers
        .iter()
        .map(|m| {
            let start_rt = flicks_to_otio_rational_time(m.start_tick, fps);
            let dur_rt = flicks_to_otio_rational_time(m.duration_ticks.max(Flicks::ZERO), fps);
            serde_json::json!({
                "OTIO_SCHEMA": "Marker.1",
                "name": m.label,
                "color": marker_color_for_kind(m.kind),
                "marked_range": OtioTimeRange::new(start_rt, dur_rt),
                "metadata": {
                    "frameshift": {
                        "kind": m.kind,
                        "exact_start_ticks": m.start_tick.0,
                        "exact_duration_ticks": m.duration_ticks.0,
                    }
                }
            })
        })
        .collect();

    let mut track_values: Vec<serde_json::Value> = Vec::new();
    let video_lanes = build_video_lanes(&project.timeline.clips);

    for (lane_idx, lane) in video_lanes.iter().enumerate() {
        let mut children: Vec<serde_json::Value> = Vec::new();
        let mut cursor = Flicks::ZERO;
        for clip in lane {
            if clip.timeline_start > cursor {
                let gap_dur = clip.timeline_start - cursor;
                children.push(serde_json::json!({
                    "OTIO_SCHEMA": "Gap.1",
                    "name": "Gap",
                    "source_range": OtioTimeRange::new(
                        OtioRationalTime::new(0.0, rate),
                        flicks_to_otio_rational_time(gap_dur, fps)
                    )
                }));
            }
            let out_dur = clip.timeline_end - clip.timeline_start;
            let src_dur = clip.speed_curve.source_offset_at(out_dur).max(Flicks(1));
            let asset = asset_map.get(clip.asset_id.as_str()).copied();
            let target_url = asset
                .map(|a| path_to_file_url(&a.path))
                .unwrap_or_else(|| clip.asset_id.clone());
            let avail_dur = asset
                .and_then(|a| a.duration_ticks)
                .unwrap_or(clip.source_start + src_dur);
            let avg_speed = src_dur.0 as f64 / out_dur.0.max(1) as f64;
            let effects = if (avg_speed - 1.0).abs() > 1e-6 {
                vec![serde_json::json!({
                    "OTIO_SCHEMA": "LinearTimeWarp.1",
                    "name": "SpeedCurve",
                    "effect_name": "LinearTimeWarp",
                    "time_scalar": avg_speed,
                    "metadata": {
                        "frameshift": {
                            "speed_curve": clip.speed_curve,
                        }
                    }
                })]
            } else {
                Vec::new()
            };

            children.push(serde_json::json!({
                "OTIO_SCHEMA": "Clip.1",
                "name": clip.id,
                "source_range": OtioTimeRange::new(
                    flicks_to_otio_rational_time(clip.source_start, fps),
                    flicks_to_otio_rational_time(out_dur, fps)
                ),
                "media_reference": {
                    "OTIO_SCHEMA": "ExternalReference.1",
                    "name": clip.asset_id,
                    "target_url": target_url,
                    "available_range": OtioTimeRange::new(
                        OtioRationalTime::new(0.0, rate),
                        flicks_to_otio_rational_time(avail_dur, fps)
                    )
                },
                "effects": effects,
                "metadata": {
                    "frameshift": {
                        "asset_id": clip.asset_id,
                        "exact_timeline_start_ticks": clip.timeline_start.0,
                        "exact_timeline_end_ticks": clip.timeline_end.0,
                        "exact_source_start_ticks": clip.source_start.0,
                    }
                }
            }));
            cursor = clip.timeline_end;
        }
        if cursor < project.timeline.duration_ticks && lane_idx == 0 {
            let tail_gap = project.timeline.duration_ticks - cursor;
            children.push(serde_json::json!({
                "OTIO_SCHEMA": "Gap.1",
                "name": "Gap",
                "source_range": OtioTimeRange::new(
                    OtioRationalTime::new(0.0, rate),
                    flicks_to_otio_rational_time(tail_gap, fps)
                )
            }));
        }

        track_values.push(serde_json::json!({
            "OTIO_SCHEMA": "Track.1",
            "name": if lane_idx == 0 { "V1".to_owned() } else { format!("V{}", lane_idx + 1) },
            "kind": "Video",
            "children": children,
            "markers": if lane_idx == 0 { otio_markers.clone() } else { Vec::new() }
        }));
    }

    // Group audio clips by AudioBus in deterministic order.
    let mut audio_by_bus: BTreeMap<&'static str, Vec<&AudioClipDef>> = BTreeMap::new();
    for ac in &project.timeline.audio_clips {
        audio_by_bus
            .entry(audio_bus_name(ac.bus))
            .or_default()
            .push(ac);
    }
    for (bus_name, mut clips) in audio_by_bus {
        clips.sort_by_key(|c| c.timeline_start);
        let mut children: Vec<serde_json::Value> = Vec::new();
        let mut cursor = Flicks::ZERO;
        for ac in clips {
            let asset = asset_map.get(ac.asset_id.as_str()).copied();
            let sample_rate = asset.and_then(|a| a.sample_rate_hz).unwrap_or(48_000);
            let audio_fps = Rational32::new(sample_rate, 1)?;
            if ac.timeline_start > cursor {
                let gap = ac.timeline_start - cursor;
                children.push(serde_json::json!({
                    "OTIO_SCHEMA": "Gap.1",
                    "name": "Gap",
                    "source_range": OtioTimeRange::new(
                        OtioRationalTime::new(0.0, sample_rate as f64),
                        flicks_to_otio_rational_time(gap, audio_fps)
                    )
                }));
            }
            let dur = ac.timeline_end - ac.timeline_start;
            let target_url = asset
                .map(|a| path_to_file_url(&a.path))
                .unwrap_or_else(|| ac.asset_id.clone());
            children.push(serde_json::json!({
                "OTIO_SCHEMA": "Clip.1",
                "name": ac.id,
                "source_range": OtioTimeRange::new(
                    flicks_to_otio_rational_time(ac.source_start, audio_fps),
                    flicks_to_otio_rational_time(dur, audio_fps)
                ),
                "media_reference": {
                    "OTIO_SCHEMA": "ExternalReference.1",
                    "name": ac.asset_id,
                    "target_url": target_url
                },
                "metadata": {
                    "frameshift": {
                        "asset_id": ac.asset_id,
                        "bus": ac.bus,
                        "gain_db": ac.gain_db,
                        "pan": ac.pan,
                        "fade_in": ac.fade_in,
                        "fade_out": ac.fade_out,
                        "exact_timeline_start_ticks": ac.timeline_start.0,
                        "exact_timeline_end_ticks": ac.timeline_end.0,
                        "exact_source_start_ticks": ac.source_start.0,
                    }
                }
            }));
            cursor = ac.timeline_end;
        }
        track_values.push(serde_json::json!({
            "OTIO_SCHEMA": "Track.1",
            "name": bus_name,
            "kind": "Audio",
            "children": children,
            "markers": []
        }));
    }

    let root = serde_json::json!({
        "OTIO_SCHEMA": "Timeline.1",
        "name": project.name,
        "global_start_time": OtioRationalTime::new(0.0, rate),
        "tracks": {
            "OTIO_SCHEMA": "Stack.1",
            "name": "tracks",
            "children": track_values
        },
        "metadata": {
            "frameshift": {
                "schema_version": project.schema_version,
                "revision_id": project.revision_id,
                "fps": fps,
                "resolution": project.timeline.resolution,
                "duration_ticks": project.timeline.duration_ticks.0,
            }
        }
    });

    Ok(serde_json::to_string_pretty(&root)?)
}

/// Imports an OpenTimelineIO (`.otio` JSON) string into a validated FrameShift [`Project`].
pub fn import_project_from_otio_json(
    otio_json: &str,
    opts: &NleImportOptions,
) -> Result<Project, SchemaError> {
    let doc: serde_json::Value = serde_json::from_str(otio_json)?;
    let schema = doc
        .get("OTIO_SCHEMA")
        .and_then(|v| v.as_str())
        .ok_or_else(|| SchemaError::InvalidNleDocument("missing OTIO_SCHEMA".into()))?;
    if !schema.starts_with("Timeline.") {
        return Err(SchemaError::InvalidNleDocument(format!(
            "expected OTIO Timeline.1 root, found '{schema}'"
        )));
    }

    let name = opts
        .project_name
        .clone()
        .or_else(|| doc.get("name").and_then(|v| v.as_str()).map(str::to_owned))
        .unwrap_or_else(|| "Imported OTIO Project".to_owned());

    let fs_meta = doc.get("metadata").and_then(|m| m.get("frameshift"));
    let fps = opts
        .fps_override
        .or_else(|| {
            fs_meta
                .and_then(|m| m.get("fps"))
                .and_then(|v| serde_json::from_value::<Rational32>(v.clone()).ok())
        })
        .or_else(|| {
            doc.get("global_start_time")
                .and_then(|g| g.get("rate"))
                .and_then(|r| r.as_f64())
                .and_then(fps_from_rate_f64)
        })
        .unwrap_or(FPS_24);

    let resolution = opts
        .resolution_override
        .or_else(|| {
            fs_meta
                .and_then(|m| m.get("resolution"))
                .and_then(|v| serde_json::from_value::<Resolution>(v.clone()).ok())
        })
        .unwrap_or(Resolution {
            width: 1920,
            height: 1080,
        });

    let tracks = doc
        .get("tracks")
        .and_then(|t| t.get("children"))
        .and_then(|c| c.as_array())
        .ok_or_else(|| SchemaError::InvalidNleDocument("missing tracks.children array".into()))?;

    let mut assets_by_url: BTreeMap<String, String> = BTreeMap::new();
    let mut assets: Vec<AssetDef> = Vec::new();
    let mut video_clips: Vec<ClipDef> = Vec::new();
    let mut audio_clips: Vec<AudioClipDef> = Vec::new();
    let mut events: Vec<EventAnchorDef> = Vec::new();
    let mut seen_ids: BTreeSet<String> = BTreeSet::new();

    for track in tracks {
        let kind = track
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("Video");
        let track_name = track
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("Track");

        if let Some(markers) = track.get("markers").and_then(|m| m.as_array()) {
            for (idx, m) in markers.iter().enumerate() {
                let label = m
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("marker")
                    .to_owned();
                let start_tick = m
                    .get("metadata")
                    .and_then(|md| md.get("frameshift"))
                    .and_then(|fs| fs.get("exact_start_ticks"))
                    .and_then(|v| v.as_i64())
                    .map(Flicks)
                    .or_else(|| {
                        m.get("marked_range")
                            .and_then(|r| r.get("start_time"))
                            .and_then(|st| serde_json::from_value::<OtioRationalTime>(st.clone()).ok())
                            .and_then(|rt| otio_rational_time_to_flicks(&rt).ok())
                    })
                    .unwrap_or(Flicks::ZERO);
                let ev_id = unique_id(&mut seen_ids, &format!("ev_{}_{}", events.len() + 1, idx));
                events.push(EventAnchorDef {
                    event_id: ev_id,
                    label,
                    source_clip_id: None,
                    source_anchor_tick: None,
                    target_output_tick: start_tick,
                    origin: EventOrigin::Detected,
                    timing: EventTimingConstraint::Absolute,
                });
            }
        }

        let Some(children) = track.get("children").and_then(|c| c.as_array()) else {
            continue;
        };
        let mut cursor = Flicks::ZERO;
        for (idx, item) in children.iter().enumerate() {
            let item_schema = item
                .get("OTIO_SCHEMA")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let src_range = item
                .get("source_range")
                .and_then(|sr| serde_json::from_value::<OtioTimeRange>(sr.clone()).ok());
            let dur_ticks = src_range
                .as_ref()
                .map(|sr| otio_rational_time_to_flicks(&sr.duration))
                .transpose()?
                .unwrap_or(Flicks::ZERO);

            if item_schema.starts_with("Gap.") {
                cursor += dur_ticks;
                continue;
            }
            if !item_schema.starts_with("Clip.") {
                continue;
            }

            let clip_fs = item.get("metadata").and_then(|m| m.get("frameshift"));
            let start_tick = clip_fs
                .and_then(|fs| fs.get("exact_timeline_start_ticks"))
                .and_then(|v| v.as_i64())
                .map(Flicks)
                .unwrap_or(cursor);
            let end_tick = clip_fs
                .and_then(|fs| fs.get("exact_timeline_end_ticks"))
                .and_then(|v| v.as_i64())
                .map(Flicks)
                .unwrap_or(start_tick + dur_ticks);
            if end_tick <= start_tick {
                continue;
            }
            let source_start = clip_fs
                .and_then(|fs| fs.get("exact_source_start_ticks"))
                .and_then(|v| v.as_i64())
                .map(Flicks)
                .or_else(|| {
                    src_range
                        .as_ref()
                        .and_then(|sr| otio_rational_time_to_flicks(&sr.start_time).ok())
                })
                .unwrap_or(Flicks::ZERO);

            let target_url = item
                .get("media_reference")
                .and_then(|mr| mr.get("target_url"))
                .and_then(|v| v.as_str())
                .unwrap_or("synthetic://imported");
            let local_path = file_url_to_path(target_url);
            let asset_kind = if kind == "Audio" {
                AssetKind::Audio
            } else {
                AssetKind::Video
            };
            let asset_key = format!("{kind}:{local_path}");
            let asset_id = if let Some(existing) = assets_by_url.get(&asset_key) {
                existing.clone()
            } else {
                let preferred = clip_fs
                    .and_then(|fs| fs.get("asset_id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("asset");
                let id = unique_id(&mut seen_ids, preferred);
                assets_by_url.insert(asset_key, id.clone());
                assets.push(AssetDef {
                    id: id.clone(),
                    kind: asset_kind,
                    path: local_path,
                    duration_ticks: None,
                    fps: if asset_kind == AssetKind::Video {
                        Some(fps)
                    } else {
                        None
                    },
                    sample_rate_hz: if asset_kind == AssetKind::Audio {
                        Some(48_000)
                    } else {
                        None
                    },
                    resolution: if asset_kind == AssetKind::Video {
                        Some(resolution)
                    } else {
                        None
                    },
                });
                id
            };

            let raw_clip_name = item
                .get("name")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or("clip");
            let clip_id = unique_id(&mut seen_ids, &format!("{raw_clip_name}_{idx}"));

            if kind == "Audio" {
                let bus = clip_fs
                    .and_then(|fs| fs.get("bus"))
                    .and_then(|v| serde_json::from_value::<AudioBus>(v.clone()).ok())
                    .unwrap_or_else(|| parse_audio_bus_name(track_name));
                let gain_db = clip_fs
                    .and_then(|fs| fs.get("gain_db"))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0) as f32;
                let pan = clip_fs
                    .and_then(|fs| fs.get("pan"))
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0) as f32;
                let fade_in = clip_fs
                    .and_then(|fs| fs.get("fade_in"))
                    .and_then(|v| serde_json::from_value::<AudioFadeDef>(v.clone()).ok());
                let fade_out = clip_fs
                    .and_then(|fs| fs.get("fade_out"))
                    .and_then(|v| serde_json::from_value::<AudioFadeDef>(v.clone()).ok());
                audio_clips.push(AudioClipDef {
                    id: clip_id,
                    asset_id,
                    timeline_start: start_tick,
                    timeline_end: end_tick,
                    source_start,
                    gain_db,
                    fade_in,
                    fade_out,
                    bus,
                    pan,
                    anchor_event_id: None,
                    apex_offset_ticks: None,
                });
            } else {
                let speed_curve = item
                    .get("effects")
                    .and_then(|e| e.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|ef| ef.get("metadata"))
                    .and_then(|m| m.get("frameshift"))
                    .and_then(|fs| fs.get("speed_curve"))
                    .and_then(|sc| serde_json::from_value::<SpeedCurve>(sc.clone()).ok())
                    .unwrap_or_else(|| {
                        let scalar = item
                            .get("effects")
                            .and_then(|e| e.as_array())
                            .and_then(|arr| arr.first())
                            .and_then(|ef| ef.get("time_scalar"))
                            .and_then(|v| v.as_f64())
                            .unwrap_or(1.0);
                        SpeedCurve::constant(scalar.max(0.0))
                            .unwrap_or_else(|_| SpeedCurve::constant(1.0).unwrap())
                    });

                video_clips.push(ClipDef {
                    id: clip_id,
                    asset_id,
                    timeline_start: start_tick,
                    timeline_end: end_tick,
                    source_start,
                    speed_curve,
                    linked_event_id: None,
                    event_animations: Vec::new(),
                });
            }
            cursor = end_tick;
        }
    }

    let max_end = video_clips
        .iter()
        .map(|c| c.timeline_end)
        .chain(audio_clips.iter().map(|a| a.timeline_end))
        .max()
        .unwrap_or(Flicks(FLICKS_PER_SECOND));
    let duration_ticks = fs_meta
        .and_then(|m| m.get("duration_ticks"))
        .and_then(|v| v.as_i64())
        .map(Flicks)
        .filter(|d| *d > Flicks::ZERO)
        .unwrap_or(max_end.max(Flicks(FLICKS_PER_SECOND)));

    build_imported_project(name, fps, resolution, duration_ticks, assets, video_clips, audio_clips, events)
}

/// Exports a FrameShift [`Project`] to FCPXML 1.11 (`<!DOCTYPE fcpxml>`) string.
pub fn export_project_to_fcpxml(
    project: &Project,
    opts: &NleExportOptions,
) -> Result<String, SchemaError> {
    let fps = project.timeline.fps;
    let frame_dur = fps.frame_duration()?;
    let mut xml = String::with_capacity(4096);
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str("<!DOCTYPE fcpxml>\n");
    xml.push_str("<fcpxml version=\"1.11\">\n");
    xml.push_str("  <resources>\n");
    let _ = writeln!(
        xml,
        "    <format id=\"r1\" name=\"FFVideoFormat{}x{}p{}\" frameDuration=\"{}\" width=\"{}\" height=\"{}\"/>",
        project.timeline.resolution.width,
        project.timeline.resolution.height,
        fps.num / fps.den.max(1),
        flicks_to_fcpxml_time(frame_dur),
        project.timeline.resolution.width,
        project.timeline.resolution.height,
    );

    let mut asset_res_id: BTreeMap<&str, String> = BTreeMap::new();
    for (idx, asset) in project.assets.iter().enumerate() {
        let rid = format!("r{}", idx + 2);
        let has_video = if asset.kind != AssetKind::Audio { "1" } else { "0" };
        let has_audio = if asset.kind == AssetKind::Audio { "1" } else { "0" };
        let dur_str = flicks_to_fcpxml_time(asset.duration_ticks.unwrap_or(project.timeline.duration_ticks));
        let src_url = path_to_file_url(&asset.path);
        let format_attr = if asset.kind != AssetKind::Audio {
            " format=\"r1\""
        } else {
            ""
        };
        let _ = writeln!(
            xml,
            "    <asset id=\"{}\" name=\"{}\" src=\"{}\" start=\"0s\" duration=\"{}\" hasVideo=\"{}\" hasAudio=\"{}\"{}>",
            xml_escape(&rid),
            xml_escape(&asset.id),
            xml_escape(&src_url),
            dur_str,
            has_video,
            has_audio,
            format_attr,
        );
        let _ = writeln!(
            xml,
            "      <media-rep kind=\"original-media\" src=\"{}\"/>",
            xml_escape(&src_url)
        );
        xml.push_str("    </asset>\n");
        asset_res_id.insert(asset.id.as_str(), rid);
    }
    xml.push_str("  </resources>\n");

    xml.push_str("  <library>\n");
    xml.push_str("    <event name=\"FrameShift Export\">\n");
    let _ = writeln!(xml, "      <project name=\"{}\">", xml_escape(&project.name));
    let _ = writeln!(
        xml,
        "        <sequence format=\"r1\" duration=\"{}\" tcStart=\"0s\" tcFormat=\"NDF\">",
        flicks_to_fcpxml_time(project.timeline.duration_ticks)
    );
    xml.push_str("          <spine>\n");

    let video_lanes = build_video_lanes(&project.timeline.clips);
    let primary = &video_lanes[0];
    let markers = collect_all_markers(project, opts);
    let mut cursor = Flicks::ZERO;
    let first_spine_offset = Flicks::ZERO;

    // Collect secondary video lanes + audio lanes so we can attach them as connected clips
    // onto the first primary spine element (standard FCPXML pattern).
    let mut connected_xml = String::new();
    for (lane_idx, lane) in video_lanes.iter().enumerate().skip(1) {
        let lane_num = lane_idx as i32;
        for clip in lane {
            let rid = asset_res_id
                .get(clip.asset_id.as_str())
                .map(String::as_str)
                .unwrap_or("r2");
            let dur = clip.timeline_end - clip.timeline_start;
            let _ = writeln!(
                connected_xml,
                "              <asset-clip ref=\"{}\" lane=\"{}\" offset=\"{}\" name=\"{}\" start=\"{}\" duration=\"{}\"/>",
                xml_escape(rid),
                lane_num,
                flicks_to_fcpxml_time(clip.timeline_start - first_spine_offset),
                xml_escape(&clip.id),
                flicks_to_fcpxml_time(clip.source_start),
                flicks_to_fcpxml_time(dur),
            );
        }
    }
    for (a_idx, ac) in project.timeline.audio_clips.iter().enumerate() {
        let rid = asset_res_id
            .get(ac.asset_id.as_str())
            .map(String::as_str)
            .unwrap_or("r2");
        let dur = ac.timeline_end - ac.timeline_start;
        let lane_num = -((a_idx as i32) + 1);
        let role = match ac.bus {
            AudioBus::Music => "music",
            AudioBus::Dialogue => "dialogue",
            _ => "effects",
        };
        let _ = writeln!(
            connected_xml,
            "              <asset-clip ref=\"{}\" lane=\"{}\" offset=\"{}\" name=\"{}\" start=\"{}\" duration=\"{}\" role=\"{}\">",
            xml_escape(rid),
            lane_num,
            flicks_to_fcpxml_time(ac.timeline_start - first_spine_offset),
            xml_escape(&ac.id),
            flicks_to_fcpxml_time(ac.source_start),
            flicks_to_fcpxml_time(dur),
            role,
        );
        if ac.gain_db.abs() > 1e-4 {
            let _ = writeln!(
                connected_xml,
                "                <adjust-volume amount=\"{:.3}dB\"/>",
                ac.gain_db
            );
        }
        connected_xml.push_str("              </asset-clip>\n");
    }

    let mut attached_extras = false;
    if primary.is_empty() {
        let _ = writeln!(
            xml,
            "            <gap name=\"Gap\" offset=\"0s\" start=\"0s\" duration=\"{}\">",
            flicks_to_fcpxml_time(project.timeline.duration_ticks)
        );
        xml.push_str(&connected_xml);
        for m in &markers {
            let _ = writeln!(
                xml,
                "              <marker start=\"{}\" duration=\"{}\" value=\"{}\"/>",
                flicks_to_fcpxml_time(m.start_tick),
                flicks_to_fcpxml_time(m.duration_ticks.max(frame_dur)),
                xml_escape(&m.label),
            );
        }
        xml.push_str("            </gap>\n");
    } else {
        for clip in primary {
            if clip.timeline_start > cursor {
                let gap = clip.timeline_start - cursor;
                if !attached_extras {
                    attached_extras = true;
                    let _ = writeln!(
                        xml,
                        "            <gap name=\"Gap\" offset=\"{}\" start=\"0s\" duration=\"{}\">",
                        flicks_to_fcpxml_time(cursor),
                        flicks_to_fcpxml_time(gap),
                    );
                    xml.push_str(&connected_xml);
                    xml.push_str("            </gap>\n");
                } else {
                    let _ = writeln!(
                        xml,
                        "            <gap name=\"Gap\" offset=\"{}\" start=\"0s\" duration=\"{}\"/>",
                        flicks_to_fcpxml_time(cursor),
                        flicks_to_fcpxml_time(gap),
                    );
                }
            }
            let rid = asset_res_id
                .get(clip.asset_id.as_str())
                .map(String::as_str)
                .unwrap_or("r2");
            let dur = clip.timeline_end - clip.timeline_start;
            let _ = writeln!(
                xml,
                "            <asset-clip ref=\"{}\" offset=\"{}\" name=\"{}\" start=\"{}\" duration=\"{}\" tcFormat=\"NDF\">",
                xml_escape(rid),
                flicks_to_fcpxml_time(clip.timeline_start),
                xml_escape(&clip.id),
                flicks_to_fcpxml_time(clip.source_start),
                flicks_to_fcpxml_time(dur),
            );
            if !attached_extras {
                attached_extras = true;
                xml.push_str(&connected_xml);
            }
            for m in &markers {
                if m.start_tick >= clip.timeline_start && m.start_tick < clip.timeline_end {
                    let local_m = clip.source_start + (m.start_tick - clip.timeline_start);
                    let _ = writeln!(
                        xml,
                        "              <marker start=\"{}\" duration=\"{}\" value=\"{}\"/>",
                        flicks_to_fcpxml_time(local_m),
                        flicks_to_fcpxml_time(m.duration_ticks.max(frame_dur)),
                        xml_escape(&m.label),
                    );
                }
            }
            xml.push_str("            </asset-clip>\n");
            cursor = clip.timeline_end;
        }
        if cursor < project.timeline.duration_ticks {
            let tail = project.timeline.duration_ticks - cursor;
            let _ = writeln!(
                xml,
                "            <gap name=\"Gap\" offset=\"{}\" start=\"0s\" duration=\"{}\"/>",
                flicks_to_fcpxml_time(cursor),
                flicks_to_fcpxml_time(tail),
            );
        }
    }

    xml.push_str("          </spine>\n");
    xml.push_str("        </sequence>\n");
    xml.push_str("      </project>\n");
    xml.push_str("    </event>\n");
    xml.push_str("  </library>\n");
    xml.push_str("</fcpxml>\n");
    Ok(xml)
}

/// Imports an FCPXML 1.10/1.11 document into a validated FrameShift [`Project`].
pub fn import_project_from_fcpxml(
    fcpxml: &str,
    opts: &NleImportOptions,
) -> Result<Project, SchemaError> {
    let root = parse_xml_document(fcpxml)?;
    if root.tag != "fcpxml" {
        return Err(SchemaError::InvalidNleDocument(format!(
            "expected <fcpxml> root, found <{}>",
            root.tag
        )));
    }

    let mut formats: BTreeMap<String, (Option<Rational32>, Option<Resolution>)> = BTreeMap::new();
    let mut xml_assets: BTreeMap<String, (String, Option<Flicks>, bool, bool)> = BTreeMap::new();

    if let Some(resources) = root.find_child("resources") {
        for child in &resources.children {
            match child.tag.as_str() {
                "format" => {
                    if let Some(id) = child.attr("id") {
                        let fps = child
                            .attr("frameDuration")
                            .and_then(|s| fcpxml_time_to_flicks(s).ok())
                            .and_then(fps_from_frame_duration);
                        let res = child
                            .attr("width")
                            .and_then(|w| w.parse::<u32>().ok())
                            .zip(child.attr("height").and_then(|h| h.parse::<u32>().ok()))
                            .filter(|&(w, h)| w > 0 && h > 0)
                            .map(|(width, height)| Resolution { width, height });
                        formats.insert(id.to_owned(), (fps, res));
                    }
                }
                "asset" => {
                    if let Some(id) = child.attr("id") {
                        let src = child
                            .find_child("media-rep")
                            .and_then(|mr| mr.attr("src"))
                            .or_else(|| child.attr("src"))
                            .unwrap_or("synthetic://imported");
                        let dur = child
                            .attr("duration")
                            .and_then(|d| fcpxml_time_to_flicks(d).ok())
                            .filter(|d| *d > Flicks::ZERO);
                        let has_video = child.attr("hasVideo").unwrap_or("1") != "0";
                        let has_audio = child.attr("hasAudio").unwrap_or("0") == "1";
                        xml_assets.insert(
                            id.to_owned(),
                            (file_url_to_path(src), dur, has_video, has_audio),
                        );
                    }
                }
                _ => {}
            }
        }
    }

    let sequence = find_descendant(&root, "sequence")
        .ok_or_else(|| SchemaError::InvalidNleDocument("missing <sequence> element".into()))?;
    let seq_fmt = sequence
        .attr("format")
        .and_then(|fid| formats.get(fid))
        .cloned()
        .unwrap_or((None, None));
    let fps = opts
        .fps_override
        .or(seq_fmt.0)
        .unwrap_or(FPS_24);
    let resolution = opts.resolution_override.or(seq_fmt.1).unwrap_or(Resolution {
        width: 1920,
        height: 1080,
    });

    let project_name = opts
        .project_name
        .clone()
        .or_else(|| {
            find_descendant(&root, "project")
                .and_then(|p| p.attr("name"))
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "Imported FCPXML Project".to_owned());

    let spine = sequence
        .find_child("spine")
        .ok_or_else(|| SchemaError::InvalidNleDocument("missing <spine> element".into()))?;

    let mut seen_ids = BTreeSet::new();
    let mut assets_by_key: BTreeMap<String, String> = BTreeMap::new();
    let mut assets = Vec::new();
    let mut video_clips = Vec::new();
    let mut audio_clips = Vec::new();
    let mut events = Vec::new();

    let mut spine_cursor = Flicks::ZERO;
    for elem in &spine.children {
        let dur = elem
            .attr("duration")
            .map(fcpxml_time_to_flicks)
            .transpose()?
            .unwrap_or(Flicks::ZERO);
        let offset = elem
            .attr("offset")
            .map(fcpxml_time_to_flicks)
            .transpose()?
            .unwrap_or(spine_cursor);
        let src_start = elem
            .attr("start")
            .map(fcpxml_time_to_flicks)
            .transpose()?
            .unwrap_or(Flicks::ZERO);

        if elem.tag == "asset-clip" || elem.tag == "clip" {
            let ref_id = elem.attr("ref").unwrap_or("r2");
            let is_audio_only = elem.attr("lane").is_some_and(|l| l.starts_with('-'))
                || elem.attr("role").is_some_and(|r| r == "music" || r == "dialogue");
            let asset_id = ensure_fcpxml_asset(
                ref_id,
                is_audio_only,
                fps,
                resolution,
                &xml_assets,
                &mut assets_by_key,
                &mut seen_ids,
                &mut assets,
            );
            let raw_name = elem.attr("name").unwrap_or("clip");
            let clip_id = unique_id(&mut seen_ids, raw_name);
            if dur > Flicks::ZERO {
                if is_audio_only {
                    audio_clips.push(AudioClipDef {
                        id: clip_id,
                        asset_id,
                        timeline_start: offset,
                        timeline_end: offset + dur,
                        source_start: src_start,
                        gain_db: parse_adjust_volume(elem),
                        fade_in: None,
                        fade_out: None,
                        bus: parse_audio_bus_name(elem.attr("role").unwrap_or("music")),
                        pan: 0.0,
                        anchor_event_id: None,
                        apex_offset_ticks: None,
                    });
                } else {
                    video_clips.push(ClipDef {
                        id: clip_id,
                        asset_id,
                        timeline_start: offset,
                        timeline_end: offset + dur,
                        source_start: src_start,
                        speed_curve: SpeedCurve::constant(1.0).unwrap(),
                        linked_event_id: None,
                        event_animations: Vec::new(),
                    });
                }
            }
        }

        // Attached lanes and markers inside this spine item
        for child in &elem.children {
            match child.tag.as_str() {
                "marker" => {
                    let m_start = child
                        .attr("start")
                        .map(fcpxml_time_to_flicks)
                        .transpose()?
                        .unwrap_or(Flicks::ZERO);
                    let out_tick = if elem.tag == "gap" {
                        m_start
                    } else {
                        offset + (m_start - src_start)
                    };
                    let label = child.attr("value").unwrap_or("marker").to_owned();
                    let ev_id = unique_id(&mut seen_ids, &format!("ev_{}", events.len() + 1));
                    events.push(EventAnchorDef {
                        event_id: ev_id,
                        label,
                        source_clip_id: None,
                        source_anchor_tick: None,
                        target_output_tick: out_tick,
                        origin: EventOrigin::Detected,
                        timing: EventTimingConstraint::Absolute,
                    });
                }
                "asset-clip" | "clip" => {
                    let c_dur = child
                        .attr("duration")
                        .map(fcpxml_time_to_flicks)
                        .transpose()?
                        .unwrap_or(Flicks::ZERO);
                    if c_dur <= Flicks::ZERO {
                        continue;
                    }
                    let c_off = child
                        .attr("offset")
                        .map(fcpxml_time_to_flicks)
                        .transpose()?
                        .unwrap_or(Flicks::ZERO);
                    let c_start = child
                        .attr("start")
                        .map(fcpxml_time_to_flicks)
                        .transpose()?
                        .unwrap_or(Flicks::ZERO);
                    let abs_start = offset + c_off;
                    let ref_id = child.attr("ref").unwrap_or("r2");
                    let is_audio = child.attr("lane").is_some_and(|l| l.starts_with('-'))
                        || xml_assets
                            .get(ref_id)
                            .is_some_and(|(_, _, has_v, has_a)| !has_v && *has_a);
                    let asset_id = ensure_fcpxml_asset(
                        ref_id,
                        is_audio,
                        fps,
                        resolution,
                        &xml_assets,
                        &mut assets_by_key,
                        &mut seen_ids,
                        &mut assets,
                    );
                    let clip_id = unique_id(&mut seen_ids, child.attr("name").unwrap_or("clip"));
                    if is_audio {
                        audio_clips.push(AudioClipDef {
                            id: clip_id,
                            asset_id,
                            timeline_start: abs_start,
                            timeline_end: abs_start + c_dur,
                            source_start: c_start,
                            gain_db: parse_adjust_volume(child),
                            fade_in: None,
                            fade_out: None,
                            bus: parse_audio_bus_name(child.attr("role").unwrap_or("music")),
                            pan: 0.0,
                            anchor_event_id: None,
                            apex_offset_ticks: None,
                        });
                    } else {
                        video_clips.push(ClipDef {
                            id: clip_id,
                            asset_id,
                            timeline_start: abs_start,
                            timeline_end: abs_start + c_dur,
                            source_start: c_start,
                            speed_curve: SpeedCurve::constant(1.0).unwrap(),
                            linked_event_id: None,
                            event_animations: Vec::new(),
                        });
                    }
                }
                _ => {}
            }
        }

        spine_cursor = offset + dur;
    }

    let seq_dur = sequence
        .attr("duration")
        .and_then(|d| fcpxml_time_to_flicks(d).ok())
        .filter(|d| *d > Flicks::ZERO)
        .unwrap_or(spine_cursor.max(Flicks(FLICKS_PER_SECOND)));

    build_imported_project(
        project_name,
        fps,
        resolution,
        seq_dur,
        assets,
        video_clips,
        audio_clips,
        events,
    )
}

#[allow(clippy::too_many_arguments)]
fn ensure_fcpxml_asset(
    ref_id: &str,
    is_audio: bool,
    fps: Rational32,
    resolution: Resolution,
    xml_assets: &BTreeMap<String, (String, Option<Flicks>, bool, bool)>,
    assets_by_key: &mut BTreeMap<String, String>,
    seen_ids: &mut BTreeSet<String>,
    assets: &mut Vec<AssetDef>,
) -> String {
    let (path, dur, _, _) = xml_assets
        .get(ref_id)
        .cloned()
        .unwrap_or_else(|| (format!("synthetic://{ref_id}"), None, !is_audio, is_audio));
    let kind = if is_audio {
        AssetKind::Audio
    } else {
        AssetKind::Video
    };
    let key = format!("{}:{path}", if is_audio { "A" } else { "V" });
    if let Some(existing) = assets_by_key.get(&key) {
        return existing.clone();
    }
    let id = unique_id(seen_ids, &format!("asset_{ref_id}"));
    assets_by_key.insert(key, id.clone());
    assets.push(AssetDef {
        id: id.clone(),
        kind,
        path,
        duration_ticks: dur,
        fps: if is_audio { None } else { Some(fps) },
        sample_rate_hz: if is_audio { Some(48_000) } else { None },
        resolution: if is_audio { None } else { Some(resolution) },
    });
    id
}

fn parse_adjust_volume(elem: &XmlNode) -> f32 {
    elem.find_child("adjust-volume")
        .and_then(|v| v.attr("amount"))
        .and_then(|a| a.strip_suffix("dB").or(Some(a)))
        .and_then(|s| s.trim().parse::<f32>().ok())
        .filter(|v| v.is_finite())
        .unwrap_or(0.0)
}

#[allow(clippy::too_many_arguments)]
fn build_imported_project(
    name: String,
    fps: Rational32,
    resolution: Resolution,
    duration_ticks: Flicks,
    assets: Vec<AssetDef>,
    clips: Vec<ClipDef>,
    audio_clips: Vec<AudioClipDef>,
    events: Vec<EventAnchorDef>,
) -> Result<Project, SchemaError> {
    let mut nodes: Vec<GraphNodeDef> = Vec::new();
    let mut prev_node: Option<NodeId> = None;
    for (idx, clip) in clips.iter().enumerate() {
        let src_id = NodeId::new(format!("src_{idx}_{}", clip.id));
        nodes.push(GraphNodeDef {
            id: src_id.clone(),
            kind: NodeKind::SourceVideo {
                asset_id: clip.asset_id.clone(),
                clip_id: Some(clip.id.clone()),
                color_space: ColorSpace::DisplaySrgb,
                fit: SourceFit::Cover,
            },
            inputs: Vec::new(),
            enabled: true,
        });
        prev_node = Some(if let Some(prev) = prev_node {
            let blend_id = NodeId::new(format!("blend_{idx}_{}", clip.id));
            nodes.push(GraphNodeDef {
                id: blend_id.clone(),
                kind: NodeKind::Blend {
                    mode: BlendMode::Normal,
                    opacity: 1.0,
                },
                inputs: vec![prev, src_id],
                enabled: true,
            });
            blend_id
        } else {
            src_id
        });
    }

    let output_node = prev_node.unwrap_or_else(|| {
        let fallback_id = NodeId::new("bg_solid");
        nodes.push(GraphNodeDef {
            id: fallback_id.clone(),
            kind: NodeKind::Solid {
                color_rgba: [0.0, 0.0, 0.0, 1.0],
                color_space: ColorSpace::LinearRec709,
            },
            inputs: Vec::new(),
            enabled: true,
        });
        fallback_id
    });

    let project = Project {
        schema_version: CURRENT_SCHEMA_VERSION,
        revision_id: 1,
        name,
        assets,
        timeline: TimelineDef {
            fps,
            duration_ticks,
            resolution,
            clips,
            audio_clips,
            ducking_rules: Vec::new(),
            micro_silences: Vec::new(),
        },
        events,
        graph: RenderGraphDef { output_node, nodes },
        render_profiles: RenderProfilesDef {
            draft: RenderProfileDef {
                kind: RenderProfileKind::Draft,
                resolution: Resolution {
                    width: (resolution.width / 2).max(2),
                    height: (resolution.height / 2).max(2),
                },
                motion_blur_sub_samples: 4,
                shutter_angle_deg: 180.0,
                pixel_format: WorkingPixelFormat::Rgba16FloatPremul,
            },
            final_profile: RenderProfileDef {
                kind: RenderProfileKind::Final,
                resolution,
                motion_blur_sub_samples: 16,
                shutter_angle_deg: 180.0,
                pixel_format: WorkingPixelFormat::Rgba16FloatPremul,
            },
        },
        param_bindings: Vec::new(),
        keyframe_tracks: Vec::new(),
        creative_decisions: Vec::new(),
        preferences: Vec::new(),
        proxy_config: ProxyConfigDef::default(),
        hw_accel_policy: HwAccelPolicy::Auto,
    };

    project.validate()?;
    Ok(project)
}

fn fps_from_rate_f64(rate: f64) -> Option<Rational32> {
    if !rate.is_finite() || rate <= 0.0 {
        return None;
    }
    if (rate - 24000.0 / 1001.0).abs() < 1e-4 {
        Rational32::new(24000, 1001).ok()
    } else if (rate - 30000.0 / 1001.0).abs() < 1e-4 {
        Rational32::new(30000, 1001).ok()
    } else if (rate - 60000.0 / 1001.0).abs() < 1e-4 {
        Rational32::new(60000, 1001).ok()
    } else {
        let r = rate.round() as u32;
        if r > 0 {
            Rational32::new(r, 1).ok()
        } else {
            None
        }
    }
}

fn fps_from_frame_duration(dur: Flicks) -> Option<Rational32> {
    if dur.0 <= 0 {
        return None;
    }
    let rate = FLICKS_PER_SECOND as f64 / dur.0 as f64;
    fps_from_rate_f64(rate)
}

fn unique_id(seen: &mut BTreeSet<String>, base: &str) -> String {
    let clean: String = base
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect();
    let stem = if clean.is_empty() { "id" } else { clean.as_str() };
    if seen.insert(stem.to_owned()) {
        return stem.to_owned();
    }
    let mut n = 2usize;
    loop {
        let cand = format!("{stem}_{n}");
        if seen.insert(cand.clone()) {
            return cand;
        }
        n += 1;
    }
}

fn xml_escape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    out
}

fn xml_unescape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        if let Some(semi) = after.find(';') {
            let ent = &after[..semi];
            match ent {
                "amp" => out.push('&'),
                "lt" => out.push('<'),
                "gt" => out.push('>'),
                "quot" => out.push('"'),
                "apos" => out.push('\''),
                _ => {
                    out.push('&');
                    out.push_str(ent);
                    out.push(';');
                }
            }
            rest = &after[semi + 1..];
        } else {
            out.push('&');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

#[derive(Debug, Clone)]
struct XmlNode {
    tag: String,
    attrs: BTreeMap<String, String>,
    children: Vec<XmlNode>,
}

impl XmlNode {
    fn attr(&self, key: &str) -> Option<&str> {
        self.attrs.get(key).map(String::as_str)
    }

    fn find_child(&self, tag: &str) -> Option<&XmlNode> {
        self.children.iter().find(|c| c.tag == tag)
    }
}

fn find_descendant<'a>(node: &'a XmlNode, tag: &str) -> Option<&'a XmlNode> {
    if node.tag == tag {
        return Some(node);
    }
    for child in &node.children {
        if let Some(found) = find_descendant(child, tag) {
            return Some(found);
        }
    }
    None
}

fn parse_xml_document(input: &str) -> Result<XmlNode, SchemaError> {
    // Reject external entity expansion / external DTDs for safety.
    let upper = input.to_ascii_uppercase();
    if upper.contains("<!ENTITY")
        || (upper.contains("<!DOCTYPE") && (upper.contains(" SYSTEM ") || upper.contains(" PUBLIC ")))
    {
        return Err(SchemaError::InvalidNleDocument(
            "external XML entities or SYSTEM/PUBLIC DOCTYPE declarations are rejected".into(),
        ));
    }

    let bytes = input.as_bytes();
    let mut pos = 0usize;
    let mut stack: Vec<XmlNode> = Vec::new();
    let mut root: Option<XmlNode> = None;

    while pos < bytes.len() {
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= bytes.len() {
            break;
        }
        if bytes[pos] != b'<' {
            // Skip text content between tags.
            while pos < bytes.len() && bytes[pos] != b'<' {
                pos += 1;
            }
            continue;
        }
        if input[pos..].starts_with("<?") {
            let end = input[pos + 2..].find("?>").ok_or_else(|| {
                SchemaError::InvalidNleDocument("unterminated XML processing instruction".into())
            })?;
            pos += 2 + end + 2;
            continue;
        }
        if input[pos..].starts_with("<!--") {
            let end = input[pos + 4..]
                .find("-->")
                .ok_or_else(|| SchemaError::InvalidNleDocument("unterminated XML comment".into()))?;
            pos += 4 + end + 3;
            continue;
        }
        if input[pos..].starts_with("<!") {
            let end = input[pos + 2..].find('>').ok_or_else(|| {
                SchemaError::InvalidNleDocument("unterminated XML declaration".into())
            })?;
            pos += 2 + end + 1;
            continue;
        }
        if input[pos..].starts_with("</") {
            let end = input[pos + 2..].find('>').ok_or_else(|| {
                SchemaError::InvalidNleDocument("unterminated XML closing tag".into())
            })?;
            let tag = input[pos + 2..pos + 2 + end].trim();
            let popped = stack.pop().ok_or_else(|| {
                SchemaError::InvalidNleDocument(format!("unexpected closing tag '</{tag}>'"))
            })?;
            if popped.tag != tag {
                return Err(SchemaError::InvalidNleDocument(format!(
                    "mismatched closing tag '</{tag}>' (expected '</{}>')",
                    popped.tag
                )));
            }
            if let Some(parent) = stack.last_mut() {
                parent.children.push(popped);
            } else {
                root = Some(popped);
            }
            pos += 2 + end + 1;
            continue;
        }

        // Opening or self-closing tag
        pos += 1; // skip '<'
        let tag_start = pos;
        while pos < bytes.len()
            && !bytes[pos].is_ascii_whitespace()
            && bytes[pos] != b'>'
            && bytes[pos] != b'/'
        {
            pos += 1;
        }
        let tag = input[tag_start..pos].to_owned();
        if tag.is_empty() {
            return Err(SchemaError::InvalidNleDocument("empty XML tag name".into()));
        }

        let mut attrs = BTreeMap::new();
        let mut self_closing = false;
        loop {
            while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
                pos += 1;
            }
            if pos >= bytes.len() {
                return Err(SchemaError::InvalidNleDocument(format!(
                    "unterminated tag <{tag}>"
                )));
            }
            if bytes[pos] == b'/' && bytes.get(pos + 1) == Some(&b'>') {
                self_closing = true;
                pos += 2;
                break;
            }
            if bytes[pos] == b'>' {
                pos += 1;
                break;
            }
            let attr_start = pos;
            while pos < bytes.len()
                && !bytes[pos].is_ascii_whitespace()
                && bytes[pos] != b'='
                && bytes[pos] != b'>'
            {
                pos += 1;
            }
            let attr_name = input[attr_start..pos].to_owned();
            while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
                pos += 1;
            }
            if bytes.get(pos) != Some(&b'=') {
                return Err(SchemaError::InvalidNleDocument(format!(
                    "expected '=' after attribute '{attr_name}' in <{tag}>"
                )));
            }
            pos += 1;
            while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
                pos += 1;
            }
            let quote = bytes.get(pos).copied().filter(|&q| q == b'"' || q == b'\'').ok_or_else(
                || {
                    SchemaError::InvalidNleDocument(format!(
                        "expected quoted value for attribute '{attr_name}' in <{tag}>"
                    ))
                },
            )?;
            pos += 1;
            let val_start = pos;
            while pos < bytes.len() && bytes[pos] != quote {
                pos += 1;
            }
            if pos >= bytes.len() {
                return Err(SchemaError::InvalidNleDocument(format!(
                    "unterminated attribute value for '{attr_name}'"
                )));
            }
            let attr_val = xml_unescape(&input[val_start..pos]);
            pos += 1;
            attrs.insert(attr_name, attr_val);
        }

        let node = XmlNode {
            tag,
            attrs,
            children: Vec::new(),
        };
        if self_closing {
            if let Some(parent) = stack.last_mut() {
                parent.children.push(node);
            } else {
                root = Some(node);
            }
        } else {
            stack.push(node);
        }
    }

    if !stack.is_empty() {
        return Err(SchemaError::InvalidNleDocument(format!(
            "unclosed XML tag <{}>",
            stack.last().unwrap().tag
        )));
    }
    root.ok_or_else(|| SchemaError::InvalidNleDocument("empty XML document".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use frameshift_time::{FPS_23_976, FPS_24, FPS_60};

    #[test]
    fn flicks_otio_and_fcpxml_time_conversions_are_lossless() {
        // 1 frame @ 23.976 (24000/1001) = 29_429_400 Flicks -> "1001/24000s"
        let ntsc_frame = FPS_23_976.frame_duration().unwrap();
        assert_eq!(flicks_to_fcpxml_time(ntsc_frame), "1001/24000s");
        assert_eq!(fcpxml_time_to_flicks("1001/24000s").unwrap(), ntsc_frame);

        // 1 frame @ 60fps = 11_760_000 Flicks -> "1/60s"
        let sixty = FPS_60.frame_duration().unwrap();
        assert_eq!(flicks_to_fcpxml_time(sixty), "1/60s");
        assert_eq!(fcpxml_time_to_flicks("1/60s").unwrap(), sixty);

        // 1 sample @ 48 kHz = 14_700 Flicks -> "1/48000s"
        let sample_48k = Flicks(14_700);
        assert_eq!(flicks_to_fcpxml_time(sample_48k), "1/48000s");
        assert_eq!(fcpxml_time_to_flicks("1/48000s").unwrap(), sample_48k);

        // OTIO round-trip on 24fps & 23.976fps
        let t24 = Flicks(FPS_24.frame_duration().unwrap().0 * 120);
        let rt24 = flicks_to_otio_rational_time(t24, FPS_24);
        assert_eq!(rt24.value, 120.0);
        assert_eq!(otio_rational_time_to_flicks(&rt24).unwrap(), t24);

        let t_ntsc = Flicks(ntsc_frame.0 * 100);
        let rt_ntsc = flicks_to_otio_rational_time(t_ntsc, FPS_23_976);
        assert_eq!(rt_ntsc.value, 100.0);
        assert_eq!(otio_rational_time_to_flicks(&rt_ntsc).unwrap(), t_ntsc);
    }

    #[test]
    fn otio_and_fcpxml_export_import_roundtrip_golden_scene() {
        let project = Project::golden_scene_preset();
        let opts = NleExportOptions {
            extra_markers: vec![NleMarker {
                label: "Chorus Downbeat".into(),
                start_tick: Flicks(FLICKS_PER_SECOND),
                duration_ticks: Flicks(11_760_000),
                kind: NleMarkerKind::Downbeat,
            }],
        };

        let otio_str = export_project_to_otio_json(&project, &opts).expect("export otio");
        let from_otio =
            import_project_from_otio_json(&otio_str, &NleImportOptions::default()).expect("import otio");
        assert_eq!(from_otio.timeline.clips.len(), 2);
        assert_eq!(from_otio.timeline.audio_clips.len(), 1);
        assert_eq!(from_otio.events.len(), 2);
        assert_eq!(from_otio.timeline.duration_ticks, project.timeline.duration_ticks);

        let fcpxml_str = export_project_to_fcpxml(&project, &opts).expect("export fcpxml");
        assert!(fcpxml_str.contains("<!DOCTYPE fcpxml>"));
        assert!(fcpxml_str.contains("version=\"1.11\""));
        let from_fcpxml =
            import_project_from_fcpxml(&fcpxml_str, &NleImportOptions::default()).expect("import fcpxml");
        assert_eq!(from_fcpxml.timeline.clips.len(), 2);
        assert_eq!(from_fcpxml.timeline.audio_clips.len(), 1);
        assert!(!from_fcpxml.events.is_empty());
    }

    #[test]
    fn fcpxml_reader_rejects_external_entities() {
        let malicious = r#"<?xml version="1.0"?>
<!DOCTYPE fcpxml [<!ENTITY xxe SYSTEM "file:///etc/passwd">]>
<fcpxml version="1.11"/>"#;
        assert!(matches!(
            import_project_from_fcpxml(malicious, &NleImportOptions::default()),
            Err(SchemaError::InvalidNleDocument(_))
        ));
    }
}
