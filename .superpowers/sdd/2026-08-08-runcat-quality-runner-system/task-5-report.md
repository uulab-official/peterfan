# Task 5 Report

- Status: complete after review fixes
- Commit: `fix: validate final asset runner cycles`

## Changed files

- `packages/menubar/src/runner/assets.rs`
- `.superpowers/sdd/2026-08-08-runcat-quality-runner-system/task-5-report.md`

## RED failures by character and metric

### Review-pass red run

Command:

`cargo test -p peterfan-menubar runner::assets::tests -- --nocapture`

Initial review regressions reproduced exactly as:

- All seven asset-backed characters (Cat, Dog, Rabbit, Fox, Dinosaur, Ghost, Human): `render_matches_build_cycle_frames_for_all_asset_characters_and_bands` failed because `render(...)` returned a post-shifted frame that did not byte-equal `build_cycle(...)[frame]`
- Five-pose mapping regression: `five_pose_resampling_uses_nearest_pose_when_alpha_delta_is_large` failed at pose `2`, selecting source pose `2` instead of the mathematically nearest source pose `1`
- Unsupported asset character regression: `unsupported_asset_characters_return_runner_build_error` panicked on `unreachable!` for `RunnerCharacter::Robot`
- Sample/crop diagnostics regression: `sample_alpha_out_of_bounds_reports_the_actual_character` reported `runner 'cat'` instead of the requested `runner 'dog'`

Intermediate review reds encountered and resolved while moving CPU-band motion into normalization input:

- Dog: `validate_cycle[union_max]` at 90% (`union 42x23 exceeds target 42x22`)
- Fox: `validate_cycle[union_max]` at 90% (`union 44x21 exceeds target 44x20`)
- Dinosaur:
  - `normalize_cycle[clip]` on frame `7`
  - `validate_cycle[union_max]` at 0/30/65/90 (`union 34x29`, `37x29`, `40x29`, then `40x28` / `40x30` during tuning)
  - `validate_cycle[clipped]` on frame `4` at 30/65/90 during one over-aggressive motion-envelope attempt
- Ghost: `validate_cycle[minimum_frame_delta]` regressed to `12px` during one shared-motion-table attempt
- Human: broader runner suite red `runner_silhouettes_are_wide_and_fill_the_menu_bar_slot` reported `Human is too short: 20px`

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

   `cargo test -p peterfan-menubar runner::assets::tests`

   Result: `5 passed; 0 failed; 0 ignored; 173 filtered out`

2. Runner gait checks:

   `cargo test -p peterfan-menubar runner_gait`

   Result: `2 passed; 0 failed; 0 ignored; 176 filtered out`

3. Broader runner suite:

   `cargo test -p peterfan-menubar runner`

   Result: `54 passed; 0 failed; 1 ignored; 123 filtered out`

4. Full workspace suite:

   `cargo test`

   Result: exit `0`

   - `peterfan` CLI: `16 passed`
   - `peterfan_core`: `35 passed`
   - `peterfand`: `32 passed`
   - `peterfan_menubar`: `177 passed, 1 ignored`
   - `peterfan_platform`: `65 passed, 5 ignored`
   - `peterfan_tui`: `0 tests`
   - doc tests: `0 tests`

   Aggregate executable/doc result: `325 passed, 0 failed, 6 ignored`

## Implementation summary

- Kept the embedded-sheet decode path cached once per asset with `OnceLock<Result<[RgbaFrame; 8], RunnerBuildError>>`, then added cached final per-band cycles so `build_cycle(character, cpu_pct)` returns the exact 8-frame cycle used by `render(...)`.
- Moved the shared CPU-band motion envelope into normalization input: `build_cycle` now applies the per-band motion shifts to the source cycle first, normalizes that exact motioned source, validates the exact final cycle, and `render` only indexes the cached final frame.
- Removed the previous-source / alternate resampling logic and kept deterministic nearest-pose fallback with a stable tie rule when adjacent source alpha delta exceeds the 35% visible-pixel threshold.
- Removed the dinosaur post-normalization stretch and ghost per-pose post-normalization offsets; the passing fit now comes from shared preprocessing plus exact crop/draw metadata only.
- Changed unsupported asset-character handling from `unreachable!` to `RunnerBuildError`, and passed the real character through sample-bound diagnostics.
- Added regression coverage for:
  - render/build byte equality for all 7 asset-backed characters × CPUs `0/30/65/90` × 8 frames
  - deterministic 5→8 nearest-pose mapping
  - unsupported asset-character error handling
  - actual-character sample-out-of-bounds diagnostics

## Full-suite result

- Full `cargo test`: passed

## Concerns

- Asset-backed characters still rely on character-specific source-preprocessing fits because the public-domain strips do not naturally land inside the shared 48×32 runner gate after uniform normalization.
- No known functional blockers remain after the mandated suites; the remaining complexity is concentrated in `packages/menubar/src/runner/assets.rs` where the heterogeneous source strips are normalized into the common runner quality contract.
