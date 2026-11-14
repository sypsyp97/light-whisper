

import contextlib
import os
from pathlib import Path
import sys
from types import SimpleNamespace
import unittest
from unittest import mock

import numpy as np

sys.path.insert(0, os.path.dirname(__file__))

import qwen3_asr_server
from qwen3_native import QwenNativeRuntime
from r2t2_native import NativeRuntime
from test_qwen3_native import FakeLibrary


class QwenCpuCudaSelectionTests(unittest.TestCase):


    PATHS = ("qwen3-asr-0.6b-q8_0.gguf", "Qwen3-ASR-0.6B-Q8_0.gguf", "resolved-current.gguf")

    def setUp(self):
        server = qwen3_asr_server.Qwen3ASRServer
        self.enterContext(mock.patch("signal.signal"))
        self.enterContext(mock.patch.object(server, "_setup_runtime_environment"))
        self.detect = self.enterContext(mock.patch.object(server, "_detect_device"))
        self.enterContext(mock.patch.object(
            server, "_get_gpu_device_info", autospec=True,
            side_effect=lambda owner: {"device": owner.device},
        ))
        self.enterContext(mock.patch.object(qwen3_asr_server, "logger"))
        self.native = SimpleNamespace(close=mock.Mock())
        self.native_factory = self.enterContext(mock.patch.object(
            qwen3_asr_server, "QwenNativeRuntime", return_value=self.native
        ))
        self.enterContext(mock.patch.dict(sys.modules, {
            "transcribe_cpp": None, "transcribe_cpp_native": None,
            "transcribe_cpp_native_cu12": None,
        }))

    def server(self, detected):
        self.detect.return_value = detected
        server = qwen3_asr_server.Qwen3ASRServer(engine="qwen3-asr-0.6b")
        server.stdout_suppressor = SimpleNamespace(suppress=contextlib.nullcontext)
        self.addCleanup(server._close_runtime)
        return server

    def assert_selection(self, server, filename, expected):
        self.assertEqual([call.kwargs["backend"] for call in self.native_factory.call_args_list], expected)
        self.assertEqual((server.device, server.backend), (expected[-1], expected[-1]))
        self.assertIs(server.model, self.native)
        self.assertIs(server.session, self.native)
        status = server.check_status()
        self.assertTrue(status["success"])
        self.assertTrue(status["model_loaded"])
        self.assertEqual((status["device"], status["backend"]), (expected[-1], expected[-1]))
        resources = Path(qwen3_asr_server.__file__).resolve().parent
        for call, backend in zip(self.native_factory.call_args_list, expected):
            self.assertEqual(call.args[0], filename)
            self.assertEqual(Path(call.args[1]), resources / "r2t2-native" / backend / "audiocpp.dll")
            self.assertIn(resources, [Path(path) for path in call.kwargs["dll_directories"]])

    def load_native(self, server, filename):
        try:
            server._load_runtime(filename)
        except ImportError as error:
            self.fail(f"Supported Qwen path depended on blocked legacy import: {error}")

    def test_supported_resolved_paths_use_only_shared_cpu_cuda_runtime(self):
        for filename in self.PATHS:
            for detected, failures, expected in (
                ("cuda", 0, ["cuda"]), ("cuda", 1, ["cuda", "vulkan"]),
                ("cuda", 2, ["cuda", "vulkan", "cpu"]),
                ("vulkan", 0, ["vulkan"]), ("vulkan", 1, ["vulkan", "cpu"]),
                ("cpu", 0, ["cpu"]),
            ):
                with self.subTest(filename=filename, detected=detected, failures=failures):
                    self.native_factory.reset_mock(side_effect=True)
                    self.native_factory.side_effect = [RuntimeError("backend unavailable")] * failures + [self.native]
                    server = self.server(detected)
                    self.load_native(server, filename)
                    self.assert_selection(server, filename, expected)

    def test_exhausted_backends_clear_owner_without_importing_legacy_fallback(self):
        for filename in self.PATHS:
            for detected, expected in (
                ("cuda", ["cuda", "vulkan", "cpu"]),
                ("vulkan", ["vulkan", "cpu"]),
                ("cpu", ["cpu"]),
            ):
                with self.subTest(filename=filename, detected=detected):
                    self.native_factory.reset_mock(side_effect=True)
                    self.native_factory.side_effect = RuntimeError("backend unavailable")
                    server = self.server(detected)
                    with self.assertRaises(RuntimeError) as failure:
                        self.load_native(server, filename)
                    self.assertEqual([call.kwargs["backend"] for call in self.native_factory.call_args_list], expected)
                    for backend in expected:
                        self.assertIn(backend, str(failure.exception))
                    self.assertNotIn("auto", str(failure.exception).casefold())
                    self.assertIsNone(server.model)
                    self.assertIsNone(server.session)
                    self.assertIsNone(server.stream)
                    self.assertFalse(server.initialized)
                    self.assertFalse(server.check_status()["model_loaded"])


