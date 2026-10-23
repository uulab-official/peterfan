# Architecture

PeterFan separates portable domain logic from operating-system access. The core
knows the `HardwareProvider` and `SystemMonitor` traits, not their platform
implementations.

```text
CLI · TUI · menu-bar/tray UI · local HTTP API
                     │
            core models and policies
                     │
       HardwareProvider · SystemMonitor
                     │
      mock · macOS · Windows · system metrics
                     │
          OS APIs and supported hardware
```

`peterfan-core` must not import a platform crate or use OS-specific APIs.
Presentation crates may depend on both core and platform utilities. The diagram
shows responsibilities rather than a claim that every UI is OS-independent:
the native tray/window integration has platform-specific code.

## Crates

| Crate | Path | Responsibility |
| --- | --- | --- |
| `peterfan-core` | `packages/core` | Domain types, metrics, curves, profiles, configuration models, and backend traits. |
| `peterfan-platform` | `packages/platform` | Real and mock providers, system metrics, TOML persistence, daemon IPC/install support, startup integration, and verified update utilities. |
| `peterfan-cli` | `packages/cli` | The `peterfan` CLI and local HTTP API. |
| `peterfan-tui` | `packages/tui` | Terminal dashboard over system and hardware providers. |
| `peterfan-menubar` | `packages/menubar` | Native macOS menu bar / Windows tray, WebView dashboard, settings, and CPU-responsive runner rendering. |
| `peterfan-daemon` | `packages/daemon` | The `peterfand` privileged control loop, profiles, local IPC, safety checks, and automatic-control restoration. |

See [DEVELOPMENT.md](./DEVELOPMENT.md) for contributor commands and
[ROADMAP.md](./ROADMAP.md) for remaining work.

## Two backend seams

`SystemMonitor` provides general CPU, memory, disk, network, process, and battery
metrics. The real implementation uses `sysinfo` and `battery`, with platform
extensions where needed. Availability varies by operating system and machine.

`HardwareProvider` provides machine information, temperature sensors, fans, and
optional fan duty/automatic-mode writes. Capabilities describe the operations a
provider can support; each operation can still fail or return no sensors.
Callers must handle both errors and empty readings.

| Backend | Implemented behavior | Boundary |
| --- | --- | --- |
| macOS | Real SMC/IOHID temperature inventory, fan RPM, and guarded SMC fan writes. | Sensor availability and write acceptance depend on model, permissions, and firmware. Fanless Macs have no fans to control. |
| Windows | Real hardware/system information and firmware ACPI thermal zones when exposed through Windows. | Thermal zones are system temperatures, not CPU-core temperatures. Fan RPM and fan control are unsupported. |
| Mock | Simulated hardware, metrics, and control for demos/tests. | Must remain clearly identified as simulated; mock success is not hardware evidence. |

`detect()` selects the OS hardware provider; `system_monitor()` selects real
system metrics. Explicit `--mock` paths use simulated providers. Some callers
can fall back to labeled simulation when a real provider cannot initialize;
others show unsupported/unavailable readings. Do not assume every missing
sensor causes a mock fallback or substitute fabricated values in a real backend.

## Configuration and presentation

Configuration models live in core; `peterfan-platform::config` loads and saves
TOML. The CLI and app share settings for profiles, startup, notifications, and
menu-bar presentation. Configuration changes are distinct from installing a
privileged helper. Monitoring and source builds do not require that helper.

The menu-bar UI combines native window/tray integration with an HTML dashboard.
Runner rendering and cadence follow CPU load; fixture-based browser checks can
verify layout and interaction logic, while native runtime and performance
checks require their own evidence.

## Fan curves, control, and recovery

A [`FanCurve`](../packages/core/src/curve.rs) is a sorted set of temperature/duty
points, evaluated with interpolation and endpoint clamping. A
[`Profile`](../packages/core/src/profile.rs) resolves a named preset or custom
curve to requested fan duties.

Monitoring is read-only by default. Persistent macOS control uses a separately
installed root daemon, reached over local IPC. Hardware writes require supported
capabilities and validated requests; callers must preserve RPM readback and
report rejected or unverified writes honestly.

The daemon handles critical temperatures, distinguishes RPM read errors from
stalled fans, and limits repeated recovery attempts with cooldown behavior.
It attempts to restore OS automatic control on supported shutdown/error paths.
This is not an unconditional guarantee after power loss, forced termination,
or firmware failure. Unit tests using mock providers do not establish real
machine thermal safety. See the [production checklist](./PRODUCTION_CHECKLIST.md)
for hardware and release validation gates.

## Update and trust boundary

Update checking and installation are explicit actions. Version precedence,
release metadata, asset verification, code signature/notarization checks, and
replacement/relaunch each have distinct failure cases. A successful download
or a newer version string is not proof that installation succeeded. Daemon
installation also distinguishes the executable on disk from the version of the
process answering IPC.

Security reports belong in [SECURITY.md](../SECURITY.md). Contribution workflow
and participation expectations are in [CONTRIBUTING.md](../CONTRIBUTING.md) and
[CODE_OF_CONDUCT.md](../CODE_OF_CONDUCT.md).
