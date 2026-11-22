---
title: Şekil/malzeme swatch galerisi — diamond_3d + material_mode 0 beyaz yıldız
date: 2026-11-22
status: draft
tags: [inspection, materials, shape3d, render, cli, color, qc, schema]
---

# Şekil/malzeme swatch galerisi

## Soru / Amaç

Asta v3'te `diamond_3d` + `material_mode 0` "dev beyaz yıldız" olarak renderlandı; beş şeklin dördü edit'ten çıkarıldı. Bu araştırma şu soruları kaynak koduyla cevaplar:

1. `diamond_3d` neden **yıldız** silüeti üretiyor ve `material_mode 0` neden **beyaz**?
2. "Dev" ölçek nereden geliyor: `size`, `scale`, kamera mı, yoksa `Scene3D` mi?
3. Mevcut inspection sheet altyapısı (3×2 contact, 4×2 FX) keyfi **N×M** grid'e genellenebilir mi; hangi fonksiyonlar?
4. 5 şekil × 4 `material_mode` = 20 hücre hangi sabit kamera/ölçek/ışıkla üretilmeli?
5. `materials_sheet.png` + `materials_manifest.json` nereye yazılmalı; `inspect` pack'i bozulmadan hangi CLI yüzeyi?
6. Hangi testler "beyaz", "dev" ve determinizm regresyonlarını yakalar?

Kapsam dışı: kod değişikliği, `cargo`, render, git. Bu belge yalnızca kaynak okuması ve türetilmiş aritmetiktir.

## Mevcut Kod Gerçekliği (Kanıtlı)

**Şema ve varsayılanlar**
- `NodeKind::Shape3D` 28 alan taşır: `shape_kind`, `size`, `aspect`, `stroke_width`, `corner_radius`, `inner_ratio`, `extrude_depth`, `bevel_size`, `material_mode`, dört renk, `pos`/`rot_deg`/`scale`, `camera_pos`/`camera_rot_deg`/`fov_deg`, pop/echo/`hdr_intensity`/`opacity` (`crates/frameshift-schema/src/graph.rs:781-839`). `color_mode` ve `palette_rgba` **yoktur**; yalnız `Text3D` taşır (`graph.rs:735-737`).
- Varsayılanlar: `size=1.0` (`graph.rs:785-786`), `aspect=1.0`, `stroke_width=0.06` (`graph.rs:1207-1209`), `inner_ratio=0.382` (`graph.rs:1211-1213`), `extrude_depth=0.12`, `bevel_size=0.01`, `material_mode=0` (`graph.rs:799-800`), `color_top=[1.0,0.95,1.0,1]`, `color_bottom=[1.0,0.2,0.75,1]`, `side=[0.25,0.05,0.45,1]` (`graph.rs:1099-1109`), `outline_color=[1,1,1,1]` (`graph.rs:807-808, 1203-1205`), `scale=[1,1,1]` (`graph.rs:1035-1037, 813-814`), `camera_pos=[0,0,2.5]` (`graph.rs:1079-1081`), `fov_deg=50` (`graph.rs:1075-1077`).
- `Shape3DKind` snake_case serileşir: `neon_tile`, `hollow_star_5`, `heart_3d`, `lightning_bolt`, `diamond_3d` (`graph.rs:307-319`).
- `Project::validate` Shape3D parametrelerini **doğrulamaz**; yalnız bindable ad listesini tanımlar (`crates/frameshift-schema/src/project.rs:383-400`). Geçersiz `size` render sırasında `VectorError` olur (`crates/frameshift-vector/src/shape3d.rs:101-107`).

