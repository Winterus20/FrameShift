---
title: "Gerçek Waveform PNG Çıktısı, Peak Pyramid ve Timeline Entegrasyonu Mimarisi"
date: 2026-11-24
status: active
tags: ["audio", "ui", "inspection", "cli", "mcp"]
---

# Gerçek Waveform PNG Çıktısı, Peak Pyramid ve Timeline Entegrasyonu Mimarisi

## Soru / Amaç

FrameShift üzerinde gerçek decode edilmiş PCM ses tamponlarından (`AudioBuffer`) **0 yeni Cargo crate** ile yüksek kaliteli, deterministik RGBA waveform PNG görüntüleri üretmek ve mevcut prosedürel timeline çubuklarını (`snapshot.rs`) gerçek dalga formuyla değiştirmek:

1. **Saf Rust RGBA Rasterizer:** Mevcut `png = "0.18"` bağımlılığını (`Rgba8Frame`) kullanarak anti-aliased dikey sütun rasterizasyonu, min/max peak + RMS çift katmanlı zarf, isteğe bağlı 3-bant spektral RGB renklendirme, `--range` zoom ve beat/downbeat/cue işaretçi katmanlarını tasarlamak.
2. **Büyük Ses Dosyalarında Ölçeklenebilirlik:** Uzun müzik/ses dosyalarında tekrarlı zoom ve timeline çizimleri için çok çözünürlüklü peak pyramid / bucket downsampling yapısı tanımlamak.
3. **Çok Katmanlı Entegrasyon:** Paylaşımlı `WaveformRenderer` API'sini crate bağımlılık yönünü bozmadan yerleştirmek; `frameshift-cli waveform`, MCP `render_waveform` ve `crates/frameshift-app/src/snapshot.rs` entegrasyonunu mevcut deterministik testleri (`ui-snapshot`, `app_tests`) kırmadan tasarlamak.

---

## İncelenen Birincil Kaynaklar

- **Harici Birincil Kaynaklar:**
  - `png` crate (`0.18.1`, `image-rs/image-png`): https://docs.rs/png/0.18.1/png/ — `Encoder::new`, `ColorType::Rgba`, `BitDepth::Eight` API doğrulaması (erişim: 2026-11-24).
  - BBC R&D `audiowaveform` (GPLv3, referans algoritma): https://github.com/bbc/audiowaveform ve https://github.com/bbc/waveform-data.js — min/max bucket downsampling, zoom seviyesi ve piksel-sütun ölçekleme mimarisi (erişim: 2026-11-24).
  - 3-Bant RGB Spektral Waveform Renklendirme Konvansiyonu (Serato / Rekordbox RGB & 3-Band): https://www.digitaldjtips.com/ ve https://deejayplaza.com/ — Low/Mid/High enerji haritalaması (erişim: 2026-11-24).
- **Yerel Kod ve Sözleşme Kaynakları (doğrudan okunan):**
  - `Cargo.toml` (`L1-L33`) ve `Cargo.lock` (`L837-L840`) — workspace crate listesi ve `png = "0.18.1"` kilidi.
  - `crates/frameshift-media/Cargo.toml` (`L1-L13`), `crates/frameshift-app/Cargo.toml` (`L1-L13`), `crates/frameshift-core/Cargo.toml` (`L1-L17`), `crates/frameshift-mcp/Cargo.toml` (`L1-L16`), `crates/frameshift-cli/Cargo.toml` (`L1-L17`).
  - `crates/frameshift-media/src/frame.rs` (`L18-L207`) — `Rgba8Frame`, `encode_png_bytes`, `save_png`.
  - `crates/frameshift-media/src/audio.rs` (`L27-L51`, `L554-L854`, `L949-L1002`) — `AudioBuffer`, `AudioPeakInfo`, `AudioAnalysisReport`, `compute_rms_envelope`, `analyze_beats_and_energy`, `detect_peak_in_window`, `apply_biquad_filter`.
  - `crates/frameshift-media/src/ffmpeg.rs` (`L1107-L1158`) — `FfmpegBridge::decode_audio`.
  - `crates/frameshift-media/src/synthetic.rs` (`L220-L279`) — `SyntheticMediaGenerator::generate_beat_audio`.
  - `crates/frameshift-app/src/snapshot.rs` (`L47-L272`, `L703-L972`, `L1457-L1822`) — `Canvas`, `render_desktop_ui_snapshot`, prosedürel A1 ses çubukları (`L825-L859`), `glyph_5x7` (`L1457-L1628`) ve snapshot testleri (`L1635-L1821`).
  - `crates/frameshift-app/tests/app_tests.rs` (`L237-L306`) — `desktop_ui_snapshot_is_deterministic_1920x1080_and_embeds_viewport_and_tracks`.
  - `crates/frameshift-core/src/resolver.rs` (`L287-L364`, `L555-L709`, `L761-L916`) — `MediaResolver::audio_cache`, `load_audio_asset`, `mix_project_audio`, `resolve_audio_clip_bounds`, `match_synthetic_asset`.
  - `crates/frameshift-core/src/range.rs` (`L12-L100`) — `RenderRange`.
  - `crates/frameshift-cli/src/main.rs` (`L381-L577`, `L731-L758`, `L1153-L1164`, `L1408-L1605`) — `ui-snapshot`, `peaks`, `autocut`, `verify-all` (M4 kapısı).
  - `crates/frameshift-mcp/src/server.rs` (`L222-L486`, `L960-L1135`) — `list_tools`, `analyze_audio`, `tools/call` yanıt zarfı.
  - `brain/specs/inspection-contract.md` (`L22-L107`) — `InspectionIdentity`, `EpistemicScopeReport` ve MCP görsel payload zorunluluğu.

