"""Pure selection contracts and subprocess-coverage startup with mocked coverage."""

import copy
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import runpy
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    "cli_context_selector", ROOT / "formal/cli/select_context_tests.py")
selector = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(selector)
COLLECTOR_SPEC = importlib.util.spec_from_file_location(
    "cli_context_collector", ROOT / "formal/cli/context_measurements.py")
collector = importlib.util.module_from_spec(COLLECTOR_SPEC)
COLLECTOR_SPEC.loader.exec_module(collector)
TESTS = ["scripts.test_owned.OwnedTests.test_first",
         "test_native_owned.NativeTests.test_second",
         "scripts.test_owned.OwnedTests.test_third"]
SOURCES = ("scripts/light_whisper_cli.py", "scripts/build_cli.py",
           "src-tauri/resources/r2t2_native.py", "src-tauri/resources/qwen3_native.py")
PRIMARY = SOURCES[0]
SOURCE = ("import math\n"
          "CONSTANT = 1\n"
          "def command(value=1):\n"
          "    if value > 0:\n"
          "        return value + 1\n"
          "    return 0\n"
          "def other():\n"
          "    return 2\n")
METHOD_SOURCE = ("class Runtime:\n"
                 "    FAMILY = 1\n"
                 "    def __init__(self, model,\n"
                 "                 library, *,\n"
                 "                 backend='cpu',\n"
                 "                 threads=None):\n"
                 "        self.threads = 2\n")
NESTED_CLASS_SOURCE = ("def factory():\n"
                       "    seed = 1\n"
                       "    @decorate(1)\n"
                       "    class Adapter(Base1):\n"
                       "        FAMILY = 1\n"
                       "        @decorate(1)\n"
                       "        def method(self, value=1):\n"
                       "            return value + 1\n"
                       "    return Adapter\n")
NESTED_FUNCTION_SOURCE = ("def outer():\n"
                          "    seed = 1\n"
                          "    @decorate(1)\n"
                          "    def inner(\n"
                          "            value=1):\n"
                          "        return value + 1\n"
                          "    return inner\n")


def digest(text):
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


