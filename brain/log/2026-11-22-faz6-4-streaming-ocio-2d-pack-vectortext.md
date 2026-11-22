---
title: Faz 6.4 — Streaming Video Decoder, Harici .cube/OCIO LUT, 2D Kolaj/Precomp/HUD/Warp ve 2D TTF VectorText
date: 2026-11-22
status: completed
tags: [media-io, color, render-graph, shaders, text-vector, schema, agent, inspection]
---

# 2026-11-22 — Faz 6.4: Streaming Video Decoder, `.cube`/OCIO, 2D Kolaj/Precomp/HUD/Warp ve 2D TTF VectorText

**Tarih:** 2026-11-22  
**Faz:** Faz 6.4 (4 Paralel Yükseltme Hattı)  
**Durum:** Tamamlandı ve Doğrulandı (**0 yeni harici Cargo crate**)

---

## Amaç

Önceki oturumlarda not edilen ve bekleyen 4 ana motor eksikliğini önce birincil kaynaklı teknik araştırmalarla (`brain/research/`), ardından Single-Writer `/swarm` orkestrasyonu ve bağımsız `verifier-final` doğrulamasıyla eksiksiz uygulamak:
1. **Sürekli Video Decoder (`frameshift-media`, `frameshift-core`)**
2. **Harici `.cube` (1D/3D/Shaper) + `.spi1d`/`.spi3d` & Saf Rust OpenColorIO (`OcioColorPipeline`)**
3. **2D Kompozisyon, Kolaj, Precomp, HUD & Deformasyon Düğümleri (`SourceImage`, `Precomp`, `PrecompInput`, `VectorPathHud`, `CornerPin`, `GridWarp`)**
4. **2D `VectorText` İçin Gerçek TTF/OTF Font, Çok Satır, Hizalama ve `FontNotFound` Kuralı + MCP `describe_schema` Senkronizasyonu**

---

## Yapılanlar

1. **4 Teknik Araştırma Belgesi (`brain/research/`):**
   - [streaming-video-decoder.md](../research/streaming-video-decoder.md) — Ortam doğrulaması (`FFMPEG_DIR`/`clang`/`vcpkg` yok; statik `ffmpeg 9.0.2` ile 120 kare sürekli pipe `0.427s` vs `4×30` pencere `0.871s` vs `d3d11va` CPU pipe `0.904s`) ve `DecoderPool` şartnamesi.
   - [cube-lut-and-ocio.md](../research/cube-lut-and-ocio.md) — Adobe `.cube` (1D/3D/shaper), `.spi1d`/`.spi3d`, `.ocio` YAML ayrıştırıcı ve CIE/Bradford/ACES/LogC3/SLog3 `f64` dönüşüm şartnamesi.
   - [2d-collage-precomp-hud-warp.md](../research/2d-collage-precomp-hud-warp.md) — `SourceImage`, `Precomp` (`MAX_PRECOMP_DEPTH=8`), `VectorPathHud` (`lyon_tessellation` + analitik SDF AA), `CornerPin` (Paul Heckbert + `glam::Mat3` homografi) ve `GridWarp` (8×8 uniform + Catmull-Rom WGSL) şartnamesi.
   - [2d-vectortext-ttf-layout.md](../research/2d-vectortext-ttf-layout.md) — `font_locator` (`FontNotFound`), çok-konturlu `NonZero` 4×4 SSAA scanline, `TextAlign2D` ve M1 golden hash koruması şartnamesi.

2. **Sürekli Video Decoder (`frameshift-media`, `frameshift-core`):**
   - `crates/frameshift-media/src/streaming.rs`: `StreamingVideoDecoder` (`std::process::Child`, `.stdin(Stdio::null())`, 2 MiB `BufReader<ChildStdout>`, `stderr` 64 KiB drain thread, `Drop` içinde `kill() + wait() + stderr_drain.join()`) ve `DecoderPool` (`DEFAULT_MAX_OPEN_STREAMS = 8`, `DEFAULT_MAX_SEQUENTIAL_SKIP = 32`).
   - `crates/frameshift-media/src/ffmpeg.rs`: `SourceFit` için `Hash` derive ve `append_fitted_output` / `midpoint_seek_seconds` `pub(crate)` paylaşımı.
   - `crates/frameshift-core/src/resolver.rs` & `engine.rs`: `MediaResolver::decoder_pool` entegrasyonu ve `clear_cache()` stream temizliği.

3. **Harici `.cube` / `.spi1d` / `.spi3d` & Saf Rust OCIO (`crates/frameshift-render/`, `crates/frameshift-schema/`):**
   - `crates/frameshift-render/src/lut.rs`: `CubeLut1D`, `LUT_1D_SIZE`, `LUT_1D_INPUT_RANGE`, `LUT_3D_INPUT_RANGE`, birleşik 1D+3D shaper, `.spi1d`/`.spi3d`, `compose_color_space_bridge` ve `CubeLut3D::resolve_source`.
   - `crates/frameshift-render/src/ocio.rs`: `OcioSpace` (10 yerleşik uzay), saf Rust `.ocio` YAML/text parser (`roles`, `colorspaces`, `aliases`, `Matrix/File/Exponent/Log/GroupTransform`, ters türetme), `OcioColorPipeline` + opsiyonel `ociobakelut` CLI fallback.
   - `crates/frameshift-render/shaders/lut3d.wgsl`: Negatif `DOMAIN_MIN` desteği (`max(..., 0.0)` kısıtı kaldırıldı).

