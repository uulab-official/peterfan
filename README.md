# PeterFan

> **Tiny fan control for developers.** A cross-platform fan controller and
> hardware monitor with a CLI, a TUI, and (soon) a desktop GUI — built in Rust.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](./LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.80%2B-orange.svg)](https://www.rust-lang.org)
![Status: pre-alpha](https://img.shields.io/badge/status-pre--alpha-red.svg)

PeterFan is **not** just a fan-speed slider. It aims to be a small, safe,
scriptable hardware-control platform for developers and power users — the kind
of tool you `brew install` next to `lazygit`, `btop`, and `mise`.

```text
Tiny · Simple · Beautiful · Safe · Extensible · Cross-platform
```

No ads. No bundleware. No vendor lock-in. MIT-licensed.

---

## Status

⚠️ **Pre-alpha — v0.1.0.** This is an early, honest foundation:

| Area | State |
| --- | --- |
| Core domain model (types, curves, profiles) | ✅ implemented & tested |
| `HardwareProvider` trait + platform layer | ✅ implemented |
| **Mock backend** (fully simulated machine) | ✅ implemented |
| **macOS** hardware info (CPU/RAM/OS via `sysctl`) | ✅ real, read-only |
| macOS temperature / fan reading (SMC) | 🚧 not yet — falls back to simulated |
| macOS / Windows fan **control** | 🚧 planned |
| CLI (`status`, `temps`, `fans`, `profile`, `curve`, `hardware`, `doctor`) | ✅ runnable |
| TUI dashboard (ratatui) | ✅ runnable |
| Desktop GUI (Tauri), daemon, plugins, HTTP API | 🗺️ roadmap |

When a backend can't read real sensors yet, the CLI/TUI **transparently fall
back to the mock backend and clearly label the data as `simulated`** — so you
always get a working demo, and we never pretend a reading is real when it isn't.

See [`docs/ROADMAP.md`](./docs/ROADMAP.md) for the full plan.

---

## Quick start

Requires a [Rust toolchain](https://rustup.rs) (1.80+).

```bash
# Build everything
cargo build

# Real hardware info on this machine
cargo run -p peterfan-cli -- hardware

# Diagnose the active backend & its capabilities
cargo run -p peterfan-cli -- doctor

# Full dashboard against the simulated machine
cargo run -p peterfan-cli -- --mock status

# Live terminal dashboard
cargo run -p peterfan-tui -- --mock
```

Once installed, the binary is simply `peterfan`.

### Example: `peterfan --mock status`

```text
PeterFan v0.1.0
backend: mock

Temperatures
  CPU CPU Package    54°C  ██████░░░░░░
  GPU GPU Core       48°C  █████░░░░░░░
  RAM Memory         41°C  ████░░░░░░░░
  SSD NVMe SSD       37°C  ████░░░░░░░░

Fans
  CPU Fan         1410 RPM   45%  █████░░░░░░░
  GPU Fan         1146 RPM   38%  █████░░░░░░░

Hardware · Mock CPU (8C/16T @ 4.5GHz)
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
│   └── tui/         peterfan-tui       — ratatui live dashboard
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
