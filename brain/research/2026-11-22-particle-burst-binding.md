---
title: Particle burst / emission binding — analitik parçacık alanında vuruş patlaması
date: 2026-11-22
status: draft
tags: [render, schema, wgsl, particles, param-binding, audio-reactive]
---

# Particle burst / emission binding

## Soru / Amaç

Asta v3 üretiminde `ParticleField3D` yalnızca `speed` üzerinden `audio_rms`'e bağlanabildi; `count` ve `seed` statik kaldığı için vuruşlarda "patlama" (doğuş + dışa uçuş + ölüm) üretilemedi. Bu araştırma şu soruları yanıtlar:

1. Mevcut parçacık modeli tamamen analitik/seek-independent mi; burst bu çerçevede nasıl kalır?
2. `count`/`seed` bindable + `emission` (A), burst alt-yapısı + `emission` (B) veya yeni `ParticleBurst` düğümü (C) — hangisi en az karmaşık ve en çok ifade gücü verir?
3. `SafeExpr` f64 sonucu `u32` alana bugün nasıl yazılıyor; güvenli dönüşüm kuralı ne olmalı?
4. Uniform 176 B → ? geçişi, WGSL hizalaması ve pipeline cache etkisi.
5. Doğuş/ölüm zamanı hash'ten türetilebilir mi; alpha envelope; `count` değişimi pop yapar mı?
6. Geriye uyumluluk ve test yüzeyi.

Kapsam dışı: uygulama, render, `cargo`, git. Bu belge yalnızca kanıt ve plan içerir.

## Mevcut Kod Gerçekliği (Kanıtlı)

**Şema düğümü.** `NodeKind::ParticleField3D` 15 alan taşır: `count: u32` (varsayılan 120), `seed: u32` (1), `shape: u32`, `color_a/b_rgba`, `speed`, `spread`, `size`, `camera_pos`, `camera_rot_deg`, `fov_deg`, `time_offset_sec`, `dof_focal_dist`, `dof_blur_strength`, `opacity`. Hiçbirinde burst/emission/lifetime alanı yok: `crates/frameshift-schema/src/graph.rs:840-873`; varsayılanlar `graph.rs:1123-1149`.

**Bindable yüzey.** `bindable_param_names(NodeKind::ParticleField3D)` yalnızca `speed, spread, size, cam_x..cam_rot_z, fov_deg, time_offset_sec, dof_focal_dist, dof_blur_strength, opacity` döndürür: `crates/frameshift-schema/src/project.rs:414-429`. `count` ve `seed` listede yok; doğrulama bu listeye karşı reddeder: `project.rs:698-724`.

**Mevcut test bunu kilitliyor.** `crates/frameshift-schema/tests/schema_tests.rs:1614-1624`, `count` bağlamayı `InvalidParamBinding` ile reddetmeyi assert eder. Yani `count`'u whitelist'e eklemek bu testi bilinçli olarak tersine çevirmek demektir.

**Bağlama modeli (kod ↔ spec farkı).** Kodda `ParamBindingDef` yalnızca `{ node_id, param_name, expression }`: `project.rs:185-189`. Spec ise `binding_id`, `event_id`, `clamp_min`, `clamp_max`, `fallback_value` de içerir: `brain/specs/project-schema.md:123-139`. Sonuç: şema düzeyinde clamp/fallback yok; güvenli aralık ifade içinde veya uygulama anında kurulmalı.

**Evaluator.** `ProjectAnimator::evaluate_node` her sub-frame'de düğümü klonlar, `event_animations` ve `compiled_bindings` değerlerini uygular: `crates/frameshift-render/src/compositor.rs:269-309`. `SafeExpr::eval_default` `f64` döner (`crates/frameshift-time/src/expr.rs:242`), fakat uygulama `value as f32` ile tek kapıdan geçer: `compositor.rs:288` ve `compositor.rs:304`. `apply_animated_parameter` imzası `value: f32`'dir: `compositor.rs:414`; `ParticleField3D` kolu `speed/spread/size/cam_*/fov_deg/time_offset_sec/dof_*/opacity` yazar, `count`/`seed` yazmaz: `compositor.rs:804-832`. Kod tabanında bindable bir `u32` parametre yok; tek `u32` dönüşümü ilgisiz bir yerde: `compositor.rs:4256`.

