# FrameShift Brain — Tek Sayfa Dizin

Bu dosya tüm `brain/` belleğinin hızlı rotasıdır. Oturum başında `state.md` ile birlikte okunur.

---

## Klasör Haritası

- [state.md](state.md) — aktif faz, tamamlananlar, sıradaki adımlar ve açık kararlar.
- [lessons.md](lessons.md) — tekrar edilmemesi gereken teknik ve epistemik dersler (append-only).
- [README.md](README.md) — 3 katmanlı bellek modeli ve okuma/yazma kuralları.
- `decisions/` — Mimari Karar Kayıtları (ADR; append-only).
- `specs/` — Çekirdek motor sözleşmeleri (zaman modeli, render grafiği, efekt/shader API, proje şeması).
- `research/` — Birincil kaynaklı teknik araştırmalar ve kök araştırma belgelerine köprü.
- `style-cards/` — Referans editlerden çıkarılan görsel stil, kurgu dili ve efekt reçetesi kartları ([Hızlı Seçim Matrisi](style-cards/README.md)).
- `log/` — Oturum günlükleri (append-only).
- `_templates/` — Yeni karar, araştırma, stil kartı, günlük ve spec şablonları.

---

## Etiket Sözlüğü (Tags)

`engine` · `timebase` · `render-graph` · `shaders` · `media-io` · `color` · `text-vector` · `schema` · `agent` · `inspection` · `ui` · `licensing`

---

## Decisions (Mimari Karar Kayıtları)

| ID | Başlık | Durum | Tarih | Etiketler |
| :--- | :--- | :--- | :--- | :--- |
| [0001](decisions/0001-independent-native-gpu-core.md) | Bağımsız Native GPU Çekirdek ve Masaüstü + Headless Sınırı | `accepted` | 2026-11-20 | `engine`, `shaders`, `ui`, `agent` |
| [0002](decisions/0002-rational-timebase-flicks.md) | Kesin Zaman Tabanı (Flicks / Rasyonel Tam Sayı Tick) | `accepted` | 2026-11-21 | `timebase`, `engine`, `schema`, `media-io` |
| [0003](decisions/0003-semantic-event-constraint-timing.md) | Anlamsal Edit ve Olay/Kısıt Tabanlı Zamanlama Ayrımı | `accepted` | 2026-11-20 | `agent`, `timebase`, `schema`, `inspection` |

---

## Specs (Çekirdek Motor Sözleşmeleri)

| Belge | Alan | Durum | Notlar |
| :--- | :--- | :--- | :--- |
| [specs/README.md](specs/README.md) | Çekirdek Sözleşme Haritası | `active` | Zaman modeli, render grafiği, efekt API, inceleme ve proje şeması hedefleri |
| [specs/timebase.md](specs/timebase.md) | Zaman Tabanı ve Rasyonel Zaman | `locked` | `Flicks(i64)` (`705_600_000/sn`), `Rational32`, `i128` ara aritmetik, VFR PTS, ses retime |
| [specs/project-schema.md](specs/project-schema.md) | Sürümlenmiş Proje Şeması | `locked` | `schema_version`, `revision_id`, M1 çekirdek + Faz 2–5 (`VectorText`, `Lut3D`, `ParamBindingDef`, `CreativeDecisionRecord`, `PreferenceEntry`, `ProxyConfigDef`, `HwAccelPolicy`) |
| [specs/render-graph.md](specs/render-graph.md) | Render Grafiği ve GPU Kompozisyon | `locked` | `Rgba16Float` premultiplied, `sRGB <-> Linear Rec.709`, motion blur, `VectorText`, `Lut3D`, seek-independent DAG |
| [specs/effect-shader-api.md](specs/effect-shader-api.md) | Efekt, Shader ve `SafeExpr` API | `locked` | `@group(0)` bağlama düzeni, 16-byte uniform, yakıt sınırlı `SafeExpr`, Naga + TDR döngü güvenliği |
| [specs/inspection-contract.md](specs/inspection-contract.md) | İnceleme Yüzeyleri ve Epistemik Sınır | `locked` | Ortak `revision_id` + `tick`, 4 yüzey, Contact Sheet, 4-panel MaskPack, 3-panel TransitionPack, VelocityPack, FullResCrop |

---

## Research (Araştırma Kayıtları)

