"""Cancellation, native ownership, and deferred failures without GPU or models."""

from concurrent.futures import Future
import threading
import unittest
from unittest import mock

import numpy as np

from qwen3_native import QwenPipelineRuntime
from r2t2_stream import R2T2StreamSession
from test_qwen_pipeline import TIMEOUT, _GatedTranscribe, _PrepareAudio, _StreamCaller


class _ObservedNative(_GatedTranscribe):
    def __init__(self, *, fail_calls=(), **options):
        super().__init__(**options)
        self.fail_calls = fail_calls
        self.threads = set()

    def __call__(self, pcm, **options):
        self.threads.add(threading.current_thread())
        index = len(self.calls) + 1
        result = super().__call__(pcm, **options)
        if index in self.fail_calls:
            raise RuntimeError("injected native failure")
        return result


class _SecondPreparationGate(_PrepareAudio):
    def __init__(self):
        super().__init__()
        self.second_entered = threading.Event()
        self.release_second = threading.Event()

    def __call__(self, pcm):
        prepared = super().__call__(pcm)
        if len(self.calls) == 2:
            self.second_entered.set()
            if not self.release_second.wait(2 * TIMEOUT):
                raise RuntimeError("test did not release second CPU preparation")
        return prepared


class QwenPipelineLifecycleTests(unittest.TestCase):
    def pipeline(self, native, prepare=None):
        caller = _StreamCaller()
        self.addCleanup(caller.close)
        runtime = caller.submit(lambda: QwenPipelineRuntime(
            native, prepare_audio=prepare or _PrepareAudio()
        )).result()
        self.addCleanup(lambda: caller.submit(runtime.close).result())
        self.addCleanup(native.unblock)
        if isinstance(prepare, _SecondPreparationGate):
            self.addCleanup(prepare.release_second.set)
        return caller, runtime

    def observe_failure(self):
        published = threading.Event()
        real_set_exception = Future.set_exception

        def set_exception(future, error):
            real_set_exception(future, error)
            published.set()

        self.enterContext(mock.patch.object(Future, "set_exception", new=set_exception))
        return published

    def test_reset_cancel_and_replacement_drain_old_work_and_isolate_new_session(self):
        for action in ("reset", "cancel", "replace"):
            with self.subTest(action=action):
                native = _ObservedNative(outputs=(("obsolete", "Chinese"), ("fresh", None)))
                prepare = _SecondPreparationGate()
                caller, runtime = self.pipeline(native, prepare)
                session = R2T2StreamSession(runtime)
                caller.submit(lambda: session.start(1, context="old context", language="zh")).result()
                caller.submit(lambda: session.feed(1, np.full(1920, .75, np.float32), offset=0)).result()
                self.assertTrue(native.entered[0].wait(TIMEOUT))
                caller.submit(lambda: session.feed(1, np.full(3200, .5, np.float32), offset=1920)).result()
                self.assertTrue(prepare.second_entered.wait(TIMEOUT))
                started = threading.Event()

                def change_session():
                    started.set()
                    if action == "reset":
                        runtime.reset()
                    elif action == "cancel":
                        session.cancel(1)
                    else:
                        session.start(2, context="new context", language="en")
                    self.assertEqual(native.active, 0, "old native work must drain before returning")

                changed = caller.submit(change_session)
                self.assertTrue(started.wait(TIMEOUT))
                self.assertFalse(changed.done.is_set())
                native.release[1].set()
                self.assertTrue(native.finished[0].wait(TIMEOUT))
                self.assertFalse(changed.done.is_set(), "session change must also drain active CPU preparation")
                prepare.release_second.set()
                changed.result()
                self.assertEqual(len(native.calls), 1, "obsolete pending preview must be cancelled")
                self.assertEqual(caller.submit(lambda: runtime.preview_text).result(), "")
                if action != "replace":
                    caller.submit(lambda: session.start(2, context="new context", language="en")).result()
                fresh = np.linspace(-.25, .25, 13, dtype=np.float32)
                caller.submit(lambda: session.feed(2, fresh, offset=0, preview=False)).result()
                final = caller.submit(lambda: session.finish(2)).result()
                self.assertEqual((final["text"], final["language"], final["sample_count"]),
                                 ("fresh", "en", 13))
                self.assertEqual(len(native.calls), 2)
                np.testing.assert_array_equal(native.calls[1][0],
                                              np.pad(fresh, (0, 7987)) * np.float32(.5))
                self.assertEqual(native.calls[1][1], {"context": "new context", "language": "en"})
                self.assertEqual(len(prepare.threads), 1, "CPU cache owner must persist across sessions")
                self.assertNotIn(caller.thread, prepare.threads)
                self.assertTrue(prepare.threads.isdisjoint(native.threads))
                caller.submit(runtime.close).result()
                self.assertTrue(all(not thread.is_alive() for thread in native.threads))
                self.assertTrue(all(not thread.is_alive() for thread in prepare.threads))

    def test_close_drains_active_cancels_pending_joins_and_rejects_restart(self):
        native = _ObservedNative()
        prepare = _SecondPreparationGate()
        caller, runtime = self.pipeline(native, prepare)
        caller.submit(runtime.start).result()
        caller.submit(lambda: runtime.feed(np.ones(1920, np.float32))).result()
        self.assertTrue(native.entered[0].wait(TIMEOUT))
        caller.submit(lambda: runtime.feed(np.ones(3200, np.float32))).result()
        self.assertTrue(prepare.second_entered.wait(TIMEOUT))
        started = threading.Event()

        def close():
            started.set()
            runtime.close()
            self.assertTrue(all(not thread.is_alive() for thread in native.threads))
            self.assertTrue(all(not thread.is_alive() for thread in prepare.threads))

        closed = caller.submit(close)
        self.assertTrue(started.wait(TIMEOUT))
        self.assertFalse(closed.done.is_set())
        native.release[1].set()
        self.assertTrue(native.finished[0].wait(TIMEOUT))
        self.assertFalse(closed.done.is_set(), "close must wait for active CPU preparation too")
        prepare.release_second.set()
        closed.result()
        caller.submit(runtime.close).result()
        self.assertEqual(len(native.calls), 1)
        self.assertEqual(caller.submit(lambda: runtime.preview_text).result(), "")
        with self.assertRaises(RuntimeError):
            caller.submit(runtime.start).result()
        with self.assertRaises(RuntimeError):
            caller.submit(lambda: runtime.feed(np.ones(13, np.float32))).result()

    def test_native_failure_during_cpu_prepare_surfaces_before_another_native_submission(self):
        published = self.observe_failure()
        native = _ObservedNative(fail_calls=(1,), outputs=(("unused", None), ("recovered", None)))
        prepare = _SecondPreparationGate()
        caller, runtime = self.pipeline(native, prepare)
        caller.submit(lambda: runtime.start("old context", "zh")).result()
        caller.submit(lambda: runtime.feed(np.ones(1920, np.float32))).result()
        self.assertTrue(native.entered[0].wait(TIMEOUT))
        second = caller.submit(lambda: runtime.feed(np.ones(3200, np.float32)))
        self.assertTrue(prepare.second_entered.wait(TIMEOUT))
        self.assertEqual(second.result(), ("", "zh"))
        native.release[1].set()
        self.assertTrue(published.wait(TIMEOUT), "real Future must publish the native error first")
        self.assertFalse(prepare.release_second.is_set())
        prepare.release_second.set()
        with self.assertRaisesRegex(RuntimeError, "injected native failure"):
            caller.submit(lambda: runtime.preview_text).result()
        self.assertEqual(len(native.calls), 1, "prepared work cannot bypass an unreported native error")
        self.assertEqual(caller.submit(lambda: runtime.preview_text).result(), "")
        caller.submit(lambda: runtime.start("fresh context", "en")).result()
        caller.submit(lambda: runtime.feed(np.full(13, -.5, np.float32), preview=False)).result()
        self.assertEqual(caller.submit(runtime.finish).result(), ("recovered", "en"))
        self.assertEqual(native.calls[-1][1], {"context": "fresh context", "language": "en"})

    def test_native_failure_cancels_queued_preview_without_stale_caption_and_restart_recovers(self):
        published = self.observe_failure()
        native = _ObservedNative(fail_calls=(1,), outputs=(("obsolete", "Chinese"), ("recovered", None)))
        caller, runtime = self.pipeline(native)
        caller.submit(lambda: runtime.start("old context", "zh")).result()
        caller.submit(lambda: runtime.feed(np.ones(1920, np.float32))).result()
        self.assertTrue(native.entered[0].wait(TIMEOUT))
        caller.submit(lambda: runtime.feed(np.ones(3200, np.float32))).result()
        native.release[1].set()
        self.assertTrue(published.wait(TIMEOUT))
        with self.assertRaisesRegex(RuntimeError, "injected native failure"):
            caller.submit(lambda: runtime.preview_text).result()
        self.assertEqual(len(native.calls), 1)
        self.assertEqual(caller.submit(lambda: runtime.preview_text).result(), "")
        with self.assertRaises(RuntimeError):
            caller.submit(runtime.finish).result()
        caller.submit(lambda: runtime.start("fresh context", "en")).result()
        caller.submit(lambda: runtime.feed(np.full(13, -.5, np.float32), preview=False)).result()
        self.assertEqual(caller.submit(runtime.finish).result(), ("recovered", "en"))
        self.assertEqual(native.calls[-1][1], {"context": "fresh context", "language": "en"})

    def test_transient_worker_start_failure_reports_error_and_fresh_session_can_run(self):
        for worker_name in ("qwen3-asr-prepare", "qwen3-asr-native"):
            with self.subTest(worker=worker_name):
                native = _ObservedNative(block_calls=())
                prepare = _PrepareAudio()
                caller, runtime = self.pipeline(native, prepare)
                caller.submit(runtime.start).result()
                real_start = threading.Thread.start
                failed_threads = []
                state = {"recovered": False}

                def fail_once(thread):
                    if thread.name == worker_name and not failed_threads:
                        failed_threads.append(thread)
                        raise OSError("injected thread start failure")
                    return real_start(thread)

                def rescue_failed_test_cleanup(threads=failed_threads, transcribe=native, outcome=state):
                    if not outcome["recovered"]:
                        transcribe.unblock()
                        for thread in threads:
                            if thread.ident is None:
                                real_start(thread)

                self.addCleanup(rescue_failed_test_cleanup)
                with mock.patch.object(threading.Thread, "start", new=fail_once):
                    try:
                        caller.submit(lambda: runtime.feed(np.ones(1920, np.float32))).result()
                    except OSError as error:
                        self.assertIn("injected thread start failure", str(error))
                    else:
                        with self.assertRaisesRegex(OSError, "injected thread start failure"):
                            caller.submit(runtime.finish).result()
                self.assertEqual(len(failed_threads), 1)
                caller.submit(runtime.reset).result()
                caller.submit(lambda: runtime.start("fresh context", "en")).result()
                caller.submit(lambda: runtime.feed(np.full(1920, -.5, np.float32))).result()
                self.assertTrue(native.entered[0].wait(TIMEOUT),
                                "fresh session must create usable workers after Thread.start fails")
                self.assertEqual(caller.submit(runtime.finish).result(), ("draft", "Chinese"))
                caller.submit(runtime.close).result()
                self.assertTrue(all(not thread.is_alive() for thread in native.threads | prepare.threads))
                state["recovered"] = True


if __name__ == "__main__":
    unittest.main()
