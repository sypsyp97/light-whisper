"""Focused tests for R2T2 native startup and first-caption batching."""
import ctypes as ct
import os
import sys
import unittest
from unittest.mock import Mock, patch

import numpy as np

sys.path.insert(0, os.path.dirname(__file__))

import r2t2_asr_server
import r2t2_native
from r2t2_asr_server import R2T2ASRServer
from r2t2_native import NativeRuntime


class FakeCFunction:
    def __init__(self, result=0):
        self.result = result
        self.restype = None
        self.argtypes = None
        self.calls = []

    def __call__(self, *args):
        self.calls.append(args)
        return self.result() if callable(self.result) else self.result


class FakeCLibrary:
    _symbols = (
        "abi_version",
        "last_error",
        "registry_create",
        "model_load",
        "options_create",
        "options_set",
        "session_create",
        "request_create",
        "request_set_text",
        "stream_start",
        "stream_push",
        "stream_finish",
        "stream_reset",
        "event_as_result",
        "result_text",
        "result_preview_text",
        "registry_free",
        "model_free",
        "session_free",
        "options_free",
        "request_free",
        "result_free",
        "event_free",
    )

    def __init__(self, abi):
        self.functions = {
            "audiocpp_" + name: FakeCFunction(0) for name in self._symbols
        }
        self.functions["audiocpp_abi_version"] = FakeCFunction(abi)
        self.functions["audiocpp_last_error"] = FakeCFunction(b"fake error")
        self.functions["audiocpp_options_create"] = FakeCFunction(ct.c_void_p(1))

    def __getattr__(self, name):
        try:
            return self.functions[name]
        except KeyError as exc:
            raise AttributeError(name) from exc


class RecordingPushFunctions:
    def __init__(self, deltas=(), emit_events=True):
        self.deltas = list(deltas)
        self.emit_events = emit_events
        self.pushes = []
        self.committed = ""
        self.read_count = 0
        self.event_frees = []
        self.reset_calls = 0
        self.start_calls = 0

    def stream_push(
        self, _session, data, sample_count, sample_rate, channels, offset, event
    ):
        sample_count = int(sample_count)
        values = np.ctypeslib.as_array(data, shape=(sample_count,)).copy()
        self.pushes.append(
            {
                "offset": int(offset),
                "sample_rate": int(sample_rate),
                "channels": int(channels),
                "values": values,
            }
        )
        if self.emit_events:
            event._obj.value = len(self.pushes)
        return 0

    def event_as_result(self, event):
        return event

    def result_text(self, _result, text, language):
        delta = self.deltas[self.read_count] if self.read_count < len(self.deltas) else ""
        self.read_count += 1
        self.committed += delta
        text._obj.value = delta.encode("utf-8")
        language._obj.value = b"en"
        return 0

    def result_preview_text(self, _result, preview):
        preview._obj.value = self.committed.encode("utf-8")
        return 0

    def event_free(self, event):
        self.event_frees.append(event.value if event else None)

    def stream_reset(self, _session):
        self.reset_calls += 1
        self.committed = ""
        return 0

    def request_create(self):
        return ct.c_void_p(2)

    def request_set_text(self, *_args):
        return 0

    def stream_start(self, *_args):
        self.start_calls += 1
        return 0

    def request_free(self, _request):
        return None


def make_feed_runtime(*, capability=True, chunk_samples=4000, deltas=()):
    recorder = RecordingPushFunctions(deltas)
    runtime = NativeRuntime.__new__(NativeRuntime)
    runtime._active = True
    runtime._committed = ""
    runtime._preview_text = ""
    runtime._language = None
    runtime._offset = 0
    runtime.session = ct.c_void_p(1)
    runtime.chunk_samples = chunk_samples
    runtime.functions = {
        "stream_push": recorder.stream_push,
        "event_as_result": recorder.event_as_result,
        "result_text": recorder.result_text,
        "result_preview_text": recorder.result_preview_text,
        "event_free": recorder.event_free,
        "stream_reset": recorder.stream_reset,
        "request_create": recorder.request_create,
        "request_set_text": recorder.request_set_text,
        "stream_start": recorder.stream_start,
        "request_free": recorder.request_free,
        "last_error": lambda: b"fake error",
    }
    if capability is not None:
        runtime._batch_initial_audio = capability
    return runtime, recorder


