# Production Readiness Checklist

This checklist is the release gate for PeterFan. A checked item must be backed
by an automated test, a release-script check, or a recorded real-Mac result.

## Automatic update reliability — 2026-10-23

- [x] Replace launch-only discovery with a 20-second startup check, six-hour
      successful interval and bounded 5/15/60-minute retries. Incomplete verified
      assets retry without claiming a failed metadata request.
- [x] Serialize background/manual checks, installation, channel changes and
      config imports. Clear stale channel metadata and detect external channel
      edits within the scheduler's 30-second poll.
- [x] Show last attempt, last success and next check; reject stale/mismatched
      installation receipts and keep failed check details visible.
- [x] Preserve Windows custom install paths and PowerShell literal/Unicode
      encoding; script preparation failure cannot leave a new pending receipt.
- [x] Full contributor gate: 450 Rust tests passed, 7 ignored; 56 Node tests
      passed. Native/Windows Clippy and Rust 1.88 check passed. Browser fixtures
      confirmed visible failures, manual recovery and disabled incomplete assets.
- [x] Local arm64 candidate from `6c3f279` is Developer ID signed, notarized,
      stapled and Gatekeeper accepted. Installed app/helper hashes match the
      candidate, one native process runs and configuration bytes are preserved.
      Its real automatic check completed 20 seconds after startup and scheduled
      the next check exactly 21,600 seconds after success.
- [ ] Public release and OTA to a newer published version.

See [check policy and verification boundaries](AUTOMATIC_UPDATES.md). Notification
history and check timestamps currently reset when the app restarts. Windows
transactional rollback and post-launch health verification remain separate work.

## Settings interaction polish — 2026-10-23

- [x] Match save/import results to window-specific requests; retain newer drafts,
      failed drafts and unconfirmed drafts without automatic resubmission.
- [x] Distinguish saved configuration from confirmed, deferred or failed daemon
      reload; a nonempty error reply cannot report successful reload.
- [x] Preserve focused temperature input across live snapshots and align random
      runner frame scheduling with fixed character selection on Windows.
- [x] Full contributor checks: 437 Rust tests passed, 7 ignored; 42 Node tests
      passed. Rust 1.99 native/Windows-target Clippy and Rust 1.88 check passed.
- [x] Regenerated real dashboard HTML and verified draft/input preservation in
      dark/light 440×520 browser fixtures, without writing real settings.

See [reproductions and evidence limits](INTERACTION_POLISH_2026-10-23.md).
Source and browser checks do not imply a public release or physical fan test.

## Local v1.32.4 production polish — 2026-10-22

- [x] Settings/system process rendering no longer throws an out-of-scope
      JavaScript helper error. Empty, populated and repeated snapshots execute
      successfully; browser reproduction changed from `show is not defined`
      to zero recorded script errors across those views.
- [x] Fan detection has one pending state; compact RPM values fit; Korean
      startup/channel/navigation and fan target accessible names are complete.
      Dark/light 440×520 browser QA uses the actual renderer with fixture data.
- [x] Native and dashboard SemVer ordering agree for prerelease/stable upgrades
      and malformed versions cannot offer installation.
- [x] Daemon installation completion requires the resident IPC version, not
      socket reachability alone. RPM read failures are distinct from stalls,
      restore active targets once and retain cooldown while sampling continues.
      The three repeated-loop regression tests failed before the correction.
- [x] All-feature workspace tests: 430 passed, 0 failed, 7 ignored; clippy with
      warnings denied, formatting, release build, docs and 43 smoke checks pass.
      Six JavaScript behavior checks run within the smoke suite.
- [x] Final local arm64 app is Developer ID signed, notarized, stapled and
      accepted by Gatekeeper, installed with byte-for-byte settings preservation
      and backups. Installed app/helper hashes match the signed candidate.
- [x] Real native startup, one menu-bar process, tray creation, WebView readiness
      and live CPU/thermal metric readiness were observed. Native accessibility
      capture timed out; browser screenshots are not native hardware evidence.
- [x] Repeated native runner drawing uses an explicit raster cache while keeping
      the shared CPU cadence. A short A/B measurement and native call-stack
      sample are recorded in the audit; no efficiency parity is claimed.
- [ ] Longer controlled rendering/energy measurements and a stable CPU budget.
      The observed AppKit status-item snapshot cost remains a watchpoint.
- [ ] Privileged system daemon replacement, physical fan fail-safe checks and
      extended hardware soak. The existing daemon remains unreachable and fan
      control still reports `needs_setup`; metrics and thermal reads are ready.
- [ ] Public v1.32.4 release/OTA. GitHub latest was freshly read back as v1.32.2;
      Homebrew remains pinned to that verified public release.

