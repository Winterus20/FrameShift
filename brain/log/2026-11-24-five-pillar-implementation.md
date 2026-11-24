# 2026-11-24 — Beş Temel Yetenek Yükseltmesi (Spectral Flux, AI Stem/Lyrics, Waveform PNG, Anime4K/Real-ESRGAN & Depth-Anything-V2 Scene3D)

**Tarih:** 2026-11-24  
**Faz:** Faz 2 — 5-Pillar Medya, Ses, GPU Shader & AI Entegrasyonu (0 Yeni Cargo Crate)  
**Durum:** Tamamlandı ve Doğrulandı

---

## Amaç

FrameShift motorunun beş temel eksikliğini **0 yeni Cargo crate** (`Cargo.toml` ve `Cargo.lock` değiştirilmeden) ile iki dalga halinde üretime almak:
1. Çok-bantlı Spectral Flux + Ellis tempo + 4/4 Downbeat tespiti ve `SafeExpr` ses bant uniformları (`audio_sub`, `audio_mid`, `audio_high`, `downbeat_phase`).
2. İzole Python 3.12 CUDA yan ortamı (`local_media/.venv-audio`) üzerinden AI stem ayrımı (`audio-separator`) ve kelime düzeyinde söz zamanlama (`stable-ts`/`faster-whisper` + `fugashi`).
3. Gerçek PCM verisinden çok çözünürlüklü PNG waveform rasterizer ve `frameshift-app` `snapshot.rs` timeline entegrasyonu.
4. Anime4K v4.x `FastS` WGSL shader zinciri (8 shader, toplam **52 WGSL**) ve `realesrgan-ncnn-vulkan v0.2.5.0` CLI sürücüsü.
5. `Depth-Anything-V2-Small` aspect-preserving 16-bit/RG16 derinlik boru hattı, temporal EMA normalizasyonu ve `Scene3D` disparity/DoF alpha-void düzeltmeleri.

---

## Yapılanlar

### 1. Çok-Bantlı Spectral Flux, Downbeat & `SafeExpr` Ses Uniformları (`frameshift-media`, `frameshift-time`, `frameshift-render`, `frameshift-core`)
- [`spectral.rs`](../../crates/frameshift-media/src/spectral.rs): Saf Rust iteratif Radix-2 Cooley-Tukey FFT (`Radix2Fft`, `N=4096`), `make_hann_window` STFT, 3 bant (`sub 20–120 Hz`, `mid 120–2000 Hz`, `high 2000–16000 Hz`) SuperFlux max-filter + half-wave rectified spectral flux, adaptif onset picking, Ellis otokorelasyon + DP beat/4-4 `downbeats` tespiti (`MultibandBeatReport`, `AudioBandEnvelope`).
- [`audio.rs`](../../crates/frameshift-media/src/audio.rs): Mevcut `AudioAnalysisReport` ve `analyze_beats_and_energy` imzaları korunarak `AudioBuffer::analyze_multiband_beats` ve `AudioBuffer::analyze_band_envelope` eklendi.
- [`expr.rs`](../../crates/frameshift-time/src/expr.rs), [`compositor.rs`](../../crates/frameshift-render/src/compositor.rs), [`engine.rs`](../../crates/frameshift-core/src/engine.rs): `ExprContext` içine `audio_sub`, `audio_mid`, `audio_high`, `downbeat_phase` eklendi; disk sesi varken `resolve_disk_audio_envelope` üzerinden `AudioBandEnvelope` ve gerçek BPM `beat_phase`/`downbeat_phase` bağlandı (sentetik `BeatAudio120Bpm` için M1 golden hash uyumluluğu `Ok(None)` ile korundu).

### 2. İzole AI Stem Ayrımı & Söz Zamanlama (`local_media/.venv-audio`, `scripts/`, `frameshift-media`)
- `local_media/.venv-audio`: İzole Python `3.12.10` ortamı (`torch 2.6.0+cu124`, `audio-separator 0.48.0`, `faster-whisper 1.2.1`, `stable-whisper 2.19.1`, `fugashi 1.5.2`, `unidic-lite 1.0.8`, `onnxruntime-gpu 1.32.0`). Sistem Python `3.14.6` (`onnxruntime-directml 1.24.4`) bozulmadan korundu.
- [`scripts/ai_audio_sidecar.py`](../../scripts/ai_audio_sidecar.py) & [`ai_audio.rs`](../../crates/frameshift-media/src/ai_audio.rs): `--probe`, `separate`, `align` alt komutları; `StemSeparationRequest`, `LyricAlignRequest`, `seconds_to_flicks`, `build_lyric_timeline_nodes` ve `detect_ai_audio_capabilities`.

