"""CPU exact-output and graph-work regression for temporal Qwen verification.

Compile the two real owning translation units against existing CPU static
libraries. Frozen asr10 owners use a serial-greedy test adapter, which must pass
the identical numerical suite before failing the temporal graph-work checks.
Candidate output/cache traces must match that frozen baseline byte-for-byte.
Synthetic weights establish correctness/work only; no speech-quality or speed
claims are made. All build inputs, commands, outcomes and traces are recorded.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess

from build_r2t2_runtime import ROOT, build_environment


TESTS = ROOT / "scripts/native_tests"
RUNTIME = "src/framework/modules/transformers/qwen_causal_decode_runtime.cpp"
GREEDY = "src/framework/runtime/greedy_qwen_decoder.cpp"

BASELINE_LF_SHA256 = {
    RUNTIME: "d3b6749bb96cbee62718448a676bda86e041af15aae1cdfb446f2986f84f3ac9",
    GREEDY: "940fc637fefc152e0d5439f5a7af9a8b45684e966ffd2828964838048577f9aa",
}
LIBRARIES = ("engine_runtime.lib", "ggml/src/ggml.lib", "ggml/src/ggml-cpu.lib",
             "ggml/src/ggml-base.lib", "external/sentencepiece/src/sentencepiece.lib",
             "cjson_vendor.lib", "yaml_vendor.lib")


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True, type=Path, help="Candidate native source/header tree")
    parser.add_argument("--libraries", required=True, type=Path, help="Existing CPU static-library build; read-only")
    parser.add_argument("--baseline-root", required=True, type=Path, help="Frozen asr10 tree containing both owning .cpp files")
    parser.add_argument("--build-dir", type=Path, default=ROOT / "tmp/r2t2-temporal-verify-tests")
    parser.add_argument("--cache-types", choices=("f16", "bf16"), default="bf16",
                        help="bf16 includes F32/F16/BF16; f16 includes F32/F16")
    args = parser.parse_args()
    source, libraries, baseline, build = (path.resolve() for path in
                                         (args.source, args.libraries, args.baseline_root, args.build_dir))
    test_files = {name: TESTS / f"r2t2_temporal_{name}" for name in
                  ("verify_test.cpp", "integration_test.cpp", "observer.h", "interpose.h")}
    paths = [Path(__file__).resolve(), ROOT / "scripts/build_r2t2_runtime.py",
             TESTS / "qwen_shared_decoder_test.cpp", *test_files.values(),
             source / RUNTIME, source / GREEDY, baseline / RUNTIME, baseline / GREEDY,
             *[libraries / name for name in LIBRARIES],
             *sorted((source / "include").rglob("*.h")),
             *sorted((source / "external/ggml/include").glob("*.h"))]
    for path in paths:
        if not path.is_file():
            parser.error(f"required native test input missing: {path}")
    for owner, expected in BASELINE_LF_SHA256.items():
        actual = hashlib.sha256((baseline / owner).read_bytes().replace(b"\r\n", b"\n")).hexdigest()
        if actual != expected:
            parser.error(f"baseline owner differs from frozen asr10: {owner}; expected {expected}, got {actual}")
    fingerprint = {str(path): sha256(path) for path in paths}
    env, _ = build_environment()
    env = {key.upper(): value for key, value in env.items()}
    compiler, linker = (shutil.which(name, path=env["PATH"]) for name in ("cl.exe", "link.exe"))
    if not compiler or not linker:
        parser.error("Visual Studio cl.exe/link.exe unavailable after vcvars")
    build.mkdir(parents=True, exist_ok=True)
    evidence = {"backend": "CPU", "threads": 1, "synthetic_only": True, "timing_claim": False,
                "cache_types": args.cache_types, "baseline_lf_sha256": BASELINE_LF_SHA256,
                "inputs": fingerprint, "commands": [], "variants": {}}
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
            raise RuntimeError(f"native test result {result.returncode}, expected {expected}; see {directory / log}")
        if expected == 2 and ("NUMERICAL_GREEN" not in result.stdout or "WORK_RED" not in result.stderr):
            raise RuntimeError("baseline failure is not numerical GREEN plus the intended semantic work RED")
        return result

    try:
        for variant, owners in (("baseline", baseline), ("candidate", source)):
            directory = build / variant
            directory.mkdir(exist_ok=True)
            results = evidence["variants"][variant] = {}
            for owner, object_name in ((RUNTIME, "runtime.obj"), (GREEDY, "greedy.obj")):
                execute([*common, f"/FI{test_files['interpose.h']}", f"/Fo{directory / object_name}", owners / owner],
                        directory, f"compile-{object_name}.log")
            for name in ("verify", "integration"):
                execute([*common, f"/DTEMPORAL_NATIVE={int(variant == 'candidate')}", f"/Fo{directory / (name + '.obj')}",
                         test_files[name + "_test.cpp"]], directory, f"compile-{name}.log")
                executable = directory / f"r2t2_temporal_{name}.exe"
                objects = [directory / f"{name}.obj", directory / "runtime.obj"]
                if name == "integration":
                    objects.append(directory / "greedy.obj")
                execute([linker, "/nologo", "/OPT:REF", f"/OUT:{executable}", *objects,
                         *[libraries / library for library in LIBRARIES], "kernel32.lib", "advapi32.lib"], directory, f"link-{name}.log")
                suffix = [args.cache_types] if name == "verify" else []
                trace, work_trace = directory / f"{name}.bin", directory / f"{name}-work.bin"
                numerical = execute([executable, "equivalence", trace, *suffix], directory, f"run-{name}-equivalence.log")
                print(f"{variant}: {numerical.stdout.splitlines()[-1]}")
                work = execute([executable, "work", work_trace, *suffix], directory, f"run-{name}-work.log",
                               expected=2 if variant == "baseline" else 0)
                if trace.read_bytes() != work_trace.read_bytes():
                    raise RuntimeError(f"{variant}/{name}: work mode changed the exact arithmetic trace")
                if variant == "candidate" and trace.read_bytes() != (build / "baseline" / f"{name}.bin").read_bytes():
                    raise RuntimeError(f"{name}: candidate trace differs bit-for-bit from frozen asr10")
                results[name] = {"numerical": numerical.stdout.splitlines()[-1], "work_returncode": work.returncode,
                                 "work": work.stderr.strip() or work.stdout.splitlines()[-1],
                                 "trace_bytes": trace.stat().st_size, "trace_sha256": sha256(trace),
                                 "executable_sha256": sha256(executable), "baseline_trace_equal": variant == "candidate"}
                print(f"{variant}: {results[name]['work']}")
        evidence["returncode"] = 0
    except RuntimeError as error:
        evidence["returncode"] = 1
        evidence["error"] = str(error)
        print(f"FAIL {error}")
    finally:
        changed = [str(path) for path in paths if not path.is_file() or sha256(path) != fingerprint[str(path)]]
        evidence["source_inputs_unchanged"] = not changed
        if changed:
            evidence["returncode"] = 1
            evidence["changed_inputs"] = changed
            print(f"FAIL native inputs changed during execution: {changed}")
        (build / "summary.json").write_text(json.dumps(evidence, indent=2) + "\n", encoding="utf-8")
    if not evidence["returncode"]:
        print(f"PASS temporal verification: frozen arithmetic GREEN/work RED; candidate exact/work GREEN; summary={build / 'summary.json'}")
    return evidence["returncode"]


if __name__ == "__main__":
    raise SystemExit(main())
