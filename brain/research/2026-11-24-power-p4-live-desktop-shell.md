---
title: "Pillar 4 Araştırma ve Mimari Tasarım: İnce Canlı Masaüstü Önizleme Kabuğu (`winit` + `egui` + `wgpu`)"
date: 2026-11-24
status: deferred
tags: [ui, engine, render-graph, agent]
---

# Pillar 4 Araştırma ve Mimari Tasarım: İnce Canlı Masaüstü Önizleme Kabuğu (`winit` + `egui` + `wgpu`)

- **Tarih:** 2026-11-24
- **Rol:** Pillar 4 Araştırmacısı (`res-live-ui`)
- **Durum:** `deferred` — Tasarım tamamlandı; Rakip Güç Yol Haritası uygulamasında (2026-11-24) kullanıcı kararıyla sonraki faza bırakıldı ([uygulama günlüğü](../log/2026-11-24-competitive-power-roadmap-implementation.md)). `AGENTS.md` gereği yeni crate (`crates/frameshift-gui`) ve `egui`/`winit` bağımlılıkları için kullanıcı onayı gerektiğinden henüz kodlanmadı.
- **Kapsam:** Mevcut `wgpu` motoru ve `frameshift-app` sözleşmesi üzerine **sıfır-kopya (zero-copy)** GPU viewport, gerçek zamanlı scrub / play-pause, `WaveformPyramid` çizimi, "Ajan değişiklik fişini gör -> Onayla / Geri Al" çubuğu ve canlı JSON/MCP yenileme akışı tasarımı.

---

## 1. Mevcut Kod Tabanı Envanteri ve Kısıtlar

### 1.1 `Cargo.toml` ve `Cargo.lock` Üzerinden `wgpu` Durumu
- **Workspace Kökü (`Cargo.toml`):** 9 crate'li `edition = "2024"`, `resolver = "2"` yapısı; paylaşılan workspace bağımlılıkları `serde = "1.0"`, `serde_json = "1.0"`, `thiserror = "2.0"`, `anyhow = "1.0"`.
- **Render Motoru (`crates/frameshift-render/Cargo.toml:17`):** `wgpu = "30.0.1"`, `bytemuck = "1.25.2"`, `glam = "0.29"`, `half = "2.7.1"`, `pollster = "1.0.1"`.
- **`Cargo.lock` Çözümlenen Sürüm:** `wgpu` ve tüm çekirdek alt paketleri (`wgpu-core`, `wgpu-core-deps-windows-linux-android`, `wgpu-hal`, `wgpu-naga-bridge`, `wgpu-types`) **`30.0.2`** sürümünde kilitlenmiştir.
- **Headless CLI Zinciri (`crates/frameshift-cli/Cargo.toml`):** `frameshift-app`, `frameshift-core`, `frameshift-mcp`, `frameshift-media`, `frameshift-schema`, `frameshift-time` bağımlıdır. `winit` veya `egui` içermez.

### 1.2 `frameshift-render` Girdi/Çıktı Sözleşmesi
- **`GpuContext` (`crates/frameshift-render/src/context.rs:4-55`):** Şu anda yalnızca `GpuContext::new_headless()` constructor'ına sahiptir ve `request_headless_adapter(&instance)` içinde `compatible_surface: None` geçer. Pencere yüzeyi (`wgpu::Surface`) ile aynı `Adapter`/`Device`/`Queue` paylaşımı için surface-uyumlu bir kurucu gereklidir.
- **`GpuCompositor::render_frame` (`crates/frameshift-render/src/compositor.rs:1510-1627`):**
  1. Alt DAG'ı `WORKING_TEXTURE_FORMAT` (`wgpu::TextureFormat::Rgba16Float`, Linear Rec.709 premultiplied alpha) içinde değerlendirir.
  2. İsteğe bağlı `post_aa` (FXAA) uygular.
  3. `self.pipelines.motion_blur_tonemap_srgb` (`crates/frameshift-render/shaders/motion_blur_accumulate.wgsl`) ile tonemapping + Linear->sRGB dönüşümünü **`wgpu::TextureFormat::Rgba8Unorm`** hedef dokusuna (`compositor.rs:1588`, `pipelines.rs:569`) yazar.
  4. Ardından `srgb_texture` yalnızca `RENDER_ATTACHMENT | COPY_SRC` ile yaratıldığı için `readback_rgba8` (`compositor.rs:6640-6690`) üzerinden staging buffer'a kopyalanıp `device.poll(wgpu::PollType::wait_indefinitely())` ile CPU'ya okunur. Canlı GUI bu CPU readback adımını tamamen ortadan kaldıracaktır.

