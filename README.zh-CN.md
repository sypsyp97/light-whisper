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
SenseVoice（中/英/日/韩/粤，内置标点恢复）、Faster Whisper（99+ 语言）或 GLM-ASR（在线，无需显卡，支持中文方言）。

**离线在线随心切**<br>
SenseVoice 和 Whisper 完全本地运行；GLM-ASR 调用云端 API——只需填入 API Key，无需 Python 和显卡。

**GPU 加速**<br>
本地引擎自动检测 NVIDIA GPU 启用 CUDA 推理，无 GPU 则回退 CPU。

**字幕悬浮窗**<br>
透明浮窗实时显示听写状态，也可承载助手结果。

**语音助手模式**<br>
支持单独热键：说出任务后生成一个悬浮答案卡，手动复制即可。

**屏幕感知助手**<br>
可选功能：自动截取全屏画面作为助手的视觉上下文。自动检测模型是否支持图片输入，不支持时自动回退。

**按住 / 切换**<br>
录音模式：按住说话 或 按一下开始/再按一下结束。

</td>
<td width="50%">

**AI 润色**<br>
可选 LLM 后处理：修正同音字、标点、口头禅；自动检测前台应用适配语气。

**多 LLM 后端**<br>
内置 OpenAI、DeepSeek、Cerebras、SiliconFlow 预设，也可接入任意 OpenAI 兼容端点。

**自适应学习**<br>
只学习结构化纠错和术语；手动删除的脏热词会进入黑名单，不再自动长回来。

**编辑选中文本**<br>
选中文字后按热键，说出指令（"翻译成英文"、"改成正式语气"），原地改写。

**上下文感知助手**<br>
助手模式会读取选中文本和当前前台应用，把它们作为生成上下文。

**实时翻译**<br>
设置目标语言（8 种预设 + 自定义），转写结果自动翻译后输出。

**输入队列**<br>
连续快速说多段，结果按顺序输入，不会丢字。

</td>
</tr>
</table>

## Light-Whisper vs Typeless

| 功能 | Light-Whisper | Typeless |
|:-----|:---:|:---:|
| **价格** | 免费开源 | 30 天免费试用；免费版 (4k 词/周)；$12–30/月 |
| **隐私** | 完全离线，数据不出本机 | 云端处理，零数据留存 |
| **开源** | ✅ | ❌ |
| **平台** | Windows | Windows、Mac、iOS、Android；定价页还列出 Web |
| **需要联网** | ❌ 本地引擎离线；GLM-ASR 和 AI 润色需 API | 云服务；官网未公开说明离线模式 |
| **ASR 引擎** | SenseVoice + Faster Whisper + GLM-ASR（可切换） | 云端专有服务 |
| **语言数** | 5 (SenseVoice) / 99+ (Whisper) / 中文方言 (GLM-ASR) | 100+ |
| **GPU 加速** | 本地 NVIDIA CUDA；GLM-ASR 无需显卡 | 不适用（云端） |
| **AI 润色** | 多 LLM 后端，自带 key | 内置自动编辑 |
| **去口头禅** | ✅ 通过 AI 润色 | ✅ 内置 |
| **应用感知语气** | ✅ 检测前台应用 | ✅ 根据上下文调整 |
| **自适应学习** | ✅ 学习结构化纠错，并支持热词黑名单 | ✅ |
| **编辑选中文本** | ✅ 语音指令改写 | ✅ |
| **语音助手模式** | ✅ 独立热键 + 悬浮答案卡 | ✅ Ask anything / Quick answers |
| **实时翻译** | ✅ 8 种预设 + 自定义 | ✅ |
| **屏幕感知助手** | ✅ 自动截屏作为视觉上下文 | ❌ |
| **字幕悬浮窗** | ✅ | ❌ |
| **输入队列** | ✅ | 未知 |

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
                                       ├─── 屏幕捕获 ──► 全屏截图 → 助手上下文
                                       └─── 用户画像 ──► 热词 + 黑名单 → ASR + LLM prompt
```

<details>
<summary><b>关键路径</b></summary>

| 层 | 路径 |
|:---|:-----|
| **前端** | `src/pages/`, `src/components/`, `src/hooks/`, `src/styles/` |
| **Rust 命令** | `src-tauri/src/commands/` — audio, assistant, clipboard, hotkey, ai_polish, profile, window |
| **Rust 服务** | `src-tauri/src/services/` — funasr_service, glm_asr_service, audio_service, assistant_service, ai_polish_service, llm_client, llm_provider, profile_service, screen_capture_service, download_service |
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

默认听写热键是 F2。助手模式也支持单独热键。如果任一热键被别的软件占用，可在设置中换成别的组合键（如 `Ctrl+Win+R`）。

</details>

<details>
<summary><b>为什么助手模式不自动输入？</b></summary>

助手模式现在使用悬浮答案卡，而不是直接自动输入。这样可以避免把生成内容误写进错误的输入框。需要时点击浮窗右上角 `复制`，再手动粘贴。

</details>

<details>
<summary><b>为什么以前删掉的脏热词又回来了？</b></summary>

旧版本只会把热词从当前列表里删掉，不会阻止它从学习词频里重新提升回来。新版本会同时清掉对应学习词频，并加入热词黑名单，所以手动删除后不应该再自动回流。

</details>

<details>
<summary><b>日志位置</b></summary>

- `%APPDATA%\com.light-whisper.app\logs\funasr_server.log`
- `%APPDATA%\com.light-whisper.app\logs\whisper_server.log`

</details>

## 致谢

- [FunASR](https://github.com/modelscope/FunASR) & [SenseVoiceSmall](https://huggingface.co/FunAudioLLM/SenseVoiceSmall) — 阿里达摩院
- [faster-whisper](https://github.com/SYSTRAN/faster-whisper) & [large-v3-turbo-ct2](https://huggingface.co/deepdml/faster-whisper-large-v3-turbo-ct2)
- [GLM-ASR](https://bigmodel.cn/) — 智谱 AI
- [Tauri](https://tauri.app/) / [React](https://react.dev/)

## 许可证

[知识共享署名-非商业性使用 4.0 国际许可协议 (CC BY-NC 4.0)](LICENSE)
