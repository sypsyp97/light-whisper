

import gc
import inspect
import json
from pathlib import Path
import unittest
from unittest import mock
import weakref

import kaldi_native_fbank as knf
import numpy as np

from firered_vad import FireRedVad


RESOURCES = Path(__file__).resolve().parent
MAX_SAMPLES = 12 * 16_000
REAL_ONLINE_FBANK = knf.OnlineFbank


def _uncached_features(audio):

    options = knf.FbankOptions()
    options.frame_opts.samp_freq = 16_000
    options.frame_opts.frame_length_ms = 25
    options.frame_opts.frame_shift_ms = 10
    options.frame_opts.dither = 0
    options.frame_opts.snip_edges = True
    options.mel_opts.num_bins = 80
    options.mel_opts.debug_mel = False
    cmvn = json.loads((RESOURCES / "fireredvad_cmvn.json").read_text("utf-8"))
    samples = np.asarray(audio, dtype=np.float32).reshape(-1)
    pcm = np.clip(samples * 32768.0, -32768.0, 32767.0)
    fbank = knf.OnlineFbank(options)
    fbank.accept_waveform(16_000, pcm.tolist())
    if not fbank.num_frames_ready:
        return np.empty((0, 80), dtype=np.float32)
    frames = np.asarray(
        [fbank.get_frame(i) for i in range(fbank.num_frames_ready)],
        dtype=np.float32,
    )
    return np.ascontiguousarray(
        (frames - np.asarray(cmvn["mean"], dtype=np.float32))
        * np.asarray(cmvn["inverse_std"], dtype=np.float32),
        dtype=np.float32,
    )


class _TrackedFbank:
    def __init__(self, owner, options):
        self.owner = owner
        self.inner = REAL_ONLINE_FBANK(options)
        self.accepted_samples = 0

    def accept_waveform(self, sample_rate, samples):
        self.owner.accepted_samples += len(samples)
        self.accepted_samples += len(samples)
        result = self.inner.accept_waveform(sample_rate, samples)
        self.owner.maybe_fail("accept")
        return result

    def get_frame(self, index):
        self.owner.frame_reads += 1
        result = self.inner.get_frame(index)
        self.owner.maybe_fail("frame")
        return result

    def __getattr__(self, name):
        return getattr(self.inner, name)


class _FbankWork:
    def __init__(self):
        self.accepted_samples = 0
        self.frame_reads = 0
        self.created = 0
        self.live = weakref.WeakSet()
        self.failure = None

    def OnlineFbank(self, options):
        result = _TrackedFbank(self, options)
        self.created += 1
        self.live.add(result)
        return result

    def __getattr__(self, name):
        return getattr(knf, name)

    def maybe_fail(self, stage):
        if self.failure and self.failure[0] == stage:
            self.failure[1] -= 1
            if self.failure[1] == 0:
                self.failure = None
                raise RuntimeError("injected partial fbank failure")

    def snapshot(self):
        return self.accepted_samples, self.frame_reads


class _SessionCalls:
    def __init__(self, inner):
        self.inner = inner
        self.calls = 0

    def run(self, *args, **kwargs):
        self.calls += 1
        return self.inner.run(*args, **kwargs)

    def __getattr__(self, name):
        return getattr(self.inner, name)


class FireRedVadFeatureCacheTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.audio = np.random.default_rng(2801).uniform(
            -0.7, 0.7, MAX_SAMPLES + 16_000
        ).astype(np.float32)

    def _vad(self):
        work = _FbankWork()

        with mock.patch.object(knf, "OnlineFbank", work.OnlineFbank):
            vad = FireRedVad(
                model_path=RESOURCES / "fireredvad_vad.onnx",
                cmvn_path=RESOURCES / "fireredvad_cmvn.json",
            )
        vad._knf = work
        return vad, work

    def _features(self, vad, audio):
        actual = vad._extract_features(audio)
        expected = _uncached_features(audio)
        np.testing.assert_array_equal(actual, expected)
        self.assertEqual(actual.tobytes(), expected.tobytes())
        self.assertEqual(actual.dtype, np.float32)
        self.assertTrue(actual.flags.c_contiguous)
        self.assertEqual(actual.shape[1:], (80,))
        return actual

    def test_public_signatures_remain_unchanged(self):
        parameters = {
            "__init__": ("self", "model_path", "cmvn_path", "options"),
            "_extract_features": ("self", "audio"),
            "probabilities": ("self", "audio"),
            "speech_timestamps": ("self", "audio"),
            "warmup": ("self",),
        }
        for method, names in parameters.items():
            with self.subTest(method=method):
                actual = inspect.signature(getattr(FireRedVad, method)).parameters
                self.assertEqual(tuple(actual), names)
                for name, parameter in actual.items():
                    self.assertEqual(parameter.kind, inspect.Parameter.POSITIONAL_OR_KEYWORD)
                    default = None if method == "__init__" and name != "self" else inspect.Parameter.empty
                    self.assertIs(parameter.default, default)

    def test_exact_repeat_does_no_additional_fbank_work(self):
        vad, work = self._vad()
        self._features(vad, self.audio[:8_013])
        before = work.snapshot()
        self._features(vad, self.audio[:8_013].copy())
        self.assertEqual(work.snapshot(), before)

    def test_growing_subframe_and_one_thirteen_sample_tails(self):
        vad, work = self._vad()
        lengths = (0, 1, 13, 159, 399, 400, 401, 413, 559, 560, 573, 8_000, 8_001, 8_013, 8_160, 8_173)
        for length in lengths:
            with self.subTest(samples=length):
                features = self._features(vad, self.audio[:length])
                self.assertEqual(features.shape[0], max(0, (length - 400) // 160 + 1))
                self.assertEqual(work.accepted_samples, length)
                self.assertEqual(work.frame_reads, features.shape[0])

    def test_append_silence_and_loud_audio_reuses_only_prefix_work(self):
        for silent_first in (True, False):
            with self.subTest(silent_first=silent_first):
                vad, work = self._vad()
                silence = np.zeros(4_013, dtype=np.float32)
                loud = self.audio[:5_601] * 3
                first, last = (silence, loud) if silent_first else (loud, silence)
                self._features(vad, first)
                full = np.concatenate((first, last))
                features = self._features(vad, full)
                self.assertEqual(work.accepted_samples, len(full))
                self.assertEqual(work.frame_reads, len(features))

    def test_changed_shorter_unrelated_and_sliding_inputs_rebuild(self):
        original = self.audio[:8_013].copy()
        changed = original.copy()
        changed[999] += 0.125
        changed_tail = original.copy()
        changed_tail[-1] += 0.125
        cases = {
            "prefix_changed": changed,
            "incomplete_tail_changed": changed_tail,
            "shorter": original[:4_001],
            "unrelated": self.audio[9_000:17_013],
            "sliding": self.audio[160:8_173],
        }
        for name, audio in cases.items():
            with self.subTest(change=name):
                vad, work = self._vad()
                self._features(vad, original)
                before_samples, before_frames = work.snapshot()
                features = self._features(vad, audio)
                self.assertEqual(work.accepted_samples - before_samples, len(audio))
                self.assertEqual(work.frame_reads - before_frames, len(features))
                before = work.snapshot()
                self._features(vad, audio.copy())
                self.assertEqual(work.snapshot(), before)

                self._features(vad, np.concatenate((audio, self.audio[20_000:20_400])))

    def test_clipped_noncontiguous_and_float64_inputs_match_baseline(self):
        for audio in (
            self.audio[:8_013] * 4,
            self.audio[:16_026:2],
            self.audio[:8_013].astype(np.float64),
            self.audio[:8_000].reshape(1_000, 8),
        ):
            with self.subTest(dtype=str(audio.dtype), stride=audio.strides):
                vad, work = self._vad()
                self._features(vad, audio)
                before = work.snapshot()
                self._features(vad, audio.copy())
                self.assertEqual(work.snapshot(), before)

    def test_input_mutation_does_not_mutate_owned_prefix(self):
        vad, work = self._vad()
        audio = self.audio[:8_013].copy()
        self._features(vad, audio)
        audio[999] += 0.25
        before = work.accepted_samples
        self._features(vad, audio)
        self.assertEqual(work.accepted_samples - before, len(audio))
        before = work.snapshot()
        self._features(vad, audio.copy())
        self.assertEqual(work.snapshot(), before)

    def test_returned_features_do_not_mutate_owned_cache(self):
        vad, work = self._vad()
        result = self._features(vad, self.audio[:8_013])
        result[:] = -123.0
        before = work.snapshot()
        repeated = self._features(vad, self.audio[:8_013])
        self.assertFalse(np.shares_memory(result, repeated))
        self.assertEqual(work.snapshot(), before)
        repeated[:] = 432.0
        self._features(vad, self.audio[:8_173])

    def test_instances_do_not_share_feature_state(self):
        first, first_work = self._vad()
        second, second_work = self._vad()
        self._features(first, self.audio[:8_013])
        self._features(second, self.audio[:8_013])
        self.assertEqual(second_work.accepted_samples, 8_013)
        self._features(first, self.audio[:8_173])
        self._features(second, self.audio[16_000:24_013])
        before = first_work.snapshot()
        self._features(first, self.audio[:8_173])
        self.assertEqual(first_work.snapshot(), before)

    def test_only_latest_prefix_is_retained(self):
        vad, work = self._vad()
        self._features(vad, self.audio[:8_013])
        self._features(vad, self.audio[16_000:24_013])
        before = work.accepted_samples
        self._features(vad, self.audio[:8_013])
        self.assertEqual(work.accepted_samples - before, 8_013)
        gc.collect()
        self.assertLessEqual(len(work.live), 1)

    def test_twelve_second_limit_bypasses_and_clears_oversized_inputs(self):
        vad, work = self._vad()
        at_limit = self.audio[:MAX_SAMPLES]
        self._features(vad, at_limit)
        before = work.snapshot()
        self._features(vad, at_limit)
        self.assertEqual(work.snapshot(), before)
        for audio in (self.audio[:MAX_SAMPLES + 1], self.audio[:MAX_SAMPLES + 1], at_limit):
            before_samples, before_frames = work.snapshot()
            features = self._features(vad, audio)
            self.assertEqual(work.accepted_samples - before_samples, len(audio))
            self.assertEqual(work.frame_reads - before_frames, len(features))
            gc.collect()
            self.assertLessEqual(len(work.live), 1)
            self.assertTrue(all(f.accepted_samples <= MAX_SAMPLES for f in work.live))
            if len(audio) > MAX_SAMPLES:
                self.assertTrue(all(f.accepted_samples == 0 for f in work.live))

    def test_partial_extraction_failures_discard_state_and_retry_cleanly(self):
        for stage, countdown in (("accept", 1), ("frame", 2)):
            with self.subTest(stage=stage):
                vad, work = self._vad()
                self._features(vad, self.audio[:8_013])
                work.failure = [stage, countdown]
                with self.assertRaisesRegex(RuntimeError, "injected partial fbank failure"):
                    vad._extract_features(self.audio[:9_013])
                before_samples, before_frames = work.snapshot()
                features = self._features(vad, self.audio[:9_013])
                self.assertEqual(work.accepted_samples - before_samples, 9_013)
                self.assertEqual(work.frame_reads - before_frames, len(features))
                before = work.snapshot()
                self._features(vad, self.audio[:9_013])
                self.assertEqual(work.snapshot(), before)

    def test_probabilities_and_regions_stay_exact_and_inference_runs_every_call(self):
        vad, work = self._vad()
        vad._session = _SessionCalls(vad._session)
        for length in (13, 4_001, 8_013, 8_173, 4_001):
            with self.subTest(samples=length):
                audio = self.audio[:length]
                features = _uncached_features(audio)
                expected = (
                    np.asarray(vad._session.inner.run(None, {"feat": features[None, :, :]})[0], dtype=np.float32).reshape(-1)
                    if len(features) else np.empty(0, dtype=np.float32)
                )
                actual = vad.probabilities(audio)
                np.testing.assert_array_equal(actual, expected)
                self.assertEqual(actual.tobytes(), expected.tobytes())
                before = work.snapshot()
                calls = vad._session.calls
                regions = vad.speech_timestamps(audio)
                self.assertEqual(regions, vad._timestamps_from_probabilities(expected, len(audio)))
                self.assertEqual(vad._session.calls - calls, int(bool(len(features))))
                self.assertEqual(work.snapshot(), before)


if __name__ == "__main__":
    unittest.main()