---

## Doğrudan Gözlemler (Kanıtlı)

1. **`png` Crate'i Zaten Workspace'te Mevcut (0 Yeni Crate Yeterli):**
   - `Cargo.lock` (`L837-L840`) üzerinde `png = "0.18.1"` kilitlidir; `crates/frameshift-media/Cargo.toml` (`L10`) doğrudan `png = "0.18"` kullanır.
   - `Rgba8Frame::encode_png_bytes` ve `Rgba8Frame::save_png` (`crates/frameshift-media/src/frame.rs:151-195`) `png::Encoder::new(..., self.width, self.height)` ile `ColorType::Rgba` + `BitDepth::Eight` PNG üretimini zaten yapar.
   - `frameshift-media` yalnızca `frameshift-time`, `png`, `serde`, `serde_json` ve `thiserror` crate'lerine bağlıdır; `frameshift-app`, `frameshift-core`, `frameshift-cli` ve `frameshift-mcp` ise `frameshift-media`'ya bağlıdır. Buna karşılık `frameshift-app` (`crates/frameshift-app/Cargo.toml:7-13`) `frameshift-core` veya `frameshift-vector`'a bağlı **değildir**.

2. **Ses Decode, Mix, Önbellek ve DSP Altyapısı:**
   - `FfmpegBridge::decode_audio` (`ffmpeg.rs:1107-1158`) herhangi bir ses/video dosyasını `ffmpeg -vn -ac <ch> -ar <sr> -f f32le pipe:1` ile interleaved `f32` PCM `AudioBuffer` olarak çözer.
   - `MediaResolver::load_audio_asset` (`resolver.rs:557-606`) disk medya damgasına (`DiskMediaStamp`) veya sentetik anahtara (`synthetic:beat120:...`) göre decode edilmiş `AudioBuffer`'ları `audio_cache` içinde saklar; `golden_scene_preset()` içindeki `local_media/beat_track.wav` diskte yoksa `match_synthetic_asset` (`resolver.rs:883-888`) `SyntheticAssetKind::BeatAudio120Bpm` üzerinden 120 BPM saf Rust tıklama/davul tamponu üretir (`synthetic.rs:222-278`).
   - `MediaResolver::mix_project_audio` (`resolver.rs:614-709`) `AudioClipDef` kliplerini 6 `AudioBus` üzerinde `resolve_audio_clip_bounds` (`anchor_event_id` + `apex_offset_ticks`), `pan`, `fade_in`/`fade_out`, `ducking_rules`, `micro_silences` ve `0.99` soft limiter ile `48_000 Hz` stereo `AudioBuffer` olarak birleştirir.
   - `frameshift-media/src/audio.rs` halihazırda RBJ 2. derece `apply_biquad_filter(buf, BiquadFilterKind::{LowPass, HighPass}, cutoff_hz, q)` (`L949-L1002`), `compute_rms_envelope` (`L574-L595`) ve `analyze_beats_and_energy` (`L643-L742`) fonksiyonlarını içerir.

