---
title: Audio fade ve gain automation — şema, miks ve doğrulama araştırması
date: 2026-11-22
status: draft
tags: [schema, media-io, timebase, engine]
---

# Audio fade ve gain automation — şema, miks ve doğrulama araştırması

## Soru / Amaç

20 saniyelik bir müziğin giriş ve çıkışını, ffmpeg ile ön-işlenmiş `bgm_v3.wav` üretmeden yalnızca proje JSON'u üzerinden yapabilir miyiz? Bu soru üç alt soruya ayrılır:

1. `AudioClipDef` içine eklenecek en küçük ve şemaya en uygun fade/gain modeli hangisi: (A) `fade_in_ticks` / `fade_out_ticks` + eğri, (B) klip-göreli `Vec<AudioGainKeyframe>`, yoksa (C) mevcut `AnimationKeyframe` / `EventBoundAnimation` yeniden kullanımı mı?
2. Per-sample gain, 48 kHz miks hattında tık üretmeden ve `Flicks` kalıcı zamanını bozmadan nasıl uygulanır?
3. Ekleme, kilitli `project-schema` sözleşmesinde ve mevcut M1 altın testlerinde geriye uyumlu kalabilir mi?

Bu rapor yalnızca araştırmadır; kaynak kod değiştirilmedi, `cargo`, render veya git çalıştırılmadı.

## Mevcut Kod Gerçekliği (Kanıtlı)

