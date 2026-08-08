# Final Review: Fourth Runner Quality Fix

Version remains `1.31.32`. This report records the final review correction on
top of `f612f8d33042bca860837d5ff093ea74fb779a47`.

## Superseded decision

The third report's zero-lower-bound decision is superseded. Meaningful visible
alpha remains defined as an alpha difference greater than `48` with at least
one alpha greater than `96`, but every adjacent canonical load band must now
have a total perceptual delta of at least `16`, at most the existing `3000`,
and at least `2` changed poses. The existing per-pose maximum of `400` and all
occupancy, centroid, contact/flight, clipping, rightward-direction, and timing
gates remain in force.

## Strict TDD evidence

RED was run before source changes:

```text
cargo test -p peterfan-menubar runner::quality::tests::quality_report_covers_every_character_and_load_band -- --exact --nocapture
```

It failed with the complete expected list (`0 passed`, `1 failed`, `205`
filtered):

```text
slime 0/1 total=0 changed_poses=0 deltas=[0, 0, 0, 0, 0, 0, 0, 0]
slime 1/2 total=3 changed_poses=1 deltas=[3, 0, 0, 0, 0, 0, 0, 0]
slime 2/3 total=0 changed_poses=0 deltas=[0, 0, 0, 0, 0, 0, 0, 0]
orbit 2/3 total=0 changed_poses=0 deltas=[0, 0, 0, 0, 0, 0, 0, 0]
sushi 2/3 total=3 changed_poses=1 deltas=[0, 0, 0, 3, 0, 0, 0, 0]
```

The test now aggregates every transition rather than stopping at the first
failure. The focused alpha-stable test proves that the lower bound is nonzero
and an eight-pose zero-delta band cannot pass. The quality test also preserves
the prior per-pose upper-bound assertion.

## Source geometry changes

Only procedural source geometry, before `normalize_cycle`, changed:

- Slime uses a wider bounded squash envelope (`0.60 + load * 1.80`) and
  pose-specific minimum stretch values. This makes all bands intentional while
  retaining distinct poses and the menu-bar width at representative load.
- Orbit retains its horizontal radius envelope and adds a bounded
  high-load-only vertical primary-orbit lift (`6.0 * load.powi(16)`). This
  changes the final transition without relaxing horizontal centroid or contact
  baseline constraints.
- Sushi retains its existing mid-load motion and adds a bounded high-load
  serving flourish (`5.0 * load.powi(16)`) to bob, swing, and wave geometry.

There are no normalization hacks, threshold relaxations, timer changes, or
browser lifecycle changes. The first-real-CPU browser lifecycle was exercised
unchanged by its focused generated-dashboard test.

## Reviewed golden updates

Only intentionally changed procedural outputs have new 20-character rendered
signatures (stored as the existing `u64` literals):

| Character | Old | New |
| --- | ---: | ---: |
| Slime | `5109980211131495317` | `10270468348235596930` |
| Orbit | `15157006937146022838` | `6578607126643036137` |
| Sushi | `9384635045786117440` | `16502167201706185881` |

## Deterministic QA

The ignored visual-QA test was run twice. Both runs passed (`1 passed`) and
produced identical SHA-256 values:

- Sprite sheet: `afb92070e1a50cad4124b3222a86e820ae97a269f0fb292edecc0095051ec4fb`
- Quality report: `58d226bdf23d3103b10da2900d43db89c4d0e00373337f3f404174b0b317662e`

The final report contains all 60 adjacent transitions: total perceptual delta
range `17..1199`, changed-pose range `3..8`, and per-pose range `0..224`.
There are zero lower-gate failures. Occupancy remains `153..513`, within the
required `150..520` range; the focused and full quality tests also verify the
remaining centroid, contact/flight, clipping, and direction constraints.

## Commands and results

All Rust commands used the stable toolchain override requested for this review.

- RED quality command: `0 passed`, `1 failed`, `205 filtered`.
- Focused GREEN quality module: `9 passed`.
- Focused procedural shared-quality gate: `1 passed`.
- Focused silhouette-width gate: `1 passed`.
- Focused rendered-golden gate: `1 passed`.
- Focused first-real-CPU browser lifecycle gate: `1 passed`.
- Visual QA: `2` deterministic runs, `1 passed` each, hashes above.
- `cargo fmt --all` and `cargo fmt --all -- --check`: passed.
- Menubar tests: `205 passed`, `1 ignored`, `0 failed`.
- Workspace tests: `353 passed`, `6 ignored`, `0 failed`.
- Workspace documentation tests: `0` tests, `0 failed`.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo build --workspace --release`: passed.
- `git diff --check`: passed before the source/test commit.

## Commits and remaining risk

- Reviewed base: `f612f8d33042bca860837d5ff093ea74fb779a47`
- Source/test fix: `8cad6c3a3e63afd02c2a11b46d0b33b482754223`
- This report: recorded in the documentation commit following this file.

No blocker remains. Per instruction, no app or browser UI was launched; the
preserved browser behavior is covered by the focused generated-dashboard test.
