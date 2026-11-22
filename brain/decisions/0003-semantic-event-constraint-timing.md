---
id: 0003
status: accepted
created: 2026-11-20
valid_from: 2026-11-20
tags: [agent, timebase, schema, inspection]
supersedes: null
---

# 0003 — Anlamsal Edit ve Olay/Kısıt Tabanlı Zamanlama Ayrımı

## Bağlam

Agent-first bir editörde kullanıcı niyeti ("yumruk tam drop anına otursun, karakter parlasın ve sonraki sahne sarsılsın") doğal dil ve estetik hedefler içerir. Eğer motor doğrudan doğal dil yorumlamaya çalışırsa determinizm kaybolur; tersine agent yalnızca ham saniye/frame sayısı yazmak zorunda kalırsa tek bir impact anı kaydığında bağlı tüm animasyonlar ve hız eğrileri bozulur.

## Karar

1. **Kesin Sorumluluk Ayrımı:**
   - **Agent (Planlama ve Niyet Çevirisi):** Doğal dil isteğini, görsel/işitsel analizi ve yaratıcı hedefleri açık kaynak/hedef işaretlerine, parametrelere ve ölçülebilir kısıtlara dönüştürür.
   - **Motor (Deterministik Hesaplama):** Kaynak-zaman haritasını, olay bağımlılıklarını ve teknik geçerliliği deterministik biçimde hesaplar; doğal dil veya estetik yargı üretmez.
2. **İlk Odak — Kaynak Impact İşaretini Hedef Zamana Hizalama:**
   - Klipte kaynak anı işaretlenir; müzikte hedef beat/drop zamanı seçilir (otomatik tespit ile kullanıcı işareti ayırt edilir).
   - Agent kullanılabilir kaynak aralığını, çıktı aralığını, hız sınırlarını ve önerilen hız eğrisini verir.
   - Motor hedef çıktı anında kaynak-zaman eşlemesinin kaynak impact işaretine oturmasını sağlar.
3. **Olay Kimliğine Bağlı Animasyonlar:** Glow, flash, shake veya yazı sabit bir saniyeye değil, bir olay kimliğine (`event_id`) zaman ofsetiyle bağlanabilir. Olay çıktıda taşındığında bağlı animasyonlar yeniden hesaplanır; ilgisiz keyframe'ler sessizce kaydırılmaz. Kaynak görüntüyü izleyen maske/tracking verisi kaynak zamanı üzerinden değerlendirilir.
4. **Sessiz Bozulma Yasağı:** Yetersiz kaynak aralığı veya hız sınırı çakışmasında motor sessizce aralığı/hızı değiştirmez; ihlal edilen koşulları ve alternatif seçenekleri raporlar.
5. **Aşamalı Kapsam:** İlk aşamada genel amaçlı evrensel bir kısıt çözücü yerine kaynak işaretini hedef zamana hizalama ve açıkça bağlı efektleri taşıma hedeflenir.

## Gerekçe

- Revizyon döngülerinde zamanlama değiştiğinde tüm bağlı efektlerin elden yeniden hesaplanması agent hatalarını büyütür.
- Deterministik motor + yaratıcı agent ayrımı, "motor yetersizliği" ile "agent planlama hatası"nın ilk doğrulama hedefinde temizce ayrılmasını sağlar (bkz. [PROJECT_NOTES.md](../../PROJECT_NOTES.md#L191-L193)).

## Sonuçlar ve Kısıtlar

- **Tasarım hedefidir:** Çalışan bir API veya çözücü henüz uygulanmamıştır.
- **Doğrulama katmanları ayrıdır:** Kaynak marker tespiti, motorun kısıtı uygulaması ve final görüntünün estetik değerlendirmesi ayrı doğrulama yüzeyleri olarak kalacaktır.
