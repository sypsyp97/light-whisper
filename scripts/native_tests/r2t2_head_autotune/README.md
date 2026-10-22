# Real CUDA dispatcher calibration regression

Only test files. Existing `ggml_mul_mat_set_lowering(..., static_cast<ggml_mul_mat_lowering>(4))`
reaches the real CUDA dispatcher. The runner recompiles `ggml-cuda.cu` with a
force-include observer and links all other operators from the existing build.
No production declarations, cache fields, or controller are replaced.

`autotune_test.cu` builds real backend tensors and graphs. Its scalar oracle is
one ordinary, default-lowering `ggml_mul_mat` graph per column on the same backend.
All output elements compare bitwise. A separate stock run directly calls the
unmodified four-column MMVF kernel and must pass before semantic RED is accepted.
The existing broader operator characterization remains an independent gate.

The typed MMVF observer calls the real kernel. The event wrapper first calls
real CUDA, classifies the timed region from actual kernel calls, then supplies
controlled elapsed values only for decision tests. It never invents CUDA success.
One separate subprocess injects a CUDA API error to verify the existing fatal
route. First-byte and last-byte parity faults each flip exactly one byte after
the first actual grouped kernel, using a stream-ordered CUDA operation.

Covered: 95/100 ms acceptance, 96/100 rejection; zero, negative, NaN, infinity
in either timing; parity rejection/restoration; immutable matching-key reuse
with different inputs/tensor allocations; independently changed K/vocabulary;
two live same-device contexts; destruction/recreation; F32 weights, F32 precision,
padded weights/inputs, actual column counts 1/2/3/5; capture miss and grouped/serial
capture hits. Ineligible/capture misses must leave the slot available to tune.
No fixed number of calibration iterations is asserted.

The matching-shape cache test keeps its original calibrated fixture alive while
a second fixture runs, asserts distinct host tensor and device data addresses,
then revisits the original allocation. Four unmarked/default-lowering requests
(normal and raw capture, before and after marked calibration) record every output
bit and stock dispatch. The following marked request must still calibrate, proving
the default requests did not occupy the slot. Candidate runs require the frozen
baseline `default-control.json` and compare all default bits/dispatch exactly.

Automatic ggml CUDA graph replay is disabled **only in the test child process**,
so repeated calls actually reach the dispatcher. Raw CUDA capture uses the public
`ggml_backend_graph_compute_async` entry (the synchronous entry always synchronizes),
then the fixture ends/launches/synchronizes outside the observed capture interval.
Capture guards observe allocation/event/synchronization APIs in the controller's
translation unit. They do not count all process-wide host heap allocations or
allocations hidden inside separately compiled vendor/operator code. Internal ggml
graph replay integration is a separate runtime test.

All native/model/audio inputs are explicit CLI paths. Run from the repository
root with Python 3.11 or later; Qwen collection also requires NumPy. Native
compilation requires Windows MSVC and the CUDA SDK matching the existing static
libraries. Set these process-local variables in the PowerShell session used for
the commands below; the native runner requires both before compilation or
execution. Run only one compile/GPU job at a time, with exclusive use of the GPU
during execution.

```powershell
$env:OMP_NUM_THREADS = '1'
$env:CARGO_BUILD_JOBS = '1'
```

The frozen `asr11` control is the patched native baseline from historical
light-whisper commit `b7bd10fc5ccfe22b6c0da138e1618818ef9bc583`, on audio.cpp
upstream revision `6c70f32d0d90a29a9863e556bd9712ce622f868b`. It predates the exact
head controller. Preserve that source/build separately from the candidate;
an unpatched upstream checkout is not the frozen control.

First collect without compilation or GPU execution:

```powershell
python scripts/native_tests/r2t2_head_autotune/run.py --source <candidate-source> --libraries <candidate-cuda-build> --cuda-root <sdk-root> --build-helper scripts/build_r2t2_runtime.py --build-dir <output-dir>/candidate --collect
```

Capture the default-lowering control from the separate frozen `asr11` source
and static build. Only `--baseline` accepts the intended missing-controller
semantic RED, after requiring stock numerical GREEN. Setup, compilation, and
fixture errors are failures in both modes.

```powershell
python scripts/native_tests/r2t2_head_autotune/run.py --source <frozen-asr11-source> --libraries <frozen-asr11-cuda-build> --cuda-root <sdk-root> --build-helper scripts/build_r2t2_runtime.py --build-dir <output-dir>/baseline --baseline
```

Then run the candidate in a distinct output directory. Its four default-lowering
observations (528 output bit patterns plus dispatch) must equal the frozen file:

```powershell
python scripts/native_tests/r2t2_head_autotune/run.py --source <candidate-source> --libraries <candidate-cuda-build> --cuda-root <sdk-root> --build-helper scripts/build_r2t2_runtime.py --build-dir <output-dir>/candidate --baseline-control <output-dir>/baseline/default-control.json
```

`--compile-only` compiles/links without GPU execution. Compilation/setup failures
are fixture failures, never semantic RED. `summary.json` freezes source/test/library
hashes and exact commands; `observations.json` records actual context/lifetime,
dimensions/capture, MMVF columns, event samples, injected corruption and bit errors.
Cache internals and complete comparison count are not observed; decisions are
inferred from dispatch/event/output behavior. Formal implementation correspondence
must retain that boundary and map source anchors before claiming correspondence.

The independent [Qwen compatibility driver](../../test_qwen_native_compatibility.py)
uses the actual product `QwenNativeRuntime` and `QwenPreviewRuntime`: 12 real
decodes and 19 events in one resident session, with one native thread. Supply
public mono 16 kHz PCM16 Chinese and English WAVs; both must fit the 12-second
preview window. Capture and check in separate fresh processes with the same
model, audio, product wrappers, driver, and dependency DLL contents:

```powershell
python scripts/test_qwen_native_compatibility.py --zh-wav <public-zh.wav> --en-wav <public-en.wav> --list-cases
python scripts/test_qwen_native_compatibility.py --model <qwen.gguf> --library <baseline.dll> --dependencies <dll-dir> --zh-wav <public-zh.wav> --en-wav <public-en.wav> --output <output-dir>/qwen-baseline.json
python scripts/test_qwen_native_compatibility.py --model <qwen.gguf> --library <candidate.dll> --dependencies <dll-dir> --zh-wav <public-zh.wav> --en-wav <public-en.wav> --reference <output-dir>/qwen-baseline.json --output <output-dir>/qwen-candidate.json
```

Qwen collection reads only the public WAVs; it loads no model or DLL. Outputs
must use fresh paths. Equality covers observed text, language, PCM, preview,
reset/restart, and cleanup traces; these synthetic/native regression drivers
make no performance or general ASR quality claim.
