# RunCat-Class Runner Quality System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring all twenty PeterFan menu-bar runners to one measurable production-quality standard while preserving smooth CPU-driven animation and updating the local app to v1.31.32.

**Architecture:** Extract runner timing, source rendering, normalization, and quality measurement from `packages/menubar/src/main.rs` into a focused `runner` module. Build one deterministic `RunnerPack` per selected character and use its same cached 48×32 frames for native menu-bar images and Settings previews.

**Tech Stack:** Rust 1.80+, `png`, macOS AppKit/Objective-C bindings, `tray-icon`, embedded WebView JavaScript, Cargo tests and Clippy.

## Global Constraints

- Preserve all existing dirty-worktree changes; do not reset, overwrite, or include unrelated changes in a task commit.
- Before executing commit steps, isolate or checkpoint the current v1.31.31 working state so `main.rs` changes can be staged without capturing unrelated work.
- Keep all twenty existing `RunnerCharacter` variants and their persisted lowercase names.
- Do not copy or redistribute RunCat artwork.
- Output is always a white template glyph on a transparent 48×32 canvas, facing left to right.
- Every character provides four CPU load bands with eight distinct ordered frames per band.
- Normal animation only selects cached frames; it does not decode PNGs, normalize geometry, or procedurally rasterize inside the timer callback.
- CPU spikes accelerate quickly, falling load decelerates smoothly, and animation phase is never reset by a speed change.
- Menu-bar and Settings preview frames come from the same `RunnerPack` bytes.
- macOS Reduce Motion continues to select a still frame.
- Bump to v1.31.32 only after focused and workspace tests pass.
- Rebuild and relaunch `/Users/uulab/Documents/workspace/uulab/peterfan/dist/PeterFan.app` after the release build.

## File Structure

- Create `packages/menubar/src/runner/mod.rs`: public runner constants, `RunnerPack`, build/fallback API, and module integration tests.
- Create `packages/menubar/src/runner/timing.rs`: CPU smoothing, load bands, frame cadence, phase, and native scheduling helpers.
- Create `packages/menubar/src/runner/manifest.rs`: declarative metadata for all twenty characters.
- Create `packages/menubar/src/runner/quality.rs`: bounds, centroid, baseline, occupancy, frame-difference metrics, and validation errors.
- Create `packages/menubar/src/runner/normalize.rs`: deterministic mirroring, scaling, translation, baseline, and white-alpha normalization.
- Create `packages/menubar/src/runner/assets.rs`: embedded asset constants and decoding for cat, dog, rabbit, fox, dinosaur, ghost, and human.
- Create `packages/menubar/src/runner/procedural.rs`: penguin, robot, and novelty character source drawing.
- Create `packages/menubar/src/runner/draw.rs`: reusable pixel, line, disc, ellipse, and triangle primitives.
- Modify `packages/menubar/src/main.rs`: consume the runner module, cache packs/native images, expose preview bytes, and retain UI lifecycle only.
- Create `scripts/check-runner-quality.sh`: generate deterministic QA artifacts and verify their presence.
- Modify `CHANGELOG.md`, `Cargo.toml`, `Cargo.lock`, `README.ko.md`, and `docs/PRODUCTION_CHECKLIST.md`: release v1.31.32.

---

### Task 1: Extract CPU-Driven Timing Without Behavior Changes

**Files:**
- Create: `packages/menubar/src/runner/mod.rs`
- Create: `packages/menubar/src/runner/timing.rs`
- Modify: `packages/menubar/src/main.rs:85-176`
- Modify: `packages/menubar/src/main.rs:5741-5877`
- Test: `packages/menubar/src/runner/timing.rs`

**Interfaces:**
- Consumes: `std::time::Duration`, `peterfan_core::config::MenubarDisplay`.
- Produces: `FRAME_COUNT`, `MIN_INTERVAL`, `MAX_INTERVAL`, `NATIVE_TICK`, `smooth_cpu`, `frame_interval`, `frame_interval_for_pose`, `load_band`, `load_band_with_hysteresis`, `phase_step`, `ease_motion_cpu`, `should_animate`, `uses_event_loop_timer`, and macOS-only `native_next_delay`.

- [ ] **Step 1: Write module-level timing tests before moving implementation**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_load_maps_monotonically_to_runner_speed() {
        let idle = frame_interval(5.0);
        let normal = frame_interval(45.0);
        let busy = frame_interval(95.0);
        assert!(idle > normal && normal > busy);
        assert!((418..=422).contains(&idle.as_millis()));
        assert!((128..=132).contains(&normal.as_millis()));
        assert!((59..=61).contains(&busy.as_millis()));
    }

    #[test]
    fn motion_attack_is_faster_than_decay_without_overshoot() {
        let tick = Duration::from_millis(30);
        let attack = ease_motion_cpu(10.0, 90.0, tick);
        let decay = ease_motion_cpu(90.0, 10.0, tick);
        assert!(attack > 10.0 && attack < 90.0);
        assert!(decay > 10.0 && decay < 90.0);
        assert!(attack - 10.0 > 90.0 - decay);
    }

    #[test]
    fn phase_keeps_remainder_when_cadence_changes() {
        let (phase, advanced) = phase_step(450_000, 30_000, 5.0, 0);
        assert!(advanced);
        let (phase, advanced) = phase_step(phase, 30_000, 95.0, 1);
        assert!(!advanced);
        let (phase, advanced) = phase_step(phase, 30_000, 95.0, 1);
        assert!(advanced);
        assert!(phase < frame_interval_for_pose(95.0, 1).as_micros() as u64);
    }
}
```

- [ ] **Step 2: Run the new focused tests and verify the missing module/API failure**

Run:

```bash
env RUSTC=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc PATH=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/usr/bin:/bin cargo test -p peterfan-menubar runner::timing::tests
```

Expected: FAIL because `runner::timing` and its functions do not exist.

- [ ] **Step 3: Move timing constants and helpers into `runner/timing.rs`**

```rust
use std::time::Duration;
use peterfan_core::config::MenubarDisplay;