**Mesh üretimi**
- `build_shape3d_mesh` bevel'i `min(0.45*extrude_depth, ...)` ile sınırlar, sonra tüm vertex'leri merkezler ve **nihai** `bounds_min/bounds_max` döndürür (`shape3d.rs:41-99`); bounds stroke'u da kapsar.
- Ölçek anlamı şekle göre değişir: `NeonTile` tam genişlik/yükseklik `size` (`shape3d.rs:155-156`), `HollowStar5` ve `Diamond3D` yarıçap `0.5*size` (`shape3d.rs:251, 430`), `Heart3D` ±0.48·s (`shape3d.rs:329-345`), `LightningBolt` ±0.48·s (`shape3d.rs:388-398`). Hepsi ≈ `size` kadardır (0.90–1.0×).
- **Diamond3D iki modlu:** `inner_ratio >= 0.99` ise 4 noktalı eşkenar dörtgen; aksi halde 8 noktalı yıldız yolu (`shape3d.rs:434-454`). Varsayılan 0.382 olduğu için varsayılan `diamond_3d` **yıldızdır**.
- Stroke `line_width = stroke_width*2.0` ile path'e eklenir (`shape3d.rs:457-463`) ve `append_stroke_outline` bu yüzleri `face_kind=3` olarak yazar (`crates/frameshift-vector/src/mesh3d.rs:581-594`).

**Materyal shader'ı**
- `cam_material.w = material_mode`: `0`=Neon3D, `1`=MetallicChrome, `2`=Flat2DStroked, `3`=IridescentAnisotropic (`crates/frameshift-render/src/uniforms.rs:831`). Değer `Shape3D`/`Text3D` uniform'una doğrudan yazılır (`crates/frameshift-render/src/compositor.rs:5523-5528`).
- `face_kind == 3` (stroke) **gölgesiz** `base_rgb * hdr_intensity` döndürür (`crates/frameshift-render/shaders/text3d_mesh.wgsl:365-370`); `material_mode 2` de düz `base*hdr` döndürür (`wgsl:372-376`).
- Malzeme başına varsayılan roughness/metallic: Neon3D 0.26/0.18, MetallicChrome 0.12/0.98, Iridescent 0.18/0.92 (`wgsl:396-405`); uniform `pbr_params=[-1,-1,0.65,0]` "malzeme varsayılanını kullan" demektir (`compositor.rs:5486, 5544`).
- Neon3D gövdesi emissive + fresnel rim: `mix(base_rgb, vec3(1,0.96,1), 0.65) * pow(1-no_v, 2.6) * ...` (`wgsl:481-489`).
- `Shape3D` uniform'unda `arc_palette=[0,0,0,0]` olduğu için `color_mode=0` (gradient) sabittir (`compositor.rs:5535`); kapak rengi her zaman `color_bottom→color_top` gradyanıdır (`wgsl:330-347`).

**Kamera ve sahne**
- Tek başına `Shape3D` kendi kamerasını kullanır: `build_camera_matrices(*camera_pos, *camera_rot_deg, *fov_deg, w, h)` (`compositor.rs:3010-3050`).
- `Scene3D` içinde çocuk `Shape3D` **sahne kamerasını** kullanır (`compositor.rs:3262, 3399`); düğümün kendi `camera_pos`/`fov_deg` alanları yok sayılır. Şema bunu engellemez, çünkü `is_scene3d_child` yalnız tür kontrolü yapar (`graph.rs:2133-2143`).
- Kamera: `perspective_rh(fov_y, aspect, near, far)`, `fov_deg` 5–175'e clamp (`compositor.rs:5296-5301`); model matrisi `scale` ile çarpılır (`compositor.rs:5516, 5703-5715`). Yani `size` (geometri) ve `scale` (matris) **çarpan** iki ölçek kaynağıdır.
- Mesh önbellek anahtarı yalnız `shape_kind,size,aspect,stroke,corner,inner,extrude,bevel` içerir (`compositor.rs:4552-4598`); `material_mode` anahtarda yoktur (uniform'a girer, mesh'i değiştirmez).

