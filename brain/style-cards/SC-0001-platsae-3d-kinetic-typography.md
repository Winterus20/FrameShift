---
id: SC-0001
title: PlatsAE 3D Neon Kinetic Typography
creator: "@platsae"
source_url: "https://www.tiktok.com/@platsae/video/7648921302203452680"
date: 2026-11-21
category: kinetic-typography
energy: high
fps: 60
aspect_ratio: "16:9"
frameshift_readiness: partial # Faz 6.2 kısmi -> Faz 6.3 hedefi
tags: [kinetic-typography, 3d-camera, deep-glow, particles, ribbons, motion-blur, lyric-sync, shape3d]
---

# SC-0001 — PlatsAE 3D Neon Kinetic Typography

## 1. Kimlik & Genel His (Ne Zaman Seçilmeli?)
- **Özet His / Estetik:** Koyu uzay/siyah zemin üzerine yüksek doygunluklu neon renkler (pembe, cyan, sarı, yeşil, kırmızı), 3D uzayda uçuşan büyük kinetik tipografi, yoğun HDR Deep Glow, parlayan yıldız/kare/kalp şekilleri, akan ışık şeritleri ve agresif 60fps kamera hareketleriyle kurulu yüksek enerjili After Effects motion graphics tarzı.
- **Uygun Kullanım Senaryoları:**
  - Belirgin ve ritmik vokalli hızlı EDM / pop / hiperpop / phonk parçalarında tam ekran lyric/typography editleri.
  - Anime AMV'lerin yüksek enerjili nakarat (chorus/drop) veya açılış (intro) sekanslarında klipler arası tipografi patlamaları.
- **Uygun Olmayan Senaryolar:** Yavaş, melankolik, minimalist veya sinematik hikâye anlatımlı sahneler.

## 2. Zamanlama, Ritim & Kurgu Dili
- **Tempo & Kesim Sıklığı:** Çok yüksek; neredeyse her 0.25–0.8 saniyede bir yeni kelime, kamera sıçraması veya vurgu değişimi (28.1 sn boyunca kesintisiz akış). Klasik düz kesmeler yerine sahneler **L-köşesi (L-junction)** boyunca kesintisiz kamera akışıyla bağlanır.
- **Easing / Speed-Graph Karakteri:** Sert fırlama + ani frenleme (yüksek peak velocity, hızlı in/out `ease_out_expo` / `ease_in_out_expo`). Harf girişlerinde **overshoot'lu pop-in (`0 -> 1.15 -> 1.0`, `ease_out_back`)**; lineer hareket neredeyse hiç yok, her kamera geçişi "whip" hissi veriyor.
- **Ses Senkronu:** Birincil olarak **kelime/hece ve harf düzeyinde vokal senkronu**:
  - Tam kelime darbeleri (`RIVAL`, `IDOL`, `SUICIDAL`, `LEGENDARY`, `BANG!`, `DOWN!`, `I WANT YOU IN THE GROUND`) + sert vuruşlarda geriye doğru **Z-echo derinlik yankı kopyaları** (`BANG!`).
  - Hece/harf ritmine kilitli **gecikmeli harf açılışı (staggered letter reveal)** (`D-y-i-n-g`, `U-N-T-I-L`).
  - İkincil olarak sert kick/beat vuruşlarında kamera sarsıntısı ve beyaz exposure flash.

## 3. Kamera & Uzay (2D / 2.5D / 3D)
- **Uzay Modeli:** Tek bir **paylaşımlı 3D sahne ve ortak Z-derinlik tamponu (`Scene3D` + `Depth32Float`)** içinde konumlandırılmış metin, `Shape3D` prop'lar, parçacıklar ve şeritler. Ön/arka nesneler birbirini gerçek Z-oklüzyonuyla keser.
- **Kamera Hareket Sözlüğü:**
  - **90° Kamera L-Kavşakları (L-Junctions):** Kelimeler yalnızca düz Z-düzlemine dizilmez; 3D uzayda X/Y/Z eksenlerinde **90° dik açılı köşeler** oluşturacak şekilde yerleştirilir. Kamera bir kelimeden diğerine 90° whip-orbit ile dönerken önceki kelime kadraj kenarında DoF ile bulanıklaşarak çıkar.
  - Kelimeler arası hızlı **Z-derinliği uçuşu (flythrough)** ve ani zoom-in / zoom-out.
  - Sık **Dutch tilt (Z-roll)** ve Y/X ekseninde keskin bakış açısı dönüşleri.
  - Her güçlü hecede (`BANG!`) kısa, yüksek frekanslı **impact shake**.
  - Kameraya çok yakın/uzak metin ve `Shape3D` nesnelerinde belirgin **Z-buffer Bokeh Depth of Field**.