pub(crate) const FRAME_COUNT: u8 = 8;
pub(crate) const MIN_INTERVAL: Duration = Duration::from_millis(60);
pub(crate) const MAX_INTERVAL: Duration = Duration::from_millis(480);
pub(crate) const NATIVE_TICK: Duration = MIN_INTERVAL;
pub(crate) const NATIVE_TOLERANCE: Duration = Duration::from_millis(8);

pub(crate) fn frame_interval(cpu_pct: f32) -> Duration {
    let load = cpu_pct.clamp(0.0, 100.0) / 100.0;
    let min_ms = MIN_INTERVAL.as_millis() as f32;
    let max_ms = MAX_INTERVAL.as_millis() as f32;
    Duration::from_millis((min_ms + (max_ms - min_ms) * (1.0 - load).powi(3)).round() as u64)
}
```

Move the remaining current helpers without changing constants, coefficients, hysteresis boundaries, or gait weights. Re-export them from `runner/mod.rs` and replace `main.rs` call sites with the re-exported names.

- [ ] **Step 4: Run focused timing and existing runner timing tests**

Run:

```bash
env RUSTC=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc PATH=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/usr/bin:/bin cargo test -p peterfan-menubar runner::timing::tests
env RUSTC=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc PATH=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/usr/bin:/bin cargo test -p peterfan-menubar runner_animation
```

Expected: PASS.

- [ ] **Step 5: Commit the isolated timing extraction**

```bash
git add packages/menubar/src/runner/mod.rs packages/menubar/src/runner/timing.rs packages/menubar/src/main.rs
git diff --cached --check
git commit -m "refactor: isolate runner timing engine"
```

### Task 2: Extract Source Renderers Behind One Raw-Frame API

**Files:**
- Create: `packages/menubar/src/runner/assets.rs`
- Create: `packages/menubar/src/runner/procedural.rs`
- Create: `packages/menubar/src/runner/draw.rs`
- Modify: `packages/menubar/src/runner/mod.rs`
- Modify: `packages/menubar/src/main.rs:92-176`
- Modify: `packages/menubar/src/main.rs:6153-8010`
- Test: `packages/menubar/src/runner/mod.rs`

**Interfaces:**
- Consumes: `peterfan_core::config::RunnerCharacter`, CPU percentage, and pose index.
- Produces: `pub(crate) type RgbaFrame = Vec<u8>` and `pub(crate) fn render_raw_frame(character: RunnerCharacter, cpu_pct: f32, frame: u8) -> Result<RgbaFrame, RunnerBuildError>`.

- [ ] **Step 1: Add a failing raw-frame contract test**

```rust
#[test]
fn every_character_renders_a_complete_raw_frame() {
    for character in RunnerCharacter::ALL {
        for frame in 0..FRAME_COUNT {
            let rgba = render_raw_frame(character, 55.0, frame).expect("raw runner frame");
            assert_eq!(rgba.len(), (PIXEL_WIDTH * PIXEL_HEIGHT * 4) as usize);
            assert!(rgba.chunks_exact(4).any(|pixel| pixel[3] > 0));
        }
    }
}
```

- [ ] **Step 2: Run the focused test and confirm the API is absent**

Run:

```bash
env RUSTC=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc PATH=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/usr/bin:/bin cargo test -p peterfan-menubar every_character_renders_a_complete_raw_frame
```

Expected: FAIL because `render_raw_frame` does not exist.

- [ ] **Step 3: Define the shared frame and error types**

```rust
pub(crate) const PIXEL_WIDTH: u32 = 48;
pub(crate) const PIXEL_HEIGHT: u32 = 32;
pub(crate) const LOAD_BANDS: usize = 4;
pub(crate) type RgbaFrame = Vec<u8>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RunnerBuildError {
    Decode { character: RunnerCharacter, detail: String },
    InvalidFrame { character: RunnerCharacter, detail: String },
}
```

Implement `Display` for `RunnerBuildError` so logs name both the character and failing stage.

- [ ] **Step 4: Move drawing primitives and renderers without visual changes**

Move `Pt`, `draw_rect`, `draw_disc`, `draw_ellipse`, `draw_rotated_ellipse`, `draw_triangle`, `draw_line`, and `draw_runner_leg` into `draw.rs`. Move embedded sheet constants, layouts, decoders, and seven asset-backed renderers into `assets.rs`. Move penguin, robot, and novelty renderers into `procedural.rs`.

```rust
pub(crate) fn render_raw_frame(
    character: RunnerCharacter,
    cpu_pct: f32,
    frame: u8,
) -> Result<RgbaFrame, RunnerBuildError> {
    match character {
        RunnerCharacter::Cat
        | RunnerCharacter::Dog
        | RunnerCharacter::Rabbit
        | RunnerCharacter::Fox
        | RunnerCharacter::Dinosaur
        | RunnerCharacter::Ghost
        | RunnerCharacter::Human => assets::render(character, cpu_pct, frame),
        _ => Ok(procedural::render(character, cpu_pct, frame)),
    }
}
```

Keep a temporary `make_runner_rgba` wrapper in `main.rs` that delegates to
`render_raw_frame`, then calls the existing `apply_white_runner_style` before
returning. This preserves all existing call sites and white-glyph tests during
extraction.

- [ ] **Step 5: Run all existing runner rendering tests**

Run:

```bash
env RUSTC=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc PATH=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/usr/bin:/bin cargo test -p peterfan-menubar runner
```

Expected: PASS with the same twenty characters, frame sizes, and existing distinct-pose checks.

- [ ] **Step 6: Commit renderer extraction**

```bash
git add packages/menubar/src/runner packages/menubar/src/main.rs
git diff --cached --check
git commit -m "refactor: isolate runner source renderers"
```

### Task 3: Add Declarative Character Manifests and Measurements

**Files:**
- Create: `packages/menubar/src/runner/manifest.rs`
- Create: `packages/menubar/src/runner/quality.rs`
- Modify: `packages/menubar/src/runner/mod.rs`
- Test: `packages/menubar/src/runner/manifest.rs`
- Test: `packages/menubar/src/runner/quality.rs`

**Interfaces:**
- Consumes: `RunnerCharacter` and a 48×32 `RgbaFrame`.
- Produces: `manifest(character) -> &'static RunnerManifest`, `measure_frame(&RgbaFrame) -> Result<FrameMetrics, RunnerBuildError>`, and `measure_cycle(&[RgbaFrame; 8]) -> Result<CycleMetrics, RunnerBuildError>`.