class NativeStartupTests(unittest.TestCase):
    def test_constructor_preserves_backend_and_real_bind_enables_only_supported_cuda(self):
        fake_library = FakeCLibrary(0x00000400)
        with (
            patch.object(r2t2_native.ct, "CDLL", return_value=fake_library),
            patch.object(
                r2t2_native.os,
                "add_dll_directory",
                return_value=Mock(),
                create=True,
            ),
        ):
            runtime = NativeRuntime(
                "model.gguf",
                "audiocpp.dll",
                backend="cuda",
                chunk_ms=160,
                rolling=True,
            )

        try:
            self.assertEqual(getattr(runtime, "backend", None), "cuda")
            self.assertTrue(getattr(runtime, "_batch_initial_audio", False))
        finally:
            runtime.close()

    def test_real_bind_accepts_03_and_gates_initial_batch_by_backend_and_rolling(self):
        cases = (
            (0x00000300, "cuda", True, False),
            (0x00000400, "cuda", True, True),
            (0x00000400, "cpu", True, False),
            (0x00000400, "cuda", False, False),
        )
        for abi, backend, rolling, expected in cases:
            with self.subTest(abi=abi, backend=backend, rolling=rolling):
                runtime = NativeRuntime.__new__(NativeRuntime)
                runtime.lib = FakeCLibrary(abi)
                runtime.functions = {}
                runtime.backend = backend
                runtime.rolling = rolling
                runtime._bind()
                self.assertEqual(
                    getattr(runtime, "_batch_initial_audio", False), expected
                )

    def test_aligned_first_feed_batches_once_then_later_feeds_split_and_keep_deltas(self):
        runtime, recorder = make_feed_runtime(
            capability=True,
            deltas=("首段", "后半", "尾段"),
        )
        first = np.arange(8000, dtype=np.float32)
        second = np.arange(8000, dtype=np.float32) + 10000

        self.assertEqual(runtime.feed(first), ("首段", "en"))
        self.assertEqual(len(recorder.pushes), 1)
        self.assertEqual(recorder.pushes[0]["offset"], 0)
        self.assertEqual(recorder.pushes[0]["sample_rate"], 16000)
        self.assertEqual(recorder.pushes[0]["channels"], 1)
        np.testing.assert_array_equal(recorder.pushes[0]["values"], first)

        self.assertEqual(runtime.feed(second), ("首段后半尾段", "en"))
        self.assertEqual(
            [push["offset"] for push in recorder.pushes],
            [0, 8000, 12000],
        )
        self.assertEqual(
            [len(push["values"]) for push in recorder.pushes],
            [8000, 4000, 4000],
        )
        np.testing.assert_array_equal(recorder.pushes[1]["values"], second[:4000])
        np.testing.assert_array_equal(recorder.pushes[2]["values"], second[4000:])

    def test_batch_boundaries_and_legacy_fakes_keep_regular_splitting(self):
        cases = ((4001, 2), (8000, 1), (16000, 1), (16001, 5))
        for length, expected_pushes in cases:
            with self.subTest(length=length):
                runtime, recorder = make_feed_runtime(
                    capability=True,
                    deltas=(),
                )
                runtime.feed(np.zeros(length, dtype=np.float32))
                self.assertEqual(len(recorder.pushes), expected_pushes)

        runtime, recorder = make_feed_runtime(
            capability=None,
            deltas=("a", "b"),
        )
        self.assertEqual(
            runtime.feed(np.ones(8000, dtype=np.float32)),
            ("ab", "en"),
        )
        self.assertEqual(
            [push["offset"] for push in recorder.pushes],
            [0, 4000],
        )

    def test_invalid_pcm_is_rejected_before_push_and_does_not_consume_first_batch(self):
        runtime, recorder = make_feed_runtime(capability=True, deltas=("ok",))
        invalid_audio = (
            np.array([1.0], dtype=np.float64),
            np.array([np.nan], dtype=np.float32),
        )
        for pcm in invalid_audio:
            with self.assertRaises(ValueError):
                runtime.feed(pcm)

        self.assertEqual(recorder.pushes, [])
        self.assertEqual(runtime._offset, 0)
        runtime.feed(np.ones(8000, dtype=np.float32))
        self.assertEqual(len(recorder.pushes), 1)
        self.assertEqual(recorder.pushes[0]["offset"], 0)

    def test_reset_and_start_reenable_first_offset_zero_batch(self):
        runtime, recorder = make_feed_runtime(
            capability=True,
            deltas=("first", "again"),
        )
        runtime.feed(np.ones(8000, dtype=np.float32))
        runtime.reset()
        runtime.start()
        runtime.feed(np.ones(8000, dtype=np.float32))

        self.assertEqual(recorder.reset_calls, 1)
        self.assertEqual(recorder.start_calls, 1)
        self.assertEqual(
            [(push["offset"], len(push["values"])) for push in recorder.pushes],
            [(0, 8000), (0, 8000)],
        )