- `AudioClipDef` yalnızca `id`, `asset_id`, `timeline_start`, `timeline_end`, `source_start` ve statik `gain_db: f32` taşır; fade veya otomasyon alanı yoktur: `crates/frameshift-schema/src/project.rs:102-111`.
- `gain_db` varsayılanı `0.0` ve serde varsayılanı vardır; eski JSON bu alanı taşımıyorsa 0 dB kabul edilir: `crates/frameshift-schema/src/project.rs:109-110`.
- Animasyon tipleri: `AnimationKeyframe.progress` normalize `f64` (`[0.0, 1.0]`), `EventBoundAnimation` ise `event_id`, `target_node_id`, `parameter`, `offset_from_event`, `duration` ve `keyframes` ister: `crates/frameshift-schema/src/project.rs:47-84`.
- `EventBoundAnimation` yalnızca `ClipDef.event_animations` üzerinden video kliplerine bağlanır; `AudioClipDef` içinde böyle bir alan yoktur: `crates/frameshift-schema/src/project.rs:96-98`.
- Miks tek noktada yapılır: her ses klibi `source_start`'tan klip süresi kadar dilimlenir ve `mix_in(&sliced, ac.timeline_start, ac.gain_db)` ile eklenir: `crates/frameshift-core/src/resolver.rs:538-560`.
- Miks sonunda yalnızca `[-1.0, 1.0]` kırpması vardır; fade veya otomasyon uygulanmaz: `crates/frameshift-core/src/resolver.rs:562-564`.
- `AudioBuffer::mix_in` kazancı bir kez `10^(gain_db/20)` olarak hesaplar ve tüm örneklerde aynı çarpanı kullanır: `crates/frameshift-media/src/audio.rs:177-225` (özellikle `197` ve `204-222`).
- `mix_tracks` de sabit `gain_db` ile `mix_in`'e delege eder: `crates/frameshift-media/src/audio.rs:227-242`.
- `gain_db` yalnızca miks sırasında `is_finite()` ile denetlenir; `Project::validate` bu değeri denetlemez: `crates/frameshift-media/src/audio.rs:184-188` ve `crates/frameshift-schema/src/project.rs:671-688`.
- Ses klibi doğrulaması yalnızca kimlik tekilliği, `asset_id` varlığı ve `timeline_start < timeline_end` kontrolüdür; varlık türünün `Audio` olması, `source_start + duration` sınırı ve fade süresi doğrulanmaz: `crates/frameshift-schema/src/project.rs:671-688`.
- `crates/**/*.rs` içinde `fade` tanımlayıcısı yoktur; yalnızca iki WGSL shader'ında görsel `wrap_fade` / `end_fade` geçer: `crates/frameshift-render/shaders/particle_field3d.wgsl:102-109` ve `crates/frameshift-render/shaders/ribbon_trail3d.wgsl:251-259`. Ses için mevcut fade yardımcısı veya şema alanı yoktur.
- `Project::validate` zaman çizgisi süresi, çözünürlük ve fps'i kontrol eder; ses klipleri bu genel kontrolden sonra ayrı döngüde işlenir: `crates/frameshift-schema/src/project.rs:594-613`.
- Parametre bağlama doğrulaması yalnızca render grafiği düğümlerini hedefler; `find_node_in_graphs` ile düğüm yoksa hata verir: `crates/frameshift-schema/src/project.rs:698-724` ve `485-487`.
- RMS zarf önbelleği anahtarı klip başına kimlik, varlık yolu, `timeline_start/end`, `source_start` ve `gain_db` bitlerini içerir; fade alanları yoktur: `crates/frameshift-core/src/engine.rs:735-756`.
- Zarf, kompozitörde `audio_rms` olarak kullanılır; miks değişirse bağlı efektler de değişir: `crates/frameshift-render/src/compositor.rs:241-253`.
- `ProjectCommand` içinde ses klibi ekleme, taşıma veya gain değiştirme komutu yoktur; yalnızca `RemoveAsset` ses klibi kullanımını korur: `crates/frameshift-schema/src/session.rs:28-72` ve `210-219`.
- Altın projede tek ses klibi `gain_db: 0.0` ile durur ve fade taşımaz: `crates/frameshift-schema/src/project.rs:898-905`.
- M1 ses-vuruş kapısı doğrudan karışmış `AudioBuffer` üzerinden tepe arar; bu nedenle miks çıktısı bit düzeyinde korunmalıdır: `crates/frameshift-core/src/engine.rs:695-718`.
- Ses klibi şemada tanımlı ama `project-schema.md` §3 yalnızca video kliplerini listeler; `audio_clips` veya `gain_db` sözleşmede yoktur: `brain/specs/project-schema.md:63-76`.
- `project-schema.md` tüm zaman değerlerini `Flicks(i64)` olarak sabitler ve `0.1.0` projelerinin varsayılanlarla geriye uyumlu yüklenmesini kabul kriteri sayar: `brain/specs/project-schema.md:58` ve `234`.
- `timebase.md` dahili miks formatını 48 kHz, `14_700` tick/örnek olarak sabitler; f64 yalnızca değerlendirme anında serbesttir, kalıcı zaman `Flicks`'tir: `brain/specs/timebase.md:83-87` ve `96-98`.
- Şema sürüm uyumluluğu major ve minor eşitliği ister; minor artışı eski dosyayı reddeder: `crates/frameshift-schema/src/version.rs:29-31`.
- Şema testi JSON'da tam olarak `"schema_version": "0.1.0"` bekler: `crates/frameshift-schema/tests/schema_tests.rs:34`.
- Asta v3'te giriş/çıkış fade'leri `bgm_v3.wav` içinde yapılmıştır; yani sorun kodla değil, ön-işlenmiş dosyayla aşılmıştır: `brain/state.md:5` ve `brain/research/2026-11-22-asta-v3-uretken-darbogazlari.md:16`.

## İncelenen Birincil Kaynaklar