3. **`snapshot.rs` İçindeki Mevcut Ses Çizimi ve Test Bağımlılıkları:**
   - `crates/frameshift-app/src/snapshot.rs:825-859` üzerinde her `ac in &project.timeline.audio_clips` için `[14, 58, 48, 255]` dolgulu ve `ACCENT_GREEN` (`[16, 185, 129, 255]`) çerçeveli dikdörtgen çizildikten sonra `step * 0.45.sin().abs()` formülüyle 4 px aralıkla **sahte prosedürel barlar** ve `FLICKS_PER_SECOND / 2` (0.5 sn) sabit beat çizgileri çizilmektedir.
   - Ayrıca `snapshot.rs:827-828` şu anda `ac.timeline_start` / `ac.timeline_end` kullanır; `anchor_event_id` + `apex_offset_ticks` (`resolver.rs:761-776`) ofsetini dikkate almaz.
   - `render_desktop_ui_snapshot(controller: &WorkspaceUiController, viewport_frame: &Rgba8Frame, width: u32, height: u32) -> Rgba8Frame` (`snapshot.rs:279-284`, `controller.rs:354-361`) şu an ses verisi veya `MediaResolver` almaz ve çağrısı sırasında alt süreç (`ffmpeg`) başlatmaz.
   - Doğrulama testleri:
     - `app_tests.rs:238-306` (`desktop_ui_snapshot_is_deterministic_1920x1080_and_embeds_viewport_and_tracks`): `snap_1.fnv1a_hash64() == snap_2.fnv1a_hash64()`, `(932, 350)` pikselinde viewport rengi `[245, 70, 140, 255]`, playhead scrub sonrası hash değişimi ve `y in 700..900, x in 150..1200` bölgesinde video klip mavisi `[37, 99, 235, 255]` arar.
     - `snapshot.rs:1636-1725` ve `1728-1821`: 1920×1080, 1280×720 ve 640×360 boyutlarını `Project::golden_scene_preset()` üzerinde test eder; sağ panel düğüm kutularını ve üst başlık boşluğunu doğrular.
     - `frameshift-cli/src/main.rs:731-758` (`verify-all` M4 kapısı): `controller.render_desktop_ui_snapshot(&viewport, 1920, 1080)` çağrısının `1920x1080` PNG dosyası üretmesini ve deterministik hash üretmesini bekler.

4. **CLI ve MCP Yüzeylerinin Mevcut Durumu:**
   - CLI (`crates/frameshift-cli/src/main.rs:1581-1604`): `peaks`, `autocut`, `frame`, `ui-snapshot` komutları `TimeSpec` (`Flicks`, `1.905s`, `1/24s`, `f:<frame>`) ve `RangeSpec` (`--range <start>..<end>`) ayrıştırıcılarını paylaşır (`L921-L1105`). `load_analysis_audio(&project, asset_id)` (`L1153-L1164`) hem `--asset <id>` hem de proje miksini yükleyebilir.
   - MCP (`crates/frameshift-mcp/src/server.rs:961-971`): `tools/call` yanıtları şu an yalnızca `content: [{ "type": "text", "text": ... }]` ve `structuredContent` döndürür; `inspection-contract.md` §2.1 (`L46`) ise görsel yüzeylerde MCP adaptörünün PNG payload'ını modele iletmesini öngörür.

---

## Çıkarımlar ve Öneriler

> Aşağıdaki mimari tasarım, doğrudan gözlenen crate sınırları ve DSP ilkelerinden çıkarılmış öneridir.

### 1. Crate Yerleşimi: `frameshift-media/src/waveform.rs`

`WaveformRenderer` ve `WaveformPyramid` tiplerinin **`frameshift-media`** içinde (`pub mod waveform;`) yaşaması önerilir:
- **Neden:** `png = "0.18"` ve `Rgba8Frame` zaten `frameshift-media`'dadır; `frameshift-app`, `frameshift-core`, `frameshift-cli` ve `frameshift-mcp` crate'lerinin tamamı `frameshift-media`'ya doğrudan bağımlıdır ve döngüsel bağımlılık oluşmaz (`0` yeni crate).
- `frameshift-app/src/snapshot.rs` içinde ses klip kutusunun rasterizasyonu doğrudan `frameshift_media::waveform` yardımcısını kullanabilir; `frameshift-media` disk/FFmpeg bağımlılığı olmadan saf `&AudioBuffer` veya `&WaveformPyramid` üzerinde çalışır.

### 2. Çok Çözünürlüklü Peak Pyramid (`WaveformPyramid`) ve Büyük Dosya Performansı

5 dakikalık `48 kHz` stereo bir `AudioBuffer` ≈ `28.8M` örnek (~`115 MB` `f32`) içerir. Her zoom/pan render'ında tüm ham örnekleri taramak yerine güç-2 kademeli bir özet piramit inşa edilir:

- **Taban Seviye (`L0`, `bucket_frames = 256` @ 48 kHz ≈ `5.33 ms`):**
  - 5 dakika (300 sn) için `56,250` kova; kova başına `24 bayt` (`min: f32`, `max: f32`, `rms: f32`, `band_energy: [f32; 3]`) ≈ **`1.35 MB`**.
