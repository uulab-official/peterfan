#!/usr/bin/env bash
# Verify that this Mac can produce and validate official PeterFan macOS
# release artifacts without printing secrets.

set -euo pipefail

# shellcheck disable=SC1091
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/scripts/load-env.sh"

ARTIFACT="${1:-}"
if [[ -z "$ARTIFACT" ]]; then
  if [[ -d "$PETERFAN_REPO_ROOT/dist" ]]; then
    ARTIFACT=$(
      find "$PETERFAN_REPO_ROOT/dist" \
        \( -path '*/local-release/*/PeterFan-v*.dmg' -o -name 'PeterFan-v*.dmg' -o -name 'PeterFan-*-universal-apple-darwin.dmg' \) \
        -print 2>/dev/null | sort -r | head -n 1
    )
  fi
fi

ok() {
  printf '  \033[32m✓\033[0m %s\n' "$1"
}

warn() {
  printf '  \033[33m!\033[0m %s\n' "$1"
}

fail() {
  printf '  \033[31m✗\033[0m %s\n' "$1"
  FAILED=1
}

have_cmd() {
  command -v "$1" >/dev/null 2>&1
}

FAILED=0

echo "PeterFan macOS release readiness"
echo

echo "Toolchain"
for cmd in git cargo rustup lipo codesign xcrun shasum hdiutil; do
  if have_cmd "$cmd"; then
    ok "$cmd is available"
  else
    fail "$cmd is missing"
  fi
done
if have_cmd gh; then
  ok "gh is available"
else
  warn "gh is missing; release upload will not work from this Mac"
fi

if have_cmd rustup; then
  if rustup target list --installed | grep -qx 'aarch64-apple-darwin'; then
    ok "Rust target installed: aarch64-apple-darwin"
  else
    warn "Rust target missing: aarch64-apple-darwin; release script can install it"
  fi
  if rustup target list --installed | grep -qx 'x86_64-apple-darwin'; then
    ok "Rust target installed: x86_64-apple-darwin"
  else
    warn "Rust target missing: x86_64-apple-darwin; release script can install it"
  fi
fi

echo
echo "Local-only files"
if [[ -f "$PETERFAN_REPO_ROOT/.env" ]]; then
  ok ".env exists"
else
  fail ".env is missing; copy .env.example and fill local values"
fi

if git -C "$PETERFAN_REPO_ROOT" check-ignore -q .env; then
  ok ".env is ignored by git"
else
  fail ".env is not ignored by git"
fi

if git -C "$PETERFAN_REPO_ROOT" check-ignore -q private; then
  ok "private/ is ignored by git"
else
  fail "private/ is not ignored by git"
fi

if git -C "$PETERFAN_REPO_ROOT" check-ignore -q dist; then
  ok "dist/ is ignored by git"
else
  fail "dist/ is not ignored by git"
fi

if [[ -f "$PETERFAN_REPO_ROOT/private/macos-signing/developer-id.key" ]]; then
  ok "local Developer ID private key material exists under private/"
else
  warn "local CSR private key not found under private/; this is fine after Keychain import"
fi

echo
echo "Signing settings"
echo "  Bundle ID: ${PETERFAN_BUNDLE_ID:-kr.co.uulab.peterfan}"
echo "  Team ID:   ${APPLE_TEAM_ID:-not set}"
echo "  Notary:    ${NOTARYTOOL_PROFILE:-not set}"

