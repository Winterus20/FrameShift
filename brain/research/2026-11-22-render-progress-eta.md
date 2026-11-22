---
title: Render ilerleme/ETA — sessiz 1200 kare döngüsüne gözlemlenebilirlik
date: 2026-11-22
status: draft
tags: [cli, render, observability, ux, agent]
---

# Render ilerleme/ETA — sessiz 1200 kare döngüsüne gözlemlenebilirlik

## Soru / Amaç

Uzun render sırasında kullanıcıya geri bildirim vermek için kesin teknik sorular:

1. İlerleme satırı nereye yazılmalı: stdout (makine tarafından ayrıştırılan sonuç) mu, stderr mi? Varsayılan açık mı, `--progress`/`--quiet` nasıl davranmalı, TTY ve CI'da fark ne olmalı?
2. Satır biçimi ve ETA hesabı: `frame 600/1200 | 3.2 fps | ETA 3:10` örneği temel alınabilir mi; EMA, warm-up ve süre biçimi nasıl tanımlanmalı?
3. Mimari: ilerleme motorun içinden callback/observer ile mi, yalnızca CLI'da mı üretilmeli? Kütüphane sessiz kalırken callback imzası ve thread güvenliği ne olmalı?
4. Hangi aşamalar raporlanmalı: decode, render, encode? Encode ayrı bir ffmpeg süreci olduğu için kare döngüsünden ayrılmalı mı?
5. `sheet`/`probe`/`render --range` ile ortak bir ilerleme arayüzü nasıl kurulur?
6. Test edilebilirlik: saf formatlayıcı, sahte saat ve determinist çıktı nasıl sağlanır?

Kapsam yalnızca araştırmadır; bu belge hiçbir kaynak dosyayı değiştirmez.

## Mevcut Kod Gerçekliği (Kanıtlı)

