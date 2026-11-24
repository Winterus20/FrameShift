---
title: Rakip Güç Analizi ve Sıradaki Yüksek Getirili Özellikler (App Power Analysis)
date: 2026-11-24
status: partially-implemented
tags: [engine, agent, shaders, ui, render-graph, timebase]
---

# Rakip Güç Analizi ve Sıradaki Yüksek Getirili Özellikler

> **Uygulama Durumu (2026-11-24):** **P1, P2, P3 ve P5** sütunları **0 yeni Cargo crate** ile (toplam 60 WGSL shader) implemente edildi ve `cargo clippy --workspace --all-targets -- -D warnings` (0 uyarı) + 452/453 workspace testi (1 önceden mevcut `HEAD` hatası) + canlı CLI smoke kapılarıyla doğrulandı ([uygulama günlüğü](../log/2026-11-24-competitive-power-roadmap-implementation.md)). **P4** (canlı `egui`/`winit` masaüstü önizleme kabuğu) kullanıcı kararıyla sonraki faza ertelendi.

## Soru / Amaç

Önceki [ürün geliştirme yol haritasında](2026-11-22-frameshift-urun-gelistirme-yol-haritasi.md) ele alınıp hâlihazırda implemente edilen dersleri (revizyonlu export, speed graph, değişiklik fişi, text animator, track matte vb.) tekrarlamadan; **OpenMotion, Kimo, HyperFrames, Remotion, FilmCraft, Gausian, Motion Canvas, MakeMyClip, After Effects, DaVinci Resolve/Fusion ve AviUtl** incelemelerinden FrameShift'e henüz girmemiş en yüksek getirili ürün ve motor özelliklerini sınıflandırmak.

## İncelenen Birincil Kaynaklar

- [2026-11-22-frameshift-urun-gelistirme-yol-haritasi.md](2026-11-22-frameshift-urun-gelistirme-yol-haritasi.md) — erişim/inceleme tarihi: 2026-11-24
- [brain/lessons.md](../lessons.md) — erişim/inceleme tarihi: 2026-11-24
- [brain/state.md](../state.md) — erişim/inceleme tarihi: 2026-11-24
- [ADR-0003: Anlamsal Edit ve Olay/Kısıt Tabanlı Zamanlama](../decisions/0003-semantic-event-constraint-timing.md) — erişim/inceleme tarihi: 2026-11-24
- [fx_optical_flow_step.wgsl](../../crates/frameshift-render/shaders/fx_optical_flow_step.wgsl) — erişim/inceleme tarihi: 2026-11-24

## Doğrudan Gözlemler (Kanıtlı)

- `crates/frameshift-render/shaders/` altında şu an **52 WGSL shader** dosyası mevcuttur (Faz 6.6 + `OpticalFlowBlur` + `BlurMoCurves` + 8 adet `fx_anime4k_*.wgsl` dahil).
- [lessons.md](../lessons.md) içinde belgelenen dört sessiz kurgu tuzağı (siyah kaynak kare, ekran dışı metin normalize koordinat hatası, sRGB/lineer renk karışıklığı, opak tabanın videoyu gizlemesi) render çıkışını başarısız saydırmadığı için otomatik yakalanmamaktadır.
- Mevcut 4 seviyeli GPU Pyramidal Lucas-Kanade optik akış shader'ı ([fx_optical_flow_step.wgsl](../../crates/frameshift-render/shaders/fx_optical_flow_step.wgsl)) piksel hız alanını üretmektedir ancak henüz 2D point/planar tracker keyframe ihracına bağlı değildir.
- [state.md](../state.md#L70) içinde `SafeExpr` ek operatörleri (`if`, karşılaştırmalar, boolean, `%`, `mix`, `floor`, `ceil`, `sqrt`, `log`) ve canlı masaüstü kabuğu açık madde olarak durmaktadır.

## Çıkarımlar ve Öneriler

*(Not: Aşağıdaki 4 başlık ve öncelik tablosu rakip karşılaştırmasına dayalı tasarım önerileridir; henüz implemente edilmemiş veya benchmark ile ölçülmemiştir.)*

### 1. Ajan Verimliliği ve Üretim Hızı (OpenMotion, Kimo, MakeMyClip, Remotion)

- **`Motion.md` Tarzı Üst Seviye Kurgu DSL'i (OpenMotion'dan):**
  - *Sorun:* Gerçek bir FrameShift editi 70–260 düğümlü büyük JSON dosyalarına ulaşıyor (ör. Mushoku 260 düğüm). LLM'ler yüzlerce düğüm ID'sini ve `inputs` bağlantısını ham JSON'da tutarken hata yapabiliyor.
  - *Alınacak özellik:* Ajanın ve insanın okuyabildiği kısa bir **deklaratif sahne reçetesi** (`shot 1 @ 0..2.4s | clip: s1 | beat_drop -> shockwave + impact_frame`) yazıp bunu **deterministik olarak FrameShift JSON DAG'ına derleyen** bir üst katman. JSON tek doğruluk kaynağı kalır; ama ajan üretkenliği 3–5× artar.
