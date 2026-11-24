import unittest
from unittest.mock import patch

import numpy as np

from r2t2_segmented import SegmentedR2T2
from r2t2_stream import R2T2StreamSession
from test_r2t2_segmented import MarkerVad, RecordingNative


def make_session(**options):
    native = RecordingNative(
        feed_results=[("", None)] * 100,
        finish_results=[("speech", "en")] * 100,
    )
    vad = MarkerVad()
    segmenter = SegmentedR2T2(native, vad, chunk_samples=4, pause_samples=100, **options)
    return R2T2StreamSession(segmenter), segmenter, native, vad


def as_bytes(windows):
    return [window.tobytes() for window in windows]


class VadHistoryWorkTests(unittest.TestCase):
    def test_windows_that_expire_before_vad_eligibility_are_never_built(self):
        for window, cap, minimum, expected_active_ends in [
            (8, None, 20, [16, 20]),
            (8, 40, 20, [16, 20]),
            (32, 9, 22, []),
        ]:
            with self.subTest(window=window, cap=cap):
                session, segmenter, native, vad = make_session(
                    window_samples=window, max_segment_samples=cap,
                    min_segment_samples=minimum,
                )
                pcm = np.arange(1, 21, dtype=np.float32)
                active_builds = []
                real_window = segmenter._vad_window

                def record_build(block):
                    if segmenter._native_active:
                        active_builds.append(int(block[-1]))
                    return real_window(block)

                session.start(1)
                with patch.object(segmenter, "_vad_window", record_build):
                    session.feed(1, pcm, offset=0)
                    session.finish(1)
                self.assertEqual(as_bytes(native.feed_inputs), as_bytes(np.split(pcm, 5)))
                expected_vad = [pcm[:4], pcm[12:16]] if cap == 9 else [pcm[:4], pcm[12:]]
                self.assertEqual(as_bytes(vad.inputs), as_bytes(expected_vad))
                self.assertEqual(active_builds, expected_active_ends)

    def test_one_sample_window_builds_only_for_observed_blocks(self):
        session, segmenter, native, vad = make_session(
            window_samples=1, max_segment_samples=None, min_segment_samples=17,
        )
        pcm = np.arange(1, 21, dtype=np.float32)
        session.start(1)
        with patch.object(segmenter, "_vad_window", wraps=segmenter._vad_window) as build:
            session.feed(1, pcm, offset=0)
            session.finish(1)
        self.assertEqual(as_bytes(vad.inputs), as_bytes([pcm[3:4], pcm[-1:]]))
        self.assertEqual(as_bytes(native.feed_inputs),
                         as_bytes([pcm[3:4], *np.split(pcm[4:], 4)]))
        self.assertEqual(build.call_count, 2)

    def test_first_eligible_window_keeps_one_sample_across_short_finish_and_overshoot(self):
        for count in (17, 20):
            with self.subTest(count=count):
                session, _, native, vad = make_session(
                    window_samples=6, max_segment_samples=None, min_segment_samples=17,
                )
                pcm = np.arange(1, count + 1, dtype=np.float32)
                session.start(1)
                for offset in range(0, count, 3):
                    session.feed(1, pcm[offset:offset + 3], offset=offset)
                self.assertEqual(session.finish(1)["text"], "speech")
                self.assertEqual(as_bytes(vad.inputs), as_bytes([pcm[:4], pcm[-6:]]))
                self.assertEqual(as_bytes(native.feed_inputs),
                                 as_bytes([pcm[i:i + 4] for i in range(0, count, 4)]))

    def test_preroll_counts_toward_threshold_and_cancel_discards_old_audio(self):
        session, _, native, vad = make_session(
            window_samples=6, max_segment_samples=None, min_segment_samples=15,
        )
        pcm = np.arange(1, 22, dtype=np.float32)
        pcm[:4] = 0
        session.start(1)
        session.feed(1, np.full(13, 99, np.float32), offset=0)
        session.cancel(1)
        session.start(2)
        session.feed(2, pcm, offset=0)
        final = session.finish(2)
        self.assertEqual((final["sample_count"], final["text"]), (21, "speech"))
        self.assertEqual(as_bytes(native.feed_inputs[3:]), as_bytes([
            pcm[2:8], pcm[8:12], pcm[12:16], pcm[16:20], pcm[20:],
        ]))
        self.assertEqual(as_bytes(vad.inputs[1:]), as_bytes([
            pcm[:4], pcm[2:8], pcm[14:20], pcm[15:21],
        ]))

    def test_nonpositive_segment_capacity_preserves_each_onset_slice(self):
        pcm = np.arange(1, 10, dtype=np.float32)
        blocks = [pcm[i:i + 4] for i in range(0, pcm.size, 4)]
        for cap in (0, -1):
            with self.subTest(cap=cap):
                session, _, native, vad = make_session(
                    window_samples=6, max_segment_samples=cap, min_segment_samples=14,
                )
                session.start(1)
                session.feed(1, pcm, offset=0)
                session.finish(1)
                expected_vad = [block[-cap:] for block in blocks]
                self.assertEqual(as_bytes(vad.inputs), as_bytes(expected_vad))
                self.assertEqual(as_bytes(native.feed_inputs),
                                 as_bytes([observed[-cap:] for observed in expected_vad
                                           if observed.size]))

    def test_nonpositive_window_preserves_legacy_python_slice_inputs(self):
        pcm = np.arange(1, 19, dtype=np.float32)
        for window in (0, -1, -2):
            with self.subTest(window=window):
                session, _, native, vad = make_session(
                    window_samples=window, max_segment_samples=None, min_segment_samples=14,
                )
                blocks = [pcm[i:i + 4] for i in range(0, pcm.size, 4)]
                # Zero retains every sample. A negative bound drops a fixed prefix
                # at every step, including blocks for which VAD itself is skipped.
                history = []
                windows = []
                segment_size = 0
                for i, block in enumerate(blocks):
                    history = (history + block.tolist())[-window:]
                    if i == 0 or segment_size + len(block) >= 14:
                        windows.append(np.array(history, dtype=np.float32))
                    segment_size = len(history) if i == 0 else segment_size + len(block)
                session.start(1)
                session.feed(1, pcm, offset=0)
                session.finish(1)
                self.assertEqual(as_bytes(vad.inputs), as_bytes(windows))
                self.assertEqual(as_bytes(native.feed_inputs), as_bytes([windows[0], *blocks[1:]]))


if __name__ == "__main__":
    unittest.main()
