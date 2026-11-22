---
title: Asta v3 — üretim engelleri ve eksik özellikler
date: 2026-11-22
status: active
tags: [engine, cli, media-io, schema, inspection, agent]
---

# Asta v3 — üretim engelleri ve eksik özellikler

**Kısa cevap:** Evet, altı yerde takıldım. Üçü gerçek motor eksiği, ikisi araç keşfi, biri benim workflow hatam. En can sıkıcısı, her kare probe'unun ayrı süreçte ~10 sn sürmesiydi.

## Gerçekten eksik olanlar

| Eksik | Bu editte neye mal oldu | Kanıt |
|---|---|---|
| **Audio fade / gain automation** | 20 sn müziğin giriş-çıkışını proje içinde yapamadım; ffmpeg'le `bgm_v3.wav` üretip asset yolunu değiştirdim | `AudioClipDef` yalnızca statik `gain_db` taşıyor: [project.rs:103](../../crates/frameshift-schema/src/project.rs#L103-L111) |
| **Particle burst / emission binding** | Vuruşlarda partikül patlaması yok; sadece `speed`'i `audio_rms`'e bağlayabildim | Bindable liste: [project.rs:414](../../crates/frameshift-schema/src/project.rs#L414-L429) — `count` ve `seed` statik |
| **Range render** | Sadece drop'u denemek için 20 sn'nin tamamını render ettim (draft ~2-3 dk, final ~6 dk) | `Render` alanları: [main.rs:34](../../crates/frameshift-cli/src/main.rs#L34-L38); usage: [main.rs:1216](../../crates/frameshift-cli/src/main.rs#L1216) |
| **Render progress / ETA** | 1200 kare boyunca tek satır çıktı yok; `Get-Process` CPU'suna bakarak bekledim | Render yalnızca son satırı basıyor: [main.rs:250](../../crates/frameshift-cli/src/main.rs#L250-L263) |
| **Çoklu kare, tek süreç** | 40 karelik grid dakikalar sürdü; kare başına ~10 sn, çoğu wgpu başlatma | 40 ayrı `frame` çağrısı; her biri yeni motor: [main.rs:273](../../crates/frameshift-cli/src/main.rs#L273-L274) |

## "Keşke şu olsa" dediklerim

- **Metin arkası scrim/backplate.** Finaldeki krom `ASTA`, parlak alev üzerinde zor okunuyor; outline + glow ile idare ettim. Blur'lu yuvarlak plaka veya otomatik kontrast gölgesi AMV tipografisini kurtarır.
- **9:16 akıllı reframe.** Şu an dikey versiyon için yeni proje + kamera/fit ayarı gerekir. `--aspect 9:16` ile konu takipli cover crop, Shorts/Reels için tek komut olurdu.
- **Ribbon renk/kalınlık kontrolü.** `ribbon_trail_3d`, 0.045–0.05 genişlikte bile beyaz tel gibi göründü; renkler additive HDR'de beyaza kaçtı. Bu yüzden şeritleri tamamen sildim.
- **Şekil/malzeme swatch galerisi.** `diamond_3d` + `material_mode 0` dev beyaz yıldız olarak renderlandı; dört şekli de kaldırmak zorunda kaldım. `inspect --materials` bunu önlerdi.
- **`autocut`.** `peaks` var ama kesim gridine bağlayan bir komut yok; 28 kesimin zamanlamasını elle kurdum.

## Benim kaçırdığım / araç keşfi

- **`peaks` zaten varmış.** [main.rs:57](../../crates/frameshift-cli/src/main.rs#L57) ve [main.rs:1121](../../crates/frameshift-cli/src/main.rs#L1121); `--write-events` ile event de yazıyor. Ben numpy ile drop ölçtüm.
- **`inspect` 3×2 contact sheet üretiyor** ([inspection.rs:91](../../crates/frameshift-core/src/inspection.rs#L91-L162)); 6 kare benim 40 karelik QC'ime yetmezdi ama başlangıç olarak kullanılabilirdi.
- **`frame --help` hata veriyor** ("unknown or unexpected argument '--help'") ve [AGENTS.md](../../AGENTS.md#L39) komut listesinde `peaks` yok. Bu yüzden araçları geç keşfettim.

## En çok fark yaratacak üç iş

1. **`probe`/`sheet` + `render --range`** — tek süreçte N kare; iterasyon 10–30× hızlanır.
2. **Audio fade/gain automation** — post-process dosyası olmadan.
3. **Progress + ETA** — "frame 600/1200, 3.2 fps, ETA 3:10".

Bunlardan birini implement edeyim mi? Bence ilk sıra `probe`/`sheet` + `--range`; her edit turunda aynı dakikaları yiyor.
