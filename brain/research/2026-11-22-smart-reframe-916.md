---
title: 9:16 akıllı reframe — çıktı profili, cover crop ve odak noktası
date: 2026-11-22
status: draft
tags: [schema, render, media-io, cli, aspect-ratio, reframe]
---

# 9:16 akıllı reframe — çıktı profili, cover crop ve odak noktası

## Soru / Amaç

`brain/research/2026-11-22-asta-v3-uretken-darbogazlari.md:25` dikey versiyon için "yeni proje + kamera/fit ayarı" gerektiğini söylüyor. Bu not, uygulamaya geçmeden önce şu soruları yanıtlar:

1. `render --aspect 9:16` / `--size 1080x1920` projeyi değiştirmeden mümkün mü?
2. Cover crop ve odak noktası hangi katmanda yaşamalı: şema, render, resolver, ffmpeg?
3. Gerçek özne takibi olmadan "akıllı" ne demek olabilir; hangi yaklaşım savunulabilir?
4. Odak animasyonu Flicks + `SafeExpr` mi, render-time otomatik mi; determinizm ve seek bağımsızlığı ne olur?
5. Dikey çıktıda güvenli alan/letterbox nasıl ele alınmalı; kapsam dışı ne?
6. Ses davranışı aynı kalır mı; "akıllı" iddiasını abartmamak için kabul kriterleri ne?

Bu araştırmada kaynak kod değiştirilmedi, `cargo`/render/git çalıştırılmadı. Tüm "bugün" ifadeleri kod okumasıdır; hiçbiri çalıştırılarak doğrulanmadı. Tek yazma bu dosyadır.

## Mevcut Kod Gerçekliği (Kanıtlı)

| Konu | Kanıt | Bugünkü sonuç |
| :--- | :--- | :--- |
| Çıktı boyutu yalnızca profilden gelir | `crates/frameshift-render/src/compositor.rs:1425-1434`; `crates/frameshift-core/src/engine.rs:198`; `crates/frameshift-core/src/report.rs:37-38` | Tuval `profile.resolution`; proje çözünürlüğü render'a girmiyor |
| `timeline.resolution` yalnızca doğrulanır | `crates/frameshift-schema/src/project.rs:114-123`, `604-608` | Sıfır olmayan değer yeterli; başka okuma yok |
| `SourceVideo.fit` düğümde, varsayılan `Stretch` | `crates/frameshift-schema/src/graph.rs:325-335`; erişim `graph.rs:1414-1417` | Fit klip değil düğüm özelliği |
| Cover crop ffmpeg'de, sabit merkez | `crates/frameshift-media/src/ffmpeg.rs:943-959` (özellikle `948-951`) | `crop=w:h:(in_w-w)/2:(in_h-h)/2`; odak parametresi yok |
| Provider yalnızca asset + fit alır | `crates/frameshift-render/src/provider.rs:76-101`; `crates/frameshift-core/src/resolver.rs:767-773`, `861-872` | `clip_id`/düğüm kimliği decode katmanına inmiyor |
| Frame cache anahtarı odak içermez | `crates/frameshift-core/src/resolver.rs:364-370`; fit ayrımı testi `resolver.rs:959-1065` | Aynı asset + fit + boyut tek kareyi paylaşır |
| Streaming decoder anahtarı `(path,w,h,fit)` | `crates/frameshift-media/src/streaming.rs:28-46`; havuz sınırı `streaming.rs:12`; tahliye `streaming.rs:520-527` | En çok 8 açık akış; anahtar değişince yeni süreç |
| GPU düğümleri kırpılan pikseli geri getiremez | `crates/frameshift-render/shaders/transform.wgsl:122-130`; `corner_pin.wgsl:96-99`; yükleme `compositor.rs:4738-4746` | Transform/CornerPin UV'yi clamp eder; kaynak tuval boyutunda |
| Animasyon tick'e bağlı saf fonksiyon | `crates/frameshift-render/src/compositor.rs:224-266`, `269-308`; `crates/frameshift-time/src/expr.rs:51-60` | `SafeExpr` deterministik; geçmiş tutmaz |
| CLI'de boyut/aspect/odak bayrağı yok | `crates/frameshift-cli/src/main.rs:34-38`, `242-294`, `1009-1060`, `1210-1228` | Yalnızca `--profile draft\|final` |
| Ses çözünürlükten bağımsız | `crates/frameshift-core/src/resolver.rs:538-566`; `crates/frameshift-media/src/ffmpeg.rs:801-832` | 48 kHz stereo mix, AAC 192k |
| Determinizm/seek kapıları var | `crates/frameshift-core/src/engine.rs:343-360` | Yeni özellik bu kapıları kırmamalı |
| Şema sürümü katı minor eşleşmesi ister | `crates/frameshift-schema/src/version.rs:6-10`, `29-32`; `project.rs:595-597` | Minor artırmak `0.1.0` projelerini reddeder |
| Asta v3 zaten cover | `local_media/asta_v3/asta_v3_project.json:62-65`, `483-487` (28 kez), `1967-1987` | 28 kaynak düğümünün tamamı `cover`; eksik olan fit değil |
| Spec ile kod ayrışıyor | `brain/specs/project-schema.md:141-160` | Spec `resolution_scale` der; kod mutlak `resolution` kullanır |

