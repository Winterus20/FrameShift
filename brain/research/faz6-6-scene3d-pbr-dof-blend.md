---
title: "Faz 6.6 — Scene3D PBR Cook-Torrance IBL, Particle/Ribbon DoF İzolasyonu, 2-Pass Separable CoC Dilation, Sinematik Bokeh & Algısal Blend Modları Şartnamesi"
date: 2026-11-22
status: active
tags: [faz6-6, scene3d, pbr, ibl, dof, bokeh, blend, wgsl, wgpu, shaders]
---

# Faz 6.6 — `Scene3D` PBR/IBL, Parçacık-Şerit DoF İzolasyonu, 2-Pass Separable CoC Dilation, Poligonal/Anamorfik/Cat's-Eye Bokeh ve Algısal `Blend` Şartnamesi

## Soru / Amaç

FrameShift Faz 6.3–6.5 sonrasında 3D sahne motoru (`Scene3D`, `Text3D`, `Shape3D`, `ParticleField3D`, `RibbonTrail3D`), Z-buffer Bokeh DoF (`fx_dof_coc.wgsl`, `fx_dof_bokeh.wgsl`) ve 2D katman birleştirme (`blend.wgsl`) çalışır durumdadır; ancak prodüksiyon editlerinde dört temel görsel ve mimari kusur gözlemlenmiştir:

