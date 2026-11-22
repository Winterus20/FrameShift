---
id: SC-0002
title: Mushoku Tensei — Full-Frame Kinetic Typography AMV
creator: "@asgardski"
source_url: "https://www.tiktok.com/@asgardski/video/7672463186175544609"
date: 2026-11-22
category: kinetic-typography
energy: high
fps: 60
aspect_ratio: "16:9"
frameshift_readiness: partial
tags: [typography, anime-amv, occlusion, collage, beat-cut]
---

# SC-0002 — Mushoku Tensei // Full-Frame Kinetic Typography AMV

## 1. Kimlik & Genel His (Ne Zaman Seçilmeli?)
- **Özet His / Estetik:** Sakin, hüzünlü fantezi görüntüsünün üzerine bindirilmiş **dev, harf harf yayılmış kelimeler**. Tipografi sahnenin önünde değil, karakterin *arkasında* durur; koyu silüetler harfleri yer yer yutar. Renkler soluk-tozlu (dusty blue, krem, kiremit kırmızısı), kontrast yumuşak, bloom ve grain belirgin.
- **Uygun Kullanım Senaryoları:** Orta/yavaş tempolu vokalli şarkılar, duygusal ama keskin kurgulu AMV'ler, söz odaklı "lyric-driven" editler. 16:9 yatay.
- **Uygun Olmayan Senaryolar:** Hızlı phonk/dubstep drop'ları (bu kartın kurgu ritmi 1 vuruş/plan), %100 3D tipografi (bkz. SC-0001), HUD/teknik kolaj yoğunluğu (bkz. VISUAL_REFERENCES #3).

## 2. Zamanlama, Ritim & Kurgu Dili
- **Tempo & Kesim Sıklığı:** Ölçülen vuruş aralığı **0.4295 sn ≈ 139.7 BPM**. 31.53 sn'lik edit boyunca **59 plan** var: neredeyse tamamı **1 vuruş = 1 plan** (0.43 sn), arada 2 vuruşluk ve 6 vuruşluk nefes alanları (6.15→8.77 sn, 14.4→15.65 sn, 23.4→24.7 sn).
- **Easing / Speed-Graph Karakteri:** Plan içinde belirgin speed-ramp yok; enerji kurgudan gelir. Tipografi ise harf başına **~0.03–0.06 sn gecikmeli** belirir (stagger), 0.05–0.10 sn içinde tam opaklığa ulaşır.
- **Ses Senkronu:** Kesmeler vuruşa kilitli; kelimeler vokal cümle sınırlarında değişir ve **plan değişse bile ekranda kalır** (tipografi zaman çizgisi kurgudan bağımsızdır). Bazı kesmelerde kısa beyaz/turuncu flash.

## 3. Kamera & Uzay (2D / 2.5D / 3D)
- **Uzay Modeli:** Düz 2D. Kamera hareketi yok denecek kadar az; derinlik hissi yalnızca tipografinin özne arkasında kalmasından gelir.
- **Kamera Hareket Sözlüğü:** Ara sıra hafif zoom-punch ve whip-pan blur geçişleri. Sarsıntı (shake) yok.

