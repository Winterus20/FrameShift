---
title: "Faz 6.5 — Hibrit B: HarfRust + Skrifa Shaping, WGSL Slug GPU Bézier ve Sinematik 2D/3D Tipografi"
date: 2026-11-22
status: active
tags: [text-vector, shaders, schema, render-graph, engine]
---

# Faz 6.5 — Sektör Zirvesi Font ve Tipografi Mimarisi

## Soru / Amaç

FrameShift'in mevcut `ttf-parser = "0.25"` + CPU 4×4 SSAA scanline + düz 1-adımlı 3D chamfer tipografi hattını; **OpenType GPOS/GSUB shaping, Variable Font (`wght`/`wdth`/`slnt`) animasyonu, Latin+CJK fallback zinciri, Dikey Japonca (`VerticalRl` / Tategaki), GPU-native WGSL Slug Bézier render, çoklu anime konturu + HDR gradient/glow/shadow ve kavisli 3D bevel + Iridescent Chrome materyali** seviyesine çıkarmak.

---

## İncelenen Birincil Kaynaklar (2025–2026 Doğrulanmış)

- **HarfRust (`0.16.1`, MIT):** Saf Rust HarfBuzz portu; `read-fonts` tabanlı tam OpenType GPOS/GSUB shaping (`Direction::LeftToRight`, `Direction::TopToBottom`, `vert`/`vrt2`).
  - [https://github.com/harfbuzz/harfrust](https://github.com/harfbuzz/harfrust)
  - [https://docs.rs/harfrust/latest/harfrust/](https://docs.rs/harfrust/latest/harfrust/)
- **Google Fonts Fontations / Skrifa (`0.50.0`, Apache-2.0 OR MIT):** Saf Rust değişken font instancing (`InstanceBuilder::variations`), `ScalerFont`, `PathSink` outline çıkarımı, `COLRv1` ve metrik tabloları.
  - [https://github.com/googlefonts/fontations](https://github.com/googlefonts/fontations)
  - [https://crates.io/crates/skrifa](https://crates.io/crates/skrifa)
- **Linebender Parley (`0.13.0`) & Fontique (`0.13.0`, Apache-2.0 OR MIT):** Yatay rich-text layout ve deterministik font keşif referansları (`CollectionOptions { system_fonts: false }`).
  - [https://github.com/linebender/parley](https://github.com/linebender/parley)
- **Eric Lengyel — Slug GPU Font Rendering (17 Mart 2026 Public Domain Patent Feragati + MIT/Apache-2.0 Referans Shader'ları):**
  - Duyuru: [https://terathon.com/blog/decade-slug.html](https://terathon.com/blog/decade-slug.html)
  - JCGT 2017 Makalesi: [https://jcgt.org/published/0006/02/02/](https://jcgt.org/published/0006/02/02/)
  - Resmi Repo: [https://github.com/EricLengyel/Slug](https://github.com/EricLengyel/Slug)
  - Rust + `wgpu` + WGSL referansları: [https://github.com/folknor/sluggrs](https://github.com/folknor/sluggrs) ve [https://github.com/santiagosantos08/slug_wgpu](https://github.com/santiagosantos08/slug_wgpu)
- **Gelecek Tier 3+ Geometri / MSDF Opsiyonları:**
  - `cavalier_contours` (`0.9.0`): [https://github.com/jbuckmccready/cavalier_contours](https://github.com/jbuckmccready/cavalier_contours)
  - `i_overlay` (`9.2.0`): [https://github.com/iShape-Rust/iOverlay](https://github.com/iShape-Rust/iOverlay)
  - `klyff_msdf` (`0.4.0`): [https://codeberg.org/SnailBionicLab/klyff](https://codeberg.org/SnailBionicLab/klyff)
  - `fdsm` (`0.8.0`): [https://gitlab.com/Kyarei/fdsm](https://gitlab.com/Kyarei/fdsm)

---

## Uygulanan Hibrit B Mimarisi

1. **Saf Rust Shaping & Font Katmanı (`crates/frameshift-vector/src/shaper.rs`):**
   - `harfrust 0.16.1` + `skrifa 0.50.0` (`read-fonts 0.47.0`).
   - `shape_text_block`: satır bazlı font-run bölme + `fallback_fonts` zinciri (`charmap().map_unicode`), `VariationAxes` (`wght`, `wdth`, `slnt`), GPOS kerning + GSUB ligatürler, `WritingMode2D::HorizontalTb` (`Left/Center/Right`) ve `WritingMode2D::VerticalRl` (sağdan sola Tategaki sütun akışı).
   - `DualGlyphSink`: tek geçişte hem Slug GPU için `QuadraticCurve` (CFF/OTF kübik eğriler 4 segmentli kuadratik alt-bölümlemeyle) hem `lyon_tessellation` için `Path` üretir.
2. **GPU-Native WGSL Slug + Sinematik 2D FX (`slug.rs`, `text_slug.wgsl`, `text2d_fx.wgsl`):**
   - `build_slug_scene`: glif başına `1..=8` yatay/dikey bant, `1/1024` em-space epsilon, yatay/dikey düz eğri elemesi, `max_x`/`max_y` azalan sıralama ve vertex dinamik dilatasyon yön vektörleri (`normal_dir`).
   - `text_slug.wgsl`: `calc_root_code` (`0x2E74u`) + `solve_horiz_poly` / `solve_vert_poly` ile doğrudan fragment shader'da kuadratik Bézier kök çözümü, anizotropik piksel-ayak-izi AA (`fwidth`), `NonZero` fill ve Linear Rec.709 HDR gradient.
   - `text2d_fx.wgsl`: tek geçişte Soft Drop Shadow + HDR Outer Glow + 24×12+midpoint ikili anime konturu (`outline_rgba` + `outer_outline_rgba` + `outline_gap_norm` knockout boşluğu).
   - Geriye dönük uyumluluk: `font_family == None && font_path == None` olduğunda M1/M2 ASCII SDF yolu birebir korunur; eksik fontlarda `VectorError::FontNotFound` yayılır.
3. **3D Tipografi Yükseltmesi (`mesh3d.rs`, `text3d_mesh.wgsl`):**
   - `extrude_text_to_mesh_ex` + `Text3DMeshOptions`: çok satır, `TextAlign2D`, `tracking_em`, `line_height_em`, `fallback_fonts`, `VariationAxes`.
   - `BevelProfile3D::{Flat, Round, Concave, Stepped}` (`bevel_segments: 1..=16`).
   - `text3d_mesh.wgsl` içinde `material_mode == 3u` (**IridescentAnisotropic**): ince film Fresnel spektral renk kayması + anizotropik specular + HDR rim.

---

## Önerilen Açık Lisanslı (SIL OFL 1.1) Küratörlü Font Seti

- **Variable Kinetic / Modern Latin:** [Roboto Flex](https://fonts.google.com/specimen/Roboto+Flex), [Recursive](https://fonts.google.com/specimen/Recursive), [Unbounded](https://github.com/googlefonts/unbounded), [Bricolage Grotesque](https://fonts.google.com/specimen/Bricolage+Grotesque), [Syne](https://fonts.google.com/specimen/Syne), [Instrument Serif](https://fonts.google.com/specimen/Instrument+Serif), [Anton](https://fonts.google.com/specimen/Anton).
- **Japonca Kanji + Latin Uyumlu (Anime/AMV):** [M PLUS 1](https://fonts.google.com/specimen/M+PLUS+1), [Zen Kaku Gothic New](https://fonts.google.com/specimen/Zen+Kaku+Gothic+New), [Dela Gothic One](https://fonts.google.com/specimen/Dela+Gothic+One), [Shippori Mincho](https://fonts.google.com/specimen/Shippori+Mincho), [DotGothic16](https://fonts.google.com/specimen/DotGothic16), [Yuji Syuku](https://fonts.google.com/specimen/Yuji+Syuku).
