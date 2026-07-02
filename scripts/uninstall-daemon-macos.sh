#!/usr/bin/env bash
# Remove the peterfand LaunchDaemon.   sudo scripts/uninstall-daemon-macos.sh
set -euo pipefail

# shellcheck disable=SC1091
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/scripts/load-env.sh"

if [[ "$(id -u)" -ne 0 ]]; then
  echo "error: run with sudo" >&2
  exit 1
fi

LABEL="${PETERFAN_DAEMON_LABEL:-kr.co.uulab.peterfan.daemon}"
LEGACY_LABEL="com.uulab.peterfan.daemon"
PLIST_DST="/Library/LaunchDaemons/$LABEL.plist"
LEGACY_PLIST_DST="/Library/LaunchDaemons/$LEGACY_LABEL.plist"

launchctl bootout system "$PLIST_DST" 2>/dev/null || true
launchctl bootout system "$LEGACY_PLIST_DST" 2>/dev/null || true
rm -f "$PLIST_DST" "$LEGACY_PLIST_DST" /usr/local/bin/peterfand
echo "removed $LABEL"
