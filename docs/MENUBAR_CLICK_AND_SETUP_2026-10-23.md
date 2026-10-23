# Menu-bar click and setup progress verification — 2026-10-23

## Native failure and correction

On macOS 27.0.1 (26A434), the previously installed signed app opened a native
context menu on left click, remained inactive, and received no tray click
callback. `tray-icon` permanently attached its menu to the status item. The
official upstream fix attaches the menu only while presenting it. PeterFan
backports that exact change to its locked 0.24.1 package, retaining Rust 1.88
and the existing dependency graph. Provenance and removal criteria are in
[`README.backport.md`](../vendor/tray-icon-0.24.1/README.backport.md).

Native tray/menu callbacks now queue their event before waking the UI loop.
Only click events cross the tray bridge, avoiding wakeups for pointer movement.
The dashboard still uses the native clicked-display anchor and preserved frame.

A second reproduction caught focus loss arriving before the same tray press:
the popover briefly closed and then reopened. The focus handler identifies a
held left button inside the native status-item window, preserves that press's
close intent, and consumes it once. A bounded marker cannot persist indefinitely.

The final Developer ID signed, Apple-notarized app was installed in
`/Applications/PeterFan.app` with the previous app backed up. Its live PID and
version matched its private startup health acknowledgement and installed binary
hash. Three consecutive native left-click cycles opened and activated the
440×520 dashboard, then kept it closed for at least 150 ms after the second
click. Right-click menu presentation, Escape dismissal and subsequent left-click
reactivation also passed. Settings bytes were unchanged. This is local native
execution evidence on one Mac, not a supported-OS or multi-display hardware matrix.

## Installation progress

Fan-control activation and bundled-helper updates publish real stage milestones:
preflight 10%, installation 30%, verification 85%, and confirmed completion 100%.
The UI labels this as installation stages rather than downloaded bytes. It
interpolates toward the latest actual milestone over 450 ms with integer-percent
labels, never running ahead while administrator approval or installation waits.
Failure stops below 100%; retry resets the display. Hidden panels and Reduce
Motion stop animation work.

Completion requires matching installed and running daemon versions. The existing
compatible-helper path performs verification without reinstalling or escalating
privileges; compatibility uses the required minimum version. Fresh installation
still verifies the newly bundled version. Cancellation, unreachable/stale helpers
and verification failure cannot become success.

## Verification and limits

- Final contributor gate: 510 Rust tests passed, 7 ignored; the canonical ignored
  runner-render test was then explicitly run and passed. All 69 Node tests,
  10 executable dashboard behavior checks, formatting, native Clippy and Rust
  documentation checks passed.
- Windows MSVC-target Clippy and Rust 1.88 workspace checks passed locally.
- Optimized workspace build and all 43 mock/packaging smoke checks passed.
- Actual renderer HTML was exercised at 440×520 in light/dark appearance. Browser
  sampling confirmed monotonic, bounded progress and stopped animation on failure,
  hidden panels and Reduce Motion, with no JavaScript errors or overflow.
- Native callback ordering and the focus-dismissal marker have regression tests;
  daemon stage tests inject installation/health probes without privileged writes.

No privileged helper installation, physical fan control or public OTA adoption
was executed by this verification. The browser uses presentation fixtures; its
results do not establish administrator approval or real daemon startup.

Local evidence (ignored build artifacts): `target/menubar-click-final-*.log`,
`target/menubar-click-final-install.json`,
`target/menubar-click-final-native-cycles.json`,
`target/menubar-click-toggle-red.log`, and
`target/menubar-progress-browser-result.log`.

## Set Up navigation follow-up

The fan view's Korean “설정하기” / English “Set Up” button previously called the
privileged installation command directly and never opened Settings. It now opens
Settings and focuses the fan-control action without requesting installation.
Settings keeps the setup status and progress visible, and labels the subsequent
action explicitly “Enable Fan Control” or “Reinstall Fan Control”. Progress
animation runs on both visible fan and settings views and stops on other views.

Two executable regressions failed before this correction and passed afterward;
the dashboard behavior suite now has 12 checks. Actual-renderer browser clicks at
440×520 in light/dark appearance confirmed navigation, focus, zero installation
commands on the first click, one command on explicit keyboard activation, and
visible progress reaching the reported stage without JavaScript errors. The
browser's command sink is a fixture and performs no privileged installation.
The final notarized app was also installed locally: native pointer clicks on
Fans → Set Up opened Settings with the explicit activation action visible.
Owned-window captures are `target/fan-setup-native-fans.png` and
`target/fan-setup-native-settings.png`; no activation button was pressed in the
real app, and the configuration bytes remained unchanged.
