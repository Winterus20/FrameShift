---
title: "TikTok, Instagram ve YouTube Ünlü Anime Editörlerinin İş Akışı ve Stilleri"
date: 2026-11-24
status: active
tags: [agent, shaders, timebase, render-graph, ui, media-io]
---

# TikTok, Instagram ve YouTube Ünlü Anime Editörlerinin İş Akışı ve Stilleri

## Soru / Amaç

TikTok, Instagram Reels ve YouTube'da öne çıkan anime editörlerinin (AMV ve kısa form edit üreticilerinin) kullandığı ana stilleri, araç/eklenti ekosistemini, ses tasarımı ve hız eğrisi disiplinini çözümlemek; ortaya çıkan teknik gereksinimleri FrameShift'in mevcut motor ve ürün yetenekleriyle karşılaştırmak.

## İncelenen Birincil Kaynaklar

- [2026-11-24 tarihli web araştırması ve topluluk breakdown özetleri](https://www.youtube.com/) — erişim/inceleme tarihi: 2026-11-24 (Adobe After Effects, Sapphire, BCC, Twixtor, RSMB, Magic Bullet Looks, Blender, Flowframes ve Alight Motion/CapCut iş akışı özetleri; Xenoz, Molob, Nep/Neptune breakdown özetleri).
- Yerel referans stil kartları — erişim tarihi: 2026-11-24: [SC-0001 PlatsAE](../style-cards/SC-0001-platsae-3d-kinetic-typography.md), [SC-0002 Asgardski](../style-cards/SC-0002-mushoku-tensei-kinetic-type-amv.md), [SC-0003 Curse](../style-cards/SC-0003-theamirhanio-curse-tiktok-edit.md).

> **Not:** Bu çalışma ikincil web araştırması ve topluluk breakdown özetlerine dayanmaktadır. Bu oturumda hiçbir editörün `.aep` proje dosyası veya indirilen videosu doğrudan kare-kare incelenmemiştir.

## Doğrudan Gözlemler (Web Araştırması Özeti)

### 1. Öne Çıkan Editör Ekolleri
- **Xenoz (`@xenoz` / `@xenozedit`):** Modern AMV sahnesinde *flow + hard impact* standardının önde gelen temsilcilerinden. Keskin ritim senkronu, klip içi hareket yönünü sürdüren kamera pan/zoom geçişleri, sert ama kontrollü impact shake'ler ve yüksek kontrastlı sinematik renk (CC) diliyle tanınır.
- **Molob (`@molob`):** Temiz maskeleme, 3D metin entegrasyonu, kompozisyon disiplini ve "1 saatte X tarzı edit" gibi teknik breakdown/proje dosyası (`.aep`) paylaşımlarıyla After Effects topluluk iş akışını etkileyen isimlerdendir.
- **Nep / Neptune (`@zneptun`):** Kendi adıyla anılan *Nep style* alt türünün öncüsüdür. Hızlı **one-framer** (tek karelik kesme/flaş), stutter efektleri ve yoğun kinetik tipografi ile yüksek tempolu kısa editler üretir.
- **Kısa Form Tipografi ve Sosyal Efekt Ekolleri (yerel kartlar):** `SC-0001` (PlatsAE 3D neon kinetik tipografi), `SC-0002` (`@asgardski` tam kare 2D harf-harf tipografi ve özne arkası metin yaklaşıklaması) ve `SC-0003` (`@theamirhanio` siyah bekleme, kısa bulanık giriş, karakter motifi ve ışık darbeleri).

### 2. Başlıca 5 Anime Edit Stili
1. **Flow:** Kesintisiz hareket sürekliliği; sert glitch yerine klip içi hareket momentumuna uyan match-cut, akıcı pan/zoom ve pürüzsüz velocity eğrileri.
2. **Jugg:** Vuruşa oturan hızlı zoom'lar, belirgin ekran sekmesi (bounce), keskin slow-mo/speed-up (Twixtor) ve ritmik impact shake'ler.
3. **Raw / Daddy:** Agresif, kaotik ve çok yönlü kamera hareketleri; ağır distorsiyon, keskin twitch, yoğun hız sıçramaları ve koyu/yüksek kontrastlı CC (çoğunlukla phonk/bass ağırlıklı müzikle).
4. **2D/3D Typography:** Şarkı sözleri veya karakter repliklerinin sahnenin ana unsuru olduğu stil; 3D kamera, DoF ve rotoscope ile metin karakterin arkasına/etrafına yerleştirilir.
5. **3D / Blender & Manga Hibrit:** Karakter Roto Brush/maske ile ayrılır; arka plan Blender'da 3D geometriye yansıtılır (*camera mapping*), 3D ışık ve parçacık eklenir veya siyah-beyaz manga panelleri 3D uzayda canlandırılır.

### 3. Standart 6 Adımlı Üretim İş Akışı
1. **Ham Kaynak ve Ön İşleme:** Crunchyroll/Netflix sıkıştırmalı yayınları yerine Discord/Drive arşivlerinden yüksek bitrate Blu-ray (`twixtor-ready`) 1080p/4K klipler seçilir. 24 fps animenin yavaş çekimde takılmaması için **Flowframes (RIFE AI)** veya **Topaz Video AI** ile 60+ fps'e ön interpolasyon/upscale uygulanır.
2. **Ses Haritası ve SFX Katmanlama:** Kurgu müzik ve ses tasarımı üzerine kurulur. **Riser** zirvesi tam kesim karesine, **whoosh** tepe noktası (transient merkezi) geçişin merkez karesine oturtulur. **Bass drop / impact** öncesinde 1–2 karelik mikro-sessizlik bırakılıp sub-bass + metalik/darbe SFX'i bindirilir. Karakter replikleri AI ile ayrıştırılıp intro veya drop önüne alınır.
3. **Time Remapping, Twixtor ve Speed Graph:** Lineer keyframe kullanılmaz. **Time Remapping + Twixtor** ile vuruş anında ani hızlanma, hemen ardından aşırı yavaşlama yapılır (morphing/bozulmayı azaltmak için uzuv maskeleri veya track point kullanılır). Zoom/pan/shake için AE **Graph Editor (Speed Graph)** içinde *"anime snap"* adı verilen sivri dağ eğrileri kurulur: hareket yavaş/durağan başlar, geçiş karesinde maksimum hıza fırlar ve aniden frenler.
4. **Kamera, Maskeleme ve Motion Blur:** 2D kliplere derinlik vermek için 3D kamera + **Null Object** rig'leri kurulur. **Roto Brush** veya manuel pen maske ile özne arka plandan ayrılır (metin ve şok dalgaları karakterin arkasında kalır). Hızlı kamera hareketlerindeki kesikliği gidermek için **RSMB (RealSmart Motion Blur)** uygulanır.
5. **Adjustment Layer Yığını ve Eklentiler:**
   - **Sapphire (`S_Shake`, `S_BlurMoCurves`, `S_Glow`, `S_Glitch`)** ve **BCC (`Lens Flare`, Optical Glow)**: Geçişler, ritmik sarsıntılar ve ışık darbeleri.
   - **Deep Glow**: Fiziksel ışık düşüşüyle göz ve enerji parlamaları.
   - **Element 3D / Trapcode Particular**: 3D tipografi ve aura/toz/kıvılcım parçacıkları.
   - **Magic Bullet Looks (MBL)**: En üst adjustment layer'da farklı anime kaynaklarını tek görsel dilde birleştiren global renk (CC), siyah ezme, chromatic aberration ve film grain.
6. **Export ve Mobil Karşılığı:** Sosyal medya yeniden sıkıştırmasını aşmak için yüksek bitrate H.264 veya ProRes → Media Encoder çıkışı alınır. Mobilde aynı görsel dil **Alight Motion** (XML preset ve manuel velocity/graph eğrileri) ve **CapCut** üzerinden uyarlanır.

## Çıkarımlar ve Öneriler (FrameShift Eşleşmesi)

- **Halihazırda Güçlü Olduğumuz Alanlar (Çıkarım):** FrameShift'in GPU render grafiğinde (`Rgba16Float`), `AgX`/tonemap, anamorfik `DeepGlow`, `FilmOptics` (grain/CA/halation), `MotionTileShake`, `RadialShockwave`, `AnimeImpactFrame`, aspect-correct shader geçişleri, `MotionBlurShutter`, `Scene3D`/`Text3D`/`Shape3D`/`ParticleField3D`/`RibbonTrail3D` ve `SafeExpr` hız/easing altyapısı mevcuttur.
- **Ünlü Editör Kalitesini Belirleyen 3 Kritik Açık (Çıkarım):**
  1. **Gerçek karakter ayrımı (rotoscope / harici alfa matte):** Metin ve post-FX katmanlarını karakterin arkasında temizce maskelemek için `MaskMatte` mevcut olsa da gerçek özne segmentasyonu/alfa matte hattı eksiktir (`SC-0002` luma yaklaşıklaması sınırlıdır).
  2. **Etkileşimli Speed Graph + canlı scrub:** *"Anime snap"* benzeri sivri velocity eğrilerini görsel olarak ayarlamak için canlı masaüstü arayüzü (`winit`/`egui`) henüz yapılmamıştır; şu an yalnızca headless projeksiyon ve zamanlama API'si bulunur.
  3. **Optik akışlı yavaş çekim (Twixtor / RIFE benzeri) ve çok kanallı SFX katmanlaması:** Riser/whoosh/impact ses kanallarını olaylara göre düzenleyen üst düzey ses miksajı ve kare interpolasyonu yerleşik değildir.

## Doğrulanmamış Noktalar

- Bu rapor ikincil web aramaları ve topluluk breakdown özetlerinden derlenmiştir; adı geçen editörlerin ham `.aep` proje dosyaları açılıp keyframe değerleri doğrudan okunmamıştır.
- Gerçek zamanlı scrub p50/p95 gecikmesi ve hedef editörlerden seçilecek birebir referans videoların kare/ses ölçümleri (ör. `ffmpeg` + `ffprobe` onset analizi) henüz yapılmamıştır.
