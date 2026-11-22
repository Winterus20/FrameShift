---
id: timebase
status: locked
created: 2026-11-21
updated: 2026-11-21
related_adr: [0002, 0003]
tags: [timebase, engine, media-io, schema]
---

# Zaman Tabanı ve Rasyonel Zaman Sözleşmesi (`timebase`)

## Amaç ve Kapsam

Bu sözleşme, FrameShift çekirdeğindeki (`frameshift-time`, `frameshift-schema`, `frameshift-media`, `frameshift-render`) tüm kalıcı zaman konumları, kare hızları, klip sınırları, hız eğrisi eşlemeleri, VFR medya PTS indekslemesi ve ses-video senkronizasyonu için tek bağlayıcı zaman modelini tanımlar.

[ADR-0002](../decisions/0002-rational-timebase-flicks.md) kararında açık bırakılan dört noktanın tamamı (NTSC `1000/1001` temsili, VFR PTS eşlemesi, ses örnekleme/retime sözleşmesi ve taşma koruması) bu belge ile kilitlenmiştir.

---

## Veri Modeli / Arayüz Sözleşmesi

### 1. Kalıcı Zaman Birimi: `Flicks(i64)`

- **Sabit:** `FLICKS_PER_SECOND: i64 = 705_600_000` (`2^9 × 3^2 × 5^5 × 7^2`).
- **Temsil:** Kalıcı proje modelinde, timeline sınırlarında, klip `in/out` noktalarında ve `event_id` ankrajlarında `f64` saniye kullanımı **yasaktır**. Tüm mutlak zaman ve süreler `Flicks(i64)` tam sayısı olarak saklanır.
- **Kapsam:** `i64::MAX` (`9_223_372_036_854_775_807` tick) yaklaşık `13_071_672_387` saniyeye (~414 yıl) karşılık gelir; pozitif ve negatif göreli ofsetler simetrik biçimde temsil edilir.

### 2. Rasyonel Kare Hızı: `Rational32` ve NTSC `1000/1001`

Timeline ve kaynak akış kare hızları kesirli oran olarak tanımlanır:

```rust
struct Rational32 {
    num: u32, // pay > 0
    den: u32, // payda > 0
}
```

Standart kare hızları `FLICKS_PER_SECOND` ile çarpılarak **sıfır kalanlı tam sayı tick/kare** değerine dönüşür (`(705_600_000 * den) / num`):

| Kare Hızı (`fps`) | `Rational32 { num, den }` | Kare Süresi (`Flicks`) |
| :--- | :---: | :---: |
| `24` | `24 / 1` | `29_400_000` |
| `23.976` (NTSC) | `24000 / 1001` | `29_429_400` |
| `25` (PAL) | `25 / 1` | `28_224_000` |
| `30` | `30 / 1` | `23_520_000` |
| `29.97` (NTSC) | `30000 / 1001` | `23_543_520` |
| `48` | `48 / 1` | `14_700_000` |
| `50` | `50 / 1` | `14_112_000` |
| `60` | `60 / 1` | `11_760_000` |
| `59.94` (NTSC) | `60000 / 1001` | `11_771_760` |
| `120` | `120 / 1` | `5_880_000` |

- Kare indeksi `n: i64` için kare başlangıç zamanı: `frame_start_tick(n) = (n as i128 * FLICKS_PER_SECOND as i128 * fps.den as i128) / (fps.num as i128)`.
- Bir `tick: Flicks` değerinin ait olduğu kare indeksi (taban yuvarlama): `floor_div(tick as i128 * fps.num as i128, FLICKS_PER_SECOND as i128 * fps.den as i128)`.

### 3. Taşma Korumalı Ara Aritmetik (`i128`)

- Herhangi bir ölçekleme, kare-tick dönüşümü, FFmpeg `AVRational` zaman tabanı çevrimi veya ses örneği dönüşümünde tüm çarpım ara adımları **`i128`** genişliğinde yürütülür; sonuç `i64` sınırına geri sığdırılırken `try_from` / `checked_*` ile taşma denetlenir.
- Yuvarlama gerektiren dönüşümlerde (ör. keyfi kaynak `time_base` veya ses örneği hizalama) kural açıkça belirtilir:
  - **Kare kapsamı / aktif kare bulma:** `floor` (aşağı/önceki kareye).
  - **En yakın örnek / PTS hizalama:** `round_half_even` veya tanımlı `nearest` kuralı (eşit uzaklıkta **daha erken** zaman damgası seçilir — deterministik tie-break).

### 4. Değişken Kare Hızlı (VFR) Medya PTS İndeksleme Sözleşmesi

- **İçe Aktarmada Zorunlu Transcode Yoktur:** VFR (ekran kaydı, telefon veya değişken kare hızlı anime) kaynaklar içe aktarılırken sabit kare hızına yeniden kodlanmaz.
- **PTS İndeks Tablosu (`VfrPtsIndex`):**
  - İçe aktarma / varlık tarama aşamasında FFmpeg akışının `time_base` (`tb_num / tb_den`) ve paket/kare `PTS` değerleri taranarak stream `start_time` referansına göre normalize edilir ve `i128` ara aritmetiğiyle artan sırada `Flicks(i64)` tablosuna dönüştürülür:
    - `pts_ticks: Flicks`
    - `duration_ticks: Option<Flicks>`
    - `is_keyframe: bool`
    - `raw_pts: i64`
  - Tekrar eden veya geri giden bozuk PTS girişleri doğrulama hatası veya deterministik sırayla işaretlenir.
