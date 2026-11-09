"""Common application stream contract; native inference stays deterministic."""

import base64
import contextlib
import logging
import os
import sys
import types
import unittest
from unittest import mock

import numpy as np

sys.path.insert(0, os.path.dirname(__file__))

import qwen3_asr_server
import r2t2_asr_server
from r2t2_stream import R2T2StreamSession


class CloseCounter:
    def __init__(self):
        self.close_calls = 0

    def close(self):
        self.close_calls += 1


class FakeOfflineSession(CloseCounter):
    def __init__(self):
        super().__init__()
        self.run_calls = 0

    def run(self, audio, **kwargs):
        self.run_calls += 1
        return types.SimpleNamespace(text="离线识别", language="zh")


class FakeVad:
    def speech_timestamps(self, audio):
        return [{"start": 0, "end": len(audio)}]


class FakeStreamRuntime(CloseCounter):
    """Only the native computation is replaced, not session validation."""

    chunk_samples = 5120

    def __init__(self, steps=(), final=("", None)):
        super().__init__()
        self.steps = list(steps)
        self.final = final
        self.preview_text = ""
        self.start_calls = []
        self.feed_inputs = []
        self.finish_calls = 0
        self.reset_calls = 0

    def start(self, context="", language=None):
        self.start_calls.append((context, language))
        self.preview_text = ""

    def feed(self, pcm):
        self.feed_inputs.append(np.array(pcm, copy=True))
        committed, self.preview_text, language = self.steps.pop(0)
        return committed, language

    def finish(self):
        self.finish_calls += 1
        return self.final

    def reset(self):
        self.reset_calls += 1
        self.preview_text = ""


class PreviewSuppressibleRuntime(FakeStreamRuntime):
    supports_preview_suppression = True

    def __init__(self, **kwargs):
        super().__init__(**kwargs)
        self.preview_flags = []

    def feed(self, pcm, *, preview=True):
        self.preview_flags.append(preview)
        return super().feed(pcm)


