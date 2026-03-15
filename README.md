<div align="center">

# Light-Whisper

**Local & Online Speech-to-Text for macOS Apple Silicon**

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

> [!IMPORTANT]
> **Branch status (`codex/apple-silicon-mlx-asr`)**
> This branch migrates the local ASR stack to Apple Silicon.
> Current ASR options are:
> - `local`: MLX Whisper on-device (`mlx-community/whisper-large-v3-turbo`)
> - `glm-asr`: Zhipu GLM-ASR online API
>
> SenseVoice / Faster Whisper have been removed from this branch. macOS permissions now matter for first-run behavior:
> - Microphone: recording
> - Accessibility + Automation (`System Events`): paste text into other apps
> - Screen Recording: screen-aware assistant

### Option A: Installer (recommended)

Download `轻语.Whisper_x.x.x_x64-setup.exe` from the [Releases](https://github.com/sypsyp97/light-whisper/releases/latest) page. Run the installer — everything is bundled, no Python or build tools needed. ASR models will be downloaded on first use (~1–1.5 GB).

> [!NOTE]
> **GPU (optional):** NVIDIA GPU with up-to-date driver for CUDA acceleration. No GPU → automatic CPU fallback.

### Option B: Build from source

See [Quick Start](#quick-start) below.

> [!NOTE]
> The packaged app bundles its own Python runtime as `engine.tar.xz`. If you change Python dependencies or ASR runtime code, rebuild the engine first:
>
> ```bash
> uv run python scripts/build_engine.py
> pnpm tauri build
> ```

## Highlights

<table>
<tr>
<td width="50%">

**One-key dictation**<br>
Hold <kbd>F2</kbd> (configurable) to record, release to transcribe & type into the active window.

**Two ASR engines**<br>
Local MLX Whisper on Apple Silicon, or GLM-ASR online (no local Python setup required for end users).

**Offline or online — your choice**<br>
MLX Whisper runs fully local on Apple Silicon. GLM-ASR calls a cloud API — just add an API key.

**GPU accelerated**<br>
Local engines auto-detect NVIDIA GPU for CUDA inference; fall back to CPU.

**Subtitle overlay**<br>
Transparent floating window shows live dictation status and assistant output.

**Voice assistant mode**<br>
Set a separate hotkey for assistant mode: speak a task, get a floating answer card with manual copy.

**Screen-aware assistant**<br>
Optional: capture full-screen screenshots as visual context for assistant mode. Auto-detects model image support; falls back gracefully.

**Hold or toggle**<br>
Recording mode: hold-to-talk or press-to-start / press-to-stop.

</td>
<td width="50%">

**AI polish**<br>
Optional LLM post-processing: fix homophones, punctuation, filler words; adapts tone to the foreground app.

**Multi-backend LLM**<br>
Built-in presets for OpenAI, DeepSeek, Cerebras, SiliconFlow — or any OpenAI-compatible endpoint.

**Adaptive learning**<br>
Learns structured ASR corrections and key terms; manually deleted hot words are blocked from coming back automatically.

**Edit selected text**<br>
Select text anywhere, press the hotkey, speak an instruction ("translate to English", "make it formal") to rewrite in-place.

**Context-aware assistant**<br>
If text is selected, assistant mode includes the selection and foreground app as context before generating.

**Real-time translation**<br>
Set a target language (8 presets + custom) — transcription results are translated before output.

**Input queue**<br>
Rapid consecutive dictations are queued and typed in order — nothing is lost.

</td>
</tr>
</table>

## Light-Whisper vs Typeless

| Feature | Light-Whisper | Typeless |
|:--------|:---:|:---:|
| **Pricing** | Free & open-source | 30-day free trial; Free tier (4k words/week); $12–30/mo |
| **Privacy** | Fully offline, data never leaves your machine | Cloud-based, zero-data-retention |
| **Open source** | ✅ | ❌ |
| **Platform** | Windows | Windows, Mac, iOS, Android; Web listed on pricing page |
| **Internet required** | ❌ Local engines offline; GLM-ASR & AI polish need API | Cloud service; no offline mode publicly documented |
| **ASR engines** | SenseVoice + Faster Whisper + GLM-ASR (switchable) | Cloud-based proprietary service |
| **Languages** | 5 (SenseVoice) / 99+ (Whisper) / zh dialects (GLM-ASR) | 100+ |
| **GPU acceleration** | Local NVIDIA CUDA; GLM-ASR needs no GPU | N/A (cloud) |
| **AI polish** | Multi-backend LLM, bring your own key | Built-in auto-editing |
| **Filler word removal** | ✅ Via AI polish | ✅ Built-in |
| **App-aware tone** | ✅ Detects foreground app | ✅ Adjusts based on context |
| **Adaptive learning** | ✅ Learns structured corrections; supports hot-word blacklist | ✅ |
| **Edit selected text** | ✅ Voice instruction rewrite | ✅ |
| **Voice assistant mode** | ✅ Separate hotkey + floating answer card | ✅ Ask-anything / quick-answer features |
| **Real-time translation** | ✅ 8 presets + custom | ✅ |
| **Screen-aware assistant** | ✅ Auto-captures screen for visual context | ❌ |
| **Subtitle overlay** | ✅ | ❌ |
| **Input queue** | ✅ | Unknown |

## Engine Comparison

| | SenseVoice (default) | Faster Whisper | GLM-ASR (online) |
|:--|:---:|:---:|:---:|
| **Chinese CER** | 2.96 % (AISHELL-1) | 5.14 % | 7.17 % |
| **English WER** | 3.15 % (LibriSpeech) | 1.82 % | — |
| **Languages** | 5 (zh/en/ja/ko/yue) | 99+ | Chinese + dialects |
| **Punctuation** | Built-in ITN | initial_prompt guided | Built-in |
| **Hot words** | ✅ | ✅ | ✅ (max 100) |
| **Model size** | ~938 MB | ~1.5 GB | Cloud (no download) |
| **Requires** | GPU/CPU (bundled) | GPU/CPU (bundled) | API key only |
| **Cost** | Free (local) | Free (local) | ¥0.06/min |

> [!NOTE]
> SenseVoice/Whisper CER source: [FunAudioLLM paper](https://arxiv.org/html/2407.04051v1), Table 6. GLM-ASR CER source: Zhipu AI.

## Build from Source — Requirements

> [!IMPORTANT]
> **macOS 14+ on Apple Silicon recommended.** Disk: ~10 GB free. These requirements are only needed for building from source — the packaged app bundles the runtime.

| Tool | Version | Purpose |
|:-----|:--------|:--------|
| [Rust](https://www.rust-lang.org/tools/install) | >= 1.75 | Backend |
| [Node.js](https://nodejs.org/) | >= 18 (20 LTS recommended) | Frontend build |
| [pnpm](https://pnpm.io/) | >= 8 | Frontend packages |
| [uv](https://docs.astral.sh/uv/) | >= 0.4 | Python env + engine packaging |
| Xcode Command Line Tools | latest | macOS toolchain |

> [!TIP]
> **GLM-ASR-only builds:** You can still build the app without using the local engine at runtime, but the packaged desktop app in this branch is expected to ship with the bundled Python engine.

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
uv sync               # Python deps
uv run python scripts/build_engine.py
```

### Build & Run

```bash
pnpm tauri build
```

The app bundle is in `src-tauri/target/release/bundle/macos/` and the DMG is in `src-tauri/target/release/bundle/dmg/`.

## macOS Permissions

On this branch, macOS privacy permissions are part of the normal app flow:

- The app requests microphone permission when you first record.
- The app requests Accessibility / Automation only when text input is actually needed.
- The app requests Screen Recording when screen-aware assistant capture is used.

If paste still fails after you allowed permissions, fully quit the app and reopen it. For repeated development builds, macOS TCC can keep stale trust records; installing and running the app consistently from `/Applications` is more reliable than launching different copies from `target/`.

## Architecture

```
┌──────────────┐                ┌──────────────┐                ┌─────────────────┐
│   React UI   │  Tauri IPC     │  Rust Core   │  stdin/stdout  │   Python ASR    │
│  TypeScript  │◄──invoke/emit─►│  (Tauri 2)   │◄────JSON──────►│  SenseVoice /   │
└──────────────┘                └──────┬───────┘                │  Faster Whisper │
                                       │                        └─────────────────┘
                                       ├─── HTTP ──► GLM-ASR API (online ASR)
                                       ├─── HTTP ──► LLM API (AI polish / assistant / translation)
                                       ├─── Screen Capture ──► full-screen screenshots → assistant context
                                       └─── User Profile ──► hot words + blacklist → ASR + LLM prompt
```

<details>
<summary><b>Key paths</b></summary>

| Layer | Paths |
|:------|:------|
| **Frontend** | `src/pages/`, `src/components/`, `src/hooks/`, `src/styles/` |
| **Rust commands** | `src-tauri/src/commands/` — audio, assistant, clipboard, hotkey, ai_polish, profile, window |
| **Rust services** | `src-tauri/src/services/` — funasr_service, glm_asr_service, audio_service, assistant_service, ai_polish_service, llm_client, llm_provider, profile_service, screen_capture_service, download_service |
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
<summary><b>Characters turn into periods when typing into apps</b></summary>

This happens when a Chinese IME intercepts `SendInput` Unicode events. Fix: switch to **Clipboard paste** mode in Settings, or toggle your IME to English mode.

</details>

<details>
<summary><b>Chinese text appears garbled</b></summary>

Enable Windows UTF-8 system locale support to resolve encoding issues:

`Control Panel` → `Region` (or `Clock and Region`) → `Administrative` → `Change system locale...` → check `Beta: Use Unicode UTF-8 for worldwide language support` → `OK` → restart Windows.

</details>

<details>
<summary><b>Hotkey not working</b></summary>

Default dictation hotkey is F2. Assistant mode supports a separate hotkey in Settings. If either is occupied by another program, change it to another combo (for example `Ctrl+Win+R`).

</details>

<details>
<summary><b>Why does assistant mode not type automatically?</b></summary>

Assistant mode now uses a floating answer card instead of auto-typing. This avoids writing generated content into the wrong place. Click `Copy` in the overlay if you want to paste it manually.

</details>

<details>
<summary><b>Why did a deleted hot word come back before?</b></summary>

Older builds only removed the item from the current hot-word list. New builds also remove its learning frequency and add it to a hot-word blacklist, so manually deleted noise should not come back automatically.

</details>

<details>
<summary><b>Log locations</b></summary>

- `%APPDATA%\com.light-whisper.app\logs\funasr_server.log`
- `%APPDATA%\com.light-whisper.app\logs\whisper_server.log`

</details>

## Acknowledgements

- [FunASR](https://github.com/modelscope/FunASR) & [SenseVoiceSmall](https://huggingface.co/FunAudioLLM/SenseVoiceSmall) — Alibaba DAMO Academy
- [faster-whisper](https://github.com/SYSTRAN/faster-whisper) & [large-v3-turbo-ct2](https://huggingface.co/deepdml/faster-whisper-large-v3-turbo-ct2)
- [GLM-ASR](https://bigmodel.cn/) — Zhipu AI
- [Tauri](https://tauri.app/) / [React](https://react.dev/)

## License

[Creative Commons Attribution-NonCommercial 4.0 International (CC BY-NC 4.0)](LICENSE)
