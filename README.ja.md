# PeterFan

[English](./README.md) | [한국어](./README.ko.md) | **日本語** | [中文](./README.zh.md)

> **開発者のための Mac ファンコントローラー & システムモニター。** CLI・TUI・macOS
> メニューバーアプリを備えた、クロスプラットフォームなファンコントローラー兼ハードウェアモニター。
> Rust 製。

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](./LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.80%2B-orange.svg)](https://www.rust-lang.org)
![Status: beta](https://img.shields.io/badge/status-beta-yellow.svg)

PeterFan は単なるファン速度スライダーでは**ない**。開発者やパワーユーザーのための、
小さくて安全、スクリプト可能なシステムモニター兼ファンコントロールプラットフォームだ。
`lazygit`、`btop`、`mise` と並べて `brew install` するようなツールでありながら、
[iStat Menus](https://bjango.com/mac/istatmenus/) や
[Stats](https://github.com/exelban/stats) のようなメニューバーアプリでもある。
メニューバーにはリアルタイムのスパークライングラフ、メトリクスごとの履歴チャート、
直接的なファン速度コントロールを表示し、その下には Raycast やダッシュボードに
`--json` を流し込みたい人向けのスクリプト可能な CLI/TUI が控えている。

```text
Tiny · Simple · Beautiful · Safe · Extensible · Cross-platform
```

**CLI・TUI・ファンコントロールデーモンは無料、MIT ライセンスで永続的に提供される。**
メニューバーアプリには 14 日間の無料トライアルがあり、それ以降は常時稼働の
メニューバーウィジェットと永続的なバックグラウンドファンコントロールを使い続けるために
一度限りのライセンス（`peterfan license activate <key>`）が必要になる —
読み取り専用のコマンドは無効化されることはない。詳細は下記の
[料金](#pricing--licensing) を参照。

---

## Mac 向けダウンロード — ターミナル不要

1. **[最新の `.dmg` をダウンロード](https://github.com/uulab-official/peterfan/releases/latest)**
   （**Assets** の中から `PeterFan-vX.Y.Z.dmg` を探す）
2. ダブルクリックして開き、**PeterFan.app** を **Applications** ショートカットに
   ドラッグする
3. Applications（または Spotlight）から **PeterFan** を開く — 初回起動時は
   **右クリック → 開く** で確認する（[理由は？](#download)）

これだけだ — PeterFan はメニューバーに静かに常駐する。14 日間の無料トライアルがあり、
アカウント登録やサインアップは不要。コマンドラインを使いたい、あるいは Windows が
必要な場合は、`.tar.gz`/`.zip` アーカイブとソースからのビルド手順について下記の
[ダウンロード](#download) を参照。

---

## ステータス

**ベータ — v1.25.0。** 開発は活発に継続中。以下の表は実際にリリース済みの内容を示す。

| 領域 | 状態 |
| --- | --- |
| **システムメトリクス** — CPU、メモリ、ディスク、ネットワーク、プロセス | ✅ `sysinfo` によるリアルなクロスプラットフォーム対応（macOS + Windows） |
| **macOS メモリ内訳** — wired / active / inactive / compressed | ✅ mach の `host_statistics64` によるリアルな値（`vm_stat` と照合済み） |
| **バッテリー** — 充電量、状態、サイクル数、残り時間、**温度** | ✅ `battery` + IOHID によるリアルな値（Apple Silicon ではヘルス情報をフィルタリング） |
| コアモデル（型、メトリクス、カーブ、プロファイル、トレイト） | ✅ 実装・テスト済み |
| モックバックエンド（完全にシミュレートされたマシン + メトリクス） | ✅ 実装済み |
| macOS ハードウェア情報（`sysctl` による CPU/RAM/OS） | ✅ リアルな値、読み取り専用 |
| **macOS の温度とファン回転数** | ✅ リアルな値 — CPU/GPU のダイ温度は **IOHID 経由**、ファン回転数と外気温は SMC 経由 |
| Windows の温度 / ファン読み取り（EC） | 🚧 計画中 |
| GPU 使用率 | 🔬 調査済み — IOReport の配線自体は動作するが、そこで得られる使用率が
Activity Monitor の GPU % と一致しないため、不正確な値を出荷するのではなく保留とした
（[`docs/RESEARCH.md`](./docs/RESEARCH.md)） |
| ファン**制御** | ⚙️ SMC への書き込みで、**root 権限が必要**（`sudo peterfan fan set N` または
デーモン経由）。`fan set` は**回転数を読み戻して検証する**ため、見せかけの「ok」ではなく
本物の ✓/✗ が得られる。Intel Mac では動作確認済み。Apple Silicon でも試行・検証されるが、
一部モデルのファームウェアはこれを無視する場合がある |
| CLI — `status`/`cpu`/`memory`/`disk`/`network`/`top`/`battery`/`system`/`temps`/`fans`/`fan`/`profile`/`curve`/`hardware`/`doctor`/`config`/`serve`/`benchmark`/`log`/`alert`/`license`/`completions`、グローバルな `--watch` と `--json` | ✅ 実行可能 |
| TUI システムダッシュボード（ratatui） — CPU/メモリ/ディスク/ネットワーク/バッテリー/プロセス + 温度/ファン/電力 | ✅ 実行可能 |
| **メニューバーアプリ** — スパークライングラフアイコン（数値/グラフ/両方を選択可）、
クイックサマリー付きホバーツールチップ、2分/1時間/1日の履歴チャートを持つポップオーバー
ダッシュボード（ホバーで正確な値と平均/ピークを表示）、**各ファンの実際の可動域に
制限された RPM スライダーによる Auto/Manual 個別制御**、プロファイル/Auto/ルール制御、
Top Processes からのプロセス終了、英語/한국어、独立したリサイズ可能な詳細ウィンドウ、
ライト/ダークモード | ✅ 実行可能 |
| **デーモン**（`peterfand`） — 継続的なカーブ制御 + 終了時の復元 + 危険温度時の
オーバーライド + IPC サーバー、LaunchDaemon インストール | ✅ 実行可能 |
| **自己アップデート** — メニューバーの「アップデートを確認…」（および
`peterfan update`）が GitHub Releases を確認しその場でインストールする | ✅ 実行可能 |
| **ローカル HTTP API**（`peterfan serve`） — 連携用の JSON メトリクスと制御 | ✅ 実行可能 |
| ライセンス — 14 日間トライアル、Ed25519 オフライン検証キー | ✅ 実装済み（メニューバーアプリとデーモンのファン制御のみ対象） |
| デスクトップ GUI（Tauri）、プラグイン | 🗺️ ロードマップ |

バックエンドがまだ実センサーを読み取れない場合、CLI/TUI は**透過的にモックバックエンドへ
フォールバックし、そのデータを明確に `simulated` としてラベル付けする** — そのため、
常に動作するデモが手に入り、実際のデータでない値を実データだと偽ることは決してない。

全体計画は [`docs/ROADMAP.md`](./docs/ROADMAP.md) を参照。

---

## 料金とライセンス (Pricing & licensing)

- **CLI（`peterfan`）、TUI（`peterfan-tui`）、そしてデーモンのファン制御コアは
  MIT ライセンスであり、永続的に無料** — スクリプトに組み込む、埋め込む、フォークする、
  すべて自由。
- **メニューバーアプリ**（`peterfan-menubar` / `PeterFan.app`）は初回起動から
  **14 日間**無料で試用できる。トライアル後、これを実行する（およびデーモンの
  *永続的な*バックグラウンドファン制御）にはライセンスが必要になる。
  ```sh
  peterfan license status              # トライアル残日数 / ライセンス状態
  peterfan license activate <key>      # 購入時に発行される PFAN1-... キー
  ```
  トライアル期間を過ぎてもライセンスがない場合、メニューバーアプリはリアルタイムの
  メトリクス表示を続ける — 制限されるのは常時稼働のバックグラウンドウィジェットと
  継続的なファン制御のみで、`sudo peterfan fan set N` による手動でのファン操作は
  引き続き可能。
- ライセンスキーは Ed25519 で署名され、完全にオフラインで検証される
  （フォンホームなし、サーバー依存なし）。ライセンスの購入: *（ストアリンクは近日公開）*。

---

## ダウンロード (Download)

ビルド済みバイナリは各 [GitHub Release](https://github.com/uulab-official/peterfan/releases/latest)
に添付されている。macOS（Apple Silicon + Intel のユニバーサルバイナリ）と Windows の
ビルドは、タグ付きリリースのたびに CI によって以下の 2 形式で生成される。

| アセット | 内容 | 向いている用途 |
| --- | --- | --- |
| `PeterFan-vX.Y.Z.dmg` | `PeterFan.app` と Applications ショートカットのみ | メニューバーアプリだけが欲しい人向け — ダブルクリック、ドラッグ、それで完了 |
| `peterfan-vX.Y.Z-universal-apple-darwin.tar.gz` | `peterfan`（CLI）、`peterfan-tui`、`peterfan-menubar`、`peterfand`、**そして** `PeterFan.app` | 開発者 / スクリプト用途 / CLI や TUI も使いたい人向け |

```sh
# .dmg（メニューバーアプリのみ、ターミナル不要）
open PeterFan-*.dmg
# → PeterFan.app を Applications ショートカットにドラッグして、通常どおり起動する

# .tar.gz（CLI + TUI + メニューバーアプリ、開発者向け）
tar -xzf peterfan-*-universal-apple-darwin.tar.gz
cd peterfan-*-universal-apple-darwin
open PeterFan.app          # メニューバーアプリ
./peterfan status          # …あるいは CLI / TUI を直接使う
```

どちらも作り方は同じ — `.dmg` は `.tar.gz` の中に入っている `.app` を、ターミナルを
使いたくない人向けに通常のディスクイメージとして再パッケージしたものにすぎない。
Windows 向けには `.zip` が用意されている（CLI/TUI/メニューバーのバイナリのみ —
`.exe` インストーラーはまだない）。

このアプリはアドホック署名されている（有料の Apple Developer アカウントの裏付けが
ないため、公証は受けていない）。初回起動時には標準の「開発元を確認できません」という
プロンプトが表示される — `PeterFan.app` を右クリックして **開く** を選ぶか、
**システム設定 → プライバシーとセキュリティ → このまま開く** を選択する。macOS が
それでも「壊れているため開けません」と拒否する場合は、隔離フラグを手動で解除する。
`xattr -dr com.apple.quarantine PeterFan.app peterfan*`

---

## ファン制御を有効にする（初回のみ）

ファン制御は SMC への書き込みを行うため、**root 権限が必要**になる — これは
Macs Fan Control や TG Pro とまったく同じだ。毎回 `sudo` を入力する代わりに、
小さな root ヘルパーを一度だけインストールすればよい（**macOS のパスワード
プロンプトが 1 回**表示されるだけで、ターミナルでの sudo は不要）。

```sh
./peterfan install-daemon      # GUI の管理者権限プロンプトが1回表示される。起動のたびに実行される
./peterfan doctor              # 確認内容: root ヘルパーへの到達性、SMC キーの存在
```

これ以降、メニューバーのボタンや `peterfan fan …` は root ヘルパー経由でファンを
制御する — それ以上のプロンプトは表示されない。削除する場合は
`peterfan uninstall-daemon` を使う。`peterfan fan set N` は**回転数を読み戻して
検証する**ため、本物の ✓/✗ が得られる。

---

## ソースからビルドする

[Rust ツールチェーン](https://rustup.rs)（1.80 以上）が必要。

```bash
# すべてをビルド
cargo build

# このマシン向けのフルダッシュボード（実際の CPU/メモリ/ディスク/ネットワーク/バッテリー）
cargo run -p peterfan-cli -- status

# 個別のメトリクス
cargo run -p peterfan-cli -- cpu
cargo run -p peterfan-cli -- top --mem -n 5
cargo run -p peterfan-cli -- network

# 有効なバックエンドとその機能を診断する
cargo run -p peterfan-cli -- doctor

# すべてをシミュレートされたマシンに対して実行（デモや CI に最適）
cargo run -p peterfan-cli -- --mock status

# ライブなターミナルダッシュボード
cargo run -p peterfan-tui -- --mock

# macOS メニューバー（Windows の場合はシステムトレイ）でのライブメトリクス
cargo run -p peterfan-menubar
```

インストール後は、バイナリは単に `peterfan` という名前になる。

### 例: `peterfan status`

```text
PeterFan v1.25.0
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

どのコマンドにも `--json` を付けると機械可読な出力が得られる（Raycast、Stream Deck、
Hammerspoon、Home Assistant などとの連携に便利）。

コマンドの全リファレンスは [`docs/CLI.md`](./docs/CLI.md) を参照。

---

## アーキテクチャを一枚の図で

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

コアは `HardwareProvider` トレイトにのみ依存する。各プラットフォームは 1 つの
実装を提供する。将来 Linux に対応する場合も、バックエンドを 1 つ追加するだけで済み、
コアには一切手を入れない。詳細は [`docs/ARCHITECTURE.md`](./docs/ARCHITECTURE.md) を参照。

---

## プロジェクト構成

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

ファン制御はハードウェアレベルの操作であり、不用意に行うと危険を伴いうる。
PeterFan の設計は以下を徹底している。

- **ケイパビリティの事前提示** — バックエンドは自分にできることを申告し、UI は
  安全に実行できない制御を決して提示しない。
- **読み取り専用が先** — モニタリングは昇格権限なしで動作し、制御はそれとは
  別の意図的なステップとして行う。
- **終了時の復元** — `peterfand` デーモンは Ctrl-C / SIGTERM / パニック時に
  制御を OS へ返し、危険な温度を超えた場合はファンを強制的に 100% にする。

---

## コントリビュート

このプロジェクトはまだ若く、参加するには絶好のタイミングだ。
[`CONTRIBUTING.md`](./CONTRIBUTING.md) を参照。初期段階で最も価値のある貢献は、
既存の `HardwareProvider` トレイトの裏側に実装する**新しいプラットフォーム
バックエンド**（macOS での実際の SMC 読み取り、Windows 向けの EC/WMI バックエンド）だ。

---

## ライセンス

このリポジトリのコードは [MIT](./LICENSE) © PeterFan contributors —
メニューバーアプリのソースコードを含む。*製品としてライセンスされている*のは、
**14 日間のトライアルを超えてメニューバーアプリの常時稼働バックグラウンドウィジェットと
永続的なファン制御を実行する権利**だ（上記の
[料金とライセンス](#pricing--licensing) を参照）。その下にある CLI、TUI、
デーモンのファンカーブロジックにはそのような制限は一切なく、プロジェクトの他の部分と
同様に MIT の条件のもとで自由に使用、study、改変できる。
