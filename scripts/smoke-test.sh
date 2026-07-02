#!/usr/bin/env bash
# End-to-end smoke test — catches the bug class unit tests can't:
# a process that hangs instead of exiting, crashes on launch, or produces
# malformed output. Every one of these has shipped at least once:
#   - peterfan-menubar ignored --version and launched as a live GUI app
#     instead of printing and exiting (found by a human noticing two menu-bar
#     icons; a 5-second timeout here catches it in <5s instead).
#   - peterfand panicking on startup with a given flag combination.
#   - a CLI subcommand crashing only in --mock mode (or only without it).
#
# Run before every release: scripts/smoke-test.sh
# CI: wired into .github/workflows/ci.yml.

set -uo pipefail

BIN_DIR="${1:-target/release}"
FAILURES=0
TESTED=0

pass() { printf '  \033[32m✓\033[0m %s\n' "$1"; TESTED=$((TESTED+1)); }
fail() { printf '  \033[31m✗\033[0m %s\n' "$1"; FAILURES=$((FAILURES+1)); TESTED=$((TESTED+1)); }

# Run a command with a hard timeout; fail if it doesn't exit in time (catches
# "should print and exit" commands that instead hang or launch a GUI).
run_bounded() {
    local desc="$1" timeout_secs="$2"
    shift 2
    if timeout "$timeout_secs" "$@" >/tmp/smoke_out.$$ 2>/tmp/smoke_err.$$; then
        pass "$desc"
    else
        local code=$?
        if [[ $code -eq 124 ]]; then
            fail "$desc (timed out after ${timeout_secs}s — hung instead of exiting)"
        else
            fail "$desc (exit $code)"
            sed 's/^/      /' /tmp/smoke_err.$$
        fi
    fi
    rm -f /tmp/smoke_out.$$ /tmp/smoke_err.$$
}

# Same, but also checks the output contains a substring.
run_bounded_contains() {
    local desc="$1" timeout_secs="$2" needle="$3"
    shift 3
    if timeout "$timeout_secs" "$@" >/tmp/smoke_out.$$ 2>/tmp/smoke_err.$$; then
        if grep -q "$needle" /tmp/smoke_out.$$; then
            pass "$desc"
        else
            fail "$desc (missing expected output: '$needle')"
            sed 's/^/      /' /tmp/smoke_out.$$
        fi
    else
        local code=$?
        if [[ $code -eq 124 ]]; then
            fail "$desc (timed out after ${timeout_secs}s — hung instead of exiting)"
        else
            fail "$desc (exit $code)"
            sed 's/^/      /' /tmp/smoke_err.$$
        fi
    fi
    rm -f /tmp/smoke_out.$$ /tmp/smoke_err.$$
}

# Runs a command and validates its stdout is well-formed JSON.
run_json() {
    local desc="$1" timeout_secs="$2"
    shift 2
    local out
    out=$(timeout "$timeout_secs" "$@" 2>/tmp/smoke_err.$$)
    local code=$?
    if [[ $code -ne 0 ]]; then
        if [[ $code -eq 124 ]]; then
            fail "$desc (timed out after ${timeout_secs}s)"
        else
            fail "$desc (exit $code)"
            sed 's/^/      /' /tmp/smoke_err.$$
        fi
    elif echo "$out" | python3 -c 'import json,sys; json.load(sys.stdin)' 2>/dev/null; then
        pass "$desc"
    else
        fail "$desc (invalid JSON)"
        echo "$out" | head -5 | sed 's/^/      /'
    fi
    rm -f /tmp/smoke_err.$$
}

# Starts a long-running process, confirms it's still alive after a delay
# (didn't crash on startup), then kills it and confirms it exits promptly.
run_lifecycle() {
    local desc="$1" alive_secs="$2"
    shift 2
    "$@" >/tmp/smoke_lifecycle.$$ 2>&1 &
    local pid=$!
    sleep "$alive_secs"
    if kill -0 "$pid" 2>/dev/null; then
        kill "$pid" 2>/dev/null
        wait "$pid" 2>/dev/null
        pass "$desc (alive after ${alive_secs}s, exited cleanly on SIGTERM)"
    else
        fail "$desc (crashed within ${alive_secs}s)"
        sed 's/^/      /' /tmp/smoke_lifecycle.$$
    fi
    rm -f /tmp/smoke_lifecycle.$$
}

PETERFAN="$BIN_DIR/peterfan"
PETERFAND="$BIN_DIR/peterfand"
PETERFAN_TUI="$BIN_DIR/peterfan-tui"
PETERFAN_MENUBAR="$BIN_DIR/peterfan-menubar"

for bin in "$PETERFAN" "$PETERFAND" "$PETERFAN_TUI" "$PETERFAN_MENUBAR"; do
    if [[ ! -x "$bin" ]]; then
        echo "error: $bin not found or not executable — build first: cargo build --release" >&2
        exit 1
    fi
done

