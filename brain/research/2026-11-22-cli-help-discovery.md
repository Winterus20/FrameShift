---
title: CLI keşfedilebilirlik — --help, komut listesi ve doküman senkronu
date: 2026-11-22
status: draft
tags: [cli, ux, docs, agent, help]
---

# CLI keşfedilebilirlik — --help, komut listesi ve doküman senkronu

## Soru / Amaç

Asta v3 oturumunda `frame --help` hata verdi ve `peaks` komutu AGENTS.md listesinde olmadığı için geç keşfedildi ([asta-v3:32-34](2026-11-22-asta-v3-uretken-darbogazlari.md)). Bu rapor şu soruları yanıtlar:

1. `frame --help` neden exit 1 ile bitiyor; hangi kod yolu hatayı üretiyor?
2. Global yardım ile alt komut yardımı nasıl ayrılmalı: tek `print_usage` mi, komut başına `print_<cmd>_usage` mi, yoksa tek bir komut tablosu mu?
3. `--help` her konumda exit 0 ile çalışmalı mı; `help <command>` ve `--version` nasıl eklenir?
4. AGENTS.md komut listesi ile CLI usage metni nasıl senkron kalır; `peaks` neden eksik?
5. README ve `brain/` dokümanlarında "hangi komut ne yapar" tablosu hangi içerikle, nerede durmalı?
6. `inspect` zaten 3x2 contact sheet üretiyorsa keşfedilebilirlik eksiği tam olarak nerede?

Kapsam yalnızca araştırmadır; bu belge hiçbir kaynak dosyayı değiştirmez, `cargo`/render/git çalıştırmaz. `probe`/`sheet`/`autocut` bu turda eklenmez; yalnız mevcut 9 komutun yardım yüzeyi ele alınır.

## Mevcut Kod Gerçekliği (Kanıtlı)

- **Yardım yalnızca ilk token'da tanınıyor.** `parse_args` ilk argümanı ayırır; `-h`/`--help`/`help` ise `CliCommand::Help` döner ve kalan argümanları hiç okumaz: `crates/frameshift-cli/src/main.rs:983-990`.
- **`Help` konu taşımıyor.** Birim varyant; hangi komutun yardımı istendiğini saklayacak alan yok: `main.rs:30-70`, özellikle `main.rs:69`.
- **Her alt komut kendi bayrak kontrolünü yazıyor.** 9 kolun çoğu `while i < rest.len()` döngüsü kurar (`mcp-stdio` yalnız boşluk kontrolü yapar); tanınmayan `-` token `bail!` eder. `frame` kolu `main.rs:1032-1060`, hata satırı `main.rs:1050-1052`. Aynı desen: `init-golden` (`993-1008`), `render` (`1009-1031`), `verify-m1` (`1061-1074`), `inspect` (`1075-1093`), `ui-snapshot` (`1094-1119`), `peaks` (`1120-1158`), `mcp-stdio` (`1159-1164`), `verify-all` (`1165-1181`).
- **Global usage tek fonksiyon ve güncel.** `print_usage` 19 satırlık tek `println!`; 9 komutun tamamını, `peaks` dahil, listeliyor: `main.rs:1210-1228` (`peaks` satırı `1221`).
- **`main` yardımda `Ok(())` döndürüyor.** `CliCommand::Help` kolu `print_usage()` çağırıp `Ok(())` döner: `main.rs:221-229`. Hata yolları `anyhow` üzerinden `Err` döner.
- **`--version` yok.** `crates/frameshift-cli` içinde `CARGO_PKG_VERSION` veya `--version` geçmiyor (grep: 0 eşleşme). `--version` ilk token olduğu için `other =>` dalına düşer: `main.rs:1182`.
- **Yardım testi yok.** Test modülü `main.rs:1230-1526`; `Help`/`print_usage`/`--help` hiçbir testte geçmiyor. Mevcut testler mutlu yol ayrıştırması ve eksik argüman reddi: `main.rs:1238-1355`.
- **AGENTS.md listesi 8 komut, CLI 9.** Liste `peaks` içermiyor: `AGENTS.md:39`. Aynı bölüm "9-Crate Workspace" başlığını taşıyor: `AGENTS.md:30`.
- **AGENTS.md'de test sayısı da eski.** `58/58` yazıyor (`AGENTS.md:5`); `brain/state.md:7` son durumu `199/199` olarak kaydediyor. Komut listesinden bağımsız ikinci bir bayatlık kanıtı.
- **`inspect` çıktısı yalnız manifest yolunu basıyor.** 6 PNG ve manifest üretilmesine rağmen tek satır yazılıyor: `main.rs:307-319`, `println!` `main.rs:314-318`.
- **İnceleme paketi 3x2 contact sheet + 5 ek artefakt üretiyor.** `contact_sheet.png` (`crates/frameshift-core/src/inspection.rs:91-162`, başlık metni `CONTACT SHEET 3X2` `inspection.rs:108-120`), `mask_pack.png` (`164-251`), `transition_pack.png` (`253-332`), `velocity_pack.png` (`334-412`), `crop_fullres.png` (`414-419`), `fx_showcase_pack.png` (`421-423`), `inspection_manifest.json` (`469-489`).
- **3x2 örnekleme sabit 6 tick.** `select_six_key_ticks` %20/%44/%48/%50/%82 noktalarını alır, 5. adayı ilk event'e kaydırır ve 6'ya kırpar: `inspection.rs:922-959`. Kullanıcı seçimi veya `--frames` bayrağı yok; `inspect` bayrak olarak yalnız `--out-dir` kabul eder (`main.rs:1075-1093`).
- **Contact sheet Final, velocity Draft profiliyle render ediliyor.** `inspection.rs:124` ve `inspection.rs:365`; aynı pakette iki profil karışıyor.
- **`peaks --write-events` proje dosyasını yerinde yeniden yazıyor.** `ProjectSession` ile event'ler eklenir, tüm proje `to_json_pretty()` ile aynı yola yazılır: `main.rs:959-981` (yazma `978-979`), `crates/frameshift-schema/src/project.rs:766-768`. Yedek alınmıyor; yalnız yazılan event sayısı basılıyor (`main.rs:395-399`).
- **CLI bağımlılıkları minimal.** Yalnız `anyhow`, `serde`, `serde_json` ve workspace crate'leri: `crates/frameshift-cli/Cargo.toml:8-17`. `clap`/`structopt` yok.
- **Yardım metni zaman biçimini Flicks'e bağlıyor.** `Time is integer Flicks, <seconds>s ... or f:<frame>`: `main.rs:1225-1226`; sözleşme `Flicks(i64)` ve `f64` saniye yasağı: `brain/specs/timebase.md:22-26`.
- **Şema değişikliği gerekmiyor.** `serde(default)` alanları geriye uyumluluğu taşıyor: `crates/frameshift-schema/src/project.rs:64,82,109,119-121,144,171`; yükleme `from_json_str` + `validate` (`project.rs:771-775`). Bu iş proje JSON'una dokunmaz.

