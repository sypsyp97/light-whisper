"""Server input cadence, native option wiring and segmented PCM conservation."""

import contextlib
import os
import sys
import unittest
import wave
from pathlib import Path
from unittest.mock import Mock, patch

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import r2t2_asr_server
from r2t2_asr_server import R2T2ASRServer
from r2t2_segmented import SegmentedR2T2
from r2t2_native import NativeRuntime

CPU_CHUNK_MS = 320
CHUNK_SAMPLES = 5120
COLLECTOR_CHUNK = 2560
RESOURCE_DIR = Path(__file__).resolve().parent


class AlwaysSpeechVad:
    """整窗报语音的 VAD 替身，保证 SegmentedR2T2 一定进入 native 路径。"""

    def __init__(self):
        self.calls = 0

    def speech_timestamps(self, window):
        self.calls += 1
        return [{"start": 0, "end": int(np.size(window))}]


def make_native(chunk_samples=CHUNK_SAMPLES, deltas=()):
    """Mock 的 native：记录每次 ``native.feed`` 的真实 PCM，返回累积文本。"""
    native = Mock(chunk_samples=chunk_samples)
    native.fed_chunks = []
    pending = list(deltas)
    committed = {"text": ""}

    def feed(pcm):
        native.fed_chunks.append(np.array(pcm, copy=True))
        if pending:
            committed["text"] += pending.pop(0)
        return committed["text"], None

    native.feed.side_effect = feed
    native.finish.side_effect = lambda: (committed["text"], None)
    return native


def fed_sizes(native):
    return [int(chunk.size) for chunk in native.fed_chunks]


def make_stream(native=None):
    native = native if native is not None else make_native()
    stream = SegmentedR2T2(native, AlwaysSpeechVad(), chunk_samples=CHUNK_SAMPLES)
    stream.start("", None)
    return stream


def feed_in_collector_chunks(stream, total_samples):
    """按采集端节奏（每次 2560）送样本，返回完整输入用于守恒断言。"""
    samples = np.arange(total_samples, dtype=np.float32)
    for offset in range(0, total_samples, COLLECTOR_CHUNK):
        stream.feed(samples[offset : offset + COLLECTOR_CHUNK])
    return samples


class DerivingNative:
    """模拟真实 NativeRuntime：chunk_samples 由构造时的 chunk_ms 派生。"""

    def __init__(self, _model_path, _library_path, *, backend, chunk_ms,
                 dll_directories=(), **_kwargs):
        self.backend = backend
        self.chunk_ms = chunk_ms
        self.chunk_samples = chunk_ms * 16
        self.dll_directories = tuple(dll_directories)
        self.feeds = []
        self.start_calls = 0
        self.finish_calls = 0
        self.reset_calls = 0
        self.closed = False

    def start(self, _context="", _language=None):
        self.start_calls += 1

    def feed(self, pcm):
        self.feeds.append(np.array(pcm, copy=True))
        return "", None

    def finish(self):
        self.finish_calls += 1
        return "", None

    def reset(self):
        self.reset_calls += 1

    def close(self):
        self.closed = True