**Inspection altyapısı**
- Contact sheet 3×2'dir: `cell 426×240`, `pad 16`, `header 52`, tuval **1342×580** (`crates/frameshift-core/src/inspection.rs:91-98`); başlık `render_label_badge_scaled(..., 2)` (`inspection.rs:107-120`), hücre etiketi `render_label_badge` (`inspection.rs:147-158`), border `blit_frame_with_border` (`inspection.rs:138`).
- FX showcase 4×2'dir: 1920×1080, `card 460×482`, `cell 456×394` (`inspection.rs:514-523`); kart çizimi 836-902, `PanelSpec{col,row,title,shader_line,params_line,frame}` 706-713.
- Yeniden kullanılabilir ilkeller: `fill_rect_u8` (`inspection.rs:1175-1183`), `blit_frame_with_border` (`inspection.rs:1185-1213`), `Rgba8Frame::{filled,set_pixel,save_png}` (`crates/frameshift-media/src/frame.rs:54-74, 99-105, 175`), `downscale_frame_nearest_or_box` (`crates/frameshift-media/src/proxy.rs:105`), `linear_to_srgb` (`frame.rs:601`, re-export `lib.rs:23`).
- Hücre render'ı: `render_subgraph_node` projeyi klonlar, `output_node`'u hedefe çevirir (`inspection.rs:908-920`); hücre profili `motion_blur_sub_samples=1`, `shutter_angle_deg=0`, `TonemapMode::AcesLite`, `Straight` (`inspection.rs:539-553`).
- Manifest sabit alanlıdır ve serde default taşımaz (`inspection.rs:58-76`); kurulum 469-491.

**CLI / API**
- `CliCommand` enum'unda `Inspect{project_path,out_dir}` var (`crates/frameshift-cli/src/main.rs:30-70`), dispatch 307-319; parser bilinmeyen bayrağı reddeder (`main.rs:1075-1093`, hata 1086); usage 1210-1228 (inspect satırı 1219); parse testi 1284-1290.
- `Engine::export_inspection_pack` / `export_fx_showcase_pack` (`crates/frameshift-core/src/engine.rs:96-108`); MCP `inspect_project` aynı pack'i çağırır (`crates/frameshift-mcp/src/server.rs:275-290`).
- Pack testi yalnız varlık + boyut + epistemik alanları doğrular (`crates/frameshift-core/tests/core_tests.rs:225-273`).

**Gerçek proje ve spec**
- Asta v3 3D overlay'i tek `scene_3d`'dir: kamera `pos=[0,0,2.9]`, `fov=46`, `mode=look_at` (`local_media/asta_v3/build_project.py:221-231`); `deep_glow` eşiği 0.82 / yoğunluk 0.34 ve `halation 0.42` (`build_project.py:246-252`).
- `asta_v3_project.json` içinde **hiç `shape_3d` düğümü yoktur**; `material_mode` yalnız `Text3D` düğümlerinde geçer (`asta_v3_project.json:1293`). Kaldırılan şekillerin parametreleri depoda **yok**.
- `inspection-contract.md` görsel yüzey listesi 45. satırda sayılır; okunabilirlik sınırı "sayfa başına en fazla 3×3 veya 4×3 hücre, hücre başına en az 320px" (`brain/specs/inspection-contract.md:63`). Şekil/malzeme galerisi bu sözleşmede **tanımlı değildir**.
- Zaman tabanı: kalıcı modelde `Flicks(i64)`, `FLICKS_PER_SECOND=705_600_000`, `f64` saniye yasak (`brain/specs/timebase.md:22-25`).

## İncelenen Birincil Kaynaklar

Tümü yerel; web kaynağı kullanılmadı, harici crate/dependency önerilmedi.

- `crates/frameshift-schema/src/graph.rs`, `project.rs`
- `crates/frameshift-vector/src/shape3d.rs`, `mesh3d.rs`
- `crates/frameshift-render/src/compositor.rs`, `uniforms.rs`
- `crates/frameshift-render/shaders/text3d_mesh.wgsl`
- `crates/frameshift-core/src/inspection.rs`, `engine.rs`, `tests/core_tests.rs`
- `crates/frameshift-cli/src/main.rs`
- `crates/frameshift-mcp/src/server.rs`
- `crates/frameshift-media/src/frame.rs`, `proxy.rs`
- `crates/frameshift-vector/src/text.rs:775-860`, `font.rs:15-17`
- `local_media/asta_v3/build_project.py`, `asta_v3_project.json`
- `brain/specs/inspection-contract.md`, `brain/specs/timebase.md`, `brain/specs/project-schema.md`
- `brain/research/2026-11-22-ribbon-color-thickness.md` (ev stili ve HDR/tonemap emsali)

## Doğrudan Gözlemler (Kanıtlı)

