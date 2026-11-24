---
title: "AI Medya Araçları Rehberi: RIFE (rife-ncnn-vulkan), Rust ONNX Runtime (ort) ve Rotoscope / Depth Modelleri"
date: 2026-11-24
status: active
tags: [media-io, render-graph, shaders, licensing]
---

# AI Medya Araçları Rehberi: RIFE (`rife-ncnn-vulkan`), `ort` ve ONNX Rotoscope / Depth

## Soru / Amaç

Ünlü anime editörlerinin kullandığı **Twixtor** (akıcı yavaş çekim / velocity) ve **Roto Brush** (karakteri arka plandan ayırma) yeteneklerinin FrameShift için açık kaynaklı, lisans uyumlu ve Rust/`wgpu` mimarisiyle bütünleşebilen karşılıklarını (`rife-ncnn-vulkan`, `ort`, ONNX rotoscope ve depth modelleri) tek referans belgesinde toplamak.

## İncelenen Birincil Kaynaklar

- `nihui/rife-ncnn-vulkan` (MIT): https://github.com/nihui/rife-ncnn-vulkan — erişim tarihi: 2026-11-24
- `hzwer/Practical-RIFE` (MIT): https://github.com/hzwer/Practical-RIFE — erişim tarihi: 2026-11-24
- `pykeio/ort` (MIT / Apache-2.0): https://github.com/pykeio/ort — erişim tarihi: 2026-11-24
- `SkyTNT/anime-segmentation` (Apache-2.0): https://github.com/SkyTNT/anime-segmentation — erişim tarihi: 2026-11-24
- `ZhengPeng7/BiRefNet` (MIT): https://github.com/ZhengPeng7/BiRefNet — erişim tarihi: 2026-11-24
- `facebookresearch/sam2` (Apache-2.0): https://github.com/facebookresearch/sam2 — erişim tarihi: 2026-11-24
- `hkchengrex/Cutie` (MIT): https://github.com/hkchengrex/Cutie — erişim tarihi: 2026-11-24
- `DepthAnything/Depth-Anything-V2` (Kod Apache-2.0; ViT-S ağırlığı Apache-2.0, ViT-B/L/G CC-BY-NC-4.0): https://github.com/DepthAnything/Depth-Anything-V2 — erişim tarihi: 2026-11-24
- Lisans kısıtlı modeller (hariç tutma referansı):
  - `Bria-AI/RMBG-2.0` (CC BY-NC 4.0): https://github.com/Bria-AI/RMBG-2.0
  - `pq-yang/MatAnyone` (NTU S-Lab Non-Commercial): https://github.com/pq-yang/MatAnyone
  - `PeterL1n/RobustVideoMatting` (GPL-3.0): https://github.com/PeterL1n/RobustVideoMatting

## Doğrudan Gözlemler (Kavramlar ve FrameShift Zemini)

### 1. `rife-ncnn-vulkan` (RIFE) — Akıcı Yavaş Çekim / Twixtor Karşılığı
- **Nedir:** İki video karesi arasındaki optik akışı yapay zekayla hesaplayarak arada bulunmayan kesirli ara kareleri (`t ∈ (0, 1)`) üreten RIFE (*Real-Time Intermediate Flow Estimation*) motorunun `ncnn` + Vulkan tabanlı hafif CLI uygulamasıdır.
- **Neden önemli:** 24 fps animeyi velocity/slow-mo ile yavaşlatırken kare tekrarı kaynaklı takılmayı ortadan kaldırır; Python veya CUDA kurulumu olmadan NVIDIA/AMD/Intel Vulkan GPU'larda çalışır.
- **Anime tuzağı (`dedup` + SCD):** Anime *on twos / threes* (12/8 fps) çizildiğinden önce duplike kareler temizlenmeli ve sert sahne kesimleri (SCD) tespit edilmelidir; aksi halde duraksama ve sahne arası morphing oluşur.
- **FrameShift'teki mevcut zemin:** [`crates/frameshift-media/src/analysis.rs`](../../crates/frameshift-media/src/analysis.rs) içinde `ClipAnalysis::analyze_frame_sequence` (`dedup` + sahne kesimi tespiti), `InterpolatorBackend::RifeNcnnCli` ve `-s <t>` argüman üreticisi (`build_rife_pair_args`) P0 aşamasında kurulmuştur.

