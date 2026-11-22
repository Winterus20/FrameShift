# FrameShift — Agent Talimatları (Single Source of Truth)

Bu dosya, FrameShift projesinde çalışan tüm ajanlar için tek bağlayıcı kural kaynağıdır. Uzun vadeli hafıza ve ayrıntılar `brain/` altında yaşar; aynı bilgi burada tekrarlanmaz.

**Proje Aşaması:** **Faz 0–5 (M1–M5) + Anime FX & Geçiş Kütüphanesi** tamamlandı ve doğrulandı. 5 çekirdek spec (`timebase.md`, `project-schema.md`, `render-graph.md`, `effect-shader-api.md`, `inspection-contract.md`) `locked` durumundadır; 9 Rust workspace crate'i (`frameshift-time`, `frameshift-schema`, `frameshift-media`, `frameshift-vector`, `frameshift-render`, `frameshift-core`, `frameshift-mcp`, `frameshift-app`, `frameshift-cli`) ve **15 WGSL shader'ı** **58/58 test** ve `verify-all` + `fx_showcase_pack.png` kapısıyla doğrulanmıştır.

---

## Oturum Başı Okuma Rotası (30 sn)

Her oturumun ilk adımında şu sırayla oku:
1. `brain/state.md` — aktif faz, sıradaki adımlar, açık sorular.
2. `brain/index.md` — tüm karar, spec, araştırma ve günlük kayıtları.
3. `AGENTS.md` — çalışma sınırları ve teslim kriterleri (bu dosya).
4. `brain/lessons.md` — tekrar edilmemesi gereken dersler.
5. Görevle ilgili `brain/decisions/`, `brain/specs/` ve `brain/research/` dosyaları (yalnızca gerekenler).

---

## Onaylanan Mimari Yön (Özet)

- **Hedef Ürün:** Agent-first video editörü ve bağımsız native GPU kompozisyon motoru (ilk görsel odak: anime editleri, AMV, motion graphics).
- **Süreç ve Çekirdek Ayrımı:** Bağımsız çekirdek kütüphane + tek masaüstü uygulaması + headless CLI/API desteği. Tüm edit durumu ortak proje tanımı ve motor sözleşmesinde yaşar.
- **Başlangıç Stack Adayı:** Rust (çekirdek) + wgpu/WGSL (native GPU compositor) + FFmpeg kütüphaneleri (medya I/O) + OpenColorIO (renk yönetimi) + Skia veya uygun alternatif (text/vector, kesin değil) + sürümlenmiş okunabilir proje formatı.
- **Zaman Tabanı:** Kalıcı proje zamanlamasında `f64` saniye yerine tam sayı tick / rasyonel zaman (güçlü aday: Flicks, `1 sn = 705,600,000 tick`) — ilk koddan önce `brain/specs/` ve `0002` ADR'siyle kilitlenecek.
- **İlk Doğrulama Hedefi:** Dışarıdan klip + müzik ile `hız eğrisi + hareketli maske + shader geçişi + temporal motion blur` içeren kısa sahneyi doğrudan proje tanımından render etmek.

---

## Komutlar (9-Crate Workspace, M1–M5)

```powershell
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run --release -p frameshift-cli -- verify-all --out-dir target/full_verify
```

