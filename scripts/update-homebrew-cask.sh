#!/usr/bin/env bash
# Pin the Homebrew cask to the exact DMG that will be uploaded to GitHub.
#
# Usage:
#   scripts/update-homebrew-cask.sh v1.30.0 dist/local-release/v1.30.0/PeterFan-v1.30.0.dmg
#
# Run this only after the final signed/notarized DMG has been produced. The
# checksum of a local ad-hoc build must never be used for a public release.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CASK="$ROOT/packaging/homebrew/Casks/peterfan.rb"
TAG="${1:-}"
DMG="${2:-}"

if [[ ! "$TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "usage: scripts/update-homebrew-cask.sh vX.Y.Z /path/to/PeterFan-vX.Y.Z.dmg" >&2
  exit 2
fi
if [[ -z "$DMG" ]]; then
  DMG="$ROOT/dist/local-release/$TAG/PeterFan-$TAG.dmg"
fi
if [[ ! -f "$DMG" ]]; then
  echo "error: DMG not found: $DMG" >&2
  exit 1
fi

VERSION="${TAG#v}"
MANIFEST_VERSION="$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[] | select(.name=="peterfan-menubar") | .version')"
if [[ "$VERSION" != "$MANIFEST_VERSION" ]]; then
  echo "error: $TAG does not match Cargo package version $MANIFEST_VERSION" >&2
  exit 1
fi

SHA256="$(shasum -a 256 "$DMG" | awk '{print $1}')"

ruby - "$CASK" "$VERSION" "$SHA256" <<'RUBY'
path, version, sha256 = ARGV
body = File.read(path)
body = body.sub(/version "[^"]+"/, %(version "#{version}"))
body = body.sub(/sha256 "[0-9a-f]{64}"/, %(sha256 "#{sha256}"))
File.write(path, body)
RUBY

echo "updated $CASK"
echo "  version: $VERSION"
echo "  sha256:  $SHA256"
echo "  url:     https://github.com/uulab-official/peterfan/releases/download/$TAG/PeterFan-$TAG.dmg"
