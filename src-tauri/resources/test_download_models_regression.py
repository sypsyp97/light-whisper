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

    def close(self):
        return None


class FakeBodyResponse:
    def __init__(self, status_code, body, headers=None):
        self.status_code = status_code
        self.body = body
        self.headers = headers or {}
        self.closed = False

    def raise_for_status(self):
        return None

    def iter_content(self, chunk_size):
        yield self.body

    def close(self):
        self.closed = True


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

    def test_new_download_process_preserves_partial_file_and_resumes_with_range(self):
        with tempfile.TemporaryDirectory() as cache_root:
            repo_dir = os.path.join(cache_root, "models--org--model")
            dest_path = os.path.join(repo_dir, "snapshots", "commit", "model.bin")
            incomplete_path = dest_path + ".incomplete"
            os.makedirs(os.path.dirname(incomplete_path), exist_ok=True)
            with open(incomplete_path, "wb") as f:
                f.write(b"abc")

            observed_headers = []

            def fake_get(_url, *, headers, **_kwargs):
                observed_headers.append(dict(headers))
                if headers.get("Range") == "bytes=3-":
                    response = FakeCompleteResponse()
                    response.status_code = 206
                    response.headers = {
                        "Content-Length": "3",
                        "Content-Range": "bytes 3-5/6",
                    }
                    return response

                response = FakeCompleteResponse()
                response.headers = {"Content-Length": "6"}
                response.iter_content = lambda chunk_size: iter((b"abcxyz",))
                return response

            with (
                mock.patch.object(hf_cache_utils, "get_hf_cache_root", return_value=cache_root),
                mock.patch.object(download_models, "get_hf_cache_root", return_value=cache_root),
                mock.patch.object(download_models.requests, "get", side_effect=fake_get),
            ):
                download_models._cleanup_locks("org/model")
                partial_survived_cleanup = os.path.exists(incomplete_path)
                download_models._download_file(
                    "org/model",
                    "model.bin",
                    dest_path,
                    "asr",
                    1,
                    1,
                    "https://hf.example",
                    expected_size=6,
                    revision="commit",
                )

            self.assertEqual(
                (partial_survived_cleanup, observed_headers[0].get("Range")),
                (True, "bytes=3-"),
                "a new download process must preserve a valid partial file and resume it with an HTTP Range request",
            )

    def test_mismatched_content_range_discards_partial_and_retries_full_download(self):
        with tempfile.TemporaryDirectory() as tmp:
            dest_path = os.path.join(tmp, "snapshot", "model.bin")
            os.makedirs(os.path.dirname(dest_path), exist_ok=True)
            with open(dest_path + ".incomplete", "wb") as f:
                f.write(b"abc")

            bad_range = FakeBodyResponse(
                206,
                b"XYZ",
                {"Content-Length": "3", "Content-Range": "bytes 0-2/6"},
            )
            full_response = FakeBodyResponse(200, b"abcxyz", {"Content-Length": "6"})
            observed_headers = []

            def fake_get(_url, *, headers, **_kwargs):
                observed_headers.append(dict(headers))
                return bad_range if len(observed_headers) == 1 else full_response

            with mock.patch.object(download_models.requests, "get", side_effect=fake_get):
                download_models._download_file(
                    "org/model",
                    "model.bin",
                    dest_path,
                    "asr",
                    1,
                    1,
                    "https://hf.example",
                    expected_size=6,
                    revision="commit",
                )

            self.assertEqual(observed_headers[0].get("Range"), "bytes=3-")
            self.assertNotIn("Range", observed_headers[1])
            self.assertTrue(bad_range.closed)
            with open(dest_path, "rb") as f:
                self.assertEqual(f.read(), b"abcxyz")

    def test_missing_content_range_does_not_append_206_body(self):
        with tempfile.TemporaryDirectory() as tmp:
            dest_path = os.path.join(tmp, "snapshot", "model.bin")
            os.makedirs(os.path.dirname(dest_path), exist_ok=True)
            with open(dest_path + ".incomplete", "wb") as f:
                f.write(b"abc")

            missing_range = FakeBodyResponse(206, b"XYZ", {"Content-Length": "3"})
            full_response = FakeBodyResponse(200, b"abcxyz", {"Content-Length": "6"})

            with mock.patch.object(
                download_models.requests,
                "get",
                side_effect=[missing_range, full_response],
            ):
                download_models._download_file(
                    "org/model",
                    "model.bin",
                    dest_path,
                    "asr",
                    1,
                    1,
                    "https://hf.example",
                    expected_size=6,
                    revision="commit",
                )

            self.assertTrue(missing_range.closed)
            with open(dest_path, "rb") as f:
                self.assertEqual(f.read(), b"abcxyz")

    def test_content_range_body_length_mismatch_discards_untrusted_partial(self):
        with tempfile.TemporaryDirectory() as tmp:
            dest_path = os.path.join(tmp, "snapshot", "model.bin")
            os.makedirs(os.path.dirname(dest_path), exist_ok=True)
            with open(dest_path + ".incomplete", "wb") as f:
                f.write(b"abc")

            bad_response = FakeBodyResponse(
                206,
                b"XYZ",
                {"Content-Length": "3", "Content-Range": "bytes 3-4/6"},
            )
            full_response = FakeBodyResponse(200, b"abcxyz", {"Content-Length": "6"})
            with mock.patch.object(
                download_models.requests,
                "get",
                side_effect=[bad_response, full_response],
            ) as mock_get:
                download_models._download_file(
                    "org/model",
                    "model.bin",
                    dest_path,
                    "asr",
                    1,
                    1,
                    "https://hf.example",
                    expected_size=6,
                    revision="commit",
                )

            self.assertEqual(mock_get.call_count, 2)
            self.assertTrue(bad_response.closed)
            with open(dest_path, "rb") as f:
                self.assertEqual(f.read(), b"abcxyz")
            self.assertFalse(os.path.exists(dest_path + ".incomplete"))

    def test_416_conflicting_remote_total_retries_full_download(self):
        with tempfile.TemporaryDirectory() as tmp:
            dest_path = os.path.join(tmp, "snapshot", "model.bin")
            os.makedirs(os.path.dirname(dest_path), exist_ok=True)
            with open(dest_path + ".incomplete", "wb") as f:
                f.write(b"ABCDEF")

            conflict = FakeRangeNotSatisfiableResponse()
            conflict.headers = {"Content-Range": "bytes */7"}
            full_response = FakeBodyResponse(200, b"abcxyz", {"Content-Length": "6"})
            with mock.patch.object(
                download_models.requests,
                "get",
                side_effect=[conflict, full_response],
            ) as mock_get:
                download_models._download_file(
                    "org/model",
                    "model.bin",
                    dest_path,
                    "asr",
                    1,
                    1,
                    "https://hf.example",
                    expected_size=6,
                    revision="commit",
                )

            self.assertEqual(mock_get.call_count, 2)
            with open(dest_path, "rb") as f:
                self.assertEqual(f.read(), b"abcxyz")

    def test_http_error_closes_streaming_response(self):
        with tempfile.TemporaryDirectory() as tmp:
            dest_path = os.path.join(tmp, "snapshot", "model.bin")
            response = FakeBodyResponse(404, b"")
            response.raise_for_status = mock.Mock(side_effect=RuntimeError("http error"))

            with (
                mock.patch.object(download_models.requests, "get", return_value=response),
                self.assertRaisesRegex(RuntimeError, "http error"),
            ):
                download_models._download_file(
                    "org/model",
                    "model.bin",
                    dest_path,
                    "asr",
                    1,
                    1,
                    "https://hf.example",
                    expected_size=6,
                    revision="commit",
                )

            self.assertTrue(response.closed)

    def test_invalid_content_length_is_ignored_and_response_is_closed(self):
        with tempfile.TemporaryDirectory() as tmp:
            dest_path = os.path.join(tmp, "snapshot", "model.bin")
            response = FakeBodyResponse(200, b"abc", {"Content-Length": "invalid"})

            with (
                mock.patch.object(download_models, "_remote_file_size", return_value=None),
                mock.patch.object(download_models.requests, "get", return_value=response),
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
                    revision="commit",
                )

            self.assertTrue(response.closed)
            with open(dest_path, "rb") as f:
                self.assertEqual(f.read(), b"abc")

    def test_unknown_size_uses_content_range_total(self):
        with tempfile.TemporaryDirectory() as tmp:
            dest_path = os.path.join(tmp, "snapshot", "model.bin")
            os.makedirs(os.path.dirname(dest_path), exist_ok=True)
            with open(dest_path + ".incomplete", "wb") as f:
                f.write(b"abc")

            response = FakeBodyResponse(
                206,
                b"xyz",
                {"Content-Length": "3", "Content-Range": "bytes 3-5/6"},
            )
            with (
                mock.patch.object(download_models, "_remote_file_size", return_value=None),
                mock.patch.object(download_models.requests, "get", return_value=response),
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
                    revision="commit",
                )

            with open(dest_path, "rb") as f:
                self.assertEqual(f.read(), b"abcxyz")

    def test_cleanup_locks_removes_legacy_blob_partial_but_preserves_resumable_snapshot(self):
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
            self.assertTrue(os.path.exists(snapshot_incomplete))
            self.assertTrue(os.path.exists(complete_file))


if __name__ == "__main__":
    unittest.main()