### 1.3 `frameshift-app` Yapıları (ADR-0001: Sıfır Gizli UI Durumu)
- **`WorkspaceUiController` (`crates/frameshift-app/src/controller.rs:79-397`):**
  - Kalıcı proje gerçeğini `session: ProjectSession` (`crates/frameshift-schema/src/session.rs:121`) içinde tutar; kendi üzerinde yalnızca geçici görünüm durumu (`playhead_tick: Flicks`, `selected_node: Option<NodeId>`, `selected_clip: Option<String>`, `active_profile: RenderProfileKind`, `activity_log: Vec<UiActivityEntry>`) barındırır.
  - Başlıca metodlar: `scrub_to`, `select_node`, `select_clip`, `set_active_profile`, `human_apply_command`, `human_split_or_trim_clip`, `human_move_event_anchor`, `human_retarget_event`, `human_update_node`, `undo()`, `redo()`, `apply_agent_command()`, `optimistic_guard()`.
  - **Kritik Invariant (`session.rs:186-217`):** `ProjectSession` yalnızca `apply_command`'da değil, `undo()` ve `redo()` çağrılarında da `revision_id`'yi **monotonik olarak artırır** (`next_rev = self.project.revision_id + 1`). UI ve render önbelleği her zaman `revision_id` değişimini tazeleme sinyali olarak almalıdır.
- **`OptimisticLockGuard` (`controller.rs:12-36`):** Eşzamanlı insan-vs-ajan düzenlemelerinde `expected_revision_id != actual_revision_id` durumunda `AppError::ConcurrencyConflict` döndürür.
- **`TimelineProjection` ve `PreviewRequestGate` (`crates/frameshift-app/src/timeline.rs:13-340`):**
  - `TimelineProjection::from_project(&Project)`: deterministik sıralı `video_clips`, `audio_clips`, `events`.
  - `snap_tick`: Frame / Beat / Event mıknatıslama (eşit mesafede öncelik `Event > Beat > Frame`).
  - `source_output_speed_graph`: `SpeedCurve` grafiği örnekleme.
  - `PreviewRequestGate` (`timeline.rs:306-340`): `begin(rev, tick) -> PreviewTicket`, `invalidate()`, `accepts(ticket, active_rev, active_tick)` ile eski asenkron önizleme karelerini otomatik eler.
- **`AgentEditReceipt` ve `ReviewNoteStore` (`crates/frameshift-app/src/proposal.rs:109-584`):**
  - `apply_agent_command` tek `ProjectSession::apply_command` çağrısı yapar (tek undo adımı) ve `AgentEditReceipt` üretir: `receipt_id`, `intent`, `actor`, `action`, `before_revision_id`, `after_revision_id`, `affected_entity_ids`, `affected_ranges: Vec<TickRange>`, `moved_events: Vec<MovedEvent>`, `preserved_creative_decisions`, `preview: ReceiptPreview`.
  - `ReviewNoteStore`: revizyona çivili inceleme notları (`Current` / `Stale`).
