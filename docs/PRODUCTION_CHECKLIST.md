# Production Readiness Checklist

This checklist is the release gate for PeterFan. A checked item must be backed
by an automated test, a release-script check, or a recorded real-Mac result.

## P0 - Hardware Safety

- [x] Reject missing, non-finite, and non-physical control temperatures.
- [x] Return every fan to macOS automatic control when sensor reads fail.
- [x] Return every fan to macOS automatic control when a fan write fails.
- [x] Expose fail-safe state and failure counters through daemon IPC, CLI, and UI.
- [x] Back off repeated fan writes after a failure and expose the next retry.
- [x] Verify commanded fan speed with delayed RPM readback and tolerance bands.
- [x] Detect stale fan RPM while manual control is active and return to OS auto
      after repeated stale samples.
- [x] Keep every `--mock` control path isolated from the real fan-control daemon.
- [x] Restore OS automatic control on startup and test graceful exit, panic,
      and forced-kill restart recovery at the process level.
- [x] Invalidate menu-bar hardware and chart caches after a real-Mac event-loop
      sleep/wake gap; manual hardware suspend/wake evidence remains tracked separately.

## P0 - Updates And Recovery

- [x] Require GitHub asset digest and `checksums.txt` verification.
- [x] Require Developer ID identity, bundle ID, Team ID, notarization, and Gatekeeper.
- [x] Keep the previous app until the replacement passes an execution health check.
- [x] Roll back automatically when copy, signature validation, launch, or startup fails.
- [x] Persist a user-visible update result after the updater process exits.
- [ ] Add stable and prerelease update channels.

## P1 - Reliability And Performance

- [x] Enforce one menu-bar app instance.
- [x] Keep the privileged daemon update path passwordless after first approval.
- [x] Keep expensive system metrics out of fast refresh ticks.
- [x] Route CLI profile changes through the running daemon before direct SMC access.
- [x] Keep Settings limited to preferences/safety and System limited to metrics.
- [x] Drive the menu-bar runner cadence from CPU load with tested idle/busy bounds.
- [ ] Add a 6-hour control-loop soak test with bounded CPU and memory assertions.
- [ ] Add sleep/wake and multi-display regression automation.
- [x] Persist bounded diagnostic logs across app and daemon restarts: the
      menu-bar app rotates `~/Library/Logs/PeterFan/menubar.log`, while the
      daemon uses the installed `newsyslog` policy for `/var/log/peterfand.log`.

## P1 - Sensor Confidence

- [x] Label sensor source and keep CPU average, hottest, and hotspot separate.
- [x] Show the raw sensor inventory for comparison with established monitors.
- [x] Record the last update time per sensor, retain the last successful sample,
      and mark or suppress stale readings across daemon IPC, CLI JSON, and UI.
- [ ] Expand model-specific core maps with samples from M1 through M5 Macs.
- [ ] Export a privacy-safe comparison report for support and calibration.

## P1 - Release Engineering

- [x] Build universal arm64/x86_64 binaries.
- [x] Sign and notarize both the app and DMG.
- [x] Run workspace tests, Clippy, JavaScript parsing, docs, and DMG smoke checks.
- [x] Install the release DMG and verify the app and daemon versions locally.
- [x] Build arm64/x86_64 release intermediates sequentially and clean them after
      copying, so Universal packaging does not depend on excess local disk space.
- [x] Build and test Windows x64 on a native GitHub Actions runner.
- [x] Verify Windows system-metrics JSON, tray single-instance behavior,
      restart behavior, archive layout, and executable versions before upload.
- [x] Attach the Windows ZIP to the existing release and refresh the shared
      `checksums.txt` manifest without rebuilding the signed macOS artifacts.
- [x] Enforce RustSec policy in CI and remove or isolate allowed warnings.
- [ ] Test release signing and notarization from a second authorized Mac.

## Release Evidence

For every public release, record these results in the release task or changelog:

- Workspace test count and ignored-test count
- Smoke-check count
- RustSec vulnerability count and allowed warnings
- App and DMG notarization status
- Installed app and daemon versions
- App/daemon process counts
- Sensor count, fan count, and `control_health` state
- OTA current/latest version and artifact SHA-256