- [ ] **Step 1: Write failing manifest completeness and frame-measurement tests**

```rust
#[test]
fn every_character_has_a_right_facing_manifest() {
    for character in RunnerCharacter::ALL {
        let manifest = manifest(character);
        assert_eq!(manifest.character, character);
        assert_eq!(manifest.output_direction, Direction::Right);
        assert!((14..=44).contains(&manifest.target_width));
        assert!((18..=29).contains(&manifest.target_height));
        assert!(manifest.baseline_y < PIXEL_HEIGHT);
        assert!(manifest.max_centroid_drift <= 3.5);
    }
}

#[test]
fn frame_metrics_report_bounds_centroid_and_baseline() {
    let mut frame = vec![0; (PIXEL_WIDTH * PIXEL_HEIGHT * 4) as usize];
    for y in 10..20 {
        for x in 8..18 {
            frame[((y * PIXEL_WIDTH + x) * 4 + 3) as usize] = 255;
        }
    }
    let metrics = measure_frame(&frame).expect("metrics");
    assert_eq!(metrics.bounds, Bounds { min_x: 8, min_y: 10, max_x: 17, max_y: 19 });
    assert!((metrics.centroid_x - 12.5).abs() < 0.01);
    assert_eq!(metrics.baseline_y, 19);
    assert_eq!(metrics.opaque_pixels, 100);
}
```

- [ ] **Step 2: Run the tests and verify the types/functions are missing**

Run:

```bash
env RUSTC=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc PATH=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/usr/bin:/bin cargo test -p peterfan-menubar runner::manifest::tests
env RUSTC=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc PATH=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/usr/bin:/bin cargo test -p peterfan-menubar runner::quality::tests
```

Expected: both commands FAIL because the corresponding modules do not exist.

- [ ] **Step 3: Implement all twenty manifests**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Direction { Right }

#[derive(Debug, Clone, Copy)]
pub(crate) struct RunnerManifest {
    pub character: RunnerCharacter,
    pub output_direction: Direction,
    pub mirror_source: bool,
    pub target_width: u32,
    pub target_height: u32,
    pub baseline_y: u32,
    pub max_centroid_drift: f32,
    pub contact_frames: [u8; 2],
    pub flight_frames: [u8; 2],
}
```

Use these exact geometry targets:

| Character | Width×Height | Baseline | Max drift | Contact | Flight |
| --- | ---: | ---: | ---: | --- | --- |
| Cat | 42×22 | 28 | 2.5 | 0,4 | 2,6 |
| Dog | 42×22 | 28 | 2.5 | 0,4 | 2,6 |
| Rabbit | 40×22 | 28 | 2.5 | 0,4 | 2,6 |
| Fox | 44×20 | 28 | 2.5 | 0,4 | 2,6 |
| Penguin | 26×28 | 29 | 2.5 | 0,4 | 2,6 |
| Dinosaur | 42×27 | 29 | 3.0 | 0,4 | 2,6 |
| Robot | 30×27 | 29 | 2.5 | 0,4 | 2,6 |
| Ghost | 32×28 | 28 | 3.0 | 0,4 | 2,6 |
| Bird | 38×20 | 26 | 2.5 | 0,4 | 2,6 |
| Duck | 38×20 | 28 | 2.5 | 0,4 | 2,6 |
| Sheep | 40×22 | 28 | 2.5 | 0,4 | 2,6 |
| Fish | 40×18 | 25 | 2.5 | 0,4 | 2,6 |
| Slime | 34×18 | 28 | 2.5 | 0,4 | 2,6 |
| Human | 24×29 | 29 | 2.5 | 0,4 | 2,6 |
| Pushup | 40×18 | 28 | 2.5 | 0,4 | 2,6 |
| Orbit | 40×22 | 27 | 2.5 | 0,4 | 2,6 |
| Sushi | 40×20 | 28 | 2.5 | 0,4 | 2,6 |
| City | 42×22 | 28 | 2.5 | 0,4 | 2,6 |
| Sausage | 42×18 | 28 | 2.5 | 0,4 | 2,6 |
| Party | 40×24 | 28 | 3.0 | 0,4 | 2,6 |

Set `mirror_source = true` for Cat, Dog, Rabbit, and Fox because the current
asset montage faces left. Set it to `false` for the other sixteen characters;
their current source renderers already face right or have no directional head.
The output direction remains `Right` for every manifest.

- [ ] **Step 4: Implement deterministic quality measurements**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Bounds { pub min_x: u32, pub min_y: u32, pub max_x: u32, pub max_y: u32 }

#[derive(Debug, Clone, Copy)]
pub(crate) struct FrameMetrics {
    pub bounds: Bounds,
    pub centroid_x: f32,
    pub baseline_y: u32,
    pub opaque_pixels: usize,
    pub clipped: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct CycleMetrics {
    pub frames: [FrameMetrics; 8],
    pub max_centroid_drift: f32,
    pub baseline_variance: u32,
    pub minimum_frame_delta: usize,
}
```

Treat alpha greater than 96 as visible for bounds and occupancy. Compute frame delta as the count of pixels whose alpha differs by more than 48. Return `InvalidFrame` for wrong buffer length or an empty frame.

- [ ] **Step 5: Run manifest and quality tests**

Run:

```bash
env RUSTC=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc PATH=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/usr/bin:/bin cargo test -p peterfan-menubar runner::manifest::tests
env RUSTC=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc PATH=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/usr/bin:/bin cargo test -p peterfan-menubar runner::quality::tests
```