| Belge | Konu | Durum | Konum |
| :--- | :--- | :--- | :--- |
| [research/README.md](research/README.md) | Araştırma Standardı ve Köprü | `active` | Araştırma kuralları ve kök belgelere bağlantı |
| [research/open-source-shader-transitions.md](research/open-source-shader-transitions.md) | Açık Kaynak Shader Geçişleri (WGSL Şartnamesi) | `active` | `gl-transitions` MIT/BSD analizi ve FrameShift WGSL geçiş uniform şartnamesi |
| [research/anime-amv-postfx-shaders.md](research/anime-amv-postfx-shaders.md) | Anime / AMV 2D Post-İşleme Shader Aileleri | `active` | Motion Tile+Shake, Impact Frame+Speed Lines, Radial Shockwave, Halftone+Glow (MIT/CC0) |
| [research/3d-kinetic-typography-architecture.md](research/3d-kinetic-typography-architecture.md) | 3D Kinetik Tipografi ve Motion Graphics Mimarisi | `active` | Eksik 6 alan ve en iyi çözümler (`glam`, `ttf-parser`+`lyon`, Jimenez Bloom, Bokeh DoF, stateless parçacık/ribbon, `Demucs`+`stable-ts`) |
| [research/two-tier-extensible-effect-architecture.md](research/two-tier-extensible-effect-architecture.md) | İki Katmanlı Genişletilebilir Efekt Mimarisi | `active` | Çekirdek yapı taşları + Küratörlü Açık Kaynak WGSL Kütüphanesi + Runtime `CustomWgsl` Sandbox |
| [research/2026-11-21-edit-friction.md](research/2026-11-21-edit-friction.md) | Edit sürtünmesi: kesim, beat, ifade, yazı, grade, solo, fit, CLI | `active` | Uygulama duruyor. 2026-11-21 doğrulamasında workspace test, clippy ve `verify-all` geçti. Glitch self-blend, LUT siyahı, font dosyası, solo ve waveform bu turda yok |
| [research/2026-11-22-asta-v3-uretken-darbogazlari.md](research/2026-11-22-asta-v3-uretken-darbogazlari.md) | Asta v3 üretim engelleri ve eksik özellikler | `active` | 5 gerçek eksik + 5 "keşke" + 3 araç keşfi; P0: `probe`/`sheet` + `render --range` |
| [research/faz6-3-scene3d-dof-camera.md](research/faz6-3-scene3d-dof-camera.md) | Faz 6.3 Part A — Paylaşımlı `Scene3D`, `Camera3D`, `Depth32Float` Bokeh DoF & `SafeExpr` Easing | `active` | 9 birincil URL doğrulamalı; `wgpu 30.0.1` depth sampling, 2 geçişli Vogel Disk DoF, `LookAt`+roll kamera, 5 `SafeExpr` easing |
| [research/faz6-3-text3d-shapes-ribbons.md](research/faz6-3-text3d-shapes-ribbons.md) | Faz 6.3 Part B — `Text3D` Kinetik/Kontur/Chrome/Echo, `Shape3D` ve 3'lü Paralel Şeritler | `active` | `lyon StrokeTessellator` (`face_kind=3u`), 5 prosedürel `Shape3D` primitifi, 416B `Text3DUniform`, Bishop parallel-transport şerit |
| [research/streaming-video-decoder.md](research/streaming-video-decoder.md) | Sürekli Video Decoder (`StreamingVideoDecoder` & `DecoderPool`) | `active` | 0 yeni crate ile sürekli açık `Child` rawvideo pipe, `MAX_OPEN_STREAMS=8`, `MAX_SEQUENTIAL_SKIP=32`, benchmark verileri |
| [research/cube-lut-and-ocio.md](research/cube-lut-and-ocio.md) | Harici `.cube` (1D+3D) & Saf Rust `OcioColorPipeline` | `active` | Adobe `.cube`, `.spi1d`/`.spi3d`, `.ocio` YAML ayrıştırıcı, `f64` CIE/Bradford/ACES/LogC3/SLog3 matrisleri |
| [research/2d-collage-precomp-hud-warp.md](research/2d-collage-precomp-hud-warp.md) | 2D Kolaj, Precomp, HUD & Deformasyon (`SourceImage`, `Precomp`, `VectorPathHud`, `CornerPin`, `GridWarp`) | `active` | `png`+`ffmpeg` image, kapsüllü `PrecompInput`, `lyon`+SDF HUD, Heckbert homografi & 8×8 `GridWarp` |
| [research/2d-vectortext-ttf-layout.md](research/2d-vectortext-ttf-layout.md) | 2D `VectorText` Gerçek TTF/OTF Font, Çok Satır & Hizalama | `active` | `font_locator` (`FontNotFound`), `NonZero` 4×4 SSAA scanline, `TextAlign2D`, M1 golden hash koruması |
| [research/2026-11-22-kinetic-typography-gap-analysis.md](research/2026-11-22-kinetic-typography-gap-analysis.md) | SC-0002 Yeniden Üretimi — Eksik Yetenek ve Yol Haritası | `active` | Harf bazlı yerleşim, depth/matte, tempo, kırpma/panel ve SafeExpr boşlukları (kanıt satırlarıyla) |
| [research/2026-11-22-frameshift-tuzaklar.md](research/2026-11-22-frameshift-tuzaklar.md) | FrameShift Tuzakları — Üretimden Çıkarılan Dersler | `active` | Söz–yazı senkronu, ofset birimleri, sRGB/lineer, opak taban, siyah kaynak, luma maskesi, SafeExpr sınırları |
| [research/typography-nextgen-architecture.md](research/typography-nextgen-architecture.md) | Faz 6.5 — HarfRust + Skrifa Shaping, WGSL Slug GPU Bézier & SIL OFL Font Paketi | `active` | `harfrust 0.16.1`, `skrifa 0.50.0`, `EricLengyel/Slug` (MIT/Public Domain), Tategaki, `BevelProfile3D`, 13 SIL OFL font |
| [research/faz6-6-optics-tonemap-aa.md](research/faz6-6-optics-tonemap-aa.md) | Faz 6.6 Part A — Optik, AgX/PBR Neutral Tonemap, TPDF Dithering, 4x MSAA/FXAA & Mipmaps | `active` | `AgX`/`PbrNeutral`/`AgxPunchy`, TPDF/IGN dither, `4x MSAA`, FXAA, separable Halation, 35mm grain, spektral CA, anamorfik DeepGlow |
| [research/faz6-6-scene3d-pbr-dof-blend.md](research/faz6-6-scene3d-pbr-dof-blend.md) | Faz 6.6 Part B — 3D PBR Cook-Torrance GGX, Kamera-Bağıl IBL, Scene3D DoF & Blend Modları | `active` | Filament GGX PBR, bevel AO, Z-echo depth ayrımı, separable CoC + poligonal/anamorfik bokeh, algısal Overlay + 5 yeni blend modu |
| [research/faz6-6-fx-transitions-sdf.md](research/faz6-6-fx-transitions-sdf.md) | Faz 6.6 Part C — Shockwave, 10 Aspect-Correct Geçiş, Halftone & 2-Yönlü JFA SDF | `active` | Sürekli Gauss shockwave, 10 geçiş aspect-ratio + IGN blur + FBM Burn + KIFS Kaleidoscope, CMYK/ColorDot halftone, `Rgba16Float` JFA SDF |
| [FrameShift ürün geliştirme yol haritası](research/2026-11-22-frameshift-urun-gelistirme-yol-haritasi.md) | HyperFrames, OpenMotion ve After Effects’ten Ürün Dersleri | `draft` | P0 canlı kurgu/ajan/export, P1 hız grafiği/tipografi/matte/precomp, kabul eşikleri |
| [2026-11-24-social-anime-editors-workflow.md](research/2026-11-24-social-anime-editors-workflow.md) | TikTok, Instagram ve YouTube Ünlü Anime Editörlerinin İş Akışı | `active` | Xenoz/Molob/Nep ekolleri, 5 stil, 6 adımlı AE/Blender/SFX iş akışı ve FrameShift boşluk eşleşmesi |
| [2026-11-24-anime-workflow-gap-audit.md](research/2026-11-24-anime-workflow-gap-audit.md) | Sosyal Medya Anime Editör İş Akışı vs FrameShift Boşluk Denetimi | `active` | 6 adımlı swarm denetimi: medya/RIFE, ses/SFX, Speed Graph, Null3D/matte/RSMB, Post-FX/tipografi, export/preset/MCP P0–P2 planı |
| [2026-11-24-ai-rife-ort-rotoscope-guide.md](research/2026-11-24-ai-rife-ort-rotoscope-guide.md) | AI Medya Araçları Rehberi: RIFE, `ort` ve ONNX Rotoscope | `active` | `rife-ncnn-vulkan` (Twixtor), `ort` (Rust ONNX), `anime-segmentation`/`BiRefNet`/`SAM 2`/`Depth-Anything-V2-Small` lisans ve entegrasyon rehberi |
| [2026-11-24-optical-flow-and-ai-inference-architecture.md](research/2026-11-24-optical-flow-and-ai-inference-architecture.md) | Optik Akış (Pyramidal GPU RSMB), İnterpolasyon & ONNX Sidecar Mimarisi | `active` | 4 seviyeli WGSL Lucas-Kanade/Horn-Schunck, `ClipAnalysis` dedup+SCD interpolasyon, raw-pipe `onnxruntime` sidecar protokolü |
| [2026-11-24-multiband-spectral-flux-downbeat.md](research/2026-11-24-multiband-spectral-flux-downbeat.md) | Çok-Bantlı Spectral Flux + 4/4 Downbeat Tespiti | `active` | 0-crate saf Rust Radix-2 FFT (`N=4096`), Sub/Mid/High flux, Ellis tempo + 4/4 downbeat, `MultibandBeatReport` & `SafeExpr` uniformları |
| [2026-11-24-ai-stem-separation-lyric-alignment.md](research/2026-11-24-ai-stem-separation-lyric-alignment.md) | AI Stem Ayrımı & Kelime-Düzeyi Söz Zamanlama | `active` | Py 3.12 izole `uv venv` (`local_media/.venv-audio`), `audio-separator` (MDX-Net/HTDemucs) + hibrit `stable-ts`/`faster-whisper` + `ai_audio_sidecar.py` |
| [2026-11-24-real-waveform-png-rendering.md](research/2026-11-24-real-waveform-png-rendering.md) | Gerçek PCM Waveform PNG Çıktısı & `snapshot.rs` Entegrasyonu | `active` | 0-crate `WaveformPyramid` + Dual-Layer Peak/RMS & 3-bant spektral rasterizer (`frameshift-media/src/waveform.rs`), CLI `waveform` & `render_waveform` MCP |
| [2026-11-24-anime4k-wgsl-and-realesrgan-upscaler.md](research/2026-11-24-anime4k-wgsl-and-realesrgan-upscaler.md) | Anime4K Tam WGSL Zinciri & `realesrgan-ncnn-vulkan` Upscaler | `active` | 0-crate Anime4K v4.x (`Clamp_Highlights`/`Restore_CNN`/`Upscale_CNN_x2`/`Darken`/`Thin`) sRGB köprülü WGSL + `realesrgan-ncnn-vulkan v0.2.5.0` batch + dedup sürücüsü |
| [2026-11-24-depth-anything-v2-scene3d-injection.md](research/2026-11-24-depth-anything-v2-scene3d-injection.md) | `Depth-Anything-V2-Small` ONNX & `Scene3D` `Depth32Float` Enjeksiyonu | `active` | Yerel `depth_anything_v2_small.onnx` (83ms sıcak DML), 16-bit/RG16 derinlik kanalı, `fx_dof_coc` alpha-void düzeltmesi, disparity Z ve graph/precomp kenar kapanışı |
| [open-source-fx-catalog-and-roadmap.md](research/open-source-fx-catalog-and-roadmap.md) | Açık Kaynak FX Kataloğu ve Yol Haritası | `active` | Faz 6.1+ için açık kaynak shader/efekt kataloğu ve önceliklendirme |
| [2026-11-24-competitive-power-analysis-roadmap.md](research/2026-11-24-competitive-power-analysis-roadmap.md) | Rakip Güç Analizi ve Sıradaki Yüksek Getirili Özellikler | `partially-implemented` | OpenMotion/Kimo/FilmCraft/Resolve/AE/AviUtl incelemesi (P1/P2/P3/P5 uygulandı, P4 ertelendi) |
| [2026-11-24-power-p1-creative-engine.md](research/2026-11-24-power-p1-creative-engine.md) | P1: Yaratıcı Motor & `SafeExpr` Mimarisi | `implemented` | `SafeExpr` (`spring`/`wiggle`/operatörler), `DisplacementMap`, `FrameEcho`, `ShapeMask` morph, ADR-0003 Kahn DAG olay kısıtları (0 crate) |
| [2026-11-24-power-p2-agent-dsl-linter.md](research/2026-11-24-power-p2-agent-dsl-linter.md) | P2: Ajan Linter, `diff_frame`, `Motion.md` DSL & Parametrik Şablonlar | `implemented` | 10 kurallı `lint_project`, 3-panelli `diff_frame`, deklaratif DSL derleyici ve `ParametricTemplateSpec` tasarımı (0 crate) |
| [2026-11-24-power-p3-gpu-yuv-smart-cache.md](research/2026-11-24-power-p3-gpu-yuv-smart-cache.md) | P3: GPU NV12→Linear Rec.709 & Düğüm Seviyesi Smart Cache | `implemented` | 12-bpp NV12 + `yuv_to_linear.wgsl` (%62,5 bant genişliği düşüşü) ve 3-katmanlı `NodeContentHash` önbellek tasarımı (0 crate) |
| [2026-11-24-power-p4-live-desktop-shell.md](research/2026-11-24-power-p4-live-desktop-shell.md) | P4: İnce Canlı Masaüstü Önizleme Kabuğu (`egui 0.37` + `winit 0.30`) | `deferred` | `wgpu 30` uyumlu sıfır-kopya `frameshift-gui` tasarımı (kullanıcı kararıyla sonraki faza bırakıldı) |
| [2026-11-24-power-p5-tracker-otio-ai-manifest.md](research/2026-11-24-power-p5-tracker-otio-ai-manifest.md) | P5: 2D/Planar Tracker, `.otio`/FCPXML & `model_manifest.json` AI Yuvası | `implemented` | Pyramidal Lucas-Kanade ROI tracker + `keyframe_tracks`, 0-crate NLE köprüsü ve SHA-256 `Strict` AI manifest tasarımı |
| `IMPLEMENTATION_PLAN.md` | Faz 0–5 Detaylı Implementasyon Planı | `active-plan` | [Kök belgede](../IMPLEMENTATION_PLAN.md) |
| `ENGINE_ALTERNATIVES_RESEARCH.md` | Motor ve Araç Alternatifleri | `archived-ref` | [Kök belgede](../ENGINE_ALTERNATIVES_RESEARCH.md) |
| `VISUAL_REFERENCES.md` | Görsel Referans Kare Gözlemleri | `active-ref` | [Kök belgede](../VISUAL_REFERENCES.md) |

