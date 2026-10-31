"""CPU/native overlap and full-capture Stop scheduling; no models or GPU."""

import gc
import os
import queue
import sys
import threading
import time
from types import SimpleNamespace
import unittest
import weakref

import numpy as np

sys.path.insert(0, os.path.dirname(__file__))

import qwen3_native


TIMEOUT = 5  # Deadlock guard; event ordering, rather than elapsed time, is tested.


class _Call:
    def __init__(self, function):
        self.function = function
        self.done = threading.Event()
        self.value = None
        self.error = None

    def result(self):
        if not self.done.wait(TIMEOUT):
            raise AssertionError("stream caller did not acknowledge operation")
        if self.error is not None:
            raise self.error
        return self.value


class _StreamCaller:
    """Keep every runtime operation on one caller, even while testing a block."""

    def __init__(self):
        self.calls = queue.Queue()
        self.thread = threading.Thread(target=self._run, daemon=True)
        self.thread.start()

    def _run(self):
        while True:
            call = self.calls.get()
            if call is None:
                return
            try:
                call.value = call.function()
            except BaseException as error:
                call.error = error
            finally:
                call.done.set()

    def submit(self, function):
        call = _Call(function)
        self.calls.put(call)
        return call

    def close(self):
        self.calls.put(None)
        self.thread.join(TIMEOUT)
        if self.thread.is_alive():
            raise AssertionError("stream caller did not join after releasing native gates")


class _PrepareAudio:
    def __init__(self, full_samples=None, *, block_full=False):
        self.calls = []
        self.prepared_refs = []
        self.threads = set()
        self.condition = threading.Condition()
        self.full_samples = full_samples
        self.full_prepared = threading.Event()
        self.release_full = threading.Event()
        if not block_full:
            self.release_full.set()

    def __call__(self, pcm):
        prepared = np.ascontiguousarray(pcm * np.float32(0.5))
        def released(_reference):
            with self.condition:
                self.condition.notify_all()

        with self.condition:
            self.calls.append((pcm.copy(), threading.get_ident()))
            self.threads.add(threading.current_thread())
            self.prepared_refs.append(weakref.ref(prepared, released))
            self.condition.notify_all()
        if len(pcm) == self.full_samples:
            self.full_prepared.set()
            if not self.release_full.wait(2 * TIMEOUT):
                raise RuntimeError("test did not release full CPU preparation gate")
        return prepared

    def wait_prepared(self, count):
        with self.condition:
            if not self.condition.wait_for(lambda: len(self.calls) >= count, TIMEOUT):
                raise AssertionError(f"CPU preparation {count} never entered")

    def wait_released(self, indices):
        with self.condition:
            if not self.condition.wait_for(
                lambda: all(self.prepared_refs[index]() is None for index in indices), TIMEOUT
            ):
                raise AssertionError("superseded prepared arrays remain retained")


class _GatedTranscribe:
    def __init__(self, *, block_calls=(1,), outputs=None):
        self.calls = []
        self.outputs = outputs or (("draft", "Chinese"), ("final", None))
        self.entered = [threading.Event() for _ in range(4)]
        self.finished = [threading.Event() for _ in range(4)]
        self.release = {index: threading.Event() for index in block_calls}
        self.lock = threading.Lock()
        self.active = 0
        self.max_active = 0

    def __call__(self, pcm, **options):
        with self.lock:
            self.calls.append((pcm.copy(), options, threading.get_ident()))
            index = len(self.calls)
            self.active += 1
            self.max_active = max(self.max_active, self.active)
        try:
            if index <= len(self.entered):
                self.entered[index - 1].set()
            gate = self.release.get(index)
            if gate is not None and not gate.wait(2 * TIMEOUT):
                raise RuntimeError("test did not release native decode gate")
            text, language = self.outputs[min(index - 1, len(self.outputs) - 1)]
            return SimpleNamespace(text=text, language=language)
        finally:
            with self.lock:
                self.active -= 1
            if index <= len(self.finished):
                self.finished[index - 1].set()

    def unblock(self):
        for gate in self.release.values():
            gate.set()


