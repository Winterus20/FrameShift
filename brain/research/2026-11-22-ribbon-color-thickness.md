---
title: RibbonTrail3D renk ve kalınlık kontrolü — beyaz tel artefaktı
date: 2026-11-22
status: draft
tags: [render, wgsl, schema, hdr, color, ribbon, tonemap]
---

# RibbonTrail3D renk ve kalınlık kontrolü

## Soru / Amaç

`ribbon_trail_3d` düğümü 0.045–0.05 dünya birimi genişlikte bile beyaz tel gibi görünüyor ve yazılan renkler additive HDR birikiminde beyaza kaçıyor. Bu araştırma şu soruları kanıtla cevaplar:

1. Beyazlamanın kesin shader terimi hangisi; HDR core, üç şeridin üst üste binmesi, halo, yoksa AgX/PbrNeutral doygunlaşması mı?
2. `color_a_rgba`, `color_b_rgba`, `width` ve `hdr_intensity` gibi parametreler şemada var mı; kullanıcı neden sonucu değiştiremedi?
3. Minimum hangi parametreler eklenmeli (core/halo yoğunluğu, saturation, piksel tabanlı minimum genişlik)?
4. Additive yerine Normal blend seçeneği ne kazandırır/kaybettirir; depth/occlusion nasıl etkilenir?
5. `width` zaten bindable mi; piksel tabanlı alt sınır için ne gerekiyor?
6. Varsayılan görünüm değişmeli mi, yoksa opt-in mi olmalı; M1 ve showcase hash'leri etkilenir mi?

Kapsam dışı: kod değişikliği, render, `cargo`, git. Bu belge yalnızca kaynak okuması ve türetilmiş aritmetiktir.

## Mevcut Kod Gerçekliği (Kanıtlı)