Expected: PASS.

- [ ] **Step 6: Commit manifests and measurement API**

```bash
git add packages/menubar/src/runner
git diff --cached --check
git commit -m "feat: define runner quality manifests"
```

### Task 4: Implement the Deterministic Normalization Pipeline

**Files:**
- Create: `packages/menubar/src/runner/normalize.rs`
- Modify: `packages/menubar/src/runner/mod.rs`
- Modify: `packages/menubar/src/runner/quality.rs`
- Test: `packages/menubar/src/runner/normalize.rs`

**Interfaces:**
- Consumes: `&RunnerManifest` and `[RgbaFrame; 8]` raw source cycle.
- Produces: `normalize_cycle(&RunnerManifest, [RgbaFrame; 8]) -> Result<[RgbaFrame; 8], RunnerBuildError>` and `validate_cycle(&RunnerManifest, &[RgbaFrame; 8]) -> Result<CycleMetrics, RunnerBuildError>`.

- [ ] **Step 1: Write failing mirror, scale, baseline, and white-style tests**

```rust
fn alpha_at(frame: &[u8], x: u32, y: u32) -> u8 {
    frame[((y * PIXEL_WIDTH + x) * 4 + 3) as usize]
}

fn set_alpha(frame: &mut [u8], x: u32, y: u32, alpha: u8) {
    let offset = ((y * PIXEL_WIDTH + x) * 4) as usize;
    frame[offset..offset + 4].copy_from_slice(&[255, 255, 255, alpha]);
}

fn synthetic_pose(left: u32, top: u32, width: u32, height: u32) -> RgbaFrame {
    let mut frame = vec![0; (PIXEL_WIDTH * PIXEL_HEIGHT * 4) as usize];
    for y in top..top + height {
        for x in left..left + width { set_alpha(&mut frame, x, y, 255); }
    }
    frame
}

fn asymmetric_arrow_frame() -> RgbaFrame {
    let mut frame = synthetic_pose(8, 12, 14, 5);
    for offset in 0..5 { set_alpha(&mut frame, 22 + offset, 10 + offset, 255); }
    frame
}

fn test_manifest(width: u32, height: u32, baseline_y: u32, mirror_source: bool) -> RunnerManifest {
    RunnerManifest {
        character: RunnerCharacter::Cat,
        output_direction: Direction::Right,
        mirror_source,
        target_width: width,
        target_height: height,
        baseline_y,
        max_centroid_drift: 2.5,
        contact_frames: [0, 4],
        flight_frames: [2, 6],
    }
}

#[test]
fn normalization_places_white_frames_on_the_manifest_baseline() {
    let manifest = test_manifest(20, 12, 27, false);
    let source = std::array::from_fn(|pose| synthetic_pose(4 + pose as u32 % 2, 6, 10, 8));
    let normalized = normalize_cycle(&manifest, source).expect("normalized cycle");
    let metrics = measure_cycle(&normalized).expect("measured cycle");
    assert!(metrics.frames.iter().all(|frame| frame.baseline_y <= manifest.baseline_y));
    assert!(metrics.max_centroid_drift <= manifest.max_centroid_drift);
    for pixel in normalized.iter().flat_map(|frame| frame.chunks_exact(4)) {
        if pixel[3] != 0 { assert_eq!(&pixel[..3], &[255, 255, 255]); }
    }
}

#[test]
fn mirror_source_reverses_horizontal_alpha_coverage() {
    let original = asymmetric_arrow_frame();
    let mirrored = mirror_alpha(&original).expect("mirrored frame");
    for y in 0..PIXEL_HEIGHT {
        for x in 0..PIXEL_WIDTH {
            let a = alpha_at(&original, x, y);
            let b = alpha_at(&mirrored, PIXEL_WIDTH - 1 - x, y);
            assert_eq!(a, b);
        }
    }
}

#[test]
fn validation_rejects_a_duplicate_cycle() {
    let manifest = test_manifest(20, 12, 27, false);
    let frame = synthetic_pose(8, 8, 10, 8);
    let cycle = std::array::from_fn(|_| frame.clone());
    let error = validate_cycle(&manifest, &cycle).expect_err("duplicate cycle must fail");
    assert!(error.to_string().contains("duplicate"));
}
```

- [ ] **Step 2: Run normalization tests and observe missing API failures**

Run:

```bash
env RUSTC=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc PATH=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/usr/bin:/bin cargo test -p peterfan-menubar runner::normalize::tests
```

Expected: FAIL because normalization helpers do not exist.

- [ ] **Step 3: Implement geometry transforms using alpha-only bilinear resampling**

```rust
pub(crate) fn normalize_cycle(
    manifest: &RunnerManifest,
    source: [RgbaFrame; 8],
) -> Result<[RgbaFrame; 8], RunnerBuildError> {
    let source = source
        .into_iter()
        .map(|frame| if manifest.mirror_source { mirror_alpha(&frame) } else { Ok(frame) })
        .collect::<Result<Vec<_>, _>>()?;
    let union = union_bounds(&source)?;
    let scale = (manifest.target_width as f32 / union.width() as f32)
        .min(manifest.target_height as f32 / union.height() as f32);
    let center_x = (PIXEL_WIDTH as f32 - 1.0) * 0.5;
    let normalized = source
        .iter()
        .enumerate()
        .map(|(pose, frame)| resample_place_white(
            frame,
            scale,
            center_x,
            manifest.baseline_y,
            manifest.contact_frames.contains(&(pose as u8)),
        ))
        .collect::<Result<Vec<_>, _>>()?;
    normalized.try_into().map_err(|frames: Vec<RgbaFrame>| RunnerBuildError::InvalidFrame {
        character: manifest.character,
        detail: format!("expected 8 normalized frames, got {}", frames.len()),
    })
}
```

Preserve source alpha, bilinearly interpolate alpha only, write RGB as 255 for every nonzero output alpha, and reject any transformed frame that would clip visible alpha at a canvas edge.