### 3. Gerçek Waveform PNG & `snapshot.rs` (`frameshift-media`, `frameshift-app`)
- [`waveform.rs`](../../crates/frameshift-media/src/waveform.rs): `WaveformPyramid` (`L0=256`), `WaveformStyle::{DualEnvelope, SpectralRgb}`, `WaveformAmplitudeScale::{Linear, Decibel}`, `WaveformChannelLayout::{MonoMix, StereoSplit}`, aşırı zoom-in polyline rasterizer, işaretçiler (`beat_ticks`, `downbeat_ticks`, `cue_ticks`, `playhead_tick`) ve `render_waveform_png_bytes`.
- [`snapshot.rs`](../../crates/frameshift-app/src/snapshot.rs), [`controller.rs`](../../crates/frameshift-app/src/controller.rs): `SnapshotAudioContext`, `SnapshotAudioClipWaveform` ve `render_desktop_ui_snapshot_with_audio`; ayrıca Node DAG Inspector'da `Scene3D.depth_plate` kenar çizimi ve `NodeKind::Anime4K` görünümü eklendi.

### 4. Anime4K v4.x WGSL Zinciri & Real-ESRGAN Upscaler (`frameshift-schema`, `frameshift-render`, `frameshift-media`)
- **8 yeni WGSL shader (`crates/frameshift-render/shaders/`):** `fx_anime4k_bridge.wgsl`, `fx_anime4k_clamp_highlights.wgsl`, `fx_anime4k_restore_cnn_s.wgsl`, `fx_anime4k_restore_cnn_soft_s.wgsl`, `fx_anime4k_upscale_cnn_x2_s.wgsl`, `fx_anime4k_upscale_denoise_cnn_x2_s.wgsl`, `fx_anime4k_darken_hq.wgsl`, `fx_anime4k_thin_hq.wgsl` (toplam **52 WGSL**).
- [`graph.rs`](../../crates/frameshift-schema/src/graph.rs), [`pipelines.rs`](../../crates/frameshift-render/src/pipelines.rs), [`compositor.rs`](../../crates/frameshift-render/src/compositor.rs): `Anime4KPreset` (`ModeA`, `ModeB`, `ModeC`, `RestoreOnly`, `LineArt`), `NodeKind::Anime4K` ve `evaluate_anime4k` (`2x->1x` supersampling + `Rgba16Float` ping-pong).
- [`upscaler.rs`](../../crates/frameshift-media/src/upscaler.rs) & `local_media/tools/realesrgan-ncnn-vulkan/realesrgan-ncnn-vulkan.exe` (`v0.2.5.0`): `RealEsrganModel` (`realesr-animevideov3` `-s 2/3/4`, `realesrgan-x4plus-anime`, `realesrgan-x4plus`), `ClipAnalysis` dedup + chunked batch video/kare büyütme.

### 5. `Depth-Anything-V2-Small` 16-bit/RG16 & `Scene3D` Depth/DoF (`scripts/`, `frameshift-media`, `frameshift-render`, `frameshift-schema`)
- [`scripts/ai_infer_sidecar.py`](../../scripts/ai_infer_sidecar.py), [`frame.rs`](../../crates/frameshift-media/src/frame.rs), [`ai_inference.rs`](../../crates/frameshift-media/src/ai_inference.rs): Aspect-preserving `_snap14` boyutlandırma, `--norm-mode {ema,per_frame,global_pass}`, `--depth-format {u8,rg16}`, `--png16-dir`, `Rgba16fFrame::from_rg16_packed`, `DepthNormMode`, `DepthOutputFormat`, `run_ai_depth_f32_inference` ve `run_ai_depth_png16_sequence`.
- [`scene3d_depth_seed.wgsl`](../../crates/frameshift-render/shaders/scene3d_depth_seed.wgsl), [`fx_dof_coc.wgsl`](../../crates/frameshift-render/shaders/fx_dof_coc.wgsl), [`fx_dof_coc_msaa.wgsl`](../../crates/frameshift-render/shaders/fx_dof_coc_msaa.wgsl): `disparity_mode` (`1/z`), `depth_gamma`, 4-tap bilinear `rg16_packed` örnekleme ve `raw_d >= 0.99999 || alpha <= 1e-4` DoF void kapısı.

### 6. CLI & MCP Entegrasyonu (`frameshift-media/src/lib.rs`, `frameshift-cli`, `frameshift-mcp`)
- [`frameshift-media/src/lib.rs`](../../crates/frameshift-media/src/lib.rs): `spectral`, `waveform`, `ai_audio`, `upscaler` modülleri ve yeni `ai_inference` API'leri dışa açıldı.
- [`main.rs`](../../crates/frameshift-cli/src/main.rs) & [`workbench.rs`](../../crates/frameshift-cli/src/workbench.rs): `peaks`/`autocut` (`--band`, `--json`), `waveform`, `ui-snapshot` disk sesi desteği, `ai-stems`, `ai-lyrics`, `upscale`, genişletilmiş `ai-status` ve `ai-matte` (`--depth-format`, `--norm-mode`, `--png16-dir`).
- [`server.rs`](../../crates/frameshift-mcp/src/server.rs): `analyze_multiband_beats`, `render_waveform`, `separate_stems`, `align_lyrics`, `upscale_media` araçları ile güncel `ai_capabilities` ve `describe_schema`.

