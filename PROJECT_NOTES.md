# FrameShift: agent-first video editörü proje notları

## Kesinleşen proje adı

Kullanıcı ürün/proje adı olarak **FrameShift** adını seçti. Yazımı bu biçimde korunacak. Elite Dangerous içindeki Frame Shift Drive çağrışımı kullanıcı tarafından belirtildi ve ismin beğenilme bağlamının parçası. Marka, alan adı veya paket adı uygunluğu henüz kontrol edilmedi; bu seçim kullanım hakkı doğrulaması değildir. Mevcut proje klasörü /root/projects/agent-first-video-editor olarak kalıyor; yeniden adlandırılmadı.

Aşağıdaki eski bölümlerde geçen geçici isim veya henüz isim seçilmedi ifadeleri tarihsel notlardır; güncel isim kararı FrameShift'tir.

## Durum

Yalnızca fikir ve tasarım aşaması. Yiğit'in açık talebi: proje klasörü oluşturulsun, konuşulanlar kaydedilsin; şu an herhangi bir kod yazılmasın. Bu belge uygulama veya çalışan prototip değildir. Proje adı geçicidir.

## Kullanıcının tanımladığı hedef

- Agent-first bir video edit uygulaması geliştirmek.
- İlk odak anime editleri, AMV ve TikTok tarzı kısa editler.
- Kalite hedefi Higgsfield Katana tanıtımındaki ve iyi TikTok editlerindeki görsel düzey; bu kalite henüz elde edilmiş veya ölçülmüş değildir.
- Klipler ve ses dışarıdan sağlanacak. İlk amaç görüntü üretmek değil, mevcut materyali kurgulamak.
- Normal bir editörde insanın UI üzerinden yaptığı edit işlemlerinin agent tarafından yazıyla, kodla veya yapılandırılmış proje tanımıyla oluşturulabilmesi.
- Agent UI'da fareyle işlem yapmaya mecbur kalmamalı.
- Geçiş, efekt, hız eğrisi, maske, animasyon ve anime editte gereken diğer işlemler programlanabilir olmalı.
- Yalnızca sabit preset ve komut listesinden ibaret bir sistem olmamalı; özel davranışları ifade edebilecek esneklik hedefleniyor.
- Edit motorunun kalitesi temel öncelik.

## Ana ürün yaklaşımı

UI'ına sohbet kutusu eklenmiş klasik editör yerine programlanabilir edit motoru ve bu motoru kullanabilen agent.

Önerilen akış:

Kullanıcı isteği + klipler + ses + isteğe bağlı referans edit -> agent planlaması -> düzenlenebilir edit programı/proje tanımı -> kompozisyon/render -> önizleme ve görsel kontrol -> revizyon -> final çıktı.

UI, agent ve diğer istemciler aynı proje durumunu okumalı ve değiştirmeli. UI'a özel gizli edit durumu bulunmaması hedefleniyor. İnsan gerektiğinde timeline üzerinden müdahale edebilmeli.

## Konuşulan mimari öneriler

Aşağıdaki maddeler tasarım önerisidir; teknoloji veya uygulama kararı olarak kesinleştirilmemiştir.

### Proje modeli ve kontrol

- Sürümlenmiş, okunabilir, düzenlenebilir proje/timeline formatı; JSON olası seçenek, kesin şema seçilmedi.
- Klip referansları, kaynak zamanları, katmanlar, efektler, hız eğrileri ve kompozisyon bağlantıları açıkça tanımlanmalı.
- Anlamlı operasyonlar: klip bölme, hız eğrisi ayarlama, kesmeyi beat'e hizalama, efekt ekleme ve taslak render gibi işlemler.
- Değişikliklerde doğrulama, undo ve proje sürümü takibi.
- Yerel API ve CLI üzerinde ince bir MCP bağlantısı önerildi. MCP ürünün çekirdeği değil, bağlantı katmanı.
- Özel expression ve shader desteği değerlendirilebilir. Agent'ın yazdığı davranışlar için güvenli yürütme modeli henüz tasarlanmadı.

### Zamanlama

- Kare hassasiyetinde kesme ve güvenilir kaynak-zaman eşlemesi.
- Gerçek hız eğrileri, yavaşlama/hızlanma, reverse ve freeze.
- Sabit hızlı parçaları birleştirmek ile smooth time-remap birbirinden ayrılmalı.
- Bézier/easing/overshoot ve parametre bağlantıları.
- Beat, ses transient'i ve elle tanımlanmış impact noktalarına hizalama.
- Retime sırasında ses davranışının açık biçimde tanımlanması.
- Kesin zaman tabanı, değişken kare hızı ve ses örnekleme ayrıntıları henüz kararlaştırılmadı.

