"""Execute the finite command-surface model, controls and reachability witnesses."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys


EXPECTED_JAR_SHA256 = "936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88"
CASES = [("CommandSurface", None)] + [
    [
        "CommandSurfaceDefaultNegative",
        "Invariant DefaultApp is violated"
    ],
    [
        "CommandSurfaceAdmissionNegative",
        "Invariant Admission is violated"
    ],
    [
        "CommandSurfaceTargetNegative",
        "Invariant RejectBeforeSpawn is violated"
    ],
    [
        "CommandSurfaceEngineValidationNegative",
        "Invariant Admission is violated"
    ],
    [
        "CommandSurfaceWireActionNegative",
        "Invariant WireActionMatches is violated"
    ],
    [
        "CommandSurfaceEngineFieldNegative",
        "Invariant EngineFieldMatches is violated"
    ],
    [
        "CommandSurfaceHotWordsNegative",
        "Invariant HotWordsAtTop is violated"
    ],
    [
        "CommandSurfaceOmittedWordsNegative",
        "Invariant OmittedHotWordsAbsent is violated"
    ],
    [
        "CommandSurfaceFatalNegative",
        "Invariant FatalTerminates is violated"
    ],
    [
        "CommandSurfaceFatalReadNegative",
        "Invariant FatalDoesNotReadTail is violated"
    ],
    [
        "CommandSurfaceFatalRetryNegative",
        "Invariant NoAutomaticRetry is violated"
    ],
    [
        "CommandSurfaceIdentityNegative",
        "Invariant ReplyIdentity is violated"
    ],
    [
        "CommandSurfaceRecoveryNegative",
        "Invariant NonFatalContinues is violated"
    ],
    [
        "CommandSurfaceFilteringNegative",
        "Invariant FatalNoiseFiltered is violated"
    ],
    [
        "CommandSurfaceReapNegative",
        "Invariant OwnedCleanup is violated"
    ],
    [
        "CommandSurfaceDoctorNegative",
        "Invariant DoctorReadOnly is violated"
    ],
    [
        "CommandSurfaceCleanupProgressNegative",
        "Temporal properties were violated"
    ]
] + [
    [
        "CommandSurfaceCommandsWitness",
        "Invariant WitnessNotReached is violated"
    ],
    [
        "CommandSurfaceDefaultWitness",
        "Invariant WitnessNotReached is violated"
    ],
    [
        "CommandSurfaceTargetWitness",
        "Invariant WitnessNotReached is violated"
    ],
    [
        "CommandSurfaceEngineValidationWitness",
        "Invariant WitnessNotReached is violated"
    ],
    [
        "CommandSurfaceEngineFieldWitness",
        "Invariant WitnessNotReached is violated"
    ],
    [
        "CommandSurfaceHotWordsWitness",
        "Invariant WitnessNotReached is violated"
    ],
    [
        "CommandSurfaceOmittedWordsWitness",
        "Invariant WitnessNotReached is violated"
    ],
    [
        "CommandSurfaceFatalWitness",
        "Invariant WitnessNotReached is violated"
    ],
    [
        "CommandSurfaceFatalSuccessWitness",
        "Invariant WitnessNotReached is violated"
    ],
    [
        "CommandSurfaceIdentityWitness",
        "Invariant WitnessNotReached is violated"
    ],
    [
        "CommandSurfaceRecoveryWitness",
        "Invariant WitnessNotReached is violated"
    ],
    [
        "CommandSurfaceFilteringWitness",
        "Invariant WitnessNotReached is violated"
    ],
    [
        "CommandSurfaceCleanupWitness",
        "Invariant WitnessNotReached is violated"
    ],
    [
        "CommandSurfaceDoctorWitness",
        "Invariant WitnessNotReached is violated"
    ]
]


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--java", type=Path, required=True)
    parser.add_argument("--jar", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args(argv)
    directory = Path(__file__).resolve().parent
    root = directory.parents[1]
    java, jar, output = args.java.resolve(), args.jar.resolve(), args.output.resolve()
    if sha256(jar) != EXPECTED_JAR_SHA256:
        parser.error("TLA+ JAR does not match the pinned SHA-256")
    output.mkdir(parents=True, exist_ok=False)
    inputs = [Path(__file__).resolve(), directory / "CommandSurface.tla", directory / "CommandSurface.md"]
    inputs += [directory / (name + ".cfg") for name, _ in CASES]
    sources = [root / "scripts/light_whisper_cli.py", root / "scripts/test_cli_commands.py"]
    receipt = {
        "jar_sha256": sha256(jar),
        "inputs": {path.name: sha256(path) for path in inputs},
        "sources": {str(path.relative_to(root)): sha256(path) for path in sources},
        "binding": "CommandSurface.md; desired Python contract, final production correspondence remains main-owned",
        "bounds": {
            "profiles": 5, "surfaces": 2, "requested_targets": 3,
            "desktop_actions": 6, "local_engines": 2,
            "invalid_engine_classes": 2, "hot_word_shapes": 2,
            "response_partitions": 7, "external_id_representatives": 2,
            "records_per_session": 2, "workers": 1, "heap_mib": 512,
            "fairness": "WF(Advance), synchronous child-response/cleanup assumption",
            "reductions": "none",
        },
        "runs": [],
    }

    def execute(name, command):
        try:
            result = subprocess.run(command, cwd=directory, capture_output=True,
                                    text=True, encoding="utf-8", errors="replace", timeout=60)
            code, log = result.returncode, result.stdout + result.stderr
        except subprocess.TimeoutExpired as error:
            def decode(value):
                return value.decode("utf-8", "replace") if isinstance(value, bytes) else value or ""
            code, log = None, decode(error.stdout) + decode(error.stderr) + "\nRUN TIMEOUT: incomplete"
        (output / (name + ".log")).write_text(log, encoding="utf-8")
        return code, log

    command = [str(java), "-Xmx512m", "-cp", str(jar), "tla2sany.SANY", "CommandSurface.tla"]
    code, log = execute("sany-CommandSurface", command)
    sany_ok = code == 0 and "Semantic processing of module CommandSurface" in log and not re.search(
        r"(?:Fatal|Semantic|Parse) errors|Parsing or semantic analysis failed", log, re.I,
    )
    receipt["sany"] = {"command": command, "exit": code, "passed": sany_ok}
    if sany_ok:
        for name, expected in CASES:
            command = [str(java), "-Xmx512m", "-cp", str(jar), "tlc2.TLC", "-workers", "1",
                       "-coverage", "1", "-metadir", str(output / (name + "-states")),
                       "-config", name + ".cfg", "CommandSurface.tla"]
            code, log = execute(name, command)
            if expected is None:
                passed = code == 0 and "Model checking completed. No error has been found." in log
                kind = "positive"
            else:
                passed = code is not None and code != 0 and expected in log
                kind = "witness" if name.endswith("Witness") else "negative"
            counts = re.findall(r"([\d,]+) states generated, ([\d,]+) distinct states found", log)
            item = {"case": name, "kind": kind, "command": command, "exit": code,
                    "passed": passed, "expected": expected,
                    "state_counts": counts[-1] if counts else None}
            receipt["runs"].append(item)
            print(json.dumps({key: item[key] for key in ("case", "kind", "exit", "passed", "state_counts")}),
                  flush=True)
    complete = sany_ok and len(receipt["runs"]) == len(CASES) and all(
        item["passed"] for item in receipt["runs"]
    )
    receipt["passed"] = complete
    (output / "receipt.json").write_text(json.dumps(receipt, indent=2), encoding="utf-8")
    return 0 if complete else 1


if __name__ == "__main__":
    sys.exit(main())
