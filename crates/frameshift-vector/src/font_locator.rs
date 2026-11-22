use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};

use frameshift_media::{AlphaMode, Rgba16fFrame};
use frameshift_schema::{TextAlign2D, WritingMode2D};
use frameshift_time::Flicks;
use lyon_tessellation::path::iterator::PathIterator;
use lyon_tessellation::path::{Event, Path as LyonPath};
use ttf_parser::{Face, Style, name_id};

use crate::canvas::{
    VectorCanvas, contour_slices_bounds, premultiply_rgba, rasterize_contour_slices_coverage,
    scale_premul, sd_segment_sq, sdf_coverage, source_over_premul,
};
use crate::error::VectorError;
use crate::shaper::{TextLayoutOptions, VariationAxes, shape_text_block};

static GLOBAL_FONT_INDEX: OnceLock<SystemFontIndex> = OnceLock::new();
static FONT_BYTES_CACHE: OnceLock<RwLock<HashMap<PathBuf, Arc<Vec<u8>>>>> = OnceLock::new();

/// Query specifying how a TTF/OTF/TTC font face should be resolved.
///
/// Resolution never silently falls back to an unrelated font family.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FontQuery {
    /// Direct path to a `.ttf`, `.otf`, `.ttc`, or `.otc` file (highest priority).
    pub font_path: Option<String>,
    /// Target typographic or family name (e.g. `"Impact"`, `"Noto Sans"`).
    pub font_family: Option<String>,
    /// Target OS/2 weight class (`1..=1000`; `400` = Regular, `700` = Bold).
    pub font_weight: u16,
}

impl Default for FontQuery {
    fn default() -> Self {
        Self {
            font_path: None,
            font_family: None,
            font_weight: 400,
        }
    }
}

/// Resolved font face bytes and metadata ready for `ttf_parser::Face::parse`.
#[derive(Debug, Clone)]
pub struct ResolvedFont {
    pub bytes: Arc<Vec<u8>>,
    pub face_index: u32,
    pub matched_family: String,
    pub matched_weight: u16,
}

/// Text alignment for multi-line 2D TTF typography.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TtfTextAlign {
    Left,
    #[default]
    Center,
    Right,
}

/// Layout and styling parameters for 2D TrueType/OpenType text rasterization.
#[derive(Debug, Clone, PartialEq)]
pub struct TtfTextConfig {
    pub text: String,
    pub font_path: Option<String>,
    pub font_family: Option<String>,
    pub font_weight: u16,
    pub font_size_norm: f32,
    pub color_rgba: [f32; 4],
    pub outline_rgba: [f32; 4],
    pub outline_width_norm: f32,
    pub align: TtfTextAlign,
    pub x_offset_norm: Option<f32>,
    pub y_offset_norm: f32,
    pub tracking_em: Option<f32>,
    pub line_height_em: Option<f32>,
    pub opacity: f32,
    pub fallback_fonts: Vec<String>,
    pub writing_mode: WritingMode2D,
    pub variations: VariationAxes,
    pub color_bottom_rgba: Option<[f32; 4]>,
    pub outer_outline_rgba: [f32; 4],
    pub outer_outline_width_norm: f32,
    pub outline_gap_norm: f32,
    pub hdr_intensity: f32,
}

impl Default for TtfTextConfig {
    fn default() -> Self {
        Self {
            text: String::new(),
            font_path: None,
            font_family: None,
            font_weight: 400,
            font_size_norm: 0.065,
            color_rgba: [1.0, 1.0, 1.0, 1.0],
            outline_rgba: [0.0, 0.0, 0.0, 0.0],
            outline_width_norm: 0.0,
            align: TtfTextAlign::Center,
            x_offset_norm: None,
            y_offset_norm: 0.5,
            tracking_em: None,
            line_height_em: None,
            opacity: 1.0,
            fallback_fonts: Vec::new(),
            writing_mode: WritingMode2D::HorizontalTb,
            variations: VariationAxes::default(),
            color_bottom_rgba: None,
            outer_outline_rgba: [0.0, 0.0, 0.0, 0.0],
            outer_outline_width_norm: 0.0,
            outline_gap_norm: 0.0,
            hdr_intensity: 1.0,
        }
    }
}

#[derive(Debug, Clone)]
struct IndexedFaceEntry {
    path: PathBuf,
    face_index: u32,
    weight: u16,
    style_penalty: u32,
    /// `(name_tier_penalty, display_family)` keyed by normalized family/full name.
    names: HashMap<String, (u32, String)>,
    primary_family: String,
}

/// Deterministic, lazily-cached index of system and workspace font faces.
#[derive(Debug, Clone, Default)]
pub struct SystemFontIndex {
    entries: Vec<IndexedFaceEntry>,
    by_name: HashMap<String, Vec<usize>>,
}