### Kompozisyon ve efektler

- Transform, kamera hareketleri ve iç içe kompozisyonlar.
- Maskeler, track matte, blend modları ve açık efekt sırası.
- Renk düzenleme, blur, glow, distortion, shake, flash ve temel geçişler.
- Programlanabilir efektler/shader'lar ve müzik/başka katman/matematiksel ifadeye bağlı parametreler.
- Hareketle tutarlı motion blur ve doğru alpha işleme.
- Karakter maskesi oluşturma, tracking, optical flow ve segmentasyon ayrı gerçek yetenekler gerektirir; yalnızca komut tanımlamak bunları sağlamaz.
- Katmanlı efekt bileşimleriyle özel geçişler kurulabilmeli; yalnızca isimli preset çağırmakla sınırlı olmamalı.

### Önizleme ve render

- Önizleme ve final aynı kompozisyon semantiğine dayanmalı.
- Proxy/düşük kalite önizlemesi zamanlamayı veya efektin anlamını değiştirmemeli; farklı kalite seviyelerindeki farklar bilinmeli ve sınanmalı.
- Her render belirli proje sürümüne bağlı olmalı.
- Headless render, taslak render ve deterministik yeniden üretim değerlendirme başlıkları.
- UI/render motoru ve medya altyapısı seçilmedi.

### Agent ve analiz

- Beat, enerji, sahne sınırı, medya bilgisi ve teknik kalite kontrollerinde önce algoritmalar/kod.
- Görsel model; sahne anlamı, karakter, impact, hareket yönü, anlatı ve estetik seçimler için.
- Agent kaynak karelerini, kompozit kareleri ve uygun kısa render'ları inceleyebilmeli.
- Her beat'e efekt eklemek yerine yoğunluk, sakinlik, beklenti, drop ve hareket sürekliliği değerlendirilmeli.
- Kaynak seçimi ve estetik planlama, motorun teknik kapasitesinden ayrı katmanlar.
- Referans editin renk, ritim, hareket ve geçiş özelliklerinden yön alma hedefi değerlendirildi; birebir yeniden üretim garantisi verilmedi.

## Önerilen ilk doğrulama ve kapsam

Önce kullanma izni olan kaynaklarla kısa bir referans editi doğrudan proje tanımı üzerinden kaliteli biçimde üretmek. Agent otomasyonu daha sonra aynı yeteneklere bağlanmalı.

Bu ayrım hatanın kaynağını belirler:
- Doğrudan proje tanımıyla da sonuç alınamıyorsa motor yetersizdir.
- Motor sonucu üretebiliyor ama agent oluşturamıyorsa planlama/kontrol katmanı yetersizdir.

Önerilen ilk uçtan uca kapsam:
1. Klip/ses içe aktarma ve proxy.
2. Beat/sahne analizi ve kaynak kare inceleme.
3. Kesme, sıralama, crop ve katmanlar.
4. Speed curve, zoom, shake, flash ve temel geçişler.
5. Timeline, undo, taslak/final render ve sohbetle revizyon.
6. Programatik erişim; daha sonra MCP katmanı.

Otomatik karakter rotoskopisi, görüntü üretimi ve efekt marketplace'i ilk kapsam dışında önerildi. Bu kapsam henüz kullanıcı tarafından nihai geliştirme planı olarak onaylanmadı.

## Kalite değerlendirmesi

- Özellik sayısı yerine gerçek referans edit koleksiyonu.
- Velocity, maskeli geçiş, typography, hareket eşlemeli kesme ve sakin sinematik edit gibi farklı örnekler.
- Hız eğrisi, katman, maske, motion blur ve ses senkronunun birlikte sınanması.
- Önizleme/export tutarlılığı, kare doğruluğu ve ses senkronu teknik testler.
- Görsel kalite ayrıca insan değerlendirmesi gerektirir; teknik test veya LLM yorumu tek başına Katana/TikTok kalitesi kanıtı değildir.
- Henüz örnek medya, benchmark, kabul eşiği veya çalışan render yok.

## Araştırmada değerlendirilen araçlar

Bunlar önceki web araştırmasının notlarıdır; bağımsız kurulum/render benchmark'ı yapılmadı. Değişebilen ürün ayrıntıları uygulama öncesi yeniden doğrulanmalı.

### FableCut

Agent'a uygun MCP/REST, düzenlenebilir JSON timeline ve tarayıcı editörü nedeniyle başlangıç tabanı olarak değerlendirildi. Belgelerde beat işlemleri, keyframe'li speed ramp, maske, tracking ve headless export bulunuyor. Beats kodunda ramp uygulanmış klipleri yeniden beat-cut yapma kısıtı görüldü; önerilen sıra önce beat-cut, sonra ramp.

