---
id: faz6-6-optics-tonemap-aa
status: active
created: 2026-11-22
related_specs: [render-graph, effect-shader-api, project-schema]
tags: [color, shaders, render-graph, engine]
---

# Faz 6.6 Teknik Araştırma ve Uygulama Şartnamesi: Optik, Tonemapping, Anti-Aliasing, Mipmap, FilmOptics, DeepGlow & Additive Glow Koruması

**Durum:** Araştırma Tamamlandı (`0` yeni Cargo crate hedefi — `wgpu 30.0.1` çekirdek yetenekleri + saf WGSL/Rust)  
**Kod Tabanı Notu:** Görev özetinde geçen `NodeKind` tanımları `crates/frameshift-schema/src/node.rs` yerine fiilen [graph.rs](../../crates/frameshift-schema/src/graph.rs#L287-L931) içinde, `bindable_param_names` ve `RenderProfileDef` ise [project.rs](../../crates/frameshift-schema/src/project.rs#L165-L360) içinde yer almaktadır.

---

## Özet Tablo: Kapsam ve Kesin Değişecek Dosyalar

| Başlık | Mevcut Durum | Faz 6.6 Hedefi | Etkilenecek Dosyalar |
| :--- | :--- | :--- | :--- |
| **(a) Tonemapping & Dithering** | `None`, `Reinhard`, `AcesLite`; dithering yok (`Rgba8Unorm` bantlaşma riski) | **AgX** (Sobotka / Filament / Three.js) + **Khronos PBR Neutral** + **TPDF/IGN Dithering** (opt-in, M1 hash korumalı) | `uniforms.rs`, `compositor.rs`, `motion_blur_accumulate.wgsl`, `project.rs` (isteğe bağlı `TonemapMode` şema alanı) |
| **(b) 4x MSAA + Post-AA + Mipmap** | `Scene3D` `sample_count: 1`, `Depth32Float` tek örnekli, tüm ara dokular `mip_level_count: 1`, `MipmapFilterMode::Nearest` | **4x MSAA** (`Rgba16Float` + `resolve_target`, `Depth32Float` + `texture_depth_multisampled_2d` DoF), **HDR-güvenli FXAA Quality**, **GPU Mipmap zinciri** (`MipmapFilterMode::Linear` + `textureSampleGrad`) | `context.rs`, `pool.rs`, `pipelines.rs`, `compositor.rs`, `fx_dof_coc.wgsl`, `transform.wgsl`, `transform3d.wgsl`, `corner_pin.wgsl`, `grid_warp.wgsl`, yeni `fx_aa.wgsl` + `mipmap_downsample.wgsl` |
| **(c) Additive Glow Koruması** | `if (a <= 1e-5) { return vec4(0.0); }` nedeniyle `alpha == 0 && rgb > 0` glow siliniyor | `a <= 1e-5 && max(rgb) <= 1e-5` birleşik eşiği + `eff_a` un-premultiply koruması | `motion_blur_accumulate.wgsl`, `color_basic.wgsl`, `lut3d.wgsl`, `fx_anime_sharpen.wgsl`, `fx_anime_impact_frame.wgsl`, `fx_film_optics.wgsl`, `blend.wgsl`, `fx_halftone_glow.wgsl` |
| **(d) `fx_film_optics.wgsl`** | 8-tap tek halka halation, piksel-başı beyaz grain, 3-tap CA, kenar karartan distortion | Çift ölçekli **ayrılabilir Gauss kırmızı-turuncu Halation**, 2-oktav **uzamsal korelasyonlu 35mm organik grain**, **7-tap spektral CA**, **auto-scale Brown-Conrady** | `graph.rs`, `project.rs`, `uniforms.rs`, `pipelines.rs`, `compositor.rs`, `fx_film_optics.wgsl`, yeni `fx_film_halation.wgsl` |
| **(e) `fx_bloom_upsample.wgsl`** | Eşit toplama (`base + upsampled`), izotropik 3x3 tent, tek renk `tint_rgb` | **Üstel scatter falloff** (`mix(base, up, scatter)`), **anamorfik yatay streak**, **mip-derinliğine bağlı kromatik bloom tint** | `graph.rs`, `project.rs`, `uniforms.rs`, `compositor.rs`, `fx_bloom_downsample.wgsl`, `fx_bloom_upsample.wgsl` |

---

## (a) AgX & Khronos PBR Neutral Tonemapper + TPDF/IGN Dithering ve M1 Hash Koruması

### 1. Birincil Kaynaklar ve Teknik Temel

1. **AgX (Troy Sobotka / Blender `AgXBaseRec2020` / Google Filament `ToneMapper.cpp` / Three.js `tonemapping_pars_fragment`):**
   - Blender AgX: [EaryChow/AgX_LUT_Gen — `AgXBaseRec2020.py`](https://github.com/EaryChow/AgX_LUT_Gen/blob/main/AgXBaseRec2020.py)
   - Google Filament (`ToneMapper.cpp`): [google/filament — `filament/src/ToneMapper.cpp`](https://github.com/google/filament/blob/main/filament/src/ToneMapper.cpp)
   - Benjamin Wrensch minimal analitik uyum (`iolite-engine.com`): [Minimal AgX Implementation](https://iolite-engine.com/blog_posts/minimal_agx_implementation)
   - Three.js shader chunk (`tonemapping_pars_fragment.glsl.js`): [mrdoob/three.js — `tonemapping_pars_fragment.glsl.js`](https://github.com/mrdoob/three.js/blob/dev/src/renderers/shaders/ShaderChunk/tonemapping_pars_fragment.glsl.js)
2. **Khronos PBR Neutral:**
   - Resmi spesifikasyon ve GLSL referansı: [KhronosGroup/ToneMapping — `PBR_Neutral/pbrNeutral.glsl`](https://github.com/KhronosGroup/ToneMapping/blob/main/PBR_Neutral/pbrNeutral.glsl)
3. **TPDF ve Interleaved Gradient Noise (IGN) Dithering:**
   - Jorge Jimenez (SIGGRAPH 2014): *Next Generation Post Processing in Call of Duty: Advanced Warfare* (IGN: `fract(52.9829189 * fract(dot(px, vec2(0.06711056, 0.00583715))))`).
   - Alex Vlachos (GDC 2015): *Advanced VR Rendering* (ekran uzayı RGB dithering).
   - Mikkel Gjøl (Playdead): [*Banding in Games: A Noisy Rant*](https://loopit.dk/banding_in_games.pdf) (TPDF `(r1 + r2 - 1.0) / 255.0` ve `0.0` / `1.0` sınır koruması).

### 2. Doğrulanmış WGSL Formülleri (`motion_blur_accumulate.wgsl`)

#### Matris Yönü Uyarısı (Sütun-Ana / Column-Major Doğrulama)
WGSL `mat3x3<f32>(col0, col1, col2)` kurucusu vektörleri **sütun** olarak alır. `AgXInsetMatrix` ve `AgXOutsetMatrix` içinde beyaz noktanın (`(1, 1, 1) -> (1, 1, 1)`) korunması için **satır toplamlarının `1.0` olması zorunludur**. Python/NumPy ile doğrulanan sütun-ana WGSL sabitleri (ayrıca Rec.709 ↔ Rec.2020 matrisleri [ocio.rs](../../crates/frameshift-render/src/ocio.rs#L116-L128) ile birebir tutarlıdır):

```wgsl
// Linear Rec.709 (D65) -> Linear Rec.2020 (D65), column-major
const M_REC709_TO_REC2020: mat3x3<f32> = mat3x3<f32>(
    vec3<f32>(0.62740390, 0.06909729, 0.01639144),
    vec3<f32>(0.32928304, 0.91954039, 0.08801331),
    vec3<f32>(0.04331306, 0.01136231, 0.89559525),
);

// Linear Rec.2020 (D65) -> Linear Rec.709 (D65), column-major
const M_REC2020_TO_REC709: mat3x3<f32> = mat3x3<f32>(
    vec3<f32>( 1.66049100, -0.12455047, -0.01815076),
    vec3<f32>(-0.58764114,  1.13289990, -0.10057890),
    vec3<f32>(-0.07284986, -0.00834942,  1.11872966),
);

// Blender / Filament AgX Inset Matrix (Rec.2020 primaries, column-major)
const AGX_INSET_MATRIX: mat3x3<f32> = mat3x3<f32>(
    vec3<f32>(0.8566271533, 0.1373189729, 0.1118982130),
    vec3<f32>(0.0951212405, 0.7612419906, 0.0767994186),
    vec3<f32>(0.0482516061, 0.1014390365, 0.8113023684),
);

// Blender / Filament AgX Outset Matrix = inverse(AgXOutsetMatrixInv), column-major
const AGX_OUTSET_MATRIX: mat3x3<f32> = mat3x3<f32>(
    vec3<f32>( 1.1271005818, -0.1413297635, -0.1413297635),
    vec3<f32>(-0.1106066431,  1.1578237022, -0.1106066431),
    vec3<f32>(-0.0164939387, -0.0164939387,  1.2519364066),
);

const AGX_MIN_EV: f32 = -12.47393;
const AGX_MAX_EV: f32 =  4.026069;

// Filament / iolite 7. derece AgX sigmoid polinomu (x=0'da >= 0 garantili)
fn agx_default_contrast_approx(x: vec3<f32>) -> vec3<f32> {
    let x2 = x * x;
    let x4 = x2 * x2;
    let x6 = x4 * x2;
    return -17.86 * x6 * x
        + 78.01 * x6
        - 126.7 * x4 * x
        + 92.06 * x4
        - 28.72 * x2 * x
        + 4.361 * x2
        - 0.1718 * x
        + vec3<f32>(0.002857);
}

fn tonemap_agx(x: vec3<f32>, punchy: bool) -> vec3<f32> {
    var val = M_REC709_TO_REC2020 * max(x, vec3<f32>(0.0));
    val = AGX_INSET_MATRIX * val;
    val = max(val, vec3<f32>(1e-10));
    val = log2(val);
    val = (val - AGX_MIN_EV) / (AGX_MAX_EV - AGX_MIN_EV);
    val = clamp(val, vec3<f32>(0.0), vec3<f32>(1.0));
    val = agx_default_contrast_approx(val);
    val = AGX_OUTSET_MATRIX * val;
    val = max(val, vec3<f32>(0.0));
    if (punchy) {
        // Blender AgX Punchy look: display-space power 1.35 + saturation 1.4
        let luma_weights = vec3<f32>(0.2126, 0.7152, 0.0722);
        let luma = dot(val, luma_weights);
        val = pow(val, vec3<f32>(1.35));
        val = max(mix(vec3<f32>(luma), val, 1.4), vec3<f32>(0.0));
    }
    val = pow(val, vec3<f32>(2.2));
    val = M_REC2020_TO_REC709 * val;
    return clamp(val, vec3<f32>(0.0), vec3<f32>(1.0));
}

// Khronos PBR Neutral (KhronosGroup/ToneMapping)
fn tonemap_pbr_neutral(color_in: vec3<f32>) -> vec3<f32> {
    let start_compression = 0.8 - 0.04; // 0.76
    let desaturation = 0.15;
    var color = max(color_in, vec3<f32>(0.0));

    let x = min(color.r, min(color.g, color.b));
    let offset = select(0.04, x - 6.25 * x * x, x < 0.08);
    color = color - vec3<f32>(offset);

    let peak = max(color.r, max(color.g, color.b));
    if (peak < start_compression) {
        return clamp(color, vec3<f32>(0.0), vec3<f32>(1.0));
    }
    let d = 1.0 - start_compression;
    let new_peak = 1.0 - (d * d) / (peak + d - start_compression);
    color = color * (new_peak / peak);

    let g = 1.0 - 1.0 / (desaturation * (peak - new_peak) + 1.0);
    return clamp(mix(color, vec3<f32>(new_peak), g), vec3<f32>(0.0), vec3<f32>(1.0));
}
```

#### `TonemapMode` Genişletmesi (`uniforms.rs`)
Mevcut `TonemapMode` değerleri (`None = 0`, `Reinhard = 1`, `AcesLite = 2`) korunur; yenileri eklenir:
- `Agx = 3` (AgX Base — yüksek doygunluklu anime/neon sahnelerde Abney renk kaymasını önler, parlak mavi/macenta/kırmızıyı doğal beyaza yakınsar)
- `PbrNeutral = 4` (Khronos PBR Neutral — marka/UI renklerini `<= 0.76` altında 1:1 korur)
- `AgxPunchy = 5` (isteğe bağlı yüksek kontrastlı AMV/film görünümü)

### 3. `Rgba8Unorm` Çıkışında TPDF/IGN Dithering ve M1 Golden Hash Koruma Stratejisi

- **Uygulama Noktası:** Yalnızca `linear_to_srgb(rgb)` dönüşümünden **hemen sonra**, `Rgba8Unorm` çıktısına yazılmadan önce (`motion_blur_accumulate.wgsl :: evaluate_tonemap_to_srgb`).
- **Formül (Skaler Luma TPDF + IGN + Siyah/Beyaz Sınır Koruması):**
  ```wgsl
  fn ign(pixel_xy: vec2<f32>) -> f32 {
      return fract(52.9829189 * fract(dot(pixel_xy, vec2<f32>(0.06711056, 0.00583715))));
  }

  fn dither_srgb_tpdf(srgb: vec3<f32>, frag_xy: vec2<f32>) -> vec3<f32> {
      let u1 = ign(frag_xy);
      let u2 = ign(frag_xy + vec2<f32>(17.0, 59.0));
      let tpdf = (u1 + u2 - 1.0) / 255.0; // [-1/255, +1/255] üçgen dağılım
      // Saf siyah (0.0) ve saf beyaz (1.0) asimetrik kırpmaya uğramaz (Gjøl, Banding in Games)
      let boundary_guard = smoothstep(0.0, 1.0 / 255.0, srgb)
                         * smoothstep(1.0, 254.0 / 255.0, srgb);
      return clamp(srgb + vec3<f32>(tpdf) * boundary_guard, vec3<f32>(0.0), vec3<f32>(1.0));
  }
  ```
- **Uniform Yerleşimi (Mevcut `32` Byte Sabit Kalır!):**
  [uniforms.rs](../../crates/frameshift-render/src/uniforms.rs#L216-L221) içindeki `MotionBlurUniform` (`32` byte) zaten 2 adet kullanılmayan `reserved` alana sahiptir:
  - `weights: [f32; 4]` → `[weight, accum_scale, exposure, dither_strength]` (`0.0` = kapalı, `1.0` = standart `±1/255`)
  - `modes: [u32; 4]` → `[pass_mode, tonemap_mode, output_alpha_mode, dither_mode]` (`0u = Off`, `1u = TpdfIgn`)
  - `std::mem::size_of::<MotionBlurUniform>() == 32` **değişmez**, dolayısıyla [compositor_tests.rs](../../crates/frameshift-render/tests/compositor_tests.rs#L210) testi bozulmaz.
- **M1 Golden Hash Koruma Stratejisi:**
  1. `RenderOptions` içine `pub dither: bool` (veya `dither_mode: DitherMode`, `#[default] Off`) alanı eklenir; `RenderOptions::default()` içinde **`false` / `DitherMode::Off`** kalır.
  2. `verify_m1_golden_scene` ([engine.rs](../../crates/frameshift-core/src/engine.rs#L277-L280)) ve mevcut tüm `compositor_tests.rs` / `faz6_*_tests.rs` çağrıları `..Default::default()` kullandığından `tonemap_mode` otomatik `AcesLite` (veya `None`) ve `dither_mode == Off` olarak kalır → **M1 golden kare hash'leri bit düzeyinde %100 korunur**.
  3. Yeni editler/profiller için `RenderOptions { dither: true, tonemap_mode: Some(TonemapMode::Agx), .. }` veya `Project::render_profiles` üzerinde `#[serde(default)]` `dither: bool` / `tonemap_mode: Option<...>` üzerinden opt-in (veya CLI `--tonemap agx --dither`) açılır. Not: `dither_srgb_tpdf` yalnızca `in.position.xy` piksel koordinatına bağlı olduğundan `frame --tick T` ile sıralı render yine %100 seek-independent ve deterministiktir.

---

## (b) 3D Sahnelerde 4x MSAA, Tam Ekran Post-AA ve Ara Dokularda Mipmap Üretimi

### 1. `wgpu 30.0.1` Format Yetenekleri (Yerel Kaynak Doğrulaması)

Yerel `wgpu-types-30.0.1` (`src/texture/format.rs` satır `991` ve `1000`) incelendiğinde `Features::empty()` altında çekirdek WebGPU garantileri şunlardır:
- **`wgpu::TextureFormat::Rgba16Float`:** `(msaa_resolve | s_ro_wo, all_flags)` → `MULTISAMPLE_X4`, `MULTISAMPLE_RESOLVE`, `FILTERABLE`, `BLENDABLE`, `RENDER_ATTACHMENT`, `TEXTURE_BINDING` **garantilidir**.
- **`wgpu::TextureFormat::Depth32Float`:** `(msaa, attachment)` → `MULTISAMPLE_X4` ve `RENDER_ATTACHMENT | TEXTURE_BINDING` **garantilidir**, ancak **`MULTISAMPLE_RESOLVE` YOKTUR** (WebGPU spesifikasyonu derinlik ekleri için `resolve_target` desteklemez).

### 2. `Scene3D`, `Text3D` ve `Shape3D` İçin 4x MSAA Tasarımı

Mevcut [pipelines.rs](../../crates/frameshift-render/src/pipelines.rs#L1052-L1063) ve [compositor.rs](../../crates/frameshift-render/src/compositor.rs#L3203-L3388) yapısında `Scene3D` tek örnekli (`sample_count: 1`) `color_tex` ve `depth_cache` kullanır.

- **Donanım Yetenek Kontrolü (Güvenli Fallback):**
  `GpuContext` başlatılırken `device.get_texture_format_features(WORKING_TEXTURE_FORMAT)` ve `Depth32Float` sorgulanır:
  `supports_msaa_4x = color_flags.contains(MULTISAMPLE_X4 | MULTISAMPLE_RESOLVE) && depth_flags.contains(MULTISAMPLE_X4)`.
  Gerçek GPU'larda `true`, olası kısıtlı yazılımsal adaptörlerde otomatik `sample_count = 1` fallback uygulanır.
- **Render Pass ve Cache Yapısı (`compositor.rs`):**
  - `msaa_color_cache: HashMap<(u32, u32), (wgpu::Texture, wgpu::TextureView)>` (`Rgba16Float`, `sample_count: 4`, `RENDER_ATTACHMENT`).
  - `msaa_depth_cache: HashMap<(u32, u32), (wgpu::Texture, wgpu::TextureView)>` (`Depth32Float`, `sample_count: 4`, `RENDER_ATTACHMENT | TEXTURE_BINDING`).
  - `Scene3D` renk eki: `view: &msaa_color_view`, `resolve_target: Some(&color_tex.view)`, `ops: { load: Clear(bg), store: StoreOp::Discard }` — donanım render pass sonunda `Rgba16Float` `color_tex`'e (`sample_count: 1`) **ek kopya geçişi olmadan** otomatik linear resolve yapar (premultiplied alpha uzayında matematiksel olarak tam doğrudur).
  - **Kritik Derinlik / DoF Çözümü (`fx_dof_coc.wgsl`):**
    `Depth32Float` üzerinde `resolve_target` olmadığından `fx_dof_coc.wgsl` içine ikinci bir giriş noktası (`fs_main_msaa`) veya ayrı `fx_dof_coc_msaa` pipeline'ı eklenir:
    ```wgsl
    @group(0) @binding(1) var depth_tex_msaa: texture_depth_multisampled_2d;

    fn load_min_depth_msaa(c: vec2<i32>) -> f32 {
        let d0 = textureLoad(depth_tex_msaa, c, 0);
        let d1 = textureLoad(depth_tex_msaa, c, 1);
        let d2 = textureLoad(depth_tex_msaa, c, 2);
        let d3 = textureLoad(depth_tex_msaa, c, 3);
        return min(min(d0, d1), min(d2, d3));
    }
    ```
    4 örnek arasından `min` (en yakın ön plan derinliği) seçmek, bokeh CoC kenarlarında arka plan sızıntısını önler ve ekstra depth-resolve pass'i gerektirmez.
  - **Not:** `TextSlug` (`text_slug.wgsl`) zaten piksel başına analitik winding anti-aliasing yaptığı için MSAA dışı bırakılır (4x fragment maliyetinden tasarruf edilir). Standalone `Text3D` / `Shape3D` (`record_mesh3d_pass`) aynı `msaa_color_cache` + `msaa_depth_cache` + `resolve_target` mekanizmasını paylaşır; `Camera3DDef` / `RenderOptions` üzerinde `#[serde(default = "default_msaa_samples")] msaa_samples: u32` (4 veya 1) ile kontrol edilir.

### 3. Tam Ekran Post-AA: FXAA Quality (HDR-Güvenli) vs SMAA 1x

| Kriter | Clean-Room HDR-Güvenli FXAA Quality | SMAA 1x (Jimenez et al., MIT) |
| :--- | :--- | :--- |
| **Pass Sayısı** | **1 pass** (`fx_aa.wgsl`) | 3 pass (Edge -> Weight -> Neighborhood) |
| **Harici Doku / LUT** | **0** | 2 gömülü LUT (`AreaTex` + `SearchTex`, ~180 KB) |
| **Lisans** | Saf WGSL clean-room (0 yeni crate/lisans riski) | MIT (`iryoku/smaa`) |
| **Kullanım Alanı** | `CornerPin`, `GridWarp`, `AnimeImpactFrame`, `VectorPathHud`, MSAA kapalı 3D sahne kenarları | Çok keskin 1px piksel art / ince HUD çizgileri |

- **Öneri:** Önce **1-pass HDR-güvenli FXAA Quality** (`fx_aa.wgsl`) eklenmesi (0 LUT, hızlı, `0.06..0.125` kontrast eşiği).
- **HDR Güvenlik Kritik Noktası:** Doğrusal `Rgba16Float` uzayında `rgb > 1.0` değerleri doğrudan luma farkına girerse parlak neon kenarlar aşırı bulanıklaşır. Kenar algılama luması mutlaka tonemap-sıkıştırılmış algısal lumadan hesaplanmalıdır:
  ```wgsl
  fn perceptual_aa_luma(rgb: vec3<f32>) -> f32 {
      let y = max(dot(max(rgb, vec3<f32>(0.0)), vec3<f32>(0.2126, 0.7152, 0.0722)), 0.0);
      return sqrt(y / (1.0 + y)); // [0, 1) aralığında HDR-güvenli algısal luma
  }
  ```
- **Entegrasyon:** `NodeKind::AntiAliasing` (opsiyonel düğüm) veya `RenderOptions::post_aa` (`Off` varsayılan → M1 golden hash korunur).

### 4. Ara ve Kaynak Dokularda GPU Mipmap Üretimi

- **Mevcut Eksiklik:**
  1. [pool.rs](../../crates/frameshift-render/src/pool.rs#L52) `mip_level_count: 1`.
  2. [context.rs](../../crates/frameshift-render/src/context.rs#L95) `linear_sampler` içinde `mipmap_filter: wgpu::MipmapFilterMode::Nearest`.
  3. `transform.wgsl`, `transform3d.wgsl`, `corner_pin.wgsl`, `grid_warp.wgsl` daima `textureSampleLevel(..., 0.0)` kullanıyor; %50 altı küçültmelerde ve eğik 3D perspektif kartlarda (`Transform3D`) shimmer/moiré oluşuyor.
- **Çözüm Mimarisi (`0` yeni crate):**
  1. **`trilinear_sampler` (veya `linear_sampler` güncellemesi):** `mipmap_filter: wgpu::MipmapFilterMode::Linear`. (Mevcut `mip_level_count == 1` dokularda ve `textureSampleLevel(..., 0.0)` çağrılarında çıktıyı **hiç değiştirmez**, dolayısıyla M1 hash'leri etkilenmez).
  2. **Mipmap Zinciri Üreticisi (`mipmap_downsample.wgsl`):**
     `mip_levels = 32 - max(w, h).leading_zeros()`. Her mip `k -> k+1` adımı için `TextureViewDescriptor { base_mip_level: Some(k), mip_level_count: Some(1), .. }` ile 4-tap / bilinear kutu downsample (`textureSampleLevel(src_mip, linear_sampler, uv, 0.0)`). `Rgba16Float` her mip'i `RENDER_ATTACHMENT` olarak destekler.
  3. **`TexturePool` ve `image_gpu_cache`:**
     - `image_gpu_cache` (`SourceImage`) yüklenirken mip zinciri tek seferlik üretilir.
     - `TexturePool` anahtarı `(width, height, mip_level_count)` yapılır (`acquire(device, w, h)` varsayılan `1`, `acquire_mipmapped(device, w, h)` tam zincir).
  4. **Shader Örneklemesi (`textureSampleGrad` Kuralı):**
     `transform.wgsl` satır `131` gibi sınırların clamp edildiği shader'larda `textureSample(..., clamped_uv)` kenar piksellerinde türevi bozar; bunun yerine **clamp edilmemiş** `src_uv`'nin ekran türevleri (`dpdx(src_uv)`, `dpdy(src_uv)`) alınıp `textureSampleGrad(t_source, s_trilinear, clamped_uv, dpdx_uv, dpdy_uv)` çağrılmalıdır. Böylece hem non-uniform dallarda WGSL doğrulamasından geçer hem de kenar mip seçimi doğru kalır!

---

## (c) Premultiplied `alpha <= 1e-5 && max(rgb) <= 1e-5` Additive Glow Koruması

### 1. Tespit Edilen Kırılma Noktaları (Kod Taraması)

`crates/frameshift-render/shaders/` içinde yapılan taramada şu dosya ve satırlarda yalnızca `alpha <= ALPHA_EPS` kontrolüyle saf additive glow enerjisinin (`rgb > 0, a ≈ 0`) yok edildiği veya `rgb / a` ile patladığı doğrulanmıştır:

| Shader | Satır | Mevcut Hatalı Davranış | Etki |
| :--- | :--- | :--- | :--- |
| [motion_blur_accumulate.wgsl](../../crates/frameshift-render/shaders/motion_blur_accumulate.wgsl#L79-L84) | L79–L84 | `if (a <= ALPHA_EPS) { return vec4(0.0); }` + `col.rgb / a` | `DeepGlow` / `HalftoneGlow` şeffaf arka plan üzerindeki haleyi çıkışta sıfırlar! |
| [color_basic.wgsl](../../crates/frameshift-render/shaders/color_basic.wgsl#L81-L85) | L81–L85 | `if (src.a <= ALPHA_EPS) { return vec4(0.0); }` + `src.rgb / src.a` | Glow üstüne exposure/saturation uygulanınca hale silinir |
| [lut3d.wgsl](../../crates/frameshift-render/shaders/lut3d.wgsl#L45-L49) | L45–L49 | `if (src.a <= ALPHA_EPS) { return vec4(0.0); }` + `src.rgb / src.a` | `Lut3D` altındaki şeffaf glow tamamen kaybolur |
| [fx_anime_sharpen.wgsl](../../crates/frameshift-render/shaders/fx_anime_sharpen.wgsl#L46-L62) | L46, L60 | `sample_unpremul` ve `fs_main` `a <= ALPHA_EPS` ile sıfırlar | Glow alanları kesilir |
| [fx_anime_impact_frame.wgsl](../../crates/frameshift-render/shaders/fx_anime_impact_frame.wgsl#L48-L62) | L48, L58 | `sample_luma` ve `fs_main` `a <= ALPHA_EPS` ile sıfırlar | Glow pikselleri etki dışı kalır |
| [fx_film_optics.wgsl](../../crates/frameshift-render/shaders/fx_film_optics.wgsl#L115-L125) | L115 | `out_a > ALPHA_EPS` guard'ı yüzünden `a == 0` halation üstüne grain uygulanmaz | Küçük tutarsızlık |
| [blend.wgsl](../../crates/frameshift-render/shaders/blend.wgsl#L43-L46) | L43–L46 | `safe_unpremultiply` `c.a <= ALPHA_EPS` için `vec3(0.0)` döner | `Multiply` / `Overlay` sırasında additive glow kaybı |

*(Not: `fx_bloom_upsample.wgsl` L82 `vec4(out_rgb, base.a)` ve `fx_halftone_glow.wgsl` L119-120 `vec4(base_premul + glow, src.a)` bilerek `a = 0` üzerinde pozitif `rgb` üretir — bu doğru HDR additive davranışıdır, ancak yukarıdaki downstream shader'lar bu enerjiyi silmektedir.)*

### 2. Matematiksel Çözüm ve Ortak WGSL Kalıbı

`Rgba16Float` premultiplied uzayda bir piksel yalnızca **hem `alpha` hem de `max(rgb)`** eşik altındaysa boştur:

```wgsl
const ALPHA_EPS: f32 = 1e-5;

fn max3(v: vec3<f32>) -> f32 {
    return max(v.r, max(v.g, v.b));
}
```

1. **Boş Piksel Kontrolü (Tüm Shader'lar):**
   ```wgsl
   let a = clamp(src.a, 0.0, 1.0);
   let max_rgb = max3(max(src.rgb, vec3<f32>(0.0)));
   if (a <= ALPHA_EPS && max_rgb <= ALPHA_EPS) {
       return vec4<f32>(0.0);
   }
   ```
2. **Güvenli Un-Premultiply / Re-Premultiply (Katman + Saf Additive Ayrımı):**
   - `a <= ALPHA_EPS && max_rgb > ALPHA_EPS` durumunda piksel **saf additive emisyon**dur (`eff_a = 1.0` gibi un-premultiply edilir, çıkışta `src.a` yani `0.0` korunur, **asla `rgb * a` ile sıfırlanmaz**):
     ```wgsl
     let is_pure_additive = a <= ALPHA_EPS;
     let denom = select(a, 1.0, is_pure_additive);
     var rgb = max(src.rgb / denom, vec3<f32>(0.0));
     // ... işlem (tonemap / LUT / color_basic / sharpen) ...
     let out_rgb = select(rgb * a, rgb, is_pure_additive);
     return vec4<f32>(out_rgb, a);
     ```
   - **`color_basic.wgsl` Özel Notu:** `contrast` işlemi `0.18` orta gri etrafında pivot yaptığı için `is_pure_additive` piksellerde (`a <= ALPHA_EPS`) siyah tabanı kaldırmaması adına `contrast` sadece `a > ALPHA_EPS` veya `rgb` ölçekli uygulanır (`exposure`, `saturation` ve `flash` doğrudan çalışır).
   - **`lut3d.wgsl` Özel Notu:** Bazı sinematik LUT'lar `(0,0,0)` girişini hafif siyah kalkışına (`0.02`) taşır. `is_pure_additive` piksellerde bu kalkışı önlemek için `black_floor = textureSampleLevel(t_lut, s_linear, vec3(half_texel), 0.0).rgb` çıkarılabilir veya `min(1.0, max_rgb * 100.0)` yumuşak emisyon maskesiyle ölçeklenir; `a <= 1e-5 && max_rgb <= 1e-5` zaten erken dönüş yaptığı için tamamen boş bölgeler sıfır kalır.
   - **`motion_blur_accumulate.wgsl` (`OutputAlphaMode`) Notu:**
     - `OutputAlphaMode::Premultiplied` (`modes.z == 0u`): `vec4<f32>(srgb, 0.0)` doğrudan additive glow enerjisini korur.
     - `OutputAlphaMode::Straight` (`modes.z == 1u`): MP4 kodlayıcı (`yuv420p`) zaten alfa kanalını düşürdüğünden `srgb` doğrudan korunur (`vec4<f32>(srgb, a)` — RGB kanalları `a == 0` olsa bile `srgb` taşır!).
   - **M1 Golden Hash Etkisi:** M1 sahnelerinde şeffaf piksel bölgeleri `rgb == [0,0,0], a == 0` olduğu için `a <= 1e-5 && max_rgb <= 1e-5` dalına girer ve yine `vec4(0.0)` döner → **M1 hash'lerine sıfır etki**.

---

## (d) `fx_film_optics.wgsl` İyileştirmeleri

### 1. Gerçek Kırmızı-Turuncu Halation (Çift Ölçekli Ayrılabilir Gauss + Emülsiyon Fiziği)
- **Fiziksel Temel ([Dehancer](https://www.dehancer.com/learn/article/halation-and-how-to-simulate-it), [halation.page](https://halation.page/)):** Renkli film emülsiyonunda üstten alta **Mavi → Yeşil → Kırmızı** katmanları dizilidir. Aşırı parlak ışık arka tabandan yansıdığında önce en alttaki **kırmızı** tabakayı, çok yüksek şiddette ise **yeşil** tabakayı da uyararak kırmızı-turuncu-kehribar hale oluşturur (`R >> G > B`).
- **Mevcut Sorun:** [fx_film_optics.wgsl](../../crates/frameshift-render/shaders/fx_film_optics.wgsl#L89-L111) yalnızca sabit `3.5 * texel` mesafesinde 8 tekil piksel örnekleyip noktasal hayalet üretiyor.
- **Önerilen 2-Ölçekli Ayrılabilir Bilinear Gauss Hattı (`fx_film_halation.wgsl`):**
  Yalnızca `halation_amount > 1e-5` olduğunda çalışır (`halation_amount <= 1e-5` iken `dummy_zero_view` bağlanarak tek geçişte kalır):
  1. **Pass 0 (`fx_film_optics_lens` veya birleşik):** Distortion + 7-tap Spektral CA uygulanır (`lens_k1==0 && lens_k2==0 && chromatic_edge==0` ise atlanır).
  2. **Pass 1 (`fx_film_halation.wgsl`, `mode=0u` Extract & Downsample 1/2):** `soft_knee` parlaklık eşiği (`threshold = 0.65`, `knee = 0.25`) + kırmızı/yeşil emülsiyon ağırlıklandırması:
     - Çekirdek rengi: parlaklığa göre `mix(vec3(1.0, 0.22, 0.04), vec3(1.0, 0.45, 0.10), clamp((luma - 1.5) * 0.4, 0.0, 1.0))`.
     - `W/2 x H/2` tampona yazılır, ayrıca `W/4 x H/4` kopyası downsample edilir.
  3. **Pass 2 & 3 (`mode=1u` Yatay, `mode=2u` Dikey Ayrılabilir Bilinear Gauss):**
     GPU donanım bilinear ara örneklemeli 9-tap Gauss (yalnızca **5 doku okuması** / geçiş):
     `offsets = [0.0, 1.38461538, 3.23076923]`, `weights = [0.22702703, 0.31621622, 0.07027027]`.
     `W/2` (sıkı çekirdek hale) ve `W/4` (geniş üstel saçılma) ayrı ayrı bulanıklaştırılıp `0.45 * tight + 0.55 * wide` oranında birleştirilir (toplam maliyet: küçük ara dokularda `< 0.15 ms`).
  4. **Final Composite (`fx_film_optics.wgsl`):** `out_rgb = out_rgb + halation_sample * halation_amt * 0.65` (additive enerji korunumlu).

### 2. Uzamsal Korelasyonlu 35mm Organik Film Grain
- **Mevcut Sorun:** Piksel başına bağımsız `hash_unit(px)` dijital karıncalanma gibi görünür.
- **Önerilen Yöntem (2-Oktav Yumuşak Kafes Gürültüsü + Renk/Luma Dengesi + D-logE Ağırlığı):**
  - Harici gürültü dokusu gerektirmeden (`0` ek binding) sürekli 2D değer gürültüsü (`value_noise_2d(coord)`) quintic Hermite eğrisiyle (`u*u*u*(u*(u*6.0 - 15.0) + 10.0)`) hesaplanır:
    - Temel koordinat: `p_grain = in.position.xy / max(grain_size_px, 1.0)` (varsayılan `grain_size_px = 1.65`, 35mm kristal topaklanması).
    - 2 oktav fBm: `n_luma = 0.68 * noise(p_grain) + 0.32 * noise(p_grain * 2.13 + vec2(31.7, 17.3))`, aralık `[-1, 1]` (sıfır ortalama).
    - Hafif renkli gümüş halojenür varyasyonu (`grain_chroma`, varsayılan `0.25`): `r, g, b` kanalları için küçük faz kaydırmalı ikinci oktav (`mix(vec3(n_luma), n_rgb, grain_chroma)`).
  - **Film Tepki Eğrisi:** Gölge-orta tonlarda maksimum (`smoothstep(0.01, 0.12, luma) * (1.0 - 0.65 * smoothstep(0.55, 1.8, luma))`); `out_rgb = max(out_rgb + grain_delta * (0.12 + 0.35 * out_rgb), vec3(0.0))` (hem gölge liftini önler hem de parlak alanlarda organik tanecik verir).

### 3. 6–8 Tap Sürekli Spektral Kromatik Aberrasyon (CA)
- **Mevcut Sorun:** 3 sert RGB örneği yüksek kontrastlı anime çizgilerinde ayrık kırmızı/mavi gölgeler bırakıyor.
- **7-Tap Enerji Korunumlumlu Spektral Entegrasyon:**
  `chromatic_edge <= 1e-5` ise **tek örnekleme** fast-path korunur. Aksi halde `i = 0..6` için `t = (f32(i) - 3.0) / 3.0` (`-1.0 .. +1.0`) boyunca `uv_dist + ca_shift * t` örneklenir; her kanalın ağırlık toplamı tam `1.00` olacak şekilde:
  - `W = [ (0.42,0.00,0.00), (0.36,0.12,0.00), (0.18,0.30,0.02), (0.04,0.38,0.08), (0.00,0.16,0.30), (0.00,0.04,0.38), (0.00,0.00,0.22) ]`
  - Toplamlar: `ΣR = 1.00`, `ΣG = 1.00`, `ΣB = 1.00` (sıfır renk kayması).

### 4. Auto-Scale Brown-Conrady Lens Distortion
- **Matematiksel Türetim:**
  Normalize merkezli koordinatta `p = vec2((uv.x - 0.5) * aspect, uv.y - 0.5)`.
  Çerçeve kenar ve köşelerinin merkezden uzaklık kareleri:
  `r2_edge_x = 0.25 * aspect * aspect`, `r2_edge_y = 0.25`, `r2_corner = r2_edge_x + r2_edge_y`.
  Distortion çarpanı `s(r2) = 1.0 + k1 * r2 + k2 * r2 * r2`.
  - Pincushion (`k1 > 0` veya köşe `s > 1.0`) durumunda `uv_dist` `[0, 1]` dışına çıkıp siyah kenar bırakır.
  - `auto_scale` açıkken (`auto_scale = true`):
    `s_bound = max(max(s(r2_edge_x), s(r2_edge_y)), s(r2_corner))`
    `zoom_scale = clamp(select(max(s_bound, 1.0), s_bound, k1 < 0.0), 0.35, 4.0)` (veya `p_dist = (p * s(r2)) / max(s_bound, 1.0)` ile `k1 > 0` siyah kenarları tamamen yok edilir; `k1 < 0` barrel ise tam çerçeve içine oturtulmak istenirse `/ s_bound`, mevcut yakınlaştırmayı korumak için `/ max(s_bound, 1.0)` seçilir — **önerilen:** `max(s_bound, 1.0)` siyah kenar garantili ve barrel `k1 < 0` kenar boşluğu üretmez!).
    Ek güvenlik: `ca_shift` için maksimum sapma köşelerde `s_bound`'a dahil edilip `zoom_scale *= (1.0 + abs(chromatic_edge) * 1.5)` ile CA kaynaklı kenar siyahlaşması da önlenir.

### 5. `FilmOpticsUniform` Düzeni (`48` Byte, 16-Byte Hizalı)

```rust
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct FilmOpticsUniform {
    pub lens_k1: f32,            // 0..4
    pub lens_k2: f32,            // 4..8
    pub chromatic_edge: f32,     // 8..12
    pub grain_amount: f32,       // 12..16
    pub grain_seed: f32,         // 16..20
    pub halation_amount: f32,    // 20..24
    pub vignette_amount: f32,    // 24..28
    pub aspect: f32,             // 28..32
    pub grain_size_px: f32,      // 32..36 (default 1.65)
    pub grain_chroma: f32,       // 36..40 (default 0.25)
    pub halation_threshold: f32, // 40..44 (default 0.65)
    pub auto_scale_flags: u32,   // 44..48 (bit 0: auto_scale)
} // Toplam: 48 byte (48 % 16 == 0)
```
`NodeKind::FilmOptics` içine eklenecek yeni şema alanları (`auto_scale`, `grain_size_px`, `grain_chroma`, `halation_threshold`) tümü `#[serde(default = ...)]` ile geriye dönük uyumlu tutulacaktır.

---

## (e) `fx_bloom_upsample.wgsl` İyileştirmeleri (`DeepGlow`)

### 1. Üstel Scatter Falloff (Unity HDRP/URP Enerji Kontrollü Mip Karışımı)
- **Mevcut Sorun:** [fx_bloom_upsample.wgsl](../../crates/frameshift-render/shaders/fx_bloom_upsample.wgsl#L85) ara basamaklarda `base.rgb + upsampled` yaparak tüm mip seviyelerini eşit (`1.0`) topluyor. Bu durum `mip_levels` arttıkça parlamanın kontrolsüz doygunlaşmasına ve lokal çekirdeğin çamurlaşmasına yol açıyor.
- **Çözüm Formülü:**
  Ara upsample geçişlerinde (`is_final_composite < 0.5`) Unity HDRP/URP `Scatter` formülü:
  ```wgsl
  let scatter = clamp(params.scatter, 0.0, 0.98);
  let blended = mix(max(base.rgb, vec3<f32>(0.0)), upsampled, scatter);
  return vec4<f32>(blended, base.a);
  ```
  Matematiksel olarak $L$ mip seviyesi için her mip'in nihai ağırlığı $w_0 = (1-s),\, w_k = s^k (1-s),\, w_{L-1} = s^{L-1}$ biçiminde **üstel azalır** ve toplam kazanç $\sum w_k = 1.0$ olarak korunur (`scatter = 0.65` varsayılan değerinde sıkı çekirdek + uzun yumuşak hale dengesi sağlar). Final kompozitte (`is_final_composite > 0.5`) ise `base.rgb + upsampled * tint * intensity` ile fiziksel additive ekleme yapılır.

### 2. Anamorfik Yatay Streak (`anamorphic_ratio`)
- **Matematik:**
  `anamorphic_ratio` `[0.0, 1.0]` parametresi (`0.0` = klasik izotropik 3x3 tent, `1.0` = geniş sinematik yatay lens streak):
  ```wgsl
  let anamorphic = clamp(params.anamorphic_ratio, 0.0, 1.0);
  let step_x = texel.x * radius * (1.0 + anamorphic * 4.0);
  let step_y = texel.y * radius * max(1.0 - anamorphic * 0.80, 0.12);
  ```
- **Çizgili Optik Streak (Bantlaşma Önleme):** Yüksek `anamorphic_ratio > 0.05` değerlerinde sadece 3 sütunlu tent kullanmak 3 ayrı nokta izi bırakabileceğinden, yatay eksende ek 4 genişletilmiş tap (`±2 * step_x`, `±3.5 * step_x`) ağırlıklı karıştırılır (`mix(tent9, streak_h, anamorphic * 0.75)`). Ek olarak `fx_bloom_downsample.wgsl` ilk birkaç geçişte `step_x` ölçeğini hafif genişletirse çok uzun yatay anamorfik parlamalar pürüzsüzleşir.

### 3. Kromatik Bloom Tint (`chromatic_shift`)
- **Mekanizma:** Sadece tek bir `tint_rgb` yerine mip piramidinin ince çekirdek katmanından (`mip_norm = 0.0`) dış geniş hale katmanına (`mip_norm = 1.0`) doğru spektral kayma uygulanır:
  ```wgsl
  let shift = clamp(params.chromatic_shift, -1.0, 1.0);
  let chroma_outer = vec3<f32>(
      1.0 + shift * 0.45,
      1.0 - abs(shift) * 0.10,
      1.0 - shift * 0.45,
  );
  let mip_tint = mix(vec3<f32>(1.0), max(chroma_outer, vec3<f32>(0.05)), params.mip_norm);
  // Ara upsample adımında veya son kompozitte:
  let upsampled_tinted = upsampled * mip_tint;
  ```
  - `chromatic_shift > 0`: İç çekirdek beyaz/nötr, dış hale sıcak kırmızı-amber.
  - `chromatic_shift < 0`: İç çekirdek nötr, dış hale soğuk siyan-mavi (anime/sci-fi neon estetiği).
  - `chromatic_shift == 0`: `mip_tint == (1, 1, 1)` → mevcut davranışla uyumlu.

### 4. `BloomPassUniform` Düzeni (`64` Byte, 16-Byte Hizalı)

```rust
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct BloomPassUniform {
    pub texel_size: [f32; 2],    //  0..8
    pub threshold: f32,          //  8..12
    pub knee: f32,               // 12..16
    pub intensity: f32,          // 16..20
    pub radius: f32,             // 20..24
    pub is_first_pass: f32,      // 24..28
    pub is_final_composite: f32, // 28..32
    pub tint_rgb: [f32; 3],      // 32..44
    pub scatter: f32,            // 44..48 (default 0.65)
    pub anamorphic_ratio: f32,   // 48..52 (default 0.0)
    pub chromatic_shift: f32,    // 52..56 (default 0.0)
    pub mip_norm: f32,           // 56..60 (0.0 = finest .. 1.0 = coarsest)
    pub _pad: f32,               // 60..64
} // Toplam: 64 byte (64 % 16 == 0)
```
*(Not: [uniforms.rs](../../crates/frameshift-render/src/uniforms.rs#L851-L853) içindeki boyut assertion testleri Faz 2'de `BloomPassUniform == 64` ve `FilmOpticsUniform == 48` olarak güncellenecektir.)*

---

## Faz 2 Uygulama Sırası Önerisi

1. **Adım 1 — Additive Glow & Tonemap + Dither (`c` + `a`):**
   - `uniforms.rs` (`TonemapMode::{Agx, PbrNeutral, AgxPunchy}`, `DitherMode`, `MotionBlurUniform` son slotları `32` byte korunarak).
   - 8 WGSL shader'da `a <= 1e-5 && max_rgb <= 1e-5` ve `is_pure_additive` düzeltmesi.
   - `motion_blur_accumulate.wgsl` içine `tonemap_agx`, `tonemap_pbr_neutral`, `dither_srgb_tpdf`.
   - Doğrulama: M1 golden hash kontrolü + şeffaf `DeepGlow` okuma testi.
2. **Adım 2 — `DeepGlow` ve `FilmOptics` (`e` + `d`):**
   - `graph.rs` (`#[serde(default)]` alanlar), `project.rs` (`bindable_param_names`), `uniforms.rs` (`BloomPassUniform = 64B`, `FilmOpticsUniform = 48B`).
   - `fx_bloom_upsample.wgsl` / `fx_bloom_downsample.wgsl`, `fx_film_halation.wgsl`, `fx_film_optics.wgsl`, `compositor.rs` ve `pipelines.rs`.
3. **Adım 3 — 4x MSAA + Mipmap + FXAA (`b`):**
   - `context.rs` (`MipmapFilterMode::Linear` + format feature cache), `pool.rs` (`(w, h, mip_levels)`).
   - `mipmap_downsample.wgsl`, `transform.wgsl` (`textureSampleGrad`), `fx_dof_coc.wgsl` (`fs_main_msaa`), `pipelines.rs` & `compositor.rs` `Scene3D` 4x MSAA + `fx_aa.wgsl`.
4. **Tam Kapı Doğrulaması:** `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `verify-all` ve görsel inceleme paketi.
