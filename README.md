<div align="center">

# Light-Whisper

**Local & Online Speech-to-Text for Windows**

[简体中文](README.zh-CN.md) | English

[![Tauri 2](https://img.shields.io/badge/Tauri-2.0-24c8db?style=for-the-badge&logo=tauri)](https://tauri.app/)
[![React 19](https://img.shields.io/badge/React-19-61dafb?style=for-the-badge&logo=react)](https://react.dev/)
[![Rust](https://img.shields.io/badge/Rust-2021-f74c00?style=for-the-badge&logo=rust)](https://www.rust-lang.org/)
[![License: CC BY-NC 4.0](https://img.shields.io/badge/License-CC%20BY--NC%204.0-lightgrey?style=for-the-badge)](LICENSE)

<br>

<img src="assets/icon.png" alt="Light-Whisper" width="128" />

<br>

**Press a hotkey, speak, release — text appears at your cursor.**

[Download Installer](https://github.com/sypsyp97/light-whisper/releases/latest)

</div>

<br>

## Installation

### Option A: Installer (recommended)

Download the `*_x64-setup.exe` installer from the [Releases](https://github.com/sypsyp97/light-whisper/releases/latest) page. Run the installer — everything is bundled, no Python or build tools needed. ASR models will be downloaded on first use (~1–1.5 GB).

> [!NOTE]
> **GPU (optional):** NVIDIA GPU with up-to-date driver for CUDA acceleration. No GPU → automatic CPU fallback.

### Option B: Build from source

See [Quick Start](#quick-start) below.

## Highlights

<table>
<tr>
<td width="50%">

**One-key dictation**<br>
Hold <kbd>F2</kbd> (configurable) to record, release to transcribe & type into the active window.

**Four ASR engines**<br>
SenseVoice (zh/en/ja/ko/yue) and Faster Whisper (99+ languages) run fully local with CUDA acceleration; GLM-ASR and Alibaba DashScope/Qwen ASR are online — just add an API key, no Python or GPU needed.

**AI polish**<br>
Optional LLM post-processing: fix homophones, punctuation, filler words; adapts tone to the foreground app. Raw ASR can appear first, then update to the polished result when the LLM returns. The result card shows ASR, AI, and total latency. Built-in presets (OpenAI / DeepSeek / Cerebras / SiliconFlow) plus OpenAI-compatible and Anthropic API formats.

**Adaptive learning**<br>
Learns structured ASR corrections and key terms; manually deleted hot words are blocked from coming back.

**Subtitle overlay**<br>
Transparent floating window shows live dictation status, raw ASR preview, polish completion state, and assistant output.

</td>
<td width="50%">

**Voice assistant**<br>
Separate hotkey triggers a floating answer card. Optionally reads selected text, foreground app, and full-screen screenshots as context. Auto-detects model image support. Built-in web search (Exa / Tavily) for real-time information retrieval.

**Edit selected text**<br>
Select text anywhere, press the hotkey, speak an instruction ("translate to English", "make it formal") to rewrite in-place.

**Real-time translation**<br>
Set a target language (8 presets + custom) — transcription results are translated before output.

**& more**<br>
Hold-to-talk or toggle mode · input queue · multilingual UI (en/zh) · auto-update check.

</td>
</tr>
</table>

## Engine Comparison

| Engine | Runtime | Best for | Language / model | Setup |
|:--|:--|:--|:--|:--|
| **SenseVoice** | Local Python engine | Default dictation, low-latency local use | zh / en / ja / ko / yue | Downloads SenseVoiceSmall + VAD models |
| **Faster Whisper** | Local Python engine | Wider language coverage | large-v3-turbo-ct2, 99+ languages | Downloads Whisper model |
| **GLM-ASR** | Online API | Cloud ASR with no local model download | `glm-asr-2512` | API key + region endpoint |
| **Alibaba DashScope** | Online API | Qwen ASR / Omni models on DashScope | Default `qwen3-asr-flash`; model list can be refreshed in Settings | API key + region + model |

> [!NOTE]
> Online ASR engines return final results only and skip the local Python engine startup. Local engines use the bundled Python engine and cached HuggingFace models.

## Build from Source — Requirements

> [!IMPORTANT]
> **Windows 10/11 (x64) only.** Disk: ~10 GB free. These requirements are only needed for building from source — the [installer](#installation) bundles everything.

| Tool | Version | Purpose |
|:-----|:--------|:--------|
| [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) | 2019+ | MSVC C++ toolchain |
| [Rust](https://www.rust-lang.org/tools/install) | >= 1.75 | Backend |
| [Node.js](https://nodejs.org/) | >= 18 | Frontend build |
| [pnpm](https://pnpm.io/) | >= 8 | Frontend packages |
| [uv](https://docs.astral.sh/uv/) | >= 0.4 | Python env (auto-installs Python 3.11) |

**GPU (optional):** NVIDIA GPU with up-to-date driver. No need to install CUDA Toolkit — PyTorch bundles CUDA 12.8.

> [!TIP]
> **Online ASR users:** If you only use GLM-ASR or Alibaba DashScope/Qwen ASR, you only need Rust, Node.js, and pnpm — no Python, uv, or GPU required. Build the app and add your API key in Settings.

<details>
<summary><b>Step-by-step tool installation</b></summary>

```powershell
# 1. Visual Studio Build Tools — download installer, check "Desktop development with C++"
# 2. Rust
winget install Rustlang.Rustup
# 3. Node.js + pnpm
winget install OpenJS.NodeJS.LTS
npm install -g pnpm
# 4. uv
winget install astral-sh.uv
```

Verify:

```powershell
rustc --version     # >= 1.75
node --version      # >= 18
pnpm --version      # >= 8
uv --version        # >= 0.4
```

> [!TIP]
> Python is managed by `uv` — no manual install required.

</details>

## Quick Start

```bash
git clone https://github.com/sypsyp97/light-whisper.git
cd light-whisper

pnpm install          # Frontend deps
uv sync               # Python deps (downloads Python 3.11, PyTorch CUDA, etc. ~5-15 min)
```

### Download ASR models (recommended before first run)

```bash
# SenseVoice (default, ~938 MB)
uv run python -c "from huggingface_hub import snapshot_download; snapshot_download('FunAudioLLM/SenseVoiceSmall'); snapshot_download('funasr/fsmn-vad')"

# Faster Whisper (~1.5 GB)
uv run python -c "from huggingface_hub import snapshot_download; snapshot_download('deepdml/faster-whisper-large-v3-turbo-ct2')"
```

Models are cached in `~/.cache/huggingface/hub/`.

> [!TIP]
> **China mainland:** set `$env:HF_ENDPOINT = "https://hf-mirror.com"` before downloading.

### Build & Run

```bash
pnpm tauri build      # First build ~5-15 min (compiles Rust deps)
```

The installer is in `src-tauri/target/release/bundle/nsis/`, or run `src-tauri/target/release/light-whisper.exe` directly.

## Architecture

```
┌──────────────┐                ┌──────────────┐                ┌─────────────────┐
│   React UI   │  Tauri IPC     │  Rust Core   │  stdin/stdout  │   Python ASR    │
│  TypeScript  │◄──invoke/emit─►│  (Tauri 2)   │◄────JSON──────►│  SenseVoice /   │
└──────────────┘                └──────┬───────┘                │  Faster Whisper │
                                       │                        └─────────────────┘
                                       ├─── HTTP ──► Online ASR APIs (GLM-ASR / Alibaba DashScope)
                                       ├─── HTTP ──► LLM API (AI polish / assistant / translation)
                                       ├─── HTTP ──► Web Search (Exa / Tavily) → assistant context
                                       ├─── Screen Capture ──► full-screen screenshots → assistant context
                                       └─── User Profile ──► hot words + blacklist → ASR + LLM prompt
```

<details>
<summary><b>Key paths</b></summary>

| Layer | Paths |
|:------|:------|
| **Frontend** | `src/pages/`, `src/components/`, `src/hooks/`, `src/contexts/`, `src/lib/`, `src/i18n/`, `src/styles/` |
| **Rust commands** | `src-tauri/src/commands/` — audio, assistant, clipboard, funasr, hotkey, ai_polish, profile, updater, window |
| **Rust services** | `src-tauri/src/services/` — funasr_service, glm_asr_service, alibaba_asr_service, audio_service, assistant_service, ai_polish_service, llm_client, llm_provider, profile_service, screen_capture_service, web_search_service, download_service |
| **State** | `src-tauri/src/state/` — app_state, user_profile |
| **Python ASR** | `src-tauri/resources/` — funasr_server.py, whisper_server.py, server_common.py |

</details>

## Dev Commands

```bash
pnpm tauri dev          # Dev mode with hot-reload
pnpm tauri build        # Production build + installer
pnpm build              # Frontend only
uv sync                 # Sync Python deps
cd src-tauri && cargo check   # Rust type check
```

## FAQ

<details>
<summary><b>PyTorch or model downloads are slow</b></summary>

- PyTorch CUDA (~2.5 GB) downloads from `download.pytorch.org` — use a stable connection or VPN. `uv sync` supports resume.
- Other Python packages can use a Tsinghua mirror: `$env:UV_INDEX_URL = "https://mirrors.tuna.tsinghua.edu.cn/pypi/web/simple"`
- HuggingFace models: `$env:HF_ENDPOINT = "https://hf-mirror.com"`

</details>

<details>
<summary><b>GPU not detected</b></summary>

Verify: `.venv\Scripts\python.exe -c "import torch; print(torch.cuda.is_available())"` should print `True`.
Requires an up-to-date NVIDIA driver. For CUDA 12.x minor-version compatibility, Windows driver should be >= 528.33. The app falls back to CPU automatically if no GPU is found.

</details>

<details>
<summary><b>Hotkey not working</b></summary>

Default dictation hotkey is F2. If occupied by another program, change it in Settings (e.g. `Ctrl+Win+R`).

</details>

<details>
<summary><b>Log locations</b></summary>

- App log: `%LOCALAPPDATA%\com.light-whisper.desktop\logs\app.log`
- Python ASR logs: `%APPDATA%\com.light-whisper.app\logs\funasr_server.log` / `whisper_server.log`
- Python stderr fallback: `%APPDATA%\com.light-whisper.app\funasr_stderr.log`

</details>

## Acknowledgements

- [FunASR](https://github.com/modelscope/FunASR) & [SenseVoiceSmall](https://huggingface.co/FunAudioLLM/SenseVoiceSmall) — Alibaba DAMO Academy
- [faster-whisper](https://github.com/SYSTRAN/faster-whisper) & [large-v3-turbo-ct2](https://huggingface.co/deepdml/faster-whisper-large-v3-turbo-ct2)
- [GLM-ASR](https://bigmodel.cn/) — Zhipu AI
- [Alibaba DashScope](https://www.alibabacloud.com/help/en/model-studio/) & Qwen ASR / Omni
- [Tauri](https://tauri.app/) / [React](https://react.dev/)

## License

[Creative Commons Attribution-NonCommercial 4.0 International (CC BY-NC 4.0)](LICENSE)
