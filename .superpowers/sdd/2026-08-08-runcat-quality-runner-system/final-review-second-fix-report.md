# Runner Quality Second Final Review Fix Report

Date: 2026-08-09
Version: 1.31.32
Worktree: `/Users/uulab/Documents/workspace/uulab/peterfan/.worktrees/runcat-quality-v1.31.32`

## Scope

This amendment completes the five confirmed second-review fixes without changing
the release version, renderer output, direction goldens, or native timer path.

- Selected canonical preview payloads now carry `runner_selected_preview_character`.
  The browser installs and uses all 32 canonical frames only when it matches the
  selected character. An optimistic character selection therefore uses that
  character's keyed static preview until its matching canonical payload arrives.
- The browser seeds a new/changed canonical preview with direct `load_band`
  semantics (20/55/80), then retains its band across ordinary backend updates
  and advances with native-equivalent hysteresis (24/16, 59/51, 84/76).
- Preview prewarm builds Cat once, memoizes either success or failure, reuses a
  successful Cat pack under each requested character key, and omits failures.
- QA now has visible-alpha occupancy and adjacent-band bounds backed by
  per-pose report evidence. It retains the bounds, contact-flight, centroid, and
  clipping gates.
- The design amendment records all eight approved plan-to-manifest deviations,
  including the measured recognizability rationale for Duck, Fish, and Slime.

## Strict TDD evidence

### RED

`cargo test -p peterfan-menubar preview -- --nocapture` failed before the
preview/fallback implementation with these expected regressions:

- `selected_preview_serializes_the_complete_pack_and_uses_native_hysteresis`:
  `function runnerPreviewBand(cpu)` was absent.
- `preview_omits_failed_characters_after_one_failed_cat_fallback_attempt`:
  Cat fallback calls were `2`, not the required `1`.
- `preview_reuses_one_successful_cat_fallback_with_requested_character_keys`:
  Cat builds were `20`, not the required `1`.

`cargo test -p peterfan-menubar quality_report_covers_every_character_and_load_band -- --nocapture`
then failed to compile before the QA report/gate implementation because the
explicit occupancy constants and `cross_band_visible_deltas`,
`cross_band_visible_delta_total`, and `cross_band_changed_poses` fields did
not exist.

### GREEN

- The preview focused suite passed: 9 passed, 0 failed.
- `browser_preview_band_lifecycle_matches_native_seed_and_tick_sequence`
  passed. It starts at direct band 2 for 65%, preserves 2 at ordinary 60%, then
  follows 50→1, 52→1, 59→2, 83→2, 84→3, 76→3, and 75→2.
- The strengthened quality gate and JSON report tests passed.

## QA thresholds and measured evidence

All metrics use visible alpha (`alpha > 96`) where stated. The generated report
measured 20 characters × 4 bands.

| Gate | Enforced bound | Observed range |
| --- | ---: | ---: |
| Visible occupancy per frame | 150–520 pixels | 153–513 |
| Adjacent-band visible total | 48–3000 changed pixels | 63–2770 |
| Changed poses per adjacent band | at least 3 of 8 | 3–8 |
| Visible change in one pose | at most 400 pixels | 0–372 |

The adjacent-band total cannot pass from a single altered pose because at least
three poses must have visible-alpha changes. The per-pose maximum also rejects
an excessive one-pose jump. The previous max-any-byte `>= 1` check was removed.

## Changed files

- `packages/menubar/src/main.rs`: canonical preview provenance, no ordinary
  payload reinstall/reset, direct browser seeding plus hysteresis ticks, Cat
  fallback memoization, and focused lifecycle/fallback tests.
- `packages/menubar/src/runner/quality.rs`: reviewed occupancy constants,
  visible-alpha per-pose adjacent-band report fields, and meaningful gates.
- `docs/superpowers/specs/2026-08-08-runcat-quality-runner-system-design.md`:
  complete eight-item manifest amendment.
- `.superpowers/sdd/2026-08-08-runcat-quality-runner-system/final-review-second-fix-report.md`:
  this evidence record.

## Verification results

| Command | Result |
| --- | --- |
| `./scripts/check-runner-quality.sh` (run 1) | Passed |
| `./scripts/check-runner-quality.sh` (run 2) | Passed; hashes matched run 1 |
| `./scripts/check-docs.sh` | Passed |
| `cargo fmt --all -- --check` | Passed |
| `cargo test -p peterfan-menubar` | 202 passed, 1 ignored |
| `cargo test --workspace` | Passed: CLI 16, core 35, daemon 32, menubar 202/1 ignored, platform 65/5 ignored, TUI 0, doc tests 0 |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed |
| `cargo build --release --workspace` | Passed |
| `git diff --check` | Passed before staging implementation changes |

Stable Rust was used for all Cargo checks:
`RUSTC=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc`.

## Deterministic artifacts

- `target/runner-sprite-sheet.png` SHA-256:
  `20afbfa555384c96f235725ec964b06686804ff1a4d52e8edcc8e2c86a4eb875`
- `target/runner-quality-report.json` SHA-256:
  `b77b58627769aa97f23c3e7457fd0bdb7d5b9366732bafd61fcf617ef0103d93`

The generated `target/` artifacts remain ignored and unstaged.

## Commits

- `898b2e666e0e06b7bff736dc515d5c8b5175d7ca` —
  `fix: complete second runner quality review`
- This evidence report is committed separately after its final diff check; its
  exact commit hash is returned in the final handoff.

## Remaining risks and blockers

No blocker remains. Per task constraints, the app was neither installed nor
launched. Browser/native parity is covered through deterministic source and
sequence tests; the production native timer path was not modified.
