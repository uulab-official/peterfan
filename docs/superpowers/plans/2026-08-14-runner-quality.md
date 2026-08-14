# Runner Quality and Full-Body Push-up Animation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the shoulder-only push-up motion and harden the runner pipeline so the curated production roster remains recognizable and crisp, while legacy character values stay configuration-compatible.

**Architecture:** Keep the existing `RunnerPack` and 48×32 frame contract. Author the push-up as a landmark-driven procedural cycle, render crisp frames without geometry interpolation, and extend runner QA with contact/connectivity and loop checks. Keep open-source assets isolated from generated runtime frames and document their licenses.

**Tech Stack:** Rust workspace, embedded PNG assets, existing runner canvas/raster helpers, cargo test/clippy/fmt, shell-based runner QA.

## Global Constraints

- Runtime frames remain 48×32 RGBA with exactly eight poses.
- All visible runtime pixels remain white with alpha.
- All animations face and move left-to-right where horizontal movement applies.
- Do not add a runtime dependency or change CPU telemetry/update behavior.
- Keep existing user changes in the worktree; modify only runner assets, renderer, QA, and related docs for this feature.
- Keep `RunnerCharacter::ALL` as the persisted compatibility set and use `RunnerCharacter::FEATURED` for the native menu, picker, preview payload, and QA artifact.

---

### Task 1: Lock down the push-up failure with behavioral tests

**Files:**
- Modify: `packages/menubar/src/runner/quality.rs`
- Modify: `packages/menubar/src/runner/procedural.rs`

**Interfaces:**
- Consumes: `procedural::render(RunnerCharacter::Pushup, cpu_pct, frame)`.
- Produces: tests that fail while push-up motion changes only the upper body and pass when the full-body landmarks move.

- [ ] **Step 1: Add a test for stable contact anchors.**

  Sample every push-up frame at a fixed CPU band and assert that the hand and foot contact regions remain occupied within a one-pixel tolerance. Use the existing RGBA alpha accessor helpers in `quality.rs`; do not compare raw frame hashes because the body is expected to change.

- [ ] **Step 2: Add a test for multi-region motion.**

  Divide the silhouette into head/shoulder, torso/hip, and leg regions. Assert that at least two regions change between the high-plank and bottom frames, and assert that the torso/hip region changes as well as the shoulder/arm region.

- [ ] **Step 3: Run the focused tests and verify the current implementation fails.**

  Run `cargo test -p peterfan-menubar runner::quality::tests -- --nocapture` from the worktree. The existing shoulder-only push-up must fail the new torso/hip motion assertion.

- [ ] **Step 4: Commit the failing-test checkpoint.**

  Run `git add packages/menubar/src/runner/quality.rs packages/menubar/src/runner/procedural.rs && git commit -m "test: define full-body push-up quality gates"`.

### Task 2: Replace the push-up renderer with a landmark-driven full-body cycle

**Files:**
- Modify: `packages/menubar/src/runner/procedural.rs`
- Modify: `packages/menubar/src/runner/draw.rs` if a small connected-polyline helper is needed

**Interfaces:**
- Consumes: `cpu_pct` and the eight-frame index through `render_pose`.
- Produces: `make_pushup_runner_rgba(cpu_pct, frame) -> RgbaFrame`, selected from `render_pose` for `RunnerCharacter::Pushup`.

- [ ] **Step 1: Define eight explicit pose landmarks.**

  Store per-frame normalized coordinates for head center, shoulder, elbow, wrist, hip, knee, ankle, and toe. Keep wrist and toe ground contacts within one pixel across all frames. Use the frame sequence high plank → descent → bottom → rise → high plank, with the second half mirroring the recovery timing.

- [ ] **Step 2: Render the body as connected segments.**

  Draw the torso from hip to shoulder, upper/lower arm from shoulder through elbow to wrist, and leg from hip through knee to ankle/toe. Draw the head attached to the shoulder line. Use the existing white/detail colors and draw order so the near limbs remain legible without introducing extra colors.

- [ ] **Step 3: Apply CPU-dependent timing only to cycle speed.**

  Keep pose geometry independent of CPU percentage. Use the existing timing layer to select frames faster at higher load; do not scale the body or contact points with CPU load.

- [ ] **Step 4: Run the focused push-up tests until they pass.**

  Run `cargo test -p peterfan-menubar runner::quality::tests -- --nocapture` and confirm the new torso/hip motion and contact tests pass.

