"""Measured constructor guards and cleanup; native allocations remain synthetic."""

from contextlib import contextmanager
import os
from pathlib import Path
import sys
from types import SimpleNamespace
import unittest
from unittest import mock

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))

import r2t2_native
from qwen3_native import QwenNativeRuntime
from r2t2_native import NativeRuntime
from test_qwen3_native import FakeLibrary


class ConstructorLibrary(FakeLibrary):
    """Reuse native ownership fixtures; snapshot the options given to each session."""

    def __init__(self, **kwargs):
        super().__init__(**kwargs)
        self.options = {}
        self.session_options = []
        self.pushes = []

    def invoke(self, name, *args):
        if name == "options_create":
            handle = super().invoke(name, *args)
            self.options[handle] = {}
            return handle
        if name == "options_set":
            self.options[self.handle(args[0])][args[1]] = args[2]
        elif name == "session_create":
            self.session_options.append(dict(self.options[self.handle(args[-2])]))
        elif name == "stream_push":
            self.pushes.append(np.ctypeslib.as_array(args[1], shape=(args[2],)).copy())
        elif name == "stream_finish":
            self.allocate("result", args[-1])
        return super().invoke(name, *args)


class NativeConstructorGapTests(unittest.TestCase):
    @contextmanager
    def constructed(self, runtime_type=NativeRuntime, *, library=None, args=None, **arguments):
        library = library if library is not None else ConstructorLibrary(abi=0x0500)
        args = ("synthetic.gguf", "synthetic.dll") if args is None else args
        directories = []

        def add_directory(_path):
            directory = mock.Mock()
            directories.append(directory)
            return directory

        runtime = None
        with mock.patch.dict(os.environ, {"LIGHT_WHISPER_NATIVE_THREADS": "2"}), \
             mock.patch("ctypes.CDLL", return_value=library), \
             mock.patch("os.add_dll_directory", side_effect=add_directory, create=True):
            try:
                runtime = runtime_type(*args, **arguments)
                yield runtime, library
            finally:
                if runtime is not None:
                    runtime.close()
                    runtime.close()
                self.assertCountEqual(library.freed,
                                      [(kind, handle) for handle, kind in library.allocated.items()])
                for directory in directories:
                    directory.close.assert_called_once()

    def test_keyword_model_and_library_paths_reach_both_native_runtimes(self):
        for runtime_type, family, mode in ((NativeRuntime, b"confucius4_r2t2", b"streaming"),
                                          (QwenNativeRuntime, b"qwen3_asr", b"offline")):
            with self.subTest(runtime=runtime_type.__name__):
                try:
                    with self.constructed(runtime_type, args=(), model_path="synthetic.gguf",
                                          library_path="synthetic.dll", backend="cpu") as (_, library):
                        self.assertEqual(library.model_loads, [
                            (str(Path("synthetic.gguf").resolve()).encode("utf-8"), family),
                        ])
                        self.assertEqual(library.sessions, [(b"asr", mode, b"cpu", 0, 2)])
                except TypeError as error:
                    self.fail(f"Public constructor rejected keyword paths: {error}")

    def test_backend_is_keyword_only_before_native_allocation(self):
        for runtime_type in (NativeRuntime, QwenNativeRuntime):
            with self.subTest(runtime=runtime_type.__name__):
                runtime, error = None, None
                library = ConstructorLibrary()
                with mock.patch("ctypes.CDLL", return_value=library) as load, \
                     mock.patch("os.add_dll_directory", create=True) as add:
                    try:
                        runtime = runtime_type("synthetic.gguf", "synthetic.dll", "cpu", threads=2)
                    except TypeError as caught:
                        error = caught
                    finally:
                        if runtime is not None:
                            runtime.close()
                self.assertIsInstance(error, TypeError)
                load.assert_not_called()
                add.assert_not_called()
                self.assertEqual(library.calls, [])

    def test_omitted_chunk_duration_is_320_ms_at_public_and_native_boundaries(self):
        with self.constructed(backend="cpu") as (runtime, library):
            self.assertEqual(runtime.chunk_samples, 5120)
            self.assertEqual(library.session_options, [{b"confucius4_r2t2.chunk_size_ms": b"320"}])

    def test_rolling_default_and_explicit_controls_reach_session_options(self):
        chunk = {b"confucius4_r2t2.chunk_size_ms": b"320"}
        rolling_options = {b"confucius4_r2t2.rolling_window": b"true",
                           b"confucius4_r2t2.rollback_punctuation": b"true"}
        for abi in (0x0400, 0x0500):
            for arguments, rolling in (({}, False), ({"rolling": False}, False),
                                       ({"rolling": True}, True)):
                with self.subTest(abi=hex(abi), arguments=arguments):
                    library = ConstructorLibrary(abi=abi)
                    with self.constructed(library=library, backend="cuda", **arguments) as (runtime, _):
                        self.assertIs(runtime.rolling, rolling)
                        expected = dict(chunk)
                        if rolling:
                            expected.update(rolling_options)
                            if abi >= 0x0500:
                                expected[b"confucius4_r2t2.coalesced_finish"] = b"true"
                        self.assertEqual(library.session_options, [expected])

    def test_unsupported_backend_errors_identify_family_before_allocation(self):
        dynamic_family = b"confucius4_r2t2".decode()
        cases = ((NativeRuntime, {}, "R2T2"),
                 (NativeRuntime, {"family": dynamic_family}, "R2T2"),
                 (QwenNativeRuntime, {}, "Qwen3-ASR"))
        for runtime_type, arguments, family in cases:
            with self.subTest(runtime=runtime_type.__name__, arguments=arguments):
                library = ConstructorLibrary()
                with mock.patch("ctypes.CDLL", return_value=library) as load, \
                     mock.patch("os.add_dll_directory", create=True) as add:
                    with self.assertRaises(ValueError) as caught:
                        runtime_type("synthetic.gguf", "synthetic.dll", backend="vulkan", **arguments)
                self.assertIn(family, str(caught.exception))
                self.assertIn("backend", str(caught.exception))
                load.assert_not_called()
                add.assert_not_called()
                self.assertEqual(library.calls, [])

    def test_native_constructor_failures_keep_family_and_release_partial_owners(self):
        for runtime_type, arguments, family in ((NativeRuntime, {}, "R2T2"),
                                               (NativeRuntime, {"family": b"confucius4_r2t2".decode()}, "R2T2"),
                                               (QwenNativeRuntime, {}, "Qwen3-ASR")):
            with self.subTest(runtime=runtime_type.__name__, arguments=arguments):
                library = ConstructorLibrary(fail_at="registry_create")
                with self.assertRaises(RuntimeError) as caught:
                    with self.constructed(runtime_type, library=library, backend="cpu", **arguments):
                        self.fail("Injected registry failure allowed construction")
                self.assertIn(family, str(caught.exception))
                self.assertIn("registry_create", str(caught.exception))
                self.assertIn("injected native failure", str(caught.exception))
                self.assertEqual(set(library.allocated.values()), {"registry"})
                self.assertEqual(library.sessions, [])

    def test_fresh_stream_rejects_feed_and_finish_and_reset_skips_native_reset(self):
        for action in ("feed", "finish", "reset"):
            with self.subTest(action=action):
                with self.constructed(backend="cpu") as (runtime, library):
                    if action == "reset":
                        runtime.reset()
                    else:
                        with self.assertRaisesRegex(RuntimeError, "No active R2T2 stream"):
                            if action == "feed":
                                runtime.feed(np.ones(37, dtype=np.float32))
                            else:
                                runtime.finish()
                    for operation in ("stream_push", "stream_finish", "stream_reset"):
                        self.assertNotIn(operation, library.calls)
        with self.constructed(backend="cpu") as (runtime, library):
            pcm = np.ones(37, dtype=np.float32)
            runtime.start()
            runtime.feed(pcm)
            self.assertEqual(library.calls.count("stream_start"), 1)
            self.assertEqual(len(library.pushes), 1)
            np.testing.assert_array_equal(library.pushes[0], pcm)
            runtime.reset()
            self.assertEqual(library.calls.count("stream_reset"), 1)

    def test_equal_string_values_negotiate_same_native_options_and_finish_policy(self):
        defaults = {"family": "confucius4_r2t2", "backend": "cuda", "mode": "streaming"}
        cases = [("literal", 0x0500, {}, True)]
        for name, literal in defaults.items():
            dynamic = literal.encode().decode()
            self.assertEqual(dynamic, literal)
            self.assertIsNot(dynamic, literal)
            cases.append((f"dynamic {name}", 0x0500, {name: dynamic}, True))
        cases.extend((("old ABI", 0x0400, {}, False),
                      ("CPU", 0x0500, {"backend": "cpu"}, False),
                      ("Qwen family", 0x0500, {"family": "qwen3_asr"}, False),
                      ("offline", 0x0500, {"mode": "offline"}, False)))
        for label, abi, arguments, coalesced in cases:
            with self.subTest(case=label):
                options = dict(defaults, **arguments)
                library = ConstructorLibrary(abi=abi)
                with self.constructed(library=library, rolling=True, chunk_ms=160,
                                      **options) as (runtime, _):
                    expected = {}
                    if options["family"] == "confucius4_r2t2":
                        expected = {b"confucius4_r2t2.chunk_size_ms": b"160",
                                    b"confucius4_r2t2.rolling_window": b"true",
                                    b"confucius4_r2t2.rollback_punctuation": b"true"}
                        if coalesced:
                            expected[b"confucius4_r2t2.coalesced_finish"] = b"true"
                    self.assertEqual(library.session_options, [expected])
                    self.assertEqual(library.model_loads[0][1], options["family"].encode())
                    self.assertEqual(library.sessions, [
                        (b"asr", options["mode"].encode(), options["backend"].encode(), 0, 2),
                    ])
                    runtime.start()
                    runtime.feed(np.ones(37, dtype=np.float32))
                    before_finish = len(library.pushes)
                    self.assertEqual(runtime.finish(), library.output)
                    padding = library.pushes[before_finish:]
                    self.assertEqual(sum(pcm.size for pcm in padding), 0 if coalesced else 5120)
                    for pcm in padding:
                        np.testing.assert_array_equal(pcm, np.zeros(pcm.size, dtype=np.float32))
                    self.assertEqual(library.calls.count("stream_finish"), 1)

    def assert_invalid(self, arguments):
        runtime, error = None, None
        with mock.patch("ctypes.CDLL", return_value=FakeLibrary()) as load, \
             mock.patch("os.add_dll_directory", create=True) as add:
            try:
                runtime = NativeRuntime("synthetic.gguf", "synthetic.dll", backend="cpu", **arguments)
            except Exception as caught:
                error = caught
            finally:
                if runtime is not None:
                    runtime.close()
        self.assertIsInstance(error, ValueError)
        self.assertTrue(str(error))
        load.assert_not_called()
        add.assert_not_called()

    def test_nonboolean_rolling_is_rejected_before_native_allocation(self):
        for value in (0, 1, None, "false", [], {}):
            with self.subTest(value=value):
                self.assert_invalid({"rolling": value})

    def test_invalid_chunk_duration_is_rejected_before_native_allocation(self):
        for value in (None, True, False, 79, 2001, 0, -1, 1.5, "320", [], {}):
            with self.subTest(value=value):
                self.assert_invalid({"chunk_ms": value})

    def test_chunk_duration_boundaries_reach_native_options_and_sample_count(self):
        for duration in (80, 2000):
            with self.subTest(duration=duration):
                library = FakeLibrary()
                with mock.patch("ctypes.CDLL", return_value=library), \
                     mock.patch("os.add_dll_directory", create=True):
                    runtime = NativeRuntime("synthetic.gguf", "synthetic.dll",
                                            backend="cpu", chunk_ms=duration)
                    try:
                        self.assertEqual(runtime.chunk_samples, duration * 16)
                        library.audiocpp_options_set.assert_any_call(
                            mock.ANY, b"confucius4_r2t2.chunk_size_ms", str(duration).encode())
                    finally:
                        runtime.close()
                self.assertCountEqual(library.freed,
                                      [(kind, handle) for handle, kind in library.allocated.items()])

    def test_null_options_allocation_frees_existing_native_and_dll_owners(self):
        for runtime_type in (NativeRuntime, QwenNativeRuntime):
            with self.subTest(runtime=runtime_type.__name__):
                library = FakeLibrary()
                library.audiocpp_options_create.side_effect = lambda: None
                dll = mock.Mock()
                runtime, error = None, None
                with mock.patch("ctypes.CDLL", return_value=library), \
                     mock.patch("os.add_dll_directory", return_value=dll, create=True):
                    try:
                        runtime = runtime_type("synthetic.gguf", "synthetic.dll", backend="cpu")
                    except Exception as caught:
                        error = caught
                    finally:
                        if runtime is not None:
                            runtime.close()
                self.assertIsInstance(error, RuntimeError)
                self.assertTrue(str(error))
                self.assertEqual(library.sessions, [])
                self.assertEqual(set(library.allocated.values()), {"registry", "model"})
                self.assertCountEqual(library.freed,
                                      [(kind, handle) for handle, kind in library.allocated.items()])
                if os.name == "nt":
                    dll.close.assert_called_once()

    def test_non_windows_constructor_policy_skips_dll_directories(self):
        # Local OS-decision simulation; this does not execute on a POSIX host.
        library = FakeLibrary()
        add = mock.Mock(return_value=mock.Mock())
        facade = SimpleNamespace(name="posix", environ={"LIGHT_WHISPER_NATIVE_THREADS": "2"},
                                 add_dll_directory=add)
        original_name = os.name
        with mock.patch.object(r2t2_native, "os", facade), \
             mock.patch("ctypes.CDLL", return_value=library):
            runtime = NativeRuntime("synthetic.gguf", "synthetic.dll", backend="cpu",
                                    dll_directories=("synthetic-dependencies",))
            try:
                self.assertEqual(library.sessions, [(b"asr", b"streaming", b"cpu", 0, 2)])
            finally:
                runtime.close()
        add.assert_not_called()
        self.assertEqual(os.name, original_name)
        self.assertCountEqual(library.freed,
                              [(kind, handle) for handle, kind in library.allocated.items()])


if __name__ == "__main__":
    unittest.main()
