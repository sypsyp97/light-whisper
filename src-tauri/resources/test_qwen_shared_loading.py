"""Offline package and server ownership checks for Qwen's shared audio.cpp path."""

import base64
import contextlib
import hashlib
import importlib.metadata
import os
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

import numpy as np

sys.path.insert(0, os.path.dirname(__file__))

import download_models
import hf_cache_utils
import qwen3_asr_server
from r2t2_stream import R2T2StreamSession


class FakeNativeRuntime:
    def __init__(self):
        self.inputs = []
        self.close = mock.Mock()

    def run(self, audio, **options):
        self.inputs.append((np.array(audio, copy=True), options))
        return SimpleNamespace(text="共享原生最终结果", language="Chinese")


class QwenSharedLoadingTests(unittest.TestCase):
    def setUp(self):
        self.enterContext(mock.patch("signal.signal"))
        self.enterContext(mock.patch.object(
            qwen3_asr_server.Qwen3ASRServer, "_setup_runtime_environment"
        ))
        self.enterContext(mock.patch.object(
            qwen3_asr_server.Qwen3ASRServer, "_detect_device", return_value="cuda"
        ))
        self.enterContext(mock.patch.object(
            qwen3_asr_server.Qwen3ASRServer, "_get_gpu_device_info",
            return_value={"device": "cuda"},
        ))
        self.enterContext(mock.patch.object(qwen3_asr_server, "logger"))
        self.native = FakeNativeRuntime()
        self.native_factory = self.enterContext(mock.patch.object(
            qwen3_asr_server, "QwenNativeRuntime", create=True,
            return_value=self.native,
        ))
        self.preview = mock.Mock(preview_text="")
        self.preview_factory = self.enterContext(mock.patch.object(
            qwen3_asr_server, "QwenPreviewRuntime", create=True,
            return_value=self.preview,
        ))
        self.pipeline_factory = self.enterContext(mock.patch.object(
            qwen3_asr_server, "QwenPipelineRuntime", create=True,
            return_value=self.preview,
        ))
        self.vad = mock.Mock()
        self.vad.speech_timestamps.return_value = []
        self.enterContext(mock.patch.object(
            qwen3_asr_server, "FireRedVad", return_value=self.vad
        ))
        self.legacy_session = mock.Mock()
        self.legacy_session.run.return_value = SimpleNamespace(
            text="legacy result", language="zh"
        )
        self.legacy_model = mock.Mock()
        self.legacy_model.session.return_value = self.legacy_session
        self.legacy_factory = mock.Mock(return_value=self.legacy_model)
        self.enterContext(mock.patch.dict(sys.modules, {
            "transcribe_cpp": SimpleNamespace(Model=self.legacy_factory),
        }))
        self.model_path = str(Path("verified-cache") / "qwen3-asr-0.6b-q8_0.gguf")

    def server(self, engine="qwen3-asr-0.6b"):
        server = qwen3_asr_server.Qwen3ASRServer(engine=engine)
        server.stdout_suppressor = mock.Mock()
        server.stdout_suppressor.suppress.side_effect = contextlib.nullcontext
        self.addCleanup(server._close_runtime)
        return server

    def initialize(self, server):
        with (
            mock.patch.object(server, "_resolve_model_path", return_value=self.model_path),
            mock.patch.object(server, "_warmup_inference"),
        ):
            return server.initialize()

    def test_only_supported_models_are_selectable_and_current_qwen_pin_is_unchanged(self):
        self.assertEqual(set(hf_cache_utils.QWEN3_ASR_MODELS), {"qwen3-asr-0.6b"})
        self.assertEqual(set(hf_cache_utils.LOCAL_ASR_MODELS), {"qwen3-asr-0.6b", "confucius4-r2t2"})
        self.assertEqual(hf_cache_utils.QWEN3_ASR_MODELS["qwen3-asr-0.6b"], {
            "repo_id": "audio-cpp/audio.cpp-gguf",
            "filename": "Qwen3-ASR-0.6B-GGUF/qwen3-asr-0.6b-q8_0.gguf",
            "revision": "f861648c0c1f4da9f18e9a0ce5e217b0679bb2f5",
            "size": 1_151_272_416,
            "sha256": "6c44ec2fb4cee513892d7863c1fcc3ea6b699ffa4d899b0ef4ab19956d9544f7",
        })

    def test_retired_selection_rejects_before_runtime_setup_or_cache_resolution(self):
        server_class = qwen3_asr_server.Qwen3ASRServer
        with mock.patch.object(server_class, "_resolve_model_path") as resolve, \
             mock.patch.object(server_class, "_setup_runtime_environment") as setup:
            for engine in ("qwen3-asr-1.7b", "unknown"):
                with self.subTest(engine=engine), self.assertRaises(ValueError):
                    server_class(engine=engine)
            with mock.patch.dict(os.environ, {"LIGHT_WHISPER_ASR_ENGINE": "qwen3-asr-1.7b"}):
                with self.assertRaises(ValueError):
                    server_class()
            resolve.assert_not_called()
            setup.assert_not_called()
        self.native_factory.assert_not_called()
        self.legacy_factory.assert_not_called()

    def test_shared_package_download_preserves_original_legacy_cache_file_and_ref(self):
        selected = hf_cache_utils.QWEN3_ASR_MODELS["qwen3-asr-0.6b"]
        # Scale only payload size/hash; exercise the selected real cache destination.
        data = b"shared audio.cpp format fixture"
        request = {
            "name": selected["repo_id"], "type": "asr", "revision": selected["revision"],
            "files": [{"rfilename": selected["filename"], "size": len(data),
                       "sha256": hashlib.sha256(data).hexdigest()}],
        }
        response = mock.Mock(status_code=200, headers={"Content-Length": str(len(data))})
        response.iter_content.return_value = [data]
        legacy_revision = "e4e16599b900eb0cb36e524514756bb92eb092b7"
        with (
            tempfile.TemporaryDirectory() as root,
            mock.patch.dict(os.environ, {"HF_HUB_CACHE": root}),
            mock.patch.object(download_models, "_candidate_endpoints", return_value=["https://example.invalid"]),
            mock.patch.object(download_models, "_emit"),
            mock.patch.object(download_models.requests, "get", return_value=response),
        ):
            legacy_repo = Path(root) / "models--handy-computer--Qwen3-ASR-0.6B-gguf"
            legacy_file = legacy_repo / "snapshots" / legacy_revision / "Qwen3-ASR-0.6B-Q8_0.gguf"
            legacy_file.parent.mkdir(parents=True)
            legacy_file.write_bytes(b"original transcribe.cpp model fixture")
            legacy_ref = legacy_repo / "refs" / "main"
            legacy_ref.parent.mkdir()
            legacy_ref.write_text(legacy_revision, encoding="utf-8")

            self.assertTrue(download_models.download_model(request)["success"])

            self.assertEqual(legacy_file.read_bytes(), b"original transcribe.cpp model fixture")
            self.assertEqual(legacy_ref.read_text(encoding="utf-8"), legacy_revision)
            shared_file = (Path(root) / "models--audio-cpp--audio.cpp-gguf" / "snapshots"
                           / "f861648c0c1f4da9f18e9a0ce5e217b0679bb2f5"
                           / "Qwen3-ASR-0.6B-GGUF" / "qwen3-asr-0.6b-q8_0.gguf")
            self.assertEqual(shared_file.read_bytes(), data)

    def test_shared_package_loads_one_native_owner_from_shared_dll_directory(self):
        server = self.server()
        server._load_runtime(self.model_path)

        self.native_factory.assert_called_once()
        args, options = self.native_factory.call_args
        self.assertEqual(args[0], self.model_path)
        resources = Path(qwen3_asr_server.__file__).resolve().parent
        self.assertEqual(Path(args[1]), resources / "r2t2-native" / "cuda" / "audiocpp.dll")
        self.assertEqual(options["backend"], "cuda")
        self.assertIn(resources, [Path(path) for path in options["dll_directories"]])
        self.assertIs(server.model, self.native)
        self.assertIs(server.session, self.native)
        self.assertEqual((server.device, server.backend), ("cuda", "cuda"))
        self.legacy_factory.assert_not_called()

    def test_shared_native_cuda_failure_retries_cpu_without_transcribe_cpp(self):
        server = self.server()

        def load_native(*_args, backend, **_options):
            if backend != "cpu":
                raise RuntimeError(f"{backend} unavailable")
            return self.native

        self.native_factory.side_effect = load_native

        server._load_runtime(self.model_path)

        self.assertEqual(self.native_factory.call_count, 2)
        resources = Path(qwen3_asr_server.__file__).resolve().parent
        for call, backend in zip(self.native_factory.call_args_list, ("cuda", "cpu")):
            self.assertEqual(call.kwargs["backend"], backend)
            self.assertEqual(Path(call.args[1]), resources / "r2t2-native" / backend / "audiocpp.dll")
        self.assertIs(server.session, self.native)
        self.assertEqual((server.device, server.backend), ("cpu", "cpu"))
        self.legacy_factory.assert_not_called()

    def test_resolved_current_model_keeps_native_options_and_stream_without_legacy_imports(self):
        self.model_path = str(Path("verified-cache") / "resolved-current.gguf")
        server = self.server()
        with mock.patch.dict(sys.modules, {
            "transcribe_cpp": None, "transcribe_cpp_native": None, "transcribe_cpp_native_cu12": None,
        }):
            self.assertTrue(self.initialize(server)["success"])
            pcm = np.arange(16013, dtype="<i2")
            self.vad.speech_timestamps.return_value = [{"start": 0, "end": len(pcm)}]
            result = server.transcribe_audio(
                None, options={"context": " 上下文 ", "language": " zh ", "hot_words": ["ignored"]},
                hot_words=[" 小爱 ", "小爱", "语音  助手"],
                audio_base64=base64.b64encode(pcm.tobytes()).decode("ascii"),
                audio_format="pcm_s16le", sample_rate=16000,
            )
            status = server.check_status()
        self.assertIs(server.model, self.native)
        self.assertIs(server.session, self.native)
        self.assertIsInstance(server.stream, R2T2StreamSession)
        self.assertTrue(result["success"])
        np.testing.assert_array_equal(self.native.inputs[-1][0], pcm.astype(np.float32) / 32768.0)
        self.assertEqual(self.native.inputs[-1][1], {
            "timestamps": "none", "context": "上下文\nHotwords: 小爱, 语音 助手", "language": "zh",
        })
        self.assertTrue(status["success"])
        self.assertTrue(status["installed"])
        self.assertTrue(status["native_streaming"])
        self.legacy_factory.assert_not_called()

    def test_initialize_wires_shared_preview_into_real_stream_session(self):
        server = self.server()

        result = self.initialize(server)

        self.assertTrue(result["success"])
        self.assertIsInstance(server.stream, R2T2StreamSession)
        self.pipeline_factory.assert_called_once_with(
            server._run_inference, prepare_audio=mock.ANY,
        )
        self.preview_factory.assert_not_called()
        self.assertIs(server.stream.runtime, self.preview)
        prepare_audio = self.pipeline_factory.call_args.kwargs["prepare_audio"]
        self.assertTrue(callable(prepare_audio))
        audio = np.arange(13, dtype=np.float32)
        self.vad.speech_timestamps.return_value = [{"start": 2, "end": 11}]
        np.testing.assert_array_equal(prepare_audio(audio), audio[2:11])
        self.vad.speech_timestamps.assert_called_once_with(audio)
        self.assertEqual(self.native.inputs, [], "CPU preparation must not invoke native inference")
        self.assertTrue(server.initialized)
        self.legacy_factory.assert_not_called()

    def test_native_aliases_close_once_and_clear_stream_on_repeated_cleanup(self):
        server = self.server()
        server.model = server.session = self.native
        server.stream = R2T2StreamSession(self.preview)
        server.initialized = True

        server._close_runtime()
        server._close_runtime()

        self.native.close.assert_called_once()
        self.assertIsNone(server.model)
        self.assertIsNone(server.session)
        self.assertIsNone(server.stream)
        self.assertFalse(server.initialized)

    def test_failed_native_load_leaves_no_owner_and_initialization_can_retry(self):
        server = self.server()
        self.native_factory.side_effect = RuntimeError("native unavailable")

        failed = self.initialize(server)

        self.assertFalse(failed["success"])
        self.assertEqual(failed["type"], "init_error")
        self.assertFalse(server.initialized)
        self.assertIsNone(server.model)
        self.assertIsNone(server.session)
        self.assertIsNone(server.stream)
        self.assertIsNone(server.vad_model)
        self.assertEqual(self.native_factory.call_count, 2)
        self.native_factory.side_effect = None

        retried = self.initialize(server)

        self.assertTrue(retried["success"])
        self.assertIs(server.model, self.native)
        self.assertIs(server.session, self.native)
        self.assertIsInstance(server.stream, R2T2StreamSession)
        self.assertIs(server.stream.runtime, self.preview)
        self.legacy_factory.assert_not_called()

    def test_native_status_does_not_depend_on_transcribe_cpp_distribution(self):
        server = self.server()
        self.assertTrue(self.initialize(server)["success"])
        with mock.patch.object(
            importlib.metadata, "version",
            side_effect=importlib.metadata.PackageNotFoundError("transcribe-cpp"),
        ) as distribution_version:
            status = server.check_status()

        self.assertTrue(status["success"])
        self.assertTrue(status["installed"])
        self.assertTrue(status["model_loaded"])
        self.assertTrue(status["native_streaming"])
        self.assertTrue(status["models"]["vad"])
        self.assertNotIn(mock.call("transcribe-cpp"), distribution_version.call_args_list)

    def test_unloaded_native_status_tracks_actual_cpu_cuda_files_without_distribution_metadata(self):
        server = self.server()
        resources = Path(qwen3_asr_server.__file__).resolve().parent
        with mock.patch.object(importlib.metadata, "version", side_effect=importlib.metadata.PackageNotFoundError) as version:
            for available in (set(), {resources / "r2t2-native/cpu/audiocpp.dll"},
                              {resources / "r2t2-native/cuda/audiocpp.dll"}):
                with self.subTest(available=available), mock.patch.object(
                    Path, "is_file", autospec=True, side_effect=lambda path: path in available,
                ):
                    status = server.check_status()
                    self.assertTrue(status["success"])
                    self.assertEqual(status["installed"], bool(available))
                    self.assertFalse(status["model_loaded"])
            version.assert_not_called()

    def test_native_final_keeps_internal_pause_and_thirteen_sample_tail_after_vad(self):
        server = self.server()
        server.model = server.session = self.native
        server.vad_model = self.vad
        server.initialized = True
        self.vad.speech_timestamps.return_value = [
            {"start": 1600, "end": 6400}, {"start": 9600, "end": 14413},
        ]
        pcm = np.arange(16013, dtype="<i2")

        result = server.transcribe_audio(
            None, audio_base64=base64.b64encode(pcm.tobytes()).decode("ascii"),
            audio_format="pcm_s16le", sample_rate=16000,
        )

        self.assertTrue(result["success"])
        self.assertEqual(result["text"], "共享原生最终结果")
        self.assertEqual(result["vad_segments"], 2)
        self.assertEqual(len(self.native.inputs), 1)
        sent, options = self.native.inputs[0]
        np.testing.assert_array_equal(sent, pcm[1600:14413].astype(np.float32) / 32768.0)
        self.assertEqual(len(sent), 12813)
        self.assertEqual(options["timestamps"], "none")
        self.legacy_factory.assert_not_called()

    def test_native_final_silence_does_not_run_native_decoder(self):
        server = self.server()
        server.model = server.session = self.native
        server.vad_model = self.vad
        server.initialized = True
        pcm = np.zeros(16013, dtype="<i2")

        result = server.transcribe_audio(
            None, audio_base64=base64.b64encode(pcm.tobytes()).decode("ascii"),
            audio_format="pcm_s16le", sample_rate=16000,
        )

        self.assertTrue(result["success"])
        self.assertEqual(result["text"], "")
        self.assertEqual(result["vad_segments"], 0)
        self.assertEqual(self.native.inputs, [])


if __name__ == "__main__":
    unittest.main()
