<p align="center">
  <img src="assets/readme-banner.svg" alt="Light-Whisper — local and cloud speech-to-text for Windows" width="100%" />
</p>

<p align="center">
  <strong>Local and cloud speech-to-text for Windows.</strong><br />
  Speak with a hotkey. Send the text to your active app.
</p>

<p align="center">
  <a href="https://github.com/sypsyp97/light-whisper/releases/latest"><strong>Download for Windows</strong></a>
  &nbsp; · &nbsp; <a href="#start">Start</a>
  &nbsp; · &nbsp; <a href="README.zh-CN.md">简体中文</a>
</p>

---

## Start

1. **Install.** Download the Windows 10/11 x64 `*_x64-setup.exe` from [Releases](https://github.com/sypsyp97/light-whisper/releases/latest). Start the installer. The installer includes the recognition runtime and its dependencies.
2. **Select an engine.** Open **Settings**. Select your microphone. Select a recognition engine. If the selected local model is missing, select **Start Download** on the main screen. For cloud recognition, enter your API key and region.
3. **Speak.** Put the text cursor in the target app. Hold <kbd>F2</kbd>. Speak. Release <kbd>F2</kbd> to send the text.

You can change the hotkey in **Settings**. You can also use toggle recording.

An AI account is not necessary for local dictation. For AI polish, translation, or the assistant, set the provider and model in **Settings**. Use an API key or account login.

## What you can do

| Function | What it does |
|:--|:--|
| **Dictation and translation** | Text input in the active app, with optional translation. |
| **Live subtitles** | Local results in a floating window, with committed text and a preview that can change. |
| **AI polish** | Text changes from your instructions. Auto mode can keep text without changes. |
| **Voice and selection assistants** | Questions, text editing, translation, explanations, and search. Optional context includes the app, selected text, or a screenshot. |
| **Personalization** | Hotwords, learned corrections, and different instructions and processing rules for each app. |
| **Local history** | Search, export, deletion, and AI polish for text in history. You can transcribe saved audio again. |
| **Command-line interface** | Desktop engine control, local diagnostics, file transcription, and JSON sessions for coding agents. |

## Recognition engines

| Engine | Processing | Setup |
|:--|:--|:--|
| **Qwen3-ASR 0.6B Q8** | Local · CUDA / CPU | Approximately 1.15 GB download; multilingual dictation. |
| **Confucius4-R2T2 Q8** | Local · CUDA / CPU | Approximately 2.48 GB download; native streaming with language and topic hints. |
| **GLM-ASR** | Cloud · final results | API key and region. |
| **Alibaba DashScope** | Cloud · final results | API key, region and model. |

The local engines operate on Windows without WSL. The app downloads models to a local cache. The app includes FireRedVAD. GPU acceleration is optional. Confucius4-R2T2 uses this [model license](src-tauri/resources/R2T2-MODEL-LICENSE.txt).

R2T2 does not change committed text. Preview text can change. Streaming does not always decrease latency. Read the [measurements and limitations](docs/r2t2-native.md).

Local engine settings include optional **Audio processing (RE-USE)**. This function uses NVIDIA Real-time RE-USE before recognition. It is off by default. When you first start this function, the app downloads a model of approximately 54 MB. Its license is for noncommercial research and education only. Read the [RE-USE terms and measurements](docs/r2t2-native.md#optional-re-use-audio-processing).

## AI services

AI polish and the assistant can use the same model or different models. Presets include OpenAI, xAI, DeepSeek, Cerebras, and SiliconFlow. You can also add a custom OpenAI-compatible or Anthropic provider. ChatGPT/Codex and Grok Build account login are available. Account permissions and limits can change which functions are available.

AI polish, screen context, and assistant web search have **Off**, **On**, and **Auto** modes. In **Auto** mode, a decision model selects when the app does each task. Set the decision model in **Settings → Decision model**.

Search options include provider search when available, Bing, Exa, Tavily, and Google grounding. For some options, an API key is necessary. If search fails, the app shows an error. The assistant can continue without web sources.

See [connection settings and service defaults](docs/api-defaults.md).

## Your data

| Setting | Where the app sends data |
|:--|:--|
| **Local recognition** | Your PC does the audio processing. Model downloads use network access. |
| **Cloud recognition** | The app sends audio to your selected recognition provider. |
| **AI polish, assistants, and decision model** | These functions send text and instructions to the services you select. Context and search can also send images, selected text, or queries. |
| **History and audio saving** | These settings are off by default. If you enable them, the app keeps the data locally. History is necessary for audio saving. |

Read the [Privacy policy](docs/privacy.md) for network requests, local storage, and third-party service policies.

## Command-line interface

The installer adds `light-whisper` to your user PATH. Open a new terminal, then run:

```powershell
light-whisper status
light-whisper transcribe "sample.wav"
```

The CLI can control the desktop instance or run a separate local engine.
It returns JSON and process exit codes. See the [CLI guide](docs/cli.md)
for commands and session examples.

## For developers

[Development](docs/development.md) · [Release procedure](docs/releasing.md) · [R2T2 backend](docs/r2t2-native.md) · [Formal verification](formal/README.md) · [Changelog](CHANGELOG.md)

Light-Whisper uses Tauri, React, and Rust. The local recognition engines use audio.cpp.

Light-Whisper is open-source software. It uses the [GPL-3.0-only](LICENSE) license. For third-party license terms, see [NOTICE](NOTICE) and [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
