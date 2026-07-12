#!/usr/bin/env bash
# Build, bundle, and launch PeterFan from this working tree.
#
# Default flow:
#   1. stop any currently running local PeterFan process
#   2. build the menu-bar app and bundled daemon
#   3. assemble dist/PeterFan.app
#   4. launch without stealing focus

set -euo pipefail

MODE="${1:-run}"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_NAME="PeterFan"
BUNDLE_ID="${PETERFAN_BUNDLE_ID:-kr.co.uulab.peterfan}"
APP_BUNDLE="$ROOT_DIR/dist/PeterFan.app"
APP_BINARY="$APP_BUNDLE/Contents/MacOS/PeterFan"

cd "$ROOT_DIR"

stop_running() {
  pkill -x "$APP_NAME" >/dev/null 2>&1 || true
  pkill -x peterfan-menubar >/dev/null 2>&1 || true
}

build_bundle() {
  cargo build --release -p peterfan-menubar -p peterfan-daemon
  scripts/bundle-macos.sh target/release/peterfan-menubar dist
}

open_app() {
  /usr/bin/open -g "$APP_BUNDLE"
}

verify_one_process() {
  local count=""
  for _ in {1..20}; do
    count="$(pgrep -x "$APP_NAME" | wc -l | tr -d ' ')"
    if [[ "$count" == "1" ]]; then
      return 0
    fi
    sleep 0.5
  done
  if [[ "$count" != "1" ]]; then
    echo "expected exactly one $APP_NAME process, found $count" >&2
    pgrep -ax "$APP_NAME|peterfan-menubar" >&2 || true
    return 1
  fi
}

usage() {
  echo "usage: $0 [run|--verify|--logs|--telemetry|--debug|--no-build]" >&2
}

case "$MODE" in
  run)
    stop_running
    build_bundle
    open_app
    ;;
  --verify|verify)
    stop_running
    build_bundle
    open_app
    verify_one_process
    ;;
  --logs|logs)
    stop_running
    build_bundle
    open_app
    /usr/bin/log stream --info --style compact --predicate "process == \"$APP_NAME\""
    ;;
  --telemetry|telemetry)
    stop_running
    build_bundle
    open_app
    /usr/bin/log stream --info --style compact --predicate "subsystem == \"$BUNDLE_ID\""
    ;;
  --debug|debug)
    stop_running
    build_bundle
    lldb -- "$APP_BINARY"
    ;;
  --no-build|no-build)
    stop_running
    if [[ ! -x "$APP_BINARY" ]]; then
      echo "error: $APP_BINARY does not exist; run without --no-build first" >&2
      exit 1
    fi
    open_app
    ;;
  *)
    usage
    exit 2
    ;;
esac
