# FrameShift

**Agent-first video editing and compositing, built around an independent native GPU engine.**

FrameShift, mevcut video klipleri, görseller ve seslerden programatik olarak düzenlenebilir editler oluşturmayı hedefleyen bir masaüstü uygulaması ve headless motor projesidir. İlk görsel odak anime editleri, AMV ve yoğun motion graphics içeren kısa videolardır.

> **Durum: tasarım ve araştırma aşaması.** Bu repo şu an yalnızca dokümantasyon içerir. Çalışan motor, masaüstü uygulaması, CLI/API, demo render veya kurulum paketi henüz yoktur. Aşağıdaki özellikler mevcut yetenekler değil, tasarım hedefleridir.

## Temel fikir

Agent, arayüzde fareyle işlem yapmak yerine klipleri, kaynak-zaman eşlemelerini, animasyon eğrilerini, maskeleri ve efekt bağlantılarını sürümlenmiş bir proje tanımı üzerinden oluşturur ve değiştirir.

İnsan ve agent aynı projeyle çalışır. Masaüstü arayüzü yapılan işlemleri gösterir ve elle düzenlemeye olanak verir; proje durumu ve edit/render mantığı arayüze bağımlı olmaz. Aynı çekirdek headless çalıştırıcı tarafından da kullanılabilir.

```text
Desktop UI ─┐
Agent ──────┼── Shared operation API ── Independent edit engine
CLI ────────┘                                  │
                               Frames / previews / final export
```

Bu bir mantıksal sınırdır, kesin süreç mimarisi değildir. Motor masaüstü uygulamasıyla aynı süreçte çalışabilir; pencere kapandığında işlerin nasıl devam edeceği ayrıca tasarlanacaktır.

## Motor hedefleri

1. **Hassas zaman modeli:** Kaynak-zaman eşlemesi, hız eğrileri, freeze/reverse ve ses senkronu; rastgele zamana seek edilebilir değerlendirme.
2. **Sahne ve kompozisyon:** Katmanlar, gruplar, parenting ve precomp; dallanabilen efekt/render grafiği.
3. **Efekt ve geçişler:** Shader tabanlı renk, glow, distortion, blur ve çok girişli geçişler; maske, track matte ve blend işlemleri.
4. **Motion graphics:** Text/vector, animasyon eğrileri, prosedürel çoğaltma, anchor/constraint ve yeniden kullanılabilir görsel rig'ler.
5. **Görüntü doğruluğu:** Float ara tamponlar, açık renk/alpha sözleşmeleri ve zamansal örneklemeli motion blur.
6. **İncelenebilirlik:** Aynı proje sürümü ve zaman aralığına bağlı görsel, video, kodsal ve teknik geri bildirim.

Kapsam aşamalı geliştirilecektir. Tam 3D, otomatik rotoskopi, tracking ve optical flow gibi alanlar başlangıçta tamamlanmış veya ilk sürüme taahhüt edilmiş değildir. Kaynakta olmayan ara kare üretimi ile katman hareketinin motion blur'u ayrı problemlerdir.

## Başlangıç teknoloji yönü

| Alan | Başlangıç adayı / karar |
| --- | --- |
| Çekirdek | Rust |
| Native GPU compositor | wgpu + WGSL |
| Medya giriş/çıkışı | FFmpeg kütüphaneleri |
| Renk yönetimi | OpenColorIO |
| Text/vector | Skia veya uygun alternatif; entegrasyon henüz seçilmedi |
| Proje modeli | Bize ait sürümlenmiş edit formatı; şema henüz tasarlanmadı |

Bu bileşenler henüz kurulmuş veya entegre edilmiş değildir. Desteklenecek işletim sistemleri/GPU'lar, donanım decode/encode, zero-copy yolları ve bağımlılık dağıtım koşulları doğrulanacaktır. UI teknolojisi seçilmemiştir.

## Agent için geri bildirim

- **Kare ve crop:** Kaynak kare, kompozit, maske ve ara node çıktısı.
- **Kısa sesli taslak:** Seçili olayın öncesi ve sonrasıyla hareket/ritim incelemesi.
- **Contact sheet:** Video anlayamayan ama görsel görebilen modeller için zaman etiketli kare şeritleri; genel bakış ve olay çevresinde yoğun örnekleme.
- **Proje incelemesi:** İlgili node'lar, parametreler, animasyon eğrileri ve kaynak-zaman eşlemeleri.
- **Teknik rapor:** Render ayarları, uyarılar ve ölçümler.

Kare şeridi akıcılığı veya ses senkronunu tek başına kanıtlamaz. İnceleme çıktıları neyin gözlendiğini ve neyin doğrulanamadığını açık tutmalıdır. Skill'ler teknik yöntemi, API şemaları kesin kullanım sözleşmesini, örnek projeler ve render'lar da doğrulama kanıtını taşıyacaktır.

## İlk doğrulama hedefi

Dışarıdan sağlanan klip ve müzikle **hız eğrisi + hareketli maske + shader geçişi + temporal motion blur** içeren kısa bir bölümü yalnız proje tanımı üzerinden render etmek.

İlk değerlendirmede kaynak kare seçimi, rastgele seek, ses senkronu, renk/alpha davranışı, önizleme/final tutarlılığı ve GPU maliyeti ölçülecek. Önce motorun bu davranışı üretebildiği doğrulanacak; agent'ın yaratıcı planlaması bunun üzerine kurulacak.

## Dokümantasyon

1. [Proje notları](PROJECT_NOTES.md): Kullanıcı kararları, mimari görüşmeler, başlangıç stack'i ve açık sorular. Tarihsel öneriler ile güncel kararlar ayrı başlıklardadır.
2. [Motor alternatifleri araştırması](ENGINE_ALTERNATIVES_RESEARCH.md): HyperFrames, Remotion, native motorlar ve uygulama/fork adayları; kaynaklar ve doğrulanmamış noktalar.
3. [Görsel referanslar](VISUAL_REFERENCES.md): Kullanıcının verdiği video bağlantıları, örnek kare gözlemleri ve motor tasarımına çıkarımlar.

Araştırma belgeleri bir performans benchmark'ı değildir. Referans videoları ve üçüncü taraf kaynak kodları bu repoya dahil edilmemiştir. Belgelerdeki yerel cache/klasör yolları geliştirme notlarıdır; kullanıcı makinesinde var olmaları beklenmez.

## Lisans ve adlandırma

Proje lisansı henüz seçilmedi. Bağımlılıkların, codec'lerin, fontların ve kullanılacak medyanın lisansları ayrıca değerlendirilecek. Referans bağlantısı bir yeniden dağıtım veya kullanım izni sağlamaz.

**FrameShift** proje adı olarak seçilmiştir; marka, alan adı ve paket adı uygunluğu henüz doğrulanmamıştır.