## İncelenen Birincil Kaynaklar

- `crates/frameshift-cli/src/main.rs` — `CliCommand` (30-70), `main` (221-410), `parse_args` (983-1184), `take_flag_value` (1186-1200), `print_usage` (1210-1228), testler (1230-1526).
- `crates/frameshift-core/src/inspection.rs` — `export_inspection_pack_impl` (78-492), `select_six_key_ticks` (922-959).
- `crates/frameshift-cli/Cargo.toml`, `Cargo.toml` (workspace sürümü `0.1.0`, `Cargo.toml:15-18`).
- `AGENTS.md` — komutlar (30-41), ask-first sınırı (53-57), 150 satır kuralı (64).
- `README.md` — durum iddiası (7), doküman dizini (64-70).
- `brain/specs/timebase.md` (22-26), `brain/specs/project-schema.md` (41), `brain/specs/inspection-contract.md` (58-64).
- `brain/research/README.md` (16-23), `brain/index.md` (50-75), `brain/state.md` (7, 35, 53).
- `brain/research/2026-11-22-asta-v3-uretken-darbogazlari.md` (28-34) — eksik tanımı.
- **Çalıştırılan ikili (gözlem):** `target/release/frameshift-cli.exe`, 14.130.176 bayt, mtime 2026-11-22 12:45:15; `main.rs` mtime 2026-11-21 21:16:11. İkili kaynaktan yeni; `cargo` çalıştırılmadı, yalnız `--help` yolları çağrıldı.

## Doğrudan Gözlemler (Kanıtlı)

Aynı ikiliye beş argv ile çağrı yapıldı (PowerShell, birleşik yakalama):

