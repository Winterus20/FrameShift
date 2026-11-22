# 2026-11-21 — Açık Kaynak Anime FX Araştırması ve 15 WGSL Shader Uyarlaması

**Tarih:** 2026-11-21  
**Faz:** Anime FX & Geçiş Kütüphanesi Araştırma ve WGSL Uyarlaması  
**Durum:** Tamamlandı

---

## Amaç

Açık kaynaklı (MIT / CC0 / BSD) geçiş ve anime/AMV post-processing shader kaynaklarını birincil kodlarından araştırıp `brain/research/` altında `.md` raporlarına dönüştürmek, ardından bu raporları temel alarak WGSL shader'larını `frameshift-render`, `frameshift-schema` ve `frameshift-core` içine uyarlayıp 8 panelli `fx_showcase_pack.png` ile doğrulamak.

---

## Yapılanlar

1. **Birincil Kaynak Araştırması ve `.md` Raporları (`fx-trans-research` & `fx-anime-research`):**
   - [open-source-shader-transitions.md](../research/open-source-shader-transitions.md): `gl-transitions/gl-transitions` deposundan (`CrossZoom.glsl`, `DirectionalWarp.glsl`, `GlitchDisplace.glsl`, `Swirl.glsl`, `DreamyZoom.glsl`, `Morph.glsl`) MIT lisansları ve GLSL kodları `curl.exe -sL` ile incelendi; `GlitchDisplace.glsl` içindeki `smoothstep(1.0, 0.5, p)` tanımsız davranışı ve sıfır-vektör normalize hatası tespit edilip WGSL şartnamesinde düzeltildi.
   - [anime-amv-postfx-shaders.md](../research/anime-amv-postfx-shaders.md): `Vidvox/ISF-Files` (MIT), `libretro/slang-shaders` (CC0), `godotshaders.com` (MIT/CC0), `evanw/glfx.js` (MIT) ve `stackgl/glsl-halftone` (MIT) kaynakları incelenerek 4 temel Anime Post-FX ailesi (`MotionTileShake`, `AnimeImpactFrame`, `RadialShockwave`, `HalftoneGlow`) şartnamesi yazıldı.
2. **7 Yeni WGSL Shader ve Şema Uyarlaması (`wgsl-fx-adapter` & `schema-fx-adapter`):**
   - `crates/frameshift-render/shaders/` altına `transition_cross_zoom.wgsl`, `transition_directional_warp.wgsl`, `transition_glitch_displace.wgsl`, `fx_motion_tile_shake.wgsl`, `fx_anime_impact_frame.wgsl`, `fx_radial_shockwave.wgsl`, `fx_halftone_glow.wgsl` eklendi (toplam 15 WGSL shader).
   - `crates/frameshift-schema/src/graph.rs` ve `project.rs` içine `NodeKind::MotionTileShake`, `NodeKind::AnimeImpactFrame`, `NodeKind::RadialShockwave`, `NodeKind::HalftoneGlow`, `BUILTIN_TRANSITION_SHADERS` ve `Project::anime_fx_showcase_preset()` eklendi.
3. **Motor Entegrasyonu ve Görsel Vitrin (`fx-integrator`):**
   - `frameshift-render`: 15 WGSL pipeline derlemesi, 48B uniform yapıları ve `GpuCompositor` / `ProjectAnimator` değerlendirmesi bağlandı.
   - `frameshift-core`: `Engine::export_fx_showcase_pack` eklendi ve `target/full_verify/fx_showcase_pack.png` (`1920x1080`, 4x2 karşılaştırma tablosu) üretildi.

---

## Doğrulama ve Kanıtlar

- `cargo test --workspace`: **58 passed, 0 failed** (9 crate: `frameshift-time`: 12, `frameshift-schema`: 7, `frameshift-media`: 9, `frameshift-vector`: 5, `frameshift-render`: 12, `frameshift-core`: 5, `frameshift-mcp`: 2, `frameshift-app`: 4, `frameshift-cli`: 2).
- `cargo clippy --workspace --all-targets -- -D warnings`: **0 uyarı, 0 hata**.
- `target/full_verify/fx_showcase_pack.png` görsel olarak incelendi: 4 geçiş (`Whip Chromatic`, `Cross Zoom`, `Directional Warp`, `Glitch Displace`) ve 4 anime post-FX (`Motion Tile + Shake`, `Anime Impact Frame`, `Radial Shockwave`, `Halftone + Deep Glow`) karelerinin her birinin benzersiz FNV-1a hash ve beklenen görsel karakteristikle üretildiği doğrulandı.
