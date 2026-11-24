---
title: "Depth-Anything-V2-Small ONNX & Scene3D Depth32Float Enjeksiyonu Araştırma Raporu"
date: 2026-11-24
status: active
tags: ["ai", "render-graph", "3d", "shaders", "schema"]
---

# Depth-Anything-V2-Small ONNX & Scene3D Depth32Float Enjeksiyonu Araştırma Raporu

## Soru / Amaç

FrameShift içindeki 2D monoküler derinlik çıkarımı (`scripts/ai_infer_sidecar.py --task depth`, `frameshift-media/src/ai_inference.rs`) ile `Scene3D` donanım derinlik tamponunun (`Depth32Float`, `scene3d_depth_seed.wgsl`, `fx_dof_coc.wgsl` / `fx_dof_bokeh.wgsl`) gerçek anime kliplerinde kesintisiz, bantlaşmasız ve fiziksel olarak tutarlı çalışması için:

1. **Depth-Anything-V2-Small (Apache-2.0)** ONNX modelinin yerel RTX 4050 (`DmlExecutionProvider`) uyumu, doğru en-boy oranlı ön-işleme, disparity çıkış semantiği ve 8-bit vs 16-bit kanal hassasiyet tasarımını netleştirmek.
2. Mevcut çalışma ağacında prototiplenmiş `Scene3D.depth_plate` (`SceneDepthPlateDef`) ve `scene3d_depth_seed.wgsl` altyapısındaki eksik/kırılgan noktaları (disparity→eye-Z doğrusal olmayan eşleme / `depth_gamma`, `DisplaySrgb` gamma tuzağı, `fx_dof_coc` boş-piksel DoF taşması, `precomp`/`extract_cycle_nodes`/animasyon bağları) belirlemek ve Faz 2 implementasyon reçetesini çıkarmak.

---

## İncelenen Birincil Kaynaklar

### Harici Birincil Kaynaklar
- `DepthAnything/Depth-Anything-V2` (Resmi GitHub deposu — Kod & `ViT-S` ağırlıkları `Apache-2.0`; `ViT-B/L/G` `CC-BY-NC-4.0`): https://github.com/DepthAnything/Depth-Anything-V2 — erişim tarihi: 2026-11-24
- `onnx-community/depth-anything-v2-small` (Hugging Face ONNX dağıtımı — `Apache-2.0`): https://huggingface.co/onnx-community/depth-anything-v2-small — erişim tarihi: 2026-11-24
- `fabio-sim/Depth-Anything-ONNX` (ONNX export ve ONNX Runtime çıkarım referansı — `Apache-2.0`): https://github.com/fabio-sim/Depth-Anything-ONNX — erişim tarihi: 2026-11-24