- **Üst Seviyeler (`L1 = 1024`, `L2 = 4096`, `L3 = 16384`, `L4 = 65536`):**
  - Her 4 alt kovayı birleştirir:
    - `min = min_i(b_i.min)`, `max = max_i(b_i.max)`
    - `rms = sqrt((sum_i b_i.rms^2) / count)` (gerçek karesel ortalama korunur)
    - `band_energy[k] = sqrt((sum_i b_i.band_energy[k]^2) / count)`
  - Toplam piramit belleği L0'ın `<= 1.333x` katıdır (~`1.8 MB` / 5 dk).
- **Render Zamanı Seviye Seçimi:**
  - Hedef sütun başına düşen ses çerçevesi `frames_per_px = span_frames / plot_width`.
  - Eğer `frames_per_px < 1.0` (aşırı zoom-in, örn. `< 40 ms` pencere): piramit yerine ham `AudioBuffer` örneğinden **doğrusal ara-bağlantılı polyline + örnek noktaları** çizilir; böylece sıfır-geçiş sütunlarında kopukluk olmaz.
  - Eğer `1.0 <= frames_per_px < 256.0`: `L0` veya doğrudan ham örnek aralığı taranır (ör. 1000 px için en fazla `256K` frame ≈ `< 0.5 ms`).
  - Eğer `frames_per_px >= 256.0`: `bucket_frames <= frames_per_px / 2` koşulunu sağlayan en kaba piramit seviyesi seçilir; böylece `O(plot_width)` sabit maliyetle (`1920 px` için ~`4K–8K` kova okuması) <1 ms'de sütun indirgeme tamamlanır.

### 3. 3-Bant Spektral Renklendirme (0 Yeni Crate ile RBJ Biquad Crossover)

