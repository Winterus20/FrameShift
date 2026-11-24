//! Zero-dependency declarative `Motion.md` / `.fsmotion` scene recipe AST and parser.
//!
//! Compiles human/agent-readable declarative edit scripts into an intermediate AST
//! (`MotionDoc`) that downstream compilers (`frameshift-core`) lower deterministically
//! to `schema_version = "0.2.0"` (`i64` Flicks) `Project` JSON.

use crate::error::SchemaError;
use crate::graph::{ShutterPhase, SourceFit};
use crate::project::{AssetKind, AudioBus, Resolution};
use frameshift_time::{FLICKS_PER_SECOND, FPS_24, Flicks, Rational32};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// LUT declaration parsed from frontmatter (`lut: anime_cyber_contrast(0.65)`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MotionLutSpec {
    pub preset: String,
    pub intensity: f32,
}

/// Frontmatter metadata header for a `Motion.md` document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MotionMeta {
    pub name: String,
    pub fps: Rational32,
    pub resolution: Resolution,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bpm: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lut: Option<MotionLutSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bg_color: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, String>,
}

impl Default for MotionMeta {
    fn default() -> Self {
        Self {
            name: "Untitled Motion".to_owned(),
            fps: FPS_24,
            resolution: Resolution {
                width: 1920,
                height: 1080,
            },
            bpm: None,
            lut: None,
            bg_color: None,
            extra: BTreeMap::new(),
        }
    }
}

/// Time token inside a `Motion.md` directive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum MotionTimeExpr {
    /// Decimal seconds (`2.5s`), quantized to the frame grid by the compiler.
    Seconds(f64),
    /// Frame index (`f:60`).
    Frame(i64),
    /// Beat index (`beat:4`) resolved via `MultibandBeatReport` or `meta.bpm`.
    Beat(i64),
    /// Downbeat bar index (`bar:2`) resolved via `MultibandBeatReport` or `meta.bpm`.
    Bar(i64),
    /// Reference to an earlier `beat_drop` identifier (`@drop_1`).
    EventRef(String),
    /// Raw integer Flicks (`1764000000t`).
    Ticks(i64),
}

impl MotionTimeExpr {
    /// Parses a single time token (`2.5s`, `f:60`, `beat:4`, `bar:2`, `@drop_1`, `1764000000t`).
    pub fn parse(raw: &str, line: usize) -> Result<Self, SchemaError> {
        let s = raw.trim();
        let err = |msg: String| SchemaError::InvalidMotionDsl { line, message: msg };
        if s.is_empty() {
            return Err(err("empty time expression".into()));
        }
        if let Some(ev) = s.strip_prefix('@') {
            if ev.is_empty() {
                return Err(err("empty event reference '@' in time expression".into()));
            }
            return Ok(Self::EventRef(ev.to_owned()));
        }
        if let Some(frame_str) = s.strip_prefix("f:") {
            let f: i64 = frame_str
                .parse()
                .map_err(|_| err(format!("invalid frame index '{s}'")))?;
            return Ok(Self::Frame(f));
        }
        if let Some(beat_str) = s.strip_prefix("beat:") {
            let b: i64 = beat_str
                .parse()
                .map_err(|_| err(format!("invalid beat index '{s}'")))?;
            return Ok(Self::Beat(b));
        }
        if let Some(bar_str) = s.strip_prefix("bar:") {
            let b: i64 = bar_str
                .parse()
                .map_err(|_| err(format!("invalid bar index '{s}'")))?;
            return Ok(Self::Bar(b));
        }
        if let Some(tick_str) = s.strip_suffix('t') {
            let t: i64 = tick_str
                .parse()
                .map_err(|_| err(format!("invalid Flicks literal '{s}'")))?;
            return Ok(Self::Ticks(t));
        }
        if let Some(sec_str) = s.strip_suffix('s') {
            let secs: f64 = sec_str
                .parse()
                .map_err(|_| err(format!("invalid seconds literal '{s}'")))?;
            if !secs.is_finite() {
                return Err(err(format!("non-finite seconds literal '{s}'")));
            }
            return Ok(Self::Seconds(secs));
        }
        Err(err(format!(
            "unrecognized time expression '{s}' (expected <num>s, f:<int>, beat:<int>, bar:<int>, @<event>, or <int>t)"
        )))
    }

