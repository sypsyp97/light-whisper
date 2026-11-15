"""Exhaustively check the release decision and named guard sensitivity controls."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import time


JAR_SHA256 = "936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--java", default="java")
    parser.add_argument("--jar", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    here = Path(__file__).resolve().parent
    root = here.parents[1]
    jar = args.jar.resolve(strict=True)
    if hashlib.sha256(jar.read_bytes()).hexdigest() != JAR_SHA256:
        raise ValueError("Unexpected TLA+ tool digest")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    matrix = json.loads((here / "matrix.json").read_text(encoding="utf-8"))
    base = [args.java, "-Xmx512m", "-cp", str(jar)]

    def execute(name, command):
        started = time.monotonic()
        result = subprocess.run(command, cwd=here, capture_output=True,
                                text=True, errors="replace", timeout=90)
        log = result.stdout + result.stderr
        (output / (name + ".log")).write_text(log, encoding="utf-8")
        return result.returncode, log, round(time.monotonic() - started, 3)

    command = base + ["tla2sany.SANY", "ReleaseGate.tla"]
    code, log, elapsed = execute("sany", command)
    sany = {"command": command, "exit": code, "elapsed_seconds": elapsed,
            "passed": code == 0 and "Semantic processing of module ReleaseGate" in log
            and not re.search(r"(?:Fatal|Semantic|Parse) errors|Parse Error", log, re.I)}
    results = []
    if sany["passed"]:
        for case in matrix:
            command = base + ["tlc2.TLC", "-workers", "1", "-coverage", "1", "-metadir",
                              str(output / (case["name"] + "-states")), "-config",
                              case["name"] + ".cfg", "ReleaseGate.tla"]
            code, log, elapsed = execute(case["name"], command)
            expected_code = {"positive": 0, "negative": 12, "witness": 12, "temporal-negative": 13}[case["kind"]]
            expected = ("Model checking completed. No error has been found." if case["kind"] == "positive"
                        else "Temporal properties were violated." if case["kind"] == "temporal-negative"
                        else f'Invariant {case["property"]} is violated.')
            counts = re.findall(r"([\d,]+) states generated, ([\d,]+) distinct states found", log)
            depth = re.findall(r"depth of the complete state graph search is (\d+)", log)
            result = {**case, "command": command, "exit": code, "expected_exit": expected_code,
                      "expected": expected, "passed": code == expected_code and expected in log,
                      "elapsed_seconds": elapsed, "state_counts": counts[-1] if counts else None,
                      "depth": depth[-1] if depth else None}
            results.append(result)
            print(case["name"], "EXPECTED" if result["passed"] else "UNEXPECTED", flush=True)
    inputs = [path for path in here.rglob("*") if path.is_file()
              and path.suffix in {".tla", ".cfg", ".py", ".json", ".md"}]
    inputs += [root / path for path in ("scripts/release.sh", ".github/workflows/ci.yml",
                                        "scripts/test_release_ci.py", "scripts/test_cli_ci.py")]
    receipt = {"bounds": {"candidate_identities": 2, "existing_required_jobs": 5,
                           "existing_jobs_abstraction": "conjunction of five successes",
                           "cli_terminal_statuses": 5, "during_fetch_drift_events": 1,
                           "heap_mib": 512, "workers": 1},
               "assumptions": ["CI eventually returns a terminal observation under ResolveCI fairness",
                               "shell is fail-fast and each invoked external command eventually returns",
                               "Exclusive publisher; no external writers from final post-fetch observations through tag creation",
                               "One optional HEAD/remote/dirty change during fetch; initial publication state is the candidate and clean",
                               "five legacy jobs are symmetric conjuncts; shell tests exercise each name"],
               "source_correspondence": "OPEN until main reviews and reruns against implemented six-job gates",
               "jar_sha256": JAR_SHA256,
               "hashes": {path.relative_to(root).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
                          for path in sorted(inputs)}, "sany": sany, "results": results}
    (output / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    return 0 if sany["passed"] and len(results) == len(matrix) and all(case["passed"] for case in results) else 1


if __name__ == "__main__":
    raise SystemExit(main())
