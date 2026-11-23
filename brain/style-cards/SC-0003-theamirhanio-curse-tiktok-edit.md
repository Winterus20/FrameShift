---
id: SC-0003
title: "Curse — TikTok anime edit referansı"
creator: "@theamirhanio"
source_url: "https://www.tiktok.com/@theamirhanio/video/7679962646442953992"
date: 2026-11-22
category: anime-amv
energy: high
fps: 24
aspect_ratio: "7:6"
frameshift_readiness: partial
tags: [tiktok-anime-edit, fx-stack, impact, character-motifs]
---

# SC-0003 — Curse / efekt odaklı TikTok anime edit referansı

**Durum:** 2026-11-23 tarihinde 24 fps referansın kesitleri ve ses atakları incelendi. Görsel sonuç gözlemlendi. Kaynak proje ve uygulama tekniği bilinmiyor.

## 1. Kimlik ve hedef

- [Kullanıcının örneği](https://www.tiktok.com/@theamirhanio/video/7679962646442953992), [hesap dizininde](https://tokgauge.com/@theamirhanio) “Curse. #madokamagica #animeedit #fyp” olarak listeleniyor.
- Kullanıcının tarifi: genel bir AMV veya sırf kinetik tipografi değil; kısa sosyal anime editinde yoğun, katmanlı efektler yapabilmek. “TikTok” burada tür ve kurgu dili; sabit 9:16 şartı değil. İndirilen dosya **1260×1080 (7:6), 24 fps, 404 kare, 16.833333 sn, HEVC + AAC 44.1 kHz**.

## 2. Zamanlama ve ritim — ölçülenler

| Ölçüm | Sonuç | Sınır |
| --- | --- | --- |
| `blackdetect`; en az %98 koyu piksel, yaklaşık 0.08 sn minimum | 0.000–0.375 ve 4.083–4.875 sn | Bunun tasarlanmış bekleme mi, görüntü mü olduğu görsel inceleme gerektirir. |
| `scene > 0.12` kare değişim skoru | 4.875 sn öncesi 17; 4.875 sn ve sonrası 101 aday. Yaklaşık **2,4×** aday/saniye farkı. | Kesim sayısı değildir. Flaş, hızlı hareket ve sahne değişimi benzer aday üretir. Öznel ritim veya beat/drop doğrulaması değildir. |

Önerilen ilk inceleme penceresi **3.8–6.5 sn**: daha sakin bölüm ile daha yoğun kare değişiminin sınırını içeriyor. Vuruş ve efekt onset işaretleri ancak sesli izleme sonrası onaylanmalı.

## 3. Görsel dil ve zamanlama

| Aralık | Referansta gözlenen |
| --- | --- |
| 0–4,08 sn | Koyu kehribar silüet, eğik yüz yakın planı, sonra göz, el ve kanat motifleri. Karaktere farklı açılardan tekrar tekrar dönülüyor. |
| 4,08–4,88 sn | Tam siyah bekleme. Görüntü yaklaşık 4,875 sn'de, ilk güçlü bass atağından yaklaşık 2–3 kare önce geri dönüyor. Ses atağı 4,97–4,98 sn civarında. |
| 4,88–7,9 sn | Yer değiştirme ve kısa bulanıklıkla içeri giren planlar birkaç karede netleşiyor. Yüz ve dans planları parlak/koyu darbelerle yineleniyor. |
| 8–11,25 sn | Altın zemin önünde koyu kanat silüetleri, kısa mor/kırmızı vurgu ve peş peşe göz/yüz yakın planları. |
| 11,25–16,83 sn | Dönüş ve hareket izi taşıyan sıcak yakın planlar, kısa mavi karşıtlık, üst üste binen pozlar ve karanlık karakter kapanışı. |

Kurgu motivi karakterin yüzüne ve ellerine düzenli dönüş. Aralıklı tam siyah, öncesindeki düşük ses enerjisini ilk darbeye bağlıyor. Plan başına 2–4 karelik bulanık giriş ve ardından net plan yararlı bir üretim hedefi. Renk omurgası koyu kırmızı/kehribar, kısa soğuk renk karşıtlıklarıyla korunabilir. Klipte büyük başlık kartı gözlenmedi.

## 4. FrameShift’te yapılacak ilk üretim deneyi

| İhtiyaç | Mevcut aday / geliştirme | Kabul |
| --- | --- | --- |
| Klip hareketi ve hız eğrisi | Var olan retime, transform, `MotionTileShake` ve `MotionBlurShutter` ile başla. | Bir impact tick’i taşındığında kaynak görüntü eşleşmesi ve bağlı hareket beraber taşınır. |
| Aynı anda birkaç etkiyi yazma | **Impact yığını:** olay bazında yönlü kamera darbesi, kısa parlaklık/kontrast darbesi, warp ve sönüm. Her katman ayrı düzeltilebilir; var olan node’ları kullanan tek düzenlenebilir tarif. | Tek olayla grubu kaydır, bir bileşeni kapat, tamamını tek geri alma işlemiyle kaldır. Referansta gerekli olduğu görülmedikçe yeni shader yazma. |
| Planlar arası hareket devamlılığı | Çıkış/giriş çiftini tek geçiş görünümünde düzenle: hareket yönü, merkez ve süre. `ShaderTransition` ile temporal blur birlikte test edilir. | Geçiş öncesi/sırası/sonrası tam kare ve sesli oynatım. Siyah kenar ve beklenmeyen parlaklık kırılması yok. |
| Yerel görünürlük ve ışık | `Precomp`, `MaskMatte`, `DeepGlow` ve `FilmOptics` ile bölgesel FX; gerçekten gerekiyorsa harici alfa matte. | Efekt yalnız hedef katmana veya seçili bölgeye uygulanır. Renk/luma maskesi gerçek özne ayrımı diye sunulmaz. |
| Hızlı ayar döngüsü | Başlangıç, tepe, sönüm kurşun işaretleri; bileşen bazında bypass ve 2–3 sn sesli A/B. | Kullanıcı üç öğeyi elle JSON yazmadan ayarlar: impact yeri, yoğunluğu, geçiş yönü. Kare şeridi tek başına akıcılık kanıtı sayılmaz. |

Grafik adayları: [hareket ve post-FX düğümleri](../../crates/frameshift-schema/src/graph.rs#L537-L655), [alt kompozisyon](../../crates/frameshift-schema/src/graph.rs#L1008-L1029), [inceleme sınırları](../specs/inspection-contract.md#L85-L105).

## 5. Yap / yapma

- Sakin an ile yoğun bölüm arasındaki kontrastı koru. Bütün sahneye aynı yoğunlukta shake/glow basma.
- Aynı etkiyi bütün planlara kopyalamak yerine ortak kontrol ve plan başına varyasyon kullan.
- Önce mevcut grafikle kısa A/B üret. Karşılanamayan hareketi, matte’i veya geçişi ismi ve kare aralığıyla belgeledikten sonra motor özelliği aç.
- Kaynak proje elde edilmedi; kreatörün kullandığı yazılım, eklenti, shader, ses senkronu ve kare bazında kesin efekt tarifi bilinmiyor.