**Uniform.** `ParticleField3DUniform` 176 B: `view_proj` (64) + 7 × `vec4` = 176; alanlar `crates/frameshift-render/src/uniforms.rs:853-868`, boyut assert'i `uniforms.rs:1069`. `params0 = [speed, spread, size, seed_f32]`, `params1 = [shape, dof_focal, dof_blur, count_f32]`: `compositor.rs:5587-5588`. `count_f32` shader'da **hiç okunmuyor**; shader yalnızca `params0.x/.y/.z/.w` ile `params1.x/.y/.z` okur: `crates/frameshift-render/shaders/particle_field3d.wgsl:56-59,84,87`.

**Shader.** Tek uniform binding; storage buffer, compute pass, ping-pong doku veya geçmiş kare yok: `particle_field3d.wgsl:28-29`. `pcg_hash` + `hash_f32(seed, iid, salt)`: `wgsl:31-40`. Yörünge kapalı formda: `drift_y = fract(...)`, `swirl = sin/cos(t ...)`, `pos_base = hash * spread`: `wgsl:62-80`. Sarmalama kenarı `wrap_fade` ile yumuşatılıyor: `wgsl:102-103`. Alfa fragmentte `particle_color.a * shape_mask * dof_atten`: `wgsl:147-177`.

**Çizim.** Tek başına düğüm: `count == 0 || opacity <= 0.001` erken çıkış, sonra `draw(0..6, 0..count)`: `compositor.rs:3065-3091`. `Scene3D` Phase B: `instance_count: *count`: `compositor.rs:3511-3530`, çizim `draw(0..6, 0..instance_count)`: `compositor.rs:3628-3635`. Bind group ve uniform buffer her çizimde yeniden ayrılır: `compositor.rs:5351-5370`.

**Seek bağımsızlığı testi.** Doğrudan seek ile sıralı render aynı `fnv1a_hash`'i verir: `crates/frameshift-render/tests/faz6_2_tests.rs:272-321`.

**Gerçek proje.** `asta_v3` iki `ParticleField3D` düğümü taşır (`count: 130`, `seed: 77/313`) ve ikisinin de `opacity: 0.0`'dır: `local_media/asta_v3/asta_v3_project.json:1657-1735`. Yani katmanlar kapatılmış, burst üretilememiş.

**Tasarım niyeti.** Stateless analitik modelde doğuş zamanı/ömür de hash'ten kapalı formda hesaplanacaktı: `brain/research/3d-kinetic-typography-architecture.md:70-72`. Mevcut shader bu kısmı uygulamıyor; yalnızca sürekli drift var.

## İncelenen Birincil Kaynaklar

Yerel dosyalar (web kullanılmadı):

- `crates/frameshift-schema/src/graph.rs` — 322-324, 840-873, 1123-1149, 1897-1918
- `crates/frameshift-schema/src/project.rs` — 185-189, 414-429, 594-724, 771
- `crates/frameshift-schema/src/error.rs` — 56-76
- `crates/frameshift-schema/src/version.rs` — 6-10, 29-31
- `crates/frameshift-schema/tests/schema_tests.rs` — 1336-1364, 1614-1624, 1847-1863, 1955-1960
- `crates/frameshift-render/src/compositor.rs` — 224-309, 414-832, 3065-3091, 3511-3530, 3628-3635, 5351-5370, 5548-5590
- `crates/frameshift-render/src/uniforms.rs` — 853-868, 1069
- `crates/frameshift-render/src/pipelines.rs` — 563-587, 664-671, 998-1000, 1215-1224, 1272
- `crates/frameshift-render/shaders/particle_field3d.wgsl` — 1-178
- `crates/frameshift-render/tests/faz6_2_tests.rs` — 272-321
- `crates/frameshift-render/tests/faz6_6_scene3d_tests.rs` — 350-371
- `crates/frameshift-time/src/expr.rs` — 242; `flicks.rs` — 11, 21
- `brain/specs/project-schema.md` — 119-139; `brain/specs/timebase.md` — 22-26
- `brain/research/3d-kinetic-typography-architecture.md` — 70-72
- `local_media/asta_v3/asta_v3_project.json` — 1657-1735

