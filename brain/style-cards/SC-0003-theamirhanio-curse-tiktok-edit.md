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
tags: [tiktok-anime-edit, fx-stack, impact, reference-pending-visual-review]
---

# SC-0003 — Curse / efekt odaklı TikTok anime edit referansı

**Durum:** Ön kart. Kullanıcının hedefi ve teknik ölçümler kaydedildi; görsel/işitsel çözümleme tamamlanmadı. Bu belgeyi hazır efekt reçetesi gibi kullanma.

## 1. Kimlik ve hedef

- [Kullanıcının örneği](https://www.tiktok.com/@theamirhanio/video/7679962646442953992), [hesap dizininde](https://tokgauge.com/@theamirhanio) “Curse. #madokamagica #animeedit #fyp” olarak listeleniyor.
- Kullanıcının tarifi: genel bir AMV veya sırf kinetik tipografi değil; kısa sosyal anime editinde yoğun, katmanlı efektler yapabilmek. “TikTok” burada tür ve kurgu dili; sabit 9:16 şartı değil. İndirilen dosya **1260×1080 (7:6), 24 fps, 404 kare, 16.833333 sn, HEVC + AAC 44.1 kHz**.

## 2. Zamanlama ve ritim — ölçülenler

| Ölçüm | Sonuç | Sınır |
| --- | --- | --- |
| `blackdetect`; en az %98 koyu piksel, yaklaşık 0.08 sn minimum | 0.000–0.375 ve 4.083–4.875 sn | Bunun tasarlanmış bekleme mi, görüntü mü olduğu görsel inceleme gerektirir. |
| `scene > 0.12` kare değişim skoru | 4.875 sn öncesi 17; 4.875 sn ve sonrası 101 aday. Yaklaşık **2,4×** aday/saniye farkı. | Kesim sayısı değildir. Flaş, hızlı hareket ve sahne değişimi benzer aday üretir. Öznel ritim veya beat/drop doğrulaması değildir. |

Önerilen ilk inceleme penceresi **3.8–6.5 sn**: daha sakin bölüm ile daha yoğun kare değişiminin sınırını içeriyor. Vuruş ve efekt onset işaretleri ancak sesli izleme sonrası onaylanmalı.

## 3. Kamera, katmanlar ve efektler — görsel inceleme bekliyor

Kamera hareketlerinin türü, özgün palet, tipografi, gerçek geçiş yöntemi ve kaynaktaki efekt zinciri doğrulanmadı. Çıkarılan kare şeritleri mevcut oturumda görsel olarak okunamadı. Aşağıdaki tasarım, **örnekte tespit edilmiş efektler değil**, istenen tür için yeniden kullanılabilir araç önerisidir.

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
