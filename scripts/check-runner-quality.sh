#!/usr/bin/env bash
set -euo pipefail

task_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
if [[ -n "${PETERFAN_RUST_BIN:-}" ]]; then
  rust_bin=$PETERFAN_RUST_BIN
elif stable_rustc=$(rustup which --toolchain stable rustc 2>/dev/null); then
  rust_bin=$(dirname "$stable_rustc")
else
  rust_bin=/Users/uulab/.rustup/toolchains/stable-aarch64-apple-darwin/bin
fi

if [[ ! -x "$rust_bin/rustc" ]]; then
  echo "PETERFAN_RUST_BIN must contain an executable rustc: $rust_bin" >&2
  exit 1
fi

cd "$task_root"
env RUSTC="$rust_bin/rustc" PATH="$rust_bin:/usr/bin:/bin" \
  cargo test -p peterfan-menubar render_runner_sprite_sheet_for_visual_qa -- --ignored --nocapture

test -s "$task_root/target/runner-sprite-sheet.png"
test -s "$task_root/target/runner-quality-report.json"
