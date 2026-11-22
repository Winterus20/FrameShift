# SC-0002 Yeniden Üretimi — Eksik Yetenek ve Yol Haritası Analizi

**Tarih:** 2026-11-22
**Referans:** [@asgardski — Mushoku Tensei kinetic-typography AMV](https://www.tiktok.com/@asgardski/video/7672463186175544609) (1920×1080, 60 fps, 31.53 sn, 59 plan)
**Stil kartı:** [SC-0002](../style-cards/SC-0002-mushoku-tensei-kinetic-type-amv.md)
**Üretilen proje:** `local_media/mushoku/mushoku_project.json` (260 düğüm, 126 binding, 59 klip, 30 kelime)
**Üretici betik:** `local_media/mushoku/build_project.py`

Bu belge, referans editin FrameShift ile yeniden üretilmesi sırasında **fiilen çarptığımız** sınırları ve bunların kapatılması için gereken değişiklikleri kaydeder. Her madde ya bu oturumda denendi ya da kodda kanıtlandı.

---

## 1. Referansın tekniği (ölçülmüş)

| Öğe | Ölçüm |
| :--- | :--- |
| Vuruş ızgarası | 0.4295 sn ≈ **139.7 BPM** (spektral flux tepe noktaları) |
| Plan sayısı | **59** (kare farkı analizi, eşik 0.116 normalize) |
| Kesme ritmi | Planların ~%85'i **1 vuruş**, 3 yerde 6 vuruşluk nefes |
| Tipografi | Tek font, tamamı büyük harf, ağır condensed grotesk; kelime kare genişliğinin %60–75'ine yayılıyor |
| Kelime sayısı | 30 kelime / 31.53 sn; kelime ömrü **0.6–1.4 sn**, çoğu plan değişimini aşıyor |
| Harf yerleşimi | Harf başına ±0.02–0.09 dikey sapma, hafif döndürme |
| Örtme | Kelimeler **özne silüetinin arkasında** kalıyor (kare kare doğrulandı) |
| Grade | Düşük doygunluk, güçlü bloom/halation, 35 mm grain, vinyet, ince chromatic edge |

---

## 2. FrameShift ile ne kadar eşleşti?

| Referans tekniği | Durum | Nasıl yapıldı / neden olmadı |
| :--- | :--- | :--- |
| Tam kare dev kelime | ✅ Eşleşti | `vector_text` + Anton, `font_size_norm` 0.20–0.34 |
| Harf harf yayma | ⚠️ Yaklaşıklama | Şemada harf dizisi yok → **harf başına ayrı düğüm** + `x_offset_norm`/`y_offset_norm` |
| Harf başına renk | ⚠️ Yaklaşıklama | `vector_text` tek renk → harf başına düğüm; `text_3d` paleti 4 renkle sınırlı |
| Harf başına döndürme | ❌ Yok | `vector_text`'te rotasyon alanı yok; `transform2_d` zinciri gerekir (denenmedi) |
| Metne özel RGB split | ❌ Yok | Yalnız tam kare `film_optics.chromatic_edge`; metin katmanına uygulanamıyor |
| Kelimenin özne arkasında kalması | ⚠️ Kaba taklit | `color_basic` ile kontrastı şişirilmiş luma maskesi + %58 blend. Koyu silüetlerde kısmen çalışıyor, açık renkli giysilerde çalışmıyor |
| Kelime kelime stagger giriş | ✅ Eşleşti | Harf başına `opacity` binding'i, 0.028 sn gecikme |
| Kelimenin plan boyunca yaşaması | ✅ Eşleşti | Tipografi katmanı plan zincirinin üstünde tek katman |
| Vuruşa kilitli kesme | ⚠️ Elle | Vuruş ızgarası dışarıda hesaplandı; `peaks` kesme üretmiyor |
| Whip / cross-zoom / burn geçiş | ✅ Eşleşti | 10 yerleşik geçiş shader'ı, `progress` binding'i |
| Kolaj panel ekleri | ❌ Yok | Kırpma düğümü yok; `transform2_d`+`mask_matte` ile kurulabilir ama denenmedi |
| Bloom / halation / grain | ✅ Eşleşti | `deep_glow` + `film_optics` |
| S/B (desatüre) bölüm | ✅ Eşleşti | `color_basic.saturation` binding'i |
| Kurgu akıcılığı | ❓ Doğrulanmadı | Yalnız kare ızgarası ve tekil kareler incelendi |

---

## 3. Eksik yetenekler (öncelik sırasıyla)

### P0 — Harf bazlı tipografi yerleşimi
**Kanıt:** `crates/frameshift-schema/src/graph.rs:419-481` (`VectorText`), `crates/frameshift-vector/src/text.rs:872-996` (`layout_text` tek `advance` kullanır).
**Sorun:** Referansın bütün kimliği "kelimeyi kareye yay, harfleri ayrı ayrı oynat" üzerine kurulu. Şemada harf dizisi yok. Bu oturumda her harf için ayrı `vector_text` düğümü kuruldu: 30 kelime → **190 harf düğümü**, her biri kare boyutunda doku üretiyor.
**Öneri:** `VectorText`'e `per_glyph: Vec<GlyphOverride>` ekle — `{ index, dx_norm, dy_norm, rot_rad, scale, color_rgba, delay_sec }`. Tek düğüm, tek doku, harf başına keyframe. Bu, düğüm sayısını ~10× düşürür ve harf rotasyonunu da çözer.

### P0 — Özne farkındalıklı örtme (text-behind-subject)
**Kanıt:** `graph.rs:364-373` (`MaskMatte` yalnız Alpha/Luma/Red), metin düğümleri 0 girdili (`graph.rs:1352-1396`), depth-map düğümü yok.
**Sorun:** Referansın imzası kelimelerin karakterin arkasında kalması. Luma maskesi yalnız "koyu = gizle" yapıyor; açık ten/giysi üzerinde metin tamamen görünür kalıyor (kare doğrulamasında görüldü).
**Öneri:** (a) `DepthMap` düğümü (ONNX/Depth-Anything küçük modeli veya ffmpeg tarafında önceden üretilmiş derinlik videosu) ve `MaskMatte`'a `Depth` modu; (b) `MatteSource` varlık türü (alfa taşıyan PNG dizisi/ProRes 4444) ve metin düğümüne maske girdisi; (c) hazır `text_behind` kompozit modu.

### P1 — Metne özel efekt katmanı
**Kanıt:** Metin shader'larında CA yok; CA yalnız `fx_film_optics.wgsl:129`, `fx_radial_shockwave.wgsl` ve geçişlerde.
**Sorun:** Referansta harflerde ince RGB kayması var. Bunu yapmak için `precomp` + `film_optics` turu veya `custom_wgsl` yazmak gerekiyor.
**Öneri:** `VectorText`/`Text3D`'ye `chromatic_shift`, `blur_radius`, `displace_amount` alanları; ya da `EffectLayer` düğümü (girdi katmanına efekt uygular, tam kareye değil).

### P1 — Kırpma / panel düğümü
**Kanıt:** Hiçbir düğümde kaynak dikdörtgeni (crop rect) yok (`graph.rs:355-373`); `grid_warp` UV warp, layout değil.
**Sorun:** Referansta beyaz çerçeveli küçük fotoğraf panelleri var. Kurmak için `transform2_d` + kapalı `vector_path_hud` maskesi hilesi gerekiyor.
**Öneri:** `crop_rect` alanı (`transform2_d` veya `source_*`) + `panel` düğümü (rect, kenarlık, gölge, açı).

### P1 — Vuruş altyapısı
**Kanıt:** `beat_phase` 120 BPM'e sabit (`crates/frameshift-render/src/compositor.rs:234-239`); `peaks` yalnız yazdırıyor, kesme üretmiyor (`crates/frameshift-cli/src/main.rs:900-981`).
**Sorun:** 139.7 BPM'lik referans ızgarasını dışarıda Python ile hesapladım ve 59 klibi elle yazdım.
**Öneri:** Projeye `tempo` alanı (`bpm`, `offset`, `beats_per_bar`); `peaks --author-cuts` ile kesme listesini doğrudan projeye yaz; `beat_phase` bu tempodan beslensin.

### P2 — SafeExpr ifade gücü
**Kanıt:** `crates/frameshift-time/src/expr.rs:5-11` — yalnız `+ - * /`, tekli işaret, 15 fonksiyon; karşılaştırma/ternary/if yok.
**Sorun:** Her harf için ayrı zarf yazmak uzun `smoothstep` zincirleri üretiyor; 126 binding'in çoğu bu yüzden.
**Öneri:** `if(cond, a, b)`, `< <= > >= == !=`, `&& || !`, `mix`, `floor`, `ceil`, `% `. (Zaten `brain/state.md` "sıradaki adımlar" listesinde.)

### P2 — Metin içeriğinin animasyonu
**Kanıt:** `text` hiçbir düğümde bindable değil (`crates/frameshift-schema/src/project.rs:219-236`).
**Sorun:** Daktilo etkisi, kelime değiştirme, sayı sayacı yapılamıyor; her durum için ayrı düğüm.
**Öneri:** `text` yerine `text_source` bağlanabilir alan (bir dizi metin + zaman aralıkları) ya da `TextSequence` düğümü.

### P2 — Varsayılan font uçurumu
**Kanıt:** Font verilmezse 8×12 ASCII bitmap fonta düşüyor (`crates/frameshift-vector/src/font.rs:15-17`).
**Öneri:** Gömülü bir condensed sans (Anton, SIL OFL) varsayılan yap; `font_path` boşsa uyarı üret.

### P3 — Renk uzayı tuzağı
**Kanıt:** `color_rgba` lineer Rec.709 bekliyor; şema sRGB girdiyi kabul edip sessizce yanlış sonuç veriyor.
**Sorun:** Bu oturumda renkleri sRGB olarak yazdım ve metin lerleşmiş/beyazımsı çıktı; `sRGB→lineer` dönüşümü ekleyince düzeldi.
**Öneri:** `color_srgb` yardımcı alanı veya yükleme sırasında "0–1 aralığında ama lineer değil" uyarısı.

### P3 — Siyah kare koruması
**Kanıt:** Yok. Kaynak klipte siyah aralık varsa plan simsiyah render ediliyor (bu oturumda ilk denemede oldu).
**Öneri:** Varlık yüklemede `blackdetect` taraması + `--strict-sources` kapısı; ya da `SourceVideo`'ya `skip_black_ranges`.

### P3 — Yazar deneyimi
- Proje yazmak için CLI'da hiçbir yazım alt komutu yok; tek yol MCP `mutate_project` veya JSON'u elle üretmek (`crates/frameshift-mcp/src/server.rs:293-313`).
- `render` ilerleme/ETA basmıyor; uzun render'larda geri bildirim yok.
- `inspect` contact sheet üretiyor ama **kelime zaman çizelgesi** veya **plan listesi** raporu yok.

---

## 4. Önerilen sıra (etki / efor)

1. **`per_glyph` dizisi** — tek başına bu stilin en büyük darboğazı; şema + Slug vertex üretimi + binding. (Orta efor, çok yüksek etki.)
2. **Derinlik/matte girdisi** — "text behind subject" ancak bununla gerçek olur. (Yüksek efor, çok yüksek etki.)
3. **Tempo + `--author-cuts`** — her AMV'de tekrar eden 30–60 satırlık elle kesme yazımını bitirir. (Düşük efor, yüksek etki.)
4. **`crop_rect` + panel** — kolaj stilleri için. (Düşük efor, orta etki.)
5. **SafeExpr genişletmesi** — 126 binding'i ~40'a indirir. (Düşük efor, orta etki.)
6. **Metne özel efekt katmanı** — RGB split/blur. (Orta efor, orta etki.)

---

## 5. Epistemik sınır

- Referans **kesintisiz oynatılmadı**; akıcılık, ses senkronu ve gerçek tempo doğrulanmadı. Ölçümler kare farkı ve spektral flux analizine dayanır.
- Referansın kaynak projesi yok; hangi uygulama/eklenti ile yapıldığı bilinmiyor. Font kimliği (Anton benzeri) **tahmindir**.
- Yeniden üretimde kaynak görüntü **Mushoku Tensei** klipleri (6 dosya, 1080p) kullanıldı; kaynaklar YouTube'dan alınmış, yeniden encode edilmiş, yerel stil testi içindir.
- Müzik, referans videonun ses kanalından çıkarıldı (`local_media/mushoku/ref_track.wav`); dağıtım için lisanslı değildir.
- Üretilen edit bir **stil eşleşme testidir**, referansın birebir kopyası değildir; kadraj, karakter ve renk içeriği farklıdır.
