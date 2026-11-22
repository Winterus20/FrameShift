---
id: effect-shader-api
status: locked
created: 2026-11-21
updated: 2026-11-21
related_adr: [0001, 0002, 0003]
tags: [shaders, render-graph, engine, agent]
---

# Efekt, Shader ve Güvenli Parametre Bağlama Sözleşmesi (`effect-shader-api`)

## Amaç ve Kapsam

Bu sözleşme, FrameShift'in `wgpu` + WGSL tabanlı kompozisyon motorunda (`frameshift-render`) çalışan tüm yerleşik ve özel efekt/geçiş shader'larının (`ShaderEffect`, `ShaderTransition`, `Lut3D` ve çekirdek düğümler) GPU bağlama düzenini (binding layout), 16-byte uniform hizalama kurallarını, yakıt sınırlı (`SafeExpr`) parametre ifade değerlendiricisini ve Naga/TDR güvenlik sınırlarını kilitler.

Bu sözleşme sayesinde hem M1'in yerleşik WGSL düğümleri hem de Faz 2–3'te eklenecek özel shader/expression bağlamaları deterministik, seek-independent ve GPU sürücü çökmesine (TDR) karşı korumalı çalışır.

---

## Veri Modeli / Arayüz Sözleşmesi

### 1. Ortak WGSL Vertex Arayüzü ve Çalışma Yüzeyi

- **Tam Ekran Üçgeni (Fullscreen Triangle):** Tüm 2D kompozisyon ve efekt shader'ları harici vertex buffer gerektirmeyen tek tip `@vertex fn vs_main(@builtin(vertex_index) vid: u32) -> VertexOutput` girişini kullanır:
  - `position: vec4<f32>` (NDC `[-1, 1]`)
  - `uv: vec2<f32>` (normalize `[0, 1]`, sol-üst `(0, 0)` → sağ-alt `(1, 1)`)
- **Fragment Çıktısı:** `@fragment fn fs_main(in: VertexOutput) -> @location(0) vec4<f32>`
- **Piksel ve Renk Sözleşmesi:** Tüm ara giriş/çıkış dokuları `wgpu::TextureFormat::Rgba16Float`, `LinearRec709` çalışma uzayı ve **Premultiplied Alpha** taşır (bkz. [render-graph.md](render-graph.md)).

### 2. Standart `@group(0)` Düğüm Bağlama Düzeni

Tüm efekt ve geçiş düğümleri yalnızca `@group(0)` bağlama grubunu kullanır; motor düğüm arity'sine (giriş sayısına) göre bağlama yuvalarını otomatik bağlar:

| Şablon Sınıfı | `@binding(0)` | `@binding(1)` | `@binding(2)` | `@binding(3)` | Örnek Düğümler |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Tek Girişli Efekt (`Effect1D`)** | `t_source: texture_2d<f32>` | `s_linear: sampler` | `params: var<uniform>` | — | `Transform2D`, `ColorBasic`, `ShaderEffect` |
| **İki Girişli Kompozisyon / Geçiş (`Effect2D`)** | `tex_a: texture_2d<f32>` | `tex_b: texture_2d<f32>` | `s_linear: sampler` | `params: var<uniform>` | `Blend`, `MaskMatte`, `ShaderTransition` |
| **3D LUT Renk Düğümü (`EffectLut3D`)** | `t_source: texture_2d<f32>` | `t_lut: texture_3d<f32>` | `s_linear: sampler` | `params: var<uniform>` | `Lut3D` |

- **Özel Efektler İçin Ek Kanal Kuralı:** `ShaderEffect` / `ShaderTransition` içinde zaman ve çözünürlük bilgisinin doğrudan kullanılabilmesi için `params` uniform yapısının ilk bloğu veya standart sistem başlığı (`FrameContextUniform`) olarak `t_sec`, `progress`, `aspect`, `resolution_scale` alanları taşınır; böylece özel shader'lar dahili zaman durumunu saklamak zorunda kalmaz.

### 3. 16-Byte Uniform Hizalama Kuralları (`std140` / WGSL)

`wgpu` uniform buffer uyumluluğu ve Rust `bytemuck::Pod` sıfır kopya yüklemesi için tüm uniform struct'ları aşağıdaki kurallara uymak zorundadır:

