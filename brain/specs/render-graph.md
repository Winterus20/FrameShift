---
id: render-graph
status: locked
created: 2026-11-21
updated: 2026-11-21
related_adr: [0001]
tags: [render-graph, shaders, color, text-vector, engine]
---

# Render Grafiği ve GPU Kompozisyon Sözleşmesi (`render-graph`)

> [!NOTE]
> M1 çekirdek düğümleri (`frameshift-render` ve 7 WGSL shader) canlı doğrulanmış; Faz 2 genişleme düğümleri (`VectorText`, `Lut3D`) ile birlikte bu sözleşme kilitlenmiştir. Bağlama ve shader kuralları için [effect-shader-api.md](effect-shader-api.md) belgesine bakınız.

## Amaç ve Kapsam

Bu sözleşme, FrameShift'in `wgpu` + WGSL tabanlı birleşik GPU kompozisyon motorunun (`frameshift-render`) renk alanı akışını, premultiplied-alpha kurallarını, dallanabilen DAG değerlendirme modelini, sub-frame temporal motion blur akümülasyonunu ve seek-independent determinizm garantilerini tanımlar.

---

## Veri Modeli / Arayüz Sözleşmesi

### 1. Çalışma Piksel Formatı ve Renk Alanı Akışı

- **Ara Çalışma Dokusu (Working Texture):** Tüm DAG düğümleri arasındaki ara dokular **`wgpu::TextureFormat::Rgba16Float`** formatındadır.
- **Alfa Kuralı:** Tüm dahili dokular **Premultiplied Alpha** (`[R·A, G·A, B·A, A]`) taşır.
  - Kenar sızıntısını (dark/light fringing) önlemek için `Transform` örneklemesi, `MaskMatte`, `Blend` ve `MotionBlurShutter` akümülasyonu doğrudan premultiplied uzayda çalışır.
  - `Multiply` veya `Overlay` gibi un-premultiply gerektiren blend işlemlerinde sıfıra bölme koruması (`alpha > 1e-6`) zorunludur.