- **`BackgroundJobSupervisor` (`crates/frameshift-app/src/jobs.rs:81-262`):** Pencere kapanışında aktif render/ajan işlerini `WindowClosePolicy` (`DetachAndContinueBackground`, `PauseGracefully`, `CancelAll`) ile yönetir (`on_window_close`, `reattach_window`).
- **`snapshot.rs` (`crates/frameshift-app/src/snapshot.rs:10-379`):** Mevcut 5 panelli karanlık tema yerleşimini (Top Bar, Media Pool & Decisions, Center Viewport, Node DAG & SafeExpr Inspector, Bottom Multi-track Timeline + `SnapshotAudioContext` waveform + Speed Curve) ve tüm renk token'larını (`BG_CANVAS`, `BG_PANEL`, `ACCENT_CYAN/GREEN/YELLOW/PINK/RED`) tanımlar.
- **`WaveformPyramid` (`crates/frameshift-media/src/waveform.rs:80-370`):** 4'lü çoklu çözünürlük piramidi; `sample_columns` / `sample_columns_with_buffer` ile görünür aralık `[start_frame, end_frame)` için $O(\text{width})$ sürede `WaveformColumn { min, max, rms, band_energy }` döndürür.

---

## 2. Doğrulanmış Crate Sürüm Matrisi (Birincil Kaynaklar)

Crates.io API ve `docs.rs` üzerinden doğrudan doğrulanan sürümlere göre, projedeki **`wgpu = "30.0.1"` (`Cargo.lock: 30.0.2`)** ile **sıfır sürüm çakışmasıyla** çalışan tam ekosistem seti şudur:

| Crate | Önerilen Sürüm | Kritik Alt Bağımlılıklar (`crates.io` doğrulaması) | Birincil Kaynak URL |
| :--- | :--- | :--- | :--- |
| `wgpu` | `30.0.1` *(mevcut, lock: `30.0.2`)* | — | https://crates.io/crates/wgpu/30.0.2 |
| `egui` | **`0.37.0`** | — | https://crates.io/crates/egui/0.37.0 |
| `egui-wgpu` | **`0.37.0`** | `wgpu ^30.0`, `egui ^0.37.0`, `winit ^0.30.13`, `bytemuck ^1.25`, `thiserror ^2.0` | https://crates.io/crates/egui-wgpu/0.37.0 |
| `egui-winit` | **`0.37.0`** | `egui ^0.37.0`, `winit ^0.30.13`, `serde ^1.0` | https://crates.io/crates/egui-winit/0.37.0 |
| `winit` | **`0.30.13`** *(kararlı; `0.31.0-beta.*` kullanılmaz)* | — | https://crates.io/crates/winit/0.30.13 |
| `notify-debouncer-mini` *(opsiyonel)* | **`0.8.0`** (`notify ^9.0.0`) | `notify ^9.0.0` (veya 0-crate `mtime`/hash poller) | https://crates.io/crates/notify-debouncer-mini/0.8.0 |
| `rodio` *(opsiyonel, ses)* | **`0.23.0`** | `cpal ^0.18.2` | https://crates.io/crates/rodio/0.23.0 |

**Uyumluluk Bulgusu:** `egui-wgpu 0.37.0`, `egui-winit 0.37.0` ve `winit 0.30.13` mevcut workspace'teki `wgpu 30.0.2`, `bytemuck 1.25.2`, `thiserror 2.0` ve `serde 1.0` sürümleriyle birebir örtüşür. İkinci bir `wgpu` veya `bytemuck` kopyası derlenmez.

---

## 3. Mimari Yerleşim: Ayrı Crate (`crates/frameshift-gui`) vs Feature-Gated (`frameshift-app/gui`)