class CpuCudaAbiTests(unittest.TestCase):
    def setUp(self):
        self.library = FakeLibrary()
        self.load_library = self.enterContext(mock.patch("ctypes.CDLL", return_value=self.library))
        self.add_directory = self.enterContext(mock.patch("os.add_dll_directory", return_value=mock.Mock(), create=True))

    def test_retired_and_implicit_backends_reject_all_families_and_modes_before_dll_load(self):
        for backend in ("auto", "metal", "", None):
            for family in ("qwen3_asr", "confucius4_r2t2", "unknown"):
                for mode in ("offline", "streaming", "unknown"):
                    with self.subTest(backend=backend, family=family, mode=mode), self.assertRaises(ValueError):
                        NativeRuntime("model.gguf", "audio.dll", backend=backend, family=family, mode=mode)
        self.load_library.assert_not_called()
        self.add_directory.assert_not_called()

    def test_qwen_accepts_vulkan_backend_and_loads_the_dll(self):
        runtime = QwenNativeRuntime("qwen.gguf", "audio.dll", backend="vulkan")
        self.addCleanup(runtime.close)
        self.assertEqual(runtime.backend, "vulkan")
        self.load_library.assert_called_once()
        self.add_directory.assert_called_once()

    def test_cpu_cuda_qwen_preserve_c_abi_exact_audio_tail_options_output_and_ownership(self):
        pcm = np.linspace(-0.8, 0.8, 12813, dtype=np.float32)
        for backend in ("cpu", "cuda"):
            with self.subTest(backend=backend):
                runtime = QwenNativeRuntime("qwen.gguf", "audio.dll", backend=backend, threads=3)
                self.addCleanup(runtime.close)
                result = runtime.run(pcm, context="保留上下文", language="zh")
                self.assertEqual(self.library.model_loads[-1],
                                 (str(Path("qwen.gguf").resolve()).encode("utf-8"), b"qwen3_asr"))
                self.assertEqual(self.library.sessions[-1], (b"asr", b"offline", backend.encode(), 0, 3))
                request = self.library.requests[-1]
                np.testing.assert_array_equal(request["pcm"], pcm)
                self.assertEqual(len(request["pcm"]), 12813)
                self.assertEqual(request["format"], (16000, 1))
                self.assertEqual(request["context_language"], ("保留上下文".encode(), b"zh"))
                self.assertEqual(request["options"], {
                    b"max_tokens": b"512", b"return_timestamps": b"false", b"audio_chunk_mode": b"none",
                })
                self.assertEqual((result.text, result.language), self.library.output)
                runtime.close()
                runtime.close()
        self.assertCountEqual(self.library.freed, [
            (kind, handle) for handle, kind in self.library.allocated.items()
        ])

    def test_r2t2_cpu_cuda_sessions_keep_family_backend_and_streaming_mode(self):
        for backend in ("cpu", "cuda"):
            with self.subTest(backend=backend):
                runtime = NativeRuntime("r2t2.gguf", "audio.dll", backend=backend)
                self.addCleanup(runtime.close)
                self.assertEqual(self.library.model_loads[-1][1], b"confucius4_r2t2")
                self.assertEqual(self.library.sessions[-1], (b"asr", b"streaming", backend.encode(), 0, 4))
                runtime.close()


if __name__ == "__main__":
    unittest.main()
