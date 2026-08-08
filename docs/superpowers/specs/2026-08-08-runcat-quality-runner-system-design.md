# RunCat-Class Runner Quality System

Date: 2026-08-08
Status: Approved design
Target release: v1.31.32

## Objective

Bring all twenty PeterFan menu-bar runners to one production-quality standard:
recognizable at native menu-bar size, visually stable across frames, responsive
to CPU load, inexpensive while idle, and identical in the menu bar and Settings
preview.

This work improves the runner system rather than copying RunCat's artwork. The
existing licensed source assets and PeterFan's original procedural characters
remain the source material.

## Success Criteria

- Every character faces and travels from left to right.
- Every output frame is a white template glyph on a transparent 48×32 canvas.
- Each cycle has eight distinct frames with an understandable contact, flight,
  and landing progression.
- Frame-to-frame horizontal centroid drift is at most 2.5 pixels unless a
  character-specific manifest explicitly allows a larger value for meaningful
  motion.
- Contact frames share a stable visual ground line and do not appear to float.
- The visible silhouette fits the menu-bar slot without clipping or becoming a
  featureless block.
- CPU spikes accelerate quickly; falling CPU load decelerates smoothly without
  changing pose order or resetting animation phase.
- The Settings picker renders the exact same normalized frames as the native
  status item.
- The animation loop swaps cached native images and performs no asset decoding,
  resampling, or procedural drawing per tick.
- All automated tests, Clippy, documentation checks, release build, local bundle
  launch, and WebView readiness checks pass before completion.

## Scope

### Included

- All twenty current `RunnerCharacter` values.
- Imported cat, dog, rabbit, fox, dinosaur, ghost, and human assets.
- Existing procedural penguin, robot, bird, duck, sheep, fish, slime, push-up,
  orbit, sushi, city, sausage, and party characters.
- Shared frame normalization, menu-bar rendering, picker preview rendering,
  CPU-driven timing, QA artifacts, and regression tests.
- Release version bump to v1.31.32 and local PeterFan.app replacement/relaunch.

### Excluded

- New runner characters or downloadable runner packs.
- Copying or redistributing RunCat artwork.
- Popover information architecture, fan-control behavior, or sensor backends.
- GPU-driven animation speed.

## Chosen Approach

Use a canonical runner pipeline with per-character manifests and one shared
quality gate.

The alternatives were replacing all artwork with one third-party pack or
reducing the runner count. A third-party pack would create licensing and
coverage risk, while reducing the count would remove an existing product
capability. The canonical pipeline preserves the current catalog and fixes the
actual problem: inconsistent source geometry and character-specific rendering.

## Architecture

The runner implementation will be extracted from the large menu-bar entry point
into a focused `runner` module. The public surface remains small:

- `RunnerPack::build(character)` returns the four CPU load bands, each with
  eight normalized RGBA frames.
- `RunnerPack::native_images()` converts those frames to cached macOS template
  images once when a character becomes active.
- `RunnerPack::preview_payload()` exposes the same alpha frames to the Settings
  WebView.
- Timing helpers map smoothed CPU load to frame cadence without depending on
  asset internals.

Suggested internal boundaries:

- `runner/assets.rs`: decode and crop licensed sprite sheets.
- `runner/procedural.rs`: draw PeterFan's original characters.
- `runner/normalize.rs`: direction, canvas placement, baseline, alpha, and
  silhouette normalization.
- `runner/timing.rs`: CPU smoothing, hysteresis, gait weighting, and phase.
- `runner/quality.rs`: reusable measurements used by tests and QA artifacts.

The entry point owns only selection, persistence, cached image replacement, and
timer lifecycle.

## Character Manifest

Each character has declarative metadata instead of scattered special cases:

- source kind and source-frame count;
- crop rectangles or procedural renderer identifier;
- horizontal mirror flag;
- target baseline and optical center;
- target width and height range;
- alpha threshold and optional edge cleanup strength;
- permitted centroid drift;
- contact and flight frame indices;
- load-band motion scaling limits.

Manifest validation fails tests when values are missing, out of canvas bounds,
or incompatible with the eight-frame output contract.

## Frame Pipeline

1. Decode or draw source frames at their natural geometry.
2. Apply the manifest's mirror flag so the character faces right.
3. Remove source matte colors while preserving useful antialiased alpha.
4. Resample source cycles to eight ordered poses without duplicating adjacent
   output frames.
5. Determine the cycle's union bounds and stable optical center.
6. Scale within character-specific width and height limits.
7. Align contact poses to the common baseline and bound non-contact vertical
   movement so jumps remain intentional.
