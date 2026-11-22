---
title: Probe/Sheet — tek süreçte N kare üretimi
date: 2026-11-22
status: draft
tags: [cli, engine, inspection, wgpu, determinism, performance]
---

# Probe/Sheet — tek süreçte N kare üretimi

## Soru / Amaç

Tek bir süreçte N karelik probe/sheet üretimi mümkün mü; kare başına ~10 sn'lik maliyetin ne kadarı `Engine::new_headless` kaynaklı ve tek Engine ile keyfi tick'ler sıralı render edilebilir mi? Kapsam yalnızca araştırma; kaynak kod, cargo, render ve git işlemi yok.

Altı alt soru: (1) `Engine::new_headless` gerçekte neyi başlatıyor, (2) tek Engine ile N keyfi tick sıralı render edilebilir mi, (3) `--start/--end/--count/--cols/--out/--profile` yeterli mi, ayrı `probe --times` gerekli mi, (4) inspection sheet kodunun hangi parçaları pack'in kalanını tetiklemeden yeniden kullanılabilir, (5) paralel Engine mantıklı mı, (6) M1 hash'leri ve sheet determinizmi nasıl korunur.

## Mevcut Kod Gerçekliği (Kanıtlı)

### Engine başlatma

- `Engine::new_headless` → `with_cache_capacity(128)`; alanlar `GpuCompositor`, `LruFrameCache` (128 kare / 1 GiB), `MediaResolver`, boş `audio_envelope_cache` — `crates/frameshift-core/src/engine.rs:29-30,46-58`.
- `GpuContext::new_headless`: `wgpu::Instance::default()`, üç aşamalı adapter denemesi (high-performance → default → software/fallback), `request_device` + queue, linear sampler, MSAA format sorgusu — `crates/frameshift-render/src/context.rs:4-54,57-97`.
- `GpuCompositor::new_headless`: `compile_all_with_msaa` (48 yerleşik pipeline, `crates/frameshift-render/src/pipelines.rs:81-132`), MSAA varsa +6 `Scene3D` pipeline (`pipelines.rs:70-77,626-700`), dummy zero texture, boş cache tabloları + `TexturePool`; kaynakta `wgpu::PipelineCache` kullanımı yok (grep: 0 eşleşme), yani her yeni süreç 48 pipeline'ı yeniden derliyor — `crates/frameshift-render/src/compositor.rs:1177-1216`.
- Init'te FFmpeg çağrısı, medya decode veya audio mix yok: `MediaResolver::new` = `Self::default()` (`crates/frameshift-core/src/resolver.rs:286-289`); container probe ilk karede ve cache'li (`resolver.rs:326-338`); disk audio RMS envelope ilk `render_frame_at`'te hesaplanıp cache'leniyor (`engine.rs:155-156,758-792`).

### Tek Engine ile çoklu kare

- `render_frame_at(&mut self, project, profile, output_tick)` — `crates/frameshift-core/src/engine.rs:122-134`; `RenderOptions` Straight + default, dither Off.
- `render_timeline_sequence` tek Engine ile tüm tick'leri sırayla render ediyor — `engine.rs:220-236`; MP4 yolu bunu kullanıyor (`engine.rs:197-200`); Asta v3 timeline'ı 1200 kare.
- Determinism kapısı: cache temizlenip tüm dizi yeniden render edilir, tüm hash'ler birebir eşleşmeli — `engine.rs:343-356`.
- Seek-independence kapısı: 5 tick karışık/ters sırada, soğuk cache; `max_channel_diff == 0` ve hash eşitliği — `engine.rs:358-388`.
- Kontrat: `Frame(t) = Evaluate(Graph, t, Profile)`; kareler arası gizli GPU durumu yasak — `brain/specs/render-graph.md:37-39`.

### CLI yüzeyi

