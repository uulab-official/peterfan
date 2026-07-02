#!/usr/bin/env bash
# Sign a PeterFan macOS binary or .app bundle.
#
# Developer distribution:
#   PETERFAN_SIGN_IDENTITY="Developer ID Application: Your Name (TEAMID)" \
#     scripts/sign-macos.sh dist/PeterFan.app
#
# Local/dev fallback:
#   scripts/sign-macos.sh dist/PeterFan.app
#   # uses ad-hoc signing so local and fork builds still work.

set -euo pipefail

# shellcheck disable=SC1091
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/scripts/load-env.sh"

TARGET="${1:?usage: scripts/sign-macos.sh <path>}"
IDENTITY="${PETERFAN_SIGN_IDENTITY:-${MACOS_SIGN_IDENTITY:--}}"
ENTITLEMENTS="${PETERFAN_ENTITLEMENTS:-${MACOS_ENTITLEMENTS:-}}"

if [[ ! -e "$TARGET" ]]; then
  echo "error: target not found: $TARGET" >&2
  exit 1
fi

CODE_SIGN_ARGS=(--force --sign "$IDENTITY")
if [[ "$IDENTITY" != "-" ]]; then
  CODE_SIGN_ARGS+=(--options runtime --timestamp)
fi
if [[ -n "$ENTITLEMENTS" ]]; then
  CODE_SIGN_ARGS+=(--entitlements "$ENTITLEMENTS")
fi

sign_path() {
  local path="$1"
  codesign "${CODE_SIGN_ARGS[@]}" "$path"
}

if [[ -d "$TARGET" && "$TARGET" == *.app ]]; then
  # Sign nested executables first, then the app bundle. This keeps the bundle
  # seal honest for notarization and avoids relying on --deep to make policy
  # decisions for us.
  while IFS= read -r -d '' exe; do
    sign_path "$exe"
  done < <(find "$TARGET/Contents/MacOS" -type f -perm -111 -print0)
  sign_path "$TARGET"
else
  sign_path "$TARGET"
fi
