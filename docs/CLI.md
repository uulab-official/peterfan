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
| `--watch` | Re-run the command on an interval, clearing the screen (Ctrl-C to stop). |
| `--interval <secs>` | Refresh interval for `--watch` (default: from config, else 2). |
| `-h`, `--help` | Help. |
| `-V`, `--version` | Version. |

## Commands

PeterFan groups commands into **system metrics** (real, cross-platform) and
**thermal hardware** (temps/fans/profiles).

### `status` (default)

Full dashboard: system info, CPU, memory, disk, network, battery, plus
temperatures and fans, in one view. Running `peterfan` with no subcommand is the
same as `peterfan status`.

```bash
peterfan
peterfan --mock status
peterfan --json status
```

### System metrics

| Command | Aliases | Shows |
| --- | --- | --- |
| `cpu` | | Aggregate + per-core usage, frequency, load average |
| `memory` | `mem` | Physical and swap usage |
| `disk` | `disks` | Mounted volumes: capacity and usage |
| `network` | `net` | Per-interface throughput (↓/↑) and totals |
| `top` | `proc` | Top processes; `--mem` to rank by memory, `-n N` to set count |
| `battery` | | Charge, state, cycles, time remaining |
| `system` | | Host, OS, kernel, arch, cores, uptime |

```bash
peterfan cpu
peterfan top --mem -n 5
peterfan --json network | jq '.[] | select(.name=="en0") | .rx_rate'
```

These are sampled twice ~300 ms apart so usage percentages and network rates are
accurate.

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

### `fan` — control fans (macOS)

Force fan speed or restore automatic control. **Requires `sudo`** (SMC writes
are privileged); without it you get a clear permission error. Forced control
persists until `fan auto` or a reboot.

```bash
sudo peterfan fan set 60          # force all controllable fans to 60%
sudo peterfan fan set 100 --fan 0 # force only fan 0 to 100%
sudo peterfan fan auto            # restore OS-managed control
```

Duty is mapped onto each fan's real `[min, max]` RPM range and clamped.

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

### `config`

Show the config file path and current values; `--init` writes a default file.

```bash
peterfan config          # show path + values
peterfan config --init   # create ~/.config/peterfan/config.toml
```

The config (`profile`, `interval_secs`, `critical_temp_c`) supplies defaults for
`--watch` and the `peterfand` daemon.

**Automation rules** let the daemon switch profile by condition (first match
wins, else the base profile):

```toml
[[rules]]
when = "cpu_above:85"    # on_ac | on_battery | cpu_above:<°C> | time:<start>-<end>
profile = "maximum"
[[rules]]
when = "on_battery"
profile = "silent"
```

A manual choice over the daemon's IPC (or the menu-bar buttons) overrides the
rules until you send `rules` (or `auto`).

### `serve` — local HTTP API

Expose metrics (and fan control) as a localhost JSON API for integrations
(Stream Deck, Raycast, Hammerspoon, Home Assistant, scripts).

```bash
peterfan serve --port 9847        # then:
curl localhost:9847/api/v1/status
curl localhost:9847/api/v1/cpu
curl -X POST localhost:9847/api/v1/profile -d '{"name":"gaming"}'
curl -X POST localhost:9847/api/v1/fan -d '{"action":"set","percent":60}'
```

`GET /` serves a small human-readable index of the routes. Data endpoints:
`GET /api/v1/{status,system,cpu,memory,disks,network,battery,temps,fans,power,processes}`;
`POST /api/v1/profile` `{"name":"…"}`; `POST /api/v1/fan` `{"action":"auto"|"set","percent":N}`.
Responses are JSON with `Access-Control-Allow-Origin: *`. Control endpoints
apply only where fan control is available (Intel Macs).

### `benchmark` — stress test

Loads every CPU core for a fixed duration and records temperature, fan RPM, and
power over time — a quick way to see how the machine (and your fan curve)
behaves under sustained load.

```bash
peterfan benchmark --secs 30          # 30s all-core stress, live samples
peterfan --json benchmark --secs 10   # machine-readable samples
```

### `log` — continuous metrics stream

Emits one row of metrics per interval (time, CPU %, memory %, disk %, hottest
temp, max fan RPM, power) for recording or piping into other tools.

```bash
peterfan log --interval 2                 # CSV (with header) every 2s
peterfan log --interval 5 --format jsonl  # one JSON object per line
peterfan log >> metrics.csv               # append to a file
```

### `completions` — shell completion scripts

```bash
peterfan completions zsh  > ~/.zfunc/_peterfan
peterfan completions bash > /usr/local/etc/bash_completion.d/peterfan
# also: fish, powershell, elvish
```

### `install-daemon` / `uninstall-daemon` — one-time root setup (macOS)

Fan control needs root. Instead of per-command `sudo`, install a small root
LaunchDaemon once — `osascript` shows **one macOS password dialog** (no Terminal
sudo), then the helper runs at every boot and the menu-bar / `fan` commands drive
fans through it.

```bash
peterfan install-daemon            # one GUI admin prompt
peterfan install-daemon --dry-run  # print the exact privileged script first
peterfan uninstall-daemon          # remove it
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
