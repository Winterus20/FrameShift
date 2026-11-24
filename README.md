# FrameShift

**Agent-first video editing and compositing, built around an independent native GPU engine.**

FrameShift, mevcut video klipleri, görseller ve seslerden programatik olarak düzenlenebilir editler oluşturmayı hedefleyen bağımsız native GPU kompozisyon motoru, headless CLI/MCP platformu ve masaüstü uygulama çekirdeğidir. İlk görsel odak anime editleri, AMV ve yoğun motion graphics içeren kısa videolardır.

> **Durum (2026-11-24):** 9 Rust crate'i, **60 WGSL shader**, **40 `NodeKind`** DAG düğüm tipi (`schema_version: 0.2.0`), **50 CLI alt komutu** ve **32 MCP aracı** depoda yer alıyor. Faz 0–6.6, Anime P0, GPU `OpticalFlowBlur` + ONNX/RIFE sidecar'ı, 5 Temel Yetenek (çok-bantlı spectral flux, AI stem/lyrics, waveform PNG, Anime4K/Real-ESRGAN, Depth-Anything-V2 `Scene3D`) ve P1/P2/P3/P5 sütunları implemente edildi. Güncel kapsam ve doğrulamalar [proje durumunda](brain/state.md) kayıtlıdır.

`frameshift-app` ortak oturum, timeline projeksiyonu, komut denetleyicisi, arka plan iş yöneticisi ve `ui-snapshot` çıktısını sağlar; etkileşimli canlı masaüstü GUI penceresi (**P4**, `egui`/`winit`) kullanıcı kararıyla sonraki faza bırakılmıştır.

## Headless hızlı başlangıç

Rust, FFmpeg/ffprobe ve bir wgpu adaptörüyle sentetik Impact Cut akışını veya `Motion.md` DSL derlemesini çalıştır:

```sh
cargo build --locked -p frameshift-cli
target/debug/frameshift-cli init-impact --out target/impact.json
target/debug/frameshift-cli preflight target/impact.json
target/debug/frameshift-cli lint --project target/impact.json
target/debug/frameshift-cli timeline target/impact.json
target/debug/frameshift-cli autocut target/impact.json --range 1s..9s
target/debug/frameshift-cli retarget target/impact.json drop_impact 4.25s 1 --out target/retimed.json
target/debug/frameshift-cli speed-graph target/retimed.json clip_a 17
target/debug/frameshift-cli export target/retimed.json target/impact.mp4 --profile draft --range 2s..6s
```

`retarget` hedef vuruşu ve klibin kaynak-zaman eşlemesini aynı revizyonda günceller. `autocut` vuruşlardan (isteğe bağlı `--band all|sub|mid|high|beats|downbeats` filtresiyle) kesim noktaları önerir. `--write-events` işaretleri yazar; klip kesmek için `author_hard_cuts` komutu kullanılır.

