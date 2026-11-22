use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};

use lyon_tessellation::math::point;
use lyon_tessellation::path::path::Builder as PathBuilder;
use lyon_tessellation::path::Path;
use lyon_tessellation::{
    BuffersBuilder, FillOptions, FillRule, FillTessellator, FillVertex, LineCap, LineJoin,
    StrokeOptions, StrokeTessellator, VertexBuffers,
};

use crate::mesh3d::{
    append_caps, append_planar_caps, append_side_walls, append_stroke_outline, compute_mesh_bounds,
    contour_bounds, flatten_contours, ExtrudedTextMesh, MeshVertex3D,
};
use crate::VectorError;

/// Procedural 3D vector shape variants rendered via the shared `MeshVertex3D` pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProceduralShapeKind {
    NeonTile,
    HollowStar5,
    Heart3D,
    LightningBolt,
    Diamond3D,
}

/// Parameters controlling procedural 3D shape generation in `build_shape3d_mesh`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shape3DParams {
    pub kind: ProceduralShapeKind,
    pub size: f32,
    pub aspect: f32,
    pub stroke_width: f32,
    pub corner_radius: f32,
    pub inner_ratio: f32,
    pub extrude_depth: f32,
    pub bevel_size: f32,
}

/// Builds a centered, finite, non-empty `ExtrudedTextMesh` for the requested procedural 3D shape.
pub fn build_shape3d_mesh(params: Shape3DParams) -> Result<ExtrudedTextMesh, VectorError> {
    validate_shape_params(&params)?;

    let bevel = if params.bevel_size > 0.0 {
        if params.extrude_depth > 0.0 {
            params.bevel_size.min(0.45 * params.extrude_depth)
        } else {
            params.bevel_size.min(params.size * 0.05)
        }
    } else {
        0.0
    };
    let half_depth = if params.extrude_depth > 0.0 {
        0.5 * params.extrude_depth
    } else {
        bevel
    };
    let tolerance = (params.size * 0.002).max(1e-5);

    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    match params.kind {
        ProceduralShapeKind::NeonTile => {
            build_neon_tile(params, half_depth, bevel, tolerance, &mut vertices, &mut indices)?;
        }
        ProceduralShapeKind::HollowStar5 => {
            build_hollow_star5(params, half_depth, bevel, tolerance, &mut vertices, &mut indices)?;
        }
        ProceduralShapeKind::Heart3D => {
            build_heart3d(params, half_depth, bevel, tolerance, &mut vertices, &mut indices)?;
        }
        ProceduralShapeKind::LightningBolt => {
            build_lightning_bolt(params, half_depth, bevel, tolerance, &mut vertices, &mut indices)?;
        }
        ProceduralShapeKind::Diamond3D => {
            build_diamond3d(params, half_depth, bevel, tolerance, &mut vertices, &mut indices)?;
        }
    }

    let (mut bounds_min, mut bounds_max) = compute_mesh_bounds(&vertices);
    let shift_x = 0.5 * (bounds_min[0] + bounds_max[0]);
    let shift_y = 0.5 * (bounds_min[1] + bounds_max[1]);
    if shift_x.abs() > 1e-6 || shift_y.abs() > 1e-6 {
        for v in &mut vertices {
            v.position[0] -= shift_x;
            v.position[1] -= shift_y;
        }
        (bounds_min, bounds_max) = compute_mesh_bounds(&vertices);
    }

    Ok(ExtrudedTextMesh {
        vertices,
        indices,
        char_count: 1,
        bounds_min,
        bounds_max,
    })
}