| Kriter | Seçenek A: Ayrı Crate (`crates/frameshift-gui`) *(Önerilen)* | Seçenek B: Feature-Gated (`frameshift-app/gui`) |
| :--- | :--- | :--- |
| **Katman / Bağımlılık Yönü** | `frameshift-app` + `frameshift-core` + `frameshift-render` -> `frameshift-gui`. Mevcut DAG temiz kalır. | **Katman ihlali / Döngü riski:** `frameshift-core` zaten `frameshift-app`'e bağımlı (`crates/frameshift-core/Cargo.toml:9`). `frameshift-app`'in `frameshift-core` veya `frameshift-render` çekmesi mimari sınırı bozar. |
| **Headless CLI İzolasyonu** | `frameshift-cli` (`verify-all`, `render`, `mcp-stdio`) pencereleme/UI crate'lerine **hiç dokunmaz**; derleme süresi ve binary boyutu 0 etkilenir. | `--all-features` testleri veya feature unification tüm CLI hedeflerine `winit`/`egui`/`accesskit` taşıyabilir. |
| **İkili (Binary) Modeli** | Ayrı hafif `frameshift-gui` masaüstü uygulaması; istenirse `frameshift-cli gui <project.json>` yalnızca `frameshift-gui` sürecini spawn eder. | Tek CLI ikilisi içine GUI gömmek zorunda kalır veya ekstra `[[bin]]` ister. |
| **`frameshift-app` Rolü** | `frameshift-app` saf, headless test edilebilir kontrolcü (`WorkspaceUiController`, `TimelineProjection`, `AgentEditReceipt`, `snapshot.rs`) olarak kalır. | `frameshift-app` hem headless hem OS window loop sorumluluğu taşır. |

**Sonuç:** `crates/frameshift-gui` ayrı crate olarak açılmalıdır. `frameshift-app` yalnızca headless UI mantığı ve sözleşmesi olarak kalır.

---

## 4. Sıfır-Kopya (Zero-Copy) GPU Viewport & Windows DX12/Vulkan Surface Tasarımı

### 4.1 Tek `wgpu::Device` / `wgpu::Queue` Paylaşımı ve Windows Surface
Mevcut `GpuContext::new_headless()` (`crates/frameshift-render/src/context.rs:19`) yerine/yanına `frameshift-render`'a pencere yüzeyi destekli bir kurucu eklenir:

1. **Instance & Surface (`winit 0.30.13` `ApplicationHandler` içinde):**
   - `resumed(&mut self, event_loop: &ActiveEventLoop)` aşamasında `Window::default_attributes()` ile `Arc<Window>` oluşturulur (`docs.rs/winit/0.30.13`).
   - `wgpu::Instance::new(&wgpu::InstanceDescriptor { backends: wgpu::Backends::PRIMARY, .. })` Windows üzerinde otomatik olarak **DX12** (birincil) ve **Vulkan** (ikincil) arka uçlarını etkinleştirir.
   - `let surface = instance.create_surface(window.clone())?;`
2. **Ortak Adapter/Device Seçimi (`GpuContext::new_with_surface`):**
   - `request_adapter` çağrısına `compatible_surface: Some(&surface)` verilir. Böylece `GpuCompositor` ve `egui_wgpu::Renderer` **aynı fiziksel GPU adaptörü, `wgpu::Device` ve `wgpu::Queue`** üzerinde çalışır; cross-device kopya gerekmez.
   - `GpuCompositor::from_context(ctx: GpuContext)` ile mevcut tüm 15+ WGSL pipeline'ı (`ShaderPipelines::compile_all_with_msaa`) bu paylaşılan cihazda derlenir.
3. **Swapchain Formatı ve Double-Gamma Koruması:**
   - `egui_wgpu 0.37.0` `Renderer::new(device, output_color_format, RendererOptions::default())` dokümantasyonu (`docs.rs/egui-wgpu/0.37.0`), `output_color_format` için **gamma-space** `Rgba8Unorm` veya `Bgra8Unorm` formatlarını önerir.
   - Bu nedenle `surface.get_capabilities(&adapter).formats` içinden `!fmt.is_srgb()` (`Bgra8Unorm` veya `Rgba8Unorm`) seçilir; `PresentMode::AutoVsync` (`Fifo`) varsayılan yapılır (`Mailbox` destekleniyorsa düşük gecikme seçeneği sunulabilir).
4. **`wgpu 30.0.2` Surface Acquisition Kuralı:**
   - `wgpu 30.0.2`'de `SurfaceError` kaldırılmıştır; `surface.get_current_texture()` **`wgpu::CurrentSurfaceTexture`** döndürür (`docs.rs/wgpu/30.0.2`):
     - `Success(tex)` -> normal çizim + `tex.present()`.
     - `Suboptimal(tex)` -> çizim yap, sonra `surface.configure(&device, &config)` yenile.
     - `Outdated` -> hemen `surface.configure(&device, &config)` yenile ve bu kareyi atla.
     - `Timeout | Occluded` -> kareyi atla (minimize / kilit ekranında GPU harcamaz).
     - `Lost` -> surface'i yeniden oluştur.

