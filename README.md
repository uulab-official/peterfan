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

⚠️ **Pre-alpha — v0.27.1.** This is an early, honest foundation:

| Area | State |
| --- | --- |
| **System metrics** — CPU, memory, disk, network, processes | ✅ real, cross-platform (macOS + Windows) via `sysinfo` |
| **macOS memory breakdown** — wired / active / inactive / compressed | ✅ real via mach `host_statistics64` (verified against `vm_stat`) |
| **Battery** — charge, state, cycles, time remaining | ✅ real via `battery` (health filtered on Apple Silicon) |
| Core model (types, metrics, curves, profiles, traits) | ✅ implemented & tested |
| Mock backends (fully simulated machine + metrics) | ✅ implemented |
| macOS hardware info (CPU/RAM/OS via `sysctl`) | ✅ real, read-only |
| **macOS temperatures & fan RPM** | ✅ real — CPU/GPU **die temps via IOHID**, fan RPM + ambient via SMC |
| Windows temperature / fan reading (EC) | 🚧 planned |
| GPU utilization | 🔬 investigated — IOReport plumbing works, but the residency it exposes doesn't match Activity Monitor's GPU %, so it's deferred rather than shipped inaccurate ([`docs/RESEARCH.md`](./docs/RESEARCH.md)) |
| Fan **control** | ⚙️ SMC writes, **needs root** (`sudo peterfan fan set N` or the daemon). `fan set` **verifies by reading RPM back** so you get a real ✓/✗, not a fake "ok". Confirmed on Intel; on Apple Silicon it's attempted and verified (some models' firmware may ignore it) |
| CLI — `status`/`cpu`/`memory`/`disk`/`network`/`top`/`battery`/`system`/`temps`/`fans`/`fan`/`profile`/`curve`/`hardware`/`doctor`/`config`/`serve`/`benchmark`/`log`/`completions`, global `--watch` & `--json` | ✅ runnable |
| TUI system dashboard (ratatui) — CPU/mem/disk/net/battery/processes + temps/fans/power | ✅ runnable |
| **Menu-bar app** — popover dashboard + **profile/Auto control buttons** that drive the daemon over IPC (no sudo) | ✅ runnable |
| **Daemon** (`peterfand`) — continuous curve + restore-on-exit + critical-temp override + IPC server; LaunchDaemon install | ✅ runnable |
| **Local HTTP API** (`peterfan serve`) — JSON metrics + control for integrations | ✅ runnable |
| Desktop GUI (Tauri), plugins | 🗺️ roadmap |

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

## Enable fan control (one-time)

Fan control writes to the SMC, which **requires root** — exactly like Macs Fan
Control or TG Pro. Rather than typing `sudo` every time, install the small root
helper once (you'll get **one macOS password prompt**, no Terminal sudo):

```sh
./peterfan install-daemon      # one GUI admin prompt; runs at every boot
./peterfan doctor              # confirms: root helper reachable, SMC keys present
```

After that the menu-bar buttons and `peterfan fan …` drive the fans through the
root helper — no further prompts. Remove it with `peterfan uninstall-daemon`.
`peterfan fan set N` **verifies by reading RPM back**, so you get a real ✓/✗.

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
PeterFan v0.25.1
backend: sysinfo + macos  ·  Darwin 26.1  ·  up 5d 7h 8m

CPU · Apple M3 Max
   21.6%  ███░░░░░░░░░   cores ▄▃▂▂▂▂▂▂▂ ▁▁ ▁

Memory
  27.4 GB / 36.0 GB ( 76.1%)  █████████░░░
  wired 5.6 GB  ·  active 7.6 GB  ·  compressed 13.4 GB

Disk
  /              896.7 GB / 926.4 GB ( 96.8%)  ████████████  SSD

Network
  en0            ↓    4.2 MB/s  ↑   53.4 KB/s   172.20.248.39  ·  total ↓50.0 GB ↑109.0 GB

Battery
   72.0%  █████████░░░  charging  ~1h 7m to full
  214 cycles  ·  41.8 W

Temperatures
  CPU CPU            58°C  ███████░░░░░   (real die temp via IOHID)
  CPU CPU hottest    60°C  ███████░░░░░
  SSD SSD            36°C  ████░░░░░░░░

Fans
  Fan 1           2445 RPM    3%  ░░░░░░░░░░░░
  Fan 2           2635 RPM    3%  ░░░░░░░░░░░░

Power · 21.2 W
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
│   ├── menubar/     peterfan-menubar   — macOS menu-bar / Windows tray app
│   └── daemon/      peterfand          — fan-control daemon (curve + safety)
├── apps/
│   └── landing/     static marketing website (open apps/landing/index.html)
├── packaging/       LaunchDaemon plist · scripts/ install helpers
├── docs/            architecture, roadmap, CLI reference, research notes
└── (planned) apps/desktop (Tauri GUI)
```

## Safety

Fan control is hardware-level and can be dangerous if done carelessly. PeterFan's
design commits to:

- **Capabilities up front** — backends advertise what they can do; the UI never
  offers control it can't safely perform.
- **Read-only first** — monitoring works without elevated privileges; control is
  a deliberate, separate step.
- **Restore on exit** — the `peterfand` daemon hands control back to the OS on
  Ctrl-C / SIGTERM / panic, and forces fans to 100% above a critical
  temperature.

## Contributing

This is a young project and a great time to get involved. See
[`CONTRIBUTING.md`](./CONTRIBUTING.md). The most valuable early contributions are
**new platform backends** (real SMC reading on macOS, an EC/WMI backend on
Windows) behind the existing `HardwareProvider` trait.

## License

[MIT](./LICENSE) © PeterFan contributors.
