# Contributing to PeterFan

Thanks for your interest! PeterFan is young, which means high-leverage
contributions are still on the table.

## Getting set up

```bash
git clone <your-fork>
cd peterfan
cargo build
cargo test
cargo run -p peterfan-cli -- --mock status
```

Requires Rust 1.80+ ([rustup](https://rustup.rs)).

## Before you open a PR

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

CI will run these; running them locally first saves a round-trip.

## The one architectural rule

**`peterfan-core` must not depend on any platform crate or OS-specific API.**

The core talks to hardware exclusively through the
[`HardwareProvider`](./packages/core/src/provider.rs) trait. If you find
yourself wanting `#[cfg(target_os = ...)]` inside `packages/core`, that logic
belongs in a backend under `packages/platform` instead. See
[`docs/ARCHITECTURE.md`](./docs/ARCHITECTURE.md).

## Where help is most valuable

1. **Real macOS SMC backend.** `packages/platform/src/macos.rs` currently
   returns real `hardware_info()` but `Unsupported` for temps/fans. Implementing
   genuine SMC reading (IOKit / `AppleSMC`) is the single biggest unlock.
2. **A Windows backend.** A new module implementing `HardwareProvider` via EC /
   WMI / a LibreHardwareMonitor-style approach.
3. **Config & profiles.** TOML config loading for default profile, startup,
   notifications.

A new backend should:

- live in its own module under `packages/platform/src`,
- be gated with `#[cfg(target_os = "...")]`,
- advertise honest `Capabilities`,
- be wired into `detect()` in `packages/platform/src/lib.rs`.

## Coding conventions

- Match the surrounding style; keep modules small and documented.
- Public items get doc comments (`///`). Explain *why*, not just *what*.
- Add unit tests for pure logic (see `curve.rs`, `profile.rs` for the pattern).
- Never make a backend report a simulated value as if it were real — use the
  capability + mock-fallback mechanism instead.

## Safety

Anything that **writes** to hardware (fan control, SMC writes) must:

- only run on a backend that advertises `control_fans = true`,
- refuse unsafe duties,
- and have a restore-on-exit story.

When in doubt, open an issue to discuss the safety design before implementing.

## License

By contributing, you agree your contributions are licensed under the project's
[MIT License](./LICENSE).
