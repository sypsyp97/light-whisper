"""Collector owns its fresh output directory before activating parent coverage."""
from contextlib import ExitStack, redirect_stderr, redirect_stdout
import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    "cli_context_collector_ownership", ROOT / "formal/cli/context_measurements.py")
collector = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(collector)
TEST_ID = "synthetic.collector.Case.test_owned"


class CollectorOwnershipTests(unittest.TestCase):
    def exercise(self, output, *, existing_parent=False, test_failure=False, collection_error=False):
        events = []
        coverage = mock.Mock()
        active = mock.Mock()
        active.config.data_file = str(output / ".coverage")
        parent = mock.Mock() if existing_parent else None
        if parent is not None:
            parent.config.data_file = str(output / ".coverage")
        coverage.Coverage.current.return_value = parent
        combined = mock.Mock()
        state = {"sources": {path: "synthetic frozen source\n" for path in collector.SOURCES},
                 "bindings": {"files": {"test.py": "frozen"}, "runtime": {"python": "synthetic"}}}
        report = {"meta": {"branch_coverage": True}, "totals": {"num_statements": 4},
                  "files": {path: {"contexts": {"1": [TEST_ID]}} for path in collector.SOURCES}}
        constructions = []

        def construct(*args, **kwargs):
            self.assertFalse(args)
            self.assertEqual(kwargs["config_file"], str(ROOT / "formal/cli/coverage.ini"))
            self.assertEqual(kwargs["data_file"], str(output.resolve() / ".coverage"))
            self.assertTrue(output.is_dir(), "coverage activation preceded owned directory creation")
            constructions.append(kwargs)
            events.append("construct")
            if len(constructions) == 1 and parent is None:
                # Models coverage's SQL/startup output creation at construction.
                (output / ".coverage.synthetic-owned").write_bytes(b"owned coverage data")
                return active
            return combined

        def start():
            self.assertTrue(output.is_dir())
            events.append("start")
            coverage.Coverage.current.return_value = active

        def switch(context):
            events.append(("context", context))

        def snapshot(root, environment):
            self.assertEqual(root, ROOT)
            events.append("snapshot")
            return state

        def load(root):
            self.assertEqual(root, ROOT)
            events.append("load")
            self.assertTrue(output.is_dir())
            if parent is None:
                self.assertEqual(os.environ.get("COVERAGE_PROCESS_START"), str(ROOT / "formal/cli/coverage.ini"))
                self.assertEqual(os.environ.get("COVERAGE_FILE"), str(output.resolve() / ".coverage"))
                self.assertEqual(os.environ.get("COVERAGE_CORE"), "ctrace")
                self.assertEqual(os.environ.get("PYTHONDONTWRITEBYTECODE"), "1")
                self.assertEqual(os.environ.get("OMP_NUM_THREADS"), "2")
                self.assertIn(("context", "<outside-test>"), events)
            if collection_error:
                raise RuntimeError("synthetic collection failure")

            def test():
                if test_failure:
                    raise AssertionError("synthetic failed cohort assertion")

            case = unittest.FunctionTestCase(test)
            case.id = lambda: TEST_ID
            return unittest.TestSuite([case]), [TEST_ID]

        def json_report(*, outfile, show_contexts):
            self.assertTrue(show_contexts)
            Path(outfile).write_text(json.dumps(report), encoding="utf-8")

        coverage.Coverage.side_effect = construct
        active.start.side_effect = start
        active.stop.side_effect = lambda: events.append("stop")
        active.switch_context.side_effect = switch
        combined.json_report.side_effect = json_report
        combined.report.side_effect = lambda *, file, show_missing: file.write("synthetic report\n")
        with ExitStack() as patches:
            patches.enter_context(mock.patch.dict(sys.modules, {"coverage": coverage}))
            patches.enter_context(mock.patch.dict(os.environ, {
                "COVERAGE_CORE": "ctrace", "PYTHONDONTWRITEBYTECODE": "1", "OMP_NUM_THREADS": "2",
                "PYTHONPATH": str(ROOT / "formal/cli/coverage_startup"),
            }, clear=True))
            patches.enter_context(mock.patch.object(sys, "argv", ["context_measurements.py", str(output)]))
            patches.enter_context(mock.patch.object(collector, "snapshot", side_effect=snapshot))
            patches.enter_context(mock.patch.object(collector, "load_suite", side_effect=load))
            patches.enter_context(mock.patch.object(sys, "path", list(sys.path)))
            patches.enter_context(redirect_stdout(io.StringIO()))
            patches.enter_context(redirect_stderr(io.StringIO()))
            try:
                outcome = collector.main()
            except (SystemExit, RuntimeError, ValueError, FileExistsError) as error:
                outcome = error
        return SimpleNamespace(outcome=outcome, events=events, active=active, parent=parent,
                               coverage=coverage, combined=combined, constructions=constructions)

    def test_fresh_directory_precedes_parent_coverage_construction_and_start(self):
        with tempfile.TemporaryDirectory(prefix="lw-context-owned-") as temporary:
            output = Path(temporary) / "fresh-evidence"
            result = self.exercise(output)
            self.assertEqual(result.outcome, 0)
            self.assertEqual(result.events[:3], ["construct", "start", ("context", "<outside-test>")])
            self.assertLess(result.events.index("start"), result.events.index("snapshot"))
            self.assertLess(result.events.index("snapshot"), result.events.index("load"))
            result.active.start.assert_called_once_with()
            result.active.stop.assert_called_once_with()
            result.active.save.assert_called_once_with()
            self.assertEqual(len(result.constructions), 2)
            self.assertTrue((output / ".coverage.synthetic-owned").is_file())

    def test_child_coverage_flags_and_successful_full_cohort_produce_bound_dataset(self):
        with tempfile.TemporaryDirectory(prefix="lw-context-owned-") as temporary:
            output = Path(temporary) / "fresh-evidence"
            result = self.exercise(output)
            self.assertEqual(result.outcome, 0)
            dataset = json.loads((output / "context-dataset.json").read_text())
            self.assertTrue(dataset["complete"])
            self.assertEqual(dataset["tests"], [TEST_ID])
            self.assertEqual(set(dataset["sources"]), set(collector.SOURCES))
            self.assertEqual(set(dataset["test_elapsed"]), {TEST_ID})
            receipt = json.loads((output / "receipt.json").read_text())
            self.assertEqual(receipt["tests"], 1)
            self.assertEqual(set(receipt["source_paths"]), set(collector.SOURCES))
            result.combined.combine.assert_called_once_with(data_paths=[str(output.resolve())], keep=True)

    def test_preexisting_payload_is_rejected_without_reusing_or_modifying_it(self):
        for directory in (False, True):
            with self.subTest(directory=directory), tempfile.TemporaryDirectory(prefix="lw-context-owned-") as temporary:
                output = Path(temporary) / "existing"
                if directory:
                    output.mkdir()
                    sentinel = output / "sentinel.bin"
                else:
                    sentinel = output
                sentinel.write_bytes(b"existing user-owned evidence")
                result = self.exercise(output)
                self.assertNotEqual(result.outcome, 0)
                self.assertEqual(sentinel.read_bytes(), b"existing user-owned evidence")
                if directory:
                    self.assertEqual(list(output.iterdir()), [sentinel])
                result.coverage.Coverage.assert_not_called()
                self.assertNotIn("snapshot", result.events)
                self.assertNotIn("load", result.events)

    def test_existing_parent_coverage_is_rejected_before_output_or_collection(self):
        with tempfile.TemporaryDirectory(prefix="lw-context-owned-") as temporary:
            output = Path(temporary) / "fresh-evidence"
            result = self.exercise(output, existing_parent=True)
            self.assertNotEqual(result.outcome, 0)
            self.assertFalse(output.exists())
            result.coverage.Coverage.assert_not_called()
            result.parent.stop.assert_not_called()
            result.parent.save.assert_not_called()
            self.assertNotIn("load", result.events)

    def test_failed_cohort_stops_owned_coverage_and_cannot_write_trusted_dataset(self):
        with tempfile.TemporaryDirectory(prefix="lw-context-owned-") as temporary:
            output = Path(temporary) / "fresh-evidence"
            result = self.exercise(output, test_failure=True)
            self.assertNotEqual(result.outcome, 0)
            result.active.start.assert_called_once_with()
            result.active.stop.assert_called_once_with()
            self.assertFalse((output / "context-dataset.json").exists())
            self.assertFalse((output / "receipt.json").exists())

    def test_collection_exception_stops_owned_coverage_and_cannot_write_trusted_dataset(self):
        with tempfile.TemporaryDirectory(prefix="lw-context-owned-") as temporary:
            output = Path(temporary) / "fresh-evidence"
            result = self.exercise(output, collection_error=True)
            self.assertNotEqual(result.outcome, 0)
            result.active.start.assert_called_once_with()
            result.active.stop.assert_called_once_with()
            self.assertFalse((output / "context-dataset.json").exists())
            self.assertFalse((output / "receipt.json").exists())


if __name__ == "__main__":
    unittest.main(verbosity=2)