- `frame` her çağrıda projeyi yükler, `Engine::new_headless` kurar, tek kare render eder, PNG yazar — `crates/frameshift-cli/src/main.rs:265-294`.
- 40 kare = 40 ayrı süreç; şikayetin kaynağı bu döngü — `brain/research/2026-11-22-asta-v3-uretken-darbogazlari.md:20`.
- `inspect` tam inspection pack'i üretir — `main.rs:307-320`; pack tek bir `export_inspection_pack_impl` fonksiyonu ve 6 yüzeyi + fx showcase'i koşulsuz üretiyor — `crates/frameshift-core/src/inspection.rs:78-83,423`; contact sheet 3x2 sabit ve 6 tick — `inspection.rs:91-98,922-959`.
- CLI'da toplu kare/sheet komutu yok; usage listesi — `main.rs:1210-1228`.
- Zaman sözdizimi: integer Flicks, `<s>s`, `<num>/<den>s`, `f:<frame>` — `main.rs:734-760`; `f:` proje fps'inde tick'e çevriliyor — `main.rs:888-898`.

### Inspection sheet'in yeniden kullanılabilir parçaları

- `Rgba8Frame::{filled,save_png,fnv1a_hash64,sha256_hex}` — `crates/frameshift-media/src/frame.rs:54-74,107-143,174-195`.
- `downscale_frame_nearest_or_box` deterministik tamsayı box ortalaması — `crates/frameshift-media/src/proxy.rs:100-109`.
- `render_label_badge` / `render_label_badge_scaled` — `crates/frameshift-vector/src/text.rs:775-798`; yalnızca canvas sınırında kırpıyor, hücre sınırında değil (`text.rs:799-802`).
- `blit_frame_with_border` ve `fill_rect_u8` — `inspection.rs:1175-1213`; ikisi de private, davranışları saf kopyalama/doldurma.
- `SampledFrameHash` (tick, frame_index, fnv1a, sha256) ve `InspectionPackManifest` — `inspection.rs:24-76`.
- `PanelSpec` fx showcase'e özel — `inspection.rs:706-713`; `select_six_key_ticks` altı tick'e sabit — `inspection.rs:922-959`.
- Kontrat contact sheet'i "çok sayfalı, etiketli" tanımlıyor; sayfa başına en fazla 3x3 veya 4x3 hücre ve hücre başına en az 320 px — `brain/specs/inspection-contract.md:58-64`.

### Zaman, hata ve kimlik

- Kalıcı zaman `Flicks(i64)`; `f64` saniye yasak — `brain/specs/timebase.md:24-26`; `Flicks` `#[repr(transparent)]` ve serde'de i64 olarak yazılıyor — `crates/frameshift-time/src/flicks.rs:16-26`.
- `ticks_to_frame_floor` tam çözümü var — `crates/frameshift-time/src/rational.rs:127-129`; mevcut pack `tick / frame_ticks` tamsayı bölmesi kullanıyor — `inspection.rs:125`; `CoreError::InvalidInspectionRange` mevcut — `crates/frameshift-core/src/error.rs:52`; kontrat boş/ters aralıkta bunu şart koşuyor — `inspection-contract.md:115`; profil farkı raporda gizlenemez — `inspection-contract.md:106`.

### Asta v3 gerçekliği

- `local_media/asta_v3/asta_v3_project.json`: disk audio `bgm_v3.wav` (satır 51), `audio_clips` (satır 420), yedi `audio_rms` binding (satır 2218-2303); üretimde 0.5 sn aralıklı contact sheet'ler kullanılmış — `brain/log/2026-11-22-asta-v3-red-line.md:71`.

## İncelenen Birincil Kaynaklar

- `crates/frameshift-core/src/engine.rs` — 29-58/96-173/197-252/343-388/629-646/735-793; `crates/frameshift-core/src/inspection.rs` — 16-76/78-162/423/494-520/706-713/922-959/1175-1213; `crates/frameshift-core/src/resolver.rs` — 276-338; `crates/frameshift-core/src/error.rs` — 52; `crates/frameshift-core/src/lib.rs` — 4-32.
- `crates/frameshift-render/src/context.rs` — 4-97; `crates/frameshift-render/src/compositor.rs` — 62-92/1177-1216; `crates/frameshift-render/src/pipelines.rs` — 70-77/81-132/626-700/843/1002.
- `crates/frameshift-cli/src/main.rs` — 30-70/221-320/665-717/727-760/888-898/983-1060/1186-1228/1239-1328; `crates/frameshift-media/src/frame.rs` — 54-74/107-143/174-195; `crates/frameshift-media/src/proxy.rs` — 100-109.
- `crates/frameshift-vector/src/text.rs` — 775-802; `crates/frameshift-time/src/flicks.rs` — 16-26; `crates/frameshift-time/src/rational.rs` — 98-129.
- `brain/specs/inspection-contract.md` — 39-40/58-64/106/115; `brain/specs/timebase.md` — 24-26; `brain/specs/project-schema.md` — 141-160; `brain/specs/render-graph.md` — 37-39.
- `brain/research/2026-11-22-asta-v3-uretken-darbogazlari.md` — 10/20/33/38; `brain/research/2026-11-22-frameshift-tuzaklar.md` — 14-16.
- `local_media/asta_v3/asta_v3_project.json` — 51/420/2218-2303; `brain/log/2026-11-22-asta-v3-red-line.md` — 71.