4. **2D Kolaj, Precomp, HUD & Deformasyon + Şema/MCP/UI Snapshot:**
   - `crates/frameshift-media/src/frame.rs` & `ffmpeg.rs`: Premultiplied-safe `fit_rgba8_frame` + `FfmpegBridge::load_image_frame_with_fit` (PNG için 0-subprocess `png` crate hızlı yolu; JPEG/WebP/BMP/TIFF için `ffmpeg -frames:v 1` fallback).
   - `crates/frameshift-render/src/warp2d.rs`, `shaders/corner_pin.wgsl`, `shaders/grid_warp.wgsl` (toplam **33 WGSL shader**).
   - `crates/frameshift-vector/src/hud_path.rs` & `canvas.rs`: `lyon_tessellation` + dash/trim-path + düğüm noktaları + veri etiketi HUD rasterizer'ı (`VectorPathHud`).
   - `crates/frameshift-schema/src/{graph.rs, project.rs, error.rs}`: `LutSourceDef`, `ExtendedColorSpace`, `TextAlign2D`, `CubicBezierSegmentDef`, `SourceImage`, `Precomp`, `PrecompInput`, `VectorPathHud`, `CornerPin`, `GridWarp`, `NodeKind::ALL_SERDE_NAMES` (**30** kanonik düğüm adı), recursive `Precomp` doğrulaması ve `SafeExpr` beyaz listeleri.
   - `crates/frameshift-mcp/src/server.rs`: `describe_schema` listesi doğrudan `NodeKind::ALL_SERDE_NAMES` ile tam senkronize edildi.
   - `crates/frameshift-app/src/snapshot.rs`: Tüm yeni düğümler için `node_kind_label` ve `describe_node_params` eklendi.

5. **2D `VectorText` Gerçek Font & Hizalama (`crates/frameshift-vector/`, `frameshift-core/`, `frameshift-render/`):**
   - `crates/frameshift-vector/src/font_locator.rs` & `text.rs`: `font_path` > `font_family + font_weight` çözümleme (sıfır sessiz fallback → `VectorError::FontNotFound`), çok-konturlu `NonZero` 4×4 SSAA dolgu + SDF kontur + `\n` çok satır + `Left | Center | Right` + `x_offset_norm` + `tracking_em` + `line_height_em` + `opacity`. `font_family: None && font_path: None` iken M1 golden yolu bit-exact korundu.
   - `crates/frameshift-render/src/compositor.rs`: `VectorText` hata yayılımı (`FontNotFound` yutulmaz), `SourceImage` GPU `Rgba16Float` önbelleği (`image_gpu_cache`), `Precomp` (`remap_precomp_tick`, `MAX_PRECOMP_DEPTH=8`), `VectorPathHud`, `CornerPin`, `GridWarp` ve `Lut3D` (`LutCacheKey`) tam entegrasyonu.

---

## Doğrulama ve Kanıtlar

- `git diff -- Cargo.toml crates/*/Cargo.toml`: **0 değişiklik (0 yeni harici crate)**.
- `rg -n "\b(loop|while)\b" crates/frameshift-render/shaders/`: **0 sınırsız kod döngüsü**.
- `cargo clippy --workspace --all-targets -- -D warnings`: **PASS (0 uyarı, tüm 9 crate + test hedefleri)**.
- `cargo test --workspace`: **177/177 test geçti** (0 failed, 0 ignored; `app` 5, `cli` 7, `core` 17, `mcp` 3, `media` 18, `render` 66, `schema` 22, `time` 13, `vector` 26).
- `cargo run --release -p frameshift-cli -- verify-all --out-dir target/full_verify`: **PASS (`all_gates_passed: true`)** — M1–M5 kapıları geçti, golden hash'ler korundu.
- **Sürekli Video Decoder Hızlanması:** `local_media/asta_amv_project.json` (`--profile draft`, 264 kare @ 1280×720 24fps, `sub_samples=6`): **`112.56s -> 70.53s`** (`42.03s` / **`%37.3`** hızlanma).
- **Görsel Doğrulama:** `target/verify_64/showcase_64.png` (`1280×720`, `fnv1a=0x48c3de69212115cc`) `frameshift-cli frame` ile render edilip `read_file` ile bizzat incelendi: `SourceImage` (`contain`), `Precomp` + `PrecompInput`, `VectorPathHud` (kesikli kübik Bézier + düğümler + `HUD // 98.60`), `GridWarp`, `CornerPin`, `VectorText` (2 satır Arial TTF + kontur) ve `.cube` `Lut3D` sıfır siyah/boş/NaN artefaktla doğrulandı.

---

## Açık Kalanlar / Sıradaki Adım

- Kullanıcı isterse kalan opsiyonel kalemler: `frameshift-app` canlı `winit`/`egui` pencere kabuğu, CLI/MCP `Demucs + stable-ts` söz hizalama otomasyonu, `SafeExpr` ekstra operatörleri (`if`, `<, >, ==`, `mix`, `floor`, `sqrt`), `solo` modu, waveform PNG ve `SC-0002+` stil kartları.
