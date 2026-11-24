---
title: "Pillar 3 Mimari Araştırması: GPU İçi NV12/YUV420P → Linear Rec.709 Decode & Düğüm Seviyesi İçerik-Hash'li Smart Cache"
date: 2026-11-24
status: implemented
tags: [engine, media-io, render-graph, shaders, performance, cache]
---

# Pillar 3 Mimari Araştırması: GPU İçi NV12/YUV420P → Linear Rec.709 Decode & Düğüm Seviyesi İçerik-Hash'li Smart Cache

> **Uygulama Durumu (2026-11-24):** **0 yeni Cargo crate** ile implemente edildi ve doğrulandı ([uygulama günlüğü](../log/2026-11-24-competitive-power-roadmap-implementation.md)): `ffprobe` renk metadatası + 12-bpp `nv12` pipe + `yuv_to_linear.wgsl` (`R8Unorm`/`Rg8Unorm`, `SourceFit::Contain` `content_uv_rect`, `Rgba8` fallback korumalı) ve `NodeContentHash` tabanlı 3-katmanlı `SmartCache` (`MemoryOnly` varsayılan + `--cache-dir` opt-in Disk LRU). *(Gerçek medya throughput/VRAM benchmark'ları ayrıca ölçülmelidir.)*

## Soru / Amaç

FrameShift'in [rekabetçi güç analizi yol haritasında](2026-11-24-competitive-power-analysis-roadmap.md#L45-L52) belirlenen **Pillar 3 (Motor Hızı)** kapsamındaki iki temel darboğazın, **0 yeni Cargo crate** kısıtı ve Asta v3'ün **`0.27–0.80 GB` bellek bütçesi** ile M1 determinizm/golden-hash garantileri korunarak nasıl tasarlanacağını belirlemek:

1. **GPU İçi NV12 / YUV420P → Linear Rec.709 Decode Hattı (FilmCraft dersi):** FFmpeg raw pipe üzerinden `rgba` (32-bit/piksel) yerine `nv12` / `yuv420p` (12-bit/piksel) okuyup `wgpu` içine `R8Unorm` Y + `Rg8Unorm` UV olarak yükleyerek WGSL shader ile doğrudan `Linear Rec.709` `Rgba16Float` üretmek; `SourceFit`, alfa kanallı kaynaklar ve M1 golden hash uyumunu çözmek.
2. **Düğüm Seviyesi İçerik-Hash'li Smart Cache (DaVinci Resolve Fusion dersi):** `GpuCompositor` değerlendirme döngüsünde ağır düğümler (`Anime4K`, `OpticalFlowBlur`, `Scene3D`, `DeepGlow`, `Precomp`, `CustomWgsl` vb.) için `revision_id`'den bağımsız içerik-hash'li RAM/VRAM + opsiyonel disk LRU önbelleği tasarlamak.

---

## İncelenen Birincil Kaynaklar

- [crates/frameshift-media/Cargo.toml](../../crates/frameshift-media/Cargo.toml) & [crates/frameshift-render/Cargo.toml](../../crates/frameshift-render/Cargo.toml) & [crates/frameshift-core/Cargo.toml](../../crates/frameshift-core/Cargo.toml) — bağımlılık ve 0-crate doğrulaması (inceleme: 2026-11-24)
- [crates/frameshift-media/src/streaming.rs](../../crates/frameshift-media/src/streaming.rs#L11-L707) (`StreamingVideoDecoder`, `DecoderPool`, `StreamKey`, `DEFAULT_MAX_OPEN_STREAMS=8`, `DEFAULT_MAX_SEQUENTIAL_SKIP=32`) — inceleme: 2026-11-24
- [crates/frameshift-media/src/ffmpeg.rs](../../crates/frameshift-media/src/ffmpeg.rs#L436-L1802) (`MediaProbeInfo`, `FfmpegBridge`, `probe_media_info`, `decode_video_window_with_fit`, `decode_video_frame_internal`, `append_fitted_output`, `midpoint_seek_seconds`, `build_ffmpeg_encode_os_args`) — inceleme: 2026-11-24
- [crates/frameshift-media/src/frame.rs](../../crates/frameshift-media/src/frame.rs#L10-L360) (`AlphaMode`, `Rgba8Frame`, `fit_rgba8_frame`, `Sha256`, `Fnv1a64`) — inceleme: 2026-11-24
- [crates/frameshift-media/src/cache.rs](../../crates/frameshift-media/src/cache.rs#L10-L232) (`LruFrameCache`, `FrameCacheKey`, `CacheTelemetry`) — inceleme: 2026-11-24
- [crates/frameshift-core/src/resolver.rs](../../crates/frameshift-core/src/resolver.rs#L284-L1102) (`MediaResolver`, `provide_video_frame`, `provide_image_frame`, `ProjectFrameProvider`, `VectorTextCacheKey`, `HudPathCacheKey`) — inceleme: 2026-11-24
- [crates/frameshift-core/src/engine.rs](../../crates/frameshift-core/src/engine.rs#L37-L720) (`Engine`, `DEFAULT_CACHE_CAPACITY=128`, `DEFAULT_CACHE_MAX_BYTES=1GiB`, `render_video_mp4_internal`, `verify_m1_golden_scene`) — inceleme: 2026-11-24
- [crates/frameshift-render/src/provider.rs](../../crates/frameshift-render/src/provider.rs#L7-L102) (`SourcePixelData`, `SourceFrame`, `FrameProvider`) — inceleme: 2026-11-24
- [crates/frameshift-render/src/pool.rs](../../crates/frameshift-render/src/pool.rs#L4-L241) (`WORKING_TEXTURE_FORMAT`, `TexturePool`, `MsaaTargetCache`) — inceleme: 2026-11-24
- [crates/frameshift-render/src/context.rs](../../crates/frameshift-render/src/context.rs#L19-L110) (`GpuContext`, `wgpu::Features::empty()`, `linear_sampler`) — inceleme: 2026-11-24
- [crates/frameshift-render/src/pipelines.rs](../../crates/frameshift-render/src/pipelines.rs#L132-L256) & [`L1179-L1303`](../../crates/frameshift-render/src/pipelines.rs#L1179-L1303) (`ShaderPipelines`, `build_program`, `create_bind_group_layout_with_dims`) — inceleme: 2026-11-24
- [crates/frameshift-render/src/compositor.rs](../../crates/frameshift-render/src/compositor.rs#L1407-L6710) (`GpuCompositor`, `render_frame`, `evaluate_subgraph_inner`, `evaluate_motion_blur`, `evaluate_optical_flow_blur`, `evaluate_single_node`, `evaluate_anime4k`, `evaluate_scene3d`, `upload_source_frame`, `readback_rgba8`, `premultiply_rgba8`) — inceleme: 2026-11-24
- [crates/frameshift-render/shaders/color_basic.wgsl](../../crates/frameshift-render/shaders/color_basic.wgsl#L1-L129) & [fx_anime4k_bridge.wgsl](../../crates/frameshift-render/shaders/fx_anime4k_bridge.wgsl#L49-L73) — inceleme: 2026-11-24
- [crates/frameshift-schema/src/project.rs](../../crates/frameshift-schema/src/project.rs#L728-L747) & [`L1164-L1340`](../../crates/frameshift-schema/src/project.rs#L1164-L1340) (`Project.revision_id`, `golden_scene_preset`) — inceleme: 2026-11-24
- [crates/frameshift-schema/src/graph.rs](../../crates/frameshift-schema/src/graph.rs#L296-L1250) (`SourceFit`, `NodeKind`) — inceleme: 2026-11-24
- [brain/specs/render-graph.md](../specs/render-graph.md) & [brain/specs/effect-shader-api.md](../specs/effect-shader-api.md) & [brain/log/2026-11-22-asta-v3-team.md](../log/2026-11-22-asta-v3-team.md#L38) — inceleme: 2026-11-24

---

## Doğrudan Gözlemler (Kanıtlı)

### Bölüm 1 Gözlemleri — Mevcut Video Decode ve GPU Yükleme Hattı

1. **FFmpeg Raw Pipe Biçimi (`rgba`, 32 bpp):**
   - `StreamingVideoDecoder::open` ([streaming.rs#L115-L135](../../crates/frameshift-media/src/streaming.rs#L115-L135)) ve `FfmpegBridge::decode_video_window_with_fit` ([ffmpeg.rs#L853-L870](../../crates/frameshift-media/src/ffmpeg.rs#L853-L870)) ile `decode_video_frame_internal` ([ffmpeg.rs#L977-L999](../../crates/frameshift-media/src/ffmpeg.rs#L977-L999)), FFmpeg çıkışını `-pix_fmt rgba -f rawvideo pipe:1` olarak zorlamaktadır.
   - Bu nedenle `1920×1080` bir kare `8.29 MB` (`8,294,400` byte), `1280×720` bir kare `3.69 MB` (`3,686,400` byte) yer kaplamakta; `StreamingVideoDecoder::read_next_frame` ([streaming.rs#L186-L197](../../crates/frameshift-media/src/streaming.rs#L186-L197)) her kare için yeni bir `Vec<u8>` ayırıp pipe'tan kopyalamaktadır.
2. **Eksik Renk ve Chroma Metadata Çıkarımı (`ffprobe`):**
   - `FfmpegBridge::probe_media_info` ([ffmpeg.rs#L634-L640](../../crates/frameshift-media/src/ffmpeg.rs#L634-L640)) ve `FfprobeStream` ([ffmpeg.rs#L1744-L1757](../../crates/frameshift-media/src/ffmpeg.rs#L1744-L1757)) yalnızca `codec_type, codec_name, width, height, pix_fmt, time_base, avg_frame_rate, r_frame_rate, start_pts, duration_ts, sample_rate, channels` alanlarını sorgulamaktadır. `color_space`, `color_transfer`, `color_primaries`, `color_range` ve `chroma_location` alanları şu an okunmamaktadır.
   - Öte yandan dışa aktarım tarafında `build_ffmpeg_encode_os_args` ([ffmpeg.rs#L1644-L1657](../../crates/frameshift-media/src/ffmpeg.rs#L1644-L1657)), `bt709_tags == true` iken `-colorspace bt709 -color_primaries bt709 -color_trc bt709 -color_range tv` etiketlerini yazmaktadır.
3. **`SourceFit` ve CPU Tarafı Alpha Padding Darboğazı:**
   - `append_fitted_output` ([ffmpeg.rs#L1767-L1783](../../crates/frameshift-media/src/ffmpeg.rs#L1767-L1783)): `Stretch` için `-s {w}x{h}`, `Cover` için `scale=...:force_divisible_by=2,crop=...`, `Contain` için ise **`format=rgba,pad={w}:{h}:...:color=0x00000000`** filtresi çalıştırmaktadır. `Contain` modunda şeffaf siyah dolgu üretebilmek için FFmpeg filtre grafiği CPU'da `rgba` dönüşümüne zorlanmaktadır.
4. **GPU Yükleme (`upload_source_frame`) Maliyeti ve Premultiply Sırası:**
   - `GpuCompositor::upload_source_frame` ([compositor.rs#L6094-L6243](../../crates/frameshift-render/src/compositor.rs#L6094-L6243)) her karede `self.ctx.device.create_texture(...)` ile yeni bir geçici `Rgba8Unorm` doku oluşturmakta (`L6123`), `premultiplied == false` ise CPU'da yeni bir `Vec<u8>` ayırıp `premultiply_rgba8(data)` çalıştırmakta (`L6142`, `L7588-L7606`), ardından `color_basic` shader'ı ile `sRGB -> LinearRec709` dönüşümü yaparak hedef `Rgba16Float` havuz dokusuna çizmektedir (`L6234-L6241`).
5. **M1 Golden Scene ve Sentetik Medya Yolu:**
   - `Project::golden_scene_preset()` ([project.rs#L1194-L1228](../../crates/frameshift-schema/src/project.rs#L1194-L1228)) disk dosyası yoksa `MediaResolver::resolve_asset_source` ([resolver.rs#L350-L364](../../crates/frameshift-core/src/resolver.rs#L350-L364)) üzerinden `SyntheticMediaGenerator` (`ClipA`, `ClipB`, `MovingMask`) yoluna düşer ve `Rgba8Frame` üretir.
6. **0-Crate GPU Yeteneği (`wgpu 30.0.1`):**
   - `GpuContext::new_headless` ([context.rs#L36-L41](../../crates/frameshift-render/src/context.rs#L36-L41)) yalnızca `wgpu::Features::empty()` ve `wgpu::Limits::default()` istemektedir. `TextureFormat::R8Unorm` ve `TextureFormat::Rg8Unorm`, WebGPU/wgpu çekirdek spesifikasyonunda ekstra `Features` gerektirmeden `Float { filterable: true }` örneklemeyi ve `COPY_DST | TEXTURE_BINDING` kullanımını destekler ([pipelines.rs#L1260-L1279](../../crates/frameshift-render/src/pipelines.rs#L1260-L1279)).

### Bölüm 2 Gözlemleri — Mevcut DAG Değerlendirme ve Bellek Bütçesi

1. **Mevcut Önbellekler Yalnızca Leaf/Statik Veriyi Tutar:**
   - `Engine` ([engine.rs#L37-L73](../../crates/frameshift-core/src/engine.rs#L37-L73)): `LruFrameCache::with_max_bytes(128, 1 GiB)` yalnızca çözülmüş kaynak karelerini (`Rgba8Frame`) saklar.
   - `MediaResolver` ([resolver.rs#L287-L293](../../crates/frameshift-core/src/resolver.rs#L287-L293)): `vector_text_cache` (`VectorTextCacheKey`, yalnızca dinamik bağlama ve `text_animator` yoksa) ve `hud_path_cache` tutar.
   - `GpuCompositor` ([compositor.rs#L1407-L1421](../../crates/frameshift-render/src/compositor.rs#L1407-L1421)): `TexturePool` ([pool.rs#L30-L128](../../crates/frameshift-render/src/pool.rs#L30-L128)) yalnızca **boş** `Rgba16Float` dokuları tahsisat tekrarı için havuzlar; `lut_cache`, `image_gpu_cache` (yalnız `SourceImage`, sınırsız `HashMap`), `text3d_mesh_cache`, `slug_cache`, `shape3d_mesh_cache` statik geometri/doku tutar.
   - **Hiçbir ara efekt/kompozisyon düğümünün `Rgba16Float` çıktısı kareler, alt-örnekler veya revizyonlar arasında önbelleklenmez.**
2. **Aynı Alt-Grafiğin Tekrar Eden Maliyetleri:**
   - `evaluate_motion_blur` ([compositor.rs#L2184-L2227](../../crates/frameshift-render/src/compositor.rs#L2184-L2227)): her çıktı karesi için `sub_samples` (`4`–`16`) kez `evaluate_subgraph_inner` çalıştırır. Alt-zamanda değişmeyen arka plan/statik dallar kare başına N kez yeniden işlenir.
   - `evaluate_optical_flow_blur` ([compositor.rs#L2255-L2303](../../crates/frameshift-render/src/compositor.rs#L2255-L2303)): her karede hem `curr_tex(tick)` hem `prev_tex(tick - dt)` için üst alt-grafiği baştan değerlendirir. Ardışık karelerde `frame[N]`'in `prev_tex`'i `frame[N-1]`'in `curr_tex`'i ile birebir aynıdır, ancak şu an sıfırdan tekrar hesaplanmaktadır.
   - `evaluate_anime4k` ([compositor.rs#L3877-L4017](../../crates/frameshift-render/src/compositor.rs#L3877-L4017)): 5–12 arası ardışık GPU shader geçişi çalıştırır; `24 fps` kaynak `60 fps` timeline'da aynı kaynak kareyi 2–3 kez tekrarladığında tüm CNN geçişleri tekrar koşar.
   - `DeepGlow` ([compositor.rs#L3121-L3246](../../crates/frameshift-render/src/compositor.rs#L3121-L3246)): 2–6 mip seviyesi downsample + upsample + composite (en fazla 12 geçiş).
   - `evaluate_scene3d` ([compositor.rs#L4387-L5238](../../crates/frameshift-render/src/compositor.rs#L4387-L5238)): 4x MSAA geometri, depth seed, atmosfer ve 3 geçişli Separable CoC + Bokeh DoF çalıştırır.
3. **Asta v3 Bellek Gerçekliği:**
   - [Asta v3 takım günlüğünde](../log/2026-11-22-asta-v3-team.md#L38) ölçüldüğü üzere P0 streaming encode sonrası çalışma kümesi `6.5 GB -> 0.27–0.80 GB` aralığına çekilmiştir (`Fame v3 720p`: `0.27 GB` / private `0.91 GB`; `asta_amv draft 720p`: `0.80 GB`; `asta_v3 draft 640×360`: `0.40 GB`). Baskın RAM terimi `128 kare / 1 GiB` kaynak `LruFrameCache`'tir.
4. **Mevcut Saf Rust Hash ve Float Primitifleri (0 Yeni Crate):**
   - `frameshift-media/src/frame.rs` içinde saf Rust `Sha256` ([L858](../../crates/frameshift-media/src/frame.rs#L858)) ve `Fnv1a64` ([L108](../../crates/frameshift-media/src/frame.rs#L108)); `frameshift-render` içinde `fnv1a_64` ([compositor.rs#L1284](../../crates/frameshift-render/src/compositor.rs#L1284)) ve `half = "2.7.1"` zaten mevcuttur.

---

## Çıkarımlar ve Öneriler (Mimari Tasarım)

*(Aşağıdaki tasarımlar doğrudan kod incelemesine dayalı mimari önerilerdir; henüz implemente edilmemiş ve benchmark ile ölçülmemiştir.)*

---

### Bölüm 1: GPU İçi NV12 / YUV420P → Linear Rec.709 Decode Hattı

#### 1.1. 0-Crate Fizibilitesi ve Bant Genişliği Kazancı

**0 yeni Cargo crate ile %100 uygulanabilir.**

| Aşama | Mevcut (`rgba` 32 bpp) | Önerilen (`nv12` 12 bpp) | Kazanç |
| :--- | :---: | :---: | :---: |
| 1080p kare boyutu | `8,294,400 B` (`8.29 MB`) | `3,110,400 B` (`3.11 MB`) | **%62,5 azalma** |
| 720p kare boyutu | `3,686,400 B` (`3.69 MB`) | `1,382,400 B` (`1.38 MB`) | **%62,5 azalma** |
| `LruFrameCache` (128 × 1080p) | `1.06 GB` (1 GiB tavanını aşar) | `398 MB` | **662 MB RAM tasarrufu** |
| `LruFrameCache` (128 × 720p) | `472 MB` | `177 MB` | **295 MB RAM tasarrufu** |
| CPU Premultiply (`upload_source_frame`) | Her karede `Vec<u8>` ayırma | **0 CPU piksel döngüsü** | **%100 eleme** |
| `wgpu` yükleme bant genişliği | `1 × Rgba8Unorm` (`8.29 MB`) | `R8Unorm` (`2.07 MB`) + `Rg8Unorm` (`1.04 MB`) | **%62,5 PCIe tasarrufu** |

Ek olarak, `nv12` (`Y` düzlemi `W×H`, interleaved `UV` düzlemi `(W/2)×(H/2)×2`) düzlemsel 3-parçalı `yuv420p` (`I420`) formatına göre üç üstünlüğe sahiptir:
1. GPU'da **3 yerine 2 doku** (`R8Unorm` + `Rg8Unorm`) ve tek donanımsal bilinear `UV` örneklemesi gerekir.
2. FFmpeg'in kendi `yuv420p -> nv12` dönüşümü yeniden örnekleme yapmaz, yalnızca `U` ve `V` düzlemlerini bayt düzeyinde serpiştirir (`interleave`).
3. `D3D11VA` / `NVDEC` / `VideoToolbox` / `VAAPI` donanım çözücülerinin doğal çıktı yerleşimi de `NV12` olduğu için ileride sıfır-kopya GPU video decode'a doğrudan uyumludur.

#### 1.2. Veri Akışı, `ffprobe` Renk Metadata'sı ve `DecodedFrame` Soyutlaması

1. **`frameshift-media/src/ffmpeg.rs` — `MediaProbeInfo` Genişletmesi:**
   `probe_media_info` içindeki `-show_entries stream=...` listesine `color_space,color_transfer,color_primaries,color_range,chroma_location` eklenir (`FfprobeStream` ve `MediaProbeInfo` `#[serde(default)]` uyumlu alanlar alır):
   ```rust
   #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
   pub enum YuvColorMatrix {
       Bt709,
       Bt601,
       Bt2020Ncl,
   }

   #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
   pub enum YuvColorRange {
       Limited, // tv / mpeg: Y [16..235], UV [16..240]
       Full,    // pc / jpeg: Y [0..255], UV [0..255]
   }

   #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
   pub enum ChromaLocation {
       Left,    // H.264/HEVC/MPEG-2 varsayılanı: (0.0, +0.5 luma satırı) -> +0.5/w
       Center,  // JPEG/MPEG-1: +0.0
       TopLeft, // UHD BT.2020: (+0.5/w, +0.5/h)
   }

   #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
   pub struct YuvColorInfo {
       pub matrix: YuvColorMatrix,
       pub range: YuvColorRange,
       pub chroma_location: ChromaLocation,
   }
   ```
   - **Sezgisel Varsayılan (Untagged Kaynaklar İçin):** `color_space` eksik/unknown ise `height >= 720 || width >= 1280` için `Bt709`, aksi halde `Bt601`; `color_range` eksikse `Limited`; `chroma_location` eksikse `Left` seçilir (FFmpeg `swscale` + H.264 standardı ile uyumlu).
2. **`frameshift-media/src/frame.rs` — `Nv12Frame` ve `DecodedFrame`:**
   - Kamu API'sindeki `FfmpegBridge::decode_video_frame` (`-> Rgba8Frame`) **değiştirilmez**; mevcut testler ve `Mp4FrameEncoder` etkilenmez.
   - Yeni `Nv12Frame`:
     ```rust
     #[derive(Debug, Clone, PartialEq, Eq)]
     pub struct Nv12Frame {
         pub width: u32,        // Çift sayı (2'ye tam bölünür)
         pub height: u32,       // Çift sayı
         pub pts: Flicks,
         pub color: YuvColorInfo,
         /// GPU'da Contain letterbox uygulamak için [u_min, v_min, u_max, v_max] (normalde [0,0,1,1])
         pub content_uv_rect: [f32; 4],
         /// Tek bitişik tampon: [0 .. y_len] = Y, [y_len .. y_len + uv_len] = interleaved UV
         pub data: Vec<u8>,
         pub y_len: usize,
     }
     ```
     *Not:* Tek `Vec<u8>` içinde `Y || UV` tutmak hem `read_exact(&mut buf)` ile tek syscall okuma hem de `LruFrameCache` için tek `buf.len()` byte muhasebesi sağlar (`&frame.data[..frame.y_len]` ve `&frame.data[frame.y_len..]`).
   - `LruFrameCache`: `CacheEntry` içinde `Rgba8Frame` yerine `Arc<CachedMediaFrame>` (`Rgba8(Rgba8Frame) | Nv12(Nv12Frame)`) veya ayrı tiplenmiş `CachedVideoFrame` saklanır; `byte_size()` ikisinde de `data.len()` döndürür.
3. **`frameshift-render/src/provider.rs` — `SourcePixelData` Genişletmesi:**
   ```rust
   pub enum SourcePixelData {
       Rgba8 { data: Vec<u8>, premultiplied: bool },
       RgbaF32 { data: Vec<[f32; 4]>, premultiplied: bool },
       Nv12 {
           y_plane: Vec<u8>, // veya Arc<[u8]> zero-copy paylaşım için!
           uv_plane: Vec<u8>,
           color: YuvColorInfo,
           content_uv_rect: [f32; 4],
       },
       Yuva420p {
           y_plane: Vec<u8>,
           u_plane: Vec<u8>,
           v_plane: Vec<u8>,
           a_plane: Vec<u8>,
           color: YuvColorInfo,
           content_uv_rect: [f32; 4],
       },
   }
   ```
   *(Öneri: `ProjectFrameProvider` her karede `LruFrameCache`'ten `.clone()` ile `Vec<u8>` kopyalamak yerine `Arc<Vec<u8>>` / slice referansı taşıyabilirse 1080p'de ekstra `3.11 MB` CPU `memcpy` de sıfırlanır.)*

#### 1.3. `SourceFit`, Alfa Kaynakları, Tek Boyutlar ve M1 Golden Hash Uyumu

1. **`SourceFit` (`Stretch` / `Cover` / `Contain`) Çözümü:**
   - **`Stretch` ve `Cover`:** Mevcut `append_fitted_output` (`-s` veya `scale+crop`) **aynen `nv12` ile çalışır** (çift tuval boyutlarında).
   - **`Contain` (Kritik Nokta!):** Mevcut kod `Contain` için FFmpeg içinde `format=rgba,pad=...:color=0x00000000` kullanıyor çünkü YUV420p alfa taşıyamaz. Bunu GPU'ya taşıyarak **hem alfa doğruluğunu koruruz hem de FFmpeg `rgba` dönüşümünü kaldırırız:**
     1. Kaynak doğal boyutu `(src_w, src_h)` ve hedef tuval `(dst_w, dst_h)` üzerinden inner boyut `(inner_w, inner_h)` hesaplanır (`force_divisible_by=2` kuralıyla çift sayıya hizalanır).
     2. FFmpeg yalnızca `scale={inner_w}:{inner_h}` + `-pix_fmt nv12` çalıştırır (sıfır `pad`, sıfır `format=rgba`!).
     3. `content_uv_rect = [pad_x / dst_w, pad_y / dst_h, (pad_x + inner_w) / dst_w, (pad_y + inner_h) / dst_h]` GPU uniform'una yazılır.
     4. `yuv_to_linear.wgsl` içinde `in.uv` bu dikdörtgen dışındaysa doğrudan `vec4<f32>(0.0)` (premultiplied şeffaf siyah) döndürülür; içindeyse `(in.uv - rect.xy) / (rect.zw - rect.xy)` ile `t_y`/`t_uv` örneklenir.
     5. İki `SourceVideo` düğümü aynı varlığı `Stretch` ve `Contain` ile kullanıyorsa `StreamKey` yine `(path, decoded_w, decoded_h, fit)` ile ayrılır; böylece Stretch ile Cover/Contain asla aynı çözülmüş tamponu paylaşmaz (`provider.rs:88-90` sözleşmesi korunur).
2. **Alfa Kanallı Kaynaklar (`yuva420p` / PNG / Sentetik):**
   - **PNG / `SourceImage` / Sentetik (`ClipA`, `ClipB`, `MovingMask`):** Mevcut `SourcePixelData::Rgba8` / `RgbaF32` → `color_basic.wgsl` yolunda kalır; hiç dokunulmaz.
   - **Alfa Kanallı Video (`ffprobe.pix_fmt` içinde `yuva420p`, `yuva444p*`, `rgba`, `argb`, `bgra` vb.):** İki seçenek vardır:
     - *Hızlı & basit yol:* `pix_fmt` alfa içeriyorsa `StreamingVideoDecoder` otomatik `-pix_fmt rgba` moduna düşer (sıfır regresyon; animasyon/overlay MOV/WebM kaynakları nadirdir).
     - *Tam planar yol:* `yuva420p` için `t_alpha: R8Unorm` 3. bağlama eklenir (`nv12` iken 1×1 beyaz `R8Unorm` dummy doku bağlanır).
3. **Tek (Odd) Çözünürlükler (`width % 2 != 0 || height % 2 != 0`):**
   - `nv12` (4:2:0) alt-örneklemesi yalnızca çift genişlik ve yükseklikte tanımlıdır. `width % 2 != 0 || height % 2 != 0` olduğunda `StreamingVideoDecoder` güvenli biçimde `-pix_fmt rgba` yedek yolunu kullanır (hata üretmez).
4. **M1 Golden Hash Koruması:**
   - `verify_m1_golden_scene` ([engine.rs#L491](../../crates/frameshift-core/src/engine.rs#L491)) `SyntheticMediaGenerator` kullandığı için `Rgba8` → `color_basic` hattını kullanmaya devam eder; M1 golden kare hash'leri **%100 bit düzeyinde değişmeden kalır**.
   - Gerçek disk MP4'lerde FFmpeg `swscale` varsayılan matrisi ile GPU BT.709 matrisi arasında yuvarlama/matris farkından 1–3 LSB fark doğabilir (ör. BT.601 yerine gerçek BT.709 uygulanması görsel doğruluğu **artırır**). Barkod testleri (`media_tests.rs#L320`) `decode_video_frame` (`Rgba8Frame`) üzerinden çalıştığı için etkilenmez.

#### 1.4. `YuvToLinear` Uniform ve WGSL Shader Taslağı + Staging Havuzu

**Staging Texture Havuzu (`GpuCompositor::yuv_staging_pool`):**
`upload_source_frame` ([compositor.rs#L6123](../../crates/frameshift-render/src/compositor.rs#L6123)) her karede `create_texture` çağırmamalıdır. `(width, height)` anahtarlı küçük bir `SourceStagingPool` (`y_tex: R8Unorm`, `uv_tex: Rg8Unorm`, `rgba_tex: Rgba8Unorm`) ile `queue.write_texture` doğrudan mevcut GPU dokularının üzerine yazar (sıfır sürücü tahsisatı).

**Uniform Yapısı (80 byte, 16-byte hizalı `#[repr(C)] Pod`):**
```rust
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct YuvToLinearUniform {
    /// Sütun-major 3x3 YUV->sRGB matrisi (her sütun vec4<f32>, .xyz aktif).
    /// BT.709 / BT.601 / BT.2020 ve range ölçekleri CPU'da çarpılarak tek matriste birleştirilir.
    pub yuv_to_rgb_col0: [f32; 4], // Y sütunu (scale_y, scale_y, scale_y, 0)
    pub yuv_to_rgb_col1: [f32; 4], // Cb (U) sütunu
    pub yuv_to_rgb_col2: [f32; 4], // Cr (V) sütunu
    /// xy = (y_offset, uv_offset) — Limited: (16/255, 128/255); Full: (0.0, 128/255)!
    /// zw = chroma_uv_offset — MPEG-2 Left: (0.5 / src_w, 0.0); Center: (0.0, 0.0)
    pub offsets_chroma: [f32; 4],
    /// SourceFit::Contain GPU letterbox dikdörtgeni: [u_min, v_min, u_max, v_max]
    pub content_uv_rect: [f32; 4],
    /// x = transfer_mode (0u = Linear, 1u = sRGB/Rec.709 EOTF),
    /// y = use_alpha_tex (0u = opak 1.0, 1u = t_alpha.r),
    /// z = src_w, w = src_h
    pub flags_dims: [u32; 4],
}
```
> [!IMPORTANT]
> **8-bit YUV Chroma Merkez Sabiti (`128/255` vs `0.5`):** 8-bit `Rg8Unorm` içinde nötr kroma (`128`), GPU'da `128.0 / 255.0 = 0.5019608` olarak okunur. `0.5` çıkarmak tüm gri tonlarda sistematik `+0.00196` yeşil/macenta kaymasına yol açar; `128.0 / 255.0` kullanmak gri nötrlüğünü bit düzeyinde korur.

**WGSL Taslağı (`crates/frameshift-render/shaders/yuv_to_linear.wgsl`):**
```wgsl
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct YuvToLinearUniform {
    yuv_to_rgb_col0: vec4<f32>,
    yuv_to_rgb_col1: vec4<f32>,
    yuv_to_rgb_col2: vec4<f32>,
    offsets_chroma: vec4<f32>,  // xy = (y_bias, uv_bias), zw = (chroma_dx, chroma_dy)
    content_uv_rect: vec4<f32>, // (u_min, v_min, u_max, v_max)
    flags_dims: vec4<u32>,      // x = transfer_mode, y = use_alpha, zw = src_dims
}

@group(0) @binding(0) var t_y: texture_2d<f32>;      // R8Unorm
@group(0) @binding(1) var t_uv: texture_2d<f32>;     // Rg8Unorm
@group(0) @binding(2) var t_alpha: texture_2d<f32>;  // R8Unorm (1x1 white dummy veya alpha)
@group(0) @binding(3) var s_linear: sampler;         // ClampToEdge + Linear
@group(0) @binding(4) var<uniform> params: YuvToLinearUniform;

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VertexOutput {
    let x = f32((vid << 1u) & 2u);
    let y = f32(vid & 2u);
    var out: VertexOutput;
    out.position = vec4<f32>(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, 1.0 - y);
    return out;
}

fn srgb_to_linear_channel(c: f32) -> f32 {
    let x = max(c, 0.0);
    return select(pow((x + 0.055) / 1.055, 2.4), x / 12.92, x <= 0.04045);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let rect = params.content_uv_rect;
    if (in.uv.x < rect.x || in.uv.x > rect.z || in.uv.y < rect.y || in.uv.y > rect.w) {
        return vec4<f32>(0.0);
    }
    let span = max(rect.zw - rect.xy, vec2<f32>(1e-5));
    let local_uv = (in.uv - rect.xy) / span;

    let y_sample = textureSampleLevel(t_y, s_linear, local_uv, 0.0).r;
    // MPEG-2 / H.264 Left chroma siting: texel i center at (2i+1)/W, luma at (2i+0.5)/W -> +0.5/W
    let uv_coord = local_uv + params.offsets_chroma.zw;
    let cbcr = textureSampleLevel(t_uv, s_linear, uv_coord, 0.0).rg;

    let y_val = y_sample - params.offsets_chroma.x;
    let u_val = cbcr.x - params.offsets_chroma.y;
    let v_val = cbcr.y - params.offsets_chroma.y;

    let m = mat3x3<f32>(
        params.yuv_to_rgb_col0.xyz,
        params.yuv_to_rgb_col1.xyz,
        params.yuv_to_rgb_col2.xyz,
    );
    var rgb = clamp(m * vec3<f32>(y_val, u_val, v_val), vec3<f32>(0.0), vec3<f32>(1.0));

    if (params.flags_dims.x == 1u) {
        rgb = vec3<f32>(
            srgb_to_linear_channel(rgb.r),
            srgb_to_linear_channel(rgb.g),
            srgb_to_linear_channel(rgb.b),
        );
    }

    let alpha = select(
        1.0,
        clamp(textureSampleLevel(t_alpha, s_linear, local_uv, 0.0).r, 0.0, 1.0),
        params.flags_dims.y == 1u,
    );
    return vec4<f32>(rgb * alpha, alpha);
}
```
- **BT.709 Limited Range Katsayı Hesabı (Rust tarafında `yuv_to_linear_uniform` üretimi):**
  - $s_Y = \frac{255}{219} = 1.16438356$, $s_C = \frac{255}{224} = 1.13839286$.
  - `col0 = [s_Y, s_Y, s_Y, 0.0]`
  - `col1 = [0.0, -0.18732427 * s_C, 1.8556 * s_C, 0.0]` (`[0.0, -0.21324861, 2.11240179, 0.0]`)
  - `col2 = [1.5748 * s_C, -0.46812427 * s_C, 0.0, 0.0]` (`[1.79274107, -0.53290933, 0.0, 0.0]`)
  - `offsets_chroma = [16.0 / 255.0, 128.0 / 255.0, 0.5 / src_w as f32, 0.0]` (Left siting için).
  - *EOTF seçimi notu:* FrameShift'in tüm shader zinciri (`color_basic.wgsl`, `fx_anime4k_bridge.wgsl`, `motion_blur_accumulate.wgsl`) IEC 61966-2-1 parçalı sRGB EOTF/OETF kullanır. Bu nedenle `yuv_to_linear.wgsl` içinde de `srgb_to_linear_channel` kullanılması uçtan uca `Mp4FrameEncoder` döngüsünde renk tutarlılığını garanti eder.

---

### Bölüm 2: Düğüm Seviyesi İçerik-Hash'li Smart Cache (DaVinci Resolve Fusion Dersi)

#### 2.1. `revision_id`-Bağımsız Merkle DAG İçerik Anahtarı (`NodeCacheKey`)

`Project.revision_id` ([project.rs#L730](../../crates/frameshift-schema/src/project.rs#L730)) herhangi bir düğümde yapılan küçük bir değişiklikte bile artar; dolayısıyla önbellek anahtarına **kesinlikle dahil edilmemelidir**. Bunun yerine topolojik sıradaki her düğüm için aşağıdan yukarıya (bottom-up) **128-bit `NodeContentHash([u8; 16])`** (mevcut `Sha256` özetinin ilk 16 baytı, 0 yeni crate) hesaplanır:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeContentHash(pub [u8; 16]);
```

**Anahtar Bileşenleri ve Zaman Bağımlılığı Sınıflandırması (Kritik Mimari Kural):**

Tüm düğümlere körlemesine `sub_tick` eklemek, `24fps -> 60fps` tekrarlanan kaynak karelerde ve `MotionBlurShutter` alt-örneklerinde önbellek isabetini öldürür. Bu yüzden düğümler 3 sınıfa ayrılır:

1. **Sınıf A — Yaprak Kaynak Düğümleri (`SourceVideo`, `SourceImage`, `Solid`):**
   - `SourceVideo`: `hash(asset_file_identity, resolved_source_frame_index_or_quantized_tick, fit, color_space, enabled, width, height)`
     - *Kazanç:* `24 fps` klip `60 fps` timeline'da oynarken ardışık 2–3 çıktı karesi aynı `matched.frame_index`'e düşer → aynı `NodeContentHash` → downstream `Anime4K` / `DeepGlow` **otomatik hit alır**!
     - `clip_id` dışındaki boş aralıklar (`resolve_source_tick == None`) sabit `EMPTY_TRANSPARENT_HASH(w, h)` üretir.
   - `SourceImage`: `hash(asset_file_identity, fit, color_space, evaluated_opacity, enabled, width, height)` (`tick` bağımsız!).
   - `Solid`: `hash(evaluated_color_rgba, color_space, enabled, width, height)`.
2. **Sınıf B — Saf Üst-Grafik Bağımlı (Time-Invariant) İşlem Düğümleri:**
   *(Bu düğümler `evaluate_single_node` içinde doğrudan `sub_tick` okumaz; animasyonları önceden `animator.evaluate_node` ile somut `f32` değerlere indirgenmiştir!)*
   - `Transform2D`, `Blend`, `MaskMatte`, `DepthMatte`, `ColorBasic`, `Lut3D` (+ `LutSourceCacheKey` dosya damgası), `DeepGlow`, `Anime4K`, `AnimeSharpen`, `FilmOptics` (`grain_seed` parametresini kullanır), `MotionTileShake` (`event_delta_sec` parametresini kullanır), `RadialShockwave`, `HalftoneGlow`, `CornerPin`, `GridWarp`, `BackdropScrim`, `BlurMoCurves`, `ShaderTransition`.
   - **Anahtar:** `hash(node_kind_discriminant, evaluated_params_canonical_bytes, upstream_content_hashes_in_port_order, enabled, width, height)`.
   - `resolved_tick` bu sınıfta `None` tutulur çünkü çıktı tamamen `upstream_content_hash` + `evaluated_params` tarafından belirlenir.
3. **Sınıf C — Doğrudan Zaman/Çocuk Bağımlı (Time-Dependent) Düğümler:**
   - `VectorText` (`gpu_slug` / `text_animator.is_some()` iken) & `VectorPathHud`: `hash(evaluated_params, font_identity, resolved_tick = sub_tick, w, h)`. (Statik `VectorText` hâlihazırda `text_animator == None` ise `sub_tick` kullanmaz; o durumda `resolved_tick = None`.)
   - `Text3D`, `Shape3D`, `ParticleField3D`, `RibbonTrail3D`: doğrudan `time_sec` okudukları için `resolved_tick = Some(sub_tick)` + `font_identity` + `evaluated_params`.
   - `AnimeImpactFrame` ([compositor.rs#L3031](../../crates/frameshift-render/src/compositor.rs#L3031)): `seed = sub_tick` kullandığı için `resolved_tick = Some(sub_tick)`.
   - `AdjustmentLayer` ([compositor.rs#L2669](../../crates/frameshift-render/src/compositor.rs#L2669)): `is_active = (sub_tick >= start_tick && sub_tick < end_tick)` boolean'ı + `upstream_hash` + `opacity` + `blend_mode`.
   - `CustomWgsl` ([compositor.rs#L3475](../../crates/frameshift-render/src/compositor.rs#L3475)): `CustomWgslUniform` içine `time_sec` ve `sub_tick` gömdüğü için `hash(wgsl_source_sha256, expected_inputs, params, upstream_hashes, resolved_tick = sub_tick, w, h)` (ileride WGSL içinde `time_sec`/`tick` referansı taranarak statik shader'larda `resolved_tick` düşürülebilir).
   - `OpticalFlowBlur` ([compositor.rs#L2233](../../crates/frameshift-render/src/compositor.rs#L2233)): `hash(upstream_hash(tick), upstream_hash(prev_tick), invert_flow_sign, evaluated_params, effective_samples(sample_ceiling), w, h)`.
   - `MotionBlurShutter` ([compositor.rs#L2140](../../crates/frameshift-render/src/compositor.rs#L2140)): `hash(vec![upstream_hash(t_i) for t_i in sample_ticks], shutter_angle_deg, effective_sub_samples, phase, w, h)`.
   - `Scene3D` ([compositor.rs#L4387](../../crates/frameshift-render/src/compositor.rs#L4387)): `hash(camera, background_rgba, depth_plate_cfg, depth_plate_upstream_hash, supports_msaa_4x, child_evaluated_kinds_and_world_matrices, child_2d_tex_upstream_hashes, resolved_tick_if_any_animated_3d_child, w, h)`.
   - `Precomp` ([compositor.rs#L1921](../../crates/frameshift-render/src/compositor.rs#L1921)): `local_tick = remap_precomp_tick(sub_tick, time_offset_ticks, time_scale)` üzerinden `sub_graph` çıkış düğümünün özyinelemeli `NodeContentHash`'i + `precomp_inputs` içerik hash'leri + `opacity`.

**Geçersizleştirme Garantisi:** Yukarıdaki Merkle DAG formülü gereği:
- Üstteki bir `VectorText` veya `ColorBasic` değiştiğinde altındaki `Anime4K` / `Scene3D` / `OpticalFlowBlur` anahtarları **değişmez** → %100 hit.
- Alttaki klibin trim'i, hız eğrisi veya `SafeExpr` / `EventBoundAnimation` değeri değiştiğinde leaf `NodeContentHash` değişir → tüm bağımlı downstream anahtarları **otomatik geçersizleşir** (sıfır manuel invalidation hatası).
- Harici medya/LUT/font dosyası değiştiğinde `FileContentIdentity` / `DiskMediaStamp` değişir → yaprak hash değişir.

#### 2.2. 3 Katmanlı Depolama Mimarisi ve GPU Stall Koruması

> [!WARNING]
> **GPU Readback Stall Tuzağı:** `readback_rgba8` ([compositor.rs#L6640-L6710](../../crates/frameshift-render/src/compositor.rs#L6640-L6710)) `map_async` + `device.poll(Wait)` ile GPU komut kuyruğunu tamamen durdurur. Bir düğüm çıktısını DAG değerlendirmesinin **ortasında** CPU'ya okumak, GPU paralelliğini kırarak önbellekten kazanılan süreden fazlasını kaybettirir.

Bu nedenle önbellek katmanları aşağıdaki sıkı kurala göre ayrılır:

1. **Katman 1 — Hot GPU VRAM (`GpuNodeCache`, 0 Stall):**
   - `HashMap<NodeContentHash, CachedGpuNodeTexture>` + `BTreeMap<u64, NodeContentHash>` LRU (`TextureFormat::Rgba16Float`, `TEXTURE_BINDING | COPY_SRC | COPY_DST`).
   - **Hit:** `evaluate_subgraph_inner` düğümü atlar; doğrudan `copy_texture_2d` (GPU-içi DMA) ile `out` dokusuna kopyalar (veya `Arc` salt-okunur view ile **sıfır kopya** paylaşır).
   - **Miss (Yazma):** Aynı `CommandEncoder` içinde `copy_texture_2d` ile önbellek dokusuna yazılır. **CPU'ya hiç dokunulmaz.**
   - **Hedef düğümler:**
     - `OpticalFlowBlur` giriş kolu (`curr_tex` otomatik 1 kare sıcak tutulur → ardışık karede `prev_tex` %100 VRAM hit!).
     - `MotionBlurShutter` alt-örnekleri içinde tekrarlanan dallar.
     - `24fps -> 60fps` kopya karelerde `SourceVideo` / `Anime4K` / `DeepGlow` / `Scene3D`.
2. **Katman 2 — Warm CPU RAM (`CpuNodeCache`, Ertelenmiş Kare-Sonu Readback):**
   - Yalnızca **yüksek maliyetli** düğümler (`Anime4K`, `OpticalFlowBlur`, `Scene3D`, `DeepGlow`, `Precomp`, `CustomWgsl` veya `estimated_gpu_passes >= 3`) seçilir (`Solid`, `Blend`, `ColorBasic`, `Transform2D` asla CPU/Diske yazılmaz).
   - **Sıfır Mid-Frame Stall:** GPU readback (`Rgba16Float`, `8 bytes/px`) yalnızca kare sonundaki tek `queue.submit` + `poll(Wait)` döngüsüne **eşzamanlı** eklenen staging buffer'lar üzerinden alınır!
   - **0-Crate Sıkıştırma Seçeneği:** `Rgba16Float` determinizmini bozmadan (lossless), şeffaf/düz bölgeleri yoğun `Scene3D` / `VectorText` / `Anime4K` kareleri için ~30 satırlık saf Rust `u64` (8-byte piksel) **RLE-0 / PackBits** sıkıştırıcısı CPU RAM kapasitesini 1.5–3× artırabilir (veya ham `Vec<u8>` ile sıfır CPU maliyeti).
   - **Hit (VRAM Miss, CPU Hit):** `write_texture` ile doğrudan `Rgba16Float` GPU dokusuna yüklenir (WGSL pass'lerin tamamı atlanır).
3. **Katman 3 — Cold Disk LRU (`DiskNodeCache`, Opsiyonel):**
   - Konum: `.frameshift_cache/nodes/<hash[0..2]>/<hash_hex>.fsc`.
   - Dosya başlığı (64 byte): `magic ("FSCACHE1")`, `format (Rgba16Float=1)`, `width`, `height`, `uncompressed_len`, `payload_len`, `shader_identity_sha256[0..8]`, `adapter_fingerprint (u64)`, `payload_fnv1a_64`.
   - **Güvenlik:** Farklı FrameShift sürümünden (`shader_identity_sha256`) veya farklı GPU sürücüsünden/adaptörden (`adapter_info`) kalan önbellek otomatik yok sayılır; atomik yazma (`.tmp` → `rename`) kullanılır; `max_disk_bytes` (örn. `2–4 GiB`) LRU index ile sınırlandırılır.

#### 2.3. Asta v3 Bellek Bütçesi (`0.27–0.80 GB` Hedefi) Matematiksel Dağılımı

Tek bir `Rgba16Float` kare `1280×720`'de **`7.37 MB`**, `1920×1080`'de **`16.59 MB`** yer kaplar. Bölüm 1 (`Nv12`) ile kazanılan RAM tasarrufu doğrudan Smart Cache bütçesine aktarılır:

| Bellek Havuzu | Eski (`Rgba8` + Cache Yok) | Yeni Tasarım (`720p` / `1080p` Tavan) | Not |
| :--- | :---: | :---: | :--- |
| `LruFrameCache` (Kaynak Decode) | `128 kare` / `1.00 GiB` (720p ~`472 MB`) | `128 kare` / **`256 MiB`** (`Nv12`: 720p **177 MB**, 1080p ~82 kare) | **-216..740 MB tasarruf** |
| `CpuNodeCache` (Ağır Düğüm `Rgba16Float`) | `0` | **`128 MiB`** (720p ~18 kare, 1080p ~8 kare) | Lossless, kare sonu readback |
| `GpuNodeCache` (VRAM `Rgba16Float`) | `0` | **`192 MiB`** VRAM (720p ~26 kare, 1080p ~12 kare) | Ayrı GPU VRAM; UMA'da bile bütçe içi |
| `DecoderPool` (≤ 8 `ffmpeg` süreci) | ~`150–250 MB` | ~`150–250 MB` | Değişmez |
| **Toplam Çalışma Kümesi (Peak)** | **`0.40–0.80 GB`** (720p) / `>1.2 GB` (1080p) | **720p: ~`0.45–0.58 GB`** / **1080p: ~`0.55–0.78 GB`** | **Asta v3 `0.27–0.80 GB` hedefi korunur!** |

*(Not: `image_gpu_cache` ([compositor.rs#L1412](../../crates/frameshift-render/src/compositor.rs#L1412)) şu an sınırsız `HashMap` olduğundan, aynı `192 MiB` VRAM bütçesine veya LRU sınırına bağlanması uzun oturumlarda VRAM sızıntısını önler.)*

---

### Değiştirilecek Dosyalar (Uygulama Yol Haritası)

| Crate | Dosya | Değişiklik Özeti |
| :--- | :--- | :--- |
| `frameshift-media` | `src/ffmpeg.rs` | `FfprobeStream` / `MediaProbeInfo`'ya `color_space`, `color_range`, `chroma_location`, `color_transfer` ekleme; `Nv12` decode & `Contain` inner-size hesaplama |
| `frameshift-media` | `src/frame.rs` | `Nv12Frame`, `YuvColorInfo`, `CachedVideoFrame` (Rgba8 \| Nv12) |
| `frameshift-media` | `src/streaming.rs` | `StreamingVideoDecoder` & `DecoderPool` için `-pix_fmt nv12` modu (`rgba` alfa/odd-dim fallback ile) |
| `frameshift-media` | `src/cache.rs` | `LruFrameCache`'in `Nv12Frame` + `Rgba8Frame` toplam baytını (`DEFAULT_CACHE_MAX_BYTES = 256 MiB`) yönetmesi |
| `frameshift-render` | `src/provider.rs` | `SourcePixelData::Nv12` (ve opsiyonel `Yuva420p`) varyantları |
| `frameshift-render` | `shaders/yuv_to_linear.wgsl` *(yeni)* | `R8Unorm` + `Rg8Unorm` (+ `t_alpha`) → BT.709/BT.601 + chroma siting + `Contain` rect → `Linear Rec.709` `Rgba16Float` |
| `frameshift-render` | `src/uniforms.rs` & `src/pipelines.rs` | `YuvToLinearUniform` (80B) ve `yuv_to_linear` `ShaderProgram` derlemesi |
| `frameshift-render` | `src/pool.rs` | `SourceStagingPool` (`R8Unorm`/`Rg8Unorm`/`Rgba8Unorm`) + bütçeli `GpuNodeCache` |
| `frameshift-render` | `src/compositor.rs` | `upload_source_frame` YUV yolu, Merkle DAG `NodeContentHash` hesaplayıcısı, `evaluate_subgraph_inner` hit/miss + ertelenmiş readback |
| `frameshift-render` | `src/node_cache.rs` *(yeni)* | `NodeContentHash`, 3 katmanlı (`GpuNodeCache` / `CpuNodeCache` / opsiyonel `DiskNodeCache`) LRU yöneticisi |
| `frameshift-core` | `src/resolver.rs` & `src/engine.rs` | `MediaResolver` `Nv12` geçişi, `Engine` bütçe sabitleri (`256 MiB` + `128 MiB` + `192 MiB`) ve `SmartCachePolicy` |

---

### Kullanıcıya Sorulacak Kritik Kararlar

Uygulamaya geçmeden önce aşağıdaki **2 mimari yol ayrımı** için kullanıcı onayı önerilir:

1. **Karar 1 — GPU YUV Decode ve `SourceFit::Contain` Stratejisi:**
   - **Seçenek A *(Önerilen)*:** `Stretch` ve `Cover` ölçekleme/kırpmasını FFmpeg'de `nv12` olarak yapıp yalnızca `Contain` şeffaf letterbox dolgusunu `content_uv_rect` ile WGSL'e taşımak; alfa kanallı video (`yuva420p` vb.) ve tek (`% 2 != 0`) boyutlarda otomatik `rgba` yedeğine düşmek. *(En düşük risk, `Contain` dahil %62,5 bant genişliği kazancı.)*
   - **Seçenek B:** FFmpeg'de tüm klipleri doğal çözünürlüğünde (`nv12`) çözüp `Stretch`/`Cover`/`Contain` dönüşümlerinin tamamını WGSL'de yapmak. *(Aynı klibin farklı `fit` düğümleri tek `ffmpeg` stream'ini paylaşır; ancak 4K kaynakta 720p draft render için CPU/GPU'ya 4K yüklenir.)*
   - **Seçenek C:** Yalnızca `Stretch`/`Cover` için `nv12`, `Contain` için mevcut `rgba` yolunu korumak. *(En az kod değişikliği, ama `Contain` modunda hız/bellek kazancı kaybolur.)*

2. **Karar 2 — Düğüm Seviyesi Smart Cache Politikası ve Disk Önbelleği:**
   - **Seçenek A *(Önerilen)*:** Varsayılan olarak **Katman 1 GPU VRAM (`192 MiB`) + Katman 2 CPU RAM (`128 MiB`, kare sonu readback)** (`MemoryOnly`, ağır düğüm seçicili) aktif olsun; **Katman 3 Disk LRU (`MemoryAndDisk`)** ise yalnızca etkileşimli oturumlar veya açık `--cache-dir` bayrağıyla opt-in açılsın. *(Batch `render` SSD ömrünü ve I/O'yu korur, Asta v3 `0.27–0.80 GB` sınırını aşmaz.)*
   - **Seçenek B:** İlk fazda yalnızca **Katman 1 GPU VRAM (`GpuNodeCache`, 0 CPU readback)** implemente edilsin; CPU/Disk katmanları canlı masaüstü UI dilimine bırakılsın. *(En sade ilk adım, `OpticalFlowBlur` + `MotionBlurShutter` + `24->60fps` kazancının çoğunu sıfır readback karmaşıklığıyla verir.)*
   - **Seçenek C:** Disk LRU önbelleği (`.frameshift_cache/`) de varsayılan olarak aktif edilsin.

---

## Doğrulanmamış Noktalar

- Bu belgedeki mimari tasarımlar, `YuvToLinear` throughput'u ve Smart Cache isabet oranları henüz kodlanmamış ve gerçek medya (`asta_amv_project.json`, `fame_is_a_gun_v3`) üzerinde **ölçülmemiştir**.
- Gerçek H.264/HEVC kliplerde `ffprobe` tarafından bildirilen `chroma_location` etiketlerinin gerçek kodlama hizalamasıyla uyumu ve GPU BT.709 matrisinin `swscale` çıkışına göre `deltaE` / PSNR farkı uygulama sırasında test edilmelidir.
- UMA (paylaşımlı bellekli entegre GPU / laptop) ve ayrık VRAM GPU'larda `GpuNodeCache` (`192 MiB`) + `CpuNodeCache` (`128 MiB`) kombinasyonunun gerçek OS çalışma kümesi (`WorkingSetSize` / `PrivateBytes`) üzerindeki etkisi ölçüm beklemektedir.
