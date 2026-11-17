# HyperFrames alternatifleri: araştırma ve karar çerçevesi

## Sonuç ve kanıt sınırı

Mevcut harici anime klipleri/ses, programlanabilir kurgu, hız eğrileri, maskeler, temporal motion blur, agent kontrolü ve preview/export tutarlılığı birlikte değerlendirildi. HyperFrames'ten her yönde üstün olduğu doğrulanmış tek aday bulunmadı. Bu, böyle bir motorun bulunamayacağı iddiası değildir. Araştırma birincil belgeler ve hedefli kaynak kodu okumalarıdır; kurulum, test yürütme, render veya performans benchmark'ı yapılmadı. Henüz seçim yapılmadı.

Önerilen kısa liste: HyperFrames ve Remotion motor karşılaştırması; MasterSelects ise ayrı kategoride uygulama/fork adayı. Native seçenekler daha fazla entegrasyon gerektiren alternatifler. Çoklu motor mimarisi veya fork kullanıcı tarafından onaylanmadı.

## 1. Remotion

En ciddi karşılaştırmalı motor adayı. HTML/React kompozisyonları ve UI'sız render mevcut. Resmî değişken hız tarifi kaynak zamanını hızların birikimiyle hesaplıyor; yalnız playbackRate animasyonu yanlış. Tarif OffthreadVideo kullanıyor ve @remotion/media Video ile henüz çalışmadığını açıkça söylüyor.

HtmlInCanvasMotionBlur gerçek kesirli zaman örnekleri alıp ortalayan deneysel bileşen. Chrome ve HTML-in-canvas desteği/flag koşulları var; iç içe kullanım daha yeni Chromium ve Remotion sürümlerine bağlı. Bu yol, HyperFrames'in layered shader blur sınırına karşı araştırılmaya değer; fakat retimed kaynak video + hareketli maske + shader + temporal blur birleşiminin doğruluğu kanıtlanmadı. Bir blur API'sinin bulunması tek başına daha iyi motor kanıtı değil.

Lisans MIT değil. Bireyler ve uygun en fazla üç kişilik ekipler ticari/otomasyon kullanımlarında ücretsiz olabiliyor. Ücretsiz kategori dışında otomasyon/editör ürünü için güncel Company Automators fiyatı render başına 0.01 USD ve aylık minimum 100 USD. Dağıtım ve kullanıcı adına kod üretimi koşulları gerçek ürün modeliyle ayrıca incelenmeli; ekip büyüklüğü tek kriter değil.

Kaynaklar:
- https://www.remotion.dev/docs/videos/accelerated-video
- https://www.remotion.dev/docs/motion-blur/html-in-canvas-motion-blur
- https://www.remotion.dev/docs/html-in-canvas
- https://www.remotion.dev/docs/html-in-canvas-bugs
- https://www.remotion.dev/docs/media/video
- https://www.remotion.dev/docs/license/faq

## 2. MasterSelects

HyperFrames gibi bağımsız motor değil, WebGPU/WebCodecs tabanlı video/motion/node editörü ve fork adayı. Harici agent için kimlik doğrulamalı yerel bridge ve MCP adaptörü belgeli. Browser gerçek yürütme otoritesi; bridge açık editör sekmesine dayanıyor, bağımsız headless motor gibi değerlendirilmemeli.

Kodda signed speed integration, source-time warp, freeze ve reverse eşleme yolları görüldü. Node grafiği, maskeler ve programatik edit yüzeyi mimari olarak hedefimizle uyumlu. Ancak motion-blur dokümanı Length/Angle ile aynı resim üzerinde uzamsal smear anlatıyor; bütün grafiğin shutter boyunca yeniden değerlendirilmesi kanıtı değil. Bu yüzden anime görsel kalitesinde HyperFrames'ten üstün ilan edilmedi.

AGPL-3.0-only. Ticari kullanım yasak değil; dağıtım ve değiştirilmiş ağ hizmetinde kaynak sunma yükümlülükleri incelenmeli. Repo dışındaki private hosted kernel açık kaynak editörle aynı şey değil. Bunu fork'lamak hosted agent ürününün tümünü edinmek anlamına gelmez.

Kaynaklar:
- https://github.com/Sportinger/MasterSelects
- https://github.com/Sportinger/MasterSelects/blob/master/docs/Features/Node-Workspace.md
- https://github.com/Sportinger/MasterSelects/blob/master/docs/Features/AI-Bridge-Control.md
- https://github.com/Sportinger/MasterSelects/blob/master/docs/Features/effects/motion-blur.md
- https://github.com/Sportinger/MasterSelects/blob/master/src/services/timeline/retime/clipRetime.ts
- https://github.com/Sportinger/MasterSelects/blob/master/LICENSING.md

## 3. MLT / Movit

Gömülebilir native medya/timeline grafiği. timeremap içinde animasyonlu time_map ve speed_map; kaynakta hız entegrasyonu bulunuyor. Frame seçimi/blending, optical flow veya tüm kompozisyonun temporal supersampling'i değildir. Movit blur uzamsal blur; HyperFrames'in temporal kompozisyon kısıtlarını kendiliğinden çözmez.