FableCut'ın kesin olarak en iyi editör olduğu gösterilmedi. Fork/yeniden kullanım önerisi var; henüz karar yok. Kod ve gerçek render doğruluğu incelenmeden temel seçilmeyecek.

### After Effects + Engine Room MCP

Profesyonel efekt ve ince kontrol için güçlü aday. Katmanlar, keyframe, easing, efekt ve expression kontrolü belgeli. MCP kapsamı AE'nin toplam yeteneğiyle aynı değil. Hazır otomatik AMV iş akışı ayrıca gerekir.

### Higgsfield Katana

Kullanıcının beğendiği ana referans. Referans edit/preset ve karakter/ürün görsellerinden yeni bir edit kuran akış; gerektiğinde yeni sahne üretimi içeriyor. Bu yönüyle yalnızca mevcut kareleri koruyan klasik edit motoruyla aynı kategori değil.

Higgsfield genel bağlantı rehberi Hermes/CLI desteği veriyor; Katana'nın Claude deneyiminin Hermes'te birebir çalışması doğrulanmadı. Ücretli plan ve kredi koşulları var; agent üzerinden üretim web unlimited kapsamına girmeyebilir. Bu projede Higgsfield bağlantısı kurulmadı ve kredi harcanmadı.

### Diğerleri

Resolve MCP tek uygulamada kurgu/renk/render için değerlendirildi; sürüm/API ve retime kısıtları var. Premiere MCP native speed kontrolü nedeniyle temkinli değerlendirildi. Remotion programlanabilir kompozisyon seçeneği; eski hosted documentation MCP'sinin kapanmış olması Remotion'ın kendisinin kapanması anlamına gelmiyor. CapCut draft araçları uygulamayı canlı kontrol etmekle karıştırılmamalı.

## Kaynaklar

- Kullanıcının paylaştığı Katana videosu: https://x.com/higgsfield/status/2108294775644585998/video/1?a=Vl0idbhMkSBfcEOMTWY&s=19
- FableCut: https://github.com/ronak-create/FableCut
- FableCut beat işlemleri: https://raw.githubusercontent.com/ronak-create/FableCut/main/beats.js
- Engine Room AE MCP: https://github.com/Engine-Room-Games/after-effects-mcp
- Katana kullanım rehberi: https://higgsfield.ai/creator-hub/help-center/integrations/how-do-i-use-katana
- Katana açıklaması: https://higgsfield.ai/blog/higgsfield-katana
- Higgsfield agent bağlantısı: https://higgsfield.ai/creator-hub/help-center/integrations/how-do-i-connect-higgsfield-to-ai-agent
- Resolve MCP: https://github.com/samuelgursky/davinci-resolve-mcp
- Resolve native retime sınırları: https://raw.githubusercontent.com/samuelgursky/davinci-resolve-mcp/main/docs/guides/native-drt-authoring.md
- Premiere işlem kataloğu: https://raw.githubusercontent.com/leancoderkavy/premiere-pro-mcp/main/docs/supported-actions.md
- Remotion eski documentation MCP durumu: https://www.remotion.dev/docs/ai/mcp

## Ek tasarım görüşmeleri: skill, OpenMotion ve HyperFrames

Bu bölüm kullanıcı talepleri ile asistanın araştırma/önerilerini ayırır. Henüz altyapı seçimi yapılmadı; kod veya render deneyi yok.

### Skill ve bilgi mimarisi önerisi

- Her atomik işlem için ayrı skill yerine motorun sorgulanabilir, sürümle eşleşen API şeması kullanılmalı. Parametre, zaman birimi, koordinat sistemi, yan etki ve doğrulama yöntemleri bu sözleşmede olmalı.
- Skill'ler teknik ailelerini ve karar süreçlerini öğretmeli: ritim/kurgu, velocity/time-remap, hareket/geçişler, maske/kompozisyon, typography ve kalite kontrol.
- Teknik tarif ile yaratıcı stil ayrılmalı: bir geçişin nasıl oluşturulduğu, ne zaman kullanılmasının doğru olduğu anlamına gelmez.
- Teknik paket; amaç, girdiler, yöntem, ayarlanabilir noktalar, başarısızlık örnekleri, gerçek kaynak klip, düzenlenebilir örnek proje ve doğrulanmış çıktı içermeli.
- Rehberler ihtiyaç oldukça yüklenmeli; henüz çalışmayan motor için yüzlerce spekülatif skill yazılmamalı.
- Önerilen agent döngüsü: isteği anla, materyali incele, ilgili rehberi yükle, küçük bölüm oluştur, render et, kontrol et, revize et.