### 4.2 Sıfır-Kopya Viewport Pipeline'ı (`register_native_texture`)
`egui_wgpu::Renderer::register_native_texture(&mut self, device: &Device, texture: &TextureView, texture_filter: FilterMode) -> TextureId` dokümantasyonuna (`docs.rs/egui-wgpu/0.37.0`) göre native texture **`wgpu::TextureFormat::Rgba8Unorm`** formatında olmalıdır.

FrameShift'in `motion_blur_tonemap_srgb` pass'i (`crates/frameshift-render/src/compositor.rs:1588-1613`) zaten Linear `Rgba16Float` -> tonemapped sRGB **`Rgba8Unorm`** çıktısı üretmektedir. Bu mükemmel uyum sayesinde ekstra renk dönüşümü veya CPU readback gerekmez:

```text
Project + output_tick (Flicks)
   │
   ▼  GpuCompositor::evaluate_subgraph(...)   [Rgba16Float, Linear Rec.709]
   │
   ▼  motion_blur_tonemap_srgb                [Tonemap + Linear->sRGB shader]
   │
   ▼  ViewportTarget (Kalıcı GPU Texture: Rgba8Unorm, RENDER_ATTACHMENT | TEXTURE_BINDING)
   │  (Sıfır CPU readback! readback_rgba8 çağrılmaz)
   │
   ├── İlk kurulum: tex_id = egui_renderer.register_native_texture(&device, &vp_view, FilterMode::Linear)
   ├── Boyut değişimi: egui_renderer.update_egui_texture_from_wgpu_texture(&device, &new_view, FilterMode::Linear, tex_id)
   │
   ▼  egui UI: ui.image((tex_id, fitted_viewport_size))
   │
   ▼  egui_renderer.render(&mut render_pass, &clipped_primitives, &screen_desc) -> SurfaceTexture (Bgra8Unorm/Rgba8Unorm)
```

- **Çözünürlük & Ölçekleme:** Viewport widget'ı panel boyutuna göre aspect-ratio koruyarak (`fit`) yerleşir; `Draft` profilinde proje draft çözünürlüğünde (ör. `960x540` veya `1280x720`) GPU'ya çizilir ve `FilterMode::Linear` ile panele ölçeklenir.
- **Kare Yeniden Kullanımı (Dirty Flag):** `last_rendered: Option<(u64 /* revision_id */, Flicks /* tick */, RenderProfileKind, u32 /* sub_samples_cap */)>` tutularak aynı revizyon ve tick'te UI fare hareketi olsa bile `GpuCompositor` tekrar çalıştırılmaz; `egui` yalnızca mevcut `tex_id`'yi ekrana çizer.

---

## 5. Etkileşimli Yüzeyler ve Veri Akışı (`frameshift-app` Köprüsü)

### 5.1 Gerçek Zamanlı Scrub + Space Play/Pause + Uyarlanabilir Kalite
- **Scrubbing & Snapping:**
  - Timeline veya cetvel üzerinde fare sürüklenirken imleç X koordinatı `Flicks` değerine çevrilir.
  - Snap aktifse `frameshift_app::snap_tick(tick, fps, &beat_ticks, &event_ticks, tolerance)` çağrılır; ardından `controller.scrub_to(snapped.tick)` güncellenir.
  - **Uyarlanabilir Kalite (Adaptive Scrub):** Aktif sürükleme veya oynatma sırasında `ShutterOverride { shutter_angle_deg: 180.0, sub_samples: 1, phase: None }` ile tek-örnek düşük gecikmeli önizleme verilir; sürükleme/oynatma durduğunda (`pointer_released` / `Pause`) son tick için yazarın tanımladığı tam `MotionBlurShutter` `sub_samples` değeriyle tek bir yüksek kaliteli "settle frame" çizilir.
  - **`PreviewRequestGate` Entegrasyonu:** Eğer arka plan veya aşamalı render kuyruğu kullanılıyorsa `PreviewRequestGate::begin(rev, tick)` ve `accepts(...)` bayat karelerin ekrana düşmesini engeller.
