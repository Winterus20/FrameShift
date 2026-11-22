---
title: "render --range: drop penceresi için aralık render'ı"
date: 2026-11-22
status: draft
tags: [cli, engine, render, audio, timebase]
---

# render --range: drop penceresi için aralık render'ı

## Soru / Amaç

20 s'lik Asta v3 timeline'ında yalnızca drop penceresini (`7.5s..9.0s`) render etmek için gereken tasarımı araştırmak: aralık gösterimi, kare seçimi, ses dilimi, MP4 zaman damgası, summary alanları, `--profile` ve gelecekteki progress ile etkileşim, hata yolları ve uygulama/doğrulama planı.

Kapsam yalnızca araştırmadır; bu görevde kaynak kod, proje dosyası veya render çıktısı değiştirilmedi, `cargo` çalıştırılmadı.

## Mevcut Kod Gerçekliği (Kanıtlı)

**CLI yüzeyi**

- `CliCommand::Render` yalnızca `project_path`, `out` ve `profile` taşıyor: `crates/frameshift-cli/src/main.rs:34-38`. Render kolu yalnızca `engine.render_video_mp4` çağırıyor: `main.rs:242-264` (çağrı 249).
- `render` parser'ı `--out`, `--profile` ve tek positional proje kabul ediyor; başka bayrak `unknown or unexpected argument` ile düşüyor: `main.rs:1009-1031` (hata 1023). `take_flag_value` hem `--flag value` hem `--flag=value` biçimini destekliyor: `main.rs:1186-1200`.
- `TimeSpec` iki biçim taşıyor: `Ticks(Flicks)` ve `FrameIndex(i64)`: `main.rs:22-27`. `parse_time_spec` `f:<index>`, `<seconds>s`, `<num>/<den>s` ve çıplak integer Flicks kabul ediyor: `main.rs:739-760` (hata metni 754-758). `resolve_time_spec` `f:` değerini proje fps'i ile Flicks'e çeviriyor: `main.rs:888-898`.
- Parser negatif değerleri reddetmiyor; `-1` çıplak integer olarak `Flicks(-1)` üretir: `main.rs:754-759`. Aralık doğrulaması bu katmanda yok. Usage'da render satırı yalnızca `--profile` içeriyor; aralık bayrağı yok: `main.rs:1210-1228` (render 1216, zaman açıklaması 1225).

**Motor**

- `render_video_mp4` → `render_video_mp4_with_options`: profil seçimi, tüm timeline, tam ses miksi, encode ve summary: `crates/frameshift-core/src/engine.rs:177-218`.
- `render_timeline_sequence`: `project.validate()`, `collect_timeline_frame_ticks`, her tick için kare ve hash: `engine.rs:220-236`. Tüm kareler `Vec<Rgba8Frame>` içinde biriktiriliyor: `engine.rs:228-234`.
- `collect_timeline_frame_ticks`: `fps.frame_to_ticks(idx)` ile kare başlangıçlarını üretir; `tick >= duration_ticks` olunca durur; sonuç boşsa hata verir: `engine.rs:629-646`.
- Tek kare `render_frame_with_profile_def` çıktının `pts` alanını tick'e eşitliyor: `engine.rs:148-173` (pts 169).

**Ses**

- `mix_project_audio` tüm `audio_clips` için `duration_ticks` uzunluğunda sessiz buffer açar, her klibi `timeline_start` ofsetiyle karıştırır ve `[-1,1]` aralığına clamp eder: `crates/frameshift-core/src/resolver.rs:538-566` (buffer 545-549, döngü 551-560, clamp 562-564).
- `AudioClipDef` yalnızca `timeline_start`, `timeline_end`, `source_start` ve statik `gain_db` taşıyor; hız eğrisi yok: `crates/frameshift-schema/src/project.rs:101-111`.
- `slice_audio_buffer(buffer, start, duration)` `[start, start+duration)` dilimini en yakın sample ile alır, kaynak biterse sessizlikle doldurur: `resolver.rs:616-651` (sample hesabı 629-635).
- `AudioBuffer::silence_for_duration` süreyi `ticks_to_audio_samples_nearest` ile örnek sayısına çevirir: `crates/frameshift-media/src/audio.rs:92-107`; `write_wav` IEEE-float WAV yazar: `audio.rs:818-852`.

