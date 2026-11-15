"""CPU-only queued KV suffix-clear regression against the frozen asr10 clear.

Compile the current and pinned cache owners against existing CPU static libraries.
The baseline must pass byte equivalence and fail the separate queued-work guard.
Build artifacts stay in an ignored directory; no model or GPU is used.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
from build_r2t2_runtime import ROOT, build_environment

TEST = ROOT / "scripts/native_tests/r2t2_kv_clear_test.cpp"
KV_CPP = Path("src/framework/runtime/kv_cache.cpp")
KV_HEADER = Path("include/engine/framework/runtime/kv_cache.h")

FROZEN_LF_SHA256 = {
    KV_CPP: "bd7c5486d3a95efdc8a55eb900c8c0a4a59dcb59c208ac5788104f03824f1aaa",
    KV_HEADER: "ea8000c6fa8934e0c54ecc875310a376631b4f980c443ecc01778995a514ee60",
}
LIBRARIES = ("engine_runtime.lib", "ggml/src/ggml.lib", "ggml/src/ggml-cpu.lib",
             "ggml/src/ggml-base.lib", "external/sentencepiece/src/sentencepiece.lib",
             "cjson_vendor.lib", "yaml_vendor.lib")


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--libraries", required=True, type=Path, help="Existing CPU static-library build")
    parser.add_argument("--baseline-root", required=True, type=Path, help="Frozen original KV source/header root")
    parser.add_argument("--build-dir", type=Path, default=ROOT / "tmp/r2t2-kv-clear-tests")
    parser.add_argument("--variant", choices=("both", "baseline", "candidate"), default="both")
    parser.add_argument("--mode", choices=("all", "equivalence", "work"), default="all")
    parser.add_argument("--no-build", action="store_true")
    args = parser.parse_args()
    source, libraries, baseline, build = (path.resolve() for path in (
        args.source, args.libraries, args.baseline_root, args.build_dir))
    build.mkdir(parents=True, exist_ok=True)
    variants = ("baseline", "candidate") if args.variant == "both" else (args.variant,)
    paths = [TEST, Path(__file__).resolve(),
             ROOT / "scripts/build_r2t2_runtime.py",
             *[baseline / path for path in FROZEN_LF_SHA256],
             *[libraries / path for path in LIBRARIES],
             *sorted((source / "include").rglob("*.h")),
             *sorted((source / "external/ggml/include").glob("*.h")),
             source / "external/ggml/src/ggml-backend-impl.h"]
    if "candidate" in variants:
        paths.append(source / KV_CPP)
    for path in paths:
        if not path.is_file():
            parser.error(f"required input missing: {path}")
    for path, expected in FROZEN_LF_SHA256.items():
        canonical = (baseline / path).read_bytes().replace(b"\r\n", b"\n")
        if hashlib.sha256(canonical).hexdigest() != expected:
            parser.error(f"frozen baseline changed: {path}")
    fingerprint = {str(path): sha256(path) for path in paths}
    env, _ = build_environment()
    env = {key.upper(): value for key, value in env.items()}
    compiler = shutil.which("cl.exe", path=env["PATH"])
    linker = shutil.which("link.exe", path=env["PATH"])
    if not compiler or not linker:
        parser.error("Visual Studio cl.exe/link.exe unavailable")
    commands = []

    def execute(command, log_name):
        command = list(map(str, command))
        commands.append(command)
        result = subprocess.run(command, cwd=build, env=env, capture_output=True,
                                text=True, errors="replace")
        with (build / log_name).open("a", encoding="utf-8") as log:
            log.write(subprocess.list2cmdline(command) + "\n" + result.stdout + result.stderr)
        return result

    executables = {name: build / f"r2t2_kv_clear_{name}.exe" for name in variants}
    manifest = build / "compiled-source.json"
    if not args.no_build:
        (build / "build.log").write_text("", encoding="utf-8")
        common = [compiler, "/nologo", "/c", "/std:c++17", "/EHsc", "/O2", "/Gy", "/MD",
                  "/DNDEBUG", "/DNOMINMAX", "/D_CRT_SECURE_NO_WARNINGS"]
        for name in variants:
            includes = ([f"/I{baseline / 'include'}", "/DBASELINE=1"] if name == "baseline" else [])
            includes.extend([f"/I{source / 'include'}", f"/I{source / 'external/ggml/include'}",
                             f"/I{source / 'external/ggml/src'}"])
            owner = baseline / KV_CPP if name == "baseline" else source / KV_CPP
            for command in [
                [*common, *includes, f"/Fo{build / (name + '-test.obj')}", TEST],
                [*common, *includes, f"/Fo{build / (name + '-cache.obj')}", owner],
                [linker, "/nologo", "/OPT:REF", f"/OUT:{executables[name]}",
                 build / (name + "-test.obj"), build / (name + "-cache.obj"),
                 *[libraries / path for path in LIBRARIES], "kernel32.lib", "advapi32.lib"],
            ]:
                result = execute(command, "build.log")
                if result.returncode:
                    print(result.stdout + result.stderr, end="")
                    raise RuntimeError(f"compilation/link error; semantic RED not established: {build / 'build.log'}")
        if {str(path): sha256(path) for path in paths} != fingerprint:
            parser.error("native test inputs changed during compilation; rebuild")
        manifest.write_text(json.dumps({"inputs": fingerprint, "executables": {
            name: sha256(path) for name, path in executables.items()}}, indent=2) + "\n", encoding="utf-8")
    else:
        previous = json.loads(manifest.read_text(encoding="utf-8"))
        if previous["inputs"] != fingerprint or previous["executables"] != {
                name: sha256(path) for name, path in executables.items()}:
            parser.error("native test inputs changed; rebuild")

    evidence = {"inputs": fingerprint, "mode": args.mode, "variant": args.variant,
                "commands": commands, "synthetic_cpu_only": True, "results": {}}
    returncode, traces = 0, {}
    for name, executable in executables.items():
        for mode in (("equivalence", "work") if args.mode == "all" else (args.mode,)):
            log = build / f"{name}-{mode}.log"
            log.write_text("", encoding="utf-8")
            trace = build / f"{name}-{mode}.bin"
            result = execute([executable, mode, trace], log.name)
            evidence["results"][f"{name}-{mode}"] = {
                "returncode": result.returncode, "stdout": result.stdout, "stderr": result.stderr}
            print(f"{name}-{mode}: " + "\n".join(result.stdout.splitlines()[-2:]))
            expected_red = (name == "baseline" and mode == "work" and result.returncode == 1
                            and "KV_ASYNC_QUALITY_OK " in result.stdout
                            and "WORK_RED queued suffix clear" in result.stdout
                            and "FAIL queued suffix clear requires async hooks and one synchronization" in result.stderr)
            if name == "baseline" and mode == "work":
                evidence["baseline_expected_semantic_red"] = expected_red
            if result.returncode and not (expected_red and args.variant == "both"):
                print(result.stderr, end="")
                returncode = result.returncode
            elif name == "baseline" and mode == "work" and not expected_red:
                print("FAIL baseline work control did not show expected semantic RED")
                returncode = 1
            if trace.is_file() and "KV_ASYNC_QUALITY_OK " in result.stdout:
                traces[f"{name}-{mode}"] = trace.read_bytes()
    for left, right in (("baseline-equivalence", "candidate-equivalence"),
                        *[(f"{name}-equivalence", f"{name}-work") for name in variants]):
        if left in traces and right in traces:
            equal = traces[left] == traces[right]
            evidence[f"{left}_vs_{right}_exact"] = equal
            print(f"{'PASS' if equal else 'FAIL'} exact byte trace {left} == {right} bytes={len(traces[left])}")
            if not equal:
                returncode = 1
    evidence["trace_sha256"] = {key: hashlib.sha256(value).hexdigest() for key, value in traces.items()}
    evidence["returncode"] = returncode
    (build / f"{args.variant}-{args.mode}.json").write_text(
        json.dumps(evidence, indent=2) + "\n", encoding="utf-8")
    return returncode


if __name__ == "__main__":
    raise SystemExit(main())
