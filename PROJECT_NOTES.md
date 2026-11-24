# FrameShift — Tasarım ve Mimari Notları

## 1. Durum ve Kapsam Sınırı

> [!NOTE]
> **Güncel Durum Notu (2026-11-24):** Bu belge projenin **Faz 0 öncesi başlangıç tasarım ve vizyon notlarını** tarihsel kayıt olarak korur. Aşağıdaki "henüz kod yazılmadı" ifadesi başlangıç tarihindeki durumu yansıtır. 2026-11-24 itibarıyla 9 crate'lik Rust + `wgpu`/WGSL motoru (**60 WGSL shader**, **40 `NodeKind`**, **50 CLI alt komutu**, **32 MCP aracı**, saf Rust `frameshift-vector` + `.cube`/OCIO boru hattı, `ffmpeg`/Python/NCNN köprüleri) implemente edilmiş olup canlı etkileşimli masaüstü GUI penceresi (P4) sonraki faza bırakılmıştır. Güncel teknik gerçeklik için [README.md](README.md) ve [brain/state.md](brain/state.md) belgelerine bakınız.

- **Başlangıç Aşaması (Tarihsel Not):** Yalnızca fikir, tasarım ve araştırma aşaması. Şu an çalışan bir motor, masaüstü uygulaması, prototip, CLI/API, demo render veya performans benchmark'ı yoktur; hiçbir kod yazılmamış, bağımlılık (dependency) kurulmamış ve harici repo klonlanmamıştır. Geliştirmeye başlamak için yeni kullanıcı talimatı beklenmektedir.
- **Kesinleşen proje adı:** Kullanıcı ürün/proje adı olarak **FrameShift** adını seçti (yazımı bu biçimde korunacaktır). *Elite Dangerous* içindeki *Frame Shift Drive* çağrışımı ismin beğenilme bağlamının parçasıdır. Marka, alan adı veya paket adı uygunluğu henüz kontrol edilmemiştir; bu seçim kullanım hakkı doğrulaması değildir. Mevcut proje klasörü geçmiş notlarda `/root/projects/agent-first-video-editor` olarak bırakılmış olup yeniden adlandırılmamıştır.
- **Epistemik sınır:** Bu belgede yer alan mimari bileşenler, inceleme mekanizmaları ve yaratıcı döngüler mevcut uygulanmış özellikler değil, tasarım hedefleridir. Önceki oturumlardan kalan "geçici isim", "yalnız headless" veya "altyapı seçilmedi" gibi eski ara değerlendirmeler güncel kararlarla karışmaması için [Bölüm 8 (Araştırma ve Karar Geçmişi)](#8-araştırma-ve-karar-geçmişi-arşiv) altına alınmıştır.
- **İlişkili belgeler:** Proje özeti için [README.md](README.md), motor adayları karşılaştırması için [ENGINE_ALTERNATIVES_RESEARCH.md](ENGINE_ALTERNATIVES_RESEARCH.md), referans video gözlemleri için [VISUAL_REFERENCES.md](VISUAL_REFERENCES.md) dosyalarına bakılabilir.

---

## 2. Ürün Vizyonu ve Temel Kararlar

### 2.1. Kullanıcının Tanımladığı Hedef ve Ana Yaklaşım

- **Hedef ürün:** Agent-first bir video edit uygulaması ve bağımsız kompozisyon motoru geliştirmek.
- **İlk görsel odak:** Anime editleri, AMV ve TikTok tarzı kısa editler. Kalite hedefi Higgsfield Katana tanıtımındaki ve iyi TikTok editlerindeki görsel düzeydir (bu kalite henüz elde edilmiş veya ölçülmüş değildir).
- **Materyal kapsamı:** Klipler ve ses dışarıdan sağlanacaktır; ilk amaç görüntü üretmek değil, mevcut materyali kurgulamak ve işlemektedir.
- **Temel paradigma:** Arayüzüne sohbet kutusu eklenmiş klasik bir editör yerine, **programlanabilir bir edit motoru ve bu motoru kullanabilen bir agent**. Normal bir editörde insanın UI üzerinden yaptığı işlemlerin agent tarafından yazıyla, kodla veya yapılandırılmış proje tanımıyla oluşturulabilmesi; agent'ın fareyle UI işlemi taklit etmek zorunda kalmaması hedeflenir.
- **Programlanabilir esneklik:** Geçiş, efekt, hız eğrisi, maske, animasyon ve anime editte gereken diğer işlemler programlanabilir olmalıdır. Yalnızca sabit preset ve komut listesinden ibaret bir sistem yerine özel davranışları (katmanlı efekt bileşimleri, özel geçişler) ifade edebilecek esneklik hedeflenir. Edit motorunun kalitesi temel önceliktir.

**Önerilen çalışma akışı:**

```text
Kullanıcı isteği + klipler + ses + isteğe bağlı referans edit
  -> agent planlaması
  -> düzenlenebilir edit programı / proje tanımı
  -> kompozisyon / render
  -> önizleme ve görsel kontrol
  -> revizyon
  -> final çıktı
```

### 2.2. Tek Masaüstü Uygulaması, Bağımsız Çekirdek ve Headless Desteği

Kullanıcı, agent'ın işlemlerini görebilmek ve klipleri kendisi de düzenleyebilmek için UI eklenmesini istemiş; motoru aynı uygulamada çalıştırıp kod/mimari olarak UI'dan bağımsız tutma yaklaşımını onaylamıştır. Bu karar önceki "yalnız headless" değerlendirmesini güncellerken headless çalışma yeteneğini korur:

- **Paketleme ve süreç sınırları:** Ürün masaüstü uygulaması olarak paketlenebilir; tarayıcı içinde çalışan ürün zorunluluğu yoktur. Motor UI ile aynı süreçte çalışabilir (ayrı süreç zorunluluğu yoktur; kesin süreç mimarisi henüz seçilmemiştir).
- **Bağımsız çekirdek:** Motor bağımsız bir çekirdek/kütüphane olarak tasarlanacaktır. Proje durumu, edit işlemleri, efektler, zamanlama, render ve geçmiş yalnız UI bileşenlerinde (UI'a özel gizli edit durumunda) tutulmayacaktır.
- **Ortak sözleşme ve çoklu istemci:** UI, agent, CLI ve diğer istemciler aynı proje durumunu ve motor sözleşmesini okuyup değiştirecektir. İnsan gerektiğinde timeline üzerinden müdahale edebilecek; UI'daki düzenlemeler agent tarafından okunabilecektir.
- **Headless yeteneği:** Motor UI olmadan da kullanılabilecek; ayrı headless çalıştırıcı üzerinden aynı çekirdekle proje oluşturma, düzenleme ve render mümkün olacaktır.
- **Programatik arayüz katmanı:** Yerel API ve CLI üzerinde ince bir MCP bağlantı katmanı önerilmiştir (MCP ürünün çekirdeği değil, bağlantı katmanıdır).
- **Yaşam döngüsü ve eşzamanlılık:** UI kapalıyken veya pencere kapatıldığında agent işlerinin nasıl devam edeceği uygulama yaşam döngüsü ve süreç tasarımıyla ayrıca çözülecektir (aynı süreçte pencereyi kapatmak motorun otomatik hayatta kalacağı anlamına gelmez). Agent işlemleri UI'da izlenebilir ve geçmiş üzerinden geri alınabilir olmalı; eşzamanlı düzenlemeler sürüm/çakışma kontrolüyle korunmalıdır (agent kullanıcının yeni düzenlemesini eski kayıtla ezmemelidir).

### 2.3. Önerilen UI Kapsamı ve Geliştirme Sırası

- **Önerilen UI kapsamı:** Medya kütüphanesi, çok katmanlı timeline, önizleme, seçili öğenin özellikleri ve işlem geçmişi. Ayrıntılı node/curve editörlerinin sırası daha sonra belirlenecektir. Bunlar henüz uygulanmış özellikler veya kesin UI teknoloji seçimi değildir.
- **Geliştirme sırası ve öncelik:** Önce edit/render motoru odağı korunur; UI, sohbet ekranı ve MCP bağlantısı ilk motor çalışmasının kapsamı dışındadır ve bağımsız çekirdeğin üzerine gelir.

---

## 3. Onaylanan Başlangıç Teknoloji Yönü

Önceki alternatif araştırmalarının ardından aşağıdaki başlangıç bileşimi kullanıcı tarafından onaylanmıştır. Bu onay henüz geliştirme başlatma talimatı değildir; masaüstü paketleme veya native yaklaşım tek başına görüntü kalitesini garanti etmez.

| Katman | Onaylanan Başlangıç Adayı / Karar | Notlar ve Açık Noktalar |
| :--- | :--- | :--- |
| **Çekirdek** | **Rust** | Proje modeli, zamanlama, render scheduler, cache ve kaynak yönetimi. |
| **Native GPU Compositor** | **wgpu + WGSL** | Grafik/compute işlemleri ve programlanabilir efektler. Browser gerektirmez; Vulkan/Metal/DirectX backend'lerini kullanır. Doğrudan Vulkan yerine wgpu ile başlanması önerilmiştir; GPU bellek paylaşımı gerekirse platforma özel yol ayrıca değerlendirilir. |
| **Medya Giriş/Çıkışı** | **FFmpeg kütüphaneleri** | Medya okuma, video/ses decode, ses işleme, encode ve mux. Ana yaratıcı compositor dev bir FFmpeg filtre zinciri olarak tasarlanmayacaktır. Build seçenekleri, bağımlılık lisansları ve codec/patent koşulları dağıtım öncesi incelenmelidir. |
| **Renk Yönetimi** | **OpenColorIO** | Giriş, çalışma ve çıktı renk alanları ile görüntüleme dönüşümleri. |
| **Text / Vector** | **Skia veya uygun alternatif** | Typography, şekil ve path rasterization. Skia kesin entegrasyon kararı değil başlangıç adayıdır; Skia-wgpu entegrasyonu otomatik varsayılmamalıdır. |
| **Proje Formatı** | **Bize ait sürümlenmiş edit formatı** | Okunabilir, düzenlenebilir format (JSON olası seçenek; kesin şema henüz yazılmadı). Node bağlantıları, parametreler, animasyon eğrileri ve kaynak-zaman haritalarını taşır. |

**Teknik doğrulama sınırları:**
- Hardware decode -> GPU texture -> encode hattında zero-copy sağlandığı henüz doğrulanmamıştır.
- Özel expression ve shader desteği için güvenli yürütme modeli henüz tasarlanmamıştır.

**Başlangıç bileşeni kaynakları:**
- wgpu: https://wgpu.rs/
- OpenColorIO: https://opencolorio.org/
- FFmpeg lisans ve hukuk bilgileri: https://ffmpeg.org/legal.html
- Skia dokümantasyonu: https://skia.org/docs/

---

## 4. Çekirdek Motor Tasarım Başlıkları

İlk tasarım odağı **zaman modeli + render grafiği + efekt sözleşmesi** üçlüsüdür (kesin şemalar henüz yazılmamıştır).

### 4.1. Zaman Tabanı Kararı (Koda Geçmeden Önce Kilitlenecek)

Zaman temsili, çekirdekte sonradan değiştirilmesi en maliyetli karar olduğundan ilk kodlamaya geçmeden önce kilitlenecektir:

- **Kayan nokta (`f64` saniye) yerine tam sayı tick / rasyonel zaman:** Timeline konumu, klip sınırları, marker/impact ankrajları ve ses-video hizalaması kayan nokta birikme hatasına açık `f64` saniye yerine tam sayı tick veya rasyonel kesir (`num / den`) olarak temsil edilmelidir.
- **Güçlü başlangıç adayı (Flicks):** `1 saniye = 705,600,000 tick` (Meta Flicks standardı). `24, 25, 30, 48, 50, 60, 120 fps` kare hızlarına, `44.1 kHz / 48 kHz` ses örnekleme oranlarına ve `23.976 / 29.97` NTSC oranlarına (`1000/1001` ölçeğiyle birlikte) tam bölünerek 1 kare kayma ve ses drift sorunlarını önler.
- **Hesaplama sınırı:** Kayan nokta hesaplamaları (hız eğrisi integrali, easing, temporal motion blur alt-kare örnekleri) yalnızca değerlendirme anında ara hesap olarak kullanılır; kalıcı proje modeli ve kaynak-zaman ankrajları kesin zaman tabanına oturur.
- **Açık ayrıntılar:** Değişken kare hızlı (VFR) kaynakların içe aktarma/PTS eşleme kuralı, ses örnekleme ayrıntıları ve NTSC `1000/1001` temsilinin Flicks ile kesin sözleşmesi çekirdek şema tasarımında netleştirilecektir.

### 4.2. Proje Modeli, Zamanlama ve Kompozisyon/Efekt Hedefleri

- **Proje modeli ve kontrol:**
  - Klip referansları, kaynak zamanları, katmanlar, efektler, hız eğrileri ve kompozisyon bağlantıları açıkça tanımlanmalıdır.
  - Anlamlı operasyonlar: klip bölme, hız eğrisi ayarlama, kesmeyi beat'e hizalama, efekt ekleme ve taslak render.
  - Değişikliklerde doğrulama, undo ve proje sürümü takibi sağlanmalıdır.
- **Zamanlama:**
  - Kare hassasiyetinde kesme ve güvenilir kaynak-zaman eşlemesi.
  - Gerçek hız eğrileri, yavaşlama/hızlanma, reverse ve freeze.
  - Sabit hızlı parçaları birleştirmek ile smooth time-remap birbirinden ayrılmalıdır.
  - Bézier/easing/overshoot ve parametre bağlantıları.
  - Beat, ses transient'i ve elle tanımlanmış impact noktalarına hizalama.
  - Retime sırasında ses davranışının açık biçimde tanımlanması.
- **Kompozisyon ve efektler:**
  - Transform, kamera hareketleri ve iç içe kompozisyonlar (precomp).
  - Maskeler, track matte, blend modları ve açık efekt sırası.
  - Renk düzenleme, blur, glow, distortion, shake, flash ve temel geçişler.
  - Dallanabilen efekt grafiği: maskeler ve ara çıktılar paylaşılabilmeli; geçişler birden çok girişli kompozisyonlar olarak ifade edilebilmeli; katmanlı efekt bileşimleriyle yalnızca isimli preset çağırmadan öte özel geçişler kurulabilmelidir.
  - Programlanabilir efektler/shader'lar ve müzik/başka katman/matematiksel ifadeye bağlı parametreler.
  - **Renk ve hassasiyet:** `16-bit float` ara renk tamponları, gerekli hesaplarda daha yüksek hassasiyet, açık renk alanı ve premultiplied-alpha kuralları hedefleniyor. Her artistik işlemin zorunlu linear alanda yapılması öngörülmüyor; işlemin çalışma alanı açıkça tanımlanmalıdır.
  - **Motion blur ve optik akış ayrımı:** Final kalite temporal motion blur, shutter aralığında farklı zamanlarda kompozisyon değerlendirmesiyle tasarlanmalıdır (maske, transform ve geçişler de zaman örneklerine katılmalıdır). Katman hareketi blur'u ile kaynak anime içindeki hareket/optical flow ayrı problemlerdir; yeni ara kare kalitesi otomatik varsayılmamalıdır.
  - **Yetenek sınırı:** Karakter maskesi oluşturma, tracking, optical flow ve segmentasyon ayrı gerçek yetenekler gerektirir; yalnızca komut tanımlamak bunları sağlamaz.

### 4.3. Anlamsal Edit İşlemleri: Olay ve Kısıt Tabanlı Zamanlama

Kullanıcı bu yaklaşımın proje tasarımına eklenmesini onaylamıştır. Bu bir tasarım hedefidir; çalışan bir işlem/API veya genel amaçlı çözücü henüz yoktur.

- **Sorumluluk ayrımı:** Serbest metindeki niyeti agent yorumlar (motor doğal dil veya estetik yargı üretmek zorunda değildir). Agent niyeti açık kaynak/hedef işaretleri, parametreler ve ölçülebilir kısıtlara dönüştürür. Motor kaynak-zaman haritasını, bağımlılıkları ve teknik geçerliliği deterministik olarak hesaplar; sonuç görsel/işitsel incelemeyle ayrıca değerlendirilir.
- **İlk odak — kaynak impact işaretini hedef zamana hizalama:**
  1. Klipte önemli kaynak anı işaretlenir (örneğin yumruğun temas ettiği kare; insan, görsel model veya analiz aracı sağlayabilir — otomatik doğru bulma garantisi yoktur).
  2. Müzikte hedef beat/drop/impact zamanı seçilir (otomatik tespit ile kullanıcı işareti ayırt edilir).
  3. Agent kullanılabilir kaynak aralığını, çıktı aralığını, hız sınırlarını ve önerilen hız eğrisini belirtir.
  4. Motor hedef çıktı anındaki kaynak-zaman eşlemesinin kaynak impact işaretine karşılık gelmesini sağlar; hızlanma/yavaşlama bu ankrajı koruyacak biçimde kurulur.
  5. Kaynak kare/PTS seçimi, kesirli zaman ve tolerans sözleşmeleri çekirdek tasarımda netleştirilecektir; birebir kare doğruluğu uygulanmadan garanti edilmiş sayılmaz.
- **Olaylara bağlı animasyonlar:** Glow, flash, shake veya yazı yalnız sabit bir saniyeye değil, bir olay kimliğine göre zaman ofsetiyle bağlanabilir. Olay çıktı zamanında taşınırsa bağlı animasyonların zamanlaması yeniden hesaplanır; ilgisiz keyframe'ler kendiliğinden taşınmaz. Kaynak görüntüyü izleyen maske/tracking verisi kaynak zamanı üzerinden değerlendirilir ve çıktı olayına bağlı animasyonla karıştırılmaz. Bağımlılıklar ve kısıtlar sorgulanabilir olmalıdır.
- **Geçersiz veya çelişkili istekler:** Yetersiz kaynak aralığı, izin verilen hız sınırları veya çelişkili ankrajlar nedeniyle çözüm bulunamayabilir. Motor ihlal edilen koşulları ve değiştirilebilecek seçenekleri raporlamalı; kaynak aralığı, hedef zaman veya hız sınırını agent/kullanıcı izni olmadan sessizce değiştirmemeli. Değişiklik sürüm/undo ve etkilenen aralık raporuyla ilişkilendirilmelidir.
- **Öznel isteklerin sınırı ve aşamalı kapsam:** "Yüzün okunabilir kalması" gibi hedefler; kadrajda bulunma, boyut, korunacak süre ve blur sınırı gibi ölçülebilir koşullara agent tarafından ayrıştırılabilir, ancak bu kontroller estetik okunabilirliğin tam kanıtı değildir. Kaynak marker tespiti, motorun kısıtları uygulaması ve son render'ın değerlendirilmesi ayrı doğrulama katmanlarıdır. Başlangıçta genel amaçlı evrensel bir kısıt çözücü yerine kaynak işaretini hedef zamana hizalama ve açıkça bağlı efektleri taşıma işlemi hedeflenir.

### 4.4. Önizleme ve Render İlkeleri

- **Rastgele erişimli (seek-independent) değerlendirme:** Belirli proje/sürüm, kaynak medya, zaman ve kalite ayarları için görüntü/ses üretilebilmeli; hesap önceki karelerin sırayla oynatılmış olmasına bağımlı olmamalıdır.
- **Semantik tutarlılık:** Önizleme ve final aynı kompozisyon ve zaman semantiğine dayanmalıdır. Proxy/düşük kalite önizlemesi zamanlamayı veya efektin anlamını değiştirmemeli; farklı kalite seviyelerindeki farklar açık ve test edilebilir olmalıdır.
- **İzlenebilirlik:** Her render belirli bir proje sürümüne bağlı olmalıdır. Headless render, taslak render ve deterministik yeniden üretim desteklenmelidir.

---

## 5. Agent Katmanı, Çoklu İnceleme ve Yaratıcı Yönetmenlik

### 5.1. Skill ve Bilgi Mimarisi

- **Sorgulanabilir API sözleşmesi:** Her atomik işlem için ayrı skill yerine motorun sorgulanabilir, sürümle eşleşen API şeması kullanılmalıdır. Parametre, zaman birimi, koordinat sistemi, yan etki ve doğrulama yöntemleri bu sözleşmede yer almalıdır.
- **Teknik aileleri ve karar süreçleri:** Skill'ler ritim/kurgu, velocity/time-remap, hareket/geçişler, maske/kompozisyon, typography ve kalite kontrol ailelerini öğretmelidir.
- **Teknik tarif vs. yaratıcı stil:** Bir geçişin nasıl oluşturulduğu, ne zaman kullanılmasının doğru olduğu anlamına gelmez; bu iki katman ayrılmalıdır.
- **Teknik paket içeriği:** Amaç, girdiler, yöntem, ayarlanabilir noktalar, başarısızlık örnekleri, gerçek kaynak klip, düzenlenebilir örnek proje ve doğrulanmış çıktı.
- **Tembel yükleme disiplini:** Rehberler ihtiyaç oldukça yüklenmeli; henüz çalışmayan motor için yüzlerce spekülatif skill yazılmamalıdır.
- **Önerilen agent döngüsü:** `isteği anla -> materyali incele -> ilgili rehberi yükle -> küçük bölüm oluştur -> render et -> kontrol et -> revize et`.

### 5.2. Çoklu Geri Bildirim ve İnceleme Sistemi

Agent seçili bir bölümü görsel, video ve kodsal olarak ayrı ayrı veya birlikte inceleyebilmelidir. (Bu bölümdeki ayrıntılar henüz uygulanmış özellik değil, tasarım önerileridir.)

- **Dört inceleme yüzeyi:**
  1. *Görsel:* Tek kare, tam çözünürlüklü kare, crop, maske ve ara efekt/node çıktısı.
  2. *Video:* Seçili olayın öncesi/sonrasını içeren kısa sesli taslak.
  3. *Kodsal:* İlgili klipler, node bağlantıları, parametreler, expression/shader kaynakları ve animasyon eğrileri.
  4. *Teknik:* Kaynak-zaman eşlemesi, beat/impact noktaları, kalite ayarları ve uyarılar.
- **Ortak kimlik ve sürüm bağı:** Bütün yüzeyler aynı proje revision/version kimliği, medya/efekt sürümleri ve zaman referanslarıyla ilişkilendirilmelidir; eski render ile yeni kodun yanlışlıkla eşleştirilmesi önlenmelidir. Karelerin kimlikleri, composition zamanı ve gerektiğinde kaynak zamanı ilgili klip/node tanımlarına bağlanmalıdır.
- **Contact sheet stratejisi:** Video anlayamayan fakat görsel görebilen modeller için tek görselde zaman etiketli kareler sunulur. Genel bakış için zaman aralığı boyunca seyrek örnekleme (sahne sırası, renk, kompozisyon); olay odaklı inceleme için kesme, impact ve geçiş çevresinde sık örnekleme yapılır. Her karenin altında okunabilir kimlik ve zaman etiketi bulunmalı; tam kare veya crop yeniden istenebilmelidir. Aşırı büyük tek kolaj yerine küçük, okunabilir sayfalar tercih edilir (küçültme maske/yüz ayrıntısını yok edebilir). Sabit örnek adedi yerine içerik ve şüpheli olay belirleyici olmalıdır.
- **Hata türüne göre görsel paketler:**
  - *Maske:* Kaynak kare + maske + dama zemin üzerinde ayrılmış karakter + final kompozit.
  - *Geçiş:* Önceki klip + sonraki klip + birleşmiş çıktı (eşleşen zamanlarda).
  - *Efekt:* Önce/sonra ve ilgili node'un ara çıktısı.
  - *Velocity:* Çıktı kareleri + kaynak zamanları + hız eğrisi.
  - *Revizyon:* Eski/yeni sürümler aynı zamanlarda (görsel karşılaştırma teknik fark ölçümlerinden ayrı tutulur).
- **İnceleme paketi ve aktarım:** Birlikte incelemek her şeyi tek resme gömmek değildir: kod metin, görüntü görsel olarak kalmalı ve ortak kimliklerle eşleşmelidir. Yalnız yerel dosya yolu döndürmek modele görsel erişim sağlamaz; agent adaptörü desteklediği medya türünü modele gerçekten ulaştırmalıdır.
- **Yeteneğe göre doğrulama sınırları:**
  - Beat, enerji, sahne sınırı, medya bilgisi, kare atlama ve teknik kalite kontrollerinde önce algoritmalar/kod çalışır; bunlar teknik belirti kontrolüdür, estetik ritim kanıtı sayılmaz.
  - Görsel model sahne anlamı, karakter, impact, hareket yönü, kompozisyon, maske ve belirgin süreksizlikleri değerlendirebilir; ancak contact sheet videonun tam karşılığı değildir, örnekler arasındaki titremeyi kaçırabilir, akıcılık ve ses senkronunu tek başına kanıtlamaz.
  - Video/ses inceleyebilen model veya insan, hareket ve ritmi ayrıca değerlendirir.
  - Sadece metin gören model için kolaj yeterli değildir; yapılandırılmış durum/ölçümler sunulabilir ama görsel kalite doğrudan doğrulanmış sayılmaz.
  - Rapor neyin gerçekten incelendiğini, neyin doğrulanamadığını ve kullanılan kalite/örnekleme kapsamını açık tutmalıdır.
- **Önerilen inceleme döngüsü:** `genel bölüm özeti ve contact sheet -> şüpheli aralığı daha sık örnekleme -> tam kare/crop/maske ve ilgili kodu inceleme -> hedefli değişiklik -> etkilenen bölümü yeniden render -> aynı zamanlarda karşılaştırma` (amaç bütün projeyi bağlama yığmak değil, sorunu doğru zaman, kaynak medya ve kodla ilişkilendirmektir).

### 5.3. Yaratıcı Yönetmen Katmanı: Karar Üretme ve Değerlendirme Döngüsü

Kullanıcı yaratıcı kararları agent'ın vermesi gerektiğini vurgulamış ve bu çerçevenin tasarım notlarına eklenmesini istemiştir. Motorun teknik yetenekleri ile agent'ın yaratıcı değerlendirmesi ayrı sorumluluklardır; bu bölüm uygulanmış özellik veya iyi edit garantisi değildir.

- **Materyali anlama:** Agent kaynakları karakter/olay, hareket başlangıcı ve zirvesi, bakış/hareket yönü, kompozisyondaki boş alan, duygu ve görsel yoğunluk bakımından inceleyebilmelidir. Her beat'e efekt eklemek yerine yoğunluk, sakinlik, beklenti, drop ve hareket sürekliliği değerlendirilmelidir. Kod teknik ölçümleri; uygun görsel/video/ses modeli anlam ve yorumları sağlayabilir. Gözlem, tahmin, güven düzeyi ve kaynak zamanları ayrılmalı; yorum kesin gerçek sayılmamalıdır. Kaynak seçimi ve estetik planlama motorun teknik kapasitesinden ayrı katmanlardır.
- **Referanstan görsel dil çıkarma:** Efekt adlarını kopyalamak yerine dikkat odağı, yoğunluğun değişimi, sahneler arası görsel süreklilik, sade bırakılan bölümler ve hareketin geçişlerde devamı analiz edilmelidir. Çıktı bir efekt listesi değil, materyale uyarlanabilir bir görsel yaklaşım olmalıdır. Birebir yeniden üretim garantisi yoktur; referansın yaratıcı niyeti kesin bilinemeyeceği için çıkarımlar taslak render'larla sınanmalıdır.
- **Alternatif yaklaşımlar ve hedefli denemeler:** Agent aynı materyale farklı yaratıcı yaklaşımlar üretebilmelidir (örneğin karakter odaklı, grafik/kolaj odaklı veya impact/velocity odaklı — bunlar sabit zorunlu kategoriler değildir). Seçimin materyal ve brief'e neden uyduğu açıklanabilmeli; her kararda kullanıcı onayı beklenmeden mevcut yetki ve maliyet sınırları korunmalıdır. Önemli veya belirsiz bölümler için kısa taslaklar render edilip karşılaştırılabilir.
- **Yaratıcı karar kaydı:** Her önemli karar; seçilen yaklaşım, gerekçe, kanıt/kaynak, korunacak özellikler ve kontrol edilecek risklerle ilişkilendirilmelidir (sonraki revizyonda gerekçenin kaybolmaması ve onaylı kararların ilgisiz değişikliklerle bozulmaması için; örneğin *yüz ifadesi ana odak olduğundan yüzü kapatan grafik yoğunluğunu azalt; sonraki yoğun sahneye geçişi ayrıca kontrol et*).
- **Eleştiri ve revizyon:** İnceleme aşaması şu somut soruları sormalıdır: dikkat odağı doğru mu; efekt hareketi destekliyor mu gizliyor mu; sahne ve geçişler anlamlı mı; yoğunluk değişiyor mu; referansın mantığı mı yoksa yalnız yüzeyi mi aktarılmış? Teknik doğruluk ve estetik değerlendirme ayrı tutulmalıdır. Ayrı bir modelin beğenmesi objektif kalite kanıtı değildir; görsel/video/ses erişimi olmayan model ilgili niteliği doğrulanmış saymamalıdır. Eleştiri belirli bölüm ve proje sürümüne bağlanmalıdır.
- **Kullanıcı tercihlerini öğrenme:** Geri bildirim ilgili bölüm, alternatif ve değişiklikle ilişkilendirilmelidir. Genel kullanıcı tercihi, projeye özgü tercih ve tek sahne kararı ayrı tutulmalıdır (bir sahnede shake azaltılması gelecekteki bütün editlerde shake yasağına dönüştürülmemelidir). Tercih modeli başlangıçta açık, düzenlenebilir kayıtlar olabilir; ayrı eğitim/fine-tuning sistemi seçilmemiştir.
- **Yaratıcı yönergenin özü ve döngüsü:** Efekt sayısını kaliteyle eşitleme; her önemli efektin yaklaşımda bir görevi olsun; belirsiz kararları kısa alternatif render'larla sınayıp karşılaştır; onaylı kararları koru; görmediğin/duymadığın niteliği doğrulanmış sayma. Önerilen döngü: `materyali anla -> görsel yaklaşım seç -> sahne planla -> hedefli kısa taslaklar üret -> karşılaştır -> revize et -> kullanıcı geri bildirimiyle tercih kayıtlarını güncelle`.

---

## 6. İlk Doğrulama Hedefi ve Kalite Değerlendirmesi

### 6.1. İlk Kabul Hedefi ve Hata Ayrımı

Önce kullanma izni olan kaynaklarla kısa bir referans editi **doğrudan proje tanımı üzerinden** kaliteli biçimde üretmek; agent otomasyonunu daha sonra aynı yeteneklere bağlamak hedeflenir. Bu ayrım hatanın kaynağını netleştirir:
- Doğrudan proje tanımıyla da sonuç alınamıyorsa **motor yetersizdir**.
- Motor sonucu üretebiliyor ama agent oluşturamıyorsa **planlama/kontrol katmanı yetersizdir**.

**İlk kabul hedefi (henüz gerçekleştirilmedi):** Dışarıdan gelen klip ve müzikle **hız eğrisi + hareketli maske + shader geçişi + temporal motion blur** içeren kısa bir bölümü yalnızca proje tanımı üzerinden render etmek. Bu testte rastgele seek, ses senkronu, tekrar üretilebilirlik ve preview/final tutarlılığı ölçülmelidir.

### 6.2. Önerilen İlk Uçtan Uca Kapsam

Aşağıdaki sıra ilk kapsam önerisidir (henüz kullanıcı tarafından nihai geliştirme planı olarak onaylanmamıştır):
1. Klip/ses içe aktarma ve proxy.
2. Beat/sahne analizi ve kaynak kare inceleme.
3. Kesme, sıralama, crop ve katmanlar.
4. Speed curve, zoom, shake, flash ve temel geçişler.
5. Timeline, undo, taslak/final render ve sohbetle revizyon.
6. Programatik erişim; daha sonra MCP katmanı.

*İlk kapsam dışında önerilenler:* Otomatik karakter rotoskopisi, görüntü üretimi ve efekt marketplace'i.

### 6.3. Kalite Değerlendirme İlkeleri

- Özellik sayısı yerine gerçek referans edit koleksiyonu (velocity, maskeli geçiş, typography, hareket eşlemeli kesme ve sakin sinematik edit gibi farklı örnekler).
- Hız eğrisi, katman, maske, motion blur ve ses senkronunun birlikte sınanması.
- Önizleme/export tutarlılığı, kare doğruluğu ve ses senkronu teknik testleri.
- Görsel kalite ayrıca insan değerlendirmesi gerektirir; teknik test veya LLM yorumu tek başına Katana/TikTok kalitesi kanıtı değildir.
- Henüz örnek medya, benchmark, kabul eşiği veya çalışan render yoktur.

---

## 7. Açık Kararlar

- **Adlandırma ve kapsam:** Proje adı **FrameShift** olarak kesinleşmiş olmakla birlikte marka, alan adı ve paket adı uygunluğu henüz kontrol edilmedi; hedef kullanıcı kapsamı ve proje lisansı netleştirilecek.
- **Çekirdek şema ve zaman sözleşmesi:** Proje şeması, kesin zaman tabanı (Flicks/rasyonel zaman, VFR PTS eşleme, NTSC `1000/1001` sözleşmesi, ses örnekleme ayrıntıları), render grafiği ve efekt sözleşmesinin koda geçmeden önce kilitlenmesi.
- **Yeniden kullanım sınırı:** Seçilen native/GPU yaklaşımında mevcut açık kaynak bileşenlerin yeniden kullanım sınırı (herhangi bir fork kararı yok).
- **Text/vector ve UI:** Skia veya alternatif text/vector entegrasyonu; masaüstü UI teknolojisi ve pencere/süreç yaşam döngüsü tasarımı (UI şimdilik ilk motor kapsamı dışında).
- **Platform ve dağıtım:** Hedef işletim sistemleri, GPU tabanı, hardware decode -> GPU -> encode zero-copy doğrulaması, FFmpeg build/codec/lisans koşulları ve paketleme hedefleri.
- **Güvenlik:** Güvenli expression/shader/plugin yürütme modeli.
- **Kabul ölçütleri:** Referans medya kullanım izinleri ve somut kalite/kabul eşikleri.
- **Model ve maliyet:** Yerel model, bulut model ve maliyet politikası.

---

## 8. Araştırma ve Karar Geçmişi (Arşiv)

Bu bölüm, güncel **FrameShift** kararlarından (`Rust + wgpu/WGSL + FFmpeg + OpenColorIO + Skia/alternatif`, tek masaüstü uygulaması + bağımsız çekirdek + headless desteği) önceki aşamalarda yapılan araç/motor araştırmalarını ve güncellenen ara değerlendirmeleri arşivler. Bağımsız kurulum veya render benchmark'ı yapılmamıştır; değişebilen ürün ayrıntıları uygulama öncesinde yeniden doğrulanmalıdır. Genişletilmiş alternatif karşılaştırması için [ENGINE_ALTERNATIVES_RESEARCH.md](ENGINE_ALTERNATIVES_RESEARCH.md) dosyasına bakınız.

### 8.1. Güncellenen Eski Aşama Varsayımları

- **Geçici ad dönemi:** İlk notlarda proje adının geçici olduğu ve henüz seçilmediği yazılmıştı; ardından ürün adı **FrameShift** olarak kesinleşti.
- **Yalnız headless ara değerlendirmesi:** Native/GPU yöneliminden sonra bir ara "asıl operatör agent olduğundan UI gerekmeyebileceği, headless-first yaklaşımın öne çıktığı" değerlendirmesi yapıldı. Ardından kullanıcının agent işlemlerini görebilme ve klipleri elle düzenleyebilme talebiyle "tek masaüstü uygulaması + bağımsız çekirdek + korunan headless desteği" kararına geçildi. (Tam UI hiçbir aşamada kalıcı olarak iptal edilmemiş, CLI/API sözleşmesi de kesinleştirilmemişti.)
- **Altyapı kararsızlığı dönemi:** İlk oturumlarda web/uygulama adayları araştırılmış ve "render motoru / medya altyapısı seçilmedi" notları yer almıştı; ardından yukarıdaki Bölüm 3'te belgelenen başlangıç stack'i onaylandı.

### 8.2. İlk Araştırmada Değerlendirilen Araçlar

- **FableCut:** Agent'a uygun MCP/REST, düzenlenebilir JSON timeline ve tarayıcı editörü nedeniyle başlangıç tabanı olarak değerlendirildi. Belgelerde beat işlemleri, keyframe'li speed ramp, maske, tracking ve headless export bulunuyor. `beats.js` kodunda ramp uygulanmış klipleri yeniden beat-cut yapma kısıtı görüldü (önerilen sıra: önce beat-cut, sonra ramp). En iyi editör olduğu gösterilmedi; kod ve gerçek render doğruluğu incelenmeden temel seçilmeyecek olup fork kararı verilmedi.
- **After Effects + Engine Room MCP:** Profesyonel efekt ve ince kontrol için güçlü aday. Katmanlar, keyframe, easing, efekt ve expression kontrolü belgeli; ancak MCP kapsamı AE'nin toplam yeteneğiyle aynı değil ve hazır otomatik AMV iş akışı ayrıca gerekir.
- **Higgsfield Katana:** Kullanıcının beğendiği ana görsel referans. Referans edit/preset ve karakter/ürün görsellerinden yeni bir edit kuran akış; gerektiğinde yeni sahne üretimi içeriyor (yalnızca mevcut kareleri koruyan klasik edit motoruyla aynı kategori değil). Higgsfield genel bağlantı rehberi Hermes/CLI desteği veriyor; Katana'nın Claude deneyiminin Hermes'te birebir çalışması doğrulanmadı. Ücretli plan ve kredi koşulları var (agent üzerinden üretim web unlimited kapsamına girmeyebilir). Bu projede Higgsfield bağlantısı kurulmadı ve kredi harcanmadı.
- **Diğer araçlar (Resolve, Premiere, Remotion, CapCut):**
  - *Resolve MCP:* Tek uygulamada kurgu/renk/render için değerlendirildi; sürüm/API ve retime kısıtları var.
  - *Premiere MCP:* Native speed kontrolü nedeniyle temkinli değerlendirildi.
  - *Remotion:* Programlanabilir kompozisyon seçeneği (eski hosted documentation MCP'sinin kapanmış olması Remotion'ın kendisinin kapanması anlamına gelmiyor).
  - *CapCut:* Draft araçları uygulamayı canlı kontrol etmekle karıştırılmamalı.

### 8.3. OpenMotion ve HyperFrames İncelemeleri

- **OpenMotion:** Kodla video yaklaşımı: HTML/CSS/JavaScript kompozisyonları, harici medya, düzenlenebilir Studio ve MP4 export. `Motion.md` yaratıcı kararları, `SKILL.md` prosedürü, kaynak proje uygulamayı, preset sözleşmesi değişebilir girdileri, eşlenmiş render ve sürüm/hash kayıtları kanıtı taşır (`Motion.md` tek başına otomatik animasyon parser'ı veya motorlar arası çalıştırma garantisi değildir). MCP'de revision/version korumalı proje yazımı, geçmiş ve snapshot üzerinden export belgelenmiş. Bağımsız renderer değil; incelenen `package.json` HyperFrames paketlerini `0.9.7`'ye sabitliyor. Apache-2.0 lisanslı; bağımlılık, medya ve asset lisansları ayrıca incelenmeli. Örnekler ağırlıkla marka/ürün/konuşmacı/motion graphics; anime edit kalitesi doğrulanmadı; fork/clone yapılmadı.
- **HyperFrames:** HTML/CSS/medya ve seek edilebilir animasyonları videoya dönüştüren, agent skill ve CLI sunan motor (Apache-2.0). İnceleme anında `main/core` ve son GitHub release `0.9.64` (OpenMotion'ın `0.9.7` bağımlılığıyla karıştırılmamalı). Gerçek speed ramp: `data-automation` içindeki `rate` kanalı `sourceTimeAt` hız eğrisinin integralini sayısal tablodan hesaplar; `0.1x-10x` aralığına sıkıştırılıyor (reverse veya tam freeze sağlamıyor), ses export'u kısa sabit hız dilimleriyle eğriyi yaklaşık izliyor, yeni ara kare üretmiyor (anime optical-flow/slow-motion kalitesi doğrulanmadı). Sayfa motion blur'unda varsayılan SVG filter (zoom/dönüş izi yaklaşık, mask/clip-path kenarları sertleşebilir) ve isteğe bağlı capture (örtüşme sırası, blend ve clipping kısıtları) var; sub-frame shutter örnekleri ortalayan blur ise shader geçişlerinin kullandığı layered compositor yoluyla desteklenmiyor ve kaynak kodunda açık hata ile reddediliyor. Testler yerelde çalıştırılmadı ve render benchmark'ı yapılmadı.
- **Eski ortak değerlendirme sahnesi önerisi:** Native/GPU kararına geçilmeden önce, HyperFrames ile olası fork adaylarını kıyaslamak için *orijinal anime klibinde impact'e hizalı smooth velocity, zoom/dönüş blur'u, maskeli karakter arkasında yazı, shader geçişi, ses senkronu ve preview/final karşılaştırması* önerilmişti.

### 8.4. Arşiv Kaynak Bağlantıları

**Higgsfield Katana ve Referans Araçlar:**
- Kullanıcının paylaştığı Katana videosu: https://x.com/higgsfield/status/2108294775644585998/video/1?a=Vl0idbhMkSBfcEOMTWY&s=19
- Katana kullanım rehberi: https://higgsfield.ai/creator-hub/help-center/integrations/how-do-i-use-katana
- Katana açıklaması: https://higgsfield.ai/blog/higgsfield-katana
- Higgsfield agent bağlantısı: https://higgsfield.ai/creator-hub/help-center/integrations/how-do-i-connect-higgsfield-to-ai-agent
- FableCut: https://github.com/ronak-create/FableCut
- FableCut beat işlemleri: https://raw.githubusercontent.com/ronak-create/FableCut/main/beats.js
- Engine Room AE MCP: https://github.com/Engine-Room-Games/after-effects-mcp
- Resolve MCP: https://github.com/samuelgursky/davinci-resolve-mcp
- Resolve native retime sınırları: https://raw.githubusercontent.com/samuelgursky/davinci-resolve-mcp/main/docs/guides/native-drt-authoring.md
- Premiere işlem kataloğu: https://raw.githubusercontent.com/leancoderkavy/premiere-pro-mcp/main/docs/supported-actions.md
- Remotion eski documentation MCP durumu: https://www.remotion.dev/docs/ai/mcp

**OpenMotion:**
- Ana depo: https://github.com/nexu-io/openmotion
- `motion-md.md`: https://github.com/nexu-io/openmotion/blob/main/docs/motion-md.md
- `mcp-phase1-tools.md`: https://github.com/nexu-io/openmotion/blob/main/docs/mcp-phase1-tools.md
- `hyperframes.md`: https://github.com/nexu-io/openmotion/blob/main/docs/hyperframes.md
- `package.json`: https://github.com/nexu-io/openmotion/blob/main/package.json

**HyperFrames:**
- Ana depo: https://github.com/heygen-com/hyperframes
- Speed ramps dokümanı: https://github.com/heygen-com/hyperframes/blob/main/docs/reference/speed-ramps.mdx
- `speedRamp.ts`: https://github.com/heygen-com/hyperframes/blob/main/packages/core/src/speedRamp.ts
- Motion blur rehberi: https://github.com/heygen-com/hyperframes/blob/main/docs/guides/motion-blur.mdx
- `motionBlur.ts`: https://github.com/heygen-com/hyperframes/blob/main/packages/engine/src/services/motionBlur.ts
- `motionBlurRoute.ts`: https://github.com/heygen-com/hyperframes/blob/main/packages/producer/src/services/render/motionBlurRoute.ts
