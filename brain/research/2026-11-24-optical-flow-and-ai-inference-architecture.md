---
title: "Optik Akış (Pyramidal GPU RSMB), İnterpolasyon Motoru ve ONNX AI Çıkarım Mimarisi"
date: 2026-11-24
status: active
tags: [render-graph, shaders, media-io, agent]
---

# Optik Akış (Pyramidal GPU RSMB), İnterpolasyon Motoru ve ONNX AI Çıkarım Mimarisi

## Soru / Amaç

FrameShift içindeki `"AI / Optik Akış Çıkarımı Henüz Arabirim Düzeyinde"` boşluğunu **0 yeni Cargo crate** kısıtı altında gerçek çalışan, test edilebilir bir üretim hattına dönüştürmek:
1. GPU üzerinde 2D piksel hareketini kestiren ve karakter/kamera hareketine gerçekçi motion blur uygulayan **Pyramidal Optical Flow (`OpticalFlowBlur`, RSMB eşdeğeri)** düğümü.
2. Anime *on twos/threes* (`ClipAnalysis`) ve sert sahne kesimi korumalı **canlı kare/video interpolasyon motoru** (`RifeNcnnCli`, `FfmpegMinterpolate`, `FrameBlend`).
3. Geçici PNG darboğazı olmadan doğrudan bellek/raw-pipe üzerinden çalışan **ONNX Runtime AI Rotoscope & Depth Map** sidecar köprüsü.

## İncelenen Birincil Kaynaklar

- **Pyramidal Optical Flow (Lucas-Kanade & Horn-Schunck):**
  - Bouguet, J.-Y., *Pyramidal Implementation of the Lucas Kanade Feature Tracker* (Intel Research): https://robots.stanford.edu/cs223b04/algo_tracking.pdf
  - Horn, B. K. P. & Schunck, B. G., *Determining Optical Flow* (MIT AI Lab): https://dspace.mit.edu/handle/1721.1/6337
  - RE:Vision Effects RSMB (Scene-cut sensitivity ve flow-guided gather kavramı): https://revisionfx.com/products/rsmb/
- **RIFE & Video İnterpolasyon:**
  - `nihui/rife-ncnn-vulkan` (MIT): https://github.com/nihui/rife-ncnn-vulkan
  - FFmpeg `minterpolate` filtresi (`mi_mode=mci:mc_mode=aobmc:me_mode=bidir:vsbmc=1`): https://ffmpeg.org/ffmpeg-filters.html#minterpolate
- **ONNX Runtime & Açık Lisanslı Modeller:**
  - ONNX Runtime Execution Providers (DirectML / CUDA / CPU): https://onnxruntime.ai/docs/execution-providers/
  - `SkyTNT/anime-segmentation` (Apache-2.0): https://github.com/SkyTNT/anime-segmentation
  - `ZhengPeng7/BiRefNet` (MIT): https://github.com/ZhengPeng7/BiRefNet
  - `DepthAnything/Depth-Anything-V2` (ViT-S Apache-2.0): https://github.com/DepthAnything/Depth-Anything-V2

---

## Mimari Kararlar ve Tasarım

### 1. Native GPU Pyramidal Optical Flow & RSMB (`NodeKind::OpticalFlowBlur`)
Harici AI bağımlılığı olmadan doğrudan `wgpu` / WGSL boru hattında çalışır:
- **4 Seviyeli Görüntü Piramidi (`L0: 1/1`, `L1: 1/2`, `L2: 1/4`, `L3: 1/8`):**
  - `fx_optical_flow_luma.wgsl`: `Rgba16Float` renk dokusundan algısal Rec.709 luma (`pow(max(luma, 0), 0.5)`) çıkarır; karanlık anime sahnelerinde gradyan duyarlılığını korur. Piramit seviyeleri `mipmap_downsample` ile oluşturulur.