**Encode**

- `encode_mp4` → `encode_mp4_internal`: en az bir kare şartı, fps kontrolü, geçici WAV, `rawvideo` + `-r fps` girişi, opsiyonel AAC ve H.264: `crates/frameshift-media/src/ffmpeg.rs:726-733` ve `759-873` (kare şartı 766-768, WAV 801-809, video girişi 813-817, ses girişi 819-821, AAC 828-829).
- Encode yalnızca `frame.data` yazıyor; `frame.pts` kullanılmıyor: `ffmpeg.rs:850-855`. Girdi akışında `-ss`, `-itsoffset`, `setpts` veya `-shortest` yok: `ffmpeg.rs:813-834`.

**Summary ve tüketiciler**

- `VideoRenderSummary`: `frame_count`, `fps`, `duration_ticks`, `frame_hashes` ve cache alanları: `crates/frameshift-core/src/report.rs:8-23`.
- `from_project`: `frame_count = frame_hashes.len()`, `fps = project.timeline.fps`, `duration_ticks = project.timeline.duration_ticks`: `report.rs:26-48` (39-41).
- MCP `render_project` aynı summary'yi döndürüyor: `crates/frameshift-mcp/src/server.rs:363-398` (video 392-396); JSON şeması 190-198.

**Sözleşmeler ve örnek proje**

- Kalıcı zaman `Flicks(i64)`; `f64` saniye yasak: `brain/specs/timebase.md:22-26`. Kare başlangıcı `frame_start_tick(n)` ve taban kare indeksi formülleri: `timebase.md:54-55`. 48 kHz'de bir örnek 14_700 tick: `timebase.md:83-87`.
- Timeline `fps`, `duration_ticks > 0`, `clips` ve `audio_clips` taşıyor: `brain/specs/project-schema.md:56-68`.
- Asta v3: timeline 60 fps, `duration_ticks = 14_112_000_000` (20 s), 1200 kare: `local_media/asta_v3/asta_v3_project.json:56-65`; ses klibi `timeline_start=0`, `timeline_end=14_112_000_000` ve `gain_db=-0.5`: `asta_v3_project.json:420-428`; drop event `target_output_tick = 5_292_000_000` (`7.5 s`): `asta_v3_project.json:432-435`.
- Render'ın seek bağımsızlığı sözleşmesi: tek kare `--tick T` ile sıralı render'ın `T` karesi aynı olmalı: `brain/specs/render-graph.md:37-39`; motor dokümanı da aynı garantiyi veriyor: `engine.rs:121-134`.

**Mevcut testler**

- CLI parse testleri tüm alt komutları ve hata yollarını kapsıyor: `main.rs:1238-1355`; başarı satırı testi 1357-1387. Core render entegrasyon testi gerçek MP4 ve `probe_media_info` kullanıyor: `crates/frameshift-core/tests/core_tests.rs:168-182`.

## İncelenen Birincil Kaynaklar

Yerel birincil kaynaklar; inceleme tarihi 2026-11-22. Web kaynağı kullanılmadı.

