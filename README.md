# PeterFan

**English** | [한국어](./README.ko.md) | [日本語](./README.ja.md) | [中文](./README.zh.md)

> **The Mac fan controller and system monitor for developers.** A cross-platform
> fan controller and hardware monitor with a CLI, a TUI, and a macOS menu-bar
> app — built in Rust.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](./LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.80%2B-orange.svg)](https://www.rust-lang.org)
![Status: beta](https://img.shields.io/badge/status-beta-yellow.svg)

PeterFan is **not** just a fan-speed slider. It's a small, safe, scriptable
system monitor *and* fan-control platform for developers and power users — the
kind of tool you `brew install` next to `lazygit`, `btop`, and `mise`, with a
menu-bar app in the spirit of [iStat Menus](https://bjango.com/mac/istatmenus/)
and [Stats](https://github.com/exelban/stats): live sparkline graphs in the
menu bar, per-metric history charts, direct fan-speed control, and a
scriptable CLI/TUI underneath for people who'd rather pipe `--json` into
Raycast or a dashboard.

```text
Tiny · Simple · Beautiful · Safe · Extensible · Cross-platform
```

**CLI, TUI, and the fan-control daemon are free and MIT-licensed, forever.**
The menu-bar app has a 14-day free trial; after that it needs a one-time
license (`peterfan license activate <key>`) to keep the always-on menu-bar
widget and persistent background fan control — read-only commands never stop
working. See [Pricing](#pricing--licensing) below.

---

## Download for Mac — no Terminal needed

1. **[Download the latest `.dmg`](https://github.com/uulab-official/peterfan/releases/latest)**
   (under **Assets**, look for `PeterFan-vX.Y.Z.dmg`)
2. Double-click it to open, then drag **PeterFan.app** onto the
   **Applications** shortcut
3. Open **PeterFan** from Applications (or Spotlight) — on first launch,
   **right-click → Open** to confirm ([why?](#download))

That's it — PeterFan lives quietly in your menu bar. 14-day free trial,
no account or sign-up required. Prefer the command line, or need Windows?
See [Download](#download) below for `.tar.gz`/`.zip` archives and
build-from-source instructions.

---

## Status

**Beta — v1.23.0.** Actively developed; this table reflects what's actually shipped:

| Area | State |
| --- | --- |
| **System metrics** — CPU, memory, disk, network, processes | ✅ real, cross-platform (macOS + Windows) via `sysinfo` |
| **macOS memory breakdown** — wired / active / inactive / compressed | ✅ real via mach `host_statistics64` (verified against `vm_stat`) |
| **Battery** — charge, state, cycles, time remaining, **temperature** | ✅ real via `battery` + IOHID (health filtered on Apple Silicon) |
| Core model (types, metrics, curves, profiles, traits) | ✅ implemented & tested |
| Mock backends (fully simulated machine + metrics) | ✅ implemented |
| macOS hardware info (CPU/RAM/OS via `sysctl`) | ✅ real, read-only |
| **macOS temperatures & fan RPM** | ✅ real — CPU/GPU **die temps via IOHID**, fan RPM + ambient via SMC |
| Windows temperature / fan reading (EC) | 🚧 planned |
| GPU utilization | 🔬 investigated — IOReport plumbing works, but the residency it exposes doesn't match Activity Monitor's GPU %, so it's deferred rather than shipped inaccurate ([`docs/RESEARCH.md`](./docs/RESEARCH.md)) |
| Fan **control** | ⚙️ SMC writes, **needs root** (`sudo peterfan fan set N` or the daemon). `fan set` **verifies by reading RPM back** so you get a real ✓/✗, not a fake "ok". Confirmed on Intel; on Apple Silicon it's attempted and verified (some models' firmware may ignore it) |
| CLI — `status`/`cpu`/`memory`/`disk`/`network`/`top`/`battery`/`system`/`temps`/`fans`/`fan`/`profile`/`curve`/`hardware`/`doctor`/`config`/`serve`/`benchmark`/`log`/`alert`/`license`/`completions`, global `--watch` & `--json` | ✅ runnable |
| TUI system dashboard (ratatui) — CPU/mem/disk/net/battery/processes + temps/fans/power | ✅ runnable |
| **Menu-bar app** — sparkline graph icon (number/graph/both, your choice), hover tooltip with a quick summary, popover dashboard with 2m/1h/1d history charts (hover for exact value + avg/peak), **per-fan Auto/Manual control with an RPM slider bounded to that fan's real range**, profile/Auto/Rules control, quit-process from Top Processes, English/한국어, a separate resizable Detail Window, light/dark mode | ✅ runnable |
| **Daemon** (`peterfand`) — continuous curve + restore-on-exit + critical-temp override + IPC server; LaunchDaemon install | ✅ runnable |
| **Self-update** — menu-bar "Check for Updates…" (and `peterfan update`) checks GitHub Releases and installs in place | ✅ runnable |
| **Local HTTP API** (`peterfan serve`) — JSON metrics + control for integrations | ✅ runnable |
| Licensing — 14-day trial, Ed25519 offline-verified keys | ✅ implemented (menu-bar app + daemon fan control only) |
| Desktop GUI (Tauri), plugins | 🗺️ roadmap |

When a backend can't read real sensors yet, the CLI/TUI **transparently fall
back to the mock backend and clearly label the data as `simulated`** — so you
always get a working demo, and we never pretend a reading is real when it isn't.

See [`docs/ROADMAP.md`](./docs/ROADMAP.md) for the full plan.

---

## Pricing & licensing

- **CLI (`peterfan`), TUI (`peterfan-tui`), and the daemon's fan-control core
  are MIT-licensed and free forever** — script them, embed them, fork them.
- **The menu-bar app** (`peterfan-menubar` / `PeterFan.app`) is free to try for
  **14 days** from first launch. After the trial, running it (and the
  daemon's *persistent* background fan control) needs a license:
  ```sh
  peterfan license status              # trial days left / license status
  peterfan license activate <key>      # PFAN1-... key from your purchase
  ```
  Without a license past the trial, the menu-bar app keeps showing live
  metrics — only the always-on background widget and continuous fan control
  are gated; you can still drive fans manually via `sudo peterfan fan set N`.
- License keys are Ed25519-signed and verified fully offline (no phone-home,
  no server dependency). Buy a license: *(store link coming soon)*.

---

## Download

Prebuilt binaries are attached to each [GitHub Release](https://github.com/uulab-official/peterfan/releases/latest).
macOS (Apple Silicon + Intel, universal) and Windows builds are produced by CI
on every tagged release, in two forms:

| Asset | Contains | Best for |
| --- | --- | --- |
| `PeterFan-vX.Y.Z.dmg` | Just `PeterFan.app` + an Applications shortcut | Anyone who just wants the menu-bar app — double-click, drag, done |
| `peterfan-vX.Y.Z-universal-apple-darwin.tar.gz` | `peterfan` (CLI), `peterfan-tui`, `peterfan-menubar`, `peterfand`, **and** `PeterFan.app` | Developers / scripting / anyone who also wants the CLI or TUI |

```sh
# .dmg (menu-bar app only, no Terminal needed)
open PeterFan-*.dmg
# → drag PeterFan.app onto the Applications shortcut, then launch it normally

# .tar.gz (CLI + TUI + menu-bar app, for developers)
tar -xzf peterfan-*-universal-apple-darwin.tar.gz
cd peterfan-*-universal-apple-darwin
open PeterFan.app          # menu-bar app
./peterfan status          # …or use the CLI / TUI directly
```

Both are built the same way — the `.dmg` is just the `.app` from inside the
`.tar.gz`, repackaged as a normal disk image for people who don't want a
Terminal. Windows gets a `.zip` (CLI/TUI/menu-bar binaries only — no `.exe`
installer yet).

The app is ad-hoc signed (no paid Apple Developer account behind it, so it's
not notarized). First launch shows the standard "cannot verify developer"
prompt — right-click `PeterFan.app` → **Open**, or **System Settings → Privacy
& Security → Open Anyway**. If macOS still refuses with "is damaged and can't
be opened," clear the quarantine flag manually: `xattr -dr
com.apple.quarantine PeterFan.app peterfan*`.

---

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

---

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
PeterFan v1.23.0
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
  BATT Battery       31°C  ███░░░░░░░░░

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

---

## Project layout

```text
peterfan/
├── packages/
│   ├── core/        peterfan-core      — OS-agnostic types, curves, profiles, trait, licensing
│   ├── platform/    peterfan-platform  — mock + macOS backends (Windows/Linux planned)
│   ├── cli/         peterfan           — the command-line interface
│   ├── tui/         peterfan-tui       — ratatui live dashboard
│   ├── menubar/     peterfan-menubar   — macOS menu-bar / Windows tray app
│   └── daemon/      peterfand          — fan-control daemon (curve + safety)
├── tools/
│   ├── icongen/          generates the app icon PNG — dev-only, excluded from workspace
│   └── license-keygen/   issues license keys — dev-only, never shipped, excluded from workspace
├── apps/
│   └── landing/     static marketing website (open apps/landing/index.html)
├── packaging/       LaunchDaemon plist · Homebrew formula · scripts/ install helpers
├── docs/            architecture, roadmap, CLI reference, research notes
└── (planned) apps/desktop (Tauri GUI)
```

---

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

---

## Contributing

This is a young project and a great time to get involved. See
[`CONTRIBUTING.md`](./CONTRIBUTING.md). The most valuable early contributions are
**new platform backends** (real SMC reading on macOS, an EC/WMI backend on
Windows) behind the existing `HardwareProvider` trait.

---

## License

The code in this repository is [MIT](./LICENSE) © PeterFan contributors —
including the menu-bar app's source. What's *licensed as a product* is the
**right to run the menu-bar app's always-on background widget and persistent
fan control past the 14-day trial** (see [Pricing & licensing](#pricing--licensing)
above); the CLI, TUI, and daemon's fan-curve logic underneath have no such
restriction and are free to use, study, and modify under the MIT terms like
the rest of the project.
