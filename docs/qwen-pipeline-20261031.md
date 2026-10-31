# Qwen Stop latency and CPU/CUDA pipeline

This follow-up addresses the mixed, long-input Stop regression documented in
`local-inference-20261031.md`. The asr14 native libraries, model weights, preview
thresholds and final audio are unchanged. No new framework or dependency is added.

## Implementation

CUDA Qwen now uses one persistent CPU preparation worker and one persistent
native inference worker. The stdio caller retains complete PCM ownership and
acknowledges feeds without waiting for either stage. One replaceable raw snapshot
and one replaceable prepared snapshot bound queued work. FireRedVAD has one CPU
owner, which can prepare the next snapshot while native CUDA inference runs.
Completed captions are published by the caller. CPU fallback retains the
original synchronous runtime.

Stop atomically discards obsolete queued previews and cancels an active CPU
preview's future GPU work. Its CPU operation drains, then the complete recording
is next on the same CPU owner. Full-recording preparation can overlap active
preview inference; final inference uses the same native worker. A preview can be reused only when it covers
every captured sample and the recording fits within the preview window. No native
call is interrupted, no PCM tail is dropped, and committed text still comes from
the complete recording.

The existing preview runtime supplies shared PCM, padding, cadence and result
publication logic. The shared stream controller now drains native work even when
audio-processor reset fails. Cancellation, replacement and shutdown wait for
active work; shutdown joins the worker before freeing the native session. Errors
published during CPU preparation are collected before another submission, and a
failed worker-thread start permits a later retry.

Protocol replies use the stdout pipe captured at server-loop entry. Background
stdout suppression remains enabled; each tagged response is explicitly flushed
to the protocol pipe.

## TDD and source verification

Tests were written by dedicated Test Writers in isolated worktrees. Main replayed
the meaningful failures, with independent local review before implementation.
Missing-interface failures were classified separately; the initial asynchronous
contract used an explicitly recorded declaration-only scaffold to expose its
blocked-feed failure.

The final combined Python resource suite passes **326 tests**. Compile checks and
`git diff --check` pass. Deterministic barriers cover actual CPU/native overlap,
latest-only execution, release of superseded prepared arrays, Stop completion
order, complete PCM and tails, context/language, caption cadence, deferred errors,
worker-start recovery, cancellation, replacement, shutdown and CPU fallback.
Calibrated NumPy-domain tracing subtracts intentional capture/window storage to
bound additional raw staging memory; weak references verify prepared-buffer
release. Stop and failure tests observe real Future cancellation/publication
barriers without replacing Future behavior.
Real buffered pipes cover per-request flushing and suppressed native noise.

The asynchronous ownership changes received independent source review. The
original native CPU/CUDA binaries and their prior numerical, model-parity and
architecture checks are retained; no new kernel or all-hardware speed claim is
made by this change. Frontend and Rust sources are unchanged by this follow-up.

## Measurement method and limits

The benchmark uses the product's runtime factory, with a frozen synchronous
asr14 Python baseline and identical asr14 CUDA library/model inputs. Both sides
use the existing coalesced feed schedule. Stop latency remains actual final
completion minus the original capture deadline, including an in-flight feed.
Input hashes, complete accepted PCM and final text/language are checked.

Capture releases 640 samples every 40 ms. The mixed fixture concatenates the
public Chinese and English clips (236,223 samples, 14.764 seconds); it is a
derived reproduction, not a natural code-switch recording. Enhancement is off
for this ASR scheduling comparison. Runs are serialized on the RTX 4070 SUPER,
with two logical CPUs. The final asr14 comparison uses the product's native CPU
budget of four threads; the original asr13 regression is also checked with a
budget of one. ORT VAD retains one thread. A fresh preview runtime is
closed after every capture; one native model/session remains resident per worker
process. A complete-clip warmup precedes each case.

Trace intersections measure CPU VAD overlapping a CUDA-backend `native.run`
invocation on a different thread. They are host call intervals, not CUDA event
timings or a kernel-utilization measurement. Text parity is not an accuracy
assessment; timing replay does not include microphone, Rust IPC or UI latency.

