#!/usr/bin/env bash
# Build, sign, notarize, and upload macOS release assets from this Mac.
#
# This intentionally keeps Apple signing material out of GitHub Actions:
# the Developer ID certificate stays in the local Keychain, notary credentials
# stay in the local Keychain or environment, and only finished artifacts are
# uploaded to GitHub Releases via `gh`.
#
# Usage:
#   scripts/release-local-macos.sh v1.26.9 [--draft] [--no-notarize] [--no-upload]
#
# Recommended one-time notary setup:
#   xcrun notarytool store-credentials peterfan-notary \
#     --apple-id you@example.com --team-id TEAMID --password app-specific-password
#   export NOTARYTOOL_PROFILE=peterfan-notary

set -euo pipefail

# shellcheck disable=SC1091
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/scripts/load-env.sh"

TAG=""
DRAFT=0
NOTARIZE=1
UPLOAD=1

while [[ $# -gt 0 ]]; do
  case "$1" in
    --draft)
      DRAFT=1
      shift
      ;;
    --no-notarize)
      NOTARIZE=0
      shift
      ;;
    --no-upload)
      UPLOAD=0
      shift
      ;;
    v*)
      TAG="$1"
      shift
      ;;
    *)
      echo "usage: scripts/release-local-macos.sh vX.Y.Z [--draft] [--no-notarize] [--no-upload]" >&2
      exit 2
      ;;
  esac
done

if [[ -z "$TAG" ]]; then
  echo "usage: scripts/release-local-macos.sh vX.Y.Z [--draft] [--no-notarize] [--no-upload]" >&2
  exit 2
fi

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "error: required command not found: $1" >&2
    exit 1
  }
}

require_cmd cargo
require_cmd jq
require_cmd lipo
require_cmd codesign
require_cmd tar
require_cmd shasum
if [[ "$UPLOAD" == "1" ]]; then
  require_cmd gh
fi

if [[ "$(uname)" != "Darwin" ]]; then
  echo "error: local macOS releases must be built on macOS" >&2
  exit 1
fi

if [[ "$TAG" != v* ]]; then
  echo "error: tag must start with v (example: v1.26.9)" >&2
  exit 1
fi

VER="${TAG#v}"
MANIFEST_VERSION=$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[] | select(.name=="peterfan-menubar") | .version')
if [[ "$VER" != "$MANIFEST_VERSION" ]]; then
  echo "error: tag $TAG does not match Cargo package version $MANIFEST_VERSION" >&2
  exit 1
fi

if ! git rev-parse "$TAG" >/dev/null 2>&1; then
  echo "error: git tag $TAG does not exist locally" >&2
  exit 1
fi

if [[ "$(git rev-parse "$TAG")" != "$(git rev-parse HEAD)" ]]; then
  echo "error: git tag $TAG does not point at HEAD" >&2
  echo "       tag the release commit first, then rerun this script." >&2
  exit 1
fi

if [[ -n "$(git status --porcelain)" ]]; then
  echo "error: working tree is dirty; commit or stash changes before releasing" >&2
  exit 1
fi

