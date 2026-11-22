---
title: Açık Kaynak Efekt & Geçiş Kataloğu, Edit Anatomisi ve Faz 6 Görsel Kalite Yol Haritası
date: 2026-11-21
status: active
tags: [engine, render-graph, shaders, text-vector, licensing, agent]
---

# Açık Kaynak Efekt & Geçiş Kataloğu, Edit Anatomisi ve Faz 6 Görsel Kalite Yol Haritası

## 1. Bir Editte Transition Dışında Neler Var? (Pro Editlerin ~%85'i)

Profesyonel AMV, TikTok ve After Effects editlerinde geçişler (transition) toplam görsel etkinin yalnızca ~%15'idir. Geri kalan ~%85 şu 7 ana katmandan oluşur:

1. **Renk, Işık ve "4K CC" Netliği:** `3D LUT (.cube)`, Anime Line Sharpen / Upscale ("4K CC"), Multi-Pass HDR Deep Glow (Bloom), 35mm Film Grain & Halation, Vignette, Anamorphic Lens Flare / Light Leaks.
2. **Kamera & Optik Kusurlar:** Optics Compensation (Brown-Conrady lens bükülmesi), Chromatic Aberration (RGB kenar ayrışması), Bokeh Depth of Field (odak derinliği), Directional / Radial / Zoom Blur, Motion Tile + Damped Camera Shake / Wiggle.
3. **Stilize & Retro Post-FX:** VHS / CRT (tarama çizgisi, tape wobble, renk kanaması), Glitch / Pixel Sort / Datamosh, Halftone / Manga Screentone, Sobel Edge / Glowing Outline / Impact Frame.
4. **Zaman ve Akıcılık:** Speed Ramps (agresif velocity eğrileri), Optical Flow / Frame Interpolation (Twixtor tarzı akıcı ağır çekim), Shutter Motion Blur, beat'e kilitli Exposure Flash.
5. **Kompozisyon & Maskeleme:** Rotoscoping / Karakter Ayırma (metin veya efekti karakterin arkasına koyma), Track Matte / Hareketli Vektör Maskeler, Kolaj / Manga Panel / Split-Screen, 2.5D/3D Kamera Paralaksı.
6. **Tipografi & Motion Graphics:** Gerçek TTF/OTF fontlar, harf bazlı (per-character) kinetik animasyon, 3D Extrusion, Trim-Path neon şeritler, HUD / mikro-tipografi.
7. **Atmosfer, Parçacık & Ses Tasarımı (SFX):** Yıldız tozu, kıvılcım, duman, bokeh, kalp/yaprak parçacıkları + geçişlerde *whoosh/riser*, vuruşlarda *sub-bass impact*, glitch'te *static/zap*, yavaşlamada *low-pass filter*.

---

## 2. Dış Açık Kaynak vs. Ajan Yapımı Geçiş Karşılaştırması (Görsel Kanıt)

Aynı 2 gerçek anime klibi (`local_media/clip_1.mp4` → `clip_2.mp4`, 0.70 sn `smoothstep` geçişi) üzerinde üretilen [transition_comparison_2x2.mp4](../../local_media/transition_comparison_2x2.mp4) (`1x normal hız + 0.5x ağır çekim`) karşılaştırmasının sonucu:
- **Dış kaynak (`gl-transitions` MIT portları — `CrossZoom`, `DirectionalWarp`, `GlitchDisplace`):** Belirgin biçimde daha organik, katmanlı ve olgun görünüyor.
- **Sıfırdan ajan yapımı (`WhipChromatic`):** Çalışıyor ancak ağır çekimde daha düz/mekanik bir kaydırma + crossfade hissi veriyor.

**Mimari Sonuç:**
- **Öncelik 1 (Kalite Tabanı):** Küratörlü, lisans-doğrulanmış açık kaynak WGSL kütüphanesini genişletip ajanların ilk tercihi yapmak.
- **Öncelik 2 (Sınırsız Esneklik):** Kütüphanede bulunmayan özel 2D efekt/geçişler için runtime `CustomWgsl` sandbox düğümünü kullanmak; beğenilen özel shader'ları sonradan olgun kütüphaneye terfi ettirmek.

---

## 3. Doğrulanmış Açık Kaynak Kütüphaneleri (GitHub API Lisans & Olgunluk Kontrolü — 2026-11-21)

FrameShift `MIT OR Apache-2.0` lisanslı olduğu için `GPLv2` repolardan (ör. `obs-shaderfilter`) kod kopyalanmaz; yalnızca `MIT / BSD / Apache-2.0 / CC0` kaynaklar port edilir.

