import os
import hashlib
import json
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, os.path.dirname(__file__))

import download_models
import hf_cache_utils


class FakeStreamingResponse:
    status_code = 200
    headers = {"Content-Length": "6"}

    def raise_for_status(self):
        return None

    def iter_content(self, chunk_size):
        yield b"abc"
        raise RuntimeError("connection dropped")


class FakeCompleteResponse:
    status_code = 200
    headers = {"Content-Length": "3"}

    def raise_for_status(self):
        return None

    def iter_content(self, chunk_size):
        yield b"xyz"


class FakeRepoInfoResponse:
    def __init__(self, payload):
        self.payload = payload

    def raise_for_status(self):
        return None

    def json(self):
        return self.payload


class FakeRangeNotSatisfiableResponse:
    status_code = 416
    headers = {}

    def raise_for_status(self):
        return None

    def iter_content(self, chunk_size):
        return iter(())


class ModelDownloadAtomicityTests(unittest.TestCase):
    def test_get_repo_info_preserves_lfs_sha256_metadata(self):
        sha256 = "a" * 64
        payload = {
            "sha": "commit123",
            "siblings": [
                {
                    "rfilename": "model.safetensors",
                    "size": 3,
                    "lfs": {"sha256": sha256},
                }
            ],
        }

        with mock.patch.object(download_models.requests, "get", return_value=FakeRepoInfoResponse(payload)):
            commit, files = download_models._get_repo_info("org/model", "https://hf.example")

        self.assertEqual(commit, "commit123")
        self.assertEqual(files[0]["sha256"], sha256)

    def test_write_completion_manifest_records_and_verifies_sha256(self):
        with tempfile.TemporaryDirectory() as tmp:
            data = b"model-bytes"
            expected_sha256 = hashlib.sha256(data).hexdigest()
            path = os.path.join(tmp, "model.safetensors")
            with open(path, "wb") as f:
                f.write(data)

            download_models._write_completion_manifest(
                tmp,
                "org/model",
                "commit123",
                [
                    {
                        "rfilename": "model.safetensors",
                        "size": len(data),
                        "sha256": expected_sha256,
                    }
                ],
            )

            manifest_path = os.path.join(tmp, download_models.COMPLETE_MANIFEST_NAME)
            with open(manifest_path, "r", encoding="utf-8") as f:
                manifest = json.load(f)
            self.assertEqual(manifest["files"][0]["sha256"], expected_sha256)

    def test_write_completion_manifest_rejects_sha256_mismatch(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "model.safetensors")
            with open(path, "wb") as f:
                f.write(b"actual")

            with self.assertRaisesRegex(RuntimeError, "sha256|hash|校验"):
                download_models._write_completion_manifest(
                    tmp,
                    "org/model",
                    "commit123",
                    [
                        {
                            "rfilename": "model.safetensors",
                            "size": len(b"actual"),
                            "sha256": hashlib.sha256(b"expected").hexdigest(),
                        }
                    ],
                )

    def test_snapshot_matches_completion_manifest_checks_sha256(self):
        with tempfile.TemporaryDirectory() as tmp:
            data = b"x" * 1_000_000
            expected_sha256 = hashlib.sha256(data).hexdigest()
            path = os.path.join(tmp, "model.safetensors")
            with open(path, "wb") as f:
                f.write(data)
            manifest = {
                "repo_id": "org/model",
                "commit_hash": "commit123",
                "files": [
                    {
                        "path": "model.safetensors",
                        "size": len(data),
                        "sha256": expected_sha256,
                    }
                ],
            }
            manifest_path = os.path.join(tmp, hf_cache_utils.COMPLETE_MANIFEST_NAME)
            with open(manifest_path, "w", encoding="utf-8") as f:
                json.dump(manifest, f)

            self.assertTrue(hf_cache_utils._snapshot_matches_completion_manifest(tmp))

            manifest["files"][0]["sha256"] = hashlib.sha256(b"different").hexdigest()
            with open(manifest_path, "w", encoding="utf-8") as f:
                json.dump(manifest, f)

            self.assertFalse(hf_cache_utils._snapshot_matches_completion_manifest(tmp))

    def test_download_file_uses_resolved_commit_in_url_when_helper_is_available(self):
        helper = getattr(download_models, "_resolve_download_url", None)
        if helper is None:
            self.skipTest("_resolve_download_url helper is not implemented yet")

        self.assertEqual(
            helper("https://hf.example", "org/model", "nested/model.bin", "commit123"),
            "https://hf.example/org/model/resolve/commit123/nested/model.bin",
        )

    def test_interrupted_download_does_not_leave_partial_final_file(self):
        with tempfile.TemporaryDirectory() as tmp:
            dest_path = os.path.join(tmp, "snapshot", "model.bin")

            with mock.patch.object(download_models.requests, "get", return_value=FakeStreamingResponse()):
                with self.assertRaises(RuntimeError):
                    download_models._download_file(
                        "org/model",
                        "model.bin",
                        dest_path,
                        "asr",
                        1,
                        1,
                        "https://hf.example",
                        expected_size=6,
                    )

            self.assertFalse(
                os.path.exists(dest_path),
                "interrupted downloads must write to an incomplete/temp path and atomically rename only after success",
            )

    def test_existing_tiny_weight_file_is_not_treated_as_complete(self):
        with tempfile.TemporaryDirectory() as cache_root:
            repo_dir = os.path.join(cache_root, "models--org--model")
            snapshot_dir = os.path.join(repo_dir, "snapshots", "commit")
            os.makedirs(snapshot_dir, exist_ok=True)
            partial_path = os.path.join(snapshot_dir, "model.bin")
            with open(partial_path, "wb") as f:
                f.write(b"partial")

            calls = []

            def fake_download(*args, **kwargs):
                calls.append(args)

            with (
                mock.patch.object(download_models, "get_hf_cache_root", return_value=cache_root),
                mock.patch.object(download_models, "is_hf_repo_ready", return_value=False),
                mock.patch.object(
                    download_models,
                    "_get_repo_info",
                    return_value=("commit", [{"rfilename": "model.bin", "size": 10}]),
                ),
                mock.patch.object(download_models, "_download_file", side_effect=fake_download),
                mock.patch.object(download_models, "_write_completion_manifest"),
                mock.patch.object(download_models, "_emit"),
            ):
                result = download_models.download_model({"name": "org/model", "type": "asr"})

            self.assertTrue(result["success"])
            self.assertEqual(
                len(calls),
                1,
                "download_model must re-download existing partial/tiny weight files instead of skipping any non-empty file",
            )

    def test_existing_file_is_reused_when_remote_size_is_unknown(self):
        with tempfile.TemporaryDirectory() as tmp:
            dest_path = os.path.join(tmp, "snapshot", "model.bin")
            os.makedirs(os.path.dirname(dest_path), exist_ok=True)
            with open(dest_path, "wb") as f:
                f.write(b"abc")

            with (
                mock.patch.object(download_models, "_remote_file_size", return_value=None),
                mock.patch.object(download_models.requests, "get") as mock_get,
            ):
                download_models._download_file(
                    "org/model",
                    "model.bin",
                    dest_path,
                    "asr",
                    1,
                    1,
                    "https://hf.example",
                    expected_size=None,
                )

            self.assertTrue(os.path.exists(dest_path))
            with open(dest_path, "rb") as f:
                self.assertEqual(f.read(), b"abc")
            mock_get.assert_not_called()

    def test_stale_incomplete_416_retries_from_scratch_when_size_unknown(self):
        with tempfile.TemporaryDirectory() as tmp:
            dest_path = os.path.join(tmp, "snapshot", "model.bin")
            os.makedirs(os.path.dirname(dest_path), exist_ok=True)
            with open(dest_path + ".incomplete", "wb") as f:
                f.write(b"stale")

            with (
                mock.patch.object(download_models, "_remote_file_size", return_value=None),
                mock.patch.object(
                    download_models.requests,
                    "get",
                    side_effect=[FakeRangeNotSatisfiableResponse(), FakeCompleteResponse()],
                ),
            ):
                download_models._download_file(
                    "org/model",
                    "model.bin",
                    dest_path,
                    "asr",
                    1,
                    1,
                    "https://hf.example",
                    expected_size=None,
                )

            self.assertTrue(os.path.exists(dest_path))
            self.assertFalse(os.path.exists(dest_path + ".incomplete"))
            with open(dest_path, "rb") as f:
                self.assertEqual(f.read(), b"xyz")

    def test_cleanup_locks_removes_all_repo_incomplete_files(self):
        with tempfile.TemporaryDirectory() as cache_root:
            repo_dir = os.path.join(cache_root, "models--org--model")
            blob_incomplete = os.path.join(repo_dir, "blobs", "abc.incomplete")
            snapshot_incomplete = os.path.join(
                repo_dir,
                "snapshots",
                "commit",
                "nested",
                "model.bin.incomplete",
            )
            complete_file = os.path.join(repo_dir, "snapshots", "commit", "model.bin")
            for path in (blob_incomplete, snapshot_incomplete, complete_file):
                os.makedirs(os.path.dirname(path), exist_ok=True)
                with open(path, "wb") as f:
                    f.write(b"x")

            with (
                mock.patch.object(hf_cache_utils, "get_hf_cache_root", return_value=cache_root),
                mock.patch.object(download_models, "get_hf_cache_root", return_value=cache_root),
                mock.patch.object(download_models, "_emit"),
            ):
                download_models._cleanup_locks("org/model")

            self.assertFalse(os.path.exists(blob_incomplete))
            self.assertFalse(os.path.exists(snapshot_incomplete))
            self.assertTrue(os.path.exists(complete_file))


if __name__ == "__main__":
    unittest.main()