Kaynak düğümü değerlendirmesi `compositor.rs:1946-1965`: fit okunur (`1960`), provider'a `(asset_id, src_tick, width, height, fit)` gider (`1961-1962`), dönen kare tuval boyutundadır (`1954`, `1963`). Yani cover kırpması tam olarak çıktı çözünürlüğünde yapılır.

## İncelenen Birincil Kaynaklar

Yerel (2026-11-22, kod okuması):

- `AGENTS.md`, `brain/research/README.md`, `brain/_templates/arastirma.md`
- `brain/research/2026-11-22-asta-v3-uretken-darbogazlari.md` (özellikle `:25`)
- `brain/specs/project-schema.md` (`status: locked`), `brain/specs/timebase.md` (`status: locked`)
- `crates/frameshift-schema/src/{graph.rs,project.rs,version.rs,session.rs}`
- `crates/frameshift-media/src/{ffmpeg.rs,streaming.rs,frame.rs}`
- `crates/frameshift-render/src/{provider.rs,compositor.rs}` ve `shaders/{transform.wgsl,corner_pin.wgsl}`
- `crates/frameshift-core/src/{resolver.rs,engine.rs,report.rs,inspection.rs}`
- `crates/frameshift-cli/src/main.rs`
- `local_media/asta_v3/asta_v3_project.json` (okundu; render edilmedi)

Web (2026-11-22):

