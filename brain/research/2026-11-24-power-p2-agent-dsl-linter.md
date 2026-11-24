---
title: "Pillar 2 Araştırması: Ajan Verimliliği — lint_project + diff_frame, Deklaratif Motion.md DSL ve Parametrik Şablonlar"
date: 2026-11-24
status: implemented
tags: [agent, schema, inspection, cli, mcp, timebase, render-graph]
---

# Pillar 2 Araştırması: Ajan Verimliliği (`lint_project` + `diff_frame`, `Motion.md` DSL ve Parametrik Şablonlar)

> **Uygulama Durumu (2026-11-24):** **0 yeni Cargo crate** ile implemente edildi ve doğrulandı ([uygulama günlüğü](../log/2026-11-24-competitive-power-roadmap-implementation.md)): 10 kurallı `lint_project` + opt-in `blackdetect`, MAE/PSNR/bbox & 3-panelli fark PNG üreten `diff_frame`, `Motion.md` deklaratif DSL parser+compiler (`compile-dsl` / `compile_motion_dsl`), BPM/downbeat uyarlamalı `ParametricTemplateSpec` (`template-metadata` / `template-instantiate`) ve ilgili CLI/MCP araçları.

## 1. Amaç ve Kısıtlar

Bu tasarım belgesi, [rakip güç analizi](2026-11-24-competitive-power-analysis-roadmap.md#L33-L44) yol haritasının **Pillar 2 (Ajan Verimliliği ve Üretim Hızı)** bileşenlerini **0 yeni Cargo crate** ekleyerek mevcut 9-crate Rust workspace'ine entegre edecek teknik mimariyi tanımlar:

1. **`lint_project` + `diff_frame`:** [lessons.md](../lessons.md#L8) içinde belgelenen 4 sessiz kurgu tuzağını ve yapısal/performans hatalarını yakalayan statik+opsiyonel medya denetçisi ve piksel/bbox fark aracı.
2. **`Motion.md` Tarzı Deklaratif Kurgu DSL Derleyicisi:** İnsan/ajan okunabilir kısa sahne reçetesini deterministik olarak geçerli `schema_version: "0.2.0"` (`i64` Flicks) FrameShift `Project` JSON DAG'ına derleyen saf Rust parser + compiler.
3. **Parametrik Şablonlar (`calculateMetadata` + tipli `props`):** Yeni müzik + klip listesi verildiğinde `MultibandBeatReport` (`estimated_bpm`, `beat_grid`, `downbeats`, `sub.onsets`) ve medya sürelerine göre toplam süre, kesim sayısı, drop anları ve efekt pencerelerini deterministik hesaplayıp geçerli proje üreten `ParametricTemplateSpec` motoru.

**0-Crate Fizibilitesi (Doğrulandı):**
- Workspace kök [Cargo.toml](../../Cargo.toml#L20-L33) ve alt crate'lerde (`frameshift-schema`, `frameshift-media`, `frameshift-core`, `frameshift-app`, `frameshift-cli`, `frameshift-mcp`) hâlihazırda `serde`, `serde_json`, `thiserror`, `anyhow` ve `frameshift-media/Cargo.toml:10` içinde `png = "0.18"` mevcuttur.
- DSL parser (satır/frontmatter tabanlı), `diff_frame` (RGB/RGBA MAE/PSNR/bbox + PNG çıktı), `lint_project` (AST/DAG tarama + `FfmpegBridge` `blackdetect` alt-süreci) ve parametrik metadata hesaplayıcısı **ek hiçbir crate olmadan** tamamen saf Rust + mevcut `ffmpeg`/`ffprobe` köprüsüyle uygulanabilir.

---

## 2. İncelenen Mevcut Kod Tabanı (Satır Referansları)

| Bileşen | Dosya ve Satır | Mevcut Durum ve Pillar 2 Boşluğu |
| :--- | :--- | :--- |
| Şema sürümü | [version.rs](../../crates/frameshift-schema/src/version.rs#L5-L42) | `CURRENT_SCHEMA_VERSION = 0.2.0`; yükleme kuralı `same major, minor <= current`. |
| Proje doğrulama | [project.rs](../../crates/frameshift-schema/src/project.rs#L769-L1006) | `Project::validate` yalnızca sert DAG/tip/arity/binding adı ve `SafeExpr::parse` hatasını yakalar; sessiz görsel/koordinat/renk/performans tuzaklarını raporlamaz. |
| Bindable parametre tablosu | [project.rs](../../crates/frameshift-schema/src/project.rs#L331-L658) | `bindable_param_names(&NodeKind)` tüm 37 `NodeKind` varyantının geçerli binding adlarını kilitler (`crates/frameshift-schema` içi `pub(crate)`). |
| Düğüm ve renk modeli | [graph.rs](../../crates/frameshift-schema/src/graph.rs#L502-L644) · [compositor.rs](../../crates/frameshift-render/src/compositor.rs#L2556-L2566) | `Solid` `color_space` taşır (`DisplaySrgb` ise `premul_srgb_to_linear` uygulanır); `VectorText` (`graph.rs:579`) doğrudan Linear Rec.709 bekler. |
| Metin koordinat modeli | [font_locator.rs](../../crates/frameshift-vector/src/font_locator.rs#L438-L443) · [graph.rs](../../crates/frameshift-schema/src/graph.rs#L2236-L2255) | `x_offset_norm`/`y_offset_norm` kare oranıdır (`0.5` = merkez); şema yalnızca `is_finite()` denetler (`<0` veya `>1` kabul edilir). |
| `MotionBlurShutter` | [compositor.rs](../../crates/frameshift-render/src/compositor.rs#L1848-L1882) | `sub_samples` kadar alt-grafik yeniden değerlendirilir; yüksek `sub_samples` veya iç içe blur için uyarı yoktur. |
| `CustomWgsl` ve `Precomp` | [graph.rs](../../crates/frameshift-schema/src/graph.rs#L1895-L1913,L2647-L2707) · [template.rs](../../crates/frameshift-schema/src/template.rs#L610-L627) | `TemplatePackage`/`EffectChainPreset` recursive `custom_wgsl_node_path` ile reddeder; normal `Project::validate` nested `Precomp` içindeki `CustomWgsl`'i uyarmaz. |
| `author_hard_cuts` & `plan_autocut` | [session.rs](../../crates/frameshift-schema/src/session.rs#L769-L807,L976-L1055) | Sıralı klip + `Blend(Normal)` üretir; efekt, geçiş, metin, event anchor veya ses ekleyemez. |
| `EffectChainPreset` & splice | [template.rs](../../crates/frameshift-schema/src/template.rs#L47-L61,L380-L463) · [workbench.rs](../../crates/frameshift-core/src/workbench.rs#L493-L920) | 5+ hazır zincir (`xenoz_impact_glow`, `anime_impact_shake`, `impact_flash` vb.) `splice_effect_chain_preset` ile grafa eklenebilir. |
| `TemplatePackage` | [template.rs](../../crates/frameshift-schema/src/template.rs#L28-L196,L279-L313) · [cli/workbench.rs](../../crates/frameshift-cli/src/workbench.rs#L703-L867) | Sabit topolojili proje üzerinde `TemplateSlot` ikamesi yapar; yeni müzik/klip listesine göre süre ve kesim sayısı hesaplayamaz. |
| `preflight` | [workbench.rs](../../crates/frameshift-core/src/workbench.rs#L191-L393) | Yalnızca eksik varlık/font/LUT, anchor aralığı ve `ffmpeg`/`ffprobe`/`wgpu` kontrolü yapar. |
| `MultibandBeatReport` | [spectral.rs](../../crates/frameshift-media/src/spectral.rs#L176-L190) | `estimated_bpm`, `beat_grid`, `downbeats`, `sub`/`mid`/`high` `BandOnsetSummary` sağlar. |
| `Rgba8Frame` & PNG I/O | [frame.rs](../../crates/frameshift-media/src/frame.rs#L20,L175-L206) | `save_png` ve `load_png` (`png = "0.18"`) hazırdır; iki kare arası PSNR/MAE/bbox fark aracı yoktur. |
| `SafeExpr` | [expr.rs](../../crates/frameshift-time/src/expr.rs#L53-L64,L129-L194) | 10 değişken (`t_sec`, `output_tick`, `beat_phase`, `downbeat_phase`, `audio_rms`, `audio_sub`, `audio_mid`, `audio_high`, `event_delta_sec`, `progress`) ve 19 fonksiyon (`smoothstep`, `step`, `clamp`, `lerp`, `cubic_bezier`, `ease_*` vb.). |
| CLI & MCP | [main.rs](../../crates/frameshift-cli/src/main.rs#L92-L164) · [cli/workbench.rs](../../crates/frameshift-cli/src/workbench.rs#L35-L102) · [server.rs](../../crates/frameshift-mcp/src/server.rs#L262-L549,L1310-L1460) | Manuel `args.split_first()` CLI yönlendirmesi ve JSON-RPC 2.0 `McpToolDescriptor` / `dispatch_tool` yapısı. |

---

## 3. Alt Sistem 1: `lint_project` + `diff_frame`

### 3.1. `lint_project` Mimarisi (`frameshift-core` + `frameshift-media`)

`preflight()` ([workbench.rs:234](../../crates/frameshift-core/src/workbench.rs#L234)) çalışma ortamı/varlık varlığını doğrular; **`lint_project()`** ise geçerli (veya yüklenebilen) bir `Project` üzerinde **sessiz görsel hataları, anlamsal tutarsızlıkları ve GPU performans tuzaklarını** tarar.

#### Veri Modeli

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LintSeverity {
    Error,   // Kesin bozuk çıktı / çalışmayan düğüm / yasak sandbox
    Warning, // Sessiz görsel tuzak (lessons.md) veya ağır performans riski
    Info,    // İyileştirme / optimizasyon önerisi
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LintIssue {
    pub code: &'static str,              // Örn. "LNT-TXT-001"
    pub severity: LintSeverity,
    pub category: &'static str,          // "silent_trap" | "dag" | "timing" | "security" | "perf" | "media"
    pub node_path: Option<String>,       // Örn. "root/precomp_1/title_a"
    pub object_id: Option<String>,       // clip_id, event_id, asset_id
    pub tick_range: Option<[i64; 2]>,    // [start_tick, end_tick)
    pub message: String,
    pub remediation: String,             // Somut düzeltme önerisi (JSON patch/CLI ipucu)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LintOptions {
    pub check_media: bool,               // true -> ffmpeg blackdetect + ffprobe duration doğrulaması
    pub strict_sandbox: bool,            // true -> nested CustomWgsl Warning yerine Error olur
    pub black_min_duration_sec: f64,     // varsayılan: 0.04 (yaklaşık 1 kare @ 24fps)
    pub black_pixel_threshold: f64,      // varsayılan: 0.08 (ffmpeg blackdetect pix_th)
    pub max_blur_sub_samples_warn: u32,  // varsayılan: 16
}
```

#### Tam Lint Kural Kataloğu

##### A. [brain/lessons.md](../lessons.md#L8) Kaynaklı 4 Sessiz Kurgu Tuzağı

| Kural Kodu | Seviye | Tespit Koşulu (Tam Mantık) | Düzeltme Önerisi (`remediation`) |
| :--- | :--- | :--- | :--- |
| **`LNT-MED-001`** | `Warning` (tam klip siyahsa `Error`) | `check_media = true` iken her `AssetKind::Video` (`ResolvedAssetSource::Disk`) için `FfmpegBridge::detect_black_intervals(path, min_dur, pix_th)` çalıştırılır: `ffmpeg -hide_banner -nostats -i <path> -vf blackdetect=d=<d>:pix_th=<th > -an -f null -`. Çıkan `[black_start_ticks, black_end_ticks)` aralıkları herhangi bir `ClipDef`'in `[source_start, source_end)` penceresiyle kesişiyorsa raporlanır (`100%` kesişim = `Error`). | Kaynak `source_start` değerini siyah aralık dışına kaydır veya klibi böl. |
| **`LNT-TXT-001`** | `Warning` (`<0.0` veya `>1.0` tam taşmada `Error`) | Her `NodeKind::VectorText` (recursive `Precomp` dahil) için `y_offset_norm` ve `x_offset_norm` kare oranı olarak denetlenir ([font_locator.rs:438-443](../../crates/frameshift-vector/src/font_locator.rs#L438-L443)): (1) `y_offset_norm == 0.0` (0 merkez sanılmış olabilir — üst kenara yapışır), (2) `y_offset_norm < 0.02` veya `> 0.98`, (3) `x_offset_norm` varsa `< 0.0` veya `> 1.0`, (4) ilgili `param_bindings` içinde `x_offset_norm`/`y_offset_norm` sabit literal veya `[0, 1]` dışı aralık üretiyorsa (`SafeExpr` sınır taraması). | `0.5` değerini ekran merkezi olarak kullan; alt üçlü için `0.78..0.85` kullan. |
| **`LNT-CLR-001`** | `Warning` | (1) `Solid { color_rgba, color_space: ColorSpace::LinearRec709 }`: RGB bileşenleri `(0.0..1.0)` aralığında ama tipik sRGB değerleri (örn. `#1a1a1a` -> `0.10` gibi veya kullanıcı/ajan tarafından sRGB hex'in `/255.0` ile direkt yazıldığı şüpheli durumlar — kesin olan: `SourceVideo`/`SourceImage` `color_space == LinearRec709` ise ve standart `.mp4`/`.png` dosyasıysa uyarı verilir). (2) `VectorText` (`color_rgba`, `outline_rgba`, `color_bottom_rgba`), `Text3D`, `Shape3D`: `color_space` alanı olmadığından motor doğrudan **Linear Rec.709** okur ([compositor.rs:2899](../../crates/frameshift-render/src/compositor.rs#L2899)); tüm RGB bileşenleri `> 0.25 && < 0.95` olan orta-ton renklerde sRGB→Lineer hatırlatma + `pow(c, 2.2)` fark uyarısı (`Info`/`Warning`), `Solid` üzerinde `DisplaySrgb` kullanılmamışsa `Warning` üretilir. | `Solid` için `color_space: "display_srgb"` seç; `VectorText` renklerini `srgb_to_linear` (`frameshift_media::srgb_to_linear`) ile dönüştür. |
| **`LNT-CMP-001`** | `Error` | İki durum yakalanır: (1) `Solid { color_rgba: [_,_,_,a], .. }` düğümünde `a >= 0.99` (tam opak) ve bu düğüm doğrudan bir `Blend { mode: Normal, opacity >= 0.99 }` düğümünün **`inputs[1]` (top)** girişine bağlıysa ve `inputs[0]` (base) altında `SourceVideo`/`SourceImage` varsa alt videoyu tamamen gizler. (2) Bir `VectorText`/`VectorPathHud` zinciri opak `Solid` taban (`a > 0.01`) üzerine `Blend(Normal)` ile bindirilip ana video dalının üstüne `Blend(Normal, 1.0)` ile giriyorsa videoyu kapatır ([lessons.md:8](../../brain/lessons.md#L8)). | Metin zincirini `Solid { color_rgba: [0,0,0,0] }` şeffaf tabandan başlat veya `VectorText` çıktısını doğrudan `Blend.inputs[1]` olarak bağla. |

##### B. Yapısal, Zamanlama, Güvenlik ve Performans Kuralları

| Kural Kodu | Seviye | Tespit Koşulu |
| :--- | :--- | :--- |
| **`LNT-DAG-001`** | `Warning` | **Ölü / Erişilemeyen Düğüm:** `output_node`'dan geriye doğru (`inputs`, `Lut3D.input`, `Transform3D/Null3D.parent`, `Scene3D.depth_plate.node_id`) ulaşılamayan düğümler (nested `Precomp` dahil). |
| **`LNT-DAG-002`** | `Warning` / `Error` | **Kopuk / Çakışan Binding ve Animasyon:** (a) `enabled == false` olan düğümü hedefleyen aktif `ParamBindingDef` veya `EventBoundAnimation`; (b) `EventBoundAnimation.keyframes` boşsa veya `progress` `[0.0, 1.0]` dışındaysa (`Error`); (c) aynı `(node_id, param_name)` çiftine hem `param_bindings` hem `event_animations` yazıyorsa (`Warning` — runtime belirsizliği). |
| **`LNT-TIM-001`** | `Error` / `Warning` | **0-Süreli veya Taşan Aralıklar:** (a) `AdjustmentLayer { start_tick, end_tick }` için `start_tick >= end_tick` (`Error` — hiçbir karede çalışmaz); (b) `EventBoundAnimation.duration <= 0` (`Error`); (c) `AudioClipDef.timeline_end > timeline.duration_ticks` (`Warning` — kesilen ses); (d) ardışık `ClipDef`ler arasında boşluk (`prev.timeline_end < next.timeline_start`) varsa ve o aralıkta başka aktif klip yoksa (`Warning` — siyah boş kare riski). |
| **`LNT-SEC-001`** | `Warning` (`strict_sandbox=true` ise `Error`) | **Nested `CustomWgsl` Düğümü:** Kök grafik veya `Precomp` alt-grafiklerinde (derinlik 1..8, `custom_wgsl_node_path` mantığıyla) `NodeKind::CustomWgsl` varlığı tam hiyerarşik `node_path` ile raporlanır. |
| **`LNT-PERF-001`** | `Warning` | **Yüksek Motion Blur / GPU Maliyeti:** (a) `MotionBlurShutter.sub_samples > 16` (`> 32` ise yüksek maliyet uyarısı); (b) `MotionBlurShutter` altında başka bir `MotionBlurShutter` (`Precomp` içi dahil, `N * M` alt-kare patlaması); (c) `OpticalFlowBlur.samples > 24` veya `BlurMoCurves.samples > 32`. |
| **`LNT-MAT-001`** | `Warning` | **Matte, LUT Sırası ve Shockwave Tuzakları:** (a) `AssetDef.duration_ticks == None` olan disk video/ses varlığı ([lessons.md:22](../../brain/lessons.md#L22)); (b) `Lut3D` veya `ColorBasic` düğümü, `MaskMatte { opaque_black_outside: true }` veya `VectorText` sonrasında değil öncesinde bağlanmışsa ([lessons.md:15,16](../../brain/lessons.md#L15-L16) — siyah flaş/letterbox grade ile renklenir); (c) `RadialShockwave` düğümünde `radius <= 0.0` ve `radius` için `param_bindings` yoksa (`Error`/`Warning` — [lessons.md:16](../../brain/lessons.md#L16)). |

### 3.2. `diff_frame` Mimarisi (`frameshift-media` + `frameshift-core`)

İki karesel görüntü arasında (PNG dosya yolları **veya** proje içi iki `(revision_id / project_path, tick, profile)` render'ı arasında) saf Rust piksel karşılaştırması:

- **Girdi Modları:**
  1. **Dosya Modu:** `png_a: PathBuf`, `png_b: PathBuf` (`Rgba8Frame::load_png` ile okunur; `frameshift-media/src/frame.rs:203`).
  2. **Proje Modu (opsiyonel MCP/CLI kolaylığı):** Aktif projeden `tick` anındaki kare vs referans `png_a` (veya `project_a.json` vs `project_b.json` aynı `tick` anında `Engine::render_frame_at`).
- **Hesaplanan Metrikler (`FrameDiffReport`):**
  - `width`, `height`, `pixel_count`
  - `identical: bool` (`max_abs_diff == 0`)
  - `mae_rgba: [f64; 4]` (0..255 ölçeği), `mae_norm: f64` (`[0.0, 1.0]`)
  - `rmse_rgb: f64`, `psnr_db: Option<f64>` (`None` veya `999.0` eğer `mse == 0.0` ise JSON `NaN`/`Inf` güvenlik kuralına göre sonlu değer)
  - `max_abs_diff: u8`, `threshold: u8` (varsayılan `2`), `changed_pixels: u64`, `changed_ratio: f64`
  - `diff_bbox: Option<DiffBoundingBox>` (`x`, `y`, `width`, `height`, `x_norm`, `y_norm`, `w_norm`, `h_norm` — `max(|dR|,|dG|,|dB|,|dA|) > threshold` olan tüm pikselleri saran kutu)
  - `quadrant_mae: [f64; 4]` (sol-üst, sağ-üst, sol-alt, sağ-alt ortalama fark — ajana farkın nerede yoğunlaştığını söyler)
- **Fark Görseli Üretimi (`out_diff: Option<PathBuf>`):**
  - `DiffVisualMode::SideBySide` (varsayılan, `3x1` şerit: `[A | B | 4× Güçlendirilmiş Fark + Kırmızı BBox Çerçevesi]`) veya `DiffVisualMode::Heatmap` (yalnızca tek kare fark haritası).
  - Tamamen `Rgba8Frame` + `save_png` ile yazılır (0 yeni crate).

### 3.3. Kesin CLI ve MCP Arayüzleri (`lint` & `diff-frame`)

#### CLI (`crates/frameshift-cli`)
- `frameshift-cli lint <project.json> [--check-media] [--strict-sandbox] [--json]`
  - Exit code: `Error` seviyesinde bulgu varsa `1` (çıktı yine stdout JSON/markdown olarak basılır); yalnızca `Warning`/`Info` varsa `0`.
- `frameshift-cli diff-frame <frame_a.png> <frame_b.png> [--out-diff <diff.png>] [--mode side_by_side|heatmap] [--threshold <u8>] [--json]`
  - Alternatif proje modu: `frameshift-cli diff-frame --project-a <a.json> --project-b <b.json> --at <time> [--profile draft|final] [--out-diff <diff.png>]`

#### MCP (`crates/frameshift-mcp/src/server.rs`)
1. **`lint_project`**
   - `inputSchema`:
     ```json
     {
       "type": "object",
       "properties": {
         "expected_revision_id": { "type": "integer" },
         "check_media": { "type": "boolean", "default": false },
         "strict_sandbox": { "type": "boolean", "default": false },
         "black_min_duration_sec": { "type": "number", "default": 0.04 },
         "black_pixel_threshold": { "type": "number", "default": 0.08 }
       }
     }
     ```
   - `structuredContent`: `{ "revision_id": 42, "ok": false, "error_count": 1, "warning_count": 2, "info_count": 0, "issues": [...] }`
2. **`diff_frame`**
   - `inputSchema`:
     ```json
     {
       "type": "object",
       "required": ["reference_png"],
       "properties": {
         "reference_png": { "type": "string" },
         "candidate_png": { "type": "string", "description": "Verilmezse aktif projeyi tick anında render eder." },
         "tick": { "type": "integer" },
         "profile": { "type": "string", "enum": ["draft", "final"], "default": "draft" },
         "out_diff_png": { "type": "string" },
         "mode": { "type": "string", "enum": ["side_by_side", "heatmap"], "default": "side_by_side" },
         "threshold": { "type": "integer", "default": 2 }
       }
     }
     ```
   - `structuredContent`: `FrameDiffReport` + `out_diff_png` yolu (ayrıca MCP `content` dizisinde PNG base64 görüntü bloğu döndürülebilir — [inspection-contract.md:46](../../brain/specs/inspection-contract.md#L46)).

---

## 4. Alt Sistem 2: `Motion.md` Tarzı Deklaratif Kurgu DSL'i

### 4.1. Tasarım Hedefi ve Mimari Sınır

Gerçek FrameShift projeleri 70–260 düğüme ulaştığında ([state.md:23-24](../../brain/state.md#L23-L24)), ham JSON yazan LLM'ler NodeId bağlantılarında, Flicks hesabında ve sRGB→Linear dönüşümünde hata yapar. Deklaratif `.motion.md` / `.fsmotion` DSL'i:
- **Yalnızca bir ön-yüz derleyicisi**dir: tek doğruluk kaynağı `Project` JSON (`schema_version: "0.2.0"`, `i64` Flicks) kalır ([ADR-0001](../decisions/0001-independent-native-gpu-core.md), [ADR-0002](../decisions/0002-rational-timebase-flicks.md)).
- Mevcut `author_hard_cuts` ([session.rs:769](../../crates/frameshift-schema/src/session.rs#L769)) ve `EffectChainPreset` ([workbench.rs:493](../../crates/frameshift-core/src/workbench.rs#L493)) mekanizmalarını birleştirir ve üstüne `EventAnchorDef`, `ShaderTransition`, `VectorText`, `AudioClipDef` ve `MotionBlurShutter` ekler.
- **0 yeni crate:** Harici `yaml`/`nom`/`pest` bağımlılığı yerine `frameshift-schema/src/dsl.rs` içinde ~350 satırlık deterministik satır/anahtar=değer parser ve `frameshift-core/src/dsl.rs` içinde compiler yaşar.

### 4.2. EBNF Benzeri Gramer

```ebnf
motion_doc     := frontmatter directive* ;
frontmatter    := "---" nl meta_line* "---" nl ;
meta_line      := key ":" value nl ;           (* name, fps, res, bpm, lut, bg_color *)

directive      := ( asset_stmt
                  | audio_stmt
                  | shot_stmt
                  | beat_drop_stmt
                  | transition_stmt
                  | fx_stmt
                  | text_stmt
                  | shutter_stmt
                  | comment ) nl ;

time_expr      := float "s"                    (* 2.5s -> Flicks, frame_nearest ile kuantize *)
                | "f:" int                     (* f:60 -> frame index * frame_ticks *)
                | "beat:" int                  (* Beat/downbeat analizinden veya BPM gridinden *)
                | "bar:" int                   (* downbeats[i] *)
                | "@" ident                    (* Daha önce tanımlı beat_drop event_id *)
                | int "t" ;                    (* Ham Flicks *)

range_expr     := time_expr ".." time_expr ;
kv_pair        := ident "=" value ;

asset_stmt     := "asset" ident kind=("video"|"audio"|"image") "path=" quoted_str ;
audio_stmt     := "audio" ident "asset=" ident "range=" range_expr
                  ("src=" time_expr)? ("bus=" bus_name)? ("gain_db=" float)?
                  ("fade_in=" time_expr)? ("fade_out=" time_expr)? ;
shot_stmt      := "shot" ident "asset=" ident "range=" range_expr
                  ("src=" time_expr)? ("fit=" ("stretch"|"cover"|"contain"))?
                  ("speed=" speed_expr)? ("anchor=" ident)? ;
beat_drop_stmt := "beat_drop" ident "at=" time_expr
                  ("clip=" ident "src=" time_expr)? ("label=" quoted_str)? ;
transition_stmt:= "transition" shader_name "between=" ident "," ident
                  "at=" time_expr "dur=" time_expr kv_pair* ;
fx_stmt        := "fx" preset_or_node_kind "on=" target_ref
                  ("at=" time_expr ("dur=" time_expr)?)? kv_pair* ;
text_stmt      := "text" quoted_str "at=" range_expr
                  ("pos=" "(" float "," float ")")? ("size=" float)?
                  ("color=" hex_or_grad)? ("font=" quoted_str)?
                  ("outline=" hex_color)? ("anim=" ("none"|"pop"|"fade"))? ;
shutter_stmt   := "shutter" ("angle=" float)? ("samples=" int)? ("phase=" ("centered"|"start"|"end"))? ;
```

### 4.3. Deterministik Derleme Kuralları (4 Tuzağa Karşı Yerleşik Koruma)

1. **Kesin `i64` Flicks ve Kare Izgarası Kuantizasyonu:** Tüm `time_expr` değerleri `frameshift_time::Rational32::frame_duration()` ızgarasına yuvarlanır; `f64` yalnızca `SafeExpr` ve `SpeedCurve` oluşturmada ara hesap olarak kullanılır.
2. **Otomatik `srgb_to_linear` Dönüşümü:** `text ... color=#FFF5EB` yazıldığında derleyici `#RRGGBB[AA]` hex değerini `frameshift_media::srgb_to_linear` ile **Linear Rec.709** `[f32; 4]` dizisine çevirir (`LNT-CLR-001` tuzağı sıfırlanır).
3. **Şeffaf Metin Kompozisyonu:** Her `text` direktifi doğrudan `VectorText` üreterek ana resim dalının üstüne `Blend { mode: Normal, opacity: 1.0 }` ile bağlanır; opak `Solid` taban asla üretilmez (`LNT-CMP-001` önlenir). Görünürlük penceresi `EventBoundAnimation` (`hold_after: true`) veya `ParamBindingDef` (`step(...)`) ile `[start, end)` aralığına bağlanır.
4. **`ShaderTransition` Otomatik `SafeExpr` Splicing:** `transition whip_chromatic between=c1,c2 at=2.5s dur=0.35s` direktifi, `c1` ve `c2` kliplerinin sonlandığı düğümler arasına `ShaderTransition` ekler, `c1` zaman aralığını geçiş sonuna kadar uzatır (`c1.end = max(c1.end, at + dur/2)`), `c2.start = min(c2.start, at - dur/2)` yapar ve `ParamBindingDef { param_name: "progress", expression: "smoothstep(t0, t1, t_sec)" }` yazar.
5. **Sabit ID Adlandırması ve Sıralama:** `shot_00_<id>_src`, `cut_blend_<id>`, `tr_<idx>_<shader>`, `fx_<idx>_<preset>_*`, `txt_<idx>`, `blend_txt_<idx>`, `lut_master`, `shutter_out` sırasıyla üretilir; her derleme bit-düzeyinde aynı JSON'ı verir ve sonunda otomatik `Project::validate()` + `lint_project()` çalıştırılır.

### 4.4. Somut Derleme Örneği

**Girdi (`asta_mini.motion.md` — 14 satır):**
```markdown
---
name: "Asta Mini Drop"
fps: 24
res: 1280x720
lut: anime_cyber_contrast(0.65)
---
asset clip_a video path="synthetic://clip_a"
asset clip_b video path="synthetic://clip_b"
asset bgm audio path="synthetic://beat"

audio a_bgm asset=bgm range=0s..5s src=0s bus=music fade_out=0.25s
shot c1 asset=clip_a range=0s..2.5s src=0s fit=cover
shot c2 asset=clip_b range=2.5s..5s src=0s fit=cover
beat_drop drop_1 at=2.5s clip=c1 src=2.4s label="Main Drop"
transition whip_chromatic between=c1,c2 at=2.5s dur=0.3s whip=0.85 ca=0.035
fx xenoz_impact_glow on=tr_01 at=@drop_1 dur=0.4s glow_intensity=1.2
text "DEVIL UNION" at=2.5s..4.0s pos=(0.5, 0.78) size=0.07 color=#FFF5EB font="Impact" anim=fade
shutter angle=180 samples=8
```

**Derlenen `Project` JSON Özeti (`schema_version: "0.2.0"`, `i64` Flicks):**
- `timeline`: `fps: 24/1`, `duration_ticks: 3_528_000_000` (`5 * 705_600_000`), `resolution: 1280x720`
  - `clips`: `c1` (`[0, 1_870_344_000)` — geçiş overlap'ı `+0.15s` ≈ 4 kare dahil), `c2` (`[1_658_160_000, 3_528_000_000)`)
  - `audio_clips`: `a_bgm` (`[0, 3_528_000_000)`, `bus: "music"`, `fade_out: 176_400_000`)
- `events`: `[{ "event_id": "drop_1", "target_output_tick": 1764000000, "source_clip_id": "c1", "source_anchor_tick": 1693440000, "origin": "user_marked" }]`
- `graph` (8 düğüm, döngüsüz DAG):
  `shot_c1_src`, `shot_c2_src` → `tr_01_whip_chromatic` → `fx_01_blur_mocurves` → `fx_01_deep_glow` → (`blend_txt_01` ← `txt_01` `VectorText { y_offset_norm: 0.78, color_rgba: [1.0, 0.913, 0.823, 0.0] }`) → `lut_master` (`anime_cyber_contrast`) → `shutter_out` (`MotionBlurShutter { sub_samples: 8 }`).
- `param_bindings`:
  - `tr_01_whip_chromatic.progress`: `"smoothstep(2.35, 2.65, t_sec)"`
  - `fx_01_deep_glow.intensity`: `"clamp(0.45 + audio_rms * 0.9 + (step(2.5, t_sec) * (1.0 - step(2.9, t_sec))) * 1.2, 0.0, 2.0)"`
  - `c1.event_animations`: `txt_01.opacity` (`0.0 -> 1.0`, `offset_from_event: 0`, `duration: 176_400_000`, `hold_after: true`)

### 4.5. CLI ve MCP Arayüzleri (`Motion.md` DSL)

- **CLI:**
  - `frameshift-cli dsl-compile <edit.motion.md> --out <project.json> [--beats-from <audio_or_report.json>] [--no-lint]`
  - `frameshift-cli dsl-decompile <project.json> --out <edit.motion.md>` *(isteğe bağlı ters özetleyici: basit projeleri okunabilir `.motion.md` taslağına çevirir)*
- **MCP:**
  - `compile_motion_script`:
    - `inputSchema`: `{ "motion_markdown": string, "beats_report_path"?: string, "replace_session_project"?: bool, "expected_revision_id"?: u64, "run_lint"?: bool }`
    - `structuredContent`: `{ "compiled_project": Project (veya "revision_id"), "node_count": usize, "clip_count": usize, "lint_report": LintReport }`

---

## 5. Alt Sistem 3: Parametrik Şablonlar (`calculateMetadata` + tipli `props`)

### 5.1. Mevcut Kısıt ve Remotion `calculateMetadata` Modelinin Rust Uyarlaması

Mevcut `TemplatePackage` ([template.rs:28-42](../../crates/frameshift-schema/src/template.rs#L28-L42)) içinde sabit bir `Project` gömülüdür; `TemplateSlot` yalnızca mevcut `asset_id` / `node_id` / `event_id` hedeflerinin değerini değiştirir. Yeni bir şarkı (ör. 18 sn, 140 BPM) ve farklı sayıda (ör. 12 adet) video klip verildiğinde süre, kesim sayısı ve efekt pencereleri kendini uyarlayamaz.

Güvenlik kuralımız gereği ([template.rs:5-7](../../crates/frameshift-schema/src/template.rs#L5-L7)) şablon paketleri harici kod/JS çalıştırmaz. Bunun yerine **bildirimsel `ParametricTemplateSpec` + saf Rust `calculate_metadata` çözücüsü** tasarlanır:

### 5.2. Veri Şeması (`frameshift-schema/src/template.rs` Uzantısı)

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParametricTemplateSpec {
    pub spec_version: u32,                 // 1
    pub template_id: String,
    pub name: String,
    pub props_schema: Vec<TemplatePropDef>,
    pub metadata_rules: MetadataRulesDef,
    pub motion_blueprint: MotionBlueprintDef, // MotionDoc şablonu veya kural tabanlı üretici
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TemplatePropKind {
    VideoClipList { min_items: usize, max_items: usize },
    AudioFile,
    Text { max_len: Option<usize> },
    Number { min: f64, max: f64 },
    ColorHex,
    Choice { options: Vec<String> },
    Boolean,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetadataRulesDef {
    pub default_fps: Rational32,
    pub aspect_presets: BTreeMap<String, Resolution>, // "16:9" -> 1920x1080, "9:16" -> 1080x1920
    pub duration_policy: DurationPolicy,              // AudioLength | BeatCount(u32) | DownbeatBars(u32) | Clamp { min_ms, max_ms }
    pub beat_source: BeatSelectionSource,             // Downbeats | SubOnsets | BeatGrid
    pub cuts_per_beat: Rational32,                    // 1/1, 2/1, 1/2, 1/4 (plan_autocut uyumlu)
    pub min_segment_ms: u64,                          // örn. 280 ms
    pub transition_shader: String,                    // örn. "whip_chromatic"
    pub transition_dur_ms: u64,                       // örn. 250 ms
    pub drop_fx_preset_id: Option<String>,            // örn. "xenoz_impact_glow"
    pub drop_fx_window_ms: [i64; 2],                  // [-120, +380] ms (anchor öncesi/sonrası)
}
```

### 5.3. `calculate_metadata` Hesaplama Akışı (`frameshift-core/src/template_engine.rs`)

`calculate_metadata(&spec, &props, &options) -> Result<CalculatedTemplateMetadata, CoreError>`:

1. **Props Doğrulama:** `VideoClipList` uzunluğu `[min_items, max_items]` içinde mi, `AudioFile` mevcut mu, `Choice` izinli listede mi, `Number` `[min, max]` içinde mi kontrol edilir.
2. **Medya & Beat Çözümleme:**
   - Her video klip `FfmpegBridge::probe` (veya `synthetic://` sentetik süresi) ile `duration_ticks`, `width`, `height`, `fps` olarak ölçülür.
   - Ses dosyası `MediaResolver::load_audio_asset` + `analyze_multiband_beats` ([spectral.rs](../../crates/frameshift-media/src/spectral.rs)) ile `MultibandBeatReport` (`estimated_bpm`, `beat_grid`, `downbeats`, `sub.onsets`) üretir (veya önbellekteki `beats_report.json` kullanılır).
3. **Süre ve Grid Hesaplama (Deterministik `Flicks`):**
   - `duration_policy`'ye göre toplam `duration_ticks` belirlenir ve `frame_floor(fps, raw_duration)` ile kare ızgarasına oturtulur.
   - `beat_source` (`downbeats` / `sub.onsets` / `beat_grid`) + `plan_autocut` ([session.rs:976](../../crates/frameshift-schema/src/session.rs#L976)) kullanılarak çakışmayan kesim sınırları ve her klibe düşen `[timeline_start, timeline_end)` + `source_start` pencereleri hesaplanır. (Klip sayısı kesim sayısından azsa round-robin/cursor ilerletme `plan_autocut` mantığıyla çalışır; fazlaysa ilk K klip veya enerji sıralı seçim yapılır).
4. **Drop Olayları ve Efekt Pencereleri:**
   - `downbeats` (veya `sub.energy_envelope` tepe noktaları) içinden ana drop `EventAnchorDef`'leri seçilir; her biri etrafında `[target + pre_ticks, target + post_ticks)` efekt/geçiş penceresi hesaplanır.
5. **Çıktı (`CalculatedTemplateMetadata`):**
   - Render başlatmadan önce tam şeffaf önizleme JSON'ı döner: `{ resolved_duration_ticks, resolved_duration_sec, resolved_resolution, estimated_bpm, cut_count, drop_events, effect_windows, clip_assignments, warnings }`.
6. **Proje Üretimi (`instantiate_parametric_template`):**
   - Hesaplanan `CalculatedTemplateMetadata` + `props` → Alt Sistem 2 (`MotionDoc` AST) veya doğrudan `Project` üreticisi üzerinden `Project` (`0.2.0`) oluşturulur → `Project::validate()` + `lint_project()` çalıştırılır.

### 5.4. CLI ve MCP Arayüzleri (Parametrik Şablonlar)

- **CLI (`frameshift-cli/src/workbench.rs`):**
  - `frameshift-cli template-metadata <template.json> <props.json> [--beats-json <beats.json>]`
    - Sadece `CalculatedTemplateMetadata` JSON basar (dry-run; render yapmaz).
  - `frameshift-cli template-instantiate <template.json> <props.json> --out <project.json> [--beats-json <beats.json>] [--no-lint]`
    - Metadata hesaplar, projeyi derler, `lint_project` kontrolünden geçirir ve `--out` dosyasına yazar.
- **MCP (`frameshift-mcp/src/server.rs`):**
  - `template_calculate_metadata`: `{ "template_path": string, "props": object, "beats_report_path"?: string }` → `CalculatedTemplateMetadata` JSON döner.
  - `instantiate_parametric_template`: `{ "template_path": string, "props": object, "beats_report_path"?: string, "load_into_session"?: bool, "expected_revision_id"?: u64 }` → üretilen `Project` özeti + `CalculatedTemplateMetadata` + `LintReport` döner (ve istenirse aktif `ProjectSession`'ı `ReplaceProject` ile günceller).

---

## 6. Değiştirilecek ve Eklenecek Dosya Listesi (0 Yeni Crate)

| Crate | Dosya | İşlem | Sorumluluk |
| :--- | :--- | :--- | :--- |
| `frameshift-media` | `crates/frameshift-media/src/ffmpeg.rs` | Düzenle | `FfmpegBridge::detect_black_intervals` (`blackdetect` stderr ayrıştırıcısı + unit testler) |
| `frameshift-media` | `crates/frameshift-media/src/frame.rs` | Düzenle | `Rgba8Frame::diff(&self, other, threshold)` (`FrameDiffMetrics`, bbox, 3-panelli diff PNG üreticisi) |
| `frameshift-schema` | `crates/frameshift-schema/src/dsl.rs` | Yeni | `MotionDoc`, `MotionDirective` AST + sıfır-bağımlılıklı `.motion.md` parser + time token parser |
| `frameshift-schema` | `crates/frameshift-schema/src/template.rs` | Düzenle | `ParametricTemplateSpec`, `TemplatePropDef`, `MetadataRulesDef`, `CalculatedTemplateMetadata` + şema doğrulaması |
| `frameshift-schema` | `crates/frameshift-schema/src/project.rs` | Düzenle | `bindable_param_names` görünürlüğünü `pub` yapmak (lint + DSL compiler paylaşımı için) |
| `frameshift-schema` | `crates/frameshift-schema/src/lib.rs` | Düzenle | `pub mod dsl;` ve yeni tiplerin re-export'u |
| `frameshift-core` | `crates/frameshift-core/src/lint.rs` | Yeni | `lint_project()` (4 sessiz tuzak + DAG/timing/security/perf/matte kuralları) + `diff_frames()` |
| `frameshift-core` | `crates/frameshift-core/src/dsl.rs` | Yeni | `compile_motion_doc()` + `decompile_project_to_motion()` (`i64` Flicks + `srgb_to_linear` + `EffectChainPreset` splice) |
| `frameshift-core` | `crates/frameshift-core/src/template_engine.rs` | Yeni | `calculate_metadata()` + `instantiate_parametric_template()` (`MultibandBeatReport` + `plan_autocut` entegrasyonu) |
| `frameshift-core` | `crates/frameshift-core/src/lib.rs` | Düzenle | `pub mod lint; pub mod dsl; pub mod template_engine;` |
| `frameshift-cli` | `crates/frameshift-cli/src/workbench.rs` | Düzenle | `lint`, `diff-frame`, `dsl-compile`, `dsl-decompile`, `template-metadata`, `template-instantiate` alt komutları + `print_usage` |
| `frameshift-mcp` | `crates/frameshift-mcp/src/server.rs` | Düzenle | `lint_project`, `diff_frame`, `compile_motion_script`, `template_calculate_metadata`, `instantiate_parametric_template` araçları |
| Testler | `crates/frameshift-core/tests/` & `crates/frameshift-mcp/tests/` | Yeni/Düzenle | 4 sessiz tuzak yakalama testleri, `diff_frame` metrik/bbox testi, `.motion.md` determinizm testi, parametrik BPM/klip sayısı uyarlama testleri |

---

## 7. Kullanıcıya Sorulacak Net Karar Seçenekleri

Uygulamaya geçilmeden önce ürün/şema sınırını etkileyen **2 net tasarım kararı** kullanıcı onayına sunulmalıdır:

### Karar 1: Parametrik Şablon Paket Formatı (`TemplatePackage` v1 Genişletmesi vs `ParametricTemplateSpec` v2)
- **Seçenek A *(Önerilen)* — Ayrı `ParametricTemplateSpec` (`"package_kind": "parametric_v1"` veya `*.fstpl.json`) + `TemplatePackage` v1 Korunması:** Mevcut `TemplatePackage` (`package_version: 1`, `template_validate` / `template_open`) sözleşmesi birebir geriye dönük korunur; dinamik süreli şablonlar kendi temiz şemasıyla (`ParametricTemplateSpec`) ve `template-metadata` / `template-instantiate` komutlarıyla çalışır.
- **Seçenek B — Mevcut `TemplatePackage` İçine Opsiyonel `parametric` Alanı Eklemek:** Tek dosya uzantısı kalır ama `TemplatePackage` içinde hem statik `project` hem dinamik `parametric` alanı bulunması `project.validate()` zorunluluğu nedeniyle boş dummy proje tutma sürtünmesi yaratır.

### Karar 2: `lint_project` Varsayılan Kapı Davranışı (`compile_motion_script` / `render_project` Entegrasyonu)
- **Seçenek A *(Önerilen)* — `Error` Durumunda Sert Red, `Warning` Durumunda Raporla ve Devam Et (`--check-media` Opsiyonel):** DSL derleme ve şablon üretiminde `LintSeverity::Error` (ör. opak `Solid` tabanın videoyu kapatması, `0`-süreli `AdjustmentLayer`, tam ekran dışı metin) işlemi durdurur; `Warning` ve yavaş `ffmpeg blackdetect` medya taraması ise yalnızca uyarı döndürür veya `--check-media` ile istenir.
- **Seçenek B — Linter'ı Yalnızca Danışman (`lint_project` Çağrılınca Raporlayan) Tutmak:** `dsl-compile` ve `template-instantiate` yalnızca `Project::validate()` çalıştırır; sessiz görsel tuzaklar yalnızca `lint_project` ayrıca çağrıldığında görülür.
- **Seçenek C — Tam Katı Kapı (`--warnings-as-errors` Varsayılan):** Tüm `Warning` bulguları da export/derlemeyi bloke eder; kasıtlı stilistik denemelerde ajanların ilerlemesini zorlaştırabilir.
