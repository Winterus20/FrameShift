---
title: Açık Kaynak Shader Geçişleri (MIT/BSD/CC0) ve FrameShift WGSL Uyarlama Şartnamesi
date: 2026-11-21
status: active
tags: [shaders, render-graph]
---

# Açık Kaynak Shader Geçişleri (MIT/BSD/CC0) ve FrameShift WGSL Uyarlama Şartnamesi

## Soru / Amaç

FrameShift'in anime edit / AMV ve motion-graphics odaklı yerel GPU kompozisyon motoruna (`frameshift-render`) entegre edilebilecek, izin verici açık kaynak lisanslı (**MIT**, **ISC**, **BSD**, **CC0**) hazır GPU geçiş shader'ları hangileridir; bu shader'ların birincil kaynak kodlarındaki matematiksel çekirdekleri nasıldır ve FrameShift'in `Rgba16Float` + `LinearRec709` + **Premultiplied Alpha** + `@group(0)` + 16-byte hizalı uniform + Naga TDR döngü sınırı sözleşmesine (`effect-shader-api.md`) nasıl uyarlanmalıdır?

---

## İncelenen Birincil Kaynaklar

Tüm harici kaynaklar `2026-11-21` tarihinde ham kaynak dosya (`curl.exe -sL`) üzerinden doğrudan okunarak lisans ve kod doğrulaması yapılmıştır:

