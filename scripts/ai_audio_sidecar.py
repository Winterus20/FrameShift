#!/usr/bin/env python3
"""FrameShift AI Audio Sidecar (Stem Separation & Word-Level Lyric Alignment).

Subcommands / Modes:
  --probe      Print JSON environment & GPU/package capability report and exit.
  separate     Separate audio/video into 2 or 4 stems via audio-separator (MDX-Net / htdemucs)
               or demucs, caching models under local_media/models/.
  align        Hybrid word-level lyric alignment:
               - Known lyrics (--lyrics / --lyrics-text) -> stable-ts forced alignment (.align())
               - Unknown lyrics -> faster-whisper transcription + word_timestamps
               - Japanese (--language ja) -> fugashi + unidic-lite morpheme segmentation.

All diagnostic/progress output is redirected to stderr so stdout emits clean JSON only.
"""

from __future__ import annotations

import argparse
import contextlib
import importlib
import importlib.metadata
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
from typing import Any
import warnings
import wave

warnings.filterwarnings("ignore", category=SyntaxWarning)


SCHEMA_PROBE = "frameshift.ai_audio.probe.v1"
SCHEMA_STEMS = "frameshift.ai_audio.stems.v1"
SCHEMA_LYRICS = "frameshift.ai_audio.lyrics.v1"

DEFAULT_MDX_MODEL = "UVR-MDX-NET-Voc_FT.onnx"
DEFAULT_HTDEMUCS_MODEL = "htdemucs.yaml"
DEFAULT_WHISPER_MODEL = "large-v3-turbo"


def _repo_root() -> Path:
    return Path(__file__).resolve().parent.parent


def _default_model_cache_dir() -> Path:
    return _repo_root() / "local_media" / "models"


def _is_offline(flag_offline: bool) -> bool:
    if flag_offline:
        return True
    val = os.environ.get("FRAMESHIFT_OFFLINE", "").strip().lower()
    return val in {"1", "true", "yes", "on"}


def _pkg_version(dist_name: str) -> str | None:
    try:
        return importlib.metadata.version(dist_name)
    except importlib.metadata.PackageNotFoundError:
        return None
    except Exception:
        return None


def _try_import(module_name: str) -> Any | None:
    try:
        return importlib.import_module(module_name)
    except Exception:
        return None


def _probe_audio_wav_info(path: Path) -> tuple[int, float]:
    """Reads sample_rate_hz and duration_sec from WAV via stdlib wave, falling back to soundfile/ffprobe."""
    try:
        with wave.open(str(path), "rb") as wf:
            sr = int(wf.getframerate())
            frames = int(wf.getnframes())
            dur = float(frames / sr) if sr > 0 else 0.0
            return sr, dur
    except Exception:
        pass

    sf = _try_import("soundfile")
    if sf is not None:
        try:
            info = sf.info(str(path))
            return int(info.samplerate), float(info.duration)
        except Exception:
            pass

    ffprobe = shutil.which("ffprobe")
    if ffprobe:
        try:
            out = subprocess.check_output(
                [
                    ffprobe,
                    "-v",
                    "error",
                    "-select_streams",
                    "a:0",
                    "-show_entries",
                    "stream=sample_rate,duration",
                    "-of",
                    "json",
                    str(path),
                ],
                stderr=subprocess.DEVNULL,
            )
            data = json.loads(out.decode("utf-8", errors="replace"))
            stream = (data.get("streams") or [{}])[0]
            sr = int(float(stream.get("sample_rate", 48000)))
            dur = float(stream.get("duration", 0.0))
            return sr, dur
        except Exception:
            pass

    return 48000, 0.0


