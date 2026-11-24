---
id: SC-0004
title: "Xenoz Ekolü — Flow & Hard Impact AMV"
creator: "@xenoz / @xenozedit"
source_url: "n/a (ikincil web & breakdown arastirmasi — 2026-11-24)"
date: 2026-11-24
category: anime-amv
energy: high
fps: 60
aspect_ratio: "9:16"
frameshift_readiness: partial
tags: [flow, impact-shake, speed-graph, sfx-layering, cinematic-cc, archetype]
---

# SC-0004 — Xenoz Ekolü — Flow & Hard Impact AMV

## 1. Kimlik & Genel His (Ne Zaman Seçilmeli?)
- **Özet His / Estetik:** Kesintisiz akış (*flow*) ile sert vuruş (*hard impact*) dengesini kuran modern AMV dili. Klipler arası kesimler ani kopuş yerine kamera hareketi ve sahne içi momentumla birbirine akar; büyük vuruşlarda yönlü kamera darbesi, yüksek kontrastlı sinematik renk (CC) ve sıkı ses tasarımıyla güçlü bir fiziksel darbe hissi üretilir.
- **Uygun Kullanım Senaryoları:** Aksiyon ağırlıklı anime sahneleri (*Jujutsu Kaisen, Naruto, Bleach* vb.), belirgin kick/snare/drop yapısına sahip yüksek enerjili AMV ve kısa form kurgular.
- **Uygun Olmayan Senaryolar:** %100 yavaş/atmosferik kesimsiz montajlar veya her karesi tek karelik kaotik flaşlara dayalı ekstrem deneysel kolajlar.

## 2. Zamanlama, Ritim & Kurgu Dili
- **Tempo & Kesim Sıklığı:** Sıkı beat-sync kurgusu; kesimler, punch-in zoom'lar ve yönlü geçişler doğrudan ağır bass vuruşlarına veya snare transient'larına oturtulur.
- **Easing / Speed-Graph Karakteri:** *"Anime snap"* eğrisi — hareket geçiş öncesinde yavaş/sakin seyreder, geçiş/vuruş karesinde keskin bir hız zirvesine fırlar, sonrasında hızlı ama yumuşak biçimde sönümlenir (lineer keyframe kullanılmaz).
- **Ses Senkronu:** Müzik beat haritasının yanında çok katmanlı SFX kullanımı: riser zirvesi tam kesim karesine, whoosh merkezi geçiş anına, sub-bass/impact SFX ise ağır vuruş karesine kilitlenir.

## 3. Kamera & Uzay (2D / 2.5D / 3D)
- **Uzay Modeli:** 2.5D katmanlı kamera + Null Object rig düzeni (statik klip bırakılmaz; sürekli ama yönü hesaplı pan/zoom/tilt akışı).
- **Kamera Hareket Sözlüğü:** Sahne içi hareketi devam ettiren yönlü pan/slide, ani punch-in/out zoom'lar, vuruş anında tetiklenen yüksek frekanslı ancak kısa sönümlü impact shake.

## 4. Görsel Katmanlar, Tipografi & Renk Paleti
- **Kaynak Görüntü:** Temiz, yüksek bitrate (Blu-ray / 60 fps ön-interpole) aksiyon anime klipleri; aksiyon yönleri ardışık kesimlerde birbirini besleyecek şekilde seçilir.
- **Tipografi Dili:** İkincil/vurgu amaçlı kalın başlık veya karakter adı kartları; sahneyi boğmadan geçiş veya drop anında stilize outline/glitch/grunge dokusuyla kullanılır.
- **Renk & Kontrast:** Yüksek kontrastlı sinematik CC (derinleştirilmiş siyahlar, doygun vurgular, müzik tonuna uyumlu sıcak/soğuk grade, hafif CA ve film grain).

## 5. Efekt & Geçiş Tarifi (Reçete)
- **Temel Post-FX Zinciri:** `Time Remap / Flow -> Kamera Pan/Zoom + Motion Blur -> Adjustment Layer (Impact Shake + DeepGlow/Lens Flare) -> Global CC (Contrast + CA + Film Grain)`.
- **Geçişler:** Yönlü slide/whip blur (`S_BlurMoCurves` benzeri yönlü motion blur), lens flare/flash ve hareket devamlılığına dayalı match-cut'lar.
- **Atmosfer / Dekoratif Katmanlar:** Parçacık/toz/kıvılcım bindirmeleri, geçişlerde ışık sızıntısı ve yüksek kaliteli optik glow.

## 6. FrameShift Yetenek Eşleşmesi
| Teknik | Durum | FrameShift Karşılığı / Eksik Notu |
| :--- | :--- | :--- |
| Yönlü geçişler ve motion blur | ✅ | `transition_whip_chromatic`, `transition_directional_warp`, `MotionBlurShutter` |
| Beat odaklı shake & ışık darbesi | ✅ | `MotionTileShake`, `DeepGlow`, `RadialShockwave`, `AnimeImpactFrame` |
| Sinematik CC, CA & Film Grain | ✅ | `Lut3D` / `OcioColorPipeline`, `FilmOptics` (grain, CA, halation), `AgX` tonemap |
| Speed Graph ("anime snap") eğrileri | ⚠️ | `SafeExpr` / speed graph API mevcut, ancak etkileşimli GUI curve editörü henüz yok |
| Twixtor / RIFE optik akış slow-mo | ❌ | Yerleşik kare interpolasyonu yok; kliplerin önceden `Flowframes`/FFmpeg ile hazırlanması gerekir |
| Çok kanallı riser/whoosh/impact SFX hattı | ⚠️ | Dışarıda hazırlanmış ses/SFX karıştırma destekli, ancak olaylara bağlı otomatik SFX şerit yönetimi sınırlı |

## 7. Ajan İçin Uygulama Kuralları (Do / Don't)
- **Yap (Do):** Kesim öncesi sahne hareket yönlerini eşle; shake yoğunluğunu yalnızca güçlü transient/drop anlarına saklayıp hızlı sönümle; tüm farklı anime kliplerini üst katmanda tek tutarlı CC altında birleştir.
- **Yapma (Don't):** Tüm sahnelere aynı şiddette sürekli shake basma; lineer keyframe veya motion blur'suz sert zoom kullanma; whoosh/riser zirvesini kesim karesinden kaydırma.

## 8. Epistemik Sınır
- Bu kart, **2026-11-24 tarihli ikincil web araştırması ve topluluk breakdown özetlerinden** çıkarılmış bir **arketip stil kartıdır**.
- `@xenoz` / `@xenozedit` kanalına ait tek bir video dosyası veya `.aep` proje dosyası indirilip kare-kare/ses-dalgası düzeyinde ölçülmemiştir. Üretimde birebir doğrulama gerekirse belirli bir referans video indirilip incelenmelidir.