fn validate_shape_params(params: &Shape3DParams) -> Result<(), VectorError> {
    if !params.size.is_finite() || params.size <= 0.0 {
        return Err(VectorError::InvalidParameter(format!(
            "shape size must be finite and > 0 (got {})",
            params.size
        )));
    }
    if !params.aspect.is_finite() || params.aspect <= 0.0 {
        return Err(VectorError::InvalidParameter(format!(
            "shape aspect must be finite and > 0 (got {})",
            params.aspect
        )));
    }
    if !params.stroke_width.is_finite() || params.stroke_width < 0.0 {
        return Err(VectorError::InvalidParameter(format!(
            "shape stroke_width must be finite and >= 0 (got {})",
            params.stroke_width
        )));
    }
    if !params.corner_radius.is_finite() || params.corner_radius < 0.0 {
        return Err(VectorError::InvalidParameter(format!(
            "shape corner_radius must be finite and >= 0 (got {})",
            params.corner_radius
        )));
    }
    if !params.inner_ratio.is_finite() || params.inner_ratio < 0.0 {
        return Err(VectorError::InvalidParameter(format!(
            "shape inner_ratio must be finite and >= 0 (got {})",
            params.inner_ratio
        )));
    }
    if !params.extrude_depth.is_finite() || params.extrude_depth < 0.0 {
        return Err(VectorError::InvalidParameter(format!(
            "shape extrude_depth must be finite and >= 0 (got {})",
            params.extrude_depth
        )));
    }
    if !params.bevel_size.is_finite() || params.bevel_size < 0.0 {
        return Err(VectorError::InvalidParameter(format!(
            "shape bevel_size must be finite and >= 0 (got {})",
            params.bevel_size
        )));
    }
    Ok(())
}

fn build_neon_tile(
    params: Shape3DParams,
    half_depth: f32,
    bevel: f32,
    tolerance: f32,
    vertices: &mut Vec<MeshVertex3D>,
    indices: &mut Vec<u32>,
) -> Result<(), VectorError> {
    let w = params.size * params.aspect;
    let h = params.size;
    let r_out = params.corner_radius.min(0.49 * w.min(h));
    let bw = params.stroke_width.min(0.45 * w.min(h));

    let mut outer_builder = Path::builder();
    append_rounded_rect_subpath(&mut outer_builder, w, h, r_out, false);
    let outer_path = outer_builder.build();

    if bw <= 1e-6 {
        return extrude_filled_path(
            &outer_path,
            FillRule::EvenOdd,
            half_depth,
            bevel,
            params.extrude_depth,
            tolerance,
            None,
            vertices,
            indices,
        );
    }

    let w_in = (w - 2.0 * bw).max(1e-4);
    let h_in = (h - 2.0 * bw).max(1e-4);
    let r_in = (r_out - bw).max(0.0).min(0.49 * w_in.min(h_in));

    let mut inner_builder = Path::builder();
    append_rounded_rect_subpath(&mut inner_builder, w_in, h_in, r_in, false);
    let inner_path = inner_builder.build();

    let mut frame_builder = Path::builder();
    append_rounded_rect_subpath(&mut frame_builder, w, h, r_out, false);
    append_rounded_rect_subpath(&mut frame_builder, w_in, h_in, r_in, true);
    let frame_path = frame_builder.build();

    let outer_contours = flatten_contours(&outer_path, tolerance);
    let uv_rect = [-0.5 * w, -0.5 * h, w.max(1e-6), h.max(1e-6)];
    let center_offset = [0.0, 0.0];
    let char_center = [0.0, 0.0];

    let mut fill_tess = FillTessellator::new();
    let fill_opts = FillOptions::tolerance(tolerance).with_fill_rule(FillRule::EvenOdd);

    let inner_geom = tessellate_fill(&mut fill_tess, &inner_path, &fill_opts)?;
    append_caps(
        &inner_geom,
        0,
        char_center,
        center_offset,
        uv_rect,
        half_depth,
        vertices,
        indices,
    )?;

    let frame_geom = tessellate_fill(&mut fill_tess, &frame_path, &fill_opts)?;
    append_planar_caps(
        &frame_geom,
        0,
        char_center,
        center_offset,
        uv_rect,
        half_depth,
        3,
        Some((-half_depth, 3)),
        vertices,
        indices,
    )?;

    let eff_bevel = bevel.min(0.45 * bw);
    if params.extrude_depth > 0.0 || eff_bevel > 0.0 {
        append_side_walls(
            &outer_contours,
            0,
            char_center,
            center_offset,
            uv_rect,
            half_depth,
            eff_bevel,
            vertices,
            indices,
        )?;
    }

    Ok(())
}

