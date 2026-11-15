




import argparse
import json
from pathlib import Path
import subprocess
import sys


HERE = Path(__file__).resolve().parent
import test_r2t2_prefill as native


def main():
    if not any(arg == "--build-dir" or arg.startswith("--build-dir=") for arg in sys.argv[1:]):
        sys.argv.extend(["--build-dir", str(native.ROOT / "tmp/r2t2-prefill-async-tests")])
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--build-dir", type=Path)
    parser.add_argument("--mode", default="all")
    args, _ = parser.parse_known_args()
    build = args.build_dir.resolve()
    build.mkdir(parents=True, exist_ok=True)
    inherited_test = native.TEST
    cache_source = args.source.resolve() / "src/framework/runtime/kv_cache.cpp"
    native.TEST = HERE / "native_tests/r2t2_prefill_async_test.cpp"
    native.INTERPOSE = HERE / "native_tests/r2t2_prefill_async_interpose.h"


    native.BASELINE_LF_SHA256 = "e846316ce0953f9311479bb8083fea09d1917ea1541b0f05ddd3cdc912e54bd0"
    extra_inputs = {str(path): native.sha256(path) for path in
                    (inherited_test, cache_source, Path(__file__).resolve())}
    provenance = build / "extra-inputs.json"
    if "--no-build" in sys.argv and json.loads(provenance.read_text()) != extra_inputs:
        parser.error("inherited suite, cache source, or wrapper changed; rerun without --no-build")



    real_run = subprocess.run
    cache_object = build / "prefill_async_cache.obj"

    def run_with_cache(command, *positional, **kwargs):
        if isinstance(command, str):
            return real_run(command, *positional, **kwargs)
        command = list(command)
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



    native.subprocess.run = run_with_cache
    try:
        result = native.main()
    finally:
        native.subprocess.run = real_run
    if result:
        return result
    provenance.write_text(json.dumps(extra_inputs, indent=2) + "\n", encoding="utf-8")
    if args.mode == "equivalence":
        return 0
    control = subprocess.run([build / "r2t2_prefill_baseline.exe", "work", build / "baseline-work.bin"],
                             capture_output=True, text=True, errors="replace")
    (build / "baseline-work.log").write_text(control.stdout + control.stderr, encoding="utf-8")
    control_ok = (control.returncode == 1 and "QUALITY_OK " in control.stdout
                  and "queued_batches=0 queued_uploads=0" in control.stdout
                  and "prefill inputs did not queue all four uploads until compute" in control.stderr)
    reference_trace = build / ("work.bin" if args.mode == "work" else "baseline.bin")
    trace_equal = (build / "baseline-work.bin").read_bytes() == reference_trace.read_bytes()
    control_ok = control_ok and trace_equal
    (build / "baseline-queue-control.json").write_text(json.dumps({
        "returncode": control.returncode, "expected_semantic_red": control_ok,
        "trace_equal_to_baseline_equivalence": trace_equal,
    }, indent=2) + "\n", encoding="utf-8")
    print("PASS baseline queue-work control: numerical trace passes; zero queued uploads rejected"
          if control_ok else "FAIL baseline queue-work control; inspect baseline-work.log")
    return 0 if control_ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
