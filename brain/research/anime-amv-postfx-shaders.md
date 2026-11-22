---
title: Anime / AMV 2D Post-İşleme Shader Aileleri (MIT/CC0) ve FrameShift WGSL Uyarlama Şartnamesi
date: 2026-11-21
status: active
tags: [shaders, render-graph, licensing]
---

# Anime / AMV 2D Post-İşleme Shader Aileleri (MIT/CC0) ve FrameShift WGSL Uyarlama Şartnamesi

## Soru / Amaç

FrameShift'in anime edit / AMV ve motion-graphics odaklı GPU kompozisyon motoru (`frameshift-render`) için 4 temel 2D post-işleme efekt ailesi:
1. **Motion Tile & Sönümlü Kamera Sarsıntısı (`fx_motion_tile_shake.wgsl`)**
2. **Anime Impact Frame & Radyal Hız Çizgileri (`fx_anime_impact_frame.wgsl`)**
3. **Radyal Şok Dalgası & Kromatik Zoom Bulanıklığı (`fx_radial_shockwave.wgsl`)**
4. **Manga Halftone / Screentone & Derin Anime Parıltısı (`fx_halftone_glow.wgsl`)**

açık kaynaklı ve izin verici (**MIT**, **CC0 / Public Domain**) birincil shader depolarında nasıl gerçeklenmektedir; bu kaynakların matematiksel çekirdekleri nelerdir ve FrameShift'in kilitli sözleşmelerine (`effect-shader-api.md` ve `render-graph.md`: `Rgba16Float`, `LinearRec709`, **Premultiplied Alpha**, `@group(0)` tek giriş `Effect1D`, 16-byte hizalı `uniform` yapıları, `SafeExpr` zaman/event bağlaması ve Naga TDR sabit döngü sınırları) nasıl deterministik biçimde uyarlanmalıdır?

---

## İncelenen Birincil Kaynaklar

Tüm harici depolar, lisans dosyaları ve ham shader kodları `2026-11-21` tarihinde `curl.exe -sL` veya doğrudan sayfa içerik çıkarımı ile okunarak doğrulanmıştır. (404 dönen veya ticari olmayan Shadertoy kaynakları kapsam dışında bırakılmıştır.)

