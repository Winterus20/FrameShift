---
id: project-schema
status: locked
created: 2026-11-21
updated: 2026-11-22
related_adr: [0001, 0002, 0003, 0004]
tags: [schema, engine, timebase, agent, color, text-vector, inspection]
---

# Sürümlenmiş Proje Şeması Sözleşmesi (`project-schema`)

> **amended by [ADR-0004](../decisions/0004-schema-minor-compatibility.md):** Mevcut şema sürümü `0.2.0`'dır ve yükleme kuralı "same major, minor <= current" olarak gevşetilmiştir. Bu dosya `locked` kalır; değişiklik yalnızca bu not ve ilgili sürüm maddeleriyle sınırlıdır.

## Amaç ve Kapsam

Bu sözleşme, FrameShift motorunun tek gerçek kaynağı (Single Source of Truth) olan sürümlenmiş JSON proje modelinin (`frameshift-schema`) yapısını, değişiklik denetimini (`revision_id` / optimistic locking), DAG doğrulamasını ve M1 çekirdek alanları ile Faz 2–5 uzantılarını (`VectorText`, `Lut3D`, `ParamBindingDef`, `CreativeDecisionRecord`, 3 seviyeli `PreferenceEntry`, `ProxyConfigDef`, `HwAccelPolicy`) kilitler.

Masaüstü UI, CLI (`frameshift-cli`) ve agent/MCP katmanı (`frameshift-mcp`) aynı proje tanımını okur ve yalnızca doğrulanmış komutlar üzerinden değiştirir (bkz. [ADR-0001](../decisions/0001-independent-native-gpu-core.md)).

---

## Veri Modeli / Arayüz Sözleşmesi

### 1. Kök Proje Yapısı

```json
{
  "schema_version": "0.2.0",
  "revision_id": 1,
  "assets": [],
  "timeline": {},
  "events": [],
  "graph": {},
  "render_profiles": {},
  "param_bindings": [],
  "creative_decisions": [],
  "preferences": [],
  "proxy_config": null,
  "hw_accel_policy": null
}
```

- **`schema_version: String`:** SemVer biçimi. Mevcut sürüm `"0.2.0"`'dır (`CURRENT_SCHEMA_VERSION`); `"0.1.0"` M1 tabanıdır. **Uyumluluk kuralı (ADR-0004): same major, minor <= current** — major eşit olmalı ve gelen minor mevcut minor'dan küçük veya eşit olmalıdır. Böylece `0.1.0` projeleri yüklenmeye devam eder, `0.2.0` yeni projeler yüklenir; `0.3.0` ve `1.0.0` `UnsupportedSchemaVersion` ile reddedilir. Patch bileşeni uyumluluğu etkilemez (hata düzeltmesi; kalıcı biçim değişmez). Minor sürüm artışları isteğe bağlı varsayılanlı (`#[serde(default)]`) alan eklemelerine izin verir.
- **`revision_id: u64`:** Her başarılı mutasyonda atomik olarak `+1` artar. Tüm edit komutları, oluşturulan önizlemeler ve inceleme paketleri bu kimliği taşır.

### 2. `assets` (Medya Varlıkları)

Her varlık benzersiz `asset_id: String` ile tanımlanır:

- `asset_id: String`
- `kind: "video" | "audio" | "image"`
- `path: String` (proje köküne göre göreli veya mutlak doğrulanan dosya yolu)
- `duration_ticks: Flicks` (`i64`, `>= 0`)
- `stream_info`:
  - Video/Image için: `width: u32`, `height: u32`, `fps: Option<Rational32>`, `color_space: "DisplaySrgb" | "LinearRec709" | "Rec709Limited"`, `vfr_pts_index: Option<VfrPtsIndexRef>`
  - Audio için: `sample_rate_hz: u32`, `channels: u16`

### 3. `timeline` (Zaman Çizgisi, Klipler ve Hız Eğrileri)

Tüm zaman değerleri [timebase.md](timebase.md) uyarınca `Flicks(i64)` cinsindendir:

- `fps: Rational32` (`{ "num": 24000, "den": 1001 }` vb.)
- `duration_ticks: Flicks` (`> 0`)
- `resolution: { width: u32, height: u32 }` (referans tuval boyutu)
- `clips: Vec<Clip>`:
  - `clip_id: String`
  - `asset_id: String` (mevcut bir `assets` kaydına referans)
  - `track_id: String`
  - `output_start_tick: Flicks`
  - `output_end_tick: Flicks` (`> output_start_tick`)
  - `source_in_tick: Flicks` (`>= 0`)
  - `source_out_tick: Flicks`
  - `speed_curve: Option<SpeedCurve>` (yoksa sabit `1.0x`)
  - `audio_retime_mode: "mute" | "varispeed" | "pitch_preserving"`
