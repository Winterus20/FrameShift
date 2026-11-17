# Kullanıcının anime edit referansları

## Kaynak ve inceleme sınırı

Kullanıcı aşağıdaki üç videoyu görsel hedef olarak verdi; özellikle üçüncüdeki yoğun kompozisyonu vurguladı. Videolar yt-dlp ile inceleme cache'ine indirildi, ffprobe ile teknik metadata alındı ve her videodan 24 zaman etiketli örnek kare çıkarılarak görsel olarak incelendi. Üçüncü için daha büyük, yoğunlaştırılmış kare şeridi de incelendi. Kesintisiz video oynatımı veya ses dinleme değerlendirmesi yapılmadı; akıcılık/beat senkronu doğrulanmış değildir. Kaynak proje dosyaları yok; efektin nasıl üretildiği kesin olarak bilinemez. Referans alma telif veya yeniden dağıtım izni anlamına gelmez.

1. https://www.tiktok.com/@koleedits/video/7700201309508603137
2. https://www.tiktok.com/@tastxv2/video/7687595643350600968
3. https://www.tiktok.com/@zwxeditor/video/7681338133312474389

Metadata indirilen kopyaya aittir; orijinal proje/üretim fps veya kalitesini kanıtlamaz. Geçici indirmeler /root/.hermes/cache/scratch/anime-references altında; scratch kalıcı arşiv değildir.

## Görsel gözlemler

### 1. koleedits

Örnek karelerde aksiyon ağırlıklı kaynak görüntü, kırmızı/mavi yüksek kontrastlı görünüm, ışıklı vuruşlar ve büyük kırmızı tipografi görülüyor. Bazı kareler kararmış/siyah. Kaynak animasyonda zaten bulunan efekt ile editte eklenen efekt ayrı kaynak olmadan kesin ayrıştırılamaz.

### 2. tastxv2

Karakter, büyük yazılar, kırmızı/beyaz grafik kartlar, müzik oynatıcı benzeri kart, bulut/gökyüzü ve şehir/heykele benzer arka planlarla tasarlanmış kompozisyonlar görülüyor. Bazı kart ve metinler perspektifli. Sahne grupları, text/vector, precomp ve 2.5D katman yerleşimi için referans.

### 3. zwxeditor: öncelikli yoğun kompozisyon referansı

Karanlık/yüksek kontrastlı karakterler, kırmızı/sarı/cyan vurgular, kenar/çizgi görünümü, karakter/yüz çevresine yerleştirilmiş küçük beyaz sayısal metinler, noktalı eğri/bağlantı çizgileri ve düğüm benzeri noktalar görülüyor. Bazı karelerde karakter parçaları veya silüet vurguları, blur/distortion görünümü var. Bunların gerçek tracker, koordinat ölçümü veya 3D geometri olduğu doğrulanmadı; dekoratif grafikler de olabilir.

Görsel yoğunluk yalnız obje sayısı değildir: ince çizgiler, mikro-tipografi, yüksek kontrastlı kenarlar ve katman düzeni önemli. Videonun gerçekten binlerce ayrı objeyle oluşturulduğu iddia edilmiyor.

## Motora yönelik çıkarımlar: uygulanmış özellik değil, öneri

