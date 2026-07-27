import base64
import os
import sys
import types
import unittest
from unittest import mock

import numpy as np

sys.path.insert(0, os.path.dirname(__file__))

import qwen3_asr_server


class FakeSession:
    def __init__(self):
        self.calls = 0

    def run(self, audio, **_kwargs):
        self.calls += 1
        return types.SimpleNamespace(text="测试文本", language="zh")

    def close(self):
        pass


class FakeModel:
    instances = []

    def __init__(self, path, backend):
        self.path = path
        self.backend = backend
        self.session_instance = FakeSession()
        self.session_calls = 0
        self.__class__.instances.append(self)

    def session(self, **_kwargs):
        self.session_calls += 1
        return self.session_instance

    def close(self):
        pass


class Qwen3ASRServerTests(unittest.TestCase):
    def setUp(self):
        FakeModel.instances.clear()

    def test_reuses_one_model_and_session_for_inline_pcm_requests(self):
        fake_module = types.SimpleNamespace(Model=FakeModel)
        with (
            mock.patch.object(
                qwen3_asr_server.Qwen3ASRServer, "_detect_device", return_value="cuda"
            ),
            mock.patch.object(
                qwen3_asr_server.Qwen3ASRServer,
                "_resolve_model_path",
                return_value="model.gguf",
            ),
            mock.patch.object(qwen3_asr_server.Qwen3ASRServer, "_warmup_inference"),
            mock.patch.dict(sys.modules, {"transcribe_cpp": fake_module}),
        ):
            server = qwen3_asr_server.Qwen3ASRServer(engine="qwen3-asr-0.6b")
            self.assertTrue(server.initialize()["success"])
            pcm = np.zeros(16_000, dtype="<i2").tobytes()
            payload = base64.b64encode(pcm).decode("ascii")

            first = server.transcribe_audio(
                None,
                audio_base64=payload,
                audio_format="pcm_s16le",
                sample_rate=16_000,
            )
            second = server.transcribe_audio(
                None,
                audio_base64=payload,
                audio_format="pcm_s16le",
                sample_rate=16_000,
            )

        self.assertEqual((first["text"], second["text"]), ("测试文本", "测试文本"))
        self.assertEqual(len(FakeModel.instances), 1)
        self.assertEqual(FakeModel.instances[0].session_calls, 1)
        self.assertEqual(FakeModel.instances[0].session_instance.calls, 2)
        self.assertEqual(first["input_mode"], "memory")


if __name__ == "__main__":
    unittest.main()
