---
title: Faz 6.6 — 2D/Anime Post-FX, 10 Shader Geçişi & JFA SDF Tipografi FX Araştırma ve Uygulama Şartnamesi
date: 2026-11-22
status: active
tags: [shaders, render-graph, text-vector, schema]
---

# Faz 6.6 — 2D/Anime Post-FX, 10 Shader Geçişi & JFA SDF Tipografi FX Araştırma ve Uygulama Şartnamesi

## Soru / Amaç

FrameShift'in 2D/Anime Post-FX (`fx_radial_shockwave.wgsl`, `fx_halftone_glow.wgsl`, `fx_motion_tile_shake.wgsl`), 10 yerleşik `transition_*.wgsl` geçişi ve 2D Slug tipografi FX (`text2d_fx.wgsl`) boru hattındaki görsel kusurlar, en-boy oranı (16:9) çarpılmaları, bantlaşma/dikiş artefaktları ve 352+ tap/px performans darboğazı **0 yeni Cargo crate** ile nasıl çözülmelidir?

Bu belge **Faz 1 (Yalnızca Araştırma)** kapsamında hazırlanmış olup Faz 2 uygulaması için kesin WGSL matematiğini, 16-byte hizalı uniform düzenlerini, değişecek dosya haritasını ve M1/mevcut test uyumluluk stratejisini kilitler. *(Not: Görev özetinde geçen `crates/frameshift-schema/src/node.rs` dosyası mevcut depoda [graph.rs](../../crates/frameshift-schema/src/graph.rs) ve [project.rs](../../crates/frameshift-schema/src/project.rs) olarak konumlanmaktadır.)*

---

## İncelenen Birincil Kaynaklar

Tüm harici birincil kaynaklar `2026-11-22` tarihinde doğrudan `curl.exe -sL` veya kaynak incelemesi ile doğrulanmıştır:

