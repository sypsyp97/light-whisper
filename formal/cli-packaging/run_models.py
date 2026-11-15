"""Bounded CLI artifact lifecycle/selector checks; never native execution proof."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess


JAR_SHA256 = "936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88"
CASES = {
    "Packaging": None,
    "VerifyNegative": "Invariant OnlyVerifiedPublication is violated",
    "InputsNegative": "Invariant OriginalInputsRequired is violated",
    "EarlyPublishNegative": "Invariant OnlyVerifiedPublication is violated",
    "UnrelatedNegative": "Invariant UnrelatedSurvives is violated",
    "CleanupNegative": "Invariant SettledOwnStageReleased is violated",
    "ReleaseNegative": "Invariant ReleaseRequiresNonempty is violated",
    "DebugOverwriteNegative": "Invariant ExistingArtifactPreserved is violated",
    "ProgressNegative": "Temporal properties were violated",
    **{name + "Witness": "Invariant WitnessNotReached is violated"
       for name in ("Commit", "Rollback", "Stale", "Debug", "Release")},
    "MetadataNegative": "Invariant MetadataFailurePreserved is violated",
    "MetadataWitness": "Invariant WitnessNotReached is violated",
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--java", type=Path, required=True)
    parser.add_argument("--jar", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    directory = Path(__file__).resolve().parent
    root = directory.parents[1]
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    sha = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
    if sha(args.jar) != JAR_SHA256:
        parser.error("TLA+ JAR digest does not match the reviewed tool")
    inputs = [directory / "Packaging.tla", Path(__file__).resolve()]
    inputs.extend(directory / (name + ".cfg") for name in CASES)
    receipt = {
        "jar_sha256": sha(args.jar),
        "inputs": {path.name: sha(path) for path in inputs},
        "source_sha256": {name: sha(root / name) for name in (
            "scripts/build_cli.py", "scripts/test_build_cli.py", "src-tauri/build_support.rs",
            "src-tauri/build.rs", "src-tauri/tauri.conf.json", "package.json", "scripts/release.sh")},
        "bounds": {"builds": 1, "input_revisions": 2, "selector_modes": 2,
                   "initial_artifacts": 5, "concurrent_builders": 1},
        "assumptions": ["single build owner", "atomic os.replace", "source changes stop at commit",
                        "filesystem stage cleanup succeeds", "weak fairness for finite local steps"],
        "runs": [],
    }

    def execute(name, command):
        result = subprocess.run(command, cwd=directory, capture_output=True,
                                text=True, errors="replace", timeout=60)
        log = result.stdout + result.stderr
        (output / (name + ".log")).write_text(log, encoding="utf-8")
        return result.returncode, log

    command = [str(args.java), "-Xmx512m", "-cp", str(args.jar), "tla2sany.SANY", "Packaging.tla"]
    code, log = execute("sany", command)
    sany_ok = (code == 0 and "Semantic processing of module Packaging" in log
               and not re.search(r"(?:Fatal|Semantic|Parse) errors", log, re.I))
    receipt["sany"] = {"command": command, "exit": code, "passed": sany_ok}
    if sany_ok:
        for case, expected in CASES.items():
            command = [str(args.java), "-Xmx512m", "-cp", str(args.jar), "tlc2.TLC", "-workers", "1",
                       "-coverage", "1", "-metadir", str(output / (case + "-states")),
                       "-config", case + ".cfg", "Packaging.tla"]
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
    raise SystemExit(main())
