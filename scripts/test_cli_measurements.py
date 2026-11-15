"""Measurement runner selection and owned synthetic-process containment."""

import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time
from types import SimpleNamespace
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    "cli_mutation_runner", ROOT / "formal/cli/run_mutation_tests.py")
runner = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(runner)

CLI_TESTS = (
    "scripts.test_light_whisper_cli",
    "scripts.test_cli_bootstrap",
    "scripts.test_cli_configuration",
    "scripts.test_cli_session",
    "scripts.test_cli_target",
    "scripts.test_cli_native_threads",
    "scripts.test_cli_commands",
    "scripts.test_cli_json_numbers",
    "scripts.test_build_cli",
    "scripts.test_cli_measurement_gaps",
    "scripts.test_cli_measurements",
)
SDK_TESTS = (
    "test_native_thread_budget",
    "test_native_constructor_gaps",
    "test_qwen3_native.QwenNativeRuntimeTests",
    "test_qwen_cpu_cuda_backends.CpuCudaAbiTests",
    "test_r2t2_finish_context.FinishContextTests",
    "test_r2t2_startup.NativeStartupTests",
)


class MeasurementRunnerTests(unittest.TestCase):
    def invoke(self, *arguments, environment=None):
        with tempfile.TemporaryDirectory(prefix="lw-measurement-test-") as temporary:
            logs = Path(temporary) / "logs"
            child = mock.Mock()
            child.wait.return_value = 0
            child.poll.return_value = 0
            output = io.StringIO()
            with mock.patch.object(sys, "argv", ["run_mutation_tests.py", "--logs", str(logs),
                                                 *arguments]), \
                 mock.patch.object(runner.subprocess, "Popen", return_value=child) as launch, \
                 mock.patch.object(runner, "new_windows_job", create=True), \
                 mock.patch.dict(os.environ, environment or {}, clear=True), \
                 contextlib.redirect_stdout(output):
                code = runner.main()
            receipt = json.loads(output.getvalue())
            self.assertEqual(code, 0)
            self.assertEqual(receipt["exit"], 0)
            self.assertFalse(receipt["timeout"])
            self.assertTrue(receipt["root_reaped"])
            self.assertEqual(json.loads(next(logs.glob("*.json")).read_text()), receipt)
            return launch.call_args

    def test_default_keeps_the_running_virtual_environment_interpreter(self):
        with mock.patch.object(sys, "executable", "synthetic-venv-python"), \
             mock.patch.object(sys, "_base_executable", "synthetic-base-python", create=True):
            launched = self.invoke()
        self.assertEqual(launched.args[0][0], "synthetic-venv-python")

    def test_default_runs_the_entire_frozen_cli_and_constructor_cohort(self):
        launched = self.invoke()
        self.assertEqual(launched.args[0][1:4], ["-m", "unittest", "-f"])
        self.assertEqual(launched.args[0][4:], [*CLI_TESTS, *SDK_TESTS])

    def test_child_can_import_sdk_tests_without_inheriting_coverage_bootstrap(self):
        launched = self.invoke(environment={
            "PYTHONPATH": "synthetic-coverage-bootstrap", "COVERAGE_PROCESS_START": "wrong.ini",
            "COVERAGE_FILE": "wrong.data", "OMP_NUM_THREADS": "2",
        })
        environment = launched.kwargs["env"]
        self.assertEqual(environment.get("PYTHONPATH"), str(ROOT / "src-tauri/resources"))
        self.assertFalse(any(key.upper().startswith("COVERAGE_") for key in environment))
        self.assertEqual(environment["OMP_NUM_THREADS"], "2")

    def test_explicit_command_keeps_its_exact_arguments(self):
        command = ["synthetic-python", "-m", "unittest", "synthetic.module"]
        self.assertEqual(self.invoke("--", *command).args[0], command)


