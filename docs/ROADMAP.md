# Roadmap

PeterFan is built bottom-up: a clean core and trait first, then real backends,
then richer surfaces. Versions are goals, not promises.

## v0.1 — Foundation

- [x] OS-agnostic core: types, fan curves, profiles
- [x] `HardwareProvider` trait + capability model
- [x] Mock backend (fully simulated, controllable)
- [x] macOS real hardware info via `sysctl`
- [x] CLI: `status`, `temps`, `fans`, `profile`, `curve`, `hardware`, `doctor`, `--json`
- [x] TUI dashboard (ratatui)

## v0.2 — System metrics

- [x] `SystemMonitor` trait + `metrics` types
- [x] Real cross-platform metrics via `sysinfo` (CPU, memory, disk, network, processes)
- [x] Battery via the `battery` crate
- [x] CLI: `cpu`, `memory`, `disk`, `network`, `top`, `battery`, `system`; full `status` dashboard
- [x] Mock metrics monitor for `--mock`

## v0.3 — System dashboard TUI

- [x] TUI rebuilt on `SystemMonitor`: CPU (global + per-core), memory, disk,
      network, CPU history, battery, top-process table

## v0.4 — Menu-bar app

- [x] `peterfan-menubar`: live CPU in the macOS menu bar + CPU/mem/net dropdown
      (tray-icon + tao; accessory app, Windows tray fallback)

## v0.5 — Popover dashboard (current)

- [x] Click-to-open popover: a WebView (wry) HTML/CSS dashboard — CPU (per-core),
      memory, storage, temperatures, fans, battery, network — refreshed live
- [x] Popover is the sole UI (left/right click); Quit via WebView IPC
- [x] Release binaries per tag (macOS arm64/Intel, Windows) via CI + downloads
- [x] Double-clickable `PeterFan.app` menu-bar agent bundle in releases
- [ ] Code-sign / notarize the `.app`; Homebrew cask / `winget` for install
- [ ] Configurable menu-bar metric (CPU / temp / net) and refresh interval
- [ ] Login-item ("start at login") toggle

## v0.6 — Real macOS sensors (current)

- [x] **macOS temperatures & fan RPM via SMC** (`macsmc`/IOKit) — real `temps`,
      `fans`, `status`; non-zero sensors only
- [ ] CPU/GPU **die** temps on Apple Silicon via the IOHID thermal API
- [ ] Surface SMC **power** (system total W) in the metrics model

## v0.9 — Fan control

- [x] **Fan control on macOS** via SMC writes: `fan set <pct>` / `fan auto`
      (requires `sudo`); duty mapped onto each fan's `[min, max]` RPM

## v0.10 — Daemon (current)

- [x] `peterfand` — applies a profile curve continuously, with
      **restore-on-exit** and a **critical-temp 100% override**
- [x] LaunchDaemon install (runs as root → no per-command `sudo`)
- [ ] Code-signed privileged helper (SMAppService) for a fully unsigned-free,
      no-`sudo` install; menu-bar app talks to the daemon over IPC

## v0.11 — Real CPU die temps (current)

- [x] **CPU/GPU die temperatures on Apple Silicon via IOHID** — real `CPU`
      temp in `temps`/`status`/popover; the daemon curve keys off it
- [ ] Distinguish GPU die from CPU die; surface SMC power (W)

## v0.12 — Watch mode & config (current)

- [x] `--watch [--interval N]` live refresh for CLI commands
- [x] TOML config (`profile`, `interval_secs`, `critical_temp_c`) + `config` command;
      daemon & watch read defaults from it
- [ ] More config (startup, alert thresholds, menu-bar metric choice)

## v1.0 — Control depth & Windows

- [ ] **Windows backend** — temps/fans via EC / LibreHardwareMonitor-style access
- [ ] Menu-bar app ↔ daemon IPC (switch profile / control from the popover)
- [ ] Code-signed privileged helper (no-`sudo` install); GPU die temp; SMC power (W)
- [ ] `peterfan-daemon` — privileged control service + safety watchdog
      (restore-on-exit, critical-temp force ramp)
- [ ] Curve editor in the TUI
- [ ] Desktop GUI (Tauri + React + TypeScript + Tailwind): dashboard, fan page,
      drag-to-edit curve editor, hardware page
- [ ] Benchmark / stress mode with live temp + RPM capture
- [ ] Alerts (threshold → notification / boost)

## v2.0 — Ecosystem

- [ ] **Plugin system** — vendor/community drivers (ASUS, Gigabyte, MSI,
      Corsair, NZXT, …) for new sensors, controllers, RGB, LCD, AIO coolers
- [ ] **Local HTTP API** (`GET /api/v1/status`, `/fans`, `/temps`;
      `POST /api/v1/profile`, `/curve`) for Stream Deck, Raycast, Hammerspoon,
      BetterTouchTool, Home Assistant
- [ ] Automation rules (battery → silent, AC → gaming, schedule, on-temp)
- [ ] RGB and AIO/liquid-cooler support via plugins

## v3.0 — Reach

- [ ] **Linux backend** (`hwmon`/sysfs), Wayland/X11-friendly
- [ ] Multi-machine monitoring
- [ ] Web dashboard
- [ ] Mobile monitoring app

## Help wanted

The highest-leverage contributions right now sit behind the existing
`HardwareProvider` trait:

1. **Real macOS SMC backend** — replace the `Unsupported` temp/fan stubs in
   `packages/platform/src/macos.rs` with genuine readings.
2. **Windows backend** — a new module implementing the same trait.

Neither requires touching `peterfan-core`. That's the point.
