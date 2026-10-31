# Local inference measurements and implementation notes

Measured on 2026-10-31. The only GPU runtime-tested here is an RTX 4070 SUPER.
Checks run serially at BelowNormal priority with affinity limited to two logical
CPUs. These conditions describe the measurements; they do not establish speeds
on other processors or graphics cards. Original model weights and input clips
remain unchanged. No PyTorch, Triton or additional inference framework is required.

## Audio processing during startup

The setting can change while the recognizer initializes. The managed Python
child starts with enhancement disabled, and Rust applies the latest saved
preference before publishing readiness. The setting command no longer rejects
an otherwise idle recognizer merely because startup is in progress. The local
process owner cleans up failed or cancelled startup. Positive IPC acknowledgement
is required before readiness; a server error remains an error.

The setting stays off by default. The enhancer follows recognition-model loading,
idle suspension, resume and shutdown. Disabling it releases the enhancer and
bypasses audio processing. Changes during an active recording remain rejected.

The existing acknowledgement timeout is still 60 seconds. A sufficiently slow
first 54-MB checkpoint download can exceed that deadline and fail synchronization.
The startup-state fix does not add a download-progress protocol or prove that all
network-induced load failures disappear. Native latency tests use verified local
weights; packaged smoke tests use the verified local cache rather than testing
cold-network download speed.

## Real-time RE-USE

