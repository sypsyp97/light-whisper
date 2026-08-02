# 发版指南

发布只在本机打包。普通 GitHub Actions CI 负责前端、Python 和 Rust 检查，不构建或上传安装包。

## 前置条件

- 当前分支为 `main`，且与 `origin/main` 完全一致。
- 工作区干净，没有未提交或未跟踪的文件。
- `gh auth status`、`uv`、`pnpm`、Rust、Python、Node.js 和 7-Zip 可用。
- 新版本号和 tag 尚未存在。

## 一键发布

需要重新构建 Python 引擎时：

```bash
bash scripts/release.sh 1.5.5 "本次发布说明。" --rebuild-engine
```

只有在打包进引擎的 Python ASR 运行时代码没有变化时，才能复用已经验证过的归档：

```bash
bash scripts/release.sh 1.5.5 --generate-notes --reuse-engine
```

脚本按以下顺序执行，任一步失败都会在创建 tag 和 Release 前停止：

1. 校验版本号、分支、工作区、远端同步状态和目标 tag。
2. 同步 `package.json`、`pyproject.toml`、Tauri/Cargo 元数据及两个 lockfile 的版本。
3. 运行与 CI 一致的前端、Python、Rust、依赖审计和 diff 检查。
4. 构建或复用 `src-tauri/resources/engine.tar.xz`，检查文件格式并用 7-Zip 完整测试归档。
5. 在本机运行 `pnpm tauri build`，验证 NSIS 安装包并输出 SHA-256。
6. 创建版本候选提交并只推送该提交，此时还不会创建 tag。
7. 按候选提交的精确 SHA 等待 `ci.yml`，并确认 `Frontend`、`Python`、`Rust` 全部成功。
8. CI 通过后再次确认 `origin/main` 仍指向候选提交，再创建并推送 annotated tag，最后通过 `gh release create --verify-tag` 上传本机安装包。

CI、引擎构建或安装包验证失败时，脚本不会创建 tag 或 GitHub Release。若候选提交已经推送，需要先修复问题并重新建立新的候选提交；不要给失败的提交补 tag。

## 手动验证命令

```bash
pnpm install --frozen-lockfile
pnpm check
pnpm audit --prod --audit-level high

uv lock --check
uv sync --frozen
uv run --no-sync python -m compileall -q scripts src-tauri/resources
uv run --no-sync python -m unittest discover -s src-tauri/resources -p "test_*.py"
uv run --no-sync python scripts/test_build_engine_atomicity.py

cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --locked -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --all-targets --locked -- \
  --skip services::qwen_hotword_service::tests::hotword_correction_p95_stays_below_one_millisecond

git diff --check
```

## 引擎归档规则

- `pnpm tauri dev` 和普通 debug 测试允许使用开发占位文件。
- 正式 release 构建只接受非空的 `src-tauri/resources/engine.tar.xz`。
- `pnpm tauri build` 还会验证 XZ 文件头；发布脚本会额外执行 7-Zip 完整测试。
- Python ASR 运行时代码发生变化时必须使用 `--rebuild-engine`。
- 复用归档前，应明确确认相关 Python 运行时代码没有变化。

## 发布失败后的处理

- tag 尚未创建：修复问题，重新运行完整流程。
- tag 已推送但 Release 创建失败：先查明失败原因，再手动运行 `gh release create <tag> <installer> --verify-tag`；不要覆盖或移动已经公开的 tag。
- 已发布资产需要替换：只有确认 tag 与候选提交正确后，才能使用 `gh release upload <tag> <installer> --clobber`。