### 1. VIDVOX ISF-Files (`Vidvox/ISF-Files`) — MIT Lisansı
- **Lisans Doğrulaması:** [GitHub License API (`MIT`)](https://api.github.com/repos/Vidvox/ISF-Files/license) — erişim tarihi: `2026-11-21`
- **Motion Tile & Shake:**
  - [Quad Tile.fs](https://raw.githubusercontent.com/Vidvox/ISF-Files/master/ISF/Quad%20Tile.fs) — erişim tarihi: `2026-11-21`
  - [Mirror Edge.fs](https://raw.githubusercontent.com/Vidvox/ISF-Files/master/ISF/Mirror%20Edge.fs) — erişim tarihi: `2026-11-21`
  - [Shake.fs](https://raw.githubusercontent.com/Vidvox/ISF-Files/master/ISF/Shake.fs) — erişim tarihi: `2026-11-21`
- **Kenar Tespiti / Negatif Flaş:**
  - [Edges.fs](https://raw.githubusercontent.com/Vidvox/ISF-Files/master/ISF/Edges.fs) — erişim tarihi: `2026-11-21`
  - [Edge Trace.fs](https://raw.githubusercontent.com/Vidvox/ISF-Files/master/ISF/Edge%20Trace.fs) — erişim tarihi: `2026-11-21`
  - [RGB Invert.fs](https://raw.githubusercontent.com/Vidvox/ISF-Files/master/ISF/RGB%20Invert.fs) — erişim tarihi: `2026-11-21`
- **Şok Dalgası / Kromatik Zoom:**
  - [Shockwave.fs](https://raw.githubusercontent.com/Vidvox/ISF-Files/master/ISF/Shockwave.fs) — erişim tarihi: `2026-11-21`
  - [Shockwave Pulse.fs](https://raw.githubusercontent.com/Vidvox/ISF-Files/master/ISF/Shockwave%20Pulse.fs) — erişim tarihi: `2026-11-21`
  - [Chroma Zoom.fs](https://raw.githubusercontent.com/Vidvox/ISF-Files/master/ISF/Chroma%20Zoom.fs) — erişim tarihi: `2026-11-21`
  - [Chromatic Aberration.fs](https://raw.githubusercontent.com/Vidvox/ISF-Files/master/ISF/Chromatic%20Aberration.fs) — erişim tarihi: `2026-11-21`
  - [CrossZoom.fs](https://raw.githubusercontent.com/Vidvox/ISF-Files/master/ISF/CrossZoom.fs) (gl-transitions / rectalogic MIT portu) — erişim tarihi: `2026-11-21`
- **Halftone & Bloom:**
  - [CMYK Halftone.fs](https://raw.githubusercontent.com/Vidvox/ISF-Files/master/ISF/CMYK%20Halftone.fs) — erişim tarihi: `2026-11-21`
  - [RGB Halftone.fs](https://raw.githubusercontent.com/Vidvox/ISF-Files/master/ISF/RGB%20Halftone.fs) — erişim tarihi: `2026-11-21`
  - [Bloom.fs](https://raw.githubusercontent.com/Vidvox/ISF-Files/master/ISF/Bloom.fs) — erişim tarihi: `2026-11-21`

### 2. Godot Shaders Topluluk Kaynakları — MIT & CC0
- [Screen Shake (`@Near`)](https://godotshaders.com/shader/screen-shake/) — Lisans: **MIT**, erişim tarihi: `2026-11-21`
- [Speed lines](https://godotshaders.com/shader/speed-lines/) — Lisans: **CC0**, erişim tarihi: `2026-11-21`
- [Positioned Shockwave](https://godotshaders.com/shader/positioned-shockwave/) — Lisans: **MIT**, erişim tarihi: `2026-11-21`
- [Radial Chromatic Aberration](https://godotshaders.com/shader/radial-chromatic-aberration/) — Lisans: **CC0**, erişim tarihi: `2026-11-21`
- [Manga Shader (`Joy-less/MangaShader`)](https://godotshaders.com/shader/manga-shader/) — Lisans: **MIT** ([GitHub repo doğrulaması](https://api.github.com/repos/Joy-less/MangaShader)), erişim tarihi: `2026-11-21`

### 3. `evanw/glfx.js`, `stackgl/glsl-halftone` & `libretro/slang-shaders` — MIT & CC0
- **`evanw/glfx.js` (MIT):** [LICENSE](https://raw.githubusercontent.com/evanw/glfx.js/master/LICENSE) ve [zoomblur.js](https://raw.githubusercontent.com/evanw/glfx.js/master/src/filters/blur/zoomblur.js) — erişim tarihi: `2026-11-21`
- **`stackgl/glsl-halftone` & `glsl-aastep` (MIT):** [glsl-halftone/LICENSE.md](https://raw.githubusercontent.com/stackgl/glsl-halftone/master/LICENSE.md), [glsl-halftone/index.glsl](https://raw.githubusercontent.com/stackgl/glsl-halftone/master/index.glsl), [glsl-aastep/LICENSE.md](https://raw.githubusercontent.com/stackgl/glsl-aastep/master/LICENSE.md), [glsl-aastep/index.glsl](https://raw.githubusercontent.com/stackgl/glsl-aastep/master/index.glsl) — erişim tarihi: `2026-11-21`
- **`libretro/slang-shaders` Dual Filter Blur & Bloom (`fishku`, CC0 Public Domain):** [dual_filter.inc](https://raw.githubusercontent.com/libretro/slang-shaders/master/blurs/shaders/dual_filter/dual_filter.inc), [downsample_bloom.slang](https://raw.githubusercontent.com/libretro/slang-shaders/master/blurs/shaders/dual_filter/downsample_bloom.slang), [upsample.slang](https://raw.githubusercontent.com/libretro/slang-shaders/master/blurs/shaders/dual_filter/upsample.slang), [bloom_blend.slang](https://raw.githubusercontent.com/libretro/slang-shaders/master/blurs/shaders/dual_filter/bloom_blend.slang), [tonemapping.inc](https://raw.githubusercontent.com/libretro/slang-shaders/master/blurs/shaders/dual_filter/tonemapping.inc) — erişim tarihi: `2026-11-21`

### 4. FrameShift İçi Sözleşme ve Referans Shader'lar
- [effect-shader-api.md](../specs/effect-shader-api.md) (`locked`)
- [render-graph.md](../specs/render-graph.md) (`locked`)
- [color_basic.wgsl](../../crates/frameshift-render/shaders/color_basic.wgsl) ve [transform.wgsl](../../crates/frameshift-render/shaders/transform.wgsl)

---

## Doğrudan Gözlemler (Kanıtlı)

### Aile 1: Motion Tile & Kamera Sarsıntısı (Camera Shake)
1. **Ayna Döşeme (Mirror / Ping-Pong Tile):**
   - `Vidvox/ISF-Files/ISF/Mirror Edge.fs` doğrudan `loc = mod(loc + modifiedCenter, 1.0); loc = 2.0 * abs(loc - 0.5);` ile UV'yi `[0, 1]` içinde 4 yönlü yansıtır.
   - `Vidvox/ISF-Files/ISF/Quad Tile.fs` önce dönme + ölçekleme + kaydırma (`slide1`/`slide2`) uygular, ardından `point = mod(point, 1.0); point = 1.0 - abs(1.0 - 2.0 * point);` formülüyle üçgen dalga katlaması yapar. Bu ifade `1.0 - abs(fract(uv * 0.5) * 2.0 - 1.0)` ile matematiksel olarak eşdeğer üçgen dalgadır; `uv ∈ [0, 1]` aralığında kimlik (`uv`) davranışı verirken `[0, 1]` dışına taşan piksel koordinatlarını dikişsiz biçimde geri aynalar.
2. **Kamera Sarsıntısı ve Kenar Taşmasını Önleyen Mikro-Zoom:**
   - `Vidvox/ISF-Files/ISF/Shake.fs` zamana bağlı sinüzoidal/rastgele X-Y kayması (`sin`/`cos` + `rand`) ile birlikte `zoom = 1.0 + offset` ve `uv = (uv - 0.5) / zoom + 0.5` uygulayarak kenarların tuval dışına çıkmasını azaltır.
   - `godotshaders.com/shader/screen-shake/` (MIT) benzer şekilde `uv = (uv - 0.5) * (1.0 - 2.0 * magnitude.x) + 0.5` ön yakınlaştırması yapar ve `dt = vec2(sin(t * fx + bx), cos(t * fy + by)) * magnitude * ShakeStrength` ekler.

### Aile 2: Anime Impact Frame & Radyal Hız Çizgileri (Speed Lines)
1. **3×3 Sobel Kenar Tespiti & Tersleme:**
   - `Vidvox/ISF-Files/ISF/Edges.fs` 3×3 komşuluk üzerinden klasik Sobel operatörü uygular:
     $$G_x = (-1 \cdot L_{LA} - 2 \cdot L_L - 1 \cdot L_{LB}) + (1 \cdot L_{RA} + 2 \cdot L_R + 1 \cdot L_{RB})$$
     $$G_y = (1 \cdot L_{LA} + 2 \cdot L_A + 1 \cdot L_{RA}) + (-1 \cdot L_{RB} - 2 \cdot L_B - 1 \cdot L_{LB})$$
     $$M = \sqrt{G_x^2 + G_y^2}$$
     ve `M < threshold` koşulunda siyah eşikleme uygular. (Not: Kaynakta `gray(n) = (r + g + b) / 3.0` kullanılmıştır; `LinearRec709` için Rec.709 luma ağırlıkları gerekir.)
   - `Edge Trace.fs` `blurred ± final * intensity` ile siyah veya beyaz çizgi (`invert_lines`) seçeneği sunar; `RGB Invert.fs` ise `1.0 - src.rgb` negatif flaş üretir.
2. **Polar Hız Çizgileri (Radial Speed Lines):**
   - `godotshaders.com/shader/speed-lines/` (CC0) fragment koordinatını merkeze göre polar uzaya çevirir:
     $$\text{dir} = uv - \text{center}, \quad r = \|\text{dir}\| \cdot 2.0, \quad \theta_{norm} = \frac{\operatorname{atan2}(\text{dir}_y, \text{dir}_x)}{2\pi}$$
   - Açı ekseni ayrık adımlara (`polarStep`) kuantize edilerek gürültü haritasından okunur ve merkezden uzaklığa (`distance(center, uv)`) bağlı bir `falloff` maskesiyle çarpılarak karakterin ortası temiz bırakılır.

### Aile 3: Radyal Şok Dalgası & Kromatik Zoom Bulanıklığı
1. **Halka İçi UV Kırılması (Shockwave Refraction):**
   - `Vidvox/ISF-Files/ISF/Shockwave.fs` merkezden uzaklık `dist = distance(uv, center)` değerini ilerleyen halka yarıçapı `adjustedTime` ile karşılaştırır; `|dist - adjustedTime| <= magnitude` bandı içinde:
     $$\text{diff} = \text{dist} - R, \quad \text{powDiff} = 1.0 - |\text{diff} \cdot \text{distortion}|^{0.8}, \quad uv' = uv + \widehat{(uv - c)} \cdot (\text{diff} \cdot \text{powDiff})$$
     uygular.
   - `Vidvox/ISF-Files/ISF/Shockwave Pulse.fs` aynı kırılma çekirdeğini `PERSISTENT: true` 1×1 piksel tamponuyla tetikler (FrameShift'in seek-independent sözleşmesine aykırı olduğu için doğrudan zaman/event farkıyla değiştirilmelidir).
   - `godotshaders.com/shader/positioned-shockwave/` (MIT) elips bozulmasını engellemek için en-boy oranı (`ratio`) düzeltmesiyle `scaledUV` kullanır ve çift `smoothstep` ile yumuşak halka maskesi (`mask = (1 - smoothstep(s - f, s, r)) * smoothstep(s - w - f, s - w, r)`) oluşturur.
2. **Radyal Zoom Blur & Premultiplied Alpha:**
   - `evanw/glfx.js` (`zoomblur.js`, MIT) ve `CrossZoom.fs` (MIT) merkezden dışa `toCenter = center - uv` vektörü boyunca sabit adımlı örnekler toplar (`percent = (t + jitter) / N`), parabolik ağırlık `weight = 4.0 * (percent - percent * percent)` uygular ve şeffaf kenarlarda siyah halo oluşmaması için **premultiplied alpha** uzayında (`sample.rgb *= sample.a`, toplama, bölme) biriktirir.
3. **Kromatik Ayrışma (RGB Dispersion):**
   - `Vidvox/ISF-Files/ISF/Chroma Zoom.fs` R/G/B kanallarını merkez etrafında farklı zoom çarpanlarıyla (`1 / (red_zoom * master_zoom)`) örnekler.
   - `Vidvox/ISF-Files/ISF/Chromatic Aberration.fs` ise lens davranışı için radyal düşüm `scale = pow(r * 2.0, falloff)` ve `balance` parametresiyle kırmızı ve mavi kanalları zıt yönlerde (`+scale * redAmt`, `-scale * blueAmt`) kaydırır.
   - `godotshaders.com/shader/radial-chromatic-aberration/` (CC0) mavi kanalı sabit tutup yeşil ve kırmızı kanalları merkeze doğru yarı/tam adımla kaydırarak kenarlarda bilgi kaybını azaltır.

### Aile 4: Manga Halftone / Screentone & Derin Anime Parıltısı (Deep Glow)
1. **Döndürülmüş 2D SDF Nokta Izgarası (Halftone / Screentone):**
   - `stackgl/glsl-halftone` (`index.glsl`, MIT — Stefan Gustavson public-domain algoritması) koordinatı dönüş matrisiyle (`mat2(cos, -sin, sin, cos)`) döndürür (`K: 45°`, `C: 15°`, `M: 75°`, `Y: 0°`), hücre içi yerel koordinatı `cell_uv = 2.0 * fract(st_rot) - 1.0` olarak merkezler ve `sqrt(ink_amount)` yarıçapını `length(cell_uv)` SDF uzaklığıyla karşılaştırır. Kenar yumuşatması için `stackgl/glsl-aastep` (`dFdx`/`dFdy` tabanlı `afwidth = length(vec2(dFdx(v), dFdy(v))) * 0.70710678`) kullanır.
   - `Vidvox/ISF-Files/ISF/CMYK Halftone.fs` ve `RGB Halftone.fs` (MIT) aynı `15° / 45° / 0° / 75°` açı setini piksel grid merkezinde (`floor(gridFragLoc / gridSize) * gridSize + gridSize * 0.5`) örneklenen rengin kanal yoğunluğuyla sürer.
   - `Joy-less/MangaShader` (MIT) 4×4 düzenli nokta matrisi (`ivec2 pixel_pos % 4`) kullanır; ancak sabit piksel modülo yaklaşımı çözünürlükten bağımsız (`draft 0.5x` vs `final 1.0x`) değildir.
2. **Çoklu Örneklemeli Bloom / Deep Glow:**
   - `libretro/slang-shaders/blurs/shaders/dual_filter/` (`dual_filter.inc`, `downsample_bloom.slang`, `upsample.slang`, `bloom_blend.slang`, `tonemapping.inc` — CC0):
     - Doğrusal uzayda (`linearize`) eşikleme (`pow(rgb, vec3(threshold))` veya yumuşak eşik),
     - 5-tap Dual Kawase downsample: $\frac{1}{8}(4 C + TL + TR + BL + BR)$,
     - 8-tap Dual Kawase upsample: $\frac{1}{12}(T + B + L + R + 2(TL + TR + BL + BR))$,
     - Çoklu ölçek toplamı (`Scale1..Scale6` Gauss ağırlıkları) ve `blend_screen(a, b) = 1.0 - (1.0 - a) * (1.0 - b)` birleşimi.
   - `Vidvox/ISF-Files/ISF/Bloom.fs` (MIT) 1/2, 1/4, 1/8 küçültme + çift yönlü Gaussian bulanıklaştırma sonrası `max(mix(orig, blurred * 1.2, intensity), orig)` harmanlaması yapar.

---

## Çıkarımlar ve Öneriler (FrameShift WGSL Uyarlama Şartnamesi)

Aşağıdaki çıkarımlar incelenen birincil kaynakların FrameShift'in kilitli `effect-shader-api.md` ve `render-graph.md` sözleşmelerine sentezlenmiş WGSL tasarım şartnamesidir.

### Ortak FrameShift Mimari Kuralları (Tüm 4 Shader İçin)
- **Bağlama Düzeni (`Effect1D`):** Tüm 4 post-FX düğümü `@group(0)` içinde:
  - `@binding(0) var t_source: texture_2d<f32>;`
  - `@binding(1) var s_linear: sampler;`
  - `@binding(2) var<uniform> params: <Shader>Uniform;`
- **Vertex Girişi:** `transform.wgsl` ve `color_basic.wgsl` ile özdeş harici vertex buffer'sız fullscreen triangle `@vertex fn vs_main(@builtin(vertex_index) vid: u32) -> VertexOutput`.
- **Çalışma Uzayı & Premultiplied Alpha:** Giriş ve çıkış `Rgba16Float`, `LinearRec709`, premultiplied alpha. Luma, eşikleme, tersleme veya kanal kırılması gibi doğrusal olmayan işlemlerden önce `if (src.a <= 1e-5)` korumasıyla `rgb = max(src.rgb / src.a, vec3<f32>(0.0))` un-premultiply edilir; çıkışta `vec4<f32>(rgb_out * alpha_out, alpha_out)` geri çarpılır.
- **Çözünürlük Bağımsızlığı (`draft` vs `final`):** Tüm yarıçap, nokta frekansı ve bulanıklık offset'leri piksel sabiti yerine normalize tuval birimi veya `aspect` + `dpdx`/`dpdy` tabanlı tanımlanır (`Joy-less/MangaShader` gibi `ivec2 % 4` piksel-kilitli yöntemler yerine `glsl-halftone` normalize UV + `dpdx/dpdy` anti-aliasing benimsenir).
- **Seek-Independence & `SafeExpr`:** `Shockwave Pulse.fs`'deki `PERSISTENT` bellek tamponu **kullanılmaz**. Tüm zamana bağlı hareketler `frame_ctx` içindeki `t_sec` / `progress` / `event_delta_sec` alanları ve `SafeExpr` (`audio_rms`, `beat_phase`, `event_delta_sec`) üzerinden saf fonksiyon olarak sürülür.
- **Naga & TDR Güvenliği:** Tüm döngüler derleme zamanı sabit üst sınırına (`MAX_LOOP <= 128`) sahiptir; bölme işlemlerinde `max(..., 1e-6)` ve `pow(max(x, 0.0), exp)` koruması uygulanır.

---

### 1. `fx_motion_tile_shake.wgsl` — Motion Tile & Sönümlü Kamera Sarsıntısı

- **Kaynak Sentezi:** `Quad Tile.fs` + `Mirror Edge.fs` (ayna/tekrar UV katlama) + `Shake.fs` + `godotshaders/screen-shake` (mikro-zoom + harmonik ofset).
- **Matematiksel Çekirdek:**
  1. **Sönümlü Harmonik Darbe Sarsıntısı (Damped Harmonic Shake):** `event_delta_sec` ($\Delta t$) girişi $\ge 0$ iken sönüm zarfı:
     $$E(\Delta t) = \exp(-\zeta \cdot \omega_n \cdot \Delta t) \cdot \text{step}(0.0, \Delta t), \quad \omega_n = 2\pi f_{Hz}$$
     Ofset ve dönüş:
     $$\Delta x = A_x \cdot E(\Delta t) \cdot \sin(\omega_n \Delta t + \phi), \quad \Delta y = A_y \cdot E(\Delta t) \cdot \cos(1.13\,\omega_n \Delta t + \phi + 0.7 \cdot \text{seed})$$
     $$\theta = A_\theta \cdot E(\Delta t) \cdot \sin(0.87\,\omega_n \Delta t + 1.9 \cdot \text{seed})$$
     *(Not: Çarpanlardaki `1.13` ve `0.87` gibi asal/irrasyonel oranlar X, Y ve dönüş eksenlerinin Lissajous döngüsünde tekdüze tekrarını önlemek için deterministik faz ayrımı sağlar.)*
  2. **İzotropik Pivot Dönüşü + Zoom Punch:** `transform.wgsl` ile aynı `aspect` düzeltmeli ters dönüş + `zoom = max(1.0 + zoom_punch * E(dt), 1e-4)` ölçeklemesi.
  3. **Tile / Mirror UV Eşlemesi:**
     - `tile_mode == 0u` (Clamp/Zero): `[0, 1]` dışı `vec4<f32>(0.0)`.
     - `tile_mode == 1u` (Repeat): `fract(uv)`.
     - `tile_mode == 2u` (MirrorTile — AMV standardı):
       $$\text{mirror}(u) = 1.0 - \left|\operatorname{fract}(u \cdot 0.5) \cdot 2.0 - 1.0\right|$$
       *(İsteğe bağlı `mirror_x` / `mirror_y` eksen bazlı seçilebilir.)*
  4. **Premultiplied Alpha & Örnekleme:** Doğrudan premultiplied uzayda `textureSampleLevel` (veya `sample_filter == 1u` ise `transform.wgsl`'deki `4x4` Catmull-Rom sabit döngüsü `4x4 = 16` iterasyon) çalıştırılır.

- **16-Byte Hizalı WGSL Uniform Yapısı (`64 byte`):**
```wgsl
struct MotionTileShakeUniform {
    // x = t_sec, y = event_delta_sec (>= 0 when impact active), z = aspect (w/h), w = resolution_scale
    frame_ctx: vec4<f32>,
    // x = amplitude_x (norm UV), y = amplitude_y (norm UV), z = rot_amplitude_rad, w = zoom_punch
    shake_params: vec4<f32>,
    // x = frequency_hz, y = damping_zeta, z = phase_offset_rad, w = seed
    damping_freq: vec4<f32>,
    // x = tile_mode (0u=ZeroBorder, 1u=Repeat, 2u=MirrorTile),
    // y = mirror_x (0u/1u), z = mirror_y (0u/1u), w = sample_filter (0u=Bilinear, 1u=CatmullRom)
    tile_flags: vec4<u32>,
}
```

---

### 2. `fx_anime_impact_frame.wgsl` — Anime Impact Frame & Radyal Hız Çizgileri

- **Kaynak Sentezi:** `Edges.fs` + `Edge Trace.fs` (3×3 Sobel) + `RGB Invert.fs` (negatif flaş) + `godotshaders/speed-lines` (polar koordinat radyal çizgileri).
- **Matematiksel Çekirdek:**
  1. **Un-premultiply & Rec.709 Luma:** `rgb = max(src.rgb / src.a, vec3<f32>(0.0))`, $Y = \operatorname{dot}(rgb, (0.2126, 0.7152, 0.0722))$.
  2. **3×3 Sobel Kontur Tespiti:** `texel = 1.0 / max(vec2<f32>(textureDimensions(t_source)), vec2<f32>(1.0))` *(veya çözünürlük bağımsız kenar kalınlığı için `resolution_scale` ile ölçeklenen adım)* üzerinden 8 komşunun Rec.709 luması örneklenir (`Edges.fs` `(r+g+b)/3` yerine Rec.709); $M = \sqrt{G_x^2 + G_y^2}$ hesaplanır ve `edge_mask = smoothstep(edge_thresh, edge_thresh + 0.1, M * edge_strength)` elde edilir.
  3. **Sert İkili / Negatif Impact Çerçevesi:**
     - `bw_mask = smoothstep(luma_thresh - luma_soft, luma_thresh + luma_soft, Y)`
     - `impact_rgb = mix(dark_color.rgb, light_color.rgb, bw_mask)`
     - `enable_sobel == 1u` ise kenarlar zıt renge (ör. beyaz zemin üstü siyah mürekkep ya da tersi) vurulur.
     - `invert_amount > 0.0` ise `mix(impact_rgb, max(vec3<f32>(1.0) - impact_rgb, vec3<f32>(0.0)), invert_amount)` uygulanır (HDR değerler negatiflenmeden önce `[0, 1]`'e tonemap/clamp edilir).
  4. **Deterministik Polar Hız Çizgileri (`NoiseTexture2D` Bağımsız):**
     - Aspect-düzeltmeli polar vektör: `d = vec2<f32>((in.uv.x - cx) * aspect, in.uv.y - cy)`, `r = length(d)`, $\theta = \operatorname{atan2}(d.y, d.x) / (2\pi) + 0.5$.
     - Açısal hücre ve deterministik hash:
       `slot = floor(fract(theta + t_sec * line_speed * 0.1) * line_count)`
       `h = fract(sin(slot * 127.1 + f32(seed_u32) * 311.7) * 43758.5453)`
     - Üçgen alt-hücre profili ile iğne ucu sivrilmesi: `local_a = fract(...) * 2.0 - 1.0`, `line_hit = step(1.0 - line_density, h) * smoothstep(1.0 - h * 0.4, 0.0, abs(local_a))`.
     - Merkez koruma maskesi: `radial_mask = smoothstep(inner_radius, outer_radius, r)`.
     - Hız çizgileri `impact_rgb` üzerine zıt ink/highlight rengiyle bindirilir, ardından `overall_mix` ile orijinal `rgb` ile karıştırılır ve `src.a` ile geri çarpılır.

- **16-Byte Hizalı WGSL Uniform Yapısı (`112 byte`):**
```wgsl
struct AnimeImpactFrameUniform {
    // x = t_sec, y = progress, z = aspect, w = resolution_scale
    frame_ctx: vec4<f32>,
    // x = luma_threshold, y = luma_softness, z = edge_strength, w = edge_threshold
    threshold_edge: vec4<f32>,
    // x = center_x, y = center_y, z = line_count (e.g. 96.0), w = line_density [0..1]
    speedline_geom: vec4<f32>,
    // x = inner_radius, y = outer_radius, z = line_speed, w = invert_amount [0..1]
    speedline_mask: vec4<f32>,
    // xyz = dark_color (LinearRec709), w = reserved
    dark_color: vec4<f32>,
    // xyz = light_color (LinearRec709), w = overall_mix [0..1]
    light_color: vec4<f32>,
    // x = enable_speedlines, y = enable_sobel, z = impact_mode (0u=BW, 1u=InvertOriginal, 2u=PosterizeEdge), w = seed_u32
    flags: vec4<u32>,
}
```

---

### 3. `fx_radial_shockwave.wgsl` — Radyal Şok Dalgası & Kromatik Zoom Blur

- **Kaynak Sentezi:** `Shockwave.fs` + `godotshaders/positioned-shockwave` (aspect-düzeltmeli halka kırılması) + `evanw/glfx.js zoomblur.js` (parabolik ağırlıklı premultiplied radyal blur) + `Chroma Zoom.fs` & `Chromatic Aberration.fs` (RGB dispersiyon).
- **Matematiksel Çekirdek:**
  1. **Aspect-Düzeltmeli Halka Kırılması:**
     - `delta = in.uv - center`, `delta_iso = vec2<f32>(delta.x * aspect, delta.y)`, `r = length(delta_iso)`.
     - Dairesel halka maskesi (`Positioned Shockwave` çift `smoothstep` + `Shockwave.fs` güç profili):
       $$\text{half\_w} = \max(\text{wave\_thickness} \cdot 0.5, 10^{-4}), \quad d = r - \text{wave\_radius}$$
       $$\text{ring\_mask} = \max(1.0 - \left|\frac{d}{\text{half\_w}}\right|, 0.0)^{\text{wave\_sharpness}}$$
     - Kırılma yer değiştirmesi: `dir_uv = delta / max(r, 1e-5)` *(not: aspect geri ölçeklemesi ile UV uzayında dairesel itme `vec2<f32>(delta_iso.x / aspect, delta_iso.y)`)*, `refracted_uv = in.uv - dir_uv * (refract_strength * ring_mask * sign(d))`.
  2. **Sabit Üst Sınırlı Kromatik Radyal Zoom Blur:**
     - Naga TDR koruması için `const MAX_ZOOM_SAMPLES: u32 = 32u;`, aktif örnek sayısı `n = clamp(flags.x, 1u, MAX_ZOOM_SAMPLES)`.
     - Dışa doğru vektör `to_center = center - refracted_uv`.
     - Deterministik alt-örnek kaydırması (opsiyonel interleaved gradient noise, `t_sec` bağımsız sabit koordinat jitter'ı `fract(52.9829189 * fract(dot(in.position.xy, vec2<f32>(0.06711056, 0.00583715))))` — seek-independent!).
     - Döngü `for (var i: u32 = 0u; i < n; i = i + 1u)`:
       `percent = (f32(i) + jitter) / f32(n)`
       `weight = 4.0 * (percent - percent * percent)` *(veya `n == 1u` için `weight = 1.0`, `percent = 0.0`)*
       Kromatik kaydırma: `ca_scale = chromatic_aberration * pow(min(r * 2.0, 2.0), ca_falloff)`.
       - `uv_r = refracted_uv + to_center * (percent * zoom_blur_strength + ca_scale * (1.0 + ca_balance))`
       - `uv_g = refracted_uv + to_center * (percent * zoom_blur_strength)`
       - `uv_b = refracted_uv + to_center * (percent * zoom_blur_strength - ca_scale * (1.0 - ca_balance))`
       Her kanal ayrı örneklenip premultiplied uzayda `acc += vec4<f32>(s_r.r, s_g.g, s_b.b, s_g.a) * weight` olarak toplanır; sonunda `acc / max(total_w, 1e-5)` döndürülür. (`border_mode == 1u` ise örnek UV'leri `mirror_tile` ile katlanır, `0u` ise `clamp` veya şeffaf sınır uygulanır.)

- **16-Byte Hizalı WGSL Uniform Yapısı (`80 byte`):**
```wgsl
struct RadialShockwaveUniform {
    // x = t_sec, y = progress, z = aspect, w = resolution_scale
    frame_ctx: vec4<f32>,
    // x = center_x, y = center_y, z = wave_radius (iso units), w = wave_thickness
    wave_geom: vec4<f32>,
    // x = refract_strength, y = wave_sharpness (>= 0.5), z = zoom_blur_strength, w = chromatic_aberration
    wave_refract: vec4<f32>,
    // x = ca_falloff (default 2.0), y = ca_balance [-1..1], z = ring_highlight_boost, w = _pad
    ca_tuning: vec4<f32>,
    // x = sample_count (1u..=32u), y = wave_enabled (0u/1u), z = border_mode (0u=Clamp, 1u=MirrorTile), w = _pad
    flags: vec4<u32>,
}
```

---

### 4. `fx_halftone_glow.wgsl` — Manga Halftone / Screentone & Deep Anime Glow

- **Kaynak Sentezi:** `stackgl/glsl-halftone` + `glsl-aastep` + `CMYK Halftone.fs` (döndürülmüş 2D SDF nokta ızgarası) + `libretro/slang-shaders/blurs/shaders/dual_filter/` (`dual_filter.inc`, `tonemapping.inc` — doğrusal eşikleme ve `blend_screen`).
- **Tek-Pass vs Çok-Pass Mimari Değerlendirmesi:**
  - `effect-shader-api.md` uyarınca tek girişli bir `ShaderEffect` düğümü (`Effect1D`) tek WGSL geçişi olarak çalışır. Bu nedenle `fx_halftone_glow.wgsl` içinde **tek-pass 13-tap 2-halkalı yıldız/diyagonal HDR highlight glow** (1 merkez + 4 yakın eksen + 4 orta diyagonal + 4 uzak eksen örneği, sabit 13 tap — TDR güvenli) + **2D SDF Halftone/Screentone** birleştirilir.
  - *Çok geniş yarıçaplı (`> 30px`) sinematik "Deep Glow" gereksinimi doğduğunda*, `frameshift-render` grafiği içinde `TexturePool` kullanan `dual_filter.inc` (CC0) tabanlı 3–4 kademeli `downsample` + `upsample` alt-grafiği (ayrı multi-pass node) ikinci aşama optimizasyonu olarak önerilir.
- **Matematiksel Çekirdek:**
  1. **HDR Parıltı Çıkarımı (Linear Rec.709):**
     - Sabit 13-tap çekirdeğin her örneği un-premultiply edilir (veya doğrudan premultiplied `Rgba16Float` toplanıp luma soft-knee eşiği uygulanır; şeffaf kenarlarda renk sızmasını önlemek için premultiplied `s.rgb` üzerinden $L = \operatorname{dot}(s.rgb, LUMA\_REC709)$ hesaplanır):
       $$\text{soft} = \operatorname{clamp}(L - T + K, 0.0, 2K), \quad w_{knee} = \frac{\text{soft}^2}{4K + 10^{-5}}, \quad \text{contrib} = \max(L - T, w_{knee}) / \max(L, 10^{-5})$$
       *(Bu yumuşak diz/soft-knee fonksiyonu `Rgba16Float` içindeki `> 1.0` HDR anime parıltılarını sert bantlaşma olmadan toplar.)*
     - Toplanan `glow_rgb` tint ile çarpılıp `glow_intensity` ile ölçeklenir ve `glow_mode` (`1u = Screen`, `2u = Additive`) ile ana renge eklenir (`Screen` modunda HDR değerlerin bozulmaması için `1 - (1 - min(a,1))*(1 - min(b,1))` veya `a + b * (1 - clamp(a,0,1))` yumuşak ekran harmanı önerilir).
  2. **Döndürülmüş 2D SDF Halftone / Screentone:**
     - Aspect-düzeltmeli normalize koordinat `st = vec2<f32>(in.uv.x * aspect, in.uv.y)` seçilen `grid_angle_rad` ile döndürülür (`mat2x2<f32>(c, -s, s, c)`).
     - Hücre koordinatı: `grid_uv = st * dot_frequency`, `cell_uv = 2.0 * fract(grid_uv) - 1.0`, uzaklık `d = length(cell_uv)`.
     - `aa_enabled == 1u` ise WGSL türevleriyle anti-aliasing (`glsl-aastep` portu):
       `af = length(vec2<f32>(dpdx(d), dpdy(d))) * 0.70710678` + kullanıcı `dot_softness`.
     - **Modlar:**
       - `1u` (Mono Manga Screentone): Rec.709 luma $Y$ ton sıkıştırmasından (`pow(clamp(Y, 0.0, 1.0), luma_gamma)`) geçirilir; mürekkep yoğunluğu `ink = clamp(1.0 - Y_gamma + dot_gain, 0.0, 1.0)`, nokta yarıçapı `r_dot = sqrt(ink)`. `dot_mask = 1.0 - smoothstep(r_dot - af, r_dot + af, d)`. Sonuç `mix(vec3(paper_brightness), vec3(ink_darkness), dot_mask)`.
       - `2u` (RGB Halftone) / `3u` (CMYK Halftone): `CMYK Halftone.fs` / `glsl-halftone` ile aynı `15° / 45° / 0° / 75°` sabit açı dizisi (`const ANGLES = array<f32, 4>(0.261799, 0.785398, 0.0, 1.308997)`) üzerinde sabit 4 iterasyonlu döngüyle her kanal için ayrı döndürülmüş SDF noktası hesaplanır.
  3. **Premultiplied Alpha Çıkışı:** `vec4<f32>(rgb_final * src.a, src.a)`.

- **16-Byte Hizalı WGSL Uniform Yapısı (`96 byte`):**
```wgsl
struct HalftoneGlowUniform {
    // x = t_sec, y = progress, z = aspect, w = resolution_scale
    frame_ctx: vec4<f32>,
    // x = dot_frequency (e.g. 60.0), y = grid_angle_rad (default ~0.7854), z = dot_softness, w = halftone_mix [0..1]
    halftone_geom: vec4<f32>,
    // x = paper_brightness, y = ink_darkness, z = dot_gain, w = luma_gamma
    halftone_tone: vec4<f32>,
    // x = glow_threshold (linear, e.g. 0.65), y = glow_knee (e.g. 0.25), z = glow_intensity, w = glow_radius_norm
    glow_params: vec4<f32>,
    // xyz = glow_tint (LinearRec709), w = _pad
    glow_tint: vec4<f32>,
    // x = halftone_mode (0u=Off, 1u=MonoManga, 2u=RGB, 3u=CMYK),
    // y = glow_mode (0u=Off, 1u=SoftScreen, 2u=Additive), z = aa_enabled (0u/1u), w = _pad
    flags: vec4<u32>,
}
```

---

## Doğrulanmamış Noktalar

- **Naga & `wgpu` Derleme Doğrulaması:** Yukarıdaki 4 `@group(0)` uniform struct'ı ve önerilen WGSL çekirdek formülleri henüz `crates/frameshift-render/shaders/` altına `.wgsl` dosyası olarak yazılmamış ve `naga::front::wgsl::parse_str` + `naga::valid::Validator` testlerinden geçirilmemiştir (**doğrulanmadı**).
- **`dpdx` / `dpdy` Türev Davranışı (`draft` vs `final`):** `fx_halftone_glow.wgsl` içindeki `glsl-aastep` türev anti-aliasing'inin `0.5x` (`draft`) ve `1.0x` (`final`) render profillerinde harelenme (moiré) yaratmadan görsel eşdeğerlik sağlayıp sağlamadığı gerçek anime görüntüleri üzerinde test edilmemiştir (**doğrulanmadı**).
- **Tek-Pass 13-Tap Glow vs Çok-Pass Dual-Kawase:** Tek-pass 13-tap yıldız/diyagonal bloom'un yüksek çözünürlükte (`1080p`/`4K`) geniş yarıçaplı ("Deep Glow") AMV sahnelerinde bantlaşma (ringing) yapıp yapmayacağı benchmark veya görsel inceleme ile ölçülmemiştir (**doğrulanmadı**).
- **Kare Hızı & GPU Süresi:** Bu 4 efektin `MotionBlurShutter` (`sub_samples = 8..16`) ile birleştiğinde hedef GPU üzerindeki milisaniye maliyeti ölçülmemiştir (**doğrulanmadı**).
