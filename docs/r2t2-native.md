# Shared Qwen3-ASR and R2T2 native backend

Qwen 0.6B and R2T2 share a resident audio.cpp C ABI runtime and the same capture, session and caption protocol. Model selection remains independent. Qwen and R2T2 support NVIDIA CUDA and CPU. No WSL, vLLM or PyTorch is required. The retired transcribe.cpp loader and provider packages are excluded from the engine. Saved Qwen 1.7B selections migrate to R2T2; existing model caches are preserved.

Qwen 0.6B now uses `audio-cpp/audio.cpp-gguf`, revision `f861648c0c1f4da9f18e9a0ce5e217b0679bb2f5`, file `Qwen3-ASR-0.6B-GGUF/qwen3-asr-0.6b-q8_0.gguf` (1,151,272,416 bytes, SHA-256 `6c44ec2fb4cee513892d7863c1fcc3ea6b699ffa4d899b0ef4ab19956d9544f7`). This download is separate from the former transcribe.cpp model; the original cache is preserved.

Qwen capture polls every 40 ms. Its first nonempty preview is attempted at 120-ms audio intervals, then previews use 200-ms intervals and at most the latest 12 seconds. When inference falls behind, the capture task combines the waiting audio into one latest preview. Transport packets remain bounded to 5,120 samples; earlier packets append audio without decoding. Hypotheses remain revisable. The shared protocol rejects stale session IDs and discontinuous sample offsets.

Stop appends all pending audio and the resampler tail with previews disabled, then recognizes the complete recording. A native call already in progress can finish first. The final result never substitutes the latest window for the full recording. Preview suppression is restricted to Qwen; R2T2 retains its streaming chunk order and complete 320-ms acoustic finish context.

When a successful preview already processed the complete unchanged capture, finish reuses that result. Reuse requires the exact same captured sample count and a recording no longer than the preview window. Even one additional sample, or any earlier audio outside that window, requires the complete final decode. Reset and failed decoding discard the reusable result.

Captures longer than 115 seconds use native automatic 30-second chunking for final recognition. This stays within the pinned encoder's 1,500-position limit and applies the generation limit to each chunk, keeping the final partial chunk. Shorter captures keep the unsplit path.

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
- Profiles retain at most 300 hotwords; ASR selects at most 100 by weight and usage. The Python request boundary also keeps at most 100 unique hotwords, normalizes whitespace and ignores case when deduplicating. Empty entries and entries longer than 80 Unicode characters are skipped without shortening words. Streaming and offline requests use the same normalization.
- VAD retains its trailing audio window but skips inference while an active segment is too short to end. Native captions render directly, without running an unused character-reveal animation.
- Stop flushes 320 ms of acoustic silence inside the native decoder before collecting the final result. This is inference input, with no timer or extra microphone recording. Paired CUDA checks recovered the missing Chinese tails “带” and “喝” and English sentence tails; 80/160 ms remained insufficient on the Chinese clips. Captured sample counts and the live-feed schedule are unchanged; ending a segment adds bounded decoder work. The opt-in Rust test `real_native_stop_before_caption_refresh_keeps_sentence_tail` covers immediate Stop through the actual server and checks sample accounting.
- With native ABI 0.5+, CUDA rolling sessions append that complete context to the pending audio and decode it once in the final flush. The adapter enables `confucius4_r2t2.coalesced_finish` before session creation; the native option defaults to false. ABI 0.3/0.4, CPU, Qwen and nonrolling sessions retain their previous finish path. Empty recordings add no context.

## Build and packaging

Follow [Development](development.md) to build the two native packages and the installer. Native builds require Visual Studio 2022 C++ tools, CMake and Ninja; CUDA builds were validated with CUDA 12.9 and MSVC 14.44.

The build pins audio.cpp revision `6c70f32d0d90a29a9863e556bd9712ce622f868b` and requires an exact match to `scripts/patches/audio-cpp-r2t2-windows-streaming.patch`. Build `light-whisper-asr-13` writes libraries, licenses and SHA-256 manifests to ignored `src-tauri/resources/r2t2-native/{cpu,cuda}/`.

CUDA builds default to the pinned engine's portable architecture selection for the installed toolkit, independent of the build machine's GPU. CUDA 12 includes legacy PTX targets; newer targets are added when the toolkit supports them. CUDA 13 excludes Maxwell, Pascal and Volta. The builder clears stale architecture settings from its build cache and child environment, and records the concrete configured targets in `runtime-manifest.json`. An explicit `--cuda-architectures` overrides this default for experiments. A successful build establishes compiled coverage; performance and driver compatibility still need validation on the target hardware.

Default builds preserve that upstream list and append missing `80-real` and
`90-real` targets when upstream selected the corresponding generic PTX target.
This restores the native sm80/sm90 code shipped before asr12 while retaining PTX
and future upstream targets. CMake is configured again only when a target was
added; its final reported list must match the plan before compilation or package
replacement. Explicit architecture overrides and CPU builds retain their existing
behavior. The [architecture contract](cuda-architecture-verification.md) records
the regression checks, formal model and verification limits.

The engine builder validates both manifests and shares byte-identical CUDA/CRT DLLs in `_internal` when a root copy exists. Otherwise, dependencies stay beside the native library. Shared files are listed in each copied manifest. Standalone native packages retain their dependencies. Only CPU/CUDA native directories enter the engine; copied legacy Vulkan DLLs are removed before compression, and reused archives containing Vulkan libraries or retired transcribe.cpp packages are rejected. Model weights are not bundled.

The shared greedy decoder retains KV data on the device and reuses bounded prefill/decode graphs. Qwen uses 1,024-token prefill blocks and attention views over existing KV storage; R2T2 retains its validated 64-token blocks. CPU/CUDA greedy generation reads one 4-byte token ID per step. The opt-in argmax retains generation's first-maximum tie rule; classification retains its existing backend tie behavior.

