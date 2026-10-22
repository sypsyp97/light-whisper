# Exact four-column head verification

`ExactHeadSelection` checks the selection controller for the marked CUDA
verification head. Its frozen model and twelve configurations are registered in
`formal/verification.json`, so the existing formal CI runs three safety checks,
six fault controls and three reachability witnesses. Their hashes and historical
results are recorded in `formal/evidence/exact-head-selection.json`. All twelve
registered configurations passed after promotion, including the expected nine
invariant violations and positive action-coverage checks.

## Contract and source boundary

The graph owner fixes verification to four columns and preserves the original
per-row transformer, normalization and optional cast operations. It marks the
combined head only for the supported CUDA, F16-weight, default-precision,
contiguous matrix and no-bias path. In the canonical
`scripts/patches/audio-cpp-r2t2-windows-streaming.patch`, the relevant symbols are
`group_head`, `GGML_MUL_MAT_LOWERING_CUDA_EXACT_F16_HEAD`,
`ggml_backend_cuda_context::mul_mat_exact_head` and `exact_head_plan`.

Each backend context owns one immutable decision keyed by actual K and vocabulary
dimensions. A matching eligible request uses the stored choice. An ineligible
request, different key or cold capture uses scalar execution without occupying
the slot. The fallback preserves the actual column count and strides; unsupported
N is outside the model's four-column bound.

Cold calibration occurs outside capture. It compares every output bit of scalar
and grouped execution on the first input, then accepts grouped execution only
with finite, strictly positive event samples and `100 * grouped <= 95 * serial`.
Parity rejection skips native timing. The selected path always executes after
calibration, including rejection, so a failed candidate cannot leave its output
in the destination. CUDA API failures follow the existing fatal error route.
Context destruction discards the slot; a new lifetime can calibrate again.

The model explores two contexts and two abstract K/vocabulary keys, with two
requests per context in the policy runs and three across at most two lifetimes
in the lifecycle run. Same-device and separate-device configurations check
identity isolation. This does not establish actual multi-GPU portability.
The modeled calibration step conservatively summarizes protected effects;
its unused mismatch timing values are not observed CUDA timings.

The concurrency assumption follows the real application: `BaseASRServer.run`
reads and executes commands synchronously, and GPU-idle unloading uses the same
lock as idle-enabled command execution. Stream calls proceed through
`ASRStreamServerMixin`, `R2T2StreamSession`, `SegmentedR2T2` and `NativeRuntime`
to synchronous C ABI calls. Arbitrary concurrent external C ABI callers are
outside this contract. Initial parity is a sanity check, not an all-input
floating-point guarantee; the model does not prove compiler or driver behavior.

## Reproduce the formal checks

Inventory validation requires no native build or GPU:

```powershell
python scripts/check_formal.py --inventory-only
```

The ordinary complete runner takes `--tlc-jar`, `--lean` and `--logs`; it has no
`--model` option. For a targeted run, use its existing functions from the
repository root. Set `TLC_JAR` to the pinned jar and `FORMAL_LOGS` to a fresh output
directory. This performs the actual twelve TLC runs and checks action coverage:

```powershell
@'
import hashlib, importlib.util, json, os
from pathlib import Path
spec = importlib.util.spec_from_file_location("check_formal", "scripts/check_formal.py")
check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check)
manifest = json.loads(Path("formal/verification.json").read_text(encoding="utf-8"))
jar = Path(os.environ["TLC_JAR"]).resolve()
logs = Path(os.environ["FORMAL_LOGS"]).resolve()
assert hashlib.sha256(jar.read_bytes()).hexdigest() == check.TLC_SHA256
check.check_inventory(manifest)
model = "ExactHeadSelection"
source = Path("formal", model + ".tla").resolve()
for config in manifest["runs"][model]:
    check.tlc(jar, model, Path("formal", config).resolve(), source,
              logs / Path(config).stem)
check.check_model_bindings(manifest, model, logs)
for case in manifest["negative"]:
    if case["model"] == model:
        config = Path("formal", case["config"]).resolve()
        check.tlc(jar, model, config, source, logs / config.stem, case["invariant"])
'@ | python -
```

