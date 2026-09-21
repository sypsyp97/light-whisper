<div align="center">

# Light-Whisper

**Local and cloud speech-to-text for Windows**

[Chinese](README.zh-CN.md) | English

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

- **Dictation and translation** with hold-to-talk or toggle hotkeys, typing into the active app.
- **Local or cloud ASR**, with a floating subtitle window for local recognition.
- **AI polish and Jev**: configurable rewriting, with optional classification to skip unnecessary polishing and keep the original text.
- **Voice assistant and selection tools** for questions, rewriting, translation, explanations and search; optional app, selection and screenshot context.
- **Personalization** through hotwords, learned corrections, per-app instructions and translation settings.
- **Optional local history** with search, export, deletion and re-polishing; save audio to enable re-transcription.

## Get started

1. Install the Windows 10/11 x64 `*_x64-setup.exe` from [Releases](https://github.com/sypsyp97/light-whisper/releases/latest). Python and build tools are not needed.
2. In Settings, choose a microphone and recognition engine. Download its local model or configure a cloud API key and region.
3. Focus your target app, hold `F2`, speak, then release. Change the hotkey or recording mode in Settings.
4. Optionally configure AI polish or the assistant with a provider/model and API key or supported account login. Basic local dictation needs no LLM account.

AI polish, screen context and assistant web search each offer **Off / On / Auto**. Only Auto uses Jev to decide whether the work is needed. Configure TypeSafe (official), OpenRouter or Vercel once under **Automatic decisions → Jev**. Missing credentials, timeouts and uncertain answers fall back to the usual processing path. Translation and explicit edits still run.

Web search includes keyless Bing snippets and Exa MCP with a rate-limited free tier or your own optional API key. Free providers may block requests or require browser verification; the app reports these failures and continues without web sources.

Optional Jev checks review AI-learned correction rules and flag possible meaning changes after polishing. Meaning checks run in the background and never replace or delay your output. User-confirmed corrections are preserved.

## Recognition engines

| Engine | Runs on | Requirements |
|:--|:--|:--|
| **Confucius4-R2T2 Q8** | Windows-native CUDA / CPU | Separate ~2.48 GB model; native streaming, language and topic hints; no WSL |
| **Qwen3-ASR 0.6B Q8** | CUDA / Vulkan / CPU | Separate ~850 MB model; multilingual dictation |
| **GLM-ASR** | Cloud | API key and region; final results only |
| **Alibaba DashScope** | Cloud | API key, region and model; final results only |

Local models are cached after download; FireRedVAD is bundled. GPU acceleration is optional. R2T2 captions have an append-only committed portion and a revisable preview; native streaming does not guarantee lower latency than every other engine.

**Upgrading to 1.6:** saved Qwen 1.7B selections migrate to R2T2, which requires its own model download. Qwen 0.6B and existing cached models are preserved. R2T2 uses a separate [model license](src-tauri/resources/R2T2-MODEL-LICENSE.txt).

## Data use

Local ASR keeps audio on your PC. Cloud recognition sends audio to the chosen provider; enabled AI polish and Jev send text and processing requirements. Optional screenshot/selection context and web search may also send data to configured services. Jev keys are stored per provider in the system credential store. History and audio saving are optional and local.

## Development and documentation

- [Build, test and troubleshoot](docs/development.md)
- [Release procedure](docs/releasing.md)
- [R2T2 implementation and measured limitations](docs/r2t2-native.md)

## License and acknowledgements

Light-Whisper is open-source software under [GPL-3.0-only](LICENSE). Third-party components and models retain their own terms; see [NOTICE](NOTICE) and [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

Built with Tauri and React, using Qwen3-ASR / transcribe.cpp, Confucius4-R2T2 / audio.cpp and FireRedVAD.