- **`gl-transitions` Ana Lisansı:** [LICENSE](https://raw.githubusercontent.com/gl-transitions/gl-transitions/master/LICENSE) — erişim/inceleme tarihi: `2026-11-21` (MIT License; bireysel dosya başlıklarında aksi belirtilmedikçe tüm geçişler MIT kapsamındadır).
- **`gl-transitions` Spesifikasyonu (v1):** [README.md](https://raw.githubusercontent.com/gl-transitions/gl-transitions/master/README.md) — erişim/inceleme tarihi: `2026-11-21`.
- **`gl-transitions` Rust Crate Manifestosu ve Kullanım Rehberi:** [crates/gl-transitions/Cargo.toml](https://raw.githubusercontent.com/gl-transitions/gl-transitions/master/crates/gl-transitions/Cargo.toml) (`v2.0.3`, `license = "MIT AND BSD-2-Clause AND BSD-3-Clause"`) ve [crates/gl-transitions/README.md](https://raw.githubusercontent.com/gl-transitions/gl-transitions/master/crates/gl-transitions/README.md) — erişim/inceleme tarihi: `2026-11-21`.
- **`gl-transitions` SPIR-V → Naga WGSL Dönüşüm Boru Hattı:** [scripts/targets/build-wgsl-msl.mjs](https://raw.githubusercontent.com/gl-transitions/gl-transitions/master/scripts/targets/build-wgsl-msl.mjs) ve [scripts/targets/spirv.mjs](https://raw.githubusercontent.com/gl-transitions/gl-transitions/master/scripts/targets/spirv.mjs) — erişim/inceleme tarihi: `2026-11-21`.
- **Doğrudan İncelenen Ham GLSL Geçiş Kaynakları (`gl-transitions/transitions/*.glsl`):**
  - [CrossZoom.glsl](https://raw.githubusercontent.com/gl-transitions/gl-transitions/master/transitions/CrossZoom.glsl) — Yazar: `rectalogic` (port: `gre`), Lisans: `MIT`, erişim: `2026-11-21`.
  - [DirectionalWarp.glsl](https://raw.githubusercontent.com/gl-transitions/gl-transitions/master/transitions/DirectionalWarp.glsl) — Yazar: `pschroen`, Lisans: `MIT`, erişim: `2026-11-21`.
  - [GlitchDisplace.glsl](https://raw.githubusercontent.com/gl-transitions/gl-transitions/master/transitions/GlitchDisplace.glsl) — Yazar: `Matt DesLauriers`, Lisans: `MIT`, erişim: `2026-11-21`.
  - [Swirl.glsl](https://raw.githubusercontent.com/gl-transitions/gl-transitions/master/transitions/Swirl.glsl) — Yazar: `Sergey Kosarevsky` (port: `gre`), Lisans: `MIT`, erişim: `2026-11-21`.
  - [DreamyZoom.glsl](https://raw.githubusercontent.com/gl-transitions/gl-transitions/master/transitions/DreamyZoom.glsl) — Yazar: `Zeh Fernando`, Lisans: `MIT`, erişim: `2026-11-21`.
  - [Morph.glsl](https://raw.githubusercontent.com/gl-transitions/gl-transitions/master/transitions/Morph.glsl) — Yazar: `paniq`, Lisans: `MIT`, erişim: `2026-11-21`.
- **Ek Anime/AMV Odaklı Ham GLSL Geçişleri (Karşılaştırma İçin İncelendi):**
  - [CameraShake.glsl](https://raw.githubusercontent.com/gl-transitions/gl-transitions/master/transitions/CameraShake.glsl) (`gre`, `MIT`), [WarpSpeed.glsl](https://raw.githubusercontent.com/gl-transitions/gl-transitions/master/transitions/WarpSpeed.glsl) (`gre`, `MIT`), [GlitchMemories.glsl](https://raw.githubusercontent.com/gl-transitions/gl-transitions/master/transitions/GlitchMemories.glsl) (`Gunnar Roth`, `MIT`), [FlashFade.glsl](https://raw.githubusercontent.com/gl-transitions/gl-transitions/master/transitions/FlashFade.glsl) (`gre`, `MIT`), [CrossWarp.glsl](https://raw.githubusercontent.com/gl-transitions/gl-transitions/master/transitions/CrossWarp.glsl) (`Eke Péter`, `MIT`), [DirectionalScaled.glsl](https://raw.githubusercontent.com/gl-transitions/gl-transitions/master/transitions/DirectionalScaled.glsl) (`Thibaut Foussard`, `MIT`), [Kaleidoscope.glsl](https://raw.githubusercontent.com/gl-transitions/gl-transitions/master/transitions/Kaleidoscope.glsl) (`nwoeanhinnogaehr`, `MIT`) — erişim: `2026-11-21`.
- **FrameShift İç Referans Sözleşmeleri ve Shader Örneği:**
  - [effect-shader-api.md](../specs/effect-shader-api.md) (`status: locked`)
  - [render-graph.md](../specs/render-graph.md) (`status: locked`)
  - [transition_whip_chromatic.wgsl](../../crates/frameshift-render/shaders/transition_whip_chromatic.wgsl)
  - [uniforms.rs](../../crates/frameshift-render/src/uniforms.rs) (`WhipTransitionUniform`, satır 149–177)

---

## Doğrudan Gözlemler (Kanıtlı)

### 1. Lisans ve Ekosistem Durumu

- `gl-transitions` deposunun kök [LICENSE](https://raw.githubusercontent.com/gl-transitions/gl-transitions/master/LICENSE) dosyası **MIT** lisanslıdır ve `transitions/` altındaki bir dosyada ayrı lisans belirtilmemişse MIT kapsamına girdiğini beyan eder.
- [README.md](https://raw.githubusercontent.com/gl-transitions/gl-transitions/master/README.md) ve [crates/gl-transitions/Cargo.toml](https://raw.githubusercontent.com/gl-transitions/gl-transitions/master/crates/gl-transitions/Cargo.toml) (`version = "2.0.3"`) koleksiyonun büyük çoğunluğunun **MIT**, yalnızca iki geçişin **BSD-2-Clause / BSD-3-Clause** altında olduğunu belirtir. Bu raporda incelenen tüm geçişlerin dosya başlıkları bizzat `// License: MIT` olarak doğrulanmıştır.
- `gl-transitions` CI boru hattı (`glslangValidator` → SPIR-V → `naga` `30.0.1` → WGSL) halihazırda Naga uyumlu WGSL üretmektedir; ancak ham `gl-transitions` WGSL çıktısı **FrameShift'e doğrudan drop-in olarak bağlanamaz** (aşağıdaki Bölüm 3).

### 2. İncelenen Geçişlerin Matematiksel Çekirdekleri ve Parametreleri

#### A. `CrossZoom.glsl` (Zoom Blur + Dissolve)
- **Parametre:** `uniform float strength; // = 0.4` (`@range(0, 1)`).
- **Eğriler (Easing):**
  - Odak merkezi X ekseninde doğrusal kayar: `center = vec2(Linear_ease(0.25, 0.5, 1.0, progress), 0.5)` (`0.25 → 0.75`).
  - İki klip arası karışım `Exponential_easeInOut(0.0, 1.0, 1.0, progress)` ile keskinleşir.
  - Blur şiddeti simetrik sinüzoidal zarf izler: `Sinusoidal_easeInOut(0.0, strength, 0.5, progress)` (`0 → strength → 0`, tepe `progress = 0.5`).
- **Örnekleme ve Döngü:**
  - Sabit üst sınırlı 41 iterasyonlu döngü (`for (float t = 0.0; t <= 40.0; t++)`).
  - Bantlaşmayı gizlemek için ekran-uzayı hash gürültüsü `offset = rand(uv)` eklenir: `percent = (t + offset) / 40.0`.
  - Parabolik ağırlık penceresi: `weight = 4.0 * (percent - percent * percent)` (uçlarda 0, ortada 1.0).
  - Her adımda `texCoord + (center - texCoord) * percent * strength` koordinatından `mix(from, to, dissolve)` örneklenir.

#### B. `DirectionalWarp.glsl` (Yönlü Köşegen Süpürme / Sıkıştırma-Genişleme)
- **Parametreler:** `uniform float smoothness; // = 0.1`, `uniform vec2 direction; // = vec2(-1.0, 1.0)`.
- **Matematiksel Çekirdek:**
  - Yön vektörü birimleştirilir ve L1 normuna bölünür: `v = normalize(direction); v /= abs(v.x) + abs(v.y);` (böylece `[0, 1]` karesi içinde tarama projeksiyonu tam kapsanır).
  - Merkez projeksiyonu `d = dot(v, vec2(0.5))`.
  - Eşik maskesi: `m = 1.0 - smoothstep(-smoothness, 0.0, v.x * uv.x + v.y * uv.y - (d - 0.5 + progress * (1.0 + smoothness)))`.
  - Çıktı: `from` klibi `(uv - 0.5) * (1.0 - m) + 0.5` ile merkezden küçülür/çekilirken `to` klibi `(uv - 0.5) * m + 0.5` ile ölçeklenip yerine oturur ve `m` ile karışır.
  - **Sınır durumu gözlemi:** `direction == vec2(0.0, 0.0)` verilirse `normalize(direction)` GLSL/WGSL'de `NaN` üretir; mutlaka koruma gerektirir.

#### C. `GlitchDisplace.glsl` (Voronoi Blok Bozulması + Parlaklık Flaşlı Glitch)
- **Parametreler:** Yerleşik sabitler (`dotDepth = 0.33`, `textureDepth = 0.7 / 0.5`); dış `uniform` parametresi tanımlanmamış.
- **Matematiksel Çekirdek:**
  - 3x3 komşuluklu sabit döngülü (`j = -1..=1`, `i = -1..=1`; 9 adım) 2D Voronoi hücresel gürültü (`voronoi(x)`).
  - `displace()` üç ölçekte (`0.003`, `0.2`, `1.0`) Voronoi çağırır (not: kaynak kodda `b`, `g`, `r` değişkenleri hesaplanır ancak doğrudan renk kanalı yerine `tex` piksel değeri üzerinden `dis = tex * dotDepth + 1.0 - tex * textureDepth` yer değiştirme vektörü üretilir).
  - İki asimetrik üssel eğri: `ease1(t)` (exponential ease-in-out) ve `ease2(t)` (exponential ease-out).
  - `dColor2` ile `dColor1` arasındaki minimum RGB'nin Rec.601 luma değeri (`dot(..., vec3(0.299, 0.587, 0.114))`) `2.0` ile çarpılarak parlayan gri glitch katmanı oluşturulur.
  - **Kritik WGSL / Naga uyumluluk bulgusu:** Kaynak kodun 193. satırında `smoothstep(1.0, 0.5, progress)` kullanılmıştır (`low = 1.0 > high = 0.5`). WGSL spesifikasyonunda `edge0 >= edge1` durumu tanımsız davranış (UB) üretir; WGSL uyarlamasında kesinlikle `1.0 - smoothstep(0.5, 1.0, progress)` biçimine çevrilmelidir.

#### D. `Swirl.glsl` (Aspect-Düzeltmeli Burgaç + Açısal Motion Blur)
- **Parametreler:** `strength = 1.0` (`@range(-3, 3)`), `radius = 0.9` (`@range(0.1, 2)`), `center = vec2(0.5, 0.5)`.
- **Matematiksel Çekirdek:**
  - İki uçta sıfır türevli zarf: `amount = sin^2(PI * progress)`, açısal hız `speed = PI * sin(TAU * progress)`, çapraz geçiş `reveal = smoothstep(0.3, 0.7, progress)`.
  - En-boy oranı düzeltmesi: `d = (uv - center) * vec2(ratio, 1.0)`.
  - Kuadratik radyal sönümleme: `x = clamp(length(d) / max(radius, 1e-3), 0.0, 1.0)`, `falloff = (1.0 - x)^2`.
  - Büküm açısı `angle = strength * TAU * falloff * amount`, merkez yakınsaması `d *= 1.0 - 0.12 * amount * falloff`.
  - Sabit `TAPS = 12` döngüde çadır ağırlıklı (`1.0 - abs(k)`) açısal motion blur (`spread = clamp(twist * speed * 0.05, -1.2, 1.2)`) ve `mirrored()` UV katlaması.

#### E. `DreamyZoom.glsl` (Dönen Zoom + Orta Nokta Beyaz Flaş)
- **Parametreler:** `rotation = 6.0` (derece olarak belgelenmiş), `scale = 1.2`.
- **Matematiksel Çekirdek:**
  - `progress = 0.5` noktasında iki faza ayrılır: ilk yarıda `from` klibi `1.0 → scale` büyür ve `0 → +rotation` döner; ikinci yarıda `to` klibi `scale → 1.0` küçülür ve `-rotation → 0` döner.
  - Polar dönüşüm: `angle = atan(p.y, p.x) + angleOffset`, `dist = distance(vec2(0.0), p)`.
  - Orta noktada (`0.5`) tepe yapan katkısal beyaz flaş: `c.rgb + mix(0.0, 1.0, phase)` (ikinci yarıda `1.0 → 0.0`).
  - **Önemli bulgular:**
    1. Üst kaynakta `#define DEG2RAD 0.03926990816987241548078304229099` değeri `PI / 180.0` (`0.01745329...`) yerine `PI / 80.0` olarak tanımlıdır.
    2. `c.rgb + flash` işlemi şeffaf piksel alfa değerini gözetmez (`straight` alfa varsayımı); FrameShift premultiplied uzayında `c.rgb + vec3<f32>(flash) * max(c.a, 0.0)` uygulanmalıdır.
    3. WGSL'de `atan(y, x)` yerine `atan2(y, x)` kullanılır ve `(0.0, 0.0)` noktasında sıfır koruması gerekir.

#### F. `Morph.glsl` (Renk Kanallarına Dayalı Karşılıklı Morfolojik Kaydırma)
- **Parametre:** `uniform float strength; // = 0.1`.
- **Matematiksel Çekirdek:**
  - İki karenin renk bilgisinden 2D yer değiştirme vektörü üretir: `oa = ((ca.rg + ca.b) * 0.5) * 2.0 - 1.0`, `ob = ((cb.rg + cb.b) * 0.5) * 2.0 - 1.0`.
  - Ortalama yer değiştirme `oc = mix(oa, ob, 0.5) * strength`.
  - `from` ileri itilirken (`p + oc * progress`), `to` geriden toplanır (`p - oc * (1.0 - progress)`) ve `progress` ile karışır.

#### G. Ek Anime/AMV Odaklı `gl-transitions` Gözlemleri
- **`CameraShake.glsl` (MIT):** `strength = 0.5`, `jolts = 12.0`, `blur = 0.5`, `split = 0.5` parametreleriyle `TAPS = 10` sabit döngüde sarsıntı, dönme (`roll`), örtücü bulanıklığı ve R/G/B kanallarında zamansal gecikme (`lag`) üretir; **kendiliğinden premultiplied alfa akümülasyonu (`vec4(c.rgb * c.a, c.a)`) kullanır**.
- **`WarpSpeed.glsl` (MIT):** `TAPS = 12` radyal `smear` + 2 katmanlı prosedürel yıldız izi (`stars`) + merkez patlama flaşı (`glow`) üretir; AMV hız patlamaları için çok uygundur.
- **`GlitchMemories.glsl` (MIT):** 16x16 blok ızgarasında `progress` ile titreşen RGB kanal kaydırmalı hızlı glitch kesmesi sunar (döngüsüz, tek geçişte 3x2 doku örneklemesi).

### 3. `gl-transitions` Sözleşmesi ile FrameShift Motor Sözleşmesinin Farkları

| Özellik | `gl-transitions` (v1 / Üretilen WGSL) | FrameShift (`effect-shader-api.md` & `render-graph.md`) |
| :--- | :--- | :--- |
| **Dokular / Renk Uzayı** | `rgba8unorm` / sRGB, **Straight RGBA** | **`Rgba16Float`**, **`LinearRec709`**, **Premultiplied Alpha** |
| **UV Koordinat Başlangıcı** | Sol-alt `(0, 0)` → Sağ-üst `(1, 1)` (`spirv.mjs` wrapper'ı `uv.y`'yi ters çevirir) | **Sol-üst `(0, 0)` → Sağ-alt `(1, 1)`** (`transition_whip_chromatic.wgsl` satır 43) |
| **`@group(0)` Bağlama Düzeni** | `0: Uniforms`, `1: _sampler`, `2: _from`, `3: _to` | `0: tex_a`, `1: tex_b`, `2: s_linear`, `3: params: var<uniform>` |
| **Uniform Hizalama** | `std140` otomatik paketleme (`progress`, `ratio`, ardından ham alanlar) | Zorunlu **`vec4<f32>`** blokları, toplam boyut `size % 16 == 0`, Rust `#[repr(C)] Pod` eşleşmesi |
| **Örnekleme Fonksiyonu** | `textureSample` (veya wrap edilmiş `getFromColor`) | **`textureSampleLevel(tex, s_linear, uv, 0.0)`** (Naga/döngü güvenli, türev bağımsız) |
| **Sınır Dışı (OOB) UV** | Clamp-to-edge veya şeffaf siyah (implementere bağlı) | Geçiş karakterine göre `mirror_uv(uv)` (bkz. `transition_whip_chromatic.wgsl`) |
| **Döngü Sınırı** | Kısıtsız (bazı shader'larda float sayaçlı döngü) | Sabit tam sayı/statik sınır, `MAX_SHADER_LOOP_ITERATIONS = 128` |

---

## Çıkarımlar ve Öneriler (FrameShift WGSL Uyarlama Şartnamesi)

*Aşağıdaki tasarımlar, yukarıdaki birincil kaynak gözlemleri ile FrameShift'in kilitli `effect-shader-api.md` ve `transition_whip_chromatic.wgsl` mimarisine dayalı teknik uyarlama şartnameleridir.*

### 1. Ortak FrameShift WGSL Şablon Kuralları

`crates/frameshift-render/shaders/` altına eklenecek tüm yeni geçiş shader'ları aşağıdaki ortak iskeleti izlemelidir:

1. **Vertex Çıkışı ve Giriş Noktası:** `transition_whip_chromatic.wgsl` ile birebir aynı `@vertex fn vs_main(@builtin(vertex_index) vid: u32) -> VertexOutput` (tam ekran üçgeni, sol-üst `(0, 0)` → sağ-alt `(1, 1)` UV).
2. **Standart `@group(0)` Bağlamaları:**
   ```wgsl
   @group(0) @binding(0) var tex_a: texture_2d<f32>;
   @group(0) @binding(1) var tex_b: texture_2d<f32>;
   @group(0) @binding(2) var s_linear: sampler;
   @group(0) @binding(3) var<uniform> params: <StructName>Uniform;
   ```
3. **Premultiplied Alpha & Doğrusal Işık Kuralı (`Rgba16Float`, `LinearRec709`):**
   - `tex_a` ve `tex_b` zaten premultiplied `LinearRec709` gelir; doğrusal ağırlıklı `mix()` ve blur toplamları (`sum / total`) doğrudan premultiplied uzayda yapılır (siyah halo üretmez).
   - Herhangi bir ek parlaklık/flaş eklenirken (örn. `DreamyZoom`, `GlitchDisplace`) mutlaka `vec3<f32>(flash) * max(res.a, 0.0)` ile alfa ağırlıklı eklenir; şeffaf pikseller asla opaklaştırılmaz.
   - Luma hesabı gerekiyorsa Rec.601 (`0.299, 0.587, 0.114`) yerine **`LinearRec709` katsayıları `vec3<f32>(0.2126, 0.7152, 0.0722)`** kullanılmalıdır.
4. **Döngü ve Örnekleme Güvenliği:**
   - Tüm `textureSample` çağrıları `textureSampleLevel(tex, s_linear, mirror_uv(uv), 0.0)` biçiminde olmalıdır.
   - Tüm döngüler `const` tam sayı üst sınırlı (`<= 128`) yazılır.

---

### 2. Hedef Shader Şartnameleri

#### Şartname 1: `transition_cross_zoom.wgsl`
- **Kaynak:** `gl-transitions/transitions/CrossZoom.glsl` (MIT)
- **Hedef Dosya:** `crates/frameshift-render/shaders/transition_cross_zoom.wgsl`
- **Uniform Yapısı (`CrossZoomUniform` — 32 byte, `size % 16 == 0`):**

| Alan (`vec4<f32>`) | Bileşenler | Varsayılan | Açıklama |
| :--- | :--- | :--- | :--- |
| `motion` | `x`: `progress` `[0..1]`, `y`: `strength` `[0..1]`, `z`: `center_x_start` (`0.25`), `w`: `center_x_end` (`0.75`) | `(p, 0.4, 0.25, 0.75)` | İlerleme, zoom-blur şiddeti ve odak merkezinin yatay süpürme aralığı |
| `style_pad` | `x`: `center_y` (`0.5`), `y`: `noise_jitter` (`1.0`), `z`: `dissolve_power` (`10.0`), `w`: `_pad` (`0.0`) | `(0.5, 1.0, 10.0, 0.0)` | Dikey merkez, hash titreşim çarpanı, üstel dissolve dikliği |

- **Rust `#[repr(C)]` Karşılığı:**
  ```rust
  #[repr(C)]
  #[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
  pub struct CrossZoomUniform {
      pub motion: [f32; 4],    // [progress, strength, center_x_start, center_x_end]
      pub style_pad: [f32; 4], // [center_y, noise_jitter, dissolve_power, 0.0]
  }
  ```
- **Uyarlama Kuralları:**
  - Sabit döngü: `const CROSS_ZOOM_SAMPLES: i32 = 40;` (`for (var i: i32 = 0; i <= CROSS_ZOOM_SAMPLES; i = i + 1)` — 41 örnek, `< 128` TDR sınırı).
  - `Exponential_easeInOut`: `p == 0.0` ve `p == 1.0` sınırlarında kesin `0.0` ve `1.0` döndürülerek `pow(2.0, ...)` dalgalanmaları ve klip ucu sızıntısı önlenir.
  - `rand(uv)`: `fract(sin(dot(uv, vec2<f32>(12.9898, 78.233))) * 43758.5453) * noise_jitter`. (İsteğe bağlı deterministik alternatif: piksel-ızgara tabanlı `hash12` ile çözünürlükten bağımsız titreşim veya `noise_jitter = 0.0` ile gürültüsüz mod.)
  - Örnekleme: `mix(textureSampleLevel(tex_a, s_linear, mirror_uv(uv_s), 0.0), textureSampleLevel(tex_b, s_linear, mirror_uv(uv_s), 0.0), dissolve) * weight`.

---

#### Şartname 2: `transition_directional_warp.wgsl`
- **Kaynak:** `gl-transitions/transitions/DirectionalWarp.glsl` (MIT)
- **Hedef Dosya:** `crates/frameshift-render/shaders/transition_directional_warp.wgsl`
- **Uniform Yapısı (`DirectionalWarpUniform` — 32 byte, `size % 16 == 0`):**

| Alan (`vec4<f32>`) | Bileşenler | Varsayılan | Açıklama |
| :--- | :--- | :--- | :--- |
| `motion` | `x`: `progress` `[0..1]`, `y`: `smoothness` `[0.001..1.0]`, `z`: `dir_x`, `w`: `dir_y` | `(p, 0.1, -1.0, 1.0)` | İlerleme, kenar yumuşaklığı, 2D süpürme yönü |
| `warp_pad` | `x`: `warp_amount` (`1.0`), `y`: `edge_mode` (`0.0 = mirror`, `1.0 = clamp/transparent`), `z`: `_pad0`, `w`: `_pad1` | `(1.0, 0.0, 0.0, 0.0)` | Ölçek bükülme şiddeti ve kenar dışı örnekleme politikası |

- **Rust `#[repr(C)]` Karşılığı:**
  ```rust
  #[repr(C)]
  #[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
  pub struct DirectionalWarpUniform {
      pub motion: [f32; 4],   // [progress, smoothness, dir_x, dir_y]
      pub warp_pad: [f32; 4], // [warp_amount, edge_mode, 0.0, 0.0]
  }
  ```
- **Uyarlama Kuralları:**
  - **Sıfır Vektör Koruması:** `let raw_dir = params.motion.zw; let len_sq = dot(raw_dir, raw_dir); let safe_dir = select(vec2<f32>(-1.0, 1.0), raw_dir, len_sq > 1e-8); let v_norm = normalize(safe_dir); let v = v_norm / max(abs(v_norm.x) + abs(v_norm.y), 1e-6);`
  - **Yönlü Kenar & `smoothstep` Koruması:** `let s = max(params.motion.y, 1e-4);` ile `smoothstep(-s, 0.0, ...)` daima `low < high` garantisi taşır.
  - **Koordinat Notu:** FrameShift UV'si sol-üst köşelidir (`y` aşağı artar); yukarı-aşağı yönlü geçişlerde `dir_y` işareti ekran uzayına göre yorumlanır (GLSL referansıyla birebir aynı görsel yön istenirse `dir_y` host tarafında `-dir_y` olarak geçirilebilir).
  - **Döngü:** `0` döngü (tek geçiş, 2 `textureSampleLevel` çağrısı).

---

#### Şartname 3: `transition_glitch_displace.wgsl`
- **Kaynak:** `gl-transitions/transitions/GlitchDisplace.glsl` (MIT)
- **Hedef Dosya:** `crates/frameshift-render/shaders/transition_glitch_displace.wgsl`
- **Uniform Yapısı (`GlitchDisplaceUniform` — 32 byte, `size % 16 == 0`):**

| Alan (`vec4<f32>`) | Bileşenler | Varsayılan | Açıklama |
| :--- | :--- | :--- | :--- |
| `motion` | `x`: `progress` `[0..1]`, `y`: `strength` (`1.0`), `z`: `dot_depth` (`0.33`), `w`: `texture_depth` (`0.6`) | `(p, 1.0, 0.33, 0.6)` | İlerleme, yer değiştirme çarpanı ve renk tabanlı sapma katsayıları |
| `glitch_pad` | `x`: `flash_boost` (`2.0`), `y`: `voronoi_scale` (`1.0`), `z`: `seed` (`0.0`), `w`: `_pad` (`0.0`) | `(2.0, 1.0, 0.0, 0.0)` | Orta nokta parlak gri glitch flaşı ve Voronoi hücre ölçeği |

- **Rust `#[repr(C)]` Karşılığı:**
  ```rust
  #[repr(C)]
  #[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
  pub struct GlitchDisplaceUniform {
      pub motion: [f32; 4],     // [progress, strength, dot_depth, texture_depth]
      pub glitch_pad: [f32; 4], // [flash_boost, voronoi_scale, seed, 0.0]
  }
  ```
- **Uyarlama Kuralları:**
  - **Naga-Güvenli `smoothstep` Düzeltmesi:** Orijinal GLSL'deki `smoothstep(1.0, 0.5, progress)` ifadesi WGSL'de tanımsız olduğundan **`1.0 - smoothstep(0.5, 1.0, p)`** olarak yazılır.
  - **Voronoi Döngü Sınırı:** `voronoi()` fonksiyonu `for (var j: i32 = -1; j <= 1; j = j + 1)` ve `for (var i: i32 = -1; i <= 1; i = i + 1)` ile 3×3 = 9 sabit iterasyon kullanır (TDR sınırı olan 128'in çok altında).
  - **GLSL `mod(x, y)` Karşılığı:** WGSL'de `%` operatörü işaretli kalan verir; pozitif modül için `x - y * floor(x / y)` kullanılır.
  - **Premultiplied + `LinearRec709` Uyumu:**
    - `displace()` için gelen `tex` rengi alfa `> 1e-5` ise un-premultiply edilerek (`tex.rgb / max(tex.a, 1e-5)`) şeffaf kenarlarda kaymanın sıfırlanması önlenir; `tex.a == 0.0` ise sapma `0.0` tutulur.
    - Luma flaş katmanı için `LinearRec709` ağırlıkları kullanılır: `let luma = dot(min(d_color2.rgb, d_color1.rgb), vec3<f32>(0.2126, 0.7152, 0.0722));` ve sonuç `vec4<f32>(vec3<f32>(luma * flash_boost) * d_color2.a, d_color2.a)` biçiminde premultiplied korunarak karıştırılır.

---

#### Şartname 4 (Ek AMV Adayı): `transition_swirl.wgsl`
- **Kaynak:** `gl-transitions/transitions/Swirl.glsl` (MIT)
- **Hedef Dosya:** `crates/frameshift-render/shaders/transition_swirl.wgsl`
- **Uniform Yapısı (`SwirlUniform` — 32 byte, `size % 16 == 0`):**
  - `motion: vec4<f32>` → `x`: `progress`, `y`: `strength` (`1.0`, tur sayısı), `z`: `radius` (`0.9`), `w`: `aspect` (`width / height`, varsayılan `16.0 / 9.0`)
  - `center_blur: vec4<f32>` → `x`: `center_x` (`0.5`), `y`: `center_y` (`0.5`), `z`: `blur_amount` (`0.05`), `w`: `zoom_amount` (`0.12`)
- **Uyarlama Notu:** `const TAPS: i32 = 12;` sabit döngüsü (`< 128`), `aspect` uniform üzerinden geçirilerek (FrameShift `effect-shader-api.md` gereği tuval oranını korur), `mirrored()` `transition_whip_chromatic.wgsl` içindeki `mirror_uv()` ile doğrudan paylaşılabilir.

---

#### Şartname 5 (Ek AMV Adayı): `transition_dreamy_zoom.wgsl` & `transition_morph.wgsl`
- **`transition_dreamy_zoom.wgsl` (`DreamyZoomUniform` — 32 byte):**
  - `motion: vec4<f32>` → `x`: `progress`, `y`: `rotation_rad` (gerçek `PI / 180.0` ile dönüştürülmüş radyan; upstream'deki `PI / 80.0` tuhaflığı yerine açık radyan sözleşmesi), `z`: `scale` (`1.2`), `w`: `aspect` (`16.0 / 9.0`)
  - `flash_pad: vec4<f32>` → `x`: `flash_intensity` (`1.0`), `yzw`: `_pad`
  - **Uyarlama Notu:** `atan2` sıfır koruması + flaş eklemesinde `c.rgb + vec3<f32>(flash) * max(c.a, 0.0)` premultiplied alfa kuralı zorunludur.
- **`transition_morph.wgsl` (`MorphUniform` — 16 byte):**
  - `motion: vec4<f32>` → `x`: `progress`, `y`: `strength` (`0.1`), `z`: `unpremul_threshold` (`1e-5`), `w`: `_pad` (`0.0`)
  - **Uyarlama Notu:** `oa` ve `ob` vektörleri un-premultiply edilmiş RGB üzerinden hesaplanır, böylece yarı şeffaf anime katmanlarında yer değiştirme yönü alfa yüzünden bükülmez.

---

### 3. Önerilen Entegrasyon Sırası (AMV / Motion Graphics Önceliği)

1. **`transition_cross_zoom.wgsl`** — Beat-senkronize AMV geçişlerinde `whip_chromatic` yanında en temel tamamlayıcı (hız eğrisi + `SafeExpr` `audio_rms` / `beat_phase` ile `strength` bağlaması için ideal).
2. **`transition_glitch_displace.wgsl`** — Aksiyon / siber / edit kesmelerinde yüksek görsel etki; ayrıca `GlitchMemories` düşük maliyetli ikinci glitch seçeneği olarak ileride eklenebilir.
3. **`transition_directional_warp.wgsl`** — Sıfır döngülü, düşük GPU maliyetli yönlü warp/wipe geçişi.
4. **`transition_swirl.wgsl`** ve **`transition_dreamy_zoom.wgsl`** — Stilistik geçiş paketi olarak ikinci dalgada eklenebilir.

*(Not: `CameraShake.glsl` ve `WarpSpeed.glsl` de MIT lisanslı güçlü AMV adaylarıdır; ancak tam entegrasyon için `CameraShake` ve `WarpSpeed` parametre yoğunluğu nedeniyle ayrı `ShaderTransition` şeması gerektirir.)*

---

## Doğrulanmamış Noktalar

- **Naga & GPU Derleme Testi (Doğrulanmadı):** Bu belgedeki WGSL şartnameleri henüz `crates/frameshift-render/shaders/` altına `.wgsl` dosyası olarak yazılmamış ve `naga::valid::Validator` veya `wgpu` pipeline testlerinden geçirilmemiştir.
- **`LinearRec709` Flaş/Eğri Kalibrasyonu (Doğrulanmadı):** `gl-transitions` kaynaklarındaki sRGB uzayı için ayarlanmış flaş katsayılarının (ör. `DreamyZoom` `+1.0`, `GlitchDisplace` `gray * 2.0`, `FlashFade`) FrameShift'in `LinearRec709` + `AcesLite` tonemap zincirinde üreteceği görsel parlaklık seviyesi henüz render edilip PNG/video üzerinden kalibre edilmemiştir.
- **BSD Lisanslı Geçiş Listesi (Doğrulanmadı):** `crates/gl-transitions/Cargo.toml` ve `README.md` koleksiyonda iki adet BSD lisanslı geçiş bulunduğunu belirtmektedir; hangi iki dosyanın BSD olduğu ayrıca taranmamıştır (bu raporda şartnamesi verilen tüm geçişler bireysel dosya başlıklarından `MIT` olarak doğrulanmıştır).
