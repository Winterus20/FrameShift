---
id: inspection-contract
status: locked
created: 2026-11-21
updated: 2026-11-21
related_adr: [0001, 0003]
tags: [inspection, agent, engine, render-graph]
---

# Çoklu İnceleme Yüzeyleri ve Epistemik Raporlama Sözleşmesi (`inspection-contract`)

## Amaç ve Kapsam

Bu sözleşme, FrameShift çekirdeğinin (`frameshift-core`, `frameshift-cli`, `frameshift-mcp`) insan kullanıcıya ve agent katmanına ürettiği tüm görsel, video, kodsal ve teknik inceleme paketlerinin yapısını, ortak sürüm/zaman kimliğini (`revision_id` + `tick`) ve epistemik doğrulama sınırlarını kilitler ([PROJECT_NOTES.md](../../PROJECT_NOTES.md#L147-L171), [ADR-0003](../decisions/0003-semantic-event-constraint-timing.md)).

Amaç; agent'ın bütün projeyi bağlama yığmadan hatalı aralığı doğru zaman, kaynak medya ve düğüm koduyla eşleştirebilmesi, ayrıca görmediği veya duymadığı bir niteliği "doğrulandı" olarak raporlamasını teknik sözleşmeyle önlemektir.

---

## Veri Modeli / Arayüz Sözleşmesi

### 1. Ortak Kimlik ve Sürüm Bağı (`InspectionIdentity`)

Dört inceleme yüzeyinin tamamı aynı kimlik başlığını taşır; eski render çıktısının yeni proje koduyla yanlışlıkla eşleştirilmesi yasaktır:

```json
{
  "schema_version": "0.2.0",
  "revision_id": 42,
  "profile": "draft",
  "timeline_fps": { "num": 24000, "den": 1001 },
  "tick_range": { "start_tick": 0, "end_tick": 1411200000 },
  "sampled_ticks": [0, 29429400],
  "asset_hashes": { "clip_1": "sha256:..." },
  "shader_hashes": { "transition_whip_chromatic": "sha256:..." }
}
```

- Her üretilen tekil PNG, çok panelli paket, contact sheet, taslak MP4 ve teknik JSON raporu `revision_id` + `output_tick` (`Flicks`) kimliğini meta veri ve/veya dosya/panel etiketinde taşır.
- İstenen inceleme paketi `project.revision_id != requested_revision_id` tespit ederse motor eski önbelleği döndürmez; yeniden değerlendirir veya `InspectionRevisionMismatch` hatası verir.

### 2. Dört İnceleme Yüzeyi

1. **Görsel Yüzey (`Visual`):**
   - Tekil tam kare (`FullFrame`), tam çözünürlüklü kesit (`FullResCrop`), çok sayfalı etiketli `ContactSheet`, 4 panelli `MaskPack`, 3 panelli `TransitionPack`, `EffectDiffPack` (önce/sonra + ara node çıktısı) ve `VelocityPack`.
   - Görsel paketler gerçek görüntü byte'ları (`PNG`) olarak üretilir; MCP/agent adaptörü yalnızca dosya yolu string'i döndürmek yerine görüntü payload'ını modele iletmekle yükümlüdür.
2. **Video Yüzeyi (`Video`):**
   - Seçili `event_id` veya aralık çevresinde kısa sesli taslak MP4 (`[start_tick - pre_roll, end_tick + post_roll]`, örn. `±1.0 sn` bağlam).
   - Ses-görsel senkronu ve hareket akıcılığı yalnızca bu yüzey (veya insan izlemesi) üzerinden değerlendirilebilir.
3. **Kodsal Yüzey (`Code`):**
   - İlgili `ClipDef`, `EventAnchorDef`, `GraphNodeDef` alt-grafiği, `SafeExpr` / `ParamBindingDef` tanımları, WGSL shader kaynak kodu ve hız eğrisi kontrol noktaları.
   - Metin olarak kalır; görselin içine gömülmez ve ortak `revision_id` + `node_id` + `tick` ile görsel yüzeye bağlanır.
4. **Teknik Yüzey (`Technical`):**
   - Yapılandırılmış JSON raporu: `output_tick -> source_tick` haritası, VFR aktif PTS eşleşmeleri, beat/impact zamanları, aktif profil (`draft` / `final`), düğüm yürütme süreleri, GPU/bellegi uyarıları ve epistemik kapsam bildirimi.

### 3. Görsel Paket Şartnameleri

#### 3.1. Contact Sheet (`ContactSheet`)
- **Amaç:** Video okuyamayan ancak tekil görselleri inceleyebilen modellere ve hızlı insan taramasına zaman etiketli kare dizisi sunmak.
- **Örnekleme Stratejisi:**
  - *Genel Bakış (`overview`):* Aralık boyunca seyrek düzenli örnekleme (sahne sırası, renk, kadraj).
  - *Olay Odaklı (`event_focused`):* Kesme, `event_id` impact noktası, hız eğrisi zirvesi ve `ShaderTransition` penceresi çevresinde sık örnekleme.
- **Okunabilirlik Sınırı:** Aşırı küçültülerek detayları (yüz ifadesi, maske kenarı, metin) yok eden dev tek kolajlar yerine küçük, okunabilir ızgara sayfaları (örn. sayfa başına en fazla `3x3` veya `4x3` hücre, hücre başına en az `320px` genişlik) üretilir.
- **Zorunlu Etiket:** Her hücrenin altında okunabilir `rev:<id> | f:<frame_idx> | tick:<output_tick> (<t_sec>s) | src:<source_tick>` etiketi ve varsa aktif `event_id` yer alır.

#### 3.2. 4 Panelli Maske Paketi (`MaskPack`)
Aynı `(revision_id, output_tick)` anı için `2x2` etiketli 4 panelden oluşur:
1. **Panel 1 (`Source`):** Maske uygulanmadan önceki ham/dönüştürülmüş kaynak katman (`MaskMatte.source`).
2. **Panel 2 (`Mask`):** Tek kanallı/gri tonlamalı maske/matte çıktısı (`MaskMatte.mask` — `alpha` veya `luma` + `invert`/`feather` uygulanmış hali).
3. **Panel 3 (`CheckerboardCutout`):** Premultiplied alfa kesimini ve saç/kenar halo/fringing hatalarını açıkça görmek için standart yüksek kontrastlı dama zemin üzerine bindirilmiş maskeli ön plan.
4. **Panel 4 (`FinalComposite`):** Arka plan ile birleştirilmiş nihai kompozit kare.

#### 3.3. 3 Panelli Geçiş Paketi (`TransitionPack`)
Bir `ShaderTransition` (veya `Blend`) düğümü için seçilen örnekleme anında (`progress` değeri etiketli) yan yana 3 panelden oluşur:
1. **Panel 1 (`Input A`):** `tex_a` girişinin o `tick` anındaki değerlendirilmiş karesi.
2. **Panel 2 (`Input B`):** `tex_b` girişinin o `tick` anındaki değerlendirilmiş karesi.
3. **Panel 3 (`BlendedOutput`):** `ShaderTransition` sonrası birleşmiş çıktı karesi.

#### 3.4. Hız Eğrisi Paketi (`VelocityPack`)
Bir `ClipDef` zaman aralığı boyunca örneklenen çıktı karelerini (`output_tick`), karşılık gelen `source_tick` değerlerini, anlık hız çarpanını (`speed_x`) ve `SpeedCurve` grafiği verisini (görsel şerit + teknik JSON serisi) birlikte sunar; böylece donma (`0.0x`), ters sarma (`< 0.0x`) ve impact hizalaması doğrulanır.

#### 3.5. Tam Çözünürlüklü Kesit (`FullResCrop`)
Normalize ROI (`{ x: f32, y: f32, width: f32, height: f32 }`, `[0..1]`) üzerinden `1:1` piksel ölçeğinde, küçültmesiz PNG kesit üretir. Yüz okunabilirliği, `VectorText` anti-aliasing, ince maske kenarı ve `chromatic_aberration` incelemesi için zorunludur.

### 4. Epistemik Raporlama Sınırları (`EpistemicScopeReport`)

Her inceleme paketi ve agent değerlendirmesi aşağıdaki epistemik sınır kurallarını teknik raporda açıkça beyan etmek zorundadır ([PROJECT_NOTES.md §5.2](../../PROJECT_NOTES.md#L165-L170)):

```json
{
  "revision_id": 42,
  "profile": "draft",
  "verified_surfaces": ["visual_contact_sheet", "visual_mask_pack", "technical_json"],
  "unverified_aspects": ["temporal_smoothness", "inter_sample_flicker", "audio_visual_sync"],
  "algorithmic_indicators": {
    "determinism_hash_match": true,
    "beat_onset_aligned_within_1_frame": true
  },
  "epistemic_note": "Contact sheet ve teknik metrikler akıcılık veya estetik ritim kanıtı değildir."
}
```

1. **Algoritmik Metrik ≠ Estetik Ritim:** Beat/onset tespiti, RMS zarfı, kare hash determinizmi ve `within_1_frame` kontrolleri yalnızca **teknik belirti** kontrolüdür; estetik ritim veya görsel kalite kanıtı sayılamaz.
2. **Contact Sheet ≠ Video:** Görsel model tek karelerden, maske paketinden ve contact sheet'ten sahne anlamını, karakteri, kadrajı ve maske kenarını inceleyebilir; ancak contact sheet örnekler arasındaki titremeyi (flicker), motion blur doğallığını, akıcılığı ve ses-görsel senkronunu **kanıtlayamaz**.
3. **Metin-Yalnız Model Sınırı:** Görsel/video erişimi olmayan bir model yalnızca teknik JSON'a dayanarak görsel kaliteyi "doğrulandı" olarak işaretleyemez.
4. **Profil Farkı:** `draft` profilinde incelenen bir çıktı `final` profilinin piksel kalitesini garantilemez; rapor kullanılan profili ve örnekleme aralığını gizleyemez.

---

## Kenar Durumlar ve Hata Davranışı

- **Revizyon Uyuşmazlığı:** Paket üretiminde veya karşılaştırmada `revision_id` uyuşmazlığı tespit edilirse `InspectionRevisionMismatch` döndürülür; eski görsel yeni sürümle etiketlenmez.
- **Geçersiz ROI (`FullResCrop`):** `[0, 1]` aralığı dışına taşan veya genişliği/yüksekliği `<= 0` olan kesit isteği `InvalidCropRegion` hatası ile reddedilir.
- **Hedef Düğüm Eksikliği:** `MaskPack` için `MaskMatte` düğümü veya `TransitionPack` için `ShaderTransition` düğümü bulunamazsa `InspectionTargetNotFound` döndürülür.
- **Boş veya Ters Zaman Aralığı (`ContactSheet` / `VelocityPack`):** `end_tick <= start_tick` veya `sample_count == 0` durumunda `InvalidInspectionRange` hatası verilir.
- **Adaptör Medya İletim Hatası:** Agent arayüzü görsel payload'ı modele aktaramıyorsa teknik raporda `verified_surfaces` boş bırakılır ve görsel doğrulama yapılmadığı açıkça işaretlenir.

---

## Kabul Kriterleri (Kod Aşamasında Doğrulanacak)

- [ ] Aynı proje üzerinde üretilen `ContactSheet`, `MaskPack`, `TransitionPack`, `VelocityPack`, `FullResCrop`, taslak MP4 ve teknik JSON raporu birebir aynı `revision_id` değerini ve tutarlı `output_tick` / `source_tick` etiketlerini taşır.
- [ ] `MaskPack` 4 paneli (`Source`, `Mask`, `CheckerboardCutout`, `FinalComposite`) doğru sırada üretir ve dama zemin üzerinde premultiplied alfa kesimini doğrular.
- [ ] `TransitionPack` 3 paneli (`Input A`, `Input B`, `BlendedOutput`) seçilen `tick` anında tek kare tutarlılığıyla üretir.
- [ ] `FullResCrop` hedef bölgede `1:1` ölçeği korur ve yeniden örnekleme bulanıklığı eklemez.
- [ ] Üretilen teknik inceleme raporu `verified_surfaces`, `unverified_aspects` ve `profile` alanlarını her zaman eksiksiz içerir.
