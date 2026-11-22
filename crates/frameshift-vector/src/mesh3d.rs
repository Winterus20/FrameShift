use std::f32::consts::FRAC_PI_2;
use std::sync::Arc;

use frameshift_schema::{BevelProfile3D, TextAlign2D, WritingMode2D};
use lyon_tessellation::path::iterator::PathIterator;
use lyon_tessellation::path::{Event, Path};
use lyon_tessellation::{
    BuffersBuilder, FillOptions, FillRule, FillTessellator, FillVertex, LineCap, LineJoin,
    StrokeOptions, StrokeTessellator, StrokeVertex, VertexBuffers,
};

use crate::font_locator::ResolvedFont;
use crate::shaper::{shape_text_block, TextLayoutOptions, VariationAxes};
use crate::VectorError;

/// Single 3D vertex of an extruded glyph mesh.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeshVertex3D {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub char_center: [f32; 2],
    pub char_index: u32,
    /// `0 = front`, `1 = side`, `2 = back`, `3 = outline`.
    pub face_kind: u32,
}

/// Complete 3D extruded mesh for a single-line text string.
#[derive(Debug, Clone)]
pub struct ExtrudedTextMesh {
    pub vertices: Vec<MeshVertex3D>,
    pub indices: Vec<u32>,
    pub char_count: u32,
    pub bounds_min: [f32; 3],
    pub bounds_max: [f32; 3],
}

/// Loads raw TTF/OTF font bytes from disk without silent fallback.
pub fn load_font_bytes(path: &std::path::Path) -> Result<Vec<u8>, VectorError> {
    std::fs::read(path).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            VectorError::FontNotFound(path.display().to_string())
        } else {
            VectorError::InvalidFont(format!("failed to read font {}: {err}", path.display()))
        }
    })
}

/// Extended 3D typography extrusion options (shaping, variable axes, multi-line, curved bevels).
#[derive(Debug, Clone)]
pub struct Text3DMeshOptions<'a> {
    pub font_size: f32,
    pub extrude_depth: f32,
    pub bevel_size: f32,
    pub bevel_profile: BevelProfile3D,
    pub bevel_segments: u32,
    pub outline_width: f32,
    pub align: TextAlign2D,
    pub tracking_em: f32,
    pub line_height_em: Option<f32>,
    pub variations: VariationAxes,
    pub fallback_fonts: &'a [ResolvedFont],
}

impl Default for Text3DMeshOptions<'_> {
    fn default() -> Self {
        Self {
            font_size: 1.0,
            extrude_depth: 0.25,
            bevel_size: 0.04,
            bevel_profile: BevelProfile3D::Flat,
            bevel_segments: 4,
            outline_width: 0.0,
            align: TextAlign2D::Center,
            tracking_em: 0.0,
            line_height_em: None,
            variations: VariationAxes::default(),
            fallback_fonts: &[],
        }
    }
}

/// Parses `font_bytes` (TTF/OTF) and extrudes `text` into a centered 3D mesh (without bevel).
pub fn extrude_text_to_mesh(
    font_bytes: &[u8],
    text: &str,
    font_size: f32,
    extrude_depth: f32,
) -> Result<ExtrudedTextMesh, VectorError> {
    extrude_text_to_mesh_with_bevel(font_bytes, text, font_size, extrude_depth, 0.0)
}

/// Parses `font_bytes` (TTF/OTF) and extrudes `text` into a centered 3D mesh with optional chamfer bevel.
pub fn extrude_text_to_mesh_with_bevel(
    font_bytes: &[u8],
    text: &str,
    font_size: f32,
    extrude_depth: f32,
    bevel_size: f32,
) -> Result<ExtrudedTextMesh, VectorError> {
    extrude_text_to_mesh_with_outline(font_bytes, text, font_size, extrude_depth, bevel_size, 0.0)
}

