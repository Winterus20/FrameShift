---
title: Anime4K Tam WGSL Zinciri ve Real-ESRGAN Upscaler Mimarisi
date: 2026-11-24
status: active
tags: ["shaders", "render-graph", "ai", "upscale", "cli", "mcp"]
---

# Anime4K Tam WGSL Zinciri ve Real-ESRGAN Upscaler Mimarisi

## Soru / Amaç

FrameShift'in mevcut `wgpu`/WGSL GPU kompozisyon motoru (`frameshift-render`) ve harici Vulkan CLI medya hattı (`frameshift-media`) üzerine iki tamamlayıcı anime kalite/çözünürlük yükseltme katmanı nasıl eklenmelidir?

1. **Gerçek Zamanlı WGSL Katmanı (Anime4K v4.x, 0 yeni crate):** `bloc97/Anime4K` projesinin çok geçişli GLSL mimarisi (`Clamp_Highlights`, `Restore_CNN_S/M`, `Restore_CNN_Soft_S/M`, `Upscale_CNN_x2_S/M`, `Upscale_Denoise_CNN_x2_S/M`, `Darken_HQ`, `Thin_HQ` ve `Mode A/B/C/A+A/B+B/C+A` zincirleri), FrameShift'in `Rgba16Float` / Linear Rec.709 / Premultiplied Alpha sözleşmesine nasıl port edilmeli; mevcut tek-geçişli `NodeKind::AnimeSharpen` ([fx_anime_sharpen.wgsl](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/shaders/fx_anime_sharpen.wgsl)) ile ilişkisi ve `NodeKind::Anime4K` şema tasarımı ne olmalıdır?
2. **Çevrimdışı Süper-Çözünürlük Katmanı (`realesrgan-ncnn-vulkan`):** `rife-ncnn-vulkan` entegrasyon kalıbı ([interpolator.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-media/src/interpolator.rs)) ve `ClipAnalysis` (anime kare tekilleştirme + sert kesim koruması) örnek alınarak `frameshift-media/src/upscaler.rs`, CLI `upscale` ve MCP aracı nasıl tasarlanmalıdır; Windows ortamındaki kurulum ve model (`realesr-animevideov3`, `realesrgan-x4plus-anime`) kısıtları nelerdir?

---

## İncelenen Birincil Kaynaklar

