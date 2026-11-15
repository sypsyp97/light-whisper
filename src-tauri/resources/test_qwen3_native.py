

import ctypes as ct
import os
from pathlib import Path
import sys
from types import SimpleNamespace
import unittest
from unittest import mock

import numpy as np

sys.path.insert(0, os.path.dirname(__file__))

from qwen3_native import QwenNativeRuntime, QwenPreviewRuntime


class ModelConfig(ct.Structure):
    _fields_ = [(name, ct.c_char_p) for name in ("family", "config", "weight", "spec")]


class BackendConfig(ct.Structure):
    _fields_ = [("backend", ct.c_char_p), ("device", ct.c_int), ("threads", ct.c_int)]


class FakeLibrary:


    def __init__(self, *, fail_at=None, abi=0x0400, null_request=False):
        self.fail_at, self.abi, self.null_request = fail_at, abi, null_request
        self.symbols = {}
        self.calls = []
        self.allocated = {}
        self.freed = []
        self.requests = []
        self.model_loads = []
        self.sessions = []
        self.output = ("今天是周四。", "Chinese")
        self.next_handle = 1

    @staticmethod
    def handle(value):
        return value.value if isinstance(value, ct.c_void_p) else value

    def allocate(self, kind, output=None):
        handle = self.next_handle
        self.next_handle += 1
        self.allocated[handle] = kind
        if output is not None:
            output._obj.value = handle
        return handle

    def __getattr__(self, symbol):
        if not symbol.startswith("audiocpp_"):
            raise AttributeError(symbol)
        name = symbol[len("audiocpp_"):]
        if name not in self.symbols:
            self.symbols[name] = mock.Mock(side_effect=lambda *args: self.invoke(name, *args))
        return self.symbols[name]

    def invoke(self, name, *args):
        self.calls.append(name)
        if name == "abi_version":
            return self.abi
        if name == "last_error":
            return b"injected native failure"
        if name.endswith("_free"):
            handle = self.handle(args[0])
            if handle:
                self.freed.append((name[:-5], handle))
            return None
        if name == "request_create":
            if self.null_request:
                return None
            handle = self.allocate("request")
            self.requests.append({"handle": handle, "options": {}})
            return handle
        if name == "options_create":
            return self.allocate("options")
        if name == "registry_create":
            self.allocate("registry", args[-1])
        elif name == "model_load":
            config = ct.cast(args[2], ct.POINTER(ModelConfig)).contents
            self.model_loads.append((args[1], config.family))
            self.allocate("model", args[-1])
        elif name == "session_create":
            backend = ct.cast(args[3], ct.POINTER(BackendConfig)).contents
            self.sessions.append((args[1], args[2], backend.backend, backend.device, backend.threads))
            self.allocate("session", args[-1])
        elif name == "request_set_text":
            self.requests[-1]["context_language"] = args[1:]
        elif name == "request_set_option":
            self.requests[-1]["options"][args[1]] = args[2]
        elif name == "request_set_audio":
            self.requests[-1]["pcm"] = np.ctypeslib.as_array(args[1], shape=(args[2],)).copy()
            self.requests[-1]["format"] = args[3:]
        elif name == "session_run":
            self.allocate("result", args[-1])
        elif name == "result_text":
            text, language = self.output
            args[1]._obj.value = text.encode("utf-8")
            args[2]._obj.value = language.encode("utf-8") if language else None
        elif name == "model_family":
            return b"qwen3_asr"
        elif name == "model_supports":
            return 1
        return 2 if name == self.fail_at else 0


