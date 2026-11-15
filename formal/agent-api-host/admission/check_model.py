"""Execute the complete source-bound controller admission matrix."""
import argparse
import hashlib
import json
import re
import subprocess
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
MODULE = "ControllerAdmission"
JAR_SHA256 = "936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88"


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def execute(command, log_path):
    started = time.monotonic()
    result = subprocess.run(command, cwd=HERE, capture_output=True, text=True,
                            encoding="utf-8", errors="replace", check=False)
    text = result.stdout + result.stderr
    log_path.write_text(text, encoding="utf-8")
    return result.returncode, text, round(time.monotonic() - started, 3)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--java", required=True)
    parser.add_argument("--jar", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    jar = args.jar.resolve(strict=True)
    if digest(jar) != JAR_SHA256:
        parser.error("TLA+ JAR SHA256 differs from pinned tool")
    bindings = json.loads((HERE / "source-bindings.json").read_text(encoding="utf-8"))
    for name, expected in bindings.items():
        if digest(ROOT / name) != expected:
            parser.error("source changed; review admission correspondence first: " + name)
    cases = json.loads((HERE / "cases.json").read_text(encoding="utf-8"))
    if not cases or len({case["name"] for case in cases}) != len(cases):
        parser.error("empty or duplicate case inventory")
    for case in cases:
        if case["kind"] not in {"positive", "safety", "progress", "witness"}:
            parser.error("unknown case kind: " + case["name"])
        if case["config"] != case["name"] + ".cfg":
            parser.error("case/config identity differs: " + case["name"])
    output = args.output.resolve()
    if output.is_relative_to(ROOT):
        parser.error("evidence output must be outside the source checkout")
    output.mkdir(parents=True, exist_ok=False)
    java_tmp = output / "java-tmp"
    java_tmp.mkdir()
    java = [args.java, "-Djava.io.tmpdir=" + str(java_tmp), "-Xmx512m", "-cp", str(jar)]
    model = HERE / (MODULE + ".tla")
    code, text, elapsed = execute(java + ["tla2sany.SANY", model.name], output / "sany.log")
    receipt = {
        "source_bindings": bindings, "source_bindings_sha256": digest(HERE / "source-bindings.json"),
        "model_sha256": digest(model), "runner_sha256": digest(Path(__file__)),
        "cases_sha256": digest(HERE / "cases.json"), "jar_sha256": digest(jar),
        "bounds": {"requests": 1, "local_engines": 2, "invalid_engine_partition": 1,
                   "busy_partitions": ["none", "recording", "processing"],
                   "owner_watch_changes": 1, "optional_download_changes": 1,
                   "optional_recording_claims_during_lifecycle_wait": 1,
                   "optional_startup_claims_during_lifecycle_wait": 1,
                   "prior_lifecycle_owners": 1, "prior_download_slot_owners": 1,
                   "heap_mib": 512, "workers": 1},
        "assumptions": ["internal prefix steps eventually run",
                        "pre-existing internal mutex owners eventually settle",
                        "owner watch sender remains live; one optional false-to-true change",
                        "one optional download start or finish; no fairness forces user events",
                        "processing and external ASR inputs fixed for this one bounded call",
                        "one optional recording or startup claim while a prior lifecycle owner holds the gate",
                        "successful owned configuration write represents admitted effects; IO faults outside scope",
                        "first config write is transfer; no post-commit completion/cancellation proof"],
        "constraints": [], "symmetry": None, "deadlock_check": True, "java_io_tmpdir": str(java_tmp),
        "sany": {"exit": code, "seconds": elapsed, "expected": code == 0 and not re.search(
            r"Semantic errors|Parsing failed|Parse Error|Fatal errors|\*\*\* Errors:", text)},
        "cases": [],
    }
    receipt_path = output / "results.json"
    def save():
        receipt_path.write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    save()
    if not receipt["sany"]["expected"]:
        raise SystemExit("SANY preflight failed; inspect sany.log")
    for case in cases:
        config = HERE / case["config"]
        config_text = config.read_text(encoding="utf-8")
        if re.search(r"(?m)^\s*(?:CONSTRAINT|ACTION_CONSTRAINT|SYMMETRY|VIEW)\b", config_text):
            raise SystemExit("state-space restriction refused: " + config.name)
        properties = re.findall(r"(?m)^PROPERTY (\w+)$", config_text)
        if case["kind"] == "progress" and properties != [case["target"]]:
            raise SystemExit("progress control must activate its named property only")
        command = java + ["tlc2.TLC", "-workers", "1", "-metadir",
                          str(output / ("states-" + case["name"])), "-config", config.name, model.name]
        code, text, elapsed = execute(command, output / (case["name"] + ".log"))
        if case["kind"] == "positive":
            expected = code == 0 and "Model checking completed. No error has been found." in text
        elif case["kind"] == "progress":
            expected = code == 13 and "Temporal properties were violated." in text
        else:
            expected = code == 12 and f"Invariant {case['target']} is violated." in text
        counts = re.search(r"([\d,]+) states generated, ([\d,]+) distinct states found", text)
        depth = re.search(r"The depth of the complete state graph search is (\d+)", text)
        row = {**case, "command": command, "config_sha256": digest(config), "exit": code,
               "expected": expected, "seconds": elapsed,
               "generated": int(counts[1].replace(",", "")) if counts else None,
               "distinct": int(counts[2].replace(",", "")) if counts else None,
               "depth": int(depth[1]) if depth else None}
        receipt["cases"].append(row)
        save()
        print(json.dumps(row), flush=True)
        if not expected:
            raise SystemExit("Unexpected checker outcome: " + case["name"])
    print(json.dumps({"expected_outcomes": len(cases), "matched": len(receipt["cases"])}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
