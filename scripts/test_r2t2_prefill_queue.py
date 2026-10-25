"""Check deferred multi-graph prefill, original host storage, and exact CPU traces.

The caller identifies a reviewed frozen runtime by raw SHA-256. No old revision
is silently substituted. Only test translation units are built; no GPU or model
is used. Compilation and CPU runs are sequential at below-normal priority.
Large-mask cap checks explicitly skip arithmetic while retaining real graph
construction, ordered uploads, original-pointer lifetime, and byte accounting.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

import test_r2t2_prefill as native


HERE = Path(__file__).resolve().parent


def main():
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--baseline", required=True, type=Path)
    parser.add_argument("--baseline-sha256", required=True)
    parser.add_argument("--build-dir", type=Path,
                        default=native.ROOT / "tmp/asr-opt-20261025/prefill-queue-tests")
    args, remaining = parser.parse_known_args()
    baseline, build = args.baseline.resolve(), args.build_dir.resolve()
    if native.sha256(baseline) != args.baseline_sha256.lower():
        parser.error("baseline does not match the explicitly approved raw SHA-256")
    build.mkdir(parents=True, exist_ok=True)
    inherited_test = native.TEST
    cache_source = args.source.resolve() / "src/framework/runtime/kv_cache.cpp"
    native.TEST = HERE / "native_tests/r2t2_prefill_queue_test.cpp"
    native.INTERPOSE = HERE / "native_tests/r2t2_prefill_queue_interpose.h"
    native.BASELINE_LF_SHA256 = hashlib.sha256(baseline.read_bytes().replace(b"\r\n", b"\n")).hexdigest()
    extra_inputs = {str(path): native.sha256(path) for path in
                    (inherited_test, cache_source, Path(__file__).resolve())}
    provenance = build / "extra-inputs.json"
    if "--no-build" in remaining and json.loads(provenance.read_text()) != extra_inputs:
        parser.error("inherited suite, cache source, or wrapper changed; rerun without --no-build")

    real_run = subprocess.run
    cache_object = build / "prefill_queue_cache.obj"

    def run_with_cache(command, *positional, **kwargs):
        if os.name == "nt":
            kwargs["creationflags"] = kwargs.get("creationflags", 0) | subprocess.BELOW_NORMAL_PRIORITY_CLASS
        if isinstance(command, str):
            return real_run(command, *positional, **kwargs)
        # Preserve the shared runner's command list so its log/manifest records
        # the effective linker command, including the current cache owner.
        command = command if isinstance(command, list) else list(command)
        if "/c" in command and str(command[-1]) == str(native.TEST):
            cache_command = [f"/Fo{cache_object}" if str(arg).startswith("/Fo") else arg for arg in command]
            cache_command[-1] = str(cache_source)
            compiled = real_run(cache_command, *positional, **kwargs)
            (build / "cache-build.log").write_text(
                subprocess.list2cmdline(list(map(str, cache_command))) + "\n" +
                compiled.stdout + compiled.stderr, encoding="utf-8")
            if compiled.returncode:
                return compiled
        if any(str(arg).startswith("/OUT:") for arg in command):
            first_library = next(i for i, arg in enumerate(command) if Path(arg).name == "engine_runtime.lib")
            command.insert(first_library, str(cache_object))
        return real_run(command, *positional, **kwargs)

    sys.argv = [sys.argv[0], "--source", str(args.source), "--baseline", str(baseline),
                "--build-dir", str(build), *remaining]
    native.subprocess.run = run_with_cache
    try:
        result = native.main()
        provenance.write_text(json.dumps(extra_inputs, indent=2) + "\n", encoding="utf-8")
        extra_results = {}
        for name, mode in (("candidate", "controls"), ("baseline", "cpu"),
                           ("candidate", "cpu"), ("baseline", "failure-control"),
                           ("candidate", "failure"), ("candidate", "cap"),
                           ("candidate", "oversized"), ("baseline", "work")):
            label = f"{name}-{mode}"
            run = run_with_cache([build / f"r2t2_prefill_{name}.exe", mode, build / f"{label}.bin"],
                                 capture_output=True, text=True, errors="replace")
            (build / f"{label}.log").write_text(run.stdout + run.stderr, encoding="utf-8")
            expected = 1 if label == "baseline-work" else 0
            valid = run.returncode == expected
            if label == "baseline-work":
                valid = valid and "QUALITY_OK " in run.stdout and "async_submissions=0 " in run.stdout
                valid = valid and "prefill still synchronizes each block" in run.stderr
                if (build / "baseline.bin").is_file():
                    valid = valid and (build / f"{label}.bin").read_bytes() == (build / "baseline.bin").read_bytes()
            extra_results[label] = {"returncode": run.returncode, "expected_outcome": valid}
            print(f"{label}: " + (run.stdout.splitlines()[-1] if run.stdout else run.stderr.strip()))
            if not valid:
                print(run.stderr, end="")
                result = 1
        equal_cpu = (build / "baseline-cpu.bin").read_bytes() == (build / "candidate-cpu.bin").read_bytes()
        extra_results["cpu_exact_trace_equal"] = equal_cpu
        result = result or int(not equal_cpu)
        (build / "queue-controls.json").write_text(json.dumps(extra_results, indent=2) + "\n", encoding="utf-8")
        print("PASS CPU fallback exact trace" if equal_cpu else "FAIL CPU fallback trace changed")
        return result
    finally:
        native.subprocess.run = real_run


if __name__ == "__main__":
    raise SystemExit(main())