- [ ] **Step 4: Implement cycle validation thresholds**

`validate_cycle` must reject:

- wrong frame count or buffer length;
- empty/clipped frame;
- cycle-union width or height above the manifest target, both dimensions below
  60% of their targets, or neither dimension within two pixels of its target;
- centroid drift above the manifest maximum;
- contact-frame baseline difference above one pixel;
- duplicate adjacent frames or minimum frame delta below 24 pixels;
- any non-white non-transparent pixel.

- [ ] **Step 5: Run normalization, manifest, and quality tests**

Run:

```bash
env RUSTC=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc PATH=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/usr/bin:/bin cargo test -p peterfan-menubar runner::normalize::tests
env RUSTC=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc PATH=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/usr/bin:/bin cargo test -p peterfan-menubar runner::quality::tests
```

Expected: PASS.

- [ ] **Step 6: Commit normalization**

```bash
git add packages/menubar/src/runner
git diff --cached --check
git commit -m "feat: normalize runner frame geometry"
```

### Task 5: Build and Validate Asset-Backed Runner Cycles

**Files:**
- Modify: `packages/menubar/src/runner/assets.rs`
- Modify: `packages/menubar/src/runner/mod.rs`
- Modify: `packages/menubar/src/runner/manifest.rs`
- Test: `packages/menubar/src/runner/assets.rs`

**Interfaces:**
- Consumes: raw embedded PNG sheets and `normalize_cycle`.
- Produces: `assets::source_cycle(character, cpu_pct) -> Result<[RgbaFrame; 8], RunnerBuildError>` and `assets::build_cycle(character, cpu_pct) -> Result<[RgbaFrame; 8], RunnerBuildError>` for Cat, Dog, Rabbit, Fox, Dinosaur, Ghost, and Human.

- [ ] **Step 1: Write failing per-character asset quality tests**

```rust
#[test]
fn asset_backed_characters_pass_the_shared_quality_gate() {
    let characters = [
        RunnerCharacter::Cat, RunnerCharacter::Dog, RunnerCharacter::Rabbit,
        RunnerCharacter::Fox, RunnerCharacter::Dinosaur,
        RunnerCharacter::Ghost, RunnerCharacter::Human,
    ];
    for character in characters {
        for cpu in [0.0, 30.0, 65.0, 90.0] {
            let cycle = assets::build_cycle(character, cpu).expect("asset cycle");
            validate_cycle(manifest(character), &cycle)
                .unwrap_or_else(|error| panic!("{character:?} at {cpu}%: {error}"));
        }
    }
}
```

- [ ] **Step 2: Run the test and record each failing character/metric**

Run:

```bash
env RUSTC=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc PATH=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/usr/bin:/bin cargo test -p peterfan-menubar asset_backed_characters_pass_the_shared_quality_gate -- --nocapture
```

Expected: FAIL until crop, mirror, and geometry metadata are correct.

- [ ] **Step 3: Route every asset source through one eight-frame resampler**

Decode each sheet once with `OnceLock<Result<[RgbaFrame; 8], RunnerBuildError>>`. Resample five- or six-frame loops by interpolating ordered phase positions; keep the dinosaur and human eight-frame order intact. Do not blend unrelated contact and flight poses into a featureless midpoint: choose the nearest source pose when the adjacent source-frame alpha delta exceeds 35% of visible pixels.

Retain these verified source crops before the shared geometry normalization:

| Character | Source crop |
| --- | --- |
| Cat | `(1, 4, 64, 58)` inside each 66×66 cell |
| Dog | `(0, 0, 60, 38)` from row 2 of six 60×38 cells |
| Rabbit | `(2, 1, 28, 24)` inside each 32×26 cell |
| Fox | `(1, 0, 62, 36)` inside each 64×36 cell |
| Ghost | `(1, 0, 30, 32)` inside each 32×32 cell |
| Dinosaur | `(20, 45, 640, 400)` inside each 680×472 frame |
| Human | full 32×32 cell, optically scaled to a 30-pixel source width |

```rust
pub(crate) fn build_cycle(
    character: RunnerCharacter,
    cpu_pct: f32,
) -> Result<[RgbaFrame; 8], RunnerBuildError> {
    let raw = source_cycle(character, cpu_pct)?;
    normalize_cycle(manifest(character), raw)
}
```

- [ ] **Step 4: Apply the fixed mirror and target geometry metadata**

Mirror Cat, Dog, Rabbit, and Fox; leave Dinosaur, Ghost, and Human unmirrored.
Use the Task 3 target geometry without adding per-frame offsets. If a source
fails, the error must identify crop bounds, union size, centroid drift, baseline,
or frame delta. Correct only that character's manifest or source crop; do not
loosen the global clipping, white-style, duplicate-frame, or baseline checks.

- [ ] **Step 5: Run asset and existing runner tests**

Run:

```bash
env RUSTC=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc PATH=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/usr/bin:/bin cargo test -p peterfan-menubar runner::assets::tests
env RUSTC=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc PATH=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/usr/bin:/bin cargo test -p peterfan-menubar runner_gait
```

Expected: PASS.

- [ ] **Step 6: Commit asset-backed quality work**

```bash
git add packages/menubar/src/runner/assets.rs packages/menubar/src/runner/manifest.rs packages/menubar/src/runner/mod.rs
git diff --cached --check
git commit -m "feat: standardize asset runner cycles"
```

### Task 6: Bring Procedural Runners Through the Same Gate

**Files:**
- Modify: `packages/menubar/src/runner/procedural.rs`
- Modify: `packages/menubar/src/runner/manifest.rs`
- Modify: `packages/menubar/src/runner/draw.rs`
- Test: `packages/menubar/src/runner/procedural.rs`

