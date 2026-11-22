---
title: "Faz 6.3 Part B: Text3D Kinetik Tipografi, Prosedürel Shape3D ve Üçlü Paralel Trim-Path RibbonTrail3D Şartnamesi"
date: 2026-11-22
status: draft
tags: [engine, render-graph, shaders, text-vector, schema]
---

# Faz 6.3 Part B — `Text3D`, `Shape3D` ve Üçlü Paralel `RibbonTrail3D` Teknik Şartnamesi

## Soru / Amaç

FrameShift Faz 6.2 motoru (`crates/frameshift-vector`, `crates/frameshift-render`, `crates/frameshift-schema`) tek satırlı 3D TTF/OTF extrusion (`Text3D`), Lissajous tabanlı 3D şeritler (`RibbonTrail3D`) ve analitik billboard parçacıklar (`ParticleField3D`) sunmaktadır. Ancak `@platsae` referans stil kartı ([SC-0001](../style-cards/SC-0001-platsae-3d-kinetic-typography.md)) ile yapılan adli karşılaştırma sonucunda tespit edilen **üç temel görsel boşluğun** mevcut bağımlılıklarla (`ttf-parser 0.25`, `lyon_tessellation 1.0`, `glam 0.29`, `wgpu 30.0.1`, `bytemuck 1.25.2` — **sıfır yeni Cargo crate**) nasıl kapatılacağı tasarlanmıştır:

1. **`Text3D` Kinetik Tipografi Eksikleri:** Harf bazlı overshoot'lu pop-in (`0 -> 1.15 -> 1.0`) ve gecikmeli harf açılışı (`D-y-i-n-g`, `U-N-T-I-L`), harf bazlı çoklu neon renk döngüsü (`R-I-V-A-L`, `B-A-N-G!`), keskin beyaz/neon dış kontur (stroke), yay/eğrisel taban çizgisi (`YOUR RAGE`), üç materyal stili (`Neon3D`, `MetallicChrome`, `Flat2DStroked`) ve donanım instanced Z-echo derinlik klonlaması (`BANG!`).
2. **Eksik `Shape3D` 3D Primitifleri:** Paylaşımlı `Scene3D` `Depth32Float` tamponuna doğrudan çizilebilen prosedürel 3D mesh'ler: kalın parlak kenarlıklı yüzen kare/dikdörtgen paneller (`NeonTile`), içi boş 5-köşeli neon kontur yıldızlar (`HollowStar5`), beveled 3D kalpler (`Heart3D`), neon zigzag şimşekler (`LightningBolt`) ve fasetli elmaslar (`Diamond3D`).
3. **`RibbonTrail3D` Spagetti Sorunu:** Her instance'ın rastgele fazlı Lissajous düğümleri çizmesi yerine, tek bir temiz 3D S-eğrisi/helis omurgası boyunca birbirine paralel 3 ince neon şerit süpüren **Üçlü Paralel Trim-Path Ribbon** mimarisi.

---

## İncelenen Birincil Kaynaklar