This research build selects the separate
[Real-time RE-USE checkpoint](https://huggingface.co/nvidia/Real-time_RE-USE/tree/4c0c186272ca853cd18d832fc5c28777b62f32d1),
54,055,764 bytes, SHA-256
`6eac4e8cc721f99a22cde7eac3c9f8e2826ced5dc8a4234fd1977de1ec0d3ade`.
Its license permits **noncommercial research and education only**. The complete
license is bundled separately from the retained offline model's OpenMDW license.

The independent native driver reuses existing GGML operators and the shared
safetensors reader. At 16 kHz it preserves temporal convolution and Mamba states,
uses a 640-sample periodic Hann window and a 320-sample hop, and reconstructs
exactly the captured sample count. The default is exit depth 8 and lookahead 1.
The driver supports depths 3–12 and lookahead 0–2; the settings page adds no
quality selector. Failed native computation requires a successful explicit reset
before more frames can run.

Initial measurements below precede the retained constant-placement change.
Three steady streams follow one first stream. Native-call time includes Python
validation/marshalling and synchronous native computation, not just GPU kernels.

| Backend, depth/lookahead | Clip | Processing p50 | Processing/audio ratio | Native frame p50 / p95 | First output wall p50 |
| --- | ---: | ---: | ---: | ---: | ---: |
| CPU, 8/1, two threads | 5.1 s | 32,450.0 ms | 6.363 | 123.0 / 177.3 ms | 293.7 ms |
| CUDA, 8/1, two threads | 5.1 s | 930.6 ms | 0.182 | 3.285 / 3.663 ms | 64.0 ms |
| CPU, 3/0, two threads | 1 s | 3,406.4 ms | 3.406 | 66.0 / 92.2 ms | 170.7 ms |
| CUDA, 3/0, two threads | 1 s | 119.4 ms | 0.119 | 1.943 / 2.242 ms | 44.7 ms |

These rows replay 20-ms capture blocks against absolute monotonic deadlines.
The application's 40-ms capture blocks produce the first default CUDA output
after 80 ms of captured audio: measured wall p50 86.7 ms, processing ratio 0.203,
and tail flush p50 18.7 ms. This is enhancer output, not the first ASR caption.
The CPU under this two-logical-CPU limit does **not** meet a 20-ms frame budget.
A separate equal-input 0.5-s comparison found one CPU thread slower than two:
5,680.3 versus 3,362.6 ms. The existing two-thread enhancer default is retained.

All measured configurations passed actual-DLL full-clip versus irregular-chunk
waveform equality, reset after a partially consumed recording, repeated-stream
equality, output ownership, finite-value and exact sample-count guards. Independent
batch NumPy equations also checked seven native depth/lookahead configurations,
raw spectral heads, interleaved handles and state history. This is numerical and
protocol evidence, not a comparison with the official PyTorch implementation or
a speech-quality evaluation across a corpus.

Reproduce with `scripts/benchmark_reuse_realtime.py`, explicit `--model`,
`--library`, `--audio`, `--backend`, `--output`, and optionally `--capture-ms 40`.
`--threads` controls the tested runtime; `--cudart` enables device-wide CUDA
memory snapshots. Resource samples and output checks are outside processing
timers. CUDA runtime/context residency and retained report/audio objects prevent
interpreting post-close host memory as a leak test on their own.

## Shared recognition optimizations

Recognition-only timings in this report keep audio enhancement off. Enhancer
frame/output timings are separate. Packaged on/off/unload/resume checks cover
combined execution, but their unpaced requests are not live microphone or
combined microphone-to-caption latency measurements.

The CUDA norm/affine fusion preserves multiplication and addition order and
retains the unfused route for unsupported shapes, types, aliases and retained
intermediates. With optional residual addition, 28 eligible synthetic graphs go
from three/four kernels to one. CUDA peers match exactly for 26,070 values;
CPU references stay within their declared tolerance. This is a kernel result,
not a threefold or fourfold ASR speedup.

Qwen now shares R2T2's raw log-mel prefix cache. Reflected boundaries are
recomputed, sample changes invalidate reuse, and a new loud peak recomputes the
global floor across old and new columns. Both frontends retain the same feature
bytes. A controlled one-FFT-worker benchmark used four AB/BA pairs and checked
4,848 complete feature hashes:

| Frontend workload | Original | Cached | Paired speed ratio |
| --- | ---: | ---: | ---: |
| 20 resident extracts | 36.653 ms | 6.677 ms | 5.490x |
| 60 growing extracts | 154.830 ms | 40.261 ms | 3.879x |
| 120 sliding 12-s windows | 575.331 ms | 587.936 ms | 0.979x |
| Two final/rewrite extracts | 28.715 ms | 29.757 ms | 0.989x |

This is a frontend-only measurement on derived public speech. It does not
measure complete recognition latency.

A separate test-first fix forwards the existing recognition-session CPU thread
budget through both frontends to full and appended-tail FFT work. Standalone
frontend calls retain automatic scheduling by default. The prior path silently
used PocketFFT automatic scheduling regardless of the session budget. Tests
observe the actual FFT request for budgets 1, 3 and default 0, and retain exact
feature traces for both models. This limits active FFT work; PocketFFT may still
retain a global pool of idle OS workers. Final native builds include this scheduling fix. Complete-model performance
results are collected separately from frontend work counts.

The combined runtime preserves the original results for Qwen's 12-decode
compatibility plan and 13 R2T2 cases, including 36-s rolling input and tails.
R2T2's CUDA ABBA replay used four independently loaded blocks, three measured
sweeps per block, and identical transcripts and committed traces:

| R2T2 input | Original whole-call p50 | Candidate whole-call p50 |
| --- | ---: | ---: |
| Chinese, 4.2 s | 1,246.5 ms | 1,254.7 ms |
| English, 10.56 s | 3,055.3 ms | 3,058.9 ms |
| Derived Chinese rolling input, 36 s | 10,985.2 ms | 10,955.9 ms |

There is no material whole-call median improvement in this comparison. The
36-s case's individual feed p95 changes from 74.105 to 69.743 ms; short-case
feed p95 stays approximately unchanged. These unpaced stream-call timings
exclude microphone waiting and do not measure word-error rate.

## CPU/GPU collaboration and upstream code

The current pipeline already uses CPU capture/resampling, VAD and log-mel/FFT
work with GPU recognition encoders and decoders. Qwen/R2T2 streaming encoder
embeddings remain on the device, and decoder prefill already uses queued copies
with bounded staging. Moving dense layers to the CPU is a different tradeoff
from overlapping preparation with GPU work.

An actual GGML scheduler trace found one CUDA compute split and six CPU-backed
inputs in the realtime enhancer. Four inputs are immutable between runs: two
sequence-index tensors, reverse indices and a 16-KiB zero SSM state, totaling
17,676 bytes. Assigning these leaves to the existing device backend before
allocation reduces split inputs from six to two. Input/output flags continue
to protect their lifetime. This removes four repeated uploads and their waits;
it does not make the mutable spectra permanently device-resident.

The four-block ABBA comparison preserves every waveform hash. Processing p50
changes from 103.300 to 102.158 ms for 0.5-s input and 922.948 to 896.417 ms for
5.1-s input. Long-input native frame p95 changes by about +0.35%. The modest
speed change accompanies a direct reduction in repeated transfers.

The practical CPU/GPU priority is to prepare the next audio block on the CPU
while the GPU handles the current graph, provided a trace shows enough GPU idle
time to justify the extra queue and buffer lifetimes. The current public native
calls are synchronous. Pinned host buffers, backend events and asynchronous
readback are available reference mechanisms, but pageable copies or an async
function name alone do not establish useful overlap. No such pipeline change
is included in this build.

For the enhancer, each spectral input pair is only 2,568 bytes and the three
output heads total 3,852 bytes per frame. Reducing submission waits and retaining
immutable leaves is therefore a better first experiment than moving dense
layers onto the CPU. CPU layer offload is mainly a separate capacity tradeoff
when a model cannot fit in VRAM; no speed benefit has been established for these
resident dense models. Reusing the current runtime also preserves CPU fallback
and avoids adopting another framework solely for its advertised benchmarks.

Primary-source references checked on 2026-10-31:

| Repository / pinned revision | Applicable mechanism | Constraints |
| --- | --- | --- |
| [GGML](https://github.com/ggml-org/ggml/tree/facdc44a67da17df4433adcc468b424f48e9ded2) and [llama.cpp](https://github.com/ggml-org/llama.cpp/tree/5047748f114564fcf271e6a648f60e2d2eae6264) | Existing backend placement, pinned host buffers, stream copies and events; closest reuse to our dependency | Generic cross-backend copies can still synchronize. An async function name alone does not prove overlap. MIT; preserve our existing CUDA targets. |
| [state-spaces/mamba](https://github.com/state-spaces/mamba/blob/e9594ce1c732d97440f0332fdc43170a2294dbfa/mamba_ssm/ops/triton/selective_state_update.py) | Single-step recurrence, state write, D skip and SiLU gate in one kernel, with separate state/output ownership | Apache-2.0. Official package depends on Linux/PyTorch/Triton; use the algorithm as a reference instead of importing that stack. |
| [causal-conv1d](https://github.com/Dao-AILab/causal-conv1d/blob/cd81f0413cad2fc1e6f17e785ac39f59aae690cd/csrc/causal_conv1d_update.cu) | Short depthwise-convolution state update, optional SiLU, circular-buffer approach | BSD-3-Clause; kernel supports widths 2/3/4. PyTorch CUDA extension is not a portable CPU/Windows replacement for the current runtime. |
| [whisper.cpp](https://github.com/ggml-org/whisper.cpp/tree/482279b8a18855ba741d7b88c30fcb3874691f59) | CPU audio preparation plus GGML backends | MIT. Its stream example reprocesses overlapping windows; it does not supply a ready-made heterogeneous pipeline for these model graphs. |

Mamba's update implementation explicitly avoids autotuning that would overwrite
live recurrent state. Our pinned gated scan cannot simply bypass its retained
state-consumer guard: that fused route does not write the required final state.
Recent upstream direct-cache-write changes are Mamba-2-specific. A Mamba-1
state/output fusion therefore needs its own independent numerical and lifetime
guards rather than an eligibility switch.

[KTransformers](https://github.com/kvcache-ai/ktransformers) and
[HybriMoE](https://github.com/PKU-SEC-Lab/HybriMoE/tree/508a85b9a43dff37124370d8816e411e74827803)
provide useful examples of heterogeneous expert scheduling. The present dense
Qwen/R2T2 models have no routed-expert branch to place on the CPU. Their MoE
benchmark gains do not establish a gain for these ASR models. No new framework
or GPU-specific dependency was introduced from these investigations.

## Excluded candidate

A device-memset experiment preserved waveform hashes and reduced reset from
roughly 14 ms to 0.5 ms and tail flush from roughly 19–21 ms to 7 ms. However,
short-input whole-processing p95 worsened in both small ABBA/BAAB blocks:
105.37 → 115.45 ms and 108.93 → 152.80 ms. The long-input regression did not
repeat. Desktop scheduling noise remains unresolved. The candidate stays out of
the delivered runtime under the tail-latency acceptance criterion; the original
reset path remains in use. No overall speed claim is made from the reset result.

## Final native correctness checkpoint

The final `light-whisper-asr-14` CPU and CUDA builds use native patch SHA-256
`880e8d9dd4453c8822de4251c2410e6ce38a3493b80d9fd84b620d1b5a39e052`.
The original CUDA architecture target list is retained. Actual execution here
covers one RTX 4070 SUPER and the local CPU under the stated affinity limit.
Final realtime native tests pass 7/7 on each backend; retained offline tests
pass 6/6 on each. Qwen passes the 12-decode CUDA plan, and R2T2 passes all
13 CUDA stream cases against the original frozen references. Both CPU models
also match freshly captured Chinese transcripts through the real C ABI.

A single CPU full-call correctness replay at two threads measured Qwen
5,167.1 → 4,904.6 ms and R2T2 10,978.6 → 11,020.7 ms, with identical text.
These are one-shot observations, not a repeated CPU performance result. Model
load and session creation are excluded from those two call timings.

## Final Qwen fixed-scheduling comparison

Both libraries use the current product-VAD Python path and scheduling variant B.
Only the native library changes. Four ABBA worker processes contribute six
measured rows per fixture/scenario after warmup. Captured PCM and final text
match exactly throughout. Microphone IPC is not included. The mixed fixture is
Chinese followed by English, rather than a natural code-switch recording.

| Paced input | First caption p50, original / candidate | Stop p50, original / candidate | Stop p95, original / candidate |
| --- | ---: | ---: | ---: |
| zh | 776.9 / 775.3 ms | 74.7 / 77.4 ms | 99.3 / 84.7 ms |
| en | 1368.3 / 1368.0 ms | 234.0 / 223.3 ms | 316.5 / 277.0 ms |
| mixed-derived | 784.1 / 775.6 ms | 304.8 / 348.7 ms | 366.8 / 568.2 ms |

The first-caption change is small. The mixed-input Stop row regresses, with
a notably slow candidate iteration. Feed p95 for all three paced cases is
slightly lower, and native final-call times are generally close outside that
outlier; Stop also includes the phase of in-flight preview work. This first
round does not establish a robust end-to-end gain. A reverse-order confirmation is reported below.

## Final R2T2 comparison after FFT-budget routing

The same four-block ABBA plan was repeated with the final native runtime, three
measured sweeps per block. All transcripts and committed traces remain exact.

| Input | Whole-call p50, original / final | Feed p95, original / final |
| --- | ---: | ---: |
| zh-full | 1317.4 / 1270.9 ms | 76.279 / 75.171 ms |
| en-full | 3065.6 / 3040.8 ms | 76.654 / 76.387 ms |
| zh-rolling-36s | 11174.9 / 11044.7 ms | 72.538 / 70.281 ms |

These are modest complete-stream changes, roughly −3.5%, −0.8% and −1.2%
for the three whole-call medians. They include the combined native changes and
do not isolate the FFT budget alone. The earlier table is retained as a clearly
labeled pre-budget checkpoint, rather than substituted into final results.

### Reverse-order confirmation and limitation

A separate four-process BAAB round again preserves all final text and accepted
PCM. Six measured rows per fixture/scenario give:

| Paced input | Stop p50, original / candidate | Stop p95, original / candidate |
| --- | ---: | ---: |
| zh | 74.6 / 74.3 ms | 77.6 / 161.8 ms |
| en | 232.0 / 225.1 ms | 234.7 / 233.8 ms |
| mixed-derived | 268.9 / 312.6 ms | 377.6 / 399.3 ms |

The mixed-case Stop median is again about 44 ms higher. The first round's
median Stop-overshoot change is 89.510 → 132.481 ms; elapsed time after Stop
is observed changes from 209.810 to 215.755 ms. An in-flight preview therefore
accounts for most of the median gap, but it remains a real end-to-end result.
The large first-round tail also includes an unusually slow native final call
and non-native work. The reports cannot attribute that outlier to external
load. Pooled feed p95 does not cancel the observed Stop regression.

This is a **known performance limitation of the experimental local build**.
The combined changes do not meet a blanket no-Stop-regression claim for Qwen.
The existing thread budget is retained as a resource-control fix, and cache
reuse retains its measured CPU-work benefit; neither is presented as proof of
uniform latency improvement. The old installer is preserved for local A/B use.
No additional speculative scheduling or offload change is introduced to chase
these small, phase-sensitive samples.

## Repeated-load resource observations

Two model loads with two stream cycles per load complete for each ASR family
and library, with no reported cleanup errors. At the final post-GC snapshot:

| Family | Original handles / threads | Final handles / threads |
| --- | ---: | ---: |
| Qwen | 416 / 33 | 396 / 13 |
| R2T2 | 416 / 34 | 396 / 13 |

This supports the FFT-budget resource benefit. It does not make the entire
process single-threaded or establish a long-running leak test. CUDA context,
code and runtime memory remain resident after model close. Device-wide memory
samples include the desktop and other programs; they are not per-model VRAM.

The final realtime driver also passes three create/stream/reset/close cycles
per backend, including exact sample counts and irregular-partition waveform
checks. CPU post-close private bytes remain around 61.5 MB; handles are
176 / 177 / 177 across the three snapshots. CUDA handles remain 411 throughout
four closes, while host private bytes vary around 1.02–1.09 GB because context
and runtime residency remains. These short sequences are smoke evidence only.

In the final CUDA 5.1-s case, three steady streams give processing p50/p95
867.375 / 868.447 ms (ratio 0.170), and first enhanced output wall p50 87.101 ms
with 40-ms capture blocks. This is a final absolute measurement, rather than a
paired speedup claim against an earlier desktop-load condition. CPU remains
slower than real time under the two-logical-CPU limit.

The first CUDA instance in that final process took 454.322 ms to load, with OS
file-cache state uncontrolled. Its first stream emitted enhanced audio after
142.752 ms; later new instances first emitted after about 91–93 ms, and steady
streams after about 87 ms. Initialization and first-stream cost therefore must
not be replaced by the steady-output number. The final 5.1-s steady flush
p50/p95 is 17.806 / 22.022 ms.

## Completed application checks

- Python compile check and 307 resource tests pass.
- Frontend lint, 773 tests, one fluidity test, TypeScript/production build and
  bundle budget pass. Production JavaScript gzip total is 319,959 bytes.
- Rust formatting and Clippy (`--all-targets --locked -D warnings`) pass.
  Rust tests report 847 passed and 15 ignored, with one unrelated microperformance
  test filtered using the existing CI command.
- Engine atomicity, CPU/CUDA/backend, source-checkout, dependency-pruning,
  archive-layout, RE-USE packaging, release/CI and verifier-runner checks pass.
- Existing Verus checks pass for three contract domains covering six production
  functions, with eleven negative controls rejected. Scope is capture bounds,
  hotkey ownership and interim scheduling; this is not formal verification of
  the new neural driver. The first attempt used the default Rust environment
  and could not find Verus's required toolchain. Reusing its already installed
  isolated Rust environment resolves that failure without changing global settings.

The engine archive is rebuilt and passes current-source/native-fingerprint
validation. Installer-payload and actual packaged-engine results are recorded below.

## Delivered local installer

The local build is available at
`src-tauri/target/release/bundle/nsis/轻语 Whisper_1.7.11_local-realtime-20261031_x64-setup.exe`.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| Installer | 666,217,502 | `c0e44b72e5a6145d412ff923a8051102618137505e16d36e63a98c4bf5e16ed4` |
| Engine archive | 654,545,496 | `0dea53a29c5c0436d484a1f4c37bbf5695e15185f5eede9f8e81bf600443833c` |

The installer was extracted without executing it. Its archive matches the
current engine archive exactly. All 14 Python data sources, both asr14 native
manifests, listed native/shared dependencies, and the complete realtime license
match the build inputs. The GUI executable differs from the release executable
only by Tauri's documented `UNK` → `NSS` bundle-type stamp; every other byte is
identical, and the GUI embeds the new engine archive fingerprint. Root separately
recomputed installer/archive hashes and checked actual payload files.

The Windows license file is 4,095 bytes, raw SHA-256
`228986971739aac9c8832f2d0878e9ecc8d8c7dbaf6ec81b45d82283541606e9`.
It contains 35 CRLF line endings; normalizing only those endings gives the
pinned 4,060-byte source SHA-256
`6d1daa9c89ec421ba80fe051ac9f0dd46010d4fa4e8f4af654eb8e62f64e2ad5`.
Both packaged copies match the primary file byte for byte. The initial raw-byte
handoff assumption was corrected; no license terms or source code were changed.

Actual packaged `engine.exe` passes both Qwen and R2T2 lifecycle smokes on CUDA:
401 tagged requests and three recording sessions per family, covering processing
on, processing off, and processing on after real idle unload/resume. Every
session retains exactly 81,600 samples, produces a final response, and closes
with process exit 0. These are unpaced lifecycle/count checks. On/off transcripts
can differ; this is not evidence of recognition-accuracy improvement.

The 54,055,764-byte realtime weight file is not embedded in the installer.
The existing pinned downloader loads it only after explicit opt-in; the verified
local copy has been seeded into this machine's absent pinned Hugging Face cache
for the trial. No existing cache file was overwritten. The old dated installer
and prior native/engine bundles are retained. No installer was executed, no
running user application was stopped, and no source commit, push, or release
was made. Code remains in the working tree on the original main HEAD.

Local evidence receipts:

- `tmp/reuse-review-20261030/final-checks/receipt.json`
- `tmp/reuse-review-20261030/final-checks/root-independent-verification.json`
- `tmp/reuse-review-20261030/final-performance/qwen-two-round-summary.json`
- `tmp/reuse-review-20261030/final-performance/r2t2-abba/benchmark.json`
- `tmp/reuse-review-20261030/final-resources/`

The known Qwen mixed-input Stop regression and the CPU realtime limitation
remain explicit above. This delivery is a research/test build with measured
tradeoffs, not a universal speedup or cross-GPU runtime-performance claim.
