

import hashlib
import inspect
import io
import json
import sys
import tempfile
import re
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from unittest.mock import patch

import check_formal
import check_request_snapshot
import generate_async_ownership_traces


class ToolPinTests(unittest.TestCase):
    def test_ci_and_verifier_require_the_same_tlc_binary(self):
        workflow = (check_formal.ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        pins = [re.search(r"[0-9a-f]{64}", line).group(0)
                for line in workflow.splitlines()
                if '"$RUNNER_TEMP/tla2tools.jar" | sha256sum --check' in line]
        self.assertEqual(pins, [check_formal.TLC_SHA256])


class TlcResourceTests(unittest.TestCase):
    def test_tlc_uses_bounded_heap_and_one_worker(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "Fixture.tla"
            config = root / "Fixture.cfg"
            source.write_text("Fixture == TRUE\n", encoding="utf-8")
            config.write_text("INVARIANT Fixture\n", encoding="utf-8")
            completed = check_formal.subprocess.CompletedProcess(
                [], 0, stdout="Model checking completed. No error has been found.\n"
                "2 states generated, 1 distinct states found\n", stderr="")
            with patch.object(check_formal.subprocess, "run", return_value=completed) as run:
                check_formal.tlc(root / "tla2tools.jar", "Fixture", config, source, root / "logs")
            command = run.call_args.args[0]
            heap = [argument for argument in command[1:command.index("-jar")]
                    if argument.startswith("-Xmx")]
            workers = command[command.index("-workers") + 1]
            self.assertEqual((heap, workers), (["-Xmx512m"], "1"))


class TlcNegativeTests(unittest.TestCase):
    temporal_output = ("Error: Temporal properties were violated.\n"
                       "4 states generated, 2 distinct states found\n")
    positive_output = ("Model checking completed. No error has been found.\n"
                       "4 states generated, 2 distinct states found\n")

    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.source = self.root / "Fixture.tla"
        self.config = self.root / "Fixture.cfg"
        self.logs = self.root / "logs"
        self.jar = self.root / "tla2tools.jar"
        self.source.write_text(
            "---- MODULE Fixture ----\nVARIABLE state\nInit == state = 0\n"
            "Next == UNCHANGED state\nSpec == Init /\\ [][Next]_state\n"
            "Safety == state = 0\nSettles == <> (state = 1)\n"
            "Other == <> (state = 2)\n====\n", encoding="utf-8")
        self.config.write_text("SPECIFICATION Spec\nPROPERTY Settles\n", encoding="utf-8")

    def run_tlc(self, returncode, stdout, **expected):
        completed = check_formal.subprocess.CompletedProcess(
            [], returncode, stdout=stdout, stderr="")
        with patch.object(check_formal.subprocess, "run", return_value=completed):
            return check_formal.tlc(self.jar, "Fixture", self.config, self.source,
                                    self.logs, **expected)

    def accept_temporal_negative(self):
        try:
            return self.run_tlc(13, self.temporal_output, expected_property="Settles")
        except ValueError as error:
            self.fail(f"valid named temporal negative was rejected: {error}")

    def test_positive_and_safety_result_schema_stays_compatible(self):
        for expected, returncode, output in [
            (None, 0, self.positive_output),
            ("Safety", 12, "Error: Invariant Safety is violated.\n"
             "4 states generated, 2 distinct states found\n"),
        ]:
            with self.subTest(expected_violation=expected):
                completed = check_formal.subprocess.CompletedProcess(
                    [], returncode, stdout=output, stderr="")
                with patch.object(check_formal.subprocess, "run", return_value=completed):
                    record = check_formal.tlc(self.jar, "Fixture", self.config, self.source,
                                              self.logs, expected)
                self.assertEqual(record, {
                    "model": "Fixture", "config": "Fixture.cfg", "distinct": 2,
                    "source_sha256": hashlib.sha256(self.source.read_bytes()).hexdigest(),
                    "config_sha256": hashlib.sha256(self.config.read_bytes()).hexdigest(),
                    "expected_violation": expected,
                })
                self.assertEqual(json.loads((self.logs / "result.json").read_text()), record)

    def test_safety_negative_requires_its_named_invariant_and_exit_12(self):
        for index, (returncode, output) in enumerate([
            (12, "Error: Invariant Other is violated.\n"),
            (13, self.temporal_output),
            (0, "Error: Invariant Safety is violated.\n"),
            (150, "Error: Parsing or semantic analysis failed.\n"),
            (11, "Error: Deadlock reached.\n"),
        ]):
            with self.subTest(returncode=returncode, output=output):
                self.logs = self.root / f"logs-{index}"
                with self.assertRaises(ValueError):
                    self.run_tlc(returncode, output, expected_violation="Safety")
                self.assertFalse((self.logs / "result.json").exists())

    def test_temporal_negative_accepts_exit_13_and_records_the_selected_property(self):
        record = self.accept_temporal_negative()
        self.assertEqual(record["expected_property"], "Settles")
        self.assertIsNone(record["expected_violation"])
        self.assertEqual(record["distinct"], 2)
        self.assertEqual(record["source_sha256"], hashlib.sha256(self.source.read_bytes()).hexdigest())
        self.assertEqual(record["config_sha256"], hashlib.sha256(self.config.read_bytes()).hexdigest())
        self.assertEqual(json.loads((self.logs / "result.json").read_text()), record)

    def test_temporal_negative_requires_both_exit_13_and_the_temporal_diagnostic(self):
        for index, (returncode, output) in enumerate([
            (0, self.positive_output),
            (0, self.temporal_output),
            (12, self.temporal_output),
            (12, "Error: Invariant Safety is violated.\n"),
            (11, "Error: Deadlock reached.\n"),
            (150, "Error: Parsing or semantic analysis failed.\n"),
            (13, "Error: Parsing or semantic analysis failed.\n"),
            (13, "Error: Deadlock reached.\n"),
            (13, "Error: Invariant Safety is violated.\n"),
        ]):
            with self.subTest(returncode=returncode, output=output):
                self.logs = self.root / f"logs-{index}"
                with self.assertRaises(ValueError):
                    self.run_tlc(returncode, output, expected_property="Settles")
                self.assertFalse((self.logs / "result.json").exists())

    def test_temporal_negative_rejects_timeout(self):
        with patch.object(check_formal.subprocess, "run",
                          side_effect=check_formal.subprocess.TimeoutExpired("java", 480)):
            with self.assertRaises(check_formal.subprocess.TimeoutExpired):
                check_formal.tlc(self.jar, "Fixture", self.config, self.source, self.logs,
                                 expected_property="Settles")
        self.assertFalse((self.logs / "result.json").exists())

    def test_temporal_negative_requires_the_only_active_temporal_property(self):
        for index, properties in enumerate([
            "",
            "\\* PROPERTY Settles\n",
            "(*\nPROPERTY Settles\n*)\n",
            "PROPERTY Other\n",
            "PROPERTY Settles\nPROPERTY Other\n",
            "PROPERTY Settles\n Other\n",
            "PROPERTY Settles Other\n",
            "PROPERTIES Settles Other\n",
            "PROPERTIES Settles\n",
            "PROPERTIES Other\nPROPERTY Settles\n",
            "PROPERTY Settles\nPROPERTIES Other\n",
            "PROPERTY Settles\nINVARIANT Safety\n",
            "PROPERTY Settles\nCHECK_DEADLOCK TRUE\n",
            "INVARIANT Safety PROPERTY Other\nPROPERTY Settles\n",
            "INVARIANT Safety PROPERTIES Other\nPROPERTY Settles\n",
        ]):
            with self.subTest(properties=properties):
                self.logs = self.root / f"logs-{index}"
                self.config.write_text("SPECIFICATION Spec\n" + properties, encoding="utf-8")
                with self.assertRaises(ValueError):
                    self.run_tlc(13, self.temporal_output, expected_property="Settles")
                self.assertFalse((self.logs / "result.json").exists())

    def test_temporal_negative_allows_invariants_and_ignores_commented_properties(self):
        self.config.write_text(
            "SPECIFICATION Spec\nINVARIANT Safety\n"
            "(* PROPERTY Other *)\nPROPERTY Settles \\* PROPERTY Other\n",
            encoding="utf-8")
        record = self.accept_temporal_negative()
        self.assertEqual(record["expected_property"], "Settles")

    def test_negative_cannot_expect_both_an_invariant_and_a_temporal_property(self):
        with self.assertRaises(ValueError):
            self.run_tlc(12, "Error: Invariant Safety is violated.\n",
                         expected_violation="Safety", expected_property="Settles")
        self.assertFalse((self.logs / "result.json").exists())

    def test_temporal_negative_rejects_unterminated_tla_before_launch(self):
        for index, (suffix, category) in enumerate([
            ('"unfinished', "string"),
            ('"unfinished\\', "string"),
            ('(* outer (* inner *)', "comment"),
        ]):
            with self.subTest(suffix=suffix):
                self.logs = self.root / f"malformed-{index}"
                self.config.write_text("PROPERTY Settles\n" + suffix, encoding="utf-8")
                completed = check_formal.subprocess.CompletedProcess(
                    [], 13, stdout=self.temporal_output, stderr="")
                with patch.object(check_formal.subprocess, "run", return_value=completed) as run:
                    with self.assertRaisesRegex(ValueError, f"unterminated TLA {category}"):
                        check_formal.tlc(self.jar, "Fixture", self.config, self.source,
                                         self.logs, expected_property="Settles")
                run.assert_not_called()
                self.assertFalse(self.logs.exists())

    def test_positive_requires_both_exit_zero_and_completion_diagnostic(self):
        for index, (returncode, output) in enumerate([
            (0, "Error: Parsing or semantic analysis failed.\n"),
            (13, self.positive_output),
            (11, "Error: Deadlock reached.\n"),
        ]):
            with self.subTest(returncode=returncode, output=output):
                self.logs = self.root / f"failed-positive-{index}"
                with self.assertRaises(ValueError):
                    self.run_tlc(returncode, output)
                self.assertEqual((self.logs / "tlc.log").read_text(), output)
                self.assertFalse((self.logs / "result.json").exists())

    def run_negative_main(self, case, returncode, output, inspect_tlc=None):
        formal = self.root / "formal"
        formal.mkdir(exist_ok=True)
        (formal / self.source.name).write_bytes(self.source.read_bytes())
        (formal / self.config.name).write_bytes(self.config.read_bytes())
        (formal / "verification.json").write_text(json.dumps({"negative": [case]}), encoding="utf-8")
        self.jar.write_bytes(b"test-only pinned jar fixture")
        completed = check_formal.subprocess.CompletedProcess(
            [], returncode, stdout=output, stderr="")
        preflight = check_formal.subprocess.CompletedProcess([], 0, stdout="", stderr="")

        def run(command, **kwargs):
            if command[0] == "java":
                if inspect_tlc is not None:
                    inspect_tlc(command, **kwargs)
                return completed
            return preflight

        with patch.object(check_formal, "ROOT", self.root), \
                patch.object(check_formal, "TLC_SHA256", hashlib.sha256(self.jar.read_bytes()).hexdigest()), \
                patch.object(check_formal, "check_inventory"), \
                patch.object(check_formal.sys, "argv", ["check_formal.py", "--negative-only",
                             "--tlc-jar", str(self.jar), "--lean", "unused-lean", "--logs", str(self.logs)]), \
                patch.object(check_formal.subprocess, "run", side_effect=run):
            return check_formal.main()

    def test_main_replacement_checks_copied_files_and_preserves_typed_receipts(self):
        original, replacement = "Next == UNCHANGED state", "Next == state' = 1"
        source_bytes = self.source.read_bytes()
        changed_bytes = source_bytes.replace(original.encode(), replacement.encode())
        self.config = self.root / "Fixture.explicit.cfg"
        for kind, expected, returncode, output in [
            ("invariant", "Safety", 12, "Error: Invariant Safety is violated.\n"
             "4 states generated, 2 distinct states found\n"),
            ("property", "Settles", 13, self.temporal_output),
        ]:
            with self.subTest(kind=kind):
                directive = "INVARIANT" if kind == "invariant" else "PROPERTY"
                config_bytes = ("SPECIFICATION Spec\r\n\\* café\r\n"
                                f"{directive} {expected}\r\n").encode("utf-8")
                self.config.write_bytes(config_bytes)
                copied = []

                def inspect_tlc(command, **kwargs):
                    source = Path(command[-1])
                    config = Path(command[command.index("-config") + 1])
                    copied.append((source, config))
                    self.assertEqual(source.read_bytes(), changed_bytes)
                    self.assertEqual(config.read_bytes(), config_bytes)
                    self.assertEqual(source.parent, config.parent)
                    self.assertEqual(kwargs["cwd"], source.parent)
                    self.assertNotEqual(source.parent, self.root / "formal")

                case = {"name": f"replacement-{kind}", "model": "Fixture",
                        "config": self.config.name, "replace": [original, replacement], kind: expected}
                try:
                    outcome = self.run_negative_main(case, returncode, output, inspect_tlc)
                except ValueError as error:
                    self.fail(f"valid {kind} replacement negative was rejected: {error}")
                self.assertEqual(outcome, 0)
                self.assertEqual(len(copied), 1)
                self.assertFalse(copied[0][0].parent.exists())
                self.assertEqual((self.root / "formal" / self.source.name).read_bytes(), source_bytes)
                self.assertEqual((self.root / "formal" / self.config.name).read_bytes(), config_bytes)
                summary = json.loads((self.logs / "negative-summary.json").read_text())
                record = json.loads((self.logs / f"negative-{case['name']}" / "result.json").read_text())
                self.assertEqual(summary, [record])
                self.assertEqual(record["config"], self.config.name)
                self.assertEqual(record["source_sha256"], hashlib.sha256(changed_bytes).hexdigest())
                self.assertEqual(record["config_sha256"], hashlib.sha256(config_bytes).hexdigest())
                self.assertEqual(record["distinct"], 2)
                self.assertEqual(record["expected_violation"], expected if kind == "invariant" else None)
                if kind == "property":
                    self.assertEqual(record["expected_property"], expected)
                else:
                    self.assertNotIn("expected_property", record)

    def test_main_replacement_refuses_zero_or_multiple_targets_before_launch(self):
        original = "Next == UNCHANGED state"
        source = self.source.read_text(encoding="utf-8")
        temporary_directory = check_formal.tempfile.TemporaryDirectory
        for content in [source.replace(original, "Next == state' = 0"), source + original + "\n"]:
            with self.subTest(occurrences=content.count(original)):
                self.logs = self.root / f"refusal-{content.count(original)}"
                self.source.write_text(content, encoding="utf-8")
                temporary_paths = []

                def temporary(*args, **kwargs):
                    directory = temporary_directory(*args, **kwargs)
                    temporary_paths.append(Path(directory.name))
                    return directory

                def reject_launch(*args, **kwargs):
                    self.fail("ambiguous replacement target reached TLC")

                case = {"name": "ambiguous-replacement", "model": "Fixture", "invariant": "Safety",
                        "replace": [original, "Next == state' = 1"]}
                with patch.object(check_formal.tempfile, "TemporaryDirectory", side_effect=temporary):
                    with self.assertRaisesRegex(ValueError, "mutation target changed"):
                        self.run_negative_main(case, 12, "Error: Invariant Safety is violated.\n", reject_launch)
                self.assertEqual(len(temporary_paths), 1)
                self.assertFalse(temporary_paths[0].exists())
                self.assertFalse((self.logs / "negative-summary.json").exists())
                self.assertFalse((self.logs / "negative-ambiguous-replacement").exists())

    def test_main_replacement_cleans_copied_files_when_tlc_rejects_outcome(self):
        copied = []

        def inspect_tlc(command, **kwargs):
            copied.append(Path(command[-1]).parent)
            self.assertTrue(Path(command[-1]).is_file())
            self.assertTrue(Path(command[command.index("-config") + 1]).is_file())

        case = {"name": "failed-replacement", "model": "Fixture", "invariant": "Safety",
                "replace": ["Next == UNCHANGED state", "Next == state' = 1"]}
        with self.assertRaises(ValueError):
            self.run_negative_main(case, 0, self.positive_output, inspect_tlc)
        self.assertEqual(len(copied), 1)
        self.assertFalse(copied[0].exists())
        self.assertTrue((self.logs / "negative-failed-replacement" / "tlc.log").exists())
        self.assertFalse((self.logs / "negative-failed-replacement" / "result.json").exists())
        self.assertFalse((self.logs / "negative-summary.json").exists())

    def test_main_dispatches_temporal_negative_as_temporal_instead_of_positive(self):
        case = {"name": "fixture-progress", "model": "Fixture",
                "invariant": None, "property": "Settles"}
        try:
            outcome = self.run_negative_main(case, 13, self.temporal_output)
        except ValueError as error:
            self.fail(f"main rejected a valid typed temporal negative: {error}")
        self.assertEqual(outcome, 0)
        summary = json.loads((self.logs / "negative-summary.json").read_text())
        self.assertEqual(summary[0]["expected_property"], "Settles")
        self.assertIsNone(summary[0]["expected_violation"])
        self.assertTrue((self.logs / "negative-fixture-progress" / "tlc.log").exists())

    def test_main_temporal_row_does_not_require_an_invariant_field(self):
        case = {"name": "fixture-progress", "model": "Fixture", "property": "Settles"}
        try:
            outcome = self.run_negative_main(case, 13, self.temporal_output)
        except KeyError as error:
            self.fail(f"temporal row requires an obsolete invariant field: {error}")
        self.assertEqual(outcome, 0)

    def test_main_preserves_safety_negative_rows_and_summary(self):
        case = {"name": "fixture-safety", "model": "Fixture", "invariant": "Safety"}
        outcome = self.run_negative_main(case, 12, "Error: Invariant Safety is violated.\n")
        self.assertEqual(outcome, 0)
        summary = json.loads((self.logs / "negative-summary.json").read_text())
        self.assertEqual(summary[0]["expected_violation"], "Safety")
        self.assertNotIn("expected_property", summary[0])
        self.assertTrue((self.logs / "negative-fixture-safety" / "tlc.log").exists())

    def test_main_rejects_ambiguous_negative_row(self):
        case = {"name": "fixture-ambiguous", "model": "Fixture",
                "invariant": "Safety", "property": "Settles"}
        with self.assertRaises(ValueError):
            self.run_negative_main(case, 12, "Error: Invariant Safety is violated.\n")


class CloudAsrSnapshotTests(unittest.TestCase):
    def test_cloud_snapshot_guard_rejects_broken_request_paths(self):
        for provider, original, replacement in [
            ("glm", "let _asr_guard = state.engine.native_asr_owner.lock().await;", ""),
            ("glm", "let _configuration = state.engine.funasr_lifecycle_op.lock().await;", ""),
            ("glm", "state.read_online_asr_api_key(),", "{ drop(_configuration); state.read_online_asr_api_key() },"),
            ("glm", "audio_data, &url, &api_key", "audio_data, &api_key, &url"),
            ("glm", ".post(url)", ".post(paths::read_online_asr_endpoint())"),
            ("glm", "if api_key.is_empty() {", "let api_key = state.read_online_asr_api_key();\n    if api_key.is_empty() {"),
            ("alibaba", "transcribe_via_omni_chat(state, &base, &model, &api_key,",
             "transcribe_via_omni_chat(state, &base, &model, &state.read_online_asr_api_key(),"),
            ("alibaba", "transcribe_via_dashscope_asr(state, &base, &model, &api_key,",
             "transcribe_via_dashscope_asr(state, &base, &model, &state.read_online_asr_api_key(),"),
        ]:
            with self.subTest(provider=provider, mutation=original):
                target = check_request_snapshot.ROOT / f"src-tauri/src/services/{provider}_asr_service.rs"
                source = target.read_text(encoding="utf-8")
                engine = f"{provider}-asr"
                check_request_snapshot.check_cloud_asr_snapshot(source, engine)
                self.assertEqual(source.count(original), 1)
                with self.assertRaises(ValueError):
                    check_request_snapshot.check_cloud_asr_snapshot(source.replace(original, replacement), engine)


class ModelBindingTests(unittest.TestCase):
    def check(self, body: str, configuration: str, bound: str = "NoLeaks",
              observed: str = "", actions: list[str] | None = None) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "formal").mkdir()
            (root / "logs" / "Fixture").mkdir(parents=True)
            (root / "formal" / "Fixture.tla").write_text(body, encoding="utf-8")
            (root / "formal" / "Fixture.cfg").write_text(configuration, encoding="utf-8")
            (root / "logs" / "Fixture" / "tlc.log").write_text(observed, encoding="utf-8")
            manifest = {"runs": {"Fixture": ["Fixture.cfg"]},
                        "contracts": {"fixture": {"models": {"Fixture": [bound]}}},
                        "actions": {"Fixture": actions or []}}
            with patch.object(check_formal, "ROOT", root):
                check_formal.check_model_bindings(manifest, "Fixture", root / "logs")

    def test_reached_predicate_is_not_a_checked_invariant(self):
        with self.assertRaises(ValueError):
            self.check("NoLeaks == TRUE\n", "CHECK_DEADLOCK FALSE\n",
                       observed="<NoLeaks line 1, col 1 to line 1, col 15 of module Fixture>: 1:1\n")

    def test_negation_does_not_prove_its_operand(self):
        with self.assertRaises(ValueError):
            self.check("NoLeaks == TRUE\nSafety == ~NoLeaks\n", "INVARIANT Safety\n")

    def test_comment_does_not_prove_a_property(self):
        with self.assertRaises(ValueError):
            self.check("NoLeaks == TRUE\nOther == TRUE\nSafety == Other \\* NoLeaks\n",
                       "INVARIANT Safety\n")

    def test_string_literal_does_not_prove_a_property(self):
        with self.assertRaises(ValueError):
            self.check('NoLeaks == TRUE\nSafety == "NoLeaks" = "NoLeaks"\n',
                       "INVARIANT Safety\n")

    def test_block_comment_does_not_prove_a_property(self):
        with self.assertRaises(ValueError):
            self.check("NoLeaks == TRUE\nOther == TRUE\nSafety == Other (* NoLeaks *)\n",
                       "INVARIANT Safety\n")

    def test_config_comment_does_not_check_a_property(self):
        with self.assertRaises(ValueError):
            self.check("NoLeaks == TRUE\n", "(*\nINVARIANT NoLeaks\n*)\n")

    def test_positive_conjunction_proves_each_named_conjunct(self):
        self.check("NoLeaks == TRUE\nOther == TRUE\nSafety == /\\ NoLeaks /\\ Other\n",
                   "INVARIANT Safety\n")

    def test_direct_temporal_property_is_checked(self):
        self.check("Settles == [] TRUE\n", "PROPERTY Settles\n", bound="Settles")

    def test_declared_action_must_really_be_an_action(self):
        with self.assertRaises(ValueError):
            self.check("Begin == TRUE\n", "", bound="Begin", actions=["Begin"],
                       observed="<Begin line 1, col 1 to line 1, col 13 of module Fixture>: 1:1\n")

    def test_reachable_state_transition_is_accepted(self):
        self.check("Begin == h' = h + 1\n", "", bound="Begin", actions=["Begin"],
                   observed="<Begin line 1, col 1 to line 1, col 20 of module Fixture>: 1:1\n")


class TlcGapFixture(unittest.TestCase):
    temporal_output = ("Error: Temporal properties were violated.\n"
                       "4 states generated, 2 distinct states found\n")
    positive_output = ("Model checking completed. No error has been found.\n"
                       "4 states generated, 2 distinct states found\n")

    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        (self.root / "formal").mkdir()
        self.source = self.root / "formal" / "Fixture.tla"
        self.config = self.root / "formal" / "Fixture.cfg"
        self.logs = self.root / "logs"
        self.jar = self.root / "tla2tools.jar"
        self.source.write_text(
            "---- MODULE Fixture ----\nCONSTANTS Label, Flag\nVARIABLE state\n"
            "Init == state = 0\nNext == UNCHANGED state\n"
            "Spec == Init /\\ [][Next]_state\nSafety == state = 0\n"
            "Settles == <> (state = 1)\nOther == <> (state = 2)\n"
            "Zzz == <> (state = 3)\n====\n", encoding="utf-8")
        self.config.write_text("SPECIFICATION Spec\nPROPERTY Settles\n", encoding="utf-8")

    def accept_temporal_config(self, configuration):
        self.config.write_text(configuration, encoding="utf-8")
        completed = check_formal.subprocess.CompletedProcess(
            [], 13, stdout=self.temporal_output, stderr="")
        with patch.object(check_formal.subprocess, "run", return_value=completed) as run:
            try:
                record = check_formal.tlc(self.jar, "Fixture", self.config, self.source,
                                          self.logs, expected_property="Settles")
            except Exception as error:
                self.fail(f"valid temporal configuration failed: {type(error).__name__}: {error}")
        run.assert_called_once()
        self.assertEqual(record["expected_property"], "Settles")
        self.assertIsNone(record["expected_violation"])
        self.assertEqual(record["distinct"], 2)
        self.assertEqual(record["source_sha256"], hashlib.sha256(self.source.read_bytes()).hexdigest())
        self.assertEqual(record["config_sha256"], hashlib.sha256(self.config.read_bytes()).hexdigest())
        self.assertEqual(json.loads((self.logs / "result.json").read_text()), record)
        self.assertEqual((self.logs / "tlc.log").read_text(), self.temporal_output)

    def accept_bound_config(self, configuration):
        (self.logs / self.config.stem).mkdir(parents=True, exist_ok=True)
        (self.logs / self.config.stem / "tlc.log").write_text("", encoding="utf-8")
        self.config.write_text(configuration, encoding="utf-8")
        manifest = {"runs": {"Fixture": [self.config.name]},
                    "contracts": {"fixture": {"models": {"Fixture": ["Settles"]}}}}
        with patch.object(check_formal, "ROOT", self.root):
            try:
                check_formal.check_model_bindings(manifest, "Fixture", self.logs)
            except Exception as error:
                self.fail(f"live configured property was lost: {type(error).__name__}: {error}")


class TlcCommentBoundaryTests(TlcGapFixture):
    def test_valid_comments_keep_the_live_temporal_property_and_binding(self):
        for index, configuration in enumerate([
            "\\* leading comment\nPROPERTY Settles\n",
            "SPECIFICATION Spec\n(**)PROPERTY Settles\n",
            "SPECIFICATION Spec\n(* ignored *)PROPERTY Settles\n",
            "SPECIFICATION Spec\nCONSTANT Flag = TRUE(*\n*)PROPERTY Settles\n",
            "SPECIFICATION Spec\nPROPERTY(*\t*) Settles\n",
            "SPECIFICATION Spec\nPROPERTY(*x*) Settles\n",
        ]):
            with self.subTest(configuration=configuration):
                self.logs = self.root / f"comments-{index}"
                self.accept_temporal_config(configuration)
        self.logs = self.root / "comment-bindings"
        self.accept_bound_config("(* ignored *)\nPROPERTY Settles\n")

    def test_eof_line_comment_finishes_and_keeps_the_live_temporal_property(self):
        # With immutable input, the scanner branches only on index/depth.
        # Its output list never feeds control flow: repeating that state cannot terminate.
        scanner = check_formal.tla_code.__code__
        lines, first_line = inspect.getsourcelines(check_formal.tla_code)
        loop_line = first_line + next(index for index, line in enumerate(lines)
                                      if line.startswith("    while "))
        seen = set()

        def observe(frame, event, argument):
            if frame.f_code is scanner and event == "line" and frame.f_lineno == loop_line:
                state = (frame.f_locals["index"], frame.f_locals["depth"])
                self.assertNotIn(state, seen, "EOF comment scanner repeated a control state; cannot finish")
                seen.add(state)
            return observe

        previous = sys.gettrace()
        try:
            sys.settrace(observe)
            self.accept_temporal_config("SPECIFICATION Spec\nPROPERTY Settles \\* EOF comment")
        finally:
            sys.settrace(previous)

    def test_overlapping_comment_delimiters_reject_before_launch(self):
        self.config.write_text("PROPERTY Settles\n(*)", encoding="utf-8")
        completed = check_formal.subprocess.CompletedProcess([], 13, stdout=self.temporal_output, stderr="")
        with patch.object(check_formal.subprocess, "run", return_value=completed) as run:
            with self.assertRaisesRegex(ValueError, "unterminated TLA comment"):
                check_formal.tlc(self.jar, "Fixture", self.config, self.source,
                                 self.logs, expected_property="Settles")
        run.assert_not_called()
        self.assertFalse(self.logs.exists())


class TlcStringBoundaryTests(TlcGapFixture):
    def test_valid_string_values_preserve_the_following_temporal_property_and_binding(self):
        for index, literal in enumerate(['"x"', '"A"', '" "', '""', r'"\""']):
            with self.subTest(literal=literal):
                self.logs = self.root / f"string-{index}"
                configuration = f"SPECIFICATION Spec\nCONSTANT Label = {literal}\nPROPERTY Settles\n"
                self.accept_temporal_config(configuration)
                self.accept_bound_config(configuration)

    def test_long_file_input_finishes_after_its_live_property(self):
        configuration = "\\*" + "x" * 300 + "\nPROPERTY Settles\n"
        self.accept_temporal_config(configuration)
        self.accept_bound_config(configuration)


class TlcTypedOutcomeTests(TlcGapFixture):
    def test_different_temporal_names_reject_in_both_lexical_directions_before_launch(self):
        for name in ["Other", "Zzz"]:
            with self.subTest(name=name):
                self.logs = self.root / name
                self.config.write_text(f"SPECIFICATION Spec\nPROPERTY {name}\n", encoding="utf-8")
                completed = check_formal.subprocess.CompletedProcess([], 13, stdout=self.temporal_output, stderr="")
                with patch.object(check_formal.subprocess, "run", return_value=completed) as run:
                    with self.assertRaises(ValueError):
                        check_formal.tlc(self.jar, "Fixture", self.config, self.source,
                                         self.logs, expected_property="Settles")
                run.assert_not_called()
                self.assertFalse(self.logs.exists())

    def test_matching_negative_diagnostic_cannot_accept_a_higher_exit_status(self):
        for kind, name, returncode, output in [
            ("property", "Settles", 14, self.temporal_output),
            ("invariant", "Safety", 13, "Error: Invariant Safety is violated.\n"),
        ]:
            with self.subTest(kind=kind, returncode=returncode):
                self.logs = self.root / kind
                directive = "PROPERTY" if kind == "property" else "INVARIANT"
                self.config.write_text(f"SPECIFICATION Spec\n{directive} {name}\n", encoding="utf-8")
                completed = check_formal.subprocess.CompletedProcess(
                    [], returncode, stdout=output, stderr="raw stderr retained\n")
                expected = {"expected_property" if kind == "property" else "expected_violation": name}
                with patch.object(check_formal.subprocess, "run", return_value=completed) as run:
                    with self.assertRaises(ValueError):
                        check_formal.tlc(self.jar, "Fixture", self.config, self.source, self.logs, **expected)
                run.assert_called_once()
                self.assertEqual((self.logs / "tlc.log").read_text(), output + completed.stderr)
                self.assertFalse((self.logs / "result.json").exists())


class TlcEvidenceSurfaceTests(TlcGapFixture):
    def test_actual_child_streams_are_captured_in_the_typed_evidence(self):
        run_child = check_formal.subprocess.run
        stderr = "separate stderr marker\n"

        def run(command, **kwargs):
            return run_child([sys.executable, "-B", "-c",
                              "import sys; sys.stdout.write(sys.argv[1]); sys.stderr.write(sys.argv[2])",
                              self.positive_output, stderr], **kwargs)

        with patch.object(check_formal.subprocess, "run", side_effect=run) as launch:
            try:
                record = check_formal.tlc(self.jar, "Fixture", self.config, self.source, self.logs)
            except Exception as error:
                self.fail(f"actual child evidence was not captured: {type(error).__name__}: {error}")
        launch.assert_called_once()
        self.assertEqual((self.logs / "tlc.log").read_text(), self.positive_output + stderr)
        self.assertEqual(json.loads((self.logs / "result.json").read_text()), record)
        self.assertEqual(record["distinct"], 2)
        self.assertIsNone(record["expected_violation"])
        self.assertNotIn("expected_property", record)

    def test_trace_generator_creates_fresh_nested_tlc_evidence_parents(self):
        self.config.write_text("SPECIFICATION Spec\nINVARIANT Safety\n", encoding="utf-8")
        negative = self.config.with_name("FixtureNegative.cfg")
        negative.write_bytes(self.config.read_bytes())
        self.jar.write_bytes(b"test-only pinned jar fixture")
        self.logs = self.root / "fresh" / "nested" / "logs"
        counterexample = ("Error: Invariant Safety is violated.\n"
                          "State 1: <Initial predicate>\n/\\ lastEvent = \"init\"\n"
                          "State 2: <Next>\n/\\ lastEvent = \"finish\"\n"
                          "4 states generated, 2 distinct states found\n")
        run_child = check_formal.subprocess.run

        def run(command, **kwargs):
            is_negative = Path(command[command.index("-config") + 1]) == negative
            return run_child([sys.executable, "-B", "-c",
                              "import sys; sys.stdout.write(sys.argv[1]); sys.exit(int(sys.argv[2]))",
                              counterexample if is_negative else self.positive_output,
                              "12" if is_negative else "0"], **kwargs)

        with patch.object(generate_async_ownership_traces, "ROOT", self.root), \
                patch.object(generate_async_ownership_traces, "CASES", {
                    "fixture": ("Fixture", negative.name, "Safety")}), \
                patch.object(generate_async_ownership_traces, "TLC_SHA256",
                             hashlib.sha256(self.jar.read_bytes()).hexdigest()), \
                patch.object(sys, "argv", ["generate_async_ownership_traces.py",
                             "--tlc-jar", str(self.jar), "--logs", str(self.logs)]), \
                patch.object(check_formal.subprocess, "run", side_effect=run) as launch:
            try:
                generate_async_ownership_traces.main()
            except Exception as error:
                self.fail(f"trace generator failed with fresh nested logs: {type(error).__name__}: {error}")
        self.assertEqual(launch.call_count, 2)
        for directory, output, expected in [
            (self.logs / "Fixture", self.positive_output, None),
            (self.logs / "negative-fixture", counterexample, "Safety"),
        ]:
            self.assertEqual((directory / "tlc.log").read_text(), output)
            record = json.loads((directory / "result.json").read_text())
            self.assertEqual(record["expected_violation"], expected)
            self.assertEqual(record["distinct"], 2)
        trace = json.loads((self.root / "formal/traces/fixture.json").read_text())
        self.assertEqual(trace["events"], ["finish"])
        self.assertEqual(trace["violated_invariant"], "Safety")

    def test_console_status_distinguishes_success_from_each_typed_negative(self):
        for index, (expected, returncode, output, label) in enumerate([
            ({}, 0, self.positive_output, "PASS"),
            ({"expected_violation": "Safety"}, 12, "Error: Invariant Safety is violated.\n", "NEGATIVE"),
            ({"expected_property": "Settles"}, 13, self.temporal_output, "NEGATIVE"),
        ]):
            with self.subTest(expected=expected):
                self.logs = self.root / f"status-{index}"
                directive = "INVARIANT Safety" if "expected_violation" in expected else "PROPERTY Settles"
                self.config.write_text(f"SPECIFICATION Spec\n{directive}\n", encoding="utf-8")
                completed = check_formal.subprocess.CompletedProcess([], returncode, stdout=output, stderr="")
                console = io.StringIO()
                with patch.object(check_formal.subprocess, "run", return_value=completed), redirect_stdout(console):
                    record = check_formal.tlc(self.jar, "Fixture", self.config, self.source, self.logs, **expected)
                self.assertEqual(console.getvalue().split()[0], label)
                self.assertIn(f"Fixture/{self.config.name}", console.getvalue())
                self.assertEqual(json.loads((self.logs / "result.json").read_text()), record)
                for key, value in expected.items():
                    self.assertEqual(record[key], value)


if __name__ == "__main__":
    unittest.main()
