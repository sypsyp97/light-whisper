"""Run the complete source-bound backend numeric-admission matrix."""
import argparse
import hashlib
import json
import re
import subprocess
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
MODULE = "JsonNumbers"
JAR_SHA256 = "936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88"
SOURCE_SHA256 = "db2655d9beecc41de1b20338cb44c6af64dae72701d833b5459639d0cc55d9b7"
CASES = (
    ("JsonNumbers", "positive", ""),
    ("LegacyAcceptNegative", "safety", "MatchingBadIsTerminal"),
    ("LegacyOutputNegative", "safety", "StrictOutput"),
    ("SkipBadNegative", "safety", "MatchingBadIsTerminal"),
    ("ValidateNoiseNegative", "safety", "IrrelevantFramesIgnored"),
    ("PublishBeforeReapNegative", "safety", "FailureAfterReap"),
    ("HiddenFailureNegative", "safety", "BadReplyVisible"),
    ("ReplyIdentityNegative", "safety", "CorrelationPreserved"),
    ("FiniteContinuationNegative", "safety", "FiniteFramesContinue"),
    ("CleanExitNegative", "safety", "CleanExit"),
    ("RejectedCleanupProgressNegative", "progress", "EventuallyDone"),
    ("StartupWitness", "witness", "WitnessNotReached"),
    ("ActionWitness", "witness", "WitnessNotReached"),
    ("NoiseWitness", "witness", "WitnessNotReached"),
    ("FiniteWitness", "witness", "WitnessNotReached"),
)
POSITIVE_INVARIANTS = {
    "TypeOK", "StrictOutput", "MatchingBadIsTerminal", "BadReplyVisible",
    "FailureAfterReap", "CorrelationPreserved", "IrrelevantFramesIgnored",
    "FiniteFramesContinue", "CleanExit",
}


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def execute(command, log_path):
    started = time.monotonic()
    try:
        result = subprocess.run(command, cwd=HERE, capture_output=True, text=True,
                                encoding="utf-8", errors="replace", check=False, timeout=60)
    except subprocess.TimeoutExpired as error:
        parts = [part.decode("utf-8", errors="replace") if isinstance(part, bytes) else part or ""
                 for part in (error.stdout, error.stderr)]
        log_path.write_text("".join(parts), encoding="utf-8")
        raise
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
        parser.error("TLA+ JAR SHA256 differs from the pinned tool")
    source = ROOT / "scripts/light_whisper_cli.py"
    if digest(source) != SOURCE_SHA256:
        parser.error("CLI source changed; review numeric-admission correspondence first")
    if {path.stem for path in HERE.glob("*.cfg")} != {name for name, _, _ in CASES}:
        parser.error("numeric-admission configuration inventory differs")
    output = args.output.resolve()
    if output.is_relative_to(ROOT):
        parser.error("evidence output must be outside the source checkout")
    try:
        output.mkdir(parents=True, exist_ok=False)
    except FileExistsError:
        parser.error("evidence output already exists")
    java_tmp = output / "java-tmp"
    java_tmp.mkdir()
    java = [args.java, "-Djava.io.tmpdir=" + str(java_tmp), "-Xmx512m", "-cp", str(jar)]
    receipt = {
        "source": {"path": "scripts/light_whisper_cli.py", "sha256": digest(source)},
        "jar_sha256": digest(jar), "runner_sha256": digest(Path(__file__)),
        "model_sha256": digest(HERE / (MODULE + ".tla")),
        "inputs": {path.name: digest(path) for path in HERE.iterdir()
                   if path.suffix in {".tla", ".cfg"}},
        "constraints": [], "symmetry": None, "deadlock_check": True,
        "java_io_tmpdir": str(java_tmp), "sany": {}, "cases": [],
    }

    def save():
        (output / "results.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")

    def run(command, log_path):
        started = time.monotonic()
        try:
            code, text, elapsed = execute(command, log_path)
            return code, text, elapsed, False
        except subprocess.TimeoutExpired:
            return None, "Checker response timed out", round(time.monotonic() - started, 3), True
        except OSError as error:
            log_path.write_text(str(error), encoding="utf-8")
            return None, str(error), round(time.monotonic() - started, 3), False

    command = java + ["tla2sany.SANY", MODULE + ".tla"]
    code, text, elapsed, timed_out = run(command, output / "sany.log")
    receipt["sany"] = {
        "command": command, "exit": code, "seconds": elapsed, "timed_out": timed_out,
        "expected": code == 0 and f"Semantic processing of module {MODULE}" in text
        and not re.search(r"(?:Semantic|Fatal|Parse) errors?|Parsing failed|\*\*\* Errors:", text, re.I),
    }
    save()
    if not receipt["sany"]["expected"]:
        raise SystemExit("SANY preflight failed; inspect sany.log")
    for name, kind, target in CASES:
        config = HERE / (name + ".cfg")
        config_text = config.read_text(encoding="utf-8")
        if re.search(r"(?m)^\s*(?:CONSTRAINTS?|ACTION_CONSTRAINTS?|SYMMETRY|VIEW)\b|CHECK_DEADLOCK\s+FALSE", config_text):
            raise SystemExit("state-space restriction refused: " + config.name)
        declarations = {}
        for keyword in ("INVARIANT", "PROPERTY"):
            lines = re.findall(rf"(?m)^\s*{keyword}S?\s+([^\n]+)$", config_text)
            declarations[keyword] = [operator for line in lines for operator in line.split()]
        invariants, properties = declarations["INVARIANT"], declarations["PROPERTY"]
        admitted = "TypeOK" in invariants
        if kind == "positive":
            admitted = admitted and set(invariants) == POSITIVE_INVARIANTS and properties == ["EventuallyDone"]
        elif kind == "progress":
            admitted = admitted and properties == [target]
        else:
            admitted = admitted and target in invariants and not properties
        if not admitted:
            raise SystemExit("required properties differ: " + config.name)
        command = java + ["tlc2.TLC", "-workers", "1", "-metadir",
                          str(output / ("states-" + name)), "-config", config.name, MODULE + ".tla"]
        code, text, elapsed, timed_out = run(command, output / (name + ".log"))
        counts = re.findall(r"([\d,]+) states generated, ([\d,]+) distinct states found", text)
        depths = re.findall(r"The depth of the complete state graph search is (\d+)", text)
        depth = int(depths[-1]) if depths else None
        if kind == "positive":
            expected = code == 0 and "Model checking completed. No error has been found." in text
        elif kind == "progress":
            progress = re.findall(
                r"(?m)^Progress\((\d+)\)[^\n]*: ([\d,]+) states generated, "
                r"([\d,]+) distinct states found, 0 states left on queue\.$", text)
            final_counts = re.findall(
                r"(?m)^([\d,]+) states generated, ([\d,]+) distinct states found, "
                r"0 states left on queue\.$", text)
            checked_counts = re.findall(
                r"Checking temporal properties for the complete state space with "
                r"([\d,]+) total distinct states", text)
            depth = int(progress[-1][0]) if progress else None
            complete = bool(progress and final_counts and checked_counts
                            and progress[-1][1:] == final_counts[-1]
                            and checked_counts[-1] == final_counts[-1][1]
                            and "Finished checking temporal properties" in text
                            and re.search(r"(?m)^Finished in .+$", text))
            expected = (code == 13 and "Temporal properties were violated." in text
                        and "The following behavior constitutes a counter-example:" in text
                        and complete)
        else:
            expected = code == 12 and f"Invariant {target} is violated." in text
        row = {
            "name": name, "kind": kind, "target": target, "command": command,
            "config_sha256": digest(config), "invariants": invariants, "properties": properties,
            "exit": code, "expected": bool(expected and counts and depth is not None),
            "seconds": elapsed, "timed_out": timed_out,
            "generated": int(counts[-1][0].replace(",", "")) if counts else None,
            "distinct": int(counts[-1][1].replace(",", "")) if counts else None,
            "depth": depth,
        }
        receipt["cases"].append(row)
        save()
        print(json.dumps(row), flush=True)
        if not row["expected"]:
            raise SystemExit("unexpected checker outcome: " + name)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
