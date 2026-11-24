# 2026-11-24 — Native GPU Optik Akış (`OpticalFlowBlur`), Canlı İnterpolasyon Motoru & ONNX AI Sidecar Hattı

**Tarih:** 2026-11-24  
**Faz:** P1 Çekirdek Optik Akış & AI Medya Çıkarım Uygulaması (0 Yeni Cargo Crate)  
**Durum:** Tamamlandı

---

## Amaç

`"AI / Optik Akış Çıkarımı Henüz Arabirim Düzeyinde"` boşluğunu 0 yeni Cargo crate ile üç katmanda çalışan ve test edilmiş üretim bileşenlerine dönüştürmek:
1. GPU üzerinde 4 seviyeli Pyramidal Lucas-Kanade + Horn-Schunck optik akış kestirimi ve RSMB eşdeğeri yönlü motion blur (`NodeKind::OpticalFlowBlur`).
2. `ClipAnalysis` (`dedup` + sahne kesimi koruması) kullanan canlı kare/video interpolasyon motoru (`RifeNcnnCli`, `FfmpegMinterpolate`, `FrameBlend`).
3. Geçici PNG darboğazı olmadan stdin/stdout rawvideo pipe ile çalışan Python + `onnxruntime` rotoscope/depth sidecar köprüsü.

---

## Yapılanlar

### 1. Araştırma Belgesi
- [2026-11-24-optical-flow-and-ai-inference-architecture.md](../research/2026-11-24-optical-flow-and-ai-inference-architecture.md) eklendi.

### 2. Şema & Render (`frameshift-schema`, `frameshift-render`)
- `frameshift-schema/src/graph.rs`, `error.rs`, `project.rs`: `NodeKind::OpticalFlowBlur` (`shutter_amount`, `samples`, `max_blur_px`, `sensitivity`, `flow_smoothness`, `visualize_flow`), `InvalidOpticalFlowBlur`, `ALL_SERDE_NAMES` (`"optical_flow_blur"`) ve animasyon parametreleri eklendi (`template.rs` içindeki 4 `clippy::needless_borrows_for_generic_args` uyarısı da temizlendi).
- `frameshift-render`:
  - 3 yeni WGSL shader: [`fx_optical_flow_luma.wgsl`](../../crates/frameshift-render/shaders/fx_optical_flow_luma.wgsl), [`fx_optical_flow_step.wgsl`](../../crates/frameshift-render/shaders/fx_optical_flow_step.wgsl), [`fx_optical_flow_blur.wgsl`](../../crates/frameshift-render/shaders/fx_optical_flow_blur.wgsl).
  - [`uniforms.rs`](../../crates/frameshift-render/src/uniforms.rs): `OpticalFlowStepUniform`, `OpticalFlowBlurUniform`.
  - [`pipelines.rs`](../../crates/frameshift-render/src/pipelines.rs) & [`compositor.rs`](../../crates/frameshift-render/src/compositor.rs): `evaluate_optical_flow_blur` (4 seviyeli piramit + seviye başına 2 iterasyon coarse-to-fine çözücü + IGN jitter'lı motion blur gather / HSV debug görselleştirme + `sensitivity` sahne kesimi koruması).
  - [`optical_flow_tests.rs`](../../crates/frameshift-render/tests/optical_flow_tests.rs): Hareketli blok HSV vektörleri + blur yayılımı, durağan sahne koruması ve sert sahne kesiminde sahte blur bastırma testleri.

### 3. Canlı İnterpolasyon & ONNX AI Sidecar (`frameshift-media`, `scripts/`)
- [`interpolator.rs`](../../crates/frameshift-media/src/interpolator.rs): `InterpolateConfig`, `InterpolationReport`, `resolve_interpolator_backend`, `interpolate_frame_sequence` (`RifeNcnnCli`, `FfmpegMinterpolate`, `FrameBlend` + `ClipAnalysis` *on twos* dedup & sahne kesimi segmentasyonu) ve `interpolate_video_file`.
- [`scripts/ai_infer_sidecar.py`](../../scripts/ai_infer_sidecar.py) & [`ai_inference.rs`](../../crates/frameshift-media/src/ai_inference.rs): `detect_ai_capabilities`, `run_ai_frame_inference`, `run_ai_video_inference` (rawvideo stdin/stdout pipe, `DmlExecutionProvider` -> `CUDAExecutionProvider` -> `CPUExecutionProvider` sırası, temporal EMA smoothing, `--synthetic-test`).

### 4. App, Core, CLI & MCP Entegrasyonu
- [`snapshot.rs`](../../crates/frameshift-app/src/snapshot.rs): `NodeKind::OpticalFlowBlur` etiket, renk ve özet formatı.
- [`workbench.rs` (`frameshift-core`)](../../crates/frameshift-core/src/workbench.rs): `rsmb_anime_motion` hazır `EffectChainPreset` eklendi.
- [`workbench.rs` (`frameshift-cli`)](../../crates/frameshift-cli/src/workbench.rs): `ai-status`, `interpolate` ve `ai-matte` alt komutları eklendi.
- [`server.rs` (`frameshift-mcp`)](../../crates/frameshift-mcp/src/server.rs): `"optical_flow_blur"` şema listesine ve `ai_capabilities` MCP aracına eklendi.

---

## Doğrulama ve Kanıtlar

- `cargo clippy --workspace --all-targets -- -D warnings`: **0 uyarı, çıkış 0**.
- `cargo test -p frameshift-schema`: **70/70 geçti** (`lib`: 3, `audio_fade_tests`: 11, `autocut_planner_tests`: 11, `schema_tests`: 30, `scrim_tests`: 6, `template_precomp_portable_tests`: 9).
- `cargo test -p frameshift-media --lib`: **27/27 geçti** (canlı `ffmpeg` 24fps→48fps `minterpolate` MP4 + `ffprobe` testi ve Python `onnxruntime` raw-pipe `rotoscope`/`depth` testi dahil).
- `cargo test -p frameshift-render --test optical_flow_tests --test compositor_tests --test p0_fx_rig_matte_tests`: **25/25 geçti** (`optical_flow_tests`: 2/2, `compositor_tests`: 16/16, `p0_fx_rig_matte_tests`: 7/7).
- `cargo test -p frameshift-core --lib`: **33/33 geçti**.
- `cargo test -p frameshift-app`: **14/14 geçti** (`lib`: 10, `app_tests`: 4).
- `cargo test -p frameshift-mcp`: **8/8 geçti** (`lib`: 5, `mcp_tests`: 3).
- `cargo test -p frameshift-cli --bin frameshift-cli`: **25/25 geçti**.
- `cargo run -p frameshift-cli -- ai-status`: `ffmpeg_available: true`, `rife_available: false`, `python_available: true`, `sidecar_script_found: true`, `onnxruntime_version: "1.32.0"`, `recommended_interpolator: "FfmpegMinterpolate"`.

## Sınırlar (Epistemik Not)

- Yerel ortamda `rife-ncnn-vulkan` ikilisi ve `.onnx` model dosyaları (`anime-segmentation`/`BiRefNet`/`Depth-Anything-V2-Small`) bulunmadığından bu iki harici modelin gerçek GPU kalite/VRAM benchmark'ları henüz ölçülmemiştir; `FfmpegMinterpolate` gerçek `ffmpeg.exe` üzerinde ve ONNX sidecar boru hattı `onnxruntime 1.32.0` + `--synthetic-test` üzerinden canlı doğrulanmıştır.
