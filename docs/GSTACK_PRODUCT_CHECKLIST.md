# Gstack Product Quality Gate

This is PeterFan's local 10-point product gate. Each point is worth one point
and may be checked only when its acceptance criteria are covered by code and an
automated test or a recorded visual check.

## 10/10 Local Gate

- [x] **One-second status** — Status gives a plain-language system verdict and
      cites the live CPU temperature, CPU load, and fan reading used to form it;
      the menu-bar cat visibly accelerates with smoothed CPU load.
- [x] **Honest measurements** — CPU average, hottest, stale, unavailable, and
      fan fail-safe states remain distinct; the verdict never invents a sensor.
- [x] **Predictable fan modes** — Every built-in profile explains its
      noise/cooling intent and previews the shipped default curve before or
      while it is selected.
- [x] **Immediate control feedback** — Pending, hardware-confirmed, failed, and
      duplicate fan commands have visible, stable states.
- [x] **Clear information architecture** — Status, Fans, Settings, and System
      each have one job and preserve a stable popover frame; runner appearance
      is controlled in Settings and deeper observability stays in System.
- [x] **Keyboard semantics** — Sensor disclosure, chart ranges, profile modes,
      process actions, and navigation expose native button semantics and state.
- [x] **Assistive feedback** — Live health and fan-control results use polite
      status announcements without stealing focus.
- [x] **Readable compact UI** — Utility text, hit targets, focus indicators, and
      dark/light contrast use the compact PeterFan type and spacing floor.
- [x] **Release truth** — Updates distinguish the installed app from the latest
      signed GitHub release and label development builds that are ahead.
- [x] **Regression proof** — Workspace tests, JavaScript parsing, release build,
      and the four-state visual QA image pass before release.

## External Production Evidence

These do not change the local 10-point score. They remain mandatory release
evidence because they require hardware, credentials, or elapsed time that a
source-tree check cannot prove.

- [ ] Six-hour fan-control soak result with CPU and memory bounds.
- [ ] Sleep/wake and mixed-DPI multi-display run on release hardware.
- [ ] Signed and notarized DMG installed from the public GitHub asset.
- [ ] OTA update from the previous public release with rollback drill.
- [ ] Signing and notarization from a second authorized Mac.