- `crates/frameshift-cli/src/main.rs` — `CliCommand` (22-70), `Render` kolu (242-264), `TimeSpec` parser (739-898), `parse_args` (983-1208), usage (1210-1228), testler (1231-1526).
- `crates/frameshift-core/src/engine.rs` — render yolu (148-252), `collect_timeline_frame_ticks` (629-646), doğrulama kapısı (311-412), test modülü (796-975).
- `crates/frameshift-core/src/resolver.rs` — `mix_project_audio` (538-566), `slice_audio_buffer` (616-651), `load_audio_asset` (506-536).
- `crates/frameshift-media/src/ffmpeg.rs` — `decode_audio` (672-723), `encode_mp4` (726-873), `MediaProbeInfo` (169-180).
- `crates/frameshift-media/src/audio.rs` — `silence_for_duration` (92-107), `mix_in` (177-225), `mix_tracks` (227-242), `write_wav` (818-852).
- `crates/frameshift-time/src/rational.rs` — `frame_to_ticks` (108-115), `ticks_to_frame_floor` (127-134), `ticks_to_frame_exact` (145-156).
- `crates/frameshift-time/src/flicks.rs` — `Flicks` tanımı ve `Default` (16-21).
- `crates/frameshift-schema/src/project.rs` — `AudioClipDef` (101-111), timeline doğrulaması (594-688).
- `brain/specs/timebase.md` (22-26, 41-55, 83-87) ve `brain/specs/project-schema.md` (56-68).
- `local_media/asta_v3/asta_v3_project.json` — gerçek 20 s / 60 fps / drop=`7.5 s` projesi (56-65, 420-435).

## Doğrudan Gözlemler (Kanıtlı)

1. **Aralık sözdizimi yok.** Render parser'ı tekil zaman değeri bile almıyor; yalnızca proje, çıktı ve profil var (`main.rs:1009-1031`). Buna karşın `TimeSpec` üç kullanışlı biçimi zaten çözüyor: `7.5s`, `f:450` ve `5292000000` (`main.rs:739-760`).
2. **Mevcut kare kümesi.** `collect_timeline_frame_ticks` yalnızca başlangıç tick'i `duration_ticks`'ten küçük kare indekslerini üretiyor (`engine.rs:629-646`). Asta'da 20 s × 60 fps = 1200 kare; `7.5 s` = 450. kare, `9.0 s` = 540. kare. 60 fps'te kare süresi 11_760_000 Flicks'tir (`timebase.md:50`), yani `f:450..f:540` tam 90 kare.
3. **Kare grid'i sample grid'iyle hizalı.** 60 fps kare = 11_760_000 tick; 48 kHz örnek = 14_700 tick; 11_760_000 / 14_700 = 800 örnek. 24 fps'te 29_400_000 / 14_700 = 2000 örnek. Standart fps'lerde kare sınırı tam örnek sınırıdır (`timebase.md:41-52`, 83-87).
4. **Tam mix süresi timeline süresi.** `mix_project_audio` buffer'ı `duration_ticks` ile açar (`resolver.rs:545-549`); frame grid'ine göre değil. `duration_ticks` kare sınırında değilse tam render'da ses videodan kısa kalabilir; bu kod okumasıdır, render alınmadı.
5. **Çıktı 0'dan başlar.** Encode `frame.pts`'i yok sayıp yalnızca `frame.data` yazıyor (`ffmpeg.rs:850-855`); `-r fps` ham video girişi ve 0'dan başlayan WAV (`ffmpeg.rs:813-821`) nedeniyle MP4 zaman damgası 0'dan başlar; bu kod okumasıdır, render alınmadığı için çıktıda doğrulanmadı. Aralık render'ı kendi başına 0..1,5 s olur; tam render'la eşleştirmek için `start_tick` eklenmelidir.
6. **Summary aralık bilgisi taşımıyor.** `frame_count` render edilen hash sayısı, `fps` proje fps'i, `duration_ticks` tüm timeline süresi (`report.rs:39-41`). Aralık render'ında `frame_count` doğal olarak 90 olur; `duration_ticks` yanıltıcı kalır.
7. **Bellek.** `render_timeline_sequence` tüm kareleri bellekte tutuyor (`engine.rs:228-234`). 1920×1080 RGBA8 kare 8_294_400 B'dir; 1200 kare yaklaşık 9,95 GB, 90 kare yaklaşık 746 MB eder (kare sayısı ve struct boyutundan çıkarım).
8. **Doğrulama boşluğu.** `AudioClipDef` için yalnızca `timeline_start < timeline_end` ve asset varlığı kontrol ediliyor; `timeline_end <= duration_ticks` kontrolü yok: `project.rs:671-688`. Aralık çözümü bu durumu varsaymamalı.
9. **Tam render sözleşmesi korunmalı.** `verify_m1` draft/final summary'lerinin `duration_ticks` eşitliğini karşılaştırıyor: `engine.rs:311-412` (412). Aralık API'si varsayılan yolu değiştirmemeli.
10. **Seek bağımsızlığı.** Aynı tick'teki tek kare ile sıralı render'ın karesi aynı olmalı (`engine.rs:121-134`; `render-graph.md:37-39`). Bu nedenle aralık kareleri tam render'ın aynı tick'lerdeki karelerinin alt kümesi olmalı.

