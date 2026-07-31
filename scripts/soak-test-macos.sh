#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DURATION="${PETERFAN_SOAK_SECONDS:-21600}"
SAMPLE_INTERVAL="${PETERFAN_SOAK_SAMPLE_SECONDS:-10}"
MAX_AVG_CPU="${PETERFAN_SOAK_MAX_AVG_CPU:-8}"
MAX_PEAK_CPU="${PETERFAN_SOAK_MAX_PEAK_CPU:-35}"
MAX_RSS_KB="${PETERFAN_SOAK_MAX_RSS_KB:-262144}"
MAX_RSS_GROWTH_KB="${PETERFAN_SOAK_MAX_RSS_GROWTH_KB:-65536}"
MAX_THREAD_GROWTH="${PETERFAN_SOAK_MAX_THREAD_GROWTH:-16}"
LOG_DIR="${PETERFAN_SOAK_LOG_DIR:-$ROOT/target/soak}"
PID="${PETERFAN_SOAK_PID:-}"
OWNS_PROCESS=0

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "error: the menu-bar soak test requires macOS" >&2
  exit 2
fi
if ! [[ "$DURATION" =~ ^[0-9]+$ ]] || (( DURATION < 30 )); then
  echo "error: PETERFAN_SOAK_SECONDS must be an integer of at least 30" >&2
  exit 2
fi
if ! [[ "$SAMPLE_INTERVAL" =~ ^[0-9]+$ ]] || (( SAMPLE_INTERVAL < 1 )); then
  echo "error: PETERFAN_SOAK_SAMPLE_SECONDS must be a positive integer" >&2
  exit 2
fi

mkdir -p "$LOG_DIR"
STAMP="$(date +%Y%m%d-%H%M%S)"
CSV="$LOG_DIR/menubar-$STAMP.csv"
APP_LOG="$LOG_DIR/menubar-$STAMP.log"

cleanup() {
  if (( OWNS_PROCESS == 1 )) && kill -0 "$PID" 2>/dev/null; then
    kill -TERM "$PID" 2>/dev/null || true
    wait "$PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT INT TERM

if [[ -z "$PID" ]]; then
  APP="${PETERFAN_MENUBAR_BIN:-$ROOT/target/release/peterfan-menubar}"
  if [[ ! -x "$APP" ]]; then
    echo "error: $APP not found or not executable; run cargo build --release first" >&2
    exit 2
  fi
  "$APP" --mock >"$APP_LOG" 2>&1 &
  PID=$!
  OWNS_PROCESS=1
  sleep 5
fi

if ! kill -0 "$PID" 2>/dev/null; then
  echo "error: PeterFan process $PID is not running" >&2
  exit 1
fi

process_sample() {
  local row cpu rss threads
  row="$(ps -p "$PID" -o %cpu=,rss=)"
  cpu="$(awk '{print $1}' <<<"$row")"
  rss="$(awk '{print $2}' <<<"$row")"
  threads="$(( $(ps -M "$PID" | wc -l | tr -d ' ') - 1 ))"
  printf '%s %s %s\n' "$cpu" "$rss" "$threads"
}

read -r _ INITIAL_RSS INITIAL_THREADS < <(process_sample)
printf 'elapsed_seconds,cpu_percent,rss_kb,threads\n' >"$CSV"

START="$(date +%s)"
SAMPLES=0
CPU_SUM="0"
CPU_PEAK="0"
RSS_PEAK="$INITIAL_RSS"
THREAD_PEAK="$INITIAL_THREADS"

while true; do
  NOW="$(date +%s)"
  ELAPSED="$(( NOW - START ))"
  (( ELAPSED >= DURATION )) && break
  if ! kill -0 "$PID" 2>/dev/null; then
    echo "error: PeterFan exited after ${ELAPSED}s; see $APP_LOG" >&2
    exit 1
  fi

  read -r CPU RSS THREADS < <(process_sample)
  printf '%s,%s,%s,%s\n' "$ELAPSED" "$CPU" "$RSS" "$THREADS" >>"$CSV"
  CPU_SUM="$(awk -v sum="$CPU_SUM" -v value="$CPU" 'BEGIN { printf "%.3f", sum + value }')"
  CPU_PEAK="$(awk -v peak="$CPU_PEAK" -v value="$CPU" 'BEGIN { print (value > peak ? value : peak) }')"
  (( RSS > RSS_PEAK )) && RSS_PEAK="$RSS"
  (( THREADS > THREAD_PEAK )) && THREAD_PEAK="$THREADS"
  SAMPLES="$(( SAMPLES + 1 ))"
  sleep "$SAMPLE_INTERVAL"
done

if (( SAMPLES == 0 )); then
  echo "error: soak test produced no samples" >&2
  exit 1
fi

read -r _ FINAL_RSS FINAL_THREADS < <(process_sample)
AVG_CPU="$(awk -v sum="$CPU_SUM" -v count="$SAMPLES" 'BEGIN { printf "%.2f", sum / count }')"
RSS_GROWTH="$(( FINAL_RSS - INITIAL_RSS ))"
THREAD_GROWTH="$(( FINAL_THREADS - INITIAL_THREADS ))"
FAILURES=0

check_float_max() {
  local label="$1" value="$2" limit="$3"
  if awk -v value="$value" -v limit="$limit" 'BEGIN { exit !(value > limit) }'; then
    echo "FAIL $label: $value > $limit"
    FAILURES="$(( FAILURES + 1 ))"
  else
    echo "PASS $label: $value <= $limit"
  fi
}

check_int_max() {
  local label="$1" value="$2" limit="$3"
  if (( value > limit )); then
    echo "FAIL $label: $value > $limit"
    FAILURES="$(( FAILURES + 1 ))"
  else
    echo "PASS $label: $value <= $limit"
  fi
}

echo "PeterFan macOS soak result"
echo "  pid=$PID duration=${DURATION}s samples=$SAMPLES csv=$CSV"
check_float_max "average CPU %" "$AVG_CPU" "$MAX_AVG_CPU"
check_float_max "peak CPU %" "$CPU_PEAK" "$MAX_PEAK_CPU"
check_int_max "peak RSS KB" "$RSS_PEAK" "$MAX_RSS_KB"
check_int_max "RSS growth KB" "$RSS_GROWTH" "$MAX_RSS_GROWTH_KB"
check_int_max "thread growth" "$THREAD_GROWTH" "$MAX_THREAD_GROWTH"

if (( FAILURES > 0 )); then
  echo "$FAILURES soak threshold(s) failed" >&2
  exit 1
fi
echo "all soak thresholds passed"