## Doğrudan Gözlemler (Kanıtlı)

- **G1 — Model tamamen analitik.** Shader'da yalnız `@group(0) @binding(0)` uniform var; storage buffer, compute pass, ping-pong doku veya geçmiş kare yok (`wgsl:28-29`). Tüm parçacık nitelikleri `pcg_hash(seed, iid, salt)`'tan (`wgsl:31-40`), konum `t_sec`'in kapalı formundan (`wgsl:62-80`) gelir.
- **G2 — Burst yok, sadece sürekli drift.** Doğuş/ölüm kavramı yok; parçacıklar `fract` ile dikeyde sarılıyor ve `wrap_fade` ile kenarda söndürülüyor (`wgsl:73,102-103`). Shader'da `age`, `lifetime`, `birth` terimleri geçmiyor.
- **G3 — `count` draw çağrısına gidiyor, uniform'a da yazılıyor ama okunmuyor.** CPU `params1.w = count` yazıyor (`compositor.rs:5588`), shader `.w`'yi okumuyor (`wgsl:56-59,84,87`). Yani `params1.w` ölü bir slot; `emission` için yeniden kullanılabilir.
- **G4 — Bindable u32 yolu yok.** `apply_animated_parameter` f32 alır (`compositor.rs:414`), `ParticleField3D` kolunda `count`/`seed` yok (`804-832`); `bindable_param_names` de listelemez (`project.rs:414-429`). Bugün `SafeExpr` f64 → f32 (`288,304`) → alan. `u32`'ye dönüşüm kuralı hiçbir yerde tanımlı değil.
- **G5 — `seed` f32 üzerinden taşınıyor.** `*seed as f32` (`compositor.rs:5587`) ve shader `u32(max(params0.w, 0.0))` (`wgsl:56`). `u32` → `f32` dönüşümü 2^24 üstünde tam sayı hassasiyetini kaybeder; `seed` bugün 24 bit ile sınırlı, sessizce yuvarlanır.
- **G6 — Uniform 176 B, tamamı vec4/mat4.** Hizalama boşluğu yok; `size_of` testi ve WGSL yorumu birlikte güncellenmeli (`uniforms.rs:853,1069`; `wgsl:13-26`).
- **G7 — Üç pipeline aynı struct'ı kullanıyor.** `particle_field3d`, `particle_field3d_scene`, `particle_field3d_scene_msaa4x`: `pipelines.rs:563-587,664-671`; `min_binding_size = size_of::<T>()` (`1215-1224`). Pipeline cache anahtarı yok, `cache: None` (`1272`); pipeline'lar başlangıçta bir kez kurulur.
- **G8 — Bind group/uniform buffer her çizimde yeniden ayrılıyor.** `create_uniform_bind_group` her çağrıda yeni buffer + bind group üretir (`compositor.rs:5351-5370`); 32 B büyüme maliyeti ihmal edilebilir.
- **G9 — Bağlama değeri sub-frame'de yeniden hesaplanıyor.** Motion blur alt örnekleri `evaluate_node`'u tekrar çağırır (`compositor.rs:1610,3287`); `audio_rms`, `beat_phase`, `event_delta_sec` her sub_tick'te tazelenir (`224-266`).
- **G10 — `event_delta_sec` işaretli.** `(sub_tick - event.target_output_tick)` (`compositor.rs:231-233`); olaydan önce negatif. Burst tetikleyen ifade `max(0.0, event_delta_sec)` ile kırpılmalı.
- **G11 — Ses yoksa `audio_rms` sentetik.** Envelope yoksa `beat_phase` tabanlı yedek üretilir (`compositor.rs:247-253`); burst testleri ses dosyası olmadan da kurulabilir.
- **G12 — Şema sürümü minor eşitliği istiyor.** `CURRENT_SCHEMA_VERSION = 0.1.0` (`version.rs:6-10`), uyumluluk `major == && minor ==` (`29-31`), yükleme reddeder (`project.rs:595-596`). `local_media/` altındaki 8 proje JSON'unun tamamı `"0.1.0"` (ör. `asta_v3_project.json:2`, `fame_is_a_gun_v3_project.json:2`); minor bump yapılırsa hepsi reddedilir.
- **G13 — Ekleme yapan alanlar derleme kırar.** `NodeKind::ParticleField3D` üç testte tam struct literal olarak kuruluyor: `faz6_2_tests.rs:279-295`, `faz6_6_scene3d_tests.rs:352-368`, `schema_tests.rs:1847-1863`. Yeni alanlar bu üç yeri güncellemeyi zorunlu kılar.
- **G14 — UI etiketi `..` kullanıyor.** `crates/frameshift-app/src/snapshot.rs:1188-1195` yeni alanlardan etkilenmez.