## Çıkarımlar ve Öneriler

- **Tek kaynak aralık tipi.** `RenderRange { start_tick, end_tick, start_frame, end_frame }` core'da yaşasın; `end_frame` exclusive olsun. `resolve(project, start, end)` doğrulama ve snap yapsın; CLI ile MCP aynı çözümü kullansın, kopya matematik olmasın.
- **Snap kuralı.** `start_frame = fps.ticks_to_frame_floor(start)`; `end_frame = exact(end) ? end_index : floor(end) + 1`; sonra `end_frame = min(end_frame, total_frames)`. Efektif `start_tick = frame_to_ticks(start_frame)`, `end_tick = frame_to_ticks(end_frame)`. Aralık dışa doğru en fazla birer kare genişler; istenen içerik kaybolmaz, end-exclusive semantiği korunur.
- **Ses.** Efektif `[start_tick, end_tick)` için tam mix'i `slice_audio_buffer` ile dilimle; dilim süresi istenen `E-S` değil `end_tick - start_tick` olsun. Böylece ses örnek sayısı video kare süresiyle aynı olur (standart fps'lerde tam). `ffmpeg` tarafında `-shortest` gerekmez; AAC priming/padding yine ölçülmelidir.
- **Tam kapsama.** Aralık tüm timeline'ı kapsıyorsa (`start_frame == 0` ve `end_frame == total_frames`) mevcut tam render yolunu kullan; tam render çıktısı değişmesin.
- **Ses yoksa.** Aralıkla kesişen `audio_clips` yoksa `audio = None`, `has_audio = false`; sessiz AAC track ekleme. Kesişim testi `ac.timeline_start < end_tick && ac.timeline_end > start_tick`.
- **Summary.** `VideoRenderSummary`'ye `#[serde(default, skip_serializing_if = "Option::is_none")] range: Option<RenderRange>` ekle; `duration_ticks` anlamını değiştirme. `frame_hashes[i]` = `start_frame + i`; MCP JSON'u yeni opsiyonel alanı taşır.
- **CLI.** `--range <start>..<end>` birincil, `--start`/`--end` eşdeğer alias. `..=` reddedilsin. Başarı satırında efektif aralık yazılsın: `range f:450..f:540 (7.5s..9.0s)`. Saniye gösterimi yalnızca metin içindir; kalıcı şemaya f64 girmez.
- **Progress.** Gelecek progress araştırmasıyla çakışmaması için toplam, aralık kare sayısı olmalı; periyodik satırlar stderr'e, nihai özet stdout'a gitmeli. `--profile` yalnızca çözünürlük ve `sub_samples` seçer; kare grid'i timeline `fps`'inden gelir (`engine.rs:622-627`). `--range` adı `--progress` ile çakışmıyor; parser sırası render kolunda netleştirilmeli.
- **Yeni bağımlılık yok.** Tüm iş mevcut `Flicks`, `Rational32`, `AudioBuffer`, `slice_audio_buffer` ve `encode_mp4` ile yapılabilir; harici crate önerilmiyor.

## Tasarım Seçenekleri (Karar Noktaları)

### 1. CLI sözdizimi

