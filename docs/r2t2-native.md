# R2T2 native backend

R2T2 replaces the Qwen3-ASR 1.7B option. Qwen 0.6B is unchanged. A resident Python IPC server calls audio.cpp through its C ABI on Windows, using CUDA or CPU. No WSL, vLLM or PyTorch is required; R2T2 does not support Vulkan.

## Fixed model

- Repository: `davidxifeng/Confucius4-R2T2-gguf`
- Revision: `a8e6b385d7df7eae9519363e07034a209004797a`
- File: `r2t2-q8_0.gguf` (Q8_0, 2,477,512,064 bytes)
- SHA-256: `19f5ccd624484bcb5d44301437de41560b0ecc40c430e8850dfeefefbe82ccf5`

The loader verifies the pinned file before loading it. Weights download
separately; old model caches are not deleted by migration. Model terms are
in [R2T2-MODEL-LICENSE.txt](../src-tauri/resources/R2T2-MODEL-LICENSE.txt).

## Streaming

- Capture resamples once and polls every 160 ms. CUDA decodes 160-ms chunks; CPU uses 320 ms. Finish flushes all real samples and the resampler tail; cancellation resets the session.
- A 16-second rolling window advances by 8 seconds. Committed text is append-only; the visible preview can change. There is no fixed 30-second recording reset.
- The encoder recomputes the current window. This is not an incremental encoder/KV-cache implementation.
- On CUDA, startup previews can add 320 ms of zero-valued right context after 640 ms of real audio. There are at most three attempts, ending when normal decoding produces text. Padded results never enter commits, language state, continuation prompts or final output.
- Language, topic hints and hotwords are fixed for each recording. They are separate from AI polish.

## Build and packaging

Follow [Development](development.md) to build both native packages and the installer. Native builds require Visual Studio 2022 C++ tools, CMake and Ninja; CUDA builds were validated with CUDA 12.9 and MSVC 14.44.

The build pins audio.cpp revision `6c70f32d0d90a29a9863e556bd9712ce622f868b` and requires an exact match to `scripts/patches/audio-cpp-r2t2-windows-streaming.patch`. It writes libraries, licenses and SHA-256 manifests to ignored `src-tauri/resources/r2t2-native/{cpu,cuda}/`. CUDA targets architectures 75, 80, 86, 89 and 90.

The engine builder validates both manifests and shares byte-identical CUDA/CRT DLLs in `_internal`. Shared files are listed in each copied manifest. Standalone native packages retain their dependencies. Qwen's pinned cu12 provider includes CUDA, Vulkan and CPU; its duplicate base provider is excluded. Model weights are not bundled.

## Validation and limits

Q8 was tested on an RTX 4070 SUPER 12 GB. Checks covered five public clips, five short truncations, silence, a 126.6-second recording, exact final audio accounting, cancellation and restart. The packaged R2T2 process passed 31 requests; packaged Qwen CUDA and R2T2 CPU also passed smoke tests. This is limited regression coverage, not a broad accuracy benchmark or AMD validation.

Paired paced server playback for build `light-whisper-r2t2-4` measured:

| First caption | Previous 160-ms runtime | Startup preview |
|:--|--:|--:|
| Chinese, warm | 1.32 s | 1.04 s |
| English, warm | 1.17 s | 0.87 s |

The first Chinese run took 1.26 s. Final transcripts and update counts matched the baseline. These are two audio fixtures, not microphone-to-screen latency guarantees. Startup previews add bounded work; they do not reduce total model computation. The former Qwen 1.7B repeated-recognition strategy still produced its first caption sooner on these clips.

## Reproduce checks

`scripts/benchmark_r2t2_startup.py` takes explicit model, library, dependency-directory and PCM16 WAV paths. Save a baseline with `--output baseline.json`, then test a candidate with `--baseline baseline.json`. It compares committed traces and final text, including silence, resets and short recordings. `--max-first-audio-seconds` checks audio position, excluding compute time.

The ignored Rust integration tests require these environment variables:

- `LIGHT_WHISPER_R2T2_TEST_PYTHON`: Python executable
- `LIGHT_WHISPER_R2T2_TEST_RESOURCES`: resource directory
- `LIGHT_WHISPER_R2T2_TEST_CACHE`: isolated model cache
- `LIGHT_WHISPER_R2T2_TEST_AUDIO`: mono 16-kHz PCM16 Chinese fixture matching the assertions in `native_integration_tests.rs`
- `LIGHT_WHISPER_R2T2_TEST_DATA`: writable test-data directory

Run sequentially with CUDA available; model downloads are disabled:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib native_integration_tests -- --ignored --test-threads=1
```
