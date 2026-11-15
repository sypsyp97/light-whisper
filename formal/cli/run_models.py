import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys


CASES = {
    "EngineClient": None,
    "CorrelationNegative": "Invariant OnlyCorrelated is violated",
    "StartupNegative": "Invariant ReadyRequiresStartup is violated",
    "ReapNegative": "Invariant ReapedMeansReleased is violated",
    "KillProgressNegative": "Temporal properties were violated",
    "ReapProgressNegative": "Temporal properties were violated",
    "CorrelationWitness": "Invariant StaleThenTwoResponsesNotReached is violated",
    "TimeoutWitness": "Invariant TimeoutReapedNotReached is violated",
    "StartupWitness": "Invariant RejectedStartupReapedNotReached is violated",
    "ReaderSetupNegative": "Invariant FaultRetainsOwnership is violated",
    "CleanupFailureNegative": "Invariant FaultRetainsOwnership is violated",
    "UnstartedJoinNegative": "Invariant NoUnstartedJoin is violated",
    "ReaderFailureWitness": "Invariant ReaderFailureReapedNotReached is violated",
    "CleanupFailureWitness": "Invariant CleanupFailureReapedNotReached is violated",
}
HANDOFF_CASES = {
    "SenderHandoff": None,
    "SenderHandoffNegative": "Invariant NoPrematureHandoff is violated",
    "SenderOwnershipNegative": "Invariant NoLostSenderSlot is violated",
    "SenderDeadlineProgressNegative": "Temporal properties were violated",
    "SenderBudgetNegative": "Invariant NoExtraTailBudget is violated",
    "SenderHandoffWitness": "Invariant TailHandoffNotReached is violated",
    "SenderTailTimeoutWitness": "Invariant TailTimeoutSkipReapedNotReached is violated",
    "SenderBlockedTimeoutWitness": "Invariant BlockedTimeoutSkipReapedNotReached is violated",
}
CASES.update(HANDOFF_CASES)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--jar", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    directory = Path(__file__).resolve().parent
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    jar = args.jar.resolve()
    sha = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
    receipt = {"jar_sha256": sha(jar), "inputs": {
        path.name: sha(path) for path in sorted(directory.iterdir())
        if path.suffix in {".tla", ".cfg"}}, "runs": []}

    def execute(name, command):
        result = subprocess.run(command, cwd=directory, capture_output=True,
                                text=True, errors="replace", timeout=60)
        log = result.stdout + result.stderr
        (output / (name + ".log")).write_text(log, encoding="utf-8")
        return result.returncode, log

    receipt["sany"] = []
    for module in ("EngineClient", "SenderHandoff"):
        sany_command = ["java", "-Xmx512m", "-cp", str(jar), "tla2sany.SANY", module + ".tla"]
        code, log = execute("sany-" + module, sany_command)
        passed = code == 0 and f"Semantic processing of module {module}" in log \
            and not re.search(r"(?:Fatal|Semantic|Parse) errors", log, re.I)
        receipt["sany"].append({"module": module, "command": sany_command,
                                "exit": code, "passed": passed})
    sany_ok = all(item["passed"] for item in receipt["sany"])
    if sany_ok:
        for case, expected in CASES.items():
            module = "SenderHandoff" if case in HANDOFF_CASES else "EngineClient"
            command = ["java", "-Xmx512m", "-cp", str(jar), "tlc2.TLC", "-workers", "1",
                       "-coverage", "1", "-metadir", str(output / (case + "-states")),
                       "-config", case + ".cfg", module + ".tla"]
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
