# Development guide

Start with [CONTRIBUTING](../CONTRIBUTING.md). The workspace contains six product
crates and commits `Cargo.lock` because it ships applications. Use `--locked`
when checking, testing and building the reviewed dependency graph.

## Prerequisites and supported checks

- Rust **1.88 or newer**. CI checks the minimum version and current stable;
  current stable is recommended for rustfmt/clippy. No global rustup setting is
  required: use `cargo +stable …` or `RUSTUP_TOOLCHAIN=stable` per invocation.
- Node.js **22 or newer** for dashboard, runner and repository checks. The
  validation scripts use Node built-ins and require no npm installation.
- Git and Bash (on Windows, Git Bash) for the shared contributor entry point.
- macOS: Xcode Command Line Tools (`xcode-select --install`). A local build can
  use ad-hoc signing; Apple Developer credentials are only for official releases.
- Windows: Visual Studio C++ Build Tools with a Windows SDK and MSVC Rust target.
  Running the tray UI requires the WebView2 runtime. Pure source checks do not
  install it or launch the tray.
- Linux: the portable core check below is supported. The complete desktop
  workspace is not a claimed Linux target; it needs additional GTK/WebKit/native
  integration work beyond installing Rust.

| Check | macOS | Windows | Linux/headless |
| --- | --- | --- | --- |
| Full workspace tests/checks | Native CI and local contributor gate | Native CI; shared Bash gate or PowerShell commands | Not claimed |
| Rust 1.88 minimum check | CI + locally verified | CI gate | Core only |
| Portable core tests | Included | Included | CI + `--core-only` |
| Actual fan behavior | Separate compatible-Mac verification | Current backend has no fan control | Not supported |

## Fresh-clone quick start

```bash
git clone https://github.com/uulab-official/peterfan.git
cd peterfan
cargo run --locked -p peterfan-cli -- --mock status
cargo run --locked -p peterfan-cli -- --mock --json doctor
scripts/check-contributor.sh
```

The mock commands do not need administrator privileges, signing material, a
running daemon or compatible thermal hardware. They explicitly label simulated
data. Source builds do not modify the already installed `/Applications` app.

The contributor gate checks tool versions, docs/local links, asset inventory,
release-source helper tests, WebView behavior/animation, formatting, clippy,
all-feature workspace tests and Rust API documentation. It creates build/QA
outputs under `target/`; it does not launch the app, install a daemon, write fan
targets or read signing credentials. Rust/Cargo may download missing crates. On Windows, nine macOS-only launcher
fixtures and three Ubuntu publication-script fixtures are explicitly skipped;
their checks run on macOS/Linux instead. Portable repository/Git tests still run.

For portable core contributions on Linux/headless hosts:

```bash
scripts/check-contributor.sh --core-only
```

This runs the same repository policy and formatting checks, then limits clippy,
tests and Rust documentation to `peterfan-core`. Passing it is not evidence that
the desktop app compiles or runs on Linux. Native checks regenerate runner sheets
and cadence evidence from Rust before comparing the JavaScript preview. Core-only
checks run frontend preview behavior with `--web-only` and explicitly omit that
native comparison; no ignored artifact from an earlier checkout is required.

PowerShell equivalents for full native Rust checks:

```powershell
node scripts/check-oss.mjs
node --test scripts/check-oss.test.mjs scripts/check-release-source.test.mjs scripts/check-dev-launcher.test.mjs
node scripts/check-dashboard-behavior.mjs
cargo test --locked -p peterfan-menubar render_runner_sprite_sheet_for_visual_qa -- --ignored
node scripts/check-runner-preview.mjs
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-features
cargo doc --locked --workspace --all-features --no-deps
```

Git Bash runs the complete shared entry point, including `scripts/check-docs.sh`.
For a minimum-toolchain check without changing your global default:

```bash
rustup toolchain install 1.88.0 --profile minimal
cargo +1.88.0 check --locked --workspace --all-targets --all-features
```

## Optional integration and maintainer gates

```bash
cargo build --locked --release --workspace
scripts/smoke-test.sh target/release
scripts/check-homebrew.sh
scripts/check-runner-quality.sh
scripts/check-sleep-display-regressions.sh
```

Read the scripts before running optional gates. The smoke suite launches mock
processes, packages app/DMG outputs on macOS and mounts/unmounts its test DMG.
Homebrew checks create/remove an owned temporary tap when Homebrew is present.
Runner quality checks write sheets/reports under `target/`. The deterministic
sleep/display checks do not put the Mac to sleep or prove physical mixed-DPI
behavior. None of these substitute for hardware fan-control testing.

`cargo audit --deny warnings` is a separate network-backed RustSec check in CI; install
`cargo-audit --locked` if running it locally. Weekly dependency PRs are proposed
by Dependabot; updates are reviewed and tested, not automatically merged.

App launch/replacement, privileged daemon setup, manual fan writes, release
signing/notarization, public publishing and real OTA each need their own
appropriate validation. Follow [macOS distribution](MACOS_DISTRIBUTION.md),
[Windows distribution](WINDOWS_DISTRIBUTION.md) and the
[production checklist](PRODUCTION_CHECKLIST.md).

## Reviewing changes

Keep core code OS-independent. Use the mock providers for deterministic logic
tests. For UI changes compare the same size/theme/data before and after. For
hardware work record exact model, source/build version and what was actually
observed, separating read-only probes from real fan writes and fail-safe checks.
Never attach `.env`, signing material or unredacted machine logs to a PR.

Before adding external artwork update [third-party notices](../THIRD_PARTY_NOTICES.md)
and [the asset inventory](../assets/runner/sources.json). Use
[SECURITY](../SECURITY.md) for vulnerabilities and [SUPPORT](../SUPPORT.md) for
ordinary reports.

## Release workflow trust boundary

Manual release dispatches accept a canonical version tag, require its commit to
be in `origin/main` history and pass an immutable SHA to build/package jobs.
Only artifact publication jobs receive repository write permission; those jobs
recheck the tag before publication and do not check out selected source code.
Ordinary pull requests do not receive Apple signing credentials.

Main ancestry is not proof of human review. Repository branch/ruleset protection
is a separate provider setting; the branch-protection endpoint reported no
protection for `main` during the 2026-10-22 audit. Maintainers should apply review
and release-environment rules appropriate to their team. This change does not
claim to protect against a compromised trusted main branch or eliminate the
small tag check-to-publication race.

A main-ref guard also rejects accidental tagged dispatch from other workflow
branches. An attacker who can edit and execute a workflow can remove its own
YAML guards; protecting signing material requires provider-enforced environment
branch/reviewer restrictions and appropriately scoped environment secrets.
Those account/security settings were not changed by this source-level work.
