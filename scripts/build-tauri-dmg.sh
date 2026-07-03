#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TAURI_CONF="$ROOT/src-tauri/tauri.conf.json"
RUN_TAURI_BUILD="${RUN_TAURI_BUILD:-1}"
CODESIGN_IDENTITY="${CODESIGN_IDENTITY:--}"
NOTARY_PROFILE="${NOTARY_PROFILE:-}"

if [[ ! -f "$TAURI_CONF" ]]; then
  echo "Missing Tauri config: $TAURI_CONF" >&2
  exit 1
fi

CONFIG_VALUES="$(python3 - "$TAURI_CONF" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as f:
    config = json.load(f)

product = str(config.get("productName", "")).strip()
version = str(config.get("version", "")).strip()
if not product:
    raise SystemExit("tauri.conf.json is missing productName")
if not version:
    raise SystemExit("tauri.conf.json is missing version")

print(f"{product}\t{version}")
PY
)"
IFS=$'\t' read -r PRODUCT_NAME VERSION <<< "$CONFIG_VALUES"

case "$(uname -m)" in
  arm64) ARCH="aarch64" ;;
  x86_64) ARCH="x64" ;;
  *) ARCH="$(uname -m)" ;;
esac

APP_DIR="$ROOT/src-tauri/target/release/bundle/macos/${PRODUCT_NAME}.app"
DMG_DIR="$ROOT/src-tauri/target/release/bundle/dmg"
DMG_PATH="$DMG_DIR/${PRODUCT_NAME}_${VERSION}_${ARCH}.dmg"
STAGING_DIR="$(mktemp -d "${TMPDIR:-/tmp}/light-whisper-dmg.XXXXXX")"
MOUNT_DIR=""

cleanup() {
  if [[ -n "$MOUNT_DIR" && -d "$MOUNT_DIR" ]]; then
    hdiutil detach "$MOUNT_DIR" >/dev/null 2>&1 || true
    rmdir "$MOUNT_DIR" >/dev/null 2>&1 || true
  fi
  rm -rf "$STAGING_DIR"
}
trap cleanup EXIT

sign_app() {
  local app_path="$1"
  xattr -cr "$app_path" 2>/dev/null || true
  if [[ "$CODESIGN_IDENTITY" == "-" ]]; then
    codesign --force --deep --sign - "$app_path" >/dev/null
  else
    codesign \
      --force \
      --deep \
      --options runtime \
      --timestamp \
      --sign "$CODESIGN_IDENTITY" \
      "$app_path" >/dev/null
  fi
  codesign --verify --deep --strict "$app_path" >/dev/null
}

if [[ "$RUN_TAURI_BUILD" != "0" ]]; then
  (cd "$ROOT" && pnpm tauri build --bundles app)
fi

if [[ ! -d "$APP_DIR" ]]; then
  echo "Missing app bundle: $APP_DIR" >&2
  exit 1
fi

mkdir -p "$DMG_DIR"

ditto --noextattr --noqtn "$APP_DIR" "$STAGING_DIR/${PRODUCT_NAME}.app"
ln -s /Applications "$STAGING_DIR/Applications"
sign_app "$STAGING_DIR/${PRODUCT_NAME}.app"

rm -f "$DMG_PATH"
hdiutil create \
  -volname "$PRODUCT_NAME" \
  -srcfolder "$STAGING_DIR" \
  -ov \
  -format UDZO \
  -fs HFS+ \
  "$DMG_PATH" >/dev/null

hdiutil verify "$DMG_PATH" >/dev/null

MOUNT_DIR="$(mktemp -d "${TMPDIR:-/tmp}/light-whisper-dmg-mount.XXXXXX")"
hdiutil attach -nobrowse -readonly -mountpoint "$MOUNT_DIR" "$DMG_PATH" >/dev/null
test -d "$MOUNT_DIR/${PRODUCT_NAME}.app"
test -L "$MOUNT_DIR/Applications"
codesign --verify --deep --strict "$MOUNT_DIR/${PRODUCT_NAME}.app" >/dev/null
hdiutil detach "$MOUNT_DIR" >/dev/null
rmdir "$MOUNT_DIR" >/dev/null 2>&1 || true
MOUNT_DIR=""

if [[ "$CODESIGN_IDENTITY" != "-" ]]; then
  codesign --force --timestamp --sign "$CODESIGN_IDENTITY" "$DMG_PATH" >/dev/null
  codesign --verify "$DMG_PATH" >/dev/null
fi

if [[ -n "$NOTARY_PROFILE" ]]; then
  xcrun notarytool submit "$DMG_PATH" --keychain-profile "$NOTARY_PROFILE" --wait >/dev/null
  xcrun stapler staple "$STAGING_DIR/${PRODUCT_NAME}.app" >/dev/null
  xcrun stapler staple "$DMG_PATH" >/dev/null
fi

echo "$APP_DIR"
echo "$DMG_PATH"