- **`speed_curve`:**
  - Çıktı zaman aralığındaki göreli zamanı (`0 .. clip_output_duration`) kaynak zamana eşleyen parçalı Bézier / Hermite veya anahtar kareli hız eğrisi (`keyframes: Vec<{ output_offset_ticks: Flicks, speed: f64, interp: "linear" | "bezier" | "hold" }>`).
  - İleri (`> 0`), yavaşlama/hızlanma rampası, donma (`0.0`) ve geri sarma (`< 0`) değerlerini destekler; hız sınırları (`min_speed`, `max_speed`) açıkça tanımlanabilir.

### 4. `events` (Anlamsal Olay ve Kısıt Ankrajları)

[ADR-0003](../decisions/0003-semantic-event-constraint-timing.md) gereği klip/müzik senkronu olay kimlikleri üzerinden bağlanır:

- `event_id: String` (benzersiz)
- `label: String` (örn. `"drop_impact_1"`)
- `kind: "impact" | "beat" | "cut" | "custom"`
- `source_anchor_tick: Option<Flicks>` (kaynak klipteki mutlak/göreli an)
- `target_output_tick: Flicks` (çıktı zaman çizgisindeki hedef an)
- `clip_id: Option<String>`
- `provenance: "user" | "detected" | "agent"`
- Parametre/efekt animasyonları sabit saniye yerine `{ "event_id": "...", "offset_ticks": Flicks }` biçiminde olaya referans verebilir.

### 5. `graph` (Dallanabilen Render Grafiği — DAG)

Render grafiği döngüsüz yönlü grafiktir (DAG):

- `output_node_id: String` (kök/nihai çıkış düğümü)
- `nodes: Vec<GraphNode>` — her düğüm `{ node_id, kind, params }` taşır.
- `edges: Vec<GraphEdge>` — `{ from_node, from_port, to_node, to_port }`.

**Faz 0 / M1 Kilitli Çekirdek Düğüm Tipleri (`kind`):**

1. `SourceVideo` — `clip_id` referansı ile zaman çizgisi + hız eğrisi üzerinden kaynak kare okur.
2. `Solid` — Düz renk / alfa üreteci (`color_rgba: [f32; 4]` Linear Rec.709).
3. `Transform` (`Transform2D`) — Konum (normalize/referans tuval oranı), ölçek, dönüş (radyan), anchor noktası, filtre modu (`bilinear` | `catmull_rom`).
4. `MaskMatte` — Ana giriş + maske girişi (`alpha` veya `luma` matte, `invert`, `feather`).
5. `Blend` — `base` + `top` girişi (`normal`, `add`, `screen`, `multiply`, `overlay`, `opacity`).
6. `ColorBasic` — Renk uzayı dönüşümü (`sRGB <-> Linear Rec.709`), `exposure`, `contrast`, `saturation`, `flash_amount` / glow eşiği.
7. `ShaderTransition` — İki klip/alt-grafik girişli (`tex_a`, `tex_b`) WGSL geçiş düğümü (`progress`, `direction_rad`, `whip_strength`, `chromatic_aberration`, `impact_flash` veya özel shader kimliği).
8. `MotionBlurShutter` — Alt-kare temporal motion blur akümülatörü (`shutter_angle_deg`, `sub_samples`, `phase: "centered" | "trailing"`).

**Faz 2+ Kilitli Genişleme Düğüm Tipleri (`kind`):**

9. `VectorText` *(0 giriş — üreteç)* — Tipografi ve 2D vektör/şekil rasterizasyon düğümü (`frameshift-vector`):
   - `text: String`, `font_family: String`, `font_weight: u16` (varsayılan `400`), `font_size_norm: f32` (referans yüksekliğe normalize oran), `position: [f32; 2]`, `anchor: [f32; 2]`, `align: "left" | "center" | "right"`, `color_rgba: [f32; 4]` (`LinearRec709`), `stroke: Option<{ color_rgba: [f32; 4], width_norm: f32 }>`, `letter_spacing_norm: f32`, `line_height: f32`, `vector_paths: Vec<VectorPathDef>` (isteğe bağlı SVG/Bézier yolları).
10. `Lut3D` *(1 giriş — `input`)* — `Rgba16Float` trilinear 3D LUT / OpenColorIO uyumlu renk dönüşüm düğümü:
    - `lut_source`: `{ kind: "cube_file", path: String }` veya `{ kind: "ocio", config_path: String, src_space: String, dst_space: String, look: Option<String> }`
    - `lut_size: u32` (`>= 2`, tipik `17` / `33` / `65`), `domain_min: [f32; 3]`, `domain_max: [f32; 3]`, `intensity: f32` (`[0.0..=1.0]`), `input_space: ColorSpace`, `output_space: ColorSpace`.