The three positive runs must complete without errors. Each other run must exit
12 with its registered invariant violation. The three `*Witness.cfg` entries
negate desirable reachability states; their counterexamples establish reachable
request orders, not safety failures. The recorded historical run used a 512 MiB
heap and one worker; the existing repository runner uses two workers.

## Native observations and correspondence

The reproducible dispatcher commands and prerequisites are in
[`scripts/native_tests/r2t2_head_autotune/README.md`](../scripts/native_tests/r2t2_head_autotune/README.md).
The fixture reaches the real dispatcher through the public lowering setter and
real backend graph computation. Its observer calls actual MMVF kernels and CUDA
events; controlled times and first/last-byte faults test decisions after real
execution. It records context plus lifetime, shape, capture, launches, event
samples and final output bits. It does not expose private cache fields or the
internal comparison count.

The historical source-backed checker bound nine hashed TLC state traces to ten
actual candidate sequences. A fault trace supplies the requests that trigger
the modeled bug; the native candidate must exhibit the correct response. C++
mutants are not executed.

| TLC configuration suffix | Native request sequence |
| --- | --- |
| WrongKeyNegative | `immutable_slot/first` then `different_k/fallback`; separately `different_vocab/fallback` |
| CrossContextNegative | `immutable_slot/first` then `independent_live_backend/first` |
| TuneInCaptureNegative | `raw_capture_miss/fallback` |
| IgnoredParityNegative | `parity_reject_first/first` |
| MutablePlanNegative | `immutable_slot/first` then `immutable_slot/cache_hit` |
| SkipSelectedNegative | `immutable_slot/first`, including final selected execution |
| GroupedCaptureWitness | `capture_miss_did_not_fill_slot/first` then `grouped_capture/capture_hit` |
| MismatchRestoreWitness | `parity_reject_last/first` |
| FreshContextWitness | `boundary_accept/first` then `boundary_reject/first` in a new backend lifetime |

Key abstraction preserves equality and distinctness, not numerical dimensions.
Context identity includes lifetime because allocator addresses can recur.
Accepted times preserve the threshold category. Intervening cache hits,
different-key fallbacks and the prefix capture miss can be omitted only where
reviewed no-write source branches and actual no-timing/output observations show
they preserve the projected state. The fresh-context witness ends when the
second calibration starts; that calibration may subsequently choose Serial.

The historical result was 59 dispatcher observations with zero work/numerical
failures, 528 unchanged default-lowering output words and the expected fatal API
error. Independent graph tests passed 258 CPU and 2524 CUDA assertions and exact
binary traces, including automatic CUDA graph execution. Reproduce that graph
test with `scripts/test_r2t2_grouped_head.py --source <candidate-source>
--libraries <candidate-cuda-build> --baseline-root <frozen-asr11-source>
--cuda-root <sdk-root> --build-dir <fresh-output> --backend all`.

The raw historical checker and traces remain local investigation artifacts;
their fingerprints are retained in the compact evidence. Formal CI verifies
the model and registry, not those local traces. A fresh native run must retain
its complete source, helper, fixture and library fingerprints. Fresh build-12
dispatcher and graph runs passed, and the frozen checker again accepted all ten
mappings with zero errors. All 33 source anchors, 119 dispatcher inputs and 847
graph inputs were rehashed; the four promoted dispatcher code files are byte
identical to the original paths used by that run. Four deliberate evidence
corruptions were rejected again. New build-12 bindings and reports are separate
from the retained build-11 records.

The recorded `build_helper_sha256` identifies the helper used for those native
runs. Later Windows environment handling and byte-preserving source checkout change that helper,
without changing the pinned native patch or fixtures. The historical helper hash
is retained; release builds and their manifests provide separate build evidence.

The correspondence still uses the original hashed one-worker TLC state traces.
The promoted two-worker runs verify the same model/configuration bytes but may
find different counterexample orders; they do not replace those projections.

Measured performance remains a separate result: resident full-protocol English
improved 4.81% initially and 4.40% in the fresh replication on the tested setup.
All 36 fresh/combined pooled latency guards passed; the evidence retains the
three initial pooled guard failures. Additional per-round failures occurred.
The main task verified 192 exact traces. This supports
the scoped measured result, not a 10% improvement or a speed guarantee on every
NVIDIA GPU.
