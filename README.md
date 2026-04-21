<div align="center">

# Light-Whisper

**macOS branch · Online Speech-to-Text for Apple Silicon**

[简体中文](README.zh-CN.md) | English

[![Tauri 2](https://img.shields.io/badge/Tauri-2.0-24c8db?style=for-the-badge&logo=tauri)](https://tauri.app/)
[![React 19](https://img.shields.io/badge/React-19-61dafb?style=for-the-badge&logo=react)](https://react.dev/)
[![Rust](https://img.shields.io/badge/Rust-2021-f74c00?style=for-the-badge&logo=rust)](https://www.rust-lang.org/)
[![License: CC BY-NC 4.0](https://img.shields.io/badge/License-CC%20BY--NC%204.0-lightgrey?style=for-the-badge)](LICENSE)

<br>

<img src="assets/icon.png" alt="Light-Whisper" width="128" />

<br>

**Press a hotkey, speak, release — text appears at your cursor.**

Current branch: `codex/apple-silicon-mlx-asr`

</div>

<br>

## Installation

> [!IMPORTANT]
> This branch is rebuilt on top of the latest `main`, but keeps macOS-specific input, hotkey, and permission flows.
> ASR on this branch is online-only:
> - `Alibaba DashScope`
> - `GLM-ASR`
>
> Legacy local engine configs (`local`, `sensevoice`, `whisper`) are migrated to `alibaba-asr`.

### Option A: Installer (recommended)

Build the mac app / dmg from source on this branch. The packaged app does not bundle any local ASR runtime or model payload.

### Option B: Build from source

See [Quick Start](#quick-start) below.

## Highlights

<table>
<tr>
<td width="50%">

**One-key dictation**<br>
Hold <kbd>F2</kbd> (configurable) to record, release to transcribe & type into the active window.

**Two online ASR engines**<br>
Alibaba DashScope or GLM-ASR. No local model download, no bundled Python runtime, no model directory management.

**AI polish**<br>
Optional LLM post-processing: fix homophones, punctuation, filler words; adapts tone to the foreground app. Built-in presets (OpenAI / DeepSeek / Cerebras / SiliconFlow) plus OpenAI-compatible and Anthropic API formats.

**Adaptive learning**<br>
Learns structured ASR corrections and key terms; manually deleted hot words are blocked from coming back.

**Subtitle overlay**<br>
Transparent floating window shows live dictation status and assistant output.

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

## Light-Whisper vs Typeless

| Feature | Light-Whisper | Typeless |
|:--------|:---:|:---:|
| **Pricing** | Free & open-source | Free tier (4k words/week); $12–30/mo |
| **Privacy** | Online ASR on this branch | Cloud-based, zero-data-retention |
| **Open source** | ✅ | ❌ |
| **Platform** | macOS branch | Windows, Mac, iOS, Android, Web |
| **ASR engines** | 2 online | Cloud proprietary |
| **Languages** | 5–99+ (engine dependent) | 100+ |
| **AI polish** | Multi-backend LLM, bring your own key | Built-in |
| **Screen-aware assistant + web search** | ✅ | ❌ |
| **Subtitle overlay** | ✅ | ❌ |

## Build from Source — Requirements

> [!IMPORTANT]
> **macOS 14+ on Apple Silicon.** These requirements are for this branch only.

| Tool | Version | Purpose |
|:-----|:--------|:--------|
| [Rust](https://www.rust-lang.org/tools/install) | >= 1.75 | Backend |
| [Node.js](https://nodejs.org/) | >= 18 | Frontend build |
| [pnpm](https://pnpm.io/) | >= 8 | Frontend packages |
| Xcode Command Line Tools | latest | macOS toolchain |

> [!TIP]
> This branch is online-ASR only. Add an Alibaba DashScope or GLM-ASR API key in Settings after launch.

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
cargo check --manifest-path src-tauri/Cargo.toml
```

### Build & Run

```bash
pnpm tauri build
```

The macOS app bundle is in `src-tauri/target/release/bundle/macos/` and the DMG is in `src-tauri/target/release/bundle/dmg/`.

## macOS Permissions

- Microphone: recording
- Accessibility + Automation (`System Events`): paste transcribed text into other apps
- Screen Recording: screen-aware assistant

If paste or screen capture still fails after granting permission, fully quit the app and reopen it.

## Architecture

```
┌──────────────┐                ┌──────────────┐
│   React UI   │  Tauri IPC     │  Rust Core   │
│  TypeScript  │◄──invoke/emit─►│  (Tauri 2)   │
└──────────────┘                └──────┬───────┘
                                       ├─── HTTP ──► GLM-ASR API
                                       ├─── HTTP ──► Alibaba DashScope ASR
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
| **Rust services** | `src-tauri/src/services/` — funasr_service, glm_asr_service, audio_service, assistant_service, ai_polish_service, llm_client, llm_provider, profile_service, screen_capture_service, web_search_service, download_service |
| **State** | `src-tauri/src/state/` — app_state, user_profile |

</details>

## Dev Commands

```bash
pnpm tauri dev          # Dev mode with hot-reload
pnpm tauri build        # Production build + mac app/dmg
pnpm build              # Frontend only
cd src-tauri && cargo check   # Rust type check
```

## FAQ

<details>
<summary><b>Online ASR is slow or requests fail</b></summary>

- Confirm the selected provider's API key is configured in Settings.
- For Alibaba DashScope, verify the region matches the console where your key was created.
- If requests are timing out, test with a different network or VPN and retry.

</details>

<details>
<summary><b>Permissions-dependent features do not work</b></summary>

- Grant Accessibility if hotkeys, selected-text capture, or auto-paste do not work.
- Grant Screen Recording if assistant/polish screen context is enabled.
- Grant Automation when the app needs to control other apps for paste actions.

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

Default dictation hotkey is F2. If occupied by another program, change it in Settings (e.g. `Ctrl+Win+R`).

</details>

<details>
<summary><b>Log locations</b></summary>

This branch no longer launches local Python ASR servers, so `funasr_server.log` / `whisper_server.log` do not apply.
Use the app log generated by Tauri and inspect the macOS app data directory under `~/Library/Application Support/com.light-whisper.desktop/` when needed.

</details>

## Acknowledgements

- [Alibaba DashScope](https://www.alibabacloud.com/help/en/model-studio/dashscope-api-reference/) — Qwen ASR / Omni online ASR
- [GLM-ASR](https://bigmodel.cn/) — Zhipu AI
- [Tauri](https://tauri.app/) / [React](https://react.dev/)

## License

[Creative Commons Attribution-NonCommercial 4.0 International (CC BY-NC 4.0)](LICENSE)
