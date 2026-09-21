import base64
import contextlib
import io
import json
import logging
import os
import sys
import unittest
from unittest import mock

import numpy as np

sys.path.insert(0, os.path.dirname(__file__))

import r2t2_asr_server
from r2t2_stream import R2T2StreamSession


class FakeStreamRuntime:
    def __init__(self, feed_results=(), finish_result=("", None), chunk_samples=4):
        self.start_calls = []
        self.feed_inputs = []
        self.finish_calls = 0
        self.reset_calls = 0
        self.feed_results = list(feed_results)
        self.finish_result = finish_result
        self.chunk_samples = chunk_samples

    def start(self, context="", language=None):
        self.start_calls.append((context, language))

    def feed(self, pcm):
        self.feed_inputs.append(np.array(pcm, copy=True))
        return self.feed_results.pop(0)

    def finish(self):
        self.finish_calls += 1
        return self.finish_result

    def reset(self):
        self.reset_calls += 1


class RecordingOfflineSegmented:
    instances = []
    error_to_raise = None

    def __init__(self, *args, **kwargs):
        self.native = args[0] if args else kwargs.get("native")
        self.vad = args[1] if len(args) > 1 else kwargs.get("vad")
        self.start_calls = []
        self.feed_inputs = []
        self.finish_calls = 0
        self.reset_calls = 0
        self.error_to_raise = self.__class__.error_to_raise
        self.__class__.instances.append(self)

    def start(self, context="", language=None):
        self.start_calls.append((context, language))

    def feed(self, pcm):
        self.feed_inputs.append(np.array(pcm, copy=True))
        if self.error_to_raise is not None:
            raise self.error_to_raise
        return "offline", "en"

    def finish(self):
        self.finish_calls += 1
        return "offline", "en"

    def reset(self):
        self.reset_calls += 1


