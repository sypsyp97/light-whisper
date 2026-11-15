"""Check only the CLI desktop-configuration finite model and its controls."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys


CASES = {
    "Configuration": None,
    "ConfigurationSavedNegative": "SelectionPrecedence",
    "ConfigurationArgumentNegative": "SelectionPrecedence",
    "ConfigurationStructureNegative": "InvalidConfigurationBlocks",
    "ConfigurationUnknownNegative": "InvalidConfigurationBlocks",
    "ConfigurationCloudNegative": "OperationalLocalGuard",
    "ConfigurationDirectoryNegative": "DesktopDirectoryOnly",
    "ConfigurationPayloadNegative": "SafeDiagnostics",
    "ConfigurationLocalSupportNegative": "LocalSelectionWorks",
    "ConfigurationDoctorSpawnNegative": "LocalSelectionWorks",
    "ConfigurationLegacyWitness": "WitnessNotReached",
    "ConfigurationCloudBlockedWitness": "WitnessNotReached",
    "ConfigurationCloudDoctorWitness": "WitnessNotReached",
    "ConfigurationMalformedOverrideWitness": "WitnessNotReached",
    "ConfigurationUnknownOverrideWitness": "WitnessNotReached",
    "ConfigurationDirectoryWitness": "WitnessNotReached",
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
    inputs = [directory / "Configuration.tla", Path(__file__).resolve()]
    inputs.extend(directory / (name + ".cfg") for name in CASES)
    sources = [root / "scripts/light_whisper_cli.py", root / "src-tauri/src/utils/paths.rs"]
    receipt = {
        "jar_sha256": sha(jar),
        "inputs": {path.name: sha(path) for path in inputs},
        "source_sha256": {str(path.relative_to(root)): sha(path) for path in sources},
        "source_binding": {
            "desktop": "paths.rs get_data_dir -> get_engine_config_path -> configured_engine",
            "cli": "main -> parser/doctor/engine_command; resolve configuration before dispatch",
            "limit": "Desired contract model; source correspondence requires independent review. "
                     "The baseline hardcoded-Qwen CLI does not implement this model.",
        },
        "bounds": {"saved_classes": 10, "arguments": 3, "commands": 3,
                   "desktop_roots": 2, "runtime_config_engines": 2},
        "runs": [],
    }

    def execute(name, command):
        result = subprocess.run(command, cwd=directory, capture_output=True,
                                text=True, errors="replace", timeout=60)
        log = result.stdout + result.stderr
        (output / (name + ".log")).write_text(log, encoding="utf-8")
        return result.returncode, log

    command = ["java", "-Xmx512m", "-cp", str(jar), "tla2sany.SANY", "Configuration.tla"]
    code, log = execute("sany-Configuration", command)
    sany_ok = code == 0 and "Semantic processing of module Configuration" in log \
        and not re.search(r"(?:Fatal|Semantic|Parse) errors", log, re.I)
    receipt["sany"] = {"command": command, "exit": code, "passed": sany_ok}
    if sany_ok:
        for case, invariant in CASES.items():
            command = ["java", "-Xmx512m", "-cp", str(jar), "tlc2.TLC", "-workers", "1",
                       "-coverage", "1", "-metadir", str(output / (case + "-states")),
                       "-config", case + ".cfg", "Configuration.tla"]
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