/// Parses `font_bytes` (TTF/OTF) and extrudes `text` into a centered 3D mesh with optional
/// chamfer bevel and outer stroke/outline ring (`face_kind = 3`).
pub fn extrude_text_to_mesh_with_outline(
    font_bytes: &[u8],
    text: &str,
    font_size: f32,
    extrude_depth: f32,
    bevel_size: f32,
    outline_width: f32,
) -> Result<ExtrudedTextMesh, VectorError> {
    extrude_text_to_mesh_ex(
        font_bytes,
        text,
        &Text3DMeshOptions {
            font_size,
            extrude_depth,
            bevel_size,
            bevel_profile: BevelProfile3D::Flat,
            bevel_segments: 1,
            outline_width,
            align: TextAlign2D::Center,
            tracking_em: 0.0,
            line_height_em: None,
            variations: VariationAxes::default(),
            fallback_fonts: &[],
        },
    )
}

/// Parses `font_bytes` (TTF/OTF), shapes multi-line text with HarfRust/Skrifa (GPOS/GSUB,
/// variable font axes, fallback chain), and extrudes into a centered 3D mesh with curved bevels.
pub fn extrude_text_to_mesh_ex(
    font_bytes: &[u8],
    text: &str,
    opts: &Text3DMeshOptions<'_>,
) -> Result<ExtrudedTextMesh, VectorError> {
    let font_size = opts.font_size;
    let extrude_depth = opts.extrude_depth;
    let bevel_size = opts.bevel_size;
    let outline_width = opts.outline_width;

    if !font_size.is_finite() || font_size <= 0.0 {
        return Err(VectorError::InvalidParameter(format!(
            "font_size must be finite and > 0 (got {font_size})"
        )));
    }
    if !extrude_depth.is_finite() || extrude_depth < 0.0 {
        return Err(VectorError::InvalidParameter(format!(
            "extrude_depth must be finite and >= 0 (got {extrude_depth})"
        )));
    }
    if !bevel_size.is_finite() || bevel_size < 0.0 {
        return Err(VectorError::InvalidParameter(format!(
            "bevel_size must be finite and >= 0 (got {bevel_size})"
        )));
    }
    if !outline_width.is_finite() || outline_width < 0.0 {
        return Err(VectorError::InvalidParameter(format!(
            "outline_width must be finite and >= 0 (got {outline_width})"
        )));
    }

    let char_count = u32::try_from(text.chars().count()).map_err(|_| {
        VectorError::InvalidParameter("text length exceeds u32 capacity".to_string())
    })?;

    if text.is_empty() {
        return Ok(ExtrudedTextMesh {
            vertices: Vec::new(),
            indices: Vec::new(),
            char_count: 0,
            bounds_min: [0.0; 3],
            bounds_max: [0.0; 3],
        });
    }

    let primary_resolved = ResolvedFont {
        bytes: Arc::new(font_bytes.to_vec()),
        face_index: 0,
        matched_family: "text3d".to_string(),
        matched_weight: 400,
    };

    let shaped = shape_text_block(
        text,
        &primary_resolved,
        opts.fallback_fonts,
        &TextLayoutOptions {
            font_size,
            align: opts.align,
            writing_mode: WritingMode2D::HorizontalTb,
            tracking_em: opts.tracking_em,
            line_height_em: opts.line_height_em,
            variations: opts.variations,
            enable_ligatures: true,
        },
    )?;

    let tolerance = (font_size * 0.002).max(1e-5);
    let mut glyph_entries = Vec::with_capacity(shaped.glyphs.len());
    let mut line_min_x = f32::INFINITY;
    let mut line_max_x = f32::NEG_INFINITY;
    let mut line_min_y = f32::INFINITY;
    let mut line_max_y = f32::NEG_INFINITY;

    for sg in shaped.glyphs {
        let contours = flatten_contours(&sg.lyon_path, tolerance);
        if let Some([gmin_x, gmin_y, gmax_x, gmax_y]) = contour_bounds(&contours) {
            line_min_x = line_min_x.min(gmin_x);
            line_max_x = line_max_x.max(gmax_x);
            line_min_y = line_min_y.min(gmin_y);
            line_max_y = line_max_y.max(gmax_y);

            glyph_entries.push(LaidOutGlyph {
                char_index: sg.char_index,
                path: sg.lyon_path,
                contours,
                min_x: gmin_x,
                max_x: gmax_x,
                min_y: gmin_y,
                max_y: gmax_y,
            });
        }
    }

    if glyph_entries.is_empty() {
        return Ok(ExtrudedTextMesh {
            vertices: Vec::new(),
            indices: Vec::new(),
            char_count,
            bounds_min: [0.0; 3],
            bounds_max: [0.0; 3],
        });
    }

    let center_x = 0.5 * (line_min_x + line_max_x);
    let center_y = 0.5 * (line_min_y + line_max_y);
    let line_width = (line_max_x - line_min_x).max(1e-6);
    let line_height = (line_max_y - line_min_y).max(1e-6);
    let bevel = if bevel_size > 0.0 {
        if extrude_depth > 0.0 {
            bevel_size.min(0.45 * extrude_depth)
        } else {
            bevel_size.min(font_size * 0.05)
        }
    } else {
        0.0
    };
    let half_depth = if extrude_depth > 0.0 {
        0.5 * extrude_depth
    } else {
        bevel
    };

    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let mut tessellator = FillTessellator::new();
    let mut stroke_tessellator = (outline_width > 0.0).then(StrokeTessellator::new);
    let fill_options = FillOptions::tolerance(tolerance).with_fill_rule(FillRule::NonZero);

    for glyph in &glyph_entries {
        let char_center = [
            0.5 * (glyph.min_x + glyph.max_x) - center_x,
            0.5 * (glyph.min_y + glyph.max_y) - center_y,
        ];

        let mut geometry: VertexBuffers<[f32; 2], u32> = VertexBuffers::new();
        tessellator
            .tessellate_path(
                &glyph.path,
                &fill_options,
                &mut BuffersBuilder::new(&mut geometry, |vertex: FillVertex| {
                    let p = vertex.position();
                    [p.x, p.y]
                }),
            )
            .map_err(|err| {
                VectorError::InvalidFont(format!("glyph tessellation failed: {err:?}"))
            })?;

        append_caps(
            &geometry,
            glyph.char_index,
            char_center,
            [center_x, center_y],
            [line_min_x, line_min_y, line_width, line_height],
            half_depth,
            &mut vertices,
            &mut indices,
        )?;

        if extrude_depth > 0.0 || bevel > 0.0 {
            append_side_walls_with_profile(
                &glyph.contours,
                glyph.char_index,
                char_center,
                [center_x, center_y],
                [line_min_x, line_min_y, line_width, line_height],
                half_depth,
                bevel,
                opts.bevel_profile,
                opts.bevel_segments,
                &mut vertices,
                &mut indices,
            )?;
        }

        if let Some(stroke_tess) = stroke_tessellator.as_mut() {
            let stroke_opts = StrokeOptions::tolerance(tolerance)
                .with_line_width(outline_width * 2.0)
                .with_line_join(LineJoin::Round)
                .with_line_cap(LineCap::Round);
            append_stroke_outline(
                stroke_tess,
                &glyph.path,
                &stroke_opts,
                glyph.char_index,
                char_center,
                [center_x, center_y],
                [line_min_x, line_min_y, line_width, line_height],
                half_depth,
                extrude_depth > 0.0,
                &mut vertices,
                &mut indices,
            )?;
        }
    }

    let (bounds_min, bounds_max) = compute_mesh_bounds(&vertices);

    Ok(ExtrudedTextMesh {
        vertices,
        indices,
        char_count,
        bounds_min,
        bounds_max,
    })
}