    /// Resolves static time expressions (`Seconds`, `Frame`, `Ticks`, and `Beat`/`Bar` when
    /// `bpm` is supplied) to frame-quantized integer `Flicks`.
    pub fn to_ticks_static(
        &self,
        fps: Rational32,
        bpm: Option<f64>,
        events: &BTreeMap<String, Flicks>,
    ) -> Result<Flicks, SchemaError> {
        let frame_ticks = fps.frame_duration()?.0;
        let quantize_sec = |secs: f64| -> Result<Flicks, SchemaError> {
            let raw = (secs * FLICKS_PER_SECOND as f64).round();
            if !raw.is_finite() || raw < i64::MIN as f64 || raw > i64::MAX as f64 {
                return Err(SchemaError::InvalidMotionDsl {
                    line: 0,
                    message: format!("seconds value {secs} overflows Flicks"),
                });
            }
            let frames = ((raw as i128 + (frame_ticks as i128 / 2) * (raw as i128).signum())
                / frame_ticks as i128)
                * frame_ticks as i128;
            i64::try_from(frames).map(Flicks).map_err(|_| {
                SchemaError::InvalidMotionDsl {
                    line: 0,
                    message: "quantized Flicks overflow".into(),
                }
            })
        };

        match self {
            Self::Ticks(t) => Ok(Flicks(*t)),
            Self::Frame(f) => {
                let total = (*f as i128).checked_mul(frame_ticks as i128).ok_or_else(|| {
                    SchemaError::InvalidMotionDsl {
                        line: 0,
                        message: "frame index overflow".into(),
                    }
                })?;
                i64::try_from(total).map(Flicks).map_err(|_| {
                    SchemaError::InvalidMotionDsl {
                        line: 0,
                        message: "frame Flicks overflow".into(),
                    }
                })
            }
            Self::Seconds(s) => quantize_sec(*s),
            Self::Beat(b) => {
                let bpm = bpm.filter(|v| v.is_finite() && *v > 0.0).ok_or_else(|| {
                    SchemaError::InvalidMotionDsl {
                        line: 0,
                        message: "beat:<n> requires frontmatter 'bpm' or external beat report".into(),
                    }
                })?;
                quantize_sec((*b as f64) * (60.0 / bpm))
            }
            Self::Bar(bar) => {
                let bpm = bpm.filter(|v| v.is_finite() && *v > 0.0).ok_or_else(|| {
                    SchemaError::InvalidMotionDsl {
                        line: 0,
                        message: "bar:<n> requires frontmatter 'bpm' or external beat report".into(),
                    }
                })?;
                quantize_sec((*bar as f64) * 4.0 * (60.0 / bpm))
            }
            Self::EventRef(id) => events.get(id).copied().ok_or_else(|| {
                SchemaError::MissingEvent(id.clone())
            }),
        }
    }
}

/// Half-open `[start, end)` time range in `Motion.md`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MotionRangeExpr {
    pub start: MotionTimeExpr,
    pub end: MotionTimeExpr,
}

impl MotionRangeExpr {
    pub fn parse(raw: &str, line: usize) -> Result<Self, SchemaError> {
        let Some((lhs, rhs)) = raw.split_once("..") else {
            return Err(SchemaError::InvalidMotionDsl {
                line,
                message: format!("expected range '<start>..<end>', got '{raw}'"),
            });
        };
        Ok(Self {
            start: MotionTimeExpr::parse(lhs, line)?,
            end: MotionTimeExpr::parse(rhs, line)?,
        })
    }
}

