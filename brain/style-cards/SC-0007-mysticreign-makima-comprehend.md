---
id: SC-0007
title: "Mysticreign Makima — Subtitle-Led Graveyard Orbit & Typewriter Drop"
creator: "@mysticreign.alt"
source_url: "https://www.tiktok.com/@mysticreign.alt/video/7677731126709931294"
date: 2026-11-25
category: anime-amv
energy: high
fps: 30
aspect_ratio: "1:1"
frameshift_readiness: ready
tags: [anime-amv, 1:1, 30fps, makima, chainsaw-man, subtitle-led, typewriter, dark-maroon-heart, graveyard, beat-cut]
---

# SC-0007 — Mysticreign Makima: Subtitle-Led Graveyard Orbit & Typewriter Drop

## 1. Kimlik & Genel His (Ne Zaman Seçilmeli?)
- **Özet His / Estetik:** 1:1 kare, 30 fps, düşük tempolu ve "sakin ama tehditkâr" bir açılış; kelime kelime açılan beyaz altyazı, tek planlık mezarlık orbiti ve 120 BPM drop'ta 31 hızlı kesme geçen karanlık, sıcak/soğuk kontrastlı bir Makima editi.
- **Uygun Kullanım Senaryoları:** *Chainsaw Man* / Makima gibi sakin, otoriter ve "control" teması olan karakterler; 13–15 sn'lik TikTok/Reels editleri; söz veya yazı odaklı, agresif efekt yığını yerine kurgu ritmi ve tipografiyle gerilim kuran editler.
- **Uygun Olmayan Senaryolar:** Sürekli aksiyon/patlama isteyen AMV'ler, 60 fps twixtor akışı veya çok katmanlı kinetic typography.

## 2. Zamanlama, Ritim & Kurgu Dili
- **Tempo & Kesim Sıklığı:** 13.40 sn toplam. Açılış 0–3.47 sn: 0.50 / 0.67 / 1.13 sn'lik üç uzun plan (ön yüz → profil → sırt). 3.47–4.40 sn siyah bekleme. Drop 4.40–11.47 sn: 31 kesim, çoğu 7 kare (0.233 sn), ilk 20 kesimde 7/7/7/6 deseni. 11.47–12.23 sn siyah. 12.23–12.43 sn tek karelik Makima flash'ı. 12.43–13.40 sn siyah + ses kuyruğu.
- **Easing / Speed-Graph Karakteri:** Neredeyse tamamı hard-cut; yumuşak geçiş yok. Her planda çok hafif (`1.0 → 1.02–1.06`) push-in; drop'ta 1–2 karelik beyaz/pozlama flaşı.
- **Ses Senkronu:** 120 BPM; ana vuruş ızgarası `0.45 + 0.50n` sn. Açılış kesimleri 0.50 / 1.15 / 2.30 / 3.47; drop 4.40'ta başlar, 11.47'de biter. Son flash 12.23–12.43, yani ses kuyruğunun son vuruşunda.
- **Yapı:** Kelime kelime altyazı → siyah → hızlı kesimler → siyah → tek flash → siyah.

## 3. Kamera & Uzay (2D / 2.5D / 3D)
- **Uzay Modeli:** Saf 2D; 1080×1080 kareye merkez kırpılmış 16:9 anime klipleri.
- **Kamera Hareket Sözlüğü:** Açılışta tek sürekli orbit (ön → sol profil → sırt); drop'ta sabit veya çok hafif zoom-in; hiçbir planda belirgin pan/roll yok.
- **Kompozisyon:** Yüz/braid merkezde; altyazı göğüs hizasında (`y ≈ 0.50–0.54`); kalpler yanak/ağız üzerinde veya siyah karede ortada.