- **Oturum, MCP (`32` araç) ve ajan kaydı.** `mcp-stdio [<project.json> --out <edited.json>]` JSON-RPC 2.0 üzerinden **32 MCP aracı** sunar (`describe_schema`, `mutate_project`, `undo`/`redo`, `lint_project`, `diff_frame`, `compile_motion_dsl`, `template_metadata`/`template_instantiate`, `track_point`/`track_planar`, `export_nle`/`import_nle`, `ai_models`/`ai_run_manifest`, `analyze_multiband_beats`, `render_waveform`, `separate_stems`, `align_lyrics`, `upscale_media`, `ai_capabilities` vb.). Geçerli `mutate_project` komutları `expected_revision_id` iyimser kilidiyle uygulanır; eski revizyon reddedilir. Her başarılı değişiklik önce/sonra yapısal anlık görüntüleri içeren fiş döndürür (`--out` ile sunucu kapandığında `.edits.json` yazılır). `edit-run <project.json> <actions.json> --out <edited.json>` insan/ajan adımları, undo/redo, isteğe bağlı önizleme ve export kuyruğunu tek headless oturumda yürütür.
- **Render, cache ve sabit export.** `render` ve `frame` komutları düğüm seviyesi `NodeContentHash` Smart Cache (`MemoryOnly` varsayılan + `--cache-dir` opt-in Disk LRU) ve `nv12`→Linear Rec.709 GPU yüklemesini destekler. `export` proje revizyonu ile medya/font/LUT bağımlılık baytlarını iş başında sabitler; `--platform` (`tiktok_1080p60`, `reels_1080p30`, `youtube_1080p60`, `anime_10bit_hevc`, `prores_422hq`) profillerini destekler, başarılı çıktıyı ffprobe ile doğrular ve `.export.json` raporu üretir. `export-set` birden fazla hedefi ve aynı süreç içi yerel yeniden denemeyi destekler.
- **DSL, şablonlar ve NLE köprüsü.** `compile-dsl` (`Motion.md` / `.fsmotion` → `0.2.0` proje + `lint` kapısı), `template-metadata` ve `template-instantiate` BPM/downbeat uyarlamalı `ParametricTemplateSpec` şablonlarını çalıştırır. `init-impact-template --out <package.json>` ve `template-open <package.json> <slots.json> --out <project.json>` yerel şablon paketini yönetir (paket grafikleri `CustomWgsl` içeremez). `export-nle` ve `import-nle` OpenTimelineIO (`.otio`) ve FCPXML 1.10/1.11 dönüşümü yapar.
- **İnceleme, efektler, tracking ve doğrulama.** `inspect`, `materials`, `ui-snapshot`, `lint` (`--check-media`), `diff-frame` (MAE/PSNR/bbox + 3-panelli PNG), `track-point`, `track-planar` (`Project.keyframe_tracks`), `timeline`, `speed-graph`, `snap`, `effect-presets`, `effect-inspect`, `effect-apply`, `effect-set-enabled`, `preview-benchmark`, `proxy`, `proxy-preview`, `media-manifest`, `media-relink`, `init-golden`, `verify-m1` ve `verify-all` desteklenir. Gerçek video proxy'sinin önbellek anahtarı kaynak baytlarının SHA-256'sına bağlıdır.
- **Ses, spektral analiz ve AI medya boru hatları.** `peaks` (`--band`), `waveform` (`--style dual|spectral`), `ai-status`, `ai-stems`, `ai-lyrics`, `upscale` (Real-ESRGAN NCNN Vulkan / Lanczos), `interpolate` (`rife-ncnn-vulkan` / `minterpolate` / blend), `ai-matte`, `ai-models` ve `ai-run` (`model_manifest.json` SHA-256 `Strict` + `--allow-unverified`). Tüm **50 CLI alt komutunun** sözdizimi için `frameshift-cli --help` kullan.

## Temel fikir

Agent, arayüzde fareyle işlem yapmak yerine klipleri, kaynak-zaman eşlemelerini, animasyon eğrilerini, maskeleri, 3D sahne rig'lerini ve efekt bağlantılarını sürümlenmiş bir proje tanımı (`schema_version: 0.2.0`) üzerinden oluşturur ve değiştirir.

İnsan ve agent aynı projeyle çalışır. Masaüstü arayüzü (`frameshift-app` çekirdeği) yapılan işlemleri gösterir ve düzenlemeye olanak verir; proje durumu ve edit/render mantığı arayüze bağımlı olmaz. Aynı çekirdek headless CLI ve MCP sunucusu tarafından doğrudan kullanılır.

```text
Desktop UI (P4 ertelenmiş) ─┐
Agent (MCP: 32 araç) ───────┼── Shared operation API (frameshift-core/app) ── Native GPU engine
CLI (50 alt komut) ─────────┘                                                       │
                                                      Frames / previews / NLE / final export
```

Bu bir mantıksal sınırdır, kesin süreç mimarisi değildir. Motor masaüstü uygulamasıyla aynı süreçte çalışabilir; pencere kapandığında işlerin yaşam döngüsü `BackgroundJobSupervisor` / `WindowClosePolicy` üzerinden yönetilir.

## Motor yetenekleri ve hedefleri

