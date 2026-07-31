import os
import sys
import unittest

import numpy as np

sys.path.insert(0, os.path.dirname(__file__))

from firered_vad import FireRedVad, FireRedVadOptions


class FireRedVadPostprocessingTests(unittest.TestCase):
    def _vad(self, **overrides):
        vad = object.__new__(FireRedVad)
        options = {
            "smooth_window_frames": 1,
            "min_speech_duration_ms": 100,
            "min_silence_duration_ms": 200,
            "speech_pad_ms": 0,
        }
        options.update(overrides)
        vad.options = FireRedVadOptions(**options)
        return vad

    def test_rejects_short_probability_spike(self):
        probabilities = np.concatenate(
            [np.zeros(10), np.ones(9), np.zeros(30)]
        ).astype(np.float32)

        self.assertEqual(
            self._vad()._timestamps_from_probabilities(probabilities, 8_000),
            [],
        )

    def test_emits_and_merges_padded_speech_segments(self):
        vad = self._vad(speech_pad_ms=120)
        probabilities = np.concatenate(
            [
                np.zeros(20),
                np.ones(15),
                np.zeros(20),
                np.ones(15),
                np.zeros(30),
            ]
        ).astype(np.float32)

        self.assertEqual(
            vad._timestamps_from_probabilities(probabilities, 16_000),
            [{"start": 1_280, "end": 13_120}],
        )


class FireRedVadAssetTests(unittest.TestCase):
    def test_bundled_model_rejects_one_second_of_silence(self):
        vad = FireRedVad()
        probabilities = vad.probabilities(np.zeros(16_000, dtype=np.float32))

        self.assertEqual(probabilities.shape, (98,))
        self.assertEqual(vad.speech_timestamps(np.zeros(16_000, dtype=np.float32)), [])


if __name__ == "__main__":
    unittest.main()
