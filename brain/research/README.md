# FrameShift — Araştırma Dizini (`brain/research/`)

Bu dizin; kütüphane doğrulamaları, GPU/medya I/O prototip incelemeleri, lisans kontrolleri ve görsel kalite analizleri için birincil kaynaklı teknik araştırma raporlarını tutar.

---

## Kök Dizindeki Mevcut Araştırma Belgeleri (Köprü)

Projenin başlangıç aşamasında üretilen iki kapsamlı belge kök dizinde korunmaktadır (bu dosyalara köprü üzerinden referans verilir, kopyaları çoğaltılmaz):

1. **[ENGINE_ALTERNATIVES_RESEARCH.md](../../ENGINE_ALTERNATIVES_RESEARCH.md):** HyperFrames, Remotion, native GPU yaklaşımları ve mevcut editör/MCP adaylarının kaynaklı karşılaştırması. *(Not: Benchmark değildir; güncel stack kararı ADR [0001](../decisions/0001-independent-native-gpu-core.md) ile netleşmiştir.)*
2. **[VISUAL_REFERENCES.md](../../VISUAL_REFERENCES.md):** Kullanıcının paylaştığı referans videolar, örnek kare gözlemleri ve motor gereksinimlerine çıkarımlar.

---

## Aktif Teknik Araştırma Raporları (`brain/research/`)

1. **[open-source-shader-transitions.md](open-source-shader-transitions.md):** `gl-transitions` (MIT/BSD) ve `libretro/slang-shaders` geçişlerinin kaynak analizi ve FrameShift WGSL uyarlama şartnamesi (`transition_cross_zoom.wgsl`, `transition_directional_warp.wgsl`, `transition_glitch_displace.wgsl`).
2. **[anime-amv-postfx-shaders.md](anime-amv-postfx-shaders.md):** `Vidvox/ISF-Files` (MIT), `godotshaders.com` (MIT/CC0), `evanw/glfx.js` (MIT), `stackgl/glsl-halftone` (MIT) ve `libretro/slang-shaders` `dual_filter` (CC0) kaynaklarından 4 Anime/AMV post-FX shader ailesinin (`fx_motion_tile_shake.wgsl`, `fx_anime_impact_frame.wgsl`, `fx_radial_shockwave.wgsl`, `fx_halftone_glow.wgsl`) matematiksel analizi ve WGSL uniform şartnamesi.
3. **[3d-kinetic-typography-architecture.md](3d-kinetic-typography-architecture.md):** 3D kinetik tipografi ve motion graphics referans analizi (`@platsae` *BANG! BANG! BANG!*), eksik yetenekler ve FrameShift için önerilen çözümler (`glam` + `Camera3D`, `ttf-parser` + `lyon_tessellation` 3D extrusion, Jimenez 2014 PBR Bloom, Bokeh DoF + Brown-Conrady lens distortion, stateless analitik GPU parçacık/ribbon, `Demucs` + `stable-ts` lyric alignment).
4. **[two-tier-extensible-effect-architecture.md](two-tier-extensible-effect-architecture.md):** İki katmanlı genişletilebilir efekt mimarisi — Çekirdek yapı taşları (Katman 1) + Küratörlü Açık Kaynak WGSL Kütüphanesi ve Runtime `CustomWgsl` Sandbox (Katman 2).
4. **[2026-11-21-edit-friction.md](2026-11-21-edit-friction.md):** Koleedits dogfood sürtünmesinin sekiz maddesi. Uygulama başlamadı.

---

## Araştırma Standardı

Yeni bir teknik araştırma dosyası açarken [../_templates/arastirma.md](../_templates/arastirma.md) şablonunu kullan ve şu 3 ilkeye uy:
1. **Birincil Kaynak Zorunluluğu:** Arama özeti yerine resmi dokümantasyon, kaynak kod, `.d.ts` / `.rs` trait imzaları veya resmi lisans metinleri incelenir; URL ve inceleme tarihi kaydedilir.
2. **Gözlem vs. Çıkarım Ayrımı:** Doğrudan okunan/ölçülen gerçeklerle tahmini çıkarımlar ayrı başlıklarda tutulur.
3. **Doğrulanmamış Noktalar:** Henüz denenmemiş (ör. zero-copy decode, belirli GPU sürücüsü) noktalar "doğrulanmadı" etiketiyle açıkça işaretlenir.
