<div align="center">

# Light-Whisper 轻语

**本地与云端语音转文字 · Windows 桌面应用**

简体中文 | [English](README.md)

[![Tauri 2](https://img.shields.io/badge/Tauri-2.0-24c8db?style=for-the-badge&logo=tauri)](https://tauri.app/)
[![React 19](https://img.shields.io/badge/React-19-61dafb?style=for-the-badge&logo=react)](https://react.dev/)
[![Rust](https://img.shields.io/badge/Rust-2021-f74c00?style=for-the-badge&logo=rust)](https://www.rust-lang.org/)
[![License: GPL-3.0-only](https://img.shields.io/badge/License-GPL--3.0--only-2f6f9f?style=for-the-badge)](LICENSE)

<br>

<img src="assets/readme-hero.png" alt="Light-Whisper 深色听写界面" width="100%" />

<br>

**按下热键，开口说话，松开后文字自动输入到当前应用。**

[下载安装包](https://github.com/sypsyp97/light-whisper/releases/latest)

</div>

## 功能

- **听写与翻译**：通过可配置热键输入到当前应用，支持按住说话、点击切换录音和指定翻译目标语言。
- **本地或云端识别**：在本机运行 Qwen3-ASR，或使用无需本地模型的 GLM-ASR / 阿里 DashScope。
- **AI 润色与 Jev**：提供四档结构化程度；可选 Jev 判断是否需要润色，无需修改时保留原文。结果卡片显示各阶段耗时。
- **字幕悬浮窗**：显示本地实时识别文本和处理状态。
- **语音助手与编辑**：提问或改写选中文字，可选加入应用、选区或截图上下文；鼠标划词还支持翻译、解释、润色、复制和搜索。
- **模型与联网搜索**：支持内置服务商、自定义 OpenAI 兼容 / Anthropic 端点、受支持的 OpenAI Codex 和 Grok Build 账号登录，以及模型内置、Exa、Tavily 搜索。
- **个性化**：热词、纠错学习、已删除词条屏蔽，以及按应用覆盖润色、翻译、截图和自定义指令。
- **本地历史记录**：可选开启，支持保留期限、搜索、导出、删除和重新润色；额外保存音频后可重新识别。

本文介绍当前源码中的功能；已发布安装包可能有所不同，请查看[版本说明](https://github.com/sypsyp97/light-whisper/releases)。

## 快速开始

1. 安装后打开设置，选择麦克风和识别引擎。使用本地 Qwen 时下载模型；使用云端引擎时填写 API Key 和区域。
2. 将焦点放到需要输入文字的应用，按住 `F2` 说话，松开后输出文字。可在设置中更换热键或改用点击切换录音。
3. 使用 AI 润色或语音助手时，选择服务商和模型，并填写 API Key 或使用受支持的账号登录。基础本地听写不需要 LLM 账号。
4. 使用 Jev 时，打开 **AI 润色 → Jev 智能跳过润色**，选择 TypeSafe（官方）、OpenRouter 或 Vercel，填写该服务的独立 API Key，同时保持 AI 润色开启。Jev 默认关闭，出错或超时继续正常润色；翻译、助手/编辑和手动重新润色不经过该判断。

## 数据使用

本地 ASR 在电脑上处理音频，云端 ASR 会将音频发送给所选服务商。启用 AI 润色或 Jev 后，文本与处理要求会发送给对应服务；可选的选区、截图上下文和联网搜索也可能向配置的服务发送数据。Jev 密钥按服务商分别保存在系统凭据库中。历史记录和音频保存均为可选的本地设置。

## ASR 引擎

| 引擎 | 运行方式 | 适合场景 | 语言 / 模型 | 说明 |
|:--|:--|:--|:--|:--|
| **Qwen3-ASR 0.6B Q8** | 本地 GGUF 引擎 | 速度优先的 Qwen 听写 | 多语言，Q8_0 | 独立下载约 850 MB；内置 FireRedVAD；CUDA / Vulkan / CPU |
| **Qwen3-ASR 1.7B Q8** | 本地 GGUF 引擎 | 更偏质量的 Qwen 选项 | 多语言，Q8_0 | 独立下载约 2.19 GB；内置 FireRedVAD；CUDA / Vulkan / CPU |
| **GLM-ASR** | 在线 API | 免本地模型的云端 ASR | `glm-asr-2512` | API Key + 区域端点 |
| **阿里 DashScope** | 在线 API | DashScope 上的 Qwen ASR / Omni | 默认 `qwen3-asr-flash`；模型列表可刷新 | API Key + 区域 + 模型 |

云端引擎只返回最终结果，不启动本地 Python。Qwen 模型下载后会缓存，FireRedVAD 已随应用内置；两种 Qwen 模型均支持热词，优先使用 CUDA，并可回退到 Vulkan/CPU。

## 安装

### 安装包

从 [Releases](https://github.com/sypsyp97/light-whisper/releases/latest) 下载 `*_x64-setup.exe`。安装包已包含应用运行时，无需安装 Python 或编译工具。本地 ASR 模型会在首次使用时下载。

GPU 加速是可选项。NVIDIA 显卡配合较新的驱动可启用 CUDA；无 GPU 时应用自动回退 CPU。

### 从源码构建

Windows 10/11 x64 构建工具（标注 CI 的版本与仓库配置一致）：

| 工具 | 版本 | 用途 |
|:--|:--|:--|
| [Visual Studio Build Tools](https://visualstudio.microsoft.com/zh-hans/visual-cpp-build-tools/) | 2019+ | MSVC C++ 编译链 |
| [Rust](https://www.rust-lang.org/tools/install) | 1.93.0 (CI) | Tauri 后端 |
| [Node.js](https://nodejs.org/) | 22.14.0 (CI) | 前端构建 |
| [pnpm](https://pnpm.io/) | 10.28.2 (CI) | 前端包管理 |
| [uv](https://docs.astral.sh/uv/) | 0.11.30 (CI) | 本地 ASR 的 Python 环境 |

`.python-version` 指定 Python 3.11，项目支持 Python 3.11–3.12，由 `uv` 管理环境。

```bash
git clone https://github.com/sypsyp97/light-whisper.git
cd light-whisper

pnpm install --frozen-lockfile
uv sync --frozen
pnpm tauri dev
```

构建可分发安装包。Python 引擎归档不会提交到 Git，因此需要先构建：

```bash
uv run --locked python scripts/build_engine.py
pnpm tauri build
```

NSIS 安装包会输出到 `src-tauri/target/release/bundle/nsis/`。
`pnpm tauri build` 会拒绝缺失、空文件或并非 XZ 格式的引擎归档，不再静默生成缺少本地 ASR 的安装包。如果补丁版本没有改动打包进引擎的 Python 运行时代码，可以复用已经验证过的 `engine.tar.xz`。

可选的本地模型预下载：

```bash
uv run python src-tauri/resources/download_models.py --engine qwen3-asr-0.6b
uv run python src-tauri/resources/download_models.py --engine qwen3-asr-1.7b
```

国内下载可在预下载前设置 `HF_ENDPOINT=https://hf-mirror.com`。

## 开发命令

```bash
pnpm tauri dev
pnpm check
pnpm build
pnpm test
uv sync
cd src-tauri && cargo check
```

## 排障

**热键没反应**：当前代码里的默认听写热键是 `F2`。如果被其他应用占用，可在设置中修改。

**GPU 未检测到**：运行 `nvidia-smi`。Qwen3-ASR 的实际设备/后端记录在 `qwen3_asr_server.log`。

**日志位置**：

- 应用日志：`%LOCALAPPDATA%\com.light-whisper.desktop\logs\app.log`
- Qwen3-ASR 日志：`%TEMP%\light_whisper_logs\qwen3_asr_server.log`
- Python stderr 兜底日志：`%APPDATA%\com.light-whisper.app\funasr_stderr.log`

## 致谢

- [Qwen3-ASR](https://github.com/QwenLM/Qwen3-ASR) & [transcribe.cpp](https://github.com/handy-computer/transcribe.cpp)
- [FireRedVAD](https://huggingface.co/FireRedTeam/FireRedVAD)
- [GLM-ASR](https://bigmodel.cn/)
- [Alibaba DashScope](https://www.alibabacloud.com/help/zh/model-studio/) & Qwen ASR / Omni
- [Tauri](https://tauri.app/) / [React](https://react.dev/)

## 许可证

Light-Whisper 是采用 [GNU General Public License v3.0 only](LICENSE) 的开源软件。
个人、组织和企业均可使用，包括商业使用。无论是否修改，以源代码或编译形式分发时都必须
遵守 GPL；分发二进制版本时，应按其条款要求提供对应源代码。
已经发布的旧版本继续适用其发布时附带的许可证。

第三方软件、模型和字体继续适用各自的许可证，详见 [NOTICE](NOTICE) 和
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