### Harici Birincil Kaynaklar (Erişim: 2026-11-24)
- [bloc97/Anime4K (MIT License)](https://raw.githubusercontent.com/bloc97/Anime4K/master/LICENSE)
- [Anime4K v4.x GLSL_Instructions_Windows_MPV.md](https://raw.githubusercontent.com/bloc97/Anime4K/master/md/GLSL_Instructions_Windows_MPV.md)
- [Anime4K v4.x GLSL_Instructions_Advanced.md](https://raw.githubusercontent.com/bloc97/Anime4K/master/md/GLSL_Instructions_Advanced.md)
- [Anime4K Low-end input.conf Presetleri](https://raw.githubusercontent.com/bloc97/Anime4K/master/md/Template/GLSL_Windows_Low-end/input.conf)
- [Anime4K High-end input.conf Presetleri](https://raw.githubusercontent.com/bloc97/Anime4K/master/md/Template/GLSL_Windows_High-end/input.conf)
- Anime4K v4.0/v3.2 GLSL Shader Kaynakları:
  - [Anime4K_Clamp_Highlights.glsl](https://raw.githubusercontent.com/bloc97/Anime4K/master/glsl/Restore/Anime4K_Clamp_Highlights.glsl)
  - [Anime4K_Restore_CNN_S.glsl](https://raw.githubusercontent.com/bloc97/Anime4K/master/glsl/Restore/Anime4K_Restore_CNN_S.glsl)
  - [Anime4K_Restore_CNN_M.glsl](https://raw.githubusercontent.com/bloc97/Anime4K/master/glsl/Restore/Anime4K_Restore_CNN_M.glsl)
  - [Anime4K_Restore_CNN_Soft_S.glsl](https://raw.githubusercontent.com/bloc97/Anime4K/master/glsl/Restore/Anime4K_Restore_CNN_Soft_S.glsl)
  - [Anime4K_Restore_CNN_Soft_M.glsl](https://raw.githubusercontent.com/bloc97/Anime4K/master/glsl/Restore/Anime4K_Restore_CNN_Soft_M.glsl)
  - [Anime4K_Upscale_CNN_x2_S.glsl](https://raw.githubusercontent.com/bloc97/Anime4K/master/glsl/Upscale/Anime4K_Upscale_CNN_x2_S.glsl)
  - [Anime4K_Upscale_CNN_x2_M.glsl](https://raw.githubusercontent.com/bloc97/Anime4K/master/glsl/Upscale/Anime4K_Upscale_CNN_x2_M.glsl)
  - [Anime4K_Upscale_Denoise_CNN_x2_S.glsl](https://raw.githubusercontent.com/bloc97/Anime4K/master/glsl/Upscale+Denoise/Anime4K_Upscale_Denoise_CNN_x2_S.glsl)
  - [Anime4K_Upscale_Denoise_CNN_x2_M.glsl](https://raw.githubusercontent.com/bloc97/Anime4K/master/glsl/Upscale+Denoise/Anime4K_Upscale_Denoise_CNN_x2_M.glsl)
  - [Anime4K_AutoDownscalePre_x2.glsl](https://raw.githubusercontent.com/bloc97/Anime4K/master/glsl/Upscale/Anime4K_AutoDownscalePre_x2.glsl) & [AutoDownscalePre_x4.glsl](https://raw.githubusercontent.com/bloc97/Anime4K/master/glsl/Upscale/Anime4K_AutoDownscalePre_x4.glsl)
  - [Anime4K_Darken_HQ.glsl](https://raw.githubusercontent.com/bloc97/Anime4K/master/glsl/Experimental-Effects/Anime4K_Darken_HQ.glsl) & [Anime4K_Thin_HQ.glsl](https://raw.githubusercontent.com/bloc97/Anime4K/master/glsl/Experimental-Effects/Anime4K_Thin_HQ.glsl)
- [xinntao/Real-ESRGAN-ncnn-vulkan (MIT License)](https://raw.githubusercontent.com/xinntao/Real-ESRGAN-ncnn-vulkan/master/LICENSE)
- [Real-ESRGAN-ncnn-vulkan README](https://raw.githubusercontent.com/xinntao/Real-ESRGAN-ncnn-vulkan/master/README.md)
- [Real-ESRGAN-ncnn-vulkan main.cpp](https://raw.githubusercontent.com/xinntao/Real-ESRGAN-ncnn-vulkan/master/src/main.cpp) & [realesrgan.cpp](https://raw.githubusercontent.com/xinntao/Real-ESRGAN-ncnn-vulkan/master/src/realesrgan.cpp)
- [xinntao/Real-ESRGAN v0.2.5.0 Release](https://github.com/xinntao/Real-ESRGAN/releases/tag/v0.2.5.0) & [anime_video_model.md](https://raw.githubusercontent.com/xinntao/Real-ESRGAN/master/docs/anime_video_model.md)

### Yerel Kod Tabanı ve Sözleşme Kaynakları
- [effect-shader-api.md](file:///C:/Users/Yigit/Documents/FrameShift/brain/specs/effect-shader-api.md) — WGSL sözleşmesi, 16-byte uniform, Naga/TDR kuralları
- [fx_anime_sharpen.wgsl](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/shaders/fx_anime_sharpen.wgsl) & [color_basic.wgsl](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/shaders/color_basic.wgsl)
- [pool.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/src/pool.rs), [context.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/src/context.rs), [uniforms.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/src/uniforms.rs), [pipelines.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/src/pipelines.rs), [compositor.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/src/compositor.rs)
- [graph.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-schema/src/graph.rs) & [project.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-schema/src/project.rs)
- [resolver.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-core/src/resolver.rs) & [workbench.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-core/src/workbench.rs)
- [interpolator.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-media/src/interpolator.rs), [analysis.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-media/src/analysis.rs), [ai_inference.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-media/src/ai_inference.rs), [frame.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-media/src/frame.rs)
- [crates/frameshift-cli/src/workbench.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-cli/src/workbench.rs) & [crates/frameshift-mcp/src/server.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-mcp/src/server.rs)

---

## Doğrudan Gözlemler (Kanıtlı)

### 1. Mevcut FrameShift Render Hattı ve Sözleşmeleri
- **Çalışma Yüzeyi ve Texture Pool:** [pool.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/src/pool.rs#L4-L107) tüm ara düğüm çıktıları için `WORKING_TEXTURE_FORMAT = wgpu::TextureFormat::Rgba16Float` (8 byte/piksel, `Linear Rec.709`, **Premultiplied Alpha**) kullanır. `TexturePool::acquire(&device, width, height)` istenen her `(width, height)` çözünürlüğü için doku kiralayıp `release` ile geri alır; `NodeKind::DeepGlow` ([compositor.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/src/compositor.rs#L3004-L3129)) ve `NodeKind::OpticalFlowBlur` ([compositor.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/src/compositor.rs#L2210-L2274)) bu havuzu çok geçişli piramit/ping-pong dokular için aktif kullanmaktadır.
- **Bağlama Düzeni ve Limitler:** [pipelines.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/src/pipelines.rs#L1137-L1261) `build_program` ve `create_bind_group_layout` üzerinden `texture_count` parametresiyle keyfi sayıda `texture_2d<f32>` + 1 `sampler` + 1 `uniform` bağlayabilir; şu an `compositor.rs` tarafında `record_one_tex_pass`, `record_two_tex_pass`, `record_three_tex_pass` yardımcıları vardır ([compositor.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/src/compositor.rs#L5847-L5991)). [context.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/src/context.rs#L39) cihazı `wgpu::Limits::default()` ile oluşturur (WebGPU standardı: aşama başına `max_sampled_textures_per_shader_stage = 16`).
- **Kaynak Çözünürlüğü Kritik Bulgu:** [resolver.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-core/src/resolver.rs#L1075-L1101) ve [compositor.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/src/compositor.rs#L2325-L2424) içinde `NodeKind::SourceVideo` ve `NodeKind::SourceImage`, `provider.provide_source_frame(asset_id, src_tick, width, height, fit)` çağrısıyla kaynağı CPU'da **proje tuvali `(width, height)` boyutuna ölçekledikten sonra** GPU'ya yükler (`Rgba8Unorm` -> `ColorBasic` `SrgbToLinear` -> `Rgba16Float`). Dolayısıyla 720p/480p bir kaynak 1080p/4K tuvale yerleştirildiğinde grafik düğümleri halihazırda ölçeklenmiş piksel ızgarasını alır.
- **Mevcut `AnimeSharpen`:** [fx_anime_sharpen.wgsl](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/shaders/fx_anime_sharpen.wgsl#L1-L143) tek geçişli (single-pass) Sobel 3x3 algısal luma (`sqrt(dot(rgb, LUMA_REC709))`) gradyanı yönünde karanlık çizgi inceltme ve 3x3 min/max `anti_ringing` kelepçeli unsharp keskinleştirme uygular; hiçbir CNN ağırlığı, `Clamp_Highlights` veya `x2` alt-piksel (`Depth-to-Space`) katmanı içermez. `AnimeSharpenUniform` 32 byte'tır (`[texel_size: [f32; 2], line_strength, sharpen_amount, anti_ringing, _pad: [f32; 3]]`, [uniforms.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/src/uniforms.rs#L708-L733)).
- **Naga / TDR Sınırları:** [effect-shader-api.md](file:///C:/Users/Yigit/Documents/FrameShift/brain/specs/effect-shader-api.md#L83-L95) ve [pipelines.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/src/pipelines.rs#L1089-L1131) `while` ve sınırsız `loop {` bloklarını yasaklar (`MAX_SHADER_LOOP_ITERATIONS = 128`); `size_of::<Uniform>() % 16 == 0` zorunludur.
- **0 Yeni Crate Kuralı Uyumu:** [crates/frameshift-render/Cargo.toml](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/Cargo.toml) (`wgpu`, `bytemuck`, `glam`, `half`, `pollster`, `thiserror`) ve [crates/frameshift-media/Cargo.toml](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-media/Cargo.toml) (`png = "0.18"`, `frameshift-time`, `serde`, `serde_json`, `thiserror`) zaten PNG I/O, GPU shader derleme ve harici süreç çağrısı için gereken tüm crate'lere sahiptir.

### 2. Anime4K v4.x GLSL Mimarisi (Doğrudan Shader İncelemesi)
`bloc97/Anime4K` MIT lisanslıdır; tüm `.glsl` shader'ları CReLU (`concat(max(x, 0), max(-x, 0))` — bu nedenle `4` kanallı ara dokudan `go_0` ve `go_1` ile `8` sanal kanal okunur!) ve gömülü `mat4` konvolüsyon ağırlıkları kullanır:

| Shader | Geçiş Sayısı | Ara Doku Bağımlılığı & Bağlama | Çıkış / İşlem | Boyut |
| :--- | :---: | :--- | :--- | :---: |
| `Clamp_Highlights` | 3 | `HOOKED` -> `STATSMAX` (5x1 max) -> `STATSMAX` (1x5 max) -> `HOOKED`+`STATSMAX` | Luma'yı 5x5 yerel maksimuma kelepçeler (`dot(vec4(0.299, 0.587, 0.114, 0.0), rgba)`) | 2,795 B |
| `Restore_CNN_S` | 4 | `MAIN` -> `conv2d_tf` -> `conv2d_1_tf` -> `conv2d_2_tf` -> `MAIN + conv(3x3x3x8)` (en fazla **2** aynı anda canlı ara doku) | 1x `Conv(4x3x3x3)`, 2x `Conv(4x3x3x8)`, 1x `Conv(3x3x3x8)` + residual `MAIN` | 17,136 B |
| `Restore_CNN_Soft_S` | 4 | `Restore_CNN_S` ile özdeş topoloji, downscale/aliasing için yumuşatılmış ağırlıklar | Aynı | 17,198 B |
| `Restore_CNN_M` | 8 | `MAIN` -> `conv2d_tf` .. `conv2d_6_tf` (7 ara doku); **Son geçiş (`Conv-3x1x1x56`) 8 dokuyu (`MAIN` + `conv2d_tf..conv2d_6_tf`) aynı anda bağlar** | DenseNet skip bağlantısı (`56 = 7 * 8` CReLU kanalı) + residual `MAIN` | 35,916 B |
| `Restore_CNN_Soft_M` | 8 | `Restore_CNN_M` ile özdeş topoloji, yumuşatılmış ağırlıklar | Aynı | 36,016 B |
| `Upscale_CNN_x2_S` | 5 | `MAIN` -> `conv2d_tf` -> `conv2d_1_tf` -> `conv2d_2_tf` -> `conv2d_last_tf` -> `Depth-to-Space (2w, 2h)` | Son geçiş `conv2d_last_tf`'den `2x2` alt-piksel luma artığı `c0` çekip `vec4(c0) + MAIN_tex` ekler | 18,638 B |
| `Upscale_CNN_x2_M` | 9 | `MAIN` + `conv2d_tf..conv2d_6_tf` -> `Conv-4x1x1x56` (`conv2d_last_tf`) -> `Depth-to-Space (2w, 2h)` (8. geçişte **7** ara doku aynı anda bağlanır) | DenseNet skip + `2x2` sub-pixel shuffle + residual `MAIN` | 37,685 B |
| `Upscale_Denoise_CNN_x2_S` / `M` | 5 / 9 | Sırasıyla `Upscale_CNN_x2_S` / `M` ile özdeş katman topolojisi | Gürültü bastırma eğitimli ağırlıklar + `Depth-to-Space` | 18,667 / 37,714 B |
| `AutoDownscalePre_x2` / `x4` | 1 | `HOOKED` -> `OUTPUT.w x OUTPUT.h` (veya `OUTPUT.w/2`) | mpv içi koşullu ara ölçekleme (FrameShift GPU DAG'da ayrı shader gerektirmez; `TexturePool` boyutuyla yönetilir) | ~1.5 KB |
| `Darken_HQ` | 5 | `LINELUMA` (1 ch), `LINEKERNEL` (1 ch) — ayrılabilir DoG Gaussian-X/Y (`SPATIAL_SIGMA = 1.0 * H/1080`) | `HOOKED + comp_gaussian_y() * STRENGTH` (karanlık çizgileri koyulaştırır) | 5,981 B |
| `Thin_HQ` | 8 | `LINELUMA` (1 ch), `LINESOBEL` (2 ch / 1 ch) — Sobel + `pow(norm, 0.7)` + ayrılabilir Gaussian + ikinci gradyan + ileri UV warp | `HOOKED_tex(pos - dd)` (çizgileri inceltir; `ITERATIONS` döngüsü sabit üst sınırlı) | 5,962 B |

- **Resmi Mode Presetleri (`GLSL_Windows_Low-end/input.conf` & `High-end/input.conf`):**
  - **Mode A** *(Fast / Low-end)*: `Clamp_Highlights` -> `Restore_CNN_M` -> `Upscale_CNN_x2_M` -> (`AutoDownscalePre_x2/x4`) -> `Upscale_CNN_x2_S` *(HQ: `M` yerine `VL` + `M`)*.
  - **Mode B** *(Fast)*: `Clamp_Highlights` -> `Restore_CNN_Soft_M` -> `Upscale_CNN_x2_M` -> (`AutoDownscale`) -> `Upscale_CNN_x2_S`.
  - **Mode C** *(Fast)*: `Clamp_Highlights` -> `Upscale_Denoise_CNN_x2_M` -> (`AutoDownscale`) -> `Upscale_CNN_x2_S`.
  - **Mode A+A / B+B / C+A**: İlk `Upscale_x2` sonrası ikinci `Restore_CNN(_Soft)_S` + ikinci `Upscale_CNN_x2_S` eklenir (yalnızca `>= 2x` ölçeklemede önerilir; 1080p->1080p'de aşırı keskinleştirme/ringing riski belgelenmiştir).
- **Renk Uzayı ve Alpha Sözleşmesi:** Tüm Anime4K v4 GLSL shader'ları **non-linear (sRGB/gamma-encoded) BT.601/709** aralığında (`[0, 1]`) eğitilmiş ağırlıklar içerir ve BT.601 luma `dot(rgb, vec3(0.299, 0.587, 0.114))` üzerinden `result + MAIN_tex` yapar. FrameShift'in `Rgba16Float` dokuları ise **Linear Rec.709 + Premultiplied Alpha** taşır.

### 3. Real-ESRGAN (`realesrgan-ncnn-vulkan`) ve Yerel Ortam Yoklaması
- **Yerel Ortam Yoklaması (2026-11-24):**
  - `where.exe realesrgan-ncnn-vulkan`: Bulunamadı (kurulu değil).
  - `where.exe rife-ncnn-vulkan`: `C:\Users\Yigit\scoop\shims\rife-ncnn-vulkan.exe` (Scoop `extras` kurulu).
  - `scoop search realesrgan` & `scoop cat realesrgan-ncnn-vulkan`: Scoop `main` bucket'ında `realesrgan-ncnn-vulkan` sürüm **`0.2.0`** tanımlı (`https://github.com/xinntao/Real-ESRGAN-ncnn-vulkan/releases/download/v0.2.0/realesrgan-ncnn-vulkan-v0.2.0-windows.zip`, SHA256 `1bbbdb12d470af80b035c773682e144c6c2f6ece9210832a289af0a48ce3fa9a`).
  - **Kritik Sürüm Uyumsuzluğu:** `realesr-animevideov3` (`-s 2`, `-s 3`, `-s 4` destekli hızlı kompakt anime video modeli) **`xinntao/Real-ESRGAN` `v0.2.5.0`** sürümüyle (`realesrgan-ncnn-vulkan-20220424-windows.zip`, 45,474,481 byte) yayımlanmıştır. Scoop `main` bucket'ındaki `0.2.0` sürümü `20220424` (`v0.2.5.0`) paketinden eskidir!
  - `vulkaninfo --summary`: Vulkan Instance 1.4.341; `GPU0`: `Intel(R) Iris(R) Xe Graphics` (integrated), **`GPU1`: `NVIDIA GeForce RTX 4050 Laptop GPU`** (discrete, Vulkan API 1.4.351, sürücü `616.92`). `GPU0` entegre GPU olduğu için Vulkan cihaz seçiminde `-g` bayrağına dikkat edilmelidir (`auto` veya discrete GPU indeksi).
- **`realesrgan-ncnn-vulkan` CLI ve Kaynak Kodu İncelemesi (`main.cpp` & `realesrgan.cpp`):**
  - Desteklenen CLI bayrakları: `-i <infile|dir>`, `-o <outfile|dir>`, `-s <2|3|4>` (varsayılan `4`), `-t <tile-size>` (`>=32` veya `0=auto`; `heap_budget > 1900 MB` ise otomatik `200`, `>550` ise `100`), `-m <model-path>` (klasör adında `"models"` veya `"models2"` geçmesi `main.cpp` içinde zorunlu kontrol edilir!), `-n <model-name>` (`realesr-animevideov3` [varsayılan], `realesrgan-x4plus`, `realesrgan-x4plus-anime`, `realesrnet-x4plus`), `-g <gpu-id>`, `-j <load:proc:save>` (varsayılan `1:2:2`), `-x` (TTA modu), `-f <jpg|png|webp>`, `-v`.
  - **Model dosya eşlemesi (`main.cpp`):** `-n realesr-animevideov3` seçildiğinde ikili `<model_dir>/realesr-animevideov3-x{scale}.param` ve `.bin` arar; diğer modellerde `<model_dir>/{modelname}.param` ve `.bin` arar.
  - **Klasör (Batch) Modu ve Boru Hattı:** `stdin`/`stdout` pipe desteği **yoktur** (`-i` ve `-o` dosya veya ikisi birden klasör olmalıdır). Ancak klasör modunda 3 aşamalı paralel kuyruk (`load -> proc -> save`, varsayılan `1:2:2`) kullanır; böylece tek proses içinde tüm benzersiz kareler NCNN modeli bir kez yüklenerek toplu işlenir (`rife-ncnn-vulkan`'daki kare-çifti başına süreç başlatma maliyeti ödenmez!).
  - **Alpha Kanalı Davranışı (`realesrgan.cpp`):** `rife-ncnn-vulkan`'ın aksine (`save_rgb_png_for_rife` yorumu), `realesrgan-ncnn-vulkan` `channels == 4` PNG'leri destekler (RGB süper-çözünürlük + alpha kanalı NCNN `Interp` `bicubic_2x/3x/4x` katmanıyla ölçeklenir). Ancak video akışlarında (`Mp4FrameEncoder` / YUV420p/YUV422p10) alpha gereksizdir; 3-kanallı RGB PNG yazmak hem disk I/O'yu %25 azaltır hem de WIC/stb kanal uyumsuzluklarını önler (isteğe bağlı straight-alpha koruması `channels == 4` PNG ile ayrılabilir).
  - **Mevcut FrameShift Kalıbı (`interpolator.rs` & `analysis.rs`):** [ClipAnalysis::analyze_frame_sequence](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-media/src/analysis.rs#L73-L129) 160x90 Rec.709 luma MAD ile anime 2'li/3'lü tekrar karelerini `unique_runs` içine katlar (`dedup_threshold = 0.008`) ve `scene_cuts` sınırlarını bulur. `realesrgan-ncnn-vulkan` kare-bazlı mekansal (spatial) bir model olduğundan `scene_cuts` kaliteyi bozmaz, ancak **`unique_runs` tekilleştirmesi** tipik 24fps "on-twos/on-threes" anime kliplerinde AI upscale iş yükünü **%50–%66 azaltır**!

---

## Çıkarımlar ve Öneriler

> Aşağıdaki mimari öneriler doğrudan gözlemlenen kod tabanı ve birincil kaynak verilerinden çıkarılmıştır.

### A. Anime4K Tam WGSL Zinciri (`frameshift-render` & `frameshift-schema`, 0 Yeni Crate)

#### 1. Renk Uzayı Köprüsü ve Premultiplied Alpha Koruması (Kritik Gereksinim)
*Çıkarım:* Anime4K GLSL ağırlıkları sRGB `[0..1]` aralığında eğitildiği için, FrameShift'in `Linear Rec.709` `Rgba16Float` dokuları üzerinde doğrudan `Conv` çalıştırmak koyu tonları ve çizgi gradyanlarını bozar. Çözüm:
- Zincirin **giriş geçişinde** (`Pass 0` veya `Clamp_Highlights` başında) `unpremul(rgba)` -> `linear_to_srgb(rgb)` dönüşümü yapılmalı; `alpha == 0` saf additive/boş pikseller ([fx_anime_sharpen.wgsl](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/shaders/fx_anime_sharpen.wgsl#L62-L76) kalıbıyla) korunmalıdır.
- Zincirin **çıkış geçişinde** (son `Restore` / `Depth-to-Space` / `Darken` sonrasında) `clamp(rgb, 0.0, 1.0)` -> `srgb_to_linear(rgb)` -> `rgb * alpha` (re-premultiply) uygulanarak `Rgba16Float` sözleşmesine dönülmelidir.

#### 2. `fx_anime_sharpen.wgsl` ile İlişki ve Şema Tasarımı (`NodeKind::Anime4K`)
- **Geriye Dönük Uyumluluk:** Mevcut `NodeKind::AnimeSharpen` ([graph.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-schema/src/graph.rs#L751-L758)) ve [fx_anime_sharpen.wgsl](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/shaders/fx_anime_sharpen.wgsl) **korunmalıdır** (Faz 6 testleri ve var olan projeler buna bağlıdır). Hafif/hızlı tek-geçiş çizgi inceltme isteyen sahneler `AnimeSharpen` kullanmaya devam eder; gerçek CNN restorasyon/upscale için yeni `NodeKind::Anime4K` eklenir.
- **Önerilen `NodeKind::Anime4K` Şema İmzası (`crates/frameshift-schema/src/graph.rs`):**

```rust
pub enum Anime4KMode {
    ModeA,        // Clamp + Restore_CNN + Upscale_CNN_x2 (1080p / sıkıştırma artığı temizleme)
    ModeB,        // Clamp + Restore_CNN_Soft + Upscale_CNN_x2 (720p / downscale aliasing)
    ModeC,        // Clamp + Upscale_Denoise_CNN_x2 (480p / gürültülü kaynak)
    ModeAA,       // ModeA + Restore_CNN_S + Upscale_CNN_x2_S (2x+ ağır restorasyon)
    ModeBB,       // ModeB + Restore_CNN_Soft_S + Upscale_CNN_x2_S
    ModeCA,       // ModeC + Restore_CNN_S + Upscale_CNN_x2_S
    RestoreOnly,  // Yalnızca Clamp + Restore_CNN (1:1 çözünürlükte çizgi/detay onarımı)
}

pub enum Anime4KQuality {
    FastS,      // S/M ağırlıklı (düşük VRAM, <2 ara doku, 60fps hedefi)
    BalancedM,  // M ağırlıklı (DenseNet 8-geçiş, yüksek kalite)
}

// NodeKind içinde:
Anime4K {
    #[serde(default)]
    mode: Anime4KMode,
    #[serde(default)]
    quality: Anime4KQuality,
    #[serde(default = "default_true")]
    clamp_highlights: bool,
    #[serde(default)]
    darken_strength: f32, // 0.0 = kapalı, 0.0..=2.0 (Darken_HQ geçişleri)
    #[serde(default)]
    thin_strength: f32,   // 0.0 = kapalı, 0.0..=2.0 (Thin_HQ geçişleri)
    #[serde(default = "default_one")]
    mix: f32,             // 0.0..=1.0 orijinal ile harman (SafeExpr animatable)
}
```
- `ALL_SERDE_NAMES` içine `"anime4_k"` (veya `#[serde(rename = "anime4k")]`) eklenir; `expected_input_count() -> 1`. `animatable_param_names` (`project.rs`): `&["darken_strength", "thin_strength", "mix"]`.

#### 3. WGSL Port Stratejisi: Sabit Gömülü Ağırlıklar vs Storage Buffer & Çoklu Kanal Paketleme
- **Ağırlık Depolama Karşılaştırması:**
  - *Seçenek 1 (Önerilen — Gömülü `const` `mat4x4<f32>` / `vec4<f32>` WGSL dosyaları):* `Restore_CNN_S` (17 KB), `Restore_CNN_M` (36 KB), `Upscale_CNN_x2_S` (19 KB), `Upscale_CNN_x2_M` (38 KB) boyutları küçüktür. Naga sabit matris çarpımlarını doğrudan register/immediate olarak derler; ekstra storage buffer descriptor/bind-group karmaşası çıkmaz ve `effect-shader-api.md` `@group(0)` uniform düzeniyle tam uyumludur. (Tek dikkat: `M` varyantının 8 geçişini tek dev shader yerine her katman veya katman grubu için ayrı entry-point/modüle bölmek Naga derleme süresini ve register basıncını düşük tutar.)
  - *Seçenek 2 (`var<storage, read>` ağırlık tamponu):* Tüm CNN katmanlarını tek parametrik `conv3x3` / `conv1x1` shader'ı ile çalıştırır ama `pipelines.rs` içindeki sadece `Uniform` destekleyen `create_bind_group_layout` altyapısını genişletmeyi gerektirir. S/M boyutları için gereksizdir.
- **Ara Kanal Paketleme vs `Rgba16Float`:**
  - Tüm CNN katmanları zaten `//!COMPONENTS 4` (`vec4`) üretir; bu nedenle `TexturePool`'un `Rgba16Float` dokularıyla 1:1 eşleşir, ek kanal paketleme gerekmez.
  - `Clamp_Highlights` (`COMPONENTS 1`), `Darken_HQ` (`COMPONENTS 1`) ve `Thin_HQ` (`COMPONENTS 1..2`) `Rgba16Float`'ın yalnızca `.r`/`.rg` kanallarını kullanır. `TexturePool` havuzunu kirletmemek için ayrı `R16Float` havuzu açmak yerine mevcut `Rgba16Float` havuzunu kullanmak sıfır altyapı maliyeti sağlar (isteyen Faz 3'te `R16Float` havuzuyla bu 3 yardımcı shader'ın VRAM kullanımını 4x azaltabilir).
- **8-Doku Bağlama (`Conv-3x1x1x56` ve `Conv-4x1x1x56`):**
  - `pipelines.rs` içindeki `build_program(..., texture_count, ...)` zaten herhangi bir `texture_count <= 16` değerini destekler. `compositor.rs`'ye yalnızca `record_multi_tex_pass(encoder, label, program, views: &[&wgpu::TextureView], uniform: &U, target_view: &wgpu::TextureView)` genel yardımcısı eklenmesi yeterlidir (`M` varyantının son `1x1x56` katmanı `8` doku bağlar: `MAIN` + `conv2d_0..6`, WebGPU 16 limitinin yarısı).
- **Çözünürlük / Ölçekleme Etkileşimi (Önemli Mimari Not):**
  - Mevcut `SourceVideo` tuval boyutuna (`width x height`) CPU'da ölçekleyip yüklediği için ([resolver.rs#L1085](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-core/src/resolver.rs#L1085)), örneğin 720p klip + 1080p tuval senaryosunda `Upscale_CNN_x2` düğümü iki biçimde çalıştırılabilir:
    1. *Supersample Modu (Değişiklik gerektirmez):* `Anime4K` düğümü tuval boyutunda `Restore_CNN` çalıştırır; `Upscale_CNN_x2` ise `(width*2, height*2)` `PooledTexture` alıp `Depth-to-Space` sonrası `Transform2D`/Catmull-Rom veya `AutoDownscale` geçişiyle `(width, height)`'e geri örnekler (1080p->2160p->1080p supersampling etkisi).
    2. *Gerçek Kaynak-Çözünürlüklü Upscale (Önerilen Faz 2 eklentisi):* `SourceVideo`/`SourceImage` için opsiyonel `native_resolution_before_graph` veya `Anime4K`'nın `width/2, height/2` ölçeğinden çalışması yerine, düşük çözünürlüklü kaynakları proje öncesi `realesrgan-ncnn-vulkan` veya `SourceVideo` üzerinde GPU lanczos/Anime4K ile yükseltmek daha temiz sonuç verir.

#### 4. VRAM ve GPU Maliyet Analizi (Tahmini Analitik Bütçe, RTX 4050 Laptop GPU)
- **Tek `Rgba16Float` doku boyutu:** `1920x1080` = `16.59 MB` (`15.82 MiB`); `3840x2160` = `66.36 MB` (`63.28 MiB`).
- **`FastS` (`Restore_CNN_S` + `Upscale_CNN_x2_S`):** Ping-pong nedeniyle aynı anda en fazla **2 ara doku + kaynak + çıkış** = `~63 MiB` @ 1080p (Depth-to-Space çıkışı `2x` = `+63 MiB` toplam `~126 MiB`).
- **`BalancedM` (`Restore_CNN_M` + `Upscale_CNN_x2_M`):** `Conv-3x1x1x56` ve `Conv-4x1x1x56` katmanları 7 ara özellik haritasını aynı anda canlı tutar -> 7 × 15.82 MiB = `~111 MiB` (artı giriş/çıkış ile `@1080p ~142 MiB`; eğer `Upscale_x2_M` sonrası ikinci aşama `4K` üzerinde çalıştırılırsa 7 × 63.3 MiB = `~443 MiB` tepe VRAM).
- **FPS Hedefi:** `GLSL_Instructions_Advanced.md` 24fps için `<41 ms`, 60fps için `<16 ms` bütçesi hedefler. RTX 4050 üzerinde `FastS` rahatlıkla 60fps+ hedeflenebilir; `BalancedM` ise özellikle `2x` süper-örnekleme zincirlerinde (`A+A` gibi) kare süresini 2x–4x artıracağından `Draft` önizleme profilinde otomatik `FastS` veya bypass seçeneği sunulması önerilir.

---

### B. Real-ESRGAN (`realesrgan-ncnn-vulkan`) Sürücü ve Pipeline Tasarımı (`frameshift-media`)

#### 1. Yeni Modül: `crates/frameshift-media/src/upscaler.rs`
[interpolator.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-media/src/interpolator.rs) ile aynı desen izlenerek, ancak **kare başına process spawn yerine sahne/sekans bazlı klasör batching** kullanarak tasarlanmalıdır:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RealEsrganModel {
    #[default]
    AnimeVideoV3,      // "realesr-animevideov3" (-s 2, 3, 4; hızlı, zamansal tutarlılık iyi)
    AnimeX4Plus,       // "realesrgan-x4plus-anime" (-s 4; illüstrasyon/detaylı 2D kareler)
    GeneralX4Plus,     // "realesrgan-x4plus" (-s 4; karma/live-action sahneler)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpscaleConfig {
    pub scale: u32,                     // 2, 3 veya 4 (AnimeX4Plus/GeneralX4Plus için 4 zorunlu)
    pub model: RealEsrganModel,
    pub model_dir: Option<PathBuf>,     // None ise exe yanı "models" dizini; Not: adında "models" geçmeli!
    pub tile_size: u32,                 // 0 = auto, >= 32
    pub gpu_id: Option<i32>,            // None = auto (RTX 4050 iGPU+dGPU ortamında açık seçim önemli)
    pub tta: bool,                      // -x (8x yavaş ama test/kalite modu)
    pub dedup_threshold: f32,           // 0.008 (ClipAnalysis; 0.0 kapatır)
    pub scene_cut_threshold: f32,       // 0.25 (raporlama & segment izolasyonu)
    pub realesrgan_binary: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpscaleReport {
    pub input_frames: usize,
    pub unique_frames_upscaled: usize,
    pub duplicate_frames_reused: usize,
    pub scene_cuts: Vec<usize>,
    pub scale: u32,
    pub input_width: u32,
    pub input_height: u32,
    pub output_width: u32,
    pub output_height: u32,
    pub model_name: String,
}
```

#### 2. `ClipAnalysis` (Dedup) + Batch Klasör Yürütme Akışı
1. **Ön Analiz:** `ClipAnalysis::analyze_frame_sequence(&refs, config.dedup_threshold, config.scene_cut_threshold)` çalıştırılır.
2. **Benzersiz Temsilcileri Yazma:** Yalnızca `analysis.unique_runs` içindeki her `run.start_frame` temsilcisi geçici `in_dir/f_{run_idx:08}.png` dosyasına **3-kanallı 8-bit RGB PNG** olarak yazılır (`png::ColorType::Rgb`, [save_rgb_png_for_rife](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-media/src/interpolator.rs#L414-L431) ile aynı yardımcı `save_rgb_png` altında ortaklaştırılabilir; düz alpha korunacaksa `Rgba` seçilir).
3. **Tek CLI Çağrısı (Batch Mod):**
   `realesrgan-ncnn-vulkan -i <in_dir> -o <out_dir> -n <model> -s <scale> -f png [-t <tile>] [-g <gpu_id>] [-m <models_dir>] [-x]`
   - `rife-ncnn-vulkan`'daki kare başı süreç açma yerine tek bir süreç modeli bir kez VRAM'e yükler ve dahili `1:2:2` load/proc/save iş parçacığı havuzuyla işler.
   - `main.cpp` içindeki `model.find("models") != npos` şartı nedeniyle özel model dizini kullanılırsa klasör yolunun `"models"` alt dizinini içermesi garanti edilmelidir.
4. **Tekrarlı Kare Çoğaltma ve PTS Koruma:** Çıkış `out_dir/f_{run_idx:08}.png` okunur (`Rgba8Frame::load_png`), ardından her `DuplicateRun` için `run.count` kadar orijinal `frames[run.start_frame + offset].pts` (`Flicks`) korunarak çıkış vektörüne klonlanır. RAII `TempDirGuard` `Drop` ile geçici klasörü temizler.
5. **Video Sarmalayıcısı (`upscale_video_file`):** Büyük videolarda bellek taşmasını önlemek için tek seferde tüm videoyu RAM'e açmak yerine örneğin `64–120` karelik pencereler (chunk) halinde `FfmpegBridge::decode_video_window` -> `upscale_frame_sequence` -> `Mp4FrameEncoder::new_with_profile(..., w * scale, h * scale, ...)` akışı izlenmelidir (1080p -> 4K dönüşümde 10 sn'lik klip 240 kare × 33 MB ≈ ~8 GB ham RGBA8 eder; pencereleme zorunludur!).

#### 3. `ai-status`, CLI `upscale` ve MCP Entegrasyonu
- **`ai_inference.rs` (`AiCapabilitiesReport`):** `realesrgan_available: bool`, `realesrgan_path: Option<PathBuf>`, `realesrgan_models_found: Vec<String>` alanları eklenir; `frameshift-cli ai-status` ve MCP `ai_capabilities` otomatik olarak raporlar.
- **CLI (`crates/frameshift-cli/src/workbench.rs`):**
  `frameshift-cli upscale <in.mp4> --out <out.mp4> [--scale 2|3|4] [--model realesr-animevideov3|realesrgan-x4plus-anime|realesrgan-x4plus] [--dedup <f32>] [--tile <u32>] [--gpu <i32>] [--tta]`
- **MCP (`crates/frameshift-mcp/src/server.rs`):**
  `upscale_media` (veya `upscale_video`) MCP aracı tanımlanır: `in_path`, `out_path`, `scale` (`2|3|4`), `model`, `dedup_threshold`, `tile_size`, `gpu_id`.
- **Scoop vs GitHub `v0.2.5.0` Kurulum Uyarısı:** Scoop `main` bucket'ındaki `0.2.0` sürümü `realesr-animevideov3` modelini içermeyebilir. Kurulum belgesinde/hata mesajında kullanıcıya öncelikle `xinntao/Real-ESRGAN` `v0.2.5.0` (`realesrgan-ncnn-vulkan-20220424-windows.zip`) kurulumu önerilmeli; ikili bulunamazsa net yönlendirme ile hata dönülmelidir (`BinarySpawnFailed`).

---

## Doğrulanmamış Noktalar

1. **Scoop `realesrgan-ncnn-vulkan` (`v0.2.0`) ZIP içeriği:** Scoop manifesti `v0.2.0` sürümüne işaret ediyor; `v0.2.0` arşivinde `realesr-animevideov3-x2/x3/x4` model dosyalarının yer almadığı (`v0.2.5.0` ile geldiği) tahmin edilmektedir, ancak arşiv indirilip içi listelenmediği için kesin model listesi doğrulanmamıştır.
2. **RTX 4050 üzerinde gerçek kare süreleri (benchmark):** Anime4K `FastS` ve `BalancedM` WGSL zincirlerinin ve `realesrgan-ncnn-vulkan`'ın RTX 4050 + Intel Iris Xe çift-GPU Windows dizüstü ortamındaki gerçek ms/kare süreleri henüz kodlanıp ölçülmemiştir; yukarıdaki VRAM sayıları doku boyutundan hesaplanmış analitik alt sınırlardır.
3. **Naga WGSL derleme süresi (`Restore_CNN_M` / `Upscale_CNN_x2_M`):** Çok sayıda sabit `mat4x4<f32>` literal'i içeren WGSL modüllerinin `wgpu`/`naga` üzerinde ilk derleme süresi (cold-start compile ms) henüz test edilmemiştir.

---

## Kullanıcı Kararı Gerektiren Açık Noktalar

1. **Anime4K WGSL Zincir Kapsamı (Faz 2 Hedefi):**
   - **Seçenek A (Önerilen Varsayılan):** `Clamp_Highlights` + `Restore_CNN_S` + `Restore_CNN_Soft_S` + `Upscale_CNN_x2_S` + `Upscale_Denoise_CNN_x2_S` + `Darken_HQ` + `Thin_HQ` + `Mode A/B/C` (tümü `FastS` profili, en fazla 2 canlı ara doku, düşük VRAM ve 60fps+ önizleme dostu).
   - **Seçenek B:** Seçenek A'ya ek olarak `Restore_CNN_M` + `Restore_CNN_Soft_M` + `Upscale_CNN_x2_M` + `Upscale_Denoise_CNN_x2_M` ve `A+A`/`B+B`/`C+A` (`BalancedM` DenseNet 8-geçişli zincir, `record_multi_tex_pass` gerektirir).
   - **Seçenek C:** Yalnızca `Restore_CNN_S` + `Clamp_Highlights` + `Darken_HQ` (2x `Depth-to-Space` upscale olmadan sadece 1:1 çizgi onarımı).

2. **`NodeKind::Anime4K` vs `NodeKind::AnimeSharpen` ve Çözünürlük Davranışı:**
   - **Seçenek A (Önerilen Varsayılan):** Mevcut `NodeKind::AnimeSharpen` korunur; yanına `NodeKind::Anime4K { mode, quality, clamp_highlights, darken_strength, thin_strength, mix }` eklenir. `Upscale_CNN_x2` varsayılan olarak tuval boyutunda `2x -> 1x` supersampling yapar (mevcut `SourceVideo` tuval ölçekleme mimarisini bozmaz).
   - **Seçenek B:** `SourceVideo` düğümüne `native_resolution: bool` opsiyonu eklenip `Anime4K` düğümünün doğrudan düşük çözünürlüklü ham kare üzerinde `2x` çalıştırılması sağlanır (daha yüksek gerçek süper-çözünürlük kalitesi, ancak grafik içi heterojen çözünürlük akışı değişikliği gerektirir).

3. **`realesrgan-ncnn-vulkan` Kurulumu ve Model Tercihi:**
   - **Seçenek A (Önerilen Varsayılan):** Kullanıcının onayıyla `xinntao/Real-ESRGAN` `v0.2.5.0` (`realesrgan-ncnn-vulkan-20220424-windows.zip`, MIT) kurularak `realesr-animevideov3` (`2x/3x/4x`) + `realesrgan-x4plus-anime` desteklenir; mevcut Scoop `0.2.0` yerine güncel `20220424` paketi kullanılır.
   - **Seçenek B:** `scoop install realesrgan-ncnn-vulkan` (`0.2.0`) ile kurulur; `realesr-animevideov3` eksikse yalnızca `realesrgan-x4plus-anime` / `realesrgan-x4plus` ile sınırlı kalınır.
   - **Seçenek C:** Şimdilik harici ikili kurulmaz; yalnızca `upscaler.rs` arayüzü, `ClipAnalysis` batch mantığı ve sahte/mock CLI testleri yazılır.