/// Parsed directive statement in a `Motion.md` edit script.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "directive", rename_all = "snake_case")]
pub enum MotionDirective {
    Asset {
        id: String,
        kind: AssetKind,
        path: String,
    },
    Audio {
        id: String,
        asset_id: String,
        range: MotionRangeExpr,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        src: Option<MotionTimeExpr>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bus: Option<AudioBus>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        gain_db: Option<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fade_in: Option<MotionTimeExpr>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fade_out: Option<MotionTimeExpr>,
    },
    Shot {
        id: String,
        asset_id: String,
        range: MotionRangeExpr,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        src: Option<MotionTimeExpr>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fit: Option<SourceFit>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        speed: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        anchor: Option<String>,
    },
    BeatDrop {
        id: String,
        at: MotionTimeExpr,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        clip_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        src: Option<MotionTimeExpr>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
    },
    Transition {
        shader: String,
        from_clip: String,
        to_clip: String,
        at: MotionTimeExpr,
        dur: MotionTimeExpr,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        params: BTreeMap<String, String>,
    },
    Fx {
        preset_or_kind: String,
        on: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        at: Option<MotionTimeExpr>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dur: Option<MotionTimeExpr>,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        params: BTreeMap<String, String>,
    },
    Text {
        text: String,
        range: MotionRangeExpr,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pos: Option<[f32; 2]>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        size: Option<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        color: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        font: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        outline: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        anim: Option<String>,
    },
    Shutter {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        angle: Option<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        samples: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        phase: Option<ShutterPhase>,
    },
}

/// Complete parsed `Motion.md` AST.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MotionDoc {
    pub meta: MotionMeta,
    pub directives: Vec<MotionDirective>,
}

impl MotionDoc {
    /// Parses a `Motion.md` / `.fsmotion` document into a [`MotionDoc`] AST.
    pub fn parse(input: &str) -> Result<Self, SchemaError> {
        parse_motion_doc(input)
    }
}

/// Parses a `Motion.md` / `.fsmotion` document into a [`MotionDoc`] AST.
pub fn parse_motion_doc(input: &str) -> Result<MotionDoc, SchemaError> {
    let mut meta = MotionMeta::default();
    let mut directives = Vec::new();
    let mut lines = input.lines().enumerate().peekable();

    // Skip leading blank lines or comments before optional frontmatter.
    while let Some(&(_, raw)) = lines.peek() {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
            lines.next();
        } else {
            break;
        }
    }

    if let Some(&(start_idx, first_line)) = lines.peek()
        && first_line.trim() == "---"
    {
        lines.next();
        let mut closed = false;
        for (idx, raw) in lines.by_ref() {
            let line_no = idx + 1;
            let trimmed = raw.trim();
            if trimmed == "---" {
                closed = true;
                break;
            }
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
                continue;
            }
            let Some((key, val)) = trimmed.split_once(':') else {
                return Err(SchemaError::InvalidMotionDsl {
                    line: line_no,
                    message: format!("expected 'key: value' in frontmatter, found '{trimmed}'"),
                });
            };
            parse_meta_entry(&mut meta, key.trim(), unquote_if_quoted(val.trim()), line_no)?;
        }
        if !closed {
            return Err(SchemaError::InvalidMotionDsl {
                line: start_idx + 1,
                message: "unclosed frontmatter block (missing closing '---')".into(),
            });
        }
    }

    for (idx, raw) in lines {
        let line_no = idx + 1;
        let stripped = strip_inline_comment(raw).trim();
        if stripped.is_empty() {
            continue;
        }
        let tokens = tokenize_directive_line(stripped, line_no)?;
        if tokens.is_empty() {
            continue;
        }
        directives.push(parse_directive(&tokens, line_no)?);
    }

    Ok(MotionDoc { meta, directives })
}

fn parse_meta_entry(
    meta: &mut MotionMeta,
    key: &str,
    val: String,
    line: usize,
) -> Result<(), SchemaError> {
    let err = |msg: String| SchemaError::InvalidMotionDsl { line, message: msg };
    match key {
        "name" => meta.name = val,
        "fps" => meta.fps = parse_fps_spec(&val).map_err(err)?,
        "res" | "resolution" => meta.resolution = parse_resolution_spec(&val).map_err(err)?,
        "bpm" => {
            let bpm: f64 = val
                .parse()
                .map_err(|_| err(format!("invalid bpm '{val}'")))?;
            if !bpm.is_finite() || bpm <= 0.0 {
                return Err(err(format!("bpm must be finite and > 0, got {bpm}")));
            }
            meta.bpm = Some(bpm);
        }
        "lut" => meta.lut = Some(parse_lut_spec(&val).map_err(err)?),
        "bg_color" => meta.bg_color = Some(val),
        other => {
            meta.extra.insert(other.to_owned(), val);
        }
    }
    Ok(())
}

