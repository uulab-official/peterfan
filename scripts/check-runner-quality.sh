#!/usr/bin/env bash
set -euo pipefail

task_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
if [[ -n "${PETERFAN_RUST_BIN:-}" ]]; then
  rust_bin=$PETERFAN_RUST_BIN
elif stable_rustc=$(rustup which --toolchain stable rustc 2>/dev/null); then
  rust_bin=$(dirname "$stable_rustc")
elif system_rustc=$(command -v rustc); then
  rust_bin="$(dirname "$system_rustc")"
else
  echo "Install Rust or set PETERFAN_RUST_BIN to a toolchain bin directory." >&2
  exit 1
fi

if [[ ! -x "$rust_bin/rustc" ]]; then
  echo "PETERFAN_RUST_BIN must contain an executable rustc: $rust_bin" >&2
  exit 1
fi

cd "$task_root"
env RUSTC="$rust_bin/rustc" PATH="$rust_bin:$PATH" \
  cargo test --locked -p peterfan-menubar render_runner_sprite_sheet_for_visual_qa -- --ignored --nocapture

test -s "$task_root/target/runner-sprite-sheet.png"
test -s "$task_root/target/runner-quality-report.json"
node --input-type=module -e '
import { readFileSync } from "node:fs";
const report=readFileSync("target/runner-quality-report.json","utf8");
for(const key of ["minimum_region_delta","loop_seam_delta"]){
  if(!report.includes(JSON.stringify(key)))throw Error("Missing runner quality metric: "+key);
}
JSON.parse(report);
'
