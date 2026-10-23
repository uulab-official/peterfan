# Runner quality targets

This pass targets a low-cost, readable and coherent menu-bar companion. It does
not claim superiority to RunCat without a same-machine comparison. Existing CPU
cadence, random selection, settings preservation and verified update installation
must survive the changes.

## Acceptance criteria

| Area | Target | Evidence required |
| --- | --- | --- |
| Native rendering cost | At least 50% lower app CPU than the preceding renderer under the same workload | Release builds, same art/backend/display, ABBA samples, CPU seconds and executable hashes |
| Low-load residency | Average app CPU at or below 3% of one core | Separate live low-load measurements; synthetic mock samples cannot establish this |
| Tiny artwork | All 20 characters remain recognizable at 27×18 points on dark/light backgrounds | Animated review using the canonical frames, plus all four load bands in the geometry report |
| Push-up | Planted hands/toes, rigid shallow plank, connected joints, controlled chest stroke | Skeleton/contact tests and tiny animated review |
| Ghost | Connected cloth hem and attached arms; two readable eye cavities after downscale | Tiny-scale cavity/connectivity checks and animated review |
| Party | Visible asymmetric weight transfer, planted supporting foot, continuous loop | Support-foot/limb/seam tests and tiny animated review |
| CPU response | Retain the shared 20–180 ms frame curve and all eight poses | Native/frontend cadence comparison, response and Reduce Motion regressions |
| Recovery and delivery | Preserve config, sign/notarize the local candidate, observe one healthy native app | Bundle identity/hash/trust checks, backup and installed runtime readback |

Targets are not waived by passing unrelated tests. A missed target is reported
as missed. An ABBA comparison records host load but does not control all desktop
activity, prove energy use or establish RunCat parity.

## Method

The accepted native performance probe changes one variable: NSStatusItem image
replacement versus a color layer with an animated alpha mask. Source pixels,
logical image size, artwork, bitmap storage and timing remain matched. The mock backend
avoids live fan/daemon/network operations and replays the same CPU pattern. The
benchmark only terminates its own child processes and verifies the user's config
and measured executable hashes remain unchanged.

Run manually on macOS, with builds and other heavy work paused:

```sh
python3 scripts/benchmark-runner-macos.py \
  --baseline /absolute/path/to/image-renderer/peterfan-menubar \
  --candidate /absolute/path/to/layer-renderer/peterfan-menubar \
  --duration 30 --warmup 5 \
  --output target/runner-comparison/results.json
```

The output must be a fresh path. The app's existing mock-instance lock refuses a
second mock instance; the harness aborts instead of stopping an existing one.
This command launches temporary native mock apps; it is intentionally excluded
from the contributor gate, which does not launch user interfaces.

For a private before/after motion study without overwriting another QA page:

```sh
node scripts/render-runner-motion-qa.mjs \
  --before target/quality-goal/before-frames.json \
  --output target/quality-goal/motion.html \
  --before-label 'Installed baseline' --after-label 'Candidate'
```

Regenerate canonical native frame artifacts before evaluating the candidate.
Synthetic 5/45/95% previews demonstrate the authored cadence, not live CPU load.

## Renderer and artwork changes

On one screen, animated Graph/Both styles retain the native button for layout,
title, tooltip, accessibility and clicks, while an owned CALayer mask displays
the cached frames. Native cell geometry, backing scale and effective appearance
determine placement and tint. Empty geometry fails back to normal image drawing.
Number-only, Reduce Motion and multiple-screen configurations retain the image
renderer; this preserves animation on every menu bar without claiming a measured
multi-monitor performance improvement. Rebinding/hotplug does not reset motion
phase or its CPU response.
Changing Reduce Motion resets the native timer and repaints the neutral pose,
instead of freezing an exercise midway through a repetition.

The push-up now uses a shallow rigid plank with fixed hands/toes and a two-bone
arm solve. The ghost has a connected two-fold cloth hem and readable eye cavities
after downscale. The party dancer shifts weight over a planted support foot with
an asymmetric step. All three retain eight poses and the shared CPU cadence.
The other seventeen artwork signatures are unchanged.
The obsolete push-up top-pose height minimum for a steep elevated plank is
replaced by a bounded shallow proportion check, alongside the independent
contact, angle, stroke and fixed-length joint tests. Shared cross-band,
occupancy, centroid and frame-change thresholds remain unchanged.

