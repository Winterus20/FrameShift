# FrameShift Tuzakları — SC-0002 Üretiminden Çıkarılan Dersler

**Tarih:** 2026-11-22
**Kapsam:** `local_media/mushoku/` altındaki kinetic-typography yeniden üretimi sırasında fiilen çarpılan hatalar.
**Amaç:** Aynı tuzaklara ikinci kez düşmemek. Her madde **belirti → kök neden → kural → nasıl yakalanır** biçiminde.

---

## 1. Söz–yazı senkronu (en pahalı tuzak)

| | |
| :--- | :--- |
| **Belirti** | Render teknik olarak doğru; ama izlerken yazılar vokalin önünde/arkasında kalıyor. |
| **Kök neden** | Kelime zamanları **2 fps contact sheet** üzerinden okundu. Her hücre 0.5 sn'lik bir pencereyi temsil eder, zaman etiketi pencerenin *başıdır* → sistematik ±0.25 sn hata. Üstüne kelime ömrü 1 vuruş (0.43 sn) yerine 0.6–1.4 sn yazıldı; kelimeler üst üste bindi ve "yazı erken gelip geç kaldı" hissi doğdu. |
| **Ölçülen gerçek** | Referansta kelime ömrü **≈ 0.45 sn (1 vuruş)**. `RAINBOW` 1.38 sn, `TOLD` 0.88 sn, `CASE` 1.10 sn gibi istisnalar var; geri kalanı tek vuruş. |
| **Kural** | Kelime zamanını contact sheet'ten okuma. Her kelime için **±0.60 sn'lik 7 karelik şerit** (0.20 sn adım) üret ve başlangıç/bitişi o şeritten oku. Kelime ömrünü vuruş ızgarasına oturt: `end ≈ start + 1 beat`. |
| **Yakalama** | Render sonrası **kendi çıktından** aynı şeridi üret ve referansla yan yana koy. |

### 1b. Kaçırılan ve uydurulan kelimeler
- `MY` (11.80–12.26) ve `KNEW` (28.42–28.92) tamamen gözden kaçtı.
- `TRADING` yanlış zamana kondu (16.35 yerine **18.16**).
- **Kök neden:** İlk okuma "beklediğim kelimeyi doğrulama" modundaydı; karelerde ne olduğu değil, ne beklediğim arandı.
- **Kural:** Kare şeridini okurken **ekranda ne varsa** yaz, beklenen kelimeyi arama. Şerit başına en az bir kez "burada hangi yazı var?" sorusunu sor.
- **Yakalama:** Kelime listesini referansın toplam süresine böl: 31.5 sn'de 32 kelime ≈ 0.98 sn/kelime. Belirgin sapma varsa liste eksik.

### 1c. Harf harf giriş hızı
- Referans: `RAINBOW` 7 harf, **~0.17 sn/harf**; `CASE` ~0.16 sn/harf; kısa kelimelerde 0.05 sn/harf.
- İlk sürümde 0.028 sn/harf kullanıldı → kelime "pat diye" belirdi, senkron hissi kayboldu.
- **Kural:** `stagger = clamp(word_duration / (n * 2.2), 0.03, 0.18)`; ayrıca `(n-1) * stagger < duration - 0.12` olmalı.

---

## 2. Tipografi yerleşimi

| Tuzak | Belirti | Kural |
| :--- | :--- | :--- |
| `x_offset_norm` / `y_offset_norm` birimi | Tüm metin ekran dışında / üst kenarda | Değerler **kare oranı**: 0.5 = merkez, 0 = sol/üst kenar. `font_size_norm` de kare yüksekliği oranı. |
| `tracking_em` çok küçük | Kelime referanstan dar, "küçük ve bitişik" görünüyor | Tek düğümlü kelimede `tracking_em ≈ 0.35–0.55`; 0.20 altı yetersiz. |
| Yayılım + font boyu çakışması | Uzun kelimede harfler üst üste (`REARRANGED`) | `gap = span_total/(n-1)`, `font_size = min(size*1.2, gap/0.54)`. 0.54 ≈ Anton ortalama harf genişliği (em). |
| Kenardan taşma | İlk/son harf kadraj dışında | `span_total ≤ 0.82`; harf merkezleri %9–91 aralığında kalsın. |
| Aynı metin iki kez | İki `YOU` birebir aynı sapmayla çıkıyor | Jitter seed'ine **kelime indeksini** de kat, sadece metni değil. |
| `vector_text`'te rotasyon yok | Harfler fazla "düz" | Harf başına `transform2_d` zinciri gerekir; maliyeti yüksek, ölçülü kullan. |

---

## 3. Renk ve alfa

