# FrameShift — Referans Stil Kartları (`brain/style-cards/`)

Bu klasör, kullanıcının beğendiği referans editlerden çıkarılan **yapılandırılmış görsel stil, kurgu dili ve efekt reçetelerini** tutar. Amaç:
1. Farklı edit türlerinden (anime AMV, 3D kinetik tipografi, kolaj, HUD/yoğun kompozisyon vb.) öğrenilen teknikleri kalıcı ve tekrar kullanılabilir kartlara dönüştürmek.
2. Kullanıcı ileride referans video vermeden *"şu karaktere / şu şarkıya edit yap"* dediğinde, ajanların bu kartlara bakarak kullanıcının kalite çıtasına ve görsel zevkine uygun, tekdüze veya aşırı basit olmayan editler üretmesini sağlamak.
3. Hangi motor özelliklerinin birden fazla beğenilen stilde ortak ihtiyaç olarak tekrar ettiğini görüp yol haritasını gerçek talebe göre önceliklendirmek.

---

## Stil Kartları İndeksi (Hızlı Seçim Matrisi)

| ID | Stil Adı | Kategori | Enerji | Güncel Motor Hazırlığı | Ne Zaman Seçilmeli? |
| :--- | :--- | :--- | :--- | :--- | :--- |
| [SC-0001](SC-0001-platsae-3d-kinetic-typography.md) | PlatsAE 3D Neon Kinetic Typography | `kinetic-typography` | `high` | `partial` *(headless hazır: Faz 6.3/6.5/6.6 + AI Lyrics + P1; P4 GUI yok)* | Hızlı pop/EDM/phonk vokalli şarkılarda %100 söz-odaklı 3D kinetik tipografi veya yüksek enerjili intro/chorus sekansları |
| [SC-0002](SC-0002-mushoku-tensei-kinetic-type-amv.md) | Mushoku Tensei Full-Frame Kinetic Typography AMV | `kinetic-typography` | `high` | `partial` *(ISNet/Depth matte & P1 `ShapeMask` var; per-unit 2D rotasyon kısmi)* | Orta/yavaş tempolu vokalli şarkılarda dev, harf harf yayılmış, özne arkasında kalan 2D tipografi; 1 vuruş = 1 plan kurgu |
| [SC-0003](SC-0003-theamirhanio-curse-tiktok-edit.md) | Curse — efekt odaklı TikTok anime edit | `anime-amv` | `high` | `partial` *(headless hazır: `BlurMoCurves`, `AdjustmentLayer`, 60 WGSL; P4 GUI yok)* | Karakter motifi, siyah bekleme, kısa bulanık giriş ve ışık vuruşları etrafında kurulan sosyal editler |
| [SC-0004](SC-0004-xenoz-flow-impact-amv.md) | Xenoz Ekolü — Flow & Hard Impact AMV | `anime-amv` | `high` | `partial` *(headless hazır: RIFE, `OpticalFlowBlur`, `AnimeSnap`, `AudioBus`; P4 GUI yok)* | Aksiyon akışı, anime snap hız eğrileri, yönlü geçişler, impact shake ve çok katmanlı SFX ile yüksek enerjili AMV'ler (arketip) |
| [SC-0005](SC-0005-nep-one-framer-jugg-edit.md) | Nep / Jugg Ekolü — One-Framer & Velocity Edit | `anime-amv` | `high` | `partial` *(headless hazır: `Flicks`, RIFE, `OpticalFlowBlur`, `AnimeImpactFrame`; P4 GUI yok)* | Hızlı transient odaklı one-framer flaşlar, stutter, sert bounce ve yoğun kinetik metin içeren kısa sosyal editler (arketip) |
| [SC-0006](SC-0006-okotcu-dark-glow-anime-flow.md) | @_okotcu Dark-Glow Anime Flow & Impact | `anime-amv` | `high` | `ready` *(1:1 60fps, `OpticalFlowBlur`, `DeepGlow`, `WhipChromatic`, `AgxPunchy`)* | 1:1 kare formatta yüksek kontrastlı neon mor/kızıl glow, göz yakın planları, directional whip/blur ve saf görsel akış odaklı editler |
| [SC-0007](SC-0007-mysticreign-makima-comprehend.md) | Mysticreign Makima — Subtitle-Led Graveyard Orbit & Typewriter Drop | `anime-amv` | `high` | `ready` *(1:1 30fps, `VectorText` kelime/harf animator, `DeepGlow`, `FilmOptics`, `BlurMoCurves`)* | Sakin/otoriter karakterlerde kelime kelime altyazı, tek orbit açılış, 120 BPM drop'ta 31 hard-cut ve koyu bordo kalp/`CONTROL` tipografisi isteyen 13–14 sn'lik editler |