- **CLI render yolu tek bir son satır basıyor.** `render` kolu projeyi yükler, motoru kurar, `render_video_mp4` çağırır ve ancak dönüşten sonra `format_render_success` çıktısını `println!` ile yazar: [main.rs:242](../../crates/frameshift-cli/src/main.rs#L242-L264). Render başlamadan önce stdout/stderr'e tek bayt yazılmıyor.
- **Kare döngüsü tamamen sessiz.** `render_timeline_sequence` tick listesini alır, `Vec::with_capacity(ticks.len())` ayırır ve her kareyi döngüde render edip hash'ler; döngü gövdesinde hiçbir konsol çıktısı yok: [engine.rs:220](../../crates/frameshift-core/src/engine.rs#L220-L236).
- **Render bitmeden encode başlamıyor.** `render_video_mp4_with_options` önce tüm kareleri `render_timeline_sequence` ile üretir (satır 199), sonra sesi miksler (satır 200), sonra `FfmpegBridge::encode_mp4` çağırır (satır 208): [engine.rs:191](../../crates/frameshift-core/src/engine.rs#L191-L218).
- **Kareler bellekte birikiyordu (P0 öncesi).** `Vec<Rgba8Frame>` ve `Rgba8Frame.data: Vec<u8>` birlikte tüm diziyi tutuyor: [engine.rs:228](../../crates/frameshift-core/src/engine.rs#L228-L234), [frame.rs:19](../../crates/frameshift-media/src/frame.rs#L19-L26). 1920×1080×4 bayt × 1200 kare ≈ **9.3 GiB** (aritmetik; ölçüm değil). Asta v3 gerçek çıktısı 1200 kare / 1920×1080: [asta-v3 log:13](../../brain/log/2026-11-22-asta-v3-red-line.md#L13).

> **P0 sonrası not (2026-11-22):** Yukarıdaki "kareler bellekte birikiyor" ifadesi artık geçerli değil; P0 streaming encode O(N) kare tamponunu kaldırdı. Tepe bellek projeye/çözünürlüğe göre **0.27–0.80 GB working set** aralığında; baskın terim 128 kare / 1 GiB LRU kaynak önbelleği. Ölçümler (Fame v3 720p 0.27 GB; asta_amv draft 720p 0.80 GB; asta_v3 draft 640×360 0.40 GB) ve ayrıntı: [takım günlüğü](../log/2026-11-22-asta-v3-team.md).
- **Encode ayrı bir ffmpeg süreci ve sessiz.** `encode_mp4` → `encode_mp4_internal`: ffmpeg `-v error` ile başlatılıyor, stdout `null`, stderr pipe'a bağlı; kareler stdin'e tek tek yazılıyor: [ffmpeg.rs:726](../../crates/frameshift-media/src/ffmpeg.rs#L726-L733), [ffmpeg.rs:813](../../crates/frameshift-media/src/ffmpeg.rs#L813-L837), [ffmpeg.rs:844](../../crates/frameshift-media/src/ffmpeg.rs#L844-L860).
- **Toplam kare sayısı tek yerden geliyor.** `collect_timeline_frame_ticks` fps ve `duration_ticks` üzerinden 0'dan başlayıp `duration_ticks` sınırına kadar tick üretiyor; aralık parametresi yok: [engine.rs:629](../../crates/frameshift-core/src/engine.rs#L629-L646).
- **Decode ayrı bir aşama değil, kare içinde örtük.** Her kare için `ProjectFrameProvider` kuruluyor ve compositor kaynak kareleri bu sağlayıcıdan çekiyor: [engine.rs:155](../../crates/frameshift-core/src/engine.rs#L155-L165), [resolver.rs:711](../../crates/frameshift-core/src/resolver.rs#L711-L729), [compositor.rs:1413](../../crates/frameshift-render/src/compositor.rs#L1413-L1435). Yani kare süresi decode + GPU render'ı birlikte içeriyor.
- **Parser elle yazılmış; clap/indicatif yok.** `render` bayrakları `project_path`, `--out`, `--profile` ile sınırlı; bilinmeyen `-` argümanı hata veriyor: [main.rs:1009](../../crates/frameshift-cli/src/main.rs#L1009-L1031). CLI bağımlılıkları yalnızca `anyhow`, `serde`, `serde_json` ve workspace crate'leri: [frameshift-cli/Cargo.toml:8](../../crates/frameshift-cli/Cargo.toml#L8-L17).
- **stdout sonuç kanalı.** Başarı satırı stdout'a gidiyor; `peaks` verisi ve `verify-all` markdown'u da stdout'ta: [main.rs:250](../../crates/frameshift-cli/src/main.rs#L250-L263), [main.rs:386](../../crates/frameshift-cli/src/main.rs#L386), [main.rs:617](../../crates/frameshift-cli/src/main.rs#L617). MCP sunucusu JSON-RPC yanıtlarını stdout'a yazıyor: [server.rs:453](../../crates/frameshift-mcp/src/server.rs#L453-L462).
- **Render hattında geçen süre ölçümü yok.** `Instant` crate genelinde hiç kullanılmıyor; tek `std::time` kullanımı LUT cache anahtarı için dosya mtime'ı: [compositor.rs:5767](../../crates/frameshift-render/src/compositor.rs#L5767-L5778). İlerleme altyapısı yok; tek yüzde alanı GUI iş modelinde: [jobs.rs:29](../../crates/frameshift-app/src/jobs.rs#L29-L35), [jobs.rs:153](../../crates/frameshift-app/src/jobs.rs#L153-L163).
- **Başarı satırının biçimi testle kilitli.** `format_render_success` saf fonksiyon ve testi satırdaki `fps` sayısının 1 olduğunu doğruluyor: [main.rs:665](../../crates/frameshift-cli/src/main.rs#L665-L691), [main.rs:1358](../../crates/frameshift-cli/src/main.rs#L1358-L1387).
- **Spec'lerde ilerleme sözleşmesi yok.** `brain/specs/` içinde ilerleme/stdout/stderr kuralı geçmiyor; yalnızca shader `progress` parametresi var. Şema değişikliği gerekmiyor; bu iş kalıcı proje zamanına dokunmuyor ([timebase.md:24](../specs/timebase.md#L24-L26)).

## İncelenen Birincil Kaynaklar

- `AGENTS.md` — çalışma sınırları, komutlar, "çalıştırılmamış özelliği doğrulandı deme" kuralı.
- `brain/research/README.md` — birincil kaynak ve gözlem/çıkarım ayrımı standardı.
- `brain/_templates/arastirma.md` — rapor iskeleti.
- `brain/research/2026-11-22-asta-v3-uretken-darbogazlari.md` — eksik tanımı (satır 19) ve istenen biçim (satır 40).
- `brain/research/2026-11-22-kinetic-typography-gap-analysis.md:101` — aynı eksik için ikinci bağımsız gözlem.
- `brain/log/2026-11-22-asta-v3-red-line.md:69-72` — gerçek render komutu, 1200 kare, draft/final çıktıları.
- `brain/specs/project-schema.md`, `brain/specs/timebase.md` — kalıcı zaman kuralı ve serde default uyumluluğu.
- `crates/frameshift-core/src/engine.rs`, `crates/frameshift-core/src/report.rs`, `crates/frameshift-core/src/resolver.rs`.
- `crates/frameshift-cli/src/main.rs`, `crates/frameshift-cli/Cargo.toml`.
- `crates/frameshift-media/src/ffmpeg.rs`, `crates/frameshift-media/src/frame.rs`.
- `crates/frameshift-mcp/src/server.rs`, `crates/frameshift-app/src/jobs.rs`.
- [std::io::IsTerminal](https://doc.rust-lang.org/std/io/trait.IsTerminal.html) — erişim: 2026-11-22; trait 1.70.0'da stabil, harici crate gerekmiyor.
- [std::io::stderr](https://doc.rust-lang.org/std/io/fn.stderr.html) — erişim: 2026-11-22; "This handle is not buffered."
- [std::time::Instant](https://doc.rust-lang.org/std/time/struct.Instant.html) — erişim: 2026-11-22; monotonik ama "not guaranteed to be steady".

## Doğrudan Gözlemler (Kanıtlı)

- 1200 karelik bir render'da süreç başlangıcı ile başarı satırı arasında stdout/stderr'e **hiçbir bayt** üretilmiyor; kod yolunda arada `println!`/`eprintln!` yok ([main.rs:242](../../crates/frameshift-cli/src/main.rs#L242-L264), [engine.rs:220](../../crates/frameshift-core/src/engine.rs#L220-L236)).
- Kullanıcı gerçek üretimde bunu yaşadı ve CPU'ya bakarak bekledi; istenen biçim açıkça `frame 600/1200, 3.2 fps, ETA 3:10` ([asta raporu:19](../../brain/research/2026-11-22-asta-v3-uretken-darbogazlari.md#L19), [asta raporu:40](../../brain/research/2026-11-22-asta-v3-uretken-darbogazlari.md#L40)).
- Kare döngüsü bittikten sonra encode ayrı ve sessiz bir aşama; encode sırasında "100%" görünen bir sayaç, kullanıcıya donmuş gibi görünür ([engine.rs:199](../../crates/frameshift-core/src/engine.rs#L199-L208), [ffmpeg.rs:844](../../crates/frameshift-media/src/ffmpeg.rs#L844-L860)).
- 600/1200 kare ve 3.2 fps sabit hız varsayımıyla kalan süre 600 / 3.2 = 187.5 sn → **3:08**; belgedeki "3:10" örneği yuvarlanmış/illüstratif bir değer, formül değil.
- Toplam kare sayısı render öncesi kesin olarak biliniyor (`collect_timeline_frame_ticks`), yani ETA paydası için tahmin gerekmiyor ([engine.rs:629](../../crates/frameshift-core/src/engine.rs#L629-L646)).
- Decode ayrı sayılamaz: kare süresi decode'u da kapsıyor; ayrı "decode %" göstergesi mevcut mimaride yanıltıcı olur ([resolver.rs:767](../../crates/frameshift-core/src/resolver.rs#L767-L774)).
- MCP modunda motor çıktısı stdout'a yazılsaydı JSON-RPC akışı bozulurdu; kütüphanenin hiçbir koşulda yazmaması gerekiyor ([server.rs:453](../../crates/frameshift-mcp/src/server.rs#L453-L462)).
- `std::io::IsTerminal` ve `Instant` standart kütüphanede; yeni bağımlılık olmadan TTY algılama ve süre ölçümü mümkün.
- Mevcut `render` alt komutu `--help` bile kabul etmiyor (bilinmeyen bayrak hatası); yeni bayraklar hem parser'a hem global usage metnine eklenmeli ([main.rs:1210](../../crates/frameshift-cli/src/main.rs#L1210-L1228)).

## Çıkarımlar ve Öneriler

- **Çıktı yeri stderr olmalı.** stdout sözleşmesi (tek başarı satırı, MCP JSON-RPC) değişmemeli. stderr tamponsuz olduğu için `\r` ile yerinde güncelleme doğrudan çalışır.
- **Varsayılan auto olmalı:** yalnızca `stderr` bir TTY ise açık; `--progress` TTY olmasa da zorla açar; `--quiet` kapatır. CI varsayılan olarak sessiz kalır, böylece log kirliliği ve yanlış ETA gürültüsü olmaz.
- **Motor sessiz kalmalı, ama olay üretmeli.** Kütüphane biçimlendirmez/yazmaz; yalnızca `RenderProgress { stage, completed, total }` yayınlar. Biçim, ETA ve yazma CLI'ya aittir.
- **ETA EMA ile hesaplanmalı, ilk 10 kare warm-up sayılmalı.** İlk karelerde shader pipeline kurulumu ve soğuk cache nedeniyle süre yüksektir; bu kareler ortalamaya katılırsa ETA sürekli yukarı sapar.
- **Süre biçimi sabit olmalı:** `M:SS`, 1 saatten sonra `H:MM:SS`, saniye yukarı yuvarlanır. Satır biçimi tek regex ile ayrıştırılabilir kalmalı: `^frame \d+/\d+ \| [0-9.]+ fps \| ETA \d+:\d{2}$`.
- **Aşamalar:** `Frames` (decode+render), `AudioMix` (kısa), `Encode` (ffmpeg). Kare döngüsü dışındaki aşamalar işaretsiz geçmemeli; encode sırasında "donmuş %100" algısı oluşmamalı.
- **Encode için kare başına ilerleme şart değil.** Faz 1'de yalnızca `encoding` işareti + geçen süre yeterli; per-frame encode ilerlemesi `frameshift-media` API'sini büyütür.
- **Ortak arayüz şimdi kurulmalı.** `sheet`/`probe`/`render --range` henüz yok; aynı `RenderProgress` tipi ve aynı CLI reporter'ı ile ileride kare sayacı doğrudan yeniden kullanılır. `--range` geldiğinde payda çağrıdaki kare sayısı, metinde ayrıca mutlak başlangıç indeksi olmalı.
- **Yeni bağımlılık önerilmiyor.** `indicatif`/renkli çıktı istenirse bu bir dış crate kararıdır: **KULLANICI ONAYI GEREKLİ**.
- **Kalıcı şemaya dokunulmuyor.** İlerleme geçicidir; proje JSON'u, `Flicks` zamanı ve serde default uyumluluğu değişmez.

## Tasarım Seçenekleri (Karar Noktaları)

### 1. Çıktı akışı ve varsayılan

| # | Seçenek | Tradeoff |
|---|---|---|
| 1 | **stderr + auto(TTY)** | stdout sözleşmesi korunur; pipe/CI sessiz. TTY algısı platforma bağlı. |
| 2 | stdout + auto | Kullanıcı görür ama başarı satırı parse eden scriptler ve MCP yolu risk altında. |
| 3 | stderr + her zaman açık | En basit zihinsel model; CI logları ve kısa render'lar gürültülü. |

**Öneri: 1.** `std::io::IsTerminal` ile `stderr` kontrolü; `--progress` zorla açar, `--quiet` kapatır.

KARAR GEREKİYOR: İlerleme stderr'e mi yazılsın (öneri), yoksa stdout'a mı?
KARAR GEREKİYOR: Varsayılan auto(TTY) mı, her zaman açık mı, yoksa yalnızca `--progress` ile mi?
KARAR GEREKİYOR: `--quiet` yalnızca ilerlemeyi mi kapatsın (öneri), yoksa başarı satırını da mı?

### 2. Mimari: callback mi, CLI sayacı mı?

| # | Seçenek | Tradeoff |
|---|---|---|
| 1 | **Engine'e `FnMut(RenderProgress)` parametreli yeni metot** | Tek doğru sayaç; kütüphane sessiz; mevcut imzalar korunur. Public API bir metot büyür. |
| 2 | Yalnızca CLI'da süre ölçümü | Sıfır motor değişikliği; ama kare ilerlemesi bilinmez, yalnızca "toplam süre" ölçülür. |
| 3 | Engine üzerinde kalıcı `progress_sink` alanı | Çağrı başına parametre yok; gizli durum, `'static`/borrow sorunları ve test edilebilirlik kaybı. |

**Öneri: 1.** Yeni metot `render_video_mp4_with_progress(project, profile, out_path, options, on_progress)`; mevcut `render_video_mp4`/`render_video_mp4_with_options` no-op closure ile delege eder. İç döngü `&mut dyn FnMut(RenderProgress)` alır (monomorfizasyon yok).

KARAR GEREKİYOR: Motor API'si callback parametreli yeni metot mu (öneri), yoksa Engine üzerinde kalıcı sink alanı mı?

### 3. Aşama kapsamı ve encode derinliği

| # | Seçenek | Tradeoff |
|---|---|---|
| 1 | **Frames + AudioMix + Encode işareti** | Doğru zihinsel model; encode sırasında donmuş görünmez. Encode ilerlemesi kaba. |
| 2 | Yalnızca Frames | En küçük değişiklik; encode sırasında sayaç 100%'de donar. |
| 3 | Frames + decode + audio + encode per-frame | En ayrıntılı; decode ayrı ölçülemediği için yanıltıcı ve media API'sinde büyük değişiklik. |

**Öneri: 1.** Encode için `frameshift-media` API'sine callback eklenmez; yalnızca aşama başlangıcı bildirilir.

KARAR GEREKİYOR: Aşamalar yalnızca kare döngüsü + encode işareti mi (öneri), yoksa decode/audio ayrı aşamalar mı?
KARAR GEREKİYOR: Encode sırasında kare başına ilerleme (media API değişikliği) mi, yoksa yalnızca `encoding` işareti mi (öneri)?

### 4. `--range` paydası ve başarı satırı

| # | Seçenek | Tradeoff |
|---|---|---|
| 1 | **Payda çağrıdaki kare sayısı; metinde mutlak başlangıç** | ETA doğru; kullanıcı "hangi aralık" bilgisini ayrıca görür. Biçim bir alan büyür. |
| 2 | Mutlak zaman çizgisi indeksi | Tek bakışta konum; ETA paydası yanlış olur (600/1200 gibi). |

**Öneri: 1.** Şimdilik `--range` olmadığı için biçim değişmez; alan `--range` ile birlikte eklenir. Başarı satırına süre eklenmesi bu turda önerilmez (stdout sözleşmesi).

KARAR GEREKİYOR: `render --range` geldiğinde payda çağrıya göre mi (öneri), yoksa zaman çizgisi mutlak indeksi mi?
KARAR GEREKİYOR: Başarı satırına toplam süre eklensin mi? (öneri: hayır, bu turda)

## Uygulama Planı

1. **Yeni tip: `crates/frameshift-core/src/progress.rs`.** `RenderStage { Frames, AudioMix, Encode }` ve `RenderProgress { stage, completed: usize, total: usize }`; `Debug + Clone + Copy + PartialEq + Eq`. `lib.rs:10` yanına `pub mod progress;`, `lib.rs:33-36` re-export bloğuna tipleri ekle. Kalıcı şema değil; `serde` türetme yok.
2. **Engine: sessiz döngüyü callback'li hale getir.** `engine.rs:220-236` yanına `render_timeline_sequence_with_progress(..., on_progress: &mut dyn FnMut(RenderProgress))` ekle; döngüden önce `Frames { 0, total }`, her kareden sonra `Frames { i+1, total }` yayınla. Mevcut `render_timeline_sequence` no-op closure ile delege etsin; `render_timeline_hashes` (238-252) değişmesin.
3. **Engine: yeni public metot.** `engine.rs:191-218` yanına `render_video_mp4_with_progress(..., options, on_progress)` ekle; satır 199'da callback'li döngüyü, satır 200 öncesi/sonrası `AudioMix { 0,1 }`, satır 208 öncesi `Encode { 0,1 }` yayınla. Mevcut `render_video_mp4_with_options` bunu `|_| {}` ile çağırsın.
4. **CLI: yeni modül `crates/frameshift-cli/src/progress.rs`.** `ProgressMode { Auto, Always, Never }`; `EtaEstimator` (warm-up 10 kare, EMA alpha 0.1); `ProgressState` (saf; `elapsed: Duration` alır, `Option<ProgressLine>` döndürür); `ProgressReporter<W: Write>` (`Instant` yalnızca burada); `format_progress_line`, `format_duration`. TTY'de `\r` + boşlukla eski satırı silme, non-TTY'de `\n` ve `max(1, total/100)` karelik seyrek kayıt.
5. **CLI: bayraklar ve bağlantı.** `main.rs:34-38` `Render` varyantına `progress: ProgressMode` ekle; `main.rs:1009-1031` parser'ında `--progress`/`--quiet` (ikisi birlikte hata); `main.rs:242-264` kolunda reporter'ı kur, `render_video_mp4_with_progress` çağır, `finish()` sonrası mevcut başarı satırını yaz; `main.rs:1216` usage satırını güncelle.
6. **Testler.**
   - `progress.rs`: sabit 100 ms karelerle 600/1200 → `ETA 1:00`; warm-up'ta satır yok; süre biçimi sınırları (59.9 → `1:00`, 3599 → `59:59`, 3600 → `1:00:00`); TTY redraw / non-TTY newline; throttle (0, 100, 300 ms).
   - `main.rs`: `--progress` parse, `--quiet` ile çakışma hatası, varsayılan `Auto`; mevcut `parses_all_subcommands` (1238-1328) beklenen değerleri güncelle.
   - `core_tests.rs`: mevcut 30 karelik fixture (168-176) ile olay listesi; ilk `(0,30)`, son `(30,30)`, `Encode` olayı, toplam olay sayısı 30 + 3.
7. **Geriye uyumluluk.** Proje JSON'u ve serde default yolu değişmez; `render_video_mp4`/`render_video_mp4_with_options` imzaları aynı kalır; stdout bayt bayt aynı (mevcut `fps` sayısı testi korunur); MCP yolu callback'siz çağırmaya devam eder; yeni bayraklar isteğe bağlı, bilinmeyen bayrak davranışı değişmez; yeni crate yok.

## Doğrulama Planı

```powershell
cargo test -p frameshift-cli -p frameshift-core
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run --release -p frameshift-cli -- render local_media/asta_v3/asta_v3_project.json --out target/progress_check.mp4 --profile draft --progress
cargo run --release -p frameshift-cli -- render local_media/asta_v3/asta_v3_project.json --out target/progress_check.mp4 --profile draft --progress 2> target/progress.txt 1> target/success.txt
```

Kabul kriterleri:

| Kriter | Ölçüm |
|---|---|
| stdout değişmedi | `success.txt` tek satır; eski `Rendered ... -> ...` biçimi; `fps` bir kez. |
| stderr satırları ayrıştırılabilir | Her satır `^frame \d+/\d+ \| [0-9.]+ fps \| ETA \d+:\d{2}$`. |
| Non-TTY hacmi | 1200 karede ~100 satır (12 karelik adım); 90-130 aralığı. |
| TTY'de yerinde güncelleme | Elle: aynı satır `\r` ile yenilenir, son satır silinmez. |
| Sessiz varsayılan | `--progress` yokken yönlendirilen stderr 0 bayt. |
| ETA doğruluğu | Son çeyrekte gerçek süreye göre ±%25 (yumuşak kontrol; ilk render varyansı). |
| Regresyon | Mevcut test paketi (`brain/state.md`: 199) + yeni testler geçer; `verify-all` çıktısı değişmez. |

## Riskler ve Doğrulanmamış Noktalar

- **ETA sapması:** İlk karelerde pipeline kurulumu ve soğuk cache süreyi şişirir; 10 kare warm-up + EMA bunu azaltır ama garanti etmez. Sahneler arası cache isabet oranı değişirse ETA dalgalanır.
- **`Instant` "steady" değil:** Resmî doküman monotonik olduğunu, tik uzunluğunun sabit olmadığını söylüyor; ETA yaklaşıklıktır, garanti değil.
- **Encode görünürlüğü:** Faz 1'de yalnızca işaret var; ffmpeg pipe'ı uzun süre bloklanırsa ekran yine "donmuş" görünebilir. Per-frame encode ilerlemesi bilinçli olarak kapsam dışı.
- **Bellek (P0 öncesi):** 1200×1080p kareler ~9.3 GiB tutuyordu; P0 streaming encode ile O(N) tampon kaldırıldı (yukarıdaki P0 sonrası nota bkz.). İlerleme bilgisi tek başına bellek sorununu çözmez; asıl düzeltme P0'dır.
- **Windows konsol farkları:** `\r` eski conhost'ta satır uzunluğu değişince artık bırakabilir; boşlukla silme ve sabit genişlikli alanlar gerekir. Windows Terminal'de beklenen davranış doğrulanmadı.
- **Detached console:** Rust dokümanı, ilişik konsolu olmayan süreçte stderr yazımlarının sessizce başarılı olduğunu söylüyor; ilerleme kaybolur.
- **CI gürültüsü:** Varsayılan auto CI'da kapalı; `--progress` ile açılırsa 1200 satıra kadar çıkabilir. Adım `total/100` ile sınırlı.
- **Kapsam dışı kalanlar:** `inspect`/`verify-all` ilerlemesi, `sheet`/`probe`/`--range` komutları ve MCP ilerleme bildirimi bu planda yok; ortak tip onları destekler ama wire edilmedi.
- **Doğrulanmadı:** Gerçek makinede kare süresi varyansı, encode'un toplam süredeki payı, 4 Hz throttle'ın okunabilirliği ve eski Windows konsolunda `\r` davranışı. Bu araştırmada hiçbir komut çalıştırılmadı.
- **Onay gerektiren sapma:** `indicatif` veya benzeri bir crate, renkli/spinner çıktı ya da AGENTS/README'ye bağlayıcı ilerleme sözleşmesi eklenmesi kullanıcı onayı ister (**KULLANICI ONAYI GEREKLİ**).