### OpenMotion incelemesi

Kaynak: https://github.com/nexu-io/openmotion

- Kodla video yaklaşımı: HTML/CSS/JavaScript kompozisyonları, harici medya, düzenlenebilir Studio ve MP4 export.
- Motion.md yaklaşımı yaratıcı kararları; SKILL.md prosedürü; kaynak proje uygulamayı; preset sözleşmesi değişebilir girdileri; eşlenmiş render ve sürüm/hash kayıtları kanıtı taşır.
- Motion.md tek başına otomatik animasyon parser'ı veya motorlar arası çalıştırma garantisi değildir.
- MCP'de revision/version korumalı proje yazımı, geçmiş ve snapshot üzerinden export belgelenmiş.
- OpenMotion kendi başına tamamen bağımsız renderer değil; incelenen package.json HyperFrames paketlerini 0.9.7'ye sabitliyor.
- Repo Apache-2.0; bağımlılık, medya ve asset lisansları ayrıca incelenmeli.
- Örnekler ağırlıkla marka/ürün/konuşmacı/motion graphics; anime edit kalitesi bağımsız doğrulanmadı.
- Fork veya sıfırdan geliştirme arasında karar verilmedi. Öneri: önce sınırlı bir teknik değerlendirme; eklenebilir anime yetenekleri ile motorun temel sınırlamalarını ayırmak. Henüz fork/clone yapılmadı.

### HyperFrames incelemesi

Kaynak: https://github.com/heygen-com/hyperframes

- HTML/CSS/medya ve seek edilebilir animasyonları videoya dönüştüren, agent skill ve CLI sunan motor. Apache-2.0.
- İnceleme anında main/core ve son GitHub release 0.9.64; OpenMotion'ın 0.9.7 bağımlılığıyla karıştırılmamalı. Bu sürüm değerleri değişebilir.
- Gerçek speed ramp: data-automation içindeki rate kanalı; sourceTimeAt hız eğrisinin integralini sayısal tablodan hesaplar. Önizleme/render ortak kaynak-zaman eşlemesi belgelenmiş.
- Rate kanalı 0.1x-10x aralığına sıkıştırılıyor; bu mekanizma reverse veya tam freeze sağlamıyor. Ses export'u kısa sabit hız dilimleriyle eğriyi yaklaşık izliyor.
- Hız değiştirme yeni ara kare üretmek değildir; anime optical-flow/slow-motion kalitesi doğrulanmadı.
- Sayfa motion blur'unda varsayılan SVG filter ve isteğe bağlı capture yöntemi var. Filter yönteminde zoom/dönüş izi yaklaşık; mask/clip-path kenarları sertleşebilir. Capture yönteminde örtüşme sırası, blend ve clipping kısıtları var.
- Ayrıca renderer sub-frame shutter örnekleri alıp ortalayan blur sunuyor. Shader geçişlerinin kullandığı layered compositor yolu bunu desteklemiyor; kaynak kodunda açık hata ile reddediliyor. Page blur da layered yol için devre dışı bırakılabiliyor.
- Blur yöntemlerinin maliyet, browser desteği ve preview/export farklılıkları var; ortak yapı birebir görüntü garantisi değildir.
- Kaynak ve test dosyaları incelendi; testler yerelde çalıştırılmadı ve render benchmark'ı yapılmadı.
- Güncellenmiş asistan önerisi: sıfırdan render çekirdeği yerine önce güncel HyperFrames'i bağımsız test etmek; uygunsa kendi sade uygulamamızı üstüne kurmak da OpenMotion fork'una alternatif. Kullanıcı henüz bunu nihai seçim olarak onaylamadı.

Önerilen ortak değerlendirme sahnesi: orijinal anime klibinde impact'e hizalı smooth velocity, zoom/dönüş blur'u, maskeli karakter arkasında yazı, shader geçişi, ses senkronu ve preview/final karşılaştırması. Özelliklerin ayrı varlığı değil, birlikte çalışması ve gerekli yama miktarı ölçülmeli.

Ek kaynaklar:
- https://github.com/nexu-io/openmotion/blob/main/docs/motion-md.md
- https://github.com/nexu-io/openmotion/blob/main/docs/mcp-phase1-tools.md
- https://github.com/nexu-io/openmotion/blob/main/docs/hyperframes.md
- https://github.com/nexu-io/openmotion/blob/main/package.json
- https://github.com/heygen-com/hyperframes/blob/main/docs/reference/speed-ramps.mdx
- https://github.com/heygen-com/hyperframes/blob/main/packages/core/src/speedRamp.ts
- https://github.com/heygen-com/hyperframes/blob/main/docs/guides/motion-blur.mdx
- https://github.com/heygen-com/hyperframes/blob/main/packages/engine/src/services/motionBlur.ts
- https://github.com/heygen-com/hyperframes/blob/main/packages/producer/src/services/render/motionBlurRoute.ts