impl SystemFontIndex {
    /// Returns the process-wide cached `SystemFontIndex`.
    pub fn global() -> &'static Self {
        GLOBAL_FONT_INDEX.get_or_init(Self::build_default)
    }

    /// Scans the default deterministic font directories (`FRAMESHIFT_FONT_DIRS`, `./fonts`,
    /// `./assets/fonts`, and OS system font directories).
    pub fn build_default() -> Self {
        let dirs = collect_default_font_dirs();
        Self::build_for_dirs(&dirs)
    }

    /// Scans the supplied directories recursively in deterministic sorted order.
    pub fn build_for_dirs(dirs: &[PathBuf]) -> Self {
        let mut files = BTreeSet::new();
        for dir in dirs {
            collect_font_files_recursive(dir, &mut files);
        }

        let mut index = Self::default();
        for path in files {
            index.index_file(&path);
        }
        index
    }

    /// Returns the number of indexed font faces.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if no font faces were indexed.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Resolves a `FontQuery` against this index (without silent fallback).
    pub fn resolve(
        &self,
        query: &FontQuery,
        project_dir: Option<&Path>,
    ) -> Result<ResolvedFont, VectorError> {
        if let Some(raw_path) = query.font_path.as_deref() {
            return resolve_explicit_font_path(raw_path, query, project_dir);
        }

        let Some(family) = query.font_family.as_deref() else {
            return Err(VectorError::FontNotFound(
                "neither font_path nor font_family was provided".to_string(),
            ));
        };

        let norm = normalize_font_name(family);
        if norm.is_empty() {
            return Err(VectorError::FontNotFound(
                "empty font_family cannot be resolved".to_string(),
            ));
        }

        let target_weight = query.font_weight.clamp(1, 1000);

        if let Some(dir) = project_dir {
            let local_dirs = [dir.join("fonts"), dir.join("assets").join("fonts")];
            if local_dirs.iter().any(|d| d.is_dir()) {
                let local_index = Self::build_for_dirs(&local_dirs);
                if let Some(found) = local_index.find_best_match(&norm, target_weight)? {
                    return Ok(found);
                }
            }
        }

        if let Some(found) = self.find_best_match(&norm, target_weight)? {
            return Ok(found);
        }

        Err(VectorError::FontNotFound(format!(
            "family '{family}' (weight {target_weight})"
        )))
    }

    fn index_file(&mut self, path: &Path) {
        let Ok(bytes) = std::fs::read(path) else {
            return;
        };
        let face_count = ttf_parser::fonts_in_collection(&bytes).unwrap_or(1);
        for face_index in 0..face_count {
            let Ok(face) = Face::parse(&bytes, face_index) else {
                continue;
            };
            if face.units_per_em() == 0 {
                continue;
            }
            let entry = build_indexed_face_entry(path.to_path_buf(), face_index, &face);
            let idx = self.entries.len();
            for norm_key in entry.names.keys() {
                self.by_name.entry(norm_key.clone()).or_default().push(idx);
            }
            self.entries.push(entry);
        }
    }

    fn find_best_match(
        &self,
        normalized_family: &str,
        target_weight: u16,
    ) -> Result<Option<ResolvedFont>, VectorError> {
        let Some(candidate_indices) = self.by_name.get(normalized_family) else {
            return Ok(None);
        };

        let mut best: Option<(u32, &IndexedFaceEntry, &str)> = None;
        for &entry_idx in candidate_indices {
            let entry = &self.entries[entry_idx];
            let Some(&(tier_penalty, ref display_name)) = entry.names.get(normalized_family) else {
                continue;
            };
            let weight_diff = (i32::from(entry.weight) - i32::from(target_weight)).unsigned_abs();
            let score = weight_diff * 10 + entry.style_penalty + tier_penalty;

            let replace = match best {
                None => true,
                Some((best_score, best_entry, _)) => {
                    (score, &entry.path, entry.face_index)
                        < (best_score, &best_entry.path, best_entry.face_index)
                }
            };
            if replace {
                best = Some((score, entry, display_name.as_str()));
            }
        }

        let Some((_, entry, display_name)) = best else {
            return Ok(None);
        };

        let bytes = read_cached_font_bytes(&entry.path)?;
        Ok(Some(ResolvedFont {
            bytes,
            face_index: entry.face_index,
            matched_family: display_name.to_string(),
            matched_weight: entry.weight,
        }))
    }
}

/// Resolves a `FontQuery` using the process-wide `SystemFontIndex` without silent fallback.
pub fn resolve_font(
    query: &FontQuery,
    project_dir: Option<&Path>,
) -> Result<ResolvedFont, VectorError> {
    SystemFontIndex::global().resolve(query, project_dir)
}

