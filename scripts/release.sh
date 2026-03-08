#!/usr/bin/env bash
# 本地构建 + 发布 Release
# 用法: bash scripts/release.sh 1.1.0

set -euo pipefail

VERSION="${1:?用法: bash scripts/release.sh <version> (例如 1.1.0)}"
TAG="v${VERSION}"
CONF="src-tauri/tauri.conf.json"
INSTALLER="src-tauri/target/release/bundle/nsis/轻语 Whisper_${VERSION}_x64-setup.exe"

echo "=== 发布 ${TAG} ==="

# 1. 更新版本号
echo "[1/6] 更新版本号 → ${VERSION}"
sed -i "s/\"version\": \"[^\"]*\"/\"version\": \"${VERSION}\"/" "$CONF"

# 2. 构建 Python 引擎
echo "[2/6] 构建 Python 引擎"
uv run python scripts/build_engine.py

# 3. 构建 Tauri 安装包
echo "[3/6] 构建 Tauri 安装包"
pnpm tauri build

# 4. 验证安装包
if [ ! -f "$INSTALLER" ]; then
    echo "错误: 安装包不存在: ${INSTALLER}"
    exit 1
fi
SIZE=$(du -h "$INSTALLER" | cut -f1)
echo "安装包: ${SIZE}"

# 5. 提交 + tag + push
echo "[4/6] 提交版本变更"
git add "$CONF"
git commit -m "chore: bump version to ${VERSION}"
git tag "$TAG"

echo "[5/6] 推送到远程"
git push && git push --tags

# 6. 上传 Release
echo "[6/6] 创建 Release 并上传安装包"
gh release create "$TAG" "$INSTALLER" \
    --title "$TAG" \
    --generate-notes

echo "=== 发布完成: https://github.com/sypsyp97/light-whisper/releases/tag/${TAG} ==="
