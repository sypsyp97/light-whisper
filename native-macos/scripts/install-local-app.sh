#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PACKAGE_DIR="$ROOT/native-macos"
BUILD_ROOT="$ROOT/build/native-macos"
APP_DIR="$BUILD_ROOT/Light Whisper.app"
TARGET_DIR="${TARGET_DIR:-$HOME/Applications}"
TARGET_APP="$TARGET_DIR/Light Whisper.app"
EXECUTABLE_NAME="LightWhisperNativeApp"
ICON_SOURCE="$PACKAGE_DIR/Bundle/AppIcon.icns"
PACKAGE_JSON="$ROOT/package.json"
CODESIGN_IDENTITY="${CODESIGN_IDENTITY:-}"

mkdir -p "$BUILD_ROOT" "$TARGET_DIR"

if [[ ! -f "$PACKAGE_JSON" ]]; then
  echo "Missing package metadata: $PACKAGE_JSON" >&2
  exit 1
fi

if [[ ! -f "$ICON_SOURCE" ]]; then
  echo "Missing icon: $ICON_SOURCE" >&2
  exit 1
fi

if [[ -z "$CODESIGN_IDENTITY" ]]; then
  CODESIGN_IDENTITY="$(
    security find-identity -v -p codesigning 2>/dev/null \
      | awk -F'"' '/Apple Development:/{print $2; exit}'
  )"
fi

if [[ -z "$CODESIGN_IDENTITY" ]]; then
  CODESIGN_IDENTITY="-"
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
mkdir -p "$APP_DIR/Contents/MacOS" "$APP_DIR/Contents/Resources"

cp "$EXECUTABLE_PATH" "$APP_DIR/Contents/MacOS/$EXECUTABLE_NAME"
cp "$PACKAGE_DIR/Bundle/Info.plist" "$APP_DIR/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $VERSION" "$APP_DIR/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $VERSION" "$APP_DIR/Contents/Info.plist"
cp "$ICON_SOURCE" "$APP_DIR/Contents/Resources/AppIcon.icns"

if [[ "$CODESIGN_IDENTITY" == "-" ]]; then
  codesign --force --deep --sign - "$APP_DIR" >/dev/null
else
  codesign \
    --force \
    --deep \
    --options runtime \
    --timestamp \
    --sign "$CODESIGN_IDENTITY" \
    "$APP_DIR" >/dev/null
fi

codesign --verify --deep --strict "$APP_DIR" >/dev/null

killall "$EXECUTABLE_NAME" 2>/dev/null || true
rm -rf "$TARGET_APP"
ditto "$APP_DIR" "$TARGET_APP"
xattr -dr com.apple.quarantine "$TARGET_APP" 2>/dev/null || true

open "$TARGET_APP"
echo "$TARGET_APP"