1. **(a) `text3d_mesh.wgsl` Materyal & Derinlik Kusurları:** Mevcut ışıklandırma Blinn-Phong (`pow(ndoth, 64.0)`) kullanır ve dünya uzayında sabit `key_dir = normalize(vec3(0.55, 0.80, 0.90))` yüzünden 90° kamera/harf dönüşlerinde harf ön yüzleri kararır. Bevel/yan duvar çukurlarında cavity AO yoktur. Yarı-şeffaf Z-echo katmanları (`echo_layer >= 1u`) opak kahramanla aynı `depth_write_enabled: Some(true)` pipeline'ını paylaştığı için derinlik tamponunu kirletir.
2. **(b) `Scene3D` İçinde `ribbon_trail3d` ve `particle_field3d`'nin `fx_dof_coc` ile Bozulması:** `ribbon_trail3d_scene` ve `particle_field3d_scene` `depth_write_enabled: Some(false)` ile `color_tex` üzerine çizilirken `depth_tex` boş arka planda `1.0` kalır; `fx_dof_coc.wgsl` `alpha > 1e-4` gördüğü şerit/parçacık piksellerini `z_view = far (200.0)` sanarak maksimum yarıçapla bozar veya arkadaki opak objenin CoC'unu üstüne uygular.
3. **(c) `fx_dof_coc.wgsl` Seyrek 5×5 Dilation & `fx_dof_bokeh.wgsl` Eksikleri:** Mevcut `5×5` seyrek örnekleme (`step_px = ceil(max_rad * 0.5)`) geniş `max_rad` değerlerinde ince neon harfleri ve ince kenarları atlar. `fx_dof_bokeh.wgsl` yalnızca dairesel Vogel diski çizer (poligonal bıçak, anamorfik oval, cat's-eye optik vinyet yoktur), ayrıca ön-plan birleştirmesinde `mix(bg_color, fg_color, fg_alpha_cov)` formülü yarı-şeffaf ön-planı opak değiştirme gibi ele alır ve arka plan toplamasında `center_coc >= 0` iken ön-plan sızıntısı (`depth_ok`) guard'ı eksiktir.
4. **(d) `blend.wgsl` Renk Uzayı & Eksik Modlar:** `Overlay` modu `b <= 0.5` eşiğini doğrudan `LinearRec709` uzayında uyguladığı için sRGB algısal orta griyi (`~0.214` linear) yanlış böler ve orta tonları ezer. Ayrıca `SoftLight`, `ColorDodge`, `HardLight`, `Lighten` ve `Darken` modları eksiktir.

Bu belge **sıfır yeni Cargo crate** kısıtı altında, M1 golden hash'lerini ve mevcut tüm testleri (`187/187`) bozmadan Faz 2 uygulamasına doğrudan geçirilebilecek kesin WGSL formüllerini, Rust `#[repr(C)]` uniform yapılarını (16-byte hizalı) ve pipeline değişikliklerini tanımlar.

---

## İncelenen Birincil Kaynaklar

### Harici Birincil Kaynaklar (Doğrulandı: `2026-11-22`)
1. **Google Filament — Physically Based Rendering & `brdf.fs` (`D_GGX`, `V_SmithGGXCorrelated`, `F_Schlick`, `computeSpecularAO`):**
   - [https://google.github.io/filament/Filament.html](https://google.github.io/filament/Filament.html)
   - [https://github.com/google/filament/blob/main/shaders/src/brdf.fs](https://github.com/google/filament/blob/main/shaders/src/brdf.fs)
   - Eric Heitz (2014), *"Understanding the Masking-Shadowing Function in Microfacet-Based BRDFs"* — height-correlated Smith-GGX `V = 0.5 / (GGXV + GGXL)`.
2. **Sebastien Lagarde & Brian Karis — Real Shading in Unreal Engine 4 / Analytic Split-Sum DFG (`EnvBRDFApprox`):**
   - [https://cdn2.unrealengine.com/Resources/files/2013SiggraphPresentationsNotes-26915738.pdf](https://cdn2.unrealengine.com/Resources/files/2013SiggraphPresentationsNotes-26915738.pdf)
3. **Jorge Jimenez (SIGGRAPH 2014 — *Next Generation Post Processing in Call of Duty: Advanced Warfare*):**
   - [https://www.iryoku.com/next-generation-post-processing-in-call-of-duty-advanced-warfare/](https://www.iryoku.com/next-generation-post-processing-in-call-of-duty-advanced-warfare/) — CoC dilation, near/far field separation, IGN jittered bokeh gather.
4. **Catlike Coding — *Depth of Field* & Godot Engine `bokeh_dof.glsl` (MIT):**
   - [https://catlikecoding.com/unity/tutorials/advanced-rendering/depth-of-field/](https://catlikecoding.com/unity/tutorials/advanced-rendering/depth-of-field/)
   - [https://github.com/godotengine/godot/blob/master/servers/rendering/renderer_rd/shaders/effects/bokeh_dof.glsl](https://github.com/godotengine/godot/blob/master/servers/rendering/renderer_rd/shaders/effects/bokeh_dof.glsl)
5. **W3C Compositing and Blending Level 1 (`overlay`, `soft-light`, `color-dodge`, `hard-light`, `lighten`, `darken` & Porter-Duff Source-Over):**
   - [https://www.w3.org/TR/compositing-1/](https://www.w3.org/TR/compositing-1/)
6. **W3C WGSL Specification & `wgpu 30.0.1`:**
   - [https://www.w3.org/TR/WGSL/](https://www.w3.org/TR/WGSL/)
   - [https://docs.rs/wgpu/30.0.1/wgpu/struct.DepthStencilState.html](https://docs.rs/wgpu/30.0.1/wgpu/struct.DepthStencilState.html)

### Yerel Kod Tabanı Kaynakları (Doğrudan İncelenen)
- [text3d_mesh.wgsl](../../crates/frameshift-render/shaders/text3d_mesh.wgsl) (satır 1–346)
- [fx_dof_coc.wgsl](../../crates/frameshift-render/shaders/fx_dof_coc.wgsl) (satır 1–93)
- [fx_dof_bokeh.wgsl](../../crates/frameshift-render/shaders/fx_dof_bokeh.wgsl) (satır 1–134)
- [blend.wgsl](../../crates/frameshift-render/shaders/blend.wgsl) (satır 1–102)
- [particle_field3d.wgsl](../../crates/frameshift-render/shaders/particle_field3d.wgsl) (satır 1–178)
- [ribbon_trail3d.wgsl](../../crates/frameshift-render/shaders/ribbon_trail3d.wgsl) (satır 1–255)
- [transform3d.wgsl](../../crates/frameshift-render/shaders/transform3d.wgsl) (satır 1–113)
- [pipelines.rs](../../crates/frameshift-render/src/pipelines.rs) (satır 396–550, 1080–1135)
- [compositor.rs](../../crates/frameshift-render/src/compositor.rs) (satır 2065–2080, 2694–2875, 2941–3443, 3978–4001, 4686–4927)
- [uniforms.rs](../../crates/frameshift-render/src/uniforms.rs) (satır 79–103, 586–710, 857–861)
- [mesh3d.rs](../../crates/frameshift-vector/src/mesh3d.rs) (satır 16–36, 451–960, 1016–1075)
- [graph.rs](../../crates/frameshift-schema/src/graph.rs) (satır 164–270, 340–346, 637–875)
- [project.rs](../../crates/frameshift-schema/src/project.rs) (satır 950–1071)
- Kilitli sözleşmeler: [render-graph.md](../specs/render-graph.md), [effect-shader-api.md](../specs/effect-shader-api.md), [project-schema.md](../specs/project-schema.md)

---

## Doğrudan Gözlemler (Kanıtlı)

1. **`text3d_mesh.wgsl` Işıklandırma & Derinlik Durumu:**
   - Satır `270`: `let key_dir = normalize(vec3<f32>(0.55, 0.80, 0.90));` dünya uzayında sabittir; kamera veya metin `Y` ekseninde `±90°` döndüğünde ön kapak (`+Z` veya `±X`) `NoL = 0` bölgesine düşer.
   - Satır `272–345`: `material_mode = 0u` (Neon3D), `1u` (MetallicChrome) ve `3u` (IridescentAnisotropic) Blinn-Phong `pow(ndoth, ...)` kullanır; `D_GGX`, `V_SmithGGXCorrelated` ve enerji korunumlu `(1 - F) * (1 - metallic)` difüz/speküler ayrımı yoktur.
   - `pipelines.rs:436–446`: `text3d_mesh` tek pipeline olarak `depth_write_enabled: Some(true)` kullanır. `text3d_mesh.wgsl:193–203` tüm `0..1+echo_count` instance'larını aynı draw call'da çizdiği için yarı-şeffaf Z-echo katmanları (`echo_layer >= 1u`) da `Depth32Float` tamponuna yazar.
   - `mesh3d.rs:1017, 1074`: Unit testler `v.face_kind == 3` tam eşitliğini kontrol eder; bu nedenle `GpuMeshVertex3D` (48 byte) düzeni ve `face_kind` (`0..=3`) tam sayı değerleri değiştirilmeden korunmalıdır.

2. **`Scene3D` + `ParticleField3D` / `RibbonTrail3D` + `fx_dof_coc.wgsl` Çakışması:**
   - `pipelines.rs:473–507`: `particle_field3d_scene` ve `ribbon_trail3d_scene` `depth_write_enabled: Some(false)`, `depth_compare: Some(Less)` ve `hdr_additive_premul_blend` kullanır.
   - `compositor.rs:3287–3307`: Phase B (`Ribbon`, `Particles`) aynı `color_tex` içine çizilir.
   - `fx_dof_coc.wgsl:58–67`: Boşluk kontrolü `if (raw_d >= 0.99999 && alpha <= 1e-4)` koşuluna bağlıdır. Şerit veya parçacık boş arka plan üzerinde `alpha > 1e-4` ürettiğinde `raw_d == 1.0` olduğundan `linearize_depth_rh(1.0, near, far) = far (200.0)` hesaplanır ve tüm şerit/parçacık pikselleri odak düzleminde olsalar bile `+max_rad` ile bulanıklaştırılır. Arkada bir `Text3D` varsa şerit/parçacık kendi derinliği yerine arkadaki `Text3D` derinliğini devralır.

3. **`fx_dof_coc.wgsl` & `fx_dof_bokeh.wgsl` Boşlukları:**
   - `fx_dof_coc.wgsl:78–90`: `step_px = max(i32(ceil(max_rad * 0.5)), 1)` ile sabit `5×5` ızgara kullanılır (`max_rad = 24` iken 12 piksel aralıklı adımlar!). `2–10 px` genişliğindeki ince neon yazılar örnekleme noktalarının arasında kaybolur.
   - `fx_dof_bokeh.wgsl:90–93`: Vogel disk örneklemesi saf daireseldir; bıçak poligonu, anamorfik oran veya cat's-eye vinyet yoktur.
   - `fx_dof_bokeh.wgsl:117`: `let depth_ok = select(smoothstep(center_z - 0.15, center_z + 0.05, s_z), 1.0, center_coc >= 0.0);` ifadesi `center_coc >= 0.0` (arka plan merkezi) durumunda tüm örnekler için `1.0` döndürür; dolayısıyla keskin veya ön-plandaki pikseller arka plan bokeh toplamına (`bg_acc`) sızar.
   - `fx_dof_bokeh.wgsl:132`: `mix(bg_color, fg_color, fg_alpha_cov)` yarı-şeffaf ön-plan (`fg_color.a < 1.0`) durumlarında `bg_color`'ı söndürür.

4. **`blend.wgsl` & M1 Golden Hash Durumu:**
   - `blend.wgsl:48–62`: `overlay_channel` `b <= 0.5` kontrolünü doğrudan `LinearRec709` üzerinde yapar.
   - `project.rs:981, 1040`: `Project::golden_scene_preset()` yalnızca `BlendMode::Screen` (`blend_atmosphere`) ve `BlendMode::Normal` (`blend_title`) kullanır; `BlendMode::Overlay` kullanmaz. `compositor_tests.rs:572` `Overlay` kullanır ancak piksel hash'i değil yalnızca `rgba8.len()` ve tekil decode sayısını doğrular.

---

## Teknik Tasarım ve Şartname

### Bölüm (a): `text3d_mesh.wgsl` — Cook-Torrance GGX PBR, Kamera-Bağıl Stüdyo IBL, Bevel Cavity AO & Z-Echo Depth-Write Ayrımı

#### 1. `Text3DUniform` Genişlemesi (416 → 432 Byte, 16-Byte Hizalı)
Mevcut ilk 416 byte alan düzeni %100 korunur; sonuna tek bir `pbr_params: [f32; 4]` alanı eklenir (`432 % 16 == 0`):

```rust
// crates/frameshift-render/src/uniforms.rs
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct Text3DUniform {
    // ... mevcut 0..416 alanları değişmez ...
    /// `x` = roughness `[0.04..1.0]` (`< 0` -> material_mode varsayılanı),
    /// `y` = metallic `[0.0..1.0]` (`< 0` -> material_mode varsayılanı),
    /// `z` = ao_strength `[0.0..1.0]` (varsayılan `0.65`),
    /// `w` = echo_pass_flag (`0.0` = hero layer 0, `1.0` = Z-echo layers `total_echoes..1`).
    pub pbr_params: [f32; 4], // 416..432
}
const _: () = assert!(std::mem::size_of::<Text3DUniform>() == 432);
```

Varsayılan PBR profilleri (`roughness < 0.0` veya `metallic < 0.0` iken `material_mode` tablosundan türetilir; böylece mevcut JSON sahneleri ek alan gerektirmeden otomatik olarak PBR kalitesine yükselir):

| `material_mode` | `roughness` | `metallic` | `f0` / Özellik |
| :--- | :---: | :---: | :--- |
| `0u` (`Neon3D`) | `0.26` | `0.18` | Dielektrik/emayemsi yarı-parlak neon gövde + güçlü self-emissive çekirdek |
| `1u` (`MetallicChrome`) | `0.12` | `0.98` | Sıvı krom iletken (`f0 = mix(0.04, base_rgb, metallic)`) |
| `2u` (`Flat2DStroked`) | — | — | PBR bypass; saf unlit HDR (`base_rgb * side_shade * hdr_intensity`) |
| `3u` (`IridescentAnisotropic`) | `0.18` | `0.92` | İnce-film spektral `f0` + `D_GGX_Anisotropic` veya izotropik GGX + anizotropik lobe |

*(Opsiyonel şema genişlemesi: `NodeKind::Text3D` ve `NodeKind::Shape3D` içine `#[serde(default = "default_neg_one")] pub roughness: f32`, `pub metallic: f32`, `#[serde(default = "default_ao_strength")] pub bevel_ao: f32` eklenebilir; `-1.0` geldiğinde `material_mode` tablosu kullanılır.)*

#### 2. Filament Cook-Torrance GGX BRDF (WGSL)
```wgsl
const PI: f32 = 3.14159265359;

fn d_ggx(no_h: f32, a: f32) -> f32 {
    let a2 = a * a;
    let f = (no_h * a2 - no_h) * no_h + 1.0;
    return a2 / (PI * max(f * f, 1e-6));
}

fn v_smith_ggx_correlated(no_v: f32, no_l: f32, a: f32) -> f32 {
    let a2 = a * a;
    let ggxv = no_l * sqrt((-no_v * a2 + no_v) * no_v + a2);
    let ggxl = no_v * sqrt((-no_l * a2 + no_l) * no_l + a2);
    return 0.5 / max(ggxv + ggxl, 1e-6);
}

fn f_schlick(u_cos: f32, f0: vec3<f32>) -> vec3<f32> {
    let f = pow(clamp(1.0 - u_cos, 0.0, 1.0), 5.0);
    return f0 + (vec3<f32>(1.0) - f0) * f;
}

fn f_schlick_roughness(no_v: f32, f0: vec3<f32>, roughness: f32) -> vec3<f32> {
    let f = pow(clamp(1.0 - no_v, 0.0, 1.0), 5.0);
    return f0 + (max(vec3<f32>(1.0 - roughness), f0) - f0) * f;
}

// Karis (2014) / Lazarov (2013) Analytic Split-Sum EnvBRDFApprox (zero LUT texture)
fn env_brdf_approx(f0: vec3<f32>, roughness: f32, no_v: f32) -> vec3<f32> {
    let c0 = vec4<f32>(-1.0, -0.0275, -0.572, 0.022);
    let c1 = vec4<f32>(1.0, 0.0425, 1.04, -0.04);
    let r = roughness * c0 + c1;
    let a004 = min(r.x * r.x, exp2(-9.28 * no_v)) * r.x + r.y;
    let ab = vec2<f32>(-1.04, 1.04) * a004 + r.zw;
    return f0 * ab.x + vec3<f32>(ab.y);
}
```
- `a = roughness * roughness` (`roughness = clamp(roughness, 0.045, 1.0)`).
- Direkt ışık katkısı: `Fr = d_ggx(no_h, a) * v_smith_ggx_correlated(no_v, no_l, a) * f_schlick(lo_h, f0)`, `Fd = (vec3(1.0) - f) * (1.0 - metallic) * base_rgb * (1.0 / PI)`, toplam `radiance * (Fd + Fr) * no_l`.

#### 3. Kamera-Bağıl Stüdyo IBL Yansıma Modeli (90° Dönüş Koruması)
`u.view` matrisinin üst `3×3` alt matrisi dünya-uzayı → kamera-uzayı ortonormal dönüşümüdür. Kamera eksenleri dünya uzayında doğrudan `u.view` sütunlarından okunur (ekstra uniform gerekmez):
```wgsl
let cam_right = safe_normalize(vec3<f32>(u.view[0].x, u.view[1].x, u.view[2].x), vec3<f32>(1.0, 0.0, 0.0));
let cam_up    = safe_normalize(vec3<f32>(u.view[0].y, u.view[1].y, u.view[2].y), vec3<f32>(0.0, 1.0, 0.0));
let cam_back  = safe_normalize(vec3<f32>(u.view[0].z, u.view[1].z, u.view[2].z), vec3<f32>(0.0, 0.0, 1.0));
```
1. **Kamera-Bağıl 3-Noktalı Stüdyo Işıkları:**
   - `key_dir = safe_normalize(cam_right * 0.42 + cam_up * 0.62 + cam_back * 0.66, cam_back)` — kamera nesnenin etrafında 90° veya 180° döndüğünde dahi ön/bevel yüzeyler daima `%66` önden gelen ana stüdyo ışığını görür, harfler asla kararmaz.
   - `fill_dir = safe_normalize(-cam_right * 0.72 + cam_up * 0.18 + cam_back * 0.67, cam_back)` — serin dolgu ışığı.
   - `rim_dir = safe_normalize(-cam_right * 0.30 + cam_up * 0.65 - cam_back * 0.70, -cam_back)` — arka kontur ayırıcı rim light.
2. **Kamera-Bağıl Analitik Stüdyo IBL (`sample_studio_ibl(r_world, n_world, roughness)`):**
   - Yansıma vektörü kamera uzayına alınır: `r_view = safe_normalize((u.view * vec4<f32>(r_world, 0.0)).xyz, vec3<f32>(0.0, 0.0, 1.0))`.
   - Küresel yön harmanlaması `r_env = safe_normalize(mix(r_view, r_world, 0.25), r_view)` ile dünya ufkunun yön hissi korunurken kamera dönüşlerinde yansıma daima stüdyo softbox'larını yakalar.
   - `roughness` arttıkça softbox kenar yumuşaklığı genişler (`blur_band = mix(18.0, 2.5, roughness)`):
     - **Üst & Alt Stüdyo Kubbesi:** `sky` (`[0.24, 0.32, 0.48] -> [0.95, 0.98, 1.06]`) ve `ground` (`[0.05, 0.04, 0.07] -> [0.22, 0.20, 0.28]`) arası `smoothstep(-0.18 - r*0.35, 0.12 + r*0.35, r_env.y)`.
     - **Stüdyo Ufuk Çizgisi (Chrome Glint):** `exp(-abs(r_env.y) * mix(16.0, 3.0, roughness)) * (1.0 - roughness * 0.55) * vec3(1.45, 1.48, 1.55)`.
     - **İki Dikdörtgensel Softbox Paneli (`r_view.xy`):** Ön-üst sağ (`normalize(vec2(0.35, 0.78, ...))`) ve sol yan (`normalize(vec2(-0.82, 0.36, ...))`) üzerinde `roughness`-bağımlı üs ile sert parlak krom şeritleri (yüksek pürüzlülükte yumuşak diffüz parlama).
     - **İrradiance (Diffuse IBL):** Normal `n_view` üzerinden aynı stüdyo küresinin düşük frekanslı (`roughness = 1.0`) karşılığı ile `(1 - F) * (1 - metallic) * base_rgb * irradiance`.

#### 4. Bevel Cavity AO (Sıfır Vertex Layout Değişikliği ile)
`GpuMeshVertex3D` (48 byte) ve `face_kind` (`0..=3`) tam sayı değerleri değiştirilmez (`mesh3d.rs` unit testleri korunur). Bunun yerine `VertexOutput` içine `@location(6) local_bevel_factor: f32` eklenir:
- `vs_main` içinde: `out.local_bevel_factor = abs(in.normal.z);` (`0.0` = dik ekstrüzyon yan duvarı, `0.05..0.95` = bevel eğim sırtı, `1.0` = ön/arka düz kapak).
- `fs_main` içinde:
  1. **Side/Crease Cavity:** `face_kind == 1u` üzerinde dik yan duvarlar bevel sırtlarına göre daha derin oyukta kaldığından `wall_ao = select(1.0, mix(0.62, 0.96, smoothstep(0.05, 0.75, in.local_bevel_factor)), in.face_kind == 1u)`.
  2. **Screen-Space Curvature AO (`fwidth`):** `let n_grad = length(fwidth(n)); let crease_ao = 1.0 - clamp(n_grad * 0.42, 0.0, 0.35);` — özellikle `Concave` / `Stepped` bevel basamakları ile `B`, `E`, `M`, `W` gibi iç harf köşelerinde temas gölgesi üretir.
  3. **Birleşik AO ve Filament `computeSpecularAO`:**
     ```wgsl
     let raw_ao = wall_ao * crease_ao;
     let ao = mix(1.0, raw_ao, ao_strength);
     let spec_ao = clamp(pow(no_v + ao, exp2(-16.0 * roughness - 1.0)) - 1.0 + ao, 0.0, 1.0);
     ```
     `ao` yalnızca **IBL diffuse** bileşenine, `spec_ao` ise **IBL specular** bileşenine uygulanır; doğrudan spot/rim highlights ve `face_kind == 3u` dış kontur (`outline_color`) karartılmaz.

#### 5. Yarı-Şeffaf Z-Echo İçin Depth-Write Ayrımı (`text3d_mesh` vs `text3d_mesh_echo`)
1. **`pipelines.rs`:**
   - `text3d_mesh`: Mevcut `scene_depth_write_state` (`depth_write_enabled: Some(true)`, `depth_compare: Some(Less)`) — yalnızca **kahraman (hero) katmanı (`echo_layer == 0u`)** için kullanılır.
   - `text3d_mesh_echo`: Aynı `SHADER_TEXT3D_MESH` modülü ve `GpuMeshVertex3D` layout'u, ancak `scene_depth_read_state` (`depth_write_enabled: Some(false)`, `depth_compare: Some(Less)`) ve `PREMULTIPLIED_ALPHA_BLENDING` ile derlenir.
2. **`text3d_mesh.wgsl` (`vs_main`):**
   - `let is_echo_pass = u.pbr_params.w > 0.5;`
   - `is_echo_pass == false` iken `echo_layer = 0u` (`instance_id = 0` kahraman katmanı).
   - `is_echo_pass == true` iken `echo_layer = total_echoes - min(instance_id, total_echoes)` (`instance_id` `0..total_echoes` aralığında arka-öne `total_echoes..1` katmanlarını çizer).
   - Avantaj: `draw_indexed(..., 0..1)` ve `draw_indexed(..., 0..echo_count)` çağrılarının ikisi de `first_instance = 0` kullanır; DX12/Vulkan/Metal backend'leri arasında `instance_index` offset tutarsızlığı riski sıfırdır.
3. **`compositor.rs` Çizim Sırası:**
   - **Standalone `Text3D` / `Shape3D` (`record_mesh3d_pass`):** Önce `text3d_mesh` ile `0..1` hero çizilir (`depth_write: true`), ardından `echo_count > 0` ise aynı render pass içinde `text3d_mesh_echo` ile `0..echo_count` çizilir (`depth_write: false`).
   - **`Scene3D` (`evaluate_scene3d`):** Hero mesh'ler (`opacity >= 0.999`) Pass 1'de `text3d_mesh` ile çizilir (`depth_write: true`). Yarı-şeffaf hero mesh'ler (`opacity < 0.999`) ve tüm Z-echo katmanları (`echo_count > 0`) `depth_write: false` ile `text3d_mesh_echo` üzerinden hero sonrası çizilir; böylece yarı-şeffaf katmanlar asla arkadaki şeritleri veya diğer 3D nesneleri `Depth32Float` üzerinden silmez.

---

### Bölüm (b): `Scene3D` İçinde `ribbon_trail3d` ve `particle_field3d`'nin `fx_dof_coc` ile Bozulmasının Çözümü

#### Neden `depth-write` veya `alpha-test` Tek Başına Çalışmaz?
1. **Doğrudan `depth-write` (`depth_write_enabled: Some(true)`):** `particle_field3d` ve `ribbon_trail3d` `hdr_additive_premul_blend` ile yumuşak Gaussian/üstel halelere (`alpha` `1.0 -> 0.001`) sahiptir. Bir parçacığın quad'ı `alpha = 0.02` kenarında derinlik yazarsa arkasındaki diğer parçacıkları ve şeritleri dikdörtgen/daire delikler halinde keser ve toplamsal parlaklığı (additive glow) bozar.
2. **`alpha-test` (`discard` at `alpha < 0.5`):** Yumuşak bokeh parçacıklarının ve şerit halelerinin dış %60'lık alanı `alpha < 0.5` olduğu için kesilip piksellenir; daha kötüsü, `alpha >= 0.5` çekirdeği derinlik yazarken `alpha < 0.5` hale bölgesi derinlik yazmaz (`raw_d = 1.0`), bu da `fx_dof_coc`'un çekirdeğe gerçek derinlik, haleye ise `z = far = 200.0` atayıp **keskin çekirdek + maksimum bulanık hale dikişi** üretmesine yol açar.

#### Önerilen Tasarım: İki Katmanlı `Scene3D` Kompozisyonu (`opaque_tex` + `atmos_tex` + Salt-Okunur `depth_tex` + Analitik Atmosfer DoF)

```
Pass 1 (Phase A - Opak Geometri):
  Text3D Hero + Shape3D Hero + Transform3D Cards
  -> color: opaque_tex (Rgba16Float)
  -> depth: depth_tex  (Depth32Float, depth_write = true, StoreOp::Store)

Pass 2 (Phase B - Şerit & Parçacık Atmosferi, Sadece !phase_b.is_empty()):
  RibbonTrail3D + ParticleField3D (Scene3D focal_dist & dof_blur_strength ile ANALİTİK DoF)
  -> color: atmos_tex (Rgba16Float, Clear Transparent)
  -> depth: depth_tex (Depth32Float, LoadOp::Load, depth_write = false, depth_compare = Less)

Pass 3 (Z-Echo Katmanı, Sadece echo/yarı-şeffaf mesh varsa):
  Text3D/Shape3D Z-Echoes (text3d_mesh_echo, depth_write = false, depth_compare = Less)
  -> color: opaque_tex (LoadOp::Load)
  -> depth: depth_tex  (LoadOp::Load, read-only)

Pass 4 & 5 (fx_dof_coc H + V, sadece dof_blur_strength > 1e-4 && max_radius > 0.5):
  Yalnızca (opaque_tex, depth_tex) üzerinden CoC haritası çıkarılır!
  (raw_d >= 0.99999 -> koşulsuz void (coc = 0.0), z = far=200 tuzağı tamamen ortadan kalkar!)

Pass 6 (fx_dof_bokeh + Atmosfer Birleştirme):
  bg_with_atmos = atmos_col + bg_color * (1.0 - atmos_col.a)
  out_rgba      = fg_layer + bg_with_atmos * (1.0 - fg_layer.a)
```

1. **Analitik DoF (`ribbon_trail3d.wgsl` & `particle_field3d.wgsl`):**
   - **`particle_field3d.wgsl`:** Zaten vertex shader'da `coc = abs(view_dist - u.params1.y) * max(u.params1.z, 0.0)`, `dof_expand`, `edge_soft`, `dof_atten` hesaplar. `Scene3D` içinde `build_particle_field3d_uniform` çağrısı, `Scene3D`'nin hesaplanmış `focal_dist` ve `camera.dof_blur_strength` değerlerini `params1.yz` içine enjekte eder (veya `max` alır).
   - **`ribbon_trail3d.wgsl`:** `RibbonTrail3DUniform` 224 → **240 byte** yapılarak `dof_params: vec4<f32>` (`x = dof_focal_dist`, `y = dof_blur_strength`, `z = max_world_expand`, `w = reserved`) eklenir. Vertex shader'da `view_dist = length(pos - u.cam_pos_time.xyz)` üzerinden:
     ```wgsl
     let ribbon_coc = clamp(abs(view_dist - u.dof_params.x) * max(u.dof_params.y, 0.0), 0.0, 2.0);
     let half_w = width * 0.5 * tip_taper * (1.0 + ribbon_coc * 1.4);
     ```
     Fragment shader'da `ribbon_coc` (`@location(3)`) ile `halo` üssü ve `core` genişliği yumuşatılır, enerji `1.0 / (1.0 + ribbon_coc * 0.85)` ile korunur.
2. **Neden Bu Tasarım Kusursuz Çalışır?**
   - Şeritler ve parçacıklar `opaque_tex`'e değil `atmos_tex`'e çizildiği için **`fx_dof_coc` asla şerit/parçacık piksellerinde `raw_d = 1.0 -> z = 200` görmez**.
   - `Phase B` aynı `depth_tex`'i (`depth_compare = Less`, `depth_write = false`) okuduğu için opak `Text3D`/`Shape3D` arkasında kalan şerit/parçacıklar donanım Z-testi ile piksel hassasiyetinde gizlenir.
   - `fx_dof_bokeh` içinde `atmos_tex` arka plan bokeh katmanının **üstüne**, ön-plan bokeh sızıntısının (`fg_layer`) ise **altına** birleştirildiği için ön-planda bulanıklaşan bir `Text3D`'nin bokeh halesi arkasındaki şeridin üstüne doğal biçimde taşar; arka plandaki bulanık `Text3D`'nin önündeki şerit ise keskin/analitik-DoF olarak üstte kalır.
   - **DoF Kapalıyken (`dof_blur_strength <= 1e-4`):** Ekstra pass açılmaz; mevcut tek `render_pass` akışı (`Phase A` + `echo` + `Phase B`) doğrudan `color_tex` üzerinde çalışır — sıfır maliyet ve Faz 6.3 testleriyle %100 uyumluluk.

---

### Bölüm (c): `fx_dof_coc.wgsl` 2-Pass Separable H+V Max CoC Dilation & `fx_dof_bokeh.wgsl` Sinematik Bokeh + Ön-Plan Alpha Kompozisyonu

#### 1. `fx_dof_coc.wgsl` — 2-Pass Separable H + V Max CoC Dilation
`max` morfolojik genişletme operatörü ayrılabilir (separable) olduğu için:
$$\max_{|dx| \le R, |dy| \le R} C_{\text{near}}(x+dx, y+dy) = \max_{|dy| \le R} \left( \max_{|dx| \le R} C_{\text{near}}(x+dx, y+dy) \right)$$
Tek `fx_dof_coc.wgsl` modülü içinde iki entry point tanımlanır (`DofCocUniform` **32 byte** olarak değişmeden kalır):

- **Void Düzeltmesi (`eval_signed_coc`):**
  ```wgsl
  // depth_tex'e hiç opak geometri yazılmadıysa (raw_d >= 0.99999) alpha ne olursa olsun void kabul edilir;
  // böylece Z-echo veya subpixel yarı-şeffaf kenarlar z = far (200.0) sanılmaz.
  if (raw_d >= 0.99999) {
      return vec4<f32>(0.0, far, alpha, 1.0);
  }
  ```
- **Pass 1 (`@fragment fn fs_coc_h`):**
  - Bindings: `@binding(0) color_tex: texture_2d<f32>`, `@binding(1) depth_tex: texture_depth_2d`, `@binding(2) u: DofCocUniform`.
  - Merkez pikselin `(signed_coc_px, near_coc, linear_z, alpha)` değerini hesaplar.
  - `let rad_i = clamp(i32(ceil(max_rad)), 0, 48);`
  - **1-piksel adımlı yatay tarama** (`for (var dx: i32 = -48; dx <= 48; dx = dx + 1)` içinde `if (abs(dx) > rad_i) { continue; }` — sabit `97` iterasyon, `MAX_SHADER_LOOP_ITERATIONS = 128` sınırının altında ve Naga uyumlu!):
    - Yalnızca gerçekten `dx` mesafesi `near_coc` yarıçapı içinde olan komşuları veya tüm `[-rad_i, rad_i]` penceresindeki `max(-n_info.x, 0.0)` değerini alır (`h_max_near`).
    - Çıkış (`coc_h_tex`, `Rgba16Float`): `vec4<f32>(center.x, h_max_near, center.y, center.z)`.
- **Pass 2 (`@fragment fn fs_coc_v`):**
  - Bindings (`pipelines.rs` içinde `fx_dof_coc_v` programı: `binding(0) = coc_h_tex: texture_2d<f32>`, `binding(2) = u: DofCocUniform`; `depth_tex` referans edilmez):
  - Merkez pikselin `r, b, a` kanallarını (`signed_coc_px`, `linear_z`, `alpha`) olduğu gibi korur.
  - **1-piksel adımlı dikey tarama** (`dy in -48..=48`, `if (abs(dy) > rad_i) { continue; }`) ile `coc_h_tex.g` üzerinden `v_max_near` alır.
  - Çıkış (`coc_tex`, `Rgba16Float`): `vec4<f32>(center_h.r, v_max_near, center_h.b, center_h.a)`.
- **Kazanç:** `25` seyrek (`step=12px`) tap yerine her pikselde `1 px` hassasiyetle `97+97` (gerçekte `2R+1`) tap gezilir; `1–2 px` kalınlığındaki neon harfler ve ince konturlar hiçbir zaman kaçırılmaz.

#### 2. `fx_dof_bokeh.wgsl` — Poligonal (6/8-Bıçaklı), Anamorfik Oval, Cat's-Eye Vinyet & Doğru Ön-Plan Alpha

**`DofBokehUniform` (32 → 48 Byte, 16-Byte Hizalı):**
```rust
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct DofBokehUniform {
    /// `x` = texel_x, `y` = texel_y, `z` = max_radius_px, `w` = tap_count.
    pub params0: [f32; 4], // 0..16
    /// `x` = highlight_boost, `y` = near_bleed_bias, `z` = jitter_seed, `w` = blade_roundness [0..1].
    pub params1: [f32; 4], // 16..32
    /// `x` = blade_count (`0`=circle, `6`=`hex`, `8`=`oct`), `y` = blade_rotation_rad,
    /// `z` = anamorphic_ratio (`1.0`=spherical, `0.5`=2:1 vertical oval), `w` = cats_eye_strength (`0..1`).
    pub params2: [f32; 4], // 32..48
}
const _: () = assert!(std::mem::size_of::<DofBokehUniform>() == 48);
```

**`Camera3DDef` Şema Alanları (`crates/frameshift-schema/src/graph.rs`, `#[serde(default)]` ile geriye dönük uyumlu):**
- `dof_aperture_blades: u32` (varsayılan `0`)
- `dof_blade_rotation_deg: f32` (varsayılan `0.0`)
- `dof_blade_roundness: f32` (varsayılan `0.25`)
- `dof_anamorphic_ratio: f32` (varsayılan `1.0`)
- `dof_cats_eye_strength: f32` (varsayılan `0.0`)

**WGSL Çekirdek Matematiği:**
1. **Poligonal Açıklık Bükümü (Bijektif Vogel Disk):**
   ```wgsl
   fn polygon_radial_scale(theta: f32, blades_f: f32, rot_rad: f32, roundness: f32) -> f32 {
       if (blades_f < 3.0) {
           return 1.0;
       }
       let n = clamp(round(blades_f), 3.0, 12.0);
       let sector = 6.2831853 / n;
       let half_sector = 0.5 * sector;
       let a = theta - rot_rad;
       let d_theta = abs(a - floor((a + half_sector) / sector) * sector);
       let poly_r = cos(half_sector) / max(cos(d_theta), 1e-4);
       return mix(poly_r, 1.0, clamp(roundness, 0.0, 1.0));
   }
   ```
   `poly_r <= 1.0` olduğu için poligon daima birim çember içinde kalır ve `dilated_near` sınırını aşmaz.
2. **Anamorfik Oval Ölçekleme:**
   ```wgsl
   let anam = clamp(u.params2.z, 0.25, 4.0);
   let anam_scale = select(vec2<f32>(1.0, 1.0 / anam), vec2<f32>(anam, 1.0), anam <= 1.0);
   ```
   Örnekleme yönü: `let disk_dir = vec2<f32>(cos(theta), sin(theta)) * poly_scale;` ve piksel ofseti:
   `let offset_uv = disk_dir * anam_scale * r_px * texel;`
   (`r_px = r_norm * gather_radius` ise pokry/coverage testinde kullanılır, böylece oval içindeki yoğunluk korunur).
3. **Cat's-Eye Optik Vinyet:**
   ```wgsl
   let aspect = dims.x / max(dims.y, 1.0);
   let p_screen = (in.uv - vec2<f32>(0.5)) * vec2<f32>(aspect, 1.0);
   let max_corner = length(vec2<f32>(0.5 * aspect, 0.5));
   let ecc = clamp(length(p_screen) / max(max_corner, 1e-4), 0.0, 1.0);
   let radial_dir = select(vec2<f32>(0.0), normalize(p_screen), length(p_screen) > 1e-5);
   let cat_shift = clamp(u.params2.w, 0.0, 1.0) * ecc * 0.72;
   ```
   Döngü içinde her tap için:
   `let q = disk_dir * r_norm;`
   `let cat_mask = smoothstep(1.02, 0.85, length(q - radial_dir * cat_shift));`
   Merkezde `cat_shift = 0` -> tam şekil; köşelere doğru optik gövde kesiti bokeh diskini merkeze teğet badem ("cat's-eye") formuna budar. `cat_shift <= 0.72` sınırı her pikselde en az merkeze yakın tap'lerin hayatta kalmasını (`bg_w > 0`) garanti eder.
4. **Arka-Plan Sızıntısı Düzeltmesi (`bg_cov`) ve Doğru Ön-Plan Alpha Kompozisyonu:**
   - Arka-plan filtresi (satır 117 düzeltmesi):
     ```wgsl
     let is_behind_or_bg = (s_coc >= 0.0) && (s_z >= center_z - 0.15 || s_coc >= center_coc);
     let depth_ok = select(0.0, smoothstep(center_z - 0.15, center_z + 0.05, s_z), s_z >= center_z - 0.15);
     let bg_gate = select(depth_ok, 1.0, s_coc >= 0.0 && center_coc >= 0.0 && s_z >= center_z - 0.15);
     ```
     Daha yalın ve kesin form:
     ```wgsl
     let s_is_fg = s_coc < 0.0;
     let depth_gate = smoothstep(center_z - 0.15, center_z + 0.05, s_z);
     let bg_allow = select(depth_gate, 0.0, s_is_fg && center_coc >= 0.0);
     let bg_rad = max(max(s_coc, center_coc), 0.0);
     let bg_cov = smoothstep(r_px - 1.0, r_px + 1.0, bg_rad) * bg_allow * cat_mask;
     ```
   - Ön-plan alpha & Porter-Duff Source-Over kompozisyonu (satır 128–132 düzeltmesi):
     ```wgsl
     let fg_cov = smoothstep(r_px - 1.0, r_px + 1.0, fg_coc_abs) * cat_mask;
     // ...
     let bg_color = select(center_col, bg_acc / max(bg_w, 1e-5), bg_w > 1e-4);
     let fg_avg = select(vec4<f32>(0.0), fg_acc / max(fg_w, 1e-5), fg_w > 1e-4);
     let fg_spatial_cov = clamp((fg_cov_sum / max(cat_w_sum, 1e-4)) * (1.0 + near_bleed_bias), 0.0, 1.0);
     let fg_layer = fg_avg * fg_spatial_cov;

     // Atmosfer katmanı (Scene3D varsa binding(2) atmos_tex, yoksa vec4(0.0)):
     let bg_with_atmos = vec4<f32>(
         atmos_col.rgb + bg_color.rgb * (1.0 - atmos_col.a),
         clamp(atmos_col.a + bg_color.a * (1.0 - atmos_col.a), 0.0, 1.0),
     );
     let out_rgba = fg_layer + bg_with_atmos * (1.0 - clamp(fg_layer.a, 0.0, 1.0));
     ```
     **Matematiksel Doğrulama:**
     - Yarı-şeffaf ön-plan (`a=0.5`, tam disk kapsaması `fg_spatial_cov=1.0`): `fg_layer = (0.5*rgb, 0.5)` -> arka plan `%50` oranında altından görünmeye devam eder!
     - Opak ön-plan (`a=1.0`, %50 kenar kapsaması `fg_spatial_cov=0.5`): `fg_layer = (0.5*rgb, 0.5)` -> boş arka planda `alpha = 0.5` yumuşak bokeh kenarı, opak arka planda `%50` karışım!
     - `fx_dof_bokeh` bind group düzeni: `binding(0) = color_tex`, `binding(1) = coc_tex`, `binding(2) = atmos_tex` (yoksa 1×1 şeffaf `0,0,0,0` dummy doku — `GpuContext` veya `GpuCompositor` içinde tek seferlik önbelleğe alınır), `binding(3) = s_linear`, `binding(4) = u: DofBokehUniform`.

---

### Bölüm (d): `blend.wgsl` — Algısal/sRGB Uyumlu `Overlay` ve Yeni Modlar (`SoftLight`, `ColorDodge`, `HardLight`, `Lighten`, `Darken`)

#### 1. Şema & Uniform Eşlemesi (`32` Byte Sabit)
`crates/frameshift-schema/src/graph.rs`:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlendMode {
    #[default]
    Normal,     // 0u (Linear Porter-Duff Source-Over)
    Add,        // 1u (Linear Dodge)
    Screen,     // 2u (Linear Screen)
    Multiply,   // 3u (Linear Multiply + W3C alpha)
    Overlay,    // 4u (Perceptual sRGB Overlay + W3C alpha)
    SoftLight,  // 5u (Perceptual sRGB W3C SoftLight + W3C alpha)
    ColorDodge, // 6u (Perceptual sRGB W3C ColorDodge + W3C alpha)
    HardLight,  // 7u (Perceptual sRGB W3C HardLight + W3C alpha)
    Lighten,    // 8u (Linear/HDR max(Cb, Cs) + W3C alpha)
    Darken,     // 9u (Linear/HDR min(Cb, Cs) + W3C alpha)
}
```
`BlendUniform` (`32` byte, `uniforms.rs:82-102`) değişmez:
- `mode_flags[0]` = `mode_id` (`0u..=9u`)
- `mode_flags[1]` = `0u` (varsayılan: non-linear/perceptual modlar sRGB'de; `Normal/Add/Screen/Multiply/Lighten/Darken` Linear'da) veya `1u` (legacy linear override, test/geriye dönük karşılaştırma için).

#### 2. Exact IEC 61966-2-1 sRGB ↔ Linear Rec.709 & W3C Compositing Level 1 Formülleri (WGSL)
```wgsl
fn linear_to_srgb_ch(c: f32) -> f32 {
    let x = clamp(c, 0.0, 1.0);
    return select(1.055 * pow(x, 1.0 / 2.4) - 0.055, x * 12.92, x <= 0.0031308);
}

fn srgb_to_linear_ch(c: f32) -> f32 {
    let x = clamp(c, 0.0, 1.0);
    return select(pow((x + 0.055) / 1.055, 2.4), x / 12.92, x <= 0.04045);
}

fn overlay_ch(b: f32, s: f32) -> f32 {
    return select(1.0 - 2.0 * (1.0 - b) * (1.0 - s), 2.0 * b * s, b <= 0.5);
}

fn soft_light_ch(b: f32, s: f32) -> f32 {
    if (s <= 0.5) {
        return b - (1.0 - 2.0 * s) * b * (1.0 - b);
    }
    let d_b = select(sqrt(b), ((16.0 * b - 12.0) * b + 4.0) * b, b <= 0.25);
    return b + (2.0 * s - 1.0) * (d_b - b);
}

fn color_dodge_ch(b: f32, s: f32) -> f32 {
    if (b <= 0.0) {
        return 0.0;
    }
    if (s >= 1.0 - 1e-5) {
        return 1.0;
    }
    return min(1.0, b / (1.0 - s));
}

fn hard_light_ch(b: f32, s: f32) -> f32 {
    return overlay_ch(s, b);
}
```
- **Uygulama Kuralı:**
  - `mode == 0u..=3u` (`Normal`, `Add`, `Screen`, `Multiply`): **Tek satır bile değiştirilmez**; M1 ve tüm eski sahnelerle bit düzeyinde özdeştir.
  - `mode == 4u..=7u` (`Overlay`, `SoftLight`, `ColorDodge`, `HardLight`):
    1. `cb = safe_unpremultiply(base)`, `cs = safe_unpremultiply(top)` (Linear Rec.709).
    2. `sb = linear_to_srgb(cb)`, `ss = linear_to_srgb(cs)`.
    3. Kanal bazlı W3C formülü uygulanır -> `s_out`, ardından `blended = srgb_to_linear(s_out)` ile `LinearRec709`'a dönülür.
    4. *(HDR Tavan Koruması — Opsiyonel/Önerilen):* `cb > 1.0` olan HDR taşma (`hdr_excess = max(cb - vec3(1.0), vec3(0.0))`) için `Overlay`/`SoftLight`/`HardLight` sonrasında `+ hdr_excess * (1.0 - 0.5 * as_top)` eklenebilir veya saf W3C `[0,1]` çıkış korunur; saf W3C `[0, 1]` sRGB davranışı After Effects / CSS / Photoshop ile birebir eşleşir.
  - `mode == 8u` (`Lighten`) ve `mode == 9u` (`Darken`):
    - `max(cb, cs)` ve `min(cb, cs)` monoton sRGB dönüşümü altında değişmez olduğundan doğrudan `LinearRec709`'da hesaplanır (`max(cb, vec3(0.0))` ve `max(cs, vec3(0.0))`); böylece `> 1.0` HDR parlaklıkları da kırpılmadan korunur!
  - Tüm `mode == 3u..=9u` için ortak W3C premultiplied alfa kompozisyonu:
    `out_rgb = (1.0 - ab) * top.rgb + (1.0 - as_top) * base.rgb + (ab * as_top) * blended;`

---

## Değişecek Dosyalar Özeti (Faz 2 Uygulama Planı)

| Dosya | Değişiklik Özeti |
| :--- | :--- |
| `crates/frameshift-schema/src/graph.rs` | `BlendMode` enumuna `SoftLight`, `ColorDodge`, `HardLight`, `Lighten`, `Darken`; `Camera3DDef`'e `#[serde(default)]` `dof_aperture_blades`, `dof_blade_rotation_deg`, `dof_blade_roundness`, `dof_anamorphic_ratio`, `dof_cats_eye_strength` |
| `crates/frameshift-render/src/uniforms.rs` | `Text3DUniform` (416→432B), `RibbonTrail3DUniform` (224→240B), `DofBokehUniform` (32→48B), `BlendUniform::new` (`0u..=9u`) |
| `crates/frameshift-render/shaders/text3d_mesh.wgsl` | `D_GGX`, `V_SmithGGXCorrelated`, `F_Schlick`, `env_brdf_approx`, `u.view` kamera-bağıl stüdyo IBL, `local_bevel_factor` + `fwidth` cavity AO, `pbr_params.w` Z-echo ayrımı |
| `crates/frameshift-render/shaders/ribbon_trail3d.wgsl` | `RibbonTrail3DUniform.dof_params` ve vertex/fragment analitik DoF |
| `crates/frameshift-render/shaders/fx_dof_coc.wgsl` | `raw_d >= 0.99999` koşulsuz void düzeltmesi; `fs_coc_h` + `fs_coc_v` 2-pass separable 1px-stepped max dilation |
| `crates/frameshift-render/shaders/fx_dof_bokeh.wgsl` | Poligonal N-blade, anamorfik oval, cat's-eye vinyet, `bg_allow` sızıntı düzeltmesi, Porter-Duff Source-Over ön-plan + `atmos_tex` kompozisyonu |
| `crates/frameshift-render/shaders/blend.wgsl` | IEC 61966-2-1 sRGB OETF/EOTF ile algısal `Overlay` + `SoftLight`, `ColorDodge`, `HardLight`, `Lighten`, `Darken` |
| `crates/frameshift-render/src/pipelines.rs` | `text3d_mesh_echo` (`depth_write: false`), `fx_dof_coc_v`, 3-tex `fx_dof_bokeh` BGL |
| `crates/frameshift-render/src/compositor.rs` | Hero/Echo iki-aşamalı çizim, `Scene3D` `opaque_tex` + `atmos_tex` ayrımı, 2-pass `fx_dof_coc_h -> fx_dof_coc_v`, 1×1 şeffaf dummy `atmos_tex`, `Scene3D` kamera DoF'unu `ParticleField3D`/`RibbonTrail3D`'ye aktarma |

---

## M1 Golden Hash ve Mevcut Test Koruma Planı

1. **M1 Golden Scene (`Project::golden_scene_preset()`):**
   - Yalnızca `BlendMode::Screen` ve `BlendMode::Normal` kullanır (`project.rs:981, 1040`); `BlendMode::Overlay` kullanmaz.
   - `Normal`, `Add`, `Screen`, `Multiply` yolları `blend.wgsl` içinde **bit düzeyinde dokunulmadan** kalacağı için M1 final/draft MP4 ve seek-independence determinizm hash'leri %100 korunacaktır.
2. **`Text3D` / `Shape3D` Geriye Dönük Testler:**
   - `GpuMeshVertex3D` (48 byte) ve `face_kind` (`0..=3`) değişmediği için `frameshift-vector` (`mesh3d.rs`) testleri kırılmaz.
   - `material_mode = 2u` (`Flat2DStroked`) unlit yolunu koruduğu için `faz6_3_tests::scene3d_shared_depth_occlusion_and_lookat_camera` piksel düzeyinde değişmez.
   - `Text3D` 90° kamera dönüş testi (`cam_rot_y` / `LookAt` X-ekseni) için ön kapak parlaklık alt sınırı doğrulaması eklenecektir.
3. **`Scene3D` DoF Testleri (`faz6_3_tests::scene3d_z_buffer_bokeh_dof_blurs_out_of_focus_object`):**
   - `dof_aperture_blades == 0`, `dof_anamorphic_ratio == 1.0`, `dof_cats_eye_strength == 0.0` varsayılanları dairesel Vogel diskine indirgenir.
   - Merkezdeki odak noktası (`center_coc <= 0.35`, `dilated_near == 0`) early-out korunduğu için `center_sharp == center_bokeh` iddiası geçmeye devam eder.
4. **`NodeKind` JSON Uyumluluğu:**
   - Tüm yeni şema alanları `#[serde(default = ...)]` ile eklendiğinden mevcut `.json` projeleri (`asta_amv_project.json`, `golden_project.json`) kırılmadan deserialize edilir.

---

## Kabul Kriterleri (Faz 2 Doğrulama Kapıları)

- [ ] `cargo check --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` (0 uyarı) ve `cargo test --workspace` (187+ mevcut test + yeni Faz 6.6 PBR/IBL, Z-echo, Particle/Ribbon DoF, 2-pass CoC, Poligonal/Cat's-eye Bokeh ve Blend testleri).
- [ ] `cargo run --release -p frameshift-cli -- verify-all --out-dir target/verify_66` (`M1–M5 all_gates_passed: true`, M1 golden hash'ler korunmuş).
- [ ] Görsel doğrulama:
  1. `Text3D` `MetallicChrome` ve `Neon3D` 90° `LookAt` yan kamera açısından karanlıkta kalmadan stüdyo softbox yansıması ve bevel cavity AO sergiler; Z-echo katmanları arkadaki şeritleri derinlik tamponuyla gizlemez.
  2. `Scene3D` içinde `RibbonTrail3D` + `ParticleField3D` odak düzleminde keskin kalır (void `z=far` bozulması sıfır), opak `Text3D` tarafından Z-testiyle kesilir ve ön-plan `Text3D` bokeh halesi şeridin üstüne taşar.
  3. `1–2 px` ince ön-plan neon harfleri 2-pass H+V dilation ile eksiksiz ön-plan bokeh halesi üretir; 6-bıçak / anamorfik / cat's-eye bokeh ve yarı-şeffaf ön-plan alpha kompozisyonu görsel olarak doğrulanır.
  4. `blend.wgsl` sRGB `Overlay`, `SoftLight`, `ColorDodge`, `HardLight`, `Lighten`, `Darken` çıktıları W3C referans değerleriyle (`tol <= 1e-3`) eşleşir.