1. **16-Byte Bloklama:** Tüm alanlar `vec4<f32>`, `vec4<u32>` veya `vec4<i32>` grupları halinde paketlenir; 3 bileşenli `vec3<f32>` doğrudan tekil alan olarak bırakılmaz, 4. bileşende açık `_pad` veya ayrılmış parametre taşır.
2. **Toplam Boyut:** Her `Uniform` struct'ının toplam byte uzunluğu **`16`'nın tam katı** olmalıdır.
3. **Host-GPU Eşitliği:** Rust tarafındaki `#[repr(C)]` struct yerleşimi ile WGSL `struct ...Uniform` alan sırası birebir eşleşir; derleme veya test aşamasında `size_of::<T>() % 16 == 0` ve alan ofsetleri doğrulanır.

### 4. Yakıt Sınırlı Döngüsüz Parametre İfadeleri (`SafeExpr`)

`ParamBindingDef` (bkz. [project-schema.md](project-schema.md)) içinde kullanılan matematiksel ifadeler (`SafeExpr`), düğüm parametrelerini müzik enerjisine, `event_id` mesafesine veya normalize ilerlemeye bağlamak için tasarlanmış saf, yan etkisiz bir ifade dilidir.

#### 4.1. Salt-Okunur Bağlam Değişkenleri

`SafeExpr` değerlendirmesi yalnızca o anki değerlendirme bağlamından (`ExprContext`) gelen salt-okunur değişkenleri görebilir:

| Değişken | Tip | Açıklama |
| :--- | :---: | :--- |
| `t_sec` | `f64` | Çıktı zaman çizgisindeki saniye karşılığı (`output_tick / 705_600_000.0`). |
| `output_tick` | `f64` | Çıktı tick değeri (salt-okunur sayısal temsil; kalıcı depolama değildir). |
| `beat_phase` | `f64` | En yakın analiz edilmiş veya işaretli beat aralığındaki normalize faz (`[0.0, 1.0)`). |
| `audio_rms` | `f64` | İlgili çıktı zamanındaki normalize ses RMS enerji zarfı (`[0.0, 1.0]`). |
| `event_delta_sec` | `f64` | Bağlı `event_id`'nin `target_output_tick` anına göre işaretli saniye farkı (`(t - t_event)`). |
| `progress` | `f64` | Klip, geçiş veya `EventBoundAnimation` süresi içindeki normalize ilerleme (`[0.0, 1.0]`). |

#### 4.2. Dil ve Yürütme Kısıtları

