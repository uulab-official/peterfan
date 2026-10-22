# Contributing to PeterFan

PeterFan welcomes reproducible bug reports, tests, documentation improvements,
and scoped code changes. Read the [development guide](./docs/DEVELOPMENT.md)
for environment setup and checks, the [architecture](./docs/ARCHITECTURE.md) for
module boundaries, and the [Code of Conduct](./CODE_OF_CONDUCT.md) for participation.
Report suspected vulnerabilities through [SECURITY.md](./SECURITY.md).

## Set up a source checkout

Use Rust 1.88 or newer through [rustup](https://rustup.rs), Node.js 22 or newer,
and, on macOS, Xcode Command Line Tools. Source development does not require
Apple signing credentials, a paid developer account, administrator access, or
installation of the fan-control daemon.

```bash
git clone https://github.com/uulab-official/peterfan.git
cd peterfan
cargo +stable build --locked -p peterfan-cli
cargo +stable run --locked -p peterfan-cli -- --mock status
```

The `--mock` example uses simulated hardware. Keep it in place when exploring
fan-control code; do not use real fan writes as a contribution prerequisite.

## Before opening a pull request

Run the contributor checks from the repository root:

```bash
scripts/check-contributor.sh
```

This runs locked dependency checks, formatting, Clippy, all-feature workspace
tests, documentation and open-source policy checks, Node checks, and Rust API
documentation generation. It does not launch the app, install a daemon, change
login items, or write to fans. It does create build outputs under `target/`.
See [DEVELOPMENT.md](./docs/DEVELOPMENT.md) for the exact commands and requirements.
Linux or headless contributors can run `scripts/check-contributor.sh --core-only`
for core tests/docs and policy checks; this does not replace native workspace QA.
On Windows, use Git Bash or the PowerShell equivalents in the development guide.

Additional gates are separate because they have different effects:

| Gate | Scope and effects |
| --- | --- |
| Release build | `cargo +stable build --locked --release --workspace`; produces local binaries. |
| Smoke tests | `scripts/smoke-test.sh` uses built binaries, launches short-lived mock app/daemon processes, and exercises local bundle packaging. Run in a desktop session; it is not a hardware validation. |
| Homebrew audit | `RUSTUP_TOOLCHAIN=stable scripts/check-homebrew.sh` requires Homebrew and uses a temporary audit tap; see the script before running. |
| Hardware and release QA | Follow [PRODUCTION_CHECKLIST.md](./docs/PRODUCTION_CHECKLIST.md). Administrator setup, real fan writes, signing, notarization, and publishing are deliberate, separate operations. |

In the PR, describe the problem, resulting behavior, verification performed,
and relevant limitations. Include reproduction steps and platform/model details
for hardware issues. Remove usernames, hostnames, serial numbers, IP addresses,
and credentials from attached logs. Local unit tests, mock UI checks, physical
hardware checks, and published release verification are distinct evidence.

## Architecture and implementation

**`peterfan-core` must not depend on a platform crate or OS-specific API.**
Hardware access belongs behind [`HardwareProvider`](./packages/core/src/provider.rs),
and general metrics behind `SystemMonitor`. Keep platform-specific logic under
`packages/platform`, advertise honest capabilities, and wire new providers into
`detect()` or `system_monitor()` as appropriate.

Current implementation already includes:

- macOS SMC/IOHID sensor reading and guarded fan control;
- real macOS and Windows system metrics, with Windows ACPI thermal zones when
  firmware exposes them; Windows CPU-core temperature and fan control are unsupported;
- TOML configuration loading/saving, profiles, notifications, and startup settings.

Useful contributions include model-specific sensor regression cases, safer
readback/recovery tests, accessible UI improvements, clearer diagnostics, and
reproducible platform coverage. Check existing issues before taking on a broad
backend or hardware-control change.

Match surrounding style, document public items, and add tests for behavior
rather than implementation details. Never present simulated data as real.
Unsupported or unavailable hardware must remain visible as such.

## Hardware safety

A hardware write requires a backend that advertises control support, validated
inputs, and a recovery path. Preserve readback verification, critical-temperature
handling, and restoration to OS automatic control. Cleanup cannot guarantee
recovery after power loss, forced termination, or every firmware failure; do not
add claims of unconditional crash safety.

Tests should use mocks unless a hardware test is explicitly requested and its
side effects are documented. Do not weaken privilege, signature, or update
verification to make development easier.

## License

Contributions are licensed under the project's [MIT License](./LICENSE).
