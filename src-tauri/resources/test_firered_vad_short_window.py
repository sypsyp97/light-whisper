import json
import unittest
import weakref
from unittest.mock import Mock, patch

import numpy as np

from firered_vad import FRAME_LENGTH_SAMPLES, FRAME_SHIFT_SAMPLES, FireRedVad, FireRedVadOptions


class FireRedVadShortWindowTests(unittest.TestCase):
    def test_frame_constants_remain_usable_as_sample_indices(self):
        self.assertIs(type(FRAME_LENGTH_SAMPLES), int)
        self.assertIs(type(FRAME_SHIFT_SAMPLES), int)
        samples = np.arange(800)
        np.testing.assert_array_equal(
            samples[:FRAME_LENGTH_SAMPLES:FRAME_SHIFT_SAMPLES],
            [0, 160, 320],
        )
        self.assertEqual(FRAME_LENGTH_SAMPLES, 400)

    def test_first_packet_and_default_boundary_use_only_eligible_frames(self):
        vad = FireRedVad()
        samples = np.zeros(2_640, dtype=np.float32)
        with (
            patch.object(vad, "_extract_features", wraps=vad._extract_features) as features,
            patch.object(vad._session, "run", wraps=vad._session.run) as inference,
        ):
            for length in (0, 2_560, 2_639):
                with self.subTest(samples=length):
                    self.assertEqual(vad.speech_timestamps(samples[:length]), [])
                    features.assert_not_called()
                    inference.assert_not_called()
            vad.speech_timestamps(samples.reshape(2, -1))
            features.assert_called_once()
            inference.assert_called_once()
            self.assertEqual(inference.call_args.args[1]["feat"].shape, (1, 15, 80))

    def test_custom_minimum_uses_full_frames_and_integer_milliseconds(self):
        # First eligible length measured for each setting with 25 ms Kaldi frames.
        for milliseconds, first_length in ((-10, 400), (0, 400), (9, 400), (19, 400),
                                           (25, 560), (39, 720), (209, 3_440)):
            with self.subTest(milliseconds=milliseconds):
                vad = FireRedVad(options=FireRedVadOptions(
                    threshold=0.0, min_speech_duration_ms=milliseconds,
                ))
                with patch.object(vad, "probabilities", wraps=vad.probabilities) as predict:
                    audio = np.zeros(first_length, dtype=np.float32)
                    self.assertEqual(vad.speech_timestamps(audio[:-1]), [])
                    predict.assert_not_called()
                    self.assertEqual(vad.speech_timestamps(audio), [{"start": 0, "end": first_length}])
                    predict.assert_called_once()

    def test_real_probabilities_and_timestamps_match_after_growth_and_shrink(self):
        audio = np.random.default_rng(37).normal(0, 0.1, 5_120).astype(np.float32)
        for threshold in (0.0, 0.5):
            vad = FireRedVad(options=FireRedVadOptions(threshold=threshold))
            reference = FireRedVad(options=vad.options)
            for length, frames in ((0, 0), (2_560, 14), (2_639, 14), (2_640, 15),
                                   (5_120, 30), (2_560, 14), (2_640, 15)):
                with self.subTest(threshold=threshold, samples=length):
                    window = audio[:length]
                    probabilities = reference.probabilities(window)
                    self.assertEqual(probabilities.shape, (frames,))
                    expected = reference._timestamps_from_probabilities(probabilities, length)
                    with patch.object(vad, "_session", wraps=vad._session) as session:
                        self.assertEqual(vad.speech_timestamps(window), expected)
                        self.assertEqual(session.run.call_count, int(frames >= 15))
                    actual = vad.probabilities(window)
                    self.assertEqual(actual.shape, probabilities.shape)
                    self.assertEqual(actual.dtype, probabilities.dtype)
                    self.assertEqual(actual.tobytes(), probabilities.tobytes())

    def test_eligible_window_preserves_smoothing_and_postprocessing(self):
        vad = FireRedVad(options=FireRedVadOptions(
            smooth_window_frames=3, min_speech_duration_ms=30,
            min_silence_duration_ms=20, speech_pad_ms=0,
        ))
        vad.probabilities = Mock(return_value=np.array([0.9, 0.3, 0.9, 0, 0], dtype=np.float32))
        regions = vad.speech_timestamps(np.zeros(1_040))
        self.assertEqual(json.dumps(regions), '[{"start": 0, "end": 480}]')
        vad.probabilities.assert_called_once()

    def test_short_replacement_releases_previous_cached_pcm(self):
        vad = FireRedVad()
        vad.speech_timestamps(np.zeros(5_120, dtype=np.float32))
        previous_pcm = weakref.ref(vad._feature_cache[0])
        self.assertIsNotNone(previous_pcm())
        self.assertEqual(vad.speech_timestamps(np.zeros(2_560, dtype=np.float32)), [])
        self.assertIsNone(previous_pcm())


if __name__ == "__main__":
    unittest.main()
