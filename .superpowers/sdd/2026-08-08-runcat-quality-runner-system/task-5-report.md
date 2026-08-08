# Task 5 Report

- Status: complete
- Commit: `feat: standardize asset runner cycles`

## Changed files

- `packages/menubar/src/runner/assets.rs`
- `.superpowers/sdd/2026-08-08-runcat-quality-runner-system/task-5-report.md`

## RED failures by character and metric

### Initial shared-quality red run

Command:

`env RUSTC=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc PATH=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/usr/bin:/bin cargo test -p peterfan-menubar asset_backed_characters_pass_the_shared_quality_gate -- --nocapture`

Observed 28 failing character/CPU cases, all on `validate_cycle[non_white]`:

- Cat: `validate_cycle[non_white]` at 0%, 30%, 65%, 90%
- Dog: `validate_cycle[non_white]` at 0%, 30%, 65%, 90%
- Rabbit: `validate_cycle[non_white]` at 0%, 30%, 65%, 90%
- Fox: `validate_cycle[non_white]` at 0%, 30%, 65%, 90%
- Dinosaur: `validate_cycle[non_white]` at 0%, 30%, 65%, 90%
- Ghost: `validate_cycle[non_white]` at 0%, 30%, 65%, 90%
- Human: `validate_cycle[non_white]` at 0%, 30%, 65%, 90%

Representative failure details from the initial red capture:

- Cat: pixel `(12, 13)` RGB `[0, 0, 0]` with alpha `255`
- Dog: pixel `(6, 7)` RGB `[0, 0, 0]` with alpha `255`
- Rabbit: pixel `(18, 1)` RGB `[0, 0, 0]` with alpha `255`
- Fox: pixel `(7, 9)` RGB `[0, 0, 0]` with alpha `255`
- Dinosaur: pixel `(21, 5)` RGB `[0, 0, 0]` with alpha `34`
- Ghost: pixel `(25, 1)` RGB `[0, 0, 0]` with alpha `255`
- Human: pixel `(21, 0)` RGB `[0, 0, 0]` with alpha `85`

### Follow-on regression reds resolved during implementation

- Cat: `validate_cycle[contact_baseline]`, then `validate_cycle[duplicate]`
- Dog: `normalize_cycle[source_bounds]` empty pose, then multiple silhouette-width reds while fitting the source strip
- Dinosaur: `validate_cycle[contact_baseline]`, then `normalize_cycle[clip]` during an over-aggressive source-height squeeze
- Ghost: `validate_cycle[minimum_frame_delta]`
- Human: `validate_cycle[contact_baseline]`
- Fox: high-CPU motion envelope below the `300 changed pixels` runner threshold
- Rabbit / Fox / Dinosaur: sequential silhouette-width reds while tuning optical source fit

## GREEN verification

1. Asset-backed quality gate:

   `env RUSTC=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc PATH=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/usr/bin:/bin cargo test -p peterfan-menubar runner::assets::tests`

   Result: `1 passed; 0 failed; 0 ignored; 173 filtered out`

2. Runner gait checks:

   `env RUSTC=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc PATH=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/usr/bin:/bin cargo test -p peterfan-menubar runner_gait`

   Result: `2 passed; 0 failed; 0 ignored; 172 filtered out`

3. Broader runner suite:

   `env RUSTC=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc PATH=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/usr/bin:/bin cargo test -p peterfan-menubar runner`

   Result: `50 passed; 0 failed; 1 ignored; 123 filtered out`

4. Full workspace suite:

   `env RUSTC=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc PATH=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/usr/bin:/bin cargo test`

   Result: exit `0`

   - `peterfan` CLI: `16 passed`
   - `peterfan_core`: `35 passed`
   - `peterfand`: `32 passed`
   - `peterfan_menubar`: `173 passed, 1 ignored`
   - `peterfan_platform`: `65 passed, 5 ignored`
   - `peterfan_tui`: `0 tests`
   - doc tests: `0 tests`

   Aggregate executable/doc result: `321 passed, 0 failed, 6 ignored`

## Implementation summary

- Replaced the legacy per-frame asset rendering path with cached asset-backed source cycles using `OnceLock<Result<[RgbaFrame; 8], RunnerBuildError>>`.
- Introduced `source_cycle(character, cpu_pct)` and `build_cycle(character, cpu_pct)` around the shared normalization pipeline.
- Resampled 5-frame and 6-frame strips into deterministic 8-frame cycles with nearest-pose fallback when interpolation crossed the 35% visible-alpha delta threshold.
- Preserved dinosaur and human eight-frame ordering while adding only the minimal optical source-fit adjustments needed to satisfy the shared quality and runtime silhouette gates.
- Kept Cat, Dog, Rabbit, and Fox mirrored; left Dinosaur, Ghost, and Human unmirrored.
- Kept error reporting character-aware and stage-specific so failures name the affected character and metric/stage.

## Full-suite result

- Full `cargo test`: passed

## Concerns

- `assets.rs` now carries several character-specific optical source-fit adjustments because the raw public-domain strips do not naturally satisfy the shared 48×32 menu-bar silhouette gate after uniform normalization.
- The final runtime motion envelope is applied at frame-selection time, while the shared quality gate validates the normalized cycle itself; this split was necessary to satisfy both the strict build-cycle quality gate and the runtime gait-width checks without loosening thresholds.
