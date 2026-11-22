# 2026-11-22 — Headless P0–P2 Ürün Yol Haritası Uygulaması

## Kapsam ve kararlar

P0–P2 ürün dilimi, tek proje modeli ve paylaşılan `ProjectSession` üzerinden headless CLI/MCP akışları olarak uygulandı. Ajan komutları, beklenen revizyon tutuyorsa **otomatik uygulanır**; her başarılı mutasyon tek revizyon ve tek undo adımı üretir, önce/sonra yapısal fiş döndürür. Revizyon bayatsa değişiklik reddedilir. Ayrı bir masaüstü onay penceresi veya canlı timeline kabuğu bu dilimin parçası değildir.

## Uygulama

- **P0.1 — Güvenilir başlangıç ve ilk taslak:** Sentetik, düzenlenebilir Impact Cut projesi; araç/medya/şema/ankraj sorunlarını ayıran preflight; açık mutlak zaman aralıklı `autocut`; kesim önerisini olay yazma ve `author_hard_cuts` ile gerçek klip kesiminden ayırma. `autocut` sıfırdaki vuruşu korur, boş analiz/end-point bayraklarını ve minimum segment aralığını açıkça işler.
- **P0.2 — Ortak timeline komut yüzeyi:** `frameshift-app` denetleyicisi ve `TimelineProjection` headless katmanda scrub/playhead, snap, split/trim, olay taşıma/retarget, revizyon geçmişi ve kaynak/çıktı hız grafiği örneklemesi sağlar. Hız eğrisi proje zaman eşlemesinden örneklenir; UI durumunun proje gerçeği haline gelmesi yoktur.
- **P0.3 — Ajan fişi ve sabit export:** MCP `mutate_project`, başarıda otomatik uygular; `list_edit_receipts` ve `.edits.json` değişiklik fişlerini tutar. Fiş kimlik/etkilenen aralıklar, taşınan olaylar, korunmuş yaratıcı kararlar ve önce/sonra yapısal özet taşır. MCP mutasyonunda ses/görsel preview varsayılan olarak `not_generated`; `edit-run` isteğe bağlı kare/kısa MP4 önizlemesi üretebilir. `ExportQueue` proje ve gerekli medya/font/LUT baytlarını kuyruğa alırken sabitler; MP4'ü ffprobe ile kontrol eder, raporu `.export.json` yazar ve doğrulanan staging çıktısını yayınlar.
- **P1 — AMV hızlandırıcıları:** `VectorText` için grafem/kelime aralığı, stagger, pozisyon, ölçek ve opaklık animasyonu; doğrudan Alpha `MaskMatte` girdilerinde ayrı video varlıkları aynı `clip_id` kullanıyorsa eşlenmiş kaynak zamanı ve `SourceFit` eşitliği kontrolü; matte `AssetDef.duration_ticks` tanımlıysa kaynak zaman kapsamı doğrulaması.
- **P2 — Yeniden kullanım ve medya:** Yerel, gözden geçirilmiş typed-slot template, konservatif precompose/unprecompose, medya manifesti/relink, SHA-256 kaynak baytına bağlı gerçek video proxy cache'i, kontrollü `proxy-preview` ve efekt preset/inspect/enable komutları. Template grafiği root veya nested precomp içinde `CustomWgsl` düğümü taşıyamaz.

Başlıca yüzeyler: [CLI workbench](../../crates/frameshift-cli/src/workbench.rs), [ortak timeline](../../crates/frameshift-app/src/timeline.rs), [oturum komutları](../../crates/frameshift-schema/src/session.rs), [ajan fişi](../../crates/frameshift-app/src/proposal.rs), [sabit export](../../crates/frameshift-core/src/export.rs), [template doğrulaması](../../crates/frameshift-schema/src/template.rs) ve [project matte doğrulaması](../../crates/frameshift-schema/src/project.rs).

## Doğrulama kanıtı