- **Vision-in-the-Loop + Otomatik Proje Linter'ı (Kimo + HyperFrames'ten):**
  - *Alınacak özellik:* Kimo'da ajan her düzenlemeden sonra tek kareyi doğrudan kendi bağlamına görsel olarak çekip doğruluyor. FrameShift'te [lessons.md](../lessons.md)'de yaşadığımız 4 sessiz tuzağı (siyah kaynak kare, ekran dışı metin koordinatı, sRGB yerine yanlışlıkla lineer olmayan renk, opak tabanın videoyu kapatması) **render öncesi yakalayan `lint_project` MCP/CLI aracı** + tek çağrıda `before/after` piksel farkı (`diff_frame`) döndüren görsel kontrol aracı eklemek ajan hata oranını dramatik biçimde düşürür.
- **NLE Dışa/İçe Aktarım Köprüsü — `.otio` / FCPXML (MakeMyClip'ten):**
  - *Alınacak özellik:* FrameShift'te ajanla yapılan beat/downbeat kesimleri, stem'ler ve söz zamanlamalarının **OpenTimelineIO (`.otio`) veya FCPXML** olarak DaVinci Resolve / Premiere / After Effects'e aktarılabilmesi (veya dışarıdan kesim alınabilmesi). Bu, FrameShift'i kapalı bir ada olmaktan çıkarıp profesyonel editörlerin iş akışına doğrudan sokar.
- **Kendi Süresini Hesaplayan Parametrik Şablonlar (Remotion'dan):**
  - *Alınacak özellik:* Remotion'daki `calculateMetadata` + tipli `props` yaklaşımı: bir AMV şablonuna yeni müzik ve klip klasörü verildiğinde, şablonun BPM'e ve downbeat sayısına göre kendi süresini, kesim sayısını ve efekt pencerelerini otomatik uyarlaması.

### 2. Motor Hızı ve Canlı Editör Deneyimi (FilmCraft, Gausian, DaVinci Resolve/Fusion)

- **GPU İçi YUV→RGB Decode + NVDEC/Vulkan Video Hattı (FilmCraft'tan):**
  - *Sorun:* FrameShift şu an `ffmpeg` pipe'ından CPU üzerinden sıkıştırılmamış `RGBA` (piksel başına 32-bit) okuyor; 1080p60/4K60'ta bellek bant genişliği darboğazı yaratır.
  - *Alınacak özellik:* FFmpeg'ten **NV12 / YUV420P (12-bit/piksel)** alıp `wgpu` içinde WGSL shader ile doğrudan `Linear Rec.709` RGB'ye çevirmek — CPU→GPU veri aktarımını **%62,5 azaltır**, özellikle çok katmanlı 60fps kurgularda render süresini ciddi kısaltır.
- **Düğüm Seviyesi İçerik-Hash'li Akıllı Önbellek / Smart Cache (DaVinci Resolve Fusion'dan):**
  - *Alınacak özellik:* `Anime4K`, `OpticalFlowBlur`, `Scene3D` veya `DeepGlow` gibi ağır düğümlerin çıktısı, `(upstream_hash + node_params + tick)` anahtarıyla RAM/disk önbelleğinde tutulur. Üstteki bir metni veya rengi değiştirdiğinizde alttaki ağır düğümler sıfırdan hesaplanmaz.
- **Mevcut Optik Akıştan 2D Point / Planar Tracker Çıkarmak (Resolve & AE'den):**
  - *Büyük fırsat:* FrameShift'te 4 seviyeli GPU Pyramidal Lucas-Kanade optik akışı ([fx_optical_flow_step.wgsl](../../crates/frameshift-render/shaders/fx_optical_flow_step.wgsl)) **zaten yazıldı**. Aynı akış vektörlerinden seçilen bir bölgenin (ör. karakterin yüzü veya kılıç ucu) kare kare hareketini çıkarıp `Null3D` veya `Transform2D`'ye keyframe olarak bağlamak çok düşük ek maliyetle AE/Resolve'un en sevilen özelliklerinden birini kazandırır.
- **İnce Canlı Masaüstü Kabuk (`winit` + `egui` + mevcut `wgpu` motoru) (Gausian & FilmCraft'tan):**
  - *Alınacak özellik:* Ağır bir Premiere klonu yerine, mevcut `frameshift-app` denetleyicisi üstüne oturan ince bir canlı pencere: gerçek zamanlı scrub, yeni gerçek PCM waveform, playhead ve "ajanın son önerisini gör → onayla / geri al" çubuğu.

### 3. Anime / AMV / Motion Graphics Yaratıcı Araçları (After Effects, AviUtl, Motion Canvas)

- **`SafeExpr` İfade Gücü — Fizik ve Deterministik Gürültü (After Effects'ten):**
  - AE editörlerinin neredeyse her editte kullandığı 3 fonksiyon şu an `SafeExpr`'de yok:
    1. **`spring(t, stiffness, damping)`:** Kinetik yazı ve kamera oturuşları için sönümlü yay fiziği.
    2. **Seek-independent `wiggle(freq, amp, seed)`:** El kamerası ve organik titreşim için deterministik 1D Perlin/Simplex gürültüsü.
    3. **Eksik dil operatörleri:** `if/else` (koşullu), `mix`, `floor`/`ceil`, `%`, `sqrt`.
- **4 Yüksek Etkili Eksik Kompozisyon Düğümü (AviUtl + AE + Alight Motion'dan):**
  1. **`DisplacementMap` (2 girdili UV kaydırma):** İkinci girdinin luma/RG kanalıyla birinci görüntüyü bükme — enerji dalgası, ısı titreşimi, su, beat-reaktif distorsiyon ve özel geçişler için evrensel yapı taşı.
  2. **`FrameEcho` / Ghost Trail:** Önceki *N* karenin renk kaymalı/şeffaf izini üst üste bindirme (dövüş/hız/anime dash sahnelerinin vazgeçilmezi).
  3. **Keyframe'lenebilir Bézier / Polygon Shape Maske:** Manga paneli bölme, diyagonal split-screen, geometrik wipe ve yerel renk düzeltme maskeleri.
  4. **Göreli Zincirli Olay Kısıtları (Motion Canvas'tan):** *"Yazı, klip geçişi %60'a ulaşınca başlasın; kamera darbesi downbeat'ten 2 kare önce hazırlansın"* gibi göreli zamanlama bağları ([ADR-0003](../decisions/0003-semantic-event-constraint-timing.md) vizyonunun tam hâli).
- **Manifest Tabanlı Genel AI Eklenti Yuvası (Gausian / ComfyUI'dan):**
  - Yarın `SAM 2`, `BiRefNet` veya yeni bir anime upscaler çıktığında Rust kodunu yeniden derlemeden küçük bir `model_manifest.json` ile sidecar'a takılabilmesi.

### 4. Bilinçli Olarak Alınmaması Gereken Tuzaklar

| Rakip Yaklaşımı | Neden Almamalıyız? |
| :--- | :--- |
| **HTML/CSS/Chromium render motoru** (Remotion / HyperFrames / OpenMotion) | Shader geçişi + sub-frame temporal motion blur, HDR linear Rec.709 ve mevcut 52 WGSL GPU avantajını yok eder. |
| **Sıfırdan saf Rust H.264/HEVC codec yazmak** (FilmCraft) | Çok yüksek bakım ve patent/uyumluluk maliyeti; FFmpeg + NVENC/NVDEC + GPU YUV shader çok daha verimli. |
| **Bulut/ücretli API bağımlı kurgu** (invideo / OpenMotion cloud) | FrameShift'in yerel, hızlı, deterministik ve sıfır API maliyetli kimliğini zedeler. |

### Özet: En Yüksek Getiri / En Düşük Efor Sıralaması

| Öncelik | Özellik | Kaynak İlham | Neden Şimdi? |
| :---: | :--- | :--- | :--- |
| **1** | **`SafeExpr` (`spring`, `wiggle`, `if`/`mix`) + `DisplacementMap` + `FrameEcho` + Shape Mask** | AE / AviUtl | Mevcut 52 WGSL altyapısıyla çok hızlı eklenir, görsel ifade gücünü katlar. |
| **2** | **`lint_project` (sessiz hata dedektörü) + `Motion.md` tarzı üst seviye reçete derleyici** | Kimo / OpenMotion / HyperFrames | Ajanların 200+ düğümlü JSON'larda hata yapmasını önler, kurgu süresini kısaltır. |
| **3** | **GPU YUV→RGB Decode + Düğüm Seviyesi Smart Cache** | FilmCraft / Fusion | 1080p60/4K render hızını ve iterasyon hızını birkaç kat hızlandırır. |
| **4** | **İnce Canlı Masaüstü Önizleme (`winit` + `egui` + `wgpu`)** | Gausian / FilmCraft | İnsan + ajan ortak çalışmasının önündeki son ürün duvarını yıkar. |
| **5** | **Mevcut Optik Akıştan 2D Point Tracker + `.otio`/FCPXML Export** | Resolve / MakeMyClip | Profesyonel editör iş akışlarına doğrudan köprü kurar. |

## Doğrulanmamış Noktalar

- Bu belgedeki özellikler tasarım ve öncelik önerileridir; hiçbirinin kod uygulaması, GPU bellek/FPS ölçümü veya ajan hata-oranı kıyaslaması henüz yapılmamıştır.
- Bahsedilen rakip karşılaştırmaları kaynak belgelerdeki ve harici ürün incelemelerindeki özetlere dayanır; ortak bir referans proje üzerinde yan yana render ölçümü yapılmamıştır.
