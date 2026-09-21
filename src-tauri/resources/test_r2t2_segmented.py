import os
import sys
import unittest

import numpy as np

sys.path.insert(0, os.path.dirname(__file__))

from r2t2_segmented import SegmentedR2T2


class RecordingNative:
    def __init__(self, feed_results=(), finish_results=(), feed_errors=()):
        self.events = []
        self.start_calls = []
        self.feed_inputs = []
        self.finish_calls = 0
        self.reset_calls = 0
        self.feed_results = list(feed_results)
        self.finish_results = list(finish_results)
        self.feed_errors = list(feed_errors)

    def start(self, context="", language=None):
        self.events.append(("start", context, language))
        self.start_calls.append((context, language))

    def feed(self, pcm):
        copied = np.array(pcm, copy=True)
        self.events.append(("feed", copied))
        self.feed_inputs.append(copied)
        if self.feed_errors:
            error = self.feed_errors.pop(0)
            raise error
        return self.feed_results.pop(0)

    def finish(self):
        self.events.append(("finish",))
        self.finish_calls += 1
        return self.finish_results.pop(0)

    def reset(self):
        self.events.append(("reset",))
        self.reset_calls += 1


class MarkerVad:
    """Return one speech region for positive marker samples, otherwise silence."""

    def __init__(self):
        self.inputs = []

    def speech_timestamps(self, pcm):
        observed = np.array(pcm, copy=True)
        self.inputs.append(observed)
        speech = np.flatnonzero(observed > 0.5)
        if speech.size == 0:
            return []
        return [{"start": int(speech[0]), "end": int(speech[-1]) + 1}]


