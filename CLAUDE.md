# FrameShift

Strictly follow [AGENTS.md](./AGENTS.md). Do not duplicate rules here.

## Verified Codebase Snapshot (2026-11-24)

- **Workspace (`Cargo.toml`):** `edition = "2024"`, `license = "MIT OR Apache-2.0"`, **9 crates**: `frameshift-time`, `frameshift-schema`, `frameshift-media`, `frameshift-vector`, `frameshift-render`, `frameshift-core`, `frameshift-mcp`, `frameshift-app`, `frameshift-cli`.
- **Render & Schema:** **60 WGSL shaders** (`crates/frameshift-render/shaders/`), **40 `NodeKind` variants** (`schema_version: 0.2.0`, 10 built-in transitions), `Rgba16Float` Linear Rec.709, `4x MSAA`, `nv12`→Linear GPU decode, 3-tier `NodeContentHash` Smart Cache, `SafeExpr` (`spring`/`wiggle`/audio-band uniforms), `Motion.md` DSL, `.otio`/FCPXML 1.10/1.11 NLE bridge.
- **Surfaces:** **50 CLI subcommands** (`frameshift-cli`) and **32 MCP tools** (`frameshift-mcp`).
- **Completed Pillars:** Faz 0–6.6, Anime P0 workflow, GPU `OpticalFlowBlur` + RIFE/ONNX sidecars, 5 Core Upgrades (Spectral Flux, AI Stem/Lyrics, Waveform PNG, Anime4K/Real-ESRGAN, Depth-Anything-V2 `Scene3D`), and Competitive Power P1/P2/P3/P5. **P4** interactive desktop window (`egui`/`winit`) is deferred by user decision.
- **Source of Truth for Status & Logs:** Always check [brain/state.md](./brain/state.md) and [brain/index.md](./brain/index.md).

## Verification Commands

```powershell
cargo check --workspace
cargo test --workspace --no-fail-fast
cargo clippy --workspace --all-targets -- -D warnings
cargo run --release -p frameshift-cli -- verify-all --out-dir target/full_verify
```

## Anime & Raw Clip Sources (Search Order)

When sourcing raw anime clips, sakuga cuts, or Twixtor scene packs for edits, agents should search these repositories in order:

1. **Sakugabooru** (`https://www.sakugabooru.com/post`) — Unsubtitled, watermark-free raw animation cuts (sakuga). Search by series/character tag with `order:score`, then download the uncompressed file via the `"Original"` link in the right sidebar.
2. **Animédia / AnimeClipsRaw** (`https://animeclipsraw.fr`) — AMV and TikTok/Reels-focused raw clips categorized by character and series, including flowframe/Twixtor scenes and voice lines.
3. **RingWitDaTwixtor** (`https://ringwitdatwixtor.com`) — Pre-made 4K/60fps Twixtor and raw scene packs for popular anime characters (typically via Google Drive/Mega links).
4. **Hii Twixtor** (`https://hiitwixtor.com`) — Clip archive focused on time-remapped / slow-motion (Twixtor) anime cuts.
5. **411 Scenepacks** (`https://411scenepacks.com`) — Large free HD/4K scene pack directory covering anime, series, films, and games.
