# Menu-bar item visibility implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Independently show CPU widget, runner character and metric text in all seven non-empty combinations.

**Architecture:** Keep `MenubarDisplay::{Number,Graph,Both}` and add a default-on `primary_visible` boolean to `MenubarConfig`. A typed three-boolean visibility value maps onto the existing display enum, primary visibility and CPU enabled flag in one atomic configuration update. Stage native items before persistence and preserve access and runner timer ownership when changing visibility.

**Tech Stack:** Rust, AppKit/tray-icon, shared HTML/JavaScript, Rust unit tests and Node behavioral tests.

**Spec:** `docs/superpowers/specs/2026-10-26-menu-item-visibility-design.md`

## Global Constraints

- Three independent checkboxes; all seven non-empty combinations; require at least one visible item.
- Preserve selected metric, CPU style/percentage and runner character preferences while hidden.
- Keep existing shared enum values readable by resident helper 1.32.7/1.32.8; minimum helper stays 1.32.7.
- Persist atomically before applying the new visible set; failed saves leave the previous visible set and allow retry.
- Keep dashboard access in CPU-only/runner-only/metric-only; preserve same-item toggle, cross-item anchor and WebView-refresh geometry.
- No new sampler, worker or timer; no fan-control changes; controls remain macOS-only.
- Do not alter installed v1.32.9, resident helper, live config, immutable tags or public assets during implementation.
- Reuse this worktree/branch. Preserve primary checkout/ref/diff and all unrelated work. No global Codex configuration changes.
- Implementation was explicitly approved; continue without another plan-approval round. Release and final installed six-hour evidence remain separate stages.

## Review Focus

- External all-off configuration must recover dashboard access without resetting fan configuration.
- CPU-only staging/native failure must keep a usable primary item and return an honest failure.
- Legacy display commands and CPU hide menu actions must enforce the same invariant as the new UI.
- Pending/stale/failed settings acknowledgements must not falsely confirm visibility or discard detailed preferences.
- Hidden primary items must stop runner animation and restore native width/image/click identity when shown again.

---

### Task 1: Atomic independent visibility across configuration, native lifecycle and settings

**Files:**
- Modify: `packages/core/src/config.rs` — backwards-compatible flags, typed visibility mapping and tests.
- Modify: `packages/menubar/src/main.rs` — app state, typed settings operation, payload, command/native menu routing, hot reload and runner lifecycle.
- Modify: `packages/menubar/src/cpu_widget_app.rs` — shared invariant and native staged transaction.
- Modify: `packages/menubar/src/native_cpu_widget.rs` — consistent visibility context menu and last-item guard.
- Create: `packages/menubar/src/menu_items_app.rs` — focused visibility request/transaction logic if needed to keep main changes small.
- Create: `packages/menubar/src/menu_items.html`, `packages/menubar/src/menu_items.js` — shared controls and acknowledgement state.
- Modify: shared injection and old display-control placement in `packages/menubar/src/main.rs`.
- Create: `scripts/check-menu-items.test.mjs` — real shared JS behavioral tests.
- Modify: existing CPU/settings tests and QA example only where changed contracts require it.
- Modify: `docs/CPU_MENUBAR_WIDGET.md` — current source behavior and explicit pending delivery boundary.

**Interfaces:**
- Consume: existing `MenubarDisplay`, `CpuWidgetConfig`, atomic `peterfan_platform::config::update`, settings request/result IDs and GUI event loop.
- Produce: `MenuItemsVisibility { cpu: bool, runner: bool, metric: bool }`, serialized with exact keys and rejecting unknown/missing/wrong-typed values.
- Produce: `MenubarConfig::item_visibility(&self) -> MenuItemsVisibility`, `MenubarConfig::set_item_visibility(&mut self, visibility: MenuItemsVisibility) -> Result<(), &'static str>`.
- Preserve underlying `display` when runner and metric are both hidden; runtime mapping recovers a visible metric for invalid external all-off input without rewriting unrelated config.
- Settings operation `menu_items` uses one complete typed visibility payload and returns native acknowledgement through existing settings results. Payload key `menu_items` carries confirmed effective state, key `menu_items_supported` gates macOS controls.
- Both hosts consume one shared HTML/JS group. CPU appearance remains separate; remove competing primary display chooser from macOS settings while retaining Windows behavior.

- [ ] **Step 1:** Add failing Rust tests for all seven combinations, all-off rejection, default migration, hiding/re-enabling preference retention, external all-off recovery and legacy-helper parsing of resulting TOML with unrelated fan settings.
- [ ] **Step 2:** Run focused core tests and record the expected missing-feature failure; implement the typed mapping and repeat until green.
- [ ] **Step 3:** Add failing transaction/parser tests for strict payload, failed save preserving prior live state, retry and last-item protection through CPU and legacy entry points. Implement staged native transaction and routing; prepare CPU-only before hiding primary; restore primary on external/native failure.
- [ ] **Step 4:** Add failing Node behavioral tests for all seven checkbox states, last active disabled guard, macOS gating, exact acknowledgement IDs, save failure/retry/timeout/stale response, simultaneous CPU-style vs visibility edits, and reload preserving hidden preferences. Implement the shared group and subordinate CPU controls.
- [ ] **Step 5:** Run focused Rust/Node tests and compare expected green outputs. Extend native QA to show/hide primary, CPU-only dashboard anchor, runner-only/metric-only, last-item protection and restore lifecycle where the harness supports it. Do not call fixture evidence installed-app evidence.
- [ ] **Step 6:** Run `cargo fmt --all -- --check`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `node --test scripts/*.test.mjs`, docs/OSS checks and `git diff --check`; capture output and report exact failures without relaxing limits.
- [ ] **Step 7:** Commit only feature files and mark the implementation report complete with commands/results and UI evidence limitations. Do not change version, publish, install or start a soak as part of this task.

### Integration and final review

- [ ] Review the task diff for spec compliance and code quality with an independent reader; reproduce and fix material findings with failing tests first.
- [ ] Verify real rendered settings through CUA at 320/390px, Korean/English, light/dark, keyboard and retry. Native fixture QA remains distinct from installed-client acceptance.
- [ ] Update source docs and progress ledger with exact observed results and pending release/installed-UI/six-hour stages.
- [ ] Continue the previously authorized release workflow only after implementation/CI gates pass, using a fresh immutable version and preserved resident helper. Keep existing heartbeat updated with the actual stage; no duplicate automation.
