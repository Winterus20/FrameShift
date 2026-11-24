---
title: Pillar 5 — 2D Point/Planar Tracker, .otio/FCPXML NLE Köprüsü & Manifest Tabanlı AI Eklenti Yuvası
date: 2026-11-24
status: implemented
tags: [engine, shaders, render-graph, timebase, schema, media-io, agent, licensing]
---

# Pillar 5 — 2D Point/Planar Tracker, `.otio`/FCPXML Köprüsü & Manifest Tabanlı Genel AI Eklenti Yuvası

> **Uygulama Durumu (2026-11-24):** **0 yeni Cargo crate** ile implemente edildi ve doğrulandı ([uygulama günlüğü](../log/2026-11-24-competitive-power-roadmap-implementation.md)): 4-seviyeli optik akıştan Forward-Backward + `32x32` ROI readback 2D Point/Planar Tracker (`Project.keyframe_tracks`), 0-crate `.otio` + FCPXML 1.10/1.11 `export-nle`/`import-nle`, `model_manifest.json` (`frameshift.ai_model.v1`, SHA-256 `Strict` + `--allow-unverified`, `scripts/ai_infer_sidecar.py --manifest`) ve CLI/MCP yüzeyleri.

## Soru / Amaç

FrameShift'in mevcut GPU optik akış, deterministik `Flicks` zaman modeli ve raw-pipe ONNX/Python sidecar altyapısı üzerine **0 yeni Cargo crate** hedefiyle üç yüksek getirili yeteneğin mimarisini belirlemek:
1. Mevcut 4 seviyeli GPU Pyramidal Lucas-Kanade optik akışından (`fx_optical_flow_step.wgsl`) **2D Point / 4-Köşe Planar Tracker** (`track-point` / `track-planar`) çıkarıp `Transform2D`, `Null3D` ve `CornerPin` düğümlerine doğrudan keyframe bağlamak.
2. FrameShift'in beat/downbeat kesimlerini, ses stem'lerini ve söz/olay zamanlamalarını **OpenTimelineIO (`.otio` JSON)** ve **FCPXML 1.10/1.11** formatlarına 0-crate ile dışa/içe aktarmak (MakeMyClip dersi).
3. Rust çekirdeğini yeniden derlemeden `local_media/models/<model>/model_manifest.json` üzerinden yeni ONNX/NCNN modellerini (`SAM 2`, `BiRefNet`, yeni upscaler'lar vb.) otomatik keşfedip çalıştıran **manifest tabanlı genel AI eklenti yuvası** kurmak (Gausian / ComfyUI dersi).

---

## İncelenen Birincil Kaynaklar

### Yerel Kod Tabanı (Doğrudan Okunan)
- [crates/frameshift-render/shaders/fx_optical_flow_luma.wgsl](../../crates/frameshift-render/shaders/fx_optical_flow_luma.wgsl) — `sqrt(Y)` algısal luma çıkarımı.
- [crates/frameshift-render/shaders/fx_optical_flow_step.wgsl](../../crates/frameshift-render/shaders/fx_optical_flow_step.wgsl) — 4 seviyeli warped Lucas-Kanade + Horn-Schunck regülarizasyonu (`vec4(flow_uv.x, flow_uv.y, confidence, mean_diff)`).
- [crates/frameshift-render/shaders/fx_optical_flow_blur.wgsl](../../crates/frameshift-render/shaders/fx_optical_flow_blur.wgsl) — akış tabanlı blur ve `visualize_flow` HSV debug görünümü.
- [crates/frameshift-render/src/compositor.rs](../../crates/frameshift-render/src/compositor.rs) — `evaluate_optical_flow_blur` (L2240–2391), `build_luma_pyramid` (L2393–2424), `readback_rgba8` (L6640–6710), `ProjectAnimator::evaluate_node` (L364–403), `build_scene_camera_matrices` (L6741–6794), `resolve_world_matrix` (L6850–6940).
- [crates/frameshift-render/src/uniforms.rs](../../crates/frameshift-render/src/uniforms.rs) — `TransformUniform` (L7), `OpticalFlowStepUniform` (L439), `OpticalFlowBlurUniform` (L476).
- [crates/frameshift-render/src/warp2d.rs](../../crates/frameshift-render/src/warp2d.rs) — `CornerPinUniform`, `compute_corner_pin_inv_homography` (`glam::Mat3` + Paul Heckbert homografisi).
- [crates/frameshift-render/src/pool.rs](../../crates/frameshift-render/src/pool.rs) — `WORKING_TEXTURE_FORMAT` (`Rgba16Float`), `TexturePool`, `PooledTexture`.
- [crates/frameshift-render/shaders/transform.wgsl](../../crates/frameshift-render/shaders/transform.wgsl) — `Transform2D` aspect-isotropic UV dönüşüm konvansiyonu.
- [crates/frameshift-schema/src/graph.rs](../../crates/frameshift-schema/src/graph.rs) — `NodeKind::{Transform2D, CornerPin, Null3D, OpticalFlowBlur}`, `Camera3DDef` (`default_camera_pos = [0.0, 0.0, 2.5]`, `default_fov_deg = 50.0`).
- [crates/frameshift-schema/src/project.rs](../../crates/frameshift-schema/src/project.rs) — `Project`, `ClipDef`, `AudioClipDef`, `AudioBus`, `EventAnchorDef`, `EventBoundAnimation`, `ParamBindingDef`, `bindable_param_names`.
- [crates/frameshift-schema/src/version.rs](../../crates/frameshift-schema/src/version.rs) — `CURRENT_SCHEMA_VERSION` (`0.2.0`) ve ADR-0004 minor uyumluluk kuralı.
- [crates/frameshift-time/src/expr.rs](../../crates/frameshift-time/src/expr.rs) — `SafeExpr`, `DEFAULT_MAX_FUEL = 256`, `MAX_EXPR_DEPTH = 64`, `MAX_EXPR_NODES = 1024`.
- [crates/frameshift-media/src/ai_inference.rs](../../crates/frameshift-media/src/ai_inference.rs) & [scripts/ai_infer_sidecar.py](../../scripts/ai_infer_sidecar.py) — raw-pipe ONNX çıkarımı.
- [crates/frameshift-media/src/ai_audio.rs](../../crates/frameshift-media/src/ai_audio.rs) & [scripts/ai_audio_sidecar.py](../../scripts/ai_audio_sidecar.py) — ses stem/lyric sidecar ve `local_media/models/` önbellek düzeni.
- [crates/frameshift-media/src/frame.rs](../../crates/frameshift-media/src/frame.rs) — saf Rust `Sha256` (L858–960).
- [crates/frameshift-media/src/spectral.rs](../../crates/frameshift-media/src/spectral.rs) — `MultibandBeatReport` (L177).
- [crates/frameshift-cli/src/workbench.rs](../../crates/frameshift-cli/src/workbench.rs) & [crates/frameshift-mcp/src/server.rs](../../crates/frameshift-mcp/src/server.rs) — CLI ve MCP araç kayıt yapısı.

### Harici Birincil Şema Kaynakları (İnceleme Tarihi: 2026-11-24)
- [OpenTimelineIO — OTIO File Format & Schema (`Timeline.1`, `Stack.1`, `Track.1`, `Clip.1`, `Gap.1`, `RationalTime.1`, `TimeRange.1`, `Marker.1`, `ExternalReference.1`)](https://opentimelineio.readthedocs.io/en/latest/tutorials/otio-file-format-specification.html) — web araması üzerinden doğrulanmıştır.
- [Apple Developer Documentation — Final Cut Pro XML (FCPXML 1.10/1.11 DTD: `<fcpxml>`, `<resources>`, `<format>`, `<asset>`, `<sequence>`, `<spine>`, `<asset-clip>`, `<marker>`)](https://developer.apple.com/documentation/finalcutpro/document_type_definition) — web araması üzerinden doğrulanmıştır.

---

## Doğrudan Gözlemler (Kanıtlı) & 0-Crate Fizibilitesi

Workspace içindeki tüm `crates/*/Cargo.toml` ve `Cargo.toml` dosyaları incelendiğinde üç başlık için de **0 yeni Cargo crate** yeterli olduğu doğrulanmıştır:

| İhtiyaç | Mevcut Workspace Karşılığı | Durum |
| :--- | :--- | :--- |
| 2D Matris / Homografi / SVD veya 3×3 lineer cebir | `glam = "0.29"` + `frameshift-render/src/warp2d.rs` (`CornerPinUniform` + `compute_corner_pin_inv_homography`) | Mevcut |
| GPU `Rgba16Float` f16→f32 readback dönüşümü | `half = "2.7.1"` (`crates/frameshift-render/Cargo.toml`) | Mevcut |
| `.otio` JSON okuma/yazma + `model_manifest.json` | `serde = "1.0"`, `serde_json = "1.0"` (tüm crate'lerde) | Mevcut |
| FCPXML 1.10/1.11 XML yazma ve temel okuma | Rust `std::fmt::Write` + minimal saf Rust recursive-descent XML parser (~300 satır, harici DTD/ENTITY reddi) | 0 yeni crate |
| Model dosyası SHA-256 doğrulaması | `frameshift-media/src/frame.rs` içindeki saf Rust `Sha256` (`FfmpegBridge::hash_file`) | Mevcut |
| GPU Optik Akış Piramidi | `fx_optical_flow_luma.wgsl`, `fx_optical_flow_step.wgsl`, `build_luma_pyramid` | Mevcut |

### Mevcut Koddaki Kritik Bulgular ve Kısıtlar
1. **Optik Akış Yönü (`fx_optical_flow_step.wgsl:92-104`):** `p_uv = clamp(c_uv - u0, ...)` ve `it = curr_c - prev_c` formülüyle akış vektörü `flow_uv = (u, v)`, `prev` kareden `curr` kareye olan **ileri yer değiştirmeyi** (`p_curr - p_prev`) normalize UV uzayında verir (`z = confidence ∈ [0, 1]`, `w = mean_diff`). Ancak `OpticalFlowStepUniform.limits_and_flags.z` (`invert_sign`) şu an shader içinde kullanılmıyor; `evaluate_optical_flow_blur` şimdilik sadece tek yön (geriye bakarak `prev -> curr`) akışı hesaplıyor.
2. **Keyframe & Expression Sınırı (`expr.rs:5-11`, `project.rs:73-84, 320`):**
   - `ParamBindingDef` yalnızca `SafeExpr` string'i taşır ve `MAX_EXPR_NODES = 1024`, `MAX_EXPR_DEPTH = 64`, `DEFAULT_MAX_FUEL = 256` sınırlarına tabidir. 24–60 fps'lik 150–600 karelik bir tracker eğrisini `lerp`/`step` ifadesine gömmek AST sınırını aşar.
   - `EventBoundAnimation` ise yalnızca `ClipDef.event_animations` altında yaşar ve bir `event_id`'ye zorunlu olarak bağlıdır (`project.rs:836-843`).
3. **AI Sidecar Kısıtları (`ai_infer_sidecar.py:57-131`, `ai_inference.rs:65-128, 362-363`):**
   - `AiTaskKind` şu an kapalı bir 2 elemanlı enum (`RotoscopeMatte`, `DepthMap`).
   - `_run_onnx_raw` giriş normalizasyonunu `task == "depth"` kontrolüne göre ImageNet veya `[-1, 1]` diye dallandırır, tek çıkış tensoru (`outputs[0]`) okur ve `run_ai_frame_inference` dönüş baytını sabit `w * h * 4` bekler (bu nedenle `2x`/`4x` ONNX upscaler modelleri çalışamaz).
   - `local_media/models/` altında şu an çıplak `isnetis.onnx` (176 MB) ve `depth_anything_v2_small.onnx` (99 MB) bulunmaktadır.

---

## Çıkarımlar ve Öneriler (Mimari Tasarım)

*(Not: Aşağıdaki tüm mimari tasarımlar, şema genişletmeleri ve CLI/MCP sözleşmeleri doğrudan okunan mevcut kod üzerine kurulu önerilerdir; henüz kodlanmamış ve çalıştırılmamıştır.)*

---

### Bölüm 1 — GPU Pyramidal Optik Akıştan 2D Point / Planar Tracker

#### 1.1 Çift Yönlü Optik Akış & Readback Maliyet Analizi
Mevcut `GpuCompositor::evaluate_optical_flow_blur` (`compositor.rs:2240`) tüm kare çözünürlüğünde `Rgba16Float` (piksel başına **8 bayt**) doku üretir.

- **Tam Kare Readback Maliyeti (Kaçınılması Gereken Yol):**
  - `1920×1080` @ `Rgba16Float` = `1920 * 1080 * 8 = 16,588,800 B (~15.82 MiB/kare)`.
  - 300 karelik (5 sn @ 60fps) bir takip işleminde CPU'ya ~4.74 GiB veri kopyalanır ve her karede GPU-CPU senkron stall (`device.poll`) yaşanır.
- **Önerilen ROI-Kırpmalı Readback (`copy_texture_to_buffer` Origin+Extent):**
  - `wgpu::CommandEncoder::copy_texture_to_buffer`, doku üzerinde keyfi bir `origin: Origin3d { x, y, z: 0 }` ve `extent: Extent3d { width: roi_w, height: roi_h, depth_or_array_layers: 1 }` penceresini okuyabilir (`COPY_BYTES_PER_ROW_ALIGNMENT = 256` kuralına uyarak).
  - **2D Point Tracker:** Nokta çevresindeki `32×32` piksel yaması (`32 * 8 = 256 B` satır = sıfır padding israfı!) toplamda yalnızca **`8 KiB/kare`** (forward + backward akış için **`16 KiB/kare`**) readback yapar — tam kareye göre **~1,000× daha az veri**.
  - **4-Köşe Planar Tracker:** Takip edilen dörtgenin kapsayıcı kutusu (bounding box) dinamik olarak `256×256` ızgaraya veya ROI kırpımına (ör. `256×256` = `512 KiB/kare`, veya 4 köşe + iç ızgaradan `64` adet `8×8` blok yerine tek sıkı bounding box) kopyalanır; hatta akış seviyesi `level 1` (`1/2x` çözünürlük) veya `level 0` ROI seçilerek `1080p`'de bile `< 512 KiB/kare` altında kalınır.
  - **Piramit Yeniden Kullanımı (0 Yeni Shader Başlangıcı):** `build_luma_pyramid` (`compositor.rs:2393`) `prev_pyr` ve `curr_pyr` piramitlerini bir kez üretir:
    - **Forward akış (`F`):** `prev_pyr -> curr_pyr` (`fx_optical_flow_step` 4 seviye × 2 iterasyon)
    - **Backward akış (`B`):** `curr_pyr -> prev_pyr` (aynı piramitlerin ters sırada bağlanması, 0 ek luma pass!).

#### 1.2 Tracker Algoritması (Forward-Backward Consistency + Drift Koruması)
Her ardışık `(t, t+1)` kare çifti için (veya kullanıcı geri yön isterse `(t, t-1)`):

1. **Kaynak Değerlendirme:** Takip edilecek düğüm (`--node <node_id>`, örn. `src_a` veya herhangi bir ara kompozit düğüm) `GpuCompositor::evaluate_subgraph` ile tek seferlik evaluate edilir (`t` karesinin çıktısı bir sonraki adım için `prev_tex` / `prev_pyr` olarak elde tutulur; böylece her kare yalnızca **1 kez** decode/evaluate edilir, `2×` değil!).
2. **2D Point Tracker (`track-point`):**
   - Başlangıçta `p_0 = (u_0, v_0)` (normalize `[0, 1]` UV) ve `patch_radius_px` (varsayılan `12 px`, ROI `32×32`) verilir.
   - `F` (forward) ve `B` (backward) yamaları CPU'da `half::f16::to_f32()` ile çözülür.
   - Yama içindeki her `(dx, dy)` pikseli için:
     - Bilinear örneklenen ileri akış `v_f = F(p_t + delta)` (px cinsinden)
     - Geri projeksiyon `p_back = p_t + delta + v_f`, geri akış `v_b = B(p_back)`
     - **Forward-Backward Tutarsızlık Hatası:** `e_fb = ||v_f + v_b||_2` (px).
     - **Ağırlık:** `w = gaussian(delta) * conf_f * conf_b * fb_gate(e_fb, max_fb_error_px)` (varsayılan `max_fb_error_px = 1.5 px`, `min_confidence = 0.25`).
   - Eğer toplam geçerli ağırlık eşik altındaysa (ani sahne kesimi / oklüzyon: `mean_diff > sensitivity` veya `e_fb > max_fb_error_px`): kare `lost/occluded` işaretlenir ve hız ekstrapolasyonu/son konum dondurma uygulanır (drift koruması).
   - **2-Nokta Modu (`--point2 <u2,v2>`):** İki nokta `pA_t`, `pB_t` paralel takip edilerek:
     - `translate_t = (midpoint_t - midpoint_0)`
     - `scale_t = ||pB_t - pA_t|| / max(||pB_0 - pA_0||, 1e-6)`
     - `rotation_rad_t = wrap_angle(atan2(pB_t.y - pA_t.y, pB_t.x - pA_t.x) - theta_0)` çıkarılır.
3. **4-Köşe Planar Tracker (`track-planar`):**
   - Giriş: başlangıç karesinde `tl, tr, br, bl` (veya `--roi x0,y0,x1,y1`).
   - Dörtgen içindeki düzenli `8×8 = 64` (veya `10×10 = 100`) alt-örnek noktası `q_i` alınır.
   - Her `q_i` için yerel `5×5` pencereden FB-tutarlılığı geçmiş akış `v_i` ve inlier skoru `w_i` hesaplanır (`e_fb <= max_fb_error_px`, `conf >= 0.30`).
   - **Homografi / Affine Çözümü (0 Yeni Crate):**
     - `inlier >= 6` ise ağırlıklı normalize DLT (Direct Linear Transform) + 3 iterasyonlu Huber IRLS (`8×8` veya `9×9` Gauss-Jordan/Householder çözümü saf Rust ~70 satır) ile kareler arası `ΔH_t` (`glam::Mat3`) hesaplanır.
     - `inlier` 3–5 arasındaysa 6-DoF Affine (`ΔA_t`) fallback yapılır; `< 3` ise kare `lost` işaretlenir.
     - Kümülatif homografi `H_t = ΔH_t * H_{t-1}` güncellenir; köşeler `c_t = project(H_t, c_0)` ile hesaplanır.
   - **Key-Frame Template Drift Koruması:** Her `reanchor_interval` (ör. 15 karede bir, yalnız yüksek güvenilirlikte `mean_fb < 0.4 px` iken) başlangıç veya son sağlam referans ROI yaması ile mevcut warped yama arasında 3×3 piksel altı luma NCC (Normalized Cross-Correlation) taramasıyla ±1.0 px birikimli kayma sıfırlanır.
4. **Keyframe Seyreltme (Opsiyonel `--simplify-tol <px>`):** Douglas-Peucker ile düz çizgi bölümlerinde gereksiz keyframe'ler temizlenebilir (`0.0` = kare-kare yoğun keyframe).

#### 1.3 `Transform2D`, `Null3D` ve `CornerPin` Koordinat Eşlemesi

Mevcut shader ve schema konvansiyonlarına göre birebir dönüşümler:

| Hedef Düğüm | Parametreler (`project.rs:333, 399, 504`) | Tracker Çıktısından Matematiksel Eşleme |
| :--- | :--- | :--- |
| **`Transform2D`** | `translate_x`, `translate_y`, `scale_x`, `scale_y`, `rotation_rad`, `anchor_x`, `anchor_y` | `transform.wgsl:111` gereği `uv=(0,0)` sol-üst, `uv=(1,1)` sağ-alt ve pivot `anchor + translate` noktasındadır. `anchor = p_0`, `translate = p_t - p_0` (normalize `[0..1]`), `scale_x = scale_y = scale_t`, `rotation_rad = rotation_rad_t`. *(Not: Kamera sabitleme `stabilize` modu için işaret terslenir: `translate = p_0 - p_t`, `scale = 1/scale_t`, `rotation_rad = -rotation_rad_t`.)* |
| **`Null3D`** | `pos_x`, `pos_y`, `pos_z`, `rot_z`, `scale_x`, `scale_y`, `scale_z` | `compositor.rs:6741` ve `graph.rs:1375-1380` gereği `Camera3DDef` sağ-elli perspektif (`pos=[0, 0, 2.5]`, `fov_deg=50.0`) kullanır. Hedef `plane_z` (varsayılan `0.0`) düzleminde `d = cam_pos.z - plane_z`, `half_h = d * tan(fov_y/2)`, `half_w = half_h * aspect`:<br>`pos_x = (2.0 * p_t.x - 1.0) * half_w`<br>`pos_y = (1.0 - 2.0 * p_t.y) * half_h` (`Y` yukarı!)<br>`rot_z = -rotation_rad_t.to_degrees()`<br>`scale_x = scale_y = scale_z = scale_t` |
| **`CornerPin`** | `tl_x`, `tl_y`, `tr_x`, `tr_y`, `br_x`, `br_y`, `bl_x`, `bl_y` | `warp2d.rs:38` doğrudan `[0, 1]` normalize hedef UV köşeleri bekler; planar tracker'ın `[tl_t, tr_t, br_t, bl_t]` çıktısı dönüşümsüz birebir bağlanır. |

#### 1.4 Şemada Keyframe Depolama Tasarımı (`KeyframeTrackDef`)
`ParamBindingDef` (`MAX_EXPR_NODES = 1024`) ve `EventBoundAnimation` (`ClipDef` + `event_id` zorunluluğu) yoğun tracker eğrileri için uygun olmadığından, `Project` üzerine ADR-0004 (`schema_version = 0.2.0`) uyumlu **`#[serde(default, skip_serializing_if = "Vec::is_empty")]`** yeni alan önerilir:

```rust
pub struct TickKeyframe {
    pub tick: Flicks,
    pub value: f64,
    #[serde(default)]
    pub easing: AnimationEasing, // Linear | EaseIn | EaseOut | EaseInOut | Step
}

pub struct KeyframeTrackDef {
    pub id: String,
    pub target_node_id: NodeId,
    pub parameter: String,           // bindable_param_names ile doğrulanır
    #[serde(default)]
    pub hold_outside: bool,          // true: aralık dışında ilk/son değeri tutar
    pub keyframes: Vec<TickKeyframe>, // artan tick sıralı -> O(log K) partition_point
}
```
- **Öncelik sırası (`ProjectAnimator::evaluate_node`):** `base_node_kind` → `event_animations` → `keyframe_tracks` → `param_bindings` (böylece kullanıcı isterse `param_bindings` ile tracker verisinin üstüne `wiggle(...)` veya offset ekleyebilir!).
- **Eski projeler:** `#[serde(default)]` sayesinde `0.1.0` ve `0.2.0` projeleri %100 geriye uyumlu kalır.

#### 1.5 CLI & MCP Tasarımı (`track-point`, `track-planar`)
- **CLI (`frameshift-cli`):**
  - `track-point <project.json> --node <node_id> --point <u,v> [--point2 <u2,v2>] [--range <start>..<end>] [--patch-radius <px>] [--max-fb-error <px>] [--mode follow|stabilize] --bind-node <target_node_id> [--plane-z <f32>] [--simplify-tol <px>] [--out <project.json>] [--json]`
  - `track-planar <project.json> --node <node_id> (--roi <x0,y0,x1,y1> | --corners <tlx,tly,trx,try,brx,bry,blx,bly>) [--range <start>..<end>] [--grid <N>] [--max-fb-error <px>] --bind-node <corner_pin_or_transform2d_id> [--out <project.json>] [--json]`
- **MCP (`frameshift-mcp`):**
  - `track_point` ve `track_planar` araçları:
    - Giriş: `node_id`, `point` / `point2` veya `corners`, `start_tick` / `end_tick`, `bind_node_id` (opsiyonel), `expected_revision_id` (proje güncellenecekse zorunlu optimistic lock), `mode` (`follow` / `stabilize`).
    - Çıkış: `frames_tracked`, `lost_frame_ticks`, `mean_fb_error_px`, `min_confidence`, `keyframe_tracks` özeti ve uygulandıysa yeni `revision_id`.

---

### Bölüm 2 — NLE Dışa/İçe Aktarım Köprüsü (`.otio` JSON & FCPXML 1.10/1.11)

#### 2.1 Birincil Şema Yapıları ve Zaman Dönüşümü (`Flicks` ↔ `RationalTime` / `"N/Ds"`)

FrameShift `FLICKS_PER_SECOND = 705_600_000` (`i64`) kullanır.

1. **OpenTimelineIO (`.otio` JSON):**
   - Kök nesne `"OTIO_SCHEMA": "Timeline.1"`, `"tracks": { "OTIO_SCHEMA": "Stack.1", "children": [Track.1...] }`.
   - Video ve Ses zaman tabanı (`RationalTime.1 { value: f64, rate: f64 }`):
     - Video `Clip.1` / `Gap.1` sınırları proje `fps = Rational32 { num, den }` oranında kareye çevrilir:
       `rate = fps.num as f64 / fps.den as f64` (`24.0`, `60.0`, `23.976023976...` için `24000/1001`).
       `value_i128 = (tick.0 as i128 * fps.num as i128) / (FLICKS_PER_SECOND as i128 * fps.den as i128)` (en yakın tam kareye yuvarlama + kalan delta bilgisi `metadata.frameshift.exact_ticks` içinde korunur).
     - `24, 25, 30, 48, 50, 60, 120` gibi tam FPS'lerde `705_600_000` tümüne kalansız bölündüğü için (`24fps = 29_400_000`, `60fps = 11_760_000`) sıfır kayma ile birebir tam sayı `value` üretilir!
     - Ses/Söz/Beat Marker `Marker.1` zamanları kare sınırına düşmek zorunda değildir: audio `Track.1` ve track-level beat/lyric marker'ları için `rate = 48000.0` (1 sample = `14_700` Flicks, kalansız!) veya video track üzerinde `rate = fps` seçilebilir. (En geniş NLE uyumluluğu — Resolve/Premiere — için `Timeline` `rate` değeri proje video `fps`'i, ses `Clip` `source_range` değeri `sample_rate_hz` veya `fps` kullanılır.)
2. **FCPXML 1.10 / 1.11 (`<!DOCTYPE fcpxml>`):**
   - Tüm zaman nitelikleri (`frameDuration`, `offset`, `start`, `duration`) rasyonel saniye kesridir: `"N/Ds"` veya tam saniyede `"Ns"`.
   - `flicks_to_fcpxml_time(tick: Flicks) -> String`:
     `g = gcd(|tick.0|, 705_600_000)`; `num = tick.0 / g`, `den = 705_600_000 / g`. `den == 1` ise `"{num}s"`, aksi halde `"{num}/{den}s"`.
     - *Örnek:* 1 kare @ `23.976` (`24000/1001`) = `29_429_400` Flicks → `gcd = 29_400` → `"1001/24000s"` (Apple DTD ile **%100 bit-birebir**!).
     - *Örnek:* 1 kare @ `60fps` = `11_760_000` Flicks → `"1/60s"`.
     - *Örnek:* `48 kHz` ses örneği = `14_700` Flicks → `"1/48000s"`.
   - `fcpxml_time_to_flicks(s: &str) -> Result<Flicks>`: `"N/Ds"` veya `"Ns"` veya ondalık `"1.5s"` ifadelerini `i128` aritmetiğiyle `Flicks`'e çevirir.

#### 2.2 FrameShift ↔ OTIO / FCPXML Tam Eşleme Tablosu

| FrameShift Kavramı | OTIO (`.otio` JSON) Karşılığı | FCPXML 1.10/1.11 Karşılığı | Kayıp / Notlar |
| :--- | :--- | :--- | :--- |
| `Project.name`, `timeline.fps`, `duration_ticks`, `resolution` | `Timeline.1.name`, `global_start_time` (`0 @ fps`), `metadata.frameshift.{width, height, schema_version, revision_id}` | `<fcpxml version="1.11"><resources><format id="r1" frameDuration="..." width="..." height="..."/></resources>...<sequence format="r1" duration="...">` | Tam eşleşir. |
| `AssetDef` (`Video`/`Audio`/`Image`) | `ExternalReference.1` (`target_url`: `file:///...` URI, `available_range`: `TimeRange.1`) | `<asset id="rN" name="..." src="file:///..." start="0s" duration="..." hasVideo="0|1" hasAudio="0|1" format="r1"/>` | `file:///` URI normalizasyonu (Windows `C:\...` → `file:///C:/...`). |
| `timeline.clips` (`ClipDef`) | `Track.1` (`kind: "Video"`): sıraya dizilmiş `Clip.1` + boşluklarda `Gap.1` | `<spine>` içinde sıralı `<asset-clip ref="rN" offset="..." start="..." duration="..." tcFormat="NDF"/>` + `<gap .../>` | Çakışan video klipleri varsa ek üst katman `Track.1` (OTIO) / `lane="1", "2"` (FCPXML) açılır. |
| `ClipDef.speed_curve` | Sabitse `LinearTimeWarp.1 { time_scalar }`; eğriyse `TimeEffect.1` + `metadata.frameshift.speed_curve` | `<timeMap>` (`<timept time="..." value="..." interp="smooth2|linear"/>`) | Karmaşık `CubicBezierSpeed` için NLE yaklaşık `<timept>` + `metadata`'da tam curve korunur. |
| `timeline.audio_clips` (`AudioClipDef`) & `AudioBus` | Her `AudioBus` (`music`, `dialogue`, `sfx_impact`, `sfx_whoosh`, `sfx_riser`, `sub_bass`) ayrı `Track.1` (`kind: "Audio"`, `name: bus_name`) + `Gap.1` | Ana `<spine>`'ın ilk öğesine (veya `<gap>`'a) bağlı negatif lane'ler: `lane="-1"` (`Music`), `lane="-2"` (`Dialogue`), `lane="-3"` (`SfxImpact`) vb. + `role="music|dialogue|effects"` + `<adjust-volume amount="{gain_db}dB"/>` | `anchor_event_id` + `apex_offset_ticks` export öncesi `resolve_audio_clip_bounds` (`resolver.rs:761`) ile mutlak tick'e çözülür! |
| `Project.events` (`EventAnchorDef`) + `MultibandBeatReport` (`beats`, `downbeats`) | `Track.1.markers` üzerinde `Marker.1`: `downbeat`=`"RED"`, `beat`=`"ORANGE"`, `user_event`=`"MAGENTA"` | `<spine>`/kök `<sequence>` veya ilgili `<asset-clip>` içinde `<marker start="..." duration="1/fps s" value="[BEAT] ..."/>` | `MultibandBeatReport` JSON verilirse downbeat & beat marker'ları doğrudan NLE gridine basılır. |
| `LyricAlignResult` (kelime / satır zamanları) | `Track.1` (veya `Dialogue` audio track) üzerinde `Marker.1` (`color: "CYAN"`, `name: word.text`, `marked_range` = `[start, end]`) | `<marker start="..." duration="..." value="[LYRIC] {word}"/>` + `<keyword start="..." duration="..." value="lyric"/>` | NLE'de karaoke / altyazı hizalaması için doğrudan görünür. |
| `ShaderTransition` (opsiyonel 2-klip geçişi) | `Transition.1` (`transition_type: "SMPTE_Dissolve"`, `in_offset`, `out_offset`, `metadata.frameshift.shader_name`) | `<transition name="..." offset="..." duration="..."/>` | NLE'lerde standart dissolve olarak açılır; shader adı metadata'da saklanır. |

#### 2.3 İçe Aktarım (`import-otio` & `import-fcpxml`) Akışı
1. **Ayrıştırma:**
   - `.otio`: `serde_json::from_str` ile `OTIO_SCHEMA == "Timeline.1"` doğrulanır; `tracks` (`Stack.1`) altındaki `Video` ve `Audio` `Track.1` öğeleri gezilir (`Gap.1` zaman imlecini ilerletir, `Clip.1` ise `ClipDef` / `AudioClipDef` üretir).
   - `.fcpxml`: Yeni saf Rust minimal XML okuyucu (`crates/frameshift-schema/src/nle/fcpxml.rs` veya `frameshift-core/src/nle/`): `<resources>` altındaki `<format>` ve `<asset>` (ve `<media-rep>`) sözlüğü toplanır; `<sequence>/<spine>` içindeki `<asset-clip>`, `<gap>`, bağlı `<asset-clip lane="...">` ses klipleri ve `<marker>`'lar okunur. Harici `<!ENTITY>` / `<!DOCTYPE ... SYSTEM ...>` reddedilir.
2. **FrameShift Projesine Dönüştürme:**
   - Her benzersiz medya URL'si `file:///` → yerel `PathBuf` çevrilip `AssetDef` olarak kaydedilir.
   - Ana video track `AuthorHardCuts` mantığıyla sıralı `ClipDef` + `SourceVideo` + `Blend` zincirine derlenir (`Project::validate()` kapısından geçirilir).
   - Ses klipleri `role` / track adına göre `AudioBus` (`Music`, `Dialogue`, `SfxImpact` vb.) atanır.
   - `Marker` öğeleri `EventAnchorDef { origin: EventOrigin::Detected | UserMarked, ... }` olarak `project.events` içine yazılır (böylece NLE'den gelen marker'lar doğrudan `autocut` / `SafeExpr` `event_delta_sec` / `EventBoundAnimation` hedefleri olarak kullanılabilir!).
3. **CLI & MCP:**
   - CLI: `export-nle <project.json> --format otio|fcpxml --out <timeline.otio|timeline.fcpxml> [--beats <beats.json>] [--lyrics <lyrics.json>]`
   - CLI: `import-nle <in.otio|in.fcpxml> --out <project.json> [--fps <num[/den]>] [--width <u32>] [--height <u32>]`
   - MCP: `export_nle`, `import_nle`.

---

### Bölüm 3 — Manifest Tabanlı Genel AI Eklenti Yuvası (`model_manifest.json`)

#### 3.1 Keşif ve Dizin Düzeni
Her harici model `local_media/models/<model_id>/` klasöründe yaşar:
```text
local_media/models/
├── isnetis/
│   ├── model_manifest.json
│   └── isnetis.onnx
├── depth_anything_v2_small/
│   ├── model_manifest.json
│   └── depth_anything_v2_small.onnx
├── birefnet_general/
│   ├── model_manifest.json
│   └── BiRefNet-general-epoch_244.onnx
└── sam2_hiera_tiny/
    ├── model_manifest.json
    ├── sam2_encoder.onnx
    └── sam2_decoder.onnx
```
*(Geriye dönük uyumluluk: Şu an `local_media/models/` kökünde duran `isnetis.onnx` ve `depth_anything_v2_small.onnx` dosyaları ya ilgili alt klasöre taşınabilir ya da `discover_ai_models` kökteki bilinen dosya adları için yerleşik fallback manifest sunar — mevcut testler `live_depth_anything_v2_small_onnx_runs_aspect_preserving_rg16` (`ai_inference.rs:616`) bozulmaz!)*

#### 3.2 `model_manifest.json` Şeması (`frameshift.ai_model.v1`)

```json
{
  "schema_version": "frameshift.ai_model.v1",
  "id": "birefnet_general",
  "display_name": "BiRefNet General Segmentation",
  "version": "1.0.0",
  "task": "matte",
  "runtime": "onnx_pipe",
  "entrypoint": "BiRefNet-general-epoch_244.onnx",
  "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
  "license": {
    "spdx": "MIT",
    "commercial_ok": true,
    "source_url": "https://github.com/ZhengPeng7/BiRefNet"
  },
  "preferred_provider": "auto",
  "input": {
    "tensor_name": null,
    "dtype": "float32",
    "layout": "nchw",
    "color_space": "rgb",
    "normalization": {
      "kind": "imagenet",
      "mean": [0.485, 0.456, 0.406],
      "std": [0.229, 0.224, 0.225]
    },
    "resize_policy": {
      "mode": "fixed",
      "width": 1024,
      "height": 1024,
      "multiple_of": 32,
      "max_side": 2048,
      "interp": "bilinear"
    },
    "tiling": null,
    "aux_inputs": []
  },
  "output": {
    "tensor_name": null,
    "output_index": -1,
    "layout": "nchw",
    "channels": 1,
    "activation": "sigmoid",
    "scale_factor": 1,
    "color_space": "alpha_matte",
    "packing": "rgba8",
    "temporal": {
      "ema_smooth_default": 0.2,
      "scene_cut_reset": true,
      "depth_norm_mode": "ema"
    }
  }
}
```

#### 3.3 Tüm Görev Türlerini Kapsayan Esneklik (Gausian / ComfyUI Dersi)
- **`task` Alanı:** `"matte" | "depth" | "upscale" | "style" | "custom"` (Rust tarafında açık string veya `AiManifestTask` + bilinmeyen değerleri `Custom(String)` olarak karşılayan `#[serde(untagged)]` yapı — böylece gelecekteki bir görev için bile derleme gerekmez!).
- **Genelleştirilmiş Ön-İşleme (`scripts/ai_infer_sidecar.py`):**
  1. **Renk Uzayı (`input.color_space`):** `"rgb"` | `"bgr"` (OpenCV tarzı modeller) | `"rgba"` | `"luma"`.
  2. **Normalizasyon (`input.normalization.kind`):**
     - `"zero_to_one"`: `x / 255.0` (Real-ESRGAN / SPAN / SwinIR ONNX upscaler'lar!)
     - `"neg_one_to_one"`: `(x / 255.0 - 0.5) / 0.5` (`isnetis` / `anime-segmentation`)
     - `"imagenet"`: `(x / 255.0 - mean) / std` (`Depth-Anything-V2`, `BiRefNet`)
     - `"custom"`: kullanıcı tanımlı `mean`, `std`, `scale`
  3. **Tensor Düzeni & Tipi:** `"nchw"` / `"nhwc"` ve `"float32"` / `"float16"` (`np.float16`, DirectML FP16 modelleri için büyük hızlanma!) / `"uint8"`.
  4. **Çözünürlük Politikası (`resize_policy.mode`):**
     - `"fixed"`: sabit `(width, height)` (ör. BiRefNet `1024×1024`).
     - `"aspect_snap"`: en-boy oranını koruyup kısa kenarı hedefe alıp `multiple_of` katına yuvarla (`Depth-Anything-V2` için `14`, `SAM 2` için `16`/`32`).
     - `"native"`: girdi çözünürlüğünde çalıştır (`upscale` ve `style` için, gerekirse `multiple_of` mirror padding ile çalışıp çıktıdan pad'i kırp!).
  5. **Tiling Desteği (`input.tiling: { tile_size, overlap }`):** 1080p/4K `upscale` veya `style` ONNX modellerinde VRAM taşmasını önlemek için kareyi örtüşen parçalara bölüp doğrusal harmanlama (feather blend) ile birleştirir.
  6. **Yardımcı Girdi Tensorları (`aux_inputs`, ör. `SAM 2` için):** CLI/MCP'den `--params-json '{"points": [[0.5, 0.4]], "labels": [1]}'` geçildiğinde manifest'teki `aux_inputs` eşlemesi (`source: "prompt.point_coords"`, `source: "prompt.point_labels"`, `source: "constant"`) üzerinden ek ONNX input tensorları (`point_coords`, `point_labels`, `has_mask_input` vb.) bağlanır! *(Not: İki aşamalı `SAM 2` encoder+decoder modelleri için `auxiliary_stages` veya `pre_encoder` alanı opsiyonel olarak eklenebilir; tek-graf birleştirilmiş ONNX `sam2` ise doğrudan `aux_inputs` ile çalışır.)*
- **Genelleştirilmiş Son-İşleme & Çıktı Boyutu:**
  1. `output.output_index` (ör. BiRefNet'te `-1` son kafa çıkışı, `0` ilk çıkış) veya `output.tensor_name`.
  2. `output.activation`: `"none"` | `"sigmoid"` | `"tanh"` (`[-1,1] -> [0,1]`) | `"clamp01"`.
  3. `output.channels` & `color_space`:
     - `1` kanal (`matte` / `depth`): `rgba8` (`[v, v, v, v]` veya depth `[v, v, v, 255]`) veya `rg16` (`16-bit` depth).
     - `3` / `4` kanal (`upscale` / `style`): RGB/BGR → RGBA8 (`A=255` veya girdi alpha kanalı lanczos/bicubic ölçeklenerek korunur!).
  4. **`output.scale_factor`:** Rust `run_ai_manifest_inference` çıkış buffer boyutunu `(w * scale_factor) * (h * scale_factor) * 4` olarak hesaplar.

#### 3.4 Güvenlik, SHA-256 ve Lisans Doğrulaması
1. **Path Traversal Engeli:** `entrypoint` (ve varsa ek model dosyaları) mutlaka model klasörü altında kalmalıdır (`..`, mutlak yol ve symlink dışa kaçışı reddedilir).
2. **SHA-256 Doğrulaması (`frameshift-media/src/frame.rs` `Sha256`):** Sidecar Python süreci başlatılmadan önce Rust tarafı `FfmpegBridge::hash_file(&entrypoint_path)` çalıştırır ve manifest'teki `sha256` ile karşılaştırır (büyük model dosyalarında stream okuyuculu `BufReader` 64 KiB chunk'larla okur).
3. **Deklaratif Güvenlik Sınırı:** `model_manifest.json` içinde asla keyfi Python kodu, komut satırı enjeksiyonu veya dinamik `.py` import yolu bulunmaz (`ai_infer_sidecar.py` sabit izinli dönüşüm primitifleri kümesini uygular).
4. **CLI & MCP:**
   - CLI: `ai-models [--models-dir <dir>] [--json]` — keşfedilen tüm manifest'leri, SHA-256 durumunu, lisans bilgisini ve görev türünü listeler.
   - CLI: `ai-run <in.mp4> --model-id <id> --out <out.mp4> [--params-json <json>] [--provider auto|dml|cuda|cpu] [--allow-unverified]`
   - MCP: `ai_capabilities` genişlemesi (`discovered_models: Vec<AiModelManifestSummary>`) + `ai_run_model`.

---

## Değiştirilecek / Eklenecek Dosyalar Özeti

| Crate / Dizin | Dosya | Değişiklik Özeti |
| :--- | :--- | :--- |
| `frameshift-schema` | `src/project.rs`, `src/session.rs`, `src/lib.rs`, `src/error.rs` | `TickKeyframe`, `KeyframeTrackDef` (`#[serde(default)]` ADR-0004), `Project::validate` kontrolü, `ProjectCommand::UpsertKeyframeTrack` |
| `frameshift-schema` / `frameshift-core` | `src/nle/mod.rs`, `src/nle/otio.rs`, `src/nle/fcpxml.rs` *(yeni)* | `.otio` JSON + saf Rust FCPXML 1.10/1.11 okuyucu/yazıcı, `Flicks` ↔ `RationalTime` / `"N/Ds"` dönüştürücü, `export_nle` / `import_nle` |
| `frameshift-render` | `shaders/fx_optical_flow_step.wgsl` *(minör opsiyonel)*, `src/compositor.rs`, `src/tracker.rs` *(yeni)* | ROI-kırpmalı `readback_rg16f_roi`, `track_point` & `track_planar` (FB consistency, IRLS/DLT homografi, NCC drift koruması), `ProjectAnimator` içinde `keyframe_tracks` eval |
| `frameshift-media` | `src/ai_manifest.rs` *(yeni)*, `src/ai_inference.rs`, `src/lib.rs` | `AiModelManifest` parser + SHA-256 + path safety doğrulayıcı, `discover_ai_models`, `run_ai_manifest_inference` (`scale_factor` destekli) |
| `scripts/` | `scripts/ai_infer_sidecar.py` | `--manifest` modu: genel tensor layout/dtype/color_space/norm/resize/tiling/aux_inputs/activation/scale_factor |
| `frameshift-cli` | `src/workbench.rs` | `track-point`, `track-planar`, `export-nle`, `import-nle`, `ai-models`, `ai-run` |
| `frameshift-mcp` | `src/server.rs` | `track_point`, `track_planar`, `export_nle`, `import_nle`, `ai_run_model` araçları + `ai_capabilities` model listesi |

---

## Doğrulanmamış Noktalar

- ROI-kırpmalı `Rgba16Float` readback'in gerçek GPU (DirectX 12 / Vulkan) sürücüsünde kare başı milisaniye maliyeti ve 4 seviyeli piramitte `track-point` / `track-planar` alt-piksel doğruluk metriği henüz kodlanıp ölçülmemiştir.
- `.otio` ve `.fcpxml` çıktılarının DaVinci Resolve / Premiere Pro / Final Cut Pro masaüstü uygulamalarıyla gerçek import uyumluluğu henüz uçtan uca test edilmemiştir.
- `SAM 2` ve `BiRefNet` ONNX modelleri henüz yerel ortama indirilip `--manifest` boru hattında denenmemiştir.

---

## Kullanıcıya Sorulması Gereken Kritik Kararlar

Implementasyona geçmeden önce yalnızca şu **2 mimari yol ayrımı** kullanıcı onayına sunulmalıdır:

1. **Tracker Keyframe Depolama Modeli (`KeyframeTrackDef` vs `SafeExpr`):**
   - **Seçenek A (Önerilen):** `Project` üzerine `#[serde(default)]` ile opsiyonel `keyframe_tracks: Vec<KeyframeTrackDef>` (`tick: Flicks`, `value: f64`, `easing`, `target_node_id`, `parameter`) eklemek. ADR-0004 (`0.2.0`) ile tam geriye uyumludur, O(log K) çalışır ve `SafeExpr` AST/fuel sınırına çarpmaz.
   - **Seçenek B:** Şemayı hiç değiştirmeden `SafeExpr`'e `pwl(t, ...)` fonksiyonu ekleyip keyframe'leri `ParamBindingDef.expression` string'ine gömmek (`MAX_EXPR_NODES=1024` sınırının artırılmasını gerektirir ve proje JSON'unu okunmaz kılar).
2. **FCPXML Okuyucu ve AI Manifest SHA-256 Politikası:**
   - **FCPXML İçe Aktarım:** **0-crate** saf Rust minimal XML okuyucu *(Önerilen)* mı yazılsın, yoksa tam XML/DTD uyumluluğu için hafif bir harici crate (`roxmltree` / `quick-xml`) mi eklensin?
   - **AI Manifest SHA-256 Doğrulaması:** `model_manifest.json` içinde `sha256` eksik veya uyuşmuyorsa varsayılan olarak **katı reddetme (`Strict`)** + açık `--allow-unverified` opt-in bayrağı *(Önerilen)* mı uygulansın, yoksa yalnızca uyarı (`Warn`) mı verilsin?
