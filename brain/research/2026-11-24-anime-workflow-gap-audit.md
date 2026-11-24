---
title: "Sosyal Medya Anime Editör İş Akışı vs FrameShift Yetenek Boşluk Denetimi"
date: 2026-11-24
status: active
tags: [agent, shaders, timebase, render-graph, media-io, ui, schema]
---

# Sosyal Medya Anime Editör İş Akışı vs FrameShift Yetenek Boşluk Denetimi

## Soru / Amaç

TikTok, Instagram Reels ve YouTube'da öne çıkan anime editörlerinin (Xenoz, Molob, Nep/Neptune vb.) kullandığı 6 adımlı üretim hattının her parçasını internet kaynaklarından araştırmak ve FrameShift'in 9-crate kod tabanıyla satır/sembol düzeyinde karşılaştırarak nelerin mevcut, nelerin eksik, nelerin hatalı olduğunu ve P0/P1/P2 uygulama planını çıkarmak.

## İncelenen Birincil Kaynaklar

- **Medya & Optik Akış:**
  - `nihui/rife-ncnn-vulkan` (MIT): https://github.com/nihui/rife-ncnn-vulkan
  - `hzwer/Practical-RIFE` (MIT): https://github.com/hzwer/Practical-RIFE
  - `xinntao/Real-ESRGAN-ncnn-vulkan` (MIT): https://github.com/xinntao/Real-ESRGAN-ncnn-vulkan
  - `bloc97/Anime4K` (MIT): https://github.com/bloc97/Anime4K | `Anime4K-WebGPU` (MIT): https://github.com/Anime4KWebBoost/Anime4K-WebGPU
  - `pykeio/ort` (MIT/Apache-2.0): https://github.com/pykeio/ort
  - FFmpeg Filtreleri (`minterpolate`, `mpdecimate`, `select`): https://ffmpeg.org/ffmpeg-filters.html
- **Ses & SFX / AI Ayrım:**
  - `realfft` (MIT): https://github.com/HEnquist/realfft | `RustFFT` (MIT/Apache-2.0): https://github.com/ejmahler/RustFFT
  - `beat-this-rs` (MIT): https://github.com/danigb/beat-this-rs | `librosa-rs` (ISC): https://github.com/DaniilPak/librosa-rs
  - `ebur128` (MIT): https://github.com/sdroege/ebur128 | `ebur128-stream` (MIT/Apache-2.0): https://github.com/vanjamodrinjak21/ebur128-stream
  - `python-audio-separator` (MIT): https://github.com/nomadkaraoke/python-audio-separator | `demucs` (MIT): https://github.com/facebookresearch/demucs
  - `faster-whisper` (MIT): https://github.com/SYSTRAN/faster-whisper | `stable-ts` (MIT): https://github.com/jianfch/stable-ts
  - *(Lisans uyarısı: `aubio-rs` GPL ve `Essentia` AGPL-3.0 olduğu için çekirdeğe alınmamalı.)*
- **Speed Graph, Easing & Canlı GUI:**
  - Adobe After Effects Speed & Graph Editor: https://helpx.adobe.com/after-effects/using/speed.html | https://ae-scripting.docsforadobe.dev/other/keyframeease/
  - `egui` / `egui-wgpu` / `winit` (MIT/Apache-2.0): https://github.com/emilk/egui | https://github.com/rust-windowing/winit
- **Rotoscope, Depth Matte & Motion Blur:**
  - `facebookresearch/sam2` (Apache-2.0): https://github.com/facebookresearch/sam2
  - `ZhengPeng7/BiRefNet` (MIT): https://github.com/ZhengPeng7/BiRefNet
  - `SkyTNT/anime-segmentation` (Apache-2.0): https://github.com/SkyTNT/anime-segmentation
  - `hkchengrex/Cutie` (MIT): https://github.com/hkchengrex/Cutie
  - `DepthAnything/Depth-Anything-V2` (Kod Apache-2.0; ViT-S Apache-2.0, ViT-B/L/G CC-BY-NC-4.0): https://github.com/DepthAnything/Depth-Anything-V2
  - RE:Vision Effects RSMB (referans): https://revisionfx.com/products/rsmb/
  - *(Lisans uyarısı: `RMBG-2.0` CC-BY-NC-4.0, `MatAnyone` NTU S-Lab Non-Commercial, `RobustVideoMatting` GPL-3.0.)*
