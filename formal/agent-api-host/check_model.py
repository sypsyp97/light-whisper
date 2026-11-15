"""One-worker finite host model matrix; exact named semantic controls only."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys
import time

HERE = Path(__file__).resolve().parent
SAFETY = ["TypeOK", "EndpointHasOneOwner", "AdmittedOperationRetainsLease", "ClientExitIsLocal",
          "BusyAndInvalidHaveNoEffects", "RetiredStartCannotPublish", "OwnedAudioLifetime",
          "StoppedIsClean", "ObservedIdentityIsActual"]
PROGRESS = ["ShutdownProgress", "OperationProgress"]
NEGATIVES = {"lease": "EndpointHasOneOwner", "disconnect": "AdmittedOperationRetainsLease",
             "client_exit": "ClientExitIsLocal", "busy": "BusyAndInvalidHaveNoEffects",
             "invalid": "BusyAndInvalidHaveNoEffects", "stale": "RetiredStartCannotPublish",
             "audio": "OwnedAudioLifetime", "release": "StoppedIsClean", "identity": "ObservedIdentityIsActual",
             "cancel": "ShutdownProgress", "finish": "OperationProgress"}
WITNESSES = ["NoDisconnectedPending", "NoStartupRetirement", "NoBusyRejection", "NoRestartAfterInvalidRejection",
             "NoDuplicateAttempt", "NoCompletion", "NoClientOnlyReady", "NoCleanShutdown"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--java", type=Path, required=True)
    parser.add_argument("--jar", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--sany-only", action="store_true")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    model = HERE / "AgentHost.tla"
    jar_hash = hashlib.sha256(args.jar.read_bytes()).hexdigest()
    if jar_hash != "936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88":
        parser.error("unexpected TLA tools digest")
    common = [str(args.java), "-Xmx512m", "-cp", str(args.jar)]
    sany = subprocess.run([*common, "tla2sany.SANY", str(model)], cwd=HERE,
                          stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    (args.output / "sany.txt").write_text(sany.stdout, encoding="utf-8")
    if sany.returncode or any(error in sany.stdout for error in
                             ("Semantic errors", "Parsing failed", "Parse Error", "Fatal errors")):
        print(sany.stdout)
        return 1
    if args.sany_only:
        print("SANY PASS")
        return 0
    cases = [("positive", "none", None)] + [(f"negative-{key}", key, prop) for key, prop in NEGATIVES.items()]
    cases += [(f"witness-{prop}", "none", prop) for prop in WITNESSES]
    results = []
    for name, broken, expected in cases:
        config = args.output / f"{name}.cfg"
        invariants = SAFETY + ([expected] if expected in WITNESSES else [])
        properties = [expected] if expected in PROGRESS else PROGRESS
        config.write_text(f'SPECIFICATION Spec\nCONSTANT Broken = "{broken}"\n'
                          + "INVARIANTS\n" + "\n".join(invariants)
                          + "\nPROPERTIES\n" + "\n".join(properties) + "\n", encoding="utf-8")
        command = [*common, "tlc2.TLC", "-workers", "1", "-coverage", "1", "-metadir",
                   str((args.output / f"states-{name}").resolve()), "-config", str(config.resolve()), "AgentHost"]
        started = time.monotonic()
        result = subprocess.run(command, cwd=HERE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        (args.output / f"{name}.txt").write_text(result.stdout, encoding="utf-8")
        invariant_failure = expected and f"Invariant {expected} is violated" in result.stdout
        temporal_failure = (expected in PROGRESS and "Temporal properties were violated" in result.stdout
                            and "State 1:" in result.stdout
                            and ('phase = "transcribing"' in result.stdout)
                            and ("Stuttering" in result.stdout or "Back to state" in result.stdout))
        # Liveness controls activate exactly one temporal property to identify it.
        ok = (result.returncode == 0 and "Model checking completed. No error has been found." in result.stdout
              if expected is None else result.returncode != 0 and (invariant_failure or temporal_failure))
        counts = re.findall(r"([\d,]+) states generated, ([\d,]+) distinct states found", result.stdout)
        depth = re.search(r"depth of the complete state graph search is (\d+)", result.stdout)
        results.append({"case": name, "broken": broken, "expected": expected, "exit": result.returncode,
                        "pass": bool(ok), "counts": counts[-1] if counts else None,
                        "depth": int(depth[1]) if depth else None,
                        "elapsed_seconds": round(time.monotonic() - started, 3), "command": command,
                        "config_sha256": hashlib.sha256(config.read_bytes()).hexdigest()})
        print(json.dumps(results[-1]), flush=True)
        if not ok:
            print(result.stdout[-6000:])
            break
    source_paths = ["src-tauri/src/lib.rs", "src-tauri/src/agent_api/controller.rs",
                    "src-tauri/src/agent_api/transport.rs", "src-tauri/src/services/funasr_service.rs",
                    "src-tauri/src/services/funasr_service/installation.rs",
                    "src-tauri/src/commands/funasr.rs", "src-tauri/src/state/app_state.rs"]
    receipt = {"model_sha256": hashlib.sha256(model.read_bytes()).hexdigest(), "jar_sha256": jar_hash,
               "bounds": {"clients": 2, "native_owners": 1, "engine_identities": 2,
                          "admitted_operations": 2, "starts_or_restarts": 2},
               "correspondence": "INTERFACE_PENDING until compiled native host implementation reviewed",
               "sources": {p: hashlib.sha256((HERE.parents[1] / p).read_bytes()).hexdigest() for p in source_paths},
               "results": results}
    (args.output / "receipt.json").write_text(json.dumps(receipt, indent=2), encoding="utf-8")
    return 0 if len(results) == len(cases) and all(r["pass"] for r in results) else 1


if __name__ == "__main__":
    sys.exit(main())
