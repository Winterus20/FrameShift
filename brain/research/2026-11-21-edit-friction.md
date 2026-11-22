---
title: Edit sürtünmesi — sekiz motor ve CLI boşluğu
date: 2026-11-21
status: active
tags: [engine, timebase, render-graph, schema, media-io, color, text-vector, agent, inspection]
---

# Edit sürtünmesi — sekiz motor ve CLI boşluğu

## Soru / Amaç

Koleedits dogfood oturumunda gerçek bir kesim durdu. Bu not, sekiz sürtünmeyi kilitli spec ve bugünkü kodla eşler, birincil dış kaynaklarla karşılaştırır ve uygulamaya geçmeden önce hangi düzeltmenin hangi crate dikişine oturduğunu söyler.

Uygulama bu belgede başlamaz. `brain/decisions/`, spec dosyaları, `AGENTS.md`, shader, `Cargo.toml` ve testler değiştirilmedi. Hiçbir `cargo` kapısı bu oturumda çalıştırılmadı. "Çalışıyor" denilen yer, okunan kod veya okunan proje JSON'udur.

Kaynak proje (okundu, yeniden render edilmedi): `target/dogfood/koleedits/koleedits_project.json`. Günlük: [log/2026-11-21-koleedits-dogfood.md](../log/2026-11-21-koleedits-dogfood.md).

## İncelenen birincil kaynaklar

Kod ve kilitli sözleşmeler (2026-11-21):

- `brain/specs/timebase.md`, `project-schema.md`, `render-graph.md`, `effect-shader-api.md`, `inspection-contract.md` (`status: locked`)
- `brain/decisions/0002-rational-timebase-flicks.md`, `0003-semantic-event-constraint-timing.md`
- `crates/frameshift-time`, `frameshift-schema`, `frameshift-media`, `frameshift-vector`, `frameshift-render`, `frameshift-core`, `frameshift-mcp`, `frameshift-cli`

Dış sayfalar (2026-11-21, açılan veya arama sonucundan okunan birincil metin):