1. **Vidvox ISF-Files (MIT Lisansı):**
   - [Shockwave.fs](https://raw.githubusercontent.com/Vidvox/ISF-Files/master/ISF/Shockwave.fs) — erişim: `2026-11-22` (orijinal `sign(diff)` benzeri `diff * (1 - |diff*d|^0.8)` kırılma çekirdeği; tepe noktasında türev kırılması kaynağının incelenmesi).
   - [CMYK Halftone.fs](https://raw.githubusercontent.com/Vidvox/ISF-Files/master/ISF/CMYK%20Halftone.fs) — erişim: `2026-11-22` (`gridRot = vec4(15.0, 45.0, 0.0, 75.0)`, `RGBAtoCMYK` / `CMYKtoRGBA` dönüşümü).
2. **`stackgl` Modular Halftone & AA Step (MIT Lisansı):**
   - [glsl-halftone/index.glsl](https://raw.githubusercontent.com/stackgl/glsl-halftone/master/index.glsl) — erişim: `2026-11-22` (15°/75°/0°/45° CMYK rosette döndürme matrisleri ve `sqrt(cmyk)` nokta yarıçapı).
   - [glsl-aastep/index.glsl](https://raw.githubusercontent.com/stackgl/glsl-aastep/master/index.glsl) — erişim: `2026-11-22` (`length(vec2(dFdx(v), dFdy(v))) * 0.70710678` ekran türevli kenar yumuşatma).
3. **`gl-transitions` (MIT Lisansı):**
   - [Kaleidoscope.glsl](https://raw.githubusercontent.com/gl-transitions/gl-transitions/master/transitions/Kaleidoscope.glsl) — erişim: `2026-11-22` (`nwoeanhinnogaehr`, MIT).
   - [BurnThrough.glsl](https://raw.githubusercontent.com/gl-transitions/gl-transitions/master/transitions/BurnThrough.glsl) — erişim: `2026-11-22` (`liubailin2020`, MIT — 4 oktavlı FBM yanma maskesi ve kor kenar parlaması).
   - [FilmBurn.glsl](https://raw.githubusercontent.com/gl-transitions/gl-transitions/master/transitions/FilmBurn.glsl) — erişim: `2026-11-22` (`Anastasia Dunbar`, MIT — organik film burn ışık sızıntısı ve ısı kırılması).
   - [Ripple.glsl](https://raw.githubusercontent.com/gl-transitions/gl-transitions/master/transitions/Ripple.glsl) — erişim: `2026-11-22` (`gre`, MIT).
   - Mevcut iç analiz köprüsü: [open-source-shader-transitions.md](open-source-shader-transitions.md), [anime-amv-postfx-shaders.md](anime-amv-postfx-shaders.md).
4. **Interleaved Gradient Noise (IGN) & Motion Blur Jitter:**
   - Jorge Jimenez (SIGGRAPH 2014 / Activision), *"Next Generation Post Processing in Call of Duty: Advanced Warfare"*: `fract(52.9829189 * fract(dot(frag_coord, vec2(0.06711056, 0.00583715))))` (halihazırda `fx_radial_shockwave.wgsl` satır 78'de doğrulanmış kullanım).
5. **GPU Distance Transform & JFA:**
   - Guodong Rong & Tiow-Seng Tan, *"Jump Flooding in GPU with Applications to Voronoi Diagram and Distance Transform"*, ACM I3D 2006 (O($\log_2 R$) paralel GPU JFA).
   - Pedro F. Felzenszwalb & Daniel P. Huttenlocher, *"Distance Transforms of Sampled Functions"*, *Theory of Computing*, 2012 (2-pass ayrılabilir 1D parabolik alt zarf EDT — GPU render-pass vs compute karşılaştırması için).
6. **FrameShift İç Sözleşmeleri ve Kod Tabanı:**
   - [effect-shader-api.md](../specs/effect-shader-api.md), [render-graph.md](../specs/render-graph.md) (`locked`).
   - [uniforms.rs](../../crates/frameshift-render/src/uniforms.rs), [pipelines.rs](../../crates/frameshift-render/src/pipelines.rs), [compositor.rs](../../crates/frameshift-render/src/compositor.rs), [pool.rs](../../crates/frameshift-render/src/pool.rs).
   - [graph.rs](../../crates/frameshift-schema/src/graph.rs), [project.rs](../../crates/frameshift-schema/src/project.rs), [engine.rs](../../crates/frameshift-core/src/engine.rs).

---

## Doğrudan Gözlemler (Mevcut Kod Denetimi)

### (a) `fx_radial_shockwave.wgsl` Denetimi
- **Kırılma Dikişi (`fx_radial_shockwave.wgsl` satır 74):**
  `let refracted_uv = in.uv - dir_uv * (refract_amp * ring_mask * sign(diff));`
  `diff = r - radius` sıfırdan geçerken `sign(diff)` fonksiyonu `-1.0` ↔ `+1.0` arasında $C^0$ sıçrama yapar. Tam bu noktada `ring_envelope = 1.0` maksimumda olduğu için halkanın tepe çizgisinde jilet gibi keskin bir kırılma dikişi (seam) oluşur.
- **Blur Ekseni & CA Ayrışması (satır 79–98):**
  `ca_offset` sabit bir vektör olarak tüm zoom blur tap'lerine aynı miktarda eklenmekte (`uv_g ± ca_offset`), ayrıca satır 97'de `vec4<f32>(sr.r, sg.g, sb.b, sg.a)` yazılarak `sr.a` ve `sb.a` yok sayılmaktadır.
- **Test Sözleşmesi ([shockwave_zero.rs](../../crates/frameshift-render/tests/shockwave_zero.rs)):**
  - `radius <= 0.0` iken `refraction_amp` ve `chromatic_aberration` sıfırlanmalı (`hot_ring == cold_ring`), ancak `zoom_blur_strength` çalışmaya devam etmelidir.
  - `radius > 0.0` iken halka kırılması ve CA `radius == 0.0` çıktısından farklı olmalıdır.

### (b) 10 `transition_*.wgsl` Dosyası Denetimi
Tüm geçişler 32 byte'lık `WhipTransitionUniform` yapısını paylaşır (`motion: [progress, direction_rad, whip_strength, chromatic_aberration]`, `flash_pad: [impact_flash, 0, 0, 0]`). `flash_pad.yzw` **boş/rezervedir** ve her shader `tex_a` (`@binding(0)`) boyutuna `textureDimensions(tex_a)` ile erişebilir.

| Shader | 16:9 Aspect Hatası | Motion Blur / Jitter | Diğer Eksikler |
| :--- | :--- | :--- | :--- |
| `transition_whip_chromatic.wgsl` | `dir = (cos, sin)` UV'de anizotropik | 12 tap var ama **IGN jitter yok** (bantlaşma/hayaletlenme) | Açık `p<=0` / `p>=1` erken dönüş eklenmeli |
| `transition_cross_zoom.wgsl` | `center` süpürmesi anizotropik | 24 tap var; eski `sin`-hash `rand_screen(in.uv)` kullanıyor | `ign(in.position.xy)`'ye geçilmeli |
| `transition_directional_warp.wgsl` | `v`, `perp`, `wave`, `ca_offset` UV'de çarpık | Tek tap (uzamsal warp; blur gerekmiyor) | İzotropik uzayda projeksiyon + dalga frekansı |
| `transition_glitch_displace.wgsl` | Voronoi hücreleri 16:9'da 1.778:1 yatay uzuyor | Tek tap (keskin dijital glitch) | **Statik tohum:** `+ vec2(1.0/4.0/17.0)` yüzünden bloklar zamana göre sıçramıyor |
| `transition_dreamy_zoom.wgsl` | `rotate_zoom_uv` aspect düzeltmesiz döndürdüğü için **görüntüyü paralelkenar gibi yamultuyor (shear)** | **Motion blur yok (1 tap)** | IGN jitter'lı zoom+dönme motion blur eklenmeli |
| `transition_swirl.wgsl` | `dist = length(in.uv - 0.5)` eliptik burgaç + shear döndürme | **Motion blur yok (1 tap)** (`gl-transitions` 12 tap önerir) | İzotropik polar burgaç + IGN jitter'lı açısal blur |
| `transition_linear_blur.wgsl` | `dir` anizotropik | 16 tap var ama **IGN jitter yok** | İzotropik yön + IGN jitter |
| `transition_burn.wgsl` | `leak_grad` düz çizgisel nokta çarpımı | Tek tap (opsiyonel soft leak) | **Gürültü/FBM yok:** organik film burn deliği, kor kenar ve light leak yok |
| `transition_ripple.wgsl` | `dist = length(in.uv - 0.5)` eliptik dalga | **Motion blur yok (1 tap)** | İzotropik dairesel dalga + IGN jitter'lı faz/radyal blur |
| `transition_kaleidoscope.wgsl` | Döndürme shear üretiyor | **Motion blur yok (1 tap)** | **Katlama bug'ı:** `abs(k) - fold_shift` dış bölgelerde katlamıyor, görüntü tek kadranda kilitleniyor |

### (c) `fx_halftone_glow.wgsl` & `fx_motion_tile_shake.wgsl` Denetimi
- `fx_motion_tile_shake.wgsl` halihazırda `aspect = dims.x / dims.y` ile aspect-düzeltmeli döndürme ve üçgen dalgalı `mirror_uv` kullanmaktadır; referans olarak temizdir (ek değişiklik gerektirmez).
- `fx_halftone_glow.wgsl` (`HalftoneGlowUniform` = **48 byte**, `glow.w` + `pad.xyzw` olmak üzere **5 boş `f32` slotu**):
  - Glow çekirdeği yalnızca 13 taplık tek/çift sabit altıgen halka kullanır (`offsets` yarıçapı `0.0, 0.5, 1.0`). `glow_radius_px >= 12` değerlerinde keskin yazı/çizgilerin etrafında belirgin altıgen nokta ekoları (ringing) üretir.
  - Screentone yalnızca tek renk (`0.96` / `0.02` siyah-beyaz) çalışır; renkli veya 4-plakalı CMYK matbaa raster modu yoktur.

### (d) `text2d_fx.wgsl` Denetimi
- Mevcut `text2d_fx.wgsl` tek fragment geçişinde:
  - `1` dolgu tap'i (`fill`)
  - `16` tap drop shadow (`golden` spiral)
  - `24` tap HDR outer glow (`golden` spiral)
  - `24 yön × 12 adım` (+ `1` midpoint arıtma) = **312 tap** raymarch anime konturu
  - **En kötü durum toplamı = 353 tap/piksel!**
- Ayrıca 24 ışınlık (`15°` aralıklı) ve 12 adımlı sabit raymarch kalın konturlarda köşelerde yıldız/poligon fasetleşmesi (starburst artifacts) ve ince seriflerde ray kaçırma üretir.

---

## Teknik Tasarım ve Uygulama Şartnamesi (Faz 2 Planı)

### Bölüm A: `fx_radial_shockwave.wgsl` — Sürekli Dalga Türevi & Blur-Entegre Spektral CA

1. **Sürekli Dalga Türevi (`C^∞` Gauss Türev Darbesi):**
   `sign(diff)` tamamen kaldırılır. `half_w = max(ring_width * 0.5, 1e-4)` ile normalize mesafe `u = (r - radius) / half_w` tanımlanır:
   - Pürüzsüz kompakt destek zarfı:
     `let active = select(1.0, 0.0, radius_param <= 0.0);`
     `let gauss = exp(-2.5 * u * u);`
     `let cutoff = 1.0 - smoothstep(0.9, 1.6, abs(u));`
     `let ring_mask = active * gauss * cutoff;`
   - Fiziksel basınç dalgasının analitik türevi ($-\frac{d}{du} e^{-2.5 u^2} \propto u e^{-2.5 u^2}$, tepe genliği `1.0`'a normalize $\sqrt{5e}/1 \approx 3.6865$):
     `let wave_deriv = active * (u * gauss * 3.6865) * cutoff;`
     `let refracted_uv = in.uv - dir_uv * (refract_amp * wave_deriv);`
   - **Sonuç:** `r == radius` (`u == 0`) noktasında `wave_deriv == 0.0` ve türev süreklidir; sıfır dikiş! Aynı polarite korunur (`u > 0` dış eğimde içeri doğru büyüteç, `u < 0` iç eğimde dışarı doğru sıkıştırma) ve `radius <= 0` iken `active == 0` olduğu için `shockwave_zero.rs` testleri **%100 korunur**.

2. **Blur Ekseniyle Entegre Spektral CA:**
   Sabit `uv_g ± ca_offset` yerine, `percent = (f32(i) + jitter) / f32(ZOOM_TAPS)` döngü değişkeni boyunca hem zoom blur hem halka kırılma ekseni spektral olarak ayrıştırılır:
   ```wgsl
   let ca_ring = ca_strength * 0.02 * ring_mask;
   let ca_zoom = ca_strength * (0.015 * clamp(r * 2.0, 0.0, 2.0) + 0.02 * ring_mask);
   // Her tap i için:
   let blur_r = to_center * (percent * (zoom_blur + ca_zoom * 0.85)) + dir_uv * (ca_ring * (0.5 + 0.5 * percent));
   let blur_g = to_center * (percent * zoom_blur);
   let blur_b = to_center * (percent * max(zoom_blur - ca_zoom * 0.85, -ca_zoom * 0.5)) - dir_uv * (ca_ring * (0.5 + 0.5 * percent));
   ```
   - `zoom_blur == 0.0` ve `ca_strength > 0.0` durumunda `dir_uv * ca_ring` ve `to_center * ca_zoom` yine 16 tap boyunca IGN-jitter'lı pürüzsüz spektral yayılım üretir (`shockwave_zero.rs::positive_radius_still_draws_the_ring` korunur).
   - Alfa biriktirme `let avg_a = (sr.a + sg.a + sb.a) * (1.0 / 3.0);` olarak güncellenir (premultiplied alpha sızıntısı önlenir).
   - **Uniform:** `RadialShockwaveUniform` (**48 byte**) değişmez.

---

### Bölüm B: 10 `transition_*.wgsl` Geçişinin Birleşik Yükseltmesi

1. **Ortak 16:9 / Keyfi En-Boy Oranı Düzeltmesi ve Uniform Stratejisi:**
   - `WhipTransitionUniform` **32 byte** kalır (`uniform_struct_sizes_match_wgsl_layouts` uyumu):
     - `flash_pad.y` = `aspect_ratio` (`width as f32 / height.max(1) as f32`; `compositor.rs` tarafından doldurulur).
     - WGSL güvenlik yedeği:
       ```wgsl
       let dims = max(vec2<f32>(textureDimensions(tex_a)), vec2<f32>(1.0));
       let aspect = select(u_trans.flash_pad.y, dims.x / dims.y, u_trans.flash_pad.y <= 1e-4);
       ```
     - Böylece `WhipTransitionUniform::new(...)` doğrudan çağrılsa bile tüm testler ve 16:9/9:16 tuvalleri otomatik doğru `aspect` alır.
   - Ortak yardımcı dönüşümler (tüm 10 shader'da):
     - `to_iso(uv_delta, aspect) = vec2<f32>(uv_delta.x * aspect, uv_delta.y)`
     - `from_iso(iso_delta, aspect) = vec2<f32>(iso_delta.x / aspect, iso_delta.y)`
     - `ign(frag_coord) = fract(52.9829189 * fract(dot(frag_coord, vec2<f32>(0.06711056, 0.00583715))))`
   - Tüm 10 shader'ın en başında kesin sınır koruması:
     `if (p <= 0.0) { return textureSampleLevel(tex_a, s_linear, clamp(in.uv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0); }`
     `if (p >= 1.0) { return textureSampleLevel(tex_b, s_linear, clamp(in.uv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0); }`

2. **IGN Jitter'lı Motion Blur (5 Ana Hedef + `cross_zoom` & `linear_blur`):**
   - **`transition_whip_chromatic.wgsl`:** `dir_iso = vec2(cos(d), sin(d))`, `dir_uv = from_iso(dir_iso, aspect) * mix(1.0, aspect, abs(dir_iso.x) * 0.45)` (yatay whip mesafesini korurken diyagonal açıyı gerçek 45° yapar). `BLUR_SAMPLES = 16`, `t = ((f32(i) + ign(in.position.xy)) / f32(BLUR_SAMPLES)) - 0.5`, parabolik/çadır ağırlık `1.0 - 1.5 * abs(t)`.
   - **`transition_dreamy_zoom.wgsl`:** `rotate_zoom_uv` izotropik uzayda döndürür (`p_iso = to_iso(uv - 0.5, aspect) * inv_scale`, döndür, `from_iso(r, aspect) + 0.5 + drift`). **12 tap** IGN jitter'lı zoom+dönme motion blur: `s = ((f32(i) + jitter) / 12.0 - 0.5) * bell * min(strength, 2.0)`, her tap'te `angle + s * 0.06`, `inv_scale * (1.0 + s * 0.14)`, çadır ağırlıklı.
   - **`transition_swirl.wgsl`:** İzotropik mesafe `uv_iso = to_iso(in.uv - 0.5, aspect)`, `SWIRL_RADIUS = 1.05` (16:9 köşe kapsama). **12 tap** IGN jitter'lı açısal motion blur (`gl-transitions/Swirl.glsl` uyumlu): `spread = clamp(radial_factor * strength * PI * cos(PI * p) * 0.08, -0.9, 0.9)` + `theta_tap = theta_base + s * spread`, teğetsel CA da `from_iso(tangent_iso, aspect)` ile ölçeklenir.
   - **`transition_ripple.wgsl`:** İzotropik dairesel dalga `uv_iso = to_iso(in.uv - 0.5, aspect)`, `dir_uv = from_iso(dir_iso, aspect)`. **8 tap** IGN jitter'lı radyal dalga hız blur'u: dalga tepe/çukur kayması `offset * (1.0 + s * 0.55 * env)` boyunca örneklenip çadır ağırlıkla toplanır.
   - **`transition_kaleidoscope.wgsl` (Gerçek Katlama Açısı + IGN Blur):**
     - Mevcut hatalı `abs(k) - fold_shift` yerine gerçek **KIFS (Kaleidoscopic Iterated Function System) + N-katmanlı açısal ayna katlaması**:
       1. İzotropik merkez: `p_iso = to_iso(in.uv - 0.5, aspect)` ve açısal spin `rot_spin = direction_rad + (p - 0.5) * 1.4 * strength`.
       2. Polar ayna sektörü (`KALEIDO_SECTORS = 8.0`, `sector = TAU / 8.0`):
          `a = theta - sector * floor(theta / sector);`
          `a_fold = abs(a - sector * 0.5);` (her sektör sınırında kesin $C^0$ sürekli ayna yansıması).
       3. Gerçek açılı KIFS düzlemsel katlama (`KALEIDO_FOLDS = 3`):
          Eksen-hizalı `abs(k)` yerine ayna normal vektörü `n = vec2<f32>(-sin(fold_ang), cos(fold_ang))` (`fold_ang = 0.523599 + 0.35 * env * strength`) ile gerçek düzlem yansıması:
          `k = k - 2.0 * min(dot(k, n), 0.0) * n;`
          ve `k -= vec2<f32>(cos(fold_ang), sin(fold_ang)) * (0.08 * env)` ile kaydırılır; ardından `sample_spin` döndürmesi uygulanarak `mirror_uv(from_iso(k, aspect) + 0.5)` örneklenir. Bu sayede katlanan görüntü tüm kareyi doldurur (tek kadranda sıkışmaz).
       4. **10 tap** IGN jitter'lı açısal+zoom motion blur (`s = ((f32(i) + jitter) / 10.0 - 0.5) * env * 0.12 * min(strength, 2.5)`): `p` ve `rot_spin` blur penceresi boyunca taranır.
   - **`transition_cross_zoom.wgsl` & `transition_linear_blur.wgsl` & `transition_directional_warp.wgsl`:**
     - `cross_zoom`: `center` süpürmesi `from_iso(dir, aspect)`, `rand_screen(in.uv)` → `ign(in.position.xy)`.
     - `linear_blur`: `span` ve `ca_offset` için `from_iso(dir, aspect)` + `ign(in.position.xy)` jitter.
     - `directional_warp`: `proj`, `wave`, `pull`, `ca_offset` hesapları `uv_iso = to_iso(in.uv - 0.5, aspect)` üzerinden yapılır.

3. **`transition_burn.wgsl` — Organik Gürültülü Film Burn / Light Leak:**
   `BurnThrough.glsl` + `FilmBurn.glsl` (MIT) çekirdeklerinden Linear Rec.709'a uyarlanır:
   - Sin-tabanlı kararsız hash yerine GPU-deterministik **2D Value/Gradient Noise + 4-Oktavlı Döndürülmüş FBM** (`mat2x2(0.8, -0.6, 0.6, 0.8)`) izotropik `uv_iso = vec2(in.uv.x * aspect, in.uv.y)` üzerinde çalışır.
   - **Yanma Maskesi (`burn_mask`):** Yönlü süpürme + FBM birleşimi:
     `field = 0.62 * fbm(uv_iso * 3.2 + dir * (p * 0.9)) + 0.38 * (0.5 + 0.5 * dot(uv_iso, dir));`
     `thresh = p * 1.18 - 0.09;`
     `mask = smoothstep(thresh - 0.08, thresh + 0.08, field);` (`mask = 1` → `tex_a`, `mask = 0` → `tex_b`).
   - **Sıcak Kor Kenar + Organik Light Leak:**
     - `ember = 4.0 * mask * (1.0 - mask);` (koyu kızıl `(1.3, 0.16, 0.02)` → kehribar `(2.8, 1.15, 0.28)` HDR parıltı).
     - `leak = pow(max(fbm(uv_iso * 1.6 - dir * (p * 1.4) + vec2(3.7, 1.9)), 0.0), 2.0) * burn_env;` (yumuşak organik film ışık sızıntısı).
     - `heat_warp`: FBM türev farkı ile `burn_env * 0.012 * strength` mikro ısı titreşimi.
     - Premultiplied alpha güvenliği: `(ember_rgb + leak_rgb + white_flash) * max(res.a, 0.0)`.

4. **`transition_glitch_displace.wgsl` — Zaman-Sıçramalı Dinamik Voronoi Glitch:**
   - Sabit `+ 1.0 / 4.0 / 17.0` ofsetleri yerine kuantize zaman adımı:
     `let step_slow = floor(p * 12.0 + direction_rad * 3.17);`
     `let step_fast = floor(p * 28.0 + direction_rad * 7.43);`
     `let row_band = floor(in.uv.y * 26.0);`
     `let row_jitter = (hash12(vec2<f32>(row_band, step_fast)) - 0.5) * 0.09 * bell_env * strength * step(0.68, hash12(vec2<f32>(row_band + 19.0, step_slow)));`
   - `uv_iso = vec2<f32>(in.uv.x * aspect + row_jitter, in.uv.y)` üzerinde dinamik kaydırılmış Voronoi çağrıları:
     `vor1 = voronoi(uv_iso * 2.5 + hash21(step_slow) * 13.0)`
     `vor2 = voronoi(uv_iso * 14.0 + hash21(step_fast) * 29.0)`
     `vor3 = voronoi(uv_iso * 52.0 + hash21(step_fast + 7.0) * 47.0)`
   - Yer değiştirme ve CA vektörleri `from_iso(..., aspect)` ile döndürülür; `p` başına %100 deterministik ve seek-independent kalır.

---

### Bölüm C: `fx_halftone_glow.wgsl` — Çoklu Halka/Yumuşak Glow & Renkli/CMYK Screentone

1. **Çoklu Halka / Yumuşak Vogel-Disk Glow (Tek Geçiş, `Effect1D` Uyumlu):**
   - 13 taplı tek-halka altıgen örnekleyici yerine **28 taplı çoklu-halka / Vogel spiral + IGN mikro-döndürmeli** yumuşak HDR bloom çekirdeği:
     - 3 eş merkezli halka ağırlık dağılımı (iç çekirdek $r \in [0, 0.45 R]$, orta hale $[0.45 R, 1.05 R]$, geniş dış kuyruk $[1.05 R, 2.0 R]$), toplam ağırlık normalize $\sum w_i = 1.0$:
       ```wgsl
       const GLOW_TAPS: i32 = 28;
       const GOLDEN_ANGLE: f32 = 2.39996323;
       let base_rot = ign(in.position.xy) * 6.2831853;
       for (var i: i32 = 0; i < GLOW_TAPS; i = i + 1) {
           let t = (f32(i) + 0.5) / f32(GLOW_TAPS);
           let r_norm = pow(t, 0.72) * 1.95; // Geniş yumuşak dış kuyruk
           let ang = f32(i) * GOLDEN_ANGLE + base_rot;
           let ofs = vec2<f32>(cos(ang) * r_norm * step_uv.x, sin(ang) * r_norm * step_uv.y);
           let w = exp(-1.65 * r_norm * r_norm);
           ...
       }
       ```
     - **Avantaj:** `28 < 128` (TDR güvenli), sıfır yeni doku/geçiş, `glow_radius_px = 24+` değerlerinde dahi altıgen nokta ekolarını tamamen yok eder.

2. **Monochrome / CMYK / Renkli Screentone Modları (`color_mode`):**
   `Vidvox/ISF-Files/CMYK Halftone.fs` ve `stackgl/glsl-halftone` (MIT) uyarlaması:
   - Ekran türevleri (`dpdx`/`dpdy`) daima **uniform control flow** içinde hesaplanır.
   - `halftone_mode` (`u32(round(params.glow.w))`):
     - `0` (`Monochrome` — varsayılan, mevcut JSON'larla geriye dönük uyumlu): Mevcut manga B&W noktaları + ayarlanabilir `dot_softness`.
     - `1` (`Cmyk`):
       1. Linear Rec.709 rengi `[0, 1]` ton eğrisine sıkıştırılır (`tone = base_rgb / (1.0 + base_rgb * 0.25)`), algısal CMYK uzayına geçirilir (`s = pow(clamp(tone, 0.0, 1.0), vec3(1.0 / 2.2))`), UCR/GCR ile `(C, M, Y, K)` ayrıştırılır:
          `k = 1.0 - max(s.r, max(s.g, s.b));`
          `cmy = (1.0 - s - k) / max(1.0 - k, 1e-4);`
       2. 4 plaka standart rosette açılarında (`angle_rad + {15°, 75°, 0°, 45°}`) `halftone_cell_dot(st, dot_scale, plate_ang, amount, aa_width)` değerlendirilir.
       3. Subtractive CMYK yeniden birleştirilip Linear Rec.709'a geri dönülür (`rgb_lin = pow((1.0 - cmy_dot) * (1.0 - k_dot), vec3(2.2))`), ardından `max(base_rgb - 1.0, vec3(0.0))` HDR taşması eklenir.
     - `2` (`ColorDot`): Orijinal rengi koruyarak nokta yarıçapını ters luma yerine renk doygunluğu + luma ile modüle eder (`mix(base_rgb * 0.12, base_rgb, dot_mask)` — neon/pop-art anime editleri için).

3. **Şema & Uniform Planı (`HalftoneGlowUniform` = 48 byte sabit):**
   - `HalftoneGlowUniform`:
     - `halftone`: `[dot_scale, angle_rad, halftone_mix, aspect_ratio]`
     - `glow`: `[glow_threshold, glow_intensity, glow_radius_px, color_mode as f32]`
     - `pad`: `[dot_softness, 0.0, 0.0, 0.0]`
   - `NodeKind::HalftoneGlow` ([graph.rs](../../crates/frameshift-schema/src/graph.rs#L553-L566)):
     - Yeni opsiyonel alanlar (`#[serde(default)]` ile eski JSON'ları bozmaz):
       - `color_mode: HalftoneColorMode` (`Monochrome` [default], `Cmyk`, `ColorDot`)
       - `dot_softness: f32` (varsayılan `0.0`, `[0..1]`)
     - `HalftoneGlow` yapı literal yerleri (toplam 7 dosya/konum): `graph.rs`, `project.rs`, `compositor.rs`, `inspection.rs`, `snapshot.rs`, `compositor_tests.rs`, `schema_tests.rs`.

---

### Bölüm D: `text2d_fx.wgsl` — 352 Tap/px Raymarch Yerine Jump Flooding Algorithm (JFA)

1. **Neden 2-Pass Separable EDT Yerine JFA Seçildi?**

| Kriter | 2-Pass Separable EDT (Felzenszwalb 2012) | Jump Flooding Algorithm (JFA, Rong & Tan 2006) |
| :--- | :--- | :--- |
| **Mimari Uyumu** | Tam satır/sütun taraması compute shader + `STORAGE_BINDING` gerektirir; mevcut `TexturePool` ve `ShaderPipelines` tamamen render-pass tabanlıdır | Mevcut `TexturePool` (`Rgba16Float`, `RENDER_ATTACHMENT \| TEXTURE_BINDING`) ve `build_program` render pipeline altyapısını **%100 yeniden kullanır** |
| **TDR Döngü Sınırı (`<= 128`)** | 1080p/4K tam satır taraması (`1920`/`3840` adım) `MAX_SHADER_LOOP_ITERATIONS = 128` sınırını aşar; sınırlı pencere `[-R, R]` ise $R=64$'te $129\times 2 = 258$ tap gerektirir | Her JFA adımı yalnızca sabit **`3×3 = 9 textureLoad`** yapar (döngü yok / sabit 9) |
| **Çift Yönlü SDF (İç+Dış)** | Ayrı iç/dış çalıştırma gerektirir | Tek `Rgba16Float` dokuda `RG = ext_delta`, `BA = int_delta` ile **sıfır ek geçişte** gerçek işaretli mesafe (SDF) üretir |
| **Toplam Maliyet (`R_max = 64 px`)** | Compute rewrite + düşük thread doluluğu (`H` thread) | 1 Seed + 6 JFA adım (`32,16,8,4,2,1`) + 1 Composite = **~65 op/px** (eski **353 tap/px**'ten **~5.4x az**, küçük konturlarda **~8x–10x az**) |

2. **Hassasiyet Kanıtı — `Rgba16Float` İçinde Göreli Piksel Delta Kodlaması:**
   - **Sorun:** `f16` yalnızca 10 bit mantisa sahiptir; mutlak piksel koordinatı `[0, 1920]` veya mutlak UV `[0, 1]` `f16` içinde 1080p/4K'da `0.5–2.0 px` kuantize olur ve köşeleri/kenarları tırtıklar.
   - **Çözüm:** Her piksel mutlak tohum konumu yerine **en yakın kenara göreli piksel farkını `(dx, dy)`** saklar:
     - `RG`: Dış alan için en yakın iç/kenar noktasına vektör `(ext_dx, ext_dy)` (geçersiz/ulaşılamaz sentinel: `vec2<f32>(16384.0, 16384.0)` — `f16` sınırı `65504.0` içinde güvenli).
     - `BA`: İç alan için en yakın dış/kenar noktasına vektör `(int_dx, int_dy)`.
     - JFA adımında `q = clamp(coord + offset, vec2<i32>(0), dims - 1)` noktasından `textureLoad(t_jfa, q, 0)` okunur (asla bilinear `textureSampleLevel` kullanılmaz, yoksa komşu tohumlar araya girip hayalet tohum üretir!), ve `cand_delta = q_delta + vec2<f32>(offset)` olarak güncellenir.
     - Kenar yakınında (`|dx|, |dy| <= 2 px`) `f16` hassasiyeti **`<= 0.002 px`**, kontur bölgesinde (`<= 64 px`) **`<= 0.0625 px`** olur — 4K'da bile pürüzsüz sub-pixel kenarlar ve yuvarlak köşeler sağlar!

3. **Sub-Pixel Tohumlama (`text2d_jfa_seed.wgsl`):**
   - `t_text` (`text_slug.wgsl` çıktısı) üzerinden `textureLoad(t_text, coord, 0).a` ve 4-komşu merkez farkı $\nabla \alpha = (a_{R} - a_{L}, a_{U} - a_{D})$ hesaplanır.
   - Sınır piksellerinde (`0.001 < a < 0.999`): normalize gradyan $\hat{n} = \nabla \alpha / \max(\|\nabla \alpha\|, 10^{-4})$ boyunca sub-pixel kenar kayması $s = (0.5 - a)$ hesaplanır:
     - `ext_delta = select(vec2(0.0), -n_hat * s, a < 0.5)` (dışta ise kenara tam `0.5 - a` piksel mesafe!)
     - `int_delta = select(vec2(0.0), n_hat * (-s), a >= 0.5)` (içte ise kenara tam `a - 0.5` piksel mesafe!)
   - Tam iç (`a >= 0.999`): `ext_delta = vec2(0.0)`, `int_delta = SENTINEL`.
   - Tam dış (`a <= 0.001`): `ext_delta = SENTINEL`, `int_delta = vec2(0.0)`.

4. **JFA Adım Sayısı ve Ping-Pong (`text2d_jfa_step.wgsl`):**
   - `max_reach_px = ceil(max(outline_total, max(glow_radius_px, shadow_blur_px))).clamp(2.0, 128.0)`.
   - Başlangıç adımı `start_step = (max_reach_px.ceil() as i32).next_power_of_two() / 2` (ör. `max_reach = 16` → adımlar `8, 4, 2, 1`; `max_reach = 64` → `32, 16, 8, 4, 2, 1`).
   - `TexturePool`'dan alınan 2 adet `Rgba16Float` geçici doku arasında ping-pong yapılır; döngü bitiminde serbest bırakılır (`pool.release`).
   - Her adımda 16 byte'lık `Text2DJfaUniform` (`step_px: i32, _pad0: i32, texel_x: f32, texel_y: f32`) bağlanır.

5. **Son Kompozisyon (`text2d_fx.wgsl` — 4 Tap/Piksel):**
   - Bağlama düzeni (`Effect2D`): `@binding(0) t_text`, `@binding(1) t_sdf` (JFA sonucu), `@binding(2) s_linear`, `@binding(3) u: Text2DFxUniform` (**112 byte** sabit!).
   - İşaretli mesafe yardımcı fonksiyonu (sınır kontrollü):
     `sd(px) = d_ext - d_int` (dışta pozitif, içte negatif, kenarda `0.0`).
   - **4 Katmanlı Çizim (toplam ~4 doku okuması):**
     1. **Drop Shadow:** `sh_uv = in.uv - u.shadow_offset.xy * texel`. Eğer `sh_uv` `[0,1]^2` dışındaysa `sh_cov = 0.0` (clamp uzaması düzeltmesi!). `shadow_blur_px <= 0.5` için `textureSampleLevel(t_text, s_linear, sh_uv, 0.0).a`, aksi halde `sd(sh_uv)` üzerinden `sh_cov = 1.0 - smoothstep(-0.5 * shadow_blur_px, 0.5 * shadow_blur_px, sh_sd)` (**1 tap**).
     2. **HDR Outer Glow:** `d_out = length(sdf.xy)` üzerinden analitik yumuşak Gauss düşümü `glow_cov = exp(-(d_out * d_out) / max(0.32 * glow_radius_px * glow_radius_px, 1e-3)) * (1.0 - smoothstep(glow_radius_px * 0.85, glow_radius_px * 1.35, d_out))` (**0 ek tap**, `sdf` zaten okundu).
     3. **İç & Dış Anime Konturu + Knockout Boşluğu:** `d_out` üzerinden `clamp(edge + 0.5 - d_out, 0.0, 1.0)` ile pürüzsüz yuvarlak köşeli ikili kontur (yıldız/poligon artefaktı sıfır!).
     4. **Slug HDR Dolgu:** `fill = textureSampleLevel(t_text, s_linear, in.uv, 0.0)` en üste `source_over`.

---

## Faz 2 Değişecek Dosyalar & Uniform Özeti

| Dosya | Değişiklik Özeti |
| :--- | :--- |
| `crates/frameshift-render/shaders/fx_radial_shockwave.wgsl` | `sign(diff)` → `wave_deriv` sürekli Gauss türevi; blur ekseniyle entegre spektral CA + ortalama alfa |
| `crates/frameshift-render/shaders/transition_*.wgsl` (10 dosya) | 16:9 `aspect` (`flash_pad.y` + `textureDimensions(tex_a)`), `ign()` motion blur (`whip_chromatic`, `dreamy_zoom`, `swirl`, `ripple`, `kaleidoscope`, `cross_zoom`, `linear_blur`), FBM film burn (`burn`), zaman-sıçramalı Voronoi (`glitch_displace`), gerçek KIFS katlama (`kaleidoscope`) |
| `crates/frameshift-render/shaders/fx_halftone_glow.wgsl` | 28-tap Vogel+IGN yumuşak multi-ring glow; `Monochrome`/`Cmyk`/`ColorDot` screentone modları |
| `crates/frameshift-render/shaders/text2d_jfa_seed.wgsl` *(yeni)* | Sub-pixel gradyan kenar tohumlamalı çift-yönlü JFA başlangıç pass'i |
| `crates/frameshift-render/shaders/text2d_jfa_step.wgsl` *(yeni)* | `textureLoad` tabanlı 3×3 JFA sıçrama pass'i |
| `crates/frameshift-render/shaders/text2d_fx.wgsl` | 352 tap raymarch → `t_text + t_sdf` (4 tap) JFA SDF kompozisyon |
| `crates/frameshift-render/src/uniforms.rs` | `WhipTransitionUniform` (32B), `RadialShockwaveUniform` (48B), `HalftoneGlowUniform` (48B), `Text2DFxUniform` (112B) boyutları korunur; `Text2DJfaUniform` (16B) eklenir |
| `crates/frameshift-render/src/pipelines.rs` | `text2d_jfa_seed` (`1` tex), `text2d_jfa_step` (`1` tex), `text2d_fx` (`2` tex) pipeline'ları |
| `crates/frameshift-render/src/compositor.rs` | `ShaderTransition` için `aspect_ratio` geçirilmesi; `render_vector_text` içine JFA seed+ping-pong; `HalftoneGlow` `color_mode`/`dot_softness` geçirilmesi |
| `crates/frameshift-schema/src/{graph.rs, project.rs}` + `snapshot.rs`, `inspection.rs` | `HalftoneColorMode` (`#[serde(default)]`) ve `HalftoneGlow` yapı literallerinin güncellenmesi |

---

## M1 & Mevcut Test Uyumluluk Stratejisi

1. **Sabit Uniform Boyut Assert'leri ([uniforms.rs](../../crates/frameshift-render/src/uniforms.rs#L839-L868), [compositor_tests.rs](../../crates/frameshift-render/tests/compositor_tests.rs#L204-L216)):**
   `WhipTransitionUniform` (`32`), `RadialShockwaveUniform` (`48`), `HalftoneGlowUniform` (`48`), `Text2DFxUniform` (`112`) byte boyutları **1 byte bile değiştirilmeyecek**, yalnızca mevcut `_pad`/`reserved` yuvaları kullanılacaktır; yeni `Text2DJfaUniform` ise `16` byte (`% 16 == 0`) olarak `uniforms.rs` derleme-zamanı assert'lerine eklenecektir.
2. **M1 Golden Scene Doğrulaması ([engine.rs](../../crates/frameshift-core/src/engine.rs#L255-L450)):**
   `verify_m1_golden_scene` sabit hardcode edilmiş hex hash beklemez; determinizm (`pass1 == pass2`), seek-bağımsızlık (`seq == seek`), motion blur farkı (`final_hash != draft_hash`, `mean_l1_channel_diff > 0.1`) ve ses senkronunu doğrular. IGN jitter piksel koordinatına (`in.position.xy`) bağlı saf deterministik fonksiyon olduğu için tüm M1 kapıları korunur.
3. **Sınır ve Regresyon Testleri:**
   - [shockwave_zero.rs](../../crates/frameshift-render/tests/shockwave_zero.rs): `radius <= 0.0` hem `ring_mask` hem `wave_deriv`'i sıfırladığı için tüm 3 test bit-birebir geçer.
   - [faz6_1_tests.rs](../../crates/frameshift-render/tests/faz6_1_tests.rs#L184-L200): Her `transition_*.wgsl` `p <= 0.0` → saf `tex_a`, `p >= 1.0` → saf `tex_b` erken dönüşünü korur; determinizm ve benzersizlik garantilenir.
   - [faz6_5_tests.rs](../../crates/frameshift-render/tests/faz6_5_tests.rs#L44-L162): JFA SDF `outline + outer_outline + gap + glow + shadow` görünür kapsama alanını genişletmeye devam eder ve `bare_a == bare_b` determinizmi korunur.
   - [schema_tests.rs](../../crates/frameshift-schema/tests/schema_tests.rs#L516-L569): `HalftoneColorMode::Monochrome` ve `dot_softness = 0.0` `#[serde(default)]` ile minimalist legacy JSON'u geriye dönük bozmadan deserialize eder.
