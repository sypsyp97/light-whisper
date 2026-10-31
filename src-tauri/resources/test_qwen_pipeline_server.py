"""Real Qwen server activation and shutdown with injected native/VAD devices."""

import contextlib
import threading
from types import SimpleNamespace
import unittest
from unittest import mock

import numpy as np

import qwen3_asr_server
from qwen3_native import QwenPipelineRuntime, QwenPreviewRuntime
from r2t2_stream import R2T2StreamSession
from test_qwen_pipeline import TIMEOUT, _GatedTranscribe, _StreamCaller
from test_qwen_preview_backlog import feed_command


class _ServerNative:
    def __init__(self):
        self.decode = _GatedTranscribe(outputs=(("draft", "Chinese"), ("complete", None)))
        self.threads = set()
        self.alive_at_close = []
        self.preparer_threads = set()
        self.preparers_alive_at_close = []
        self.close = mock.Mock(side_effect=self._close)

    def run(self, audio, **options):
        self.threads.add(threading.current_thread())
        return self.decode(audio, **options)

    def _close(self):
        self.alive_at_close.append([thread.is_alive() for thread in self.threads])
        self.preparers_alive_at_close.append([thread.is_alive() for thread in self.preparer_threads])


class _ServerVad:
    def __init__(self):
        self.calls = []
        self.silence = False
        self.full_prepared = threading.Event()
        self.threads = set()
        self.condition = threading.Condition()

    def speech_timestamps(self, audio):
        with self.condition:
            self.calls.append((audio.copy(), threading.get_ident()))
            self.threads.add(threading.current_thread())
            self.condition.notify_all()
        if len(audio) == 12813:
            self.full_prepared.set()
        return [] if self.silence else [{"start": 0, "end": len(audio)}]

    def wait_calls(self, count):
        with self.condition:
            if not self.condition.wait_for(lambda: len(self.calls) >= count, TIMEOUT):
                raise AssertionError(f"VAD preparation {count} never entered")


