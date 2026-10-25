"""Qwen backlog scheduling through real adapters; deterministic callbacks, no GPU."""

import base64
import contextlib
import logging
import os
import sys
from types import SimpleNamespace
import unittest

import numpy as np

sys.path.insert(0, os.path.dirname(__file__))

from asr_stream_server import ASRStreamServerMixin
from qwen3_native import QwenPreviewRuntime
from r2t2_stream import R2T2StreamSession
from test_qwen_preview_final_reuse import RecordingTranscribe
from test_r2t2_stream import RecordingRuntime


class StreamServer(ASRStreamServerMixin):
    """Only device initialization is replaced; dispatch and session are real."""

    def __init__(self, runtime):
        self.initialized = True
        self.engine = "qwen3-asr-0.6b"
        self.backend = "cpu"
        self._stream_display_name = "Qwen3-ASR"
        self.stream = R2T2StreamSession(runtime)
        self.stdout_suppressor = SimpleNamespace(suppress=contextlib.nullcontext)
        self.logger = logging.Logger(__name__)
        self.logger.addHandler(logging.NullHandler())
        self.transcription_count = 0
        self.total_audio_duration = 0.0
        self._total_inference_ms = 0.0


def feed_command(pcm, *, session_id=1, offset=0, **fields):
    return {
        "action": "stream_feed", "session_id": session_id, "offset": offset,
        "audio_format": "pcm_s16le", "sample_rate": 16000,
        "audio_base64": base64.b64encode(np.asarray(pcm, dtype="<i2").tobytes()).decode("ascii"),
        **fields,
    }


