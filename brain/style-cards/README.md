# FrameShift — Referans Stil Kartları (`brain/style-cards/`)

Bu klasör, kullanıcının beğendiği referans editlerden çıkarılan **yapılandırılmış görsel stil, kurgu dili ve efekt reçetelerini** tutar. Amaç:
1. Farklı edit türlerinden (anime AMV, 3D kinetik tipografi, kolaj, HUD/yoğun kompozisyon vb.) öğrenilen teknikleri kalıcı ve tekrar kullanılabilir kartlara dönüştürmek.
2. Kullanıcı ileride referans video vermeden *"şu karaktere / şu şarkıya edit yap"* dediğinde, ajanların bu kartlara bakarak kullanıcının kalite çıtasına ve görsel zevkine uygun, tekdüze veya aşırı basit olmayan editler üretmesini sağlamak.
3. Hangi motor özelliklerinin birden fazla beğenilen stilde ortak ihtiyaç olarak tekrar ettiğini görüp yol haritasını gerçek talebe göre önceliklendirmek.

---

## Stil Kartları İndeksi (Hızlı Seçim Matrisi)

| ID | Stil Adı | Kategori | Enerji | Hazırlık | Ne Zaman Seçilmeli? |
| :--- | :--- | :--- | :--- | :--- | :--- |
| [SC-0001](SC-0001-platsae-3d-kinetic-typography.md) | PlatsAE 3D Neon Kinetic Typography | `kinetic-typography` | `high` | `needs-features` (Faz 6A/6B) | Hızlı pop/EDM/phonk vokalli şarkılarda %100 söz-odaklı 3D kinetik tipografi veya yüksek enerjili intro/chorus sekansları |
| [SC-0002](SC-0002-mushoku-tensei-kinetic-type-amv.md) | Mushoku Tensei Full-Frame Kinetic Typography AMV | `kinetic-typography` | `high` | `partial` | Orta/yavaş tempolu vokalli şarkılarda dev, harf harf yayılmış, özne arkasında kalan 2D tipografi; 1 vuruş = 1 plan kurgu |
| [SC-0003](SC-0003-theamirhanio-curse-tiktok-edit.md) | Curse — efekt odaklı TikTok anime edit | `anime-amv` | `high` | `partial` | Karakter motifi, siyah bekleme, kısa bulanık giriş ve ışık vuruşları etrafında kurulan sosyal editler. |
| [SC-0004](SC-0004-xenoz-flow-impact-amv.md) | Xenoz Ekolü — Flow & Hard Impact AMV | `anime-amv` | `high` | `partial` | Aksiyon akışı, anime snap hız eğrileri, yönlü geçişler, impact shake ve çok katmanlı SFX ile yüksek enerjili AMV'ler (arketip). |
| [SC-0005](SC-0005-nep-one-framer-jugg-edit.md) | Nep / Jugg Ekolü — One-Framer & Velocity Edit | `anime-amv` | `high` | `partial` | Hızlı transient odaklı one-framer flaşlar, stutter, sert bounce ve yoğun kinetik metin içeren kısa sosyal editler (arketip). |

*(Eski serbest formatlı ilk 4 referans notu kök dizindeki [VISUAL_REFERENCES.md](../../VISUAL_REFERENCES.md) dosyasında korunmaktadır; istendiğinde onlar da `SC-0002+` kartlarına dönüştürülebilir.)*

---

## Ajan Çalışma Protokolü

### 1. Yeni Referans Video Geldiğinde
1. Videoyu indirip doğrudan izle (`uvx yt-dlp` → geçici `scratch/` dizini; repoyu kirletme) ve `ffprobe` ile teknik metadatayı (çözünürlük, fps, süre, ses) al.
2. [../_templates/stil-karti.md](../_templates/stil-karti.md) şablonunu kullanarak `SC-NNNN-<kisa-ad>.md` dosyasını oluştur.
3. Yukarıdaki **Stil Kartları İndeksi** tablosuna ekle ve `brain/index.md` / `brain/log/` güncellemelerini yap.
4. Mevcut motorda eksik olan yeni bir teknik çıktıysa kartın 6. bölümüne (`FrameShift Yetenek Eşleşmesi`) net şekilde işle.

### 2. Kullanıcı Referanssız Edit İstediğinde
1. Önce bu dosyadaki **Hızlı Seçim Matrisi**'ni oku.
2. İstenen karakter, şarkı enerjisi ve mevcut motor hazırlığına göre en uygun **1–2 stil kartını** (veya hibrit birleşimini) seçip kartın içindeki **Kurgu Dili, Kamera, Renk, Efekt Tarifi ve Do/Don't Kuralları**'nı proje şemasına uygula.
3. Birden fazla uygun stil varsa kullanıcıya 2–3 somut stil kartı seçeneği sun (ör. *"SC-0001 tarzı tipografi ağırlıklı mı, yoksa SC-000X tarzı aksiyon/kolaj ağırlıklı mı gidelim?"*).
