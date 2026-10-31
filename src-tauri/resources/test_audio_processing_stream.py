"""Processor integration through public streaming APIs; no models or devices."""

import os
import sys
import unittest

import numpy as np

sys.path.insert(0, os.path.dirname(__file__))

from qwen3_native import QwenPreviewRuntime
from r2t2_stream import R2T2StreamSession
from test_qwen_preview_final_reuse import RecordingTranscribe
from test_r2t2_stream import RecordingRuntime


class BufferedProcessor:
    """Emit complete blocks at half amplitude; flush the delayed final block."""

    def __init__(self, block=4):
        self.block = block
        self.pending = np.empty(0, dtype=np.float32)
        self.feed_inputs = []
        self.reset_calls = 0
        self.flush_calls = 0
        self.feed_errors = []
        self.flush_errors = []

    def reset(self):
        self.reset_calls += 1
        self.pending = np.empty(0, dtype=np.float32)

    def feed(self, pcm):
        self.feed_inputs.append(pcm.copy())
        self.pending = np.concatenate((self.pending, pcm))
        if self.feed_errors:
            raise self.feed_errors.pop(0)
        emitted = len(self.pending) // self.block * self.block
        output = self.pending[:emitted].copy() * np.float32(0.5)
        self.pending = self.pending[emitted:].copy()
        return output

    def flush(self):
        self.flush_calls += 1
        if self.flush_errors:
            raise self.flush_errors.pop(0)
        output = self.pending.copy() * np.float32(0.5)
        self.pending = np.empty(0, dtype=np.float32)
        return output


def pcm(*values):
    return np.asarray(values, dtype=np.float32)