| Tuzak | Belirti | Kural |
| :--- | :--- | :--- |
| `color_rgba` **lineer Rec.709** bekliyor | sRGB hex yazılınca metin lerleşip beyazımsı | `c/12.92` veya `((c+0.055)/1.055)^2.4` dönüşümü uygula. Şema sessiz kabul ediyor, uyarı yok. |
| Opak `solid` taban | Metin zinciri tüm kareyi siyah kaplıyor, video görünmüyor | Metin katmanını **şeffaf** `solid` (`[0,0,0,0]`) üzerine kur. |
| `mask_matte` girdi sırası | Maske ters çalışıyor | `inputs[0] = kaynak` (maskelenecek), `inputs[1] = matte`. |
| Luma maskesi global karartma | Tüm metin her sahnede soluk | Matte'i önce `color_basic(contrast≈2.6)` ile sıkıştır, sonra kısmi blend (`opacity≈0.58`) uygula. |
| Luma maskesi açık sahnede | Örtme hiç çalışmıyor (açık ten/giysi) | Luma tabanlı örtme bir **yaklaşıklama**; "referansla aynı" diye raporlama. Gerçek çözüm depth/matte girdisi. |

---

## 4. Kaynak medya

| Tuzak | Belirti | Kural |
| :--- | :--- | :--- |
| Siyah kaynak aralığı | Plan simsiyah render ediliyor, uyarı yok | `ffmpeg -vf blackdetect=d=0.12:pix_th=0.06` ile ön tara; `sources.json`'a `black` aralıkları yaz ve atla. |
| Karışık fps | 23.976 / 30 / 60 kaynaklar aynı zaman çizgisinde | `fps` alanını **kesir** olarak yaz (`{"num":24000,"den":1001}`), `fps: 23.976` yuvarlaması drift üretir. |
| Twixtor / yapay slow-motion paketler | Hız eğrisi doğal görünmüyor, kareler interpolasyonlu | Kaynak manifestinde işaretle; `speed > 1` kullan. |
| Altyazı/logo yanık klipler | Metin üstüne metin | Klip kabulünde `read_image` ile **görsel doğrulama** zorunlu. |

---

## 5. Motor ve ifade dili

| Tuzak | Belirti | Kural |
| :--- | :--- | :--- |
| Harf başına düğüm zorunluluğu | 30 kelime → 190 düğüm, her biri tam kare doku | Kelime sayısını ve harf sayısını önceden hesapla; bütçe ~250 düğüm. Uzun vadede `per_glyph` şeması gerekli. |
| `text` bindable değil | Daktilo / kelime değiştirme imkânsız | Her durum için ayrı düğüm kur. |
| `SafeExpr`'de karşılaştırma yok | `if`, `<`, `?:` yok; zarflar uzun `smoothstep` zinciri oluyor | Zinciri kısalt: tek `smoothstep` çifti + çarpım. Fuel sınırı 256; 6 kelimelik flash ifadesi sınıra yaklaşıyor. |
| `beat_phase` 120 BPM sabit | 139.7 BPM'lik şarkıda faz kayması | Vuruşları dışarıda hesapla, `t_sec` üzerinden mutlak zaman kullan. |
| Profil `motion_blur_sub_samples` | Profil 1 yazsa da "effective 2" basılıyor | Profil değerini tavan sanma; düğümün kendi `sub_samples` değeri geçerli. |

---

## 6. Epistemik tuzaklar

- **Contact sheet akıcılık kanıtı değildir.** Kare şeridi sahne içeriğini gösterir; titreşim, hareket sürekliliği ve ses senkronunu göstermez. Raporda "doğrulandı" yazma.
- **"Aynı edit" iddiası.** Kadraj, karakter ve renk içeriği farklıysa bu bir **stil eşleşme testidir**; birebir kopya değil.
- **Font kimliği tahmini.** Referansın fontu ölçülmedi; Anton benzeri olduğu görsel kıyasla söyleniyor.
- **Telif.** Referansın ses kanalı ve indirilen klipler yalnız yerel test içindir; dağıtım lisansı yok.

---

## 7. Hızlı kontrol listesi (yeni bir tipografi editi öncesi)

1. Vuruş ızgarasını spektral flux ile ölç, BPM'i yaz.
2. Kelime listesini **±0.6 sn / 7 kare** şeritleriyle doğrula; eksik kelime sayısını süreye bölerek kontrol et.
3. Kelime ömrünü 1 vuruşa oturt, stagger'ı kelime süresine göre sınırla.
4. Renkleri sRGB→lineer dönüştür; metin tabanını şeffaf yap.
5. `span_total`, `gap`, `font_size` üçlüsünü formülle bağla.
6. Kaynakları `blackdetect` ile tara; fps'i kesir yaz.
7. Render sonrası **kendi çıktından** senkron şeridi üret ve referansla karşılaştır.
8. Contact sheet'i yalnız kompozisyon kanıtı olarak sun.