- CLI alt komutları: `init-golden`, `render`, `frame`, `inspect`, `ui-snapshot`, `mcp-stdio`, `verify-m1`, `verify-all`.
- Önemli: Yeni büyük harici bağımlılık (C++ OCIO SDK, pencereleme framework'ü vb.) eklemeden önce kullanıcıya danış.

---

## Çalışma Sınırları (3-Tier Boundaries)

### Always do (Her zaman yap)
- Oturum başında `brain/state.md` oku; görev sonunda `brain/state.md` güncelle ve `brain/log/YYYY-MM-DD-<konu>.md` kaydı ekle.
- Epistemik sınırı koru: tasarım hedefini mevcut yetenek, contact sheet'i akıcılık/senkron kanıtı, tahmini benchmark olarak sunma.
- Dış kütüphane, lisans veya araç iddiasında birincil kaynak URL'sini (veya yerel dosya yolunu) belirt.
- Yeni bir hata veya kritik mimari ders çıkarıldığında `brain/lessons.md` dosyasına ekle.
- Yeni referans edit videosu incelendiğinde `brain/style-cards/` altına stil kartı (`SC-NNNN`) kaydet; referanssız edit istendiğinde önce `brain/style-cards/README.md` matrisinden uygun stil kartlarını oku.

### Ask first (Önce kullanıcıya sor)
- İlk kodlama / `Cargo.toml` / crate scaffold'u başlatma.
- Herhangi bir harici dependency, crate, FFmpeg build seçeneği veya UI framework seçimi ekleme.
- `brain/decisions/` altına `status: accepted` yeni bir ADR yazma (veya mevcut bir `accepted` kararı supersede etme).
- `AGENTS.md` içeriğini değiştirme veya kapsamı genişletme.

### Never do (Asla yapma)
- `brain/decisions/` ve `brain/log/` dosyalarını geriye dönük düzenleme veya silme (append-only; karar değişirse `superseded by NNNN` + yeni ADR).
- Edit durumunu, zamanlamayı veya undo geçmişini UI bileşenlerinin içine gizli state olarak gömme.
- Kalıcı proje zaman tabanı veya kaynak-zaman ankrajı için `f64` saniye kullanma.
- Çalıştırılmamış veya incelenmemiş bir özelliği "çalışıyor / doğrulandı" olarak raporlama.
- `AGENTS.md` dosyasını 150 satırın üzerine şişirme; ayrıntıları `brain/` altında tut.

---

## Things to Avoid (Bilinen Tuzaklar)

- **Web/DOM katmanlı efekt sınırları:** Shader geçişi + sub-frame temporal motion blur birlikte gerektiğinde web tabanlı katmanlı yaklaşımlara dönme.
- **Preset ezberi:** Sabit geçiş/efekt isimlerine sıkışmak yerine dallanabilen render grafiği ve birleştirilebilir efekt sözleşmesi kur.
- **Önizleme/final ayrışması:** Proxy veya taslak önizlemede zamanlama ya da maske semantiğini değiştirme.
- **Spekülatif skill kalabalığı:** Çalışmayan motor için yüzlerce skill/doküman yazma; çekirdek sözleşmeleri önce kilitle.

---

## Definition of Done (Teslim Kriteri)

- [ ] İstenen değişiklik kapsam sınırları içinde tamamlandı ve tasarım/kod gerçekliği çarpıtılmadı.
- [ ] Varsa teknik doğrulamalar çalıştırıldı (şu an: markdown/link/tutarlılık ve `git status` kontrolü).
- [ ] `brain/state.md` güncel durumu yansıtıyor.
- [ ] `brain/log/YYYY-MM-DD-<konu>.md` oturum günlüğü eklendi ve `brain/index.md` güncellendi.

---

## Where to Look (Hızlı Harita)

- [PROJECT_NOTES.md](PROJECT_NOTES.md) — ayrıntılı vizyon, kullanıcı kararları, arşivlenmiş araç incelemeleri.
- [README.md](README.md) — depo özeti ve genel mimari şeması.
- [ENGINE_ALTERNATIVES_RESEARCH.md](ENGINE_ALTERNATIVES_RESEARCH.md) — motor ve kütüphane alternatifleri araştırması.
- [VISUAL_REFERENCES.md](VISUAL_REFERENCES.md) — referans video ve kare gözlemleri.
- [brain/style-cards/README.md](brain/style-cards/README.md) — referans editlerden çıkarılan görsel stil ve kurgu reçetesi kartları.
- [brain/index.md](brain/index.md) — kalıcı hafıza dizini (`decisions/`, `specs/`, `research/`, `style-cards/`, `log/`, `_templates/`).
