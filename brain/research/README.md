# FrameShift — Araştırma Dizini (`brain/research/`)

Bu dizin; kütüphane doğrulamaları, GPU/medya I/O mimari tasarımları, lisans kontrolleri, üretim dogfood denetimleri ve görsel kalite analizleri için birincil kaynaklı teknik araştırma raporlarını (**46 belge**) tutar. Genel indeks için ayrıca [../index.md](../index.md) ve uygulama doğrulamaları için [../state.md](../state.md) dosyalarına bakılabilir.

---

## Kök Dizindeki Mevcut Araştırma ve Plan Belgeleri (Köprü)

Projenin başlangıç ve Faz 0–5 planlama aşamasında üretilen kapsamlı belgeler kök dizinde korunmaktadır (bu dosyalara köprü üzerinden referans verilir, kopyaları çoğaltılmaz):

1. **[ENGINE_ALTERNATIVES_RESEARCH.md](../../ENGINE_ALTERNATIVES_RESEARCH.md):** HyperFrames, Remotion, native GPU yaklaşımları ve mevcut editör/MCP adaylarının kaynaklı karşılaştırması. *(Not: Benchmark değildir; güncel stack kararı ADR [0001](../decisions/0001-independent-native-gpu-core.md) ile netleşmiştir.)*
2. **[VISUAL_REFERENCES.md](../../VISUAL_REFERENCES.md):** Kullanıcının paylaştığı ilk referans videolar, örnek kare gözlemleri ve motor gereksinimlerine çıkarımlar.
3. **[IMPLEMENTATION_PLAN.md](../../IMPLEMENTATION_PLAN.md):** Faz 0–5 (M1–M5) implementasyon planı, crate mimarisi ve doğrulama kapıları.

---

## 1. Faz 6.1–6.6 Motor, Shader, Decoder, Renk & Tipografi Araştırmaları (15 Belge)

| Belge | Durum | Konu Özeti |
| :--- | :---: | :--- |
| [open-source-shader-transitions.md](open-source-shader-transitions.md) | `active` | `gl-transitions` (MIT/BSD) analizi ve FrameShift WGSL geçiş uniform şartnamesi |
| [anime-amv-postfx-shaders.md](anime-amv-postfx-shaders.md) | `active` | Vidvox ISF, Libretro `slang-shaders`, GodotShaders (MIT/CC0) tabanlı 4 Anime/AMV post-FX shader ailesi |
| [open-source-fx-catalog-and-roadmap.md](open-source-fx-catalog-and-roadmap.md) | `active` | Açık kaynak efekt/geçiş kataloğu (GitHub SPDX lisans kontrolü) ve Faz 6.1–6.2 görsel kalite yol haritası |
| [two-tier-extensible-effect-architecture.md](two-tier-extensible-effect-architecture.md) | `active` | İki katmanlı genişletilebilir efekt mimarisi — Çekirdek yapı taşları + Küratörlü WGSL Kütüphanesi & runtime `CustomWgsl` sandbox |
| [3d-kinetic-typography-architecture.md](3d-kinetic-typography-architecture.md) | `active` | 3D kinetik tipografi için eksik 6 teknik alan ve mimari çözümler (`glam`, `ttf-parser`+`lyon`, Jimenez Bloom, DoF, parçacık/şerit, lyric alignment) |
| [faz6-3-scene3d-dof-camera.md](faz6-3-scene3d-dof-camera.md) | `active` | Faz 6.3 Part A — Paylaşımlı `Scene3D`, `Camera3D`, `Depth32Float` Vogel Disk Bokeh DoF & `SafeExpr` easing |
| [faz6-3-text3d-shapes-ribbons.md](faz6-3-text3d-shapes-ribbons.md) | `active` | Faz 6.3 Part B — `Text3D` kinetik/kontur/chrome/Z-echo, 5 prosedürel `Shape3D` primitifi ve 3'lü paralel `RibbonTrail3D` |
| [streaming-video-decoder.md](streaming-video-decoder.md) | `active` | Sürekli açık `StreamingVideoDecoder` & `DecoderPool` (`MAX_OPEN_STREAMS=8`, `MAX_SEQUENTIAL_SKIP=32`) |
| [cube-lut-and-ocio.md](cube-lut-and-ocio.md) | `active` | Harici `.cube` (1D+3D) / `.spi1d`/`.spi3d` ayrıştırıcı ve saf Rust `OcioColorPipeline` |
| [2d-collage-precomp-hud-warp.md](2d-collage-precomp-hud-warp.md) | `active` | `SourceImage`, kapsüllü `Precomp`/`PrecompInput`, `VectorPathHud`, `CornerPin` ve 8×8 `GridWarp` |
| [2d-vectortext-ttf-layout.md](2d-vectortext-ttf-layout.md) | `draft` | 2D `VectorText` gerçek TTF/OTF font, çok satır, `TextAlign2D` ve M1 golden hash koruması *(uygulandı: Faz 6.4)* |
| [typography-nextgen-architecture.md](typography-nextgen-architecture.md) | `active` | Faz 6.5 — `harfrust` + `skrifa` shaping, WGSL Slug GPU Bézier (`text_slug.wgsl`), Tategaki, `BevelProfile3D` ve 13 SIL OFL font |
| [faz6-6-optics-tonemap-aa.md](faz6-6-optics-tonemap-aa.md) | `active` | Faz 6.6 Part A — `AgX`/`PbrNeutral`/`AgxPunchy` tonemap, TPDF/IGN dithering, `4x MSAA`, FXAA, separable Halation/Grain/CA/DeepGlow |
| [faz6-6-scene3d-pbr-dof-blend.md](faz6-6-scene3d-pbr-dof-blend.md) | `active` | Faz 6.6 Part B — 3D Cook-Torrance GGX PBR, kamera-bağıl IBL, separable CoC bokeh DoF & 5 yeni blend modu |
| [faz6-6-fx-transitions-sdf.md](faz6-6-fx-transitions-sdf.md) | `active` | Faz 6.6 Part C — Sürekli Gauss Shockwave, 10 aspect-correct geçiş, CMYK/ColorDot Halftone & 2-yönlü JFA SDF `text2d_fx` |