/// Resolves a primary `FontQuery` plus an ordered fallback font chain (by file path or family name)
/// without silent fallback to unrelated fonts.
pub fn resolve_font_chain(
    primary: &FontQuery,
    fallback_fonts: &[String],
    project_dir: Option<&Path>,
) -> Result<(ResolvedFont, Vec<ResolvedFont>), VectorError> {
    let primary_resolved = resolve_font(primary, project_dir)?;
    let mut fallbacks = Vec::with_capacity(fallback_fonts.len());
    for raw in fallback_fonts {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(VectorError::FontNotFound(
                "empty fallback font entry".to_string(),
            ));
        }
        let lower = trimmed.to_ascii_lowercase();
        let is_path = trimmed.contains('/')
            || trimmed.contains('\\')
            || lower.ends_with(".ttf")
            || lower.ends_with(".otf")
            || lower.ends_with(".ttc")
            || lower.ends_with(".otc");
        let fb_query = if is_path {
            FontQuery {
                font_path: Some(trimmed.to_string()),
                font_family: None,
                font_weight: primary.font_weight,
            }
        } else {
            FontQuery {
                font_path: None,
                font_family: Some(trimmed.to_string()),
                font_weight: primary.font_weight,
            }
        };
        fallbacks.push(resolve_font(&fb_query, project_dir)?);
    }
    Ok((primary_resolved, fallbacks))
}

/// Rasterizes multi-line TTF/OTF text directly into a `Rgba16fFrame` (`LinearRec709`).
pub fn rasterize_ttf_text_rgba16f(
    config: &TtfTextConfig,
    width: u32,
    height: u32,
    pts: Flicks,
    alpha_mode: AlphaMode,
    project_dir: Option<&Path>,
) -> Result<Rgba16fFrame, VectorError> {
    validate_ttf_text_config(config)?;
    let query = FontQuery {
        font_path: config.font_path.clone(),
        font_family: config.font_family.clone(),
        font_weight: config.font_weight,
    };
    let (resolved, fallbacks) = resolve_font_chain(&query, &config.fallback_fonts, project_dir)?;
    let mut canvas = VectorCanvas::new(width, height)?;
    draw_ttf_text_with_fallbacks(&mut canvas, config, &resolved, &fallbacks)?;
    canvas.to_rgba16f_frame(pts, alpha_mode)
}

/// Rasterizes multi-line TTF/OTF text using an already-resolved font face into `Rgba16fFrame`.
pub fn rasterize_ttf_text_with_font_rgba16f(
    config: &TtfTextConfig,
    resolved: &ResolvedFont,
    width: u32,
    height: u32,
    pts: Flicks,
    alpha_mode: AlphaMode,
) -> Result<Rgba16fFrame, VectorError> {
    let mut canvas = VectorCanvas::new(width, height)?;
    draw_ttf_text(&mut canvas, config, resolved)?;
    canvas.to_rgba16f_frame(pts, alpha_mode)
}

/// Draws multi-line, shaped, variable-capable TTF/OTF text with 4x4 SSAA `NonZero` fill and
/// multi-layer SDF outline onto `canvas` in premultiplied Linear Rec.709 space.
pub fn draw_ttf_text(
    canvas: &mut VectorCanvas,
    config: &TtfTextConfig,
    resolved: &ResolvedFont,
) -> Result<(), VectorError> {
    let mut fallbacks = Vec::with_capacity(config.fallback_fonts.len());
    for raw in &config.fallback_fonts {
        let trimmed = raw.trim();
        let lower = trimmed.to_ascii_lowercase();
        let is_path = trimmed.contains('/')
            || trimmed.contains('\\')
            || lower.ends_with(".ttf")
            || lower.ends_with(".otf")
            || lower.ends_with(".ttc")
            || lower.ends_with(".otc");
        let fb_query = if is_path {
            FontQuery {
                font_path: Some(trimmed.to_string()),
                font_family: None,
                font_weight: config.font_weight,
            }
        } else {
            FontQuery {
                font_path: None,
                font_family: Some(trimmed.to_string()),
                font_weight: config.font_weight,
            }
        };
        fallbacks.push(resolve_font(&fb_query, None)?);
    }
    draw_ttf_text_with_fallbacks(canvas, config, resolved, &fallbacks)
}