## Çıkarımlar ve Öneriler

- **C1 — Modeli stateful yapmadan burst mümkün.** Doğuş zamanını `birth = burst_start + hash(seed,iid,8) * jitter`, yaşı `age = t - birth`, ömrü `life = burst_duration` olarak tanımlarsak her kare kapalı formda hesaplanır; seek hangi ana atlarsa atsın aynı sonucu verir. Bu, `3d-kinetic-typography-architecture.md:70-72`'deki niyetin doğrudan devamıdır.
- **C2 — `count` bağlamak yanlış araç.** `instance_count` değişince yeni `iid`'ler o anda görünür; burst'ta bu "geç doğmuş" parçacık ya görünmez ya da yarı ömründe belirir. `seed` değişimi ise tüm hash'leri değiştirir → tek karede tam yeniden dağılım (sert pop). Patlama hissi `count`'tan değil, yaşam döngüsünden gelir.
- **C3 — `emission` yalnız alpha çarpanıysa `opacity` ile çakışır.** `opacity` zaten bindable ve uniform kurulumunda alfa ile çarpılıyor (`compositor.rs:5574,5585-5586`). Yeni bir alan ancak **doğum/yoğunluk kapısı** (hash eşiği) veya **burst genliği** anlamı taşırsa değer katar.
- **C4 — En iyi oran B.** Burst alt-yapısı + `emission` (yoğunluk kapısı), `count`/`seed` statik kalır. A en ucuz ama patlama üretmez; C aynı ifade gücünü daha çok dosyaya yayarak verir.
- **C5 — Zaman alanları Flicks olmalı.** `brain/specs/timebase.md:22-26` kalıcı zaman için f64 saniyeyi yasaklar; `Flicks` `i64` (`flicks.rs:11,21`). `burst_start/duration/period_ticks: Flicks`; yalnız uniform'a yazarken `as f64 / FLICKS_PER_SECOND as f64` ile saniyeye çevrilmeli. Mevcut `time_offset_sec` f32 istisnası yeni alanlara taşınmamalı.
- **C6 — Geriye uyumluluk "legacy dal" ile garanti edilir.** `burst_duration_ticks == 0` → mevcut sürekli drift kodu birebir çalışsın; `emission` varsayılanı 1.0. Böylece eski projeler aynı pikseli üretir.

## Tasarım Seçenekleri (Karar Noktaları)

### Seçenek A — `count` + `seed` bindable + `emission`

