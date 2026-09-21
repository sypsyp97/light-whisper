import os
import sys
import unittest

import numpy as np

sys.path.insert(0, os.path.dirname(__file__))

from r2t2_stream import R2T2StreamSession


class RecordingRuntime:
    """Deterministic native-runtime seam for controller behavior tests."""

    def __init__(self, feed_results=(), finish_result=("", None)):
        self.events = []
        self.start_calls = []
        self.feed_inputs = []
        self.feed_results = list(feed_results)
        self.finish_result = finish_result
        self.finish_calls = 0
        self.reset_calls = 0
        self.start_errors = []
        self.feed_errors = []
        self.finish_errors = []
        self.reset_errors = []

    @staticmethod
    def _raise_next(errors):
        if errors:
            error = errors.pop(0)
            if error is not None:
                raise error

    def start(self, context, language):
        self.events.append(("start", context, language))
        self.start_calls.append((context, language))
        self._raise_next(self.start_errors)

    def feed(self, pcm):
        copied = np.array(pcm, copy=True)
        self.events.append(("feed", copied))
        self.feed_inputs.append(copied)
        self._raise_next(self.feed_errors)
        if self.feed_results:
            result = self.feed_results.pop(0)
            if isinstance(result, BaseException):
                raise result
            return result
        return "", None

    def finish(self):
        self.events.append(("finish",))
        self.finish_calls += 1
        self._raise_next(self.finish_errors)
        return self.finish_result

    def reset(self):
        self.events.append(("reset",))
        self.reset_calls += 1
        self._raise_next(self.reset_errors)

    def close(self):
        raise AssertionError("the controller must not close the native runtime")