- `target/product-roadmap/impact-draft.mp4` **gerçek 4.00 sn H.264/AAC çıktı**, 24 fps ve **96 kare** olarak doğrulandı.
- CLI smoke: timeline projeksiyonu, speed graph, template açma ve 4.25 sn hedefe retarget adımları çalıştı.
- Gerçek MP4 proxy smoke'u: kaynak dosya baytları değişince eski proxy cache anahtarı kullanılmadı. `proxy-preview`, 64×48 kaynaktan 32×24 `yuv420p` proxy kareyi çıkardı; aspect ratio ve PTS eşitliği doğrulandı.
- MCP smoke'u: revision 1'deki geçerli ajan mutasyonu otomatik uygulandı ve receipt oluşturdu; aynı eski revizyonla mutasyon stale conflict aldı. `--out` ile kapanışta proje ile `.edits.json` receipt sidecar'ı yazıldı.
- Export smoke'u: revizyon 1'de kuyruğa alınan tek-kare H.264/AAC iş, insan adımı projeyi revizyon 2'ye taşıdıktan sonra da manifestte revizyon 1'i üretti.
- P1 GPU matte entegrasyonunda source/matte aynı clip ID ile aynı crop/retime rotasını izledi; kaynak zaman tick'leri eşleşti ve alfa sonrası RGB farkı en çok 1 LSB oldu. Süresi yetersiz declared matte fikstürü şema doğrulamasında reddedildi.
- Yeni hedefli schema regresyonları: paylaşılan clip alpha matte kapsam/kırpma testi **1/1**, root ve nested precomp `CustomWgsl` template yasağı **1/1** geçti. Template paketleri ve matte validation için proje şema sürümü artırılmadı.
- Efekt rafı parametre metadata kontrolü `frameshift-core` workbench hedefli koşusunda **4/4** geçti. Bu targeted test build'i son metadata descriptor eklemesinden önceydi; descriptor değişikliğinden sonra strict workspace clippy geçti.
- `cargo clippy --workspace --all-targets -- -D warnings` geçti. Gerçek Impact Cut üzerinde `preview-benchmark` lavapipe smoke'u (n=3): init ~2068 ms; cold p50 ~59.78 ms / p95 ~318.65 ms; warm p50 ~25.66 ms / p95 ~29.92 ms; son kare cache telemetrisi 4 hit / 4 miss. Bu örnek performans genellemesi değildir.

## Test durumu ve sınırlar

- İlk no-fail-fast workspace koşusu **310 geçti / 10 kaldı**. Temiz `d7283b4` archive üzerinde aynı yazılım Vulkan adaptörüyla 8 mevcut başarısızlık doğrulandı; çalışma dalındaki 2 yeni split/Hermite kırpma regresyonu düzeltildi. Sonraki hedefli split image ve kaynak tick koşuları geçti; `faz6_2` hedefli seti bundled font fallback ile **5/5** geçti.
- Nihai kabul koşusu **318 geçti / 0 başarısız / 6 filtrelendi**. Filtrelenen altı test, clean HEAD'de aynı yazılım Vulkan/Windows font koşulunda doğrulanmış baseline görsel/platform başarısızlıklarıdır. Koşu split pixel/time düzeltmesini, bundled font değişikliğini ve hedefli schema regresyonlarını içerdi. Çalıştırma kaydı: `../../../frameshift-toolchain/test-roadmap-acceptance.log`.
- `verify-all`, yazılım Vulkan/lavapipe adaptöründe M1 sırasında **300 sn timeout** ile `exit 124` verdi; yalnız golden project JSON'i üretildi. Donanım GPU'sunda ayrı sonuç bekleniyor.
- Henüz yok: etkileşimli masaüstü timeline/önizleme; export kuyruğunun süreçler arası kalıcı kurtarması; MCP `mutate_project` içinde otomatik sesli/görsel preview; reel-time waveform; genel mask/depth/`text_behind` girdisi. Template `CustomWgsl` düğümlerini reddeder, ancak template manifestindeki yerel yollar dosyaları kopyalamaz veya lisans uygunluğunu kanıtlamaz. Proxy'nin SHA-256 geçersizleştirme smoke'u gerçek MP4 yoluna aittir; sentetik proxy yolu kaynak içerik hash'iyle anahtarlanmıyor.
- Metin seçicisi şu an `VectorText` için position/scale/opacity sağlar. Döndürme/renk, tam Unicode UAX #29 grapheme garantisi ve RTL kabul seti tamamlanmış değildir.
