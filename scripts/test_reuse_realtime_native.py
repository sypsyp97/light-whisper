"""Opt-in stateful Real-time RE-USE spectral C ABI acceptance tests.

REUSE_RT_LIBRARY=/absolute/path/reuse.dll python scripts/test_reuse_realtime_native.py -v
REUSE_RT_MODEL and REUSE_RT_AUDIO_FIXTURE override pinned local research inputs.
REUSE_RT_BACKEND is strictly cpu (default) or cuda; REUSE_NATIVE_DLL_DIRS supplies
dependency directories. The independent NumPy batch oracle uses at most two BLAS
threads. No model download, dependency install, app integration, or benchmark.
"""

from contextlib import ExitStack
import ctypes as ct
from functools import lru_cache
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest

for _name in ("OPENBLAS_NUM_THREADS", "OMP_NUM_THREADS", "MKL_NUM_THREADS"):
    os.environ[_name] = "2"

import numpy as np

from native_tests.reuse_realtime_reference import BINS, Reference, frequency_shuffle, load_weights, spectral_inputs


ROOT = Path(__file__).resolve().parents[1]
FLOAT_POINTER = ct.POINTER(ct.c_float)
# Gate raw decoder heads, avoiding atan2's singularity around (0, 0).
# F32/F64 calibration across CONFIGURATIONS records the error relative to
# ATOL + RTOL*abs(reference); the margin permits different F32 reduction orders.
ATOL, RTOL = 0.0003, 0.0003
CONFIGURATIONS = ((3, 0), (12, 0), (3, 1), (8, 1), (12, 1), (3, 2), (12, 2))


def pointer(array):
    return array.ctypes.data_as(FLOAT_POINTER)


class NativeApi:
    def __init__(self, library):
        signatures = {
            "reuse_rt_create": (ct.c_void_p, [ct.c_char_p, ct.c_char_p, ct.c_int, ct.c_int, ct.c_int]),
            "reuse_rt_run": (ct.c_int, [ct.c_void_p, FLOAT_POINTER, FLOAT_POINTER, ct.c_int,
                                        FLOAT_POINTER, FLOAT_POINTER, FLOAT_POINTER]),
            "reuse_rt_reset": (ct.c_int, [ct.c_void_p]),
            "reuse_rt_free": (None, [ct.c_void_p]),
            "reuse_rt_last_error": (ct.c_char_p, []),
        }
        for name, (result, arguments) in signatures.items():
            try:
                function = getattr(library, name)
            except AttributeError as error:
                raise RuntimeError(f"INTERFACE_PENDING: required C ABI export {name} missing") from error
            function.restype, function.argtypes = result, arguments
            setattr(self, name, function)

    def error(self):
        return (self.reuse_rt_last_error() or b"").decode("utf-8", errors="replace")


def load_api(library, stack):
    library = Path(library).resolve()
    if os.name == "nt":
        directories = [str(library.parent), *filter(None, os.environ.get("REUSE_NATIVE_DLL_DIRS", "").split(os.pathsep))]
        for directory in dict.fromkeys(directories):
            stack.enter_context(os.add_dll_directory(str(Path(directory).resolve())))
    return NativeApi(ct.CDLL(str(library)))


def probe_create(path, backend, exit_layers, lookahead):
    """Crash-prone checkpoint validation runs in a child process, never the suite."""
    with ExitStack() as stack:
        api = load_api(os.environ["REUSE_RT_LIBRARY"], stack)
        handle = api.reuse_rt_create(os.fsencode(path), backend.encode(), 2, exit_layers, lookahead)
        if handle:
            api.reuse_rt_free(handle)
        return {"created": bool(handle), "error": api.error()}


class ReferenceLayoutTests(unittest.TestCase):
    def test_frequency_subpixel_matches_documented_index_equation(self):
        # Literal index equation derived from pinned codec's [R,C,H,W] view.
        frames, frequencies, ratio, channels = 2, 3, 2, 4
        source = np.arange(frames * frequencies * ratio * channels).reshape(frames, frequencies, ratio * channels)
        expected = np.empty((frames, frequencies * ratio, channels), dtype=source.dtype)
        for t in range(frames):
            for f in range(frequencies):
                for r in range(ratio):
                    for c in range(channels):
                        expected[t, f * ratio + r, c] = source[t, f, r * channels + c]
        np.testing.assert_array_equal(frequency_shuffle(source), expected)


class ReuseRealtimeNativeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        library = os.environ.get("REUSE_RT_LIBRARY")
        if not library:
            raise unittest.SkipTest("REUSE_RT_LIBRARY unset; Real-time RE-USE spectral ABI is opt-in")
        cls.stack = ExitStack()
        cls.addClassCleanup(cls.stack.close)
        cls.api = load_api(library, cls.stack)
        cls.model = Path(os.environ.get("REUSE_RT_MODEL", ROOT / "tmp/reuse-realtime-20261031/model.safetensors")).resolve()
        cls.backend = os.environ.get("REUSE_RT_BACKEND", "cpu").encode("ascii")
        cls.weights = load_weights(cls.model)
        audio = os.environ.get("REUSE_RT_AUDIO_FIXTURE", ROOT / "tmp/reuse-investigation/oracle-2560.npz")
        cls.magnitude, cls.phase = spectral_inputs(audio)

    def create(self, exit_layers=3, lookahead=2, *, cleanup=True):
        handle = self.api.reuse_rt_create(os.fsencode(self.model), self.backend, 2, exit_layers, lookahead)
        self.assertTrue(handle, f"reuse_rt_create failed: {self.api.error()}")
        if cleanup:
            self.addCleanup(self.api.reuse_rt_free, handle)
        return handle

    def run_frame(self, handle, magnitude, phase):
        # Guard suffix catches implementations writing K output frames on priming.
        outputs = [np.full(3 * BINS + 8, 12345.0, dtype=np.float32) for _ in range(3)]
        for output in outputs:
            output[:BINS] = np.nan
        arguments = [handle, pointer(magnitude), pointer(phase), len(magnitude), *map(pointer, outputs)]
        status = self.checked_run(arguments, magnitude, phase)
        self.assertEqual(status, 0, f"reuse_rt_run failed: {self.api.error()}")
        for output in outputs:
            self.assertTrue(np.isfinite(output[:BINS]).all(), "one complete finite output frame required")
            np.testing.assert_array_equal(output[BINS:], 12345.0, err_msg="run wrote beyond one frame")
        return np.stack([output[:BINS] for output in outputs])

    def checked_run(self, arguments, magnitude, phase):
        snapshots = magnitude.tobytes(), phase.tobytes()
        status = self.api.reuse_rt_run(*arguments)
        for name, array, before in zip(("logmag", "phase"), (magnitude, phase), snapshots):
            self.assertTrue(array.tobytes() == before, f"reuse_rt_run mutated const {name} input")
        return status

    def chunks(self, lookahead, magnitude=None, phase=None):
        magnitude = self.magnitude if magnitude is None else magnitude
        phase = self.phase if phase is None else phase
        magnitude = np.pad(magnitude, ((0, lookahead), (0, 0)))
        phase = np.pad(phase, ((0, lookahead), (0, 0)))
        yield magnitude[:lookahead + 1], phase[:lookahead + 1]
        for index in range(lookahead + 1, len(magnitude)):
            yield magnitude[index:index + 1], phase[index:index + 1]

    def stream(self, handle, lookahead, magnitude=None, phase=None):
        return np.stack([self.run_frame(handle, mag, pha)
                         for mag, pha in self.chunks(lookahead, magnitude, phase)], axis=1)

    @classmethod
    @lru_cache(maxsize=3)
    def expected(cls, lookahead):
        exits = (3, 8, 12) if lookahead == 1 else (3, 12)
        return Reference(cls.weights).enhance(cls.magnitude, cls.phase, lookahead, exits)

    def assert_spectra_close(self, actual, expected):
        error = float(np.max(np.abs(actual - expected)))
        np.testing.assert_allclose(actual, expected, atol=ATOL, rtol=RTOL,
                                   err_msg=f"raw spectral head max_abs={error:.9g}")

    def test_stream_matches_independent_batch_oracle_at_exit_and_lookahead_boundaries(self):
        for exit_layers, lookahead in CONFIGURATIONS:
            with self.subTest(exit_layers=exit_layers, lookahead=lookahead):
                handle = self.create(exit_layers, lookahead, cleanup=False)
                try:
                    actual = self.stream(handle, lookahead)
                    self.assertEqual(actual.shape, (3, len(self.magnitude), BINS))
                    self.assert_spectra_close(actual, self.expected(lookahead)[exit_layers])
                finally:
                    self.api.reuse_rt_free(handle)

    def test_reset_after_nonzero_history_repeats_fresh_stream_and_requires_priming(self):
        handle = self.create()
        chunks = list(self.chunks(2))
        for mag, pha in chunks[:17]:
            self.run_frame(handle, mag, pha)
        self.assertEqual(self.api.reuse_rt_reset(handle), 0, self.api.error())
        outputs = [np.zeros(BINS, dtype=np.float32) for _ in range(3)]
        arguments = [handle, pointer(self.magnitude), pointer(self.phase), 1, *map(pointer, outputs)]
        self.assertNotEqual(self.checked_run(arguments, self.magnitude, self.phase), 0,
                            "reset must restore K-frame priming")
        first = self.stream(handle, 2)
        self.assert_spectra_close(first, self.expected(2)[3])
        self.assertEqual(self.api.reuse_rt_reset(handle), 0, self.api.error())
        second = self.stream(handle, 2)
        np.testing.assert_array_equal(first, second, err_msg="reset must clear every model state")

    def test_interleaved_handles_with_different_configs_and_histories_are_isolated(self):
        handles = [self.create(3, 2), self.create(8, 1)]
        altered_mag = np.ascontiguousarray(self.magnitude[::-1])
        altered_pha = np.ascontiguousarray(-self.phase[::-1])
        streams = [list(self.chunks(2)), list(self.chunks(1, altered_mag, altered_pha))]
        actual = [[], []]
        for index in range(len(self.magnitude)):
            for lane in (1, 0):
                actual[lane].append(self.run_frame(handles[lane], *streams[lane][index]))
        self.assert_spectra_close(np.stack(actual[0], axis=1), self.expected(2)[3])
        # Separate fresh handles establish isolation for a distinct nonzero history.
        fresh = self.stream(self.create(8, 1), 1, altered_mag, altered_pha)
        np.testing.assert_array_equal(np.stack(actual[1], axis=1), fresh)

    def test_invalid_frames_and_null_arguments_do_not_advance_primed_or_unprimed_state(self):
        handle = self.create()
        chunks = list(self.chunks(2))
        for primed in (False, True):
            mag, pha = chunks[1 if primed else 0]
            outputs = [np.zeros(BINS + 8, dtype=np.float32) for _ in range(3)]
            arguments = [handle, pointer(mag), pointer(pha), len(mag), *map(pointer, outputs)]
            for frames in (0, -1, 2, 4, 2147483647, 3 if primed else 1):
                with self.subTest(primed=primed, invalid_frames=frames):
                    invalid = arguments.copy()
                    invalid[3] = frames
                    self.assertNotEqual(self.checked_run(invalid, mag, pha), 0, "invalid frame count accepted")
                    self.assertTrue(self.api.error(), "failure must expose a diagnostic")
            for index in (0, 1, 2, 4, 5, 6):
                with self.subTest(primed=primed, null_index=index):
                    invalid = arguments.copy()
                    invalid[index] = None
                    self.assertNotEqual(self.checked_run(invalid, mag, pha), 0, "null argument accepted")
            actual = self.run_frame(handle, mag, pha)
            self.assert_spectra_close(actual, self.expected(2)[3][:, int(primed)])

    def test_create_rejects_invalid_config_backend_and_checkpoint_without_aborting(self):
        with tempfile.TemporaryDirectory(prefix="reuse-rt-invalid-") as directory:
            directory = Path(directory)
            corrupt = directory / "corrupt.safetensors"
            corrupt.write_bytes(b"not a checkpoint")
            with self.model.open("rb") as source:
                header_bytes = struct.unpack("<Q", source.read(8))[0]
                header = json.loads(source.read(header_bytes))
                payload = source.read()
            # Keep the byte count valid: only architecture-specific shape validation rejects this.
            name = "dense_encoder.dense_conv_1_1.0.weight"
            shape = header[name]["shape"]
            self.assertNotEqual(shape[0], shape[1])
            shape[0], shape[1] = shape[1], shape[0]
            start, end = header[name]["data_offsets"]
            self.assertEqual(int(np.prod(shape)) * 4, end - start)
            encoded = json.dumps(header, separators=(",", ":")).encode()
            encoded += b" " * (-len(encoded) % 8)
            wrong_shape = directory / "wrong-shape.safetensors"
            wrong_shape.write_bytes(struct.pack("<Q", len(encoded)) + encoded + payload)
            offline = Path(os.environ.get("REUSE_RT_OFFLINE_MODEL", self.model.parent.parent / "reuse-investigation/model.safetensors"))
            self.assertTrue(offline.is_file(), "original offline checkpoint required for rejection coverage")
            cases = [(self.model, "cpu", e, l) for e, l in ((2, 0), (13, 0), (3, -1), (3, 3))]
            cases += [(self.model, "invalid-test-backend", 3, 0)]
            cases += [(path, "cpu", 3, 0) for path in
                      (directory / "missing.safetensors", corrupt, wrong_shape, offline)]
            for path, backend, exit_layers, lookahead in cases:
                with self.subTest(path=path.name, backend=backend, exit_layers=exit_layers, lookahead=lookahead):
                    command = [sys.executable, str(Path(__file__).resolve()), "--probe-create", str(path),
                               backend, str(exit_layers), str(lookahead)]
                    result = subprocess.run(command, capture_output=True, text=True, timeout=45, check=False)
                    self.assertEqual(result.returncode, 0, f"create aborted: {result.stderr[-2000:]}")
                    status = json.loads(result.stdout.strip())
                    self.assertFalse(status["created"], "invalid configuration/checkpoint accepted")
                    self.assertTrue(status["error"], "failed creation must expose a diagnostic")

    def test_null_create_arguments_reset_and_free_are_safe(self):
        for path, backend in ((None, self.backend), (b"", self.backend), (os.fsencode(self.model), None)):
            handle = self.api.reuse_rt_create(path, backend, 2, 3, 0)
            if handle:
                self.api.reuse_rt_free(handle)
            self.assertFalse(handle, "null or empty create argument accepted")
            self.assertTrue(self.api.error())
        self.assertNotEqual(self.api.reuse_rt_reset(None), 0)
        self.api.reuse_rt_free(None)


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--probe-create":
        print(json.dumps(probe_create(sys.argv[2], sys.argv[3], int(sys.argv[4]), int(sys.argv[5]))))
    else:
        unittest.main()