class R2T2StreamSessionTests(unittest.TestCase):
    def assert_response(self, response, *, session_id, text, language, sample_count, final):
        self.assertEqual(
            response,
            {
                "success": True,
                "session_id": session_id,
                "text": text,
                "tentative_text": "",
                "language": language,
                "sample_count": sample_count,
                "final": final,
            },
        )

    def test_start_forwards_context_and_language_and_returns_active_response(self):
        runtime = RecordingRuntime()
        session = R2T2StreamSession(runtime)

        response = session.start(7, context="hotword: 你好\t", language="zh")

        self.assert_response(
            response,
            session_id=7,
            text="",
            language="zh",
            sample_count=0,
            final=False,
        )
        self.assertEqual(runtime.start_calls, [("hotword: 你好\t", "zh")])
        self.assertEqual(runtime.events, [("start", "hotword: 你好\t", "zh")])

    def test_start_replaces_active_stream_and_stale_calls_cannot_touch_current(self):
        runtime = RecordingRuntime(feed_results=[("旧", "zh"), ("新", "zh")])
        session = R2T2StreamSession(runtime)

        session.start(7)
        session.feed(7, np.array([0.1, 0.2], dtype=np.float32), offset=0)
        response = session.start(8)

        self.assert_response(
            response,
            session_id=8,
            text="",
            language=None,
            sample_count=0,
            final=False,
        )
        self.assertEqual(runtime.reset_calls, 1)

        with self.assertRaises(ValueError):
            session.start(8)
        with self.assertRaises(ValueError):
            session.start(7)
        with self.assertRaises(ValueError):
            session.feed(7, np.array([0.3], dtype=np.float32), offset=0)
        with self.assertRaises(ValueError):
            session.finish(7)
        with self.assertRaises(ValueError):
            session.cancel(7)

        response = session.feed(8, np.array([0.4], dtype=np.float32), offset=0)
        self.assert_response(
            response,
            session_id=8,
            text="新",
            language="zh",
            sample_count=1,
            final=False,
        )
        self.assertEqual(runtime.reset_calls, 1)
        self.assertEqual(len(runtime.feed_inputs), 2)
        self.assertEqual(
            [event[0] for event in runtime.events],
            ["start", "feed", "reset", "start", "feed"],
        )

    def test_session_ids_validate_and_failed_start_consumes_id(self):
        runtime = RecordingRuntime()
        session = R2T2StreamSession(runtime)

        for invalid_id in (0, -1, True, 1.5, "1", None):
            with self.assertRaises(ValueError):
                session.start(invalid_id)
        self.assertEqual(runtime.start_calls, [])

        runtime.start_errors.append(RuntimeError("native start failed"))
        with self.assertRaisesRegex(RuntimeError, "native start failed"):
            session.start(1)
        self.assertEqual(runtime.reset_calls, 1)

        with self.assertRaises(ValueError):
            session.start(1)
        response = session.start(2)
        self.assert_response(
            response,
            session_id=2,
            text="",
            language=None,
            sample_count=0,
            final=False,
        )
        self.assertEqual(runtime.start_calls, [("", None), ("", None)])

    def test_feed_rejects_invalid_audio_and_offsets_before_native_call(self):
        runtime = RecordingRuntime()
        session = R2T2StreamSession(runtime)
        session.start(3)

        invalid_audio = (
            np.array([], dtype=np.float32),
            np.array([[0.1]], dtype=np.float32),
            np.array([0.1], dtype=np.float64),
            np.array([np.nan], dtype=np.float32),
            np.array([np.inf], dtype=np.float32),
            [0.1],
            None,
            0.1,
        )
        for audio in invalid_audio:
            with self.assertRaises(ValueError):
                session.feed(3, audio, offset=0)

        valid_audio = np.array([0.1], dtype=np.float32)
        for invalid_offset in (-1, True, 0.5, 1):
            with self.assertRaises(ValueError):
                session.feed(3, valid_audio, offset=invalid_offset)

        self.assertEqual(runtime.feed_inputs, [])
        self.assertEqual(runtime.reset_calls, 0)
        response = session.feed(3, np.array([0.1, 0.2], dtype=np.float32), offset=0)
        self.assert_response(
            response,
            session_id=3,
            text="",
            language=None,
            sample_count=2,
            final=False,
        )
        self.assertEqual(len(runtime.feed_inputs), 1)

        for invalid_offset in (0, 1, 3):
            with self.assertRaises(ValueError):
                session.feed(
                    3,
                    np.array([0.3], dtype=np.float32),
                    offset=invalid_offset,
                )
        self.assertEqual(len(runtime.feed_inputs), 1)

        response = session.feed(3, np.array([0.3], dtype=np.float32), offset=2)
        self.assert_response(
            response,
            session_id=3,
            text="",
            language=None,
            sample_count=3,
            final=False,
        )
        self.assertEqual(len(runtime.feed_inputs), 2)

    def test_feed_preserves_float32_audio_unicode_text_and_known_language(self):
        runtime = RecordingRuntime(
            feed_results=[
                ("你🚀", "zh"),
                ("你🚀好", "de"),
                ("你🚀好!", ""),
            ]
        )
        session = R2T2StreamSession(runtime)
        session.start(4, language="en")

        first_audio = np.array([0.1, 0.2], dtype=np.float32)
        first_response = session.feed(4, first_audio, offset=0)
        second_response = session.feed(
            4, np.array([0.3], dtype=np.float32), offset=2
        )
        third_response = session.feed(
            4, np.array([0.4, 0.5], dtype=np.float32), offset=3
        )

        self.assert_response(
            first_response,
            session_id=4,
            text="你🚀",
            language="zh",
            sample_count=2,
            final=False,
        )
        self.assert_response(
            second_response,
            session_id=4,
            text="你🚀好",
            language="de",
            sample_count=3,
            final=False,
        )
        self.assert_response(
            third_response,
            session_id=4,
            text="你🚀好!",
            language="de",
            sample_count=5,
            final=False,
        )
        np.testing.assert_array_equal(
            runtime.feed_inputs[0], np.array([0.1, 0.2], dtype=np.float32)
        )
        self.assertEqual(runtime.feed_inputs[0].dtype, np.dtype(np.float32))

    def test_feed_text_regression_resets_and_invalidates_without_retry(self):
        for regressed_text in ("ab", "xbc"):
            with self.subTest(regressed_text=regressed_text):
                runtime = RecordingRuntime(
                    feed_results=[("abc", "en"), (regressed_text, "en")]
                )
                session = R2T2StreamSession(runtime)
                session.start(5)
                session.feed(5, np.array([0.1], dtype=np.float32), offset=0)

                with self.assertRaises(RuntimeError):
                    session.feed(5, np.array([0.2], dtype=np.float32), offset=1)

                self.assertEqual(len(runtime.feed_inputs), 2)
                self.assertEqual(runtime.reset_calls, 1)
                with self.assertRaises(ValueError):
                    session.feed(5, np.array([0.3], dtype=np.float32), offset=0)
                with self.assertRaises(ValueError):
                    session.finish(5)
                with self.assertRaises(ValueError):
                    session.cancel(5)
                self.assertEqual(runtime.reset_calls, 1)

    def test_finish_text_regression_resets_and_invalidates_session(self):
        for regressed_text in ("ab", "xbc"):
            with self.subTest(regressed_text=regressed_text):
                runtime = RecordingRuntime(
                    feed_results=[("abc", "en")],
                    finish_result=(regressed_text, "en"),
                )
                session = R2T2StreamSession(runtime)
                session.start(6)
                session.feed(6, np.array([0.1], dtype=np.float32), offset=0)

                with self.assertRaises(RuntimeError):
                    session.finish(6)

                self.assertEqual(runtime.finish_calls, 1)
                self.assertEqual(runtime.reset_calls, 1)
                with self.assertRaises(ValueError):
                    session.feed(6, np.array([0.2], dtype=np.float32), offset=1)
                with self.assertRaises(ValueError):
                    session.finish(6)
                with self.assertRaises(ValueError):
                    session.cancel(6)
                self.assertEqual(runtime.finish_calls, 1)
                self.assertEqual(runtime.reset_calls, 1)

    def test_finish_flushes_once_and_returns_complete_unicode_text(self):
        runtime = RecordingRuntime(
            feed_results=[("你好", "zh")],
            finish_result=("你好世界", "ja"),
        )
        session = R2T2StreamSession(runtime)
        session.start(6)
        session.feed(6, np.array([0.1, 0.2, 0.3], dtype=np.float32), offset=0)

        response = session.finish(6)

        self.assert_response(
            response,
            session_id=6,
            text="你好世界",
            language="ja",
            sample_count=3,
            final=True,
        )
        self.assertEqual(runtime.finish_calls, 1)
        with self.assertRaises(ValueError):
            session.finish(6)
        with self.assertRaises(ValueError):
            session.feed(6, np.array([0.4], dtype=np.float32), offset=3)
        with self.assertRaises(ValueError):
            session.cancel(6)
        self.assertEqual(runtime.finish_calls, 1)

    def test_empty_finish_avoids_native_finish_and_resets_stream(self):
        runtime = RecordingRuntime()
        runtime.finish_errors.append(AssertionError("finish must not be called"))
        session = R2T2StreamSession(runtime)
        session.start(7)

        response = session.finish(7)

        self.assert_response(
            response,
            session_id=7,
            text="",
            language=None,
            sample_count=0,
            final=True,
        )
        self.assertEqual(runtime.finish_calls, 0)
        self.assertEqual(runtime.reset_calls, 1)
        with self.assertRaises(ValueError):
            session.cancel(7)

    def test_cancel_returns_empty_final_and_resets_once(self):
        runtime = RecordingRuntime(feed_results=[("spoken", "de")])
        session = R2T2StreamSession(runtime)
        session.start(8, language="en")
        session.feed(8, np.array([0.1, 0.2, 0.3, 0.4], dtype=np.float32), offset=0)

        response = session.cancel(8)

        self.assert_response(
            response,
            session_id=8,
            text="",
            language="de",
            sample_count=4,
            final=True,
        )
        self.assertEqual(runtime.reset_calls, 1)
        with self.assertRaises(ValueError):
            session.cancel(8)
        self.assertEqual(runtime.reset_calls, 1)

    def test_close_is_idempotent_and_does_not_close_native_runtime(self):
        runtime = RecordingRuntime()
        session = R2T2StreamSession(runtime)

        self.assertIsNone(session.close())
        self.assertEqual(runtime.reset_calls, 0)
        session.start(9)
        session.feed(9, np.array([0.1, 0.2], dtype=np.float32), offset=0)
        self.assertIsNone(session.close())
        self.assertIsNone(session.close())

        self.assertEqual(runtime.reset_calls, 1)
        with self.assertRaises(ValueError):
            session.feed(9, np.array([0.3], dtype=np.float32), offset=2)
        response = session.start(10)
        self.assert_response(
            response,
            session_id=10,
            text="",
            language=None,
            sample_count=0,
            final=False,
        )

    def test_feed_error_preserves_original_error_when_reset_also_fails(self):
        runtime = RecordingRuntime()
        runtime.feed_errors.append(RuntimeError("feed failed"))
        runtime.reset_errors.append(OSError("reset failed"))
        session = R2T2StreamSession(runtime)
        session.start(11)

        with self.assertRaisesRegex(RuntimeError, "feed failed"):
            session.feed(11, np.array([0.1], dtype=np.float32), offset=0)

        self.assertEqual(len(runtime.feed_inputs), 1)
        self.assertEqual(runtime.reset_calls, 1)
        with self.assertRaises(ValueError):
            session.finish(11)

    def test_finish_error_preserves_original_error_and_invalidates_session(self):
        runtime = RecordingRuntime(feed_results=[("text", "en")])
        runtime.finish_errors.append(RuntimeError("finish failed"))
        runtime.reset_errors.append(OSError("reset failed"))
        session = R2T2StreamSession(runtime)
        session.start(12)
        session.feed(12, np.array([0.1], dtype=np.float32), offset=0)

        with self.assertRaisesRegex(RuntimeError, "finish failed"):
            session.finish(12)

        self.assertEqual(runtime.finish_calls, 1)
        self.assertEqual(runtime.reset_calls, 1)
        with self.assertRaises(ValueError):
            session.cancel(12)

    def test_start_error_preserves_original_error_and_consumes_session_id(self):
        runtime = RecordingRuntime()
        runtime.reset_errors.extend([None, OSError("reset failed")])
        session = R2T2StreamSession(runtime)
        session.start(13)
        session.feed(13, np.array([0.1], dtype=np.float32), offset=0)
        runtime.start_errors.append(RuntimeError("start failed"))

        with self.assertRaisesRegex(RuntimeError, "start failed"):
            session.start(14)

        with self.assertRaises(ValueError):
            session.feed(13, np.array([0.2], dtype=np.float32), offset=1)
        with self.assertRaises(ValueError):
            session.finish(13)
        with self.assertRaises(ValueError):
            session.cancel(13)
        with self.assertRaises(ValueError):
            session.start(14)
        response = session.start(15)
        self.assert_response(
            response,
            session_id=15,
            text="",
            language=None,
            sample_count=0,
            final=False,
        )

    def test_finish_retains_latest_detected_language_when_native_language_is_empty(self):
        for final_language in (None, ""):
            with self.subTest(final_language=final_language):
                runtime = RecordingRuntime(
                    feed_results=[("Hallo", "de")],
                    finish_result=("Hallo!", final_language),
                )
                session = R2T2StreamSession(runtime)
                session.start(1, language="en")
                session.feed(1, np.array([0.1], dtype=np.float32), offset=0)
                self.assert_response(
                    session.finish(1), session_id=1, text="Hallo!",
                    language="de", sample_count=1, final=True,
                )

    def test_reset_error_on_cancel_invalidates_and_propagates(self):
        runtime = RecordingRuntime()
        runtime.reset_errors.append(RuntimeError("cancel reset failed"))
        session = R2T2StreamSession(runtime)
        session.start(15)

        with self.assertRaisesRegex(RuntimeError, "cancel reset failed"):
            session.cancel(15)

        self.assertEqual(runtime.reset_calls, 1)
        with self.assertRaises(ValueError):
            session.feed(15, np.array([0.1], dtype=np.float32), offset=0)

    def test_invalid_active_session_ids_cannot_address_feed_finish_or_cancel(self):
        runtime = RecordingRuntime()
        session = R2T2StreamSession(runtime)
        session.start(1)

        for invalid_id in (True, 1.0, "1"):
            with self.subTest(invalid_id=invalid_id):
                with self.assertRaises(ValueError):
                    session.feed(
                        invalid_id,
                        np.array([0.1], dtype=np.float32),
                        offset=0,
                    )
                with self.assertRaises(ValueError):
                    session.finish(invalid_id)
                with self.assertRaises(ValueError):
                    session.cancel(invalid_id)

        self.assertEqual(runtime.feed_inputs, [])
        self.assertEqual(runtime.finish_calls, 0)
        self.assertEqual(runtime.reset_calls, 0)
        response = session.feed(1, np.array([0.1], dtype=np.float32), offset=0)
        self.assert_response(
            response,
            session_id=1,
            text="",
            language=None,
            sample_count=1,
            final=False,
        )


if __name__ == "__main__":
    unittest.main()
