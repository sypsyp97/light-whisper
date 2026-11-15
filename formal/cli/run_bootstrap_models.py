"""Finite CLI bootstrap/cache/capability checks; no packaging or native execution proof."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys


CASES = {
    "Bootstrap": None,
    "BootstrapFrozenNegative": "Invariant FrozenResources is violated",
    "BootstrapMetadataNegative": "Invariant ResourceMetadata is violated",
    "BootstrapNeighborNegative": "Invariant AppPrecedence is violated",
    "BootstrapSourceNeighborNegative": "Invariant AppPrecedence is violated",
    "BootstrapExplicitAppNegative": "Invariant AppPrecedence is violated",
    "BootstrapSkipUnmarkedNegative": "Invariant AppPrecedence is violated",
    "BootstrapMarkerNegative": "Invariant MarkerRequired is violated",
    "BootstrapCapabilityNegative": "Invariant CapabilityTruth is violated",
    "BootstrapPartialSourceNegative": "Invariant RuntimePrecedence is violated",
    "BootstrapInstalledRuntimeNegative": "Invariant RuntimePrecedence is violated",
    "BootstrapExplicitRuntimeNegative": "Invariant RuntimePrecedence is violated",
    "BootstrapWrapperNegative": "Invariant RuntimeWrapper is violated",
    "BootstrapRuntimeGuardNegative": "Invariant RuntimeLaunchGuard is violated",
    "BootstrapCustomCacheNegative": "Invariant CachePrecedence is violated",
    "BootstrapEmptyCustomNegative": "Invariant CachePrecedence is violated",
    "BootstrapNonstringCustomNegative": "Invariant CachePrecedence is violated",
    "BootstrapEmptyHFNegative": "Invariant CachePrecedence is violated",
    "BootstrapCacheMetadataNegative": "Invariant CacheMetadata is violated",
    "BootstrapEngineCacheNegative": "Invariant CacheEnvironment is violated",
    "BootstrapAppCacheNegative": "Invariant CacheEnvironment is violated",
    "BootstrapResolvedCacheNegative": "Invariant ResolvedCache is violated",
    "BootstrapFrozenWitness": "Invariant WitnessNotReached is violated",
    "BootstrapNeighborWitness": "Invariant WitnessNotReached is violated",
    "BootstrapUnsupportedWitness": "Invariant WitnessNotReached is violated",
    "BootstrapInstalledAppWitness": "Invariant WitnessNotReached is violated",
    "BootstrapInstalledRuntimeWitness": "Invariant WitnessNotReached is violated",
    "BootstrapInvalidExplicitWitness": "Invariant WitnessNotReached is violated",
    "BootstrapEmptyHFWitness": "Invariant WitnessNotReached is violated",
    "BootstrapAppCacheWitness": "Invariant WitnessNotReached is violated",
    "BootstrapUTF8Negative": "Invariant UTF8InputPreserved is violated",
    "BootstrapUTF8Witness": "Invariant WitnessNotReached is violated",
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
    inputs = [directory / "Bootstrap.tla", Path(__file__).resolve()]
    inputs.extend(directory / (name + ".cfg") for name in CASES)
    sources = [root / name for name in (
        "scripts/light_whisper_cli.py", "scripts/test_cli_bootstrap.py",
        "scripts/test_cli_target.py", "src-tauri/src/utils/paths.rs")]
    receipt = {
        "jar_sha256": sha(jar),
        "inputs": {path.name: sha(path) for path in inputs},
        "source_sha256": {str(path.relative_to(root)): sha(path) for path in sources},
        "source_binding": {
            "resources": "ROOT initialization -> doctor resource_root/version; "
                         "repository is source root in source mode, null when frozen",
            "app": "app_path first regular file -> marker capability -> app_command; doctor reports capability",
            "runtime": "engine_command/doctor explicit > complete source pair > config_dir/engine/engine.exe",
            "cache": "resolve_engine_selection reads one config; paths.rs434-475 precedence; "
                     "owned_engine overrides HF_HUB_CACHE; request_client app preserves environment",
            "encoding": "real __main__/session stdin must decode UTF8 before session_request validation",
            "limit": "Desired Python bootstrap contract. Source correspondence requires main review. "
                     "File/marker predicates abstract scanning. No frozen build, installer or native proof.",
        },
        "bounds": {"resource_inputs": 2, "app_inputs": 128, "runtime_inputs": 40,
                   "cache_inputs": 24, "encoding_inputs": 2, "config_observations": 1},
        "runs": [],
    }

    def execute(name, command):
        result = subprocess.run(command, cwd=directory, capture_output=True,
                                text=True, errors="replace", timeout=60)
        log = result.stdout + result.stderr
        (output / (name + ".log")).write_text(log, encoding="utf-8")
        return result.returncode, log

    command = ["java", "-Xmx512m", "-cp", str(jar), "tla2sany.SANY", "Bootstrap.tla"]
    code, log = execute("sany-Bootstrap", command)
    sany_ok = code == 0 and "Semantic processing of module Bootstrap" in log \
        and not re.search(r"(?:Fatal|Semantic|Parse) errors", log, re.I)
    receipt["sany"] = {"command": command, "exit": code, "passed": sany_ok}
    if sany_ok:
        for case, expected in CASES.items():
            command = ["java", "-Xmx512m", "-cp", str(jar), "tlc2.TLC", "-workers", "1",
                       "-coverage", "1", "-metadir", str(output / (case + "-states")),
                       "-config", case + ".cfg", "Bootstrap.tla"]
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
