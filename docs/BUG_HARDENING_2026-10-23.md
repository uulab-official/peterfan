# Configuration and resume bug hardening — 2026-10-23

## Reproductions and changes

A partial config edit previously loaded defaults after a read/parse failure and
saved those defaults over the original file. Separate read/modify/save sequences
also allowed a CLI edit and a menu-bar edit to discard each other's unrelated
changes. Direct writes could leave a truncated file after an I/O failure.

Partial edits now strictly read the latest config while holding a cross-process
sidecar lock. Only a missing file starts from defaults. An owned, private,
same-directory temporary file is flushed and atomically renamed over the target;
errors before rename retain the original bytes. Lock acquisition is bounded to
approximately 500 ms, and the OS releases the lock if its owner exits. Explicit
whole-file import remains a replacement operation. Initialization does not
replace an existing file. All app and CLI partial mutation paths use this API.

The menu-bar app now prepares runner assets and saves preferences before applying
live changes. Failed writes preserve the active selection, notification settings
and runner timer, reconcile the menu selection, and expose an error for retry.
Random timed rotations continue without unnecessary configuration writes.

The existing recovery detector relied solely on Rust `Instant`. Darwin uses
`CLOCK_UPTIME_RAW`, which does not include suspend time. The macOS event loop now
also samples `CLOCK_MONOTONIC_RAW`, preserving the existing ordinary-pause fallback
and avoiding dependence on adjustable wall time. A detected pause invokes the
existing cached-reading, runner and update-scheduler recovery plan.

Primary references: [Rust Instant platform behavior](https://doc.rust-lang.org/std/time/struct.Instant.html)
and [Apple continuous clock](https://developer.apple.com/documentation/driverkit/mach_continuous_time).

The CLI now also refuses a 33rd automation rule and rejects non-finite or
out-of-range alert thresholds (CPU/memory 0–100%, temperature 0–150°C) and zero
alert intervals. The rule count is checked against the latest locked config.
Alert saves and runtime checks validate resolved values, while a single-field
config edit can repair that field independently.

## Evidence and limits

The malformed-config regression failed with the legacy implementation and passes
with the transaction. The suspend regression likewise failed when `Instant`
advanced only 10 ms across a simulated one-hour sleep and now passes.

Tests use private temporary paths, including real child processes for concurrent
edits, lock contention, initialization and owner termination. No HOME overrides
or production config-path overrides are introduced. Partial-write failure,
malformed/invalid UTF-8 input, read-only files, non-regular targets, Unix symlinks,
owned-temp cleanup and active-preference failure paths are covered.

The lock coordinates cooperating app/CLI writers; an external editor must honor
the sidecar lock to participate. Standard permissions are preserved; extended
ACLs, ownership and xattrs are not copied by atomic replacement. Parent-directory
sync is best-effort after the commit point. No whole-system sleep was forced,
so physical suspend/resume acceptance remains separate from injected clock
regressions and actual Darwin clock availability. Public release, newer-version
OTA adoption and privileged fan hardware checks are separate delivery gates.

## Final verification

- Full contributor gate: **498 Rust tests passed, 7 intentionally ignored;
  61 Node tests passed**, plus canonical runner rendering and Rust documentation.
- Native and Windows-target Clippy with warnings denied; Rust 1.88
  all-target/all-feature check; optimized workspace build: passed.
- Optimized-binary smoke gate: **43 checks, 0 failed**. Actual mock CLI rejects
  CPU 150 and NaN with errors and accepts the documented upper bounds.
- Developer ID signed, Apple notarized and stapled local app installed with
  backup. Notary submission `a468ccd3-f0e4-4021-8f40-44c3bda45c25` was Accepted.
  Signature, staple and Gatekeeper checks passed after installation/restart;
  installed app/helper hashes match the candidate and settings bytes are intact.
- Real startup discovery succeeded after **20.180 seconds**, then after restart
  after **20.151 seconds**. Saved history was restored before network work in an
  idle, non-installable state, and both fresh checks scheduled another discovery
  **21,600 seconds** after success. The history sidecar remains mode 0600.

The first full verification attempt exhausted disk space. Only this worktree's
regenerable debug incremental cache was removed; the complete contributor and
optimized-binary smoke gates were rerun successfully afterward. The primary dirty
checkout and existing user settings were preserved. Fan-control readiness still
requires its separately privileged setup; no daemon/hardware changes were made.
