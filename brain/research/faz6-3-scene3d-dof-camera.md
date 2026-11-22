---
title: "Faz 6.3 Part A: Shared Scene3D, Unified Camera3D, Depth32Float Post-Process Bokeh DoF & SafeExpr Kinetic Easing Şartnamesi"
date: 2026-11-22
status: active
tags: [faz6-3, scene3d, camera3d, depth32float, bokeh-dof, wgsl, wgpu, safe-expr]
---

# Faz 6.3 Part A: Shared `Scene3D`, Unified `Camera3D`, `Depth32Float` Post-Process Bokeh DoF ve `SafeExpr` Kinetik Easing Teknik Şartnamesi

## Soru / Amaç

FrameShift Faz 6.2 sonrasında motor; ayrı çalışan [`Text3D`](../../crates/frameshift-render/shaders/text3d_mesh.wgsl), [`ParticleField3D`](../../crates/frameshift-render/shaders/particle_field3d.wgsl), [`RibbonTrail3D`](../../crates/frameshift-render/shaders/ribbon_trail3d.wgsl) ve [`Transform3D`](../../crates/frameshift-render/shaders/transform3d.wgsl) düğümlerine sahiptir. Ancak her 3D düğüm kendi ayrı `Rgba16Float` dokusuna çizilip 2D `Blend` ile birleştirildiği için şu 4 temel eksiklik oluşmaktadır:

1. **Paylaşılan Z-Buffer Yok:** 3D kelimeler, 3D şekiller, şeritler ve parçacıklar arasında gerçek derinlik gizlemesi (Z-occlusion) yok; örn. bir şerit veya parçacık kelimenin arkasından dolanıp önüne geçemiyor, 90° L-köşe kelimeleri birbirini fiziksel olarak kesemiyor.
2. **Birleşik Uçan `Camera3D` Yok:** Her düğüm kendi `camera_pos / camera_rot_deg / fov_deg` alanlarını taşıyor; 90° dönüşlü kamera uçuşlarında `ParamBindingDef` ifadeleri her düğüme ayrı ayrı kopyalanmak zorunda kalıyor ve hedefe kilitli (`look_at` + `roll`) kamera modu bulunmuyor.
3. **Gerçek Z-Buffer Bokeh DoF Yok:** `Text3D` ve `RibbonTrail3D` üzerinde Depth of Field hiç yok; `Transform3D` yalnızca düzlem üzerinde analitik 9-tap blur uyguluyor.
4. **`SafeExpr` Agresif Kinetik Easing Eksikliği:** [`crates/frameshift-time/src/expr.rs`](../../crates/frameshift-time/src/expr.rs) yalnızca `sin, cos, abs, min, max, clamp, smoothstep, lerp, step, pow` içeriyor; `exp` ve `@platsae` stilindeki sert ivmeli `ease_out_expo`, `ease_in_expo`, `ease_in_out_expo`, `ease_out_back` eğrileri eksik.

Bu belge; mevcut 9-crate Rust + `wgpu 30.0.1` + `glam 0.29.3` + `Rgba16Float` (Linear Rec.709, premultiplied alpha) mimarisiyle **sıfır yeni harici Cargo crate'i ekleyerek** ve mevcut M1–M5 / Faz 6.1–6.2 düğümleriyle **%100 geriye dönük uyumlu** kalarak bu 4 bileşenin uygulamaya hazır teknik şartnamesini tanımlar.

---

## İncelenen Birincil Kaynaklar

Tüm harici API, matematiksel formül ve shader referansları `2026-11-22` tarihinde doğrulanmıştır:

