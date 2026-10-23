# Production polish audit — 2026-10-22

Scope: local arm64 v1.32.4 candidate, following the v1.32.3 runner and random
character changes. Public release and Homebrew remain v1.32.2. This is a bounded
quality pass, not evidence that every hardware or release gate is complete.

## Evidence and flows

The current dashboard HTML was rendered by the real Rust renderer and opened in
the in-app browser at the native 440×520 popover size. Deterministic fixtures
exercise normal, detecting, empty, stale, malformed-update and prerelease states.
These screenshots verify product rendering and interaction; their temperatures,
fan RPM and device state are fixture values, not hardware measurements. Native
accessibility capture timed out, so native runtime evidence is recorded
separately instead of substituting browser screenshots for the installed app.

1. Status → CPU/core history, memory, temperature and fan summary. Checked dark
   and light appearance; RPM fits its summary cell and retains the complete
   value/unit as accessible text and a tooltip.
2. Fan control → detection in progress, completed empty result, read-only and
   controllable fans. Checking no longer announces “No fan sensors,” shows stale
   mode guidance or prompts for setup before readiness is known. Inputs identify
   the individual fan, target-speed purpose and RPM/% unit.
3. Settings → startup, menu-bar display, character selector and update status.
   Korean visible copy and accessible names agree. The previous runner changes
   and Random (every minute) option are retained.
4. System → hardware availability, network, storage, battery and process search.
   Checked the four-view navigation and readable layout at popover size.

Screenshots are local ignored evidence in `target/production-polish/`:
`01-before-overview.png`, `02-before-settings.png`, `03-before-detecting.png`,
`01-after-overview.png`, `02-after-settings.png`, `03-after-detecting.png`,
`04-after-fans.png`, `05-after-system.png`, `06-after-light-overview.png`.
The before overview predates a fixture-only core-history addition; empty graph
content in that screenshot is not reported as a native product defect.

## Findings and implemented changes

| Priority | Finding | Result |
| --- | --- | --- |
| P1 | Settings/system updates called a helper outside its scope and stopped rendering process updates. | Reproduced the exact `show is not defined` browser error; switched to the global visibility helper and added loading/empty/populated/repeated-snapshot execution coverage. |
| P1 | Update ordering dropped prerelease information, so beta → stable could be missed. | Native and dashboard comparisons now use SemVer precedence; build metadata does not trigger an update, invalid versions fail closed. Browser beta → stable offers installation; malformed metadata disables it with an explicit explanation. |
| P1 | A fresh socket and replaced daemon file could be accepted while the old process still ran. | Installation completion requires a valid resident IPC version as well as a sufficient disk version; delayed copy/restart transitions cannot report premature success. |
| P1 | RPM read errors became apparent mechanical stalls. | Read failures get a distinct diagnosis and return active manual targets to OS auto. Existing cooldown prevents repeated restore/log/counter/deadline effects while sensors continue to be sampled. Targetless auto mode is preserved. |
| P2 | Startup fan detection contradicted itself with missing-hardware and setup messages. | A single busy state gates fan cards, profiles, mode guidance and setup copy until detection completes. |
| P2 | Fan RPM was clipped in the compact summary. | The cell displays the numeric value with RPM in the label and full value in accessibility text. |
| P2 | Korean settings and assistive names were incomplete; fan controls lacked purpose. | Startup/channel/navigation labels and fan target names are localized. Unchanged live status text avoids redundant DOM writes and announcements. |

## Verification

- Workspace all-feature tests: **430 passed, 0 failed, 7 ignored**.
- Formatting, all-target/all-feature clippy with warnings denied, release workspace
  build, docs checks and whitespace checks pass.
- Smoke suite: **43 passed**. Its JavaScript behavior checks execute the actual
  dashboard functions for version ordering, invalid metadata, fan states and
  unchanged live announcements, and repeated process-list rendering.
- The three new repeated-loop RPM-error regressions failed before the cooldown
  correction and pass afterward. They verify continued probing, one fallback,
  stable diagnostics/deadline during cooldown and no false fallback in auto mode.
- Independent read-only review found no remaining material regression in that
  fan-read path after the correction.

Logs: `target/production-workspace-tests.log`, `target/production-clippy.log`,
`target/production-release-build.log`, `target/production-smoke.log`,
`target/production-docs.log`. Seven ignored tests are not included in the pass
count; this pass separately ran the ignored HTML renderer for browser QA.

## Remaining production evidence

The installed privileged system daemon was not replaced, no manual fan targets
were written, and no physical fan fail-safe or extended hardware soak was run.
The daemon transport and transition tests use local sockets and mock providers.
A public universal/Windows v1.32.4 release and real OTA to that release remain
separate release work. The previously verified public OTA evidence is v1.32.2.

## Native rendering cost

A three-second real-process sample found the main drawing cost in AppKit status
item replicant/snapshot rendering, with PNG decoding still visible despite
prebuilt NSImage instances. Native runner images now request
[`NSImageCacheMode::Always`](https://developer.apple.com/documentation/appkit/nsimage/cachemode-swift.property?language=objc)
for repeated raster draws. Frame cadence, CPU response and icon sequence are
unchanged. A short same-character/default-display experiment measured 17.16%
(default cache) and 15.99% (always cache) of one CPU core over 12 seconds each.
This single sequential comparison is not a controlled performance benchmark or
proof of a durable reduction. Rendering overhead remains a production watchpoint;
it is not reported as RunCat-equivalent efficiency or an established CPU budget.
Raw local evidence: `target/production-polish-runtime.sample.txt` and
`target/native-cache-experiment.json`. The experiment restored the saved display
and character configuration without writing settings.
