"""Run the finite subscription protocol matrix, serially, with owned evidence."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys
import time


JAR_SHA256 = "936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88"
CASES = (
    ("AsrSubscriptionBootstrapZeroWrites", "positive", None),
    ("AsrSubscriptionBootstrapOneWrite", "positive", None),
    ("AsrSubscriptionBootstrap", "positive", None),
    ("LegacyEagerReadNegative", "invariant", "SettledSavedConfig"),
    ("GenerationFenceNegative", "invariant", "PublicationGenerationOwned"),
    ("DisposedHydrationNegative", "invariant", "NoReadAfterDispose"),
    ("BootstrapOwnershipNegative", "invariant", "PublicationNeverRegresses"),
    ("ListenerReleaseNegative", "invariant", "ListenerReleasedAfterAck"),
    ("FailureDiagnosticNegative", "invariant", "FailureVisible"),
    ("AckHydrationProgressNegative", "temporal", "LatestSavedEventually"),
    ("FallbackHydrationProgressNegative", "temporal", "FallbackEventuallyUsable"),
    ("RecoveredMissedEventWitness", "witness", "NoRecoveredMissedEvent"),
    ("PreAckEventWitness", "witness", "NoPreAckDelivery"),
    ("StaleCompletionWitness", "witness", "NoStaleCompletion"),
    ("FallbackHydrationWitness", "witness", "NoFallbackHydration"),
    ("LateReleaseWitness", "witness", "NoLateRelease"),
)


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def execute(command: list[str], cwd: Path, log: Path) -> tuple[int, str]:
    result = subprocess.run(command, cwd=cwd, capture_output=True, text=True,
                            encoding="utf-8", errors="replace", check=False)
    output = result.stdout + result.stderr
    log.write_text(output, encoding="utf-8")
    return result.returncode, output


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--java", type=Path, required=True)
    parser.add_argument("--jar", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--sany-only", action="store_true")
    parser.add_argument("--case", choices=[case[0] for case in CASES], action="append")
    args = parser.parse_args()
    model_dir = Path(__file__).resolve().parent
    output_dir = args.output.resolve()
    output_dir.mkdir(parents=True, exist_ok=False)
    java, jar = args.java.resolve(), args.jar.resolve()
    if sha256(jar) != JAR_SHA256:
        parser.error("tla2tools.jar SHA256 differs from the reviewed pin")
    module = model_dir / "AsrSubscriptionBootstrap.tla"
    source = model_dir.parent.parent / "src/hooks/useAsrEngineSettings.ts"
    receipt = {"jar_sha256": sha256(jar), "module_sha256": sha256(module),
               "source_sha256": sha256(source), "max_heap_mib": 512,
               "workers": 1, "runs": []}
    selected = [case for case in CASES if not args.case or case[0] in args.case]
    receipt["configurations"] = [{
        "case": name, "kind": kind, "property": property_name,
        "config_sha256": sha256(model_dir / f"{name}.cfg"),
        "fixed_write_count": int(re.search(r"MaxWrites\s*=\s*(\d+)",
            (model_dir / f"{name}.cfg").read_text()).group(1)),
        "read_slot_bound": int(re.search(r"MaxReads\s*=\s*(\d+)",
            (model_dir / f"{name}.cfg").read_text()).group(1)),
    } for name, kind, property_name in selected]
    base = [str(java), "-Xmx512m", "-cp", str(jar)]
    sany_command = base + ["tla2sany.SANY", module.name]
    code, text = execute(sany_command, model_dir, output_dir / "sany.log")
    sany_ok = (code == 0 and "Semantic processing of module AsrSubscriptionBootstrap" in text
               and not re.search(r"(?:\*\*\* Errors:|Fatal errors|Semantic errors|Parse Error)", text))
    receipt["sany"] = {"command": sany_command, "exit_code": code, "pass": sany_ok,
                       "shared_module_configurations": [case[0] for case in selected]}
    passed = sany_ok
    if sany_ok and not args.sany_only:
        for name, kind, property_name in selected:
            config = model_dir / f"{name}.cfg"
            command = base + ["tlc2.TLC", "-workers", "1", "-coverage", "1",
                              "-metadir", str(output_dir / f"{name}-states"),
                              "-config", config.name, module.name]
            started = time.monotonic()
            code, text = execute(command, model_dir, output_dir / f"{name}.log")
            if kind == "positive":
                okay = code == 0 and "Model checking completed. No error has been found." in text
            elif kind == "temporal":
                config_properties = re.findall(r"^PROPERTY\s+(\w+)\s*$", config.read_text(), re.M)
                okay = (code != 0 and config_properties == [property_name]
                        and "Temporal properties were violated" in text
                        and "counter-example" in text)
            else:
                okay = (code != 0 and f"Invariant {property_name} is violated" in text
                        and "The behavior up to this point is" in text)
            counts = re.findall(r"([\d,]+) states generated, ([\d,]+) distinct states found", text)
            receipt["runs"].append({
                "case": name, "kind": kind, "property": property_name,
                "config_sha256": sha256(config), "command": command,
                "exit_code": code, "pass": okay,
                "elapsed_seconds": round(time.monotonic() - started, 3),
                "generated_distinct": list(counts[-1]) if counts else None,
            })
            passed = passed and okay
            print(f"{'PASS' if okay else 'FAIL'} {name}", flush=True)
    receipt["pass"] = passed
    receipt["enumerated"] = not args.sany_only and bool(receipt["runs"])
    (output_dir / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
