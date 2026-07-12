import os
import sys
import types
import unittest
from unittest import mock

sys.path.insert(0, os.path.dirname(__file__))

import server_common
import whisper_server


class WhisperDeviceDetectionTests(unittest.TestCase):
    def setUp(self):
        self.server = object.__new__(whisper_server.WhisperServer)
        self.server.logger = mock.Mock()
        self.fake_ctranslate2 = types.SimpleNamespace(
            get_supported_compute_types=mock.Mock(
                return_value={"float16", "int8_float16"}
            )
        )
        self.fake_torch = types.SimpleNamespace(
            cuda=types.SimpleNamespace(is_available=mock.Mock(return_value=False))
        )

    def test_ctranslate2_cuda_compute_types_select_cuda_without_pytorch_fallback(self):
        with (
            mock.patch.dict(
                sys.modules,
                {
                    "ctranslate2": self.fake_ctranslate2,
                    "torch": self.fake_torch,
                },
            ),
            mock.patch.object(
                server_common.BaseASRServer,
                "_detect_device",
                return_value="cpu",
            ) as fallback,
        ):
            device = self.server._detect_device()

        self.assertEqual(
            (device, fallback.call_count),
            ("cuda", 0),
            "CTranslate2 CUDA compute types should select CUDA directly instead of falling through to PyTorch detection",
        )

    def test_pytorch_fallback_handles_ctranslate2_cuda_probe_failure(self):
        self.fake_ctranslate2.get_supported_compute_types.side_effect = RuntimeError(
            "CUDA runtime unavailable"
        )
        with (
            mock.patch.dict(
                sys.modules,
                {
                    "ctranslate2": self.fake_ctranslate2,
                    "torch": self.fake_torch,
                },
            ),
            mock.patch.object(
                server_common.BaseASRServer,
                "_detect_device",
                return_value="cuda",
            ) as fallback,
        ):
            device = self.server._detect_device()

        self.assertEqual(device, "cuda")
        self.assertEqual(
            fallback.call_count,
            1,
            "PyTorch detection should remain the fallback when the CTranslate2 probe fails",
        )


if __name__ == "__main__":
    unittest.main()