struct LaidOutGlyph {
    char_index: u32,
    path: Path,
    contours: Vec<FlattenedContour>,
    min_x: f32,
    max_x: f32,
    min_y: f32,
    max_y: f32,
}

pub(crate) struct FlattenedContour {
    pub(crate) points: Vec<[f32; 2]>,
    pub(crate) closed: bool,
}

pub(crate) fn flatten_contours(path: &Path, tolerance: f32) -> Vec<FlattenedContour> {
    let mut contours = Vec::new();
    let mut current: Option<Vec<[f32; 2]>> = None;

    for event in path.iter().flattened(tolerance) {
        match event {
            Event::Begin { at } => {
                if let Some(pts) = current.take()
                    && pts.len() >= 2
                {
                    contours.push(FlattenedContour {
                        points: pts,
                        closed: false,
                    });
                }
                current = Some(vec![[at.x, at.y]]);
            }
            Event::Line { to, .. } => {
                if let Some(pts) = current.as_mut() {
                    let next = [to.x, to.y];
                    if let Some(&prev) = pts.last() {
                        if dist_sq(prev, next) > 1e-12 {
                            pts.push(next);
                        }
                    } else {
                        pts.push(next);
                    }
                }
            }
            Event::End { close, .. } => {
                if let Some(mut pts) = current.take() {
                    if close && pts.len() >= 2 {
                        let first = pts[0];
                        if let Some(&last) = pts.last()
                            && dist_sq(first, last) <= 1e-12
                        {
                            pts.pop();
                        }
                    }
                    if pts.len() >= 2 {
                        contours.push(FlattenedContour {
                            points: pts,
                            closed: close,
                        });
                    }
                }
            }
            Event::Quadratic { .. } | Event::Cubic { .. } => {}
        }
    }

    if let Some(pts) = current.take()
        && pts.len() >= 2
    {
        contours.push(FlattenedContour {
            points: pts,
            closed: false,
        });
    }

    contours
}