class LoadRuntimeChunkContractTests(unittest.TestCase):
    """Both backends load shared DLLs and keep their own input cadence."""

    def test_cuda_runtime_requests_adaptive_160ms_input(self):
        server = R2T2ASRServer.__new__(R2T2ASRServer)
        with patch.object(r2t2_asr_server, "NativeRuntime") as runtime:
            runtime.return_value.chunk_samples = COLLECTOR_CHUNK
            server._load_runtime("verified-model")

        kwargs = runtime.call_args.kwargs
        self.assertEqual(kwargs.get("backend"), "cuda")
        self.assertEqual(kwargs.get("chunk_ms"), 160)
        self.assertIs(kwargs.get("adaptive_cadence"), True)
        self.assertIn(RESOURCE_DIR, kwargs["dll_directories"])
        self.assertEqual(server.backend, "cuda")
        self.assertIs(server.native, runtime.return_value)
        self.assertEqual(server.native.chunk_samples, COLLECTOR_CHUNK)

    def test_cpu_fallback_runtime_is_constructed_with_320ms_chunks(self):
        server = R2T2ASRServer.__new__(R2T2ASRServer)
        cpu_runtime = Mock(chunk_samples=CHUNK_SAMPLES)
        with patch.object(
            r2t2_asr_server,
            "NativeRuntime",
            side_effect=[RuntimeError("CUDA unavailable"), cpu_runtime],
        ) as runtime:
            server._load_runtime("verified-model")

        calls = runtime.call_args_list
        self.assertEqual([call.kwargs.get("backend") for call in calls], ["cuda", "cpu"])
        cuda_call, cpu_call = calls[0], calls[-1]
        self.assertEqual(cpu_call.kwargs.get("chunk_ms"), CPU_CHUNK_MS)
        self.assertFalse(cpu_call.kwargs.get("adaptive_cadence", False))
        self.assertEqual(cuda_call.kwargs.get("chunk_ms"), 160)
        self.assertIs(cuda_call.kwargs.get("adaptive_cadence"), True)
        for call in calls:
            self.assertIn(RESOURCE_DIR, call.kwargs["dll_directories"])
        self.assertEqual(server.backend, "cpu")
        self.assertIs(server.native, cpu_runtime)
        self.assertEqual(server.native.chunk_samples, CHUNK_SAMPLES)


class SegmentedChunkCadenceTests(unittest.TestCase):
    """契约 3/4：5120 为块长，2560 采集块必须两两聚合。"""

    def assert_samples_conserved(self, native, samples):
        self.assertTrue(native.fed_chunks, "native.feed 从未被调用")
        self.assertEqual(sum(fed_sizes(native)), int(samples.size))
        np.testing.assert_array_equal(np.concatenate(native.fed_chunks), samples)

    def test_exact_chunk_is_processed_immediately_and_pending_is_empty(self):
        native = make_native()
        stream = make_stream(native)

        stream.feed(np.arange(CHUNK_SAMPLES, dtype=np.float32))

        self.assertEqual(fed_sizes(native), [CHUNK_SAMPLES])
        self.assertEqual(stream._pending.size, 0)

    def test_sub_chunk_feed_is_held_until_finish(self):
        native = make_native()
        stream = make_stream(native)
        samples = np.arange(COLLECTOR_CHUNK, dtype=np.float32)

        stream.feed(samples)

        self.assertEqual(fed_sizes(native), [])
        self.assertEqual(stream._pending.size, COLLECTOR_CHUNK)
        native.start.assert_not_called()

        stream.finish()

        self.assertEqual(fed_sizes(native), [COLLECTOR_CHUNK])
        self.assertEqual(native.finish.call_count, 1)
        self.assert_samples_conserved(native, samples)

    def test_three_collector_chunks_process_one_block_then_flush_remainder(self):
        # 3 x 2560 = 7680 = 5120 + 2560：一次满块，余量由 finish 冲刷。
        native = make_native()
        stream = make_stream(native)

        samples = feed_in_collector_chunks(stream, 3 * COLLECTOR_CHUNK)
        self.assertEqual(fed_sizes(native), [CHUNK_SAMPLES])
        self.assertEqual(stream._pending.size, COLLECTOR_CHUNK)

        stream.finish()

        self.assertEqual(fed_sizes(native), [CHUNK_SAMPLES, COLLECTOR_CHUNK])
        self.assertEqual(native.finish.call_count, 1)
        self.assertEqual(stream._pending.size, 0)
        self.assert_samples_conserved(native, samples)

    def test_five_collector_chunks_process_two_blocks_then_flush_remainder(self):
        # 5 x 2560 = 12800：两次满块 + finish 冲刷 2560（父代理要求的块长序列）。
        native = make_native()
        stream = make_stream(native)

        samples = feed_in_collector_chunks(stream, 5 * COLLECTOR_CHUNK)
        self.assertEqual(fed_sizes(native), [CHUNK_SAMPLES, CHUNK_SAMPLES])
        self.assertEqual(stream._pending.size, COLLECTOR_CHUNK)

        stream.finish()

        self.assertEqual(fed_sizes(native), [CHUNK_SAMPLES, CHUNK_SAMPLES, COLLECTOR_CHUNK])
        self.assertEqual(native.finish.call_count, 1)
        self.assertEqual(stream._pending.size, 0)
        self.assert_samples_conserved(native, samples)

    def test_one_and_a_half_blocks_process_one_block_and_flush_2560(self):
        # 7680 = 1.5 个 5120 块：立刻处理 1 块，finish 冲刷剩余 2560。
        native = make_native()
        stream = make_stream(native)
        samples = np.arange(7680, dtype=np.float32)

        stream.feed(samples)
        self.assertEqual(fed_sizes(native), [CHUNK_SAMPLES])
        self.assertEqual(stream._pending.size, 2560)

        stream.finish()

        self.assertEqual(fed_sizes(native), [CHUNK_SAMPLES, 2560])
        self.assertEqual(native.finish.call_count, 1)
        self.assert_samples_conserved(native, samples)

    def test_8080_samples_process_one_block_and_flush_2960(self):
        # 8080 = 5120 + 2960：余量是 2960（1.5 块对应的是 7680）。
        native = make_native()
        stream = make_stream(native)
        samples = np.arange(8080, dtype=np.float32)

        stream.feed(samples)
        self.assertEqual(fed_sizes(native), [CHUNK_SAMPLES])
        self.assertEqual(stream._pending.size, 2960)

        stream.finish()

        self.assertEqual(fed_sizes(native), [CHUNK_SAMPLES, 2960])
        self.assert_samples_conserved(native, samples)

    def test_finish_flushes_pending_exactly_once(self):
        native = make_native()
        stream = make_stream(native)
        samples = feed_in_collector_chunks(stream, 5 * COLLECTOR_CHUNK)

        stream.finish()

        self.assertEqual(native.finish.call_count, 1)
        self.assertEqual(fed_sizes(native), [CHUNK_SAMPLES, CHUNK_SAMPLES, COLLECTOR_CHUNK])
        with self.assertRaises(ValueError):
            stream.finish()
        self.assertEqual(native.finish.call_count, 1)
        self.assert_samples_conserved(native, samples)

    def test_submitted_text_is_append_only_across_collector_chunks(self):
        native = make_native(deltas=("甲", "乙", "丙"))
        stream = SegmentedR2T2(native, AlwaysSpeechVad(), chunk_samples=CHUNK_SAMPLES)
        stream.start("", None)
        samples = np.arange(5 * COLLECTOR_CHUNK, dtype=np.float32)

        committed = []
        for offset in range(0, samples.size, COLLECTOR_CHUNK):
            text, _language = stream.feed(samples[offset : offset + COLLECTOR_CHUNK])
            committed.append(text)

        for previous, current in zip(committed, committed[1:]):
            self.assertTrue(
                current.startswith(previous),
                f"提交文本回退：{previous!r} -> {current!r}",
            )

        final_text, _language = stream.finish()
        self.assertEqual(final_text, "甲乙丙")
        self.assertTrue(final_text.startswith(committed[-1]))
        self.assertEqual(fed_sizes(native), [CHUNK_SAMPLES, CHUNK_SAMPLES, COLLECTOR_CHUNK])


