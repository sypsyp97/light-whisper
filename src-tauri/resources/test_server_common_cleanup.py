import base64
import io
import os
import sys
import tempfile
import unittest
import wave
from unittest import mock

import numpy as np

sys.path.insert(0, os.path.dirname(__file__))

import server_common

with (
    mock.patch.dict(os.environ),
    mock.patch.object(server_common, "_has_nvidia_gpu", return_value=False),
):
    import qwen3_asr_server
import r2t2_asr_server


class RuntimeEnvironmentTests(unittest.TestCase):
    def setUp(self):
        self.server = server_common.BaseASRServer.__new__(server_common.BaseASRServer)
        self.server.logger = mock.Mock()

    def test_explicit_thread_limit_is_preserved(self):
        for thread_limit in ("1", "3"):
            with (
                self.subTest(thread_limit=thread_limit),
                mock.patch.dict(os.environ, {
                    "OMP_NUM_THREADS": thread_limit, "HF_HUB_OFFLINE": "0",
                }, clear=True),
                mock.patch.object(os, "cpu_count", return_value=20),
            ):
                self.server._setup_runtime_environment()
                self.assertEqual(os.environ["HF_HUB_OFFLINE"], "1")
                self.assertEqual(os.environ["OMP_NUM_THREADS"], thread_limit)

    def test_default_thread_count_reserves_two_cpus_with_minimum_one(self):
        for cpu_count, expected in ((20, "18"), (2, "1"), (1, "1"), (None, "1")):
            with (
                self.subTest(cpu_count=cpu_count),
                mock.patch.dict(os.environ, {"HF_HUB_OFFLINE": "0"}, clear=True),
                mock.patch.object(os, "cpu_count", return_value=cpu_count),
            ):
                self.server._setup_runtime_environment()
                self.assertEqual(os.environ["HF_HUB_OFFLINE"], "1")
                self.assertEqual(os.environ["OMP_NUM_THREADS"], expected)


class ServerAudioParityTests(unittest.TestCase):
    server_classes = (
        qwen3_asr_server.Qwen3ASRServer,
        r2t2_asr_server.R2T2ASRServer,
    )

    @staticmethod
    def stereo_wav():
        samples = np.array([
            [-24000, -8000], [-18000, -2000], [-12000, 4000], [-6000, 10000],
            [0, 16000], [6000, 22000], [12000, 28000], [18000, 32000],
        ], dtype="<i2")
        buffer = io.BytesIO()
        with wave.open(buffer, "wb") as writer:
            writer.setnchannels(2)
            writer.setsampwidth(2)
            writer.setframerate(48_000)
            writer.writeframes(samples.tobytes())
        return buffer.getvalue()

    def assert_mono_audio(self, audio, expected):
        self.assertEqual(audio.dtype, np.dtype(np.float32))
        self.assertEqual(audio.ndim, 1)
        self.assertTrue(audio.flags.c_contiguous)
        np.testing.assert_array_equal(audio, expected)

    def test_resample_preserves_16khz_and_interpolates_48khz(self):
        audio = np.array([-1, -.75, -.5, -.25, 0, .25, .5, .75], dtype=np.float32)
        for server_class in self.server_classes:
            with self.subTest(engine=server_class.__name__, sample_rate=16_000):
                self.assert_mono_audio(server_class._resample(audio, 16_000), audio)
            with self.subTest(engine=server_class.__name__, sample_rate=48_000):
                self.assert_mono_audio(
                    server_class._resample(audio, 48_000),
                    np.array([-1, -.125, .75], dtype=np.float32),
                )

    def test_inline_pcm_returns_mono_float32_source_duration_and_memory_mode(self):
        samples = np.array([-32768, -24576, -16384, -8192, 0, 8192, 16384, 24576], dtype="<i2")
        payload = base64.b64encode(samples.tobytes()).decode("ascii")
        for server_class in self.server_classes:
            server = server_class.__new__(server_class)
            for sample_rate, expected in (
                (16_000, samples.astype(np.float32) / 32768.0),
                (48_000, np.array([-1, -.125, .75], dtype=np.float32)),
            ):
                with self.subTest(engine=server_class.__name__, sample_rate=sample_rate):
                    audio, duration, input_mode = server._load_audio(
                        None, payload, "pcm_s16le", sample_rate
                    )
                    self.assert_mono_audio(audio, expected)
                    self.assertEqual(duration, len(samples) / sample_rate)
                    self.assertEqual(input_mode, "memory")

    def test_stereo_wav_path_averages_channels_and_preserves_engine_duration(self):
        expected = np.array([-16000, 5000, 25000], dtype=np.float32) / 32768.0
        with tempfile.TemporaryDirectory() as directory:
            path = os.path.join(directory, "stereo.wav")
            with open(path, "wb") as output:
                output.write(self.stereo_wav())
            for server_class in self.server_classes:
                with self.subTest(engine=server_class.__name__):
                    server = server_class.__new__(server_class)
                    audio, duration, input_mode = server._load_audio(path, None, None, None)
                    self.assert_mono_audio(audio, expected)
                    expected_duration = (
                        3 / 16_000 if server_class is qwen3_asr_server.Qwen3ASRServer
                        else 8 / 48_000
                    )
                    self.assertEqual(duration, expected_duration)
                    self.assertEqual(input_mode, "path")

    def test_inline_wav_is_rejected_by_qwen_and_accepted_by_r2t2(self):
        payload = base64.b64encode(self.stereo_wav()).decode("ascii")
        qwen = qwen3_asr_server.Qwen3ASRServer.__new__(qwen3_asr_server.Qwen3ASRServer)
        with self.assertRaises(ValueError):
            qwen._load_audio(None, payload, "wav", None)
        r2t2 = r2t2_asr_server.R2T2ASRServer.__new__(r2t2_asr_server.R2T2ASRServer)
        audio, duration, input_mode = r2t2._load_audio(None, payload, "wav", None)
        self.assert_mono_audio(
            audio, np.array([-16000, 5000, 25000], dtype=np.float32) / 32768.0
        )
        self.assertEqual(duration, 8 / 48_000)
        self.assertEqual(input_mode, "memory")


if __name__ == "__main__":
    unittest.main()
