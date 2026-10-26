# Battery Health Details Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show charge, health, design/full-charge capacity and cycles inline in the existing hardware battery area.

**Architecture:** Preserve the cheap `battery()` path used by daemon power rules. Add an opt-in `battery_details()` path with native Mac capacities and an on-demand, bounded, cached System Profiler health probe; poll its completion from existing monitor ticks, adding no worker or timer. Forward structured optional values through the existing dashboard slow cache and render explicit unavailable/estimated states.

**Tech Stack:** Rust, existing CoreFoundation/IOKit bindings and serde JSON, embedded dashboard HTML/JS, Node behavior harness, CUA.

**Spec:** `docs/superpowers/specs/2026-10-26-battery-health-details-design.md` (user approved design and implementation).

## Global Constraints

- Preserve installed v1.32.10, helper v1.32.7, current preferences, fan control and immutable release assets during development.
- Charge and health are distinct; OS-reported health takes precedence. Label full/design fallback as estimated; never predict remaining years.
- Physical capacity fields must share verified mAh provenance. Never treat normalized `MaxCapacity=100` as mAh; unknown fields stay unavailable.
- No new sampler, worker or timer; no subprocess in daemon `battery()`, quick CPU sampling or per-frame rendering. Polling an already requested probe is permitted.
- System Profiler runs only on an explicit battery-detail request, at most once per 300 seconds, with 10-second timeout, bounded output, owned-child cleanup and successful-cache expiry after 600 seconds.
- Preserve helper floor1.32.7; portable/mock backends compile. No new third-party dependency is needed.

## Review Focus

- Normalized percentages alongside raw capacities must never produce a falsely reported health percentage.
- Failed/hung/oversized profiler output must not block the UI, launch repeated processes, leak children or keep stale reported health indefinitely.
- Missing/zero/nonfinite data and battery-free desktops must show unavailable/no-battery rather than fabricated zero or old values.
- Restored/legacy JSON and supported non-Mac platforms must retain compatibility and clearly identify derived health.
- Korean/English narrow layouts and changing battery presence must not overflow or retain prior detail values.

---

### Task 1: End-to-end battery details and isolated QA

**Files:**
- Modify: `packages/core/src/metrics.rs`, `packages/core/src/monitor.rs` (additive optional fields and default detailed accessor).
- Create: `packages/platform/src/macos_battery.rs` (native acquisition, bounded nonblocking probe/cache and tests).
- Modify: `packages/platform/src/lib.rs`, `packages/platform/src/system.rs`, `packages/platform/src/mock_monitor.rs`.
- Modify: `packages/menubar/src/main.rs` (slow cache/state, hardware rows, localization, export fixture and tests).
- Modify: `scripts/check-dashboard-behavior.mjs` (actual embedded JS behavior).

**Interfaces:**
- `BatteryInfo` adds serde-defaulted `health_source: Option<BatteryHealthSource>`, `design_capacity_mah: Option<u32>` and `full_charge_capacity_mah: Option<u32>`; enum serializes `reported`/`estimated`.
- `SystemMonitor::battery_details(&self) -> Option<BatteryInfo>` defaults to `battery()`; `SysinfoMonitor` enriches only this explicit path. Keep `battery()` free of profiler work.
- Dashboard slow cache retains an optional detailed battery snapshot and exports `batt_details` with the existing charge/presence payload. UI uses only the five requested measurements, with textContent/null-safe formatting.

- [ ] Write failing core/backend tests: legacy deserialization defaults; raw5655/design6249 is estimated, OS96 overrides it; normalized100 is never a physical fallback; zero/missing/nonfinite/wrong JSON types fail closed; valid low physical capacity is not mistaken for a normalized field solely by magnitude.
- [ ] Write failing deterministic probe tests: pending/ready/timeout/nonzero/invalid/oversized output, one request per300s, cache expiry600s, drop kills/reaps only its own child. Verify no process creation from basic battery/CPU paths. Use injected time/probe fixtures and an ignored explicit real Mac probe for source readback.
- [ ] Implement validated native capacity acquisition and pollable bounded System Profiler capture. Reuse existing monitor ticks for completion/timeout; no blocking wait on a live child and no pipeline through shell. Parse only the internal battery health subtree; collect no serials into output/evidence.
- [ ] Write failing JS tests for reported96 versus charge76, estimated label, unknown per-field, zero cycles, no battery, presence changes and escaped text. Add Rust EN/KO label checks and a target-only real-template hardware export fixture.
- [ ] Implement the inline hardware rows and structured slow-cache forwarding. Preserve the compact charge meter and all fan/menu actions. Each missing field is unavailable; absent batteries hide detail rows and retain a clear no-battery label.
- [ ] Run focused tests RED→GREEN, then workspace tests/Clippy/fmt, Node behavior/docs/OSS checks. Export and inspect actual template using CUA at ko/en320/390 in light/dark; preserve screenshots and distinguish fixture from installed evidence.
- [ ] Commit the completed bounded feature and request an independent read-only review against this spec; fix material findings with regressions before delivery.

### Task 2: New immutable release and installed acceptance

**Files:** Version metadata/changelog/current docs and new release ledger/evidence; operational scripts under a fresh ignored `target/` release directory.

- [ ] After feature verification/review, prepare next available version, PR/exact-source CI/main/tag. Keep tag10 and public bytes unchanged and retain the compatible resident helper7.
- [ ] Sign/notarize and verify complete Mac+native Windows assets, provider/download/manifests; publish once and prove real public-feed OTA from the genuine previous released CLI. Preserve non-feature preferences and record new readiness/PID/start/bytes/trust/health.
- [ ] Exercise the actual installed battery UI when CUA can access it. Mark that gate pending if inaccessible; never substitute fixtures or repeat the outstanding dashboard-open question.
- [ ] After all final installed UI gates, calibrate and start a fresh continuous21,600s observation with the existing strict launcher/limits. No older uptime, fixture or calibration contributes time. Use only the existing peterfan-6 heartbeat; report real results and scope through evidence PR/CI/main.

## Execution decision

The user explicitly approved implementation and automatic task routing. Execute this bounded feature with one implementation owner and a fresh independent review; do not request another plan/method approval. The primary agent owns delivery metadata and preserves the operational baseline. Raw test/release evidence remains in the owned ignored QA directories.
