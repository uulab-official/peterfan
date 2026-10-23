# Update recovery and runtime verification — 2026-10-23

This pass raises the production candidate's update failure handling and the
trustworthiness of runtime measurements. Source checks, synthetic transactions,
local signed-app execution and public OTA remain separate acceptance levels.

## Acceptance targets

| Path | Required behavior |
| --- | --- |
| Startup | Do not report installed before this launch confirms tray and WebView readiness |
| Identity | Match fresh launch ID, PID, version and the expected live executable |
| Windows copy/startup error | Restore previous managed files; preserve unrelated files/processes |
| Concurrent helpers | Serialize one installation without replacing the active receipt |
| Failed macOS bundle removal | Retain the separate recovery backup and report failure |
| CLI version inspection | Bound execution to ten seconds |
| Soak | Complete positive CPU intervals under pinned process identity and preserve config bytes |

## Changes and regression evidence

The previous Windows helper ran the downloaded installer, replaced files one by
one and reported success when that script returned. The new helper is embedded
in the trusted app. It stages a fixed allowlist, checks captured file hashes,
waits only for processes from the target installation, retains a reverse rollback
journal and backs up previous files until the replacement acknowledges startup.
Existing shortcuts, registry integration and unrelated files are untouched.

The shared readiness writer emits a private atomic JSON receipt only when tray
and popover WebView readiness stay true for five seconds. Mock mode does not
publish it. Helpers match the correlation ID, PID and version, check the expected
process and observe another two seconds of survival. Invalid/missing readiness,
crashes and failed copies cannot become successful installs. A request ID is for
correlation, not authorization. Health tests cover unavailable UI, reset of the
readiness window, malformed/duplicate requests and write failure.

macOS also uses a persistent adjacent file lock, kept across the entire detached
apply helper. Native command-mode `lockf -k -t 0` prevents helpers from the app and
CLI from replacing the same app concurrently. Lock refusal does not overwrite
another helper's result. Its behavior is checked against the installed manual and
[Apple's utility source](https://github.com/apple-oss-distributions/shell_cmds/blob/main/lockf/lockf.1).

Review found that ignoring failed `rm` could make `mv` nest the backup inside the
failed app and report rollback success. The injected removal failure regression
failed before the guard; it now requires successful removal and target absence
before restoration. The concurrency regression likewise fails when locking is
removed. Native temporary fixtures cover signature rejection, absent UI readiness,
failed removal and held-lock refusal. Trust/launch commands are explicitly
substituted only in the no-readiness/removal fixtures; they are not public OTA.

Native Windows fixtures execute the actual helper against isolated synthetic
programs. They cover commit, crash before/after readiness, missing readiness,
wrong PID/version, mid-file locked-file failure, tampered payload, lock contention,
locked final receipt, unchanged user data and separate portable process survival.
Healthy and hanging CLI probes also exercise the bounded inspection path. Actual
runtime execution requires Windows CI; cross-Clippy and PowerShell syntax parsing
alone do not establish it.

The soak observer now calculates CPU time deltas over monotonic sample intervals
instead of sampling lifetime `%cpu`. Each sample pins executable and launch time,
validates native thread output, rejects missing samples and excessive gaps, and
records host load. External processes are never terminated. Configuration changes
invalidate the run and are never silently restored. Both the development binary
and installed app path are supported; cumulative CPU times over sixty minutes
have a regression. Reports retain failed as well as successful runs.

## Remaining boundaries

Handled I/O/startup errors have rollback; forced helper termination and power loss
have no durable automatic resume protocol and can require manual recovery from
retained backup files. No machine-wide sleep, network disruption, fan-control
setup or privileged daemon replacement is forced during this pass. A local app
health acknowledgement and a ten-minute observation do not establish six-hour
stability, low-load energy targets, physical sleep/wake acceptance, a signed
Windows user's desktop upgrade or public newer-version OTA adoption.

## Local verification

The final local contributor gate passed **505 Rust tests (7 intentionally
ignored), 69 Node tests**, canonical runner rendering and documentation. Native
and Windows-target Clippy denied warnings; Rust 1.88 all-target/all-feature check,
optimized workspace build and **43 smoke/package checks** passed.

The already-installed baseline source `89d29f8` completed a read-only
**600.023-second / 120-interval** observation. Average process CPU was **1.4283%**
of one core, peak interval CPU **4.5790%**, peak RSS **88,608 KiB** (86.53 MiB),
final RSS growth **15,584 KiB**, and peak thread growth **14** against its starting
sample. All configured thresholds passed and configuration bytes were preserved.
The host had eighteen logical CPUs and substantial concurrent build activity;
this is ordinary-workload baseline evidence, not a low-load or new-build soak.
The first attempt correctly failed because the observer did not yet recognize
the installed bundle basename; that path and long CPU-time parsing now have
regressions, and the complete observation was rerun successfully.

Local evidence is under `target/production-update/`. New-candidate startup,
readiness acknowledgement, native Windows CI and any further runtime observations
must be recorded separately before claiming those acceptance levels.

The new local candidate is Developer ID signed, notarized (submission
`8098b110-ded4-4e5f-92b0-1c9bd11b34f8` Accepted), stapled and Gatekeeper accepted.
It was installed with an exact prior-binary guard, app/settings backup and
unchanged settings. Actual native logs show startup readiness acknowledged after
**6.604 seconds**, **5.008 seconds after WebView readiness**; the nonce/PID/version
matched the installed app and the receipt was mode 0600. The app was then restarted
normally to verify history restoration and fresh update discovery. App/helper
hashes still match the trusted candidate. The privileged daemon was not replaced.