1. **Hassas zaman modeli (`frameshift-time`):** `Flicks(i64)` (`705_600_000/sn`), `Rational32`, `i128` ara aritmetik, VFR PTS eşlemesi, hız eğrileri, freeze/reverse, ADR-0003 `EventTimingConstraint` Kahn DAG çözücüsü ve `SafeExpr` (`spring`, `wiggle`, audio-reactive bant değişkenleri).
2. **Sahne ve kompozisyon (`frameshift-schema` / `frameshift-render`):** **40 `NodeKind`**, dallanabilen DAG, `Precomp`/`PrecompInput`, `AdjustmentLayer`, `Scene3D` paylaşımlı kamera + `Depth32Float` + `Null3D` parent rig + `DepthMatte` + `ShapeMask` (arc-length morph).
3. **Efekt ve geçişler (60 WGSL shader):** 10 built-in geçiş, `Anime4K` (8 WGSL), `OpticalFlowBlur` (4-seviyeli Lucas-Kanade/Horn-Schunck), `DeepGlow`, `BlurMoCurves`, `DisplacementMap`, `FrameEcho`, `FilmOptics` (AgX/PBR Neutral tonemap, 35mm grain, halation), `HalftoneGlow`, `RadialShockwave`, `MotionTileShake`, `AnimeImpactFrame` ve `CustomWgsl` sandbox.
4. **Motion graphics & tipografi (`frameshift-vector`):** Saf Rust `ttf-parser` + `lyon_tessellation` + `skrifa` + `harfrust` şekillendirme, WGSL Slug GPU Bézier (`text_slug.wgsl`), JFA SDF (`text2d_jfa_*`), `Text3D`/`Shape3D`/`ParticleField3D`/`RibbonTrail3D` ve `VectorPathHud`.
5. **Görüntü doğruluğu ve performans:** `Rgba16Float` Linear Rec.709 premultiplied-alpha çalışma alanı, `4x MSAA`, `nv12`→Linear GPU decode (`yuv_to_linear.wgsl`), 3-katmanlı `NodeContentHash` Smart Cache ve sub-frame temporal motion blur.
6. **İncelenebilirlik:** Aynı `revision_id` ve zaman aralığına bağlı multi-surface inceleme paketi (`inspect`), `lint_project` (10 kural + opt-in `blackdetect`), `diff_frame`, `ui-snapshot` ve `waveform`.

Kapsam aşamalı geliştirilir: tam genel amaçlı 3D modelleme/render veya yerleşik tam GUI zaman çizelgesi (P4) mevcut dilimde değildir; AI rotoskopi/derinlik/stem/upscale harici ONNX/Python/NCNN sidecar ve araçlarıyla köprülenir.

## Mevcut mimari ve teknoloji envanteri

| Alan | Uygulanan Bileşenler / Durum |
| --- | --- |
| Workspace | Rust (`edition = "2024"`, **9 crate**) |
| Zaman tabanı & İfade | `frameshift-time` (`Flicks`, `Rational32`, `SpeedCurve`, `SafeExpr`) |
| Proje şeması & NLE | `frameshift-schema` (`0.2.0`, 40 `NodeKind`, `Motion.md` DSL, `.otio`/FCPXML 1.10/1.11) |
| Native GPU compositor | `frameshift-render` (`wgpu 30.0.1` + **60 WGSL shader**, `4x MSAA`, Smart Cache, 2D/Planar Tracker) |
| Text / Vector | `frameshift-vector` (`ttf-parser`, `lyon_tessellation`, `skrifa`, `harfrust`, Slug GPU Bézier, JFA SDF) |
| Medya I/O, Ses & AI | `frameshift-media` (`ffmpeg`/`ffprobe`, `nv12` pipe, Radix-2 FFT spectral flux, waveform PNG; harici `rife-ncnn-vulkan`, `realesrgan-ncnn-vulkan`, Python ONNX/Audio sidecar) |
| Renk yönetimi | Saf Rust `.cube`/`.spi1d`/`.spi3d`/`.ocio` YAML boru hattı + `Rgba16Float` Linear Rec.709 + `yuv_to_linear.wgsl` |
| İstemci katmanları | `frameshift-core`, `frameshift-cli` (**50 alt komut**), `frameshift-mcp` (**32 araç**), `frameshift-app` (headless controller + `ui-snapshot`; P4 GUI ertelenmiş) |

