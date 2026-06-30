#!/usr/bin/env bash
# Remove the peterfand LaunchDaemon.   sudo scripts/uninstall-daemon-macos.sh
set -euo pipefail

if [[ "$(id -u)" -ne 0 ]]; then
  echo "error: run with sudo" >&2
  exit 1
fi

LABEL="com.uulab.peterfan.daemon"
PLIST_DST="/Library/LaunchDaemons/$LABEL.plist"

launchctl bootout system "$PLIST_DST" 2>/dev/null || true
rm -f "$PLIST_DST" /usr/local/bin/peterfand
echo "removed $LABEL"