## Güncel yön ve kullanıcı tarafından onaylanan başlangıç stack'i

Bu bölüm önceki alternatif değerlendirmelerinin ardından gelen güncel yönü kaydeder. Önceki bölümlerdeki kararsızlık ifadeleri araştırma geçmişidir; başlangıç stack'i artık kullanıcı tarafından onaylanmıştır. Onay uygulama geliştirmesine başlama talimatı değildir.

### Kullanıcının netleştirdiği kapsam

- Ürün masaüstü uygulaması olarak hedefleniyor; tarayıcı içinde çalışan ürün zorunluluğu yok.
- İlk odak yalnızca edit/render motoru. UI, sohbet ekranı ve MCP bağlantısı ilk motor çalışmasının kapsamı dışında.
- Agent editi programatik proje tanımı üzerinden oluşturmalı; insanın UI işlemlerini taklit etmesi gerekmemeli.
- Öncelik efektler, geçişler, görsel zenginlik ve esneklik; bunların gerçek bir kompozisyon altyapısında birlikte çalışması.
- Native ve GPU birbirine alternatif değildir: native motor da GPU/shader tabanlı olabilir. Masaüstü paketleme tek başına görüntü kalitesini artırmaz.

### Onaylanan başlangıç bileşimi

1. Rust: proje modeli, zamanlama, render scheduler, cache ve kaynak yönetimi.
2. wgpu + WGSL: native GPU compositor, grafik/compute işlemleri ve programlanabilir efektler. wgpu browser gerektirmez; platformda uygun Vulkan/Metal/DirectX backend'leri kullanılabilir.
3. FFmpeg kütüphaneleri: medya dosyası okuma, video/ses decode, ses işleme, encode ve mux. Ana yaratıcı compositor dev bir FFmpeg filtre zinciri olarak tasarlanmayacak.
4. OpenColorIO: giriş, çalışma ve çıktı renk alanları ile görüntüleme dönüşümleri.
5. Skia veya uygun alternatif text/vector altyapısı: typography, şekil ve path rasterization. Bu parçada Skia kesin entegrasyon kararı değil, başlangıç adayıdır.
6. Bize ait sürümlenmiş edit/proje formatı: node bağlantıları, parametreler, animasyon eğrileri ve kaynak-zaman haritaları.

### Motor tasarımı için konuşulan ilkeler

- İlk tasarım konusu zaman modeli + render grafiği + efekt sözleşmesi. Bunların kesin şeması henüz yazılmadı.
- Belirli proje/sürüm, kaynak medya, zaman ve kalite ayarları için görüntü/ses üretilebilmeli; hesap önceki karelerin sırayla oynatılmış olmasına bağımlı olmamalı.
- Efekt grafiği dallanabilmeli; maskeler ve ara çıktılar paylaşılabilmeli; geçişler birden çok girişli kompozisyonlar olarak ifade edilebilmeli.
- 16-bit float ara renk tamponları, gerekli hesaplarda daha yüksek hassasiyet, açık renk alanı ve premultiplied-alpha kuralları hedefleniyor. Her artistik işlemin zorunlu linear alanda yapılması öngörülmüyor; işlemin çalışma alanı tanımlanmalı.
- Final kalite temporal motion blur shutter aralığında farklı zamanlarda kompozisyon değerlendirmesiyle tasarlanmalı. Maske/transform/geçiş de zaman örneklerine katılmalı.
- Katman hareketi blur'u ile kaynak anime içindeki hareket/optical flow ayrı problemler. Yeni ara kare kalitesi otomatik varsayılmamalı.
- Önizleme ve final aynı kompozisyon/zaman semantiğine dayanmalı; düşük kalite seçeneklerinin farkları açık ve test edilebilir olmalı.
- Doğrudan Vulkan ile başlamak yerine wgpu önerildi. Video/GPU bellek paylaşımı veya donanım özelliği gerektirirse platforma özel yol ayrıca değerlendirilecek.
- Hardware decode -> GPU texture -> encode hattında zero-copy sağlandığı henüz doğrulanmadı. Skia ile wgpu entegrasyonu da otomatik varsayılmamalı.
- FFmpeg build seçenekleri, bağımlılık lisansları ve codec/patent koşulları dağıtım öncesi incelenmeli.

### İlk kabul hedefi

