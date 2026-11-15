"""Run the finite private-file model; negative controls must fail semantically."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import time


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--jar", type=Path, required=True)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    here = Path(__file__).resolve().parent
    output = args.output or here.parents[1] / "formal" / "results" / "agent-api-security" / str(time.time_ns())
    output.mkdir(parents=True, exist_ok=False)
    jar = args.jar.resolve(strict=True)
    base = ["java", "-Xmx512m", "-cp", str(jar)]
    preflight = subprocess.run(base + ["tla2sany.SANY", "PrivateFiles.tla"], cwd=here,
                               text=True, capture_output=True, timeout=45)
    sany_log = preflight.stdout + preflight.stderr
    (output / "sany.log").write_text(sany_log, encoding="utf-8")
    if preflight.returncode or "Semantic errors:" in sany_log or "Parse Error" in sany_log:
        raise RuntimeError("SANY failed; see " + str(output / "sany.log"))
    cases = json.loads((here / "matrix.json").read_text(encoding="utf-8"))
    results = []
    for case in cases:
        case_dir = output / case["name"]
        command = base + ["tlc2.TLC", "-workers", "1", "-coverage", "1", "-metadir",
                          str(case_dir), "-config", str(here / case["config"]), "PrivateFiles.tla"]
        started = time.monotonic()
        run = subprocess.run(command, cwd=here, text=True, capture_output=True, timeout=45)
        log = run.stdout + run.stderr
        (output / (case["name"] + ".log")).write_text(log, encoding="utf-8")
        if case["kind"] == "positive":
            passed = run.returncode == 0 and "Model checking completed. No error has been found." in log
        elif case["kind"] == "temporal-negative":
            passed = run.returncode != 0 and "Temporal properties were violated." in log and 'phase = "failed"' in log
        else:
            passed = run.returncode != 0 and f'Invariant {case["property"]} is violated.' in log
        counts = re.search(r"([\d,]+) states generated, ([\d,]+) distinct states found", log)
        depth = re.search(r"depth of the complete state graph search is (\d+)", log)
        results.append({**case, "passed": passed, "exit_code": run.returncode,
                        "elapsed_seconds": round(time.monotonic() - started, 3), "command": command,
                        "generated_states": int(counts[1].replace(",", "")) if counts else None,
                        "distinct_states": int(counts[2].replace(",", "")) if counts else None,
                        "depth": int(depth[1]) if depth else None})
        print(case["name"], "EXPECTED" if passed else "UNEXPECTED", flush=True)
    hashes = {}
    for path in [here / "PrivateFiles.tla", here / "matrix.json", jar,
                 here.parents[1] / "src-tauri" / "src" / "agent_api" / "secure_files.rs"]:
        hashes[str(path)] = hashlib.sha256(path.read_bytes()).hexdigest()
    for path in sorted((here / "configs").glob("*.cfg")):
        hashes[str(path)] = hashlib.sha256(path.read_bytes()).hexdigest()
    for path in [here / "check_model.py", here / "private_measurement.py", here / "Cargo.toml", here / "Cargo.lock",
                 here / "tests" / "private_files.rs", here / "tests" / "support" / "mod.rs",
                 here / "src" / "bin" / "private_file_probe.rs"]:
        hashes[str(path)] = hashlib.sha256(path.read_bytes()).hexdigest()
    receipt = {"bounds": {"hosts": 2, "path_generations": 2, "publications": 1,
                           "initial_destination_cases": ["existing", "absent"],
                           "file_identity_representatives": ["absent", "old", "staged", "copied"]},
               "hashes": hashes, "sany_exit_code": preflight.returncode, "results": results}
    (output / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    print(output)
    return 0 if all(result["passed"] for result in results) else 1


if __name__ == "__main__":
    raise SystemExit(main())