**Interfaces:**
- Consumes: procedural source renderer, manifests, and `normalize_cycle`.
- Produces: `procedural::source_cycle(character, cpu_pct) -> Result<[RgbaFrame; 8], RunnerBuildError>`, `procedural::build_cycle(character, cpu_pct) -> Result<[RgbaFrame; 8], RunnerBuildError>`, and module-level `build_normalized_cycle(character, cpu_pct) -> Result<[RgbaFrame; 8], RunnerBuildError>` for dispatching all twenty characters.

- [ ] **Step 1: Write the failing procedural quality matrix**

```rust
#[test]
fn procedural_characters_pass_the_shared_quality_gate() {
    let characters = [
        RunnerCharacter::Penguin, RunnerCharacter::Robot, RunnerCharacter::Bird,
        RunnerCharacter::Duck, RunnerCharacter::Sheep, RunnerCharacter::Fish,
        RunnerCharacter::Slime, RunnerCharacter::Pushup, RunnerCharacter::Orbit,
        RunnerCharacter::Sushi, RunnerCharacter::City,
        RunnerCharacter::Sausage, RunnerCharacter::Party,
    ];
    for character in characters {
        for cpu in [0.0, 30.0, 65.0, 90.0] {
            let cycle = procedural::build_cycle(character, cpu).expect("procedural cycle");
            validate_cycle(manifest(character), &cycle)
                .unwrap_or_else(|error| panic!("{character:?} at {cpu}%: {error}"));
        }
    }
}
```

- [ ] **Step 2: Run the quality matrix and keep the failure report**

Run:

```bash
env RUSTC=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc PATH=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/usr/bin:/bin cargo test -p peterfan-menubar procedural_characters_pass_the_shared_quality_gate -- --nocapture
```

Expected: FAIL for current inconsistent bounds, baseline, or frame-difference scores.

- [ ] **Step 3: Separate identity geometry from gait motion**

For each renderer, keep head/body proportions constant across all frames. Drive only limbs, tail, wings, bounce, orbit phase, or attached decorative elements from pose. Apply CPU load only to the motion envelope before normalization.

```rust
pub(crate) fn source_cycle(
    character: RunnerCharacter,
    cpu_pct: f32,
) -> Result<[RgbaFrame; 8], RunnerBuildError> {
    if matches!(
        character,
        RunnerCharacter::Cat
            | RunnerCharacter::Dog
            | RunnerCharacter::Rabbit
            | RunnerCharacter::Fox
            | RunnerCharacter::Dinosaur
            | RunnerCharacter::Ghost
            | RunnerCharacter::Human
    ) {
        return Err(RunnerBuildError::InvalidFrame {
            character,
            detail: "not a procedural runner".to_string(),
        });
    }
    Ok(std::array::from_fn(|pose| render_pose(character, cpu_pct, pose as u8)))
}

pub(crate) fn build_cycle(
    character: RunnerCharacter,
    cpu_pct: f32,
) -> Result<[RgbaFrame; 8], RunnerBuildError> {
    normalize_cycle(manifest(character), source_cycle(character, cpu_pct)?)
}
```

In `runner/mod.rs`, add `build_normalized_cycle` with an explicit match over the
seven asset-backed characters. Dispatch those to `assets::build_cycle` and the
other thirteen to `procedural::build_cycle`.

- [ ] **Step 4: Tune each procedural character without global threshold exceptions**

Keep thinner joint and facial cut-outs by using the existing `0.82` stroke scale as the default. Character-local manifest geometry may vary, but every output remains right-facing, white, unclipped, and recognizably horizontal where the concept is horizontal.

Use one primary motion cue per character so silhouettes remain readable at 27×18
display size: Penguin feet/wing swing, Robot opposing arm/leg swing, Bird wing
stroke, Duck leg stride, Sheep leg stride, Fish tail beat, Slime squash/stretch,
Pushup elbow/body height, Orbit satellite angle, Sushi belt offset, City window
pulse, Sausage leg stride, and Party dancer arm/leg swing. Keep body and head
bounds fixed within each cycle; only those cues and a maximum two-pixel vertical
bounce may vary.

- [ ] **Step 5: Run all twenty-character quality and rendering tests**

Run:

```bash
env RUSTC=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc PATH=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/usr/bin:/bin cargo test -p peterfan-menubar asset_backed_characters_pass_the_shared_quality_gate
env RUSTC=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc PATH=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/usr/bin:/bin cargo test -p peterfan-menubar procedural_characters_pass_the_shared_quality_gate
env RUSTC=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc PATH=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/usr/bin:/bin cargo test -p peterfan-menubar runner_characters_have_distinct_animated_silhouettes
```

Expected: PASS.

- [ ] **Step 6: Commit procedural quality work**

```bash
git add packages/menubar/src/runner/procedural.rs packages/menubar/src/runner/draw.rs packages/menubar/src/runner/manifest.rs
git diff --cached --check
git commit -m "feat: standardize procedural runner cycles"
```

### Task 7: Unify Native and Settings Frames in `RunnerPack`

**Files:**
- Modify: `packages/menubar/src/runner/mod.rs`
- Modify: `packages/menubar/src/main.rs:784-838`
- Modify: `packages/menubar/src/main.rs:5879-6158`
- Modify: `packages/menubar/src/main.rs:8012-8049`
- Test: `packages/menubar/src/runner/mod.rs`
- Test: `packages/menubar/src/main.rs`

**Interfaces:**
- Consumes: normalized cycles for four load bands.
- Produces: `RunnerPack::build`, `RunnerPack::character`, `RunnerPack::len`, `RunnerPack::frames`, `RunnerPack::frame`, `RunnerPack::frame_by_index`, `RunnerPack::preview_alpha_hex`, `alpha_hex`, `build_or_fallback`, `build_or_fallback_with`, and a fixed 32-frame cache.

- [ ] **Step 1: Write failing pack shape, preview parity, and fallback tests**

