# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.9.0] — Fan control

### Added
- **Fan control on macOS** via SMC writes. New commands:
  - `peterfan fan set <pct> [--fan N]` — force fan(s) to a duty cycle.
  - `peterfan fan auto [--fan N]` — restore automatic (OS-managed) control.
  `peterfan profile <name>` now also applies on macOS.
- Implemented a minimal SMC write client (`smc_write`, IOKit) since `macsmc` is
  read-only. Duty % is mapped onto each fan's real `[min, max]` RPM range.

### Notes
- SMC writes are **privileged**: without root the kernel returns
  `kIOReturnNotPrivileged`, surfaced as a clear "re-run with `sudo`" error.
  Use `sudo peterfan fan set 60`.
- **Safety**: forced control persists until `fan auto` (or reboot) — the CLI
  warns about this on every `set`. Target RPM is clamped to the fan's rated
  range. A daemon with restore-on-exit / critical-temp ramp is future work.

## [0.8.1] — App icon

### Added
- A proper **app icon** for `PeterFan.app` — a white four-blade fan on a
  teal→sky→blue gradient squircle. Generated from `tools/icongen` (tiny-skia)
  into `assets/icon-1024.png`, turned into `assets/AppIcon.icns` by
  `scripts/make-icns.sh`, and bundled by `scripts/bundle-macos.sh`.

## [0.8.0] — Double-clickable .app + consistent precision

### Added
- **`PeterFan.app`** — a double-clickable macOS menu-bar agent bundle
  (`LSUIElement`, no Dock icon), assembled by `scripts/bundle-macos.sh` and
  attached to macOS releases. Drag to /Applications and open.

### Fixed
- The menu-bar CPU percentage and the popover's CPU value disagreed because
  they rounded to different precision (e.g. `43%` vs `42.8%`). Both now use one
  decimal, so they always match.

## [0.7.1] — Clean menu-bar title

### Fixed
- The menu-bar title showed a block-character CPU sparkline that smeared into a
  solid white bar at high load. Replaced it with a plain, always-readable CPU
  percentage (e.g. `42%`) next to the icon.

## [0.7.0] — Unified popover with temps & fans

### Changed
- **The popover is now the whole menu-bar UI** — both left- and right-click
  (two-finger) open it, so there's no more inconsistent native menu. Quit moved
  into the popover (a button, via WebView IPC).
- **Added Temperature and Fans sections** to the popover (real SMC data on
  macOS): hottest temperature with the rest in the sub-line, and per-fan RPM.
- Refined spacing, alignment, and typography (consistent padding, uppercase
  section labels, aligned values and bars).

## [0.6.0] — Real macOS temperatures & fans

### Added
- **Real temperature and fan readings on macOS via the SMC** (`macsmc`/IOKit),
  no privileges required. `peterfan temps` / `fans` / `status` now show genuine
  data instead of the simulated fallback. Fans report actual/min/max RPM.

### Notes
- Only sensors that return a plausible value are shown. On Apple Silicon the SMC
  doesn't expose CPU/GPU **die** temps (they read 0 and are filtered); sensors
  the chip does expose (airflow/airport, palm rest, memory) are reported.
  CPU/GPU die temps need the IOHID thermal API — a future milestone.
- Fan **control** (SMC writes) is not yet implemented; fans are read-only
  (`controllable: false`).

## [0.5.0] — Popover dashboard

### Added
- **Left-click the menu-bar icon for a clean popover dashboard** — a borderless
  WebView window (wry) rendering an HTML/CSS panel à la RunCat/Stats: CPU (with
  a live per-core bar chart), memory, storage, battery, and network, each with
  an icon, sub-stats, and a load-colored progress bar. It positions itself under
  the icon, refreshes once a second, and closes when it loses focus.
- Right-click still opens the native menu (same figures + Quit) as a fallback.

## [0.4.2] — Readable menu-bar rows

### Fixed
- Menu-bar dropdown rows were rendered dim/grey because every row was a
  *disabled* menu item (macOS dims disabled items). Data rows are now enabled
  so they render in full, readable color; the header stays a subtle title.

## [0.4.1] — Professional menu-bar UI

