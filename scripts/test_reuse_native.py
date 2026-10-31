"""Opt-in full 30-layer RE-USE C ABI parity against precomputed ONNX outputs.

Run: REUSE_NATIVE_LIBRARY=/absolute/path/reuse.dll python scripts/test_reuse_native.py -v
REUSE_NATIVE_DLL_DIRS supplies dependency directories separated by os.pathsep.
REUSE_NATIVE_MODEL and REUSE_NATIVE_FIXTURE_ROOT override the local investigation
checkpoint and fixture locations. REUSE_NATIVE_BACKEND defaults to cpu; select
cuda explicitly only when GPU execution is available. No oracle model is run.
"""

from contextlib import ExitStack
import ctypes as ct
import hashlib
import os
from pathlib import Path
import unittest

import numpy as np


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_ROOT = ROOT / "tmp/reuse-investigation"
MODEL_SHA256 = "06badaf4029b297f97f7fe48d62990b48d5482608210df577354e0d4e0ed70e1"
FFT_SIZE, HOP_SIZE, BINS = 640, 80, 321
FLOAT_POINTER = ct.POINTER(ct.c_float)


def pointer(array):
    return array.ctypes.data_as(FLOAT_POINTER)


def reconstruct_audio(magnitude, phase, samples):
    """Independent NumPy overlap-add for NVIDIA's 16 kHz inference transform.

    Mirrors inference.py's sweep suppression and stfts.py's ReLU/log1p inverse,
    periodic Hann, unnormalized centered ISTFT; does not import native/model code.
    Inputs are frame-major compressed magnitude and phase in radians.
    """
    magnitude = np.expm1(np.maximum(magnitude.astype(np.float64), 0))
    magnitude[np.mean(magnitude == 0, axis=1) > 0.5] = 0
    spectrum = magnitude * np.exp(1j * phase.astype(np.float64))
    window = np.hanning(FFT_SIZE + 1)[:-1]  # Periodic, like torch.hann_window.
    frames = np.fft.irfft(spectrum, n=FFT_SIZE, axis=1) * window
    padded_samples = FFT_SIZE + (len(frames) - 1) * HOP_SIZE
    audio, weights = np.zeros(padded_samples), np.zeros(padded_samples)
    for index, frame in enumerate(frames):
        start = index * HOP_SIZE
        audio[start:start + FFT_SIZE] += frame
        weights[start:start + FFT_SIZE] += window ** 2
    audio = audio[FFT_SIZE // 2:-FFT_SIZE // 2]
    weights = weights[FFT_SIZE // 2:-FFT_SIZE // 2]
    if not np.all(weights > 0):
        raise ValueError("ISTFT fixture has an uncovered output sample")
    audio /= weights
    # Official pad_or_trim_to_match(..., pad_value=1e-8).
    return np.pad(audio[:samples], (0, max(0, samples - len(audio))), constant_values=1e-8)


class NativeApi:
    def __init__(self, library):
        signatures = {
            "reuse_create": (ct.c_void_p, [ct.c_char_p, ct.c_char_p, ct.c_int]),
            "reuse_run": (ct.c_int, [ct.c_void_p, FLOAT_POINTER, FLOAT_POINTER, ct.c_int,
                                     FLOAT_POINTER, FLOAT_POINTER, FLOAT_POINTER]),
            "reuse_free": (None, [ct.c_void_p]),
            "reuse_last_error": (ct.c_char_p, []),
        }
        for name, (result, arguments) in signatures.items():
            try:
                function = getattr(library, name)
            except AttributeError as error:
                raise RuntimeError(f"INTERFACE_PENDING: required C ABI export {name} missing") from error
            function.restype, function.argtypes = result, arguments
            setattr(self, name, function)

    def error(self):
        return (self.reuse_last_error() or b"").decode("utf-8", errors="replace")


class ReuseNativeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        library = os.environ.get("REUSE_NATIVE_LIBRARY")
        if not library:
            raise unittest.SkipTest("REUSE_NATIVE_LIBRARY unset; native RE-USE ABI parity is opt-in")
        cls.model = Path(os.environ.get("REUSE_NATIVE_MODEL", DEFAULT_ROOT / "model.safetensors")).resolve()
        with cls.model.open("rb") as source:
            actual_hash = hashlib.file_digest(source, "sha256").hexdigest()
        if actual_hash != MODEL_SHA256:
            raise AssertionError(f"30-layer checkpoint SHA-256 mismatch: {actual_hash}")
        cls.backend = os.environ.get("REUSE_NATIVE_BACKEND", "cpu").encode("ascii")
        cls.threads = int(os.environ.get("REUSE_NATIVE_THREADS", "2"))
        cls.stack = ExitStack()
        cls.addClassCleanup(cls.stack.close)
        library = Path(library).resolve()
        if os.name == "nt":
            directories = [str(library.parent), *filter(None, os.environ.get("REUSE_NATIVE_DLL_DIRS", "").split(os.pathsep))]
            for directory in dict.fromkeys(directories):
                cls.stack.enter_context(os.add_dll_directory(str(Path(directory).resolve())))
        cls.api = NativeApi(ct.CDLL(str(library)))
        fixture_root = Path(os.environ.get("REUSE_NATIVE_FIXTURE_ROOT", DEFAULT_ROOT))
        cls.fixtures = {}
        for samples in (640, 2560, 8000):
            with np.load(fixture_root / f"oracle-{samples}.npz", allow_pickle=False) as archive:
                fixture = {}
                for key in ("magnitude", "phase", "enhanced_magnitude", "enhanced_phase"):
                    values = archive[key]
                    expected_shape = (1, BINS, samples // HOP_SIZE + 1)
                    if values.dtype != np.float32 or values.shape != expected_shape or not np.isfinite(values).all():
                        raise AssertionError(f"oracle-{samples} {key}: expected finite float32{expected_shape}")
                    fixture[key] = np.ascontiguousarray(values[0].T)
                audio = archive["audio"]
                if audio.dtype != np.float32 or audio.shape != (samples,) or not np.isfinite(audio).all():
                    raise AssertionError(f"oracle-{samples} audio: expected finite float32[{samples}]")
                fixture["audio"] = audio.copy()
            cls.fixtures[samples] = fixture

    def create_handle(self):
        handle = self.api.reuse_create(str(self.model).encode("utf-8"), self.backend, self.threads)
        self.assertTrue(handle, f"reuse_create failed: {self.api.error()}")
        self.addCleanup(self.api.reuse_free, handle)
        return handle

    def test_full_model_parity_repeated_inputs_and_changed_frame_lengths(self):
        handle = self.create_handle()
        # Same input twice, grow, shrink, then revisit: graph data must refresh.
        for invocation, samples in enumerate((640, 640, 8000, 2560, 640)):
            fixture = self.fixtures[samples]
            magnitude, phase = fixture["magnitude"], fixture["phase"]
            outputs = [np.full(magnitude.shape, np.nan, dtype=np.float32) for _ in range(3)]
            with self.subTest(samples=samples, invocation=invocation):
                status = self.api.reuse_run(handle, pointer(magnitude), pointer(phase), len(magnitude),
                                            *[pointer(output) for output in outputs])
                self.assertEqual(status, 0, f"reuse_run failed: {self.api.error()}")
                for name, output in zip(("magnitude", "phase_real", "phase_imag"), outputs):
                    self.assertEqual(output.shape, magnitude.shape, name)
                    self.assertTrue(np.isfinite(output).all(), f"{name}: output not fully written or nonfinite")
                actual_magnitude, real, imag = outputs
                actual_phase = np.arctan2(imag, real)
                expected_magnitude, expected_phase = fixture["enhanced_magnitude"], fixture["enhanced_phase"]
                expected_audio = reconstruct_audio(expected_magnitude, expected_phase, samples)
                actual_audio = reconstruct_audio(actual_magnitude, actual_phase, samples)
                mag_error = float(np.max(np.abs(actual_magnitude - expected_magnitude)))
                waveform_error = float(np.max(np.abs(actual_audio - expected_audio)))
                phase_error = float(np.max(np.abs(np.angle(np.exp(1j * (actual_phase - expected_phase))))))
                diagnostic = (f"magnitude_max_abs={mag_error:.9g}, waveform_max_abs={waveform_error:.9g}, "
                              f"wrapped_phase_max_abs={phase_error:.9g} (phase diagnostic only)")
                # Raw atan2 is unstable near zero; waveform parity gates phase.
                self.assertTrue(np.allclose(actual_magnitude, expected_magnitude, atol=0.003, rtol=0.001), diagnostic)
                self.assertLessEqual(waveform_error, 0.0003, diagnostic)

    def test_oracle_input_spectra_reconstruct_fixture_audio(self):
        for samples, fixture in self.fixtures.items():
            with self.subTest(samples=samples):
                audio = reconstruct_audio(fixture["magnitude"], fixture["phase"], samples)
                np.testing.assert_allclose(audio, fixture["audio"], atol=1e-7, rtol=1e-5)

    def test_create_rejects_bad_backend(self):
        handle = self.api.reuse_create(str(self.model).encode("utf-8"), b"unsupported-test-backend", self.threads)
        if handle:
            self.api.reuse_free(handle)
        self.assertFalse(handle, "reuse_create must reject an unsupported backend")

    def test_create_rejects_missing_model(self):
        missing = self.model.parent / "missing-reuse-test-model.safetensors"
        self.assertFalse(missing.exists(), "missing-model test path unexpectedly exists")
        handle = self.api.reuse_create(str(missing).encode("utf-8"), self.backend, self.threads)
        if handle:
            self.api.reuse_free(handle)
        self.assertFalse(handle, "reuse_create must reject a missing checkpoint")

    def test_run_rejects_invalid_frames_and_null_arguments(self):
        handle = self.create_handle()
        fixture = self.fixtures[640]
        outputs = [np.empty_like(fixture["magnitude"]) for _ in range(3)]
        arguments = [handle, pointer(fixture["magnitude"]), pointer(fixture["phase"]),
                     len(fixture["magnitude"]), *[pointer(output) for output in outputs]]
        for frames in (0, -1):
            with self.subTest(frames=frames):
                invalid = arguments.copy()
                invalid[3] = frames
                self.assertNotEqual(self.api.reuse_run(*invalid), 0, "invalid frame count accepted")
        for index, name in ((0, "handle"), (1, "magnitude"), (2, "phase"),
                            (4, "out_magnitude"), (5, "out_phase_real"), (6, "out_phase_imag")):
            with self.subTest(null_argument=name):
                invalid = arguments.copy()
                invalid[index] = None
                self.assertNotEqual(self.api.reuse_run(*invalid), 0, f"null {name} accepted")

    def test_free_null_is_safe(self):
        self.api.reuse_free(None)


if __name__ == "__main__":
    unittest.main()