class ContextSelectionTests(unittest.TestCase):
    def setUp(self):
        self.bindings = {
            "files": {"scripts/test_owned.py": "test-file-hash",
                      "formal/cli/coverage.ini": "configuration-hash",
                      "formal/cli/select_context_tests.py": "helper-hash",
                      "src-tauri/resources/dependency.py": "module-dependency-hash"},
            "runtime": {"python": "3.11.11", "coverage": "7.17.3",
                        "cosmic-ray": "8.9.0", "numpy": "installed-version",
                        "PyInstaller": "installed-version", "platform": "win32",
                        "environment": {"OMP_NUM_THREADS": "2", "HF_HUB_OFFLINE": "1"}},
        }
        self.dataset = {
            "schema": 1, "complete": True, "tests": list(TESTS), "bindings": copy.deepcopy(self.bindings),
            "sources": {PRIMARY: {
                "text": SOURCE, "sha256": digest(SOURCE),
                "contexts": {"1": [TESTS[0]], "2": [TESTS[0]], "3": [TESTS[0]],
                             "4": [TESTS[1], TESTS[0], TESTS[1]],
                             "5": [TESTS[0]], "6": [TESTS[2]], "8": [TESTS[1]]},
            }},
        }
        self.dataset["sources"] = {
            path: copy.deepcopy(self.dataset["sources"][PRIMARY]) for path in SOURCES}

    def select(self, source=None, *, dataset=None, bindings=None, full_ids=None, sources=None):
        if sources is None:
            sources = {path: SOURCE for path in SOURCES}
            sources[PRIMARY] = SOURCE if source is None else source
        try:
            return selector.select_tests(
                self.dataset if dataset is None else dataset, sources,
                self.bindings if bindings is None else bindings,
                TESTS if full_ids is None else full_ids)
        except NotImplementedError as error:
            self.fail(str(error))

    def assert_full(self, result, expected=TESTS):
        self.assertTrue(result["full"])
        self.assertEqual(result["tests"], expected)
        self.assertIsInstance(result["reason"], str)
        self.assertTrue(result["reason"])

    def select_fixture_line(self, original, changed, line):
        dataset = copy.deepcopy(self.dataset)
        dataset["sources"][PRIMARY] = {
            "text": original, "sha256": digest(original),
            "contexts": {str(line): [TESTS[1], TESTS[0], TESTS[1]]},
        }
        return self.select(changed, dataset=dataset)

    def test_selects_every_covering_id_once_in_full_cohort_order(self):
        result = self.select(SOURCE.replace("value > 0", "value >= 0"))
        self.assertFalse(result["full"])
        self.assertEqual(result["tests"], TESTS[:2])
        self.assertTrue(result["reason"])

    def test_selects_a_body_return_without_adding_unrelated_test_ids(self):
        result = self.select(SOURCE.replace("value + 1", "value - 1"))
        self.assertFalse(result["full"])
        self.assertEqual(result["tests"], TESTS[:1])

    def test_method_body_selects_all_covering_ids_under_a_multiline_signature(self):
        result = self.select_fixture_line(
            METHOD_SOURCE, METHOD_SOURCE.replace("self.threads = 2", "self.threads = 3"), 7)
        self.assertFalse(result["full"])
        self.assertEqual(result["tests"], TESTS[:2])

    def test_constructor_continuation_line_defaults_run_full_with_known_contexts(self):
        for old, new, line in (("backend='cpu'", "backend='cuda'", 5),
                               ("threads=None", "threads=4", 6)):
            with self.subTest(line=line):
                self.assert_full(self.select_fixture_line(
                    METHOD_SOURCE, METHOD_SOURCE.replace(old, new), line))

    def test_class_initialization_runs_full_with_known_contexts(self):
        self.assert_full(self.select_fixture_line(
            METHOD_SOURCE, METHOD_SOURCE.replace("FAMILY = 1", "FAMILY = 2"), 2))

    def test_imports_inside_functions_run_full_with_known_contexts(self):
        for original, changed in (
            ("def command():\n    import math\n    return math.pi\n",
             "def command():\n    import cmath\n    return math.pi\n"),
            ("def command():\n    from math import sqrt\n    return sqrt(4)\n",
             "def command():\n    from math import floor\n    return sqrt(4)\n"),
        ):
            with self.subTest(original=original):
                self.assert_full(self.select_fixture_line(original, changed, 2))

    def test_one_line_function_header_and_body_run_full_with_known_contexts(self):
        original = "def command(): return 1\n"
        self.assert_full(self.select_fixture_line(original, original.replace("return 1", "return 2"), 1))

    def test_nested_class_initialization_and_declarations_run_full_with_known_contexts(self):
        for old, new, line in (("    @decorate(1)", "    @decorate(2)", 3),
                               ("Adapter(Base1)", "Adapter(Base2)", 4),
                               ("FAMILY = 1", "FAMILY = 2", 5),
                               ("        @decorate(1)", "        @decorate(2)", 6),
                               ("value=1", "value=2", 7)):
            with self.subTest(line=line):
                self.assert_full(self.select_fixture_line(
                    NESTED_CLASS_SOURCE, NESTED_CLASS_SOURCE.replace(old, new, 1), line))

    def test_nested_function_decorators_and_defaults_run_full_with_known_contexts(self):
        for old, new, line in (("@decorate(1)", "@decorate(2)", 3),
                               ("value=1", "value=2", 5)):
            with self.subTest(line=line):
                self.assert_full(self.select_fixture_line(
                    NESTED_FUNCTION_SOURCE, NESTED_FUNCTION_SOURCE.replace(old, new), line))

    def test_nested_method_and_function_bodies_still_select_all_covering_ids(self):
        for source, line in ((NESTED_CLASS_SOURCE, 8), (NESTED_FUNCTION_SOURCE, 6)):
            with self.subTest(line=line):
                result = self.select_fixture_line(source, source.replace("value + 1", "value - 1"), line)
                self.assertFalse(result["full"])
                self.assertEqual(result["tests"], TESTS[:2])

    def test_unmutated_baseline_runs_the_complete_cohort(self):
        self.assert_full(self.select())

    def test_import_constant_and_function_signature_changes_run_full(self):
        for old, new in (("import math", "import sys"), ("CONSTANT = 1", "CONSTANT = 2"),
                         ("command(value=1)", "command(value=2)")):
            with self.subTest(old=old):
                self.assert_full(self.select(SOURCE.replace(old, new)))

    def test_insert_delete_multiline_and_multihunk_changes_run_full(self):
        variants = [
            SOURCE.replace("        return value + 1\n", "        pass\n        return value + 1\n"),
            SOURCE.replace("        return value + 1\n", ""),
            SOURCE.replace("    if value > 0:\n        return value + 1",
                           "    if value >= 0:\n        return value - 1"),
            SOURCE.replace("value > 0", "value >= 0").replace("return 2", "return 3"),
        ]
        for source in variants:
            with self.subTest(source=source):
                self.assert_full(self.select(source))

    def test_unknown_empty_missing_and_outside_contexts_run_full(self):
        for contexts in ([], ["unrecognized.Test.test_case"], [""], ["<outside-test>"],
                         [TESTS[0], "<outside-test>"], None):
            dataset = copy.deepcopy(self.dataset)
            if contexts is None:
                del dataset["sources"][PRIMARY]["contexts"]["5"]
            else:
                dataset["sources"][PRIMARY]["contexts"]["5"] = contexts
            with self.subTest(contexts=contexts):
                self.assert_full(self.select(SOURCE.replace("value + 1", "value - 1"), dataset=dataset))

    def test_every_file_runtime_tool_dependency_or_environment_binding_change_runs_full(self):
        changes = [
            ("files", "scripts/test_owned.py", "changed-test"),
            ("files", "formal/cli/coverage.ini", "changed-config"),
            ("files", "formal/cli/select_context_tests.py", "changed-helper"),
            ("files", "src-tauri/resources/dependency.py", "changed-module"),
            ("runtime", "python", "changed-python"), ("runtime", "coverage", "changed-tool"),
            ("runtime", "cosmic-ray", "changed-tool"), ("runtime", "numpy", "changed-dependency"),
            ("runtime", "PyInstaller", "changed-dependency"), ("runtime", "platform", "changed-platform"),
            ("runtime", "environment", {"OMP_NUM_THREADS": "9", "HF_HUB_OFFLINE": "1"}),
        ]
        for section, key, value in changes:
            bindings = copy.deepcopy(self.bindings)
            bindings[section][key] = value
            with self.subTest(section=section, key=key):
                self.assert_full(self.select(SOURCE.replace("value + 1", "value - 1"), bindings=bindings))
        bindings = copy.deepcopy(self.bindings)
        bindings["files"]["newly-used.py"] = "new-dependency"
        self.assert_full(self.select(SOURCE.replace("value + 1", "value - 1"), bindings=bindings))

    def test_tampered_baseline_text_or_source_hash_runs_full(self):
        for key, value in (("sha256", "invalid-hash"), ("text", SOURCE.replace("return 2", "return 3"))):
            dataset = copy.deepcopy(self.dataset)
            dataset["sources"][PRIMARY][key] = value
            with self.subTest(key=key):
                self.assert_full(self.select(SOURCE.replace("value + 1", "value - 1"), dataset=dataset))

    def test_each_unchanged_source_hash_is_verified_before_selecting(self):
        for path in SOURCES[1:]:
            dataset = copy.deepcopy(self.dataset)
            dataset["sources"][path]["sha256"] = "invalid-unchanged-source-hash"
            with self.subTest(path=path):
                self.assert_full(self.select(SOURCE.replace("value + 1", "value - 1"), dataset=dataset))

    def test_added_current_test_is_included_in_full_fallback(self):
        full_ids = [*TESTS, "scripts.test_new.NewTests.test_new"]
        self.assert_full(self.select(SOURCE.replace("value + 1", "value - 1"), full_ids=full_ids), full_ids)

    def test_unknown_removed_or_reordered_frozen_test_id_runs_full(self):
        for frozen in ([*TESTS, "scripts.test_removed.OldTests.test_old"], TESTS[:-1], list(reversed(TESTS))):
            dataset = copy.deepcopy(self.dataset)
            dataset["tests"] = frozen
            with self.subTest(frozen=frozen):
                self.assert_full(self.select(SOURCE.replace("value + 1", "value - 1"), dataset=dataset))

    def test_missing_or_multiple_changed_source_files_run_full(self):
        baseline = {path: SOURCE for path in SOURCES}
        for sources in (
            {PRIMARY: SOURCE.replace("value + 1", "value - 1")},
            {**baseline, PRIMARY: SOURCE.replace("value + 1", "value - 1"),
             SOURCES[1]: SOURCE.replace("return 2", "return 3")},
            {**baseline, PRIMARY: SOURCE.replace("value + 1", "value - 1"), "unexpected.py": SOURCE},
        ):
            with self.subTest(sources=sources):
                self.assert_full(self.select(sources=sources))
        dataset = copy.deepcopy(self.dataset)
        del dataset["sources"][SOURCES[-1]]
        self.assert_full(self.select(SOURCE.replace("value + 1", "value - 1"), dataset=dataset))

    def test_syntax_invalid_mutation_runs_full(self):
        self.assert_full(self.select(SOURCE.replace("value + 1", "value +")))

    def test_untrusted_dataset_schema_or_missing_bindings_run_full(self):
        for field, value in (("schema", 999), ("complete", False), ("bindings", {}), ("sources", {})):
            dataset = copy.deepcopy(self.dataset)
            dataset[field] = value
            with self.subTest(field=field):
                self.assert_full(self.select(SOURCE.replace("value + 1", "value - 1"), dataset=dataset))

    def test_empty_or_duplicated_current_cohort_is_an_error_not_a_passing_empty_run(self):
        for full_ids in ([], [TESTS[0], TESTS[0]]):
            with self.subTest(full_ids=full_ids), self.assertRaises(ValueError):
                self.select(SOURCE.replace("value + 1", "value - 1"), full_ids=full_ids)