fn parse_fps_spec(raw: &str) -> Result<Rational32, String> {
    if let Some((num_s, den_s)) = raw.split_once('/') {
        let num: u32 = num_s
            .trim()
            .parse()
            .map_err(|_| format!("invalid fps numerator '{raw}'"))?;
        let den: u32 = den_s
            .trim()
            .parse()
            .map_err(|_| format!("invalid fps denominator '{raw}'"))?;
        if num == 0 || den == 0 {
            return Err("fps numerator and denominator must be non-zero".into());
        }
        return Rational32::new(num, den).map_err(|e| e.to_string());
    }
    let num: u32 = raw
        .trim()
        .parse()
        .map_err(|_| format!("invalid fps '{raw}'"))?;
    Rational32::new(num, 1).map_err(|e| e.to_string())
}

fn parse_resolution_spec(raw: &str) -> Result<Resolution, String> {
    let Some((w_s, h_s)) = raw.split_once('x').or_else(|| raw.split_once('X')) else {
        return Err(format!("expected resolution '<width>x<height>', got '{raw}'"));
    };
    let width: u32 = w_s
        .trim()
        .parse()
        .map_err(|_| format!("invalid resolution width '{raw}'"))?;
    let height: u32 = h_s
        .trim()
        .parse()
        .map_err(|_| format!("invalid resolution height '{raw}'"))?;
    if width == 0 || height == 0 {
        return Err("resolution dimensions must be non-zero".into());
    }
    Ok(Resolution { width, height })
}

fn parse_lut_spec(raw: &str) -> Result<MotionLutSpec, String> {
    let trimmed = raw.trim();
    if let Some((name, rest)) = trimmed.split_once('(') {
        let Some(inside) = rest.strip_suffix(')') else {
            return Err(format!("unclosed '(' in lut spec '{trimmed}'"));
        };
        let intensity: f32 = inside
            .trim()
            .parse()
            .map_err(|_| format!("invalid lut intensity '{inside}'"))?;
        if !intensity.is_finite() || intensity < 0.0 {
            return Err(format!("lut intensity must be finite and >= 0, got {intensity}"));
        }
        return Ok(MotionLutSpec {
            preset: name.trim().to_owned(),
            intensity,
        });
    }
    if trimmed.is_empty() {
        return Err("lut preset must not be empty".into());
    }
    Ok(MotionLutSpec {
        preset: trimmed.to_owned(),
        intensity: 1.0,
    })
}

fn unquote_if_quoted(s: &str) -> String {
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        unescape_quoted(&s[1..s.len() - 1])
    } else {
        s.to_owned()
    }
}

fn unescape_quoted(inner: &str) -> String {
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn strip_inline_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut in_quotes = false;
    let mut escaped = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if escaped {
            escaped = false;
            i += 1;
            continue;
        }
        if in_quotes {
            if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_quotes = false;
            }
            i += 1;
            continue;
        }
        if b == b'"' {
            in_quotes = true;
            i += 1;
            continue;
        }
        // `#` outside quotes is only a comment when preceded by start-of-line or whitespace
        // so hex colors like `#FFF5EB` inside `color=#FFF5EB` are preserved.
        if b == b'#' && (i == 0 || bytes[i - 1].is_ascii_whitespace()) {
            return &line[..i];
        }
        if b == b'/' && bytes.get(i + 1) == Some(&b'/') {
            return &line[..i];
        }
        i += 1;
    }
    line
}

#[derive(Debug, Clone)]
struct LineToken {
    raw: String,
    was_quoted: bool,
}

