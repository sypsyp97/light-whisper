"""Run finite transport models and require each named semantic outcome."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import time


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--jar", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--case", action="append", help="Run named cases only; never claim full matrix closure")
    args = parser.parse_args()
    here = Path(__file__).resolve().parent
    root = here.parents[1]
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    jar = args.jar.resolve(strict=True)
    matrix = json.loads((here / "matrix.json").read_text(encoding="utf-8"))
    cases = [case for case in matrix if not args.case or case["name"] in args.case]
    if not cases or args.case and set(args.case) - {case["name"] for case in cases}:
        raise ValueError("Unknown/empty case selection")
    base = ["java", "-Xmx512m", "-cp", str(jar)]
    receipts = []
    preflights = []
    for module in sorted({case["module"] for case in cases}):
        command = base + ["tla2sany.SANY", module + ".tla"]
        run = subprocess.run(command, cwd=here, capture_output=True, text=True, timeout=45)
        log = run.stdout + run.stderr
        (output / ("sany-" + module + ".log")).write_text(log, encoding="utf-8")
        passed = run.returncode == 0 and f"Semantic processing of module {module}" in log \
            and not re.search(r"(?:Fatal|Semantic|Parse) errors|Parse Error", log, re.I)
        preflights.append({"module": module, "command": command, "exit": run.returncode, "passed": passed})
        if not passed:
            raise RuntimeError("SANY authoring failure: " + module)
    for case in cases:
        command = base + ["tlc2.TLC", "-workers", "1", "-coverage", "1", "-metadir",
                          str(output / (case["name"] + "-states")), "-config",
                          str(here / case["config"]), case["module"] + ".tla"]
        started = time.monotonic()
        run = subprocess.run(command, cwd=here, capture_output=True, text=True, timeout=120)
        log = run.stdout + run.stderr
        (output / (case["name"] + ".log")).write_text(log, encoding="utf-8")
        if case["kind"] == "positive":
            expected = "Model checking completed. No error has been found."
            passed = run.returncode == 0 and expected in log
        else:
            expected = "Temporal properties were violated." if case["kind"] == "temporal-negative" \
                else f'Invariant {case["property"]} is violated.'
            passed = run.returncode != 0 and expected in log
        counts = re.findall(r"([\d,]+) states generated, ([\d,]+) distinct states found", log)
        depth = re.findall(r"depth of the complete state graph search is (\d+)", log)
        receipts.append({**case, "command": command, "exit": run.returncode, "passed": passed,
            "expected": expected, "elapsed_seconds": round(time.monotonic() - started, 3),
            "state_counts": counts[-1] if counts else None, "depth": depth[-1] if depth else None})
        print(case["name"], "EXPECTED" if passed else "UNEXPECTED", flush=True)
    paths = list(here.glob("*.tla")) + list((here / "configs").glob("*.cfg"))
    paths += [here / name for name in ["matrix.json", "check_model.py", "Cargo.toml", "Cargo.lock",
                                      "src/lib.rs", "src/bin/agent_transport_probe.rs", "tests/transport.rs"]]
    paths += [root / "src-tauri/src/agent_api" / name for name in ["secure_files.rs", "transport.rs"]]
    paths += [root / name for name in ["scripts/light_whisper_cli.py",
        "src-tauri/src/services/funasr_service.rs", "src-tauri/src/services/funasr_service/protocol.rs",
        "src-tauri/src/utils/paths.rs"]]
    paths += [root / "formal/agent-api-security/tests/support/mod.rs", jar]
    hashes = {str(path): hashlib.sha256(path.read_bytes()).hexdigest() for path in paths}
    receipt = {"bounds": {"request_schema_partitions": 23, "hosts": 2, "requests": 5,
        "network_worker_capacity": 4, "broker_input_records": 2,
        "worker_phases": ["unreserved", "preauth-ingress", "dispatch", "egress", "released"],
        "clock_ticks": 3, "deadline_ticks": 2, "chunks": 2, "dispatch_duration_ticks": 3},
        "assumptions": ["HMAC authenticity and cryptographic nonce freshness; real ring tested separately",
            "Weak fairness of enabled finite owner/dispatch completion steps",
            "Only cooperating lease holders publish between cleanup observation and unlink",
            "Arbitrary same-user endpoint rewriting during cleanup is outside trusted-principal scope"],
        "source_correspondence": "OPEN: partial transport implementation not yet GREEN; final source/model review and bound rerun required",
        "engine_wire_binding": "canonical qwen3-asr-0.6b/confucius4-r2t2 only; localQwen/R2T2 aliases rejected, per Python LOCAL_ENGINES and configured_engine",
        "local_reject_binding": "Broker bad class includes schema-valid body whose signed envelope exceeds the byte cap; existing RecoverableFailures/stop-recoverable negative/recovery witness cover continuation",
        "egress_write_schedule_binding": "cfgtest start_observed_writes(1024) must share the public start core and actual write_frame; observer follows actual successful native writes with actual stream/original absolute deadline/written length; test queries native write_timeout and gates two write phases; no kernel-backpressure claim; current seam Unsupported is INTERFACE_PENDING, source coupling and reset/fixed-timeout/early-release controls OPEN",
        "transcribe_wire_binding": "audio_path plus options(language/context only); optional top-level hot_words list[str]; nested options.hot_words invalid, per actual Python/native callers",
        "full_matrix": not bool(args.case), "hashes": hashes,
        "sany": preflights, "results": receipts}
    (output / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    print(output, flush=True)
    return 0 if all(case["passed"] for case in receipts) else 1


if __name__ == "__main__":
    raise SystemExit(main())