- [Auto Reframe overview — Adobe](https://helpx.adobe.com/premiere/desktop/add-video-effects/commonly-used-effects/auto-reframe-overview.html)
- [Add Auto Reframe effect to sequences — Adobe](https://helpx.adobe.com/premiere/desktop/add-video-effects/commonly-used-effects/add-auto-reframe-effect-to-a-sequence.html)

Adobe sayfaları "Auto Reframe"in hedef çözünürlüğü ayrı aldığını, klibi/sekansı izleyip **keyframe ürettiğini** ve Motion Tracking preset'leriyle ("Slower/Default/Faster Motion") hareketi takip ettiğini söylüyor. Bu, "akıllı" kelimesinin endüstride en az hareket takibi içerdiğini gösterir; bizim analitik merkez yöntemimiz bu iddiayı karşılamaz.

## Doğrudan Gözlemler (Kanıtlı)

1. **Bugünkü zincir:** `profile.resolution` → `render_frame(width,height)` → `evaluate_single_node` → `provide_source_frame(w,h,fit)` → `ffmpeg crop`. Odak için tek doğal giriş, ffmpeg'in `crop` ifadesidir (`ffmpeg.rs:948-951`).
2. **16:9 → 9:16 sayıları:** 1920×1080 kaynak, 1080×1920 hedef. Cover ölçeği `1920/1080 = 1.7778`; ölçekli genişlik `3413` (ffmpeg'de çift sayıya yuvarlanır), merkez crop `x ≈ 1167`. Görünür kaynak genişliği `1080/1.7778 ≈ 607` px, yani kaynağın **%31.6**'sı. 607 px'i 1080'e çıkarmak **1.78× yukarı örnekleme** demektir.
3. **4K kaynak:** 3840×2160 → ölçek `1920/2160 = 0.889`; görünür kaynak genişliği `1215` px → 1080'e **0.89×** küçültme. Dikey çıktı kalitesi için 4K kaynak belirgin avantaj.
4. **Asta v3'te fit hazır:** 28 `source_video` düğümünün 28'i de `"fit": "cover"` (`asta_v3_project.json:483-487` ve devamı); `stretch`/`contain` yok. Timeline ve final profil 1920×1080 (`:62-65`, `:1978-1987`). Yani bu proje için "yeni proje + fit ayarı" değil, **çıktı profili override'ı** eksik.
5. **Odak bugün yok:** Ne `SourceVideo` alanlarında (`graph.rs:325-335`), ne ffmpeg filtresinde (`ffmpeg.rs:948-951`), ne de provider imzasında (`provider.rs:91-101`) odak parametresi var.
6. **Klip kimliği decode'a inmiyor:** `SourceVideo.clip_id` compositor'da çözülür (`compositor.rs:1952-1953`) ama provider'a yalnızca `asset_id` gider (`1962`). Klip başına odak, bu imza değişmeden uygulanamaz.
7. **Paylaşılan kare riski kanıtlı:** Aynı asset'in `stretch` ve `cover` varyantları ayrı cache girdisi ister; test bunu doğruluyor (`resolver.rs:1046-1053`). Odak da anahtara girmezse aynı hata oluşur.
8. **Kırpılmış piksel geri gelmez:** `Transform2D` kaynak UV'yi `[0,1]`'e clamp eder (`transform.wgsl:124-130`); `CornerPin` de aynısını yapar (`corner_pin.wgsl:96-99`). Cover crop'tan sonra pan yapmak kenar uzatması/şeffaflık üretir, yeni alan açmaz.
9. **Motion blur çarpanı:** `MotionBlurShutter` her alt örnek için alt grafiği yeniden değerlendirir (`compositor.rs:1858-1926`). Odak her tick'te değişirse provider çağrısı `sub_samples` ile çarpılır (Asta finalde 3).
10. **Akış havuzu sınırı:** `DEFAULT_MAX_OPEN_STREAMS = 8` (`streaming.rs:12`). Her düğüm farklı odak → farklı `StreamKey` → kesimlerde LRU tahliyesi ve yeni ffmpeg süreci. Kare başına değişen odak ise **her karede** yeni anahtar demektir.
11. **Metin ölçeği tuval yüksekliğine bağlı:** `font_px = font_size_norm * h` (`crates/frameshift-vector/src/font_locator.rs:418`). 1080×1920 tuvalde aynı `font_size_norm`, 1920 px yükseklikte 1.78× daha büyük piksel üretir; 16:9 için yazılmış başlık dikeyde taşabilir.
12. **Bellek:** `Rgba8Frame` 1080×1920×4 = **8.29 MB**; 1200 kare (20 sn @60) ≈ **9.3 GiB** (`engine.rs:228-235` tüm kareleri tutar). Bu, mevcut 1920×1080 ile aynı piksel sayısıdır; aspect değişikliği yeni bir sınır getirmez.
13. **Ses yolu ayrı:** `mix_project_audio` yalnızca `audio_clips` ve `duration_ticks` okur (`resolver.rs:540-566`); encode tarafı kare boyutunu ilk kareden alır (`ffmpeg.rs:776-780`).
14. **Şema sürümü tuzağı:** `is_compatible_with_current` minor eşitliği ister (`version.rs:29-32`). Yeni alanı `0.1.0` içinde `#[serde(default)]` ile eklemek eski JSON'ları yükler; minor'u `0.2.0` yapmak mevcut tüm projeleri reddeder.

## Çıkarımlar ve Öneriler

- **Dürüst MVP iki parçadır, üç değil:** (a) çıktı profili override'ı (boyut/aspect), (b) `SourceVideo` başına statik `focus`. Cover seçeneği zaten var (`graph.rs:207-212`, `ffmpeg.rs:948-951`); "kamera" diye ayrı bir 2D kavram yok. `Scene3D` kamerası bu işin aracı değildir.
- **Odak katmanı şemadır, render değil:** `SourceVideo.focus: Option<[f32;2]>` → `NodeKind::source_focus()` → provider isteği → resolver cache anahtarı → ffmpeg `crop x/y`. GPU'ya bırakılan her çözüm, kırpılmış pikseli geri getiremez (Gözlem 8).
- **Odak koordinatı "crop anchor"dur:** `0.0` kaynağın sol/üst taşma kenarı, `0.5` merkez, `1.0` sağ/alt. Konunun kaynak UV'si değildir; analiz aracı bu dönüşümü yapıp yazmalıdır.
- **"Akıllı" v1 = çekim başına statik odak:** Klibin örneklenmiş luma/edge ağırlık merkezinin zamansal medyanı; kare başına pan yok. Bu, "özne takibi" değil "otomatik çerçeveleme" olarak adlandırılmalı. Adobe'nin Auto Reframe'i hareket takibi + keyframe üretir; bizimki ona denk değil.
- **Kare başına pan bugünkü mimariyle pahalı:** Her farklı odak yeni `StreamKey` (`streaming.rs:28-46`), yani yeni ffmpeg süreci; motion blur alt örnekleriyle çarpılınca maliyet katlanır. Gerçek yumuşak pan için "overscan decode + GPU crop" gerekir; bu ayrı bir mimari dilimdir.
- **Zamanlama:** Render-time otomatik analiz reddedilmeli; temporal yumuşatma geçmiş ister ve seek bağımsızlığını bozar. Animasyon gerekirse **Flicks keyframe** olarak projeye yazılmalı, `output_tick`'e göre değerlendirilmeli; `SafeExpr` yalnızca bake aşamasında yardımcı olmalı. Kalıcı zaman `Flicks(i64)` kalır (`timebase.md:22-26`).
- **Fit override'ı sessiz olmamalı:** `--aspect` tek başına `Stretch` düğümleri ezip görüntüyü bozabilir. Öneri: aspect değişiyorsa ve `Stretch` kaynak varsa açık hata; kullanıcı `--fit cover` yazar. Asta v3 bu kontrolü zaten geçer.
- **Kompozisyon yeniden yerleşir:** Grafik normalize koordinat kullanır (`transform.wgsl:111-122`, `corner_pin` `[0,1]`); 9:16 tuval, 16:9 render'ın kırpılmışı değildir. Kaynaklar kırpılır, ama başlık/şekil/partikül yeniden ölçeklenir. Bu, "aynı edit'in dikey kopyası" beklentisiyle çelişir.
- **Ses aynı kalır:** Profil override'ı `mix_project_audio`'ya dokunmamalı; kabul testi `has_audio`, örnek sayısı ve AAC stream'ini doğrulamalı.
- **Geriye uyumluluk:** Alan `Option` + `serde(default)` + `skip_serializing_if`; sürüm **artırılmaz**; eski JSON'lar yüklenir; odak yoksa ffmpeg'in eski merkez ifadesi aynen kalır (hash değişmez).

## Tasarım Seçenekleri (Karar Noktaları)

**A. Çıktı profili override'ı**

1. **Yeni Engine metodu + CLI bayrağı (önerilen).** `render_video_mp4_with_profile_def` / `render_frame_at_with_profile_def`; CLI seçili profili klonlar, yalnızca `resolution` değiştirir. Proje dosyası değişmez; özet boyutu profilden gelir (`report.rs:37-38`).
2. **Proje profilini mutasyona uğrat.** `ProjectSession` ile `final.resolution` yaz; kalıcı ama her dikey denemede revision/history kirletir.
3. **16:9 render edip sonra kırp.** Kompozisyonu korur, ama 607 px'i 1080'e çıkarır (Gözlem 2) ve başlıkları keser.

**B. Odak noktasının katmanı**

1. **`SourceVideo.focus` (önerilen).** `fit` ile aynı yerde; iki düğüm aynı asset'i farklı çerçeveleyebilir. Maliyet: ~35 yapıcı siteye `focus: None` eklemek (ağırlıklı testler).
2. **`ClipDef.focus`.** Analizle doğal eşleşir; ama provider imzasına `clip_id` eklemek gerekir ve aynı klibi iki düğüm farklı çerçeveleyemez.
3. **Üst düzey `reframe` tablosu.** `Project`'e `Vec<{node_id, focus}>`; daha az literal kırılır, ama `render_frame` yalnızca grafı gördüğü için haritayı aşağı taşımak gerekir; düğüm tanımı ikiye bölünür.

**C. "Akıllı" kapsamı**

1. **(A) Statik odak.** Kullanıcı/agent `focus` yazar; analiz yok. En dürüst, en hızlı.
2. **(B) Çekim başına analitik odak (önerilen v1).** 96×54 luma/edge merkezi, medyan + EMA, klip sınırında sıfırlama; sonuç statik `focus` olarak yazılır. Harici bağımlılık yok.
3. **(C) Yüz/kişi tespiti.** Harici model + yeni crate; bu araştırmada **önerilmez**; "KULLANICI ONAYI GEREKLİ" sınıfına girer.
4. **(D) Overscan + GPU crop ile kare başına pan.** Gerçek yumuşak takip; ayrı dilim, kare başına 26 MB ara doku, yeni düğüm/akış.

**D. Fit override semantiği**

1. **Aspect değişince `Stretch` varsa hata (önerilen).** `--fit cover` açıkça istenir; sürpriz yok.
2. **Otomatik `cover` + stdout uyarısı.** Tek komut kolaylığı; PiP/Contain kompozisyonlarını bozabilir.
3. **Projeyi `reframe --write` ile mutasyona uğrat.** En kalıcı, ama tek komut değil.

**E. Güvenli alan / letterbox**

1. **Kapsam dışı (önerilen v1).** Yalnızca tam kanamalı cover; platform UI marjları modellenmez.
2. **`frame --safe-area` QC overlay'i.** Render'a girmez; yalnızca PNG kontrolünde kılavuz çizer.
3. **`contain` letterbox.** Bugün şeffaf dolgu (`ffmpeg.rs:953-957`), yuv420p çıkışta siyah bant olur; bulanık arka plan dolgusu yok.

**F. Animasyon**

1. **Yok (önerilen v1).** Statik odak; determinizm ve akış dostu.
2. **Flicks keyframe + animator.** Kararlı ama kare başına farklı crop → akış yeniden açılışı.
3. **`SafeExpr` binding.** `SourceVideo`'da bindable parametre yok (`project.rs:196-207`); eklenirse alt örnek başına decode riski.

- **KARAR GEREKİYOR:** `--aspect`/`--size` verildiğinde `Stretch` kaynaklarda hata mı verilsin, yoksa otomatik `cover` + uyarı mı?
- **KARAR GEREKİYOR:** Odak `SourceVideo.focus` (düğüm) mı, `ClipDef.focus` (klip) mı olsun?
- **KARAR GEREKİYOR:** "Akıllı" v1 statik çekim odağı mı, yoksa overscan maliyetini kabul edip kare başına pan mı?
- **KARAR GEREKİYOR:** `--aspect` boyut kuralı "uzun kenar sabit" (1920×1080 → 1080×1920) onaylanıyor mu?
- **KARAR GEREKİYOR:** Dikey çıktı **yeniden kompozisyon** mu kabul edilsin, yoksa 16:9 render'ı kırpan ikinci bir mod da isteniyor mu?
- **KARAR GEREKİYOR:** Güvenli alan/letterbox v1 kapsam dışı mı; `frame --safe-area` QC overlay'i istensin mi?
- **KARAR GEREKİYOR:** `SourceVideo`'ya alan eklemek `locked` `project-schema.md` güncellemesi gerektirir; onay ve analiz sonuçlarının projeye yazılması (revision artışı) kabul mü?

## Uygulama Planı

**Dilim 1 — Şema (davranış değişmez).**
`crates/frameshift-schema/src/graph.rs:325-335`: `focus: Option<[f32; 2]>`, `#[serde(default, skip_serializing_if = "Option::is_none")]`; `graph.rs:1414-1417` yanına `source_focus() -> Option<[f32;2]>`. `project.rs:745-763` içinde (Precomp dahil) `focus` sonlu ve `[0,1]` aralığında mı doğrula. `session.rs:452-464`, preset'ler (`project.rs:920/931/1157/1168`), `inspection.rs:569/580/617/628`, `app/snapshot.rs:1030` ve testlerdeki ~35 literal `focus: None` alır. Testler: eski JSON `None` yükler; aralık dışı reddedilir; round-trip.

**Dilim 2 — Media crop.**
`ffmpeg.rs:943-959`: `append_fitted_output(..., focus: [f32;2])`. `focus == [0.5,0.5]` için **eski ifade aynen** kalsın; aksi halde `crop={w}:{h}:(in_w-{w})*{qx}/1000:(in_h-{h})*{qy}/1000` (qx/qy 0..1000 tam sayı). `ffmpeg.rs:513-570` iç decode ve `decode_video_frame_with_fit`/`decode_video_window_with_fit` sarmalayıcıları; eski imzalar merkez odakla delege eder. `streaming.rs:28-46` `StreamKey`'e `focus_q: [u16;2]`; `decode_frame`/`open_and_read_into_pool` zinciri. Testler: odak 0/0.5/1'de parlak çubuğun piksel konumu; aynı asset farklı odak → farklı `StreamKey`; mevcut cover testi (`media_tests.rs:653-764`) değişmez.

**Dilim 3 — Resolver.**
`resolver.rs:346-445`: yeni `provide_video_frame_with_focus(..., focus)`; eski `provide_video_frame` merkezle delege eder. Cache anahtarı `"{asset}@{w}x{h}:{fit}:{qx:04}{qy:04}"` (`364-370`). `resolver.rs:756-887`: `ProjectFrameProvider` odaklı isteği iletir. Test: `focus_changes_decode_and_cache_key`, `resolver.rs:959-1065`'in kardeşi; aynı asset + aynı fit + farklı odak asla aynı buffer'ı paylaşmamalı.

**Dilim 4 — Render.**
`provider.rs:91-101`: istek `SourceFraming { fit, focus }` taşısın (iki implementasyon: `ProjectFrameProvider`, `FnFrameProvider`). `compositor.rs:1960-1962`: `kind.source_focus().unwrap_or([0.5,0.5])` geçir. Animator değişmez (statik). Test: sahte provider gelen odağı kaydeder; `Transform2D`/GPU sonrası kırpma yok.

**Dilim 5 — Engine.**
`engine.rs:111-119` yanına public `render_frame_at_with_profile_def`; `engine.rs:191-218` yanına `render_video_mp4_with_profile_def`. `report.rs` değişmez; `VideoRenderSummary` zaten `profile_def` boyutunu kullanır (`report.rs:26-48`). Test: override 1080×1920 → özet boyutu, kare sayısı, `has_audio`.

**Dilim 6 — CLI.**
`main.rs:34-38`: `Render`/`Frame`'e `OutputOverride { size: Option<Resolution>, aspect: Option<(u32,u32)>, fit: Option<SourceFit>, focus: Option<[f32;2]> }`. `main.rs:1009-1060`: `--size WxH`, `--aspect A:B`, `--fit`, `--focus X,Y`; ikisi birlikte hata; çift/tek boyut kuralı. `main.rs:242-294`: seçili profili klonla, `resolution`ı değiştir, yeni Engine metotlarını çağır; `format_render_success`/`format_frame_success` (`main.rs:664-717`) override'ı ve fit'i yazsın. `main.rs:1210-1228` usage. Testler: parse, eksik değer, geçersiz aspect, başarı satırı.

**Dilim 7 — Analitik odak (opsiyonel, karar sonrası).**
Yeni `crates/frameshift-core/src/reframe.rs` (yeni crate yok): çekim aralığını 96×54'te `decode_video_window_with_fit` ile bir geçişte oku; `L = 0.2126R + 0.7152G + 0.0722B`, `E = |∇L|`; ağırlık `w = max(0, L - mean) * (1 + E)`; merkez `u = Σwx/Σw`, boşsa 0.5; `x = clamp((u - win/2)/(1 - win), 0, 1)`; medyan(5) + EMA(0.25), klip sınırında sıfırla, pan hızı sınırı. Sonucu `ProjectSession::apply_command(UpsertGraphNode)` (`session.rs:60-61`) ile yaz; `RevisionConflict` korunur. Testler: hareketli parlak çubuk, statik sahne, düşük kontrast (0.5'e düş), kesim sınırı, ters/hız rampalı klipte örnekleme sınırı.

**Geriye uyumluluk:** `focus: None` eski JSON'da yok sayılır; `schema_version` `0.1.0` kalır (`version.rs:29-32`); odak yoksa ffmpeg merkez ifadesi bit-bit aynı kalır; M1 golden sentetik asset kullanır, crop'a girmez. Yeni alanları eski ikili okursa serde bilinmeyen alanı yok sayar (`deny_unknown_fields` yok).

## Doğrulama Planı

Uygulama sonrası (bu araştırmada çalıştırılmadı):

```powershell
cargo check -p frameshift-schema -p frameshift-media -p frameshift-core -p frameshift-render -p frameshift-cli
cargo test -p frameshift-schema -p frameshift-media -p frameshift-core -p frameshift-render -p frameshift-cli
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run --release -p frameshift-cli -- verify-all --out-dir target/verify_reframe
```

Kabul kriterleri:

- `verify-all` M1–M5 `all_gates_passed: true`; determinizm ve seek kapıları (`engine.rs:343-360`) değişmemiş.
- `render local_media/asta_v3/asta_v3_project.json --out target/reframe/asta_9x16.mp4 --aspect 9:16`: `ffprobe` **1080×1920**, 1200 kare, 20.000 sn, H.264 + AAC 48 kHz stereo; ses stream'i önceki 16:9 çıktıyla aynı (aynı ffmpeg build'inde MD5).
- `frame --at 7.5s --aspect 9:16` PNG'si 1080×1920; merkez odakta görünen içerik, 16:9 karenin yatay merkez %31.6'sıyla eşleşir (elle kontrol).
- Odak testi: kaynakta sol/sağ parlak çubuk → `focus 0.0/1.0` çıktıda çubuğu karşı kenara taşır; `0.5` mevcut cover çıktısıyla **bit-bit aynı**.
- Cache testi: aynı asset + aynı fit + farklı odak → farklı buffer; aynı odak → cache hit.
- Determinizm: aynı 5 tick iki kez, soğuk cache, aynı PNG hash'leri; ters sırada da aynı.
- Akış telemetrisi: 28 klipli Asta render'ında `spawns` makul (klip + asset sayısı mertebesinde), kare başına spawn yok.
- Bellek: 1200 kare için tepe RSS ~10 GiB sınırında; bu, mevcut 1080p davranışıyla aynı.

## Riskler ve Doğrulanmamış Noktalar

- **Yeniden kompozisyon doğrulanmadı:** 9:16'da başlık/partikül/3D yerleşimi görsel olarak test edilmedi; `font_px = norm * h` (`font_locator.rs:418`) nedeniyle taşma beklenir.
- **Ölçek kalitesi:** 1080p kaynaktan 9:16, 607 px'lik dilimi 1.78× büyütür; keskinlik kaybı kaçınılmaz. 4K kaynak önerilir; ffmpeg `scale` bayrağı (ör. lanczos) bu araştırmada denenmedi.
- **Akış tahliyesi:** Düğüm başına farklı odak, 8 akış sınırında (`streaming.rs:12`) aynı asset'in ardışık kliplerinde gereksiz yeniden açma yaratabilir; ölçülmedi.
- **Kare başına pan:** Mevcut mimaride her karede yeni `StreamKey` → süreç patlaması; overscan çözümü tasarlanmadı, yalnızca öneri.
- **Analiz determinizmi:** Luma/edge merkezi ffmpeg decode çıktısına bağlıdır; farklı ffmpeg sürümlerinde küçük farklar olabilir. Bake edilen odak projede sabitlendiği için render determinizmi korunur; analiz tekrarı birebir olmayabilir.
- **SAR/rotasyon:** Anamorfik veya döndürülmüş kaynaklarda cover/crop davranışı doğrulanmadı; ffmpeg autorotate varsayımı test edilmedi.
- **Güvenli alan:** Platform UI marjları için birincil kaynak bu oturumda doğrulanmadı; sayısal marj verilmedi. Kapsam dışı bırakılması önerildi.
- **Şema/spec çelişkisi:** `project-schema.md:160` draft/final farkını yalnızca ölçek/blur/filtre ile sınırlar; aspect değişimi normalize koordinat anlamını değiştirir. Kilitli spec güncellenmeden özellik "spec dışı" kalır.
- **Asta v3 kanıtı sınırlı:** 28 cover düğümü okundu; dikey render denenmedi. `asta_v3_project.json` içinde `vector_text` yok, bu yüzden tipografi taşması bu projede gözlenemez.
- **Bu oturumda hiçbir komut çalıştırılmadı:** Tüm sayılar (3413, 607, %31.6, 8.29 MB) aritmetik ve kod okumasıdır; ölçüm değildir.