### 2. `ort` (Rust ONNX Runtime) — Uygulama İçi GPU AI Çıkarımı
- **Nedir:** `.onnx` formatındaki yapay zeka modellerini Python olmadan doğrudan Rust kodunun içinden çalıştıran `ort` crate'idir (`DirectML`, `CoreML`, `WebGPU` vb. execution provider destekli).
- **Neden önemli:** `rife-ncnn-vulkan` gibi harici CLI araçlarında oluşan geçici PNG disk yazma/okuma darboğazını kaldırır; doğrudan bellekteki kare tamponlarını GPU'da işler.
- **Not:** `AGENTS.md` kuralı gereği yeni harici crate eklemesi kullanıcı onayıyla P2 adımında yapılır; şu an `Cargo.toml` dosyalarına eklenmemiştir.

### 3. ONNX Rotoscope & Depth — Karakteri Arka Plandan Ayırma (Roto Brush Karşılığı)
- **Nedir:** Anime klibinden karakter alfa maskesi (matte) veya derinlik haritası (depth map) üreterek dev kinetik yazıların (`SC-0002`), ışık efektlerinin ve parçacıkların karakterin **arkasında** kalmasını sağlayan AI segmentasyon hattıdır.
- **Ticari kullanıma uygun açık lisanslı model adayları:**
  - `SkyTNT/anime-segmentation` (`Apache-2.0`): Hızlı tek-kare anime karakter ayrımı.
  - `BiRefNet` (`MIT`): Saç ve ince konturlar için yüksek kenar kalitesi.
  - `SAM 2` (`Apache-2.0`) / `Cutie` (`MIT`): Video kareleri boyunca titremeyen (flicker-free) nesne takibi.
  - `Depth-Anything-V2-Small (ViT-S)` (`Apache-2.0`): Monoküler depth-map üretimi *(yalnızca `ViT-S` Apache-2.0'dır; `ViT-B/L/G` CC-BY-NC-4.0 olduğundan ticari dağıtımdan hariç tutulur)*.
- **Hariç tutulması gerekenler:** `RMBG-2.0` (`CC BY-NC 4.0`), `MatAnyone` (Non-Commercial) ve `RobustVideoMatting` (`GPL-3.0`).
- **FrameShift'teki mevcut zemin:** [`MaskMatte`](../../crates/frameshift-render/shaders/mask_matte.wgsl) (`Alpha`, `Luma`, `Red`, `opacity`, `choke_px`, `spatial_feather_px`) ve [`DepthMatte`](../../crates/frameshift-schema/src/graph.rs) (`near_depth`, `far_depth`, `feather`, `invert`) düğümleri P0 aşamasında hazır hale getirilmiştir; AI çıkarımı offline ön-işleme (`frameshift-cli matte` / `depth`) ile önbellekli matte/depth videosu üreterek bu düğümleri besleyecektir.

## Çıkarımlar ve Öneriler

1. **Aşama 1 (P1 — CLI Sidecar, `Cargo.toml` değişmeden):** `rife-ncnn-vulkan` binary'sini `analysis.rs` `ClipAnalysis` çıktısıyla bağlayıp yalnızca benzersiz kare aralıklarında ve sahne kesimi sınırları içinde `-s <t>` interpolasyonu çalıştırmak; ayrıca harici ONNX scripti/sidecar ile üretilen matte/depth videolarını `MaskMatte`/`DepthMatte` üzerinden tüketmek.
2. **Aşama 2 (P2 — Kullanıcı onayıyla `ort` crate'i):** `ort` (`MIT/Apache-2.0`) eklenerek hem RIFE hem `anime-segmentation` / `BiRefNet` / `Depth-Anything-V2-Small` çıkarımını geçici PNG I/O olmadan bellek içi ön-işleme komutuna (`frameshift-cli matte` / `interpolate`) taşımak.

## Doğrulanmamış Noktalar

- Bu belgedeki AI araçları ve modelleri henüz yerel GPU üzerinde çalıştırılıp süre/VRAM/kalite benchmark'ı alınmamıştır; entegrasyon P1/P2 aşamasında gerçek anime klipleriyle doğrulanacaktır.
