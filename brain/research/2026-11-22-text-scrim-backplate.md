---
title: Metin arkası scrim/backplate — parlak zemin üzerinde tipografi okunabilirliği
date: 2026-11-22
status: draft
tags: [render-graph, shaders, schema, text-vector, engine]
---

# Metin arkası scrim/backplate — parlak zemin üzerinde tipografi okunabilirliği

## Soru / Amaç

Asta v3 finalinde krom `ASTA` yazısı parlak alev üzerinde okunmuyor; `outline` + `glow` yetmedi ([asta-v3-uretken-darbogazlari.md:24](2026-11-22-asta-v3-uretken-darbogazlari.md#L24)). Bu not üç soruyu yanıtlar: (1) mevcut düğümlerle blur'lu yuvarlak plaka kurulabilir mi, (2) yeni bir `BackdropScrim` düğümü mü yoksa `VectorText`/`Text3D` içine scrim parametreleri mi, (3) WGSL mi CPU mu ve DAG'a nereye oturur.

Bu belge uygulama değildir. Hiçbir kaynak dosya, spec, `Cargo.toml` veya test değiştirilmedi; `cargo`, render ve git çalıştırılmadı. Aşağıdaki her iddia okunan kod satırına bağlıdır; "çalışıyor/doğrulandı" denmez.

## Mevcut Kod Gerçekliği (Kanıtlı)

- **`NodeKind` envanteri 30 varyanttır** ve içinde **spatial blur düğümü yoktur**: [graph.rs:324-985](../../crates/frameshift-schema/src/graph.rs#L324-L985). Tek "blur" benzeri düğümler `MotionBlurShutter` (temporal, [graph.rs:409-417](../../crates/frameshift-schema/src/graph.rs#L409-L417)) ve `RadialShockwave`'in `zoom_blur_strength` alanıdır ([graph.rs:583](../../crates/frameshift-schema/src/graph.rs#L583)). `transition_linear_blur` yalnızca iki klip arası geçiştir, düğüm olarak kullanılamaz ([pipelines.rs:31](../../crates/frameshift-render/src/pipelines.rs#L31)).
- **`Solid`** tüm tuvale tek renk yazar, geometri parametresi yoktur; `color_rgba` premultiplied kabul edilip doğrudan clear edilir: [graph.rs:347-352](../../crates/frameshift-schema/src/graph.rs#L347-L352), [compositor.rs:2055-2066](../../crates/frameshift-render/src/compositor.rs#L2055-L2066).
- **`MaskMatte`** 2 girişlidir (`source`, `mask`), `alpha|luma|red` matte + `invert` + `feather` uygular: [graph.rs:363-373](../../crates/frameshift-schema/src/graph.rs#L363-L373). **`feather` spatial blur değildir**: maske değerine `smoothstep(edge0, edge1, matte)` uygular ([mask_matte.wgsl:62-68](../../crates/frameshift-render/shaders/mask_matte.wgsl#L62-L68)), yani ikili maskede sadece eşik kaydırır.
- **`Blend`** 2 girişlidir (`base`=`inputs[0]`, `top`=`inputs[1]`) ve 10 mod taşır: [compositor.rs:2133-2148](../../crates/frameshift-render/src/compositor.rs#L2133-L2148), [blend.wgsl:1-5](../../crates/frameshift-render/shaders/blend.wgsl#L1-L5).
- **`DeepGlow`** Jimenez mip-piramidi bloom'dur; son composite **toplamalıdır** (`base_rgb + upsampled*tint*intensity`), alfa girişten kopyalanır: [fx_bloom_upsample.wgsl:104-109](../../crates/frameshift-render/shaders/fx_bloom_upsample.wgsl#L104-L109). Pass sayısı `2 * mip_levels` (downsample + upsample + composite): [compositor.rs:2544-2652](../../crates/frameshift-render/src/compositor.rs#L2544-L2652). Varsayılan `mip_levels = 5` ([graph.rs:1039-1041](../../crates/frameshift-schema/src/graph.rs#L1039-L1041)) → 10 fullscreen pass.
- **`VectorText`** 0 girişli üreteçtir ve gölge/glow zaten vardır: `shadow_blur_norm`, `glow_radius_norm`, iki katman outline ([graph.rs:418-481](../../crates/frameshift-schema/src/graph.rs#L418-L481)). Bu efektler yalnızca glif çevresine ışık ekler, **arkaplandan örnek almaz**: [text2d_fx.wgsl:109-137](../../crates/frameshift-render/shaders/text2d_fx.wgsl#L109-L137). `glow_radius_norm` piksele `norm * height_px` ile çevrilir ([compositor.rs:4167-4168](../../crates/frameshift-render/src/compositor.rs#L4167-L4168)).
- **`VectorPathHud`** kapalı Bézier yolu + `fill_rgba` + stroke üretir; **`corner_radius` alanı yoktur**, yuvarlatma `p0/c1/c2/p1` kontrol noktalarıyla elle yazılır: [graph.rs:482-516](../../crates/frameshift-schema/src/graph.rs#L482-L516), [graph.rs:137-144](../../crates/frameshift-schema/src/graph.rs#L137-L144). Raster CPU'da, 4x4 supersampling ile yapılır ve `fill_rgba` **straight alpha** kabul edilip içeride premultiply edilir: [hud_path.rs:136-145](../../crates/frameshift-vector/src/hud_path.rs#L136-L145).
- **`FilmOptics`** içinde iki ölçekli ayrılabilir Gauss halation vardır (½ ve ¼ çözünürlük, H+V ayrı pass'ler): [compositor.rs:2744-2820](../../crates/frameshift-render/src/compositor.rs#L2744-L2820). Ancak shader highlight çıkarır, kırmızı-turuncu emulsion tint uygular ve **`alpha = 1.0` döndürür** ([fx_film_halation.wgsl:1-5](../../crates/frameshift-render/shaders/fx_film_halation.wgsl#L1-L5), [fx_film_halation.wgsl:92](../../crates/frameshift-render/shaders/fx_film_halation.wgsl#L92)); backdrop blur olarak yeniden kullanılamaz.
- **`Text3D`** `Scene3D`'nin çocuğudur, mesh olarak çizilir; scrim/billboard quad yoktur: [graph.rs:691-780](../../crates/frameshift-schema/src/graph.rs#L691-L780), [graph.rs:920-929](../../crates/frameshift-schema/src/graph.rs#L920-L929). `Scene3D` varsayılan olarak şeffaf temizler (`background_rgba` = 0).
- **Fullscreen pass altyapısı hazır**: `record_one_tex_pass` / `record_two_tex_pass` + `draw_fullscreen_pass`: [compositor.rs:5103-5144](../../crates/frameshift-render/src/compositor.rs#L5103-L5144), [compositor.rs:5936](../../crates/frameshift-render/src/compositor.rs#L5936). Çok pass'li düğüm örneği DeepGlow ve FilmOptics aynı fonksiyon içinde pool'dan ara doku alır ([compositor.rs:2744-2753](../../crates/frameshift-render/src/compositor.rs#L2744-L2753)).
- **Arity şemada zorlanır**: yanlış giriş sayısı `InvalidNodeArity` üretir ([graph.rs:2074-2085](../../crates/frameshift-schema/src/graph.rs#L2074-L2085)). Bu yüzden `VectorText`'e giriş eklemek eski projeleri kırar.
- **Parametre bağlama beyaz listesi düğüm başına ayrıdır** ve eşleşmeyen ad sessizce yok sayılır ([project.rs:191-196](../../crates/frameshift-schema/src/project.rs#L191-L196)); yeni düğüm için arm zorunlu.
- **Şema sürümü katıdır**: yükleyici `major.minor` eşitliği arar ([version.rs:29-31](../../crates/frameshift-schema/src/version.rs#L29-L31), [project.rs:595-596](../../crates/frameshift-schema/src/project.rs#L595-L596)); migration kodu yok. `0.2.0`'a çıkmak mevcut `0.1.0` projelerini reddeder.
- **MCP'de elle senkron yok**: `describe_schema` doğrudan `NodeKind::ALL_SERDE_NAMES` okur ([server.rs:207-218](../../crates/frameshift-mcp/src/server.rs#L207-L218)) ve parity testi vardır ([server.rs:642-652](../../crates/frameshift-mcp/src/server.rs#L642-L652)). Tek kaynak [graph.rs:1301-1332](../../crates/frameshift-schema/src/graph.rs#L1301-L1332).

## İncelenen Birincil Kaynaklar

Yerel (2026-11-22'de okundu; web kullanılmadı):

- `brain/specs/render-graph.md` — §2 normalize uzamsal parametre kuralı (satır 44), §5 sınırlı döngü (satır 99), `VectorText` 0 giriş sözleşmesi (satır 58).
- `brain/specs/project-schema.md` — `VectorText` alanları (satır 112), minor sürüm kuralı (satır 41).
- `brain/specs/effect-shader-api.md` — `Effect1D` binding düzeni (satır 36), 16-byte hizalama (satır 42-48), döngü sınırı (satır 92).
- `brain/specs/inspection-contract.md` — okunabilirlik sınırı (satır 63).
- `crates/frameshift-schema/src/graph.rs`, `project.rs`, `version.rs`.
- `crates/frameshift-render/src/compositor.rs`, `pipelines.rs`, `uniforms.rs`, `shaders/*.wgsl`.
- `crates/frameshift-vector/src/hud_path.rs`, `crates/frameshift-core/src/inspection.rs`, `crates/frameshift-mcp/src/server.rs`, `crates/frameshift-app/src/snapshot.rs`.
- Testler: `crates/frameshift-schema/tests/schema_tests.rs`, `crates/frameshift-render/tests/compositor_tests.rs`.

## Doğrudan Gözlemler (Kanıtlı)

1. **Spatial blur üreten hiçbir düğüm yok.** `expected_input_count` tablosundaki 1 girişli FX'ler şunlardır: `Transform2D`, `ColorBasic`, `MotionBlurShutter`, `Lut3D`, `MotionTileShake`, `AnimeImpactFrame`, `RadialShockwave`, `HalftoneGlow`, `DeepGlow`, `AnimeSharpen`, `FilmOptics`, `CornerPin`, `GridWarp` ([graph.rs:1366-1379](../../crates/frameshift-schema/src/graph.rs#L1366-L1379)). Hiçbiri arka planı bulanıklaştıran bir Gaussian pass'ini dışa açmaz; tek Gauss `FilmOptics` içinde highlight çıkarma + tint sonrası çalışır (aşağıdaki madde).
2. **`DeepGlow` blur değil, ışık ekler.** Çıktı `vec4(base_rgb + bloom, base.a)`; varsayılan `threshold = 0.8` ([graph.rs:1027-1029](../../crates/frameshift-schema/src/graph.rs#L1027-L1029)) altında kalan koyu bir plaka bloom üretmez, çünkü `soft_knee_threshold` eşik altını sıfırlar ([fx_bloom_downsample.wgsl:54-63](../../crates/frameshift-render/shaders/fx_bloom_downsample.wgsl#L54-L63)). Kartın arkasındaki alevi "bulanıklaştırmaz", yalnızca parlak bölgeleri taşırır.
3. **`MaskMatte.feather` ikili maskeyi yumuşatmaz.** `smoothstep` maske değerine uygulanır, komşu piksellere değil ([mask_matte.wgsl:62-68](../../crates/frameshift-render/shaders/mask_matte.wgsl#L62-L68)); yuvarlak plakanın kenarı ancak kaynak maskenin kendi anti-aliasing'i kadar yumuşak olur.
4. **`VectorPathHud` ile yuvarlak dikdörtgen çizilebilir** (4 kübik Bézier, `closed: true`, `fill_rgba`), ama yarıçap parametresi yoktur ve yol tamamen normalize koordinatta elle verilir ([graph.rs:485-516](../../crates/frameshift-schema/src/graph.rs#L485-L516)).
5. **`Blend` scrim'i metnin altına koymak için yeterlidir**, ama scrim'in kendisi arka planı örnekleyemez: `Blend` yalnızca iki hazır dokuyu birleştirir ([compositor.rs:2133-2148](../../crates/frameshift-render/src/compositor.rs#L2133-L2148)).
6. **Her düğüm tam tuval çözünürlüğünde `Rgba16Float` doku alır** (`pool.acquire(width, height)`); ½/¼ çözünürlük yalnızca düğüm içinde açıkça istenirse kullanılır ([compositor.rs:2745-2754](../../crates/frameshift-render/src/compositor.rs#L2745-L2754)).
7. **Normalize blur yarıçapı için mevcut örnek var**: `VectorText.glow_radius_norm` → `* height_px` ([compositor.rs:4167-4168](../../crates/frameshift-render/src/compositor.rs#L4167-L4168)); buna karşılık `HalftoneGlow.glow_radius_px` ham pikseldir ([graph.rs:600-601](../../crates/frameshift-schema/src/graph.rs#L600-L601)).
8. **Alfa konvansiyonu düğümden düğüme değişir**: `Solid` premultiplied kabul eder ([compositor.rs:2060-2064](../../crates/frameshift-render/src/compositor.rs#L2060-L2064)), `VectorPathHud` straight kabul edip içeride premultiply eder ([hud_path.rs:143](../../crates/frameshift-vector/src/hud_path.rs#L143)).
9. **`CustomWgsl` sandbox'ı `loop`/`while` yasaklar** ([pipelines.rs:830-834](../../crates/frameshift-render/src/pipelines.rs#L830-L834)); blur ancak sabit sayıda unrolled tap ile yazılabilir. 2 girişe ve 16 float parametreye kadar izin verir ([pipelines.rs:812-855](../../crates/frameshift-render/src/pipelines.rs#L812-L855)).
10. **Showcase 4x2 = 8 paneldir** ve başlığı "15 WGSL SHADERS" yazar ([inspection.rs:533](../../crates/frameshift-core/src/inspection.rs#L533), [inspection.rs:812-831](../../crates/frameshift-core/src/inspection.rs#L812-L831)); yeni shader bu sayıyı da günceller.

## Çıkarımlar ve Öneriler

- **Yuvarlak, yumuşak kenarlı *katı* plaka mevcut düğümlerle kurulabilir** (çıkarım), 5 düğüm ve sıfır kod değişikliğiyle: `VectorPathHud` (beyaz dolgu, kapalı yuvarlak Bézier) → `DeepGlow` (threshold≈0, yumuşak falloff üretir) → maske; `Solid` (koyu `fill`) → `MaskMatte` (`source`=Solid, `mask`=glow, `mode: luma`) → `Blend` (`base`=arka plan, `top`=plaka). Sonuç bloom şeklinde bir falloff'tur, sabit sigma'lı Gauss değil.
- **"Buzlu cam" (backdrop blur) mevcut düğümlerle kurulamaz** (çıkarım): hiçbir düğüm arka plan dokusunu okuyup uzamsal olarak yaymıyor; `DeepGlow` toplamalı, `MaskMatte` eşik tabanlı, `FilmOptics` halation alfasız.
- **Bugünkü en ucuz iyileştirme** (çıkarım, kod değişikliği yok): `VectorPathHud` (`fill_rgba=[0,0,0,0.55]`, kapalı Bézier) + `Blend`. Keskin kenarlı plaka okunabilirliği büyük ölçüde çözer; estetik olarak "flat" durur.
- **Scrim üreteç değil, filtre olmalıdır** (çıkarım): arka planı girdi olarak alan tek girişli bir düğüm, hem 2D hem `Scene3D` çıktısı üzerinde çalışır ve `VectorText`, `VectorPathHud`, `Precomp` başlıklarının hepsine hizmet eder.
- **`VectorText`'e giriş eklemek geriye dönük uyumluluğu kırar** (kanıtlı): `VectorText` bugün 0 girişlidir ([graph.rs:1357](../../crates/frameshift-schema/src/graph.rs#L1357)) ve arity doğrulaması eşitlik ister ([graph.rs:2074-2085](../../crates/frameshift-schema/src/graph.rs#L2074-L2085)); mevcut tüm projeler `InvalidNodeArity` ile düşer.
- **Parametreler normalize olmalı** (spec): render-graph §2 "blur yarıçapı, feather vb. piksel cinsinden değil" der ([render-graph.md:44](../../brain/specs/render-graph.md#L44)). `HalftoneGlow`'un `_px` alanı bu kurala aykırı bir örnektir; yeni düğüm onu taklit etmemeli.
- **`blend` parametresi eklenmemeli** (çıkarım): aynı semantiği `Blend` düğümü zaten taşıyor ([blend.wgsl:1-5](../../crates/frameshift-render/shaders/blend.wgsl#L1-L5)); scrim içinde ikinci bir blend yolu semantiği çatallar.
- **`auto_contrast` metin rengini bilmez** (çıkarım): yalnızca bulanık arka planın yerel luma'sına göre plakayı güçlendirebilir. "Açık metin / parlak zemin" (ASTA durumu) için doğru yöndür; koyu metin için polarite gerekir.
- **Minimum parametre seti 7 alandır** (çıkarım): `rect_center_norm`, `rect_size_norm`, `corner_radius_norm`, `blur_radius_norm`, `fill_rgba`, `opacity`, `auto_contrast`. `blend` DAG'a ait olduğu için dışarıda; `padding` ancak maske girişiyle anlamlı olduğu için v2'ye ertelenir.
- **Blur WGSL olmalı** (çıkarım): CPU yolu her karede GPU→CPU geri okuma gerektirir; mevcut `readback_rgba8` ayrı ve pahalı bir yoldur ([compositor.rs:5195](../../crates/frameshift-render/src/compositor.rs#L5195)). Fullscreen pass altyapısı ve ½ çözünürlük doku havuzu zaten var.

## Tasarım Seçenekleri (Karar Noktaları)

### Seçenek 1 — `BackdropScrim` (1 giriş: backdrop) + prosedürel yuvarlak rect  *(öneri)*

Arka planı tek giriş olarak alır, ½ çözünürlükte ayrılabilir blur uygular, SDF yuvarlak dikdörtgen kaplama üretir, premultiplied source-over ile arka planın üstüne bindirir. Çıktı "arka plan + plaka" olduğu için metin `Blend` ile üstüne eklenir.

- **Artı:** Gerçek backdrop blur; `VectorText`, `VectorPathHud`, `Scene3D` çıktısı için tek düğüm; 3-4 pass (DeepGlow'un varsayılan 10'undan az); hiçbir mevcut arity değişmez; `enabled=false` zaten `Input(0)` passthrough verir ([graph.rs:1456-1458](../../crates/frameshift-schema/src/graph.rs#L1456-L1458)).
- **Eksi:** Plaka geometrisi metinden bağımsız yazılır (padding otomatik değil); metin rengini bilmez; 3D sahnede derinlik/oklüzyon ilişkisi yok.

### Seçenek 2 — 2 girişli `BackdropScrim` (backdrop + metin maskesi), glif sarılı plaka

İkinci giriş metnin alfa maskesidir; JFA/SDF altyapısı ([pipeline'lar: text2d_jfa_seed/step](../../crates/frameshift-render/src/pipelines.rs#L128-L130)) ile maskeyi `padding` kadar genişletip harflere yapışan bir plaka üretir.

- **Artı:** `padding` gerçekten anlamlı; metin boyutu değişince plaka takip eder; daha "AMV sticker" estetiği.
- **Eksi:** 2-3 ek pass (JFA), yeni düğümde ikinci bir kod yolu; maskeyi beslemek için metnin ikinci kez bağlanması gerekir; rect estetiği kaybolur; `corner_radius` anlamsızlaşır.

### Seçenek 3 — Scrim parametrelerini `VectorText`/`Text3D` içine gömmek

- **Artı:** Metin sınırları rasterizer'da bilinir; padding/corner otomatik; kullanıcı tek düğüm yazar.
- **Eksi:** Scrim arka planı göremez (üreteç 0 girişli), dolayısıyla blur imkânsız; `VectorText`'e giriş eklemek tüm eski projeleri kırar; aynı kod 2D ve 3D için iki kez yazılır; scrim yalnızca o metne bağlanır, `Precomp` başlıklarına uygulanamaz.

### Seçenek 0 — `CustomWgsl` ile proje-içi prototip (motor değişikliği yok)

- **Artı:** Bugün denenebilir; 2 doku girişi ve 16 float parametre yeterli ([pipelines.rs:812-855](../../crates/frameshift-render/src/pipelines.rs#L812-L855)).
- **Eksi:** `loop`/`while` yasak, tap'ler elle unroll edilmeli; shader proje JSON'unda taşınır, kütüphane/cache avantajı yok; her projede kopyalanır; tip güvenliği ve test kapsamı yok. **Prototip için uygun, ürün için değil.**

**Net öneri:** Seçenek 1. Rect geometrisi v1'de açıkça yazılsın; Seçenek 2 (`padding` + glif sarılı plaka) v2 olarak `mode` alanıyla eklenebilir.

DAG yerleşimi (scrim metnin **altında**, girdisi prosedürel dolgu değil arka plan dokusudur):

```
bg ──► BackdropScrim ──► Blend.base ──► out
text ─────────────────► Blend.top
```

KARAR GEREKİYOR: Düğüm şekli Seçenek 1 (1 giriş, rect) mi, Seçenek 2 (2 giriş, maskeli + padding) mi, yoksa ikisi tek düğümde `mode` ile mi?
KARAR GEREKİYOR: `padding` v1'de hiç olmasın mı (rect `center/size` ile yazılır), yoksa v1 kapsamına maskeli mod da alınsın mı?
KARAR GEREKİYOR: `blur_radius` birimi `*_norm` (spec §2, öneri) mi, `*_px` (`HalftoneGlow` örneği) mi; `fill_rgba` straight alpha (`VectorPathHud` gibi) mi, premultiplied (`Solid` gibi) mi?
KARAR GEREKİYOR: `auto_contrast` tek yönlü (parlak zemin → plaka güçlenir) mü kalsın, işaretli `[-1,1]` polarite mi eklensin?
KARAR GEREKİYOR: 3D kapsamı v1'de "2D düğüm + `Scene3D` çıktısı" ile mi sınırlı olsun, yoksa `Scrim3D`/`Text3D` billboard ayrı iş olarak mı planlansın?
KARAR GEREKİYOR: `schema_version` `0.1.0`'da mı kalsın (mevcut projeler yüklenir), yoksa `0.2.0` + yükleyicide `minor <= current` gevşetmesi mi yapılsın ([project.rs:595-596](../../crates/frameshift-schema/src/project.rs#L595-L596))?
KARAR GEREKİYOR: Showcase'te panel 8 A/B ile mi değiştirilsin, yoksa 9. panel eklenip ızgara 3x3'e mi çevrilsin?

## Uygulama Planı

1. **`crates/frameshift-schema/src/graph.rs`** — `NodeKind`'a varyant ekle (enum sonuna, `PrecompInput`'tan önce, ~satır 978):
   `BackdropScrim { rect_center_norm: [f32;2], rect_size_norm: [f32;2], corner_radius_norm: f32, blur_radius_norm: f32, fill_rgba: [f32;4], opacity: f32, auto_contrast: f32 }`. Tüm alanlar `#[serde(default)]` + `default_*` fonksiyonları (mevcut desen: [graph.rs:987-1297](../../crates/frameshift-schema/src/graph.rs#L987-L1297)).
2. **Aynı dosya** — `ALL_SERDE_NAMES`'e `"backdrop_scrim"` (satır 1301-1332) ve `expected_input_count`'ta 1 girişli gruba ekle (satır 1366-1379). Serde adı `rename_all = "snake_case"` ile otomatik `backdrop_scrim` olur ([graph.rs:322-324](../../crates/frameshift-schema/src/graph.rs#L322-L324)).
3. **`crates/frameshift-schema/src/project.rs`** — `bindable_param_names`'e arm ekle (satır 196-482): `rect_center_x/y`, `rect_size_x/y`, `corner_radius_norm`, `blur_radius_norm`, `opacity`, `auto_contrast`.
4. **`crates/frameshift-render/src/uniforms.rs`** — `BackdropScrimUniform` (64 byte, 4x`vec4`): `rect = [cx, cy, half_w, half_h]` (şemadaki `rect_size_norm` tam genişlik/yükseklik, burada yarıya bölünür), `shape = [corner_r, blur_r, opacity, auto_contrast]`, `fill`, `misc = [dir_x, dir_y, aspect, _]` (blur pass yönü; composite yok sayar). 16-byte kuralı ve boyut testi: [uniforms.rs:431-466](../../crates/frameshift-render/src/uniforms.rs#L431-L466), [uniforms.rs:1059](../../crates/frameshift-render/src/uniforms.rs#L1059). `lib.rs` re-export'una ekle.
5. **İki WGSL dosyası** (bloom'un down/up ayrımı gibi, [pipelines.rs:40-41](../../crates/frameshift-render/src/pipelines.rs#L40-L41)): (a) `backdrop_scrim_blur.wgsl` — `Effect1D` düzeni (`t_source`, `s_linear`, `params`), unrolled 9-tap tent, `misc.xy` yön vektörüyle H ve V aynı programdan; (b) `backdrop_scrim_composite.wgsl` — `Effect2D` düzeni (`tex_a`=orijinal kaynak, `tex_b`=bulanık), [effect-shader-api.md:36-37](../../brain/specs/effect-shader-api.md#L36-L37). SDF: `p = (uv - center) * vec2(aspect, 1)`, `d = length(max(abs(p) - (hs - r), 0)) + min(max(qx,qy),0) - r`; kenar yumuşatma `aa = 1.5/height`; `auto_contrast` için `luma = dot(blurred.rgb / max(a,1e-5), LUMA_REC709)`; sonuç `plate` premultiplied source-over ile `tex_a` üstüne.
6. **`crates/frameshift-render/src/pipelines.rs`** — iki `include_str!` sabiti (satır 18-61 deseni), iki `ShaderProgram` alanı (`backdrop_scrim_blur`, `backdrop_scrim_composite`, satır 82-131) ve `compile_all_with_msaa` içinde iki `build_program` çağrısı (1 ve 2 doku, aynı `BackdropScrimUniform`; örnek: satır 326-334).
7. **`crates/frameshift-render/src/compositor.rs`** — `apply_animated_parameter`'a arm (satır 414-982, örnek 591-606) ve `evaluate_single_node`'a arm (FilmOptics'ten sonra, ~satır 2890). Pass'ler: (a) `blur_radius_norm > 0` ise ½ çözünürlük H, (b) ½ çözünürlük V, (c) tam çözünürlük composite (`record_two_tex_pass`: kaynak + bulanık; blur kapalıyken ikisi de kaynak). Ara dokular `pool.acquire` ile, FilmOptics deseni ([compositor.rs:2745-2753](../../crates/frameshift-render/src/compositor.rs#L2745-L2753)).
8. **`crates/frameshift-app/src/snapshot.rs`** — `node_kind_label` (satır 993-1026) ve `describe_node_params` (satır 1028+) arm'ları.
9. **Testler** — aşağıdaki bölümdeki 8 test.
10. **Showcase** — `inspection.rs` panel listesi (satır 812-831) ve gerekirse "15 WGSL SHADERS" başlığı (satır 533).

**Geriye uyumluluk:** Yeni varyant eski JSON'ları etkilemez; tüm alanlar `#[serde(default)]` olduğu için `{"type":"backdrop_scrim"}` minimal JSON'u geçerli olur (desen: [schema_tests.rs:2230-2245](../../crates/frameshift-schema/tests/schema_tests.rs#L2230-L2245)). Eski ikili yeni etiketi tanımaz (beklenen ileri uyumsuzluk). `schema_version` kararı verilmeden sürüm artırılmamalı; aksi halde `0.1.0` projeleri `UnsupportedSchemaVersion` ile reddedilir ([version.rs:29-31](../../crates/frameshift-schema/src/version.rs#L29-L31)).

## Doğrulama Planı

Çalıştırılacak (bu oturumda çalıştırılmadı):

```powershell
cargo test -p frameshift-schema --test schema_tests
cargo test -p frameshift-render --test compositor_tests
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run --release -p frameshift-cli -- verify-all --out-dir target/scrim_verify
```

Eklenecek testler ve kabul kriterleri:

1. **Serde defaults:** `{"type":"backdrop_scrim"}` deserialize olur; `rect_size_norm`, `fill_rgba`, `opacity` varsayılanları beklenen değerlerde.
2. **Arity:** 0 giriş ve 2 giriş `InvalidNodeArity` verir ([graph.rs:2074-2085](../../crates/frameshift-schema/src/graph.rs#L2074-L2085)).
3. **Etiket parity:** `ALL_SERDE_NAMES` uzunluğu +1, testteki varyant listesi +1 ([schema_tests.rs:2258-2276](../../crates/frameshift-schema/tests/schema_tests.rs#L2258-L2276)).
4. **Blur enerjisi:** sert dikey kenarlı iki tonlu arka planda, rect içinde kenardan ~20 px uzakta bir piksel `blur_radius_norm=0` iken kaynak rengi, `>0` iken iki rengin arasında bir değeri verir (gradyan yumuşadı). Rect dışındaki piksel ise girişle bit düzeyinde aynı kalır (blur rect dışına sızmaz).
5. **Determinizm:** iki render aynı `fnv1a_hash` (desen: [compositor_tests.rs:1387-1390](../../crates/frameshift-render/tests/compositor_tests.rs#L1387-L1390)).
6. **Ölçek değişmezliği:** 96x54 ve 192x108'de plaka kenarı ±1 px içinde aynı göreli konumda; draft/final ayrışması yok (render-graph §2).
7. **Premultiplied alfa:** şeffaf giriş üzerinde yarı saydam dolguyla her pikselde `rgb <= a + 1e-3`; `alpha > 0` iken siyah halo yok.
8. **`auto_contrast` monotonluğu:** parlak arka planda merkez alfa, karanlık arka plandakinden büyük; şeffaf arka planda (`Scene3D` varsayılanı) plaka yalnızca `fill_rgba` olur.

Kabul: workspace test sayısı düşmez; `fx_showcase_pack.png` üretilir ve yeni panel boş/tek renk değildir; `verify-all` manifestinde `visual_fx_showcase_pack` yüzeyi durur ([inspection.rs:452-460](../../crates/frameshift-core/src/inspection.rs#L452-L460)).

## Riskler ve Doğrulanmamış Noktalar

- **`auto_contrast` gerçek kontrast oranı garanti etmez** — metin rengi ve kapsama alanı düğümde yok; yalnızca sezgisel güçlendirme. WCAG benzeri ölçüm isteniyorsa maske girişi (Seçenek 2) şart.
- **½ çözünürlük blur bantlanma yapabilir** — 1080p'de düşük frekanslı gradyanda görünür bant riski; ölçülmedi. Tam çözünürlük alternatifi 4 kat tap maliyeti demek.
- **Pass maliyeti ölçülmedi.** Tahmin: 1920x1080'de pass başına ~2.07 MPiksel ve ~16.6 MB yazma; 3-4 pass ≈ 100-130 MB/frame. DeepGlow'un varsayılan 10 pass'inden düşük olduğu kodla sabit, gerçek fps kazancı bilinmiyor.
- **Şeffaf arka plan davranışı tanımsız** — `Scene3D` şeffaf temizlediği için (`background_rgba=0`) blur kaynağı boş kalır; "yalnızca dolgu" politikası karar bekliyor.
- **3D metinde plaka geometrisi** — `Text3D` kamera döndüğünde 2D plaka metin düzlemini takip etmez; perspektif uyumu yok. Bu araştırmada denenmedi.
- **CPU raster HUD maliyeti** — mevcut 5 düğümlü geçici çözüm her karede `render_vector_path_hud_node_rgba16f` çağırır ([compositor.rs:2314-2341](../../crates/frameshift-render/src/compositor.rs#L2314-L2341)); 1080p'de kare süresi ölçülmedi.
- **Doküman kayması** — `AGENTS.md` ve showcase başlığı "15 WGSL shader" der ([AGENTS.md:5](../../AGENTS.md#L5)); `shaders/` altında 40+ dosya var. Yeni shader eklenirse bu sayı ve başlık güncellenmeli.
- **`schema_version` kararı verilmezse** yeni projeler eski ikililerde açılmaz; karar verilip minor artırılırsa mevcut `0.1.0` projeleri açılmaz. İkisi aynı anda korunamaz; migration yolu yok.
