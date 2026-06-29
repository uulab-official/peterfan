# CLI reference

The binary is `peterfan`. During development, run it via Cargo:

```bash
cargo run -p peterfan-cli -- <command> [flags]
```

## Global flags

| Flag | Effect |
| --- | --- |
| `--mock` | Use the fully simulated backend instead of real hardware. |
| `--json` | Emit machine-readable JSON instead of formatted text. |
| `-h`, `--help` | Help. |
| `-V`, `--version` | Version. |

## Commands

### `status` (default)

Full dashboard: hardware summary, temperatures, fans, in one view. Running
`peterfan` with no subcommand is the same as `peterfan status`.

```bash
peterfan
peterfan --mock status
peterfan --json status
```

### `temps`

Temperature sensors with colored severity bars.

```bash
peterfan temps
peterfan --json temps     # array of {id,label,kind,value}
```

### `fans`

Fans with RPM and duty cycle.

```bash
peterfan fans
peterfan --json fans
```

### `hardware`

Detected machine: CPU, GPU, motherboard, memory, OS. On macOS this is **real**
(`sysctl`).

```bash
peterfan hardware
```

### `profile [name]`

With no name, lists the built-in profiles. With a name
(`silent`, `balanced`, `gaming`, `performance`, `maximum`), evaluates that
profile's curve at the current temperature and:

- **applies** it to controllable fans if the backend supports control, or
- **previews** the resulting duty if the backend is read-only.

```bash
peterfan profile                 # list
peterfan profile gaming          # preview (read-only backend) or apply
peterfan --mock profile maximum  # actually applies, on the mock machine
```

### `curve [name]`

Shows a profile's fan curve as a table, ASCII bars, and interpolated samples.
Defaults to `balanced`.

```bash
peterfan curve
peterfan curve performance
peterfan --json curve gaming     # array of {temp_c,duty_percent}
```

### `doctor`

Diagnoses the active backend, its capabilities, and whether the process is
running elevated. Start here when something looks off.

```bash
peterfan doctor
peterfan --json doctor
```

## Scripting

`--json` makes every command pipeable. Example with `jq`:

```bash
# Hottest sensor right now
peterfan --mock --json temps | jq 'max_by(.value.0 // .value)'

# Current CPU-fan RPM
peterfan --mock --json fans | jq '.[] | select(.id=="fan.cpu") | .rpm'
```

This is the same data the planned local HTTP API will expose, so scripts written
against `--json` will map cleanly onto `GET /api/v1/*` later.
