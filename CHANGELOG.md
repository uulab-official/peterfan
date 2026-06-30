# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.28.2] — Daemon state persistence across reboots

### Added
- **`peterfand` saves its mode to disk** (`/Library/Application Support/peterfand/state.toml`
  on macOS, `/var/lib/peterfand/state.toml` on Linux) on every IPC state
  change (`hold`, `profile`, `auto`, `rules`). On next startup the last mode
  is restored — `hold:80%` survives a reboot without any extra `peterfan fan
  set` after boot. The startup log now includes `restored=<mode>`.

## [0.28.1] — `peterfan fan status` subcommand

### Added
- **`peterfan fan status`** — shows the current fan-control mode (daemon:
  `hold:N%` / `rules:…` / `auto` / `manual:profile`, or the local provider
  fallback) plus live RPM for every fan. Useful for scripting and quick checks
  without needing the full `peterfan status` output.

## [0.28.0] — Fan control without sudo + TUI keyboard fan control

### Added
- **`peterfan fan set N` no longer needs `sudo`** when `peterfand` is running:
  the command routes through the daemon IPC (`hold N%`) so the setting persists
  across reboots and the daemon re-asserts it every tick. Falls back to a direct
  SMC write (needs `sudo`) when no daemon is running.
- **`peterfan fan auto`** similarly routes through the daemon when available.
- **Daemon `hold <percent>` IPC command** — holds fans at a fixed duty until
  `auto`, `rules`, or `profile` clears it. `status` now reports `hold:N%`.
- **TUI fan control keyboard shortcuts** (when daemon is running or process has
  root): `1` silent · `2` balanced · `3` gaming · `4` performance · `5` maximum
  · `a` auto. Current daemon mode shown in the Thermals block title.
- **Menu-bar popover** shows the daemon's current mode (`rules:balanced`,
  `hold:80%`, `auto`) in real-time; shows an install-daemon tip when no daemon
  is present.
- **`peterfan status`** shows daemon mode below the Fans section.
- **HTTP API** (`peterfan serve`) fan and profile endpoints route through the
  daemon IPC when available.

### Changed
- `platform/ipc`: added shared `send_command()` helper used by CLI, TUI, and
  menu-bar — removes three copies of the same IPC logic.

## [0.27.1] — Fan-control sequence matched to a proven implementation

### Changed
- Byte-for-byte aligned the Apple Silicon unlock with the known-working
  reference (agoodkind/macos-smc-fan): after `Ftst = 1` we now wait ~0.5 s for
  the thermal servo to settle, then poll the mode key for up to ~10 s (was 4 s)
  until manual mode holds. Target RPM stays a native-endian `flt` (`F0Tg`); mode
  key casing (`F0Md`/`F0md`) auto-detected.
- The slow unlock+poll runs **at most once per connection**, so the daemon never
  burns ~10 s every tick on firmware that ignores manual control.
- `peterfan fan set N` prints an "Applying…" line so the multi-second unlock
  isn't mistaken for a hang.

Confirmed against this M3 Max via `doctor`: `F0Md` + `Ftst` keys are present, so
the sequence is applicable; physical confirmation needs one root run of
`peterfan fan set N` (it verifies by reading RPM back).

## [0.27.0] — One-prompt fan-control setup (like Macs Fan Control)

### Added
- **`peterfan install-daemon` / `uninstall-daemon`** — install the root
  fan-control helper with a single macOS password dialog (`osascript … with
  administrator privileges`), no Terminal `sudo`. After that the menu-bar buttons
  and `peterfan fan …` drive fans through the root daemon with no further
  prompts — the same model Macs Fan Control / TG Pro use. `--dry-run` prints the
  exact privileged script first.

### Why
Fan control fundamentally needs root; competitors just hide it behind a one-time
privileged helper. PeterFan already had the unprivileged-app + root-daemon
architecture — this makes installing that daemon a one-click, GUI-password step.