class QwenNativeRuntimeTests(unittest.TestCase):
    def setUp(self):
        self.library = FakeLibrary()
        self.dll_directories = []

        def add_directory(_path):
            directory = mock.Mock()
            self.dll_directories.append(directory)
            return directory

        self.enterContext(mock.patch("ctypes.CDLL", side_effect=lambda _path: self.library))
        self.enterContext(mock.patch("os.add_dll_directory", side_effect=add_directory, create=True))

    def runtime(self, **kwargs):
        runtime = QwenNativeRuntime("model.gguf", "audio.dll", **kwargs)
        self.addCleanup(runtime.close)
        return runtime

    def assert_all_allocations_freed_once(self):
        self.assertCountEqual(self.library.freed, [
            (kind, handle) for handle, kind in self.library.allocated.items()
        ])

    def test_loads_qwen_family_offline_session_and_closes_once(self):
        runtime = self.runtime(backend="cpu", threads=3, dll_directories=("dependencies",))
        self.assertEqual(self.library.model_loads, [
            (str(Path("model.gguf").resolve()).encode("utf-8"), b"qwen3_asr")
        ])
        self.assertEqual(self.library.sessions, [(b"asr", b"offline", b"cpu", 0, 3)])
        runtime.close()
        runtime.close()
        self.assert_all_allocations_freed_once()
        for directory in self.dll_directories:
            directory.close.assert_called_once()

    def test_run_preserves_noncontiguous_pcm_and_thirteen_sample_tail(self):
        runtime = self.runtime()
        pcm = np.linspace(-0.75, 0.75, 20506, dtype=np.float32)[::2]
        self.assertFalse(pcm.flags.c_contiguous)
        result = runtime.run(pcm, context="医学术语", language="Chinese", timestamps="none")
        self.assertEqual((result.text, result.language), self.library.output)
        request = self.library.requests[0]
        self.assertEqual(request["format"], (16000, 1))
        self.assertEqual(len(request["pcm"]), 10253)
        np.testing.assert_array_equal(request["pcm"], pcm)
        self.assertEqual(request["context_language"], ("医学术语".encode(), b"Chinese"))
        self.assertEqual(request["options"], {
            b"max_tokens": b"512", b"return_timestamps": b"false", b"audio_chunk_mode": b"none",
        })
        runtime.close()
        self.assert_all_allocations_freed_once()

    def test_fresh_requests_clear_previous_context_and_forced_language(self):
        runtime = self.runtime()
        pcm = np.zeros(13, dtype=np.float32)
        runtime.run(pcm, context="旧上下文", language="Chinese")
        self.library.output = ("new output", None)
        result = runtime.run(pcm)
        self.assertEqual((result.text, result.language), ("new output", None))
        self.assertEqual([request["context_language"] for request in self.library.requests], [
            ("旧上下文".encode(), b"Chinese"), (b"", None),
        ])
        self.assertEqual(len({request["handle"] for request in self.library.requests}), 2)
        self.assertEqual(self.library.requests[0]["options"], self.library.requests[1]["options"])
        self.assertEqual(self.library.calls.count("model_load"), 1)
        self.assertEqual(self.library.calls.count("session_create"), 1)

    def test_invalid_pcm_never_creates_a_request_or_runs_native_audio(self):
        runtime = self.runtime()
        for invalid in (
            [], np.array([], dtype=np.float32), np.zeros(3, dtype=np.float64),
            np.zeros((2, 2), dtype=np.float32), np.array(1, dtype=np.float32),
            np.array([np.nan], dtype=np.float32), np.array([np.inf], dtype=np.float32),
            np.array([-np.inf], dtype=np.float32),
        ):
            with self.subTest(shape=getattr(invalid, "shape", None), value=repr(invalid)):
                with self.assertRaises(ValueError):
                    runtime.run(invalid)
        self.assertEqual(self.library.requests, [])
        self.assertNotIn("session_run", self.library.calls)

    def test_partial_constructor_handles_are_freed_when_native_calls_fail(self):
        for failure in ("registry_create", "model_load", "session_create"):
            with self.subTest(failure=failure):
                self.library = FakeLibrary(fail_at=failure)
                with self.assertRaisesRegex(RuntimeError, "injected native failure"):
                    QwenNativeRuntime("model.gguf", "audio.dll")
                self.assert_all_allocations_freed_once()

    def test_request_and_result_are_freed_even_when_output_is_set_before_failure(self):
        for failure in ("request_set_text", "request_set_option", "request_set_audio",
                        "session_run", "result_text"):
            with self.subTest(failure=failure):
                self.library = FakeLibrary()
                runtime = self.runtime()
                self.library.fail_at = failure
                with self.assertRaisesRegex(RuntimeError, "injected native failure"):
                    runtime.run(np.ones(13, dtype=np.float32))
                runtime.close()
                self.assert_all_allocations_freed_once()

    def test_null_request_and_closed_runtime_never_run_audio(self):
        runtime = self.runtime()
        self.library.null_request = True
        with self.assertRaises(RuntimeError):
            runtime.run(np.zeros(13, dtype=np.float32))
        runtime.close()
        with self.assertRaises(RuntimeError):
            runtime.run(np.zeros(13, dtype=np.float32))
        self.assertNotIn("session_run", self.library.calls)
        self.assert_all_allocations_freed_once()


