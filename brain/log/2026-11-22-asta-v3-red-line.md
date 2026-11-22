# 2026-11-22 — Asta // Devil Union — Red Line (v3)

## Amaç
Workspace'teki gerçek Asta klipleri ve müzikle, önceki 15 sn'lik denemeden daha güçlü bir 20 sn AMV üretmek: 1080p60, downbeat'e oturan hızlı kesim, 3D tipografi ve anime FX zinciri.

## Kaynaklar
- `local_media/raw_asta_1.mp4` — 1920×1080, 60 fps, 25.20 sn
- `local_media/raw_asta_2.mp4` — 1920×1080, 60 fps, 48.03 sn
- `local_media/raw_asta_3.mp4` — 1920×1080, 30 fps, 36.23 sn (0–4 sn kanal introsu ve 31–35 sn outro kullanılmadı)
- `local_media/bgm_revenge.wav` — 48 kHz stereo; 25.85–45.85 sn dilimi `local_media/asta_v3/bgm_v3.wav` olarak fade'li alındı.

## Yapı
- **Süre:** 20.000 sn, 1200 kare @ 60 fps, 1920×1080.
- **Kesim:** 28 klip; çoğu 0.70 sn, hızlanan bölümde 0.35 sn, finalde 1.30 sn.
- **Bölümler:** 0–5.10 kırmızı çöküş / 5.10–7.50 breakdown / **7.50 drop** / 7.50–18.70 yükseliş / 18.70–20.00 altın final.
- **Müzik senkronu:** ölçülen drop 33.35 sn; ses 25.85 sn'den başladığı için edit zamanında tam **7.50 sn**. RMS ölçümü: 7.48 sn −19.1 dB → 7.50 sn −12.7 dB.
- **Ses fade:** 40 ms giriş, 19.15–20.00 sn çıkış (bgm_v3.wav içinde).

### Kesim tablosu (kaynak başlangıç / kullanılan kaynak)
| # | Klip | Kaynak | Kaynak başlangıç | Süre |
|---|---|---|---|---|
| 1 | a1_claw | raw1 | 0.10 | 0.61 |
| 2 | a2_teeth | raw1 | 2.05 | 0.49 |
| 3 | a3_run | raw1 | 4.15 | 0.86 (1.60→0.50 hız) |
| 4 | a4_crawl | raw1 | 6.10 | 0.52 |
| 5 | a5_hand | raw1 | 8.15 | 0.59 |
| 6 | a6_crack | raw1 | 9.30 | 0.70 |
| 7 | a7_wings | raw1 | 12.30 | 0.63 |
| 8 | a8_light | raw1 | 16.40 | 0.24 |
| 9 | a9_profile | raw2 | 18.10 | 0.48 |
| 10 | a10_eye | raw2 | 14.10 | 0.50 |
| 11 | b1_ring | raw2 | 0.40 | 0.68 (1.20→0.50) |
| 12 | b2_flash | raw2 | 2.10 | 0.59 |
| 13 | b3_blue | raw1 | 18.10 | 0.49 |
| 14 | b4_sword | raw3 | 22.00 | 0.56 |
| 15 | b5_fly | raw1 | 20.00 | 0.77 |
| 16 | b6_burst | raw2 | 33.20 | 0.49 |
| 17 | b7_flame | raw3 | 27.60 | 0.49 |
| 18 | b8_grin | raw1 | 22.00 | 0.55 (1.50→0.55) |
| 19 | b9_devil | raw2 | 36.20 | 0.56 |
| 20 | b10_stance | raw3 | 24.00 | 0.63 |
| 21 | b11_gold | raw2 | 30.40 | 0.49 |
| 22 | b12_face | raw2 | 39.00 | 0.49 |
| 23 | b13_flame2 | raw3 | 29.00 | 0.49 |
| 24 | b14_sky | raw1 | 21.00 | 0.70 |
| 25 | b15_shock | raw2 | 4.20 | 0.63 |
| 26 | b16_final | raw2 | 19.00 | 0.52 |
| 27 | b17_union | raw3 | 30.00 | 0.42 |
| 28 | b18_outro | raw1 | 23.05 | 0.24 (0.55→0.18) |

### Geçişler
`glitch_displace`, `whip_chromatic`, `directional_warp`, `cross_zoom`, `burn`, `dreamy_zoom`, `kaleidoscope`, `swirl`, `ripple`, `linear_blur` — 27 sınır; drop öncesi 0.30 sn `burn`, drop'ta 0.55 flash.

### 3D tipografi
- `ASTA` (DelaGothicOne, neon kırmızı, alt üçte bir)
- `NO MAGIC` (Anton, neon kırmızı-turuncu)
- `DEVIL UNION` (Unbounded VF, camgöbeği→magenta neon, üst bant)
- `ANTI-MAGIC` (Anton, altın neon, alt bant)
- `ASTA` final (DelaGothicOne, krom/altın, alt bant)

Her başlıkta kademeli pop, rot_y ve opaklık penceresi `param_bindings` ile `t_sec`'e bağlı.

### FX zinciri
`comp_3d` → `fx_sharpen` → `fx_shake` → `fx_shock` → `fx_impact` → `fx_glow` → `fx_optics` → `fx_grade` → `fx_mblur` → `out_final` (siyah üstüne fade-in 0–0.28 sn, fade-out 19.18–19.94 sn).
3D katman: 130 ember + 130 spark parçacığı, `scene_3d` (look_at, DoF) içinde; kamera nefes alma bağları `audio_rms` ve `t_sec` ile.

## Üretim ve Doğrulama
- Üretici: `local_media/asta_v3/build_project.py` → `local_media/asta_v3/asta_v3_project.json` (76 node, 66 binding, 28 video klip, 1 audio klip).
- Render: `target/release/frameshift-cli.exe render local_media/asta_v3/asta_v3_project.json --out local_media/asta_v3/asta_v3_final.mp4 --profile final`
- `ffprobe`: **20.000000 sn**, 1920×1080, 60/1 fps, H.264 + AAC 48 kHz stereo, 33.330.034 bayt (13.3 Mbps).
- Draft (640×360, 1200 kare) ve final (1920×1080, 1200 kare) başarıyla renderlandı; 0.5 sn aralıklı contact sheet'lerle kompozisyon kontrol edildi.
- Ses: drop 7.50 sn'de; giriş −18 dB → −7 dB, çıkış −5 dB → −35 dB.

## Epistemik Sınır
- Contact sheet'ler kompozisyon, kadraj ve tipografi okunabilirliğini kanıtlar; **akıcılık, tam ses senkronu ve öznel kurgu kalitesi** yalnızca videonun tamamı izlenerek değerlendirilebilir.
- Kaynak klipler 1 fps tarama ve nokta probe'larla seçildi; iki probe arasında kalan karelerde beklenmeyen içerik olabilir.
- `bgm_v3.wav` dilimi ve fade'ler ffmpeg ile önceden işlendi; proje bu dosyayı 0'dan çalar.

## Dosyalar
- `local_media/asta_v3/asta_v3_final.mp4` — teslim
- `local_media/asta_v3/asta_v3_project.json` — proje
- `local_media/asta_v3/build_project.py` — üretici
- `local_media/asta_v3/bgm_v3.wav` — fade'li 20 sn müzik
- `local_media/asta_v3/probe/f_sheet1.jpg`, `f_sheet2.jpg` — final contact sheet