/// Draws multi-line, shaped, variable-capable TTF/OTF text with explicit fallback faces.
pub fn draw_ttf_text_with_fallbacks(
    canvas: &mut VectorCanvas,
    config: &TtfTextConfig,
    resolved: &ResolvedFont,
    fallback_fonts: &[ResolvedFont],
) -> Result<(), VectorError> {
    validate_ttf_text_config(config)?;

    let op = config.opacity.clamp(0.0, 1.0);
    if config.text.is_empty() || config.font_size_norm <= 0.0 || op <= 0.0 {
        return Ok(());
    }

    let w_f = canvas.width() as f32;
    let h_f = canvas.height() as f32;
    let font_px = config.font_size_norm * h_f;
    if !font_px.is_finite() || font_px <= 0.0 {
        return Ok(());
    }

    let anchor_x = config.x_offset_norm.unwrap_or(match config.align {
        TtfTextAlign::Left => 0.04,
        TtfTextAlign::Center => 0.5,
        TtfTextAlign::Right => 0.96,
    }) * w_f;
    let block_center_y = config.y_offset_norm * h_f;
    let align_2d = match config.align {
        TtfTextAlign::Left => TextAlign2D::Left,
        TtfTextAlign::Center => TextAlign2D::Center,
        TtfTextAlign::Right => TextAlign2D::Right,
    };

    let shaped = shape_text_block(
        &config.text,
        resolved,
        fallback_fonts,
        &TextLayoutOptions {
            font_size: font_px,
            align: align_2d,
            writing_mode: config.writing_mode,
            tracking_em: config.tracking_em.unwrap_or(0.0),
            line_height_em: config.line_height_em,
            variations: config.variations,
            enable_ligatures: true,
        },
    )?;

    if shaped.glyphs.is_empty() {
        return Ok(());
    }

    let flatten_tol = 0.12_f32.min(font_px * 0.005).max(0.02);

    struct GlyphOutlines {
        contours: Vec<Vec<[f32; 2]>>,
        bounds: [f32; 4],
    }

    let mut glyphs: Vec<GlyphOutlines> = Vec::with_capacity(shaped.glyphs.len());
    let mut total_min_x = f32::INFINITY;
    let mut total_min_y = f32::INFINITY;
    let mut total_max_x = f32::NEG_INFINITY;
    let mut total_max_y = f32::NEG_INFINITY;

    for sg in &shaped.glyphs {
        let raw_contours = flatten_closed_contours(&sg.lyon_path, flatten_tol);
        let contours: Vec<Vec<[f32; 2]>> = raw_contours
            .into_iter()
            .map(|c| {
                c.into_iter()
                    .map(|[x, y]| [anchor_x + x, block_center_y - y])
                    .collect()
            })
            .collect();
        let Some(bounds) = contour_list_bounds(&contours) else {
            continue;
        };
        total_min_x = total_min_x.min(bounds[0]);
        total_min_y = total_min_y.min(bounds[1]);
        total_max_x = total_max_x.max(bounds[2]);
        total_max_y = total_max_y.max(bounds[3]);
        glyphs.push(GlyphOutlines { contours, bounds });
    }

    if glyphs.is_empty() || !total_min_x.is_finite() {
        return Ok(());
    }

    let outline_px = config.outline_width_norm.max(0.0) * h_f;
    let gap_px = config.outline_gap_norm.max(0.0) * h_f;
    let outer_outline_px = config.outer_outline_width_norm.max(0.0) * h_f;
    let hdr = config.hdr_intensity.max(0.0);

    let mut fill_top = config.color_rgba;
    fill_top[0] *= hdr;
    fill_top[1] *= hdr;
    fill_top[2] *= hdr;
    let mut fill_bot = config.color_bottom_rgba.unwrap_or(config.color_rgba);
    fill_bot[0] *= hdr;
    fill_bot[1] *= hdr;
    fill_bot[2] *= hdr;

    let outline_premul = premultiply_rgba(config.outline_rgba);
    let outer_outline_premul = premultiply_rgba(config.outer_outline_rgba);
    let has_outline = outline_px > 0.0 && outline_premul[3] > 0.0;
    let has_outer_outline = outer_outline_px > 0.0 && outer_outline_premul[3] > 0.0;
    let has_fill = fill_top[3] > 0.0 || fill_bot[3] > 0.0;
    if !has_outline && !has_outer_outline && !has_fill {
        return Ok(());
    }

    let max_stroke_reach = if has_outer_outline {
        outline_px + gap_px + outer_outline_px
    } else if has_outline {
        outline_px
    } else {
        0.0
    };
    let region_pad = if max_stroke_reach > 0.0 {
        max_stroke_reach + 2.0
    } else {
        1.5
    };
    let Some((rx0, ry0, rx1, ry1)) = canvas.clipped_bounds(
        total_min_x - region_pad,
        total_min_y - region_pad,
        total_max_x + region_pad,
        total_max_y + region_pad,
    ) else {
        return Ok(());
    };

    let rw = (rx1 - rx0 + 1) as usize;
    let rh = (ry1 - ry0 + 1) as usize;
    let mut fill_cov_buf = vec![0.0_f32; rw * rh];
    let need_dist = has_outline || has_outer_outline;
    let mut min_dist_buf = if need_dist {
        vec![max_stroke_reach + 2.0; rw * rh]
    } else {
        Vec::new()
    };

    for glyph in &glyphs {
        let contour_slices: Vec<&[[f32; 2]]> =
            glyph.contours.iter().map(|c| c.as_slice()).collect();
        if let Some((gx0, gy0, gx1, gy1)) = canvas.clipped_bounds(
            glyph.bounds[0] - 1.0,
            glyph.bounds[1] - 1.0,
            glyph.bounds[2] + 1.0,
            glyph.bounds[3] + 1.0,
        ) {
            let cx0 = gx0.max(rx0);
            let cy0 = gy0.max(ry0);
            let cx1 = gx1.min(rx1);
            let cy1 = gy1.min(ry1);
            if cx1 >= cx0 && cy1 >= cy0 {
                let start_off = ((cy0 - ry0) as usize) * rw + ((cx0 - rx0) as usize);
                rasterize_contour_slices_coverage(
                    &contour_slices,
                    cx0,
                    cy0,
                    cx1,
                    cy1,
                    false,
                    rw,
                    &mut fill_cov_buf[start_off..],
                );
            }
        }

        if need_dist {
            let out_pad = max_stroke_reach + 1.5;
            let Some((ox0, oy0, ox1, oy1)) = canvas.clipped_bounds(
                glyph.bounds[0] - out_pad,
                glyph.bounds[1] - out_pad,
                glyph.bounds[2] + out_pad,
                glyph.bounds[3] + out_pad,
            ) else {
                continue;
            };
            let cx0 = ox0.max(rx0);
            let cy0 = oy0.max(ry0);
            let cx1 = ox1.min(rx1);
            let cy1 = oy1.min(ry1);
            if cx1 < cx0 || cy1 < cy0 {
                continue;
            }

            let segments = collect_contour_segments_with_aabb(&glyph.contours);
            if segments.is_empty() {
                continue;
            }
            let max_r = max_stroke_reach + 1.0;
            let max_r_sq = max_r * max_r;

            for py in cy0..=cy1 {
                let p_y = py as f32 + 0.5;
                let row_off = ((py - ry0) as usize) * rw;
                for px in cx0..=cx1 {
                    let idx = row_off + ((px - rx0) as usize);
                    if fill_cov_buf[idx] >= 1.0 {
                        min_dist_buf[idx] = 0.0;
                        continue;
                    }
                    let p_x = px as f32 + 0.5;
                    let mut min_dist_sq = max_r_sq;
                    for seg in &segments {
                        if p_x < seg.min_x - max_r
                            || p_x > seg.max_x + max_r
                            || p_y < seg.min_y - max_r
                            || p_y > seg.max_y + max_r
                        {
                            continue;
                        }
                        let d_sq = sd_segment_sq([p_x, p_y], seg.a, seg.b);
                        if d_sq < min_dist_sq {
                            min_dist_sq = d_sq;
                        }
                    }
                    let dist = min_dist_sq.sqrt();
                    if dist < min_dist_buf[idx] {
                        min_dist_buf[idx] = dist;
                    }
                }
            }
        }
    }

    let apply_opacity = (op - 1.0).abs() > 0.0;
    let span_y = (total_max_y - total_min_y).max(1.0);
    for row in 0..rh {
        let py = ry0 + row as u32;
        let grad_t = (((py as f32 + 0.5) - total_min_y) / span_y).clamp(0.0, 1.0);
        let fill_rgba = [
            fill_top[0] + (fill_bot[0] - fill_top[0]) * grad_t,
            fill_top[1] + (fill_bot[1] - fill_top[1]) * grad_t,
            fill_top[2] + (fill_bot[2] - fill_top[2]) * grad_t,
            fill_top[3] + (fill_bot[3] - fill_top[3]) * grad_t,
        ];
        let fill_premul = premultiply_rgba(fill_rgba);

        let row_off = row * rw;
        for col in 0..rw {
            let idx = row_off + col;
            let fcov = fill_cov_buf[idx];
            let dist = if need_dist { min_dist_buf[idx] } else { 1e6 };
            let ocov = if has_outline {
                sdf_coverage(dist - outline_px).max(fcov)
            } else {
                0.0
            };
            let outer_cov = if has_outer_outline {
                let inner_edge = outline_px + gap_px;
                let outer_edge = inner_edge + outer_outline_px;
                let outer_band = sdf_coverage(dist - outer_edge);
                if gap_px > 0.0 {
                    let inner_cut = sdf_coverage(dist - inner_edge);
                    (outer_band * (1.0 - inner_cut)).clamp(0.0, 1.0)
                } else {
                    outer_band
                }
            } else {
                0.0
            };

            if fcov <= 0.0 && ocov <= 0.0 && outer_cov <= 0.0 {
                continue;
            }

            let mut acc = [0.0_f32; 4];
            if has_outer_outline && outer_cov > 0.0 {
                acc = source_over_premul(acc, scale_premul(outer_outline_premul, outer_cov));
            }
            if has_outline && ocov > 0.0 {
                acc = source_over_premul(acc, scale_premul(outline_premul, ocov));
            }
            if has_fill && fcov > 0.0 {
                acc = source_over_premul(acc, scale_premul(fill_premul, fcov));
            }
            if apply_opacity {
                acc = scale_premul(acc, op);
            }
            if acc[3] > 0.0 {
                canvas.blend_pixel_premul(rx0 + col as u32, py, acc, 1.0);
            }
        }
    }

    Ok(())
}

