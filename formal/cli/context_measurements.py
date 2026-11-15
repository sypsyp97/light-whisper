"""Full-cohort context collection and input fingerprints for local verification."""

from importlib import metadata
import hashlib
import json
import locale
import os
from pathlib import Path
import subprocess
import sys
import sysconfig
import time
import unittest


SOURCES = ("scripts/light_whisper_cli.py", "scripts/build_cli.py",
           "src-tauri/resources/r2t2_native.py", "src-tauri/resources/qwen3_native.py")
CONTEXT = "LIGHT_WHISPER_COVERAGE_CONTEXT"


class ContextResult(unittest.TextTestResult):
    def __init__(self, stream, descriptions, verbosity, *, active, environment):
        super().__init__(stream, descriptions, verbosity)
        self.active, self.environment = active, environment
        self.elapsed = {}

    def startTest(self, test):
        if self.active is None:
            raise RuntimeError("Context collection requires active coverage")
        self.previous = self.environment.get(CONTEXT)
        self.started = time.monotonic()
        self.active.switch_context(test.id())
        self.environment[CONTEXT] = test.id()
        super().startTest(test)

    def stopTest(self, test):
        self.elapsed[test.id()] = time.monotonic() - self.started
        self.active.switch_context("<outside-test>")
        if self.previous is None:
            self.environment.pop(CONTEXT, None)
        else:
            self.environment[CONTEXT] = self.previous
        super().stopTest(test)


def snapshot(root, environment):
    root = Path(root)
    tracked = subprocess.check_output(["git", "ls-files", "-z"], cwd=root).decode().split("\0")
    extensions = {".py", ".json", ".toml", ".ini", ".cfg", ".yaml", ".yml", ".lock", ".txt",
                  ".sh", ".rs", ".mjs", ".js", ".ts", ".tsx", ".wav", ".npy", ".npz"}
    paths = {path for path in tracked if Path(path).suffix in extensions and path not in SOURCES}
    # These owned additions are not tracked until the accepted delivery patch is integrated.
    paths.update(str(path.relative_to(root)).replace("\\", "/") for path in (root / "formal/cli").glob("*.py"))
    paths.update(str(path.relative_to(root)).replace("\\", "/") for path in (root / "scripts").glob("test_cli_*.py"))
    files = {path: hashlib.sha256((root / path).read_bytes()).hexdigest() for path in sorted(paths)}
    effective = {key: value for key, value in environment.items()
                 if not key.upper().startswith("COVERAGE_") and key not in {
                     CONTEXT, "LIGHT_WHISPER_CONTEXT_DATASET", "LIGHT_WHISPER_CONTEXT_SHA256",
                     "PYTHONDONTWRITEBYTECODE"}}
    effective["PYTHONPATH"] = str(root / "src-tauri/resources")
    search = [sysconfig.get_path("purelib")]
    if environment.get("LIGHT_WHISPER_VERIFICATION_SITE_PACKAGES"):
        search.append(environment["LIGHT_WHISPER_VERIFICATION_SITE_PACKAGES"])
    distributions = sorted({(item.metadata["Name"], item.version) for item in metadata.distributions(path=search)})
    runtime = {"python": sys.version, "executable_sha256": hashlib.sha256(Path(sys.executable).read_bytes()).hexdigest(),
               "platform": sys.platform, "encoding": locale.getpreferredencoding(False),
               "distributions": [list(item) for item in distributions],
               "environment_sha256": hashlib.sha256(json.dumps(effective, sort_keys=True).encode()).hexdigest()}
    return {"sources": {path: (root / path).read_bytes().decode("utf-8") for path in SOURCES},
            "bindings": {"files": files, "runtime": runtime}}


