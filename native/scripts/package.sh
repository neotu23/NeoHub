#!/usr/bin/env bash
# Build NeoHub.app from the Rust workspace.
#
# The bundle is assembled by hand rather than with a bundler so the layout is explicit:
#   NeoHub.app/Contents/MacOS/neohub-desktop      the binary
#   NeoHub.app/Contents/Info.plist                identity, versions, activation policy
#   NeoHub.app/Contents/Resources/icon.icns       app icon (copied from the Electron build)
#   NeoHub.app/Contents/Resources/locales/*.json  UI strings, loaded at runtime
#
# Signing: uses the first available Developer ID identity, else ad-hoc (`-`). An ad-hoc
# build runs locally but is not distributable - Gatekeeper refuses it on another Mac.
# Set NEOHUB_SIGN_IDENTITY to choose explicitly, or SKIP_SIGN=1 to skip signing.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO="$(cd "$ROOT/.." && pwd)"
DIST="$ROOT/dist"
APP="$DIST/NeoHub.app"
VERSION="${NEOHUB_VERSION:-1.4.0}"
export DEVELOPER_DIR="${DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}"

echo "==> building the release binary"
cd "$ROOT"
cargo build --release --locked -p neohub-desktop

echo "==> assembling $APP"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources/locales"
cp target/release/neohub-desktop "$APP/Contents/MacOS/neohub-desktop"
chmod +x "$APP/Contents/MacOS/neohub-desktop"

# Locales are runtime files: the UI falls back to English when a key is missing.
cp "$ROOT"/crates/ui/locales/*.json "$APP/Contents/Resources/locales/"

if [ -f "$REPO/build/icon.icns" ]; then
  cp "$REPO/build/icon.icns" "$APP/Contents/Resources/icon.icns"
else
  echo "    (no build/icon.icns found; the bundle will use the generic icon)"
fi

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>NeoHub</string>
  <key>CFBundleDisplayName</key><string>NeoHub</string>
  <key>CFBundleExecutable</key><string>neohub-desktop</string>
  <key>CFBundleIdentifier</key><string>app.neohub.desktop</string>
  <key>CFBundleIconFile</key><string>icon.icns</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.developer-tools</string>
  <key>NSHighResolutionCapable</key><true/>
  <!-- The app keeps running in the menu bar when its window closes; it must not get the
       automatic "quit when last window closes" treatment. -->
  <key>NSAppSleepDisabled</key><false/>
  <key>NSHumanReadableCopyright</key><string>Copyright © 2026 Jainath Ponnala</string>
</dict>
</plist>
PLIST

plutil -lint "$APP/Contents/Info.plist" >/dev/null

if [ "${SKIP_SIGN:-0}" = "1" ]; then
  echo "==> skipping signing (SKIP_SIGN=1)"
else
  IDENTITY="${NEOHUB_SIGN_IDENTITY:-}"
  if [ -z "$IDENTITY" ]; then
    IDENTITY="$(security find-identity -v -p codesigning 2>/dev/null \
      | sed -n 's/.*"\(Developer ID Application:.*\)"/\1/p' | head -1)"
  fi
  if [ -n "$IDENTITY" ]; then
    echo "==> signing with: $IDENTITY"
    codesign --force --options runtime --timestamp --sign "$IDENTITY" "$APP"
  else
    echo "==> no Developer ID found; signing ad-hoc (local use only)"
    codesign --force --sign - "$APP"
  fi
fi

echo "==> verifying"
codesign --verify --strict "$APP"
echo "    signature: $(codesign -dv "$APP" 2>&1 | sed -n 's/^Authority=//p' | head -1 || echo adhoc)"
echo "    arch:      $(lipo -archs "$APP/Contents/MacOS/neohub-desktop")"
echo "    size:      $(du -sh "$APP" | cut -f1)"

if [ "${SKIP_DMG:-0}" != "1" ]; then
  echo "==> building the dmg"
  rm -f "$DIST/NeoHub-$VERSION.dmg"
  STAGE="$(mktemp -d)"
  cp -R "$APP" "$STAGE/"
  ln -s /Applications "$STAGE/Applications"
  hdiutil create -quiet -volname "NeoHub $VERSION" -srcfolder "$STAGE" \
    -ov -format UDZO "$DIST/NeoHub-$VERSION.dmg"
  rm -rf "$STAGE"
  echo "    dmg: $DIST/NeoHub-$VERSION.dmg ($(du -h "$DIST/NeoHub-$VERSION.dmg" | cut -f1))"
fi

echo "==> done: $APP"
