#!/usr/bin/env bash
# Build and launch only this checkout's local macOS app. Never stop an installed
# app or another checkout. Contributor checks do not invoke this launcher.
set -euo pipefail

MODE="${1:-run}"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
APP_NAME="PeterFan"
BUNDLE_ID="${PETERFAN_BUNDLE_ID:-kr.co.uulab.peterfan}"
APP_BUNDLE="$ROOT_DIR/dist/PeterFan.app"
APP_BINARY="$APP_BUNDLE/Contents/MacOS/PeterFan"
RAW_BINARY="$ROOT_DIR/target/release/peterfan-menubar"

usage() {
  echo "usage: $0 [run|--verify|--logs|--telemetry|--debug|--no-build|--help]"
  echo 'macOS only. Builds before stopping this checkout’s app; other instances are never stopped.'
  echo '--no-build requires an existing executable bundle. --verify checks only this checkout.'
}
case "$MODE" in
  -h|--help) usage; exit 0 ;;
  run|--verify|verify|--logs|logs|--telemetry|telemetry|--debug|debug|--no-build|no-build) ;;
  *) usage >&2; exit 2 ;;
esac
if [[ $# -gt 1 ]]; then usage >&2; exit 2; fi
if [[ "$(uname -s)" != Darwin ]]; then
  echo 'error: this launcher requires macOS; use the CLI/mock contributor workflow on other platforms.' >&2
  exit 1
fi
cd "$ROOT_DIR"

is_owned() { [[ "$1" == "$APP_BINARY" || "$1" == "$RAW_BINARY" ]]; }

# Darwin comm is the executable path; -ww prevents display-width truncation.
# read's final variable preserves spaces in paths. Never match command arguments
# or interpolate checkout paths into regular expressions.
processes() { ps -ww -axo pid=,comm=; }
owned_pids() {
  local pid executable inventory
  inventory="$(processes)" || return 1
  while read -r pid executable; do
    [[ "$pid" =~ ^[0-9]+$ ]] || continue
    if is_owned "$executable"; then echo "$pid"; fi
  done <<< "$inventory"
}
reject_other_instances() {
  local pid executable found=0 inventory
  inventory="$(processes)" || { echo 'error: cannot inspect running applications safely.' >&2; return 1; }
  while read -r pid executable; do
    [[ "$pid" =~ ^[0-9]+$ ]] || continue
    case "$executable" in
      */PeterFan|*/peterfan-menubar)
        if ! is_owned "$executable"; then
          echo "error: another PeterFan instance is running (PID $pid): $executable" >&2
          found=1
        fi ;;
    esac
  done <<< "$inventory"
  if [[ "$found" == 1 ]]; then
    echo 'Quit that instance yourself before launching this checkout; it has not been stopped.' >&2
    return 1
  fi
}
stop_owned() {
  local pid executable remaining inventory
  inventory="$(owned_pids)" || return 1
  while read -r pid; do
    [[ -n "$pid" ]] || continue
    # Re-read before signaling, so a stale inventory cannot target another app.
    executable="$(ps -ww -p "$pid" -o comm= 2>/dev/null || true)"
    if is_owned "$executable"; then kill -TERM "$pid" 2>/dev/null || true; fi
  done <<< "$inventory"
  for _ in {1..20}; do
    remaining="$(owned_pids)" || return 1
    [[ -z "$remaining" ]] && return 0
    sleep 0.1
  done
  echo 'error: this checkout’s app did not exit; quit it before trying again.' >&2
  return 1
}
verify_one_process() {
  local count
  for _ in {1..20}; do
    count="$(owned_pids | awk 'NF { n++ } END { print n+0 }')"
    [[ "$count" == 1 ]] && return 0
    sleep 0.5
  done
  echo "error: expected exactly one process from this checkout, found $count" >&2
  return 1
}

if [[ "$MODE" == --no-build || "$MODE" == no-build ]]; then
  if [[ ! -x "$APP_BINARY" ]]; then
    echo "error: executable not found: $APP_BINARY; run without --no-build first" >&2
    exit 1
  fi
else
  # Compile errors leave every running app untouched.
  cargo +stable build --locked --release -p peterfan-menubar -p peterfan-daemon
fi
reject_other_instances
stop_owned
if [[ "$MODE" != --no-build && "$MODE" != no-build ]]; then
  scripts/bundle-macos.sh target/release/peterfan-menubar dist
fi
if [[ ! -x "$APP_BINARY" ]]; then
  echo "error: bundle does not contain an executable: $APP_BINARY" >&2
  exit 1
fi
# Recheck after building/bundling in case an unrelated instance started meanwhile.
reject_other_instances
case "$MODE" in
  --debug|debug) lldb -- "$APP_BINARY" ;;
  *)
    open -g "$APP_BUNDLE"
    case "$MODE" in
      --verify|verify) verify_one_process ;;
      --logs|logs) /usr/bin/log stream --info --style compact --predicate "process == \"$APP_NAME\"" ;;
      --telemetry|telemetry) /usr/bin/log stream --info --style compact --predicate "subsystem == \"$BUNDLE_ID\"" ;;
    esac ;;
esac
