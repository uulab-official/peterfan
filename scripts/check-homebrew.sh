#!/usr/bin/env bash
# Validate the repository-owned Homebrew Formula and Cask without requiring
# Homebrew itself. If brew is installed, also run its native audit command.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CASK="$ROOT/packaging/homebrew/Casks/peterfan.rb"
FORMULA="$ROOT/packaging/homebrew/peterfan.rb"

fail=0
for file in "$CASK" "$FORMULA"; do
  if [[ ! -f "$file" ]]; then
    echo "error: missing Homebrew definition: $file" >&2
    fail=1
  elif ! ruby -c "$file" >/dev/null; then
    echo "error: invalid Ruby syntax: $file" >&2
    fail=1
  fi
done

if [[ "$fail" != "0" ]]; then
  exit 1
fi

VERSION="$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[] | select(.name=="peterfan-menubar") | .version')"
CASK_VERSION="$(ruby -ne 'puts $1 if $_ =~ /^\s*version\s+\"([^\"]+)\"/' "$CASK" | head -1)"
PUBLIC_VERSION="$(sed -n 's/.*Public latest release identified as `v\([^`]*\)`.*/\1/p' "$ROOT/docs/PRODUCTION_CHECKLIST.md" | head -1)"
if [[ -z "$CASK_VERSION" || ! "$CASK_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "error: Homebrew cask does not contain a valid pinned semver" >&2
  exit 1
fi
if [[ -z "$PUBLIC_VERSION" || "$CASK_VERSION" != "$PUBLIC_VERSION" ]]; then
  echo "error: Homebrew cask must stay pinned to documented public release v${PUBLIC_VERSION:-unknown}; found v$CASK_VERSION" >&2
  exit 1
fi
if [[ "$CASK_VERSION" == "$VERSION" ]]; then
  echo "Homebrew cask matches the workspace release version v$VERSION."
else
  echo "Homebrew cask remains pinned to public v$CASK_VERSION while workspace is v$VERSION."
fi
CASK_SHA256="$(ruby -ne 'puts $1 if $_ =~ /^\s*sha256\s+\"([0-9a-f]{64})\"/' "$CASK" | head -1)"
if [[ ! "$CASK_SHA256" =~ ^[0-9a-f]{64}$ ]]; then
  echo "error: Homebrew cask checksum is not a pinned SHA-256" >&2
  exit 1
fi
if grep -Eq 'PLACEHOLDER|:no_check|uulab/peterfan/releases' "$CASK"; then
  echo "error: Homebrew cask still contains an unpinned or stale release reference" >&2
  exit 1
fi
if ! grep -Fq "releases/download/v#{version}/PeterFan-v#{version}.dmg" "$CASK"; then
  echo "error: Homebrew cask URL is not the official versioned DMG" >&2
  exit 1
fi

if command -v brew >/dev/null 2>&1; then
  brew audit --cask --strict "$CASK"
  brew audit --formula --strict "$FORMULA"
else
  echo "Homebrew not installed; Ruby and repository checks passed."
fi