*(Ayrıca `SourceImage`, `ShaderEffect` ve `Precomp` düğümleri aynı DAG ve [render-graph.md](render-graph.md) `Rgba16Float` sözleşmesini izler.)*

### 5.1. `ParamBindingDef` (Parametre Bağlama ve `SafeExpr` Sözleşmesi — Faz 2)

Düğüm parametrelerini müzik enerjisine (`audio_rms`, `beat_phase`), anlamsal olay mesafesine (`event_delta_sec`) veya güvenli matematiksel ifadelere ([effect-shader-api.md](effect-shader-api.md)) bağlar:

```json
{
  "binding_id": "bind_glow_drop",
  "target_node_id": "grade_main",
  "parameter": "flash_amount",
  "event_id": "drop_impact_1",
  "expr": "clamp(audio_rms * 0.8 + max(0.0, 1.0 - abs(event_delta_sec) * 6.0), 0.0, 1.5)",
  "clamp_min": 0.0,
  "clamp_max": 1.5,
  "fallback_value": 0.0
}
```

- `binding_id: String` (benzersiz), `target_node_id: NodeId`, `parameter: String`
- `event_id: Option<String>` (`event_delta_sec` değişkeninin referans aldığı olay)
- `expr: String` (`SafeExpr` ayrıştırıcısı tarafından yakıt/derinlik sınırıyla doğrulanır)
- `clamp_min: f64`, `clamp_max: f64` (`clamp_min <= clamp_max`), `fallback_value: f64` (`is_finite()`).

### 6. `render_profiles` (`draft` ve `final`)

```json
{
  "draft": {
    "resolution_scale": 0.5,
    "motion_blur_sub_samples": 4,
    "sampling_quality": "bilinear",
    "audio_sample_rate_hz": 48000
  },
  "final": {
    "resolution_scale": 1.0,
    "motion_blur_sub_samples": 16,
    "sampling_quality": "bicubic",
    "audio_sample_rate_hz": 48000
  }
}
```

- **Önizleme / Final Semantik Eşdeğerliği:** `draft` ve `final` profilleri yalnızca piksel çözünürlük ölçeği, motion blur alt-örnek sayısı (`sub_samples`) ve filtre kalitesinde farklılaşabilir; zamanlama, FPS, hız eğrisi, maske sınırları ve normalize koordinat anlamı değişmez.

### 7. Mutasyon, Optimistic Locking ve Undo/Redo Sözleşmesi

- Her proje mutasyonu `apply_command(expected_revision_id: u64, cmd: ProjectCommand)` imzasıyla uygulanır.
- `expected_revision_id != project.revision_id` ise işlem uygulanmaz ve `RevisionConflict { expected, actual }` döndürülür.
- Başarılı mutasyon geri dönüşümlü ters komut (inverse command) üretir ve undo/redo yığınına eklenir; bu geçmiş UI bileşenlerinin dışında, çekirdek proje oturumunda tutulur.

### 8. Yaratıcı Karar Günlüğü ve 3 Seviyeli Tercih Modeli (Faz 3)