## [0.26.2] — `doctor` diagnoses fan-control readiness

### Added
- `peterfan doctor` now has a **Fan control readiness** section: running as root?
  `peterfand` reachable? and (macOS) a read-only SMC probe showing the fan mode
  key (`F0Md`/`F0md`), whether the `Ftst` unlock key and Intel `FS! ` key are
  present — plus a one-line verdict on how to actually drive the fans. Same data
  in `--json` under `fan_control`. Needs no root (reads key-info only).

## [0.26.1] — Apple Silicon fan control: the real unlock sequence

### Fixed
- Implemented the **`Ftst` unlock sequence** required to actually drive fans on
  Apple Silicon. A bare `F0Md = 1` is reverted by `thermalmonitord` after a few
  seconds; we now write `Ftst = 1`, poll `F0Md = 1` until it holds, set `F0Tg`
  (little-endian float), and clear `Ftst` on restore. Mode-key casing (`F0Md`
  vs M5's `F0md`) is auto-detected. (Based on community reverse engineering —
  see `docs/RESEARCH.md`.)

This is what was missing in 0.26.0, where control was un-gated but still used a
bare mode write that Apple Silicon firmware ignores. Verification (RPM
read-back) is unchanged, so `sudo peterfan fan set N` will report a real ✓/✗.

## [0.26.0] — Fan control: un-gated, root-aware, and *verified*

This release fixes the central problem: a fan controller that didn't control fans.

### Changed
- **Apple Silicon fan control is no longer disabled.** It was gated to Intel
  after early writes showed no effect — but those writes were never run as root
  (the SMC rejects non-root writes), and tools like Macs Fan Control/TG Pro do
  drive Apple Silicon fans. Control is now attempted wherever the SMC is present.
- **`peterfan fan set N` verifies the result.** It records fan RPM, writes, waits,
  then re-reads RPM and reports a real **✓ responded / ✗ no change** — instead of
  printing "ok" for a write the firmware may have ignored. The menu-bar buttons
  show daemon status the same way.
- **Clear root guidance.** Fan writes need root; `fan set` now says exactly that
  (`sudo peterfan fan set N`, or run the `peterfand` daemon) instead of a generic
  permission error.

### Note
Fan control requires **root**. Run `sudo peterfan fan set 80` (or install the
daemon) — the verification will tell you definitively whether your Mac honors
manual fan control.

## [0.25.2] — Menu-bar popover: no inner scroll, clearer fan-control state

### Fixed
- Popover no longer shows an inner scrollbar / "frame-in-a-frame" look: the
  window is sized to the exact content height (measured via `scrollHeight`
  after layout settles, reported only once real data has populated), and the
  body has `overflow:hidden`.

### Changed
- When fan control isn't available (Apple Silicon, where macOS governs the
  fans), the Fan-control section now explains *why* there are no speed buttons
  ("monitor-only" + a one-line note) instead of a terse footnote.

## [0.25.1] — Memory breakdown in `status`, docs polish

### Added
- `peterfan status` now shows the wired / active / compressed memory line
  (previously only in `peterfan memory`).

### Changed
- Docs: documented `benchmark`, `log`, and `completions` in `docs/CLI.md`;
  refreshed the README example output and feature matrix.
- GPU utilization investigated via IOReport and **deferred** rather than shipped
  inaccurate — see `docs/RESEARCH.md`. The plumbing lives behind the
  off-by-default `experimental-gpu` feature.

## [0.25.0] — Memory breakdown + CI

### Added
- **macOS memory breakdown** — wired / active / inactive / compressed bytes via
  the mach `host_statistics64(HOST_VM_INFO64)` call (the same source Activity
  Monitor uses). Shown in `peterfan memory` and exposed on the memory API.
  Cross-checked against `vm_stat`.
- **CI workflow** (`.github/workflows/ci.yml`) — `cargo fmt --check`, `clippy
  -D warnings`, and `cargo test` on every push / PR to `main`.

