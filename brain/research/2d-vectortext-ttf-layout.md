---
title: "2D VectorText Gerçek TTF/OTF Font, Çok Satır Yerleşimi, Hizalama ve Önbellek Şartnamesi"
date: 2026-11-22
status: draft
tags: [text-vector, schema, render-graph, engine]
---

# 2D `VectorText` — Gerçek TTF/OTF Font, Çok Satır Yerleşimi, Hizalama ve Önbellek Şartnamesi

## Soru / Amaç

FrameShift'te 3D metin düğümü (`NodeKind::Text3D`) Faz 6.2–6.3 ile `ttf-parser` + `lyon_tessellation` üzerinden gerçek TTF/OTF fontları desteklerken, 2D üreteç düğümü `NodeKind::VectorText` hâlâ sınırlı bir 7 alanlı şemaya ve gömülü ASCII SDF/kapsül rasterizer'larına bağlıdır. Ayrıca [project-schema.md §5 #9](../specs/project-schema.md#L111-L112) ile [render-graph.md §3.1 ve Kenar Durumlar](../specs/render-graph.md#L63-L67) tarafından kilitlenen `font_family`, `font_weight`, `align` (`left | center | right`), çok satır, boşluklandırma ve `FontNotFound` zorunluluğu mevcut kod tabanında henüz karşılanmamaktadır.

Bu belge, **sıfır yeni Cargo crate** ekleyerek (`ttf-parser 0.25.1` ve `lyon_tessellation 1.0.23` zaten workspace'te yüklüdür):

1. `NodeKind::VectorText` şemasını **%100 geriye uyumlu** (`#[serde(default)]`) genişletmeyi,
2. Hem `font_family` hem `font_path` `None` olduğunda **M1/M2 Golden Scene hash'ini** birebir korumayı,
3. `font_path` veya `font_family` verildiğinde sessiz rastgele fonta düşmeden **`FontNotFound`** garantili gerçek TTF/OTF/TTC çözümlemeyi,
4. Çok satır (`\n`), `Left | Center | Right` hizalama, `x_offset_norm`, `tracking_em`, `line_height_em`, `kern` tablosu, dolgu + kontur ve `opacity` ile `Rgba16fFrame` (`LinearRec709` premultiplied) rasterizasyonunu,
5. `resolver.rs` ve `compositor.rs` önbellek anahtarı ile hata yayılımını

uygulamaya hazır teknik şartname olarak tanımlar.

---

## İncelenen Birincil Kaynaklar

### Harici Birincil Kaynaklar (Doğrulanmış API ve Spesifikasyonlar)
- **`ttf-parser` v0.25.1 (`Face`, `Weight`, `Style`, `name_id`, `kern`, `fonts_in_collection`):** [https://docs.rs/ttf-parser/0.25.1/ttf_parser/struct.Face.html](https://docs.rs/ttf-parser/0.25.1/ttf_parser/struct.Face.html) ve [https://docs.rs/ttf-parser/0.25.1/ttf_parser/enum.Weight.html](https://docs.rs/ttf-parser/0.25.1/ttf_parser/enum.Weight.html) (yerel crate kaynağı `ttf-parser-0.25.1/src/lib.rs`, `src/tables/os2.rs`, `src/tables/kern.rs`, `src/tables/name.rs` ile birebir doğrulandı) — erişim/inceleme tarihi: 2026-11-22
- **`lyon_tessellation` v1.0.23 (`FillTessellator`, `StrokeTessellator`, `FillOptions`, `StrokeOptions`, `FillRule`, `LineJoin`, `LineCap`):** [https://docs.rs/lyon_tessellation/1.0.23/lyon_tessellation/](https://docs.rs/lyon_tessellation/1.0.23/lyon_tessellation/) (yerel crate kaynağı `lyon_tessellation-1.0.23/src/lib.rs`, `src/fill.rs`, `src/stroke.rs` ile birebir doğrulandı) — erişim/inceleme tarihi: 2026-11-22
- **OpenType `name` ve `kern` Tablo Spesifikasyonu:** [https://learn.microsoft.com/en-us/typography/opentype/spec/name](https://learn.microsoft.com/en-us/typography/opentype/spec/name) ve [https://learn.microsoft.com/en-us/typography/opentype/spec/kern](https://learn.microsoft.com/en-us/typography/opentype/spec/kern) — erişim/inceleme tarihi: 2026-11-22

### Yerel Kod Tabanı ve Sözleşme Dosyaları (Doğrudan İncelenen)
- [project-schema.md §5 #9](../specs/project-schema.md#L111-L112) (`VectorText` şema sözleşmesi)
- [render-graph.md §3.1 ve Kenar Durumlar](../specs/render-graph.md#L63-L110) (`VectorText` renk/alfa/determinizm ve `FontNotFound` kuralı)
- [crates/frameshift-schema/src/graph.rs](../../crates/frameshift-schema/src/graph.rs#L264-L278) (`NodeKind::VectorText`, `Text3D`, `validate_graph`)
- [crates/frameshift-schema/src/project.rs](../../crates/frameshift-schema/src/project.rs#L196-L220) (`bindable_param_names`, `golden_scene_preset` satır 926–939)
- [crates/frameshift-vector/Cargo.toml](../../crates/frameshift-vector/Cargo.toml#L1-L15) (`ttf-parser = "0.25"`, `lyon_tessellation = "1.0"`)
- [crates/frameshift-vector/src/text.rs](../../crates/frameshift-vector/src/text.rs#L1-L783) (`VectorTextConfig`, `draw_vector_text`, `layout_text`, `render_vector_text_rgba16f`)
- [crates/frameshift-vector/src/font.rs](../../crates/frameshift-vector/src/font.rs#L1-L260) (gömülü 8×12 ASCII `GlyphSdf` atlası)
- [crates/frameshift-vector/src/mesh3d.rs](../../crates/frameshift-vector/src/mesh3d.rs#L34-L382) (`load_font_bytes`, `kerning_between`, `LyonGlyphBuilder`, `FillTessellator`, `StrokeTessellator`)
- [crates/frameshift-vector/src/canvas.rs](../../crates/frameshift-vector/src/canvas.rs#L459-L649) (`VectorCanvas`, `fill_polygon` 4×4 SSAA, `premultiply_rgba`, `source_over_premul`, `sdf_coverage`)
- [crates/frameshift-vector/src/error.rs](../../crates/frameshift-vector/src/error.rs#L1-L24) (`VectorError::FontNotFound`, `VectorError::InvalidFont`)
- [crates/frameshift-render/src/vector_text.rs](../../crates/frameshift-render/src/vector_text.rs#L1-L160) (`rasterize_vector_text` kapsül-SDF fallback)
- [crates/frameshift-render/src/compositor.rs](../../crates/frameshift-render/src/compositor.rs#L462-L472) (`apply_animated_parameter`, `font_cache` satır 926, `NodeKind::VectorText` dalı satır 1747–1780)
- [crates/frameshift-render/src/error.rs](../../crates/frameshift-render/src/error.rs#L70-L72) (`RenderError::Vector(#[from] VectorError)`)
- [crates/frameshift-core/src/resolver.rs](../../crates/frameshift-core/src/resolver.rs#L36-L49) (`vector_text_cache`, `render_vector_text_node` satır 286–304, `ProjectFrameProvider` satır 441–507)

---

## Doğrudan Gözlemler (Kanıtlı)

1. **Mevcut Şema Eksikliği ve Sözleşme Farkı:**
   - [graph.rs](../../crates/frameshift-schema/src/graph.rs#L265-L277) içinde `NodeKind::VectorText` yalnızca 7 alan tanımlar: `text`, `font_size_norm`, `color_rgba`, `outline_rgba`, `outline_width_norm`, `align_center`, `y_offset_norm`. [project-schema.md §5 #9](../specs/project-schema.md#L112) ile hedeflenen `font_family`, `font_weight`, `align` (`left | center | right`), `letter_spacing` / `tracking_em`, `line_height` ve `opacity` şemada yoktur.
   - [project.rs](../../crates/frameshift-schema/src/project.rs#L219) içinde `bindable_param_names` yalnız `["font_size_norm", "outline_width_norm", "y_offset_norm"]` döndürür; [compositor.rs](../../crates/frameshift-render/src/compositor.rs#L462-L472) `apply_animated_parameter` da yalnızca bu 3 parametreyi günceller.

2. **Kritik Mimari Tutarsızlık — Çift Rasterizer Ayrışması ve Hata Yutma Hatası:**
   - [resolver.rs](../../crates/frameshift-core/src/resolver.rs#L441-L507) içinde `ProjectFrameProvider::provide_source_frame`, `vector_text://<node_id>` istendiğinde düğüm üzerinde dinamik bağlama yoksa (`!has_dynamic_binding`) `MediaResolver::render_vector_text_node` -> `frameshift_vector::render_vector_text_rgba16f` ([text.rs](../../crates/frameshift-vector/src/text.rs#L253)) yolunu çağırır (`8x12` SDF atlası, `\n` çok satır ve `x_offset_norm` destekli). Ancak `has_dynamic_binding == true` ise `Err(RenderError::FrameProvider { message: "dynamic or non-VectorText fallback requested" })` döndürür.
   - [compositor.rs](../../crates/frameshift-render/src/compositor.rs#L1757-L1770) ise `provider.provide_frame(&override_key, ...)` çağrısından **herhangi bir `Err(_)`** döndüğünde `crate::vector_text::rasterize_vector_text` ([vector_text.rs](../../crates/frameshift-render/src/vector_text.rs#L21)) fonksiyonuna düşer.
   - `vector_text.rs` tamamen farklı bir tek satırlı kapsül-segment SDF rasterizer'ıdır (`\n` karakterlerini atlar, `frameshift-vector/src/text.rs` ile farklı glif şekilleri, farklı advance `0.68 * font_px` ve farklı sol marj `0.06` vs `0.04` kullanır).
   - **Daha kritik sorun:** `compositor.rs:1759`'daki `Err(_)` tüm hataları yakaladığı için `ProjectFrameProvider` içinde oluşacak gerçek bir `FontNotFound` veya `InvalidFont` hatasını da sessizce kapsül rasterizer'a düşürerek yutar; bu doğrudan [render-graph.md §Kenar Durumlar](../specs/render-graph.md#L110) ile çelişir. Üstelik [frameshift-render/Cargo.toml](../../crates/frameshift-render/Cargo.toml#L12) halihazırda `frameshift-vector` crate'ine doğrudan bağımlıdır.

3. **M1/M2 Golden Scene Koruması İçin Gerçek Çalışma Yolu:**
   - [project.rs](../../crates/frameshift-schema/src/project.rs#L927-L939) içinde `Project::golden_scene_preset()`, `impact_title` adlı statik bir `NodeKind::VectorText` düğümü (`text: "FRAMESHIFT // IMPACT"`, `font_size_norm: 0.065`, `align_center: true`, `y_offset_norm: 0.78`) içerir ve `param_bindings` listesinde `impact_title` için bağlama yoktur.
   - Bu nedenle `verify-m1` ve `verify-all` akışlarında `impact_title`, `ProjectFrameProvider` -> `frameshift_vector::render_vector_text_rgba16f` ([text.rs](../../crates/frameshift-vector/src/text.rs)) yolundan geçer. `frameshift_vector::text::VectorTextConfig` varsayılanları (`tracking_em = 0.05`, `line_height_em = 1.3`, `x_offset_norm = None` -> `0.5`, `DEFAULT_LEFT_MARGIN_NORM = 0.04`) korunduğu sürece M1/M2 Golden Scene piksel hash'i korunur.
   - `crates/` genelinde `fnv1a` / `sha256` araması, [vector_tests.rs](../../crates/frameshift-vector/tests/vector_tests.rs#L63-L89) içinde deterministik kendi-kendine eşitlik testlerini ve [compositor_tests.rs](../../crates/frameshift-render/tests/compositor_tests.rs#L813-L916) içinde `non_empty_pixels > 40`, `centered.fnv1a_hash != left_aligned.fnv1a_hash` ve `vector_text://title` override testini doğrulamaktadır.

4. **Mevcut `resolver.rs` Cache Anahtarı Zafiyetleri:**
   - [resolver.rs](../../crates/frameshift-core/src/resolver.rs#L467-L481) içindeki `vector_text_cache: HashMap<(String, u32, u32), Rgba16fFrame>` anahtarı `{:.4}` formatıyla float yuvarlama yapar (örn. `0.06501` ile `0.06504` çarpışır) ve yalnızca eski 7 alanı içerir.

5. **`ttf-parser 0.25.1` ve `lyon_tessellation 1.0.23` Doğrulanan API Gerçekleri:**
   - `ttf_parser::Face<'a>`:
     - `Face::parse(data: &[u8], index: u32)` ve `ttf_parser::fonts_in_collection(data: &[u8]) -> Option<u32>` (`.ttc` desteği).
     - Metrikler (`ttf-parser-0.25.1/src/lib.rs:1499-1906`): `units_per_em() -> u16` (`16..=16384`), `ascender() -> i16`, `descender() -> i16`, `line_gap() -> i16`, `typographic_ascender() -> Option<i16>`, `capital_height() -> Option<i16>`, `glyph_hor_advance(GlyphId) -> Option<u16>`.
     - Ağırlık/Stil/İsim (`lib.rs:1396-1471`, `os2.rs:27-57`, `name.rs:13-150`): `face.weight() -> Weight` ve **`Weight::to_number(self) -> u16`** (`Thin=100 .. Black=900`, `Other(n)=n`); `face.style() -> Style` (`Normal`, `Italic`, `Oblique`); `face.names()` üzerinde `name.is_unicode()` + `name.to_string() -> Option<String>` ile `name_id::FAMILY` (`1`), `name_id::TYPOGRAPHIC_FAMILY` (`16`), `name_id::WWS_FAMILY` (`21`).
     - Kerning (`kern.rs:113-140`, [mesh3d.rs](../../crates/frameshift-vector/src/mesh3d.rs#L368-L382)): `face.tables().kern` alt tablolarında `subtable.horizontal && !subtable.variable` için `subtable.glyphs_kerning(left, right) -> Option<i16>`.
     - **Koordinat Yönü Uyarısı:** `ttf-parser` outline koordinatları Y-yukarıdır (`y > 0` baseline üstü); 3D mesh üreten [mesh3d.rs:312](../../crates/frameshift-vector/src/mesh3d.rs#L312) `y * scale` kullanır, ancak 2D piksel tuvalinde (`y = 0` üst kenar) dikey dönüşüm mutlaka `baseline_y - y * scale` olmalıdır.
   - `lyon_tessellation 1.0.23`:
     - `FillTessellator::tessellate_path` (`FillOptions::tolerance(..).with_fill_rule(FillRule::NonZero)`) ve `StrokeTessellator::tessellate_path` (`StrokeOptions::tolerance(..).with_line_width(..).with_line_join(LineJoin::Round).with_line_cap(LineCap::Round)`) doğrudan mevcuttur.

---

## Çıkarımlar ve Öneriler (Uygulamaya Hazır Mimari Şartname)

### 1. %100 Geriye Uyumlu Şema (`crates/frameshift-schema`)

#### 1.1. `TextAlign2D` ve `NodeKind::VectorText`
`crates/frameshift-schema/src/graph.rs` içinde yeni hizalama tipi eklenir ve `NodeKind::VectorText` mevcut 7 alan korunarak `#[serde(default)]` alanlarla genişletilir:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextAlign2D {
    Left,
    #[default]
    Center,
    Right,
}
```

`NodeKind::VectorText` alan sözleşmesi:

| Alan | Tip | Serde Default | Açıklama |
| :--- | :--- | :--- | :--- |
| `text` | `String` | *(zorunlu)* | Çok satır için `\n` (`\r\n` normalize edilir) destekli UTF-8 metin |
| `font_size_norm` | `f32` | *(zorunlu)* | Referans tuval yüksekliğine normalize em/cap boyutu (`>= 0.0`) |
| `color_rgba` | `[f32; 4]` | *(zorunlu)* | Straight `LinearRec709` dolgu RGBA |
| `outline_rgba` | `[f32; 4]` | `[0.0; 4]` | Straight `LinearRec709` kontur RGBA |
| `outline_width_norm` | `f32` | `0.0` | Tuval yüksekliğine normalize kontur yarı-kalınlığı (radyal dışa taşma) |
| `align_center` | `bool` | `true` | Eski alan; `align` `None` iken `true -> Center`, `false -> Left` |
| `y_offset_norm` | `f32` | `0.0` | Metin bloğunun dikey merkez konumu (`0.0` üst .. `1.0` alt) |
| `font_family` | `Option<String>` | `None` | İsteğe bağlı sistem/proje font aile adı (ör. `"Impact"`, `"Noto Sans"`) |
| `font_path` | `Option<String>` | `None` | İsteğe bağlı doğrudan `.ttf` / `.otf` / `.ttc` dosya yolu (`font_family`'yi ezer) |
| `font_weight` | `u16` | `400` | Hedef OS/2 ağırlık sınıfı (`1..=1000`; `400` Regular, `700` Bold) |
| `align` | `Option<TextAlign2D>` | `None` | Varsa `align_center` yerine geçer (`left \| center \| right`) |
| `x_offset_norm` | `Option<f32>` | `None` | Tuval genişliğine göre yatay anchor (`Left=0.04`, `Center=0.5`, `Right=0.96`) |
| `tracking_em` | `Option<f32>` | `None` | Karakterler arası ek boşluk (em cinsinden; `None` -> analitik yolda `0.05`, TTF'de `0.0`) |
| `line_height_em` | `Option<f32>` | `None` | Satır aralığı çarpanı (em cinsinden; `None` -> analitik yolda `1.3`, TTF'de font metriği) |
| `opacity` | `f32` | `1.0` | Katman opaklık çarpanı (`[0.0..=1.0]`, birleşik raster sonrası uygulanır) |

*(Çıkarım — öneri)*: Eski projelerin ve testlerin ergonomik yazımı için `NodeKind::vector_text_legacy(...)` yardımcı kurucusu veya `VectorText` struct literal yerlerinin yeni default'larla güncellenmesi gerekir; bilinen çağrı yerleri: [project.rs:928](../../crates/frameshift-schema/src/project.rs#L928), [schema_tests.rs:332](../../crates/frameshift-schema/tests/schema_tests.rs#L332), [compositor_tests.rs:826](../../crates/frameshift-render/tests/compositor_tests.rs#L826), [inspection.rs:648](../../crates/frameshift-core/src/inspection.rs#L648).

#### 1.2. Bağlanabilir Parametreler ve Doğrulama
- `crates/frameshift-schema/src/project.rs` (`bindable_param_names`):
  ```rust
  NodeKind::VectorText { .. } => &[
      "font_size_norm",
      "outline_width_norm",
      "y_offset_norm",
      "x_offset_norm",
      "tracking_em",
      "line_height_em",
      "opacity",
  ],
  ```
- `crates/frameshift-render/src/compositor.rs` (`apply_animated_parameter`): aynı 7 parametre adı güncellenir (`*x_offset_norm = Some(value)`, `*tracking_em = Some(value)`, `*line_height_em = Some(value)`, `*opacity = value.clamp(0.0, 1.0)`).
- `RenderGraphDef::validate_graph` (`crates/frameshift-schema/src/graph.rs`):
  - `font_size_norm`, `outline_width_norm`, `y_offset_norm`, `opacity`, ve varsa `x_offset_norm`, `tracking_em`, `line_height_em` sonlu (`is_finite()`) olmalıdır; `font_size_norm < 0.0` veya `outline_width_norm < 0.0` veya `opacity < 0.0` reddedilir.
  - `font_weight == 0 || font_weight > 1000` reddedilir.
  - `font_path` `Some(p)` ise `p.trim().is_empty()` reddedilir; `font_family` `Some(f)` ise `f.trim().is_empty()` reddedilir (`SchemaError::InvalidVectorText`).

---

### 2. M1 Golden Hash Koruması ve Tek Çekirdek Rasterizer Birleştirmesi

1. **Analitik SDF Koruma Koşulu:** `font_family.is_none() && font_path.is_none()` ise motor `frameshift-vector/src/text.rs` içindeki deterministik 8×12 `GlyphSdf` yolunu çalıştırır.
   - `golden_scene_preset()` için: `align: None`, `align_center: true`, `x_offset_norm: None`, `tracking_em: None` (`0.05`), `line_height_em: None` (`1.3`), `opacity: 1.0` değerleri mevcut `VectorTextConfig::default()` ile birebir aynı çıktıyı üretir.
   - **Bit-exact `opacity = 1.0` kuralı:** `(opacity - 1.0).abs() == 0.0` iken piksel buffer'ı üzerinde ekstra kayan nokta çarpımı yapılmaz; böylece IEEE 754 bit düzeyinde M1/M2 hash korunur.
   - **Analitik Yolda `TextAlign2D::Right` ve Çok Satır:** [text.rs:657-661](../../crates/frameshift-vector/src/text.rs#L657-L661) zaten `\n` satır bölme içerir; `align: Option<TextAlign2D>` eklenip `Left` (`anchor_x`), `Center` (`anchor_x - line_width_px * 0.5`), `Right` (`anchor_x - line_width_px`, varsayılan `x_offset_norm = 0.96`) olarak genelleştirilir. Tek satır `Center`/`Left` davranışı hiç değişmez.
2. **Çift Rasterizer Tutarsızlığının Kaldırılması (`compositor.rs` + `vector_text.rs`):**
   - *(Öneri)*: [compositor.rs:1757-1770](../../crates/frameshift-render/src/compositor.rs#L1757-L1770) içindeki animasyonlu/standalone fallback doğrudan `frameshift_vector::render_vector_text_rgba16f` çağrısına bağlanmalıdır (`crates/frameshift-render/Cargo.toml:12` zaten `frameshift-vector`'a bağımlıdır). `crates/frameshift-render/src/vector_text.rs` geriye dönük `pub use` veya ince delegasyon sarmalayıcısına indirgenebilir. Böylece:
     - Statik `VectorText` (`ProjectFrameProvider`) ile dinamik/animasyonlu `VectorText` (compositor fallback) **aynı font, aynı layout, aynı metrik ve aynı TTF/OTF motorunu** kullanır.
     - `compositor.rs`'deki `Err(_)` hata yutma hatası düzeltilir: yalnızca `FrameProvider` override'ın mevcut olmadığını belirten durum (örn. `RenderError::FrameProvider { .. }` "not overridden / dynamic fallback") dahili `frameshift_vector` rasterizer'ına geçer; `frameshift_vector` gerçek font yolunda `VectorError::FontNotFound` veya `VectorError::InvalidFont` dönerse bu hata **`RenderError::Vector(..)` olarak yukarı iletilir**.

---

### 3. Gerçek Font Çözümleme ve `FontNotFound` Sözleşmesi (`frameshift-vector/src/font_locator.rs`)

Hem `font_path` hem `font_family` için sessiz rastgele font fallback'i **kesinlikle yasaktır** ([render-graph.md §Kenar Durumlar](../specs/render-graph.md#L110)).

#### 3.1. Çözümleme Önceliği ve Yüz Seçimi
1. **`font_path: Some(raw_path)` (en yüksek öncelik):**
   - Yol çözümlenir (mutlaksa doğrudan; göreliyse varsa `project_dir.join(raw_path)` -> yoksa `Path::new(raw_path)`).
   - Dosya yoksa `Err(VectorError::FontNotFound(raw_path.to_owned()))`; okuma/parse hatasında `Err(VectorError::InvalidFont(..))`.
   - `ttf_parser::fonts_in_collection(&bytes)`:
     - `None` (tekil TTF/OTF): `face_index = 0`.
     - `Some(count)` (`.ttc`/`.otc` koleksiyon): `0..count` yüzleri parse edilir; `font_family` varsa aile+ağırlık skorlamasıyla en iyi indeks seçilir, yoksa `font_weight`'e en yakın normal yüz seçilir (eşitlikte en düşük `face_index`).

2. **`font_family: Some(family)` ve `font_path: None`:**
   - **Açık Arama Dizinleri (deterministik sıralı):**
     1. `FRAMESHIFT_FONT_DIRS` ortam değişkenindeki dizinler (`std::env::split_paths`),
     2. Varsa proje dizini altı (`<project_dir>/fonts`, `<project_dir>/assets/fonts`) ve `./fonts`,
     3. Platform standart sistem dizinleri:
        - **Windows (`#[cfg(target_os = "windows")]`):** `%WINDIR%\Fonts` (yoksa `C:\Windows\Fonts`) ve `%LOCALAPPDATA%\Microsoft\Windows\Fonts`,
        - **macOS (`#[cfg(target_os = "macos")]`):** `/System/Library/Fonts`, `/Library/Fonts`, `$HOME/Library/Fonts`,
        - **Linux/Unix (`#[cfg(all(unix, not(target_os = "macos")))]`):** `/usr/share/fonts`, `/usr/local/share/fonts`, `$HOME/.local/share/fonts`, `$HOME/.fonts`.
   - **İndeksleme ve Eşleme Kuralları (`SystemFontIndex`):**
     - Yukarıdaki dizinler özyinelemeli taranır; yalnızca `.ttf`, `.otf`, `.ttc`, `.otc` (case-insensitive) uzantılı dosyalar alınır ve yollar deterministik sıralanır (`BTreeSet` / `sort()`).
     - Her dosya yüzü (`0..face_count`) için `Face::parse(&bytes, idx)` çalıştırılır ve `face.names()` üzerinden Unicode (`name.is_unicode()`, `name.to_string()`) kayıtlarından `name_id::TYPOGRAPHIC_FAMILY` (`16`), `name_id::WWS_FAMILY` (`21`) ve `name_id::FAMILY` (`1`) değerleri normalize edilerek (`trim().to_ascii_lowercase()` + Unicode lowercase) indekslenir.
     - **Aile Eşleşme Kuralı:** Yalnızca hedef `family` ile normalize tam eşleşme yapan yüzler aday havuzuna girer. *(İsteğe bağlı olarak Windows/macOS'ta `"Arial Black"` gibi `FULL_NAME` veya `FAMILY="Arial"` + `SUBFAMILY="Black"` durumları için `name_id::FULL_NAME` (`4`) tam eşleşmesi de aynı aile içinde kabul edilebilir; ancak **farklı bir aile adına asla düşülmez**.)*
     - **Ağırlık ve Stil Skorlaması:**
       $$\text{score} = |w_{\text{face}} - w_{\text{target}}| \cdot 10 + p_{\text{style}} + p_{\text{name\_tier}}$$
       burada $w_{\text{face}} = \text{face.weight().to\_number()}$, $p_{\text{style}} = 0$ (`Style::Normal` için) / $5$ (`Italic`/`Oblique` için), $p_{\text{name\_tier}} = 0$ (`TYPOGRAPHIC_FAMILY` eşleşmesi) / $1$ (`FAMILY` eşleşmesi). Eşit skorda alfabetik `(path, face_index)` deterministik tie-breaker seçilir.
     - **Aday Havuzu Boşsa:** Doğrudan `Err(VectorError::FontNotFound(format!("family '{family}' (weight {font_weight})")))` döndürülür.
   - **Önbellek (`OnceLock<SystemFontIndex>` + `FontBytesCache`):** Dizin taraması ve header indekslemesi süreç boyunca `FontRegistry` içinde önbelleklenir (opsiyonel `refresh()` ile yeni kurulan fontlar taranabilir); yalnızca seçilen font dosyasının byte'ları `Arc<Vec<u8>>` olarak bellekte tutulur.

---

### 4. `ttf_parser::Face` Metrikleri, Çok Satır ve Hizalama Yerleşim Matematiği

#### 4.1. Ölçek ve Dikey Metrikler
- `units_per_em = face.units_per_em() as f32` (`0` olamaz; `ttf-parser` `16..=16384` garantiler, yine de `== 0` kontrolü `InvalidFont` döner).
- Çözünürlük-bağımsız piksel ölçeği (`draft` vs `final` orantısal eşitlik):
  $$\text{font\_px} = \text{font\_size\_norm} \cdot H, \qquad \text{scale} = \frac{\text{font\_px}}{\text{units\_per\_em}}$$
- Font dikey metrikleri (piksel cinsinden):
  - $\text{asc\_px} = \text{face.ascender()} \cdot \text{scale}$ (pozitif)
  - $\text{desc\_px} = \text{face.descender()} \cdot \text{scale}$ (tipik negatif)
  - $\text{gap\_px} = \text{face.line\_gap().max}(0) \cdot \text{scale}$
  - $\text{cap\_px} = \text{face.capital\_height().filter}(|&c| c > 0).\text{map}(|c| c \cdot \text{scale}).\text{unwrap\_or}(\text{asc\_px} \cdot 0.72)$
- Doğal satır adımı (`line_step_px`):
  - `line_height_em` `Some(lh)` ise: $\text{line\_step\_px} = \text{lh.max}(0.25) \cdot \text{font\_px}$
  - `None` ise (doğal font satır yüksekliği):
    $$\text{natural\_lh\_em} = \frac{(\text{face.ascender() as i32} - \text{face.descender() as i32} + \text{face.line\_gap().max(0) as i32}) \text{ as f32}}{\text{units\_per\_em}}$$
    $$\text{line\_step\_px} = \text{natural\_lh\_em.max}(1.0) \cdot \text{font\_px}$$

#### 4.2. Çok Satır Bölme ve Dikey Merkezleme (`y_offset_norm`)
- Metin `\r\n` -> `\n` normalize edilerek `lines: Vec<&str>` elde edilir (`split('\n')`; boş satırlar `""` satır yüksekliği adımı tüketir, glif üretmez).
- $N = \text{lines.len().max}(1)$ satırlık bloğun görsel dikey merkezi `y_offset_norm * H` olacak şekilde ilk satırın baseline konumu:
  $$\text{block\_center\_y} = \text{y\_offset\_norm} \cdot H$$
  $$\text{baseline}_0 = \text{block\_center\_y} - \frac{N - 1}{2} \cdot \text{line\_step\_px} + \frac{\text{cap\_px}}{2}$$
  $$\text{baseline}_i = \text{baseline}_0 + i \cdot \text{line\_step\_px}$$
  *Doğrulama:* Tek satırda ($N=1$) $\text{baseline}_0 = \text{block\_center\_y} + \text{cap\_px}/2$, yani büyük harflerin orta noktası tam `y_offset_norm * H` üzerinde oturur (`frameshift-vector/src/text.rs:603` ile kavramsal olarak aynı merkezleme).

#### 4.3. Satır İçi Shaping (`glyph_hor_advance`, `kern`, `tracking_em`) ve Yatay Hizalama
Her satır için:
1. `tracking_px = tracking_em.unwrap_or(0.0) * font_px`.
2. Karakterler üzerinden `pen_x = 0.0` ile ilerlenir:
   - `gid = face.glyph_index(ch).unwrap_or(GlyphId(0))`
   - `prev_gid` varsa `kern` tablosundan `kerning_between(&face, prev_gid, gid)` ([mesh3d.rs:368-382](../../crates/frameshift-vector/src/mesh3d.rs#L368-L382) ile birebir aynı; `subtable.horizontal && !subtable.variable`) okunur ve `pen_x += kern_units as f32 * scale`.
   - Glif yerleşim X'i `glyph_x = pen_x` olarak kaydedilir.
   - `adv_px = face.glyph_hor_advance(gid).unwrap_or(0) as f32 * scale`.
   - `pen_x += adv_px + if not_last_char { tracking_px } else { 0.0 }`.
3. Satır genişliği `line_width_px = pen_x.max(0.0)` (trailing tracking zaten eklenmedi; boşluklar advance'lerini korur).
4. Yatay anchor ve satır başlangıcı:
   - `effective_align = align.unwrap_or(if align_center { TextAlign2D::Center } else { TextAlign2D::Left })`
   - `anchor_x = x_offset_norm.unwrap_or(match effective_align { Left => 0.04, Center => 0.5, Right => 0.96 }) * (width as f32)`
   - `line_start_x`:
     - `Left` $\rightarrow$ `anchor_x`
     - `Center` $\rightarrow$ `anchor_x - line_width_px * 0.5`
     - `Right` $\rightarrow$ `anchor_x - line_width_px`
5. Outline dönüşümü (`OutlineBuilder` -> 2D tuval koordinatı):
   $$(x_{\text{canvas}}, y_{\text{canvas}}) = \bigl(\text{line\_start\_x} + \text{glyph\_x} + x_{\text{font}} \cdot \text{scale},\; \text{baseline}_i - y_{\text{font}} \cdot \text{scale}\bigr)$$

---

### 5. 2D Rasterizasyon: Yöntem Seçimi, Dolgu + Kontur ve `Rgba16fFrame` Üretimi

`MediaResolver::render_vector_text_node` GPU bağlamı olmadan (`frameshift-cli` / unit testler) CPU'da `Rgba16fFrame` döndürebilmelidir. Mevcut bağımlılıklarla iki saf-Rust 2D rasterizasyon yaklaşımı değerlendirildi:

| Kriter | Seçenek A: `lyon_tessellation` (`FillTessellator` + `StrokeTessellator`) -> CPU Üçgen Rasterizer | Seçenek B (Önerilen): `OutlineBuilder` / `lyon::path` Flattening -> Multi-Contour `NonZero` 4×4 SSAA Scanline + Analitik Kontur Mesafesi |
| :--- | :--- | :--- |
| **İç Delik (`O`, `B`, `A`) Doğruluğu** | `FillRule::NonZero` üçgenlerle çalışır | Tüm alt-konturlar ortak scanline winding sayacında toplanır; delikler `NonZero` ile tam kesilir |
| **Bitişik Üçgen Kenar Çatlağı (Seam)** | Ayrı üçgenler alpha-blend edilirse ince çatlak oluşur; piksel başı `u16` SSAA bitmask şarttır | Scanline doğrudan alt-kontur kenarlarıyla çalıştığı için iç üçgen kenarı yoktur, seam sıfırdır |
| **Kontur (`outline_width_norm`) Kalitesi** | `StrokeTessellator` kalın stroke'larda kendini kesen bölgelerde bitmask olmadan alpha doubling yapar; ince köşelerde `LineJoin::Round` avantajlıdır | Düzleştirilmiş kontur segmentlerine Öklid mesafe + `sdf_coverage(dist - outline_px)` ile yumuşak 1px AA, sıfır self-intersection artefaktı; geniş/keskin köşelerde rondeli |
| **Glif Tile Cache Uygunluğu** | Her glif için mesh cache gerekir | Glif başına lokal `(fill_cov, outline_cov)` alpha tile önbelleği (`f32`/`u8` değil `f32`) kurulabilir; animasyonda çok hızlıdır |
| **Kod Yeniden Kullanımı** | `mesh3d.rs`'deki `LyonGlyphBuilder` paylaşılır | [canvas.rs:460-538](../../crates/frameshift-vector/src/canvas.rs#L460-L538) `fill_polygon` 4×4 SSAA scanline + `sd_segment` + `sdf_coverage` altyapısı zaten mevcut |

**Önerilen Hibrit Tasarım (Seçenek B + Opsiyonel `StrokeTessellator` Alternatifi):**
1. **Düzleştirme (`Flattening`):** `LyonGlyphBuilder` (Y-flipped: `baseline_y - y * scale`) ile `ttf_parser::Face::outline_glyph(gid, &mut builder)` üzerinden `lyon_tessellation::path::Path` oluşturulur ve `path.iter().flattened(tolerance)` (`tolerance = (0.12_f32).min(font_px * 0.005).max(0.02)`) ile kapalı alt-konturlar (`Vec<FlattenedContour>`) çıkarılır (`mesh3d.rs:384-445` ile aynı mantık).
2. **Çok-Konturlu `NonZero` 4×4 SSAA Scanline (Dolgu):**
   - `VectorCanvas::fill_polygon` şu an tek poligon alır; bunu `fill_contours_nonzero(contours: &[FlattenedContour])` şeklinde genelleştirerek bir glifin tüm dış + iç delik konturlarını aynı `intersections` listesinde biriktirmek gerekir.
   - Her pikselde 4×4 alt-örnek (`SUBPIXEL_OFFSETS = [0.125, 0.375, 0.625, 0.875]`) için soldan sağa `winding != 0` olan aralıklar sayılır (`fill_cov = hits / 16.0`).
   - *(Opsiyonel kalite artışı)*: Küçük puntolarda (`font_px < 28.0`) 8×8 (`64` örnek) veya analitik kenar mesafesi, büyük puntolarda 4×4 (`16` örnek) kullanılabilir.
3. **Kontur (`outline_width_norm > 0`):**
   - Glifin dolgu dışı piksellerinde (`fill_cov < 1.0`, bounding box + `outline_px + 1.5` pad) düzleştirilmiş tüm alt-kontur segmentlerine minimum Öklid mesafe `min_dist` hesaplanır; glif içindeyse (veya `fill_cov >= 1.0`) `outline_cov = 1.0`, dışındaysa `outline_cov = sdf_coverage(min_dist - outline_px)` (`(0.5 - (min_dist - outline_px)).clamp(0.0, 1.0)`).
   - **Alternatif (`StrokeTessellator`):** Eğer `LineJoin::Miter` veya aşırı keskin anime başlık köşeleri istenirse `StrokeTessellator::tessellate_path` + `StrokeOptions::tolerance(tol).with_line_width(outline_px * 2.0).with_line_join(LineJoin::Round)` ile üretilen üçgenler piksel başına `u16` (`4x4`) bitmask tamponuna `|=` ile yazılıp (`hits.count_ones() / 16.0`) self-overlap çift sayımı engellenebilir.
4. **Glif Tile Cache + Premultiplied `LinearRec709` Birleştirme:**
   - Birden çok karakterin konturu veya dar tracking ile gövdeleri birbirine değebileceğinden, tüm gliflerin **birleşik** `fill_cov_buf: Vec<f32>` ve `outline_cov_buf: Vec<f32>` (metin bounding box'ı boyutunda, piksel başına `max` veya Porter-Duff over) tamponları hazırlanır — *önemli*: her glif için `fill` arka plandaki bir önceki glifin `outline`'ının üstünde kalabilmelidir; dolayısıyla en temiz katmanlama:
     - Önce tüm gliflerin **kontur** katmanı `outline_alpha = max(outline_alpha, g_outline_cov)` (veya sırayla composited) olarak biriktirilir,
     - Ardından tüm gliflerin **dolgu** katmanı `fill_alpha = max(fill_alpha, g_fill_cov)` (ya da Porter-Duff over ile her glif kendi sırasıyla: `acc_outline` üzerine tüm gliflerin `acc_fill`'i `SourceOver`!) şeklinde biriktirilir. *Neden?* İki harf birbirine çok yakın veya negatif `tracking_em` ile bitişik olduğunda, 2. harfin konturunun 1. harfin gövdesini ezmemesi için tüm harflerin kontur katmanı önce, tüm harflerin dolgu katmanı üstte birleştirilir!
   - Renk ve `opacity` birleştirme (`LinearRec709` premultiplied):
     - `fill_premul = premultiply_rgba(color_rgba)`, `outline_premul = premultiply_rgba(outline_rgba)`
     - `src = source_over_premul(scale_premul(outline_premul, out_cov), scale_premul(fill_premul, fill_cov))`
     - `op = opacity.clamp(0.0, 1.0)` -> `final_px = [src[0] * op, src[1] * op, src[2] * op, src[3] * op]`
     - `Rgba16fFrame::new(width, height, pts, AlphaMode::Premultiplied, pixels)`.

---

### 6. `resolver.rs` & `compositor.rs` Önbellek Anahtarı ve Pipeline Entegrasyonu

#### 6.1. Bit-Exact `VectorTextCacheKey` (`frameshift-core/src/resolver.rs`)
`vector_text_cache` anahtarı formatlı string (`{:.4}`) yerine tam yapısal hash anahtarına geçirilir:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VectorTextCacheKey {
    pub width: u32,
    pub height: u32,
    pub text: String,
    pub font_family: Option<String>,
    pub font_path: Option<String>,
    pub font_weight: u16,
    pub font_size_bits: u32,
    pub color_bits: [u32; 4],
    pub outline_bits: [u32; 4],
    pub outline_width_bits: u32,
    pub align: TextAlign2D, // effective_align
    pub x_offset_bits: Option<u32>,
    pub y_offset_bits: u32,
    pub tracking_bits: Option<u32>,
    pub line_height_bits: Option<u32>,
    pub opacity_bits: u32,
}
```

- `MediaResolver::render_vector_text_node` (ve `frameshift_vector::render_vector_text_node_rgba16f`) tüm `NodeKind::VectorText` alanlarını `VectorTextConfig`'e aktarır.
- Diskteki bir `font_path` değişirse kullanıcıya `MediaResolver::clear_probe_cache()` (veya `font_bytes_len` / `mtime` parmak izi) temizleme imkânı verilir.

#### 6.2. `GpuCompositor` Entegrasyonu ve Hata Semantiği (`crates/frameshift-render/src/compositor.rs`)
1. `GpuCompositor` halihazırda `font_cache: HashMap<String, Arc<Vec<u8>>>` ([compositor.rs:926](../../crates/frameshift-render/src/compositor.rs#L926)) taşır. `frameshift_vector::FontResolver` / `SystemFontIndex` + paylaşımlı byte cache ile entegre edildiğinde animasyonlu `VectorText` düğümlerinde her alt-karede (`MotionBlurShutter` `sub_samples=8` dahil!) disk I/O tekrarlanmaz.
2. `NodeKind::VectorText` değerlendirmesi ([compositor.rs:1747-1780](../../crates/frameshift-render/src/compositor.rs#L1747-L1780)):
   - `provider.provide_frame(&override_key, sub_tick, width, height)` çağrılır:
     - `Ok(frame)` -> dış `FrameProvider` (ör. `ProjectFrameProvider` cache hit veya özel test provider) sonucunu doğrudan GPU'ya yükle.
     - `Err(RenderError::FrameProvider { .. })` -> yalnızca `override_key` sağlanmadı / dinamik bağlama nedeniyle dahili rasterizer istendi durumudur: `frameshift_vector` üzerinden `VectorTextConfig::from_node_kind(&node_kind)` ile rasterize et; buradan dönen `VectorError::FontNotFound` / `InvalidFont` hatasını **`?` ile `RenderError::Vector` olarak döndür** (asla yutma!).
     - Diğer `Err(other)` -> doğrudan `?` ile yukarı döndür (ör. `ProjectFrameProvider` statik bir düğümde `FontNotFound` ürettiyse `compositor` bunu sessizce bastırmaz).
   - **Animasyonlu `VectorText` Performans İyileştirmesi (Opsiyonel Aşama 2):** `GpuCompositor` içinde küçük bir LRU/son-kare `vector_text_tex_cache: HashMap<VectorTextCacheKey, SourceFrame>` tutulursa `MotionBlurShutter` altında metin parametreleri o alt-zamanda değişmediğinde bile yeniden rasterizasyon önlenir.

---

### 7. Uygulama Sonrası Doğrulama Planı

Kodlama fazına geçildiğinde aşağıdaki test ve kapıların eklenmesi/çalıştırılması önerilir:
1. **Geriye Uyumluluk & Şema Round-Trip:** Eski 7 alanlı JSON (`0.1.0`/`0.2.0` formatı) yeni şemaya hatasız yüklenir; `bindable_param_names` ve `validate_graph` yeni alanları doğrular.
2. **M1/M2 Golden Hash Koruma Testi:** `font_family: None && font_path: None` olan `Project::golden_scene_preset()` çıktısının `frameshift-vector` hash'i ve `verify-all` M1–M5 kapısı birebir geçer.
3. **Sessiz Fallback Yasağı (`FontNotFound`):** Olmayan `font_path` (`"missing.ttf"`) ve olmayan `font_family` (`"DefinitelyNonExistentFontXYZ"`) verilen projede hem `MediaResolver::render_vector_text_node` hem `GpuCompositor::render_frame` (animasyonlu `ParamBindingDef` olsa bile!) `FontNotFound` hatası döndürür.
4. **Gerçek TTF/OTF Çok Satır + Hizalama + Kerning Testi:** Sistemdeki gerçek bir `.ttf`/`.ttc` fontu (veya test fixture TTF'i) ile `\n` çok satır, `Left | Center | Right` hizalama bbox karşılaştırması, `tracking_em`, `line_height_em`, `outline_width_norm`, `opacity = 0.5` premultiplied alfa sızıntısızlığı (`FullResCrop` / kenar pikselleri) ve `draft` (`0.5x`) vs `final` (`1.0x`) çözünürlük değişmezliği test edilir.

---

## Doğrulanmamış Noktalar

1. **Kompleks Metin Şekillendirme (HarfBuzz / BiDi / Ligatür):** `ttf-parser` tek başına `cmap` + `kern` tablosu üzerinden 1:1 karakter-glif eşlemesi yapar; OpenType `GSUB` ligatürleri, sağdan sola (Arapça/İbranice BiDi) veya Devanagari/Tayca kompleks küme şekillendirme (shaping) sağlamaz (`rustybuzz` olmadan bu diller kapsam dışıdır).
2. **`GPOS` Tablosu Kerning'i vs Klasik `kern` Tablosu:** Modern bazı OpenType/CFF fontları (`OTF`) kerning çiftlerini klasik `kern` tablosu yerine `GPOS` PairPos alt tablolarında taşır (`mesh3d.rs:368` şu an yalnız `face.tables().kern` okur). `ttf-parser` `GPOS` sınıf-tabanlı PairPos çözümlemesi kod aşamasında ekstra dikkat veya opsiyonel `GPOS` desteği gerektirip gerektirmediği hedef fontlarla test edilmelidir.
3. **Variable Font (`fvar`) Örnekleri:** `ttf-parser 0.25` varsayılan derleme özelliklerinde `variable-fonts` feature bayrağı kapalıdır (`Face::is_variable()` daima `false` döner); değişken fontlarda ağırlık (`wght` ekseni) seçimi için `ttf-parser` feature flag'i açılmadığı sürece yalnızca statik/ayrı ağırlık dosyaları seçilir.
4. **Sistem Font Dizin Tarama Maliyeti:** Windows `C:\Windows\Fonts` yüzlerce `.ttf`/`.ttc` dosyası içerir; başlangıç taramasının yalnızca header/name/OS2 parse ederek (`Face::parse` zero-allocation olduğundan) kaç milisaniye sürdüğü implementasyon sonrası benchmark ile doğrulanmalıdır.
