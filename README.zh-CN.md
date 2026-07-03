# 轻语 Whisper

轻语 Whisper 是一个面向 macOS 的桌面听写应用，基于 Tauri、React 和 Rust 构建。

当前分支：`codex/apple-silicon-mlx-asr`

> [!IMPORTANT]
> 这个分支聚焦 macOS，目前只保留在线 ASR：
> `Alibaba DashScope` 和 `GLM-ASR`。
> 历史本地引擎配置（`local`、`sensevoice`、`whisper`）会迁移到 `alibaba-asr`。

## 功能

**一键听写**
默认按住 <kbd>F2</kbd> 录音，松开后自动转写并输入到当前活动应用。

**在线 ASR 引擎**
可在设置中选择阿里 DashScope/Qwen ASR 或 GLM-ASR，并填写对应 API Key。这个分支不再打包本地 Python 运行时，也不提供本地模型下载。

**AI 润色与助手**
可选 LLM 后处理：修正标点、口头禅和常见识别错误。ASR 原文可以先显示，再在 AI 润色完成后更新；结果卡片会展示 ASR、AI 和总耗时。助手支持屏幕上下文和联网搜索。

**macOS 权限流程**
应用会引导麦克风、辅助功能、屏幕录制和自动化权限；当 macOS 拒绝权限时，可直接打开对应的隐私设置页面。

## 快速开始

环境要求：

| 工具 | 版本 | 用途 |
|:--|:--|:--|
| macOS | 建议 14+ | 目标平台 |
| Xcode Command Line Tools | 最新版 | 原生编译工具链 |
| Rust | 1.75+ | Tauri 后端 |
| Node.js | 18+ | 前端构建 |
| pnpm | 8+ | 前端包管理 |

安装并启动：

```bash
pnpm install
pnpm tauri dev
```

构建 macOS app / DMG：

```bash
pnpm tauri build
```

启动后进入设置，配置其中一种在线 ASR：

- 阿里 DashScope：选择区域和模型，并填写对应 DashScope API Key。
- GLM-ASR：填写 GLM API Key。

## 引擎对比

| 引擎 | 运行方式 | 配置 | 说明 |
|:--|:--|:--|:--|
| GLM-ASR | 在线 API | API Key | 仅返回最终结果 |
| 阿里 DashScope | 在线 API | 区域、模型、API Key | 默认 `qwen3-asr-flash`；可刷新模型列表 |

本地 ASR 是这个分支后续要补上的差异化能力，但当前尚未实现。

## 开发

前端检查：

```bash
pnpm install --frozen-lockfile
pnpm build
pnpm test
```

Rust 检查：

```bash
cd src-tauri
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```

实验性原生 Swift 包：

```bash
swift build --package-path native-macos
swift test --package-path native-macos
```

Swift 应用在 CI 中能构建并通过 parity 测试前，仍视为实验项目。

## 架构

```text
React UI  <--Tauri IPC/events-->  Rust Core
                                      |
                                      +--> GLM-ASR API
                                      +--> Alibaba DashScope ASR
                                      +--> LLM API（润色、助手、翻译）
                                      +--> Web Search API
                                      +--> macOS 权限、剪贴板、热键、音频
```

主要路径：

| 区域 | 路径 |
|:--|:--|
| 前端 | `src/pages/`, `src/components/`, `src/hooks/`, `src/contexts/`, `src/lib/`, `src/i18n/`, `src/styles/` |
| Rust 命令 | `src-tauri/src/commands/` |
| Rust 服务 | `src-tauri/src/services/` |
| 原生 Swift 实验 | `native-macos/` |
| 设计与 parity 文档 | `docs/` |

## 排障

**麦克风、辅助功能或自动化权限被拒绝**
打开 macOS 系统设置 -> 隐私与安全性，授予对应权限。权限刚修改后，请完全退出并重新打开应用。

**在线 ASR 认证或区域报错**
确认选择的引擎、区域和 API Key 来自同一个服务商账号和端点。

**日志位置**
Tauri 应用日志位于 `com.light-whisper.desktop` 对应的 macOS 应用数据 / 日志目录。应用数据目录：

```text
~/Library/Application Support/com.light-whisper.desktop/
```

这个分支不再启动本地 Python ASR 服务，因此 `funasr_server.log` 和 `whisper_server.log` 不再适用。

## 致谢

- [Alibaba DashScope](https://www.alibabacloud.com/help/zh/model-studio/) 和 Qwen ASR / Omni
- [GLM-ASR](https://bigmodel.cn/)
- [Tauri](https://tauri.app/) 和 [React](https://react.dev/)

## 许可证

本项目使用 CC BY-NC 4.0 许可证。详见 [LICENSE](LICENSE)。
