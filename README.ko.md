# PeterFan

[English](./README.md) | **한국어** | [日本語](./README.ja.md) | [中文](./README.zh.md)

> **개발자를 위한 Mac 팬 컨트롤러 & 시스템 모니터.** CLI, TUI, macOS 메뉴바 앱을 모두
> 갖춘 크로스플랫폼 팬 컨트롤러이자 하드웨어 모니터 — Rust로 만들었습니다.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](./LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.80%2B-orange.svg)](https://www.rust-lang.org)
![Status: beta](https://img.shields.io/badge/status-beta-yellow.svg)

PeterFan은 단순한 팬 속도 슬라이더가 아닙니다. 개발자와 파워유저를 위한 작고 안전하며
스크립트로 다룰 수 있는 시스템 모니터 *겸* 팬 제어 플랫폼입니다 — `lazygit`, `btop`,
`mise` 옆에 나란히 `brew install`해두는 그런 종류의 도구이면서, [iStat
Menus](https://bjango.com/mac/istatmenus/)나 [Stats](https://github.com/exelban/stats)
같은 정신을 이어받은 메뉴바 앱이기도 합니다: 메뉴바에 실시간으로 그려지는 스파크라인
그래프, 지표별 히스토리 차트, 팬 속도 직접 제어, 그리고 `--json`을 Raycast나 대시보드로
파이프하고 싶은 사람들을 위한 스크립트 가능한 CLI/TUI까지 밑단에 갖추고 있습니다.

```text
Tiny · Simple · Beautiful · Safe · Extensible · Cross-platform
```

**CLI, TUI, 팬 제어 데몬은 영원히 무료이며 MIT 라이선스입니다.**
메뉴바 앱은 14일 무료 체험을 제공하며, 이후에는 상시 구동되는 메뉴바 위젯과 지속적인
백그라운드 팬 제어를 계속 쓰려면 1회성 라이선스(`peterfan license activate <key>`)가
필요합니다 — 읽기 전용 명령어들은 언제나 그대로 동작합니다. 아래 [가격 정책](#pricing--licensing)을
참고하세요.

---

## Mac용 다운로드 — 터미널 필요 없음

1. **[최신 `.dmg` 다운로드](https://github.com/uulab-official/peterfan/releases/latest)**
   (**Assets** 항목에서 `PeterFan-vX.Y.Z.dmg`를 찾으세요)
2. 더블클릭해서 열고, **PeterFan.app**을 **Applications** 바로가기로 드래그하세요
3. Applications(또는 Spotlight)에서 **PeterFan**을 실행합니다 — 처음 실행할 때는
   **우클릭 → 열기**로 한 번 확인해줘야 합니다 ([이유는?](#download))

이게 끝입니다 — PeterFan은 이제 조용히 메뉴바에 자리잡습니다. 14일 무료 체험이며 계정
생성이나 가입 절차는 필요 없습니다. 커맨드라인을 선호하거나 Windows가 필요하다면 아래
[다운로드](#download) 섹션에서 `.tar.gz`/`.zip` 아카이브와 소스 빌드 방법을 확인하세요.

---

## 현재 상태

**베타 — v1.26.2.** 활발히 개발 중이며, 아래 표는 실제로 출시된 기능을 그대로 반영합니다:

| 영역 | 상태 |
| --- | --- |
| **시스템 지표** — CPU, 메모리, 디스크, 네트워크, 프로세스 | ✅ 실측치, `sysinfo` 기반 크로스플랫폼(macOS + Windows) |
| **macOS 메모리 세부 정보** — wired / active / inactive / compressed | ✅ mach `host_statistics64` 기반 실측치 (`vm_stat` 대비 검증 완료) |
| **배터리** — 충전량, 상태, 사이클, 남은 시간, **온도** | ✅ `battery` + IOHID 기반 실측치 (Apple Silicon에서는 health 값 필터링) |
| 코어 모델(타입, 지표, 커브, 프로파일, 트레이트) | ✅ 구현 및 테스트 완료 |
| Mock 백엔드(완전히 시뮬레이션된 머신 + 지표) | ✅ 구현 완료 |
| macOS 하드웨어 정보(`sysctl` 기반 CPU/RAM/OS) | ✅ 실측치, 읽기 전용 |
| **macOS 온도 & 팬 RPM** | ✅ 실측치 — CPU/GPU **다이 온도는 IOHID**, 팬 RPM과 주변 온도는 SMC 경유 |
| Windows 온도/팬 정보 읽기(EC) | 🚧 계획 중 |
| GPU 사용률 | 🔬 조사 완료 — IOReport 연동 자체는 동작하지만, 노출되는 residency 값이 Activity Monitor의 GPU % 값과 일치하지 않아 부정확한 값을 내보내느니 보류함 ([`docs/RESEARCH.md`](./docs/RESEARCH.md)) |
| 팬 **제어** | ⚙️ SMC 쓰기, **root 권한 필요** (`sudo peterfan fan set N` 또는 데몬 사용). `fan set`은 **RPM을 다시 읽어들여 검증**하므로 가짜 "성공" 메시지가 아니라 진짜 ✓/✗를 확인할 수 있습니다. Intel에서는 검증 완료, Apple Silicon에서는 시도 및 검증되지만(일부 모델은 펌웨어가 이를 무시할 수 있음) |
| CLI — `status`/`cpu`/`memory`/`disk`/`network`/`top`/`battery`/`system`/`temps`/`fans`/`fan`/`profile`/`curve`/`hardware`/`doctor`/`config`/`serve`/`benchmark`/`log`/`alert`/`license`/`completions`, 전역 `--watch` & `--json` | ✅ 실행 가능 |
| TUI 시스템 대시보드(ratatui) — CPU/메모리/디스크/네트워크/배터리/프로세스 + 온도/팬/전력 | ✅ 실행 가능 |
| **메뉴바 앱** — 스파크라인 그래프 아이콘(숫자/그래프/둘 다 선택 가능), 호버 시 간단 요약 툴팁, 2분/1시간/1일 히스토리 차트가 있는 팝오버 대시보드(호버로 정확한 값 + 평균/피크 확인), **각 팬의 실제 범위에 맞춰진 RPM 슬라이더로 팬별 Auto/Manual 제어**, 프로파일/Auto/Rules 제어, Top Processes에서 프로세스 종료, 영어/한국어 지원, 별도의 크기 조절 가능한 상세 창, 라이트/다크 모드 | ✅ 실행 가능 |
| **데몬**(`peterfand`) — 지속적인 커브 적용 + 종료 시 복원 + 임계 온도 오버라이드 + IPC 서버, LaunchDaemon 설치 지원 | ✅ 실행 가능 |
| **자동 업데이트** — 메뉴바의 "Check for Updates…"(그리고 `peterfan update`)가 GitHub Releases를 확인하고 제자리에서 설치 | ✅ 실행 가능 |
| **로컬 HTTP API**(`peterfan serve`) — 연동을 위한 JSON 지표 제공 및 제어 | ✅ 실행 가능 |
| 라이선싱 — 14일 체험판, Ed25519 오프라인 검증 키 | ✅ 구현 완료 (메뉴바 앱과 데몬의 팬 제어 기능에만 적용) |
| 데스크톱 GUI(Tauri), 플러그인 | 🗺️ 로드맵 |

아직 실제 센서를 읽을 수 없는 백엔드의 경우, CLI/TUI는 **자동으로 mock 백엔드로
전환되며 해당 데이터에 `simulated`라고 명확히 표시**합니다 — 그래서 항상 동작하는
데모를 볼 수 있고, 실제가 아닌 값을 실제인 척 보여주는 일은 절대 없습니다.

전체 계획은 [`docs/ROADMAP.md`](./docs/ROADMAP.md)를 참고하세요.

---

## 가격 및 라이선스 (Pricing & licensing)

- **CLI(`peterfan`), TUI(`peterfan-tui`), 그리고 데몬의 팬 제어 핵심 로직은 MIT
  라이선스이며 영원히 무료입니다** — 자유롭게 스크립트로 활용하고, 임베드하고,
  포크하세요.
- **메뉴바 앱**(`peterfan-menubar` / `PeterFan.app`)은 최초 실행 시점부터 **14일간**
  무료로 체험할 수 있습니다. 체험 기간이 끝난 뒤에도 이를 계속 실행하려면(그리고
  데몬의 *지속적인* 백그라운드 팬 제어를 쓰려면) 라이선스가 필요합니다:
  ```sh
  peterfan license status              # 남은 체험 일수 / 라이선스 상태 확인
  peterfan license activate <key>      # 구매 시 받은 PFAN1-... 키
  ```
  체험 기간이 끝난 뒤에도 라이선스 없이 메뉴바 앱은 계속 실시간 지표를 보여줍니다 —
  제한되는 것은 상시 구동되는 백그라운드 위젯과 지속적인 팬 제어뿐이며,
  `sudo peterfan fan set N`으로 팬을 수동 제어하는 것은 여전히 가능합니다.
- 라이선스 키는 Ed25519로 서명되며 완전히 오프라인으로 검증됩니다(서버 통신 없음,
  서버 의존성 없음). 라이선스 구매: *(스토어 링크 준비 중)*.

---

## 다운로드 (Download)

미리 빌드된 바이너리는 각 [GitHub 릴리즈](https://github.com/uulab-official/peterfan/releases/latest)에
첨부되어 있습니다. macOS(Apple Silicon + Intel, 유니버설)와 Windows 빌드는 태그가
붙은 릴리즈마다 CI가 두 가지 형태로 생성합니다:

| 자산 | 포함 내용 | 이런 분께 적합 |
| --- | --- | --- |
| `PeterFan-vX.Y.Z.dmg` | `PeterFan.app`과 Applications 바로가기만 포함 | 메뉴바 앱만 필요한 분 — 더블클릭, 드래그, 끝 |
| `peterfan-vX.Y.Z-universal-apple-darwin.tar.gz` | `peterfan`(CLI), `peterfan-tui`, `peterfan-menubar`, `peterfand`, **그리고** `PeterFan.app` | 개발자 / 스크립팅 목적 / CLI나 TUI도 함께 쓰고 싶은 분 |

```sh
# .dmg (메뉴바 앱만, 터미널 불필요)
open PeterFan-*.dmg
# → PeterFan.app을 Applications 바로가기로 드래그한 뒤 평소처럼 실행

# .tar.gz (CLI + TUI + 메뉴바 앱, 개발자용)
tar -xzf peterfan-*-universal-apple-darwin.tar.gz
cd peterfan-*-universal-apple-darwin
open PeterFan.app          # 메뉴바 앱
./peterfan status          # …또는 CLI / TUI를 직접 사용
```

두 형태 모두 같은 방식으로 빌드됩니다 — `.dmg`는 사실 `.tar.gz` 안에 있는 `.app`을
터미널을 쓰고 싶지 않은 사람들을 위해 일반 디스크 이미지로 다시 포장한 것뿐입니다.
Windows는 `.zip`으로 제공됩니다(CLI/TUI/메뉴바 바이너리만 포함 — 아직 `.exe`
설치 프로그램은 없습니다).

이 앱은 애드혹(ad-hoc) 서명만 되어 있습니다(유료 Apple Developer 계정이 없어
공증(notarize)은 되어 있지 않습니다). 최초 실행 시 "개발자를 확인할 수 없음"이라는
표준 macOS 경고가 뜹니다 — `PeterFan.app`을 우클릭 → **열기**를 선택하거나,
**시스템 설정 → 개인정보 보호 및 보안 → 그래도 열기**를 이용하세요. macOS가 여전히
"손상되어 열 수 없음"이라며 거부한다면 격리(quarantine) 플래그를 직접 제거하세요:
`xattr -dr com.apple.quarantine PeterFan.app peterfan*`.

---

## 팬 제어 활성화(최초 1회)

팬 제어는 SMC에 값을 써야 하므로 **root 권한이 필요**합니다 — Macs Fan Control이나
TG Pro와 정확히 같은 방식입니다. 매번 `sudo`를 입력하는 대신, 작은 root 헬퍼를 한 번만
설치해두세요(macOS 비밀번호 프롬프트가 **딱 한 번** 뜨고, 터미널에서 sudo를 칠 필요는
없습니다):

```sh
./peterfan install-daemon      # GUI 관리자 권한 프롬프트 1회; 매 부팅 시 자동 실행
./peterfan doctor              # 확인 사항: root 헬퍼 연결 여부, SMC 키 존재 여부
```

이후로는 메뉴바의 버튼들과 `peterfan fan …` 명령어가 root 헬퍼를 통해 팬을
제어하며, 추가 프롬프트는 뜨지 않습니다. 제거하려면 `peterfan uninstall-daemon`을
사용하세요. `peterfan fan set N`은 **RPM을 다시 읽어들여 검증**하므로 실제 ✓/✗
결과를 확인할 수 있습니다.

---

## 소스에서 빌드하기

[Rust 툴체인](https://rustup.rs)(1.80+)이 필요합니다.

```bash
# 전체 빌드
cargo build

# 현재 머신의 전체 대시보드(실제 CPU/메모리/디스크/네트워크/배터리)
cargo run -p peterfan-cli -- status

# 개별 지표
cargo run -p peterfan-cli -- cpu
cargo run -p peterfan-cli -- top --mem -n 5
cargo run -p peterfan-cli -- network

# 활성화된 백엔드와 그 기능들을 진단
cargo run -p peterfan-cli -- doctor

# 시뮬레이션된 머신 기준으로 전체 실행(데모/CI에 유용)
cargo run -p peterfan-cli -- --mock status

# 실시간 터미널 대시보드
cargo run -p peterfan-tui -- --mock

# macOS 메뉴바(Windows는 시스템 트레이)에서 실시간 지표 보기
cargo run -p peterfan-menubar
```

설치하고 나면 바이너리 이름은 그냥 `peterfan`입니다.

### 예시: `peterfan status`

```text
PeterFan v1.26.2
backend: sysinfo + macos  ·  Darwin 26.1  ·  up 5d 7h 8m

CPU · Apple M3 Max
   21.6%  ███░░░░░░░░░   cores ▄▃▂▂▂▂▂▂▂ ▁▁ ▁

Memory
  27.4 GB / 36.0 GB ( 76.1%)  █████████░░░
  wired 5.6 GB  ·  active 7.6 GB  ·  compressed 13.4 GB

Disk
  /              896.7 GB / 926.4 GB ( 96.8%)  ████████████  SSD

Network
  en0            ↓    4.2 MB/s  ↑   53.4 KB/s   172.20.248.39  ·  total ↓50.0 GB ↑109.0 GB

Battery
   72.0%  █████████░░░  charging  ~1h 7m to full
  214 cycles  ·  41.8 W

Temperatures
  CPU CPU            58°C  ███████░░░░░   (real die temp via IOHID)
  CPU CPU hottest    60°C  ███████░░░░░
  SSD SSD            36°C  ████░░░░░░░░
  BATT Battery       31°C  ███░░░░░░░░░

Fans
  Fan 1           2445 RPM    3%  ░░░░░░░░░░░░
  Fan 2           2635 RPM    3%  ░░░░░░░░░░░░

Power · 21.2 W
```

어떤 명령어든 `--json`을 붙이면 기계가 읽기 좋은 출력을 얻을 수 있습니다(Raycast,
Stream Deck, Hammerspoon, Home Assistant 등과 연동할 때 유용합니다).

전체 명령어 레퍼런스는 [`docs/CLI.md`](./docs/CLI.md)를 참고하세요.

---

## 한눈에 보는 아키텍처

```text
   CLI · TUI · GUI · HTTP API        ← presentation, portable
            │
            ▼
        peterfan-core                ← domain types, curves, profiles
            │   (knows nothing about any OS)
            ▼
     HardwareProvider  (trait)       ← the single seam
            ▲
            │ implemented by
   ┌────────┴─────────┬──────────────┐
  mock              macOS          Windows (planned)
                  (sysctl / SMC)   (EC / WMI)
```

코어는 **오직** `HardwareProvider` 트레이트에만 의존합니다. 각 플랫폼은 이 트레이트의
구현체를 하나씩 제공합니다. 나중에 Linux를 지원하려면 백엔드 하나만 추가하면
되고, 코어 코드는 건드릴 필요가 없습니다. 자세한 내용은
[`docs/ARCHITECTURE.md`](./docs/ARCHITECTURE.md)에서 확인하세요.

---

## 프로젝트 구조

```text
peterfan/
├── packages/
│   ├── core/        peterfan-core      — OS-agnostic types, curves, profiles, trait, licensing
│   ├── platform/    peterfan-platform  — mock + macOS backends (Windows/Linux planned)
│   ├── cli/         peterfan           — the command-line interface
│   ├── tui/         peterfan-tui       — ratatui live dashboard
│   ├── menubar/     peterfan-menubar   — macOS menu-bar / Windows tray app
│   └── daemon/      peterfand          — fan-control daemon (curve + safety)
├── tools/
│   ├── icongen/          generates the app icon PNG — dev-only, excluded from workspace
│   └── license-keygen/   issues license keys — dev-only, never shipped, excluded from workspace
├── apps/
│   └── landing/     static marketing website (open apps/landing/index.html)
├── packaging/       LaunchDaemon plist · Homebrew formula · scripts/ install helpers
├── docs/            architecture, roadmap, CLI reference, research notes
└── (planned) apps/desktop (Tauri GUI)
```

---

## 안전성

팬 제어는 하드웨어 수준의 작업이며 부주의하게 다루면 위험할 수 있습니다. PeterFan은
다음 원칙을 지킵니다:

- **역량을 미리 명시** — 각 백엔드는 자신이 무엇을 할 수 있는지 미리 알리며, UI는
  안전하게 수행할 수 없는 제어 기능을 절대 제공하지 않습니다.
- **읽기 전용이 우선** — 모니터링은 권한 상승 없이도 동작하며, 제어는 의도적으로
  분리된 별도의 단계입니다.
- **종료 시 복원** — `peterfand` 데몬은 Ctrl-C / SIGTERM / panic 발생 시 제어권을
  OS에 되돌려주며, 임계 온도를 넘으면 팬을 강제로 100%로 돌립니다.

---

## 기여하기

이제 막 시작한 프로젝트라 참여하기 좋은 시점입니다. [`CONTRIBUTING.md`](./CONTRIBUTING.md)를
참고하세요. 초기 단계에서 가장 가치 있는 기여는 기존 `HardwareProvider` 트레이트
뒤에 붙는 **새로운 플랫폼 백엔드**입니다(macOS의 실제 SMC 읽기, Windows의
EC/WMI 백엔드 등).

---

## 라이선스

이 저장소의 코드는 [MIT](./LICENSE) © PeterFan contributors 라이선스를 따릅니다 —
메뉴바 앱의 소스 코드도 포함해서요. *제품으로서 라이선스가 필요한* 부분은
**14일 체험 기간이 지난 뒤에도 메뉴바 앱의 상시 백그라운드 위젯과 지속적인 팬
제어를 계속 실행할 권리**뿐입니다(위의 [가격 및 라이선스](#pricing--licensing)
참고). 그 아래에 있는 CLI, TUI, 데몬의 팬 커브 로직에는 그런 제약이 전혀 없으며,
프로젝트의 나머지 부분과 마찬가지로 MIT 조건 아래 자유롭게 사용, 학습, 수정할 수
있습니다.
