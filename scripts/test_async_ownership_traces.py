

import json
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import generate_async_ownership_traces as traces


class TraceProvenance(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.path = self.root / "formal/traces/history-leases.json"
        original = traces.ROOT / "formal/traces/history-leases.json"
        self.document = json.loads(original.read_text(encoding="utf-8"))
        for name in ["model", "config", "counterexample"]:
            target = self.root / self.document[name]
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(traces.ROOT / self.document[name], target)
        self.save()
        self.patch = patch.object(traces, "ROOT", self.root)
        self.patch.start()
        self.addCleanup(self.patch.stop)

    def save(self):
        self.path.write_text(json.dumps(self.document), encoding="utf-8")

    def test_generated_trace_is_accepted(self):
        self.assertEqual(traces.verify_trace(self.path)["events"], self.document["events"])

    def test_source_drift_is_rejected(self):
        source = self.root / self.document["model"]
        source.write_text(source.read_text(encoding="utf-8") + "\n\\* drift\n", encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "stale model"):
            traces.verify_trace(self.path)

    def test_config_drift_is_rejected(self):
        (self.root / self.document["config"]).write_text("SPECIFICATION Different\n", encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "stale config"):
            traces.verify_trace(self.path)

    def test_hand_edited_or_unknown_event_is_rejected(self):
        self.document["events"][0] = "unknown:fake-event"
        self.save()
        with self.assertRaisesRegex(ValueError, "events differ"):
            traces.verify_trace(self.path)

    def test_modified_counterexample_is_rejected(self):
        (self.root / self.document["counterexample"]).write_text("not a TLC log", encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "corrupt counterexample"):
            traces.verify_trace(self.path)

    def test_valid_hash_cannot_substitute_a_different_contract(self):
        self.document["violated_invariant"] = "UnrelatedInvariant"
        self.save()
        with self.assertRaisesRegex(ValueError, "contract identity changed"):
            traces.verify_trace(self.path)



class TraceCheckoutPortability(unittest.TestCase):
    @staticmethod
    def hashed_inputs():
        paths = set()
        for name, (model, _config, _invariant) in traces.CASES.items():
            document = json.loads((traces.ROOT / "formal/traces" / f"{name}.json").read_text(encoding="utf-8"))
            paths.update(document[field] for field in ["model", "config", "counterexample"])
            paths.add(f"formal/{model}.cfg")
        return sorted(paths)

    def test_hashed_model_config_and_counterexample_bytes_are_lf(self):
        for name in self.hashed_inputs():
            with self.subTest(path=name):
                content = (traces.ROOT / name).read_bytes()
                self.assertFalse(b"\r" in content, "CRLF changes the recorded SHA256 across platform checkout")
                self.assertTrue(content.endswith(b"\n"), "hashed text needs a stable final LF")

    def test_git_attributes_pin_every_hashed_input_to_lf(self):
        names = self.hashed_inputs()
        result = subprocess.run(["git", "check-attr", "-z", "--stdin", "text", "eol"], cwd=traces.ROOT,
                                input=b"\0".join(name.encode("utf-8") for name in names) + b"\0",
                                capture_output=True, check=True)
        attributes = {}
        fields = result.stdout.decode("utf-8").split("\0")[:-1]
        for index in range(0, len(fields), 3):
            name, kind, value = fields[index:index + 3]
            attributes[(name, kind)] = value
        for name in names:
            with self.subTest(path=name):
                self.assertEqual(attributes[(name, "text")], "set")
                self.assertEqual(attributes[(name, "eol")], "lf")


if __name__ == "__main__":
    unittest.main()