Build `light-whisper-asr-13` keeps CUDA encoder audio embeddings on the device for Qwen and R2T2 streaming. The decoder uploads cached text rows and copies audio rows directly into one device prompt. The encoder output is borrowed only until prompt composition completes; the previous prompt has separate owned storage. CPU and the R2T2 offline path retain host embeddings.

Exact prefix reuse compares the original F32 storage as integer bits on the GPU, including signed zeros and NaN payloads. Only equality counts return to the CPU. Changed shapes, alternate prefill APIs, failure and graph release invalidate reuse; the final block still recomputes. Prompt and snapshot buffers retain one current capacity, replacing the old allocation on growth. Temporary comparison graphs evict their CUDA graph-cache entries before their metadata is freed.

CUDA prefill submits copies and block graphs on the same backend stream. Every block owns immutable host input, mask, position and last-token storage until a batch drains. Host payload staging is bounded to 32 MiB, or one oversized block; view metadata is bounded to the same batch. The implementation synchronizes before recycling storage, reading results, publishing a prefix snapshot, and freeing storage after an exception. CPU keeps one synchronous slot. Session reset preserves warm graph capacity; session destruction releases it.

## Validation and limits

### asr13 Qwen Stop and backlog replay

On 2026-10-25, an RTX 4070 SUPER ran eight separate resident-model workers in
ABBA/BAAB order, with the production FireRedVAD callback and one complete-clip
warmup per case. The 144 measured recordings provide twelve observations per
version, fixture and scheduling scenario. All final texts, languages and captured
PCM hashes match the original implementation. Values below are milliseconds,
shown as **p50 / p95**; model loading is excluded.

| Paced 40-ms arrivals | Previous | asr13 |
|:--|--:|--:|
| Chinese 4.20 s: first nonempty caption | 764.42 / 800.54 | 761.20 / 796.34 |
| Chinese 4.20 s: Stop to final | 58.44 / 77.06 | 57.02 / 58.48 |
| English 10.56 s: first nonempty caption | 1356.04 / 1427.58 | 1356.29 / 1365.67 |
| English 10.56 s: Stop to final | 170.42 / 229.71 | 171.74 / 176.36 |

| Entire clip queued when Stop arrives | Previous Stop to final | asr13 Stop to final |
|:--|--:|--:|
| Chinese 4.20 s | 896.03 / 1229.69 | 45.26 / 46.98 |
| English 10.56 s | 3768.99 / 5124.96 | 104.61 / 106.53 |

The backlogged Chinese/English cases execute 19/48 native decodes before the
change and one final decode afterward. They represent an explicit backlog,
not the normal no-backlog experience. Paced first-caption and Stop medians are
largely unchanged. These measurements replay capture scheduling in Python,
including in-flight work, but exclude microphone hardware, Rust IPC and display
rendering; the real Rust adapter has separate subprocess protocol tests.

The English phrase containing “25 to 30 years” and the Chinese sentence tail
remain intact. A separate concatenated Chinese/English fixture produces the
same incorrect transcript in both versions. Exact parity on that fixture is a
regression check, not an accuracy success or a natural code-switch benchmark.

