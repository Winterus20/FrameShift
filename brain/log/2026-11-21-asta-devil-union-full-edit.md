---
title: Asta // Anti-Magic Devil Union — Tam Özellikli Hibrit AMV & 3D Kinetik Tipografi Editi
date: 2026-11-21
status: completed
tags: [engine, render-graph, shaders, text-vector, media-io, inspection]
---

# 2026-11-21 — Asta // Anti-Magic Devil Union Tam Özellikli Edit

## Özet

Kullanıcının indirdiği ham Asta klipleri (`raw_asta_1.mp4`, `raw_asta_2.mp4`) ve `bgm_revenge.wav` parçasının en yüksek dinamik aralıklı 15 saniyelik drop/climax kesiti (`29.8s–44.8s`) seçilerek, FrameShift'e Faz 0–6.2 boyunca eklenen tüm çekirdek, anime post-FX, 3D kinetik tipografi, 3D atmosfer ve runtime `CustomWgsl` özelliklerini tek bir 43 düğümlü DAG projesinde birleştiren **15.00 saniyelik, 1280×720, 30 fps (450 kare)** final MP4 üretildi.

## Yaratıcı ve Teknik Kurgu

- **Üretilen Dosyalar:**
  - Proje JSON: `local_media/asta_devil_union_project.json` (43 düğüm, 53 `SafeExpr` bağlamı, 10 klip, 7 beat/event ankrajı)
  - Final Video: `local_media/asta_devil_union_final.mp4` (`1280×720`, `30 fps`, H.264 11.1 Mbps + 48 kHz stereo AAC, `20.86 MB`, `15.000s`)
  - Kesilmiş Medya: `local_media/asta_v2_clips/s1.mp4..s10.mp4`, `local_media/bgm_asta_revenge_cut.wav`
- **Kullanılan FrameShift Özellikleri:**
  - **9 Farklı WGSL Geçişi:** `burn`, `dreamy_zoom`, `whip_chromatic`, `cross_zoom`, `glitch_displace`, `swirl`, `kaleidoscope`, `directional_warp`, `ripple` (`SafeExpr smoothstep` ile kalıcı ilerleme)
  - **Velocity Speed Ramp:** 10 klibin tamamında `cubic_hermite` hız eğrisi
  - **Faz 6.2 3D Motoru:** `Transform3D` (canlı beat-zoom + Dutch tilt 3D kamera), 4 adet gerçek Windows `impact.ttf` 3D extruded `Text3D` (`"NO MAGIC"`, `"ASTA"`, `"DEVIL UNION"`, `"ANTI-MAGIC"`), `ParticleField3D` (88 adet 4-köşeli kızıl/altın 3D kıvılcım), `RibbonTrail3D` (3 adet kızıl-magenta neon enerji şeridi)
  - **Faz 6.1 Post-FX & Sandbox:** `AnimeSharpen`, Jimenez 2014 5-mip `DeepGlow`, `FilmOptics`, özel yazılmış runtime `CustomWgsl` ("Anti-Magic Crimson Aura & Anamorphic Beat Pulse")
  - **Faz 5 Anime Post-FX:** `MotionTileShake`, `RadialShockwave` (3.60s / 8.15s / 12.25s), `AnimeImpactFrame` (4 vuruşta kısa siyah-beyaz manga impact)
  - **Faz 1–4 Çekirdek:** `MotionBlurShutter` (4 sub-sample), `AudioRmsEnvelope` (`audio_rms`), `SourceFit::Cover`, `ColorBasic` + grade sonrası saf siyah açılış/kapanış (`Solid` + `Blend`)

## Doğrulama & Epistemik Sınır

- **Render Süresi:** `68.2 sn` (450 kare × 4 sub-sample = 1800 GPU pası, çıkış kodu 0).
- **Görsel İnceleme:** Önce 6 kilit probe karesi v1→v2 kalibrasyonuyla (HDR `DeepGlow` eşiği, `FilmOptics` kenar boşluğu, 3D yazı yerleşimi/kontrastı) düzeltildi; ardından final MP4'ten çıkarılan 5×5 (25 kare) contact sheet `read_file` ile bizzat kontrol edildi (sıfır siyah kare hatası, sıfır HDR patlaması, tüm 9 geçiş ve 4 adet 3D başlık temiz).
- **Epistemik Sınır:** Contact sheet ve probe kareleri kompozisyon, 3D tipografi okunaklılığı, renk/glow dengesi ve geçiş görsellerini doğrular; gerçek zamanlı hareket akıcılığı ve ses-ritim senkronu videonun doğrudan oynatılmasıyla doğrulanmalıdır.
