
import ctypes as ct
import unittest
from unittest.mock import Mock, patch

import numpy as np

import r2t2_native
from r2t2_native import NativeRuntime
from r2t2_stream import R2T2StreamSession


class FinishLibrary:


    SYMBOLS = (
        "abi_version last_error registry_create model_load options_create options_set "
        "session_create request_create request_set_text stream_start stream_push "
        "stream_finish stream_reset event_as_result result_text result_preview_text "
        "registry_free model_free session_free options_free request_free result_free "
        "event_free session_run request_set_audio request_set_option"
    ).split()

    def __init__(self, abi=0x00000500, fail=None):
        self.abi, self.fail = abi, fail
        self.calls, self.pushes = [], []
        self.options = {}
        self.session_options_handle = None
        self.session_options = None
        self.functions = {
            "audiocpp_" + name: Mock(side_effect=lambda *args, name=name: self.call(name, *args))
            for name in self.SYMBOLS
        }

    def __getattr__(self, name):
        if name not in self.functions:
            raise AttributeError(name)
        return self.functions[name]

    def call(self, name, *args):
        self.calls.append((name, args))
        if name == "abi_version":
            return self.abi
        if name == "last_error":
            return b"injected native failure"
        if name == "options_create":
            self.options[4] = {}
            return ct.c_void_p(4)
        if name == "request_create":
            return ct.c_void_p(4)
        if name == "options_set":
            handle = args[0].value if isinstance(args[0], ct.c_void_p) else args[0]
            self.options.setdefault(handle, {})[args[1]] = args[2]
        if name == "session_create":
            options = args[-2]
            handle = options.value if isinstance(options, ct.c_void_p) else options
            self.session_options_handle = handle
            self.session_options = dict(self.options.get(handle, {}))
        if name in ("registry_create", "model_load", "session_create", "stream_finish"):
            args[-1]._obj.value = 3
        if name == "stream_push":
            self.pushes.append((args[5], np.ctypeslib.as_array(args[1], shape=(args[2],)).copy()))
        if name == self.fail:
            return 1
        if name == "result_text":
            args[1]._obj.value = "完整句子".encode("utf-8")
            args[2]._obj.value = None
        return 0

    def count(self, name):
        return sum(kind == name for kind, _ in self.calls)

