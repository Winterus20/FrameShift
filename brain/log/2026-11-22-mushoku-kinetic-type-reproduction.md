# 2026-11-22 — SC-0002 Yeniden Üretimi: Mushoku Tensei Kinetic Typography AMV

**Görev:** Kullanıcının verdiği TikTok referansının (`@asgardski`, video 7672463186175544609) aynısını FrameShift ile üretmek; ardından eksikleri ve uygulamaya eklenmesi/geliştirilmesi gerekenleri raporlamak. Orkestrasyon: lead ajan; alt ajanlar: `media` (kaynak görüntü), `audit` (yetenek matrisi).

## 1. Referans analizi

- `uvx yt-dlp` ile indirildi: 1920×1080, 60 fps, **31.516667 sn**, HEVC + AAC 44.1 kHz stereo.
- Kare farkı analizi (64×36 gri, L1): **59 plan**, eşik 0.116 normalize.
- Spektral flux onset analizi (1024/256, 22050 Hz): vuruş aralığı **0.4295 sn ≈ 139.7 BPM**, ilk onset 0.07 sn.
- 2 fps / 4 fps / 8 fps kare ızgaraları görsel olarak incelendi (kullanıcı düzeltmesi: anime **Mushoku Tensei**, Frieren değil).
- 30 kelimelik tipografi zaman çizelgesi çıkarıldı (`local_media/mushoku/ref_spec.json`).

## 2. Üretim

