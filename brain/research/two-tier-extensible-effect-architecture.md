---
title: İki Katmanlı Genişletilebilir Efekt ve Geçiş Mimarisi (Çekirdek Yapı Taşları + Runtime CustomWgsl)
date: 2026-11-21
status: active
tags: [engine, render-graph, shaders, agent]
---

# İki Katmanlı Genişletilebilir Efekt ve Geçiş Mimarisi

## Soru / Amaç

Edit türleri (anime AMV, 3D kinetik tipografi, kolaj, HUD/cyber edit vb.) değiştikçe ihtiyaç duyulan efekt ve geçişler de değişmektedir. Her yeni efekti tek tek Rust `NodeKind` enum'una eklemek motoru şişirip yavaşlatırken, her şeyi ajanın sıfırdan yazmasına bırakmak da görsel olgunluk ve kalite tutarlılığı riski doğurur. Bu belge, FrameShift için **Çekirdek Yapı Taşları + Küratörlü Açık Kaynak Kütüphanesi + Runtime `CustomWgsl` Sandbox** dengesini tanımlar.

---

## İncelenen Birincil Kaynaklar

- [brain/specs/effect-shader-api.md](../specs/effect-shader-api.md) — WGSL `@group(0)` binding sözleşmesi, 16-byte uniform hizalaması, Naga doğrulama ve TDR döngü güvenliği (`status: locked`).
- [brain/research/open-source-shader-transitions.md](open-source-shader-transitions.md) — `gl-transitions` (MIT/BSD) olgun açık kaynak geçiş analizi.
- [crates/frameshift-schema/src/graph.rs](../../crates/frameshift-schema/src/graph.rs) — Mevcut 14 sabit `NodeKind` enum tanımı.

---

## Doğrudan Gözlemler (Kanıtlı)

1. **Mevcut Sınır:** `crates/frameshift-schema/src/graph.rs` içinde `NodeKind` sabit 14 varyantlı bir Rust enum'udur. Çalışma zamanında proje JSON'ı üzerinden yeni WGSL fragment kodu gönderip derleyecek bir `CustomWgsl` düğümü henüz yoktur.
2. **Mevcut Açık Kaynak vs. Sıfırdan Yazılan Geçiş Ayrımı:**
   - **Dış kaynaklı (olgun açık kaynak portları):** `transition_cross_zoom.wgsl`, `transition_directional_warp.wgsl`, `transition_glitch_displace.wgsl` (`gl-transitions` MIT portları).
   - **Ajan tarafından sıfırdan yazılan:** `transition_whip_chromatic.wgsl` (`whip_chromatic`).
3. **Kalite Farkı Gözlemi (Kullanıcı Geri Bildirimi):** Sıfırdan ajan tarafından üretilen tek seferlik shader'lar bazen yıllarca rafine edilmiş dış açık kaynak shader'lar kadar olgun/organik görünmeyebilir; buna karşın yalnızca sabit hazır geçişlere sıkışmak da yaratıcı esnekliği ve özel efekt üretimini kısıtlar.

---

## Çıkarımlar ve Öneriler: 2 Katmanlı (+ Küratörlü Kütüphane) Mimari

### Katman 1 — Çekirdek Motor Yapı Taşları (Rust + `wgpu` Primitives)
Tek bir 2D fragment shader ile çözülemeyen, tüm stillerin altında çalışan ağır ve deterministik motor yetenekleri **bir kez** çekirdeğe eklenir:
- `SourceVideo` / medya decode, `Flicks` zaman tabanı, `SafeExpr` ve `AudioRmsEnvelope`.
- `Camera3D` + 3D perspektif + `Depth32Float` Z-buffer (Faz 6A hedefi).
- Gerçek TTF/OTF font + vektör tessellation + 3D extrusion (`ttf-parser` + `lyon_tessellation`, Faz 6A hedefi).
- Çok geçişli (multi-pass) piramit efektleri: Jimenez 2014 HDR Deep Glow (Bloom), 2 geçişli Bokeh DoF, `MotionBlurShutter`.
- Stateless analitik GPU parçacık (`ParticleField3D`) ve 3D Trim-Path şeritler (`RibbonTrail3D`).

### Katman 2 — Sınırsız 2D Efekt ve Geçişler İçin İki Kaynaklı Model
Tüm 2D piksel efektleri ve geçişleri için **öncelik sırası**:

1. **Öncelik 1 — Küratörlü Açık Kaynak WGSL Kütüphanesi (Olgun Kalite Tabanı):**
   - `gl-transitions`, Vidvox ISF, Libretro `slang-shaders`, GodotShaders (MIT/BSD/CC0) gibi toplulukta yıllarca rafine edilmiş, görsel olarak kanıtlanmış shader'lar standart FrameShift WGSL sözleşmesine port edilir ve `brain/style-cards/` kartlarına bağlanır.
   - Ajan bildiği ve yüksek kaliteli bir geçiş/efekt gerektiğinde **tekerleği yeniden icat etmek yerine** önce bu olgun kütüphaneden seçer.
2. **Öncelik 2 — Runtime `CustomWgsl` Sandbox Düğümü (Sınırsız Esneklik Emniyet Valfi):**
   - Proje JSON'ına `NodeKind::CustomWgsl { wgsl_source, inputs, params: [f32; 16] }` düğümü eklenir.
   - Kütüphanede bulunmayan özel bir 2D efekt/geçiş (örn. özel CRT, pixelate, kaleidoskop, prosedürel arka plan, özel maske deseni, stilize glitch) gerektiğinde ajan **Rust koduna dokunmadan ve yeniden derlemeden** kendi WGSL shader'ını proje içine yazar; motor bunu çalışma zamanında Naga güvenlik kontrolünden geçirip GPU'da çalıştırır.
   - Ajanın ürettiği ve kullanıcının beğendiği bir `CustomWgsl` shader'ı daha sonra test edilip **Küratörlü Kütüphane**'ye terfi ettirilebilir (evrimsel kütüphane büyümesi).

### Özet Karar Kuralı (Ajan İçin)
- **Olgun kütüphanede varsa →** hazır, doğrulanmış açık kaynak shader'ı kullan (daha yüksek görsel olgunluk).
- **Kütüphanede yoksa ama 2D piksel efekti/geçişi ise →** `CustomWgsl` ile çalışma zamanında üret ve görsel inceleme (video/kare) ile doğrula.
- **Yeni geometri / 3D / multi-pass / font gerektiriyorsa →** eksikliği ilgili stil kartına (`brain/style-cards/`) işle ve Katman 1 çekirdek yapı taşı olarak planla.

---

## Doğrulanmamış Noktalar

- `NodeKind::CustomWgsl` çalışma zamanı derleme süresi, Naga statik döngü/TDR doğrulama sınırları ve pipeline önbellekleme (`shader_hash -> RenderPipeline`) henüz koda dökülüp benchmark edilmedi.
- Dış kaynaklı (`gl-transitions`) vs. ajan yapımı (`whip_chromatic`) geçişlerin gerçek anime klipleri üzerindeki yan yana video kalite karşılaştırması bu oturumda üretilip kullanıcı değerlendirmesine sunulacaktır.
