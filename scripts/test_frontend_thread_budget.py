"""Run real DSP/Qwen/R2T2 FFT-budget propagation checks without models or CUDA.

Example: python scripts/test_frontend_thread_budget.py --source build/r2t2/source
The test forwards every observed request to the unchanged FFT. Requested jobs
are observed at STFT's FFT boundary; OS thread counts are outside this contract.
"""
import argparse
import json
from pathlib import Path
import shutil
import subprocess

from build_r2t2_runtime import ROOT, build_environment
from test_r2t2_logmel_cache import EMPTY, FFT, SUPPORT, save, sha


TEST = ROOT / "scripts/native_tests/frontend_thread_budget_test.cpp"
OWNERS = (
    "src/models/qwen3_asr/frontend_whisper.cpp",
    "src/community_models/confucius4_r2t2/frontend_whisper.cpp",
    "src/framework/audio/dsp.cpp",
)


def fingerprint(source):
    paths = {name: source / name for name in (*OWNERS, *SUPPORT, FFT)}
    for directory in (source / "include", source / "external/ggml/include",
                      source / "src/framework/audio/detail"):
        for path in directory.rglob("*.h"):
            paths[path.relative_to(source).as_posix()] = path
    for path in (TEST, EMPTY, Path(__file__).resolve(), ROOT / "scripts/build_r2t2_runtime.py",
                 ROOT / "scripts/test_r2t2_logmel_cache.py"):
        paths[path.relative_to(ROOT).as_posix()] = path
    return {name: sha(path) for name, path in sorted(paths.items())}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--build-dir", type=Path, default=ROOT / "tmp/frontend-thread-budget")
    parser.add_argument("--no-build", action="store_true")
    args = parser.parse_args()
    source, build = args.source.resolve(), args.build_dir.resolve()
    build.mkdir(parents=True, exist_ok=True)
    inputs = fingerprint(source)
    env, _ = build_environment()
    env = {key.upper(): value for key, value in env.items()}
    env.update(OMP_NUM_THREADS="1", MKL_NUM_THREADS="1", OPENBLAS_NUM_THREADS="1")
    compiler, linker = (shutil.which(name, path=env["PATH"]) for name in ("cl.exe", "link.exe"))
    if not compiler or not linker:
        parser.error("MSVC compiler/linker unavailable after vcvars")
    executable = build / "frontend_thread_budget_test.exe"
    manifest = build / "compiled-source.json"
    commands = []

    def run(command):
        command = list(map(str, command))
        commands.append(command)
        result = subprocess.run(command, cwd=build, env=env, capture_output=True,
                                text=True, errors="replace")
        with (build / "build.log").open("a", encoding="utf-8") as log:
            log.write(subprocess.list2cmdline(command) + "\n" + result.stdout + result.stderr)
        if result.returncode:
            save(build / "build-failure.json", {"source": str(source), "inputs": inputs,
                 "commands": commands, "returncode": result.returncode,
                 "output": result.stdout + result.stderr})
            print(result.stdout + result.stderr, end="")
            raise RuntimeError(f"native compile/link failed ({result.returncode}); see {build / 'build.log'}")

    if args.no_build:
        previous = json.loads(manifest.read_text(encoding="utf-8"))
        if previous["inputs"] != inputs or previous["executable_sha256"] != sha(executable):
            parser.error("compiled inputs changed; rerun without --no-build")
    else:
        (build / "build.log").write_text("", encoding="utf-8")
        common = [compiler, "/nologo", "/c", "/std:c++17", "/EHsc", "/O2", "/Gy", "/MD", "/MP1",
                  "/DNOMINMAX", "/D_CRT_SECURE_NO_WARNINGS", f"/I{source / 'include'}",
                  f"/I{source / 'external/ggml/include'}"]
        objects = []
        # Check the typed API first. Missing declarations are INTERFACE_PENDING,
        # never semantic RED; a declaration scaffold may then expose behavior.
        sources = [TEST, EMPTY, *[source / name for name in (*OWNERS, *SUPPORT, FFT)]]
        for index, path in enumerate(sources):
            obj = build / f"{index}_{path.stem}.obj"
            rename = ["/Dreal_fft_forward=frontend_budget_real_fft_forward"] if path == source / FFT else []
            run([*common, *rename, f"/Fo{obj}", path])
            objects.append(obj)
        run([linker, "/nologo", "/OPT:REF", f"/OUT:{executable}", *objects])
        if fingerprint(source) != inputs:
            raise RuntimeError("inputs changed during compilation")
        save(manifest, {"source": str(source), "inputs": inputs, "commands": commands,
                        "executable_sha256": sha(executable)})

    result = subprocess.run([str(executable)], cwd=build, env=env, capture_output=True,
                            text=True, errors="replace")
    output = result.stdout + result.stderr
    (build / "test.log").write_text(output, encoding="utf-8")
    save(build / "results.json", {"source": str(source), "inputs": inputs,
         "command": [str(executable)], "returncode": result.returncode,
         "executable_sha256": sha(executable), "output": output})
    print(output, end="")
    return result.returncode


if __name__ == "__main__":
    raise SystemExit(main())