- **Coarse-to-Fine Warped Lucas-Kanade + Horn-Schunck Regularization (`fx_optical_flow_step.wgsl`):**
  - En kaba seviyeden (`L3`) başlayarak her seviyede 2 iterasyon çalıştırılır.
  - Kaba akış `u0` 3×3 komşuluktan `flow_smoothness` oranında yumuşatılır, `prev_luma` `uv - u0` konumundan örneklenir (warp).
  - 5×5 pencere üzerinde uzamsal türevler (`Ix`, `Iy`) ve warped zamansal fark (`It = curr(uv) - prev(uv - u0)`) ile 2×2 yapı tensörü kurulur.
  - Tikhonov / Horn-Schunck regularization (`alpha = mix(0.002, 0.08, smoothness)`) eklenerek Cramer kuralıyla artımsal akış `du` çözülür ve `u = u0 + clamp(du, -max_step, max_step)` olarak güncellenir.
  - **Sahne Kesimi Koruması (`sensitivity`):** `abs(It)` ortalaması `sensitivity` eşiğini aştığında `confidence = smoothstep(sensitivity * 1.5, sensitivity * 0.4, mean_diff)` ile güven düşürülür; sert kesimlerde sahte smear/blur bastırılır.
- **Yönlü Motion-Blur Gather & HSV Görselleştirme (`fx_optical_flow_blur.wgsl`):**
  - `vel_uv = flow.xy * shutter_amount * 0.5 * confidence`, uzunluk `max_blur_px / resolution` ile sınırlanır.
  - Jimenez (2014) IGN jitter destekli `[-0.5, +0.5]` simetrik N-tap gather; örneklerin yerel flow büyüklüğü uyumuyla kenar sızıntısı (bleeding) azaltılır.
  - `visualize_flow = true` olduğunda yön (`atan2`) → HSV hue ve büyüklük → value haritası basar.

### 2. Canlı Kare/Video İnterpolasyon Motoru (`frameshift-media/src/interpolator.rs`)
- `ClipAnalysis::analyze_frame_sequence` çıktısındaki `unique_runs` ve `scene_cuts` verilerini doğrudan tüketir.
- Video `scene_cuts` sınırlarından bağımsız segmentlere ayrılır; sahne geçişi üzerinden asla interpolasyon yapılmaz.
- Üç arka uç (`InterpolatorBackend`):
  1. `RifeNcnnCli`: `rife-ncnn-vulkan` CLI üzerinden benzersiz kare çiftleri için `-s <t>` çıkarımı.
  2. `FfmpegMinterpolate`: `ffmpeg` rawvideo stdin/stdout boru hattı üzerinden gerçek hareket-dengelemeli (`mi_mode=mci:mc_mode=aobmc:me_mode=bidir:vsbmc=1`) segment interpolasyonu.
  3. `FrameBlend`: Saf Rust deterministik lineer kare harmanlama yedeği.

### 3. Canlı ONNX Runtime AI Sidecar Köprüsü (`frameshift-media/src/ai_inference.rs` + `scripts/ai_infer_sidecar.py`)
- `Cargo.toml`'a yeni crate eklemeden mevcut sistem `python` + `onnxruntime` (`1.32.0`) ortamıyla rawvideo stdin/stdout pipe protokolü üzerinden haberleşir (sıfır geçici PNG darboğazı).
- Execution Provider sırası: `DmlExecutionProvider` → `CUDAExecutionProvider` → `CPUExecutionProvider`.
- **Modlar:**
  - `rotoscope`: Tek kanallı veya `[1, 1, H, W]` olasılık maskesini `[0, 1]` aralığına normalize eder, opsiyonel temporal EMA smoothing uygular ve `MaskMatte` (`Alpha`/`Luma`/`Red`) ile doğrudan uyumlu RGBA8 `[m, m, m, m]` çıktısı üretir.
  - `depth`: Monoküler derinlik çıktısını robust yüzdelik normalizasyon + temporal EMA ile `DepthMatte` uyumlu `[d, d, d, 255]` çıktısına dönüştürür.
  - `--synthetic-test`: Model dosyası yokken deterministik kenar/luma tabanlı çıktı üreterek uçtan uca pipe doğrulaması sağlar.

## Doğrulanmamış Noktalar

- Yerel ortamda `rife-ncnn-vulkan` ve `.onnx` model ağırlık dosyaları kurulu değildir; bunların yüksek FPS/kalite benchmark'ları gerçek model/binary eklendiğinde ölçülecektir. `ffmpeg`/`ffprobe`, `wgpu` GPU shader boru hattı ve `python` + `onnxruntime` raw-pipe köprüsü canlı ortamda test edilmektedir.
