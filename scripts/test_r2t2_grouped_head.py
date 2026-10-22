"""Real CPU/CUDA verification-graph regression, with frozen asr11 work-RED control.

Synthetic weights; same-backend serial oracle; no timing claims. The observer
executes the real backend and retains argmax-input logits for exact readback.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import runpy
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]
TESTS = ROOT / "scripts/native_tests"
OWNER = "src/framework/modules/transformers/qwen_causal_decode_runtime.cpp"
BASELINE_SHA256 = "d17003f9acb450cde3a7d91030048c32e5ac9912e5c858d7762ddbe16f2f0d3a"
LIBRARIES = ("engine_runtime.lib", "ggml/src/ggml.lib", "ggml/src/ggml-cpu.lib",
             "ggml/src/ggml-cuda/ggml-cuda.lib", "ggml/src/ggml-base.lib",
             "external/sentencepiece/src/sentencepiece.lib", "cjson_vendor.lib", "yaml_vendor.lib")


def sha256(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True, type=Path, help="Candidate native source/header tree")
    parser.add_argument("--libraries", required=True, type=Path, help="Existing CUDA static-library build; read-only")
    parser.add_argument("--baseline-root", required=True, type=Path, help="Frozen asr11 tree containing the runtime owner")
    parser.add_argument("--build-dir", type=Path, default=ROOT / "tmp/r2t2-grouped-head-tests")
    parser.add_argument("--cuda-root", required=True, type=Path, help="CUDA SDK root matching the static-library build")
    parser.add_argument("--backend", choices=("cpu", "cuda", "all"), default="all")
    parser.add_argument("--baseline-only", action="store_true", help="record frozen arithmetic GREEN plus intended CUDA graph-work RED")
    parser.add_argument("--collect", action="store_true", help="check test/owner inputs without compile or GPU work")
    args = parser.parse_args()
    source, libraries, baseline, build, sdk = (p.resolve() for p in
        (args.source, args.libraries, args.baseline_root, args.build_dir, args.cuda_root))
    test_files = [TESTS / f"r2t2_grouped_head_{suffix}" for suffix in ("test.cpp", "observer.h", "interpose.h")]
    inputs = [Path(__file__).resolve(), ROOT / "scripts/build_r2t2_runtime.py", *test_files,
              source / OWNER, baseline / OWNER, *sorted((source / "include").rglob("*.h")),
              *sorted((source / "external/ggml/include").glob("*.h"))]
    for path in inputs:
        if not path.is_file():
            parser.error(f"required input missing: {path}")
    if sha256(baseline / OWNER) != BASELINE_SHA256:
        parser.error("frozen asr11 runtime owner identity changed")
    backends = ("cpu", "cuda") if args.backend == "all" else (args.backend,)
    if args.collect:
        print(json.dumps({"status": "COLLECTED_NOT_COMPILED", "cases": {"cpu": 1, "cuda": 10},
                          "baseline_expected_cuda_work_red_cases": 3, "reserved_lowering_value": 4,
                          "test_sha256": {p.name: sha256(p) for p in [Path(__file__).resolve(), *test_files]},
                          "baseline_owner_sha256": BASELINE_SHA256,
                          "public_api": "existing verify_tokens and ggml_mul_mat_set_lowering"}, indent=2))
        return 0
    if os.environ.get("OMP_NUM_THREADS") != "1" or os.environ.get("CARGO_BUILD_JOBS") != "1":
        parser.error("set OMP_NUM_THREADS=1 and CARGO_BUILD_JOBS=1 using your resource limiter before compilation/execution")
    inputs += [libraries / name for name in LIBRARIES]
    inputs += [libraries / "CMakeCache.txt", libraries / "build.ninja"]
    for path in inputs:
        if not path.is_file():
            parser.error(f"build input unavailable; setup pending, not semantic RED: {path}")
    fingerprint = {str(path): sha256(path) for path in inputs}
    env, _ = runpy.run_path(str(ROOT / "scripts/build_r2t2_runtime.py"))["build_environment"]()
    env = {key.upper(): value for key, value in env.items()}
    env["PATH"] = str(sdk / "bin") + os.pathsep + env["PATH"]
    compiler, linker = (shutil.which(name, path=env["PATH"]) for name in ("cl.exe", "link.exe"))
    if not compiler or not linker:
        parser.error("Visual Studio cl.exe/link.exe unavailable")
    build.mkdir(parents=True, exist_ok=True)
    evidence = {"synthetic_only": True, "timing_claim": False, "backends": backends,
                "inputs": fingerprint, "commands": [], "variants": {},
                "baseline_owner_sha256": BASELINE_SHA256, "reserved_lowering_value": 4,
                "source_revision": subprocess.check_output(["git", "-C", str(source), "rev-parse", "HEAD"]).decode().strip(),
                "source_diff_sha256": hashlib.sha256(subprocess.check_output(["git", "-C", str(source), "diff", "--binary", "HEAD"])).hexdigest()}
    common = [compiler, "/nologo", "/c", "/std:c++17", "/EHsc", "/O2", "/Gy", "/MD", "/DNDEBUG",
              "/DNOMINMAX", "/D_CRT_SECURE_NO_WARNINGS", f"/I{source / 'include'}",
              f"/I{source / 'external/ggml/include'}", f"/I{TESTS}"]

    def execute(command, directory, log, expected=0):
        command = list(map(str, command))
        result = subprocess.run(command, cwd=directory, env=env, capture_output=True, text=True, errors="replace")
        evidence["commands"].append({"command": command, "returncode": result.returncode, "expected": expected})
        (directory / log).write_text(subprocess.list2cmdline(command) + "\n" + result.stdout + result.stderr, encoding="utf-8")
        if result.returncode != expected:
            print(result.stdout + result.stderr, end="")
            raise RuntimeError(f"command returned {result.returncode}, expected {expected}; see {directory / log}")
        if expected == 2 and ("NUMERICAL_GREEN" not in result.stdout or "WORK_RED" not in result.stderr):
            raise RuntimeError("failure was not numerical GREEN plus intended graph-work RED")
        return result

    try:
        variants = [("baseline", baseline)]
        if not args.baseline_only:
            variants.append(("candidate", source))
        for variant, owners in variants:
            directory = build / variant
            directory.mkdir(exist_ok=True)
            results = evidence["variants"][variant] = {}
            execute([*common, f"/FI{test_files[2]}", f"/Fo{directory / 'runtime.obj'}", owners / OWNER], directory, "compile-runtime.log")
            execute([*common, f"/Fo{directory / 'test.obj'}", test_files[0]], directory, "compile-test.log")
            executable = directory / "r2t2_grouped_head_test.exe"
            execute([linker, "/nologo", "/OPT:REF", f"/OUT:{executable}", directory / "test.obj", directory / "runtime.obj",
                     *[libraries / name for name in LIBRARIES], f"/LIBPATH:{sdk / 'lib/x64'}",
                     "cudart.lib", "cublas.lib", "cublasLt.lib", "cuda.lib", "cufft.lib", "cudadevrt.lib",
                     "cudart_static.lib", "kernel32.lib", "advapi32.lib", "ws2_32.lib"], directory, "link.log")
            for backend in backends:
                trace = directory / f"{backend}.bin"
                numerical = execute([executable, backend, "equivalence", trace], directory, f"{backend}-equivalence.log")
                work_trace = directory / f"{backend}-work.bin"
                expected = 2 if variant == "baseline" and backend == "cuda" else 0
                work = execute([executable, backend, "work", work_trace], directory, f"{backend}-work.log", expected)
                summary_fields = dict(part.split("=", 1) for part in work.stdout.splitlines()[-1].split() if "=" in part)
                expected_cases = 1 if backend == "cpu" else 10
                expected_red = 3 if expected == 2 else 0
                if int(summary_fields.get("cases", -1)) != expected_cases or int(summary_fields.get("work_red", -1)) != expected_red:
                    raise RuntimeError(f"{variant}/{backend}: incomplete cases or unexpected graph-work failure count")
                if trace.read_bytes() != work_trace.read_bytes():
                    raise RuntimeError(f"{variant}/{backend}: work mode changed numerical/cache trace")
                if variant == "candidate" and trace.read_bytes() != (build / "baseline" / f"{backend}.bin").read_bytes():
                    raise RuntimeError(f"candidate/{backend}: trace differs from frozen same-backend baseline")
                results[backend] = {"numerical": numerical.stdout.splitlines()[-1],
                                    "work_returncode": work.returncode, "work": work.stderr.strip() or work.stdout.splitlines()[-1],
                                    "trace_sha256": sha256(trace), "trace_bytes": trace.stat().st_size,
                                    "executable_sha256": sha256(executable), "baseline_trace_equal": variant == "candidate"}
                print(f"{variant}/{backend}: {results[backend]['numerical']}; {results[backend]['work']}")
        evidence["returncode"] = 0
        evidence["status"] = "FROZEN_CONTROL_VERIFIED" if args.baseline_only else "PASS"
    except RuntimeError as error:
        evidence.update(returncode=1, status="FAIL", error=str(error))
        print(f"FAIL {error}")
    finally:
        changed = [str(path) for path in inputs if not path.is_file() or sha256(path) != fingerprint[str(path)]]
        evidence["inputs_unchanged"] = not changed
        if changed:
            evidence.update(returncode=1, status="FAIL", changed_inputs=changed)
        (build / "summary.json").write_text(json.dumps(evidence, indent=2) + "\n", encoding="utf-8")
    return evidence["returncode"]


if __name__ == "__main__":
    raise SystemExit(main())
