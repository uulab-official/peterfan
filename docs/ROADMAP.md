# Roadmap

PeterFan is built bottom-up: a clean core and trait first, then real backends,
then richer surfaces. Versions are goals, not promises.

## v0.1 — Foundation (current)

- [x] OS-agnostic core: types, fan curves, profiles
- [x] `HardwareProvider` trait + capability model
- [x] Mock backend (fully simulated, controllable)
- [x] macOS real hardware info via `sysctl`
- [x] CLI: `status`, `temps`, `fans`, `profile`, `curve`, `hardware`, `doctor`, `--json`
- [x] TUI dashboard (ratatui)
- [ ] Config file (TOML): default profile, startup, notifications
- [ ] `peterfan-core` published docs

## v1.0 — Real monitoring & control

- [ ] **macOS SMC reading** — real CPU/GPU temps and fan RPM via IOKit/`AppleSMC`
      (Apple Silicon + Intel key sets)
- [ ] **Windows backend** — temps/fans via EC / LibreHardwareMonitor-style access
- [ ] Fan **control** on at least one platform, behind the safety model
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