Dışarıdan gelen klip ve müzikle, hız eğrisi + hareketli maske + shader geçişi + temporal motion blur içeren kısa bir bölümü yalnızca proje tanımı üzerinden render etmek. Rastgele seek, ses senkronu, tekrar üretilebilirlik ve preview/final tutarlılığı ölçülmeli. Bu henüz gerçekleştirilmedi.

### Araştırma ve geliştirme sınırı

HyperFrames ve Remotion karşılaştırması ile diğer motor/fork adayları ayrı ENGINE_ALTERNATIVES_RESEARCH.md dosyasında. Bu araştırma geçmişi yeni native/GPU başlangıç yönüyle karıştırılmamalı. Çalışan bir motor, prototip veya kalite benchmark'ı yok; kod yazılmadı, bağımlılık kurulmadı, repo klonlanmadı.

Ek kaynaklar:
- https://wgpu.rs/
- https://opencolorio.org/
- https://ffmpeg.org/legal.html
- https://skia.org/docs/

## Headless yönü ve çoklu inceleme sistemi

### Kullanıcının değerlendirmeye açtığı ürün yönü

Kullanıcı, asıl operatör agent olduğundan UI gerekmeyebileceğini ve headless bir uygulamayı değerlendirmek istediğini belirtti. Headless-first yaklaşım önerildi; tam timeline UI'ı yerine programlanabilir motor, CLI/API ve görsel inceleme çıktıları öne çıktı. Bu yön native/GPU stack'iyle uyumlu; headless pencere açılmasını gerektirmeyen çalışma biçimidir. Agent-first ve programlanabilirlik ayrı niteliklerdir. Tam UI'ın kalıcı olarak iptal edildiği veya CLI/API sözleşmesinin kesinleştirildiği varsayılmamalı.

### Kullanıcının çoklu geri bildirim talebi

Agent seçili bir bölümü görsel, video ve kodsal olarak ayrı ayrı veya birlikte inceleyebilmeli. Video anlayamayan fakat görsel görebilen modeller için tek görselde birden fazla zaman etiketli kare (contact sheet) sunulması istendi. Kullanıcı bu konudaki görüşmenin notlara eklenmesini onayladı; ayrıntılar henüz uygulanmış özellik değil, tasarım önerileridir.

### İnceleme yüzeyleri

1. Görsel: tek kare, tam çözünürlüklü kare, crop, maske ve ara efekt/node çıktısı.
2. Video: seçili olayın öncesi/sonrasını içeren kısa sesli taslak.
3. Kodsal: ilgili klipler, node bağlantıları, parametreler, expression/shader kaynakları ve animasyon eğrileri.
4. Teknik: kaynak-zaman eşlemesi, beat/impact noktaları, kalite ayarları ve uyarılar.

Bütün yüzeyler aynı proje revision/version kimliği, medya/efekt sürümleri ve zaman referanslarıyla ilişkilendirilmeli. Eski render ile yeni kodun yanlışlıkla eşleştirilmesi önlenmeli. Karelerin kimlikleri, composition zamanı ve gerektiğinde kaynak zamanı ilgili klip/node tanımlarına bağlanmalı.

### Contact sheet stratejisi

- Genel bakış için zaman aralığı boyunca seyrek örnekleme; sahne sırası, renk ve kompozisyon inceleme.
- Olay odaklı inceleme için kesme, impact ve geçiş çevresinde daha sık örnekleme.
- Her karenin altında okunabilir kimlik ve zaman etiketi; tam kareyi veya crop'u yeniden isteme olanağı.
- Aşırı büyük tek kolaj yerine küçük, okunabilir sayfalar ve genel bakıştan ayrıntıya ilerleme. Modelin görüntü küçültmesi maske/yüz ayrıntısını yok edebilir.
- Sabit bir örnek adedi veya aralığı kararlaştırılmadı; içerik ve şüpheli olay belirleyici olmalı.

### Hata türüne göre görsel paketler

- Maske: kaynak kare + maske + dama zemin üzerinde ayrılmış karakter + final kompozit.
- Geçiş: önceki klip + sonraki klip + birleşmiş çıktı, eşleşen zamanlarda.
- Efekt: önce/sonra ve ilgili node'un ara çıktısı.
- Velocity: çıktı kareleri + kaynak zamanları + hız eğrisi.
- Revizyon: eski/yeni sürümler aynı zamanlarda; görsel karşılaştırma teknik fark ölçümlerinden ayrı.

### İnceleme paketi ve aktarım

Seçili bölüm için kısa sesli video, contact sheet, istenen ayrıntılı kare/crop'lar, ilgili proje/efekt tanımları ve teknik rapor birlikte referanslanabilmeli. Birlikte incelemek hepsini tek resme gömmek değildir: kod metin, görüntü görsel olarak kalmalı ve ortak kimliklerle eşleşmeli. Yalnız yerel dosya yolu döndürmek modele görsel erişim sağlamaz; agent adaptörü desteklediği medya türünü modele gerçekten ulaştırmalı.

