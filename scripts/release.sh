#!/usr/bin/env bash
# 本地构建 + 发布 macOS Release
# 用法: bash scripts/release.sh 1.3.10 "本次发布说明。"

set -euo pipefail

VERSION="${1:?用法: bash scripts/release.sh <version> (例如 1.1.0)}"
RELEASE_NOTES="${2:---generate-notes}"
TAG="v${VERSION}"
PKG_JSON="package.json"
TAURI_CONF="src-tauri/tauri.conf.json"
CARGO_TOML="src-tauri/Cargo.toml"
NATIVE_INFO="native-macos/Bundle/Info.plist"

echo "=== 发布 ${TAG} ==="

# 前置检查
if [ ! -f "$TAURI_CONF" ]; then
    echo "错误: 请在项目根目录下运行此脚本"
    exit 1
fi

if ! git diff --quiet || ! git diff --cached --quiet; then
    echo "错误: 工作区有未提交的改动，请先 commit 或 stash"
    exit 1
fi

# 1. 更新版本号
echo "[1/6] 更新版本号 → ${VERSION}"
scripts/sync-version.sh "$VERSION"

# 2. 构建 Tauri macOS app / DMG
echo "[2/6] 构建 Tauri macOS app / DMG"
pnpm tauri build

# 3. 验证 DMG
echo "[3/6] 验证 DMG"
DMG="$(find src-tauri/target/release/bundle/dmg -maxdepth 1 -name '*.dmg' -print | sort | tail -n 1)"
if [ -z "$DMG" ] || [ ! -f "$DMG" ]; then
    echo "错误: DMG 不存在"
    exit 1
fi
SIZE=$(du -h "$DMG" | cut -f1)
echo "DMG: ${DMG} (${SIZE})"

# 4. 提交 + tag + push
echo "[4/6] 提交 + 推送"
git add "$PKG_JSON" "$TAURI_CONF" "$CARGO_TOML" "$NATIVE_INFO" src-tauri/Cargo.lock
git commit -m "chore: bump version to ${VERSION}"
git tag "$TAG"
git push && git push --tags

# 5. 上传 Release
echo "[5/6] 创建 Release 并上传 DMG"
if [ "$RELEASE_NOTES" = "--generate-notes" ]; then
    gh release create "$TAG" "$DMG" \
        --title "$TAG" \
        --generate-notes
else
    gh release create "$TAG" "$DMG" \
        --title "$TAG" \
        --notes "$RELEASE_NOTES"
fi

# 6. Done
echo "=== 发布完成: https://github.com/sypsyp97/light-whisper/releases/tag/${TAG} ==="
