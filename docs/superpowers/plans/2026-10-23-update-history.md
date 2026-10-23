# Update Check History Implementation Plan

> **For agentic workers:** Use the assigned bounded implementation and review ownership. Do not recursively delegate.

**Goal:** Preserve trustworthy check history across restarts without reusing install authority.

**Architecture:** A versioned native JSON sidecar holds only diagnostic history.
Main startup restores historical fields after scheduling the fresh startup check;
the discovery pipeline persists attempts and results outside its state lock.

**Tech Stack:** Existing Rust, serde/serde_json, std filesystem APIs and Node fixtures.

**Spec:** [design](../specs/2026-10-23-update-history-design.md).

## Global Constraints

- Rust >=1.88; no added dependencies; macOS and Windows support.
- Startup 20 seconds; successful discovery 21,600 seconds; existing retries.
- No real user storage, application launches or network calls from tests.
- Preserve the primary checkout's dirty files and global Codex configuration.

## Review Focus

- Restored attempts cannot suppress the new startup deadline.
- History cannot authorize an install or claim a current release.
- Mock paths cannot read or replace existing user history.
- A failed atomic replacement leaves existing data and no owned temporary file.
- Channel changes and backwards clocks cannot display another channel's future history.

## Task 1: Durable diagnostics

Files: new `packages/menubar/src/update_history.rs`, integration in `main.rs`,
regressions in module/main tests and `scripts/check-update-behavior.test.mjs`.

- [x] Reproduce restart history loss with executable regression tests.
- [x] Implement bounded parsing and atomic storage; integrate startup/begin/finish.
- [x] Run focused tests, Node fixtures and native Clippy; review the final diff.

## Task 2: Delivery and readback

Files: `docs/AUTOMATIC_UPDATES.md`, `docs/PRODUCTION_CHECKLIST.md`, `CHANGELOG.md`.

- [x] Run full contributor, Windows/MSRV and release smoke gates.
- [ ] Commit after local gates and review, push main without force, then verify CI.
- [x] Sign/notarize/install with backup and unchanged settings; verify actual restart history and fresh discovery.
- [x] Record completed checks and remaining platform/OTA limits without claiming public delivery.

This checklist is the pre-push snapshot. The pushed main commit's CI and final
delivery readback are recorded in `target/update-history/runtime-verification.json`.