fn dist_sq(a: [f32; 2], b: [f32; 2]) -> f32 {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    dx * dx + dy * dy
}

pub(crate) fn contour_bounds(contours: &[FlattenedContour]) -> Option<[f32; 4]> {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    let mut any = false;

    for contour in contours {
        for &[x, y] in &contour.points {
            any = true;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }

    if any {
        Some([min_x, min_y, max_x, max_y])
    } else {
        None
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn append_caps(
    geometry: &VertexBuffers<[f32; 2], u32>,
    char_index: u32,
    char_center: [f32; 2],
    center_offset: [f32; 2],
    uv_rect: [f32; 4],
    half_depth: f32,
    vertices: &mut Vec<MeshVertex3D>,
    indices: &mut Vec<u32>,
) -> Result<(), VectorError> {
    append_planar_caps(
        geometry,
        char_index,
        char_center,
        center_offset,
        uv_rect,
        half_depth,
        0,
        Some((-half_depth, 2)),
        vertices,
        indices,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn append_planar_caps(
    geometry: &VertexBuffers<[f32; 2], u32>,
    char_index: u32,
    char_center: [f32; 2],
    center_offset: [f32; 2],
    uv_rect: [f32; 4],
    front_z: f32,
    front_face_kind: u32,
    back_cap: Option<(f32, u32)>,
    vertices: &mut Vec<MeshVertex3D>,
    indices: &mut Vec<u32>,
) -> Result<(), VectorError> {
    if geometry.vertices.is_empty() || geometry.indices.is_empty() {
        return Ok(());
    }

    let front_base = u32::try_from(vertices.len())
        .map_err(|_| VectorError::InvalidParameter("mesh vertex count overflow".to_string()))?;

    for &[x, y] in &geometry.vertices {
        vertices.push(make_vertex(
            x,
            y,
            front_z,
            [0.0, 0.0, 1.0],
            center_offset,
            uv_rect,
            char_center,
            char_index,
            front_face_kind,
        ));
    }

    let back_base = if let Some((back_z, back_face_kind)) = back_cap {
        let base = u32::try_from(vertices.len())
            .map_err(|_| VectorError::InvalidParameter("mesh vertex count overflow".to_string()))?;
        for &[x, y] in &geometry.vertices {
            vertices.push(make_vertex(
                x,
                y,
                back_z,
                [0.0, 0.0, -1.0],
                center_offset,
                uv_rect,
                char_center,
                char_index,
                back_face_kind,
            ));
        }
        Some(base)
    } else {
        None
    };

    for &[i0, i1, i2] in geometry.indices.as_chunks::<3>().0 {
        let p0 = geometry.vertices[i0 as usize];
        let p1 = geometry.vertices[i1 as usize];
        let p2 = geometry.vertices[i2 as usize];
        let cross_z = (p1[0] - p0[0]) * (p2[1] - p0[1]) - (p1[1] - p0[1]) * (p2[0] - p0[0]);

        if cross_z > 0.0 {
            indices.extend_from_slice(&[front_base + i0, front_base + i1, front_base + i2]);
            if let Some(back) = back_base {
                indices.extend_from_slice(&[back + i0, back + i2, back + i1]);
            }
        } else if cross_z < 0.0 {
            indices.extend_from_slice(&[front_base + i0, front_base + i2, front_base + i1]);
            if let Some(back) = back_base {
                indices.extend_from_slice(&[back + i0, back + i1, back + i2]);
            }
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn append_stroke_outline(
    stroke_tessellator: &mut StrokeTessellator,
    path: &Path,
    stroke_options: &StrokeOptions,
    char_index: u32,
    char_center: [f32; 2],
    center_offset: [f32; 2],
    uv_rect: [f32; 4],
    half_depth: f32,
    emit_back: bool,
    vertices: &mut Vec<MeshVertex3D>,
    indices: &mut Vec<u32>,
) -> Result<(), VectorError> {
    let mut stroke_geometry: VertexBuffers<[f32; 2], u32> = VertexBuffers::new();
    stroke_tessellator
        .tessellate_path(
            path,
            stroke_options,
            &mut BuffersBuilder::new(&mut stroke_geometry, |vertex: StrokeVertex| {
                let p = vertex.position();
                [p.x, p.y]
            }),
        )
        .map_err(|err| {
            VectorError::InvalidFont(format!("outline stroke tessellation failed: {err:?}"))
        })?;

    let z_front = half_depth - 0.001;
    let back_cap = emit_back.then_some((-half_depth + 0.001, 3));
    append_planar_caps(
        &stroke_geometry,
        char_index,
        char_center,
        center_offset,
        uv_rect,
        z_front,
        3,
        back_cap,
        vertices,
        indices,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn append_side_walls(
    contours: &[FlattenedContour],
    char_index: u32,
    char_center: [f32; 2],
    center_offset: [f32; 2],
    uv_rect: [f32; 4],
    half_depth: f32,
    bevel: f32,
    vertices: &mut Vec<MeshVertex3D>,
    indices: &mut Vec<u32>,
) -> Result<(), VectorError> {
    append_side_walls_with_profile(
        contours,
        char_index,
        char_center,
        center_offset,
        uv_rect,
        half_depth,
        bevel,
        BevelProfile3D::Flat,
        1,
        vertices,
        indices,
    )
}

fn sample_front_bevel_profile(
    bevel: f32,
    half_depth: f32,
    profile: BevelProfile3D,
    segments: u32,
) -> Vec<(f32, f32)> {
    let segs = segments.clamp(1, 16) as usize;
    match profile {
        BevelProfile3D::Flat => vec![(bevel, half_depth), (0.0, half_depth - bevel)],
        BevelProfile3D::Round => {
            let mut pts = Vec::with_capacity(segs + 1);
            for i in 0..=segs {
                let t = i as f32 / segs as f32;
                let ang = t * FRAC_PI_2;
                let inward = bevel * (1.0 - ang.sin());
                let z = half_depth - bevel * (1.0 - ang.cos());
                pts.push((inward, z));
            }
            pts
        }
        BevelProfile3D::Concave => {
            let mut pts = Vec::with_capacity(segs + 1);
            for i in 0..=segs {
                let t = i as f32 / segs as f32;
                let ang = t * FRAC_PI_2;
                let inward = bevel * ang.cos();
                let z = half_depth - bevel * ang.sin();
                pts.push((inward, z));
            }
            pts
        }
        BevelProfile3D::Stepped => {
            let steps = (segs / 2).clamp(2, 6);
            let mut pts = Vec::with_capacity(steps * 2 + 1);
            pts.push((bevel, half_depth));
            for s in 0..steps {
                let t_next = (s + 1) as f32 / steps as f32;
                let next_in = bevel * (1.0 - t_next);
                let prev_z = half_depth - bevel * (s as f32 / steps as f32);
                let next_z = half_depth - bevel * t_next;
                pts.push((next_in, prev_z));
                pts.push((next_in, next_z));
            }
            pts
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn append_side_walls_with_profile(
    contours: &[FlattenedContour],
    char_index: u32,
    char_center: [f32; 2],
    center_offset: [f32; 2],
    uv_rect: [f32; 4],
    half_depth: f32,
    bevel: f32,
    bevel_profile: BevelProfile3D,
    bevel_segments: u32,
    vertices: &mut Vec<MeshVertex3D>,
    indices: &mut Vec<u32>,
) -> Result<(), VectorError> {
    let total_area: f32 = contours
        .iter()
        .filter(|c| c.closed && c.points.len() >= 3)
        .map(|c| signed_area(&c.points))
        .sum();
    let ccw_outer = total_area >= 0.0;

    let front_profile = if bevel > 0.0 {
        sample_front_bevel_profile(bevel, half_depth, bevel_profile, bevel_segments)
    } else {
        Vec::new()
    };

    for contour in contours {
        let n = contour.points.len();
        if n < 2 {
            continue;
        }
        let edge_count = if contour.closed { n } else { n - 1 };

        for i in 0..edge_count {
            let p0 = contour.points[i];
            let p1 = contour.points[(i + 1) % n];
            let dx = p1[0] - p0[0];
            let dy = p1[1] - p0[1];
            let len = (dx * dx + dy * dy).sqrt();
            if len <= 1e-6 {
                continue;
            }

            let [nx, ny] = if ccw_outer {
                [dy / len, -dx / len]
            } else {
                [-dy / len, dx / len]
            };

            if bevel > 0.0 {
                for w in front_profile.windows(2) {
                    let (in0, z0) = w[0];
                    let (in1, z1) = w[1];
                    let d_out = in0 - in1;
                    let d_z = z1 - z0;
                    let normal = normalize3([nx * (-d_z), ny * (-d_z), d_out]);
                    let top_p0 = [p0[0] - nx * in0, p0[1] - ny * in0];
                    let top_p1 = [p1[0] - nx * in0, p1[1] - ny * in0];
                    let bot_p0 = [p0[0] - nx * in1, p0[1] - ny * in1];
                    let bot_p1 = [p1[0] - nx * in1, p1[1] - ny * in1];
                    push_side_quad(
                        top_p0,
                        top_p1,
                        z0,
                        bot_p0,
                        bot_p1,
                        z1,
                        normal,
                        ccw_outer,
                        char_index,
                        char_center,
                        center_offset,
                        uv_rect,
                        vertices,
                        indices,
                    )?;
                }

                let z_front_mid = half_depth - bevel;
                let z_back_mid = -half_depth + bevel;
                if z_front_mid > z_back_mid + 1e-6 {
                    push_side_quad(
                        p0,
                        p1,
                        z_front_mid,
                        p0,
                        p1,
                        z_back_mid,
                        [nx, ny, 0.0],
                        ccw_outer,
                        char_index,
                        char_center,
                        center_offset,
                        uv_rect,
                        vertices,
                        indices,
                    )?;
                }

                // Back bevel mirrors the front profile in reverse (`-z` from `z_back_mid` to `-half_depth`).
                let back_pts: Vec<(f32, f32)> = front_profile
                    .iter()
                    .rev()
                    .map(|&(inward, z)| (inward, -z))
                    .collect();
                for w in back_pts.windows(2) {
                    let (in0, z0) = w[0];
                    let (in1, z1) = w[1];
                    let d_out = in0 - in1;
                    let d_z = z1 - z0;
                    let normal = normalize3([nx * (-d_z), ny * (-d_z), d_out]);
                    let top_p0 = [p0[0] - nx * in0, p0[1] - ny * in0];
                    let top_p1 = [p1[0] - nx * in0, p1[1] - ny * in0];
                    let bot_p0 = [p0[0] - nx * in1, p0[1] - ny * in1];
                    let bot_p1 = [p1[0] - nx * in1, p1[1] - ny * in1];
                    push_side_quad(
                        top_p0,
                        top_p1,
                        z0,
                        bot_p0,
                        bot_p1,
                        z1,
                        normal,
                        ccw_outer,
                        char_index,
                        char_center,
                        center_offset,
                        uv_rect,
                        vertices,
                        indices,
                    )?;
                }
            } else {
                push_side_quad(
                    p0,
                    p1,
                    half_depth,
                    p0,
                    p1,
                    -half_depth,
                    [nx, ny, 0.0],
                    ccw_outer,
                    char_index,
                    char_center,
                    center_offset,
                    uv_rect,
                    vertices,
                    indices,
                )?;
            }
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn push_side_quad(
    top_p0: [f32; 2],
    top_p1: [f32; 2],
    top_z: f32,
    bot_p0: [f32; 2],
    bot_p1: [f32; 2],
    bot_z: f32,
    normal: [f32; 3],
    ccw_outer: bool,
    char_index: u32,
    char_center: [f32; 2],
    center_offset: [f32; 2],
    uv_rect: [f32; 4],
    vertices: &mut Vec<MeshVertex3D>,
    indices: &mut Vec<u32>,
) -> Result<(), VectorError> {
    let base = u32::try_from(vertices.len())
        .map_err(|_| VectorError::InvalidParameter("mesh vertex count overflow".to_string()))?;

    vertices.push(make_vertex(
        top_p0[0],
        top_p0[1],
        top_z,
        normal,
        center_offset,
        uv_rect,
        char_center,
        char_index,
        1,
    ));
    vertices.push(make_vertex(
        top_p1[0],
        top_p1[1],
        top_z,
        normal,
        center_offset,
        uv_rect,
        char_center,
        char_index,
        1,
    ));
    vertices.push(make_vertex(
        bot_p1[0],
        bot_p1[1],
        bot_z,
        normal,
        center_offset,
        uv_rect,
        char_center,
        char_index,
        1,
    ));
    vertices.push(make_vertex(
        bot_p0[0],
        bot_p0[1],
        bot_z,
        normal,
        center_offset,
        uv_rect,
        char_center,
        char_index,
        1,
    ));

    if ccw_outer {
        indices.extend_from_slice(&[base, base + 3, base + 2, base, base + 2, base + 1]);
    } else {
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    Ok(())
}

fn normalize3(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len <= 1e-12 {
        [0.0, 0.0, 1.0]
    } else {
        [v[0] / len, v[1] / len, v[2] / len]
    }
}

fn signed_area(points: &[[f32; 2]]) -> f32 {
    let mut sum = 0.0_f32;
    let n = points.len();
    for i in 0..n {
        let a = points[i];
        let b = points[(i + 1) % n];
        sum += a[0] * b[1] - b[0] * a[1];
    }
    0.5 * sum
}

#[allow(clippy::too_many_arguments)]
fn make_vertex(
    x: f32,
    y: f32,
    z: f32,
    normal: [f32; 3],
    center_offset: [f32; 2],
    uv_rect: [f32; 4],
    char_center: [f32; 2],
    char_index: u32,
    face_kind: u32,
) -> MeshVertex3D {
    let u = ((x - uv_rect[0]) / uv_rect[2]).clamp(0.0, 1.0);
    let v = ((y - uv_rect[1]) / uv_rect[3]).clamp(0.0, 1.0);
    MeshVertex3D {
        position: [x - center_offset[0], y - center_offset[1], z],
        normal,
        uv: [u, v],
        char_center,
        char_index,
        face_kind,
    }
}

pub(crate) fn compute_mesh_bounds(vertices: &[MeshVertex3D]) -> ([f32; 3], [f32; 3]) {
    if vertices.is_empty() {
        return ([0.0; 3], [0.0; 3]);
    }
    let mut bmin = [f32::INFINITY; 3];
    let mut bmax = [f32::NEG_INFINITY; 3];
    for v in vertices {
        for axis in 0..3 {
            bmin[axis] = bmin[axis].min(v.position[axis]);
            bmax[axis] = bmax[axis].max(v.position[axis]);
        }
    }
    (bmin, bmax)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outline_width_validation_rejects_negative_and_non_finite() {
        let neg = extrude_text_to_mesh_with_outline(b"", "A", 32.0, 4.0, 0.0, -0.5)
            .expect_err("negative outline_width must fail");
        assert!(matches!(neg, VectorError::InvalidParameter(_)));

        let nan = extrude_text_to_mesh_with_outline(b"", "A", 32.0, 4.0, 0.0, f32::NAN)
            .expect_err("NaN outline_width must fail");
        assert!(matches!(nan, VectorError::InvalidParameter(_)));
    }

    #[test]
    fn append_stroke_outline_emits_face_kind_3_for_3d_and_flat_modes() {
        use lyon_tessellation::math::point;
        let mut builder = Path::builder();
        builder.begin(point(-10.0, -10.0));
        builder.line_to(point(10.0, -10.0));
        builder.line_to(point(10.0, 10.0));
        builder.line_to(point(-10.0, 10.0));
        builder.close();
        let path = builder.build();

        let opts = StrokeOptions::tolerance(0.05)
            .with_line_width(4.0)
            .with_line_join(LineJoin::Round)
            .with_line_cap(LineCap::Round);

        // 3D mode (emit_back = true)
        let mut verts_3d = Vec::new();
        let mut idx_3d = Vec::new();
        let mut stroke_tess = StrokeTessellator::new();
        append_stroke_outline(
            &mut stroke_tess,
            &path,
            &opts,
            0,
            [0.0, 0.0],
            [0.0, 0.0],
            [-10.0, -10.0, 20.0, 20.0],
            5.0,
            true,
            &mut verts_3d,
            &mut idx_3d,
        )
        .expect("3d stroke outline");

        assert!(!verts_3d.is_empty());
        assert_eq!(idx_3d.len() % 3, 0);
        let mut has_front_stroke = false;
        let mut has_back_stroke = false;
        for v in &verts_3d {
            assert_eq!(v.face_kind, 3);
            if v.normal == [0.0, 0.0, 1.0] {
                assert!((v.position[2] - 4.999).abs() < 1e-4);
                has_front_stroke = true;
            } else if v.normal == [0.0, 0.0, -1.0] {
                assert!((v.position[2] + 4.999).abs() < 1e-4);
                has_back_stroke = true;
            } else {
                panic!("unexpected stroke normal {:?}", v.normal);
            }
        }
        assert!(has_front_stroke && has_back_stroke);

        for &tri in idx_3d.as_chunks::<3>().0 {
            let v0 = verts_3d[tri[0] as usize];
            let v1 = verts_3d[tri[1] as usize];
            let v2 = verts_3d[tri[2] as usize];
            let e1 = [
                v1.position[0] - v0.position[0],
                v1.position[1] - v0.position[1],
                v1.position[2] - v0.position[2],
            ];
            let e2 = [
                v2.position[0] - v0.position[0],
                v2.position[1] - v0.position[1],
                v2.position[2] - v0.position[2],
            ];
            let fnrm = [
                e1[1] * e2[2] - e1[2] * e2[1],
                e1[2] * e2[0] - e1[0] * e2[2],
                e1[0] * e2[1] - e1[1] * e2[0],
            ];
            let dot = fnrm[0] * v0.normal[0] + fnrm[1] * v0.normal[1] + fnrm[2] * v0.normal[2];
            assert!(dot > 0.0, "stroke triangle winding opposes normal: dot={dot}");
        }

        // Flat 2D mode (half_depth = 0.0, emit_back = false)
        let mut verts_2d = Vec::new();
        let mut idx_2d = Vec::new();
        append_stroke_outline(
            &mut stroke_tess,
            &path,
            &opts,
            0,
            [0.0, 0.0],
            [0.0, 0.0],
            [-10.0, -10.0, 20.0, 20.0],
            0.0,
            false,
            &mut verts_2d,
            &mut idx_2d,
        )
        .expect("2d stroke outline");

        assert!(!verts_2d.is_empty());
        assert!(verts_2d
            .iter()
            .all(|v| v.face_kind == 3 && v.normal == [0.0, 0.0, 1.0] && (v.position[2] + 0.001).abs() < 1e-5));
    }
}

