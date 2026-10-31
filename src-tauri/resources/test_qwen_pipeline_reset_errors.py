"""Processor cleanup failures must still drain the real Qwen native owner."""

import os
import sys
import threading
import unittest

import numpy as np

sys.path.insert(0, os.path.dirname(__file__))

from qwen3_native import QwenPipelineRuntime
from r2t2_stream import R2T2StreamSession
from test_audio_processing_stream import BufferedProcessor
from test_qwen_pipeline import TIMEOUT, _GatedTranscribe, _StreamCaller


class _FailingResetProcessor(BufferedProcessor):
    def __init__(self):
        super().__init__()
        self.reset_error = None
        self.reset_entered = threading.Event()

    def reset(self):
        if self.reset_error is not None:
            self.reset_entered.set()
            raise self.reset_error
        super().reset()


class QwenPipelineResetErrorTests(unittest.TestCase):
    def test_invalidation_drains_native_even_when_processor_reset_raises(self):
        for action in ("cancel", "close", "replace", "feed_error"):
            with self.subTest(action=action):
                self._assert_reset_failure_drains_native(action)

    def _assert_reset_failure_drains_native(self, action):
        caller = _StreamCaller()
        native = _GatedTranscribe(
            outputs=(("old draft", "zh"), ("fresh final", "en"))
        )
        processor = _FailingResetProcessor()
        runtime = None
        try:
            runtime = caller.submit(
                lambda: QwenPipelineRuntime(native, prepare_audio=lambda pcm: pcm.copy())
            ).result()
            session = R2T2StreamSession(runtime, audio_processor=processor)
            caller.submit(lambda: session.start(1, context="old context", language="zh")).result()
            old_pcm = np.linspace(0.1, 0.8, 6400, dtype=np.float32)
            first = caller.submit(lambda: session.feed(1, old_pcm[:3200], offset=0))
            self.assertTrue(native.entered[0].wait(TIMEOUT), "native preview never started")
            self.assertEqual(first.result()["sample_count"], 3200)
            queued = caller.submit(lambda: session.feed(1, old_pcm[3200:], offset=3200)).result()
            self.assertEqual(queued["sample_count"], 6400)
            self.assertEqual(len(native.calls), 1, "second preview must be pending behind the gate")

            reset_error = OSError("injected processor reset failure")
            feed_error = RuntimeError("injected processor feed failure")
            processor.reset_error = reset_error
            if action == "cancel":
                invalidate = lambda: session.cancel(1)
            elif action == "close":
                invalidate = session.close
            elif action == "replace":
                invalidate = lambda: session.start(2, context="replacement", language="de")
            else:
                processor.feed_errors.append(feed_error)
                invalidate = lambda: session.feed(
                    1, np.array([0.9], dtype=np.float32), offset=6400
                )

            active_at_return = []

            def invalidate_and_record_owner():
                try:
                    return invalidate()
                finally:
                    with native.lock:
                        active_at_return.append(native.active)

            invalidation = caller.submit(invalidate_and_record_owner)
            self.assertTrue(processor.reset_entered.wait(TIMEOUT),
                            "invalidation never entered processor cleanup")
            self.assertFalse(native.release[1].is_set())
            self.assertEqual(native.active, 1)
            # Cleanup has entered while native is blocked. A completed call here
            # would let the server observe an inactive stream with native still busy.
            returned_before_native_release = invalidation.done.wait(TIMEOUT)
            native.unblock()
            expected_errors = (reset_error, feed_error) if action == "feed_error" else (reset_error,)
            with self.assertRaises(tuple(type(error) for error in expected_errors)) as caught:
                invalidation.result()
            self.assertIn(caught.exception, expected_errors)
            self.assertEqual(active_at_return, [0],
                             "invalidation returned while native still owned the old session")
            self.assertFalse(returned_before_native_release,
                             "processor failure must not bypass the native drain")
            self.assertEqual(len(native.calls), 1, "obsolete pending preview must never execute")
            self.assertFalse(session.active)
            self.assertEqual(caller.submit(lambda: runtime.preview_text).result(), "")
            with self.assertRaises(RuntimeError):
                caller.submit(lambda: runtime.feed(np.ones(1, dtype=np.float32))).result()

            processor.reset_error = None
            with self.assertRaises(ValueError):
                caller.submit(lambda: session.start(2 if action == "replace" else 1)).result()
            started = caller.submit(
                lambda: session.start(3, context="fresh context", language="en")
            ).result()
            self.assertEqual((started["session_id"], started["sample_count"]), (3, 0))
            with self.assertRaises(ValueError):
                caller.submit(lambda: session.finish(1)).result()
            fresh_pcm = np.linspace(-0.8, -0.1, 13, dtype=np.float32)
            accepted = caller.submit(
                lambda: session.feed(3, fresh_pcm, offset=0, preview=False)
            ).result()
            self.assertEqual(accepted["sample_count"], 13)
            final = caller.submit(lambda: session.finish(3)).result()
            self.assertEqual(
                (final["session_id"], final["sample_count"], final["text"], final["language"], final["final"]),
                (3, 13, "fresh final", "en", True),
            )
            self.assertEqual(len(native.calls), 2)
            audio, options, thread_id = native.calls[-1]
            np.testing.assert_array_equal(
                audio, np.pad(fresh_pcm * np.float32(0.5), (0, 8000 - len(fresh_pcm)))
            )
            self.assertEqual(options, {"context": "fresh context", "language": "en"})
            self.assertEqual(thread_id, native.calls[0][2], "cleanup must preserve the resident worker")
            self.assertEqual(native.max_active, 1)
            self.assertFalse(session.active)
        finally:
            native.unblock()
            try:
                if runtime is not None:
                    caller.submit(runtime.close).result()
            finally:
                caller.close()


if __name__ == "__main__":
    unittest.main()