Web kaynağı kullanılmadı; tüm kanıt yerel kaynak kod ve yerel dokümandır.

## Doğrudan Gözlemler (Kanıtlı)

1. `Engine::new_headless`'ın maliyeti wgpu Instance/adapter/device/queue + 48 render pipeline derlemesi (MSAA açıksa 54) + dummy texture + boş cache kurulumudur; medya ve ses tembel başlatılır (`context.rs:19-54`, `pipelines.rs:146-733`, `compositor.rs:1196-1216`, `resolver.rs:286-338`).
2. Tek Engine ile keyfi tick sıralı render kontratta garanti; kapı 5 tick ile `max_channel_diff == 0` doğruluyor (`render-graph.md:37-39`, `engine.rs:358-388`).
3. Aynı desen üretimde zaten kullanılıyor: MP4 render'ı tüm timeline'ı tek Engine'de döngüyle çiziyor (`engine.rs:220-236`).
4. `render_frame_at` `&mut self` aldığı için tek Engine paralel kullanılamaz; paralellik ancak her iş parçacığına ayrı Engine (=ayrı wgpu device) ile mümkün (`engine.rs:122-127`, `compositor.rs:1178-1180`).
5. Inspection pack'te kısmi mod yok; pack çağrısı mask/transition/velocity/crop/fx dahil her şeyi üretir ve contact sheet ızgarası fonksiyon içinde gömülü, 3 sütun/6 tick sabit (`inspection.rs:78-83,93-98,423,922-959`).
6. Pack hash'i downscale'den önce alıyor; `fnv1a_hash64` boyut + alpha + piksel byte'larını kapsıyor, `pts`'yi kapsamıyor (`inspection.rs:124-133`, `frame.rs:107-118`).
7. `save_png` png crate ile yazıyor; kodda zaman damgası yok (`frame.rs:174-195`). Byte düzeyi determinizm bu araştırmada test edilmedi.
8. Asta v3'te disk audio ve yedi `audio_rms` binding var; her yeni süreç ilk karede tüm sesi mix edip RMS envelope hesaplıyor (`asta_v3_project.json:51,420,2218-2303`, `engine.rs:758-792`).
9. Etiket rozetleri yalnızca canvas sınırında kırpılıyor; 320 px hücrede uzun etiket komşu hücreye taşabilir (`text.rs:799-802`).
10. `inspection-contract` sayfa başına 3x3/4x3 ve ≥320 px hücre istiyor; 40 karelik tek büyük grid bu okunabilirlik sınırını aşar (`inspection-contract.md:63`).

## Çıkarımlar ve Öneriler

### 6.1 Engine başlatma ve tek Engine ile N tick

`new_headless` bir kez kurulduğunda adapter/device/queue, 48 pipeline ve texture pool hazır olur; sonraki `render_frame_at` çağrıları yalnızca graph değerlendirmesi + readback yapar. Tek Engine ile N keyfi tick sıralı render edilebilir; sıra önemli değil, çünkü kontrat saf fonksiyon ve mevcut seek kapısı bunu 5 tick için kanıtlıyor. Kısıt: API `&mut self`, yani aynı Engine eşzamanlı kullanılamaz; sıralı kullanım serbest.

### 6.2 CLI yüzeyi

