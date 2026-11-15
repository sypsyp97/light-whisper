
import ctypes as ct
import unittest
from types import SimpleNamespace
from unittest.mock import Mock

import numpy as np

from r2t2_native import NativeRuntime


class NativePreviewTests(unittest.TestCase):
    def make_finishing_runtime(self, chunk_samples=2560, captured_samples=9000):
        runtime = NativeRuntime.__new__(NativeRuntime)
        runtime._active = True
        runtime._offset = captured_samples
        runtime._committed = "之前有顾客" if captured_samples else ""
        runtime._preview_text = runtime._committed
        runtime._language = "Chinese"
        runtime.session = ct.c_void_p(1)
        runtime.chunk_samples = chunk_samples
        pushes = []
        calls = []

        def call(name, *args):
            calls.append(name)
            if name == "stream_push":
                pcm = np.ctypeslib.as_array(args[1], shape=(args[2],)).copy()
                pushes.append((args[5], pcm))
            elif name == "stream_finish":
                args[-1]._obj.value = 2

        runtime._call = Mock(side_effect=call)
        runtime._read_text = Mock(side_effect=lambda result: (
            ("之前有顾客自己带" if sum(len(pcm) for _, pcm in pushes) >= 5120 else runtime._committed),
            "Chinese",
        ))
        runtime.functions = {"event_free": Mock(), "result_free": Mock()}
        return runtime, pushes, calls

    def test_finish_decodes_sentence_tail_without_waiting_for_new_microphone_audio(self):
        for chunk_samples in [2560, 5120]:
            with self.subTest(chunk_samples=chunk_samples):
                runtime, pushes, calls = self.make_finishing_runtime(chunk_samples)
                self.assertEqual(runtime.finish(), ("之前有顾客自己带", "Chinese"))
                self.assertEqual(sum(len(pcm) for _, pcm in pushes), 5120)
                offset = 9000
                for actual_offset, pcm in pushes:
                    self.assertEqual(actual_offset, offset)
                    self.assertLessEqual(len(pcm), chunk_samples)
                    np.testing.assert_array_equal(pcm, np.zeros(len(pcm), dtype=np.float32))
                    offset += len(pcm)
                self.assertEqual(calls[-1], "stream_finish")
                self.assertFalse(runtime._active)
                self.assertEqual(runtime.preview_text, "")
                runtime.functions["result_free"].assert_called_once()

    def test_empty_finish_does_not_invent_audio(self):
        runtime, pushes, calls = self.make_finishing_runtime(captured_samples=0)
        self.assertEqual(runtime.finish(), ("", "Chinese"))
        self.assertEqual(pushes, [])
        self.assertEqual(calls, ["stream_finish"])

    def test_finish_does_not_publish_a_stale_result_after_flush_failure(self):
        runtime, _, calls = self.make_finishing_runtime()
        original_call = runtime._call.side_effect
        def fail_push(name, *args):
            if name == "stream_push":
                raise RuntimeError("audio flush failed")
            return original_call(name, *args)
        runtime._call.side_effect = fail_push
        with self.assertRaisesRegex(RuntimeError, "audio flush failed"):
            runtime.finish()
        runtime._read_text.assert_not_called()
        self.assertEqual(calls, [])
        self.assertEqual([call.args[0] for call in runtime._call.call_args_list], ["stream_push"])

    def test_rejects_old_abi_before_resolving_new_symbols(self):
        runtime = NativeRuntime.__new__(NativeRuntime)
        runtime.functions = {}
        version = Mock(return_value=0x0200)
        runtime.lib = SimpleNamespace(audiocpp_abi_version=version)
        with self.assertRaisesRegex(RuntimeError, "Unsupported audio.cpp ABI"):
            runtime._bind()
        version.assert_called_once()

    def test_partial_audio_without_a_decoded_event_retains_preview(self):
        runtime = NativeRuntime.__new__(NativeRuntime)
        runtime._active = True
        runtime._committed = "明天"
        runtime._preview_text = "明天去上海"
        runtime._language = "Chinese"
        runtime._offset = 0
        runtime.session = ct.c_void_p(1)
        runtime.chunk_samples = 5120

        def push(*args):
            args[-1]._obj.value = 2
            return 0

        runtime.functions = {
            "stream_push": push, "event_as_result": lambda event: event,
            "result_text": lambda *args: 7, "result_preview_text": lambda *args: 7,
            "event_free": Mock(), "last_error": lambda: b"no text output",
        }
        self.assertEqual(runtime.feed(np.zeros(1, dtype=np.float32))[0], "明天")
        self.assertEqual(runtime.preview_text, "明天去上海")

    def test_reads_latest_preview_even_when_committed_delta_is_empty(self):
        runtime = NativeRuntime.__new__(NativeRuntime)
        runtime._active = True
        runtime._committed = ""
        runtime._preview_text = ""
        runtime._language = None
        runtime._offset = 0
        runtime.session = ct.c_void_p(1)
        runtime.chunk_samples = 5120
        snapshots = iter([("", "明天去上海"), ("明天去", "明天去上班"), ("", "明天去")])
        current = [None]

        def push(*args):
            current[0] = next(snapshots)
            args[-1]._obj.value = 2
            return 0

        def read_text(result, text, language):
            text._obj.value = current[0][0].encode()
            language._obj.value = b"Chinese"
            return 0

        def read_preview(result, text):
            text._obj.value = current[0][1].encode()
            return 0

        preview_reader = Mock(side_effect=read_preview)
        freed = Mock()
        runtime.functions = {
            "stream_push": push, "event_as_result": lambda event: event,
            "result_text": read_text, "result_preview_text": preview_reader,
            "event_free": freed,
        }
        for committed, preview in [("", "明天去上海"), ("明天去", "明天去上班"), ("明天去", "明天去")]:
            self.assertEqual(runtime.feed(np.zeros(5120, dtype=np.float32))[0], committed)
            preview_reader.assert_called()
            self.assertEqual(runtime.preview_text, preview)
        self.assertEqual(preview_reader.call_count, 3)
        self.assertEqual(freed.call_count, 3)


if __name__ == "__main__":
    unittest.main()