- **Renk Alanı (`LinearRec709` ↔ `DisplaySrgb`):**
  - **Dahili Çalışma Uzayı:** Doğrusal ışık uzayı (**`LinearRec709`**).
  - **Giriş Dönüşümü:** `DisplaySrgb` (8-bit video / PNG vb.) girişler `LinearRec709` uzayına IEC 61966-2-1 parçalı sRGB EOTF ile dönüştürülür ve alfa ile çarpılır.
  - **Çıkış Dönüşümü:** Nihai `Rgba16Float` yüzey (gerektiğinde tonemap/clamp sonrası) sRGB OETF ile `Rgba8Unorm` (PNG inspection) veya video encode formatına (`YUV420P`) dönüştürülür.
  - Her düğüm beklediği giriş/çıkış renk uzayını açıkça beyan eder (`ColorTransform` trait sözleşmesi; M2'de OpenColorIO / 3D LUT entegrasyonu bu soyutlama arkasına yerleşir).

### 2. Seek-Independent Değerlendirme ve DAG Yönetimi

- **Saf Fonksiyon İlkesi:** Bir çıktı karesinin değerlendirilmesi yalnızca `(project_snapshot, output_tick, render_profile)` üçlüsüne bağlıdır:
  $$Frame(t) = Evaluate(Graph, t, Profile)$$
  Kareler arası gizli kalıcı durum (önceki karenin GPU akümülatöründe bırakılması vb.) **yasaktır**. Bu sayede rastgele bir `--tick T` çağrısı ile baştan sona sıralı MP4 render'ındaki `T` anı karesi birebir eşleşir.
- **Topolojik Sıralama ve Bellek Havuzu:**
  - `graph` topolojik sıraya dizilir; aynı alt-grafik birden fazla dalda kullanılıyorsa (örn. bir klibin hem ana katmana hem bir maskeye veya `ShaderTransition`'a girmesi) aynı `tick` için yalnızca **bir kez** değerlendirilir.
  - Ara `Rgba16Float` dokular `TexturePool` üzerinden yaşam döngüsüne göre yeniden kullanılır.
- **Önizleme / Final Tutarlılığı:**
  - Tüm uzamsal parametreler (konum, hız vektörü, blur yarıçapı, tüy/feather, vignette vb.) piksel cinsinden değil, normalize `[0, 1]` veya referans tuval çözünürlüğüne göre tanımlanır. Böylece `draft` (`resolution_scale = 0.5`) ile `final` (`1.0`) farklı piksel boyutlarında aynı kompozisyon oranını korur.

### 3. Çekirdek ve Faz 2+ Düğüm Sözleşmeleri

| Düğüm | Girişler | Çıkış / Davranış |
| :--- | :--- | :--- |
| `SourceVideo` | Yok (timeline + `tick` bağlamı) | İlgili `clip_id` için `speed_curve` üzerinden `source_tick` hesaplar, VFR PTS indeksinden kareyi alır, `LinearRec709` premultiplied `Rgba16Float` üretir. |
| `Solid` | Yok | Verilen `LinearRec709` rengi (premultiplied) tüm tuvale doldurur. |
| `Transform` (`Transform2D`) | `input` | 2D taşıma, ölçek, dönüş (radyan), anchor ve bilinear/bicubic (`CatmullRom`) örnekleme; `[0, 1]` UV dışı şeffaf siyah (`vec4(0.0)`). |
| `MaskMatte` | `source`, `mask` | `alpha` veya `luma` maske modu, `invert` bayrağı ve `smoothstep` feather; premultiplied alfa korunur. |
| `Blend` | `base`, `top` | `Normal`, `Add`, `Screen`, `Multiply`, `Overlay` + `opacity`; premultiplied uyumlu. |
| `ColorBasic` | `input` | `sRGB <-> LinearRec709`, `exposure`, `contrast`, `saturation`, `flash_amount` / highlight boost. |
| `ShaderTransition` | `tex_a`, `tex_b` | `progress [0..1]`, `direction`, `whip_strength`, `chromatic_aberration`, `impact_flash` gibi parametrelerle iki alt-grafiği birleştiren WGSL geçişi. |
| `MotionBlurShutter` | `input` (alt-grafik) | Aşağıdaki sub-frame temporal motion blur sözleşmesine göre üst grafiği `N` alt-zamanda değerlendirip biriktirir. |
| `VectorText` *(Faz 2)* | Yok (üreteç) | Tipografi ve 2D vektör/şekil katmanını (`frameshift-vector`: `vello` / `skia-safe`) referans tuval oranında rasterize edip **`LinearRec709` premultiplied-alpha `Rgba16Float`** çıktısı üretir. |
| `Lut3D` *(Faz 2)* | `input` | `Rgba16Float` **trilinear 3D LUT** (`texture_3d<f32>`) renk dönüşüm düğümü; `.cube` / OpenColorIO uyumlu giriş-çıkış renk alanı eşlemesi (`domain_min..domain_max`, `intensity [0..1]`) uygular ve premultiplied alfayı korur. |

*(İsteğe bağlı ek genişleme yuvaları: `SourceImage`, `ShaderEffect`, `Precomp` — aynı `Rgba16Float` premultiplied-alpha sözleşmesini izler.)*

#### 3.1. `VectorText` Rasterizasyon Sözleşmesi (Faz 2)

- **Koordinat ve Çözünürlük Bağımsızlığı:** `font_size_norm`, `position`, `letter_spacing` ve vektör `path` koordinatları referans tuval yüksekliğine/genişliğine göre normalize edilir. `draft` (`0.5x`) ve `final` (`1.0x`) profillerinde metin yerleşimi, satır kırılımı ve göreli kalınlık değişmez.
- **Premultiplied-Alpha ve Renk Alanı:** Rasterizer çıktısı `LinearRec709` uzayına ve premultiplied-alpha biçimine dönüştürülerek doğrudan `Rgba16Float` ara dokuya yazılır; `MotionBlurShutter` alt-zamanlarında ve `MaskMatte` arkasında karakterlerin arasına gizlenen metinlerde kenar sızıntısı (fringing) oluşmaz.
- **Determinizm:** Aynı font dosyası, metin içeriği, glif/shaping ayarları ve `revision_id` ile her render'da piksel düzeyinde özdeş raster üretilir.

#### 3.2. `Lut3D` Trilinear 3D LUT ve OCIO Sözleşmesi (Faz 2)

- **3D Doku Formatı:** 3D LUT tabloları GPU'ya `wgpu::TextureDimension::D3` üzerinde `Rgba16Float` (veya donanım filtrelemeli normalize 3D doku + WGSL trilinear interpolasyon) olarak yüklenir (`@binding(1) var t_lut: texture_3d<f32>`).
- **Trilinear Örnekleme ve Domain Eşlemesi:**
  - Giriş pikseli `alpha > 1e-5` korumasıyla un-premultiply edilir (`rgb / max(alpha, 1e-5)`).
  - İsteğe bağlı giriş shaper/transfer (`sRGB` ↔ `LinearRec709` veya OCIO config dönüşümü) sonrası `coord = clamp((rgb - domain_min) / (domain_max - domain_min), 0.0, 1.0)` ile `[(0.5/N), 1.0 - (0.5/N)]` merkezli LUT koordinatına haritalanır ve trilinear interpolasyon uygulanır.
  - `intensity [0..1]` ile orijinal ve LUT uygulanmış RGB karıştırılır, çıkış renk uzayı beyanı uygulanır ve tekrar `src.a` ile premultiply edilir (`vec4(rgb_out * src.a, src.a)`).

### 4. Sub-Frame Temporal Motion Blur Sözleşmesi (`MotionBlurShutter`)

- **Parametreler:**
  - `shutter_angle_deg: f32` — Örtücü açısı (tipik anime/sinema değeri `180.0°`, geçerli aralık `0.0 ..= 720.0`). `0.0°` iken motion blur devre dışıdır (`sub_samples = 1`).
  - `sub_samples: u32` — Alt-kare örnek sayısı (`N >= 1`; `draft` profilinde örn. `4`, `final` profilinde `8`–`16`).
  - `phase: ShutterPhase` — `centered` veya `trailing`.
- **Zaman Penceresi Hesabı:**
  - Kare süresi `F = round(FLICKS_PER_SECOND * fps.den / fps.num)` (`Flicks`).
  - Örtücü açıklık süresi: `W = F * (shutter_angle_deg / 360.0)`.
  - **`centered`:** `[t - W / 2, t + W / 2]`
  - **`trailing`:** `[t - W, t]`
- **Alt-Örnek Zamanları ve Ağırlıklı Biriktirme:**
  - $i \in \{0, \dots, N-1\}$ için örnekleme anı deterministik orta-nokta ızgarasıyla hesaplanır:
    $$t_i = t_{start} + \frac{i + 0.5}{N} \cdot (t_{end} - t_{start})$$
  - Üst alt-grafiğin **tamamı** (hız eğrisi + `Transform` + `MaskMatte` + `ShaderTransition` dahil) her $t_i$ anında değerlendirilir.
  - Akümülasyon `Rgba16Float` tamponda eşit veya normalize ağırlıklarla ($\sum_{i=0}^{N-1} w_i = 1.0$) toplanır:
    $$C_{out} = \sum_{i=0}^{N-1} w_i \cdot C(t_i)$$
  - `t_i` değerleri klip sınırlarının dışına düşerse klempleme veya şeffaf siyah politikası açıkça uygulanır; hata ile çökme olmaz.

### 5. GPU Güvenliği (Naga Doğrulama ve TDR Koruması)

- Tüm yerleşik ve özel WGSL shader'ları `wgpu` / `naga` doğrulamasından geçmek zorundadır.
- Sonsuz döngülerin Windows GPU TDR (~2 sn sürücü sıfırlaması) tetiklemesini önlemek için shader içi tüm döngüler sabit veya üst sınırlı (bounded loop) olmak zorundadır.

---

## Kenar Durumlar ve Hata Davranışı

- **Geçersiz `shutter_angle_deg` (`< 0.0`) veya `sub_samples == 0`:** Düğüm doğrulamasında `InvalidMotionBlurParams` hatası döndürülür.
- **Eksik Düğüm Girişi (`MaskMatte.mask`, `ShaderTransition.tex_b` vb.):** Grafiği çalıştırmadan önce `MissingGraphInput` hatası verilir.
- **WGSL Derleme / Naga Hatası:** `ShaderCompilationFailed` hatası shader adı ve Naga tanı bilgisiyle raporlanır; sessizce pas geçilmez.
- **Alt-Kare Zamanının Klip Dışına Taşması:** Motion blur penceresi klip sınırına değdiğinde zaman çizgisinin aktif kuralı (boş/şeffaf siyah veya edge clamp) deterministik uygulanır.
- **Eksik/Bozuk 3D LUT veya Geçersiz Domain (`Lut3D`):** `.cube` / OCIO dosyası bulunamaz, boyut (`N < 2`) geçersiz veya `domain_max <= domain_min` ise `InvalidLut3D` hatasıyla reddedilir.
- **Eksik Font veya Boş/Geçersiz Vektör Yolu (`VectorText`):** Belirtilen font çözümlenemezse sessiz rastgele font seçimi yerine tanımlı fallback politikası raporlanır veya `FontNotFound` / `InvalidVectorPath` hatası verilir.

---

## Kabul Kriterleri (Kod Aşamasında Doğrulanacak)

- [ ] Tüm çekirdek WGSL shader'ları (`transform`, `mask_matte`, `blend`, `color_basic`, `transition_whip_chromatic`, `motion_blur_accumulate`, `lut_3d`) `wgpu` / Naga ile hatasız derlenir.
- [ ] **Seek-Independence Testi:** Golden Scene içinde rastgele seçilen en az 5 `tick` noktasında tekil kare çıktısı (`frameshift frame --tick T`) ile sıralı MP4 render çıktısının `T` karesi piksel düzeyinde eşleşir.
- [ ] **Determinizm Testi:** Aynı proje dosyası ve profille arka arkaya iki render çalıştırıldığında kare hash'leri birebir aynıdır.
- [ ] `MotionBlurShutter` (`centered` vs `trailing`, `sub_samples = 1` vs `8`) hareketli bir dönüşüm üzerinde beklenen iz yönünü ve `Rgba16Float` enerji korunumunu doğrular.
- [ ] `VectorText` (`draft` vs `final` ölçekleme) metin konumunu/orantısını korur ve şeffaf kenarlarda siyah halo (premultiplied-alpha hatası) üretmez.
- [ ] `Lut3D` birim (identity) 3D LUT tablosu ile giriş pikselini tolerans (`<= 1e-3`) içinde korur ve yarı şeffaf bölgelerde premultiplied alfayı bozmaz.