- **Space Play/Pause & Flicks Zaman Saati:**
  - `egui_ctx.wants_keyboard_input()` `false` iken `Space` tuşu `PlaybackState` (`Paused <-> Playing`) durumunu değiştirir (`J/K/L` ve `Left/Right` tek-kare adımlama da aynı kapıdan geçer).
  - Oynatırken `anchor_instant: Instant` ve `anchor_tick: Flicks` saklanır. Her `RedrawRequested` içinde duvar saati farkı doğrudan tam sayı Flicks'e çevrilir:
    `delta_flicks = (elapsed.as_nanos() * FLICKS_PER_SECOND as u128 / 1_000_000_000) as i64`
    ve `fps` karesine yuvarlanarak `controller.scrub_to(...)` çağrılır. `f64` saniye birikimi yapılmaz.
  - **Güç Tasarrufu:** Oynatma durdurulduğunda `ControlFlow::Wait` moduna geçilir; yalnızca girdi, dosya/MCP olayı veya arka plan render tamamlanmasında `window.request_redraw()` tetiklenir.

### 5.2 Doğrudan `WaveformPyramid` Çizimi (CPU Rasterizer Olmadan)
- `frameshift-app/src/snapshot.rs` PNG çıktısı için `Rgba8Frame` rasterizer kullanır; canlı GUI'de bunu tekrar PNG/RGBA8'e çizmeye gerek yoktur.
- Ses klibi yüklendiğinde arka planda bir kez `WaveformPyramid::build(&audio_buf, true)` (veya önbelleğe alınmış piramit) oluşturulur.
- Her `egui` karesinde yalnızca görünür timeline piksel genişliği `plot_w_px` kadar sütun çekilir:
  `let cols = pyramid.sample_columns(None, vis_start_frame, vis_end_frame, plot_w_px);`
- `egui::Painter` ile `A1 AUDIO` şeridine:
  - Dış tepe zarfı (`[min, max]`, yarı saydam `ACCENT_GREEN`),
  - İç `rms` çekirdeği (parlak nane yeşili) veya `SpectralRgb` modunda `band_energy` (`Low=Red, Mid=Green, High=Cyan`),
  - Üstüne `detected_beats` (ince dikey çizgi), `downbeat_ticks` (kalın cyan çizgi), `project.events` (sarı elmas + etiket) ve kırmızı `playhead` çizilir.
- Zoom/pan değiştiğinde $O(\text{plot\_w\_px})$ sürede anlık (60+ FPS) yeniden çizilir.

### 5.3 "Ajan Değişiklik Fişini Gör -> Onayla / Geri Al" Çubuğu
Mevcut `frameshift-app/src/proposal.rs` (`AgentEditReceipt`) ve `ProjectSession::undo()` (`frameshift-schema/src/session.rs:186`) bu çubuk için gereken tüm alanlara sahiptir:

1. **Veri Modeli (`GuiReceiptBannerState`):**
   - Son gelen `AgentEditReceipt` (ve varsa öncesi/sonrası proje özeti) `pending_reviews: VecDeque<AgentEditReceipt>` olarak kuyruklanır.
