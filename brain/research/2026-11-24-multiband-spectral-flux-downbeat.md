---
title: "Çok-Bantlı Spectral Flux, Adaptif Onset Picking ve Downbeat (Ölçü Başı) Tespiti Mimarisi"
date: 2026-11-24
status: active
tags: ["audio", "timebase", "cli", "mcp"]
---

# Çok-Bantlı Spectral Flux, Adaptif Onset Picking ve Downbeat (Ölçü Başı) Tespiti Mimarisi

## Soru / Amaç

FrameShift'in mevcut ses analiz hattı (`AudioBuffer::analyze_beats_and_energy`) yalnızca tek-bant tam-spektrum zaman-domenli RMS pencerelemesine ve basit IBI medyan kestirimine dayanmaktadır. Anime/AMV ve phonk/EDM kurgularında kick/808 (`sub`), snare/vokal (`mid`) ve hi-hat/riser (`high`) darbelerini birbirinden ayırmak, oktav hatalarına (60 vs 120 vs 240 BPM) dirençli tempo ile **4/4 ölçü başı (downbeat / bar phase)** konumlarını `Flicks(i64)` tam sayı zaman tabanında deterministik olarak üretmek için:

1. **Sıfır yeni crate ile saf Rust Radix-2 Cooley-Tukey FFT** ile **`rustfft`** (`v6.4.1`) ekleme maliyeti, performansı ve lisansının karşılaştırılması,
2. **3-bantlı (`20–120 Hz`, `120–2000 Hz`, `2000–16000 Hz`)** half-wave rectified spectral flux + adaptif medyan/ortalama eşikli onset picking algoritmasının tasarlanması,
3. **Otokorelasyon + algısal tempo önceliği + 4/4 comb-filter/faz skorlamalı downbeat** tespitinin `Flicks(i64)` (`705_600_000 / sn`) çıktısı verecek biçimde kurgulanması,
4. Mevcut `AudioPeakInfo`, `AudioRmsEnvelope`, **M1 golden hash** (sentetik `BeatAudio120Bpm` varlıkları dahil), `autocut`, CLI `peaks`/`autocut`, MCP ve `SafeExpr` katmanlarıyla geriye dönük uyumlu Faz 2 uygulama reçetesinin çıkarılması.

---

## İncelenen Birincil Kaynaklar

### Harici Literatür ve Crate Dokümantasyonu (erişim: 2026-11-24)

- **Bello, J. P., Daudet, L., Abdallah, S., Duxbury, C., Davies, M. & Sandler, M. B. (2005):** *"A Tutorial on Onset Detection in Music Signals"*, IEEE Transactions on Speech and Audio Processing — https://doi.org/10.1109/TSA.2005.851998 (ve Semantic Scholar Corpus ID 9539848: https://api.semanticscholar.org/CorpusID:9539848). Spectral difference, half-wave rectification ($H(x) = \frac{x + |x|}{2}$) ve dinamik eşikleme temelleri.
- **Dixon, S. (2006):** *"Onset Detection Revisited"*, DAFx-06 Proceedings — http://www.dafx.de/proceedings/papers/dafx_06/p133.pdf (ve Semantic Scholar Corpus ID 7540041: https://api.semanticscholar.org/CorpusID:7540041). Log-spectral flux + kayan pencere adaptif medyan/ortalama tepe seçimi.
- **Böck, S. & Widmer, G. (2013):** *"Maximum Filter Vibrato Suppression for Onset Detection" (SuperFlux)*, DAFx-13 Proceedings — yerel frekans komşuluğu maksimum filtresi ile vibrato/tremolo yanlış pozitiflerinin bastırılması.
- **Ellis, D. P. W. (2007):** *"Beat Tracking by Dynamic Programming"*, Journal of New Music Research — https://www.ee.columbia.edu/~dpwe/pubs/Ellis07-beattrack.pdf (ve `librosa.beat.beat_track` referansı: https://librosa.org/doc/latest/generated/librosa.beat.beat_track.html). Otokorelasyon + log-normal tempo tercihi + Viterbi/DP vuruş ızgarası hizalaması.
- **Davies, M. E. P. & Plumbley, M. D. (2007):** *"Context-Dependent Beat Tracking of Musical Audio"*, IEEE TASLP — iki durumlu otokorelasyon + adaptif tarak filtresi (comb filterbank) ile periyot ve faz çıkarımı.
- **Krebs, F., Böck, S. & Widmer, G. (2016):** *"Downbeat Tracking Using Beat-synchronous Features and Recurrent Neural Networks"*, ISMIR 2016 — beat-synchronous spektral/bant özelliklerinden ölçü başı (downbeat) kestirimi kavramı.
- **`rustfft` (`v6.4.1`) & `realfft`:** https://crates.io/crates/rustfft , https://docs.rs/rustfft/ , https://github.com/ejmahler/RustFFT ve https://docs.rs/realfft/ (lisans, `std` gereksinimi, SIMD özellikleri ve bağımlılık grafiği).

### Yerel Kod Tabanı ve Kilitli Sözleşmeler

