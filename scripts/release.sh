#!/usr/bin/env bash
# 本地构建 + 发布 Release
# 用法: bash scripts/release.sh 1.1.0

set -euo pipefail

VERSION="${1:?用法: bash scripts/release.sh <version> (例如 1.1.0)}"
TAG="v${VERSION}"
TAURI_CONF="src-tauri/tauri.conf.json"
CARGO_TOML="src-tauri/Cargo.toml"
INSTALLER="src-tauri/target/release/bundle/nsis/轻语 Whisper_${VERSION}_x64-setup.exe"

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

# 1. 更新版本号（tauri.conf.json + Cargo.toml）
echo "[1/6] 更新版本号 → ${VERSION}"
python -c "
import json, re, sys

version = sys.argv[1]

# tauri.conf.json
with open('$TAURI_CONF', 'r', encoding='utf-8') as f:
    conf = json.load(f)
conf['version'] = version
with open('$TAURI_CONF', 'w', encoding='utf-8') as f:
    json.dump(conf, f, indent=2, ensure_ascii=False)
    f.write('\n')

# Cargo.toml — 只替换 [package] 下的第一个 version
with open('$CARGO_TOML', 'r', encoding='utf-8') as f:
    text = f.read()
text = re.sub(r'^(version = \").*?\"', rf'\g<1>{version}\"', text, count=1, flags=re.MULTILINE)
with open('$CARGO_TOML', 'w', encoding='utf-8') as f:
    f.write(text)
" "$VERSION"

# 2. 构建 Python 引擎
echo "[2/6] 构建 Python 引擎"
uv run python scripts/build_engine.py

# 3. 构建 Tauri 安装包
echo "[3/6] 构建 Tauri 安装包"
pnpm tauri build

# 4. 验证安装包
echo "[4/6] 验证安装包"
if [ ! -f "$INSTALLER" ]; then
    echo "错误: 安装包不存在: ${INSTALLER}"
    exit 1
fi
SIZE=$(du -h "$INSTALLER" | cut -f1)
echo "安装包: ${SIZE}"

# 5. 提交 + tag + push
echo "[5/6] 提交 + 推送"
git add "$TAURI_CONF" "$CARGO_TOML" src-tauri/Cargo.lock
git commit -m "chore: bump version to ${VERSION}"
git tag "$TAG"
git push && git push --tags

# 6. 上传 Release
echo "[6/6] 创建 Release 并上传安装包"
gh release create "$TAG" "$INSTALLER" \
    --title "$TAG" \
    --generate-notes

echo "=== 发布完成: https://github.com/sypsyp97/light-whisper/releases/tag/${TAG} ==="
