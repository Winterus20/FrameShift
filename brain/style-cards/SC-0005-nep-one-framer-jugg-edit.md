---
id: SC-0005
title: "Nep / Jugg Ekolü — One-Framer & Yüksek Enerjili Velocity Edit"
creator: "@zneptun (Nep / Neptune)"
source_url: "n/a (ikincil web & breakdown arastirmasi — 2026-11-24)"
date: 2026-11-24
category: anime-amv
energy: high
fps: 60
aspect_ratio: "9:16"
frameshift_readiness: partial
tags: [one-framer, jugg, twixtor, velocity, stutter, kinetic-text, archetype]
---

# SC-0005 — Nep / Jugg Ekolü — One-Framer & Yüksek Enerjili Velocity Edit

## 1. Kimlik & Genel His (Ne Zaman Seçilmeli?)
- **Özet His / Estetik:** Yüksek tempolu, duyusal yoğunluğu yüksek ve vuruş odaklı kısa form sosyal medya stili. Hızlı **one-framer** (tek karelik kesme/flaşlar), *stutter* (takılma/tekrar) efektleri, agresif velocity sıçramaları, belirgin ekran sekmesi (bounce) ve ritme sıkı kilitli kinetik tipografi ile izleyiciyi yüksek enerjili kısa bir döngüde tutar.
- **Uygun Kullanım Senaryoları:** Hızlı trap, hyperpop, phonk veya yoğun hi-hat/transient içeren 10–25 sn TikTok/Reels karakter editleri.
- **Uygun Olmayan Senaryolar:** Uzun anlatı odaklı dramatik AMV'ler, düşük tempolu duygusal kurgular veya görsel minimalizm isteyen projeler.

## 2. Zamanlama, Ritim & Kurgu Dili
- **Tempo & Kesim Sıklığı:** Çok yüksek kesim sıklığı; ana vuruşların arasındaki mikro transient'lar (hi-hat, snare, vokal heceleri) dahi tek veya birkaç karelik kesme/flaşlarla işaretlenir.
- **Easing / Speed-Graph Karakteri:** *Jugg* ve *velocity* karakteri: klip içi hız kısa bir an donar veya aşırı yavaşlar, vuruş anında dik bir sıçrama (speed spike) ile ileri atılır ve sert bir ekran bounce'u ile oturur.
- **Ses Senkronu:** Hızlı perküsyon ve vokal vurgularına birebir senkron; ağır drop öncesi 1–2 karelik mikro-sessizlik veya siyah flaş ile kontrast yaratılır.

## 3. Kamera & Uzay (2D / 2.5D / 3D)
- **Uzay Modeli:** Ağırlıklı 2D/2.5D katmanlı uzay; hızlı ölçek (scale), tilt ve ekran sekmesi (bounce) merkezli dinamizm.
- **Kamera Hareket Sözlüğü:** Ani zoom-in/out patlamaları, stutter/glitch atlamaları, kısa süreli yönlü twitch ve sert sönümlü shake darbeleri.

## 4. Görsel Katmanlar, Tipografi & Renk Paleti
- **Kaynak Görüntü:** Keskin kontrastlı karakter yakın planları, hızlı bakış/silah/efekt kareleri ve tek karelik sembolik kesitler (manga karesi veya kontrast patlaması).
- **Tipografi Dili:** Yoğun, hızlı belirip kaybolan kinetik metin blokları; kelime veya hece bazlı ani scale/stutter girişleri ve güçlü kontrast/stroke vurguları.
- **Renk & Kontrast:** Yüksek kontrast, ani exposure/invert flaşları (one-framer anlarında), doygun vurgu renkleri ve belirgin chromatic aberration.

## 5. Efekt & Geçiş Tarifi (Reçete)
- **Temel Post-FX Zinciri:** `Twixtor / Velocity Time-Remap -> Hızlı Scale Bounce + Motion Blur -> One-Framer / Stutter / Glitch -> Impact Flash + DeepGlow -> Global High-Contrast CC + CA`.
- **Geçişler:** Hızlı zoom geçişleri, glitch displacement, whip/directional warp ve tek karelik beyaz/siyah/invert ara kareler.
- **Atmosfer / Dekoratif Katmanlar:** Kinetik tipografi katmanları, glitch/RGB split darbeleri, isteğe bağlı özne arkası yazı/efekt (rotoscope varsa).

## 6. FrameShift Yetenek Eşleşmesi
| Teknik | Durum | FrameShift Karşılığı / Eksik Notu |
| :--- | :--- | :--- |
| Tek karelik flaş & impact darbeleri | ✅ | `Flicks` tam sayı zamanlaması + `AnimeImpactFrame` + `SafeExpr` step darbeleri |
| Glitch, chromatic split & hızlı zoom | ✅ | `transition_glitch_displace`, `transition_cross_zoom`, `FilmOptics` |
| Kinetik 2D/3D tipografi katmanları | ⚠️ | `VectorText` (`HarfRust`/`Skrifa`) ve `Text3D` mevcut; ancak harf bazlı rotasyon/renk ve gerçek özne arkası alfa matte açık |
| Aşırı yavaş çekim / Twixtor velocity sıçramaları | ❌ | Optik akış tabanlı kare interpolasyonu yok; `SpeedGraph` hızlandırması için 60+ fps ön-işlenmiş klip gerekir |
| Canlı scrub & hız eğrisi ince ayarı | ⚠️ | Headless `speed graph` ve CLI `sheet` ile kontrol mümkün, canlı masaüstü önizleme arayüzü yok |

## 7. Ajan İçin Uygulama Kuralları (Do / Don't)
- **Yap (Do):** One-framer ve stutter kesimlerini kulakla doğrulanmış ses transient'larına tam `Flicks` karesiyle kilitle; hızlı zoom/bounce hareketlerinde mutlaka motion blur açık tut; metin girişlerini vuruş ömrüne göre kısa ve okunaklı tut.
- **Yapma (Don't):** Rastgele zamanlara tek kare flaş atma (ritimsiz gürültüye dönüşür); 24 fps ham klibi ara kare üretmeden aşırı yavaşlatmaya çalışma (takılma yapar); arka planı okuyamayacak kadar yoğun metin ve efekti aynı karede üst üste yığma.

## 8. Epistemik Sınır
- Bu kart, **2026-11-24 tarihli ikincil web araştırması ve topluluk breakdown özetlerinden** çıkarılmış bir **arketip stil kartıdır**.
- `@zneptun` hesabına ait tek bir video dosyası veya `.aep` / XML proje dosyası indirilip kare-kare/ses-dalgası düzeyinde incelenmemiştir. Birebir stil replikasyonu yapılacaksa seçilen referans video indirilerek `ffprobe` ve kare şeridi ile doğrulanmalıdır.