- **A. Yalnız `--range a..b`.** En küçük yüzey, tek ayrıştırma yolu. Dezavantaj: başlangıç/bitiş ayrı değişkenlerden geliyorsa string birleştirme gerekir.
- **B. `--range` + `--start/--end` alias (öneri).** İkisi de aynı `RenderRange`'e çözülür; biri diğeriyle karışırsa hata. Yaklaşık 25 satır ek parser.
- **C. Yalnız `--start/--end`.** En açık; `--range`'in kompaktlığı ve `f:450..f:540` tek token avantajı kaybolur.
- **Net öneri: B.**
- **KARAR GEREKİYOR: CLI'da yalnız `--range` mi, yoksa `--range` + `--start/--end` alias birlikte mi sunulsun?**

### 2. Frame grid davranışı

- **A. Otomatik dışa snap (öneri).** `floor(start)`, `ceil(end)`; efektif aralık yazdırılır. İstenen içerik kaybolmaz, en fazla birer kare fazla render edilir.
- **B. Katı grid.** Kare sınırında olmayan değer hata verir; kullanıcıyı `f:` kullanmaya zorlar. Sürpriz yok ama saniye tabanlı kullanımı engeller.
- **C. En yakın kare.** Yuvarlama yönü içeriği kırpabilir; end-exclusive semantiği zayıflar.
- **Net öneri: A.** `f:` zaten kesin kontrol sağlıyor.
- **KARAR GEREKİYOR: Grid dışı değerler otomatik dışa mı snap edilsin, yoksa katı hata mı verilsin?**

### 3. Timeline dışına taşan end

- **A. End'i son geçerli kareye clamp et; start timeline dışındaysa hata ver (öneri).** Drop penceresi 20.0 s'de bitebilir; tam render zaten son karede duruyor. Efektif aralık raporlanır.
- **B. Katı hata.** Sessiz kırpma olmaz; ama `19.5s..20.5s` gibi doğal istekler reddedilir.
- **C. İkisini de clamp et.** Start timeline dışındaysa boş aralık riski doğar; önerilmez.
- **Net öneri: A.**
- **KARAR GEREKİYOR: `end > duration_ticks` sessizce son kareye clamp mi edilsin, yoksa hata mı olsun?**

### 4. Ses stratejisi

- **A. Tam mix + `slice_audio_buffer` (öneri, ilk sürüm).** Mevcut ve yeniden kullanılabilir yol; 20 s projede mix yaklaşık 7,68 MB. Uzun projelerde bellek ve CPU doğrusal büyür.
- **B. Aralık için ayrı mix.** Yalnız kesişen klipleri yükler; bellek O(aralık). `AudioBuffer::mix_tracks` (`audio.rs:227-242`) temel alınabilir; klip kesişim/ofset matematiği yeni ve test gerektirir.
- **C. Klip bazında doğrudan dilim.** `mix_project_audio` mantığını kopyalar; iki yol arasında fark riski doğar.
- **Net öneri: A** (bu görev için); 5+ dakikalık projelerde B ayrı iş.
- **KARAR GEREKİYOR: İlk sürümde tam mix + dilim mi, yoksa aralık-duyarlı mix hemen mi yazılsın?**

### 5. Summary semantiği

- **A. `duration_ticks` = timeline süresi; yeni `range` alanı (öneri).** Mevcut tüketiciler ve verify kapıları değişmez.
- **B. `duration_ticks` = render edilen süre.** Aralık için sezgisel; tam render'da genellikle aynı, ama alanın anlamı değişir ve eski tüketiciler yanlış yorumlayabilir.
- **Net öneri: A.**
- **KARAR GEREKİYOR: `duration_ticks` tüm timeline süresi olarak mı kalsın, yoksa render edilen süreye mi dönüşsün?**

### 6. MCP yüzeyi

