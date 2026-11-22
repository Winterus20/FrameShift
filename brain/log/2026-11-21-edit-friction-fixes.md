# 2026-11-21 — Edit sürtünmesi uygulaması

**Tarih:** 2026-11-21  
**Faz:** Faz 0–5 (M1–M5) tamam; edit-friction uygulaması  
**Durum:** Implementation of the edit-friction work landed. `verify-all` bu oturumda çalıştırılmadı.

---

## Amaç

[research/2026-11-21-edit-friction.md](../research/2026-11-21-edit-friction.md) içindeki sürtünme dilimlerini koda almak. Bu kapanış oturumu, aynı varlığı paylaşan iki `SourceVideo` düğümünün kendi `SourceFit` değerini decode isteğine taşımasını bitirdi. Kilitli spec, ADR ve `AGENTS.md` değişmedi. Yeni bağımlılık, codec veya FFmpeg derlemesi eklenmedi.

---

## Yapılanlar

Implementation of the edit-friction work landed. Kullanıcı kararları: shockwave ring-only, opt-in fit, node-owned `sub_samples`, `enabled` bayrağı, gömülü atlas.

1. **Araştırma:** [research/2026-11-21-edit-friction.md](../research/2026-11-21-edit-friction.md).
2. **CLI:** Başarı satırı fps'i bir kez basar. Zaman bayrakları Flicks, `1.905s` ve `f:<frame>` kabul eder. `peaks` cue/beat tick basabilir. Varsayılan profil Final kalır. Waveform yok.
3. **Hard cuts:** `author_hard_cuts` sıralı, örtüşmeyen klipleri tam sayı Flicks artı bir Blend zinciri olarak yazar. Aralık dışı kaynaklar şeffaf kalır.
4. **SafeExpr:** Bilinmeyen parametre adları `Project::validate` içinde `InvalidParamBinding` olarak düşer. Değerlendirme hataları `RenderError::Expression` olur ve base değeri korumaz. `t_sec` türetilmiş bir uniform olarak kalır. `step(1.905, t_sec) * (1 - step(1.980, t_sec))` değerlendirilir.
5. **Shockwave:** `radius <= 0` halka maskesini sıfırlar. `zoom_blur_strength` çalışmaya devam eder. Halka maskesini kullanmayan küçük bir distance chromatic terim yerinde bırakıldı.
6. **Schema:** `SourceVideo.fit` `stretch|cover|contain`, varsayılan `stretch`. `GraphNodeDef.enabled` varsayılan `true`. Kapalı bir 1 girdili düğüm girdisini kopyalar; 2 girdili bir düğüm base döner; bir üretici şeffafa temizler.
7. **Fit pikselleri:** Stretch ffmpeg `-s`. Cover scale+crop. Contain scale+pad, pad rengi şeffaf siyah. Aynı varlığı paylaşan ve farklı mod isteyen iki `SourceVideo` düğümü artık her biri kendi çözülmüş karesini alır; stretch ve cover aynı tamponu paylaşmaz.
8. **Blur:** Her `MotionBlurShutter` yazılmış `sub_samples` değerini tutar. Bir profil onu değiştirmez. `ShutterOverride.sub_samples` açık bir tavandır (`min(authored, ceiling)`). CLI `node N, effective M` basar. `verify-all`'ın 1 örnek kapısı o tavandır. `verify-all` bu oturumda çalıştırılmadı.
9. **Bu turda yok:** glitch self-blend, LUT/grade siyah tutuşu, bir font dosyası (ASCII atlas durur), solo modu, waveform.

Per-node fit yolu: `GpuCompositor` kaynak düğümde `NodeKind::source_fit` okur ve `FrameProvider::provide_source_frame` ile şema `SourceFit` değerini ister. `ProjectFrameProvider` disk videoyu o modla çözer. Önbellek anahtarı `asset@widthxheight:mode` biçimindedir. Sentetik, tuval boyutunda çizilen varlıklar ayrı bir kırpma yolu almaz. İkinci bir proje alanı eklenmedi.

---

## Doğrulama ve Kanıtlar

- `CARGO_TARGET_DIR=target/friction-fit` ile `cargo test -p frameshift-media -p frameshift-core -p frameshift-render` geçti (media 12, core 10, render 25).
- `disagreeing_source_nodes_decode_their_own_fit` ve `disagreeing_source_nodes_keep_separate_fitted_frames` geçti. Stretch pikselleri tarihsel `-s` decode ile aynı kaldı. Cover ayrı bir tampon kullandı ve pikselleri stretch ile aynı değildi.
- Aynı üç pakette `cargo clippy --all-targets -- -D warnings` uyarı vermeden bitti.
- `verify-all` çalıştırılmadı.

---

## Açık Kalanlar / Sıradaki Adım

Glitch self-blend, LUT/grade siyah tutuşu, font dosyası, solo modu ve waveform bu turda yok. Sürekli decoder ve pencere kabuğu hâlâ isteğe bağlı ve onay bekler.
