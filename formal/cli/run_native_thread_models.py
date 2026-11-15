"""Run the finite native thread-budget model, sensitivity controls and witnesses."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys


TLC_SHA256 = "936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88"
CASES = {
    "NativeThreads": None,
    "NativeThreadsCLIEnvironmentNegative": "EngineEnvironment",
    "NativeThreadsAppEnvironmentNegative": "AppEnvironment",
    "NativeThreadsCLIValidationNegative": "CLIRejectsBeforeSpawn",
    "NativeThreadsEnvironmentNegative": "SelectionPrecedence",
    "NativeThreadsExplicitNegative": "SelectionPrecedence",
    "NativeThreadsOMPNegative": "OMPDefault",
    "NativeThreadsValidationNegative": "ValidationBeforeAllocation",
    "NativeThreadsABINegative": "ABIExactness",
    "NativeThreadsEnvironmentWitness": "WitnessNotReached",
    "NativeThreadsExplicitWitness": "WitnessNotReached",
    "NativeThreadsInvalidWitness": "WitnessNotReached",
    "NativeThreadsEngineWitness": "WitnessNotReached",
    "NativeThreadsAppWitness": "WitnessNotReached",
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--jar", type=Path, required=True)
    parser.add_argument("--java", default="java")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    directory = Path(__file__).resolve().parent
    root = directory.parents[1]
    sha = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
    jar = args.jar.resolve()
    if sha(jar) != TLC_SHA256:
        parser.error("--jar must identify the pinned TLA+ 1.7.4 JAR")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    inputs = [directory / "NativeThreads.tla", Path(__file__).resolve()]
    inputs.extend(directory / (name + ".cfg") for name in CASES)
    sources = [root / name for name in (
        "scripts/light_whisper_cli.py", "src-tauri/resources/r2t2_native.py",
        "src-tauri/resources/qwen3_native.py", "src-tauri/resources/r2t2_asr_server.py",
        "src-tauri/resources/qwen3_asr_server.py", "src-tauri/resources/server_common.py",
        "scripts/test_cli_native_threads.py", "src-tauri/resources/test_native_thread_budget.py",
        "scripts/test_light_whisper_cli.py", "src-tauri/resources/test_qwen3_native.py",
    )]
    receipt = {
        "jar_sha256": sha(jar),
        "inputs": {path.name: sha(path) for path in inputs},
        "source_sha256": {str(path.relative_to(root)): sha(path) for path in sources},
        "source_binding": {
            "cli": "main -> positive_threads -> request_client -> owned_engine -> EngineClient(env)",
            "sdk": "QwenNativeRuntime.__init__ -> NativeRuntime.__init__ -> _BackendConfig -> session_create",
            "implicit_callers": "both native server loaders omit threads; BaseASRServer independently sets OMP",
            "limit": "Desired decision contract; main must review current source correspondence. "
                     "DLL functions and models are synthetic in tests. No inference, GPU, OS thread-count, "
                     "installed application or arbitrary concurrent environment-writer claim.",
        },
        "bounds": {"sdk_argument_classes": 12, "environment_classes": 10,
                   "cli_argument_classes": 8, "omp_classes": 2, "backends": 2,
                   "model_kinds": 2, "entries": 3, "maximum_threads": 2147483647},
        "runs": [],
    }

    def execute(name, command):
        result = subprocess.run(command, cwd=directory, capture_output=True,
                                text=True, errors="replace", timeout=60)
        log = result.stdout + result.stderr
        (output / (name + ".log")).write_text(log, encoding="utf-8")
        return result.returncode, log

    command = [args.java, "-Xmx512m", "-cp", str(jar), "tla2sany.SANY", "NativeThreads.tla"]
    code, log = execute("sany-NativeThreads", command)
    sany_ok = code == 0 and "Semantic processing of module NativeThreads" in log \
        and not re.search(r"(?:Fatal|Semantic|Parse) errors", log, re.I)
    receipt["sany"] = {"command": command, "exit": code, "passed": sany_ok}
    if sany_ok:
        for case, invariant in CASES.items():
            command = [args.java, "-Xmx512m", "-cp", str(jar), "tlc2.TLC", "-workers", "1",
                       "-coverage", "1", "-metadir", str(output / (case + "-states")),
                       "-config", case + ".cfg", "NativeThreads.tla"]
            code, log = execute(case, command)
            expected = None if invariant is None else f"Invariant {invariant} is violated"
            passed = (code == 0 and "Model checking completed. No error has been found." in log
                      if expected is None else code != 0 and expected in log)
            counts = re.findall(r"(\d+) states generated, (\d+) distinct states found", log)
            item = {"case": case, "command": command, "exit": code, "passed": passed,
                    "expected": expected, "state_counts": counts[-1] if counts else None}
            receipt["runs"].append(item)
            print(json.dumps({key: item[key] for key in ("case", "exit", "passed", "state_counts")}))
    (output / "receipt.json").write_text(json.dumps(receipt, indent=2), encoding="utf-8")
    return 0 if sany_ok and len(receipt["runs"]) == len(CASES) \
        and all(item["passed"] for item in receipt["runs"]) else 1


if __name__ == "__main__":
    sys.exit(main())
