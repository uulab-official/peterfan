# Final Review: Third Runner Quality Fix

Version remains `1.31.32`.  This report covers only the two final independently
reviewed fixes from `1b0a01e6b2637382398649832574db6bcd30e559`.

## Strict TDD evidence

### Perceptual adjacent-band delta

RED: `cargo test -p peterfan-menubar
runner::quality::tests::visible_alpha_frame_delta_requires_a_meaningful_visible_alpha_change
-- --exact` failed as intended: alpha `120` versus `121` returned `1`, not `0`.

GREEN: the same focused test passed after requiring both an alpha difference
strictly greater than `DELTA_ALPHA_THRESHOLD` (`48`) and one alpha value
strictly greater than `VISIBLE_ALPHA_THRESHOLD` (`96`).  The test also proves
that `0` versus `200` and `120` versus `200` each count as one visible delta.

### Browser initial CPU lifecycle

RED: `cargo test -p peterfan-menubar
tests::browser_preview_first_real_payload_seeds_current_cpu_without_resetting_later_updates
-- --exact` failed because the generated dashboard script had no
`hasRealCpuSample` state.  The existing optimistic-selection wiring test was
also changed first and failed while it still passed literal `0` to
`startRunnerPreview`.

GREEN: both browser-preview tests passed.  The generated-dashboard lifecycle
test covers a first real `90%` sample seeding band `3`, an ordinary `60%`
payload retaining the current band until the timer's hysteresis tick, canonical
pack replacement, and optimistic selection forwarding the current payload CPU.

## Final browser lifecycle

1. Initial paint still starts the preview at synthetic CPU `0`.
2. `hasRealCpuSample` marks whether any backend CPU payload has arrived.
3. The first real payload sets the preview CPU, direct-seeds
   `runnerPreviewBand(actualCpu)`, and repaints immediately.
4. A canonical character or 32-frame pack replacement is detected by character
   plus frame content, direct-seeded from that payload's CPU, and repainted.
5. Ordinary later payloads only replace the CPU target.  The scheduled timer is
   the sole path that applies `runnerPreviewBandWithHysteresis`.
6. Optimistic character selection restarts with the current payload CPU, while
   an embedded preview whose character does not match is not installed as the
   canonical pack.

## Perceptual QA recalibration

Two deterministic QA generations produced these observed cross-band ranges:

| Metric | Observed range | Gate decision |
| --- | ---: | --- |
| Per-pose visible alpha delta | `0..224` | Existing maximum `400` retained. |
| Total visible alpha delta | `0..1199` | Maximum `3000` retained; lower bound changed from `48` to `0`. |
| Changed poses | `0..8` | Obsolete lower bound `3` removed. |

The new `0` minima are evidence-driven, not a general relaxation: some
intentional adjacent bands (for example, slime) are alpha-stable under the
corrected meaningful-alpha definition.  A nonzero lower bound would reject
those valid packs.  Upper bounds remain enforced.

## Verification

- Menubar tests: `204 passed`, `1 ignored`, `0 failed`.
- Workspace tests: `352 passed`, `6 ignored`, `0 failed`.
- Documentation tests: `0 failed`.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo build --workspace --release`: passed.
- `git diff --check`: passed before the source commit.
- QA run 1 and run 2 both passed and produced identical SHA-256 values:
  - Sprite sheet: `20afbfa555384c96f235725ec964b06686804ff1a4d52e8edcc8e2c86a4eb875`
  - Quality report: `6253a123652e008d22e516ea63a5ca24fd48ef34e2fa4dc7f45de0719925458a`

## Commits

- Base reviewed: `1b0a01e6b2637382398649832574db6bcd30e559`
- Source fix: `c79ebc4555e192c0b5d5148859aedcf67bf0afc1`
- This report: recorded in the follow-up documentation commit.

## Remaining risks / blockers

No blocker remains.  Per instruction, the app and webview were not launched;
the browser behavior is covered through the generated dashboard script's
lifecycle wiring tests rather than a live UI session.  Alpha-stable pack
transitions are deliberately accepted because this metric measures meaningful
visible alpha changes, not every possible load-band distinction.
