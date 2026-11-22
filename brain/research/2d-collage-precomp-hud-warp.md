---
title: "2D Kompozisyon, Kolaj, Precomp, Vektör/HUD ve Deformasyon Düğümleri (SourceImage, Precomp, VectorPathHud, CornerPin, GridWarp)"
date: 2026-11-22
status: active
tags: [render-graph, schema, shaders, media-io, text-vector, 2d-collage, precomp, hud, warp]
---

# 2D Kompozisyon, Kolaj, Precomp, Vektör/HUD ve Deformasyon Düğümleri

## Soru / Amaç

[VISUAL_REFERENCES.md](../../VISUAL_REFERENCES.md) içinde incelenen `tastxv2`, `zwxeditor` ve `_gutsz` referansları; FrameShift'in mevcut video/3D/post-FX yeteneklerine ek olarak 2D kolaj/panel düzeni, kapsüllenmiş alt-kompozisyonlar (**precomp**), veri/eğri tabanlı ince vektör HUD grafikleri (kübik Bézier, `dash`, `trim-path`, düğüm noktaları ve sayısal etiketler) ve 2D perspektif / ızgara deformasyonu (**CornerPin**, **GridWarp**) düğümlerine ihtiyaç duyulduğunu göstermektedir.

Bu araştırmanın amacı; mevcut 9-crate Rust + `wgpu 30.0.2` + `glam 0.29.3` + `lyon_tessellation 1.0.23` + `png 0.18.1` + `ffmpeg` çalışma ortamında **sıfır yeni harici Cargo crate'i ekleyerek**, kilitli [project-schema.md](../specs/project-schema.md), [render-graph.md](../specs/render-graph.md) ve [effect-shader-api.md](../specs/effect-shader-api.md) sözleşmeleriyle **%100 geriye dönük uyumlu** biçimde şu düğümlerin uygulamaya hazır teknik şartnamesini çıkarmaktır:

1. `NodeKind::SourceImage` (0 girdi)
2. `NodeKind::Precomp` (0 veya N girdi / kendi kapsüllenmiş alt-grafiği)
3. `NodeKind::VectorPathHud` (0 girdi)
4. `NodeKind::CornerPin` (1 girdi) ve `NodeKind::GridWarp` (1 girdi)

---

## İncelenen Birincil Kaynaklar

### Harici Birincil Kaynaklar (Doğrulama Tarihi: `2026-11-22`)