IDENTITY="${PETERFAN_SIGN_IDENTITY:-}"
if [[ -z "$IDENTITY" ]]; then
  IDENTITY=$(security find-identity -p codesigning -v | awk -F\" '/Developer ID Application/ {print $2; exit}')
fi
if [[ -z "$IDENTITY" ]]; then
  echo "error: no Developer ID Application identity found." >&2
  echo "       Create/export one from Apple Developer, install it in Keychain, or set PETERFAN_SIGN_IDENTITY." >&2
  exit 1
fi
export PETERFAN_SIGN_IDENTITY="$IDENTITY"
echo "Using signing identity: $PETERFAN_SIGN_IDENTITY"

if [[ "$NOTARIZE" == "1" ]]; then
  if [[ -z "${NOTARYTOOL_PROFILE:-}" && ( -z "${APPLE_API_KEY_ID:-}" || -z "${APPLE_API_ISSUER_ID:-}" || -z "${APPLE_API_KEY_PATH:-}" ) && ( -z "${APPLE_ID:-}" || -z "${APPLE_TEAM_ID:-}" || -z "${APPLE_APP_SPECIFIC_PASSWORD:-}" ) ]]; then
    cat >&2 <<'EOF'
error: no notarization credentials configured.

Recommended:
  xcrun notarytool store-credentials peterfan-notary \
    --apple-id you@example.com --team-id TEAMID --password app-specific-password
  export NOTARYTOOL_PROFILE=peterfan-notary

Or rerun with --no-notarize for a local packaging dry run.
EOF
    exit 1
  fi
fi

rustup target add aarch64-apple-darwin x86_64-apple-darwin

OUT_ROOT="dist/local-release/$TAG"
NAME="peterfan-${TAG}-universal-apple-darwin"
PKG_DIR="$OUT_ROOT/$NAME"
rm -rf "$OUT_ROOT"
mkdir -p "$PKG_DIR" "$OUT_ROOT/bins/universal"

echo "Building arm64..."
cargo build --release --target aarch64-apple-darwin --bins
echo "Building x86_64..."
cargo build --release --target x86_64-apple-darwin --bins

for bin in peterfan peterfan-tui peterfan-menubar peterfand; do
  lipo -create \
    "target/aarch64-apple-darwin/release/$bin" \
    "target/x86_64-apple-darwin/release/$bin" \
    -output "$OUT_ROOT/bins/universal/$bin"
  chmod +x "$OUT_ROOT/bins/universal/$bin"
  cp "$OUT_ROOT/bins/universal/$bin" "$PKG_DIR/$bin"
  scripts/sign-macos.sh "$PKG_DIR/$bin"
done

cp README.md LICENSE CHANGELOG.md "$PKG_DIR/"

VERSION="$VER" scripts/bundle-macos.sh "$OUT_ROOT/bins/universal/peterfan-menubar" "$PKG_DIR"

if [[ "$NOTARIZE" == "1" ]]; then
  scripts/notarize-macos.sh "$PKG_DIR/PeterFan.app"
fi

tar -czf "$OUT_ROOT/${NAME}.tar.gz" -C "$OUT_ROOT" "$NAME"

DMG="$OUT_ROOT/PeterFan-${TAG}.dmg"
scripts/make-dmg.sh "$PKG_DIR/PeterFan.app" "$DMG"
if [[ "$NOTARIZE" == "1" ]]; then
  scripts/notarize-macos.sh "$DMG"
  scripts/check-macos-release.sh "$DMG"
fi

(cd "$OUT_ROOT" && shasum -a 256 "${NAME}.tar.gz" "PeterFan-${TAG}.dmg" > checksums.txt)

echo "Built release assets:"
ls -lh "$OUT_ROOT/${NAME}.tar.gz" "$DMG" "$OUT_ROOT/checksums.txt"

if [[ "$UPLOAD" != "1" ]]; then
  echo "Skipping GitHub upload (--no-upload)."
  exit 0
fi

if ! gh auth status >/dev/null 2>&1; then
  echo "error: gh is not authenticated. Run: gh auth login" >&2
  exit 1
fi

NOTES=$(awk "/^## \\[$VER\\]/{found=1; next} found && /^## \\[/{exit} found{print}" CHANGELOG.md)
if [[ -z "$NOTES" ]]; then
  NOTES="See CHANGELOG.md for details."
fi
NOTES_FILE="$OUT_ROOT/release-notes.md"
printf '%s\n' "$NOTES" > "$NOTES_FILE"

if gh release view "$TAG" >/dev/null 2>&1; then
  gh release upload "$TAG" \
    "$OUT_ROOT/${NAME}.tar.gz" \
    "$DMG" \
    "$OUT_ROOT/checksums.txt" \
    --clobber
else
  CREATE_ARGS=()
  if [[ "$DRAFT" == "1" ]]; then
    CREATE_ARGS+=(--draft)
  fi
  gh release create "$TAG" \
    "$OUT_ROOT/${NAME}.tar.gz" \
    "$DMG" \
    "$OUT_ROOT/checksums.txt" \
    --title "PeterFan $TAG" \
    --notes-file "$NOTES_FILE" \
    "${CREATE_ARGS[@]}"
fi

echo "Uploaded GitHub Release assets for $TAG"
