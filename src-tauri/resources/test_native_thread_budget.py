"""Thread selection at the real SDK constructors and ctypes backend ABI boundary."""

import os
from pathlib import Path
import sys
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))

from qwen3_native import QwenNativeRuntime
from r2t2_native import NativeRuntime
from test_qwen3_native import FakeLibrary


THREAD_ENV = "LIGHT_WHISPER_NATIVE_THREADS"
MAX_THREADS = 2 ** 31 - 1  # _BackendConfig.threads is a signed 32-bit ct.c_int.
RUNTIMES = ((NativeRuntime, b"streaming"), (QwenNativeRuntime, b"offline"))


class NativeThreadBudgetTests(unittest.TestCase):
    def construct(self, runtime_type, backend, environment, arguments):
        library = FakeLibrary()
        with mock.patch.dict(os.environ, environment, clear=True), \
             mock.patch("ctypes.CDLL", return_value=library), \
             mock.patch("os.add_dll_directory", create=True):
            try:
                runtime = runtime_type("synthetic-model.gguf", "synthetic-audio.dll",
                                       backend=backend, **arguments)
            except Exception as error:
                self.fail(f"valid thread selection raised {type(error).__name__}: {error}")
            try:
                sessions = list(library.sessions)
            finally:
                runtime.close()
        self.assertCountEqual(library.freed, [
            (kind, handle) for handle, kind in library.allocated.items()
        ])
        return sessions

    def test_selected_budget_reaches_actual_backend_config_for_both_model_kinds(self):
        cases = [
            ({}, {}, 4),
            ({"OMP_NUM_THREADS": "19"}, {}, 4),
            ({"OMP_NUM_THREADS": "19"}, {"threads": None}, 4),
            ({THREAD_ENV: "2", "OMP_NUM_THREADS": "19"}, {}, 2),
            ({THREAD_ENV: "2"}, {"threads": None}, 2),
            ({THREAD_ENV: "1"}, {}, 1),
            ({THREAD_ENV: str(MAX_THREADS)}, {}, MAX_THREADS),
            ({THREAD_ENV: "2"}, {"threads": 3}, 3),
            ({THREAD_ENV: "invalid"}, {"threads": 3}, 3),
            ({THREAD_ENV: str(MAX_THREADS + 1)}, {"threads": 3}, 3),
            ({THREAD_ENV: "0"}, {"threads": 1}, 1),
            ({THREAD_ENV: "0"}, {"threads": MAX_THREADS}, MAX_THREADS),
        ]
        for runtime_type, mode in RUNTIMES:
            for backend in ("cpu", "cuda"):
                for environment, arguments, expected in cases:
                    with self.subTest(runtime=runtime_type.__name__, backend=backend,
                                      environment=environment, arguments=arguments):
                        self.assertEqual(
                            self.construct(runtime_type, backend, environment, arguments),
                            [(b"asr", mode, backend.encode(), 0, expected)],
                        )

    def assert_rejected_before_native_allocation(self, runtime_type, backend,
                                                environment, arguments):
        library = FakeLibrary()
        runtime, failure = None, None
        with mock.patch.dict(os.environ, environment, clear=True), \
             mock.patch("ctypes.CDLL", return_value=library) as load, \
             mock.patch("os.add_dll_directory", create=True) as directory:
            try:
                runtime = runtime_type("synthetic-model.gguf", "synthetic-audio.dll",
                                       backend=backend, **arguments)
            except Exception as error:
                failure = error
            finally:
                if runtime is not None:
                    runtime.close()
        self.assertIsInstance(failure, ValueError,
                              "invalid thread budget must raise a clear validation error")
        self.assertTrue(str(failure))
        load.assert_not_called()
        directory.assert_not_called()
        self.assertEqual(library.calls, [])
        self.assertEqual(library.allocated, {})

    def test_invalid_environment_is_rejected_before_loading_either_native_backend(self):
        invalid = ("", "0", "-1", "1.5", "text", "True", "nan",
                   str(MAX_THREADS + 1), str(2 ** 32), "9" * 100)
        for runtime_type, _mode in RUNTIMES:
            for backend in ("cpu", "cuda"):
                for value in invalid:
                    for arguments in ({}, {"threads": None}):
                        with self.subTest(runtime=runtime_type.__name__, backend=backend,
                                          environment=value, arguments=arguments):
                            self.assert_rejected_before_native_allocation(
                                runtime_type, backend, {THREAD_ENV: value}, arguments,
                            )

    def test_invalid_explicit_budget_cannot_be_hidden_by_valid_environment(self):
        invalid = (0, -1, MAX_THREADS + 1, 2 ** 32, 10 ** 100,
                   True, False, 1.5, "2", "text", [], {})
        for runtime_type, _mode in RUNTIMES:
            for backend in ("cpu", "cuda"):
                for value in invalid:
                    with self.subTest(runtime=runtime_type.__name__, backend=backend,
                                      argument=value):
                        self.assert_rejected_before_native_allocation(
                            runtime_type, backend, {THREAD_ENV: "2"}, {"threads": value},
                        )


if __name__ == "__main__":
    unittest.main()
