# Task 7 Report — Unify Native and Settings Frames in `RunnerPack`

Status: GREEN (follow-up fix applied)

Commit: `fix: make runner asset cache conversion fallible`

Scope:
- `packages/menubar/src/runner/mod.rs`
- `packages/menubar/src/main.rs`
- `packages/menubar/src/runner/normalize.rs`
- `.superpowers/sdd/2026-08-08-runcat-quality-runner-system/task-7-report.md`

## RED evidence

Initial focused RED run:

- Command: `cargo test -p peterfan-menubar runner::tests`
- Result: compile failure, as expected
- Missing APIs/errors included:
  - `could not find RunnerPack in runner`
  - `cannot find function alpha_hex`
  - `cannot find function build_or_fallback_with`
  - `cannot find function runner_preview_alpha_json_for`
  - `cannot find function runner_preview_frames_json_for`
  - `cannot find function build_runner_assets`
  - `cannot find function replace_runner_assets_with`

This confirmed the new pack/fallback/preview/atomic-switch interfaces were not present before implementation.

Follow-up RED run for the cache-conversion review fix:

- Command: `cargo test -p peterfan-menubar runner_character_switch_keeps_previous_assets_on_icon_cache_failure`
- Result: compile failure, as expected
- Missing/failing pieces included:
  - `no variant named Cache found for enum RunnerBuildError`
  - `build_runner_assets_with` still accepted only pack-builder injection
  - `runner_icon_from_rgba` still returned `Icon` directly
  - `make_runner_native_image_from_rgba` still returned `NSImage` directly
  - `initialize_runner_assets_with` did not exist
  - `runner_startup_fatal_message` did not exist

This confirmed the review issue: the runtime asset/cache conversion path was still not fallible end-to-end.

## GREEN evidence

Focused pack pass:

- Command: `cargo test -p peterfan-menubar runner::tests`
- Result: `6 passed, 0 failed`
- Verified:
  - 32-frame pack shape
  - parity between cached RGBA and preview alpha
  - bounds handling
  - Cat fallback pack behavior
  - exactly four cycle builds per pack

Follow-up focused cache-failure passes:

- `cargo test -p peterfan-menubar runner_character_switch_keeps_previous_assets_on_icon_cache_failure` → `1 passed, 0 failed`
- `cargo test -p peterfan-menubar runner_character_switch_keeps_previous_assets_on_native_cache_failure` → `1 passed, 0 failed`
- `cargo test -p peterfan-menubar startup_runner_initialization_returns_controlled_error_when_cat_cache_conversion_fails` → `1 passed, 0 failed`

Verified:

- icon cache conversion errors return bounded `RunnerBuildError::Cache`
- native cache conversion errors return bounded `RunnerBuildError::Cache`
- previous character/assets remain intact on icon/native cache failure
- startup returns a controlled fatal initialization error if Cat cache conversion also fails
- runtime cache build no longer uses `expect`/panic in icon/native conversion

Focused UI/native/settings passes:

- `cargo test -p peterfan-menubar runner_picker` → `1 passed, 0 failed`
- `cargo test -p peterfan-menubar native_runner` → `2 passed, 0 failed`
- `cargo test -p peterfan-menubar settings` → `9 passed, 0 failed`

Full package pass:

- `cargo test -p peterfan-menubar`
- Result: `190 passed, 0 failed, 1 ignored`

Full workspace pass:

- `cargo test`
- Result:
  - `peterfan`: `16 passed`
  - `peterfan_core`: `35 passed`
  - `peterfand`: `32 passed`
  - `peterfan_menubar`: `190 passed, 1 ignored`
  - `peterfan_platform`: `65 passed, 5 ignored`
  - `peterfan_tui`: `0 tests`
  - doc-tests: `0 tests`
- Aggregate: `338 passed, 0 failed, 6 ignored`

## Implementation summary

- Added `RunnerPack` as the shared 32-frame validated RGBA cache in `runner/mod.rs`.
- Added `alpha_hex`, `build_or_fallback`, and `build_or_fallback_with`.
- Styled cached pack frames once so tray icons, native images, and settings previews all consume the same normalized RGBA bytes.
- Added `RunnerAssets` in `main.rs` so the selected app state keeps one pack plus derived icon/native caches.
- Replaced production duplicate RGBA generation paths for:
  - tray icon cache
  - native `NSImage` cache
  - settings selected preview alpha
  - picker still-frame alpha list
- Preserved previous character/cache state on failed character switch and bounded the status/log message.
- Startup now may fall back to Cat while retaining/logging the original build error.
- Made the asset pipeline fallible end-to-end for:
  - icon conversion
  - native image conversion
  - frame-length validation
  - startup fallback cache conversion
- Added controlled startup initialization failure handling instead of panicking when even Cat cache conversion cannot succeed.

## Performance / build-count evidence

- Test: `runner_pack_builds_exactly_four_cycles_per_character`
- Evidence: one `RunnerPack::build` performs exactly `4` normalized cycle builds at CPU bands `0.0`, `30.0`, `65.0`, `90.0`
- Effect: closes the Task 6 minor performance issue by avoiding per-frame cycle reconstruction in the shared production cache path.

## Atomic-switch / fallback evidence

- Test: `build_failure_falls_back_to_cat_without_emptying_the_cache`
- Test: `runner_character_switch_keeps_previous_assets_on_build_failure`
- Test: `runner_character_switch_keeps_previous_assets_on_icon_cache_failure`
- Test: `runner_character_switch_keeps_previous_assets_on_native_cache_failure`
- Test: `startup_runner_initialization_returns_controlled_error_when_cat_cache_conversion_fails`
- Evidence:
  - startup fallback returns a valid 32-frame Cat pack with the original error retained
  - runtime character-switch failures keep the previous character and cached assets intact
  - icon/native cache-stage failures keep the previous assets intact
  - Cat startup cache failure returns a controlled fatal initialization error instead of an uncontrolled panic

## Changed files

- `packages/menubar/src/runner/mod.rs`
- `packages/menubar/src/main.rs`
- `packages/menubar/src/runner/normalize.rs`
- `.superpowers/sdd/2026-08-08-runcat-quality-runner-system/task-7-report.md`

## Concerns

- Release-version bump to `v1.31.32` and the release-build/relaunch flow were not performed in this task because this follow-up stayed scoped to the Task 7 review fix.
