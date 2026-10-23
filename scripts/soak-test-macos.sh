#!/usr/bin/env bash
# Observes PETERFAN_SOAK_PID without stopping it; otherwise owns a --mock child.
# PETERFAN_SOAK_* thresholds and PETERFAN_MENUBAR_BIN are parsed by the helper.
# Receipt records interval CPU, process identity, host load and config preservation.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
if ! command -v python3 >/dev/null 2>&1; then
  echo 'error: Python 3 is required for the macOS soak observer' >&2
  exit 2
fi
exec python3 "$ROOT/scripts/macos-soak.py" "$@"
