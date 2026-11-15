"""Exercise real Qwen chunked prefill on CPU with synthetic test-only weights.

Compile only the owning runtime translation unit, interposing graph execution
to count actual LM-head/final-normalization nodes. Other engine/ggml code comes
from read-only cached static libraries. A frozen runtime copy supplies exact
before/after output and greedy-continuation traces; no model quality is claimed.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess

from build_r2t2_runtime import ROOT, build_environment


RUNTIME = "src/framework/modules/transformers/qwen_causal_decode_runtime.cpp"
BASELINE_LF_SHA256 = "a84e92c31f641f75768061e7241d78d3da45fcca69900ab8507a1d7ea72e0cfc"
TEST = ROOT / "scripts/native_tests/r2t2_prefill_test.cpp"
INTERPOSE = ROOT / "scripts/native_tests/r2t2_prefill_interpose.h"
LIBRARIES = ("engine_runtime.lib", "ggml/src/ggml.lib", "ggml/src/ggml-cpu.lib",
             "ggml/src/ggml-base.lib", "external/sentencepiece/src/sentencepiece.lib",
             "cjson_vendor.lib", "yaml_vendor.lib")


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--libraries", required=True, type=Path, help="Existing CPU static-library build")
    parser.add_argument("--baseline", required=True, type=Path, help="Frozen original runtime .cpp")
    parser.add_argument("--build-dir", type=Path, default=ROOT / "tmp/asr-inference-20261017/prefill-tests")
    parser.add_argument("--mode", choices=("all", "equivalence", "work"), default="all")
    parser.add_argument("--no-build", action="store_true")
    args = parser.parse_args()
    source, libraries, baseline, build = (path.resolve() for path in (
        args.source, args.libraries, args.baseline, args.build_dir))
    build.mkdir(parents=True, exist_ok=True)
    paths = [source / RUNTIME, baseline, TEST, INTERPOSE, Path(__file__).resolve(),
             *[libraries / name for name in LIBRARIES],
             *sorted((source / "include").rglob("*.h")),
             *sorted((source / "external/ggml/include").glob("*.h"))]
    for path in paths:
        if not path.is_file():
            parser.error(f"required native test input missing: {path}")
    fingerprint = {str(path): sha256(path) for path in paths}

    if hashlib.sha256(baseline.read_bytes().replace(b"\r\n", b"\n")).hexdigest() != BASELINE_LF_SHA256:
        parser.error("baseline runtime differs from the frozen pre-optimization fixture")
    env, _ = build_environment()
    env = {key.upper(): value for key, value in env.items()}
    compiler = shutil.which("cl.exe", path=env["PATH"])
    linker = shutil.which("link.exe", path=env["PATH"])
    if not compiler or not linker:
        parser.error("Visual Studio cl.exe/link.exe unavailable after vcvars")
    commands = []

    def execute(command, log_name):
        command = list(map(str, command))
        commands.append(command)
        result = subprocess.run(command, cwd=build, env=env, capture_output=True,
                                text=True, errors="replace")
        with (build / log_name).open("a", encoding="utf-8") as log:
            log.write(subprocess.list2cmdline(command) + "\n" + result.stdout + result.stderr)
        return result

    executables = {name: build / f"r2t2_prefill_{name}.exe" for name in ("baseline", "candidate")}
    manifest = build / "compiled-source.json"
    if not args.no_build:
        (build / "build.log").write_text("", encoding="utf-8")
        common = [compiler, "/nologo", "/c", "/std:c++17", "/EHsc", "/O2", "/Gy", "/MD",
                  "/DNDEBUG", "/DNOMINMAX", "/D_CRT_SECURE_NO_WARNINGS",
                  f"/I{source / 'include'}", f"/I{source / 'external/ggml/include'}"]
        build_commands = [[*common, f"/Fo{build / 'test.obj'}", TEST]]
        for name, runtime in (("baseline", baseline), ("candidate", source / RUNTIME)):
            build_commands.extend([
                [*common, f"/FI{INTERPOSE}", f"/Fo{build / (name + '.obj')}", runtime],
                [linker, "/nologo", "/OPT:REF", f"/OUT:{executables[name]}", build / "test.obj",
                 build / (name + ".obj"), *[libraries / path for path in LIBRARIES],
                 "kernel32.lib", "advapi32.lib"],
            ])
        for command in build_commands:
            result = execute(command, "build.log")
            if result.returncode:
                print(result.stdout + result.stderr, end="")
                raise RuntimeError(f"native test compilation/link failed; see {build / 'build.log'}")
        manifest.write_text(json.dumps({"inputs": fingerprint, "executables": {
            name: sha256(path) for name, path in executables.items()}}, indent=2) + "\n", encoding="utf-8")
    else:
        previous = json.loads(manifest.read_text(encoding="utf-8"))
        if previous["inputs"] != fingerprint or previous["executables"] != {
                name: sha256(path) for name, path in executables.items()}:
            parser.error("compiled native test inputs changed; rerun without --no-build")

    evidence = {"inputs": fingerprint, "mode": args.mode, "commands": commands,
                "synthetic_weights_only": True, "results": {}}
    returncode = 0
    if args.mode != "work":
        for name in executables:
            log = build / f"{name}-equivalence.log"
            log.write_text("", encoding="utf-8")
            result = execute([executables[name], "equivalence", build / f"{name}.bin"], log.name)
            evidence["results"][name] = result.returncode
            print(f"{name}: " + (result.stdout.splitlines()[-1] if result.stdout else "no output"))
            if result.returncode:
                print(result.stderr, end="")
                returncode = result.returncode
                break
        if not returncode:
            expected, actual = ((build / f"{name}.bin").read_bytes() for name in executables)
            evidence["exact_trace_equal"] = expected == actual
            evidence["trace_sha256"] = {name: sha256(build / f"{name}.bin") for name in executables}
            if expected != actual:
                returncode = 1
                print("FAIL frozen-baseline prefill/greedy continuation trace differs bit-for-bit")
            else:
                print(f"PASS exact frozen-baseline output/cache/greedy trace bytes={len(actual)}")
    if not returncode and args.mode != "equivalence":
        (build / "work.log").write_text("", encoding="utf-8")
        result = execute([executables["candidate"], "work", build / "work.bin"], "work.log")
        evidence["results"]["work"] = result.returncode
        failures = [line for line in result.stdout.splitlines() if line.startswith("WORK_RED ")]
        print("\n".join(failures[:3] + result.stdout.splitlines()[-1:]))
        print(result.stderr, end="")
        returncode = result.returncode
    evidence["returncode"] = returncode
    (build / f"{args.mode}.json").write_text(json.dumps(evidence, indent=2) + "\n", encoding="utf-8")
    return returncode


if __name__ == "__main__":
    raise SystemExit(main())