## 4. Görsel Katmanlar, Tipografi & Renk Paleti
- **Kaynak Görüntü:** Anime bölüm klipleri (Mushoku Tensei), 1920×1080 / 60 fps. Ara sıra **çoklu küçük panel kolajı** (beyaz çerçeveli fotoğraf kartları sahneye tek tek düşer).
- **Tipografi Dili:** Tek font ailesi: **ağır condensed grotesk, tamamı büyük harf** (Anton/Druk benzeri). Kelime başına cap-height ≈ 0.15–0.26 × kare yüksekliği. Harfler arası boşluk çok geniş (kelime kare genişliğinin %60–75'ine yayılır), harf başına **±0.02–0.09 dikey sapma**, hafif döndürme. Kelimeler ekran merkezine yakın, bazen iki satır (ALL / BACK).
- **Renk & Kontrast:** Palet: dusty blue `#7C7FD0`, krem `#EDE6D6`, kiremit `#D8402A`, turuncu `#DE7A22`, lacivert `#1E2C46`, siyah `#121212`, beyaz `#F2F2F2`. Genel grade düşük doygunluk + yüksek bloom; harflerde hafif RGB kayması ve koyu gölge.

## 5. Efekt & Geçiş Tarifi (Reçete)
- **Temel Post-FX Zinciri:** Kaynak → tipografi katmanı (özne maskesiyle kısmi örtme) → HDR bloom/deep glow → film optics (halation + 35mm grain + vinyet + ince chromatic edge) → kontrast/doygunluk → motion blur.
- **Geçişler:** Çoğunlukla sert kesme; %25'inde 0.05–0.14 sn whip/chromatic blur, cross-zoom, burn-flash.
- **Atmosfer / Dekoratif Katmanlar:** Bol halation, yumuşak bloom, grain; parçacık/şerit yok. Ara sıra siyah-beyaz (desatüre) bölümler.

## 6. FrameShift Yetenek Eşleşmesi
| Teknik | Durum | FrameShift Karşılığı / Eksik Notu |
| :--- | :--- | :--- |
| Tam kare dev 2D kelime | ✅ | `vector_text` + `font_path` (Anton) |
| Harf başına serbest X/Y | ⚠️ | Şemada yok; **harf başına bir `vector_text` düğümü** + `x_offset_norm`/`y_offset_norm` ile kuruldu |
| Harf başına döndürme | ⚠️ | `vector_text`'te rotasyon yok; `transform2_d` zinciri gerekir |
| Harf başına renk | ⚠️ | `text_3d` `color_mode=1` + ≤4 renk paleti; `vector_text`'te tek renk → düğüm başına harf |
| Metne özel RGB split | ❌ | Yalnız tam kare `film_optics.chromatic_edge`; metin katmanına `Precomp`+`FilmOptics` gerekir |
| Metnin özne arkasında kalması | ⚠️ | Gerçek depth yok; `mask_matte` (luma) + kısmi blend ile yaklaşıklama |
| Kolaj / çoklu panel | ⚠️ | `precomp` + `transform2_d` + `mask_matte`; kırpma düğümü yok |
| Beat'e kilitli kesme | ⚠️ | `peaks` ölçüyor ama kesme üretmiyor; kesme listesi dışarıda hesaplanmalı |
| Whip / zoom / burn geçiş | ✅ | 10 yerleşik geçiş shader'ı |

## 7. Ajan İçin Uygulama Kuralları (Do / Don't)
- **Yap (Do):** Tek font kullan; kelimeyi kare genişliğinin ~%70'ine yay; harf başına küçük dikey sapma ver; kelimeyi en az 2–3 plan boyunca ekranda tut; tipografiyi koyu silüetlerin arkasında bırak; bloom'u cömert, doygunluğu düşük tut.
- **Yapma (Don't):** Her plana yeni kelime koyma (kelime plan değişiminden uzun yaşamalı); kelimeyi küçük ve ortada bitişik bırakma; renkli gradient/outline yığma; parçacık ve HUD ekleyip sadeliği bozma; harfleri kare kenarından taşırma (en dış harf merkezi %10–90 aralığında kalmalı).

## 8. Epistemik Sınır
- Videodan doğrudan gözlenen: kesme sayısı ve zamanları (kare farkı analizi), vuruş ızgarası (spektral flux), kelime listesi ve renkleri (2/4/8 fps kare ızgaraları), tipografinin özne arkasında kaldığı (kare kare görsel doğrulama).
- Çıkarım: font kimliği (Anton benzeri olduğu tahmini), kelimelerin tam opaklık eğrileri, kullanılan ticari eklenti veya uygulama. Kaynak proje dosyası yoktur.
- Kesintisiz oynatım ve ses dinleme yapılmadı; **akıcılık ve dudak/söz senkronu doğrulanmış değildir**.