fn tokenize_directive_line(line: &str, line_no: usize) -> Result<Vec<LineToken>, SchemaError> {
    let mut tokens = Vec::new();
    let mut curr = String::new();
    let mut curr_quoted = false;
    let mut in_quotes = false;
    let mut paren_depth = 0usize;
    let mut escaped = false;

    for ch in line.chars() {
        if escaped {
            match ch {
                'n' => curr.push('\n'),
                't' => curr.push('\t'),
                other => curr.push(other),
            }
            escaped = false;
            continue;
        }
        if in_quotes {
            if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_quotes = false;
            } else {
                curr.push(ch);
            }
            continue;
        }
        if ch == '"' {
            in_quotes = true;
            if curr.is_empty() {
                curr_quoted = true;
            }
            continue;
        }
        if ch == '(' {
            paren_depth += 1;
            curr.push(ch);
            continue;
        }
        if ch == ')' {
            if paren_depth == 0 {
                return Err(SchemaError::InvalidMotionDsl {
                    line: line_no,
                    message: "unexpected ')'".into(),
                });
            }
            paren_depth -= 1;
            curr.push(ch);
            continue;
        }
        if ch.is_whitespace() && paren_depth == 0 {
            if !curr.is_empty() || curr_quoted {
                tokens.push(LineToken {
                    raw: std::mem::take(&mut curr),
                    was_quoted: curr_quoted,
                });
                curr_quoted = false;
            }
            continue;
        }
        curr.push(ch);
    }

    if in_quotes {
        return Err(SchemaError::InvalidMotionDsl {
            line: line_no,
            message: "unterminated string literal".into(),
        });
    }
    if paren_depth != 0 {
        return Err(SchemaError::InvalidMotionDsl {
            line: line_no,
            message: "unclosed '(' in directive".into(),
        });
    }
    if !curr.is_empty() || curr_quoted {
        tokens.push(LineToken {
            raw: curr,
            was_quoted: curr_quoted,
        });
    }
    Ok(tokens)
}

fn split_kv(tok: &LineToken) -> Option<(&str, &str)> {
    if tok.was_quoted {
        return None;
    }
    tok.raw.split_once('=')
}

