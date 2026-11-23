# 2026-11-23 — Sol Levante / Afterlight

- Görüntü: Akira Saitoh / Production I.G, [Sol Levante](https://commons.wikimedia.org/wiki/File:Sol_Levante.webm), © Netflix, [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). Klipler kısaltıldı, renkleri düzenlendi ve özgün ses çıkarıldı. Müzik: Lesiakower, [Dark, Dark Phonk](https://pixabay.com/music/beats-dark-dark-phonk-110069/).
- İndirilen kaynaklar üzerinden 23 plan, 132 BPM ve 4,25 sn impact kullanıldı. Ana çıktı Python/FFmpeg ile render edildi: 1280×720, 24 fps, 446 kare, 18,583 sn, H.264/AAC stereo. Video ve ses tam decode edildi, seçilen kareler incelendi. Çıktının 32–125 Hz RMS değeri impact öncesi 0,0060, hemen sonrası 0,1323 olarak ölçüldü.
- FrameShift CLI, kilitli Cargo bağımlılıklarıyla derlendi ve yazılım Vulkan adaptörüyle çalıştırıldı. Aynı 23 plan düzenlenebilir proje grafiğine taşındı. Impact, kaynak kare ankrajına ve bağlı flash/shake efektlerine bağlandı. Drop çevresindeki 5 sn aralık FrameShift ile 640×360, 24 fps, 120 kare, H.264/AAC olarak doğrulandı.
- Kod veya bağımlılık değişikliği yapılmadı. Tam workspace doğrulaması çalıştırılmadı.
- Kurguya özel retarget sınırı: Impact ankrajı gelen planın ilk kaynak karesinde. Cue’yu sonra taşıyan deneme, impact öncesinde sıfır oynatma hızı gerektirdiği için reddedildi. 4,25 sn sabit cue render edildi.
- Düzenlenebilir ZIP, 23 planı ve kaynak görüntü kesitlerini içeriyor. Müzik indirme bağlantısı ve yerleştirme yolu BENIOKU.md içinde.
- Ardından ZIP’teki aynı proje ve görüntü kesitleriyle tam FrameShift draft export tamamlandı. 18,583 sn, 640×360, 24 fps, 446 kare, H.264/AAC. Export manifesti ve tam video/ses decode başarılı. Bass eşiği 4,26 sn’de ölçüldü.
