#!/usr/bin/env python3
"""FrameShift ONNX Runtime AI Sidecar (Rotoscope Matte, Depth Map & Manifest Models).

Streams raw RGBA8 frames over stdin/stdout pipes so zero temporary PNG files touch disk.
Supports DirectML (Windows GPU), CUDA, and CPU execution providers automatically, plus
`--manifest <path>` for declarative `frameshift.ai_model.v1` ONNX models.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any

import numpy as np
from PIL import Image


def probe_environment() -> dict[str, Any]:
    info: dict[str, Any] = {
        "python_version": sys.version.split()[0],
        "onnxruntime_available": False,
        "onnxruntime_version": None,
        "providers": [],
        "depth_formats": ["u8", "rg16", "png16"],
        "norm_modes": ["ema", "per_frame", "global_pass"],
        "manifest_schema": "frameshift.ai_model.v1",
    }
    try:
        import onnxruntime as ort

        info["onnxruntime_available"] = True
        info["onnxruntime_version"] = ort.__version__
        info["providers"] = list(ort.get_available_providers())
    except Exception as exc:  # pragma: no cover - defensive probe
        info["error"] = str(exc)
    return info


def select_providers(requested: str, available: list[str]) -> list[str]:
    if requested == "dml":
        order = ["DmlExecutionProvider", "CPUExecutionProvider"]
    elif requested == "cuda":
        order = ["CUDAExecutionProvider", "CPUExecutionProvider"]
    elif requested == "cpu":
        order = ["CPUExecutionProvider"]
    else:
        order = ["DmlExecutionProvider", "CUDAExecutionProvider", "CPUExecutionProvider"]
    chosen = [p for p in order if p in available]
    return chosen or available or ["CPUExecutionProvider"]


def _snap14(val: float) -> int:
    return max(14, int(round(val / 14.0)) * 14)


def _snap_multiple(val: float, mult: int) -> int:
    m = max(1, mult)
    return max(m, int(round(val / float(m))) * m)


def _resolve_hw(
    shape: list[Any],
    fallback: int,
    task: str,
    src_h: int = 0,
    src_w: int = 0,
) -> tuple[int, int]:
    has_fixed_h = len(shape) >= 4 and isinstance(shape[2], int) and shape[2] > 0
    has_fixed_w = len(shape) >= 4 and isinstance(shape[3], int) and shape[3] > 0

    if task == "depth":
        if has_fixed_h and has_fixed_w:
            h = max(14, (int(shape[2]) // 14) * 14)
            w = max(14, (int(shape[3]) // 14) * 14)
            return int(h), int(w)
        target_short = _snap14(float(fallback))
        if src_w > 0 and src_h > 0:
            if src_w >= src_h:
                h = target_short
                w = min(2044, _snap14((float(src_w) / float(src_h)) * float(target_short)))
            else:
                w = target_short
                h = min(2044, _snap14((float(src_h) / float(src_w)) * float(target_short)))
            return int(h), int(w)
        return target_short, target_short

    h = int(shape[2]) if has_fixed_h else fallback
    w = int(shape[3]) if has_fixed_w else fallback
    return int(h), int(w)


def _resize_float_map(arr: np.ndarray, out_w: int, out_h: int) -> np.ndarray:
    if arr.shape[0] == out_h and arr.shape[1] == out_w:
        return np.clip(arr.astype(np.float32, copy=False), 0.0, 1.0)
    resized = Image.fromarray(arr.astype(np.float32), mode="F").resize(
        (out_w, out_h), Image.Resampling.BILINEAR
    )
    return np.clip(np.asarray(resized, dtype=np.float32), 0.0, 1.0)


def _run_onnx_raw(
    session: Any,
    input_name: str,
    target_hw: tuple[int, int],
    rgba: np.ndarray,
    task: str,
) -> np.ndarray:
    h_in, w_in = target_hw
    rgb_img = Image.fromarray(rgba[:, :, :3], mode="RGB").resize(
        (w_in, h_in), Image.Resampling.BILINEAR
    )
    arr = np.asarray(rgb_img, dtype=np.float32) / 255.0

    if task == "depth":
        mean = np.array([0.485, 0.456, 0.406], dtype=np.float32)
        std = np.array([0.229, 0.224, 0.225], dtype=np.float32)
        arr = (arr - mean) / std
    else:
        arr = (arr - 0.5) / 0.5

    nchw = np.transpose(arr, (2, 0, 1))[None, :, :, :].astype(np.float32)
    outputs = session.run(None, {input_name: nchw})
    out = np.asarray(outputs[0], dtype=np.float32)
    while out.ndim > 2:
        out = out[0] if out.shape[0] == 1 else out.mean(axis=0)

    if task == "rotoscope":
        if float(out.min()) < -0.05 or float(out.max()) > 1.05:
            out = 1.0 / (1.0 + np.exp(-np.clip(out, -30.0, 30.0)))
        return np.clip(out, 0.0, 1.0)
    return out


def _normalize_depth_and_resize(
    raw_out: np.ndarray,
    lo: float,
    hi: float,
    out_w: int,
    out_h: int,
) -> np.ndarray:
    denom = max(hi - lo, 1e-5)
    norm = np.clip((raw_out - lo) / denom, 0.0, 1.0)
    return _resize_float_map(norm, out_w, out_h)


def _encode_pipe_rgba(
    scalar: np.ndarray,
    task: str,
    depth_format: str,
    height: int,
    width: int,
) -> np.ndarray:
    out_rgba = np.empty((height, width, 4), dtype=np.uint8)
    if task == "depth" and depth_format == "rg16":
        d16 = np.clip(np.rint(scalar * 65535.0), 0.0, 65535.0).astype(np.uint16)
        hi = (d16 >> 8).astype(np.uint8)
        lo = (d16 & 0xFF).astype(np.uint8)
        out_rgba[:, :, 0] = hi
        out_rgba[:, :, 1] = lo
        out_rgba[:, :, 2] = hi
        out_rgba[:, :, 3] = 255
        return out_rgba

    u8_map = np.clip(np.rint(scalar * 255.0), 0, 255).astype(np.uint8)
    out_rgba[:, :, 0] = u8_map
    out_rgba[:, :, 1] = u8_map
    out_rgba[:, :, 2] = u8_map
    out_rgba[:, :, 3] = u8_map if task == "rotoscope" else 255
    return out_rgba


def _save_depth_png16(scalar: np.ndarray, out_dir: Path, frame_idx: int) -> None:
    d16 = np.clip(np.rint(scalar * 65535.0), 0.0, 65535.0).astype(np.uint16)
    out_path = out_dir / f"depth_{frame_idx:06d}.png"
    Image.fromarray(d16, mode="I;16").save(out_path)


def _synthetic_frame(rgba: np.ndarray, task: str) -> np.ndarray:
    rgb = rgba[:, :, :3].astype(np.float32) / 255.0
    luma = 0.2126 * rgb[:, :, 0] + 0.7152 * rgb[:, :, 1] + 0.0722 * rgb[:, :, 2]
    if task in ("rotoscope", "matte"):
        sat = rgb.max(axis=2) - rgb.min(axis=2)
        return np.clip(0.65 * luma + 0.75 * sat, 0.0, 1.0)
    h, _ = luma.shape
    yy = np.linspace(0.2, 0.9, h, dtype=np.float32)[:, None]
    return np.clip(0.5 * luma + 0.5 * yy, 0.0, 1.0)


# --- Manifest-Driven Inference (`frameshift.ai_model.v1`) ---


def _resample_from_name(name: str | None) -> Image.Resampling:
    key = (name or "bilinear").strip().lower()
    if key == "bicubic":
        return Image.Resampling.BICUBIC
    if key == "lanczos":
        return Image.Resampling.LANCZOS
    if key == "nearest":
        return Image.Resampling.NEAREST
    return Image.Resampling.BILINEAR


def _resolve_manifest_target_hw(
    manifest: dict[str, Any],
    src_h: int,
    src_w: int,
) -> tuple[int, int, tuple[int, int]]:
    resize = manifest.get("resize") or {}
    mode = str(resize.get("mode", "native")).strip().lower()
    mult = int(resize.get("multiple_of") or 1)
    max_side = int(resize.get("max_side") or 4096)

    if mode == "fixed":
        w = int(resize.get("width") or src_w)
        h = int(resize.get("height") or src_h)
        return h, w, (0, 0)

    if mode == "aspect_snap":
        target_short = int(resize.get("width") or resize.get("height") or 518)
        target_short = _snap_multiple(float(target_short), mult)
        if src_w >= src_h:
            h = target_short
            w = min(max_side, _snap_multiple((float(src_w) / float(src_h)) * float(target_short), mult))
        else:
            w = target_short
            h = min(max_side, _snap_multiple((float(src_h) / float(src_w)) * float(target_short), mult))
        return h, w, (0, 0)

    pad_h = (mult - (src_h % mult)) % mult if mult > 1 else 0
    pad_w = (mult - (src_w % mult)) % mult if mult > 1 else 0
    return src_h, src_w, (pad_h, pad_w)


def _prepare_manifest_image_tensor(
    rgba: np.ndarray,
    manifest: dict[str, Any],
    target_hw: tuple[int, int],
    pad_hw: tuple[int, int],
) -> np.ndarray:
    h_tgt, w_tgt = target_hw
    resize = manifest.get("resize") or {}
    resample = _resample_from_name(resize.get("interp"))
    color_space = str(manifest.get("color_space", "rgb")).strip().lower()
    layout = str(manifest.get("input_layout", "nchw")).strip().lower()
    dtype_str = str(manifest.get("input_dtype", "float32")).strip().lower()
    mean = np.array(manifest.get("mean") or [0.0, 0.0, 0.0], dtype=np.float32)
    std = np.array(manifest.get("std") or [1.0, 1.0, 1.0], dtype=np.float32)
    std = np.where(std == 0.0, 1.0, std)

    if rgba.shape[0] != h_tgt or rgba.shape[1] != w_tgt:
        img = Image.fromarray(rgba, mode="RGBA").resize((w_tgt, h_tgt), resample)
        work = np.asarray(img, dtype=np.uint8)
    else:
        work = rgba

    pad_h, pad_w = pad_hw
    if pad_h > 0 or pad_w > 0:
        work = np.pad(work, ((0, pad_h), (0, pad_w), (0, 0)), mode="reflect")

    if color_space == "bgr":
        channels = work[:, :, [2, 1, 0]]
    elif color_space == "rgba":
        channels = work[:, :, :4]
    elif color_space == "luma":
        rgb_f = work[:, :, :3].astype(np.float32)
        luma = 0.299 * rgb_f[:, :, 0] + 0.587 * rgb_f[:, :, 1] + 0.114 * rgb_f[:, :, 2]
        channels = np.clip(np.rint(luma), 0, 255).astype(np.uint8)[:, :, None]
    else:
        channels = work[:, :, :3]

    if dtype_str == "uint8":
        arr = channels.astype(np.uint8)
    else:
        arr = channels.astype(np.float32) / 255.0
        if channels.shape[2] == 1:
            arr = (arr - mean[0]) / std[0]
        elif channels.shape[2] == 4:
            arr[:, :, :3] = (arr[:, :, :3] - mean) / std
        else:
            arr = (arr - mean) / std
        if dtype_str == "float16":
            arr = arr.astype(np.float16)
        else:
            arr = arr.astype(np.float32)

    if layout == "nhwc":
        return arr[None, :, :, :]
    return np.transpose(arr, (2, 0, 1))[None, :, :, :]


def _dtype_from_str(dtype_str: str) -> Any:
    d = dtype_str.strip().lower()
    if d == "float16":
        return np.float16
    if d == "uint8":
        return np.uint8
    if d == "int64":
        return np.int64
    return np.float32


def _build_aux_feed(manifest: dict[str, Any], params: dict[str, Any]) -> dict[str, np.ndarray]:
    feed: dict[str, np.ndarray] = {}
    for spec in manifest.get("aux_inputs") or []:
        name = str(spec["name"])
        source = str(spec.get("source", "constant"))
        np_dtype = _dtype_from_str(str(spec.get("dtype", "float32")))
        val: Any = spec.get("default_value")
        if source == "prompt.point_coords" and "points" in params:
            val = params["points"]
        elif source == "prompt.point_labels" and "labels" in params:
            val = params["labels"]
        elif source.startswith("params."):
            key = source.split(".", 1)[1]
            if key in params:
                val = params[key]
        if val is None:
            val = 0
        arr = np.asarray(val, dtype=np_dtype)
        shape = spec.get("shape") or []
        if shape and all(isinstance(x, int) and x > 0 for x in shape) and arr.size == int(np.prod(shape)):
            arr = arr.reshape(tuple(shape))
        feed[name] = arr
    return feed


def _extract_manifest_output(
    session: Any,
    outputs: list[Any],
    manifest: dict[str, Any],
    pad_hw: tuple[int, int],
    scale_factor: int,
) -> np.ndarray:
    out_name = manifest.get("output_tensor_name")
    if out_name:
        meta_names = [o.name for o in session.get_outputs()]
        idx = meta_names.index(out_name) if out_name in meta_names else 0
    else:
        idx = int(manifest.get("output_index", 0))
    raw = np.asarray(outputs[idx], dtype=np.float32)

    layout = str(manifest.get("input_layout", "nchw")).strip().lower()
    if raw.ndim == 4:
        raw = raw[0]
        if layout == "nchw" or (raw.shape[0] in (1, 3, 4) and raw.shape[2] > 4):
            hwc = np.transpose(raw, (1, 2, 0))
        else:
            hwc = raw
    elif raw.ndim == 3:
        if raw.shape[0] in (1, 3, 4) and raw.shape[2] > 4:
            hwc = np.transpose(raw, (1, 2, 0))
        elif raw.shape[0] == 1:
            hwc = raw[0][:, :, None]
        else:
            hwc = raw[:, :, None] if raw.shape[2] not in (1, 3, 4) else raw
    elif raw.ndim == 2:
        hwc = raw[:, :, None]
    else:
        raise ValueError(f"Unsupported ONNX output ndim={raw.ndim} shape={raw.shape}")

    pad_h, pad_w = pad_hw
    if pad_h > 0:
        hwc = hwc[: hwc.shape[0] - pad_h * scale_factor, :, :]
    if pad_w > 0:
        hwc = hwc[:, : hwc.shape[1] - pad_w * scale_factor, :]
    return hwc


def _apply_activation(hwc: np.ndarray, manifest: dict[str, Any]) -> np.ndarray:
    act = str(manifest.get("activation", "auto")).strip().lower()
    task = str(manifest.get("task", "matte")).strip().lower()
    if act == "none":
        return hwc
    if act == "sigmoid":
        return 1.0 / (1.0 + np.exp(-np.clip(hwc, -30.0, 30.0)))
    if act == "tanh":
        return 0.5 * (np.tanh(hwc) + 1.0)
    if act == "clamp01":
        return np.clip(hwc, 0.0, 1.0)
    if task in ("matte", "rotoscope"):
        if float(hwc.min()) < -0.05 or float(hwc.max()) > 1.05:
            hwc = 1.0 / (1.0 + np.exp(-np.clip(hwc, -30.0, 30.0)))
        return np.clip(hwc, 0.0, 1.0)
    if task in ("upscale", "style"):
        return np.clip(hwc, 0.0, 1.0)
    return hwc


def _run_manifest_onnx(
    session: Any,
    input_name: str,
    rgba: np.ndarray,
    manifest: dict[str, Any],
    params: dict[str, Any],
) -> np.ndarray:
    src_h, src_w = rgba.shape[0], rgba.shape[1]
    scale_factor = max(1, int(manifest.get("scale_factor") or 1))
    aux_feed = _build_aux_feed(manifest, params)
    tiling = manifest.get("tiling")

    if tiling and int(tiling.get("tile_size") or 0) > 0:
        tile_size = int(tiling["tile_size"])
        overlap = int(tiling.get("overlap") or 0)
        stride = max(1, tile_size - overlap)
        out_h = src_h * scale_factor
        out_w = src_w * scale_factor
        accum: np.ndarray | None = None
        weight_sum = np.zeros((out_h, out_w, 1), dtype=np.float32)

        ys = list(range(0, max(1, src_h - overlap), stride)) or [0]
        xs = list(range(0, max(1, src_w - overlap), stride)) or [0]

        for y0 in ys:
            y1 = min(src_h, y0 + tile_size)
            y0_eff = max(0, y1 - tile_size)
            for x0 in xs:
                x1 = min(src_w, x0 + tile_size)
                x0_eff = max(0, x1 - tile_size)
                tile = rgba[y0_eff:y1, x0_eff:x1, :]
                th, tw = tile.shape[0], tile.shape[1]
                h_tgt, w_tgt, pad_hw = _resolve_manifest_target_hw(manifest, th, tw)
                tensor = _prepare_manifest_image_tensor(tile, manifest, (h_tgt, w_tgt), pad_hw)
                feed = {input_name: tensor, **aux_feed}
                outputs = session.run(None, feed)
                hwc = _extract_manifest_output(session, outputs, manifest, pad_hw, scale_factor)
                hwc = _apply_activation(hwc, manifest)
                if accum is None:
                    accum = np.zeros((out_h, out_w, hwc.shape[2]), dtype=np.float32)
                oy0, oy1 = y0_eff * scale_factor, y1 * scale_factor
                ox0, ox1 = x0_eff * scale_factor, x1 * scale_factor
                tile_oh, tile_ow = oy1 - oy0, ox1 - ox0
                if hwc.shape[0] != tile_oh or hwc.shape[1] != tile_ow:
                    resized_ch = [
                        np.asarray(
                            Image.fromarray(hwc[:, :, c], mode="F").resize(
                                (tile_ow, tile_oh), Image.Resampling.BILINEAR
                            ),
                            dtype=np.float32,
                        )
                        for c in range(hwc.shape[2])
                    ]
                    hwc = np.stack(resized_ch, axis=2)
                accum[oy0:oy1, ox0:ox1, :] += hwc
                weight_sum[oy0:oy1, ox0:ox1, :] += 1.0

        assert accum is not None
        return accum / np.maximum(weight_sum, 1.0)

    h_tgt, w_tgt, pad_hw = _resolve_manifest_target_hw(manifest, src_h, src_w)
    tensor = _prepare_manifest_image_tensor(rgba, manifest, (h_tgt, w_tgt), pad_hw)
    feed = {input_name: tensor, **aux_feed}
    outputs = session.run(None, feed)
    hwc = _extract_manifest_output(session, outputs, manifest, pad_hw, scale_factor)
    return _apply_activation(hwc, manifest)


def _resize_hwc(hwc: np.ndarray, out_w: int, out_h: int, clip01: bool) -> np.ndarray:
    if hwc.shape[0] == out_h and hwc.shape[1] == out_w:
        return np.clip(hwc, 0.0, 1.0) if clip01 else hwc
    chans = []
    for c in range(hwc.shape[2]):
        r = Image.fromarray(hwc[:, :, c].astype(np.float32), mode="F").resize(
            (out_w, out_h), Image.Resampling.BILINEAR
        )
        chans.append(np.asarray(r, dtype=np.float32))
    out = np.stack(chans, axis=2)
    return np.clip(out, 0.0, 1.0) if clip01 else out


def _encode_manifest_rgba(
    hwc: np.ndarray,
    rgba_in: np.ndarray,
    manifest: dict[str, Any],
    out_w: int,
    out_h: int,
) -> np.ndarray:
    task = str(manifest.get("task", "matte")).strip().lower()
    out_rgba = np.empty((out_h, out_w, 4), dtype=np.uint8)
    c_out = hwc.shape[2]
    if c_out == 1:
        u8_map = np.clip(np.rint(hwc[:, :, 0] * 255.0), 0, 255).astype(np.uint8)
        out_rgba[:, :, 0] = u8_map
        out_rgba[:, :, 1] = u8_map
        out_rgba[:, :, 2] = u8_map
        out_rgba[:, :, 3] = u8_map if task in ("matte", "rotoscope") else 255
        return out_rgba

    color_space = str(manifest.get("color_space", "rgb")).strip().lower()
    rgb = hwc[:, :, [2, 1, 0]] if color_space == "bgr" else hwc[:, :, :3]
    out_rgba[:, :, :3] = np.clip(np.rint(rgb * 255.0), 0, 255).astype(np.uint8)
    if c_out >= 4:
        out_rgba[:, :, 3] = np.clip(np.rint(hwc[:, :, 3] * 255.0), 0, 255).astype(np.uint8)
    else:
        if rgba_in.shape[0] != out_h or rgba_in.shape[1] != out_w:
            a_img = Image.fromarray(rgba_in[:, :, 3], mode="L").resize(
                (out_w, out_h), Image.Resampling.BILINEAR
            )
            out_rgba[:, :, 3] = np.asarray(a_img, dtype=np.uint8)
        else:
            out_rgba[:, :, 3] = rgba_in[:, :, 3]
    return out_rgba


def _run_manifest_mode(args: argparse.Namespace) -> int:
    manifest_path = Path(args.manifest)
    if not manifest_path.is_file():
        print(f"manifest not found: {manifest_path}", file=sys.stderr)
        return 2
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    onnx_rel = manifest.get("onnx_file") or manifest.get("entrypoint") or ""
    onnx_path = (manifest_path.parent / onnx_rel).resolve()
    manifest_dir = manifest_path.parent.resolve()
    try:
        onnx_path.relative_to(manifest_dir)
    except ValueError:
        print(f"manifest onnx_file escapes directory: {onnx_rel}", file=sys.stderr)
        return 2

    params: dict[str, Any] = {}
    if args.params_json:
        params = json.loads(args.params_json)

    scale_factor = max(1, int(manifest.get("scale_factor") or 1))
    task = str(manifest.get("task", "matte")).strip().lower()
    out_w = args.width * scale_factor
    out_h = args.height * scale_factor

    session = None
    input_name = ""
    if not args.synthetic_test:
        if not onnx_path.is_file():
            print(f"ONNX model not found: {onnx_path}", file=sys.stderr)
            return 2
        import onnxruntime as ort

        providers = select_providers(args.provider, list(ort.get_available_providers()))
        session = ort.InferenceSession(str(onnx_path), providers=providers)
        input_name = session.get_inputs()[0].name

    frame_bytes = args.width * args.height * 4
    smooth = float(np.clip(args.temporal_smooth, 0.0, 0.95))
    ema_decay = float(np.clip(args.ema_decay, 0.0, 0.99))

    stdin_buf = sys.stdin.buffer
    stdout_buf = sys.stdout.buffer

    prev_hwc: np.ndarray | None = None
    prev_rgb_thumb: np.ndarray | None = None
    ema_lo: float | None = None
    ema_hi: float | None = None

    for _ in range(args.frames):
        raw = stdin_buf.read(frame_bytes)
        if len(raw) != frame_bytes:
            print(
                f"unexpected EOF from stdin: expected {frame_bytes} bytes, got {len(raw)}",
                file=sys.stderr,
            )
            return 3
        rgba = np.frombuffer(raw, dtype=np.uint8).reshape((args.height, args.width, 4))
        rgb_thumb = (
            np.asarray(
                Image.fromarray(rgba[:, :, :3], mode="RGB").resize(
                    (64, 36), Image.Resampling.BILINEAR
                ),
                dtype=np.float32,
            )
            / 255.0
        )
        scene_cut = False
        if prev_rgb_thumb is not None and float(np.mean(np.abs(rgb_thumb - prev_rgb_thumb))) > 0.25:
            scene_cut = True
        prev_rgb_thumb = rgb_thumb

        if args.synthetic_test:
            if task in ("upscale", "style"):
                up = Image.fromarray(rgba, mode="RGBA").resize(
                    (out_w, out_h), Image.Resampling.BICUBIC
                )
                hwc = np.asarray(up, dtype=np.float32)[:, :, :3] / 255.0
                if task == "style":
                    hwc = np.clip(hwc * 1.05 + 0.02, 0.0, 1.0)
            else:
                scalar = _synthetic_frame(rgba, task)
                scalar = _resize_float_map(scalar, out_w, out_h)
                hwc = scalar[:, :, None]
        else:
            raw_hwc = _run_manifest_onnx(session, input_name, rgba, manifest, params)
            if task == "depth":
                scalar = raw_hwc[:, :, 0]
                p1 = float(np.percentile(scalar, 1.0))
                p99 = float(np.percentile(scalar, 99.0))
                if ema_lo is None or ema_hi is None or scene_cut:
                    ema_lo, ema_hi = p1, p99
                else:
                    ema_lo = ema_decay * ema_lo + (1.0 - ema_decay) * p1
                    ema_hi = ema_decay * ema_hi + (1.0 - ema_decay) * p99
                scalar = _normalize_depth_and_resize(scalar, ema_lo, ema_hi, out_w, out_h)
                hwc = scalar[:, :, None]
            else:
                hwc = _resize_hwc(raw_hwc, out_w, out_h, clip01=True)

        if scene_cut:
            prev_hwc = None
        if prev_hwc is not None and smooth > 0.0:
            hwc = prev_hwc * smooth + hwc * (1.0 - smooth)
        prev_hwc = hwc

        out_rgba = _encode_manifest_rgba(hwc, rgba, manifest, out_w, out_h)
        stdout_buf.write(out_rgba.tobytes())

    stdout_buf.flush()
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description="FrameShift ONNX AI Raw-Pipe Sidecar")
    parser.add_argument("--probe", action="store_true", help="Print environment JSON and exit")
    parser.add_argument("--manifest", default="", help="Path to frameshift.ai_model.v1 model_manifest.json")
    parser.add_argument("--params-json", default="", help="Optional JSON string for aux_inputs")
    parser.add_argument("--task", choices=["rotoscope", "depth"], default="rotoscope")
    parser.add_argument("--model", default="")
    parser.add_argument("--width", type=int, default=0)
    parser.add_argument("--height", type=int, default=0)
    parser.add_argument("--frames", type=int, default=0)
    parser.add_argument("--input-size", type=int, default=518)
    parser.add_argument("--temporal-smooth", type=float, default=0.25)
    parser.add_argument(
        "--norm-mode",
        choices=["ema", "per_frame", "global_pass"],
        default="ema",
        help="Depth range normalization strategy",
    )
    parser.add_argument(
        "--ema-decay",
        type=float,
        default=0.85,
        help="Temporal EMA decay for depth percentile range [p1, p99]",
    )
    parser.add_argument(
        "--depth-format",
        choices=["u8", "rg16"],
        default="u8",
        help="Raw stdout pixel packing for depth task",
    )
    parser.add_argument(
        "--png16-dir",
        default="",
        help="Optional directory to write 16-bit grayscale depth PNGs (depth_XXXXXX.png)",
    )
    parser.add_argument("--provider", choices=["auto", "dml", "cuda", "cpu"], default="auto")
    parser.add_argument("--synthetic-test", action="store_true")
    args = parser.parse_args()

    if args.probe:
        print(json.dumps(probe_environment()))
        return 0

    if args.width <= 0 or args.height <= 0 or args.frames <= 0:
        print("width, height, and frames must be > 0", file=sys.stderr)
        return 2

    if args.manifest:
        return _run_manifest_mode(args)

    png16_dir: Path | None = None
    if args.png16_dir and args.task == "depth":
        png16_dir = Path(args.png16_dir)
        png16_dir.mkdir(parents=True, exist_ok=True)

    session = None
    input_name = ""
    target_hw = (args.input_size, args.input_size)

    if not args.synthetic_test:
        if not args.model:
            print("--model is required unless --synthetic-test is set", file=sys.stderr)
            return 2
        import onnxruntime as ort

        providers = select_providers(args.provider, list(ort.get_available_providers()))
        session = ort.InferenceSession(args.model, providers=providers)
        in_meta = session.get_inputs()[0]
        input_name = in_meta.name
        target_hw = _resolve_hw(
            list(in_meta.shape),
            args.input_size,
            args.task,
            src_h=args.height,
            src_w=args.width,
        )

    frame_bytes = args.width * args.height * 4
    smooth = float(np.clip(args.temporal_smooth, 0.0, 0.95))
    ema_decay = float(np.clip(args.ema_decay, 0.0, 0.99))

    stdin_buf = sys.stdin.buffer
    stdout_buf = sys.stdout.buffer

    if args.task == "depth" and args.norm_mode == "global_pass" and not args.synthetic_test:
        raw_outputs: list[np.ndarray] = []
        for _ in range(args.frames):
            raw = stdin_buf.read(frame_bytes)
            if len(raw) != frame_bytes:
                print(
                    f"unexpected EOF from stdin: expected {frame_bytes} bytes, got {len(raw)}",
                    file=sys.stderr,
                )
                return 3
            rgba = np.frombuffer(raw, dtype=np.uint8).reshape((args.height, args.width, 4))
            raw_outputs.append(_run_onnx_raw(session, input_name, target_hw, rgba, "depth"))

        stacked = np.stack(raw_outputs, axis=0)
        global_lo = float(np.percentile(stacked, 0.5))
        global_hi = float(np.percentile(stacked, 99.5))
        prev_map: np.ndarray | None = None
        for idx, raw_out in enumerate(raw_outputs):
            scalar = _normalize_depth_and_resize(
                raw_out, global_lo, global_hi, args.width, args.height
            )
            if prev_map is not None and smooth > 0.0:
                scalar = prev_map * smooth + scalar * (1.0 - smooth)
            prev_map = scalar
            if png16_dir is not None:
                _save_depth_png16(scalar, png16_dir, idx)
            out_rgba = _encode_pipe_rgba(
                scalar, args.task, args.depth_format, args.height, args.width
            )
            stdout_buf.write(out_rgba.tobytes())
        stdout_buf.flush()
        return 0

    prev_map = None
    prev_rgb_thumb = None
    ema_lo = None
    ema_hi = None

    for idx in range(args.frames):
        raw = stdin_buf.read(frame_bytes)
        if len(raw) != frame_bytes:
            print(
                f"unexpected EOF from stdin: expected {frame_bytes} bytes, got {len(raw)}",
                file=sys.stderr,
            )
            return 3

        rgba = np.frombuffer(raw, dtype=np.uint8).reshape((args.height, args.width, 4))
        rgb_thumb = (
            np.asarray(
                Image.fromarray(rgba[:, :, :3], mode="RGB").resize(
                    (64, 36), Image.Resampling.BILINEAR
                ),
                dtype=np.float32,
            )
            / 255.0
        )
        scene_cut = False
        if prev_rgb_thumb is not None:
            mad = float(np.mean(np.abs(rgb_thumb - prev_rgb_thumb)))
            if mad > 0.25:
                scene_cut = True
        prev_rgb_thumb = rgb_thumb

        if args.synthetic_test:
            scalar = _synthetic_frame(rgba, args.task)
        elif args.task == "rotoscope":
            raw_out = _run_onnx_raw(session, input_name, target_hw, rgba, "rotoscope")
            scalar = _resize_float_map(raw_out, args.width, args.height)
        else:
            raw_out = _run_onnx_raw(session, input_name, target_hw, rgba, "depth")
            p1 = float(np.percentile(raw_out, 1.0))
            p99 = float(np.percentile(raw_out, 99.0))
            if args.norm_mode == "per_frame" or ema_lo is None or ema_hi is None or scene_cut:
                ema_lo = p1
                ema_hi = p99
            else:
                ema_lo = ema_decay * ema_lo + (1.0 - ema_decay) * p1
                ema_hi = ema_decay * ema_hi + (1.0 - ema_decay) * p99
            scalar = _normalize_depth_and_resize(
                raw_out, ema_lo, ema_hi, args.width, args.height
            )

        if scene_cut:
            prev_map = None
        if prev_map is not None and smooth > 0.0:
            scalar = prev_map * smooth + scalar * (1.0 - smooth)
        prev_map = scalar

        if png16_dir is not None:
            _save_depth_png16(scalar, png16_dir, idx)
        out_rgba = _encode_pipe_rgba(
            scalar, args.task, args.depth_format, args.height, args.width
        )
        stdout_buf.write(out_rgba.tobytes())

    stdout_buf.flush()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
