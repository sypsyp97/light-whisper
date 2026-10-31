"""RE-USE DSP and buffering through a fake native spectral driver."""

import os
import sys
import unittest
from unittest import mock

import numpy as np

sys.path.insert(0, os.path.dirname(__file__))

from reuse_enhancement import ReuseEnhancer


class _SpectralDriver:
    def __init__(self):
        self.gain = 1.0
        self.zero_bins = 0
        self.negative_magnitude = False
        self.inputs = []
        self.close_calls = 0

    def run(self, magnitude, phase):
        self.inputs.append((magnitude.copy(), phase.copy()))
        processed = np.log1p(np.expm1(magnitude.astype(np.float64)) * self.gain).astype(np.float32)
        if self.negative_magnitude:
            processed = -np.abs(processed)
        processed[:, :self.zero_bins] = 0
        return processed, np.cos(phase).astype(np.float32), np.sin(phase).astype(np.float32)

    def close(self):
        self.close_calls += 1


class ReuseEnhancementTests(unittest.TestCase):
    def setUp(self):
        self.driver = _SpectralDriver()
        patcher = mock.patch("reuse_enhancement.NativeReuseRuntime", return_value=self.driver)
        patcher.start()
        self.addCleanup(patcher.stop)
        self.enhancer = ReuseEnhancer()
        self.addCleanup(self.enhancer.close)
        self.rng = np.random.default_rng(20261030)

    def _audio(self, samples):
        return self.rng.uniform(-0.2, 0.2, samples).astype(np.float32)

    def _assert_audio(self, audio, samples=None):
        self.assertIsInstance(audio, np.ndarray)
        self.assertEqual(audio.dtype, np.float32)
        self.assertEqual(audio.ndim, 1)
        self.assertTrue(np.isfinite(audio).all())
        if samples is not None:
            self.assertEqual(len(audio), samples)

    def _assert_spectral_inputs(self):
        self.assertTrue(self.driver.inputs, "Nonempty audio never reached the spectral driver")
        for magnitude, phase in self.driver.inputs:
            self.assertEqual(magnitude.shape, phase.shape)
            self.assertEqual(magnitude.ndim, 2)
            self.assertEqual(magnitude.shape[1], 321)
            self.assertGreaterEqual(magnitude.shape[0], 9)  # 640 samples / hop 80, centered.
            self.assertLessEqual(magnitude.shape[0], 201)  # At most one 16,000-sample window.
            for values in (magnitude, phase):
                self.assertEqual(values.dtype, np.float32)
                self.assertTrue(np.isfinite(values).all())
            self.assertTrue((magnitude >= 0).all())

    def test_forward_spectra_use_centered_reflect_periodic_hann_and_hop_80(self):
        source = self._audio(640)
        padded = np.pad(source, (320, 320), mode="reflect")
        window = np.hanning(641)[:-1]  # Independent periodic Hann oracle.
        frames = np.stack([
            padded[offset:offset + 640] * window
            for offset in range(0, len(padded) - 640 + 1, 80)
        ])
        expected = np.fft.rfft(frames, n=640, axis=1)

        self.enhancer.enhance(source)

        self.assertEqual(len(self.driver.inputs), 1)
        magnitude, phase = self.driver.inputs[0]
        self.assertEqual(magnitude.shape, (9, 321))
        self.assertEqual(phase.shape, magnitude.shape)
        np.testing.assert_allclose(magnitude, np.log1p(np.abs(expected)), rtol=0, atol=1e-5)
        actual = np.expm1(magnitude.astype(np.float64)) * np.exp(1j * phase.astype(np.float64))
        # Magnitude-weighted phase comparison remains stable near zero-energy bins.
        np.testing.assert_allclose(actual, expected, rtol=0, atol=1e-5)

    def test_identity_roundtrip_preserves_short_unaligned_and_multiwindow_audio(self):
        for samples in (1, 17, 319, 639, 640, 641, 719, 8000, 16000, 16001, 52173):
            with self.subTest(samples=samples):
                source = self._audio(samples)
                original = source.copy()
                output = self.enhancer.enhance(source)

                self._assert_audio(output, samples)
                np.testing.assert_allclose(output, original, rtol=0, atol=1e-6)
                np.testing.assert_array_equal(source, original)

        self._assert_spectral_inputs()

    def test_half_amplitude_driver_changes_audio_without_gain_normalization(self):
        self.driver.gain = 0.5
        source = self._audio(48137)

        output = self.enhancer.enhance(source)

        self._assert_audio(output, len(source))
        np.testing.assert_allclose(output, source * np.float32(0.5), rtol=0, atol=1e-6)
        self._assert_spectral_inputs()

    def test_arbitrary_stream_chunks_match_whole_clip_without_lost_or_duplicated_samples(self):
        source = self._audio(52173)
        chunk_sizes = (1, 79, 2001, 17, 16003, 431, 7000, 3, 8192)
        for gain in (1.0, 0.5):
            with self.subTest(gain=gain):
                self.driver.gain = gain
                expected = self.enhancer.enhance(source)
                self._assert_audio(expected, len(source))
                self.enhancer.reset()
                outputs = []
                offset = 0
                chunk = 0
                while offset < len(source):
                    end = min(len(source), offset + chunk_sizes[chunk % len(chunk_sizes)])
                    output = self.enhancer.feed(source[offset:end])
                    self._assert_audio(output)
                    outputs.append(output)
                    offset = end
                    chunk += 1
                tail = self.enhancer.flush()
                self._assert_audio(tail)
                output = np.concatenate([*outputs, tail])

                self._assert_audio(output, len(source))
                np.testing.assert_allclose(output, expected, rtol=0, atol=1e-6)
                np.testing.assert_allclose(output, source * np.float32(gain), rtol=0, atol=1e-6)
                self._assert_audio(self.enhancer.flush(), 0)

        self._assert_spectral_inputs()

    def test_stream_waits_for_one_second_then_emits_steps_and_flushes_held_tail(self):
        source = self._audio(30000)

        self._assert_audio(self.enhancer.feed(source[:15999]), 0)
        self.assertEqual(len(self.driver.inputs), 0)
        first = self.enhancer.feed(source[15999:16000])
        self._assert_audio(first, 14000)
        self.assertEqual(len(self.driver.inputs), 1)
        np.testing.assert_allclose(first, source[:14000], rtol=0, atol=1e-6)

        self._assert_audio(self.enhancer.feed(source[16000:29999]), 0)
        self.assertEqual(len(self.driver.inputs), 1)
        second = self.enhancer.feed(source[29999:])
        self._assert_audio(second, 14000)
        self.assertEqual(len(self.driver.inputs), 2)
        tail = self.enhancer.flush()
        self._assert_audio(tail, 2000)

        np.testing.assert_allclose(np.concatenate((first, second, tail)), source, rtol=0, atol=1e-6)
        self._assert_audio(self.enhancer.flush(), 0)
        self._assert_spectral_inputs()

    def test_invalid_audio_is_rejected_before_native_work_and_valid_audio_still_works(self):
        invalid = (
            np.ones(2, dtype=np.float64),
            np.ones(2, dtype=np.int16),
            np.ones(2, dtype=np.bool_),
            np.ones((2, 3), dtype=np.float32),
            np.array(0.1, dtype=np.float32),
            np.array([np.nan], dtype=np.float32),
            np.array([np.inf], dtype=np.float32),
            np.array([-np.inf], dtype=np.float32),
        )
        for operation in (self.enhancer.enhance, self.enhancer.feed):
            for audio in invalid:
                with self.subTest(operation=operation.__name__, dtype=audio.dtype, shape=audio.shape):
                    with self.assertRaises((TypeError, ValueError)):
                        operation(audio)
                    self.assertEqual(len(self.driver.inputs), 0)

        source = self._audio(643)
        output = self.enhancer.enhance(source)
        self._assert_audio(output, len(source))
        np.testing.assert_allclose(output, source, rtol=0, atol=1e-6)

    def test_empty_reset_and_close_are_safe_and_reset_discards_all_buffered_audio(self):
        empty = np.empty(0, dtype=np.float32)
        self._assert_audio(self.enhancer.enhance(empty), 0)
        self._assert_audio(self.enhancer.feed(empty), 0)
        self._assert_audio(self.enhancer.flush(), 0)
        self.assertEqual(len(self.driver.inputs), 0)

        self._assert_audio(self.enhancer.feed(self._audio(15999)), 0)
        self.enhancer.reset()
        self.enhancer.reset()
        self._assert_audio(self.enhancer.flush(), 0)
        self.assertEqual(len(self.driver.inputs), 0)

        self._assert_audio(self.enhancer.feed(self._audio(16000)), 14000)
        self.enhancer.reset()
        replacement = self._audio(213)
        self._assert_audio(self.enhancer.feed(replacement), 0)
        output = self.enhancer.flush()
        self._assert_audio(output, len(replacement))
        np.testing.assert_allclose(output, replacement, rtol=0, atol=1e-6)

        self.enhancer.close()
        self.enhancer.close()
        self.assertEqual(self.driver.close_calls, 1)
        calls = len(self.driver.inputs)
        for operation in (lambda: self.enhancer.enhance(replacement),
                          lambda: self.enhancer.feed(replacement), self.enhancer.flush):
            with self.assertRaises((RuntimeError, ValueError)):
                operation()
        self.assertEqual(len(self.driver.inputs), calls)

    def test_inverse_relu_and_strict_majority_zero_bin_suppression(self):
        source = self._audio(643)
        self.driver.negative_magnitude = True
        output = self.enhancer.enhance(source)
        self._assert_audio(output, len(source))
        np.testing.assert_allclose(output, 0, rtol=0, atol=1e-9)

        self.driver.negative_magnitude = False
        self.driver.zero_bins = 161  # More than half of 321 bins: suppress the whole frame.
        output = self.enhancer.enhance(source)
        self._assert_audio(output, len(source))
        np.testing.assert_allclose(output, 0, rtol=0, atol=1e-9)

        self.driver.zero_bins = 160  # Below half: remaining spectral content must survive.
        output = self.enhancer.enhance(source)
        self._assert_audio(output, len(source))
        self.assertGreater(float(np.max(np.abs(output))), 1e-3)


if __name__ == "__main__":
    unittest.main()
