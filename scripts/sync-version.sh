#!/usr/bin/env bash
set -euo pipefail

VERSION="${1:?Usage: bash scripts/sync-version.sh <version>}"

python3 - "$VERSION" <<'PY'
import json
import plistlib
import re
import sys
from pathlib import Path

version = sys.argv[1]
root = Path.cwd()

def update_json(path: Path) -> None:
    data = json.loads(path.read_text(encoding="utf-8"))
    data["version"] = version
    path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

def update_cargo(path: Path) -> None:
    text = path.read_text(encoding="utf-8")
    text = re.sub(
        r'^(version\s*=\s*").*?(")',
        rf'\g<1>{version}\2',
        text,
        count=1,
        flags=re.MULTILINE,
    )
    path.write_text(text, encoding="utf-8")

def update_plist(path: Path) -> None:
    with path.open("rb") as handle:
        data = plistlib.load(handle)
    data["CFBundleShortVersionString"] = version
    data["CFBundleVersion"] = version
    with path.open("wb") as handle:
        plistlib.dump(data, handle, sort_keys=False)

update_json(root / "package.json")
update_json(root / "src-tauri" / "tauri.conf.json")
update_cargo(root / "src-tauri" / "Cargo.toml")
update_plist(root / "native-macos" / "Bundle" / "Info.plist")
PY

echo "Synced version to ${VERSION}"