class TypedWarmupRuntime:
    def __init__(self, server, backend, fail_stage=None):
        self.server = server
        self.backend = backend
        self.chunk_samples = 2560 if backend == "cuda" else 5120
        self.fail_stage = fail_stage
        self.events = []
        self.closed = False

    def start(self):
        self.events.append(("start", self.server.native))
        if self.fail_stage == "start":
            raise RuntimeError("warmup start failed")

    def feed(self, pcm):
        copied = np.array(pcm, copy=True)
        self.events.append(("feed", self.server.native, copied))
        if self.fail_stage == "feed":
            raise RuntimeError("warmup feed failed")

    def reset(self):
        self.events.append(("reset", self.server.native))
        if self.fail_stage == "reset":
            raise RuntimeError("warmup reset failed")

    def close(self):
        self.closed = True


class ServerWarmupTests(unittest.TestCase):
    @staticmethod
    def _server():
        server = R2T2ASRServer.__new__(R2T2ASRServer)
        server.native = "previous-runtime"
        server.transcription_count = 7
        server.total_audio_duration = 11.5
        server._total_inference_ms = 123.0
        return server

    @staticmethod
    def _factory(server, instances, fail_stage=None):
        def construct(_model_path, _library_path, *, backend, **_kwargs):
            runtime = TypedWarmupRuntime(
                server,
                backend,
                fail_stage=fail_stage if backend == "cuda" else None,
            )
            instances.append(runtime)
            return runtime

        return construct

    def test_cuda_warmup_is_exact_and_ready_runtime_is_assigned_after_reset(self):
        server = self._server()
        instances = []
        with patch.object(
            r2t2_asr_server,
            "NativeRuntime",
            side_effect=self._factory(server, instances),
        ):
            server._load_runtime("model.gguf")

        self.assertEqual(len(instances), 1)
        runtime = instances[0]
        self.assertEqual(runtime.backend, "cuda")
        self.assertEqual([event[0] for event in runtime.events], ["start", "feed", "reset"])
        self.assertTrue(all(event[1] is None for event in runtime.events))
        feed = runtime.events[1][2]
        self.assertEqual(feed.dtype, np.dtype(np.float32))
        self.assertEqual(feed.shape, (runtime.chunk_samples,))
        np.testing.assert_array_equal(feed, np.zeros(runtime.chunk_samples, dtype=np.float32))
        self.assertIs(server.native, runtime)
        self.assertFalse(runtime.closed)
        self.assertEqual(server.transcription_count, 7)
        self.assertEqual(server.total_audio_duration, 11.5)
        self.assertEqual(server._total_inference_ms, 123.0)

    def test_cuda_warmup_failures_reset_and_close_cuda_then_use_unwarmed_cpu(self):
        for fail_stage in ("start", "feed", "reset"):
            with self.subTest(fail_stage=fail_stage):
                server = self._server()
                instances = []
                with patch.object(
                    r2t2_asr_server,
                    "NativeRuntime",
                    side_effect=self._factory(server, instances, fail_stage),
                ):
                    server._load_runtime("model.gguf")

                self.assertEqual([runtime.backend for runtime in instances], ["cuda", "cpu"])
                cuda, cpu = instances
                self.assertTrue(cuda.closed)
                expected_events = {
                    "start": ["start", "reset"],
                    "feed": ["start", "feed", "reset"],
                    "reset": ["start", "feed", "reset"],
                }
                self.assertEqual(
                    [event[0] for event in cuda.events], expected_events[fail_stage]
                )
                self.assertEqual(cpu.events, [])
                self.assertIs(server.native, cpu)
                self.assertEqual(server.backend, "cpu")
                self.assertEqual(server.device, "cpu")


if __name__ == "__main__":
    unittest.main()