2. **Üst Bildirim Çubuğu (Toast / Banner):**
   - Örnek içerik: `[AGENT: director] "align impact to music drop" | Rev #4 -> #5 | Etkilenen: event:drop_impact, clip:clip_a`
   - **Buton 1 — `Fişi Gör`:** Yan/modal inceleme penceresi açar:
     - `moved_events` (`from_tick -> to_tick`), `affected_ranges` (tıklayınca playhead o aralığın başına zıplar), `affected_entity_ids`, `preserved_creative_decisions` (korunan kullanıcı kararlarının rozet listesi).
   - **Buton 2 — `Onayla`:** Fişi onaylı sayar (`ReviewNoteStore` veya activity log'a onay kaydı yazar) ve çubuğu kapatır.
   - **Buton 3 — `Geri Al`:** Doğrudan `controller.undo()` çağırır (`ProjectSession::undo` tek adımda ajan değişikliğini geri alır, `revision_id` monotonik olarak `#6` olur) ve eğer proje dosya tabanlı izleniyorsa güncellenmiş projeyi atomik olarak diske kaydeder (veya bir sonraki bölümdeki çakışma kuralına uyar).

### 5.4 Canlı Yenileme Akışı: JSON Dosya İzleyici & MCP Köprüsü
**Mevcut Durum Boşluğu:** `frameshift-cli` içindeki `McpStdio` (`crates/frameshift-cli/src/main.rs:651-655`) şu anda `McpServer::new(Project::golden_scene_preset())` ile bellekte çalışır ve diske otomatik yazmaz. Bu yüzden canlı senkronizasyon için iki katmanlı akış tasarlanır:

1. **Katman 1 — Atomik JSON Dosya İzleyici (İlk Hedef):**
   - `frameshift-gui <project.json>` proje dosyasını (ve varsa yanındaki `<project>.receipt.json` / `.frameshift/receipts/` dizinini) izler (`notify-debouncer-mini` ~150 ms debounce veya hafif poller).
   - **Yarım-Yazım Koruması:** Ajan/CLI JSON'ı diske yazarken `write temp -> rename` yapmasa bile, parse hatasında 50–100 ms içinde 2 kez yeniden dener; başarısızsa UI durum çubuğunda hata rozeti gösterir, son geçerli `ProjectSession` bozulmaz.
   - **İnsan-vs-Ajan Çakışma Koruması:**
     - GUI'nin kendi diske yazdığı son SHA/FNV-1a hash kaydedilir (`last_written_hash`). Kendi yazımından doğan dosya olayları yoksayılır.
     - Diskte harici bir değişiklik algılandığında, eğer GUI'de kullanıcının henüz kaydetmediği yerel değişiklik (`dirty_local_edits == true`) yoksa:
       - Önceki proje ile yeni proje arasındaki farktan (`changed_entity_ids`, `moved_events`, `preserved_creative_decisions` yardımcıları `proposal.rs` içinden pub yapılarak veya yan `.receipt.json` okunarak) bir `AgentEditReceipt` üretilir.
       - Yeni proje `ProjectSession` içine `ProjectCommand::ReplaceProject` (veya session reload) ile alınır; böylece kullanıcı **`Geri Al (Undo)`** dediğinde bir önceki revizyona tek tıkla dönebilir!
     - Eğer `dirty_local_edits == true` ise otomatik üzerine yazılmaz; üst çubukta **`Çakışma: Diskteki Rev #N vs Yerel Rev #M [Diski Yükle] [Yereli Koru]`** uyarısı çıkar.
2. **Katman 2 — Canlı MCP / IPC Köprüsü (Sonraki Adım):**
   - `frameshift-cli mcp-stdio --project <path> [--autosave]` veya yerel Named Pipe / localhost JSON-RPC köprüsü ile MCP sunucusu her `mutate_project` sonrası doğrudan atomik `.json` + `.receipt.json` yazar (ya da GUI içindeki sokete `AgentEditReceipt` gönderir). Böylece ajan düzenlemeleri milisaniye içinde GUI'ye akar.

---

## 6. Değiştirilecek / Eklenecek Dosya Planı (Onay Sonrası)

1. `Cargo.toml`: Workspace üyelerine `"crates/frameshift-gui"` eklenmesi ve `egui = "0.37.0"`, `egui-wgpu = "0.37.0"`, `egui-winit = "0.37.0"`, `winit = "0.30.13"` (ve seçilirse `notify-debouncer-mini = "0.8.0"`) tanımlanması.
2. `crates/frameshift-render/src/context.rs`: `GpuContext::new_with_surface(instance, &surface)` ve `request_compatible_adapter` eklenmesi.
3. `crates/frameshift-render/src/compositor.rs`: `GpuCompositor::from_context(ctx)`, ve `readback_rgba8` olmadan doğrudan `Rgba8Unorm` `wgpu::TextureView` hedefine yazan `render_frame_to_view` / `render_project_frame_to_view` metodlarının eklenmesi.
4. `crates/frameshift-core/src/engine.rs`: `Engine::new_with_compositor(compositor)` ve `Engine::render_frame_to_view(...)` (mevcut `LruFrameCache`, `MediaResolver` ve `AudioRmsEnvelope` önbelleğini paylaşır).
5. `crates/frameshift-app/src/proposal.rs`: Disk tabanlı reload'larda sentetik diff receipt oluşturabilmek için `build_project_diff_receipt(...)` yardımcısının dışa açılması.
6. `crates/frameshift-gui/` *(yeni crate)*: `winit` event loop, `egui-wgpu` renderer, sıfır-kopya viewport bileşeni, `WaveformPyramid` + `SpeedCurve` timeline bileşeni, Node DAG & SafeExpr inceleme paneli, Ajan Fişi banner/drawer'ı ve JSON dosya izleyici.

---

## 7. Kullanıcı Onayı Gereken Kararlar (`AGENTS.md` Ask-First Kapısı)

`AGENTS.md` gereği yeni crate scaffold'u ve dış bağımlılık eklemeden önce aşağıdaki **4 karar** kullanıcı onayına sunulmalıdır:

1. **Crate Yerleşimi ve Çekirdek GUI Bağımlılıkları Onayı:**
   - **Seçenek A (Önerilen):** `crates/frameshift-gui` yeni crate'i + `egui = "0.37.0"`, `egui-wgpu = "0.37.0"`, `egui-winit = "0.37.0"`, `winit = "0.30.13"` (`frameshift-cli` sıfır yeni bağımlılıkla headless kalır).
   - **Seçenek B:** `frameshift-cli` altına `gui` feature bayrağıyla bağlamak (UI crate'lerini CLI paketine yakınlaştırır, önerilmez).

2. **Canlı Dosya İzleyici Bağımlılığı:**
   - **Seçenek A (Önerilen):** `notify-debouncer-mini = "0.8.0"` (`notify 9.0.0`, anlık OS `ReadDirectoryChangesW` olayı, 150 ms debounce).
   - **Seçenek B:** Sıfır yeni crate — arka plan thread'inde 250 ms aralıkla `std::fs::metadata().modified()` + boyut/hash yoklaması.

3. **Ajan Değişikliği Onay Akışı ("Onayla / Geri Al"):**
   - **Seçenek A (Önerilen — Commit-then-Review):** Gelen ajan/JSON değişikliği mevcut `apply_agent_command` sözleşmesine sadık kalarak tek undo adımı olarak uygulanır ve canlı önizlenir; kullanıcı banner'dan **"Fişi Gör"**, **"Onayla"** (bildirimi kapat/not düş) veya **"Geri Al"** (`controller.undo()` + diske yaz) seçer.
   - **Seçenek B (Stage-before-Commit):** Değişiklik `ProjectSession`'a uygulanmadan geçici kopyada önizlenir; **"Onayla"** basılana kadar proje revizyonu artmaz (harici `project.json` düzenlemeleriyle senkronu zorlaştırır).

4. **İlk GUI Fazında `Space` Play/Pause Ses Çıkışı:**
   - **Seçenek A (Önerilen — Önce Görüntü + Görsel Waveform):** İlk adımda `0` ekstra ses bağımlılığıyla kesin `Flicks` duvar-saati oynatma + canlı `WaveformPyramid` + scrubbing; `rodio 0.23.0` canlı ses çıkışını `audio-playback` feature'ı ile ikinci adımda eklemek.
   - **Seçenek B:** İlk adımda `rodio = "0.23.0"` (`cpal`) ekleyip canlı PCM ses çıkışını da aynı PR'da açmak.
