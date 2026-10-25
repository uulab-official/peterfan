# Per-core CPU widget release and installed acceptance

Status (2026-10-26): v1.32.9 is public and actual 8→9 OTA is verified on one Mac.
Per-core mode with percentage off is configured. Actual installed visual/click
acceptance is pending native dashboard access; the new six-hour run has not started.

## Requested behavior

The user selected an additional **Per-core bars / 코어별 바** mode, approved
the design, and requested the existing deployment/OTA/final-build observation
work to continue. Each logical CPU has a separate 0–100% vertical bar. Entering
the mode hides the optional aggregate percentage, which can be enabled again.
Existing modes, runner and fan-control preferences remain available.

See the [approved design](superpowers/specs/2026-10-25-cpu-per-core-bars-design.md)
and [implementation plan](superpowers/plans/2026-10-25-cpu-per-core-bars.md).

## Source validation

- Real existing `CpuMetrics.per_core` readings feed the same bounded, at-most
  one-second widget sampling path. There is no extra monitor or timer.
- Raster tests cover 1/8/10/18/32/64 CPUs, separate 0/50/100% heights, missing
  readings, non-finite/out-of-range samples, cadence and topology changes.
- Isolated AppKit lifecycle passes eight snapshots, including 10 and 64 cores,
  changed image/item/button widths, percentage, hiding and ownership. Native
  image tests validate variable-width Retina storage and template appearance.
  This example does not open or modify the installed app, config or helper.
- CUA browser QA uses the shared production settings templates with fixture
  readings: Korean 64 cores plus percentage and English 8 cores at 320/390px,
  each in light/dark appearance. Keyboard Space toggles percentage; switching
  History → Per-core bars hides it and acknowledges the simulated save.
  The 320px/64-core case retains all 64 SVG columns, a 516px viewBox, and no page
  overflow: preview 150.797px, graph 99.313px, number 26.484px, children fit.
- Feature behavior checks pass 554 workspace Rust tests and 97 Node tests;
  targeted widget tests pass 14 Rust tests and 10 Node tests. Current config
  module tests pass 18 cases, including the frozen v1.32.8 reader schema.
  Strict workspace Clippy, docs/OSS/dashboard checks and native Homebrew audit
  pass. Exact v1.32.9 main CI also passes all six required jobs.
- Initial Node runs retained three unchanged launcher-fixture 10-second timeout
  failures during high host load. A later unchanged-limit serial run passes all
  97 tests. No test timeout, resource threshold or product safety rule changed.
- Fresh independent source review found no confirmed material or minor defects.
  A subsequent compatibility audit found and fixed a shared-config issue with a
  failing regression: old helpers reject a new enum value. Disk TOML now uses
  legacy `style = "bar"` plus `per_core_bars = true`; UI/JSON still use `cores`.
  The frozen old schema and new round trip both pass without a helper reinstall.

![Settings preview using fixture readings](images/peterfan-cpu-cores-settings-qa.png)

The image is a settings fixture, not the installed app or live hardware data.

## Delivery gates

- [x] Approved source implementation, compatibility regression and scoped QA.
- [x] Exact feature/release-source CI, main integration and immutable v1.32.9 tag.
- [x] Signed/notarized/stapled universal Mac app/DMG, strict trust checks, native
      Windows acceptance and complete provider/download/checksum verification.
- [x] Public publication and actual v1.32.8 → v1.32.9 OTA with fresh matching
      native readiness, exact installed bytes and preserved configuration.
- [ ] Actual installed per-core UI/style/click acceptance; final per-core mode
      enabled, percentage off, all non-CPU preferences preserved.
- [ ] Fresh strict calibration and full 21,600-second final-installed-build
      observation, independent audit, documentation and main result integration.

The installed v1.32.8 app, compatible v1.32.7 helper and current settings remained
unchanged during development. Its historical app re-execution and prior v1.32.7
six-hour result contribute no observation time to this feature. Keep all prior
release artifacts and raw results. A single-Mac result cannot establish physical
sleep/forced-install interruption or other-model acceptance.

## Public release and actual OTA — 2026-10-26

PR22 merged to immutable release source `178acb6952121206a456d9ceedaa7cdee92d9616`.
PR head and main trees match. Exact PR CI 40201976935/40201976937 and main CI
40202506708/40202506569 pass six required jobs; code/security reviews report no
findings. The annotated v1.32.9 tag is unchanged after publication.

[Public v1.32.9](https://github.com/uulab-official/peterfan/releases/tag/v1.32.9)
was published at 00:34:05 KST (release 423471632). Universal arm64/x86_64 binaries,
Developer ID signatures, app/DMG notarization and stapling, six strict trust
checks, and native Windows run 40205012140 pass. Windows verifies real metrics,
Microsoft WebView2 signature, installation, tray readiness, startup and uninstall.
All four provider sizes/digests and actual downloads, three checksum entries and
local Mac bytes match. Public unauthenticated latest/checksum readback succeeds.
The earlier v1.32.8/v1.32.7 asset identities and hashes remain unchanged.

| Public asset | SHA-256 |
| --- | --- |
| DMG | `53452533321b2b86b93f0b6fb0dd83e2965ac674a4125598bef3b4d496f1510b` |
| Universal archive | `e27b21defe2f73ed1cb1eae667164975323101356aecbfc5237f84b245da998a` |
| Windows archive | `ee88393b8521c70751d8e0301bcf76e0756b9a6d9fae468da25a3d22494f3712` |
| Checksums | `681d3f0c70b91064a5ce8ccc9df665af718eebb86dc6441b9c54f3c60eb754d7` |

The released v1.32.8 CLI used the public feed and queued installation before only
its pinned app PID85808 was terminated. New v1.32.9 PID22034 started at
00:38:39 KST with fresh matching nonce/readiness and an installed result.
Three installed bundle files match the published artifact; signature, stapling,
Gatekeeper and surviving identity pass. OTA preserves configuration SHA
`b3495d26a94ffc2aeb8693a69709c50116f8b01285fb60fc0a14d9e247f39775`.
The app automatically checks successfully at 00:39:00 KST, persists history, and
logs its next check at 06:39:00 KST (exactly 21,600 seconds later). This startup
event is not an in-window six-hour acceptance event.

The resident v1.32.7 helper PID324 and bytes remain unchanged; bundled helper9
differs intentionally and does not require privileged replacement. Doctor reports
healthy metrics/thermal/fan readiness, no issues and compatible helper7. Auto
desired/applied revision6 matches, sensors are fresh, manual targets/overrides
are empty, and cumulative sensor/write/readback failures are zero.

After OTA, only CPU preferences were atomically changed to enabled Cores with
percentage off (disk `style="bar"`, `per_core_bars=true`). Non-CPU TOML values
and file mode are preserved; final configuration SHA is
`51fcf01e1c33e0c22b601967520c99f1109ec3018b5f9db8954ea19d7484a382`.
CUA still cannot bind the installed dashboard (five-second timeout). The existing
user dashboard-open request remains pending. Configuration, native readiness,
browser fixtures and isolated AppKit snapshots do not establish actual installed
visual/style/click acceptance. Calibration and full six-hour observation remain
NOT STARTED; no elapsed uptime or earlier result is counted.

See [sanitized release/OTA evidence](evidence/2026-10-26-cpu-per-core-v1.32.9.json).
Raw receipts remain in the ignored `target/cpu-per-core-release-v1329` directory.