class CoverageStartupContextTests(unittest.TestCase):
    def run_startup(self, environment):
        coverage = mock.Mock()
        with mock.patch.dict(sys.modules, {"coverage": coverage}), \
             mock.patch.dict(os.environ, environment, clear=True):
            runpy.run_path(str(ROOT / "formal/cli/coverage_startup/sitecustomize.py"))
        return coverage

    def test_child_startup_uses_the_inherited_test_identity(self):
        coverage = self.run_startup({"COVERAGE_PROCESS_START": "synthetic-coverage.ini",
                                     "LIGHT_WHISPER_COVERAGE_CONTEXT": TESTS[1]})
        coverage.process_startup.assert_called_once_with()
        coverage.Coverage.current.return_value.switch_context.assert_called_once_with(TESTS[1])

    def test_startup_outside_a_test_is_explicitly_unattributed(self):
        coverage = self.run_startup({"COVERAGE_PROCESS_START": "synthetic-coverage.ini"})
        coverage.process_startup.assert_called_once_with()
        coverage.Coverage.current.return_value.switch_context.assert_called_once_with("<outside-test>")

    def test_without_coverage_startup_settings_no_context_hook_runs(self):
        coverage = self.run_startup({"LIGHT_WHISPER_COVERAGE_CONTEXT": TESTS[1]})
        coverage.process_startup.assert_not_called()
        coverage.Coverage.current.assert_not_called()