### Yeteneğe göre doğrulama sınırları

- Contact sheet videonun tam karşılığı değildir; örnekler arasındaki titremeyi kaçırabilir, akıcılık ve ses senkronunu tek başına kanıtlamaz.
- Görsel model kompozisyon, maske ve belirgin süreksizlikleri değerlendirebilir.
- Algoritmalar/kod ses/beat zamanlaması, kare atlama ve teknik belirtileri kontrol eder; bunlar estetik ritim kanıtı sayılmaz.
- Video/ses inceleyebilen model veya insan hareket ve ritmi ayrıca değerlendirir.
- Sadece metin gören model için kolaj yeterli değildir; yapılandırılmış durum/ölçümler sunulabilir ama görsel kalite doğrudan doğrulanmış sayılmaz.
- Rapor neyin gerçekten incelendiğini, neyin doğrulanamadığını ve kullanılan kalite/örnekleme kapsamını açık tutmalı.

### Önerilen inceleme döngüsü

Genel bölüm özeti ve contact sheet -> şüpheli aralığı daha sık örnekleme -> tam kare/crop/maske ve ilgili kodu inceleme -> hedefli değişiklik -> etkilenen bölümü yeniden render -> aynı zamanlarda karşılaştırma. Amaç bütün projeyi bağlama yığmak değil, gözlenen sorunu doğru zaman, kaynak medya ve kodla ilişkilendirmek.

Bu bölüm kapsamında kod, CLI, MCP aracı veya render örneği oluşturulmadı.

## Güncel ürün kararı: tek masaüstü uygulaması, bağımsız çekirdek

Kullanıcı, agent'ın işlemlerini görebilmek ve klipleri kendisi de düzenleyebilmek için UI eklenmesini istedi; ardından motoru aynı uygulamada çalıştırıp kod/mimari olarak UI'dan bağımsız tutma önerisini olumlu karşılayıp kaydedilmesini talep etti. Bu bölüm önceki yalnız headless ürün değerlendirmesini günceller; headless çalışma yeteneği korunur.

### Karar

- Ürün tek masaüstü uygulaması olarak paketlenebilir; motor UI ile aynı süreçte çalışabilir. Ayrı süreç zorunluluğu yok, kesin süreç mimarisi henüz seçilmedi.
- Motor bağımsız bir çekirdek/kütüphane olarak tasarlanacak. Proje durumu, edit işlemleri, efektler, zamanlama, render ve geçmiş yalnız UI bileşenlerinde tutulmayacak.
- UI, agent ve CLI aynı motor sözleşmesini kullanacak. Kullanıcı ve agent aynı projeyi düzenleyebilecek; UI'daki düzenlemeler agent tarafından okunabilecek.
- Motor UI olmadan da kullanılabilecek; ayrı headless çalıştırıcıyla aynı çekirdek üzerinden proje oluşturma, düzenleme ve render mümkün olmalı.
- UI'ın kapalı olması veya kapatılması sırasında agent işlerinin nasıl devam edeceği, uygulama yaşam döngüsü ve süreç tasarımıyla ayrıca çözülecek; aynı süreçte pencereyi kapatmak motorun otomatik hayatta kalacağı anlamına gelmez.
- Agent işlemleri UI'da izlenebilir ve geçmiş üzerinden geri alınabilir olmalı. Eşzamanlı düzenlemeler sürüm/çakışma kontrolüyle korunmalı; agent kullanıcının yeni düzenlemesini eski kayıtla ezmemeli.

### Önerilen UI kapsamı

Medya kütüphanesi, çok katmanlı timeline, önizleme, seçili öğenin özellikleri ve işlem geçmişi. Ayrıntılı node/curve editörlerinin sırası daha sonra belirlenecek. Bunlar henüz uygulanmış özellikler veya kesin UI teknoloji seçimi değildir.

### Geliştirme sırası ve sınırı

Önce motor odağı korunur; UI bağımsız çekirdeğin üzerine gelir. Başlangıç stack'i Rust + wgpu/WGSL + FFmpeg + OpenColorIO ve Skia/uygun text-vector alternatifi olarak korunuyor. Bu kayıt kod yazma, dependency kurma veya prototip başlatma yetkisi değildir; yalnız tasarım notları güncellenmiştir.

## Anlamsal edit işlemleri: olay ve kısıt tabanlı zamanlama

Kullanıcı bu yaklaşımın proje tasarımına eklenmesini onayladı. Bu bir tasarım hedefidir; çalışan bir işlem/API veya genel amaçlı çözücü henüz yoktur.

