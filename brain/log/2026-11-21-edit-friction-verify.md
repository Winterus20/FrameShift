# 2026-11-21 — Edit sürtünmesi doğrulaması

**Tarih:** 2026-11-21  
**Faz:** Faz 0–5 (M1–M5) tamam; edit-friction uygulaması doğrulandı  
**Durum:** Üç komut da bitti ve geçti. Kaynak kod değişmedi.

---

## Amaç

Edit-friction uygulamasını, özellikle profilin düğüm `sub_samples` değerini ezmediği ve `verify-all`ın 1 örnek kapısını açık bir `ShutterOverride` tavanıyla zorladığı yolu çalıştırmak. Kilitli spec, ADR ve `AGENTS.md` değişmedi. Yeni bağımlılık eklenmedi. Golden hash zayıflatılmadı, test silinmedi.

---

## Yapılanlar

1. Üç komut sırayla, `CARGO_TARGET_DIR=C:\Users\Yigit\Documents\FrameShift\target\friction-verify` ile çalıştı.
2. Başarısızlık olmadığı için kod düzeltilmedi.
3. Bu günlük eklendi. `brain/state.md` ve `brain/index.md` güncellendi. Eski günlükler ve `brain/decisions/` duruyor.

---

## Doğrulama ve Kanıtlar

- `cargo test --workspace`: **geçti** (çıkış kodu 0). 91 test geçti, 0 başarısız. Doc-test çalıştı ve 0 test içeriyordu. Süre yaklaşık 239 sn; derleme `Finished test profile ... in 1m 38s`.
- `cargo clippy --workspace --all-targets -- -D warnings`: **geçti** (çıkış kodu 0). Uyarı basılmadı. `Finished dev profile ... in 56.92s`.
- `cargo run --release -p frameshift-cli -- verify-all --out-dir C:\Users\Yigit\Documents\FrameShift\target\friction-verify\full_verify`: **geçti** (çıkış kodu 0). Rapor `Overall Status: PASS`, `all_gates_passed: true`. M1–M5 tablo satırlarının hepsi PASS. Derleme `Finished release profile ... in 3m 02s`; komutun tamamı yaklaşık 795 sn.
- Blur kapısı (`full_verification_report.json` → `m1.motion_blur`): `passed: true`. Authored `final_sub_samples` 8. Explicit tavan sonrası `draft_sub_samples` 1. `final_hash` `4416298738205128470`, `draft_hash` `5729329321580515486` (hash değişti). `timing_anchors_identical: true`. `mean_l1_channel_diff` `4.026597945601852`. İncelenen tick `1764000000`.
- Contact sheet ve paket PNG'leri akıcılık veya ses senkronu kanıtı değildir. Geçen kapı, komutun bitmiş raporu ve JSON alanlarıdır.

---

## Açık Kalanlar / Sıradaki Adım

Glitch self-blend, LUT/grade siyah tutuşu, font dosyası (ASCII atlas durur), solo modu ve waveform bu turda yok. Sürekli decoder ve pencere kabuğu isteğe bağlı ve onay bekler.
