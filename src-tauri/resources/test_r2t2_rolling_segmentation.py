"""VAD keeps pause boundaries while the native rolling backend owns capacity."""
import unittest

import numpy as np

from r2t2_segmented import SegmentedR2T2
from test_r2t2_segmented import MarkerVad, RecordingNative


class RollingSegmentationTests(unittest.TestCase):
    def test_continuous_audio_crosses_old_limit_without_reset_or_sample_loss(self):
        audio = np.ones(480001, dtype=np.float32)
        native = RecordingNative(
            feed_results=[("", None)] * 94, finish_results=[("complete", "en")]
        )
        runtime = SegmentedR2T2(native, MarkerVad(), max_segment_samples=None)
        runtime.start()
        runtime.feed(audio)
        self.assertEqual(native.finish_calls, 0)
        self.assertEqual(len(native.start_calls), 1)
        self.assertEqual(runtime.finish(), ("complete", "en"))
        self.assertEqual(native.finish_calls, 1)
        np.testing.assert_array_equal(np.concatenate(native.feed_inputs), audio)

    def test_disabling_capacity_cut_preserves_real_pause_segmentation(self):
        native = RecordingNative(
            feed_results=[("", None)] * 3, finish_results=[("first", "en")]
        )
        runtime = SegmentedR2T2(
            native, MarkerVad(), chunk_samples=4, window_samples=4,
            min_segment_samples=8, max_segment_samples=None, pause_samples=2,
        )
        runtime.start()
        runtime.feed(np.ones(8, dtype=np.float32))
        self.assertEqual(native.finish_calls, 0)
        self.assertEqual(runtime.feed(np.zeros(4, dtype=np.float32)), ("first", "en"))
        self.assertEqual(native.finish_calls, 1)
        self.assertEqual(runtime.finish(), ("first", "en"))
        self.assertEqual(native.finish_calls, 1)


if __name__ == "__main__":
    unittest.main()