A separate check used four natural mixed-language utterances from
[CAiRE/ASCEND](https://huggingface.co/datasets/CAiRE/ASCEND), configuration `main`,
test rows 9, 66, 68 and 81 (CC BY-SA 4.0), plus the existing English number and
Chinese tail clips. Both models retain exact before/after final text, language
and captured sample accounting on all six clips. R2T2 receives a separate
13-sample last feed; Qwen passes the byte-identical complete clip through the
production VAD callback and records any VAD trimming separately.

The four mixed clips contain 84 reference characters after NFKC/lowercase and
removal of spaces/punctuation. Qwen has 13 character edits before and after;
R2T2 has 5 before and after. The English number sentence and Chinese tail have
zero normalized edits in both versions. All four mixed clips share speaker 17,
so these are limited coverage observations, not corpus CER/WER or an accuracy
improvement. In particular, Qwen still recognizes only “language” in the clip
whose reference is “computer languages是”.

### asr13 native timing and attention experiment

The original profiler reported about 0.10–0.12 ms per encoder output read on
these clips, a small part of complete inference. A device-embeddings-only ABBA
run changed accumulated native-call medians by +1.3%, -1.4% and -5.1% on Chinese,
English and repeated 36-second Chinese respectively. No general speedup is
attributed to removing that transfer alone.

The combined device-input/queued-prefill candidate ran three four-process
rounds, each with one warmup and three measured recordings per case per worker.
All 108 measured and 36 warmup streams retain exact live/final text, language
and sample traces. The table gives **candidate / baseline** ratios of accumulated
native `start` + `feed` + `finish` median time; smaller is faster.

| Recording | ABBA | BAAB | BAAB repeat |
|:--|--:|--:|--:|
| Chinese 4.20 s | 0.9757 | 1.0161 | 0.9669 |
| English 10.56 s | 0.9604 | 0.9963 | 0.9756 |
| Repeated Chinese 36 s | 0.9520 | 0.9694 | 0.9477 |

The long case improves by 3.1–5.2% across these rounds. Short-case effects vary.
One candidate process in the first BAAB round contains a 2.42-second feed
outlier; English feed p95 rises 9.3% in that round. The repeat does not reproduce
that regression (feed p95 ratios 0.924/0.940/0.968), but the earlier observations
remain in the evidence. This does not establish a universal p95 improvement.
First-caption audio positions stay at 960 ms for Chinese and 1760 ms for English.
These are native-call measurements, not paced Stop-to-final or UI latency.

The rebuilt packaged servers were also replayed in old/new/new/old order, one
model process at a time, with one warmup and three paced 16-second recordings
plus a thirteen-sample tail per process. All twelve measured streams preserve
the same 256,013 captured samples, PCM hash, final text and language, and every
process exits normally. This adds production VAD and stdio IPC, while still
excluding microphone hardware, Rust capture and UI. Values are milliseconds:

| Packaged R2T2 paced replay | Previous p50 / p95 | asr13 p50 / p95 |
|:--|--:|--:|
| First nonempty caption | 1025.71 / 1032.31 | 1023.51 / 1025.53 |
| Stop to final | 117.54 / 120.95 | 116.03 / 118.26 |
| Finish request alone | 62.56 / 63.81 | 61.32 / 62.82 |

The roughly 1–2 ms median changes do not establish a substantial user-visible
R2T2 latency improvement; reduced accumulated inference work is a separate result.

The separate attention experiment uses synthetic post-projection Q/K/V with
the actual R2T2 model dimensions: 16 heads of width 64 and 104-token attention
windows. Eight boundary/padding cases ran dense, per-window explicit and window
Flash attention on CPU and CUDA. CPU maximum absolute differences were about
1.2e-7/3.0e-7 for explicit/Flash; CUDA differences reached 4.88e-4/1.15e-3.
CUDA timings varied by shape and did not consistently favor a replacement.
A deliberately wrong window changed all 212,992 outputs of its control case.
Production attention remains dense; no model-quality equivalence is claimed.

### asr13 resource lifetime checks

Separate baseline/candidate processes each loaded and closed Qwen three times
with eight measured recording cycles per load, and R2T2 three times with twelve
cycles per load. Including warmups, these are 144 complete recording checks,
plus short active recordings followed by cancellation or restart. No native
handles remained after close, no cleanup call failed, and repeated same-input
final text, language and sample counts matched. The complete 320-ms R2T2 finish
context remained present.

| Observation after each candidate model close + GC | Load 1 | Load 2 | Load 3 |
|:--|--:|--:|--:|
| Qwen process handles | 451 | 451 | 451 |
| Qwen process threads | 71 | 71 | 72 |
| Qwen private memory, MiB | 1831.19 | 1839.00 | 1848.19 |
| R2T2 process handles | 448 | 448 | 448 |
| R2T2 process threads | 72 | 69 | 69 |
| R2T2 private memory, MiB | 1929.85 | 1941.74 | 1949.78 |

The device query returned to the same post-close value in all three loads of
each family, matching the baseline. Repeated 16-second recordings also reached
the same device-query plateau within each load. Candidate plateaus were 8 MiB
higher for Qwen and 6 MiB higher for R2T2, consistent with retaining device prompt
and snapshot buffers; this attribution is an inference, not allocation tracing.
Peak process private memory was 4728.75/5977.21 MiB for Qwen/R2T2 versus
4708.14/5969.00 MiB before the change. Working set on the repeated long cases
remained roughly 884–919 MiB across both versions.

These short runs do not prove leak freedom. Both versions retain host allocations
after model close, and the candidate's post-close private memory grows by about
17/20 MiB across the three loads. The probe retains its own report; CUDA DLLs,
driver state and context remain loaded. `cudaMemGetInfo` supplies current-context
total and OS-reported free memory, not per-process VRAM or physical NVML usage;
the legacy JSON field name `cuda_global` must not be read as a sum of all process
allocations. See the [NVIDIA memory-query contract](https://docs.nvidia.com/cuda/archive/12.9.1/cuda-driver-api/group__CUDA__MEM.html).

Final local verification passes 238 Python resource tests, 67 native-build and
packaging-helper tests, 85 Rust native tests, and all three opt-in real
Rust/Python/CUDA integration tests using the required restaurant fixture. The
CPU/CUDA packages build from the exact reviewed patch applied to a fresh pinned
checkout; native numerical, deferred-buffer, failure/retry and CPU fallback
checks pass. Formatting and formal inventory/source bindings also pass.
The rebuilt `engine.tar.xz` passes source/manifest verification and both extracted
CUDA servers pass their protocol smoke. Packaged Qwen VAD counters are 0 before
feeding, 0 after suppressed feeds, and 1 after final; all 67,263 samples and the
separate thirteen-sample tail are retained. The old archive remains preserved.
These are local build artifacts, not an installation or a published release.

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

### Incremental frontend

Build `light-whisper-r2t2-6` reuses raw STFT magnitudes whose input samples remain unchanged. Appended audio recomputes the right-reflected boundary and new frames. The full mel transform and global log-mel floor still run on every window, so a louder append can change earlier normalized features. Rewritten, shortened, shifted or peak-renormalized audio invalidates reuse. Each frontend retains only its latest normalized PCM and magnitude window; shared copies synchronize cache access.

On the same i5-13600KF, a 36-second PCM replay with 160 ms appends and 16-second rolling windows reduced cumulative frontend median time from 1,053.4 to 323.6 ms (69.3%). Per-extraction p95 fell from 7.78 to 2.32 ms. Each variant had 12 measured replays and 2,700 measured extractions, following warmups in separate ABBA process blocks. These are CPU frontend timings, not equal full-model or microphone-to-screen speedups.

Separate CUDA process blocks compared complete native feeds on Chinese, English and repeated 36-second Chinese audio. Median feed time improved by 1.0%, 1.0% and 1.2%; p95 changed from 86.89 to 83.98 ms, 93.00 to 92.28 ms and 98.60 to 98.41 ms. These small full-model differences are close to runtime noise; the repeatable result is the frontend reduction. Encoder and autoregressive decoder work still dominate.

Regression checks compare every feature value with a fresh full extraction: 47 cases and 294 assertions cover reflected tails, non-hop endings, loud appends, normalization, sliding windows and independent owners. CUDA streaming checks cover 13 scenarios and 508 audio pushes, including a 36-second rolling recording, reset, silence and final speech appends. Committed, preview and final text and captured sample counts matched the baseline. The existing 320 ms finish context remains unchanged. This is fixture-based regression coverage, not a broad accuracy guarantee.

### Chunked prefill output work

Build `light-whisper-r2t2-7` executes only the decoder stack and KV writes on intermediate prefill blocks. The final block retains the original normalization, output projection and readback. Both graphs share the full graph's tensor allocation and use separate CUDA capture keys. Model weights, precision, attention, streaming windows and the 320 ms finish context are unchanged.

Warmed CUDA comparisons on the same RTX 4070 SUPER used ABBA and BAAB process blocks. Each of four processes per variant warmed all three clips, then ran two measured sweeps: eight runs per clip and variant. The table reports medians of summed stream start, feed and finish calls, excluding model loading, microphone capture and desktop rendering.

| Public audio | Runtime 6 | Runtime 7 | Reduction |
|:--|--:|--:|--:|
| Chinese, 4.20 s | 1,339.2 ms | 1,332.3 ms | 0.5%, within noise |
| English, 10.56 s | 3,642.2 ms | 3,553.7 ms | 2.4% |
| Chinese rolling, 36 s | 14,577.6 ms | 14,033.1 ms | 3.7% |

The rolling case improved by 4.5% and 3.5% in the two rounds. Its feed p95 fell from 96.63 to 92.69 ms. GPU clocks ranged from 2,910 to 2,925 MHz. These are fixture-based stream-call timings, not an isolated native prefill timer or a microphone-to-screen guarantee; Chinese short-clip improvement was not established.

Native CPU tests execute real ggml graphs with deterministic synthetic weights: 39 cases and 156 continuation steps preserve the frozen runtime's logits, hidden outputs and greedy-token traces bit-for-bit. They count actual graph operations and check that only the final block executes the output head and final normalization, including hidden-only mode, partial blocks, cache growth and graph rebuilds. Separate real-model CUDA checks retain all committed, preview and final outputs and sample counts across 13 scenarios and 508 pushes. Every benchmark replay also matches that reference. This is regression coverage, not a broad accuracy benchmark.


### Shared runtime and Qwen measurements

Build `light-whisper-asr-8` was compared with the preceding local installer engine on the same RTX 4070 SUPER. Each process warmed once and measured three repeated requests with identical PCM and the product VAD. These are server inference and wall times; they exclude model startup, microphone capture and desktop rendering.

| Chinese recording | Previous inference median | Shared inference median | Reduction |
|:--|--:|--:|--:|
| Natural speech, 31.12 s | 344.9 ms | 206.8 ms | 40.0% |
| Repeated speech, 32 s | 272.7 ms | 228.5 ms | 16.2% |

The natural-speech transcript matched exactly. The repeated clip retained seven complete repetitions and its partial final phrase; the older process omitted one complete repetition. A separate seven-fixture raw-native comparison preserved text and language across the KV-view optimization. The new GGUF package has different input-embedding storage and encoder behavior from the older provider; these checks do not establish broad CER/WER parity.

Three paced 16-second replays plus a 13-sample tail measured median first nonempty caption at 783.9 → 757.0 ms. Median per-run changed-caption gap p95 was 947.2 → 767.5 ms. All three new streams accepted exactly 256,013 samples and the original PCM hash. This Python stdio driver reproduces the scheduling policy; it does not execute the Rust capture loop or measure microphone-to-screen latency.

CPU synthetic checks retain 48 generation and 24 classification cases, 68 guards, and 8,736 bytes of frozen trace; resident generation performs zero host KV exports. A separate 57-case greedy readback check preserves 1,344 trace bytes, with 210 full-vocabulary reads replaced by 210 four-byte token reads. Actual CUDA argmax checks cover 39 finite-value tie/stride cases. R2T2 keeps its previous committed, preview, final and sample-count traces across 13 real-model scenarios and 508 pushes. The CPU Qwen DLL was also exercised on the packaged public Chinese clip.

### Qwen complete-preview finish reuse

A follow-up compares two previous and two updated packaged Qwen processes in old/new/new/old order. Each process runs all eight cases once for warmup and three times for measurement. The table pools six measured `stream_finish` requests per case; audio feed, microphone capture and desktop rendering are outside this interval.

| Complete unchanged preview | Previous finish median | Reused finish median |
|:--|--:|--:|
| Public Chinese clip, 4.20 s | 52.49 ms | 0.196 ms |
| Public English clip, 10.56 s | 127.45 ms | 0.204 ms |

The same clips with an additional one or thirteen samples, and repeated 16-second captures exceeding the preview window, still execute the full final callback. All 128 results across the four processes preserve exact text, language and captured sample counts against the reference. Existing VAD counters show zero final callback runs on eligible reuse and one on every fallback. Fallback median changes range from 17.3% lower to 2.34% higher; no fallback acceleration is claimed. An earlier source-versus-package run had a 26% higher Chinese one-sample-tail median, retained in the local evidence; the controlled packaged comparison did not reproduce it. Reuse eligibility frequency in real microphone recordings has not been measured.

### Bounded VAD feature reuse

Both servers reuse the current FireRedVAD owner's fbank and CMVN features only when normalized PCM is an unchanged prefix of the next input. Complete frames remain identical with `snip_edges=True` and zero dither; partial final frames retain their samples. Changed, shorter or shifted inputs rebuild. Each owner retains one prefix up to the Qwen preview limit of 12 seconds; longer final recordings clear the cache and use full extraction. Returned features own their storage, and extraction failures discard partial state. Full ONNX inference and speech segmentation still run on every call.

A local CPU comparison used the same frozen Chinese and English recordings, two warmups and eight alternating paired measurements against the original source. The table reports cumulative medians across 32 growing inputs ending at 12 seconds, excluding VAD construction/model loading.

| Recording | Feature extraction before/after | Complete VAD before/after |
|:--|--:|--:|
| Chinese | 156.5 / 15.4 ms | 448.6 / 305.4 ms |
| English | 159.1 / 14.6 ms | 450.7 / 303.3 ms |

Paired median feature reductions were 90.2% and 90.8%; complete VAD reductions were 32.1% and 32.8%. A 36-second replay with 12-second rolling windows reduced complete VAD time by 6.5% in both languages. All feature/probability bytes and speech regions matched. Complete VAD median/p95 ratios stayed below 1.05 across six workloads per language; no single-pass or whole-ASR speedup follows from these figures. Dedicated tests cover changed input, ownership, the cache bound, partial failures, silence/loud appends and one/thirteen-sample tails.

Qwen also caches optional GPU name/memory metadata for each loaded runtime, including failed probes. Runtime loads invalidate it, CPU fallback omits stale CUDA fields, and caller mutation cannot change cached metadata. This removes repeated status subprocess work; it does not accelerate the decoder.

Two Qwen encoder reuse experiments were discarded. Padded-token capacity graphs changed CPU softmax rounding; exact-shape reuse preserved all synthetic outputs and reduced graph rebuilds from 20 to 14, but four paired CUDA blocks showed only -0.98% to +0.23% changes across growing, repeated, complete and thirteen-sample-tail inputs. It did not reach the declared 3% retention threshold. The original native build and cuBLAS/Lt remain unchanged.

The final rebuilt engines were compared in old/new/new/old process order with one offline warmup and three paced 16-second replays per process. All twelve measured replays accepted the same 256,013 samples and PCM hash, preserving exact final text and language. First-caption median was 764.5 / 762.7 ms; finish-request median was 225.4 / 224.7 ms. The median of per-run changed-caption-gap p95 was 772.8 / 783.9 ms. All median/p95 guard ratios remained below 1.05. These small differences do not establish a whole-subtitle speedup; this Python stdio driver does not measure Rust microphone capture or desktop rendering.

### R2T2 final acoustic context

Build `light-whisper-asr-9` combines the pending real tail and the complete 320 ms acoustic context in one final decode for CUDA rolling sessions. Models, precision, rolling windows and live feed boundaries are unchanged.

On the same RTX 4070 SUPER, separate resident-model processes ran two rounds in ABBA/BAAB order, with one warmup per case and three measured sweeps per process. Each variant has twelve measured recordings per case. The interval below is `NativeRuntime.finish`; model loading, microphone capture, VAD, IPC and AI polish are outside it.

| Recording | Previous finish median | Combined finish median | Reduction |
|:--|--:|--:|--:|
| Chinese, 4.20 s | 85.90 ms | 30.42 ms | 64.6% |
| English, 10.56 s | 82.04 ms | 38.64 ms | 52.9% |
| Repeated Chinese, 36 s | 139.50 ms | 67.74 ms | 51.4% |

Both rounds and the aggregate passed the declared 10% finish target for the two original clips, with no greater-than-5% regression in first-caption compute time, feed p95 or finish p95. All 96 warmup/measured streams preserved exact committed, preview and final text, language and sample traces, and all eight processes closed normally. The final DLL also passes the thirteen-case, 508-feed base regression. A separate eight-case comparison preserves 729 feed responses across forced languages, context hints, mixed speech, rolling tails and real product VAD segmentation. Additional natural 31.12-second and FLEURS 10.38-second Chinese clips retain exact live/final text, language and sample traces across 260 feeds. CPU R2T2 output, cancellation, reset and captured counts match the preceding CPU package; CPU Qwen text and language also match. These public-fixture checks are limited regression evidence, not a broad CER/WER evaluation.

Rebuilt packaged engines were then replayed in old/new/new/old process order, with one offline warmup and three paced 16-second streams plus a thirteen-sample tail per process. All twelve measured streams retained exact final text, language, 256,013 captured samples and the accepted PCM hash, and every child exited normally. Median finish-request latency including VAD and stdio IPC fell from 201.95 to 73.29 ms; simulated audio-stop-to-final latency fell from 269.59 to 138.63 ms (48.6%). First-caption median was 1,023.64 / 1,029.34 ms. This driver exercises the packaged Python server and scheduling policy; it does not measure Rust microphone capture, desktop rendering or AI polish.

### Exact streaming prefill prefix reuse

Build `light-whisper-asr-10` queues the four prefill input uploads on the backend stream and reuses complete prefill blocks whose input embeddings are byte-identical to the previous prompt. Reuse requires the same graph shape and sufficient cache capacity; the final block always recomputes. Unused future KV rows are zeroed, including potentially non-finite old values. Alternate prefill/import APIs, failed prefills and graph release invalidate the saved prompt. Models, precision and audio feed boundaries stay unchanged.

The prefix experiment compares against the same queued-upload implementation on an RTX 4070 SUPER. Eight sequential resident-model processes in ABBA/BAAB order supply twelve measured recordings per variant/case, plus warmups. All 96 streams retain exact live/final text, language and sample traces. Groups with user-confirmed concurrent GPU use were excluded from timing. The retained clean comparison measures accumulated native start/feed/finish processing time, excluding recording duration, model loading, AI polish, VAD, IPC and UI rendering:

| Recording | Queued uploads | With exact prefix reuse | Reduction |
|:--|--:|--:|--:|
| Chinese, 4.20 s | 1,381.82 ms | 1,316.42 ms | 4.7% |
| English, 10.56 s | 3,609.32 ms | 3,397.21 ms | 5.9% |
| Repeated Chinese, 36 s | 13,796.94 ms | 12,382.93 ms | 10.2% |

Both orders favor the candidate in all three cases. The short Chinese effect varies by round and precedes any reusable block in the inspected long-stream diagnostic, so its full timing change cannot be attributed to avoided prefill alone. A separate four-process identical-DLL short-case control has a 0.013% aggregate difference. First-caption audio positions remain 15,360 samples for Chinese and 28,160 for English. The logged 36-second stream avoids 167 complete 64-token prefill blocks. CPU regressions preserve all numerical traces and greedy continuations; the prefix work slice executes 97 graphs versus 128 before reuse. These checks do not establish broad CER/WER coverage or a theoretical latency limit, and no preset speed percentage determines retention.

### Reuse before global log-mel normalization

The asr10 frontend also caches raw log-mel energies instead of complete STFT magnitudes. Exact normalized-PCM prefixes reuse only columns whose FFT windows are stable, capped by the previous log-mel output count because the final STFT column is discarded. Changed or right-reflected columns recompute; identical PCM performs no FFT or mel transform. Every result still applies the global maximum, floor and normalization across all columns, so a louder append can change historical normalized features correctly. Peak renormalization, rewritten audio and rolling-window shifts invalidate reuse.

The real CPU regression covers 141 observations across three FFT/hop/mel configurations, with a 4,861,231-byte feature/mask/shape trace identical to the frozen previous frontend. Interposition counts the actual unchanged sparse-mel arithmetic and FFT work; the previous implementation fails 63 work checks while the candidate passes all. The original 47-observation frontend regression also passes. On the same 225-extraction, repeated 36-second PCM replay, eight preserved-executable processes in ABBA/BAAB order produce 24 measured replays per variant. Median CPU frontend time falls from 320.405 to 161.996 ms; round ratios are 0.507 and 0.508. This 49.4% reduction applies to the CPU frontend stage.

An additional eight-process ABBA/BAAB CUDA comparison isolates this frontend change against retained prefix reuse. All 96 streams match the reference exactly. Total native processing medians are 1,324.96 / 1,326.17 ms for Chinese, 3,416.75 / 3,379.67 ms for English, and 12,044.69 / 11,981.90 ms for repeated 36-second Chinese. Short Chinese is effectively flat; English reverses direction between rounds. Long-stream round ratios are 0.980 and 0.999, with a 0.52% aggregate reduction. These small differences do not establish uniform whole-ASR acceleration. The repeated CPU-stage gain is retained; no repeatable whole-native latency regression was observed. Real CUDA 13-case/508-feed traces, eight forced-language/mixed/VAD scenarios with 729 feeds, and CPU R2T2/Qwen public outputs remain exact. First-caption audio positions are unchanged. Per-round first-caption accumulated-compute ratios are 1.000/0.983 for English and 0.996/0.986 for the long Chinese replay. Short Chinese adds 1.24/0.65 ms per round; this small effect remains unresolved with the available independent process blocks. These unpaced measurements do not establish unchanged microphone-to-screen latency.

### Temporal draft verification

Build `light-whisper-asr-11` retains three further changes at the existing model precision and audio feed boundaries:

- The reusable greedy decoder looks up only text embeddings; audio rows already have injected embeddings. It caches the latest text-ID sequence and its embeddings, publishing a refresh only after every lookup succeeds.
- CUDA KV suffix clears queue all layer memsets on the owning backend stream and synchronize once. An optional registry hook leaves the original synchronous clear available to CPU and backends without the hook. Rejected and unused rows are still zeroed, including old non-finite values.
- The previous prompt tail and generated token IDs supply a draft only when the current tail is their exact token prefix. Full four-token blocks are verified against the current prefill prediction and causal KV state. Each token retains the serial arithmetic shapes; graph scheduling groups matrix products by their shared weight tensor. Only the verified prefix enters the output and cache. A mismatch, small token budget, insufficient cache capacity or short draft uses ordinary serial decoding. EOS terminates the draft before verification, including when a shorter prompt exposes an EOS ID from the previous prompt tail. Failed verification drains uploads, restores the previous cache boundary and reinitializes the decode mask.

Dense multi-query verification changed a live punctuation boundary in the CUDA regression and was rejected. Scalar layer-major scheduling preserved outputs but made whole inference slower; grouping by weight avoids that measured regression. Rounding early prefill lengths to graph buckets was also rejected because it changed valid numerical traces. No preset speed percentage determines retention.

The retained CPU guards compare frozen asr10 arithmetic with real production owners and distinguish numerical equivalence from eliminated work. Prompt lookup covers 96 generations, 44 guards and four failed refreshes; lookup work falls from 212 to 72 graphs and from 7,616 to 2,364 rows. KV clear covers 23 cases, four invalid requests and an injected callback failure. Temporal verification covers 127 F32/F16/BF16 observations, 57 guards and 12 failure recoveries, plus 71 greedy integration calls. The latter preserve the original 68-call trace and add three EOS-tail transitions. Synthetic numerical traces match byte-for-byte; synthetic work counts are not model latency measurements. The final CUDA package also retains exact 13-case/508-feed and 8-case/729-feed context, mixed-language and product-VAD traces. Real CPU R2T2 and Qwen public outputs match the asr10 package.

The combined final package was measured against the original asr10 DLL on the RTX 4070 SUPER in eight sequential fresh processes, with ABBA then BAAB order. Each process warms all three cases and runs three measured sweeps: 24 warmup and 72 measured streams, all exact. Timings accumulate native start/feed/finish calls and exclude model loading, recording duration, VAD, IPC, rendering and AI polish.

| Recording | asr10 pooled median | asr11 pooled median | Pooled reduction | ABBA / BAAB reductions |
|:--|--:|--:|--:|--:|
| Chinese, 4.20 s | 1,364.84 ms | 1,261.59 ms | 7.56% | 6.51% / 5.90% |
| English, 10.56 s | 3,398.63 ms | 3,113.59 ms | 8.39% | 8.64% / 7.24% |
| Repeated Chinese, 36 s | 12,062.84 ms | 11,338.11 ms | 6.01% | 5.51% / 5.52% |

Both orders favor asr11 in every case. Clock variation between rounds makes the within-round comparisons preferable for the size of the effect; pooled medians are also shown for reproducibility. First-caption audio positions remain 15,360 Chinese / 28,160 English samples. First-caption accumulated compute improves by 1.4–4.0% across the six case/order combinations, and every feed-p95 comparison improves. These results measure the combined changes on these fixtures; they neither isolate each component's GPU contribution nor establish broad CER/WER parity, microphone-to-screen latency or a theoretical speed limit. An earlier mixed-object prompt experiment was invalidated after a rollback timestamp audit; only the clean rebuilt final package supplies the figures above.

## Device-measured output-head selection

Build `light-whisper-asr-12` groups only the four eligible F16 output-head projections during temporal verification. The transformer rows and their normalization remain separate. NVIDIA CUDA Events compare the existing scalar and four-column kernels on the actual device and matrix shape, after a complete initial bit comparison. A valid measured gain of at least 5% selects grouping; other results select the scalar path. The immutable decision belongs to one backend context. Capture misses and unsupported layouts use scalar execution without calibration. No GPU model names select a path, and ordinary unmarked matrix operations retain their dispatcher.

NVIDIA Nsight Systems confirmed the grouped kernel runs in real R2T2 inference. Two independent ABBA/BAAB experiments compared rebuilt asr11 and asr12 DLLs with the same CUDA 12.9, sm89 configuration on one RTX 4070 SUPER. Across 144 measured streams and 48 warmups, all 192 output traces and first-caption audio positions matched exactly. The English fixture's native start/feed/finish median improved by 4.81% and 4.40% in the two complete experiments. The last individual BAAB round improved by 2.96%; this is a repeatable full-experiment result, not a guarantee that every round exceeds 3%.

The first experiment failed latency guards, including a 5.80% increase in pooled Chinese feed p95. One unchanged replication was fixed in advance; all original observations were retained. Both that replication and the combined data pass all 36 measured/warmup guard comparisons for feed p95, first-caption compute and finish medians, with no increase above 5%. The broad 10% acceleration target was not reached. These measurements exclude loading, recording duration, IPC, rendering and AI polish; they do not establish other GPUs' performance or broad recognition accuracy.

The measured DLLs are isolated sm89 experiments, not portable release packages. Shipping requires rebuilding CPU and portable CUDA packages with matching asr12 manifests. Existing installed applications and asr11 resource bundles were not replaced by this experiment.

The controller's bounded TLA+ checks, source correspondence and native regression scope are documented in [Exact head verification](exact-head-verification.md). Initial numerical calibration and model checking do not prove floating-point equality for every possible input or compiler.

## Reproduce checks

`scripts/benchmark_r2t2_startup.py` takes explicit model, library, dependency-directory and PCM16 WAV paths. Save a baseline with `--output baseline.json`, then test a candidate with `--baseline baseline.json`. It compares committed traces and final text, including silence, resets and short recordings. `--max-first-audio-seconds` checks audio position, excluding compute time.

Native frontend regression (MSVC required; no model or CUDA):

```powershell
python scripts/test_r2t2_frontend_cache.py --source build/r2t2/source --build-dir build/native-tests/r2t2-frontend
```

`scripts/benchmark_r2t2_frontend.py` builds the real frontend without FFT mocks. Pass `--source`, `--build-dir` and a mono 16 kHz `--wav`. Build baseline and candidate separately with `--build-only`, then measure preserved executables with `--no-build` in ABBA order while other compilation and inference are idle.

`scripts/test_r2t2_native_cache.py` captures or checks complete CUDA streaming traces. Pass `--model`, `--library`, `--dependencies`, `--zh-wav`, `--en-wav` and a fresh `--output`; use `--reference baseline.json` to compare a candidate. It keeps the original finish padding and verifies exact audio accounting, including non-hop final appends.

`scripts/benchmark_r2t2_finish.py` compares preserved baseline and candidate wrapper directories and DLLs in separate sequential workers. Pass `--baseline-source-dir`, `--candidate-source-dir`, `--baseline-library`, `--candidate-library`, `--model`, `--dependencies`, `--reference`, `--zh-wav`, `--en-wav` and a fresh `--output-dir`. Defaults run two ABBA/BAAB rounds, one warmup per case and three measured sweeps per process. Exact live/final text, language and sample traces must match before timing is considered; native cleanup failures reject the worker. Keep other compilation and inference idle. `--self-test` checks the measurement guards without loading a model.

After building the native CPU runtime, extract the unmodified pinned runtime from its Git history and run the chunked-prefill unit check (MSVC required; no model or GPU):

```powershell
python -c "from pathlib import Path; import subprocess; Path('build/r2t2/prefill-baseline.cpp').write_bytes(subprocess.check_output(['git', '-C', 'build/r2t2/source', 'show', 'HEAD:src/framework/modules/transformers/qwen_causal_decode_runtime.cpp']))"
python scripts/test_r2t2_prefill.py --source build/r2t2/source --libraries build/r2t2/cpu --baseline build/r2t2/prefill-baseline.cpp --build-dir build/native-tests/r2t2-prefill
```

For real-model timing, `scripts/benchmark_r2t2_prefill.py` takes `--baseline-library`, `--candidate-library`, `--model`, `--dependencies`, `--reference`, `--zh-wav`, `--en-wav` and a new `--output-dir`. Use the frozen 13-scenario/508-push reference from `test_r2t2_native_cache.py`, `--rounds 2 --repeats 3 --order ABBA`, and idle other compilation/inference. It preloads the same CUDA dependencies in separate resident-model workers, alternates ABBA/BAAB order, checks every replay against the reference and saves raw timings, hashes and GPU snapshots. Independent process blocks and complete recordings are the repeated measurements; individual feeds are correlated.

The queued-upload lifetime/work guard and exact-prefix guard use the same CPU source/libraries arguments with `scripts/test_r2t2_prefill_async.py` and `scripts/test_r2t2_prefix_reuse.py`. Their `--baseline` is the frozen asr9 runtime with LF SHA-256 `e846316ce0953f9311479bb8083fea09d1917ea1541b0f05ddd3cdc912e54bd0`, retained before applying the asr10 patch. To reconstruct it, apply the patch from project commit `47064e24525b36b4b539d2c95cd6ab9d316931ab` to a separate checkout of the pinned audio.cpp revision. Each runner checks exact arithmetic against that baseline and requires the baseline to fail its separate work criterion. `--no-build` verifies compiled input and executable fingerprints before replaying.

The asr11 guards take `--baseline-root` pointing to a separate pinned audio.cpp tree with the patch from project commit `c64c7f136049037c4186e831a9ac88af4287e5bd` applied. Each runner pins the relevant asr10 owners, records raw input and executable hashes, and requires numerical GREEN with a separate work RED from the baseline:

```powershell
python scripts/test_r2t2_prompt_embeddings.py --source build/r2t2/source --libraries build/r2t2/cpu --baseline-root build/r2t2/asr10-source --build-dir build/native-tests/r2t2-prompt
python scripts/test_r2t2_kv_clear.py --source build/r2t2/source --libraries build/r2t2/cpu --baseline-root build/r2t2/asr10-source --build-dir build/native-tests/r2t2-kv-clear
python scripts/test_r2t2_temporal_verify.py --source build/r2t2/source --libraries build/r2t2/cpu --baseline-root build/r2t2/asr10-source --build-dir build/native-tests/r2t2-temporal
```

The same asr9 checkout supplies the four pinned frontend/DSP source and header files for the raw-log cache regression:

```powershell
python scripts/test_r2t2_logmel_cache.py --source build/r2t2/source --baseline-source build/r2t2/asr9-source --build-dir build/native-tests/r2t2-logmel
```

This check compares complete binary feature traces and counts actual sparse-mel and FFT work. Its baseline must pass numerics and fail the work contract; `--no-build` also validates source, generated overlay and executable fingerprints. Neither model weights nor CUDA are needed.

The asr13 queue guard uses the reviewed asr12 runtime from project commit
`b1ee2e4db5ab319928c3fd532df0dd217946c36a`, applied to the pinned audio.cpp
revision. Preserve that runtime before applying the current patch. Its raw
SHA-256 is `e11f876d71d4da9971f507c36d8429bc6dd5b70f0b4bc52625c4c559c2048081`:

```powershell
python scripts/test_r2t2_prefill_queue.py --source build/r2t2/source --libraries build/r2t2/cpu --baseline build/r2t2/asr12-source/src/framework/modules/transformers/qwen_causal_decode_runtime.cpp --baseline-sha256 e11f876d71d4da9971f507c36d8429bc6dd5b70f0b4bc52625c4c559c2048081 --mode all
python scripts/test_device_embeddings.py --source build/r2t2/source --libraries build/r2t2/cpu --baseline build/r2t2/asr12-source/src/framework/modules/transformers/qwen_causal_decode_runtime.cpp --mode all
```

The device runner adds a small, hash-checked host-copy API scaffold to this
baseline in its test build directory. This establishes the arithmetic oracle
and a failing transfer-work control; it is not an older production device API.
The queue guard checks deferred source-pointer lifetime, the staging limit,
failure with work still queued, CPU fallback, exact KV bytes and continuations.
Both runners support fingerprint-checked `--no-build` replay.

`scripts/benchmark_qwen_backlog.py` compares preserved Python source directories
and native DLLs in eight sequential ABBA/BAAB workers. Pass `--baseline-source`,
`--candidate-source`, `--baseline-library`, `--candidate-library`, `--model`,
`--dependencies`, `--zh-wav`, `--en-wav`, `--vad-model`, `--vad-cmvn` and a fresh
`--output`. The default scope uses the production VAD callback. It measures
40-ms paced arrivals and all-audio-queued-at-Stop separately, including an
in-flight sink batch. It reports first nonempty caption and Stop-to-final
p50/p95, callback work, exact final text and PCM hashes. Its mixed fixture is
concatenated Chinese/English, not a natural code-switch recording. This is a
Python scheduling replay; Rust capture/IPC has separate production-adapter tests.

`scripts/benchmark_asr_quality.py` is a separate six-clip observation helper.
Pass `--family qwen|r2t2`, `--model`, `--library`, `--dependencies`,
`--source-dir`, `--manifest` and a fresh `--output`; add `--reference` for the
candidate. Its explicit fixture manifest records official ASCEND row IDs,
references, license, original metadata and WAV/PCM hashes. The local experiment
keeps the manifest and provisioning script under
`tmp/asr-opt-20261025/natural-quality/`. Run `--self-test` without models first.
The helper reports human-reference character errors separately from exact
before/after parity and does not turn unchanged model mistakes into successes.

`scripts/benchmark_asr_resources.py` takes `--family qwen|r2t2`, `--model`,
`--library`, `--dependencies`, an explicit matching `--cudart`, and a new
`--output`. Use separate baseline/candidate processes, `--model-loads 3`,
`--long-seconds 16` and `--tracemalloc`. It records warmup, growing/shrinking
recordings, cancel/restart, finish, model close and GC, including process handle
and thread counts. Missing counters remain
unavailable. The CUDA device query is not per-process VRAM; process private memory and
Python allocations include the probe's own report. Trends are observations,
not a proof of no leaks. `--self-test` runs without a model or GPU.

`scripts/benchmark_encoder_attention.py` is a separate, synthetic numerical
experiment using dimensions read from `--gguf`. It compares dense masked,
per-window explicit and optional `--include-flash` attention with partial
windows and capacity padding. Use `--backend cpu|cuda`; `--no-build` verifies
compiled fingerprints. These outputs and timings do not establish model
quality or a lossless replacement. Production attention remains unchanged.

The ignored Rust integration tests require these environment variables:

- `LIGHT_WHISPER_R2T2_TEST_PYTHON`: Python executable
- `LIGHT_WHISPER_R2T2_TEST_RESOURCES`: resource directory
- `LIGHT_WHISPER_R2T2_TEST_CACHE`: isolated model cache
- `LIGHT_WHISPER_R2T2_TEST_AUDIO`: mono 16-kHz PCM16 Chinese fixture matching the assertions in `native_integration_tests.rs`
- `LIGHT_WHISPER_R2T2_TEST_DATA`: writable test-data directory

The required integration clip is the upstream
[restaurant sample](https://raw.githubusercontent.com/netease-youdao/Confucius4-R2T2/master/resources/test.wav),
107,840 samples, SHA-256
`b703174ef013bf7332ab74d4b65d568e2981cff2633cb620252bb4ba18ea7f67`.
Its assertions expect “顾客自己带酒水” and the “带”/“喝” truncated endings;
the shorter Qwen stock-market fixture cannot satisfy these assertions.

Run sequentially with CUDA available; model downloads are disabled:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib native_integration_tests -- --ignored --test-threads=1
```