---

## Asta v3 — 2026-11-22 Araştırma ve Uygulama

Asta v3 üretimindeki darboğazları ele alan tur: 11 araştırma raporu, ADR-0004 ve takım günlüğü. Raporlar `status: draft`; kararlar ve uygulama durumu günlükte izlenir. P0 streaming encode ölçümü: **6.5 GB → 0.27–0.80 GB** (O(N) tampon kaldırıldı; tepe LRU önbelleğiyle sınırlı); Fame v3 720p 1920 karede **0.27 GB** / private **0.91 GB** / **100.2 sn**, asta_amv draft 720p **0.80 GB**, asta_v3 draft 640×360 **0.40 GB**. Ayrıntılı ölçüm: [takım günlüğü](log/2026-11-22-asta-v3-team.md).

| Belge | Tür | Konu / Karar | Durum |
| :--- | :--- | :--- | :--- |
| [2026-11-22-render-range.md](research/2026-11-22-render-range.md) | Araştırma | `render --range`: drop penceresi için aralık render'ı; `RenderRange`, dışa snap, ses dilimi, summary'de opsiyonel `range` | `draft` |
| [2026-11-22-render-progress-eta.md](research/2026-11-22-render-progress-eta.md) | Araştırma | Render ilerleme/ETA: sessiz kare döngüsüne gözlemlenebilirlik; stderr, callback API, EMA'lı ETA | `draft` |
| [2026-11-22-probe-sheet-multiframe.md](research/2026-11-22-probe-sheet-multiframe.md) | Araştırma | Probe/Sheet: tek süreçte N kare; tek Engine, `sheet` komutu, manifest + isteğe bağlı tam çözünürlük | `draft` |
| [2026-11-22-material-swatch-gallery.md](research/2026-11-22-material-swatch-gallery.md) | Araştırma | Şekil/malzeme swatch galerisi; `diamond_3d` varsayılan `inner_ratio` yıldız, `material_mode 0` beyaz; ayrı `materials` komutu | `draft` |
| [2026-11-22-cli-help-discovery.md](research/2026-11-22-cli-help-discovery.md) | Araştırma | CLI keşfedilebilirlik: `frame --help` exit 1, `help <cmd>`/`--version`, komut tablosu ve `AGENTS.md` senkronu | `draft` |
| [2026-11-22-autocut.md](research/2026-11-22-autocut.md) | Araştırma | `autocut`: peaks tespitini kesim gridine bağlama; deterministik planlayıcı, dry-run JSON, v1 yalnız hard cut | `draft` |
| [2026-11-22-particle-burst-binding.md](research/2026-11-22-particle-burst-binding.md) | Araştırma | Particle burst/emission: Seçenek **B** (burst alt-yapısı + `emission`); `count`/`seed` statik, zaman `Flicks` | `draft` |
| [2026-11-22-smart-reframe-916.md](research/2026-11-22-smart-reframe-916.md) | Araştırma | 9:16 akıllı reframe: statik odak v1, çıktı profili override + `SourceVideo.focus`, cover crop | `draft` |
| [2026-11-22-audio-fade-gain-automation.md](research/2026-11-22-audio-fade-gain-automation.md) | Araştırma | Audio fade/gain: fade-only v1, `AudioFadeDef`, per-sample çarpan, `gain_db` taban | `draft` |
| [2026-11-22-ribbon-color-thickness.md](research/2026-11-22-ribbon-color-thickness.md) | Araştırma | `RibbonTrail3D` renk/kalınlık: `wgsl:265` beyaz karışımı; varsayılan düzeltme, `min_width_px` önerisi | `draft` |
| [2026-11-22-text-scrim-backplate.md](research/2026-11-22-text-scrim-backplate.md) | Araştırma | Metin arkası scrim: 1 girişli `BackdropScrim` + yarı çözünürlük blur; `VectorText` arity'si korunur | `draft` |
| [0004-schema-minor-compatibility.md](decisions/0004-schema-minor-compatibility.md) | ADR | Şema sürümünde minor geriye uyumluluğu (`0.2.0`); yükleme kuralı `minor <= current` | `accepted` |
| [2026-11-22-asta-v3-team.md](log/2026-11-22-asta-v3-team.md) | Günlük | Asta v3 takım günlüğü: araştırma, kararlar, P0 ölçümü ve planlanan dalgalar | `devam ediyor` |

