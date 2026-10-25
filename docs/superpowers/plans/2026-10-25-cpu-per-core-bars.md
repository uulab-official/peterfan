# CPU Per-core Bars Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Add a real logical-core-per-bar CPU menu-bar mode.

**Architecture:** Extend the existing widget configuration and bounded sample
state. Reuse `CpuMetrics.per_core` at the current one-second widget cadence and
render an original variable-width native template image; share the same data
with the settings preview. Preserve the installed app during development.

**Tech Stack:** Rust, serde, existing objc2/AppKit/tray-icon, plain JS/SVG, Node tests.

**Spec:** `docs/superpowers/specs/2026-10-25-cpu-per-core-bars-design.md` (approved).

## Global Constraints

- One bar per reported logical CPU, each on a fixed 0–100% scale.
- Entering this mode hides percentage; reselecting it preserves that preference.
- No new monitoring worker/timer, fan/helper changes or installed-app changes.
- Existing Bar/History/Number settings retain behavior and serialization.
- Keep widths stable under load; only topology/preferences may change width.
- Local/native QA is separate from later signed release/OTA/final-build six hours.

## Review Focus

- Invalid individual readings must not silently become zero or stale values.
- A topology change must resize the image and native hit area consistently.
- Selecting the active mode must not erase an explicitly re-enabled percentage.
- Failed saves must retain confirmed preferences and support retry in both UIs.
- Large core counts must retain every bar without overflowing narrow settings.

---

### Task 1: Config and sampled raster

**Files:** `packages/core/src/config.rs`, `packages/menubar/src/cpu_widget.rs`.

**Interfaces:** Add JSON/UI style `cores` / `CpuWidgetStyle::Cores` and a
`select_style` preference transition. `CpuWidgetHistory` accepts aggregate plus
per-core samples, exposes current core readings, raster pixel width, and native
point width. Keep aggregate-only sampling for existing QA/tests. Persist TOML with legacy
`style="bar"` plus `per_core_bars=true`; restore it on read and validate the flag.
A frozen v1.32.8 widget-schema test must accept the disk format and retain fan
preferences, including when the CPU widget is disabled.

- [x] Add failing config roundtrip/selection tests and raster tests: distinct
  0/50/100 heights, 1/8/10/18/32/64 bars, invalid/missing, cadence/topology/width.
- [x] Run `cargo test -p peterfan-core cpu_widget` and
  `cargo test -p peterfan-menubar cpu_widget` and observe missing-feature failures.
- [x] Implement the enum/transition and sampled core raster with fixed scale.
- [x] Run those tests to green; verify old raster tests remain green.
- [x] Commit config/raster and record results in the plan ledger.

### Task 2: Native, runtime and settings integration

**Files:** `packages/menubar/src/{native_cpu_widget.rs,cpu_widget_app.rs,main.rs,
cpu_widget.html,cpu_widget.js}`, `scripts/check-cpu-widget.test.mjs`,
`packages/menubar/examples/cpu-widget-qa.rs`.

**Interfaces:** Refresh consumes `usage: f32, per_core: &[f32]`; native image
creation consumes checked `width: u32` and RGBA data from Task 1. Settings payload
includes sampled `cpu_widget_cores`; `cores` is selectable in HTML and native menu.

- [x] Add failing JS tests for mode transition/reselection, real per-core SVG,
  invalid data and topology, save failure/retry; add native variable-image tests.
- [x] Run `node --test scripts/check-cpu-widget.test.mjs` and targeted Rust tests
  and observe the new-feature failures before integration.
- [x] Connect existing per-core samples, native image/layout/menu and settings;
  preserve the existing transaction path, tooltip language and click anchoring.
- [x] Extend isolated native lifecycle QA to cores with/without percentage and
  topology resize; validate item/image/button widths and owned storage.
- [x] Run targeted Rust/Node tests to green; commit and ledger results.

### Task 3: Product QA, documentation and integration

**Files:** `docs/CPU_MENUBAR_WIDGET.md`, approved spec/this plan; fixtures in
`packages/menubar/src/main.rs` only if needed to export per-core settings QA.

- [x] Export real production settings with per-core fixture data; inspect
  Korean/English 320/390px light/dark and keyboard/save flow through CUA.
- [x] Run isolated native lifecycle QA, all workspace Rust/Node tests, strict
  Clippy, docs/OSS/Homebrew checks and diff checks. Record exact evidence limits.
- [x] Fresh whole-branch reviewer checks the spec and review-focus cases.
  Resolve material findings with failing regression then green suite.
- [x] Update feature docs and completed checklist; preserve source/release
  distinctions. Commit, push branch, attach PR; integrate main only after CI.
- [x] Preserve public v1.32.8 and installed app/helper/config during development.
  Then complete separately authorized v1.32.9 release/OTA and record pending
  installed UI/new six-hour gates explicitly in the release ledger.

Implementation/integration completed through PR22 and exact CI. Release/OTA pass;
installed visual acceptance and the new six-hour run remain open in
`docs/CPU_PER_CORE_RELEASE_2026-10-25.md`.
