#!/usr/bin/env bash
# Safe local checks: no application launch, install, fan writes or signing.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

case "${1:-}" in
  "") scope=(--workspace) ;;
  --core-only) scope=(-p peterfan-core) ;;
  -h|--help)
    echo 'Usage: scripts/check-contributor.sh [--core-only]'
    echo 'Requires Rust >=1.88 with rustfmt/clippy and Node.js >=22; downloads crates as needed.'
    exit 0 ;;
  *) echo "Unknown argument: $1" >&2; exit 2 ;;
esac
if [[ $# -gt 1 ]]; then echo 'Use at most one option.' >&2; exit 2; fi

for tool in cargo rustc node; do
  if ! command -v "$tool" >/dev/null 2>&1; then echo "Required tool not found: $tool" >&2; exit 1; fi
done
node --input-type=module -e 'if(Number(process.versions.node.split(".")[0])<22)throw Error("Node.js >=22 required")'
# Cargo validates the workspace MSRV before compiling. Stable rustfmt/clippy are
# recommended; callers may choose a toolchain with RUSTUP_TOOLCHAIN or PATH.
rustc --version
node scripts/check-oss.mjs
node --test scripts/*.test.mjs
scripts/check-docs.sh
node scripts/check-dashboard-behavior.mjs
cargo fmt --all -- --check
cargo clippy --locked "${scope[@]}" --all-targets --all-features -- -D warnings
cargo test --locked "${scope[@]}" --all-features
if [[ "${1:-}" == --core-only ]]; then
  node scripts/check-runner-preview.mjs --web-only
else
  # Generate timing evidence from current Rust source; never reuse a developer's
  # ignored target artifact as a prerequisite for a fresh checkout.
  cargo test --locked -p peterfan-menubar render_runner_sprite_sheet_for_visual_qa -- --ignored --nocapture
  node scripts/check-runner-preview.mjs
fi
RUSTDOCFLAGS="${RUSTDOCFLAGS:-} -D warnings" cargo doc --locked "${scope[@]}" --all-features --no-deps
echo 'PASS contributor checks (no app launch or hardware changes)'