- `crates/frameshift-schema/src/project.rs` — `AudioClipDef` (102-111), animasyon tipleri (47-84), doğrulama (594-745), altın proje (784-905), geriye uyumluluk test deseni (1309-1348).
- `crates/frameshift-schema/src/error.rs` — hata varyantı üslubu ve mevcut `InvalidClipBounds` / `InvalidTimeline` (30-38).
- `crates/frameshift-schema/src/session.rs` — `ProjectCommand` yüzeyi (28-72); ses klibi mutasyonu yok.
- `crates/frameshift-schema/src/version.rs` — `CURRENT_SCHEMA_VERSION` ve uyumluluk kuralı (1-32).
- `crates/frameshift-media/src/audio.rs` — `AudioBuffer`, `mix_in`, `mix_tracks`, retime ve RMS (44-242, 1040-1090).
- `crates/frameshift-media/tests/media_tests.rs` — miks ve resample testleri (220-256).
- `crates/frameshift-core/src/resolver.rs` — `mix_project_audio` (538-566) ve `slice_audio_buffer` (616-650).
- `crates/frameshift-core/src/engine.rs` — zarf önbellek anahtarı (735-756), M1 ses kapısı (695-733), önbellek testleri (818-871).
- `crates/frameshift-render/src/compositor.rs` — `audio_rms` tüketimi (241-253).
- `crates/frameshift-cli/src/main.rs` — ses analizi yolu (946-956); `crates/frameshift-mcp/src/server.rs` — `analyze_audio` (252-272).
- `crates/frameshift-time/src/rational.rs` — örnek/tick dönüşümleri (169-207); `crates/frameshift-time/src/flicks.rs` — `Flicks` API (16-83).
- `brain/specs/project-schema.md` (özellikle 41, 58, 63-76, 234), `brain/specs/timebase.md` (83-98), `brain/state.md:5`, `brain/research/2026-11-22-asta-v3-uretken-darbogazlari.md:16`.
- Tüm yerel kaynaklar 2026-11-22'de okundu. Harici web kaynağı kullanılmadı; eğri matematiği birincil kaynak gerektirmeyen kapalı formüllerden türetildi. Harici crate veya dependency önerilmedi.

## Doğrudan Gözlemler (Kanıtlı)

- Proje miksi için tek giriş noktası vardır: render, inceleme, CLI'nin proje-miks analizi ve MCP aynı `mix_project_audio` yolunu kullanır; CLI `asset_id` verilirse doğrudan `load_audio_asset`'e gider (`main.rs:946-956`): `crates/frameshift-core/src/engine.rs:200`, `284`, `crates/frameshift-core/src/inspection.rs:426`, `crates/frameshift-cli/src/main.rs:955`, `crates/frameshift-mcp/src/server.rs:267`.
- Klip, `timeline_end - timeline_start` kadar dilimlenir; kaynak kısa kalırsa sessizlikle doldurulur: `crates/frameshift-core/src/resolver.rs:555-558` ve `616-650`. Bu, fade'in klip sınırlarına göre tanımlanmasını deterministik yapar.
- `mix_in` kaynağı hedef örnekleme hızına yeniden örnekler, sonra sabit kazançla toplar: `crates/frameshift-media/src/audio.rs:189-195`. Per-sample fade uygulanacaksa doğru yer bu döngüdür; ayrı bir post-process geçişi gerekmez.
- Örnek indeksinden tick'e dönüşüm `i128` ara aritmetiğiyle ve yuvarlama kuralıyla zaten vardır: `crates/frameshift-time/src/rational.rs:195-207`. 48 kHz'de bir örnek tam `14_700` tick'tir: `brain/specs/timebase.md:83-87`.
- `gain_db` NaN/Inf ise hata miks anında oluşur, şema yüklemesinde değil; yani geçersiz proje `validate()`'ten geçip render'da patlayabilir: `crates/frameshift-schema/src/project.rs:671-688` ve `crates/frameshift-media/src/audio.rs:184-188`.
- Ses klibinin `asset_id`'si `AssetKind::Video` bir varlığa işaret edebilir; doğrulama yalnızca varlığın var olmasına bakar: `crates/frameshift-schema/src/project.rs:678-680`. `SourceImage` tarafında ters yönde bir tür kontrolü vardır: `745-762`.
- Zarf önbelleği `gain_db` bitlerini anahtara kattığı için gain değişimi önbelleği geçersiz kılar; fade eklenirse aynı mantık fade alanlarına da uygulanmalıdır: `crates/frameshift-core/src/engine.rs:735-756` ve `818-871`.
- Altın projede fade alanı yoktur; yeni alanlar `Option` + `skip_serializing_if` ile eklenirse `golden_project.json` ve miks çıktısı değişmez: `crates/frameshift-schema/src/project.rs:898-905` ve `1313-1348`.
- Mevcut geriye uyumluluk testi yeni `Option` alanları JSON'dan silip eski dosyayı taklit eder; fade testi için birebir aynı desen kullanılabilir: `crates/frameshift-schema/src/project.rs:1313-1348`.
- `AnimationKeyframe` normalize `f64` ilerleme kullanır ve hedefi bir grafik düğümüdür; `AudioClipDef` bir grafik düğümü değildir, bu yüzden C seçeneği mevcut doğrulamayla doğrudan çalışmaz: `crates/frameshift-schema/src/project.rs:60-62`, `73-81`, `661-667`.
- Şema sürümü minor eşitliği istediğinden `0.2.0`'a çıkmak `0.1.0` dosyalarını yüklenemez yapar; bu, `project-schema.md:234` kabul kriteriyle çelişir: `crates/frameshift-schema/src/version.rs:29-31`.
- Mevcut kodda Faz 2–5 alanları (`VectorText`, `Lut3D`, `preferences` vb.) `0.1.0` sürümü altında eklenmiştir; yani bu projede "opsiyonel alan ekle, sürümü artırma" fiili bir örnek oluşturmuştur: `crates/frameshift-schema/src/project.rs:554-572` ve `crates/frameshift-schema/src/version.rs:5-10`.
- `AudioClipDef` kilitli sözleşmede hiç tanımlı değildir; dolayısıyla fade eklemeden önce yapı zaten spec dışıdır ve her ekleme sözleşmeyi genişletir: `brain/specs/project-schema.md:20-76`.

