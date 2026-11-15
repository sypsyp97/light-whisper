"""Complete numeric-frame matrix orchestration; mocked replies are not TLC evidence."""

import contextlib
import hashlib
import importlib.util
import io
import json
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


HERE = Path(__file__).resolve().parent
EXPECTED_CASES = (
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
CASE_MAP = {name: (kind, target) for name, kind, target in EXPECTED_CASES}
JAR_SHA256 = "936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88"
POSITIVE_INVARIANTS = (
    "TypeOK", "StrictOutput", "MatchingBadIsTerminal", "BadReplyVisible",
    "FailureAfterReap", "CorrelationPreserved", "IrrelevantFramesIgnored",
    "FiniteFramesContinue", "CleanExit",
)
POSITIVE_PROPERTIES = ("EventuallyDone",)
COMPLETE = ("Model checking completed. No error has been found.\n"
            "12 states generated, 7 distinct states found\n"
            "The depth of the complete state graph search is 4\n")
# Actual TLC 2.19 output, with private paths reduced to module basenames.
# Temporal counterexamples report Progress(depth), without a complete-graph depth line.
TEMPORAL_COUNTEREXAMPLE = """TLC2 Version 2.19 of 08 August 2024 (rev: 5a47802)
Parsing file JsonNumbers.tla
Parsing file Integers.tla
Parsing file Sequences.tla
Parsing file Naturals.tla
Semantic processing of module Naturals
Semantic processing of module Integers
Semantic processing of module Sequences
Semantic processing of module JsonNumbers
Starting... (2026-11-15 01:37:07)
Implied-temporal checking--satisfiability problem has 1 branches.
Computing initial states...
Computed 2 initial states...
Computed 4 initial states...
Computed 8 initial states...
Computed 16 initial states...
Computed 32 initial states...
Computed 64 initial states...
Finished computing initial states: 120 distinct states generated at 2026-11-15 01:37:08.
Progress(7) at 2026-11-15 01:37:08: 692 states generated, 516 distinct states found, 0 states left on queue.
Checking temporal properties for the complete state space with 516 total distinct states at (2026-11-15 01:37:08)
Error: Temporal properties were violated.

Error: The following behavior constitutes a counter-example:

State 1: <Initial predicate>
s = [ surface |-> "one_shot",
  badAt |-> "startup",
  number |-> "nan",
  noise |-> "none",
  noiseAt |-> "startup",
  externalId |-> -7,
  phase |-> "startup",
  live |-> TRUE,
  ignored |-> FALSE,
  badSeen |-> FALSE,
  dispatches |-> 0,
  consumed |-> 1,
  attempts |-> 1,
  code |-> 0,
  replies |-> <<>> ]

State 2: <ReceiveBad line 55, col 5 to line 62, col 86 of module JsonNumbers>
s = [ surface |-> "one_shot",
  badAt |-> "startup",
  number |-> "nan",
  noise |-> "none",
  noiseAt |-> "startup",
  externalId |-> -7,
  phase |-> "reap",
  live |-> TRUE,
  ignored |-> FALSE,
  badSeen |-> TRUE,
  dispatches |-> 0,
  consumed |-> 1,
  attempts |-> 1,
  code |-> 1,
  replies |-> <<>> ]

State 3: Stuttering
Finished checking temporal properties in 00s at 2026-11-15 01:37:08
692 states generated, 516 distinct states found, 0 states left on queue.
Finished in 01s at (2026-11-15 01:37:08)
"""


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def declared(source, keyword):
    # The actual frozen configs declare their operator lists on one line.
    values = re.findall(rf"(?m)^\s*{keyword}S?\s+([^\n]+)$", source)
    return [name for value in values for name in value.split()]


class JsonNumbersRunnerTests(unittest.TestCase):
    def setUp(self):
        self.assertTrue((HERE / "check_model.py").is_file(),
                        "INTERFACE_PENDING: maintained JSON-number runner is missing")
        spec = importlib.util.spec_from_file_location("json_numbers_runner", HERE / "check_model.py")
        self.runner = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.runner)
        temporary = tempfile.TemporaryDirectory(prefix="lw-json-runner-test-")
        self.addCleanup(temporary.cleanup)
        self.directory = Path(temporary.name)
        self.jar = self.directory / "synthetic-tool.jar"
        self.jar.write_bytes(b"synthetic checker identity; Java is never executed")
        self.commands, self.overrides, self.exceptions = [], {}, {}
        self.bad_digest = None
        self.runs = 0

    def execute(self, command, log_path):
        self.commands.append((command, log_path))
        if "tla2sany.SANY" in command:
            name, code, text = "sany", 0, "Semantic processing of module JsonNumbers\n"
        else:
            name = Path(command[command.index("-config") + 1]).stem
            kind, target = CASE_MAP[name]
            if kind == "positive":
                code, text = 0, COMPLETE
            elif kind == "progress":
                code, text = 13, TEMPORAL_COUNTEREXAMPLE
            else:
                code, text = 12, f"Invariant {target} is violated.\n" + COMPLETE.split("\n", 1)[1]
        if name in self.exceptions:
            raise self.exceptions[name]
        code, text = self.overrides.get(name, (code, text))
        log_path.write_text(text, encoding="utf-8")
        return code, text, .001

    def invoke(self, *, output=None, extra=()):
        self.runs += 1
        self.output = output or self.directory / f"fresh-output-{self.runs}"
        self.commands = []
        original_digest = self.runner.digest

        def checked_digest(path):
            if Path(path) == self.bad_digest:
                return "0" * 64
            return JAR_SHA256 if Path(path) == self.jar.resolve() else original_digest(path)

        args = ["check_model.py", "--java", "synthetic-java", "--jar", str(self.jar),
                "--output", str(self.output), *extra]
        with patch.object(sys, "argv", args), \
             patch.object(self.runner, "digest", side_effect=checked_digest), \
             patch.object(self.runner, "execute", side_effect=self.execute), \
             patch.object(subprocess, "Popen", side_effect=AssertionError("unmocked executable boundary")), \
             contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            try:
                status = self.runner.main()
            except SystemExit as error:
                status = error.code
        receipt = self.output / "results.json"
        return status, json.loads(receipt.read_text(encoding="utf-8")) if receipt.is_file() else None

    def assert_failed(self, status):
        self.assertNotIn(status, (0, None), "runner accepted an incomplete or incorrect checker outcome")

    def isolated_runner(self, name, *, drift=False):
        root = self.directory / name
        here = root / "formal/cli/json-numbers"
        here.mkdir(parents=True)
        for source in HERE.iterdir():
            if source.name == "check_model.py" or source.suffix in {".tla", ".cfg"}:
                (here / source.name).write_bytes(source.read_bytes())
        source = root / "scripts/light_whisper_cli.py"
        source.parent.mkdir()
        original = (self.runner.ROOT / "scripts/light_whisper_cli.py").read_bytes()
        source.write_bytes(original + (b"\n# owned synthetic source drift\n" if drift else b""))
        spec = importlib.util.spec_from_file_location("isolated_json_numbers_runner", here / "check_model.py")
        runner = importlib.util.module_from_spec(spec)
        with patch.object(subprocess, "Popen", side_effect=AssertionError("checker launched during import")):
            spec.loader.exec_module(runner)
        return runner

    def test_complete_actual_matrix_runs_unfiltered_with_source_and_case_receipts(self):
        status, receipt = self.invoke()
        self.assertEqual(len(self.commands), 1 + len(EXPECTED_CASES), "SANY and all 15 configs must execute")
        self.assertIn(status, (0, None))
        self.assertIsNotNone(receipt, "successful matrix needs a complete receipt")
        self.assertEqual({path.stem for path in HERE.glob("*.cfg")}, set(CASE_MAP))
        source = self.runner.ROOT / "scripts/light_whisper_cli.py"
        self.assertEqual(self.runner.SOURCE_SHA256, digest(source), "review current source before advancing its binding")
        self.assertEqual(receipt["source"], {"path": "scripts/light_whisper_cli.py", "sha256": digest(source)})
        self.assertEqual(receipt["jar_sha256"], JAR_SHA256)
        self.assertEqual(receipt["runner_sha256"], digest(HERE / "check_model.py"))
        self.assertEqual(receipt["model_sha256"], digest(HERE / "JsonNumbers.tla"))
        inputs = {path.name: digest(path) for path in HERE.iterdir() if path.suffix in {".tla", ".cfg"}}
        self.assertEqual(receipt["inputs"], inputs)
        self.assertEqual(receipt["constraints"], [])
        self.assertIsNone(receipt["symmetry"])
        self.assertIs(receipt["deadlock_check"], True)
        temporary = (self.output / "java-tmp").resolve()
        self.assertTrue(temporary.is_dir())
        self.assertEqual(receipt["java_io_tmpdir"], str(temporary))
        prefix = ["synthetic-java", "-Djava.io.tmpdir=" + str(temporary), "-Xmx512m", "-cp", str(self.jar.resolve())]
        self.assertEqual(self.commands[0][0], prefix + ["tla2sany.SANY", "JsonNumbers.tla"])
        self.assertEqual(self.commands[0][1], self.output.resolve() / "sany.log")
        self.assertTrue(receipt["sany"]["expected"])
        self.assertEqual(receipt["sany"]["command"], self.commands[0][0])
        rows = {row["name"]: row for row in receipt["cases"]}
        self.assertEqual(len(receipt["cases"]), len(EXPECTED_CASES))
        self.assertEqual(set(rows), set(CASE_MAP))
        by_config = {Path(command[command.index("-config") + 1]).stem: (command, log)
                     for command, log in self.commands[1:]}
        self.assertEqual(set(by_config), set(CASE_MAP))
        for name, kind, target in EXPECTED_CASES:
            with self.subTest(case=name):
                row = rows[name]
                config = HERE / (name + ".cfg")
                invariants = declared(config.read_text(encoding="utf-8"), "INVARIANT")
                properties = declared(config.read_text(encoding="utf-8"), "PROPERTY")
                self.assertEqual((row["kind"], row["target"]), (kind, target))
                self.assertEqual(row["config_sha256"], digest(config))
                self.assertEqual(row["invariants"], invariants)
                self.assertEqual(row["properties"], properties)
                if kind == "positive":
                    self.assertCountEqual(invariants, POSITIVE_INVARIANTS)
                    self.assertCountEqual(properties, POSITIVE_PROPERTIES)
                    self.assertCountEqual(row["invariants"], POSITIVE_INVARIANTS)
                    self.assertCountEqual(row["properties"], POSITIVE_PROPERTIES)
                self.assertIn("TypeOK", invariants)
                self.assertTrue(row["expected"])
                self.assertEqual((row["generated"], row["distinct"], row["depth"]),
                                 (692, 516, 7) if kind == "progress" else (12, 7, 4))
                self.assertEqual(row["exit"], 0 if kind == "positive" else 13 if kind == "progress" else 12)
                if kind == "progress":
                    self.assertEqual(properties, [target])
                elif kind != "positive":
                    self.assertIn(target, invariants)
                command, log = by_config[name]
                self.assertEqual(command, prefix + ["tlc2.TLC", "-workers", "1", "-metadir",
                                 str(self.output.resolve() / ("states-" + name)), "-config", config.name, "JsonNumbers.tla"])
                self.assertEqual(row["command"], command)
                self.assertEqual(log, self.output.resolve() / (name + ".log"))
                self.assertTrue(log.is_file())

    def test_completed_temporal_counterexample_records_observed_progress_depth(self):
        name = "RejectedCleanupProgressNegative"
        for depth in (7, 11):
            with self.subTest(depth=depth):
                text = TEMPORAL_COUNTEREXAMPLE.replace("Progress(7)", f"Progress({depth})")
                self.overrides = {name: (13, text)}
                status, receipt = self.invoke()
                row = next(row for row in receipt["cases"] if row["name"] == name)
                self.assertEqual(row["exit"], 13)
                self.assertEqual((row["generated"], row["distinct"], row["depth"]), (692, 516, depth))
                self.assertTrue(row["expected"])
                self.assertIn(status, (0, None))
                self.assertEqual(len(self.commands), 1 + len(EXPECTED_CASES))
                self.assertEqual((self.output / (name + ".log")).read_text(encoding="utf-8"), text)

    def test_temporal_counterexample_rejects_missing_or_incomplete_completion_evidence(self):
        lines = TEMPORAL_COUNTEREXAMPLE.splitlines(keepends=True)
        progress = next(line for line in lines if line.startswith("Progress("))
        checking = next(line for line in lines if line.startswith("Checking temporal properties"))
        checked = next(line for line in lines if line.startswith("Finished checking temporal properties"))
        final_counts = next(line for line in lines if line.startswith("692 states generated"))
        finished = next(line for line in lines if line.startswith("Finished in"))
        incomplete = {
            "missing observed depth": TEMPORAL_COUNTEREXAMPLE.replace(progress, ""),
            "missing complete-space check": TEMPORAL_COUNTEREXAMPLE.replace(checking, ""),
            "partial-space check": TEMPORAL_COUNTEREXAMPLE.replace("complete state space", "current state space"),
            "missing temporal completion": TEMPORAL_COUNTEREXAMPLE.replace(checked, ""),
            "missing final counts": TEMPORAL_COUNTEREXAMPLE.replace(final_counts, ""),
            "missing progress queue evidence": TEMPORAL_COUNTEREXAMPLE.replace(
                progress, progress.replace(", 0 states left on queue", "")),
            "missing final queue evidence": TEMPORAL_COUNTEREXAMPLE.replace(
                final_counts, final_counts.replace(", 0 states left on queue", "")),
            "missing all queue evidence": TEMPORAL_COUNTEREXAMPLE.replace(", 0 states left on queue", ""),
            "queued states in progress": TEMPORAL_COUNTEREXAMPLE.replace(
                progress, progress.replace("0 states left on queue", "1 states left on queue")),
            "queued states in final counts": TEMPORAL_COUNTEREXAMPLE.replace(
                final_counts, final_counts.replace("0 states left on queue", "1 states left on queue")),
            "remaining queued states": TEMPORAL_COUNTEREXAMPLE.replace("0 states left on queue", "1 states left on queue"),
            "missing process completion": TEMPORAL_COUNTEREXAMPLE.replace(finished, ""),
            "truncated temporal trace": TEMPORAL_COUNTEREXAMPLE.split(checked)[0],
        }
        for reason, text in incomplete.items():
            with self.subTest(reason=reason):
                self.overrides = {"RejectedCleanupProgressNegative": (13, text)}
                status, receipt = self.invoke()
                self.assert_failed(status)
                row = receipt["cases"][-1]
                self.assertEqual(row["name"], "RejectedCleanupProgressNegative")
                self.assertFalse(row["expected"])

    def test_temporal_counterexample_rejects_wrong_exit_or_diagnostic_with_complete_counts(self):
        failures = [(code, TEMPORAL_COUNTEREXAMPLE) for code in (0, 1, 12, 124)]
        failures += [(13, TEMPORAL_COUNTEREXAMPLE.replace("Error: Temporal properties were violated.", diagnostic))
                     for diagnostic in ("Error: Invariant TypeOK is violated.", "Error: Deadlock reached.",
                                        "Error: Parsing failed.")]
        for code, text in failures:
            with self.subTest(code=code, diagnostic=text):
                self.overrides = {"RejectedCleanupProgressNegative": (code, text)}
                status, receipt = self.invoke()
                self.assert_failed(status)
                self.assertEqual(receipt["cases"][-1]["name"], "RejectedCleanupProgressNegative")
                self.assertFalse(receipt["cases"][-1]["expected"])

    def test_positive_progress_depth_cannot_replace_complete_graph_depth(self):
        text = ("Progress(4) at 2026-11-15 01:37:08: 12 states generated, 7 distinct states found, 0 states left on queue.\n"
                "Checking temporal properties for the complete state space with 7 total distinct states at (2026-11-15 01:37:08)\n"
                "Finished checking temporal properties in 00s at 2026-11-15 01:37:08\n"
                "Model checking completed. No error has been found.\n"
                "12 states generated, 7 distinct states found, 0 states left on queue.\n"
                "Finished in 01s at (2026-11-15 01:37:08)\n")
        self.overrides = {"JsonNumbers": (0, text)}
        status, receipt = self.invoke()
        self.assert_failed(status)
        row = receipt["cases"][-1]
        self.assertEqual(row["name"], "JsonNumbers")
        self.assertFalse(row["expected"])
        self.assertIsNone(row["depth"], "positive receipts require the complete-graph depth")

    def test_false_sany_success_stops_before_all_tlc_and_records_failure(self):
        for code, text in ((0, ""), (0, "Semantic processing of module OtherModule\n"),
                           (0, "Semantic processing of module JsonNumbers\nSemantic errors:\n"),
                           (1, "Semantic processing of module JsonNumbers\n")):
            with self.subTest(code=code, text=text):
                self.overrides = {"sany": (code, text)}
                status, receipt = self.invoke()
                self.assert_failed(status)
                self.assertEqual(len(self.commands), 1)
                self.assertFalse(receipt["sany"]["expected"])
                self.assertEqual(receipt["cases"], [])

    def test_wrong_negative_witness_progress_and_incomplete_positive_are_rejected(self):
        failures = (
            ("JsonNumbers", 0, "Incomplete model search\n"),
            ("JsonNumbers", 0, "Model checking completed. No error has been found.\n"),
            ("LegacyAcceptNegative", 12, "Invariant TypeOK is violated.\n"),
            ("LegacyOutputNegative", 0, "Invariant StrictOutput is violated.\n"),
            ("StartupWitness", 0, COMPLETE),
            ("ActionWitness", 12, "Invariant UnrelatedProperty is violated.\n"),
            ("RejectedCleanupProgressNegative", 12, "Temporal properties were violated.\n"),
            ("RejectedCleanupProgressNegative", 13, "Invariant TypeOK is violated.\n"),
            ("JsonNumbers", 124, "Timed out before exhaustive completion\n"),
        )
        for name, code, text in failures:
            with self.subTest(case=name, code=code):
                self.overrides = {name: (code, text)}
                status, receipt = self.invoke()
                self.assert_failed(status)
                self.assertEqual(receipt["cases"][-1]["name"], name)
                self.assertFalse(receipt["cases"][-1]["expected"])

    def test_checker_timeout_is_terminal_and_preserves_a_failed_receipt(self):
        for name in ("sany", "JsonNumbers", "RejectedCleanupProgressNegative"):
            with self.subTest(case=name):
                self.exceptions = {name: subprocess.TimeoutExpired("synthetic-java", 1)}
                status, receipt = self.invoke()
                self.assert_failed(status)
                row = receipt["sany"] if name == "sany" else receipt["cases"][-1]
                self.assertFalse(row["expected"])
                self.assertIs(row["timed_out"], True)
                if name == "sany":
                    self.assertEqual(len(self.commands), 1)
                    self.assertEqual(receipt["cases"], [])
                else:
                    self.assertEqual(row["name"], name)

    def test_named_targets_and_positive_progress_must_be_activated_in_actual_configs(self):
        original_read = Path.read_text
        changes = (("LegacyAcceptNegative.cfg", "MatchingBadIsTerminal", "UnrelatedProperty"),
                   ("RejectedCleanupProgressNegative.cfg", "PROPERTY EventuallyDone", "PROPERTY OtherProperty"),
                   *(("JsonNumbers.cfg", operator, "") for operator in POSITIVE_INVARIANTS),
                   ("JsonNumbers.cfg", "PROPERTY EventuallyDone", ""))
        for name, before, after in changes:
            with self.subTest(config=name, removed=before):
                def read(path, *args, **kwargs):
                    source = original_read(path, *args, **kwargs)
                    return re.sub(r"\b" + re.escape(before) + r"\b", after, source, count=1) if path == HERE / name else source

                with patch.object(Path, "read_text", read):
                    status, _ = self.invoke()
                self.assert_failed(status)
                self.assertNotIn(name, [arg for command, _ in self.commands for arg in command])

    def test_untrusted_jar_and_current_source_drift_fail_before_any_checker(self):
        for path in (self.jar, self.runner.ROOT / "scripts/light_whisper_cli.py"):
            with self.subTest(path=path):
                self.bad_digest = path.resolve()
                status, _ = self.invoke()
                self.assert_failed(status)
                self.assertEqual(self.commands, [])

    def test_cold_import_keeps_reviewed_binding_and_rejects_changed_source_before_checker(self):
        reviewed_binding = self.runner.SOURCE_SHA256
        isolated = self.isolated_runner("cold-import-source", drift=True)
        changed_source = isolated.ROOT / "scripts/light_whisper_cli.py"
        self.assertNotEqual(digest(changed_source), reviewed_binding)
        self.assertEqual(isolated.SOURCE_SHA256, reviewed_binding,
                         "cold import must retain the reviewed pin instead of computing a new source identity")
        with patch.object(self, "runner", isolated):
            status, _ = self.invoke()
        self.assert_failed(status)
        self.assertEqual(self.commands, [])

    def test_case_filters_are_refused_instead_of_publishing_partial_matrix_success(self):
        for extra in (("--only", "JsonNumbers"), ("--case", "JsonNumbers"), ("--sany-only",)):
            with self.subTest(extra=extra):
                status, _ = self.invoke(extra=extra)
                self.assert_failed(status)
                self.assertEqual(self.commands, [])

    def test_existing_evidence_is_preserved_and_never_reused_as_a_new_pass(self):
        output = self.directory / "existing-output"
        output.mkdir()
        original = output / "keep.json"
        original.write_text('{"preserve":true}', encoding="utf-8")
        status, _ = self.invoke(output=output)
        self.assert_failed(status)
        self.assertEqual(original.read_text(encoding="utf-8"), '{"preserve":true}')
        self.assertEqual(self.commands, [])

    def test_fresh_in_source_evidence_path_is_rejected_without_creation_or_checker(self):
        isolated = self.isolated_runner("isolated-output-source")
        output = isolated.ROOT / "fresh-evidence"
        self.assertFalse(output.exists())
        with patch.object(self, "runner", isolated):
            status, _ = self.invoke(output=output)
        self.assert_failed(status)
        self.assertEqual(self.commands, [])
        self.assertFalse(output.exists(), "refused in-source output must not be created")

    def test_executable_boundary_has_a_bounded_wait_and_retains_raw_diagnostics(self):
        log = self.directory / "synthetic-checker.log"
        command = ["synthetic-java", "-Xmx512m", "-cp", str(self.jar), "tlc2.TLC", "-workers", "1",
                   "-metadir", str(self.directory / "states"), "-config", "LegacyOutputNegative.cfg", "JsonNumbers.tla"]
        completed = subprocess.CompletedProcess(["synthetic-java"], 12, "raw stdout\n", "raw stderr\n")
        with patch.object(subprocess, "run", return_value=completed) as launch:
            code, text, _ = self.runner.execute(command, log)
        self.assertEqual(launch.call_count, 1, "runner must execute its declared checker boundary")
        self.assertEqual(launch.call_args.args, (command,))
        self.assertEqual(launch.call_args.kwargs["cwd"], HERE,
                         "relative model/config arguments require the maintained model directory")
        self.assertGreater(launch.call_args.kwargs["timeout"], 0)
        self.assertLessEqual(launch.call_args.kwargs["timeout"], 60)
        self.assertEqual(code, 12)
        self.assertEqual(text, "raw stdout\nraw stderr\n")
        self.assertEqual(log.read_text(encoding="utf-8"), text)


if __name__ == "__main__":
    unittest.main()