- **Kaynak:** `media` alt ajanı 6 adet 1080p Mushoku Tensei klibi indirdi ve `read_image` ile doğruladı (`local_media/mushoku_src/src_01..06.mp4`, 15–18 sn, altyazı/logo yok). Kaynak URL'ler alt ajan raporunda.
- **Müzik:** referansın ses kanalı `local_media/mushoku/ref_track.wav` (48 kHz stereo) — yalnız yerel stil testi.
- **Proje:** `local_media/mushoku/mushoku_project.json` — 260 düğüm, 126 binding, 59 klip, 30 kelime, 190 harf düğümü.
- **Teknik kararlar:**
  - Harf başına `vector_text` düğümü + `x_offset_norm`/`y_offset_norm` (şemada harf dizisi yok).
  - Kelime ömrü plan ömründen uzun: tipografi tek katman, plan zincirinin üstünde.
  - Özne arkası örtme: `color_basic(contrast=2.6)` ile kontrastı şişirilmiş kopya luma maskesi + %58 `blend`. **Taklit**, gerçek matte değil.
  - Sahne parlaklığına göre kelime rengi (ffmpeg ile 59 karenin ortalama luması ölçülüp koyu/açık renk seçildi).
  - 16.40–18.20 sn arası desatürasyon (`color_basic.saturation` binding'i) — referanstaki S/B bölüm.
- **Çıktılar:** `mushoku_draft.mp4` (960×540, 1892 kare) ve `mushoku_final.mp4` (1920×1080, 1892 kare, 60 fps, AAC 48 kHz).

## 3. Bulunan gerçek sınırlar (özet)

1. **Harf bazlı yerleşim yok** → harf başına düğüm; 190 düğüm yalnız tipografi için.
2. **Metne özel chromatic split yok** → yalnız tam kare `film_optics.chromatic_edge`.
3. **Özne/depth matte yok** → imza teknik (kelimenin karakter arkasında kalması) yalnız luma hilesiyle taklit edilebiliyor.
4. **Kırpma/panel düğümü yok** → kolaj panel ekleri kurulamadı.
5. **Vuruş altyapısı yok** → `beat_phase` 120 BPM sabit; `peaks` kesme üretmiyor; 59 kesme dışarıda hesaplandı.
6. **`text` bindable değil** → daktilo/kelime değiştirme animasyonu yok.
7. **SafeExpr'te karşılaştırma/ternary yok** → 126 binding'in çoğu uzun `smoothstep` zinciri.
8. **sRGB→lineer tuzağı** → renkler lineer bekliyor; ilk render'da metin lerleşmiş çıktı.
9. **Siyah kare koruması yok** → kaynakta siyah aralık varsa plan simsiyah render ediliyor.

## 4. Doğrulama

- `ffprobe`: `mushoku_draft.mp4` 960×540, 60/1, 1892 kare, 31.533 sn, H.264 + AAC 48 kHz stereo.
- `ffprobe`: `mushoku_final.mp4` **1920×1080, 60/1, 1892 kare, 31.533 sn, 44.55 MB**, H.264 + AAC 48 kHz stereo, motion blur efektif 2 örnek. Proje: 366 düğüm, 179 binding, 59 klip, **32 kelime**.
- **Senkron doğrulaması:** `local_media/mushoku/sync_check_ref_vs_frameshift.png` — 6 kelime × 5 zaman noktası, REF/FS çift satır. Kelime başlangıçları aynı karede; kelime tamamlanması ~0.10–0.15 sn geç (harf stagger'ından).
- Karşılaştırma sayfası: `local_media/mushoku/comparison_ref_vs_frameshift.png` (sol: referans, sağ: FrameShift; 8 zaman noktası).
- Tekil kareler (`target/mk_probe/`) ve 2 sayfalık contact sheet görsel olarak incelendi; kelime zamanlaması, renk ve ölçek referansla karşılaştırıldı.
- **Doğrulanmayan:** akıcılık ve ses senkronu kesintisiz oynatımla test edilmedi; contact sheet akıcılık kanıtı değildir.

## 3b. Söz–yazı senkronu düzeltmesi (v2 → v4)

Kullanıcı "sözler ile yazılar arasındaki senkron kaçmış" dedi. Ölçüm ve düzeltme:

| Öğe | v1 (hatalı) | Ölçülen gerçek | v4 |
| :--- | :--- | :--- | :--- |
| Zamanlama kaynağı | 2 fps contact sheet (±0.25 sn) | — | ±0.60 sn'lik **7 karelik şerit**, 0.20 sn adım |
| Kelime ömrü | 0.6–1.4 sn | **≈ 0.45 sn (1 vuruş)** | 0.42–1.44 sn, çoğu 1 vuruş |
| Kelime sayısı | 30 | **32** | 32 (`MY` ve `KNEW` eklendi) |
| `TRADING` | 16.35 | **18.16** | 18.16 |
| Harf stagger | 0.028 sn (çok hızlı) | 0.03–0.20 sn, kelimeye göre | 0.02–0.20 sn |
| `CASE` yerleşimi | yayılım | **kompakt** | center |

Kalan sapma: harf girişi tamamlanması referanstan ~0.10–0.15 sn geç; başlangıçlar eşleşiyor. Doğrulama `target/mk_probe/sync_check.png` (REF/FS çift satır, 6 kelime × 5 zaman).

## 4b. Ölçek formülü (tekrar kullanılabilir)

Harf harf yayılan kelimede üst üste binmeyi önleyen formül:

```
span_total = clamp(tracking_em * (n - 1), 0.32, 0.82)   # kare genişliği oranı
gap        = span_total / (n - 1)
font_size  = min(size * 1.20, gap / 0.54)               # 0.54 ≈ Anton ortalama harf genişliği (em)
```

Merkez hizalı (tek düğüm) kelimelerde okunabilir yayılma için `tracking_em ≈ 0.35–0.55` gerekiyor; 0.20 altı referanstan belirgin şekilde dar kalıyor.

## 5. Sonraki adımlar

- Tuzak kataloğu: [research/2026-11-22-frameshift-tuzaklar.md](../research/2026-11-22-frameshift-tuzaklar.md).
- `research/2026-11-22-kinetic-typography-gap-analysis.md` içindeki P0/P1 maddeleri. En yüksek getiri: `per_glyph` dizisi ve depth/matte girdisi.
