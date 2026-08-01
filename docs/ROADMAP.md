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
- [x] Apple Silicon M1-M5 CPU Core Average from generation-specific SMC thermal
      zones, with aggregate/IOHID readings as explicit fallbacks
- [x] Menu-bar temperature is the arithmetic mean of mapped CPU core zones;
      hottest, aggregate, summary, and raw sensors remain separate diagnostics
- [x] SMC fan RPM, fan control, per-fan manual pins, and auto restore
- [x] RunCat-style compact popover with a right-side action rail
- [x] Menu-bar metric selection, natural eight-frame CPU cat/number/both styles,
      and hover tooltip
- [x] 2m / 1h / 1d charts with range averages and peaks
- [x] Mixed-DPI multi-monitor popover placement pinned to the clicked display
- [x] GitHub Releases OTA update path and local release scripts
- [x] LaunchDaemon install, self-reinstall path, and admin-prompt explanation
- [x] README screenshot generation and release readiness checks
- [x] Production release gate checklist in
      [`PRODUCTION_CHECKLIST.md`](./PRODUCTION_CHECKLIST.md)
- [x] Gstack 10-point product and design gate in
      [`GSTACK_PRODUCT_CHECKLIST.md`](./GSTACK_PRODUCT_CHECKLIST.md)
- [x] iStat/RunCat capability matrix in
      [`ISTAT_RUNCAT_PARITY.md`](./ISTAT_RUNCAT_PARITY.md)

## Now

These are the highest-leverage improvements before adding large new surfaces.

### v1.26.x — Menu-Bar Polish

- [x] CPU average temperature as the top temperature metric
- [x] RunCat-style right action rail in the popover
- [x] Fixed compact popover with deeper metrics isolated in the System view
- [x] Keep popover on the clicked monitor in multi-display setups
- [x] Fan Control Health card in Settings: daemon, approval, command status
- [x] Hide low-priority sections from the first viewport when they are idle
- [x] Better empty states for unsupported fans, missing battery, and no network
- [x] Visual QA screenshots for dark/light mode and Korean/English text fit
- [x] More stable popover height when fan cards or setup copy changes

### v1.27 — Fan Control Confidence

- [x] Serialize fan commands and coalesce repeated WebView control requests so
      rapid UI input cannot apply stale targets out of order
- [x] Invalidate sensor, fan, daemon, and chart caches after a long sleep/wake gap
- [x] Detail-window "Fan Control Health" panel: daemon version, install state,
      helper path, Team ID, LaunchDaemon state, last command result
- [x] Make "first approval vs no prompt" state impossible to miss
      (explicit status in Fan/Settings and a single install action path)
- [x] Show active fan-control input temperature: CPU average, core hottest,
      safety hottest, and critical limit
- [x] Add fan-control dry-run diagnostics in the menu-bar detail window
- [x] Keep a short local log of fan-control actions and failures
- [x] Document why macOS requires approval for first LaunchDaemon install
- [x] Optimistic fan-control feedback with duplicate-command prevention
- [x] Hardware-confirmed fan mode feedback with curve temperature and target duty
- [x] Notify once per fan-control failure incident, suppress repeated retry
      alerts, and rearm only after the daemon reports recovery
- [x] Reject missing or non-physical control temperatures and immediately
      return every fan to macOS automatic control
- [x] Watch fan writes, expose failure counters in the UI/CLI, and fall back to
      macOS automatic control when a write fails
- [x] Refresh the Fan Control Health panel from the live settings update path
      and log rail navigation requests for support diagnostics
- [x] Focus the popover only on explicit user open so WebView controls receive
      input without stealing focus during app startup
- [x] Wait for the previous menu-bar process to exit before OTA replacement so
      the single-instance lock cannot reject the relaunched app
- [x] Keep the fan screen header stable while daemon state is loading and align
      fan control summaries and profile actions for compact popovers
- [x] Keep fan status truthful when hardware is absent or read-only; disable
      profile actions until at least one controllable fan is confirmed
- [x] Show hardware availability in Settings and provide a non-blocking initial
      sensor-loading state before the first dashboard payload arrives
- [x] Guard control and maintenance actions until the first payload arrives;
      keep the temperature section visible when CPU sensors are unavailable
- [x] Surface a retry action when the first sensor payload is delayed without
      running hardware reads on the WebView/UI path
