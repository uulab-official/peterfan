# Battery health and capacity acceptance

Status (2026-10-26): design and implementation are user approved. The implementation candidate is locally verified and accepted by independent task review. The current public and installed release is
v1.32.10, which does **not** include these battery details. Final branch review, exact-source CI,
signed publication, real OTA, installed UI acceptance and a fresh final-build
six-hour observation are pending.

See the [approved design](superpowers/specs/2026-10-26-battery-health-details-design.md)
and [implementation plan](superpowers/plans/2026-10-26-battery-health-details.md).

## Intended hardware battery information

The existing hardware battery area will show current charge, battery health,
design capacity, current full-charge capacity and cycle count together. Charge
describes the energy currently remaining; health compares the battery with its
new condition. Full-charge capacity is how much it can currently hold when full,
not the current charge level. Capacity readings use explicit matching mAh units.

Reported macOS health takes precedence over a derived full/design estimate.
Calculated health is labeled **Estimated / 추정**. Missing or invalid individual
readings show **Unavailable / 확인 불가**, and a battery-free desktop has a
separate no-battery state. No remaining-years or replacement-date prediction is
made from these readings or cycle count.

## Data and resource boundaries

On the development Mac, macOS reports health 96% and cycle 110. Selected raw
battery observations include design 6249mAh and full-charge 5653mAh, while the
top-level MaxCapacity is normalized 100. A simple physical full/design estimate
therefore differs from the reported macOS health. These are source observations,
not proof that the new installed UI works; values can change before acceptance.

The implementation must not substitute normalized percentage values for physical
capacity. The existing daemon power-rule battery path stays cheap. Numeric
macOS health is requested separately, with a bounded nonblocking System Profiler
probe, long cache interval and cleanup of its own child only. There is no new
worker, timer or sampling thread. Fan control and compatible helper7 stay intact.

The development source version is 1.32.11; immutable public v1.32.10 is preserved.

## Development verification

Feature candidate `7457e8a` passes 589 workspace Rust tests (10 existing/explicit
ignored checks), 104 Node tests and 18 standalone dashboard behavior checks,
strict workspace Clippy, formatting and documentation/OSS checks. The reviewed
portable range fix at `f70b7c7` passes 28 focused dashboard checks after six
observed regression failures. Exact final-source CI remains pending. Windows
core/platform all-target cross-compilation passes; this is not native Windows
execution. The read-only native implementation probe reports 96% OS health,
5653/6249mAh full/design capacity and 110 cycles on this Mac.

CUA runs the actual exported production template with isolated fixture readings:
eight Korean/English320/390px light/dark layouts have no horizontal overflow.
Normal fixture controls verify reported→estimated→absent→partial→zero→reported
states, stale-value clearing, explicit estimate/unavailable labels and real0
charge/cycle visibility. These are fixture checks, not installed-client evidence.

![Battery detail layout with isolated fixture readings](images/peterfan-battery-details-qa.png)

This image is a test fixture, not the installed app or live battery readback.

## Acceptance gates

- [x] User-approved inline all-details design and implementation request.
- [x] Read-only source reconnaissance and test drafts preserved during the
      temporary shared-host verification hold; user explicitly resumed work.
- [x] Observed RED→GREEN backend, process-lifecycle and actual UI-function tests;
      local workspace/Clippy/Node/documentation checks.
- [x] Independent task review and scoped re-review: portable estimated-health
      display regression fixed with RED6→GREEN28; no remaining material findings.
- [ ] Final whole-branch review, exact-source CI and release metadata.
- [x] CUA fixture checks for Korean/English320/390px and light/dark, including
      estimated, missing, zero cycles, no battery and presence changes.
- [ ] New immutable source PR/CI/main/tag and complete signed/notarized Mac plus
      native Windows release; provider, downloads and checksum verification.
- [ ] Actual public-feed OTA into the installed app; matching readiness, artifact
      bytes/trust, preserved compatible helper and unrelated configuration.
- [ ] Actual installed hardware battery UI readback against macOS reported
      health and the contemporaneous physical full/design capacities.
- [ ] Strict calibration followed by a fresh continuous 21,600-second observation
      of the final installed version/configuration and independent fixed-limit audit.

Preserve v1.32.10 and all earlier immutable tags/artifacts and raw observations.
The historical v1.32.7 pass, later uptime, test fixtures and calibration add no
time to a new feature's final-build observation. Until actual installed UI access
is available, keep that gate explicitly pending. Scope remains one Mac and the
selected final configuration; other hardware, physical sleep and forced macOS
installation interruption remain unverified.