class SegmentedR2T2Tests(unittest.TestCase):
    def test_vad_runs_only_when_it_can_start_or_end_a_segment(self):
        native = RecordingNative(
            feed_results=[("speech", "en")] * 4,
            finish_results=[("speech", "en")],
        )
        vad = MarkerVad()
        session = self._session(native, vad, min_segment_samples=16, pause_samples=4)
        session.start()
        session.feed(self._audio([1, 1, 1, 1]))
        session.feed(self._audio([0] * 8))
        self.assertEqual(len(vad.inputs), 1)
        self.assertEqual(native.finish_calls, 0)
        session.feed(self._audio([0] * 4))
        self.assertEqual(len(vad.inputs), 2)
        self.assert_audio(vad.inputs[-1], [0] * 6)
        self.assertEqual(native.finish_calls, 1)
        np.testing.assert_array_equal(
            np.concatenate(native.feed_inputs), self._audio([1] * 4 + [0] * 12)
        )
        self.assertEqual(session.finish(), ("speech", "en"))

    @staticmethod
    def _session(native, vad, **overrides):
        options = {
            "chunk_samples": 4,
            "window_samples": 6,
            "min_segment_samples": 100,
            "max_segment_samples": 100,
            "pause_samples": 100,
        }
        options.update(overrides)
        return SegmentedR2T2(native, vad, **options)

    @staticmethod
    def _audio(values):
        return np.asarray(values, dtype=np.float32)

    def assert_audio(self, observed, expected):
        np.testing.assert_array_equal(observed, self._audio(expected))
        self.assertEqual(observed.dtype, np.dtype(np.float32))

    def test_preroll_and_incomplete_tail_are_forwarded_once_without_padding(self):
        native = RecordingNative(
            feed_results=[("stable", "en"), ("stable tail", None)],
            finish_results=[("stable tail", None)],
        )
        vad = MarkerVad()
        session = self._session(native, vad)
        session.start(context="ctx", language="hint")

        session.feed(self._audio([0, 0, 0, 0]))
        current_text, current_language = session.feed(
            self._audio([0, 0, 1, 1, 1, 1])
        )
        self.assertEqual((current_text, current_language), ("stable", "en"))
        final_text, final_language = session.finish()

        self.assertEqual(native.start_calls, [("ctx", "hint")])
        self.assertEqual(
            [event[0] for event in native.events if event[0] != "reset"],
            ["start", "feed", "feed", "finish"],
        )
        self.assertEqual(len(native.feed_inputs), 2)
        self.assert_audio(native.feed_inputs[0], [0, 0, 0, 0, 1, 1])
        self.assert_audio(native.feed_inputs[1], [1, 1])
        self.assertEqual(native.finish_calls, 1)
        self.assertEqual((final_text, final_language), ("stable tail", "en"))
        self.assertLessEqual(max(len(observed) for observed in vad.inputs), 6)

    def test_no_speech_suppresses_native_finish(self):
        native = RecordingNative(
            feed_results=[],
            finish_results=[],
        )
        vad = MarkerVad()
        session = self._session(native, vad, window_samples=4, min_segment_samples=4)
        session.start(context="old", language="en")

        self.assertEqual(session.feed(self._audio([0, 0, 0, 0]))[0], "")
        self.assertEqual(session.finish()[0], "")
        self.assertEqual(native.start_calls, [])
        self.assertEqual(native.feed_inputs, [])
        self.assertEqual(native.finish_calls, 0)

    def test_reset_and_new_start_clear_old_text_and_context(self):
        native = RecordingNative(
            feed_results=[("old", "en"), ("new", "fr")],
            finish_results=[("new", "fr")],
        )
        vad = MarkerVad()
        session = self._session(native, vad, window_samples=4)
        session.reset()
        session.reset()
        self.assertEqual(native.start_calls, [])
        self.assertGreaterEqual(native.reset_calls, 2)
        with self.assertRaises(ValueError):
            session.feed(self._audio([1, 1, 1, 1]))
        with self.assertRaises(ValueError):
            session.finish()
        session.start(context="old context", language="en")
        session.feed(self._audio([1, 1, 1, 1]))
        resets_before_replacement = native.reset_calls
        session.start(context="new context", language="de")
        self.assertGreater(native.reset_calls, resets_before_replacement)
        session.feed(self._audio([1, 1, 1, 1]))
        final_text, final_language = session.finish()

        self.assertEqual(
            native.start_calls,
            [("old context", "en"), ("new context", "de")],
        )
        self.assertEqual((final_text, final_language), ("new", "fr"))
        self.assertEqual(native.finish_calls, 1)
        with self.assertRaises(ValueError):
            session.feed(self._audio([1, 1, 1, 1]))
        with self.assertRaises(ValueError):
            session.finish()

    def test_pause_splits_segments_preserves_context_and_joins_without_rewriting(self):
        native = RecordingNative(
            feed_results=[
                ("alpha", "en"),
                ("alpha", None),
                ("beta!", "fr"),
                ("beta!", None),
                ("(tail", None),
            ],
            finish_results=[("alpha", None), ("beta!", None), ("(tail", "")],
        )
        vad = MarkerVad()
        session = self._session(
            native,
            vad,
            window_samples=4,
            min_segment_samples=4,
            pause_samples=4,
        )
        session.start(context="same context", language="hint")
        speech = self._audio([1, 1, 1, 1])
        silence = self._audio([0, 0, 0, 0])

        short_native = RecordingNative(
            feed_results=[("short", "en"), ("short", "en")],
            finish_results=[],
        )
        short_session = self._session(
            short_native,
            MarkerVad(),
            window_samples=4,
            min_segment_samples=12,
            pause_samples=4,
        )
        short_session.start()
        short_session.feed(speech)
        short_session.feed(silence)
        self.assertEqual(short_native.finish_calls, 0)

        session.feed(speech)
        boundary_text, boundary_language = session.feed(silence)
        self.assertEqual((boundary_text, boundary_language), ("alpha", "en"))
        session.feed(speech)
        session.feed(silence)
        session.feed(speech)
        final_text, final_language = session.finish()

        self.assertEqual(native.start_calls, [("same context", "hint")] * 3)
        self.assertEqual(native.finish_calls, 3)
        self.assertEqual(len(native.feed_inputs), 5)
        self.assert_audio(native.feed_inputs[0], [1, 1, 1, 1])
        self.assert_audio(native.feed_inputs[1], [0, 0, 0, 0])
        self.assert_audio(native.feed_inputs[2], [1, 1, 1, 1])
        self.assert_audio(native.feed_inputs[3], [0, 0, 0, 0])
        self.assert_audio(native.feed_inputs[4], [1, 1, 1, 1])
        self.assertEqual((final_text, final_language), ("alpha beta!(tail", "fr"))

    def test_max_segment_limit_splits_after_a_block_and_keeps_native_inputs_bounded(self):
        native = RecordingNative(
            feed_results=[("one", "en"), ("one", "en"), ("two", "en")],
            finish_results=[("one", "en"), ("two", "en")],
        )
        vad = MarkerVad()
        session = self._session(
            native,
            vad,
            window_samples=4,
            max_segment_samples=8,
            pause_samples=100,
        )
        session.start(context="ctx", language="en")
        speech = self._audio([1, 1, 1, 1])

        session.feed(speech)
        boundary_text, boundary_language = session.feed(speech)
        self.assertEqual((boundary_text, boundary_language), ("one", "en"))
        session.feed(speech)
        final_text, final_language = session.finish()

        self.assertEqual(len(native.start_calls), 2)
        self.assertEqual(native.finish_calls, 2)
        self.assertEqual(len(native.feed_inputs), 3)
        for observed in native.feed_inputs:
            self.assert_audio(observed, [1, 1, 1, 1])
        self.assertEqual((final_text, final_language), ("one two", "en"))

    def test_finish_prefix_violation_resets_and_raises_without_publishing_rewrite(self):
        native = RecordingNative(
            feed_results=[("abc", "en")],
            finish_results=[("xbc", "en")],
        )
        vad = MarkerVad()
        session = self._session(native, vad)
        session.start()
        session.feed(self._audio([1, 1, 1, 1]))
        resets_before = native.reset_calls

        with self.assertRaises(RuntimeError):
            session.finish()

        self.assertEqual(native.finish_calls, 1)
        self.assertGreater(native.reset_calls, resets_before)
        self.assertEqual(len(native.feed_inputs), 1)
        with self.assertRaises(ValueError):
            session.feed(self._audio([1, 1, 1, 1]))
        with self.assertRaises(ValueError):
            session.finish()

    def test_native_feed_error_propagates_without_retry(self):
        native = RecordingNative(feed_errors=[RuntimeError("native feed failed")])
        vad = MarkerVad()
        session = self._session(native, vad)
        session.start()

        with self.assertRaisesRegex(RuntimeError, "native feed failed"):
            session.feed(self._audio([1, 1, 1, 1]))

        self.assertEqual(len(native.feed_inputs), 1)
        self.assertEqual(native.finish_calls, 0)


if __name__ == "__main__":
    unittest.main()
