"""Quality CLI replay through the production Python stack, with deterministic audio.cpp and VAD."""
import ctypes
import hashlib
import json
import logging
import os
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import numpy as np

import benchmark_asr_quality as quality

RESOURCES = Path(__file__).resolve().parents[1] / "src-tauri" / "resources"
with patch.object(sys, "path", [str(RESOURCES), *sys.path]):
    import firered_vad
    import server_common
    from r2t2_native import NativeRuntime
    from test_r2t2_finish_context import FinishLibrary


class ReplayLibrary(FinishLibrary):
    """Publish the first tentative text on push two of each native segment."""

    def __init__(self, abi, fail=None):
        super().__init__(abi, fail)
        self.segments = []
        self.current = []

    def call(self, name, *args):
        status = super().call(name, *args)
        if name == self.fail:
            return status
        if name == "stream_start":
            self.current = []
        elif name == "stream_push":
            self.current.append(self.pushes[-1][1])
            args[-1]._obj.value = 1
        elif name == "event_as_result":
            return args[0]
        elif name == "result_text":
            args[1]._obj.value = b"" if args[0].value == 1 else b"word"
            args[2]._obj.value = b"en"
        elif name == "result_preview_text":
            args[1]._obj.value = b"word" if len(self.current) >= 2 else b""
        elif name == "stream_finish":
            self.segments.append(self.current)
        return status


def detect_nonzero_speech(window):
    voiced = np.flatnonzero(window)
    return [{"start": int(voiced[0]), "end": int(voiced[-1]) + 1}] if voiced.size else []


