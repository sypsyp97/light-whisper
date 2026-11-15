"""Run the bounded CLI session model, semantic controls and reachability matrix."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys


CASES = {
    "Session": None,
    "SessionEagerNegative": "LazyAcquisition",
    "SessionOwnerNegative": "SingleOwner",
    "SessionInputNegative": "InputGuard",
    "SessionWireNegative": "WireMonotonic",
    "SessionIdentityNegative": "ClientIdentity",
    "SessionDuplicateExitNegative": "RepliesPerLine",
    "SessionBlankNegative": "BlankIgnored",
    "SessionTransportNegative": "NoPostFailureDispatch",
    "SessionStatsNegative": "StatsRetained",
    "SessionInvalidStopNegative": "RecoverableFailures",
    "SessionBackendStopNegative": "RecoverableFailures",
    "SessionExitAckNegative": "ExitAcknowledged",
    "SessionReapNegative": "CleanExit",
    "SessionCleanupProgressNegative": "temporal",
    "SessionEarlyExitAckNegative": "ExitAckAfterReap",
    "SessionCleanupReportNegative": "CleanupFailureReported",
    "SessionEOFReplyNegative": "EOFNoExtraReply",
    "SessionInterruptNegative": "InterruptResult",
    "SessionPendingClearNegative": "RepliesPerLine",
    "SessionReuseWitness": "WitnessNotReached",
    "SessionRecoveryWitness": "WitnessNotReached",
    "SessionBackendFalseWitness": "WitnessNotReached",
    "SessionTransportWitness": "WitnessNotReached",
    "SessionStartupWitness": "WitnessNotReached",
    "SessionEmptyEOFWitness": "WitnessNotReached",
    "SessionExitWithoutEngineWitness": "WitnessNotReached",
    "SessionStatsWitness": "WitnessNotReached",
    "SessionExitAfterReapWitness": "WitnessNotReached",
    "SessionCleanupExitWitness": "WitnessNotReached",
    "SessionCleanupEOFWitness": "WitnessNotReached",
    "SessionInterruptReadWitness": "WitnessNotReached",
    "SessionInterruptWaitWitness": "WitnessNotReached",
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
    inputs = [directory / "Session.tla", Path(__file__).resolve()]
    inputs.extend(directory / (case + ".cfg") for case in CASES)
    receipt = {
        "jar_sha256": sha(jar),
        "inputs": {path.name: sha(path) for path in inputs},
        "source_sha256": {"scripts/light_whisper_cli.py": sha(root / "scripts/light_whisper_cli.py")},
        "test_sha256": {"scripts/test_cli_session.py": sha(root / "scripts/test_cli_session.py")},
        "source_binding": {
            "entry": "main -> run_session -> per-line session_request -> lazy owned_engine/EngineClient -> serial request -> correlated JSON",
            "terminal_order": "run_session resources.close/ExitStack unwinding -> ExitReply or FailureReply; EOF cleanup failure has stderr only without pending_id",
            "existing_transport": "EngineClient.__enter__/request/_receive/__exit__; detailed protocol modeled separately",
            "limit": "CleanupFailure abstracts reporting after resource release settles; failures preventing reaping and hard-kill containment remain in separate transport models. Independent source correspondence review required for this source hash.",
        },
        "bounds": {"input_records_max": 3, "engine_operations_max": 2,
                   "client_ids": [41, -7, 41], "event_classes": 7},
        "assumption": "Weak fairness of enabled settling steps; outcomes may be backend false, timeout or EOF.",
        "runs": [],
    }

    def execute(name, command):
        completed = subprocess.run(command, cwd=directory, capture_output=True,
                                   text=True, errors="replace", timeout=60)
        log = completed.stdout + completed.stderr
        (output / (name + ".log")).write_text(log, encoding="utf-8")
        return completed.returncode, log

    command = ["java", "-Xmx512m", "-cp", str(jar), "tla2sany.SANY", "Session.tla"]
    code, log = execute("sany-Session", command)
    sany_ok = code == 0 and "Semantic processing of module Session" in log \
        and not re.search(r"(?:Fatal|Semantic|Parse) errors", log, re.I)
    receipt["sany"] = {"command": command, "exit": code, "passed": sany_ok}
    if sany_ok:
        for case, invariant in CASES.items():
            command = ["java", "-Xmx512m", "-cp", str(jar), "tlc2.TLC", "-workers", "1",
                       "-coverage", "1", "-metadir", str(output / (case + "-states")),
                       "-config", case + ".cfg", "Session.tla"]
            code, log = execute(case, command)
            expected = (None if invariant is None else "Temporal properties were violated"
                        if invariant == "temporal" else f"Invariant {invariant} is violated")
            passed = (code == 0 and "Model checking completed. No error has been found." in log
                      if expected is None else code != 0 and expected in log)
            counts = re.findall(r"(\d+) states generated, (\d+) distinct states found", log)
            depth = re.findall(r"The depth of the complete state graph search is (\d+)", log)
            item = {"case": case, "command": command, "exit": code, "passed": passed,
                    "expected": expected, "state_counts": counts[-1] if counts else None,
                    "depth": depth[-1] if depth else None}
            receipt["runs"].append(item)
            print(json.dumps({key: item[key] for key in ("case", "exit", "passed", "state_counts")}))
    (output / "receipt.json").write_text(json.dumps(receipt, indent=2), encoding="utf-8")
    return 0 if sany_ok and len(receipt["runs"]) == len(CASES) \
        and all(item["passed"] for item in receipt["runs"]) else 1


if __name__ == "__main__":
    sys.exit(main())
