#!/usr/bin/env bash
# Build assets/AppIcon.icns from assets/icon-1024.png using macOS sips+iconutil.
# Regenerate the source PNG first with:
#   cargo run --manifest-path tools/icongen/Cargo.toml
set -euo pipefail

SRC="${1:-assets/icon-1024.png}"
OUT="${2:-assets/AppIcon.icns}"

if [[ ! -f "$SRC" ]]; then
  echo "error: source PNG not found at '$SRC'" >&2
  exit 1
fi

SET="$(mktemp -d)/AppIcon.iconset"
mkdir -p "$SET"
gen() { sips -z "$1" "$1" "$SRC" --out "$SET/$2" >/dev/null; }
gen 16   icon_16x16.png
gen 32   icon_16x16@2x.png
gen 32   icon_32x32.png
gen 64   icon_32x32@2x.png
gen 128  icon_128x128.png
gen 256  icon_128x128@2x.png
gen 256  icon_256x256.png
gen 512  icon_256x256@2x.png
gen 512  icon_512x512.png
cp "$SRC" "$SET/icon_512x512@2x.png"

iconutil -c icns "$SET" -o "$OUT"
echo "built $OUT"
