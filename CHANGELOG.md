# Changelog

## 1.8.2 — 2026-11-17

- Discover new OpenAI models available to the signed-in account when Settings opens. The catalog now follows stable Codex releases and preserves the selected model.

## 1.8.1 — 2026-11-16

- Register `light-whisper` on the current user's PATH during installation. Open a new terminal to use the command directly.
- Keep the command available after repeated installation and remove its PATH entry during uninstall. Preserve other PATH entries, settings and history.
- Keep the command available if the user cancels closing the app during uninstall.
- Fix desktop discovery when the CLI runs from the installed `bin` directory.
- Document direct commands for PowerShell and Command Prompt.

## 1.8.0 — 2026-11-15

- Added a standalone Windows CLI with JSON output and JSONL sessions. It can inspect and control the desktop instance or run a separate local engine. See the [CLI guide](docs/cli.md).
- Added desktop engine selection, start and restart commands, model checks, device enumeration, status, statistics and file transcription.
- Made diagnostics distinguish saved engine selection from observed runtime state. Independent engine runs use the saved local selection unless an explicit option overrides it.
- Fixed stale engine settings after external changes, including changes during listener registration and overlapping desktop operations.
- Fixed invalid JSON output when an engine returns nonfinite numbers. The CLI reports a runtime failure, cleans up its child and stops the session.
- Fixed silent replacement of escaped unpaired Unicode characters in CLI request fields.
- Reject invalid Unicode and oversized desktop requests before dispatch. Sessions can continue after these input errors.
- Added offline engine debugging with an explicit native thread limit and isolated temporary state.
- Added private authenticated desktop communication, bounded messages and deadlines, and cleanup of owned processes and audio copies.
- Added CLI lifecycle, packaging and native application tests, with finite protocol models and release checks.
- Rebuilt CPU and CUDA native libraries in a neutral directory to remove local user paths from build inputs.
- Removed redundant source comments and adjusted source formatting.

## 1.7.7

Qwen 0.6B moved to audio.cpp and a different GGUF model file. If you select this model after an upgrade from the earlier runtime, download approximately 1.15 GB once. Interrupted downloads can resume if the server supports byte ranges.

## Historical model migration

Saved Qwen 1.7B selections migrate to Confucius4-R2T2, which requires a separate model download. Qwen 0.6B selections and existing model caches remain available. R2T2 has a separate [model license](src-tauri/resources/R2T2-MODEL-LICENSE.txt).
