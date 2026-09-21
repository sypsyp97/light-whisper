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

- Capture resamples once and polls every 40 ms, sending complete 160-ms blocks instead of partial requests or merged 320-ms updates. CUDA decodes 160-ms chunks; CPU uses 320 ms. Finish flushes all real samples and the resampler tail; cancellation resets the session.
- CUDA initialization decodes one silent chunk and resets the session before reporting ready. With native ABI 0.4+, the first aligned VAD prefix (at most one second) is decoded once instead of replaying each chunk. Later audio keeps normal chunk boundaries; ABI 0.3 and CPU retain chunk-by-chunk feeds.
- A 16-second rolling window advances by 8 seconds. Committed text is append-only; the visible preview can change. There is no fixed 30-second recording reset.
- The encoder recomputes the current window. This is not an incremental encoder/KV-cache implementation.
- On CUDA, startup previews can add 320 ms of zero-valued right context after 640 ms of real audio. There are at most three attempts, ending when normal decoding produces text. Padded results never enter commits, language state, continuation prompts or final output.
- Language, topic hints and hotwords are fixed for each recording. They are separate from AI polish.
- VAD retains its trailing audio window but skips inference while an active segment is too short to end. Native captions render directly, without running an unused character-reveal animation.

## Build and packaging

Follow [Development](development.md) to build both native packages and the installer. Native builds require Visual Studio 2022 C++ tools, CMake and Ninja; CUDA builds were validated with CUDA 12.9 and MSVC 14.44.

The build pins audio.cpp revision `6c70f32d0d90a29a9863e556bd9712ce622f868b` and requires an exact match to `scripts/patches/audio-cpp-r2t2-windows-streaming.patch`. It writes libraries, licenses and SHA-256 manifests to ignored `src-tauri/resources/r2t2-native/{cpu,cuda}/`. CUDA targets architectures 75, 80, 86, 89 and 90.

The engine builder validates both manifests and shares byte-identical CUDA/CRT DLLs in `_internal`. Shared files are listed in each copied manifest. Standalone native packages retain their dependencies. Qwen's pinned cu12 provider includes CUDA, Vulkan and CPU; its duplicate base provider is excluded. Model weights are not bundled.

## Validation and limits

Q8 was tested on an RTX 4070 SUPER 12 GB. Checks covered five public clips, five short truncations, silence, a 126.6-second recording, exact final audio accounting, cancellation and restart. The packaged R2T2 process passed 31 requests; packaged Qwen CUDA and R2T2 CPU also passed smoke tests. This is limited regression coverage, not a broad accuracy benchmark or AMD validation.

Paired paced server playback for build `light-whisper-r2t2-5` measured:

| First caption after model readiness | Runtime 4 | Runtime 5 |
|:--|--:|--:|
| Chinese, first recording | 1.17 s | 0.92 s |
| English, first recording | 1.02 s | 0.90 s |
| Chinese, later recordings | 1.01 s | 0.91 s |
| English, later recordings | 0.87 s | 0.87 s |

Each version ran three repetitions in separate Chinese-first and English-first processes. Later-recording values are medians. Warmup itself took about 0.19 seconds before readiness; total initialization varied from 4.8 to 5.5 seconds across these runs. First-prefix coalescing removes repeated decoding of queued audio. Committed traces and final transcripts matched on the regression clips. These are two latency fixtures, not microphone-to-screen guarantees or a broad accuracy benchmark. The former Qwen 1.7B repeated-recognition strategy still produced its first caption sooner on these clips.

Capture scheduling was also simulated with real inference and 0, 20 and 150 ms of already-captured audio. At 150 ms of offset, two-run median first-caption time improved from 1.06 to 0.94 seconds in Chinese and 1.02 to 0.89 seconds in English; aligned inputs were effectively unchanged. These measurements isolate polling alignment, not the full desktop path. On the same full clips, redundant VAD calls fell from 43 to 5 and from 88 to 23 without changing committed traces or final text.

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
