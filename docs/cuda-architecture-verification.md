# CUDA architecture selection contract

`scripts/build_r2t2_runtime.py::main` first clears cached/ambient architecture
selection and reads the actual upstream CMake report. On the default CUDA path,
`portable_cuda_architectures` retains every original token, duplicate and order,
including future and suffixed targets. It appends `80-real`, then `90-real`, only
when the corresponding exact `80-virtual`/`90-virtual` token exists and neither
its bare numeric token nor its `-real` token exists. Suffixed targets such as
`90a` and `100f` do not count as generic targets.

A changed list requires a second configure with the complete planned list.
`resolved_cuda_architectures` must parse an actual final report equal to that
plan; missing, invalid, conflicting, stale or different reports stop before
compile and runtime copying. The manifest records that actual final result.
An unchanged list needs one configure. Explicit `--cuda-architectures` keeps
existing upstream normalization semantics and receives no automatic additions.
The CPU path receives no architecture selection or extra configure.

## Reproduce

The offline regression reaches the real `main()` entry, runtime copying and
manifest creation; CMake/toolkit files are deterministic synthetic fixtures:

```powershell
uv run --no-sync python scripts/test_cpu_cuda_backends.py
uv run --no-sync python scripts/check_formal.py --inventory-only
```

`CudaArchitectureSelection` and its configurations are registered in
`formal/verification.json` for the existing complete formal runner. For a
targeted run, set `TLC_JAR` to the pinned jar and `FORMAL_LOGS` to a fresh output
directory, then use the existing runner functions as in
[`exact-head-verification.md`](exact-head-verification.md). This local invocation
limits Java to 1 GiB and one TLC worker without changing the shared runner:

```powershell
$env:JAVA_TOOL_OPTIONS = '-Xms64m -Xmx1024m -XX:ActiveProcessorCount=2'
@'
import hashlib, importlib.util, json, os
from pathlib import Path
spec = importlib.util.spec_from_file_location("check_formal", "scripts/check_formal.py")
check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check)
original_run = check.subprocess.run
def one_worker(command, **kwargs):
    command = list(command)
    command[command.index("-workers") + 1] = "1"
    return original_run(command, **kwargs)
check.subprocess.run = one_worker
manifest = json.loads(Path("formal/verification.json").read_text(encoding="utf-8"))
jar = Path(os.environ["TLC_JAR"]).resolve()
logs = Path(os.environ["FORMAL_LOGS"]).resolve()
assert hashlib.sha256(jar.read_bytes()).hexdigest() == check.TLC_SHA256
check.check_inventory(manifest)
model = "CudaArchitectureSelection"
source = Path("formal", model + ".tla").resolve()
for config in manifest["runs"][model]:
    check.tlc(jar, model, Path("formal", config).resolve(), source, logs / Path(config).stem)
check.check_model_bindings(manifest, model, logs)
for case in manifest["negative"]:
    if case["model"] == model:
        config = Path("formal", case["config"]).resolve()
        check.tlc(jar, model, config, source, logs / config.stem, case["invariant"])
'@ | uv run --no-sync python -
```

The positive configuration must finish without errors and exercise all six
registered actions. Each negative must exit 12 with its registered violation:

| Fault control | Required violation | Implementation regression |
| --- | --- | --- |
| Omit native additions | `SelectedGenericHasNative` | default missing-target cases and toolkit manifest lists |
| Drop upstream PTX | `UpstreamPreserved` | ordered full-list configure and manifest assertions |
| Accept a stale second report | `FinalMismatchBlocksBuild` | stale/different report rejection before compile or publish |

The model explores eleven representative tokens and all sequences of length
one through three, including duplicates, bare/native/virtual generic 80/90,
suffixed and future targets. It covers default, explicit and CPU paths, matching,
stale, changed and invalid final reports, and manifest provenance. Configure
failure and report-parser syntax have implementation regressions. The model
does not parse Python, inspect cubins, prove CMake/compiler/driver behavior,
establish compatibility on every toolkit/GPU, or prove performance. Native
CPU/CUDA rebuilds and binary/hardware observations provide separate evidence.
