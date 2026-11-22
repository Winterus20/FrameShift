# FrameShift — Çekirdek Motor Sözleşmeleri (`brain/specs/`)

Bu dizin, kodlamaya geçilmeden önce kilitlenmesi gereken çekirdek motor sözleşmelerini barındırır. `AGENTS.md` kısa kalırken tüm ayrıntılı veri yapıları, sınır koşulları ve kabul kriterleri burada tanımlanır.

---

## Çekirdek Sözleşmeler Durumu

| Sözleşme | Durum | İlişkili ADR | Kapsam Özeti |
| :--- | :---: | :--- | :--- |
| [timebase.md](timebase.md) | `locked` | [0002](../decisions/0002-rational-timebase-flicks.md), [0003](../decisions/0003-semantic-event-constraint-timing.md) | `Flicks(i64)` (`705_600_000/sn`), `Rational32` FPS, `i128` ara aritmetik, VFR PTS indeksleme, ses `44.1/48/96 kHz` ve retime |
| [project-schema.md](project-schema.md) | `locked` | [0001](../decisions/0001-independent-native-gpu-core.md), [0002](../decisions/0002-rational-timebase-flicks.md), [0003](../decisions/0003-semantic-event-constraint-timing.md) | Sürümlenmiş JSON (`0.1.0`+Faz 2–5 uzantıları), `revision_id`, `VectorText`, `Lut3D`, `ParamBindingDef`, `CreativeDecisionRecord`, 3 seviyeli `PreferenceEntry`, `ProxyConfigDef`, `HwAccelPolicy` |
| [render-graph.md](render-graph.md) | `locked` | [0001](../decisions/0001-independent-native-gpu-core.md) | `Rgba16Float` premultiplied-alpha, `sRGB <-> Linear Rec.709`, sub-frame temporal motion blur, `VectorText`, `Lut3D` (trilinear 3D LUT/OCIO), seek-independent DAG |
| [effect-shader-api.md](effect-shader-api.md) | `locked` | [0001](../decisions/0001-independent-native-gpu-core.md), [0002](../decisions/0002-rational-timebase-flicks.md), [0003](../decisions/0003-semantic-event-constraint-timing.md) | `@group(0)` WGSL bağlama düzeni, 16-byte uniform hizalama, `SafeExpr` yakıt sınırlı ifade dili, Naga doğrulama ve TDR döngü güvenliği |
| [inspection-contract.md](inspection-contract.md) | `locked` | [0001](../decisions/0001-independent-native-gpu-core.md), [0003](../decisions/0003-semantic-event-constraint-timing.md) | Ortak `revision_id` + `tick` kimlikli 4 inceleme yüzeyi (Visual, Video, Code, Technical), Contact Sheet, 4-panel MaskPack, 3-panel TransitionPack, VelocityPack, FullResCrop ve epistemik sınırlar |

---

## Sözleşme Kuralları

- Yeni spec oluştururken [../_templates/spec.md](../_templates/spec.md) şablonunu kullan.
- Her spec belgesinde `status: draft | review | locked` durumu, sınır/edge-case listesi ve kabul testi ölçütleri açıkça yazılır.
- Sözleşme değiştiğinde `brain/index.md` ve `brain/state.md` güncellenir.