#### 8.1. `CreativeDecisionRecord` (Yaratıcı Karar Kaydı)
Önemli yaratıcı kararların gerekçesini ve korunacak özelliklerini proje sürümüne bağlayarak sonraki revizyonlarda kaybolmasını önler ([PROJECT_NOTES.md §5.3](../../PROJECT_NOTES.md#L180)):

- `decision_id: String` (benzersiz)
- `revision_id: u64`
- `scope_range: { start_tick: Flicks, end_tick: Flicks }`
- `approach: String` (örn. `"character_expression_focus"`)
- `rationale: String`
- `evidence_refs: Vec<String>` (ilgili klip, `event_id` veya inceleme paketi kimlikleri)
- `protected_invariants: Vec<String>` (ör. `"Yüz ifadesini kapatan yoğun grafik ekleme"`)
- `risks_to_check: Vec<String>` (ör. `"Sonraki yoğun drop sahnesine geçiş kontrastını kontrol et"`)
- `status: "proposed" | "approved" | "superseded"`

#### 8.2. `PreferenceEntry` (3 Seviyeli Kullanıcı Tercihi: `Global`, `Project`, `Scene`)
Kullanıcı geri bildirimini açık, düzenlenebilir JSON/YAML kayıtlarına ayırır; tek sahnelik bir düzeltme asla genel tercihe sızmaz:

- `preference_id: String` (benzersiz)
- `tier: "global" | "project" | "scene"`
- `scope_ref: Option<String>` (`global` için `null`; `project` için `project_id`; `scene` için `scene_id` veya `[start_tick, end_tick]` aralığı)
- `key: String` (örn. `"camera_shake.max_intensity"`)
- `value: serde_json::Value`
- `rationale: String`
- `source_feedback: Option<String>`
- `active: bool`
- **Öncelik Kuralı:** Çatışma halinde dar kapsam geniş kapsamı geçersiz kılar (`Scene > Project > Global`).

### 9. Proxy ve Donanım Hızlandırma Politikaları (Faz 5)

#### 9.1. `ProxyConfigDef`
Hızlı önizleme için proxy üretim ve eşleme politikasını tanımlar:

- `enabled: bool`
- `resolution_scale: f32` (veya `max_height_px: u32`, örn. `720`)
- `codec: "h264_intra" | "prores_proxy" | "dnxhr_lb"` (all-intra seek hızı için)
- `storage_dir: String`
- `semantic_equivalence_verified: bool`
- **Değişmez:** Proxy dosyaları orijinal klibin `Flicks` süresini, `Rational32` / `VfrPtsIndex` zaman haritasını ve normalize `[0, 1]` koordinat anlamını birebir korumak zorundadır; yalnızca piksel çözünürlüğü düşer.

#### 9.2. `HwAccelPolicy`
Platforma özgü zero-copy donanım decode/encode ve deterministik yazılım fallback sınırını tanımlar:

- `mode: "auto" | "prefer_hw" | "software_only"`
- `decode_backends: Vec<"d3d11va" | "nvdec" | "videotoolbox" | "vaapi">`
- `encode_backends: Vec<"nvenc" | "amf" | "qsv" | "videotoolbox" | "vaapi">`
- `zero_copy_required: bool`
- `fallback_to_software: bool`
- **Sessiz Düşme Yasağı:** `zero_copy_required == true` ve `fallback_to_software == false` iken donanım hattı başlatılamazsa motor sessizce yazılıma düşmez, `HwAccelUnavailable` hatası döndürür. Yazılım fallback'i kullanıldığında aktif backend inceleme teknik raporuna (`inspection-contract.md`) açıkça yazılır.

---

## Kenar Durumlar ve Hata Davranışı

- **Desteklenmeyen Şema Sürümü:** Major sürüm eşleşmezse veya gelen minor mevcut minor'dan büyükse `UnsupportedSchemaVersion` hatası döndürülür (ADR-0004). Daha eski minor'lar `#[serde(default)]` alanlarıyla yüklenir.
- **Kırık Referanslar:** Var olmayan `asset_id`, `clip_id`, `event_id` veya `node_id` referansları doğrulamada `MissingReference` hatası verir.
- **DAG Döngüsü (Cycle):** `graph` içinde döngü veya ulaşılamaz `output_node_id` tespit edildiğinde `GraphCycleDetected` / `InvalidGraphOutput` hatası ile reddedilir.
- **Çakışan Revizyon (`expected_revision_id` uyuşmazlığı):** Sessiz üzerine yazma yasaktır; `RevisionConflict` döndürülür.
- **Geçersiz Zaman/Hız Sınırları:** `output_end_tick <= output_start_tick` veya tanımsız profil adı (`profile != draft|final` ve özel profil bulunamadıysa) açık hata verir.
- **Geçersiz `ParamBindingDef` / `PreferenceEntry` / `HwAccelPolicy`:** `clamp_max < clamp_min`, geçersiz `SafeExpr`, `tier == "scene"` iken boş `scope_ref`, veya `zero_copy_required && !fallback_to_software` iken donanım backend'i olmaması açık doğrulama/çalışma hatasıyla reddedilir.

---

## Kabul Kriterleri (Kod Aşamasında Doğrulanacak)

- [ ] Geçerli bir Golden Scene projesi (`SourceVideo` + `SpeedCurve` + `MaskMatte` + `ShaderTransition` + `ColorBasic` + `MotionBlurShutter` + `events` + `draft`/`final` profilleri) JSON'a serileştirilip kayıpsız geri yüklenir (round-trip eşitliği).
- [ ] `VectorText`, `Lut3D`, `ParamBindingDef`, `CreativeDecisionRecord`, 3 seviyeli `PreferenceEntry`, `ProxyConfigDef` ve `HwAccelPolicy` alanlarını içeren Faz 2–5 projesi serileştirme/geri yükleme round-trip testini kayıpsız geçer; `0.1.0` ve `0.2.0` projeleri yüklenir (`0.1.0` varsayılanlarla), `0.3.0` ve `1.0.0` reddedilir.
- [ ] Döngülü grafik, kırık `asset_id`/`clip_id` referansı ve uyumsuz `schema_version` şema doğrulamasından hata ile reddedilir.
- [ ] `apply_command` eski `revision_id` ile çağrıldığında `RevisionConflict` döner ve proje değişmez; geçerli undo/redo zinciri `revision_id` ve proje durumunu deterministik biçimde geri alır.
- [ ] Tüm kalıcı zaman alanlarında `f64` saniye yerine yalnızca `Flicks(i64)` ve `Rational32` serileştirildiği şema testleriyle doğrulanır.