def build_dataset(before, after, report, test_ids, successful):
    if not successful or before != after or not test_ids or len(set(test_ids)) != len(test_ids):
        raise ValueError("Context collection failed, drifted or has an invalid cohort")
    files = {path.replace("\\", "/"): data for path, data in report.get("files", {}).items()}
    if (set(files) != set(SOURCES) or set(before["sources"]) != set(SOURCES)
            or not report.get("meta", {}).get("branch_coverage")
            or any("contexts" not in entry for entry in files.values())):
        raise ValueError("Missing source or context instrumentation")
    sources = {path: {"text": text, "sha256": hashlib.sha256(text.encode()).hexdigest(),
                      "contexts": files[path]["contexts"]} for path, text in before["sources"].items()}
    return {"schema": 1, "complete": True, "tests": list(test_ids),
            "bindings": before["bindings"], "sources": sources}


def load_suite(root):
    root = Path(root)
    resources = str(root / "src-tauri/resources")
    if resources not in sys.path:
        sys.path.insert(0, resources)
    names = ["scripts.test_light_whisper_cli", "scripts.test_build_cli", *(
        "scripts." + path.stem for path in sorted((root / "scripts").glob("test_cli_*.py"))
        if path.stem != "test_cli_ci")]
    loader = unittest.TestLoader()
    suite = unittest.TestSuite([loader.loadTestsFromNames(names),
                               loader.discover(resources, pattern="test_*.py", top_level_dir=resources)])
    if loader.errors:
        raise RuntimeError("Full-cohort collection has loader errors: " + "\n".join(loader.errors))

    def ids(group):
        for item in group:
            if isinstance(item, unittest.TestSuite):
                yield from ids(item)
            else:
                yield item.id()

    test_ids = list(ids(suite))
    if not test_ids or len(set(test_ids)) != len(test_ids):
        raise ValueError("Full-cohort collection is empty or has duplicate IDs")
    return suite, test_ids


def main():
    import argparse
    import coverage

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("evidence", type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    output = args.evidence.resolve()
    if output == root or root in output.parents:
        parser.error("Evidence must be outside the disposable source checkout")
    if coverage.Coverage.current() is not None:
        parser.error("Disable parent startup coverage before collecting contexts")
    output.mkdir(parents=True, exist_ok=False)
    config_file = str(root / "formal/cli/coverage.ini")
    active = coverage.Coverage(config_file=config_file, data_file=str(output / ".coverage"))
    active.start()
    try:
        active.switch_context("<outside-test>")
        os.environ["COVERAGE_PROCESS_START"] = config_file
        os.environ["COVERAGE_FILE"] = str(output / ".coverage")
        sys.path.insert(0, str(root))
        before = snapshot(root, os.environ)
        suite, test_ids = load_suite(root)
        print(json.dumps({"phase": "collecting", "tests": len(test_ids)}), flush=True)
        with (output / "unittest.log").open("w", encoding="utf-8") as stream:
            runner = unittest.TextTestRunner(stream=stream, verbosity=2, resultclass=lambda *arguments:
                ContextResult(*arguments, active=active, environment=os.environ))
            result = runner.run(suite)
        after = snapshot(root, os.environ)
    finally:
        active.stop()
        active.save()
    combined = coverage.Coverage(config_file=config_file,
                                 data_file=str(output / ".coverage"))
    combined.combine(data_paths=[str(output)], keep=True)
    combined.save()
    combined.json_report(outfile=str(output / "coverage.json"), show_contexts=True)
    with (output / "coverage.txt").open("w", encoding="utf-8") as stream:
        combined.report(file=stream, show_missing=True)
    report = json.loads((output / "coverage.json").read_text())
    dataset = build_dataset(before, after, report, test_ids,
                            result.wasSuccessful() and result.testsRun == len(test_ids))
    dataset["test_elapsed"] = result.elapsed
    payload = json.dumps(dataset, sort_keys=True, indent=2).encode()
    (output / "context-dataset.json").write_bytes(payload)
    receipt = {"tests": len(test_ids), "dataset_sha256": hashlib.sha256(payload).hexdigest(),
               "coverage_totals": report["totals"], "source_paths": list(dataset["sources"])}
    (output / "receipt.json").write_text(json.dumps(receipt, indent=2), encoding="utf-8")
    print(json.dumps(receipt), flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
