"""Compile the actual CUDA dispatcher with test-only observers; run on NVIDIA GPU 0.

Paths are CLI inputs. Existing static libraries/source are read-only. All outputs
stay in --build-dir. Collection performs no compilation or GPU work.
"""
import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import runpy
import shutil
import subprocess
import sys

HERE = Path(__file__).resolve().parent
OWNER = Path("external/ggml/src/ggml-cuda/ggml-cuda.cu")
LIBRARIES = ("ggml/src/ggml-cuda/ggml-cuda.lib", "ggml/src/ggml-base.lib",
             "ggml/src/ggml.lib", "ggml/src/ggml-cpu.lib")


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def windows_arguments(command):
    count = ctypes.c_int()
    split = ctypes.windll.shell32.CommandLineToArgvW
    split.argtypes = (ctypes.c_wchar_p, ctypes.POINTER(ctypes.c_int))
    split.restype = ctypes.POINTER(ctypes.c_wchar_p)
    values = split(command, ctypes.byref(count))
    if not values:
        raise RuntimeError("cannot parse recorded Windows compiler command")
    try:
        return [values[i] for i in range(count.value)]
    finally:
        ctypes.windll.kernel32.LocalFree(ctypes.cast(values, ctypes.c_void_p))


def compile_flags(build, source):
    records = json.loads((build / "compile_commands.json").read_text(encoding="utf-8"))
    owner = next(row for row in records if Path(row["file"]).name == OWNER.name)
    tokens = windows_arguments(owner["command"])
    flags = []
    index = 1
    while index < len(tokens):
        token = tokens[index]
        if token in ("-c",):
            index += 1
        elif token in ("-o", "-x"):
            index += 2
        elif token in (owner["file"], str(source / OWNER)):
            index += 1
        elif token.startswith("-Xcompiler=-Fd"):
            index += 1
        else:
            flags.append(token)
            index += 1
    recorded_source = str(Path(owner["file"]).parents[4])
    return [flag.replace(recorded_source, str(source)) for flag in flags]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--libraries", type=Path, required=True)
    parser.add_argument("--cuda-root", type=Path, required=True)
    parser.add_argument("--build-helper", type=Path, required=True, help="Existing build_r2t2_runtime.py for MSVC environment")
    parser.add_argument("--build-dir", type=Path, default=HERE / "build")
    parser.add_argument("--collect", action="store_true")
    parser.add_argument("--compile-only", action="store_true")
    parser.add_argument("--baseline", action="store_true", help="Require stock GREEN and intended missing-controller semantic RED")
    parser.add_argument("--baseline-control", type=Path, help="Frozen baseline/default-control.json required for candidate execution")
    args = parser.parse_args()
    source, libraries, sdk, helper, build = (p.resolve() for p in
        (args.source, args.libraries, args.cuda_root, args.build_helper, args.build_dir))
    tests = [HERE / name for name in ("autotune_test.cu", "observer.h", "interpose.h", "run.py")]
    inputs = [*tests, helper, source / OWNER, libraries / "compile_commands.json",
              *sorted((source / "external/ggml/include").glob("*.h")),
              *sorted((source / "external/ggml/src/ggml-cuda").rglob("*.cuh")),
              source / "external/ggml/src/ggml-backend-impl.h"]
    for path in inputs:
        if not path.is_file():
            parser.error(f"input missing; setup failure, not semantic RED: {path}")
    if args.collect:
        print(json.dumps({"status": "COLLECTED_NOT_COMPILED", "reserved_lowering": 4,
                          "public_apis": ["ggml_mul_mat_set_lowering", "ggml_backend_graph_compute", "ggml_backend_graph_compute_async"],
                          "cases": "stock; calibration/cache/key/context/lifetime/type/precision/strides/columns/capture; fatal CUDA event error",
                          "test_sha256": {p.name: digest(p) for p in tests},
                          "owner_sha256": digest(source / OWNER), "semantic_red": "not yet executed"}, indent=2))
        return 0
    if os.name != "nt":
        parser.error("this runner reuses a Windows MSVC/CUDA static-library build")
    if not args.baseline and not args.compile_only:
        if args.baseline_control is None:
            parser.error("candidate execution requires --baseline-control for unchanged default-lowering bits/dispatch")
        args.baseline_control = args.baseline_control.resolve()
        if args.baseline_control == build / "default-control.json":
            parser.error("frozen baseline control must differ from candidate output")
        inputs.append(args.baseline_control)
    if os.environ.get("OMP_NUM_THREADS") != "1" or os.environ.get("CARGO_BUILD_JOBS") != "1":
        parser.error("apply the task resource limiter before compilation/execution")
    inputs += [libraries / name for name in LIBRARIES]
    for path in inputs:
        if not path.is_file():
            parser.error(f"static build not ready; setup failure, not semantic RED: {path}")
    fingerprint = {str(p): digest(p) for p in inputs}
    env, _ = runpy.run_path(str(helper))["build_environment"]()
    env = {key.upper(): value for key, value in env.items()}
    env["PATH"] = str(sdk / "bin") + os.pathsep + env["PATH"]
    env["GGML_CUDA_DISABLE_GRAPHS"] = "1"  # Process-local; test manual raw capture explicitly.
    compiler = sdk / "bin/nvcc.exe"
    linker = shutil.which("link.exe", path=env["PATH"])
    if not compiler.is_file() or not linker:
        parser.error("CUDA compiler/MSVC linker unavailable")
    build.mkdir(parents=True, exist_ok=True)
    evidence = {"status": "PENDING", "inputs": fingerprint, "commands": [], "results": {},
                "gpu": 0, "automatic_cuda_graphs": False, "manual_raw_capture": True,
                "synthetic": True, "performance_claim": False, "baseline": args.baseline}

    def execute(command, name, expected=None):
        command = list(map(str, command))
        result = subprocess.run(command, cwd=build, env=env, capture_output=True, text=True, errors="replace")
        (build / f"{name}.log").write_text(subprocess.list2cmdline(command) + "\n" + result.stdout + result.stderr, encoding="utf-8")
        evidence["commands"].append({"command": command, "returncode": result.returncode})
        if expected is not None and result.returncode != expected:
            raise RuntimeError(f"{name}: exit {result.returncode}, expected {expected}; inspect {build / (name + '.log')}")
        return result

    try:
        flags = compile_flags(libraries, source)
        execute([compiler, *flags, "--pre-include", HERE / "interpose.h", "-I" + str(HERE),
                 "-c", source / OWNER, "-o", build / "dispatcher.obj"], "compile-dispatcher", 0)
        execute([compiler, *flags, "-I" + str(HERE), "-c", HERE / "autotune_test.cu",
                 "-o", build / "fixture.obj"], "compile-fixture", 0)
        executable = build / "autotune_test.exe"
        # Explicit dispatcher object satisfies its archive members' symbols first.
        execute([linker, "/nologo", "/OPT:REF", f"/OUT:{executable}", build / "fixture.obj", build / "dispatcher.obj",
                 *[libraries / name for name in LIBRARIES], f"/LIBPATH:{sdk / 'lib/x64'}",
                 "cudart.lib", "cublas.lib", "cublasLt.lib", "cuda.lib", "cudadevrt.lib",
                 "kernel32.lib", "advapi32.lib", "ws2_32.lib"], "link", 0)
        evidence["executable_sha256"] = digest(executable)
        if args.compile_only:
            evidence["status"] = "COMPILED_NOT_EXECUTED"
        else:
            stock = execute([executable, "stock"], "stock", 0)
            if '"STOCK_GREEN"' not in stock.stdout:
                raise RuntimeError("stock characterization lacked GREEN evidence")
            evidence["results"]["stock"] = json.loads(stock.stdout.splitlines()[-1])
            contract = execute([executable, "contract"], "contract", 2 if args.baseline else 0)
            rows = [json.loads(line) for line in contract.stdout.splitlines() if line.startswith("{")]
            summary = rows[-1]
            if args.baseline and (summary.get("work_red", 0) == 0 or "WORK_RED" not in contract.stderr):
                raise RuntimeError("baseline did not fail for missing controller behavior")
            if "FIXTURE_ERROR" in contract.stderr:
                raise RuntimeError("fixture/setup failure is not semantic RED")
            (build / "observations.json").write_text(json.dumps(rows, indent=2) + "\n", encoding="utf-8")
            evidence["results"]["contract"] = summary
            default_rows = [{key: row[key] for key in ("case", "phase", "lowering", "capture", "k", "vocab", "n", "launches", "default_bits")}
                            for row in rows if row.get("case") == "default_lowering"]
            if len(default_rows) != 4 or any(len(row["default_bits"]) != 132 or row["lowering"] != 0 for row in default_rows):
                raise RuntimeError("default-lowering control observations incomplete")
            control = {"source_owner_sha256": digest(source / OWNER), "observations": default_rows}
            (build / "default-control.json").write_text(json.dumps(control, indent=2) + "\n", encoding="utf-8")
            if not args.baseline:
                frozen = json.loads(args.baseline_control.read_text(encoding="utf-8"))
                if frozen["observations"] != default_rows:
                    raise RuntimeError("candidate changed frozen default-lowering output bits or dispatch")
            evidence["results"]["default_lowering"] = {"observations": 4, "output_bits": 528,
                "frozen_bits_equal": None if args.baseline else True,
                "control_sha256": digest(build / "default-control.json")}
            fatal = execute([executable, "fatal-event"], "fatal-event")
            if args.baseline:
                if fatal.returncode != 2 or "WORK_RED" not in fatal.stderr:
                    raise RuntimeError("baseline fatal-event case did not expose missing calibration work")
            elif fatal.returncode in (0, 1, 2) or "CUDA error" not in fatal.stderr:
                raise RuntimeError("injected CUDA error did not reach the existing fatal CUDA route")
            evidence["results"]["fatal_event"] = {"returncode": fatal.returncode}
            evidence["status"] = "BASELINE_SEMANTIC_RED_VERIFIED" if args.baseline else "PASS"
    except (RuntimeError, StopIteration, ValueError) as error:
        evidence.update(status="FIXTURE_OR_CHECK_FAILURE", error=str(error))
        print(error, file=sys.stderr)
    finally:
        changed = [name for name, old in fingerprint.items() if not Path(name).is_file() or digest(Path(name)) != old]
        evidence["inputs_unchanged"] = not changed
        if changed:
            evidence.update(status="INPUT_CHANGED", changed_inputs=changed)
        (build / "summary.json").write_text(json.dumps(evidence, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"status": evidence["status"], "results": evidence["results"], "summary": str(build / "summary.json")}))
    return 0 if evidence["status"] in ("PASS", "BASELINE_SEMANTIC_RED_VERIFIED", "COMPILED_NOT_EXECUTED") else 1


if __name__ == "__main__":
    raise SystemExit(main())