class AudioProcessingStreamTests(unittest.TestCase):
    def test_delayed_output_preserves_raw_counts_and_flushes_before_native_finish(self):
        processor = BufferedProcessor()
        runtime = RecordingRuntime(feed_results=[("你", "zh"), ("你好", None)],
                                   finish_result=("你好!", None))
        session = R2T2StreamSession(runtime, audio_processor=processor)
        session.start(7, context="术语", language="en")

        first = session.feed(7, pcm(0.1, 0.2, 0.3), offset=0)
        self.assertEqual((first["session_id"], first["sample_count"], first["text"]), (7, 3, ""))
        self.assertEqual(runtime.feed_inputs, [], "buffered empty output must not reach native feed")
        second = session.feed(7, pcm(0.4, 0.5, 0.6), offset=3)
        self.assertEqual((second["sample_count"], second["text"], second["language"]), (6, "你", "zh"))
        np.testing.assert_array_equal(runtime.feed_inputs[0], pcm(0.1, 0.2, 0.3, 0.4) * np.float32(0.5))

        final = session.finish(7)
        self.assertEqual((final["session_id"], final["sample_count"], final["text"], final["final"]),
                         (7, 6, "你好!", True))
        self.assertEqual(final["language"], "zh")
        self.assertEqual(processor.flush_calls, 1)
        self.assertEqual([event[0] for event in runtime.events], ["start", "feed", "feed", "finish"])
        np.testing.assert_array_equal(runtime.feed_inputs[1], pcm(0.5, 0.6) * np.float32(0.5))
        self.assertFalse(session.active)
        with self.assertRaises(ValueError):
            session.finish(7)
        self.assertEqual(processor.flush_calls, 1)

    def test_input_validation_precedes_processor_and_preserves_raw_offset(self):
        processor, runtime = BufferedProcessor(), RecordingRuntime()
        session = R2T2StreamSession(runtime, audio_processor=processor)
        session.start(1)
        session.feed(1, pcm(0.1, 0.2, 0.3), offset=0)
        reset_calls = processor.reset_calls
        for offset in (-1, True, 0, 2, 4):
            with self.subTest(offset=offset), self.assertRaises(ValueError):
                session.feed(1, pcm(0.4), offset=offset)
        for audio in (pcm(), pcm(float("nan")), np.ones((1, 1), dtype=np.float32),
                      np.ones(1, dtype=np.float64)):
            with self.subTest(audio=audio), self.assertRaises(ValueError):
                session.feed(1, audio, offset=3)
        for arguments in ({"session_id": 2}, {"preview": 1}, {"preview": False}):
            with self.subTest(arguments=arguments), self.assertRaises(ValueError):
                session.feed(pcm=pcm(0.4), **{"session_id": 1, "offset": 3, **arguments})
        self.assertEqual(len(processor.feed_inputs), 1)
        self.assertEqual(processor.reset_calls, reset_calls)
        self.assertEqual(runtime.feed_inputs, [])
        self.assertTrue(session.active)
        accepted = session.feed(1, pcm(0.4), offset=3)
        self.assertEqual(accepted["sample_count"], 4)
        np.testing.assert_array_equal(runtime.feed_inputs[0], pcm(0.1, 0.2, 0.3, 0.4) * np.float32(0.5))

    def test_start_cancel_replacement_and_close_discard_buffered_audio(self):
        for action in ("cancel", "replace", "close"):
            with self.subTest(action=action):
                processor, runtime = BufferedProcessor(), RecordingRuntime()
                processor.pending = pcm(99)  # A reused processor must be reset at start.
                session = R2T2StreamSession(runtime, audio_processor=processor)
                session.start(1)
                self.assertGreater(processor.reset_calls, 0)
                session.feed(1, pcm(0.8, 0.7, 0.6), offset=0)
                before = processor.reset_calls
                if action == "cancel":
                    response = session.cancel(1)
                    self.assertEqual((response["sample_count"], response["text"], response["final"]),
                                     (3, "", True))
                    self.assertGreater(processor.reset_calls, before)
                elif action == "close":
                    session.close()
                    self.assertFalse(session.active)
                    self.assertGreater(processor.reset_calls, before)
                session.start(2, language="en")
                self.assertGreater(processor.reset_calls, before)
                with self.assertRaises(ValueError):
                    session.feed(1, pcm(0.9), offset=3)
                session.feed(2, pcm(-0.1, -0.2, -0.3), offset=0)
                final = session.finish(2)
                self.assertEqual((final["session_id"], final["sample_count"], final["language"]), (2, 3, "en"))
                self.assertEqual(len(runtime.feed_inputs), 1)
                np.testing.assert_array_equal(runtime.feed_inputs[0], pcm(-0.1, -0.2, -0.3) * np.float32(0.5))

    def test_processor_feed_or_flush_error_invalidates_and_next_session_is_clean(self):
        for stage in ("feed", "flush"):
            with self.subTest(stage=stage):
                processor, runtime = BufferedProcessor(), RecordingRuntime()
                session = R2T2StreamSession(runtime, audio_processor=processor)
                session.start(1)
                session.feed(1, pcm(0.8, 0.7, 0.6), offset=0)
                failure = RuntimeError("injected processor failure")
                before = processor.reset_calls
                if stage == "feed":
                    processor.feed_errors.append(failure)
                    operation = lambda: session.feed(1, pcm(0.5), offset=3)
                else:
                    processor.flush_errors.append(failure)
                    operation = lambda: session.finish(1)
                with self.assertRaises(RuntimeError) as caught:
                    operation()
                self.assertIs(caught.exception, failure)
                self.assertFalse(session.active)
                self.assertGreater(processor.reset_calls, before)
                self.assertGreater(runtime.reset_calls, 0)
                with self.assertRaises(ValueError):
                    session.finish(1)
                session.start(2)
                session.feed(2, pcm(-0.1, -0.2, -0.3), offset=0)
                self.assertEqual(session.finish(2)["sample_count"], 3)
                self.assertEqual(len(runtime.feed_inputs), 1)
                np.testing.assert_array_equal(runtime.feed_inputs[0], pcm(-0.1, -0.2, -0.3) * np.float32(0.5))

    def test_native_feed_or_finish_error_resets_processor_before_restart(self):
        for stage in ("feed", "finish"):
            with self.subTest(stage=stage):
                processor, runtime = BufferedProcessor(), RecordingRuntime()
                session = R2T2StreamSession(runtime, audio_processor=processor)
                session.start(1)
                session.feed(1, pcm(0.8, 0.7, 0.6), offset=0)
                failure = RuntimeError("injected native failure")
                before = processor.reset_calls
                if stage == "feed":
                    runtime.feed_errors.append(failure)
                    operation = lambda: session.feed(1, pcm(0.5), offset=3)
                else:
                    runtime.finish_errors.append(failure)
                    operation = lambda: session.finish(1)
                with self.assertRaises(RuntimeError) as caught:
                    operation()
                self.assertIs(caught.exception, failure)
                self.assertFalse(session.active)
                self.assertGreater(processor.reset_calls, before)
                with self.assertRaises(ValueError):
                    session.feed(1, pcm(0.1), offset=0)
                session.start(2)
                session.feed(2, pcm(-0.1, -0.2, -0.3), offset=0)
                self.assertEqual(session.finish(2)["sample_count"], 3)
                np.testing.assert_array_equal(runtime.feed_inputs[-1], pcm(-0.1, -0.2, -0.3) * np.float32(0.5))

    def test_native_text_must_remain_monotonic_on_processed_feed_flush_and_finish(self):
        for stage in ("feed", "flush", "finish"):
            with self.subTest(stage=stage):
                processor = BufferedProcessor(block=2)
                runtime = RecordingRuntime(feed_results=[("abc", "en"), ("ab", "en")],
                                           finish_result=("ab", "en"))
                session = R2T2StreamSession(runtime, audio_processor=processor)
                session.start(1)
                self.assertEqual(session.feed(1, pcm(0.1, 0.2), offset=0)["text"], "abc")
                before = processor.reset_calls
                if stage == "feed":
                    operation = lambda: session.feed(1, pcm(0.3, 0.4), offset=2)
                else:
                    if stage == "flush":
                        self.assertEqual(session.feed(1, pcm(0.3), offset=2)["text"], "abc")
                    operation = lambda: session.finish(1)
                with self.assertRaises(RuntimeError):
                    operation()
                self.assertFalse(session.active)
                self.assertGreater(processor.reset_calls, before)
                self.assertEqual(runtime.finish_calls, int(stage == "finish"))

    def test_qwen_suppressed_preview_keeps_processed_audio_and_flushes_full_tail(self):
        processor = BufferedProcessor(block=2048)
        transcribe = RecordingTranscribe(("final transcript", "English"))
        runtime = QwenPreviewRuntime(transcribe)
        session = R2T2StreamSession(runtime, audio_processor=processor)
        session.start(1, context="terminology", language="en")
        raw = np.linspace(-0.5, 0.5, 5120, dtype=np.float32)
        first = session.feed(1, raw[:1920], offset=0, preview=False)
        second = session.feed(1, raw[1920:], offset=1920, preview=False)
        self.assertEqual((first["sample_count"], second["sample_count"]), (1920, 5120))
        self.assertEqual((first["text"], second["text"], second["tentative_text"]), ("", "", ""))
        self.assertEqual(transcribe.calls, [], "suppressed feeds must not trigger Qwen previews")

        final = session.finish(1)
        self.assertEqual((final["text"], final["sample_count"], final["final"]), ("final transcript", 5120, True))
        self.assertEqual(processor.flush_calls, 1)
        self.assertEqual(len(transcribe.calls), 1)
        audio, options = transcribe.calls[0]
        np.testing.assert_array_equal(audio, np.pad(raw * np.float32(0.5), (0, 8000 - len(raw))))
        self.assertEqual(options, {"context": "terminology", "language": "en"})
        self.assertFalse(session.active)

    def test_explicit_none_preserves_unprocessed_native_path(self):
        runtime = RecordingRuntime(feed_results=[("ok", "en")], finish_result=("ok final", None))
        session = R2T2StreamSession(runtime, audio_processor=None)
        session.start(1, context="context", language="en")
        raw = pcm(0.1, 0.2, 0.3)
        response = session.feed(1, raw, offset=0)
        self.assertEqual((response["text"], response["sample_count"]), ("ok", 3))
        np.testing.assert_array_equal(runtime.feed_inputs[0], raw)
        final = session.finish(1)
        self.assertEqual((final["text"], final["sample_count"], final["language"]), ("ok final", 3, "en"))
        self.assertEqual([event[0] for event in runtime.events], ["start", "feed", "finish"])


if __name__ == "__main__":
    unittest.main()