fn build_hollow_star5(
    params: Shape3DParams,
    half_depth: f32,
    bevel: f32,
    tolerance: f32,
    vertices: &mut Vec<MeshVertex3D>,
    indices: &mut Vec<u32>,
) -> Result<(), VectorError> {
    let r_out = 0.5 * params.size;
    let r_in = r_out * params.inner_ratio.clamp(0.15, 0.85);

    let mut base_pts = [[0.0_f32; 2]; 10];
    for (k, pt) in base_pts.iter_mut().enumerate() {
        let angle = FRAC_PI_2 + (k as f32) * (PI / 5.0);
        let r = if k % 2 == 0 { r_out } else { r_in };
        *pt = [r * angle.cos(), r * angle.sin()];
    }

    let eff_stroke = if params.stroke_width > 0.0 {
        params.stroke_width
    } else {
        0.08 * params.size
    };
    let half_w = (0.5 * eff_stroke).min(0.35 * r_in);

    let mut outer_pts = [[0.0_f32; 2]; 10];
    let mut inner_pts = [[0.0_f32; 2]; 10];

    for i in 0..10 {
        let p_prev = base_pts[(i + 9) % 10];
        let p = base_pts[i];
        let p_next = base_pts[(i + 1) % 10];

        let e_prev = normalize2([p[0] - p_prev[0], p[1] - p_prev[1]]);
        let e_next = normalize2([p_next[0] - p[0], p_next[1] - p[1]]);

        // Outward edge normals for CCW contour
        let n_prev = [e_prev[1], -e_prev[0]];
        let n_next = [e_next[1], -e_next[0]];

        let m_dir = normalize2([n_prev[0] + n_next[0], n_prev[1] + n_next[1]]);
        let dot = (m_dir[0] * n_prev[0] + m_dir[1] * n_prev[1]).max(0.25);
        let miter = [m_dir[0] / dot, m_dir[1] / dot];

        outer_pts[i] = [p[0] + miter[0] * half_w, p[1] + miter[1] * half_w];
        inner_pts[i] = [p[0] - miter[0] * half_w, p[1] - miter[1] * half_w];
    }

    let mut builder = Path::builder();
    builder.begin(point(outer_pts[0][0], outer_pts[0][1]));
    for &[x, y] in &outer_pts[1..] {
        builder.line_to(point(x, y));
    }
    builder.close();

    // Inner hole wound CW so FillRule::EvenOdd + append_side_walls point inward into the hole.
    builder.begin(point(inner_pts[9][0], inner_pts[9][1]));
    for &[x, y] in inner_pts[..9].iter().rev() {
        builder.line_to(point(x, y));
    }
    builder.close();

    let path = builder.build();
    let eff_bevel = bevel.min(0.45 * (2.0 * half_w));

    extrude_filled_path(
        &path,
        FillRule::EvenOdd,
        half_depth,
        eff_bevel,
        params.extrude_depth,
        tolerance,
        None,
        vertices,
        indices,
    )
}

fn build_heart3d(
    params: Shape3DParams,
    half_depth: f32,
    bevel: f32,
    tolerance: f32,
    vertices: &mut Vec<MeshVertex3D>,
    indices: &mut Vec<u32>,
) -> Result<(), VectorError> {
    let s = params.size;
    let mut builder = Path::builder();
    builder.begin(point(0.0, -0.44 * s));
    builder.cubic_bezier_to(
        point(0.14 * s, -0.32 * s),
        point(0.48 * s, -0.12 * s),
        point(0.48 * s, 0.14 * s),
    );
    builder.cubic_bezier_to(
        point(0.48 * s, 0.40 * s),
        point(0.14 * s, 0.46 * s),
        point(0.0, 0.22 * s),
    );
    builder.cubic_bezier_to(
        point(-0.14 * s, 0.46 * s),
        point(-0.48 * s, 0.40 * s),
        point(-0.48 * s, 0.14 * s),
    );
    builder.cubic_bezier_to(
        point(-0.48 * s, -0.12 * s),
        point(-0.14 * s, -0.32 * s),
        point(0.0, -0.44 * s),
    );
    builder.close();
    let path = builder.build();

    let stroke_opts = (params.stroke_width > 0.0).then(|| {
        StrokeOptions::tolerance(tolerance)
            .with_line_width(params.stroke_width * 2.0)
            .with_line_join(LineJoin::Round)
            .with_line_cap(LineCap::Round)
    });

    extrude_filled_path(
        &path,
        FillRule::NonZero,
        half_depth,
        bevel,
        params.extrude_depth,
        tolerance,
        stroke_opts.as_ref(),
        vertices,
        indices,
    )
}