| argv | exit | stdout | stderr |
| :--- | :---: | :--- | :--- |
| `--help` | 0 | 845 bayt, 9 `frameshift-cli` satırı | boş |
| `frame --help` | 1 | boş | `Error: unknown or unexpected argument '--help' for frame` |
| `help frame` | 0 | global usage (845 bayt) | boş |
| `--version` | 1 | boş | `Error: unknown subcommand '--version'. Run with --help for usage.` |
| `inspect --help` | 1 | boş | `Error: unknown or unexpected argument '--help' for inspect` |

- **`help frame` sessizce yanlış cevap veriyor.** Konu yok sayılıyor, exit 0 ve global metin dönüyor; bu, hata vermekten daha yanıltıcı çünkü başarılı görünüyor.
- **Hata kanalı stderr.** `frame --help` çıktısı PowerShell'de `ErrorRecord` olarak geldi; stdout 0 bayt. `--help` çıktısı stdout'ta.
- **`--version` yardım listesinde yok.** Global metin `help` veya `--version` satırı içermiyor (`main.rs:1212-1226`).
- **`peaks` global usage'da var, AGENTS.md'de yok.** İki liste elle tutulduğu için zaten ayrışmış durumda.
- **3x2 sheet boyutu aritmetik olarak 1342x580.** `3*426 + 4*16 = 1342`, `52 + 2*240 + 3*16 = 580` (`inspection.rs:93-98`); hücre 426x240, spec'in "en az 320 px" sınırının üstünde (`inspection-contract.md:63`).
- **README komut içermiyor.** `frameshift-cli`/`init-golden`/`peaks` için `README.md` içinde 0 eşleşme; satır 7 hâlâ "repo yalnızca dokümantasyon içerir, çalışan CLI yoktur" diyor (`README.md:7`).
- **brain listeleri de elle ve eksik.** `brain/research/README.md` 5 kayıt içeriyor ve iki madde hem "4." numaralı (`16-23`); `brain/index.md` araştırma tablosunda `2026-11-22-render-progress-eta.md` yok (`50-75`).

## Çıkarımlar ve Öneriler

- **Kök neden bir tasarım boşluğu, tek satırlık bir hata değil.** Yardım, argüman ayrıştırmasının "özel ilk token" dalı olarak modellenmiş (`main.rs:988-990`); alt komut kolları yardımı tanımıyor. Her yeni komut aynı boşluğu miras alıyor.
- **İki yardım kavramı ayrılmalı:** global usage (komut dizini) ve komut yardımı (bayraklar + örnek). Global usage bugün komut dizini olarak yeterli; eksik olan ikincisi.
- **Metin tek tablodan üretilmeli.** Elle yazılan 9 ayrı string, tam da bugünkü `peaks` ayrışmasını üretir. Tek `CommandSpec` tablosu hem global usage'ı hem komut yardımını besler; parser elle kalır ama tablo testle kilitlenir.
- **`--help` her konumda kazanmalı.** Kullanıcı `frame p.json --help` yazabiliyor; yardım bayrağı bayrak ayrıştırmasından önce taranmalı. Aynı geçişte `-h` ve `help` eşlenmeli.
- **`help <command>` konuyu onurlandırmalı; bilinmeyen konu hata vermeli.** Bugünkü sessiz yanlış cevap en kötü davranış.
- **`--version` `env!("CARGO_PKG_VERSION")` ile gelmeli.** Yeni bağımlılık gerekmiyor; sürüm `Cargo.toml:16` ile tek kaynak.
- **AGENTS.md listesi CLI ile senkron bir testle korunmalı.** Aksi halde `probe`/`sheet`/`autocut` eklendiğinde aynı ayrışma tekrarlar. Test, `AGENTS.md` içindeki `CLI alt komutları:` satırını okuyup tablo adlarıyla karşılaştırabilir; yeni crate gerekmez.
- **Planlı komutlar `--help`'te görünmemeli.** Var olmayan komutu yardımda göstermek agent'ı çalışmayan komuta yönlendirir; "planlı" etiketi yalnız README/AGENTS yol haritasında durmalı.
- **`inspect` çıktısı artefaktları listelemeli.** Kullanıcı 6 PNG'nin varlığını ancak `ls` ile öğreniyor; manifest zaten tüm yolları taşıyor (`inspection.rs:470-488`). Tek satırlık liste keşif maliyetini düşürür.
- **README'nin durum satırı düzeltilmeli.** "Çalışan CLI yok" iddiası yanlış ve agent'ı CLI'yi hiç denememeye itiyor (`README.md:7`).

## Tasarım Seçenekleri (Karar Noktaları)

### 1. Yardım mimarisi

