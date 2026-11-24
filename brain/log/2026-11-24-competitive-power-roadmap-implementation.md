---
title: Rakip Güç Yol Haritası — P1, P2, P3 & P5 İmplementasyonu
date: 2026-11-24
tags: [engine, agent, shaders, render-graph, timebase, media-io, schema]
---

# Rakip Güç Yol Haritası — P1, P2, P3 & P5 İmplementasyonu

## Kapsam ve Kullanıcı Kararları

[Rakip Güç Analizi ve Yol Haritası](../research/2026-11-24-competitive-power-analysis-roadmap.md) 5 paralel `Gemini 4 Hydrogen (High)` araştırmacısı tarafından incelenip 5 ayrı `.md` araştırma raporuna kaydedildi:
- [P1 — Yaratıcı Motor & `SafeExpr`](../research/2026-11-24-power-p1-creative-engine.md)
- [P2 — Ajan Linter, `diff_frame`, `Motion.md` DSL & Parametrik Şablonlar](../research/2026-11-24-power-p2-agent-dsl-linter.md)
- [P3 — GPU NV12 → Linear Rec.709 & Düğüm Seviyesi Smart Cache](../research/2026-11-24-power-p3-gpu-yuv-smart-cache.md)
- [P4 — İnce Canlı Masaüstü Önizleme Kabuğu (`egui 0.37` + `winit 0.30`)](../research/2026-11-24-power-p4-live-desktop-shell.md) *(kullanıcı kararıyla ileriki faza bırakıldı)*
- [P5 — 2D/Planar Tracker, `.otio`/FCPXML & `model_manifest.json` AI Yuvası](../research/2026-11-24-power-p5-tracker-otio-ai-manifest.md)

Kullanıcı onayıyla **0 yeni Cargo crate** kısıtı korunarak P1, P2, P3 ve P5 sütunları 5 dalgalı swarm ile (`Wave 1: time/schema/vector + media/sidecar + 4 WGSL` → `Wave 2: render` → `Wave 3: core/app` → `Wave 4: cli/mcp` → `Wave 5: verifier`) implemente edildi.

## Uygulanan Bileşenler

1. **P1 — Yaratıcı Motor & İfade Gücü:**
   - `crates/frameshift-time/src/expr.rs`: `%`, `< <= > >= == !=`, `&&`, `||`, `!`, kısa devreli `if`/`select`, `ceil`, `sqrt` (clamped), `log` (clamped), `mix`, `pow` NaN koruması, analitik sönümlü `spring(t, stiffness, damping)` ve SplitMix64 1D gradient noise `wiggle(freq, amp, seed)` (örtük `t_sec`, seek-independent).
   - `crates/frameshift-schema/src/{graph.rs, project.rs, session.rs}` & `crates/frameshift-vector/src/shape_mask.rs`: `NodeKind::{DisplacementMap, FrameEcho, ShapeMask}` (0-girdili üretici), arc-length morph'lu `ShapeMask` rasterizer, `EventTimingConstraint` + Kahn DAG topolojik sıralama/döngü tespiti (`i128->i64` Flicks) ve `Project.keyframe_tracks` (`KeyframeTrackDef`, ADR-0004 `0.2.0` uyumlu).
   - `crates/frameshift-render/shaders/{fx_displacement_map.wgsl, fx_frame_echo.wgsl, fx_shape_mask_feather.wgsl}` ve GPU kompozitör pipeline entegrasyonu.
2. **P2 — Ajan Verimliliği, Linter, DSL & Parametrik Şablonlar:**
   - `crates/frameshift-core/src/linter.rs` & `crates/frameshift-media/src/blackdetect.rs`: `lessons.md` 4 sessiz kurgu tuzağı (`LNT-MED-001` `blackdetect` opt-in, `LNT-TXT-001`, `LNT-CLR-001`, `LNT-CMP-001`) + 6 yapısal/güvenlik/performans kuralı + MAE/PSNR/bbox & 3-panelli fark PNG üreten `diff_frame`.
   - `crates/frameshift-schema/src/dsl.rs` & `crates/frameshift-core/src/dsl.rs`: `Motion.md` deklaratif sahne reçetesi parser + deterministik DAG JSON derleyicisi (derleme sonrası `lint_project` kapılı).
   - `crates/frameshift-schema/src/template_parametric.rs` & `crates/frameshift-core/src/template_parametric.rs`: BPM/downbeat uyarlamalı `ParametricTemplateSpec` (`calculate_template_metadata`, `instantiate_parametric_template`).
