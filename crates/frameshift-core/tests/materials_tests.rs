//! Regression tests for the project-independent material/shape swatch gallery
//! (`materials_sheet.png` + `materials_manifest.json`).

use frameshift_core::Engine;
use frameshift_core::inspection::{
    MATERIAL_GALLERY_MANIFEST_FILE, MATERIAL_GALLERY_SHEET_FILE, MaterialGalleryManifest,
    export_material_gallery_impl,
};
use frameshift_media::linear_to_srgb;
use frameshift_schema::Shape3DKind;
use frameshift_time::Flicks;
use frameshift_vector::{ProceduralShapeKind, Shape3DParams, build_shape3d_mesh};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

struct TempDirGuard(PathBuf);

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn unique_temp_dir(label: &str) -> TempDirGuard {
    let dir = std::env::temp_dir().join(format!(
        "frameshift_material_gallery_{label}_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp gallery dir");
    TempDirGuard(dir)
}

fn generate(label: &str) -> (TempDirGuard, MaterialGalleryManifest) {
    let dir = unique_temp_dir(label);
    let mut engine = Engine::new_headless().expect("headless engine");
    let manifest = export_material_gallery_impl(&mut engine, &dir.0).expect("material gallery");
    (dir, manifest)
}

fn shape_name(kind: Shape3DKind) -> &'static str {
    match kind {
        Shape3DKind::NeonTile => "neon_tile",
        Shape3DKind::HollowStar5 => "hollow_star_5",
        Shape3DKind::Heart3D => "heart_3d",
        Shape3DKind::LightningBolt => "lightning_bolt",
        Shape3DKind::Diamond3D => "diamond_3d",
    }
}

fn channel_diff_8bit(a: [f32; 3], b: [f32; 3]) -> f32 {
    let to_u8 = |c: f32| linear_to_srgb(c.clamp(0.0, 1.0)) * 255.0;
    let mut max_diff: f32 = 0.0;
    for i in 0..3 {
        max_diff = max_diff.max((to_u8(a[i]) - to_u8(b[i])).abs());
    }
    max_diff
}

fn cell_saturation_8bit(mean: [f32; 3]) -> f32 {
    let channels = mean.map(|c| linear_to_srgb(c.clamp(0.0, 1.0)) * 255.0);
    let max = channels.iter().copied().fold(f32::MIN, f32::max);
    let min = channels.iter().copied().fold(f32::MAX, f32::min);
    max - min
}

#[test]
fn materials_gallery_writes_20_cells_and_manifest_fields() {
    let (dir, manifest) = generate("fields");

    assert_eq!(manifest.cells.len(), 20, "expected 5 shapes x 4 materials");
    assert_eq!(manifest.grid.cols, 4);
    assert_eq!(manifest.grid.rows, 5);
    assert_eq!(manifest.grid.cell_width, 320);
    assert_eq!(manifest.grid.cell_height, 180);
    assert_eq!(manifest.sample_tick, Flicks::ZERO);
    assert_eq!(manifest.color_space, "linear_rec709");
    assert_eq!(manifest.tonemap, "aces_lite");

    let expected_w = manifest.grid.cols * manifest.grid.cell_width
        + (manifest.grid.cols + 1) * manifest.grid.pad;
    let expected_h = manifest.grid.header_height
        + manifest.grid.rows * manifest.grid.cell_height
        + (manifest.grid.rows + 1) * manifest.grid.pad;
    assert_eq!(manifest.sheet.width, expected_w);
    assert_eq!(manifest.sheet.height, expected_h);

    let mut pairs = HashSet::new();
    let mut rects = HashSet::new();
    for (index, cell) in manifest.cells.iter().enumerate() {
        assert_eq!(cell.cell_index, index as u32);
        assert_eq!(cell.row, cell.cell_index / manifest.grid.cols);
        assert_eq!(cell.col, cell.cell_index % manifest.grid.cols);
        assert_eq!(cell.cell_rect.width, manifest.grid.cell_width);
        assert_eq!(cell.cell_rect.height, manifest.grid.cell_height);
        assert!(cell.cell_rect.x >= manifest.grid.pad);
        assert!(cell.cell_rect.y >= manifest.grid.header_height);
        assert!(cell.cell_rect.x + cell.cell_rect.width <= manifest.sheet.width);
        assert!(cell.cell_rect.y + cell.cell_rect.height <= manifest.sheet.height);
        assert!(rects.insert((
            cell.cell_rect.x,
            cell.cell_rect.y,
            cell.cell_rect.width,
            cell.cell_rect.height
        )));
        assert!(!cell.material_name.is_empty());
        assert_ne!(cell.pixel_hash64, 0);
        assert_eq!(cell.sha256_hex.len(), 64);
        pairs.insert((shape_name(cell.shape_kind), cell.material_mode));
    }
    assert_eq!(pairs.len(), 20, "every shape/material pair appears once");

    for kind in [
        Shape3DKind::NeonTile,
        Shape3DKind::HollowStar5,
        Shape3DKind::Heart3D,
        Shape3DKind::LightningBolt,
        Shape3DKind::Diamond3D,
    ] {
        let modes: Vec<u32> = manifest
            .cells
            .iter()
            .filter(|c| c.shape_kind == kind)
            .map(|c| c.material_mode)
            .collect();
        assert_eq!(modes.len(), 4);
        assert_eq!(modes, vec![0, 1, 2, 3]);
    }

    let sheet_path = dir.0.join(MATERIAL_GALLERY_SHEET_FILE);
    let manifest_path = dir.0.join(MATERIAL_GALLERY_MANIFEST_FILE);
    let sheet_bytes = std::fs::read(&sheet_path).expect("materials_sheet.png exists");
    let manifest_bytes = std::fs::read(&manifest_path).expect("materials_manifest.json exists");
    assert!(sheet_bytes.len() > 1000);
    assert!(manifest_bytes.len() > 1000);
    let written: MaterialGalleryManifest =
        serde_json::from_slice(&manifest_bytes).expect("manifest round-trips");
    assert_eq!(written, manifest);
    assert_eq!(manifest.sheet.file, MATERIAL_GALLERY_SHEET_FILE);
}