## 4. Görsel Katmanlar, Tipografi & Renk Paleti
- **Kaynak Görüntü:** Logosuz/altyazısız 1080p *Chainsaw Man* scenepack; mezarlık (beyaz haçlar, yeşil çim, parlak gökyüzü), kafe (sıcak ahşap/beyaz pencere), tren, orman ve göz yakın planları.
- **Tipografi Dili:** İnce/orta ağırlıklı geometrik sans (Futura/Century Gothic hissi), tamamı büyük harf, beyaz, yumuşak siyah gölge; outline yok. Açılışta kelime kelime açılır: `YOU` → `YOU NEED TO` → `YOU NEED TO COMPREHEND`. Kafede iki satır: beyaz `WHO'S IN` (`y ≈ 0.485`, ~0.032 em) ve altında koyu kızıl `CONTROL` (`y ≈ 0.539`, ~0.052 em), harf harf daktilo efektiyle açılır ve siyahtan önce tamamlanmaz.
- **Renk & Kontrast:** Sıcak planlarda amber highlight (`#ffb880`), soğuk planlarda mavi gölge (`#b8d0ff`); derin siyah gölgeler, güçlü vignette, hafif halation. Kalp ve `CONTROL` koyu bordo (`#7a2a2e`–`#993638`).
- **Kalp Motifi:** Düz, outline'sız, koyu bordo dolgulu `♥`; yanakta, ağız üzerinde veya siyah karede; 0.07–0.13 em boyutunda, 0.1 sn'lik pop-in.

## 5. Efekt & Geçiş Tarifi (Reçete)
- **Temel Post-FX Zinciri:** `Transform2D` (hafif push-in) → `ColorBasic` (exposure/contrast/saturation + 1–2 kare flash) → `AnimeSharpen` → `DeepGlow` (sıcak `1.0,0.62–0.74,0.42–0.55` veya soğuk `0.72,0.82,1.0`) → `FilmOptics` (grain 0.012, halation 0.12, vignette 0.22–0.42) → `BlurMoCurves` (yalnızca 1–2 whip/zoom planında).
- **Geçişler:** Yok; tüm kesimler hard-cut. Drop girişinde `flash_amount` ile 1–2 karelik pozlama sıçraması.
- **Atmosfer / Dekoratif Katmanlar:** `VectorText` (Slug GPU) altyazı ve kalp; harf harf `grapheme` animator; siyah `Solid` planlar.

## 6. FrameShift Yetenek Eşleşmesi
| Teknik | Durum | FrameShift Karşılığı / Not |
| :--- | :--- | :--- |
| 1:1 30fps kare format | ✅ | `timeline.resolution 1080×1080`, `fps 30/1` |
| Kelime/harf bazlı altyazı | ✅ | `VectorText` + `TextAnimatorDef` (`unit: word` / `grapheme`) |
| Türkçe/Latin dışı font ve `♥` | ✅ | `assets/fonts/ZenKakuGothicNew-Bold.ttf` (U+2665 dahil) |
| Sıcak/soğuk glow grade | ✅ | `ColorBasic` + `DeepGlow.tint_rgb` + `FilmOptics` |
| Koyu bordo kalp | ✅ | `VectorText` `♥`, `color_rgba` lineer |
| Beat-locked hard-cut | ✅ | Kare bazlı ayrı projeler + `ffmpeg concat`; tek grafta 31 kesim yerine per-shot render |
| Kare hassasiyetinde flash | ✅ | `flash_amount` + `step()` binding |

## 7. Ajan İçin Uygulama Kuralları (Do / Don't)
- **Yap (Do):** Açılışı tek sürekli orbit olarak kur (ön → profil → sırt); altyazıyı kesimlerle aynı anda ilerlet; kafede `WHO'S IN` + koyu kızıl `CONTROL` iki satırını kullan ve `CONTROL`'ü siyahtan önce bitirme; drop'u 4.40'ta başlat, 11.47'de bitir; son flash'ı 12.2–12.4 aralığına koy.
- **Yapma (Don't):** Altyazıyı tek seferde tam gösterme; `CONTROL`'ü beyaz veya parlak kırmızı yapma; açılışta hızlı kesim kullanma; siyah kuyruğu kısaltma; kalpleri parlak neon yapma.

## 8. Epistemik Sınır
- TikTok videosu (`ref.mp4`), orijinal ses (`original_sound.mp3`, 13.4008 sn) ve `ffprobe`/`scdet`/RMS analizinden doğrudan gözlemlenmiştir; orijinal After Effects/CapCut proje dosyası incelenmemiştir.
- Kesim sayıları ve kare süreleri 30 fps kare ızgarasına oturtulmuş yaklaşık değerlerdir; kaynak videonun kendi kareleri arasında 1 karelik sapma olabilir.