8. Place frames on the 48×32 canvas with bounded centroid drift.
9. Convert every non-transparent pixel to white while preserving alpha.
10. Build four load-band variants by adjusting motion envelope only; identity,
    direction, scale, and baseline remain stable.

Normalization is deterministic so menu-bar images, previews, tests, and QA
montages always agree.

## Animation and CPU Response

The existing behavior remains the foundation:

- sampled CPU load uses fast attack and slower decay;
- motion cadence eases toward the sampled load;
- band changes use hysteresis;
- animation phase keeps its remainder when speed changes;
- contact frames are slightly longer than flight frames at low load;
- Reduce Motion uses a representative still frame.

The refactor must not allocate, decode PNG data, or rasterize frames in the
native timer callback. A character change may rebuild the selected cache, but
normal animation only selects the next cached image.

## Settings Experience

The character selector remains compact. It uses the production `RunnerPack`
frames, not a separate preview drawing path. Selecting a character immediately
updates both the preview and native menu-bar item without restarting PeterFan.

The picker continues to show one character per row and one animated selected
preview. Background rows remain still to avoid unnecessary WebView work.

## Error Handling

- Invalid embedded asset data fails during development and tests with the
  character name and failing pipeline stage.
- Runtime construction failure falls back to the known-good cat pack and logs a
  bounded diagnostic message; the menu-bar item must remain usable.
- A failed native image conversion keeps the previous image cache rather than
  leaving an empty status item.
- Missing preview data displays the character label without starting a retry
  loop.
- Reduce Motion and app visibility changes stop preview timers deterministically.

## Quality Gates

Automated checks cover every character and every output frame:

- exactly four load bands and eight frames per band;
- non-empty, distinct poses;
- right-facing manifest configuration;
- no clipping at canvas edges;
- bounded width, height, occupancy, centroid drift, and baseline variance;
- white-only RGB with preserved alpha edges;
- meaningful contact-versus-flight pixel differences;
- identical menu-bar and preview frame hashes;
- monotonic CPU-to-speed mapping and smooth attack/decay;
- bounded cached image count and no per-tick rasterization path.

A deterministic sprite-sheet montage at native size and 4× scale is generated
for local visual QA. A small report lists each character's bounds, occupancy,
centroid drift, baseline variance, and frame-difference score so weak characters
are visible without subjective guesswork.

## Approved Manifest Target Tuning Amendment

Task 6 measurements approved eight final procedural manifest deviations from
the planning table: Penguin 26×28→40×28, Robot 30×27→38×27, Bird
38×20→42×26, Duck 38×20→40×22, Fish 40×18→40×21, Slime 34×18→36×18,
Sausage 42×18→42×23, and Party 40×24→40×26. The rendered 48×32
visible-alpha bounds showed that the narrower targets made the affected
silhouettes lose recognizable limbs, wings, head/body separation, or their
defining motion cue at the native 27×18 display footprint. In particular, Duck
needs 40×22 to retain its beak, body, and leg stride; Fish needs 40×21 to keep
the tail beat distinct from the body; and Slime needs 36×18 to preserve its
squash/stretch outline instead of reading as a flat bar. The tuning remains
bounded before normalization: identity dimensions stay fixed per cycle, only
the documented gait cue moves, contact frames retain their baseline, and
centroid drift remains at or below each manifest limit. The generated QA report
is the approval record: it includes per-band bounds, visible occupancy range,
contact-to-flight delta, and per-pose visible cross-band deltas for every
character.

## Verification and Release

Completion requires:

1. Focused runner tests pass.
2. `cargo test --workspace` passes.
3. `cargo clippy --workspace --all-targets -- -D warnings` passes.
4. Formatting, documentation checks, and `git diff --check` pass.
5. Release workspace builds at v1.31.32.
6. `dist/PeterFan.app` is rebuilt.
7. The previous local app process exits and v1.31.32 launches.
8. The menu-bar log records startup, popover WebView creation, and readiness.
9. Signing status is reported honestly; lack of Developer ID identity may allow
   a local ad-hoc build but blocks claiming a distributable production release.

## Rollout Order

1. Extract the shared runner module without changing rendered output.
2. Add measurements and failing quality gates for current inconsistencies.
3. Normalize asset-backed runners.
4. Normalize procedural runners.
5. Unify picker and native frame sources.
6. Tune timing only where the shared measurements expose visible stutter.
7. Generate QA artifacts, run the complete release gate, bump the version, and
   update the local app.