Öneri: tek `sheet` komutu. `--start` (varsayılan 0), `--end` (varsayılan `timeline.duration_ticks`), `--count` veya `--times`, `--cols` (varsayılan 4), `--cell-width` (varsayılan 426), `--out`, `--profile`, opsiyonel `--frames-dir` ve `--manifest`. `--count` ile `--times` birbirini dışlar. Ayrı `probe --times` gereksiz: aynı render maliyetini ödeyip ikinci bir yüzey açar; hash/metadata ihtiyacını manifest karşılar. Çıktı adlandırma: tek sayfada `--out` aynen yazılır; çok sayfada `stem_p01.png`, `stem_p02.png`. Manifest varsayılanı `stem_manifest.json`. Manifest `Flicks` taşır, `f64` saniye taşımaz.

### 6.3 Yeniden kullanım

Doğrudan kullanılabilir: `Rgba8Frame`, `downscale_frame_nearest_or_box`, `render_label_badge(_scaled)`, `blit_frame_with_border`, `fill_rect_u8`, `SampledFrameHash`, renk sabitleri. Kullanılmamalı: `PanelSpec` (fx showcase kartı) ve `select_six_key_ticks` (altı tick varsayımı). Pack fonksiyonu çağrılmamalı; kısmi modu yok ve 6 yüzeyi birden üretir. İlk turda yeni `sheet.rs` modülü, yalnızca düşük seviye yardımcıları `pub(crate)` yaparak yeniden kullanmalı; pack'in ızgara döngüsünü çıkarmak M2 gate'lerini gereksiz riske atar.

### 6.4 Paralel Engine

Önerilmez. Her Engine kendi wgpu device'ını, 48 pipeline'ını ve cache'lerini kurar; N paralel Engine = N× init + N× VRAM. Tek GPU'da render zaten GPU'ya bağlıysa duvar süresi kazancı belirsiz, init maliyeti ve bellek baskısı kesin artar. Ölçüm yapılmadı; bu bir çıkarımdır. Gerekirse sonraki adım tek device + iş parçacığı başına compositor refactor'ü olur, bu görevin dışında.

### 6.5 M1 hash'leri ve determinizm

Koşul: sheet yolu `render_frame_at` ile aynı profili ve aynı varsayılan `RenderOptions`'u kullanmalı; hash downscale'den önce alınmalı; `RenderOptions` varsayılanları (dither Off) değişmemeli. Bu durumda sheet manifest'indeki `fnv1a_hash64`, aynı tick/profil için `frame` komutunun bastığı hash ile aynı olur. M1 `verify-all` çıktısı değişmemeli çünkü render yolu değişmiyor. Sheet PNG'sinin byte determinizmi ayrı bir konu; hash otoriter, PNG determinizmi doğrulanmadı.

### 6.6 Çıktı biçimi

Yalnızca hash/metadata yetmez: kullanıcının şikayeti görsel QC. Yalnızca sheet de yetmez: 426 px hücrede metin/kenar detayı kaybolur. Öneri: sheet + manifest varsayılan; `--frames-dir` verilirse tam çözünürlüklü PNG'ler de yazılır. Ayrı bir "hash-only probe" gerçek maliyeti (render) azaltmaz, yalnızca çıktı yüzeyini böler.

### 6.7 Beklenen hızlanma

Kod düzeyinde kesin olan: 40 kare için 40 → 1 Engine init, yani 39 init (%97.5) ortadan kalkar; 40 → 1 süreç, her varlık için 40 → 1 container probe, disk audio varsa 40 → 1 audio RMS mix. Duvar süresi kazancı init payına bağlıdır ve ölçülmedi. Kullanıcının ~10 sn/kare gözlemiyle kaba model (init = I, kare render = 10−I): I=8 → ~88 sn (~4.5×), I=5 → ~205 sn (~2.0×), I=3 → ~283 sn (~1.4×), I=2 → ~322 sn (~1.2×). Bu tablo ölçüm değil, aritmetik modeldir.

## Tasarım Seçenekleri (Karar Noktaları)

### D1 — Komut adı