class FinishContextTests(unittest.TestCase):
    def runtime(self, *, abi=0x00000500, fail=None, **kwargs):
        library = FinishLibrary(abi, fail)
        defaults = dict(backend="cuda", rolling=True, chunk_ms=160)
        defaults.update(kwargs)
        with (
            patch.object(r2t2_native.ct, "CDLL", return_value=library),
            patch.object(r2t2_native.os, "add_dll_directory", return_value=Mock(), create=True),
        ):
            runtime = NativeRuntime("model.gguf", "audiocpp.dll", **defaults)
        self.addCleanup(runtime.close)
        return runtime, library

    def test_adaptive_cadence_negotiates_native_option_and_preserves_input_grain(self):
        cases = (
            (0x00000600, {"adaptive_cadence": True}, True),
            (0x00000700, {"adaptive_cadence": True}, True),
            (0x00000500, {"adaptive_cadence": True}, False),
            (0x00000600, {}, False),
            (0x00000600, {"adaptive_cadence": False}, False),
            (0x00000600, {"adaptive_cadence": True, "backend": "cpu"}, False),
            (0x00000600, {"adaptive_cadence": True, "rolling": False}, False),
            (0x00000600, {"adaptive_cadence": True, "family": "qwen3_asr"}, False),
            (0x00000600, {"adaptive_cadence": True, "mode": "offline"}, False),
            (0x00000600, {"adaptive_cadence": True, "chunk_ms": 320}, False),
        )
        key = b"confucius4_r2t2.adaptive_cadence"
        for abi, kwargs, enabled in cases:
            with self.subTest(abi=hex(abi), kwargs=kwargs):
                runtime, library = self.runtime(abi=abi, **kwargs)
                options = library.session_options
                if enabled:
                    self.assertEqual(options.get(key), b"true")
                else:
                    self.assertNotIn(key, options)
                chunk_ms = kwargs.get("chunk_ms", 160)
                if kwargs.get("family", "confucius4_r2t2") == "confucius4_r2t2":
                    self.assertEqual(options[b"confucius4_r2t2.chunk_size_ms"], str(chunk_ms).encode())
                self.assertEqual(runtime.chunk_samples, chunk_ms * 16)
                self.assertEqual(runtime.steady_chunk_samples, 5120 if enabled else runtime.chunk_samples)

    def test_negotiates_only_supported_family_backend_and_streaming_policy(self):
        matrix = (
            (0x00000500, {}, True),
            (0x00000600, {}, True),
            (0x00000300, {}, False),
            (0x00000400, {}, False),
            (0x00000500, {"backend": "cpu"}, False),
            (0x00000500, {"rolling": False}, False),
            (0x00000500, {"family": "qwen3_asr"}, False),
            (0x00000500, {"mode": "offline"}, False),
        )
        for abi, options, optimized in matrix:
            with self.subTest(abi=hex(abi), options=options):
                runtime, library = self.runtime(abi=abi, **options)
                self.assertEqual(library.session_options_handle, 4)
                self.assertEqual(
                    library.session_options.get(b"confucius4_r2t2.coalesced_finish"),
                    b"true" if optimized else None,
                )
                runtime.start()
                runtime.feed(np.ones(37, dtype=np.float32))
                before = len(library.pushes)
                runtime.finish()
                padding = library.pushes[before:]
                self.assertEqual(len(padding), 0 if optimized else 2)
                self.assertEqual(sum(pcm.size for _, pcm in padding), 0 if optimized else 5120)
                for _, pcm in padding:
                    np.testing.assert_array_equal(pcm, np.zeros(pcm.size, dtype=np.float32))
                self.assertEqual(runtime._offset, 37 + 5120)
                self.assertEqual(library.count("stream_finish"), 1)

    def test_fake_session_copies_only_the_supplied_options_at_creation(self):
        library = FinishLibrary()
        options = library.audiocpp_options_create()
        library.audiocpp_session_create(None, None, None, None, options, ct.byref(ct.c_void_p()))
        library.audiocpp_options_set(options, b"confucius4_r2t2.coalesced_finish", b"true")
        self.assertEqual(library.session_options, {})
        for wrong in (None, ct.c_void_p(99)):
            library.audiocpp_session_create(None, None, None, None, wrong, ct.byref(ct.c_void_p()))
            self.assertEqual(library.session_options, {})
            self.assertNotEqual(library.session_options_handle, options.value)

    def test_one_finish_preserves_tiny_nonhop_and_chunk_boundary_sample_counts(self):
        for count in (1, 37, 2559, 2560, 5123):
            with self.subTest(captured_samples=count):
                runtime, library = self.runtime()
                session = R2T2StreamSession(runtime)
                session.start(1, context="医学 热词", language="Chinese")
                response = session.feed(1, np.ones(count, dtype=np.float32), offset=0)
                self.assertEqual(response["sample_count"], count)
                pushes = len(library.pushes)
                final = session.finish(1)
                self.assertEqual(len(library.pushes), pushes)
                self.assertEqual(library.count("stream_finish"), 1)
                self.assertEqual(library.count("result_free"), 1)
                self.assertEqual(final["sample_count"], count)
                self.assertEqual(final["text"], "完整句子")
                self.assertEqual(final["language"], "Chinese")
                self.assertEqual(runtime._offset, count + 5120)
                self.assertFalse(runtime._active)
                self.assertFalse(session.active)
                self.assertEqual(runtime.preview_text, "")
                request = next(args for name, args in library.calls if name == "request_set_text")
                self.assertEqual(request[1:], ("医学 热词".encode("utf-8"), b"Chinese"))
                with self.assertRaises(RuntimeError):
                    runtime.finish()
                self.assertEqual(runtime._offset, count + 5120)
                self.assertEqual(library.count("stream_finish"), 1)

    def test_empty_direct_and_controller_finish_add_no_context(self):
        runtime, library = self.runtime()
        runtime.start()
        runtime.finish()
        self.assertEqual(library.pushes, [])
        self.assertEqual(runtime._offset, 0)
        self.assertEqual(library.count("stream_finish"), 1)
        session = R2T2StreamSession(runtime)
        session.start(1)
        self.assertEqual(session.finish(1)["sample_count"], 0)
        self.assertEqual(library.count("stream_finish"), 1)
        self.assertEqual(runtime._offset, 0)

    def test_finish_failures_free_result_and_do_not_advance_context_or_read_stale_text(self):
        for failure in ("stream_finish", "result_text"):
            with self.subTest(failure=failure):
                runtime, library = self.runtime(fail=failure)
                runtime.start()
                runtime.feed(np.ones(37, dtype=np.float32))
                before = len(library.pushes)
                with self.assertRaises(RuntimeError):
                    runtime.finish()
                self.assertEqual(len(library.pushes), before)
                self.assertEqual(runtime._offset, 37)
                self.assertEqual(library.count("result_text"), int(failure == "result_text"))
                self.assertEqual(library.count("result_free"), 1)

    def test_controller_error_clears_active_state_before_next_start(self):
        runtime, library = self.runtime(fail="stream_finish")
        session = R2T2StreamSession(runtime)
        session.start(1)
        session.feed(1, np.ones(5, dtype=np.float32), offset=0)
        with self.assertRaises(RuntimeError):
            session.finish(1)
        self.assertFalse(session.active)
        self.assertFalse(runtime._active)
        self.assertEqual(runtime._offset, 0)
        self.assertEqual(runtime.preview_text, "")
        library.fail = None
        session.start(2)
        session.feed(2, np.ones(7, dtype=np.float32), offset=0)
        self.assertEqual(session.finish(2)["sample_count"], 7)
        self.assertEqual(runtime._offset, 7 + 5120)

    def test_cancel_and_restart_retain_negotiated_policy_without_old_audio(self):
        runtime, library = self.runtime()
        session = R2T2StreamSession(runtime)
        session.start(1)
        session.feed(1, np.ones(5, dtype=np.float32), offset=0)
        self.assertEqual(session.cancel(1)["sample_count"], 5)
        session.start(2)
        session.feed(2, np.ones(7, dtype=np.float32), offset=0)
        session.start(3)
        self.assertEqual(runtime._offset, 0)
        session.feed(3, np.ones(9, dtype=np.float32), offset=0)
        before = len(library.pushes)
        self.assertEqual(session.finish(3)["sample_count"], 9)
        self.assertEqual(len(library.pushes), before)
        self.assertEqual(runtime._offset, 9 + 5120)
        self.assertEqual(library.count("stream_reset"), 2)
        self.assertEqual(sum(name == "options_set" and args[1] == b"confucius4_r2t2.coalesced_finish"
                             for name, args in library.calls), 1)

    def test_session_create_failure_frees_all_allocated_owners(self):
        library = FinishLibrary(fail="session_create")
        with (
            patch.object(r2t2_native.ct, "CDLL", return_value=library),
            patch.object(r2t2_native.os, "add_dll_directory", return_value=Mock(), create=True),
        ):
            with self.assertRaises(RuntimeError):
                NativeRuntime("model.gguf", "audiocpp.dll", backend="cuda", rolling=True)
        for owner in ("options", "session", "model", "registry"):
            self.assertEqual(library.count(owner + "_free"), 1)
        self.assertEqual(library.count("stream_finish"), 0)


if __name__ == "__main__":
    unittest.main()
