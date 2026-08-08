# Production Readiness Checklist

This checklist is the release gate for PeterFan. A checked item must be backed
by an automated test, a release-script check, or a recorded real-Mac result.

## Current Release Verification — 2026-08-08

### Public artifact status carried forward from 2026-08-05 verification

- [x] Public latest release identified as `v1.27.78`:
      https://github.com/uulab-official/peterfan/releases/tag/v1.27.78
- [x] Public DMG downloaded and SHA-256 matched GitHub `checksums.txt`:
      `d3308bc93293f4e27102723b68973577d615e18f317f89ccce750dd6844fe9d8`
- [x] Public DMG code signature valid.
- [x] Public DMG stapled notarization ticket valid.
- [x] Gatekeeper accepted the public DMG and the embedded `PeterFan.app`.
- [x] DMG volume, filesystem, bundle ID, and bundled `peterfand` verified.

### Current source build

- [x] Workspace source version is `v1.31.32`.
- [x] `cargo fmt --all -- --check`, `./scripts/check-runner-quality.sh`,
      `./scripts/check-docs.sh`, and `git diff --check` pass.
- [x] `cargo test --workspace`: 341 passed, 0 failed, 6 ignored.
- [x] `cargo clippy --workspace --all-targets -- -D warnings`: 0 warnings.
- [x] `cargo build --release --workspace` succeeded.
- [x] The staged bundle and canonical
      `/Users/uulab/Documents/workspace/uulab/peterfan/dist/PeterFan.app`
      both report `peterfan-menubar 1.31.32`,
      `CFBundleShortVersionString=1.31.32`, and `CFBundleVersion=1.31.32`.
- [x] `codesign --verify --deep --strict` succeeds for the canonical local
      bundle. The signature is local/ad-hoc only
      (`Signature=adhoc`, `TeamIdentifier=not set`), so this build is neither
      Developer ID signed nor notarized.
- [x] The previous canonical bundle was preserved recoverably at
      `/Users/uulab/Documents/workspace/uulab/peterfan/dist/PeterFan.app.backup-task9-20260808-232526`
      before replacement.
- [x] The canonical app relaunch was verified with exactly one live PID from
      `/Users/uulab/Documents/workspace/uulab/peterfan/dist/PeterFan.app/Contents/MacOS/PeterFan`,
      and `~/Library/Logs/PeterFan/menubar.log` recorded `startup mock=false`,
      `tray created`, `popover webview created`, and `popover webview ready`.
- [x] The release bundle includes `peterfand`, but this local-install task did
      not replace the system LaunchDaemon; production daemon rollout remains a
      separate release-machine step.
- [ ] Publish `v1.31.32` as a new official release.
- [ ] Sign the new release with a Developer ID identity on an authorized Mac.
- [ ] Notarize/staple the new DMG and rerun the artifact gate.
- [ ] Upload the new DMG, universal archive, and `checksums.txt` to GitHub.

### Current release-machine blockers

- [ ] Developer ID identity available in the current Keychain — current check:
      `0 valid identities found`.
- [ ] `peterfan-notary` profile or App Store Connect credentials available.
- [ ] `gh` authenticated for release upload.

The public v1.27.78 release is production-trusted. The local v1.31.32 bundle
is a development/ad-hoc build until the unchecked release-machine items above
are completed; it must not replace the public DMG in distribution links.

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
- [x] Add stable and prerelease update channels.

## P1 - Reliability And Performance

- [x] Enforce one menu-bar app instance.
- [x] Keep the privileged daemon update path passwordless after first approval.
- [x] Keep expensive system metrics out of fast refresh ticks.
- [x] Keep fan-control notifications incident-based so repeated hardware retries
      cannot flood Notification Center before a confirmed recovery.
- [x] Route CLI profile changes through the running daemon before direct SMC access.
- [x] Keep the root daemon synchronized with the logged-in user's validated
      automation rules, base profile, and custom fan curve.
- [x] Edit, validate, save, and daemon-reload ordered automation rules from the
      detail window for battery, AC power, time windows, and CPU temperature.
- [x] Copy the current TOML config and import a validated TOML config from the
      detail window with daemon reload confirmation.
- [x] Offer a non-destructive quiet-on-battery / performance-on-AC automation
      preset that remains pending until the user explicitly saves it.
- [x] Keep Settings limited to preferences/safety and System limited to metrics.
- [x] Drive the menu-bar runner cadence from CPU load with tested idle/busy bounds.
- [x] Avoid fixed-rate runner polling at steady load; schedule only the next
      visible frame while preserving fast CPU-change response.
- [x] Persist and hot-swap four built-in runner characters without changing the
      status-item width or restarting the app.
- [x] Provide a 6-hour macOS menu-bar soak test with bounded CPU, RSS, thread
      growth, process-liveness assertions, and CSV evidence. It can monitor an
      existing PID without launching a duplicate menu-bar item.
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
- [x] Export a privacy-safe comparison report for support and calibration;
      exclude hostnames, usernames, serial numbers, IP addresses, and paths.

## P1 - Release Engineering

- [x] Build universal arm64/x86_64 binaries.
- [x] Sign and notarize both the app and DMG.
- [x] Run workspace tests, Clippy, JavaScript parsing, docs, and DMG smoke checks.
- [x] Install the release DMG and verify the app and daemon versions locally.
- [x] Build arm64/x86_64 release intermediates sequentially and clean them after
      copying, so Universal packaging does not depend on excess local disk space.
- [x] Build and test Windows x64 on a native GitHub Actions runner.
- [x] Verify Windows system-metrics JSON, WebView2/tray readiness, per-user
      install, startup registration, single-instance/restart behavior,
      uninstall, archive layout, and executable versions before upload.
- [x] Keep a new release private until the verified Windows ZIP is attached,
      then publish macOS and Windows assets with one shared `checksums.txt`.
- [x] Select the Windows ZIP for Windows update checks and verify its GitHub
      digest, checksum manifest, and embedded CLI version before reinstalling.
- [x] Keep the Homebrew cask pinned to the official DMG URL and release SHA-256;
      update it from the exact notarized DMG before publishing a new version.
- [x] Never substitute mock temperatures or fans on a production backend.
- [x] Enforce RustSec policy in CI and remove or isolate allowed warnings.
- [ ] Run the credentialed signing/notarization test on a second authorized
      release Mac; the transfer runbook and verification command are ready.

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
- Homebrew cask version, DMG URL, and SHA-256
