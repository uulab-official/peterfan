# Roadmap

PeterFan is a small Mac-first fan controller and system monitor: simple like
RunCat, observability-minded like Stats, and accurate enough to earn comparison
with iStat Menus. Versions are plans, not promises, but this document is the
working product map we use to decide what to polish next.

## Product North Star

- **Glanceable first** — the menu bar should answer one question in one second:
  "Is my Mac okay?"
- **Simple until expanded** — the popover stays compact; deeper fan curves,
  process tables, and diagnostics live behind clear actions.
- **Honest sensors** — real readings are labeled as real, simulated/fallback
  readings are labeled as such, and CPU average vs hottest are kept separate.
- **Safe fan control** — manual control is reversible, verified by RPM readback,
  restored on exit, and overridden by critical-temperature protection.
- **No surprise password prompts** — root daemon work is explicit, predictable,
  and as quiet as macOS allows after the helper is installed.
- **Open-source friendly** — releases are reproducible from local signing
  material, docs include screenshots, and CLI/API surfaces stay scriptable.

## Current State

- [x] Universal notarized macOS DMG with `PeterFan.app`
- [x] CLI, TUI, menu-bar app, and privileged `peterfand`
- [x] Apple Silicon CPU die average and hottest temperature via IOHID
- [x] SMC fan RPM, fan control, per-fan manual pins, and auto restore
- [x] RunCat-style compact popover with a right-side action rail
- [x] Menu-bar metric selection, graph/number/both styles, and hover tooltip
- [x] 2m / 1h / 1d charts with range averages and peaks
- [x] Multi-monitor popover placement pinned to the clicked display
- [x] GitHub Releases OTA update path and local release scripts
- [x] LaunchDaemon install, self-reinstall path, and admin-prompt explanation
- [x] README screenshot generation and release readiness checks

## Now

These are the highest-leverage improvements before adding large new surfaces.

### v1.26.x — Menu-Bar Polish

- [x] CPU average temperature as the top temperature metric
- [x] RunCat-style right action rail in the popover
- [x] Compact/expanded popover modes
- [x] Keep popover on the clicked monitor in multi-display setups
- [x] Fan Control Health card in Settings: daemon, approval, command status
- [ ] Hide low-priority sections from the first viewport when they are idle
- [x] Better empty states for unsupported fans, missing battery, and no network
- [ ] Visual QA screenshots for dark/light mode and Korean/English text fit
- [ ] More stable popover height when fan cards or setup copy changes

### v1.27 — Fan Control Confidence

- [ ] Detail-window "Fan Control Health" panel: daemon version, install state,
      helper path, Team ID, LaunchDaemon state, last command result
- [ ] Make "first approval vs no prompt" state impossible to miss
- [ ] Show active fan-control input temperature: CPU average, hottest, critical
- [ ] Add fan-control dry-run diagnostics in the menu-bar detail window
- [ ] Keep a short local log of fan-control actions and failures
- [ ] Document why macOS requires approval for first LaunchDaemon install

### v1.28 — Sensor Accuracy

- [ ] Split CPU average, CPU hottest, GPU die, SSD, battery, and board sensors
      into named groups
- [ ] Distinguish Apple Silicon GPU die sensors from CPU die sensors
- [ ] Add sensor-source metadata to JSON output (`iohid`, `smc`, `battery`)
- [ ] Add "sensor debugger" output for comparing PeterFan with iStat/Stats
- [ ] Make benchmark/log commands record both average and hottest temperature
- [ ] Add tests for representative temperature selection across mixed sensors

### v1.29 — Distribution & Updates

- [x] Local signed/notarized release workflow
- [x] DMG install smoke test against `/Applications/PeterFan.app`
- [ ] Homebrew cask
- [ ] In-app release notes preview before install
- [ ] Update channel preference: stable / pre-release
- [ ] Rollback path when an update fails after download
- [ ] Clearer docs for moving signing material to another Mac

## Next

### v1.30 — Automation

- [ ] Rule editor in the detail window: battery, AC power, time, CPU temp
- [ ] Per-profile curve previews in the popover
- [ ] Import/export config from the UI
- [ ] Notifications for critical temperature, fan command failures, and updates
- [ ] "Quiet on battery, performance on AC" preset

### v1.31 — Power-User Surfaces

- [ ] Local HTTP API hardening and docs examples for Raycast/Hammerspoon
- [ ] Stream Deck / BetterTouchTool snippets
- [ ] CLI `doctor --json` focused on release/support diagnostics
- [ ] TUI curve editor parity with the detail window
- [ ] Process list filtering and "quit process" safeguards

### v1.32 — Platform Reach

- [ ] Windows read-only metrics packaging
- [ ] Windows temperature/fan research spike
- [ ] Linux `hwmon` research spike
- [ ] Separate platform capability matrix in docs

## Later

- [ ] SMAppService-style privileged helper investigation
- [ ] Plugin driver system for vendor hardware
- [ ] Multi-machine monitoring
- [ ] Web dashboard
- [ ] Mobile companion
- [ ] RGB / AIO / liquid-cooler integrations

## Completed Milestones

### Core & CLI

- [x] OS-agnostic core types, fan curves, profiles
- [x] `HardwareProvider` and `SystemMonitor` traits
- [x] Fully simulated mock backend
- [x] CLI commands for status, sensors, fans, profile, config, logging,
      benchmark, alerts, updates, and local HTTP serving
- [x] Machine-readable JSON output for automation

### macOS Sensors & Control

- [x] macOS hardware info via `sysctl`
- [x] Apple Silicon CPU die temperatures through IOHID
- [x] SMC ambient/board sensors, fan RPM, and system power
- [x] SMC fan writes with RPM verification
- [x] Critical-temperature override and restore-on-exit behavior

### Menu-Bar App

- [x] Accessory app with no Dock icon
- [x] Menu-bar number/graph/both display modes
- [x] Popover dashboard with CPU, memory, storage, temperature, battery,
      network, process, fan, and license sections
- [x] Per-fan Auto/Manual controls with fan-specific RPM ranges
- [x] Detail window with fan curve editor
- [x] Korean and English UI

### Daemon & Distribution

- [x] LaunchDaemon installer and uninstall scripts
- [x] Unix-socket IPC between menu-bar app/CLI and daemon
- [x] Root daemon self-reinstall path for quieter future updates
- [x] Developer ID signing and Apple notarization
- [x] DMG readiness checks, Gatekeeper validation, and local install tests

## Help Wanted

- **Sensor naming samples** — `peterfan temps --json` outputs from different
  Apple Silicon and Intel Macs help us map CPU/GPU/board sensors correctly.
- **Windows fan research** — EC/WMI/LibreHardwareMonitor-style readings need
  careful safety boundaries before write support.
- **Design review** — screenshots comparing PeterFan, RunCat, Stats, and iStat
  help keep the popover useful without becoming crowded.
