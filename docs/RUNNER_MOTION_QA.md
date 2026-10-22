# Runner motion verification — 2026-10-22

The latest local v1.32.1 checkout is `.worktrees/runcat-quality-v1.31.32`.
The root checkout and unrelated pending release/process-control changes were preserved.

## Behavior

- All 20 persisted character IDs are now selectable. Bird, Duck, Sheep, Fish and Slime have newly authored original cycles in `playful_animals.rs`; Orbit, Sushi, City, Sausage and Party in `playful_objects.rs`. The other ten cycles retain their reviewed artwork, including the linked-joint push-up.
- Each new character has eight distinct poses in all four load bands. Bird flaps, Duck waddles, Sheep bounds, Fish articulates its tail, Slime compresses and hops, Sushi and Sausage jog, Party is a single dancer, Orbit moves two moons, and City keeps a stationary skyline with traffic/windows.
- All 20 characters now share the same CPU clock and uniform pose intervals. Per-character action caps and contact-pose timing weights were removed after visual feedback about uneven speeds.

- Cat, Dog, Rabbit, Fox, Penguin, Robot and Ghost use original eight-pose silhouettes with stable bodies and articulated motion. Human and Dinosaur retain their existing source cycles. Push-up now uses a rigid linked-joint model with fixed hand/toe contacts, a straight spine, constant arm lengths, full extension and a ground-facing elbow.
- CPU-only samples run every 250 ms while the runner animates. Expensive system families remain on their existing dashboard schedule. Rising samples immediately update the target; visible motion eases over a 75 ms response constant. Falling samples and motion ease more slowly.
- All pose intervals range from 180 ms at zero load to 20 ms at full load. At 5/45/95% load, all characters use 167/81/21 ms per pose. All eight-pose cycles therefore have equal duration at equal load; different actions can still have a different visual rhythm.
- Push-up, Ghost and Party share the same timing as Cat and the rest of the roster in both native and browser playback. At full load an eight-pose cycle lasts 160 ms, versus the earlier 880 ms Push-up / 720 ms Party limits.
- Native callbacks consume measured `Instant` time and retain phase remainder. Updating CPU brings distant deadlines forward without postponing an imminent frame. Pauses over one second do not replay a backlog.
- The Settings preview shares the selected canonical image bytes, caches decoded ImageData, and keeps one timer. Hidden documents, non-Settings panels and Reduce Motion stop preview work; returning resumes it.

## Verification

- `cargo +stable test --workspace`: **403 passed, 0 failed, 6 ignored**.
- `cargo +stable clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo +stable fmt --all -- --check`, `git diff --check`, `bash scripts/check-docs.sh`: passed.
- `bash scripts/check-runner-quality.sh`: passed; executes the normally ignored artifact generator separately. All shared clipping, contact/flight, region, drift and loop quality limits remain active.
- `node scripts/check-runner-preview.mjs`: cache reuse, cadence, acceleration, visibility, Reduce Motion and one-timer lifecycle passed against the actual shipped preview functions. Native/preview cadence agrees within 1 ms for all 20 characters × 101 CPU values × 8 poses, using the fresh Rust-generated `runner-motion-cadence.json`.
- `cargo +stable build --release --workspace`: passed.
- `bash scripts/smoke-test.sh target/release`: **42 checks, 0 failures**, including mock lifecycle, embedded JavaScript parsing, signed bundle and DMG layout.
- Independent read-only review of the expansion: no actionable findings; new renderer tests, timing tests and preview parity checks passed. Earlier Ghost direction, carried animation phase and Robot antenna/visor fixes remain intact.
- Browser comparison inspected with all twenty featured characters at fixed 5/45/95% loads in dark and light backgrounds. These are controlled comparison values, not live CPU readings.

## Local installation

