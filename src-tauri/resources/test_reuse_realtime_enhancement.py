"""Incremental Real-time RE-USE DSP; NumPy test data, no model or DLL required."""

import os
import sys
import unittest
from unittest import mock

import numpy as np

sys.path.insert(0, os.path.dirname(__file__))

import reuse_enhancement as enhancement


FFT = 640
HOP = 320
BINS = 321
# Independent periodic-Hann definition; synthesis and analysis use float32 taps.
WINDOW = (0.5 - 0.5 * np.cos(2 * np.pi * np.arange(FFT) / FFT)).astype(np.float32)
# FFT results and model heads cross the native seam as float32. These tolerances
# allow their rounding, while rejecting a one-sample shift or a wrong head index.
PCM_ATOL = 3e-6
PCM_RTOL = 2e-5


def _audio(count):
    """Deterministic synthetic test PCM, independent of capture devices."""
    return np.random.default_rng(20261031 + count).uniform(-0.2, 0.2, count).astype(np.float32)


def _batch_spectra(audio):
    """Whole-utterance oracle, deliberately independent of streaming buffers."""
    if not len(audio):
        return np.empty((0, BINS), np.float32), np.empty((0, BINS), np.float32), 0
    padded_count = max(FFT, ((len(audio) + HOP - 1) // HOP) * HOP)
    waveform = np.full(padded_count, 1e-8, dtype=np.float32)
    waveform[:len(audio)] = audio
    centered = np.pad(waveform, (HOP, HOP), mode="reflect")
    frames = np.stack([
        centered[center:center + FFT]
        for center in range(0, padded_count + 1, HOP)
    ])
    spectrum = np.fft.rfft(frames * WINDOW, n=FFT, axis=1).astype(np.complex64)
    return np.log1p(np.abs(spectrum)).astype(np.float32), np.angle(spectrum).astype(np.float32), padded_count


def _batch_oracle(audio, lookahead, mode="stateful"):
    magnitude, phase, padded_count = _batch_spectra(audio)
    if not len(audio):
        return np.empty(0, dtype=np.float32)
    count = len(magnitude)
    if mode == "stateful":
        past_mag = np.concatenate((np.zeros((1, BINS), np.float32), magnitude[:-1]))
        past_phase = np.concatenate((np.zeros((1, BINS), np.float32), phase[:-1]))
        zeros = np.zeros((lookahead, BINS), np.float32)
        future_mag = np.concatenate((magnitude, zeros))[lookahead:lookahead + count]
        future_phase = np.concatenate((phase, zeros))[lookahead:lookahead + count]
        indices = np.arange(count, dtype=np.float32)[:, None]
        heads = (np.float32(0.61) * magnitude + np.float32(0.07) * past_mag
                 + np.float32(0.025) * future_mag + np.float32(0.006) * (indices % 4))
        angles = (phase + np.float32(0.08) * np.sin(past_phase)
                  + np.float32(0.05) * np.sin(future_phase) + np.float32(0.03) * (indices % 3))
        real = np.float32(1.7) * np.cos(angles) + np.float32(0.11)
        imag = np.float32(0.7) * np.sin(angles) - np.float32(0.09)
    else:
        heads = magnitude.copy()
        real, imag = np.cos(phase), np.sin(phase)
        if mode == "negative":
            heads = -np.abs(heads) - np.float32(0.1)
        elif mode.startswith("zeros"):
            heads[:, :int(mode[5:])] = 0
    linear = np.expm1(np.maximum(heads, np.float32(0)))
    linear[np.count_nonzero(linear == 0, axis=1) > BINS / 2] = 0
    spectrum = linear * np.exp(1j * np.arctan2(imag, real))
    frames = np.fft.irfft(spectrum, n=FFT, axis=1) * WINDOW
    output = np.zeros(padded_count + FFT, dtype=np.float64)
    weights = np.zeros_like(output)
    for index, frame in enumerate(frames):
        start = index * HOP
        output[start:start + FFT] += frame
        weights[start:start + FFT] += WINDOW ** 2
    selected = slice(HOP, HOP + len(audio))
    return (output[selected] / weights[selected]).astype(np.float32)


class _DelayedSpectralDriver:
    """Fake native seam: first L+1 frames, then one; each call returns one head.

    Stateful heads depend on target, preceding, and lookahead frames plus the
    target index. Raw phase heads deliberately have unequal scales and offsets.
    Output buffers are reused, as a caller must consume native results promptly.
    """

    def __init__(self, model_path=None, library_path=None, backend=None, threads=2,
                 exit_layers=8, lookahead=1):
        self.backend = backend or "cpu"
        self.lookahead = lookahead
        self.exit_layers = exit_layers
        self.mode = "stateful"
        self.calls = []
        self.reset_calls = 0
        self.close_calls = 0
        self.closed = False
        self._history = []
        self._index = 0
        self._outputs = [np.empty((1, BINS), np.float32) for _ in range(3)]

    def run(self, magnitude, phase):
        if self.closed:
            raise AssertionError("DSP called a closed native runtime")
        expected = self.lookahead + 1 if not self._history else 1
        for values in (magnitude, phase):
            if (not isinstance(values, np.ndarray) or values.dtype != np.float32
                    or values.shape != (expected, BINS) or not np.isfinite(values).all()):
                raise AssertionError(f"native input must be finite float32 [{expected},321]")
        self.calls.append((magnitude.copy(), phase.copy()))
        self._history.extend((mag.copy(), angle.copy()) for mag, angle in zip(magnitude, phase))
        current_mag, current_phase = self._history[self._index]
        if self.mode == "stateful":
            if self._index:
                past_mag, past_phase = self._history[self._index - 1]
            else:
                past_mag = past_phase = np.zeros(BINS, np.float32)
            future_mag, future_phase = self._history[self._index + self.lookahead]
            head = (np.float32(0.61) * current_mag + np.float32(0.07) * past_mag
                    + np.float32(0.025) * future_mag + np.float32(0.006 * (self._index % 4)))
            angle = (current_phase + np.float32(0.08) * np.sin(past_phase)
                     + np.float32(0.05) * np.sin(future_phase) + np.float32(0.03 * (self._index % 3)))
            real = np.float32(1.7) * np.cos(angle) + np.float32(0.11)
            imag = np.float32(0.7) * np.sin(angle) - np.float32(0.09)
        else:
            head = current_mag.copy()
            real, imag = np.cos(current_phase), np.sin(current_phase)
            if self.mode == "negative":
                head = -np.abs(head) - np.float32(0.1)
            elif self.mode.startswith("zeros"):
                head[:int(self.mode[5:])] = 0
        for target, value in zip(self._outputs, (head, real, imag)):
            target[0] = value
        self._index += 1
        return self._outputs

    def reset(self):
        if self.closed:
            raise AssertionError("reset after close must not call the native runtime")
        self.reset_calls += 1
        self._history.clear()
        self._index = 0

    def close(self):
        self.close_calls += 1
        self.closed = True


class RealtimeReuseEnhancementTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        missing = [name for name in ("NativeRealtimeReuseRuntime", "RealtimeReuseEnhancer")
                   if not hasattr(enhancement, name)]
        if missing:
            raise RuntimeError("INTERFACE_PENDING: " + ", ".join(missing))

    def _make(self, mode="stateful", **options):
        drivers = []

        def construct(**kwargs):
            driver = _DelayedSpectralDriver(**kwargs)
            driver.mode = mode
            drivers.append(driver)
            return driver

        with mock.patch.object(enhancement, "NativeRealtimeReuseRuntime", side_effect=construct) as factory:
            enhancer = enhancement.RealtimeReuseEnhancer(**options)
        self.assertEqual(len(drivers), 1)
        factory.assert_called_once()
        for name, value in options.items():
            self.assertEqual(factory.call_args.kwargs.get(name), value)
        self.addCleanup(enhancer.close)
        return enhancer, drivers[0]

    def _assert_pcm(self, output, count=None):
        self.assertIsInstance(output, np.ndarray)
        self.assertEqual(output.dtype, np.float32)
        self.assertEqual(output.ndim, 1)
        self.assertTrue(np.isfinite(output).all())
        self.assertTrue(output.flags.owndata, "returned PCM must own its storage")
        if count is not None:
            self.assertEqual(len(output), count)

    def _assert_close_audio(self, actual, expected):
        self._assert_pcm(actual, len(expected))
        np.testing.assert_allclose(actual, expected, rtol=PCM_RTOL, atol=PCM_ATOL)

    def _assert_calls(self, calls, source, lookahead):
        if not len(source):
            self.assertEqual(calls, [])
            return
        magnitude, phase, _ = _batch_spectra(source)
        self.assertEqual([len(mag) for mag, _ in calls], [lookahead + 1] + [1] * (len(magnitude) - 1))
        actual_mag = np.concatenate([mag for mag, _ in calls])
        actual_phase = np.concatenate([angle for _, angle in calls])
        np.testing.assert_allclose(actual_mag[:len(magnitude)], magnitude, rtol=0, atol=1e-5)
        # Phase is undefined in zero-energy bins; compare reconstructed spectra.
        actual_complex = np.expm1(actual_mag[:len(magnitude)]) * np.exp(1j * actual_phase[:len(magnitude)])
        expected_complex = np.expm1(magnitude) * np.exp(1j * phase)
        np.testing.assert_allclose(actual_complex, expected_complex, rtol=0, atol=2e-5)
        np.testing.assert_array_equal(actual_mag[len(magnitude):], np.zeros((lookahead, BINS), np.float32))
        np.testing.assert_array_equal(actual_phase[len(magnitude):], np.zeros((lookahead, BINS), np.float32))

    def _stream(self, enhancer, driver, source, sizes):
        pieces, snapshots = [], []
        resets = driver.reset_calls
        offset, index = 0, 0
        while offset < len(source):
            end = min(len(source), offset + sizes[index % len(sizes)])
            output = enhancer.feed(source[offset:end])
            self._assert_pcm(output)
            self.assertEqual(driver.reset_calls, resets, "feed must preserve native time/frequency state")
            if len(output):
                pieces.append(output)
                snapshots.append(output.copy())
            offset, index = end, index + 1
        tail = enhancer.flush()
        self._assert_pcm(tail)
        for output, snapshot in zip(pieces, snapshots):
            np.testing.assert_array_equal(output, snapshot)
        return np.concatenate([*pieces, tail])

    def test_runtime_defaults_and_explicit_runtime_options(self):
        _, driver = self._make()
        self.assertEqual(driver.exit_layers, 8)
        self.assertEqual(driver.lookahead, 1)
        _, driver = self._make(model_path="test-model.safetensors", library_path="test-reuse.dll",
                               backend="cpu", threads=1, exit_layers=4, lookahead=2)
        self.assertEqual(driver.exit_layers, 4)
        self.assertEqual(driver.lookahead, 2)
        self.assertEqual(driver.backend, "cpu")

    def test_identity_roundtrip_for_boundary_lengths_and_all_chunk_partitions(self):
        lengths = (0, 1, 319, 320, 321, 639, 640, 641, 959, 960, 961, 16000, 16037)
        for lookahead in (0, 1, 2):
            for count in lengths:
                source = _audio(count)
                before = source.tobytes()
                for sizes in ((1,), (7, 319, 2, 641, 13), (640,), (max(count, 1),)):
                    with self.subTest(lookahead=lookahead, count=count, sizes=sizes):
                        enhancer, driver = self._make("identity", lookahead=lookahead)
                        actual = self._stream(enhancer, driver, source, sizes)
                        self._assert_close_audio(actual, source)
                        self._assert_calls(driver.calls, source, lookahead)
                        self.assertEqual(source.tobytes(), before)
                        calls = len(driver.calls)
                        self._assert_pcm(enhancer.flush(), 0)
                        self.assertEqual(len(driver.calls), calls)

    def test_stateful_delayed_heads_match_independent_whole_utterance_oracle(self):
        for lookahead in (0, 1, 2):
            for count in (1, 321, 641, 961, 2567, 16037):
                source = _audio(count)
                expected = _batch_oracle(source, lookahead)
                if count >= 641:
                    self.assertGreater(float(np.max(np.abs(expected - source))), 1e-3)
                for sizes in ((1,), (79, 3, 947, 321, 1), (640,), (count,)):
                    with self.subTest(lookahead=lookahead, count=count, sizes=sizes):
                        enhancer, driver = self._make(lookahead=lookahead)
                        self._assert_close_audio(self._stream(enhancer, driver, source, sizes), expected)
                        self._assert_calls(driver.calls, source, lookahead)

    def test_first_pcm_block_at_40_60_80_ms_then_continues_incrementally(self):
        source = _audio(16037)
        for lookahead in (0, 1, 2):
            with self.subTest(lookahead=lookahead):
                enhancer, driver = self._make(lookahead=lookahead)
                expected = _batch_oracle(source, lookahead)
                threshold = (2 + lookahead) * HOP
                self._assert_pcm(enhancer.feed(source[:threshold - 1]), 0)
                first = enhancer.feed(source[threshold - 1:threshold])
                self._assert_close_audio(first, expected[:HOP])
                self._assert_pcm(enhancer.feed(source[threshold:threshold + 1]), 0)
                outputs, offset, emitted = [first], threshold + 1, HOP
                resets = driver.reset_calls
                while offset < len(source):
                    end = min(len(source), offset + 640)
                    output = enhancer.feed(source[offset:end])
                    expected_total = (end // HOP - lookahead - 1) * HOP
                    self._assert_close_audio(output, expected[emitted:expected_total])
                    self.assertEqual(driver.reset_calls, resets)
                    outputs.append(output)
                    emitted, offset = expected_total, end
                self.assertLess(len(source) - emitted, threshold + HOP)
                outputs.append(enhancer.flush())
                self._assert_close_audio(np.concatenate(outputs), expected)
                self._assert_calls(driver.calls, source, lookahead)

    def test_eof_padding_preserves_last_sample_impulses_and_uses_epsilon(self):
        for lookahead in (0, 1, 2):
            for count in (1, 319, 320, 321, 639, 640, 641, 959, 960, 961):
                with self.subTest(lookahead=lookahead, count=count):
                    source = np.zeros(count, np.float32)
                    source[-1] = np.float32(0.5)
                    enhancer, driver = self._make("identity", lookahead=lookahead)
                    output = enhancer.feed(source)
                    if count < FFT:
                        self._assert_pcm(output, 0)
                    tail = enhancer.flush()
                    self._assert_close_audio(np.concatenate((output, tail)), source)
                    self._assert_calls(driver.calls, source, lookahead)
            enhancer, driver = self._make("identity", lookahead=lookahead)
            source = np.zeros(1, np.float32)
            self._assert_close_audio(enhancer.enhance(source), source)
            expected_mag, _, _ = _batch_spectra(source)
            actual_mag = np.concatenate([mag for mag, _ in driver.calls])[:len(expected_mag)]
            self.assertGreater(float(np.max(expected_mag)), 1e-6)
            np.testing.assert_allclose(actual_mag, expected_mag, rtol=3e-6, atol=5e-12)

    def test_inverse_clips_logmag_and_suppresses_only_strict_majority_zero_bins(self):
        source = _audio(1287)
        for lookahead in (0, 1, 2):
            for mode in ("negative", "zeros160", "zeros161"):
                with self.subTest(lookahead=lookahead, mode=mode):
                    enhancer, driver = self._make(mode, lookahead=lookahead)
                    actual = self._stream(enhancer, driver, source, (321, 1, 640))
                    self._assert_close_audio(actual, _batch_oracle(source, lookahead, mode))
                    if mode == "zeros160":
                        self.assertGreater(float(np.max(np.abs(actual))), 1e-3)
                    else:
                        np.testing.assert_allclose(actual, 0, rtol=0, atol=1e-9)

    def test_reset_discards_pending_pcm_and_native_history_between_utterances(self):
        for lookahead in (0, 1, 2):
            with self.subTest(lookahead=lookahead):
                enhancer, driver = self._make(lookahead=lookahead)
                enhancer.feed(_audio(2049))
                enhancer.feed(_audio(7))
                calls = len(driver.calls)
                resets = driver.reset_calls
                enhancer.reset()
                self.assertGreater(driver.reset_calls, resets)
                self._assert_pcm(enhancer.flush(), 0)
                self.assertEqual(len(driver.calls), calls)
                source = _audio(2567)
                expected = _batch_oracle(source, lookahead)
                for _ in range(2):
                    start = len(driver.calls)
                    self._assert_close_audio(self._stream(enhancer, driver, source, (640, 1, 319)), expected)
                    self._assert_calls(driver.calls[start:], source, lookahead)

    def test_empty_feed_is_noop_and_returned_pcm_is_owned_and_stable(self):
        enhancer, driver = self._make("identity")
        empty = np.empty(0, np.float32)
        resets = driver.reset_calls
        self._assert_pcm(enhancer.feed(empty), 0)
        self.assertEqual(driver.reset_calls, resets)
        self.assertEqual(driver.calls, [])
        self._assert_pcm(enhancer.flush(), 0)
        self._assert_pcm(enhancer.flush(), 0)
        self._assert_pcm(enhancer.enhance(empty), 0)
        self.assertEqual(driver.calls, [])
        # Strided input is still valid mono float32 PCM and must remain untouched.
        storage = _audio(6000)
        before = storage.tobytes()
        source = storage[::2]
        first = enhancer.feed(source[:960])
        self._assert_close_audio(first, source[:320])
        snapshot = first.copy()
        enhancer.feed(source[960:])
        tail = enhancer.flush()
        self._assert_pcm(tail)
        tail_snapshot = tail.copy()
        enhancer.reset()
        enhancer.enhance(_audio(17))
        np.testing.assert_array_equal(first, snapshot)
        np.testing.assert_array_equal(tail, tail_snapshot)
        self.assertEqual(storage.tobytes(), before)

    def test_invalid_pcm_does_not_advance_an_active_utterance(self):
        invalid = (
            [0.1], None, np.float32(0.1),
            np.ones(2, np.float64), np.ones(2, np.int16), np.ones(2, np.bool_),
            np.ones(2, np.complex64), np.ones((2, 3), np.float32), np.array(0.1, np.float32),
            np.array([np.nan], np.float32), np.array([np.inf], np.float32),
            np.array([-np.inf], np.float32),
        )
        source = _audio(2567)
        for lookahead in (0, 1, 2):
            with self.subTest(lookahead=lookahead):
                enhancer, driver = self._make(lookahead=lookahead)
                prefix = enhancer.feed(source[:639])
                self._assert_pcm(prefix, 0)
                for operation in (enhancer.feed, enhancer.enhance):
                    for value in invalid:
                        calls, resets = len(driver.calls), driver.reset_calls
                        with self.assertRaises((TypeError, ValueError)):
                            operation(value)
                        self.assertEqual(len(driver.calls), calls)
                        self.assertEqual(driver.reset_calls, resets)
                actual = np.concatenate((prefix, enhancer.feed(source[639:]), enhancer.flush()))
                self._assert_close_audio(actual, _batch_oracle(source, lookahead))
                self._assert_calls(driver.calls, source, lookahead)

    def test_enhance_replaces_partial_stream_and_reuses_runtime(self):
        source = _audio(2567)
        for lookahead in (0, 1, 2):
            with self.subTest(lookahead=lookahead):
                enhancer, driver = self._make(lookahead=lookahead)
                enhancer.feed(_audio(2111))
                for _ in range(2):
                    start = len(driver.calls)
                    actual = enhancer.enhance(source)
                    self._assert_close_audio(actual, _batch_oracle(source, lookahead))
                    self._assert_calls(driver.calls[start:], source, lookahead)
                    self._assert_pcm(enhancer.flush(), 0)

    def test_close_is_idempotent_and_rejects_processing_without_native_calls(self):
        enhancer, driver = self._make()
        source = _audio(1607)
        output = enhancer.feed(source)
        self._assert_pcm(output)
        saved = output.copy()
        enhancer.close()
        enhancer.close()
        self.assertEqual(driver.close_calls, 1)
        calls, resets = len(driver.calls), driver.reset_calls
        for operation in (lambda: enhancer.feed(source), lambda: enhancer.feed(source[:0]),
                          enhancer.flush, lambda: enhancer.enhance(source)):
            with self.assertRaises((RuntimeError, ValueError)):
                operation()
        enhancer.reset()
        self.assertEqual(len(driver.calls), calls)
        self.assertEqual(driver.reset_calls, resets)
        np.testing.assert_array_equal(output, saved)


if __name__ == "__main__":
    unittest.main()