- **A. Yalnız CLI (öneri).** Kullanıcının bildirdiği acı CLI'da; MCP aynı summary tipini paylaştığı için alan eklemesi yine uyumlu.
- **B. MCP `render_project`'e `start_tick`/`end_tick` ekle.** Agent-first hedefe uygun; şema, parametre ve test ekler.
- **Net öneri: A**; B ayrı küçük iş.
- **KARAR GEREKİYOR: `--range` yalnız CLI'da mı kalsın, yoksa MCP `render_project` de aralık alsın mı?**

### 7. Aralıkta ses kesişimi yoksa

- **A. Track'i atla (öneri).** `has_audio = false`; MP4 sessiz olur.
- **B. Sessiz AAC track ekle.** Tam render davranışına benzer; gereksiz 1,5 s sessiz track oluşur.
- **Net öneri: A.**
- **KARAR GEREKİYOR: Aralıkta ses klibi yoksa MP4 ses track'i hiç yazılmasın mı, sessiz track mi yazılsın?**

## Uygulama Planı

**0. Kararları kilitle.** Yukarıdaki 7 madde; özellikle 1, 2, 3, 4 ve 5.

**1. Core aralık tipi** — yeni `crates/frameshift-core/src/range.rs`

- `pub struct RenderRange { pub start_tick: Flicks, pub end_tick: Flicks, pub start_frame: i64, pub end_frame: i64 }` (`end_frame` exclusive).
- `impl RenderRange`: `pub fn resolve(project: &Project, start: Flicks, end: Flicks) -> Result<Self, CoreError>`; `pub fn duration_ticks(&self) -> Flicks`; `pub fn frame_count(&self) -> usize`; `pub fn covers_timeline(&self, total_frames: i64) -> bool`.
- `resolve` sırası: `start < 0` hata; `end <= start` hata; `start >= duration_ticks` hata; `total_frames = collect_timeline_frame_ticks(project)?.len() as i64`; `start_frame = fps.ticks_to_frame_floor(start)?`; `end_frame` exact/ceil; `min(total_frames)`; `end_frame <= start_frame` hata. Hatalar `CoreError::InvalidArgument` ile açık mesaj taşısın.
- `crates/frameshift-core/src/lib.rs:4-10` modül ve export satırlarına `range` / `RenderRange` ekle; `report.rs` aynı tipi kullanır.

**2. Kare toplama refactor'ü** — `crates/frameshift-core/src/engine.rs:220-236`

- `render_timeline_sequence`'ı ikiye ayır: mevcut tam yol `collect_timeline_frame_ticks` çağırır; yeni `render_timeline_sequence_for_ticks(project, profile_def, ticks: &[Flicks], options)` kare ve hash üretir. Doğrulama bir kez yapılır.
- `render_timeline_hashes` (238-252) isteğe bağlı aynı yardımcıyı kullanır.
- Aralık için `collect_timeline_frame_ticks_in_range(project, &RenderRange) -> Result<Vec<Flicks>, CoreError>`: `start_frame..end_frame` indekslerinden doğrudan `fps.frame_to_ticks` üretir; tüm collector'ı yeniden taramaz.

**3. Aralık render API'si** — `engine.rs:177-218`

- `pub fn render_video_mp4_range(&mut self, project, profile, out_path, range: RenderRange)` ve `pub fn render_video_mp4_range_with_options(..., options)` ekle; ortak private `render_video_mp4_impl(..., range: Option<&RenderRange>)`.
- `range == None`: mevcut davranış birebir korunur (tam sıra, tam mix, aynı summary).
- `range == Some`: kareler `collect_timeline_frame_ticks_in_range`; ses `mix_project_audio` + `slice_audio_buffer(range.start_tick, range.duration_ticks())`. Tam kapsama ise dilim yapma, mevcut tam mix'i kullan.
- `has_audio`: aralıkla kesişen `audio_clips` var mı (`project.rs:101-111` alanları); yoksa `None`.
- Encode çağrısı `FfmpegBridge::encode_mp4(&frames, audio.as_ref(), project.timeline.fps, out_path)` olarak kalır (`ffmpeg.rs:726-733`).