- Installed: `/Applications/PeterFan.app` (local ad-hoc signed v1.32.1).
- Binary SHA-256: `fa9756ae8a4522701d564f2ede263ec041bfe478ad67b97ad109a7d0f2183789`.
- Previous application preserved at `/Users/uulab/Documents/workspace/uulab/peterfan/.worktrees/runcat-quality-v1.31.32/dist/unified-cadence-backups/PeterFan-before-20261022T100657Z.app`.
- Fresh readback found exactly one installed app process and successful tray creation / WebView readiness.
- Provenance and full checks: `target/unified-cadence-install.json`, `target/unified-cadence-workspace.log`, `target/unified-cadence-clippy.log`, `target/unified-cadence-smoke.log`, `target/unified-cadence-launch.log`.
- Signing changes the Mach-O signature blob. All 18 Mach-O section metadata/content entries match the raw release build; the installed signed-binary hash and source-file hashes were freshly read back.
- No public release or notarization was performed. Global Codex settings were not modified.

## Evidence limits

Native app Computer Use access timed out, including after replacement, so a direct visual inspection of the live AppKit status item/popover is **unverified**. Sustained-load AppKit cadence and subjective equality with RunCat are **not established** by these tests. The generated frames and deterministic timing behavior were verified separately from native launch evidence.

The read-only `doctor` output reports real metrics and thermal monitoring ready; fan-control readiness is `needs_setup` because the installed daemon was not reachable. Fan setup/daemon administration was not part of this animation change.

## Review decisions

The push-up's former independent point curves flattened the body and let the arm pass through its back. New tests enforce fixed ground contacts, collinear spine joints, fixed segment lengths, almost full arm extension at the top, and elbow/forearm below the body axis in every pose/load. An independent dense sweep of 8,008 pose/load combinations verified feasible IK and at least 1.513 pixels of elbow/forearm clearance. The push-up regional motion metric now splits along the horizontal body axis (rear-chain versus shoulder/arm), retaining the existing minimum difference threshold. This avoids a screen Y split losing both head positions when the body properly descends. The raised plank now requires at least 30px width and 20px height in its representative pose, within its 40x25 cycle manifest.


The ten new characters keep all shared occupancy (150–520), visible frame difference (24+), contact/flight, clipping, center drift and cross-band gates unchanged. Representative-frame size checks now use anatomy-specific floors for an upright dancer, height-limited skyline and compressing slime; entire cycles retain the original manifest bounds. Reviewed direction signatures were refreshed only for the ten re-authored characters. All ten previously featured artwork signatures remain unchanged. The speed-unification follow-up changes timing only; no artwork signatures changed.

The former horizontal-width assertion counted decorative speed lines and rejected upright anatomy; Penguin/Robot now have upright height/width checks, while the complete cycle still passes unchanged manifest bounds. Rabbit ears and optical height were adjusted until the representative pose is at least 30 pixels wide across all four CPU bands. Fox's old 300-pixel whole-frame difference encouraged torso deformation; a 120-pixel floor now accompanies its unchanged regional/contact/flight/loop tests. These changes preserve meaningful motion while retaining body volume.

## Reproduce visual comparison

```sh
bash scripts/check-runner-quality.sh
node scripts/render-runner-motion-qa.mjs
node scripts/check-runner-preview.mjs
python3 -m http.server 8769 --bind 127.0.0.1 --directory target
```

Open `http://127.0.0.1:8769/runner-motion-qa.html`. Latest speed-comparison screenshot: `target/unified-cadence-busy.png`. Previous catalog browser screenshots are `target/playful-catalog-dark.png`, `target/playful-catalog-light.png` and `target/playful-catalog-busy.png`. The first three rows feature Push-up, Party and Ghost. The comparison uses fixed loads, not live CPU measurements.

Follow-up visual proof: `target/pushup-motion-dark.png` and `target/pushup-motion-light.png`; the comparison page now includes all eight exercise poses and lists Push-up first. Earlier local application backups and the original `target/runner-polish-install.json` are retained.
