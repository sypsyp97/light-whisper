"""The formal gate must distinguish checked properties from reached actions."""

import tempfile
import re
import unittest
from pathlib import Path
from unittest.mock import patch

import check_formal
import check_request_snapshot


class ToolPinTests(unittest.TestCase):
    def test_ci_and_verifier_require_the_same_tlc_binary(self):
        workflow = (check_formal.ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        pins = [re.search(r"[0-9a-f]{64}", line).group(0)
                for line in workflow.splitlines()
                if '"$RUNNER_TEMP/tla2tools.jar" | sha256sum --check' in line]
        self.assertEqual(pins, [check_formal.TLC_SHA256])


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


if __name__ == "__main__":
    unittest.main()
