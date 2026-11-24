"""Run the finite standalone CLI cache model, challenges, and reachability checks."""
import argparse
import hashlib
import json
import re
import shutil
import subprocess
import tempfile
from pathlib import Path

SAFETY = "TypeOK SelectionTrusted OnlyVerifiedPublication ExclusiveStaging HostPreserved DoctorReadOnly Settled"
NEGATIVES = {
    "NoStamp": "OnlyVerifiedPublication", "NoCacheStamp": "SelectionTrusted",
    "LegacyLaunch": "SelectionTrusted", "EarlyPublish": "OnlyVerifiedPublication",
    "NoLock": "ExclusiveStaging", "AcceptLink": "OnlyVerifiedPublication",
    "SkipChecksum": "OnlyVerifiedPublication", "OverwriteHost": "HostPreserved",
    "DoctorInstalls": "DoctorReadOnly", "LeakStage": "Settled",
}
WITNESSES = (
    "NoConcurrentReuse", "NoLegacyUpgrade", "NoCleanInstall", "NoDoctorDuringStage",
    "NoLinkCleanup", "NoBadStampCleanup", "NoWithoutArchiveHost",
)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--java", default="java")
    parser.add_argument("--jar", type=Path, required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    args = parser.parse_args()
    jar = args.jar.resolve(strict=True)
    evidence = args.evidence.resolve()
    evidence.mkdir(parents=True, exist_ok=True)
    model = Path(__file__).with_name("CliBootstrap.tla")
    java = [args.java, "-Xmx512m", "-XX:+UseParallelGC", "-cp", str(jar)]
    cases = [("Correct", "Correct", f"INVARIANTS {SAFETY}\nPROPERTY EventuallySettled",
              0, "Model checking completed. No error has been found.")]
    cases.extend((mode, mode, f"INVARIANT {prop}", 12, f"Invariant {prop} is violated")
                 for mode, prop in NEGATIVES.items())
    cases.append(("StuckCleanup", "StuckCleanup", "PROPERTY EventuallySettled", 13,
                  "Temporal properties were violated"))
    cases.extend((prop, "Correct", f"INVARIANT {prop}", 12, f"Invariant {prop} is violated")
                 for prop in WITNESSES)
    receipt = {"model_sha256": hashlib.sha256(model.read_bytes()).hexdigest(),
               "jar_sha256": hashlib.sha256(jar.read_bytes()).hexdigest(), "runs": {}}
    with tempfile.TemporaryDirectory(prefix="lw-cli-model-") as work:
        root = Path(work)
        shutil.copyfile(model, root/model.name)
        output = subprocess.run(java + ["tla2sany.SANY", model.name], cwd=root,
                                stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        (evidence/"sany.log").write_text(output.stdout, encoding="utf-8")
        if output.returncode or f"Semantic processing of module {model.stem}" not in output.stdout:
            raise SystemExit("SANY failed")
        for name, mode, checks, code, match in cases:
            cfg = f'SPECIFICATION Spec\nCONSTANT Mode = "{mode}"\n{checks}\n'
            (root/"case.cfg").write_text(cfg)
            (evidence/f"{name}.cfg").write_text(cfg)
            result = subprocess.run(java + ["tlc2.TLC", "-workers", "1", "-metadir", f"states-{name}",
                                    "-config", "case.cfg", model.name], cwd=root, text=True,
                                    stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
            (evidence/f"{name}.log").write_text(result.stdout, encoding="utf-8")
            counts = re.search(r"([\d,]+) states generated, ([\d,]+) distinct states found, ([\d,]+) states left", result.stdout)
            passed = result.returncode == code and match in result.stdout and counts is not None
            if code == 0:
                passed = passed and counts[3] == "0"
            receipt["runs"][name] = {"returncode": result.returncode, "expected": match,
                                     "counts": list(counts.groups()) if counts else None, "matched": passed}
            print(f"{name}: {result.returncode}, expected verdict {'confirmed' if passed else 'FAILED'}", flush=True)
            (evidence/"results.json").write_text(json.dumps(receipt, indent=2), encoding="utf-8")
            if not passed:
                raise SystemExit(f"Unexpected TLC outcome; see {evidence/name}.log")


if __name__ == "__main__":
    main()
