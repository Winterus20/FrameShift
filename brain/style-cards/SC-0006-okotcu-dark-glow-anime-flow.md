---
id: SC-0006
title: "@_okotcu Dark-Glow Anime Flow & Impact"
creator: "@_okotcu"
source_url: "https://www.tiktok.com/@_okotcu/video/7554984925942811924"
date: 2026-11-25
category: anime-amv
energy: high
fps: 60
aspect_ratio: "1:1"
frameshift_readiness: ready
tags: [anime-amv, 1:1, 60fps, dark-glow, whip-zoom, eye-closeup, impact-frame, motion-blur]
---

# SC-0006 — @_okotcu Dark-Glow Anime Flow & Impact

## 1. Kimlik & Genel His (Ne Zaman Seçilmeli?)
- **Özet His / Estetik:** 1:1 kare formatta, derin siyah gölgeler ve yüksek kontrastlı neon mor/kızıl/altın HDR glow üzerine kurulu, neredeyse yazısız saf anime flow & impact editi.
- **Uygun Kullanım Senaryoları:** Güçlü drop'a sahip pop/phonk/EDM edit sesleri (ör. Kesha - Blow edit audio) ve karanlık aura, kanat, parlayan göz yakın planları içeren güçlü anime karakterleri (ör. Lufas Maphaahl).
- **Uygun Olmayan Senaryolar:** %100 söz-odaklı kinetik tipografi videoları veya pastel/düşük kontrastlı slice-of-life sahneleri.

## 2. Zamanlama, Ritim & Kurgu Dili
- **Tempo & Kesim Sıklığı:** 14.75 sn içinde ~12–14 ana kesim. İlk ~5 sn (build-up) 1.2–2.2 sn'lik uzun, gerilimli planlarla akar; drop (6.0 sn) sonrası ~1.2 sn'lik vuruşlara, finalde ise 0.58–0.60 sn'lik çift/üçlü hızlı vuruşlara geçer.
- **Easing / Speed-Graph Karakteri:** Sert başlangıç + uzun sönümleme (`AnimeSnap` / `CubicBezier`), geçişlerde güçlü directional smear/blur.
- **Ses Senkronu:** Drop öncesi nefes boşluğunda gerilim/blur artışı, drop anında sert impact + shockwave, ardından kick/snare tepelerine kesim.

## 3. Kamera & Uzay (2D / 2.5D / 3D)
- **Uzay Modeli:** 2D/2.5D katmanlı hareket (zoom, pan, hafif roll, motion-tile bounce).
- **Kamera Hareket Sözlüğü:** Sürekli mikro zoom-in, beat anlarında punch-zoom + sönümlü shake, geçişlerde whip-pan / cross-zoom.

## 4. Görsel Katmanlar, Tipografi & Renk Paleti
- **Kaynak Görüntü:** Temiz, altyazısız 1080p/4K anime klipleri (göz yakın planları, silüetler, enerji patlamaları, aksiyon pozları).
- **Tipografi Dili:** Büyük yazı yok; yalnızca görsel akış (en fazla çok küçük/minimal imza).
- **Renk & Kontrast:** Ezilmiş derin siyahlar, yüksek kontrast, neon mor (`#a855f7`), kızıl (`#ef4444`) ve altın (`#f59e0b`) HDR glow vurguları.

## 5. Efekt & Geçiş Tarifi (Reçete)
- **Temel Post-FX Zinciri:** `AnimeSharpen` -> `OpticalFlowBlur` (RSMB) -> `ColorBasic` (yüksek kontrast) -> `DeepGlow` -> `FilmOptics` (`AgxPunchy` + Spektral CA + Vignette) -> `MotionBlurShutter`.
- **Geçişler:** `WhipChromatic`, `CrossZoom`, `DirectionalWarp`, `GlitchDisplace`.
- **Atmosfer / Dekoratif Katmanlar:** Drop anında `RadialShockwave` ve 2–3 karelik `AnimeImpactFrame` flaşları.

## 6. FrameShift Yetenek Eşleşmesi
| Teknik | Durum | FrameShift Karşılığı / Eksik Notu |
| :--- | :--- | :--- |
| 1:1 60fps Kare Format | ✅ | `Project.width=1080, height=1080, fps=60` |
| Pyramidal RSMB / Motion Blur | ✅ | `OpticalFlowBlur` + `MotionBlurShutter` |
| Whip/Zoom/Warp Geçişleri | ✅ | `WhipChromatic`, `CrossZoom`, `DirectionalWarp` |
| Dark Glow + AgX Punchy CC | ✅ | `DeepGlow` + `ColorBasic` + `FilmOptics` |
| Shockwave & Impact Frame | ✅ | `RadialShockwave` + `AnimeImpactFrame` |

## 7. Ajan İçin Uygulama Kuralları (Do / Don't)
- **Yap (Do):** Yalnızca temiz, logosuz/altyazısız 4K/1080p klipler kullan; göz yakın planlarını drop veya hızlı vuruşlara yerleştir; `OpticalFlowBlur` + `DeepGlow` kombinasyonunu koru.
- **Yapma (Don't):** Altyazılı/watermark'lı veya düşük çözünürlüklü klip kullanma; drop öncesi sessizliği/nefesi atlama; ekranı büyük yazılarla doldurma.

## 8. Epistemik Sınır
- TikTok videosunun `scdet` kesimleri, `ref.wav` RMS profili ve `sheet_4fps.png` karelerinden doğrudan gözlemlenmiştir; orijinal After Effects proje dosyası incelenmemiştir.