- Dokunulan: `project.rs` (whitelist + apply), `graph.rs` (emission alanı), `compositor.rs` (u32 dönüşümü), testler.
- Artı: en az kod; `count` ile yoğunluk, `seed` ile varyasyon.
- Eksi: patlama yok; `count` pop, `seed` tam reshuffle; `emission` `opacity` ile çakışır; mevcut "count bindable değil" testi ters çevrilir.
- Uygun olduğu yer: yalnızca "yoğunluk nefesi" isteniyorsa.

### Seçenek B — Burst alt-yapısı + `emission` (önerilen)

- Yeni alanlar (`graph.rs`, hepsi `#[serde(default)]`): `burst_start_ticks: Flicks`, `burst_duration_ticks: Flicks`, `burst_period_ticks: Flicks` (0 = tek atış), `burst_jitter: f32` (0..1), `burst_decay: f32`, `emission: f32` (1.0).
- `count`/`seed` statik; `emission` hash eşiğiyle yoğunluk kapısı; alpha envelope shader'da.
- Artı: gerçek patlama (doğuş + dışa uçuş + ölüm), periyodik vuruş tekrarı, tek düğümle hem atmosfer hem burst; pop yok (instance sayısı sabit).
- Eksi: ~30 satır WGSL, +32 B uniform, 6 yeni alan, 3 test literal güncellemesi.
- Risk: `emission` eşiği parçacık bazında aç/kapa yaptığı için çok hızlı değişirse parıldama; ifade tarafında yumuşatma gerekir.

### Seçenek C — Yeni `ParticleBurst` düğümü

- Dokunulan: `graph.rs` (yeni varyant + arite + Scene3D çocuk listesi: `1363,1404,2140`), `project.rs`, `compositor.rs` (iki çizim yolu), `snapshot.rs`, testler.
- Artı: statik alan + burst aynı projede ayrı düğümler; parametreler karışmaz.
- Eksi: aynı shader/uniform için ikinci düğüm yolu; en çok dosya ve test; ifade gücü B'den fazla değil (B'de iki `ParticleField3D` zaten aynı işi görür).

| Ölçüt | A | B | C |
|---|---|---|---|
| Yeni schema alanı | 1 | 6 | 6 + yeni tür |
| Dokunulan dosya | 4 | 9 | 11+ |
| Patlama hissi | yok | var | var |
| Pop riski | yüksek | düşük | düşük |
| Geriye uyum | kolay | kolay (legacy dal) | orta |

**Net öneri:** B. `emission` = yoğunluk kapısı; `count`/`seed` statik; `burst_period_ticks > 0` periyodik, `0` tek atış; burst zamanı Flicks.

