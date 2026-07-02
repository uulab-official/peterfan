#!/usr/bin/env bash
# Submit a signed macOS artifact to Apple's notary service and staple the
# resulting ticket. Supports .dmg, .zip, .pkg, and signed .app bundles.
#
# Authentication options, checked in this order:
#   NOTARYTOOL_PROFILE
#   APPLE_API_KEY_ID + APPLE_API_ISSUER_ID + APPLE_API_KEY_PATH
#   APPLE_ID + APPLE_TEAM_ID + APPLE_APP_SPECIFIC_PASSWORD

set -euo pipefail

# shellcheck disable=SC1091
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/scripts/load-env.sh"

ARTIFACT="${1:?usage: scripts/notarize-macos.sh <artifact>}"

if [[ ! -e "$ARTIFACT" ]]; then
  echo "error: artifact not found: $ARTIFACT" >&2
  exit 1
fi
if ! command -v xcrun >/dev/null 2>&1; then
  echo "error: xcrun not found; notarization requires Xcode command line tools" >&2
  exit 1
fi

AUTH_ARGS=()
if [[ -n "${NOTARYTOOL_PROFILE:-}" ]]; then
  AUTH_ARGS=(--keychain-profile "$NOTARYTOOL_PROFILE")
elif [[ -n "${APPLE_API_KEY_ID:-}" && -n "${APPLE_API_ISSUER_ID:-}" && -n "${APPLE_API_KEY_PATH:-}" ]]; then
  AUTH_ARGS=(--key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY_ID" --issuer "$APPLE_API_ISSUER_ID")
elif [[ -n "${APPLE_ID:-}" && -n "${APPLE_TEAM_ID:-}" && -n "${APPLE_APP_SPECIFIC_PASSWORD:-}" ]]; then
  AUTH_ARGS=(--apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" --password "$APPLE_APP_SPECIFIC_PASSWORD")
else
  cat >&2 <<'EOF'
error: no notarization credentials configured.

Set one of:
  NOTARYTOOL_PROFILE
  APPLE_API_KEY_ID + APPLE_API_ISSUER_ID + APPLE_API_KEY_PATH
  APPLE_ID + APPLE_TEAM_ID + APPLE_APP_SPECIFIC_PASSWORD
EOF
  exit 1
fi

SUBMIT_ARTIFACT="$ARTIFACT"
TMP_ZIP=""
cleanup() {
  if [[ -n "$TMP_ZIP" ]]; then
    rm -f "$TMP_ZIP"
  fi
}
trap cleanup EXIT

if [[ -d "$ARTIFACT" && "$ARTIFACT" == *.app ]]; then
  TMP_ZIP="$(mktemp "${TMPDIR:-/tmp}/peterfan-notary.XXXXXX.zip")"
  ditto -c -k --keepParent "$ARTIFACT" "$TMP_ZIP"
  SUBMIT_ARTIFACT="$TMP_ZIP"
fi

echo "submitting $ARTIFACT to Apple notary service..."
set +e
SUBMIT_OUTPUT=$(xcrun notarytool submit "$SUBMIT_ARTIFACT" "${AUTH_ARGS[@]}" --wait 2>&1)
STATUS=$?
set -e
printf '%s\n' "$SUBMIT_OUTPUT"

if [[ $STATUS -ne 0 ]]; then
  SUBMISSION_ID=$(printf '%s\n' "$SUBMIT_OUTPUT" | awk '/id:/ {print $2; exit}')
  if [[ -n "$SUBMISSION_ID" ]]; then
    echo "notarization failed; fetching log for $SUBMISSION_ID..." >&2
    xcrun notarytool log "$SUBMISSION_ID" "${AUTH_ARGS[@]}" >&2 || true
  fi
  exit "$STATUS"
fi

echo "stapling notarization ticket..."
xcrun stapler staple "$ARTIFACT"
xcrun stapler validate "$ARTIFACT"
echo "notarized and stapled: $ARTIFACT"