class SharedASRStreamContract:
    def _server(self, runtime):
        with (
            mock.patch.object(self.server_type, "_detect_device", return_value="cpu"),
            mock.patch("server_common.signal.signal"),
        ):
            server = self.server_type(**self.server_options)
        server.initialized = True
        server.backend = "cpu"
        server.native = runtime
        # Keep the incumbent Qwen close/run APIs usable while testing stream guards.
        server.model = CloseCounter()
        server.session = FakeOfflineSession()
        server.vad_model = FakeVad()
        server.stream = R2T2StreamSession(runtime)
        server.stdout_suppressor = types.SimpleNamespace(suppress=contextlib.nullcontext)
        server.logger = logging.getLogger(__name__)
        server.logger.addHandler(logging.NullHandler())
        server.logger.propagate = False
        self.addCleanup(server.stream.close)
        return server

    @staticmethod
    def _pcm16(values):
        return base64.b64encode(np.asarray(values, dtype="<i2").tobytes()).decode("ascii")

    def _feed(self, server, session_id, offset, samples):
        return server.handle_stream_command({
            "action": "stream_feed",
            "session_id": session_id,
            "offset": offset,
            "sample_rate": 16000,
            "audio_format": "pcm_s16le",
            "audio_base64": self._pcm16(samples),
        })

    def _assert_response(self, response, session_id, text, tentative, count, final,
                         language="zh"):
        self.assertIs(response.get("success"), True, response)
        self.assertEqual(response.get("engine"), self.engine)
        self.assertEqual(response.get("backend"), "cpu")
        self.assertIs(type(response.get("sample_count")), int)
        self.assertIs(type(response.get("final")), bool)
        self.assertEqual(
            {key: response.get(key) for key in (
                "session_id", "text", "tentative_text", "language", "sample_count", "final"
            )},
            dict(session_id=session_id, text=text, tentative_text=tentative,
                 language=language, sample_count=count, final=final),
        )

    def _assert_error(self, response):
        self.assertIs(response.get("success"), False, response)
        self.assertEqual(response.get("type"), "stream_error", response)
        self.assertEqual(response.get("engine"), self.engine)
        self.assertEqual(response.get("backend"), "cpu")
        self.assertIsInstance(response.get("error"), str)
        self.assertTrue(response["error"].strip())

    def test_runtime_capability_allows_suppression_and_processed_tail_for_any_engine(self):
        runtime = PreviewSuppressibleRuntime(
            steps=[("", "", None), ("", "", None)], final=("完整文本", "zh")
        )
        server = self._server(runtime)
        audio = np.asarray([1, 2, 3], dtype=np.float32) / 32768.0
        processor = types.SimpleNamespace(
            reset=mock.Mock(),
            feed=mock.Mock(side_effect=lambda pcm: pcm[:2].copy()),
            flush=mock.Mock(return_value=audio[2:].copy()),
        )
        server.stream.audio_processor = processor
        self._assert_response(server.handle_stream_command({
            "action": "stream_start", "session_id": 1,
        }), 1, "", "", 0, False, language=None)

        response = server.handle_stream_command({
            "action": "stream_feed", "session_id": 1, "offset": 0,
            "sample_rate": 16000, "audio_format": "pcm_s16le",
            "audio_base64": self._pcm16([1, 2, 3]), "preview": False,
        })

        self._assert_response(response, 1, "", "", 3, False, language=None)
        self._assert_response(server.handle_stream_command({
            "action": "stream_finish", "session_id": 1,
        }), 1, "完整文本", "", 3, True)
        self.assertEqual(runtime.preview_flags, [False, False])
        np.testing.assert_array_equal(np.concatenate(runtime.feed_inputs), audio)
        self.assertEqual(processor.flush.call_count, 1)
        self.assertEqual(runtime.finish_calls, 1)
        self.assertFalse(server.stream.active)

    def test_unsupported_suppression_rejects_before_decoding_or_consuming_audio(self):
        for capability in ("missing", False, None, 1, "true"):
            with self.subTest(capability=capability):
                runtime = FakeStreamRuntime(steps=[("好", "好的", "zh")])
                if capability != "missing":
                    runtime.supports_preview_suppression = capability
                server = self._server(runtime)
                processor = types.SimpleNamespace(
                    reset=mock.Mock(), feed=mock.Mock(side_effect=lambda pcm: pcm.copy())
                )
                server.stream.audio_processor = processor
                server.stream.start(1)
                command = dict(
                    action="stream_feed", session_id=1, offset=0,
                    sample_rate=16000, audio_format="pcm_s16le",
                    audio_base64=self._pcm16([1, 2, 3]), preview=False,
                )
                with mock.patch.object(
                    server, "_decode_stream_audio", wraps=server._decode_stream_audio
                ) as decode:
                    self._assert_error(server.handle_stream_command(command))
                decode.assert_not_called()
                processor.feed.assert_not_called()
                self.assertEqual(runtime.feed_inputs, [])
                self.assertEqual(runtime.reset_calls, 0)
                self.assertTrue(server.stream.active)
                self._assert_response(self._feed(server, 1, 0, [1, 2, 3]),
                                      1, "好", "的", 3, False)

    def test_pcm16_preview_revisions_and_partial_tail_finish(self):
        runtime = FakeStreamRuntime(
            steps=[("", "今天是周三", "zh"), ("", "今天是周四", None),
                   ("今天", "今天是周四", "")],
            final=("今天是周四。", None),
        )
        server = self._server(runtime)
        self._assert_response(server.handle_stream_command({
            "action": "stream_start", "session_id": 1,
        }), 1, "", "", 0, False, language=None)
        chunks = [
            np.resize(np.asarray([-32768, -16384, 0, 16384, 32767], dtype="<i2"), 5120),
            np.resize(np.asarray([32767, 16384, 0, -16384, -32768], dtype="<i2"), 5120),
            np.arange(-6, 7, dtype="<i2"),
        ]
        expected = [("", "今天是周三"), ("", "今天是周四"), ("今天", "是周四")]
        offset = 0
        committed = ""
        for chunk, (text, tentative) in zip(chunks, expected):
            response = self._feed(server, 1, offset, chunk)
            offset += len(chunk)
            self._assert_response(response, 1, text, tentative, offset, False)
            self.assertTrue(response["text"].startswith(committed))
            committed = response["text"]
        self._assert_response(server.handle_stream_command({
            "action": "stream_finish", "session_id": 1,
        }), 1, "今天是周四。", "", 10253, True)
        self.assertFalse(server.stream.active)
        self.assertEqual(runtime.finish_calls, 1)
        self.assertEqual(len(runtime.feed_inputs), len(chunks))
        for actual, original in zip(runtime.feed_inputs, chunks):
            self.assertEqual(actual.dtype, np.dtype(np.float32))
            self.assertTrue(actual.flags.c_contiguous)
            np.testing.assert_array_equal(actual, original.astype(np.float32) / 32768.0)
        self._assert_error(server.handle_stream_command({
            "action": "stream_finish", "session_id": 1,
        }))
        self.assertEqual(runtime.finish_calls, 1)

    def test_context_hotwords_normalize_unicode_and_enforce_100_by_80_limit(self):
        runtime = FakeStreamRuntime()
        server = self._server(runtime)
        words = [" \t\u00a0\u2003\n", " \tAlpha\u00a0\u2003Beta\n ",
                 "alpha beta", "ALPHA BETA", " " + "中" * 80 + " ",
                 "文" * 81, "🙂" * 80, "🚀" * 81,
                 "界" * 39 + "\t\u2003\n" + "界" * 40]
        numbered = [f"term-{index}" for index in range(101)]
        words.extend(numbered)
        expected = ["Alpha Beta", "中" * 80, "🙂" * 80,
                    "界" * 39 + " " + "界" * 40] + numbered[:96]
        self._assert_response(server.handle_stream_command({
            "action": "stream_start", "session_id": 2, "context": "  base\n ",
            "hot_words": words, "language": " zh ",
        }), 2, "", "", 0, False)
        self.assertEqual(runtime.start_calls, [
            ("base\nHotwords: " + ", ".join(expected), "zh")
        ])

    def test_invalid_starts_preserve_active_session_and_available_next_id(self):
        runtime = FakeStreamRuntime(steps=[("好", "好的", "zh")], final=("好的", "zh"))
        server = self._server(runtime)
        self._assert_response(server.handle_stream_command({
            "action": "stream_start", "session_id": 3, "language": "zh",
        }), 3, "", "", 0, False)
        invalid = [
            {"session_id": value} for value in (0, -1, True, 4.0, "4", None, 3)
        ] + [
            {"context": None}, {"language": 7}, {"hot_words": None},
            {"hot_words": "word"}, {"hot_words": ("word",)},
            {"hot_words": [f"word-{index}" for index in range(100)] + [None]},
        ]
        for fields in invalid:
            with self.subTest(fields=fields):
                self._assert_error(server.handle_stream_command({
                    "action": "stream_start", "session_id": 4, **fields,
                }))
        self.assertEqual(runtime.start_calls, [("", "zh")])
        self.assertEqual(runtime.reset_calls, 0)
        self._assert_response(self._feed(server, 3, 0, [1, 2, 3]),
                              3, "好", "的", 3, False)
        self._assert_response(server.handle_stream_command({
            "action": "stream_finish", "session_id": 3,
        }), 3, "好的", "", 3, True)
        self._assert_response(server.handle_stream_command({
            "action": "stream_start", "session_id": 4, "language": " \t ",
        }), 4, "", "", 0, False, language=None)

    def test_invalid_pcm_rate_offset_and_session_do_not_consume_audio(self):
        runtime = FakeStreamRuntime(
            steps=[("好", "好的", "zh"), ("好", "好的。", None)],
            final=("好的。", None),
        )
        server = self._server(runtime)
        self._assert_response(server.handle_stream_command({
            "action": "stream_start", "session_id": 5,
        }), 5, "", "", 0, False, language=None)
        self._assert_response(self._feed(server, 5, 0, [1, 2, 3]),
                              5, "好", "的", 3, False)
        command = dict(action="stream_feed", session_id=5, offset=3,
                       sample_rate=16000, audio_format="pcm_s16le",
                       audio_base64=self._pcm16([4]))
        invalid = [
            {"sample_rate": value} for value in (8000, "16000", None)
        ] + [
            {"offset": value} for value in (-1, True, 3.0, "3", 2, 4)
        ] + [
            {"session_id": value} for value in (4, 6, True, 5.0, "5")
        ] + [
            {"audio_base64": value} for value in (
                "%%%", "", None, 1, base64.b64encode(b"\x00").decode("ascii")
            )
        ] + [{"audio_format": "wav"}, {"action": "unknown_action"}]
        for fields in invalid:
            with self.subTest(fields=fields):
                self._assert_error(server.handle_stream_command({**command, **fields}))
        self.assertTrue(server.stream.active)
        self.assertEqual(len(runtime.feed_inputs), 1)
        self.assertEqual(runtime.start_calls, [("", None)])
        self.assertEqual(runtime.reset_calls, 0)
        self.assertEqual(runtime.finish_calls, 0)
        self._assert_response(self._feed(server, 5, 3, [4]), 5, "好", "的。", 4, False)
        self._assert_response(server.handle_stream_command({
            "action": "stream_finish", "session_id": 5,
        }), 5, "好的。", "", 4, True)

    def test_cancel_restart_rejects_stale_commands_without_changing_new_stream(self):
        runtime = FakeStreamRuntime(
            steps=[("旧", "旧草稿", "zh"), ("新", "新内容", "zh")],
            final=("新内容。", "zh"),
        )
        server = self._server(runtime)
        self._assert_response(server.handle_stream_command({
            "action": "stream_start", "session_id": 6,
        }), 6, "", "", 0, False, language=None)
        self._assert_response(self._feed(server, 6, 0, [1, 2]), 6, "旧", "草稿", 2, False)
        self._assert_response(server.handle_stream_command({
            "action": "stream_cancel", "session_id": 6,
        }), 6, "", "", 2, True)
        self.assertEqual(runtime.reset_calls, 1)
        self.assertEqual(runtime.finish_calls, 0)
        self._assert_response(server.handle_stream_command({
            "action": "stream_start", "session_id": 7,
        }), 7, "", "", 0, False, language=None)
        for action in ("stream_start", "stream_finish", "stream_cancel"):
            self._assert_error(server.handle_stream_command({"action": action, "session_id": 6}))
        self._assert_error(self._feed(server, 6, 2, [3]))
        self.assertEqual(runtime.reset_calls, 1)
        self.assertEqual(len(runtime.start_calls), 2)
        self.assertEqual(len(runtime.feed_inputs), 1)
        self._assert_response(self._feed(server, 7, 0, [4]), 7, "新", "内容", 1, False)
        self._assert_response(server.handle_stream_command({
            "action": "stream_finish", "session_id": 7,
        }), 7, "新内容。", "", 1, True)

    def test_gpu_idle_activity_tracks_real_session_lifetime(self):
        server = self._server(FakeStreamRuntime())
        self.assertFalse(server._gpu_idle_stream_active())
        server.stream.start(8)
        self.assertTrue(server._gpu_idle_stream_active())
        server.stream.cancel(8)
        self.assertFalse(server._gpu_idle_stream_active())

    def test_gpu_suspend_keeps_active_runtime_vad_and_accepted_audio(self):
        runtime = FakeStreamRuntime(
            steps=[("保留", "保留尾", "zh"), ("保留", "保留尾音", None)],
            final=("保留尾音", "zh"),
        )
        server = self._server(runtime)
        model, offline, vad = server.model, server.session, server.vad_model
        # Start through the existing session API to isolate the unload guard
        # from a missing server stream-command implementation.
        server.stream.start(9)
        server.stream.feed(9, np.asarray([0.1, 0.2], dtype=np.float32), offset=0)
        self.assertFalse(server._suspend_gpu_runtime())
        self.assertTrue(server.initialized)
        self.assertFalse(server._gpu_suspended)
        self.assertIs(server.model, model)
        self.assertIs(server.session, offline)
        self.assertIs(server.vad_model, vad)
        self.assertEqual((runtime.close_calls, model.close_calls, offline.close_calls), (0, 0, 0))
        self.assertEqual(runtime.reset_calls, 0)
        self.assertTrue(server.stream.active)
        response = server.stream.feed(9, np.asarray([0.3], dtype=np.float32), offset=2)
        self.assertEqual(response["sample_count"], 3)
        self.assertEqual(server.stream.finish(9)["text"], "保留尾音")

    def test_offline_transcription_is_busy_without_touching_active_runtime(self):
        runtime = FakeStreamRuntime(
            steps=[("保留", "保留尾", "zh"), ("保留", "保留尾音", None)],
            final=("保留尾音", "zh"),
        )
        server = self._server(runtime)
        stream = server.stream
        stream.start(10)
        stream.feed(10, np.asarray([0.1, 0.2], dtype=np.float32), offset=0)
        result = server.transcribe_audio(
            None, audio_base64=self._pcm16(np.ones(8000, dtype="<i2")),
            audio_format="pcm_s16le", sample_rate=16000,
        )
        self.assertIs(result.get("success"), False, result)
        self.assertEqual(result.get("type"), "stream_busy", result)
        self.assertEqual(result.get("engine"), self.engine)
        self.assertEqual(server.session.run_calls, 0)
        self.assertIs(server.stream, stream)
        self.assertTrue(stream.active)
        self.assertEqual(len(runtime.start_calls), 1)
        self.assertEqual(len(runtime.feed_inputs), 1)
        self.assertEqual((runtime.finish_calls, runtime.reset_calls, runtime.close_calls), (0, 0, 0))
        response = stream.feed(10, np.asarray([0.3], dtype=np.float32), offset=2)
        self.assertEqual(response["sample_count"], 3)
        self.assertEqual(stream.finish(10)["text"], "保留尾音")


class Qwen3ASRSharedStreamTests(SharedASRStreamContract, unittest.TestCase):
    engine = "qwen3-asr-0.6b"
    server_type = qwen3_asr_server.Qwen3ASRServer
    server_options = {"engine": engine}


class R2T2ASRSharedStreamTests(SharedASRStreamContract, unittest.TestCase):
    engine = "confucius4-r2t2"
    server_type = r2t2_asr_server.R2T2ASRServer
    server_options = {}


if __name__ == "__main__":
    unittest.main()
