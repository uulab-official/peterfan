# Long-run daemon safety and six-hour acceptance — 2026-10-24

Scope: public v1.32.7, one physical Apple M5 Max Mac (`Mac17,7`). A six-hour run is a separate runtime
gate; source tests, prior process uptime and the previous ten-minute run do not
complete it. The current release remains beta until the documented remaining
hardware and recovery gates have evidence.

## Failure paths addressed

Rapid IPC control changes previously created one OS worker per revision. Each
worker could retain a state snapshot while waiting for serialized SMC writes.
Slow hardware writes therefore allowed an unbounded number of waiting threads
and delayed application of superseded targets.

A failed sensor read restored automatic fan control but left a previously valid
temperature available to immediate control requests. A subsequent manual request
could use that cached value and clear the sensor failsafe. Failure restoration
also needed to share the same write serialization as manual control.

The corrected helper bounds pending work and requires current healthy sensor
readings before cached manual writes. Its fan-control behavior requires helper
v1.32.7; the independent secure self-replacement capability floor stays v1.32.5,
so secure existing helpers can install the fix without another password prompt.

The soak observer also uses the suspend-inclusive Darwin clock. A simulated
120-second suspend previously yielded a passing receipt from ordinary uptime;
the behavioral regression now rejects it without touching the external app or
configuration. Wall-clock adjustments cannot create a false elapsed interval.

## Source verification

- Contributor gate: 536 Rust tests passed (7 opt-in tests ignored); the canonical
  rendering test passed separately. All 70 Node tests, 12 executable dashboard
  checks, native Clippy, formatting and rustdoc passed.
- Daemon suite: 53/53 in serial and default parallel execution. Fake hardware
  reproduces the command burst, stale/failed/future cache, delayed failure
  invalidation, in-flight restore ordering and lock-wait sample aging failures.
- The first full gate exposed mock doctor API dependence on a stale installed
  helper. Mock API/CLI diagnostics now skip real daemon metadata, native SMC
  probes and installation receipts; the API regression and actual mock CLI pass
  while the previous helper is still installed.

## Runtime acceptance protocol

1. Verify the exact signed/notarized final app and resident daemon, the new OTA
   readiness receipt, automatic update discovery and unchanged configuration.
2. Confirm physical fan response using a bounded maximum-speed request, with a
   `finally` restoration to the original automatic mode. No extra heat or low
   fan-speed requests are used.
3. Freeze the observation scripts and pin app/daemon PID, executable, start
   identity and SHA-256. Observe both processes every ten seconds for at least
   21,600 measured seconds. Do not restart or replace them during this window.
4. Retain interval CPU (100% is one core), RSS and thread samples. Fail on process
   replacement, a sample gap over 32 seconds, configuration changes, helper
   health loss or failed threshold checks. Never restore user changes or signal
   an externally owned application on test failure.
5. For each process require average CPU <=8%, peak interval CPU <=35%, peak RSS
   <=256 MiB, final RSS growth <=64 MiB and peak thread growth <=16. Record host
   load as context rather than treating other workloads as application CPU.
6. Observe a fresh scheduled update check within the window; verify persisted
   success history and the next six-hour deadline against native log entries.
7. Use only a process-scoped idle-sleep assertion during observation. The display
   may sleep normally. This run does not prove physical sleep/wake or power-loss
   recovery, forced update interruption, Windows runtime or other Mac models.

## Signed release and installed-Mac acceptance

