---
title: "Sürekli Video Decoder (StreamingVideoDecoder & DecoderPool) Mimari Araştırması ve Şartnamesi"
date: 2026-11-22
status: active
tags: [media-io, engine, timebase, performance, ffmpeg, streaming-decoder]
---

# Sürekli Video Decoder (`StreamingVideoDecoder` & `DecoderPool`) Mimari Araştırması ve Şartnamesi

## Soru / Amaç

FrameShift'in mevcut video okuma hattı ([`FfmpegBridge::decode_video_window_with_fit`](../../crates/frameshift-media/src/ffmpeg.rs#L396-L473) ve [`MediaResolver::provide_video_frame`](../../crates/frameshift-core/src/resolver.rs#L89-L163)), önbellek dışı (cache miss) her erişimde `DEFAULT_DECODE_WINDOW = 32` karelik yeni bir `ffmpeg` alt süreci başlatıp kapatmaktadır. Bu tasarım tekil kare spawn maliyetini azaltmış olsa da, `asta_amv_project.json` (`sub_samples=6`) gibi çok klipli gerçek AMV projelerinde her 32 karede bir yeniden `ffmpeg` başlatma + keyframe seek + filtre grafiği kurulumu nedeniyle toplam render süresini **`112.56s`** bandında tutmaktadır.