class CollectorSafetyTests(unittest.TestCase):
    def invoke(self, function, *arguments, **keywords):
        try:
            return function(*arguments, **keywords)
        except NotImplementedError as error:
            self.fail(str(error))

    def test_parent_context_lifecycle_restores_owned_environment(self):
        active = mock.Mock()
        environment = {"LIGHT_WHISPER_COVERAGE_CONTEXT": "outer-context", "KEEP": "value"}
        result = collector.ContextResult(io.StringIO(), False, 0, active=active, environment=environment)
        test = mock.Mock()
        test.id.return_value = TESTS[0]
        self.invoke(result.startTest, test)
        active.switch_context.assert_called_with(TESTS[0])
        self.assertEqual(environment["LIGHT_WHISPER_COVERAGE_CONTEXT"], TESTS[0])
        self.invoke(result.stopTest, test)
        active.switch_context.assert_called_with("<outside-test>")
        self.assertEqual(environment, {"LIGHT_WHISPER_COVERAGE_CONTEXT": "outer-context", "KEEP": "value"})
        self.assertEqual(result.testsRun, 1)

    def test_parent_context_requires_active_coverage(self):
        result = collector.ContextResult(io.StringIO(), False, 0, active=None, environment={})
        with self.assertRaises(RuntimeError):
            self.invoke(result.startTest, mock.Mock())

    def test_fingerprints_all_local_callers_helpers_tests_and_configuration(self):
        with tempfile.TemporaryDirectory(prefix="lw-context-inputs-") as temporary:
            root = Path(temporary)
            paths = [*SOURCES, "scripts/dependency.py", "scripts/test_extra.py",
                     "src-tauri/resources/helper.py", "formal/cli/helper.py", "package.json", "uv.lock"]
            for path in paths:
                target = root / path
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes((SOURCE if path in SOURCES else "original").encode())
            listing = b"\0".join(path.encode() for path in paths)
            distribution = SimpleNamespace(metadata={"Name": "synthetic-package"}, version="1.0")
            environment = {"SYNTHETIC_SECRET": "do-not-record-this-value", "OMP_NUM_THREADS": "2"}
            with mock.patch.object(collector.subprocess, "check_output", return_value=listing), \
                 mock.patch.object(collector.metadata, "distributions", return_value=[distribution]):
                before = self.invoke(collector.snapshot, root, environment)
                self.assertEqual(before["sources"], {path: SOURCE for path in SOURCES})
                self.assertTrue(set(paths) - set(SOURCES) <= set(before["bindings"]["files"]))
                self.assertTrue(set(SOURCES).isdisjoint(before["bindings"]["files"]))
                self.assertNotIn("do-not-record-this-value", json.dumps(before))
                self.assertIn(["synthetic-package", "1.0"], before["bindings"]["runtime"]["distributions"])
                for path in paths[len(SOURCES):]:
                    (root / path).write_text("changed")
                    after = self.invoke(collector.snapshot, root, environment)
                    self.assertNotEqual(before["bindings"]["files"][path], after["bindings"]["files"][path])
                    (root / path).write_text("original")
                changed = self.invoke(collector.snapshot, root, {**environment, "OMP_NUM_THREADS": "3"})
                self.assertNotEqual(before["bindings"], changed["bindings"])

    def dataset_inputs(self):
        fixture = ContextSelectionTests("runTest")
        fixture.setUp()
        state = {"sources": {path: SOURCE for path in SOURCES}, "bindings": fixture.bindings}
        report = {"meta": {"branch_coverage": True},
                  "files": {path.replace("/", "\\"): {"contexts": {"5": [TESTS[0], "<outside-test>"]}}
                            for path in SOURCES}}
        return state, report

    def test_dataset_retains_all_four_sources_and_unattributed_contexts(self):
        before, report = self.dataset_inputs()
        result = self.invoke(collector.build_dataset, before, copy.deepcopy(before), report, TESTS, True)
        self.assertTrue(result["complete"])
        self.assertEqual(result["tests"], TESTS)
        self.assertEqual(result["bindings"], before["bindings"])
        self.assertEqual(set(result["sources"]), set(SOURCES))
        for path in SOURCES:
            self.assertEqual(result["sources"][path]["sha256"], digest(SOURCE))
            self.assertEqual(result["sources"][path]["contexts"]["5"], [TESTS[0], "<outside-test>"])

    def test_failed_drifted_or_incomplete_collection_cannot_produce_a_trusted_dataset(self):
        before, report = self.dataset_inputs()
        scenarios = []
        after = copy.deepcopy(before)
        after["sources"][SOURCES[-1]] += "\nCHANGED = True\n"
        scenarios.append((after, report, TESTS, True))
        after = copy.deepcopy(before)
        after["bindings"]["files"]["scripts/test_owned.py"] = "changed-test"
        scenarios.extend([(after, report, TESTS, True), (before, report, TESTS, False),
                          (before, report, [], True), (before, report, [TESTS[0], TESTS[0]], True)])
        missing = copy.deepcopy(report)
        del missing["files"][SOURCES[-1].replace("/", "\\")]
        scenarios.append((before, missing, TESTS, True))
        uninstrumented = copy.deepcopy(report)
        del uninstrumented["files"][SOURCES[0].replace("/", "\\")]["contexts"]
        scenarios.append((before, uninstrumented, TESTS, True))
        for arguments in scenarios:
            with self.subTest(arguments=arguments), self.assertRaises(ValueError):
                self.invoke(collector.build_dataset, before, *arguments)

    def test_full_suite_collects_cli_context_contracts_and_every_resource_test(self):
        first, second = unittest.FunctionTestCase(lambda: None), unittest.FunctionTestCase(lambda: None)
        first.id = lambda: TESTS[0]
        second.id = lambda: TESTS[1]
        loader = mock.Mock(errors=[])
        loader.loadTestsFromNames.side_effect = lambda names: unittest.TestSuite([first])
        loader.discover.side_effect = lambda *args, **kwargs: unittest.TestSuite([second])
        with mock.patch.object(collector.unittest, "TestLoader", return_value=loader):
            suite, ids = self.invoke(collector.load_suite, ROOT)
            self.assertEqual(ids, TESTS[:2])
            self.assertEqual(suite.countTestCases(), 2)
            names = loader.loadTestsFromNames.call_args.args[0]
            self.assertIn("scripts.test_light_whisper_cli", names)
            self.assertIn("scripts.test_build_cli", names)
            self.assertIn("scripts.test_cli_context_selection", names)
            self.assertNotIn("scripts.test_cli_ci", names)
            self.assertEqual(loader.discover.call_args.kwargs["pattern"], "test_*.py")
            loader.errors = ["synthetic loader failure"]
            with self.assertRaises(RuntimeError):
                self.invoke(collector.load_suite, ROOT)
            loader.errors = []
            loader.discover.side_effect = lambda *args, **kwargs: unittest.TestSuite([first])
            with self.assertRaises(ValueError):
                self.invoke(collector.load_suite, ROOT)


