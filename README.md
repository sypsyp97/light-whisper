# Light Whisper

Light Whisper is a macOS dictation app built with Tauri, React, and Rust.

Current branch: `codex/apple-silicon-mlx-asr`

> [!IMPORTANT]
> This branch is macOS-focused and currently uses online ASR only:
> `Alibaba DashScope` and `GLM-ASR`.
> Legacy local engine values (`local`, `sensevoice`, `whisper`) are migrated to `alibaba-asr`.

## Features

**One-key dictation**
Hold <kbd>F2</kbd> by default, then release to transcribe and type into the active app.

**Online ASR engines**
Choose Alibaba DashScope/Qwen ASR or GLM-ASR in Settings. Configure the provider API key in the app; no local Python runtime or model download is bundled on this branch.

**AI polish and assistant**
Optional LLM post-processing can fix punctuation, filler words, and common ASR mistakes. Raw ASR can appear first, then update after AI polish; the result card shows ASR, AI, and total latency. The assistant supports screen context and web search.

**macOS permissions flow**
The app guides Microphone, Accessibility, Screen Recording, and Automation permissions and can open the matching Privacy & Security pane when macOS denies access.

## Quick Start

Requirements:

| Tool | Version | Purpose |
|:--|:--|:--|
| macOS | 14+ recommended | Shipping target |
| Xcode Command Line Tools | latest | Native build toolchain |
| Rust | 1.75+ | Tauri backend |
| Node.js | 18+ | Frontend build |
| pnpm | 8+ | Frontend packages |

Install and run:

```bash
pnpm install
pnpm tauri dev
```

Build a macOS app / DMG:

```bash
pnpm tauri build
```

After launch, open Settings and configure either:

- Alibaba DashScope: choose the region and model, then add the matching DashScope API key.
- GLM-ASR: add the GLM API key.

## Engine Comparison

| Engine | Runtime | Setup | Notes |
|:--|:--|:--|:--|
| GLM-ASR | Online API | API key | Final results only |
| Alibaba DashScope | Online API | Region, model, API key | Defaults to `qwen3-asr-flash`; model list can be refreshed |

Local ASR is the intended future differentiator for this branch, but it is not implemented yet.

## Development

Run frontend checks:

```bash
pnpm install --frozen-lockfile
pnpm build
pnpm test
```

Run Rust checks:

```bash
cd src-tauri
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```

Run the experimental native Swift package:

```bash
swift build --package-path native-macos
swift test --package-path native-macos
```

The Swift app is experimental until it builds and passes parity tests in CI.

## Architecture

```text
React UI  <--Tauri IPC/events-->  Rust Core
                                      |
                                      +--> GLM-ASR API
                                      +--> Alibaba DashScope ASR
                                      +--> LLM APIs for polish, assistant, translation
                                      +--> Web Search APIs
                                      +--> macOS permissions, clipboard, hotkeys, audio
```

Key paths:

| Area | Path |
|:--|:--|
| Frontend | `src/pages/`, `src/components/`, `src/hooks/`, `src/contexts/`, `src/lib/`, `src/i18n/`, `src/styles/` |
| Rust commands | `src-tauri/src/commands/` |
| Rust services | `src-tauri/src/services/` |
| Native Swift experiment | `native-macos/` |
| Design and parity docs | `docs/` |

## Troubleshooting

**Microphone, Accessibility, or Automation is denied**
Open macOS System Settings -> Privacy & Security and grant the requested permission. If a permission was just changed, fully quit and reopen the app.

**Online ASR reports authentication or region errors**
Confirm the selected engine, region, and API key belong to the same provider account and endpoint.

**Logs**
Tauri app logs are written under the macOS app data/log directory for `com.light-whisper.desktop`. App data is under:

```text
~/Library/Application Support/com.light-whisper.desktop/
```

This branch does not launch local Python ASR servers, so `funasr_server.log` and `whisper_server.log` do not apply.

## Acknowledgements

- [Alibaba DashScope](https://www.alibabacloud.com/help/en/model-studio/) and Qwen ASR / Omni
- [GLM-ASR](https://bigmodel.cn/)
- [Tauri](https://tauri.app/) and [React](https://react.dev/)

## License

This project is licensed under CC BY-NC 4.0. See [LICENSE](LICENSE).
