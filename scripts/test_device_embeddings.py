"""CPU-only device-embedding parity, transfer, prefix, and invalidation checks.

The frozen baseline is the reviewed runtime plus the host-copy API scaffold, not
an older prefill optimization baseline. Real ggml arithmetic and four subsequent
decode steps are compared byte-for-byte. No model or GPU is used.

Pass --baseline either the original frozen runtime or the existing scaffold.
Reconstruct the original in a separate audio.cpp tree at revision
6c70f32d0d90a29a9863e556bd9712ce622f868b by applying this project's native patch
from commit b1ee2e4db5ab319928c3fd532df0dd217946c36a. The original is verified by
raw SHA-256, then the checked-in host-copy scaffold is inserted only in the test
build directory. The source tree is never changed.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys

import test_r2t2_prefill as native


HERE = Path(__file__).resolve().parent
DEFAULT_BUILD = native.ROOT / "tmp/asr-opt-20261025/device-tests"
# Frozen before production implementation; only line-ending normalization allowed.
SCAFFOLD_LF_SHA256 = "3dba1ca0dba17bae23f420695693e82079d13df68aa62b58f2282f35fad4df4f"
BASELINE_RAW_SHA256 = "e11f876d71d4da9971f507c36d8429bc6dd5b70f0b4bc52625c4c559c2048081"
SCAFFOLD = HERE / "native_tests/device_embeddings_baseline_scaffold.cpp"


def prepare_baseline(baseline, build, no_build=False):
    if not baseline.is_file():
        raise ValueError(f"baseline runtime missing: {baseline}; pass --baseline with the frozen "
                         "original runtime reconstructed as described in this runner's docstring")
    original = baseline.read_bytes()
    if hashlib.sha256(original.replace(b"\r\n", b"\n")).hexdigest() == SCAFFOLD_LF_SHA256:
        return baseline
    if hashlib.sha256(original).hexdigest() != BASELINE_RAW_SHA256:
        raise ValueError("baseline runtime differs from both the frozen original and host-copy scaffold")
    scaffold = SCAFFOLD.read_bytes().replace(b"\r\n", b"\n")
    anchor = b"void QwenCausalDecodeRuntime::start_decode_tokens(\n"
    generated = original.replace(anchor, scaffold + anchor, 1)
    if original.count(anchor) != 1 or hashlib.sha256(generated).hexdigest() != SCAFFOLD_LF_SHA256:
        raise ValueError("host-copy scaffold does not reproduce the frozen baseline SHA-256")
    output = build / "generated_device_baseline.cpp"
    if output == baseline:
        raise ValueError("original baseline must be outside the generated test output path")
    if not no_build:
        output.write_bytes(generated)
    return output


def main():
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--build-dir", type=Path, default=DEFAULT_BUILD)
    parser.add_argument("--baseline", type=Path, default=DEFAULT_BUILD / "baseline_scaffold.cpp")
    args, remaining = parser.parse_known_args()
    source, build = args.source.resolve(), args.build_dir.resolve()
    build.mkdir(parents=True, exist_ok=True)
    try:
        baseline = prepare_baseline(args.baseline.resolve(), build, "--no-build" in remaining)
    except (OSError, ValueError) as error:
        parser.error(str(error))
    runtime = source / native.RUNTIME
    header = source / "include/engine/framework/modules/transformers/qwen_causal_decode_runtime.h"
    if "prefill_device_embeddings_into_cache" not in header.read_text(encoding="utf-8") or \
       "prefill_device_embeddings_into_cache" not in runtime.read_text(encoding="utf-8"):
        print("INTERFACE_PENDING: device prefill needs a compiled host-copy scaffold before semantic RED")
        return 2

    native.TEST = HERE / "native_tests/device_embeddings_test.cpp"
    native.INTERPOSE = HERE / "native_tests/device_embeddings_interpose.h"
    native.BASELINE_LF_SHA256 = SCAFFOLD_LF_SHA256
    cache_source = source / "src/framework/runtime/kv_cache.cpp"
    extra_paths = (HERE / "native_tests/r2t2_prefill_test.cpp", cache_source, Path(__file__).resolve(),
                   args.baseline.resolve(), SCAFFOLD)
    for path in extra_paths:
        if not path.is_file():
            parser.error(f"required native test input missing: {path}")
    extra_inputs = {str(path): native.sha256(path) for path in extra_paths}
    provenance = build / "extra-inputs.json"
    if "--no-build" in sys.argv and (not provenance.is_file() or
            json.loads(provenance.read_text(encoding="utf-8")) != extra_inputs):
        parser.error("fixture, cache source, or wrapper changed; rerun without --no-build")

    # Link the current cache TU ahead of read-only cached engine libraries, as in
    # test_r2t2_prefill_async.py. Build only these test/runtime/cache translation units.
    cache_object = build / "device_embeddings_cache.obj"
    real_run = subprocess.run

    def run_with_cache(command, *positional, **kwargs):
        if isinstance(command, str):
            return real_run(command, *positional, **kwargs)
        command = list(command)
        if "/c" in command and str(command[-1]) == str(native.TEST):
            cache_command = [f"/Fo{cache_object}" if str(arg).startswith("/Fo") else arg
                             for arg in command]
            cache_command[-1] = str(cache_source)
            compiled = real_run(cache_command, *positional, **kwargs)
            (build / "cache-build.log").write_text(
                subprocess.list2cmdline(list(map(str, cache_command))) + "\n" +
                compiled.stdout + compiled.stderr, encoding="utf-8")
            if compiled.returncode:
                return compiled
        if any(str(arg).startswith("/OUT:") for arg in command):
            first_library = next(i for i, arg in enumerate(command)
                                 if Path(arg).name == "engine_runtime.lib")
            command.insert(first_library, str(cache_object))
        if sys.platform == "win32":
            kwargs.setdefault("creationflags", subprocess.BELOW_NORMAL_PRIORITY_CLASS)
        return real_run(command, *positional, **kwargs)

    native.subprocess.run = run_with_cache
    sys.argv = [sys.argv[0], "--source", str(source), "--baseline", str(baseline),
                "--build-dir", str(build), *remaining]
    try:
        result = native.main()
    finally:
        native.subprocess.run = real_run
    provenance.write_text(json.dumps(extra_inputs, indent=2) + "\n", encoding="utf-8")

    # Keep the old host-copy adapter as an executable negative control after GREEN.
    control = real_run([build / "r2t2_prefill_baseline.exe", "work", build / "baseline-work.bin"],
                       capture_output=True, text=True, errors="replace")
    (build / "baseline-work.log").write_text(control.stdout + control.stderr, encoding="utf-8")
    expected_red = (control.returncode == 1 and "QUALITY_OK " in control.stdout and
                    "embedding data crossed the host boundary" in control.stdout and
                    "no backend exact-prefix comparison" in control.stdout and
                    "device embedding contract failed" in control.stderr)
    (build / "baseline-control.json").write_text(json.dumps({
        "baseline_lf_sha256": SCAFFOLD_LF_SHA256,
        "returncode": control.returncode,
        "expected_semantic_red": expected_red,
    }, indent=2) + "\n", encoding="utf-8")
    print("PASS frozen host-copy control: numerical parity retained; transfer/prefix contract rejected"
          if expected_red else "FAIL frozen host-copy control; inspect baseline-work.log")
    return result or (0 if expected_red else 1)


if __name__ == "__main__":
    raise SystemExit(main())