Core LGPL-2.1; melt ve modüllerin lisansları ayrı. Movit blur GPLv2 olarak belgelenmiş. GPU/headless OpenGL context ve dağıtım ek iş ister. Native zamanlama omurgası için ciddi, hızlı web-agent ürün başlangıcı için otomatik daha iyi değil.

Kaynaklar:
- https://www.mltframework.org/docs/framework
- https://www.mltframework.org/plugins/LinkTimeremap
- https://www.mltframework.org/plugins/FilterMovit-blur
- https://www.mltframework.org/docs/copyrightpolicy

## 4. libopenshot

C++/binding tabanlı, JSON ve ApplyJsonDiff/GetFrame API'siyle agent kontrolüne uygun. Eğri tabanlı time mapping ve reverse için gerçek kaynak yolları var. Ancak incelenen blur efekti uzamsal; genel temporal shader compositor kanıtı yok. Qt/FFmpeg/audio ve binding paketleme yükü var. LGPL-3.0-or-later; masaüstü OpenShot uygulamasının lisansıyla karıştırılmamalı. Develop kaynağı kararlı sürümle eşit sayılmamalı.

Kaynak: https://github.com/OpenShot/libopenshot

## 5. Blender

Python ve background render ile ayrı worker olarak kullanılabilir. Mask Node animasyonlu maskelerde samples/shutter tabanlı blur sunuyor. Bu güçlü ve somut bir fark. Ancak 3D scene motion blur ayarı, VSE'deki her klip/transform/retime için otomatik temporal blur demek değil. Vector Blur için hazır anime klibinden kendiliğinden çıkmayan motion/depth verileri gerekir. Entegrasyon/paketleme ve GPL koşulları incelenmeli. Ana hafif motor yerine gerektiğinde ileri compositing worker'ı olarak değerlendirilebilir; şimdiden hibrit mimari kararı verilmedi.

Kaynaklar:
- https://docs.blender.org/manual/en/4.5/compositing/types/input/mask.html
- https://docs.blender.org/manual/en/4.5/compositing/types/filter/vector_blur.html
- https://www.blender.org/about/license

## 6. Revideo / Motion Canvas

Revideo MIT ve headless render API'si sunuyor; ancak incelenen Media kodunda reactive playbackRate kısıtı, audio export'ta sabit hız işleme ve preview/export farklı decode yolları var. Hazır, bütünleşik temporal blur doğrulanmadı. Midrender'ın ticari ürünündeki yeniliklerin açık kaynak Revideo'ya aktarılmış olduğu varsayılmamalı. Motion Canvas daha çok vektör/bilgilendirici animasyon odaklı. Bu ihtiyaç için HyperFrames'e karşı belirgin yükseltme kanıtlanmadı.

Kaynaklar:
- https://github.com/midrender/revideo
- https://github.com/midrender/revideo/blob/main/packages/2d/src/lib/components/Media.ts
- https://github.com/midrender/revideo/blob/main/packages/ffmpeg/src/generate-audio.ts
- https://midrender.com/revideo
- https://github.com/motion-canvas/motion-canvas

## 7. Diğer web adayları

- FreeCut: MIT, WebGPU/WebCodecs, çok katmanlı editör, efekt/maske/animasyon. İyi bir UI/efekt referansı; ana ihtiyacımız olan birleşik kaynak-time-ramp + temporal blur + agent-first headless iş akışında üstünlük doğrulanmadı. İncelenen README geniş özellik listesi performans/kalite kanıtı sayılmadı.
- Diffusion Studio Core: etkileşimli preview ve final export sunan TypeScript/WebCodecs motoru. GitHub metadata'sı MPL-2.0 derken README watermark kaldırmak için ücretli key belirtiyor. Kaynak lisansı, dağıtılan paket ve watermark şartları ayrı incelenmeden özgür/ücretsiz alternatif diye önerilmedi.
- WebAV: MIT WebCodecs SDK; daha düşük seviyeli medya bileşim altyapısı. Eksiksiz anime finishing motoru olduğu doğrulanmadı.
- Framewave: MIT, WebGPU ve temporal blur iddiası ilginç. README'de Node headless export hâlâ roadmap'te. Mevcut anime kaynak videolarının retime/maske iş akışı kanıtlanmadı; erken izleme adayı.
- Olive: alpha, headless export gövdesi tamamlanmamış kaynak yolları nedeniyle hazır üretim motoru olarak önerilmedi.

Kaynaklar:
- https://github.com/walterlow/freecut
- https://github.com/diffusionstudio/core
- https://github.com/WebAV-Tech/WebAV
- https://github.com/VFitter/framewave
- https://github.com/olive-editor/olive

## Karar ölçütü

Önce HyperFrames ile Remotion'da aynı kaynak medya, zaman haritası, hareketli maske, shader geçişi ve temporal blur içeren kısa bir bölüm. Rastgele seek ve final karelerini, ses senkronunu, fallback'leri, gerekli yamaları ve gerçek render maliyetini ölçmek. Gerekirse MasterSelects uygulama/fork değerlendirmesini ayrı yürütmek. Hiçbir motor için pazarlama demosundan anime kalite üstünlüğü çıkarılmamalı.