fn validate_ttf_text_config(config: &TtfTextConfig) -> Result<(), VectorError> {
    if !config.font_size_norm.is_finite()
        || config.font_size_norm < 0.0
        || !config.outline_width_norm.is_finite()
        || config.outline_width_norm < 0.0
        || !config.outer_outline_width_norm.is_finite()
        || config.outer_outline_width_norm < 0.0
        || !config.outline_gap_norm.is_finite()
        || config.outline_gap_norm < 0.0
        || !config.hdr_intensity.is_finite()
        || config.hdr_intensity < 0.0
        || !config.y_offset_norm.is_finite()
        || !config.opacity.is_finite()
        || config.opacity < 0.0
        || config.x_offset_norm.is_some_and(|v| !v.is_finite())
        || config.tracking_em.is_some_and(|v| !v.is_finite())
        || config.line_height_em.is_some_and(|v| !v.is_finite())
        || config.variations.wght.is_some_and(|v| !v.is_finite())
        || config.variations.wdth.is_some_and(|v| !v.is_finite())
        || config.variations.slnt.is_some_and(|v| !v.is_finite())
        || config.color_rgba.iter().any(|v| !v.is_finite())
        || config.outline_rgba.iter().any(|v| !v.is_finite())
        || config.outer_outline_rgba.iter().any(|v| !v.is_finite())
        || config
            .color_bottom_rgba
            .is_some_and(|c| c.iter().any(|v| !v.is_finite()))
    {
        return Err(VectorError::InvalidParameter(
            "TtfTextConfig contains invalid or non-finite parameters".to_string(),
        ));
    }
    if config.font_weight == 0 || config.font_weight > 1000 {
        return Err(VectorError::InvalidParameter(format!(
            "font_weight must be in 1..=1000 (got {})",
            config.font_weight
        )));
    }
    Ok(())
}

