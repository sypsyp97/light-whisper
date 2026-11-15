"""Real CPU prompt lookup regression; frozen arithmetic, graph work, failed refresh.

Only local test/owner objects are compiled. Existing static libraries stay read-only.
The established Qwen synthetic assets are included by the C++ test, while the
production greedy owner and causal runtime perform all embedding/decoder math.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess

from build_r2t2_runtime import ROOT, build_environment
from test_r2t2_prefill import LIBRARIES

OWNER = Path("src/framework/runtime/greedy_qwen_decoder.cpp")
CAUSAL = Path("src/framework/modules/transformers/qwen_causal_decode_runtime.cpp")
TEST = ROOT / "scripts/native_tests/r2t2_prompt_embeddings_test.cpp"
INTERPOSE = ROOT / "scripts/native_tests/r2t2_prompt_embeddings_interpose.h"
BASELINE_LF_SHA256 = "940fc637fefc152e0d5439f5a7af9a8b45684e966ffd2828964838048577f9aa"


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--libraries", required=True, type=Path)
    parser.add_argument("--baseline-root", required=True, type=Path)
    parser.add_argument("--build-dir", type=Path, default=ROOT / "tmp/r2t2-prompt-embedding-tests")
    parser.add_argument("--variant", choices=("baseline", "candidate", "both"), default="both")
    parser.add_argument("--mode", choices=("all", "equivalence", "work"), default="all")
    parser.add_argument("--no-build", action="store_true")
    args = parser.parse_args()
    source, libraries, baseline, build = (path.resolve() for path in (
        args.source, args.libraries, args.baseline_root, args.build_dir))
    if build == source or build == libraries or (build / "CMakeCache.txt").exists():
        parser.error("test build must be separate from source and cached CPU libraries")
    build.mkdir(parents=True, exist_ok=True)
    variants = ("baseline", "candidate") if args.variant == "both" else (args.variant,)
    paths = [TEST, INTERPOSE,
             Path(__file__).resolve(), ROOT / "scripts/native_tests/qwen_shared_decoder_test.cpp",
             ROOT / "scripts/build_r2t2_runtime.py", ROOT / "scripts/test_r2t2_prefill.py",
             source / OWNER, baseline / OWNER, source / CAUSAL,
             *[libraries / name for name in LIBRARIES],
             *sorted((source / "include").rglob("*.h")),
             *sorted((source / "external/ggml/include").glob("*.h"))]
    for path in paths:
        if not path.is_file():
            parser.error(f"required test input missing: {path}")

    if hashlib.sha256((baseline / OWNER).read_bytes().replace(b"\r\n", b"\n")).hexdigest() != BASELINE_LF_SHA256:
        parser.error("frozen greedy owner normalized-LF SHA256 mismatch")
    fingerprint = {str(path): sha256(path) for path in paths}
    env, _ = build_environment()
    env = {key.upper(): value for key, value in env.items()}
    compiler = shutil.which("cl.exe", path=env["PATH"])
    linker = shutil.which("link.exe", path=env["PATH"])
    if not compiler or not linker:
        parser.error("MSVC cl.exe/link.exe unavailable")
    commands = []

    def execute(command, log_name):
        command = list(map(str, command))
        commands.append(command)
        result = subprocess.run(command, cwd=build, env=env, capture_output=True,
                                text=True, errors="replace")
        with (build / log_name).open("a", encoding="utf-8") as log:
            log.write(subprocess.list2cmdline(command) + "\n" + result.stdout + result.stderr)
        return result

    executables = {name: build / f"r2t2_prompt_embeddings_{name}.exe" for name in variants}
    manifest = build / "compiled-source.json"
    if not args.no_build:
        (build / "build.log").write_text("", encoding="utf-8")
        common = [compiler, "/nologo", "/c", "/std:c++17", "/EHsc", "/O2", "/Gy", "/MD",
                  "/DNDEBUG", "/DNOMINMAX", "/D_CRT_SECURE_NO_WARNINGS",
                  f"/I{source / 'include'}", f"/I{source / 'external/ggml/include'}"]
        interpose = f"/FI{INTERPOSE}"
        build_commands = [
            [*common, f"/Fo{build / 'test.obj'}", TEST],
            [*common, interpose, f"/Fo{build / 'causal.obj'}", source / CAUSAL],
        ]
        for name in variants:
            owner = baseline / OWNER if name == "baseline" else source / OWNER
            obj = build / f"{name}.obj"
            build_commands.extend([
                [*common, interpose, f"/Fo{obj}", owner],
                [linker, "/nologo", "/OPT:REF", f"/OUT:{executables[name]}",
                 build / "test.obj", build / "causal.obj", obj,
                 *[libraries / path for path in LIBRARIES], "kernel32.lib", "advapi32.lib"],
            ])
        for command in build_commands:
            result = execute(command, "build.log")
            if result.returncode:
                print(result.stdout + result.stderr, end="")
                raise RuntimeError(f"INFRASTRUCTURE compile/link error; semantic RED not established: {build / 'build.log'}")
        manifest.write_text(json.dumps({"inputs": fingerprint, "executables": {
            name: sha256(path) for name, path in executables.items()}}, indent=2) + "\n", encoding="utf-8")
    else:
        previous = json.loads(manifest.read_text(encoding="utf-8"))
        if previous["inputs"] != fingerprint or previous["executables"] != {
                name: sha256(path) for name, path in executables.items()}:
            parser.error("compiled test inputs changed; rebuild without --no-build")

    evidence = {"inputs": fingerprint, "mode": args.mode, "variant": args.variant,
                "commands": commands, "synthetic_cpu_only": True, "results": {}}
    returncode = 0
    traces = {}
    for name, executable in executables.items():
        for mode in (("equivalence", "work") if args.mode == "all" else (args.mode,)):
            key = f"{name}-{mode}"
            log = build / f"{key}.log"
            log.write_text("", encoding="utf-8")
            trace = build / f"{key}.bin"
            result = execute([executable, mode, trace], log.name)
            evidence["results"][key] = {"returncode": result.returncode,
                                        "stdout": result.stdout, "stderr": result.stderr}
            print(key + ": " + "\n".join(result.stdout.splitlines()[-2:]))
            semantic_red = result.returncode == 1 and "PROMPT_QUALITY_OK " in result.stdout and (
                "FAIL prompt lookup still computes placeholder or unchanged-text rows in " in result.stderr)
            if name == "baseline" and mode == "work":
                evidence["baseline_expected_semantic_red"] = semantic_red
                if not semantic_red:
                    print("FAIL frozen baseline did not expose expected prompt lookup work")
                    returncode = 1
            if result.returncode and not (name == "baseline" and mode == "work" and semantic_red and
                                          args.variant == "both"):
                print(result.stderr, end="")
                returncode = result.returncode
            if trace.is_file() and "PROMPT_QUALITY_OK " in result.stdout:
                traces[key] = trace.read_bytes()
    if "baseline-equivalence" in traces and "candidate-equivalence" in traces:
        equal = traces["baseline-equivalence"] == traces["candidate-equivalence"]
        evidence["exact_trace_equal"] = equal
        print(f"{'PASS' if equal else 'FAIL'} exact frozen-baseline token trace bytes={len(traces['baseline-equivalence'])}")
        if not equal:
            returncode = 1
    for name in variants:
        if f"{name}-equivalence" in traces and f"{name}-work" in traces:
            equal = traces[f"{name}-equivalence"] == traces[f"{name}-work"]
            evidence[f"{name}_work_trace_equal"] = equal
            if not equal:
                returncode = 1
    evidence["trace_sha256"] = {key: hashlib.sha256(value).hexdigest() for key, value in traces.items()}
    evidence["returncode"] = returncode
    (build / f"{args.variant}-{args.mode}.json").write_text(
        json.dumps(evidence, indent=2) + "\n", encoding="utf-8")
    return returncode


if __name__ == "__main__":
    raise SystemExit(main())
