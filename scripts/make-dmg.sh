#!/usr/bin/env bash
# Package PeterFan.app into a distributable .dmg — double-click to mount,
# drag PeterFan.app onto the Applications shortcut, done. No Terminal needed,
# unlike the .tar.gz (which is still produced for CLI/TUI users who just want
# the loose binaries).
#
# Usage:
#   scripts/make-dmg.sh [APP_PATH] [OUTPUT_DMG]
#     APP_PATH    path to PeterFan.app (default: dist/PeterFan.app)
#     OUTPUT_DMG  where to write the .dmg (default: dist/PeterFan.dmg)

set -euo pipefail

APP="${1:-dist/PeterFan.app}"
OUT="${2:-dist/PeterFan.dmg}"

if [[ ! -d "$APP" ]]; then
  echo "error: app bundle not found at '$APP' (build it first: scripts/bundle-macos.sh)" >&2
  exit 1
fi

STAGING="$(mktemp -d)"
trap 'rm -rf "$STAGING"' EXIT

cp -R "$APP" "$STAGING/"
ln -s /Applications "$STAGING/Applications"

rm -f "$OUT"
hdiutil create -volname "PeterFan" -srcfolder "$STAGING" -ov -format UDZO "$OUT" >/dev/null

echo "built $OUT"
