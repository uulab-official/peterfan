# Long-run daemon safety and six-hour acceptance — 2026-10-24

Scope: v1.32.7 candidate, one physical Mac. A six-hour run is a separate runtime
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

Run state and measured results will be recorded after the final installed build
starts and completes observation. A scheduled follow-up or an active process is
not a passing six-hour result.
