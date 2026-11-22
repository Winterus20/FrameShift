# Oturum Günlüğü — 2026-11-21: Swarm ile 6 Prodüksiyon Darboğazının Araştırılması ve Çözümü

- **Tarih:** 2026-11-21
- **Kapsam:** [2026-11-21-gercek-asta-amv-denemesi.md](2026-11-21-gercek-asta-amv-denemesi.md) içinde tespit edilen 6 prodüksiyon darboğazının `/swarm` orkestrasyonu (`researcher` -> `media-decoder` + `engine-anim` + `ui-vector` -> `verifier`) ile araştırılması, uygulanması ve doğrulanması.

---

## Yapılan İşler

1. **Faz 1 — Derin Kod ve FFmpeg VFR/Seek Araştırması (`researcher`):**
   - `local_media/clip_1.mp4` (60fps, B-frame, `time_base = 1/15360`) üzerinde gerçek FFmpeg 9.0.2 seek davranışları sınandı.
   - Pre-input orta nokta `-ss` (`seek_ticks = pts[start_idx] - back_offset`) ve `-fps_mode passthrough` birlikte kullanıldığında pencere/batch decode çıktısının 0. kareden itibaren ardışık decode ile **bit-for-bit (`sha256` özdeş)** eşleştiği kanıtlandı.
   - 6 darboğaz için satır numaralı arayüz şartnamesi çıkarıldı.
2. **Faz 2 — Paralel Uygulama (`media-decoder`, `engine-anim`, `ui-vector`):**
   - **Pencere/Batch Video Decode (`frameshift-media` & `frameshift-core`):**
     - [ffmpeg.rs](../../crates/frameshift-media/src/ffmpeg.rs) içine `-fps_mode passthrough` ve `FfmpegBridge::decode_video_window` eklendi.
     - [resolver.rs](../../crates/frameshift-core/src/resolver.rs) içinde disk video önbellek ıskalamalarında (`cache miss`) 32 karelik pencere (`DEFAULT_DECODE_WINDOW = 32`) tek `ffmpeg` pipe sürecinde çözülüp `LruFrameCache`'e dolduruldu.
     - [engine.rs](../../crates/frameshift-core/src/engine.rs) içinde `DEFAULT_CACHE_CAPACITY` `64`'ten `128`'e (`1 GiB` üst sınır ile) çıkarıldı.
   - **Gerçek `AudioRmsEnvelope` Bağlamı (`frameshift-media`, `frameshift-render`, `frameshift-core`):**
     - [audio.rs](../../crates/frameshift-media/src/audio.rs) içine 10ms pencereli doğrusal RMS zarfı çıkaran `AudioBuffer::compute_rms_envelope` eklendi.
     - [compositor.rs](../../crates/frameshift-render/src/compositor.rs) içine `AudioRmsEnvelope`, `ProjectAnimator::with_audio_envelope` ve `GpuCompositor::render_project_frame_with_audio` eklendi. M1 sentetik testlerinin (`BeatAudio120Bpm`) deterministik hash'lerini korumak için gerçek RMS zarfı yalnızca disk üzerindeki ses varlıklarında (`ResolvedAssetSource::Disk`) etkinleştirildi.
   - **`EventBoundAnimation.hold_after` ve `MaskMatte.opaque_black_outside` (`frameshift-schema`, `frameshift-render`):**
     - [project.rs](../../crates/frameshift-schema/src/project.rs) içindeki `EventBoundAnimation` yapısına `#[serde(default)] pub hold_after: bool` eklendi; [compositor.rs](../../crates/frameshift-render/src/compositor.rs) içindeki `evaluate_event_animation` pencere sonrasında son keyframe değerini koruyacak şekilde güncellendi.
     - [graph.rs](../../crates/frameshift-schema/src/graph.rs) içindeki `NodeKind::MaskMatte` yapısına `#[serde(default)] opaque_black_outside: bool` ve [mask_matte.wgsl](../../crates/frameshift-render/shaders/mask_matte.wgsl) içine `params.mode_flags.z` kontrolü eklendi.
   - **Desktop UI Snapshot Yerleşimi (`frameshift-app`) ve Analitik SDF Tipografi (`frameshift-vector`):**
     - [snapshot.rs](../../crates/frameshift-app/src/snapshot.rs) içinde üst bardaki FPS/Timecode çakışması giderildi; timeline cetveli `36px` yüksekliğe ve 2 şeritli (`2-lane staggered`) çakışmasız event etiketlerine yükseltildi; `Node DAG Inspector` paneli `12+` düğümlü projelerde **2 sütunlu kompakt grid** (30 düğüme kadar) ve dinamik `SAFEEXPR PARAM BINDINGS` listesi gösterecek şekilde genişletildi.
     - [font.rs](../../crates/frameshift-vector/src/font.rs) ve [text.rs](../../crates/frameshift-vector/src/text.rs) içinde `scale >= 3.0` başlıklar için `4x` grid interpolasyonu yerine tam analitik `GlyphSdf::eval_exact` + diyagonal/eğrisel `Capsule` ve `RoundBox` (`smin` yumuşatmalı) vektör primitifleri devreye alındı.

---

## Doğrulama ve Benchmark Sonuçları (`verifier`)

- `cargo check --workspace` ve `cargo clippy --workspace --all-targets -- -D warnings`: **0 uyarı / 0 hata**.
- `cargo test --workspace`: **70/70 test geçti** (önceki 58 testten 12 yeni birim/entegrasyon testiyle 70'e çıktı).
- `cargo run --release -p frameshift-cli -- verify-all --out-dir target/full_verify`: **PASS (`all_gates_passed: true`)** — M1 `determinism` (`0` fark), `seek_independence` (`max_channel_diff = 0`), `motion_blur`, `audio_impact_sync` ve M5 `phase5_proxy_and_hw` kapılarının tamamı geçti.
- **Gerçek Asta AMV Render Hızlanması (`local_media/asta_amv_project.json`, 264 kare @ 1280x720 24fps):**
  - Önceki süre (tek-kare `ffmpeg` spawn): **`218.59 sn`**
  - Yeni süre (`decode_video_window` 32-kare pipe): **`63.86 sn`** (**`3.42x` hızlanma / `%70.8` süre düşüşü**).
- **Görsel İnceleme (`local_media/inspect_out/ui_snapshot.png` & `contact_sheet.png`):**
  - `ui_snapshot.png` üzerinde üst bar, timeline cetveli (`0S..11S`), 9 event etiketi (`EV_DROP_1..EV_KICK_9`), 27 düğümlü 2 sütunlu `RENDER GRAPH NODES (27)` ve 12 `SAFEEXPR PARAM BINDINGS` satırının sıfır çakışmayla çizildiği doğrulandı.
  - `contact_sheet.png` üzerinde `ASTA // ANTI-MAGIC` ve `DEVIL UNION` başlıklarının pürüzsüz analitik SDF kenarlarla üretildiği doğrulandı.