**4. Summary** — `crates/frameshift-core/src/report.rs:8-48`

- `VideoRenderSummary`'ye `#[serde(default, skip_serializing_if = "Option::is_none")] pub range: Option<RenderRange>` ekle.
- `from_project` `range: None`; yeni `from_project_range(..., range)` aynı alanları doldurur; `duration_ticks` dokunulmaz; `frame_count` hash sayısı kalır.
- `Flicks` zaten `Default` taşıyor (`flicks.rs:17-21`); `RenderRange` için `Serialize`/`Deserialize` türet.

**5. CLI** — `crates/frameshift-cli/src/main.rs`

- `CliCommand::Render` (34-38) alanlarına `range: Option<(TimeSpec, TimeSpec)>` ekle.
- `parse_args` render kolu (1009-1031): `--range`, `--start`, `--end`. `parse_range_spec(raw)`: `split_once("..")`, `..=` reddi, boş taraf reddi, ikinci `..` reddi; `--range` ile `--start/--end` karışımı hata; eksik taraf hata.
- `main` render kolu (242-264): `resolve_time_spec` ile iki ucu Flicks'e çevir; `RenderRange::resolve(&project, start, end)`; `engine.render_video_mp4_range(...)`; başarı satırı.
- `format_render_success` (664-691) imzasını bozmamak için `format_render_success_with_range(...)` ekle; mevcut fonksiyon `range: None` ile delege etsin. Satır örneği: `Rendered 1920x1080 (profile draft, 90 frames @ 60 fps, audio=true, range f:450..f:540 [7.5s..9.0s]) -> ...`.
- Usage (1216) ve zaman açıklaması (1225): `[--range <start>..<end> | --start <t> --end <t>]`, `end exclusive`, `f:` örneği.

**6. Testler**

- CLI parse: `--range 7.5s..9.0s` → `Ticks(5_292_000_000)` ve `Ticks(6_350_400_000)`; `f:450..f:540`; `--range=f:450..f:540`; `--start`/`--end`; ters, boş, `..=` ve karışık biçim reddi (`main.rs:1238-1355` düzenine ekle).
- Core saf testler (`engine.rs` `mod tests` 796+ veya yeni `range.rs` testleri): dışa snap, `start == duration` hatası, end clamp, tek kare, tam kapsama, `frame_ticks` toplamının tam collector'ın dilimiyle eşitliği.
- Ses testi: sentetik beat buffer ile dilim uzunluğu `ticks_to_audio_samples_nearest(end - start, 48_000)`; ilk örnek tam mix'teki `start` örneğiyle aynı; kesişim yoksa `has_audio = false`.
- Entegrasyon (`core_tests.rs:168-182` düzenine ekle): küçük projede `render_video_mp4` tam hash'leri ile `render_video_mp4_range(f:1..f:2)` hash'inin `full[1]`e eşitliği; `frame_count == 1`; `probe_media_info` ile `has_audio`.
- Regresyon: aralıksız `render` aynı özet/hash; `verify_m1` draft/final `duration_ticks` eşitliği (`engine.rs:412`) bozulmaz.

**7. Opsiyonel MCP** — `crates/frameshift-mcp/src/server.rs`

- `render_project` (363-398) `start_tick`/`end_tick` opsiyonel parametreleri; şema (190-198) ve parametre struct (635) güncelle; `RenderRange::resolve` + aralık API. Karar 6'ya bağlı.

**Geriye uyumluluk**

- Proje şemasına alan eklenmiyor; eski JSON'lar aynı yüklenir (`project.rs:114-123`).
- Aralıksız `render` yolu ve `duration_ticks` anlamı korunur; yeni summary alanı `serde(default)` ile eski JSON'lara uyumlu.
- Yeni crate veya dependency yok; `Cargo.toml` değişmiyor.

## Doğrulama Planı

Bu araştırmada çalıştırılmadı; uygulama sonrası çalıştırılacak komutlar:

