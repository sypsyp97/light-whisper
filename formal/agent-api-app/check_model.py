"""Check the finite query contract; require named semantic controls, not any error."""
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
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--only", action="append")
    args = parser.parse_args()
    here = Path(__file__).resolve().parent
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    jar = args.jar.resolve(strict=True)
    java = ["java", "-Xmx512m", "-cp", str(jar)]
    preflight = subprocess.run(java + ["tla2sany.SANY", "AppQuery.tla"], cwd=here,
                               text=True, capture_output=True, timeout=45)
    log = preflight.stdout + preflight.stderr
    (output / "sany.log").write_text(log, encoding="utf-8")
    if preflight.returncode or any(error in log for error in ("Semantic errors:", "Parse Error", "Fatal errors:")):
        raise RuntimeError("SANY failed: " + str(output / "sany.log"))
    matrix = json.loads((here / "matrix.json").read_text(encoding="utf-8"))
    selected = [case for case in matrix if not args.only or case["name"] in args.only]
    if not selected or (args.only and set(args.only) != {case["name"] for case in selected}):
        raise ValueError("--only must name existing matrix cases")
    results = []
    for case in selected:
        command = java + ["tlc2.TLC", "-workers", "1", "-coverage", "1", "-metadir",
                          str(output / (case["name"] + "-states")), "-config", case["config"], "AppQuery.tla"]
        start = time.monotonic()
        run = subprocess.run(command, cwd=here, text=True, capture_output=True, timeout=90)
        log = run.stdout + run.stderr
        (output / (case["name"] + ".log")).write_text(log, encoding="utf-8")
        if case["kind"] == "positive":
            passed = run.returncode == 0 and "Model checking completed. No error has been found." in log
        elif case["kind"] == "temporal-negative":
            passed = run.returncode != 0 and "Temporal properties were violated." in log
            passed = passed and bool(re.search(r'abortIpc \|-> \{[12](?:,\s*[12])?\}', log))
            passed = passed and 'child |-> "stopping"' in log
        else:
            passed = run.returncode != 0 and f'Invariant {case["expected"]} is violated.' in log
        counts = re.search(r"([\d,]+) states generated, ([\d,]+) distinct states found", log)
        depth = re.search(r"depth of the complete state graph search is (\d+)", log)
        results.append({**case, "passed": bool(passed), "exit_code": run.returncode,
            "command": command, "elapsed_seconds": round(time.monotonic() - start, 3),
            "generated_states": int(counts[1].replace(",", "")) if counts else None,
            "distinct_states": int(counts[2].replace(",", "")) if counts else None,
            "depth": int(depth[1]) if depth else None})
        print(case["name"], "EXPECTED" if passed else "UNEXPECTED", flush=True)
    repo = here.parents[1]
    paths = [here / "AppQuery.tla", here / "check_model.py", here / "matrix.json", jar,
        repo / "src-tauri/src/agent_api/controller.rs", repo / "src-tauri/src/agent_api/tests.rs",
        repo / "src-tauri/src/services/funasr_service.rs", repo / "src-tauri/src/services/funasr_service/protocol.rs",
        repo / "src-tauri/src/state/app_state.rs", repo / "src-tauri/src/utils/paths.rs"]
    paths += sorted((here / "configs").glob("*.cfg"))
    receipt = {"bounds": {"clients": 2, "native_owners": 1, "external_owner_representatives": 1,
        "requests_per_client": 1, "byte_classes": ["valid", "missing", "oversize", "cloud"],
        "transcript_classes": ["present_string", "explicit_empty", "missing_or_null", "wrong_type_ipc_error"]},
        "assumptions": ["enabled local steps eventually execute", "owned child OS termination eventually reaps"],
        "baseline": "9a5b94b6bdda88e689224519e5061c26de55c558",
        "sany_exit_code": preflight.returncode,
        "hashes": {str(path): hashlib.sha256(path.read_bytes()).hexdigest() for path in paths},
        "results": results}
    (output / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    print(output)
    return 0 if all(case["passed"] for case in results) else 1


if __name__ == "__main__":
    raise SystemExit(main())
