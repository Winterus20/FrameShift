# 2026-11-22 — Yol haritası uygulama araştırması

[Ürün yol haritası](../research/2026-11-22-frameshift-urun-gelistirme-yol-haritasi.md) dört Luna xhigh ajanıyla incelendi. İncelenen Git revizyonu `d7283b4`.

## Temiz klonda alınan sonuçlar

- `cargo test --workspace --locked` şema testini derleyemedi. Yedi `include_str!` girdisi `.gitignore` kapsamındaki `local_media/` proje dosyalarını bekliyor. Depoya ait sentetik fikstürler ve isteğe bağlı yerel medya korpusu, iki doğrulama türünü yeniden çalıştırılabilir kılar.
- `cargo test --locked -p frameshift-core --lib autocut`: iki test başarısız. Sıfırdaki vuruş atlanıyor ve vuruş olmadığında beklenti ile varsayılan bitiş bayrağı uyuşmuyor. `AudioAnalysisReport.window_ticks` RMS adımıdır. Planlayıcının bitişi ayrı bir plan aralığı olarak alması gerekiyor.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` mevcut `ffmpeg.rs` kullanılmayan import ve gereksiz `mut` uyarılarında durdu. Bu oturumda Rust 1.100.0 ve yazılımsal Vulkan kullanıldı. `verify-all` başlatıldı. Karar soruları gönderildiğinde tamamlanmış bir render sonucu yoktu.

## Uygulama sınırları

- **P0.1:** Temiz klon testlerini, açık `autocut` aralığını ve sentetik Impact Cut örneğini önce ele al. CLI preflight eksik girdiyi çözülemeyen olaydan ayırsın. Vuruş önerisi ile projeye kesim yazma ayrı işlemler olarak sunulsun.
- **P0.2:** Mevcut `ProjectSession` ve `WorkspaceUiController` üzerine timeline, snap ve revizyon + tick + istek kimliği denetimli önizleme kur. Hedef olay taşındığında hedef tick ve çözülen hız eğrisi tek işlemle değişsin. Hazır headless kareyi ilk masaüstü kabuğuna aktar. `eframe/egui` tercihi kullanıcı onayına bağlıdır. [egui-wgpu 0.36 bağımlılıkları](https://docs.rs/crate/egui-wgpu/0.36.0/source/Cargo.toml)
- **P0.3:** Tek adımlı undo ve revizyon denetimi mevcut. Eksikler, gözden geçirilebilir teklif, ortak canlı oturum ve proje ile girdileri sabitleyen yerel export işidir. Çıktıyı geçici yola render edip doğrulamadan sonra yayınla. Mevcut çıktı varsa iptal veya hata onu bozmasın.
- **P1 ve P2:** Hız grafiği motorun kaynak eşlemesini okumalı. Metin seçicisinin shaping kümesi ve yön sözleşmesi, harici matte'in kaynak zamanı ve dönüşümü, precomp'un sınırdan geçen bağımlılıkları ayrı kabul senaryoları ister. Bu kararlar P0.1'i durdurmaz.

Kullanıcıya uygulama kapsamı, masaüstü bağımlılığı ve ajan tekliflerinin onay kuralı soruldu. Bu araştırma sırasında uygulama kodu değiştirilmedi.