- **G1 — "Yıldız" bir varsayılan hatası, malzeme hatası değil.** `Diamond3D` yolu `inner_ratio >= 0.99` kontrolüyle ikiye ayrılır (`shape3d.rs:434-454`); varsayılan 0.382 (`graph.rs:1211-1213`). Yani `{"type":"shape_3d","shape_kind":"diamond_3d"}` yazan her proje **8 noktalı yıldız** üretir. `diamond_3d` adı yanıltıcıdır.
- **G2 — "Beyaz" üç bağımsız terimden gelir.** (a) `outline_color_rgba` varsayılanı opak beyazdır (`graph.rs:1203-1205`) ve stroke yüzleri gölgesiz `base*hdr` döner (`wgsl:365-370`); (b) `stroke_width=0.06`, yani yarıçapı 0.5 olan şekilde dışa doğru **%12** genişleme (`shape3d.rs:459`); (c) Neon3D fresnel rim'i beyaza %65 kaydırır (`wgsl:483-486`). Üçü birlikte varsayılan `diamond_3d + material_mode 0` kombinasyonunu **beyaz parlayan yıldız** yapar.
- **G3 — `material_mode 0` "düz renk" değildir.** Neon3D, Cook-Torrance GGX + kamera-bağıl studio IBL + emissive gövde çalıştırır (`wgsl:466-490`); düz renk isteyen kullanıcı `material_mode 2` almalıdır (`wgsl:372-376`). Beyazlığın algısal kaynağı budur.
- **G4 — Ölçek iki çarpanlıdır.** `size` geometriyi (`shape3d.rs:430`), `scale` model matrisini (`compositor.rs:5516`) ölçekler; ikisi çarpılır. `size=1.0, scale=[1,1,1]` varsayılanında şekil yüksekliği ≈1.12 (stroke dahil).
- **G5 — Varsayılan kadraj şekli küçültmez.** Kamera `z=2.5`, `fov=50` → `z=0`'da görünür yükseklik `2*2.5*tan(25°)=2.3315`; 1.12'lik şekil karenin **%48**'ini kaplar. Tam kare için `size ≈ 2.2` gerekir. Yani "dev" görünüm varsayılanla **açıklanmaz**; `size`/`scale` büyütülmüş ya da kamera yaklaştırılmış olmalıdır (kaldırılan düğümlerin değerleri depoda yok — G10).
- **G6 — Scene3D düğüm kamerasını sessizce yutar.** Asta overlay'i `scene_3d` + `z=2.9`, `fov=46` (`build_project.py:221-231`); bu sahnede görünür yükseklik 2.4619, 1.12'lik şekil **%45.5**. Çocuk `Shape3D`'nin `camera_pos`'u yok sayılır (`compositor.rs:3262, 3399`); yazar şekli küçültmek için `camera_pos` yazdıysa **hiçbir etkisi olmadı**.
- **G7 — Beyazlık Asta zincirinde büyür.** `deep_glow` eşiği 0.82, yoğunluk 0.34 ve `halation 0.42` (`build_project.py:246-252`) beyaz stroke'u eşik üstü yakalayıp dışa taşırır; silüet geometriden daha büyük okunur.
- **G8 — Grid altyapısı genellenebilir, çünkü ilkeller grid-bağımsız.** `fill_rect_u8`, `blit_frame_with_border`, `render_label_badge[_scaled]`, `Rgba8Frame` hiçbir grid sabiti içermez (`inspection.rs:1175-1213`; `text.rs:778-798`). Grid'e bağlı olan tek şey **düzen aritmetiğidir**: 91-98 (3×2), 514-523 (4×2), 836-902 (kart döngüsü). İki farklı sabit düzen, soyutlamanın eksik olduğunun kanıtıdır.
- **G9 — Etiketler hücreye kırpılmaz.** `render_label_badge` yalnız **tuval** sınırına kırpar (`text.rs:825-826, 854`); hücre scissor'u yoktur. Uzun parametre satırı komşu hücrenin üstüne boyar. Glif seti ASCII 0x20–0x7E'dir (`font.rs:15-17`), yani `=`, `.`, `/`, `%` güvenli.
- **G10 — Kaldırılan şekillerin kanıtı yok.** `asta_v3_project.json` içinde `shape_3d` yok; `material_mode` yalnız `Text3D` alanlarında (`1293`). "Dört şekil kaldırıldı" ifadesi kullanıcı raporudur, depoyla **doğrulanamaz**.
- **G11 — Manifest geriye uyumludur ama alan eklemek dikkat ister.** `InspectionPackManifest` `Serialize+Deserialize` türetir ve alanlarında `serde(default)` yoktur (`inspection.rs:57-76`); yeni alan eklenirse eski JSON okunamaz.