- [x] Separate Settings actions from System metrics so each rail button has one
      clear job and slow storage/process reads run only on the System view
- [x] Show a view-specific loading state while slow System metrics are being
      collected, instead of presenting blank values on first entry
- [x] Prefetch fan and daemon state before the first popover open, preserve the
      last valid fan identity across transient reads, and keep loading feedback
      out of document flow so values never reflow the screen

### v1.28 — Sensor Accuracy

- [ ] Split CPU average, CPU hottest, GPU die, SSD, battery, and board sensors
      into named groups
- [ ] Distinguish Apple Silicon GPU die sensors from CPU die sensors
- [x] Add sensor-source metadata to JSON output (`iohid`, `smc`, `battery`)
- [x] Add "sensor debugger" output for comparing PeterFan with iStat/Stats
- [x] Make benchmark/log commands record both average and hottest temperature
- [x] Group the menu-bar raw sensor inventory by component kind and show its
      collection source
- [x] Keep diagnostic CPU hotspot feeds out of the critical fan-control input
      when a mapped core-hottest reading is available
- [x] Route LaunchDaemon notifications through the logged-in user's bootstrap
      session without focus or authorization prompts
- [x] Add tests for representative temperature selection across mixed sensors

### v1.30 — Glanceable System Monitor

- [x] Twenty selectable eight-frame menu-bar runners whose pace follows smoothed CPU usage
- [x] Adaptive macOS runner timer that sleeps until the next visible pose at
      steady load and wakes immediately when CPU speed changes
- [x] Menu-bar temperature / runner / combined selector in Settings
- [x] System quick facts for load average, power, network rate, and uptime
- [x] Per-core CPU detail with Apple Silicon efficiency/performance grouping
- [ ] GPU utilization backend where public platform APIs provide a stable value
- [x] User-configurable CPU-average, fan-failure, and update notifications
- [ ] Exportable diagnostics snapshot for comparing readings across monitors

### v1.29 — Distribution & Updates

- [x] Local signed/notarized release workflow
- [x] Disk-safe sequential arm64/x86_64 release builds with disposable intermediates
- [x] DMG install smoke test against `/Applications/PeterFan.app`
- [x] In-app update panel shows current/latest version and check result
- [x] In-app release notes preview before install
- [x] One-click native check, verification, install, rollback, and relaunch
- [ ] Homebrew cask
- [ ] Update channel preference: stable / pre-release
- [x] Rollback path when an update fails after download
- [ ] Clearer docs for moving signing material to another Mac

## Next

### v1.31 — Automation

- [ ] Rule editor in the detail window: battery, AC power, time, CPU temp
- [x] Per-profile curve previews in the popover
- [ ] Import/export config from the UI
- [x] Native notifications for optional CPU-average warnings, fan command
      failures, and updates; the critical daemon safety alert remains mandatory
- [ ] "Quiet on battery, performance on AC" preset

### v1.32 — Power-User Surfaces

- [ ] Local HTTP API hardening and docs examples for Raycast/Hammerspoon
- [ ] Stream Deck / BetterTouchTool snippets
- [ ] CLI `doctor --json` focused on release/support diagnostics
- [ ] TUI curve editor parity with the detail window
- [ ] Process list filtering and "quit process" safeguards

### v1.33 — Platform Reach

- [x] Windows read-only metrics packaging with a native tray app, per-user
      installer scripts, checksum validation, and GitHub Actions release upload
- [x] Gate public releases on native Windows install, WebView2, tray readiness,
      startup registration, and uninstall verification
- [x] Read firmware-provided Windows ACPI system thermal zones without
      mislabeling them as CPU-core temperatures
- [ ] Windows vendor-specific CPU temperature, fan RPM, and fan-control
      research spike
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
- [x] Apple Silicon CPU temperatures through SMC core keys and IOHID fallback
- [x] SMC ambient/board sensors, fan RPM, and system power
- [x] SMC fan writes with RPM verification
- [x] Critical-temperature override and restore-on-exit behavior

### Menu-Bar App

- [x] Accessory app with no Dock icon
- [x] Menu-bar number/graph/both display modes
- [x] Popover dashboard with CPU, memory, storage, temperature, battery,
      network, process, fan, settings, and system sections
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
