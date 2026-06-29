# PeterFan

> **Tiny fan control for developers.** A cross-platform fan controller and
> hardware monitor with a CLI, a TUI, and (soon) a desktop GUI — built in Rust.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](./LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.80%2B-orange.svg)](https://www.rust-lang.org)
![Status: pre-alpha](https://img.shields.io/badge/status-pre--alpha-red.svg)

PeterFan is **not** just a fan-speed slider. It's a small, safe, scriptable
system monitor *and* fan-control platform for developers and power users — the
kind of tool you `brew install` next to `lazygit`, `btop`, and `mise`. Think of
the system-monitoring breadth of [Stats](https://mac-stats.com), but
cross-platform and developer-first (CLI + TUI + `--json`).

```text
Tiny · Simple · Beautiful · Safe · Extensible · Cross-platform
```

No ads. No bundleware. No vendor lock-in. MIT-licensed.

---

## Status

⚠️ **Pre-alpha — v0.8.1.** This is an early, honest foundation:

| Area | State |
| --- | --- |
| **System metrics** — CPU, memory, disk, network, processes | ✅ real, cross-platform (macOS + Windows) via `sysinfo` |
| **Battery** — charge, state, cycles, time remaining | ✅ real via `battery` (health filtered on Apple Silicon) |
| Core model (types, metrics, curves, profiles, traits) | ✅ implemented & tested |
| Mock backends (fully simulated machine + metrics) | ✅ implemented |
| macOS hardware info (CPU/RAM/OS via `sysctl`) | ✅ real, read-only |
| **macOS temperatures & fan RPM** (SMC via `macsmc`) | ✅ real (Apple Silicon: airflow/palm-rest/etc.; CPU/GPU die temps need IOHID — planned) |
| Windows temperature / fan reading (EC) | 🚧 planned |
| GPU utilization | 🚧 planned |
| Fan **control** (SMC writes) | 🚧 planned (reading works; control is next) |
| CLI — `status`, `cpu`, `memory`, `disk`, `network`, `top`, `battery`, `system`, `temps`, `fans`, `profile`, `curve`, `hardware`, `doctor` | ✅ runnable |
| TUI system dashboard (ratatui) — CPU/mem/disk/net/battery/processes | ✅ runnable |
| **Menu-bar app** — live CPU in the menu bar + a click-to-open popover dashboard (WebView): CPU/memory/storage/temps/fans/battery/network | ✅ runnable |
| Desktop GUI (Tauri), daemon, plugins, HTTP API | 🗺️ roadmap |

When a backend can't read real sensors yet, the CLI/TUI **transparently fall
back to the mock backend and clearly label the data as `simulated`** — so you
always get a working demo, and we never pretend a reading is real when it isn't.

See [`docs/ROADMAP.md`](./docs/ROADMAP.md) for the full plan.

---

## Download

Prebuilt binaries are attached to each [GitHub Release](https://github.com/uulab-official/peterfan/releases/latest).
Each macOS archive contains `peterfan` (CLI), `peterfan-tui` (dashboard),
`peterfan-menubar` (menu-bar binary), **and a double-clickable `PeterFan.app`**
menu-bar agent. macOS (Apple Silicon + Intel) and Windows builds are produced by
CI on every tagged release.

```sh
# macOS (Apple Silicon) — from the Releases page
tar -xzf peterfan-*-aarch64-apple-darwin.tar.gz
cd peterfan-*-aarch64-apple-darwin

# the build is unsigned, so clear the quarantine flag once:
xattr -dr com.apple.quarantine PeterFan.app peterfan*

# menu-bar app: drag PeterFan.app to /Applications and double-click it
open PeterFan.app
# …or use the CLI / TUI directly
./peterfan status
```

## Build from source

Requires a [Rust toolchain](https://rustup.rs) (1.80+).

```bash
# Build everything
cargo build

# Full dashboard for THIS machine (real CPU/mem/disk/net/battery)
cargo run -p peterfan-cli -- status

# Individual metrics
cargo run -p peterfan-cli -- cpu
cargo run -p peterfan-cli -- top --mem -n 5
cargo run -p peterfan-cli -- network

# Diagnose the active backends & their capabilities
cargo run -p peterfan-cli -- doctor

# Everything against the simulated machine (great for demos/CI)
cargo run -p peterfan-cli -- --mock status

# Live terminal dashboard
cargo run -p peterfan-tui -- --mock

# Live metrics in the macOS menu bar (Windows: system tray)
cargo run -p peterfan-menubar
```

Once installed, the binary is simply `peterfan`.

### Example: `peterfan status`

```text
PeterFan v0.2.0
backend: sysinfo + macos  ·  macOS 26.1  ·  up 4d 7h 8m

CPU · Apple M3 Max
   21.3%  ███░░░░░░░░░   cores ▄▃▂▁ ▂ ▁▁▁▃▁▃▂

Memory
  25.7 GB / 36.0 GB ( 71.3%)  █████████░░░

Disk
  /              868.3 GB / 926.4 GB ( 93.7%)  ███████████░  SSD

Network
  en0            ↓    2.4 KB/s  ↑     541 B/s   total ↓39.2 GB ↑82.2 GB

Battery
  100.0%  ████████████  full
  213 cycles  ·  0.0 W

Temperatures   (simulated — real SMC reading not implemented on this backend yet)
  CPU CPU Package    42°C  █████░░░░░░░
  ...

Fans
  CPU Fan         1410 RPM   45%  █████░░░░░░░
```

Add `--json` to any command for machine-readable output (handy for Raycast,
Stream Deck, Hammerspoon, Home Assistant, …).

See [`docs/CLI.md`](./docs/CLI.md) for the full command reference.

---

## Architecture in one picture

```text
   CLI · TUI · GUI · HTTP API        ← presentation, portable
            │
            ▼
        peterfan-core                ← domain types, curves, profiles
            │   (knows nothing about any OS)
            ▼
     HardwareProvider  (trait)       ← the single seam
            ▲
            │ implemented by
   ┌────────┴─────────┬──────────────┐
  mock              macOS          Windows (planned)
                  (sysctl / SMC)   (EC / WMI)
```

The core depends **only** on the `HardwareProvider` trait. Each platform
provides one implementation. Adding Linux later means adding one backend — not
touching the core. Full details in [`docs/ARCHITECTURE.md`](./docs/ARCHITECTURE.md).

## Project layout

```text
peterfan/
├── packages/
│   ├── core/        peterfan-core      — OS-agnostic types, curves, profiles, trait
│   ├── platform/    peterfan-platform  — mock + macOS backends (Windows/Linux planned)
│   ├── cli/         peterfan           — the command-line interface
│   ├── tui/         peterfan-tui       — ratatui live dashboard
│   └── menubar/     peterfan-menubar   — macOS menu-bar / Windows tray app
├── apps/
│   └── landing/     static marketing website (open apps/landing/index.html)
├── docs/            architecture, roadmap, CLI reference
└── (planned) packages/daemon, apps/desktop
```

## Safety

Fan control is hardware-level and can be dangerous if done carelessly. PeterFan's
design commits to:

- **Capabilities up front** — backends advertise what they can do; the UI never
  offers control it can't safely perform.
- **Read-only first** — monitoring works without elevated privileges; control is
  a deliberate, separate step.
- **Restore on exit** — the (planned) daemon hands control back to the OS on
  crash or shutdown, and ramps fans up on critical temperatures.

## Contributing

This is a young project and a great time to get involved. See
[`CONTRIBUTING.md`](./CONTRIBUTING.md). The most valuable early contributions are
**new platform backends** (real SMC reading on macOS, an EC/WMI backend on
Windows) behind the existing `HardwareProvider` trait.

## License

[MIT](./LICENSE) © PeterFan contributors.