```powershell
cargo test -p frameshift-core
cargo test -p frameshift-cli
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

```powershell
cargo run --release -p frameshift-cli -- render local_media/asta_v3/asta_v3_project.json --profile draft --range 7.5s..9.0s --out target/range_drop.mp4
ffprobe -v error -count_frames -select_streams v:0 -show_entries stream=nb_read_frames,r_frame_rate,duration -of default=nw=1 target/range_drop.mp4
ffprobe -v error -show_entries format=duration -show_entries stream=codec_type,duration -of json target/range_drop.mp4
```

Kabul kriterleri:

- Video 90 kare ve 60 fps; video süresi 1,5 s.
- Ses stream'i varsa süresi video süresinden en fazla tipik bir AAC frame (yaklaşık 21 ms) sapar; container süresi 1,5 s'e yakın olur. Bu araştırmada render alınmadı; ölçüm yapılmadı.
- Aralık karesinin hash'i tam render'ın aynı tick'teki karesiyle aynı olur (seek bağımsızlığı: `render-graph.md:37-39`).
- Aralıksız render ve `verify-all` önceki sonuçları korur.
- Ters/boş aralık, timeline dışı start, `..=` ve karışık bayrak hataları sıfır olmayan çıkış ve açık mesaj verir.
- `cargo clippy` 0 uyarı; yeni dependency yok.
- 20 s tam render'a göre beklenen süre oranı 90/1200 = %7,5'tir; bu bir tahmindir, benchmark değildir.

## Riskler ve Doğrulanmamış Noktalar

- **AAC priming/padding:** `encode_mp4` WAV'ı olduğu gibi mux ediyor, `-shortest` yok (`ffmpeg.rs:819-834`). Ses frame grid'ine hizalansa da AAC encoder priming nedeniyle container süresi birkaç ms sapabilir; render alınmadığı için doğrulanmadı.
- **Bellek:** Aralık, `Vec<Rgba8Frame>` tepe belleğini küçültür ama tam render yolu hâlâ tüm kareleri tutuyor (`engine.rs:228-234`). 1080p'de 1200 kare yaklaşık 9,95 GB'tır; bu, aralık dışı ayrı bir darboğazdır.
- **Snap genişlemesi:** Aralık dışa en fazla birer kare taşar; drop öncesi bir kare de çıkabilir. `--range` inceleme aracıdır, final kesim aracı değildir.
- **Frame-aligned olmayan duration:** `duration_ticks` kare sınırında değilse tam render'da ses videodan kısa kalabilir (`resolver.rs:545-549`); aralık çözümü end'i kare grid'ine clamp ettiği için fark büyüyebilir. Somut ffprobe ölçümü yapılmadı.
- **Doğrulama boşluğu:** `timeline_end > duration_ticks` olan ses klipleri doğrulanmıyor (`project.rs:671-688`); tam mix bu kısmı kırpar, aralık dilimi sessizlikle doldurur. Mevcut davranış korunur, bu araştırmada düzeltilmez.
- **MCP uyumu:** `VideoRenderSummary`'ye opsiyonel alan eklemek Rust `Deserialize` için `serde(default)` ile uyumlu; harici JSON tüketicileri ek alanı tolere etmelidir. MCP tarafı bu araştırmada değiştirilmiyor.
- **Progress çakışması:** Progress araştırması henüz kodda yok; `--range` toplam kare sayısını değiştireceği için progress tasarımı aralık farkındalığıyla yapılmalı. Bu araştırmada progress davranışı doğrulanmadı.
- **Non-standard fps:** `frame_to_ticks` floor kullanıyor (`rational.rs:108-115`); efektif süre `N/fps`'ten 1 tick'ten az sapabilir. 1 tick = 1/705_600_000 s'dir; pratikte önemsiz ama tam eşitlik iddiası değildir.
- **Web kaynağı kullanılmadı:** FFmpeg davranışı yalnızca yerel `ffmpeg.rs` kodundan okundu; resmi FFmpeg belgesi veya ampirik render bu görevde incelenmedi.