- **A. Komut başına `print_<cmd>_usage`:** 9 ayrı fonksiyon + `print_usage`. En küçük değişiklik, mevcut stile en yakın. Dezavantaj: 10 elle yazılmış metin; bayrak eklemek iki yeri güncellemeyi gerektirir.
- **B. Tek `CommandSpec` tablosu (önerilen):** `COMMANDS: &[CommandSpec]`, `render_global()`, `render_command(name)`. Tek kaynak; yeni komut tek kayıt; test edilebilir saf fonksiyonlar. Dezavantaj: ~120 satır yeni modül, parser ile tablo arasında bayrak uyumsuzluğu yine mümkün (testle azaltılır).
- **C. Hibrit:** Tablo yalnız ad/özet/zorunlu bayraklar; uzun örnekler `print_<cmd>_usage` içinde. Zengin örnek verir, iki katman getirir.

**Öneri: B.** Gerekçe: bugünkü hata sınıfı "iki elle tutulan liste ayrıştı" (`print_usage` 9, AGENTS.md 8); A aynı sınıfı büyütür.

KARAR GEREKİYOR: Yardım mimarisi A, B veya C?

### 2. Yardım metninin yeri

- **A. `main.rs` içinde kalsın:** Dosya zaten 1526 satır (`main.rs:1-1526`); +120 satır daha.
- **B. Yeni `crates/frameshift-cli/src/help.rs` (önerilen):** Saf veri + render; `main.rs` yalnız çağırır; birim testleri modül içinde.

KARAR GEREKİYOR: Yardım metni `main.rs` içinde mi, yeni `help.rs` modülünde mi?

### 3. `--help` önceliği ve `help <command>`

- **A. Yalnız ilk token (bugünkü):** `frame --help` hata verir; reddedilir.
- **B. Her konumda kazanır (önerilen):** `parse_args` içinde, alt komut koluna girmeden `rest` içinde `-h`/`--help` aranır; bulunursa `Help { topic: Some(sub) }`.
- **C. B + `help <command>`:** `help frame` de aynı komut yardımını basar; `help` tek başına global.

**Öneri: C.** Kenar durum: `--out --help` yazılırsa `--help` değer değil yardım sayılır; bu davranış yardım metninde belirtilmeli.

KARAR GEREKİYOR: Yalnız B mi, B+C mi?

### 4. Bilinmeyen konu ve hata kodu

- **A. Exit 1 (önerilen):** Mevcut tüm hatalar `anyhow` ile 1 dönüyor; tutarlı.
- **B. Exit 2:** `clap` geleneği; ama kod tabanında başka yerde 2 yok, yeni bir sözleşme doğar.

KARAR GEREKİYOR: `help nope` için exit 1 mi, 2 mi?

### 5. `--version` biçimi

- **A. `frameshift-cli 0.1.0` (önerilen):** `env!("CARGO_PKG_VERSION")`, tek satır, stdout, exit 0.
- **B. Ek bilgi:** şema sürümü veya git hash. Git hash build script gerektirir; şema sürümü CLI sürümüyle karışabilir.

KARAR GEREKİYOR: `--version` yalnız ad+sürüm mü, ek bilgi de içersin mi?

### 6. AGENTS.md senkron stratejisi

- **A. Elle liste (bugünkü):** Ucuz; `peaks` örneğinde olduğu gibi ayrışır.
- **B. Yalnız işaretçi:** AGENTS.md "komutlar için `frameshift-cli --help`" der; liste tutmaz. Sıfır ayrışma, ama agent oturum başında bir komut çalıştırmadan listeyi göremez.
- **C. Liste + drift testi (önerilen):** `AGENTS.md:39` güncellenir; test, satırdaki backtick'li adları `COMMANDS` ile karşılaştırır. Ayrışma CI'da yakalanır.
- **D. C + "planlı" satırı:** Mevcut 9 komutun yanına `probe`, `sheet`, `autocut` "planlı, henüz yok" olarak eklenir.

**Öneri: D.** Planlı satırı, ters yöndeki hatayı (var olmayan komutu denemek) önler. AGENTS.md 93 satır (`AGENTS.md:1-93`), 150 sınırının altında (`AGENTS.md:64`).

KULLANICI ONAYI GEREKLİ: `AGENTS.md` değişikliği ask-first kapsamında (`AGENTS.md:53-57`); uygulama başlamadan onay alınmalı.

KARAR GEREKİYOR: AGENTS.md için A, B, C veya D?

### 7. Drift testi AGENTS.md'yi okusun mu?

