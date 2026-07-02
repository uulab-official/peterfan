#!/usr/bin/env bash
# Local Developer ID setup helper for PeterFan.
#
# Flow:
#   1. scripts/setup-macos-signing.sh csr
#   2. Upload private/macos-signing/CertificateSigningRequest.certSigningRequest
#      to Apple Developer > Certificates > Developer ID Application.
#   3. Download the .cer from Apple.
#   4. scripts/setup-macos-signing.sh import ~/Downloads/developerID_application.cer
#   5. scripts/setup-macos-signing.sh notary

set -euo pipefail

# shellcheck disable=SC1091
source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/scripts/load-env.sh"

CMD="${1:-check}"
REPO_ROOT="$PETERFAN_REPO_ROOT"
PRIVATE_DIR="$REPO_ROOT/private/macos-signing"
KEY_PATH="$PRIVATE_DIR/developer-id.key"
CSR_PATH="$PRIVATE_DIR/CertificateSigningRequest.certSigningRequest"
ENV_PATH="$REPO_ROOT/.env"

usage() {
  cat <<'EOF'
usage:
  scripts/setup-macos-signing.sh check
  scripts/setup-macos-signing.sh teams
  scripts/setup-macos-signing.sh csr
  scripts/setup-macos-signing.sh import [downloaded-developer-id.cer]
  scripts/setup-macos-signing.sh notary

This keeps signing material local:
  - .env is gitignored
  - private/ is gitignored
  - Developer ID private key stays on this Mac
EOF
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "error: required command not found: $1" >&2
    exit 1
  }
}

set_env_value() {
  local key="$1"
  local value="$2"
  touch "$ENV_PATH"
  if grep -q "^${key}=" "$ENV_PATH"; then
    KEY="$key" VALUE="$value" perl -0pi -e 's/^\Q$ENV{KEY}\E=.*$/$ENV{KEY} . "=\"" . $ENV{VALUE} . "\""/meg' "$ENV_PATH"
  else
    printf '%s="%s"\n' "$key" "$value" >> "$ENV_PATH"
  fi
}

developer_id_identity() {
  security find-identity -p codesigning -v 2>/dev/null | awk -F\" '/Developer ID Application/ {print $2; exit}'
}

list_local_teams() {
  local profiles_dir="$HOME/Library/MobileDevice/Provisioning Profiles"
  if [[ ! -d "$profiles_dir" ]]; then
    echo "No provisioning profiles found at: $profiles_dir"
    return 0
  fi

  find "$profiles_dir" -maxdepth 1 -name '*.mobileprovision' -print0 2>/dev/null |
    while IFS= read -r -d '' profile; do
      local tmp
      tmp="$(mktemp)"
      if security cms -D -i "$profile" >"$tmp" 2>/dev/null; then
        local name team_id team_name app_id
        name="$(plutil -extract Name raw -o - "$tmp" 2>/dev/null || true)"
        team_id="$(plutil -extract TeamIdentifier.0 raw -o - "$tmp" 2>/dev/null || true)"
        team_name="$(plutil -extract TeamName raw -o - "$tmp" 2>/dev/null || true)"
        app_id="$(plutil -extract Entitlements.application-identifier raw -o - "$tmp" 2>/dev/null || true)"
        if [[ -n "$team_id" || -n "$team_name" ]]; then
          printf '%s\t%s\t%s\t%s\n' "$team_id" "$team_name" "$app_id" "$name"
        fi
      fi
      rm -f "$tmp"
    done |
    sort -u |
    awk -F '\t' 'BEGIN {
      printf "%-12s  %-28s  %-36s  %s\n", "TEAM_ID", "TEAM_NAME", "APP_ID", "PROFILE"
    } {
      printf "%-12s  %-28s  %-36s  %s\n", $1, $2, $3, $4
    }'
}