### Yerel Kod ve Ortam Dosyaları
- [scripts/ai_infer_sidecar.py](file:///C:/Users/Yigit/Documents/FrameShift/scripts/ai_infer_sidecar.py) (`_resolve_hw`, `_run_onnx_frame`, `--task depth`)
- [crates/frameshift-media/src/ai_inference.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-media/src/ai_inference.rs) (`AiTaskKind`, `AiInferenceConfig`, `run_ai_frame_inference`, `run_ai_video_inference`)
- [crates/frameshift-media/src/frame.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-media/src/frame.rs) (`decode_png_reader`, `Rgba8Frame`, `Rgba16fFrame`)
- [crates/frameshift-media/src/ffmpeg.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-media/src/ffmpeg.rs) (`Mp4FrameEncoder`, `ExportEncodingProfile`, `load_image_frame_with_fit`)
- [crates/frameshift-render/shaders/scene3d_depth_seed.wgsl](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/shaders/scene3d_depth_seed.wgsl)
- [crates/frameshift-render/shaders/fx_dof_coc.wgsl](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/shaders/fx_dof_coc.wgsl) & [fx_dof_coc_msaa.wgsl](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/shaders/fx_dof_coc_msaa.wgsl) & [fx_dof_bokeh.wgsl](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/shaders/fx_dof_bokeh.wgsl)
- [crates/frameshift-render/shaders/mask_matte.wgsl](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/shaders/mask_matte.wgsl) & [color_basic.wgsl](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/shaders/color_basic.wgsl)
- [crates/frameshift-render/src/compositor.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/src/compositor.rs) (`evaluate_scene3d`, `apply_animated_parameter`, `upload_source_frame`, `collect_direct_cone`)
- [crates/frameshift-render/src/pipelines.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/src/pipelines.rs) (`build_depth_seed_program`) & [uniforms.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/src/uniforms.rs) (`Scene3DDepthSeedUniform`)
- [crates/frameshift-render/src/provider.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/src/provider.rs) (`SourcePixelData`, `SourceFrame`)
- [crates/frameshift-render/tests/optical_flow_tests.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/tests/optical_flow_tests.rs) (`scene3d_depth_plate_occludes_3d_objects_behind_foreground`)
- [crates/frameshift-schema/src/graph.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-schema/src/graph.rs) (`SceneDepthPlateDef`, `NodeKind::Scene3D`, `validate_dag_nodes`, `extract_cycle_nodes`)
- [crates/frameshift-schema/src/project.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-schema/src/project.rs) (`bindable_param_names`) & [precomp.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-schema/src/precomp.rs)
- [brain/research/2026-11-24-ai-rife-ort-rotoscope-guide.md](file:///C:/Users/Yigit/Documents/FrameShift/brain/research/2026-11-24-ai-rife-ort-rotoscope-guide.md) & [2026-11-24-optical-flow-and-ai-inference-architecture.md](file:///C:/Users/Yigit/Documents/FrameShift/brain/research/2026-11-24-optical-flow-and-ai-inference-architecture.md)

---

## Doğrudan Gözlemler (Kanıtlı)

### 1. Yerel ONNX Ortamı ve `local_media/models/depth_anything_v2_small.onnx` Bulguları
- `local_media/models/` dizininde iki model mevcuttur: `depth_anything_v2_small.onnx` (`99,060,839` bayt ≈ `94.47 MiB`) ve `isnetis.onnx` (`176,069,933` bayt).
- Yerel Python ortamı (`onnxruntime 1.24.4`, `PIL 12.2.0`, `numpy 2.5.2`) `['DmlExecutionProvider', 'CPUExecutionProvider']` sağlayıcılarını sunmaktadır.
- `depth_anything_v2_small.onnx` doğrudan `ort.InferenceSession` ile incelenip çalıştırıldığında:
  - **Giriş tensörü:** `pixel_values`, şekil `['batch_size', 3, 'height', 'width']`, tip `tensor(float)` (dinamik yükseklik/genişlik destekli).
  - **Çıkış tensörü:** `predicted_depth`, şekil `['floor(1.0*batch_size)', '14*floor(height/14)', '14*floor(width/14)']`, tip `tensor(float)`.
  - **RTX 4050 `DmlExecutionProvider` canlı ölçümü:** `518×518` girdi ile ilk soğuk çıkarım `561.7 ms`, ısınmış 5 koşu ortalaması **`83.0 ms/kare` (~12.0 FPS)**; `378×518` (14'ün katı dikdörtgen) dinamik girdi de doğrudan `(1, 378, 518)` çıktısı üreterek hatasız çalıştı.
- **Model Çıkış Semantiği (Birincil Kaynaklar):** Depth Anything V2 göreceli kontrol noktaları **affine-invariant inverse depth (disparity, $d \propto 1/z$)** üretir; yani yüksek sayısal değer **kameraya yakın**, düşük değer **uzak** anlamına gelir (ancak mutlak ölçek ve kayma kare içeriğine göre değişir).

### 2. Mevcut Sidecar (`ai_infer_sidecar.py` & `ai_inference.rs`) Gözlemleri
- [scripts/ai_infer_sidecar.py](file:///C:/Users/Yigit/Documents/FrameShift/scripts/ai_infer_sidecar.py#L50-L106) içinde depth görevi için 4 somut darboğaz/risk tespit edildi:
  1. **En-boy oranı bozulması ve yanlış varsayılan çözünürlük:** `AiInferenceConfig::default()` (`input_size: 512`) nedeniyle `_resolve_hw` dinamik eksenleri `512 // 14 * 14 = 504` olarak ve daima **1:1 kare `(504, 504)`** biçiminde çözüyor (`16:9` veya `9:16` klipler eziliyor; modelin eğitim standardı `518`).
  2. **Kare-başına `1%–99%` yüzdelik normalizasyon pompalanması (Depth Pumping):** `_run_onnx_frame` satır 97–100'de `lo = np.percentile(out, 1.0)` ve `hi = np.percentile(out, 99.0)` her kare için sıfırdan hesaplanıyor. Öne yeni bir el/kılıç girdiğinde sahnedeki tüm arka plan derinliği bir anda kayar; piksel bazlı `temporal_smooth` (satır 183) bu global kaymayı engelleyemez.
  3. **Bilinear büyütme öncesi erken 8-bit kuantalama:** Satır 103'te `norm` dizisi `(out_w, out_h)` hedefine büyütülmeden **önce** `(norm * 255.0).astype(np.uint8)` ile 8-bit'e düşürülüyor; düz gradyanlarda merdiven basamağı (stair-stepping) oluşuyor.
  4. **8-bit raw-pipe + 8-bit `yuv420p` MP4 darboğazı:** Satır 186–192 yalnızca `uint8` (`[d, d, d, 255]`) döndürüyor ve `run_ai_video_inference` ([ai_inference.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-media/src/ai_inference.rs#L383-L390)) çıktıyı `ExportEncodingProfile::default()` (`H264`, `yuv420p`, `Crf(16)`) ile kodluyor. 8-bit TV-range/4:2:0 sıkıştırma 256'dan az ayrık seviye bırakır; `Scene3D` Z-testi ve DoF CoC hesabında titreme yaratır.
- Ayrıca [frame.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-media/src/frame.rs#L214-L216) içindeki `decode_png_reader`, `png::Transformations::normalize_to_color8()` kullandığı için 16-bit derinlik PNG'si okunsa bile 8-bit'e kırpılmaktadır.

### 3. Çalışma Ağacındaki `Scene3D` Depth Enjeksiyonu Durumu (Gözlemlenen Kod)
Çalışma ağacında `git status` kapsamında uncommitted olarak **temel bir `depth_plate` prototipi** halihazırda yazılmış durumdadır:
- **Şema ([graph.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-schema/src/graph.rs#L325-L345)):** `SceneDepthPlateDef { node_id: NodeId, near_z: f32 (0.5), far_z: f32 (6.0), invert: bool }` tanımlanmış ve `NodeKind::Scene3D` içine `#[serde(default, skip_serializing_if = "Option::is_none")] depth_plate: Option<SceneDepthPlateDef>` eklenmiştir (`validate_dag_nodes` satır 2665–2687'de `0 < near_z < far_z` ve DAG bağımlılığı kontrol edilir).
- **WGSL & Pipeline ([scene3d_depth_seed.wgsl](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/shaders/scene3d_depth_seed.wgsl#L36-L51), [pipelines.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/src/pipelines.rs#L1422-L1493)):** `build_depth_seed_program` hem `1x` hem `4x MSAA` için `Depth32Float` üzerine `CompareFunction::Always` + `ColorWrites::empty()` ile tam ekran üçgen (`draw(0..3, 0..1)`) yazar. Şu dönüşümü kullanır:
  ```wgsl
  let eye_z = mix(plate_far, plate_near, d);
  let ndc_z = (cam_far * (eye_z - cam_near)) / (eye_z * (cam_far - cam_near));
  ```
- **Kompozitör ([compositor.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/src/compositor.rs#L4104-L4368)):** `evaluate_scene3d` içinde tüm 3D çizimlerden (Phase A opak `Text3D`/`Shape3D`/`Transform3D`, Echo ve Phase B `RibbonTrail3D`/`ParticleField3D`) **önce** `depth_seed_bg` çalışır; böylece ön-plandan arkada kalan 3D nesneler donanım Z-testi ile örtülür (`optical_flow_tests.rs` içindeki `scene3d_depth_plate_occludes_3d_objects_behind_foreground` bu temel Z-kesişimini test eder).

### 4. `Scene3D` Prototipinde Tespit Edilen Kritik Buglar ve Eksikler
Kod okumasında 5 somut teknik boşluk/bug doğrulandı:

1. **`fx_dof_coc` + Şeffaf `background_rgba` Taşma (Halo) Bug'ı:** [fx_dof_coc.wgsl](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/shaders/fx_dof_coc.wgsl#L65-L73) ve [fx_dof_coc_msaa.wgsl](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/shaders/fx_dof_coc_msaa.wgsl#L62-L69) yalnızca `raw_d >= 0.99999` durumunu `void (0 CoC)` sayar. Ancak `depth_plate` etkin olduğunda `scene3d_depth_seed.wgsl` her piksele `eye_z <= plate_far` (`raw_d < 0.99999`) yazar. `color_tex` üzerinde 3D nesne bulunmayan şeffaf (`alpha == 0`) ön-plan pikselleri dahi negatif ön-plan CoC (`signed_coc < 0`) üretir; `fs_coc_h`/`fs_coc_v` bu sahte CoC değerini odak düzlemindeki keskin `Text3D` kenarlarına taşır (`dilated_near > 0.35`) ve `fx_dof_bokeh.wgsl` satır 87 early-out'unu bozarak `Text3D` kenarlarını gereksiz yere bulanıklaştırır/karartır.
2. **`SourceImage` Varsayılan `DisplaySrgb` Gamma Bozulması:** [graph.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-schema/src/graph.rs#L453) `SourceImage.color_space` için `DisplaySrgb` varsayılanını kullanır. Kullanıcı/ajan derinlik PNG'sini `SourceImage` ile bağlayıp `linear_rec709` belirtmezse, `color_basic.wgsl` `srgb_to_linear` (`~pow(2.4)`) uygular ve `0.5` orta derinliği `0.214`'e ezer.
3. **Disparity vs Linear Eye-Z Eğrisi ve `depth_gamma` Eksikliği:** `scene3d_depth_seed.wgsl` şu an `eye_z = mix(plate_far, plate_near, d)` yapıyor. Oysa Depth-Anything-V2 çıkışı doğrusal `z` değil **disparity ($1/z$)** olduğundan, uzak nesneler aşırı sıkışır ve odak/yerleşim ayarı zorlaşır. `depth_gamma` ve disparity-uzayı eşleme (`1 / mix(1/plate_far, 1/plate_near, d)`) yoktur.
4. **Döngü Raporlama, `Precomp` ve `ui-snapshot` Eksikleri:**
   - [graph.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-schema/src/graph.rs#L2833) `extract_cycle_nodes`, `node.inputs` ve `node_parent` gezmesine rağmen `depth_plate.node_id`'yi gezmez (`validate_dag_nodes` döngü yakalasa bile boş döngü listesi üretebilir).
   - [precomp.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-schema/src/precomp.rs) `depth_plate.node_id` referanslarını yeniden bağlamaz/dışarı çekmez.
   - [snapshot.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-app/src/snapshot.rs#L608) yalnızca `node.inputs` kenarlarını çizer (`depth_plate` bağlantısı grafikte görünmez).
5. **Animasyon Bağlanamazlığı (`SafeExpr`):** [project.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-schema/src/project.rs#L618-L638) ve [compositor.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/src/compositor.rs#L963-L983) içinde `Scene3D` parametre listesinde `depth_plate_near_z`, `depth_plate_far_z`, `depth_plate_gamma` bulunmamaktadır.

---

## Çıkarımlar ve Öneriler

*(Bu bölümdeki öneriler yukarıdaki kanıtlı gözlemlere dayanan Faz 2 mühendislik tasarımıdır.)*

### A. `scripts/ai_infer_sidecar.py` + `ai_inference.rs` İyileştirmeleri (Depth-Anything-V2-Small)

1. **En-Boy Oranı Korumalı 14-Katı Ön-İşleme (`_resolve_hw_aspect`):**
   - `input_size` varsayılanı `depth` görevi için `518` olmalıdır (kısa kenar `518` veya uzun kenar `518` + `14`'ün katına yuvarlama).
   - Örnek `1920×1080` için kısa kenar `518`: `h_in = 518`, `w_in = round((1920/1080 * 518) / 14) * 14 = 924` (veya hızlı önizleme için uzun kenar `518` → `518×294`).
   - ImageNet normalizasyonu (`mean=[0.485, 0.456, 0.406]`, `std=[0.229, 0.224, 0.225]`) ve `BILINEAR`/`BICUBIC` yeniden boyutlandırma korunur; çıkarım sonrası **float32 uzayında** `(out_w, out_h)` boyutuna `Image.fromarray(norm, mode="F").resize((out_w, out_h), Image.Resampling.BILINEAR)` ile büyütülüp yalnızca en sonda kuantalanmalıdır.
2. **Temporal Ölçek Sabitleme (Anti-Pumping Normalizasyon):**
   - Her karede bağımsız `percentile(1, 99)` almak yerine iki mod desteklenmelidir:
     - `norm_mode = "ema"` (varsayılan): `lo_t = ema(p1)`, `hi_t = ema(p99)` (aynı `smooth` katsayısı veya `0.85` atalet ile; kareler arası L1/sahne kesimi eşiği aşılırsa EMA sıfırlanır).
     - `norm_mode = "global_pass"`: İki geçişli video çıkarımında tüm klip boyunca tek global `[p0.5, p99.5]` aralığı kullanılır (sıfır depth pompalanması).
3. **16-Bit Hassasiyetli Depth Kanalı Protokolü (`RG16` Paketleme + 16-bit PNG / 10-bit HEVC):**
   - Mevcut raw-pipe protokolünü (`width * height * 4` bayt) kırmadan 16-bit depth (`0..65535`) taşımak için `scripts/ai_infer_sidecar.py`'ye `--depth-format {u8,rg16}` bayrağı eklenebilir:
     - `rg16`: `d16 = np.clip(np.rint(norm * 65535.0), 0, 65535).astype(np.uint16)` → `R = (d16 >> 8) as u8`, `G = (d16 & 0xFF) as u8`, `B = R`, `A = 255`.
   - `frameshift-media/src/ai_inference.rs` içinde `DepthOutputFormat { U8Mp4, Hevc10BitMp4, Png16Sequence, Rg16Raw }` tanımlanıp `frame.rs` içindeki `decode_png_reader` fonksiyonunun `BitDepth::Sixteen` durumunda `Rgba16fFrame` / `SourcePixelData::RgbaF32` döndüren bir `load_png_f32` yolu eklenmelidir (sıfır ek crate — `png = "0.18"` halihazırda bağımlılıktır).

### B. `Scene3D` `Depth32Float` Enjeksiyonu & `text_behind` + DoF Reçetesi

1. **WGSL & Uniform Güncellemesi (`scene3d_depth_seed.wgsl`, `uniforms.rs`):**
   - Mevcut 32 baytlık `Scene3DDepthSeedUniform` korunup `flags.yzw` ayrılmış alanları aktif edilir:
     - `flags.x`: `invert` (`0.0` | `1.0`)
     - `flags.y`: `depth_gamma` (varsayılan `1.0`, aralık `0.1..=5.0`)
     - `flags.z`: `remap_mode` (`0.0 = DisparityInverseZ`, `1.0 = LinearEyeZ`)
     - `flags.w`: `decode_mode` (`0.0 = Luma/R`, `1.0 = RG16 Packed`)
   - Disparity modunda önerilen fiziksel formül:
     ```wgsl
     let raw_d = decode_depth(texel, u_seed.flags.w); // R veya (R*255 + G/255.0)/256.0
     let d0 = select(raw_d, 1.0 - raw_d, u_seed.flags.x >= 0.5);
     let d = pow(clamp(d0, 0.0, 1.0), max(u_seed.flags.y, 0.05));
     // Disparity (d=1 near, d=0 far) -> inverse eye_z doğrusal enterpolasyonu:
     let inv_z = mix(1.0 / plate_far, 1.0 / plate_near, d);
     let eye_z_disp = 1.0 / max(inv_z, 1e-5);
     let eye_z_lin = mix(plate_far, plate_near, d);
     let eye_z = select(eye_z_disp, eye_z_lin, u_seed.flags.z >= 0.5);
     ```
2. **`fx_dof_coc.wgsl` / `fx_dof_coc_msaa.wgsl` Şeffaf-Piksel Void Düzeltmesi:**
   - `eval_signed_coc` ve `eval_signed_coc_msaa` içinde `if (raw_d >= 0.99999 || alpha <= 1e-4)` kontrolü eklenerek `depth_plate` tarafından doldurulmuş ama üzerinde hiçbir 3D geometri çizilmemiş şeffaf piksellerin sahte ön-plan CoC yayması önlenmelidir.
   - *(Opsiyonel alternatif)*: `Scene3D` içine `color_plate: Option<NodeId>` veya arka plan `Transform3D` kartı verilerek tüm 2D sahne de doğrudan 3D Bokeh DoF alanına dahil edilebilir (böylece 2D anime sahnesi + 3D yazı birlikte gerçekçi DoF alır).
3. **`text_behind` Gerçek Özne-Arkası Örtme Hattı:**
   - **Aşama 1 (`Scene3D`):** `depth_plate` ön-planda `eye_z = 1.5`, arka planda `eye_z = 5.0` olacak şekilde `Depth32Float`'a yazılır; `Text3D` / `Shape3D` / `ParticleField3D` `z = 2.5..3.5` (veya kamera `z=3.0` iken `pos_z = -0.5`) konumuna konur. Ön-plan pikselleri Z-testiyle atılır (şeffaf kalır), arka plan pikselleri çizilir.
   - **Aşama 2 (`Blend`):** Orijinal 2D video (`base`) altına konup `Scene3D` çıktısı `BlendMode::Normal` ile üstüne bindirilir (veya ince kenar yumuşatması için `DepthMatte` ile yalnızca ön-plan silüeti bir kez daha üst katman olarak bindirilebilir).
4. **Şema / Graf Sağlamlaştırması (`graph.rs`, `project.rs`, `compositor.rs`, `precomp.rs`, `snapshot.rs`):**
   - `SceneDepthPlateDef` içine `#[serde(default = "default_one")] pub depth_gamma: f32`, `#[serde(default)] pub disparity_space: bool` (ve `rg16_packed: bool`) eklenmesi (tümü `#[serde(default)]` olduğu için mevcut JSON projelerle %100 geriye dönük uyumlu).
   - `project.rs` `bindable_param_names` + `compositor.rs` `apply_animated_parameter` içine `"depth_plate_near_z"`, `"depth_plate_far_z"`, `"depth_plate_gamma"` eklenmesi.
   - `extract_cycle_nodes` (`graph.rs`), `precomp.rs` ve `snapshot.rs` düğüm kenar taramalarının `Scene3D.depth_plate.node_id` referansını da kapsaması.
   - `Scene3D` değerlendirilirken `depth_plate` kaynağı `SourceVideo`/`SourceImage` ise `DisplaySrgb` gamma dönüşümünün yanlışlıkla uygulanmadığını doğrulayan bir şema/uyarı veya `depth_plate` okumasında `ColorSpace` dokunulmazlığı dokümantasyonu.

---

## Doğrulanmamış Noktalar

- Gerçek anime kareleri üzerinde `Depth-Anything-V2-Small` çıkışının `Text3D` + `Blend` ile `text_behind` görsel kenar kalitesi (özellikle ince saç telleri ve cel-shaded düz bölgelerde temporal titreşim düzeyi) henüz canlı anime klibiyle render edilip piksel bazlı ölçülmemiştir.
- `1080p60` tam uzunlukta bir anime klibinde `DmlExecutionProvider` (`518×518` vs `924×518`) ile ardışık batch (`batch_size > 1`) veya FP16 (`onnxruntime` graph optimization) hızlanma oranı henüz benchmark edilmemiştir.
- `Scene3D` + `depth_plate` + `fx_dof_coc` açıkken şeffaf piksel CoC taşmasının gerçek görsel artefakt üretip üretmediği birim testle henüz kanıtlanmamıştır (kod okumasından çıkarılmıştır).

---

## Kullanıcı Kararı Gerektiren Açık Noktalar

1. **`Depth-Anything-V2-Small` ONNX Model Kaynağı ve Doğrulama Politikası:**
   - **Durum:** `local_media/models/depth_anything_v2_small.onnx` (`99.06 MB`) şu an diskte mevcuttur.
   - **Seçenek A (Önerilen):** Mevcut yerel dosyayı temel alıp `SHA-256` özetini dokümante etmek; model eksik olduğunda indirme/export kaynağı olarak `onnx-community/depth-anything-v2-small` (`Apache-2.0`) veya `fabio-sim/Depth-Anything-ONNX` (`Apache-2.0`) kullanmak ve CLI kurulumuna açık kullanıcı onayı istemek.
   - **Seçenek B:** Resmi `DepthAnything/Depth-Anything-V2` (`ViT-S`, `Apache-2.0`) PyTorch ağırlığından dinamik eksenli kendi deterministik ONNX export betiğimizi (`scripts/export_depth_onnx.py`) üretmek.

2. **Depth Kanalı Hassasiyeti (8-bit vs 16-bit):**
   - **Seçenek A:** Mevcut 8-bit grayscale MP4 (`yuv420p`) akışını korumak *(hızlı/uyumlu ama `Scene3D` Z-testi ve DoF'ta merdiven bantlaşması riski yüksek)*.
   - **Seçenek B (Önerilen):** Raw pipe üzerinde `RG16` paketlemeyi (`R = hi, G = lo`, sıfır şema kırılması) ekleyip diskte **10-bit HEVC (`anime_10bit_hevc`, `yuv420p10le`, `Crf(12)`)** veya kısa klipler için **16-bit PNG sekansı (`load_png_f32` → `RgbaF32` → `Rgba16Float`)** kullanmak.
   - **Seçenek C:** Yalnızca 16-bit PNG sekansı (`frames/%06d.png`) ve ham `.f32` bellek önbelleği kullanmak *(en yüksek hassasiyet, ancak yüksek disk/IO maliyeti)*.

3. **`Scene3D` Depth Enjeksiyon Şema Tasarımı:**
   - **Seçenek A (Önerilen):** Mevcut `NodeKind::Scene3D { depth_plate: Option<SceneDepthPlateDef> }` gömülü sözleşmesini koruyup genişletmek (`depth_gamma`, `disparity_space`, `rg16_packed`, animasyon bağları + `precomp`/`extract_cycle_nodes`/`fx_dof_coc` void düzeltmesi). Geriye dönük uyumluluk %100 korunur ve gereksiz DAG ara düğümü çıkmaz.
   - **Seçenek B:** `Scene3D.depth_plate` alanını kaldırıp `NodeKind::DepthSeed` adlı ayrı bir düğüm türü eklemek *(mevcut `optical_flow_tests.rs` ve `graph.rs` şemasını kırar, ek pas/bellek maliyeti doğurur)*.
   - **Seçenek C:** `Scene3D.depth_plate` yanında, `color_plate: Option<NodeId>` opsiyonu ekleyerek 2D anime sahnesinin kendisinin de `Scene3D` Z-buffer Bokeh DoF efektinden doğrudan etkilenmesini sağlamak.