- **A. Evet (önerilen):** `env!("CARGO_MANIFEST_DIR")/../../AGENTS.md`, yalnız `CLI alt komutları:` satırı ayrıştırılır. Kırılganlık: dosya taşınırsa test kırılır; kabul edilebilir.
- **B. Hayır:** Test yalnız `COMMANDS` iç tutarlılığını doğrular; AGENTS.md ayrışması yine elle yakalanır.

KARAR GEREKİYOR: Drift testi AGENTS.md dosyasını okusun mu?

### 8. `inspect` çıktısı

- **A. Davranış değişmesin:** Yalnız doküman tablosu artefaktları anlatır.
- **B. Artefakt listesi basılsın (önerilen):** Manifest alanlarından 7 yol tek satırda listelenir (`inspection.rs:470-488`). Küçük davranış değişikliği; mevcut çıktı satırı korunur.

KARAR GEREKİYOR: `inspect` çıktısı artefakt listesi de basmalı mı?

### 9. README'nin kapsamı

- **A. Yalnız durum satırı düzeltilsin:** `README.md:7`.
- **B. Durum + "CLI" tablosu (önerilen):** 9 komut, ne yapar, hangi dosyaları üretir, ne zaman kullanılır.
- **C. B + `brain/` tablosu:** Aynı tablo `brain/state.md` veya `brain/research/README.md` içine de kopyalanır; kopya sayısı artar, ayrışma riski büyür.

**Öneri: B.** Tek insan yüzeyi; `brain/` yalnız bağlantı verir.

KARAR GEREKİYOR: README için A, B veya C?

## Uygulama Planı

Sıra, her adım tek başına derlenebilir olacak şekilde:

1. **`crates/frameshift-cli/src/help.rs` (yeni).** `FlagSpec { flag, value, summary }`, `CommandSpec { name, summary, synopsis, flags }`, `pub(crate) const COMMANDS: &[CommandSpec]` (9 kayıt), `render_global() -> String`, `render_command(&str) -> Option<String>`. Yalnız `std::fmt`. Yeni bağımlılık yok.
2. **`main.rs:30-70`.** `CliCommand::Help` → `Help { topic: Option<String> }`; `Version` varyantı eklenir.
3. **`main.rs:983-990`.** `parse_args` başında, `split_first` sonrası tek geçiş: `-h`/`--help`/`help` yakalanır; `help <topic>` konuyu taşır; topic `COMMANDS` içinde yoksa `bail!`. `--version`/`-V` `Version` döner. Boş argv `Help { topic: None }` olur.
4. **`main.rs:1032-1060` (ve diğer 8 kol).** Kollar değişmez; yardım artık kola girmeden yakalandığı için `frame --help` hatası ortadan kalkar. `take_flag_value` (`1186-1200`) dokunulmaz.
5. **`main.rs:1210-1228`.** `print_usage` gövdesi `println!("{}", help::render_global())` olur; 19 satırlık literal silinir.
6. **`main.rs:221-229`.** `Help { topic }` kolu: topic varsa `render_command`, yoksa `render_global`; `Version` kolu `env!("CARGO_PKG_VERSION")` basar. İkisi de `Ok(())`.
7. **`main.rs:1230-1526` testleri.** Yeni birim testleri: `global_help_lists_every_command` (satır sayısı `COMMANDS.len()`, her ad mevcut), `every_subcommand_accepts_help_flag` (9 ad için `parse_args(&[name, "--help"])` → `Help { topic: Some(name) }`), `help_flag_after_positional_wins` (`frame p.json --help`), `help_topic_subcommand` (`help peaks`), `unknown_topic_is_error`, `version_flag`. Mevcut iki test değişmeden geçmeli (`1238-1355`).
8. **`crates/frameshift-cli/tests/cli_help.rs` (yeni entegrasyon testi).** `env!("CARGO_BIN_EXE_frameshift-cli")` ile süreç çalıştırılır; exit kodu ve stdout/stderr ayrımı doğrulanır: `--help` (0/stdout), `frame --help` (0/stdout), `help peaks` (0/stdout), `--version` (0/stdout, sürüm içerir), `frame` (1/stderr), `help nope` (1/stderr). Yeni crate gerekmez; `std::process::Command` yeterli.
9. **`AGENTS.md:39`.** `peaks` eklenir; ayrı satırda `planlı (henüz yok): probe, sheet, autocut`. Dosya 93 satırdan ~96 satıra çıkar; 150 sınırı korunur (`AGENTS.md:64`).
10. **`README.md`.** Satır 7 durum iddiası düzeltilir; "Dokümantasyon" bölümünden (64-70) önce "CLI" tablosu eklenir: 9 komut, amaç, üretilen dosyalar, dikkat notları (`peaks --write-events` proje dosyasını yeniden yazar; `inspect` 6 PNG + manifest üretir, 3x2 sheet sabit 6 kare).
11. **`brain/research/README.md:16-23` ve `brain/index.md:50-75`.** Bu raporun kaydı eklenir; eksik `2026-11-22-render-progress-eta.md` kaydı da tamamlanır. (Bu araştırmanın yazma izni yok; uygulama oturumunda yapılır.)
12. **Şema/depolama dokunulmaz.** Proje JSON'u, `Flicks` alanları ve `--write-events` davranışı değişmez; yeni kalıcı alan veya bayrak yok.