- **Düğüm alanları:** `NodeKind::RibbonTrail3D` 21 alan taşır — `ribbon_count`, `segments`, `seed`, `width`, `speed`, `radius`, `trim_start`, `trim_end`, `color_a_rgba`, `color_b_rgba`, `camera_pos`, `camera_rot_deg`, `fov_deg`, `time_offset_sec`, `ribbon_style`, `strand_spacing`, `curve_bend`, `pos`, `rot_deg`, `scale`, `opacity` (`crates/frameshift-schema/src/graph.rs:874-918`). `hdr_intensity`, `core_intensity`, `halo_intensity`, `saturation`, `min_width_px` veya blend modu **yoktur**.
- **Varsayılanlar:** `ribbon_count=4`, `segments=48`, `width=0.035`, `radius=1.1`, `color_a=[1.0,0.15,0.7,1.0]`, `color_b=[0.1,0.95,1.0,1.0]` (`graph.rs:1151-1173`), `strand_spacing=0.06` (`graph.rs:1231-1233`), `curve_bend=0.45` (`graph.rs:1235-1237`), kamera `[0,0,2.5]` + `fov=50` (`graph.rs:1075-1081`).
- **Bindable listesi:** `width` zaten listede (`crates/frameshift-schema/src/project.rs:430-456`, özellikle `431`). Renk alanları ve `color_*` hiçbir düğümde bindable değil; liste yalnızca `f32` isimleri kabul eder (`project.rs:196-215`).
- **Bindable uygulaması:** liste ile shader arasında ikinci bir isim eşlemesi var; `"width" => *width = value` (`crates/frameshift-render/src/compositor.rs:851`), diğer adlar `850-877` aralığında, tanınmayan ad `_ => {}` ile sessizce yutulur (`compositor.rs:876`). Yeni parametre eklenirse **iki** listeye de yazılmalıdır.
- **Uniform:** `RibbonTrail3DUniform` 240 byte, 16-byte hizalı; `params0.x = width`, `params2 = [style, strand_spacing, curve_bend, opacity]`, `dof_params = [focal, blur, max_expand, reserved]` (`crates/frameshift-render/src/uniforms.rs:870-888`; boyut assert'i `uniforms.rs:1070`).
- **Paketleme:** `build_ribbon_trail3d_uniform` alanları doğrudan kopyalar (`compositor.rs:5592-5647`); `params0` satırı `5636`, `params2` satırları `5639-5644`, `dof_params` `5645`. Çağrılar `compositor.rs:3111` (tek başına) ve `compositor.rs:3498` (Scene3D).
- **Blend durumu:** `hdr_additive_premul_blend` = renk `One/One/Add`, alfa `One/OneMinusSrcAlpha/Add` (`crates/frameshift-render/src/pipelines.rs:550-561`). Ribbon pipeline'ları bu durumu kullanır: tek başına `589-600` (depth `None`), Scene3D `602-613`, MSAA4x `676-687`.
- **Depth:** Scene3D ribbon'ları `scene_depth_read_state` kullanır — `Depth32Float`, `depth_write_enabled=false`, `depth_compare=Less` (`pipelines.rs:490-496`). Tek başına ribbon'da depth eki yoktur (`pipelines.rs:597` → `None`).
- **Çizim:** Scene3D'de ribbon başına `instance_count = ribbon_count` (`compositor.rs:3505-3509`); tüm ribbon'lar tek pipeline ile tek geçişte çizilir (`PhaseBDraw::Ribbon` `compositor.rs:3250-3255`, `draw_phase_b` `3614-3638`).
- **Vertex genişliği:** `half_w = width * 0.5 * tip_taper * (1 + coc*1.35)` (`crates/frameshift-render/shaders/ribbon_trail3d.wgsl:222-224`); genişlik dünya birimidir, perspektifle küçülür, piksel alt sınırı yoktur. `width` alt sınırı yalnızca `0.0005` (`wgsl:124`).
- **Fragment renk üretimi:** `halo = pow(1-|u|, mix(1.6,0.85,dof))`, `core = exp(-u*u*mix(7.5,2.2,dof))` (`wgsl:253-254`); `hdr_rgb = base*(1.15 + halo*0.45) + mix(base,1,0.6)*(core*1.75)` (`wgsl:265`); dönüş `hdr_rgb*alpha` (premultiplied, `wgsl:266`). Shader başlığı bunu bilinçli tasarım olarak yazar: "HDR core > 1.5" (`wgsl:2`).
- **Tonemap:** Son kodlama `Rgba8Unorm` (`compositor.rs:1359`); mod yalnızca kök düğüm `MotionBlurShutter { tonemap: true }` ise `AcesLite`, aksi halde `None` (`compositor.rs:1299-1307`). `TonemapMode` listesi `uniforms.rs:209-217`; AgX `motion_blur_accumulate.wgsl:126-144`, PbrNeutral `146-165` — PbrNeutral tepe > 0.76 olduğunda `mix(color, vec3(new_peak), g)` ile **açıkça beyaza çeker** (`wgsl:163`).
- **Şema sürümü:** `CURRENT_SCHEMA_VERSION = 0.1.0` (`crates/frameshift-schema/src/version.rs:6-10`); uyumluluk **tam minor eşitliği** ister (`version.rs:29-31`), yani sürüm 0.2.0'a çıkarsa mevcut 0.1.0 dosyaları reddedilir.
- **UI özeti:** app snapshot ribbon için yalnızca `n=<count> trim=[a,b]` yazar (`crates/frameshift-app/src/snapshot.rs:1196-1203`); genişlik/renk/yoğunluk hiç gösterilmez.
- **Test kapsamı:** ribbon testi `width: 0.045` ile render eder (`crates/frameshift-render/tests/faz6_3_tests.rs:519`) ama yalnızca `count_non_zero_alpha > 40` ve determinizm arar (`faz6_3_tests.rs:549-554`); renk/beyazlık iddiası yoktur. `faz6_2_tests.rs:324-423` de yalnızca "boş değil" kontrolü yapar.
- **Golden yüzeyi:** `golden_scene_preset` içinde hiç ribbon düğümü yoktur (grep: `crates/frameshift-schema/src/project.rs` içinde yalnızca `430` bindable satırı); M1 doğrulaması kendi kendini yeniden render edip hash karşılaştırır (`crates/frameshift-core/src/engine.rs:343-353`), saklı bir golden hash dosyası yoktur. Showcase preset'i de ribbon içermez (`project.rs:1145-1304`).

## İncelenen Birincil Kaynaklar

Tümü yerel; bu araştırmada web kaynağı kullanılmadı, harici crate/dependency önerilmedi.

- `crates/frameshift-schema/src/graph.rs` (düğüm tanımı, varsayılanlar)
- `crates/frameshift-schema/src/project.rs` (bindable listesi, preset'ler)
- `crates/frameshift-schema/src/version.rs` (şema uyumluluğu)
- `crates/frameshift-render/src/compositor.rs` (uniform paketleme, çizim, tonemap seçimi)
- `crates/frameshift-render/src/pipelines.rs` (blend/depth durumları)
- `crates/frameshift-render/src/uniforms.rs` (uniform layout)
- `crates/frameshift-render/shaders/ribbon_trail3d.wgsl` (asıl artefakt)
- `crates/frameshift-render/shaders/motion_blur_accumulate.wgsl` (tonemap eğrileri)
- `crates/frameshift-render/shaders/color_basic.wgsl:39,108-110` (Rec.709 luma + saturation emsali)
- `crates/frameshift-render/tests/faz6_2_tests.rs`, `faz6_3_tests.rs`, `faz6_6_scene3d_tests.rs`
- `local_media/asta_devil_union_project.json` (tek gerçek ribbon kullanımı), `local_media/asta_v3/asta_v3_project.json` (0 ribbon)
- `brain/research/faz6-3-text3d-shapes-ribbons.md:322-396` (tasarım şartnamesi ve bilinen açık noktalar)
- `brain/specs/project-schema.md:41,234` (minor sürüm ve geriye uyumluluk maddesi)

## Doğrudan Gözlemler (Kanıtlı)

- **G1 — Beyaz taban cebirsel olarak garanti.** `wgsl:265` merkezde (`u=0` → `halo=1`, `core=1`) şu ifadeye indirgenir: `hdr = base*2.3 + 1.05`. `base` negatif olmadığı için **her kanal ≥ 1.05** olur. Yani yazılan renk ne olursa olsun şerit merkezi `Rgba8Unorm`'a yazılırken kırpılır ve beyazdır.
- **G2 — Kırpma eşiği ölçüldü.** `u` yarı-genişliğin oranı olmak üzere `u=0`'da min kanal 1.05–1.40 aralığında. v2 rengi `[1.15,0.06,0.24]` için min kanal `u≈0.156`'da 1.0'in altına düşer; yani beyaz çekirdek yarı-genişliğin ortadaki ~%16'sı (tam genişliğin ~%8'i) kadardır, kalan kısım renkli halo olarak kalır — "beyaz tel" görünümü tam olarak budur.
- **G3 — Gerçek proje değerleri.** `local_media/asta_devil_union_project.json` içindeki tek ribbon düğümü: `ribbon_count=3`, `width=0.022`, `radius=1.55`, `camera_pos=[0,0,2.6]`, `fov=52`, `color_a=[1.15,0.06,0.24]`, `color_b=[0.75,0.1,1.05]`, `ribbon_style` alanı yok → varsayılan `0` (Lissajous). `asta_v3_project.json` içinde ribbon düğümü sayısı **0** (kullanıcı sildi).
- **G4 — Hesaplanan HDR ve tonemap sonuçları** (G3 renkleriyle, `u=0`): `color_a` → `[3.695, 1.188, 1.602]`; ACES-lite `[0.97, 0.84, 0.89]`, PbrNeutral `[0.98, 0.50, 0.58]`. Üç katman üst üste (additive) → `[11.09, 3.56, 4.81]`; ACES-lite `[1.00, 0.97, 0.98]` — tam beyaz.
- **G5 — Tonemap bu editte devrede değildi.** Projenin kök düğümü `out_final` bir `blend` düğümüdür; `tonemap: true` yalnızca ara `fx_mblur` düğümündedir. `compositor.rs:1299-1307` yalnızca **kök** düğüme baktığı için kodlama `TonemapMode::None` ile yapıldı; yani HDR değerler eğriye girmeden [0,1]'e kırpıldı.
- **G6 — Üç şerit gerçekten üst üste biniyor.** `ribbon_style=0` yolunda her `iid` bağımsız Lissajous eğrisi üretir (`wgsl:58-86`, çağrı `133-141`); 3 instance birbirini keser ve additive toplama kesişimlerde 2–3× yapar. `ribbon_style=1`'de şeritler `strand_spacing` ile ayrılır (`wgsl:147`) ama bükülmelerde ve eğik bakışta yine yaklaşır.
- **G7 — Genişlik pikselde ölçüldü.** 1920×1080, `fov=52`, mesafe 2.6 varsayımıyla 1 dünya birimi ≈ 426 px: `width=0.022` ≈ 9.4 px, `0.045` ≈ 19.2 px, `0.05` ≈ 21.3 px (şerit `z≈0` ve `scale=1` iken). `scale` model matrisiyle çarpıldığı için (`wgsl:205`) genişliği de ölçekler.
- **G8 — Halo beyazın kaynağı değil.** Halo terimi tek başına `base*1.6` üretir: `[1.15,0.06,0.24]` → `[1.84,0.10,0.38]`. Yalnız kırmızı kırpılır, sonuç koyu kırmızı kalır. Beyazı üreten fazlalık `mix(base,1,0.6)*1.75` = her kanala **+1.05** ekleyen terimdir.
- **G9 — Occlusion zaten doğru.** Scene3D'de ribbon depth okur ve yazmaz (`pipelines.rs:490-496, 602-613`); opak geometri arkasında kalan şerit çizilmez. Tek başına kullanımda depth eki yoktur, yani her zaman üste çizer.
- **G10 — DeepGlow amplifikatörü.** v2 projesinde `deep_glow threshold=1.05, intensity=0.24, mip_levels=5`. Şerit çekirdeği 1.19–3.70 aralığında olduğu için eşiğin çok üstünde; bloom beyaz çekirdeği genişletir, "tel"i kalınlaştırır.

## Çıkarımlar ve Öneriler

- **Kesin neden sıralaması:** (1) `wgsl:265` içindeki beyaz karışımı — tek başına yeterli, çünkü tüm kanallara ≥1.05 taban basıyor; (2) additive birikim — 3 Lissajous şeridin kesişiminde 2–3×; (3) `TonemapMode::None` altında kırpma — bu projede gerçekten olan bu; (4) DeepGlow'un beyaz çekirdeği büyütmesi. AgX/PbrNeutral **bu projede devrede değildi**; devreye girseydi PbrNeutral `mix(..., vec3(new_peak), g)` ile beyazı daha da artırırdı (`motion_blur_accumulate.wgsl:163`).
- **"hdr_intensity zaten var mı?"** Hayır. `Text3D` (`graph.rs:758-759`) ve `Shape3D` (`graph.rs:833-834`; `Diamond3D` bu türün bir varyantı, `graph.rs:318`) bu alanı taşır ve shader'da yalnızca çarpan olarak kullanılır (`text3d_mesh.wgsl:362-368`, `compositor.rs:5429`); ribbon'da karşılığı yok.
- **"Kullanıcı neden kontrol edemedi?"** Dört ayrı neden: (a) beyazlık parametre değil, shader sabiti; (b) `hdr_intensity`/`core_intensity` alanı hiç yok; (c) renkler bindable değil, yani yalnızca proje dosyasını elle düzenleyerek değişir; (d) UI yalnızca `count` ve `trim` gösterir (`snapshot.rs:1196-1203`), `inspect` çıktısında parametre dökümü yok.
- **Minimum parametre seti (öneri):** `hdr_intensity` (master, varsayılan 1.0), `core_intensity` (1.75 sabitinin yerine, varsayılan ~1.10), `halo_intensity` (0.45 sabitinin yerine, ~0.30), `saturation` (Rec.709 luma mix, 1.0 = kimlik), `min_width_px` (ekran tabanlı alt sınır, ~1.5). Beyaz karışım oranı `core_whiten` olarak açılırsa eski görünüm birebir yeniden üretilebilir (0.0 = saf renk, 0.6 = eski beyaz çekirdek).
- **Saturation yeri:** Taban rengine, HDR çarpanlarından **önce** uygulanmalı: `base = mix(vec3(dot(base,LUMA_REC709)), base, saturation)`. Çekirdek zaten >1'e çıktıktan sonra doygunluk düşürmek kanalları birbirine yaklaştırıp beyazı **artırır**.
- **Blend:** Additive neon hissi verir ve sıra bağımsızdır, ama kesişimde doygunluğu yok eder. Normal (premultiplied alpha) her şeridin rengini korur ve örtmeyi doğru yapar; bedeli, kesişen şeritlerde çizim sırasının görünür olması ve 3 ek pipeline.
- **Kalınlık:** `width` dünya birimi olarak doğru bir tasarım; eksik olan **piksel alt sınırı**. Perspektifle küçülen şerit 1 px altına düşünce MSAA ile birlikte titrer ve "tel"leşir. Öneri: vertex shader'da `pos` ile `pos+side_dir*half_w` noktalarını clip space'e projekte edip NDC farkından piksel genişliğini ölçmek, `min_width_px`'in altındaysa `half_w`'ı ölçeklemek. FOV/roll'dan bağımsızdır; uniform'a yalnızca `viewport_w/h` eklemek yeter.
- **Geriye uyumluluk:** Eski JSON'lar `#[serde(default)]` ile sorunsuz yüklenir; `schema_version` **artırılmamalı**, çünkü `version.rs:29-31` tam minor eşitliği istiyor ve 0.2.0'a çıkmak mevcut tüm 0.1.0 dosyalarını reddeder. Şartname `project-schema.md:41` bunun tersini vaat ediyor; bu bir spec/kod çelişkisidir, ayrı karar gerektirir.
- **Hash etkisi:** M1 golden preset'inde ve showcase preset'inde ribbon yok; M1 determinizm kontrolü saklı hash dosyasına değil, aynı projenin ikinci render'ına dayanıyor (`engine.rs:343-353`). Dolayısıyla varsayılan değişikliği M1/showcase hash'lerini **etkilemez**; yalnızca ribbon içeren projeler görsel olarak değişir.

## Tasarım Seçenekleri (Karar Noktaları)

**Seçenek A — Varsayılanı düzelt (önerilen).** `wgsl:265` beyaz karışımı kaldırılır, `core_intensity=1.10`, `halo_intensity=0.30`. Artı: hata kökten gider, yeni projeler doğru başlar. Eksi: ribbon kullanan projeler (yalnızca v2) farklı görünür. Risk düşük, çünkü hash kapısı yok.

**Seçenek B — Opt-in bırak.** Varsayılanlar eski sabitleri birebir korur (`core_whiten=0.6`, `core_intensity=1.75`); kullanıcı yeni alanları elle yazar. Artı: sıfır görsel regresyon. Eksi: beyaz hata varsayılan kalır, her yeni projede tekrar eder.

KARAR GEREKİYOR: Varsayılan görünüm düzeltilsin mi (A), yoksa eski görünüm korunup düzeltme opt-in mi olsun (B)?

**Seçenek A — Beyaz karışımını tamamen kaldır.** `mix(base,1,0.6)` → `base`. En temiz ve en az parametre. Eski görünüm gerekirse geri getirilemez.

**Seçenek B — `core_whiten` parametresi (varsayılan 0.0).** Aynı düzeltme, ama 0.6 verilerek eski görünüm birebir üretilebilir. Bir fazla alan.

**Seçenek C — Yalnızca `hdr_intensity` ekle, sabitleri bırak.** En ucuz, ama kullanıcı yoğunluğu düşürünce tüm neon söner; renk yine beyaz kalır. Önerilmez.

KARAR GEREKİYOR: Beyaz karışımı kaldırılsın mı (A), yoksa `core_whiten` ile geriye dönük seçilebilir mi kalsın (B)?

**Seçenek A — Blend varsayılanı Additive, Normal opt-in.** Mevcut dosyalar birebir aynı kalır; kullanıcı gerektiğinde Normal seçer. 3 yeni pipeline (`ribbon_trail3d_normal`, `_scene_normal`, `_scene_normal_msaa4x`) + `draw_phase_b`'de blend'e göre iki liste.

**Seçenek B — Varsayılan Normal.** Renk sadakati en yüksek; ama neon hissi kaybolur ve tüm mevcut ribbon projeleri değişir. Aynı pipeline maliyeti.

**Seçenek C — Blend seçeneği ekleme, yalnızca shader'ı düzelt.** En düşük maliyet; additive kesişimde hâlâ parlak ama artık beyaz değil.

KARAR GEREKİYOR: Normal blend seçeneği eklensin mi; eklenecekse varsayılan Additive mi Normal mı olsun?

**Seçenek A — `min_width_px` (ekran alt sınırı).** Perspektif korunur, yalnızca çok uzak/ince şeritler kalınlaştırılır. Önerilen.

**Seçenek B — `width_px` (mutlak piksel genişliği).** 3D derinlik hissini bozar; yakın/uzak aynı kalınlıkta görünür. Önerilmez.

**Seçenek C — İkisi de.** `width` dünya birimi kalır, `min_width_px` taban; mutlak mod istenirse ayrı bayrak. Karmaşık.

KARAR GEREKİYOR: Kalınlık `min_width_px` alt sınırı olarak mı, yoksa mutlak `width_px` olarak mı modellenecek?

**Seçenek A — Sürümü 0.1.0'da bırak, yalnızca `#[serde(default)]` ekle.** Eski dosyalar yüklenir; yeni alanlar eski binary'de sessizce düşer. Önerilen.

**Seçenek B — 0.2.0'a çıkar ve `is_compatible_with_current`'i `minor <= current` yap.** Şartnameyle uyumlu olur, ama uyumluluk politikası değişir ve tüm eski dosyalar için test gerekir.

KARAR GEREKİYOR: `schema_version` 0.1.0'da mı kalsın, yoksa 0.2.0 + gevşetilmiş uyumluluk kuralı mı uygulansın?

## Uygulama Planı

1. **Şema (`crates/frameshift-schema/src/graph.rs:874-918`):** `RibbonTrail3D`'ye `hdr_intensity`, `core_intensity`, `halo_intensity`, `saturation`, `min_width_px`, `core_whiten` (`#[serde(default = "...")]`) ve `blend: RibbonBlend` (`#[serde(default)]`) ekle. Varsayılan fonksiyonları `graph.rs:1159-1173` bloğunun yanına koy. `RibbonBlend` için `#[derive(Default)]` + `Additive` varyantı.
2. **Bindable (`project.rs:430-456`):** `"hdr_intensity"`, `"core_intensity"`, `"halo_intensity"`, `"saturation"`, `"min_width_px"` ekle (renkler bindable değil, eklenmesin).
3. **Setter (`compositor.rs:850-877`):** Aynı adlar için match kolları; `saturation` ve `min_width_px` negatife düşmesin (`max(0.0)`).
4. **Uniform (`uniforms.rs:870-888`):** `params3 = [min_width_px, viewport_w, viewport_h, saturation]`, `params4 = [hdr_intensity, core_intensity, halo_intensity, core_whiten]`; boyut 240 → 272, `uniforms.rs:1070` assert'i güncelle.
5. **Paketleme (`compositor.rs:5592-5647`):** Fonksiyona `viewport: (u32,u32)` parametresi ekle; çağrılar `3111` ve `3498` zaten `width`/`height` taşıyor. Yeni alanları `params3/params4`'e yaz.
6. **Shader (`ribbon_trail3d.wgsl`):** `222-224`'te clip-space piksel ölçümü + `min_width_px` düzeltmesi; `253-266`'da `base_rgb`'ye saturation, sonra `base*(1 + halo*halo_intensity) + mix(base,1,core_whiten)*core*core_intensity`, en sonda `*hdr_intensity`. `alpha` hesabı (`259`) değişmez.
7. **Pipeline (`pipelines.rs:589-613, 676-687`):** `PREMULTIPLIED_ALPHA_BLENDING` kullanan üç Normal varyantı ekle; `PhaseBDraw::Ribbon`'a blend alanı (`compositor.rs:3250-3255`), `draw_phase_b`'ye ikinci pipeline parametresi (`3614-3638`), dört çağrı noktası (`3675, 3707, 3750, 3782`).
8. **Testler:** (a) şema: eski JSON (`{"type":"ribbon_trail_3d"}`) yüklenir ve yeni alanlar varsayılanı alır; round-trip; (b) render: `color_a=[1.0,0.0,0.0]` ile en parlak pikselde yeşil/mavi kanalın kırmızının %35'ini geçmediğini doğrula — beyaz regresyon testi; (c) `min_width_px=2` ile uzak kamerada en az 2 px kapsama; (d) mevcut determinizm (`bundle_1 == bundle_2`) korunur; (e) 8 adet struct-literal testini (`faz6_2` ×2, `faz6_3` ×2, `faz6_6` ×2, `schema_tests` ×2) yeni alanlarla güncelle.
9. **Dokümantasyon:** `brain/research/faz6-3-text3d-shapes-ribbons.md:366-389` uniform tablosunu 272 byte'a güncelle; kilitli spec'lerde ribbon tanımı olmadığı için ADR gerekmez.

## Doğrulama Planı

Bu araştırmada **hiçbir komut çalıştırılmadı**; aşağıdakiler uygulama aşaması içindir.

```powershell
cargo check --workspace
cargo test -p frameshift-schema
cargo test -p frameshift-render --test faz6_2_tests --test faz6_3_tests --test faz6_6_scene3d_tests
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Kabul kriterleri:

- `{"type":"ribbon_trail_3d"}` eski JSON'u hatasız yüklenir; yeni alanlar `hdr_intensity=1.0`, `core_intensity=1.10`, `halo_intensity=0.30`, `saturation=1.0`, `min_width_px=1.5`, `core_whiten=0.0`, `blend=Additive` döner.
- Saf kırmızı ribbon render'ında en parlak pikselde `g <= 0.35*r` ve `b <= 0.35*r`; hiçbir pikselde üç kanal birden 0.95'i geçmez.
- Aynı proje iki kez render edildiğinde `fnv1a_hash64` eşit (mevcut determinizm korunur).
- `min_width_px` testinde uzak kamerada tarama satırında en az `min_width_px` kadar ardışık alfa>0 piksel bulunur.
- `verify-all` PASS; `golden_scene_preset` ve showcase ribbon içermediği için M1 çıktıları değişmez.

## Riskler ve Doğrulanmamış Noktalar

- **Doğrulanmadı:** Hiç render alınmadı; G4'teki sayılar `wgsl:265` formülüne değer koyarak türetildi, GPU çıktısıyla karşılaştırılmadı. G7'deki piksel değerleri `scale=1` ve şeridin `z≈0` olduğu varsayımına dayanır.
- **Doğrulanmadı:** Asta v3 projesinde ribbon düğümü yok (0 adet); kullanıcının 0.045–0.05 değerleriyle kurduğu sahne deposunda bulunmuyor, bu yüzden o sahnenin kamera/ölçek bağlamı bilinmiyor.
- **Risk:** Uniform 240 → 272 byte; `min_binding_size` `size_of`'tan geldiği için (`pipelines.rs:594`) otomatik güncellenir, ancak WGSL struct'ı ile Rust struct'ı elle senkron tutulmalı; uyumsuzluk sessiz veri kayması üretir.
- **Risk:** Normal blend'de kesişen şeritlerde çizim sırası görünür olur (depth yazılmadığı için sıralama yok). Additive'in sıra bağımsızlığı kaybolur.
- **Risk:** `schema_version` 0.1.0'da kalırsa, yeni alanları yazan bir proje eski binary'de sessizce alanları kaybeder (unknown field'lar yok sayılır; `deny_unknown_fields` yok).
- **Risk:** `saturation` yanlış yerde (HDR çarpanından sonra) uygulanırsa beyazlığı artırır; uygulama sırası testle sabitlenmeli.
- **Doğrulanmadı:** DeepGlow eşiğinin (`1.05`) yeni varsayılanlarla nasıl davranacağı; `core_intensity=1.10` ile tepe ~1.3–1.6 olacağı için bloom'un hâlâ eşiği aşacağı, ama beyaz çekirdek olmayacağı öngörülüyor — ölçülmedi.
- **Harici bağımlılık:** Gerek yok. Önerilen çözüm yalnızca mevcut `wgpu`/WGSL ve `serde` yeteneklerini kullanır; yeni crate gerekmez.
