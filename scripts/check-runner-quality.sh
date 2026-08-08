#!/usr/bin/env bash
set -euo pipefail

task_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
rust_bin=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin

cd "$task_root"
env RUSTC="$rust_bin/rustc" PATH="$rust_bin:/usr/bin:/bin" \
  cargo test -p peterfan-menubar render_runner_sprite_sheet_for_visual_qa -- --ignored --nocapture

test -s "$task_root/target/runner-sprite-sheet.png"
test -s "$task_root/target/runner-quality-report.json"