Geriye uyumluluk: yardım metinleri ve `--version` yalnız stdout'a yazar; proje dosyasına dokunmaz. Eski proje JSON'ları `serde(default)` ile yüklenmeye devam eder (`project.rs:64-171`, `771-775`). `parse_args` dönüş tipi değişir ama yalnız crate içi kullanılıyor (`main.rs:223`).

## Doğrulama Planı

Uygulama oturumunda sırayla:

```powershell
cargo test -p frameshift-cli
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p frameshift-cli -- --help
cargo run -p frameshift-cli -- frame --help
cargo run -p frameshift-cli -- help peaks
cargo run -p frameshift-cli -- --version
cargo run -p frameshift-cli -- frame
cargo run -p frameshift-cli -- help nope
```

Kabul kriterleri:

- Dört başarılı CLI çağrısı (`--help`, `frame --help`, `help peaks`, `--version`) **exit 0**, çıktı stdout'ta, stderr boş.
- İki hatalı çağrı (`frame`, `help nope`) **exit 1**, mesaj stderr'de `Error:` ile başlar, stdout boş.
- `--help` çıktısı 9 komutun tamamını + `help` + `--version` satırlarını içerir; `frame --help` yalnız `frame` bayraklarını anlatır.
- `help peaks` çıktısı `--write-events` ve `--half-window` bayraklarını içerir.
- `cargo test --workspace` yeşil; `clippy` 0 uyarı.
- `AGENTS.md` satır sayısı ≤150: `(Get-Content AGENTS.md).Count`.
- `git diff --stat` yalnız beklenen dosyaları gösterir (uygulama oturumunda).

## Riskler ve Doğrulanmamış Noktalar

- **Bu rapor hiçbir değişiklik uygulamadı.** Tüm davranış iddiaları mevcut kod ve mevcut ikili gözlemine dayanır; düzeltme sonrası davranış doğrulanmadı.
- **Gözlem ikilisi kaynaktan yeni ama aynı derleme olduğu kanıtlanmadı.** `main.rs` mtime karşılaştırması bunu destekliyor; başka crate'lerin durumu yardım yolunu etkilemez.
- **AGENTS.md düzenlemesi onay gerektirir** (`AGENTS.md:53-57`); onay gelmezse adım 9 ve drift testi (adım 7) düşer.
- **Drift testi dosya yoluna bağlı.** `AGENTS.md` taşınır veya satır biçimi değişirse test kırılır; bu bilinçli bir maliyet.
- **Tablo ile parser arasında bayrak uyumsuzluğu tamamen kapanmaz.** Tablo yalnız metni tek kaynağa indirir; parser hâlâ elle. Örnek-argv testleri kısmi koruma sağlar.
- **`--help` ön taraması bir kenar durum yaratır:** `--out --help` değer olarak kullanılamaz. Nadir; belgelenmeli.
- **`inspect` çıktısı değişirse** mevcut çıktıyı ayrıştıran bir betik varsa kırılabilir; repo içinde böyle bir tüketici bulunmadı (`Exported inspection pack` yalnız `main.rs:315`).
- **Planlı komutların yardımda gösterilmemesi** bilinçli bir sınır: `probe`/`sheet`/`autocut` uygulanana kadar `--help`'te yer almamalı.
- **`frame` adı ile ikili adı farklı.** Kullanıcı `frame --help` yazıyor; usage `frameshift-cli` diyor (`main.rs:1215-1223`). Bir alias varsa dokümanda belirtilmeli; alias'ın varlığı doğrulanmadı.
- **README tablosunun doğruluğu** uygulama sonrası `--help` çıktısıyla karşılaştırılmalı; bu araştırma tabloyu yazmadı.