## Çıkarımlar ve Öneriler

- **A seçeneği en küçük ve en uygun artımdır.** İki opsiyonel fade tanımı ve üç değerli bir eğri enum'u, bildirilen "20 saniyelik müziğin giriş-çıkışı" darboğazını doğrudan çözer; yeni düğüm, yeni olay veya yeni grafik bağı gerektirmez (`project.rs:102-111` mevcut alanlar; `project.rs:58-84` mevcut animasyon modeli).
- **B seçeneği daha geneldir ama şimdilik erken.** Orta-klipte ducking veya elle gain eğrisi için gereklidir; ancak kazanç semantiği (mutlak dB mi çarpan mı), ilk/son keyframe davranışı ve sıra doğrulaması gibi ek kararlar ister. A'dan B'ye geçiş, yeni alanlar opsiyonel eklendiği için kırıcı olmak zorunda değildir.
- **C seçeneği yapısal olarak uygun değildir.** `EventBoundAnimation` olay ve düğüm hedeflidir; ses klibi düğüm değildir (`project.rs:72-84`, `661-667`). Ayrıca `progress` normalize f64'tür (`project.rs:60-62`); animasyon klip süresine değil, `event_id` + `offset_from_event` ile olay çıkış tick'ine `Flicks` olarak bağlanır (`project.rs:73-81`) ve `AudioFadeCurve` eşdeğeri bir eğri içermez.
- Fade, `gain_db`'nin **üzerine çarpan** olarak uygulanmalıdır: mevcut statik seviye korunur, fade yalnızca 0→1 ve 1→0 zarfı sağlar (`audio.rs:197`). Böylece eski projelerde varsayılan `None` tam olarak 1.0 çarpanına karşılık gelir.
- Per-sample gain miks döngüsünde uygulanmalıdır (`audio.rs:204-222`, `resolver.rs:559`); ayrı bir "fade pass" veya ffmpeg ön-işleme kullanılmamalıdır. Bu, `bgm_v3.wav` benzeri türetilmiş dosyaları ortadan kaldırır ve proje tek gerçek kaynak kalır.
- Eğri matematiği:
  - `linear`: `g(x) = x`.
  - `equal_power`: `g(x) = sin(pi/2 * x)`; fade-in'de `x` geçen süre oranı, fade-out'ta kalan süre oranıdır; tamamlayıcı fade'lerde `g_in^2 + g_out^2 = 1`.
  - `smoothstep`: `g(x) = x^2 * (3 - 2x)`; iki uçta türev sıfırdır.