### Changed
- Polished the menu-bar dropdown to a proper mini-dashboard: each row now has a
  load-colored status dot, a `▕████░░░░░▏` block-bar gauge, and aligned figures
  — CPU (with a per-core sparkline row), memory, disk, network, and battery
  (battery row only shown when present). The header shows the CPU brand.
- The menu-bar title now shows a tiny CPU-usage sparkline next to the percentage.

## [0.4.0] — Menu-bar app

### Added
- **`peterfan-menubar`** — a macOS menu-bar app (à la Stats) that shows live
  CPU usage in the menu bar with a dropdown of CPU / memory / network detail and
  a Quit item, refreshing once a second from the shared `SystemMonitor`. Runs as
  an accessory app (no Dock icon) via `tray-icon` + `tao`. On Windows the same
  binary shows a system-tray icon with the metrics in its tooltip. `--mock`
  drives it from the simulated machine. Run with `cargo run -p peterfan-menubar`.

## [0.3.0] — System dashboard TUI

### Changed
- **`peterfan-tui` is now a full system dashboard.** It polls the
  `SystemMonitor` once a second and renders CPU (global gauge + per-core
  sparkline + frequency/load), memory, disk(s), aggregate network throughput,
  a live CPU-usage history sparkline, battery, and a top-process table. Quit
  with `q`/`Esc`/`Ctrl-C`; `--mock` drives it from the simulated machine.

## [0.2.0] — System metrics

### Added
- **Real, cross-platform system metrics** via the `sysinfo` crate (macOS,
  Windows, Linux): CPU usage (global + per-core), frequency, load average,
  memory & swap, mounted disks, network throughput, and top processes.
- **Battery** state via the `battery` crate: charge, state, cycle count, time
  remaining, vendor/model, energy rate. State-of-health is filtered when the
  underlying crate reports an implausible value (a known Apple Silicon quirk).
- New core seam: the `SystemMonitor` trait plus `metrics` types, alongside a
  real `SysinfoMonitor` and a simulated `MockMonitor`.
- New CLI commands: `cpu`, `memory` (`mem`), `disk` (`disks`), `network`
  (`net`), `top` (`proc`, `--mem`, `-n`), `battery`, `system`. `status` is now a
  full dashboard combining system metrics and thermals.
- Performance: the monitor keeps a single long-lived handle and refreshes only
  the metric families it exposes (not `refresh_all`), tracking the sample
  interval to convert byte deltas into per-second network rates.

## [0.1.0] — Foundation

### Added
- Initial workspace scaffold: `peterfan-core`, `peterfan-platform`,
  `peterfan-cli`, `peterfan-tui`.
- OS-agnostic core: temperature/fan/hardware types, validated fan curves with
  linear interpolation, and built-in profiles (Silent / Balanced / Gaming /
  Performance / Maximum / Custom).
- `HardwareProvider` trait with an up-front capability model.
- Mock backend: a fully simulated, controllable machine with drifting temps.
- macOS backend: real, read-only hardware info (CPU, memory, OS) via `sysctl`.
  Temperature/fan reading (SMC) is not yet implemented and reports
  `Unsupported`; the CLI/TUI fall back to simulated sensor data, clearly
  labeled.
- CLI (`peterfan`): `status`, `temps`, `fans`, `profile`, `curve`, `hardware`,
  `doctor`, with global `--mock` and `--json` flags.
- TUI (`peterfan-tui`): live ratatui dashboard with temperature/fan gauges and a
  CPU-temperature sparkline.
- Documentation: README, architecture, roadmap, CLI reference, contributing.

[Unreleased]: https://github.com/uulab-official/peterfan/compare/v0.9.0...HEAD
[0.9.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.9.0
[0.8.1]: https://github.com/uulab-official/peterfan/releases/tag/v0.8.1
[0.8.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.8.0
[0.7.1]: https://github.com/uulab-official/peterfan/releases/tag/v0.7.1
[0.7.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.7.0
[0.6.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.6.0
[0.5.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.5.0
[0.4.2]: https://github.com/uulab-official/peterfan/releases/tag/v0.4.2
[0.4.1]: https://github.com/uulab-official/peterfan/releases/tag/v0.4.1
[0.4.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.4.0
[0.3.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.3.0
[0.2.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.2.0
[0.1.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.1.0