See [audit, fixes and evidence limits](PRODUCTION_POLISH_2026-10-22.md).
Local receipts/logs are `target/production-polish-*.json` and
`target/production-*.log`; backups are in `dist/production-polish-backups/`.

## Local v1.32.3 runner polish — 2026-10-22

- [x] All 20 characters retain the shared CPU clock; authored anatomy,
      planted/recovering feet, connected wings and dancer joints were revised.
      Human is now an original athletic silhouette, and Dinosaur's artificial
      whole-sprite jumps were reduced. Geometry/perceptual quality gates pass.
- [x] Settings and native character menus offer Random (every minute). A shuffled
      bag visits all characters before reuse and avoids consecutive repeats.
      Fixed selection disables random mode; number-only and Reduce Motion pause
      timed replacement. The preference survives configuration round trips.
- [x] Workspace all-feature tests: 414 passed, 0 failed, 7 ignored.
      Clippy, formatting, docs, native Homebrew audits, all 42 smoke checks and
      whitespace checks pass.
- [x] Browser verification of the real settings code at the native 440×520 size: random selection and return to a fixed character
      send the expected IPC commands and keep the active character preview.
- [x] The local arm64 app is Developer ID signed, notarized, stapled and accepted
      by Gatekeeper. Installed in `/Applications/PeterFan.app` after backing up
      the existing v1.32.2 bundle and preserving the user's exact config bytes.
- [x] Real native random-mode run observed City → Rabbit → Robot at approximately
      60-second intervals, with one running app, tray creation, WebView readiness
      and no artwork fallbacks. Restarted with the user's saved fixed choice
      after verification. Local evidence is in `target/random-polish-*.json`.
- [ ] Public v1.32.3 release and OTA publication. Public downloads and Homebrew
      remain on the verified v1.32.2 release below.

## v1.32.2 public release — 2026-10-22

- [x] Public latest release identified as `v1.32.2`:
      https://github.com/uulab-official/peterfan/releases/tag/v1.32.2
- [x] Exact release source: `6cb5f921e9a4d85f335da306bb2d1eb1ddc8569d`.
      PR #1 merged after macOS CI and Windows CI passed. Local workspace tests:
      407 passed, 0 failed, 6 ignored; clippy and formatting passed.
- [x] Twenty runner characters share one CPU cadence, with no character-specific
      speed cap or repeated pose hold. The pushup uses fixed contact points and
      rigid body geometry. Browser preview and animation regression checks passed.
- [x] Universal macOS app and DMG signed with Developer ID Team N99FMBQ662,
      accepted by Apple notarization, stapled, and accepted by Gatekeeper.
      DMG volume/filesystem, bundle identifier and bundled helper verified.
- [x] Windows release workflow built, tested and uploaded the x64 archive:
      https://github.com/uulab-official/peterfan/actions/runs/39763998309
- [x] All three published assets and combined checksums manifest match GitHub
      provider SHA-256 digests. Public CLI integrity verification passed for the
      DMG and universal archive, including both embedded apps' trust checks.
- [x] Actual stable OTA from the verified public v1.27.78 app downloaded v1.32.2,
      installed it, wrote a durable `installed` receipt and relaunched one process.
      Installed app/helper hashes match the release; config and fan-action data
      were preserved. Startup, tray creation and WebView readiness were observed.
- [x] The new v1.32.2 updater also downloaded and reinstalled the public release,
      exercising its exact-version/helper validation before a second successful
      replacement, durable receipt and healthy relaunch with preserved settings.
- [x] Homebrew Cask and Formula pinned to the verified public v1.32.2 assets.
- [ ] Physical fan-control daemon replacement and extended hardware soak are
      outside this runner/OTA release verification. The existing system daemon
      was not manually replaced during the OTA check.

Published SHA-256 values:

| Asset | SHA-256 |
| --- | --- |
| `PeterFan-v1.32.2.dmg` | `e8edcec56126ad7afc570b94163e8272f21123c158ed8ba68e6c076ed3ef2cd6` |
| `peterfan-v1.32.2-universal-apple-darwin.tar.gz` | `1459f711391cd4fc9e843bd1eadf6cfb8bf1b745eef1cda4fc69129170e6ee9e` |
| `peterfan-v1.32.2-x86_64-pc-windows-msvc.zip` | `1aa395444eb76d475a7a0acf3e80c697ac6c35697013491c4f28e9a0c06629f3` |

The older dated sections below are historical verification snapshots. Their
source versions and former signing-machine blockers do not describe v1.32.2.

## Bugfix Verification — 2026-08-13