### Harici Birincil Kaynaklar
1. **W3C WGSL Specification (`texture_depth_2d`, `textureLoad`, `textureSampleLevel`):**
   - [https://www.w3.org/TR/WGSL/](https://www.w3.org/TR/WGSL/) (HTTP 200 doğrulandı: `2026-11-22`)
2. **`wgpu` v30.0.1 Resmi Dokümantasyonu (`TextureSampleType::Depth`, `TextureUsages`, `TextureAspect`):**
   - [https://docs.rs/wgpu/30.0.1/wgpu/enum.TextureSampleType.html](https://docs.rs/wgpu/30.0.1/wgpu/enum.TextureSampleType.html) (HTTP 200 doğrulandı: `2026-11-22`)
   - [https://docs.rs/wgpu/30.0.1/wgpu/struct.TextureUsages.html](https://docs.rs/wgpu/30.0.1/wgpu/struct.TextureUsages.html) (HTTP 200 doğrulandı: `2026-11-22`)
3. **`glam` v0.29.3 Resmi Dokümantasyonu (`Mat4::perspective_rh`, `Mat4::look_at_rh`, `[0, 1]` NDC Z aralığı):**
   - [https://docs.rs/glam/0.29.3/glam/f32/struct.Mat4.html](https://docs.rs/glam/0.29.3/glam/f32/struct.Mat4.html) (HTTP 200 doğrulandı: `2026-11-22`)
4. **Jorge Jimenez (SIGGRAPH 2014 — *Next Generation Post Processing in Call of Duty: Advanced Warfare*):**
   - [https://www.iryoku.com/next-generation-post-processing-in-call-of-duty-advanced-warfare/](https://www.iryoku.com/next-generation-post-processing-in-call-of-duty-advanced-warfare/) (HTTP 200 doğrulandı: `2026-11-22`) — Scatter-as-You-Gather, CoC ayrıştırma, Interleaved Gradient Noise (IGN) ile disk örnekleme titreşimini azaltma ve şeffaflık/HDR bokeh korunumu.
5. **Godot Engine `bokeh_dof_raster.glsl` & `bokeh_dof.glsl` (MIT Lisansı):**
   - [https://raw.githubusercontent.com/godotengine/godot/master/servers/rendering/renderer_rd/shaders/effects/bokeh_dof_raster.glsl](https://raw.githubusercontent.com/godotengine/godot/master/servers/rendering/renderer_rd/shaders/effects/bokeh_dof_raster.glsl) (HTTP 200 doğrulandı: `2026-11-22`)
   - [https://raw.githubusercontent.com/godotengine/godot/master/servers/rendering/renderer_rd/shaders/effects/bokeh_dof.glsl](https://raw.githubusercontent.com/godotengine/godot/master/servers/rendering/renderer_rd/shaders/effects/bokeh_dof.glsl) (HTTP 200 doğrulandı: `2026-11-22`)
6. **Catlike Coding — *Depth of Field* (CoC & Near/Far Ağırlıklandırma Referansı):**
   - [https://catlikecoding.com/unity/tutorials/advanced-rendering/depth-of-field/](https://catlikecoding.com/unity/tutorials/advanced-rendering/depth-of-field/) (HTTP 200 doğrulandı: `2026-11-22`)
7. **Easings.net (Standart `Expo` ve `Back` Easing Formülleri):**
   - [https://easings.net/](https://easings.net/) (HTTP 200 doğrulandı: `2026-11-22`)

### Yerel Kod Tabanı & Sözleşme Kaynakları
- [Cargo.toml](../../Cargo.toml), [Cargo.lock](../../Cargo.lock) (`glam = "0.29.3"`), [crates/frameshift-render/Cargo.toml](../../crates/frameshift-render/Cargo.toml) (`wgpu = "30.0.1"`, `bytemuck = "1.25.2"`)
- [crates/frameshift-schema/src/graph.rs](../../crates/frameshift-schema/src/graph.rs), [crates/frameshift-schema/src/project.rs](../../crates/frameshift-schema/src/project.rs), [crates/frameshift-schema/src/error.rs](../../crates/frameshift-schema/src/error.rs)
- [crates/frameshift-render/src/compositor.rs](../../crates/frameshift-render/src/compositor.rs), [crates/frameshift-render/src/pipelines.rs](../../crates/frameshift-render/src/pipelines.rs), [crates/frameshift-render/src/uniforms.rs](../../crates/frameshift-render/src/uniforms.rs), [crates/frameshift-render/src/context.rs](../../crates/frameshift-render/src/context.rs), [crates/frameshift-render/src/pool.rs](../../crates/frameshift-render/src/pool.rs)
- [crates/frameshift-time/src/expr.rs](../../crates/frameshift-time/src/expr.rs)
- Kilitli sözleşmeler: [render-graph.md](../specs/render-graph.md), [effect-shader-api.md](../specs/effect-shader-api.md), [project-schema.md](../specs/project-schema.md)
- Stil referansı: [SC-0001-platsae-3d-kinetic-typography.md](../style-cards/SC-0001-platsae-3d-kinetic-typography.md) ve [3d-kinetic-typography-architecture.md](3d-kinetic-typography-architecture.md)

---

## Doğrudan Gözlemler (Kanıtlı)

### 1. Mevcut `wgpu` / `Depth32Float` / `Camera` Durumu (`frameshift-render`)
- [context.rs](../../crates/frameshift-render/src/context.rs#L24-L30): `GpuContext::new_headless()` cihazı `wgpu::Features::empty()` ve `wgpu::Limits::default()` ile açar.
- [pool.rs](../../crates/frameshift-render/src/pool.rs#L4): Çalışma formatı `WORKING_TEXTURE_FORMAT = wgpu::TextureFormat::Rgba16Float` (Linear Rec.709, premultiplied alpha).
- [compositor.rs](../../crates/frameshift-render/src/compositor.rs#L2297-L2317): `ensure_depth_texture_cached` şu anda `Depth32Float` dokusunu yalnızca `wgpu::TextureUsages::RENDER_ATTACHMENT` ile üretmektedir (`TEXTURE_BINDING` bayrağı eksiktir).
- [compositor.rs](../../crates/frameshift-render/src/compositor.rs#L2097-L2104): `Text3D` render geçişi `depth_ops.store` için `wgpu::StoreOp::Discard` kullanmaktadır; dolayısıyla geçiş sonrası derinlik okunamaz.
- [pipelines.rs](../../crates/frameshift-render/src/pipelines.rs#L422-L444): `particle_field3d` ve `ribbon_trail3d` boru hatları `depth_stencil: None` ile derlenir (`text3d_mesh` ise `Depth32Float`, `depth_write_enabled: Some(true)`, `depth_compare: Some(CompareFunction::Less)` kullanır).
- [compositor.rs](../../crates/frameshift-render/src/compositor.rs#L2867-L2897): `build_camera_matrices` `glam::Mat4::perspective_rh(fov_y_rad, aspect, 0.05, 200.0)` kullanır (`near = 0.05`, `far = 200.0`). Kamera yönelimi yalnızca `Quat::from_euler(EulerRot::XYZ, ...)` ile kurulur; `look_at` hedefi desteklenmez.

### 2. `Depth32Float` Okuma Kuralları (`wgpu 30.0.1` + WGSL)
- **Özellik Gereksinimi:** WebGPU ve `wgpu 30.0.1` çekirdek spesifikasyonunda `TextureFormat::Depth32Float`, `RENDER_ATTACHMENT | TEXTURE_BINDING` kullanım bayraklarını hiçbir ek `wgpu::Features` açmadan yerel olarak destekler.
- **Bind Group Tipi:** `wgpu::TextureSampleType::Depth` (`view_dimension: D2`, `multisampled: false`) WGSL'deki `texture_depth_2d` tipine karşılık gelir (`TextureSampleType::Float` ile bağlanamaz).
- **`textureLoad` vs `textureSampleLevel` Karşılaştırması:**
  - `textureSampleLevel(depth_tex, s, uv, 0.0)` WGSL'de `texture_depth_2d` için yalnızca **`SamplerBindingType::NonFiltering`** bir sampler ile geçerlidir; mevcut `ctx.linear_sampler` (`SamplerBindingType::Filtering`) ile kullanıldığında `wgpu` doğrulama hatası verir.
  - `textureLoad(depth_tex, coords: vec2<i32>, 0)` **hiçbir sampler gerektirmez**, ek sampler bağlaması eklemez ve keskin nesne kenarlarında derinliğin yanlışlıkla interpolasyonla "hayalet orta derinliğe" karışmasını önler. DoF ve SSAO gibi ekran uzayı post-FX geçişleri için kanonik ve doğrulamadan en temiz geçen yöntem `textureLoad` kullanımıdır.

### 3. `glam::Mat4::perspective_rh` Derinlik Doğrusallaştırma Matematiği
`glam 0.29.3` `Mat4::perspective_rh(fov_y_radians, aspect, near, far)` sağ-elli (right-handed) kamera uzayını (`-z` ileri yön, kamera önü uzaklığı $z_{\text{view}} = -z > 0$) WebGPU/DX NDC derinlik aralığı $d \in [0.0, 1.0]$ içine eşler:
$$d = \frac{\text{far}}{\text{far} - \text{near}} \left(1 - \frac{\text{near}}{z_{\text{view}}}\right)$$
Bu eşitlik $z_{\text{view}}$ için çözüldüğünde:
$$z_{\text{view}} = \frac{\text{near} \cdot \text{far}}{\text{far} - d \cdot (\text{far} - \text{near})}$$
- $d = 0.0 \implies z_{\text{view}} = \text{near} = 0.05$
- $d = 1.0 \implies z_{\text{view}} = \text{far} = 200.0$

### 4. Mevcut `SafeExpr` Yapısı (`frameshift-time`)
- [expr.rs](../../crates/frameshift-time/src/expr.rs#L113-L149): `BuiltinFunc` 1, 2 veya 3 argümanlı yerleşik fonksiyonları destekler (`vals = [0.0_f64; 3]`).
- Her AST düğümü değerlendirilirken `fuel` 1 azaltılır (`DEFAULT_MAX_FUEL = 256`); tüm girişler ve çıkışlar `ensure_finite` ile `NaN`/`Inf`'a karşı denetlenir; `Smoothstep` giriş parametresini `.clamp(0.0, 1.0)` ile sınırlar.

---

## Teknik Tasarım ve Şartname (Çıkarımlar ve Öneriler)

### Bölüm 1: `NodeKind::Scene3D` ve Birleşik `Camera3D` DAG Şeması (`frameshift-schema`)

#### Tasarım Kararı: Variadic Girdili `Scene3D` Düğümü + Birinci Sınıf 3D Çocuk Düğümler
`ParamBindingDef` (`node_id + param_name`) ve `GraphNodeDef.enabled` mekanizmaları **düğüm kimliği (`NodeId`)** düzeyinde çalıştığı için 3D nesneleri `Scene3D` içine gömülü bir diziye hapsetmek yerine, mevcut 3D düğümleri (`Text3D`, Part B'deki `Shape3D`, `ParticleField3D`, `RibbonTrail3D`, `Transform3D`) kendi `NodeId`'leri ile graf içinde tutmak ve **`NodeKind::Scene3D`** düğümünü bunları `inputs: Vec<NodeId>` üzerinden toplayan paylaşımlı 3D sahne geçişi olarak tanımlamak en temiz çözümdür.

- **Geriye Dönük Uyumluluk:** `Transform3D`, `Text3D`, `ParticleField3D` ve `RibbonTrail3D` tek başlarına (`Scene3D` dışında) kullanıldıklarında mevcut davranışlarını %100 korur; tüm M1–M5 ve Faz 6.1–6.2 testleri dokunulmadan geçer.
- **Paylaşımlı Sahne Semantiği:** Bir 3D düğüm `Scene3D.inputs` içine bağlandığında, `Scene3D` bu çocukların `NodeKind` parametrelerini kendi `sub_tick` zamanında `animator.evaluate_node(...)` ile çözer, `child.enabled == false` ise atlar ve hepsini **kendi ana `Camera3D` matrisleri ile** tek bir paylaşımlı `(Rgba16Float, Depth32Float)` render geçişinde çizer.

#### Önerilen Rust Şema Yapıları (`crates/frameshift-schema/src/graph.rs`)

```rust
/// Camera orientation mode for unified `Scene3D` rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Camera3DMode {
    /// Uses `camera_rot_deg` as XYZ Euler orientation (matches Faz 6.2 `build_camera_matrices`).
    #[default]
    Euler,
    /// Aims camera at `camera_target` and applies `camera_rot_deg[2]` as Dutch roll.
    LookAt,
}

/// Shared camera & Z-buffer Bokeh DoF configuration for `NodeKind::Scene3D`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Camera3DDef {
    #[serde(default = "default_camera_pos")]
    pub pos: [f32; 3],
    /// Euler XYZ degrees (`Euler` mode), or `rot_deg[2]` = roll degrees (`LookAt` mode).
    #[serde(default)]
    pub rot_deg: [f32; 3],
    #[serde(default)]
    pub target: [f32; 3],
    #[serde(default)]
    pub mode: Camera3DMode,
    #[serde(default = "default_fov_deg")]
    pub fov_deg: f32,
    #[serde(default = "default_near_plane")]
    pub near: f32,
    #[serde(default = "default_far_plane")]
    pub far: f32,
    #[serde(default = "default_dof_focal_dist")]
    pub dof_focal_dist: f32,
    /// When true in `LookAt` mode, overrides `dof_focal_dist` with `length(target - pos)`.
    #[serde(default)]
    pub dof_autofocus_target: bool,
    #[serde(default)]
    pub dof_blur_strength: f32,
    #[serde(default = "default_dof_max_radius_px")]
    pub dof_max_radius_px: f32,
    #[serde(default = "default_dof_samples")]
    pub dof_samples: u32,
    #[serde(default = "default_dof_highlight_boost")]
    pub dof_highlight_boost: f32,
}
```

`NodeKind` içine eklenecek yeni varyant:

```rust
    /// Shared 3D scene pass combining 3D objects (`inputs[0..N]`) under one unified `Camera3D`,
    /// one shared `Depth32Float` buffer, and optional post-process Z-buffer Bokeh DoF.
    #[serde(alias = "scene_3d")]
    Scene3D {
        #[serde(default)]
        camera: Camera3DDef,
        /// Background clear color in Linear Rec.709 premultiplied alpha (default transparent).
        #[serde(default)]
        background_rgba: [f32; 4],
    },
```

#### `validate_graph()` ve `expected_input_count()` Kuralları
1. `expected_input_count()` tek bir sabit yerine `Scene3D` için variadic arity doğrulaması yapmalıdır (`0..=64` girdi; mevcut `expected_input_count(&self) -> usize` imzasını bozmamak için `NodeKind::accepts_input_count(&self, count: usize) -> bool` yardımcı metodu eklenir, diğer tüm düğümler `count == self.expected_input_count()` döner).
2. `Scene3D` çocuk düğüm doğrulaması (`SchemaError::InvalidScene3D { node_id, message }`):
   - `Scene3D.inputs` içindeki her `NodeId` var olmalı ve türü `Text3D | Shape3D | ParticleField3D | RibbonTrail3D | Transform3D` olmalıdır (`SourceVideo`, `VectorText`, `Blend` vb. 2D dokular `Transform3D` sarmalayıcısı üzerinden 3D karta dönüştürülerek sahneye bağlanır).
   - `camera.near > 0.0`, `camera.far > camera.near`, `camera.fov_deg` sonlu (`5.0..=175.0`), `camera.dof_samples` `8..=32` aralığında olmalıdır.
3. **`ParamBindingDef` Bağlanabilir Parametreler (`crates/frameshift-schema/src/project.rs`):**
   - `NodeKind::Scene3D` için:
     `"cam_x"`, `"cam_y"`, `"cam_z"`, `"cam_rot_x"`, `"cam_rot_y"`, `"cam_rot_z"`, `"cam_roll"`, `"cam_target_x"`, `"cam_target_y"`, `"cam_target_z"`, `"fov_deg"`, `"dof_focal_dist"`, `"dof_blur_strength"`, `"dof_max_radius_px"`, `"dof_highlight_boost"`.
   - Not: `"cam_roll"` hem `rot_deg[2]` üzerine yazar; böylece `LookAt` modunda doğrudan `"cam_roll"` veya `"cam_rot_z"` animasyonu Dutch tilt üretir.

#### `Scene3D` Render Akışı (`compositor.rs` & `pipelines.rs`)
`evaluate_subgraph` içinde `Scene3D` düğümüne ulaşıldığında:
1. **`collect_direct_cone` & Çocuk Dokusu İptali:** `Scene3D`'nin doğrudan altındaki `Text3D`, `Shape3D`, `ParticleField3D`, `RibbonTrail3D` çocukları başka bir 2D düğüm tarafından tüketilmiyorsa ayrı 2D doku olarak önceden render edilmez (yalnızca `Transform3D` çocuklarının `inputs[0]` 2D dokuları normal değerlendirilir).
2. **Ortak Kamera Matrisi (`build_scene_camera_matrices`):**
   - `Euler` modunda mevcut `build_camera_matrices` ile özdeştir.
   - `LookAt` modunda:
     ```rust
     let pos_v = glam::Vec3::from_array(camera.pos);
     let target_v = glam::Vec3::from_array(camera.target);
     let forward = (target_v - pos_v).try_normalize().unwrap_or(glam::Vec3::NEG_Z);
     let ref_up = if forward.dot(glam::Vec3::Y).abs() > 0.999 {
         glam::Vec3::Z
     } else {
         glam::Vec3::Y
     };
     let roll_quat = glam::Quat::from_axis_angle(forward, camera.rot_deg[2].to_radians());
     let up_v = roll_quat * ref_up;
     let view = glam::Mat4::look_at_rh(pos_v, target_v, up_v);
     let cam_world = view.inverse();
     let cam_right = cam_world.x_axis.truncate().normalize_or(glam::Vec3::X);
     let cam_up = cam_world.y_axis.truncate().normalize_or(glam::Vec3::Y);
     ```
   - `proj = glam::Mat4::perspective_rh(fov_y_rad, aspect, near, far)`.
3. **İki Fazlı Tek Depth-Buffer Render Geçişi (Tek `color_tex` + Tek `depth_tex`):**
   - `color_tex` (`Rgba16Float`): `LoadOp::Clear(background_rgba)`, `StoreOp::Store`.
   - `depth_tex` (`Depth32Float`, `RENDER_ATTACHMENT | TEXTURE_BINDING`): `LoadOp::Clear(1.0)`, **`StoreOp::Store`**.
   - **Faz A — Opak / Z-Yazan Geometri (`depth_write_enabled: Some(true)`, `depth_compare: Some(Less)`):**
     1. `Text3D` mesh'leri (`text3d_mesh` pipeline)
     2. `Shape3D` mesh'leri (`shape3d_mesh` / `text3d_mesh` uyumlu pipeline)
     3. `Transform3D` 3D kartları (`transform3d_scene` pipeline: `alpha <= 0.001` pikselinde `discard` yaparak şeffaf kenarların yanlış derinlik yazmasını engeller)
   - **Faz B — Yarı-Saydam / Toplamsal HDR Atmosfer (`depth_write_enabled: Some(false)`, `depth_compare: Some(Less)`):**
     1. `RibbonTrail3D` (`ribbon_trail3d_scene` pipeline — Faz 6.2 `ribbon_trail3d` ile aynı shader, ancak `depth_stencil` ekli ve `depth_write_enabled: Some(false)`)
     2. `ParticleField3D` (`particle_field3d_scene` pipeline — aynı shader, `depth_write_enabled: Some(false)`). Not: `Scene3D` DoF aktifken parçacık içi `dof_blur_strength` `0.0` tutulabilir veya yumuşak geçiş için düşük değerle post-FX DoF'a devredilebilir.
   - **Kritik Neden:** Faz B `depth_write: false` + `depth_compare: Less` kullandığı için şeritler ve parçacıklar 3D metinlerin ve şekillerin **gerçekten arkasından dolaşıp önüne geçebilir**, ancak parçacıkların yarı-saydam SDF kenarları başka parçacıkları veya arkadaki yazıları dikdörtgen karelerle maskelemez.

---

### Bölüm 2: `Depth32Float` Örnekleme & 2-Geçişli Vogel Disk Bokeh DoF (`wgpu` + WGSL)

#### Neden 2 Geçişli (`Pass 1: CoC + Near-Dilate`, `Pass 2: Vogel Gather`)?
1. **Saçılma Yarıçapı Sorunu (Scatter-as-Gather):** Bir ön plan nesnesi (`coc < 0`, örn. `-22 px`) odak noktasındaki bir arka planın veya şeffaf boşluğun üzerine taştığında, o arka plan pikselinin kendi `center_coc`'u `0` olsa bile komşu ön plan bokeh'ini toplaması gerekir. Tek geçişte tüm ekran boyunca sabit `40 px` taramak hem keskin bölgelerde 24 tap'ı boşa harcar hem de örnekleme yoğunluğunu düşürür.
2. **2 Geçişle Tam Çözüm (0 Yeni Texture Formatı, Tüm Ara Buffer'lar `PooledTexture` `Rgba16Float`):**
   - **Pass 1 (`fx_dof_coc.wgsl`):** Tam çözünürlükte çalışır ve her piksel için 4 kanallı CoC/yardımcı tamponunu (`coc_tex: Rgba16Float`) üretir:
     - `.r`: `signed_coc_px` (`< 0` yakın/ön plan, `> 0` uzak/arka plan, `0` odakta veya `raw_depth >= 0.99999` boş uzay)
     - `.g`: `dilated_near_coc_px` — merkez etrafında küçük sabit `5x5` seyrek (step=4px, 9/25-tap) veya 9-tap çapraz örnekleme ile en güçlü yakın ön plan `min(signed_coc_px, 0.0)` mutlak değeri (`<= max_radius_px`). Bu sayede keskin arka plan/boşluk kenarları, ön plan nesnenin `20 px` taşan bokeh halosunu önceden bilir!
     - `.b`: `linear_depth` (`z_view`)
     - `.a`: `center_alpha` (orijinal `color_tex.a`)
   - **Pass 2 (`fx_dof_bokeh.wgsl`):** Tam çözünürlükte `color_tex` + `coc_tex` okur ve **24-tap Vogel Disk + Interleaved Gradient Noise (IGN)** ile nihai `Rgba16Float` (premultiplied alpha) çıktısını üretir:
     - `gather_radius_px = max(abs(center_coc), dilated_near_coc)`.
     - Eğer `gather_radius_px <= 0.35`: tek okuma ile doğrudan `color_tex` döner (keskin alanlarda sıfır bulanıklık kaybı!).
     - Aksi halde sabit döngü (`MAX_TAPS = 32`, varsayılan `24` tap, her zaman TDR-güvenli):

#### `wgpu 30.0.1` Depth Bağlantısı ve `ensure_depth_texture_cached` Güncellemesi

```rust
// crates/frameshift-render/src/compositor.rs
let texture = self.ctx.device.create_texture(&wgpu::TextureDescriptor {
    label: Some("frameshift-depth32f"),
    size: wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    },
    mip_level_count: 1,
    sample_count: 1,
    dimension: wgpu::TextureDimension::D2,
    format: wgpu::TextureFormat::Depth32Float,
    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
    view_formats: &[],
});
let view = texture.create_view(&wgpu::TextureViewDescriptor {
    aspect: wgpu::TextureAspect::DepthOnly,
    ..Default::default()
});
```

Pass 1 (`fx_dof_coc`) BindGroupLayout düzeni:
- `@binding(0)`: `color_tex` (`TextureSampleType::Float { filterable: true }`, `Rgba16Float`)
- `@binding(1)`: `depth_tex` (`TextureSampleType::Depth`, `Depth32Float`, sampler yok -> `textureLoad`)
- `@binding(2)`: `uniform` (`DofCocUniform`, 32 byte)

Pass 2 (`fx_dof_bokeh`) BindGroupLayout düzeni (mevcut 2-girdi `build_program(..., 2, Some(32), WORKING_TEXTURE_FORMAT)` kalıbıyla birebir uyumlu!):
- `@binding(0)`: `color_tex` (`TextureSampleType::Float { filterable: true }`)
- `@binding(1)`: `coc_tex` (`TextureSampleType::Float { filterable: true }`)
- `@binding(2)`: `linear_sampler` (`SamplerBindingType::Filtering`)
- `@binding(3)`: `uniform` (`DofBokehUniform`, 32 byte)

#### 16-Byte Hizalı Rust Uniform Yapıları (`crates/frameshift-render/src/uniforms.rs`)

```rust
/// Uniform buffer for `fx_dof_coc.wgsl` (32 bytes, 16-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct DofCocUniform {
    /// `x` = near, `y` = far, `z` = focal_dist, `w` = blur_strength.
    pub depth_params: [f32; 4],
    /// `x` = max_radius_px, `y` = texel_size_x, `z` = texel_size_y, `w` = reserved.
    pub screen_params: [f32; 4],
}

/// Uniform buffer for `fx_dof_bokeh.wgsl` (32 bytes, 16-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct DofBokehUniform {
    /// `x` = texel_size_x, `y` = texel_size_y, `z` = max_radius_px, `w` = tap_count_f32.
    pub params0: [f32; 4],
    /// `x` = highlight_boost, `y` = near_bleed_bias, `z` = jitter_seed, `w` = reserved.
    pub params1: [f32; 4],
}

const _: () = {
    assert!(std::mem::size_of::<DofCocUniform>() == 32);
    assert!(std::mem::size_of::<DofBokehUniform>() == 32);
};
```

#### WGSL Shader Referans Uygulaması: Pass 1 (`fx_dof_coc.wgsl`)

```wgsl
// FrameShift Faz 6.3 Shader: fx_dof_coc.wgsl
// Computes linearized glam::perspective_rh depth and signed Circle of Confusion (px),
// plus sparse 5x5 foreground near-CoC dilation for bleed-free bokeh gather.

struct DofCocUniform {
    // x = near, y = far, z = focal_dist, w = blur_strength
    depth_params: vec4<f32>,
    // x = max_radius_px, y = texel_size_x, z = texel_size_y, w = reserved
    screen_params: vec4<f32>,
}

@group(0) @binding(0) var color_tex: texture_2d<f32>;
@group(0) @binding(1) var depth_tex: texture_depth_2d;
@group(0) @binding(2) var<uniform> u: DofCocUniform;

fn linearize_depth_rh(raw_d: f32, near: f32, far: f32) -> f32 {
    let d = clamp(raw_d, 0.0, 1.0);
    let denom = max(far - d * (far - near), 1e-6);
    return (near * far) / denom;
}

fn eval_signed_coc(coord: vec2<i32>, max_coord: vec2<i32>) -> vec4<f32> {
    let c = clamp(coord, vec2<i32>(0, 0), max_coord);
    let raw_d = textureLoad(depth_tex, c, 0);
    let col = textureLoad(color_tex, c, 0);

    let near = max(u.depth_params.x, 1e-4);
    let far = max(u.depth_params.y, near + 0.01);
    let focal = max(u.depth_params.z, near);
    let strength = max(u.depth_params.w, 0.0);
    let max_rad = clamp(u.screen_params.x, 0.0, 48.0);

    // Empty transparent void (depth == 1.0 and alpha <= 1e-4) has 0 CoC so sharp objects
    // never bleed into transparent background.
    if (raw_d >= 0.99999 && col.a <= 1e-4) {
        return vec4<f32>(0.0, far, col.a, 1.0);
    }

    let z_view = linearize_depth_rh(raw_d, near, far);
    // Thin-lens signed CoC: negative = foreground (near), positive = background (far).
    let raw_coc = ((z_view - focal) / max(z_view, 1e-3)) * strength * 24.0;
    let signed_coc = clamp(raw_coc, -max_rad, max_rad);
    return vec4<f32>(signed_coc, z_view, col.a, 0.0);
}

@fragment
fn fs_main(@builtin(position) frag_coord: vec4<f32>) -> @location(0) vec4<f32> {
    let dims = vec2<i32>(textureDimensions(depth_tex));
    let max_coord = max(dims - vec2<i32>(1, 1), vec2<i32>(0, 0));
    let center = vec2<i32>(frag_coord.xy);

    let info = eval_signed_coc(center, max_coord);
    let max_rad = clamp(u.screen_params.x, 0.0, 48.0);
    let step_px = max(i32(ceil(max_rad * 0.5)), 1);

    // Sparse 5x5 neighborhood dilation for near-field foreground CoC (< 0).
    var min_near_coc = min(info.x, 0.0);
    for (var dy: i32 = -2; dy <= 2; dy = dy + 1) {
        for (var dx: i32 = -2; dx <= 2; dx = dx + 1) {
            if (dx == 0 && dy == 0) {
                continue;
            }
            let n_info = eval_signed_coc(center + vec2<i32>(dx, dy) * step_px, max_coord);
            min_near_coc = min(min_near_coc, n_info.x);
        }
    }

    // r = signed_coc_px, g = dilated_near_coc_abs_px, b = linear_z, a = center_alpha
    return vec4<f32>(info.x, abs(min_near_coc), info.y, info.z);
}
```

#### WGSL Shader Referans Uygulaması: Pass 2 (`fx_dof_bokeh.wgsl` — Premultiplied Alpha & Linear Rec.709 HDR Güvenli)

```wgsl
// FrameShift Faz 6.3 Shader: fx_dof_bokeh.wgsl
// Bounded <= 32-tap Vogel Disk + Interleaved Gradient Noise (Jimenez 2014) Bokeh DoF.
// Working format: Rgba16Float, Linear Rec.709, premultiplied alpha.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct DofBokehUniform {
    // x = texel_size_x, y = texel_size_y, z = max_radius_px, w = tap_count_f32
    params0: vec4<f32>,
    // x = highlight_boost, y = near_bleed_bias, z = jitter_seed, w = reserved
    params1: vec4<f32>,
}

@group(0) @binding(0) var color_tex: texture_2d<f32>;
@group(0) @binding(1) var coc_tex: texture_2d<f32>;
@group(0) @binding(2) var linear_sampler: sampler;
@group(0) @binding(3) var<uniform> u: DofBokehUniform;

const GOLDEN_ANGLE: f32 = 2.39996323;
const MAX_VOGEL_TAPS: i32 = 32;

// Jimenez (2014) Interleaved Gradient Noise for temporally stable per-pixel disk rotation.
fn interleaved_gradient_noise(pixel_xy: vec2<f32>) -> f32 {
    let magic = vec3<f32>(0.06711056, 0.00583715, 52.9829189);
    return fract(magic.z * fract(dot(pixel_xy, magic.xy)));
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let center_col = textureSampleLevel(color_tex, linear_sampler, in.uv, 0.0);
    let center_coc_data = textureSampleLevel(coc_tex, linear_sampler, in.uv, 0.0);

    let center_coc = center_coc_data.r;
    let dilated_near = center_coc_data.g;
    let center_z = center_coc_data.b;
    let gather_radius = min(max(abs(center_coc), dilated_near), clamp(u.params0.z, 0.0, 48.0));

    if (gather_radius <= 0.35) {
        return vec4<f32>(max(center_col.rgb, vec3<f32>(0.0)), clamp(center_col.a, 0.0, 1.0));
    }

    let tap_count = clamp(i32(round(u.params0.w)), 8, MAX_VOGEL_TAPS);
    let texel = max(u.params0.xy, vec2<f32>(1e-6));
    let rot_offset = interleaved_gradient_noise(in.position.xy + vec2<f32>(u.params1.z)) * 6.2831853;

    var bg_acc = vec4<f32>(0.0);
    var bg_w = 0.0;
    var fg_acc = vec4<f32>(0.0);
    var fg_w = 0.0;

    for (var i: i32 = 0; i < MAX_VOGEL_TAPS; i = i + 1) {
        if (i >= tap_count) {
            break;
        }
        let fi = f32(i);
        let r_norm = sqrt((fi + 0.5) / f32(tap_count));
        let r_px = r_norm * gather_radius;
        let theta = fi * GOLDEN_ANGLE + rot_offset;
        let offset_uv = vec2<f32>(cos(theta), sin(theta)) * r_px * texel;
        let suv = clamp(in.uv + offset_uv, vec2<f32>(0.0), vec2<f32>(1.0));

        let s_col_raw = textureSampleLevel(color_tex, linear_sampler, suv, 0.0);
        let s_col = vec4<f32>(max(s_col_raw.rgb, vec3<f32>(0.0)), clamp(s_col_raw.a, 0.0, 1.0));
        let s_coc_data = textureSampleLevel(coc_tex, linear_sampler, suv, 0.0);
        let s_coc = s_coc_data.r;
        let s_z = s_coc_data.b;

        // HDR bokeh highlight weighting: preserves bright >1.0 neon/specular discs without clamping.
        let luma = dot(s_col.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
        let hdr_w = 1.0 + max(luma - 1.0, 0.0) * max(u.params1.x, 0.0);

        // Foreground scatter-as-gather (s_coc < 0): foreground blurs over background & void.
        let fg_coc_abs = max(-s_coc, 0.0);
        let fg_cov = smoothstep(r_px - 1.0, r_px + 1.0, fg_coc_abs);
        if (fg_cov > 0.001) {
            let w_fg = fg_cov * hdr_w;
            fg_acc = fg_acc + s_col * w_fg;
            fg_w = fg_w + w_fg;
        }

        // Background gather (s_coc >= 0 or empty void): prevent background from bleeding
        // onto sharp foreground objects in front of it.
        let depth_ok = select(smoothstep(center_z - 0.15, center_z + 0.05, s_z), 1.0, center_coc >= 0.0);
        let bg_rad = max(max(s_coc, center_coc), 0.0);
        let bg_cov = smoothstep(r_px - 1.0, r_px + 1.0, bg_rad) * depth_ok;
        if (bg_cov > 0.001) {
            let w_bg = bg_cov * hdr_w;
            bg_acc = bg_acc + s_col * w_bg;
            bg_w = bg_w + w_bg;
        }
    }

    let bg_color = select(center_col, bg_acc / max(bg_w, 1e-5), bg_w > 1e-4);
    let fg_alpha_cov = clamp(fg_w / f32(tap_count) * (1.0 + max(u.params1.y, 0.0)), 0.0, 1.0);
    let fg_color = select(vec4<f32>(0.0), fg_acc / max(fg_w, 1e-5), fg_w > 1e-4);

    // Premultiplied alpha mix between background/in-focus and scattered foreground bokeh.
    let out_rgba = mix(bg_color, fg_color, fg_alpha_cov);
    return vec4<f32>(max(out_rgba.rgb, vec3<f32>(0.0)), clamp(out_rgba.a, 0.0, 1.0));
}
```

> **Tasarım Notu (Tek Geçiş vs 2 Geçiş):** Eğer ileride performans profili için 1-geçişli hızlı önizleme modu istenirse, `fx_dof_bokeh` tek geçişte `depth_tex`'i doğrudan `textureLoad` ile okuyabilir; ancak bu modda `dilated_near_coc` bilinmediği için ön plan nesneleri kendi silüetlerinin dışına yumuşak bokeh taşıramaz. Prodüksiyon kalitesi için yukarıdaki **2-geçişli mimari** önerilir.

---

### Bölüm 3: `SafeExpr` Kinetik Easing Eklentileri (`crates/frameshift-time/src/expr.rs`)

#### Yeni `BuiltinFunc` Varyantları ve Sözleşme
[`BuiltinFunc`](../../crates/frameshift-time/src/expr.rs#L113) enum'una aşağıdaki 5 tek-argümanlı (arity = `1`) fonksiyon eklenir:

| Fonksiyon Adı | Arity | Matematiksel Tanım ($t = \text{clamp}(x, 0.0, 1.0)$) | Güvenlik & Sınır Garantileri |
| :--- | :---: | :--- | :--- |
| `exp(x)` | `1` | $e^x$ (`vals[0].exp()`) | `x > 709.78` için `f64::exp` $+\infty$ üretir; `ensure_finite` deterministik `ExprError::NonFiniteValue` döner. |
| `ease_out_expo(x)` | `1` | $t \le 0 \implies 0$, $t \ge 1 \implies 1$, aksi halde $1 - 2^{-10t}$ | Giriş `[0, 1]`'e clamp edilir; süre dışı zamanlarda ($t < 0$ veya $t > 1$) patlama olmaz. |
| `ease_in_expo(x)` | `1` | $t \le 0 \implies 0$, $t \ge 1 \implies 1$, aksi halde $2^{10(t - 1)}$ | Sürekli uç değerler (`0.0` ve `1.0`), `NaN` üretmez. |
| `ease_in_out_expo(x)` | `1` | $t < 0.5 \implies \frac{2^{20t - 10}}{2}$, $t \ge 0.5 \implies \frac{2 - 2^{-20t + 10}}{2}$ | $t=0 \to 0$, $t=0.5 \to 0.5$, $t=1 \to 1$ tam sürekli. |
| `ease_out_back(x)` | `1` | $u = t - 1$; $1 + 2.70158\,u^3 + 1.70158\,u^2$ | %10.04 doğal overshoot (`~1.1004`), ardından `1.0`'a oturur; `[0, 1]` dışı clamp edilir. |

#### Önerilen Rust Uygulaması (`crates/frameshift-time/src/expr.rs`)

```rust
// BuiltinFunc::from_name:
"exp" => Some(Self::Exp),
"ease_out_expo" => Some(Self::EaseOutExpo),
"ease_in_expo" => Some(Self::EaseInExpo),
"ease_in_out_expo" => Some(Self::EaseInOutExpo),
"ease_out_back" => Some(Self::EaseOutBack),

// BuiltinFunc::arity:
Self::Sin
| Self::Cos
| Self::Abs
| Self::Exp
| Self::EaseOutExpo
| Self::EaseInExpo
| Self::EaseInOutExpo
| Self::EaseOutBack => 1,

// eval_node match arm:
BuiltinFunc::Exp => vals[0].exp(),
BuiltinFunc::EaseOutExpo => {
    let t = vals[0].clamp(0.0, 1.0);
    if t <= 0.0 {
        0.0
    } else if t >= 1.0 {
        1.0
    } else {
        1.0 - 2.0_f64.powf(-10.0 * t)
    }
}
BuiltinFunc::EaseInExpo => {
    let t = vals[0].clamp(0.0, 1.0);
    if t <= 0.0 {
        0.0
    } else if t >= 1.0 {
        1.0
    } else {
        2.0_f64.powf(10.0 * (t - 1.0))
    }
}
BuiltinFunc::EaseInOutExpo => {
    let t = vals[0].clamp(0.0, 1.0);
    if t <= 0.0 {
        0.0
    } else if t >= 1.0 {
        1.0
    } else if t < 0.5 {
        2.0_f64.powf(20.0 * t - 10.0) * 0.5
    } else {
        (2.0 - 2.0_f64.powf(-20.0 * t + 10.0)) * 0.5
    }
}
BuiltinFunc::EaseOutBack => {
    const C1: f64 = 1.70158;
    const C3: f64 = C1 + 1.0;
    let t = vals[0].clamp(0.0, 1.0);
    if t <= 0.0 {
        0.0
    } else if t >= 1.0 {
        1.0
    } else {
        let u = t - 1.0;
        1.0 + C3 * u * u * u + C1 * u * u
    }
}
```

- **Fuel ve Döngü Güvenliği:** Her yeni fonksiyon sabit $O(1)$ aritmetik işlem yapar; döngü veya özyineleme içermez; `eval_node` girişinde `1` fuel düşer ve çıkışı `ensure_finite(out)` ile doğrular.

---

### Bölüm 4: Etkilenecek Dosyalar Özeti (Uygulama Yol Haritası)

| Crate / Dosya | Planlanan Değişiklik |
| :--- | :--- |
| `crates/frameshift-time/src/expr.rs` | `Exp`, `EaseOutExpo`, `EaseInExpo`, `EaseInOutExpo`, `EaseOutBack` + birim testleri (`NaN`/`Inf`, sınır sürekliliği, fuel). |
| `crates/frameshift-schema/src/graph.rs`, `project.rs`, `error.rs`, `lib.rs` | `Camera3DMode`, `Camera3DDef`, `NodeKind::Scene3D`, `SchemaError::InvalidScene3D`, `accepts_input_count`, `bindable_param_names`. |
| `crates/frameshift-render/src/uniforms.rs` | `DofCocUniform` (32B), `DofBokehUniform` (32B) ve `compile-time` boyut assert'leri. |
| `crates/frameshift-render/shaders/fx_dof_coc.wgsl`, `fx_dof_bokeh.wgsl` | 2-geçişli `Depth32Float` (`textureLoad`) + Vogel Disk Bokeh WGSL shader'ları. |
| `crates/frameshift-render/src/pipelines.rs` | `fx_dof_coc`, `fx_dof_bokeh`, `particle_field3d_scene`, `ribbon_trail3d_scene`, `transform3d_scene` boru hatları. |
| `crates/frameshift-render/src/compositor.rs` | `ensure_depth_texture_cached` (`RENDER_ATTACHMENT \| TEXTURE_BINDING`), `StoreOp::Store`, `Scene3D` paylaşımlı 2-fazlı geçiş + `evaluate_node` dalları. |
| `crates/frameshift-app/src/snapshot.rs` | `node_kind_label` ve `describe_node_params` içinde `NodeKind::Scene3D` (ve varsa `Shape3D`) kolları. |

---

## Doğrulanmamış Noktalar

- **Derleme ve WGSL/Naga Doğrulaması:** Önerilen `fx_dof_coc.wgsl`, `fx_dof_bokeh.wgsl` ve `Scene3D` boru hatları henüz `frameshift-render` içinde `wgpu 30.0.1` Naga derleyicisiyle derlenip çalıştırılmadı (**doğrulanmadı**).
- **GPU Kare Süresi (1080p / 60fps):** 2-geçişli DoF (`5x5` seyrek CoC dilation + `24-tap` Vogel Disk gather) + `MotionBlurShutter (sub_samples=3..8)` + `DeepGlow` kombinasyonunun gerçek GPU render süresi henüz ölçülmedi (**doğrulanmadı**).
- **Yarı-Saydam `Transform3D` Kartların Sıralaması:** Faz A'da `alpha <= 0.001` `discard` ile tam saydam kenarlar elense de, iç bölgesinde kısmi saydamlık (`0 < alpha < 1`) taşıyan 2D video kartlarının (`Transform3D`) birbiri önünden geçerken arkadan-öne sıralanması henüz ele alınmadı; şu an en öndeki kısmi saydam kart derinlik yazdığı için arkasındaki objeyi kesebilir (**doğrulanmadı**).