@unittest.skipUnless(os.name == "nt", "Windows kernel Job contract")
class MeasurementContainmentTests(unittest.TestCase):
    CHILD = r'''
import json
import os
from pathlib import Path
import subprocess
import sys
import threading

mode, report, release, argument = sys.argv[1:]
child = subprocess.Popen([sys.executable, '-c', 'import threading; threading.Event().wait()'],
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                         creationflags=subprocess.CREATE_NO_WINDOW)
report = Path(report)
temporary = report.with_suffix('.pending')
temporary.write_text(json.dumps({'root': os.getpid(), 'descendant': child.pid,
                                'argument': argument,
                                'environment': os.environ['LW_MEASUREMENT_SENTINEL']}))
temporary.replace(report)
while not Path(release).exists():
    threading.Event().wait(.01)
if mode == 'timeout':
    threading.Event().wait()
'''

    @classmethod
    def setUpClass(cls):
        spec = importlib.util.spec_from_file_location(
            "frozen_native_host_driver", ROOT / "formal/agent-api-host/run.py")
        cls.host = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cls.host)

    def wait_until(self, predicate, timeout=5):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            value = predicate()
            if value:
                return value
            threading.Event().wait(.01)  # observe explicit process/file barriers
        self.fail("owned fixture did not reach the requested process state")

    def wait_for_report(self, report):
        def read_ready():
            try:
                if report.is_file():
                    return (report.read_text(),)
            except (FileNotFoundError, PermissionError):
                return None

        text, = self.wait_until(read_ready)
        return json.loads(text)

    def exercise(self, mode):
        with tempfile.TemporaryDirectory(prefix="lw-measurement-process-") as temporary:
            directory = Path(temporary)
            script = directory / "synthetic child with spaces.py"
            script.write_text(self.CHILD, encoding="utf-8")
            report, release = directory / "ready.json", directory / "release"
            logs = directory / "runner-logs"
            watchdog = self.host.Job()
            processes = []
            outer = subprocess.Popen([
                sys.executable, str(ROOT / "formal/cli/run_mutation_tests.py"),
                "--timeout", "1.5" if mode == "timeout" else "5", "--logs", str(logs),
                "--", sys.executable, str(script), mode, str(report), str(release), "参数 with spaces",
            ], stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                env={**os.environ, "LW_MEASUREMENT_SENTINEL": "synthetic inherited value"},
                creationflags=subprocess.CREATE_NO_WINDOW)
            try:
                watchdog.add(outer)
                ready = self.wait_for_report(report)
                processes = [self.host.ExactProcess(ready[key]) for key in ("root", "descendant")]
                self.assertTrue(all(not process.reaped() for process in processes))
                self.assertIsNone(outer.poll(), "runner terminated its active selection")
                self.assertEqual(ready["argument"], "参数 with spaces")
                self.assertEqual(ready["environment"], "synthetic inherited value")
                release.touch()
                output, errors = outer.communicate(timeout=7)
                self.assertFalse(errors, errors.decode(errors="replace"))
                receipt = json.loads(output)
                self.assertEqual(receipt, json.loads(next(logs.glob("*.json")).read_text()))
                self.assertTrue(receipt["root_reaped"])
                self.assertIsNone(receipt["cleanup_error"])
                self.assertEqual(receipt["timeout"], mode == "timeout")
                self.assertEqual(outer.returncode, 1 if mode == "timeout" else 0)
                self.assertTrue(all(process.reaped() for process in processes),
                                "runner returned while an owned descendant remained live")
            finally:
                watchdog.close()
                if outer.poll() is None:
                    outer.kill()
                outer.wait(timeout=3)
                for pipe in (outer.stdout, outer.stderr):
                    pipe.close()
                for process in processes:
                    self.wait_until(process.reaped)
                    process.close()

    def test_normal_selection_exit_reaps_its_remaining_descendants(self):
        self.exercise("normal")

    def test_selection_timeout_reaps_its_root_and_descendants(self):
        self.exercise("timeout")


class SdkMutationSelectionTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="lw-mutation-selection-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        resources = self.root / "src-tauri/resources"
        resources.mkdir(parents=True)
        (resources / "r2t2_native.py").write_text(
            "class NativeRuntime:\n"
            "    def __init__(self, threads=None):\n"
            "        self.threads = threads\n"
            "    def close(self):\n"
            "        pass\n"
            "class OtherRuntime:\n"
            "    def __init__(self):\n"
            "        pass\n")
        (resources / "qwen3_native.py").write_text(
            "class QwenNativeRuntime:\n"
            "    def __init__(self, threads=None):\n"
            "        self.threads = threads\n"
            "class QwenPreviewRuntime:\n"
            "    def __init__(self):\n"
            "        pass\n")
        spec = importlib.util.spec_from_file_location(
            "sdk_mutation_selector", ROOT / "formal/cli/select_sdk_mutants.py")
        self.selector = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.selector)

    def item(self, job_id, module, start, end, definition="__init__"):
        mutation = SimpleNamespace(module_path=Path("src-tauri/resources") / module,
                                   start_pos=start, end_pos=end, definition_name=definition)
        return SimpleNamespace(job_id=job_id, mutations=(mutation,))

    def partition(self, items):
        try:
            return self.selector.partition(items, self.root)
        except NotImplementedError as error:
            self.fail(str(error))

    def test_partitions_all_job_ids_by_exact_class_and_complete_constructor_range(self):
        items = [
            self.item("r2t2-signature", "r2t2_native.py", (2, 31), (2, 35)),
            self.item("r2t2-body", "r2t2_native.py", (3, 23), (3, 30)),
            self.item("r2t2-other-init", "r2t2_native.py", (8, 8), (8, 12)),
            self.item("qwen-body", "qwen3_native.py", (3, 23), (3, 30)),
            self.item("qwen-preview-init", "qwen3_native.py", (6, 8), (6, 12)),
            self.item("r2t2-close", "r2t2_native.py", (5, 8), (5, 12), "close"),
        ]
        selected, outside = self.partition(items)
        self.assertEqual([item.job_id for item in selected],
                         ["r2t2-signature", "r2t2-body", "qwen-body"])
        self.assertEqual([item.job_id for item in outside],
                         ["r2t2-other-init", "qwen-preview-init", "r2t2-close"])
        self.assertEqual({item.job_id for item in selected + outside},
                         {item.job_id for item in items})
        self.assertIs(selected[0], items[0], "retain original framework IDs and mutation specs")

    def test_rejects_cross_boundary_mutations_instead_of_hiding_their_impact(self):
        item = self.item("overlap", "r2t2_native.py", (3, 8), (5, 12))
        with self.assertRaises(ValueError):
            self.partition([item])

    def test_rejects_duplicate_job_ids_before_partitioning(self):
        item = self.item("duplicate", "r2t2_native.py", (3, 23), (3, 30))
        with self.assertRaises(ValueError):
            self.partition([item, item])

    def test_rejects_unexpected_modules_in_the_frozen_sdk_session(self):
        item = self.item("wrong-module", "unrelated.py", (1, 0), (1, 4))
        with self.assertRaises(ValueError):
            self.partition([item])


if __name__ == "__main__":
    unittest.main()
