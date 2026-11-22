# FrameShift Kalıcı Hafıza Sistemi (`brain/`)

`brain/`, FrameShift projesinde çalışan ajanların oturumlar arası hiçbir mimari kararı, teknik kısıtı, açık soruyu veya öğrenilen dersi kaybetmemesi için tasarlanmış **3 katmanlı kalıcı bellek** yapısıdır.

---

## 3 Katmanlı Bellek Modeli

| Katman | Dizin / Dosya | Ömür & Rol | Değişim Kuralı |
| :--- | :--- | :--- | :--- |
| **1. Oturum Katmanı** | `brain/log/` | Her çalışma oturumunun ham izi, alınan kararlar ve doğrulama kanıtları | **Append-only** (ekle, eskileri değiştirme) |
| **2. Çalışma Katmanı** | `brain/state.md`, `brain/lessons.md`, `brain/specs/`, `brain/research/`, `brain/decisions/` (draft/proposed) | Aktif durum, öğrenilen dersler, çekirdek motor sözleşmeleri ve araştırma notları | Görev sonunda güncellenir / yeni spec eklenir |
| **3. Kalıcı Katman** | `brain/decisions/` (`accepted`) + kök `AGENTS.md` | Kilitlenmiş mimari kararlar ve bağlayıcı kurallar | Karar değişirse eski ADR'a dokunulmaz; `supersedes` ile yeni ADR açılır |

---

## Okuma ve Yazma Kuralları

### Oturum Başında Okuma
1. `brain/state.md` — nerede kaldığımızı, aktif fazı ve açık soruları gör.
2. `brain/index.md` — ilgili ADR, spec, araştırma ve son günlükleri bul.
3. `AGENTS.md` — sınırları ve teslim kriterlerini teyit et.
4. `brain/lessons.md` — geçmiş tuzakları hatırla.
5. Yalnızca mevcut görevle ilgili `decisions/`, `specs/` veya `research/` dosyalarını oku.

### Görev Sonunda Yazma & Sıkıştırma (Consolidation)
1. **Günlük ekle:** `brain/log/YYYY-MM-DD-<konu>.md` dosyası oluştur (`_templates/gunluk.md` şablonuyla).
2. **Durumu güncelle:** `brain/state.md` içindeki tamamlananları, aktif odağı, sıradaki adımları ve açık kararları güncelle.
3. **Ders çıkar:** Yeni bir hata, kısıt veya yanlış varsayım tespit edildiyse `brain/lessons.md` sonuna ekle.
4. **Sözleşme / Karar:** Bir teknik sözleşme netleştiyse `brain/specs/` altında belge aç; mimari karar alındıysa `brain/decisions/NNNN-<kisa-ad>.md` yaz.
5. **İndeksi tazele:** Yeni eklenen her dosyayı `brain/index.md` tablolarına ekle.

---

## Epistemik Disiplin

- Kod yazılmamış hiçbir bileşeni çalışıyor veya benchmark edilmiş gibi kaydetme.
- Taslak (`proposed`) kararları kilitlenmiş (`accepted`) kararlarla karıştırma.
- Her dış teknik iddiada birincil kaynak URL'si veya yerel referans dosya yolu göster.
