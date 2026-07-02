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
| `--watch` | Continuously refresh the command until interrupted (Ctrl-C). Works with any command, not just `watch`. |
| `--interval <secs>` | Refresh interval for `--watch` (default: from config, or 2). |
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
peterfan status --compact   # one-line summary for shell prompts/status bars
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

**Custom curves** — define your own temp→duty curve and use it like a
built-in profile (including in `rule add`):

```bash
peterfan profile create custom --points "30:20,60:50,80:90,90:100"
peterfan profile create work --points "30:10,55:30,75:70,88:100"  # named curve
peterfan profile delete work
peterfan profile list             # shows built-ins + your custom curves
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

설정 파일 경로와 현재 값 표시. `--init`으로 기본 파일 생성, `--set`/`--get`으로 단일 값 편집.

```bash
peterfan config                        # 경로 + 현재 값 출력
peterfan config --init                 # 기본값으로 config 파일 생성
peterfan config --set profile gaming   # profile 값 변경
peterfan config --set interval 3       # 갱신 주기 변경
peterfan config --set critical 95      # 임계 온도 변경
peterfan config --get profile          # 특정 값만 출력
```

The config (`profile`, `interval_secs`, `critical_temp_c`) supplies defaults for
the `peterfand` daemon.

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

### `watch` — 라이브 단일 줄 모니터링

CPU%, MEM%, 온도, RPM, 전력, 데몬 모드를 한 줄에 색상으로 실시간 표시.
Ctrl-C로 종료. tmux 상태바, 빠른 현황 확인에 적합.

```bash
peterfan watch              # 2초마다 갱신 (기본값)
peterfan watch -i 1         # 1초마다 갱신
peterfan watch --interval 5
```

### `update` — 버전 확인

GitHub 최신 릴리즈와 현재 버전을 비교해 업데이트 여부를 안내.

```bash
peterfan update
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

### `fan` — fan control (macOS)

```bash
peterfan fan status                  # current control mode + live RPM
peterfan fan set 80                  # force all fans to ~80% duty
sudo peterfan fan set 80 --fan 0     # direct SMC write (no daemon)
peterfan fan auto                    # restore OS-managed control
```

When `peterfand` is installed and running, `peterfan fan set N` routes through
the daemon IPC — **no `sudo` required**. The daemon re-asserts every tick so
the setting persists until `peterfan fan auto`.

Without a running daemon the CLI falls back to a direct SMC write (needs
`sudo`). On **Apple Silicon** that write is process-scoped — it reverts when
the command exits. Install the daemon for persistent control.

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

### `login-item` — start the menu-bar app at login (macOS)

Installs a per-user LaunchAgent for `peterfan-menubar` — unlike
`install-daemon`, this never needs an admin password (it's your own login
item, not a system-wide service).

```bash
peterfan login-item status                       # installed? which binary?
peterfan login-item install                      # auto-start at login, menu bar shows CPU%
peterfan login-item install --metric temp        # shows temperature instead
peterfan login-item install --binary /path/to/peterfan-menubar
peterfan login-item remove
```

### `doctor`

Diagnoses the active backend, its capabilities, and whether the process is
running elevated. Start here when something looks off.

```bash
peterfan doctor
peterfan --json doctor
```

### `rule` — 자동화 규칙 관리

조건 기반으로 팬 프로파일을 자동 전환. 데몬이 매 틱마다 첫 번째 매칭 규칙을 적용.

```bash
peterfan rule                                      # 현재 규칙 목록
peterfan rule add on_battery silent                # <조건> <프로파일> — 위치 인자, 플래그 아님
peterfan rule add cpu_above:85 maximum
peterfan rule add time:22-7 silent
peterfan rule remove 0                             # 인덱스 0 규칙 삭제
peterfan rule clear                                # 전체 삭제
```

조건 형식: `on_ac`, `on_battery`, `cpu_above:<°C>`, `time:<시작>-<끝>`

### `daemon` — 데몬 관리

```bash
peterfan daemon status    # 현재 모드 + 백엔드 확인
peterfan daemon reload    # 설정 파일 다시 읽기
peterfan daemon stop      # 데몬 종료
peterfan daemon log       # 최근 로그 40줄 출력 (--lines/-n 으로 변경 가능)
peterfan daemon log --follow  # 로그 실시간 팔로우 (tail -f)
```

### `benchmark` — 스트레스 테스트

```bash
peterfan benchmark --secs 30              # 30초 전체 코어 부하
peterfan benchmark --profile gaming       # gaming 프로파일 적용 후 테스트 (종료 시 복원)
peterfan --json benchmark --secs 10
```

### `alert` — 임계값 초과 시 데스크탑 알림

CPU/메모리/온도가 지정한 임계값을 넘으면 알림(macOS `osascript`, Linux `notify-send`)을 보냄.

```bash
peterfan alert --cpu 90 --temp 95         # 인터벌마다 체크, 초과 시 알림
peterfan alert --memory 90                # 메모리 사용률 기준 (별칭: --mem)
peterfan alert --cpu 90 --save            # 임계값을 config에 저장 (이후 플래그 없이 재사용)
peterfan alert --once                     # 한 번만 체크하고 종료 (cron/스크립트용, 초과 시 exit 1)
peterfan alert install                    # 로그인 시 자동 실행되는 LaunchAgent 설치
peterfan alert status                     # LaunchAgent 설치 여부 확인
peterfan alert remove                     # LaunchAgent 제거
```

### `license` — 메뉴바 앱 라이선스

CLI/TUI/데몬의 팬 커브 로직 자체는 게이팅되지 않음 — 이 커맨드는 메뉴바 앱과
데몬의 상시 백그라운드 팬 제어에만 적용되는 14일 체험판/라이선스 상태를 다룸.

```bash
peterfan license                          # 체험판 잔여일 또는 라이선스 상태
peterfan license activate PFAN1-...       # 구매한 라이선스 키 등록
peterfan license deactivate               # 등록된 키 제거 (체험판 시계로 복귀)
peterfan --json license
```

## Scripting

`--json` makes every command pipeable. Example with `jq`:

```bash
# Hottest sensor right now
peterfan --mock --json temps | jq 'max_by(.value.0 // .value)'

# Current CPU-fan RPM
peterfan --mock --json fans | jq '.[] | select(.id=="fan.cpu") | .rpm'

# Daemon status in scripts
peterfan --json daemon status | jq -r '.mode'
```
