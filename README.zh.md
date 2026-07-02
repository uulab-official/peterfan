# PeterFan

[English](./README.md) | [한국어](./README.ko.md) | [日本語](./README.ja.md) | **中文**

> **为开发者打造的 Mac 风扇控制器与系统监控工具。** 一款跨平台的风扇控制与硬件监控工具，
> 提供 CLI、TUI 以及 macOS 菜单栏应用 —— 使用 Rust 构建。

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](./LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.80%2B-orange.svg)](https://www.rust-lang.org)
![Status: beta](https://img.shields.io/badge/status-beta-yellow.svg)

PeterFan **不只是**一个风扇转速滑块。它是为开发者和高阶用户打造的一个小巧、安全、
可脚本化的系统监控工具，*同时也是*一个风扇控制平台 —— 就是那种你会和 `lazygit`、
`btop`、`mise` 一起 `brew install` 的工具，菜单栏应用的风格类似
[iStat Menus](https://bjango.com/mac/istatmenus/) 和
[Stats](https://github.com/exelban/stats)：菜单栏里的实时迷你图表、按指标划分的历史
曲线图、直接的风扇转速控制，底层还有可脚本化的 CLI/TUI，方便那些更想把 `--json`
输出接入 Raycast 或自建仪表盘的人。

```text
Tiny · Simple · Beautiful · Safe · Extensible · Cross-platform
```

**CLI、TUI 以及风扇控制守护进程永久免费，并采用 MIT 协议开源。**
菜单栏应用提供 14 天免费试用；试用期结束后，需要一次性授权
（`peterfan license activate <key>`）才能继续使用常驻的菜单栏组件和持久化的后台风扇
控制 —— 只读命令则永远不受影响，可以一直正常使用。详见下方[价格](#pricing--licensing)。

---

## 下载 Mac 版 —— 无需终端

1. **[下载最新的 `.dmg`](https://github.com/uulab-official/peterfan/releases/latest)**
   （在 **Assets** 中查找 `PeterFan-vX.Y.Z.dmg`）
2. 双击打开后，将 **PeterFan.app** 拖到 **Applications** 快捷方式上
3. 从「应用程序」（或 Spotlight）中打开 **PeterFan** —— 首次启动时，
   **右键点击 → 打开** 以确认（[为什么？](#download)）

就这样 —— PeterFan 会安静地驻留在你的菜单栏中。14 天免费试用，无需注册账号。
更喜欢命令行，或者需要 Windows 版本？请参阅下方的[下载](#download)章节，获取
`.tar.gz`/`.zip` 压缩包以及源码构建说明。

---

## 状态

**Beta 版 —— v1.25.2。** 项目正在积极开发中；下表反映的是目前已经实际交付的功能：

| 领域 | 状态 |
| --- | --- |
| **系统指标** —— CPU、内存、磁盘、网络、进程 | ✅ 真实数据，跨平台（macOS + Windows），基于 `sysinfo` |
| **macOS 内存明细** —— wired / active / inactive / compressed | ✅ 通过 mach `host_statistics64` 获取真实数据（已与 `vm_stat` 校验一致） |
| **电池** —— 电量、状态、循环次数、剩余时间、**温度** | ✅ 通过 `battery` + IOHID 获取真实数据（在 Apple Silicon 上电池健康度已做过滤） |
| 核心模型（types、metrics、curves、profiles、trait） | ✅ 已实现并测试 |
| 模拟后端（完全模拟的机器 + 指标） | ✅ 已实现 |
| macOS 硬件信息（通过 `sysctl` 获取 CPU/RAM/OS 信息） | ✅ 真实数据，只读 |
| **macOS 温度与风扇转速** | ✅ 真实数据 —— CPU/GPU **芯片温度通过 IOHID 获取**，风扇转速与环境温度通过 SMC 获取 |
| Windows 温度/风扇读取（EC） | 🚧 规划中 |
| GPU 利用率 | 🔬 已调研 —— IOReport 链路可以跑通，但它暴露出的占用率与「活动监视器」的 GPU % 对不上，因此暂缓交付，而不是提供不准确的数据（详见 [`docs/RESEARCH.md`](./docs/RESEARCH.md)） |
| 风扇**控制** | ⚙️ 通过 SMC 写入，**需要 root 权限**（`sudo peterfan fan set N` 或使用守护进程）。`fan set` 会**通过回读转速进行校验**，因此你得到的是真实的 ✓/✗，而非虚假的「ok」。在 Intel 机型上已确认可用；在 Apple Silicon 上会尝试并校验（部分机型的固件可能会忽略该指令） |
| CLI —— `status`/`cpu`/`memory`/`disk`/`network`/`top`/`battery`/`system`/`temps`/`fans`/`fan`/`profile`/`curve`/`hardware`/`doctor`/`config`/`serve`/`benchmark`/`log`/`alert`/`license`/`completions`，以及全局的 `--watch` 与 `--json` | ✅ 可运行 |
| TUI 系统仪表盘（ratatui）—— CPU/内存/磁盘/网络/电池/进程 + 温度/风扇/功耗 | ✅ 可运行 |
| **菜单栏应用** —— 迷你图表图标（数字/图表/两者皆可，自由选择）、悬停显示快速摘要的提示框、带有 2 分钟/1 小时/1 天历史曲线图的弹出式仪表盘（悬停可查看精确数值及均值/峰值）、**每个风扇独立的自动/手动控制，配有限定在该风扇真实转速范围内的转速滑块**、配置文件/自动/规则控制、可在「Top Processes」中直接结束进程、支持英文/한국어、独立可调整大小的详情窗口、明暗两种主题 | ✅ 可运行 |
| **守护进程**（`peterfand`）—— 持续曲线控制 + 退出时自动恢复 + 临界温度强制介入 + IPC 服务；支持 LaunchDaemon 安装 | ✅ 可运行 |
| **自动更新** —— 菜单栏中的「检查更新…」（以及 `peterfan update`）会检查 GitHub Releases 并原地安装 | ✅ 可运行 |
| **本地 HTTP API**（`peterfan serve`）—— 提供 JSON 格式的指标数据与控制接口，便于集成 | ✅ 可运行 |
| 授权系统 —— 14 天试用期，Ed25519 离线验证密钥 | ✅ 已实现（仅限菜单栏应用及守护进程的风扇控制功能） |
| 桌面 GUI（Tauri）、插件系统 | 🗺️ 路线图中 |

当某个后端尚无法读取真实传感器数据时，CLI/TUI **会自动无缝回退到模拟后端，并明确将
数据标注为 `simulated`** —— 这样你总能看到一个可用的演示效果，而我们也绝不会在数据
不真实的情况下假装它是真实的。

完整规划请参阅 [`docs/ROADMAP.md`](./docs/ROADMAP.md)。

---

## 价格与许可 (Pricing & licensing)

- **CLI（`peterfan`）、TUI（`peterfan-tui`）以及守护进程中的风扇控制核心均采用
  MIT 协议，永久免费** —— 你可以随意编写脚本调用它们、将它们嵌入其他项目、或者
  fork 它们。
- **菜单栏应用**（`peterfan-menubar` / `PeterFan.app`）自首次启动起可免费试用
  **14 天**。试用期结束后，继续运行它（以及守护进程*持续性*的后台风扇控制）需要
  授权：
  ```sh
  peterfan license status              # trial days left / license status
  peterfan license activate <key>      # PFAN1-... key from your purchase
  ```
  即使试用期结束后没有授权，菜单栏应用依然会持续显示实时指标 —— 仅有常驻的后台
  组件和持续性风扇控制会被锁定；你仍然可以通过 `sudo peterfan fan set N`
  手动控制风扇。
- 授权密钥采用 Ed25519 签名，完全离线验证（无需联网校验，不依赖任何服务器）。
  购买授权：*（商店链接即将上线）*。

---

## 下载 (Download)

每个 [GitHub Release](https://github.com/uulab-official/peterfan/releases/latest)
都附带了预编译好的二进制文件。macOS（Apple Silicon + Intel 通用版）与 Windows
构建版本会在每次打标签发布时由 CI 自动生成，共有两种形式：

| 资源包 | 包含内容 | 适用场景 |
| --- | --- | --- |
| `PeterFan-vX.Y.Z.dmg` | 仅包含 `PeterFan.app` 与一个应用程序快捷方式 | 只想使用菜单栏应用的用户 —— 双击、拖拽，即可完成 |
| `peterfan-vX.Y.Z-universal-apple-darwin.tar.gz` | `peterfan`（CLI）、`peterfan-tui`、`peterfan-menubar`、`peterfand`，**以及** `PeterFan.app` | 开发者/脚本用户/同时想使用 CLI 或 TUI 的用户 |

```sh
# .dmg (menu-bar app only, no Terminal needed)
open PeterFan-*.dmg
# → drag PeterFan.app onto the Applications shortcut, then launch it normally

# .tar.gz (CLI + TUI + menu-bar app, for developers)
tar -xzf peterfan-*-universal-apple-darwin.tar.gz
cd peterfan-*-universal-apple-darwin
open PeterFan.app          # menu-bar app
./peterfan status          # …or use the CLI / TUI directly
```

两者的构建方式其实是一样的 —— `.dmg` 里的内容就是 `.tar.gz` 中的那个 `.app`，
只是为了方便不想用终端的用户，重新打包成了普通的磁盘镜像。Windows 用户会得到一个
`.zip`（仅包含 CLI/TUI/菜单栏应用的二进制文件 —— 暂时还没有 `.exe` 安装程序）。

该应用采用临时签名（ad-hoc signing，背后没有付费的 Apple 开发者账号，因此未经过
公证）。首次启动时会出现系统标准的「无法验证开发者」提示 —— 右键点击
`PeterFan.app` → **打开**，或者进入 **系统设置 → 隐私与安全性 → 仍要打开**。
如果 macOS 仍然提示「已损坏，无法打开」，请手动清除隔离标记：
`xattr -dr com.apple.quarantine PeterFan.app peterfan*`。

---

## 启用风扇控制（一次性设置）

风扇控制需要写入 SMC，这**需要 root 权限** —— 这一点与 Macs Fan Control 或
TG Pro 完全一致。与其每次都手动输入 `sudo`，不如一次性安装这个小巧的 root
辅助进程（只需在 macOS 弹出的**一次**管理员密码提示中确认，无需在终端输入 sudo）：

```sh
./peterfan install-daemon      # one GUI admin prompt; runs at every boot
./peterfan doctor              # confirms: root helper reachable, SMC keys present
```

设置完成后，菜单栏中的按钮以及 `peterfan fan …` 命令都会通过这个 root 辅助进程
来驱动风扇 —— 之后不会再弹出任何提示。可以使用 `peterfan uninstall-daemon`
将其卸载。`peterfan fan set N` 会**通过回读转速进行校验**，因此你得到的是真实的
✓/✗ 结果。

---

## 从源码构建

需要安装 [Rust 工具链](https://rustup.rs)（1.80 及以上版本）。

```bash
# Build everything
cargo build

# Full dashboard for THIS machine (real CPU/mem/disk/net/battery)
cargo run -p peterfan-cli -- status

# Individual metrics
cargo run -p peterfan-cli -- cpu
cargo run -p peterfan-cli -- top --mem -n 5
cargo run -p peterfan-cli -- network

# Diagnose the active backends & their capabilities
cargo run -p peterfan-cli -- doctor

# Everything against the simulated machine (great for demos/CI)
cargo run -p peterfan-cli -- --mock status

# Live terminal dashboard
cargo run -p peterfan-tui -- --mock

# Live metrics in the macOS menu bar (Windows: system tray)
cargo run -p peterfan-menubar
```

安装完成后，该二进制文件即为 `peterfan`。

### 示例：`peterfan status`

```text
PeterFan v1.25.2
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

在任意命令后加上 `--json`，即可获得机器可读的输出（非常适合接入 Raycast、
Stream Deck、Hammerspoon、Home Assistant 等工具）。

完整命令参考请参阅 [`docs/CLI.md`](./docs/CLI.md)。

---

## 一图看懂架构

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

核心层**只**依赖 `HardwareProvider` trait。每个平台只需提供一份实现即可。
未来如果要支持 Linux，只需新增一个后端 —— 无需改动核心层代码。完整细节请参阅
[`docs/ARCHITECTURE.md`](./docs/ARCHITECTURE.md)。

---

## 项目结构

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

## 安全性

风扇控制属于硬件级操作，如果操作不慎可能会带来风险。PeterFan 在设计上始终坚持：

- **能力预先声明** —— 各后端会明确声明自己支持哪些功能；UI 绝不会提供它无法安全
  执行的控制选项。
- **只读优先** —— 监控功能无需提升权限即可使用；控制功能则是一个单独且刻意为之
  的步骤。
- **退出时自动恢复** —— `peterfand` 守护进程在收到 Ctrl-C / SIGTERM / 发生 panic
  时会将控制权交还给操作系统，并且在温度超过临界值时会强制将风扇转速提升到 100%。

---

## 参与贡献

这是一个年轻的项目，现在正是加入的好时机。详见
[`CONTRIBUTING.md`](./CONTRIBUTING.md)。目前最有价值的早期贡献是基于现有的
`HardwareProvider` trait 编写**新的平台后端**（例如 macOS 上真实的 SMC 读取、
Windows 上的 EC/WMI 后端）。

---

## 许可协议

本仓库中的代码采用 [MIT](./LICENSE) 协议 © PeterFan contributors ——
包括菜单栏应用的源代码在内。真正*作为产品收费授权*的，是**在 14 天试用期之后
继续运行菜单栏应用常驻后台组件与持久化风扇控制的权利**（详见上方的
[价格与许可](#pricing--licensing)）；而底层的 CLI、TUI 以及守护进程的风扇曲线
逻辑并不受此限制，与本项目的其余部分一样，可依据 MIT 条款自由使用、研究和修改。