## Çıkarımlar ve Öneriler

- **Kök neden bir "malzeme seçimi" değil, üç varsayılanın birleşimidir** (G1+G2+G3). Kullanıcı doğru düğümü seçti; varsayılanlar yıldız + beyaz + emissive üretti. Bu yüzden çözüm "material_mode'u değiştir" değil, **varsayılanı görünür kılan bir QC yüzeyi**dir.
- **Galeri, `inspect` pack'inin parçası olmamalıdır.** Pack projeye bağlıdır (`revision_id`, tick'ler, klipler); galeri ise **projeden bağımsız** bir motor referans tablosudur (5 şekil × 4 malzeme sabit). Ayrı komut, pack sözleşmesini ve M2/M5 kapılarını hiç etkilemez.
- **Ölçek normalizasyonu tek kamerayla yapılabilir.** Beş şeklin tamamı ≈`size` kadar (0.90–1.0×, G4); `size=1.0` + stroke ile yarı yükseklik 0.56; hedef dolgu %72 için `camera_z = 0.56/(0.72*tan(25°)) ≈ 1.67`. Tek kamera, beş şekli %66–72 bandında tutar; "sabit kamera" şartı böylece bozulmaz.
- **Renk karşılaştırması malzemeden ayrılmalı.** Aynı `color_top/bottom/side` üçlüsü dört malzemede de kullanılmazsa hücreler malzemeyi değil rengi karşılaştırır. Öneri: sabit test paleti (ör. `top=[1.20,0.62,0.08]`, `bottom=[1.00,0.15,0.02]`, `side=[0.10,0.02,0.00]`) ve `outline=[1,1,1,1]` yerine **koyu** outline (`[0.05,0.05,0.06,1]`) — aksi halde G2 tekrar eder.
- **Manifest renkleri lineer Rec.709 `f32` taşımalı**, tuvaldeki çip ise `linear_to_srgb` ile kodlanmalı (`frame.rs:601`); aksi halde çip ile render farklı görünür.
- **Varsayılanlar değiştirilmemeli.** `inner_ratio` varsayılanını 0.99 yapmak mevcut projelerin silüetini değiştirir (geriye uyumluluk ihlali). Doğru çözüm: galeride her hücreye `inner_ratio` ve `stroke_width` yazmak + `diamond_3d`'nin varsayılanının yıldız olduğunu belgelemek.

## Tasarım Seçenekleri (Karar Noktaları)