IDENTITY="${PETERFAN_SIGN_IDENTITY:-}"
if [[ -z "$IDENTITY" && "$(uname)" == "Darwin" ]]; then
  IDENTITY=$(security find-identity -p codesigning -v 2>/dev/null | awk -F\" '/Developer ID Application/ {print $2; exit}')
fi
if [[ -n "$IDENTITY" ]]; then
  ok "Developer ID identity available: $IDENTITY"
else
  fail "Developer ID Application identity is not available in Keychain"
fi

if [[ "$(uname)" == "Darwin" ]] && have_cmd xcrun && [[ -n "${NOTARYTOOL_PROFILE:-}" ]]; then
  if xcrun notarytool history --keychain-profile "$NOTARYTOOL_PROFILE" >/dev/null 2>&1; then
    ok "notarytool keychain profile works: $NOTARYTOOL_PROFILE"
  else
    fail "notarytool keychain profile is missing or invalid: $NOTARYTOOL_PROFILE"
  fi
else
  warn "notary profile check skipped"
fi

echo
echo "Artifact"
if [[ -n "$ARTIFACT" && -e "$ARTIFACT" ]]; then
  echo "  Path: $ARTIFACT"
else
  warn "no DMG artifact found; pass one explicitly or build with scripts/release-local-macos.sh"
  ARTIFACT=""
fi

if [[ -n "$ARTIFACT" && "$(uname)" == "Darwin" ]]; then
  if codesign --verify --verbose=2 "$ARTIFACT" >/dev/null 2>&1; then
    ok "DMG code signature is valid"
  else
    fail "DMG code signature is invalid"
  fi

  if xcrun stapler validate "$ARTIFACT" >/dev/null 2>&1; then
    ok "DMG has a stapled notarization ticket"
  else
    fail "DMG does not have a valid stapled notarization ticket"
  fi

  if spctl -a -vv -t open --context context:primary-signature "$ARTIFACT" >/dev/null 2>&1; then
    ok "Gatekeeper accepts the DMG"
  else
    fail "Gatekeeper rejects the DMG"
  fi

  MOUNT_DIR="$(mktemp -d "${TMPDIR:-/tmp}/peterfan-release-check.XXXXXX")"
  if hdiutil attach -nobrowse -quiet -mountpoint "$MOUNT_DIR" "$ARTIFACT"; then
    VOLUME_NAME=$(diskutil info "$MOUNT_DIR" 2>/dev/null | awk -F: '/Volume Name/ {sub(/^ +/, "", $2); print $2; exit}')
    FS_PERSONALITY=$(diskutil info "$MOUNT_DIR" 2>/dev/null | awk -F: '/File System Personality/ {sub(/^ +/, "", $2); print $2; exit}')
    if [[ "$VOLUME_NAME" == "PeterFan" ]]; then
      ok "DMG volume name is PeterFan"
    else
      fail "DMG volume name mismatch: ${VOLUME_NAME:-missing}"
    fi
    if [[ "$FS_PERSONALITY" == "HFS+" ]]; then
      ok "DMG filesystem is HFS+"
    else
      fail "DMG filesystem should be HFS+ for Finder compatibility, got ${FS_PERSONALITY:-missing}"
    fi
    if [[ -d "$MOUNT_DIR/PeterFan.app" ]]; then
      ok "DMG contains PeterFan.app"
      BUNDLE_ID=$(plutil -extract CFBundleIdentifier raw -o - "$MOUNT_DIR/PeterFan.app/Contents/Info.plist" 2>/dev/null || true)
      if [[ "$BUNDLE_ID" == "${PETERFAN_BUNDLE_ID:-kr.co.uulab.peterfan}" ]]; then
        ok "DMG app bundle id is $BUNDLE_ID"
      else
        fail "DMG app bundle id mismatch: ${BUNDLE_ID:-missing}"
      fi
      if [[ -x "$MOUNT_DIR/PeterFan.app/Contents/MacOS/peterfand" ]]; then
        ok "DMG app bundles peterfand helper"
      else
        fail "DMG app is missing bundled peterfand helper"
      fi
      if xcrun stapler validate "$MOUNT_DIR/PeterFan.app" >/dev/null 2>&1; then
        ok "DMG app has a stapled notarization ticket"
      else
        fail "DMG app does not have a valid stapled notarization ticket"
      fi
      if spctl -a -vv -t exec "$MOUNT_DIR/PeterFan.app" >/dev/null 2>&1; then
        ok "Gatekeeper accepts the app inside the DMG"
      else
        fail "Gatekeeper rejects the app inside the DMG"
      fi
    else
      fail "DMG does not contain PeterFan.app"
    fi
    hdiutil detach -quiet "$MOUNT_DIR" >/dev/null 2>&1 || true
  else
    fail "DMG could not be mounted for inspection"
  fi
  rmdir "$MOUNT_DIR" >/dev/null 2>&1 || true

  shasum -a 256 "$ARTIFACT"
fi

echo
if [[ "$FAILED" == "0" ]]; then
  ok "release machine is ready"
else
  fail "release machine needs attention"
  exit 1
fi