- Tık önlemenin kritik koşulu değer sürekliliğidir: fade-in'in ilk örneği ve fade-out'un son örneği tam 0 kazanç almalıdır. Eğrinin uçtaki eğimi sıfır olmasa da değer 0'dan başladığı için örnek sıçraması oluşmaz; `smoothstep` ek olarak eğim sürekliliği sağlar.
- Fade süreleri kalıcı dosyada `Flicks` olmalıdır; yalnızca değerlendirme anında normalize `f64` ilerleme hesaplanabilir. `timebase.md` §6'nın "kalıcı nokta Flicks, değerlendirme f64" ilkesi burada da geçerlidir: `brain/specs/timebase.md:96-98`.
- Geriye uyumluluk için alanlar `#[serde(default, skip_serializing_if = "Option::is_none")]` olmalı; eski JSON'da alan yoksa `None` okunur ve miks yolu hiç değişmez (`project.rs:109-110`, `1313-1348`). Ters yönde eski binary bilinmeyen alanları yok sayar; şemada `deny_unknown_fields` yoktur (arama 0 sonuç).
- Fade yokken `mix_in`'in mevcut sabit kazanç yolu korunmalı; yeni per-sample çarpma yalnızca fade varsa devreye girmelidir (`audio.rs:177-225`). Böylece M1 altın ses örnekleri bit düzeyinde aynı kalır.
- `Project::validate`'e fade kuralları eklenmeli (`project.rs:671-688`): negatif süre, fade toplamının klip süresini aşması, sonsuz/NaN `gain_db` ve (B seçilirse) sırasız keyframe reddedilmeli. `Flicks` kullanıldığı için sürelerde NaN yapısal olarak imkânsızdır; NaN riski yalnızca `f32`/`f64` kazanç alanlarındadır.
- Zarf önbellek anahtarı fade alanlarını içermelidir (`engine.rs:735-756`); aksi halde fade eklendikten sonra eski RMS zarfı kullanılır ve `audio_rms` bağlı efektler yanlış kalır (`compositor.rs:241-253`).

## Tasarım Seçenekleri (Karar Noktaları)

| Seçenek | Şema yükü | Yetenek | Risk | Değerlendirme |
| :--- | :--- | :--- | :--- | :--- |
| **(A) `fade_in` / `fade_out` + eğri** | 2 opsiyonel alan + 3 değerli enum | Giriş/çıkış fade'i; 20 sn müzik sorunu | Düşük; varsayılan `None` ile eski davranış korunur | **Önerilen v1** |
| **(B) `Vec<AudioGainKeyframe>` + fade** | Yeni keyframe tipi, sıra ve extrapolation kuralları | Ducking, elle gain eğrisi, karmaşık otomasyon | Orta/yüksek; semantik ve doğrulama kararları çok | A sonrası opsiyonel genişleme |
| **(C) `AnimationKeyframe` yeniden kullanımı** | Yeni alan yok gibi görünür | Ses için dolaylı; olay/düğüm bağımlı | Yüksek; ses klibi düğüm değil, progress f64 normalize | **Reddedilmeli** |

**Net öneri:** A ile başla. `AudioClipDef`'e iki opsiyonel `AudioFadeDef` ekle; `gain_db` taban seviye kalsın; fade çarpanı miks sırasında per-sample uygulansın. B'yi ancak gerçek bir ducking ihtiyacı doğduğunda, A'nın üzerine ek alan olarak tasarla. C'yi kullanma.

KARAR GEREKİYOR: v1 kapsamı yalnızca fade (A) mı, yoksa keyframe gain automation (B) da bu değişikliğe dahil mi?

KARAR GEREKİYOR: JSON şekli `Option<AudioFadeDef>` (iç içe `{ duration_ticks, curve }`) mı, yoksa düz `fade_in_ticks` / `fade_out_ticks` + tek `fade_curve` alanı mı olsun?

KARAR GEREKİYOR: varsayılan eğri `linear` mı, `smoothstep` mi, yoksa `equal_power` mı olsun? (Öneri: varsayılan `linear`, isteğe bağlı `equal_power`.)