- [ ] **Step 5: Commit the renderer change.**

  Run `git add packages/menubar/src/runner/procedural.rs packages/menubar/src/runner/draw.rs packages/menubar/src/runner/quality.rs && git commit -m "fix: animate push-up with full-body poses"`.

### Task 3: Remove geometry ghosting from frame normalization

**Files:**
- Modify: `packages/menubar/src/runner/assets.rs`
- Modify: `packages/menubar/src/runner/normalize.rs`
- Test: existing asset and normalization tests in those modules

**Interfaces:**
- Consumes: source frame cycles with varying frame counts.
- Produces: deterministic nearest-frame selection for geometry-changing pixel-art poses, preserving the eight-frame contract.

- [ ] **Step 1: Add a regression test with two translated shapes.**

  Build two small alpha frames whose geometry moves horizontally and assert that the intermediate normalized frame is a crisp source pose, not a two-shape alpha blend.

- [ ] **Step 2: Replace same-pixel alpha interpolation for pixel-art source cycles.**

  When a source cycle is being expanded to eight frames, choose the nearest authored source frame by phase. Keep any existing continuous interpolation only for explicitly marked continuous effects, not character silhouettes.

- [ ] **Step 3: Run asset and normalization tests.**

  Run `cargo test -p peterfan-menubar runner::assets::tests runner::normalize::tests` and verify all existing asset validation remains green.

- [ ] **Step 4: Commit the normalization change.**

  Run `git add packages/menubar/src/runner/assets.rs packages/menubar/src/runner/normalize.rs && git commit -m "fix: keep runner pixel poses crisp during normalization"`.

### Task 4: Strengthen the visual QA report and document open-source sources

**Files:**
- Modify: `packages/menubar/src/runner/quality.rs`
- Modify: `scripts/check-runner-quality.sh`
- Modify: `assets/runner/README.md`
- Create: `target/runner-sprite-sheet.png` as a generated artifact only if the existing QA command already tracks it

**Interfaces:**
- Consumes: every runner's four CPU bands and eight frames.
- Produces: QA failures for disconnected push-up poses, low full-body motion, unexpected contact movement, and loop discontinuity; source/license documentation for CC0 references.

- [ ] **Step 1: Add region and loop metrics to the report.**

  Record per-character minimum region delta, contact drift, and first/last frame delta. Keep the JSON schema backward-compatible by adding fields rather than renaming current fields.

- [ ] **Step 2: Raise the acceptance rule above “any pixels changed.”**

  Require full-body characters to change at least two body regions, require the push-up torso/hip region to change, and require the cycle seam to stay within the existing one-pixel/contact tolerance.

- [ ] **Step 3: Document the OpenGameArt CC0 Runner Character and Kenney CC0 reference links.**

  State which files are references versus bundled assets, preserve each source's license wording, and do not claim an asset is CC0 unless its source page confirms it.

- [ ] **Step 4: Run the runner quality gate and inspect the generated sheet.**

  Run `./scripts/check-runner-quality.sh target/release` and inspect the native-size and nearest-neighbor enlarged sheets. The gate must fail if the push-up again becomes an upper-body-only change.

- [ ] **Step 5: Commit QA and documentation.**

  Run `git add packages/menubar/src/runner/quality.rs scripts/check-runner-quality.sh assets/runner/README.md` and commit with `test: enforce runner silhouette quality`.

### Task 5: Full verification and local app refresh

**Files:**
- Modify: `CHANGELOG.md` only if the repository release workflow requires a user-visible entry for this visual fix

- [ ] **Step 1: Run formatting and lint.**

  Run `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets --all-features -- -D warnings`.

- [ ] **Step 2: Run the full workspace test suite.**

  Run `cargo test --workspace` and confirm the runner-specific and unrelated package tests pass.

- [ ] **Step 3: Build the release bundle and run smoke checks.**

  Run the repository's release build and `./scripts/smoke-test.sh target/release`; confirm the generated app still starts as one process and the runner sheet is embedded.

- [ ] **Step 4: Refresh the local installed app only after verification.**

  Replace the existing `/Applications/PeterFan.app` with the verified local build using the existing release/install script, preserve a recoverable backup, launch it once, and verify the process count and startup logs.

- [ ] **Step 5: Commit the final changelog/version entry if required.**

  Run the repository's documentation/version checks, then commit only the runner-quality changes and the required changelog entry.