### Sorumluluk ayrımı

- Serbest metindeki niyeti agent yorumlar. Motor doğal dil veya estetik yargı üretmek zorunda değildir.
- Agent niyeti açık kaynak/hedef işaretleri, parametreler ve ölçülebilir kısıtlara dönüştürür.
- Motor kaynak-zaman haritasını, bağımlılıkları ve teknik geçerliliği deterministik olarak hesaplar; sonuç görsel/işitsel incelemeyle ayrıca değerlendirilir.

### İlk odak: kaynak impact işaretini hedef zamana hizalama

1. Klipte önemli kaynak anı işaretlenir; örneğin yumruğun temas ettiği kare. İnsan, görsel model veya analiz aracı bu işareti sağlayabilir. İşaretin anlamını otomatik doğru bulma garantisi yoktur.
2. Müzikte hedef beat/drop/impact zamanı seçilir; otomatik tespit ile kullanıcı işareti ayırt edilir.
3. Agent kullanılabilir kaynak aralığını, çıktı aralığını, hız sınırlarını ve önerilen hız eğrisini belirtir.
4. Motor hedef çıktı anındaki kaynak-zaman eşlemesinin kaynak impact işaretine karşılık gelmesini sağlar. Hızlanma/yavaşlama bu ankrajı koruyacak biçimde kurulmalıdır.
5. Zaman tabanı, kaynak kare/PTS seçimi, kesirli zaman ve tolerans sözleşmeleri henüz belirlenmedi; birebir kare doğruluğu uygulanmadan garanti edilmiş sayılmaz.

### Olaylara bağlı animasyonlar

Glow, flash, shake veya yazı yalnız sabit bir saniyeye değil, bir olay kimliğine göre zaman ofsetiyle bağlanabilir. Olay çıktı zamanında taşınırsa bağlı animasyonların zamanlaması yeniden hesaplanır; ilgisiz keyframe'ler kendiliğinden taşınmaz. Kaynak görüntüyü izleyen maske/tracking verisi kaynak zamanı üzerinden değerlendirilir; çıktı olayına bağlı animasyonla karıştırılmaz. Bağımlılıklar ve kısıtlar sorgulanabilir olmalıdır.

### Geçersiz veya çelişkili istekler

Yetersiz kaynak aralığı, izin verilen hız sınırları veya çelişkili ankrajlar nedeniyle çözüm bulunamayabilir. Motor ihlal edilen koşulları ve değiştirilebilecek seçenekleri raporlamalı; kaynak aralığı, hedef zaman veya hız sınırını agent/kullanıcı izni olmadan sessizce değiştirmemeli. Değişiklik sürüm/undo ve etkilenen aralık raporuyla ilişkilendirilmeli.

### Öznel isteklerin sınırı

Yüzün okunabilir kalması gibi hedefler; yüzün kadrajda bulunması, boyutu, korunacak süre ve blur sınırı gibi ölçülebilir koşullara agent tarafından ayrıştırılabilir. Bu kontroller estetik okunabilirliğin tam kanıtı değildir. Kaynak marker tespiti, motorun kısıtları uygulaması ve son render'ın değerlendirilmesi ayrı doğrulama katmanlarıdır.

### Aşamalı kapsam

Başlangıçta genel amaçlı evrensel bir kısıt çözücü yerine kaynak işaretini hedef zamana hizalama ve açıkça bağlı efektleri taşıma işlemi hedeflenir. İşaret kimlikleri, zaman aralıkları, hız eğrileri ve doğrulama raporlarının kesin şemaları motor tasarımında belirlenecektir. Kod veya dependency eklenmedi.

## Açık kararlar

- Nihai ürün adı ve hedef kullanıcı kapsamı.
- Seçilen native/GPU yaklaşımında mevcut bileşenlerin yeniden kullanım sınırı; herhangi bir fork kararı yok.
- Proje şeması, kesin zaman tabanı, render grafiği ve efekt sözleşmesi.
- Skia veya alternatif text/vector entegrasyonu; UI teknolojisi şimdilik kapsam dışında.
- Hedef işletim sistemleri, GPU tabanı, codec ve paketleme hedefleri.
- Güvenli expression/shader/plugin yürütmesi.
- Referans medya ve somut kalite/kabul kriterleri.
- Yerel model, bulut model ve maliyet politikası.

## Sonraki işlem sınırı

Bu kayıt için yalnızca klasör ve dokümantasyon oluşturulacak. Kod, dependency kurulumu, repo klonlama, prototip, servis başlatma veya ücretli üretim yapılmayacak. Geliştirmeye başlamak için yeni kullanıcı talimatı beklenecek.