class ServerCollectorAggregationTests(unittest.TestCase):
    """契约 2/4 的生产接缝：_load_runtime -> initialize -> SegmentedR2T2 -> native.feed。"""

    @staticmethod
    def _server_shell():
        server = object.__new__(R2T2ASRServer)
        server.engine = "confucius4-r2t2"
        server.backend = "cuda"
        server.device = "cuda"
        server.initialized = False
        server._gpu_suspended = False
        server.native = None
        server.vad_model = None
        server.segmented = None
        server.stream = None
        server.stdout_suppressor = Mock()
        server.stdout_suppressor.suppress.side_effect = contextlib.nullcontext
        server.logger = Mock()
        return server

    def test_initialize_hands_cuda_input_grain_to_segmented_stream(self):
        server = self._server_shell()
        with (
            patch.object(server, "_resolve_model_path", return_value="verified-model"),
            patch.object(server, "_get_gpu_device_info", return_value={"device": "cuda"}),
            patch.object(r2t2_asr_server, "NativeRuntime", DerivingNative),
            patch.object(r2t2_asr_server, "FireRedVad", return_value=Mock()),
            patch.object(r2t2_asr_server, "SegmentedR2T2") as segmented,
            patch.object(r2t2_asr_server, "R2T2StreamSession"),
        ):
            result = server.initialize()

        self.assertTrue(result["success"])
        self.assertEqual(server.native.chunk_ms, 160)
        self.assertEqual(server.native.chunk_samples, COLLECTOR_CHUNK)
        self.assertEqual(segmented.call_args.kwargs.get("chunk_samples"), COLLECTOR_CHUNK)
        self.assertIs(server.segmented, segmented.return_value)

    def test_cuda_collector_chunks_reach_native_individually(self):
        server = R2T2ASRServer.__new__(R2T2ASRServer)
        with patch.object(r2t2_asr_server, "NativeRuntime", DerivingNative):
            server._load_runtime("verified-model")

        native = server.native
        native.feeds.clear()

        # initialize() 用同一表达式把 native.chunk_samples 交给 SegmentedR2T2。
        stream = SegmentedR2T2(
            native,
            AlwaysSpeechVad(),
            chunk_samples=native.chunk_samples,
        )
        stream.start("", None)
        samples = feed_in_collector_chunks(stream, 2 * COLLECTOR_CHUNK + 17)
        self.assertEqual([chunk.size for chunk in native.feeds], [COLLECTOR_CHUNK] * 2)
        stream.finish()

        self.assertEqual([int(chunk.size) for chunk in native.feeds], [COLLECTOR_CHUNK, COLLECTOR_CHUNK, 17])
        np.testing.assert_array_equal(np.concatenate(native.feeds), samples)
        self.assertEqual(native.finish_calls, 1)
        self.assertEqual(native.chunk_samples, COLLECTOR_CHUNK)