class InvocationBindingTests(unittest.TestCase):
    def setUp(self):
        fixture = ContextSelectionTests("runTest")
        fixture.setUp()
        self.dataset = fixture.dataset
        self.state = {"sources": {path: SOURCE for path in SOURCES}, "bindings": fixture.bindings}
        self.state["sources"][PRIMARY] = SOURCE.replace("value + 1", "value - 1")
        self.payload = json.dumps(self.dataset).encode()
        self.expected = hashlib.sha256(self.payload).hexdigest()

    def invoke(self, payload=None, expected=None):
        try:
            return selector.select_invocation(self.payload if payload is None else payload,
                                              self.expected if expected is None else expected, self.state, TESTS)
        except NotImplementedError as error:
            self.fail(str(error))

    def test_invocation_records_actual_dataset_source_hashes_ids_and_exact_command(self):
        result = self.invoke()
        self.assertEqual(result["tests"], TESTS[:1])
        self.assertEqual(result["dataset_sha256"], self.expected)
        self.assertEqual(result["source_sha256"],
                         {path: digest(text) for path, text in self.state["sources"].items()})
        self.assertEqual(result["command"], [sys.executable, "-m", "unittest", "-f", TESTS[0]])

    def test_missing_or_wrong_trusted_dataset_hash_runs_full(self):
        for expected in ("", "wrong-hash"):
            with self.subTest(expected=expected):
                result = self.invoke(expected=expected)
                self.assertTrue(result["full"])
                self.assertEqual(result["tests"], TESTS)

    def test_even_hash_matched_invalid_json_runs_full(self):
        payload = b"not JSON"
        result = self.invoke(payload=payload, expected=hashlib.sha256(payload).hexdigest())
        self.assertTrue(result["full"])
        self.assertEqual(result["tests"], TESTS)


if __name__ == "__main__":
    unittest.main()