## 4. Görsel Katmanlar, Tipografi & Renk Paleti
- **Kaynak Görüntü:** %100 prosedürel tipografi, 3D vektör/geometrik primitifler ve parçacıklar (video klibi yok).
- **Tipografi Dili:**
  - Tek fonta saplanılmaz: Kalın geometrik sans-serif (`BANG!`, `DOWN!`), yuvarlak/oyuncu sans (`Dying`), dar/condensed (`I WANT YOU IN THE GROUND`) sahneye göre değişir.
  - **Harf Bazlı Kinetik & Çoklu Renk:** Her harf kendi pivotu etrafında `0 -> 1.15 -> 1.0` pop-in, Z/Y/X spin-in ve farklı neon palet döngüsü taşır (`R-I-V-A-L`, `B-A-N-G!`, `D-y-i-n-g`).
  - **Yay/Kavisli Taban Çizgisi:** Vurgu cümlelerinde (`YOUR RAGE`) harfler dairesel yay boyunca dizilir.
  - **3 Materyal Stili & Dış Kontur:** (1) `Neon 3D` (ekstrüzyon + HDR rim), (2) `Metallic Chrome` (yüksek kontrastlı krom yansıma), (3) `Flat-2D Stroked` (keskin beyaz/neon dış konturlu düz vektör).
  - **Z-Echo Derinlik Klonlama:** Güçlü drop kelimelerinin (`BANG!`) arkasında `-Z` ekseninde küçülerek/solarak dizilen 3–6 yankı kopyası.
- **Dekoratif & Kelime-Birebir (Word-Literal) 3D Şekiller (`Shape3D`):**
  - Arka/orta planda yüzen **kalın parlak beyaz/neon çerçeveli renkli kare ve dikdörtgen paneller (`NeonTile`)**.
  - Dönen **içi boş 5-köşeli neon kontur yıldızlar (`HollowStar5`)**.
  - **Kelime anlamıyla eşleşen 3D prop'lar:** Aşk/kalp sözlerinde beveled **3D kalpler (`Heart3D`)**, enerji/şok vurgularında **neon zigzag şimşekler (`LightningBolt`)**, parıltı anlarında **fasetli elmaslar (`Diamond3D`)**.
- **Renk & Kontrast:**
  - Ana zemin koyu siyah/lacivert uzay; ön planda yüksek parlaklıklı HDR neon renkler (magenta, cyan, elektrik sarısı, lime yeşil, ateş kırmızısı).
  - Bölüm geçişlerinde tematik renk değişimi: örn. 00:07–00:08 ateş/turuncu kıvılcım, 00:12 kırmızı kalpler, 00:17–00:19 parlak beyaz/pastel flash + renkli arka plan.

## 5. Efekt & Geçiş Tarifi (Reçete)
- **Temel Post-FX Zinciri:**
  `Paylaşımlı Scene3D (Extruded/Stroked Metin + Shape3D + Parçacık + Üçlü Paralel Ribbon, tek Depth32Float)` → `Temporal Shutter Motion Blur` → `Multi-Pass HDR Deep Glow (Bloom)` → `Z-Buffer Bokeh DoF` → `Chromatic Aberration (RGB Split)` → `Brown-Conrady Lens Distortion (Optics Comp.)` → `Beat Exposure Flash / Impact Shake`.
- **Geçişler:** Klasik crossfade yok; tüm geçişler 90° L-junction whip-orbit, hızlı Z-flythrough ve beyaz exposure patlamalarıyla (flash) birbirine bağlanıyor.
- **Atmosfer / Dekoratif Katmanlar:**
  - Sürekli akan yıldız tozu, kıvılcım, bokeh ve sahneye özel parçacıklar (`Heart`, `Star4`, `Diamond`).
  - **Üçlü Paralel Trim-Path Şeritler (`RibbonTrail3D`):** Karışık Lissajous yumakları yerine, kelimelerin ve yıldızların etrafından tek bir akıcı 3D S-eğrisi/helis boyunca yan yana sabit aralıkla akan **3 paralel ince neon fiber şerit**.
  - Arka ve orta derinlikte dönen/ölçeklenen `NeonTile`, `HollowStar5`, `Heart3D` ve `LightningBolt` katmanları.

## 6. FrameShift Yetenek Eşleşmesi (Faz 6.2 → Faz 6.3)