- [Cargo.toml](file:///C:/Users/Yigit/Documents/FrameShift/Cargo.toml) (workspace bağımlılıkları)
- [crates/frameshift-media/Cargo.toml](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-media/Cargo.toml)
- [crates/frameshift-core/Cargo.toml](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-core/Cargo.toml)
- [crates/frameshift-time/Cargo.toml](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-time/Cargo.toml)
- [brain/specs/timebase.md](file:///C:/Users/Yigit/Documents/FrameShift/brain/specs/timebase.md) (`Flicks(i64)`, `i128` ara aritmetiği, `48_000 Hz` = `14_700` ticks/örnek)
- [crates/frameshift-media/src/audio.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-media/src/audio.rs) (`AudioPeakInfo`, `AudioAnalysisReport`, `AudioBuffer`, `compute_rms_envelope`, `analyze_beats_and_energy`, `detect_peak_in_window`, `detect_transients`, `make_hann_window`, `estimate_bpm_from_beats`)
- [crates/frameshift-media/src/analysis.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-media/src/analysis.rs) (`ClipAnalysis`, saf Rust analiz konvansiyonu)
- [crates/frameshift-media/src/synthetic.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-media/src/synthetic.rs) (`SyntheticMediaGenerator::generate_beat_audio`)
- [crates/frameshift-media/tests/media_tests.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-media/tests/media_tests.rs) (`analyze_beats_and_energy_detects_beats_and_bpm`)
- [crates/frameshift-render/src/compositor.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/src/compositor.rs) (`AudioRmsEnvelope`, `ProjectAnimator::with_audio_envelope`, `ProjectAnimator::build_expr_context`)
- [crates/frameshift-time/src/expr.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-time/src/expr.rs) (`ExprContext`, `ContextVar`, `SafeExpr`)
- [crates/frameshift-core/src/engine.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-core/src/engine.rs) (`audio_envelope_cache_key`, `resolve_disk_audio_envelope`, `verify_m1_golden_scene`, `verify_audio_impact_sync`)
- [crates/frameshift-core/src/autocut.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-core/src/autocut.rs) ve [crates/frameshift-schema/src/session.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-schema/src/session.rs) (`plan_autocut`)
- [crates/frameshift-core/src/inspection.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-core/src/inspection.rs) (`audio_analysis: Option<AudioAnalysisReport>`)
- [crates/frameshift-cli/src/main.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-cli/src/main.rs) (`peaks`, `autocut`, `format_beat_report`, `format_autocut_report`)
- [crates/frameshift-mcp/src/server.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-mcp/src/server.rs) (`describe_schema`, `analyze_audio`, `plan_autocut`)

---

## Doğrudan Gözlemler (Kanıtlı)

1. **Workspace bağımlılık ve kilit durumu:**
   - [Cargo.toml](file:///C:/Users/Yigit/Documents/FrameShift/Cargo.toml#L15-L25) lisansı `MIT OR Apache-2.0`, sürüm `edition = "2024"`.
   - `Cargo.lock` içinde `rustfft`, `realfft`, `num-complex`, `strength_reduce`, `transpose` veya `primal-check` paketlerinin **hiçbiri mevcut değildir** (`rg` araması exit code 1 döndürdü).
   - [crates/frameshift-media/Cargo.toml](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-media/Cargo.toml#L8-L13) yalnızca `frameshift-time`, `png`, `serde`, `serde_json` ve `thiserror` bağımlılıklarını taşır.
   - `rustfft` (`v6.4.1`, `MIT OR Apache-2.0`) `std` gerektirir, varsayılan olarak `avx`/`sse`/`neon` SIMD etkinleştirir, ancak kendi `Cargo.toml` dosyasında `num-complex`, `num-traits`, `num-integer`, `strength_reduce`, `transpose`, `primal-check` olmak üzere 6 doğrudan bağımlılık çeker ve karmaşık (`Complex<T>`) C2C arayüzü sunar; gerçek değerli sinyalde `N/2` bellek/hız kazancı için ayrıca `realfft` (`MIT`) wrapper'ı veya manuel çift/tek paketleme gerekir.

2. **Mevcut ses analiz ve beat tespiti (`frameshift-media/src/audio.rs`):**
   - `DEFAULT_AUDIO_SAMPLE_RATE = 48_000` Hz ([audio.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-media/src/audio.rs#L12)); [timebase.md](file:///C:/Users/Yigit/Documents/FrameShift/brain/specs/timebase.md#L83-L87) uyarınca `48_000 Hz` tam olarak `14_700` Flicks/örnek (`705_600_000 / 48_000`) ile bölünür.
   - `AudioAnalysisReport` ([audio.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-media/src/audio.rs#L36-L42)) dört public alandan oluşur: `window_ticks: Flicks`, `rms_envelope: Vec<(Flicks, f32)>`, `detected_beats: Vec<Flicks>`, `estimated_bpm: f64`. Yapı `Default` türetmemiştir ve [autocut.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-core/src/autocut.rs#L108) ile [main.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-cli/src/main.rs#L2068) içindeki testlerde doğrudan 4 alanlı struct literal ile kurulmaktadır.
   - `AudioBuffer::analyze_beats_and_energy` ([audio.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-media/src/audio.rs#L643-L742)) zaman-domenli tam bant RMS (`compute_rms_windows`) hesaplar, yerel komşulukta (`±0.5s`, `min 4` pencere) `local_mean * 1.4 + 0.06 * max_rms` eşiğini aşan tepe pencerelerinin en yüksek mutlak genlikli örneğini `beat_tick` olarak seçer ve `refractory_ticks = FLICKS_PER_SECOND / 5` (`200 ms`, max 300 BPM) uygular. Frekans bandı ayrımı, spektral flux, half-wave rectification veya downbeat faz tespiti yoktur.
   - `estimate_bpm_from_beats` ([audio.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-media/src/audio.rs#L1323-L1365)) yalnızca ardışık `detected_beats` farklarının (`IBI`) `[0.15, 2.5]` sn aralığındaki medyanı etrafındaki `±%15` kümesinin ortalamasını alır; otokorelasyon veya oktav önceliği yoktur.
   - `make_hann_window(len: usize)` ([audio.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-media/src/audio.rs#L1193-L1201)) ve `to_mono_mixdown(&self)` ([audio.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-media/src/audio.rs#L554-L568)) halihazırda `audio.rs` içinde özel yardımcı fonksiyon olarak mevcuttur.

3. **Sentetik testler ve M1 Golden Hash sınırı:**
   - `SyntheticMediaGenerator::generate_beat_audio` ([synthetic.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-media/src/synthetic.rs#L222-L278)) `0.98` genlikli tek-örneklik impulse + `880 Hz` sönümlü (`exp(-120*t)`) kosinüs darbesi üretir; yani sentetik sinyalin enerjisi `20–120 Hz` sub bandında değil, geniş bant impulse + `880 Hz` (`mid`) bandındadır.
   - `analyze_beats_and_energy_detects_beats_and_bpm` testi ([media_tests.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-media/tests/media_tests.rs#L342-L387)) `120 BPM` (`4s` -> `8` beat, her biri `<= 1 ms` hata, `estimated_bpm` `120 ± 1`), `140 BPM` (`140 ± 2`) ve tam sessizlik (`0` beat, `0.0` BPM) koşullarını doğrular.
   - `resolve_disk_audio_envelope` ([engine.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-core/src/engine.rs#L1071-L1106)) `Project::golden_scene_preset()` gibi sentetik `BeatAudio120Bpm` projelerinde `has_disk_audio == false` gördüğünden `Ok(None)` döndürür; `default_cache_capacity_is_128_and_synthetic_audio_skips_envelope` birim testi bu davranışı "M1 hash'lerini korumak" için açıkça doğrulamaktadır ([engine.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-core/src/engine.rs#L1127)).
   - `verify_audio_impact_sync` ([engine.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-core/src/engine.rs#L988-L996)) ise `audio_buf.detect_peak_in_window(drop_tick, frame_ticks)` fonksiyonunu kullanır.

4. **`SafeExpr`, `ProjectAnimator`, `CLI` ve `MCP` yüzeyleri:**
   - `ExprContext` ([expr.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-time/src/expr.rs#L53-L96)) şu an 6 değişken sunar: `t_sec`, `output_tick`, `beat_phase`, `audio_rms`, `event_delta_sec`, `progress`. Struct `Default` türetmektedir; ancak [expr_tests.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-time/tests/expr_tests.rs#L4-L426), [main.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-cli/src/main.rs#L595-L602) ve [compositor.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/src/compositor.rs#L259-L266) içinde açık struct literal kurulumları vardır.
   - `ProjectAnimator::build_expr_context` ([compositor.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-render/src/compositor.rs#L225-L267)) içinde `beat_phase` şu an **sabit 120 BPM'e** (`beat_period_ticks = FLICKS_PER_SECOND / 2`) kilitlidir ve en yakın `project.events` çapasına göre `rem_euclid(1.0)` hesaplar; `audio_envelope` yalnızca `Option<&'a AudioRmsEnvelope>` üzerinden tek-bant `audio_rms` besler.
   - `crates/frameshift-core/src/autocut.rs` (`plan_autocut(report: &AudioAnalysisReport, params: AutocutParams)`) ile `crates/frameshift-schema/src/session.rs` (`plan_autocut(beats: &[Flicks], assets: &[AutocutAsset], options: &AutocutOptions)`) birbirinden ayrı iki yardımcıdır; her ikisi de yalnızca tek bir `Vec<Flicks>` vuruş listesi tüketir.
   - CLI `peaks` ve `autocut` ([main.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-cli/src/main.rs#L417-L575)) şu an düz metin (`beats=... bpm=...`, `autocut cuts=...`) basar ve `--json` veya bant/downbeat seçim bayrağı içermez.
   - MCP `analyze_audio` ([server.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-mcp/src/server.rs#L465-L486)) doğrudan `AudioAnalysisReport` serileştirir; `describe_schema` ([server.rs](file:///C:/Users/Yigit/Documents/FrameShift/crates/frameshift-mcp/src/server.rs#L450-L457)) `safe_expr_variables` listesini döndürür.

---

## Çıkarımlar ve Öneriler

### 1. FFT Motoru Karşılaştırması: Saf Rust Radix-2 Cooley-Tukey vs `rustfft`

*(Mimari değerlendirme: gözlemlenen `Cargo.lock` ve `rustfft` özelliklerine dayanır.)*

| Kriter | Saf Rust Radix-2 Cooley-Tukey (Önerilen) | `rustfft` (`6.4.1`) + `realfft` |
| :--- | :--- | :--- |
| **Yeni Cargo Bağımlılığı** | **0 crate** (`Cargo.lock` değişmez) | **7–8 crate** (`rustfft` + 6 alt crate + isteğe bağlı `realfft`) |
| **Lisans** | Depo lisansı (`MIT OR Apache-2.0`) | `MIT OR Apache-2.0` (`realfft`: `MIT`) |
| **Pencere Boyutu (`N`)** | Yalnızca $2^k$ (`N = 2048` veya `4096`) | Keyfi tam sayı $N$ (Bluestein / Rader / mixed-radix) |
| **Gerçek Değerli (R2C) Verim** | `N/2` complex paketleme ile doğrudan R2C (`N/2+1` bin) veya doğrudan in-place Radix-2 | `rustfft` tek başına `Complex<f32>` ister; `realfft` ile `N/2` optimizasyonu gelir |
| **Çalışma Maliyeti (Kestirim)** | 30 sn @ 48 kHz, `H=480` (3000 pencere), `N=4096`: $\sim 3000 \times 4096 \log_2(4096) \approx 147\text{M}$ temel işlem (tek çekirdek ~30–80 ms kestirilir; `ffmpeg` decode'dan kısa) | AVX2/NEON SIMD ile aynı iş ~5–15 ms kestirilir; ancak analiz zaten offline/önbellekli (`resolve_disk_audio_envelope`) |
| **Determinizm & Bakım** | Platform SIMD farklılıklarından bağımsız saf `f64`/`f32` iteratif kelebek; ~130 satır, kolay denetim | Donanım SIMD yoluna göre son bit yuvarlama farkı olasılığı; harici crate güncelleme yükü |

**Öneri:** FrameShift'in ses analizi çevrimdışı (render öncesi önbellekli veya `peaks`/`autocut` CLI/MCP çağrısı) çalıştığı ve tüm analiz pencereleri $N \in \{2048, 4096\}$ olarak $2^k$ seçilebildiği için **sıfır bağımlılıklı saf Rust iteratif Radix-2 Cooley-Tukey FFT** (`crates/frameshift-media/src/spectral.rs`) yeterlidir ve `AGENTS.md` bağımlılık disipliniyle uyumludur. İleride rastgele $N$ (ör. WSOLA veya dinamik pitch-shifting) gerekirse `rustfft` eklentisi trait arkasından takılabilir.

### 2. 3-Bantlı Half-Wave Rectified Spectral Flux + Adaptif Eşikli Onset Picking

**Ön-işleme ve STFT Parametreleri (`48_000 Hz` dahili miks):**
- Giriş: `buf.to_mono_mixdown()` (farklı `sample_rate` varsa önce `resample_to(DEFAULT_AUDIO_SAMPLE_RATE)` veya doğrudan `fs` üzerinden frekans-bin eşlemesi).
- FFT boyutu **`N = 4096`** ($\Delta f = 48000 / 4096 = 11.71875\text{ Hz/bin}$). Neden `4096`? `N = 2048` ile `20–120 Hz` sub bandı yalnızca 4–5 bin (`23.4–117.2 Hz`) içerirken, `N = 4096` ile **9 bin** (`k = 2..=10`, `23.4–117.2 Hz`) elde edilir ve kick temel frekansı (`50–90 Hz`) ile bassline ayrımı belirginleşir.
- Hop adımı: Mevcut `window_ticks` ile senkronize (`10 ms` = `7_056_000` Flicks $\rightarrow$ `480` örnek @ 48 kHz). Her çerçeve `i` merkezi/başlangıcı tam sayı `Flicks(i * window_ticks.0)` (veya `audio_samples_to_ticks(center_sample, sample_rate)`) ile damgalanır.
- Pencereleme: Mevcut `make_hann_window(N)` kullanılır (sıfır-faz kayması veya çerçeve başı dokümante edilir).

**Frekans Bantları ve Log-Magnitude Sıkıştırma:**
Her bin $k \in [1, N/2]$ için $f_k = \frac{k \cdot f_s}{N}$:
- **Sub (`20–120 Hz`):** Kick / 808 sub-bass darbeleri.
- **Mid (`120–2000 Hz`):** Snare gövdesi, vokal/synth atakları, gitar/piyano transient'leri.
- **High (`2000–16000 Hz`):** Hi-hat, crash, clap snap ve anime FX riser parlaklığı.

Dinamik aralık sıkıştırması (Böck & Widmer / Dixon):
$$\tilde{X}_t[k] = \ln\left(1.0 + \gamma \cdot |X_t[k]|_{\text{norm}}\right), \qquad \gamma = 100.0$$
Burada $|X_t[k]|_{\text{norm}} = \frac{2 |X_t[k]|}{\sum_{j=0}^{N-1} w[j]}$ (Hann pencere toplamına göre normalize genlik).

**Half-Wave Rectified Spectral Flux (Bant Bazlı) + Opsiyonel SuperFlux Max-Filter:**
Önceki çerçeve referansı (vibrato bastırma için $\mu \in \{0, 1\}$, varsayılan `max_filter_radius = 1` bin):
$$X^{\text{ref}}_{t-1}[k] = \max_{m \in [\max(k_{\min}, k-\mu),\, \min(k_{\max}, k+\mu)]} \tilde{X}_{t-1}[m]$$
Bant $b \in \{\text{sub}, \text{mid}, \text{high}\}$ için half-wave rectified ($H(x) = \max(0, x)$) spectral flux:
$$SF_b(t) = \frac{1}{|B_b|} \sum_{k \in B_b} \max\left(0.0,\; \tilde{X}_t[k] - X^{\text{ref}}_{t-1}[k]\right)$$
Ayrıca her bantta normalleştirilmiş bant enerjisi $E_b(t) \in [0, 1]$ (Parseval / bin RMS'i tepe-normalize) hesaplanır; bu değerler hem downbeat faz skorlamasında hem de istenirse `SafeExpr` bant zarflarında (`audio_sub`, `audio_mid`, `audio_high`) yeniden kullanılır.

**Bileşik ODF ve Adaptif Medyan + Ortalama Peak Picking:**
Her bant kendi tepe/99.5-yüzdelik veya medyan-ölçekli değerine göre $[0, 1]$ aralığına normalize edilir ($\widehat{SF}_b$):
$$ODF_{\text{mix}}(t) = 0.45 \cdot \widehat{SF}_{\text{sub}}(t) + 0.35 \cdot \widehat{SF}_{\text{mid}}(t) + 0.20 \cdot \widehat{SF}_{\text{high}}(t)$$
*(Not: Ağırlıklar `MultibandFluxConfig` ile yapılandırılabilir olmalı; ör. saf vokal/klik sinyali için tüm bantlar katkı verir.)*

Dixon (2006) / Böck (2013) adaptif eşiği, kayan pencere $W = [t - M, t + M]$ ($M = 0.25\text{ sn} \approx 25$ çerçeve @ 10 ms) üzerinde:
$$\theta(t) = \delta_{\text{floor}} + \lambda_{\text{med}} \cdot \operatorname{median}_{u \in W} ODF(u) + \alpha_{\text{mean}} \cdot \operatorname{mean}_{u \in W} ODF(u)$$
Önerilen varsayılanlar: $\lambda_{\text{med}} = 1.35$, $\alpha_{\text{mean}} = 0.25$, $\delta_{\text{floor}} = 0.05 \cdot \max ODF$.
Bir çerçeve $t$ aşağıdaki 3 koşulu sağladığında **onset** adayı olur:
1. Yerel maksimum: $ODF(t) \ge ODF(u) \;\forall u \in [t-2, t+2]$ ve $ODF(t) > ODF(t-1)$,
2. Eşik üstü: $ODF(t) > \theta(t)$,
3. Refrakter aralık: $(t - t_{\text{prev}}) \ge \text{refractory\_ticks}$ (varsayılan `120 ms` = `84_672_000` Flicks; yüksek hızlı EDM/jugg roll'ları için 200 ms yerine 120 ms, `analyze_beats_and_energy` uyumluluk modunda ise mevcut `200 ms` korunabilir).

Alt-çerçeve hassasiyet: Seçilen onset penceresinin $[-\text{hop}/2, +\text{hop}/2]$ sample aralığında kısa-pencere enerji/genlik artış noktası veya parabolik tepe interpolasyonu ile örnek indeksi $s^*$ bulunur ve `audio_samples_to_ticks(s^* as i64, sample_rate)` ile **tam sayı `Flicks(i64)`** olarak döndürülür.

### 3. Tempo (BPM), Beat Izgarası ve 4/4 Downbeat (Ölçü Başı) Tespiti

1. **Otokorelasyon + Log-Normal Tempo Önceliği (Ellis 2007):**
   - `ODF_mix` sinyali mean-subtracted hâle getirilir: $\tilde{o}(t) = ODF_{\text{mix}}(t) - \bar{o}$.
   - Gecikme (lag) $\tau$ aralığı `40 BPM..=220 BPM` (`lag_min = round(60 / (220 * hop_sec))`, `lag_max = round(60 / (40 * hop_sec))`) taranır; normalize otokorelasyon $R(\tau)$ hesaplanır.
   - Çoklu-harmonik (comb) puanı: $C(\tau) = R(\tau) + 0.5 \cdot R(2\tau) + 0.25 \cdot R(\tau/2)$ (oktav belirsizliğini azaltır).
   - Algısal tempo önceliği (Ellis 2007): $P(\tau) = \exp\left(-\frac{1}{2}\left(\frac{\log_2(\text{bpm}(\tau) / 120.0)}{0.9}\right)^2\right)$.
   - $\tau^* = \arg\max_{\tau} C(\tau) \cdot P(\tau)$ seçilir; komşuları $(\tau^*-1, \tau^*, \tau^*+1)$ ile parabolik interpolasyon yapılıp $\text{BPM} = 60.0 / (\tau_{\text{interp}} \cdot \text{hop\_sec})$ bulunur.

2. **DP / Viterbi Beat Izgarası Hizalaması:**
   - Hedef periyot $\hat{\Delta} = \tau_{\text{interp}}$ çerçeve cinsinden alınır.
   - Ellis (2007) dinamik programlama özeti:
     $$D(t) = ODF_{\text{mix}}(t) + \max_{t' \in [t - 2\hat{\Delta},\; t - \lfloor 0.5\hat{\Delta} \rfloor]} \left( D(t') - \eta \left(\ln \frac{t - t'}{\hat{\Delta}}\right)^2 \right)$$
     (`eta = 6.0`–`8.0` sertlik sabiti). Geri izleme (backtrack) sonrası elde edilen beat çerçeveleri, varsa $\pm 2$ çerçeve içinde en yakın lokal ODF zirvesine hizalanır ve `Flicks(i64)` dizisine (`beat_grid: Vec<Flicks>`) çevrilir.
   - *Kısa sinyal / sentetik klip koruması:* Ses `< 1.5 sn` ise veya otokorelasyon zirvesi zayıfsa (`C(tau*) < 0.12`), doğrudan adaptif onset picking sonuçları + `estimate_bpm_from_beats` fallback'i çalışır.

3. **4/4 Ölçü Başı (Downbeat / Bar Phase) Tespiti:**
   - Varsayılan ölçü `beats_per_bar = 4` (4/4; AMV/anime edit müziklerinin ezici çoğunluğu 4/4).
   - `beat_grid` üzerinde $p \in \{0, 1, 2, 3\}$ dört olası bar-fazı değerlendirilir. Her beat $b_k$ için:
     - **Sub-Kick Atak ($s_{\text{sub}}(b_k)$):** $[b_k - 2\text{ çer.}, b_k + 2\text{ çer.}]$ penceresindeki $\max \widehat{SF}_{\text{sub}}$ ve yerel $\widehat{E}_{\text{sub}}$.
     - **Ölçü-Büyüklüğü Spektral Değişim ($\Delta_{\text{bar}} S(b_k)$):** Beat $b_k$ etrafındaki 12-bant (veya 32-bin log-frekans) ortalama spektrum vektörü $\mathbf{v}_k$ ile bir önceki vuruş $\mathbf{v}_{k-1}$ arasındaki kosinüs mesafesi $1 - \cos(\mathbf{v}_k, \mathbf{v}_{k-1})$ (ölçü başlarında akor/enstrüman değişimi nedeniyle yükselir).
     - **Backbeat (2 ve 4. vuruş) Snare/Clap Desteği:** $k \equiv p+1, p+3 \pmod 4$ vuruşlarında $\widehat{SF}_{\text{mid}} + \widehat{SF}_{\text{high}}$ toplamı.
   - Faz skoru:
     $$\text{BarScore}(p) = \sum_{k \equiv p \pmod 4} \left(0.50\, s_{\text{sub}}(b_k) + 0.30\, \Delta_{\text{bar}} S(b_k) + 0.20\, \widehat{E}_{\text{sub}}(b_k)\right) + 0.25 \sum_{k \equiv p+1, p+3 \pmod 4} \left(\widehat{SF}_{\text{mid}}(b_k) + \widehat{SF}_{\text{high}}(b_k)\right)$$
   - `downbeat_phase = argmax_p BarScore(p)` seçilir; `downbeats: Vec<Flicks>` yalnızca `beat_grid[k]` ($k \equiv p^* \pmod 4$) konumlarından oluşur; `bar_phase_confidence` ise `(best_score - second_best_score) / (best_score.abs() + 1e-6)` olarak hesaplanır.

### 4. Somut Rust API Tasarımı ve Dosya Bazlı Faz 2 Reçetesi

#### A. `crates/frameshift-media/src/spectral.rs` (Yeni Modül — 0 Yeni Crate)
```rust
/// Pure-Rust deterministic in-place Radix-2 Cooley-Tukey FFT plan for `N = 2^k`.
pub struct Radix2Fft {
    len: usize,
    bit_rev: Vec<usize>,
    twiddles_re: Vec<f32>,
    twiddles_im: Vec<f32>,
}

impl Radix2Fft {
    pub fn new(len: usize) -> Result<Self, MediaError>; // len.is_power_of_two() && len >= 2
    pub fn forward_real_magnitudes(&self, windowed_frame: &[f32], out_bins: &mut [f32]);
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MultibandFluxConfig {
    pub fft_size: usize,            // default: 4096
    pub window_ticks: Flicks,       // default: 7_056_000 (10 ms)
    pub sub_hz: (f32, f32),         // default: (20.0, 120.0)
    pub mid_hz: (f32, f32),         // default: (120.0, 2000.0)
    pub high_hz: (f32, f32),        // default: (2000.0, 16000.0)
    pub log_gamma: f32,             // default: 100.0
    pub max_filter_bins: usize,     // default: 1 (SuperFlux)
    pub median_mult: f32,           // default: 1.35
    pub mean_mult: f32,             // default: 0.25
    pub refractory_ticks: Flicks,   // default: 84_672_000 (120 ms)
    pub beats_per_bar: u8,          // default: 4
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BandOnsetSummary {
    pub onsets: Vec<Flicks>,        // deterministik Flicks(i64)
    pub flux_envelope: Vec<f32>,    // [0.0, 1.0], window_ticks adımlı
    pub energy_envelope: Vec<f32>,  // [0.0, 1.0], window_ticks adımlı
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MultibandBeatReport {
    pub window_ticks: Flicks,
    pub fft_size: usize,
    pub estimated_bpm: f64,
    pub tempo_confidence: f64,
    pub beats_per_bar: u8,
    pub downbeat_phase: u8,         // 0..beats_per_bar
    pub bar_phase_confidence: f64,
    pub beat_grid: Vec<Flicks>,     // DP-hizalı vuruşlar
    pub downbeats: Vec<Flicks>,     // Ölçü başları (4/4 bar 1)
    pub sub: BandOnsetSummary,      // 20-120 Hz
    pub mid: BandOnsetSummary,      // 120-2000 Hz
    pub high: BandOnsetSummary,     // 2000-16000 Hz
}
```
`AudioBuffer` üzerine eklenecek metot:
```rust
impl AudioBuffer {
    pub fn analyze_multiband_beats(
        &self,
        config: MultibandFluxConfig,
    ) -> Result<MultibandBeatReport, MediaError>;
}
```

#### B. Geriye Dönük Uyumluluk (`AudioAnalysisReport`, M1 Golden Hash ve `AudioPeakInfo`)
1. **`AudioAnalysisReport` korunumu:** Mevcut `AudioAnalysisReport` struct'ının 4 alanı (`window_ticks`, `rms_envelope`, `detected_beats`, `estimated_bpm`) değiştirilmez veya `#[serde(default, skip_serializing_if = "Option::is_none")] pub multiband: Option<MultibandBeatReport>` eklenirse `Default` türetilerek eski 4 alanlı literal noktalar (`autocut.rs#L108`, `main.rs#L2068`) güncellenir. Daha temiz yol (seçeneklere bkz.): `AudioBuffer::analyze_beats_and_energy` imzasını ve mevcut 4 alanı **hiç kırmadan**, `analyze_multiband_beats` ile ayrı `MultibandBeatReport` üretmek ve `AudioAnalysisReport` JSON çıktısına opsiyonel `#[serde(default)]` köprüsü koymaktır.
2. **Sentetik `generate_beat_audio` testleri:** `analyze_beats_and_energy` mevcut sentetik testleri (`120 BPM`, `140 BPM`, `<= 1 ms` tolerans, `silence`) korumaya devam eder; yeni `analyze_multiband_beats` üzerinde ayrıca hem `generate_beat_audio` (mid-bant 880 Hz click!) hem sentetik 4/4 `sub kick (60 Hz) + snare (400 Hz + gürültü) + hi-hat (8 kHz)` testi yazılır.
3. **M1 Golden Hash Koruması:** `crates/frameshift-core/src/engine.rs#L1071-L1106` içindeki `resolve_disk_audio_envelope` **yalnızca `ResolvedAssetSource::Disk(_)`** için çalışmaya devam etmeli; `BeatAudio120Bpm` sentetik varlığı daima `Ok(None)` döndürerek `ProjectAnimator` fallback formülünü ve M1 frame hash'lerini birebir korumalıdır.
4. **`AudioPeakInfo` / `verify_audio_impact_sync`:** `detect_peak_in_window` ve `detect_transients` dokunulmadan kalır; M1 audio-impact sync kapısı etkilenmez.

#### C. `frameshift-core/src/autocut.rs` & `frameshift-cli` & `frameshift-mcp`
- `AutocutBeatSource` enum'u (`DetectedBeats`, `BeatGrid`, `Downbeats`, `SubOnsets`, `MidOnsets`, `HighOnsets`):
  - `plan_autocut_from_ticks(ticks: &[Flicks], params: AutocutParams)` ortak çekirdek fonksiyonu çıkarılır; `plan_autocut(report: &AudioAnalysisReport, params)` bu çekirdeğe delege eder (sıfır kod tekrarı).
- CLI `peaks` ve `autocut` (`crates/frameshift-cli/src/main.rs`):
  - `--band all|sub|mid|high|beats|downbeats` (varsayılan mevcut davranış) ve `--json` (tam `MultibandBeatReport` / kesim çıktısı JSON).
  - `--write-events` kullanıldığında downbeat'ler `detected_downbeat_000`, `detected_sub_000` vb. adlarıyla `EventOrigin::Detected` olarak damgalanabilir.
- MCP (`crates/frameshift-mcp/src/server.rs`):
  - `analyze_audio` isteğine opsiyonel `multiband: bool` (veya yeni `analyze_audio_multiband` aracı) + `plan_autocut` içine `source: "detected" | "beats" | "downbeats" | "sub" | "mid" | "high"`.

#### D. `SafeExpr` (`frameshift-time/src/expr.rs`) & `AudioRmsEnvelope` (`frameshift-render/src/compositor.rs`)
- `AudioBandEnvelope` (veya `AudioRmsEnvelope` genişletmesi):
  ```rust
  pub struct AudioBandEnvelope {
      pub window_ticks: Flicks,
      pub rms: Vec<f32>,
      pub sub: Vec<f32>,
      pub mid: Vec<f32>,
      pub high: Vec<f32>,
      pub estimated_bpm: f64,
      pub beat_anchor_tick: Flicks,
      pub downbeat_anchor_tick: Flicks,
      pub beats_per_bar: u8,
  }
  ```
- `ExprContext` içine opsiyonel yeni alanlar (`audio_sub`, `audio_mid`, `audio_high`, `downbeat_phase`) eklendiğinde, disk sesi olan projelerde `ProjectAnimator::build_expr_context` sabit `FLICKS_PER_SECOND / 2` (120 BPM) yerine `estimated_bpm` tabanlı gerçek `beat_phase` ve `downbeat_phase = ((sub_tick.0 - downbeat_anchor_tick.0) as f64 / (beat_period_ticks * beats_per_bar as f64)).rem_euclid(1.0)` hesaplayabilir; disk sesi yoksa (M1 golden scene dahil) mevcut davranış birebir korunur.

---

## Doğrulanmamış Noktalar

- Saf Rust Radix-2 Cooley-Tukey (`N = 4096`, `H = 480`) için gerçek 20–60 sn phonk/anime MP4 ses dosyası üzerindeki kesin CPU çalışma süresi (ms) henüz benchmark edilmemiştir.
- Sabit 4/4 ölçü (`beats_per_bar = 4`) dışındaki 3/4 (vals) veya 6/8 anime OST parçalarında yukarıdaki `BarScore(p)` formülünün davranışı henüz gerçek veri kümesi üzerinde test edilmemiştir.
- `generate_beat_audio` sentetik sinyali sub-bant (20–120 Hz) içermediğinden, yeni çok-bantlı boru hattının sub/mid/high birim testleri için 60 Hz kick + 400 Hz snare + 8 kHz hi-hat üreten ayrı bir sentetik test yardımcısı gerekecektir.

---

## Kullanıcı Kararı Gerektiren Açık Noktalar

1. **FFT Uygulama Motoru (`Cargo.toml` Bağımlılık Kapısı):**
   - **Seçenek A — Saf Rust Radix-2 Cooley-Tukey (Önerilen Varsayılan):** `crates/frameshift-media/src/spectral.rs` içinde ~130 satır sıfır-bağımlılıklı FFT (`N = 2048`/`4096`). *Artısı:* 0 yeni crate, `Cargo.lock` temiz kalır, tam deterministik. *Eksisi:* Yalnızca $2^k$ boyutlar ve SIMD yok (offline analizde önemsiz).
   - **Seçenek B — `rustfft = "6.4.1"` (+ `realfft`):** *Artısı:* Keyfi $N$ ve AVX2/NEON SIMD hızı. *Eksisi:* `Cargo.lock`'a 7–8 yeni crate ekler; `AGENTS.md` uyarınca harici bağımlılık onayı gerekir.

2. **CLI `peaks`/`autocut` ve MCP Rapor Şema Uzantısı:**
   - **Seçenek A — Mevcut Struct'ı Kırmadan Opsiyonel Uzantı + `--json` / `--band` Bayrakları (Önerilen Varsayılan):** `AudioBuffer::analyze_multiband_beats` ayrı `MultibandBeatReport` döndürür; `AudioAnalysisReport` geriye dönük uyumlu tutulur (veya `#[serde(default, skip_serializing_if = "Option::is_none")] pub spectral: Option<MultibandBeatReport>` ile JSON olarak genişletilir); CLI `peaks`/`autocut`'a `--band all|sub|mid|high|beats|downbeats` ve `--json` eklenir.
   - **Seçenek B — `AudioAnalysisReport` Zorunlu Alan Değişikliği:** Tüm mevcut çağrı ve test struct literal'lerini yeni zorunlu alanlarla güncellemek (kırıcı iç API değişikliği).
   - **Seçenek C — Tamamen Ayrı Komut/Araç:** `frameshift-cli spectral-peaks` ve MCP `analyze_audio_multiband` eklemek; mevcut `peaks`/`autocut`/`analyze_audio` yüzeyine dokunmamak.

3. **`SafeExpr` (`ExprContext`) için Yeni Ses Uniformları:**
   - **Seçenek A — `audio_sub`, `audio_mid`, `audio_high`, `downbeat_phase` + Disk Sesinde Gerçek BPM'li `beat_phase` (Önerilen Varsayılan):** Anime editlerde `DeepGlow`, `MotionTileShake` ve `Shockwave`'i doğrudan kick (`audio_sub`), vokal/snare (`audio_mid`), hi-hat (`audio_high`) ve ölçü başına (`downbeat_phase`) bağlamayı mümkün kılar; M1 sentetik varlıkta `resolve_disk_audio_envelope` `None` döndürmeye devam ettiği için M1 golden hash bozulmaz. (`crates/frameshift-mcp` `describe_schema` ve `ExprContext` literal'leri güncellenir.)
   - **Seçenek B — Yalnızca `audio_sub`, `audio_mid`, `audio_high`:** `downbeat_phase` uniformu yerine `--write-events` ile damgalanan `detected_downbeat_*` olaylarını ve mevcut `event_delta_sec`'i kullanmak.
   - **Seçenek C — Faz 2'de `ExprContext`'e Dokunmamak:** Yalnızca CLI/MCP analiz + `autocut` + event anchor tarafını güncellemek, shader/ifade uniformlarını Faz 3'e bırakmak.
