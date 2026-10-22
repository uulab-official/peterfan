# Runner Quality and Full-Body Push-up Animation Design

## Goal

Make the runner selector and menubar animation visually intentional at the existing 48×32 monochrome size, with a real full-body push-up cycle instead of a shoulder-only deformation.

## Context

The current runner pack mixes CC0 image sheets, unrelated frame counts, and procedural shapes. It normalizes them into eight frames and sometimes interpolates alpha at the same pixel coordinates. That produces ghosting and inconsistent anatomy. The current `Pushup` renderer changes shoulder and elbow coordinates while leaving the hand, foot, and much of the body structure fixed, so it reads as a moving upper body rather than a complete exercise.

OpenGameArt's CC0 `Runner Character` provides a useful reference for an eight-frame, 32×32 side-view run cycle. Kenney's CC0 character packs provide a consistent licensing and silhouette reference. These are references and source candidates; every imported file must retain its license note in `assets/runner/README.md`.

## Design

### Frame and asset policy

- Keep the runtime canvas at 48×32 and the existing eight-frame cycle contract.
- Use nearest-neighbor resampling for pixel art. Do not alpha-interpolate between poses whose geometry changes.
- Preserve one side-view direction: all characters face and move left-to-right.
- Keep the final runtime output white with alpha; retain source color only in checked-in source assets and QA previews.
- Prefer complete, consistently authored CC0 frame packs. Do not combine frames from unrelated art styles in one character.
- Keep every historical character value parseable for configuration compatibility, but expose a curated `FEATURED` roster in the product UI: Cat, Dog, Rabbit, Fox, Penguin, Dinosaur, Robot, Ghost, Human, and Push-up. Legacy novelty icons remain renderable for migration/debugging but cannot leak into the picker or preview payload.
- Use the OpenGameArt CC0 Runner Character sheet as the active Human reference/source so the picker includes a genuine eight-pose run cycle rather than a loosely related character sheet.

### Push-up cycle

`Pushup` is a stationary side-view exercise, not a horizontal runner. Its eight frames are a seamless two-repetition cycle:

1. high plank, body straight;
2. controlled descent begins, elbows bend;
3. lower descent, head and shoulders follow the torso;
4. bottom position, chest low and elbows fully bent;
5. bottom recovery begins;
6. controlled rise, hips and knees follow the rigid body line;
7. near plank;
8. high plank hold before looping.

Hands and feet remain the primary ground contacts. The shoulder, elbow, hip, knee, and head landmarks move together. The torso remains a connected silhouette and never disappears between poses. The frame generator uses easing between authored pose values, but it renders each final frame as a crisp raster silhouette without cross-frame pixel blending.

### Quality gates

For every character and CPU band, QA must verify:

- all eight frames are non-empty and inside the 48×32 canvas;
- the first and last frames form a seamless loop;
- contact points that should be fixed remain fixed within one pixel;
- the silhouette area and bounding box do not jump unexpectedly;
- at least two distinct limb/body regions change for a full-body motion;
- the push-up torso, hands, and feet remain connected in every frame;
- no frame is accepted merely because unrelated pixels changed;
- generated sprite sheets are reviewed at native size and nearest-neighbor enlarged size.

## Scope

This change covers the runner frame pipeline, the push-up renderer, the runner quality assertions, the asset license notes, and the generated QA sheet. It does not change CPU telemetry, menubar layout, or update delivery.

## Acceptance criteria

- The push-up animation visibly moves as a complete body with stable hand and foot contacts.
- No push-up frame is a shoulder-only change.
- The runner sprite sheet shows consistent left-to-right silhouettes with no interpolation ghosting.
- The picker and QA sheet contain only the curated production roster; legacy values continue to parse and fall back to Cat at startup.
- Existing Rust workspace tests, format, clippy, runner QA, and smoke checks remain green.
- The selected CC0/reference sources and their licenses are documented.