3. **P3 — GPU NV12 Decode & Düğüm Seviyesi Smart Cache:**
   - `crates/frameshift-media/src/{frame.rs, cache.rs, ffmpeg.rs, streaming.rs}` & `crates/frameshift-render/shaders/yuv_to_linear.wgsl`: `ffprobe` renk metadatası + 12-bpp `nv12` pipe + `R8Unorm` Y / `Rg8Unorm` UV GPU dönüşümü (`SourceFit::Contain` `content_uv_rect`, alfa/tek-boyut ve M1 sentetik `Rgba8` fallback korumalı), `LruFrameCache` `256 MiB` bayt tavanı.
   - `crates/frameshift-render/src/smart_cache.rs`: `revision_id`'den bağımsız 128-bit `NodeContentHash` (saf Rust SHA-256), `MemoryOnly` varsayılan (`192 MiB` GPU VRAM + `128 MiB` CPU RAM ağır düğüm seçicili) + opt-in Disk LRU (`--cache-dir`).
4. **P5 — 2D/Planar Tracker, `.otio`/FCPXML & AI Model Manifest:**
   - `crates/frameshift-render/src/tracker.rs` & `crates/frameshift-core/src/tracker_bridge.rs`: 4-seviyeli `fx_optical_flow_step.wgsl` piramidinden Forward-Backward + `32x32` ROI readback 2D Point / Planar (`glam::Mat3` homografi) tracker ve `Project.keyframe_tracks` bağlayıcısı.
   - `crates/frameshift-schema/src/nle_bridge.rs`: 0-crate OpenTimelineIO (`.otio`) ve FCPXML 1.10/1.11 export/import (`Flicks` ↔ `RationalTime` / `"N/Ds"`).
   - `crates/frameshift-media/src/ai_manifest.rs` & `scripts/ai_infer_sidecar.py`: `local_media/models/<id>/model_manifest.json` (`frameshift.ai_model.v1`), SHA-256 `Strict` (+ `--allow-unverified`), `--manifest` dinamik ONNX sidecar yürütücüsü.
5. **CLI & MCP Yüzeyleri:**
   - `crates/frameshift-cli/src/{p2_p5.rs, main.rs}`: `lint`, `diff-frame`, `compile-dsl`, `template-metadata`, `template-instantiate`, `track-point`, `track-planar`, `export-nle`, `import-nle`, `ai-models`, `ai-run`, `render`/`frame --cache-dir`.
   - `crates/frameshift-mcp/src/server.rs`: 11 yeni MCP aracı + `NodeKind::ALL_SERDE_NAMES` ve `project_commands` senkronu.

## Bağımsız Doğrulama (`verifier`)

- **Bağımlılık kontrolü (`git diff --stat Cargo.toml Cargo.lock crates/*/Cargo.toml`):** Boş (**0 yeni Cargo crate**).
- **WGSL envanteri:** **60** `.wgsl` dosyası (`crates/frameshift-render/shaders/`).
- **Clippy (`cargo clippy --workspace --all-targets -- -D warnings`):** **0 uyarı**, exit 0.
- **Testler (`cargo test --workspace --no-fail-fast`):** **452/453 PASS** (`frameshift-time` 20/20, `frameshift-schema` 82/82, `frameshift-vector` 38/38, `frameshift-media` 78/78, `frameshift-render` 114/114, `frameshift-app` 15/15, `frameshift-mcp` 10/10, `frameshift-cli` 28/28, `frameshift-core` 67/68 — tek başarısızlık önceden mevcut `HEAD` `materials_gallery_fit_is_normalized` at `crates/frameshift-core/tests/materials_tests.rs:166:9`).
- **Canlı CLI Smoke (`target/power_verify/`):**
  - `compile-dsl` (`motion.md` → `project.json`, `lint_report.ok=true`) + `lint --json` (`error_count=0, warning_count=0`): PASS.
  - `export-nle` + `import-nle` roundtrip (`.otio` & `.fcpxml`, `clips=1`, `duration_ticks=705600000`): PASS.
  - `frame` (2 kare) + `diff-frame` (`diff.png`, `changed_pixels=1001/3520`, `mae=13.047`): PASS.
  - `ai-models --json`: PASS.
