<div align="center">

# Light-Whisper 轻语

**本地 & 在线语音转文字 · Windows 桌面应用**

简体中文 | [English](README.md)

[![Tauri 2](https://img.shields.io/badge/Tauri-2.0-24c8db?style=for-the-badge&logo=tauri)](https://tauri.app/)
[![React 19](https://img.shields.io/badge/React-19-61dafb?style=for-the-badge&logo=react)](https://react.dev/)
[![Rust](https://img.shields.io/badge/Rust-2021-f74c00?style=for-the-badge&logo=rust)](https://www.rust-lang.org/)
[![License: CC BY-NC 4.0](https://img.shields.io/badge/License-CC%20BY--NC%204.0-lightgrey?style=for-the-badge)](LICENSE)

<br>

<img src="assets/icon.png" alt="Light-Whisper" width="128" />

<br>

**按下热键，开口说话，松开即得文字。**

[下载安装包](https://github.com/sypsyp97/light-whisper/releases/latest)

</div>

<br>

## 安装

### 方式一：安装包（推荐）

从 [Releases](https://github.com/sypsyp97/light-whisper/releases/latest) 页面下载 `轻语.Whisper_x.x.x_x64-setup.exe`，运行安装即可。所有依赖已内置，无需安装 Python 或编译工具。ASR 模型会在首次使用时自动下载（约 1–1.5 GB）。

> [!NOTE]
> **GPU（可选）：** NVIDIA 显卡 + 最新驱动即可启用 CUDA 加速。无显卡自动回退 CPU。

### 方式二：从源码构建

参见下方[快速开始](#快速开始)。

## 功能亮点

<table>
<tr>
<td width="50%">

**一键听写**<br>
按住 <kbd>F2</kbd>（可自定义）录音，松开自动转写并输入到当前活动窗口。

**三大 ASR 引擎**<br>
SenseVoice（中/英/日/韩/粤）和 Faster Whisper（99+ 语言）完全本地运行，支持 CUDA 加速；GLM-ASR 在线调用——只需填入 API Key，无需 Python 和显卡。

**AI 润色**<br>
可选 LLM 后处理：修正同音字、标点、口头禅；自动检测前台应用适配语气。内置 OpenAI / DeepSeek / Cerebras / SiliconFlow 预设，支持 OpenAI 兼容和 Anthropic API 格式。

**自适应学习**<br>
只学习结构化纠错和术语；手动删除的脏热词会进入黑名单，不再自动长回来。

**字幕悬浮窗**<br>
透明浮窗实时显示听写状态和助手结果。

</td>
<td width="50%">

**语音助手**<br>
独立热键触发悬浮答案卡。可自动读取选中文本、前台应用和全屏截图作为上下文，自动检测模型图片支持。内置 Web 搜索（Exa / Tavily）获取实时信息。

**编辑选中文本**<br>
选中文字后按热键，说出指令（"翻译成英文"、"改成正式语气"），原地改写。

**实时翻译**<br>
设置目标语言（8 种预设 + 自定义），转写结果自动翻译后输出。

**更多**<br>
按住说话 / 切换模式 · 输入队列 · 中英文界面 · 自动更新检查

</td>
</tr>
</table>

## Light-Whisper vs Typeless

| 功能 | Light-Whisper | Typeless |
|:-----|:---:|:---:|
| **价格** | 免费开源 | 免费版 (4k 词/周)；$12–30/月 |
| **隐私** | 完全离线（本地引擎） | 云端处理，零数据留存 |
| **开源** | ✅ | ❌ |
| **平台** | Windows | Windows、Mac、iOS、Android、Web |
| **ASR 引擎** | 3 种可切换（本地 + 在线） | 云端专有 |
| **语言数** | 5–99+（取决于引擎） | 100+ |
| **AI 润色** | 多 LLM 后端，自带 key | 内置 |
| **屏幕感知助手 + Web 搜索** | ✅ | ❌ |
| **字幕悬浮窗** | ✅ | ❌ |

## 引擎对比

| | SenseVoice（默认） | Faster Whisper | GLM-ASR（在线） |
|:--|:---:|:---:|:---:|
| **中文 CER** | 2.96 %（AISHELL-1） | 5.14 % | 7.17 % |
| **英文 WER** | 3.15 %（LibriSpeech） | 1.82 % | — |
| **语言数** | 5（中/英/日/韩/粤） | 99+ | 中文 + 方言 |
| **标点** | 内置 ITN | initial_prompt 引导 | 内置 |
| **热词** | ✅ | ✅ | ✅（最多 100 个） |
| **模型大小** | ~938 MB | ~1.5 GB | 云端（无需下载） |
| **依赖** | GPU/CPU（已内置） | GPU/CPU（已内置） | 仅需 API Key |
| **费用** | 免费（本地） | 免费（本地） | ¥0.06/分钟 |

> [!NOTE]
> SenseVoice/Whisper CER 来源：[FunAudioLLM 论文](https://arxiv.org/html/2407.04051v1) Table 6。GLM-ASR CER 来源：智谱 AI。

## 从源码构建 — 环境要求

> [!IMPORTANT]
> **Windows 10/11（x64）**，磁盘空间 ≥ 10 GB。以下要求仅用于源码构建——[安装包](#安装)已内置所有依赖。

| 工具 | 版本 | 用途 |
|:-----|:-----|:-----|
| [Visual Studio Build Tools](https://visualstudio.microsoft.com/zh-hans/visual-cpp-build-tools/) | 2019+ | MSVC C++ 编译链 |
| [Rust](https://www.rust-lang.org/tools/install) | >= 1.75 | 后端编译 |
| [Node.js](https://nodejs.org/) | >= 18 | 前端构建 |
| [pnpm](https://pnpm.io/) | >= 8 | 前端包管理 |
| [uv](https://docs.astral.sh/uv/) | >= 0.4 | Python 环境（自动安装 Python 3.11） |

**GPU（可选）：** NVIDIA 显卡 + 最新驱动即可，无需单独安装 CUDA Toolkit — PyTorch 自带 CUDA 12.8。

> [!TIP]
> **GLM-ASR 用户：** 如果只使用在线 GLM-ASR 引擎，只需安装 Rust、Node.js、pnpm 即可——无需 Python、uv 和显卡。构建后在设置中填入 API Key 即可使用。

<details>
<summary><b>逐步安装指引</b></summary>

```powershell
# 1. Visual Studio Build Tools — 下载安装器，勾选「使用 C++ 的桌面开发」
# 2. Rust
winget install Rustlang.Rustup
# 3. Node.js + pnpm
winget install OpenJS.NodeJS.LTS
npm install -g pnpm
# 4. uv
winget install astral-sh.uv
```

验证：

```powershell
rustc --version     # >= 1.75
node --version      # >= 18
pnpm --version      # >= 8
uv --version        # >= 0.4
```

> [!TIP]
> Python 由 `uv` 自动管理，无需手动安装。

</details>

## 快速开始

```bash
git clone https://github.com/sypsyp97/light-whisper.git
cd light-whisper

pnpm install          # 前端依赖
uv sync               # Python 依赖（自动下载 Python 3.11、PyTorch CUDA 等，约 5-15 分钟）
```

### 下载 ASR 模型（建议首次运行前手动下载）

```bash
# SenseVoice（默认，约 938 MB）
uv run python -c "from huggingface_hub import snapshot_download; snapshot_download('FunAudioLLM/SenseVoiceSmall'); snapshot_download('funasr/fsmn-vad')"

# Faster Whisper（约 1.5 GB）
uv run python -c "from huggingface_hub import snapshot_download; snapshot_download('deepdml/faster-whisper-large-v3-turbo-ct2')"
```

模型缓存在 `~/.cache/huggingface/hub/`，只需下载一次。

> [!TIP]
> **国内用户：** 下载前设置 `$env:HF_ENDPOINT = "https://hf-mirror.com"`。

### 构建运行

```bash
pnpm tauri build      # 首次编译 Rust 依赖约 5-15 分钟
```

安装包位于 `src-tauri/target/release/bundle/nsis/`，也可直接运行 `src-tauri/target/release/light-whisper.exe`。

## 架构

```
┌──────────────┐                ┌──────────────┐                ┌─────────────────┐
│   React 前端  │  Tauri IPC     │   Rust 核心   │  stdin/stdout  │   Python ASR    │
│  TypeScript  │◄──invoke/emit─►│  (Tauri 2)   │◄────JSON──────►│  SenseVoice /   │
└──────────────┘                └──────┬───────┘                │  Faster Whisper │
                                       │                        └─────────────────┘
                                       ├─── HTTP ──► GLM-ASR API（在线语音识别）
                                       ├─── HTTP ──► LLM API（AI 润色 / 助手 / 翻译）
                                       ├─── HTTP ──► Web 搜索（Exa / Tavily）→ 助手上下文
                                       ├─── 屏幕捕获 ──► 全屏截图 → 助手上下文
                                       └─── 用户画像 ──► 热词 + 黑名单 → ASR + LLM prompt
```

<details>
<summary><b>关键路径</b></summary>

| 层 | 路径 |
|:---|:-----|
| **前端** | `src/pages/`, `src/components/`, `src/hooks/`, `src/contexts/`, `src/lib/`, `src/i18n/`, `src/styles/` |
| **Rust 命令** | `src-tauri/src/commands/` — audio, assistant, clipboard, funasr, hotkey, ai_polish, profile, updater, window |
| **Rust 服务** | `src-tauri/src/services/` — funasr_service, glm_asr_service, audio_service, assistant_service, ai_polish_service, llm_client, llm_provider, profile_service, screen_capture_service, web_search_service, download_service |
| **状态** | `src-tauri/src/state/` — app_state, user_profile |
| **Python ASR** | `src-tauri/resources/` — funasr_server.py, whisper_server.py, server_common.py |

</details>

## 开发命令

```bash
pnpm tauri dev          # 开发模式（热更新）
pnpm tauri build        # 生产构建 + 安装包
pnpm build              # 仅构建前端
uv sync                 # 同步 Python 依赖
cd src-tauri && cargo check   # Rust 类型检查
```

## 常见问题

<details>
<summary><b>PyTorch 或模型下载慢</b></summary>

- PyTorch CUDA（约 2.5 GB）从 `download.pytorch.org` 下载，建议使用稳定网络。`uv sync` 支持断点续传。
- 其他 Python 包可用清华镜像：`$env:UV_INDEX_URL = "https://mirrors.tuna.tsinghua.edu.cn/pypi/web/simple"`
- HuggingFace 模型：`$env:HF_ENDPOINT = "https://hf-mirror.com"`

</details>

<details>
<summary><b>GPU 未检测到</b></summary>

验证：`.venv\Scripts\python.exe -c "import torch; print(torch.cuda.is_available())"` 应输出 `True`。
需要较新的 NVIDIA 驱动。按 CUDA 12.x 的 minor-version compatibility 口径，Windows 驱动至少应为 528.33。无 GPU 时应用自动回退 CPU。

</details>

<details>
<summary><b>输入到应用时部分字符变成句号</b></summary>

中文输入法拦截了 `SendInput` Unicode 事件。解决：在设置中切换为**剪贴板粘贴**模式，或将输入法切到英文模式。

</details>

<details>
<summary><b>中文显示乱码怎么办？</b></summary>

开启 Windows 的 UTF-8 支持来解决编码问题：控制面板 → 区域（或者叫“时钟和区域”）→ 管理 → 更改系统区域设置 → 勾选底部的 `Beta 版: 使用 Unicode UTF-8 提供全球语言支持` → 确定 → 重启电脑。

</details>

<details>
<summary><b>热键没反应</b></summary>

默认听写热键是 F2。如果被其他软件占用，可在设置中更换（如 `Ctrl+Win+R`）。

</details>

<details>
<summary><b>日志位置</b></summary>

`%APPDATA%\com.light-whisper.app\logs\` — `funasr_server.log` / `whisper_server.log`

</details>

## 致谢

- [FunASR](https://github.com/modelscope/FunASR) & [SenseVoiceSmall](https://huggingface.co/FunAudioLLM/SenseVoiceSmall) — 阿里达摩院
- [faster-whisper](https://github.com/SYSTRAN/faster-whisper) & [large-v3-turbo-ct2](https://huggingface.co/deepdml/faster-whisper-large-v3-turbo-ct2)
- [GLM-ASR](https://bigmodel.cn/) — 智谱 AI
- [Tauri](https://tauri.app/) / [React](https://react.dev/)

## 许可证

[知识共享署名-非商业性使用 4.0 国际许可协议 (CC BY-NC 4.0)](LICENSE)