| # | Seçenek A | Seçenek B | Tradeoff | Öneri |
|---|---|---|---|---|
| 1 | `materials --out-dir` (yeni alt komut, projesiz) | `inspect <p> --materials` (bayrak) | A: pack sözleşmesi sıfır risk, galeri projeden bağımsız. B: tek komut ailesi ama `inspect` anlamı bulanır ve her çağrıda 20 render eklenir | **A** |
| 2 | Galeri projeden bağımsız sabit palet | `--project` ile ilk `Shape3D`'nin renklerini miras al | A: deterministik referans. B: kullanıcının paletini görür ama çıktı projeye bağlı olur | **A + opsiyonel `--project`** |
| 3 | 20 hücre tek sayfa (4×5, 320×180 → 1360×1064) | İki sayfa (4×3 + 4×2) | A: tek bakışta karşılaştırma; `inspection-contract.md:63` "en fazla 4×3" tavsiyesini aşar (ama o madde ContactSheet'e özgü). B: sözleşmeye harfiyen uyar, iki dosya | **A**, spec'e "materials grid" istisnası notuyla |
| 4 | Renkler dört malzemede sabit | Malzeme başına imza paleti | A: malzeme farkı izole edilir. B: palet kataloğu olur, malzeme farkı renge karışır | **A** (B ikinci sayfa olarak eklenebilir) |
| 5 | Tek sabit kamera (`z≈1.67`) | Şekil başına fit kamera | A: "sabit kamera/ölçek" şartına uyar, 5 şekil %66–72 bandında. B: her hücre aynı dolgu, ama kamera değişir | **A** |
| 6 | `InspectionPackManifest`'a `materials_sheet_path` ekle | Hiç dokunma, ayrı manifest | A: tek manifest. B: eski JSON'lar bozulmaz, pack byte-stabil kalır | **B** |
| 7 | Varsayılanları düzelt (`inner_ratio=0.99`) | Varsayılanları koru, belgele + galeri | A: "diamond" adı doğru olur. B: mevcut projeler bozulmaz | **B** |

KARAR GEREKİYOR: CLI yüzeyi — ayrı `materials` alt komutu mu, `inspect --materials` bayrağı mı? (Öneri: ayrı komut.)
KARAR GEREKİYOR: Galeri projeden bağımsız mı olsun, yoksa `--project` ile renk mirası mı alsın? (Öneri: bağımsız + opsiyonel `--project`.)
KARAR GEREKİYOR: 20 hücre tek sayfada mı (4×5), yoksa iki sayfada mı (4×3 + 4×2) üretilsin? (Öneri: tek sayfa.)
KARAR GEREKİYOR: Renkler dört malzemede sabit mi, malzeme başına imza paleti mi? (Öneri: sabit.)
KARAR GEREKİYOR: Kamera tek sabit mi, şekil başına fit mi? (Öneri: tek sabit.)
KARAR GEREKİYOR: `inspection_manifest.json`'a yeni alan eklensin mi? (Öneri: hayır.)
KARAR GEREKİYOR: `diamond_3d` varsayılan `inner_ratio` 0.382 kalsın mı? (Öneri: kalsın; davranış değişikliği ayrı ADR ister.)
KARAR GEREKİYOR: `AGENTS.md:39` komut listesine `materials` eklensin mi? (Kullanıcı dosyası; ayrı onay.)

## Uygulama Planı

1. **`crates/frameshift-core/src/inspection.rs` — grid soyutlaması (yeni; mevcutları bozmadan).**
   - `struct GridSheetSpec { cols, rows, cell_w, cell_h, pad, header_h }` + `fn grid_sheet_dimensions(&GridSheetSpec) -> (u32,u32)`. Formül: genişlik `cols*cell_w + (cols+1)*pad`, yükseklik `header_h + rows*cell_h + (rows+1)*pad`; 91-98 ve 514-523'ün genel hali.
   - `fn grid_cell_rect(spec, col, row) -> (x, y, w, h)` — testlerin piksel ölçümü için.
   - `fn draw_grid_cell(sheet, spec, col, row, title, frame, footer: &[(&str, [u8;4])])` — 836-902'nin genel hali; başlık `scale=2`, footer `scale=1`. Metin genişliği hücreyi aşıyorsa `CoreError` döndür (G9).
   - **Mevcut contact/showcase kodunu bu turda refactor etme**; yalnız yeni galeri kullanır, böylece `fx_showcase_pack.png` bit-bazında değişmez.
2. **`inspection.rs` — galeri üretimi.** `pub(crate) fn export_materials_gallery_impl(engine, out_dir) -> Result<MaterialsManifest, CoreError>`:
   - `base_project = Project::golden_scene_preset()`; `param_bindings.clear()`, klip `event_animations.clear()` (emsal 504-508).
   - Sabitler: `SHAPES=[NeonTile,HollowStar5,Heart3D,LightningBolt,Diamond3D]`, `MATERIALS=[(0,"neon_3d"),(1,"metallic_chrome"),(2,"flat_2d_stroked"),(3,"iridescent")]`, `SIZE=1.0`, `FOV=50.0`, `FILL=0.72`, `CAMERA_Z≈1.67`.
   - Her hücre: `RenderGraphDef { output_node: "shape_out", nodes: vec![GraphNodeDef { id:"shape_out", kind: NodeKind::Shape3D { shape_kind, size:SIZE, aspect:1.0, stroke_width:0.06, corner_radius:0.0, inner_ratio:0.382, extrude_depth:0.12, bevel_size:0.01, material_mode, dört renk, pos:[0,0,0], rot_deg:[0,0,0], scale:[1,1,1], camera_pos:[0,0,CAMERA_Z], camera_rot_deg:[0,0,0], fov_deg:FOV, pop_duration_sec:0.0, pop_overshoot:1.0, spin_in_deg:0.0, echo_count:0, echo_offset:[0,0,0], echo_alpha_decay:1.0, hdr_intensity:1.0, time_offset_sec:0.0, opacity:1.0 }, inputs: vec![], enabled: true }] }` → `validate()` → `render_frame_with_profile_def_pub` (`inspection.rs:908-920` deseni).
   - Hücre profili: 320×180, `motion_blur_sub_samples:1`, `shutter_angle_deg:0`; `RenderOptions { tonemap_mode: Some(AcesLite), output_alpha_mode: Straight, exposure: 1.0, .. }` (emsal 539-553).
   - `frameshift_vector::build_shape3d_mesh(Shape3DParams{..})` ile **aynı parametrelerle** mesh bounds hesapla; `height_fraction = (max.y-min.y) / (2*CAMERA_Z*tan(FOV/2))` manifest'e yaz (G5'in ölçülebilir hali).
   - Tuval: `Rgba8Frame::filled` + header `fill_rect_u8` + 20× `draw_grid_cell`; her hücreye üç renk çipi (`fill_rect_u8` + `linear_to_srgb`, `frame.rs:601`).
   - Çıktı: `materials_sheet.png`, `materials_manifest.json`; zaman alanı **`sample_tick: Flicks = Flicks::ZERO`**, `f64` saniye yok (`timebase.md:22-25`).
