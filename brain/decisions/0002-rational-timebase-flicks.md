---
id: 0002
status: accepted
created: 2026-11-20
valid_from: 2026-11-21
tags: [timebase, engine, schema, media-io]
supersedes: null
---

# 0002 — Kesin Zaman Tabanı (Flicks / Rasyonel Tam Sayı Tick)

## Bağlam

Timeline konumları, klip sınırları, beat/impact ankrajları, speed ramp kaynak-zaman eşlemeleri ve ses-video senkronu bir video edit motorunun tüm katmanlarını keser. Zaman temsilinin ilk kodlamadan sonra değiştirilmesi tüm çekirdeği, proje şemasını ve testleri kıracak kadar maliyetlidir. Kayan nokta (`f64` saniye) temsili, `23.976` / `29.97` NTSC kare hızları ile `44.1 kHz / 48 kHz` ses sınırlarında birikimli yuvarlama ve 1-kare kayma hatalarına açıktır.

## Önerilen Karar

1. **Kalıcı Zaman Birimi:** Kalıcı proje modelinde, timeline sınırlarında, marker/event ankrajlarında ve ses-video hizalamasında `f64` saniye yerine tam sayı tick veya rasyonel kesir (`num / den`) kullanılır.
2. **Güçlü Başlangıç Adayı — Flicks (`1 sn = 705,600,000 tick`):**
   - `24, 25, 30, 48, 50, 60, 120 fps` kare hızlarına ve `44.1 kHz / 48 kHz` ses örnekleme oranlarına tam bölünür.
   - `1000/1001` NTSC ölçeği ile birlikte `23.976` ve `29.97` gibi oranları deterministik ifade etmeye adaydır.
3. **Ara Hesap Sınırı:** Hız eğrisi integrali, easing ve temporal motion blur alt-kare örnekleri gibi sürekli matematik işlemleri yalnızca değerlendirme anında ara `f64` hesaplaması olarak yapılır; kalıcı ankrajlar kesin zaman tabanında saklanır.

## Kabul Edilmeden Önce Kilitlenecek Açık Noktalar

Bu karar bilinçli olarak `proposed` durumundadır. `accepted` olmadan önce `brain/specs/` içinde şu 4 sözleşme netleştirilmelidir:
1. **NTSC `1000/1001` ve kesirli kare hızı temsili:** Saf Flicks tam sayısı mı yoksa `(tick, scale)` / genel rasyonel `Rational64` ile birleşik yapı mı kullanılacak?
2. **Değişken Kare Hızlı (VFR) Medya Eşlemesi:** İçe aktarılan VFR kliplerin PTS değerleri kesin zaman tabanına nasıl yuvarlanacak veya haritalanacak?
3. **Ses Örnekleme ve Retime Sözleşmesi:** Alt-örneklem düzeyinde retime ve `44.1 kHz` / `48 kHz` / `96 kHz` dönüşüm sınırları nasıl tanımlanacak?
4. **Overflow ve Süre Sınırı:** `i64` tick taşma sınırı ve proje maksimum süresi (Flicks ile `i64::MAX` ~414 yıl eder; çarpma ara adımlarında `i128` gereksinimi).

## Alternatifler

- **Genel Rasyonel (`Rational { num: i64, den: i64 }`):** Keyfi zaman tabanlarını kayıpsız taşır, ancak ortak payda büyüdüğünde sadeleştirme ve çarpma maliyetleri artar.
- **Sabit 90 kHz / 1 µs Tick:** `44.1 kHz` ses veya `24/48/60 fps` ortak çarpanında Flicks kadar temiz bölünmez.

## Referanslar

- [PROJECT_NOTES.md — Bölüm 4.1 Zaman Tabanı Kararı](../../PROJECT_NOTES.md#L81-L88)
- Meta Flicks referansı: https://github.com/facebookarchive/Flicks (koda geçmeden önce resmi teknik detay doğrulanacaktır)

## Onay ve Kilitleme Notu (2026-11-21)

- **Durum Güncellemesi:** Kullanıcı tarafından Faz 0 Hibrit Hızlı Yol kapsamında onaylandı (`valid_from: 2026-11-21`).
- **Kilitlenen Sözleşme:** Yukarıdaki 4 açık nokta (`NTSC 1000/1001` rasyonel temsili, `VFR PTS` indeksleme, `44.1/48/96 kHz` ses retime kuralları ve `i128` taşma korumalı ara aritmetik) [timebase.md](../specs/timebase.md) (`status: locked`) içinde kesinleştirilmiştir.
