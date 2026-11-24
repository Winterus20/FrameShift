---
title: Pillar 1 — Yaratıcı Motor & İfade Gücü Entegrasyon Mimarisi (0 Yeni Crate)
date: 2026-11-24
status: implemented
tags: [timebase, shaders, render-graph, vector, schema, creative-engine]
---

# Pillar 1 — Yaratıcı Motor & İfade Gücü Entegrasyon Mimarisi

> **Uygulama Durumu (2026-11-24):** **0 yeni Cargo crate** ile implemente edildi ve doğrulandı ([uygulama günlüğü](../log/2026-11-24-competitive-power-roadmap-implementation.md)): `SafeExpr` (`spring`, `wiggle` SplitMix64, `%`, `< <= > >= == !=`, `&&`/`||`/`!`, kısa devreli `if`/`select`, `ceil`/`sqrt`/`log`/`mix`, `pow` NaN koruması), `NodeKind::{DisplacementMap, FrameEcho, ShapeMask}` + 3 yeni WGSL shader (`fx_displacement_map.wgsl`, `fx_frame_echo.wgsl`, `fx_shape_mask_feather.wgsl`) + `frameshift-vector::shape_mask`, ve ADR-0003 Kahn DAG zincirli `EventTimingConstraint` çözümleyicisi.

## Soru / Amaç

FrameShift kod tabanına **0 yeni Cargo crate** ekleyerek Pillar 1 kapsamındaki beş yaratıcı motor bileşeninin entegrasyon mimarisini çıkarmak:
1. **`SafeExpr` İfade Gücü:** `spring(t, stiffness, damping)`, seek-independent `wiggle(freq, amp, seed)` ve eksik dil operatörleri/fonksiyonları (`if/select`, `< <= > >= == !=`, `&& || !`, `%`, `mix`, `floor`, `ceil`, `sqrt`, `log`).
2. **`DisplacementMap` (2 Girdili UV Kaydırma):** `Effect2D` WGSL hattında luma/RG/polar modları, kenar sarma (`clamp`/`mirror`/`wrap`/`transparent`) ve premultiplied alpha koruması.
3. **`FrameEcho` / Ghost Trail:** Mevcut `MotionBlurShutter` / `OpticalFlowBlur` deseninde seek-independent çoklu geçmiş alt-örnek değerlendirmesi + renk kaymalı accumulation buffer.
4. **Keyframe'lenebilir Bézier / Polygon `ShapeMask`:** `frameshift-vector` (`lyon_tessellation` + `VectorCanvas`) ve `MaskMatte` entegrasyonu, feather/expand/invert ve vertex morph interpolasyonu.
5. **Göreli Zincirli Olay Kısıtları ([ADR-0003](../decisions/0003-semantic-event-constraint-timing.md)):** `EventAnchorDef` / `EventBoundAnimation` üzerinde DAG topolojik sıralama, döngü tespiti ve `i64` Flicks deterministik çözümleyici.

---

## İncelenen Birincil Kaynaklar (Erişim: 2026-11-24)

- [brain/specs/effect-shader-api.md](../specs/effect-shader-api.md) — `@group(0)` binding düzeni, 16-byte uniform kuralları, `SafeExpr` ve Naga/TDR sınırları.
- [brain/decisions/0003-semantic-event-constraint-timing.md](../decisions/0003-semantic-event-constraint-timing.md) — Olay/kısıt tabanlı zamanlama ve sessiz bozulma yasağı.
- [Cargo.toml](../../Cargo.toml), [frameshift-time/Cargo.toml](../../crates/frameshift-time/Cargo.toml), [frameshift-schema/Cargo.toml](../../crates/frameshift-schema/Cargo.toml), [frameshift-vector/Cargo.toml](../../crates/frameshift-vector/Cargo.toml), [frameshift-render/Cargo.toml](../../crates/frameshift-render/Cargo.toml) — mevcut bağımlılıklar (`serde`, `thiserror`, `bytemuck`, `glam`, `wgpu`, `lyon_tessellation`).
- [crates/frameshift-time/src/expr.rs](../../crates/frameshift-time/src/expr.rs) — `SafeExpr` tokenizer, parser, AST, evaluator ve bütçe sabitleri.
- [crates/frameshift-time/src/impact_solver.rs](../../crates/frameshift-time/src/impact_solver.rs) — `ImpactSolver`, `ConstraintViolation`.
- [crates/frameshift-schema/src/project.rs](../../crates/frameshift-schema/src/project.rs) — `EventAnchorDef`, `EventBoundAnimation`, `ParamBindingDef`, `bindable_param_names`, `Project::validate`.
- [crates/frameshift-schema/src/graph.rs](../../crates/frameshift-schema/src/graph.rs) — `NodeKind`, `MatteMode`, `expected_input_count`, `ALL_SERDE_NAMES`.
- [crates/frameshift-schema/src/session.rs](../../crates/frameshift-schema/src/session.rs) — `ProjectCommand` (`MoveEventTarget`, `RetargetEvent`, `UpsertEventAnchor`).
- [crates/frameshift-schema/src/version.rs](../../crates/frameshift-schema/src/version.rs) — `SchemaVersion` (`0.2.0`) uyumluluk kuralı (ADR-0004).
- [crates/frameshift-render/src/compositor.rs](../../crates/frameshift-render/src/compositor.rs) — `ProjectAnimator`, `evaluate_subgraph_inner`, `evaluate_motion_blur`, `evaluate_optical_flow_blur`, `VectorPathHud` eval, `record_two_tex_pass`, `collect_direct_cone`.
- [crates/frameshift-render/src/pipelines.rs](../../crates/frameshift-render/src/pipelines.rs) — `ShaderPipelines`, `build_program`, `build_bgl`.
- [crates/frameshift-render/src/uniforms.rs](../../crates/frameshift-render/src/uniforms.rs) — `MaskMatteUniform`, `MotionBlurUniform`, `BlurMoCurvesUniform`.
- [crates/frameshift-render/shaders/mask_matte.wgsl](../../crates/frameshift-render/shaders/mask_matte.wgsl), [motion_blur_accumulate.wgsl](../../crates/frameshift-render/shaders/motion_blur_accumulate.wgsl), [grid_warp.wgsl](../../crates/frameshift-render/shaders/grid_warp.wgsl), [fx_blur_mocurves.wgsl](../../crates/frameshift-render/shaders/fx_blur_mocurves.wgsl), [transition_glitch_displace.wgsl](../../crates/frameshift-render/shaders/transition_glitch_displace.wgsl).
- [crates/frameshift-vector/src/hud_path.rs](../../crates/frameshift-vector/src/hud_path.rs) & [canvas.rs](../../crates/frameshift-vector/src/canvas.rs) — `VectorPathHudConfig`, `build_hud_lyon_path`, `flatten_hud_subpaths`, `VectorCanvas::fill_contours_nonzero`.
- [crates/frameshift-core/src/resolver.rs](../../crates/frameshift-core/src/resolver.rs) — `resolve_audio_clip_bounds`, `node_has_dynamic_binding`, `VectorPathHud` render.

