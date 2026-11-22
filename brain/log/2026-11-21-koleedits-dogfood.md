# Oturum Günlüğü — 2026-11-21: Koleedits TikTok benzeri edit dogfood

- **Tarih:** 2026-11-21
- **Kapsam:** Tek bir TikTok referansını önce kare kare ve ses zarfıyla incelemek, sonra yalnızca FrameShift ile benzer teknikte kısa bir edit render etmek. OpenMotion kullanılmadı. Yeni Cargo bağımlılığı yok. Git commit yok.

---

## Referansta görülen (kareler)

Kaynak: `https://www.tiktok.com/@koleedits/video/7700201309508603137`  
Dosya: `target/dogfood/koleedits/reference/ref.mp4`  
ffprobe: HEVC Main, 1400×1080 (DAR 35:27), 20 fps, 331 kare, 16.550 sn. Ses HE-AACv2, 44.1 kHz stereo. 2 fps şerit (`reference/frames/t_000.png` … `t_032.png`) ve vuruş anı kareleri (`reference/hits/`) izlendi.

Wuthering Waves sinematik AMV. Yinlin (kızıl saç) ve maskeli düşmanlar; açılışta sisli sokak, sonra şapka altından göz yakın planı. 9:16 değil. Sabit `KOLEEDITSS` filigranı. Soğuk mavi gölge, kırmızı enerji, ağır hareket bulanıklığı.

2 fps şerit (kare = 0.5 sn):

| Zaman | Kare |
| :--- | :--- |
| 0.0 | Sisli sokak, fener, pankart |
| 0.5 | Göz / profil yakın plan |
| 1.0 | 2 fps örneği siyaha denk geldi; 20 fps şerit (`burst_drop.png`, 0.55–1.55) bunun tek karelik bir kesik olduğunu gösteriyor |
| 1.5–2.5 | Kırmızı halka / ışın dövüşü, sonra havada takla |
| 3.0–3.5 | Enerji çukurunda kromatik yazı kartı (ekranda "HATE" → "HATE ON ME") |
| 3.7 | Kırmızı/beyaz whip bulanıklığı |
| 4.0 | Net kahraman pozu |
| 5.0–6.5 | "THINK" kartı, eğik darbe, hız çizgisi / bokeh patlaması |
| 8.0–11.5 | Aynı plan dizisinin ikinci turu; yazı kartı yine çukurda, en büyük sub vuruş 11.00'de eğik motion-blur darbede |
| 12.05 | Beyaz zoom flaş |
| 12.1–16.5 | Görüntü siyah. 2 fps örneklerinin hepsi siyah |

## Referansta duyulan (zarf, hoparlör değil)

`reference/ref_audio.wav` (PCM 44.1 kHz) ve `reference/waveform.png`. 10 ms RMS, 40 ms pencere; alçak <140 Hz, orta 300–2500 Hz, yüksek >3 kHz. Otokorelasyon zayıf (en iyi skor ~0.31, 165–169 BPM adayları birbirine yapışık). Sabit BPM iddia edilmez. Vokal cümle + tekme; ızgara değil.

Vuruş → görüntü (ölçülen tepe, sonra o andaki kare):

| Zaman | Ses | Görüntü |
| :--- | :--- | :--- |
| 0.21 | Dosyanın en keskin yüksek-bant transienti | Geniş plandan göz yakın planına kırılma |
| 0.38 | Orta+yüksek, daha kısık | Göz yakın planı duruyor |
| 0.92 | Tam bant ve sub'ın en yükseği (rms 0.36) | Whip / zoom ile dövüş planına giriş. Siyah değil |
| ~1.10 ve ~1.31 | İkincil tepeler | 20 fps şeritte 1–2 kare siyah kesik, sonra aksiyon devam |
| 2.4–2.6 | Şişme (rel ~0.83) | Havada çarpışma |
| 3.0–3.5 | Çukur (3.0 sn rel ~0.25) | Yazı kartı burada duruyor, tekmede değil |
| 3.75 | Sub rel 0.83 | Kırmızı whip; ~3.9–4.0'ta net poz |
| 6.50 | İkinci en yüksek tam bant (rel 0.94), sub değil | Bokeh / zoom bulanıklığı |
| 8.5–9.1 | İkinci nefes (9.0 sn rel 0.16) | Yine yazı kartı |
| 9.3–9.4 | Tam bant rel 0.83 | Whip, ardından kahraman |
| 10.45 | Yüksek bant rel 0.93 | Kıvılcım / saldırı başlangıcı |
| 11.00 | Dosyanın en yüksek sub'ı | Eğik motion-blur darbe |
| 12.05 | Tam bant rel 0.75 + yüksek rel 0.82 | Beyaz zoom flaş, sonra sert siyah |
| 13–16.5 | Gürültü tabanı (rel ~0.08) | Siyah tutuş. Uzun bir ikinci drop yok |