- **Seçenek A (öneri):** `sheet`. Kısa, contact sheet ile aynı terminoloji, `inspect` pack'inden ayrı.
- **Seçenek B:** `probe`. Kullanıcı diliyle uyumlu ama `probe_media_info`/`ffprobe` ile karışır ve metadata çağrışımı yapar.
- **Seçenek C:** `inspect --sheet`. Yeni komut yok; ancak pack sözleşmesini ve "tam paket" beklentisini bozar.

Tradeoff: A en düşük kavram çakışması; B keşfedilebilirlik; C yüzey sayısını korur ama `inspect` semantiğini kirletir. Net öneri: A.

KARAR GEREKİYOR: Komut adı `sheet` mi, `probe` mu, yoksa `inspect` alt bayrağı mı olsun?

### D2 — Sayfalama

- **Seçenek A:** 4 sütun × 3 satır sayfa; 40 kare → 4 sayfa (12/12/12/4). Kontratın 3x3/4x3 ve ≥320 px kuralına uyar.
- **Seçenek B:** Tek büyük grid (`--cols 8` → 8x5). Tek görselde tarama kolay; hücre 320 px'te kalırsa sayfa ~2704×1048 olur ve görsel modeller downscale eder, detay kaybı riski var.
- **Seçenek C (hibrit, öneri):** ≤12 kare tek sayfa, fazlası otomatik sayfalı; `--single-page` ile zorla tek görsel. En esnek ama iki davranışı belgelemek gerekir.

Tradeoff: A kontrat uyumlu ve okunabilir; B hızlı tarama ama okunabilirlik sınırında; C kullanıcıya esneklik verir, kod ve test yüzeyi biraz büyür. Net öneri: C (varsayılan A davranışı, kaçış kapısı B).

KARAR GEREKİYOR: Varsayılan 4x3 sayfalama mı, tek büyük grid mi, yoksa hibrit + `--single-page` mi?

### D3 — Aralık semantiği

- **Seçenek A (öneri):** `[start, end)` yarı açık. 0–20 sn, 40 kare → tam 0.5 sn adım; üretimdeki 0.5 sn'lik sheet ile aynı.
- **Seçenek B:** `[start, end]` kapalı. 40 kare → 20/39 = 0.5128 sn adım; sezgisel ama üretimdeki 0.5 sn gridini bozar.

Tradeoff: A temiz ve öngörülebilir; B "son kareyi de al" beklentisine uyar. Net öneri: A; son kare gerekiyorsa `--times` ile açıkça verilir.

KARAR GEREKİYOR: `--end` hariç mi (yarı açık) yoksa dahil mi olsun?

### D4 — `--times` yüzeyi

- **Seçenek A (öneri):** `sheet --times f:0,f:30,...`; `--count` ile dışlar. Tek komut, tek engine init.
- **Seçenek B:** Ayrı `probe --times`. İki komut, iki keşif yüzeyi; aynı render maliyeti.
- **Seçenek C:** `frame --times`. Mevcut `frame` semantiğini (tek kare + `--out`) çoklu çıktıya genişletir; geriye uyum riski ve karmaşık hata yolları.

Tradeoff: A en az yüzey ve en yüksek hız; B kavramsal ayrım ama gereksiz; C mevcut komutu bozar. Net öneri: A.

KARAR GEREKİYOR: Açık tick listesi `sheet --times` içinde mi olsun, ayrı `probe --times` komutu mu olsun?

### D5 — Varsayılan profil

- **Seçenek A (öneri):** `final`. `frame` ile aynı varsayılan, hash eşliği ve M1 karşılaştırması doğrudan.
- **Seçenek B:** `draft`. Daha hızlı iterasyon; hash'ler `final` ile kıyaslanamaz, kontrat gereği raporda profil belirtilmeli.

Tradeoff: A doğruluk/karşılaştırılabilirlik; B hız. Net öneri: A; hızlı tur için kullanıcı açıkça `--profile draft` verir.

KARAR GEREKİYOR: `sheet` varsayılan profili `final` mi, `draft` mı olsun?

### D6 — Render çözünürlüğü