fn build_lightning_bolt(
    params: Shape3DParams,
    half_depth: f32,
    bevel: f32,
    tolerance: f32,
    vertices: &mut Vec<MeshVertex3D>,
    indices: &mut Vec<u32>,
) -> Result<(), VectorError> {
    const BOLT_PTS: [[f32; 2]; 7] = [
        [-0.08, 0.48],
        [-0.34, 0.02],
        [-0.04, 0.02],
        [-0.22, -0.48],
        [0.34, 0.06],
        [0.04, 0.06],
        [0.24, 0.48],
    ];
    let s = params.size;
    let mut builder = Path::builder();
    builder.begin(point(BOLT_PTS[0][0] * s, BOLT_PTS[0][1] * s));
    for &[x, y] in &BOLT_PTS[1..] {
        builder.line_to(point(x * s, y * s));
    }
    builder.close();
    let path = builder.build();

    let stroke_opts = (params.stroke_width > 0.0).then(|| {
        StrokeOptions::tolerance(tolerance)
            .with_line_width(params.stroke_width * 2.0)
            .with_line_join(LineJoin::Miter)
            .with_miter_limit(3.0)
            .with_line_cap(LineCap::Butt)
    });

    extrude_filled_path(
        &path,
        FillRule::NonZero,
        half_depth,
        bevel,
        params.extrude_depth,
        tolerance,
        stroke_opts.as_ref(),
        vertices,
        indices,
    )
}

fn build_diamond3d(
    params: Shape3DParams,
    half_depth: f32,
    bevel: f32,
    tolerance: f32,
    vertices: &mut Vec<MeshVertex3D>,
    indices: &mut Vec<u32>,
) -> Result<(), VectorError> {
    let r_out = 0.5 * params.size;
    let aspect_x = params.aspect;

    let mut builder = Path::builder();
    if params.inner_ratio >= 0.99 {
        builder.begin(point(r_out * aspect_x, 0.0));
        builder.line_to(point(0.0, r_out));
        builder.line_to(point(-r_out * aspect_x, 0.0));
        builder.line_to(point(0.0, -r_out));
        builder.close();
    } else {
        let r_in = r_out * params.inner_ratio.clamp(0.15, 0.95);
        for k in 0..8 {
            let angle = (k as f32) * FRAC_PI_4;
            let r = if k % 2 == 0 { r_out } else { r_in };
            let x = r * angle.cos() * aspect_x;
            let y = r * angle.sin();
            if k == 0 {
                builder.begin(point(x, y));
            } else {
                builder.line_to(point(x, y));
            }
        }
        builder.close();
    }
    let path = builder.build();

    let stroke_opts = (params.stroke_width > 0.0).then(|| {
        StrokeOptions::tolerance(tolerance)
            .with_line_width(params.stroke_width * 2.0)
            .with_line_join(LineJoin::Miter)
            .with_miter_limit(3.0)
            .with_line_cap(LineCap::Butt)
    });

    extrude_filled_path(
        &path,
        FillRule::NonZero,
        half_depth,
        bevel,
        params.extrude_depth,
        tolerance,
        stroke_opts.as_ref(),
        vertices,
        indices,
    )
}

