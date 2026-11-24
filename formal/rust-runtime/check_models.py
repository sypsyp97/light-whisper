"""Run every native-runtime TLA+ model and check each control against its designated verdict."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys

JAR_SHA256 = "936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88"
TEMPORAL = "Temporal properties were violated"
# None is exhaustive success. Other values are named violated invariants except TEMPORAL.
MODELS = {
    "native-asr/NativeStream": {"NativeStream": None, "Offset": "Contiguous",
        "Leak": "NoLeakedEvent", "DoublePad": "FinalContextOnce"},
    "cli/NativeCliSession": {"Correct": None, "ForgetChild": "Retired", "SendInvalid": "OnlyValid",
        "Retry": "NoRetry", "Hang": TEMPORAL, "DoubleUse": "NotDoubleUse",
        "FailureRetired": "NotFailureRetired"},
    "vad/VadBoundaries": {"VadBoundaries": None, "earlyWork": "EligibleWork",
        **{name: "ExactRegions" for name in ("lateStart","shortGap","staleCandidate","staleSilence","truncateTail")}},
    "product/EngineLifecycle": {"none": None, "idle": "OwnedRuntime", "id": "AlignedAudio",
        "offset": "AlignedAudio", "finish": "NoStaleCompletion"},
    "product/WakeBackend": {"wake": None, "wake-no-fallback": "AvailableBackendUsed"},
}
BINDINGS = {
    "native-asr/NativeStream": ["native-asr/src/abi.rs", "native-asr/src/lib.rs"],
    "cli/NativeCliSession": ["cli/src/main.rs", "cli/src/client.rs", "cli/src/protocol.rs"],
    "vad/VadBoundaries": ["vad/src/lib.rs", "vad/src/native.rs", "vad/native/adapter.cpp"],
    "product/EngineLifecycle": ["engine/src/server.rs"],
    "product/WakeBackend": ["engine/src/server.rs", "engine/src/recognition.rs"],
    "cli-bootstrap/CliBootstrap": ["cli/src/standalone.rs", "cli/src/config.rs"],
}
COUNTS = re.compile(r"([\d,]+) states generated, ([\d,]+) distinct states found, ([\d,]+) states left")
SUCCESS = "Model checking completed. No error has been found."


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def execute(cmd, cwd, log):
    result = subprocess.run(cmd, cwd=cwd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                            text=True, errors="replace", timeout=180)
    log.write_text(result.stdout, encoding="utf-8")
    return result.returncode, result.stdout


def run(args, receipt):
    root = args.source_root.resolve()
    folder = root/"formal/rust-runtime"
    out = args.output.resolve()
    jar = args.jar.resolve(strict=True)
    java = shutil.which(str(args.java)) or str(args.java)
    if sha(jar) != JAR_SHA256:
        raise ValueError("TLA+ tools JAR differs from the project's 1.7.4 pin")
    expected_modules = {name+".tla" for name in BINDINGS}
    actual_modules = {path.relative_to(folder).as_posix() for path in folder.rglob("*.tla")}
    if actual_modules != expected_modules:
        raise ValueError(f"model inventory mismatch: {sorted(actual_modules ^ expected_modules)}")
    expected_configs = {str(Path(model).parent/case).replace("\\","/")+".cfg"
                        for model,cases in MODELS.items() for case in cases}
    actual_configs = {path.relative_to(folder).as_posix() for path in folder.rglob("*.cfg")}
    if expected_configs != actual_configs:
        raise ValueError(f"configuration inventory mismatch: {sorted(actual_configs ^ expected_configs)}")
    receipt["inputs"] = {file:sha(folder/file) for file in sorted(actual_modules|actual_configs|{"cli-bootstrap/check.py"})}
    receipt["owners"] = {name:{file:sha(root/"rust-runtime/crates"/file) for file in paths}
                         for name,paths in BINDINGS.items()}
    receipt.update(runner_sha256=sha(Path(__file__)), jar_sha256=sha(jar), java=java)
    base = [java, "-XX:+UseParallelGC", "-Xmx512m", "-cp", str(jar)]
    for model,cases in MODELS.items():
        source = folder/(model+".tla")
        owner = out/model
        owner.mkdir(parents=True)
        shutil.copy2(source,owner/source.name)
        cmd = base+["tla2sany.SANY",source.name]
        code,log = execute(cmd,owner,owner/"sany.log")
        if code or f"Semantic processing of module {source.stem}" not in log or re.search(r"(Fatal|Semantic|Parse) errors",log,re.I):
            raise RuntimeError(f"SANY failed for {model}")
        for case,expected in cases.items():
            cfg = source.parent/(case+".cfg")
            shutil.copy2(cfg,owner/cfg.name)
            cmd = base+["tlc2.TLC","-workers","1","-metadir",str(owner/("states-"+case)),
                        "-config",cfg.name,source.name]
            code,log = execute(cmd,owner,owner/(case+".log"))
            counts = COUNTS.findall(log)
            verdict = SUCCESS if expected is None else TEMPORAL if expected == TEMPORAL else f"Invariant {expected} is violated"
            wanted_code = 0 if expected is None else 13 if expected == TEMPORAL else 12
            good = bool(counts) and code == wanted_code and verdict in log
            if expected is None:
                good = good and counts[-1][2] == "0"
            receipt["runs"].append(dict(model=model,case=case,expected=verdict,actual_exit=code,
                                        counts=counts[-1] if counts else None,passed=good))
            if not good:
                raise RuntimeError(f"unexpected {model}/{case} verdict, see {owner/(case+'.log')}")
    dedicated = folder/"cli-bootstrap/check.py"
    bootstrap = out/"cli-bootstrap"
    cmd = [sys.executable,str(dedicated),"--java",java,"--jar",str(jar),"--evidence",str(bootstrap)]
    code,_ = execute(cmd,root,out/"cli-bootstrap.log")
    if code:
        raise RuntimeError("CLI bootstrap matrix failed")
    data = json.loads((bootstrap/"results.json").read_text(encoding="utf-8"))
    if len(data["runs"]) != 19 or not all(r["matched"] for r in data["runs"].values()):
        raise RuntimeError("incomplete CLI bootstrap positive, challenge or witness receipt")
    for name,result in data["runs"].items():
        receipt["runs"].append(dict(model="cli-bootstrap/CliBootstrap",case=name,expected=result["expected"],
                                    actual_exit=result["returncode"],counts=result["counts"],passed=result["matched"]))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--jar", type=Path, required=True)
    parser.add_argument("--java", default="java")
    parser.add_argument("--output", type=Path, required=True, help="new external receipt directory")
    parser.add_argument("--source-root", type=Path, default=Path(__file__).resolve().parents[2])
    args = parser.parse_args()
    if args.output.resolve().is_relative_to((args.source_root/"formal/rust-runtime").resolve()):
        parser.error("use an output directory outside the model sources")
    args.output.mkdir(parents=True,exist_ok=False)
    receipt = {"runs":[],"passed":False}
    try:
        run(args,receipt)
        receipt["passed"] = True
    except Exception as error:
        receipt["error"] = f"{type(error).__name__}: {error}"
    (args.output/"receipt.json").write_text(json.dumps(receipt,indent=2),encoding="utf-8")
    print(json.dumps({"passed":receipt["passed"],"cases":len(receipt["runs"]),"error":receipt.get("error")}))
    return int(not receipt["passed"])


if __name__ == "__main__":
    raise SystemExit(main())