- **Deterministik Eşleme (`source_tick -> nearest_or_active_pts`):**
  - İstenen `source_tick` için ikili arama (`partition_point`) uygulanır:
    - `source_tick < entries[0].pts_ticks` ise `entries[0]` döndürülür (klempleme; klip `in` sınırının dışındaysa üst katman boş/siyah veya hata politikasını uygular).
    - `entries[i].pts_ticks <= source_tick < entries[i + 1].pts_ticks` aralığında aktif kare **`entries[i]`** olarak seçilir (`active_pts` semantiği).
    - Opsiyonel `nearest` sorgusu istenirse `|source_tick - entries[i]|` ile `|entries[i+1] - source_tick|` karşılaştırılır; eşitlik halinde `entries[i]` (daha erken PTS) seçilir.
  - Decode edici, seçilen `entries[i]` için geriye dönük en yakın `is_keyframe == true` girişine seek eder ve `entries[i]` karesine ulaşana dek sıralı decode ederek **seek-independent** aynı kareyi üretir.

### 5. Ses Örnekleme ve Retime Sözleşmesi

- **Tam Bölünebilirlik:** Standart ses hızları `Flicks` ile tam bölünür:
  - `44_100 Hz` → `16_000` ticks / örnek
  - `48_000 Hz` → `14_700` ticks / örnek
  - `96_000 Hz` → `7_350` ticks / örnek
- **Ortak Dahili Ses Miks Formatı:** Motor ses işleme ve miksleme hattı **`48_000 Hz` (`14_700` ticks/örnek), `f32` lineer PCM** formatında çalışır. Farklı örnekleme oranlı kaynaklar (`44.1 kHz`, `96 kHz` vb.) `i128` sample-to-tick zaman tabanı üzerinden drift olmadan `48 kHz`'e örneklenir.
- **Retime (Hız Eğrisi) Sırasında Ses Davranışı (`AudioRetimeMode`):**
  Her sesli klip için davranış şemada açıkça beyan edilir; sessiz varsayım yapılmaz:
  1. `Mute` — Hız eğrisi uygulanan görsel klip sesini tamamen susturur (AMV/edit kliplerinde harici müzik track'i kullanılırken yaygın varsayılan).
  2. `Varispeed` — Ses hızı görsel hız eğrisinin integraliyle birebir eşlenir (perde/pitch hıza bağlı değişir); sürekli zaman eşlemesi `output_tick -> source_tick` üzerinden deterministik ara değerleme ile yapılır. Ters hız (`speed < 0`) desteklenirse ses ters yönde örneklenir; donma (`speed == 0`) aralığında sessizlik (`0.0`) üretilir.
  3. `PitchPreserving` — Faz vocoder / WSOLA tabanlı perde korumalı zaman ölçekleme (Faz 2/M2 kapsamı). M1 çekirdeği bu modu henüz uygulamamışsa sessizce `Varispeed`'e düşmez; açık **`UnsupportedAudioRetimeMode`** hatası döndürür.

### 6. Sürekli Değerlendirme Sınırı (Hız Eğrisi ve Motion Blur)

- Parçalı Bézier / Hermite hız eğrisi `v(t_out)` ve alt-kare temporal motion blur `t_i` örnekleri değerlendirme anında `f64` veya yüksek hassasiyetli sayısal integral ile hesaplanabilir; ancak:
  - Eğri kontrol noktalarının zaman konumları ve `event_id` ankrajları her zaman `Flicks(i64)` cinsinden saklanır (bkz. [ADR-0003](../decisions/0003-semantic-event-constraint-timing.md)).
  - Entegrasyon sonucu üretilen `source_tick`, kalıcı proje dosyasına yazılmaz; yalnızca değerlendirme anında `Flicks(i64)`'e yuvarlanarak VFR/CFR kare ve ses okuyucusuna iletilir.

---

## Kenar Durumlar ve Hata Davranışı

- **Geçersiz FPS (`num == 0` veya `den == 0`):** `InvalidFrameRate` hatası ile şema yükleme/mutasyon reddedilir.
- **Taşma (`i64` Overflow):** `i128` ara hesabının `i64::MIN..=i64::MAX` dışına taşması durumunda sessiz sarma (wrapping) yapılmaz; `TimeOverflow` hatası döndürülür.
- **Monotonik Olmayan / Boş VFR PTS İndeksi:** Boş akışta `EmptyMediaStream`, geri giden/tekrarlı PTS tespitinde deterministik indeks onarımı mümkün değilse `InvalidPtsIndex` hatası raporlanır.
- **Negatif veya Sıfır Süreli Klip Aralığı:** `output_end_tick <= output_start_tick` durumunda `InvalidTimeRange` hatası döndürülür.
- **Desteklenmeyen Ses Retime Modu:** Uygulanmamış bir retime modu (`PitchPreserving` M1'de istendiğinde) sessiz fallback yerine açık hata döndürür.

---

## Kabul Kriterleri (Kod Aşamasında Doğrulanacak)

- [ ] `24000/1001`, `30000/1001`, `60000/1001`, `24`, `25`, `30`, `50`, `60` fps ile `44.1 kHz / 48 kHz / 96 kHz` ses sınırlarında **10 saatlik** (`36_000` sn) zaman çizgisinde 0 tick / 0 örnek kümülatif kayma (drift) property testi geçer.
- [ ] `i64::MAX / 2` civarındaki sınır değerlerde yapılan FPS/sample dönüşümleri `i128` ara aritmetiğiyle taşmasız tamamlanır veya temiz `TimeOverflow` hatası verir.
- [ ] Sentetik bir VFR PTS tablosunda (değişken kare süreleri ve B-frame reorder senaryosu) rastgele `source_tick` sorguları hem ileri sıralı okumada hem doğrudan seek'te aynı PTS girişini döndürür.
- [ ] Geçersiz (`0/1`) FPS ve ters zaman aralığı (`end <= start`) girişleri açık hata döndürür.
