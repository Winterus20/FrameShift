# 2026-11-21 — Edit sürtünmesi araştırması

**Tarih:** 2026-11-21  
**Faz:** Faz 0–5 (M1–M5) tamam; bu oturum yalnızca araştırma  
**Durum:** Araştırma yazıldı. Uygulama başlamadı.

---

## Amaç

Koleedits dogfood kesimindeki sekiz sürtünmeyi kilitli spec, bugünkü kod ve birincil dış kaynaklarla eşlemek. Kod, shader, `Cargo.toml`, test, spec ve ADR yazılmadı.

---

## Yapılanlar

1. `brain/research/2026-11-21-edit-friction.md` yazıldı. Sekiz madde için kod satırı, kök neden, reddedilen alternatif, spec/bağımlılık etkisi, risk ve dış kaynak alt bölümü var.
2. `target/dogfood/koleedits/koleedits_project.json` okundu (yeniden render edilmedi). Çıkış düğümü `over_end`; motion blur iç düğüm ve örnek sayısı profille aynı (`2`). Golden sahnede çıkış `motion_blur_out`, düğüm `8`, draft profil `4`.
3. `brain/index.md`, `brain/state.md`, `brain/research/README.md` ve `brain/lessons.md` bu araştırmayı gösterecek şekilde güncellendi.

---

## Doğrulama ve Kanıtlar

- `cargo test` ve `verify-all` bu oturumda çalıştırılmadı.
- CLI ikilisi çalıştırılmadı. `24 fps fps` sonucu `Rational32` gösterimi ile CLI format dizisinin birleşiminden okundu.
- Dış sayfalar 2026-11-21 tarihinde açıldı veya arama sonucunda birincil metin olarak okundu. Liste araştırma dosyasının başındadır.

---

## Açık Kalanlar / Sıradaki Adım

Uygulama yok. Beş soru cevaplanmadan Dilim D, F ve G başlamaz. Dilim A, B ve C ile (soru 1 uygunsa) Dilim E dosya paylaşmadan başlayabilir. Sorular ve dilimler araştırma dosyasında.
