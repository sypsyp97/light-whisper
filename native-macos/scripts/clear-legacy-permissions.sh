#!/usr/bin/env bash
set -euo pipefail

LEGACY_IDENTIFIER="com.light-whisper.app"
CURRENT_IDENTIFIER="com.light-whisper.desktop"
HOME_LIBRARY="$HOME/Library"
INCLUDE_CURRENT=0

if [[ "${1:-}" == "--include-current" ]]; then
  INCLUDE_CURRENT=1
fi

paths=(
  "$HOME_LIBRARY/Application Support/$LEGACY_IDENTIFIER"
  "$HOME_LIBRARY/Caches/$LEGACY_IDENTIFIER"
  "$HOME_LIBRARY/Preferences/$LEGACY_IDENTIFIER.plist"
)

if [[ "$INCLUDE_CURRENT" -eq 1 ]]; then
  paths+=(
    "$HOME_LIBRARY/Application Support/com.light-whisper.desktop"
    "$HOME_LIBRARY/Caches/com.light-whisper.desktop"
    "$HOME_LIBRARY/Preferences/com.light-whisper.desktop.plist"
  )
fi

if command -v tccutil >/dev/null 2>&1; then
  tccutil reset All "$LEGACY_IDENTIFIER" || true
  if [[ "$INCLUDE_CURRENT" -eq 1 ]]; then
    tccutil reset All "$CURRENT_IDENTIFIER" || true
  fi
fi

rm -rf "${paths[@]}"

if [[ "$INCLUDE_CURRENT" -eq 1 ]]; then
  echo "Cleared legacy and current local state for $LEGACY_IDENTIFIER and $CURRENT_IDENTIFIER"
else
  echo "Cleared legacy local state for $LEGACY_IDENTIFIER"
fi