## [0.24.0] — Completions, logging, richer API

### Added
- **`peterfan completions <bash|zsh|fish|powershell>`** — shell completion
  scripts (clap_complete).
- **`peterfan log [--interval N] [--format csv|jsonl]`** — stream one metrics
  row per interval (time, cpu%, mem%, disk%, temp, fan rpm, power) for
  recording/piping (the spec's "Logs").
- HTTP API: **`GET /`** human-friendly index page and **`GET /api/v1/processes`**
  (top processes).

## [0.23.0] — Critical-temperature alerts

### Added
- The daemon now posts a **desktop notification** (macOS, via `osascript`) when
  the hottest temperature crosses the critical threshold — and another when it
  returns to normal (5°C hysteresis). Pairs with the existing force-to-100%
  safety override.

## [0.22.0] — Benchmark / stress mode

### Added
- **`peterfan benchmark [--secs N]`** — saturates every CPU core and samples
  CPU%, hottest temperature, fan RPM, and power once a second, then prints a
  summary (avg/peak CPU, peak temp, peak fan, peak power). `--json` too.
  Verified real: a short run drove CPU to 100%, power from ~24→35 W, and the
  fans up past 7000 RPM.

## [0.21.0] — TUI thermals panel

### Added
- The `peterfan-tui` dashboard now has a **Thermals** panel: temperature
  sensors (color-coded), fan RPMs, and total system power in the title. The TUI
  now reads the `HardwareProvider` alongside the `SystemMonitor`.

## [0.20.0] — Network IP & disk I/O

### Added
- **Per-interface local IP** and **per-disk read/write throughput** (bytes/s).
  `peterfan network` shows the IPv4 address; `peterfan disk` shows live `R …/s
  W …/s`. Both are in `--json`, `status`, and the HTTP API automatically.

## [0.19.0] — Automation rules

### Added
- **Automation rules** in the daemon: switch fan profile automatically by power
  source, temperature, or time of day. Configure in the TOML config:
  ```toml
  [[rules]]
  when = "on_battery"      # on_ac | on_battery | cpu_above:85 | time:22-7
  profile = "silent"
  ```
  Conditions are evaluated in order (first match wins); falls back to the base
  profile. The daemon reads power state and the local hour each tick.
- IPC gained `rules` (hand control back to automation) and `status` now reports
  the mode (`auto`/`manual`/`rules`). A manual `profile` over IPC overrides the
  rules until `rules`/`auto`. `peterfan config` lists the rules.

## [0.18.0] — Local HTTP API (`serve`)

### Added
- **`peterfan serve`** — a local JSON HTTP API (localhost) so other tools
  (Stream Deck, Raycast, Hammerspoon, Home Assistant, scripts) can read metrics
  and drive fan profiles:
  - `GET /api/v1/{status,system,cpu,memory,disks,network,battery,temps,fans,power}`
  - `POST /api/v1/profile` `{"name":"gaming"}` · `POST /api/v1/fan` `{"action":"auto"|"set","percent":N}`
  - CORS-enabled; single-threaded with ~1s background refresh. `--port` (default 9847).
  Verified end-to-end with curl (status keys, profile/fan POST, 404).

## [0.17.0] — Honest fan-control capability

### Changed
- **Fan control is now reported honestly per platform.** On **Intel** Macs the
  SMC fan-write path is offered (needs root/daemon). On **Apple Silicon** the
  fans are governed by the system — the same SMC writes are accepted but have no
  effect — so `control_fans` is now `false` there: `doctor` shows `✗ control
  fans`, `fan set` explains it's unavailable, and the popover hides the control
  buttons and notes "system-governed on Apple Silicon". Monitoring (CPU/die
  temps/fan RPM/power/…) is unaffected and fully real.

Background: across earlier versions the SMC write path was verified correct
(`F0Md`=ui8, `F0Tg`=flt; `FS! ` absent on Apple Silicon) and the connection is
held open, yet the physical fan does not respond on Apple Silicon. Rather than
ship a control that does nothing, PeterFan now says so.

## [0.16.0] — System power (watts)

### Added
- **Real system power draw (W)** on macOS via the SMC (`power_system_total`).
  `peterfan status` shows a **Power** line and the menu-bar popover appends it
  to the CPU line (e.g. `4.1 GHz   load …   24.3 W`). `HardwareProvider` gained
  `power_watts()` (None where unsupported).

## [0.15.0] — Hold the SMC connection (Apple Silicon fan control)

### Changed
- **Fan control now keeps the SMC write connection open** instead of opening
  and closing it per write. On Apple Silicon a forced fan reverts to automatic
  as soon as the SMC connection closes, so a one-shot `fan set` had no lasting
  effect; the **daemon holds the connection open** and re-asserts the target
  each tick, which is the correct way to hold a forced speed.

### Diagnostics / honesty
- Verified the write encoding is correct on this hardware (`F0Md` = ui8,
  `F0Tg` = `flt`; `FS! ` is absent on Apple Silicon, size 0). Writes succeed
  without error. Whether the fan physically responds depends on the machine —
  use `sudo peterfand --profile maximum` (continuous) and watch the RPM. A
  one-shot `peterfan fan set` won't hold on Apple Silicon because the process
  exits and the connection closes.

## [0.14.0] — Per-sensor & per-fan detail; sturdier fan control

### Added
- The popover now lists **every temperature sensor and every fan on its own
  line** (CPU / CPU-hottest / SSD / Airport / palm-rest …, and Fan 1 / Fan 2 …
  each with its own speed bar) instead of one truncated summary line — so
  machines with multiple CPU-die clusters or multiple fans show all of it.

### Changed
- Fan forcing now also flips the `FS! ` manual-mode bitmask (in addition to
  `Fn Md`), which some Macs require for `Fn Tg` to take effect. Best-effort:
  skipped where the key is absent. (Real-fan efficacy depends on the machine /
  SMC and needs a root daemon to exercise.)

## [0.13.2] — Daemon backend tag

### Changed
- The daemon now tags its IPC replies with its backend, e.g.
  `ok maximum (macos)` vs `ok maximum (mock)`. The popover's "Fan control"
  status shows it, so a **simulated (`mock`) daemon** can't be mistaken for one
  that actually drives the hardware — pressing a profile only moves real fans
  when a real (root) daemon is running.

## [0.13.1] — Popover control buttons always respond

### Fixed
- The popover control buttons did nothing (and gave no feedback) when no daemon
  was running. Now each button: (1) sends to the daemon if one is running and
  shows its reply, or (2) falls back to controlling fans directly via this
  process, or (3) shows a clear status (`start peterfand (needs root)`). A
  "Fan control" status line in the popover reflects the result of every click.

## [0.13.0] — Menu-bar ↔ daemon control (IPC)

### Added
- **Control buttons in the popover** — Auto / Silent / Balanced / Gaming /
  Performance / Max. They send a command to the running `peterfand` daemon over
  a Unix socket, so the menu-bar app (no privileges) can change the fan profile
  while the root daemon performs the SMC writes — **no per-action sudo**.
- **`peterfand` IPC server** (`platform::ipc`): line protocol `profile <name>` /
  `auto` / `ping` / `status` over `/var/run/peterfand.sock` (falls back to
  `/tmp`). The daemon switches profile / hands fans to the OS live; verified
  end-to-end. The socket is world-accessible (local-trust convenience).

## [0.12.0] — Watch mode & config file

### Added
- **`--watch [--interval N]`** — re-run any command on an interval, clearing
  the screen each time (a lightweight live monitor for `status`, `cpu`, `top`, …).
- **TOML config** at `~/.config/peterfan/config.toml` (platform config dir):
  `profile`, `interval_secs`, `critical_temp_c`. New `peterfan config [--init]`
  shows the path/values and writes a default file. The daemon and `--watch` now
  read their defaults from it (explicit flags still win).
- `Config` lives in `peterfan-core` (pure data + TOML); path/IO in
  `peterfan-platform::config`.

## [0.11.0] — Real CPU die temperature (Apple Silicon)

### Added
- **Real CPU/GPU die temperatures on Apple Silicon** via IOKit's IOHID
  temperature-sensor API (the SMC doesn't expose these). `peterfan temps` /
  `status` now show a real **CPU** temperature (average of the die sensors)
  plus **CPU hottest** and **SSD** (NAND), alongside the existing ambient SMC
  sensors. The menu-bar popover and the daemon's curve now key off the real CPU
  temperature.

### Notes
- Sensors are read by matching HID services on the Apple-vendor temperature
  usage page; the IOKit functions are private but exported by the framework.
  No root required.

## [0.10.0] — Fan-control daemon

### Added
- **`peterfand`** — a fan-control daemon that applies a profile's curve
  continuously (hottest temperature → curve → fan duty), with two safety
  behaviors:
  - **critical-temperature override** (`--critical`, default 90°C → 100% fans);
  - **restore-on-exit** — on `Ctrl-C`/`SIGTERM`/panic it returns the fans to
    automatic control, so it never leaves them forced.
  Flags: `--profile`, `--interval`, `--critical`, `--once`, `--mock`.
- **LaunchDaemon install** (`packaging/com.uulab.peterfan.daemon.plist` +
  `scripts/install-daemon-macos.sh`) so the daemon runs as root at boot — fan
  control then works without per-command `sudo`. (`peterfand` ships in macOS
  release archives.)

### Notes
- Running `peterfand` directly still needs root for SMC writes
  (`sudo peterfand`); the LaunchDaemon runs as root for you. `--mock` needs no
  privileges. Curve quality on Apple Silicon is limited until CPU/GPU die temps
  (IOHID) land — it currently keys off the hottest available sensor.

## [0.9.1] — Refined popover

### Changed
- Made the popover more compact and premium: tighter rows and padding, smaller
  uppercase section labels, lighter value weight with tabular-figure numerals,
  thinner bars, and subtler dividers.
- **The window now sizes itself to the content** — the WebView reports its real
  height and the window resizes to fit exactly (≈455px, down from 680), so
  there's no oversized panel or empty space.

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

[Unreleased]: https://github.com/uulab-official/peterfan/compare/v0.27.1...HEAD
[0.27.1]: https://github.com/uulab-official/peterfan/releases/tag/v0.27.1
[0.27.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.27.0
[0.26.2]: https://github.com/uulab-official/peterfan/releases/tag/v0.26.2
[0.26.1]: https://github.com/uulab-official/peterfan/releases/tag/v0.26.1
[0.26.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.26.0
[0.25.2]: https://github.com/uulab-official/peterfan/releases/tag/v0.25.2
[0.25.1]: https://github.com/uulab-official/peterfan/releases/tag/v0.25.1
[0.25.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.25.0
[0.24.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.24.0
[0.23.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.23.0
[0.22.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.22.0
[0.21.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.21.0
[0.20.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.20.0
[0.19.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.19.0
[0.18.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.18.0
[0.17.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.17.0
[0.16.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.16.0
[0.15.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.15.0
[0.14.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.14.0
[0.13.2]: https://github.com/uulab-official/peterfan/releases/tag/v0.13.2
[0.13.1]: https://github.com/uulab-official/peterfan/releases/tag/v0.13.1
[0.13.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.13.0
[0.12.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.12.0
[0.11.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.11.0
[0.10.0]: https://github.com/uulab-official/peterfan/releases/tag/v0.10.0
[0.9.1]: https://github.com/uulab-official/peterfan/releases/tag/v0.9.1
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
