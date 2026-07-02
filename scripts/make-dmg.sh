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

# shellcheck disable=SC1091
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/scripts/load-env.sh"

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

FIXER="scripts/dmg-fix-gatekeeper.command"
if [[ -f "$FIXER" ]]; then
  cp "$FIXER" "$STAGING/Open PeterFan if macOS blocks it.command"
  chmod +x "$STAGING/Open PeterFan if macOS blocks it.command"
fi

rm -f "$OUT"
hdiutil create \
  -volname "PeterFan" \
  -fs HFS+ \
  -srcfolder "$STAGING" \
  -ov \
  -format UDZO \
  "$OUT" >/dev/null

IDENTITY="${PETERFAN_SIGN_IDENTITY:-${MACOS_SIGN_IDENTITY:-}}"
if [[ -n "$IDENTITY" && "$IDENTITY" != "-" ]] && command -v codesign >/dev/null 2>&1; then
  codesign --force --sign "$IDENTITY" --timestamp "$OUT"
fi

echo "built $OUT"
