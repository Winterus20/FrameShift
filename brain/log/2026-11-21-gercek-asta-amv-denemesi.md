# Oturum Günlüğü — 2026-11-21: Gerçek Asta AMV Medyası ile Uçtan Uca Deneme ve Değerlendirme

- **Tarih:** 2026-11-21
- **Kapsam:** İnternetten indirilen gerçek Black Clover (Asta) klipleri ve 48 kHz phonk müzik ile FrameShift CLI / Render Graph / Shader / Inspection yeteneklerinin uçtan uca sınanması.

---

## Yapılan İşler

1. **`local_media/` Medya Hazırlığı (`yt-dlp` + `ffmpeg`):**
   - YouTube'dan 3 adet 1080p Asta klibi (`raw_asta_1.mp4`, `raw_asta_2.mp4`, `raw_asta_3.mp4`) ve 2 adet müzik (`bgm_phonk.wav`, `bgm_revenge.wav`) indirildi.
   - Sahne şeritleri görsel olarak incelenerek watermark/intro/outro olmayan 5 adet 1920x1080 60fps kesit (`clip_1.mp4` .. `clip_5.mp4`) ve beat-drop'a hizalı 48 kHz stereo `bgm_edit.wav` (11.50 sn) üretildi.
2. **Gerçek AMV Proje Kurgusu (`local_media/asta_amv_project.json`):**
   - **5 klip**, `cubic_hermite` hız rampaları, **9 adet** 10ms hassasiyetli ses kick/drop `EventAnchorDef` kaydı.
   - **4 farklı WGSL geçişi zincirlenmiş DAG:** `cross_zoom` -> `whip_chromatic` -> `glitch_displace` -> `directional_warp` (`SafeExpr` `smoothstep` ile geri dönüşsüz geçiş ilerlemesi).
   - **4 Anime Post-FX zinciri:** `MotionTileShake` (`event_delta_sec` ile tüm vuruşlarda zoom-punch + sarsıntı) -> `AnimeImpactFrame` -> `RadialShockwave` -> `HalftoneGlow` -> `ColorBasic` -> `Lut3D` (`anime_cyber_contrast`) -> `MaskMatte` sinematik letterbox -> `VectorText` (`ASTA // ANTI-MAGIC`, `DEVIL UNION`) -> `MotionBlurShutter` (180°).
3. **Çıktılar ve Doğrulama:**
   - `frameshift-cli render ... --profile draft --out local_media/asta_amv_draft.mp4`: **1280x720, 24fps, 264 kare, 11.00 sn, H.264 + 48 kHz AAC stereo (`9.15 MB`)**, render süresi **218.59 sn**.
   - `frameshift-cli inspect` ve `ui-snapshot` ile tüm inceleme paketleri (`contact_sheet.png`, `mask_pack.png`, `transition_pack.png`, `velocity_pack.png`, `crop_fullres.png`, `inspection_manifest.json`, `ui_snapshot.png`) üretildi ve görsel olarak doğrulandı.
   - `inspection_manifest.json` ses analizi `estimated_bpm = 124.95` ve beat peak'lerini 1–6 ms farkla doğruladı.

---

## Uygulama Değerlendirmesi (Gerçek Kullanımda Tespit Edilen Güçlü Yönler & Darboğazlar)

- **Güçlü Yönler:**
  - `Rgba16Float` Linear Rec.709 + premultiplied alpha + 15 WGSL shader kombinasyonu (özellikle `MotionTileShake`, `AnimeImpactFrame`, `RadialShockwave`, `HalftoneGlow` ve 4 geçiş) gerçek anime sahnelerinde temiz, patlamasız ve sinematik sonuç verdi.
  - `SafeExpr` + `EventBoundAnimation` + `CubicHermite` `SpeedCurve` mimarisi çoklu vuruşa senkron AMV kurgusunu deterministik biçimde taşıdı.
  - Ses beat algılayıcısı (`analyze_beats_and_energy`) gerçek müzikte 124.95 BPM ve 1–6 ms hassasiyetli onset tespiti yaptı.
- **Tespit Edilen Eksikler / İyileştirme Alanları:**
  1. **Video Decode Performansı (`frameshift-media/src/ffmpeg.rs`):** `decode_video_frame` her önbellekte olmayan kaynak kare için ayrı bir `ffmpeg` alt süreci başlattığından 11 saniyelik 720p24 (264 kare) draft render 218.6 saniye (~0.83 sn/kare) sürdü; sürekli decoder oturumu / FFmpeg C FFI veya pipe streaming olmadan uzun/1080p60 renderlar yavaş kalıyor.
  2. **Canlı `audio_rms` Bağlamı (`frameshift-render/src/compositor.rs`):** `ProjectAnimator::build_expr_context` içindeki `audio_rms` şu an gerçek ses dalga formundan değil, en yakın `event` merkezli sentetik 120 BPM zarfından hesaplanıyor.
  3. **Çoklu-Klip Geçiş Ergonomisi (`ShaderTransition` + `EventBoundAnimation`):** `EventBoundAnimation` penceresi bitince `base` değere döndüğü için sıralı geçiş `progress` değerleri `EventBoundAnimation` yerine `SafeExpr` (`smoothstep`) ile tutulmak zorunda kaldı; `Track` / zaman çizgisi katmanı yerine manuel zincirlenmiş `ShaderTransition` düğümleri kuruldu.
  4. **Letterbox ve `Lut3D` Sıralaması:** `MaskMatte` çıktı dışını `alpha=0` yaptığı için arka plana siyah `Solid` konulmazsa şeffaf/beyaz kenar riski doğuyor; ayrıca `Lut3D` sonrasına alınmazsa siyah barlar LUT tonlamasından etkilenip koyu laciverte kayıyor.
  5. **UI Snapshot Metin Çakışmaları (`frameshift-app`):** `ui_snapshot.png` üzerinde üst banttaki `24.00 FPS` / zaman kodu ve zaman çizelgesindeki saniye / event etiketleri uzun isimlerde üst üste bindi; `Node DAG Inspector` paneli 25 düğümlü gerçek projede yalnızca ilk ~11 düğümü ve ~4 binding'i sığdırabildi.
  6. **Tipografi Kalitesi (`frameshift-vector`):** Yerleşik `VectorText` bitmap/piksel stilinde font rasterizasyonu kullanıyor; modern AMV'ler için TTF/OTF (HarfBuzz/Swash veya Skia/Cosmic-text) tipografi desteği önemli bir sonraki adım.
