# FrameShift — HyperFrames, OpenMotion ve After Effects’ten Ürün Dersleri

**Tarih:** 2026-11-22 · **Durum:** Ürün önerisi / onaylanmış ADR değil  
**Hedef:** Native GPU motorunu değiştirmeden, anime AMV ve kinetik tipografi için kurulup kullanılabilen bir insan + ajan editörü yapmak.  
**Kanıt sınırı:** Dış kaynaklar doküman ve kod incelemesidir. Rakipler kurulmadı; ortak render, hız veya kalite benchmark’ı yapılmadı. Aşağıdaki kabul eşikleri hedeftir, ölçüm değildir.

## 1. Ürün önerisi

FrameShift’in ayırt edici işi **müzikteki bir vuruşa, kaynak görüntüdeki belirli bir anı kilitlemek**. Kullanıcı, o anı timeline’da taşıyabilmeli; kaynak kare eşleşmesi, ona bağlı impact efektleri ve ilgili metin birlikte güncellenmeli. Değişiklik gözden geçirilebilmeli, geri alınabilmeli ve seçili proje sürümü dışa aktarılabilmeli.

| Kaynak | Alınacak fikir | FrameShift’e özgü uygulama | Alınmayacak şey |
| --- | --- | --- | --- |
| [HyperFrames](https://github.com/heygen-com/hyperframes) | Kısa başlangıç, planla → lint → önizle → render; örnek ve bileşen kataloğu | Hazır AMV başlangıcı, yapılandırılmış proje hataları, tek aralık için PNG + sesli taslak | Chromium/HTML kompozisyon çekirdeği; web ekosistemiyle genel özellik yarışı |
| [OpenMotion](https://github.com/nexu-io/openmotion) | Seçilebilir şablonlar, Studio ve ajan için ortak proje, belirli sürümden export | Tipli slotlar, stil kartı + çalışan proje, atomik ajan değişikliği, sabit kaynak manifesti | İkinci proje modeli, şablon indirince kod çalıştırma, bulut/medya üretim servisleri |
| [After Effects](https://helpx.adobe.com/after-effects/user-guide.html) | Graph Editor, precomp, track matte, text animator, efekt kontrolü, render kuyruğu | Aynı GPU grafiğine timeline ve katman görünümü; görünür zaman eşleme, tipli seçici ve matte bağlantıları | Her paneli/efekti kopyalamak, JSX ve eklenti uyumluluğu, yerleşik Roto Brush klonu |

### Hedef netleştirmesi — efekt odaklı TikTok anime editleri

Kullanıcının yeni örneği [SC-0003](../style-cards/SC-0003-theamirhanio-curse-tiktok-edit.md). Genel AMV ve tipografi demosu tek başına yeterli ürün testi değil. Impact Cut dilimine, kısa bir sakin → yoğun pencere ve birlikte düzenlenebilen **hareket / vurgu / geçiş** yığını ekle. İlk prototipi mevcut render düğümleriyle dene; yeni shader talebini gözlemlenmiş açığa bağla. Şablonu dikeyle sınırlama: örnek dosya 7:6. Stil kartı teknik ön incelemedir; bu örneğin kesin efekt tarifi çıkarılmış değildir.

### Mevcut taban ve gerçek açıklar

- **Var:** Tam sayı Flicks, sürümlü proje ve render grafiği, CLI/MCP, revizyon denetimli komutlar, uygulama denetleyicisi ve statik arayüz görüntüsü. [Şema](../specs/project-schema.md#L143-L168) · [MCP](../../crates/frameshift-mcp/src/server.rs#L280-L312) · [Uygulama katmanı](../../crates/frameshift-app/src/lib.rs#L1-L22).
- **Var:** Gerçek Asta çıktısı: **20 sn, 1080p60, 1.200 kare**. Bu, uçtan uca motor kanıtı; günlük editör kullanımının kanıtı değil. [Üretim kaydı](../log/2026-11-22-asta-v3-red-line.md#L67-L71).
- **Darboğaz:** Mushoku denemesinde 30 kelime için **190 ayrı harf düğümü** kurulmuş; karakter arkasındaki yazı gerçek alfa matte yerine luma ile taklit edilmiş. [Boşluk analizi](2026-11-22-kinetic-typography-gap-analysis.md#L49-L59).
- **İlk kapı:** Son karşılaştırma koşusunda iki `autocut` testi kaldı. Özellikle sıfırdaki vuruş ve boş vuruş listesinde başlangıç/bitiş işareti kuralları açık davranış istiyor. Yeni faza başlamadan tekrar ölçülmeli. [Koşu kaydı](../log/2026-11-22-hyperframes-openmotion-frameshift-karsilastirma.md#L14-L18) · [Testler](../../crates/frameshift-core/src/autocut.rs#L60-L91).

## 2. P0 — Önce tek bir tamamlanmış kurgu akışı

### P0.1 — Doğrulama, başlangıç projesi ve ilk taslak · HyperFrames

**Ders:** Örnek ve hızlı hata geri bildirimi, efekt kataloğundan önce kullanıcıya değer verir. HyperFrames’in [CLI becerisi](https://github.com/heygen-com/hyperframes/tree/main/skills/hyperframes-cli) proje açma, kontrol, kare inceleme ve render’ı tek akışta anlatır.

**Uygulama:** Mevcut CLI komutlarını yeniden kullan. Lisansı açık veya sentetik medya ile 8–12 saniyelik **Impact Cut** örneği sun: 3 klip, 1 müzik, 1 impact işareti, 1 yazı ve 1 geçiş. Kurulum ve medya denetimi FFmpeg erişimi, GPU adaptörü, eksik dosya/font, proje şeması ve çözülemeyen ankrajı ayırsın. İnsan için kısa mesaj; ajan için kararlı hata kodu, nesne kimliği, tick ve düzeltme önerisi. Örneğin `MissingAsset` hatası hangi klibin ve kaç referansın etkilendiğini göstersin; sessiz siyah kare üretmesin.

**Kapsam:** Var olan `inspect`, aralık render’ı ve doğrulama komutları yeterliyse yeni komut/ikili yaratma. Önce `autocut` sınır sözleşmesini test et: 0 vuruşu, boş analiz, çift vuruş, min. segment, `include_start/end` kombinasyonları. Vuruş önerisi ile klipleri gerçekten kesen işlem kullanıcıya ayrı gösterilsin.

**Kabul:** Yeni kullanıcı test makinesinde tek örneği kod yazmadan açıp 4–8 saniyelik sesli taslak üretebilir. Eksik medya ve çözülemeyen impact farklı hata kodu getirir. İlk taslak süresi kaydedilir; eski ve yeni akış aynı makine ve projeyle ölçülür.

### P0.2 — Canlı timeline + olay ankrajı · OpenMotion + After Effects

**Uygulama:** Bir ekranla başla: önizleme, temel timeline, öğe özellikleri ve kısa değişiklik geçmişi. İlk işlemler: scrub, split/trim, playhead, kaynak impact işareti, hedef beat/drop işareti, snap ve geri al/yinele. Dalga formu ayrı bir analiz ürünü; zaman çizgisi motor zamanından çizilir. Ajan da insan da aynı proje komutlarını kullanır. Oturum veya UI, ikinci bir edit durumu oluşturmaz.

**FrameShift farkı:** Timeline üç şeyi etiketleyerek ayırsın: **çıktı zamanı**, **kaynak zamanı**, **olay kimliği**. Kullanıcı drop’u taşırken olay bağlantılı flash, shake ve yazı hareket eder; bağımsız keyframe’ler kaymaz. Desteklenen hız/aralık dışında motor sessizce klibi kırpmaz; sınırı ve uygulanabilir seçeneği gösterir. Scrub sırasında son istek kazanır; eskimiş kare yanıtı _revizyon + tick + istek kimliği_ eşleşmeden ekrana basılmaz. [Olay ankrajı kararı](../decisions/0003-semantic-event-constraint-timing.md#L18-L27) · [Kesin zaman tabanı](../specs/timebase.md#L22-L28).

**Kabul:** Bir editte hedef drop 6.00’dan 6.25 saniyeye taşınır; kaynak impact karesi yeniden eşlenir, bağlı efektler yeni olayla taşınır, bağımsız efekt değişmez. Aynı düzenleme UI ve MCP üzerinden aynı sonucu verir. Undo, önceki proje içeriğini geri getirir; revizyon sayacı geri sarılmaz. İlk ekranda node editörü veya çoklu pencere zorunlu değil.

### P0.3 — Ajan değişiklik fişi ve sabit sürüm export · OpenMotion + After Effects

**Ders:** OpenMotion’ın [MCP sözleşmesi](https://github.com/nexu-io/openmotion/blob/main/docs/mcp-phase1-tools.md) export’u bir proje sürümü ve materyal anlık kopyasına bağlar. After Effects [Render Queue](https://helpx.adobe.com/after-effects/using/basics-rendering-exporting.html) render ayarı ile çıktı kodlama biçimini ayırır.

**Uygulama:** Önce ajan değişikliğini açık bir fiş olarak göster: niyet, etkilenen nesne ve aralıklar, taşınan olaylar, korunacak yaratıcı kararlar, değişiklik öncesi/sonrası bir kare veya kısa sesli klip. Zamanlama, medya veya render grafiği değişiyorsa uygulama öncesinde beklenen revizyonu kontrol et. İlk sürümde tek geri alınabilir grup yeterli; dal, çok kullanıcılı birleştirme veya iş akışı dili yok.

**Export:** Kuyruğa alırken proje kimliği, revizyon, profil, tick aralığı, medya/font/shader parmak izleri ve çıktı ayarlarını sabitle. İçerik kopyalanmadıysa hash değişikliğinde iş iptal veya açık hata ile dursun; güncel dosyayı sessiz kullanmasın. İptal edilen iş final dosyası bırakmasın; başarıda çıktı manifesti ve teknik doğrulama kaydı olsun. İlk hedef tek MP4 ve yerel kuyruk; çoklu kodlama ve kuyruk kurtarma sonraki dilim.

**Kabul:** Ajan revizyon 12’ye teklif verirken insan 13’e geçerse teklif uygulanmaz. Revizyon 13’ten kuyruğa giren render, zaman çizgisi 14’e geçtikten sonra da 13’ü çıkarır. Çıktı `ffprobe` ile süre/FPS/codec/ses açısından ve kısa klip manuel izleme ile kontrol edilir. [İnceleme kimliği](../specs/inspection-contract.md#L22-L54).

## 3. P1 — AMV üretimini hızlandıracak nitelikli özellikler

### P1.1 — Olay ankrajlı hız grafiği · After Effects Graph Editor

Adobe’nin [Speed Graph](https://helpx.adobe.com/after-effects/using/speed.html) yaklaşımı hızın zamanla nasıl değiştiğini görünür kılar. FrameShift’te aynı görünümde iki ayrı kip olsun: **oynatma hızı** ve **kaynak zaman eşlemesi**. Hız tablosu motorun hesapladığı eşlemeden üretilsin; UI kendisi farklı bir eğri entegre etmesin. X ekseninde çıktı tick’i, kaynak impact işaretinde kaynak tick’i ve hedef beat, köşede de imlecin iki zaman karşılığı gösterilsin. Sınırlandırılmış tanjant ve birkaç easing ile başla; ters oynatma, freeze, optical flow ve ses retime kalitesini ayrı karar/test olmadan taahhüt etme.

**Kabul:** Impact örneğinde hedef işaretten önce/sonra seçilen karelerde teknik rapor ve UI aynı kaynak tick’lerini gösterir. Kesirli FPS/VFR örnekleri doğrulanır; draft ve final kaynak zamanı ve maskeyi aynı yorumlar. Kullanıcı geçersiz kaynak aralığını grafikte görebilir.

### P1.2 — Harf/kelime seçicili tipografi · After Effects Text Animators

Adobe’nin [text animator + selector ayrımı](https://helpx.adobe.com/after-effects/using/animating-text.html), tek metin katmanında bir özelliğin kapsamını ayrı yönetir. FrameShift’te mevcut shaping hattını koru. İlk dalgada `VectorText` için aralık seçici, sıra gecikmesi ve pozisyon/opaklık/ölçek; ardından rotasyon, renk ve `Text3D`. Kalıcı gecikme tam sayı tick olsun. Kullanıcıya görünen birim (harf/grafem veya kelime) ile shaping çıktısındaki glyph aynı indeks değildir: birleşik işaret, ligatür ve sağdan sola metin için küme eşlemesi ve açık fallback gerekir. Kelime zamanlarını önce içe aktarılan işaretlerden al; otomatik söz çıkarma çekirdek önkoşul olmasın.

**Kabul:** Mushoku’dan seçilen **5 kelimelik** bir pencere, harf başına node olmadan kelime başına en fazla bir metin node’u ile oluşur. Birleşik karakter, ligatür, boşluk ve iki farklı yönlü metin için sıralama testi eklenir. Karşılaştırma kısa sesli klipte ve kelime çevresinde kare düzeyi kanıtla yapılır; düşük FPS kare şeridi ses senkronu kanıtı değildir.

### P1.3 — Yeniden kullanılabilir matte ve karakter arkası yazı · After Effects Track Matte

Adobe’nin [Track Matte](https://helpx.adobe.com/after-effects/using/track-mattes-and-traveling-mattes.html) yaklaşımından alınacak fikir: Görüntüyü tutan katman ile görünürlüğü üreten katman ayrı olsun. Bir matte birden fazla katmana bağlanabilsin; kaynak UI’da adıyla görünsün, komşu katman sırasına bağlı olmasın. FrameShift zaten `MaskMatte` barındırıyor; ilk iş yeni efekt değil, mevcut grafiğe bağlanabilen **harici alfa matte akışı** ve seçili matte görünümü. Tek bir zamanlı matte girdi yolunu seç; tek kare test girdisi ve kısa zamanlı matte fikstürü ile başla. Kaynağı takip eden matte aynı source-time, crop ve koordinat dönüşümünü paylaşsın. Eksik kare açık hata versin; luma fallback ancak kullanıcı seçerse.

**Kabul:** Hem parlak ten/giysi hem koyu fonda yazı karakterin arkasında kalır; retime uygulandığında matte doğru kaynak kareyi izler. [MaskPack](../specs/inspection-contract.md#L66-L73) yanında kısa oynatım izlenir. Segmentasyon/derinlik modeli, kullanışlı girdi ve edge kalite kapısı olmadan sonraki sürüme alınmaz.

### P1.4 — Düzenlenebilir şablon ve precomp · OpenMotion + After Effects

OpenMotion’ın [Motion Package taslağı](https://github.com/nexu-io/openmotion/blob/main/docs/motion-package-spec.md) niyet, uygulama ve dosya envanterini ayırır. Adobe’nin [precomp](https://helpx.adobe.com/after-effects/using/precomposing-nesting-pre-rendering.html) yaklaşımı bir düzenlenebilir alt kompozisyonu üst timeline’da tek öğe olarak sunar. FrameShift’te var olan `Precomp` yeteneğini bir yazar deneyimine çevir: seçimden alt kompozisyon, açık giriş/çıkış, zaman aralığı ve render sırası; alt kompozisyonu aç ve değiştir. Her kutulama işlemi render eşdeğeri değildir: grup sınırını kesen matte, 3D kamera, blend veya post-FX bağımlılığı varsa v1’de işlem uyarı ile reddedilsin.

| Şablon parçası | Yetkili olduğu konu | Örnek |
| --- | --- | --- |
| Çalışan örnek proje ve alt grafikleri | Render, keyframe, olay ve bağların gerçeği | `Impact Cut`, `Character Behind Text`, `3D Lyric Stack` |
| Tipli slotlar + varsayılanlar | Kullanıcının güvenli olarak değiştirebileceği değerler | Klip/ses/font; drop tick’i; başlık; etki şiddeti |
| Stil kartı ve örnek video | Niyet, referans, yaratıcı sınır; doğrulama değil | “Yüzü kapatma; impact 1–2 kare”; test edilmiş kaynak bilgisi |
| Manifest | Sürüm, bağımlılıklar, hash, lisans ve yeniden dağıtım durumu | Font ayrı kayıt; lisanssız referans klip paket dışı |

**Kabul:** İki farklı medya setiyle aynı şablon bağımsız kopyalara açılır. Bir slot eksikse render başlamaz; biri değiştiğinde diğer proje etkilenmez. Düz bir uygun sahneyi precomp’a alıp geri açınca seçili kareler korunur; sınırı kesen bağımlılıkta net hata çıkar. Paketi doğrulamak, önizleme eşdeğerliğini veya lisans uygunluğunu tek başına kanıtlamaz. İlk sürümde yalnız yerel ve denetlenmiş paketler; otomatik script/shader çalıştırma yok.

## 4. P2 — Büyüme, kalite ve dağıtım

| Özellik | Tasarım ilkesi | Neden sonra? | Başarı kanıtı |
| --- | --- | --- | --- |
| Proxy ve kare önbelleği görünümü | Önizleme kalite/proxy seçimini göster; son revizyonun geçerli karelerini işaretle. Varlık, bağımlılık, zaman ve profil değişince doğru alanı geçersizleştir. | Canlı scrub’ı ölçmeden disk önbelleği boyutlandırmak tahmin olur. | Soğuk/sıcak scrub p50/p95 ayrı; VFR kaynak tick’i, matte koordinatı ve olay zamanında semantik sapma yok. |
| Efekt rafı ve okunabilir parametreler | Mevcut efektleri giriş aralığı, birim, varsayılan ve uyumlu node ile keşfet; önce/sonra göster; solo/bypass açık seçim. | Yeni efektlerden önce var olanların bulunabilirliği. | Bir kullanıcı impact preset’ini WGSL yazmadan ekler ve geri alır. Yanlış giriş veya bilinmeyen parametre açık hata verir. |
| İfade görünümü ve görsel bağlar | `SafeExpr` yanında tür, birim, çözülmüş değer ve olay bağlantısı göster; sık işlemler için tipli kontrol. | JavaScript benzeri daha büyük dil, hata yüzeyini artırır. | Geçersiz ifade yerine base değere sessiz dönüş olmaz; seçili tick’in çözümü raporla eşleşir. |
| İnceleme ve paket taşınabilirliği | Kareye/aralığa yorum, revizyon kimliği, medya yeniden bağlama ve lisanslı dışa paylaşım. | Önce aynı makinede sabit sürüm export güvenilir olmalı. | Farklı dizine açılan paket referanslarını bulur; eski yorum güncel revizyonmuş gibi sunulmaz. |
| Birden çok çıktı ve yeniden deneme | Kompozisyon render’ı ile codec/kap ayarını ayır; birden çok oran veya kodlayıcı ancak maliyeti ölçülünce. | 9:16 statik reframe bir çıktı profili; otomatik kadraj takipçisi ayrı problem. | Çıktı kimliği, tekrar üretim ve iptal testi; aynı render’ın paylaşılıp paylaşılmadığı ölçülür, varsayılmaz. |

Adobe’nin [bellek ve önbellek](https://helpx.adobe.com/after-effects/using/memory-storage1.html), [efekt ve hazır ayar](https://helpx.adobe.com/after-effects/using/effects-animation-presets-overview.html) ve [ifade](https://helpx.adobe.com/after-effects/using/expression-basics.html) belgelerinden üç ayrım çıkar:

- **Proxy alternatif medyadır; cache hesaplanmış sonuçtur.** İkisini ayrı göster, varsayılan finalde orijinali kullan. Eski kareyi hızlı sunmak yerine güvenilir biçimde yeniden hesapla.
- **Preset düzenlenebilir bir tarif, ifade bir parametre hesabıdır.** Preset hangi efektleri ve zaman ofsetlerini uygulayacağını işlem öncesinde gösterir. Özellik kontrolünde taban ve değerlendirilmiş değer yan yana okunur.
- **Aynı kare akışının iki kodlayıcıya verilmesi**, oran veya renk işlem hattı farklıysa ücretsiz değildir. İlk optimizasyon deneyinde yalnız aynı render ayarından iki kodlama çıktısı al; kompozisyon geçişi sayısını ölç.

## 5. Uygulama sırası ve ölçüm planı

| Dilim | Teslim | Devam kapısı |
| --- | --- | --- |
| **0 — Güvenilir taban** | `autocut` sınır kararı + regresyon testleri; CLI örneği; mevcut iş akışında görev süresi ve müdahale sayısı | Çalışma kopyasında test/clippy/uygun render kapıları çalışır; sonuçlar eski koşudan ayrı raporlanır. |
| **1 — Dikine ürün kesiti** | Impact Cut örneği, canlı timeline ve önizleme, ortak komutlar, revizyon çatışması, sürümden MP4 | Klip al → ajan taslağı → bir elle düzeltme → undo → sabit export; aynı kullanıcı tarafından baştan sona tamamlanır. |
| **2 — AMV üretim hızlandırıcıları** | Kaynak/çıktı hız grafiği; metin seçici; matte girişinin ilk dilimi | Asta impact testi ve Mushoku seçili sahne testi; düğüm sayısı, elle müdahale ve sürtünme karşılaştırılır. |
| **3 — Yeniden kullanım** | Şablon/precomp akışı, önizleme bütçesi ve taşınabilir paket | İki medya seti, iki temiz kurulum ve kanıtlı yeniden üretim. |

**Ölçüm protokolü:** Mevcut CLI taban çizgisi ve yeni UI, aynı makine/GPU, aynı medya, aynı profil ve aynı görevle ölçülür. Kurulum dahil süre ile görev içi süre ayrı tutulur. Soğuk/sıcak koşu ve aygıt sınıfı kayda girer. Ortalama tek başına kullanılmaz; ilk kare ve scrub için p50/p95, bellek tepe değeri, başarısız iş sayısı ve manuel proje düzenleme sayısı raporlanır. Tek deneme performans genellemesine yetmez.

**Önerilen ilk hedefler (ölçülmedi):** Örnek açıldıktan sonra **5 dakikada ilk düzenlenebilir taslak**; timeline akışında **0 elle JSON düzenlemesi**; bir ajan teklifini **tek undo** ile geri alma; hız/olay/matte kaynak-zamanı için **0 sözleşme ihlali**. Scrub gecikmesi ve bellek bütçesi taban çizgisi alındıktan sonra cihaz sınıfına göre belirlenir. Piksel eşitliği yalnız aynı profil/backend ve deterministik fikstürde; farklı profiller için zaman/koordinat eşdeğerliği kontrol edilir.

### Uygulama katmanı ve iş bağımlılıkları

| İş | İlk dokunulacak alan | Bağımlılık / sınır |
| --- | --- | --- |
| Zamanlama ve hata tabanı | `frameshift-core` + `frameshift-cli` | Testleri yeşile döndür; örnek medya seçimi ve taban ölçümü paralel ilerleyebilir. |
| Önizleme ve timeline | `frameshift-app` denetleyicisi + mevcut GPU önizleme hattı | UI yalnız komutu ve tick’i gönderir; çizilen kare aynı oturumun revizyonuna bağlıdır. |
| Ajan fişi ve sabit export | `frameshift-mcp` + proje oturumu + iş yöneticisi | Medya/hash politikası belirlenmeden “snapshot” yalnız proje verisini korur; aynı komut grubu UI ve ajan için ortak. |
| Hız grafiği, tipografi ve matte | `frameshift-time` / `frameshift-schema` / `frameshift-vector` / `frameshift-render` | Hız grafiği motor eşlemesini okur. Metin ve matte ayrı fikstürlerle geliştirilebilir; sonra SC-0002 örneğinde birleştirilir. |
| Şablon ve precomp | Mevcut precomp, stil kartı ve şema doğrulama | Örnek varlık ve sürüm kuralları oturduktan sonra tipli slotları ve örneğe özel değerleri aç. |

### Karar bekleyenler ve kapsam dışı

- Canlı kabuk için pencereleme ve GUI teknolojisi seçimi ayrı onay ister; ilk geliştirme dilimi bir framework dayatmaz. Kod, bağımlılık ve kabul edilmiş ADR bu araştırmayla değişmedi.
- Metin seçicisinin kalıcı modeli ve matte giriş formatı ayrı tasarım kararı. Uyumluluk, onaylı şema sürümleme politikasına göre test edilir; önce eklemeli değişiklik tasarla. [Şema sürümü kararı](../decisions/0004-schema-minor-compatibility.md).
- Tam After Effects uyumluluğu, pazaryeri, bulut, işbirliği/live merge, yapay zekâ rotoskopi ve ses gain otomasyonu bu kesitin dışında. Mevcut fade-only kararı korunur.
- İlk ürün deneyi başarısızsa yeni shader’a kaçma. Kullanıcının hangi işte durduğunu ayır: kurulum, medya, zaman eşlemesi, önizleme, ajan revizyonu veya export.

**Önerilen başlangıç bileti:** Mevcut Asta sahnesinden lisanslı/sentetik bir Impact Cut fikstürü türet, `autocut` sınır davranışını netleştir ve bugün aynı fikstürle ilk taslak süresini ölç. Böylece canlı editör değişikliği sayıyla kıyaslanabilir.