## İndirilen medya

Müzik, referansın şarkısı değil. Aynı *tür* vuruş (sessizlik, tek büyük vuruş, çukur, ikinci küme, kuyruk) için kısa bir trailer cue kesildi.

| Dosya | Kaynak | Ne |
| :--- | :--- | :--- |
| `media/raw_carlotta.mp4` | `https://www.youtube.com/watch?v=gsEQoQ7srBs` 00:22–00:36 | AV1 1280×720 30 fps, ~14 sn |
| `media/raw_jingran.mp4` | `https://www.youtube.com/watch?v=AYOqDVuCKMk` 00:28–00:42 | AV1 1280×720, ~14 sn |
| `media/raw_cartethyia.mp4` | `https://www.youtube.com/watch?v=P9dUQPIEdFs` 00:30–00:46 | AV1 1280×590, ~16 sn. Kullanılan plan açılıştaki süzülen geniş plan |
| `media/raw_teaser.m4a` | `https://www.youtube.com/watch?v=td7OY10bvZs` | AAC 44.1 kHz, 32.07 sn. "Cinematic Teaser Music No Copyright \| 30 Seconds Short BGM" |

Cue'nun kendi tepeleri (10 ms RMS): ~1.2 sn'ye kadar neredeyse sessiz, 1.67 en yüksek (rel 1.0, sub da 1.0), 1.91 ikincil, 3.5–4.2 çukur (4.0 rel 0.21), 4.58 cümle, 5.47 ve 5.6–5.84 küme, 8.0'da rel 0.23 çukur, 8.57'de döngü yeniden başlıyor. Edit 0–8.40 sn. Bu aralık vokal değil; ~4 sn'lik trailer cümleleri. Kesikler bu tepelere kondu, eşit aralığa değil.

Kullanılan planlar ffmpeg ile 960×740 cover-crop, H.264 yuv420p, 30 fps: `clip_face.mp4` (1.6 sn, süzülüş), `clip_look.mp4` (1.3 sn, el detayı; alttaki oyun altyazısı kırpıldı), `clip_burst.mp4` (3.4 sn), `clip_hero.mp4` (1.5 sn), `clip_impact.mp4` (1.6 sn, mavi saçlı poz), `clip_hold.mp4` (1.6 sn, patlama), `clip_echo.mp4` (2.0 sn). Ses: `bgm_edit.wav`, PCM s16le, 48 kHz stereo, 8.40 sn.

## Render

Proje: `target/dogfood/koleedits/koleedits_project.json` (üretici `build_project.py`). Zaman tabanı Flicks. `schema_version` 0.1.0. 7 video klibi, hızlar sabit veya `cubic_hermite` (dash 2.2× → 0.95×). Whip, glitch (yazı penceresinde progress 0.035), `DRIFT` yazısı çukurda (3.30–4.46), siyah kesik 1.905–1.980 (LUT sonrası), beyaz flaş 7.86–8.05, 8.05'ten sonra grade sonrası siyah. Draft profili motion blur örnek sayısı 2 (profil, düğümdeki değeri eziyor).

```text
target\release\frameshift-cli.exe render target\dogfood\koleedits\koleedits_project.json --out target\dogfood\koleedits\koleedits_draft.mp4 --profile draft
```

Çalışma dizini depo kökü. Son geçiş **26.57 sn**. Çıktı: 960×740, 24 fps, 202 kare, 8.416667 sn, H.264 + AAC 48 kHz stereo, 7 180 428 bayt. İlk geçiş (siyah kesik LUT öncesindeyken) 13.6 sn sürmüştü; o kare koyu mordu, yeniden render edildi.

İzlenen çıktı kareleri: `out_contact.png` (2 fps, 6×3) ve `out_frames/`. 0.6 sn süzülüş, 1.67 sn magenta whip (okunur dash, referanstaki net kırmızı ışın değil), 1.95 sn düz siyah, 3.70 sn ortada okunur `DRIFT`, 4.8 sn kahraman pozu, 5.55 sn şok halkası + patlama, 7.92 sn beyaz, 8.2 sn siyah.

## Doğrulanmayan

Contact sheet ve tek kareler akıcılık veya ses-görüntü senkronu kanıtı değil. MP4 oynatılıp kulakla dinlenmedi. Hız rampasının kaynak kare ilerlemesi ölçülmedi. `cargo test` bu oturumda çalıştırılmadı; release ikili, kaynaklardan yeni değildi (`frameshift-cli.exe` 18:49, daha yeni `.rs` yok).
