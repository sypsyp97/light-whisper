

import os
import sys
import unittest
from unittest import mock

import numpy as np

sys.path.insert(0, os.path.dirname(__file__))

from qwen3_native import QwenNativeRuntime
from test_qwen3_native import FakeLibrary


class QwenNativeLongAudioTests(unittest.TestCase):


    UNSPLIT_SAMPLES = 115 * 16000

    def setUp(self):
        self.library = FakeLibrary()
        self.enterContext(mock.patch("ctypes.CDLL", side_effect=lambda _path: self.library))
        self.enterContext(mock.patch("os.add_dll_directory", create=True,
                                     side_effect=lambda _path: mock.Mock()))

    def runtime(self):
        runtime = QwenNativeRuntime("model.gguf", "audio.dll")
        self.addCleanup(runtime.close)
        return runtime

    @staticmethod
    def pcm(samples):
        return np.linspace(-0.75, 0.75, samples, dtype=np.float32)

    def assert_request(self, request, pcm, chunk_mode, context="", language=None):
        self.assertEqual(request["format"], (16000, 1))
        np.testing.assert_array_equal(request["pcm"], pcm)
        self.assertEqual(request["context_language"], (
            context.encode("utf-8"), language.encode("utf-8") if language else None,
        ))
        self.assertEqual(request["options"], {
            b"max_tokens": b"512",
            b"return_timestamps": b"false",
            b"audio_chunk_mode": chunk_mode,
        })

    def assert_resident(self, requests):
        self.assertEqual(self.library.calls.count("model_load"), 1)
        self.assertEqual(self.library.calls.count("session_create"), 1)
        self.assertEqual(self.library.calls.count("session_run"), requests)
        self.assertEqual(len(self.library.requests), requests)
        self.assertEqual(len({request["handle"] for request in self.library.requests}), requests)

    def assert_all_allocations_freed_once(self):
        self.assertCountEqual(self.library.freed, [
            (kind, handle) for handle, kind in self.library.allocated.items()
        ])

    def test_short_and_exact_boundary_keep_existing_unsplit_options(self):
        runtime = self.runtime()
        for samples in (32 * 16000, self.UNSPLIT_SAMPLES):
            with self.subTest(samples=samples):
                pcm = self.pcm(samples)
                result = runtime.run(pcm, context="医学术语", language="Chinese")
                self.assertEqual((result.text, result.language), self.library.output)
                self.assert_request(self.library.requests[-1], pcm, b"none",
                                    context="医学术语", language="Chinese")
        self.assert_resident(2)
        runtime.close()
        self.assert_all_allocations_freed_once()

    def test_over_boundary_forwards_full_audio_to_one_native_auto_request(self):
        runtime = self.runtime()
        for samples in (self.UNSPLIT_SAMPLES + 1, self.UNSPLIT_SAMPLES + 13,
                        126 * 16000 + 13):
            with self.subTest(samples=samples):
                pcm = self.pcm(samples)
                result = runtime.run(pcm, context="保留尾句", language="Chinese")
                self.assertEqual((result.text, result.language), self.library.output)
                self.assert_request(self.library.requests[-1], pcm, b"auto",
                                    context="保留尾句", language="Chinese")
        self.assert_resident(3)
        runtime.close()
        self.assert_all_allocations_freed_once()

    def test_short_request_after_long_capture_clears_auto_context_and_language(self):
        runtime = self.runtime()
        long_pcm = self.pcm(self.UNSPLIT_SAMPLES + 13)
        short_pcm = self.pcm(13)
        runtime.run(long_pcm, context="旧上下文", language="Chinese")
        self.library.output = ("fresh short output", None)
        result = runtime.run(short_pcm)
        self.assertEqual((result.text, result.language), self.library.output)
        self.assert_resident(2)
        self.assert_request(self.library.requests[0], long_pcm, b"auto",
                            context="旧上下文", language="Chinese")
        self.assert_request(self.library.requests[1], short_pcm, b"none")
        runtime.close()
        self.assert_all_allocations_freed_once()

    def test_long_request_failure_frees_handles_and_next_short_request_is_clean(self):
        pcm = self.pcm(126 * 16000 + 13)
        for failure in ("request_set_audio", "session_run", "result_text"):
            with self.subTest(failure=failure):
                self.library = FakeLibrary()
                runtime = self.runtime()
                self.library.fail_at = failure
                with self.assertRaisesRegex(RuntimeError, "injected native failure"):
                    runtime.run(pcm, context="旧上下文", language="Chinese")
                self.library.fail_at = None
                short_pcm = self.pcm(13)
                result = runtime.run(short_pcm)
                self.assertEqual((result.text, result.language), self.library.output)
                runtime.close()
                self.assert_all_allocations_freed_once()
                self.assert_request(self.library.requests[1], short_pcm, b"none")
                self.assertEqual(self.library.calls.count("model_load"), 1)
                self.assertEqual(self.library.calls.count("session_create"), 1)
                self.assert_request(self.library.requests[0], pcm, b"auto",
                                    context="旧上下文", language="Chinese")


if __name__ == "__main__":
    unittest.main()
