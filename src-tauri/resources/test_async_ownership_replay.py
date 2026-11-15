

import hashlib
import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import download_models
import hf_cache_utils

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
from generate_async_ownership_traces import verify_trace


class CacheIntegrityReplay(unittest.TestCase):
    def test_tlc_integrity_and_explicit_repair_schedules(self):
        for name in ["model-integrity", "model-repair"]:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as directory:
                cache = Path(directory)
                repo = "test/formal-replay"
                revision = "revision"
                snapshot = cache / "models--test--formal-replay" / "snapshots" / revision
                snapshot.mkdir(parents=True)
                weight = b"formal-weight" * 100_000
                filename = "model.gguf"
                item = {"rfilename": filename, "size": len(weight), "sha256": hashlib.sha256(weight).hexdigest()}
                config = {"name": repo, "type": "formal-test", "revision": revision, "files": [item]}
                trace = verify_trace(ROOT / "formal/traces" / f"{name}.json")
                handled = []
                repair_requested = False
                with (mock.patch.object(hf_cache_utils, "get_hf_cache_root", return_value=str(cache)),
                      mock.patch.object(download_models, "get_hf_cache_root", return_value=str(cache)),
                      mock.patch.object(download_models.requests, "get", side_effect=AssertionError("repairing verified local bytes must not fetch"))):
                    for event in trace["events"]:
                        if event.startswith("cache:fixture:"):
                            kind = event.split(":")[-1]
                            if kind != "missing":
                                (snapshot / filename).write_bytes(weight)
                            if kind == "invalid":
                                (snapshot / hf_cache_utils.COMPLETE_MANIFEST_NAME).write_text("{", encoding="utf-8")
                            elif kind == "valid":
                                download_models._write_completion_manifest(str(snapshot), repo, revision, [item])
                        elif event == "cache:inspect":
                            ready = hf_cache_utils.is_hf_repo_ready(repo)
                            resolved = hf_cache_utils.find_hf_snapshot_file(repo, filename)
                            if repair_requested:
                                self.assertTrue(ready)
                                self.assertEqual(resolved, str(snapshot / filename))
                            else:
                                self.assertFalse(ready, "present invalid manifest must offer explicit repair")
                                self.assertIsNone(resolved)
                        elif event == "cache:repair:begin":
                            repair_requested = True
                        elif event == "cache:repair:complete":
                            self.assertTrue(repair_requested)
                            result = download_models.download_model(config)
                            self.assertTrue(result["success"])
                            manifest = json.loads((snapshot / hf_cache_utils.COMPLETE_MANIFEST_NAME).read_text(encoding="utf-8"))
                            self.assertEqual(manifest["files"][0]["sha256"], item["sha256"])
                        else:
                            self.fail(f"unmapped TLC cache event: {event}")
                        handled.append(event)
                self.assertEqual(handled, trace["events"])


if __name__ == "__main__":
    unittest.main()