```rust
#[test]
fn pack_has_four_bands_and_preview_uses_identical_alpha() {
    for character in RunnerCharacter::ALL {
        let pack = RunnerPack::build(character).expect("runner pack");
        assert_eq!(pack.len(), LOAD_BANDS * FRAME_COUNT as usize);
        for band in 0..LOAD_BANDS {
            for pose in 0..FRAME_COUNT {
                let rgba = pack.frame_by_index(band, pose).expect("cached frame");
                assert_eq!(pack.preview_alpha_hex(band, pose), Some(alpha_hex(rgba)));
            }
        }
    }
}

#[test]
fn build_failure_falls_back_to_cat_without_emptying_the_cache() {
    let (pack, error) = build_or_fallback_with(RunnerCharacter::Fox, |character| {
        if character == RunnerCharacter::Fox {
            Err(RunnerBuildError::InvalidFrame {
                character,
                detail: "forced failure".to_string(),
            })
        } else {
            RunnerPack::build(character)
        }
    });
    assert_eq!(pack.character(), RunnerCharacter::Cat);
    assert!(error.is_some());
    assert_eq!(pack.len(), 32);
}
```

- [ ] **Step 2: Run focused pack tests and confirm missing APIs**

Run:

```bash
env RUSTC=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc PATH=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/usr/bin:/bin cargo test -p peterfan-menubar runner::tests::pack
```

Expected: FAIL.

- [ ] **Step 3: Implement the fixed pack API**

```rust
pub(crate) struct RunnerPack {
    character: RunnerCharacter,
    frames: Vec<RgbaFrame>,
}

impl RunnerPack {
    pub(crate) fn build(character: RunnerCharacter) -> Result<Self, RunnerBuildError> {
        let frames = [0.0, 30.0, 65.0, 90.0]
            .into_iter()
            .map(|cpu| build_normalized_cycle(character, cpu))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        if frames.len() != LOAD_BANDS * FRAME_COUNT as usize {
            return Err(RunnerBuildError::InvalidFrame {
                character,
                detail: format!("expected 32 cached frames, got {}", frames.len()),
            });
        }
        Ok(Self { character, frames })
    }

    pub(crate) fn frame(&self, cpu_pct: f32, pose: u8) -> &[u8] {
        &self.frames[timing::load_band(cpu_pct) * FRAME_COUNT as usize
            + usize::from(pose % FRAME_COUNT)]
    }

    pub(crate) fn character(&self) -> RunnerCharacter { self.character }
    pub(crate) fn len(&self) -> usize { self.frames.len() }
    pub(crate) fn frames(&self) -> &[RgbaFrame] { &self.frames }
    pub(crate) fn frame_by_index(&self, band: usize, pose: u8) -> Option<&[u8]> {
        (band < LOAD_BANDS && pose < FRAME_COUNT)
            .then(|| self.frames[band * FRAME_COUNT as usize + pose as usize].as_slice())
    }
    pub(crate) fn preview_alpha_hex(&self, band: usize, pose: u8) -> Option<String> {
        self.frame_by_index(band, pose).map(alpha_hex)
    }
}

pub(crate) fn build_or_fallback_with<F>(
    character: RunnerCharacter,
    builder: F,
) -> (RunnerPack, Option<RunnerBuildError>)
where
    F: Fn(RunnerCharacter) -> Result<RunnerPack, RunnerBuildError>,
{
    match builder(character) {
        Ok(pack) => (pack, None),
        Err(error) => (
            builder(RunnerCharacter::Cat).expect("embedded cat runner pack must validate"),
            Some(error),
        ),
    }
}
```

Implement `alpha_hex(rgba: &[u8]) -> String` by moving the current
`runner_alpha_hex` body. `build_or_fallback(character)` delegates to
`build_or_fallback_with(character, RunnerPack::build)`.

- [ ] **Step 4: Replace duplicate menu-bar and preview generation**

Store the selected `RunnerPack` on `App`. Build tray `Icon` and macOS `NSImage` caches directly from `pack.frames()`. Generate the Settings selected preview and option-row still frames from `RunnerPack` alpha. Remove the separate calls that independently invoke `make_runner_rgba` for native images and WebView preview JSON.

- [ ] **Step 5: Preserve the previous cache on failed character switch**

`set_runner_character` builds the new pack and native image cache before mutating `App`. On error, log one bounded message, keep the previous character/cache, and update status text. Startup may use `build_or_fallback` because there is no previous cache yet.

- [ ] **Step 6: Run pack, picker, native timer, and settings tests**

Run:

```bash
env RUSTC=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc PATH=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/usr/bin:/bin cargo test -p peterfan-menubar runner::tests
env RUSTC=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc PATH=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/usr/bin:/bin cargo test -p peterfan-menubar runner_picker
env RUSTC=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc PATH=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/usr/bin:/bin cargo test -p peterfan-menubar native_runner
```

Expected: PASS.

- [ ] **Step 7: Commit unified caching and preview path**

```bash
git add packages/menubar/src/runner packages/menubar/src/main.rs
git diff --cached --check
git commit -m "feat: share runner packs across native and settings UI"
```

### Task 8: Add Deterministic QA Artifacts and Animation Cost Gates

**Files:**
- Modify: `packages/menubar/src/runner/quality.rs`
- Modify: `packages/menubar/src/main.rs`
- Create: `scripts/check-runner-quality.sh`
- Test: `packages/menubar/src/runner/quality.rs`

**Interfaces:**
- Consumes: all `RunnerPack` frames and quality metrics.
- Produces: `target/runner-sprite-sheet.png`, `target/runner-quality-report.json`, and a script that fails when either artifact is absent or empty.

- [ ] **Step 1: Write a failing all-pack report test**

```rust
#[test]
fn quality_report_covers_every_character_and_load_band() {
    let report = build_quality_report().expect("quality report");
    assert_eq!(report.characters.len(), RunnerCharacter::ALL.len());
    assert!(report.characters.iter().all(|entry| entry.bands.len() == LOAD_BANDS));
    assert!(report.characters.iter().flat_map(|entry| &entry.bands).all(|band| {
        band.max_centroid_drift <= band.allowed_centroid_drift
            && band.minimum_frame_delta >= 24
            && !band.clipped
    }));
}
```