KARAR GEREKİYOR: `schema_version` `0.1.0` olarak mı kalsın, `0.1.1` mi olsun, yoksa `0.2.0` + `minor <= current` uyumluluk düzeltmesi mi yapılsın? (Öneri: bu artımda `0.1.0` kalsın; sürüm uyumluluğu ayrı ADR'de düzeltilsin.)

KARAR GEREKİYOR: kilitli `project-schema.md` güncellenecek mi ve `brain/decisions/0004-...` ADR'si yazılacak mı? (Öneri: evet; `AudioClipDef` zaten spec dışı, fade ile birlikte tanımlanmalı.)

KARAR GEREKİYOR: ses klibi fade'ini yazmak için `ProjectCommand` / MCP mutasyonu eklenecek mi, yoksa v1 yalnızca JSON düzenleme + `ReplaceProject` ile mi sınırlı kalsın?

KARAR GEREKİYOR: (B seçilirse) keyframe kazancı mutlak dB mi, yoksa `gain_db` üzerine çarpan mı olsun; ilk keyframe öncesi ve son keyframe sonrası hangi değer geçerli olsun?

## Uygulama Planı

0. **Karar kapısı:** Yukarıdaki KARAR GEREKİYOR maddeleri netleşmeden kod yazılmasın. A seçilirse ADR-0004 ve `project-schema.md` §3.1 taslağı aynı değişiklikte hazırlansın.
1. **Şema tipleri** — `crates/frameshift-schema/src/project.rs`:
   - `AnimationEasing` yakınına (47-56) `AudioFadeCurve` ekle: `Linear`, `EqualPower`, `Smoothstep`; `#[serde(rename_all = "snake_case")]`, `Smoothstep` için açık `#[serde(rename = "smoothstep")]`.
   - `AudioClipDef` öncesine (101) `AudioFadeDef { pub duration_ticks: Flicks, #[serde(default)] pub curve: AudioFadeCurve }` ekle; `AudioFadeCurve` `Default` türetsin ve varsayılan `Linear` olsun.
   - `AudioClipDef` içine (109-111) `#[serde(default, skip_serializing_if = "Option::is_none")] pub fade_in: Option<AudioFadeDef>` ve aynı şekilde `fade_out` ekle. `gain_db` aynen kalsın.
   - Eğri değerlendirmesini saf fonksiyon olarak ekle: `pub fn gain_at(self, progress: f64) -> f64`; `progress` dışında girdi almasın, `[0,1]` dışını klemplesin.
2. **Hata ve doğrulama** — `crates/frameshift-schema/src/error.rs:30-38` yakınına `InvalidAudioFade { clip_id: String, message: String }` ekle; `project.rs:671-688` ses klibi döngüsünde:
   - `ac.gain_db.is_finite()` değilse hata.
   - Her fade için `duration_ticks >= 0`; `0` no-op kabul edilebilir, negatif reddedilir.
   - `fade_in.duration_ticks.checked_add(fade_out.duration_ticks)` taşma ve `> clip_duration` kontrolü; eşitlik serbest, çakışan bölgede çarpan `min(fade_in, fade_out)` ile birleştirilir.
   - B seçilirse: `offset_ticks` kesin artan, `[0, clip_duration]` aralığında ve kazançlar finite.
   - Not: `Flicks` i64 olduğu için sürelerde NaN yoktur; NaN kontrolü yalnızca kazanç alanlarında anlamlıdır.
3. **Miks API'si** — `crates/frameshift-media/src/audio.rs:177-225`:
   - `mix_in` imzasını koru; gövdesini sabit kazançla yeni metoda delege et.
   - `pub fn mix_in_with_gain<F: FnMut(usize) -> f32>(&mut self, source: &AudioBuffer, offset_ticks: Flicks, mut gain_at: F) -> Result<(), MediaError>` ekle; `gain_at` yeniden örneklenmiş kaynak kare indeksini alsın.
   - Fade yoksa mevcut `gain` sabitini döndüren yol kullanılsın; böylece eski çıktı bit düzeyinde korunur.
   - Fade sınırlarını örnek indeksine çevirirken `ticks_to_audio_samples_nearest` kullan (`rational.rs:195-207`); fade-in için `sf / fade_in_frames`, fade-out için `(last_frame - sf) / fade_out_frames` normalize et; `last_frame = sliced.frame_count() - 1` (dilim zaten 48 kHz), `fade_*_frames == 0` ise çarpan `1.0` (no-op). Fade-in varsa ilk örnek tam 0, fade-out varsa son örnek tam 0 olur.
4. **Resolver entegrasyonu** — `crates/frameshift-core/src/resolver.rs:538-566`:
   - `ac.fade_in` / `ac.fade_out` varsa `gain_at` kapanışı kur; taban `gain = 10^(gain_db/20)` ile çarp; iki fade çakışırsa `min` uygula.
   - `slice_audio_buffer` (616-650) değişmesin; fade klip süresine göre uygulanır, `source_start`'a göre değil.
   - Fade yoksa eski `mix_in` çağrısı aynen kalsın.
5. **Önbellek anahtarı** — `crates/frameshift-core/src/engine.rs:735-756`:
   - Her klip için `fade_in`/`fade_out` varlığını, `duration_ticks` ve eğri ayırt edicisini anahtara ekle; `gain_db.to_bits()` desenini izle.
   - Fade yokken anahtarın eski biçimde kalması zorunlu değil; doğruluk için koşulsuz ekleme yeterli, ancak testte anahtarın fade ile değiştiği doğrulanmalı (`engine.rs:818-871` deseni).
6. **Testler** — aşağıdaki dosyalarda:
   - `crates/frameshift-schema/src/project.rs` test modülü (1309+): fade negatif süre, toplam süre aşımı, NaN `gain_db`, eski JSON'dan `None` okuma, round-trip.
   - `crates/frameshift-media/src/audio.rs` test modülü (1040+): 1 s DC tamponunda fade-in ilk örnek 0, orta nokta beklenen, fade sonrası 1; fade-out son örnek 0; `equal_power` için `gain_at(x)^2 + gain_at(1-x)^2 ≈ 1`; `smoothstep` uç türevleri; 1 örneklik fade; stereo ve mono.
   - `crates/frameshift-core/src/resolver.rs` veya mevcut testler: fade'li ve fade'siz miks farkı; fade'siz miks altın projeyle bit düzeyinde eşit.
   - `crates/frameshift-core/src/engine.rs`: fade eklenince zarf anahtarının değişmesi ve zarfın yeniden hesaplanması.
   - `crates/frameshift-schema/tests/schema_tests.rs`: `0.1.0` eski JSON'un fade alanları olmadan yüklendiği ve round-trip; miks örneklerinin değişmediği kontrolü `frameshift-core` testinde yapılır (schema crate `frameshift-core`'a bağımlı değildir).
