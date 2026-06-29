#!/usr/bin/env bash
# Assemble PeterFan.app — a macOS menu-bar agent bundle around the
# peterfan-menubar binary.
#
# Usage:
#   scripts/bundle-macos.sh [BINARY] [OUTDIR]
#     BINARY  path to the built peterfan-menubar (default: target/release/peterfan-menubar)
#     OUTDIR  where to write PeterFan.app   (default: dist)
#   VERSION=0.8.0 scripts/bundle-macos.sh   # stamps the bundle version
#
# LSUIElement=true makes it an "accessory" app: a menu-bar item with no Dock
# icon. The binary is unsigned; users clear the quarantine flag on first run.

set -euo pipefail

BIN="${1:-target/release/peterfan-menubar}"
OUTDIR="${2:-dist}"
VERSION="${VERSION:-0.0.0}"
APP="$OUTDIR/PeterFan.app"

if [[ ! -x "$BIN" ]]; then
  echo "error: binary not found at '$BIN' (build it first: cargo build --release)" >&2
  exit 1
fi

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/PeterFan"
chmod +x "$APP/Contents/MacOS/PeterFan"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>PeterFan</string>
  <key>CFBundleDisplayName</key><string>PeterFan</string>
  <key>CFBundleIdentifier</key><string>com.uulab.peterfan</string>
  <key>CFBundleExecutable</key><string>PeterFan</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>${VERSION}</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>LSUIElement</key><true/>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSHumanReadableCopyright</key><string>MIT © PeterFan contributors</string>
</dict>
</plist>
PLIST

# Validate the plist if the tool is available.
if command -v plutil >/dev/null 2>&1; then
  plutil -lint "$APP/Contents/Info.plist" >/dev/null
fi

echo "built $APP (version $VERSION)"