- [ ] **Step 2: Run the report test and observe the missing API failure**

Run:

```bash
env RUSTC=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc PATH=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/usr/bin:/bin cargo test -p peterfan-menubar quality_report_covers_every_character_and_load_band
```

Expected: FAIL.

- [ ] **Step 3: Implement serializable quality report records**

```rust
#[derive(serde::Serialize)]
pub(crate) struct RunnerBandReport {
    pub cpu_pct: f32,
    pub max_centroid_drift: f32,
    pub allowed_centroid_drift: f32,
    pub baseline_variance: u32,
    pub minimum_frame_delta: usize,
    pub clipped: bool,
}
```

Add a character record with the lowercase character name and four band reports. Sort by `RunnerCharacter::ALL` order for deterministic JSON.

- [ ] **Step 4: Upgrade the ignored visual-QA test**

Use `RunnerPack` frames to render native-size and 4× cells into `target/runner-sprite-sheet.png`. Write pretty JSON to `target/runner-quality-report.json`. Do not use screenshots from the user's display or include machine-specific paths in the report.

- [ ] **Step 5: Create the QA script**

```bash
#!/usr/bin/env bash
set -euo pipefail

task_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
rust_bin=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin
cd "$task_root"
env RUSTC="$rust_bin/rustc" PATH="$rust_bin:/usr/bin:/bin" \
  cargo test -p peterfan-menubar render_runner_sprite_sheet_for_visual_qa -- --ignored --nocapture
test -s "$task_root/target/runner-sprite-sheet.png"
test -s "$task_root/target/runner-quality-report.json"
```

Mark it executable and keep task-specific variable names rather than using `HOME` or `CODEX_HOME`.

- [ ] **Step 6: Prove normal animation does not rasterize**

Keep the native timer closure limited to `ease_motion_cpu`, `phase_step`, `load_band_with_hysteresis`, an index lookup, and `setImage`. Add a source regression assertion that the closure does not call `RunnerPack::build`, `render_raw_frame`, `normalize_cycle`, PNG decoding, or native image conversion.

- [ ] **Step 7: Run QA and runner tests**

Run:

```bash
./scripts/check-runner-quality.sh
env RUSTC=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc PATH=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/usr/bin:/bin cargo test -p peterfan-menubar runner
```

Expected: PASS and both target artifacts are non-empty.

- [ ] **Step 8: Commit QA gates**

```bash
git add packages/menubar/src/runner/quality.rs packages/menubar/src/main.rs scripts/check-runner-quality.sh
git diff --cached --check
git commit -m "test: gate runner animation quality"
```

### Task 9: Release v1.31.32 and Update the Local App

**Files:**
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `CHANGELOG.md`
- Modify: `README.ko.md`
- Modify: `docs/PRODUCTION_CHECKLIST.md`

**Interfaces:**
- Consumes: completed runner system and passing quality report.
- Produces: a verified v1.31.32 local `PeterFan.app` bundle running from `dist`.

- [ ] **Step 1: Bump release metadata to v1.31.32**

Set `[workspace.package].version = "1.31.32"`, update all six PeterFan workspace package entries in `Cargo.lock`, update README current/example versions, and add this changelog section:

```markdown
## [1.31.32] — RunCat급 러너 품질 시스템

### Changed

- 20종 러너를 공통 48×32 정규화 파이프라인과 품질 게이트로 통합했습니다.
- 메뉴바와 설정 미리보기가 동일한 캐시 프레임을 사용하도록 정리했습니다.
- 프레임 중심, 바닥선, 방향, 형태 인식도와 CPU 비용을 자동 검증합니다.
```

Add `[1.31.32]: https://github.com/uulab-official/peterfan/compare/v1.31.31...v1.31.32` to the changelog links.

- [ ] **Step 2: Format and run focused quality gates**

Run:

```bash
env RUSTC=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc PATH=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/usr/bin:/bin cargo fmt --all -- --check
./scripts/check-runner-quality.sh
./scripts/check-docs.sh
git diff --check
```

Expected: PASS.

- [ ] **Step 3: Run the complete workspace test and lint gates**

Run:

```bash
env RUSTC=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc PATH=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/usr/bin:/bin cargo test --workspace
env RUSTC=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc PATH=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/usr/bin:/bin cargo clippy --workspace --all-targets -- -D warnings
```

Expected: all non-ignored tests pass and Clippy reports no warnings.

- [ ] **Step 4: Build and bundle the release app**

Run:

```bash
env RUSTC=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc PATH=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin:/usr/bin:/bin cargo build --release --workspace
./scripts/bundle-macos.sh target/release/peterfan-menubar dist
/Users/uulab/Documents/workspace/uulab/peterfan/dist/PeterFan.app/Contents/MacOS/PeterFan --version
```

Expected: `peterfan-menubar 1.31.32`. Report whether the bundle has a Developer ID or only local/ad-hoc signing; do not claim public release readiness without Developer ID signing and notarization.

- [ ] **Step 5: Replace the running local app process**

Run:

```bash
local_app=/Users/uulab/Documents/workspace/uulab/peterfan/dist/PeterFan.app
local_pid=$(pgrep -f "^$local_app/Contents/MacOS/PeterFan" | head -n 1 || true)
if [[ -n "$local_pid" ]]; then
  kill "$local_pid"
  sleep 1
fi
open -g "$local_app"
sleep 3
pgrep -fl "^$local_app/Contents/MacOS/PeterFan"
tail -n 20 /Users/uulab/Library/Logs/PeterFan/menubar.log
```

Expected: one v1.31.32 process and log entries for startup, tray creation, popover WebView creation, and WebView readiness.

- [ ] **Step 6: Commit release metadata and final runner changes**

```bash
git add Cargo.toml Cargo.lock CHANGELOG.md README.ko.md docs/PRODUCTION_CHECKLIST.md packages/menubar/src/runner packages/menubar/src/main.rs scripts/check-runner-quality.sh
git diff --cached --check
git commit -m "feat: deliver RunCat-class runner quality"
```
