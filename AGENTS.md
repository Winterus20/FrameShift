# FrameShift — Agent Talimatları (Single Source of Truth)

Bu dosya, FrameShift projesinde çalışan tüm ajanlar için tek bağlayıcı kural kaynağıdır. Uzun vadeli hafıza ve ayrıntılar `brain/` altında yaşar; aynı bilgi burada tekrarlanmaz.

**Proje Aşaması:** Faz 0–6.6 + Headless P0–P2 + Anime İş Akışı P0 + Native GPU Optik Akış/ONNX + 5 Temel Yetenek + Rakip Güç Yol Haritası (**P1, P2, P3, P5** tamamlandı; **P4** canlı masaüstü GUI sonraki faza bırakıldı). 5 çekirdek spec (`timebase.md`, `project-schema.md`, `render-graph.md`, `effect-shader-api.md`, `inspection-contract.md`) `locked` durumundadır; **9 Rust workspace crate'i**, **60 WGSL shader** (`40 NodeKind`, `schema_version: 0.2.0`), **50 CLI alt komutu** ve **32 MCP aracı** `clippy` **0 uyarı** ve **452/453 workspace testi** (1 önceden mevcut `HEAD` testi) ile doğrulanmıştır.

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
- **Uygulanan Stack:** Rust (9 çekirdek crate) + `wgpu`/WGSL (native GPU compositor, `Rgba16Float` Linear Rec.709, 60 WGSL) + `ffmpeg`/`ffprobe` (`nv12`→Linear GPU decode + opsiyonel `onnxruntime`/`rife-ncnn-vulkan`/`realesrgan-ncnn-vulkan`/`audio-separator`/`stable-ts` sidecar'ları) + saf Rust `.cube`/OCIO renk hattı + `frameshift-vector` (`harfrust`/`skrifa`/`ttf-parser`/`lyon_tessellation`/WGSL Slug tipografi) + JSON `schema_version: 0.2.0` + `Motion.md` deklaratif DSL.
- **Zaman Tabanı:** Kalıcı proje zamanlamasında `f64` saniye yerine `Flicks(i64)` (`1 sn = 705,600,000 tick`) + `Rational32` — [ADR-0002](brain/decisions/0002-rational-timebase-flicks.md) ve [timebase.md](brain/specs/timebase.md) ile kilitli.
- **Doğrulanmış Yetenek:** Dışarıdan klip + müzikle hız eğrisi, hareketli/Bézier maske, shader geçişi, temporal/optik-akış motion blur, `SafeExpr` (`spring`/`wiggle`), 2D/Planar Tracker, `.otio`/FCPXML köprüsü ve `lint`/`diff-frame` doğrulaması doğrudan proje tanımından çalışır.

---

## Komutlar (9-Crate Workspace)

```powershell
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run --release -p frameshift-cli -- verify-all --out-dir target/full_verify
```

- CLI alt komutları (**50 komut**, tam liste için `cargo run -p frameshift-cli -- --help`): Çekirdek (`init-golden`, `render`, `frame`, `inspect`, `ui-snapshot`, `materials`, `peaks`, `waveform`, `autocut`, `mcp-stdio`, `verify-m1`, `verify-all`), Ajan/DSL/NLE/Tracker (`lint`, `diff-frame`, `compile-dsl`, `template-metadata`, `template-instantiate`, `track-point`, `track-planar`, `export-nle`, `import-nle`, `ai-models`, `ai-run`) ve Workbench/AI (`init-impact`, `preflight`, `effect-*`, `proxy*`, `timeline`, `speed-graph`, `snap`, `edit-run`, `template-*`, `media-*`, `retarget`, `export*`, `ai-status`, `ai-stems`, `ai-lyrics`, `upscale`, `interpolate`, `ai-matte`).
- MCP sunucusu (`frameshift-mcp`): `mcp-stdio` üzerinden **32 araç** sunar.
- Önemli: Yeni harici bağımlılık (yeni crate, C++ SDK, `egui`/`winit` pencereleme vb.) eklemeden önce kullanıcıya danış.

---

## Çalışma Sınırları (3-Tier Boundaries)

### Always do (Her zaman yap)
- Oturum başında `brain/state.md` oku; görev sonunda `brain/state.md` güncelle ve `brain/log/YYYY-MM-DD-<konu>.md` kaydı ekle.
- Epistemik sınırı koru: tasarım hedefini mevcut yetenek, contact sheet'i akıcılık/senkron kanıtı, tahmini benchmark olarak sunma.
- Dış kütüphane, lisans veya araç iddiasında birincil kaynak URL'sini (veya yerel dosya yolunu) belirt.
- Yeni bir hata veya kritik mimari ders çıkarıldığında `brain/lessons.md` dosyasına ekle.
- Yeni referans edit videosu incelendiğinde `brain/style-cards/` altına stil kartı (`SC-NNNN`) kaydet; referanssız edit istendiğinde önce `brain/style-cards/README.md` matrisinden uygun stil kartlarını oku.

### Ask first (Önce kullanıcıya sor)
- Yeni Cargo crate'i (ör. `crates/frameshift-gui`) veya workspace'e yeni harici bağımlılık ekleme.
- Herhangi bir harici dependency, FFmpeg build seçeneği veya UI framework seçimi ekleme.
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
- [ ] İlgili teknik doğrulamalar çalıştırıldı (`cargo clippy --workspace --all-targets -- -D warnings`, hedefli/workspace `cargo test`, dokümantasyonda link/`git status` kontrolü).
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

---

## Anime & Ham Klip Arama Sırası

Editler için ham anime klibi, sakuga kesiti veya Twixtor scene pack ararken ajanlar aşağıdaki kaynakları **sırayla** tarar:

1. **Sakugabooru** (`https://www.sakugabooru.com/post`) — Altyazısız/logosuz ham animasyon kesitleri; seri/karakter etiketi + `order:score` ile ara, sağ menüden `"Original"` ile indir.
2. **Animédia / AnimeClipsRaw** (`https://animeclipsraw.fr`) — Karakter/seriye göre ham klipler, flowframe/Twixtor sahneleri ve ses replikleri.
3. **RingWitDaTwixtor** (`https://ringwitdatwixtor.com`) — Popüler anime karakterleri için hazır 4K/60fps Twixtor ve raw scene pack arşivleri.
4. **Hii Twixtor** (`https://hiitwixtor.com`) — Time-remapped / slow-motion (Twixtor) anime kesitleri arşivi.
5. **411 Scenepacks** (`https://411scenepacks.com`) — Anime dahil dizi/film/oyun için ücretsiz HD/4K scene pack dizini.