3. **Manifest tipleri (`inspection.rs`).** `MaterialsManifest { schema_version, grid:{cols,rows,cell_width,cell_height}, sheet:{width,height,fnv1a_hash64,sha256_hex}, camera:{pos,fov_deg}, color_space:"linear_rec709", tonemap:"aces_lite", sample_tick: Flicks, cells: Vec<MaterialsCellRecord> }`; `MaterialsCellRecord { row, col, shape_kind, material_mode, material_name, size, scale, rot_deg, color_top_rgba, color_bottom_rgba, side_color_rgba, outline_color_rgba, pbr_defaults:{roughness,metallic,ao_strength}, mesh_bounds_min, mesh_bounds_max, height_fraction, pixel_hash64, sha256_hex, non_bg_pixels }`. **Zaman damgası alanı yok** (determinizm).
4. **`crates/frameshift-core/src/engine.rs:96-108` yanına** `pub fn export_materials_gallery(&mut self, out_dir:&Path) -> Result<MaterialsManifest, CoreError>`.
5. **`crates/frameshift-core/src/lib.rs:31`** yanına `MaterialsManifest`, `MaterialsCellRecord` re-export.
6. **`crates/frameshift-cli/src/main.rs`:** (a) enum 30-70'e `Materials{out_dir:PathBuf}`; (b) parser 1093'ten sonra `"materials"` kolu (`--out-dir` zorunlu, proje argümanı yok); (c) dispatch 320'ye kol; (d) usage 1219'a satır; (e) parse testi 1284-1290 yanına.
7. **Testler — `crates/frameshift-core/tests/materials_gallery_tests.rs` (yeni):**
   - `materials_gallery_writes_20_cells`: `cells.len()==20`, her `(shape,material)` çifti bir kez, iki dosya var, tuval boyutu formülle uyumlu.
   - `materials_gallery_cells_are_not_white`: her hücrede `non_bg_pixels` > hücre alanının %8'i **ve** doygun palette `max(r,g,b)-min(r,g,b) > 24` (G2 regresyonu).
   - `materials_gallery_fit_is_normalized`: her hücrede `height_fraction ∈ [0.60, 0.85]` (G5 regresyonu).
   - `materials_gallery_is_deterministic`: iki kez üret; `sheet.fnv1a_hash64`, her `pixel_hash64` ve JSON byte'ları eşit (`frame.rs:108-118`).
   - `diamond_3d_default_is_star`: `inner_ratio=0.382` ile `inner_ratio=1.0` mesh vertex sayıları farklı; ikincisi 4 köşeli (G1 belgesi).