[Public v1.32.7](https://github.com/uulab-official/peterfan/releases/tag/v1.32.7)
was published at 2026-10-24T01:53:18Z. Immutable source tag points to
`4a9114f9b74ae54aa028df9ca2bb5499f9fa87de`.
[Source main CI](https://github.com/uulab-official/peterfan/actions/runs/40050902718)
and [Windows main CI](https://github.com/uulab-official/peterfan/actions/runs/40050902713)
pass all six checks. The [native Windows release run](https://github.com/uulab-official/peterfan/actions/runs/40051218206)
builds, installs, verifies and attaches the exact tag's Windows archive.

Both Mac architectures, app/DMG code signatures, Apple notarization tickets and
Gatekeeper acceptance pass. Complete release-directory integrity, all four
provider asset digests and all three manifest hashes match:

```text
97702c1da0e78f230fc4c05cc341a4036ef86515e56ab4806d8358116095cd6c  PeterFan-v1.32.7.dmg
7205a1a40c21825b827556fbbe1adf4a6eee19494ade3ca2380c1928cab260b0  peterfan-v1.32.7-universal-apple-darwin.tar.gz
aa081960d512eeced80ccfbc6a4cbcd24a14e6e7e169aca334ece301ae305289  peterfan-v1.32.7-x86_64-pc-windows-msvc.zip
```

The installed v1.32.6 CLI uses the public feed to download, verify, replace and
relaunch v1.32.7. A fresh per-launch native tray/WebView readiness acknowledgement
and one new surviving installed process confirm delivery. Installed app/helper
bytes match the public package, and native strict signature, staple and Gatekeeper
checks pass. The secure v1.32.6 resident helper automatically becomes v1.32.7
without another administrator prompt; root ownership, mode 0755 and a root-owned
mode 0644 service plist without quarantine are verified. User configuration stays
byte-identical.

The real fans reach 7599 RPM, 7781 RPM (reported maximum 7,826 RPM) in
9.22 seconds. A `finally` restores automatic control. The daemon confirms matching
requested/applied revisions, no manual fan targets or overrides, and no sensor,
fan-write or fan-readback failures. Fresh automatic update discovery succeeds on
the final installed app, with persisted success history and a 21,600-second next
check. These results apply to this one Mac.

## Interrupted first observation and observer repair

The first run started at **2026-10-24 10:55:13 KST**. Its helper CSV stops at
12:38:40 KST (618 samples, 6,206.86 seconds); the independent app observer stops
at 12:49:51 KST (685 samples, 6,878.21 seconds). Both observers disappeared
without terminal receipts. The original run is **inconclusive and does not pass**
the six-hour gate; neither duration will be added to the replacement run.

The host had approximately 306 MiB free when investigated. OS services report
`ENOSPC` / “No space left on device” from 12:39 KST. The precise observer exit
exception/signal was not retained, so the evidence establishes concurrent disk
exhaustion rather than a conclusively captured observer exit cause. The app and
helper retain their pinned PIDs, start identities and executable hashes, with
healthy automatic control, matching applied revisions, empty manual targets and
zero cumulative sensor/fan errors. No corresponding app/helper crash report is
present after observation start. Configuration and the primary dirty checkout
remain unchanged. Original CSVs and the separate `interruption-audit.json` remain
in `target/production-six-hour-20261024T015512Z/`; no final observer receipt was
invented for this interrupted window.

The QA tools now reserve 256 KiB of real fsynced storage per observer, write
terminal receipts through fsynced temporary files and atomic replacement, and
record handled SIGTERM/SIGINT as failure. Publication failures force a failed
result even if the bounded retry persists it. Tests cover sample/CSV/terminal
ENOSPC, partial writes, reservation failure, directory-sync failure and actual
termination of an owned test observer. All **81 Node tests** pass locally,
including the unchanged existing behavior tests. Documentation and repository
policy checks also pass. These are QA-only changes; the public v1.32.7 tag,
artifacts and installed app/helper have not been replaced.

`start-installed-soak.py` freezes the observer sources and verified baseline in
a private directory under `~/Library/Application Support/PeterFan QA/`. It
registers a temporary per-user launchd job, so macOS retains its exit status.
There is no KeepAlive restart and no persistent LaunchAgents entry. The
supervisor refuses startup below 1 GiB free and fails the run below 512 MiB,
terminating only its owned observer/sleep-assertion children. Original application
processes and user settings are never signalled or restored. A reserve cannot
guarantee a receipt after SIGKILL, device failure or competing writes; missing
receipts still invalidate acceptance.

Two 30-second installed-app/helper calibrations produced passing combined and
app receipts with launchd exit code 0. A separate owned supervisor was sent
SIGTERM: combined and app receipts both fail, launchd records exit code 1, and
both owned children stop. The external app/helper and configuration remain
unchanged. These short calibrations are **not** six-hour acceptance. Evidence is
indexed by `target/production-six-hour-preflight/launchd-calibration.json`.
Only this worktree's reproducible Rust compiler caches were removed to recover
space; source, release distributions and runtime evidence were retained.

## Replacement six-hour observation — in progress

A fresh full measurement of the same final installed app and resident helper
started at approximately **2026-10-24 13:17:22 KST**; expected completion is
approximately **19:17:22 KST**. The observation source is frozen from QA commit
`c9e5c70`; the installed release source remains the immutable v1.32.7 tag above.
The new job is `kr.co.uulab.peterfan.qa-soak.20261024t041721354368z`.

Frozen scripts, identity/hash baseline, live ten-second CSV, helper-health
samples, launchd status and final receipts are retained locally in
`~/Library/Application Support/PeterFan QA/production-six-hour-20261024T041721354368Z/`.
The active pointer remains
`target/production-six-hour-preflight/active-run-path.txt`. An hourly thread
follow-up stays quiet on unchanged progress and reports completion, failure or
required action. Scheduled automatic discovery must succeed inside this fresh
window, with native-log/history agreement and a new 21,600-second deadline.

The six-hour gate remains unchecked until the entire replacement observation
and terminal receipts pass. Physical sleep/wake, forced Mac installation
interruption, Windows runtime soak and other Mac models remain unverified.
