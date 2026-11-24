---
title: "AI Stem Ayrımı ve Kelime-Düzeyi Söz Zamanlama (Demucs / audio-separator + faster-whisper / stable-ts)"
date: 2026-11-24
status: active
tags: ["audio", "ai", "lyrics", "cli", "mcp"]
---

# AI Stem Ayrımı ve Kelime-Düzeyi Söz Zamanlama (Demucs / audio-separator + faster-whisper / stable-ts)

## Soru / Amaç

FrameShift (`brain/state.md` sıradaki adım #5 ve `brain/research/3d-kinetic-typography-architecture.md` Bölüm 3.E / Faz 6C) kapsamında **0 yeni Cargo crate** veya minimal saf-Rust şema genişlemesi ile müzik parçalarından temiz vokal/enstrümantal/davul/bas stem ayrımı yapan ve İngilizce ile Japonca (anime OP/ED/insert song) vokallerden **`Flicks(i64)` tabanlı kelime-düzeyi söz zamanlama (`word_timestamps`)** çıkaran harici Python AI sidecar + `frameshift-cli` + `frameshift-mcp` mimarisi nasıl tasarlanmalıdır?

Özellikle şu dört teknik soru yanıtlanmıştır:
1. **Stem ayrımı:** `demucs` v4 (`htdemucs` / `htdemucs_ft`) ile `python-audio-separator` (`audio-separator` — ONNX MDX-Net / BS-RoFormer / Demucs) arasındaki kalite, lisans, disk/kurulum boyutu ve Windows + RTX 4050 (6 GB VRAM) üzerinde DirectML/CUDA/CPU uyumu farkları nelerdir; mevcut `onnxruntime-directml` kurulumuyla nasıl sinerji kurulur?
2. **Kelime-düzeyi söz zamanlama:** `faster-whisper` vs `stable-ts` vs `whisper-timestamped` arasında İngilizce ve Japonca (boşluksuz/morfem bazlı) anime vokallerinde kelime zamanlama doğruluğu, known-lyrics forced alignment desteği, VRAM kullanımı ve lisans uyumu nasıldır?
3. **Yerel Python 3.14 ortam kısıtları:** Mevcut Windows ortamındaki Python 3.14.6 + `onnxruntime-directml 1.24.4` kurulumu ile PyPI wheel uyumluluğu nasıl çözülmelidir?
4. **FrameShift entegrasyonu:** Sidecar CLI/JSON sözleşmesi (`Flicks(i64)` + güven skoru), `frameshift-cli` ve `frameshift-mcp` yüzeyleri ile `VectorText` / `Text3D` kinetik söz şablonu nasıl tasarlanmalıdır?

---

## İncelenen Birincil Kaynaklar

### Harici Birincil Kaynaklar (GitHub, PyPI JSON API & Dokümantasyon — İnceleme: 2026-11-24)
- [facebookresearch/demucs (GitHub — MIT kod, MUSDB18-HQ eğitim ağırlıkları uyarısı)](https://github.com/facebookresearch/demucs) — PyPI: [demucs 4.1.0](https://pypi.org/pypi/demucs/json)
- [nomadkaraoke/python-audio-separator (GitHub — MIT kod, UVR/BS-RoFormer/MDX-Net/Demucs sarmalayıcı)](https://github.com/nomadkaraoke/python-audio-separator) — PyPI: [audio-separator 0.48.0](https://pypi.org/pypi/audio-separator/json)
- [SYSTRAN/faster-whisper (GitHub — MIT)](https://github.com/SYSTRAN/faster-whisper) — PyPI: [faster-whisper 1.2.1](https://pypi.org/pypi/faster-whisper/json)
- [OpenNMT/CTranslate2 (GitHub — MIT)](https://github.com/OpenNMT/CTranslate2) — PyPI: [ctranslate2 4.9.1](https://pypi.org/pypi/ctranslate2/json)
- [jianfch/stable-ts (GitHub — MIT)](https://github.com/jianfch/stable-ts) — PyPI: [stable-ts 2.19.1](https://pypi.org/pypi/stable-ts/json)
- [linto-ai/whisper-timestamped (GitHub — AGPL-3.0 + dtw-python GPL-3.0)](https://github.com/linto-ai/whisper-timestamped) — PyPI: [whisper-timestamped 1.15.9](https://pypi.org/pypi/whisper-timestamped/json)
- [ONNX Runtime Execution Providers (DirectML / CUDA / CPU)](https://onnxruntime.ai/docs/execution-providers/) — PyPI: [onnxruntime-directml 1.24.4](https://pypi.org/pypi/onnxruntime-directml/json), [torch-directml 0.2.5.dev240914](https://pypi.org/pypi/torch-directml/json), [torch 2.15.0](https://pypi.org/pypi/torch/json)
- [polm/fugashi (PyPI — MIT, MeCab Japonca morfem çözücü)](https://pypi.org/project/fugashi/)

### Yerel Depo ve Ortam İncelemesi (2026-11-24)
- [scripts/ai_infer_sidecar.py](../../scripts/ai_infer_sidecar.py) — mevcut ONNX raw-pipe sidecar (`probe_environment()`, `select_providers()`)
- [crates/frameshift-media/src/ai_inference.rs](../../crates/frameshift-media/src/ai_inference.rs) — `AiTaskKind`, `AiCapabilitiesReport`, `AiInferenceConfig`, `detect_ai_capabilities()`
- [crates/frameshift-media/src/audio.rs](../../crates/frameshift-media/src/audio.rs) — `AudioBuffer`, `AudioAnalysisReport`, `analyze_beats_and_energy()`
- [crates/frameshift-cli/src/workbench.rs](../../crates/frameshift-cli/src/workbench.rs) & [crates/frameshift-cli/src/main.rs](../../crates/frameshift-cli/src/main.rs) — `ai-status`, `ai-matte`, `interpolate`, `detected_event_anchors()`
- [crates/frameshift-mcp/src/server.rs](../../crates/frameshift-mcp/src/server.rs) — `McpServer`, `list_tools()`, `dispatch_tool()`, `ai_capabilities()`, `analyze_audio()`
- [crates/frameshift-schema/src/project.rs](../../crates/frameshift-schema/src/project.rs) & [crates/frameshift-schema/src/graph.rs](../../crates/frameshift-schema/src/graph.rs) — `EventAnchorDef`, `EventOrigin`, `ParamBindingDef`, `AudioClipDef`, `NodeKind::VectorText`, `NodeKind::Text3D`, `TextAnimatorDef`
- [crates/frameshift-time/src/lib.rs](../../crates/frameshift-time/src/lib.rs) — `Flicks`, `FLICKS_PER_SECOND` (`705_600_000`)
- [brain/research/3d-kinetic-typography-architecture.md](3d-kinetic-typography-architecture.md) & [brain/research/2026-11-24-optical-flow-and-ai-inference-architecture.md](2026-11-24-optical-flow-and-ai-inference-architecture.md)

---

## Doğrudan Gözlemler (Kanıtlı)

### 1. Yerel Çalışma Ortamı ve Donanım Gerçekliği
Salt-okunur sistem sorgularıyla doğrulanan yerel ortam durumu:
- **GPU:** `NVIDIA GeForce RTX 4050 Laptop GPU` (**6141 MiB / ~6 GB VRAM**, sürücü `616.92`). Hem NVIDIA **CUDA** hem Windows **DirectML** donanım hızlandırmasını destekler.
- **Python Yorumlayıcıları ve Araçlar (`py --list` & `where.exe`):**
  - Varsayılan `python`: **Python 3.14.6 (64-bit)** — kurulu paketler arasında `onnxruntime-directml 1.24.4` (`['DmlExecutionProvider', 'CPUExecutionProvider']`), `numpy 2.5.2`, `scipy 1.18.1`, `soundfile 0.12.1` bulunur.
  - Alternatif kurulu sürümler: **Python 3.12 (64-bit)** (`py -3.12`, temel ortamında `numpy 2.4.6`, `scipy 1.17.1` mevcut) ve **Astral/CPython 3.11.15**.
  - Paket yöneticisi: **`uv.exe`** (`C:\Users\Yigit\.local\bin\uv.exe`) ve **`ffmpeg`** (`9.0.2-full_build-www.gyan.dev`) sistemde hazırdır.
- **Mevcut Modeller ve Sidecar Altyapısı:**
  - `local_media/models/` altında `isnetis.onnx` (176.1 MB) ve `depth_anything_v2_small.onnx` (99.1 MB) bulunur.
  - `scripts/ai_infer_sidecar.py` yalnızca görsel `rotoscope` ve `depth` görevlerini destekler; `detect_ai_capabilities()` (`crates/frameshift-media/src/ai_inference.rs`) yalnızca `ffmpeg`, `rife-ncnn-vulkan`, `python`, `ai_infer_sidecar.py` ve `onnxruntime` provider listesini raporlar. Henüz stem ayrımı veya ASR/söz zamanlama paketi kurulu değildir.

### 2. PyPI Wheel ve Bağımlılık Çakışma Bulguları (Python 3.14 vs 3.12)
PyPI JSON API (`https://pypi.org/pypi/<pkg>/json`) sorgularından doğrudan gözlenen kritik uyum gerçekleri:

| Paket (PyPI Sürüm) | `requires_python` | Lisans (Kod / Ağırlık) | Windows `win_amd64` Wheel / Kritik Bağımlılık Bulgusu |
| :--- | :--- | :--- | :--- |
| `demucs` (`4.1.0`) | `>=3.10` | **MIT** (kod) / MUSDB18-HQ eğitim ağırlıkları ticari olmayan veri seti uyarısı içerir | `py3-none-any`; `torch>=2.1`, `einops`, `julius>=0.2.3`, `safetensors`, `sphn>=0.1.12` ister (`torchaudio` yalnızca `[train]` extrasında). Upstream repo (`facebookresearch/demucs`) düşük bakım modundadır. |
| `audio-separator` (`0.48.0`) | `!=3.14.1,>=3.10` | **MIT** (kod) / Model ağırlıkları yaratıcısına bağlıdır (UVR MIT+atıf veya belirsiz topluluk ağırlıkları) | `py3-none-any`; Windows'ta `diffq-fixed>=0.2` ister (`diffq-fixed 0.2.4` yalnızca `cp310`–`cp313` wheel sunar, **`cp314` wheel'i yoktur**). `[dml]` extrası `torch_directml` ister (`torch_directml 0.2.5.dev240914` yalnızca `cp38`–`cp312` wheel sunar, **Python 3.14'te kurulamaz**). |
| `onnxruntime-directml` (`1.24.4`) | `>=3.11` | **MIT** | `cp311`–`cp314` `win_amd64` wheel'leri vardır ve Python 3.14.6'da zaten kuruludur. |
| `faster-whisper` (`1.2.1`) | `>=3.9` | **MIT** | `py3-none-any`; `ctranslate2<5,>=4.0`, `onnxruntime<2,>=1.14`, `av>=11`, `tokenizers`, `huggingface-hub` ister. **`torch` gerektirmez.** |
| `ctranslate2` (`4.9.1`) | `>=3.9` | **MIT** | `cp39`–`cp315` `win_amd64` wheel'leri vardır. **CUDA ve CPU destekler; DirectML backend'i yoktur.** |
| `stable-ts` (`2.19.1`) | `>=3.8` | **MIT** | sdist; varsayılan olarak `torch`, `torchaudio`, `openai-whisper` ister; `[fw]` extrası `faster-whisper` ekler. |
| `whisper-timestamped` (`1.15.9`) | `>=3.7` | **AGPL-3.0** (repo) / **GPLv3** (PyPI + `dtw-python`) | Güçlü copyleft lisansı (`AGPL-3.0`/`GPLv3`) nedeniyle kapalı/ticari dağıtım hedefleyen FrameShift çekirdeği veya dağıtımlı paketlerle uyumsuzdur. |

**Kritik DLL Çakışma Riski:** `faster-whisper` bağımlılık listesinde `onnxruntime<2,>=1.14` bulunduğundan, mevcut Python 3.14 ortamına doğrudan `pip install faster-whisper` çalıştırmak `onnxruntime-directml 1.24.4`'ün `onnxruntime` C++ DLL'lerini ezerek `scripts/ai_infer_sidecar.py` DirectML rotoscope/depth çıkarımını bozabilir.

### 3. Stem Ayrım Motorları Karşılaştırması (Windows RTX 4050 / 6 GB VRAM)

| Kriter | `demucs` v4 (`htdemucs` / `htdemucs_ft`) | `audio-separator` + ONNX MDX-Net (ör. `UVR-MDX-NET-Voc_FT.onnx` / `Inst_HQ_3`) | `audio-separator` + BS-RoFormer / MelBand RoFormer |
| :--- | :--- | :--- | :--- |
| **Çıktı Stemleri** | 4 stem (`vocals`, `drums`, `bass`, `other`) veya 6 stem (`htdemucs_6s`) — beat/kick ayrımı ile Vokal ayrımını tek seferde verir | 2 stem (`Vocals` + `Instrumental`) | 2 stem (`Vocals` + `Instrumental`) — vokal temizliğinde en yüksek SDR |
| **Donanım Backend'i** | PyTorch CUDA (hızlı) veya CPU; DirectML için `torch-directml` (`<=py3.12`, deneysel ve yavaş) | **`onnxruntime-directml`** (`DmlExecutionProvider`) ile doğrudan GPU hızlandırma; PyTorch CUDA gerekmez | PyTorch CUDA (önerilen); `torch-directml` üzerinde yüksek VRAM veya op fallback riski |
| **Model Ağırlık Boyutu** | `htdemucs` / `htdemucs_ft`: ~300–320 MB; `htdemucs_6s`: ~480 MB | ~40–150 MB (standart `.onnx`); ~300–500 MB (HQ `.onnx`) | ~1.2–1.5 GB (`.ckpt` / `.safetensors`) |
| **Python / Paket Ayak İzi** | `torch` (~2.5 GB CUDA wheel veya ~200 MB CPU) + `demucs` | Saf ONNX modunda mevcut `onnxruntime-directml` + `soundfile` + `scipy` ile çalıştırılabilir (~0–200 MB ek paket); `audio-separator` tam kurulumda `torch` çeker | `audio-separator` + `torch` + `rotary-embedding-torch` (~2.5+ GB) |
| **RTX 4050 (6 GB VRAM) Uyumu** | Rahat (~4 GB önerilir; `segment` ile düşürülebilir). `htdemucs_ft` 4× daha yavaştır | Çok rahat (4 GB+ VRAM, düşük bellek, hızlı) | Sınırda (6–8 GB+ önerilir; büyük segmentte OOM riski) |
| **Mevcut Sidecar Sinerjisi** | Ayrı PyTorch çalışma zamanı gerektirir | Mevcut `onnxruntime-directml 1.24.4` (`DmlExecutionProvider`) ile tam uyumlu; hafif modda `torch` olmadan da `.onnx` çalıştırılabilir | Ayrı PyTorch CUDA venv gerektirir |

### 4. Söz Zamanlama ve Forced Alignment Karşılaştırması

1. **`faster-whisper` (CTranslate2, MIT):**
   - `word_timestamps=True` ile token olasılıkları + cross-attention DTW üzerinden kelime başlangıç/bitiş ve olasılık (`probability`) üretir.
   - `large-v3-turbo` VRAM kullanımı: **`float16` ~2,537 MB (~2.5 GB)**, **`int8` / `int8_float16` ~1,545 MB (~1.5 GB)** — 6 GB RTX 4050'de hem `float16` hem `int8_float16` ile çok rahat çalışır.
   - **Sınırlama:** Yalnızca serbest ASR transkripsiyonu yapar; eldeki doğru şarkı sözü metnini (`known_lyrics.txt`) vokale zorunlu hizalayan (**forced alignment**) bir `.align()` API'si yoktur. Ayrıca CTranslate2 **DirectML desteklemez** (NVIDIA CUDA veya CPU).
2. **`stable-ts` (`stable_whisper`, MIT):**
   - `transcribe_stable(..., vad=True)` ile sessizlik/enstrümantal boşluklarda zaman damgası sürüklenmesini (drift) Silero VAD ile bastırır.
   - **`model.align(audio, text, language=...)` (Forced Alignment):** Şarkı sözleri biliniyorsa (ör. anime OP/ED sözleri veya Genius/Musixmatch metni), ASR'nin yanlış kelime uydurmasını önleyerek doğrudan verilen kelimeleri vokale hizalar (`WordTiming` başlangıç, bitiş, olasılık).
   - **Kritik API Sınırı:** GitHub bulgularına göre `stable_whisper.load_faster_whisper()` bir `faster-whisper` `WhisperModel` sarmalayıcısı döndürür ve üzerinde `.align()` metodu yoktur (`AttributeError: 'WhisperModel' object has no attribute 'align'`). Forced alignment (`.align()`) için PyTorch tabanlı `stable_whisper.load_model()` gerekir!
3. **`whisper-timestamped` (`linto-ai`, AGPL-3.0 / GPLv3):**
   - `dtw-python` (GPLv3) ve kendi **AGPL-3.0** lisansı nedeniyle FrameShift için elenmiştir.
4. **İngilizce vs. Japonca Anime Vokalleri:**
   - İngilizce vokallerde boşluklar kelime sınırını doğal verir; hızlı rap/EDM/melisma uzatmalarında çıplak ASR halüsinasyon yapabileceği için stem ayrımı (`vocals.wav`) sonrası `stable-ts` `.align()` çok daha tutarlıdır.
   - Japonca boşluksuz yazıldığından Whisper BPE alt-kelime tokenları kanjileri/kanaları keyfi böler (`kanji` + `okurigana` veya hece parçaları). Hem `stable-ts` hem doğru lyric kartı üretimi için Japonca metnin hizalamadan/gruplamadan önce **`fugashi` (MeCab + `unidic-lite`, MIT)** ile morfem/kelime sınırlarına bölünmesi veya kana/kelime gruplama kuralı uygulanması gerekir.

### 5. Mevcut FrameShift Kod Tabanı Bağlantı Noktaları
- `crates/frameshift-media/src/ai_inference.rs`: `AiTaskKind`, `AiCapabilitiesReport`, `AiInferenceConfig`, `detect_ai_capabilities()` mevcuttur; CLI'da `crates/frameshift-cli/src/workbench.rs` (`ai-status`, `ai-matte`, `interpolate`) ve MCP'de `crates/frameshift-mcp/src/server.rs` (`ai_capabilities`, `analyze_audio`, `mutate_project`) üzerinden sunulur.
- `crates/frameshift-schema/src/project.rs`: `EventAnchorDef` (`event_id`, `label`, `source_clip_id`, `source_anchor_tick`, `target_output_tick`, `origin: EventOrigin::Detected`) ve `ParamBindingDef` (`node_id`, `param_name`, `expression`) halihazırda mevcuttur.
- `crates/frameshift-schema/src/graph.rs`:
  - `NodeKind::VectorText` üzerinde `text`, `font_size_norm`, `color_rgba` (lineer Rec.709!), `x_offset_norm` / `y_offset_norm` (0.5 = merkez!), `opacity`, `writing_mode` (`WritingMode2D` — Japonca dikey `VerticalRl` Tategaki destekli), `text_animator: Option<TextAnimatorDef>` alanları vardır.
  - `TextAnimatorDef` tekdüze aralıklarla (`start_tick`, `duration_ticks`, `delay_ticks`) grafem/kelime animasyonu yapar; ancak her kelimenin farklı müzikal sürede tutulduğu vokal senkronunda kelime/satır başına ayrı `VectorText` veya `Text3D` düğümü + `ParamBindingDef` (`SafeExpr` `step(t_start, t_sec) * (1 - step(t_end, t_sec))`) veya `EventAnchorDef` zamanlaması gerekir.
  - `Text3D` düğümü `time_offset_sec`, `pop_duration_sec`, `pop_overshoot`, `char_stagger_sec` ile doğrudan kelime bazlı 3D kinetik pop animasyonu destekler.

---

## Çıkarımlar ve Öneriler

*(Aşağıdaki öneriler yukarıdaki kanıtlı gözlemlere dayalı mimari çıkarımlardır; Python bağımlılık kurulumu ve Rust arayüz eklemeleri için Bölüm 7'deki kullanıcı kararları beklenmektedir.)*

### 1. Önerilen İki Katmanlı AI Audio Mimari Yaklaşımı

#### A. Ortam İzolasyonu (Python 3.14 Çakışma Koruması)
- Mevcut varsayılan **Python 3.14.6** (`onnxruntime-directml 1.24.4`) ortamı **bozulmadan** `scripts/ai_infer_sidecar.py` (rotoscope/depth) için korunmalıdır.
- Stem ayrımı ve ASR/alignment tarafı için **`uv` + Python 3.12 (`py -3.12`)** üzerinde proje içi izole bir sanal ortam (`local_media/.venv-audio` veya `%LOCALAPPDATA%/FrameShift/venv-audio`) önerilir.
  - **Neden Python 3.12?** `audio-separator` bağımlılığı olan `diffq-fixed` (`cp312-win_amd64`), `torch` CUDA (`cp312-win_amd64`), `ctranslate2` (`cp312-win_amd64`), `stable-ts` ve `fugashi` Python 3.12'de sorunsuz binary wheel'lere sahiptir; Python 3.14'teki `diffq-fixed` / `torch-directml` eksikliği ve `onnxruntime` DLL ezilme riski tamamen ortadan kalkar.
- **Alternatif Hafif Yol (0 PyTorch):** Sadece 2-stem (`vocals` + `instrumental`) isteniyorsa, `UVR-MDX-NET-Voc_FT.onnx` + `onnxruntime-directml` doğrudan mevcut Python 3.14 ortamında PyTorch indirmeden çalıştırılabilir; ancak `htdemucs` 4-stem (davul/bas ayrımı) ve `stable-ts` forced alignment (`.align()`) için `Python 3.12` + `CUDA PyTorch` gereklidir.

#### B. Stem Ayrımı Motoru Önerisi
- **Önerilen Birincil Motor:** **`audio-separator` (v0.48+, MIT kod)** sarmalayıcısı üzerinden çift profil:
  1. **Hızlı / Varsayılan (`vocals_mdx`):** ONNX MDX-Net (`UVR-MDX-NET-Voc_FT.onnx` veya `Kim_Vocal_2.onnx`) — düşük VRAM, hızlı 2-stem (`vocals.wav` + `instrumental.wav`) çıkarımı; söz hizalama öncesi temiz vokal elde etmek için ideal.
  2. **4-Stem AMV/Beat Modu (`htdemucs` / `htdemucs_ft`):** `vocals`, `drums`, `bass`, `other` ayrımı — özellikle `drums.wav` + `bass.wav` sinyallerini `AudioBuffer::analyze_beats_and_energy` beslemesine vererek enstrümantal gürültüsüz beat/drop tespiti yapılabilir.
  3. **Yüksek Kalite (`bs_roformer`, opsiyonel):** RTX 4050 6 GB'da `segment_size` kısıtı ile en temiz acapella çıkarımı (model boyutu ~1.2 GB olduğu için yalnızca kullanıcı isterse).

#### C. Söz Zamanlama Motoru Önerisi (Hibrit `faster-whisper` + `stable-ts`)
- **Senaryo 1 — Sözler Bilinmiyor (Transkripsiyon):** `faster-whisper` (`large-v3-turbo`, CUDA `float16` / `int8_float16`) veya `stable_whisper.load_faster_whisper("large-v3-turbo")` ile hızlı kelime-düzeyi zaman damgası + Silero VAD.
- **Senaryo 2 — Şarkı Sözleri Biliniyor (Anime Edit / Kinetik Tipografi İçin Önerilen Varsayılan):** `stable-ts` (`stable_whisper.load_model("small" | "medium" | "large-v3-turbo")`) ile **`model.align(vocals_wav, lyrics_text, language="en" | "ja")`** forced alignment!
  - Bilinen sözleri müzikte uydurma olmadan doğrudan vokal enerji penceresine oturtur.
  - Japonca (`language="ja"`) modunda opsiyonel `fugashi` (MeCab) ön-bölümlemesi ile boşluksuz dizgiden gerçek morfem/kelime dizisi üretilir.

### 2. Sidecar CLI / JSON Sözleşmesi (`scripts/ai_audio_sidecar.py`)

Görsel sidecar (`scripts/ai_infer_sidecar.py`) ile tutarlı, ayrı sorumluluklu yeni Python sidecar: `scripts/ai_audio_sidecar.py`.

#### Alt Komutlar
- `python scripts/ai_audio_sidecar.py --probe` → JSON ortam yetenek raporu (`demucs`, `audio_separator`, `faster_whisper`, `stable_whisper`, `fugashi`, `torch_cuda`, `onnx_providers`).
- `python scripts/ai_audio_sidecar.py separate --input <audio_or_video> --out-dir <dir> [--backend auto|audio-separator|demucs|mdx-onnx] [--model htdemucs|htdemucs_ft|UVR-MDX-NET-Voc_FT.onnx|bs_roformer] [--stems 2|4] [--device auto|cuda|dml|cpu]` → stdout'a JSON `StemSeparationManifest`.
- `python scripts/ai_audio_sidecar.py align --input <vocals.wav> [--lyrics <lyrics.txt>] [--language en|ja|auto] [--backend stable-ts|faster-whisper] [--model large-v3-turbo|medium|small] [--compute-type float16|int8_float16|int8] [--device auto|cuda|cpu] [--ja-tokenizer fugashi|char|raw] [--vad]` → stdout'a JSON `LyricAlignmentManifest`.

#### JSON Sözleşmesi (Sidecar Çıktısı & `Flicks(i64)` Eşlemesi)
Sidecar Python katmanı milisaniye ve örnek (48 kHz) kesinliğinde saniye/örnek döndürür; **kesin `Flicks(i64)` dönüşümü (`round(sec * 705_600_000)`) deterministik olarak Rust çekirdeğinde (`frameshift-time`) yapılır** (böylece `f64` yuvarlama hataları proje şemasına sızmaz; tam `44.1 kHz` / `48 kHz` / `24/30/60 fps` ızgarasına hizalanır):

```json
{
  "schema": "frameshift.ai_audio.lyrics.v1",
  "source_audio": "vocals.wav",
  "language": "ja",
  "backend": "stable-ts",
  "model": "large-v3-turbo",
  "duration_sec": 31.533333,
  "segments": [
    {
      "index": 0,
      "text": "世界が終わるその前に",
      "start_sec": 1.420,
      "end_sec": 3.880,
      "words": [
        { "text": "世界", "start_sec": 1.420, "end_sec": 1.940, "confidence": 0.94, "speaker": null },
        { "text": "が",   "start_sec": 1.940, "end_sec": 2.110, "confidence": 0.89, "speaker": null }
      ]
    }
  ]
}
```

### 3. Somut Rust Struct / Fonksiyon İmzaları (Faz 2 Uygulama Reçetesi)

Aşağıdaki yapılar mevcut `frameshift-media/src/ai_inference.rs` ile aynı desende, **0 yeni Cargo crate** ile (`serde`, `std::process::Command`, `frameshift_time::Flicks`) eklenebilir:

#### A. `crates/frameshift-media/src/ai_audio.rs` (ve `crates/frameshift-media/src/lib.rs` içinde `pub mod ai_audio;`)
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StemBackend {
    #[default]
    Auto,
    AudioSeparator,
    Demucs,
    MdxOnnxDirect,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StemSeparationConfig {
    pub backend: StemBackend,
    pub model: String,          // ör. "htdemucs" veya "UVR-MDX-NET-Voc_FT.onnx"
    pub stems: u8,              // 2 (vocals+inst) veya 4 (vocals,drums,bass,other)
    pub device: String,         // "auto" | "cuda" | "dml" | "cpu"
    pub python_binary: Option<PathBuf>,
    pub sidecar_script: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeparatedStemFile {
    pub role: String,           // "vocals" | "instrumental" | "drums" | "bass" | "other"
    pub path: PathBuf,
    pub sample_rate_hz: u32,
    pub duration_ticks: Flicks,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LyricAlignBackend {
    #[default]
    Auto,            // lyrics varsa stable-ts, yoksa faster-whisper
    StableTs,
    FasterWhisper,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LyricAlignmentConfig {
    pub backend: LyricAlignBackend,
    pub model: String,          // ör. "large-v3-turbo"
    pub language: Option<String>, // ör. Some("en"), Some("ja"), None ("auto")
    pub lyrics_text: Option<String>,
    pub compute_type: String,   // "float16" | "int8_float16" | "int8"
    pub device: String,         // "auto" | "cuda" | "cpu"
    pub vad: bool,
    pub python_binary: Option<PathBuf>,
    pub sidecar_script: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlignedLyricWord {
    pub text: String,
    pub start_tick: Flicks,     // i64 integer Flicks (s * 705_600_000)
    pub end_tick: Flicks,
    pub confidence: f32,        // 0.0..=1.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlignedLyricSegment {
    pub index: usize,
    pub text: String,
    pub start_tick: Flicks,
    pub end_tick: Flicks,
    pub words: Vec<AlignedLyricWord>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LyricAlignmentReport {
    pub language: String,
    pub backend: String,
    pub model: String,
    pub duration_ticks: Flicks,
    pub segments: Vec<AlignedLyricSegment>,
}

pub fn detect_ai_audio_capabilities(python_binary: Option<&Path>) -> AiAudioCapabilitiesReport;
pub fn separate_audio_stems(input: &Path, out_dir: &Path, config: &StemSeparationConfig) -> Result<StemSeparationReport, MediaError>;
pub fn align_audio_lyrics(vocals_audio: &Path, config: &LyricAlignmentConfig) -> Result<LyricAlignmentReport, MediaError>;
```

- `AiCapabilitiesReport` (`crates/frameshift-media/src/ai_inference.rs`) içine veya paralel `AiAudioCapabilitiesReport` içine `audio_python_path`, `audio_separator_available`, `demucs_available`, `faster_whisper_available`, `stable_ts_available`, `fugashi_available`, `cuda_available` alanları eklenir (`#[serde(default)]` ile mevcut JSON tüketicileri kırılmaz).

#### B. `frameshift-cli` Alt Komutları (`crates/frameshift-cli/src/workbench.rs`)
`workbench::dispatch` içine şu 2 yeni alt komut eklenir:
1. `frameshift-cli ai-stems <in.wav|mp4> --out-dir <dir> [--backend auto|audio-separator|demucs|mdx-onnx] [--model <str>] [--stems 2|4] [--device auto|cuda|dml|cpu]`
2. `frameshift-cli ai-lyrics <in.wav|mp4> --out <lyrics.json> [--lyrics <text.txt>] [--lang en|ja|auto] [--backend auto|stable-ts|faster-whisper] [--model large-v3-turbo] [--separate-vocals] [--project <proj.json> --inject-events --out-project <proj_out.json>]`
   - `--separate-vocals` bayrağı tek komutta önce `vocals.wav` ayırıp ardından hizalar.
   - `--inject-events` opsiyonu, mevcut `detected_event_anchors` (`crates/frameshift-cli/src/main.rs`) desenini kullanarak her kelime veya satır için `EventAnchorDef { event_id: format!("lyric_w_{i:04}"), label: word.text, target_output_tick: word.start_tick, origin: EventOrigin::Detected, .. }` kayıtlarını `ProjectCommand::UpsertEventAnchor` ile projeye yazar.

#### C. `frameshift-mcp` Araçları (`crates/frameshift-mcp/src/server.rs`)
`McpServer::list_tools()` ve `dispatch_tool()` içine iki yeni araç eklenir:
1. **`separate_stems`**: `asset_id` veya `audio_path`, `out_dir`, `stems` (2|4), `model` alır; ayrılmış `vocals.wav` / `drums.wav` / `bass.wav` yollarını döner (ajan isterse `drums.wav` üzerinde `analyze_audio` çağırarak vuruşları temiz yakalar).
2. **`align_lyrics`**: `audio_path` (veya `asset_id`), opsiyonel `lyrics_text`, `language` (`"en"` | `"ja"`), `separate_vocals: bool`, `apply_to_project: bool`, `expected_revision_id` alır; `LyricAlignmentReport` (`Flicks` tick'leri + confidence) döner ve `apply_to_project=true` ise optimistic locking (`expected_revision_id`) ile `EventAnchorDef` + opsiyonel kinetik tipografi düğümlerini `mutate_project_as_agent` üzerinden atomik fişle (receipt) projeye yazar.

#### D. `VectorText` / `Text3D` Kinetik Söz Timeline Üretim Şablonu
`frameshift-core/src/workbench.rs` içine eklenecek yardımcı (`build_lyric_typography_nodes`):
1. **Zamanlama ve `SafeExpr` Görünürlük Penceresi:**
   - Her kelime/satır (`start_tick`, `end_tick`) için saniye sınırları `t_start = start_tick as f64 / 705_600_000.0`, `t_end = end_tick as f64 / 705_600_000.0` olarak hesaplanır.
   - **`VectorText` (2D Kinetik Sözler) Şablonu:**
     - Taban mutlaka şeffaf `Solid` (`[0.0, 0.0, 0.0, 0.0]`) olmalıdır (`brain/lessons.md` 2026-11-22 SC-0002 dersi).
     - `x_offset_norm = Some(0.5)`, `y_offset_norm = Some(0.5)` kare merkezidir; `color_rgba` sRGB değil **lineer Rec.709** yazılır.
     - Japonca dikey tipografi için `writing_mode: WritingMode2D::VerticalRl` seçilebilir.
     - Görünürlük `ParamBindingDef` ile: `param_name: "opacity"`, `expression: format!("step({t_start:.6}, t_sec) * (1.0 - step({t_end:.6}, t_sec))")`.
     - Giriş pop'u için `text_animator: Some(TextAnimatorDef { unit: TextSelectorUnit::Grapheme, start_tick: word.start_tick.0, duration_ticks: 84_672_000 /* ~120ms */, delay_ticks: 14_112_000 /* ~20ms */, scale_from: 1.35, opacity_from: 0.0, shape: TextRangeShape::Smooth, .. })` bağlanır.
   - **`Text3D` (3D Kinetik Sözler) Şablonu:**
     - `time_offset_sec: -(t_start as f32)`, `pop_duration_sec: 0.14`, `pop_overshoot: 1.22`, `char_stagger_sec: 0.025` ve `opacity` için yukarıdaki `step(...)` penceresi kullanılır.
   - Tüm üretilen düğümler arka arkaya `NodeKind::Blend` (`BlendMode::Normal`) zinciriyle ana video + efekt hattının üzerine (grade/LUT sonrasına!) bağlanır.

---

## Doğrulanmamış Noktalar

- **Canlı Vokal/Söz Hizalama Sapma Ölçümü:** `audio-separator` (`htdemucs` / `UVR-MDX-NET-Voc_FT.onnx`) + `stable-ts` (`large-v3-turbo`) boru hattı bu oturumda paket kurulumu yasak olduğu için yerel Python ortamında gerçek bir anime OP/ED veya İngilizce AMV şarkısı üzerinde çalıştırılıp ±ms sapma ve RTX 4050 çalışma süresi henüz ölçülmemiştir.
- **`audio-separator` 0.48.0 Python 3.12 `uv` Çözümleme Testi:** PyPI JSON metadata üzerinden `diffq-fixed` (`cp312`) ve `torch` (`cp312`) wheel varlığı doğrulandı; ancak `audio-separator==0.48.0` içindeki `samplerate==0.1.0` sabitinin `uv pip compile` / Windows MSVC derleyici gerektirip gerektirmediği kurulum izni sonrası test edilmelidir.
- **Japonca Şarkılarda `fugashi` + `stable-ts.align()` Kesim Noktaları:** Kanji/hiragana/katakana karışık hızlı anime nakaratlarında `fugashi` morfem bölmesinin müzikal hece/kelime vurgu sınırlarıyla birebir örtüşüp örtüşmediği gerçek test parçasıyla doğrulanmalıdır.

---

## Kullanıcı Kararı Gerektiren Açık Noktalar

1. **Stem Ayrımı Motoru ve Ortam Tercihi:**
   - **Seçenek A (Önerilen Varsayılan):** İzole `Python 3.12` (`uv venv`) içinde **`audio-separator` (CUDA PyTorch + ONNX)** kurulumu — tek arayüzden hem hızlı `UVR-MDX-NET` ONNX vokal ayrımı hem `htdemucs` 4-stem (beat/drop için `drums`/`bass`) destekler.
   - **Seçenek B (Sıfır PyTorch / En Hafif):** Mevcut `Python 3.14` + `onnxruntime-directml` ortamında yalnızca saf ONNX `UVR-MDX-NET-Voc_FT.onnx` çalıştırıcı yazmak (yalnızca 2-stem `vocals/inst` verir, 4-stem `drums/bass` ve PyTorch `stable-ts .align()` vermez).
   - **Seçenek C (Sadece Demucs):** Doğrudan `facebookresearch/demucs` (4-stem, BS-RoFormer / UVR MDX-Net yok).
2. **Söz Zamanlama Stratejisi:**
   - **Seçenek A (Önerilen Varsayılan — Hibrit):** Bilinen sözler (`--lyrics`) varsa **`stable-ts` (`load_model` + `.align()`)** ile sıfır halüsinasyonlu forced alignment; sözler yoksa **`faster-whisper` (`large-v3-turbo`, CUDA `float16`/`int8_float16`)** ile otomatik transkripsiyon + Japonca için opsiyonel **`fugashi` (`unidic-lite`)** kelime segmentasyonu.
   - **Seçenek B (Yalnızca `faster-whisper`):** Kurulumu en hafif (`torch` gerektirmez, yalnız `ctranslate2`), ancak eldeki doğru şarkı sözlerini zorunlu hizalama (`.align()`) yeteneği yoktur.
3. **Python Sanal Ortam (`uv venv`) Kurulumu ve Otomatik Model İndirme Politikası:**
   - **Seçenek A (Önerilen Varsayılan):** `local_media/.venv-audio` (Python 3.12) kurulumu ve model ağırlıklarının (`htdemucs` ~310 MB, `UVR-MDX-NET-Voc_FT.onnx` ~65 MB, `whisper large-v3-turbo` ~1.5 GB) kullanıcı onayıyla yalnızca `local_media/models/` altına indirilmesi (`FRAMESHIFT_OFFLINE=1` modunda indirme engelli).
   - **Seçenek B (Tamamen Manuel / Çevrimdışı):** Kullanıcı paket ve modelleri elle indirir; sidecar eksik modelde açıklayıcı hata + indirme URL'si döner.