class QwenPreviewRuntimeTests(unittest.TestCase):
    @staticmethod
    def result(text, language=None):
        return SimpleNamespace(text=text, language=language)

    def test_sample_threshold_emits_revision_and_pads_first_preview_to_half_second(self):
        calls = []
        outputs = iter([self.result("今天是周三", "Chinese"), self.result("今天是周四", None)])

        def transcribe(pcm, **options):
            calls.append((pcm.copy(), options))
            return next(outputs)

        runtime = QwenPreviewRuntime(transcribe)
        runtime.start("术语", "zh")
        first = np.linspace(-0.5, 0.5, 1920, dtype=np.float32)
        original = first.copy()
        self.assertEqual(runtime.feed(first[:1919]), ("", "zh"))
        first[:1919] = 0
        self.assertEqual(calls, [])
        self.assertEqual(runtime.feed(first[1919:]), ("", "Chinese"))
        self.assertEqual(runtime.preview_text, "今天是周三")
        self.assertEqual(len(calls), 1)
        np.testing.assert_array_equal(calls[0][0][:1920], original)
        np.testing.assert_array_equal(calls[0][0][1920:], np.zeros(6080, dtype=np.float32))
        self.assertEqual(calls[0][1], {"context": "术语", "language": "zh"})
        second = np.full(3200, 0.25, dtype=np.float32)
        self.assertEqual(runtime.feed(second[:3199]), ("", "Chinese"))
        self.assertEqual(len(calls), 1)
        self.assertEqual(runtime.feed(second[3199:]), ("", "Chinese"))
        self.assertEqual(len(calls), 2)
        self.assertEqual(runtime.preview_text, "今天是周四")
        np.testing.assert_array_equal(calls[1][0][:5120], np.concatenate((original, second)))
        np.testing.assert_array_equal(calls[1][0][5120:], np.zeros(2880, dtype=np.float32))
        self.assertEqual(calls[1][1], {"context": "术语", "language": "zh"})

    def test_empty_startup_previews_retry_every_three_capture_polls_until_first_text(self):
        calls = []
        outputs = iter(self.result(text) for text in ("", "", "首个草稿", "", "后续修订"))

        def transcribe(pcm, **options):
            calls.append((pcm.copy(), options))
            return next(outputs)

        runtime = QwenPreviewRuntime(transcribe)
        runtime.start("术语", "zh")
        pcm = np.linspace(-0.5, 0.5, 12160, dtype=np.float32)
        preview_polls = []
        hypotheses = {}
        for poll in range(1, 20):
            previous_calls = len(calls)
            self.assertEqual(runtime.feed(pcm[(poll - 1) * 640:poll * 640]), ("", "zh"))
            if len(calls) != previous_calls:
                preview_polls.append(poll)
                hypotheses[poll] = runtime.preview_text



        self.assertEqual(preview_polls, [3, 6, 9, 14, 19])
        self.assertEqual(hypotheses, {
            3: "", 6: "", 9: "首个草稿", 14: "", 19: "后续修订",
        })
        for (audio, options), captured_samples in zip(calls, (1920, 3840, 5760, 8960, 12160)):
            np.testing.assert_array_equal(audio[:captured_samples], pcm[:captured_samples])
            self.assertEqual(len(audio), max(8000, captured_samples))
            self.assertEqual(options, {"context": "术语", "language": "zh"})

    def test_custom_preview_threshold_bounds_startup_and_controls_later_revisions(self):
        for preview_samples in (640, 1600, 1920, 4800):
            with self.subTest(preview_samples=preview_samples):
                transcribe = mock.Mock(return_value=self.result("草稿"))
                runtime = QwenPreviewRuntime(transcribe, preview_samples=preview_samples)
                runtime.start()
                startup_samples = min(preview_samples, 1920)
                runtime.feed(np.ones(startup_samples - 1, dtype=np.float32))
                transcribe.assert_not_called()
                runtime.feed(np.ones(1, dtype=np.float32))
                self.assertEqual(transcribe.call_count, 1)
                runtime.feed(np.ones(preview_samples - 1, dtype=np.float32))
                self.assertEqual(transcribe.call_count, 1)
                runtime.feed(np.ones(1, dtype=np.float32))
                self.assertEqual(transcribe.call_count, 2)

    def test_reset_and_start_restore_startup_cadence_after_a_nonempty_preview(self):
        for reset_first in (False, True):
            with self.subTest(reset_first=reset_first):
                transcribe = mock.Mock(side_effect=[self.result("旧草稿", "Chinese"),
                                                    self.result("新草稿")])
                runtime = QwenPreviewRuntime(transcribe)
                runtime.start("旧上下文", "Chinese")
                runtime.feed(np.ones(3200, dtype=np.float32))
                self.assertEqual(runtime.preview_text, "旧草稿")
                runtime.feed(np.ones(3199, dtype=np.float32))
                self.assertEqual(transcribe.call_count, 1)
                if reset_first:
                    runtime.reset()
                    self.assertEqual(runtime.preview_text, "")
                    with self.assertRaises(RuntimeError):
                        runtime.feed(np.ones(1, dtype=np.float32))
                runtime.start("新上下文", "en")
                self.assertEqual(runtime.preview_text, "")
                fresh = np.full(1920, 0.25, dtype=np.float32)
                self.assertEqual(runtime.feed(fresh[:1919]), ("", "en"))
                self.assertEqual(transcribe.call_count, 1)
                self.assertEqual(runtime.feed(fresh[1919:]), ("", "en"))
                self.assertEqual(transcribe.call_count, 2)
                self.assertEqual(runtime.preview_text, "新草稿")
                audio = transcribe.call_args.args[0]
                np.testing.assert_array_equal(audio[:1920], fresh)
                np.testing.assert_array_equal(audio[1920:], np.zeros(6080, dtype=np.float32))
                self.assertEqual(transcribe.call_args.kwargs, {"context": "新上下文", "language": "en"})

    def test_finish_uses_full_capture_including_tail_after_bounded_preview_window(self):
        calls = []

        def transcribe(pcm, **options):
            calls.append((pcm.copy(), options))
            return self.result("完整最终结果" if len(pcm) == 12813 else "窗口草稿", "Chinese")

        runtime = QwenPreviewRuntime(transcribe, window_samples=9600)
        runtime.start("保留上下文", "Chinese")
        pcm = np.linspace(-0.8, 0.8, 12813, dtype=np.float32)
        for offset in range(0, 12800, 3200):
            self.assertEqual(runtime.feed(pcm[offset:offset + 3200]), ("", "Chinese"))
        previews = len(calls)
        self.assertEqual(runtime.feed(pcm[12800:]), ("", "Chinese"))
        self.assertEqual(len(calls), previews)
        self.assertEqual(len(calls[-1][0]), 9600)
        np.testing.assert_array_equal(calls[-1][0], pcm[3200:12800])
        self.assertEqual(runtime.preview_text, "窗口草稿")
        self.assertEqual(runtime.finish(), ("完整最终结果", "Chinese"))
        self.assertEqual(len(calls), previews + 1)
        np.testing.assert_array_equal(calls[-1][0], pcm)
        self.assertEqual(calls[-1][1], {"context": "保留上下文", "language": "Chinese"})

    def test_reset_drops_audio_context_language_and_preview_before_next_capture(self):
        calls = []

        def transcribe(pcm, **options):
            calls.append((pcm.copy(), options))
            return self.result("旧草稿", "Chinese") if len(calls) == 1 else self.result("新内容")

        runtime = QwenPreviewRuntime(transcribe)
        runtime.start("旧上下文", "Chinese")
        runtime.feed(np.ones(3200, dtype=np.float32))
        runtime.reset()
        self.assertEqual(runtime.preview_text, "")
        runtime.start()
        short = np.linspace(-0.1, 0.1, 13, dtype=np.float32)
        self.assertEqual(runtime.feed(short), ("", None))
        self.assertEqual(runtime.finish(), ("新内容", None))
        self.assertEqual(len(calls), 2)
        self.assertEqual(calls[-1][1], {"context": "", "language": None})
        self.assertEqual(len(calls[-1][0]), 8000)
        np.testing.assert_array_equal(calls[-1][0][:13], short)
        np.testing.assert_array_equal(calls[-1][0][13:], np.zeros(7987, dtype=np.float32))

    def test_empty_finish_does_not_invoke_transcription(self):
        transcribe = mock.Mock()
        runtime = QwenPreviewRuntime(transcribe)
        runtime.start("术语", "Chinese")
        self.assertEqual(runtime.finish(), ("", "Chinese"))
        transcribe.assert_not_called()

    def test_final_transcription_failure_never_returns_the_previous_preview(self):
        transcribe = mock.Mock(side_effect=[self.result("旧窗口草稿", "Chinese"),
                                            RuntimeError("full capture decode failed")])
        runtime = QwenPreviewRuntime(transcribe)
        runtime.start()
        runtime.feed(np.ones(3200, dtype=np.float32))
        self.assertEqual(runtime.preview_text, "旧窗口草稿")

        runtime.feed(np.full(13, 0.25, dtype=np.float32))
        with self.assertRaisesRegex(RuntimeError, "full capture decode failed"):
            runtime.finish()
        self.assertEqual(transcribe.call_count, 2)


if __name__ == "__main__":
    unittest.main()
