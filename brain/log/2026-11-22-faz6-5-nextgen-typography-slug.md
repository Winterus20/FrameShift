---
title: "Faz 6.5 — Hibrit B: HarfRust + Skrifa Shaping, WGSL Slug GPU Bézier, Kavisli 3D Bevel ve SIL OFL Font Paketi"
date: 2026-11-22
status: completed
tags: [text-vector, shaders, schema, render-graph, engine]
---

# 2026-11-22 — Faz 6.5: Next-Gen Tipografi & WGSL Slug

## Ne Yapıldı?

1. **Saf Rust Shaping, Variable Font, Fallback & Dikey Japonca (`crates/frameshift-vector/src/shaper.rs`):**
   - `harfrust = "0.16"` (`0.16.1`) + `skrifa = "0.50"` (`0.50.0`) eklendi; `ttf-parser` M1 uyumluluğu için korundu.
   - `shape_text_block`: OpenType **GPOS kerning + GSUB ligatür**, `VariationAxes` (`wght`, `wdth`, `slnt`), `fallback_fonts` zinciri (`charmap().map_unicode`), `WritingMode2D::HorizontalTb` (`Left/Center/Right`) ve `WritingMode2D::VerticalRl` (**Tategaki** sağdan sola sütun akışı) eklendi.
   - `DualGlyphSink`: hem Slug GPU için `QuadraticCurve` (CFF/OTF kübik → 4 kuadratik) hem `lyon_tessellation::path::Path` üretir.
2. **WGSL Slug GPU Bézier + Sinematik 2D FX (`slug.rs`, `text_slug.wgsl`, `text2d_fx.wgsl`):**
   - `build_slug_scene`: glif başına `1..=8` yatay/dikey bant, `1/1024` epsilon, yatay/dikey düz eğri elemesi, `max_x`/`max_y` azalan sıralama.
   - `text_slug.wgsl` + `text2d_fx.wgsl`: Dynamic Glyph Dilation, kuadratik Bézier kök çözümü (`calc_root_code`, `solve_horiz_poly`, `solve_vert_poly`), Linear Rec.709 HDR gradient, Soft Drop Shadow, HDR Outer Glow ve 24×12+midpoint ikili anime konturu + `outline_gap_norm` knockout boşluğu.
   - M1/M2 ASCII SDF yolu (`font_family == None && font_path == None`) korundu; eksik fontlarda `VectorError::FontNotFound` yayılır.
3. **3D Tipografi Yükseltmesi (`mesh3d.rs`, `text3d_mesh.wgsl`, `compositor.rs`):**
   - `extrude_text_to_mesh_ex` + `Text3DMeshOptions`: çok satır, hizalama, `tracking_em`, `line_height_em`, `fallback_fonts`, variable axes, `BevelProfile3D::{Flat, Round, Concave, Stepped}` (`bevel_segments: 1..=16`).
   - `text3d_mesh.wgsl`: `material_mode == 3u` (**IridescentAnisotropic**).
4. **Şema & Küratörlü SIL OFL 1.1 Font Paketi (`graph.rs`, `project.rs`, `assets/fonts/`):**
   - `WritingMode2D`, `BevelProfile3D` ve yeni `VectorText`/`Text3D` `#[serde(default)]` alanları + `SafeExpr` `bindable_param_names` eklendi.
   - 13 resmi **SIL OFL 1.1** font ailesi (`RobotoFlex-VF.ttf`, `Recursive-VF.ttf`, `Unbounded-VF.ttf`, `BricolageGrotesque-VF.ttf`, `Syne-VF.ttf`, `InstrumentSerif-Regular.ttf`, `Anton-Regular.ttf`, `MPLUS1-VF.ttf`, `ZenKakuGothicNew-Bold.ttf`, `DelaGothicOne-Regular.ttf`, `ShipporiMincho-Bold.ttf`, `DotGothic16-Regular.ttf`, `YujiSyuku-Regular.ttf`) `assets/fonts/` altına ve `assets/fonts/LICENSES_OFL.md` kaydına eklendi.

## Doğrulamalar

- `cargo clippy --workspace --all-targets -- -D warnings`: **0 uyarı**.
- `cargo test --workspace`: **187/187 geçti** (`0` başarısız, `0` yok sayılan).
- `cargo run --release -p frameshift-cli -- verify-all --out-dir target/verify_65`: **PASS** (M1–M5 tüm kapılar geçti).
- Görsel Showcase: `target/verify_65/typography_showcase_65.png` (1280×720, 4 panel) `read_file` ile gözle doğrulandı.