fn parse_directive(tokens: &[LineToken], line: usize) -> Result<MotionDirective, SchemaError> {
    let err = |msg: String| SchemaError::InvalidMotionDsl { line, message: msg };
    let head = tokens[0].raw.as_str();
    let rest = &tokens[1..];

    match head {
        "asset" => {
            if rest.len() < 3 {
                return Err(err(
                    "asset requires: asset <id> <video|audio|image> path=<path>".into(),
                ));
            }
            let id = rest[0].raw.clone();
            let kind = match rest[1].raw.as_str() {
                "video" => AssetKind::Video,
                "audio" => AssetKind::Audio,
                "image" => AssetKind::Image,
                other => return Err(err(format!("invalid asset kind '{other}'"))),
            };
            let mut path: Option<String> = None;
            for tok in &rest[2..] {
                if let Some((k, v)) = split_kv(tok)
                    && k == "path"
                {
                    path = Some(v.to_owned());
                } else {
                    return Err(err(format!("unexpected token '{}' in asset", tok.raw)));
                }
            }
            let path = path.ok_or_else(|| err("asset requires path=...".into()))?;
            Ok(MotionDirective::Asset { id, kind, path })
        }
        "audio" => {
            if rest.is_empty() {
                return Err(err("audio requires <id> and key=value attributes".into()));
            }
            let id = rest[0].raw.clone();
            let mut asset_id = None;
            let mut range = None;
            let mut src = None;
            let mut bus = None;
            let mut gain_db = None;
            let mut fade_in = None;
            let mut fade_out = None;
            for tok in &rest[1..] {
                let Some((k, v)) = split_kv(tok) else {
                    return Err(err(format!("expected key=value in audio, got '{}'", tok.raw)));
                };
                match k {
                    "asset" => asset_id = Some(v.to_owned()),
                    "range" => range = Some(MotionRangeExpr::parse(v, line)?),
                    "src" => src = Some(MotionTimeExpr::parse(v, line)?),
                    "bus" => bus = Some(parse_audio_bus(v, line)?),
                    "gain_db" => gain_db = Some(parse_f32(v, "gain_db", line)?),
                    "fade_in" => fade_in = Some(MotionTimeExpr::parse(v, line)?),
                    "fade_out" => fade_out = Some(MotionTimeExpr::parse(v, line)?),
                    other => return Err(err(format!("unknown audio attribute '{other}'"))),
                }
            }
            Ok(MotionDirective::Audio {
                id,
                asset_id: asset_id.ok_or_else(|| err("audio requires asset=...".into()))?,
                range: range.ok_or_else(|| err("audio requires range=...".into()))?,
                src,
                bus,
                gain_db,
                fade_in,
                fade_out,
            })
        }
        "shot" => {
            if rest.is_empty() {
                return Err(err("shot requires <id> and key=value attributes".into()));
            }
            let id = rest[0].raw.clone();
            let mut asset_id = None;
            let mut range = None;
            let mut src = None;
            let mut fit = None;
            let mut speed = None;
            let mut anchor = None;
            for tok in &rest[1..] {
                let Some((k, v)) = split_kv(tok) else {
                    return Err(err(format!("expected key=value in shot, got '{}'", tok.raw)));
                };
                match k {
                    "asset" => asset_id = Some(v.to_owned()),
                    "range" => range = Some(MotionRangeExpr::parse(v, line)?),
                    "src" => src = Some(MotionTimeExpr::parse(v, line)?),
                    "fit" => {
                        fit = Some(match v {
                            "stretch" => SourceFit::Stretch,
                            "cover" => SourceFit::Cover,
                            "contain" => SourceFit::Contain,
                            other => return Err(err(format!("invalid fit '{other}'"))),
                        })
                    }
                    "speed" => speed = Some(v.to_owned()),
                    "anchor" => anchor = Some(v.to_owned()),
                    other => return Err(err(format!("unknown shot attribute '{other}'"))),
                }
            }
            Ok(MotionDirective::Shot {
                id,
                asset_id: asset_id.ok_or_else(|| err("shot requires asset=...".into()))?,
                range: range.ok_or_else(|| err("shot requires range=...".into()))?,
                src,
                fit,
                speed,
                anchor,
            })
        }
        "beat_drop" => {
            if rest.is_empty() {
                return Err(err("beat_drop requires <id> and at=...".into()));
            }
            let id = rest[0].raw.clone();
            let mut at = None;
            let mut clip_id = None;
            let mut src = None;
            let mut label = None;
            for tok in &rest[1..] {
                let Some((k, v)) = split_kv(tok) else {
                    return Err(err(format!(
                        "expected key=value in beat_drop, got '{}'",
                        tok.raw
                    )));
                };
                match k {
                    "at" => at = Some(MotionTimeExpr::parse(v, line)?),
                    "clip" => clip_id = Some(v.to_owned()),
                    "src" => src = Some(MotionTimeExpr::parse(v, line)?),
                    "label" => label = Some(v.to_owned()),
                    other => return Err(err(format!("unknown beat_drop attribute '{other}'"))),
                }
            }
            Ok(MotionDirective::BeatDrop {
                id,
                at: at.ok_or_else(|| err("beat_drop requires at=...".into()))?,
                clip_id,
                src,
                label,
            })
        }
        "transition" => {
            if rest.is_empty() {
                return Err(err("transition requires <shader> between=<a>,<b> at=... dur=...".into()));
            }
            let shader = rest[0].raw.clone();
            let mut between: Option<(String, String)> = None;
            let mut at = None;
            let mut dur = None;
            let mut params = BTreeMap::new();
            for tok in &rest[1..] {
                let Some((k, v)) = split_kv(tok) else {
                    return Err(err(format!(
                        "expected key=value in transition, got '{}'",
                        tok.raw
                    )));
                };
                match k {
                    "between" => {
                        let Some((a, b)) = v.split_once(',') else {
                            return Err(err(format!(
                                "transition between= requires '<clip_a>,<clip_b>', got '{v}'"
                            )));
                        };
                        between = Some((a.trim().to_owned(), b.trim().to_owned()));
                    }
                    "at" => at = Some(MotionTimeExpr::parse(v, line)?),
                    "dur" => dur = Some(MotionTimeExpr::parse(v, line)?),
                    other => {
                        params.insert(other.to_owned(), v.to_owned());
                    }
                }
            }
            let (from_clip, to_clip) =
                between.ok_or_else(|| err("transition requires between=...".into()))?;
            Ok(MotionDirective::Transition {
                shader,
                from_clip,
                to_clip,
                at: at.ok_or_else(|| err("transition requires at=...".into()))?,
                dur: dur.ok_or_else(|| err("transition requires dur=...".into()))?,
                params,
            })
        }
        "fx" => {
            if rest.is_empty() {
                return Err(err("fx requires <preset_or_kind> on=<target>".into()));
            }
            let preset_or_kind = rest[0].raw.clone();
            let mut on = None;
            let mut at = None;
            let mut dur = None;
            let mut params = BTreeMap::new();
            for tok in &rest[1..] {
                let Some((k, v)) = split_kv(tok) else {
                    return Err(err(format!("expected key=value in fx, got '{}'", tok.raw)));
                };
                match k {
                    "on" => on = Some(v.to_owned()),
                    "at" => at = Some(MotionTimeExpr::parse(v, line)?),
                    "dur" => dur = Some(MotionTimeExpr::parse(v, line)?),
                    other => {
                        params.insert(other.to_owned(), v.to_owned());
                    }
                }
            }
            Ok(MotionDirective::Fx {
                preset_or_kind,
                on: on.ok_or_else(|| err("fx requires on=...".into()))?,
                at,
                dur,
                params,
            })
        }
        "text" => {
            if rest.is_empty() {
                return Err(err("text requires <quoted_str> at=<range>".into()));
            }
            let text = rest[0].raw.clone();
            let mut range = None;
            let mut pos = None;
            let mut size = None;
            let mut color = None;
            let mut font = None;
            let mut outline = None;
            let mut anim = None;
            for tok in &rest[1..] {
                let Some((k, v)) = split_kv(tok) else {
                    return Err(err(format!("expected key=value in text, got '{}'", tok.raw)));
                };
                match k {
                    "at" => range = Some(MotionRangeExpr::parse(v, line)?),
                    "pos" => pos = Some(parse_vec2_tuple(v, line)?),
                    "size" => size = Some(parse_f32(v, "size", line)?),
                    "color" => color = Some(v.to_owned()),
                    "font" => font = Some(v.to_owned()),
                    "outline" => outline = Some(v.to_owned()),
                    "anim" => anim = Some(v.to_owned()),
                    other => return Err(err(format!("unknown text attribute '{other}'"))),
                }
            }
            Ok(MotionDirective::Text {
                text,
                range: range.ok_or_else(|| err("text requires at=<range>".into()))?,
                pos,
                size,
                color,
                font,
                outline,
                anim,
            })
        }
        "shutter" => {
            let mut angle = None;
            let mut samples = None;
            let mut phase = None;
            for tok in rest {
                let Some((k, v)) = split_kv(tok) else {
                    return Err(err(format!(
                        "expected key=value in shutter, got '{}'",
                        tok.raw
                    )));
                };
                match k {
                    "angle" => angle = Some(parse_f32(v, "angle", line)?),
                    "samples" => {
                        let s: u32 = v
                            .parse()
                            .map_err(|_| err(format!("invalid shutter samples '{v}'")))?;
                        samples = Some(s);
                    }
                    "phase" => {
                        phase = Some(match v {
                            "centered" => ShutterPhase::Centered,
                            "trailing" | "end" => ShutterPhase::Trailing,
                            other => return Err(err(format!("invalid shutter phase '{other}'"))),
                        });
                    }
                    other => return Err(err(format!("unknown shutter attribute '{other}'"))),
                }
            }
            Ok(MotionDirective::Shutter {
                angle,
                samples,
                phase,
            })
        }
        other => Err(err(format!("unknown directive '{other}'"))),
    }
}