Tarihsel tasarım notları için [proje notlarına](PROJECT_NOTES.md), güncel test ve doğrulama kanıtları için [proje durumuna](brain/state.md) bakınız.

## Agent için geri bildirim

- **Kare, fark ve crop:** `frame`, `diff-frame` (MAE/PSNR/bbox + 3-panelli ısı haritası), `inspect` (tam çözünürlük crop, maske, geçiş, velocity paketleri).
- **Kısa sesli taslak ve export:** `render --range`, `export --platform ...`, ses senkronu ve ffprobe doğrulaması.
- **Contact sheet ve materyal galerisi:** Zaman etiketli kare şeritleri (`inspect`) ve `Shape3D`/`Text3D` materyal örnekleri (`materials`).
- **Proje ve ses incelemesi:** `lint` (10 kural), `timeline`, `speed-graph`, `peaks --band`, `waveform`, `describe_schema` ve `list_edit_receipts`.

Kare şeridi akıcılığı veya ses senkronunu tek başına kanıtlamaz. İnceleme çıktıları neyin gözlendiğini ve neyin doğrulanamadığını açık tutmalıdır.

## Doğrulama durumu

İlk M1 "Golden Vertical Slice" (**hız eğrisi + hareketli maske + shader geçişi + temporal motion blur + müzik**) ve M1–M5 `verify-all` kapıları tamamlanmıştır.

2026-11-24 tarihli bağımsız `verifier` doğrulama özeti ([state.md](brain/state.md)):
- `cargo clippy --workspace --all-targets -- -D warnings`: **0 uyarı**.
- `cargo test --workspace --no-fail-fast`: **452/453 geçti** (`frameshift-time` 20/20, `frameshift-schema` 82/82, `frameshift-vector` 38/38, `frameshift-media` 78/78, `frameshift-render` 114/114, `frameshift-app` 15/15, `frameshift-mcp` 10/10, `frameshift-cli` 28/28, `frameshift-core` 67/68 — tek hata `HEAD`'de önceden mevcut `materials_gallery_fit_is_normalized`).
- Canlı CLI smoke (`compile-dsl`, `lint`, `.otio`/`.fcpxml` roundtrip, `frame` + `diff-frame`, `ai-models`, `waveform`, `peaks --band`): PASS.

## Dokümantasyon

1. [Proje durumu](brain/state.md) ve [beyin dizini](brain/index.md): Güncel faz, doğrulama kayıtları, ADR'ler, kilitli spec'ler ve araştırma raporları.
2. [Proje notları](PROJECT_NOTES.md): Başlangıç tasarım kararları, mimari görüşmeler ve arşivlenmiş araç incelemeleri.
3. [Implementasyon planı](IMPLEMENTATION_PLAN.md): Faz 0–5 yol haritası ve risk matrisi.
4. [Motor alternatifleri araştırması](ENGINE_ALTERNATIVES_RESEARCH.md): HyperFrames, Remotion, native motorlar ve uygulama/fork adayları (arşiv referansı).
5. [Görsel referanslar](VISUAL_REFERENCES.md): Referans video bağlantıları, örnek kare gözlemleri ve motor tasarımına çıkarımlar.

Araştırma belgeleri bir performans benchmark'ı değildir. Referans videoları ve üçüncü taraf kaynak kodları bu repoya dahil edilmemiştir. Belgelerdeki yerel cache/klasör yolları geliştirme notlarıdır; kullanıcı makinesinde var olmaları beklenmez.

## Lisans ve adlandırma

Cargo workspace'i (`Cargo.toml`) **`MIT OR Apache-2.0`** lisansını beyan eder. Harici ikililerin, codec'lerin, fontların, ONNX modellerinin ve kullanılacak medyanın lisansları ayrı değerlendirilir; referans bağlantısı yeniden dağıtım veya kullanım izni sağlamaz.

**FrameShift** proje adı olarak seçilmiştir; marka, alan adı ve paket adı uygunluğu henüz doğrulanmamıştır.
