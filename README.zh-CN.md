<div align="center">

# Light-Whisper 轻语

**本地与云端语音转文字 · Windows 桌面应用**

简体中文 | [English](README.md)

[![Tauri 2](https://img.shields.io/badge/Tauri-2.0-24c8db?style=for-the-badge&logo=tauri)](https://tauri.app/)
[![React 19](https://img.shields.io/badge/React-19-61dafb?style=for-the-badge&logo=react)](https://react.dev/)
[![Rust](https://img.shields.io/badge/Rust-2021-f74c00?style=for-the-badge&logo=rust)](https://www.rust-lang.org/)
[![License: CC BY-NC 4.0](https://img.shields.io/badge/License-CC%20BY--NC%204.0-lightgrey?style=for-the-badge)](LICENSE)

<br>

<img src="assets/readme-hero.png" alt="Light-Whisper 深色听写界面" width="100%" />

<br>

**按下热键，开口说话，松开后文字自动输入到当前应用。**

[下载安装包](https://github.com/sypsyp97/light-whisper/releases/latest)

</div>

## 功能

- **一键听写**：通过可配置全局热键录音，转写后自动输入到当前活动窗口。
- **本地与云端 ASR**：本地运行 SenseVoice / Faster Whisper，也可使用 GLM-ASR / 阿里 DashScope，免本地模型。
- **ASR 原文优先 + AI 润色**：先快速显示 ASR 结果，再在 LLM 返回后替换或预览润色结果；结果卡片显示 ASR、AI 和总耗时。
- **字幕悬浮窗**：透明浮窗显示听写、识别、润色、联网搜索和助手状态。
- **语音助手**：独立热键唤起，可选读取选中文本、前台应用和全屏截图作为上下文。
- **选中文本编辑与翻译**：选中文字后说出指令，可原地改写；也可输出预设或自定义目标语言。
- **模型与搜索配置**：内置 OpenAI、DeepSeek、Cerebras、SiliconFlow；支持自定义 OpenAI 兼容或 Anthropic 端点；助手支持模型内置搜索、Exa、Tavily。
- **个人词库**：热词、结构化纠错学习，以及手动删除词条的黑名单。

## ASR 引擎

| 引擎 | 运行方式 | 适合场景 | 语言 / 模型 | 说明 |
|:--|:--|:--|:--|:--|
| **SenseVoice** | 本地 Python 引擎 | 默认低延迟听写 | 中 / 英 / 日 / 韩 / 粤 | 下载 SenseVoiceSmall + VAD 模型 |
| **Faster Whisper** | 本地 Python 引擎 | 更广语言覆盖 | large-v3-turbo-ct2，99+ 语言 | 下载 Whisper 模型 |
| **GLM-ASR** | 在线 API | 免本地模型的云端 ASR | `glm-asr-2512` | API Key + 区域端点 |
| **阿里 DashScope** | 在线 API | DashScope 上的 Qwen ASR / Omni | 默认 `qwen3-asr-flash`；模型列表可刷新 | API Key + 区域 + 模型 |

在线 ASR 引擎只返回最终结果，并跳过本地 Python 引擎启动。本地引擎使用打包的 Python 引擎和缓存的 HuggingFace 模型。

## 安装

### 安装包

从 [Releases](https://github.com/sypsyp97/light-whisper/releases/latest) 下载 `*_x64-setup.exe`。安装包已包含应用运行时，无需安装 Python 或编译工具。本地 ASR 模型会在首次使用时下载。

GPU 加速是可选项。NVIDIA 显卡配合较新的驱动可启用 CUDA；无 GPU 时应用自动回退 CPU。

### 从源码构建

Windows 10/11 x64 环境要求：

| 工具 | 版本 | 用途 |
|:--|:--|:--|
| [Visual Studio Build Tools](https://visualstudio.microsoft.com/zh-hans/visual-cpp-build-tools/) | 2019+ | MSVC C++ 编译链 |
| [Rust](https://www.rust-lang.org/tools/install) | >= 1.75 | Tauri 后端 |
| [Node.js](https://nodejs.org/) | >= 18 | 前端构建 |
| [pnpm](https://pnpm.io/) | >= 8 | 前端包管理 |
| [uv](https://docs.astral.sh/uv/) | >= 0.4 | 本地 ASR 的 Python 环境 |

```bash
git clone https://github.com/sypsyp97/light-whisper.git
cd light-whisper

pnpm install
uv sync
pnpm tauri dev
```

构建安装包：

```bash
pnpm tauri build
```

NSIS 安装包会输出到 `src-tauri/target/release/bundle/nsis/`。

可选的本地模型预下载：

```bash
uv run python src-tauri/resources/download_models.py --engine sensevoice
uv run python src-tauri/resources/download_models.py --engine whisper
```

国内下载可在预下载前设置 `HF_ENDPOINT=https://hf-mirror.com`。

## 开发命令

```bash
pnpm tauri dev
pnpm tauri build
pnpm build
pnpm test
uv sync
cd src-tauri && cargo check
```

## 排障

**热键没反应**：当前代码里的默认听写热键是 `F2`。如果被其他应用占用，可在设置中修改。

**GPU 未检测到**：运行 `.venv\Scripts\python.exe -c "import torch; print(torch.cuda.is_available())"` 检查。保持 NVIDIA 驱动较新即可；无需单独安装 CUDA Toolkit，PyTorch 自带 CUDA。

**日志位置**：

- 应用日志：`%LOCALAPPDATA%\com.light-whisper.desktop\logs\app.log`
- Python ASR 日志：`%APPDATA%\com.light-whisper.app\logs\funasr_server.log` / `whisper_server.log`
- Python stderr 兜底日志：`%APPDATA%\com.light-whisper.app\funasr_stderr.log`

## 致谢

- [FunASR](https://github.com/modelscope/FunASR) & [SenseVoiceSmall](https://huggingface.co/FunAudioLLM/SenseVoiceSmall)
- [faster-whisper](https://github.com/SYSTRAN/faster-whisper) & [large-v3-turbo-ct2](https://huggingface.co/deepdml/faster-whisper-large-v3-turbo-ct2)
- [GLM-ASR](https://bigmodel.cn/)
- [Alibaba DashScope](https://www.alibabacloud.com/help/zh/model-studio/) & Qwen ASR / Omni
- [Tauri](https://tauri.app/) / [React](https://react.dev/)

## 许可证

[知识共享署名-非商业性使用 4.0 国际许可协议 (CC BY-NC 4.0)](LICENSE)