KARAR GEREKİYOR: A, B veya C'den hangisi uygulanacak? (öneri: B)
KARAR GEREKİYOR: `count` ve `seed` bindable olacak mı? (öneri: hayır; B'de statik kalır)
KARAR GEREKİYOR: `emission` semantiği — hash tabanlı yoğunluk kapısı mı (öneri), yoksa yalnız alpha çarpanı mı?
KARAR GEREKİYOR: burst zaman alanları `Flicks` mi (öneri), yoksa mevcut `time_offset_sec` gibi f32 saniye mi?
KARAR GEREKİYOR: tek atış + periyodik aynı düğümde mi (öneri), yoksa yalnız tek atış mı?
KARAR GEREKİYOR: `seed` 24-bit sınırı — bitcast ile tam `u32` mi (öneri), yoksa `seed <= 0x00FF_FFFF` dokümantasyonu/validasyonu mu?
KARAR GEREKİYOR: `count` üst sınırı (öneri: 8192) ve aşımda hata mı, klempleme mi?

## Uygulama Planı

1. **Schema — alanlar** (`crates/frameshift-schema/src/graph.rs:842-873`): `burst_start_ticks`, `burst_duration_ticks`, `burst_period_ticks` (`Flicks`), `burst_jitter`, `burst_decay`, `emission` ekle; hepsi `#[serde(default)]` + varsayılan fonksiyonları (`graph.rs:1123-1149` bloğuna). `use frameshift_time::Flicks;` ekle (crate bağımlılığı var: `crates/frameshift-schema/Cargo.toml:9`).
2. **Schema — doğrulama** (`graph.rs:1897-1918` deseni): `burst_duration_ticks >= 0`, `burst_period_ticks >= 0`, `period > 0 && period < duration` reddi, `burst_jitter ∈ [0,1]`, `burst_decay > 0`, `emission ∈ [0,1]`, `count <= MAX`. Yeni `SchemaError::InvalidParticleBurst` (`error.rs:56-76` deseni); varyant eklemek güvenli, çünkü `SchemaError` üzerinde exhaustive match yok.
3. **Schema — bindable** (`project.rs:414-429`): `"emission"` ekle; `count`/`seed` eklenmezse `schema_tests.rs:1614-1624` aynen kalır.
4. **Evaluator** (`compositor.rs:804-832`): `"emission" => *emission = value.clamp(0.0, 1.0)`. `count` seçilirse: `*count = value.clamp(0.0, MAX as f32).round() as u32` — açık kural; NaN güvenli (f32 `as` zaten doyurur, ama klempleme niyeti belgeler).
5. **Uniform** (`uniforms.rs:853-868`): `params1.w` = `emission` (ölü `count_f32` yerine), `params2 = [burst_start_sec, burst_duration_sec, burst_period_sec, burst_jitter]`, `params3 = [burst_decay, 0,0,0]` → **208 B**. `uniforms.rs:1069` assert'i 208 yap; WGSL yorumunu (`wgsl:13-26`) güncelle.
6. **Compositor** (`compositor.rs:5548-5590`): Flicks → saniye dönüşümü (`tick as f64 / FLICKS_PER_SECOND as f64`), `params1.w = emission`, `params2/3` doldur. İki çizim yolu (`3065-3091`, `3511-3530`) aynı uniform'u kullanır; ek bayrak gerekmez.
7. **Shader** (`particle_field3d.wgsl`): `params2/3` ekle; `burst_duration_sec > 0` ise burst dalı:
   - `local = t - start`; periyodikse `cycle = floor(local/period)`, `u = local - cycle*period`, `bseed = seed ^ (u32(cycle) * 2654435761u)`; tek atışta `cycle = 0`.
   - `birth = hash_f32(bseed, iid, 8u) * jitter * duration`; `a = u - birth`; `born = a >= 0 && a <= duration`; `age = clamp(a/duration, 0, 1)`.
   - Yön: `dir = normalize(vec3(hx, hy*0.6, hz))`; `pos = dir * spread * speed * age + swirl * age`.
   - `env = smoothstep(0.0, 0.12, age) * pow(1.0 - age, decay)`; `alpha *= emission * env`; `!born → alpha = 0` (fragment `<=0.001` discard, `wgsl:149`).
   - Burst dalında `wrap_fade` uygulanmaz (`wgsl:102-103`).
8. **Test literalleri**: `faz6_2_tests.rs:279-295`, `faz6_6_scene3d_tests.rs:352-368`, `schema_tests.rs:1847-1863` yeni alanlarla güncelle.
9. **Geriye uyumluluk**: `schema_version` **artırılmaz** (`version.rs:29-31`); `burst_duration_ticks == 0` legacy dal; `emission` 1.0; `count`/`seed` dokunulmaz.
10. **Yeni testler**: aşağıdaki Doğrulama Planı.

## Doğrulama Planı

Çalıştırılacak (uygulama aşamasında; bu araştırmada çalıştırılmadı):

```powershell
cargo test -p frameshift-schema
cargo test -p frameshift-render
cargo clippy --workspace --all-targets -- -D warnings
cargo run --release -p frameshift-cli -- verify-all --out-dir target/full_verify
```

Kabul kriterleri:

- **K1 — Eski JSON:** `burst_*` alanları olmayan `asta_v3_project.json` ve `{"type":"particle_field3_d"}` yüklenir; `opacity` 1.0 varsayılanı korunur (`schema_tests.rs:1955-1960` genişletilir).
- **K2 — Round-trip:** yeni alanlarla serileştir→yükle eşitliği; `burst_*_ticks` JSON'da tam sayı (Flicks), ondalık saniye yok.
- **K3 — Legacy eşitlik:** `burst_duration_ticks = 0` olan bir sahnenin `fnv1a_hash`'i, değişiklik öncesi aynı sahnenin hash'i ile aynı (hash uygulama öncesi kaydedilmeli).
- **K4 — Seek bağımsızlığı:** burst ortasında (`start + duration/2`) doğrudan seek ile 0'dan sıralı render aynı `fnv1a_hash` (`faz6_2_tests.rs:301-318` deseni).
- **K5 — Yaşam döngüsü:** `t < start` → boş; `start+0.1*duration` → dolu; `t > start+duration` → boş (alpha sayımı).
- **K6 — Periyodik:** iki ardışık cycle farklı desen üretir (hash farklı), her cycle tekrar üretilebilir (aynı tick → aynı hash).
- **K7 — Emission:** `emission=0` → boş; `1` → dolu; `0.5` → ikisinin arasında `count_non_zero_alpha`.
- **K8 — Uniform:** `size_of::<ParticleField3DUniform>() == 208`; üç pipeline headless derlenir (`compositor_tests.rs:219`).
- **K9 — Validasyon:** negatif süre, `period < duration`, `jitter > 1`, `decay <= 0`, `emission > 1` reddedilir; `count` bindable değilse eski test korunur.

## Riskler ve Doğrulanmamış Noktalar

- **R1 — f32 zaman çözünürlüğü.** `t_sec` f32 (`compositor.rs:3049,3080`); uzun projelerde (saatler) cycle sınırı ve `birth` hesabı 1e-3 s mertebesinde kayabilir. Mevcut modelde de aynı sınır var; burst bunu görünür kılabilir. Doğrulanmadı.
- **R2 — `floor` sınır davranışı.** Tam cycle sınırında `floor` bir önceki/sonraki cycle'a düşebilir; `born` koşulu iki cycle'da da doğru sonuç vermeli. GPU'da test edilmedi.
- **R3 — `emission` eşiği parıldaması.** Hash eşiği keskin; yavaş değişen ifadede bile tek tek parçacıklar aç/kapa yapar. Görsel kabul kullanıcı kararı.
- **R4 — `seed` 24-bit.** Bitcast seçilmezse `seed > 16_777_216` sessizce yuvarlanır (`compositor.rs:5587`, `wgsl:56`). Mevcut projelerde seed küçük (77, 313), bu yüzden bugün görünmüyor.
- **R5 — Spec ↔ kod farkı.** `ParamBindingDef` spec'teki `clamp_min/max` ve `fallback_value` alanlarını taşımıyor (`project-schema.md:123-139` vs `project.rs:185-189`); aralık güvenliği ifadeye bırakılıyor. Bu araştırmada spec güncellenmedi.
- **R6 — Scene3D/MSAA yolu.** Üç pipeline aynı struct'ı kullanıyor (`pipelines.rs:563-587,664-671`); biri güncellenmezse `min_binding_size` uyuşmazlığı çizim anında validasyon hatası verir. Kod incelemesiyle tespit edildi, GPU'da denenmedi.
- **R7 — Görsel kalite.** Patlama yoğunluğu/ölçeği yalnız sayısal testlerle değil, `asta_v3` drop anında render ile değerlendirilmeli; bu araştırmada render alınmadı.
- **R8 — `count` üst sınırı.** Yeni bir sınır getirilirse daha önce geçerli olan büyük `count` değerleri reddedilebilir; gözlenen en büyük değer 220 (`local_media/fame_is_a_gun/fame_is_a_gun_v3_project.json:2627`).