class NativeDecodeCadenceTests(unittest.TestCase):
    def test_real_cuda_decodes_startup_grains_then_pairs_until_segment_end(self):
        """Opt in with LIGHT_WHISPER_R2T2_CADENCE_{MODEL,LIBRARY,DLL_DIR,WAV}."""
        names = ("MODEL", "LIBRARY", "DLL_DIR", "WAV")
        paths = [os.environ.get("LIGHT_WHISPER_R2T2_CADENCE_" + name) for name in names]
        if not any(paths):
            self.skipTest("Real CUDA cadence fixture is opt-in")
        self.assertTrue(all(paths), f"Specify all four paths: {names}")
        model, library, dll_dir, wav = paths
        with wave.open(wav, "rb") as source:
            self.assertEqual((source.getframerate(), source.getnchannels(), source.getsampwidth()), (16000, 1, 2))
            pcm = np.frombuffer(source.readframes(source.getnframes()), dtype="<i2").astype(np.float32) / 32768

        native = NativeRuntime(model, library, backend="cuda", rolling=True, chunk_ms=160,
                               adaptive_cadence=True, dll_directories=(dll_dir,))
        self.addCleanup(native.close)

        # The DLL returns a Transcript for each decode, including an empty-text
        # decode. A buffered push has no Transcript (status 7). Wrap the read
        # without changing its result so repeated text can still reveal a decode.
        read_preview = native.functions["result_preview_text"]
        for ending in ("reset", "finish", "finish"):
            native.start()
            seen_preview = False
            after_preview = 0
            statuses = []

            def observe(*args):
                status = read_preview(*args)
                statuses.append(status)
                return status

            with patch.dict(native.functions, result_preview_text=observe):
                for offset in range(0, pcm.size - COLLECTOR_CHUNK + 1, COLLECTOR_CHUNK):
                    statuses.clear()
                    had_preview = seen_preview
                    native.feed(pcm[offset:offset + COLLECTOR_CHUNK])
                    expected_decode = not had_preview or after_preview % 2 == 1
                    self.assertEqual(statuses, [0 if expected_decode else 7],
                                     f"{ending}, offset={offset}, preview seen={had_preview}")
                    seen_preview |= bool(native.preview_text)
                    if had_preview:
                        after_preview += 1
                        if after_preview == 3:
                            break
            self.assertEqual(after_preview, 3, "Fixture needs speech and three full grains after the first preview")
            if ending == "reset":
                native.reset()
            else:
                committed, _ = native.finish()
                self.assertIsInstance(committed, str)
            self.assertEqual(native.preview_text, "")


if __name__ == "__main__":
    unittest.main()
