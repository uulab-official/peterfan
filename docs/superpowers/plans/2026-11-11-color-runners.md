# Color Runners Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans to implement and verify each task.

**Goal:** Replace all 20 runner appearances with coherent, looping, colored dimensional sprite animations while preserving their identity and CPU-responsive speed.

**Architecture:** Author transparent eight-pose source cycles, normalize them into the existing cached 48×32 RGBA RunnerPack for all four CPU bands, and retain shared menu-bar and preview consumers and cadence. Preserve legacy config identifiers. Color must survive both the native icon and settings preview, which currently include template/alpha transformations and require inspection.

**Tech Stack:** GPT Image, PNG, Rust, existing menubar renderer and QA tools.

**Spec:** docs/superpowers/specs/2026-11-11-color-runner-design.md

## Tasks

- [ ] Produce eight-pose transparent sources for every one of the 20 existing characters. Preserve originals and record generation provenance.
- [ ] Inspect each sheet and correct inconsistent size, silhouette, missing poses, accidental background and frame-to-frame motion. Render looping previews at actual menu-bar dimensions; reject sheets whose motions do not loop naturally.
- [ ] Add a sprite ingestion path in `packages/menubar/src/runner/assets.rs` and integrate it through `runner/mod.rs`, preserving cache, dimensions, alpha and settings IDs. Preserve existing assets for fallback.
- [ ] Preserve RGBA through native menu bar and Settings preview; inspect template image behavior rather than assuming color survives. Maintain light/dark legibility and matching previews.
- [ ] Verify eight frames, valid alpha and bounds for all 20 runners, loop continuity and character-specific motion; test CPU cadence, phase continuity and all load bands using existing runner tests and focused regression tests.
- [ ] Run formatting, relevant Rust tests and quality reports; visually inspect actual-size animated output and real installed UI where isolated target-scoped control is available. Report missing device evidence rather than treating fixtures as installed proof.

Do not change the primary checkout or running app, modify past release assets, or publish a release as part of this work without the appropriate release verification.