- [x] Workspace source and local app bundle are v1.32.1.
- [x] Manual stale-daemon reinstall falls back to one privileged install when
      a local ad-hoc bundle cannot satisfy the production signature check.
- [x] Homebrew cask remains pinned to the documented public v1.27.78 DMG and
      its recorded SHA-256 while v1.32.1 is not publicly signed/notarized.
- [x] macOS release checker validates the ignored `private/` directory path.
- [x] Local install keeps one running menu-bar PID and preserves the previous
      app bundle at `/Applications/PeterFan.app.backup-pre-bugfix-20260813`.
- [ ] Developer ID signing, notarization, and public v1.32.1 upload remain
      release-machine work because this Mac has no Developer ID identity or
      notary credentials.

## Current Release Verification — 2026-08-10

### Public artifact status carried forward from 2026-08-05 verification

- [x] Historical public release identified as `v1.27.78`:
      https://github.com/uulab-official/peterfan/releases/tag/v1.27.78
- [x] Public DMG downloaded and SHA-256 matched GitHub `checksums.txt`:
      `d3308bc93293f4e27102723b68973577d615e18f317f89ccce750dd6844fe9d8`
- [x] Public DMG code signature valid.
- [x] Public DMG stapled notarization ticket valid.
- [x] Gatekeeper accepted the public DMG and the embedded `PeterFan.app`.
- [x] DMG volume, filesystem, bundle ID, and bundled `peterfand` verified.

### Current source build

- [x] Workspace source version is `v1.32.0`.
- [x] `cargo fmt --all -- --check`, `./scripts/check-runner-quality.sh`,
      `./scripts/check-sleep-display-regressions.sh`, `./scripts/check-docs.sh`,
      and `git diff --check` pass.
- [x] `cargo test --workspace`: 376 passed, 0 failed, 6 ignored.
- [x] `cargo clippy --workspace --all-targets -- -D warnings`: 0 warnings.
- [x] `cargo build --release --workspace` succeeded.
- [x] The staged bundle and canonical
      `/Users/uulab/Documents/workspace/uulab/peterfan/dist/PeterFan.app`
      both report `peterfan-menubar 1.32.0`,
      `CFBundleShortVersionString=1.32.0`, and `CFBundleVersion=1.32.0`.
- [x] `codesign --verify --deep --strict` succeeds for the canonical local
      bundle. The signature is local/ad-hoc only
      (`Signature=adhoc`, `TeamIdentifier=not set`), so this build is neither
      Developer ID signed nor notarized.
- [x] The previous canonical bundle was preserved recoverably at
      `/Users/uulab/Documents/workspace/uulab/peterfan/dist/PeterFan.app.backup-pre-v1.32.0-20260810-134838`
      before replacement.
- [x] The post-audit local replacement also preserved its immediate predecessor
      at `/Users/uulab/Documents/workspace/uulab/peterfan/dist/PeterFan.app.backup-pre-v1.32.0-audit-20260810-135521`.
- [x] The canonical app relaunch was verified with exactly one live PID from
      `/Users/uulab/Documents/workspace/uulab/peterfan/dist/PeterFan.app/Contents/MacOS/PeterFan`,
      and `~/Library/Logs/PeterFan/menubar.log` recorded `startup mock=false`,
      `tray created`, `popover webview created`, and `popover webview ready`.
- [x] The release bundle includes `peterfand`, but this local-install task did
      not replace the system LaunchDaemon; production daemon rollout remains a
      separate release-machine step.
- [ ] Publish `v1.32.0` as a new official release.
- [ ] Sign the new release with a Developer ID identity on an authorized Mac.
- [ ] Notarize/staple the new DMG and rerun the artifact gate.
- [ ] Upload the new DMG, universal archive, and `checksums.txt` to GitHub.

### Current release-machine blockers

- [ ] Developer ID Application identity available in the current Keychain —
      none found; only an Apple Distribution identity is installed.
- [ ] `peterfan-notary` profile or App Store Connect credentials available —
      `notarytool` reports no Keychain item for that profile.
- [ ] `gh` authenticated for release upload — the `gh` CLI is unavailable on
      this Mac.

The public v1.27.78 release is production-trusted. The local v1.32.0 bundle
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

- [x] Safe process control filters the complete native snapshot, revalidates
      PID/start-time identity and current-user ownership immediately before
      macOS `SIGTERM`, protects PeterFan/system targets, and requires inline
      two-step confirmation without exposing a direct WebView kill path.
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
- [x] Add deterministic sleep/wake and multi-display regression automation via
      `./scripts/check-sleep-display-regressions.sh`; real sleep/wake and
      mixed-DPI hardware evidence remains tracked in the external gate below.
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
