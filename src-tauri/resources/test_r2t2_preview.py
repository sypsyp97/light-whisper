import os
import sys
import unittest

import numpy as np

sys.path.insert(0, os.path.dirname(__file__))

from r2t2_segmented import SegmentedR2T2
from r2t2_stream import R2T2StreamSession


class PreviewRuntime:
    """Deterministic runtime seam with a full transcript preview."""

    def __init__(self, feed_results=(), finish_result=("", None)):
        self.feed_results = list(feed_results)
        self.finish_result = finish_result
        self.preview_text = ""
        self.start_calls = []
        self.feed_inputs = []
        self.finish_calls = 0
        self.reset_calls = 0

    def start(self, context="", language=None):
        self.start_calls.append((context, language))
        self.preview_text = ""

    def feed(self, pcm):
        self.feed_inputs.append(np.array(pcm, copy=True))
        committed, language, preview = self.feed_results.pop(0)
        self.preview_text = preview
        return committed, language

    def finish(self):
        self.finish_calls += 1
        committed, language = self.finish_result
        self.preview_text = committed
        return committed, language

    def reset(self):
        self.reset_calls += 1
        self.preview_text = ""


class LegacyRuntime:
    """Legacy runtime seam intentionally has no preview_text attribute."""

    def __init__(self, result=("", None)):
        self.result = result
        self.reset_calls = 0

    def start(self, context="", language=None):
        pass

    def feed(self, pcm):
        return self.result

    def finish(self):
        return self.result

    def reset(self):
        self.reset_calls += 1


class PreviewNative:
    """Native seam for segmented propagation and preview revisions."""

    def __init__(self, feed_results=(), finish_results=()):
        self.feed_results = list(feed_results)
        self.finish_results = list(finish_results)
        self.preview_text = ""
        self.reset_calls = 0

    def start(self, context="", language=None):
        self.preview_text = ""

    def feed(self, pcm):
        committed, language, preview = self.feed_results.pop(0)
        self.preview_text = preview
        return committed, language

    def finish(self):
        committed, language, preview = self.finish_results.pop(0)
        self.preview_text = preview
        return committed, language

    def reset(self):
        self.reset_calls += 1
        self.preview_text = ""


class LegacyNative:
    """Legacy segmented native seam without preview_text."""

    def __init__(self, result=("legacy", "en")):
        self.result = result

    def start(self, context="", language=None):
        pass

    def feed(self, pcm):
        return self.result

    def finish(self):
        return self.result

    def reset(self):
        pass


class SpeechMarkerVad:
    @staticmethod
    def speech_timestamps(pcm):
        if np.any(np.asarray(pcm) > 0.5):
            return [{"start": 0, "end": len(pcm)}]
        return []