The AppKit snapshot bottleneck and layer-rendering direction are also described
in the primary [RunCatNeo runner view](https://github.com/runcat-dev/RunCatNeo/blob/main/LocalPackage/Sources/UserInterface/Views/RunnerBar/RunnerBarView.swift)
and [layer implementation](https://github.com/runcat-dev/RunCatNeo/blob/main/LocalPackage/Sources/UserInterface/Views/RunnerBar/RunnerLayer.swift).
PeterFan's Rust implementation uses public AppKit/Core Animation APIs and its own
frame clock; no RunCat benchmark result is treated as a PeterFan measurement.

## Recorded measurements — 2026-10-23

macOS 27.0.1, one screen, release builds, Cat/Both, four ABBA samples of 30 seconds
after five seconds of warmup. The installed normal app remained running in both
conditions; builds and the QA animation page were stopped during measurement.

| Matched experiment | Baseline CPU | Candidate CPU | Result |
| --- | --- | --- | --- |
| PNG versus eager bitmap only | 5.9485% | 5.9315% | 0.29% reduction, no accepted performance benefit |
| Image replacement versus layer mask | 6.7815% | 0.7995% | 88.21% lower app CPU, above the 50% target |

CPU values are medians expressed as a percentage of one core. Layer comparison
binary SHA-256 values: baseline
`4bd9e6ab4e93f9a55ca6199b99f06cd33d0dc10580ae6ead2257e4810051c740`,
candidate `d6fc3886cfaf1d7b74e9e726349ea770c74a785e44e8d1ae3eadbbca2c7a0e72`.
Sixteen shared source/dependency hashes matched; the probe changed only the
layer eligibility policy. Raw local evidence is
`target/quality-goal/layer-abba.json` and `probe-layer-provenance.json`.
Later diagnostics, cleanup, Reduce Motion handling and ghost artwork changes
are verified separately. The measured Cat pixels and animated clock are
unchanged; the final installed candidate also has its own live measurement.

Global WindowServer CPU medians were 30.577% and 18.126%. These include unrelated
desktop work and varying host load, so they do not isolate compositor cost or
establish an energy saving. Synthetic mock results do not establish the separate
live low-load target or superiority over RunCat.

Native offscreen CALayer compositing tests verify nonempty upright pixels,
frame changes, alpha 255/128 and scoped Aqua/Dark Aqua tint. A deliberate blank
mask fails the visibility regression. Canonical-frame browser QA covers all
twenty characters in light/dark themes and all eight poses of the revised three.
Native menu-bar screenshot capture was unavailable; browser imagery and owned
offscreen rendering are not a physical display capture.

## Installed candidate

Product source `1f97687` was built for arm64, Developer ID signed, accepted by
Apple notarization (`0ed35b07-ec1c-4b83-846f-ccf33cf3a3fd`), stapled and Gatekeeper
accepted. The installed app/helper match the signed candidate; the prior app
and configuration are backed up. Configuration bytes are unchanged and one
normal native process runs. Runtime logs confirm nonempty 27×18 layer geometry,
WebView readiness and Random rotation. The automatic release check started
20.157 seconds after startup, succeeded and scheduled another check 21,600
seconds later. Public latest remains v1.32.2; this does not prove delivery of a
newer published update.

Three live 30-second samples with saved Random settings measured app CPU at
1.366%, 1.533% and 1.599% of one core (mean **1.4993%**). Host CPU samples averaged
29.3754% across all cores. Before measuring, low load was defined as host mean
CPU at or below 20%, so the separate low-load acceptance target remains
**unverified** despite the observed app CPU being below 3%. No other processes
or user settings were changed to manufacture an idle environment. Raw evidence:
`target/quality-goal/live-installed.json` and `runtime-verification.json`.

The full local contributor gate passed 463 Rust tests (7 intentionally ignored)
and 59 Node tests, with native/Windows-target Clippy, Rust 1.88 checks, generated
runner comparisons and documentation. All 43 release/mock/package smoke checks
passed. CLI doctor reports healthy metrics, no issues and thermal readiness;
privileged fan-control readiness remains `needs_setup` and is outside this
rendering pass. Energy, multi-monitor hardware behavior, extended soak and
same-machine RunCat comparison remain unverified.
