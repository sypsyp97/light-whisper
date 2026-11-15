"""Mocked runner controls; these results are not SANY or TLC evidence."""
import contextlib
import importlib.util
import io
import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("controller_admission_runner", HERE / "check_model.py")
runner = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(runner)


class AdmissionRunnerTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.jar = self.root / "synthetic-tool.jar"
        self.jar.write_bytes(b"owned synthetic JAR identity; never executed")
        self.commands = []
        self.unrelated = False

    def execute(self, command, log_path):
        self.commands.append(command)
        if "tla2sany.SANY" in command:
            code, text = 0, "Semantic processing of module ControllerAdmission\n"
        else:
            cases = json.loads((HERE / "cases.json").read_text())
            case = next(row for row in cases if row["name"] == log_path.stem)
            if case["kind"] == "positive":
                code, text = 0, "Model checking completed. No error has been found.\n"
            elif case["kind"] == "progress":
                code, text = 13, "Temporal properties were violated.\n"
            else:
                target = "UnrelatedProperty" if self.unrelated else case["target"]
                code, text = 12, f"Invariant {target} is violated.\n"
            text += "12 states generated, 7 distinct states found\n"
            text += "The depth of the complete state graph search is 4\n"
        log_path.write_text(text, encoding="utf-8")
        return code, text, 0.001

    def invoke(self):
        actual_digest = runner.digest
        def digest(path):
            return runner.JAR_SHA256 if path == self.jar else actual_digest(path)
        args = ["check_model.py", "--java", "synthetic-java", "--jar", str(self.jar),
                "--output", str(self.root / "fresh-output")]
        with patch.object(sys, "argv", args), patch.object(runner, "digest", side_effect=digest), \
                patch.object(runner, "execute", side_effect=self.execute), \
                contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            result = runner.main()
        return result

    def test_full_inventory_runs_with_pinned_resources_and_receipts(self):
        self.invoke()
        cases = json.loads((HERE / "cases.json").read_text())
        self.assertEqual(len(self.commands), 1 + len(cases), "every declared case must execute")
        temporary = self.root / "fresh-output/java-tmp"
        self.assertEqual(self.commands[0][:5], ["synthetic-java", "-Djava.io.tmpdir=" + str(temporary),
                                              "-Xmx512m", "-cp", str(self.jar)])
        self.assertTrue(temporary.is_dir())
        self.assertIn("tla2sany.SANY", self.commands[0])
        receipt = json.loads((self.root / "fresh-output/results.json").read_text())
        self.assertEqual([row["name"] for row in receipt["cases"]], [row["name"] for row in cases])
        self.assertTrue(all(row["expected"] for row in receipt["cases"]))
        self.assertEqual(receipt["constraints"], [])
        self.assertIsNone(receipt["symmetry"])
        self.assertEqual(receipt["bounds"].get("optional_recording_claims_during_lifecycle_wait"), 1)
        self.assertEqual(receipt["bounds"].get("optional_startup_claims_during_lifecycle_wait"), 1)
        for command in self.commands[1:]:
            self.assertEqual(command[command.index("-workers") + 1], "1")
            self.assertNotIn("-coverage", command)

    def test_unrelated_named_violation_is_refused(self):
        self.unrelated = True
        with self.assertRaises(SystemExit):
            self.invoke()


if __name__ == "__main__":
    unittest.main()
