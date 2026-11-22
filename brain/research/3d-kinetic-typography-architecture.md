---
title: 3D Kinetik Tipografi ve Motion Graphics İçin Eksik Yetenekler ve En İyi Mimari Çözümler
date: 2026-11-21
status: active
tags: [engine, render-graph, shaders, text-vector, timebase, agent]
---

# 3D Kinetik Tipografi ve Motion Graphics İçin Eksik Yetenekler ve En İyi Mimari Çözümler

## Soru / Amaç

Referans olarak incelenen After Effects 3D kinetik tipografi videosu (`@platsae` — *BANG! BANG! BANG! Typography Edit*, 1920×1080 @ 60fps, 28.1 sn) kalitesinde, tamamen prosedürel 3D kinetik tipografi ve motion graphics videolarını FrameShift (`Rust` + `wgpu 30.0.1` + `Rgba16Float` HDR render grafiği + `Flicks` zaman tabanı) ile üretebilmek için **mevcut motorda hangi yeteneklerin eksik olduğu** ve **her eksik için lisans-uyumlu, seek-independent ve en yüksek görsel kaliteli çözümlerin neler olduğu** araştırılmıştır.

---

## İncelenen Birincil Kaynaklar

- [ttf-parser (docs.rs — v0.25.1, MIT OR Apache-2.0)](https://docs.rs/ttf-parser/latest/ttf_parser/) — erişim/inceleme tarihi: 2026-11-21
- [lyon_tessellation (docs.rs — v1.0.23, MIT OR Apache-2.0)](https://docs.rs/lyon_tessellation/latest/lyon_tessellation/) — erişim/inceleme tarihi: 2026-11-21
- [glam (docs.rs — v0.34.3, MIT OR Apache-2.0)](https://docs.rs/glam/latest/glam/) — erişim/inceleme tarihi: 2026-11-21
- [parley (github.com/linebender/parley — Apache-2.0 OR MIT)](https://github.com/linebender/parley) — erişim/inceleme tarihi: 2026-11-21
- [swash (crates.io — v0.2.10, Apache-2.0 OR MIT)](https://crates.io/crates/swash) — erişim/inceleme tarihi: 2026-11-21
- [vello (github.com/linebender/vello — Apache-2.0 OR MIT)](https://github.com/linebender/vello) — erişim/inceleme tarihi: 2026-11-21
- [LearnOpenGL — Physically Based Bloom (Jorge Jimenez, SIGGRAPH 2014 *Call of Duty: Advanced Warfare*)](https://learnopengl.com/Guest-Articles/2022/Phys.-Based-Bloom) — erişim/inceleme tarihi: 2026-11-21
- [Dual Kawase Blur / Bloom (Marius Bjørge, SIGGRAPH 2015 / ARM Bandwidth-Efficient Rendering)](https://community.arm.com/cfs-file/__key/communityserver-blogs-components-weblogfiles/00-00-00-20-66/siggraph2015_2D00_mmg_2D00_marius_2D00_notes.pdf) — erişim/inceleme tarihi: 2026-11-21
- [jianfch/stable-ts (GitHub — MIT)](https://github.com/jianfch/stable-ts) — erişim/inceleme tarihi: 2026-11-21
- [m-bain/whisperX (GitHub — BSD-2-Clause)](https://github.com/m-bain/whisperX) — erişim/inceleme tarihi: 2026-11-21
- [facebookresearch/demucs (GitHub — MIT kod, model ağırlıkları ayrı kullanım koşullarına tabi)](https://github.com/facebookresearch/demucs) — erişim/inceleme tarihi: 2026-11-21
- Yerel depo durumu: [Cargo.toml](../../Cargo.toml), [crates/frameshift-render/Cargo.toml](../../crates/frameshift-render/Cargo.toml), [crates/frameshift-vector/Cargo.toml](../../crates/frameshift-vector/Cargo.toml), [brain/state.md](../state.md) — inceleme tarihi: 2026-11-21

---

## Doğrudan Gözlemler (Kanıtlı)

### 1. Referans Videoda Gözlemlenen Görsel Teknikler
- Referans video (`1920×1080`, `60fps`, `28.10 sn`, `HEVC + 44.1 kHz AAC`) hiçbir dış video klibi içermeyen %100 3D kinetik tipografi + motion graphics kurgusudur.
- Gözlemlenen 7 ana bileşen:
  1. **3D Kamera & 3D Katman Uzayı:** Metinler ve geometrik şekiller (yıldız, kare, çerçeve, kalp, şimşek) X/Y/Z derinliğine yerleştirilmiş; kamera sert easing'lerle 3D pan, zoom, orbit ve Z-roll yapıyor.
  2. **Çoklu Font, Harf Bazlı Animasyon ve 3D Extrusion:** Birden fazla profesyonel TTF/OTF font, harf bazlı ayrı renk/ölçek/zıplama (`D-y-i-n-g`, `B-A-N-G!`), gradyan dolgu, kalın stroke ve seçili kelimelerde (`LEGENDARY`, `FRATERNITY`, `DOWN!`) 3D derinlik (extrusion).
  3. **Geniş Yarıçaplı HDR Deep Glow:** Tüm neon metin ve şekillerde yumuşak, yüksek dinamik aralıklı, çok kademeli parlama.
  4. **Parçacıklar & 3D Neon Şeritler:** Yıldız tozu, kıvılcımlar, bokeh daireleri, kalp parçacıkları ve kelimelerin etrafından akan animasyonlu neon şeritler (Trim Paths / 3D Stroke).
  5. **Depth of Field (Bokeh DoF):** Kameraya çok yakın/uzak metin ve parçacıklarda odak dışı bulanıklık.
  6. **Shutter Motion Blur + Impact Shake + Lens Distortion + RGB Split:** Hızlı kamera hareketlerinde ağır motion blur, beat vuruşlarında sarsıntı, beyaz exposure patlamaları ve kenarlarda chromatic aberration + hafif barrel/pincushion lens bükülmesi.
  7. **Vokale Sıkı Söz Senkronu:** Her kelime/hece şarkıdaki vokal vuruşuna tam karede oturuyor.

### 2. FrameShift Mevcut Durumu ve Boşluk Matrisi
- **Halihazırda mevcut olanlar:** `wgpu 30.0.1` + `Rgba16Float` premultiplied HDR render grafiği, `Flicks(i64)` kesin zaman tabanı (60fps + 44.1/48 kHz tam uyumlu), `motion_blur_accumulate.wgsl` (sub-frame temporal motion blur), `fx_motion_tile_shake.wgsl`, `transition_whip_chromatic.wgsl`, `color_basic.wgsl`, `lut3d.wgsl`, `blend.wgsl`, `AudioRmsEnvelope`, `SafeExpr`.
- **Eksik olan 6 temel alan:**
  1. **Gerçek 3D Kamera + 2.5D/3D Perspektif + Depth Buffer** (`transform.wgsl` şu an yalnızca 2D affine).
  2. **Harici TTF/OTF Font Yükleme + Per-Character Layout + 3D Extrusion / Vektör Şekiller** (`frameshift-vector` şu an harici bağımlılıksız basit analitik SDF glifleriyle sınırlı).
  3. **Multi-Pass HDR Bloom (Deep Glow)** (`fx_halftone_glow.wgsl` tek geçişli basit parlama yapıyor).
  4. **Bokeh Depth of Field & Brown-Conrady Lens Distortion** (henüz yok).
  5. **Seek-Independent GPU Parçacık Sistemi & 3D Trim-Path Ribbon Şeritler** (henüz yok).
  6. **Kelime Düzeyinde Şarkı Sözü Zamanlama (Word-Level Lyric Alignment)** (şu an yalnızca ses genliği zarfı `AudioRmsEnvelope` var).

### 3. Literatür ve Ekosistem Bulguları

#### A. Font, Vektör ve 3D Extrusion Hattı
- **`ttf-parser` (v0.25.1) + `lyon_tessellation` (v1.0.23):** `ttf-parser` sıfır-ayırımlı (zero-allocation) güvenli Rust parser'ı olarak glif konturlarını (`move_to`, `line_to`, `quad_to`, `curve_to`) verir; `lyon_tessellation` bu konturları doğrudan üçgen mesh'e (fill ve stroke) çevirir. Ön yüz üçgenlemesi Z ekseninde kaydırılıp yan duvarlar kontur kenarlarından dikildiğinde gerçek **3D extruded metin ve vektör şekil mesh'i** elde edilir ve doğrudan `wgpu` 3D pipeline'ına verilir.
- **`Parley` + `Swash` / `Vello` (Linebender ekosistemi):** `Parley` (HarfRust tabanlı) karmaşık çok-dilli shaping, bidi ve satır kırma sağlar; `Vello` ise `wgpu` compute shader'larıyla 2D vektör rasterizasyonu yapar. Ancak `Vello` saf 2D ekran uzayı rasterizer'ıdır — gerçek 3D extrusion, 3D perspektif kamera ve Z-buffer derinliği için kendi başına yeterli değildir.

#### B. Multi-Pass HDR Deep Glow (Bloom)
- **Jimenez 2014 (Physically Based Bloom):** 13-tap ağırlıklı downsample (Karis-benzeri parlaklık ağırlıklandırmasıyla tek piksellik "firefly" titreşimini engeller) + 9-tap tent upsample piramidi. HDR (`Rgba16Float`) hattında yapay sert eşik gerektirmeden fiziksel ve zamansal olarak kararlı (flicker-free) parlama üretir.
- **Dual Kawase Bloom (Bjørge 2015):** 4-tap çapraz downsample + 8-tap upsample. Çok hızlıdır ve yumuşak/stilize parlama verir; ancak firefly bastırma için girişte ayrı bir yumuşak eşik/prefilter geçişi ister.

#### C. Bokeh DoF ve Brown-Conrady Lens Distortion
- **Depth of Field:** 3D render geçişinden çıkan `Depth32Float` tamponu doğrusallaştırılıp ince mercek denklemiyle (focal distance, focal length, f-stop) piksel başına **Circle of Confusion (CoC)** hesaplanır (ön plan negatif, arka plan pozitif); ardından Vogel/spiral disk örneklemesi ile (ön plan sızmasını engelleyen ağırlıklarla) bokeh gather geçişi uygulanır.
- **Optics Compensation (Lens Distortion):** Brown-Conrady radyal (`k1, k2, k3`) + teğetsel (`p1, p2`) modeli tek bir WGSL fragment geçişinde hem barrel hem pincushion lens bükülmesi (ve opsiyonel kanal bazlı RGB split) sağlar.

#### D. Seek-Independent Analitik Parçacıklar ve 3D Şeritler
- Video editörlerinde rastgele kareye atlama (seek/scrub) ve paralel/deterministik render gerektiğinden, kare-kare durum güncelleyen (stateful) compute parçacık sistemleri ısınma (warm-up) maliyeti ve seek uyumsuzluğu doğurur.
- **Stateless Analitik Parçacık Modeli (`P(t, i) = f(seed, T)`):** Her parçacığın doğuş zamanı, ömrü, konumu ve boyutu `@builtin(instance_index)` + global `uniforms.time` üzerinden vertex shader'da doğrudan kapalı formda (spline + başlangıç hızı + ivme + hash tabanlı gürültü) hesaplanır. Sıfır warm-up gecikmesi ve %100 determinizm sağlar.
- **3D Trim-Path Ribbon Şeritler:** Aynı analitik spline fonksiyonu `t_k = T - k * dt` geçmiş örneklerinde değerlendirilip türev (`dP/dt`) ile kamera yönünün çapraz çarpımından (`cross(tangent, view_dir)`) sürekli kamera-dönük 3D şerit (triangle strip) üretilir.

#### E. Müzikte Kelime Düzeyinde Şarkı Sözü Zamanlama
- Polifonik müzikte enstrümanlar vokali maskelediği için ham şarkıyı doğrudan ASR'a vermek ciddi zaman kayması ve halüsinasyon üretir.
- Endüstri standardı 2 aşamalı boru hattı:
  1. **Vokal Ayrıştırma (`Demucs` — `htdemucs`):** Şarkıdan temiz `vocals.wav` katmanı çıkarılır.
  2. **Forced Alignment (`stable-ts` veya `WhisperX` / `ctc-forced-aligner`):**
     - Şarkı sözleri biliniyorsa **`stable-ts` (known-text alignment)** Whisper'ın DTW dikkat haritalarını kullanarak metni uydurmadan doğrudan verilen kelimeleri vokale hizalar.
     - Sözler bilinmiyorsa **`WhisperX`** (`faster-whisper` + Wav2Vec2 CTC fonem hizalama) kullanılır.

---

## Çıkarımlar ve Öneriler

*(Aşağıdaki öneriler yukarıdaki gözlemlere dayalı mimari çıkarımlardır; yeni harici crate eklemeden önce kullanıcı onayı alınacaktır.)*

### FrameShift İçin En İyi Teknik Mimari Seçimleri

| Eksik Alan | Önerilen En İyi Çözüm | Neden FrameShift İçin En Uygun? | Lisans Uyumu |
| :--- | :--- | :--- | :--- |
| **3D Matematik & Kamera** | `glam` (`v0.34.x`) + `Camera3D` düğümü + `Depth32Float` Z-buffer | Rust/`wgpu` ekosisteminde SIMD hızlandırmalı fiili standart; `bytemuck` ile sıfır kopya uniform uyumu. | `MIT OR Apache-2.0` ✅ |
| **Font + Per-Character + 3D Extrusion** | `ttf-parser` (`0.25.x`) + `lyon_tessellation` (`1.0.x`) (+ ileri aşamada çok-dilli shaping gerekirse `parley`) | Hem keskin 2D vektör/stroke hem gerçek 3D extruded metin mesh'ini tek hatta çözer; `Vello` gibi ağır ayrı compute pipeline gerektirmez, mevcut `wgpu 30` ile tam uyumlu. | `MIT OR Apache-2.0` ✅ |
| **HDR Deep Glow (Bloom)** | **Jimenez 2014 PBR Bloom** (6–7 seviye mip piramidi, 13-tap downsample + 9-tap tent upsample, WGSL) | FrameShift zaten `Rgba16Float` lineer HDR kullanıyor; 60fps hızlı kinetik metinde ince harf kenarlarında firefly/titreşim üretmez (Dual Kawase'ye göre zamansal olarak çok daha temiz). | Saf WGSL (0 harici bağımlılık) ✅ |
| **Bokeh DoF & Optics Comp.** | 2 geçişli WGSL DoF (CoC + 16/24-tap Vogel disk gather) + Brown-Conrady `fx_lens_distortion.wgsl` | `Camera3D` Z-buffer'ını doğrudan tüketir; `Optics Compensation` + kenar RGB split tek geçişte çözülür. | Saf WGSL (0 harici bağımlılık) ✅ |
| **Parçacıklar & 3D Şeritler** | **Stateless Analitik GPU Parçacık + 3D Trim-Path Ribbon** (WGSL instanced vertex/fragment) | FrameShift'in `render-graph.md` **seek-independent DAG** sözleşmesini bozmaz; kare 1200'e doğrudan atlandığında sıfır simülasyon ısınmasıyla deterministik çıktı verir. | Saf WGSL (0 harici bağımlılık) ✅ |
| **Şarkı Sözü Zamanlama** | Çekirdek dışı CLI / MCP aracı olarak **`Demucs` + `stable-ts`** → standart `lyric_timings.json` (`Flicks` tick'e dönüştürülerek) | Python/PyTorch ağırlığını Rust çekirdeğine sokmaz; proje şemasına yalnızca deterministik `EventAnchor` / `LyricWordEvent` tick kayıtları girer. | Çekirdek dışı araç (`MIT` kod) ✅ |

### Önerilen Aşamalı Uygulama Yol Haritası (Faz 6)

1. **Faz 6A — 3D Kinetik Tipografi Çekirdeği (En Yüksek Görsel Etki, ~%75 kalite sıçraması):**
   - `glam` + `Camera3D` (perspektif projeksiyon, FOV, pan/orbit/roll, 3D transform katmanları, `Depth32Float`).
   - `ttf-parser` + `lyon_tessellation`: Harici TTF/OTF font yükleme, harf bazlı (per-character) animasyon ve opsiyonel 3D extrusion + gradyan/stroke.
   - `fx_bloom_pbr.wgsl` (Jimenez 2014 multi-pass HDR Deep Glow) + `fx_lens_distortion.wgsl` (Brown-Conrady Optics Compensation).
2. **Faz 6B — Atmosfer ve Derinlik (~%95 hedef kalite):**
   - Stateless analitik 3D GPU parçacık düğümü (`ParticleField3D`: yıldız tozu, kıvılcım, bokeh, özel şekil sprite'ları).
   - 3D Trim-Path spline şerit düğümü (`RibbonTrail3D`: neon ışık şeritleri).
   - `fx_bokeh_dof.wgsl` (Z-buffer tabanlı Circle of Confusion + Vogel disk bokeh).
3. **Faz 6C — Ajan Otomasyonu (Tek Komutla Söz-Senkronlu Edit):**
   - `Demucs` + `stable-ts` dış boru hattından üretilen kelime zaman damgalarını (`word_timestamps.json`) FrameShift `Flicks` olaylarına (`LyricTimeline`) aktaran CLI/MCP komutu.

---

## Doğrulanmamış Noktalar

- **60fps 1080p Render Performansı:** `motion_blur_accumulate` (ör. 8–16 sub-sample) + 6-seviye Jimenez Bloom + 24-tap Vogel DoF + 3D extruded metin birlikte çalıştırıldığında gerçek GPU kare süresi henüz ölçülmedi (ör. Bloom ve DoF'un motion blur biriktirme *sonrasında* tek kez uygulanması kritik performans optimizasyonu olacaktır).
- **Karmaşık Font Konturlarında `lyon` Dayanıklılığı:** Kendini kesen (self-intersecting) veya bozuk bezier içeren serbest/stilize display fontlarında `lyon_tessellation` kenar durumları ve 3D yan-duvar normal yönü (winding order) gerçek fontlarla test edilmedi.
- **Hızlı EDM / Vokal Efektli Şarkılarda Hizalama İsabeti:** `Demucs + stable-ts` hattının referanstaki gibi hızlı, uzatmalı (melisma) veya çok katmanlı pop/EDM vokallerde ortalama milisaniye sapması henüz yerel ses dosyalarıyla ölçülmedi.
- **`wgpu 30.0.1` + `glam` / `lyon` Derleme Testi:** Önerilen crate sürümleri henüz `Cargo.toml`'a eklenip `cargo check --workspace` ile doğrulanmadı (kullanıcı onayı bekleniyor).
