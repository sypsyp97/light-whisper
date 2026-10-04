import base64
import os
import subprocess
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


class FakeVad:
    def __init__(self, chunks):
        self.chunks = chunks

    def warmup(self):
        pass

    def speech_timestamps(self, _audio):
        return self.chunks


class Qwen3ASRServerTests(unittest.TestCase):
    def setUp(self):
        FakeModel.instances.clear()
        # Model and VAD tests must not query GPU metadata from the host machine.
        gpu_info_patcher = mock.patch.object(
            qwen3_asr_server.Qwen3ASRServer,
            "_get_gpu_device_info",
            return_value={
                "device": "cuda",
                "gpu_name": "Test GPU",
                "gpu_memory_total": 24.0,
            },
        )
        gpu_info_patcher.start()
        self.gpu_info_patcher = gpu_info_patcher
        self.addCleanup(gpu_info_patcher.stop)

    def test_gpu_metadata_failure_does_not_discard_loaded_runtime(self):
        self.gpu_info_patcher.stop()
        for error in (FileNotFoundError("nvidia-smi"), subprocess.TimeoutExpired("nvidia-smi", 3)):
            with (
                self.subTest(error=type(error).__name__),
                mock.patch.object(qwen3_asr_server.Qwen3ASRServer, "_detect_device", return_value="cuda"),
                mock.patch.object(qwen3_asr_server.Qwen3ASRServer, "_resolve_model_path", return_value="model.gguf"),
                mock.patch.object(qwen3_asr_server.Qwen3ASRServer, "_warmup_inference"),
                mock.patch.object(qwen3_asr_server, "FireRedVad", return_value=FakeVad([])),
                mock.patch.dict(sys.modules, {"transcribe_cpp": types.SimpleNamespace(Model=FakeModel)}),
                mock.patch.object(qwen3_asr_server.subprocess, "run", side_effect=error) as query,
            ):
                server = qwen3_asr_server.Qwen3ASRServer(engine="qwen3-asr-0.6b")
                self.assertTrue(server.initialize()["success"])
                self.assertTrue(server.initialized)
                self.assertIsNotNone(server.model)
                self.assertIsNotNone(server.session)
                self.assertGreater(query.call_args.kwargs["timeout"], 0)
                self.assertLessEqual(query.call_args.kwargs["timeout"], 3)

    def test_failed_initialization_resets_state_and_can_retry(self):
        with (
            mock.patch.object(qwen3_asr_server.Qwen3ASRServer, "_detect_device", return_value="cuda"),
            mock.patch.object(qwen3_asr_server.Qwen3ASRServer, "_resolve_model_path", return_value="model.gguf"),
            mock.patch.object(qwen3_asr_server.Qwen3ASRServer, "_warmup_inference"),
            mock.patch.object(qwen3_asr_server, "FireRedVad", return_value=FakeVad([])),
            mock.patch.dict(sys.modules, {"transcribe_cpp": types.SimpleNamespace(Model=FakeModel)}),
        ):
            server = qwen3_asr_server.Qwen3ASRServer(engine="qwen3-asr-0.6b")
            with mock.patch.object(server, "_get_gpu_device_info", side_effect=RuntimeError("metadata failure")):
                self.assertFalse(server.initialize()["success"])
            self.assertFalse(server.initialized)
            self.assertIsNone(server.model)
            self.assertIsNone(server.session)
            self.assertIsNone(server.vad_model)
            self.assertTrue(server.initialize()["success"])
            self.assertIsNotNone(server.model)
            self.assertIsNotNone(server.session)

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
            mock.patch.object(
                qwen3_asr_server,
                "FireRedVad",
                return_value=FakeVad([{"start": 0, "end": 16_000}]),
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
            mock.patch.object(
                qwen3_asr_server,
                "FireRedVad",
                return_value=FakeVad([]),
            ),
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
            mock.patch.object(
                qwen3_asr_server,
                "FireRedVad",
                return_value=FakeVad(chunks),
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

    def test_idle_unload_closes_runtime_and_keeps_vad(self):
        from gpu_idle import status_keeps_process_ready

        with mock.patch.object(
            qwen3_asr_server.Qwen3ASRServer, "_detect_device", return_value="cuda"
        ):
            server = qwen3_asr_server.Qwen3ASRServer(engine="qwen3-asr-0.6b")
        model = mock.Mock()
        session = mock.Mock()
        vad = object()
        server.model = model
        server.session = session
        server.vad_model = vad
        server.initialized = True

        self.assertTrue(server._suspend_gpu_runtime())
        model.close.assert_called_once()
        session.close.assert_called_once()
        self.assertIsNone(server.model)
        self.assertIsNone(server.session)
        self.assertIs(server.vad_model, vad)
        self.assertFalse(server.initialized)
        self.assertTrue(server._gpu_suspended)
        self.assertFalse(server._suspend_gpu_runtime())

        with mock.patch("qwen3_asr_server.importlib.metadata.version", return_value="9"):
            status = server.check_status()
        self.assertTrue(status["success"])
        self.assertFalse(status["model_loaded"])
        self.assertFalse(status["initialized"])
        self.assertTrue(status["models"]["vad"])
        self.assertFalse(status["models"]["asr"])
        self.assertTrue(status["gpu_suspended"])
        self.assertTrue(status_keeps_process_ready(True, status))

        with (
            mock.patch.object(server, "initialize", return_value={"success": False, "error": "reload"}) as initialize,
        ):
            result = server.transcribe_audio("clip.wav")
        initialize.assert_called_once()
        self.assertEqual(result["error"], "reload")


if __name__ == "__main__":
    unittest.main()
