<div align="center">

# Light-Whisper

**Local Offline Speech-to-Text for Windows**

[简体中文](README.zh-CN.md) | English

[![Tauri 2](https://img.shields.io/badge/Tauri-2.0-24c8db?style=flat-square&logo=tauri)](https://tauri.app/)
[![React 19](https://img.shields.io/badge/React-19-61dafb?style=flat-square&logo=react)](https://react.dev/)
[![Rust](https://img.shields.io/badge/Rust-2021-f74c00?style=flat-square&logo=rust)](https://www.rust-lang.org/)
[![License: CC BY-NC 4.0](https://img.shields.io/badge/License-CC%20BY--NC%204.0-lightgrey?style=flat-square)](LICENSE)

<img src="assets/icon.png" alt="Light-Whisper" width="120" />

*Press a hotkey, speak, release — text appears at your cursor.*

</div>

---

## Highlights

| | |
|---|---|
| **One-key dictation** | Hold F2 (configurable) to record, release to transcribe & type into the active window |
| **Dual ASR engine** | SenseVoice (zh/en/ja/ko/yue, built-in punctuation) or Faster Whisper (99+ languages) |
| **Fully offline** | All ASR models run locally — no data leaves your machine |
| **GPU accelerated** | Auto-detects NVIDIA GPU for CUDA inference; falls back to CPU |
| **AI polish** | Optional LLM post-processing: fix homophones, punctuation, filler words; adapts tone to the foreground app |
| **Multi-backend LLM** | Built-in presets for OpenAI, DeepSeek, Cerebras, SiliconFlow — or any OpenAI-compatible endpoint |
| **Adaptive learning** | Auto-extracts correction patterns from AI polish; user edits to transcription results are learned with higher priority |
| **Edit selected text** | Select text anywhere, press the hotkey, speak an instruction ("translate to English", "make it formal") to rewrite in-place |
| **Real-time translation** | Set a target language (8 presets + custom) — transcription results are translated before output |
| **Subtitle overlay** | Transparent floating subtitle window shows real-time transcription status |
| **Hold or toggle** | Recording mode: hold-to-talk or press-to-start / press-to-stop |
| **Input queue** | Rapid consecutive dictations are queued and typed in order — nothing is lost |

---

## Engine Comparison

| | SenseVoice (default) | Faster Whisper |
|---|:---:|:---:|
| **Chinese CER** | 2.96 % (AISHELL-1) | 5.14 % |
| **English WER** | 3.15 % (LibriSpeech) | 1.82 % |
| **Languages** | 5 (zh/en/ja/ko/yue) | 99+ |
| **Punctuation** | Built-in ITN | initial_prompt guided |
| **Model size** | ~938 MB | ~1.5 GB |

> Source: [FunAudioLLM paper](https://arxiv.org/html/2407.04051v1), Table 6

---

## Requirements

> **Windows 10/11 (x64) only.** Disk: ~10 GB free.

| Tool | Version | Purpose |
|------|---------|---------|
| [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) | 2019+ | MSVC C++ toolchain |
| [Rust](https://www.rust-lang.org/tools/install) | >= 1.75 | Backend |
| [Node.js](https://nodejs.org/) | >= 18 | Frontend build |
| [pnpm](https://pnpm.io/) | >= 8 | Frontend packages |
| [uv](https://docs.astral.sh/uv/) | >= 0.4 | Python env (auto-installs Python 3.11) |

**GPU (optional):** NVIDIA GPU with up-to-date driver. No need to install CUDA Toolkit — PyTorch bundles CUDA 12.4.

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

> Python is managed by `uv` — no manual install required.

</details>

---

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

> **China mainland:** set `$env:HF_ENDPOINT = "https://hf-mirror.com"` before downloading.

### Build & Run

```bash
pnpm tauri build      # First build ~5-15 min (compiles Rust deps)
```

The installer is in `src-tauri/target/release/bundle/nsis/`, or run `src-tauri/target/release/light-whisper.exe` directly.

---

## Architecture

```
┌──────────────┐     Tauri IPC      ┌──────────────┐   stdin/stdout   ┌───────────────────┐
│  React UI    │ <── invoke/emit ──>│  Rust Core   │ <──── JSON ────> │  Python ASR       │
│  (TypeScript)│                    │  (Tauri 2)   │                  │  SenseVoice /     │
└──────────────┘                    └──────┬───────┘                  │  Faster Whisper   │
                                          │                          └───────────────────┘
                                          ├── HTTP ──> LLM API (AI polish & translation)
                                          └── User Profile ──> hot words -> ASR + LLM prompt
```

| Layer | Key paths |
|-------|-----------|
| **Frontend** | `src/pages/`, `src/components/`, `src/hooks/`, `src/styles/` |
| **Rust commands** | `src-tauri/src/commands/` — audio, clipboard, hotkey, ai_polish, profile, window |
| **Rust services** | `src-tauri/src/services/` — funasr_service, audio_service, ai_polish_service, llm_provider, profile_service |
| **State** | `src-tauri/src/state/` — app_state, user_profile |
| **Python ASR** | `src-tauri/resources/` — funasr_server.py, whisper_server.py, server_common.py |

---

## Dev Commands

```bash
pnpm tauri dev          # Dev mode with hot-reload
pnpm tauri build        # Production build + installer
pnpm build              # Frontend only
uv sync                 # Sync Python deps
cd src-tauri && cargo check   # Rust type check
```

---

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
Requires NVIDIA driver >= 525.60. The app falls back to CPU automatically if no GPU is found.

</details>

<details>
<summary><b>Characters turn into periods when typing into apps</b></summary>

This happens when a Chinese IME intercepts `SendInput` Unicode events. Fix: switch to **Clipboard paste** mode in Settings, or toggle your IME to English mode.

</details>

<details>
<summary><b>Hotkey not working</b></summary>

Default is F2. If occupied by another program, change it in Settings > Speech Hotkey. Supports any key combo (e.g., `Ctrl+Win+R`).

</details>

<details>
<summary><b>Log locations</b></summary>

- `%APPDATA%\com.light-whisper.app\logs\funasr_server.log`
- `%APPDATA%\com.light-whisper.app\logs\whisper_server.log`

</details>

---

## Acknowledgements

- [FunASR](https://github.com/modelscope/FunASR) & [SenseVoiceSmall](https://huggingface.co/FunAudioLLM/SenseVoiceSmall) — Alibaba DAMO Academy
- [faster-whisper](https://github.com/SYSTRAN/faster-whisper) & [large-v3-turbo-ct2](https://huggingface.co/deepdml/faster-whisper-large-v3-turbo-ct2)
- [Tauri](https://tauri.app/) / [React](https://react.dev/)

## License

[Creative Commons Attribution-NonCommercial 4.0 International (CC BY-NC 4.0)](LICENSE)
