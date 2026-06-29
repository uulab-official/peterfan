# Architecture

PeterFan is organized around one rule:

> **The core knows nothing about any operating system.**
> It knows only the `HardwareProvider` trait.

Everything else follows from that.

## Layers

```text
┌─────────────────────────────────────────────┐
│  Presentation:  CLI · TUI · GUI · HTTP API    │  portable, OS-unaware
├─────────────────────────────────────────────┤
│  Application / Core:  peterfan-core           │  types, curves, profiles
│      depends only on ▼                        │
│  Seam:  HardwareProvider (trait)              │  the single boundary
├─────────────────────────────────────────────┤
│  Platform backends:  mock · macOS · Windows   │  implement the trait
├─────────────────────────────────────────────┤
│  OS / hardware:  SMC · EC · sysfs · sensors   │
└─────────────────────────────────────────────┘
```

Dependencies point **downward and inward only**. `peterfan-core` must never
`use` a platform crate; doing so would break portability and is the one
architectural invariant to defend in review.

## Crates

| Crate | Path | Responsibility |
| --- | --- | --- |
| `peterfan-core` | `packages/core` | Domain types, system `metrics`, fan `curve`s, `profile`s, and the two backend traits (`HardwareProvider`, `SystemMonitor`). Pure, no OS deps. Unit-tested. |
| `peterfan-platform` | `packages/platform` | Backend implementations: `system` (real cross-platform metrics via `sysinfo` + `battery`), `mock`/`mock_monitor` (simulated), `macos` (real read-only thermal info via `sysctl`). `detect()`/`system_monitor()` pick real backends; `mock()`/`mock_monitor()` force simulation. |
| `peterfan-cli` | `packages/cli` | The `peterfan` binary. Pure presentation over core + a provider. |
| `peterfan-tui` | `packages/tui` | `peterfan-tui` binary: a ratatui dashboard polling a provider. |

Planned (see [ROADMAP](./ROADMAP.md)): `packages/daemon` (privileged control
service + safety watchdog) and `apps/desktop` (Tauri + React GUI).

## Two seams: `SystemMonitor` and `HardwareProvider`

PeterFan has two backend traits because the data has two very different access
stories:

- **`SystemMonitor`** — general system metrics (CPU, memory, disk, network,
  processes, battery). These are available cross-platform through `sysinfo`/
  `battery` without any per-OS code or privileges, so the real backend
  (`SysinfoMonitor`) already works on macOS and Windows today.
- **`HardwareProvider`** — thermal hardware (temperatures, fans, control). This
  needs per-OS native access (SMC on macOS, EC on Windows) and is where the
  platform-specific work and the safety model live.

Both follow the same capability + mock-fallback philosophy below.

## The `HardwareProvider` trait

This is the heart of the thermal design. A backend reports:

- `name()` / `capabilities()` — what it is and what it can do **right now**;
- `hardware_info()` — static machine description;
- `temperatures()` / `fans()` — live readings;
- `set_fan_duty()` — control (defaults to `Unsupported`, so read-only backends
  are correct for free).

### Capabilities, not exceptions

Backends advertise `Capabilities { read_temps, read_fans, control_fans }` up
front. The UI uses this to stay honest:

- It disables controls a backend can't perform instead of failing on use.
- When `read_temps` is false (e.g. macOS today, where SMC reading isn't
  implemented), the CLI/TUI fall back to the **mock** backend for sensor values
  and label them `simulated`. Real `hardware_info()` is still shown, because
  that part *is* real.

This keeps the "everything works in a demo" experience without ever lying about
whether a number came from real silicon.

## Fan curves & profiles

- A [`FanCurve`](../packages/core/src/curve.rs) is a sorted list of
  `(temp, duty%)` points. `duty_at(temp)` does linear interpolation and clamps
  outside the defined range. This is exactly the model the GUI's drag-to-edit
  curve editor will produce.
- A [`Profile`](../packages/core/src/profile.rs) (Silent / Balanced / Gaming /
  Performance / Maximum / Custom) is a named preset that resolves to a default
  curve. Applying a profile evaluates its curve at the current temperature and
  drives controllable fans to that duty.

## Safety model

Fan control writes to hardware and can be dangerous. The design commits to:

1. **Read-only by default.** Monitoring needs no elevated privileges.
2. **Explicit, separate control.** Writes go through a backend that has
   declared `control_fans = true`.
3. **Restore on exit / crash.** The planned daemon owns active control and
   hands fans back to OS-default behavior on shutdown or crash, and force-ramps
   on critical temperatures. The CLI never leaves fans in a custom state it
   can't recover from.

## Why Rust

Memory safety for code that pokes at hardware, a first-class CLI ecosystem
(`clap`, `ratatui`), easy single-binary distribution, straightforward FFI to
each OS's native APIs, and a clean story for an eventual long-running daemon.