#[allow(clippy::too_many_arguments)]
fn extrude_filled_path(
    path: &Path,
    fill_rule: FillRule,
    half_depth: f32,
    bevel: f32,
    extrude_depth: f32,
    tolerance: f32,
    stroke_opts: Option<&StrokeOptions>,
    vertices: &mut Vec<MeshVertex3D>,
    indices: &mut Vec<u32>,
) -> Result<(), VectorError> {
    let contours = flatten_contours(path, tolerance);
    let [min_x, min_y, max_x, max_y] = contour_bounds(&contours).ok_or_else(|| {
        VectorError::InvalidParameter("procedural shape produced empty contour bounds".to_string())
    })?;
    let center_offset = [0.5 * (min_x + max_x), 0.5 * (min_y + max_y)];
    let uv_rect = [min_x, min_y, (max_x - min_x).max(1e-6), (max_y - min_y).max(1e-6)];
    let char_center = [0.0, 0.0];

    let mut fill_tess = FillTessellator::new();
    let fill_opts = FillOptions::tolerance(tolerance).with_fill_rule(fill_rule);
    let geometry = tessellate_fill(&mut fill_tess, path, &fill_opts)?;

    append_caps(
        &geometry,
        0,
        char_center,
        center_offset,
        uv_rect,
        half_depth,
        vertices,
        indices,
    )?;

    if extrude_depth > 0.0 || bevel > 0.0 {
        append_side_walls(
            &contours,
            0,
            char_center,
            center_offset,
            uv_rect,
            half_depth,
            bevel,
            vertices,
            indices,
        )?;
    }

    if let Some(opts) = stroke_opts {
        let mut stroke_tess = StrokeTessellator::new();
        append_stroke_outline(
            &mut stroke_tess,
            path,
            opts,
            0,
            char_center,
            center_offset,
            uv_rect,
            half_depth,
            extrude_depth > 0.0,
            vertices,
            indices,
        )?;
    }

    Ok(())
}

fn tessellate_fill(
    tessellator: &mut FillTessellator,
    path: &Path,
    options: &FillOptions,
) -> Result<VertexBuffers<[f32; 2], u32>, VectorError> {
    let mut geometry: VertexBuffers<[f32; 2], u32> = VertexBuffers::new();
    tessellator
        .tessellate_path(
            path,
            options,
            &mut BuffersBuilder::new(&mut geometry, |vertex: FillVertex| {
                let p = vertex.position();
                [p.x, p.y]
            }),
        )
        .map_err(|err| {
            VectorError::InvalidParameter(format!("shape tessellation failed: {err:?}"))
        })?;
    Ok(geometry)
}

fn append_rounded_rect_subpath(
    builder: &mut PathBuilder,
    width: f32,
    height: f32,
    radius: f32,
    clockwise: bool,
) {
    let hx = 0.5 * width;
    let hy = 0.5 * height;
    let r = radius.max(0.0).min(0.49 * width.min(height));

    if r <= 1e-6 {
        if !clockwise {
            builder.begin(point(-hx, -hy));
            builder.line_to(point(hx, -hy));
            builder.line_to(point(hx, hy));
            builder.line_to(point(-hx, hy));
        } else {
            builder.begin(point(-hx, -hy));
            builder.line_to(point(-hx, hy));
            builder.line_to(point(hx, hy));
            builder.line_to(point(hx, -hy));
        }
        builder.close();
        return;
    }

    if !clockwise {
        builder.begin(point(-hx + r, -hy));
        builder.line_to(point(hx - r, -hy));
        builder.quadratic_bezier_to(point(hx, -hy), point(hx, -hy + r));
        builder.line_to(point(hx, hy - r));
        builder.quadratic_bezier_to(point(hx, hy), point(hx - r, hy));
        builder.line_to(point(-hx + r, hy));
        builder.quadratic_bezier_to(point(-hx, hy), point(-hx, hy - r));
        builder.line_to(point(-hx, -hy + r));
        builder.quadratic_bezier_to(point(-hx, -hy), point(-hx + r, -hy));
    } else {
        builder.begin(point(-hx + r, -hy));
        builder.quadratic_bezier_to(point(-hx, -hy), point(-hx, -hy + r));
        builder.line_to(point(-hx, hy - r));
        builder.quadratic_bezier_to(point(-hx, hy), point(-hx + r, hy));
        builder.line_to(point(hx - r, hy));
        builder.quadratic_bezier_to(point(hx, hy), point(hx, hy - r));
        builder.line_to(point(hx, -hy + r));
        builder.quadratic_bezier_to(point(hx, -hy), point(hx - r, -hy));
        builder.line_to(point(-hx + r, -hy));
    }
    builder.close();
}

