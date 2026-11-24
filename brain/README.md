# FrameShift Kalıcı Hafıza Sistemi (`brain/`)

`brain/`, FrameShift projesinde çalışan ajanların oturumlar arası hiçbir mimari kararı, teknik kısıtı, açık soruyu veya öğrenilen dersi kaybetmemesi için tasarlanmış **çok katmanlı kalıcı bellek** yapısıdır.

---

## 3 Katmanlı Bellek Modeli

| Katman | Dizin / Dosya | Ömür & Rol | Değişim Kuralı |
| :--- | :--- | :--- | :--- |
| **1. Oturum Katmanı** | `brain/log/` | Her çalışma oturumunun ham izi, alınan kararlar ve doğrulama kanıtları | **Append-only** (ekle, eskileri değiştirme) |
| **2. Çalışma Katmanı** | [state.md](state.md), [lessons.md](lessons.md), [specs/](specs/README.md), [research/](research/README.md), [style-cards/](style-cards/README.md), `brain/decisions/` (`draft`/`proposed`) | Aktif durum, öğrenilen dersler, motor sözleşmeleri, araştırma raporları ve referans stil kartları (`SC-NNNN`) | Görev sonunda güncellenir / yeni belge eklenir |
| **3. Kalıcı Katman** | `brain/decisions/` (`accepted`) + kök [AGENTS.md](../AGENTS.md) | Kilitlenmiş mimari kararlar (`ADR-0001..0004`) ve bağlayıcı çalışma kuralları | Karar değişirse eski ADR'a dokunulmaz; `supersedes` ile yeni ADR açılır |

Destekleyici indeks ve şablonlar: [index.md](index.md) (tüm `brain/` belleğinin tek sayfa rotası) ve `_templates/` (`arastirma.md`, `gunluk.md`, `karar.md`, `spec.md`, `stil-karti.md`).

---

## Güncel Kod Tabanı Ölçeği (Doğrulanmış Envanter)

Ayrıntılar ve son doğrulama çıktıları için [state.md](state.md) ve [index.md](index.md) esas alınır:
- **Workspace:** 9 Rust crate'i (`frameshift-time`, `frameshift-schema`, `frameshift-media`, `frameshift-vector`, `frameshift-render`, `frameshift-core`, `frameshift-app`, `frameshift-mcp`, `frameshift-cli`).
- **Render & Şema:** **60 WGSL shader** (`crates/frameshift-render/shaders/`), **40 `NodeKind`** (`crates/frameshift-schema/src/graph.rs`), **10 yerleşik `ShaderTransition`**, şema sürümü `0.2.0` ([ADR-0004](decisions/0004-schema-minor-compatibility.md)).
- **Araç Yüzeyi:** **32 MCP aracı** (`crates/frameshift-mcp/src/server.rs`) ve headless CLI (`crates/frameshift-cli/src/main.rs` + `workbench.rs`).
- **Aşama Durumu:** Faz 0–5 (M1–M5), Edit-Friction, Faz 6.1–6.6, Anime Workflow P0, Beş Temel Yetenek (Spectral, AI Stem/Lyrics, Waveform PNG, Anime4K/Real-ESRGAN, Depth/Scene3D) ve Rakip Güç Yol Haritası **P1, P2, P3 & P5** headless olarak uygulanmıştır; **P4** (canlı `egui` + `winit` masaüstü GUI) kullanıcı kararıyla sonraki faza bırakılmıştır.

---

## Okuma ve Yazma Kuralları

### Oturum Başında Okuma
1. [state.md](state.md) — nerede kaldığımızı, aktif fazı, doğrulama durumunu ve açık soruları gör.
2. [index.md](index.md) — ilgili ADR, spec, araştırma, stil kartı ve son günlükleri bul.
3. [AGENTS.md](../AGENTS.md) — çalışma sınırlarını ve teslim kriterlerini teyit et.
4. [lessons.md](lessons.md) — geçmiş tuzakları hatırla.
5. Yalnızca mevcut görevle ilgili `decisions/`, [specs/](specs/README.md), [research/](research/README.md) veya [style-cards/](style-cards/README.md) dosyalarını oku.

### Görev Sonunda Yazma & Sıkıştırma (Consolidation)
1. **Günlük ekle:** `brain/log/YYYY-MM-DD-<konu>.md` dosyası oluştur (`_templates/gunluk.md` şablonuyla).
2. **Durumu güncelle:** [state.md](state.md) içindeki tamamlananları, aktif odağı, sıradaki adımları ve açık kararları güncelle.
3. **Ders çıkar:** Yeni bir hata, kısıt veya yanlış varsayım tespit edildiyse [lessons.md](lessons.md) sonuna ekle.
4. **Sözleşme / Karar / Araştırma / Stil Kartı:** İlgili `_templates/` şablonunu (`spec.md`, `karar.md`, `arastirma.md`, `stil-karti.md`) kullan.
5. **İndeksi tazele:** Yeni eklenen her dosyayı [index.md](index.md) tablolarına ve ilgili alt dizin `README.md` haritasına (`specs/README.md`, `research/README.md`, `style-cards/README.md`) ekle.

---

## Epistemik Disiplin

- Kod yazılmamış veya doğrulanmamış hiçbir bileşeni çalışıyor veya benchmark edilmiş gibi kaydetme.
- Taslak (`draft`/`proposed`) kararları ve araştırma hedeflerini kilitlenmiş (`accepted`/`locked`) kararlarla veya mevcut yeteneklerle karıştırma.
- Her dış teknik iddiada birincil kaynak URL'si veya yerel referans dosya yolu göster.