---

## 2. Anime Edit İş Akışı, AI Sidecar, Optik Akış & Beş Temel Yetenek (9 Belge)

| Belge | Durum | Konu Özeti |
| :--- | :---: | :--- |
| [2026-11-24-social-anime-editors-workflow.md](2026-11-24-social-anime-editors-workflow.md) | `active` | TikTok/IG/YouTube anime editörleri (Xenoz/Molob/Nep) iş akışı, 5 stil ve FrameShift eşleşmesi |
| [2026-11-24-anime-workflow-gap-audit.md](2026-11-24-anime-workflow-gap-audit.md) | `active` | 6 adımlı swarm denetimi: medya/RIFE, ses/SFX, Speed Graph, Null3D/matte/RSMB, Post-FX/tipografi, export/MCP P0–P2 planı |
| [2026-11-24-ai-rife-ort-rotoscope-guide.md](2026-11-24-ai-rife-ort-rotoscope-guide.md) | `active` | `rife-ncnn-vulkan`, `ort` ve ONNX rotoscope/depth (`anime-segmentation`/`BiRefNet`/`SAM 2`/`Depth-Anything-V2-Small`) lisans & entegrasyon rehberi |
| [2026-11-24-optical-flow-and-ai-inference-architecture.md](2026-11-24-optical-flow-and-ai-inference-architecture.md) | `active` | 4 seviyeli WGSL Pyramidal Lucas-Kanade/Horn-Schunck RSMB (`OpticalFlowBlur`), `ClipAnalysis` interpolasyon & raw-pipe ONNX sidecar |
| [2026-11-24-multiband-spectral-flux-downbeat.md](2026-11-24-multiband-spectral-flux-downbeat.md) | `active` | Saf Rust Radix-2 FFT (`N=4096`), Sub/Mid/High flux, Ellis tempo + 4/4 downbeat & `SafeExpr` bant uniformları |
| [2026-11-24-ai-stem-separation-lyric-alignment.md](2026-11-24-ai-stem-separation-lyric-alignment.md) | `active` | İzole Py 3.12 CUDA `audio-separator` + hibrit `stable-ts`/`faster-whisper` + `scripts/ai_audio_sidecar.py` |
| [2026-11-24-real-waveform-png-rendering.md](2026-11-24-real-waveform-png-rendering.md) | `active` | `WaveformPyramid`, Dual-Layer Peak/RMS & 3-bant spektral rasterizer + `snapshot.rs` ve `render_waveform` MCP |
| [2026-11-24-anime4k-wgsl-and-realesrgan-upscaler.md](2026-11-24-anime4k-wgsl-and-realesrgan-upscaler.md) | `active` | 8 shader'lı Anime4K v4.x WGSL zinciri (`NodeKind::Anime4K`) & `realesrgan-ncnn-vulkan v0.2.5.0` batch/dedup sürücüsü |
| [2026-11-24-depth-anything-v2-scene3d-injection.md](2026-11-24-depth-anything-v2-scene3d-injection.md) | `active` | `Depth-Anything-V2-Small` 16-bit/`rg16` derinlik, temporal EMA, DoF alpha-void düzeltmesi & `Scene3D.depth_plate` |

