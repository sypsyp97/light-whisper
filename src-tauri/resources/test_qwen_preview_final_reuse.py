

import os
import sys
from types import SimpleNamespace
import unittest

import numpy as np

sys.path.insert(0, os.path.dirname(__file__))

from qwen3_native import QwenPreviewRuntime
from r2t2_stream import R2T2StreamSession


class RecordingTranscribe:
    def __init__(self, *outputs, fail_calls=()):
        self.calls = []
        self.outputs = outputs
        self.fail_calls = fail_calls

    def __call__(self, pcm, **options):
        self.calls.append((pcm.copy(), options))
        if len(self.calls) in self.fail_calls:
            raise RuntimeError("injected decode failure")
        text, language = self.outputs[min(len(self.calls) - 1, len(self.outputs) - 1)]
        return SimpleNamespace(text=text, language=language)


class QwenPreviewFinalReuseTests(unittest.TestCase):
    def assert_decode(self, call, captured, *, context="", language=None):
        audio, options = call
        expected = np.pad(captured, (0, max(0, 8000 - len(captured))))
        np.testing.assert_array_equal(audio, expected)
        self.assertEqual(audio.dtype, np.float32)
        self.assertEqual(options, {"context": context, "language": language})

    def assert_inactive(self, runtime):
        self.assertEqual(runtime.preview_text, "")
        with self.assertRaises(RuntimeError):
            runtime.feed(np.ones(1, dtype=np.float32))
        with self.assertRaises(RuntimeError):
            runtime.finish()

    def test_exact_full_capture_preview_is_final_below_and_at_window_limit(self):
        for sample_count in (1920, 8000, 9600):
            with self.subTest(sample_count=sample_count):
                raw_text = "  原样\n草稿。  "
                transcribe = RecordingTranscribe((raw_text, "Chinese"))
                runtime = QwenPreviewRuntime(transcribe, window_samples=9600)
                runtime.start("术语", "zh")
                pcm = np.linspace(-0.7, 0.7, sample_count, dtype=np.float32)

                self.assertEqual(runtime.feed(pcm), ("", "Chinese"))
                self.assertEqual(runtime.preview_text, raw_text)
                self.assertEqual(len(transcribe.calls), 1)
                self.assert_decode(transcribe.calls[0], pcm, context="术语", language="zh")

                self.assertEqual(runtime.finish(), (raw_text, "Chinese"))
                self.assertEqual(len(transcribe.calls), 1)
                self.assert_inactive(runtime)

    def test_successful_empty_preview_is_reused_with_current_language_fallback(self):
        cases = (
            ((("", None),), (1920,), "zh"),
            ((("早先草稿", "Chinese"), ("", None)), (1920, 3200), "Chinese"),
        )
        for outputs, chunk_sizes, expected_language in cases:
            with self.subTest(outputs=outputs):
                transcribe = RecordingTranscribe(*outputs)
                runtime = QwenPreviewRuntime(transcribe)
                runtime.start("术语", "zh")
                pcm = np.linspace(-0.6, 0.6, sum(chunk_sizes), dtype=np.float32)
                offset = 0
                for index, chunk_size in enumerate(chunk_sizes):
                    runtime.feed(pcm[offset:offset + chunk_size])
                    offset += chunk_size
                    self.assert_decode(transcribe.calls[index], pcm[:offset],
                                       context="术语", language="zh")

                self.assertEqual(runtime.preview_text, "")
                self.assertEqual(runtime.finish(), ("", expected_language))
                self.assertEqual(len(transcribe.calls), len(chunk_sizes))

    def test_one_or_thirteen_new_samples_require_the_complete_padded_capture(self):
        for tail_samples in (1, 13):
            with self.subTest(tail_samples=tail_samples):
                transcribe = RecordingTranscribe(("草稿", "Chinese"), ("含尾音的最终结果", None))
                runtime = QwenPreviewRuntime(transcribe)
                runtime.start("术语", "zh")
                pcm = np.concatenate((np.linspace(-0.5, 0.5, 1920, dtype=np.float32),
                                      np.full(tail_samples, 0.875, dtype=np.float32)))
                runtime.feed(pcm[:1920])
                runtime.feed(pcm[1920:])
                self.assertEqual(len(transcribe.calls), 1)
                self.assert_decode(transcribe.calls[0], pcm[:1920], context="术语", language="zh")

                self.assertEqual(runtime.finish(), ("含尾音的最终结果", "Chinese"))
                self.assertEqual(len(transcribe.calls), 2)
                self.assert_decode(transcribe.calls[1], pcm, context="术语", language="zh")

    def test_latest_window_matching_a_short_capture_still_requires_earlier_speech(self):
        transcribe = RecordingTranscribe(("早先语音", "Chinese"), ("最后窗口", None),
                                         ("早先语音和最后窗口", None))
        runtime = QwenPreviewRuntime(transcribe, window_samples=9600)
        runtime.start("完整上下文", "zh")
        earlier = np.full(3200, 0.75, dtype=np.float32)
        latest = np.linspace(-0.4, 0.4, 9600, dtype=np.float32)
        runtime.feed(earlier)
        runtime.feed(latest)
        self.assertEqual(len(transcribe.calls), 2)
        self.assert_decode(transcribe.calls[0], earlier, context="完整上下文", language="zh")
        self.assert_decode(transcribe.calls[1], latest, context="完整上下文", language="zh")

        self.assertEqual(runtime.finish(), ("早先语音和最后窗口", "Chinese"))
        self.assertEqual(len(transcribe.calls), 3)
        self.assert_decode(transcribe.calls[2], np.concatenate((earlier, latest)),
                           context="完整上下文", language="zh")

    def test_start_and_reset_discard_old_preview_before_a_new_short_capture(self):
        for reset_first in (False, True):
            with self.subTest(reset_first=reset_first):
                transcribe = RecordingTranscribe(("旧草稿", "Chinese"), ("新内容", None))
                runtime = QwenPreviewRuntime(transcribe)
                runtime.start("旧上下文", "zh")
                old_pcm = np.full(1920, 0.5, dtype=np.float32)
                runtime.feed(old_pcm)
                if reset_first:
                    runtime.reset()
                    self.assert_inactive(runtime)
                runtime.start("新上下文", "en")
                fresh = np.linspace(-0.1, 0.1, 13, dtype=np.float32)
                self.assertEqual(runtime.preview_text, "")
                self.assertEqual(runtime.feed(fresh), ("", "en"))
                self.assertEqual(len(transcribe.calls), 1)

                self.assertEqual(runtime.finish(), ("新内容", "en"))
                self.assertEqual(len(transcribe.calls), 2)
                self.assert_decode(transcribe.calls[0], old_pcm, context="旧上下文", language="zh")
                self.assert_decode(transcribe.calls[1], fresh, context="新上下文", language="en")

    def test_required_final_failure_raises_and_resets_before_a_new_session(self):
        transcribe = RecordingTranscribe(("旧草稿", "Chinese"), ("未使用", None),
                                         ("新内容", None), fail_calls=(2,))
        runtime = QwenPreviewRuntime(transcribe)
        runtime.start("旧上下文", "zh")
        pcm = np.linspace(-0.5, 0.5, 1933, dtype=np.float32)
        runtime.feed(pcm[:1920])
        runtime.feed(pcm[1920:])
        with self.assertRaisesRegex(RuntimeError, "injected decode failure"):
            runtime.finish()
        self.assertEqual(len(transcribe.calls), 2)
        self.assert_decode(transcribe.calls[1], pcm, context="旧上下文", language="zh")
        self.assert_inactive(runtime)

        runtime.start()
        fresh = np.full(13, -0.25, dtype=np.float32)
        runtime.feed(fresh)
        self.assertEqual(runtime.finish(), ("新内容", None))
        self.assertEqual(len(transcribe.calls), 3)
        self.assert_decode(transcribe.calls[2], fresh)

    def test_failed_preview_discards_prior_result_and_active_capture(self):
        transcribe = RecordingTranscribe(("旧草稿", "Chinese"), ("未使用", None),
                                         ("新内容", None), fail_calls=(2,))
        runtime = QwenPreviewRuntime(transcribe)
        runtime.start("旧上下文", "zh")
        pcm = np.linspace(-0.5, 0.5, 5120, dtype=np.float32)
        runtime.feed(pcm[:1920])
        with self.assertRaisesRegex(RuntimeError, "injected decode failure"):
            runtime.feed(pcm[1920:])
        self.assertEqual(len(transcribe.calls), 2)
        self.assert_decode(transcribe.calls[1], pcm, context="旧上下文", language="zh")
        self.assert_inactive(runtime)

        runtime.start()
        fresh = np.full(13, -0.25, dtype=np.float32)
        runtime.feed(fresh)
        self.assertEqual(runtime.finish(), ("新内容", None))
        self.assertEqual(len(transcribe.calls), 3)
        self.assert_decode(transcribe.calls[2], fresh)

    def test_session_commits_preview_with_captured_sample_count_and_resets_after_hit(self):
        transcribe = RecordingTranscribe(("原样最终结果", "Chinese"), ("新内容", None))
        runtime = QwenPreviewRuntime(transcribe)
        session = R2T2StreamSession(runtime)
        session.start(7, context="术语", language="zh")
        pcm = np.linspace(-0.2, 0.2, 1920, dtype=np.float32)
        self.assertEqual(session.feed(7, pcm, offset=0), {
            "success": True, "session_id": 7, "text": "", "tentative_text": "原样最终结果",
            "language": "Chinese", "sample_count": 1920, "final": False,
        })
        self.assertEqual(session.finish(7), {
            "success": True, "session_id": 7, "text": "原样最终结果", "tentative_text": "",
            "language": "Chinese", "sample_count": 1920, "final": True,
        })
        self.assertEqual(len(transcribe.calls), 1)
        self.assert_decode(transcribe.calls[0], pcm, context="术语", language="zh")
        self.assertFalse(session.active)
        self.assert_inactive(runtime)
        with self.assertRaises(ValueError):
            session.finish(7)

        session.start(8, context="新上下文", language="en")
        fresh = np.full(13, -0.3, dtype=np.float32)
        session.feed(8, fresh, offset=0)
        self.assertEqual(session.finish(8), {
            "success": True, "session_id": 8, "text": "新内容", "tentative_text": "",
            "language": "en", "sample_count": 13, "final": True,
        })
        self.assertEqual(len(transcribe.calls), 2)
        self.assert_decode(transcribe.calls[1], fresh, context="新上下文", language="en")


if __name__ == "__main__":
    unittest.main()