- **Post-FX, Tipografi & Blender Hibrit:**
  - Boris FX Sapphire (`S_Shake`, `S_BlurMoCurves`, `S_Glow`, `S_LensFlare`): https://borisfx.com/documentation/sapphire/
  - Deep Glow: https://aescripts.com/deep-glow/
  - Maxon Magic Bullet Looks: https://www.maxon.net/en/red-giant/magic-bullet/looks
  - AE Text Animators: https://helpx.adobe.com/after-effects/using/animating-text.html
  - Blender UV Project & Grease Pencil: https://docs.blender.org/manual/en/latest/modeling/modifiers/modify/uv_project.html
  - `gltf` crate (MIT/Apache-2.0): https://crates.io/crates/gltf
- **Sosyal Medya Export:**
  - YouTube Encoding: https://support.google.com/youtube/answer/1722171
  - FFmpeg H.264 / H.265 Guides: https://trac.ffmpeg.org/wiki/Encode/H.264 | https://trac.ffmpeg.org/wiki/Encode/H.265

---

## Doğrudan Gözlemler (6 Adımlı Kod Denetimi — 2026-11-24)

### Adım 1: Kaynak Medya, Ön İşleme ve Optik Akış (`crates/frameshift-media`)
- **Mevcut:** `ffmpeg.rs` içinde `ffprobe` tabanlı deterministik `PtsIndex`, `HwAccelDetector`, `Mp4FrameEncoder` ve `decode_video_window`; `streaming.rs` içinde `DecoderPool` (`max_open_streams=8`, `max_sequential_skip=32`); `frame.rs` içinde sRGB↔Linear `Rgba16fFrame` ve CPU bilinear `fit_rgba8_frame`; `cache.rs` LRU bellek önbelleği; `proxy.rs` SHA-256 doğrulamalı MP4 proxy yöneticisi.
- **Eksik:** (1) Kesirli zaman `t ∈ (0,1)` için RIFE/Twixtor optik akış interpolasyonu (`PtsIndex` yalnızca en yakın tam kareyi veriyor); (2) animedeki *on twos/threes* (12/8 fps) için eşik tabanlı duplike kare temizleme (`dedup`) ve sahne kesimi algılama (SCD); (3) gerçek zamanlı Anime4K WGSL veya offline Real-ESRGAN upscaling; (4) `probe_media_info` içinde 10-bit/renk alanı/telecine metadata ve mezzanine ön işleme önbelleği.
- **Geliştirilecekler:** `DecoderPool` yalnızca ileri sıralı okumada pipe'ı koruyor (geri adım/speed-ramp'ta yeni `ffmpeg` süreci açıyor); `FrameCacheKey` fit/kesirli `t` bilgisini tutmuyor.

### Adım 2: Ses Haritası, Beat/Onset, Çok Kanallı SFX & AI Ayrım (`audio.rs`, `autocut.rs`, `engine.rs`, `resolver.rs`, `project.rs`)
- **Mevcut:** 48 kHz stereo PCM miksaj, `AudioRetimeMode::{Mute, Varispeed, PitchPreserving}` (WSOLA), tek-bant RMS + adaptif zirve tespiti (`analyze_beats_and_energy`), `plan_autocut`, `AudioFadeDef`, `AudioRmsEnvelope` ve `verify_audio_impact_sync`.
- **Eksik:** (1) Çok-bantlı (sub/mid/high) spectral flux + downbeat (ölçü başı) tespiti; (2) `AudioBus` (`Music/Dialogue/SfxImpact/SfxWhoosh/SfxRiser/SubBass`), stereo pan ve olay-çapalı SFX hizalama (`anchor_event_id` + `apex_offset_ticks`); (3) drop öncesi 1–5 kare mikro-sessizlik ve sidechain ducking; (4) HPF/LPF biquad EQ, true-peak limiter ve EBU R128 (-14 LUFS); (5) AI stem/diyalog ayrımı (`python-audio-separator`/`demucs`) ve kelime zamanlaması (`faster-whisper`+`stable-ts`).
- **Tespit Edilen Bug & İyileştirmeler:**
  - **Bug (`engine.rs:970-991`):** `audio_envelope_cache_key` `ac.fade_in` ve `ac.fade_out` alanlarını anahtara dahil etmiyor.
  - **Hard Clipping (`resolver.rs:611-613`, `audio.rs:282-284`):** Miksaj sonunda `clamp(-1.0, 1.0)` dijital kırpma üretiyor; soft/lookahead limiter gerekli.
  - **Tekrarlı Decode (`resolver.rs:594-609`):** Aynı SFX asset'i her klip için yeniden FFmpeg çalıştırıyor; `AudioBuffer` önbelleği eklenmeli.

