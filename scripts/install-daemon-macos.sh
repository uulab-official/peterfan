#!/usr/bin/env bash
# Install peterfand as a root LaunchDaemon so fan control runs continuously
# (with restore-on-exit + critical-temp safety) and without per-command sudo.
#
#   sudo scripts/install-daemon-macos.sh [path-to-peterfand]
#
# Uninstall with scripts/uninstall-daemon-macos.sh.
set -euo pipefail

# shellcheck disable=SC1091
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/scripts/load-env.sh"

if [[ "$(id -u)" -ne 0 ]]; then
  echo "error: run with sudo (this installs a system LaunchDaemon)" >&2
  exit 1
fi

BIN="${1:-target/release/peterfand}"
LABEL="${PETERFAN_DAEMON_LABEL:-kr.co.uulab.peterfan.daemon}"
LEGACY_LABEL="com.uulab.peterfan.daemon"
PLIST_SRC="packaging/$LABEL.plist"
PLIST_DST="/Library/LaunchDaemons/$LABEL.plist"
LEGACY_PLIST_DST="/Library/LaunchDaemons/$LEGACY_LABEL.plist"

[[ -x "$BIN" ]] || { echo "error: peterfand not found at '$BIN' (cargo build --release -p peterfan-daemon)" >&2; exit 1; }
[[ -f "$PLIST_SRC" ]] || { echo "error: plist not found at '$PLIST_SRC'" >&2; exit 1; }

install -m 755 "$BIN" /usr/local/bin/peterfand
launchctl bootout system "$LEGACY_PLIST_DST" 2>/dev/null || true
rm -f "$LEGACY_PLIST_DST"
install -m 644 "$PLIST_SRC" "$PLIST_DST"
chown root:wheel "$PLIST_DST"

# Reload if already present.
launchctl bootout system "$PLIST_DST" 2>/dev/null || true
launchctl bootstrap system "$PLIST_DST"

echo "installed: $LABEL (running as root; logs at /var/log/peterfand.log)"