---

## 3. Rakip Güç Analizi & P1–P5 Mimari Tasarımları (6 Belge)

| Belge | Belge Durumu | Uygulama Durumu | Konu Özeti |
| :--- | :---: | :---: | :--- |
| [2026-11-24-competitive-power-analysis-roadmap.md](2026-11-24-competitive-power-analysis-roadmap.md) | `draft` | `P1/P2/P3/P5 uygulandı` | OpenMotion/Kimo/FilmCraft/Resolve/AE/AviUtl incelemesi ve P1–P5 öncelik yol haritası |
| [2026-11-24-power-p1-creative-engine.md](2026-11-24-power-p1-creative-engine.md) | `draft` | `uygulandı (2026-11-24)` | `SafeExpr` (`spring`/`wiggle`/operatörler/`if`), `DisplacementMap`, `FrameEcho`, `ShapeMask` morph ve ADR-0003 Kahn DAG kısıtları |
| [2026-11-24-power-p2-agent-dsl-linter.md](2026-11-24-power-p2-agent-dsl-linter.md) | `draft` | `uygulandı (2026-11-24)` | 10 kurallı `lint_project`, 3-panelli `diff_frame`, `Motion.md` DSL derleyici ve `ParametricTemplateSpec` |
| [2026-11-24-power-p3-gpu-yuv-smart-cache.md](2026-11-24-power-p3-gpu-yuv-smart-cache.md) | `draft` | `uygulandı (2026-11-24)` | GPU NV12 → Linear Rec.709 (`yuv_to_linear.wgsl`) ve 3-katmanlı `NodeContentHash` Smart Cache |
| [2026-11-24-power-p4-live-desktop-shell.md](2026-11-24-power-p4-live-desktop-shell.md) | `draft` | `sonraki faza bırakıldı` | Sıfır-kopya `egui 0.37` + `winit 0.30` ince canlı masaüstü önizleme kabuğu tasarımı |
| [2026-11-24-power-p5-tracker-otio-ai-manifest.md](2026-11-24-power-p5-tracker-otio-ai-manifest.md) | `draft` | `uygulandı (2026-11-24)` | Pyramidal LK 2D/Planar Tracker (`keyframe_tracks`), `.otio`/FCPXML köprüsü & `model_manifest.json` AI yuvası |

---

## 4. Üretim Dogfood, Sürtünme & Asta v3 Araştırmaları (16 Belge)