class QualityReplayTests(unittest.TestCase):
    def replay(self, samples, abi=0x0600, fail=None, *, expected_exit=0):
        library = ReplayLibrary(abi, fail)
        raw = samples.astype("<i2").tobytes()
        fixture = {"id": "test-clip", "reference": "word"}
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            args = ["quality", "--family", "r2t2", "--source-dir", str(RESOURCES),
                    "--dependencies", str(root), "--output", str(root / "report.json")]
            for name in ("model", "library", "manifest", "vad-model", "vad-cmvn"):
                path = root / name
                path.write_bytes(name.encode())
                args.extend(["--" + name, str(path)])
            for name in ("cublasLt64_12.dll", "cublas64_12.dll", "cudart64_12.dll"):
                (root / name).touch()
            # The CLI's Windows gate is independent of the host that runs this test.
            host = SimpleNamespace(**vars(os))
            host.name = "nt"
            with (
                patch.object(quality, "os", host),
                patch.object(host, "add_dll_directory", create=True),
                patch.object(ctypes, "WinDLL", create=True),
                patch.object(ctypes, "CDLL", return_value=library),
                patch.object(os, "add_dll_directory", create=True),
                patch.dict(os.environ),
                patch.dict(sys.modules),
                patch.object(server_common, "setup_rotating_logger", return_value=logging.getLogger(__name__)),
                patch.object(sys, "path", sys.path.copy()),
                patch.object(sys, "argv", args),
                patch.object(quality, "load_fixtures", return_value=({}, [(fixture, raw)])),
                patch.object(firered_vad.FireRedVad, "__init__", return_value=None),
                patch.object(firered_vad.FireRedVad, "speech_timestamps",
                             side_effect=detect_nonzero_speech),
            ):
                exit_code = quality.main()
                report = json.loads((root / "report.json").read_text(encoding="utf-8"))
        self.assertEqual(exit_code, expected_exit, report.get("traceback"))
        self.assertEqual(report["cleanup_errors"], [])
        self.assertTrue(report["native_closed"])
        for acquire, release in (("registry_create", "registry_free"),
                                 ("model_load", "model_free"), ("session_create", "session_free")):
            self.assertGreater(library.count(acquire), 0)
            self.assertEqual(library.count(acquire), library.count(release))
        return report, library

    def test_native_policy_matches_report_while_capture_and_push_grain_stay_fixed(self):
        samples = np.arange(6 * 2560 + 13, dtype=np.int16)
        runs = {abi: self.replay(samples, abi) for abi in (0x0600, 0x0500)}
        for abi, steady in ((0x0600, 5120), (0x0500, 2560)):
            with self.subTest(abi=hex(abi)):
                report, native = runs[abi]
                row = report["rows"][0]
                self.assertEqual([feed["samples"] for feed in row["feeds"]], [2560] * 6 + [13])
                self.assertEqual(row["accepted_pcm"]["pcm16_sha256"],
                                 hashlib.sha256(samples.astype("<i2").tobytes()).hexdigest())
                self.assertEqual(len(native.segments), 1)
                blocks = native.segments[0]
                self.assertEqual([len(pcm) for pcm in blocks], [2560] * 6 + [13])
                np.testing.assert_array_equal(np.concatenate(blocks), samples.astype(np.float32) / 32768)
                self.assertEqual(native.session_options.get(b"confucius4_r2t2.adaptive_cadence"),
                                 b"true" if abi == 0x0600 else None)
                self.assertEqual(row["final"], {"text": "word", "language": "en", "sample_count": len(samples)})
                self.assertEqual(report["config"]["feed_chunk_samples"], 2560)
                self.assertEqual(report["config"].get("r2t2_startup_chunk_samples"), 2560)
                self.assertEqual(report["config"].get("r2t2_steady_chunk_samples"), steady)
                for key, data in (("vad_model", b"vad-model"), ("vad_cmvn", b"vad-cmvn")):
                    self.assertEqual(report["inputs"].get(key), hashlib.sha256(data).hexdigest())
        adaptive, legacy = runs[0x0600][0], runs[0x0500][0]
        self.assertEqual(quality.compare_reference(adaptive, adaptive)["status"], "match")
        with self.assertRaises(AssertionError):
            quality.compare_reference(adaptive, legacy)

    def test_silence_retains_capture_count_with_no_native_finish_context(self):
        samples = np.zeros(2560 + 13, dtype=np.int16)
        report, native = self.replay(samples)
        row = report["rows"][0]
        self.assertEqual(len(native.segments), 0)
        self.assertEqual(row["final"]["text"], "")
        self.assertEqual(row["final"]["sample_count"], len(samples))
        self.assertEqual(row["sample_accounting"]["native_final_samples"], 0)
        self.assertEqual(row["sample_accounting"]["finish_context_samples"], 0)

    def test_vad_pause_accounts_for_both_finishes_and_the_last_thirteen_samples(self):
        # The first segment ends after 8 s of speech and two silent capture packets.
        samples = np.concatenate((np.ones(128000), np.zeros(5120), np.full(10240 + 13, 2)))
        report, native = self.replay(samples)
        row = report["rows"][0]
        self.assertEqual(len(native.segments), 2)
        self.assertEqual([len(pcm) for pcm in native.segments[-1]], [2560] * 4 + [13])
        blocks = [pcm for segment in native.segments for pcm in segment]
        np.testing.assert_array_equal(np.concatenate(blocks), samples.astype(np.float32) / 32768)
        self.assertEqual(row["final"], {"text": "word word", "language": "en", "sample_count": len(samples)})
        self.assertEqual(row["sample_accounting"]["finish_context_samples"], 2 * 5120)
        self.assertEqual(row["sample_accounting"]["native_final_samples"], len(samples) + 2 * 5120)

    def test_native_finish_error_marks_clip_failed_and_releases_native_handles(self):
        report, _ = self.replay(np.ones(2560 + 13), fail="stream_finish", expected_exit=1)
        self.assertEqual(report["status"], "failed")
        self.assertEqual(report["rows"][0]["status"], "failed")
        self.assertIn("stream_finish", report["error"])

    def test_one_missing_native_sample_invalidates_the_report_at_feed_or_finish(self):
        for operation in ("feed", "finish"):
            with self.subTest(operation=operation):
                original = getattr(NativeRuntime, operation)

                def lose_sample(runtime, *args):
                    result = original(runtime, *args)
                    if operation == "finish" or len(args[0]) == 13:
                        runtime._offset -= 1
                    return result

                with patch.object(NativeRuntime, operation, lose_sample):
                    report, library = self.replay(np.ones(2560 + 13), expected_exit=1)
                self.assertEqual(report["status"], "failed")
                self.assertEqual(report["rows"][0]["status"], "failed")
                self.assertTrue(report["error"].startswith("AssertionError:"), report["error"])
                self.assertEqual(library.count("stream_finish"), int(operation == "finish"))


if __name__ == "__main__":
    unittest.main()
