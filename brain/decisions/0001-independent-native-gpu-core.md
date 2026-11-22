---
id: 0001
status: accepted
created: 2026-11-20
valid_from: 2026-11-20
tags: [engine, shaders, ui, agent]
supersedes: null
---

# 0001 — Bağımsız Native GPU Çekirdek ve Masaüstü + Headless Sınırı

## Bağlam

FrameShift'in ilk görsel odağı anime editleri, AMV ve yoğun motion graphics içeren kısa videolardır. Bu tarzda hız eğrileri, hareketli maskeler, çok girişli özel shader geçişleri, `16-bit float` ara tamponlar ve sub-frame temporal motion blur aynı anda ve deterministik biçimde çalışmak zorundadır. Ayrıca kullanıcı, agent'ın işlemlerini görebilmek ve gerektiğinde timeline üzerinden elle müdahale edebilmek için tek bir masaüstü uygulaması istemekte; aynı çekirdeğin UI olmadan headless çalıştırılabilmesi de beklenmektedir.

## Karar

1. **Bağımsız Çekirdek Motor:** Proje durumu, edit işlemleri, zamanlama, render grafiği ve undo geçmişi UI bileşenlerinden bağımsız bir çekirdek kütüphanede yaşar. UI içine gizli edit durumu gömülmez.
2. **Ortak Sözleşme:** Masaüstü UI, agent ve CLI/headless çalıştırıcı aynı sürüm kontrollü proje tanımını ve motor operasyon sözleşmesini kullanır.
3. **Başlangıç Native GPU Stack Adayı:**
   - **Çekirdek:** Rust
   - **GPU Compositor:** `wgpu` + WGSL
   - **Medya I/O:** FFmpeg kütüphaneleri (dev bir FFmpeg CLI filtre zinciri yerine kütüphane tabanlı decode/ses/encode)
   - **Renk Yönetimi:** OpenColorIO
   - **Text/Vector:** Skia veya uygun alternatif (entegrasyon yolu henüz kesinleşmedi)
   - **Proje Formatı:** Sürümlenmiş, okunabilir/düzenlenebilir proje tanımı
4. **Süreç ve Paketleme Esnekliği:** Tek masaüstü uygulaması içinde motor aynı süreçte çalışabilir; kesin süreç ayrımı, UI teknolojisi ve pencere kapanınca devam eden iş yaşam döngüsü ayrı tasarımla çözülecektir.

## Gerekçe

- Web/DOM tabanlı katmanlı motor incelemelerinde (bkz. [PROJECT_NOTES.md](../../PROJECT_NOTES.md#L256)) sub-frame temporal motion blur ile shader geçişlerinin bir arada çalıştırılmasında mimari kısıtlar gözlenmiştir.
- `wgpu` + WGSL tarayıcı gereksinimi olmadan Vulkan/Metal/DirectX üzerinde tek bir unified render grafiği kurmaya olanak tanır.
- Çekirdek/UI ayrımı hem insanın timeline müdahalesini hem de agent'ın programatik ve headless üretimini aynı doğruluk sözleşmesine bağlar.

## Sonuçlar ve Kısıtlar

- **Henüz kodlanmamıştır:** Bu karar onaylanmış mimari yönü sabitler; herhangi bir Rust crate'i, UI framework'ü veya FFmpeg build'i henüz kurulmamıştır.
- **Doğrulanması gerekenler:** Hardware decode → GPU doku → encode zero-copy yolları, Skia/`wgpu` entegrasyonu ve FFmpeg codec/lisans dağıtım koşulları uygulama öncesi ayrıca doğrulanacaktır.
- **Geliştirme sırası:** Önce çekirdek edit/render motoru ve ilk doğrulama sahnesi hedeflenir; UI, sohbet ekranı ve ince MCP katmanı bunun üzerine gelir.