#[test]
fn materials_gallery_cells_have_visible_shape() {
    let (_dir, manifest) = generate("visible");

    let mut min_ratio = f32::MAX;
    for cell in &manifest.cells {
        assert!(
            cell.non_background_ratio > 0.08,
            "{} m={} covers only {:.3} of the cell",
            shape_name(cell.shape_kind),
            cell.material_mode,
            cell.non_background_ratio
        );
        assert!(cell.mean_rgb.iter().all(|c| c.is_finite()));
        min_ratio = min_ratio.min(cell.non_background_ratio);
    }
    println!("min non-background ratio: {min_ratio:.3}");
}

#[test]
fn materials_gallery_fit_is_normalized() {
    let (_dir, manifest) = generate("fit");

    for cell in &manifest.cells {
        assert!(
            (0.60..=0.85).contains(&cell.height_fraction),
            "{} m={} height_fraction {:.3} outside [0.60, 0.85]",
            shape_name(cell.shape_kind),
            cell.material_mode,
            cell.height_fraction
        );
    }
}

#[test]
fn materials_gallery_materials_differ_from_white() {
    let (_dir, manifest) = generate("materials");

    let mut per_shape: HashMap<&'static str, Vec<[f32; 3]>> = HashMap::new();
    for cell in &manifest.cells {
        assert!(
            cell_saturation_8bit(cell.mean_rgb) > 24.0,
            "{} m={} mean_rgb {:?} is too desaturated",
            shape_name(cell.shape_kind),
            cell.material_mode,
            cell.mean_rgb
        );
        per_shape
            .entry(shape_name(cell.shape_kind))
            .or_default()
            .push(cell.mean_rgb);
    }

    for (kind, means) in per_shape {
        let mut max_diff: f32 = 0.0;
        for i in 0..means.len() {
            for j in (i + 1)..means.len() {
                max_diff = max_diff.max(channel_diff_8bit(means[i], means[j]));
            }
        }
        assert!(
            max_diff > 24.0,
            "{kind} material means differ by only {max_diff:.2} channel levels"
        );
        println!("{kind} max material channel diff: {max_diff:.2}");
    }
}

#[test]
fn materials_gallery_is_deterministic() {
    let (dir_a, manifest_a) = generate("determinism_a");
    let (dir_b, manifest_b) = generate("determinism_b");

    let png_a = std::fs::read(dir_a.0.join(MATERIAL_GALLERY_SHEET_FILE)).expect("png a");
    let png_b = std::fs::read(dir_b.0.join(MATERIAL_GALLERY_SHEET_FILE)).expect("png b");
    assert_eq!(png_a, png_b, "materials_sheet.png bytes must be identical");

    let json_a =
        std::fs::read(dir_a.0.join(MATERIAL_GALLERY_MANIFEST_FILE)).expect("manifest a");
    let json_b =
        std::fs::read(dir_b.0.join(MATERIAL_GALLERY_MANIFEST_FILE)).expect("manifest b");
    assert_eq!(json_a, json_b, "materials_manifest.json bytes must be identical");

    assert_eq!(manifest_a.sheet.fnv1a_hash64, manifest_b.sheet.fnv1a_hash64);
    assert_eq!(manifest_a.sheet.sha256_hex, manifest_b.sheet.sha256_hex);
    for (a, b) in manifest_a.cells.iter().zip(manifest_b.cells.iter()) {
        assert_eq!(a.pixel_hash64, b.pixel_hash64);
        assert_eq!(a.sha256_hex, b.sha256_hex);
    }
}

#[test]
fn materials_diamond_3d_default_note_is_star() {
    let (_dir, manifest) = generate("diamond_note");

    let diamond_cells: Vec<_> = manifest
        .cells
        .iter()
        .filter(|c| c.shape_kind == Shape3DKind::Diamond3D)
        .collect();
    assert_eq!(diamond_cells.len(), 4);
    for cell in diamond_cells {
        assert_eq!(cell.inner_ratio, 0.382);
        let note = cell.note.as_deref().unwrap_or_default();
        assert!(
            note.contains("8-point star at default inner_ratio"),
            "diamond note missing star disclosure: {note}"
        );
    }
    assert!(
        manifest
            .notes
            .iter()
            .any(|n| n.contains("8-point star at default inner_ratio")),
        "top-level notes must disclose the diamond star default"
    );

    // Prove the disclosure: default inner_ratio builds an 8-point star, 1.0 a 4-point rhombus.
    let params = |inner_ratio: f32| Shape3DParams {
        kind: ProceduralShapeKind::Diamond3D,
        size: 1.0,
        aspect: 1.0,
        stroke_width: 0.06,
        corner_radius: 0.0,
        inner_ratio,
        extrude_depth: 0.12,
        bevel_size: 0.01,
    };
    let star = build_shape3d_mesh(params(0.382)).expect("star mesh");
    let rhombus = build_shape3d_mesh(params(1.0)).expect("rhombus mesh");
    assert_ne!(star.vertices.len(), rhombus.vertices.len());
    assert!(star.vertices.len() > rhombus.vertices.len());
}