class R2T2ASRServerAdapterTests(unittest.TestCase):
    @staticmethod
    def _server_shell(runtime=None, *, initialized=True):
        server = object.__new__(r2t2_asr_server.R2T2ASRServer)
        server.engine = "confucius4-r2t2"
        server.backend = "cpu"
        server.device = "cpu"
        server.initialized = initialized
        server.native = runtime
        server.vad_model = object() if runtime is not None else None
        server.segmented = object() if runtime is not None else None
        server.stream = R2T2StreamSession(runtime) if runtime is not None else None
        server.logger = logging.getLogger("test_r2t2_asr_server")
        server.stdout_suppressor = mock.Mock()
        server.stdout_suppressor.suppress.side_effect = contextlib.nullcontext
        server._last_load_error = None
        server.transcription_count = 0
        server.total_audio_duration = 0.0
        server._total_inference_ms = 0.0
        return server

    @staticmethod
    def _pcm16(values):
        return base64.b64encode(np.asarray(values, dtype="<i2").tobytes()).decode(
            "ascii"
        )

    def _assert_success(self, response, *, session_id, text, language, sample_count, final):
        self.assertTrue(response["success"])
        self.assertEqual(response["engine"], "confucius4-r2t2")
        self.assertEqual(response["backend"], "cpu")
        self.assertEqual(
            {key: response[key] for key in ("session_id", "text", "language", "sample_count", "final")},
            {
                "session_id": session_id,
                "text": text,
                "language": language,
                "sample_count": sample_count,
                "final": final,
            },
        )

    def _assert_stream_error(self, response):
        self.assertFalse(response["success"])
        self.assertEqual(response.get("type"), "stream_error")
        self.assertIsInstance(response.get("error"), str)
        self.assertTrue(response["error"].strip())

    def test_stream_commands_decode_pcm16_and_preserve_context_language_and_cancel(self):
        runtime = FakeStreamRuntime(
            feed_results=[("hello", "zh"), ("hello!", None), ("discard", "de")],
            finish_result=("hello!", None),
        )
        server = self._server_shell(runtime)
        status = server.check_status()
        self.assertTrue(status["initialized"])
        self.assertTrue(status["native_streaming"])
        self.assertTrue(status["inline_audio"])
        self.assertTrue(status["model_loaded"])

        start = server.handle_stream_command(
            {
                "action": "stream_start",
                "session_id": 1,
                "context": "base",
                "hot_words": [" foo ", "", "bar"],
                "language": "   ",
            }
        )
        self._assert_success(
            start,
            session_id=1,
            text="",
            language=None,
            sample_count=0,
            final=False,
        )
        self.assertEqual(runtime.start_calls, [("base\nHotwords: foo, bar", None)])

        first = server.handle_stream_command(
            {
                "action": "stream_feed",
                "session_id": 1,
                "offset": 0,
                "sample_rate": 16_000,
                "audio_format": "pcm_s16le",
                "audio_base64": self._pcm16([0, 16_384]),
            }
        )
        self._assert_success(
            first,
            session_id=1,
            text="hello",
            language="zh",
            sample_count=2,
            final=False,
        )
        second = server.handle_stream_command(
            {
                "action": "stream_feed",
                "session_id": 1,
                "offset": 2,
                "sample_rate": 16_000,
                "audio_format": "pcm_s16le",
                "audio_base64": self._pcm16([32_767]),
            }
        )
        self._assert_success(
            second,
            session_id=1,
            text="hello!",
            language="zh",
            sample_count=3,
            final=False,
        )
        final = server.handle_stream_command(
            {"action": "stream_finish", "session_id": 1}
        )
        self._assert_success(
            final,
            session_id=1,
            text="hello!",
            language="zh",
            sample_count=3,
            final=True,
        )

        server.handle_stream_command(
            {
                "action": "stream_start",
                "session_id": 2,
                "context": "",
                "hot_words": [],
                "language": "",
            }
        )
        cancelled_feed = server.handle_stream_command(
            {
                "action": "stream_feed",
                "session_id": 2,
                "offset": 0,
                "sample_rate": 16_000,
                "audio_format": "pcm_s16le",
                "audio_base64": self._pcm16([100]),
            }
        )
        self._assert_success(
            cancelled_feed,
            session_id=2,
            text="discard",
            language="de",
            sample_count=1,
            final=False,
        )
        cancelled = server.handle_stream_command(
            {"action": "stream_cancel", "session_id": 2}
        )
        self._assert_success(
            cancelled,
            session_id=2,
            text="",
            language="de",
            sample_count=1,
            final=True,
        )
        self.assertEqual(
            runtime.start_calls,
            [("base\nHotwords: foo, bar", None), ("", None)],
        )

        np.testing.assert_allclose(
            runtime.feed_inputs[0], np.asarray([0.0, 0.5], dtype=np.float32)
        )
        np.testing.assert_allclose(
            runtime.feed_inputs[1], np.asarray([32_767 / 32_768], dtype=np.float32)
        )
        self.assertEqual(runtime.feed_inputs[0].dtype, np.dtype(np.float32))

    def test_malformed_stream_inputs_return_errors_without_mutating_active_stream(self):
        runtime = FakeStreamRuntime(feed_results=[("ok", "en")])
        server = self._server_shell(runtime)
        server.handle_stream_command(
            {"action": "stream_start", "session_id": 1, "language": "en"}
        )
        starts_before = list(runtime.start_calls)
        resets_before = runtime.reset_calls
        valid_audio = self._pcm16([1, 2])

        invalid_commands = [
            {
                "action": "stream_feed",
                "session_id": 1,
                "offset": 0,
                "sample_rate": 8_000,
                "audio_format": "pcm_s16le",
                "audio_base64": valid_audio,
            },
            {
                "action": "stream_feed",
                "session_id": 1,
                "offset": 0,
                "sample_rate": 16_000,
                "audio_format": "wav",
                "audio_base64": valid_audio,
            },
            {
                "action": "stream_feed",
                "session_id": 1,
                "offset": 0,
                "sample_rate": 16_000,
                "audio_format": "pcm_s16le",
                "audio_base64": "%%%",
            },
            {
                "action": "stream_feed",
                "session_id": 1,
                "offset": 0,
                "sample_rate": 16_000,
                "audio_format": "pcm_s16le",
                "audio_base64": base64.b64encode(b"\x00").decode("ascii"),
            },
            {
                "action": "stream_feed",
                "session_id": 1,
                "offset": 0,
                "sample_rate": 16_000,
                "audio_format": "pcm_s16le",
                "audio_base64": "",
            },
            {
                "action": "stream_feed",
                "session_id": 1,
                "offset": 0,
                "sample_rate": 16_000,
                "audio_format": "pcm_s16le",
                "audio_base64": float("nan"),
            },
            {
                "action": "stream_start",
                "session_id": 2,
                "context": None,
                "hot_words": [],
                "language": "en",
            },
            {
                "action": "stream_start",
                "session_id": 2,
                "context": "",
                "hot_words": [1],
                "language": "en",
            },
            {
                "action": "stream_start",
                "session_id": 2,
                "context": "",
                "hot_words": [],
                "language": 7,
            },
            {"action": "unknown_action", "session_id": 1},
        ]
        for command in invalid_commands:
            self._assert_stream_error(server.handle_stream_command(command))

        self.assertEqual(runtime.start_calls, starts_before)
        self.assertEqual(runtime.reset_calls, resets_before)
        self.assertEqual(runtime.feed_inputs, [])
        valid = server.handle_stream_command(
            {
                "action": "stream_feed",
                "session_id": 1,
                "offset": 0,
                "sample_rate": 16_000,
                "audio_format": "pcm_s16le",
                "audio_base64": valid_audio,
            }
        )
        self._assert_success(
            valid,
            session_id=1,
            text="ok",
            language="en",
            sample_count=2,
            final=False,
        )
        self.assertEqual(len(runtime.feed_inputs), 1)

    def test_initialize_missing_model_returns_models_not_downloaded(self):
        server = self._server_shell(None, initialized=False)

        with mock.patch.object(server, "_resolve_model_path", return_value=None):
            result = server.initialize()

        self.assertFalse(result["success"])
        self.assertEqual(result["type"], "models_not_downloaded")
        self.assertEqual(result["engine"], "confucius4-r2t2")
        self.assertFalse(server.initialized)
        self.assertIsNone(server.native)
        self.assertIsNone(server.vad_model)
        self.assertIsNone(server.segmented)
        self.assertIsNone(server.stream)

        status = server.check_status()
        self.assertFalse(status["initialized"])
        self.assertFalse(status["native_streaming"])
        self.assertFalse(status["inline_audio"])
        self.assertFalse(status["model_loaded"])

        with mock.patch.object(server, "initialize", return_value=result) as initialize:
            passthrough = server.handle_stream_command(
                {"action": "stream_start", "session_id": 1}
            )
        self.assertEqual(passthrough, result)
        initialize.assert_called_once()
        self.assertIsNone(server.native)
        self.assertIsNone(server.stream)

    def test_stream_busy_blocks_offline_transcription_without_touching_stream(self):
        runtime = FakeStreamRuntime()
        server = self._server_shell(runtime)
        server.handle_stream_command(
            {"action": "stream_start", "session_id": 1, "language": "en"}
        )
        resets_before = runtime.reset_calls

        with mock.patch.object(server, "_load_audio") as load_audio:
            result = server.transcribe_audio(None, options={})

        self.assertFalse(result["success"])
        self.assertEqual(result["type"], "stream_busy")
        load_audio.assert_not_called()
        self.assertEqual(runtime.feed_inputs, [])
        self.assertEqual(runtime.reset_calls, resets_before)

    def test_offline_transcription_feeds_true_tail_once_and_finishes_once(self):
        RecordingOfflineSegmented.instances.clear()
        runtime = FakeStreamRuntime()
        server = self._server_shell(runtime)
        audio = np.arange(10, dtype=np.float32)

        with (
            mock.patch.object(r2t2_asr_server, "SegmentedR2T2", RecordingOfflineSegmented),
            mock.patch.object(
                server, "_load_audio", return_value=(audio, 1.0, "memory")
            ),
        ):
            server.handle_stream_command(
                {"action": "stream_start", "session_id": 5, "language": "en"}
            )
            server.handle_stream_command(
                {"action": "stream_cancel", "session_id": 5}
            )
            result = server.transcribe_audio(
                None,
                options={
                    "context": "offline",
                    "hot_words": [" foo ", "", "bar"],
                    "language": "en",
                },
                audio_base64="ignored-by-loader",
                audio_format="pcm_s16le",
                sample_rate=16_000,
            )

        self.assertTrue(result["success"])
        self.assertEqual(
            {key: result[key] for key in ("text", "raw_text", "language", "duration", "engine", "backend", "input_mode")},
            {
                "text": "offline",
                "raw_text": "offline",
                "language": "en",
                "duration": 1.0,
                "engine": "confucius4-r2t2",
                "backend": "cpu",
                "input_mode": "memory",
            },
        )
        self.assertGreaterEqual(result["inference_ms"], 0.0)
        self.assertEqual(len(RecordingOfflineSegmented.instances), 1)
        segmented = RecordingOfflineSegmented.instances[0]
        self.assertIs(segmented.native, runtime)
        self.assertEqual(
            segmented.start_calls,
            [("offline\nHotwords: foo, bar", "en")],
        )
        self.assertEqual(segmented.finish_calls, 1)
        self.assertEqual(len(segmented.feed_inputs), 3)
        np.testing.assert_array_equal(segmented.feed_inputs[0], audio[:4])
        np.testing.assert_array_equal(segmented.feed_inputs[1], audio[4:8])
        np.testing.assert_array_equal(segmented.feed_inputs[2], audio[8:])
        for chunk in segmented.feed_inputs:
            self.assertEqual(chunk.dtype, np.dtype(np.float32))
        self.assertFalse(server.handle_stream_command(
            {"action": "stream_start", "session_id": 5}
        )["success"])
        follow_up = server.handle_stream_command(
            {"action": "stream_start", "session_id": 6, "language": "en"}
        )
        self.assertTrue(follow_up["success"])

    def test_load_runtime_falls_back_to_cpu_and_partial_initialize_closes_native(self):
        class FallbackNative:
            attempts = []

            def __init__(self, *args, **kwargs):
                backend = kwargs["backend"]
                self.__class__.attempts.append(backend)
                if backend == "cuda":
                    raise OSError("cuda unavailable")
                self.backend = backend
                self.chunk_samples = 4
                self.close_calls = 0

            def close(self):
                self.close_calls += 1

        server = self._server_shell(None, initialized=False)
        with mock.patch.object(r2t2_asr_server, "NativeRuntime", FallbackNative):
            server._load_runtime("model.gguf")

        self.assertEqual(FallbackNative.attempts, ["cuda", "cpu"])
        self.assertIsInstance(server.native, FallbackNative)
        self.assertEqual(server.backend, "cpu")
        self.assertEqual(server.device, "cpu")

        partial = mock.Mock()
        failed = self._server_shell(None, initialized=False)

        def install_partial_runtime(_model_path):
            failed.native = partial
            failed.backend = "cpu"
            failed.device = "cpu"

        failed._resolve_model_path = mock.Mock(return_value="model.gguf")
        failed._load_runtime = install_partial_runtime
        with mock.patch.object(
            r2t2_asr_server,
            "FireRedVad",
            side_effect=RuntimeError("vad unavailable"),
        ):
            result = failed.initialize()

        self.assertFalse(result["success"])
        self.assertEqual(result["type"], "init_error")
        partial.close.assert_called_once()
        self.assertFalse(failed.initialized)
        self.assertIsNone(failed.native)
        self.assertIsNone(failed.vad_model)
        self.assertIsNone(failed.segmented)
        self.assertIsNone(failed.stream)
        failed_status = failed.check_status()
        self.assertFalse(failed_status["initialized"])
        self.assertFalse(failed_status["native_streaming"])
        self.assertFalse(failed_status["inline_audio"])
        self.assertFalse(failed_status["model_loaded"])

    def test_offline_native_error_is_sanitized_and_resets_segmented(self):
        RecordingOfflineSegmented.instances.clear()
        sentinel = "SENTINEL_TRANSCRIPT encoded-audio-secret"
        RecordingOfflineSegmented.error_to_raise = RuntimeError(sentinel)
        runtime = FakeStreamRuntime()
        server = self._server_shell(runtime)
        log_output = io.StringIO()
        handler = logging.StreamHandler(log_output)
        server.logger.addHandler(handler)
        server.logger.setLevel(logging.DEBUG)

        try:
            with (
                mock.patch.object(
                    r2t2_asr_server, "SegmentedR2T2", RecordingOfflineSegmented
                ),
                mock.patch.object(
                    server,
                    "_load_audio",
                    return_value=(np.arange(4, dtype=np.float32), 1.0, "memory"),
                ),
                mock.patch.object(
                    r2t2_asr_server, "logger", server.logger, create=True
                ),
            ):
                result = server.transcribe_audio(
                    None,
                    options={"context": "offline", "language": "en"},
                    audio_base64="ignored-by-loader",
                    audio_format="pcm_s16le",
                    sample_rate=16_000,
                )
        finally:
            RecordingOfflineSegmented.error_to_raise = None
            server.logger.removeHandler(handler)

        self.assertFalse(result["success"])
        self.assertIsInstance(result.get("error"), str)
        self.assertNotIn(sentinel, json.dumps(result, ensure_ascii=False))
        self.assertNotIn(sentinel, log_output.getvalue())
        self.assertEqual(len(RecordingOfflineSegmented.instances), 1)
        self.assertEqual(RecordingOfflineSegmented.instances[0].reset_calls, 1)


if __name__ == "__main__":
    unittest.main()
