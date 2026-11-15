"""Finite Python app/engine target-routing checks; no native Rust proof."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys


CASES = {
    "TargetRouting": None,
    "TargetDefaultNegative": "Invariant TargetPrecedence is violated",
    "TargetConflictNegative": "Invariant ConflictBlocks is violated",
    "TargetAppRouteNegative": "Invariant BrokerRouting is violated",
    "TargetEngineRouteNegative": "Invariant EngineRouting is violated",
    "TargetEnvironmentNegative": "Invariant EnvironmentBoundary is violated",
    "TargetDataNegative": "Invariant DataBoundary is violated",
    "TargetDoctorNegative": "Invariant DoctorReadOnly is violated",
    "TargetMissingFileNegative": "Invariant ExistingAppRequired is violated",
    "TargetPriorityNegative": "Invariant CandidatePriority is violated",
    "TargetExplicitMissingNegative": "Invariant ExistingAppRequired is violated",
    "TargetReapNegative": "Invariant OwnedResourceCleanup is violated",
    "TargetDesktopNegative": "Invariant DesktopSurvives is violated",
    "TargetCleanupProgressNegative": "Temporal properties were violated",
    "TargetAppSessionWitness": "Invariant WitnessNotReached is violated",
    "TargetEngineWitness": "Invariant WitnessNotReached is violated",
    "TargetConflictWitness": "Invariant WitnessNotReached is violated",
    "TargetStartupFailureWitness": "Invariant WitnessNotReached is violated",
    "TargetInstalledWitness": "Invariant WitnessNotReached is violated",
    "TargetExplicitMissingWitness": "Invariant WitnessNotReached is violated",
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--jar", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    directory = Path(__file__).resolve().parent
    root = directory.parents[1]
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    jar = args.jar.resolve()
    sha = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
    inputs = [directory / "TargetRouting.tla", Path(__file__).resolve()]
    inputs.extend(directory / (name + ".cfg") for name in CASES)
    sources = [root / "scripts/light_whisper_cli.py", root / "scripts/test_cli_target.py"]
    receipt = {
        "jar_sha256": sha(jar),
        "inputs": {path.name: sha(path) for path in inputs},
        "source_sha256": {str(path.relative_to(root)): sha(path) for path in sources},
        "source_binding": {
            "target": "parser/main -> resolve_target -> doctor or app/engine ownership",
            "routing": "app_command/engine_command -> EngineClient startup/request/cleanup",
            "session": "run_session lazily retains one transport; exit closes transport only",
            "limit": "Desired Python contract. Baseline contains uncalled declarations. "
                     "Main must review final source correspondence. Native server not modeled.",
        },
        "bounds": {"explicit_targets": 3, "independent_option_kinds": 5,
                   "app_exe": 2, "commands": 3, "explicit_is_file": 2,
                   "available_auto_candidate_subsets": 8, "startup_outcomes": 2,
                   "session_requests": 2},
        "runs": [],
    }

    def execute(name, command):
        result = subprocess.run(command, cwd=directory, capture_output=True,
                                text=True, errors="replace", timeout=60)
        log = result.stdout + result.stderr
        (output / (name + ".log")).write_text(log, encoding="utf-8")
        return result.returncode, log

    command = ["java", "-Xmx512m", "-cp", str(jar), "tla2sany.SANY", "TargetRouting.tla"]
    code, log = execute("sany-TargetRouting", command)
    sany_ok = code == 0 and "Semantic processing of module TargetRouting" in log \
        and not re.search(r"(?:Fatal|Semantic|Parse) errors", log, re.I)
    receipt["sany"] = {"command": command, "exit": code, "passed": sany_ok}
    if sany_ok:
        for case, expected in CASES.items():
            command = ["java", "-Xmx512m", "-cp", str(jar), "tlc2.TLC", "-workers", "1",
                       "-coverage", "1", "-metadir", str(output / (case + "-states")),
                       "-config", case + ".cfg", "TargetRouting.tla"]
            code, log = execute(case, command)
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
