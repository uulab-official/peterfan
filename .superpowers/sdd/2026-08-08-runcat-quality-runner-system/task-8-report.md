# Task 8 Report — Deterministic Runner QA Artifacts

Date: 2026-08-08
Status: GREEN
Commit: current HEAD (`test: gate runner animation quality`)

## RED evidence

- `cargo test -p peterfan-menubar quality_report_covers_every_character_and_load_band` initially failed before the quality-report records/serialization API existed in `packages/menubar/src/runner/quality.rs`.
- The upgraded QA/timer test wave initially failed before `write_runner_quality_artifacts()` existed and before the PNG validation helper matched the current `png` API.

## GREEN evidence

- Added deterministic quality report records for all `RunnerCharacter::ALL` entries and all 4 load bands with pretty JSON serialization and stable ordering.
- Upgraded the ignored visual QA test to render a deterministic sprite sheet directly from validated `RunnerPack` frames and to parse/validate the JSON report and PNG dimensions.
- Added a native timer regression test that extracts just the timer closure body and verifies it stays limited to timing/index/image-lookup work and avoids heavy raster/decode/build paths.
- Added `scripts/check-runner-quality.sh`, marked executable, scoped to the ignored QA test, and failing on missing/empty artifacts.
- Bumped the workspace version to `1.31.32` only after the focused/package/workspace test sweep passed.
- Rebuilt and relaunched `dist/PeterFan.app`; `dist/PeterFan.app/Contents/Info.plist` now reports `CFBundleShortVersionString = 1.31.32`.

## Artifacts

- `target/runner-sprite-sheet.png`
  - size: 1,049,408 bytes
  - dimensions: 2016 × 10880
  - layout: 20 characters × 4 bands × 8 frames, each cell containing a native-size render and a 4× render

- `target/runner-quality-report.json`
  - size: 19,678 bytes
  - records: 20 character entries, 80 band entries total
  - properties: pretty JSON, stable order by `RunnerCharacter::ALL`, lowercase character names, no machine paths, no timestamps

## Test evidence

- `./scripts/check-runner-quality.sh`
  - PASS
  - ignored QA test: 1 passed

- `cargo test -p peterfan-menubar runner`
  - PASS
  - 70 passed, 1 ignored

- `cargo test -p peterfan-menubar`
  - PASS
  - 193 passed, 1 ignored

- `cargo test --workspace`
  - PASS
  - CLI: 16 passed
  - core: 35 passed
  - daemon: 32 passed
  - menubar: 193 passed, 1 ignored
  - platform: 65 passed, 5 ignored
  - tui/doc-tests: 0 failed

## Changed files

- `Cargo.toml`
- `Cargo.lock`
- `packages/menubar/Cargo.toml`
- `packages/menubar/src/main.rs`
- `packages/menubar/src/runner/mod.rs`
- `packages/menubar/src/runner/quality.rs`
- `scripts/check-runner-quality.sh`

## Concerns

- The release bundle build succeeded, but `cargo build --release` still emits non-blocking `peterfan-menubar` warnings for existing/test-only code paths. No test or build failures remain.