case "$CMD" in
  check)
    echo "Bundle ID: ${PETERFAN_BUNDLE_ID:-kr.co.uulab.peterfan}"
    echo "Daemon:    ${PETERFAN_DAEMON_LABEL:-kr.co.uulab.peterfan.daemon}"
    echo "Login:     ${PETERFAN_LOGIN_ITEM_LABEL:-kr.co.uulab.peterfan.menubar}"
    if identity="$(developer_id_identity)" && [[ -n "$identity" ]]; then
      echo "Developer ID identity: $identity"
    else
      echo "Developer ID identity: not installed"
    fi
    echo "Notary profile: ${NOTARYTOOL_PROFILE:-not set}"
    ;;

  teams)
    list_local_teams
    ;;

  csr)
    require_cmd openssl
    mkdir -p "$PRIVATE_DIR"
    chmod 700 "$PRIVATE_DIR"
    if [[ -e "$KEY_PATH" || -e "$CSR_PATH" ]]; then
      echo "error: CSR/private key already exists in $PRIVATE_DIR" >&2
      echo "       move it aside first if you intentionally want to regenerate." >&2
      exit 1
    fi
    EMAIL="${APPLE_ID:-dev@bonjin.app}"
    COMMON_NAME="${PETERFAN_CERT_COMMON_NAME:-bonjin.app PeterFan Developer ID}"
    openssl req \
      -new \
      -newkey rsa:2048 \
      -nodes \
      -keyout "$KEY_PATH" \
      -out "$CSR_PATH" \
      -subj "/emailAddress=${EMAIL}/CN=${COMMON_NAME}/C=KR"
    chmod 600 "$KEY_PATH"
    echo "created:"
    echo "  $CSR_PATH"
    echo "  $KEY_PATH"
    echo
    echo "Upload the CSR to Apple Developer as a Developer ID Application certificate."
    if command -v open >/dev/null 2>&1; then
      open "https://developer.apple.com/account/resources/certificates/add"
    fi
    ;;

  import)
    CERT_PATH="${2:-}"
    if [[ -z "$CERT_PATH" ]]; then
      CERT_PATH=$(find "$HOME/Downloads" -maxdepth 1 \( -name '*.cer' -o -name '*.CER' \) -print -mtime -7 2>/dev/null | sort -r | head -n 1)
    fi
    if [[ -z "$CERT_PATH" || ! -f "$CERT_PATH" ]]; then
      echo "error: pass the .cer file downloaded from Apple Developer, or put it in ~/Downloads" >&2
      usage
      exit 2
    fi
    if [[ ! -f "$KEY_PATH" ]]; then
      echo "error: private key not found at $KEY_PATH" >&2
      echo "       run scripts/setup-macos-signing.sh csr first on this Mac." >&2
      exit 1
    fi
    KEYCHAIN="${PETERFAN_KEYCHAIN:-$HOME/Library/Keychains/login.keychain-db}"
    security import "$KEY_PATH" -k "$KEYCHAIN" -A
    security import "$CERT_PATH" -k "$KEYCHAIN" -A
    if identity="$(developer_id_identity)" && [[ -n "$identity" ]]; then
      set_env_value PETERFAN_SIGN_IDENTITY "$identity"
      echo "imported Developer ID identity:"
      echo "  $identity"
      echo "updated .env: PETERFAN_SIGN_IDENTITY"
    else
      echo "warning: imported certificate, but no Developer ID Application identity was found yet." >&2
      echo "         Check Keychain Access for a matching private key + certificate pair." >&2
    fi
    ;;

  notary)
    require_cmd xcrun
    PROFILE="${NOTARYTOOL_PROFILE:-peterfan-notary}"
    if [[ -z "${APPLE_ID:-}" ]]; then
      read -r -p "Apple ID email: " APPLE_ID
      if [[ -n "$APPLE_ID" ]]; then
        set_env_value APPLE_ID "$APPLE_ID"
      fi
    fi
    if [[ -z "${APPLE_TEAM_ID:-}" ]]; then
      read -r -p "Apple Developer Team ID: " APPLE_TEAM_ID
      if [[ -n "$APPLE_TEAM_ID" ]]; then
        set_env_value APPLE_TEAM_ID "$APPLE_TEAM_ID"
      fi
    fi
    if [[ -z "${APPLE_ID:-}" || -z "${APPLE_TEAM_ID:-}" ]]; then
      echo "error: Apple ID and Team ID are required for notarization" >&2
      exit 1
    fi
    echo "Storing notary credentials in Keychain profile: $PROFILE"
    xcrun notarytool store-credentials "$PROFILE" \
      --apple-id "$APPLE_ID" \
      --team-id "$APPLE_TEAM_ID"
    set_env_value NOTARYTOOL_PROFILE "$PROFILE"
    ;;

  -h|--help|help)
    usage
    ;;

  *)
    echo "error: unknown command: $CMD" >&2
    usage
    exit 2
    ;;
esac