| Kategori | Kaynak Repo | Doğrulanmış Lisans (SPDX) | Yıldız | Kullanım Notu |
| :--- | :--- | :--- | :---: | :--- |
| **Anime "4K CC" Çizgi Netleştirme** | [bloc97/Anime4K](https://github.com/bloc97/Anime4K) | `MIT` | 21,651 | Anime editlerindeki "Topaz/4K CC" keskinliği için birincil GPU shader kaynağı. |
| **Transition'lar (~200 Geçiş)** | [gl-transitions/gl-transitions](https://github.com/gl-transitions/gl-transitions) | Dosya bazlı (`NOASSERTION` — çoğunlukla `MIT/BSD`) | 2,265 | `CrossZoom`, `DirectionalWarp`, `GlitchDisplace`, `DreamyZoom`, `Swirl`, `Burn`, `LinearBlur`, `Ripple`, `Kaleidoscope`. |
| **Post-FX (Bloom, Flare, Sharpen, CC)** | [crosire/reshade-shaders](https://github.com/crosire/reshade-shaders) | Dosya bazlı (`NONE/CHECK` — BSD/MIT/CC0 seçilmeli) | 1,216 | Yıllarca rafine edilmiş bloom, anamorphic flare, sharpen, color grading shader'ları. |
| **Retro / VHS / CRT / Glitch / Grain** | [ahopness/GodotRetro](https://github.com/ahopness/GodotRetro) | Dosya bazlı (`NOASSERTION` — MIT/CC0 modülleri) | 811 | NTSC/VHS tracking, CRT, lens distortion, glitch ve film grain modülleri. |
| **Genel 2D Efekt, Blur & Warp (VJ)** | [Vidvox/ISF-Files](https://github.com/Vidvox/ISF-Files) | `MIT` | 104 | Hazır JSON parametre metadatalı profesyonel video/VJ efekt shader'ları. |
| **HDR Deep Glow (Bloom)** | Jimenez 2014 PBR Bloom (13-tap / 9-tap mip piramidi) | Standart algoritma (sıfırdan WGSL) | — | `Rgba16Float` hattında titreşimsiz, fiziksel tabanlı Deep Glow. |

*(Not: 2–5 yıldızlı küçük/niş repolar ana kütüphane listesinden çıkarılmıştır.)*

---

## 4. En Yüksek Görsel Etki Çekirdek Planı (Faz 6.1 & Faz 6.2)

### Faz 6.1 — Sıfır Yeni Crate ile Görsel Kalite & Sınırsız 2D Efekt Paketi
1. **`DeepGlow` (`NodeKind::DeepGlow`):** 6-mip Jimenez 2014 PBR Bloom (`fx_bloom_downsample.wgsl` + `fx_bloom_upsample.wgsl`).
2. **`AnimeSharpen` (`NodeKind::AnimeSharpen`):** `bloc97/Anime4K` (MIT) tabanlı luma-gradient çizgi keskinleştirme + anti-ringing (`fx_anime_sharpen.wgsl`).
3. **`FilmOptics` (`NodeKind::FilmOptics`):** Brown-Conrady lens distortion, kenar RGB split, 35mm organik film grain, kırmızımsı halation ve vignette (`fx_film_optics.wgsl`).
4. **+6 Olgun `gl-transitions` (MIT) Geçişi:** `dreamy_zoom`, `swirl`, `linear_blur`, `burn`, `ripple`, `kaleidoscope` (yerleşik geçiş sayısı `4 → 10`).
5. **Runtime `CustomWgsl` Sandbox (`NodeKind::CustomWgsl`):** Naga doğrulamalı, hash önbellekli, proje JSON'ı içinden 0..2 girdili özel 2D WGSL efekt/geçiş yürütme.

### Faz 6.2 — 3D Kinetik Tipografi, Gerçek Fontlar ve Atmosfer
1. **`Camera3D` + 3D Katmanlar + `Depth32Float` Z-Buffer** (`glam` — `MIT OR Apache-2.0`).
2. **Gerçek TTF/OTF Fontlar + Per-Character Animasyon + 3D Extrusion** (`ttf-parser` + `lyon_tessellation` — `MIT OR Apache-2.0`).
3. **Z-Buffer Bokeh DoF** (`fx_bokeh_dof.wgsl`) + **Stateless Analitik 3D Parçacık (`ParticleField3D`) ve Trim-Path Şeritler (`RibbonTrail3D`)**.

---

## 5. Harici Crate Ekleme Karar İlkesi (Kullanıcı Tercihi — 2026-11-21)

- **Yüksek Net Değer (Otonom/Hızlı Yol):** Eklenen crate saf-Rust, lisans uyumlu (`MIT / Apache-2.0 / BSD`) ve sağladığı değer derleme/bakım maliyetinden açıkça yüksekse (ör. `glam`, `ttf-parser`, `lyon_tessellation`) eklenebilir.
- **Tradeoff / Risk Varsa (Kullanıcıya Bildir):** Ağır C/C++ sistem bağımlılığı (ör. OCIO C++ SDK, libavcodec FFI), belirsiz/GPL lisans, yüksek derleme maliyeti veya mimari kırılma riski varsa eklemeden önce tradeoff'larıyla birlikte kullanıcıya bildirilir.
