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
[![Open Source](https://img.shields.io/badge/open-source-MIT-blue.svg)](./LICENSE)

![PeterFan menu-bar dashboard and CLI diagnostics](./docs/images/peterfan-readme-overview.png)

> Status: beta. PeterFan is useful today, but fan-control behavior depends on
> Mac model and firmware. Monitoring is read-only by default; fan writes require
> an explicit administrator setup step.

## Features

| Area | Status |
| --- | --- |
| macOS menu-bar app | Live menu-bar sparkline, popover dashboard, detail window, light/dark mode |
| CLI | `status`, `cpu`, `memory`, `disk`, `network`, `top`, `battery`, `temps`, `temps --all`, `fans`, `fan`, `doctor`, `integrity`, `serve`, `update`, and more |
| TUI | Terminal dashboard built with ratatui |
| System metrics | CPU, memory, disks, network, processes, battery |
| macOS sensors | The menu-bar headline temperature defaults to CPU Core Average, while CPU Hottest, SMC summary/aggregate diagnostics, IOHID tdie, SSD and battery temperature, fan RPM, and the full SMC/IOHID inventory remain visible in the detailed lists |
| Fan control | Manual fan setting, profiles, editable curves, daemon-driven persistent control |
| Safety | Capability checks, RPM verification, restore-on-exit, critical-temperature override |
| Automation | JSON output, local HTTP API, shell completions |
| Updates | GitHub Release checks from CLI and menu-bar app |
| Integrity | Installed-app, GitHub release, offline local DMG, and complete release-directory verification for SHA-256, bundle id, Team ID, code signature, notarization, Gatekeeper, and bundled helper |
| Windows | Basic system metrics; fan/sensor control is planned |

## Repository Map

- `packages/cli`: CLI (`peterfan`)
- `packages/menubar`: macOS menu-bar app (`peterfan-menubar`)
- `packages/tui`: terminal UI (`peterfan-tui`)
- `packages/daemon`: root helper (`peterfand`)
- `packages/core`: shared models and metric calculations
- `packages/platform`: platform backends (`macos`, `mock`, updater utilities)
- `scripts/`: build, release, smoke-test, and signing helpers
- `docs/`: architecture notes, roadmap, release notes, and QA references

When PeterFan cannot read a real sensor, it labels data as simulated rather than
pretending the reading is real. See [docs/ROADMAP.md](./docs/ROADMAP.md) and
[docs/RESEARCH.md](./docs/RESEARCH.md) for implementation notes.

## Screens and Interfaces

The screenshot above shows the two surfaces PeterFan is built around: a quiet
menu-bar dashboard for daily use, and a scriptable `peterfan doctor` path for
debugging release, daemon, and hardware state.

![PeterFan popover visual QA: dark/light, English/Korean](./docs/images/peterfan-popover-qa.png)

The visual QA sheet above is regenerated from
`scripts/render-popover-qa.swift` and is meant to catch dark/light and
English/Korean text-fit regressions before a release.

PeterFan ships as multiple interfaces over the same core:

- `PeterFan.app`: menu-bar app for macOS
- `peterfan`: command-line interface
- `peterfan-tui`: live terminal dashboard
- `peterfand`: root helper daemon used for persistent fan control
- `peterfan serve`: local JSON HTTP API

PeterFan is MIT-licensed open source. The menu-bar app, CLI, TUI, and daemon
source live in this repository, and the app runs without account creation,
login, or a license key.

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

### One-line install (from source)

```bash
./script/build_and_run.sh --verify
```

That command builds `peterfan-menubar` and `peterfand`, assembles
`dist/PeterFan.app`, launches it without taking keyboard focus, and verifies
that exactly one app process is running. To install the result, move
`dist/PeterFan.app` to `/Applications`.

If you just want CLI tools:

```bash
cargo build --release -p peterfan-cli -p peterfan-tui
```

## Quick Start

Build from source:

```bash
cargo build --release --workspace
```

Run the CLI:

```bash
target/release/peterfan status
target/release/peterfan temps --all
target/release/peterfan doctor
target/release/peterfan integrity
target/release/peterfan integrity --latest
target/release/peterfan fans
target/release/peterfan update
target/release/peterfan --json status
```

`peterfan temps --all` prints every SMC `T*` and IOHID temperature sensor with
human-readable groups such as `CPU hotspot`, `CPU core hot sensor`, `GPU sensor`,
and `Battery sensor`, while preserving the original raw key for comparison.
The normal menu-bar headline temperature defaults to `CPU Core Average`;
`CPU Hottest` and every raw SMC/IOHID sensor remain visible in the full inventory.

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

If the menu-bar item is visible but its popover does not open, right-click it
and choose **Open Diagnostic Log…**. PeterFan keeps a bounded log at
`~/Library/Logs/PeterFan/menubar.log` with click routing, popover placement,
and WebView creation failures.

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

### Verify locally before publish

Run the workspace smoke test in CI-equivalent mode:

```bash
cargo build --workspace
./scripts/smoke-test.sh target/debug
```

This validates:

- CLI commands under timeout / JSON schema checks
- menu-bar startup/shutdown behavior (`--mock`)
- bundled `PeterFan.app` + `peterfand` presence
- DMG packaging path + mount checks (on macOS)

Useful development commands:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
scripts/smoke-test.sh target/release
```

Run the local menu-bar app from this checkout:

```bash
./script/build_and_run.sh          # build, bundle, and launch without stealing focus
./script/build_and_run.sh --verify # also verify exactly one PeterFan process is running
./script/build_and_run.sh --logs   # launch, then stream PeterFan logs
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

Maintainer reminder before every version bump:

- update `CHANGELOG.md`, `Cargo.toml`, and `Cargo.lock`
- refresh README screenshots when the menu-bar or diagnostics UI changes
- run `scripts/render-readme-overview.swift`
- run `scripts/render-popover-qa.swift`
- run `scripts/check-docs.sh`
- run `cargo fmt --check`, `cargo test --workspace`, and
  `cargo clippy --workspace --all-targets -- -D warnings`
- run `scripts/release-local-macos.sh vX.Y.Z` and install-test the DMG from
  `/Applications`

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

The entire repository, including the menu-bar app, CLI, TUI, core crates, and
daemon, is free to use, fork, modify, and redistribute under MIT.

No PeterFan feature requires an account, login, trial, or license key.
Fan-control installation still requires local macOS privileges and is handled
by the one-time setup flow shown inside the app.

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