def _ensure_wav_input(input_path: Path, work_dir: Path) -> Path:
    """If input is a video or non-WAV audio, extracts 44.1/48 kHz PCM WAV via ffmpeg when available."""
    if input_path.suffix.lower() == ".wav":
        return input_path
    ffmpeg = shutil.which("ffmpeg")
    if not ffmpeg:
        return input_path
    extracted = work_dir / f"{input_path.stem}_input.wav"
    cmd = [
        ffmpeg,
        "-y",
        "-i",
        str(input_path),
        "-vn",
        "-acodec",
        "pcm_s16le",
        "-ar",
        "44100",
        "-ac",
        "2",
        str(extracted),
    ]
    subprocess.run(cmd, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    return extracted


def _configure_cache_env(model_dir: Path, offline: bool) -> None:
    model_dir.mkdir(parents=True, exist_ok=True)
    os.environ.setdefault("HF_HOME", str(model_dir / "hf_cache"))
    os.environ.setdefault("TORCH_HOME", str(model_dir / "torch_cache"))
    if offline:
        os.environ["HF_HUB_OFFLINE"] = "1"
        os.environ["TRANSFORMERS_OFFLINE"] = "1"


def probe_environment(model_dir: Path | None = None) -> dict[str, Any]:
    cache_dir = model_dir or _default_model_cache_dir()
    info: dict[str, Any] = {
        "schema": SCHEMA_PROBE,
        "python_version": sys.version.split()[0],
        "python_executable": sys.executable,
        "torch_available": False,
        "torch_version": None,
        "torch_cuda_available": False,
        "cuda_device_name": None,
        "audio_separator_available": False,
        "audio_separator_version": None,
        "demucs_available": False,
        "demucs_version": None,
        "faster_whisper_available": False,
        "faster_whisper_version": None,
        "stable_whisper_available": False,
        "stable_whisper_version": None,
        "fugashi_available": False,
        "unidic_lite_available": False,
        "onnxruntime_available": False,
        "onnxruntime_version": None,
        "onnx_providers": [],
        "ffmpeg_available": shutil.which("ffmpeg") is not None,
        "model_cache_dir": str(cache_dir),
    }

    torch = _try_import("torch")
    if torch is not None:
        info["torch_available"] = True
        info["torch_version"] = getattr(torch, "__version__", _pkg_version("torch"))
        try:
            cuda_ok = bool(torch.cuda.is_available())
            info["torch_cuda_available"] = cuda_ok
            if cuda_ok:
                info["cuda_device_name"] = str(torch.cuda.get_device_name(0))
        except Exception:
            pass

    audio_sep = _try_import("audio_separator")
    if audio_sep is not None:
        info["audio_separator_available"] = True
        info["audio_separator_version"] = _pkg_version("audio-separator") or getattr(
            audio_sep, "__version__", None
        )

    demucs = _try_import("demucs")
    if demucs is not None:
        info["demucs_available"] = True
        info["demucs_version"] = _pkg_version("demucs")

    fw = _try_import("faster_whisper")
    if fw is not None:
        info["faster_whisper_available"] = True
        info["faster_whisper_version"] = _pkg_version("faster-whisper") or getattr(
            fw, "__version__", None
        )

    sw = _try_import("stable_whisper")
    if sw is not None:
        info["stable_whisper_available"] = True
        info["stable_whisper_version"] = _pkg_version("stable-ts") or getattr(
            sw, "__version__", None
        )

    fugashi = _try_import("fugashi")
    if fugashi is not None:
        info["fugashi_available"] = True

    unidic_lite = _try_import("unidic_lite")
    if unidic_lite is not None:
        info["unidic_lite_available"] = True

    ort = _try_import("onnxruntime")
    if ort is not None:
        info["onnxruntime_available"] = True
        info["onnxruntime_version"] = getattr(ort, "__version__", None)
        try:
            info["onnx_providers"] = list(ort.get_available_providers())
        except Exception:
            pass

    return info


def _classify_stem_role(filename: str) -> str:
    lower = filename.lower()
    for role in ("vocals", "instrumental", "drums", "bass", "other", "guitar", "piano"):
        if f"({role})" in lower or f"_{role}" in lower or f"-{role}" in lower or lower.startswith(role):
            return role
    if "no_vocals" in lower or "accompaniment" in lower or "inst" in lower:
        return "instrumental"
    return "other"


def _resolve_device(requested: str) -> str:
    if requested != "auto":
        return requested
    torch = _try_import("torch")
    if torch is not None:
        try:
            if torch.cuda.is_available():
                return "cuda"
        except Exception:
            pass
    ort = _try_import("onnxruntime")
    if ort is not None:
        try:
            providers = ort.get_available_providers()
            if "CUDAExecutionProvider" in providers:
                return "cuda"
            if "DmlExecutionProvider" in providers:
                return "dml"
        except Exception:
            pass
    return "cpu"


def run_separate(args: argparse.Namespace) -> dict[str, Any]:
    input_path = Path(args.input).resolve()
    if not input_path.is_file():
        raise FileNotFoundError(f"input audio/video file not found: {input_path}")

    out_dir = Path(args.out_dir).resolve()
    out_dir.mkdir(parents=True, exist_ok=True)
    model_dir = Path(args.model_dir).resolve() if args.model_dir else _default_model_cache_dir()
    offline = _is_offline(args.offline)
    _configure_cache_env(model_dir, offline)

    stems_req = int(args.stems)
    backend = args.backend
    if backend == "mdx-onnx":
        backend = "audio-separator"
        if not args.model or args.model == "auto":
            args.model = DEFAULT_MDX_MODEL
    if backend == "auto":
        if _try_import("audio_separator") is not None:
            backend = "audio-separator"
        elif _try_import("demucs") is not None:
            backend = "demucs"
        else:
            raise RuntimeError(
                "Neither audio-separator nor demucs is installed in the current Python environment"
            )

    model_name = args.model
    if not model_name or model_name == "auto":
        if backend == "demucs":
            model_name = "htdemucs"
        elif stems_req == 4:
            model_name = DEFAULT_HTDEMUCS_MODEL
        else:
            model_name = DEFAULT_MDX_MODEL

    device = _resolve_device(args.device)

    produced_files: list[Path] = []
    if backend == "audio-separator":
        from audio_separator.separator import Separator

        audio_in = _ensure_wav_input(input_path, out_dir)
        sep_kwargs: dict[str, Any] = {
            "model_file_dir": str(model_dir),
            "output_dir": str(out_dir),
            "output_format": args.output_format.upper(),
        }
        if device == "cpu":
            sep_kwargs["use_autocast"] = False

        with contextlib.redirect_stdout(sys.stderr):
            separator = Separator(**sep_kwargs)
            separator.load_model(model_filename=model_name)
            raw_outputs = separator.separate(str(audio_in))

        for item in raw_outputs:
            p = Path(item)
            if not p.is_absolute():
                p = out_dir / p
            if p.is_file():
                produced_files.append(p.resolve())
    elif backend == "demucs":
        demucs_out = out_dir / "_demucs_tmp"
        demucs_out.mkdir(parents=True, exist_ok=True)
        cmd = [
            sys.executable,
            "-m",
            "demucs.separate",
            "-n",
            model_name.removesuffix(".yaml"),
            "-o",
            str(demucs_out),
            "-d",
            "cuda" if device == "cuda" else "cpu",
        ]
        if stems_req == 2:
            cmd.extend(["--two-stems", "vocals"])
        cmd.append(str(input_path))
        subprocess.run(cmd, check=True, stdout=sys.stderr, stderr=sys.stderr)
        for p in sorted(demucs_out.rglob("*.wav")):
            target = out_dir / f"{input_path.stem}_{p.stem}.wav"
            shutil.move(str(p), str(target))
            produced_files.append(target.resolve())
        shutil.rmtree(demucs_out, ignore_errors=True)
    else:
        raise ValueError(f"Unsupported stem separation backend: {backend}")

    stems_json: list[dict[str, Any]] = []
    for p in produced_files:
        sr, dur = _probe_audio_wav_info(p)
        stems_json.append(
            {
                "role": _classify_stem_role(p.name),
                "path": str(p),
                "sample_rate_hz": sr,
                "duration_sec": round(dur, 6),
            }
        )

    return {
        "schema": SCHEMA_STEMS,
        "input_path": str(input_path),
        "backend": backend,
        "model": model_name,
        "stems_requested": stems_req,
        "device": device,
        "model_cache_dir": str(model_dir),
        "stems": stems_json,
    }


def _segment_japanese_with_fugashi(text: str) -> list[str]:
    import fugashi

    tagger = fugashi.Tagger()
    tokens: list[str] = []
    for word in tagger(text):
        surface = word.surface
        if surface and not surface.isspace():
            tokens.append(surface)
    return tokens


def _tokenize_japanese_line(line: str, mode: str) -> str:
    stripped = line.strip()
    if not stripped or mode == "raw":
        return stripped
    if mode == "char":
        return " ".join(ch for ch in stripped if not ch.isspace())
    # mode == "fugashi"
    if _try_import("fugashi") is not None:
        try:
            tokens = _segment_japanese_with_fugashi(stripped)
            if tokens:
                return " ".join(tokens)
        except Exception as exc:
            print(f"[warn] fugashi tokenization failed ({exc}), falling back to raw", file=sys.stderr)
    return stripped


def _prepare_lyrics_text(raw_lyrics: str, language: str | None, ja_tokenizer: str) -> str:
    lines = [ln.strip() for ln in raw_lyrics.splitlines() if ln.strip()]
    if language == "ja" or (language in (None, "auto") and re.search(r"[\u3040-\u30ff\u4e00-\u9fff]", raw_lyrics)):
        lines = [_tokenize_japanese_line(ln, ja_tokenizer) for ln in lines]
    return "\n".join(lines)


def _split_japanese_words_post(
    words: list[dict[str, Any]],
    ja_tokenizer: str,
) -> list[dict[str, Any]]:
    """Refines space-less Japanese word items into morpheme-level timing via fugashi when needed."""
    if ja_tokenizer == "raw" or not words:
        return words
    if ja_tokenizer == "fugashi" and _try_import("fugashi") is None:
        return words

    refined: list[dict[str, Any]] = []
    for item in words:
        text = str(item.get("text", "")).strip()
        if not text:
            continue
        start_s = float(item["start_sec"])
        end_s = float(item["end_sec"])
        conf = float(item.get("confidence", 1.0))
        speaker = item.get("speaker")

        if ja_tokenizer == "char":
            parts = [ch for ch in text if not ch.isspace()]
        else:
            try:
                parts = _segment_japanese_with_fugashi(text)
            except Exception:
                parts = [text]

        if len(parts) <= 1:
            refined.append(
                {
                    "text": text,
                    "start_sec": round(start_s, 6),
                    "end_sec": round(max(end_s, start_s), 6),
                    "confidence": round(conf, 4),
                    "speaker": speaker,
                }
            )
            continue

        total_chars = sum(max(len(p), 1) for p in parts)
        dur = max(end_s - start_s, 0.0)
        cursor = start_s
        for p in parts:
            frac = max(len(p), 1) / total_chars
            part_end = cursor + dur * frac
            refined.append(
                {
                    "text": p,
                    "start_sec": round(cursor, 6),
                    "end_sec": round(max(part_end, cursor), 6),
                    "confidence": round(conf, 4),
                    "speaker": speaker,
                }
            )
            cursor = part_end
    return refined


def _align_with_stable_ts(
    audio_path: Path,
    lyrics_text: str | None,
    language: str | None,
    model_name: str,
    device: str,
    vad: bool,
    ja_tokenizer: str,
    model_dir: Path,
) -> tuple[str, list[dict[str, Any]]]:
    import stable_whisper

    whisper_cache = model_dir / "whisper"
    whisper_cache.mkdir(parents=True, exist_ok=True)
    lang_arg = None if language in (None, "", "auto") else language

    with contextlib.redirect_stdout(sys.stderr):
        model = stable_whisper.load_model(
            model_name,
            device=device,
            download_root=str(whisper_cache),
        )
        if lyrics_text is not None:
            prepared = _prepare_lyrics_text(lyrics_text, lang_arg, ja_tokenizer)
            result = model.align(str(audio_path), prepared, language=lang_arg or "en")
        else:
            result = model.transcribe(
                str(audio_path),
                language=lang_arg,
                vad=vad,
                word_timestamps=True,
            )

    detected_lang = str(getattr(result, "language", None) or lang_arg or "en")
    segments_out: list[dict[str, Any]] = []
    for idx, seg in enumerate(getattr(result, "segments", [])):
        words_raw: list[dict[str, Any]] = []
        for w in getattr(seg, "words", []) or []:
            w_text = str(getattr(w, "word", "")).strip()
            if not w_text:
                continue
            w_start = float(getattr(w, "start", 0.0) or 0.0)
            w_end = float(getattr(w, "end", w_start) or w_start)
            prob = float(getattr(w, "probability", 1.0) or 1.0)
            words_raw.append(
                {
                    "text": w_text,
                    "start_sec": round(max(0.0, w_start), 6),
                    "end_sec": round(max(w_start, w_end), 6),
                    "confidence": round(min(max(prob, 0.0), 1.0), 4),
                    "speaker": None,
                }
            )
        if detected_lang == "ja" and lyrics_text is None:
            words_raw = _split_japanese_words_post(words_raw, ja_tokenizer)

        seg_start = float(getattr(seg, "start", 0.0) or 0.0)
        seg_end = float(getattr(seg, "end", seg_start) or seg_start)
        segments_out.append(
            {
                "index": idx,
                "text": str(getattr(seg, "text", "")).strip(),
                "start_sec": round(max(0.0, seg_start), 6),
                "end_sec": round(max(seg_start, seg_end), 6),
                "words": words_raw,
            }
        )
    return detected_lang, segments_out


def _align_with_faster_whisper(
    audio_path: Path,
    language: str | None,
    model_name: str,
    compute_type: str,
    device: str,
    vad: bool,
    ja_tokenizer: str,
    model_dir: Path,
    offline: bool,
) -> tuple[str, list[dict[str, Any]]]:
    from faster_whisper import WhisperModel

    whisper_cache = model_dir / "whisper"
    whisper_cache.mkdir(parents=True, exist_ok=True)
    lang_arg = None if language in (None, "", "auto") else language
    effective_compute = compute_type
    if device == "cpu" and effective_compute in ("float16", "int8_float16"):
        effective_compute = "int8"

    with contextlib.redirect_stdout(sys.stderr):
        model = WhisperModel(
            model_name,
            device=device,
            compute_type=effective_compute,
            download_root=str(whisper_cache),
            local_files_only=offline,
        )
        segments_iter, info = model.transcribe(
            str(audio_path),
            language=lang_arg,
            word_timestamps=True,
            vad_filter=vad,
        )
        segments_list = list(segments_iter)

    detected_lang = str(getattr(info, "language", None) or lang_arg or "en")
    segments_out: list[dict[str, Any]] = []
    for idx, seg in enumerate(segments_list):
        words_raw: list[dict[str, Any]] = []
        for w in getattr(seg, "words", None) or []:
            w_text = str(getattr(w, "word", "")).strip()
            if not w_text:
                continue
            w_start = float(getattr(w, "start", 0.0) or 0.0)
            w_end = float(getattr(w, "end", w_start) or w_start)
            prob = float(getattr(w, "probability", 1.0) or 1.0)
            words_raw.append(
                {
                    "text": w_text,
                    "start_sec": round(max(0.0, w_start), 6),
                    "end_sec": round(max(w_start, w_end), 6),
                    "confidence": round(min(max(prob, 0.0), 1.0), 4),
                    "speaker": None,
                }
            )

        if detected_lang == "ja":
            words_raw = _split_japanese_words_post(words_raw, ja_tokenizer)

        seg_start = float(getattr(seg, "start", 0.0) or 0.0)
        seg_end = float(getattr(seg, "end", seg_start) or seg_start)
        segments_out.append(
            {
                "index": idx,
                "text": str(getattr(seg, "text", "")).strip(),
                "start_sec": round(max(0.0, seg_start), 6),
                "end_sec": round(max(seg_start, seg_end), 6),
                "words": words_raw,
            }
        )
    return detected_lang, segments_out


def run_align(args: argparse.Namespace) -> dict[str, Any]:
    audio_path = Path(args.input).resolve()
    if not audio_path.is_file():
        raise FileNotFoundError(f"input audio file not found: {audio_path}")

    model_dir = Path(args.model_dir).resolve() if args.model_dir else _default_model_cache_dir()
    offline = _is_offline(args.offline)
    _configure_cache_env(model_dir, offline)

    lyrics_text: str | None = None
    if args.lyrics:
        lyrics_path = Path(args.lyrics).resolve()
        if not lyrics_path.is_file():
            raise FileNotFoundError(f"lyrics file not found: {lyrics_path}")
        lyrics_text = lyrics_path.read_text(encoding="utf-8")
    elif args.lyrics_text:
        lyrics_text = args.lyrics_text

    backend = args.backend
    if backend == "auto":
        if lyrics_text is not None:
            backend = "stable-ts"
        elif _try_import("faster_whisper") is not None:
            backend = "faster-whisper"
        else:
            backend = "stable-ts"

    device = _resolve_device(args.device)
    if device == "dml":
        # Neither stable-ts (PyTorch) nor faster-whisper (CTranslate2) uses DmlExecutionProvider natively.
        device = "cpu"

    if backend == "stable-ts":
        detected_lang, segments = _align_with_stable_ts(
            audio_path=audio_path,
            lyrics_text=lyrics_text,
            language=args.language,
            model_name=args.model,
            device=device,
            vad=args.vad,
            ja_tokenizer=args.ja_tokenizer,
            model_dir=model_dir,
        )
    elif backend == "faster-whisper":
        if lyrics_text is not None:
            print(
                "[warn] faster-whisper does not support forced alignment; falling back to free transcription",
                file=sys.stderr,
            )
        detected_lang, segments = _align_with_faster_whisper(
            audio_path=audio_path,
            language=args.language,
            model_name=args.model,
            compute_type=args.compute_type,
            device=device,
            vad=args.vad,
            ja_tokenizer=args.ja_tokenizer,
            model_dir=model_dir,
            offline=offline,
        )
    else:
        raise ValueError(f"Unsupported lyric alignment backend: {backend}")

    _, duration_sec = _probe_audio_wav_info(audio_path)
    if duration_sec <= 0.0 and segments:
        duration_sec = max(float(seg.get("end_sec", 0.0)) for seg in segments)

    return {
        "schema": SCHEMA_LYRICS,
        "source_audio": str(audio_path),
        "language": detected_lang,
        "backend": backend,
        "model": args.model,
        "duration_sec": round(duration_sec, 6),
        "segments": segments,
    }


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="FrameShift AI Audio Sidecar (Stem Separation & Lyric Alignment)"
    )
    parser.add_argument(
        "--probe",
        action="store_true",
        help="Print JSON capability probe report and exit",
    )
    parser.add_argument(
        "--model-dir",
        default="",
        help="Model cache directory (default: local_media/models)",
    )

    sub = parser.add_subparsers(dest="command")

    p_probe = sub.add_parser("probe", help="Print JSON capability probe report")
    p_probe.add_argument("--model-dir", default="")

    p_sep = sub.add_parser("separate", help="Separate audio/video into vocal & instrumental stems")
    p_sep.add_argument("--input", required=True, help="Input audio or video file")
    p_sep.add_argument("--out-dir", required=True, help="Directory for separated stem WAV files")
    p_sep.add_argument(
        "--backend",
        choices=["auto", "audio-separator", "demucs", "mdx-onnx"],
        default="auto",
    )
    p_sep.add_argument("--model", default="auto")
    p_sep.add_argument("--stems", type=int, choices=[2, 4], default=2)
    p_sep.add_argument("--device", choices=["auto", "cuda", "dml", "cpu"], default="auto")
    p_sep.add_argument("--output-format", choices=["wav", "flac", "mp3"], default="wav")
    p_sep.add_argument("--model-dir", default="")
    p_sep.add_argument("--offline", action="store_true")

    p_align = sub.add_parser("align", help="Align or transcribe word-level lyrics")
    p_align.add_argument("--input", required=True, help="Input vocals WAV/audio file")
    p_align.add_argument("--lyrics", default="", help="Path to UTF-8 lyrics text file")
    p_align.add_argument("--lyrics-text", default="", help="Inline UTF-8 lyrics text")
    p_align.add_argument("--language", default="auto", help="en, ja, auto, etc.")
    p_align.add_argument(
        "--backend",
        choices=["auto", "stable-ts", "faster-whisper"],
        default="auto",
    )
    p_align.add_argument("--model", default=DEFAULT_WHISPER_MODEL)
    p_align.add_argument(
        "--compute-type",
        choices=["float16", "int8_float16", "int8", "float32"],
        default="float16",
    )
    p_align.add_argument("--device", choices=["auto", "cuda", "cpu"], default="auto")
    p_align.add_argument(
        "--ja-tokenizer",
        choices=["fugashi", "char", "raw"],
        default="fugashi",
    )
    p_align.add_argument("--vad", action="store_true")
    p_align.add_argument("--model-dir", default="")
    p_align.add_argument("--offline", action="store_true")

    return parser


def main() -> int:
    parser = _build_parser()
    args = parser.parse_args()

    if args.probe or args.command == "probe":
        model_dir = Path(args.model_dir).resolve() if getattr(args, "model_dir", "") else None
        print(json.dumps(probe_environment(model_dir)))
        return 0

    try:
        if args.command == "separate":
            payload = run_separate(args)
            print(json.dumps(payload, ensure_ascii=False))
            return 0
        if args.command == "align":
            payload = run_align(args)
            print(json.dumps(payload, ensure_ascii=False))
            return 0
    except Exception as exc:
        print(f"ai_audio_sidecar error: {exc}", file=sys.stderr)
        return 1

    parser.print_help(sys.stderr)
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
