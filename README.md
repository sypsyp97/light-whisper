<div align="center">

# Light-Whisper

**Local and cloud speech-to-text for Windows**

[简体中文](README.zh-CN.md) | English

[![Tauri 2](https://img.shields.io/badge/Tauri-2.0-24c8db?style=for-the-badge&logo=tauri)](https://tauri.app/)
[![React 19](https://img.shields.io/badge/React-19-61dafb?style=for-the-badge&logo=react)](https://react.dev/)
[![Rust](https://img.shields.io/badge/Rust-2021-f74c00?style=for-the-badge&logo=rust)](https://www.rust-lang.org/)
[![License: GPL-3.0-only](https://img.shields.io/badge/License-GPL--3.0--only-2f6f9f?style=for-the-badge)](LICENSE)

<br>

<img src="assets/readme-hero.png" alt="Light-Whisper dark mode dictation interface" width="100%" />

<br>

**Hold a hotkey, speak, release. Light-Whisper types the result into the active app.**

[Download Installer](https://github.com/sypsyp97/light-whisper/releases/latest)

</div>

## Features

- **Dictation and translation**: type into the active app with a configurable hotkey; choose hold-to-talk or toggle recording and an optional translation target.
- **Local or cloud recognition**: Qwen3-ASR on your PC, or GLM-ASR / Alibaba DashScope without local models.
- **AI polish and Jev**: four structure levels; optional Jev classification skips unnecessary polishing and preserves the original text. Result cards show processing times.
- **Subtitles**: a floating window shows live local recognition and processing status.
- **Voice assistant and editing**: ask questions or rewrite selected text; optionally include app, selection, or screenshot context. Mouse selection also offers translation, explanation, polishing, copying, and search.
- **Models and web search**: built-in providers or custom OpenAI-compatible / Anthropic endpoints; supported OpenAI Codex and Grok Build login options; model-native, Exa, or Tavily search.
- **Personalization**: hot words, learned corrections, deleted-term blocking, and per-app overrides for polishing, translation, screenshots, and custom instructions.
- **Local history**: opt-in records with retention controls, search, export, deletion, and re-polishing. Save audio optionally to enable re-transcription.

This README describes the current source; published installers may differ. Check the [release notes](https://github.com/sypsyp97/light-whisper/releases).

## Quick Start

1. Install the app, open Settings, and select a microphone and ASR engine. Download a local Qwen model or enter your cloud engine API key and region.
2. Focus the app where you want text, hold `F2`, speak, then release. Change the hotkey or choose toggle recording in Settings.
3. For AI polish or the assistant, select a provider and model, then configure its API key or supported account login. Basic local dictation does not require an LLM account.
4. To use Jev, enable **AI Polish → Jev smart polish skip**, choose TypeSafe (official), OpenRouter, or Vercel, and enter that service's separate API key. AI polish must also be enabled. Jev is off by default; failures/timeouts continue normal polishing. Translation, assistant/editing, and manual re-polish bypass this gate.

## Data Use

Local ASR processes audio on your PC; cloud ASR sends it to the selected provider. Enabled AI polish and Jev send text and processing requirements to their providers. Optional selection/screenshot context and web search can also send data to the configured services. Jev API keys are stored separately in the system credential store. History and audio saving are optional local settings.

## ASR Engines

| Engine | Runtime | Best for | Language / model | Notes |
|:--|:--|:--|:--|:--|
| **Qwen3-ASR 0.6B Q8** | Local GGUF engine | Speed-oriented Qwen dictation | Multilingual, Q8_0 | Separate ~850 MB download; bundled FireRedVAD; CUDA / Vulkan / CPU |
| **Qwen3-ASR 1.7B Q8** | Local GGUF engine | Quality-oriented Qwen option | Multilingual, Q8_0 | Separate ~2.19 GB download; bundled FireRedVAD; CUDA / Vulkan / CPU |
| **GLM-ASR** | Online API | Cloud ASR without local models | `glm-asr-2512` | API key + region endpoint |
| **Alibaba DashScope** | Online API | Qwen ASR / Omni on DashScope | Default `qwen3-asr-flash`; refreshable model list | API key + region + model |

Cloud engines return final results only and skip local Python startup. Local Qwen models download once and are cached; FireRedVAD is bundled. Both Qwen sizes support hot words and prefer CUDA, with Vulkan/CPU fallback.

## Installation

### Installer

Download the `*_x64-setup.exe` installer from [Releases](https://github.com/sypsyp97/light-whisper/releases/latest). The installer bundles the app runtime, so no Python or build tools are needed. Local ASR models download on first use.

GPU acceleration is optional. An NVIDIA GPU with a current driver enables CUDA; the app falls back to CPU when no GPU is available.

### Build from Source

Windows 10/11 x64 build tools (versions below match repository CI where specified):

| Tool | Version | Purpose |
|:--|:--|:--|
| [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) | 2019+ | MSVC C++ toolchain |
| [Rust](https://www.rust-lang.org/tools/install) | 1.93.0 (CI) | Tauri backend |
| [Node.js](https://nodejs.org/) | 22.14.0 (CI) | Frontend build |
| [pnpm](https://pnpm.io/) | 10.28.2 (CI) | Frontend packages |
| [uv](https://docs.astral.sh/uv/) | 0.11.30 (CI) | Python environment for local ASR |

Python 3.11 is selected by `.python-version`; the project supports Python 3.11–3.12. `uv` manages this environment.

```bash
git clone https://github.com/sypsyp97/light-whisper.git
cd light-whisper

pnpm install --frozen-lockfile
uv sync --frozen
pnpm tauri dev
```

Build a distributable installer. The bundled Python engine archive is intentionally
not stored in Git, so build it first:

```bash
uv run --locked python scripts/build_engine.py
pnpm tauri build
```

The NSIS installer is written to `src-tauri/target/release/bundle/nsis/`.
`pnpm tauri build` rejects a missing, empty, or non-XZ engine archive instead of
silently producing an installer without local ASR. For a patch that does not
change the packaged Python runtime, an existing verified `engine.tar.xz` may be
reused.

Optional local-model prefetch:

```bash
uv run python src-tauri/resources/download_models.py --engine qwen3-asr-0.6b
uv run python src-tauri/resources/download_models.py --engine qwen3-asr-1.7b
```

For China mainland downloads, set `HF_ENDPOINT=https://hf-mirror.com` before prefetching.

## Development Commands

```bash
pnpm tauri dev
pnpm check
pnpm build
pnpm test
uv sync
cd src-tauri && cargo check
```

## Troubleshooting

**Hotkey not working**: the current default dictation hotkey is `F2`. Change it in Settings if another app owns it.

**GPU not detected**: run `nvidia-smi`. Qwen3-ASR records its selected device/backend in `qwen3_asr_server.log`.

**Log locations**:

- App log: `%LOCALAPPDATA%\com.light-whisper.desktop\logs\app.log`
- Qwen3-ASR log: `%TEMP%\light_whisper_logs\qwen3_asr_server.log`
- Python stderr fallback: `%APPDATA%\com.light-whisper.app\funasr_stderr.log`

## Acknowledgements

- [Qwen3-ASR](https://github.com/QwenLM/Qwen3-ASR) & [transcribe.cpp](https://github.com/handy-computer/transcribe.cpp)
- [FireRedVAD](https://huggingface.co/FireRedTeam/FireRedVAD)
- [GLM-ASR](https://bigmodel.cn/)
- [Alibaba DashScope](https://www.alibabacloud.com/help/en/model-studio/) & Qwen ASR / Omni
- [Tauri](https://tauri.app/) / [React](https://react.dev/)

## License

Light-Whisper is open-source software licensed under the
[GNU General Public License v3.0 only](LICENSE). Personal, organizational, and
commercial use is permitted. Distribution in source or compiled form, whether
modified or unmodified, must comply with the GPL. Binary distributions must
make corresponding source code available when required by its terms.
Previously published versions remain under the license distributed with those
versions.

Third-party software, models, and fonts remain under their own licenses. See
[NOTICE](NOTICE) and [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