- **Seçenek A (öneri):** Kareleri proje profilinde (ör. 1920×1080) render et, hücreye box-downscale et. Hash eşliği ve metin/AA detayı korunur; maliyet tam çözünürlük.
- **Seçenek B:** Kareleri doğrudan hücre çözünürlüğünde (426×240) render et. Piksel maliyeti ~20× düşer; hash'ler `frame` ile kıyaslanamaz, metin rasterizasyonu değişir.
- **Seçenek C:** `--render-scale` ile opt-in ara çözünürlük. Esnek ama yeni bir "hangi profilde üretildi" boyutu ve karışıklık getirir.

Tradeoff: A doğruluk, B hız, C esneklik. Net öneri: A; B/C ancak ölçüm sonrası ve ayrı bayrakla.

KARAR GEREKİYOR: Sheet kareleri proje profilinde mi, hücre çözünürlüğünde mi render edilsin?

## Uygulama Planı

### 1. Core: yeni `sheet` modülü

- `crates/frameshift-core/src/sheet.rs` (yeni): `SheetSpec`, `SheetPage`, `SheetManifest`, `export_sheet_impl(engine: &mut Engine, project: &Project, spec: &SheetSpec, out_path: &Path) -> Result<SheetManifest, CoreError>`.
- `SheetSpec`: `start_tick: Flicks`, `end_tick: Flicks`, `count: usize`, `times: Vec<Flicks>`, `cols: u32`, `cell_width: u32`, `profile: RenderProfileKind`, `frames_dir: Option<PathBuf>`.
- Örnekleme saf fonksiyon: `sample_ticks(start, end, count) -> Result<Vec<Flicks>, CoreError>`; `i128` ara aritmetik, `tick_i = start + floor(i*(end-start)/count)`, `i in 0..count`; `end <= start` veya `count == 0` → `CoreError::InvalidInspectionRange` (`error.rs:52`, kontrat `inspection-contract.md:115`); `MAX_SHEET_FRAMES = 512`, aşımda `InvalidArgument`.
- Sayfa düzeni saf fonksiyon `layout_pages(count, cols, rows_per_page) -> Vec<SheetPage>` (son sayfa gerçek satır sayısına kırpılır); hücre yüksekliği proje en-boy oranından tamsayı yuvarlamayla: `cell_h = round(cell_w * res.h / res.w)`, 1920×1080 + 426 → 240.
- Döngü: `render_frame_at` → hash (downscale öncesi) → opsiyonel `save_png` → `downscale_frame_nearest_or_box` → `blit_frame_with_border` → tam kareyi bırak. Tam çözünürlüklü kareler biriktirilmez; yalnızca sayfa başına hücreler tutulur.
- Manifest: `manifest_version: u32`, `schema_version`, `revision_id`, `project_name`, `profile`, `timeline_fps`, `start_tick`, `end_tick`, `render_width/height`, `cols`, `cell_width/height`, `sampled_ticks: Vec<Flicks>`, `frame_hashes: Vec<SampledFrameHash>`, `pages: Vec<SheetPage>`, `frame_paths: Vec<PathBuf>`, `manifest_path`. Zaman alanları yalnızca `Flicks`; `#[serde(default)]` yeni alanlarda.
- `frame_index`: `project.timeline.fps.ticks_to_frame_floor(tick)` (NTSC'de doğru; pack'teki `tick/frame_ticks` bölmesini taklit etme, pack'e dokunma).

### 2. Core: görünürlük ve Engine API

- `inspection.rs:16-21`: `SHEET_BG`, `HEADER_BG`, `CELL_BORDER`, `BADGE_FG`, `BADGE_BG`, `HEADER_FG` → `pub(crate) const`; davranış değişmez.
- `inspection.rs:1175-1183` `fill_rect_u8` ve `1185-1213` `blit_frame_with_border` → `pub(crate) fn`; gövdeler aynı kalır.
- `engine.rs:2-4`: `use crate::sheet::{SheetManifest, SheetSpec, export_sheet_impl};`.
- `engine.rs:96-108` civarına `pub fn export_sheet(&mut self, project: &Project, spec: &SheetSpec, out_path: &Path) -> Result<SheetManifest, CoreError>`; içinde `project.validate()?` + `export_sheet_impl`.
- `lib.rs:4-8` → `pub mod sheet;`; `lib.rs:30-32` → `SheetManifest`, `SheetSpec` re-export.

### 3. CLI

- `main.rs:39-44` yanına `CliCommand::Sheet { project_path, out, start: TimeSpec, end: TimeSpec, count: usize, times: Vec<TimeSpec>, cols: u32, cell_width: u32, profile, frames_dir, manifest }`.
- `main.rs:1060` sonrasına parse kolu: `--start`, `--end`, `--count`, `--times` (virgüllü liste, her parça `parse_time_spec`), `--cols`, `--cell-width`, `--profile`, `--frames-dir`, `--manifest`, `--out`. `--times` ile `--start/--end/--count` birlikte verilirse hata.
- `main.rs:294` sonrasına çalıştırma kolu: projeyi bir kez yükle, `Engine::new_headless()` bir kez kur, `Instant` ile `engine_init_ms` ve `render_ms` ölç, `export_sheet`, tek satır özet bas. Zamanlama manifest'e yazılmaz (manifest byte-stabilitesi için).
- `main.rs:693-717` yanına `format_sheet_success(...)`: kare sayısı, sayfa sayısı, profil, çözünürlük, `engine_init_ms`, `render_ms`, ilk sayfa yolu, manifest yolu.
- `main.rs:1215-1223` usage'a `sheet` satırı; `main.rs:1225-1226` zaman açıklaması aynı kalır.
- `main.rs:1239-1328` testlerine sheet parse vakaları; `--times` çakışması ve eksik `--out` reddi.

### 4. Testler

- `sheet_sampling_is_half_open_and_exact`: 0–20 sn, 40 örnek → 40 tick, ilk 0, son 19.5 sn, adım 352_800_000 Flicks.
- `sheet_rejects_empty_reversed_and_oversized_ranges`: `end <= start`, `count == 0`, `count > MAX_SHEET_FRAMES` → `InvalidInspectionRange` / `InvalidArgument`.
- `sheet_page_layout_paginates`: 40 kare, 4 sütun, 3 satır → 4 sayfa; son sayfa 4 hücre/1 satır.
- `sheet_cell_height_follows_project_aspect`: 1920×1080 + 426 → 240; 1080×1920 → 757.
- `sheet_manifest_hash_matches_direct_frame_render` (GPU): golden preset, draft profili 160×90'a override (`core_tests.rs:40-43` deseni), 3 tick; manifest hash'leri aynı Engine'de `render_frame_at` hash'leriyle birebir.
- `sheet_pages_and_manifest_are_written_deterministically`: aynı spec iki kez → aynı `sampled_ticks`, aynı hash dizisi, aynı sayfa sayısı.
- `sheet_writes_full_res_frames_only_when_requested`: `frames_dir` yokken PNG yok, varken tick başına bir PNG.
- CLI: `parses_all_subcommands`'a sheet; `sheet_times_conflicts_with_range`; `sheet_requires_out`.

### 5. Geriye uyumluluk

- `Project` şemasına dokunulmaz; eski proje JSON'ları aynı serde default'larla yüklenir. Yeni manifest ayrı bir dosyadır, proje yüklemesini etkilemez; yeni harici crate/dependency yok.
- `RenderOptions` varsayılanları ve `select_profile` değişmez; M1 hash'leri korunur (`compositor.rs:75-91`, `engine.rs:622-627`).
- Yeni CLI varyantı yalnızca `main` match'ini ve testleri etkiler; mevcut komutların argümanları değişmez.
- `pub(crate)` görünürlük değişiklikleri dış API'yi değiştirmez.
- `manifest_version` ve yeni alanlarda `#[serde(default)]`; ileride alan eklemek eski manifestleri kırmaz.

## Doğrulama Planı

Uygulama oturumunda çalıştırılacak komutlar:

1. `cargo test -p frameshift-core --test core_tests sheet` — tüm sheet testleri geçmeli.
2. `cargo test -p frameshift-cli sheet` — parse ve format testleri geçmeli.
3. `cargo clippy --workspace --all-targets -- -D warnings` — çıkış 0, 0 uyarı.
4. Baseline: değişiklik öncesi `cargo run --release -p frameshift-cli -- verify-all --out-dir target/verify_sheet_before`; sonra `target/verify_sheet_after`. `m1.final_summary.frame_hashes` dizileri birebir aynı olmalı; `determinism.passed`, `seek_independence.passed`, `all_gates_passed` `true` kalmalı.
5. `cargo run --release -p frameshift-cli -- sheet local_media/asta_v3/asta_v3_project.json --count 40 --out target/qc/asta_sheet.png --profile final` — 40 kare, 4 sayfa, tek `engine_init_ms` satırı.
6. Hash eşliği: `sheet ... --times f:0,f:450,f:900 --frames-dir target/qc/frames --out target/qc/parity.png`; manifest'teki üç `fnv1a_hash64`, ayrı `frame --tick f:0` / `f:450` / `f:900` çıktılarındaki `fnv1a` değerleriyle aynı olmalı.
7. Görsel kabul: 4 sayfa 100% zoom'da okunabilir; etiketler komşu hücreye taşmıyor; her hücrede rev/f/tick var.

Kabul kriterleri: tek süreçte tek Engine init; 40 kare için 40 hash; sayfa sayısı `ceil(40/12) = 4`; `final` profilinde hash'ler `frame` ile birebir; M1 hash'leri baseline ile birebir; clippy 0 uyarı; testler yeşil. Hızlanma oranı ölçülüp rapora yazılır, önceden iddia edilmez.

## Riskler ve Doğrulanmamış Noktalar

- **Init payı ölçülmedi.** ~10 sn/kare gözleminin ne kadarı wgpu init, ne kadarı render, bilinmiyor. Hızlanma modeli yalnızca aritmetik; gerçek oran `engine_init_ms` / toplam ile ölçülmeli.
- **Sürücü shader cache'i.** Windows/DX12 sürücüsü pipeline'ları süreçler arası cache'leyebilir; soğuk ve sıcak ilk çalıştırma farklı init süresi verebilir. Ölçümde ikisi ayrı raporlanmalı.
- **40 tick'te bit-eşitlik kanıtlı değil.** Seek kapısı yalnızca 5 tick ve golden preset üzerinde; Asta v3'ün 76 düğüm/66 binding'lik grafiğinde 40 tick'in tamamı test edilmedi. İki kez sheet çalıştırıp hash dizilerini karşılaştırmak gerekir.
- **Etiket taşması.** `render_label_badge` hücre sınırını bilmiyor (`text.rs:799-802`); 320 px hücrede uzun etiket komşuya taşar. Öneri: üstte kısa event rozeti, altta iki kısa satır (`f:...`, `tick:...`); 426 px varsayılan.
- **Dikey proje sayfaları.** 9:16 projede 426 px genişlik → 757 px yükseklik; 4x3 sayfa ~1784×2387 olur. Gerekirse `--cell-width` küçültülür veya sayfa başına hücre azaltılır.
- **Bellek.** Tam çözünürlüklü kareler biriktirilmemeli; 40×1080p = ~332 MB. Döngüde render → hash → thumb → drop yapılmalı. LRU zaten 128 kare/1 GiB ile sınırlı (`engine.rs:29-30`).
- **PNG byte determinizmi.** png crate'in zaman damgası yazmadığı görülüyor (`frame.rs:174-195`) ama byte-eşitlik test edilmedi; otoriter kanıt frame hash'idir.
- **Paralel Engine ölçülmedi.** N device'ın gerçekten hızlandırdığına dair kanıt yok; VRAM ve init çoğalması kesin. Önerilmiyor.
- **MCP paritesi yok.** `frameshift-mcp` hâlâ tek kare (`server.rs:375`); sheet yalnızca CLI'da olacak. Agent akışı MCP üzerinden gidiyorsa sonraki adım aynı core API'yi MCP'ye bağlamak.
- **`frame --help` UX'i.** Alt komut düzeyinde `--help` hâlâ hata veriyor (`main.rs:988-990,1032-1052`); sheet eklerken global bir `-h/--help` kontrolü küçük bir iyileştirme olur, bu görevin kapsamı değil.
- **Kontrat yorumu.** `sheet`'in `inspection-contract` §3.1 kapsamına girip girmediği açık değil; giriyorsa 3x3/4x3 sayfalama ve zorunlu etiketler bağlayıcı. D2 kararı bunu netleştirmeli.
