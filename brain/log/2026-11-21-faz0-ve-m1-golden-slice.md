# 2026-11-21 — Faz 0 Sözleşmeleri ve Faz 1 (M1) Golden Vertical Slice

**Tarih:** 2026-11-21  
**Faz:** Faz 0 (Hibrit Hızlı Yol) + Faz 1 (M1 Çekirdek Motor)  
**Durum:** Tamamlandı

---

## Amaç

`/grill-me` oturumunda onaylanan 6 başlangıç kararı ve [IMPLEMENTATION_PLAN.md](../../IMPLEMENTATION_PLAN.md) doğrultusunda Faz 0 sözleşmelerini kilitlemek ve Faz 1 (M1 Golden Vertical Slice) çekirdek motorunu swarm orkestrasyonuyla (`gemini-4-hydrogen-high` worker'lar, `gemini-4-argon-medium` coordinator) uçtan uca inşa edip doğrulamak.

---

## Yapılanlar

1. **Faz 0 Sözleşmeleri (`specs-author`):**
   - [timebase.md](../specs/timebase.md) (`status: locked`), [project-schema.md](../specs/project-schema.md) (`status: locked`), [render-graph.md](../specs/render-graph.md) (`status: review`) oluşturuldu.
   - [0002-rational-timebase-flicks.md](../decisions/0002-rational-timebase-flicks.md) `status: accepted` (`valid_from: 2026-11-21`) olarak güncellendi.
   - [specs/README.md](../specs/README.md) ve [index.md](../index.md) senkronize edildi.
2. **Cargo Workspace & Zaman/Şema Çekirdeği (`workspace-time-schema`):**
   - Kök `Cargo.toml`, `.gitignore` ve 6 crate'lik workspace oluşturuldu.
   - `frameshift-time`: `Flicks(i64)`, `Rational32` FPS, `i128` taşma-güvenli dönüşümler, `PtsIndex` VFR binary search, `SpeedCurve` (Gauss-Legendre integrali), `ImpactSolver` (ADR-0003).
   - `frameshift-schema`: Sürümlenmiş proje JSON (`0.1.0`), `ProjectSession` (optimistic locking + undo/redo), DAG validasyonu, `Project::golden_scene_preset()`.
3. **7 WGSL Shader ve GPU Compositor (`wgsl-shaders` + `render-engine`):**
   - `crates/frameshift-render/shaders/` altında `common_fullscreen.wgsl`, `transform.wgsl`, `mask_matte.wgsl`, `blend.wgsl`, `color_basic.wgsl`, `transition_whip_chromatic.wgsl`, `motion_blur_accumulate.wgsl` oluşturuldu.
   - `frameshift-render`: Headless `wgpu` `GpuCompositor`, `TexturePool` (`Rgba16Float`), DAG değerlendirici ve sub-frame temporal motion blur akümülatörü uygulandı.
4. **Medya I/O ve Sentetik Test Üreticisi (`media-engine`):**
   - `frameshift-media`: `Rgba8Frame`/`Rgba16fFrame`, `AudioBuffer`, `LruFrameCache`, `SyntheticMediaGenerator` (hareketli sahne A/B, hareketli maske, 64-bit tick barkodu, 120 BPM beat ses izi) ve `FfmpegBridge` (`ffmpeg`/`ffprobe` ham pipe I/O) uygulandı.
5. **Motor Orkestrasyonu ve CLI (`core-cli`):**
   - `frameshift-core`: `Engine` (`render_frame_at`, `render_video_mp4`, `verify_m1_golden_scene`) ve ses-impact senkron doğrulaması uygulandı.
   - `frameshift-cli`: `init-golden`, `render`, `frame`, `verify-m1` alt komutları eklendi.

---

## Doğrulama ve Kanıtlar

- `cargo test --workspace`: **30 passed, 0 failed** (`frameshift-time`: 8, `frameshift-schema`: 4, `frameshift-media`: 5, `frameshift-render`: 7, `frameshift-core`: 4, `frameshift-cli`: 2).
- `cargo clippy --workspace --all-targets -- -D warnings`: 0 uyarı, 0 hata.
- `cargo run --release -p frameshift-cli -- verify-m1 --out-dir target/m1_verify`: `all_gates_passed: true` (`target/m1_verify/m1_verification_report.json` ve `.md`):
  - **Determinizm (2x 300 kare tam render):** 300/300 kare birebir eşleşti (0 uyumsuzluk).
  - **Seek Independence (5 rastgele sıçramalı kare):** Maksimum kanal farkı = 0.
  - **Temporal Motion Blur (Final 16 alt-örnek vs Draft 1 örnek):** Ortalama L1 farkı = `4.7608`, zamanlama ankrajları özdeş.
  - **Ses-Impact Senkronu & ADR-0003:** Görsel flash ile ses transient tepe farkı `0` tick (`1764000000`), `ImpactSolver` hatası `0` tick; imkânsız kısıt 2 alternatif öneriyle reddedildi.
  - **Container Doğrulaması (`ffprobe`) & Görsel Kare Kontrolü:** `golden_final.mp4` (1920x1080, 300 kare, sesli) doğrulandı; 4 PNG kare (`frame_01_clip_a_ramp.png` .. `frame_04_impact_flash.png`) görsel olarak incelendi (boş olmayan, maskeli ve hareket blur'lu sentetik kompozit kareler doğrulandı; `brain/lessons.md` gereği hareket akıcılığı için yalnızca statik PNG'ye dayanılmadı).

---

## Açık Kalanlar / Sıradaki Adım

- **Faz 2 (M2):** `frameshift-vector` (`vello` vs `skia-safe` PoC), `inspection-contract.md` + contact sheet/crop/maske paketi üreticisi, OpenColorIO entegrasyonu, güvenli expression değerlendiricisi ve `local_media/` üzerinde gerçek anime klibi + müzik sınaması.
