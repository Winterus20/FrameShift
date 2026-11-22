# FrameShift — Aktif Durum (`state.md`)

**Son Güncelleme:** 2026-11-22  
**Asta v3 Takım Çalışması (2026-11-22):** 11 araştırma raporu tamamlandı; ADR-0004 kabul edildi (`schema_version` `0.2.0`, yükleme kuralı `minor <= current`, `0.1.0` projeler yüklenir). Kullanıcı kararları: **fade-only** ses (gain automation yok), parçacık **B** (burst alt-yapısı + `emission`), **1 girişli scrim**, **statik reframe** (9:16), **ribbon varsayılan düzeltmesi**, **`autocut` v1**, ilerleme satırları **stderr**'e, ayrı **`sheet`/`materials`** komutları. P0 streaming encode ölçüldü: çalışma kümesi **6.5 GB → 0.27–0.80 GB** (O(N) tampon kaldırıldı; tepe LRU önbelleğiyle sınırlı). Fame v3 720p 1920 kare: **0.27 GB** / private **0.91 GB** / **100.2 sn**; asta_amv draft 720p: **0.80 GB**; asta_v3 draft 640×360: **0.40 GB**. Uygulama dalgaları devam ediyor; tamamlanmayan özellikler "tamamlandı" sayılmaz. Ayrıntı: [takım günlüğü](log/2026-11-22-asta-v3-team.md).  
**SC-0002 Yeniden Üretimi (2026-11-22):** Referans [@asgardski kinetic-typography AMV](https://www.tiktok.com/@asgardski/video/7672463186175544609) (Mushoku Tensei, 1920×1080, 60 fps, 31.53 sn, 59 plan, 139.7 BPM) FrameShift ile yeniden üretildi: `local_media/mushoku/mushoku_project.json` (**260 düğüm, 126 binding, 59 klip, 30 kelime**), üretici `local_media/mushoku/build_project.py`, çıktı `local_media/mushoku/mushoku_final.mp4`. Harf harf yerleşim **harf başına ayrı `vector_text` düğümü** ile kuruldu; özne arkası örtme, kontrastı şişirilmiş luma maskesi + %58 blend ile **taklit edildi** (gerçek matte yok). Stil kartı: [SC-0002](style-cards/SC-0002-mushoku-tensei-kinetic-type-amv.md). Eksik/yol haritası: [gap analysis](research/2026-11-22-kinetic-typography-gap-analysis.md).  
**Asta v3 Prodüksiyon (2026-11-22):** `local_media/asta_v3/asta_v3_final.mp4` — 20.000 sn, 1920×1080, 60 fps, 28 kesim, 76 node / 66 binding. Müzik drop'u ölçülen 33.35 sn'den 25.85 sn offset ile tam **7.50 sn**'ye oturtuldu (RMS −19.1 → −12.7 dB); giriş/çıkış fade'leri `bgm_v3.wav` içinde. `ffprobe`: 1200 kare / 20.000 sn, H.264 + AAC 48 kHz stereo. Ayrıntı: [log](log/2026-11-22-asta-v3-red-line.md).  
**Aktif Faz:** Faz 0–5 (M1–M5) + Edit-Friction + Faz 6.1–6.5 + **Faz 6.6 (Shader Görsel Kalite Yükseltmesi — `AgX`/`PbrNeutral`/`AgxPunchy` Tonemap + TPDF/IGN Dithering + `4x MSAA` & Mipmap + FXAA + Gerçek Gauss Halation / 35mm Grain / Spektral CA + Anamorfik DeepGlow + Additive Glow Koruması + 3D Cook-Torrance PBR GGX / Kamera-Bağıl IBL / Bevel AO / Z-Echo Depth + `Scene3D` Separable CoC & Poligonal/Anamorfik Bokeh DoF + Algısal `Overlay` & 5 Yeni Blend Modu + Sürekli Gauss Shockwave + 10 Aspect-Correct IGN-Blur Geçiş + CMYK/ColorDot Halftone + 2-Yönlü JFA SDF `text2d_fx` — 41 WGSL Shader, 0 Yeni Crate)** Tamamlandı ve Doğrulandı  
**Faz 6.6 Doğrulama:** `0` yeni Cargo crate, `cargo clippy --workspace --all-targets -- -D warnings` (**0 uyarı**), `cargo test --workspace` (**199/199 geçti**, +12 yeni Faz 6.6 testi) ve `verify-all --out-dir target/verify_66` (**M1–M5 PASS**).  
**Faz 6.5 Doğrulama:** `2` yeni saf Rust crate (`skrifa = "0.50"`, `harfrust = "0.16"`), `cargo clippy --workspace --all-targets -- -D warnings` (**0 uyarı**), `cargo test --workspace` (**187/187 geçti**), `verify-all --out-dir target/verify_65` (**M1–M5 PASS**) ve `target/verify_65/typography_showcase_65.png` görsel olarak doğrulandı.  
**Faz 6.4 Doğrulama:** `0` yeni Cargo crate, `cargo clippy --workspace --all-targets -- -D warnings` (**0 uyarı**), `cargo test --workspace` (**177/177 geçti**), `verify-all` (**M1–M5 `all_gates_passed: true`**), `local_media/asta_amv_project.json` draft render (`112.56s -> 70.53s`, `%37.3` hızlanma) ve `target/verify_64/showcase_64.png` görsel olarak doğrulandı.  
**Faz 6.3 Doğrulama & Prodüksiyon Çıktısı (2026-11-22):** `0` yeni Cargo crate, `127/127` test, `verify-all` PASS; `local_media/fame_is_a_gun/fame_is_a_gun_v3_platsae_final.mp4` (`1280×720`, `60fps`, `32.00s`, `1920` kare).
**Edit-friction:** Uygulama duruyor. Bu oturumda `cargo test --workspace` geçti, `cargo clippy --workspace --all-targets -- -D warnings` geçti, `verify-all` geçti (`CARGO_TARGET_DIR=target/friction-verify`).
**Faz 6.1 + 6.2 doğrulama:** `cargo test --workspace` (**107/107 geçti**), `cargo clippy --workspace --all-targets -- -D warnings` (**0 uyarı**), `verify-all` (**M1–M5 PASS**), gerçek medya karşılaştırmaları (`local_media/faz6_1_showcase.mp4` ve `local_media/faz6_2_kinetic3d_showcase.mp4`) görsel olarak doğrulandı.

---

## Mevcut Gerçeklik (Epistemik Sınır)

- **Tamamlanan ve Doğrulanan Çekirdek:**
  - **5 Kilitli Sözleşme (`brain/specs/`):** [timebase.md](specs/timebase.md), [project-schema.md](specs/project-schema.md), [render-graph.md](specs/render-graph.md), [effect-shader-api.md](specs/effect-shader-api.md), [inspection-contract.md](specs/inspection-contract.md) (`status: locked`).
  - **3 Birincil Kaynaklı Teknik Araştırma Belgesi (`brain/research/`):**
    - [open-source-shader-transitions.md](research/open-source-shader-transitions.md) (`gl-transitions` MIT/BSD birincil GLSL analizi ve WGSL geçiş şartnamesi).
    - [anime-amv-postfx-shaders.md](research/anime-amv-postfx-shaders.md) (Vidvox ISF, Libretro `slang-shaders`, GodotShaders MIT/CC0 analizi ve 4 anime post-FX WGSL şartnamesi).
    - [3d-kinetic-typography-architecture.md](research/3d-kinetic-typography-architecture.md) (3D kinetik tipografi / motion graphics için eksik 6 alan ve önerilen Rust/`wgpu`/WGSL + lyric alignment mimarisi).
  - **9 Rust Crate'i + 15 WGSL Shader (`crates/frameshift-render/shaders/`):**
    - **Temel & Renk (7):** `common_fullscreen.wgsl`, `transform.wgsl`, `mask_matte.wgsl`, `blend.wgsl`, `color_basic.wgsl`, `lut3d.wgsl`, `motion_blur_accumulate.wgsl`.
    - **Geçişler (4):** `transition_whip_chromatic.wgsl`, `transition_cross_zoom.wgsl` (MIT), `transition_directional_warp.wgsl` (MIT), `transition_glitch_displace.wgsl` (MIT).
    - **Anime Post-FX (4):** `fx_motion_tile_shake.wgsl` (`MotionTileShake`), `fx_anime_impact_frame.wgsl` (`AnimeImpactFrame`), `fx_radial_shockwave.wgsl` (`RadialShockwave`), `fx_halftone_glow.wgsl` (`HalftoneGlow`).
- **Son Güncelleme:** 2026-11-21
- **Edit-friction uygulaması:** Implementation of the edit-friction work landed. Araştırma: [research/2026-11-21-edit-friction.md](research/2026-11-21-edit-friction.md). Uygulama günlüğü: [log](log/2026-11-21-edit-friction-fixes.md). Uygulama oturumunda `verify-all` çalışmamıştı; `frameshift-media`, `frameshift-core` ve `frameshift-render` testleri ve aynı üç pakette clippy geçmişti.
- **Edit-friction doğrulama (bu oturum, `CARGO_TARGET_DIR=target/friction-verify`):** Üç komut bitti. `cargo test --workspace` geçti (91 test, 0 başarısız). `cargo clippy --workspace --all-targets -- -D warnings` geçti (çıkış 0, uyarı yok). `verify-all` geçti (`all_gates_passed: true`). Blur kapısı: authored `sub_samples` 8, explicit `ShutterOverride` tavanı 1, hash `4416298738205128470` → `5729329321580515486`, `timing_anchors_identical: true`. Kaynak kod değişmedi. Ayrıntı: [log](log/2026-11-21-edit-friction-verify.md).
- **Köle edits dogfood:** Referans TikTok kare kare ve bant enerjisiyle izlendi; ayrı klipler + trailer cue ile `target/dogfood/koleedits/koleedits_draft.mp4` (960×740, 24 fps, 202 kare, 8.42 sn, H.264+AAC, render 26.57 sn) üretildi. Contact sheet akıcılık veya ses senkronu kanıtı değildir. Ayrıntı: [log](log/2026-11-21-koleedits-dogfood.md).
- **Önceki oturum kanıtı (70/70 sayımı ve Asta render bu oturumda tekrarlanmadı):** `cargo test --workspace` (**70/70 test geçti**), `cargo clippy --workspace --all-targets -- -D warnings` (**0 uyarı**), `verify-all` + `fx_showcase_pack.png`, ayrıca `local_media/asta_amv_project.json` üzerinden gerçek 1080p Asta klipleri + 48 kHz phonk müzik ile `local_media/asta_amv_draft.mp4` (1280x720, 24fps, 264 kare, 11.00s) render süresi pencere/pipe decode (`FfmpegBridge::decode_video_window`) ile **`218.59 sn -> 63.86 sn` (`3.42x` hızlanma)** seviyesine indirildi; `AudioRmsEnvelope`, `EventBoundAnimation.hold_after`, `MaskMatte.opaque_black_outside`, 2 sütunlu 27 düğümlü `ui_snapshot.png` ve analitik `GlyphSdf::eval_exact` tipografisi görsel olarak doğrulandı.
- **Edit-friction uygulama sonuçları** (doğrulama yukarıdaki maddede):
  - Araştırma: [research/2026-11-21-edit-friction.md](research/2026-11-21-edit-friction.md).
  - CLI: başarı satırı fps'i bir kez basar; zaman bayrakları Flicks, `1.905s` ve `f:<frame>` kabul eder; `peaks` cue/beat tick basabilir. Varsayılan profil Final. Waveform yok.
  - Hard cuts: `author_hard_cuts` sıralı, örtüşmeyen klipleri tam sayı Flicks ve bir Blend zinciri olarak yazar. Aralık dışı kaynaklar şeffaf kalır.
  - SafeExpr: bilinmeyen parametre adları `Project::validate` içinde `InvalidParamBinding` olur. Değerlendirme hataları `RenderError::Expression` olur ve base değeri korumaz. `t_sec` türetilmiş uniform kalır. `step(1.905, t_sec) * (1 - step(1.980, t_sec))` değerlendirilir.
  - Shockwave: `radius <= 0` halka maskesini sıfırlar. `zoom_blur_strength` çalışmaya devam eder. Halka maskesini kullanmayan küçük bir distance chromatic terim yerinde bırakıldı.
  - Schema: `SourceVideo.fit` `stretch|cover|contain`, varsayılan `stretch`. `GraphNodeDef.enabled` varsayılan `true`. Kapalı 1 girdili düğüm girdisini kopyalar; 2 girdili düğüm base döner; üretici şeffafa temizler.
  - Fit pikselleri: stretch ffmpeg `-s`; cover scale+crop; contain scale+pad, şeffaf siyah. Aynı varlığı paylaşan iki `SourceVideo` düğümü farklı mod isterse her biri kendi çözülmüş karesini alır; önbellek anahtarı modu içerir, stretch ile cover aynı tamponu paylaşmaz.
  - Blur: her `MotionBlurShutter` yazılmış `sub_samples` değerini tutar. Profil onu değiştirmez. `ShutterOverride.sub_samples` açık bir tavandır (`min(authored, ceiling)`). CLI `node N, effective M` basar. `verify-all`'ın 1 örnek kapısı o tavandır ve bu oturumda geçti (8 → 1, hash değişti, zaman çıtaları aynı).
  - Bu turda yok: glitch self-blend, LUT/grade siyah tutuşu, font dosyası (ASCII atlas durur), solo modu, waveform.
  - Kullanıcı kararları: shockwave ring-only, opt-in fit, node-owned `sub_samples`, `enabled` bayrağı, gömülü atlas.

---

## Sıradaki Öncelikli Adımlar

1. **Faz 6.5 Tamamlandı (2026-11-22):** `harfrust 0.16.1` + `skrifa 0.50.0` shaping (`shaper.rs`), WGSL Slug GPU Bézier (`slug.rs`, `text_slug.wgsl`, `text2d_fx.wgsl`), `BevelProfile3D::{Flat, Round, Concave, Stepped}` + `material_mode = 3` Iridescent Chrome (`mesh3d.rs`, `text3d_mesh.wgsl`) ve 13 SIL OFL 1.1 küratörlü font (`assets/fonts/`) tamamlandı ve **187/187** test + `verify-all` ile doğrulandı.
2. **SC-0002'den Çıkan Yeni Öncelikler (2026-11-22):**
   - **P0** `VectorText`/`Text3D`'ye `per_glyph` dizisi (harf başına dx/dy/rot/scale/color/delay) — harf başına düğüm zorunluluğunu kaldırır.
   - **P0** `DepthMap` düğümü veya matte varlık girdisi + metin için `text_behind` kompoziti — özne arkası tipografi ancak bununla gerçek olur.
   - **P1** Projeye `tempo` (bpm/offset) alanı ve `peaks --author-cuts` — 59 kesmeyi elle yazma zorunluluğunu bitirir.
   - **P1** `crop_rect` + panel düğümü; metne özel `chromatic_shift`.
   - Ayrıntı: [gap analysis](research/2026-11-22-kinetic-typography-gap-analysis.md).
3. **İsteğe Bağlı Kalan Uzantılar:**
   - `frameshift-app` içine `winit` / `egui` canlı pencere kabuğu (timeline scrubbing + canlı önizleme).
   - CLI/MCP `Demucs + stable-ts` söz zamanlama boru hattı (Faz 6C).
   - `SafeExpr` ekstra operatörleri (`if`, `<, <=, >, >=, ==`, `&&, ||, !`, `%`, `mix`, `floor`, `ceil`, `sqrt`, `log`), `solo` modu, waveform PNG ve `SC-0002+` stil kartları.

---

## Son Oturum Bağlantısı

- [2026-11-22 — GitHub publication checkpoint](log/2026-11-22-github-publication.md): `cargo test --workspace --locked` stopped on two existing `autocut` failures. No implementation changes were made in the publication session.

- [2026-11-22 — Efekt odaklı TikTok anime edit hedefi](log/2026-11-22-tiktok-fx-edit-referansi.md): [SC-0003](style-cards/SC-0003-theamirhanio-curse-tiktok-edit.md) teknik ön kartı ve ürün hedefi eklendi. Görsel/işitsel efekt çözümlemesi bekliyor.

- [2026-11-22 — Rakiplerden ürün geliştirme dersleri](log/2026-11-22-frameshift-urun-gelistirme-yol-haritasi.md): Canlı timeline, ajan revizyonu ve sabit sürüm export öncelikli ürün önerisi; hız grafiği, metin seçicisi ve matte için kabul senaryoları. Tasarım önerisidir, yeni uygulama veya test sonucu değildir.

- [2026-11-22 — HyperFrames, OpenMotion ve FrameShift olgunluk karşılaştırması](log/2026-11-22-hyperframes-openmotion-frameshift-karsilastirma.md): Asta v3 MP4 yeniden `ffprobe` edildi (1080p60, 1200 kare, 20 sn). Güncel `cargo test --workspace --locked` çıkış 101: iki `autocut` testi kaldı; önceki Faz 6.6 sonucu tarihsel. Canlı editör ve ortak rakip render kıyası doğrulanmadı.

- [2026-11-22 — SC-0002 Yeniden Üretimi: Mushoku Tensei Kinetic Typography AMV ve Yetenek Boşluğu Analizi](log/2026-11-22-mushoku-kinetic-type-reproduction.md)
- [2026-11-22 — Asta // Devil Union — Red Line (v3) 20 sn AMV](log/2026-11-22-asta-v3-red-line.md)
- [2026-11-22 — Faz 6.6: Shader Görsel Kalite Yükseltmesi (Optik/AgX/MSAA, 3D PBR/DoF/Blend, Anime FX/Geçişler/JFA SDF)](log/2026-11-22-faz6-6-shader-visual-upgrade.md)
- [2026-11-22 — Faz 6.5: HarfRust + Skrifa Shaping, WGSL Slug GPU Bézier, Kavisli 3D Bevel & SIL OFL Font Paketi](log/2026-11-22-faz6-5-nextgen-typography-slug.md)
- [2026-11-22 — Faz 6.4: Streaming Decoder, .cube/OCIO, 2D Kolaj/Precomp/HUD/Warp ve 2D TTF VectorText](log/2026-11-22-faz6-4-streaming-ocio-2d-pack-vectortext.md)
- [2026-11-22 — Faz 6.3: PlatsAE-Seviyesi 3D Sahne Motoru & Fame is a Gun v3](log/2026-11-22-faz6-3-platsae-3d-engine-ve-fame-v3.md)
- [2026-11-22 — Addison Rae — Fame is a Gun (3D Kinetik Tipografi Editi)](log/2026-11-22-fame-is-a-gun-3d-kinetic-edit.md)
- [2026-11-21 — Asta // Anti-Magic Devil Union Tam Özellikli Edit](log/2026-11-21-asta-devil-union-full-edit.md)
- [2026-11-21 — Faz 6.2: 3D Kamera, TTF/OTF 3D Extrusion, Parçacıklar & Neon Şeritler](log/2026-11-21-faz6-2-3d-kinetik-tipografi-ve-parcaciklar.md)
- [2026-11-21 — Faz 6.1: Yüksek Görsel Etki Paketi ve Runtime CustomWgsl Sandbox](log/2026-11-21-faz6-1-visual-fx-ve-custom-wgsl.md)
- [2026-11-21 — Edit sürtünmesi doğrulaması](log/2026-11-21-edit-friction-verify.md)
- [2026-11-21 — Edit sürtünmesi uygulaması](log/2026-11-21-edit-friction-fixes.md)
- [2026-11-21 — Edit sürtünmesi araştırması](log/2026-11-21-edit-friction-research.md)
- [2026-11-21 — 3D Kinetik Tipografi Referans İncelemesi ve Mimari Araştırma](log/2026-11-21-3d-kinetik-tipografi-arastirmasi.md)
- [2026-11-21 — Koleedits TikTok dogfood](log/2026-11-21-koleedits-dogfood.md)
- [2026-11-21 — Swarm ile 6 Prodüksiyon Darboğazının Araştırılması ve Çözümü](log/2026-11-21-swarm-performans-ve-motor-iyilestirmeleri.md)
- [2026-11-21 — Gerçek Asta AMV Medyası ile Uçtan Uca Deneme ve Değerlendirme](log/2026-11-21-gercek-asta-amv-denemesi.md)