1. **`png` v0.18 Resmi Dokümantasyonu (`Decoder`, `ColorType`, `Transformations`):**
   - [https://docs.rs/png/latest/png/struct.Decoder.html](https://docs.rs/png/latest/png/struct.Decoder.html)
   - [https://docs.rs/png/latest/png/struct.Transformations.html](https://docs.rs/png/latest/png/struct.Transformations.html)
   - [https://docs.rs/png/latest/png/enum.ColorType.html](https://docs.rs/png/latest/png/enum.ColorType.html)
2. **FFmpeg Resmi Dokümantasyonu (`image2` Tek-Kare Görsel Demuxer):**
   - [https://ffmpeg.org/ffmpeg-formats.html](https://ffmpeg.org/ffmpeg-formats.html)
3. **`lyon_tessellation` v1.0.23 Resmi Dokümantasyonu (`FillTessellator`, `StrokeTessellator`, `FillRule`, `StrokeOptions`):**
   - [https://docs.rs/lyon_tessellation/latest/lyon_tessellation/](https://docs.rs/lyon_tessellation/latest/lyon_tessellation/)
   - [https://docs.rs/lyon_tessellation/latest/lyon_tessellation/struct.FillOptions.html](https://docs.rs/lyon_tessellation/latest/lyon_tessellation/struct.FillOptions.html)
   - [https://docs.rs/lyon_tessellation/latest/lyon_tessellation/struct.StrokeOptions.html](https://docs.rs/lyon_tessellation/latest/lyon_tessellation/struct.StrokeOptions.html)
4. **`glam` v0.29.3 Resmi Dokümantasyonu (`Mat3::try_inverse`, `Mat3::from_cols`):**
   - [https://docs.rs/glam/0.29.3/glam/f32/struct.Mat3.html](https://docs.rs/glam/0.29.3/glam/f32/struct.Mat3.html)
5. **W3C WebGPU Shading Language (WGSL) Spesifikasyonu (`Memory Layout`, `Address Space Layout Constraints`):**
   - [https://www.w3.org/TR/WGSL/](https://www.w3.org/TR/WGSL/)
6. **`wgpu` v30.0.1 / v30.0.2 Resmi Dokümantasyonu (`Queue::write_texture`, `TextureFormat::Rgba16Float`, `Rgba8Unorm`):**
   - [https://docs.rs/wgpu/30.0.1/wgpu/struct.Queue.html#method.write_texture](https://docs.rs/wgpu/30.0.1/wgpu/struct.Queue.html#method.write_texture)
   - [https://docs.rs/wgpu/30.0.1/wgpu/enum.TextureFormat.html#variant.Rgba16Float](https://docs.rs/wgpu/30.0.1/wgpu/enum.TextureFormat.html#variant.Rgba16Float)
7. **Projective Homography / DLT Birincil Kaynakları:**
   - Paul S. Heckbert, *Fundamentals of Texture Mapping and Image Warping* (UC Berkeley, 1989): [http://www.cs.cmu.edu/~ph/texfund/texfund.pdf](http://www.cs.cmu.edu/~ph/texfund/texfund.pdf)
   - OpenCV `imgproc` / `calib3d` (`getPerspectiveTransform`, `findHomography` DLT): [https://docs.opencv.org/4.x/da/d54/group__imgproc__transform.html](https://docs.opencv.org/4.x/da/d54/group__imgproc__transform.html)

### Yerel Kod Tabanı & Sözleşme Kaynakları (`2026-11-22`)

- Bağımlılıklar: [Cargo.lock](../../Cargo.lock) (`glam = "0.29.3"`, `lyon_tessellation = "1.0.23"`, `naga = "30.0.2"`, `png = "0.18.1"`, `wgpu = "30.0.2"`), [crates/frameshift-media/Cargo.toml](../../crates/frameshift-media/Cargo.toml), [crates/frameshift-vector/Cargo.toml](../../crates/frameshift-vector/Cargo.toml), [crates/frameshift-render/Cargo.toml](../../crates/frameshift-render/Cargo.toml)
- Şema ve doğrulama: [crates/frameshift-schema/src/graph.rs](../../crates/frameshift-schema/src/graph.rs), [crates/frameshift-schema/src/project.rs](../../crates/frameshift-schema/src/project.rs), [crates/frameshift-schema/src/error.rs](../../crates/frameshift-schema/src/error.rs)
- Medya & çözümleyici: [crates/frameshift-media/src/frame.rs](../../crates/frameshift-media/src/frame.rs), [crates/frameshift-media/src/ffmpeg.rs](../../crates/frameshift-media/src/ffmpeg.rs), [crates/frameshift-core/src/resolver.rs](../../crates/frameshift-core/src/resolver.rs)
- Vektör & GPU kompozitör: [crates/frameshift-vector/src/canvas.rs](../../crates/frameshift-vector/src/canvas.rs), [crates/frameshift-vector/src/shape3d.rs](../../crates/frameshift-vector/src/shape3d.rs), [crates/frameshift-render/src/compositor.rs](../../crates/frameshift-render/src/compositor.rs), [crates/frameshift-render/src/pipelines.rs](../../crates/frameshift-render/src/pipelines.rs), [crates/frameshift-render/src/uniforms.rs](../../crates/frameshift-render/src/uniforms.rs), [crates/frameshift-render/src/vector_text.rs](../../crates/frameshift-render/src/vector_text.rs), [crates/frameshift-render/shaders/transform.wgsl](../../crates/frameshift-render/shaders/transform.wgsl), [crates/frameshift-render/shaders/color_basic.wgsl](../../crates/frameshift-render/shaders/color_basic.wgsl)
- Referans & sözleşmeler: [VISUAL_REFERENCES.md](../../VISUAL_REFERENCES.md), [brain/specs/project-schema.md](../specs/project-schema.md), [brain/specs/render-graph.md](../specs/render-graph.md), [brain/specs/effect-shader-api.md](../specs/effect-shader-api.md)

---

## Doğrudan Gözlemler (Kanıtlı)

1. **Mevcut Bağımlılık Envanteri (`Cargo.lock` & Crate `Cargo.toml`):**
   - `frameshift-media` doğrudan `png = "0.18"` (`0.18.1`) içerir; [frame.rs](../../crates/frameshift-media/src/frame.rs#L202-L257) içinde `Rgba8Frame::load_png` / `decode_png_reader` hâlihazırda `Transformations::normalize_to_color8() | Transformations::ALPHA` ile RGB, RGBA, Grayscale, GrayscaleAlpha, Indexed ve 16-bit PNG'leri `AlphaMode::Straight` `Rgba8Frame`'e çözmektedir.
   - `frameshift-vector` doğrudan `lyon_tessellation = "1.0"` (`1.0.23`) ve `ttf-parser = "0.25"` içerir; [shape3d.rs](../../crates/frameshift-vector/src/shape3d.rs#L6-L9) içinde `FillTessellator`, `StrokeTessellator`, `Path::builder()` aktif kullanılmaktadır.
   - `frameshift-render` doğrudan `wgpu = "30.0.1"` (`30.0.2`), `glam = { version = "0.29", features = ["bytemuck"] }` (`0.29.3`) ve `bytemuck = "1.25.2"` içerir.

2. **`SourceFit` ve Video/Image Çözümleme (`graph.rs`, `ffmpeg.rs`, `resolver.rs`):**
   - [graph.rs](../../crates/frameshift-schema/src/graph.rs#L84-L89) `SourceFit { Stretch, Cover, Contain }` tanımlar; [NodeKind::source_fit](../../crates/frameshift-schema/src/graph.rs#L967-L972) şu anda yalnızca `SourceVideo` için `Some(*fit)` döndürür.
   - [project.rs](../../crates/frameshift-schema/src/project.rs#L17-L21) `AssetKind::Image` varyantını ve [resolver.rs](../../crates/frameshift-core/src/resolver.rs#L376) `match_synthetic_asset` içinde `AssetKind::Video | AssetKind::Image` eşlemesini zaten barındırır; ancak `NodeKind::SourceImage` henüz `NodeKind` enum'unda tanımlı değildir ([project-schema.md](../specs/project-schema.md#L117) ve [render-graph.md](../specs/render-graph.md#L61) içinde gelecek genişleme yuvası olarak ayrılmıştır).
   - [resolver.rs](../../crates/frameshift-core/src/resolver.rs#L117-L125) disk varlıklarında `FfmpegBridge::probe_media_info` ile `PtsIndex` oluşturmaya zorlar; [ffmpeg.rs](../../crates/frameshift-media/src/ffmpeg.rs#L248-L280) içindeki `probe_media_info` ise `avg_frame_rate` / `time_base` ve `frames` listesi beklediği için tekil statik JPEG/WebP/PNG dosyalarında `ffprobe` ek süreci gereksizdir (ayrıca tekil görsellerde `PtsIndex` anlamsızdır).
   - [ffmpeg.rs](../../crates/frameshift-media/src/ffmpeg.rs#L852-L868) `append_fitted_output` fonksiyonu `Stretch` (`-s WxH`), `Cover` (`force_original_aspect_ratio=increase,crop=...`) ve `Contain` (`force_original_aspect_ratio=decrease,format=rgba,pad=...:color=0x00000000`) filtrelerini zaten uygular.

3. **DAG Değerlendirme, `MotionBlurShutter` ve `SafeExpr` (`compositor.rs`, `project.rs`, `expr.rs`):**
   - [compositor.rs](../../crates/frameshift-render/src/compositor.rs#L1152-L1375) `evaluate_subgraph` fonksiyonu düğümleri topolojik sırayla (`full_topo`) yürütür; `MotionBlurShutter` kendi alt-konisini (`collect_direct_cone`) birden fazla alt-zamanda (`sub_tick`) tekrar değerlendirir.
   - [compositor.rs](../../crates/frameshift-render/src/compositor.rs#L3050-L3199) `upload_source_frame`, her çağrıda geçici bir `staging_tex` oluşturup `queue.write_texture` + `color_basic` (`SrgbToLinear`) GPU geçişi çalıştırır. Statik bir `SourceImage` için aynı işlemi her karede (ve motion blur'un her `sub_samples` alt-örneğinde) tekrarlamak yerine GPU üzerinde `Rgba16Float` LinearRec709 premultiplied doku önbelleği kullanılmamaktadır.
   - [project.rs](../../crates/frameshift-schema/src/project.rs#L196-L420) `bindable_param_names` ve [compositor.rs](../../crates/frameshift-render/src/compositor.rs#L398-L890) `apply_animated_parameter`, `ParamBindingDef` ve `EventBoundAnimation` hedeflerini beyaz listeyle doğrular ve düğüm alanlarına `f32` olarak yazar.

4. **WGSL Uniform Hizalama ve Naga Güvenliği (`effect-shader-api.md`, `transform.wgsl`, `W3C WGSL`):**
   - [effect-shader-api.md](../specs/effect-shader-api.md#L34-L48) tüm tek-girdili efektlerin `@group(0)` üzerinde `@binding(0) t_source`, `@binding(1) s_linear`, `@binding(2) params: var<uniform>` düzenini ve **16-byte katı `#[repr(C)]` / `vec4` paketleme** kuralını zorunlu kılar.
   - W3C WGSL spesifikasyonu uyarınca `var<uniform>` içindeki `mat3x3<f32>` tipi `3` adet `vec3<f32>` sütunundan oluşur ve her sütunun hizalaması **`16 byte`** (`48 byte` toplam) olduğundan Rust tarafındaki sıkı paketli `glam::Mat3` (`36 byte`) ile doğrudan eşleşmez.
   - [transform.wgsl](../../crates/frameshift-render/shaders/transform.wgsl#L59-L86) `sample_catmull_rom` fonksiyonu sabit `0..4` döngüleri (`4x4` texel load, negatif ringing clamp) ile halihazırda Naga-doğrulanmış ve `Rgba16Float` HDR uyumlu çalışır.
   - [lyon_tessellation](https://docs.rs/lyon_tessellation/latest/lyon_tessellation/) belgelerine göre `StrokeTessellator` şerit üçgenleri üretir ve yarı saydam çizgilerin kendi üzerine binen köşe/birleşimlerinde çift sayım (double-blending) oluşturabilir; buna karşılık [vector_text.rs](../../crates/frameshift-render/src/vector_text.rs#L97-L154) ve [canvas.rs](../../crates/frameshift-vector/src/canvas.rs) içindeki analitik SDF kapsama (`smoothstep`) sıfır self-overlap artefaktı ile çalışmaktadır.

---

## Çıkarımlar ve Öneriler

Aşağıdaki bölümler doğrudan kodlanan sözleşmelerle uyumlu **uygulama mimarisi önerisidir**. Tüm yeni şema alanları `#[serde(default)]` taşır ve mevcut M1–M5 / Faz 6.1–6.3 projelerini kırmaz.

### Bölüm 1: `NodeKind::SourceImage` (0 Girdi — Statik Görsel & GPU Renk Önbelleği)

#### 1.1. Şema (`crates/frameshift-schema/src/graph.rs` & `project.rs`)
```rust
/// Decodes a still image asset (PNG/JPEG/WebP/TIFF/BMP) into Linear Rec.709 premultiplied RGBA (0 inputs).
#[serde(alias = "source_image")]
SourceImage {
    asset_id: String,
    #[serde(default = "default_display_srgb")] // default: ColorSpace::DisplaySrgb
    color_space: ColorSpace,
    #[serde(default)]
    fit: SourceFit,
    #[serde(default = "default_one")]
    opacity: f32,
},
```
- **Varsayılan `color_space` Notu:** Video kliplerde varsayılan `ColorSpace::LinearRec709` ya da açık `DisplaySrgb` verilebiliyor; diskteki PNG/JPEG/WebP görsellerinin ezici çoğunluğu sRGB olduğu için `SourceImage.color_space` varsayılanının `ColorSpace::DisplaySrgb` (`default_display_srgb()`) olması en güvenli davranıştır (explicit JSON alanı varsa korunur).
- **`NodeKind::expected_input_count`:** `0` (`DisabledPassthrough::Clear`).
- **`NodeKind::source_fit`:** `NodeKind::SourceImage { fit, .. } => Some(*fit)` eklenmelidir.
- **`bindable_param_names` (`SafeExpr` beyaz listesi):** `&["opacity"]` (`apply_animated_parameter` içinde `opacity` güncellenir).
- **`Project::validate()`:** `SourceImage.asset_id` için `self.asset(asset_id)` varlığı ve `AssetKind != AssetKind::Audio` denetimi önerilir (yoksa `SchemaError::MissingAsset`).

#### 1.2. Dekodlama Hattı (`frameshift-media`) — 0 Yeni Crate
`SourceImage` için iki kademeli, deterministik dekodlama hattı kurulur (`crates/frameshift-media/src/image.rs` veya `ffmpeg.rs` + `frame.rs` eklentisi):
1. **Hızlı Yol (Saf Rust `png = "0.18.1"`):**
   - Dosyanın ilk 8 byte'ı PNG imzası (`[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]`) veya uzantısı `.png` ise `Rgba8Frame::load_png(path, Flicks::ZERO)` çağrılır. (`0` subprocess spawn!).
   - Ardından hedef `(width, height)` ve `SourceFit` (`Stretch | Cover | Contain`) için saf-Rust deterministik **`fit_rgba8_frame(src: &Rgba8Frame, dst_w: u32, dst_h: u32, fit: SourceFit) -> Result<Rgba8Frame, MediaError>`** çalıştırılır:
     - `src.width == dst_w && src.height == dst_h`: kopyasız doğrudan dönüş.
     - `Stretch`: tam `(dst_w, dst_h)` boyutuna deterministik bilinear yeniden örnekleme (premultiplied ara uzayda `rgb*a` enterpolasyonu yapılıp tekrar straight `u8`'e bölünür; şeffaf kenarlarda siyah halo oluşmaz).
     - `Cover`: `scale = max(dst_w / src_w, dst_h / src_h)` ile ölçekleyip merkezden `(dst_w, dst_h)` kırpar.
     - `Contain`: `scale = min(dst_w / src_w, dst_h / src_h)` ile ölçekleyip kalan kenarları şeffaf siyah `[0, 0, 0, 0]` ile doldurur.
2. **Fallback Yolu (`ffmpeg` tek-kare decode — JPEG / WebP / TIFF / BMP):**
   - PNG dışındaki formatlar için `FfmpegBridge::decode_image_frame_with_fit(path: &Path, width: u32, height: u32, fit: SourceFit) -> Result<Rgba8Frame, MediaError>` eklenir:
     ```text
     ffmpeg -v error -nostdin -i <path> -frames:v 1 [<append_fitted_output>] -pix_fmt rgba -f rawvideo pipe:1
     ```
   - Kritik fark: Video yolunun aksine `probe_media_info` / `PtsIndex` **çağrılmaz** (`Flicks::ZERO`, `AlphaMode::Straight`).

#### 1.3. Çift Katmanlı Önbellek (`MediaResolver` + `GpuCompositor`)
- **CPU / Disk Önbelleği (`crates/frameshift-core/src/resolver.rs`):**
  - `ProjectFrameProvider::provide_source_frame` içinde `asset.kind == AssetKind::Image` için `provide_image_frame(&mut cache, asset, width, height, fit)` yolu çalıştırılır; `LruFrameCache` anahtarı `format!("img:{}@{}x{}:{}", asset.id, width, height, source_fit_cache_name(fit))` ve `frame_key = 0` olur.
- **GPU `LinearRec709` Premultiplied Önbellek (`crates/frameshift-render/src/compositor.rs`):**
  - `GpuCompositor` içine `image_gpu_cache: HashMap<ImageGpuKey, Arc<CachedImageTexture>>` eklenir (`ImageGpuKey = (String /*asset_id*/, u32 /*w*/, u32 /*h*/, u8 /*fit*/, u8 /*color_space*/)`).
  - İlk erişimde `provider.provide_source_frame(asset_id, Flicks::ZERO, width, height, fit)` sonucu alınan `Rgba8Frame`, `upload_source_frame` üzerinden kalıcı bir `Rgba16Float` (Linear Rec.709, premultiplied alpha) dokusuna dönüştürülür ve cache'e konur.
  - Sonraki tüm karelerde (ve `MotionBlurShutter` alt-örneklerinde) disk I/O, CPU->GPU `write_texture` ve `sRGB->Linear` shader geçişi **0 maliyetle atlanır**; yalnızca önbellekteki dokudan `TexturePool`'dan alınan hedef dokuya `copy_texture_to_texture` (veya `opacity < 0.999` ise premultiplied alfa çarpanlı hızlı `blend`/`color_scale` pass'i) yapılır. `opacity <= 0.001` ise provider çağrılmadan şeffaf siyah temizlenir.

---

### Bölüm 2: `NodeKind::Precomp` (0 veya N Girdi — Kapsüllenmiş Alt-Grafik)

[VISUAL_REFERENCES.md](../../VISUAL_REFERENCES.md) içindeki `tastxv2` (kart/sahne grupları) ve `_gutsz` (kolaj düzeni, yeniden kullanılabilir kompozit katmanlar) gereksinimlerini karşılamak üzere tasarlanır.

#### 2.1. Şema ve Girdi Sözleşmesi (`crates/frameshift-schema/src/graph.rs`)
```rust
pub const MAX_PRECOMP_DEPTH: usize = 8;
pub const MAX_PRECOMP_INPUTS: usize = 16;

/// Encapsulated nested render sub-graph with independent time offset/scale and opacity (0..=16 inputs).
#[serde(alias = "precomp")]
Precomp {
    sub_graph: RenderGraphDef,
    #[serde(default)]
    time_offset_ticks: i64,
    #[serde(default = "default_one")]
    time_scale: f32,
    #[serde(default = "default_one")]
    opacity: f32,
},

/// Reads an external input slot (`inputs[slot]`) passed into a `Precomp` node (0 inputs inside `sub_graph`).
#[serde(alias = "precomp_input")]
PrecompInput {
    #[serde(default)]
    slot: usize,
},
```
- **Neden `PrecompInput { slot }`?**
  - `Precomp.inputs` boşsa (`inputs: []`), alt-grafik tamamen kendi iç kaynaklarını (`SourceVideo`, `SourceImage`, `VectorPathHud`, `Scene3D` vb.) kullanan **0-girdili** özerk bir alt-kompozisyondur.
  - `Precomp.inputs` doluysa (`inputs: [clip_a, mask_b]`, max `16`), alt-grafik içindeki `PrecompInput { slot: 0 }` ve `PrecompInput { slot: 1 }` düğümleri üst grafiğin önceden değerlendirilmiş `PooledTexture` çıktılarını sıfır kopya (`TextureView` referansı veya havuz kopyası) ile alt-grafiğin içine taşır. Böylece `Precomp`, hem bağımsız kolaj sahnesi hem de yeniden kullanılabilir efekt/çerçeve rig'i olarak çalışır.
- **`accepts_input_count` ve `DisabledPassthrough`:**
  - `NodeKind::Precomp { .. }`: `count <= MAX_PRECOMP_INPUTS` (`0..=16`). `expected_input_count()` `0` döndürür; ancak `GraphNodeDef::disabled_passthrough` içinde:
    - `Precomp` için `self.inputs.is_empty()` ise `DisabledPassthrough::Clear`, `!self.inputs.is_empty()` ise `DisabledPassthrough::Input(0)` döndürülür.
  - `NodeKind::PrecompInput { .. }`: `0` girdi (`DisabledPassthrough::Clear`).
- **`bindable_param_names` (`SafeExpr` beyaz listesi):**
  - `Precomp`: `&["time_offset_ticks", "time_scale", "opacity"]` (`time_offset_ticks` için `value.round() as i64`).

#### 2.2. Statik Döngü, Derinlik ve Referans Doğrulaması (`graph.rs` & `project.rs`)
- `RenderGraphDef::validate_graph(&self)` -> `self.validate_graph_with_depth(0)`'a delege edilir:
  1. `depth > MAX_PRECOMP_DEPTH` (`8`) ise yeni `SchemaError::PrecompDepthExceeded { node_id, depth, max_depth: MAX_PRECOMP_DEPTH }` hatası döner. Bu, doğrudan veya dolaylı sonsuz alt-grafik gömülmesini statik olarak engeller.
  2. `time_scale` sonlu olmalıdır (`time_scale.is_finite()`).
  3. `sub_graph.validate_graph_with_depth(depth + 1)?` çağrılır; böylece içteki DAG döngüleri, eksik düğümler ve arity hataları yakalanır.
  4. `sub_graph` içindeki her `NodeKind::PrecompInput { slot }` için `slot < node.inputs.len()` zorunludur; aksi halde `SchemaError::InvalidPrecomp { node_id, message }` döner.
  5. Üst düzey `RenderGraphDef` içinde (en dış kök grafikte) `PrecompInput` bulunursa `SchemaError::InvalidPrecomp` ile reddedilir.
- **`Project::validate()` Genişlemesi:**
  - `ParamBindingDef` ve `EventBoundAnimation` doğrulaması yalnızca kök `self.graph` yerine özyinelemeli `find_node_in_graphs(&self.graph, node_id)` ile alt-grafiklerdeki düğümleri de bulabilmelidir *(Not: alt-grafik düğümlerine benzersiz `NodeId` atanması önerilir; çakışmayı önlemek için `validate_unique_node_ids_across_precomps` veya `precomp_id/node_id` kuralı eklenebilir)*.

#### 2.3. Seek-Independent Alt-Zaman Eşlemesi ve `MotionBlurShutter` Uyumu (`compositor.rs`)
- **Zaman Eşleme Fonksiyonu (`remap_precomp_tick`):**
  ```rust
  pub fn remap_precomp_tick(sub_tick: Flicks, time_offset_ticks: i64, time_scale: f32) -> Flicks {
      if (time_scale - 1.0).abs() <= f32::EPSILON {
          return Flicks(sub_tick.0.saturating_add(time_offset_ticks));
      }
      let scaled = ((sub_tick.0 as f64) * f64::from(time_scale)).round();
      let scaled_i64 = if scaled.is_finite() {
          scaled.clamp(i64::MIN as f64, i64::MAX as f64) as i64
      } else {
          0
      };
      Flicks(scaled_i64.saturating_add(time_offset_ticks))
  }
  ```
  - `time_scale == 1.0` iken kayan nokta yuvarlama hatası olmadan tam `i64` `Flicks` eklemesi yapılır.
- **Yürütme Akışı:**
  1. `opacity <= 0.001` ise `sub_graph` hiç değerlendirilmez; doğrudan şeffaf siyah `[0, 0, 0, 0]` döndürülür.
  2. `precomp_inputs: &[&PooledTexture]` dilimi hazırlanır (`node.inputs` sırasıyla).
  3. `evaluate_subgraph_inner(..., sub_graph, &sub_topo, &sub_graph.output_node, local_tick, ..., Some(precomp_inputs), depth + 1, encoder)` çağrılır.
  4. İçerideki her düğüm tamamen `local_tick` üzerinden değerlendirilir; hiçbir kareler-arası kalıcı state tutulmaz. Dış grafikte bir `MotionBlurShutter` varsa her alt-örnek anı $t_i$ kendi `local_tick_i`'sine dönüşür; böylece `Precomp` içindeki hareketler dış motion blur içine doğal olarak akar.
  5. Çıktı `opacity < 0.999` ise `Blend` / premultiplied alfa ölçekleme geçişiyle `opacity.clamp(0.0, 1.0)` uygulanır.

---

### Bölüm 3: `NodeKind::VectorPathHud` (0 Girdi — Vektör Eğri, Dash, Trim-Path & Veri HUD)

[VISUAL_REFERENCES.md](../../VISUAL_REFERENCES.md) `zwxeditor` referansındaki karakter çevresi ince çizgiler, noktalı bağlantı eğrileri, düğüm noktaları ve küçük sayısal/etiket HUD grafikleri için tasarlanır.

#### 3.1. Şema Tanımı (`crates/frameshift-schema/src/graph.rs` & `project.rs`)
```rust
/// Cubic Bézier curve segment in normalized `[0, 1]` canvas coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CubicBezierSegmentDef {
    pub p0: [f32; 2],
    pub c1: [f32; 2],
    pub c2: [f32; 2],
    pub p1: [f32; 2],
}

/// Anti-aliased 2D vector path & HUD overlay generator with trim-path, dashes,
/// node junction dots, and numeric data readout (0 inputs).
#[serde(alias = "vector_path_hud")]
VectorPathHud {
    #[serde(default)]
    points_norm: Vec<[f32; 2]>,
    #[serde(default)]
    bezier_segments: Vec<CubicBezierSegmentDef>,
    #[serde(default)]
    closed: bool,
    #[serde(default = "default_hud_stroke_width")] // 0.003
    stroke_width_norm: f32,
    #[serde(default = "default_white_rgba")]
    stroke_rgba: [f32; 4],
    #[serde(default)]
    fill_rgba: [f32; 4], // [0, 0, 0, 0] default (no fill)
    #[serde(default)]
    dash_len_norm: f32,
    #[serde(default)]
    gap_len_norm: f32,
    #[serde(default)]
    dash_offset_norm: f32,
    #[serde(default)]
    trim_start: f32, // [0.0..=1.0]
    #[serde(default = "default_one")]
    trim_end: f32,   // [0.0..=1.0]
    #[serde(default)]
    node_dot_radius_norm: f32,
    #[serde(default)]
    data_value: f32,
    #[serde(default)]
    data_label: String,
    #[serde(default = "default_one")]
    opacity: f32,
},
```
- **Çözünürlük Bağımsızlığı ([render-graph.md §3.1](../specs/render-graph.md#L65)):**
  - `points_norm` ve `bezier_segments` tuval `[0..1] x [0..1]` uzayındadır (`x * width`, `y * height`).
  - Tüm kalınlık ve uzunluk oranları (`stroke_width_norm`, `dash_len_norm`, `gap_len_norm`, `dash_offset_norm`, `node_dot_radius_norm`) `height as f32` ile çarpılarak `draft (0.5x)` ve `final (1.0x)` profillerinde özdeş görsel oran korunur.
- **`SafeExpr` Beyaz Listesi (`bindable_param_names`):**
  ```rust
  NodeKind::VectorPathHud { .. } => &[
      "stroke_width_norm",
      "dash_len_norm",
      "gap_len_norm",
      "dash_offset_norm",
      "trim_start",
      "trim_end",
      "node_dot_radius_norm",
      "data_value",
      "opacity",
  ]
  ```
  *(Not: `data_value` alanının `Option<f32>` yerine `f32` tutulmasının nedeni mevcut `apply_animated_parameter` sözleşmesinin doğrudan `f32` yazmasıdır; `data_label` boş değilse veya `data_label` formatlı metin taşıyorsa HUD etiketi çizilir — örn. `"VEL: {val}"` şablonu veya `"{label} {data_value:.1}"` kuralı ile `SafeExpr` animasyonu doğrudan sayısal değeri sürer).*

#### 3.2. `lyon_tessellation` + Analitik SDF Hibrit Rasterizasyon Boru Hattı (`frameshift-vector`)
[lyon_tessellation](https://docs.rs/lyon_tessellation/latest/lyon_tessellation/) hem `FillTessellator` hem `StrokeTessellator` sunar; ancak yarı saydam HUD çizgilerinde `StrokeTessellator`'ın şerit birleşimlerindeki öz-örtüşme (self-overlap) artefaktını önlemek ve kesintisiz alt-piksel AA + dinamik `trim_start..trim_end` + `dash` elde etmek için aşağıdaki 4 aşamalı boru hattı önerilir:

1. **Yol İnşası ve Düzleştirme (`lyon_tessellation::path::Path`):**
   - `bezier_segments` boş değilse `Path::builder()` ile `begin(p0)` -> `cubic_bezier_to(c1, c2, p1)` -> `end(closed)` inşa edilir; aksi halde `points_norm` üzerinden `line_to` ile `Path` kurulur.
   - `lyon_tessellation::path::iterator::Flattened` (`tolerance = 0.25_f32` piksel) ile kübik Bézier eğrileri deterministik doğru parçası zincirine (polyline) dönüştürülür.
2. **Yay Uzunluğu Parametrizasyonu, Trim-Path ve Dash Dilimleme:**
   - Düzleştirilmiş noktalar üzerinde kümülatif yay uzunluğu $s_0 = 0, s_1, \dots, s_M = L$ hesaplanır.
   - `t0 = trim_start.clamp(0.0, 1.0)`, `t1 = trim_end.clamp(0.0, 1.0)`. `t1 <= t0` ise çizgi/nokta çizilmez. Aksi halde $[t_0 L, t_1 L]$ aralığındaki alt-polyline, başlangıç ve bitiş kesim noktaları doğrusal enterpolasyonla hesaplanarak çıkarılır.
   - `dash_len_px > 0.5 && gap_len_px > 0.5` ise kırpılmış alt-polyline üzerinde `period = dash_len_px + gap_len_px` boyunca `(s - dash_offset_px).rem_euclid(period) < dash_len_px` aralıkları ayrı kesintisiz alt-segmentlere bölünür.
3. **Dolgu + Kontur + Düğüm Noktası + HUD Etiket Çizimi (`VectorCanvas` — Linear Rec.709 Premultiplied):**
   - **Dolgu (`closed && fill_rgba[3] > 1e-6`):** Orijinal `Path` üzerine `lyon_tessellation::FillTessellator::tessellate_path` (`FillOptions::tolerance(0.25).with_fill_rule(FillRule::EvenOdd)`) uygulanır; üretilen üçgenler [canvas.rs](../../crates/frameshift-vector/src/canvas.rs#L6) içindeki `4x4` alt-piksel deterministik kapsama (veya kenar fonksiyonu rasterizer) ile `VectorCanvas`'a yazılır.
   - **Çizgi & Dash (`stroke_width_px > 0.0 && stroke_rgba[3] > 1e-6`):** Dilimlenmiş dash alt-segmentlerinin her piksel bounding-box'ı için en yakın segment mesafesi `d = min_seg dist(p, seg)` hesaplanır ve `cov = smoothstep(half_w + 0.75, half_w - 0.75, d)` ile tek geçişte harmanlanır. Tek geçişli `min_dist` yaklaşımı köşelerde ve dar açılı bükümlerde sıfır double-blending ve pürüzsüz Yuvarlak/Butt AA sağlar.
   - **Düğüm Noktaları (`node_dot_radius_norm > 0.0`):** Orijinal köşe noktaları (veya aktif `trim_start` / `trim_end` uç noktaları) üzerine tam daire SDF (`dist(p, c) <= r_px`) ile bağlantı noktası diskleri çizilir.
   - **Veri Etiketi (`!data_label.is_empty()`):** İlk görünür noktanın (veya `trim_end` ucunun) yanına `[+8px, -12px]` ofsetle mevcut `frameshift-vector` deterministik metin rasterizer'ı (`draw_vector_text` / `render_label_badge`) ile `data_label` + biçimlenmiş `data_value` (örn. `format!("{data_label}{data_value:.2}")`) çizilir.
4. **Çıktı & Önbellek (`frameshift-core/src/resolver.rs`):**
   - `VectorCanvas::to_rgba16f_frame(pts, AlphaMode::Premultiplied)` -> `SourceFrame::rgba_f32_premul` -> `GpuCompositor::upload_source_frame(..., ColorSpace::LinearRec709, ...)`.
   - Dinamik `ParamBindingDef` / `EventBoundAnimation` bağlı olmayan statik `VectorPathHud` düğümleri `vector_path_hud://<node_id>` anahtarıyla `MediaResolver` önbelleğinden sunulabilir (mevcut `vector_text://` deseninin birebir aynısı).

---

### Bölüm 4: `NodeKind::CornerPin` (1 Girdi) ve `NodeKind::GridWarp` (1 Girdi)

[VISUAL_REFERENCES.md](../../VISUAL_REFERENCES.md) içindeki `tastxv2` (perspektifli grafik/metin kartları), `zwxeditor` ve `_gutsz` (dama zemin/perspektif ve dalgalı/kıvrımlı 2D grid deformasyonu) gereksinimlerini karşılar. Her ikisi de `1` girdili saf WGSL fragment shader düğümüdür.

#### 4.1. `NodeKind::CornerPin` — Şema, DLT/Homografi Matematiği ve WGSL Düzeni

##### Şema (`crates/frameshift-schema/src/graph.rs` & `project.rs`)
```rust
/// 4-corner projective perspective warp in normalized `[0, 1]` canvas coordinates (1 input).
#[serde(alias = "corner_pin")]
CornerPin {
    #[serde(default = "default_corner_tl")] // [0.0, 0.0]
    tl: [f32; 2],
    #[serde(default = "default_corner_tr")] // [1.0, 0.0]
    tr: [f32; 2],
    #[serde(default = "default_corner_br")] // [1.0, 1.0]
    br: [f32; 2],
    #[serde(default = "default_corner_bl")] // [0.0, 1.0]
    bl: [f32; 2],
    #[serde(default)]
    filter: SampleFilter,
    #[serde(default = "default_one")]
    opacity: f32,
},
```
- **`SafeExpr` Beyaz Listesi (`bindable_param_names`):**
  `&["tl_x", "tl_y", "tr_x", "tr_y", "br_x", "br_y", "bl_x", "bl_y", "opacity"]`.

##### Kapalı-Form DLT / Projective Homografi ve `glam::Mat3::try_inverse`
Kaynak birim kare köşeleri $s_0=(0,0)\to\text{tl}=(x_0,y_0)$, $s_1=(1,0)\to\text{tr}=(x_1,y_1)$, $s_2=(1,1)\to\text{br}=(x_2,y_2)$, $s_3=(0,1)\to\text{bl}=(x_3,y_3)$ için Paul Heckbert (1989) *unit-square-to-quad* projektif eşlemesi kapalı formda $O(1)$ sürede çözülür:

$$\Delta x_1 = x_1 - x_2, \quad \Delta x_2 = x_3 - x_2, \quad \Sigma x = x_0 - x_1 + x_2 - x_3$$
$$\Delta y_1 = y_1 - y_2, \quad \Delta y_2 = y_3 - y_2, \quad \Sigma y = y_0 - y_1 + y_2 - y_3$$
$$\text{det} = \Delta x_1 \Delta y_2 - \Delta x_2 \Delta y_1$$
$$g = \frac{\Sigma x \Delta y_2 - \Sigma y \Delta x_2}{\text{det}}, \qquad h = \frac{\Delta x_1 \Sigma y - \Delta y_1 \Sigma x}{\text{det}}$$

Bu katsayılardan ileri homografi matrisi $H_{\text{fwd}}$ (`glam::Mat3::from_cols`):
- Sütun 0: $[a, d, g]^T$, burada $a = x_1 - x_0 + g x_1$, $d = y_1 - y_0 + g y_1$
- Sütun 1: $[b, e, h]^T$, burada $b = x_3 - x_0 + h x_3$, $e = y_3 - y_0 + h y_3$
- Sütun 2: $[c, f, 1.0]^T$, burada $c = x_0$, $f = y_0$

Ardından GPU'daki ters örnekleme (`dst_uv -> src_uv`) için CPU'da `H_fwd.try_inverse()` çağrılır:
- `det.abs() <= 1e-8` veya `H_fwd.try_inverse()` `None` dönerse (örn. animasyon sırasında dörtgen düz bir çizgiye çöktüyse) `valid = 0.0` bayrağı gönderilir ve shader çökmek yerine şeffaf siyah (`vec4(0.0)`) üretir.
- `H_fwd`'nin köşe projektif ağırlıkları $w_0=1$, $w_1=g+1$, $w_2=g+h+1$, $w_3=h+1$ tümüyle $> 0$ ise dörtgen konvekstir ve iç bölgede `p.z > 0.0` garanti edilir; konkav/öz-kesişen köşelerde `p.z > 1e-6` denetimi ufuk çizgisi (vanishing line) ötesindeki ters yansıma artefaktını önler.

##### 16-Byte Hizalı `CornerPinUniform` & `@group(0)` WGSL Sözleşmesi
W3C WGSL `var<uniform>` içinde `mat3x3<f32>` sütunlarının `16 byte` adımlı olması nedeniyle doğrudan `glam::Mat3` (`36 byte`) kullanılmaz; bunun yerine 3 sütun `vec4<f32>` olarak paketlenir (**toplam `64 byte`, `64 % 16 == 0`**):

```rust
/// Uniform buffer for `corner_pin.wgsl` (64 bytes, 16-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct CornerPinUniform {
    /// Columns 0..2 of inverse homography `H_inv` stored as `[m0j, m1j, m2j, 0.0]`.
    pub inv_cols: [[f32; 4]; 3],
    /// `x` = opacity `[0..1]`, `y` = filter_mode (`0.0`=Bilinear, `1.0`=CatmullRom),
    /// `z` = valid (`1.0` or `0.0`), `w` = reserved.
    pub params: [f32; 4],
}
```
- **WGSL Bağlama (`@group(0)` — `Effect1D` şablonu):**
  - `@binding(0) var t_source: texture_2d<f32>;`
  - `@binding(1) var s_linear: sampler;`
  - `@binding(2) var<uniform> u: CornerPinUniform;`
- **Fragment Çekirdeği (`corner_pin.wgsl`):**
  ```wgsl
  let h_inv = mat3x3<f32>(u.inv_cols[0].xyz, u.inv_cols[1].xyz, u.inv_cols[2].xyz);
  let p = h_inv * vec3<f32>(in.uv, 1.0);
  let valid = u.params.z > 0.5 && p.z > 1e-6;
  let src_uv = p.xy / select(1.0, p.z, p.z > 1e-6);
  let in_bounds = valid
      && src_uv.x >= 0.0 && src_uv.x <= 1.0
      && src_uv.y >= 0.0 && src_uv.y <= 1.0;
  ```
  `in_bounds` doğruysa `Bilinear` (`textureSampleLevel`) veya `CatmullRom` (`sample_catmull_rom`, `transform.wgsl` ile aynı 4x4 sabit döngülü fonksiyon) örneklenir ve `opacity` ile çarpılır; değilse `vec4<f32>(0.0)` döner.

---

#### 4.2. `NodeKind::GridWarp` — Şema, Sabit 8x8 Uniform Dizisi ve Naga-Güvenli WGSL

##### Şema (`crates/frameshift-schema/src/graph.rs` & `project.rs`)
```rust
pub const MAX_GRID_WARP_DIM: u32 = 8;
pub const MAX_GRID_WARP_POINTS: usize = 64;

/// 2D control-mesh & harmonic wave UV deformation node (1 input).
#[serde(alias = "grid_warp")]
GridWarp {
    #[serde(default = "default_grid_dim")] // 4 (valid range 2..=8)
    cols: u32,
    #[serde(default = "default_grid_dim")] // 4 (valid range 2..=8)
    rows: u32,
    /// Normalized UV displacement `[dx, dy]` at each `(row * cols + col)` control vertex.
    /// Empty or shorter vectors are zero-padded to `cols * rows`.
    #[serde(default)]
    control_offsets: Vec<[f32; 2]>,
    #[serde(default)]
    wave_amp: [f32; 2],
    #[serde(default = "default_wave_freq")] // [2.0, 2.0]
    wave_freq: [f32; 2],
    #[serde(default)]
    wave_phase: f32,
    #[serde(default = "default_one")]
    mix_amount: f32,
    #[serde(default)]
    filter: SampleFilter,
    #[serde(default = "default_one")]
    opacity: f32,
},
```
- **Doğrulama (`validate_graph_with_depth`):** `cols` ve `rows` `2..=8` aralığında olmalıdır; aksi halde `SchemaError::InvalidGridWarp { node_id, message }` döner. `control_offsets` tüm elemanları sonlu olmalıdır (`dx.is_finite() && dy.is_finite()`).
- **`SafeExpr` Beyaz Listesi (`bindable_param_names`):**
  `&["wave_amp_x", "wave_amp_y", "wave_freq_x", "wave_freq_y", "wave_phase", "mix_amount", "opacity"]`.

##### 16-Byte Hizalı Sabit Uniform Dizisi (`GridWarpUniform` — `1072 bytes`)
WGSL `var<uniform>` içinde dizi elemanlarının 16-byte stride kuralına tam uymak için en fazla `8x8 = 64` kontrol noktası `[f32; 4]` (`vec4<f32>`) slotları olarak taşınır (**`1072 byte`, `1072 % 16 == 0`**):

```rust
/// Uniform buffer for `grid_warp.wgsl` (1072 bytes, 16-byte aligned).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct GridWarpUniform {
    /// `x` = cols (`2..=8`), `y` = rows (`2..=8`), `z` = wave_phase, `w` = mix_amount `[0..1]`.
    pub grid_info: [f32; 4],
    /// `xy` = wave_amp, `zw` = wave_freq.
    pub wave_params: [f32; 4],
    /// `x` = opacity `[0..1]`, `y` = filter_mode (`0.0`=Bilinear, `1.0`=CatmullRom), `zw` = reserved.
    pub render_flags: [f32; 4],
    /// Up to `8 x 8 = 64` control point offsets `(dx, dy, 0.0, 0.0)`.
    pub offsets: [[f32; 4]; 64],
}
```
*(WebGPU taban `maxUniformBufferBindingSize` garantisi `65536` byte olduğundan `1072 byte` tüm `wgpu` backend'lerinde güvenle çalışır).*

##### Bilinear / Catmull-Rom Mesh Deformasyonu & Naga TDR Koruması (`grid_warp.wgsl`)
- `@group(0)` bağlama düzeni `Effect1D` (`@binding(0) t_source`, `@binding(1) s_linear`, `@binding(2) u: GridWarpUniform`).
- **Mesh Enterpolasyonu (`in.uv` $\to$ `disp`):**
  - `cols_i = clamp(i32(round(u.grid_info.x)), 2, 8)`, `rows_i = clamp(i32(round(u.grid_info.y)), 2, 8)`.
  - `gx = clamp(in.uv.x, 0.0, 1.0) * f32(cols_i - 1)`, `gy = clamp(in.uv.y, 0.0, 1.0) * f32(rows_i - 1)`.
  - `ix = clamp(i32(floor(gx)), 0, cols_i - 2)`, `iy = clamp(i32(floor(gy)), 0, rows_i - 2)`, `fx = gx - f32(ix)`, `fy = gy - f32(iy)`.
  - `filter == 0` (**Bilinear**): yalnızca `4` tablo okuması (`(ix, iy)`, `(ix+1, iy)`, `(ix, iy+1)`, `(ix+1, iy+1)`), döngüsüz $O(1)$.
  - `filter == 1` (**Catmull-Rom 4x4**): sınırları `clamp(ix + dx - 1, 0, cols_i - 1)` ile klemplenmiş sabit `for (var j: i32 = 0; j < 4; j = j + 1)` ve `for (var i: i32 = 0; i < 4; i = i + 1)` döngüleri (`4x4 = 16` sabit iterasyon; `MAX_SHADER_LOOP_ITERATIONS = 128` sınırının çok altında, sıfır dinamik döngü).
- **Analitik Dalga Alanı + Ters UV Örneklemesi:**
  - `wave_disp = vec2<f32>(wave_amp.x * sin(in.uv.y * wave_freq.x * TAU + wave_phase), wave_amp.y * cos(in.uv.x * wave_freq.y * TAU + wave_phase))`
  - `src_uv = in.uv - (disp + wave_disp) * clamp(mix_amount, 0.0, 1.0)`
  - `src_uv` `[0, 1]^2` dışındaysa `vec4<f32>(0.0)`; içindeyse `filter` moduna göre Bilinear veya `sample_catmull_rom` örneklenip `opacity` ile çarpılır.

---

## Doğrulanmamış Noktalar

1. **Derleme ve GPU Çalışma Zamanı Testi (Doğrulanmadı):** Bu belge araştırma/şartname aşamasındadır; önerilen Rust tipleri ve `corner_pin.wgsl` / `grid_warp.wgsl` modülleri henüz `cargo test` veya `wgpu` / Naga derleyicisiyle çalıştırılmamıştır.
2. **Referans Video Efekt Kökeni (Doğrulanmadı):** [VISUAL_REFERENCES.md](../../VISUAL_REFERENCES.md) içinde belirtildiği üzere, `zwxeditor` ve `_gutsz` videolarının kaynak proje dosyaları olmadığından bu efektlerin orijinalde tam olarak hangi araç/eklentiyle üretildiği değil, FrameShift içinde hedeflenen davranışları tanımlanmıştır.
3. **`VectorPathHud` Performans Bütçesi (Doğrulanmadı):** Çok sayıda (örn. `> 20`) yüksek çözünürlüklü dinamik `VectorPathHud` düğümünün aynı karede CPU `VectorCanvas` üzerinden rasterize edilmesinin kare başına milisaniye maliyeti henüz ölçülmemiştir; darboğaz görülürse `FillTessellator` üçgenlerinin doğrudan `wgpu` MSAA render pass'ine taşınması açık bir sonraki adımdır.
4. **Windows / Linux GPU Sürücülerinde `Rgba16Float` Önbellek Boyutu (Doğrulanmadı):** Çok sayıda yüksek çözünürlüklü (ör. 4K+) `SourceImage` varlığı aynı projede tutulduğunda `image_gpu_cache` için VRAM üst sınırı / LRU tahliye eşiği henüz belirlenmemiştir.