class R2T2PreviewTests(unittest.TestCase):
    @staticmethod
    def _pcm(values=(0.1,)):
        return np.asarray(values, dtype=np.float32)

    @staticmethod
    def _segmented(native):
        return SegmentedR2T2(
            native,
            SpeechMarkerVad(),
            chunk_samples=4,
            window_samples=4,
            min_segment_samples=4,
            max_segment_samples=100,
            pause_samples=4,
        )

    def assert_response(
        self,
        response,
        *,
        session_id,
        text,
        tentative_text,
        language,
        sample_count,
        final,
    ):
        self.assertEqual(
            response,
            {
                "success": True,
                "session_id": session_id,
                "text": text,
                "tentative_text": tentative_text,
                "language": language,
                "sample_count": sample_count,
                "final": final,
            },
        )

    def test_session_reports_revisable_unicode_preview_suffix(self):
        runtime = PreviewRuntime(
            feed_results=[
                ("你好", "zh", "你好世界"),
                ("你好", None, "你好世代"),
                ("你好", None, "你好世"),
                ("你好", None, "你好"),
            ]
        )
        session = R2T2StreamSession(runtime)

        self.assert_response(
            session.start(1, language="zh"),
            session_id=1,
            text="",
            tentative_text="",
            language="zh",
            sample_count=0,
            final=False,
        )

        responses = [
            session.feed(1, self._pcm(), offset=0),
            session.feed(1, self._pcm(), offset=1),
            session.feed(1, self._pcm(), offset=2),
            session.feed(1, self._pcm(), offset=3),
        ]
        self.assertEqual(len(responses), 4)
        for sample_count, (response, tentative_text) in enumerate(
            zip(responses, ("世界", "世代", "世", "")), start=1
        ):
            self.assert_response(
                response,
                session_id=1,
                text="你好",
                tentative_text=tentative_text,
                language="zh",
                sample_count=sample_count,
                final=False,
            )

    def test_legacy_runtime_without_preview_reports_committed_only(self):
        session = R2T2StreamSession(LegacyRuntime(("legacy", "en")))
        session.start(2)

        self.assert_response(
            session.feed(2, self._pcm(), offset=0),
            session_id=2,
            text="legacy",
            tentative_text="",
            language="en",
            sample_count=1,
            final=False,
        )

    def test_finish_and_cancel_responses_always_clear_tentative_text(self):
        finishing = PreviewRuntime(
            feed_results=[("committed", "en", "committed draft")],
            finish_result=("committed final", "en"),
        )
        session = R2T2StreamSession(finishing)
        session.start(3)
        self.assert_response(
            session.feed(3, self._pcm(), offset=0),
            session_id=3,
            text="committed",
            tentative_text=" draft",
            language="en",
            sample_count=1,
            final=False,
        )

        self.assert_response(
            session.finish(3),
            session_id=3,
            text="committed final",
            tentative_text="",
            language="en",
            sample_count=1,
            final=True,
        )

        cancelling = PreviewRuntime(
            feed_results=[("committed", "en", "committed draft")]
        )
        session = R2T2StreamSession(cancelling)
        session.start(4)
        session.feed(4, self._pcm(), offset=0)

        self.assert_response(
            session.cancel(4),
            session_id=4,
            text="",
            tentative_text="",
            language="en",
            sample_count=1,
            final=True,
        )

    def test_conflicting_preview_resets_and_invalidates_session(self):
        runtime = PreviewRuntime(
            feed_results=[
                ("hello", "en", "hello draft"),
                ("hello world", "en", "hello stale"),
            ]
        )
        session = R2T2StreamSession(runtime)
        session.start(5)
        session.feed(5, self._pcm(), offset=0)

        with self.assertRaises(RuntimeError):
            session.feed(5, self._pcm(), offset=1)

        self.assertEqual(runtime.reset_calls, 1)
        with self.assertRaises(ValueError):
            session.feed(5, self._pcm(), offset=0)

    def test_segmented_preview_tracks_native_revisions_and_keeps_join_spacing(self):
        native = PreviewNative(
            feed_results=[
                ("hello", "en", "hello draft"),
                ("hello", None, "hello"),
                ("world", "en", "world 世界"),
                ("world", None, "world 世界!"),
            ],
            finish_results=[("hello", None, "hello")],
        )
        session = self._segmented(native)
        session.start()

        speech = self._pcm((1.0, 1.0, 1.0, 1.0))
        silence = self._pcm((0.0, 0.0, 0.0, 0.0))

        self.assertEqual(session.feed(speech), ("hello", "en"))
        self.assertEqual(getattr(session, "preview_text", None), "hello draft")
        self.assertEqual(session.feed(silence), ("hello", "en"))
        self.assertEqual(getattr(session, "preview_text", None), "hello")

        self.assertEqual(session.feed(speech), ("hello world", "en"))
        self.assertEqual(getattr(session, "preview_text", None), "hello world 世界")
        self.assertEqual(session.feed(speech), ("hello world", "en"))
        self.assertEqual(
            getattr(session, "preview_text", None), "hello world 世界!"
        )

    def test_segmented_legacy_native_preview_defaults_to_committed(self):
        session = self._segmented(LegacyNative())
        session.start()

        self.assertEqual(session.feed(self._pcm((1.0, 1.0, 1.0, 1.0))), ("legacy", "en"))
        self.assertEqual(getattr(session, "preview_text", None), "legacy")


if __name__ == "__main__":
    unittest.main()