class QwenPipelineTests(unittest.TestCase):
    def setUp(self):
        self.caller = None
        self.runtime = None
        self.native = None
        self.prepare = None
        self.addCleanup(self._cleanup)

    def _cleanup(self):
        if self.prepare is not None:
            self.prepare.release_full.set()
        if self.native is not None:
            self.native.unblock()
        try:
            if self.runtime is not None:
                self.caller.submit(self.runtime.close).result()
        finally:
            if self.caller is not None:
                self.caller.close()

    def make_runtime(self, native, prepare, **options):
        runtime_type = getattr(qwen3_native, "QwenPipelineRuntime", None)
        self.assertIsNotNone(runtime_type, "INTERFACE_PENDING: QwenPipelineRuntime is absent")
        self.assertTrue(issubclass(runtime_type, qwen3_native.QwenPreviewRuntime))
        self.native = native
        self.prepare = prepare
        self.caller = _StreamCaller()
        self.runtime = self.caller.submit(
            lambda: runtime_type(native, prepare_audio=prepare, **options)
        ).result()
        self.caller.submit(lambda: self.runtime.start("mixed 术语", "zh")).result()
        return self.runtime

    def assert_prepare(self, call, captured):
        audio, thread_id = call
        expected = np.pad(captured, (0, max(0, 8000 - len(captured))))
        np.testing.assert_array_equal(audio, expected)
        self.assertEqual(audio.dtype, np.float32)
        self.assertTrue(audio.flags.c_contiguous)
        self.assertNotEqual(thread_id, self.caller.thread.ident)
        self.assertEqual(len(self.prepare.threads), 1, "one persistent CPU owner must prepare all audio")
        self.assertNotIn(thread_id, {call[2] for call in self.native.calls})

    def assert_native(self, call, captured):
        audio, options, thread_id = call
        expected = np.pad(captured, (0, max(0, 8000 - len(captured))))
        np.testing.assert_array_equal(audio, expected * np.float32(0.5))
        self.assertEqual(audio.dtype, np.float32)
        self.assertTrue(audio.flags.c_contiguous)
        self.assertEqual(options, {"context": "mixed 术语", "language": "zh"})
        self.assertNotEqual(thread_id, self.caller.thread.ident)

    def test_feed_acknowledges_during_native_and_only_latest_pending_runs(self):
        native = _GatedTranscribe(block_calls=(1, 2),
                                  outputs=(("old draft", "Chinese"), ("latest draft", None)))
        prepare = _PrepareAudio()
        runtime = self.make_runtime(native, prepare, window_samples=19200)
        pcm = np.linspace(-0.8, 0.8, 19200, dtype=np.float32)

        first = self.caller.submit(lambda: runtime.feed(pcm[:3200]))
        self.assertTrue(native.entered[0].wait(TIMEOUT), "first preview never started")
        self.assertEqual(first.result(), ("", "zh"))
        self.assertFalse(native.release[1].is_set(), "ACK requires no native completion")

        for start in range(3200, len(pcm), 3200):
            borrowed = pcm[start:start + 3200].copy()
            self.assertEqual(self.caller.submit(lambda: runtime.feed(borrowed)).result(), ("", "zh"))
            borrowed.fill(1)  # Feed already owns raw PCM and its prepared pending work.
            prepare.wait_prepared(start // 3200 + 1)
            gc.collect()
            prepare.wait_released(range(1, len(prepare.prepared_refs) - 1))
            live = {index for index, reference in enumerate(prepare.prepared_refs)
                    if reference() is not None}
            latest = len(prepare.prepared_refs) - 1
            self.assertLessEqual(live, {0, latest},
                                 "superseded prepared arrays must not accumulate in a queue")
        self.assertEqual(len(prepare.calls), 6, "CPU preparation must run while native is blocked")
        self.assertEqual(len(native.calls), 1)
        for call, count in zip(prepare.calls, range(3200, len(pcm) + 1, 3200)):
            self.assert_prepare(call, pcm[:count])

        native.release[1].set()
        self.assertTrue(native.entered[1].wait(TIMEOUT), "latest pending preview never started")
        self.assertEqual(len(native.calls), 2)
        self.assert_native(native.calls[0], pcm[:3200])
        self.assert_native(native.calls[1], pcm)
        final = self.caller.submit(runtime.finish)
        self.assertFalse(final.done.is_set(), "finish must wait for its exact active preview")
        native.release[2].set()
        self.assertEqual(final.result(), ("latest draft", "Chinese"))
        self.assertEqual(len(native.calls), 2, "exact full preview should be reused")
        self.assertEqual(native.max_active, 1)
        self.assertEqual(len({call[2] for call in native.calls}), 1,
                         "all native calls must use one persistent worker")

    def test_finish_prepares_complete_pcm_during_native_and_cancels_pending_preview(self):
        native = _GatedTranscribe(block_calls=(1, 2),
                                  outputs=(("window draft", "Chinese"), ("complete final", None)))
        prepare = _PrepareAudio(full_samples=12813, block_full=True)
        runtime = self.make_runtime(native, prepare, window_samples=9600)
        pcm = np.linspace(-0.9, 0.9, 12813, dtype=np.float32)
        first = self.caller.submit(lambda: runtime.feed(pcm[:3200]))
        self.assertTrue(native.entered[0].wait(TIMEOUT))
        first.result()
        self.caller.submit(lambda: runtime.feed(pcm[3200:12800])).result()
        prepare.wait_prepared(2)
        tail = pcm[12800:].copy()
        self.caller.submit(lambda: runtime.feed(tail, preview=False)).result()
        tail.fill(0)

        final = self.caller.submit(runtime.finish)
        self.assertTrue(prepare.full_prepared.wait(TIMEOUT),
                        "full CPU preparation must overlap the active native preview")
        self.assertFalse(native.release[1].is_set())
        self.assertFalse(final.done.is_set())
        self.assertEqual(len(native.calls), 1)
        self.assertEqual(len(prepare.calls), 3)
        self.assert_prepare(prepare.calls[0], pcm[:3200])
        self.assert_prepare(prepare.calls[1], pcm[3200:12800])
        self.assert_prepare(prepare.calls[2], pcm)
        gc.collect()
        self.assertIsNone(prepare.prepared_refs[1](),
                          "Stop must release obsolete pending PCM before full CPU preparation returns")

        native.release[1].set()
        self.assertTrue(native.finished[0].wait(TIMEOUT),
                        "active native preview must finish while full CPU preparation remains blocked")
        self.assertFalse(prepare.release_full.is_set())
        self.assertFalse(final.done.is_set())
        prepare.release_full.set()
        self.assertTrue(native.entered[1].wait(TIMEOUT))
        self.assert_native(native.calls[1], pcm)
        native.release[2].set()
        self.assertEqual(final.result(), ("complete final", "Chinese"))
        self.assertEqual(len(native.calls), 2, "Stop must bypass obsolete prepared previews")
        self.assert_native(native.calls[0], pcm[:3200])
        self.assert_native(native.calls[1], pcm)
        self.assertEqual(native.max_active, 1)
        self.assertEqual(len({call[2] for call in native.calls}), 1)

    def test_exact_capture_count_cannot_reuse_a_truncated_preview_as_final(self):
        native = _GatedTranscribe(outputs=(("last window", "Chinese"), ("whole recording", None)))
        prepare = _PrepareAudio()
        runtime = self.make_runtime(native, prepare, window_samples=9600)
        pcm = np.linspace(-0.7, 0.7, 9601, dtype=np.float32)
        first = self.caller.submit(lambda: runtime.feed(pcm))
        self.assertTrue(native.entered[0].wait(TIMEOUT))
        first.result()
        self.assert_native(native.calls[0], pcm[-9600:])

        final = self.caller.submit(runtime.finish)
        native.release[1].set()
        self.assertEqual(final.result(), ("whole recording", "Chinese"))
        self.assertEqual(len(native.calls), 2)
        self.assert_prepare(prepare.calls[-1], pcm)
        self.assert_native(native.calls[-1], pcm)

    def test_startup_then_steady_cadence_and_caller_published_caption(self):
        native = _GatedTranscribe(block_calls=(1, 2),
                                  outputs=(("first caption", "Chinese"), ("revised caption", None)))
        prepare = _PrepareAudio()
        runtime = self.make_runtime(native, prepare)
        pcm = np.linspace(-0.4, 0.4, 5120, dtype=np.float32)
        self.assertEqual(self.caller.submit(lambda: runtime.feed(pcm[:1919])).result(), ("", "zh"))
        self.assertEqual(prepare.calls, [])
        first = self.caller.submit(lambda: runtime.feed(pcm[1919:1920]))
        self.assertTrue(native.entered[0].wait(TIMEOUT))
        first.result()
        self.assertEqual(self.caller.submit(lambda: runtime.preview_text).result(), "")
        native.release[1].set()

        def await_caption():
            deadline = time.monotonic() + TIMEOUT
            while time.monotonic() < deadline:
                caption = runtime.preview_text
                if caption == "first caption":
                    return caption
                threading.Event().wait(0.001)
            raise AssertionError("caller never published the completed preview")

        self.assertEqual(self.caller.submit(await_caption).result(), "first caption")
        self.assertEqual(self.caller.submit(lambda: runtime.feed(pcm[1920:5119])).result(), ("", "Chinese"))
        self.assertEqual(len(prepare.calls), 1)
        second = self.caller.submit(lambda: runtime.feed(pcm[5119:]))
        self.assertTrue(native.entered[1].wait(TIMEOUT))
        second.result()
        self.assertEqual(len(prepare.calls), 2)
        self.assert_prepare(prepare.calls[0], pcm[:1920])
        self.assert_prepare(prepare.calls[1], pcm)
        self.assert_native(native.calls[0], pcm[:1920])
        self.assert_native(native.calls[1], pcm)
        final = self.caller.submit(runtime.finish)
        native.release[2].set()
        self.assertEqual(final.result(), ("revised caption", "Chinese"))
        self.assertEqual(len(native.calls), 2)


if __name__ == "__main__":
    unittest.main()