Bu araştırmanın amacı; sistemde `FFMPEG_DIR` ve `clang`/`vcpkg` bulunmadığı, buna karşın statik `ffmpeg 9.0.2` ikili dosyasının hazır olduğu mevcut çalışma ortamında **0 yeni Cargo crate ekleyerek**:
1. Açık `Child` rawvideo pipe süreçlerini LRU havuzunda yaşatan (`DecoderPool` / `StreamingVideoDecoder`) mimarisini tanımlamak,
2. İleri sıralı okuma, küçük ileri atlama (`0 < delta <= MAX_SEQUENTIAL_SKIP`), geri sarma/uzak seek, EOF ve `Drop` altında zombie/yetim process temizliği davranışlarını Rust `std::process` ve FFmpeg birincil kaynaklarıyla doğrulamak,
3. M1–M5 determinizm, bit-for-bit eşdeğerlik ve seek-independence garantilerini bozmadan [`MediaResolver`](../../crates/frameshift-core/src/resolver.rs#L36-L40) ve [`LruFrameCache`](../../crates/frameshift-media/src/cache.rs#L54-L64) ile entegrasyon sözleşmesini kesin Rust imzalarıyla ortaya koymaktır.

---

## İncelenen Birincil Kaynaklar

### Harici Birincil Kaynaklar (Doğrulama Tarihi: `2026-11-22`)
1. **Rust Standard Library — `std::process::Child` (`kill`, `wait`, `Drop` davranışı):**
   - [https://doc.rust-lang.org/std/process/struct.Child.html](https://doc.rust-lang.org/std/process/struct.Child.html) — `Child::kill` (Unix'te `SIGKILL`, Windows'ta alt katmanda süreç sonlandırma), `Child::wait` (child `stdin`'ini kapatıp OS kaynaklarını toplar/reap eder) ve `Child`'ın `Drop` edildiğinde alt süreci otomatik olarak öldürmediği veya beklemediği belgelenmiştir.
2. **Rust Standard Library — `std::process::Command` & Pipe Deadlock Koruması:**
   - [https://doc.rust-lang.org/std/process/struct.Command.html](https://doc.rust-lang.org/std/process/struct.Command.html) — `Stdio::piped()` ile `stdout`/`stderr` tamponlarının dolması halinde ebeveyn-alt süreç kilitlenmesi (deadlock) riski ve `stdin(Stdio::null())` / eşzamanlı okuma gerekliliği.
3. **Rust Standard Library — `std::process::ChildStdout`, `std::io::BufReader`, `std::io::Read::read_exact`, `ErrorKind::UnexpectedEof`:**
   - [https://doc.rust-lang.org/std/process/struct.ChildStdout.html](https://doc.rust-lang.org/std/process/struct.ChildStdout.html)
   - [https://doc.rust-lang.org/std/io/struct.BufReader.html](https://doc.rust-lang.org/std/io/struct.BufReader.html)
   - [https://doc.rust-lang.org/std/io/trait.Read.html#method.read_exact](https://doc.rust-lang.org/std/io/trait.Read.html#method.read_exact)
   - [https://doc.rust-lang.org/std/io/enum.ErrorKind.html#variant.UnexpectedEof](https://doc.rust-lang.org/std/io/enum.ErrorKind.html#variant.UnexpectedEof)
4. **FFmpeg Resmi Dokümantasyonu — `-fps_mode passthrough`, Pre-Input `-ss` (Fast + Accurate Seek), `-f rawvideo`:**
   - [https://ffmpeg.org/ffmpeg.html#Main-options](https://ffmpeg.org/ffmpeg.html#Main-options) — `-fps_mode passthrough` (eski `-vsync passthrough` yerine; kareleri demuxer zaman damgasıyla düşürmeden/çoğaltmadan geçirir) ve `-i` öncesinde verilen `-ss position` (hızlı keyframe atlaması + varsayılan `-accurate_seek` ile hedef noktaya kadar decode edip öncesini atma).
   - [https://ffmpeg.org/ffmpeg-formats.html#rawvideo](https://ffmpeg.org/ffmpeg-formats.html#rawvideo) — Başlıksız (headerless) ham piksel akışı semantiği; tüketici çözünürlük ve `pix_fmt` bilgisini bilmek zorundadır.

### Yerel Kod Tabanı & Sözleşme Kaynakları
- [Cargo.toml](../../Cargo.toml), [crates/frameshift-media/Cargo.toml](../../crates/frameshift-media/Cargo.toml), [crates/frameshift-core/Cargo.toml](../../crates/frameshift-core/Cargo.toml)
- [crates/frameshift-media/src/ffmpeg.rs](../../crates/frameshift-media/src/ffmpeg.rs) (`SourceFit`, `MediaProbeInfo`, `FfmpegBridge`, `decode_video_window_with_fit`, `decode_video_frame_internal`, `append_fitted_output`, `midpoint_seek_seconds`)
- [crates/frameshift-media/src/cache.rs](../../crates/frameshift-media/src/cache.rs) (`LruFrameCache`, `FrameCacheKey`, `CacheTelemetry`)
- [crates/frameshift-media/src/frame.rs](../../crates/frameshift-media/src/frame.rs) (`Rgba8Frame`, `AlphaMode`)
- [crates/frameshift-media/src/error.rs](../../crates/frameshift-media/src/error.rs) (`MediaError`)
- [crates/frameshift-media/src/lib.rs](../../crates/frameshift-media/src/lib.rs)
- [crates/frameshift-core/src/resolver.rs](../../crates/frameshift-core/src/resolver.rs) (`DEFAULT_DECODE_WINDOW`, `MediaResolver`, `ProjectFrameProvider`)
- [crates/frameshift-core/src/engine.rs](../../crates/frameshift-core/src/engine.rs) (`Engine`, `DEFAULT_CACHE_CAPACITY`, `DEFAULT_CACHE_MAX_BYTES`, `render_timeline_sequence`, `verify_m1_golden_scene`)
- [crates/frameshift-time/src/pts_index.rs](../../crates/frameshift-time/src/pts_index.rs) (`PtsIndex`, `FrameMatch`)
- [crates/frameshift-render/src/compositor.rs](../../crates/frameshift-render/src/compositor.rs#L1471-L1573) (`MotionBlurShutter` alt-örnek döngüsü ve `NodeKind::SourceVideo` çağrısı)
- Kilitli sözleşmeler: [render-graph.md](../specs/render-graph.md), [timebase.md](../specs/timebase.md)

---

## Doğrudan Gözlemler (Kanıtlı)

### 1. Mevcut Kod Tabanı Durumu (`frameshift-media`, `frameshift-core`, `frameshift-render`)
- **Mevcut Pencere Tabanlı Decode ([`ffmpeg.rs`](../../crates/frameshift-media/src/ffmpeg.rs#L396-L473)):**
  - `decode_video_window_with_fit` her çağrıda `Command::new("ffmpeg")` ile `"-v", "error", "-nostdin"`, varsa pre-input `"-ss" <seek_sec:.9>`, `"-i" <path>`, `"-fps_mode", "passthrough"`, `"-frames:v", <count>`, [`append_fitted_output`](../../crates/frameshift-media/src/ffmpeg.rs#L852-L868) (`Stretch` -> `-s`, `Cover` -> `scale+crop`, `Contain` -> `scale+format=rgba+pad`) ve `"-pix_fmt", "rgba", "-f", "rawvideo", "pipe:1"` çalıştırıp `cmd.output()` ile sürecin bitmesini bekler.
  - `midpoint_seek_seconds(pts_index, frame_index)` ([`ffmpeg.rs`](../../crates/frameshift-media/src/ffmpeg.rs#L870-L887)): `frame_index == 0` veya `target_pts <= 0` ise `None` döner; aksi halde önceki kare ile arasındaki boşluğun yarısı kadar (en fazla `0.5 ms`, en az `1` tick) geriye giderek float yuvarlamanın bir sonraki kareye atlamasını engeller.
  - `SourceFit` enum'ı ([`ffmpeg.rs`](../../crates/frameshift-media/src/ffmpeg.rs#L35-L41)) şu anda `#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]` taşır; `Hash` türetimi henüz yoktur.
- **Mevcut Önbellek ve Çözücü ([`cache.rs`](../../crates/frameshift-media/src/cache.rs#L54-L163), [`resolver.rs`](../../crates/frameshift-core/src/resolver.rs#L15-L163)):**
  - [`Engine`](../../crates/frameshift-core/src/engine.rs#L28-L57), `DEFAULT_CACHE_CAPACITY = 128` kare ve `DEFAULT_CACHE_MAX_BYTES = 1024 * 1024 * 1024` (1 GiB) sınırıyla `LruFrameCache` oluşturur.
  - [`LruFrameCache`](../../crates/frameshift-media/src/cache.rs#L54-L64), `HashMap<FrameCacheKey, CacheEntry>` + `BTreeMap<u64, FrameCacheKey>` kullanarak ek harici crate olmadan $O(\log N)$ deterministik LRU tahliyesi uygular.
  - [`MediaResolver::provide_video_frame`](../../crates/frameshift-core/src/resolver.rs#L107-L163), önbellek anahtarı olarak `"{asset_id}@{width}x{height}:{fit}"` + `matched.frame_index as u64` kullanır; cache miss olduğunda `DEFAULT_DECODE_WINDOW = 32` (`(cache.capacity / 2).max(1)` ile sınırlı) karelik pencereyi tek seferde decode edip hepsini `LruFrameCache` içine koyar.
- **Render Erişim Deseni ([`compositor.rs`](../../crates/frameshift-render/src/compositor.rs#L1471-L1573), [`engine.rs`](../../crates/frameshift-core/src/engine.rs#L213-L229)):**
  - `render_timeline_sequence` çıktı karelerini baştan sona sıralı `tick` listesi üzerinde değerlendirir.
  - Kök düğüm `MotionBlurShutter` olduğunda her çıktı karesi için `sample_ticks` alt-örnekleri sırayla çalıştırılır (`L1494`); hız eğrisi yavaşlatma/hızlandırma/ters oynatma yapıyorsa yakın `frame_index` değerleri art arda veya küçük sıçramalarla `provide_source_frame`'e düşer.
- **Workspace Bağımlılıkları ([`frameshift-media/Cargo.toml`](../../crates/frameshift-media/Cargo.toml)):**
  - Yalnızca `frameshift-time`, `png`, `serde`, `serde_json`, `thiserror` bağımlılıkları mevcuttur.

### 2. Oturumda Doğrulanmış Sistem Ortamı ve Gerçek Benchmark Ölçümleri
*(Bu bölümdeki ortam ve süre ölçümleri, mevcut oturumda çalıştırılarak doğrulanmış gerçek değerlerdir.)*

- **Derleme / Sistem Kısıtı:** Sistemde `FFMPEG_DIR`, `clang` ve `vcpkg` kurulu değildir (dolayısıyla `ffmpeg-the-third` / `libavcodec` C FFI bağlaması bu ortamda doğrudan derlenemez); buna karşılık statik `ffmpeg 9.0.2` ikilisi sistemde mevcuttur.
- **`clip_1.mp4` (`1920x1080`, `60fps`) Üzerinde `120` Kare `1280x720 rgba` Pipe Ölçümleri:**
  - **Tek sürekli pipe (`120` kare kesintisiz):** `0.427s` (~`281 fps`) -> kare başı ~`3.56 ms`.
  - **`4 × 30` kare ayrı pencere (mevcut `decode_video_window` deseni):** `0.871s` (~`137.8 fps`) -> sürekli pipe'ın `2.04x` gerisinde.
  - **Orta noktaya `-ss` seek + `30` kare:** `0.237s`.
  - **`-hwaccel d3d11va` ile CPU raw pipe (`120` kare):** `0.904s` -> donanım yüzeyinden CPU RAM'ine readback + pipe maliyeti nedeniyle yazılımdan (`0.427s`) **`2.12x` daha yavaş**.
- **Tam Proje Ölçümü:** `asta_amv_project.json` (`sub_samples=6`) mevcut pencereli decode (`DEFAULT_DECODE_WINDOW=32`) ile **`112.56s`** sürmüştür.

### 3. Rust `std::process` & `std::io` ve FFmpeg Pipe Davranışları (Birincil Kaynaklar)
- **`Child` Yaşam Döngüsü ve Zombie Koruması (`doc.rust-lang.org`):**
  - `std::process::Child` `Drop` sırasında alt süreci **öldürmez ve beklemez**; açık bırakılan süreçler arka planda yaşamaya devam eder veya çıkış sonrası OS süreç tablosunda zombie olarak kalır.
  - Güvenli kapatma sırası: `stdout` (`BufReader<ChildStdout>`) serbest bırakılır -> `let _ = child.kill();` (Windows'ta zorla sonlandırır) -> `let _ = child.wait();` (işletim sistemi tutamacını ve çıkış durumunu temizler).
- **`BufReader<ChildStdout>` + `read_exact` (`doc.rust-lang.org`):**
  - `read_exact(&mut frame_buf)` tam `width * height * 4` byte dolana kadar `ChildStdout` üzerinden okur; stream beklenenden erken biterse `std::io::ErrorKind::UnexpectedEof` döndürür.
  - `BufReader::with_capacity(cap, stdout)` büyük tamponla (örn. 1–4 MiB) sistem çağrısı sayısını azaltır; ancak `BufReader` yok edilip iç `ChildStdout` geri alınırsa ön-okunmuş tampon verisi kaybolacağından `BufReader<ChildStdout>` stream ömrü boyunca aynı yapıda tutulmalıdır.
- **`stdin` ve `stderr` Pipe Güvenliği (`doc.rust-lang.org`):**
  - `frameshift-cli mcp-stdio` JSON-RPC için ebeveyn sürecin `stdin`/`stdout`'unu kullandığından, alt `ffmpeg` süreçleri yalnızca `-nostdin` argümanıyla değil, mutlaka `.stdin(Stdio::null())` ile başlatılmalıdır.
  - `stderr(Stdio::piped())` kullanılıp ebeveyn tarafından aktif okunmazsa `ffmpeg`'in hata/uyarı çıktısı OS pipe tamponunu doldurduğunda süreç kilitlenebilir (deadlock). Bu nedenle uzun ömürlü streaming `Child` için `stderr` ya sınırlı bir arka plan `std::thread::spawn` okuyucusuyla tüketilmeli (örn. ilk 32 KiB yakalanıp geri kalanı drain edilir) ya da sessiz modda güvenli şekilde boşaltılmalıdır.
- **FFmpeg `-fps_mode passthrough` + Pre-Input `-ss` (`ffmpeg.org`):**
  - `-ss <t>` parametresi `-i` öncesinde verildiğinde FFmpeg en yakın keyframe'e hızlı atlar ve `-f rawvideo` çıkışında varsayılan olarak aktif olan `-accurate_seek` sayesinde `<t>` konumuna kadar ara kareleri çözüp atarak çıktıyı tam hedef zaman damgasından başlatır.
  - `-fps_mode passthrough` her kareyi demuxer/decoder zaman damgasıyla geçirir; VFR veya kesirli FPS dosyalarda `PtsIndex` indisleriyle birebir hizayı bozabilecek kare çoğaltma veya düşürme yapmaz.
  - `-frames:v` argümanı verilmediğinde `pipe:1` akış sonuna kadar açık kalır; medya sonuna gelindiğinde FFmpeg temiz çıkış yapar ve pipe EOF verir.

---

## Çıkarımlar ve Öneriler (Mimari Şartname)

*(Aşağıdaki tasarım kararları ve eşik değerleri, yukarıdaki doğrudan gözlemler ile benchmark verilerine dayalı mühendislik çıkarımlarıdır.)*

### 1. Temel Mimari Kararlar ve Hesaplamalar

1. **Neden `StreamingVideoDecoder` + `DecoderPool` (0 Yeni Crate)?**
   - Sistemde `FFMPEG_DIR`, `clang` ve `vcpkg` bulunmadığı için `libavcodec` C FFI (`ffmpeg-the-third`) entegrasyonu mevcut ortamda derlenemez.
   - Sürekli pipe ölçümü (`0.427s` / 120 kare = ~`281 fps`) `ffmpeg.exe` süreç içi decoding'inin yeterince hızlı olduğunu, mevcut yavaşlığın temel kaynağının **her 32 karede bir yeni process spawn + keyframe seek** (`0.871s`) olduğunu göstermektedir.
   - Bu nedenle `frameshift-media` içine tamamen Rust `std` ile yazılan `StreamingVideoDecoder` + `DecoderPool` eklenmelidir.
2. **Neden CPU Pipe'ta `-hwaccel d3d11va` Kullanılmamalı?**
   - Ölçülen `0.904s` vs `0.427s` sonucu açıkça göstermektedir ki donanım decode + CPU RAM readback + pipe aktarımı yazılımdan `2.12x` yavaştır. `StreamingVideoDecoder` CPU pipe hattında varsayılan olarak yazılım decode kullanmalıdır (ileride doğrudan GPU texture interop yapılmadığı sürece `-hwaccel` eklenmemelidir).
3. **`MAX_SEQUENTIAL_SKIP` Eşiği Hesabı (Çıkarım):**
   - `clip_1.mp4` (`1080p->720p`) verilerinden: sürekli akışta 1 kare maliyeti $\approx 0.427\text{s} / 120 \approx 3.56\text{ ms/kare}$.
   - `30` kareli `-ss` seek toplamı `0.237s` olduğuna göre, tek bir yeni süreç spawn + keyframe seek + filtre kurulumu sabit yükü $\approx 0.237\text{s} - (30 \times 0.00356\text{s}) \approx 0.130\text{s}$ mertebesindedir.
   - Başabaş noktası: $0.130\text{s} / 0.00356\text{s} \approx 36.5$ kare.
   - **Öneri:** `DEFAULT_MAX_SEQUENTIAL_SKIP: usize = 32`. `0 < delta <= 32` aralığındaki ileri atlamalarda yeni süreç açmak yerine açık pipe üzerinden `delta` kareyi okuyup atlamak (zero-allocation tek bir yeniden kullanılabilir `scratch_buf` ile) sürecin yeniden başlatılmasından hem daha hızlı hem daha kararlıdır. `delta > 32` veya `delta < 0` (geri sarma) durumlarında ise stream kapatılıp `-ss` ile yeniden açılır.
4. **Neden `DEFAULT_MAX_OPEN_STREAMS = 8`?**
   - Çoklu klip geçişlerinde (`ShaderTransition` veya `Blend` içinde eşzamanlı 2–4 aktif video katmanı) her katman kendi sürekli stream'ini açık tutabilmeli, ancak Windows handle ve `ffmpeg` bellek tüketimi (her `ffmpeg` ~30–80 MiB) kontrolsüz büyümemelidir. 8 açık stream, tipik AMV/edit grafiğindeki eşzamanlı katmanları thrashing olmadan kapsar.

---

### 2. Kesin Rust Veri Modelleri ve İmzalar (`crates/frameshift-media`)

#### 2.1. Ön Koşul Düzenlemesi (`crates/frameshift-media/src/ffmpeg.rs`)
`SourceFit` (`L35-L41`) `Hash` derive'ı almalıdır ki `StreamKey` içinde doğrudan hash anahtarı olabilsin:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SourceFit {
    #[default]
    Stretch,
    Cover,
    Contain,
}
```

Ayrıca [`append_fitted_output`](../../crates/frameshift-media/src/ffmpeg.rs#L852-L868) ve [`midpoint_seek_seconds`](../../crates/frameshift-media/src/ffmpeg.rs#L870-L887) fonksiyonları `pub(crate)` görünürlüğüne çekilerek yeni `streaming.rs` modülünden birebir paylaşılır (sıfır kod tekrarı ve `-s` / `-vf` fit semantiğinde %100 tutarlılık).

#### 2.2. Yeni Modül: `crates/frameshift-media/src/streaming.rs`

```rust
use crate::error::MediaError;
use crate::ffmpeg::{SourceFit, append_fitted_output, midpoint_seek_seconds};
use crate::frame::{AlphaMode, Rgba8Frame};
use frameshift_time::PtsIndex;
use std::collections::{BTreeMap, HashMap};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::thread::JoinHandle;

/// Varsayılan eşzamanlı açık `ffmpeg` streaming süreci üst sınırı.
pub const DEFAULT_MAX_OPEN_STREAMS: usize = 8;

/// Yeni process spawn + keyframe seek yerine ileri okuyup atlanacak azami kare farkı.
pub const DEFAULT_MAX_SEQUENTIAL_SKIP: usize = 32;

/// Pipe I/O sistem çağrılarını azaltmak için `BufReader` kapasitesi (2 MiB).
const STREAM_READ_BUF_BYTES: usize = 2 * 1024 * 1024;

/// `DecoderPool` içinde her açık FFmpeg sürecini benzersiz tanımlayan anahtar.
///
/// Aynı dosyanın farklı çözünürlük veya farklı `SourceFit` (`stretch`/`cover`/`contain`)
/// isteyen düğümleri ayrı stream yuvalarında tutulur.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StreamKey {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub fit: SourceFit,
}

impl StreamKey {
    pub fn new(path: impl Into<PathBuf>, width: u32, height: u32, fit: SourceFit) -> Self {
        Self {
            path: path.into(),
            width,
            height,
            fit,
        }
    }
}

/// `DecoderPool` yapılandırması.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecoderPoolConfig {
    pub max_open_streams: usize,
    pub max_sequential_skip: usize,
}

impl Default for DecoderPoolConfig {
    fn default() -> Self {
        Self {
            max_open_streams: DEFAULT_MAX_OPEN_STREAMS,
            max_sequential_skip: DEFAULT_MAX_SEQUENTIAL_SKIP,
        }
    }
}

/// Sürekli decode telemetrisi (test ve performans doğrulaması için).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DecoderPoolTelemetry {
    pub active_streams: usize,
    pub spawns: u64,
    pub sequential_reads: u64,
    pub skipped_frames: u64,
    pub seek_reopens: u64,
    pub lru_evictions: u64,
    pub eof_reaches: u64,
}

/// Tek bir açık `ffmpeg` rawvideo stdout pipe oturumu.
#[derive(Debug)]
pub struct StreamingVideoDecoder {
    key: StreamKey,
    child: Child,
    reader: BufReader<ChildStdout>,
    stderr_drain: Option<JoinHandle<String>>,
    /// Bir sonraki `read_exact` çağrısının okuyacağı beklenen `PtsIndex` kare indisi.
    next_cursor: usize,
    frame_byte_len: usize,
    scratch_skip_buf: Vec<u8>,
    finished: bool,
}

impl StreamingVideoDecoder {
    /// `start_frame_index` konumundan başlayarak sürekli rawvideo RGBA pipe'ı açar.
    ///
    /// Komut bayrakları `FfmpegBridge::decode_video_window_with_fit` ile birebir uyumludur,
    /// tek farkı `-frames:v` sınırlaması olmaması ve `.stdin(Stdio::null())` atanmasıdır.
    pub fn open(
        key: StreamKey,
        pts_index: &PtsIndex,
        start_frame_index: usize,
    ) -> Result<Self, MediaError>;

    /// Açık pipe'tan bir sonraki kareyi (`self.next_cursor`) `read_exact` ile okur,
    /// `pts_index[self.next_cursor]` damgasıyla `Rgba8Frame` üretir ve imleci 1 ilerletir.
    ///
    /// Akış temiz EOF'a ulaştıysa `Ok(None)` döner ve `finished = true` işaretler;
    /// alt süreç sıfırdan farklı çıkış koduyla sonlandıysa `Err(MediaError::FfmpegFailed)` döner.
    pub fn read_next_frame(
        &mut self,
        pts_index: &PtsIndex,
    ) -> Result<Option<Rgba8Frame>, MediaError>;

    /// `count` adet ara kareyi `Rgba8Frame` tahsisi yapmadan `scratch_skip_buf` içine okuyup atlar.
    ///
    /// Tüm `count` kare okunursa `Ok(true)`, erken EOF oluşursa `Ok(false)` döner.
    pub fn skip_frames(&mut self, count: usize) -> Result<bool, MediaError>;

    /// Açık stream'in bir sonraki beklenen kare indisini döndürür.
    #[inline]
    pub fn next_cursor(&self) -> usize {
        self.next_cursor
    }

    /// Alt süreci zorla durdurur, `wait()` ile OS kaynağını toplar ve `stderr` yakalamasını döner.
    pub fn shutdown(&mut self) -> String;
}

impl Drop for StreamingVideoDecoder {
    fn drop(&mut self) {
        // Stdout reader'ı bırak, sonra kill + wait ile zombie/yetim process oluşumunu engelle.
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(handle) = self.stderr_drain.take() {
            let _ = handle.join();
        }
    }
}

/// `(path, width, height, fit)` anahtarlı sınırlı LRU açık decoder havuzu.
#[derive(Debug)]
pub struct DecoderPool {
    config: DecoderPoolConfig,
    access_seq: u64,
    streams: HashMap<StreamKey, PoolEntry>,
    lru_order: BTreeMap<u64, StreamKey>,
    telemetry: DecoderPoolTelemetry,
}

#[derive(Debug)]
struct PoolEntry {
    decoder: StreamingVideoDecoder,
    last_used_seq: u64,
}

impl DecoderPool {
    pub fn new(config: DecoderPoolConfig) -> Self;

    /// Verilen `(path, pts_index, frame_index, width, height, fit)` için kareyi üretir.
    ///
    /// Sıralı okuma ve `<= max_sequential_skip` ileri atlamada mevcut `Child` korunur;
    /// geri sarma, uzak seek veya beklenmeyen EOF durumunda stream kapatılıp yeniden açılır.
    pub fn decode_frame(
        &mut self,
        path: &Path,
        pts_index: &PtsIndex,
        target_frame_index: usize,
        width: u32,
        height: u32,
        fit: SourceFit,
    ) -> Result<Rgba8Frame, MediaError>;

    /// Belirli bir dosya yoluna ait tüm açık `Child` süreçlerini kapatır.
    pub fn invalidate_path(&mut self, path: &Path);

    /// Havuzdaki tüm açık `Child` süreçlerini kapatır ve `wait()` ile temizler.
    pub fn close_all(&mut self);

    pub fn telemetry(&self) -> DecoderPoolTelemetry;
}

impl Default for DecoderPool {
    fn default() -> Self {
        Self::new(DecoderPoolConfig::default())
    }
}

impl Drop for DecoderPool {
    fn drop(&mut self) {
        self.close_all();
    }
}
```

*(Not: `frameshift-media/src/lib.rs` dosyasına `pub mod streaming;` ve `pub use streaming::{DEFAULT_MAX_OPEN_STREAMS, DEFAULT_MAX_SEQUENTIAL_SKIP, DecoderPool, DecoderPoolConfig, DecoderPoolTelemetry, StreamKey, StreamingVideoDecoder};` eklenir.)*

---

### 3. Süreç Başlatma, İleri Atlama, Seek, EOF ve Hata Davranışı Sözleşmesi

#### 3.1. `ffmpeg` Süreç Başlatma (Spawn) Sözleşmesi
`StreamingVideoDecoder::open(key, pts_index, start_frame_index)` şu sırayı izler:
1. Boyut ve `PtsIndex` doğrulaması:
   - `width == 0 || height == 0` -> `Err(MediaError::InvalidDimensions { width, height })`.
   - `pts_index.is_empty()` veya `start_frame_index >= pts_index.len()` -> `Err(MediaError::EmptyMediaStream(...))`.
   - `frame_byte_len = width.checked_mul(height)?.checked_mul(4)?`.
2. Komut kurulumu:
   ```text
   ffmpeg -v error -nostdin [-ss <midpoint_seek_seconds>] -i <path> -fps_mode passthrough <append_fitted_output> -pix_fmt rgba -f rawvideo pipe:1
   ```
   - `.stdin(Stdio::null())` **zorunludur** (`mcp-stdio` JSON-RPC girişini korumak için).
   - `.stdout(Stdio::piped())` ve `.stderr(Stdio::piped())` ayarlanır.
   - `spawn()` başarısız olursa `Err(MediaError::BinarySpawnFailed { binary: "ffmpeg".into(), reason })`.
3. `stderr` Deadlock Koruması:
   - `child.stderr.take()` alınır ve hafif bir `std::thread::spawn` ile EOF'a kadar okunur (bellek taşmasını önlemek için örn. en fazla `64 KiB` saklanır, kalanı sessizce okunup atılır).
   - `child.stdout.take()` alınarak `BufReader::with_capacity(STREAM_READ_BUF_BYTES, stdout)` oluşturulur.

#### 3.2. `DecoderPool::decode_frame` Karar Ağacı (State Machine)

Hedef `target_frame_index` için `key = StreamKey::new(path, width, height, fit)`:

1. **Havuzda Mevcut Stream Var mı?**
   - **Durum A (`target == cursor` — Tam Sıralı Okuma):**
     - `spawns += 0`, `sequential_reads += 1`.
     - `read_next_frame` çağrılır; `cursor` `1` artırılır.
   - **Durum B (`target > cursor` ve `target - cursor <= config.max_sequential_skip` — Küçük İleri Atlama):**
     - `delta = target - cursor`.
     - `skip_frames(delta)` ile `delta` kare tek `scratch_skip_buf` (`len == frame_byte_len`) üzerine `read_exact` ile okunup atlanır (`skipped_frames += delta as u64`, 0 spawn, 0 `Rgba8Frame` allocation).
     - Ardından `read_next_frame` ile `target` karesi okunur (`cursor = target + 1`).
   - **Durum C (`target < cursor` veya `target - cursor > config.max_sequential_skip` — Geri Sarma / Uzak Seek):**
     - Mevcut entry havuzdan çıkarılır ve drop edilerek `kill() + wait()` ile kapatılır.
     - `seek_reopens += 1`.
     - `StreamingVideoDecoder::open(key, pts_index, target)` ile yeni stream açılır; ardından `read_next_frame` çağrılır (`cursor = target + 1`).
2. **Havuzda Stream Yoksa:**
   - Havuz kapasitesi doluysa (`streams.len() >= config.max_open_streams`) `lru_order` en eski erişilen anahtarı çıkarır (`lru_evictions += 1`), `PoolEntry` düşürülürken `Drop` otomatik `kill() + wait()` çalıştırır.
   - `spawns += 1`, `StreamingVideoDecoder::open(key, pts_index, target)` açılır ve `read_next_frame` çağrılır.
3. **EOF ve Kısmi Kare Hata Yönetimi (`UnexpectedEof`):**
   - `read_exact` `ErrorKind::UnexpectedEof` döndürürse stream `finished = true` işaretlenir, `eof_reaches += 1` sayılır ve `shutdown()` ile `child.wait()` çıkış kodu alınır:
     - Eğer süreç hata koduyla çıkmışsa (`!status.success()`) -> `Err(MediaError::FfmpegFailed { exit_code, stderr })`.
     - Eğer `target_frame_index < pts_index.len()` iken süreç sıfır kodla beklenenden erken EOF verdiyse (ör. `-ss` seek toleransının klip sonu karesini atlaması veya kesik dosya):
       - İlk savunma: mevcut entry havuzdan kaldırılır; eğer bu okuma önceki açık bir stream'in sonundayken olduysa (ör. B-frame reorder / seek drift) **bir kez** doğrudan `FfmpegBridge::decode_video_frame_with_fit` (veya tek kare `open(..., target)` yeniden denemesi) ile doğrulanır, yine boşsa `Err(MediaError::BufferLengthMismatch { ... })` döndürülür.
   - Bu sayede klip sonuna kadar okuma veya `-ss` kenar durumları sessizce yanlış kare üretmez ve asla zombie süreç bırakmaz.

#### 3.3. `Drop` ve Temizlik Garantisi (Windows & Unix)
- Hem `StreamingVideoDecoder::drop` hem `DecoderPool::drop` içinde deterministik kapatma garanti edilir:
  1. `BufReader<ChildStdout>` önce düşürülebilir (veya `child.kill()` doğrudan pipe'ı keser).
  2. `let _ = self.child.kill();` (`Windows`: `TerminateProcess`, `Unix`: `SIGKILL`).
  3. `let _ = self.child.wait();` (OS süreç tanıtıcısını kapatır; `ffmpeg.exe` dosya kilidini serbest bırakır — Windows'ta geçici `.mp4` test dosyalarının silinebilmesi için kritik!).
  4. `stderr_drain` thread'i `join()` edilir (pipe kapandığı için anında sonlanır).
- **Kilitli Dosya Notu (Windows):** Mevcut testlerde (`TempGuard` / `std::fs::remove_file`) bir `.mp4` dosyası açık `ffmpeg.exe` tarafından tutulurken Windows üzerinde silinemez. `DecoderPool::invalidate_path(path)` veya `Engine::clear_cache()` içinde `close_all()` çağrılması dosya kilidinin anında serbest kalmasını sağlar.

---

### 4. `MediaResolver`, `LruFrameCache` ve Seek-Independence / M1–M5 Entegrasyonu

#### 4.1. `MediaResolver` Değişikliği (`crates/frameshift-core/src/resolver.rs`)

```rust
#[derive(Debug, Default)]
pub struct MediaResolver {
    probe_cache: HashMap<String, MediaProbeInfo>,
    vector_text_cache: HashMap<(String, u32, u32), Rgba16fFrame>,
    decoder_pool: DecoderPool, // YENİ: açık streaming decoder havuzu
}
```

`MediaResolver::provide_video_frame` içindeki `ResolvedAssetSource::Disk(path)` kolu şu akışa geçer:
1. `probe = self.probe_disk_video(&asset.id, &path)?;`
2. `matched = probe.pts_index.lookup_frame(clamped_tick)?;`
3. `frame_key = matched.frame_index as u64;`
4. **`LruFrameCache` Kontrolü:** `if let Some(cached) = cache.get(&cache_asset_key, frame_key)` -> eşleşirse anında `Ok((*cached).clone())` döner (`0` pipe okuma, `0` spawn).
5. **Cache Miss:** `FfmpegBridge::decode_video_window_with_fit` yerine:
   ```rust
   let frame = self.decoder_pool.decode_frame(
       &path,
       &probe.pts_index,
       matched.frame_index,
       width,
       height,
       decode_fit,
   )?;
   cache.insert(cache_asset_key, frame_key, frame.clone());
   Ok(frame)
   ```
6. **Temizlik API'leri:**
   - `MediaResolver::clear_probe_cache(&mut self)` (veya `Engine::clear_cache` içinde çağrılan ek `close_video_streams(&mut self)`): `self.decoder_pool.close_all()` çağırır. Böylece `Engine::clear_cache()` sonrasında hem `LruFrameCache` hem açık `ffmpeg` süreçleri sıfırlanır.

#### 4.2. Motion Blur, Ters Oynatma ve M1–M5 Determinizm Garantileri

1. **Sub-Frame Temporal Motion Blur (`MotionBlurShutter`, `sub_samples = 6` vb.):**
   - `compositor.rs` ([`L1494`](../../crates/frameshift-render/src/compositor.rs#L1494)) bir çıktı karesi için `sample_ticks` alt-örneklerini sırayla ister.
   - Çoğu alt-örnek aynı kaynak `frame_index`'e veya `frame_index ± 1`'e düşer.
   - **İlk** alt-örnek miss olup `decoder_pool.decode_frame` ile çözülür ve `LruFrameCache`'e (`DEFAULT_CACHE_CAPACITY = 128`) girer.
   - Kalan `5` alt-örnek **doğrudan `LruFrameCache` hit** olur (`0` decode, `0` seek).
   - Bir sonraki çıktı karesinde geriye bakan (`trailing` veya `centered`) shutter penceresi bir önceki karenin `LruFrameCache`'te tuttuğu yakın kareleri yine bellekten alır; ileri kaydığında ise `StreamingVideoDecoder` imleci kaldığı yerden kesintisiz devam eder.
2. **Ters Oynatma (Reverse Speed Curve) Davranışı:**
   - `SpeedCurve` negatif hız verdiğinde kaynak kareler `N, N-1, N-2, ...` sırasıyla istenir.
   - Önbellek soğuksa `delta = -1` olduğu için saf streaming decoder her karede geri seek yapacaktır.
   - **Çıkarım / Gelecek İyileştirme Seçeneği:** Eğer bir `StreamKey` üzerinde ardışık `2+` geri adım (`delta < 0` ve `|delta| <= 32`) gözlenirse, `target.saturating_sub(REVERSE_PREFETCH_WINDOW - 1)` konumundan `decode_video_window_with_fit` (ör. `16` veya `32` karelik blok) ile geriye dönük bloğu `LruFrameCache`'e ön-doldurmak ters oynatmada da $16\times$–$32\times$ hızlanma sağlar. **Ancak** ilk faz `StreamingVideoDecoder` şartnamesinde bile `LruFrameCache` (`128` kare) sıcak olduğu sürece motion blur geri-örnekleri ve küçük ileri-geri jitter tamamen cache'ten karşılanır.
3. **Seek-Independence (`render-graph.md` §2) ve Bit-for-Bit Eşdeğerlik:**
   - `StreamingVideoDecoder` yalnızca kaynak medya karelerini (`Rgba8Frame`) üretir; GPU grafiği değerlendirmesi hâlâ saf `Evaluate(Graph, t, Profile)` fonksiyonudur.
   - `StreamingVideoDecoder`'ın ürettiği her `(frame_index, Rgba8Frame)` çıktısı, aynı argümanlarla (`-ss <midpoint> -i <path> -fps_mode passthrough <fit> -pix_fmt rgba -f rawvideo`) çalışan `FfmpegBridge::decode_video_frame_with_fit` çıktısıyla **byte düzeyinde birebir aynı** olmalıdır.
   - `Engine::verify_m1_golden_scene` içindeki Determinizm Kapısı (`L337-L349`) ve 5 noktalı Seek-Independence Kapısı (`L351-L381`) gerçek disk MP4 varlıkları üzerinde de doğrulanmalıdır.

---

### 5. Beklenen Performans Etkisi (Çıkarım)

- `asta_amv_project.json` (`112.56s`) üzerinde mevcut pencereli decode (`DEFAULT_DECODE_WINDOW=32`), her 32 karelik dilimde ~`0.130s` spawn+seek maliyeti ve pencere kenarı yeniden açma maliyeti ödemektedir.
- `DecoderPool` ile her aktif klip yalnızca **1 kez** (`spawns ≈ klip sayısı`) açılıp ileri doğru kesintisiz okunduğunda (`0.871s` -> `0.427s` gözlenen `2.04x` decode hızlanması ve çoklu klipte paralel pencere israfının ortadan kalkması), disk decode payı yaklaşık yarıya ve üzeri oranında düşecektir. (Kesin toplam render süresi GPU shader + encode maliyetine de bağlı olduğundan implementasyon sonrası tekrar ölçülmelidir.)

---

## Doğrulanmamış Noktalar

1. **VFR / B-Frame Reorder ve `start_pts != 0` Konteynerlerde Uzun Süreli İleri Okuma Drift Testi:**
   - `decode_video_window_with_fit` testinde 24 karelik CFR MP4 için bit-for-bit eşleşme kanıtlanmıştır ([`ffmpeg.rs`](../../crates/frameshift-media/src/ffmpeg.rs#L905-L1010)); ancak 1000+ karelik VFR (değişken kare hızlı) veya edit-list (`elst`) içeren gerçek anime kliplerinde baştan sona tek streaming pipe ile tekil `-ss` seek arasında kare kayması olup olmadığı implementasyon sırasında bir entegrasyon testiyle doğrulanmalıdır.
2. **Windows Handle ve Bellek Tavanı (`max_open_streams = 8`):**
   - Eşzamanlı 8 adet `1920x1080 -> 1280x720` `ffmpeg.exe` sürecinin tepe RAM kullanımı ve Windows altında `kill() + wait()` sonrası dosya kilidi bırakma gecikmesi çalışma anında ölçülmelidir.
3. **`asta_amv_project.json` Üzerinde Nihai Süre Kazancı:**
   - `112.56s` süresinin ne kadarının `ffmpeg` decode, ne kadarının `wgpu` shader / `encode_mp4` aşamalarından geldiği `DecoderPool` entegrasyonu sonrasında profil alınarak doğrulanmalıdır.
4. **Ters Oynatma (`SpeedCurve < 0`) Ön-Doldurma Eşiği:**
   - Saf streaming decoder ileri akış için tasarlanmıştır; ters oynatma yoğun projelerde geri-blok (`reverse window prefetch`) hibrit kuralının gerekip gerekmeyeceği gerçek ters-klip testiyle ölçülmelidir.