| Belge | Durum | Konu Özeti |
| :--- | :---: | :--- |
| [2026-11-21-edit-friction.md](2026-11-21-edit-friction.md) | `active` | Koleedits dogfood sürtünmesinin 8 maddesi *(2026-11-21'de uygulandı ve doğrulandı; bkz. [log](../log/2026-11-21-edit-friction-verify.md))* |
| [2026-11-22-asta-v3-uretken-darbogazlari.md](2026-11-22-asta-v3-uretken-darbogazlari.md) | `active` | Asta v3 üretim engelleri: 5 gerçek eksik + 5 "keşke" + 3 araç keşfi |
| [2026-11-22-kinetic-typography-gap-analysis.md](2026-11-22-kinetic-typography-gap-analysis.md) | `active` | SC-0002 Mushoku Tensei yeniden üretimi — tipografi, matte ve kırpma boşluk analizi |
| [2026-11-22-frameshift-tuzaklar.md](2026-11-22-frameshift-tuzaklar.md) | `active` | SC-0002 üretiminden çıkarılan sessiz tuzaklar (söz senkronu, ofset birimleri, sRGB/lineer, opak taban, luma maskesi) |
| [2026-11-22-frameshift-urun-gelistirme-yol-haritasi.md](2026-11-22-frameshift-urun-gelistirme-yol-haritasi.md) | `draft` | HyperFrames, OpenMotion ve After Effects'ten P0–P2 ürün geliştirme dersleri |
| [2026-11-22-render-range.md](2026-11-22-render-range.md) | `draft` | Asta v3: `render --range` aralık render'ı ve dışa snap |
| [2026-11-22-render-progress-eta.md](2026-11-22-render-progress-eta.md) | `draft` | Asta v3: stderr render ilerleme/ETA ve EMA tahmini |
| [2026-11-22-probe-sheet-multiframe.md](2026-11-22-probe-sheet-multiframe.md) | `draft` | Asta v3: tek süreçte N kare üretimi (`sheet` komutu) |
| [2026-11-22-material-swatch-gallery.md](2026-11-22-material-swatch-gallery.md) | `draft` | Asta v3: şekil/malzeme swatch galerisi (`materials` komutu) |
| [2026-11-22-cli-help-discovery.md](2026-11-22-cli-help-discovery.md) | `draft` | Asta v3: CLI keşfedilebilirlik (`--help`, `help <cmd>`, komut tablosu) |
| [2026-11-22-autocut.md](2026-11-22-autocut.md) | `draft` | Asta v3: `autocut` v1 deterministik kesim gridi |
| [2026-11-22-particle-burst-binding.md](2026-11-22-particle-burst-binding.md) | `draft` | Asta v3: `ParticleField3D` burst alt-yapısı + `emission` |
| [2026-11-22-smart-reframe-916.md](2026-11-22-smart-reframe-916.md) | `draft` | Asta v3: 9:16 akıllı statik reframe ve `SourceVideo.focus` |
| [2026-11-22-audio-fade-gain-automation.md](2026-11-22-audio-fade-gain-automation.md) | `draft` | Asta v3: fade-only v1 (`AudioFadeDef` + `gain_db`) |
| [2026-11-22-ribbon-color-thickness.md](2026-11-22-ribbon-color-thickness.md) | `draft` | Asta v3: `RibbonTrail3D` renk/kalınlık varsayılan düzeltmesi |
| [2026-11-22-text-scrim-backplate.md](2026-11-22-text-scrim-backplate.md) | `draft` | Asta v3: 1 girişli `BackdropScrim` + yarı çözünürlük blur |

---

## Araştırma Standardı

Yeni bir teknik araştırma dosyası açarken [../_templates/arastirma.md](../_templates/arastirma.md) şablonunu kullan ve şu 3 ilkeye uy:
1. **Birincil Kaynak Zorunluluğu:** Arama özeti yerine resmi dokümantasyon, kaynak kod, `.d.ts` / `.rs` trait imzaları veya resmi lisans metinleri incelenir; URL ve inceleme tarihi kaydedilir.
2. **Gözlem vs. Çıkarım Ayrımı:** Doğrudan okunan/ölçülen gerçeklerle tahmini çıkarımlar ayrı başlıklarda tutulur.
3. **Doğrulanmamış Noktalar:** Henüz denenmemiş (ör. belirli GPU sürücüsü, canlı GUI gecikmesi) noktalar "doğrulanmadı" etiketiyle açıkça işaretlenir.
