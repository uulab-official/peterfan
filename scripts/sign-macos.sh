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
  local attempt
  for attempt in 1 2 3; do
    if codesign "${CODE_SIGN_ARGS[@]}" "$path"; then
      return 0
    fi
    if [[ "$attempt" == "3" ]]; then
      break
    fi
    echo "codesign failed for $path; retrying in ${attempt}s..." >&2
    sleep "$attempt"
  done
  return 1
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