fn parse_f32(raw: &str, name: &str, line: usize) -> Result<f32, SchemaError> {
    let v: f32 = raw.parse().map_err(|_| SchemaError::InvalidMotionDsl {
        line,
        message: format!("invalid float for '{name}': '{raw}'"),
    })?;
    if !v.is_finite() {
        return Err(SchemaError::InvalidMotionDsl {
            line,
            message: format!("non-finite float for '{name}'"),
        });
    }
    Ok(v)
}

fn parse_vec2_tuple(raw: &str, line: usize) -> Result<[f32; 2], SchemaError> {
    let inner = raw
        .trim()
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .ok_or_else(|| SchemaError::InvalidMotionDsl {
            line,
            message: format!("expected '(x, y)' tuple, got '{raw}'"),
        })?;
    let Some((xs, ys)) = inner.split_once(',') else {
        return Err(SchemaError::InvalidMotionDsl {
            line,
            message: format!("expected '(x, y)' tuple, got '{raw}'"),
        });
    };
    Ok([
        parse_f32(xs.trim(), "pos.x", line)?,
        parse_f32(ys.trim(), "pos.y", line)?,
    ])
}

fn parse_audio_bus(raw: &str, line: usize) -> Result<AudioBus, SchemaError> {
    match raw {
        "music" => Ok(AudioBus::Music),
        "dialogue" => Ok(AudioBus::Dialogue),
        "sfx_impact" => Ok(AudioBus::SfxImpact),
        "sfx_whoosh" => Ok(AudioBus::SfxWhoosh),
        "sfx_riser" => Ok(AudioBus::SfxRiser),
        "sub_bass" => Ok(AudioBus::SubBass),
        other => Err(SchemaError::InvalidMotionDsl {
            line,
            message: format!("unknown audio bus '{other}'"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_motion_md_frontmatter_and_all_directives() {
        let doc_src = r#"
---
name: "Asta Mini Drop"
fps: 24
res: 1280x720
bpm: 140
lut: anime_cyber_contrast(0.65)
---
# Asset declarations
asset clip_a video path="synthetic://clip_a"
asset clip_b video path="synthetic://clip_b"
asset bgm audio path="synthetic://beat"

audio a_bgm asset=bgm range=0s..5s src=0s bus=music fade_out=0.25s gain_db=-1.5
shot c1 asset=clip_a range=0s..2.5s src=0s fit=cover
shot c2 asset=clip_b range=f:60..f:120 src=0s fit=contain
beat_drop drop_1 at=2.5s clip=c1 src=2.4s label="Main Drop"
transition whip_chromatic between=c1,c2 at=@drop_1 dur=0.3s whip=0.85 ca=0.035
fx xenoz_impact_glow on=tr_01 at=@drop_1 dur=0.4s glow_intensity=1.2
text "DEVIL UNION" at=2.5s..4.0s pos=(0.5, 0.78) size=0.07 color=#FFF5EB font="Impact" anim=fade
shutter angle=180 samples=8 phase=centered
"#;
        let doc = MotionDoc::parse(doc_src).expect("valid Motion.md");
        assert_eq!(doc.meta.name, "Asta Mini Drop");
        assert_eq!(doc.meta.fps, FPS_24);
        assert_eq!(
            doc.meta.resolution,
            Resolution {
                width: 1280,
                height: 720
            }
        );
        assert_eq!(doc.meta.bpm, Some(140.0));
        assert_eq!(
            doc.meta.lut,
            Some(MotionLutSpec {
                preset: "anime_cyber_contrast".into(),
                intensity: 0.65
            })
        );
        assert_eq!(doc.directives.len(), 11);

        match &doc.directives[9] {
            MotionDirective::Text {
                text,
                pos,
                color,
                anim,
                ..
            } => {
                assert_eq!(text, "DEVIL UNION");
                assert_eq!(*pos, Some([0.5, 0.78]));
                assert_eq!(color.as_deref(), Some("#FFF5EB"));
                assert_eq!(anim.as_deref(), Some("fade"));
            }
            other => panic!("expected Text directive, got {other:?}"),
        }
    }

    #[test]
    fn time_expr_static_resolution_is_exact_and_quantized() {
        let fps = FPS_24;
        let events = BTreeMap::from([("drop_1".to_owned(), Flicks(FLICKS_PER_SECOND * 2))]);
        assert_eq!(
            MotionTimeExpr::Frame(12)
                .to_ticks_static(fps, None, &events)
                .unwrap(),
            Flicks(FLICKS_PER_SECOND / 2)
        );
        assert_eq!(
            MotionTimeExpr::EventRef("drop_1".into())
                .to_ticks_static(fps, None, &events)
                .unwrap(),
            Flicks(FLICKS_PER_SECOND * 2)
        );
        assert_eq!(
            MotionTimeExpr::Beat(2)
                .to_ticks_static(fps, Some(120.0), &events)
                .unwrap(),
            Flicks(FLICKS_PER_SECOND)
        );
    }

    #[test]
    fn rejects_malformed_frontmatter_and_directives() {
        let unclosed = "---\nname: \"broken\"\nshot c1 asset=a range=0s..1s";
        assert!(matches!(
            MotionDoc::parse(unclosed),
            Err(SchemaError::InvalidMotionDsl { .. })
        ));

        let bad_dir = "unknown_cmd foo=bar";
        assert!(matches!(
            MotionDoc::parse(bad_dir),
            Err(SchemaError::InvalidMotionDsl { line: 1, .. })
        ));
    }
}
