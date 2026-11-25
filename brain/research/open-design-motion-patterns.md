---
title: OpenDesign / HyperFrames Motion Tasarım Kalıpları & Stil Rehberi (FrameShift Uyarlaması)
status: active
date: 2026-11-25
source_repo: https://github.com/nexu-io/open-design
license: Apache-2.0
tags: [agent, shaders, text-vector, render-graph, licensing]
---

# OpenDesign / HyperFrames Motion Tasarım Kalıpları & Stil Rehberi

Bu belge, **Apache-2.0** lisanslı [`nexu-io/open-design`](https://github.com/nexu-io/open-design) deposundaki HyperFrames ve OpenMotion dokümantasyonundan FrameShift ajanlarının doğrudan kullanabileceği **motion kuralları, görsel stil arketipleri, kompozisyon ilkeleri ve OpenMotion referans özeti** derlenerek oluşturulmuştur.

> **Kapsam Sınırı:** OpenDesign'ın HTML/CSS/GSAP render katmanı FrameShift'in native `wgpu`/WGSL motoru için **kullanılmaz** (`AGENTS.md` — web/DOM katmanlı yaklaşımlara dönmeme kuralı). Buraya yalnızca **agnostik motion/tasarım ilkeleri**, **FrameShift `NodeKind`/`SafeExpr`/WGSL karşılıkları** ve **referans kataloğu özeti** alınmıştır.

---

## 1. Birincil Kaynaklar (Apache-2.0)

- Depo & Lisans: [`nexu-io/open-design`](https://github.com/nexu-io/open-design) ([`LICENSE`](https://github.com/nexu-io/open-design/blob/main/LICENSE))
- Motion İlkeleri: [`design-templates/hyperframes/references/motion-principles.md`](https://github.com/nexu-io/open-design/blob/main/design-templates/hyperframes/references/motion-principles.md)
- Görsel Stil Kütüphanesi: [`design-templates/hyperframes/visual-styles.md`](https://github.com/nexu-io/open-design/blob/main/design-templates/hyperframes/visual-styles.md)
- Veri Hareketi (Data in Motion): [`design-templates/hyperframes/data-in-motion.md`](https://github.com/nexu-io/open-design/blob/main/design-templates/hyperframes/data-in-motion.md)
- House Style & Anti-Pattern'ler: [`design-templates/hyperframes/house-style.md`](https://github.com/nexu-io/open-design/blob/main/design-templates/hyperframes/house-style.md)
- OpenMotion 286 Referans Kataloğu: [`docs/motion-templates/catalog.json`](https://github.com/nexu-io/open-design/blob/main/docs/motion-templates/catalog.json)

---

## 2. Ajan Motion Koruma Kuralları (Guardrails)

`Motion.md` DSL veya `Project` grafiği üretirken her sahnede aşağıdaki tekdüzelik tuzaklarından kaçın:

1. **Aynı easing'i her düğüme basma:** Bir sahnede aynı easing profilini en fazla 2 bağımsız animasyonda kullan. Girişler **`.out`** (hızlı başla, yavaşlayarak otur — `CubicBezierSpeed` / `expo.out`), çıkışlar **`.in`** (yavaş başla, hızlanarak çık), konum geçişleri **`.inOut`** olmalıdır.
2. **Aynı süreyi kullanma:** Tüm animasyonları 0.4–0.5 sn yapma. En yavaş sahne/öğe en hızlıdan en az `3×` daha yavaş olmalı:
   - Hızlı (`0.15–0.30 sn`): enerji, aciliyet, punch
   - Orta (`0.30–0.50 sn`): standart UI / kart / tipografi
   - Yavaş (`0.50–0.80 sn`): ağırlık, lüks, vurgu
   - Çok yavaş (`0.80–2.00 sn`): sinematik, atmosferik
3. **Her şeyi aynı yönden sokma:** Hep `y + opacity` kullanma; soldan, sağdan, `scale`, `letter-spacing` veya saf `opacity` ile çeşitlendir.
4. **`t = 0` anında başlatma:** İlk animasyona `0.1–0.3 sn` ofset ver (aksi halde jump-cut hissi verir; istisna: beat-locked hard-cut AMV açılışları).
5. **Sahne yapısı (Build / Breathe / Resolve):**
   - **Build (`%0–30`):** Öğeler önem sırasına göre kademeli (staggered) girer; toplam stagger süresi öğe sayısından bağımsız `< 500 ms` tutulur.
   - **Breathe (`%30–70`):** İçerik görünürken tek bir ortam hareketi (pan, hafif rotasyon, glow nefesi veya bilinçli durağanlık) çalışır.
   - **Resolve (`%70–100`):** Çıkışlar girişlerden **daha hızlı** olmalıdır (ör. `0.40 sn` giriş, `0.25 sn` çıkış).
6. **Kompozisyon kuralları:**
   - Sahnede en az **2 odak noktası** ve **3 katman** (arka plan dokusu/glow, ön plan içerik, vurgu çizgisi/HUD/veri çubuğu) kur.
   - Saf `#000000` veya `#FFFFFF` yerine ana vurgu rengine hafifçe kırılmış tonlar kullan.
   - Veri/istatistik sahnelerinde pasta grafik, çok eksenli grafik, grid/tick kalabalığı ve 6'lı web kart ızgarası kullanma; sayıyı tek başına havada bırakma, orantılı bar/halka veya `VectorPathHud` ile destekle.

---

## 3. 8 Görsel Stil Arketipi → FrameShift Karşılıkları

| Stil | Ruh Hali | Palet (sRGB Önerisi) | Tipografi & Hareket | FrameShift Karşılıkları |
| :--- | :--- | :--- | :--- | :--- |
| **Swiss Pulse** | Klinik, keskin | `#1A1A1A`, `#F4F4F4`, vurgu `#0066FF` veya `#FFB300` | Kalın sans + büyük sayılar (`80–120px`), 12-sütun ızgara, 0'dan sayan sayaçlar | `VectorText` / `Text3D`, `SafeExpr` sayaç, sert hard-cut veya `CinematicZoom` / `ShapeMask` iris, `CubicBezierSpeed` (`expo.out`) |
| **Velvet Standard** | Lüks, zamansız | Siyah/beyaz + `#1A237E` veya `#C9A84C` | İnce ALL CAPS + geniş tracking, bol negatif boşluk, uzun beklemeler | `Text3D` ince tracking, `TransitionCrossWarp` / `DirectionalWarp`, yavaş `sine.inOut`, `FilmOptics` |
| **Deconstructed** | Endüstriyel, ham | `#1A1A1A`, `#D4501E`, `#F0F0F0` | Açılı/taşan tipografi, tarama çizgileri, glitch, scramble | `GlitchDisplace`, `TransitionWhipChromatic`, `MotionTileShake`, `SafeExpr` `wiggle` / `steps` |
| **Maximalist Type** | Yüksek sesli, kinetik | `#E63946`, `#FFD60A`, siyah, beyaz | Çerçevenin `%50–80`'ini kaplayan katmanlı yazı, video üstü tipografi, 2–3 sn hızlı sahneler | `Text3D` + `TextAnimator` v2, `isnetis.onnx` matte ile özne arkası/üstü, `BlurMoCurves`, `AgxPunchy` |
| **Data Drift** | Fütüristik, akışkan | `#0A0A0A`, `#7C3AED`, `#06B6D4` | İnce sans, mikro→makro ölçek, sayılara dönüşen parçacıklar | `ParticleField3D`, `RibbonTrail3D`, `DisplacementMap`, `DeepGlow`, `Scene3D` DoF |
| **Soft Signal** | Samimi, sıcak | `#F5A623`, `#FFF8EC`, `#C4A3A3`, `#8FAF8C` | İnsancıl serif / el yazısı, tek büyük odak, nefes alan yavaş drift | `VectorText` serif, `FrameEcho` yumuşak iz, `FilmOptics` halation, yavaş `sine.inOut` |
| **Folk Frequency** | Canlı, kültürel | `#FF1493`, `#0047AB`, `#FFE000`, `#009B77` | Kalın yuvarlak tipografi, tekrarlayan desenler, yoğun katman | `Shape3D` / `ShapeMask` desenler, `HalftoneGlow`, `OpticalFlowBlur` |
| **Shadow Cut** | Karanlık, sinematik | Koyu antrasit/siyah + tek soğuk veya kızıl vurgu | Dramatik reveal, yüksek kontrast, yavaş gerilim | `DepthMatte` + `Scene3D`, `AgX` tonemap, `DeepGlow`, `DirectionalWarp` |

---

## 4. OpenMotion Referans Kataloğu Özeti (`catalog.json`)

[`docs/motion-templates/catalog.json`](https://github.com/nexu-io/open-design/blob/main/docs/motion-templates/catalog.json) içinde **286 adet** ürün/marka motion referansı bulunur (en yoğun markalar: Figma 16, Perplexity 16, Anthropic 15, xAI/Grok 15, Cursor 14, Linear 10, OpenAI 10, Manus 10, Notion 9, Google 9).

Katalogdaki öne çıkan yapısal örüntüler (FrameShift `Motion.md` / `ParametricTemplateSpec` için):
- **Kod ↔ Kanvas / Önce–Sonra (`V099`, `V276`):** Önce sonucu göster, sonra kontrol noktalarının/parametrelerin görüntüyü nasıl değiştirdiğini yan yana kanıtla.
- **Ön/Arka Katman Ayrımı (`V102`):** Ön planda keskin ürün/klip penceresi, arka planda taşan dev tipografi dokusu; katmanları birbirine karıştırma.
- **Bağımsız İlerleme Çubuğu + Dağılan Harfler (`V104`):** Harf parçaları birleşirken ince ilerleme çizgisi bağımsız büyür; çıkışta çizgi tamamlanmadan harfler çözülür.
- **Kenar Sabit Bilgi Şeridi (`V101`):** Yoğun kesilmiş tipografi ve renk blokları değişirken alt bilgi şeridini sabit tut.