Local evidence is under `tmp/stop-pipeline-20261031/`; TDD gate decisions are in
`.codex/autoresearch/stop-pipeline-20261031/state.md`.

## Final pipeline measurements

The product-default comparison ran ABBA followed by BAAB in eight isolated
processes, with three captures per case in each process: twelve observations per
variant per case. Both variants use asr14 and the native budget of four. All six
cases preserve exact final text/language and accepted PCM, and every native
session closes. The table reports milliseconds, not end-to-end microphone latency.

| Case | Baseline Stop p50 / p95 | Pipeline Stop p50 / p95 |
| --- | ---: | ---: |
| Chinese paced | 78.042 / 228.747 | 75.075 / 110.186 |
| English paced | 224.333 / 239.045 | 135.138 / 183.941 |
| Derived mixed paced | 287.280 / 379.962 | 235.597 / 267.631 |
| Chinese backlog | 58.255 / 64.761 | 58.557 / 59.998 |
| English backlog | 132.031 / 136.049 | 133.727 / 138.815 |
| Derived mixed backlog | 178.467 / 217.388 | 178.182 / 208.571 |

Mixed-input Stop improves by 18.0% at p50 and 29.6% at p95. Backlog runs have no
preview work and retain essentially the same final inference cost; their medians
change by -0.3 to +1.7 ms. Mixed-input CPU/native interval overlap is 135.797 ms
per capture at p50 and 209.276 ms at p95, compared with zero in the synchronous
baseline. Short clips frequently finish preparation before the next native call,
so there is no claim of continuous overlap on every input.

Earlier Stop handling provides the latency gain. In the mixed case, time inside
`finish()` itself increases from 209.524 to 234.322 ms at p50, and its VAD time
from 86.125 to 111.539 ms under the two-logical-CPU restriction. The complete
deadline-to-result metric still improves because the caller no longer waits
through a synchronous preview before entering Stop. There is no claim that
threading makes each individual VAD or native operation faster.

First-caption publication has a measurable cost: Chinese/English/mixed medians
change from 769.886/1367.724/772.861 ms to 801.351/1401.025/800.830 ms. Results
are collected at feed boundaries, so asynchronous completion generally adds up
to one 40 ms capture poll. Chinese p95 in this run increases from 819.773 to
913.478 ms, including one 1001 ms first caption. A focused confirmation is
recorded separately below; this tail is retained rather than removed from the
reported data. Preview cadence and thresholds are unchanged.

Evidence: `tmp/stop-pipeline-20261031/paired-cpu-prep-asr14-default4/report.json`.

The original regression reference was checked separately with a native budget
of one: frozen synchronous Python plus the preserved asr13 DLL versus the final
pipeline plus asr14. Each order contains six observations per variant. Both
orders improve the mixed-paced Stop median and p95:

| Order | asr13 Stop p50 / p95 | Pipeline/asr14 Stop p50 / p95 |
| --- | ---: | ---: |
| ABBA | 289.752 / 345.381 | 236.179 / 242.477 |
| BAAB | 324.835 / 426.818 | 244.947 / 330.396 |
| Combined | 298.334 / 414.795 | 239.435 / 297.665 |

All final text/language and accepted PCM match. First-caption p50 changes from
774.469 to 801.350 ms and p95 from 826.086 to 841.312 ms. This check addresses the
recorded mixed-long Stop regression; it does not attribute all performance
changes exclusively to Python because the reference uses the old native DLL.
Evidence: `tmp/stop-pipeline-20261031/paired-cpu-prep-asr13-budget1/report.json`.

The Chinese first-caption confirmation repeats ABBA/BAAB with five captures per
process (twenty per variant). First-caption p50 is 768.722 versus 801.268 ms
(+32.546 ms), and p95 is 825.604 versus 850.197 ms (+24.594 ms). Stop p50/p95 is
74.583/121.173 versus 75.201/90.453 ms. All final text/language and accepted PCM
match, and every native session closes. The original larger tail remains in the
full-run table. This confirmation supports accepting a measured approximately
one-feed-poll publication cost, not a claim of zero first-caption regression.
Evidence: `tmp/stop-pipeline-20261031/paired-cpu-prep-zh-confirm/report.json`.

