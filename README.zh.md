# PeterFan

[English](./README.md) | [한국어](./README.ko.md) | [日本語](./README.ja.md) | **中文**

**为开发者打造的系统监控与 Mac 风扇控制工具。**
使用 Rust 构建，提供 macOS 菜单栏 / Windows 托盘应用、CLI、TUI 和本地 HTTP API。

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](./LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.88%2B-orange.svg)](https://www.rust-lang.org)

![PeterFan 仪表盘与 CLI](./docs/images/peterfan-readme-overview.png)

## 当前状态

项目处于 Beta 阶段。截至 2026 年 10 月 22 日，公开发行版为 **v1.32.2**。
本地开发版本 **v1.32.4** 与已发布的下载文件不同。请在
[GitHub Releases](https://github.com/uulab-official/peterfan/releases) 确认当前版本与文件。

| 功能 | 实现情况 |
| --- | --- |
| 系统指标 | macOS / Windows 的真实 CPU、内存、磁盘、网络、进程和电池数据；可用字段取决于运行环境。 |
| macOS 传感器 | 通过 SMC / IOHID 读取温度和风扇 RPM。默认关注 CPU Core Average，详细视图保留各传感器读数。 |
| macOS 风扇控制 | 手动设置、配置方案、曲线与守护进程持续控制；需要受支持的机型及明确的管理员设置。 |
| Windows 温度 | 读取固件公开的 ACPI 温度区域。这是系统温度，不是 CPU 核心温度。风扇 RPM 和风扇控制尚不支持。 |
| 菜单栏 / 托盘 | 随 CPU 负载变化的角色动画、历史图表、详细窗口和浅色 / 深色模式。本地 v1.32.4 包含角色改进与随机切换。 |
| 设置与集成 | TOML 配置、通知、启动设置、JSON 输出、CLI / TUI 和本地 HTTP API。 |
| 更新 | 通过应用或 CLI 明确检查并安装，验证发行文件后才替换应用。 |

全部源码采用 [MIT 协议](./LICENSE)，无需账号、登录或许可证密钥。
无法获取真实传感器时，会显示不支持 / 不可用，或明确标记为模拟数据。
模拟测试成功不代表已通过实机验证。

## 安装

从[公开发行页](https://github.com/uulab-official/peterfan/releases)选择对应文件。

| 文件 | 内容 |
| --- | --- |
| `PeterFan-vX.Y.Z.dmg` | macOS 应用与 Applications 快捷方式；将应用拖入 Applications 后启动。 |
| `peterfan-vX.Y.Z-universal-apple-darwin.tar.gz` | macOS Universal CLI、TUI、守护进程、菜单栏二进制和应用包。 |
| `peterfan-vX.Y.Z-x86_64-pc-windows-msvc.zip` | Windows 应用、CLI / TUI、按用户安装 / 卸载脚本、WebView2 引导程序及说明。 |

macOS 正式分发文件应通过 Developer ID 签名、公证与票据装订验证。
安装前可以检查下载的 DMG：

```bash
spctl -a -vv -t open --context context:primary-signature PeterFan-vX.Y.Z.dmg
```

预期结果为 `accepted` 和 `source=Notarized Developer ID`。
如果 Gatekeeper 拒绝文件，请使用更新的已签名发行版，或从源码构建。
不建议绕过签名检查。

Windows 用户解压 ZIP 后运行随附的安装脚本：

```powershell
powershell -ExecutionPolicy Bypass -File .\install.ps1
```

应用安装到 `%LOCALAPPDATA%\Programs\PeterFan` 并创建开始菜单快捷方式，无需管理员权限。
卸载时运行安装目录中的 `uninstall.ps1`。

## 从源码开始

需要 Rust **1.88 或更新版本**、Node.js **22 或更新版本**；macOS 还需要 Xcode Command Line Tools。
常规源码构建和测试不需要签名密钥、Apple Developer 账号或管理员权限。

```bash
git clone https://github.com/uulab-official/peterfan.git
cd peterfan
cargo +stable build --locked -p peterfan-cli
cargo +stable run --locked -p peterfan-cli -- --mock status
cargo +stable run --locked -p peterfan-cli -- --mock --json status
scripts/check-contributor.sh
```

最后一条命令检查格式、Clippy、测试和文档等内容，不启动应用、不安装守护进程，也不写入风扇。
Linux / 无桌面环境可使用 `scripts/check-contributor.sh --core-only`；
该模式验证核心逻辑，不代表原生 UI 已通过验证。

在原生平台上构建所有二进制：

```bash
cargo +stable build --locked --release --workspace
```

使用 `target/release/peterfan status` 读取真实系统指标，
`target/release/peterfan doctor` 进行诊断，
`target/release/peterfan temps --all` 查看传感器列表。
执行 `target/release/peterfan-menubar` 会实际启动菜单栏 / 托盘应用。

## 风扇控制与安全

默认监控为只读。macOS 持续风扇控制需要单独安装 root 辅助进程，并进行管理员认证。
请参阅[英文 README](./README.md) 与 [CLI 参考](./docs/CLI.md)。
常规开发和模拟运行无需安装该辅助进程。

程序包含能力检查、RPM 回读、临界温度处理和恢复系统自动控制的逻辑，
但控制结果仍取决于机型和固件，不能保证在强制终止、断电或所有固件故障后自动恢复。
请在 [PRODUCTION_CHECKLIST.md](./docs/PRODUCTION_CHECKLIST.md) 中分别记录实机与发布验证。

## 参与开发

- [开发指南](./docs/DEVELOPMENT.md)：环境、检查命令及额外验证的副作用。
- [CONTRIBUTING.md](./CONTRIBUTING.md)：PR、问题报告与硬件安全要求。
- [架构说明](./docs/ARCHITECTURE.md)：核心与平台模块的边界。
- [SECURITY.md](./SECURITY.md)：漏洞报告方式。
- [CODE_OF_CONDUCT.md](./CODE_OF_CONDUCT.md)：社区参与准则。
- [THIRD_PARTY_NOTICES.md](./THIRD_PARTY_NOTICES.md)：第三方素材及依赖的许可信息。

完整功能和公共命令参考请以 [English README](./README.md) 为准。