class QwenPreviewBacklogTests(unittest.TestCase):
    def assert_decode(self, call, pcm, *, context="术语", language="zh"):
        audio, options = call
        np.testing.assert_array_equal(audio, np.pad(pcm, (0, max(0, 8000 - len(pcm)))))
        self.assertEqual(audio.dtype, np.float32)
        self.assertEqual(options, {"context": context, "language": language})

    def test_stop_ingests_ten_seconds_and_one_sample_without_any_preview(self):
        transcribe = RecordingTranscribe(("完整最终结果", "Chinese"))
        runtime = QwenPreviewRuntime(transcribe, window_samples=9600)
        runtime.start("术语", "zh")
        pcm = np.linspace(-0.8, 0.8, 160001, dtype=np.float32)
        responses = []
        for offset in range(0, len(pcm), 5120):
            borrowed = pcm[offset:offset + 5120].copy()
            responses.append(runtime.feed(borrowed, preview=False))
            borrowed.fill(0)  # Every accepted sample must already be owned by the runtime.

        self.assertEqual(len(transcribe.calls), 0, "Stop must not drain queued preview decodes")
        self.assertTrue(all(response == ("", "zh") for response in responses))
        self.assertEqual(runtime.preview_text, "")
        self.assertEqual(runtime.finish(), ("完整最终结果", "Chinese"))
        self.assertEqual(len(transcribe.calls), 1)
        self.assert_decode(transcribe.calls[0], pcm)

    def test_latest_live_feed_decodes_once_after_a_suppressed_backlog(self):
        transcribe = RecordingTranscribe(("最新窗口", "Chinese"), ("完整最终结果", None))
        runtime = QwenPreviewRuntime(transcribe, window_samples=9600)
        runtime.start("术语", "zh")
        pcm = np.linspace(-0.7, 0.7, 160001, dtype=np.float32)
        for offset in range(0, 160000, 5120):
            runtime.feed(pcm[offset:min(offset + 5120, 160000)], preview=False)
        self.assertEqual(len(transcribe.calls), 0)

        self.assertEqual(runtime.feed(pcm[-1:], preview=True), ("", "Chinese"))
        self.assertEqual(len(transcribe.calls), 1)
        self.assertEqual(runtime.preview_text, "最新窗口")
        self.assert_decode(transcribe.calls[0], pcm[-9600:])
        self.assertEqual(runtime.finish(), ("完整最终结果", "Chinese"))
        self.assertEqual(len(transcribe.calls), 2)
        self.assert_decode(transcribe.calls[1], pcm)

    def test_suppressed_backlog_keeps_caption_and_language_until_final(self):
        transcribe = RecordingTranscribe(("已有草稿", "Chinese"), ("完整最终结果", None))
        runtime = QwenPreviewRuntime(transcribe)
        runtime.start("术语", "zh")
        pcm = np.linspace(-0.3, 0.3, 5120, dtype=np.float32)
        runtime.feed(pcm[:1920])
        self.assertEqual(runtime.feed(pcm[1920:], preview=False), ("", "Chinese"))
        self.assertEqual(len(transcribe.calls), 1)
        self.assertEqual(runtime.preview_text, "已有草稿")
        self.assertEqual(runtime.finish(), ("完整最终结果", "Chinese"))
        self.assertEqual(len(transcribe.calls), 2)
        self.assert_decode(transcribe.calls[1], pcm)

    def test_exact_full_window_preview_after_suppression_is_reused(self):
        for count in (1920, 9600):
            with self.subTest(count=count):
                transcribe = RecordingTranscribe(("最终结果", "Chinese"))
                runtime = QwenPreviewRuntime(transcribe, window_samples=9600)
                runtime.start("术语", "zh")
                pcm = np.linspace(-0.2, 0.2, count, dtype=np.float32)
                runtime.feed(pcm[:-1], preview=False)
                self.assertEqual(len(transcribe.calls), 0)
                runtime.feed(pcm[-1:], preview=True)
                self.assertEqual(runtime.finish(), ("最终结果", "Chinese"))
                self.assertEqual(len(transcribe.calls), 1)
                self.assert_decode(transcribe.calls[0], pcm)

    def test_one_suppressed_sample_prevents_stale_final_reuse(self):
        transcribe = RecordingTranscribe(("草稿", "Chinese"), ("带尾音", None))
        runtime = QwenPreviewRuntime(transcribe)
        runtime.start("术语", "zh")
        pcm = np.concatenate((np.full(1920, 0.125, dtype=np.float32),
                              np.asarray([-0.875], dtype=np.float32)))
        runtime.feed(pcm[:-1])
        runtime.feed(pcm[-1:], preview=False)
        self.assertEqual(len(transcribe.calls), 1)
        self.assertEqual(runtime.finish(), ("带尾音", "Chinese"))
        self.assertEqual(len(transcribe.calls), 2)
        self.assert_decode(transcribe.calls[1], pcm)

    def test_default_and_explicit_true_keep_startup_and_steady_preview_cadence(self):
        for explicit in (False, True):
            with self.subTest(explicit=explicit):
                transcribe = RecordingTranscribe(("草稿", "Chinese"))
                runtime = QwenPreviewRuntime(transcribe)
                runtime.start("术语", "zh")
                pcm = np.linspace(-0.4, 0.4, 5120, dtype=np.float32)
                counts = []
                for offset in range(0, len(pcm), 640):
                    runtime.feed(pcm[offset:offset + 640], **({"preview": True} if explicit else {}))
                    counts.append(len(transcribe.calls))
                self.assertEqual(counts, [0, 0, 1, 1, 1, 1, 1, 2])
                self.assert_decode(transcribe.calls[0], pcm[:1920])
                self.assert_decode(transcribe.calls[1], pcm)

    def test_non_boolean_runtime_preview_is_rejected_without_accepting_audio(self):
        for invalid in (None, 0, 1, "false", [], {}, np.bool_(False)):
            with self.subTest(preview=invalid):
                transcribe = RecordingTranscribe(("原有草稿", "Chinese"))
                runtime = QwenPreviewRuntime(transcribe)
                runtime.start("术语", "zh")
                pcm = np.full(1920, 0.25, dtype=np.float32)
                runtime.feed(pcm)
                with self.assertRaises(ValueError):
                    runtime.feed(np.full(17, -0.5, dtype=np.float32), preview=invalid)
                self.assertEqual(runtime.finish(), ("原有草稿", "Chinese"))
                self.assertEqual(len(transcribe.calls), 1)
                self.assert_decode(transcribe.calls[0], pcm)

    def test_session_invalid_preview_offset_and_identity_do_not_mutate_capture(self):
        invalid_fields = [{"preview": value} for value in (None, 0, 1, "false", [], {})]
        invalid_fields += [{"offset": value} for value in (-1, True, 1920.0, "1920", 1919, 1921)]
        invalid_fields += [{"session_id": value} for value in (0, 2, True, 1.0, "1")]
        for fields in invalid_fields:
            with self.subTest(fields=fields):
                transcribe = RecordingTranscribe(("原有草稿", "Chinese"))
                session = R2T2StreamSession(QwenPreviewRuntime(transcribe))
                session.start(1, context="术语", language="zh")
                pcm = np.full(1920, 0.25, dtype=np.float32)
                session.feed(1, pcm, offset=0)
                args = {"session_id": 1, "offset": 1920, "preview": False, **fields}
                with self.assertRaises(ValueError):
                    session.feed(pcm=np.full(17, -0.5, dtype=np.float32), **args)
                self.assertTrue(session.active)
                final = session.finish(1)
                self.assertEqual(final["sample_count"], len(pcm))
                self.assertEqual(final["text"], "原有草稿")
                self.assertEqual(len(transcribe.calls), 1)
                self.assert_decode(transcribe.calls[0], pcm)

    def test_cancel_and_replacement_isolate_suppressed_audio_context_and_language(self):
        for cancel_first in (False, True):
            with self.subTest(cancel_first=cancel_first):
                transcribe = RecordingTranscribe(("新会话", None))
                runtime = QwenPreviewRuntime(transcribe)
                session = R2T2StreamSession(runtime)
                session.start(1, context="旧上下文", language="zh")
                session.feed(1, np.full(16000, 0.5, dtype=np.float32), offset=0, preview=False)
                if cancel_first:
                    response = session.cancel(1)
                    self.assertEqual(response["sample_count"], 16000)
                    self.assertEqual(response["text"], "")
                self.assertEqual(len(transcribe.calls), 0)
                session.start(2, context="new context", language="en")
                pcm = np.full(13, -0.25, dtype=np.float32)
                session.feed(2, pcm, offset=0, preview=False)
                final = session.finish(2)
                self.assertEqual((final["text"], final["language"], final["sample_count"]),
                                 ("新会话", "en", 13))
                self.assertEqual(len(transcribe.calls), 1)
                self.assert_decode(transcribe.calls[0], pcm, context="new context", language="en")
                self.assertFalse(session.active)
                with self.assertRaises(RuntimeError):
                    runtime.feed(pcm, preview=False)

    def test_final_decode_error_after_suppression_resets_before_restart(self):
        transcribe = RecordingTranscribe(("未使用", None), ("恢复", "English"), fail_calls=(1,))
        runtime = QwenPreviewRuntime(transcribe)
        session = R2T2StreamSession(runtime)
        session.start(1, context="术语", language="zh")
        pcm = np.full(13, 0.5, dtype=np.float32)
        session.feed(1, pcm, offset=0, preview=False)
        with self.assertRaisesRegex(RuntimeError, "injected decode failure"):
            session.finish(1)
        self.assertFalse(session.active)
        self.assertEqual(runtime.preview_text, "")
        with self.assertRaises(RuntimeError):
            runtime.finish()
        session.start(2, context="new context", language="en")
        fresh = np.full(17, -0.25, dtype=np.float32)
        session.feed(2, fresh, offset=0, preview=False)
        self.assertEqual(session.finish(2)["text"], "恢复")
        self.assertEqual(len(transcribe.calls), 2)
        self.assert_decode(transcribe.calls[1], fresh, context="new context", language="en")

    def test_mixin_stop_packets_forward_false_and_preserve_complete_pcm(self):
        transcribe = RecordingTranscribe(("完整最终结果", "Chinese"))
        server = StreamServer(QwenPreviewRuntime(transcribe))
        start = server.handle_stream_command({
            "action": "stream_start", "session_id": 1, "context": "  术语 ", "language": " zh ",
        })
        self.assertTrue(start["success"])
        pcm = ((np.arange(160001, dtype=np.int32) % 65536) - 32768).astype("<i2")
        previews = []
        for offset in range(0, len(pcm), 5120):
            chunk = pcm[offset:offset + 5120]
            response = server.handle_stream_command(feed_command(chunk, offset=offset, preview=False))
            self.assertTrue(response["success"], response)
            self.assertEqual(response["sample_count"], offset + len(chunk))
            previews.append(response["tentative_text"])
        self.assertEqual(len(transcribe.calls), 0)
        self.assertTrue(all(preview == "" for preview in previews))
        final = server.handle_stream_command({"action": "stream_finish", "session_id": 1})
        self.assertTrue(final["success"], final)
        self.assertEqual((final["text"], final["sample_count"], final["final"]),
                         ("完整最终结果", len(pcm), True))
        self.assertEqual(len(transcribe.calls), 1)
        self.assert_decode(transcribe.calls[0], pcm.astype(np.float32) / 32768.0)

    def test_mixin_invalid_preview_types_leave_active_audio_unchanged(self):
        for invalid in (None, 0, 1, "false", [], {}):
            with self.subTest(preview=invalid):
                transcribe = RecordingTranscribe(("原有草稿", "Chinese"))
                server = StreamServer(QwenPreviewRuntime(transcribe))
                server.handle_stream_command({"action": "stream_start", "session_id": 1})
                pcm = np.full(1920, 1000, dtype="<i2")
                self.assertTrue(server.handle_stream_command(feed_command(pcm))["success"])
                response = server.handle_stream_command(feed_command([1], offset=1920, preview=invalid))
                self.assertIs(response["success"], False)
                self.assertEqual(response["type"], "stream_error")
                self.assertTrue(server.stream.active)
                final = server.handle_stream_command({"action": "stream_finish", "session_id": 1})
                self.assertEqual(final["sample_count"], len(pcm))
                self.assertEqual(len(transcribe.calls), 1)

    def test_r2t2_default_and_true_preserve_legacy_feed_signature_and_order(self):
        runtime = RecordingRuntime(feed_results=[("a", "en"), ("ab", None)], finish_result=("abc", None))
        server = StreamServer(runtime)
        server.engine = "confucius4-r2t2"
        server.handle_stream_command({"action": "stream_start", "session_id": 1})
        for offset, chunk, fields in ((0, [1, 2], {}), (2, [3], {"preview": True})):
            response = server.handle_stream_command(feed_command(chunk, offset=offset, **fields))
            self.assertTrue(response["success"], response)
        self.assertEqual(len(runtime.feed_inputs), 2)
        np.testing.assert_array_equal(np.concatenate(runtime.feed_inputs),
                                      np.asarray([1, 2, 3], dtype=np.float32) / 32768.0)
        self.assertEqual(server.handle_stream_command({"action": "stream_finish", "session_id": 1})["text"], "abc")

    def test_r2t2_false_is_rejected_without_silently_dropping_streaming_work(self):
        runtime = RecordingRuntime()
        server = StreamServer(runtime)
        server.engine = "confucius4-r2t2"
        server.handle_stream_command({"action": "stream_start", "session_id": 1})
        response = server.handle_stream_command(feed_command([1, 2, 3], preview=False))
        self.assertIs(response["success"], False)
        self.assertEqual(response["type"], "stream_error")
        self.assertEqual(len(runtime.feed_inputs), 0)
        self.assertEqual(runtime.finish_calls, 0)


if __name__ == "__main__":
    unittest.main()