1. Scene graph ile grup/parenting, render graph ile görüntü işleme bağlantılarını ayırmak; precomp desteği.
2. Path/vector katmanı: Bézier eğrileri, noktalı çizgi, stroke/dash, trim-path, noktalar ve bağlantılar.
3. Anchor/constraint sistemi: çizgi ucu, sayı/metin veya şekli belirli kaynak/kompozisyon noktalarına bağlama. Tracking verisi ayrı üretilebilir; manuel keyframe de kabul edilmeli.
4. Data-driven text: sayısal/kısa yazıları biçim, konum ve animasyonla üretme; sabit seed ile rastgele dekorasyon. Gerçek ölçüm ile dekoratif sayı açıkça ayrılmalı.
5. Maske/track matte, selective effects, edge/outline, threshold, glow ve displacement gibi efekt kombinasyonları.
6. Parametrik çoğaltma/instancing ve beat/curve ile sürülen animation modifier'ları; her objeye ayrı shader veya ayrı API çağrısı zorunlu olmamalı.
7. 2.5D kamera, perspective/corner-pin ve katman derinliği; tam 3D motor gerekliliği bu karelerden çıkarılmamalı.
8. Küçük metin ve çizgiler için rasterization/antialiasing kalitesi; önizleme ve export farklarının ölçülmesi.
9. Scene-level kontrol ve yeniden kullanılabilir görsel rig: agent tek tek yüzlerce objeyi düzenlemek yerine seçili rig parametrelerini değiştirebilmeli; içerik yine inspect edilebilir/editable kalmalı.
10. Performans değerlendirmesinde obje adedi yanında draw/pass sayısı, overdraw, VRAM, decode ve temporal örnekleme maliyetleri ölçülmeli.

## 4. _gutsz: kolaj, katmanlı sahne ve deformasyon referansı

Kaynak: https://www.tiktok.com/@_gutsz/video/7658742974045572384

İndirilen kopya ffprobe ile 15.633333 saniye, 1902x1080 ve 60 fps olarak raporlandı. Bu orijinal proje ayarlarının kanıtı değildir. İki sayfada toplam 32 eşit aralıklı kare görsel olarak incelendi; kesintisiz video/ses değerlendirmesi yapılmadı.

Gözlemler:
- Başlangıçta üst üste binen karakter görselleri/paneller ve metin kartı; kolaj düzeni.
- Kırmızı çiçek/kelebek benzeri motifler, manga dokulu karakter görüntüleri, bulut ve grafik çizgi katmanları.
- Bazı örneklerde dalgalı/kıvrımlı görünüm ve karakterin çevresinde kırmızı uzantılar.
- Orta bölümde derine uzanan dama zemin ve üzerinde karakter; perspektifli sahne hissi.
- İleride daha sade mor/mavi, koyu karakter yakın planları ve dikey küçük yazılar.

Kaynak projeler olmadığından motiflerin hazır overlay, kaynak çizim, 2D deformasyon veya gerçek 3D ile üretildiği kesin değil. Dama zemin tek başına tam 3D motor gerekliliğini kanıtlamaz. Örnekler arası hızlı dönüşümler kare incelemesiyle ayrıntılı doğrulanmadı.

Tasarım çıkarımları:
1. Precomp, kolaj/panel düzeni, çoklu kaynak ve alpha/track matte gereksinimini güçlendiriyor.
2. Perspektif/corner-pin ve 2.5D kamera yalnız grafik etiketler için değil sahne oluşturma için de yararlı.
3. Displacement'ın yanında mesh/grid warp gibi kontrollü 2D deformasyon yolu değerlendirilmeli. Bu örneğin gerçekten bu teknikle üretildiği iddia edilmiyor.
4. Özel bir nesneyi semantik olarak üretmek shader'ın görevi değil; çiçek/kelebek gibi görseller harici texture/vector/alpha-video varlıkları olarak da desteklenmeli.
5. Tek bir efekt yoğunluğu yerine sahne bazlı farklı stil/kompozisyon; karmaşık kolaj ve sade yakın planlar aynı projede bulunabilmeli.
6. Karmaşık sahne rig'leri, varlık referansları ve kaynak-zaman eşlemesi agent tarafından ayrı ayrı inspect edilebilmeli.

## Sonraki inceleme

Kullanıcı geliştirme izni verdiğinde özellikle üçüncüden kısa bir bölüm seçilip daha sık kare örnekleme ve gerçek sesli hareket incelemesiyle olay dökümü hazırlanabilir. Hedef görsel davranışı tanımlamak; kullanılan ticari plugin'i veya uygulamayı kaynak olmadan tahmin etmek değil. Mevcut not gerçek render denemesi veya motor kabul testi değildir.
