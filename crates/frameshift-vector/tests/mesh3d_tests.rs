use std::path::Path;

use frameshift_vector::{
    extrude_text_to_mesh, extrude_text_to_mesh_with_bevel, extrude_text_to_mesh_with_outline,
    load_font_bytes, VectorError,
};

fn find_windows_test_font() -> Option<&'static Path> {
    let candidates = [
        Path::new(r"C:\Windows\Fonts\arialbd.ttf"),
        Path::new(r"C:\Windows\Fonts\arial.ttf"),
    ];
    candidates.into_iter().find(|p| p.exists())
}

#[test]
fn missing_font_returns_font_not_found() {
    let missing = Path::new(r"C:\nonexistent_frameshift_dir\definitely_missing_font.ttf");
    let err = load_font_bytes(missing).expect_err("missing font must fail");
    match err {
        VectorError::FontNotFound(msg) => {
            assert!(msg.contains("definitely_missing_font.ttf"), "unexpected msg: {msg}");
        }
        other => panic!("expected FontNotFound, got {other:?}"),
    }
}

#[test]
fn invalid_font_bytes_and_parameters_error() {
    let err = extrude_text_to_mesh(b"not a valid sfnt font", "HI", 32.0, 8.0)
        .expect_err("garbage bytes must return InvalidFont");
    assert!(matches!(err, VectorError::InvalidFont(_)));

    let err_size = extrude_text_to_mesh(b"", "HI", 0.0, 8.0)
        .expect_err("zero font_size must return InvalidParameter");
    assert!(matches!(err_size, VectorError::InvalidParameter(_)));

    let err_depth = extrude_text_to_mesh(b"", "HI", 32.0, -1.0)
        .expect_err("negative extrude_depth must return InvalidParameter");
    assert!(matches!(err_depth, VectorError::InvalidParameter(_)));
}

#[test]
fn extrude_real_windows_font_produces_valid_centered_3d_mesh() {
    let Some(font_path) = find_windows_test_font() else {
        return;
    };

    let bytes = load_font_bytes(font_path).expect("existing system font should load");
    let mesh = extrude_text_to_mesh(&bytes, "SHIFT", 64.0, 12.0)
        .expect("extrude_text_to_mesh should succeed on system TTF");

    assert_eq!(mesh.char_count, 5);
    assert!(!mesh.vertices.is_empty());
    assert!(!mesh.indices.is_empty());
    assert_eq!(mesh.indices.len() % 3, 0);

    let mut has_front = false;
    let mut has_side = false;
    let mut has_back = false;
    let mut min_v = f32::INFINITY;
    let mut max_v = f32::NEG_INFINITY;
    let mut seen_chars = [false; 5];
    let mut char_centers_x = [0.0_f32; 5];

    for v in &mesh.vertices {
        for &c in &v.position {
            assert!(c.is_finite(), "non-finite position");
        }
        for &c in &v.normal {
            assert!(c.is_finite(), "non-finite normal");
        }
        let nlen = (v.normal[0] * v.normal[0]
            + v.normal[1] * v.normal[1]
            + v.normal[2] * v.normal[2])
            .sqrt();
        assert!((nlen - 1.0).abs() < 1e-4, "normal must be unit length, got {nlen}");

        assert!((0.0..=1.0).contains(&v.uv[0]), "uv.x out of [0,1]: {}", v.uv[0]);
        assert!((0.0..=1.0).contains(&v.uv[1]), "uv.y out of [0,1]: {}", v.uv[1]);
        min_v = min_v.min(v.uv[1]);
        max_v = max_v.max(v.uv[1]);

        assert!(v.char_center[0].is_finite() && v.char_center[1].is_finite());
        assert!(v.char_index < 5);
        seen_chars[v.char_index as usize] = true;
        char_centers_x[v.char_index as usize] = v.char_center[0];

        match v.face_kind {
            0 => {
                has_front = true;
                assert_eq!(v.normal, [0.0, 0.0, 1.0]);
            }
            1 => {
                has_side = true;
                assert!(v.normal[2].abs() < 1e-5);
            }
            2 => {
                has_back = true;
                assert_eq!(v.normal, [0.0, 0.0, -1.0]);
            }
            other => panic!("unexpected face_kind {other}"),
        }
    }

    assert!(has_front && has_side && has_back, "must emit front, side, and back faces");
    assert!(seen_chars.iter().all(|&s| s), "all 5 characters should emit vertices");
    for pair in char_centers_x.windows(2) {
        assert!(pair[0] < pair[1], "char centers should increase left-to-right: {char_centers_x:?}");
    }
    assert!(min_v <= 1e-4 && max_v >= 1.0 - 1e-4, "uv.y should span [0, 1]");

    // Verify centered bounds
    assert!((mesh.bounds_min[0] + mesh.bounds_max[0]).abs() < 1e-3);
    assert!((mesh.bounds_min[1] + mesh.bounds_max[1]).abs() < 1e-3);
    assert!((mesh.bounds_min[2] + 6.0).abs() < 1e-4);
    assert!((mesh.bounds_max[2] - 6.0).abs() < 1e-4);

    // Verify consistent outward winding on every triangle
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

    // Empty and whitespace strings
    let empty_mesh = extrude_text_to_mesh(&bytes, "", 64.0, 12.0).unwrap();
    assert_eq!(empty_mesh.char_count, 0);
    assert!(empty_mesh.vertices.is_empty());

    let space_mesh = extrude_text_to_mesh(&bytes, "   ", 64.0, 12.0).unwrap();
    assert_eq!(space_mesh.char_count, 3);
    assert!(space_mesh.vertices.is_empty());
}