7. **Geriye uyumluluk:** `golden_scene_preset` (project.rs:784-905) değiştirilmesin; `fade_in/fade_out = None` kalsın. `skip_serializing_if` sayesinde `golden_project.json` yeni anahtar üretmez. Eski JSON testi `project.rs:1313-1348` desenindeki gibi alanları silerek yapılsın.
8. **İsteğe bağlı yüzey:** `ProjectCommand` (session.rs:28-72) ve MCP `mutate_project` için `SetAudioClipFade` / `UpsertAudioClip` komutu ayrı bir iş olarak değerlendirilsin; v1'de JSON + `ReplaceProject` yeterli olabilir. Yeni harici crate gerekmez; KULLANICI ONAYI GEREKLİ bir dependency yoktur.

## Doğrulama Planı

Aşağıdaki komutlar uygulama aşamasında çalıştırılacaktır; bu araştırmada çalıştırılmadı.

```powershell
cargo test -p frameshift-schema
cargo test -p frameshift-media
cargo test -p frameshift-core
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run --release -p frameshift-cli -- verify-all --out-dir target/verify_audio_fade
```

Kabul kriterleri:

- Yeni şema testleri: negatif fade süresi, `fade_in + fade_out > clip_duration`, sonsuz/NaN `gain_db` ve (B'de) sırasız keyframe açık hata döndürür.
- Geriye uyumluluk: fade alanları silinmiş `0.1.0` JSON yüklenir; `mix_project_audio` çıktısı `golden_scene_preset` çıktısıyla **birebir aynı** `Vec<f32>` olur (yaklaşık değil, eşitlik). Değişiklik öncesi alınan örnek FNV-1a hash'i sonrasında da aynı kalmalı.
- Fade testleri: DC tamponda ilk örnek `0.0`, son örnek `0.0`; `linear` orta nokta `0.5`; `equal_power` için `gain_at(x)^2 + gain_at(1-x)^2 = 1.0 ± 1e-6`; çıktıda NaN/Inf yok.
- `cargo test --workspace`: mevcut 199/199 tabanı korunur, yeni testler eklenir, 0 başarısız.
- `cargo clippy --workspace --all-targets -- -D warnings`: 0 uyarı.
- `verify-all`: `all_gates_passed: true`; M1 determinism, seek, motion blur, audio-impact ve MP4 probe kapıları geçer; `golden_project.json` yeni fade anahtarı içermez.
- Fade'li 2 saniyelik bir test render'ı ayrıca `ffprobe` ile süre/akış doğrular; ancak AAC encoder'ın örnek düzeyinde fade'i koruduğu iddiası bu araştırmada doğrulanmamıştır.

## Riskler ve Doğrulanmamış Noktalar

- **Kod çalıştırılmadı:** Tüm çıkarımlar statik okumaya dayanır; derleme, test veya render kanıtı yoktur.
- **Örnek yuvarlaması:** Fade süresi tick'ten örneğe `nearest` ile çevrilir; yarım örnekten kısa fade 0 kareye düşebilir ve pratikte no-op olur. Bu durumda tık önleme garanti edilmez.
- **Tık eşiği ölçülmedi:** Değer sürekliliği matematiksel olarak sağlanır, ancak algısal tık eşiği ve spektral sızıntı bu araştırmada test edilmemiştir.
- **Equal-power varsayımı:** `g_in^2 + g_out^2 = 1` ilkesi tamamlayıcı fade'ler içindir; tek klip giriş/çıkışında iki kaynak arasındaki faz ilişkisi yoktur. Crossfade semantiği ayrıca tasarlanmalıdır.
- **Kırpma:** Fade sonunda tepe azalır ama çoklu kliplerin toplamı yine `[-1,1]` dışına çıkabilir; mevcut son kırpma (`resolver.rs:562-564`) tek savunmadır.
- **Şema sürümü tuzağı:** `is_compatible_with_current` minor eşitliği istediği için `0.2.0`'a çıkmak eski projeleri yüklenemez yapar; bu, `project-schema.md:234` ile çelişir. Ayrı ADR olmadan sürüm artırılmamalıdır.
- **Kilitli spec:** `project-schema.md` ve `timebase.md` `locked`; bu ekleme spec revizyonu ve ADR gerektirir. AGENTS.md'ye göre yeni `accepted` ADR yazmak "ask first" kapsamındadır: `AGENTS.md:56`.
- **Tanımsız ses şeması:** `AudioClipDef` zaten spec'te yok; fade eklemek bu boşluğu büyütür. Spec güncellemesi yalnızca fade'i değil, mevcut `audio_clips` ve `gain_db` alanlarını da tanımlamalıdır.
- **Mutasyon yüzeyi yok:** `ProjectCommand` içinde ses klibi komutu olmadığı için ajanlar fade'i ancak JSON düzenleyerek veya `ReplaceProject` ile yazabilir; bu, doğrulama ve undo açısından ayrı bir tasarım kararıdır.
- **Kaynak sınırı doğrulanmıyor:** `source_start + clip_duration` varlık süresini aşarsa `slice_audio_buffer` sessizlikle doldurur (`resolver.rs:643-645`); fade-out sessizlik üzerinde çalışabilir. Bu davranış mevcut ve bu değişiklikle düzelmiyor.
- **Varlık türü doğrulanmıyor:** Ses klibi video varlığına bağlanabilir (`project.rs:678-680`); fade eklemek bu hatayı gizlemez, ayrı bir doğrulama gerekir.
- **AAC/MP4 doğrulanmadı:** Birim testleri `AudioBuffer` düzeyinde doğru olsa bile ffmpeg AAC kodlayıcısının fade başlangıcını örnek düzeyinde koruduğu gösterilmemiştir; MP4 çıktısında `astats`/dalga formu ile ayrıca bakılmalıdır.
- **B seçeneği belirsizlikleri:** Keyframe kazancının mutlak dB mi çarpan mı olduğu, ilk keyframe öncesi ve son keyframe sonrası davranış netleşmeden B uygulanmamalıdır.