---

## Bağımsız Doğrulama ve Kanıtlar (`verifier`)

- **0 Yeni Crate:** `git diff --stat -- Cargo.toml Cargo.lock crates/*/Cargo.toml` boş döndü (0 değişiklik).
- **Clippy:** `cargo clippy --workspace --all-targets -- -D warnings` → **0 uyarı, 0 hata** (`exit 0`).
- **9-Crate Test Suite (`415 passed, 1 pre-existing HEAD failure`):**
  - `frameshift-time`: **18/18 passed** (`lib`: 6, `expr_tests`: 12)
  - `frameshift-schema`: **71/71 passed** (`lib`: 3, `audio_fade_tests`: 11, `autocut_planner_tests`: 11, `schema_tests`: 31, `scrim_tests`: 6, `template_precomp_portable_tests`: 9)
  - `frameshift-media`: **73/73 passed** (`lib`: 57, integration: 16)
  - `frameshift-vector`: **36/36 passed** (`lib`: 15, `faz6_5_vector_tests`: 6, `mesh3d_tests`: 5, `vector_tests`: 10)
  - `frameshift-render`: **106/106 passed** (`lib`: 27, `anime4k_and_depth_tests`: 3, `compositor_tests`: 16, `faz6_1`: 6, `faz6_2`: 5, `faz6_3`: 6, `faz6_4`: 8, `faz6_5`: 4, `faz6_6_fx`: 4, `faz6_6_optics`: 4, `faz6_6_scene3d`: 4, `optical_flow_tests`: 3, `p0_fx_rig_matte`: 7, `scrim_tests`: 6, `transition_tests`: 3)
  - `frameshift-core`: **61/62 passed, 1 failed** (`lib`: 33/33, `audio_fade_tests`: 6/6, `core_tests`: 16/16, `faz6_5_showcase_tests`: 1/1, `materials_tests`: 5/6 — `materials_gallery_fit_is_normalized` `diamond_3d m=0 height_fraction 0.896 outside [0.60, 0.85]` hatası; `git diff c75427a` ile `inspection.rs` ve `shape3d.rs`'nin `HEAD`'den beri değişmediği, hatanın önceden mevcut olduğu doğrulandı)
  - `frameshift-app`: **15/15 passed** (`lib`: 11, `app_tests`: 4)
  - `frameshift-mcp`: **9/9 passed** (`lib`: 6, `mcp_tests`: 3)
  - `frameshift-cli`: **26/26 passed** (`bin`: 26)
- **Canlı CLI Smoke Testleri (`target\debug\frameshift-cli.exe`):**
  1. `ai-status`: `exit 0` — Sistem Python `C:\Python314\python.exe` (`onnxruntime 1.24.4`, `DmlExecutionProvider`), `.venv-audio` Python `3.12.10` (`torch 2.6.0+cu124`, `NVIDIA GeForce RTX 4050 Laptop GPU`, `audio-separator 0.48.0`, `faster-whisper 1.2.1`, `stable-whisper 2.19.1`, `fugashi`, `unidic_lite`, `onnxruntime 1.32.0` `CUDAExecutionProvider`), `rife-ncnn-vulkan.exe`, `realesrgan-ncnn-vulkan.exe` (5 model, `supports_animevideov3: true`), `depth_anything_v2_small.onnx` ve `isnetis.onnx` tümü `true`.
  2. `waveform --input local_media\bgm_edit.wav --style spectral --width 1280 --height 240`: `exit 0` — `53,524` byte gerçek PNG (`89 50 4E 47 0D 0A 1A 0A`).
  3. `peaks local_media\asta_amv_project.json --band sub --json`: `exit 0` — geçerli JSON (`band: "sub"`, `selected_ticks: 30`, `estimated_bpm: 115.25`, `downbeats: 5`).

## Sınırlar (Epistemik Not)

- `materials_gallery_fit_is_normalized` testi temiz `HEAD` (`c75427a`) üzerinde de başarısız olan önceden mevcut bir sorundur (`diamond_3d m=0 height_fraction=0.896`); bu oturumda `inspection.rs` veya `shape3d.rs`'ye dokunulmamıştır.
- `C:` sürücüsünde ~5.46 GB boş alan kaldığından `frameshift-render` ve crate grupları PDB disk basıncını önlemek için sıralı (`-j 1` / crate bazlı) koşturulmuştur.