fn resolve_explicit_font_path(
    raw_path: &str,
    query: &FontQuery,
    project_dir: Option<&Path>,
) -> Result<ResolvedFont, VectorError> {
    if raw_path.trim().is_empty() {
        return Err(VectorError::FontNotFound(raw_path.to_owned()));
    }
    let candidate = Path::new(raw_path);
    let resolved_path = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else if let Some(dir) = project_dir {
        let joined = dir.join(candidate);
        if joined.is_file() {
            joined
        } else {
            candidate.to_path_buf()
        }
    } else {
        candidate.to_path_buf()
    };

    if !resolved_path.is_file() {
        return Err(VectorError::FontNotFound(raw_path.to_owned()));
    }

    let bytes = read_cached_font_bytes(&resolved_path)?;
    let face_count = ttf_parser::fonts_in_collection(&bytes).unwrap_or(1).max(1);
    let target_weight = query.font_weight.clamp(1, 1000);
    let target_family_norm = query
        .font_family
        .as_deref()
        .map(normalize_font_name)
        .filter(|s| !s.is_empty());

    let mut best: Option<(u32, u32, String, u16)> = None;
    for face_index in 0..face_count {
        let Ok(face) = Face::parse(&bytes, face_index) else {
            continue;
        };
        if face.units_per_em() == 0 {
            continue;
        }
        let entry = build_indexed_face_entry(resolved_path.clone(), face_index, &face);
        let tier_penalty = if let Some(ref fam) = target_family_norm {
            match entry.names.get(fam) {
                Some(&(tier, _)) => tier,
                None => continue,
            }
        } else {
            0
        };
        let weight_diff = (i32::from(entry.weight) - i32::from(target_weight)).unsigned_abs();
        let score = weight_diff * 10 + entry.style_penalty + tier_penalty;
        let display_name = target_family_norm
            .as_ref()
            .and_then(|fam| entry.names.get(fam).map(|(_, d)| d.clone()))
            .unwrap_or_else(|| entry.primary_family.clone());

        let replace = match best {
            None => true,
            Some((best_score, best_idx, _, _)) => (score, face_index) < (best_score, best_idx),
        };
        if replace {
            best = Some((score, face_index, display_name, entry.weight));
        }
    }

    if let Some((_, face_index, matched_family, matched_weight)) = best {
        Ok(ResolvedFont {
            bytes,
            face_index,
            matched_family,
            matched_weight,
        })
    } else if let Some(fam) = query.font_family.as_deref() {
        Err(VectorError::FontNotFound(format!(
            "family '{fam}' inside font file '{}'",
            resolved_path.display()
        )))
    } else {
        Err(VectorError::InvalidFont(format!(
            "no valid font face found in '{}'",
            resolved_path.display()
        )))
    }
}

