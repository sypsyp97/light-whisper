"""Native event previews must survive empty committed deltas and corrections."""
import ctypes as ct
import unittest
from types import SimpleNamespace
from unittest.mock import Mock

import numpy as np

from r2t2_native import NativeRuntime


class NativePreviewTests(unittest.TestCase):
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
