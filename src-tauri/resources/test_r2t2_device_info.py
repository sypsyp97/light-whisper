"""Device badges use real hardware metadata without loading PyTorch."""
import subprocess
import unittest
from unittest.mock import patch

from r2t2_asr_server import R2T2ASRServer


def server(backend="cuda"):
    instance = R2T2ASRServer.__new__(R2T2ASRServer)
    instance.device = instance.backend = backend
    instance.engine = "confucius4-r2t2"
    instance.native = object()
    instance.vad_model = object()
    instance.initialized = True
    return instance


class DeviceInfoTests(unittest.TestCase):
    def test_status_returns_real_gpu_name_and_queries_only_once(self):
        instance = server()
        with patch("subprocess.run", return_value=subprocess.CompletedProcess(
            [], 0, "NVIDIA GeForce RTX 4070 SUPER\n", ""
        )) as query, patch.dict("sys.modules", {"torch": None}):
            for _ in range(2):
                status = instance.check_status()
                self.assertEqual(status["gpu_name"], "NVIDIA GeForce RTX 4070 SUPER")
                self.assertEqual(status["device"], "cuda")
                self.assertTrue(status["model_loaded"])
            query.assert_called_once()
            self.assertIn("--query-gpu=name", query.call_args.args[0])
            self.assertGreater(query.call_args.kwargs["timeout"], 0)
            self.assertLessEqual(query.call_args.kwargs["timeout"], 2)

    def test_cpu_status_clears_gpu_name_without_querying(self):
        instance = server("cpu")
        with patch("subprocess.run") as query:
            status = instance.check_status()
            self.assertIsNone(status.get("gpu_name", "missing"))
            self.assertEqual(status["device"], "cpu")
            query.assert_not_called()

    def test_probe_failures_preserve_working_status(self):
        for failure in (FileNotFoundError(), subprocess.TimeoutExpired("nvidia-smi", 1)):
            with self.subTest(failure=type(failure).__name__):
                instance = server()
                with patch("subprocess.run", side_effect=failure):
                    status = instance.check_status()
                self.assertTrue(status["success"])
                self.assertTrue(status["model_loaded"])
                self.assertIsNone(status.get("gpu_name", "missing"))

    def test_failed_command_does_not_publish_its_output_as_gpu_name(self):
        instance = server()
        with patch("subprocess.run", return_value=subprocess.CompletedProcess(
            [], 1, "driver error", ""
        )):
            self.assertIsNone(instance.check_status().get("gpu_name", "missing"))