fn read_cached_font_bytes(path: &Path) -> Result<Arc<Vec<u8>>, VectorError> {
    let cache = FONT_BYTES_CACHE.get_or_init(|| RwLock::new(HashMap::new()));
    if let Ok(guard) = cache.read()
        && let Some(bytes) = guard.get(path)
    {
        return Ok(Arc::clone(bytes));
    }

    let raw = std::fs::read(path).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            VectorError::FontNotFound(path.display().to_string())
        } else {
            VectorError::InvalidFont(format!("failed to read font {}: {err}", path.display()))
        }
    })?;
    let arc = Arc::new(raw);
    if let Ok(mut guard) = cache.write() {
        guard.insert(path.to_path_buf(), Arc::clone(&arc));
    }
    Ok(arc)
}

fn build_indexed_face_entry(path: PathBuf, face_index: u32, face: &Face<'_>) -> IndexedFaceEntry {
    let weight = face.weight().to_number();
    let style_penalty = match face.style() {
        Style::Normal => 0,
        Style::Italic | Style::Oblique => 5,
    };

    let mut names: HashMap<String, (u32, String)> = HashMap::new();
    let mut primary_family = String::new();

    for name in face.names() {
        if !name.is_unicode() {
            continue;
        }
        let tier = match name.name_id {
            name_id::TYPOGRAPHIC_FAMILY | name_id::WWS_FAMILY => 0,
            name_id::FAMILY => 1,
            name_id::FULL_NAME => 2,
            _ => continue,
        };
        let Some(raw) = name.to_string() else {
            continue;
        };
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        if primary_family.is_empty() || tier == 0 {
            primary_family = trimmed.to_string();
        }
        let norm = normalize_font_name(trimmed);
        match names.get_mut(&norm) {
            Some((existing_tier, existing_disp)) => {
                if tier < *existing_tier {
                    *existing_tier = tier;
                    *existing_disp = trimmed.to_string();
                }
            }
            None => {
                names.insert(norm, (tier, trimmed.to_string()));
            }
        }
    }

    if primary_family.is_empty() {
        primary_family = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();
    }

    IndexedFaceEntry {
        path,
        face_index,
        weight,
        style_penalty,
        names,
        primary_family,
    }
}

fn normalize_font_name(name: &str) -> String {
    name.trim().to_lowercase()
}

fn collect_default_font_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(env_dirs) = std::env::var("FRAMESHIFT_FONT_DIRS") {
        for p in std::env::split_paths(&env_dirs) {
            if !p.as_os_str().is_empty() {
                dirs.push(p);
            }
        }
    }

    dirs.push(PathBuf::from("fonts"));
    dirs.push(PathBuf::from("assets").join("fonts"));

    #[cfg(target_os = "windows")]
    {
        if let Ok(windir) = std::env::var("WINDIR") {
            dirs.push(PathBuf::from(windir).join("Fonts"));
        } else {
            dirs.push(PathBuf::from(r"C:\Windows\Fonts"));
        }
        if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
            dirs.push(
                PathBuf::from(local_app_data)
                    .join("Microsoft")
                    .join("Windows")
                    .join("Fonts"),
            );
        }
    }

    #[cfg(target_os = "macos")]
    {
        dirs.push(PathBuf::from("/System/Library/Fonts"));
        dirs.push(PathBuf::from("/Library/Fonts"));
        if let Ok(home) = std::env::var("HOME") {
            dirs.push(PathBuf::from(home).join("Library").join("Fonts"));
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        dirs.push(PathBuf::from("/usr/share/fonts"));
        dirs.push(PathBuf::from("/usr/local/share/fonts"));
        if let Ok(home) = std::env::var("HOME") {
            let h = PathBuf::from(home);
            dirs.push(h.join(".local").join("share").join("fonts"));
            dirs.push(h.join(".fonts"));
        }
    }

    dirs
}