echo "== --version / --help must print and exit, never hang or launch a GUI =="
run_bounded_contains "peterfan --version"        5 "peterfan" "$PETERFAN" --version
run_bounded_contains "peterfand --version"       5 "peterfand" "$PETERFAND" --version
run_bounded_contains "peterfan-tui --version"    5 "peterfan-tui" "$PETERFAN_TUI" --version
run_bounded_contains "peterfan-menubar --version" 5 "peterfan-menubar" "$PETERFAN_MENUBAR" --version
run_bounded "peterfan --help"          5 "$PETERFAN" --help
run_bounded "peterfan-menubar --help"  5 "$PETERFAN_MENUBAR" --help

echo "== read-only CLI commands must not crash (--mock) =="
for cmd in status cpu memory disk network "top -n 5" battery system temps fans hardware doctor config "curve balanced" "profile list"; do
    run_bounded "peterfan --mock $cmd" 10 "$PETERFAN" --mock $cmd
done

echo "== --json output must be valid JSON =="
for cmd in status cpu memory disk network temps fans hardware; do
    run_json "peterfan --mock --json $cmd" 10 "$PETERFAN" --mock --json $cmd
done

echo "== daemon one-shot run must apply a curve and exit cleanly =="
run_bounded_contains "peterfand --mock --once" 10 "restored" "$PETERFAND" --mock --once

echo "== menu-bar app must survive startup and shut down on signal (--mock) =="
run_lifecycle "peterfan-menubar --mock" 3 "$PETERFAN_MENUBAR" --mock

if [[ "$(uname)" == "Darwin" ]]; then
    echo "== --mock must never trigger the real first-run setup dialog =="
    "$PETERFAN_MENUBAR" --mock >/dev/null 2>&1 &
    mb_pid=$!
    sleep 2
    if pgrep -f osascript >/dev/null; then
        fail "peterfan-menubar --mock spawned osascript (would pop a real dialog / trigger a real privileged install)"
        pkill -f osascript 2>/dev/null
    else
        pass "peterfan-menubar --mock (no osascript spawned)"
    fi
    kill "$mb_pid" 2>/dev/null
    wait "$mb_pid" 2>/dev/null
fi

if [[ "$(uname)" == "Darwin" && -f scripts/bundle-macos.sh ]]; then
    echo "== app bundle must include peterfand (needed by 'Enable Fan Control') =="
    TMP_BUNDLE_DIR=$(mktemp -d)
    if VERSION=0.0.0-smoketest scripts/bundle-macos.sh "$PETERFAN_MENUBAR" "$TMP_BUNDLE_DIR" >/dev/null 2>&1 \
        && [[ -x "$TMP_BUNDLE_DIR/PeterFan.app/Contents/MacOS/peterfand" ]]; then
        pass "PeterFan.app bundles peterfand"
    else
        fail "PeterFan.app does NOT bundle peterfand — 'Enable Fan Control' menu item would fail silently"
    fi

    echo "== app bundle must be signed (Developer ID in releases, ad-hoc in local/fork builds) =="
    # Captured into a variable rather than piped straight into `grep -q`:
    # under `pipefail`, grep's early exit on first match can SIGPIPE the
    # still-writing codesign process, which then looks like a failure even
    # though the signature check actually matched.
    codesign_output=$(codesign -dv "$TMP_BUNDLE_DIR/PeterFan.app" 2>&1 || true)
    if codesign --verify --deep --strict "$TMP_BUNDLE_DIR/PeterFan.app" >/dev/null 2>&1; then
        if echo "$codesign_output" | grep -q "adhoc"; then
            pass "PeterFan.app is ad-hoc signed"
        else
            pass "PeterFan.app is signed"
        fi
    else
        fail "PeterFan.app is NOT signed — downloaded copies will show 'is damaged and can't be opened'"
    fi

    if [[ -f scripts/make-dmg.sh ]]; then
        echo "== .dmg must build and contain the app + Applications shortcut + Gatekeeper helper =="
        if scripts/make-dmg.sh "$TMP_BUNDLE_DIR/PeterFan.app" "$TMP_BUNDLE_DIR/PeterFan.dmg" >/dev/null 2>&1; then
            MOUNT_DIR=$(mktemp -d)
            if hdiutil attach "$TMP_BUNDLE_DIR/PeterFan.dmg" -nobrowse -mountpoint "$MOUNT_DIR" >/dev/null 2>&1; then
                if [[ -d "$MOUNT_DIR/PeterFan.app" && -L "$MOUNT_DIR/Applications" && -x "$MOUNT_DIR/Open PeterFan if macOS blocks it.command" ]]; then
                    pass "PeterFan.dmg mounts with PeterFan.app + Applications shortcut + Gatekeeper helper"
                else
                    fail "PeterFan.dmg is missing the app bundle, Applications shortcut, or Gatekeeper helper"
                fi
                hdiutil detach "$MOUNT_DIR" >/dev/null 2>&1
            else
                fail "PeterFan.dmg built but failed to mount"
            fi
            rmdir "$MOUNT_DIR" 2>/dev/null
        else
            fail "scripts/make-dmg.sh failed to build a .dmg from the bundled app"
        fi
    fi

    rm -rf "$TMP_BUNDLE_DIR"
fi

echo
echo "── $TESTED checks, $FAILURES failed ──"
if [[ $FAILURES -gt 0 ]]; then
    exit 1
fi
