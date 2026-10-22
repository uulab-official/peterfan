#!/usr/bin/env bash
# Run deterministic sleep/wake and multi-display regression checks before a
# release build. Hardware sleep/wake and mixed-DPI evidence still requires a
# real release Mac; this gate proves the recovery and placement invariants.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ -n "${PETERFAN_RUST_BIN:-}" ]]; then
  RUST_BIN="$PETERFAN_RUST_BIN"
elif STABLE_RUSTC=$(rustup which --toolchain stable rustc 2>/dev/null); then
  RUST_BIN="$(dirname "$STABLE_RUSTC")"
elif system_rustc=$(command -v rustc); then
  RUST_BIN="$(dirname "$system_rustc")"
else
  echo "Install Rust or set PETERFAN_RUST_BIN to a toolchain bin directory." >&2
  exit 1
fi

if [[ ! -x "$RUST_BIN/rustc" ]]; then
  echo "PETERFAN_RUST_BIN must contain an executable rustc: $RUST_BIN" >&2
  exit 1
fi

TESTS=(
  "pause_recovery_requires_a_real_event_loop_gap"
  "resume_recovery_plan_invalidates_cached_state_before_first_post_wake_sample"
  "multi_display_regression_matrix_keeps_popover_on_clicked_display"
  "logical_popover_anchor_ignores_previous_window_scale_on_mixed_dpi_display"
  "logical_popover_anchor_stays_on_left_retina_display"
  "appkit_top_left_conversion_preserves_external_display_coordinates"
  "max_popover_height_respects_display_y_offset"
)

cd "$ROOT"
echo "PeterFan sleep/wake and multi-display regression gate"
echo

for test_name in "${TESTS[@]}"; do
  echo "[run] $test_name"
  env RUSTC="$RUST_BIN/rustc" PATH="$RUST_BIN:$PATH" \
    cargo test --locked -p peterfan-menubar "tests::$test_name" -- --exact --nocapture
done

echo
echo "PASS: ${#TESTS[@]} deterministic recovery/display checks"