fn normalize2(v: [f32; 2]) -> [f32; 2] {
    let len = (v[0] * v[0] + v[1] * v[1]).sqrt();
    if len <= 1e-12 {
        [1.0, 0.0]
    } else {
        [v[0] / len, v[1] / len]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_params(kind: ProceduralShapeKind) -> Shape3DParams {
        Shape3DParams {
            kind,
            size: 1.0,
            aspect: 1.0,
            stroke_width: 0.06,
            corner_radius: 0.08,
            inner_ratio: 0.382,
            extrude_depth: 0.24,
            bevel_size: 0.02,
        }
    }

    fn assert_valid_mesh(mesh: &ExtrudedTextMesh, expect_centered_xy: bool, expect_centered_z: bool) {
        assert_eq!(mesh.char_count, 1);
        assert!(!mesh.vertices.is_empty(), "mesh vertices must not be empty");
        assert!(!mesh.indices.is_empty(), "mesh indices must not be empty");
        assert_eq!(mesh.indices.len() % 3, 0);

        for v in &mesh.vertices {
            assert!(v.position.iter().all(|c| c.is_finite()), "non-finite pos");
            assert!(v.normal.iter().all(|c| c.is_finite()), "non-finite normal");
            let nlen =
                (v.normal[0] * v.normal[0] + v.normal[1] * v.normal[1] + v.normal[2] * v.normal[2])
                    .sqrt();
            assert!((nlen - 1.0).abs() < 1e-4, "normal not unit length: {nlen}");
            assert!((0.0..=1.0).contains(&v.uv[0]), "uv.x out of range: {}", v.uv[0]);
            assert!((0.0..=1.0).contains(&v.uv[1]), "uv.y out of range: {}", v.uv[1]);
            assert_eq!(v.char_index, 0);
            assert_eq!(v.char_center, [0.0, 0.0]);
            assert!(v.face_kind <= 3, "unexpected face_kind: {}", v.face_kind);
        }

        if expect_centered_xy {
            assert!(
                (mesh.bounds_min[0] + mesh.bounds_max[0]).abs() < 1e-3,
                "X not centered: {:?}..{:?}",
                mesh.bounds_min[0],
                mesh.bounds_max[0]
            );
            assert!(
                (mesh.bounds_min[1] + mesh.bounds_max[1]).abs() < 1e-3,
                "Y not centered: {:?}..{:?}",
                mesh.bounds_min[1],
                mesh.bounds_max[1]
            );
        }
        if expect_centered_z {
            assert!(
                (mesh.bounds_min[2] + mesh.bounds_max[2]).abs() < 1e-3,
                "Z not centered: {:?}..{:?}",
                mesh.bounds_min[2],
                mesh.bounds_max[2]
            );
        } else {
            assert!(
                mesh.bounds_min[2] >= -0.0011 && mesh.bounds_max[2] <= 0.0011,
                "unexpected flat Z bounds: {:?}..{:?}",
                mesh.bounds_min[2],
                mesh.bounds_max[2]
            );
        }

        for &tri in mesh.indices.as_chunks::<3>().0 {
            let v0 = mesh.vertices[tri[0] as usize];
            let v1 = mesh.vertices[tri[1] as usize];
            let v2 = mesh.vertices[tri[2] as usize];
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
            let face_normal = [
                e1[1] * e2[2] - e1[2] * e2[1],
                e1[2] * e2[0] - e1[0] * e2[2],
                e1[0] * e2[1] - e1[1] * e2[0],
            ];
            let dot = face_normal[0] * v0.normal[0]
                + face_normal[1] * v0.normal[1]
                + face_normal[2] * v0.normal[2];
            assert!(
                dot > 0.0,
                "triangle winding opposes vertex normal (face_kind={}, dot={dot})",
                v0.face_kind
            );
        }
    }

    fn face_kinds_present(mesh: &ExtrudedTextMesh) -> [bool; 4] {
        let mut seen = [false; 4];
        for v in &mesh.vertices {
            if (v.face_kind as usize) < 4 {
                seen[v.face_kind as usize] = true;
            }
        }
        seen
    }

    #[test]
    fn all_procedural_shapes_build_valid_centered_3d_meshes() {
        for kind in [
            ProceduralShapeKind::NeonTile,
            ProceduralShapeKind::HollowStar5,
            ProceduralShapeKind::Heart3D,
            ProceduralShapeKind::LightningBolt,
            ProceduralShapeKind::Diamond3D,
        ] {
            let mesh = build_shape3d_mesh(default_params(kind)).expect("shape build must succeed");
            assert_valid_mesh(&mesh, true, true);
            let kinds = face_kinds_present(&mesh);
            assert!(kinds[0] && kinds[1] && kinds[2], "{kind:?} must have front/side/back");
            match kind {
                ProceduralShapeKind::HollowStar5 => {
                    assert!(!kinds[3], "HollowStar5 uses front/back caps (0/2) and side walls (1)");
                }
                _ => {
                    assert!(kinds[3], "{kind:?} with stroke_width > 0 must emit face_kind 3");
                }
            }
        }
    }

    #[test]
    fn neon_tile_aspect_and_borderless_modes_work() {
        let mut wide = default_params(ProceduralShapeKind::NeonTile);
        wide.aspect = 1.8;
        let wide_mesh = build_shape3d_mesh(wide).expect("wide NeonTile");
        assert_valid_mesh(&wide_mesh, true, true);
        let width = wide_mesh.bounds_max[0] - wide_mesh.bounds_min[0];
        let height = wide_mesh.bounds_max[1] - wide_mesh.bounds_min[1];
        assert!((width - 1.8).abs() < 1e-3, "expected width 1.8, got {width}");
        assert!((height - 1.0).abs() < 1e-3, "expected height 1.0, got {height}");

        let mut solid = default_params(ProceduralShapeKind::NeonTile);
        solid.stroke_width = 0.0;
        let solid_mesh = build_shape3d_mesh(solid).expect("solid NeonTile");
        assert_valid_mesh(&solid_mesh, true, true);
        assert_eq!(face_kinds_present(&solid_mesh), [true, true, true, false]);
    }

    #[test]
    fn diamond3d_supports_classic_diamond_and_4point_star_and_flat_mode() {
        let mut classic = default_params(ProceduralShapeKind::Diamond3D);
        classic.inner_ratio = 1.0;
        classic.stroke_width = 0.0;
        let classic_mesh = build_shape3d_mesh(classic).expect("classic diamond");
        assert_valid_mesh(&classic_mesh, true, true);
        assert_eq!(face_kinds_present(&classic_mesh), [true, true, true, false]);

        let mut flat = default_params(ProceduralShapeKind::Heart3D);
        flat.extrude_depth = 0.0;
        flat.bevel_size = 0.0;
        let flat_mesh = build_shape3d_mesh(flat).expect("flat 2d heart with stroke");
        assert_valid_mesh(&flat_mesh, true, false);
        let kinds = face_kinds_present(&flat_mesh);
        assert!(kinds[0] && !kinds[1] && kinds[2] && kinds[3]);
    }

    #[test]
    fn invalid_shape_parameters_return_invalid_parameter_error() {
        let base = default_params(ProceduralShapeKind::Heart3D);

        for bad in [
            Shape3DParams { size: 0.0, ..base },
            Shape3DParams { size: f32::NAN, ..base },
            Shape3DParams { aspect: -1.0, ..base },
            Shape3DParams { stroke_width: -0.1, ..base },
            Shape3DParams { corner_radius: -0.05, ..base },
            Shape3DParams { inner_ratio: -0.2, ..base },
            Shape3DParams { extrude_depth: -0.5, ..base },
            Shape3DParams { bevel_size: f32::INFINITY, ..base },
        ] {
            let err = build_shape3d_mesh(bad).expect_err("invalid param must fail");
            assert!(matches!(err, VectorError::InvalidParameter(_)), "unexpected err: {err:?}");
        }
    }
}

