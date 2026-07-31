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
        self.inputs = []

    def run(self, audio, **_kwargs):
        self.calls += 1
        self.inputs.append(np.array(audio, copy=True))
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
        self.session_options = None
        self.__class__.instances.append(self)

    def session(self, **kwargs):
        self.session_calls += 1
        self.session_options = kwargs
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
            mock.patch.object(qwen3_asr_server, "get_vad_model", return_value=object()),
            mock.patch.object(
                qwen3_asr_server,
                "get_speech_timestamps",
                return_value=[{"start": 0, "end": 16_000}],
            ),
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
        self.assertEqual(
            FakeModel.instances[0].session_options,
            {"kv_type": "f16", "n_ctx": 32_768},
        )
        self.assertEqual(FakeModel.instances[0].session_instance.calls, 2)
        self.assertEqual(first["input_mode"], "memory")
        self.assertEqual(first["engine"], "qwen3-asr-0.6b")

    def test_vad_rejects_silence_without_running_qwen(self):
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
            mock.patch.object(qwen3_asr_server, "get_vad_model", return_value=object()),
            mock.patch.object(qwen3_asr_server, "get_speech_timestamps", return_value=[]),
            mock.patch.dict(sys.modules, {"transcribe_cpp": fake_module}),
        ):
            server = qwen3_asr_server.Qwen3ASRServer(engine="qwen3-asr-0.6b")
            self.assertTrue(server.initialize()["success"])
            pcm = np.zeros(16_000, dtype="<i2").tobytes()
            payload = base64.b64encode(pcm).decode("ascii")

            result = server.transcribe_audio(
                None,
                audio_base64=payload,
                audio_format="pcm_s16le",
                sample_rate=16_000,
            )

        self.assertEqual(result["text"], "")
        self.assertEqual(result["speech_duration"], 0.0)
        self.assertEqual(result["vad_segments"], 0)
        self.assertEqual(result["inference_ms"], 0.0)
        self.assertEqual(FakeModel.instances[0].session_instance.calls, 0)
        stats = server.get_performance_stats()
        self.assertEqual(stats["vad_rejected"], 1)
        self.assertTrue(stats["models_loaded"]["vad"])

    def test_vad_trims_only_outer_silence_before_qwen(self):
        fake_module = types.SimpleNamespace(Model=FakeModel)
        chunks = [
            {"start": 1_600, "end": 6_400},
            {"start": 9_600, "end": 14_400},
        ]
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
            mock.patch.object(qwen3_asr_server, "get_vad_model", return_value=object()),
            mock.patch.object(
                qwen3_asr_server, "get_speech_timestamps", return_value=chunks
            ),
            mock.patch.dict(sys.modules, {"transcribe_cpp": fake_module}),
        ):
            server = qwen3_asr_server.Qwen3ASRServer(engine="qwen3-asr-1.7b")
            self.assertTrue(server.initialize()["success"])
            pcm = np.arange(16_000, dtype="<i2").tobytes()
            payload = base64.b64encode(pcm).decode("ascii")

            result = server.transcribe_audio(
                None,
                audio_base64=payload,
                audio_format="pcm_s16le",
                sample_rate=16_000,
            )

        sent = FakeModel.instances[0].session_instance.inputs[0]
        self.assertEqual(len(sent), 12_800)
        self.assertAlmostEqual(float(sent[0]), 1_600 / 32768.0)
        self.assertAlmostEqual(float(sent[-1]), 14_399 / 32768.0)
        self.assertEqual(result["vad_segments"], 2)
        self.assertEqual(result["speech_duration"], 0.8)
        self.assertTrue(server.check_status()["models"]["vad"])


if __name__ == "__main__":
    unittest.main()