class QwenPipelineServerTests(unittest.TestCase):
    def setUp(self):
        server_type = qwen3_asr_server.Qwen3ASRServer
        self.enterContext(mock.patch("signal.signal"))
        self.enterContext(mock.patch.object(qwen3_asr_server, "logger"))
        self.enterContext(mock.patch.object(server_type, "_setup_runtime_environment"))
        self.enterContext(mock.patch.object(server_type, "_detect_device", return_value="cuda"))
        self.enterContext(mock.patch.object(server_type, "_get_gpu_device_info", return_value={"device": "test"}))
        self.enterContext(mock.patch.object(server_type, "_resolve_model_path", return_value="test-qwen.gguf"))
        self.enterContext(mock.patch.object(server_type, "_warmup_inference"))
        self.enterContext(mock.patch.object(server_type, "_load_audio_processor"))
        self.native_factory = self.enterContext(mock.patch.object(qwen3_asr_server, "QwenNativeRuntime"))
        self.vad_factory = self.enterContext(mock.patch.object(qwen3_asr_server, "FireRedVad"))

    def initialized_server(self, *, fallback=False, attach_pipeline=False):
        caller = _StreamCaller()
        native, vad = _ServerNative(), _ServerVad()
        native.preparer_threads = vad.threads
        self.native_factory.reset_mock(side_effect=True)
        self.native_factory.side_effect = [RuntimeError("CUDA unavailable"), native] if fallback else None
        self.native_factory.return_value = native
        self.vad_factory.return_value = vad
        server = caller.submit(lambda: qwen3_asr_server.Qwen3ASRServer(engine="qwen3-asr-0.6b")).result()
        server.stdout_suppressor = SimpleNamespace(suppress=contextlib.nullcontext)
        runtimes = []

        def cleanup():
            native.decode.unblock()
            try:
                for runtime in runtimes:
                    if isinstance(runtime, QwenPipelineRuntime):
                        caller.submit(runtime.close).result()
                try:
                    caller.submit(server._close_runtime).result()
                except RuntimeError:
                    # Deliberately injected cleanup errors are asserted in the test.
                    pass
            finally:
                caller.close()

        self.addCleanup(cleanup)
        initialized = caller.submit(server.initialize).result()
        self.assertTrue(initialized["success"], initialized)
        runtimes.append(server.stream.runtime)
        if attach_pipeline:
            # Isolate unload from selection: activation has its own behavioral test.
            runtime = caller.submit(lambda: QwenPipelineRuntime(
                server._run_inference, prepare_audio=lambda audio: server._filter_speech(audio)[0]
            )).result()
            server.stream = R2T2StreamSession(runtime)
            runtimes.append(runtime)
        return caller, server, native, vad

    def command(self, caller, server, command):
        response = caller.submit(lambda: server.handle_stream_command(command)).result()
        self.assertTrue(response["success"], response)
        return response

    def start(self, caller, server, session_id=1):
        return self.command(caller, server, {
            "action": "stream_start", "session_id": session_id,
            "context": " mixed 术语 ", "language": " zh ",
        })

    def test_initialize_cuda_acks_during_native_and_overlaps_cpu_vad_with_full_final(self):
        caller, server, native, vad = self.initialized_server()
        self.start(caller, server)
        pcm16 = (np.arange(12813, dtype=np.int32) * 3 - 20000).astype("<i2")
        pcm = pcm16.astype(np.float32) / np.float32(32768)
        first = caller.submit(lambda: server.handle_stream_command(feed_command(pcm16[:3200])))
        self.assertTrue(native.decode.entered[0].wait(TIMEOUT))
        first_response = first.result()
        self.assertTrue(first_response["success"], first_response)
        self.assertEqual(first_response["sample_count"], 3200)
        self.assertFalse(native.decode.release[1].is_set(), "stream ACK cannot wait for native decode")
        self.assertIsInstance(server.stream.runtime, QwenPipelineRuntime)
        self.command(caller, server, feed_command(pcm16[3200:12800], offset=3200))
        vad.wait_calls(2)
        self.command(caller, server, feed_command(pcm16[12800:], offset=12800, preview=False))
        self.assertEqual(len(vad.calls), 2, "next CPU VAD must finish while native is blocked")
        final = caller.submit(lambda: server.handle_stream_command({"action": "stream_finish", "session_id": 1}))
        self.assertTrue(vad.full_prepared.wait(TIMEOUT))
        self.assertEqual(len(native.decode.calls), 1)
        native.decode.release[1].set()
        response = final.result()
        self.assertTrue(response["success"], response)
        self.assertEqual((response["text"], response["language"], response["sample_count"]),
                         ("complete", "Chinese", 12813))
        self.assertEqual(len(native.decode.calls), 2)
        np.testing.assert_array_equal(vad.calls[-1][0], pcm)
        np.testing.assert_array_equal(native.decode.calls[-1][0], pcm)
        self.assertEqual(len(vad.threads), 1)
        self.assertNotIn(caller.thread, vad.threads)
        self.assertTrue(vad.threads.isdisjoint(native.threads))
        self.assertEqual(len(native.threads), 1)
        self.assertNotIn(caller.thread, native.threads)
        self.assertEqual(native.decode.max_active, 1)
        for _, options, _ in native.decode.calls:
            self.assertEqual(options, {"timestamps": "none", "context": "mixed 术语", "language": "zh"})

    def test_cuda_load_failure_keeps_cpu_synchronous_and_silence_skips_native(self):
        caller, server, native, vad = self.initialized_server(fallback=True)
        self.assertEqual([call.kwargs["backend"] for call in self.native_factory.call_args_list], ["cuda", "cpu"])
        self.assertEqual(server.backend, "cpu")
        self.assertIs(type(server.stream.runtime), QwenPreviewRuntime)
        self.start(caller, server)
        pcm = np.full(1920, 1000, dtype="<i2")
        first = caller.submit(lambda: server.handle_stream_command(feed_command(pcm)))
        self.assertTrue(native.decode.entered[0].wait(TIMEOUT))
        self.assertFalse(first.done.is_set())
        self.assertEqual(native.threads, {caller.thread})
        native.decode.release[1].set()
        self.assertTrue(first.result()["success"])
        self.command(caller, server, {"action": "stream_finish", "session_id": 1})
        vad.silence = True
        self.start(caller, server, 2)
        silent = self.command(caller, server, feed_command(pcm, session_id=2))
        self.assertEqual(silent["tentative_text"], "")
        final = self.command(caller, server, {"action": "stream_finish", "session_id": 2})
        self.assertEqual((final["text"], final["sample_count"]), ("", 1920))
        self.assertEqual(len(native.decode.calls), 1)
        self.assertEqual({thread_id for _, thread_id in vad.calls}, {caller.thread.ident})

    def test_inactive_stream_unload_joins_pipeline_before_closing_native_once(self):
        caller, server, native, vad = self.initialized_server(attach_pipeline=True)
        runtime = server.stream.runtime
        self.start(caller, server)
        self.command(caller, server, feed_command(np.full(1920, 1000, dtype="<i2")))
        self.assertTrue(native.decode.entered[0].wait(TIMEOUT))
        native.decode.release[1].set()
        self.command(caller, server, {"action": "stream_finish", "session_id": 1})
        self.assertFalse(server.stream.active)
        self.assertTrue(all(thread.is_alive() for thread in native.threads))
        caller.submit(server._close_runtime).result()
        native.close.assert_called_once_with()
        self.assertEqual(native.alive_at_close, [[False]], "native cannot be freed before its worker joins")
        self.assertEqual(native.preparers_alive_at_close, [[False]], "CPU owner must also join before native is freed")
        self.assertTrue(all(not thread.is_alive() for thread in native.threads | vad.threads))
        caller.submit(server._close_runtime).result()
        native.close.assert_called_once_with()
        with self.assertRaises(RuntimeError):
            caller.submit(runtime.start).result()

    def test_cleanup_exceptions_still_attempt_processor_pipeline_and_native_close(self):
        for failure in ("stream_reset", "processor_close"):
            with self.subTest(failure=failure):
                caller, server, native, vad = self.initialized_server(attach_pipeline=True)
                runtime = server.stream.runtime
                self.start(caller, server)
                self.command(caller, server, feed_command(np.full(1920, 1000, dtype="<i2")))
                self.assertTrue(native.decode.entered[0].wait(TIMEOUT))
                processor = mock.Mock()
                server._audio_processor = server.stream.audio_processor = processor
                if failure == "stream_reset":
                    processor.reset.side_effect = RuntimeError("injected stream reset failure")
                else:
                    processor.close.side_effect = RuntimeError("injected processor close failure")
                stream_close = self.enterContext(mock.patch.object(server.stream, "close", wraps=server.stream.close))
                started = threading.Event()

                def unload():
                    started.set()
                    try:
                        server._close_runtime()
                    except RuntimeError:
                        pass  # Reporting cleanup errors is allowed; abandoning resources is not.

                closed = caller.submit(unload)
                self.assertTrue(started.wait(TIMEOUT))
                native.decode.release[1].set()
                closed.result()
                stream_close.assert_called_once_with()
                processor.close.assert_called_once_with()
                native.close.assert_called_once_with()
                self.assertEqual(native.alive_at_close, [[False]])
                self.assertEqual(native.preparers_alive_at_close, [[False]])
                self.assertTrue(all(not thread.is_alive() for thread in native.threads | vad.threads))
                with self.assertRaises(RuntimeError):
                    caller.submit(runtime.start).result()


if __name__ == "__main__":
    unittest.main()
