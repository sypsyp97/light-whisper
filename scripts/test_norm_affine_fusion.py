"""Build/run a frozen real-GGML CUDA LayerNorm affine/residual test slice.

Only one test translation unit is compiled, against existing read-only GGML
libraries. All outputs stay under the worktree's tmp/norm-fusion by default.
Each CUDA process sets fusion before backend initialization, and external CUDA
graph capture counts actual kernel nodes around graph_compute_async. Numerical
checks compare an independent double reference, a CPU graph, and a separate
GGML_CUDA_DISABLE_FUSION=1 process. No downloads, engine builds or models.
"""
import argparse
import ctypes
from ctypes import wintypes
import hashlib
import json
import math
import os
from pathlib import Path
import re
import shutil
import subprocess

from build_r2t2_runtime import ROOT, build_environment


ATOL = RTOL = 3.0e-5


def sha256(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def constrain_process():
    """Children inherit two allowed logical CPUs and BelowNormal priority."""
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.GetCurrentProcess.restype = wintypes.HANDLE
    kernel.GetProcessAffinityMask.argtypes = [wintypes.HANDLE, ctypes.POINTER(ctypes.c_size_t), ctypes.POINTER(ctypes.c_size_t)]
    kernel.SetProcessAffinityMask.argtypes = [wintypes.HANDLE, ctypes.c_size_t]
    kernel.SetPriorityClass.argtypes = [wintypes.HANDLE, wintypes.DWORD]
    process = kernel.GetCurrentProcess()
    allowed, system = ctypes.c_size_t(), ctypes.c_size_t()
    if not kernel.GetProcessAffinityMask(process, ctypes.byref(allowed), ctypes.byref(system)):
        raise ctypes.WinError(ctypes.get_last_error())
    cpus = [1 << bit for bit in range(allowed.value.bit_length()) if allowed.value & (1 << bit)][:2]
    if not cpus or not kernel.SetProcessAffinityMask(process, sum(cpus)) or not kernel.SetPriorityClass(process, 0x4000):
        raise ctypes.WinError(ctypes.get_last_error())
    return {"priority": "BelowNormal", "logical_cpus": len(cpus), "affinity_mask": sum(cpus), "compiler_jobs": 1}


def compare(actual, expected):
    """Compare every exposed output, including retained intermediate consumers."""
    peers = {item["case"]: item for item in expected["cases"]}
    failures, elements, maximum = [], 0, 0.0
    actual_names = {item["case"] for item in actual["cases"]}
    expected_names = set(peers)
    if expected["backend"] == "cpu":
        actual_names -= {"f16_residual_fallback"}
    if actual_names != expected_names:
        failures.append("compared case names differ")
    for item in actual["cases"]:
        if item["case"] not in peers:
            continue  # The explicit case-name check above permits only the CPU F16 omission.
        other = peers[item["case"]]
        if item["outputs"].keys() != other["outputs"].keys():
            failures.append(f'{item["case"]}: output names differ')
            continue
        for name, values in item["outputs"].items():
            reference = other["outputs"][name]
            if len(values) != len(reference):
                failures.append(f'{item["case"]}/{name}: output length differs')
                continue
            for index, (value, peer) in enumerate(zip(values, reference)):
                elements += 1
                error = abs(value - peer)
                maximum = max(maximum, error)
                if not math.isfinite(value) or not math.isfinite(peer) or error > ATOL + RTOL * abs(peer):
                    failures.append(f'{item["case"]}/{name}[{index}]: {value} != {peer}')
                    break
    return {"elements": elements, "max_abs": maximum, "failures": failures}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True, type=Path, help="Existing reviewed audio.cpp source tree")
    parser.add_argument("--native-build-dir", required=True, type=Path, help="Existing GGML static-library build")
    parser.add_argument("--cuda-library", type=Path, help="Candidate ggml-cuda.lib; other libraries remain read-only")
    parser.add_argument("--cuda-root", type=Path, help="Defaults to CUDAToolkit_ROOT in the native CMakeCache")
    parser.add_argument("--build-dir", type=Path, default=ROOT / "tmp/norm-fusion")
    parser.add_argument("--mode", choices=("all", "numerics"), default="all")
    parser.add_argument("--backend", choices=("all", "cpu", "cuda"), default="all")
    parser.add_argument("--build-only", action="store_true")
    parser.add_argument("--no-build", action="store_true")
    args = parser.parse_args()
    if os.name != "nt":
        parser.error("this cached-library test harness requires Windows Visual Studio/CUDA")
    resources = constrain_process()
    source, native, build = (path.resolve() for path in (args.source, args.native_build_dir, args.build_dir))
    if args.cuda_root:
        cuda = args.cuda_root.resolve()
    else:
        cache = native / "CMakeCache.txt"
        if not cache.is_file():
            parser.error("--cuda-root required when native build has no CMakeCache.txt")
        match = re.search(r"^CUDAToolkit_ROOT:[^=]+=(.+)$", cache.read_text(encoding="utf-8"), re.M)
        if not match:
            parser.error("CUDAToolkit_ROOT absent from native CMakeCache; pass --cuda-root")
        cuda = Path(match.group(1).strip()).resolve()
    ggml = source / "external/ggml"
    test = ROOT / "scripts/native_tests/norm_affine_fusion_test.cpp"
    libs = [native / f"ggml/src/{name}.lib" for name in ("ggml", "ggml-cpu", "ggml-base")]
    cuda_library = (args.cuda_library or native / "ggml/src/ggml-cuda/ggml-cuda.lib").resolve()
    libs += [cuda_library, *[cuda / f"lib/x64/{name}.lib" for name in ("cublas", "cublasLt", "cuda", "cudart")]]
    inputs = [test, Path(__file__).resolve(), *libs, *sorted((ggml / "include").glob("*.h")), cuda / "include/cuda_runtime_api.h"]
    for path in inputs:
        if not path.is_file():
            parser.error(f"required native test input missing: {path}")
    fingerprints = {str(path): sha256(path) for path in inputs}
    build.mkdir(parents=True, exist_ok=True)
    env, _ = build_environment()
    env = {key.upper(): value for key, value in env.items()}
    env["PATH"] = str(cuda / "bin") + os.pathsep + env["PATH"]
    env["GGML_CUDA_DISABLE_GRAPHS"] = "1"
    env.pop("GGML_CUDA_GRAPH_OPT", None)
    env.update(OMP_NUM_THREADS="1", MKL_NUM_THREADS="1", OPENBLAS_NUM_THREADS="1")
    executable = build / "norm_affine_fusion_test.exe"
    manifest = build / "compiled.json"
    commands = []

    def execute(command, name, child_env=env):
        command = list(map(str, command))
        commands.append(command)
        result = subprocess.run(command, cwd=build, env=child_env, capture_output=True, text=True, errors="replace")
        (build / name).write_text(subprocess.list2cmdline(command) + "\n" + result.stdout + result.stderr, encoding="utf-8")
        return result

    if not args.no_build:
        compiler = shutil.which("cl.exe", path=env["PATH"])
        if not compiler:
            parser.error("Visual Studio cl.exe unavailable after vcvars")
        command = [compiler, "/nologo", "/std:c++17", "/EHsc", "/O2", "/MD", "/DNOMINMAX",
                   f"/I{ggml / 'include'}", f"/I{cuda / 'include'}", f"/Fo{build / 'test.obj'}", f"/Fe{executable}",
                   test, "/link", "/OPT:REF", *libs, "kernel32.lib", "advapi32.lib"]
        compiled = execute(command, "build.log")
        if compiled.returncode:
            print(compiled.stdout + compiled.stderr, end="")
            return compiled.returncode
        manifest.write_text(json.dumps({"inputs": fingerprints, "executable": sha256(executable),
                                        "command": commands[-1], "resources": resources}, indent=2) + "\n", encoding="utf-8")
    else:
        previous = json.loads(manifest.read_text(encoding="utf-8"))
        if previous["inputs"] != fingerprints or previous["executable"] != sha256(executable):
            parser.error("compiled native test inputs changed; rerun without --no-build")
    if args.build_only:
        print(json.dumps({"executable": str(executable), "sha256": sha256(executable), "executed": False}))
        return 0

    evidence = {"inputs": fingerprints, "resources": resources, "mode": args.mode, "results": {}, "comparisons": {}}
    specs = []
    if args.backend in ("all", "cpu"):
        specs.append(("cpu", "cpu", False))
    if args.backend in ("all", "cuda"):
        specs += [("cuda-unfused", "cuda", True), ("cuda-candidate", "cuda", False)]
    datasets = {}
    status = 0
    # Deliberately serialized: one compiler, one CPU run, one GPU run at a time.
    for name, backend, unfused in specs:
        child_env = {**env, "GGML_CUDA_DISABLE_FUSION": str(int(unfused))}
        result = execute([executable, "--backend", backend, "--mode", args.mode, "--unfused", str(int(unfused)),
                          "--output", build / f"{name}.json"], f"{name}.log", child_env)
        print(result.stdout, end="")
        evidence["results"][name] = {"returncode": result.returncode}
        if result.returncode:
            print(result.stderr, end="")
            status = max(status, 1)
        if result.returncode not in (0, 1):
            status = 2
            break
        datasets[name] = json.loads((build / f"{name}.json").read_text(encoding="utf-8"))
        evidence["results"][name].update({key: datasets[name][key] for key in ("total", "eligible", "failures")})
    for actual, reference in (("cuda-candidate", "cuda-unfused"), ("cuda-candidate", "cpu"), ("cuda-unfused", "cpu")):
        if actual not in datasets or reference not in datasets:
            continue
        comparison = compare(datasets[actual], datasets[reference])
        evidence["comparisons"][f"{actual}_vs_{reference}"] = comparison
        print(f'{actual} vs {reference}: elements={comparison["elements"]} max_abs={comparison["max_abs"]:.9g} failures={len(comparison["failures"])}')
        if comparison["failures"]:
            print("\n".join(comparison["failures"][:5]))
            status = max(status, 1)
    evidence.update(commands=commands, returncode=status)
    path = build / f"{args.mode}-evidence.json"
    path.write_text(json.dumps(evidence, indent=2) + "\n", encoding="utf-8")
    print(f"Evidence: {path}")
    return status


if __name__ == "__main__":
    raise SystemExit(main())
