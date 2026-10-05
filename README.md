<p align="center">
  <img src="assets/readme-banner.svg" alt="Light-Whisper — voice to text, right where you work" width="100%" />
</p>

<p align="center">
  <strong>Local and cloud speech-to-text for Windows.</strong><br />
  Hold a hotkey, speak, release. The text goes into your active app.
</p>

<p align="center">
  <a href="https://github.com/sypsyp97/light-whisper/releases/latest"><strong>Download for Windows</strong></a>
  &nbsp; · &nbsp; <a href="#get-started">Get started</a>
  &nbsp; · &nbsp; <a href="README.zh-CN.md">简体中文</a>
</p>

---

## Get started

1. **Install.** Download the Windows 10/11 x64 `*_x64-setup.exe` from [Releases](https://github.com/sypsyp97/light-whisper/releases/latest). The recognition runtime is bundled; no separate Python or build tools are needed.
2. **Choose an engine.** Open Settings, select your microphone, then download a local model or configure a cloud API key and region.
3. **Speak.** Focus the app you want to type into, hold <kbd>F2</kbd>, speak, then release. You can change the hotkey or switch to toggle recording in Settings.

Basic local dictation works without an LLM account. For rewriting, translation or the voice assistant, configure a provider and model with an API key or supported account login.

## What you can do

| Feature | In everyday use |
|:--|:--|
| **Dictation & translation** | Type into the active app with a hotkey; optionally translate to your chosen language. |
| **Live subtitles** | Follow local recognition in a floating window, with committed text and a revisable preview. |
| **AI polish** | Rewrite with your own instructions; Auto can keep the original when polishing is unnecessary. |
| **Voice & selection assistants** | Ask questions, rewrite, translate, explain or search; optionally include app, selected-text or screenshot context. |
| **Personalization** | Add hotwords, learn corrections and set instructions, translation and processing rules per app. |
| **Local history** | Opt in to search, export, delete or re-polish past text; enable audio saving to re-transcribe it. |

## Recognition engines

| Engine | Processing | Setup |
|:--|:--|:--|
| **Qwen3-ASR 0.6B Q8** | Local · CUDA / Vulkan / CPU | Download ~850 MB; multilingual dictation. |
| **Confucius4-R2T2 Q8** | Local · CUDA / CPU | Download ~2.48 GB; native streaming with language and topic hints. |
| **GLM-ASR** | Cloud · final results | API key and region. |
| **Alibaba DashScope** | Cloud · final results | API key, region and model. |

Both local engines run on Windows without WSL. Models download separately and are cached; FireRedVAD is bundled. GPU acceleration is optional. R2T2's committed subtitle text does not roll back, while its preview can change. Streaming alone does not guarantee lower latency; see [measured behavior and limitations](docs/r2t2-native.md).

<details>
<summary><strong>AI providers, automatic decisions and web search</strong></summary>

AI polish and the assistant can share a model or use separate providers and models. Presets include OpenAI, xAI, DeepSeek, Cerebras and SiliconFlow; custom providers support OpenAI-compatible and Anthropic formats. ChatGPT browser sign-in uses OpenAI's official local open-source app authorization and public Responses API; the account picker can reauthorize saved accounts and workspaces. Codex device-code sign-in remains available for compatibility. Grok Build account login uses the official CLI proxy. Model choices come from the authenticated upstream catalog, and account access and limits still apply. Subscription requests never fall back to a manually configured API key. Authentication failures refresh once; permission and exhausted-quota errors stop transport fallbacks.

AI polish, screen context and assistant web search each offer **Off / On / Auto**. Only Auto uses the selected decision model to decide whether the work is needed. Under **Settings → Decision model**, choose **JEV** through TypeSafe (official), OpenRouter or Vercel, or Liquid AI's **d1:free**. Missing credentials, timeouts or uncertain decisions fall back to the usual processing path; the decision model does not generate replacement text, and translation and explicit edits still run.

Auto polish skips processing at an 80% pass probability. When automatic screen context is enabled, an 80% `unneeded` screen decision is also required. Service errors fall back to normal polishing and are recorded in `app.log`.

Optional decision-model checks review AI-learned correction rules and flag possible meaning changes after polishing. Meaning checks run in the background without delaying or replacing output. User-confirmed corrections are preserved.

Search options include provider-native search where supported, keyless Bing snippets, Exa MCP with an optional API key, Tavily and Google grounding. Tavily and Google require keys. ChatGPT/Codex login uses Exa for web search. Services can rate-limit or require browser verification; the app reports search failures and can continue without web sources.

</details>

<details>
<summary><strong>Upgrading from before 1.6</strong></summary>

Saved Qwen 1.7B selections migrate to R2T2, which needs its own model download. Qwen 0.6B and existing model caches are preserved. R2T2 has a separate [model license](src-tauri/resources/R2T2-MODEL-LICENSE.txt).

</details>

## What's new in 1.7

- **Lifecycle fixes** for recording, hotkey replacement, microphone monitoring, OAuth sessions and model downloads, including stale callbacks and cancellation.
- **Subtitle finality**: late preview events cannot overwrite a completed subtitle result.
- **A required formal CI gate** combining bounded TLA+ state models, Lean contracts and implementation regression tests for each release candidate.

These checks cover application state and protocol contracts under [documented assumptions](formal/COVERAGE.md). They do not prove every compiled instruction, external dependency or recognition result. Read the [verification guide](formal/README.md) for reproducible commands and evidence.

## Your data

| Setting | Where data goes |
|:--|:--|
| **Local recognition** | Audio is processed on your PC. Initial model downloads require network access. |
| **Cloud recognition** | Audio is sent to your chosen recognition provider. |
| **AI polish, assistants & decision model** | Text and processing instructions are sent to the configured services when used. Enabled screenshot/selection context and search can also send images, selected text or queries. |
| **History & audio saving** | Both are off by default and stored locally when enabled. Audio saving requires history to be enabled. |

Decision-model API keys are stored in separate system credential slots for each provider.

## For developers

[Build & troubleshoot](docs/development.md) · [Release procedure](docs/releasing.md) · [R2T2 backend](docs/r2t2-native.md) · [Formal verification](formal/README.md)

Built with Tauri, React and Rust, using Qwen3-ASR / transcribe.cpp, Confucius4-R2T2 / audio.cpp and FireRedVAD.

Light-Whisper is open-source software under [GPL-3.0-only](LICENSE). Third-party code and models retain their own terms; see [NOTICE](NOTICE) and [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