---

## Log (Oturum Günlükleri)

| Tarih | Başlık | Özet |
| :--- | :--- | :--- |
| [2026-11-25](log/2026-11-25-lufas-maphaahl-kesha-blow.md) | Lufas Maphaahl × Kesha Blow (`SC-0006`) 1080×1080 60fps Editi | `@_okotcu` referansı incelenip `SC-0006` kaydedildi; 15 temiz 4K/1080p kesit, `107.5 BPM` / `4.88s` drop, `OpticalFlowBlur` + `DeepGlow` + `FilmOptics` ile 990 karelik `lufas_blow_final.mp4` üretildi ve doğrulandı. |
| [2026-11-25](log/2026-11-25-anime-clip-sources.md) | Anime & Raw Clip Arama Kaynaklarının Belgelenmesi | Ajanların klip ararken sırayla kullanacağı 5 kaynak (Sakugabooru, AnimeClipsRaw, RingWitDaTwixtor, Hii Twixtor, 411 Scenepacks) kök `CLAUDE.md` içine eklendi. |
| [2026-11-24](log/2026-11-24-competitive-power-roadmap-implementation.md) | Rakip Güç Yol Haritası — P1, P2, P3 & P5 İmplementasyonu | 5 paralel araştırma raporu + 5 dalgalı swarm ile 0 yeni crate, 60 WGSL (+4 yeni), `SafeExpr` fizik/gürültü, `DisplacementMap`/`FrameEcho`/`ShapeMask`, ADR-0003 kısıtlar, `lint_project`/`diff_frame`/`Motion.md` DSL/parametrik şablon, GPU NV12 + 3-katmanlı Smart Cache, 2D/Planar Tracker, `.otio`/FCPXML ve `model_manifest.json`; clippy 0 uyarı, 452/453 test (1 önceden mevcut HEAD hatası) ve canlı CLI smoke PASS. |
| [2026-11-24](log/2026-11-24-five-pillar-implementation.md) | Beş Temel Yetenek Yükseltmesi (Spectral, AI Stem/Lyrics, Waveform PNG, Anime4K/Real-ESRGAN & Depth/Scene3D) | 0 yeni crate ile 52 WGSL (8 Anime4K), saf Rust Radix-2 FFT + downbeat, `.venv-audio` CUDA stem/lyrics, gerçek waveform PNG + `snapshot.rs`, `realesrgan-ncnn-vulkan v0.2.5.0`, 16-bit/RG16 depth + DoF void düzeltmesi ve CLI/MCP entegrasyonu; clippy 0 uyarı, 415/416 test geçti (1 önceden mevcut HEAD hatası). |
| [2026-11-24](log/2026-11-24-optical-flow-and-ai-inference.md) | Native GPU Optik Akış (`OpticalFlowBlur`), İnterpolasyon & ONNX Sidecar | 0 yeni crate ile 44 WGSL (`OpticalFlowBlur` Pyramidal RSMB), `interpolator.rs`, `ai_infer_sidecar.py` + `ai_inference.rs`, CLI/MCP araçları; clippy 0 uyarı ve tüm hedefli testler PASS. |
| [2026-11-24](log/2026-11-24-anime-workflow-p0-implementation.md) | Anime Edit İş Akışı Boşluk Denetimi ve P0 Implementasyonu | 0 yeni crate ile 9 crate'te SpeedGraph/AnimeSnap, platform export, `BlurMoCurves`/`AdjustmentLayer`/`Null3D`/`DepthMatte`, `TextAnimator` v2, ses limiter/bus ve 6 yeni MCP aracı; `clippy` 0 uyarı. |
| [2026-11-24](log/2026-11-24-social-anime-editors-research.md) | Sosyal Medya Ünlü Anime Editörleri Araştırması | Xenoz/Molob/Nep iş akışı, 5 anime edit stili araştırma raporu ve SC-0004 / SC-0005 arketip stil kartları eklendi. |
| [2026-11-22](log/2026-11-22-github-publication.md) | GitHub publication checkpoint | Existing workspace publication and two known `autocut` test failures. |
| [2026-11-20](log/2026-11-20-brain-ve-agents-kurulumu.md) | `brain/` ve `AGENTS.md` Kurulumu | Kalıcı bellek, ilk ADR'lar ve kurumsal ajan kuralları oluşturuldu |
| [2026-11-21](log/2026-11-21-implementasyon-plani.md) | Detaylı Implementasyon Planı Hazırlığı | Faz 0–5 yol haritası, crate mimarisi ve risk matrisi kullanıcı onayına sunuldu |
| [2026-11-21](log/2026-11-21-faz0-ve-m1-golden-slice.md) | Faz 0 Sözleşmeleri ve Faz 1 (M1) Golden Vertical Slice | Faz 0 spec kilidi, 6 Cargo crate'i, 7 WGSL shader ve 5 M1 doğrulama kapısı geçildi |
| [2026-11-21](log/2026-11-21-faz2-faz5-tam-implementasyon.md) | Faz 2–5 (M2–M5) Tam İmplementasyon ve Doğrulama | 9 Cargo crate'i, 8 WGSL shader, 54/54 test, MCP sunucusu, inceleme paketleri ve UI snapshot doğrulandı |
| [2026-11-21](log/2026-11-21-anime-fx-arastirma-ve-15-wgsl-uyarlama.md) | Açık Kaynak Anime FX Araştırması ve 15 WGSL Shader Uyarlaması | 2 birincil kaynak araştırma `.md` raporu, 15 WGSL shader, 58/58 test ve 8 panelli `fx_showcase_pack.png` doğrulandı |
| [2026-11-21](log/2026-11-21-gercek-asta-amv-denemesi.md) | Gerçek Asta AMV Medyası ile Uçtan Uca Deneme | 5 gerçek Asta klibi + 48 kHz müzikle 11 sn MP4 (`asta_amv_draft.mp4`) renderlandı; performans/UX darboğazları raporlandı |
| [2026-11-21](log/2026-11-21-swarm-performans-ve-motor-iyilestirmeleri.md) | Swarm ile 6 Prodüksiyon Darboğazının Araştırılması ve Çözümü | `decode_video_window` (3.42x hızlanma: 218.6s -> 63.9s), gerçek `AudioRmsEnvelope`, `hold_after`, `opaque_black_outside`, 2 sütunlu UI snapshot ve analitik SDF tipografi (**70/70 test**) |
| [2026-11-21](log/2026-11-21-3d-kinetik-tipografi-arastirmasi.md) | 3D Kinetik Tipografi Referans İncelemesi ve Mimari Araştırma | Referans TikTok videosu incelendi; eksik 6 teknik alan ve birincil kaynaklı çözümler `brain/research/3d-kinetic-typography-architecture.md` içine kaydedildi |
| [2026-11-21](log/2026-11-21-faz6-1-visual-fx-ve-custom-wgsl.md) | Faz 6.1 — DeepGlow, AnimeSharpen, FilmOptics, 6 Geçiş ve Runtime CustomWgsl | Swarm ile 10 yeni WGSL (toplam 25), 4 yeni `NodeKind`, mip-pyramid DeepGlow ve Naga doğrulamalı runtime `CustomWgsl` (**98/98 test**, `verify-all` PASS, `faz6_1_showcase.mp4`) |
| [2026-11-21](log/2026-11-21-faz6-2-3d-kinetik-tipografi-ve-parcaciklar.md) | Faz 6.2 — 3D Kamera, TTF/OTF 3D Extrusion, Parçacıklar & Neon Şeritler | Swarm ile `glam` + `ttf-parser` + `lyon_tessellation`, 4 yeni 3D WGSL (toplam 29), `Transform3D`/`Text3D`/`ParticleField3D`/`RibbonTrail3D` (**107/107 test**, `verify-all` PASS, `faz6_2_kinetic3d_showcase.mp4`) |
| [2026-11-21](log/2026-11-21-koleedits-dogfood.md) | Koleedits TikTok benzeri edit dogfood | Referans 16.55 sn HEVC izlendi ve ses zarfı çıkarıldı; ayrı WW klipleri + 8.4 sn trailer cue ile 960×740 draft MP4 renderlandı. Contact sheet senkron kanıtı değil. |
| [2026-11-21](log/2026-11-21-edit-friction-research.md) | Edit sürtünmesi araştırması | Sekiz boşluk kod, kilitli spec ve dış birincil kaynaklarla eşlendi. Uygulama başlamadı. |
| [2026-11-21](log/2026-11-21-edit-friction-fixes.md) | Edit sürtünmesi uygulaması | Implementation of the edit-friction work landed. `verify-all` çalıştırılmadı. Per-node fit: aynı varlığı paylaşan iki `SourceVideo` kendi `SourceFit` karesini alır. |
| [2026-11-21](log/2026-11-21-edit-friction-verify.md) | Edit sürtünmesi doğrulaması | `CARGO_TARGET_DIR=target/friction-verify`: workspace test geçti (91/0), clippy geçti, `verify-all` geçti. Blur kapısı 8 örneği 1 tavana indirdi, hash değişti, zaman çıtaları aynı kaldı. |
| [2026-11-21](log/2026-11-21-asta-devil-union-full-edit.md) | Asta // Anti-Magic Devil Union Tam Özellikli Edit | 10 küratörlü Asta klibi + `bgm_revenge` drop kesiti, 43 düğüm, 9 WGSL geçişi, 4 adet 3D extruded TTF başlık, 3D parçacık/şerit, DeepGlow, AnimeSharpen, FilmOptics ve CustomWgsl ile 15 sn 720p30 final MP4 (`68.2s` render). |
| [2026-11-22](log/2026-11-22-fame-is-a-gun-3d-kinetic-edit.md) | Addison Rae — Fame is a Gun (3D Kinetik Tipografi Editi) | 32 sn 60fps 720p (`1920` kare, `sub_samples=3`) 3D kinetik tipografi editi: 44 kelime-senkronlu 3D TTF söz kartı (4 font), 3D yıldız/elmas/kalp parçacıkları, çift neon şerit, 2 özel HDR WGSL shader, shockwave/impact/DeepGlow/FilmOptics (`131.87s` render). |
| [2026-11-22](log/2026-11-22-faz6-3-platsae-3d-engine-ve-fame-v3.md) | Faz 6.3 — PlatsAE 3D Sahne Motoru & Fame is a Gun v3 | Swarm ile 0 yeni crate, 31 WGSL, paylaşımlı `Scene3D`, Z-DoF, `Text3D` kinetik/kontur/chrome/echo, 5 `Shape3D` primitifi, 3'lü paralel şeritler, 5 `SafeExpr` easing (**127/127 test**, `verify-all` PASS) ve `fame_is_a_gun_v3_platsae_final.mp4` doğrulandı. |
| [2026-11-22](log/2026-11-22-faz6-4-streaming-ocio-2d-pack-vectortext.md) | Faz 6.4 — Streaming Decoder, `.cube`/OCIO, 2D Kolaj/Precomp/HUD/Warp ve 2D TTF VectorText | Swarm ile 0 yeni crate, 33 WGSL, `DecoderPool` (`112.56s -> 70.53s`), 1D/3D `.cube` + saf Rust OCIO, `SourceImage`/`Precomp`/`VectorPathHud`/`CornerPin`/`GridWarp`, TTF `VectorText` & 30 `NodeKind` MCP senkronu (**177/177 test**, `clippy` 0 uyarı, `verify-all` PASS). |
| [2026-11-22](log/2026-11-22-faz6-5-nextgen-typography-slug.md) | Faz 6.5 — HarfRust + Skrifa Shaping, WGSL Slug GPU Bézier, Kavisli 3D Bevel & SIL OFL Font Paketi | `harfrust 0.16.1` + `skrifa 0.50.0`, WGSL Slug (`text_slug.wgsl` + `text2d_fx.wgsl`), `VerticalRl` Tategaki, `BevelProfile3D::{Round, Concave, Stepped}`, `material_mode=3` Iridescent Chrome & 13 SIL OFL font (`assets/fonts/`) (**187/187 test**, `clippy` 0 uyarı, `verify-all` PASS, `typography_showcase_65.png`). |
| [2026-11-22](log/2026-11-22-asta-v3-red-line.md) | Asta // Devil Union — Red Line (v3) 20 sn AMV | 28 kesim, 1080p60, 20.000 sn; drop tam 7.50 sn (RMS −19.1 → −12.7 dB), 3D tipografi + anime FX zinciri; `ffprobe` 1200 kare doğrulandı. |
| [2026-11-22](log/2026-11-22-faz6-6-shader-visual-upgrade.md) | Faz 6.6 — Shader Görsel Kalite Yükseltmesi (Optik, 3D PBR/DoF & Anime FX/Geçişler) | Swarm ile 0 yeni crate, 41 WGSL shader: `AgX`/`PbrNeutral`/`AgxPunchy` + TPDF dithering + `4x MSAA`/FXAA/mipmap + separable Halation/35mm grain/spektral CA + anamorfik DeepGlow + 3D Cook-Torrance PBR/IBL/Bevel AO + separable CoC/poligonal DoF + 5 yeni Blend + sürekli Shockwave + 10 aspect-correct IGN-blur geçiş + 2-yönlü JFA SDF (**199/199 test**, `clippy` 0 uyarı, `verify-all` M1–M5 PASS). |
| [2026-11-22](log/2026-11-22-hyperframes-openmotion-frameshift-karsilastirma.md) | HyperFrames, OpenMotion ve FrameShift | Motor/ürün olgunluğu, 1080p60 MP4 yeniden kontrolü ve güncel iki `autocut` test hatası. |
| [2026-11-22](log/2026-11-22-frameshift-urun-gelistirme-yol-haritasi.md) | Rakiplerden ürün geliştirme dersleri | HyperFrames, OpenMotion ve After Effects kaynaklı P0–P2 özellikler ve ölçüm planı. |
| [2026-11-22](log/2026-11-22-yol-haritasi-uygulama-arastirmasi.md) | Yol haritası uygulama araştırması | Temiz klon test engeli, autocut aralığı, timeline ve export uygulama sınırları, kullanıcı kararları. |
| [2026-11-22](log/2026-11-22-headless-roadmap-implementation.md) | Headless P0–P2 ürün yol haritası uygulaması | 4 sn Impact Cut, CLI smoke'ları, MCP fişleri, export/proxy/schema kontrolleri; workspace acceptance 318 geçti, 6 baseline testi filtrelendi; `verify-all` lavapipe timeout. |
| [2026-11-22](log/2026-11-22-tiktok-fx-edit-referansi.md) | Efekt odaklı TikTok anime edit hedefi | SC-0003 teknik ön ölçümleri ve mevcut efektlerle kısa üretim deneyi önerisi. |
| [2026-11-23](log/2026-11-23-sol-levante-afterlight.md) | Sol Levante / Afterlight anime editi | 23 plan, 4,25 sn impact, 18,583 sn ana MP4, düzenlenebilir proje ve doğrulanmış tam FrameShift draft. |
| [2026-11-23](log/2026-11-23-afterlight-referans-kurgusu.md) | Afterlight V2 referans kurgusu | Curse, zwxeditor ve koleedits kare incelemesi. Karakter motifi, 7:6 kadraj ve siyah beklemeli 12,125 sn çıktı. |
| [2026-11-23](log/2026-11-23-chainsaw-man-blow.md) | Chainsaw Man × Blow | Denji dönüşümüne bağlı bass drop, 7:6, tipografi ve vuruşa bağlı efektler. |

---

## Şablonlar (`_templates/`)

- [karar.md](_templates/karar.md) — yeni ADR (`NNNN-kisa-ad.md`)
- [spec.md](_templates/spec.md) — yeni çekirdek motor sözleşmesi
- [arastirma.md](_templates/arastirma.md) — yeni teknik araştırma raporu
- [stil-karti.md](_templates/stil-karti.md) — yeni referans edit stil kartı (`SC-NNNN-kisa-ad.md`)
- [gunluk.md](_templates/gunluk.md) — yeni oturum günlüğü (`YYYY-MM-DD-konu.md`)
