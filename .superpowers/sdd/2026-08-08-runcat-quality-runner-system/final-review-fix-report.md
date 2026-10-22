# Runner Quality Final Review Fix Report

Date: 2026-08-09
Version: 1.31.32
Branch: `codex/runcat-quality-v1.31.32`

## Scope completed

The final-review fixes are implemented without changing the release version, persisted character names, or rightward runner motion. The production cache now owns the selected canonical `RunnerPack`; Settings consumes that pack rather than rebuilding a preview. The selected Settings canvas serializes all four canonical bands (32 frames) and advances through the same hysteresis transitions as native rendering. Non-macOS icon selection uses the same persistent hysteresis state.

Preview construction is now fallible: if both the requested pack and Cat fallback fail, the affected payload is omitted after one fallback attempt, with no panic and no retry loop.

## Strict TDD evidence

Focused tests were added and run before their production implementations. Recorded RED evidence:

- Complete selected-preview serialization/parity initially failed to compile because `runner_selected_preview_frames_json` and `dashboard_html_with_runner_pack` did not exist.
- Double selected/fallback failure initially failed to compile because `runner_preview_packs_with` did not exist.
- Retained production pack coverage initially failed because `RunnerAssets` had no selected-preview method and its pack was test-only.
- Canonical all-band variants initially failed with `Cat repeats bands 1 and 2`.
- Settings CSS test initially failed because the preview remained `33px` by `22px` instead of `27px` by `18px`.
- QA report coverage initially failed because `bounds`, occupancy, contact-flight, and cross-band fields were absent. After adding the fields, Orbit exceeded the measured centroid-drift gate (`2.6134872 > 2.5`); reducing its pre-normalization orbital radius was the minimal source-level correction.
- Literal rendered-direction signature coverage initially failed because the signature helper/table was absent; the hand-approved signatures below were then captured as literals.
- Non-macOS hysteresis coverage initially failed because `runner_icon_index_with_hysteresis` did not exist.
- QA script override coverage initially showed that an invalid `PETERFAN_RUST_BIN` was ignored; it now fails explicitly, and a valid override succeeds.

All of those focused tests were subsequently GREEN, including the 32-frame JSON/native parity, double-failure omission, retained pack, all-band variant, CSS, QA metric, golden-signature, and non-macOS hysteresis cases.

## Changed files

- `packages/menubar/src/main.rs`: retained selected canonical pack in production assets; fallible preview construction; full selected preview serialization; shared browser hysteresis; persistent non-macOS band state; 27x18 Settings display CSS; focused tests.
- `packages/menubar/src/runner/assets.rs`: intentional bounded per-band pre-normalization shifts.
- `packages/menubar/src/runner/procedural.rs`: CPU-responsive pre-normalization variants for Orbit and City.
- `packages/menubar/src/runner/mod.rs`: all-band cycle test and literal rendered-direction golden signatures for all 20 characters.
- `packages/menubar/src/runner/quality.rs`: QA JSON metrics and gate tests for bounds, occupancy, contact/flight motion, and cross-band differences.
- `scripts/check-runner-quality.sh`: `PETERFAN_RUST_BIN` override, active stable toolchain discovery, and local stable fallback.
- `docs/superpowers/specs/2026-08-08-runcat-quality-runner-system-design.md`: measured, approved manifest target-tuning amendment.

## Design decisions

- The 48x32 canvas backing buffers remain unchanged; CSS scales them to 27x18 for the selected Settings preview.
- Each band is generated from source geometry before normalization. No post-normalization frame adjustment, timer rasterization, or per-frame pack rebuild was introduced.
- Hysteresis transitions remain: 0→1 at 24/1→0 below 16; 1→2 at 59/2→1 below 51; 2→3 at 84/3→2 below 76.
- The golden signatures cover every rendered RGBA frame of every canonical pack, preserving hand-approved rightward-direction outputs across all 20 persisted characters.

## Verification results

| Command | Result |
| --- | --- |
| Focused RED/GREEN runner tests | Passed after the recorded RED failures above |
| `./scripts/check-runner-quality.sh` | Passed twice deterministically |
| `./scripts/check-docs.sh` | Passed |
| `cargo fmt --all -- --check` | Passed |
| `cargo test -p peterfan-menubar` | Passed: 199 passed, 1 ignored |
| `cargo test --workspace` | Passed: menubar 199/1 ignored; cli 16; core 35; daemon 32; platform 65/5 ignored; tui 0; doc tests 0 |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed |
| `cargo build --release --workspace` | Passed |
| `git diff --check` | Passed |

## QA metrics and deterministic artifacts

- Bands evaluated: 80 (20 characters × 4 bands)
- Occupancy range: 153–513 pixels
- Minimum contact-flight delta: 25 pixels
- Minimum cross-band delta after band 0: 27 pixels
- Maximum cross-band delta: 622 pixels
- Maximum centroid drift: 2.4776134 pixels
- Sprite sheet SHA-256: `20afbfa555384c96f235725ec964b06686804ff1a4d52e8edcc8e2c86a4eb875`
- Quality report SHA-256: `6371c303770ece1a704c2fb21e7b7707beabfdc861db4d36ce12b564bdd3cbd0`

The generated artifacts remain under ignored `target/` paths and were not staged.

## Final implementation commit

`abfd6cc9ec088f4031d2ed798cb19c324ad67c4f` — `fix: complete runner quality review gates`

## Remaining risks

No unresolved code or test gate remains. Per the task constraint, the production app was not launched, so native WebView behavior was verified through source-level parity and automated tests rather than a manual app session.