Mevcut `audio.rs:949` üzerindeki `apply_biquad_filter` (`LowPass` / `HighPass`, Butterworth `Q = 1/sqrt(2)`) kullanılarak **FFT kütüphanesi eklemeden** `O(N)` karmaşıklıkta 3-bant enerji çıkarımı yapılabilir:
1. Mono mixdown (`to_mono_mixdown`) kopyaları üzerinde:
   - **Low (`20–250 Hz`):** `LowPass(250 Hz)` → kick, sub-bass (`band[0]`).
   - **High (`2500 Hz+`):** `HighPass(2500 Hz)` → hi-hat, zil, riser (`band[2]`).
   - **Mid (`250–2500 Hz`):** `HighPass(250 Hz)` ardından `LowPass(2500 Hz)` (veya `x - low - high` artık bandı; faz uyumu istenirse iki biquad kaskad) → vokal, snare gövdesi, synth (`band[1]`).
   *(Not: Kardeş araştırma olan `2026-11-24-multiband-spectral-flux-downbeat.md` Faz 2'de `rustfft` veya saf Rust FFT eklerse, `WaveformPyramid::build_with_spectral_bands` aynı kova şemasını FFT spektrumundan da doldurabilir; ancak RBJ biquad yolu sıfır bağımlılıkla hemen üretime hazırdır.)*
2. **Renk Haritalaması (`SpectralRgb`):**
   - Kova enerjileri `(e_lo, e_mid, e_hi)` toplam veya `max(e_lo, e_mid, e_hi, 1e-6)` ile normalize edilir; `gamma = 0.65` eğrisi ile zayıf yüksek frekanslar görünür kılınır.
   - FrameShift koyu arayüz paletine uyumlu RGB karışımı:
     - Low → Kırmızı/Turuncu (`[244, 63, 94]`)
     - Mid → Zümrüt/Sarı (`[34, 197, 94]` veya `[234, 179, 8]`)
     - High → Camgöbeği/Mavi (`[56, 189, 248]`)
   - Peak zarfı bu spektral rengin `%42` opak/koyu tonuyla, iç RMS zarfı ise aynı rengin parlak/doygun (`+25%` luma, `%92` opak) tonuyla çizilir.

### 4. Anti-Aliased Dikey Sütun Rasterizasyonu & Dual-Layer Zarf

Her `x` piksel sütunu için:
1. **Genlik Ölçekleme (`AmplitudeScale`):**
   - `Linear`: `v_scaled = (v * normalize_gain).clamp(-1.0, 1.0)`.
   - `Decibel { floor_db }` (örn. `-60.0 dB`): işaret korunarak `mag_norm = ((20 * log10(abs(v).max(1e-6)) - floor_db) / -floor_db).clamp(0.0, 1.0)`.
2. **Süreklilik Koruması (Gap-Free Envelope):**
   - Birbirini izleyen piksel sütunlarında keskin tek-örnek geçişlerinin kopuk görünmemesi için bitişik sütunlar arası `y_top` / `y_bot` aralığı en az `1.0 px` kalınlığa genişletilir ve sessiz kısımlarda `0.0` merkez çizgisi korunur.
3. **Sub-piksel Dikey Kapsama (Vertical Anti-Aliasing):**
   - Sütunun sürekli dikey aralığı `[y_top, y_bot]` (`y_top <= y_bot`) her piksel satırı `py` için:
     ```rust
     let cov = (y_bot.min(py as f32 + 1.0) - y_top.max(py as f32)).clamp(0.0, 1.0);
     ```
   - `cov * layer_alpha` ile arka plan üzerine alpha-blend (`source-over` straight sRGB) yapılır.
   - Çizim sırası: (1) Arka plan + dB kılavuz çizgileri → (2) Peak zarfı (`[y_peak_top, y_peak_bot]`, yarı saydam) → (3) RMS iç çekirdeği (`[y_rms_top, y_rms_bot]`, yüksek kontrast) → (4) Sıfır çizgisi (ince nötr çizgi) → (5) Beat / Downbeat / Cue / Event / Playhead overlay'leri.

### 5. `--range` Zoom, İşaretçiler ve Kendi Kendine Yeten PNG Ruler

- **`--range` Zaman Haritalaması:**
  - `start_tick` ve `end_tick` (`Flicks`, `[start_tick, end_tick)`) doğrudan `ticks_to_audio_samples_nearest(tick, sample_rate)` ile örnek indeksine çevrilir (`0..=total_frames` aralığına kırpılır).
  - Görsel üzerindeki her `x` sütununun zamanı `tick(x) = start_tick + round((x + 0.5) * (end_tick - start_tick) / plot_width)` biçiminde tam sayı `Flicks` eksenine bağlıdır; böylece beat ve `event_id` işaretçileri piksel-kesinliğiyle (`±0.5 px`) dalga formu tepe noktalarının üstüne oturur.
- **İşaretçi Katmanları (`WaveformMarker`):**
  - `Beat`: ince yarı-saydam dikey çizgi (`[110, 231, 183, 120]`).
  - `Downbeat` (ölçü başı): kalın `2px` parlak çizgi + üst şerit çentiği (`[56, 189, 248, 220]`).
  - `EventAnchor` / `CuePeak`: kesikli sarı çizgi + elmas/rozet etiket (`ACCENT_YELLOW = [250, 204, 21, 230]`).
  - `Playhead`: `2px` kırmızı çizgi + üst üçgen (`ACCENT_RED = [239, 68, 68, 255]`).
- **Bağımsız Cetvel & Epistemik Kimlik Şeridi:**
  - `frameshift-media` içinde `frameshift-vector`'a bağımlılık açmadan kompakt bir 5×7 ASCII bitmap yazı tipi (veya `snapshot.rs:1457` ile paylaşılan ortak `ascii_5x7` tablosu) kullanıldığında `WaveformRenderer` tek başına `REV #<id> | BPM <bpm> | [<start>s..<end>s]` başlığını, zaman cetvelini (`0.0s`, `1.0s`, ...) ve `-6 dB / 0 dB` eksen etiketlerini çizebilir. Bu yapı `inspection-contract.md` §1 (`revision_id` + `tick_range`) gereksinimine uyumludur.

### 6. Somut API İmzaları (`frameshift-media/src/waveform.rs`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum WaveformChannelLayout {
    #[default]
    MonoMix,
    StereoSplit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WaveformColorMode {
    DualEnvelope {
        peak_rgba: [u8; 4],
        rms_rgba: [u8; 4],
    },
    SpectralRgb,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum WaveformAmplitudeScale {
    Linear,
    Decibel { floor_db: f32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WaveformMarkerKind {
    Beat,
    Downbeat,
    EventAnchor,
    CuePeak,
    Playhead,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaveformMarker {
    pub tick: Flicks,
    pub label: Option<String>,
    pub kind: WaveformMarkerKind,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaveformColumn {
    pub min: f32,
    pub max: f32,
    pub rms: f32,
    /// Normalize edilmiş (Low, Mid, High) enerjisi [0.0..1.0].
    pub band_energy: [f32; 3],
}

#[derive(Debug, Clone, PartialEq)]
pub struct WaveformPyramid {
    pub sample_rate: u32,
    pub channels: u16,
    pub total_frames: usize,
    pub base_bucket_frames: usize, // varsayılan: 256
    /// Kanal başına seviyeler: levels[ch][level_idx]
    pub levels: Vec<Vec<Vec<WaveformColumn>>>,
}

impl WaveformPyramid {
    pub fn build(buffer: &AudioBuffer, compute_spectral: bool) -> Result<Self, MediaError>;
    pub fn sample_columns(
        &self,
        channel_idx: Option<usize>, // None = mono mix
        start_frame: usize,
        end_frame: usize,
        column_count: usize,
    ) -> Vec<WaveformColumn>;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WaveformRenderConfig {
    pub width: u32,
    pub height: u32,
    pub start_tick: Option<Flicks>,
    pub end_tick: Option<Flicks>,
    pub channel_layout: WaveformChannelLayout,
    pub color_mode: WaveformColorMode,
    pub amplitude_scale: WaveformAmplitudeScale,
    pub normalize_to_peak: bool,
    pub show_ruler: bool,
    pub show_zero_line: bool,
    pub background_rgba: [u8; 4],
    pub title: Option<String>,
    pub revision_id: Option<u64>,
    pub bpm: Option<f64>,
    pub markers: Vec<WaveformMarker>,
}

pub struct WaveformRenderer;

impl WaveformRenderer {
    pub fn render_rgba8(
        buffer: &AudioBuffer,
        config: &WaveformRenderConfig,
    ) -> Result<Rgba8Frame, MediaError>;

    pub fn render_from_pyramid(
        pyramid: &WaveformPyramid,
        raw_fallback: Option<&AudioBuffer>,
        config: &WaveformRenderConfig,
    ) -> Result<Rgba8Frame, MediaError>;

    /// `snapshot.rs` ve GUI katmanı için: mevcut bir RGBA tamponunun `dst` dikdörtgenine
    /// ruler/kenarlık olmadan yalnızca dalga formu sütunlarını anti-aliased çizer.
    pub fn paint_into_rgba_rect(
        dst_pixels: &mut [u8],
        canvas_w: u32,
        canvas_h: u32,
        rect: (i32, i32, i32, i32),
        columns: &[WaveformColumn],
        color_mode: WaveformColorMode,
        amplitude_scale: WaveformAmplitudeScale,
        gain_envelope: Option<&[f32]>, // fade_in / fade_out / gain_db görselleştirmesi
    );
}
```

### 7. `snapshot.rs` Entegrasyonu ve Geriye Dönük Test Uyumluluğu

Mevcut testler (`app_tests.rs:238-306`, `snapshot.rs:1636-1821`) `render_desktop_ui_snapshot` çağrısının `ffmpeg` olmadan deterministik çalışmasını bekler. Önerilen iki aşamalı köprü:

1. **Veri Taşıma Yapısı (`frameshift-app/src/snapshot.rs`):**
   - `SnapshotAudioClipWaveform { pub clip_id: String, pub effective_start: Flicks, pub effective_end: Flicks, pub pyramid: Arc<WaveformPyramid>, pub gain_envelope: Vec<f32> }` veya `SnapshotAudioContext { pub clip_waveforms: HashMap<String, SnapshotAudioClipWaveform>, pub detected_beats: Vec<Flicks> }`.
   - `render_desktop_ui_snapshot(&WorkspaceUiController, &Rgba8Frame, u32, u32)` imzası **aynen korunur** (geriye dönük 100% uyumlu); içeride `render_desktop_ui_snapshot_with_audio(controller, viewport_frame, width, height, None)` çağrısını yapar.
   - `WorkspaceUiController::render_desktop_ui_snapshot_with_audio(&self, ..., audio: Option<&SnapshotAudioContext>)` eklenir.
2. **Fallback ve Deterministik Test Koruması:**
   - `audio == Some(ctx)` ise: `snapshot.rs:826` döngüsü `ctx.clip_waveforms.get(&ac.id)` üzerinden gerçek PCM sütunlarını `WaveformRenderer::paint_into_rgba_rect` ile çizer; ayrıca `ctx.detected_beats` mevcutsa `0.5s` sabit ızgara yerine gerçek beat çizgileri çizilir.
   - `audio == None` ise (mevcut `app_tests.rs` ve `snapshot.rs` birim testleri):
     - `frameshift-app` sentetik varlık yolunu (`local_media/beat_track.wav`, `asset_bgm`, `synthetic://beat*`) tanıdığında pure-Rust `SyntheticMediaGenerator::generate_beat_audio` üzerinden deterministik gerçek PCM dalga formu üretebilir **veya** eksik medya durumunda prosedürel bar fallback'ine döner.
     - Mevcut `app_tests.rs:238-306` testi yalnızca `snap_1 == snap_2` determinizmi, `(932, 350)` viewport pikseli ve `[37, 99, 235, 255]` mavi video klip pikselini doğruladığı için iki yol da mevcut assertion'ları kırmaz; ancak `CliCommand::UiSnapshot` (`main.rs:396-401`) ve `verify-all` (`main.rs:731-738`) `MediaResolver` kullanarak `SnapshotAudioContext` geçirir ve gerçek disk/sentetik ses dalga formunu render eder.

### 8. CLI (`waveform`) ve MCP (`render_waveform`) Tasarımı

- **CLI (`crates/frameshift-cli/src/main.rs`):**
  ```text
  frameshift-cli waveform <project.json> --out <out.png>
    [--asset <id>] [--range <start>..<end>] [--width <px>] [--height <px>]
    [--style dual|spectral] [--channels mono|stereo] [--scale linear|db]
    [--beats] [--events] [--normalize] [--no-ruler]
  ```
  - Varsayılanlar: `1600x360`, `--style dual`, `--channels mono`, `--scale linear`, `--events` açık, `--beats` açık (veya `--no-beats` opsiyonlu; `AudioBuffer::analyze_beats_and_energy(Flicks(FLICKS_PER_SECOND / 100))` ile otomatik beat çizgileri + BPM etiketi).
  - `--asset <id>` verilmezse `MediaResolver::mix_project_audio(&project)` ile tüm mikslenmiş proje sesinin waveform'unu üretir; verildiğinde yalnızca o asset'i decode eder (`load_analysis_audio` kalıbı `main.rs:1153`).
  - `--range <start>..<end>` verildiğinde `RenderRange` veya doğrudan `Flicks` zaman penceresi üzerinden zoom yapar (proje FPS ızgarasına yuvarlamak yerine alt-kare audio zoom için ham `Flicks` aralığı da kabul edilebilir; sınırlar `[0, duration]` içine kırpılır).

- **MCP (`crates/frameshift-mcp/src/server.rs`):**
  - Araç adı: **`render_waveform`** (mevcut `analyze_audio` ve `render_project` isimlendirme stiline paralel).
  - Giriş şeması:
    - `out_path` *(zorunlu string)*, `asset_id` *(opsiyonel string)*, `start_tick` / `end_tick` *(opsiyonel `i64` Flicks)*, `width` *(varsayılan `1600`)*, `height` *(varsayılan `360`)*, `style` (`"dual" | "spectral"`), `channels` (`"mono" | "stereo"`), `scale` (`"linear" | "db"`), `include_beats` *(bool, varsayılan `true`)*, `include_events` *(bool, varsayılan `true`)*, `include_image_content` *(bool, varsayılan `true`)*, `expected_revision_id` *(opsiyonel `u64`)*.
  - Çıktı:
    - Diskte `out_path` PNG dosyası oluşturur.
    - `expected_revision_id` uyuşmazlığında `McpError::RevisionConflict` döndürür (`inspection-contract.md` §1).
    - `tools/call` yanıtında `include_image_content == true` ise `content` dizisine `{ "type": "image", "data": "<base64>", "mimeType": "image/png" }` (küçük saf Rust base64 kodlayıcı ile 0 crate) + metin JSON özeti (`revision_id`, `fnv1a_hash64`, `peak_amplitude`, `estimated_bpm`, `beat_count`, `start_tick`, `end_tick`, `epistemic_note`) ekler.

### 9. Faz 2 Implementasyon Reçetesi (Adım Adım)

1. **`frameshift-media` (`crates/frameshift-media/src/waveform.rs` + `lib.rs`):**
   - `WaveformPyramid`, `WaveformRenderer`, `WaveformRenderConfig`, biquad 3-bant spektral çıkarım, anti-aliased dikey sütun + sub-sample polyline rasterizer ve kompakt 5×7 ASCII ruler/etiket çizicisini ekle; birim testleri (sessiz buffer, 1 kHz sinüs peak/RMS doğruluğu, stereo split yüksekliği, zoom-in polyline, 3-bant spektral renk ayrımı, deterministik FNV-1a hash) yaz.
2. **`frameshift-core` (`crates/frameshift-core/src/resolver.rs` veya `engine.rs`):**
   - `MediaResolver` üzerine `waveform_cache: RefCell<HashMap<String, Arc<WaveformPyramid>>>` (veya `Engine::build_project_waveform`) ekleyip `anchor_event_id` + `fade_in/out` zarflı `SnapshotAudioContext` oluşturucu helper sağla.
3. **`frameshift-app` (`crates/frameshift-app/src/snapshot.rs` + `controller.rs`):**
   - `render_desktop_ui_snapshot_with_audio` ekle; mevcut `render_desktop_ui_snapshot` imzasını `None` delegasyonuyla koru; A1 ses klip çizimini gerçek PCM dalga formu + `resolve_audio_clip_bounds` uyumu + fallback ile güncelle.
4. **`frameshift-cli` (`crates/frameshift-cli/src/main.rs`) & `frameshift-mcp` (`crates/frameshift-mcp/src/server.rs`):**
   - `waveform` CLI alt komutunu ve `ui-snapshot` / `verify-all` ses bağlamı aktarımını ekle.
   - `render_waveform` MCP aracını ve `tools/call` içinde opsiyonel `type: "image"` base64 PNG desteğini ekle.
5. **Doğrulama:**
   - `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `frameshift-cli waveform` ile gerçek `.wav`/`.mp4` çıktısı üretimi ve görsel inceleme.

---

## Doğrulanmamış Noktalar

- **Büyük Dosya Benchmark'ı:** 10–30 dakikalık `48 kHz` stereo bir ses dosyasında 3× `apply_biquad_filter` + `WaveformPyramid::build` çalışma süresi henüz ölçülmedi (tahmini ~15–40 ms; tek geçişli biquad ile ham örnek tarama arasında gerçek CPU profili çıkarılmalıdır).
- **MCP İstemci Uyumluluğu:** Antigravity / dış MCP istemcilerinin `tools/call` yanıtında `content` içinde hem `image` hem `text` + `structuredContent` kombinasyonunu nasıl işlediği canlı `mcp-stdio` testiyle doğrulanmamıştır.
- **Gerçek Sesli `ui-snapshot` Okunabilirliği:** `1920x1080` çözünürlükte `A1 AUDIO` şeridinin mevcut yüksekliği (`a_lane_h = tracks_avail * 24 / 100`, yaklaşık `~60–75 px`) içinde `ac.id` metin etiketi ile gerçek dalga formunun üst üste okunabilirliği görsel çıktıyla kontrol edilmelidir.

---

## Kullanıcı Kararı Gerektiren Açık Noktalar

1. **Varsayılan Görsel Stil (`WaveformColorMode`):**
   - **Seçenek A (Önerilen Varsayılan):** Varsayılan olarak **Klasik Çift Katmanlı Zarf (`DualEnvelope`: yarı saydam Peak dış + parlak RMS iç)**, `--style spectral` bayrağıyla isteğe bağlı **3-Bant RBJ Biquad Spektral RGB** (`Low=Kırmızı/Turuncu`, `Mid=Yeşil`, `High=Cyan`).
     - *Artısı:* FrameShift'in mevcut koyu UI paletiyle (`ACCENT_GREEN` / `ACCENT_CYAN`) doğrudan uyum sağlar, ek biquad filtresi maliyeti varsayılan yolda sıfırdır; DJ/AMV drop analizi istendiğinde `spectral` seçilebilir.
   - **Seçenek B:** Varsayılan olarak **3-Bant Spektral RGB** (`SpectralRgb`), `dual` opsiyonel.
     - *Artısı:* Drop, kick ve hi-hat yoğunluğunu her çıktıda anında gösterir; *Eksisi:* her `WaveformPyramid` inşasında 3× biquad geçişi gerektirir.
   - **Seçenek C:** Yalnızca tek katmanlı Peak + RMS (`DualEnvelope`), spektral renklendirme hiç eklenmez (yalnızca `multiband-spectral-flux` ile ayrı grafik olarak kalır).

2. **`snapshot.rs` Ses Kaynağı ve Geriye Dönük Test Stratejisi:**
   - **Seçenek A (Önerilen Varsayılan):** `render_desktop_ui_snapshot(...)` imzasını değiştirmeden koru (`None` durumunda sentetik varlıklar için saf Rust `generate_beat_audio` veya prosedürel fallback); yanında `render_desktop_ui_snapshot_with_audio(..., Option<&SnapshotAudioContext>)` açarak `CliCommand::UiSnapshot` ve `verify-all` içinden gerçek decode edilmiş/mikslenmiş sesi geçir.
     - *Artısı:* `ffmpeg` olmayan ortamlarda `frameshift-app` testleri `%100` hızlı ve deterministik kalır; canlı CLI/masaüstü kabuğunda ise gerçek PCM dalga formu gösterilir.
   - **Seçenek B:** `snapshot.rs` içinde `frameshift-app`'e doğrudan `FfmpegBridge` çağrısı eklemek.
     - *Eksisi:* Her `render_desktop_ui_snapshot` çağrısını yavaş G/Ç bağımlısı yapar ve `ffmpeg` olmayan test ortamlarını bozar.

3. **CLI `waveform` Girdi & Parametre Seti:**
   - **Seçenek A (Önerilen Varsayılan):** Mevcut `peaks` / `autocut` komutlarıyla aynı sözleşme: `frameshift-cli waveform <project.json> --out <out.png> [--asset <id>] [--range <start>..<end>] [--width <px>] [--height <px>] [--style dual|spectral] [--channels mono|stereo] [--scale linear|db] [--beats] [--events] [--no-ruler]`.
   - **Seçenek B:** Seçenek A'ya ek olarak proje JSON'u olmadan tekil ses/video dosyası alabilen `--input <audio.wav|mp3|mp4>` modu eklemek.
     - *Not:* Seçenek B hızlı tekil ses incelemesinde pratiktir; istenirse `<project.json>` yerine `--input <path>` alternatif pozisyonel/bayrak yolu olarak Faz 2'de eklenebilir.