The Stop optimization is accepted on this machine with the disclosed caption
cost and unchanged backlog throughput. The preserved CPU fallback is covered by
the unit suite; latency results apply only to the tested CUDA hardware and replay.

## Rejected first experiment

The first implementation kept VAD on the caller and moved native inference to a
worker. Eight isolated worker processes ran ABBA then BAAB, with three captures
per process. Final text/language and PCM matched exactly, and all native handles
closed. The timing acceptance failed:

| Mixed paced case | Synchronous asr14 | Caller-VAD candidate |
| --- | ---: | ---: |
| Stop p50 | 307.630 ms | 375.373 ms |
| Stop p95 | 347.962 ms | 420.424 ms |
| First caption p50 | 779.750 ms | 800.572 ms |
| VAD/native overlap per capture p50 | 0 ms | 617.785 ms |

This experiment used a native CPU budget of one. It demonstrates overlap, not a speedup. VAD still delays receipt of Stop, and a
late obsolete preview can occupy the native worker before final inference. This
candidate is rejected. The subsequent implementation moves preparation to a
single CPU worker so accepting PCM and recording Stop do not wait for VAD. The original
deadline and final-audio checks remain unchanged. Evidence:
`tmp/stop-pipeline-20261031/paired-asr14-mixed/report.json`.

An isolated VAD thread-count experiment also retained the existing default:
ABBA instance medians for one/two/two/one threads were
85.013/123.721/72.647/85.489 ms. The pooled medians were 85.251 versus 87.537 ms;
all twenty probability hashes and trimming endpoints matched exactly. There is
no stable evidence to promote two VAD threads. No thread default was changed.

## Local trial installer

The completed installer is
`src-tauri/target/release/bundle/nsis/轻语 Whisper_1.7.11_local-pipeline-20261031_x64-setup.exe`
(666,260,532 bytes, approximately 635.4 MiB). Its SHA-256 is:

```text
fda01facf466698cf35cec42e49d51d387a018348df49ce772fc7906abca71cb
```

A sibling `.sha256` file is included. The packaged engine archive is
654,586,688 bytes, SHA-256
`586a033043ae65f27d8c5a7f529a42b424c5790e72fd371b28e2b0d8d0cfe3e0`.
The previous dated asr14 installer is preserved with its original checksum.

Primary-workspace verification passes all 326 Python resource tests and 53
packaging tests, Python compilation and diff checks, engine archive validation,
frontend type checking/production build, and the Tauri release/NSIS build.
Comparison against 850 previously frozen source files finds only the six
approved existing-file changes; frontend, Rust and proof sources are unchanged,
so their prior unit/formal validation remains applicable. New test files and this
report are recorded separately in the promotion receipt.

The actual NSIS installer was extracted without installation. Its engine archive
matches the newly built archive. All fourteen bundled Python source files, data,
licenses and native manifest/dependency tables match the primary workspace. The
four changed modules inside the executable's PYZ archive match freshly compiled
source, normalizing only recursive `co_filename` values. The GUI matches the
release executable except for Tauri's documented three-byte `UNK` to `NSS` stamp.

The extracted engine passes real CUDA smoke runs for both `qwen3-asr-0.6b` and
`confucius4-r2t2`: three 81,600-sample sessions each, processing on/off/on after
actual idle unloading and resumption, monotonic committed text and exact accepted
sample counts. Qwen completes 401 protocol requests and R2T2 400; both child
processes exit with code zero. These unpaced checks establish package behavior,
not microphone latency or recognition accuracy.

RE-USE support and licenses are included. Model weights continue to use the
existing cache/download mechanism; they are not embedded into the installer.
CPU/CUDA binaries and architecture targets remain the existing asr14 build.
No installation, commit, remote push or release was performed.

Receipts and logs:
`tmp/stop-pipeline-20261031/final-checks/{source-promotion,packaged-identity,packaged-qwen,packaged-r2t2}.json`.