---

## 0. Sıfır Yeni Crate Fizibilitesi

Tüm 5 özellik mevcut workspace bağımlılıklarıyla (**0 yeni crate**) uygulanabilir:
- **Matematik / AST / DAG:** `std`, `serde`, `thiserror` (`frameshift-time`, `frameshift-schema`).
- **GPU Kompozisyon / Shader:** `wgpu 30.0.1`, `bytemuck 1.25.2`, `glam 0.29`, `half 2.7.1` (`frameshift-render`).
- **2D Vektör / Bézier Flattening / Rasterizasyon:** `lyon_tessellation 1.0`, `VectorCanvas` (`frameshift-vector`).

---

## 1. `SafeExpr` İfade Gücü Genişlemesi

### 1.1 Doğrudan Gözlemler (Kanıtlı)
- [expr.rs:5-11](../../crates/frameshift-time/src/expr.rs#L5-L11): `DEFAULT_MAX_FUEL = 256`, `MAX_EXPR_DEPTH = 64`, `MAX_EXPR_NODES = 1024`. *(Not: [effect-shader-api.md:72](../specs/effect-shader-api.md#L72) derinlik sınırını `16` olarak belgeliyor ancak kodda `64` tanımlı — dokümantasyon/spec hizalaması yapılmalı.)*
- [expr.rs:52-64](../../crates/frameshift-time/src/expr.rs#L52-L64): `ExprContext` içinde `t_sec`, `output_tick`, `beat_phase`, `audio_rms`, `audio_sub`, `audio_mid`, `audio_high`, `downbeat_phase`, `event_delta_sec`, `progress` mevcut.
- [expr.rs:115-194](../../crates/frameshift-time/src/expr.rs#L115-L194) & [452-462](../../crates/frameshift-time/src/expr.rs#L452-L462): `Token` yalnızca `+ - * / ( ) ,` destekliyor. `%`, `<`, `<=`, `>`, `>=`, `==`, `!=`, `&&`, `||`, `!` tokenları yok. `BuiltinFunc` içinde `Floor` zaten var; **`Ceil`, `Sqrt`, `Log`, `Mix`, `Select`/`If`, `Spring`, `Wiggle`** eksik.
- [expr.rs:435](../../crates/frameshift-time/src/expr.rs#L435): `BuiltinFunc::Pow` doğrudan `vals[0].powf(vals[1])` çağırıyor; [effect-shader-api.md:79](../specs/effect-shader-api.md#L79) ile öngörülen negatif `base` + kesirli `exp` koruması henüz yok (`NonFiniteValue` fırlatıyor).

### 1.2 Tasarım: Tokenizer, AST Öncelik Merdiveni ve Kısa Devre (Short-Circuit)

**Yeni `Token` Varyantları:**
`Percent` (`%`), `Lt` (`<`), `Le` (`<=`), `Gt` (`>`), `Ge` (`>=`), `EqEq` (`==`), `NotEq` (`!=`), `AndAnd` (`&&`), `OrOr` (`||`), `Bang` (`!`). *(Tek başına `=` veya `&` / `|` gelirse açık `ExprError::SyntaxError` döner.)*

**Operatör Öncelik Merdiveni (Düşükten Yükseğe):**
1. `parse_or`: `||` (sol birleşimli, kısa devreli)
2. `parse_and`: `&&` (sol birleşimli, kısa devreli)
3. `parse_eq`: `==`, `!=` (`abs(a - b) <= 1e-9` toleransı yerine IEEE `f64` eşitliği veya `approx`? Determinizm ve `0 == 0` için standart `f64` `==` / `!=`; her ikisi de sonlu olduğu için `NaN` yok)
4. `parse_cmp`: `<`, `<=`, `>`, `>=`
5. `parse_expr` (`+`, `-`)
6. `parse_term` (`*`, `/`, `%`) — `%` `BuiltinFunc::Mod` ile aynı güvenli öklid/fmod kuralı (`b == 0.0 -> DivisionByZero`)
7. `parse_unary` (`-`, `+`, `!`) — `!x` sonucu `if x == 0.0 { 1.0 } else { 0.0 }`
8. `parse_primary`

**Yeni `ExprNode` ve Kısa Devre:**
```rust
enum UnaryOp { Neg, Pos, Not }
enum BinaryOp { Add, Sub, Mul, Div, Mod, Lt, Le, Gt, Ge, Eq, Ne }
enum LogicalOp { And, Or }

enum ExprNode {
    Literal(f64),
    Var(ContextVar),
    Unary { op: UnaryOp, expr: Box<ExprNode> },
    Binary { op: BinaryOp, lhs: Box<ExprNode>, rhs: Box<ExprNode> },
    Logical { op: LogicalOp, lhs: Box<ExprNode>, rhs: Box<ExprNode> },
    Cond { cond: Box<ExprNode>, then_branch: Box<ExprNode>, else_branch: Box<ExprNode> },
    Call { func: BuiltinFunc, args: Vec<ExprNode> },
}
```
- `if(cond, a, b)` ve `select(cond, a, b)` ayrıştırma sırasında doğrudan `ExprNode::Cond` düğümüne derlenir (`if` anahtar kelimesi fonksiyon sözdizimiyle kabul edilir; değişken adı olarak yasaklı kalır).
- **Kısa devre garantisi:** `Cond` yalnızca seçilen dalı değerlendirir (`select(t > 0.0, 1.0 / t, 0.0)` sıfıra bölme üretmez); `Logical::And` `lhs == 0.0` ise `0.0`, `Logical::Or` `lhs != 0.0` ise `1.0` döndürür ve sağ dal fuel tüketmez.

### 1.3 Eksik Fonksiyonlar, `spring` ve `wiggle` Tasarımı

1. **`ceil(x)`**: `x.ceil()`
2. **`sqrt(x)`**: `x.max(0.0).sqrt()` ([effect-shader-api.md:80](../specs/effect-shader-api.md#L80) tanım kümesi koruması ile birebir uyumlu; negatif girdide `NaN` yerine `0.0`).
3. **`log(x)`**: `x.max(1e-6).ln()` (doğal logaritma; `0` veya negatif girdide `-Inf`/`NaN` yerine güvenli alt sınır).
4. **`mix(a, b, t)`**: `BuiltinFunc::Lerp` ile özdeş (`a + (b - a) * t`).
5. **`pow(base, exp)`**: `if base < 0.0 && exp.fract() != 0.0 { 0.0 } else { base.powf(exp) }`, ardından `ensure_finite`.

6. **`spring(t, stiffness, damping)` — Analitik Sönümlü Harmonik Salınıcı (Seek-Independent):**
   - `t <= 0.0` için `0.0`; `t >= 30.0` için hızlı oturma (`1.0`).
   - Parametreler: $\omega_0 = \sqrt{\text{stiffness.clamp}(0.01, 10000.0)}$, boyutsuz sönüm oranı $\zeta = \text{damping.clamp}(0.0, 10.0)$. *(Birim basamak tepkisi: $x(0)=0, x'(0)=0, \lim_{t\to\infty} x(t) = 1.0$.)*
   ```rust
   fn eval_spring(t: f64, stiffness: f64, damping: f64) -> f64 {
       if t <= 0.0 { return 0.0; }
       let omega0 = stiffness.clamp(0.01, 10_000.0).sqrt();
       let zeta = damping.clamp(0.0, 10.0);
       let env = (-zeta * omega0 * t).exp();
       if zeta < 1.0 - 1e-5 {
           let wd = omega0 * (1.0 - zeta * zeta).sqrt();
           let c = zeta / (1.0 - zeta * zeta).sqrt();
           1.0 - env * ((wd * t).cos() + c * (wd * t).sin())
       } else if zeta <= 1.0 + 1e-5 {
           1.0 - env * (1.0 + omega0 * t)
       } else {
           let r = (zeta * zeta - 1.0).sqrt();
           let r1 = -omega0 * (zeta - r);
           let r2 = -omega0 * (zeta + r);
           let c1 = r2 / (r2 - r1);
           let c2 = -r1 / (r2 - r1);
           1.0 - (c1 * (r1 * t).exp() + c2 * (r2 * t).exp())
       }
   }
   ```
   - Durumsuz (stateless), kapalı form olduğundan kare 500'e zıplamakla 0..500 arası taramak bit düzeyinde özdeş sonuç üretir.

7. **`wiggle(freq, amp, seed)` — Seek-Independent 1D Gradient (Perlin) Noise:**
   - Girdi zamanı: `u = ctx.t_sec * freq.max(0.0)`. (Ayrıca istenirse 4 argümanlı `wiggle_at(t, freq, amp, seed)` veya `event_delta_sec` ile kullanılabilen varyant.)
   - Deterministik SplitMix64 hash -> $[-1.0, 1.0]$ gradyan:
   ```rust
   #[inline]
   fn splitmix64(mut x: u64) -> u64 {
       x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
       x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
       x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
       x ^ (x >> 31)
   }
   #[inline]
   fn grad_1d(cell: i64, seed: u64) -> f64 {
       let h = splitmix64((cell as u64).wrapping_mul(0xd6e8_feb8_6659_fd93) ^ seed);
       let unit = ((h >> 11) as f64) / ((1u64 << 53) as f64); // [0, 1)
       unit * 2.0 - 1.0
   }
   fn eval_wiggle(t_sec: f64, freq: f64, amp: f64, seed: f64) -> f64 {
       if !freq.is_finite() || !amp.is_finite() || freq <= 0.0 || amp == 0.0 {
           return 0.0;
       }
       let u = t_sec * freq;
       let i0 = u.floor() as i64;
       let f = u - (i0 as f64);
       let s = f * f * f * (f * (f * 6.0 - 15.0) + 10.0); // quintic fade
       let seed_u64 = (seed.round() as i64) as u64;
       let n0 = grad_1d(i0, seed_u64) * f;
       let n1 = grad_1d(i0.wrapping_add(1), seed_u64) * (f - 1.0);
       (n0 + (n1 - n0) * s) * 2.0 * amp
   }
   ```
   - 1D Perlin'in tepe değeri $\pm 0.5$ olduğundan `* 2.0 * amp` ölçeklemesi tam `[-amp, +amp]` bandında $C^2$ sürekli, seed'li, seek-independent salınım üretir.

### 1.4 Fuel Bütçesi ve Değişecek Dosyalar
- `DEFAULT_MAX_FUEL = 256` sabit kalır; `spring` ve `wiggle` gibi ağır fonksiyonlar düğüm başına ek `3` fuel tüketir (normal `Call` 1 + 3 = 4 fuel). `MAX_EXPR_DEPTH` spec/kod farkı (`16` vs `64`) `expr.rs` lehine `64` olarak spec'e not edilir.
- **Değişecek Dosyalar:** `crates/frameshift-time/src/expr.rs`, `crates/frameshift-time/tests/expr_tests.rs`, `brain/specs/effect-shader-api.md` (yalnızca `MAX_EXPR_DEPTH` ve `wiggle`/`spring` belgelenirse).

---

## 2. `DisplacementMap` (2 Girdili UV Kaydırma)

### 2.1 Doğrudan Gözlemler (Kanıtlı)
- [effect-shader-api.md:37](../specs/effect-shader-api.md#L37): `Effect2D` sözleşmesi: `@binding(0) tex_a`, `@binding(1) tex_b`, `@binding(2) s_linear`, `@binding(3) params: var<uniform>`.
- [pipelines.rs:228-236](../../crates/frameshift-render/src/pipelines.rs#L228-L236) & [1280-1303](../../crates/frameshift-render/src/pipelines.rs#L1280-L1303): `build_program(device, name, src, "fs_main", 2, Some(size_non_zero::<U>()), WORKING_TEXTURE_FORMAT)` 2-girdili pipeline'ı otomatik kuruyor.
- [compositor.rs:6538-6575](../../crates/frameshift-render/src/compositor.rs#L6538-L6575): `self.record_two_tex_pass(...)` doğrudan iki `TextureView`, `linear_sampler` ve `Pod` uniform'u bağlıyor.
- [grid_warp.wgsl:139-147](../../crates/frameshift-render/shaders/grid_warp.wgsl#L139-L147) & [fx_blur_mocurves.wgsl:42-50](../../crates/frameshift-render/shaders/fx_blur_mocurves.wgsl#L42-L50): `dpdx`/`dpdy` + `textureSampleGrad` ile UV süreksizliği olmayan wrap ve premultiplied sınır temizliği zaten proje kalıbı olarak kullanılıyor.

### 2.2 Rust Şema ve Uniform Tasarımı

**`crates/frameshift-schema/src/graph.rs`:**
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplacementChannelMode {
    #[default]
    Rg,        // R -> dX, G -> dY
    Luma,      // Rec.709 luma -> yön vektörü (angle_rad) boyunca itme
    Polar,     // R -> açı [0..2pi], G -> yarıçap genliği
    SignedRg,  // R, G doğrudan [-1, 1] signed (neutral_level = 0.0, optical-flow / normal map uyumlu)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplacementEdgeMode {
    #[default]
    Clamp,
    Mirror,
    Wrap,
    Transparent,
}

// NodeKind içinde (arity = 2: inputs[0] = source, inputs[1] = displacement map)
#[serde(alias = "displacement_map")]
DisplacementMap {
    #[serde(default)]
    channel_mode: DisplacementChannelMode,
    #[serde(default)]
    edge_mode: DisplacementEdgeMode,
    #[serde(default = "default_disp_scale")] // 0.05
    scale_x: f32,
    #[serde(default = "default_disp_scale")] // 0.05
    scale_y: f32,
    #[serde(default = "default_half")] // 0.5 (SignedRg hariç)
    neutral_level: f32,
    #[serde(default)]
    angle_rad: f32,        // Luma modunda itme açısı
    #[serde(default)]
    chromatic_split: f32,  // R/G/B kanal ayrışması [0.0..1.0]
    #[serde(default = "default_one")]
    opacity: f32,
}
```
`bindable_param_names`: `&["scale_x", "scale_y", "neutral_level", "angle_rad", "chromatic_split", "opacity"]`.

**`crates/frameshift-render/src/uniforms.rs` (32 byte, `16`'nın tam katı):**
```rust
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct DisplacementMapUniform {
    /// x = channel_mode (0=Rg, 1=Luma, 2=Polar, 3=SignedRg),
    /// y = edge_mode (0=Clamp, 1=Mirror, 2=Wrap, 3=Transparent), z,w = reserved
    pub modes: [u32; 4],
    /// x = scale_x, y = scale_y, z = neutral_level, w = angle_rad
    pub scale_bias: [f32; 4],
    /// x = chromatic_split, y = opacity, z = aspect_ratio, w = reserved
    pub fx_params: [f32; 4],
}
// size_of::<DisplacementMapUniform>() == 48 (48 % 16 == 0)
```

### 2.3 WGSL Tasarımı ve Premultiplied Alpha Koruması (`shaders/fx_displacement_map.wgsl`)

1. **Map Okuma & Alpha Ağırlıklandırma:**
   `Rgba16Float` harita dokusu **premultiplied alpha** taşıdığından, yumuşak kenarlı veya saydam bir maske/partikül harita olarak bağlandığında doğrudan `map.rgb - neutral` almak saydam pikselde `-0.5` sahte kaydırma yaratır! Çözüm:
   - `let map_px = textureSampleLevel(t_map, s_linear, in.uv, 0.0);`
   - `let map_a = clamp(map_px.a, 0.0, 1.0);`
   - `let straight_rgb = select(vec3<f32>(params.scale_bias.z), map_px.rgb / max(map_a, 1e-5), map_a > 1e-5);`
   - `SignedRg` modunda (`mode == 3u`): `raw = map_px.rg;` (signed flow/normal doğrudan `map_a` ile ölçeklenir); diğer modlarda `centered = (straight_rgb - vec3<f32>(neutral)) * map_a;`.
2. **Mod Hesabı:**
   - `Rg (0u)`: `delta = centered.rg`
   - `Luma (1u)`: `luma = dot(straight_rgb, vec3<f32>(0.2126, 0.7152, 0.0722)) - neutral; delta = vec2<f32>(cos(angle), sin(angle)) * (luma * map_a)`
   - `Polar (2u)`: `theta = straight_rgb.r * 6.2831853 + angle; rad = (straight_rgb.g - neutral) * map_a; delta = vec2<f32>(cos(theta), sin(theta)) * rad`
   - Aspect düzeltme: `offset = vec2<f32>(delta.x * scale_x / max(aspect, 1e-4), delta.y * scale_y)`
3. **Kenar Sarma & Premultiplied Kaynak Örnekleme:**
   - `duv_dx = dpdx(in.uv)`, `duv_dy = dpdy(in.uv)` (veya `src_uv` türevleri; wrap sınırında seam piksel çamurlanmasını önlemek için `in.uv` türevleri veya `textureSampleLevel` tercih edilir; yüksek frekanslı displacement'ta `textureSampleGrad(..., dpdx(src_uv), dpdy(src_uv))` yalnızca sürekli bölgelerde güvenlidir, en temiz sonuç `Wrap`/`Mirror` modunda `in.uv` veya `textureSampleLevel(..., 0.0)` kullanmaktır!).
   - `sample_edge(uv, edge_mode)`:
     - `0u (Clamp)`: `clamp(uv, 0..1)`
     - `1u (Mirror)`: `1.0 - abs(fract(uv * 0.5) * 2.0 - 1.0)`
     - `2u (Wrap)`: `fract(uv)`
     - `3u (Transparent)`: `uv` `[0,1]` dışındaysa `vec4<f32>(0.0)` (tam şeffaf premultiplied siyah).
   - **Chromatic Split:** `abs(chromatic_split) > 1e-5` ise R, G, B kanalları `offset * (1.0 + k)`, `offset`, `offset * (1.0 - k)` ile ayrı örneklenir; alfa kanalı G (merkez) örneğinden veya 3 kanal ortalamasından alınır ve RGB'nin premultiplied sınırını bozmaması için her kanal kendi alpha oranıyla veya `straight` -> `re-premultiply` ile birleştirilir:
     - *Hassas nokta:* `R_sample.r` zaten `R_sample.a` ile çarpılıdır! R ve B farklı UV'lerden geliyorsa farklı alpha'ya sahip olabilir. En doğru premultiplied birleştirme: her kanal örneği için `alpha_i` ağırlıklı toplam alıp `out_a = (sr.a + sg.a + sb.a) / 3.0`, renkleri `sr.r, sg.g, sb.b` olarak yazmak (saydam arka plan üzerinde renk sızıntısı yapmaz!).
   - **Wet/Dry Mix:** `let orig = textureSampleLevel(t_source, s_linear, in.uv, 0.0); return mix(orig, warped, opacity);` — lineer premultiplied uzayda doğrudan doğrudur.

### 2.4 Değişecek Dosyalar
`crates/frameshift-schema/src/graph.rs`, `project.rs`, `crates/frameshift-render/shaders/fx_displacement_map.wgsl` (yeni), `crates/frameshift-render/src/uniforms.rs`, `pipelines.rs`, `compositor.rs`, `crates/frameshift-app/src/snapshot.rs`.

---

## 3. `FrameEcho` / Ghost Trail

### 3.1 Doğrudan Gözlemler (Kanıtlı)
- [compositor.rs:1731-1774](../../crates/frameshift-render/src/compositor.rs#L1731-L1774), [1848-1920](../../crates/frameshift-render/src/compositor.rs#L1848-L1920), [2020-2023](../../crates/frameshift-render/src/compositor.rs#L2020-L2023), [2113-2133](../../crates/frameshift-render/src/compositor.rs#L2113-L2133), [3867-3872](../../crates/frameshift-render/src/compositor.rs#L3867-L3872), [7453-7459](../../crates/frameshift-render/src/compositor.rs#L7453-L7459): `MotionBlurShutter` ve `OpticalFlowBlur` sıradan tek-pass düğüm değildir; üst akışlarını birden fazla `sub_tick` anında `evaluate_subgraph_inner` ile kendileri değerlendirirler. Bu nedenle `collect_direct_cone` bu düğümlerde durur ve `release_inputs` onlar için atlanır.
- [compositor.rs:2140-2230](../../crates/frameshift-render/src/compositor.rs#L2140-L2230): `evaluate_motion_blur`, `sample_ticks` üzerinde dönüp `motion_blur_accumulate` ile `PooledTexture` ping-pong birikimi yapar ve her örnek sonrası `self.pool.release(sample_tex)` çağırır.
- [compositor.rs:418-420](../../crates/frameshift-render/src/compositor.rs#L418-L420): `resolve_source_tick` klip aralığı dışındaki `sub_tick` değerlerinde `Ok(None)` döndürerek şeffaf `[0,0,0,0]` üretir — geçmişe taşan echo örnekleri için doğal koruma sağlar.

### 3.2 Seek-Independent Mimari Tasarımı (`NodeKind::FrameEcho`)

**Temel İlke:** Stateful ring-buffer (kare geçmişi önbelleği) rastgele erişimde (`frame`, paralel render, seek) bozulur. `FrameEcho`, `MotionBlurShutter` gibi **seek-independent çoklu zaman değerlendirmesi** kullanmalıdır.

**`crates/frameshift-schema/src/graph.rs`:**
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EchoOperator {
    #[default]
    CompositeUnder, // En yeni üstte, eski izler altta (premultiplied SourceOver)
    CompositeOver,  // Eski izler üstte
    Add,            // Lineer toplama (anime parlama izi)
    Screen,         // Soft ekran karışımı
    Maximum,        // Kanal bazlı max
}

#[serde(alias = "frame_echo")]
FrameEcho {
    /// Echo örnekleri arasındaki zaman adımı (negatif = geçmişe bakar; örn. -29_400_000 = -1/24s)
    #[serde(default = "default_echo_step_ticks")]
    echo_step_ticks: i64,
    /// Tekrar sayısı (0 = passthrough, max 24; render_options sample_ceiling ile kısıtlanabilir)
    #[serde(default = "default_echo_count")] // 5
    echo_count: u32,
    /// İlk (ana veya ilk echo) yoğunluğu [0..2]
    #[serde(default = "default_one")]
    starting_intensity: f32,
    /// Her echo adımında opaklık çarpanı [0..1]
    #[serde(default = "default_echo_decay")] // 0.65
    decay: f32,
    /// Her echo adımında RGB renk çarpanı (örn. [0.85, 0.55, 1.0] mor/mavi anime ghost izi)
    #[serde(default = "default_rgb_one")] // [1.0, 1.0, 1.0]
    color_decay_rgb: [f32; 3],
    /// Her echo adımında ek HSV/YIQ hue kayması (derece)
    #[serde(default)]
    hue_shift_deg: f32,
    #[serde(default)]
    operator: EchoOperator,
}
```
`bindable_param_names`: `&["starting_intensity", "decay", "hue_shift_deg", "echo_step_ticks"]`.
`SchemaError::InvalidFrameEcho`: `echo_count > 24` veya non-finite katsayılar reddedilir.

**Değerlendirme Sırası (`compositor.rs::evaluate_frame_echo`):**
1. `n = effective_echo_count(echo_count, sample_ceiling)` (0 ise sadece `tick` anı doğrudan döner).
2. `0..=n` arası `i` için `t_i = tick.0.saturating_add((i as i64).saturating_mul(echo_step_ticks))`.
   - `t_i < 0` ise ve `clamp_before_zero` aktifse atlanır (boş/şeffaf örnek).
3. Sıralama: `CompositeUnder` ve `CompositeOver` sıra duyarlıdır!
   - `CompositeUnder` (mevcut kare önde, geçmiş iz arkada): En eski örnekten (`i = n`) en yeniye (`i = 0`) doğru `SourceOver(accum, sample_i)` biriktirmek yerine, ping-pong sırasını `i = n` -> `0` kurmak **tek bir `SourceOver` operatörüyle** hem `CompositeUnder` hem `CompositeOver` (`i = 0` -> `n`) sağlar!
4. Renk kayması + ağırlık hesabı (CPU tarafında `i` için):
   - `weight_i = starting_intensity * decay.powi(i as i32)` (not: `i == 0` mevcut kare için ağırlık `1.0`, `color_tint = [1, 1, 1]` veya opsiyonel `keep_current_unmodified = true`).
   - `rgb_decay_i = [color_decay_rgb[0].powi(i), color_decay_rgb[1].powi(i), color_decay_rgb[2].powi(i)]`.
   - `hue_deg_i = hue_shift_deg * (i as f32)`.

**`FrameEchoUniform` (32 byte) & `shaders/fx_frame_echo.wgsl` (`Effect2D`):**
```rust
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct FrameEchoUniform {
    /// xyz = linear RGB tint multiplier, w = alpha/intensity weight
    pub tint_weight: [f32; 4],
    /// x = operator (0=Over, 1=Add, 2=Screen, 3=Max), y = is_first (0|1), z = hue_rad, w = reserved
    pub params: [f32; 4],
}
// size_of::<FrameEchoUniform>() == 32
```
*(Not: `hue_rad` WGSL'de Rodrigues/YIQ eksen dönüşü ile premultiplied RGB üzerinde uygulanır; alpha etkilenmez, ardından `s_tinted = vec4(rgb * tint * weight, a * weight)` ile premultiplied kuralı korunur.)*

**Performans / Önbellek Notu:** `FrameEcho` üst grafiği `N+1` kez değerlendirir. Ardışık kareler (`frame k`, `frame k+1`) render edilirken `frame k`'da zaten hesaplanmış `tick - frame_ticks` dokusunun tekrar değerlendirilmemesi için Pillar 3 Smart Cache (veya `FrameEcho` içi alt-grafik tick önbelleği) doğrudan katlanarak hızlanma sağlar — ancak önbellek olmadan bile doğruluk %100 seek-independent kalır.

### 3.3 Değişecek Dosyalar
`crates/frameshift-schema/src/graph.rs`, `error.rs`, `project.rs`, `crates/frameshift-render/shaders/fx_frame_echo.wgsl` (yeni), `crates/frameshift-render/src/uniforms.rs`, `pipelines.rs`, `compositor.rs` (`evaluate_subgraph_inner`, `collect_direct_cone`, `release_inputs`, `evaluate_disabled_node`), `crates/frameshift-app/src/snapshot.rs`.

---

## 4. Keyframe'lenebilir Bézier / Polygon `ShapeMask`

### 4.1 Doğrudan Gözlemler (Kanıtlı)
- [hud_path.rs:13-54](../../crates/frameshift-vector/src/hud_path.rs#L13-L54) & [288-341](../../crates/frameshift-vector/src/hud_path.rs#L288-L341): `VectorPathHudConfig` ve `build_hud_lyon_path` zaten `lyon_tessellation::path::Path` ile kübik Bézier + polygon konturları inşa ediyor ve `flatten_hud_subpaths` ile `0.25px` toleranslı alt-yollara düzleştiriyor.
- [canvas.rs:475-488](../../crates/frameshift-vector/src/canvas.rs#L475-L488): `VectorCanvas::fill_contours_nonzero` 4x4 sub-pixel supersampling ile kapalı çokgenleri premultiplied `Rgba16fFrame` olarak rasterize ediyor.
- [mask_matte.wgsl:90-136](../../crates/frameshift-render/shaders/mask_matte.wgsl#L90-L136): Mevcut `MaskMatte` düğümü 2 girdili (`t_source`, `t_mask`) çalışıyor; ancak `spatial_feather_px` tek-pass 3x3 çekirdekle `[0, 16]` piksel ile sınırlı (`blur_r * 0.65`). Geniş (ör. 40–100px) yumuşak maske kenarları için mevcut `MaskMatte` yetersiz kalıyor.

### 4.2 Mimari Tasarım: `ShapeMask` + `MaskMatte` Entegrasyonu

**Veri Modeli (`crates/frameshift-schema/src/graph.rs` & `frameshift-vector/src/shape_mask.rs`):**
```rust
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ShapeMaskVertex {
    pub pos: [f32; 2],         // Normalize [0..1] canvas koordinatı
    #[serde(default)]
    pub in_tangent: [f32; 2],  // pos'a göre GÖRELİ giriş kolu (0,0 = keskin köşe)
    #[serde(default)]
    pub out_tangent: [f32; 2], // pos'a göre GÖRELİ çıkış kolu (0,0 = keskin köşe)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShapeMaskKeyframe {
    pub tick: Flicks,
    pub vertices: Vec<ShapeMaskVertex>,
    #[serde(default)]
    pub easing: AnimationEasing,
}

#[serde(alias = "shape_mask")]
ShapeMask {
    #[serde(default)]
    pub vertices: Vec<ShapeMaskVertex>, // Keyframe yoksa statik yol
    #[serde(default)]
    pub keyframes: Vec<ShapeMaskKeyframe>, // tick'e göre sıralı
    #[serde(default = "default_true")]
    pub closed: bool,
    #[serde(default)]
    pub feather_px: f32,   // >= 0.0 (geniş yumuşatma destekli)
    #[serde(default)]
    pub expand_px: f32,    // + şişirme / - daraltma (choke)
    #[serde(default)]
    pub invert: bool,
    #[serde(default = "default_one")]
    pub opacity: f32,
}
```
*Arity:* `0` girdi (üretici düğüm: beyaz `Rgba16Float` maske üretir ve mevcut `MaskMatte` veya `Blend` `inputs[1]` yuvasına bağlanır — **bölüm 7'deki Karar 2'ye bakınız**).

**Keyframe Morph İnterpolasyon Semantiği (`frameshift-vector`):**
1. **Aynı Uzunlukta (`A.vertices.len() == B.vertices.len()`):**
   Her vertex `i` için `pos`, `in_tangent` ve `out_tangent` doğrudan `lerp(a, b, eased_t)` ile interpole edilir. Ardından `ShapeMaskVertex` dizisi `LyonPath::builder()` ile kübik Bézier (`p0 = v[i].pos`, `c1 = v[i].pos + v[i].out_tangent`, `c2 = v[next].pos + v[next].in_tangent`, `p1 = v[next].pos`) olarak inşa edilir.
2. **Farklı Uzunlukta (Deterministik Yay-Uzunluğu Yeniden Örnekleme):**
   - `A` ve `B` önce `lyon_tessellation` ile `0.25px` toleransta düzleştirilir.
   - Her iki kontur `N = (A.len().max(B.len()) * 4).clamp(64, 512)` eşit yay-uzunluğu noktasına örneklenir.
   - Burkulmayı önlemek için `B`'nin başlangıç indeksi, tüm döngüsel kaydırmalar arasında $\sum_k \|A_k - B_{(k+s) \bmod N}\|^2$ mesafesini minimize eden `s` seçilerek hizalanır (deterministik `O(N^2)` küçük `N <= 512` için < 0.2 ms).
   - Hizalanmış poligonlar nokta-bazlı lineer interpole edilir.

**Feather, Expand ve Invert Uygulaması:**
- **CPU (`VectorCanvas`) vs GPU (Separable Gaussian + SDF):**
  - Küçük `feather_px <= 16.0` ve `expand_px` mevcut `VectorCanvas::fill_contours_nonzero` -> 2-pass separable GPU Gaussian (`backdrop_scrim_blur` ile aynı 9-tap/downsampled ping-pong kalıbı!) veya `MaskMatte` üzerinden yapılabilir.
  - **Önerilen Hibrit Hat:**
    1. `frameshift-vector` yalnızca sert/anti-aliased konturu (`expand_px` varsa polygon offset / segment SDF ile) 4x4 SSAA ile rasterize eder -> `Rgba16Float` yüklenir.
    2. `feather_px > 0.0` ise GPU'da mevcut separable Gaussian blur pass'i (veya tek yeni `fx_mask_feather.wgsl` 2-pass H/V Gaussian + `invert`/`opacity` final pass) çalıştırılır — böylece `feather_px = 100.0` bile 4K'da < 0.3 ms'de pürüzsüz çalışır! CPU'da 100px 2D konvolüsyon darboğazı önlenir.
- **Önbellek:** Statik `keyframes.is_empty()` durumlarında mevcut `VectorPathHud` önbellek deseni ([resolver.rs:207](../../crates/frameshift-core/src/resolver.rs#L207)) gibi `ShapeMask` GPU dokusu `revision` + `params_hash` ile önbelleklenir; keyframe'li durumda yalnızca aktif `[t_k, t_{k+1}]` aralığındaki karelerde yeniden rasterize edilir (keyframe dışındaki sabit bölgeler de önbellekten gelir!).

### 4.3 Değişecek Dosyalar
`crates/frameshift-vector/src/shape_mask.rs` (yeni) & `lib.rs`, `crates/frameshift-schema/src/graph.rs`, `error.rs`, `project.rs`, `crates/frameshift-render/src/compositor.rs`, `pipelines.rs`, `shaders/fx_mask_feather.wgsl` (yeni veya `backdrop_scrim_blur.wgsl` ortak kullanımı), `crates/frameshift-app/src/snapshot.rs`.

---

## 5. Göreli Zincirli Olay Kısıtları (ADR-0003)

### 5.1 Doğrudan Gözlemler (Kanıtlı)
- [project.rs:271-281](../../crates/frameshift-schema/src/project.rs#L271-L281): `EventAnchorDef` şu an yalnızca mutlak `target_output_tick: Flicks` alanına sahip.
- [project.rs:73-84](../../crates/frameshift-schema/src/project.rs#L73-L84): `EventBoundAnimation` tek bir `event_id` ve `offset_from_event: Flicks` taşıyor; bir olayın başka bir olaya veya klip yüzdesine bağlanması yok.
- [session.rs:329-389](../../crates/frameshift-schema/src/session.rs#L329-L389): `MoveEventTarget` ve `RetargetEvent` sadece tekil olayın `target_output_tick` değerini güncelliyor; bağımlı olayları veya zincirli kısıtları yaymıyor (propagate etmiyor).
- [resolver.rs:761-776](../../crates/frameshift-core/src/resolver.rs#L761-L776): `resolve_audio_clip_bounds` `event.target_output_tick`'i doğrudan okuyor.

### 5.2 Tasarım: Rasyonel Kısıt Modeli ve DAG Çözümleyici

**Geriye Dönük Uyumlu Şema Genişlemesi (`crates/frameshift-schema/src/project.rs`):**
ADR-0004 (`schema_version 0.2.0`) uyarınca eski JSON dosyalarının kırılmaması için mevcut `target_output_tick: Flicks` alanı **çözümlenmiş (cached/resolved) çıktı tick'i** olarak kalır ve `#[serde(default)]` ile opsiyonel `timing` alanı eklenir:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClipTimingEdge {
    Start,
    End,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventTimingConstraint {
    /// Mevcut mutlak tick davranışı
    Absolute,
    /// Örn: "downbeat'ten -2 kare önce" -> parent_event_id + offset_ticks
    RelativeToEvent {
        parent_event_id: String,
        #[serde(default)]
        offset_ticks: Flicks,
    },
    /// Örn: "klip geçişinin %60'ında" -> start + ((end - start) * fraction_num) / fraction_den + offset_ticks
    /// Tam i128->i64 tamsayı rasyonel aritmetik (f64 yasak!).
    RelativeToClipFraction {
        clip_id: String,
        fraction_num: i64,
        fraction_den: i64, // > 0 zorunlu
        #[serde(default)]
        offset_ticks: Flicks,
    },
    /// Klip başlangıç veya bitişine göreli
    RelativeToClipEdge {
        clip_id: String,
        edge: ClipTimingEdge,
        #[serde(default)]
        offset_ticks: Flicks,
    },
}
```
*Ayrıca klip sınırları da zincirlenmek istenirse:* `ClipAnchorConstraint` (opsiyonel: `start_anchor_event_id` veya mevcut `EventAnchorDef` + `RelativeToClipFraction` tek yönlü tutulursa `Clip -> Event -> ChildEvent` DAG'ı her zaman düz ve çakışmasız kalır).

**DAG Topolojik Sıralama & Döngü Tespiti (`frameshift-time` veya `frameshift-schema`):**
*(Not: `EventAnchorDef` ve `ClipDef` tipleri `frameshift-schema` içinde tanımlı olduğundan, genel DAG trait/algoritması `frameshift-time/src/event_graph.rs` içinde saf `(&str, Option<&str>, i64)` girdileriyle yazılabilir veya doğrudan `frameshift-schema`'da çağrılabilir.)*

Algoritma (`resolve_event_chain`):
1. Her olay için bağımlılık kenarı çıkarılır: `RelativeToEvent { parent_event_id, .. }` -> `parent_event_id -> event_id`.
2. **Eksik referans kontrolü:** `parent_event_id` veya `clip_id` yoksa `SchemaError::MissingEvent` / `MissingClip` döner (sessiz fallback yasak).
3. **Kahn Topolojik Sıralama (`O(V + E)`):**
   - `in_degree == 0` olan düğümler (`Absolute`, `RelativeToClipFraction`, `RelativeToClipEdge`) kuyruğa alınır.
   - Kuyruktan çıkan her düğümün çocuklarının `in_degree`'si azaltılır.
   - İşlenen düğüm sayısı `< V` ise kalan düğümler üzerinde deterministik DFS ile tam döngü yolu (`A -> B -> C -> A`) çıkarılır ve `SchemaError::CyclicEventConstraint { cycle }` fırlatılır.
4. **Tam `i64` Flicks Hesabı (`i128` ara aritmetik):**
   ```rust
   // RelativeToClipFraction:
   let span = (clip.timeline_end.0 as i128) - (clip.timeline_start.0 as i128);
   let num = span.checked_mul(fraction_num as i128)
       .ok_or(SchemaError::EventTimingOverflow { ... })?;
   // En yakın tam Flicks'e deterministik yuvarlama (round-half-away-from-zero veya floor):
   let scaled = div_round_i128(num, fraction_den as i128);
   let resolved_i128 = (clip.timeline_start.0 as i128) + scaled + (offset_ticks.0 as i128);
   let resolved = i64::try_from(resolved_i128)
       .map(Flicks)
       .map_err(|_| SchemaError::EventTimingOverflow { ... })?;
   ```
   - `fraction_den <= 0` -> `SchemaError::InvalidEventConstraint`.
5. **Entegrasyon Noktaları:**
   - `Project::validate()` (`project.rs:800-810` & `953-960`): DAG doğrulaması yapar; `timing` varsa `resolved_tick == ev.target_output_tick` tutarlılığını kontrol eder veya `ProjectSession::commit` her `MoveEventTarget`, `RetargetEvent`, `TrimClip`, `SplitClip`, `UpsertEventAnchor` komutundan sonra `recompute_chained_events(project)` çalıştırarak tüm bağımlı `target_output_tick` değerlerini atomik günceller.
   - Böylece `ProjectAnimator` (`compositor.rs:298-385`), `resolve_audio_clip_bounds` (`resolver.rs:761-776`) ve `EventBoundAnimation` sıfır değişiklikle her zaman güncel çözülmüş tick'leri okur!
   - **Ek İyileştirme (`EventBoundAnimation`):** Mevcut `EventBoundAnimation` (`project.rs:73-84`) zaten `offset_from_event: Flicks` destekliyor; ancak `"animasyon bir sonraki olayda bitsin"` veya `"klip süresinin %20'si sürsün"` senaryoları için opsiyonel `end_event_id: Option<String>` / `end_offset_from_event: Option<Flicks>` alanı da aynı çözümleyiciye eklenebilir.

### 5.3 Değişecek Dosyalar
`crates/frameshift-time/src/event_graph.rs` (yeni) & `lib.rs`, `crates/frameshift-schema/src/project.rs`, `error.rs`, `session.rs` (`MoveEventTarget`, `RetargetEvent`, `TrimClip`, `SplitClip`, `UpsertEventAnchor` sonrası propagasyon).

---

## Riskler ve Doğrulanmamış Noktalar

1. **`MAX_EXPR_DEPTH` Spec vs Kod Çelişkisi:** [effect-shader-api.md:72](../specs/effect-shader-api.md#L72) `16` derken [expr.rs:8](../../crates/frameshift-time/src/expr.rs#L8) `64` tanımlamaktadır. Yeni ikili karşılaştırma/mantıksal merdiven parse derinliğini artıracağı için `64` korunmalıdır (16 seçilirse orta karmaşıklıktaki `if(a && b, ...)` ifadeleri reddedilebilir).
2. **`FrameEcho` Performans Maliyeti:** `echo_count = 10` seçildiğinde üst grafik 11 kez değerlendirilir; eğer üst grafikte `OpticalFlowBlur` veya `Scene3D` varsa kare süresi doğrusal olarak ~11× artar. Pillar 3 Smart Cache gelene kadar `sample_ceiling` ve `MAX_ECHO_COUNT = 24` sert tavanı şarttır.
3. **Naga / TDR Sınırları (`DisplacementMap` & `FrameEcho`):** Her iki yeni WGSL shader da `loop`/`while` içermez; yalnızca sabit ya da tek adımlı `effect2d` fragment örneklemesi yapar, bu nedenle TDR riski sıfıra yakındır (Naga doğrulaması `ShaderPipelines::compile_all` testlerinde otomatik doğrulanacaktır).
4. **Doğrulanmamış Noktalar:** Tüm tasarım çıkarımları mevcut kod incelemesine dayalıdır; derleme, `cargo test --workspace` ve görsel golden doğrulamaları uygulama aşamasında yapılacaktır.

---

## Kullanıcıya Sorulacak Net Karar Seçenekleri

1. **`ShapeMask` Düğüm Topolojisi (`0-girdi Üretici` vs `1-girdi Inline Maske`):**
   - **Seçenek A *(Önerilen)* — 0 Girdili Üretici (`ShapeMask` -> `MaskMatte` / `Blend`):** Mevcut `MaskMatte` (`choke_px`, `invert`, `Alpha/Luma`, `DepthMatte`) ve `Transform2D` ile tam birleşir; aynı şekil maskesi birden fazla katmana/efekte (`ColorBasic`, `DeepGlow`, `Blend`) paylaşılabilir.
   - **Seçenek B — 1 Girdili Inline Maske (`Source -> ShapeMask`):** Tek düğümde doğrudan girdiyi keser; ajan JSON'unda 1 düğüm daha az yazar ama maskeyi başka bir düğümle paylaşmayı zorlaştırır.

2. **`SafeExpr::wiggle` Zaman Kaynağı ve `sqrt`/`log` Hata Politikası:**
   - **Seçenek A *(Önerilen)* — `wiggle(freq, amp, seed)` örtük `t_sec` kullansın + `sqrt`/`log` [effect-shader-api.md:80](../specs/effect-shader-api.md#L80) uyarınca clamp'lensin (`sqrt(max(x,0))`, `log(max(x,1e-6))`):** Mevcut kilitli sözleşmeyle tam uyumlu; ifadeler kısa kalır (`wiggle(4.0, 0.02, 42)`). İhtiyaç halinde opsiyonel 4 argümanlı `wiggle_at(t, freq, amp, seed)` da eklenebilir.
   - **Seçenek B — Açık zaman parametreli `wiggle(t, freq, amp, seed)` zorunlu + negatif `sqrt`/`log`'da sert hata (`InvalidArgument`):** İfadeyi uzatır (`wiggle(event_delta_sec, ...)`), ancak ses/olay zamanına kaydırma gerektiğinde daha açıktır.