#[test]
fn extrude_text_with_bevel_generates_chamfer_strips_and_tilted_normals() {
    let Some(font_path) = find_windows_test_font() else {
        return;
    };

    let bytes = load_font_bytes(font_path).expect("existing system font should load");
    let flat_mesh = extrude_text_to_mesh(&bytes, "FX", 64.0, 12.0).expect("flat extrude");
    let bevel_mesh =
        extrude_text_to_mesh_with_bevel(&bytes, "FX", 64.0, 12.0, 2.0).expect("bevel extrude");

    assert!(
        bevel_mesh.vertices.len() > flat_mesh.vertices.len(),
        "bevel must add chamfer vertices (flat={}, bevel={})",
        flat_mesh.vertices.len(),
        bevel_mesh.vertices.len()
    );
    assert!(
        bevel_mesh.indices.len() > flat_mesh.indices.len(),
        "bevel must add chamfer indices (flat={}, bevel={})",
        flat_mesh.indices.len(),
        bevel_mesh.indices.len()
    );

    let mut has_front_bevel = false;
    let mut has_back_bevel = false;
    let mut has_flat_side = false;

    for v in &bevel_mesh.vertices {
        assert!(v.position.iter().all(|c| c.is_finite()));
        assert!(v.normal.iter().all(|c| c.is_finite()));
        let nlen = (v.normal[0] * v.normal[0]
            + v.normal[1] * v.normal[1]
            + v.normal[2] * v.normal[2])
            .sqrt();
        assert!((nlen - 1.0).abs() < 1e-4);

        if v.face_kind == 1 {
            let nz = v.normal[2];
            if nz > 0.1 && nz < 0.99 {
                has_front_bevel = true;
            } else if nz < -0.1 && nz > -0.99 {
                has_back_bevel = true;
            } else if nz.abs() < 1e-5 {
                has_flat_side = true;
            }
        }
    }

    assert!(
        has_front_bevel && has_back_bevel && has_flat_side,
        "expected front chamfer, middle flat wall, and back chamfer on side faces"
    );

    // Negative or non-finite bevel_size must error cleanly.
    let err = extrude_text_to_mesh_with_bevel(&bytes, "FX", 64.0, 12.0, -0.5)
        .expect_err("negative bevel must error");
    assert!(matches!(err, VectorError::InvalidParameter(_)));
}

#[test]
fn extrude_text_with_outline_emits_front_and_back_face_kind_3_on_real_font() {
    let Some(font_path) = find_windows_test_font() else {
        return;
    };

    let bytes = load_font_bytes(font_path).expect("existing system font should load");
    let no_outline =
        extrude_text_to_mesh_with_bevel(&bytes, "OK", 64.0, 12.0, 1.0).expect("no outline");
    let zero_outline =
        extrude_text_to_mesh_with_outline(&bytes, "OK", 64.0, 12.0, 1.0, 0.0).expect("zero outline");
    assert_eq!(no_outline.vertices.len(), zero_outline.vertices.len());
    assert_eq!(no_outline.indices.len(), zero_outline.indices.len());

    let outlined = extrude_text_to_mesh_with_outline(&bytes, "OK", 64.0, 12.0, 1.0, 2.5)
        .expect("outlined 3d text");
    assert!(outlined.vertices.len() > no_outline.vertices.len());

    let mut has_front_outline = false;
    let mut has_back_outline = false;
    for v in &outlined.vertices {
        assert!(v.position.iter().all(|c| c.is_finite()));
        assert!(v.normal.iter().all(|c| c.is_finite()));
        if v.face_kind == 3 {
            if v.normal == [0.0, 0.0, 1.0] {
                assert!((v.position[2] - 5.999).abs() < 1e-4);
                has_front_outline = true;
            } else if v.normal == [0.0, 0.0, -1.0] {
                assert!((v.position[2] + 5.999).abs() < 1e-4);
                has_back_outline = true;
            } else {
                panic!("unexpected outline normal {:?}", v.normal);
            }
        }
    }
    assert!(has_front_outline && has_back_outline);

    for &tri in outlined.indices.as_chunks::<3>().0 {
        let v0 = outlined.vertices[tri[0] as usize];
        let v1 = outlined.vertices[tri[1] as usize];
        let v2 = outlined.vertices[tri[2] as usize];
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
        assert!(dot > 0.0, "outlined triangle opposes normal (face_kind={}, dot={dot})", v0.face_kind);
    }

    // Flat 2D outlined text
    let flat_outlined = extrude_text_to_mesh_with_outline(&bytes, "OK", 64.0, 0.0, 0.0, 2.0)
        .expect("flat 2d outlined text");
    assert!(flat_outlined
        .vertices
        .iter()
        .any(|v| v.face_kind == 3 && v.normal == [0.0, 0.0, 1.0] && (v.position[2] + 0.001).abs() < 1e-5));
    assert!(!flat_outlined
        .vertices
        .iter()
        .any(|v| v.face_kind == 3 && v.normal == [0.0, 0.0, -1.0]));
}