*(Eski serbest formatlı ilk 4 referans notu kök dizindeki [VISUAL_REFERENCES.md](../../VISUAL_REFERENCES.md) dosyasında korunmaktadır; istendiğinde onlar da `SC-0006+` kartlarına dönüştürülebilir.)*

> **Tarihsel Kart Notu:** `SC-0001..SC-0005` dosyalarının kendi içindeki "Eksik / Faz 6.3 hedefi / RIFE yok" bölümleri kartların yazıldığı tarihteki (`2026-11-21..24`) durumu yansıtır. Güncel 9-crate / 60-WGSL motorda ([state.md](../state.md)) `Scene3D`/`Depth32Float`, `Shape3D`, `Text3D` outline/chrome/Z-echo, üçlü `RibbonTrail3D`, `HarfRust+Skrifa+Slug`, `OpticalFlowBlur` (Pyramidal RSMB), `RIFE`/`minterpolate`, `isnetis.onnx` rotoscope, `Depth-Anything-V2-Small` derinlik, `ai-stems`/`ai-lyrics`, `AnimeSnap`/`CubicBezierSpeed`, `BlurMoCurves`, `AdjustmentLayer`, `Null3D`, `AudioBus`, `SafeExpr` (`spring`/`wiggle`/koşullu ifadeler), `DisplacementMap`, `FrameEcho` ve `ShapeMask` yetenekleri headless olarak mevcuttur.

---

## Ajan Çalışma Protokolü

### 1. Yeni Referans Video Geldiğinde
1. Videoyu indirip doğrudan izle (`uvx yt-dlp` → geçici `scratch/` dizini; repoyu kirletme) ve `ffprobe` ile teknik metadatayı (çözünürlük, fps, süre, ses) al.
2. [../_templates/stil-karti.md](../_templates/stil-karti.md) şablonunu kullanarak `SC-NNNN-<kisa-ad>.md` dosyasını oluştur.
3. Yukarıdaki **Stil Kartları İndeksi** tablosuna ekle ve [../index.md](../index.md) / `brain/log/` güncellemelerini yap.
4. Mevcut motorda eksik olan yeni bir teknik çıktıysa kartın 6. bölümüne (`FrameShift Yetenek Eşleşmesi`) net şekilde işle.

### 2. Kullanıcı Referanssız Edit İstediğinde
1. Önce bu dosyadaki **Hızlı Seçim Matrisi**'ni oku.
2. İstenen karakter, şarkı enerjisi ve mevcut motor hazırlığına göre en uygun **1–2 stil kartını** (veya hibrit birleşimini) seçip kartın içindeki **Kurgu Dili, Kamera, Renk, Efekt Tarifi ve Do/Don't Kuralları**'nı uygula; üretim için `peaks --band` / `analyze_multiband_beats`, `Motion.md` DSL (`compile-dsl` / `compile_motion_dsl`), `ParametricTemplateSpec` (`template-instantiate`) ve hazır `EffectChainPreset` (`apply_effect_preset`) araçlarını kullan, çıktı öncesi `lint` / `lint_project` ve `frame` / `diff-frame` ile doğrula.
3. Birden fazla uygun stil varsa kullanıcıya 2–3 somut stil kartı seçeneği sun (ör. *"SC-0001 tarzı tipografi ağırlıklı mı, yoksa SC-0004/SC-0005 tarzı anime flow/velocity ağırlıklı mı gidelim?"*).
