"""Feed and Stop must stay responsive while the persistent CPU owner is busy."""

from concurrent.futures import Future
import gc
import threading
import tracemalloc
import unittest
from unittest import mock

import numpy as np

from qwen3_native import QwenPipelineRuntime, QwenPreviewRuntime
from test_qwen_pipeline import TIMEOUT, _PrepareAudio, _StreamCaller
from test_qwen_pipeline_lifecycle import _ObservedNative


class _BlockedPreparation(_PrepareAudio):
    def __init__(self, blocked_call=1, *, fail=False, full_samples=None, block_full=False):
        super().__init__(full_samples=full_samples, block_full=block_full)
        self.blocked_call = blocked_call
        self.fail = fail
        self.entered = threading.Event()
        self.release = threading.Event()

    def __call__(self, pcm):
        prepared = super().__call__(pcm)
        if len(self.calls) == self.blocked_call:
            self.entered.set()
            if not self.release.wait(2 * TIMEOUT):
                raise RuntimeError("test did not release CPU preparation")
            if self.fail:
                raise RuntimeError("injected CPU preparation failure")
        return prepared


class QwenPipelinePreparationTests(unittest.TestCase):
    def pipeline(self, prepare, native, *, window_samples=19200):
        caller = _StreamCaller()
        self.addCleanup(caller.close)
        runtime = caller.submit(lambda: QwenPipelineRuntime(
            native, prepare_audio=prepare, window_samples=window_samples
        )).result()
        self.addCleanup(lambda: caller.submit(runtime.close).result())
        self.addCleanup(native.unblock)
        self.addCleanup(prepare.release.set)
        self.addCleanup(prepare.release_full.set)
        caller.submit(lambda: runtime.start("mixed 术语", "zh")).result()
        return caller, runtime

    def test_feed_acks_while_cpu_is_blocked_and_only_latest_raw_snapshot_is_prepared(self):
        prepare = _BlockedPreparation()
        native = _ObservedNative(outputs=(("complete", "Chinese"),))
        caller, runtime = self.pipeline(prepare, native)
        pcm = np.linspace(-.8, .8, 19200, dtype=np.float32)
        first_pcm = pcm[:3200].copy()
        first = caller.submit(lambda: runtime.feed(first_pcm))
        self.assertTrue(prepare.entered.wait(TIMEOUT))
        self.assertEqual(first.result(), ("", "zh"))
        first_pcm.fill(1)
        self.assertFalse(prepare.release.is_set())
        for offset in range(3200, len(pcm), 3200):
            borrowed = pcm[offset:offset + 3200].copy()
            self.assertEqual(caller.submit(lambda: runtime.feed(borrowed)).result(), ("", "zh"))
            borrowed.fill(1)
        self.assertEqual(len(prepare.calls), 1)
        self.assertEqual(native.calls, [])
        prepare.release.set()
        prepare.wait_prepared(2)
        self.assertTrue(native.entered[0].wait(TIMEOUT))
        self.assertEqual(len(prepare.calls), 2, "intermediate raw snapshots must never reach VAD")
        np.testing.assert_array_equal(prepare.calls[0][0], np.pad(pcm[:3200], (0, 4800)))
        np.testing.assert_array_equal(prepare.calls[1][0], pcm)
        native.release[1].set()
        self.assertEqual(caller.submit(runtime.finish).result(), ("complete", "Chinese"))
        np.testing.assert_array_equal(native.calls[-1][0], pcm * np.float32(.5))
        self.assertLessEqual(len(native.calls), 2)
        self.assertEqual(len(prepare.threads), 1)
        self.assertNotIn(caller.thread, prepare.threads)
        self.assertTrue(prepare.threads.isdisjoint(native.threads))

    def test_stop_during_cpu_preview_drops_its_native_work_and_prioritizes_complete_final(self):
        prepare = _BlockedPreparation(blocked_call=2, full_samples=16013, block_full=True)
        native = _ObservedNative(block_calls=(1, 2),
                                 outputs=(("obsolete", "Chinese"), ("complete final", None)))
        caller, runtime = self.pipeline(prepare, native)
        pcm = np.linspace(-.9, .9, 16013, dtype=np.float32)
        caller.submit(lambda: runtime.feed(pcm[:3200])).result()
        self.assertTrue(native.entered[0].wait(TIMEOUT))
        second = caller.submit(lambda: runtime.feed(pcm[3200:12800]))
        self.assertTrue(prepare.entered.wait(TIMEOUT))
        self.assertEqual(second.result(), ("", "zh"))
        caller.submit(lambda: runtime.feed(pcm[12800:16000])).result()
        tail = pcm[16000:].copy()
        caller.submit(lambda: runtime.feed(tail, preview=False)).result()
        tail.fill(0)
        priority_established = threading.Event()
        real_cancel = Future.cancel

        def cancel(future):
            cancelled = real_cancel(future)
            if cancelled and threading.current_thread() is caller.thread:
                priority_established.set()
            return cancelled

        with mock.patch.object(Future, "cancel", new=cancel):
            final = caller.submit(runtime.finish)
            self.assertTrue(priority_established.wait(TIMEOUT),
                            "Stop must cancel queued work before active CPU preparation returns")
            self.assertFalse(prepare.release.is_set())
            native.release[1].set()
            self.assertTrue(native.finished[0].wait(TIMEOUT))
            self.assertFalse(final.done.is_set())
            prepare.release.set()
            self.assertTrue(prepare.full_prepared.wait(TIMEOUT))
            prepare.wait_released((1,))
            self.assertEqual(len(native.calls), 1,
                             "obsolete CPU2 output cannot enter native while full preparation is held")
            self.assertFalse(prepare.release_full.is_set())
            prepare.release_full.set()
            self.assertTrue(native.entered[1].wait(TIMEOUT))
        self.assertEqual(len(prepare.calls), 3, "queued raw preview must be discarded at Stop")
        np.testing.assert_array_equal(prepare.calls[-1][0], pcm)
        np.testing.assert_array_equal(native.calls[1][0], pcm * np.float32(.5))
        self.assertEqual(native.calls[1][1], {"context": "mixed 术语", "language": "zh"})
        native.release[2].set()
        self.assertEqual(final.result(), ("complete final", "Chinese"))
        self.assertEqual(len(native.calls), 2, "active CPU preview must never become a late native preview")
        self.assertEqual(len(prepare.threads), 1)
        self.assertTrue(prepare.threads.isdisjoint(native.threads))

    def test_cpu_preparation_failure_skips_native_cancels_pending_and_reset_allows_retry(self):
        prepare = _BlockedPreparation(fail=True)
        native = _ObservedNative(block_calls=(), outputs=(("recovered", None),))
        caller, runtime = self.pipeline(prepare, native)
        first = caller.submit(lambda: runtime.feed(np.ones(1920, np.float32)))
        self.assertTrue(prepare.entered.wait(TIMEOUT))
        self.assertEqual(first.result(), ("", "zh"))
        caller.submit(lambda: runtime.feed(np.ones(3200, np.float32))).result()
        published, pending_cancelled = threading.Event(), threading.Event()
        real_set_exception, real_cancel = Future.set_exception, Future.cancel

        def set_exception(future, error):
            real_set_exception(future, error)
            if threading.current_thread() in prepare.threads:
                published.set()

        def cancel(future):
            cancelled = real_cancel(future)
            if cancelled and threading.current_thread() in prepare.threads:
                pending_cancelled.set()
            return cancelled

        with (mock.patch.object(Future, "set_exception", new=set_exception),
              mock.patch.object(Future, "cancel", new=cancel)):
            prepare.release.set()
            self.assertTrue(published.wait(TIMEOUT), "real Future must publish the CPU error")
            self.assertTrue(pending_cancelled.wait(TIMEOUT),
                            "CPU failure must cancel queued raw work before any caller cleanup")
            self.assertEqual(len(prepare.calls), 1)
            self.assertEqual(native.calls, [])
            with self.assertRaisesRegex(RuntimeError, "injected CPU preparation failure"):
                caller.submit(lambda: runtime.preview_text).result()
        self.assertEqual(native.calls, [])
        caller.submit(runtime.reset).result()
        self.assertEqual(caller.submit(lambda: runtime.preview_text).result(), "")
        caller.submit(lambda: runtime.start("fresh context", "en")).result()
        fresh = np.full(13, -.5, np.float32)
        caller.submit(lambda: runtime.feed(fresh, preview=False)).result()
        self.assertEqual(caller.submit(runtime.finish).result(), ("recovered", "en"))
        self.assertEqual(len(prepare.calls), 2)
        self.assertEqual(len(native.calls), 1)
        np.testing.assert_array_equal(native.calls[0][0], np.pad(fresh, (0, 7987)) * np.float32(.5))
        self.assertEqual(native.calls[0][1], {"context": "fresh context", "language": "en"})
        caller.submit(runtime.close).result()
        self.assertTrue(all(not thread.is_alive() for thread in prepare.threads | native.threads))

    def test_blocked_cpu_retains_only_bounded_extra_numpy_audio_buffers(self):
        domain = getattr(np.lib, "tracemalloc_domain", None)
        if domain is None:
            self.skipTest("Infrastructure: installed NumPy exposes no tracemalloc domain")
        window_samples, feed_samples, feed_count = 192000, 3200, 32
        initial = np.linspace(-.8, .8, window_samples, dtype=np.float32)
        chunks = [np.full(feed_samples, index / feed_count, np.float32)
                  for index in range(feed_count)]
        window_bytes = initial.nbytes
        if not tracemalloc.is_tracing():
            tracemalloc.start()
            self.addCleanup(tracemalloc.stop)

        def numpy_bytes():
            snapshot = tracemalloc.take_snapshot()
            return sum(trace.size for trace in snapshot.filter_traces(
                [tracemalloc.DomainFilter(True, domain)]
            ).traces)

        before_probe = numpy_bytes()
        probe = np.empty(window_samples, dtype=np.float32)
        self.assertEqual(numpy_bytes() - before_probe, probe.nbytes,
                         "Infrastructure: NumPy domain must account for an actual buffer allocation")
        del probe
        gc.collect()

        baseline = QwenPreviewRuntime(lambda *_args, **_options: self.fail("suppressed baseline decoded"),
                                      window_samples=window_samples)
        baseline.start("mixed 术语", "zh")
        before_baseline = numpy_bytes()
        baseline.feed(initial, preview=False)
        for chunk in chunks:
            baseline.feed(chunk, preview=False)
        captured_and_window_bytes = numpy_bytes() - before_baseline
        baseline.reset()
        gc.collect()

        prepare = _BlockedPreparation()
        native = _ObservedNative()
        caller, runtime = self.pipeline(prepare, native, window_samples=window_samples)
        before_pipeline = numpy_bytes()
        first = caller.submit(lambda: runtime.feed(initial))
        self.assertTrue(prepare.entered.wait(TIMEOUT))
        self.assertEqual(first.result(), ("", "zh"))
        for chunk in chunks:
            self.assertEqual(caller.submit(lambda: runtime.feed(chunk)).result(), ("", "zh"))
        gc.collect()
        live_pipeline_bytes = numpy_bytes() - before_pipeline
        # CPU input/result, observer copy, raw pending, prepared pending, and one
        # transient window are the six allowed extra full-window allocations.
        staging_allowance = 6 * window_bytes
        self.assertLessEqual(live_pipeline_bytes - captured_and_window_bytes, staging_allowance,
                             "superseded raw snapshots must release their NumPy buffers")
        self.assertEqual(len(prepare.calls), 1)
        self.assertEqual(native.calls, [])


if __name__ == "__main__":
    unittest.main()
