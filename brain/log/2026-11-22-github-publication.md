# GitHub Publication Checkpoint

Date: 2026-11-22

- Prepared the existing FrameShift workspace for publication to `origin/main`. No implementation changes were made during publication.
- Local media and Cargo build output were excluded by the repository ignore rules.
- `cargo test --workspace --locked` stopped in `frameshift-core`: 16 passed and 2 failed. The failing tests were `test_plan_autocut_no_beats` and `test_plan_autocut_with_beats`. This commit is a checkpoint, not a passing release.
