# PeterFan

**English** | [한국어](./README.ko.md) | [日本語](./README.ja.md) | [中文](./README.zh.md)

PeterFan is a Rust-based macOS fan controller and system monitor for people who
want both a polished menu-bar app and scriptable command-line tools.

It combines:

- a macOS menu-bar monitor with live charts and fan controls
- a CLI for automation, JSON output, diagnostics, and scripting
- a TUI dashboard for terminal-first workflows
- a small privileged daemon for persistent fan curves
- a local HTTP API for integrations

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](./LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.80%2B-orange.svg)](https://www.rust-lang.org)
![Status: beta](https://img.shields.io/badge/status-beta-yellow.svg)

> Status: beta. PeterFan is useful today, but fan-control behavior depends on
> Mac model and firmware. Monitoring is read-only by default; fan writes require
> an explicit administrator setup step.

## Features

| Area | Status |
| --- | --- |
| macOS menu-bar app | Live menu-bar sparkline, popover dashboard, detail window, light/dark mode |
| CLI | `status`, `cpu`, `memory`, `disk`, `network`, `top`, `battery`, `temps`, `fans`, `fan`, `doctor`, `serve`, `update`, `license`, and more |
| TUI | Terminal dashboard built with ratatui |
| System metrics | CPU, memory, disks, network, processes, battery |
| macOS sensors | CPU/GPU die temperature, SSD and battery temperature, fan RPM, SMC-backed readings |
| Fan control | Manual fan setting, profiles, editable curves, daemon-driven persistent control |
| Safety | Capability checks, RPM verification, restore-on-exit, critical-temperature override |
| Automation | JSON output, local HTTP API, shell completions |
| Updates | GitHub Release checks from CLI and menu-bar app |
| Windows | Basic system metrics; fan/sensor control is planned |

When PeterFan cannot read a real sensor, it labels data as simulated rather than
pretending the reading is real. See [docs/ROADMAP.md](./docs/ROADMAP.md) and
[docs/RESEARCH.md](./docs/RESEARCH.md) for implementation notes.

## Screens and Interfaces

PeterFan ships as multiple interfaces over the same core:

- `PeterFan.app`: menu-bar app for macOS
- `peterfan`: command-line interface
- `peterfan-tui`: live terminal dashboard
- `peterfand`: root helper daemon used for persistent fan control
- `peterfan serve`: local JSON HTTP API

The CLI, TUI, core libraries, and daemon fan-control core are MIT-licensed. The
menu-bar app source is also in this repository, but running the always-on
menu-bar product and persistent background fan control after the trial requires
a license key. See [Licensing](#licensing).

## Install

Prebuilt release artifacts live on
[GitHub Releases](https://github.com/uulab-official/peterfan/releases).

| Asset | Platform | Contents |
| --- | --- | --- |
| `PeterFan-vX.Y.Z.dmg` | macOS | `PeterFan.app` and an Applications shortcut |
| `peterfan-vX.Y.Z-universal-apple-darwin.tar.gz` | macOS | CLI, TUI, daemon, menu-bar binary, and app bundle |
| `peterfan-vX.Y.Z-x86_64-pc-windows-msvc.zip` | Windows | CLI/TUI/tray binaries where available |

For macOS, a properly published DMG should be Developer ID signed, notarized,
and stapled. You can verify a downloaded DMG before installing:

```bash
spctl -a -vv -t open --context context:primary-signature PeterFan-vX.Y.Z.dmg
```

Expected result:

```text
accepted
source=Notarized Developer ID
```

If a release asset is rejected by Gatekeeper, prefer building from source or use
a newer signed release. Maintainers can verify release artifacts with
[scripts/check-macos-release.sh](./scripts/check-macos-release.sh).

## Quick Start

Build from source:

```bash
cargo build --release --workspace
```

Run the CLI:

```bash
target/release/peterfan status
target/release/peterfan doctor
target/release/peterfan fans
target/release/peterfan update
target/release/peterfan --json status
```

Run the TUI:

```bash
target/release/peterfan-tui
```

Run the macOS menu-bar app from source:

```bash
target/release/peterfan-menubar
```

Build a local macOS app bundle:

```bash
scripts/bundle-macos.sh target/release/peterfan-menubar dist
open dist/PeterFan.app
```

## Fan Control Setup

Reading metrics does not require administrator privileges. Writing fan speeds
does. For persistent fan control, install the daemon once:

```bash
target/release/peterfan install-daemon
target/release/peterfan doctor
```

After setup, menu-bar controls and CLI fan commands route through the daemon:

```bash
target/release/peterfan fan status
target/release/peterfan fan set 55
target/release/peterfan profile set gaming
```

Remove the daemon:

```bash
target/release/peterfan uninstall-daemon
```

Fan control is hardware-level. PeterFan verifies writes by reading RPM back and
restores OS control on daemon exit, but you should still use conservative curves
and keep critical-temperature protection enabled.

## Example Output

```text
PeterFan doctor
  Version:         1.x
  OS / arch:       macos / aarch64
  Metrics backend: sysinfo
  Thermal backend: macos

System metrics
  ok cpu
  ok memory
  ok disks
  ok networks
  ok processes
  ok battery

Thermal hardware
  ok read temperatures
  ok read fans
  ok control fans

Fan control readiness
  ok peterfand daemon reachable
  ok fully ready - daemon is running
```

Use `--json` with most commands when integrating with Raycast, Hammerspoon,
Stream Deck, dashboards, or scripts.

## Build Requirements

- Rust 1.80 or newer
- macOS 11+ for the app bundle and SMC backend
- Xcode Command Line Tools for signing, notarization, and DMG validation
- `jq`, `gh`, and Apple Developer credentials only for official release builds

Useful development commands:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
scripts/smoke-test.sh target/release
```

Check for updates:

```bash
peterfan update
peterfan update --open
peterfan update --install   # macOS app OTA, when running from PeterFan.app
```

## Release Builds

Official macOS release builds are created locally so Apple signing material does
not need to live in GitHub Actions secrets.

One-time setup on a release Mac:

```bash
cp .env.example .env
scripts/setup-macos-signing.sh teams
scripts/setup-macos-signing.sh csr
scripts/setup-macos-signing.sh import /path/to/developerID_application.cer
scripts/setup-macos-signing.sh notary
```

Build, sign, notarize, staple, checksum, and upload a tagged release:

```bash
scripts/release-local-macos.sh vX.Y.Z --draft
```

Verify an artifact:

```bash
scripts/check-macos-release.sh /path/to/PeterFan-vX.Y.Z.dmg
```

See [docs/MACOS_DISTRIBUTION.md](./docs/MACOS_DISTRIBUTION.md) for the full
release-machine model, including which files are public and which stay local in
Keychain, `.env`, and `private/`.

## Project Layout

```text
peterfan/
├── packages/
│   ├── core/        OS-agnostic types, curves, profiles, licensing
│   ├── platform/    mock and platform hardware backends
│   ├── cli/         peterfan command-line app
│   ├── tui/         terminal dashboard
│   ├── menubar/     macOS menu-bar / Windows tray app
│   └── daemon/      fan-control daemon
├── packaging/       launchd plists and packaging support
├── scripts/         build, install, signing, notarization, release helpers
├── docs/            architecture, roadmap, CLI, distribution notes
├── tools/           development-only utilities
└── apps/            supporting apps and experiments
```

Architecture details are in [docs/ARCHITECTURE.md](./docs/ARCHITECTURE.md).
CLI details are in [docs/CLI.md](./docs/CLI.md).

## Safety Model

PeterFan is designed around a read-first, control-second model:

- sensor reads work without elevation
- fan writes require explicit admin setup
- backends declare capabilities before UI controls are shown
- manual writes are verified by reading RPM back
- daemon fan control restores OS defaults on exit
- critical-temperature protection overrides custom curves

Some Apple Silicon Macs may ignore specific SMC fan-control writes. In those
cases PeterFan reports the failed verification instead of claiming success.

## Licensing

The repository is MIT-licensed. The CLI, TUI, core crates, and daemon
fan-control logic are free to use, fork, and modify under MIT.

The menu-bar app includes a 14-day trial. After the trial, continuing to run the
always-on menu-bar product and persistent background fan control requires a
license key:

```bash
peterfan license status
peterfan license activate PFAN1-...
```

License keys are Ed25519-signed and verified offline. Read-only CLI/TUI usage
does not phone home and does not require an account.

## Contributing

Contributions are welcome. Good first areas:

- new platform sensor backends
- Windows EC/WMI fan and temperature work
- UI polish for the menu-bar dashboard
- additional smoke tests and release validation
- documentation improvements

Start with [CONTRIBUTING.md](./CONTRIBUTING.md), then run:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## License

MIT. See [LICENSE](./LICENSE).