### Harici Birincil Kaynaklar
- **`ttf-parser` v0.25 (`Face::outline_glyph`, `OutlineBuilder`):** [https://docs.rs/ttf-parser/latest/ttf_parser/](https://docs.rs/ttf-parser/latest/ttf_parser/) — erişim/inceleme tarihi: 2026-11-22
- **`lyon_tessellation` v1.0 (`StrokeTessellator`, `FillTessellator`, `StrokeOptions`, `FillRule`):** [https://docs.rs/lyon_tessellation/latest/lyon_tessellation/](https://docs.rs/lyon_tessellation/latest/lyon_tessellation/) — erişim/inceleme tarihi: 2026-11-22
- **`easings.net` (`ai/easings.net` — `easeOutBack`, `c1 = 1.70158`):** [https://easings.net/#easeOutBack](https://easings.net/#easeOutBack) ve [https://github.com/ai/easings.net/blob/master/src/easings/easingsFunctions.ts](https://github.com/ai/easings.net/blob/master/src/easings/easingsFunctions.ts) — erişim/inceleme tarihi: 2026-11-22
- **Pomax — *A Primer on Bézier Curves* (`#derivatives`, `#curvature`, `#arclength`):** [https://pomax.github.io/bezierinfo/#derivatives](https://pomax.github.io/bezierinfo/#derivatives) — erişim/inceleme tarihi: 2026-11-22
- **Andrew J. Hanson & Hui Ma (1995) — *Parallel Transport Approach to Curve Framing* (Indiana Univ. CS Tech Report 425 / Bishop 1975 Rotation-Minimizing Frame):** [https://hdl.handle.net/2022/34260](https://hdl.handle.net/2022/34260) — erişim/inceleme tarihi: 2026-11-22
- **Inigo Quilez — *2D Distance Functions* (`sdHeart`, `sdStar`, `sdPolygon`):** [https://iquilezles.org/articles/distfunctions2d/](https://iquilezles.org/articles/distfunctions2d/) — erişim/inceleme tarihi: 2026-11-22
- **LearnOpenGL — *PBR Theory & Lighting* (Fresnel-Schlick, Trowbridge-Reitz GGX, Smith-GGX):** [https://learnopengl.com/PBR/Theory](https://learnopengl.com/PBR/Theory) ve [https://learnopengl.com/PBR/Lighting](https://learnopengl.com/PBR/Lighting) — erişim/inceleme tarihi: 2026-11-22
- **Jaume Sanchez (`clicktorelease`) — *Creating a Spherical Environment Mapping (MatCap) Shader*:** [https://www.clicktorelease.com/blog/creating-spherical-environment-mapping-shader/](https://www.clicktorelease.com/blog/creating-spherical-environment-mapping-shader/) — erişim/inceleme tarihi: 2026-11-22
- **W3C WebGPU Shading Language (WGSL) — *Memory Layouts (§ 14.4)*:** [https://www.w3.org/TR/WGSL/#memory-layouts](https://www.w3.org/TR/WGSL/#memory-layouts) — erişim/inceleme tarihi: 2026-11-22

### Yerel Kod Tabanı Dosyaları (Doğrudan İncelenen)
- [mesh3d.rs](../../crates/frameshift-vector/src/mesh3d.rs) (satır 1–759)
- [text3d_mesh.wgsl](../../crates/frameshift-render/shaders/text3d_mesh.wgsl) (satır 1–128)
- [ribbon_trail3d.wgsl](../../crates/frameshift-render/shaders/ribbon_trail3d.wgsl) (satır 1–155)
- [particle_field3d.wgsl](../../crates/frameshift-render/shaders/particle_field3d.wgsl) (satır 1–178)
- [uniforms.rs](../../crates/frameshift-render/src/uniforms.rs) (satır 585–696)
- [compositor.rs](../../crates/frameshift-render/src/compositor.rs) (satır 2010–2295, 2867–2911)
- [pipelines.rs](../../crates/frameshift-render/src/pipelines.rs) (satır 391–444)
- [graph.rs](../../crates/frameshift-schema/src/graph.rs) (satır 344–446)
- [project.rs](../../crates/frameshift-schema/src/project.rs) (satır 297–345)
- [SC-0001-platsae-3d-kinetic-typography.md](../style-cards/SC-0001-platsae-3d-kinetic-typography.md)

---

## Doğrudan Gözlemler (Kanıtlı)

1. **Mevcut `MeshVertex3D` / `GpuMeshVertex3D` (48 byte):** [mesh3d.rs](../../crates/frameshift-vector/src/mesh3d.rs#L13-L21) ve [uniforms.rs](../../crates/frameshift-render/src/uniforms.rs#L589-L628) içinde `position: [f32; 3]`, `normal: [f32; 3]`, `uv: [f32; 2]`, `char_center: [f32; 2]`, `char_index: u32`, `face_kind: u32` (`0 = front`, `1 = side`, `2 = back`) tanımlıdır. Bu düzen **48 byte** olup `uv` satır-genel `[0, 1]` normalize koordinatı, `char_center` ise satır merkezine göre harf pivotunu taşır.
2. **Mevcut `text3d_mesh.wgsl` Sınırları ([text3d_mesh.wgsl](../../crates/frameshift-render/shaders/text3d_mesh.wgsl)):**
   - Vertex shader'da (satır 45–64) yalnızca `sin(phase) * amp` dikey zıplama ve `z_tilt` Z-eğimi vardır; pop-in ölçekleme, gecikmeli harf açılışı, 3 eksenli spin, yay eğriliği (`arc`) ve Z-echo instancing yoktur (`compositor.rs:2113` `0..1` instance çizer).
   - Fragment shader'da (satır 89–92) yalnızca `color_bottom -> color_top` dikey gradyan ve `side_color` vardır; `char_index` fragment aşamasına hiç taşınmaz, çoklu renk paleti, dış kontur veya Chrome/Flat2D materyali yoktur.
   - **Tespit edilen gerçek hata (satır 100):** `view_dir` hesaplamasında `vec3<f32>(0.0, 0.15, 2.5)` sabit kamera konumu hardcode edilmiştir; `Camera3D` 90° L-köşesi döndüğünde speküler ve Fresnel highlights yanlış açıdan hesaplanır.
3. **Mevcut `mesh3d.rs` Kontur Yönü ve Delik Davranışı ([mesh3d.rs](../../crates/frameshift-vector/src/mesh3d.rs#L516-L698)):** `append_side_walls`, `ccw_outer` bayrağını toplam işaretli alandan hesaplar; dış kontur **CCW** ve iç delik konturu **CW** sarıldığında `[dy/len, -dx/len]` normali ve `push_side_quad` üçgen sarımı hem dış duvarlarda dışarı, hem iç delik duvarlarında deliğin içine doğru doğru bakar.
4. **Mevcut `ribbon_trail3d.wgsl` Sınırları ([ribbon_trail3d.wgsl](../../crates/frameshift-render/shaders/ribbon_trail3d.wgsl#L47-L75)):** `eval_ribbon_point` her `iid` için bağımsız `freq_x/y/z` ve `phase_x/y/z` ile 3 frekanslı Lissajous eğrisi üretir ve her şeridi farklı açıyla eğer (`tilt`). Bu nedenle `ribbon_count >= 3` olduğunda şeritler paralel akmak yerine düğümlü spagetti görünümü verir. Ayrıca `model` matrisi yoktur; dünya orijini etrafında dönmeye mahkûmdur.
5. **Mevcut `pipelines.rs` Pipeline Durumu ([pipelines.rs](../../crates/frameshift-render/src/pipelines.rs#L391-L444)):** `text3d_mesh` `Depth32Float` (`CompareFunction::Less`, `depth_write_enabled: Some(true)`) + `PREMULTIPLIED_ALPHA_BLENDING` kullanır; `ribbon_trail3d` `TriangleStrip` + `hdr_additive_premul_blend` kullanır.

---

## Çıkarımlar ve Öneriler (Uygulamaya Hazır Şartname)

> **Not:** Aşağıdaki tüm tasarım kararları mevcut `frameshift-vector` / `frameshift-render` / `frameshift-schema` mimarisi üzerine inşa edilmiş çıkarımlardır; **Part A** ([faz6-3-scene3d-dof-camera.md](faz6-3-scene3d-dof-camera.md)) ile ortak `Scene3D` `Depth32Float` ve `Camera3D` geçişini paylaşacak şekilde hizalanmıştır.

---

### Bölüm 1 — `Text3D` Kinetik Tipografi Yükseltmeleri

#### 1.1 `GpuMeshVertex3D` Uyumluluğu (Sıfır Vertex Layout Değişikliği)
Mevcut 48-byte `MeshVertex3D` / `GpuMeshVertex3D` yapısı **hiç değiştirilmeden** korunur. Yalnızca `face_kind: u32` semantiği genişletilir:

| `face_kind` | Anlamı (`Text3D`) | Anlamı (`Shape3D`) | Shader Renk Kaynağı |
| :--- | :--- | :--- | :--- |
| `0u` | Ön kapak (`+Z`) | Ön yüzey (`+Z`) | `color_mode` (gradyan / harf paleti / gökkuşağı) |
| `1u` | Yan duvar & bevel | Yan duvar & bevel | `side_color` (veya palet tonu ile modüle) |
| `2u` | Arka kapak (`-Z`) | Arka yüzey (`-Z`) | `color_mode` |
| `3u` | Dış kontur / stroke halkası | Kalın dış çerçeve (`NeonTile` border / stroke) | `outline_color` (HDR beyaz / neon) |

#### 1.2 Keskin Dış Kontur / Stroke Üretimi (`mesh3d.rs` + `lyon_tessellation::StrokeTessellator`)
- `extrude_text_to_mesh_with_outline(font_bytes, text, font_size, extrude_depth, bevel_size, outline_width)` yeni bir fonksiyon olarak eklenir; mevcut `extrude_text_to_mesh` ve `extrude_text_to_mesh_with_bevel` `outline_width = 0.0` ile buna delege eder (**%100 geriye uyumlu**).
- `outline_width > 0.0` olduğunda:
  1. Önce her zamanki gibi ön/arka kapaklar (`face_kind = 0u, 2u`) ve yan duvarlar (`face_kind = 1u`) push edilir.
  2. Ardından `lyon_tessellation::StrokeTessellator` ile `glyph.path` üzerinde:
     ```rust
     let stroke_opts = StrokeOptions::tolerance(tolerance)
         .with_line_width(outline_width * 2.0)
         .with_line_join(LineJoin::Round) // display fontlarda miter sivri taşmalarını önler
         .with_line_cap(LineCap::Round);
     ```
     tessellasyon çalıştırılır. `line_width = 2.0 * outline_width` seçilmesinin nedeni, `StrokeTessellator`'ın çizgiyi konturun ortasına yerleştirmesidir (yarısı içeride, yarısı `outline_width` kadar dışarıda).
  3. **Sıfır Overdraw / Z-Buffer Çarpışmasız Yerleşim:** Stroke üçgenleri `z_front_stroke = half_depth - 0.001` ve `z_back_stroke = -half_depth + 0.001` derinliğinde (`face_kind = 3u`) push edilir.
     - `text3d_mesh` `DepthCompare::Less` ve `depth_write_enabled: true` kullandığı ve ön/arka kapaklar **önce** çizildiği için, konturun içte kalan yarısı donanım Z-testi tarafından pikselsel olarak elenir; yalnızca harfin dış çevresi ve iç boşluk (`O`, `A`, `B`, `R`) kenarları boyunca net `outline_width` kalınlığında dış çerçeve görünür. Yarı saydam `alpha < 1.0` durumunda bile çift karışım (double-blending) oluşmaz.
  4. **3D Ekstrüzyonlu Metinlerde Kenar Konturu:** `extrude_depth > 0.0` ve `material_mode != Flat2DStroked` iken yan duvarlarda (`face_kind = 1u`) ekstra olarak Fresnel kenar vurgusu `outline_color` ile karıştırılabilir; `Flat2DStroked` (`extrude_depth == 0.0` veya `material_mode == 2`) modunda `z_front_stroke = -0.001` ile ön kapak (`z = 0.0`) arkasında bıçak gibi keskin vektör stroke oluşur.

#### 1.3 Yeni `Text3DUniform` Düzeni (416 Byte, %100 16-Byte Hizalı)
W3C WGSL § 14.4 (`var<uniform>`) kurallarına göre tüm üyeler `mat4x4<f32>` (64B) ve `vec4<f32>` (16B) olarak hizalanmıştır; `vec3` dolgu tuzağı yoktur ve sabit `vec3(0.0, 0.15, 2.5)` hatası `view` + `cam_pos` ile düzeltilmiştir:

```rust
/// Uniform buffer for upgraded `text3d_mesh.wgsl` (416 bytes, 16-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct Text3DUniform {
    pub mvp: [[f32; 4]; 4],            //   0..64
    pub model: [[f32; 4]; 4],          //  64..128
    pub view: [[f32; 4]; 4],           // 128..192 (MatCap view-space normal için)
    /// xyz = camera_world_pos, w = material_mode (0=neon_3d, 1=metallic_chrome, 2=flat_2d_stroked)
    pub cam_material: [f32; 4],        // 192..208
    pub color_top: [f32; 4],           // 208..224
    pub color_bottom: [f32; 4],        // 224..240
    pub side_color: [f32; 4],          // 240..256
    pub outline_color: [f32; 4],       // 256..272 (face_kind == 3u rengi)
    /// x = time_sec, y = char_stagger_sec, z = char_bounce_amp, w = char_wave_freq
    pub anim: [f32; 4],                // 272..288
    /// x = pop_duration_sec (0=kapalı), y = pop_overshoot_c1 (2.15 -> ~1.15 tepe),
    /// z = spin_in_deg, w = char_jitter_deg
    pub kinetic: [f32; 4],             // 288..304
    /// x = arc_curvature (1/R, 0=düz), y = arc_axis (0=XY yay, 1=XZ silindir),
    /// z = color_mode (0=gradyan, 1=palet_dongu, 2=neon_gokkusagi), w = palette_count (1..4)
    pub arc_palette: [f32; 4],         // 304..320
    /// xyz = echo_offset (model/world uzayı adım), w = echo_count (0..16)
    pub echo_params: [f32; 4],         // 320..336
    /// x = echo_alpha_decay (örn. 0.55), y = echo_scale_step (örn. 0.94),
    /// z = hue_shift_per_char, w = hdr_intensity (varsayılan 1.0)
    pub echo_decay: [f32; 4],          // 336..352
    /// Harf bazlı özel 4 renkli palet (Linear Rec.709 RGBA)
    pub palette: [[f32; 4]; 4],        // 352..416
}
const _: () = assert!(std::mem::size_of::<Text3DUniform>() == 416);
```

#### 1.4 Stateless Harf Bazlı Kinetik Animasyon Matematiği (WGSL)
Her harf tamamen seek-independent olarak `in.char_index` ve `u.anim.x` üzerinden hesaplanır:

1. **Gecikmeli Harf Açılışı (`D-y-i-n-g`, `U-N-T-I-L`) ve `ease_out_back` Overshoot:**
   - Yerel harf zamanı: `let char_t = u.anim.x - f32(in.char_index) * u.anim.y;`
   - `pop_dur = u.kinetic.x`:
     - `pop_dur <= 0.0` ise `pop_scale = 1.0`, `spin_rem = 0.0` (**Faz 6.2 ile %100 geriye uyumlu**).
     - `pop_dur > 0.0` ise:
       - `let p = clamp(char_t / pop_dur, 0.0, 1.0);`
       - `p <= 0.0` iken `pop_scale = 0.0` (harf henüz açılmamıştır; köşeler `char_center` noktasına çöker, üçgen alanı 0 olduğu için GPU rasterizer tarafından atlanır ve `out.char_alpha = 0.0` fragment discard garantisi verir).
       - `ease_out_back(p, c1)` (`easings.net` referans formülü):
         ```wgsl
         fn ease_out_back(x: f32, c1: f32) -> f32 {
             let c3 = c1 + 1.0;
             let xm1 = x - 1.0;
             return 1.0 + c3 * (xm1 * xm1 * xm1) + c1 * (xm1 * xm1);
         }
         ```
       - **Matematiksel Tepe Kanıtı:** Türevin sıfır olduğu noktada tepe değeri `1.0 + (4.0 * c1^3) / (27.0 * (c1 + 1.0)^2)` olur. `c1 = 1.70158` için tepe **`1.100`** (%10 overshoot); **`c1 = 2.15`** için tepe **`1.1484` (~`1.15`)** olur (`0 -> 1.15 -> 1.0` PlatsAE sıçraması).
2. **Harf Bazlı 3 Eksenli Spin-In + Kalıcı Mikro-Tilt:**
   - Giriş dönüş açısı `p -> 1` iken sıfıra sönümlenir:
     `let spin_decay = pow(1.0 - p, 3.0);` (veya `p <= 0.0` iken `1.0`).
     Her harfe deterministik yön çeşitliliği vermek için `pcg_hash(in.char_index)` ile işaret/aksiyon üretilir:
     - `theta_z = radians(u.kinetic.z) * spin_decay * sign_z + wave_tilt + jitter_z`
     - `theta_y = radians(u.kinetic.z * 0.65) * spin_decay * sign_y`
     - `theta_x = radians(u.kinetic.z * 0.35) * spin_decay`
   - Pivot `vec3<f32>(in.char_center, 0.0)` etrafında dönüş sırası:
     1. `p_rel = in.position - vec3<f32>(in.char_center, 0.0)`
     2. `p_scaled = p_rel * (pop_scale * echo_scale)`
     3. `p_rot = rotate_xyz(p_scaled, theta_x, theta_y, theta_z)` (aynı rotasyon `in.normal`'a da uygulanır!)
     4. `p_local = vec3<f32>(in.char_center, 0.0) + p_rot + vec3<f32>(0.0, y_bounce, 0.0)`

#### 1.5 Eğrisel / Yay Taban Çizgisi (`arc_curvature`, `arc_axis`)
`YOUR RAGE` gibi kavisli başlıklar için `p_local` üzerinden dünya/model uzayı bükülmesi uygulanır (`k = u.arc_palette.x`):
- `abs(k) > 1e-4` ise `R = 1.0 / k`, `phi = clamp(p_local.x * k, -3.14159265, 3.14159265)`:
  - **Sayısal Kararlı `1 - cos(phi)`:** `let one_minus_cos = 2.0 * sin(0.5 * phi) * sin(0.5 * phi);` (küçük `k` değerlerinde catastrophic cancellation önler).
  - **`arc_axis == 0` (XY Gökkuşağı / Gülümseme Yayı):**
    - `x_arc = (R + p_local.y) * sin(phi)`
    - `y_arc = p_local.y * cos(phi) - R * one_minus_cos`
    - Normal vektör XY düzleminde `-phi` kadar döndürülür (`rot_z(-phi)`).
  - **`arc_axis == 1` (XZ Silindirik Sarım — Kamera Etrafında Kavis):**
    - `x_arc = (R + p_local.z) * sin(phi)`
    - `z_arc = p_local.z * cos(phi) - R * one_minus_cos`
    - Normal vektör XZ düzleminde `+phi` kadar döndürülür (`rot_y(phi)`).

#### 1.6 Harf Bazlı Çoklu Renk Paleti (`color_mode`)
`char_index` vertex shader'dan fragment shader'a `@location(4) @interpolate(flat) char_index: u32` olarak aktarılır:
- `color_mode == 0u` (**Legacy Vertical Gradient**): `mix(u.color_bottom, u.color_top, grad_t)` — Faz 6.2 ile birebir aynı.
- `color_mode == 1u` (**Discrete Palette Cycle** — `R-I-V-A-L`, `B-A-N-G!`):
  ```wgsl
  let p_count = max(u32(round(u.arc_palette.w)), 1u);
  let slot = (in.char_index % min(p_count, 4u));
  let base_p = u.palette[slot];
  let cap_color = vec4<f32>(base_p.rgb * mix(0.82, 1.18, grad_t), base_p.a);
  ```
- `color_mode == 2u` (**Neon HSV Rainbow Cycle** — 4'ten fazla renkli uzun kelimeler):
  `hue = fract(f32(in.char_index) * u.echo_decay.z + u.anim.x * 0.15)` ile yüksek doygunluklu `hsv_to_linear_rgb(hue, 0.92, 1.0)` üretilir.

#### 1.7 Materyal Modları (`material_mode` in `u.cam_material.w`)
- **`0u` (`Neon3D`):** Mevcut 3-noktalı stüdyo HDR ışıklandırma (`text3d_mesh.wgsl:102-126`) + dinamik `u.cam_material.xyz` kamera pozisyonu ile doğru `view_dir` + `u.echo_decay.w` (`hdr_intensity`) çarpanı.
- **`1u` (`MetallicChrome` — Analitik View-Space MatCap + Cook-Torrance/Schlick Fresnel):**
  - Ek doku bağlaması gerektirmeden `n_view = safe_normalize((u.view * vec4<f32>(n, 0.0)).xyz, vec3<f32>(0.0, 0.0, 1.0))` kullanılır (`clicktorelease` view-space normal eşlemesi):
  - `let r_vec = reflect(-view_dir, n);` ve `let horizon = smoothstep(-0.18, 0.04, r_vec.y);` ile **yüksek kontrastlı sıvı krom stüdyo ufuk bandı** (alt koyu zemin yansıması, keskin beyaz ufuk parlaması, üstte softbox şeritleri `pow(max(dot(n_view.xy, ...)), 12.0)`) sentezlenir.
  - LearnOpenGL PBR Fresnel-Schlick (`F0 = mix(vec3<f32>(0.04), base_rgb, 0.92)`):
    `let f = F0 + (vec3<f32>(1.0) - F0) * pow(1.0 - max(dot(n, view_dir), 0.0), 5.0);`
  - Güçlü GGX/Blinn speküler çekirdeği (`pow(ndoth, 64.0) * 2.4`) `DeepGlow` için parlak HDR krom parıltıları üretir.
- **`2u` (`Flat2DStroked`):** `face_kind != 1u` yüzeyleri için gölgelemesiz saf HDR renk (`base_rgb * hdr_intensity`), `face_kind == 3u` için keskin `u.outline_color.rgb * hdr_intensity` stroke çıktısı verir.

#### 1.8 Donanım Instanced Z-Echo Derinlik Klonlaması (`BANG!` Yankı Kuyruğu)
- `compositor.rs` `pass.draw_indexed(0..cached.index_count, 0, 0..(1 + echo_count))` çağrısı yapar (ekstra vertex/index tamponu kopyalamadan tek draw call).
- **Arka-Öne (Back-to-Front) Sıralama Kuralı:**
  - `let total_echoes = u32(clamp(round(u.echo_params.w), 0.0, 16.0));`
  - `let echo_layer = total_echoes - min(instance_id, total_echoes);`
  - Böylece `instance_id == 0` **en arkadaki yankıyı** (`echo_layer = total_echoes`), `instance_id == total_echoes` ise **en öndeki kahraman metni** (`echo_layer = 0`) çizer!
  - `echo_layer >= 1u` için:
    - Konum ötelemesi: `p_echo = p_curved * pow(u.echo_decay.y, f32(echo_layer)) + u.echo_params.xyz * f32(echo_layer)`
    - Alfa azalması: `echo_alpha = pow(clamp(u.echo_decay.x, 0.0, 1.0), f32(echo_layer))`
    - Bu sıra sayesinde yarı saydam Z-yankıları (`PREMULTIPLIED_ALPHA_BLENDING`) arkadan öne doğru doğru harmanlanır ve `echo_layer = 0` katmanı ön derinliği `Depth32Float`'a yazdığında `Scene3D` içindeki diğer objelerle gerçek Z-oklüzyonu korunur.

#### 1.9 `NodeKind::Text3D` Geriye Uyumlu Şema Uzantısı
[graph.rs](../../crates/frameshift-schema/src/graph.rs#L346-L381) içindeki `NodeKind::Text3D` varyantına tümü `#[serde(default = ...)]` ile korunan yeni alanlar eklenir (mevcut Faz 6.2 JSON projeleri sıfır değişiklikle yüklenir):
- `outline_width: f32` (default `0.0`)
- `outline_color_rgba: [f32; 4]` (default `[1.0, 1.0, 1.0, 1.0]`)
- `material_mode: u32` (default `0` -> `Neon3D`)
- `color_mode: u32` (default `0` -> `VerticalGradient`)
- `palette_rgba: Vec<[f32; 4]>` (default `[]`, ilk 4 eleman `Text3DUniform::palette`'e yazılır)
- `pop_duration_sec: f32` (default `0.0`)
- `pop_overshoot: f32` (default `2.15`)
- `spin_in_deg: f32` (default `0.0`)
- `char_jitter_deg: f32` (default `0.0`)
- `arc_curvature: f32` (default `0.0`)
- `arc_axis: u32` (default `0`)
- `echo_count: u32` (default `0`)
- `echo_offset: [f32; 3]` (default `[0.0, 0.0, -0.35]`)
- `echo_alpha_decay: f32` (default `0.55`)
- `echo_scale_step: f32` (default `0.94`)
- `hdr_intensity: f32` (default `1.0`)
- `SafeExpr` beyaz listesi ([project.rs](../../crates/frameshift-schema/src/project.rs#L297-L314)): `"arc_curvature"`, `"hdr_intensity"`, `"pop_duration_sec"`, `"spin_in_deg"` eklenir.

---

### Bölüm 2 — Prosedürel `Shape3D` 3D Primitifleri (`frameshift-vector` + `NodeKind::Shape3D`)

#### 2.1 Birleşik Mesh Sözleşmesi (Sıfır Yeni Crate, Ortak `GpuMeshVertex3D`)
Tüm `Shape3D` primitifleri `frameshift-vector::shape3d` modülünde (`lyon_tessellation::FillTessellator` + `StrokeTessellator` ile) üretilir ve mevcut `ExtrudedTextMesh` (veya ortak tip alias'ı `ExtrudedMesh3D = ExtrudedTextMesh`) + `MeshVertex3D` (48 byte) döndürür. Böylece `Shape3D` nesneleri doğrudan `text3d_mesh.wgsl` pipeline'ını, `GpuMeshVertex3D` tamponunu, `MetallicChrome` / `Neon3D` / `Flat2DStroked` materyallerini, `ease_out_back` pop-in'i (`char_index = 0`, `char_center = [0.0, 0.0]`), `Z-echo`'yu ve `Scene3D`'nin ortak `Depth32Float` derinlik tamponunu paylaşır!

#### 2.2 Beş Prosedürel Primitif Tasarımı (Normalize `[-0.5, 0.5]` Birim Uzayında)

| Primitif (`Shape3DKind`) | Geometri & `lyon` İnşası | `face_kind` Ataması & Ekstrüzyon |
| :--- | :--- | :--- |
| **`NeonTile`** | Genişlik `w`, yükseklik `h` (varsayılan `1.0 × 1.0`), `corner_radius`, `border_width`. İki katman: (1) İç renkli levha (`w - 2*border_width`, `h - 2*border_width`), `Path::builder()` + `quadratic_bezier_to` yuvarlatılmış köşelerle CCW doldurulur; (2) Dış çerçeve: dış CCW dikdörtgen + iç CW dikdörtgen delik `FillRule::EvenOdd` ile tessellate edilir. | İç levha kapakları: `0u / 2u` (gradyan); Dış çerçeve ön/arka: `3u` (`outline_color` — parlak beyaz/neon); Çerçeve yan duvarları: `1u`. |
| **`HollowStar5`** | 5 dış tepe ($R_{\text{out}} = 0.5$), 5 iç çukur ($R_{\text{in}} = R_{\text{out}} \cdot r_{\text{ratio}}$, klasik pentagram için $\sin(18^\circ)/\sin(54^\circ) \approx 0.382$). Her köşede birim kenar normalleri $\hat{n}_{\text{prev}}, \hat{n}_{\text{next}}$ üzerinden exact miter bisector $\vec{m}_i = \frac{\hat{n}_{\text{prev}} + \hat{n}_{\text{next}}}{\|\hat{n}_{\text{prev}} + \hat{n}_{\text{next}}\| \cdot \max(\hat{m}_0 \cdot \hat{n}_{\text{prev}}, 0.25)}$ hesaplanır. Dış halka CCW ($V_i + 0.5 w \vec{m}_i$), iç boşluk CW ($V_i - 0.5 w \vec{m}_i$) olarak `FillTessellator` (`FillRule::EvenOdd`) ve `append_side_walls` içine verilir. | Kapaklar `0u / 2u` (veya içi boş neon çerçeve vurgusu için `outline_color` opsiyonlu `3u`), hem dış hem iç boşluk yan duvarları `1u`. `mesh3d.rs` hole-winding kuralıyla uyumlu su geçirmez 3D mesh. |
| **`Heart3D`** | Simetrik 4 segmentli kübik Bézier kalp eğrisi (alt uç `(0, -0.45)`, sağ/sol üst loblar `(±0.50, +0.28)`, üst yarık `(0, +0.22)` — tüm kontrol noktaları `[-0.5, 0.5]` içinde) `lyon` `cubic_bezier_to` ile eğrisel düzgünlükte oluşturulur. `bevel_size > 0` ile fasetli 3D kalp yüzeyi, `outline_width > 0` ile `StrokeTessellator` (`face_kind = 3u`) neon çevre konturu eklenir. | Ön/arka kapak `0u / 2u`, bevel+yan duvar `1u`, opsiyonel kontur `3u`. |
| **`LightningBolt`** | 7–11 köşeli keskin zigzag şimşek CCW poligonu (normalize `[-0.5, 0.5]` içinde üst geniş giriş, orta keskin kırılma, alt sivri uç). `FillTessellator` + `append_side_walls` (bevel destekli) + opsiyonel `StrokeTessellator` (`LineJoin::Miter`, `miter_limit = 3.0`, `face_kind = 3u`). | Ön/arka kapak `0u / 2u`, yan duvar `1u`, dış neon kontur `3u`. |
| **`Diamond3D`** | 4-köşeli anime fasetli elmas/yıldız: `inner_ratio` parametresi `1.0` iken keskin eşkenar elmas/baklava, `0.22..0.40` aralığında Inigo Quilez `sdStar` / `particle_field3d.wgsl` `sdf_star4` benzeri 4-köşeli içbükey parıltı yıldızı (8 köşe CCW). Bevel ile ışığı yakalayan elmas fasetleri oluşur. | Ön/arka kapak `0u / 2u`, bevel+yan duvar `1u`, opsiyonel kontur `3u`. |

#### 2.3 `NodeKind::Shape3D` Rust Şema Önerisi
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shape3DKind {
    NeonTile,
    HollowStar5,
    Heart3D,
    LightningBolt,
    Diamond3D,
}

// NodeKind içinde yeni 0-girdili üretici düğüm:
#[serde(alias = "shape_3d")]
Shape3D {
    pub shape_kind: Shape3DKind,
    #[serde(default = "default_one")]
    pub size: f32,
    #[serde(default = "default_one")]
    pub aspect: f32,               // NeonTile için genişlik/yükseklik oranı
    #[serde(default = "default_shape_stroke")]
    pub stroke_width: f32,         // NeonTile border kalınlığı / HollowStar5 halka kalınlığı / dış kontur
    #[serde(default)]
    pub corner_radius: f32,        // NeonTile köşe yuvarlama
    #[serde(default = "default_star_inner_ratio")]
    pub inner_ratio: f32,          // HollowStar5 (0.382) / Diamond3D (0.30 veya 1.0)
    #[serde(default = "default_text3d_extrude_depth")]
    pub extrude_depth: f32,
    #[serde(default = "default_text3d_bevel_size")]
    pub bevel_size: f32,
    #[serde(default)]
    pub material_mode: u32,        // 0=neon_3d, 1=metallic_chrome, 2=flat_2d_stroked
    #[serde(default = "default_text3d_color_top")]
    pub color_top_rgba: [f32; 4],
    #[serde(default = "default_text3d_color_bottom")]
    pub color_bottom_rgba: [f32; 4],
    #[serde(default = "default_text3d_side_color")]
    pub side_color_rgba: [f32; 4],
    #[serde(default = "default_white_rgba")]
    pub outline_color_rgba: [f32; 4],
    #[serde(default)]
    pub pos: [f32; 3],
    #[serde(default)]
    pub rot_deg: [f32; 3],
    #[serde(default = "default_one_vec3")]
    pub scale: [f32; 3],
    // Tekil kullanım için kamera alanları (Scene3D içinde üst kamera tarafından override edilir):
    #[serde(default = "default_camera_pos")]
    pub camera_pos: [f32; 3],
    #[serde(default)]
    pub camera_rot_deg: [f32; 3],
    #[serde(default = "default_fov_deg")]
    pub fov_deg: f32,
    // Kinetik animasyon ve Z-echo:
    #[serde(default)]
    pub pop_duration_sec: f32,
    #[serde(default = "default_pop_overshoot")]
    pub pop_overshoot: f32,
    #[serde(default)]
    pub spin_in_deg: f32,
    #[serde(default)]
    pub echo_count: u32,
    #[serde(default = "default_echo_offset")]
    pub echo_offset: [f32; 3],
    #[serde(default = "default_echo_alpha_decay")]
    pub echo_alpha_decay: f32,
    #[serde(default = "default_one")]
    pub hdr_intensity: f32,
    #[serde(default)]
    pub time_offset_sec: f32,
}
```
- **Önbellek Anahtarı (`compositor.rs`):** `(shape_kind_u8, size.to_bits(), aspect.to_bits(), stroke_width.to_bits(), corner_radius.to_bits(), inner_ratio.to_bits(), extrude_depth.to_bits(), bevel_size.to_bits())` üzerinden `shape3d_mesh_cache` (veya ortak `mesh3d_cache`) içinde tek sefer GPU buffer'a yüklenir.

---

### Bölüm 3 — Temiz Üçlü Paralel Trim-Path `RibbonTrail3D` Yükseltmesi

#### 3.1 Spagetti Sorunu ve Çözüm İlkesi
PlatsAE referansındaki şeritler birbirini rastgele kesen düğümler değildir; **tek bir akıcı 3D S-eğrisi veya helis omurga** boyunca yan yana sabit aralıkla akan **3 paralel ince neon fiberden** oluşur.
- `ribbon_style = 0u` (`lissajous_legacy`): Mevcut [ribbon_trail3d.wgsl](../../crates/frameshift-render/shaders/ribbon_trail3d.wgsl#L47-L75) fonksiyonu korunur (**%100 geriye uyumlu**).
- `ribbon_style = 1u` (`parallel_triple_bundle` — 3D Kübik Bézier S-Eğrisi):
  - `strand_idx = iid % 3u` (`-1.0, 0.0, +1.0` paralel şerit indeksi)
  - `bundle_idx = iid / 3u` (`ribbon_count = 3` → 1 üçlü demet, `ribbon_count = 6` → 2 ayrı üçlü demet!)

#### 3.2 Twist-Free / Flip-Free Kübik Bézier S-Eğrisi + Bishop Çerçeve Matematiği
Differential geometride Frenet-Serret çerçevesi ($\mathbf{T}, \mathbf{N}, \mathbf{B}$), S-eğrilerinin büküm noktalarında ($P''(s) = 0$) 180° ters döner (Hanson & Ma 1995, *Parallel Transport Approach to Curve Framing*). Stateless vertex shader'da sıfır büküm (twist-free) paralel demet garantisi için:

1. **Monoton Yerel İlerlemeli Kübik Bézier Omurgası:**
   `bundle_idx` ve `seed` üzerinden deterministik 4 kontrol noktası kurulur (`P0, P1, P2, P3`, yerel `x` koordinatı kesin artan: `P0.x < P1.x <= P2.x < P3.x`):
   ```wgsl
   // Pomax Bézier Primer: konum B(s) ve analitik 1. türev B'(s)
   fn eval_cubic_bezier(p0: vec3<f32>, p1: vec3<f32>, p2: vec3<f32>, p3: vec3<f32>, s: f32) -> vec3<f32> {
       let u = 1.0 - s;
       let u2 = u * u;
       let s2 = s * s;
       return p0 * (u2 * u) + p1 * (3.0 * u2 * s) + p2 * (3.0 * u * s2) + p3 * (s2 * s);
   }

   fn eval_cubic_bezier_deriv(p0: vec3<f32>, p1: vec3<f32>, p2: vec3<f32>, p3: vec3<f32>, s: f32) -> vec3<f32> {
       let u = 1.0 - s;
       return 3.0 * u * u * (p1 - p0) + 6.0 * u * s * (p2 - p1) + 3.0 * s * s * (p3 - p2);
   }
   ```
   Kontrol noktalarının zamanla akışkan hareketi için `P1` ve `P2` noktalarına düşük frekanslı analitik faz salınımı (`sin(time_sec * speed + h_phase) * 0.25 * radius`) eklenir; ancak `x'(s) > 0` korunduğu için teğet $\mathbf{T}_{\text{local}}(s)$ hiçbir zaman yerel $Z$ ekseniyle $(0, 0, 1)$ paralel olmaz!
2. **Tekilliksiz Paralel Şerit Ofseti:**
   - Yerel omurga teğeti: `T = safe_normalize(eval_cubic_bezier_deriv(...), vec3<f32>(1.0, 0.0, 0.0))`
   - Referans vektör `U_ref = vec3<f32>(0.0, 0.0, 1.0)` ile `B = safe_normalize(cross(T, U_ref), vec3<f32>(0.0, 1.0, 0.0))` ve `N = cross(B, T)` elde edilir.
   - `N` ve `B` S-eğrisinin büküm noktasında dahi **asla 180° sıçramaz**.
   - Paralel şerit ofseti:
     ```wgsl
     let strand_idx = iid % 3u;
     let lane = f32(strand_idx) - 1.0; // -1.0, 0.0, +1.0
     let spacing = width * max(u.params2.y, 1.2); // varsayılan 2.2 * width -> net karanlık ara boşluk
     let strand_center = center_pos + N * (lane * spacing);
     ```
   - Daha sonra `u.model` matrisi ile demet 3D sahnede herhangi bir kelimenin veya L-köşesinin etrafına döndürülüp konumlandırılır (`world_center = (u.model * vec4<f32>(strand_center, 1.0)).xyz`).
   - Her şerit kendi `t_seg` uzunluğu boyunca `lane * u.params2.z` (`trim_stagger`) kadar kaydırılabilir; böylece 3 paralel şerit milimetrik arayla birbirini kovalayan Trim-Path animasyonu yapar!
3. **Opsiyonel `ribbon_style = 2u` (`parallel_triple_helix`):** Kelime etrafında dönen temiz tek sarmallı ark (`theta = mix(-2.2, 2.2, s) + time_sec * speed`, sabit eğimli temiz yay + `N` boyunca 3 paralel şerit).

#### 3.3 `RibbonTrail3DUniform` Düzeni (224 Byte, 16-Byte Hizalı)
İlk 144 byte ([uniforms.rs](../../crates/frameshift-render/src/uniforms.rs#L663-L673)) **birebir korunur**:
```rust
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct RibbonTrail3DUniform {
    pub view_proj: [[f32; 4]; 4],     //   0..64
    pub cam_pos_time: [f32; 4],       //  64..80
    pub color_a: [f32; 4],            //  80..96
    pub color_b: [f32; 4],            //  96..112
    /// x = width, y = speed, z = radius, w = seed_f32
    pub params0: [f32; 4],            // 112..128
    /// x = trim_start, y = trim_end, z = segments_f32, w = ribbon_count_f32
    pub params1: [f32; 4],            // 128..144
    pub model: [[f32; 4]; 4],         // 144..208 (3D konum/rotasyon/ölçek için)
    /// x = ribbon_style (0=lissajous_legacy, 1=parallel_triple_bundle, 2=parallel_triple_helix),
    /// y = strand_spacing_mult (varsayılan 2.2),
    /// z = trim_stagger (varsayılan 0.03),
    /// w = curve_bend (varsayılan 1.0)
    pub params2: [f32; 4],            // 208..224
}
const _: () = assert!(std::mem::size_of::<RibbonTrail3DUniform>() == 224);
```
`NodeKind::RibbonTrail3D` yeni `#[serde(default)]` alanları: `ribbon_style: u32` (default `0`), `strand_spacing_mult: f32` (default `2.2`), `trim_stagger: f32` (default `0.03`), `curve_bend: f32` (default `1.0`), `pos: [f32; 3]` (default `[0.0; 3]`), `rot_deg: [f32; 3]` (default `[0.0; 3]`). `SafeExpr` beyaz listesine `"pos_x"`, `"pos_y"`, `"pos_z"`, `"rot_x"`, `"rot_y"`, `"rot_z"`, `"curve_bend"` eklenir.

---

## Doğrulanmamış Noktalar

1. **Karmaşık Display Fontlarda `StrokeTessellator` Kendini Kesme Davranışı:** Serbest/el yazısı veya aşırı dar aralıklı TTF/OTF fontlarda komşu harflerin kalın dış konturları (`outline_width` büyükken) birbirinin üzerine binebilir. `DepthCompare::Less` her harf arasında da derinlik yazdığı için yarı saydam `alpha < 1.0` konturlarda komşu harf kesişimleri görsel olarak test edilmelidir.
2. **Yüksek `echo_count` + `DeepGlow` HDR Doygunluğu:** `echo_count = 6..8` ve yüksek `hdr_intensity` birlikte kullanıldığında arka arkaya binen yarı saydam yankı katmanlarının `DeepGlow` mip piramidinde beyazlamaya (clipping) yol açmaması için `echo_alpha_decay <= 0.55` aralığı gerçek sahnede kalibre edilmelidir.
3. **Ortak `Scene3D` Depth-Write Etkileşimi (`Shape3D` / `Text3D` / `RibbonTrail3D`):** `RibbonTrail3D` şu an `hdr_additive_premul_blend` kullanır ve `DepthStencilState` içermez. `Scene3D` birleşik geçişinde `RibbonTrail3D`'nin kelimenin önünden/arkasından geçebilmesi için `depth_compare: Less` + `depth_write_enabled: false` ile `Scene3D` derinlik tamponunu okuması (Part A şartnamesiyle birlikte) derleme ve görsel testle doğrulanacaktır.
