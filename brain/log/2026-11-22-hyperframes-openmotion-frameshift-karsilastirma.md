# 2026-11-22 — HyperFrames, OpenMotion ve FrameShift olgunluk karşılaştırması

## Sonuç

FrameShift, anime AMV ve kinetik tipografi için gerçek çıktı üreten bir native GPU motoru ve kodla kullanılabilen bir alfa. Genel amaçlı, kurup açılan bir editör olarak HyperFrames ve OpenMotion'ın ürün katmanının gerisinde. Bu bir rakip render benchmark'ı veya özellik paritesi yüzdesi değil.

| Alan | HyperFrames | OpenMotion | FrameShift |
| --- | --- | --- | --- |
| Motor | HTML/CSS/JS, headless Chrome + FFmpeg | Bağımsız renderer değil; pakette HyperFrames `0.9.7` sabitlenmiş | Rust/wgpu/WGSL, tam sayı Flicks, tek render grafiği |
| Uçtan uca kullanım | CLI, agent skill'leri, tarayıcı editörü ve dağıtık render yolu belgelenmiş | Studio, şablon, medya ve geçmiş; sabit sürümden MP4 export | CLI/MCP ve arayüz denetleyicisi/statik snapshot; canlı editör doğrulanmadı |
| Anime AMV | Katmanlı shader geçişi rotasında sub-frame motion blur açıkça reddediliyor | Motor sürümü farklı; aynı davranış varsayılmadı | Birleşik motor mimarisi; gerçek 1080p60 AMV var, fakat glyph başına yazım ve özne matte akışı hâlâ darboğaz |

## Doğrulama

- `ffprobe` mevcut Asta v3 MP4'te 1920×1080, 60/1 fps, 1200 kare, H.264 + AAC ve 20.000000 sn döndürdü. Video bu oturumda yeniden render edilmedi; görsel eşdeğerlik veya senkron tekrar ölçülmedi. [Önceki render kaydı](2026-11-22-asta-v3-red-line.md#L67-L71).
- `cargo test --workspace --locked` çıkış 101. Core birim paketi 16 geçti, iki `autocut` testi kaldı: vuruşlu 3/4, vuruşsuz 0/1. `AutocutParams::default()` bitişi kapalı bırakıyor; planlayıcı başlangıçtaki sıfır vuruşunu da atlıyor. Beklenen sınır davranışı netleştirilmeden uygulama/test değiştirilmedi. [Planlayıcı ve testler](../../crates/frameshift-core/src/autocut.rs#L7-L90).
- Faz 6.6 için daha önce kaydedilmiş 199/199 ve M1–M5 PASS bu çalışma kopyasının güncel yeşil kapısı değildir. Bu oturumda `verify-all` ve rakip renderları çalıştırılmadı.
- [MCP araç listesi](../../crates/frameshift-mcp/src/server.rs#L135-L202) ve [uygulama yüzeyi](../../crates/frameshift-app/src/lib.rs#L1-L22) koddan incelendi.

## Sonraki adım

Önce test kapısını yeşile döndür; ardından tek dikey akışta canlı timeline/önizleme, revizyon bazlı ajan geri alma ve sabit proje sürümünden export. Hazır bir AMV şablonunda sıfırdan ilk düzenlenebilir taslağa geçen süreyi ve gerekli el müdahalesi sayısını ölç.

## Birincil kaynaklar

- [HyperFrames ürün ve motor dokümanı](https://github.com/heygen-com/hyperframes), [sub-frame blur rota kontrolü](https://github.com/heygen-com/hyperframes/blob/main/packages/producer/src/services/render/motionBlurRoute.ts).
- [OpenMotion Studio ve şablonlar](https://github.com/nexu-io/openmotion), [HyperFrames sürüm sabitlemesi](https://github.com/nexu-io/openmotion/blob/main/package.json), [MCP sürüm sözleşmesi](https://github.com/nexu-io/openmotion/blob/main/docs/mcp-phase1-tools.md).