- [OTIO timeline structure](https://opentimelineio.readthedocs.io/en/latest/tutorials/otio-timeline-structure.html), [opentime.RationalTime](https://opentimelineio.readthedocs.io/en/latest/api/python/opentimelineio.opentime.html)
- [Flicks README](https://github.com/facebookarchive/Flicks/blob/master/README.md) (ayrıca `OculusVR/Flicks` aynı metin)
- [librosa onset_detect](https://librosa.org/doc/latest/api/generated/librosa.onset.onset_detect.html), [beat_track](https://librosa.org/doc/latest/api/generated/librosa.beat.beat_track.html)
- [aubio CLI](https://aubio.org/manual/latest/cli.html)
- [FFmpeg Filters](https://ffmpeg.org/ffmpeg-filters.html) — `silencedetect`, `scale`, `showwavespic`
- [Nuke handling errors](https://learn.foundry.com/nuke/current/content/getting_started/using_interface/handling_errors.html), [Nuke disable node](https://learn.foundry.com/nuke/current/content/getting_started/using_interface/working_nodes.html)
- [After Effects expression errors](https://helpx.adobe.com/after-effects/desktop/work-with-expressions/edit-expressions/troubleshooting-expressions.html)
- [Blender drivers panel](https://docs.blender.org/manual/en/latest/animation/drivers/drivers_panel.html), [node Mute](https://docs.blender.org/manual/en/latest/interface/controls/nodes/node_editors.html), [VSE strip mute](https://docs.blender.org/manual/en/latest/video_editing/edit/montage/strips/strip_properties.html), [VSE text strip](https://docs.blender.org/manual/en/latest/video_editing/edit/montage/strips/text.html)
- [OCIO Looks](https://opencolorio.readthedocs.io/en/stable/guides/authoring/looks.html)
- [Blender Color Balance / ASC-CDL](https://docs.blender.org/manual/en/latest/compositing/types/color/adjust/color_balance.html)
- [MDN object-fit](https://developer.mozilla.org/en-US/docs/Web/CSS/Reference/Properties/object-fit)
- [FFmpeg libdav1d.c](https://github.com/FFmpeg/FFmpeg/blob/master/libavcodec/libdav1d.c)
- [gl-transitions README](https://github.com/gl-transitions/gl-transitions/), [GlitchDisplace.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/GlitchDisplace.glsl)
- [fontconfig-user](https://www.freedesktop.org/software/fontconfig/fontconfig-user.html)
- Resolve düğüm kapatma: [BMD Colorist Guide PDF](https://documents.blackmagicdesign.com/UserManuals/DaVinci-Resolve-20-Colorist-Guide.pdf) ve üçüncü taraf kopya [imaster ch. 143](https://www.imaster.org/docs/davinci-resolve-21-manual/part-10-color/ch.-143-node-editing-basics). Premiere'in güncel EDL sayfası bu oturumda açılmadı.

## Kısa kök nedenler

| # | Kök neden | Sınıf |
| :--- | :--- | :--- |
| 1 | `ClipDef` var; sert kesim birinci sınıf karar değil. Aralık dışı `SourceVideo` şeffaf, kesim el ile `Blend` zinciri. | Eksik yazarlık komutu |
| 2 | Tepeler `Flicks` olarak `frameshift-media` ve MCP'de duruyor. `beat_phase` 120 BPM sentetik. CLI damga basmıyor. | Eksik CLI yolu |
| 3 | Ayrıştırma `validate` ile reddedilir; değerlendirme hatası ve yanlış parametre adı base değerde kalır. `i32` tick taşması bu kodda yok. | Hata yutma |
| 4 | `VectorText` tek dize, font dosyası yok, opaklık yok. Glitch iki girişli geçiş; kendine bağlanınca mozaik sözleşmesi. | Eksik katman + sözleşme |
| 5 | `anime_cyber_contrast` siyahı sabit tutmaz. `radius == 0` halkayı merkeze çökertir, geçiş erken çıkmaz. | LUT matematiği + shader |
| 6 | Düğümde bypass yok. Profil örnek sayısı yalnızca çıkış düğümü `MotionBlurShutter` ise ezer. Dogfood grafiğinde çıkış blur değil. | Eksik bypass + yarım profil kuralı |
| 7 | Decode `ffmpeg -s` ile tuvale gerer. Grafikte cover/crop yok. Codec beyaz listesi yok; encode H.264. | Eksik fit |
| 8 | Varsayılan profil `Final`. Tick girişi ham `i64`. Başarı satırı birimi iki kez basar. `frame --tick` var; insan zamanı, dalga ve tepe damgası yok. | CLI sunumu |

---

## 1. Timeline yokluğu

**Ne oldu.** Yedi sert kesim, her klip için bir `SourceVideo` artı bir `Blend` yığılarak yapıldı. Aralık dışı klibin şeffaf olduğu şemayı okuyunca bulundu.

**Bugün.** `ClipDef` `timeline_start` / `timeline_end` / `source_start` / `speed_curve` tutar (`crates/frameshift-schema/src/project.rs:86-99`). `TimelineDef.clips` bir vektördür; `track_id` yoktur (`project.rs:113-123`). Aralık dışında `ProjectAnimator::resolve_source_tick` `None` döner (`crates/frameshift-render/src/compositor.rs:264-265`) ve `SourceVideo` dokuyu saydam siyaha temizler (`compositor.rs:1059-1064`). `ProjectCommand::AddClip` klibi listeye ekler, grafik düğümü kurmaz (`crates/frameshift-schema/src/session.rs:20-21`). `CreativeDecisionRecord` bir gerekçe günlüğüdür, kesim listesi değildir (`project.rs:191-202`). `ImpactSolver::solve` vardır (`crates/frameshift-time/src/impact_solver.rs:158`); bir kaynağın impact anını hedef tick'e oturtan hız eğrisidir, sert kesim listesi değildir. ADR 0003 metni çözücünün henüz yazılmadığını söyler; bu cümle koda göre eskidir. ADR bu oturumda değiştirilmedi.

Kilitli spec alan adları kodla uyuşmuyor: spec `clip_id`, `track_id`, `output_start_tick`, `output_end_tick`, `source_in_tick` der (`brain/specs/project-schema.md:63-72`). Kod `id`, `timeline_start`, `timeline_end`, `source_start` kullanır. JSON alanlarını spec adına çevirmek mevcut projeleri kırar.

**Kök neden.** Eksik yazarlık komutu ve keşfedilebilirlik. Şeffaflık bir sert kesim uygulamasıdır; editör kararı değildir.

**En iyi düzeltme.** `frameshift-schema` içinde `ProjectCommand::AuthorHardCuts`: sıralı, örtüşmeyen `ClipDef` aralıkları, her klip için bir `SourceVideo`, tek bir `Blend` zinciri. Grafik hâlâ DAG kalır. Yeni düğüm türü ve yeni preset adı yok. Reddedilen: yeni `Sequence` düğümü (kilitli spec'e tür ekler), OTIO dosyasını ikinci zaman tabanı yapmak, UI state içine kesim gömmek.

**Spec / bağımlılık.** Yok, komut mevcut türlerle yazılırsa. Alan adlarını spec'e uydurmak veya `track_id` eklemek spec sorusudur; bu dilim onu yapmaz. Yeni crate yok.

**Risk.** M1 golden sahnesi bu komutu çağırmazsa hash'ler değişmez. `schema_tests` yeni komutu kapsamalı. `verify-all` görüntüsü değişmez.

**Sıra.** Dilim B. Dilim C (`project.rs`) ve Dilim G (`graph.rs`) ile aynı dosyaya girmez. `session.rs` + `schema_tests.rs` + `mcp/server.rs`.

### Dış kaynaklar

- [OTIO — Simple Cut List](https://opentimelineio.readthedocs.io/en/latest/tutorials/otio-timeline-structure.html). Bir `Track` içinde uç uca `Clip`. Her klibin `source_range`'i medyayı kırpar; parça süresi çocukların toplamıdır. **Kopyala:** kesim listesinin kendisi bir parça üzerindeki sıralı kliplerdir; grafik her kesim için elle kurulmaz. **Kopyalama:** `RationalTime`, `value/rate` ve kurucu `SupportsFloat` kabul eder ([opentime](https://opentimelineio.readthedocs.io/en/latest/api/python/opentimelineio.opentime.html)). Bu, kayan saniyeyi kalıcı zaman yapar. FrameShift `Flicks(i64)` kalır (`timebase.md`).
- Blender VSE şeridi kendi kanalında durur ve `Mute` çıkışı keser ([strip properties](https://docs.blender.org/manual/en/latest/video_editing/edit/montage/strips/strip_properties.html)). **Kopyala:** bir kesimin kendi zaman aralığı ve kendi kapatma anahtarı vardır. **Kopyalama:** GUI kısayolları ve şerit state'ini UI'da tutmak.

---

## 2. Beat motorun dışında

**Ne oldu.** Tepeler ffmpeg ve Python ile ölçüldü, Flicks elle yazıldı. `beat_phase` en yakın event'e göre 120 BPM sentetik faz.

**Bugün.** `ProjectAnimator::build_expr_context` beat periyodunu `FLICKS_PER_SECOND / 2` sabitler ve en yakın `target_output_tick`'ten faz üretir (`compositor.rs:181-186`). Disk zarfı yoksa `audio_rms` de bu fazdan sentetiklenir (`compositor.rs:194-200`). Aynı dosyada gerçek zarf, `AudioRmsEnvelope` verilmişse kullanılır. `AudioBuffer::analyze_beats_and_energy` yerel RMS tepesini örnek indeksinden `Flicks`'e çevirir (`crates/frameshift-media/src/audio.rs:591-696`). `detect_peak_in_window` bir pencerenin tepe tick'ini döner (`audio.rs:698-756`). MCP `analyze_audio` bunu çağırır (`crates/frameshift-mcp/src/server.rs:258-279`). `frameshift-cli` içinde `analyze`, `peak` veya `stamp` alt komutu yoktur (`main.rs` eşleşmedi).

Kilitli spec `beat_phase` için "en yakın analiz edilmiş veya işaretli beat aralığı" der (`effect-shader-api.md:62`). Kod analiz edilmiş listeyi okumaz. Bu, spec'e göre sapmadır. M1 dersi: sentetik `BeatAudio120Bpm` gerçek zarfa bağlanırsa golden hash değişir (`brain/lessons.md`).

**Kök neden.** Özellik duruyor, CLI'ye ve `beat_phase`'e bağlı değil. Tespitin kendisi için yeni kütüphane gerekmez.

**En iyi düzeltme.** İki dikiş, iki dosya sahibi:

1. CLI (`main.rs`): mevcut `analyze_beats_and_energy` / `detect_peak_in_window` çıktısını `i64` tick ve yanında türetilmiş saniye olarak basar. Projeyi kendiliğinden yazmaz. İsteğe bağlı `--write-events` ayrı ve varsayılan kapalı kalır; `EventOrigin::Detected` kullanır.
2. `beat_phase` (`compositor.rs`): yalnızca `origin: Detected` (veya açık bir beat listesi) varsa ardışık tick aralığından faz. Aksi halde bugünkü 120 BPM formülü durur. Böylece golden sentetik sahne hash'i korunur.

Reddedilen: librosa veya aubio'yu crate yapmak (yeni bağımlılık, saniye çıktısı), `beat_phase`'i her projede gerçek analizle değiştirmek (M1 hash), tepeyi `f64` saniye olarak projeye yazmak.

**Spec / bağımlılık.** Yok. Spec zaten analiz edilmiş fazı tarif eder. Yeni crate yok.

**Risk.** `beat_phase` formülü golden sahnede değişirse `verify-all` hash'i kırılır. Sentetik dal aynen kalmalı. CLI komutu render hash'ine girmez.

**Sıra.** CLI kısmı Dilim A (`main.rs`). Faz formülü Dilim D (`compositor.rs`). Aynı dosyayı paylaşmazlar. Dilim D, blur sorusu cevaplanana kadar başlamamalı çünkü aynı dosyada blur kuralı da var.

### Dış kaynaklar

- [Flicks README](https://github.com/facebookarchive/Flicks/blob/master/README.md). `1 flick = 1/705600000` saniye. 24 fps kare `29400000` flick. Metin, biriken zaman için kayan nokta kullanılmamasını söyler. `std::chrono::nanoseconds::rep` tamsayıdır (yaygın platformda 64 bit). **Kopyala:** tick tamsayı kalsın. **Kopyalama:** `i32` sayaç. `i32::MAX / 705_600_000 ≈ 3.04` saniye. Dogfood projesi `hit_white` için `5574240000` tick tutuyor (`koleedits_project.json`); bu `i32::MAX` (`2147483647`) üstündedir ve şema bunu `i64` olarak taşıyor.
- [librosa.onset.onset_detect](https://librosa.org/doc/latest/api/generated/librosa.onset.onset_detect.html) ve [beat_track](https://librosa.org/doc/latest/api/generated/librosa.beat.beat_track.html). Tepe seçimi: yerel maksimum, komşu ortalamadan `delta` kadar yüksek, `wait` kadar ayrık. `units='time'` saniye döner. `start_bpm` varsayılanı 120. **Kopyala:** eşik + bekleme. Motorun `analyze_beats_and_energy` zaten yerel maksimum, `1.4 * local_mean` ve 200 ms refractory kullanır (`audio.rs:638-684`). **Kopyalama:** saniyeyi kalıcı alan yapmak, 120 BPM'i gerçek müzikte varsayılan faz saymak.
- [aubioonset](https://aubio.org/manual/latest/cli.html). Varsayılan zaman formatı saniye. `-T` `samples`, `ms` veya `seconds`. Eşik tipik aralığı `[0.001, 0.900]`, varsayılan `0.3`. Minimum aralık varsayılan `0.020` saniye. **Kopyala:** dış araç kullanılacaksa `-T samples`, sonra mevcut `audio_samples_to_ticks`. **Kopyalama:** aubio'yu motora bağlamak (yeni bağımlılık, sor).
- [FFmpeg silencedetect](https://ffmpeg.org/ffmpeg-filters.html). Sessizlik süresi ve gürültü eşiği; zamanlar saniye ve metadata. Tepenin kendisini bir beat listesi olarak basmaz. [astats](https://ffmpeg.org/doxygen/trunk/af__astats_8c_source.html) pencere istatistiği verir, onset listesi vermez. **Kopyala:** sessizlik tespiti beat tespiti değildir. **Kopyalama:** `silencedetect` çıktısını kesim listesi sanmak.

---

## 3. SafeExpr sessiz kalıyor

**Ne oldu.** Bozuk ifade base değerde kaldı. İstenen flaş `step(1.905, t_sec) * (1 - step(1.980, t_sec))` idi. Tick'in `Int32`'ye sığmadığı söylendi.

**Bugün.** Kayıtlı proje bu flaşı `over_stutter.opacity` üzerinde, saniye ile, geçerli bir ifade olarak tutuyor (`koleedits_project.json` param binding). `step` uygulandı (`crates/frameshift-time/src/expr.rs:318-321`). Sayılar `f64` (`expr.rs:382-440`). `output_tick` bağlamda `i64`, okunurken `f64` (`expr.rs:55`, `expr.rs:85-88`). `i32`'ye daraltan bir tick yolu `expr.rs`, `flicks.rs` veya compositor içinde yok.

`Project::validate` ayrıştırma hatasını `InvalidParamBinding` yapar (`project.rs:398-408`). Değerlendirme sırasında iki yutma vardır: `SafeExpr::parse(...).ok()` bağları düşürür (`compositor.rs:155-162`); `eval_default` hatası `if let Ok` ile yok sayılır (`compositor.rs:242-244`). `apply_animated_parameter` tanımsız ada hiçbir şey yapmaz (`compositor.rs:357-508`, `_ => {}`). Sonuç `f32`'ye kesilir (`compositor.rs:243`). `f32` tam sayıları yalnızca `2^24` (`16777216`) kadar kesin tutar. `705_600_000` bu sınırı aşar. `0` ve `1` kesindir; flaş ifadesinin sonucu `f32`'de bozulmaz. `2^53` altındaki tam sayılar `f64`'te kesindir; pratik bir edit'in tick'i bu aralıktadır. Kayıtlı `5574240000` tick `f64`'te kesindir, `i32`'de değildir.

Spec'te olup kodda olmayanlar: `if`, karşılaştırma, `&&`, `||`, `!`, `%`, `mix`, `floor`, `ceil`, `exp`, `sqrt`, `log` (`effect-shader-api.md:69` karşı `expr.rs:126-140`). `ParamBindingDef` spec'teki `clamp_min`, `clamp_max`, `fallback_value` alanlarını taşımaz (`project.rs:183-189`). `MAX_EXPR_DEPTH` kodda `64`, spec'te `16`.

**Kök neden.** Hata yutma ve birim tuzağı. `Int32` taşması bu depoda bir bug olarak durmuyor. Tick'i `t_sec` ile karşılaştırmak hata vermeden `0` üretir. Yanlış `param_name` de hata vermez.

**En iyi düzeltme.** `frameshift-schema` `validate`: `param_name` düğüm türünün beyaz listesinde değilse `InvalidParamBinding`. `frameshift-render` `ProjectAnimator`: `eval_default` hatasını yutma; kare değerlendirmesi `RenderError` ile dursun. Base değerde sessiz kalma kalksın. Kalıcı zaman `Flicks(i64)` kalsın. İfade içinde tick karşılaştırması gerekiyorsa mevcut `output_tick` (`f64`, `2^53` altında kesin) kullanılsın. Yeni `i32` veya `f64` zaman tabanı yok.

Reddedilen: tick'i `f32` parametreye yazmak, `from_seconds_f64_approx` ile kalıcı ankraj üretmek, spec'teki eksik operatörlerin tamamını bu dilimde yazmak (ayrı, daha geniş iş; flaş ifadesi `step` ile zaten kuruluyor).

**Spec / bağımlılık.** Yok. Spec zaten hata ve sonluluk ister (`effect-shader-api.md:102-103`). `fallback_value` eklemek spec alanıdır; sessiz fallback'i geri getirir. Eklenmemeli.

**Risk.** Bugün yanlış parametre adıyla render alan bir proje yüklenemez veya karede durur. Golden sahnedeki bağlar geçerli adlara bakıyor (`flash_amount`). Yeni şema testi şart. Hash, geçerli sahnede değişmez.

**Sıra.** Beyaz liste Dilim C (`project.rs`). Yutmayı kaldırmak Dilim D (`compositor.rs`). Dosyalar ayrı. Dilim D blur sorusu yüzünden bekler; beyaz liste beklemez. İkisi birlikte "sessiz ifade bitti" demez: yükleme yanlış adı reddeder, kare içi `eval` hatası Dilim D'ye kalır.

### Dış kaynaklar

- [Nuke — Handling Errors](https://learn.foundry.com/nuke/current/content/getting_started/using_interface/handling_errors.html). Hata düğümde ve Error Console'da görünür. Bağlı olmayan hatalı düğüm popup açmaz; görüntü yine bakılabilir. **Kopyala:** hatanın metnini ve düğüm kimliğini yüzeye çıkar. **Kopyalama:** hatalı ifadeyle base değerde render'a devam etmek.
- [After Effects expression errors](https://helpx.adobe.com/after-effects/desktop/work-with-expressions/edit-expressions/troubleshooting-expressions.html) (sayfa 2026-05-05). Aynı sayfa iki politika yazar: banner ifadenin kapanmadığını söyler; sonraki bölüm işlenemeyen ifadeyi otomatik kapatır. **Kopyala:** hatayı gizleme. **Kopyalama:** banner'ı tercih ile susturmak ve hangi cümlenin geçerli olduğunu seçmeden AE'yi taklit etmek. FrameShift spec'i hata döndürür; base değer dönüşü spec'te yok.
- [Blender drivers](https://docs.blender.org/manual/en/latest/animation/drivers/drivers_panel.html). Basit ifade alt kümesi ve dışarıdaki ifade için "Slow Python expression" uyarısı. Başarısız sürücünün eski değerde kaldığı cümle bu sayfada yok; onu iddia etmiyorum. **Kopyalama:** `frame` kayan sayısını kalıcı zaman yapmak.

---

## 4. Yazı tek dize; glitch okunmaz oluyor

**Ne oldu.** `DRIFT` kendi blend'i ve opaklık penceresi istedi. Glitch `progress` 0.28 kareyi kendisiyle karıştırıp mozaik yaptı; 0.035 okunur kaldı. Font dosyası yok.

**Bugün.** `NodeKind::VectorText` tek `text: String` taşır; `font_family` / dosya yolu yoktur (`crates/frameshift-schema/src/graph.rs:159-171`). Raster, gömülü ASCII SDF atlasıdır (`crates/frameshift-vector/src/font.rs:169-172`). `VectorTextConfig::from_node_kind` font alanı okumaz (`crates/frameshift-vector/src/text.rs:89-113`). `apply_animated_parameter` yazıda yalnızca `font_size_norm`, `outline_width_norm`, `y_offset_norm` değiştirir; alfa değişmez (`compositor.rs:421-430`). Opaklık, yazının üstündeki `Blend` üzerinden gelir. Kayıtlı proje bunu yapıyor: `title_drift` ayrı, `over_text.opacity` = `step(3.30, t_sec) * (1 - step(4.46, t_sec))`.

`glitch_card` iki girişini de `over_text`'e bağlar (`koleedits_project.json`). `transition_glitch_displace.wgsl` `progress <= 0` iken yalnızca `tex_a`, `progress >= 1` iken yalnızca `tex_b` döner (`shaders/transition_glitch_displace.wgsl:145-149`). Arada voronoi kaydırma ve `luma * 2` gri katman karışır (`wgsl:170-176`). Bu, kendine bağlanmış iki girişte mozaik üretir. 0.035, `smoothstep(0.0, 0.5, p)` küçük kaldığı için kaydırma küçük kalır. Bu bir bozuk uniform değil.

Spec `font_family`, `font_weight`, hizalama ve vektör yolu ister (`project-schema.md:111-113`). `render-graph.md:110` eksik fontta sessiz rastgele fontu yasaklar, `FontNotFound` der. Kod font aramaz; her zaman atlasa düşer. Bu, spec'e göre sapmadır.

**Kök neden.** Yazı zamanı için ayrı düğüm zaten mümkün; keşif ve font eksik. Glitch parametresi bir baharat değil, iki doku arasındaki geçiş ilerlemesidir.

**En iyi düzeltme.** Shader matematiğini değiştirme. Okunur glitch: `progress` 0'a yakın, `whip_strength` düşük, `tex_b` farklı bir plaka ya da aynı plakanın mozaik olacağı bilinerek. Zamanlanmış sözcük: ayrı `VectorText` + `Blend` opaklığı. Bunu Dilim B'nin komutu üretebilir (`AuthorOverlay`). Font dosyası bu turda yok. `ttf-parser` / `fontdue` / `ab_glyph` ancak kullanıcı onayından sonra. Onaya kadar atlas sözleşmesi açık yazılır; rastgele sistem fontu seçilmez.

Reddedilen: glitch'i tek girişli "okunur" preset yapmak (kilitli geçiş sözleşmesini ve MIT portunu bozar), fontconfig ile her zaman bir font döndürmek (spec `FontNotFound` ister).

**Spec / bağımlılık.** Glitch için yok. Font parser: **ask user first**. Soru 5.

**Risk.** Shader'a dokunulmazsa 15 WGSL ve golden hash durur. Font crate'i eklenirse `VectorText` pikselleri değişir; golden'daki başlık hash'i kırılır. O yüzden parser bu araştırmanın uygulama dilimi değil.

**Sıra.** Yazı penceresi Dilim B (`session.rs`). Font bekler. Glitch için kod dilimi yok.

### Dış kaynaklar

- [Blender Text strip](https://docs.blender.org/manual/en/latest/video_editing/edit/montage/strips/text.html). Yazı kendi şeridi, kendi font dosyası, kendi rengi. Opaklık şerit özelliği ([strip properties](https://docs.blender.org/manual/en/latest/video_editing/edit/montage/strips/strip_properties.html)). **Kopyala:** sözcük, plakadan ayrı bir katmandır; opaklık penceresi yazının kendisindedir, plakanın içinde değil. **Kopyalama:** Blender'ın gömülü fontunu veya GUI preset'lerini (`Subtitle`, `Main Title`) kopyalamak.
- [fontconfig](https://www.freedesktop.org/software/fontconfig/fontconfig-user.html). Eşleşme her zaman bir font döndürür; istenene benzemeyebilir. **Kopyalama:** bu "her zaman bir şey döner" kuralı. FrameShift spec'i sessiz rastgele fontu yasaklar.
- [gl-transitions](https://github.com/gl-transitions/gl-transitions/). `progress` 0.0 yalnız `from`, 1.0 yalnız `to`. [GlitchDisplace.glsl](https://github.com/gl-transitions/gl-transitions/blob/master/transitions/GlitchDisplace.glsl) gri katmanı `*= 2.0` yapar ve `smoothstep(0.0, 0.5, progress)` ile karıştırır. **Kopyala:** 0 ve 1 uçları kimliktir; ara değer bir geçiştir. Okunur bir üst sınır olarak "0.28 güvenli" diye bir sayı bu kaynakta yok. **Kopyalama:** ara `progress`'i hafif bir baharat sanmak.

---

## 5. Grade sırası siyahı boyar; shockwave nokta çizer

**Ne oldu.** Siyah flaş LUT'tan önce koyu mora döndü, LUT daha sonraya alınca düz siyah kaldı. `RadialShockwave` yarıçap 0 iken merkezde nokta kaldı.

**Bugün.** Sıra yazara aittir. Motor düğümleri yeniden sıralamaz. Golden sahnede başlık LUT'tan önce birleşir (`project.rs:681-707`, çıkış `motion_blur_out`, `project.rs:581`). `lut3d.wgsl` RGB'yi LUT ile karıştırır (`shaders/lut3d.wgsl:49-61`); alfa `0` ise erken şeffaf döner, opak siyah LUT'a girer.

`cyber_contrast_grade(0,0,0)` siyahı sabit tutmaz (`crates/frameshift-render/src/lut.rs:333-358`). `s_curve(0) = 0`, luma `0`, `shadow_w = 1`. Sonuç yaklaşık `R = 0`, `G = 0.045`, `B = 0.095` (doğrusal). Bu, formülden okunmuş bir değerdir; dogfood PNG'si bu oturumda ölçülmedi. Günlük o kareyi koyu mor diye anlatır. Renk adı ile formül aynı piksel ölçümü değildir. `ColorBasic` kontrastı `0.18` etrafında döner (`shaders/color_basic.wgsl:97-99`); kontrast `1` iken siyah `0` kalır. `flash_amount > 0` siyaha ışık ekler (`color_basic.wgsl:106-113`).

Shockwave erken çıkmaz (`shaders/fx_radial_shockwave.wgsl:49-66`). `ring_mask`, `r == radius` civarında zirve yapar. `radius == 0` zirveyi merkeze koyar; `ring_width` en az `1e-4`. 16 taplık zoom döngüsü her zaman çalışır (`wgsl:80-96`). Kayıtlı `shock` düğümü `radius: 0`, `refraction_amp: 0`, `zoom_blur_strength: 0`, `chromatic_aberration: 0.02`, `ring_width: 0.07`. Bağlar yarıçapı `(event_delta_sec - 0.04)` ile geciktirir, zoom'u ise `t_sec` ile `1.67` ve `5.60` civarında açar. Yani yarıçap hâlâ `0` iken zoom ayrı bir parametre olarak durabilir. Merkez noktası, çökmüş halka artı sabit `0.02` CA ile uyumludur. Bunu bu oturumda piksel ölçerek doğrulamadım.

**Kök neden.** LUT preset'i siyahı bilerek iter; bu bir sıra hatası değil. Siyahı LUT'tan sonra kompozitlemek zaten düz siyah verir. Shockwave'de `radius == 0` "efekt yok" değildir.

**En iyi düzeltme.** LUT eğrisini değiştirme. Siyah, flaş ve yazı grade'ten sonra `Blend` ile girer. Otomatik yeniden sıralama golden başlığı LUT'un önünden alır ve her kareyi değiştirir. Shockwave: `radius <= 0` iken `ring_mask = 0` (merkez kırılımı yok). `zoom_blur_strength` ayrı kalsın; kayıtlı proje onu yarıçap gecikmesiyle birlikte kullanıyor. Tam erken çıkış, `1.67` zoom'unu da siler.

Reddedilen: `cyber_contrast_grade` siyahını `0` yapmak (tüm grade'li kareler ve LUT testleri değişir), DAG'ı "grafikler her zaman sonda" diye yeniden yazmak, CSS/DOM katmanı.

**Spec / bağımlılık.** LUT için yok. Shockwave yarıçap kuralı kilitli spec cümlesi değil; shader davranışıdır. Soru 1 cevabı "tüm düğümü kapat" ise shader değişir, zoom anlamı da değişir.

**Risk.** LUT'a dokunulmazsa renk testleri durur. Halka maskesini sıfırlamak, yarıçapı `0` olan karelerde merkez kaymasını değiştirir. Zoom aynı kalırsa `1.67` enerji patlaması durur; o patlama impact + zoom + flaş yığınındandır (madde 6), tek başına shockwave noktasından değil.

**Sıra.** Dilim E, yalnızca `fx_radial_shockwave.wgsl` ve yeni bir test dosyası. `compositor.rs` ve `compositor_tests.rs` Dilim D'ye aittir; E onlara girmez. Soru 1 "halkayı kapat, zoom kalsın" ise E bloklu değildir. "Tüm efekti kapat" seçilirse E'nin kabul testi zoom'un da sıfırlandığını arar.

### Dış kaynaklar

- [OCIO Looks](https://opencolorio.readthedocs.io/en/stable/guides/authoring/looks.html). Look, `process_space` içinde uygulanır: giriş o uzaya çevrilir, transform çalışır, çıkış uzayına dönülür. 3D LUT'un tersi otomatik değildir. **Kopyala:** grade, üzerinde durduğu piksele uygulanır; sonraki bir `Blend` grade'i geri almaz. **Kopyalama:** OCIO'yu bu dilimde bağlamak (yeni bağımlılık, sor). Motorun LUT'si zaten analitik bir küp.
- [Blender ASC-CDL](https://docs.blender.org/manual/en/latest/compositing/types/color/adjust/color_balance.html). `out = (i * s + o) ^ p`. Offset `0` değilse siyah yerinden oynar. Slope `1` ve offset `0` iken siyah durur. **Kopyala:** siyahın durması, eğrinin `(0,0,0)` sabit noktası olmasına bağlıdır. `anime_cyber_contrast` offset benzeri bir gölge itmesi yapar (`+0.045 G`, `+0.095 B`). **Kopyalama:** CDL'i yeni bir preset yığını olarak eklemek.
- Shockwave için "yarıçap 0 ise çizme" diyen bir birincil shader sayfası bu oturumda açılmadı. Öneri, okunan WGSL matematiğinden.

---

## 6. Yığın solo'suz; profil örnek sayısını eziyor

**Ne oldu.** Impact + zoom blur + flaş, `t = 1.67` civarını beyaz bir patlamaya çevirdi. Draft profilinin, düğümdeki motion-blur örnek sayısını ezdiği söylendi.

**Bugün.** `GraphNodeDef` üzerinde `enabled` / solo / bypass yoktur (`graph.rs:321-328`). Kapatmak, elde olan `mix_amount` veya `Blend.opacity` ile sınırlıdır. `RadialShockwave` ve `MotionBlurShutter` için ortak bir kapı yoktur (`compositor.rs:505-507` blur parametresini animasyondan da almaz).

Profil enjeksiyonu: `render_project_frame_with_audio`, seçeneklerde override yoksa profilin `shutter_angle_deg` ve `motion_blur_sub_samples` değerlerini `ShutterOverride` yapar (`compositor.rs:796-801`). Bu override yalnızca `evaluate_subgraph` hedefi `MotionBlurShutter` ise uygulanır (`compositor.rs:834-855`). İçerideki blur düğümü kendi `sub_samples` değerini kullanır (`compositor.rs:892-915`).

Golden sahne çıkışı `motion_blur_out` (`project.rs:581`), düğüm `sub_samples: 8` (`project.rs:698-707`), draft profil `4` (`project.rs:711-718`). Bu yolda draft, düğümün 8'ini 4 yapar. `verify-all` draft profili ayrıca `1` yapar (`crates/frameshift-core/src/engine.rs:296-298`) ve kapı `draft_sub_samples == 1` ile `final_hash != draft_hash` ister (`engine.rs:414-416`). Zaman ankrajlarının aynı kaldığı ayrı kontrol edilir.

Dogfood kaydı: `output_node` `over_end` (bir `Blend`), `motion_blur_out` onun girdisidir, düğüm `sub_samples: 2`, her iki profil de `2` (`koleedits_project.json`). Bu grafikte profil, iç blur'u ezmez; sayılar zaten eşit. Günlükteki "profil eziyor" cümlesi bu dosyada bir sayı farkı olarak durmuyor. Ezme, golden'daki kök blur yolunda duruyor. İç blur'da profil alanı bugün etkisizdir. İki kaynak, iki farklı sessizlik.

`t = 1.67` yığını kayıtlarda: impact `mix_amount`, shock `zoom_blur_strength` (`t_sec - 1.67`), grade `flash_amount`. Solo olmadığı için üçü birlikte açılır. Bu oturum o kareyi yeniden render etmedi.

**Kök neden.** Bypass eksik. Profil kuralı yarım: yalnız kök blur'da ezer, iç blur'da yok sayar. Dogfood dosyası ezmenin örneği değil.

**En iyi düzeltme.** Her `MotionBlurShutter` kendi `sub_samples` değerini kullanır. Profil ancak açık bir tavan istenirse ve CLI başarı satırı `node N, effective M` yazarsa düşürür. `verify-all` 1 örnekli kapıyı profil alanına gizlice güvenmek yerine açık `ShutterOverride` ile korur. Zamanlama ve maske aynı kalır; değişen yalnız integral örnek sayısıdır (`project-schema.md:160`).

Bypass: `GraphNodeDef.enabled` varsayılan `true`. `false` iken 1 girişli düğüm girişi geçirir, 2 girişli düğüm base'i döner. Bu, Blender Mute ve Nuke Disable ile aynı fikirdir. Solo ayrı bir mod olmasın; kapatılmayan düğümler durur. `enabled` eklemek, `GraphNodeDef { ... }` yazan her dosyayı derlemede kırar: `project.rs`, `compositor.rs`, `compositor_tests.rs`, `inspection.rs`, `schema_tests.rs`, `core_tests.rs`, `snapshot.rs`. Bu yüzden bypass, o dosyaların sahipleriyle paralel değildir.

Reddedilen: draft'ı her zaman `min(düğüm, profil)` yapmak (yüksek açık değeri yine keser), kök override'ı silip verify kapısını güncellememek (golden draft hash'i final ile aynılaşıp kapı düşer), preview'da maske veya kesim zamanını değiştirmek.

**Spec / bağımlılık.** Örnek sayısı politikası spec cümlesini bozmaz; spec farka izin verir, farkı zorunlu kılmaz. `enabled` alanı kilitli şemada yok. **Ask user first.** Soru 3 ve Soru 4.

**Risk.** Override kalkıp verify hâlâ `1` yazmazsa M1 motion-blur kapısı düşer. `enabled` varsayılan `true` ise mevcut JSON ve mevcut literal'ler tamamlandıktan sonra görüntü değişmez.

**Sıra.** Blur politikası Dilim D: `compositor.rs`, `engine.rs`. Bypass alanı Dilim G: yukarıdaki literal dosyalarının hepsi. G ile D paralel değil. G, soru 4'e bloklu. D, soru 3'e bloklu.

### Dış kaynaklar

- [Blender node Mute](https://docs.blender.org/manual/en/latest/interface/controls/nodes/node_editors.html). Mute düğümü silmez; mümkünse girdileri çıktıya geçirir. **Kopyala:** bypass kimliği ve kenarları durur. **Kopyalama:** GUI.
- [Nuke Disable](https://learn.foundry.com/nuke/current/content/getting_started/using_interface/working_nodes.html). `D` düğümün etkisini keser. İfade ile de kapanabilir. **Kopyala:** kapatmak açık bir durumdur, örnek sayısını gizlice değiştirmek değildir.
- [Resolve Colorist Guide](https://documents.blackmagicdesign.com/UserManuals/DaVinci-Resolve-20-Colorist-Guide.pdf) ve [imaster ch. 143](https://www.imaster.org/docs/davinci-resolve-21-manual/part-10-color/ch.-143-node-editing-basics). Kapatılan düğüm render'a girmez ve grade saklanınca kapalı kalır. Tümünü açmak, önceden tek tek kapanmış düğümleri de açar. **Kopyala:** kapalı kalmak proje durumudur. **Kopyalama:** "hepsini aç"ın seçici kapıları unutması. FrameShift'te `enabled` varsayılanı tek tek durur.
- Preview/final: kilitli spec yalnızca çözünürlük, örnek sayısı ve filtre kalitesinin ayrılmasına izin verir (`project-schema.md:160`). Dış araçlardaki "proxy başka kare zamanı" modeli alınmaz.

---

## 7. Cover-crop grafikte yok

**Ne oldu.** 16:9 kaynak, tuval `960×740` (35:27) için ffmpeg ile dışarıda cover-crop edildi. Oyun altyazısı ancak kırpılınca gitti. AV1, ffmpeg sayesinde açıldı; kullanıcı yine H.264'e çevirdi.

**Bugün.** `FfmpegBridge::decode_video_frame` ve `decode_video_window` ölçeği `-s {width}x{height}` ile ister (`crates/frameshift-media/src/ffmpeg.rs:482-497` ve `:368-380`). Bu, en-boy koruması olmadan hedef kutuya gerer. FFmpeg belgesi buna `force_original_aspect_ratio=disable` der. Codec seçilmez; dosya `ffmpeg -i` ile açılır. Eldeki ffmpeg hangi decoder ile kurulduysa o çalışır. Bu oturumda AV1 dosyası decode edilmedi. Encode yolu `libx264` ve `yuv420p` (`ffmpeg.rs:586-689`). Donanım encoder da H.264 ailesindedir (`ffmpeg.rs:47-53`).

`Transform2D` ölçekler; kaynak UV dışına çıkan yer şeffaftır (`shaders/transform.wgsl:124-138`). Bu, decode'dan önce gerilmiş bir doku üzerinde zoom'dur. Cover, gerilmeden önce gerekir. `upload_source_frame` gelen kareyi tuval dokusuna tam ekran basar (`compositor.rs:1633-1642`); kare zaten istenen genişlik ve yüksekliktedir (`resolver.rs:139` civarı `decode_video_frame(..., width, height)`).

**Kök neden.** Fit modu yok. Decode sözleşmesi "tam olarak WxH döndür" ve bunu gererek yapar. Codec matrisi kodda yazılı değil.

**En iyi düzeltme.** Varsayılan `stretch` kalsın; bugünkü pikseller değişmesin. Opt-in `cover` ve `contain`, kaynak en-boyunu korur. Tercih: fit, proje verisi olsun (`SourceVideo` alanı) ve hem draft hem final aynı fonksiyonu çağırsın. Decode fonksiyonunun mevcut `-s` imzası bozulmasın; yeni bir yol eklensin. Sistem ffmpeg'inin `scale` + `crop` filtresi yeni bir FFmpeg derlemesi değildir. Yine de asıl yer grafiktir; gizli bir ön-transcode, proje dosyasında görünmez.

AV1: motor decoder seçmez. Kurulu `ffmpeg` `libdav1d` (veya başka bir AV1 decoder) içeriyorsa açılır. İçermiyorsa açılmaz. Bunu bu makinede ölçmedim. H.264 transcode, encode çıktısı için zaten zorunludur (`libx264`). Decode için kod bir transcode şartı koymaz. Kullanıcının H.264'e çevirmesi, bu kodun AV1'i reddettiğini göstermez.

Reddedilen: her kaynağı her zaman cover yapmak (mevcut kareler değişir), yeni FFmpeg build veya `dav1d` crate'i, web `object-fit`.

**Spec / bağımlılık.** `SourceVideo` alanına `fit` eklemek kilitli şema editidir. **Ask user first.** Soru 2. Yeni FFmpeg yok.

**Risk.** Varsayılan `stretch` ise `media_tests` ve Asta kareleri durur. `cover` varsayılan yapılırsa gerçek medya karelerinin kompozisyonu değişir.

**Sıra.** Dilim F: `ffmpeg.rs`, `resolver.rs`, `media_tests.rs`. `graph.rs` Dilim G. F, alan G inene kadar proje JSON'undan cover seçemez. F ve G paralel değil. F, CLI ve shockwave shader ile paralel olabilir.

### Dış kaynaklar

- [MDN object-fit](https://developer.mozilla.org/en-US/docs/Web/CSS/Reference/Properties/object-fit). `cover` oranı korur ve taşanı keser. `contain` sığdırır, bant bırakır. `fill` gerer ve başlangıç değeridir. **Kopyala:** üç kelime. Bugünkü `-s` = `fill`. **Kopyalama:** CSS motoru.
- [FFmpeg scale](https://ffmpeg.org/ffmpeg-filters.html) `force_original_aspect_ratio`. `increase` kutuyu doldurana kadar büyütür; ardından `crop` cover'dır. `decrease` + `pad` contain'dır. **Kopyala:** bu filtre çifti, sistem ffmpeg'inde varsa, yeni derleme değildir. **Kopyalama:** fit'i yalnız CLI ön-adımında bırakıp grafikten gizlemek (draft/final aynı kalır ama proje kararı görünmez).
- [FFmpeg libdav1d.c](https://github.com/FFmpeg/FFmpeg/blob/master/libavcodec/libdav1d.c). FFmpeg AV1'i dav1d sarmalayıcısıyla çözer. **Kopyala:** decode yeteneği, bizim crate'imizin codec listesi değil, ffmpeg binary'sinin decoder'ıdır. **Kopyalama:** `dav1d` veya `ffmpeg-sys` bağlamak (sor).

---

## 8. CLI

**Ne oldu.** Varsayılan profil final. Tick ham `i64`. Başarı satırı "24 fps fps". Dalga formu ve kullanılabilir "bu saniyeyi bas" yok.

**Bugün.** `render` ve `frame` profili yazılmazsa `RenderProfileKind::Final` (`crates/frameshift-cli/src/main.rs:548` ve `:572`). `frame --tick` vardır ve tek kare basar (`main.rs:213-236`, ayrıştırma `:575-579`). Giriş `i64::parse`. `inspect` tick almaz (`main.rs:612-628`); paket, seçilmiş ana tick'lerde final profille çıkar (`inspection.rs`).

`Rational32` gösterimi payda `1` iken `"{num} fps"` yazar (`crates/frameshift-time/src/rational.rs:159-166`). CLI formatı `{} fps` ekler (`main.rs:202-207`). 24/1 için okunan dize `24 fps fps` olur. Bu oturumda ikili çalıştırılmadı; sonuç format dizisi ile `Display` birleşimidir. `Flicks` gösterimi zaten `"{ticks} flicks ({seconds}s)"` (`flicks.rs:146-149`). `frame` başarı satırı tick'i bu `Display` ile basar. Eksik olan giriş tarafıdır.

Dalga ve tepe alt komutu yoktur. `from_seconds_f64_approx` vardır ve kendisi kalıcı kaynak olmadığını söyler (`flicks.rs:63-76`). `1.905` ikili `f64`'te kesin değildir (`1905/1000`). Üç haneli saniye, milisaniye tam sayısıdır: `1905 * 705_600 = 1_344_168_000`. Bunu `f64` çarpımına bırakmamak gerekir.

**Kök neden.** Sunum ve giriş. Tek kare yolu yok değildir. İnsan zamanı, birim ve tepe/dalga yolu yoktur. Varsayılan `Final`, kodda doğrudur.

**En iyi düzeltme.** Yalnız `frameshift-cli/src/main.rs`:

- Başarı satırı profil adını bassın. FPS için `Display` kullanıp üstüne `fps` eklemesin. `24/1` için bir kez `fps`.
- Zaman belirteci: ham tick; `1.905s` ondalığı milisaniye tam sayısı (`ms * 705_600`) veya daha uzun kesirde tam sayı oran; `f:10` proje fps'i ile `frame_index * frame_duration`. Depolanan değer `i64`. `from_seconds_f64_approx` bu kapının varsayılanı olmasın.
- `frame --at` bu belirtecin diğer adıdır. `frame --tick` durur.
- `peaks` mevcut `AudioBuffer` API'sini çağırır, tick basar. İsteğe bağlı dalga PNG'si `Rgba8Frame` ile yazılır. `showwavespic` bir UI framework değildir; çağırmak da yeni pencere değildir. Yeni crate yok.
- Varsayılan profil `Final` kalsın. Unutulan bayrak ucuz taslağı "asıl görüntü" diye basmasın. Satırda profil görünsün.

Reddedilen: GUI, tick'i `f64` saniye olarak JSON'a yazmak, varsayılanı sessizce `draft` yapmak.

**Spec / bağımlılık.** Yok.

**Risk.** CLI birim testleri format dizesine bakmıyorsa hash kapısı değişmez. `frame` başarı satırı `Display` kullandığı için tick metni zaten saniye ekler; onu bozmamak gerekir.

**Sıra.** Dilim A. Başka dilimle dosya paylaşmaz. Blok yok.

### Dış kaynaklar

- [FFmpeg showwavespic](https://ffmpeg.org/ffmpeg-filters.html). Tek kare dalga PNG'si. `filter=peak` tepeyi, `average` ortalamayı çizer. Zaman damgası basmaz. **Kopyala:** dalga bir resimdir, bir pencere framework'ü değildir. **Kopyalama:** dalgayı tek başına kesim otoritesi yapmak.
- [OTIO RationalTime](https://opentimelineio.readthedocs.io/en/latest/api/python/opentimelineio.opentime.html). Değer bir hızda tutulur ve başka hıza ölçeklenir. **Kopyala:** insan girişi "24 fps'te kare 10" ise tick'e çevrilir. **Kopyalama:** `SupportsFloat` saniyeyi proje alanı yapmak.
- [Flicks README](https://github.com/facebookarchive/Flicks/blob/master/README.md). 24 fps = `29400000` flick. **Kopyala:** kare numarası `index * frame_ticks`. **Kopyalama:** saniyeyi biriktirmek.
- "fps fps" sınıfı: birim, değerin `Display` uygulamasında zaten varsa format dizesi birimi tekrar etmez. Bu, okunan `rational.rs:161` ve `main.rs:203` birleşimidir.

---

## Yapılmayacaklar

- Kalıcı zaman veya kaynak ankrajı için `f64` saniye. CLI sınırı bile ondalık saniyeyi tam sayı milisaniye veya tam sayı oranla `i64`'e çevirir.
- Tick'i `i32` veya `f32` yapmak. `f32` parametreye tick yazmak.
- `SafeExpr` hatasında base değere düşmek. `fallback_value` ile sessizliği spec'e geri yazmak.
- Golden sahneyi "yazı ve siyah her zaman LUT'tan sonra" diye otomatik yeniden sıralamak.
- `anime_cyber_contrast` siyah noktasını değiştirmek.
- `beat_phase`'i sentetik golden sahnede gerçek analizle değiştirmek.
- librosa, aubio, fontconfig, `ttf-parser`, `dav1d` veya yeni bir FFmpeg derlemesi eklemek (onaysız).
- Glitch geçiş matematiğini "okunur preset" diye değiştirmek.
- Draft'ta kesim zamanını veya maske anlamını değiştirmek.
- Web/DOM kompozitör.
- İkinci bir zaman çizelgesi (OTIO dosyası motorun zamanı olmaz). OTIO ancak dışa aktarma konuşulursa ve tick'ler `Flicks` kalırsa.
- fontconfig'in "her zaman bir font döner" kuralı.
- `GraphNodeDef` alanını, literal yazan diğer dilimlerle aynı anda eklemek.

## Kullanıcıya sorulacaklar

Bunlar uygulama başlamadan cevaplanmalı. Kilitli spec'ten çıkanlar sorulmaz: zaman tabanı `Flicks(i64)`, draft/final zaman ve maske anlamı aynı, grafik DAG, web kompozitör yok.

1. **Shockwave `radius <= 0` iken ne kapansın?** Kayıtlı proje zoom'u yarıçap gecikmesinden ayrı sürüyor. Tam kapatma, `t ≈ 1.67` zoom'unu da siler.
   - (Önerilen) Halka maskesi sıfırlansın; merkez noktası gitsin. `zoom_blur_strength` ayrı parametre olarak kalsın.
   - Yarıçap `0` ise düğümün tamamı kaynak pikseli döndürsün (zoom dahil).
   - Shader değişmesin; yalnızca sözleşme yazılsın.

2. **Cover/crop kilitli şemaya girsin mi?**
   - (Önerilen) `SourceVideo` üzerine opt-in `fit`: varsayılan `stretch` (bugünkü piksel), ayrıca `cover` ve `contain`. Yeni FFmpeg derlemesi yok.
   - Decode her zaman cover yapsın. Mevcut gerçek medya kareleri değişir.
   - Fit motorun dışında, ffmpeg ön-adımında kalsın.

3. **Motion blur örnek sayısı kimin?** Golden çıkış blur'dur; düğüm `8`, draft profil `4`, verify draft'ı `1` yapıp hash farkı arar. Dogfood çıkışı blur değildir; o dosyada ezme yoktur.
   - (Önerilen) Her blur düğümü kendi `sub_samples` değerini kullanır. Profil ancak açık tavan istenirse düşürür ve CLI efektif sayıyı yazar. `verify-all` 1 örnek kapısını açık override ile korur. Zaman ve maske aynı kalır.
   - Draft her zaman `min(düğüm, profil)` kullanır. Yüksek açık değer yine kesilir.
   - Kök override kalsın; CLI yalnızca uyarsın.

4. **Düğüm bypass alanı eklensin mi?** Alan, `GraphNodeDef` literal'i yazan yedi dosyayı birlikte ister. O dosyalara giren dilimlerle paralel yapılamaz.
   - (Önerilen) `enabled: bool`, varsayılan `true`. 1 giriş geçirilir, 2 girişte base döner. Ayrı solo modu yok.
   - Alan eklenmesin. Kapatma, elde olan `opacity` / `mix_amount` ile kalsın. Shockwave ve blur için ortak kapı olmaz.
   - Solo: seçilen düğüm dışında efekt dalları kapansın.

5. **Font dosyası?**
   - (Önerilen) Bu turda gömülü ASCII atlas kalsın. Rastgele sistem fontu seçilmesin. Yeni crate yok.
   - `ttf-parser`, `fontdue` veya `ab_glyph` eklensin. Yeni bağımlılık; golden yazı pikselleri değişebilir.
   - fontconfig veya başka bir C font kütüphanesi. Spec'teki `FontNotFound` ile "her zaman bir font" çelişir.

## Uygulayıcı dilimleri

Ortak dosya yoksa paralel. `GraphNodeDef` literal'i ekleyen iş hepsini kilitler.

| Dilim | Dosyalar | Blok | Kabul | Girmemesi gerekenler |
| :--- | :--- | :--- | :--- | :--- |
| A CLI | `crates/frameshift-cli/src/main.rs` | Yok | Birim test: `24/1` metninde `fps` bir kez; `1.905s` → `1344168000`; `frame --at` aynı tick | `compositor.rs`, `graph.rs`, `project.rs`, `ffmpeg.rs`, shader |
| B Kesim listesi | `session.rs`, `schema_tests.rs`, `mcp/server.rs` | Yok | Komut N örtüşmeyen `ClipDef` ve geçerli bir `Blend` zinciri üretir; zamanlar `Flicks` | `project.rs`, `graph.rs`, `compositor.rs`, `main.rs` |
| C Bağlama adı | `project.rs` | Yok | Bilinmeyen `param_name` `validate` hatası | `session.rs`, `compositor.rs`, `graph.rs` |
| D Değerlendirme | `compositor.rs`, `engine.rs`, `expr.rs` | Soru 3. Bypass okuması da bu dosyaysa Soru 4 ve Dilim G bittikten sonra | `eval` hatası yutulmaz. Sentetik 120 BPM golden yolu aynı kalır. Verify 1 örnek kapısı durur ve zaman alanları eşit kalır | `main.rs`, `ffmpeg.rs`, `fx_radial_shockwave.wgsl`, `project.rs` |
| E Shockwave halkası | `shaders/fx_radial_shockwave.wgsl`, yeni `crates/frameshift-render/tests/shockwave_zero.rs` | Soru 1 "yalnız sözleşme" ise dilim yok. "Halka kapansın" ise blok yok. "Tüm düğüm kapansın" ise test zoom'u da sıfır arar | `radius == 0` iken merkez halka kayması yok. Önerilen cevapta zoom ayrı durur | `compositor.rs`, `compositor_tests.rs`, `lut.rs` |
| F Fit API | `ffmpeg.rs`, `resolver.rs`, `media_tests.rs` | Soru 2. JSON alanı Dilim G'den gelir; F, G ile paralel değil | Varsayılan çağrı bugünkü gerilmiş boyutu korur. `cover` oranı korur ve taşanı atar | `graph.rs`, `main.rs`, `compositor.rs` |
| G Şema alanı | `graph.rs` ve `GraphNodeDef {` yazan dosyalar: `project.rs`, `compositor.rs`, `compositor_tests.rs`, `inspection.rs`, `schema_tests.rs`, `core_tests.rs`, `snapshot.rs` | Soru 2 ve Soru 4. Soru 5 "crate ekle" değilse font alanı eklenmez | `enabled` varsayılan `true` ile mevcut JSON yüklenir. `fit` varsayılanı `stretch` | A ve E'nin dosyaları hariç, yukarıdaki listeyi başka dilim aynı anda düzenlemez |

Glitch ve LUT için kod dilimi yok.

Paralel başlayabilir: A, B, C, ve Soru 1 "halka kapansın" ise E. D, F, G soru cevaplarından önce başlamaz. D ile G aynı anda `compositor.rs` düzenleyemez. B ile G aynı anda `schema_tests.rs` düzenleyemez. C ile G aynı anda `project.rs` düzenleyemez.

## Doğrulanmamış

- CLI ikilisi çalıştırılmadı. `24 fps fps` dizesi format + `Display` okumasından çıkar.
- Dogfood MP4 ve LUT karesi yeniden ölçülmedi. Siyahın formül değeri yaklaşık `(0, 0.045, 0.095)` doğrusaldır.
- Shockwave merkez noktası piksel olarak ölçülmedi. Mekanizma WGSL ve kayıtlı `chromatic_aberration: 0.02` ile uyumludur.
- Bu makinede AV1, sistem ffmpeg'i ile açılmadı. Decoder varlığı ölçülmedi.
- `ImpactSolver::solve` gövdesi satır satır okunmadı. İmzası bir kesim listesi değildir.
- Premiere EDL birincil sayfası açılmadı.
- "Yarıçap 0 ise çizme" diyen bir shockwave spesifikasyonu açılmadı.
