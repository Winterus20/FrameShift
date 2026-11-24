# FrameShift — Çekirdek Motor Sözleşmeleri (`brain/specs/`)

Bu dizin, FrameShift'in **9-crate Rust + `wgpu`/WGSL** çekirdeğinin kilitli temel sözleşmelerini barındırır. [AGENTS.md](../../AGENTS.md) kısa tutulurken tüm ayrıntılı veri yapıları, sınır koşulları ve kabul kriterleri burada tanımlanır.

---

## Çekirdek Sözleşmeler Durumu

| Sözleşme | Durum | İlişkili ADR | Kapsam Özeti (Kilitli Temel + Güncel Kod Uzantıları) |
| :--- | :---: | :--- | :--- |
| [timebase.md](timebase.md) | `locked` | [0002](../decisions/0002-rational-timebase-flicks.md), [0003](../decisions/0003-semantic-event-constraint-timing.md) | `Flicks(i64)` (`705_600_000/sn`), `Rational32` FPS, `i128` ara aritmetik, VFR PTS indeksleme, ses `44.1/48/96 kHz` ve retime; kodda `AnimeSnap`, `CubicBezierSpeed` ve P1 Kahn DAG `EventTimingConstraint` çözümleyicisi ile genişletilmiştir |
| [project-schema.md](project-schema.md) | `locked` | [0001](../decisions/0001-independent-native-gpu-core.md), [0002](../decisions/0002-rational-timebase-flicks.md), [0003](../decisions/0003-semantic-event-constraint-timing.md), [0004](../decisions/0004-schema-minor-compatibility.md) | Sürümlenmiş JSON (`0.1.0` temel → ADR-0004 ile güncel `0.2.0`, `minor <= current`), `revision_id`, `CreativeDecisionRecord`, `PreferenceEntry`, `ProxyConfigDef`, `HwAccelPolicy`; kodda **40 `NodeKind`**, `Project.keyframe_tracks` (P5), `Motion.md` DSL derleyici ve `ParametricTemplateSpec` (P2) eklenmiştir |
| [render-graph.md](render-graph.md) | `locked` | [0001](../decisions/0001-independent-native-gpu-core.md) | `Rgba16Float` premultiplied-alpha, `sRGB <-> Linear Rec.709`, sub-frame temporal motion blur, `VectorText`, `Lut3D` (3D LUT/OCIO), seek-independent DAG; canlı motor bugün **60 WGSL shader**, `Scene3D`/`Depth32Float`, `AgX`/`PbrNeutral`, `4x MSAA`/FXAA, `OpticalFlowBlur`, `Anime4K`, `yuv_to_linear.wgsl` (P3 NV12) ve 3-katmanlı `NodeContentHash` Smart Cache içerir |
| [effect-shader-api.md](effect-shader-api.md) | `locked` | [0001](../decisions/0001-independent-native-gpu-core.md), [0002](../decisions/0002-rational-timebase-flicks.md), [0003](../decisions/0003-semantic-event-constraint-timing.md) | `@group(0)` WGSL bağlama düzeni, 16-byte uniform hizalama, yakıt sınırlı `SafeExpr`, Naga doğrulama ve TDR döngü güvenliği; kodda 60 WGSL shader, 10 yerleşik `ShaderTransition`, runtime `CustomWgsl` ve P1 `SafeExpr` (`spring`, `wiggle`, karşılaştırmalar, mantık operatörleri, kısa devreli `if`/`select`) bulunur |
| [inspection-contract.md](inspection-contract.md) | `locked` | [0001](../decisions/0001-independent-native-gpu-core.md), [0003](../decisions/0003-semantic-event-constraint-timing.md) | Ortak `revision_id` + `tick` kimlikli 4 inceleme yüzeyi (Visual, Video, Code, Technical), Contact Sheet, 4-panel MaskPack, 3-panel TransitionPack, VelocityPack, FullResCrop ve epistemik sınırlar; kodda ek olarak `ui-snapshot`, `materials`, `waveform` PNG, `lint_project`, `diff_frame` ve **32 MCP aracı** vardır |

---

## Kilitli Sözleşme vs. Güncel Kod Senkron Notları

Kilitli (`locked`) spec dosyaları geriye dönük değiştirilmez; sonraki fazlarda eklenen doğrulanmış kod farkları aşağıda izlenir:
- **Şema Sürümü (`project-schema.md` vs. `version.rs`):** Spec `0.1.0` temel şemayı kilitler; [ADR-0004](../decisions/0004-schema-minor-compatibility.md) ile `CURRENT_SCHEMA_VERSION = 0.2.0` (`minor <= current` uyumluluk kuralı) yürürlüktedir.
- **`SafeExpr` Derinliği ve Operatörleri (`effect-shader-api.md` vs. `expr.rs`):** Spec AST derinlik sınırını `16` olarak belgelese de `crates/frameshift-time/src/expr.rs` içinde `MAX_EXPR_DEPTH = 64` (ve `MAX_EXPR_NODES = 1024`, `DEFAULT_MAX_FUEL = 256`) tanımlıdır; ayrıca `audio_sub`, `audio_mid`, `audio_high`, `downbeat_phase` değişkenleri ile P1 fonksiyon/operatörleri (`spring`, `wiggle`, `mix`, `ceil`, `sqrt`, `log`, `%`, `< <= > >= == !=`, `&& || !`, `if`/`select`) eklenmiştir.
- **Düğüm ve Shader Kapsamı:** Faz 6.1–6.6, Beş Temel Yetenek ve P1–P3/P5 ile motor **40 `NodeKind`** ve **60 WGSL shader** düzeyine çıkmıştır; tüm yeni düğümler kilitli `Rgba16Float` Linear Rec.709, deterministik seek-independent DAG ve `Flicks(i64)` zaman invariants'larını korur.

---

## Sözleşme Kuralları

- Yeni spec oluştururken [../_templates/spec.md](../_templates/spec.md) şablonunu kullan.
- Her spec belgesinde `status: draft | review | locked` durumu, sınır/edge-case listesi ve kabul testi ölçütleri açıkça yazılır.
- Sözleşme değiştiğinde [../index.md](../index.md) ve [../state.md](../state.md) güncellenir; kilitli sözleşmeler doğrudan değiştirilmek yerine yeni ADR veya yeni sürüm sözleşme ile genişletilir.
