import os
import subprocess
import sys
import unittest
from unittest import mock

sys.path.insert(0, os.path.dirname(__file__))

import qwen3_asr_server


class QwenGpuMetadataTests(unittest.TestCase):
    def setUp(self):
        self.detect_device = self.enterContext(mock.patch.object(
            qwen3_asr_server.Qwen3ASRServer, "_detect_device", return_value="cuda"
        ))
        self.enterContext(mock.patch.object(
            qwen3_asr_server.Qwen3ASRServer, "_resolve_model_path",
            return_value="qwen3-asr-0.6b-q8_0.gguf",
        ))
        self.enterContext(mock.patch.object(
            qwen3_asr_server.Qwen3ASRServer, "_warmup_inference"
        ))
        self.native_factory = self.enterContext(mock.patch.object(
            qwen3_asr_server, "QwenNativeRuntime", side_effect=lambda *args, **kwargs: mock.Mock()
        ))
        self.vad_factory = self.enterContext(mock.patch.object(
            qwen3_asr_server, "FireRedVad", side_effect=lambda: mock.Mock()
        ))
        self.enterContext(mock.patch.object(qwen3_asr_server, "QwenPreviewRuntime"))
        # Device detection and optional metadata must never reach host GPU tools.
        self.query = self.enterContext(mock.patch.object(
            subprocess, "run",
            return_value=self.probe_result("First GPU, 24576\n"),
        ))

    @staticmethod
    def probe_result(stdout):
        return subprocess.CompletedProcess(["nvidia-smi"], 0, stdout=stdout, stderr="")

    def make_server(self, device="cuda"):
        self.detect_device.return_value = device
        server = qwen3_asr_server.Qwen3ASRServer(engine="qwen3-asr-0.6b")
        self.addCleanup(server._close_runtime)
        return server

    def assert_cuda_metadata(self, result, name="First GPU", memory=24.0):
        self.assertTrue(result["success"])
        self.assertEqual(result["device"], "cuda")
        self.assertEqual(result["gpu_name"], name)
        self.assertEqual(result["gpu_memory_total"], memory)

    def assert_cpu_metadata(self, result):
        self.assertTrue(result["success"])
        self.assertEqual(result["backend"], "cpu")
        self.assertEqual(result["device"], "cpu")
        self.assertNotIn("gpu_name", result)
        self.assertNotIn("gpu_memory_total", result)

    def test_repeated_initialize_and_status_share_one_bounded_probe(self):
        server = self.make_server()
        self.assert_cuda_metadata(server.initialize())
        runtime = server.model
        for _ in range(3):
            self.assertTrue(server.initialize()["success"])
            status = server.check_status()
            self.assert_cuda_metadata(status)
            self.assertTrue(status["initialized"])
            self.assertTrue(status["model_loaded"])
            self.assertIs(server.model, runtime)
        self.native_factory.assert_called_once()
        timeout = self.query.call_args.kwargs["timeout"]
        self.assertGreater(timeout, 0)
        self.assertLessEqual(timeout, 3)
        self.assertEqual(self.query.call_count, 1)

    def test_optional_probe_failures_and_malformed_output_are_cached(self):
        outcomes = (
            ("missing-tool", FileNotFoundError("nvidia-smi")),
            ("timeout", subprocess.TimeoutExpired("nvidia-smi", 3)),
            ("empty", self.probe_result("")),
            ("missing-column", self.probe_result("not-csv\n")),
            ("invalid-memory", self.probe_result("First GPU, not-a-number\n")),
        )
        for label, outcome in outcomes:
            with self.subTest(outcome=label):
                self.query.reset_mock(side_effect=True)
                if isinstance(outcome, Exception):
                    self.query.side_effect = outcome
                else:
                    self.query.return_value = outcome
                server = self.make_server()
                initialized = server.initialize()
                self.assertTrue(initialized["success"])
                runtime = server.model
                metadata = {
                    key: initialized[key]
                    for key in ("device", "gpu_name", "gpu_memory_total")
                    if key in initialized
                }
                for _ in range(3):
                    self.assertTrue(server.initialize()["success"])
                    status = server.check_status()
                    self.assertTrue(status["success"])
                    self.assertTrue(status["initialized"])
                    self.assertTrue(status["model_loaded"])
                    self.assertEqual({key: status[key] for key in metadata}, metadata)
                    self.assertIs(server.model, runtime)
                    self.assertIs(server.session, runtime)
                self.assertEqual(self.query.call_count, 1)

    def test_cpu_runtime_never_queries_gpu_metadata(self):
        server = self.make_server(device="cpu")
        self.assert_cpu_metadata(server.initialize())
        for _ in range(3):
            self.assertTrue(server.initialize()["success"])
            self.assert_cpu_metadata(server.check_status())
        self.query.assert_not_called()
        self.assertEqual(self.native_factory.call_args.kwargs["backend"], "cpu")

    def test_failed_probe_does_not_publish_stdout_as_gpu_metadata(self):
        self.query.return_value = subprocess.CompletedProcess(
            ["nvidia-smi"], 1, stdout="Driver diagnostic, 24576\n", stderr="failed"
        )
        server = self.make_server()
        initialized = server.initialize()
        status = server.check_status()
        for result in (initialized, status):
            with self.subTest(response="status" if result is status else "initialize"):
                self.assertTrue(result["success"])
                self.assertEqual(result["device"], "cuda")
                self.assertNotIn("gpu_name", result)
                self.assertNotIn("gpu_memory_total", result)

    def test_metadata_results_cannot_be_poisoned_by_callers(self):
        server = self.make_server()
        initialized = server.initialize()
        self.assert_cuda_metadata(initialized)
        initialized.update(device="cpu", gpu_name="caller mutation", gpu_memory_total=-1)
        status = server.check_status()
        self.assert_cuda_metadata(status)
        status.update(device="cpu", gpu_name="second mutation", gpu_memory_total=-2)
        self.assert_cuda_metadata(server.check_status())

    def test_reloaded_runtime_reprobes_metadata_once(self):
        server = self.make_server()
        self.assert_cuda_metadata(server.initialize())
        old_runtime = server.model
        server._close_runtime()
        self.query.return_value = self.probe_result("Reloaded GPU, 12288\n")
        server._load_runtime("replacement-q8_0.gguf")
        self.assertIsNot(server.model, old_runtime)
        old_runtime.close.assert_called_once()
        for _ in range(2):
            status = server.check_status()
            self.assert_cuda_metadata(status, "Reloaded GPU", 12.0)
            self.assertTrue(status["model_loaded"])
        self.assertEqual(self.native_factory.call_count, 2)
        self.assertEqual(self.query.call_count, 2)

    def test_reload_can_recover_from_cached_probe_failure(self):
        server = self.make_server()
        self.query.side_effect = FileNotFoundError("nvidia-smi")
        self.assertTrue(server.initialize()["success"])
        server._close_runtime()
        self.query.side_effect = None
        self.query.return_value = self.probe_result("Recovered GPU, 8192\n")
        self.assert_cuda_metadata(server.initialize(), "Recovered GPU", 8.0)
        self.assert_cuda_metadata(server.check_status(), "Recovered GPU", 8.0)
        self.assertEqual(self.query.call_count, 2)

    def test_failed_load_attempt_does_not_reuse_previous_metadata(self):
        server = self.make_server()
        self.assert_cuda_metadata(server.initialize())
        server._close_runtime()
        self.native_factory.side_effect = RuntimeError("runtime unavailable")
        self.query.return_value = self.probe_result("Current GPU, 8192\n")
        with self.assertRaises(RuntimeError):
            server._load_runtime("unloadable-q8_0.gguf")
        status = server.check_status()
        self.assertFalse(status["model_loaded"])
        self.assertFalse(status["initialized"])
        self.assertNotEqual(status.get("gpu_name"), "First GPU")

    def test_cuda_to_cpu_fallback_discards_previous_gpu_metadata(self):
        server = self.make_server()
        self.assert_cuda_metadata(server.initialize())
        server._close_runtime()

        def load_with_cpu_fallback(*args, backend, **kwargs):
            if backend == "cuda":
                raise RuntimeError("CUDA unavailable")
            return mock.Mock()

        self.native_factory.side_effect = load_with_cpu_fallback
        self.assert_cpu_metadata(server.initialize())
        self.assert_cpu_metadata(server.check_status())
        self.assertEqual(
            [call.kwargs["backend"] for call in self.native_factory.call_args_list],
            ["cuda", "cuda", "cpu"],
        )
        self.assertEqual(self.query.call_count, 1)

    def test_idle_resume_probes_metadata_for_new_loaded_runtime(self):
        server = self.make_server()
        self.assert_cuda_metadata(server.initialize())
        old_runtime = server.model
        vad = server.vad_model
        self.assertTrue(server._suspend_gpu_runtime())
        old_runtime.close.assert_called_once()
        self.assertFalse(server.initialized)
        self.assertIsNone(server.model)
        self.assertIs(server.vad_model, vad)
        self.query.return_value = self.probe_result("Resumed GPU, 12288\n")
        self.assert_cuda_metadata(server.initialize(), "Resumed GPU", 12.0)
        self.assertIsNot(server.model, old_runtime)
        self.assertIs(server.vad_model, vad)
        self.vad_factory.assert_called_once_with()
        for _ in range(2):
            self.assert_cuda_metadata(server.check_status(), "Resumed GPU", 12.0)
        self.assertEqual(self.query.call_count, 2)

    def test_separate_loaded_runtimes_keep_independent_metadata(self):
        first = self.make_server()
        self.assert_cuda_metadata(first.initialize())
        self.query.return_value = self.probe_result("Second GPU, 8192\n")
        second = self.make_server()
        self.assert_cuda_metadata(second.initialize(), "Second GPU", 8.0)
        self.assert_cuda_metadata(first.check_status())
        self.assert_cuda_metadata(second.check_status(), "Second GPU", 8.0)
        self.assertEqual(self.query.call_count, 2)

    def test_initialize_replaces_metadata_polled_before_first_load(self):
        server = self.make_server()
        unloaded = server.check_status()
        self.assert_cuda_metadata(unloaded)
        self.assertFalse(unloaded["model_loaded"])
        self.assertFalse(unloaded["initialized"])
        self.assertEqual(self.query.call_count, 1)
        self.query.return_value = self.probe_result("Loaded GPU, 12288\n")
        self.assert_cuda_metadata(server.initialize(), "Loaded GPU", 12.0)
        self.assertEqual(self.query.call_count, 2)
        for _ in range(2):
            self.assert_cuda_metadata(server.check_status(), "Loaded GPU", 12.0)
        self.assertEqual(self.query.call_count, 2)

    def test_idle_resume_replaces_metadata_polled_while_unloaded(self):
        server = self.make_server()
        self.assert_cuda_metadata(server.initialize())
        self.assertTrue(server._suspend_gpu_runtime())
        self.query.return_value = self.probe_result("Unloaded GPU, 4096\n")
        unloaded = server.check_status()
        self.assertFalse(unloaded["model_loaded"])
        self.assertFalse(unloaded["initialized"])
        probes_before_load = self.query.call_count
        self.query.return_value = self.probe_result("Resumed GPU, 8192\n")
        self.assert_cuda_metadata(server.initialize(), "Resumed GPU", 8.0)
        self.assertEqual(self.query.call_count, probes_before_load + 1)
        for _ in range(2):
            self.assert_cuda_metadata(server.check_status(), "Resumed GPU", 8.0)
        self.assertEqual(self.query.call_count, probes_before_load + 1)


if __name__ == "__main__":
    unittest.main()
