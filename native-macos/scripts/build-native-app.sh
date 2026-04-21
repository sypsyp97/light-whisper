#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PACKAGE_DIR="$ROOT/native-macos"
BUILD_ROOT="$ROOT/build/native-macos"
APP_DIR="$BUILD_ROOT/Light Whisper.app"
DMG_PATH="$BUILD_ROOT/Light Whisper.dmg"
DMG_STAGING_DIR="$BUILD_ROOT/dmg-root"
EXECUTABLE_NAME="LightWhisperNativeApp"
ICON_SOURCE="$PACKAGE_DIR/Bundle/AppIcon.icns"
PACKAGE_JSON="$ROOT/package.json"
CODESIGN_IDENTITY="${CODESIGN_IDENTITY:-}"
NOTARY_PROFILE="${NOTARY_PROFILE:-}"

mkdir -p "$BUILD_ROOT"

if [[ ! -f "$PACKAGE_JSON" ]]; then
  echo "Missing package metadata: $PACKAGE_JSON" >&2
  exit 1
fi

if [[ ! -f "$ICON_SOURCE" ]]; then
  echo "Missing icon: $ICON_SOURCE" >&2
  exit 1
fi

if [[ -z "$CODESIGN_IDENTITY" ]]; then
  echo "CODESIGN_IDENTITY is required for release packaging." >&2
  exit 1
fi

if [[ "$CODESIGN_IDENTITY" != Developer\ ID\ Application:* ]]; then
  echo "CODESIGN_IDENTITY must be a Developer ID Application identity for an installable DMG." >&2
  exit 1
fi

VERSION="$(python3 - "$PACKAGE_JSON" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as f:
    package = json.load(f)

version = str(package.get("version", "")).strip()
if not version:
    raise SystemExit("package.json is missing a non-empty version")

print(version)
PY
)"

swift build \
  --package-path "$PACKAGE_DIR" \
  --configuration release \
  --product "$EXECUTABLE_NAME"

BIN_DIR="$(swift build --package-path "$PACKAGE_DIR" --configuration release --show-bin-path)"
EXECUTABLE_PATH="$BIN_DIR/$EXECUTABLE_NAME"

rm -rf "$APP_DIR"
rm -rf "$DMG_STAGING_DIR"
mkdir -p "$APP_DIR/Contents/MacOS" "$APP_DIR/Contents/Resources"

cp "$EXECUTABLE_PATH" "$APP_DIR/Contents/MacOS/$EXECUTABLE_NAME"
cp "$PACKAGE_DIR/Bundle/Info.plist" "$APP_DIR/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $VERSION" "$APP_DIR/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $VERSION" "$APP_DIR/Contents/Info.plist"

cp "$ICON_SOURCE" "$APP_DIR/Contents/Resources/AppIcon.icns"

codesign \
  --force \
  --deep \
  --options runtime \
  --timestamp \
  --sign "$CODESIGN_IDENTITY" \
  "$APP_DIR" >/dev/null
codesign --verify --deep --strict "$APP_DIR" >/dev/null

rm -f "$DMG_PATH"
mkdir -p "$DMG_STAGING_DIR"
cp -R "$APP_DIR" "$DMG_STAGING_DIR/Light Whisper.app"
ln -s /Applications "$DMG_STAGING_DIR/Applications"
hdiutil create \
  -volname "Light Whisper" \
  -srcfolder "$DMG_STAGING_DIR" \
  -ov \
  -format UDZO \
  "$DMG_PATH" >/dev/null

codesign --force --timestamp --sign "$CODESIGN_IDENTITY" "$DMG_PATH" >/dev/null

if [[ -n "$NOTARY_PROFILE" ]]; then
  xcrun notarytool submit "$DMG_PATH" --keychain-profile "$NOTARY_PROFILE" --wait >/dev/null
  xcrun stapler staple "$APP_DIR" >/dev/null
  xcrun stapler staple "$DMG_PATH" >/dev/null
fi

echo "$APP_DIR"