| Teknik | Durum | FrameShift Karşılığı / Faz 6.3 Hedefi |
| :--- | :--- | :--- |
| 60fps kesin zamanlama & beat/RMS | ✅ Var (M1–M5) | `Flicks(i64)`, `AudioRmsEnvelope`, `SafeExpr` (Faz 6.3'te `exp`, `ease_out_back`, `ease_*_expo` eklenecek) |
| Temporal Shutter Motion Blur | ✅ Var (M1) | `motion_blur_accumulate.wgsl` |
| Impact Shake, RGB Split, Optics Comp. | ✅ Var (Faz 6.1) | `fx_motion_tile_shake.wgsl`, `transition_whip_chromatic.wgsl`, `fx_film_optics.wgsl`, `color_basic.wgsl` |
| Multi-Pass HDR Deep Glow (Bloom) | ✅ Var (Faz 6.1) | 5-mip Jimenez 2014 PBR Bloom (`fx_deep_glow.wgsl`) |
| 3D Kamera & Tekil `Text3D` / `Transform3D` | ⚠️ Faz 6.2 Kısmi | Tekil düğümler var; ortak `Scene3D` birleşik `Camera3D` (90° L-junction) + paylaşımlı `Depth32Float` eksik → [Part A](../research/faz6-3-scene3d-dof-camera.md) |
| Z-Buffer Bokeh DoF | ⚠️ Faz 6.2 Kısmi | Yalnız `Transform3D` ve `ParticleField3D` içinde var; `Text3D` ve `Shape3D` için sahne geneli Z-buffer DoF eksik → [Part A](../research/faz6-3-scene3d-dof-camera.md) |
| Gelişmiş `Text3D` (Pop-in, Palet, Stroke, Yay, Chrome, Z-Echo) | ⚠️ Faz 6.2 Kısmi | Temel TTF extrusion + sinüs dalgası var; `ease_out_back` pop-in, çoklu renk, `StrokeTessellator` kontur, yay taban, `MetallicChrome` ve instanced Z-echo eksik → [Part B](../research/faz6-3-text3d-shapes-ribbons.md) |
| Prosedürel `Shape3D` (`NeonTile`, `HollowStar5`, `Heart3D`, `LightningBolt`, `Diamond3D`) | ❌ Faz 6.3 Hedefi | `frameshift-vector` + `GpuMeshVertex3D` ortak pipeline'ında inşa edilecek → [Part B](../research/faz6-3-text3d-shapes-ribbons.md) |
| Üçlü Paralel Trim-Path `RibbonTrail3D` | ⚠️ Faz 6.2 Kısmi | Mevcut `ribbon_trail3d.wgsl` yalnızca Lissajous çiziyor; `parallel_triple_bundle` (S-eğrisi/helis 3 paralel fiber) eksik → [Part B](../research/faz6-3-text3d-shapes-ribbons.md) |
| Stateless 3D Parçacıklar | ✅ Var (Faz 6.2) | `particle_field3d.wgsl` (bokeh, star4, diamond, heart + DoF) |
| Kelime Düzeyinde Söz Zamanlama | ⚠️ Çekirdek Dışı | Dış araçla (`faster-whisper` / `Demucs` + `stable-ts`) JSON üretilip `Flicks` olaylarına aktarılabiliyor |

## 7. Ajan İçin Uygulama Kuralları (Do / Don't)
- **Yap (Do):**
  - Kelimeleri düz tek bir Z-düzlemine dizmek yerine **90° kamera L-kavşakları** kur ve kamerayı whip-orbit ile köşelerden geçir.
  - Her kelimeye düz metin bloğu değil, **harf bazlı `ease_out_back` pop-in (`0 -> 1.15 -> 1.0`), spin-in, çoklu neon palet ve vurgu kelimelerinde Z-echo yankı kuyruğu** ver.
  - `RibbonTrail3D` için dağınık Lissajous yerine **`parallel_triple_bundle` (3 paralel ince neon şerit)** ile kelimeleri çerçevele.
  - Arka ve orta derinlik katmanlarını **`NeonTile` (kalın beyaz çerçeveli renkli paneller)**, **`HollowStar5` (içi boş neon yıldızlar)** ve şarkı sözüne birebir oturan **word-literal `Shape3D` prop'larla** (`Heart3D`, `LightningBolt`, `Diamond3D`) doldur.
  - Sahne genelinde ortak `Scene3D` derinlik tamponu + Z-buffer Bokeh DoF kullan; kısa beyaz exposure flash'larla beat'i patlat.
- **Yapma (Don't):**
  - Lineer (sabit hızlı) kamera veya yazı hareketi kullanma — anında amatör görünür.
  - Spagetti gibi birbirini kesen düzensiz Lissajous şerit yığınları bırakma.
  - Tüm videoyu tek font, tek materyal ve tek renkte bırakma; `Neon3D`, `MetallicChrome` ve `Flat2DStroked` arasında bölüm/vurgu geçişi yap.
  - Söz zamanlamasını tahmini sabit aralıklarla yapma; mutlaka kelime-milisaniye zaman damgasına bağla.

## 8. Epistemik Sınır
- Gözlemler `tiktok_video.mp4` (`1920×1080`, `60fps`, `28.10 sn`) dosyasının doğrudan izlenmesi ve `ffprobe` çıktısına dayanır.
- Orijinal After Effects proje dosyası (`.aep`) elde olmadığından hangi ticari eklentilerin (ör. Deep Glow, Trapcode Particular, Element 3D, Saber/3D Stroke vb.) kullanıldığı kesin değildir; yukarıdaki reçete görsel davranışın FrameShift Faz 6.2 → Faz 6.3 mimarisine açık kaynaklı eşdeğer uyarlamasıdır.
- Tablodaki Faz 6.3 hedefleri henüz uygulanmamış araştırma şartnamesidir; yalnızca Faz 6.2 satırları mevcut kodu yansıtır.
