# CPU Menu-Bar Widget Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Add the approved optional CPU bar/history/number widget alongside the runner.

**Architecture:** Persist a disabled-by-default `CpuWidgetConfig` in `MenubarConfig`.
A portable bounded sample/raster module feeds a macOS-only status item from the
existing one-second refresh. The main event loop owns lifecycle and settings
transactions; both WebViews share the request-ID acknowledgement path.

**Tech Stack:** Rust, existing serde/TOML configuration transactions, tray-icon,
objc2/AppKit, embedded HTML/JavaScript and existing Node behavior tests.

**Spec:** `docs/superpowers/specs/2026-10-24-cpu-menubar-widget-design.md` (user approved).

## Global Constraints

- Existing v1.32.7 app/daemon, config and frozen six-hour observation stay unchanged.
- Use the existing isolated worktree and a new `codex/` branch; preserve primary main.
- CPU widget defaults off; styles are `bar`, `history`, `number`; percentage defaults on.
- Use existing CPU samples, a maximum 60-sample history, fixed widths and native template colors.
- No new sampling worker, dependency, privileged action or high-frequency timer.
- Keep Mac-only lifecycle gated; preserve portable builds and existing runner behavior.
- Root owns Cargo invocations to avoid concurrent builds and host disk exhaustion.

## Review Focus

- Non-finite/missing CPU readings show unavailable data instead of a false zero.
- Failed or stale settings acknowledgements never confirm a different pending save.
- Switching between the two status items anchors at the item actually clicked.
- Number-only/percentage toggles do not leave an invisible or moving status item.
- Missing, partial and older configurations preserve all unrelated settings.

### Task 1: Configuration and bounded raster model

**Files:** Modify `packages/core/src/config.rs`; create `packages/menubar/src/cpu_widget.rs`.

**Interfaces:** Core exports copyable/equatable `CpuWidgetStyle` (`Bar`, `History`,
`Number`, `parse`, `as_str`) and `CpuWidgetConfig { enabled, style, show_percentage }`.
`MenubarConfig.cpu_widget` persists it. Portable module exports
`CpuWidgetHistory::default/push/current/render_rgba`, `WIDGET_PIXEL_WIDTH=64`,
`WIDGET_PIXEL_HEIGHT=36`, and `title(config, Option<f32>) -> String`.

- [x] Write default/legacy/partial/round-trip tests and raster/history edge tests.
- [x] Confirm failing behavior before implementation; root coordinates Cargo tests.
- [x] Implement the minimal types, 60-sample optional-value history and original template raster.
- [x] Verify 0/50/100%, invalid readings, bounded history and stable title formatting.

### Task 2: Independent native status item

**Files:** Create `packages/menubar/src/native_cpu_widget.rs` (macOS only).

**Interfaces:** `NativeCpuWidget::new(config, ResolvedLanguage) -> Result<Self,String>`
creates an initially hidden owned item. Methods: `tray() -> &TrayIcon`,
`update(config, language, &CpuWidgetHistory) -> Result<(),String>`,
`set_visible(bool) -> Result<(),String>`,
`menu_action(&MenuId) -> Option<CpuWidgetAction>`.
Actions: `OpenDashboard`, `Enabled(bool)`, `Style(CpuWidgetStyle)`, `Percentage(bool)`.

- [x] Test width/image-position contracts and native bitmap dimensions/alpha.
- [x] Build native template images with owned storage, fixed size, digit font and cached updates.
- [x] Add localized right-click settings without any fan-control action.
- [x] Verify creation/drop/disabled behavior and retain menu IDs for main-loop dispatch.

### Task 3: Main-loop/settings integration

**Files:** Modify `packages/menubar/src/main.rs`; create `scripts/check-cpu-widget.test.mjs`.

- [x] Write failing JS regressions for real IPC routing, unavailable platform, delayed/stale save results and retry.
- [x] Add App fields/lifecycle, reuse one-second CPU samples and preflight a hidden item before enabling-save.
- [x] Add typed `SettingsOperation::CpuWidget` acknowledgements; apply preferences only after successful persistence.
- [x] Add Settings controls/live preview; native click/menu routing uses the correct tray ID and rectangle.
- [x] Route existing `cpu-core-graph:` messages through both WebView bridges and test the regression.
- [x] Verify saved preference reapplication after config import without modifying installed config during QA.

### Task 4: Reviewable validation and integration

**Files:** QA preview/export and feature documentation appropriate to existing tooling.

- [x] Export light/dark CPU raster/settings previews; exercise keyboard, save failure and all style choices.
- [x] Run scoped then appropriate complete Rust/Node checks with a disk-space guard; no installed app launch/replacement.
- [x] Obtain an independent read-only code review and fix actionable findings.
- [x] Commit, publish/attach PR, verify exact-head CI and integrate remote main within existing authorization. PR18 merged as `67a48a3`; PR head and main each passed all six CI jobs.
- [x] Update the existing heartbeat with source/runtime evidence boundaries; retain the v1.32.7 final-run acceptance gate. Installed v1.32.7 subsequently passed its own six-hour window; the unreleased CPU widget requires separate native-release/runtime acceptance.