- **Döngüsüz AST (Acyclic):** `SafeExpr` ayrıştırıcısı yalnızca aritmetik (`+`, `-`, `*`, `/`, `%`, tekli `-`), karşılaştırma/koşul (`if(cond, a, b)`, `<`, `<=`, `>`, `>=`, `==`), mantıksal operatörler (`&&`, `||`, `!`) ve beyaz listeli saf matematik fonksiyonlarını (`sin`, `cos`, `abs`, `min`, `max`, `clamp`, `mix`, `smoothstep`, `step`, `floor`, `ceil`, `exp`, `sqrt`, `pow`, `log`) kabul eder.
- **Yasaklar:** Döngü (`for`/`while`), özyineleme (recursion), fonksiyon tanımı, bellek tahsisi, I/O, rastgele sayı (seed'siz RNG) ve düğüm-dışı değişken mutasyonu **yasaktır**. Düğüm parametrelerinin birbirine dairesel referans vermesi grafik doğrulamasında reddedilir.
- **Yakıt (Fuel) ve Derinlik Sınırları:**
  - `MAX_EXPR_DEPTH: usize = 16` (AST derinlik üst sınırı).
  - `MAX_EXPR_FUEL: u32 = 256` (tek bir kare/alt-örnek değerlendirmesi için toplam AST işlem adımı bütçesi).
  - Her AST düğümü yürütülürken `fuel` 1 azalır; `fuel == 0` olursa değerlendirme derhal kesilir.

#### 4.3. Güvenli Bölme, Üs Alma ve Sonlu Sayı Koruması

- **Güvenli Bölme (`a / b`) ve Mod (`a % b`):** `abs(b) < 1e-6` ise sıfıra bölme hatası üretmek veya `Inf`/`NaN` yaymak yerine tanımlı politika uygulanır (`0.0` döndürülür veya `ExprEvaluationError::DivisionByZero` ile fallback değerine düşülür; sessiz `NaN` yasaktır).
- **Güvenli Üs Alma (`pow(base, exp)`):** Negatif `base` ile kesirli `exp` birleşiminde (`NaN` üretecek durum) `pow(max(base, 0.0), exp)` veya `0.0` koruması uygulanır; `exp` ve sonuç büyüklüğü `Inf` üretmeyecek şekilde sonluluk (`is_finite()`) kontrolünden geçirilir.
- **Diğer Tanım Kümesi Korumaları:** `sqrt(max(x, 0.0))`, `log(max(x, 1e-6))`, `asin`/`acos(clamp(x, -1.0, 1.0))`.
- **Sonuç Klempleme:** Her `SafeExpr` çıktısı `is_finite()` testinden geçer ve ilgili `ParamBindingDef` üzerinde tanımlı `[clamp_min, clamp_max]` aralığına klemplenir.

### 5. Naga Doğrulaması ve GPU TDR Döngü Güvenliği

Özel veya agent tarafından üretilen WGSL shader'ları `wgpu::Device::create_shader_module` aşamasına gönderilmeden önce katı statik denetimden geçer:

1. **Naga Ön Doğrulaması:**
   - `naga::front::wgsl::parse_str` ile sözdizimi ayrıştırılır.
   - `naga::valid::Validator::new(ValidationFlags::all(), Capabilities::empty())` ile tip güvenliği, bellek sınırı ve erişim denetimi yapılır; başarısız shader derhal `ShaderValidationFailed` hatası ile reddedilir.
2. **Döngü Üst Sınırı (TDR Koruması):**
   - Windows GPU sürücülerindeki ~2 saniyelik TDR (Timeout Detection and Recovery) sıfırlamasını önlemek için shader içinde sınırsız `loop` veya sabit olmayan sonsuz döngü **yasaktır**.
   - Tüm `for` / döngü blokları statik veya sabit üst sınırlı (`MAX_SHADER_LOOP_ITERATIONS: u32 = 128`) olmak zorundadır; Naga modülü taramasında döngü üst sınırı kanıtlanamayan veya sınırı aşan kullanıcı shader'ları `UnsafeShaderLoopBound` hatasıyla reddedilir.
3. **Çalışma Zamanı Kurtarma (Worker Timeout):**
   - GPU komut gönderimi zaman aşımına uğrar veya `wgpu::Error::OutOfMemory` / `DeviceLost` tetiklenirse motor sessizce bozuk kare yazmaz; mevcut `revision_id` ile açık `GpuExecutionTimeout` / `GpuDeviceLost` hatası döndürür ve güvenli kurtarma başlatır.

---

## Kenar Durumlar ve Hata Davranışı

- **Yanlış Uniform Boyutu (`size % 16 != 0`):** Pipeline oluşturma aşamasında `InvalidUniformLayout` hatası verilir.
- **Bağlama Grubu Uyuşmazlığı (`@group(0)`):** Beklenen `t_source` / `tex_a` / `tex_b` / `t_lut` / `params` bağlamaları eksik veya tip uyumsuzsa `ShaderBindingMismatch` döndürülür.
- **Yakıt Tüketimi / Derinlik Aşımı (`SafeExpr`):** Ayrıştırmada `ExprDepthExceeded` veya `ExprFuelExceeded` döndürülür; tanımsız değişken/fonksiyon `UnsupportedExprSymbol` hatasıyla reddedilir.
- **`NaN` / `Inf` Üreten İfade:** Kısıt dışı bölme/üs alma sonucu `is_finite() == false` üretirse `ExprNonFiniteResult` kaydedilir; motor bozuk değeri GPU uniform'una yazmaz.
- **Naga veya TDR Sınır İhlali:** Geçersiz WGSL `ShaderCompilationFailed`, döngü sınırı olmayan/aşan WGSL `UnsafeShaderLoopBound` ile reddedilir.

---

## Kabul Kriterleri (Kod Aşamasında Doğrulanacak)

- [ ] Tüm yerleşik ve özel WGSL uniform yapıları `size_of::<T>() % 16 == 0` ve `@group(0)` bağlama doğrulama testlerini geçer.
- [ ] `SafeExpr` ayrıştırıcısı derinliği `> 16`, yakıt maliyeti `> 256`, döngü/mutasyon içeren ifadeleri reddeder; `0`'a bölme ve `pow(-1.0, 0.5)` gibi girdilerde asla `NaN`/`Inf` üretmez.
- [ ] `SafeExpr` aynı `ExprContext` ile iki kez değerlendirildiğinde bit düzeyinde özdeş sonuç üretir.
- [ ] Naga doğrulayıcısı sözdizimi hatalı veya döngü üst sınırı olmayan test WGSL shader'larını `wgpu` pipeline'ına göndermeden reddeder.