fn collect_font_files_recursive(dir: &Path, out: &mut BTreeSet<PathBuf>) {
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return;
    };
    let mut children: Vec<PathBuf> = read_dir
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .collect();
    children.sort();
    for path in children {
        if path.is_dir() {
            collect_font_files_recursive(&path, out);
        } else if is_supported_font_extension(&path) {
            out.insert(path);
        }
    }
}

fn is_supported_font_extension(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "ttf" | "otf" | "ttc" | "otc"
    )
}

fn flatten_closed_contours(path: &LyonPath, tolerance: f32) -> Vec<Vec<[f32; 2]>> {
    let mut contours = Vec::new();
    let mut current: Option<Vec<[f32; 2]>> = None;

    for event in path.iter().flattened(tolerance) {
        match event {
            Event::Begin { at } => {
                if let Some(pts) = current.take()
                    && pts.len() >= 3
                {
                    contours.push(pts);
                }
                current = Some(vec![[at.x, at.y]]);
            }
            Event::Line { to, .. } => {
                if let Some(pts) = current.as_mut() {
                    let next = [to.x, to.y];
                    if let Some(&prev) = pts.last() {
                        let dx = next[0] - prev[0];
                        let dy = next[1] - prev[1];
                        if dx * dx + dy * dy > 1e-10 {
                            pts.push(next);
                        }
                    } else {
                        pts.push(next);
                    }
                }
            }
            Event::End { .. } => {
                if let Some(mut pts) = current.take() {
                    if pts.len() >= 2 {
                        let first = pts[0];
                        let last = *pts.last().unwrap();
                        let dx = first[0] - last[0];
                        let dy = first[1] - last[1];
                        if dx * dx + dy * dy <= 1e-10 {
                            pts.pop();
                        }
                    }
                    if pts.len() >= 3 {
                        contours.push(pts);
                    }
                }
            }
            Event::Quadratic { .. } | Event::Cubic { .. } => {}
        }
    }

    if let Some(pts) = current.take()
        && pts.len() >= 3
    {
        contours.push(pts);
    }

    contours
}

fn contour_list_bounds(contours: &[Vec<[f32; 2]>]) -> Option<[f32; 4]> {
    let slices: Vec<&[[f32; 2]]> = contours.iter().map(|c| c.as_slice()).collect();
    contour_slices_bounds(&slices).map(|(a, b, c, d)| [a, b, c, d])
}

struct SegmentWithAabb {
    a: [f32; 2],
    b: [f32; 2],
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
}

fn collect_contour_segments_with_aabb(contours: &[Vec<[f32; 2]>]) -> Vec<SegmentWithAabb> {
    let total_edges: usize = contours.iter().map(|c| c.len()).sum();
    let mut segs = Vec::with_capacity(total_edges);
    for contour in contours {
        let n = contour.len();
        if n < 2 {
            continue;
        }
        for i in 0..n {
            let a = contour[i];
            let b = contour[(i + 1) % n];
            segs.push(SegmentWithAabb {
                a,
                b,
                min_x: a[0].min(b[0]),
                min_y: a[1].min(b[1]),
                max_x: a[0].max(b[0]),
                max_y: a[1].max(b[1]),
            });
        }
    }
    segs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_font_path_returns_font_not_found_without_fallback() {
        let query = FontQuery {
            font_path: Some("definitely_missing_font_xyz_12345.ttf".to_string()),
            font_family: None,
            font_weight: 400,
        };
        let err = resolve_font(&query, None).unwrap_err();
        assert!(matches!(err, VectorError::FontNotFound(_)));
    }

    #[test]
    fn missing_font_family_returns_font_not_found_without_fallback() {
        let index = SystemFontIndex::build_for_dirs(&[]);
        let query = FontQuery {
            font_path: None,
            font_family: Some("DefinitelyNonExistentFontFamilyXYZ".to_string()),
            font_weight: 700,
        };
        let err = index.resolve(&query, None).unwrap_err();
        assert!(matches!(err, VectorError::FontNotFound(_)));
    }

    #[test]
    fn invalid_ttf_config_rejected() {
        let cfg = TtfTextConfig {
            font_size_norm: f32::NAN,
            ..TtfTextConfig::default()
        };
        let err = rasterize_ttf_text_rgba16f(
            &cfg,
            128,
            128,
            Flicks::ZERO,
            AlphaMode::Premultiplied,
            None,
        )
        .unwrap_err();
        assert!(matches!(err, VectorError::InvalidParameter(_)));
    }
}