### Adım 3: Time Remapping, Speed Graph ("Anime Snap"), One-Framer & Canlı Scrub (`frameshift-time`, `frameshift-app`)
- **Mevcut:** `SpeedKeyframe` (tek skaler `tangent`), `SpeedCurveSpec` (`Constant/Freeze/PiecewiseLinear/CubicHermite`), 4-noktalı GL4 integrasyon LUT (`32` alt-bölme), `ImpactSolver` (`TwoSegmentConstant`/`SmoothRamp`), yakıt-sınırlı `SafeExpr`, headless `TimelineProjection`, `snap_tick`, `PreviewRequestGate` ve `WorkspaceUiController`.
- **Eksik:** (1) AE kırık/asimetrik `KeyframeEase(speed, influence)` ve **Value Graph ↔ Speed Graph** çift yönlü dönüşümü; (2) asimetrik yüksek pikli **"Anime Snap"** eğri stili; (3) **Posterize Time (stutter)**, **One-Framer** üretici ve `SafeExpr` içinde `floor`/`fract`/`mod`/`cubic_bezier`; (4) `winit` + `egui` + `egui-wgpu` canlı viewport/timeline editörü (`scrub_to` henüz `PreviewRequestGate` worker'ına bağlı değil).

### Adım 4: 2.5D/3D Kamera + Null Rig, Rotoscope / Matte & Motion Blur (`mask_matte.wgsl`, `motion_blur_accumulate.wgsl`, `transform3d.wgsl`, `graph.rs`, `compositor.rs`)
- **Mevcut:** `Camera3DDef` (`Euler`/`LookAt`, Bokeh DoF), `Transform3D` ve ortak `Depth32Float` + 4x MSAA kullanan `Scene3D`, `MaskMatte` (`Alpha`/`Luma`), `MotionBlurShutter` (0–720° sub-sample DAG biriktirme).
- **Eksik:** (1) `Null3D` & parenting hiyerarşisi (`Camera -> CAM_Shake_Null -> ...`) ve `pivot`; (2) `Transform3D` kenar yansıtması (`mirror_edges`); (3) AI rotoscope (`SAM 2`/`BiRefNet`/`anime-segmentation`), `DepthMatte` ve 2D depth plate'in `Scene3D` depth buffer'ına enjekte edilmesi; (4) ham 24/12 fps klipteki karakter hareketini bulanıklaştıran **RSMB eşdeğeri `OpticalFlowBlur`**.
- **Kodda Uyumsuzluk:** `mask_matte.wgsl` `2u = Red Channel` ve `mask_opacity` destekliyor ama `graph.rs`/`compositor.rs` `MatteMode::Red` ve dinamik `opacity` geçirmiyor; `MotionBlurShutter.shutter_angle_deg` `apply_animated_parameter` içinde eksik; `MaskMatte`'de uzamsal feather/choker yok.

### Adım 5: Adjustment Layer, Post-FX, Tipografi & 3D/Manga Hibrit (`frameshift-render`, `frameshift-vector`)
- **Mevcut:** `fx_motion_tile_shake.wgsl` (2D sin/cos + `mirror_uv`), Jimenez 2014 `fx_bloom_downsample/upsample`, `fx_film_optics.wgsl` (Brown-Conrady, 7-tap spektral CA, 35mm grain) & `fx_film_halation.wgsl`, `HarfRust`+`Skrifa`+`Lyon` 2D/3D tipografi (`TextAnimatorDef` doğrusal `position/scale/opacity`, `text3d_mesh.wgsl` GGX PBR + Z-echo).
- **Eksik:** (1) Zaman aralıklı birinci sınıf `AdjustmentLayer`; (2) `S_BlurMoCurves` (`fx_blur_mocurves.wgsl`); (3) `S_Shake` 3D tilt + noise/wave + dahili blur; (4) Deep Glow inverse-square falloff + RGB CA; (5) `S_LensFlare` & MBL Optical Diffusion + Film Toe/Shoulder S-Curve; (6) AE `TextAnimator` v2 (`rotation`, `tracking`, `Range Selector` şekilleri, `randomize`, CPU `text.rs`'de `text_animator` yok sayılıyor); (7) Blender Camera Projection Mapping (`ProjectedMesh3D`) & `.glb` sahne/kamera içe aktarma.

### Adım 6: Sosyal Medya Export, Preset Ekosistemi & Ajan/CLI/MCP (`export.rs`, `template.rs`, `portable.rs`, `jobs.rs`, `workbench.rs`, `server.rs`)
- **Mevcut:** Revizyon-kilitli `ExportQueue` (`pin_project_dependencies` + `ffprobe` + atomik `rename`), `TemplatePackage` v1 (`CustomWgsl` güvenlik reddili), `ProjectMediaManifest` & relink, `ProxyManager`, bellek-içi `BackgroundJobSupervisor`, CLI `main.rs`/`workbench.rs`, 9 araçlı MCP `server.rs`.
- **Eksik:** (1) `Mp4FrameEncoder` `-colorspace bt709 -color_primaries bt709 -color_trc bt709 -color_range tv`, `-profile:v high` ve `-movflags +faststart` yazmıyor; 10-bit (`yuv420p10le`), HEVC, ProRes, VBR, AAC 48kHz 320k, `deband`/`unsharp` ve platform preset'leri (`tiktok`, `reels`, `youtube`, `anime_10bit`, `prores`) yok; (2) mevcut projeye eklenen parçalı `EffectChainPreset` (`.fsfx.json`) ve `Color`/`Choice`/`SpeedCurve`/`Font` slotları yok; (3) `portable.rs` LUT/OCIO/font relink yapmıyor; (4) `undo`/`redo`/`preflight`/`plan_autocut`/`apply_effect_preset` MCP araçları eksik, `render_project` `ExportQueue` atlıyor; (5) diskte kalıcı iş kuyruğu ve proxy kota/GC yok.

---

## Çıkarımlar ve Öneriler (Öncelik Sırası)

1. **P0 (0 Yeni Cargo Crate — Çekirdek Motor & Ajan Tamamlama):**
   - `frameshift-time`: AE `KeyframeEase`, `CubicBezierValue`/`Speed`, `ImpactCurveStyle::AnimeSnap`, `TimeQuantizeSpec` (posterize/stutter) ve `SafeExpr` `floor`/`fract`/`mod`/`cubic_bezier`.
   - `frameshift-media`: Rec.709 + `faststart` + H.264 High/10-bit HEVC/ProRes/VBR + platform profilleri + `ClipAnalysis` (`dedup` + SCD) + `RifeNcnnCli` sidecar hazırlığı + audio soft-limiter / biquad EQ / micro-silence / sidechain ducking / stereo pan.
   - `frameshift-schema` + `frameshift-render` + `frameshift-vector`: `AudioBus` & `anchor_event_id`, `AdjustmentLayer`, `BlurMoCurves` WGSL, `Null3D` parenting rig + `Transform3D` pivot/mirror_edges, `MaskMatte` (`Red` + `opacity` + `choke/spatial_feather`) + `DepthMatte`, `TextAnimatorDef` v2, DeepGlow inverse-square & `S_Shake` 3D tilt/noise, `MotionBlurShutter` `shutter_angle_deg` animasyonu.
   - `frameshift-core` + `frameshift-cli` + `frameshift-mcp`: `audio_envelope_cache_key` fade bug fix, `AudioBuffer` cache, `mix_project_audio` bus/pan/micro-silence/limiter, `ExportTarget` platform preset, CLI `--platform`, MCP araç senkronu (`undo`, `redo`, `preflight_project`, `plan_autocut`, `list_effect_presets`, `apply_effect_preset`).
2. **P1 (Orta Vadeli Kalite & Ekosistem — Kısmen Sidecar / Opsiyonel Crate):**
   - Saf WGSL `OpticalFlowBlur` (RSMB eşdeğeri), Anime4K WGSL, `rife-ncnn-vulkan` & `realesrgan-ncnn-vulkan` canlı CLI çalıştırıcı, AI stem/kelime sidecar (`python-audio-separator` + `faster-whisper`/`stable-ts`), offline AI rotoscope (`SkyTNT/anime-segmentation`, `BiRefNet`, `SAM 2`, `Depth-Anything-V2-Small`), MBL Optical Diffusion & Film Toe/Shoulder.
3. **P2 (Kullanıcı Onayı Gerektiren Crate'li Masaüstü & 3D Genişlemeler):**
   - Canlı `winit` + `egui` + `egui-wgpu` masaüstü timeline & Speed Graph editörü, gömülü `ort` ONNX Runtime, Blender `.glb` içe aktarma (`gltf` crate), `.fspack` ZIP (`zip` crate), kalıcı iş kuyruğu ve `Scene3D` velocity AOV.

## Doğrulanmamış Noktalar

- RIFE / Real-ESRGAN / AI rotoscope / `winit`+`egui` adımları dış araç/crate veya model ağırlığı gerektirdiğinden P0 kapsamında yalnızca arabirim/analiz/komut üreticileri kurulur; gerçek GPU üzerinde RIFE/AI çıkarım benchmark'ları P1/P2 aşamasında test edilecektir.
