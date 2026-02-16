<div align="center">

# Light-Whisper

**Local Offline Speech-to-Text Desktop App**

[简体中文](README.zh-CN.md) | English

[![Tauri](https://img.shields.io/badge/Tauri-2.0-blue?style=flat-square&logo=tauri)](https://tauri.app/)
[![React](https://img.shields.io/badge/React-19-61dafb?style=flat-square&logo=react)](https://react.dev/)
[![Rust](https://img.shields.io/badge/Rust-2021-orange?style=flat-square&logo=rust)](https://www.rust-lang.org/)
[![FunASR](https://img.shields.io/badge/FunASR-SenseVoice-green?style=flat-square)](https://github.com/modelscope/FunASR)
[![Whisper](https://img.shields.io/badge/Faster--Whisper-turbo-orange?style=flat-square)](https://github.com/SYSTRAN/faster-whisper)
[![License](https://img.shields.io/badge/License-Apache%202.0-blue?style=flat-square)](LICENSE)

<img src="assets/icon.png" alt="Light-Whisper Logo" width="120" />

*Press F2, speak, release to get text*

</div>

---

<!-- TODO: Add app screenshot or GIF here, e.g.: -->
<!-- ![Screenshot](assets/screenshot.png) -->

## Features

- **One-key transcription (F2 by default)** — Hold to record, release to transcribe, result is typed directly into the active window
- **Continuous speech without losing words** — When you start the next segment quickly, the previous result enters an input queue and is typed in order
- **Streaming feedback** — Intermediate results refresh at high frequency during recording for a near-real-time subtitle feel
- **Dual engine** — Switch between engines in settings with one click (see comparison below)
  - **SenseVoice** — High Chinese accuracy, built-in punctuation recovery (ITN), extremely fast inference
  - **Faster Whisper** — Supports 99+ languages
- **Fully offline** — All models run locally, no data leaves your machine
- **GPU acceleration** — Automatically detects NVIDIA GPU and enables CUDA; falls back to CPU if unavailable
- **Dual input mode** — SendInput (doesn't occupy clipboard) and clipboard paste (compatible with Chinese IME)
- **Floating window** — Borderless transparent window, always on top, minimizes to system tray
- **Launch at startup** — Can be enabled in settings

### Engine Comparison

| | SenseVoice (default) | Faster Whisper |
|---|:---:|:---:|
| **Chinese** | CER 2.96% (AISHELL-1) | CER 5.14% |
| **English** | WER 3.15% (LibriSpeech) | WER 1.82% |
| **Languages** | zh/en/ja/ko/yue (5) | 99+ |
| **Punctuation** | Built-in ITN | Built-in (initial_prompt guided) |
| **Model size** | ~938 MB (ASR + VAD) | ~1.5 GB |
| **Inference speed** | 10s audio in ~70ms | Fast (CTranslate2 accelerated) |

> Data source: [FunAudioLLM paper](https://arxiv.org/html/2407.04051v1) Table 6

---

## Requirements

> **OS**: Windows 10/11 (x64) only

| Tool | Version | Purpose |
|------|---------|---------|
| [Node.js](https://nodejs.org/) | >= 18 | Frontend build |
| [pnpm](https://pnpm.io/) | >= 8 | Frontend package manager |
| [Rust](https://www.rust-lang.org/tools/install) | >= 1.75 | Backend compilation |
| [uv](https://docs.astral.sh/uv/) | >= 0.4 | Python package manager (auto-installs Python 3.11) |
| [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) | 2019+ | Rust/C++ build dependencies |

**Disk space**: At least **10 GB** free (Python deps ~5 GB + models ~1–2 GB).

**GPU acceleration (optional)**: If you have an NVIDIA GPU, you do NOT need to install CUDA Toolkit separately — PyTorch ships with CUDA 12.4 runtime. Just make sure you have the latest [NVIDIA driver](https://www.nvidia.com/drivers/lookup/).

---

## Quick Start

### Step 0: Install prerequisites

If you already have all the tools above, skip to Step 1.

<details>
<summary><b>0.1 Install Visual Studio Build Tools</b></summary>

Rust on Windows requires MSVC C++ build tools.

1. Download [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
2. Run the installer and check **"Desktop development with C++"**
3. Restart your computer after installation

</details>

<details>
<summary><b>0.2 Install Rust</b></summary>

```powershell
# In PowerShell
winget install Rustlang.Rustup
# Or download from https://rustup.rs/
```

Verify:
```powershell
rustc --version   # Should show 1.75+
```

</details>

<details>
<summary><b>0.3 Install Node.js and pnpm</b></summary>

```powershell
# Install Node.js (LTS recommended)
winget install OpenJS.NodeJS.LTS

# Install pnpm
npm install -g pnpm
```

Verify:
```powershell
node --version    # Should show v18+
pnpm --version    # Should show 8+
```

</details>

<details>
<summary><b>0.4 Install uv</b></summary>

[uv](https://docs.astral.sh/uv/) is an extremely fast Python package manager. It **automatically downloads and installs the required Python version** (this project uses 3.11) — no need to install Python manually:

```powershell
# PowerShell
irm https://astral.sh/uv/install.ps1 | iex

# Or via winget
winget install astral-sh.uv
```

Verify:
```powershell
uv --version
```

</details>

<details>
<summary><b>Verify all tools are ready</b></summary>

Run the following in PowerShell to confirm everything is installed:

```powershell
node --version      # >= 18
pnpm --version      # >= 8
rustc --version     # >= 1.75
uv --version        # >= 0.4
```

If any command says "not recognized", the corresponding tool is not installed or not in PATH — go back to the relevant step above.

> **Python is not required separately**: `uv sync` will automatically download and manage Python 3.11.

</details>

---

### Step 1: Clone the repository

```bash
git clone https://github.com/sypsyp97/light-whisper.git
cd light-whisper
```

### Step 2: Install frontend dependencies

```bash
pnpm install
```

### Step 3: Install Python dependencies

```bash
uv sync
```

This will:
- Automatically download and install Python 3.11 (if not already present)
- Create a `.venv` virtual environment in the project root
- Install PyTorch (with CUDA 12.4), FunASR, faster-whisper, and other dependencies
- **Takes a while** (~5–15 minutes depending on network speed) because PyTorch is large

> **Network issues?** If PyTorch downloads slowly, see [FAQ](#faq) below.

### Step 4: Download ASR models (strongly recommended)

> **Important**: It is strongly recommended to download models manually before first run. In-app auto-download may fail or timeout due to network issues.

```bash
# SenseVoice engine (default, ~938 MB)
uv run python -c "from huggingface_hub import snapshot_download; snapshot_download('FunAudioLLM/SenseVoiceSmall'); snapshot_download('funasr/fsmn-vad')"

# Faster Whisper engine (~1.5 GB)
uv run python -c "from huggingface_hub import snapshot_download; snapshot_download('deepdml/faster-whisper-large-v3-turbo-ct2')"
```

Models are cached in `~/.cache/huggingface/hub/` and won't be re-downloaded on subsequent launches.

> **Model details**:
>
> | Engine | Model | Size | Description |
> |--------|-------|------|-------------|
> | SenseVoice | [SenseVoiceSmall](https://huggingface.co/FunAudioLLM/SenseVoiceSmall) | ~936 MB | ASR model — zh/en/ja/ko/yue with built-in punctuation (ITN) |
> | SenseVoice | [fsmn-vad](https://huggingface.co/funasr/fsmn-vad) | ~1.7 MB | Voice Activity Detection (VAD) |
> | Faster Whisper | [faster-whisper-large-v3-turbo-ct2](https://huggingface.co/deepdml/faster-whisper-large-v3-turbo-ct2) | ~1.5 GB | CTranslate2 format, 99+ languages, built-in Silero VAD |

> **Slow downloads from China?** Set a HuggingFace mirror:
> ```powershell
> $env:HF_ENDPOINT = "https://hf-mirror.com"
> ```
> Then re-run the download commands above.

### Step 5: Build and run

```bash
pnpm tauri build
```

The first build compiles all Rust dependencies and takes about **5–15 minutes**. After it finishes:

1. Run `src-tauri/target/release/light-whisper.exe` directly
2. Or find the installer in `src-tauri/target/release/bundle/nsis/` and install it
3. The app window appears at the center of the screen (borderless floating window)
4. Wait for the status to show "Ready" (progress is shown while models load)
5. **Hold F2 to speak, release to transcribe and type at the current cursor position**

---

## Usage

| Action | Description |
|--------|-------------|
| **Hold F2** | Start recording; release to transcribe |
| **Rapid consecutive F2 presses** | Speak multiple segments — results are queued and typed in order |
| **Click the circle button** | Manually start/stop recording |
| **System tray icon** | Right-click menu (Show/Hide/Exit); double-click to toggle |
| **Gear icon** | Open settings |

### Settings

| Option | Description |
|--------|-------------|
| **Recognition engine** | SenseVoice (Chinese-first) or Faster Whisper (multilingual); auto-reloads on switch |
| **Theme** | Light / Dark / Follow system |
| **Hotkey** | Default F2; customizable in settings (supports key combos) |
| **Input method** | Direct input (SendInput, doesn't use clipboard) or Clipboard paste (compatible with Chinese IME) |
| **Launch at startup** | Auto-run on system boot |

### Status Indicators

| Status | Meaning |
|--------|---------|
| `GPU: NVIDIA RTX...` | GPU acceleration enabled |
| `CPU` | Using CPU inference |
| `Loading model...` | Initializing model (~10–30s on first launch) |
| `Downloading 45%` | Downloading ASR model |

---

## Project Structure

```
light-whisper/
├── src/                        # Frontend (React + TypeScript)
│   ├── api/                    # Tauri API wrappers
│   │   ├── funasr.ts           #   FunASR service calls
│   │   ├── clipboard.ts        #   Clipboard / text input
│   │   ├── hotkey.ts           #   Hotkey registration
│   │   ├── window.ts           #   Window control
│   │   └── autostart.ts        #   Launch at startup
│   ├── pages/                  # Page components
│   │   ├── MainPage.tsx        #   Main UI (record + transcribe)
│   │   ├── SettingsPage.tsx    #   Settings page
│   │   └── SubtitleOverlay.tsx #   Subtitle overlay page
│   ├── components/             # Shared components
│   │   └── TitleBar.tsx        #   Title bar (drag, window controls)
│   ├── hooks/                  # React Hooks
│   │   ├── useRecording.ts     #   WebAudio recording logic
│   │   ├── useModelStatus.ts   #   Model status event listener
│   │   ├── useHotkey.ts        #   Global hotkey handling (customizable)
│   │   ├── useTheme.ts         #   Theme switching
│   │   └── useWindowDrag.ts    #   Borderless window dragging
│   ├── contexts/
│   │   └── RecordingContext.tsx #   Global recording state
│   ├── types/
│   │   └── index.ts            #   TypeScript type definitions
│   ├── styles/
│   │   └── subtitle.css        #   Subtitle overlay styles
│   └── main.tsx                # React entry point
│
├── src-tauri/                  # Backend (Rust + Tauri 2)
│   ├── src/
│   │   ├── lib.rs              #   App entry, plugin registration, tray
│   │   ├── commands/           #   Tauri commands (callable from frontend)
│   │   │   ├── funasr.rs       #     Start/stop/transcribe/status
│   │   │   ├── clipboard.rs    #     Copy/input (SendInput / clipboard paste)
│   │   │   ├── hotkey.rs       #     Hotkey registration
│   │   │   └── window.rs       #     Window control
│   │   ├── services/
│   │   │   ├── funasr_service.rs  # Python subprocess mgmt, JSON IPC
│   │   │   └── download_service.rs # Model download process mgmt
│   │   ├── state/
│   │   │   └── app_state.rs    #   Global app state
│   │   └── utils/
│   │       ├── error.rs        #   Error type definitions
│   │       └── paths.rs        #   Path utilities
│   ├── resources/              # Python scripts embedded in the app
│   │   ├── funasr_server.py    #   SenseVoice inference service (stdin/stdout IPC)
│   │   ├── whisper_server.py   #   Faster Whisper inference service (same protocol)
│   │   ├── download_models.py  #   Model download script
│   │   └── hf_cache_utils.py   #   HuggingFace cache detection utility
│   ├── Cargo.toml
│   └── tauri.conf.json
│
├── package.json                # Frontend dependencies
├── pyproject.toml              # Python dependencies (with CUDA 12.4 PyTorch)
├── vite.config.ts              # Vite build config
└── .python-version             # Python version constraint (3.11)
```

### Architecture & Communication Flow

```
┌──────────────┐     Tauri IPC      ┌──────────────┐   stdin/stdout   ┌───────────────────┐
│  React UI    │ ◄──── invoke() ───►│  Rust Backend │ ◄──── JSON ────► │  Python ASR Svc   │
│ (TypeScript) │ ◄──── emit() ─────│  (Tauri 2)    │                  │ SenseVoice/Whisper │
└──────────────┘                    └──────────────┘                  └───────────────────┘
```

1. **Frontend → Rust**: Calls Tauri commands via `invoke()`
2. **Rust → Python**: Sends JSON commands to the subprocess via stdin, reads JSON responses from stdout
3. **Rust → Frontend**: Broadcasts status events via `emit()`

---

## FAQ

<details>
<summary><b>Network issues: PyTorch or model downloads are slow</b></summary>

**Slow PyTorch download**: `uv sync` downloads PyTorch CUDA from `download.pytorch.org` (~2.5 GB). Since the project specifies the official PyTorch CUDA source, Chinese mirrors cannot accelerate this step. Suggestions:

- Use a stable network (VPN or campus network)
- `uv sync` supports resume on interruption — just re-run it
- Other Python dependencies are downloaded from PyPI and can be accelerated with a Tsinghua mirror:
```powershell
$env:UV_INDEX_URL = "https://mirrors.tuna.tsinghua.edu.cn/pypi/web/simple"
uv sync
```

**Slow model download**: ASR models are downloaded from HuggingFace Hub. Users in China can set a mirror:

```powershell
$env:HF_ENDPOINT = "https://hf-mirror.com"
# Then restart the app or manually download models
```

</details>

<details>
<summary><b>Python not found or wrong version</b></summary>

On startup, the Rust backend looks for `.venv/Scripts/python.exe` in the project root.

**Make sure you ran `uv sync` in the project root** — it automatically downloads Python 3.11 and creates the `.venv` directory. Verify:

```powershell
.venv\Scripts\python.exe --version   # Should show Python 3.11.x
```

</details>

<details>
<summary><b>GPU not detected</b></summary>

1. Make sure you have the latest [NVIDIA driver](https://www.nvidia.com/drivers/lookup/) installed
2. Verify PyTorch has CUDA support:
   ```powershell
   .venv\Scripts\python.exe -c "import torch; print(torch.cuda.is_available())"
   ```
   Should output `True`. If `False`:
   - Check that your driver version supports CUDA 12.4 (driver >= 525.60)
   - Confirm `uv sync` installed the CUDA build of PyTorch (configured in `pyproject.toml`)

If you don't need GPU acceleration, the app automatically falls back to CPU mode — no extra steps needed.

</details>

<details>
<summary><b>Hotkey not working or occupied</b></summary>

The default hotkey is F2. If it's occupied by another program (e.g., a game or utility), open Settings and change the "Speech hotkey" to another combo (e.g., `Ctrl+Shift+R` or `Ctrl+Win+R`).

</details>

<details>
<summary><b>Will results be lost when speaking two segments in a row?</b></summary>

No. The current version uses an **input queue**: even if you start a new segment before the previous result finishes typing, the earlier result is preserved and typed in order.

If you still notice delays, it's usually the target app processing input slowly (e.g., heavy editors, remote desktop, high-load scenarios). Try:
1. Switch input method to **Clipboard paste** (better compatibility)
2. Disable high-frequency auto-formatting plugins in the target app

</details>

<details>
<summary><b>Some characters become periods or garbled when typed at cursor</b></summary>

This happens because the default "Direct input" mode uses Win32 `SendInput` API with `KEYEVENTF_UNICODE` to simulate keyboard input character by character. **When a Chinese IME is active, it may intercept and mishandle these synthesized Unicode keyboard events**, causing some Chinese characters (e.g., "我", "你") to become other characters (e.g., "。").

**Solutions** (pick one):
1. **Recommended**: Open Settings and switch input method to **"Clipboard paste"** — this uses clipboard + Ctrl+V, fully compatible with Chinese IME
2. Switch your IME to **English mode** before using voice transcription (press `Shift` or `Ctrl+Space`)

</details>

<details>
<summary><b>Where are the app logs?</b></summary>

- **SenseVoice logs**: `%APPDATA%\com.light-whisper.app\logs\funasr_server.log`
- **Whisper logs**: `%APPDATA%\com.light-whisper.app\logs\whisper_server.log`
- **Rust/Tauri logs**: Output to console in dev mode

</details>

---

## Dev Commands

```bash
pnpm tauri build        # Build Windows installer
pnpm build              # Build frontend only
uv sync                 # Sync Python dependencies
uv add <package>        # Add Python dependency
cd src-tauri && cargo check   # Rust type check
cd src-tauri && cargo fmt     # Rust code formatting
```

---

## Acknowledgements

This project is based on [**ququ**](https://github.com/yan5xu/ququ) — thanks to the original author.

- [FunASR](https://github.com/modelscope/FunASR) — Open-source speech recognition by Alibaba DAMO Academy
- [SenseVoiceSmall](https://huggingface.co/FunAudioLLM/SenseVoiceSmall) — Multilingual ASR model (zh/en/ja/ko/yue)
- [faster-whisper](https://github.com/SYSTRAN/faster-whisper) — CTranslate2-accelerated Whisper inference engine
- [faster-whisper-large-v3-turbo-ct2](https://huggingface.co/deepdml/faster-whisper-large-v3-turbo-ct2) — CTranslate2 format Whisper model (99+ languages)
- [Tauri](https://tauri.app/) — Modern desktop application framework
- [React](https://react.dev/) — UI library

## License

[Apache License 2.0](LICENSE)
