---
title: "Harici .cube LUT (1D + 3D + Shaper) ve Saf Rust OpenColorIO (OCIO) Renk Hattı Şartnamesi"
date: 2026-11-22
status: draft
tags: [color, lut3d, cube-lut, ocio, aces, transfer-functions, rg16f, schema, wgpu]
---

# Harici `.cube` LUT (1D + 3D + Shaper) ve Saf Rust OpenColorIO (OCIO) Renk Hattı Şartnamesi

## Soru / Amaç

FrameShift'in mevcut `Lut3D` altyapısı ([`lut.rs`](../../crates/frameshift-render/src/lut.rs), [`lut3d.wgsl`](../../crates/frameshift-render/shaders/lut3d.wgsl) ve [`NodeKind::Lut3D`](../../crates/frameshift-schema/src/graph.rs#L280-L285)) yalnızca 4 yerleşik analitik preset'i ve tek bloklu `LUT_3D_SIZE` `.cube` dosyalarını desteklemekte; `LUT_1D_SIZE`, birleşik 1D shaper + 3D LUT `.cube` dosyaları, `.spi1d`/`.spi3d` tabloları ve [project-schema.md §5](../specs/project-schema.md#L113-L115) içinde kilitlenmiş olan `lut_source: cube_file | ocio` OpenColorIO sözleşmesi henüz uygulanmamıştır.

Ayrıca mevcut çalışma ortamında `PyOpenColorIO`, `ociobakelut`, `clang` ve `vcpkg` bulunmamakta; [AGENTS.md](../../AGENTS.md) gereği kullanıcı onayı olmadan yeni harici C++ SDK veya Cargo crate'i eklenememektedir.

Bu araştırma şu kesin teknik soruyu yanıtlar:
> **Sıfır yeni Cargo crate'i ve sıfır zorunlu C++ SDK bağımlılığı ile**, mevcut `Rgba16Float` (Linear Rec.709, premultiplied alpha) `wgpu` kompozitörüne tam geriye uyumlu olarak:
> 1. Adobe/Iridas `.cube` 1D, 3D ve birleşik 1D Shaper + 3D LUT desteği,
> 2. `.ocio` YAML/text konfigürasyonunu çözümleyip `src_space -> dst_space` dönüşümünü 3D LUT'a bake eden saf Rust `OcioColorPipeline` (+ sistemde varsa opsiyonel `ociobakelut` CLI köprüsü),
> 3. Yerleşik ACES2065-1 / ACEScg / ACEScct / LinearRec709 / DisplaySrgb / Rec709 / Rec2020 / ARRI LogC3 / Sony S-Log3 analitik dönüşümleri ve
> 4. Mevcut `lut_preset` ve `input` alanlarını bozmadan `NodeKind::Lut3D` şema genişlemesi
> nasıl tasarlanmalıdır?

---

## İncelenen Birincil Kaynaklar

### Harici Birincil Kaynaklar (İnceleme Tarihi: `2026-11-22`)

1. **Adobe — *Cube LUT Specification Version 1.0* (IRIDAS / Adobe SpeedGrade, Eylül 2013):**
   - [http://wwwimages.adobe.com/content/dam/Adobe/en/products/speedgrade/cc/pdfs/cube-lut-specification-1.0.pdf](http://wwwimages.adobe.com/content/dam/Adobe/en/products/speedgrade/cc/pdfs/cube-lut-specification-1.0.pdf) — inceleme tarihi: `2026-11-22` (`TITLE`, `LUT_1D_SIZE`, `LUT_3D_SIZE`, `DOMAIN_MIN`, `DOMAIN_MAX`, yorum satırı `#`, 3D eksen sırası R-en-hızlı / G-orta / B-en-yavaş kuralları; birleşik 1D+3D shaper uzantısı için OCIO/Resolve referansı).
2. **OpenColorIO (ASWF) — *Configuration File Format & Transform Syntax (YAML v1/v2)*:**
   - [https://opencolorio.readthedocs.io/en/latest/guides/authoring/overview.html](https://opencolorio.readthedocs.io/en/latest/guides/authoring/overview.html) — inceleme tarihi: `2026-11-22` (`ocio_profile_version`, `search_path`, `roles`, `colorspaces`, `to_reference` / `from_reference` / `to_scene_reference` / `from_scene_reference`).
   - [https://opencolorio.readthedocs.io/en/latest/api/transforms.html](https://opencolorio.readthedocs.io/en/latest/api/transforms.html) — inceleme tarihi: `2026-11-22` (`MatrixTransform`, `FileTransform`, `ExponentTransform`, `LogTransform`, `GroupTransform` semantiği ve `direction: forward | inverse`).
3. **OpenColorIO (ASWF) — *`ociobakelut` CLI & `.spi1d` / `.spi3d` LUT Biçimleri:**
   - [https://opencolorio.readthedocs.io/en/latest/guides/using_ocio/tool_overview.html](https://opencolorio.readthedocs.io/en/latest/guides/using_ocio/tool_overview.html) — inceleme tarihi: `2026-11-22` (`ociobakelut --iconfig --inputspace --outputspace --shaperspace --cubesize --format --stdout`).
   - [https://github.com/AcademySoftwareFoundation/OpenColorIO/blob/main/src/OpenColorIO/fileformats/FileFormatSpi1D.cpp](https://github.com/AcademySoftwareFoundation/OpenColorIO/blob/main/src/OpenColorIO/fileformats/FileFormatSpi1D.cpp) & `FileFormatSpi3D.cpp` — inceleme tarihi: `2026-11-22` (`.spi1d` `Version 1 / From / To / Length / Components / { ... }` ve `.spi3d` `SPILUT 1.0 / 3 / nx ny nz / i j k r g b` düzeni).
4. **Academy Color Encoding System (ACES) — S-2014-004 (ACEScg / AP1), S-2016-001 (ACEScct), AP0↔AP1 ve Bradford D60↔D65:**
   - [https://docs.acescentral.com/](https://docs.acescentral.com/) ve [https://github.com/ampas/aces-core](https://github.com/ampas/aces-core) (`ACESlib.Utilities_Color.ctl` / `AP0_2_AP1_MAT` / `D60_2_D65_CAT`) — inceleme tarihi: `2026-11-22` (AP0, AP1, D60 `(0.32168, 0.33767)`, D65 `(0.3127, 0.3290)` ve ACEScct `X_break = 0.0078125`, `A = 10.54023774`, `B = 0.07290553`, `17.52`, `9.72` sabitleri).
5. **Colour Science / Chromatic Adaptation (Bradford Cone Response Matrix):**
   - [https://www.colour-science.org/](https://www.colour-science.org/) — inceleme tarihi: `2026-11-22` (Bradford `M_BFD` matrisi ve `AP1_to_XYZ` / `XYZ_to_AP1`).
6. **IEC 61966-2-1 (sRGB), ITU-R BT.709 ve ITU-R BT.2020:**
   - [https://www.color.org/chardata/rgb/srgb.xalter](https://www.color.org/chardata/rgb/srgb.xalter) & ITU-R BT.709 / BT.2020 parametreleri — inceleme tarihi: `2026-11-22` (`0.04045`, `12.92`, `0.055`, `2.4`; BT.709 `0.018`, `4.5`, `0.099`, `0.45`; BT.2020 primerleri `(0.708, 0.292), (0.170, 0.797), (0.131, 0.046)` D65).
7. **ARRI — *ALEXA Log C Curve – Usage in VFX* (LogC3 EI 800 & ARRI Wide Gamut 3):**
   - [https://www.arri.com/en/learn-help/learn-help-camera-system/image-science/log-c](https://www.arri.com/en/learn-help/learn-help-camera-system/image-science/log-c) — inceleme tarihi: `2026-11-22` (`cut = 0.010591`, `a = 5.555556`, `b = 0.052272`, `c = 0.247190`, `d = 0.385537`, `e = 5.367655`, `f = 0.092809` ve AWG3→XYZ matrisi).
8. **Sony — *Technical Summary for S-Gamut3.Cine/S-Log3 and S-Gamut3/S-Log3*:**
   - [https://pro.sony/ue_US/support-resources/software/00263050](https://pro.sony/ue_US/support-resources/software/00263050) — inceleme tarihi: `2026-11-22` (`0.01125000`, `420.0`, `261.5`, `95.0`, `171.2102946929`, `1023.0` ve S-Gamut3 primerleri `(0.730, 0.280), (0.140, 0.855), (0.100, -0.050)` D65).

### Yerel Kod Tabanı ve Sözleşme Kaynakları
- [`crates/frameshift-render/src/lut.rs`](../../crates/frameshift-render/src/lut.rs) (satır 1–383)
- [`crates/frameshift-render/shaders/lut3d.wgsl`](../../crates/frameshift-render/shaders/lut3d.wgsl) (satır 1–64)
- [`crates/frameshift-render/src/uniforms.rs`](../../crates/frameshift-render/src/uniforms.rs#L248-L278) (`Lut3DUniform`)
- [`crates/frameshift-render/src/compositor.rs`](../../crates/frameshift-render/src/compositor.rs#L457-L461) (`apply_animated_parameter`), [`L903-L909`](../../crates/frameshift-render/src/compositor.rs#L903-L909) (`CachedLut3D`), [`L1781-L1806`](../../crates/frameshift-render/src/compositor.rs#L1781-L1806) (`NodeKind::Lut3D`), [`L2977-L3048`](../../crates/frameshift-render/src/compositor.rs#L2977-L3048) (`ensure_lut3d_cached`)
- [`crates/frameshift-render/src/error.rs`](../../crates/frameshift-render/src/error.rs#L33-L34) (`RenderError::InvalidLut3D`)
- [`crates/frameshift-render/tests/compositor_tests.rs`](../../crates/frameshift-render/tests/compositor_tests.rs#L644-L686) (`cube_lut_parse_and_presets_validate_and_grade`)
- [`crates/frameshift-schema/src/graph.rs`](../../crates/frameshift-schema/src/graph.rs#L30-L36) (`ColorSpace`), [`L280-L285`](../../crates/frameshift-schema/src/graph.rs#L280-L285) (`NodeKind::Lut3D`), [`L1157-L1161`](../../crates/frameshift-schema/src/graph.rs#L1157-L1161) (`validate_graph`)
- [`crates/frameshift-schema/src/project.rs`](../../crates/frameshift-schema/src/project.rs#L218) (`bindable_param_names`), [`L950-L958`](../../crates/frameshift-schema/src/project.rs#L950-L958) (`golden_scene_preset`)
- [`crates/frameshift-render/Cargo.toml`](../../crates/frameshift-render/Cargo.toml) (`half = "2.7.1"`, `glam = "0.29"`, `wgpu = "30.0.1"`, `bytemuck = "1.25.2"`)
- Kilitli sözleşmeler: [project-schema.md §5](../specs/project-schema.md#L113-L115), [render-graph.md §1 & §3.2](../specs/render-graph.md#L25-L76)

---

## Doğrudan Gözlemler (Kanıtlı)

### 1. Mevcut Kod Tabanındaki Gerçek Durum ve Sınırlar

1. **`.cube` Ayrıştırıcı (`lut.rs:38-126`):**
   - `CubeLut3D::parse_cube` şu anda `LUT_1D_SIZE` anahtar kelimesini gördüğünde doğrudan `RenderError::InvalidLut3D("line N: LUT_1D_SIZE is not supported; expected LUT_3D_SIZE")` döndürmektedir ([`lut.rs:56-60`](../../crates/frameshift-render/src/lut.rs#L56-L60)). Mevcut test [`compositor_tests.rs:676-679`](../../crates/frameshift-render/tests/compositor_tests.rs#L676-L679) `"LUT_1D_SIZE 16\n"` (veri satırı olmadan) için `Err(RenderError::InvalidLut3D(_))` beklemektedir; dolayısıyla eksik satırlı `LUT_1D_SIZE 16\n` hata vermeye devam edecek, ancak geçerli 16 satırlı 1D LUT dosyaları başarılı şekilde ayrıştırılacaktır.
   - `LUT_3D_INPUT_RANGE`, `LUT_IN_VIDEO_RANGE`, `LUT_OUT_VIDEO_RANGE` anahtar kelimeleri şu anda sessizce yok sayılmaktadır ([`lut.rs:84-89`](../../crates/frameshift-render/src/lut.rs#L84-L89)); `LUT_1D_INPUT_RANGE` ve `DOMAIN_MIN` / `DOMAIN_MAX` değerlerinin 1D vs 3D kapsamı ayrıştırılmamıştır.
   - `CubeLut3D::resolve` ([`lut.rs:155-175`](../../crates/frameshift-render/src/lut.rs#L155-L175)) yalnızca `spec.contains("LUT_3D_SIZE")` kontrolüyle inline `.cube` algılar; saf 1D inline metinlerde başarısız olur.
   - `build_analytic` ([`lut.rs:248-252`](../../crates/frameshift-render/src/lut.rs#L248-L252)) her çıktıyı `.max(0.0)` ile klemplemektedir. Geniş gamut matris dönüşümlerinde (ör. ACEScg/AWG3 → LinearRec709 arası ara adımlar) küçük negatif değerler geçici olarak oluşabilir; yalnızca nihai display çıkışında veya bilinçli gamut mapping sonrası klemplenmelidir.
   - `ColorTransform for CubeLut3D` ([`lut.rs:267-283`](../../crates/frameshift-render/src/lut.rs#L267-L283)) giriş ve çıkış uzayını sabit `ColorSpace::LinearRec709` döndürür.

2. **GPU Shader ve Önbellek (`lut3d.wgsl`, `uniforms.rs`, `compositor.rs`):**
   - [`lut3d.wgsl:49`](../../crates/frameshift-render/shaders/lut3d.wgsl#L49) un-premultiply sırasında `max(src.rgb / src.a, vec3<f32>(0.0))` uygular. Eğer LUT `domain_min` negatif bir değer taşıyorsa (ör. geniş gamut veya offset'li log/ACEScct domain'i), `max(..., 0.0)` negatif kanalları LUT domain'ine girmeden keser; bunun yerine `domain_min` altındaki klempleme `norm_rgb = clamp((rgb_in - domain_min) * domain_inv_span, 0.0, 1.0)` satırına ([`lut3d.wgsl:52`](../../crates/frameshift-render/shaders/lut3d.wgsl#L52)) bırakılmalıdır.
   - `Lut3DUniform` ([`uniforms.rs:251-258`](../../crates/frameshift-render/src/uniforms.rs#L251-L258)) 48 byte'tır ve `intensity_size.zw`, `domain_min_pad.w`, `domain_inv_span_pad.w` alanları boştur (`reserved`).
   - `GpuCompositor::lut_cache` ([`compositor.rs:925`](../../crates/frameshift-render/src/compositor.rs#L925)) yalnızca `String` (`lut_preset`) ile anahtarlanır. Aynı `lut_preset` adı veya dosya yolu içeriği değiştiğinde ya da farklı `src_space` / `dst_space` / `lut_size` / `look` istendiğinde çakışma yaşanmaması için yapılandırılmış bir `LutCacheKey` gerekir.
   - `ensure_lut3d_cached` ([`compositor.rs:2987-3030`](../../crates/frameshift-render/src/compositor.rs#L2987-L3030)) 3D dokuyu `half::f16` olarak `WORKING_TEXTURE_FORMAT` (`Rgba16Float`) formatında `wgpu::TextureDimension::D3` dokusuna yazar. `f16` en büyük sonlu değer olarak `65504.0` ve negatif sayıları yerel olarak temsil edebilir.

3. **Şema Durumu (`graph.rs`, `project.rs`, `project-schema.md`):**
   - [`NodeKind::Lut3D`](../../crates/frameshift-schema/src/graph.rs#L280-L285) şu anda yalnızca `input: NodeId`, `lut_preset: String`, `intensity: f32` taşır; `project-schema.md §5 #10` içinde belirtilen `lut_source` (`cube_file | ocio`), `input_space`, `output_space` alanları henüz Rust struct'ına eklenmemiştir.
   - `NodeKind::Lut3D` içinde `input: NodeId` alanı tarihsel olarak hem `kind` içinde hem `GraphNodeDef.inputs` içinde yer alır; [`validate_graph`](../../crates/frameshift-schema/src/graph.rs#L1157-L1161) ikisini de doğrular ve [`golden_scene_preset`](../../crates/frameshift-schema/src/project.rs#L950-L958) bu şemayı kullanır. Bu yüzden `input` ve `lut_preset` kesinlikle korunmalıdır.
   - [`ColorSpace`](../../crates/frameshift-schema/src/graph.rs#L32-L36) yalnızca `LinearRec709` ve `DisplaySrgb` içerir.

4. **Bağımlılıklar (`Cargo.toml` & Sistem Araçları):**
   - `frameshift-render/Cargo.toml` zaten `glam = "0.29"` (3x3 ve 4x4 matrisler için `Mat3` / `Mat4` hazır) ve `half = "2.7.1"` içerir; `serde_yaml` veya `opencolorio` crate'i **yoktur**.
   - Sistem ortamında `PyOpenColorIO`, `ociobakelut`, `clang`, `vcpkg` mevcut değildir; ancak opsiyonel runtime CLI köprüsü `which("ociobakelut")` / `Command::new("ociobakelut")` üzerinden kurgulanabilir.

---

### 2. Doğrulanmış Matematiksel Sabitler ve Format Kuralları

#### A. Adobe Cube LUT 1.0 + Birleşik 1D Shaper + 3D Düzeni
- **Anahtar Kelimeler:** `TITLE "..."`, `LUT_1D_SIZE N` (`2 <= N <= 65536`), `LUT_3D_SIZE N` (`2 <= N <= 256`, FrameShift GPU tavanı `128`), `DOMAIN_MIN r g b` (varsayılan `0 0 0`), `DOMAIN_MAX r g b` (varsayılan `1 1 1`), ayrıca Resolve/OCIO uyumlu `LUT_1D_INPUT_RANGE min max` ve `LUT_3D_INPUT_RANGE min max`.
- **1D + 3D Birleşik Sıra:** Birleşik `.cube` dosyasında önce `LUT_1D_SIZE M` + `M` adet `R G B` satırı (shaper), ardından `LUT_3D_SIZE N` + `N^3` adet `R G B` satırı gelir. Değerlendirme sırası: $\text{RGB}_{\text{out}} = \text{LUT}_{3D}(\text{LUT}_{1D}(\text{RGB}_{\text{in}}))$.
- **3D İndeksleme:** `R` en hızlı, `G` orta, `B` en yavaş değişir (`index = r + g * N + b * N * N`).

#### B. OpenColorIO `.ocio` YAML Config ve `.spi1d` / `.spi3d` Biçimleri
- **`.ocio` YAML alanları:**
  - `ocio_profile_version: 1` veya `2`
  - `search_path: "luts"` (veya `:` / `;` ayrılmış çoklu klasör listesi)
  - `roles:` (ör. `scene_linear: ACEScg`, `default: sRGB`, `color_picking: sRGB`, `compositing_log: ACEScct`)
  - `colorspaces:` listesi (`- !<ColorSpace>`), her biri `name`, isteğe bağlı `aliases`, `isdata`, `to_reference` / `to_scene_reference` ve `from_reference` / `from_scene_reference` içerir.
- **OCIO Transform Tipleri (Satır/Blok veya Inline `{...}`):**
  - `!<MatrixTransform>`: 4x4 satır-büyük (`row-major`) `matrix: [16 f64]`, isteğe bağlı `offset: [4 f64]`, `direction: forward | inverse`.
  - `!<FileTransform>`: `src: <path>`, `interpolation: nearest | linear | tetrahedral | best`, `direction: forward | inverse`.
  - `!<ExponentTransform>`: `value: [r, g, b, a]`, `direction: forward | inverse`, isteğe bağlı `negative_style: clamp | mirror | pass_thru | linear`.
  - `!<LogTransform>`: `base: <f64>` (varsayılan `2.0`), `direction: forward | inverse` ($y = \log_{\text{base}}(\max(x, 10^{-10}))$, tersi $x = \text{base}^y$).
  - `!<GroupTransform>`: `children: [...]`, `direction: forward | inverse` (`inverse` iken çocuklar ters sırada ve ters yönde çalıştırılır).
- **`.spi1d`:**
  ```text
  Version 1
  From <min> <max>
  Length <N>
  Components <1|3>
  {
    <v0> ...
  }
  ```
- **`.spi3d`:**
  ```text
  SPILUT 1.0
  3
  <nr> <ng> <nb>
  <ri> <gi> <bi> <r_out> <g_out> <b_out>
  ...
  ```

#### C. Doğrulanmış Yerleşik Renk Uzayı Transferleri ve `f64` 3×3 Matrisler

1. **IEC 61966-2-1 sRGB (`DisplaySrgb` ↔ `LinearRec709`):**
   - EOTF (`sRGB → Linear`): $c \le 0.04045 \implies c / 12.92$; aksi halde $((c + 0.055)/1.055)^{2.4}$.
   - OETF (`Linear → sRGB`): $x \le 0.0031308 \implies 12.92 x$; aksi halde $1.055 x^{1/2.4} - 0.055$.
2. **ITU-R BT.709 (`Rec709` OETF ↔ LinearRec709):**
   - OETF (`Linear → BT.709`): $x < 0.018 \implies 4.5 x$; aksi halde $1.099 x^{0.45} - 0.099$.
   - Inverse OETF (`BT.709 → Linear`): $v < 0.081 \implies v / 4.5$; aksi halde $((v + 0.099)/1.099)^{1/0.45}$.
   - *(Not: Display izleme için BT.1886 $\gamma=2.4$ EOTF ayrımı `DisplayRec709_1886` olarak not edilmelidir.)*
3. **ACEScct (Academy S-2016-001, AP1 primerleri):**
   - `Linear AP1 → ACEScct`:
     - $X_{\text{break}} = 0.0078125$, $A = 10.54023774$, $B = 0.07290553$
     - $x \le X_{\text{break}} \implies A \cdot x + B$
     - $x > X_{\text{break}} \implies (\log_2(x) + 9.72) / 17.52$
   - `ACEScct → Linear AP1`:
     - $Y_{\text{break}} = A \cdot X_{\text{break}} + B = 0.155251141552511$
     - $y \le Y_{\text{break}} \implies (y - B) / A$
     - $y > Y_{\text{break}} \implies 2^{17.52 y - 9.72}$
4. **ARRI LogC3 EI 800 (ALEXA Log C Curve – Usage in VFX):**
   - `cut = 0.010591`, `a = 5.555556`, `b = 0.052272`, `c = 0.247190`, `d = 0.385537`, `e = 5.367655`, `f = 0.092809`.
   - Forward (`Linear AWG3 → LogC3`): $x > \text{cut} \implies c \cdot \log_{10}(a x + b) + d$; aksi halde $e x + f$.
   - Inverse (`LogC3 → Linear AWG3`, $\text{cut}_y = e \cdot \text{cut} + f = 0.1496579...$): $y > \text{cut}_y \implies (10^{(y - d)/c} - b)/a$; aksi halde $(y - f)/e$.
5. **Sony S-Log3 (10-bit 0..1023 normalize `[0, 1]`):**
   - Forward (`Linear S-Gamut3 → S-Log3`):
     - $x \ge 0.01125 \implies (420.0 + \log_{10}((x + 0.01) / 0.19) \cdot 261.5) / 1023.0$
     - $x < 0.01125 \implies (x \cdot ((171.2102946929 - 95.0) / 0.01125) + 95.0) / 1023.0$
   - Inverse (`S-Log3 → Linear S-Gamut3`, $\text{cut}_y = 171.2102946929 / 1023.0$):
     - $y \ge \text{cut}_y \implies 10^{(1023.0 y - 420.0) / 261.5} \cdot 0.19 - 0.01$
     - $y < \text{cut}_y \implies (1023.0 y - 95.0) \cdot 0.01125 / (171.2102946929 - 95.0)$
6. **Doğrulanmış 3×3 Matrisler (`f64` satır-büyük, CIE xy + Bradford CAT):**
   - **Bradford D60 `(0.32168, 0.33767)` → D65 `(0.3127, 0.3290)` CAT:**
     $$\begin{bmatrix} 0.98722401 & -0.00611323 & 0.01595329 \\ -0.00759837 & 1.00186148 & 0.00533004 \\ 0.00307258 & -0.00509596 & 1.08168060 \end{bmatrix}$$
   - **`AP0_2_AP1` (ACES2065-1 → ACEScg):**
     $$\begin{bmatrix} 1.4514393161 & -0.2365107469 & -0.2149285693 \\ -0.0765537734 & 1.1762296998 & -0.0996759264 \\ 0.0083161484 & -0.0060324498 & 0.9977163014 \end{bmatrix}$$
   - **`AP1_2_AP0` (ACEScg → ACES2065-1):**
     $$\begin{bmatrix} 0.6954522414 & 0.1406786965 & 0.1638690622 \\ 0.0447945634 & 0.8596711185 & 0.0955343182 \\ -0.0055258826 & 0.0040252103 & 1.0015006723 \end{bmatrix}$$
   - **`AP1_to_LinearRec709` (ACEScg D60 → Linear Rec.709 D65, Bradford ile):**
     $$\begin{bmatrix} 1.70505099 & -0.62179212 & -0.08325887 \\ -0.13025642 & 1.14080474 & -0.01054832 \\ -0.02400336 & -0.12896898 & 1.15297233 \end{bmatrix}$$
   - **`LinearRec709_to_AP1` (Linear Rec.709 D65 → ACEScg D60, Bradford ile):**
     $$\begin{bmatrix} 0.61309740 & 0.33952315 & 0.04737945 \\ 0.07019372 & 0.91635388 & 0.01345240 \\ 0.02061559 & 0.10956977 & 0.86981463 \end{bmatrix}$$
   - **`LinearRec2020_to_LinearRec709` (D65):**
     $$\begin{bmatrix} 1.66049100 & -0.58764114 & -0.07284986 \\ -0.12455047 & 1.13289990 & -0.00834942 \\ -0.01815076 & -0.10057890 & 1.11872966 \end{bmatrix}$$
   - **`AWG3_to_LinearRec709` (D65):**
     $$\begin{bmatrix} 1.61752344 & -0.53728662 & -0.08023681 \\ -0.07057274 & 1.33461306 & -0.26404032 \\ -0.02110173 & -0.22695388 & 1.24805560 \end{bmatrix}$$
   - **`SGamut3_to_LinearRec709` (D65):**
     $$\begin{bmatrix} 1.87791513 & -0.79416876 & -0.08374637 \\ -0.17680698 & 1.35099962 & -0.17419264 \\ -0.02620113 & -0.14842226 & 1.17462339 \end{bmatrix}$$

*(Not: Tüm matrisler ve tersleri `f64` hassasiyetle CIE 1931 xy primerlerinden ve Bradford `M_BFD` matrisinden türetilerek çapraz doğrulanmıştır.)*

---

## Çıkarımlar ve Öneriler (Mimari Şartname)

### 1. `CubeLut3D` ve `Lut1D` Genişletmesi (`crates/frameshift-render/src/lut.rs`)

> [!IMPORTANT]
> **Shaper Precision Kör Noktası:** Birleşik `LUT_1D_SIZE` + `LUT_3D_SIZE` `.cube` dosyalarında 1D shaper genellikle HDR/log aralığını `[0, 1]` 3D küp içine sıkıştırmak içindir. Eğer bir HDR log shaper doğrudan doğrusal eksenli tek bir `33^3` 3D LUT'a re-bake edilirse koyu tonlar (`[0.0, 0.1]`) ilk tek vokselin içine sıkışır ve posterizasyon/bantlaşma oluşur. Bu nedenle iki katmanlı bir tasarım önerilir:
> - **SDR `[0, 1]` 1D veya 1D+3D `.cube` için:** Mevcut `lut3d.wgsl` ile %100 uyumlu olacak şekilde 3D grid'e (`DEFAULT_BAKED_LUT_SIZE = 65` veya `size_3d.max(33)`) compose edilip yüklenebilir.
> - **HDR / Geniş Domain Shaper `.cube` için (Önerilen):** `CubeLut3D` üzerinde `shaper_1d: Option<CubeLut1D>` saklanır; CPU `apply_rgb` daima tam `lut3d(shaper_1d(rgb))` uygular, GPU tarafında ise `lut3d.wgsl` içine opsiyonel 1D shaper dokusu (`t_shaper_1d: texture_2d<f32>` boyut `M × 1` veya `has_shaper` uniform bayrağı) bağlandığında 0 hassasiyet kaybı elde edilir. İlk aşamada `Rgba16Float` `3D` dokuya re-bake + `shaper_1d` alanı birlikte tutulursa hem eski boru hattı bozulmaz hem sonraki adımda GPU 1D shaper dokusu doğrudan açılabilir.

#### Önerilen Rust Veri Yapıları (`crates/frameshift-render/src/lut.rs`)

```rust
pub const MAX_LUT_1D_SIZE: u32 = 65_536;
pub const DEFAULT_1D_TO_3D_BAKE_SIZE: u32 = 33; // pürüzsüz 1D eğriler için; veya 65

#[derive(Debug, Clone, PartialEq)]
pub struct CubeLut1D {
    pub title: String,
    pub size: u32,
    pub domain_min: [f32; 3],
    pub domain_max: [f32; 3],
    pub entries: Vec<[f32; 3]>, // len == size
}

impl CubeLut1D {
    /// Her kanal için bağımsız doğrusal interpolasyon (out-of-domain clamp).
    pub fn sample_linear(&self, rgb: [f32; 3]) -> [f32; 3] { /* ... */ }

    /// 1D eğriyi `grid_size^3` ayrılabilir 3D kimlik ızgarasına bake eder.
    pub fn bake_to_3d(&self, grid_size: u32) -> Result<CubeLut3D, RenderError> { /* ... */ }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CubeLut3D {
    pub title: String,
    pub size: u32,
    pub domain_min: [f32; 3],
    pub domain_max: [f32; 3],
    pub data_rgba16f_or_f32: Vec<[f32; 4]>,
    /// Varsa 3D küpten önce uygulanan 1D shaper (CPU exact path + gelecek GPU 1D bind için).
    pub shaper_1d: Option<CubeLut1D>,
    /// Dönüşümün beklediği/ürettiği renk uzayı (varsayılan LinearRec709).
    pub input_space: ColorSpace,
    pub output_space: ColorSpace,
}
```

#### `.cube` Ayrıştırma Kuralları (`CubeLut3D::parse_cube`)
1. Satır satır `#` yorumlarını temizle, boş satırları atla.
2. Başlık anahtar kelimelerini takip et:
   - `TITLE "..."`
   - `LUT_1D_SIZE <m>` (`2..=65536`)
   - `LUT_3D_SIZE <n>` (`2..=128`)
   - `DOMAIN_MIN <r> <g> <b>` / `DOMAIN_MAX <r> <g> <b>`
   - `LUT_1D_INPUT_RANGE <min> <max>` (her 3 kanala `[min, min, min]..[max, max, max]` yazar)
   - `LUT_3D_INPUT_RANGE <min> <max>` (3D domain'i set eder; birleşik dosyada 1D ve 3D domain'leri ayrı tutulur!)
3. **3 Geçerli Durum:**
   - **Yalnızca `LUT_3D_SIZE N`:** Tam `N^3` satır beklenir (mevcut davranış korunur).
   - **Yalnızca `LUT_1D_SIZE M`:** Tam `M` satır beklenir. `CubeLut1D` kurulur ve `bake_to_3d(DEFAULT_1D_TO_3D_BAKE_SIZE)` ile 3D ızgaraya dönüştürülür (`shaper_1d: None` veya isteğe bağlı korunur; `entries.len() != M` ise `RenderError::InvalidLut3D` döner — böylece mevcut `"LUT_1D_SIZE 16\n"` testi geçmeye devam eder).
   - **Hem `LUT_1D_SIZE M` hem `LUT_3D_SIZE N`:** Toplam `M + N^3` veri satırı beklenir (`M` adet 1D shaper satırı + `N^3` adet 3D küp satırı). `CubeLut3D` doğrudan 3D tabloyu kendi `data_rgba16f_or_f32` alanında tutup `shaper_1d = Some(lut1d)` saklar **veya** GPU `lut3d.wgsl` henüz 1D doku bağlamıyorsa `bake_composed_1d_then_3d(size.max(65))` ile birleşik 3D `Rgba16Float` dokusu üretir.
4. **Hata Koşulları (`RenderError::InvalidLut3D`):** Ne 1D ne 3D boyutu varsa, `domain_max[c] <= domain_min[c]` ise, `NaN/Inf` varsa veya veri satır sayısı tam eşleşmiyorsa hata verilir.
5. **`.spi1d` ve `.spi3d` Desteği (`CubeLut1D::parse_spi1d`, `CubeLut3D::parse_spi3d`):**
   - `.spi1d`: `From <min> <max>`, `Length <N>`, `Components <1|3>`, `{ ... }` içi veriyi okur (`Components 1` ise R=G=B çoğaltılır).
   - `.spi3d`: `SPILUT 1.0`, boyut satırı `3`, ızgara boyutu `nx ny nz` (`nx == ny == nz` veya farklıysa en büyük `min(max, 128)` üzerine resample), `ri gi bi r_out g_out b_out` satırlarını `ri + gi*nx + bi*nx*ny` indeksine yerleştirir.

#### GPU Shader Küçük Güncellemesi (`lut3d.wgsl` & `Lut3DUniform`)
- [`lut3d.wgsl:49`](../../crates/frameshift-render/shaders/lut3d.wgsl#L49) içindeki `max(src.rgb / src.a, vec3<f32>(0.0))` yerine `let rgb_in = src.rgb / src.a;` yazılmalıdır; çünkü [`lut3d.wgsl:52`](../../crates/frameshift-render/shaders/lut3d.wgsl#L52) zaten `clamp((rgb_in - domain_min) * domain_inv_span, 0.0, 1.0)` ile `domain_min` altını klemplemektedir. Böylece negatif `DOMAIN_MIN` taşıyan geniş-gamut/ACEScct LUT'lar kesilmez.
- Eğer GPU 1D shaper dokusu (`t_shaper_1d` veya `LUT_1D_SIZE` + 3D) eklenecekse: `Lut3DUniform.intensity_size.z` (`has_shaper: f32`, `0.0` veya `1.0`) ve `w` (`shaper_size: f32`) boş/reserved alanları kullanılarak **48-byte uniform boyutu değiştirilmeden** genişletilebilir.

---

### 2. Saf Rust `OcioColorPipeline` (`crates/frameshift-render/src/ocio.rs`)

0 yeni Cargo crate ile saf Rust olarak tasarlanan `ocio.rs` modülü 4 alt bileşenden oluşur:

#### 2.1. Yerleşik Analitik Renk Uzayı Kataloğu (`BuiltinColorSpace`)
Harici `.ocio` dosyası verilmediğinde (veya `.ocio` içindeki `BuiltinTransform` / rol eşlemelerinde) doğrudan kullanılmak üzere `f64` hassasiyetli tam dönüşüm zinciri sağlanır:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtendedColorSpace {
    LinearRec709,
    DisplaySrgb,
    Rec709,         // BT.709 OETF
    LinearRec2020,
    Rec2020,        // BT.2020 SDR OETF
    Aces2065_1,     // AP0, D60
    AcesCg,         // AP1, D60
    AcesCct,        // AP1, D60 + S-2016-001 toe/log
    ArriLogC3,      // AWG3, D65 + EI 800 LogC3
    SonySLog3,      // S-Gamut3, D65 + S-Log3
}
```

Tüm yerleşik dönüşümler tek bir ortak hub üzerinden geçer: **`LinearRec709` (D65)** (veya ACES uzayları arası doğrudan `AP0 ↔ AP1` geçerek gereksiz D60↔D65 gidiş-gelişi önlenir):
- `to_linear_rec709_f64(space, rgb: [f64; 3]) -> [f64; 3]`
- `from_linear_rec709_f64(space, rgb: [f64; 3]) -> [f64; 3]`
- `eval_builtin_transform_f64(src, dst, rgb: [f64; 3]) -> [f64; 3]`

#### 2.2. `.ocio` YAML / Text Konfigürasyon Ayrıştırıcısı (`OcioConfig`)
`serde_yaml` crate'i eklemeden, OpenColorIO konfigürasyonlarının düzenli girinti + YAML tag (`!<ColorSpace>`, `!<MatrixTransform>`, `!<FileTransform>`, `!<ExponentTransform>`, `!<LogTransform>`, `!<GroupTransform>`) yapısını okuyan deterministik bir parser yazılır:

```rust
pub enum OcioTransform {
    Matrix { matrix: [f64; 16], offset: [f64; 4], inverse: bool },
    File { src: String, interpolation: OcioInterpolation, inverse: bool },
    Exponent { value: [f64; 4], negative_style: NegativeStyle, inverse: bool },
    Log { base: f64, inverse: bool },
    Group { children: Vec<OcioTransform>, inverse: bool },
}

pub struct OcioColorSpaceDef {
    pub name: String,
    pub aliases: Vec<String>,
    pub family: String,
    pub is_data: bool,
    pub to_reference: Option<OcioTransform>,
    pub from_reference: Option<OcioTransform>,
}

pub struct OcioConfig {
    pub profile_version: u32,
    pub config_dir: PathBuf,
    pub search_paths: Vec<PathBuf>,
    pub roles: HashMap<String, String>,
    pub colorspaces: HashMap<String, OcioColorSpaceDef>,
}
```

**Ayrıştırma Detayları:**
1. Hem çok satırlı girintili blokları hem tek satırlı akış sözdizimini (`!<MatrixTransform> {matrix: [...], direction: inverse}`) destekler; `#` yorumlarını ve Windows/Unix satır sonlarını (`\r\n` / `\n`) temizler.
2. `to_scene_reference` / `from_scene_reference` anahtarlarını OCIO v1 karşılıkları `to_reference` / `from_reference` ile eşdeğer kabul eder.
3. `roles` ve `aliases` çözümlemesi büyük/küçük harfe duyarsız ikincil eşleme tablosuyla desteklenir (`scene_linear`, `default`, vb.).
4. `FileTransform.src` yolunu önce mutlak/göreli config dizinine, bulunamazsa `search_paths` dizinlerine göre sırayla arar (`.cube`, `.spi1d`, `.spi3d`); bulunamazsa açık `RenderError::InvalidLut3D` döndürür.
5. `MatrixTransform` ters yönü (`inverse: true`) için 3×3 / 4×4 `f64` matris tersi alınır (tekil matris ise `RenderError::InvalidLut3D`).
6. Eğer bir `ColorSpace` yalnızca `to_reference` tanımlamışsa ve ters yön isteniyorsa (ör. hedef uzay için `from_reference` yoksa), `to_reference` içindeki tüm transform ağacı otomatik olarak tersine çevrilir (`invert_transform`).

#### 2.3. `OcioColorPipeline` ve 3D LUT Bake Motoru

```rust
pub struct OcioBakeOptions {
    pub lut_size: u32,           // varsayılan 33 (veya 65)
    pub domain_min: [f32; 3],    // varsayılan [0.0, 0.0, 0.0]
    pub domain_max: [f32; 3],    // varsayılan [1.0, 1.0, 1.0]
    pub shaper_space: Option<String>,
    pub allow_cli_fallback: bool, // sistemde ociobakelut varsa
}

pub struct OcioColorPipeline {
    config: OcioConfig,
}

impl OcioColorPipeline {
    pub fn from_file(config_path: &Path) -> Result<Self, RenderError>;
    pub fn from_str(yaml_text: &str, base_dir: &Path) -> Result<Self, RenderError>;

    /// `src_space` -> reference -> `dst_space` zincirini tek bir CPU değerlendiricisi olarak kurar.
    pub fn build_chain(&self, src_space: &str, dst_space: &str) -> Result<CompiledOcioChain, RenderError>;

    /// Zinciri doğrudan `CubeLut3D` (`size^3`) üzerine bake eder (0 dış süreç, deterministik).
    pub fn bake_to_cube_lut(
        &self,
        src_space: &str,
        dst_space: &str,
        opts: &OcioBakeOptions,
    ) -> Result<CubeLut3D, RenderError>;
}
```

**Opsiyonel `ociobakelut` CLI Köprüsü (Hibrit Yol):**
- Eğer `config` içinde saf Rust'ın henüz ayrıştıramadığı bir OCIO v2 transformu (ör. karmaşık `Look` + `GradingToneTransform` + `.clf`/`.ctf` XML veya `BuiltinTransform` ACES 1.3 RRT/ODT) varsa:
  1. Sistemde `ociobakelut` ikilisinin varlığı (`Command::new("ociobakelut").arg("--help")`) kontrol edilir.
  2. Varsa geçici veya önbellek dizininde:
     ```text
     ociobakelut --iconfig <config.ocio> --inputspace <src> --outputspace <dst> --cubesize <N> --format iridas_cube --stdout
     ```
     (veya `--shaperspace <shaper>` ile) çalıştırılıp çıktısı doğrudan `CubeLut3D::parse_cube` ile yüklenir.
  3. Sistemde `ociobakelut` yoksa (mevcut ortam gibi) sessiz düşme yapılmaz; desteklenmeyen transformun adı ve yoluyla açık `RenderError::InvalidLut3D("unsupported OCIO transform '<Tag>' and 'ociobakelut' CLI not found on PATH")` hatası döndürülür.

---

### 3. `NodeKind::Lut3D` ve `ColorSpace` Geriye Uyumlu Şema Genişlemesi (`frameshift-schema`)

Mevcut [`NodeKind::Lut3D`](../../crates/frameshift-schema/src/graph.rs#L280-L285) ve `golden_scene_preset` JSON'larını kırmadan [project-schema.md §5 #10](../specs/project-schema.md#L113-L115) ile tam uyum sağlamak için tüm yeni alanlar `#[serde(default)]` olarak eklenir:

```rust
/// External LUT or OCIO color transform source for `NodeKind::Lut3D`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LutSourceDef {
    /// Built-in preset name (`"identity"`, `"anime_cyber_contrast"`, `"warm_sunset"`, `"bleach_bypass"`).
    Preset {
        name: String,
    },
    /// External `.cube` (1D, 3D, or combined 1D+3D) or `.spi1d` / `.spi3d` file.
    CubeFile {
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lut_size_override: Option<u32>,
    },
    /// OpenColorIO `.ocio` config-driven transform baked to 3D LUT.
    Ocio {
        config_path: String,
        src_space: String,
        dst_space: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        look: Option<String>,
        #[serde(default = "default_ocio_lut_size")]
        lut_size: u32, // default 33
        #[serde(default, skip_serializing_if = "Option::is_none")]
        shaper_space: Option<String>,
    },
    /// Built-in analytic space-to-space transform (no external file needed).
    BuiltinTransform {
        src_space: ExtendedColorSpace,
        dst_space: ExtendedColorSpace,
        #[serde(default = "default_ocio_lut_size")]
        lut_size: u32, // default 33
    },
}
```

`NodeKind::Lut3D` güncellemesi:

```rust
    #[serde(alias = "lut_3d")]
    Lut3D {
        input: NodeId,
        /// Legacy / quick preset or inline/path string (kept for 100% backward compatibility).
        #[serde(default)]
        lut_preset: String,
        #[serde(default = "default_one")]
        intensity: f32,
        /// Structured LUT / OCIO source; takes precedence over `lut_preset` when `Some`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lut_source: Option<LutSourceDef>,
        /// Optional explicit input color space declaration for DAG color adaptation.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        input_space: Option<ExtendedColorSpace>,
        /// Optional explicit output color space declaration for DAG color adaptation.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        output_space: Option<ExtendedColorSpace>,
    },
```

**Geriye Uyumluluk ve Doğrulama Kuralları:**
1. Eski `"type": "lut3_d"` JSON'larında `lut_source`, `input_space`, `output_space` yoksa `None` olarak yüklenir ve mevcut `lut_preset` üzerinden çalışır; `golden_scene_preset` değişmeden geçer.
2. `ColorSpace` enum'u `SourceVideo` ve `Solid` içinde kullanıldığı için mevcut `LinearRec709 | DisplaySrgb` varyantları korunur; istenirse ileride `ColorSpace` genişletilebilir veya `ExtendedColorSpace` üzerinden köprülenir (`From<ColorSpace> for ExtendedColorSpace`).
3. `RenderGraphDef::validate_graph`:
   - `lut_source` `None` ise `lut_preset` boş olmamalıdır.
   - `lut_source` `Some(LutSourceDef::Ocio { lut_size, config_path, src_space, dst_space, .. })` ise `!(2..=128).contains(lut_size)` veya boş `config_path`/`src_space`/`dst_space` için `SchemaError::InvalidLut3D` (yeni veya mevcut doğrulama hatası) döndürülebilir.
   - `bindable_param_names`: `NodeKind::Lut3D` için `&["intensity"]` korunur.

---

### 4. `GpuCompositor` Entegrasyonu, Cache Anahtarı ve `LinearRec709` Çalışma Uzayı Sınırı

1. **Yapılandırılmış `LutCacheKey`:**
   Mevcut `lut_cache: HashMap<String, CachedLut3D>` yerine kanonik bir `LutCacheKey` kullanılır:
   - `Preset(String)`
   - `CubeFile { path, size_override, modified_nanos_or_hash }` (dosya disk üzerinde değiştiğinde deterministik yeniden yükleme veya `revision_id`/render oturumu başına tutarlı cache)
   - `Ocio { config_path, src_space, dst_space, look, lut_size, shaper_space }`
   - `BuiltinTransform { src_space, dst_space, lut_size }`
2. **`LinearRec709` Çalışma Uzayı Köprüsü ([render-graph.md §1 & §3.2](../specs/render-graph.md#L29-L76)):**
   - FrameShift DAG ara dokuları daima **`LinearRec709` premultiplied alpha (`Rgba16Float`)** taşır.
   - Eğer bir `Lut3D` düğümünün `input_space` alanı `LinearRec709` dışında bir uzay beyan ediyorsa (ör. `DisplaySrgb`, `ArriLogC3`, `SonySLog3`, `AcesCg`) ve `lut_source` bir `.cube` dosyasıysa (ör. klasik sRGB/LogC3 → Rec.709 kreatif `.cube` LUT'ı):
     - Kompozitör `CubeLut3D` tablosunu GPU'ya yüklerken (veya `Lut3D` öncesi/sonrası) giriş adaptasyonunu (`LinearRec709 → input_space`) LUT içine pre-bake edebilir **ya da** WGSL/ön-geçiş ile çevirir, çıkışta ise `output_space → LinearRec709` dönüşümünü uygular.
     - **En temiz ve sıfır ekstra GPU geçişli yöntem:** Eğer `input_space != LinearRec709` veya `output_space != LinearRec709` belirtilmişse, CPU yükleme anında `LinearRec709 → input_space → [LUT / OCIO] → output_space → LinearRec709` bileşimi tek `CubeLut3D` (veya `shaper_1d + 3D`) içine bake edilir; böylece DAG'daki komşu düğümler (`Blend`, `DeepGlow`, `MotionBlurShutter`) her zaman `LinearRec709` almaya devam eder.
     - *(Dikkat: Yalnızca `input_space` log/HDR olup `LinearRec709` doğrusal ışık değerleri `> 1.0` veya `< 0.1` yoğunlaştığında, yukarıda belirtilen 1D shaper hassasiyet kuralı devreye girer; bu durumda `lut3d.wgsl` içinde analitik pre-shaper/transfer bayrağı kullanmak 33³ tek biçimli ızgaradan çok daha yüksek hassasiyet sağlar.)*

---

## Doğrulanmamış Noktalar

1. **HDR/Log `.cube` Shaper Hassasiyet Karşılaştırması:** Mevcut `lut3d.wgsl` yalnızca 3D doku (`t_lut: texture_3d<f32>`) bağlar. Birleşik 1D+3D `.cube` dosyalarında 1D shaper'ı 65³ 3D dokuya re-bake etmenin `Rgba16Float` doğrusal HDR sahnelerde kabul edilebilir PSNR/ΔE verip vermediği, yoksa `lut3d.wgsl`'e `t_shaper_1d` (`M×1` 2D doku) eklenmesinin mi gerektiği kodlama aşamasında gerçek bir log-shaper `.cube` dosyası üzerinde ölçülmelidir.
2. **Karmaşık OCIO v2 Config'ler (`ACES 1.3 Studio/CG`):** `aces-dev` / `OpenColorIO-Config-ACES` v2 konfigürasyonlarında `BuiltinTransform` (`ACES-OUTPUT - ACES2065-1_to_CIE-XYZ-D65 - SDR-100nit-REC709_1.1` vb.) ve `GradingPrimaryTransform` düğümleri yoğun kullanılır. Sistemde `ociobakelut` CLI olmadığı ortamda bu v2 `BuiltinTransform` adlarının ne kadarının saf Rust'a dahil edileceği (yalnızca temel ACEScg/ACEScct/sRGB/LogC3/SLog3 mü, yoksa tam ACES 1.0/1.3 RRT+ODT tonemap de mi) uygulama öncesi karar noktasıdır.
3. **M1/M2 Mevcut Test Etkisi:** [`compositor_tests.rs:677`](../../crates/frameshift-render/tests/compositor_tests.rs#L677) içindeki `"LUT_1D_SIZE 16\n"` negatif testi veri satırı içermediği için yeni ayrıştırıcıda da `Err(InvalidLut3D)` döndürmeye devam edecektir; ancak mevcut dış projeler veya testler `LUT_1D_SIZE` hata mesajı metnine tam string eşlemesi yapıyorsa kontrol edilmelidir.
4. **Dönüşüm Doğruluğu Testi (Referans Karşılaştırması):** Üretilen 3×3 matrisler CIE xy ve Bradford sabitlerinden `f64` ile doğrulanmış olsa da, `ociobakelut` ikilisinin kurulu olduğu bir makinede referans `.ocio` çıktısı ile FrameShift `OcioColorPipeline` çıktısının piksel başı $\Delta E$ sapması henüz canlı test edilmemiştir.