8. **Geriye uyumluluk:** `Shape3D` alanları ve varsayılanları **değişmez** → eski proje JSON'ları aynı yüklenir (`graph.rs:781-839`). `InspectionPackManifest` **değişmez** (seçenek 6B) → `inspect` pack'i ve MCP `inspect_project` aynı kalır (`server.rs:275-290`). Yeni manifest kendi `schema_version` alanını taşır ve okunurken tüm alanlar `#[serde(default)]` ile yüklenebilir olmalı.

## Doğrulama Planı

```powershell
cargo test -p frameshift-core --test materials_gallery_tests
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run --release -p frameshift-cli -- materials --out-dir target/materials
cargo run --release -p frameshift-cli -- materials --out-dir target/materials_2
Get-FileHash target/materials/materials_sheet.png, target/materials_2/materials_sheet.png -Algorithm SHA256
```

Kabul kriterleri:
- `materials_sheet.png` 20 hücre içerir; her hücrede silüet görünür (`non_bg_pixels` > %8) — **piksel varlığı**.
- `materials_manifest.json`: 20 kayıt; her birinde `shape_kind`, `material_mode`, dört lineer renk, `camera`, `height_fraction`, iki hash — **manifest alanları**.
- İki ayrı çalıştırmanın PNG SHA-256'sı ve JSON byte'ları **birebir aynı** — **determinizm**.
- Hiçbir hücrede `height_fraction` [0.60, 0.85] dışında değil — **dev ölçek yok**.
- Doygun palette kanal farkı > 24 — **beyaz taşması yok**.
- `inspect` çıktısı (contact sheet + manifest alan kümesi) değişmemiş — **pack bozulmadı**.

## Riskler ve Doğrulanmamış Noktalar

- **Etiket taşması (G9):** `render_label_badge` hücreye kırpmaz; 320px hücrede ~45 karakterden uzun satır komşuya boyar. Önlem: metin genişliğini `draw_grid_cell` içinde ölç, sığmıyorsa hata döndür (sessiz kırpma yok).
- **GPU'ya bağlı determinizm:** `fnv1a_hash64` aynı makinede tekrarlanabilir; farklı adaptör/sürücü arasında bit-eşitliği **doğrulanmadı**. Test "aynı çalıştırmada iki kez" olmalı.
- **20 render maliyeti:** tek `Engine` içinde 20 `render_frame_with_profile_def_pub`; süre **ölçülmedi**, bütçe bilinmiyor.
- **"Dev" iddiası kısmen doğrulanmadı (G10):** kaldırılan düğümlerin `size`/`scale` değerleri depoda yok. Varsayılanla %48, tam kare için `size≈2.2`; kullanıcının girdiği değer **bilinmiyor**.
- **`inner_ratio` iki şekli paylaşıyor:** `HollowStar5` ve `Diamond3D` aynı alanı kullanır (`graph.rs:793-794`); galeri ikisini aynı değerle çizerse fark yalnız köşe sayısı olur. `HollowStar5` için ayrı değer (ör. 0.45) manifest'e yazılmalı.
- **Renk çipi ≠ render rengi:** çip `linear_to_srgb` ile kodlanır, render ise ACES-lite + bloom'dan geçer (`inspection.rs:548-553`); çip "yazılan renk"tir, "görünen renk" değildir.
- **Sözleşme boşluğu:** `inspection-contract.md` şekil/malzeme galerisini tanımlamaz (`45`); yeni yüzey eklemek `locked` spec değişikliği gerektirir — bu araştırma **spec değiştirmedi**.
- **`AGENTS.md` komut listesi:** yeni alt komut eklenip `AGENTS.md:39` güncellenmezse araç keşfi sorunu (asta-v3 raporu, satır 34) tekrar eder; bu dosya kullanıcı onayı gerektirir.
