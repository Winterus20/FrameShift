---
title: Addison Rae — Fame is a Gun (3D Kinetik Tipografi Tam Özellikli Edit)
date: 2026-11-22
status: completed
tags: [render-graph, shaders, text-vector, schema, inspection]
---

# 2026-11-22 — Addison Rae — Fame is a Gun (3D Kinetik Tipografi Editi)

## Özet

`SC-0001` (PlatsAE 3D Neon Kinetic Typography) stil kartı ve FrameShift Faz 6.1 + Faz 6.2 motor yetenekleri kullanılarak **Addison Rae — Fame is a Gun** parçasının `"It makes me want it more"` pre-drop yükselişinden `"Fame is a gun..."` ve son `"I got a taste of the glamorous life"` nakarat bitişine kadar olan **32.00 sn**'lik bölümü için tam 3D kinetik tipografi editi üretildi ve video/ses bütünlüğü bizzat izlenerek doğrulandı.

## Yapılan İşler

1. **Medya İndirme, Kırpma ve Kelime Bazlı Hizalama:**
   - Resmi lyric videosu (`o6flxGrbmCw`) `uvx yt-dlp` ile 48 kHz stereo WAV olarak `local_media/fame_is_a_gun/full_song.wav` yoluna indirildi.
   - `faster-whisper` (`base.en`, `word_timestamps=True`) ve `scipy` düşük-bant kick tepe analizi (`~138 BPM`, vuruş aralığı `0.435s`) ile tüm sözlerin ve vuruşların milisaniye konumları çıkarıldı.
   - Şarkı `69.40s -> 101.40s` aralığından (`32.00s`, 48 kHz stereo, 0.04s fade-in + 0.75s fade-out) `local_media/fame_is_a_gun/fame_chorus_cut.wav` olarak kırpıldı ve dinlenerek doğrulandı.
2. **Proje Mimarisi (`local_media/fame_is_a_gun/fame_is_a_gun_project.json` — 109 düğüm, 356 `SafeExpr` bağlamı, 18 olay ankrajı):**
   - **Özel HDR WGSL Sahne (`bg_glam_stage`, 0 girdi):** 6 perdeye bölünmüş dinamik renk teması (Ultraviolet → Electric Hot-Pink/Cyan → Molten Solar Gold → Champagne Gold/Cyan → Hyper Rose-Violet → Supernova Pink-Gold), sonsuz 3D perspektif neon ızgara, 4 dönen paparazzi spot ışığı, lazer hedef nişangâhı (`FAME IS A GUN`), elmas starburst halkaları ve `event_delta_sec` ile genişleyen beat halkaları.
   - **3 Derinlikli 3D Parçacık Katmanı (`ParticleField3D` × 3):** 4 köşeli yıldızlar (150), elmaslar (100) ve `LOVE IS A DRUG` / `DREAM GIRL` / final perdelerinde devreye giren neon 3D kalpler (80), ortak 3D kamera hareketi + DoF bulanıklığıyla.
   - **Çift 3D Neon Şerit Katmanı (`RibbonTrail3D` × 2):** `audio_rms` ile dinamik trim-path ve ters fazlı 3D yörünge şeritleri.
   - **44 Adet Senkronize 3D Extruded TTF Tipografi Kartı (`Text3D`):** 4 farklı font (`impact.ttf`, `ariblk.ttf`, `BOD_BLAI.TTF`, `GOTHICB.TTF`), harf bazlı dalga/zıplama (`char_stagger_sec`, `char_bounce_amp`, `char_wave_freq`), 3D whip-in/orbit/recoil/vogue-sway koreografisi ve üst-alt çift satır derinlikli yerleşimler.
   - **Post-FX Zinciri:** 18 olaya kilitli `MotionTileShake` (kenar aynalamalı), 6 büyük düşüşte `RadialShockwave`, 4 ana düşüşte `AnimeImpactFrame` (hız çizgileri, beyazlatmasız `invert_amount=0`), kenar-güvenli `CustomWgsl` (`fx_prism_glam`: RGB prizma dağılımı + anamorfik lens flare şeritleri + paparazzi flaşları), 5-mip `DeepGlow`, `FilmOptics` (halasyon + 35mm gren + vinyet), `ColorBasic` beat flaşları, grade sonrası saf siyah giriş/çıkış geçişi ve kök düğümde `MotionBlurShutter` (`180°`, `sub_samples=3`, ACES-lite HDR tonemap).
3. **Görsel İnceleme ve v2 Cilalama:**
   - v1 önizleme karelerinde tespit edilen 3 sorun (yüksek HDR taban rengi nedeniyle beyazlaşma, uzun kelimelerin kenara yaklaşması ve `FilmOptics.chromatic_edge` + `invert_amount` kaynaklı kenar cyan çizgisi) v2'de tamamen giderildi.

## Doğrulama

- **Final Render:** `target\release\frameshift-cli.exe render local_media/fame_is_a_gun/fame_is_a_gun_project.json --out local_media/fame_is_a_gun/fame_is_a_gun_3d_kinetic_final.mp4 --profile final`
  - **Süre:** `131.87 sn` (1,920 kare × 3 motion-blur alt örneği = 5,760 GPU alt karesi).
  - **`ffprobe`:** `1280x720`, `H.264`, `60/1 fps`, `1920` kare + `AAC`, `48000 Hz`, stereo, `32.000000 sn`, `79.8 MB` (`19.95 Mbps`).
- **Doğrudan Video + Ses İzleme:** Final MP4'ten üretilen `verify_watch.mp4` ve 16 panelli `verify_contact_sheet.png` `read_file` ile doğrudan izlendi/dinlendi; tüm sözlerin vokal ataklarıyla hizalı olduğu, kenar taşması/cyan çizgi/beyazlaşma bulunmadığı doğrulandı.
