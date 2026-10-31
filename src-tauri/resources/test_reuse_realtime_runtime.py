"""Real-time RE-USE public loader/FFI contract; no real weights, DLL or network."""

import ctypes as ct
import hashlib
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

import numpy as np

sys.path.insert(0, os.path.dirname(__file__))

import hf_cache_utils
import reuse_enhancement as enhancement


REPO = "nvidia/Real-time_RE-USE"
REVISION = "4c0c186272ca853cd18d832fc5c28777b62f32d1"
SHA256 = "6eac4e8cc721f99a22cde7eac3c9f8e2826ced5dc8a4234fd1977de1ec0d3ade"
MODEL_BYTES = 54_055_764
URL = f"https://huggingface.co/{REPO}/resolve/{REVISION}/model.safetensors"
# Integrity-only fixture: the fake DLL never parses a safetensors checkpoint.
PAYLOAD = b"real-time RE-USE integrity-only unit-test checkpoint\n"
BINS = 321
FLOAT_POINTER = ct.POINTER(ct.c_float)


class _PublicApiTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        required = ("NativeRealtimeReuseRuntime", "ensure_realtime_reuse_model",
                    "REALTIME_MODEL_REPO", "REALTIME_MODEL_REVISION",
                    "REALTIME_MODEL_SHA256", "REALTIME_MODEL_BYTES")
        missing = [name for name in required if not hasattr(enhancement, name)]
        if missing:
            raise RuntimeError("INTERFACE_PENDING: " + ", ".join(missing))


class _TinyModelTests(_PublicApiTests):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        for name, value in (("REALTIME_MODEL_BYTES", len(PAYLOAD)),
                            ("REALTIME_MODEL_SHA256", hashlib.sha256(PAYLOAD).hexdigest())):
            patcher = mock.patch.object(enhancement, name, value, create=True)
            patcher.start()
            self.addCleanup(patcher.stop)
        patcher = mock.patch.object(hf_cache_utils, "get_hf_cache_root", return_value=str(self.root / "hub"))
        patcher.start()
        self.addCleanup(patcher.stop)
        patcher = mock.patch("requests.get", side_effect=AssertionError("unexpected network request"))
        self.get = patcher.start()
        self.addCleanup(patcher.stop)
        self.cached = (self.root / "hub" / "models--nvidia--Real-time_RE-USE"
                       / "snapshots" / REVISION / "model.safetensors")

    def _response(self, *blocks, error=None, inspect=None):
        response = _Response(blocks, error=error, inspect=inspect)
        self.get.side_effect = None
        self.get.return_value = response
        return response

    def _ensure(self):
        try:
            return enhancement.ensure_realtime_reuse_model()
        except RuntimeError as error:
            self.fail(f"valid real-time model acquisition failed: {error}")


class _Response:
    def __init__(self, blocks, error=None, inspect=None):
        self.blocks, self.error, self.inspect = blocks, error, inspect
        self.closed = False
        self.status_error = None

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.closed = True

    def raise_for_status(self):
        if self.status_error:
            raise self.status_error

    def iter_content(self, chunk_size):
        if chunk_size <= 0:
            raise AssertionError("download must use a positive streaming chunk size")
        for block in self.blocks:
            if self.inspect:
                self.inspect()
            yield block
        if self.error:
            raise self.error


class RealtimeReuseMetadataTests(_PublicApiTests):
    def test_real_checkpoint_metadata_is_pinned_separately_from_tiny_fixtures(self):
        self.assertEqual(enhancement.REALTIME_MODEL_REPO, REPO)
        self.assertEqual(enhancement.REALTIME_MODEL_REVISION, REVISION)
        self.assertEqual(enhancement.REALTIME_MODEL_BYTES, MODEL_BYTES)
        self.assertEqual(enhancement.REALTIME_MODEL_SHA256, SHA256)


class RealtimeReuseModelTests(_TinyModelTests):
    def test_valid_pinned_cache_returns_path_without_fetch_or_replacement(self):
        self.cached.parent.mkdir(parents=True)
        self.cached.write_bytes(PAYLOAD)
        with mock.patch.object(enhancement.os, "replace", wraps=os.replace) as replace:
            result = self._ensure()
        self.assertIsInstance(result, Path)
        self.assertEqual(result, self.cached)
        self.assertEqual(result.read_bytes(), PAYLOAD)
        self.get.assert_not_called()
        replace.assert_not_called()

    def test_explicit_download_streams_pinned_url_atomically_despite_hf_offline_mode(self):
        self.cached.parent.mkdir(parents=True)
        self.cached.write_bytes(b"old invalid cached checkpoint")
        previous = self.cached.read_bytes()
        response = self._response(PAYLOAD[:11], b"", PAYLOAD[11:],
                                  inspect=lambda: self.assertEqual(self.cached.read_bytes(), previous))
        with mock.patch.dict(os.environ, {"HF_HUB_OFFLINE": "1", "TRANSFORMERS_OFFLINE": "1"}):
            before = dict(os.environ)
            with mock.patch.object(enhancement.os, "replace", wraps=os.replace) as replace:
                self.assertEqual(self._ensure(), self.cached)
            self.assertEqual(dict(os.environ), before)
        self.get.assert_called_once()
        self.assertEqual(self.get.call_args.args[0], URL)
        self.assertIs(self.get.call_args.kwargs.get("stream"), True)
        self.assertTrue(response.closed)
        replace.assert_called_once()
        self.assertEqual(Path(replace.call_args.args[1]), self.cached)
        self.assertNotEqual(Path(replace.call_args.args[0]), self.cached)
        self.assertEqual(self.cached.read_bytes(), PAYLOAD)
        self.assertEqual(list(self.cached.parent.iterdir()), [self.cached])

    def test_bad_size_hash_and_interrupted_download_remove_partial_files(self):
        cases = (("short", (PAYLOAD[:-1],), None),
                 ("oversized", (PAYLOAD, b"extra"), None),
                 ("same-size wrong hash", (b"!" + PAYLOAD[1:],), None),
                 ("interrupted", (PAYLOAD[:10],), OSError("test stream interrupted")))
        for name, blocks, error in cases:
            with self.subTest(case=name):
                self.get.reset_mock()
                response = self._response(*blocks, error=error)
                with self.assertRaises((RuntimeError, OSError)):
                    enhancement.ensure_realtime_reuse_model()
                self.get.assert_called_once()
                self.assertTrue(response.closed)
                self.assertFalse(self.cached.exists())
                self.assertEqual(list(self.cached.parent.iterdir()), [])

    def test_http_failure_is_checked_and_does_not_publish_a_checkpoint(self):
        response = self._response(PAYLOAD)
        response.status_error = OSError("test HTTP status failure")
        with self.assertRaisesRegex((RuntimeError, OSError), "test HTTP status failure"):
            enhancement.ensure_realtime_reuse_model()
        self.get.assert_called_once()
        self.assertTrue(response.closed)
        self.assertFalse(self.cached.exists())
        self.assertEqual(list(self.cached.parent.iterdir()), [])


class _Function:
    """Python export with the public ctypes binding attributes."""
    def __init__(self, function):
        self.function = function
        self.calls = []
        self.argtypes = self.restype = None

    def __call__(self, *args):
        self.calls.append(args)
        return self.function(*args)


class _Library:
    """Only the real-time ABI exists; offline-symbol lookups cannot pass."""
    def __init__(self):
        self.handle = 0xC0DE
        self.create_ok = True
        self.run_status = self.reset_status = 0
        self.nonfinite_output = False
        self.error = b"test native diagnostic"
        self.inputs = []
        self.ticks = 0
        self.live = self.primed = False
        self.lookahead = None
        self.reuse_rt_create = _Function(self._create)
        self.reuse_rt_run = _Function(self._run)
        self.reuse_rt_reset = _Function(self._reset)
        self.reuse_rt_free = _Function(self._free)
        self.reuse_rt_last_error = _Function(lambda: self.error)

    def _create(self, model, backend, threads, exit_layers, lookahead):
        if not self.create_ok:
            return None
        if self.live:
            raise AssertionError("fake DLL handle was created twice without being freed")
        self.live, self.primed = True, False
        self.lookahead, self.ticks = lookahead, 0
        return self.handle

    def _require_live_handle(self, handle):
        if not self.live or getattr(handle, "value", handle) != self.handle:
            raise AssertionError("fake DLL received an invalid or freed handle")

    def _run(self, handle, magnitude, phase, frames, out_mag, out_real, out_imag):
        self._require_live_handle(handle)
        count = int(getattr(frames, "value", frames))
        expected = 1 if self.primed else self.lookahead + 1
        if count != expected:
            raise AssertionError(f"fake native state requires {expected} frames, received {count}")
        mag = np.ctypeslib.as_array(magnitude, shape=(count * BINS,)).reshape(count, BINS)
        angle = np.ctypeslib.as_array(phase, shape=(count * BINS,)).reshape(count, BINS)
        self.inputs.append((mag.copy(), angle.copy()))
        tick = self.ticks
        # Native state can change before compute reports failure. A Python-only
        # priming rollback cannot recover this stream; reuse_rt_reset must run.
        self.primed = True
        self.ticks += 1
        if self.run_status:
            return self.run_status
        values = (mag[0] + np.float32(tick + 0.25), np.cos(angle[0]), np.sin(angle[0]))
        for pointer, value in zip((out_mag, out_real, out_imag), values):
            np.ctypeslib.as_array(pointer, shape=(BINS,))[:] = value
        if self.nonfinite_output:
            out_imag[17] = float("nan")
        return 0

    def _reset(self, handle):
        self._require_live_handle(handle)
        if self.reset_status:
            self.primed = True
            self.ticks += 1
        else:
            self.primed, self.ticks = False, 0
            self.error = b""
        return self.reset_status

    def _free(self, handle):
        self._require_live_handle(handle)
        self.live = False
        # A wrapper must capture a create error before resource cleanup changes it.
        self.error = b"diagnostic replaced by free"


class RealtimeReuseFakeProtocolTests(unittest.TestCase):
    def setUp(self):
        self.library = _Library()
        self.handle = self.library.reuse_rt_create(b"fixture", b"cpu", 2, 8, 2)
        self.magnitude = np.arange(3 * BINS, dtype=np.float32).reshape(3, BINS) / 1000
        self.phase = self.magnitude - np.float32(0.2)
        self.outputs = [np.empty((1, BINS), np.float32) for _ in range(3)]

    def _run(self, handle, frames=3):
        pointer = lambda value: value.ctypes.data_as(FLOAT_POINTER)
        return self.library.reuse_rt_run(handle, pointer(self.magnitude), pointer(self.phase), frames,
                                        *[pointer(output) for output in self.outputs])

    def test_fake_checks_created_live_handle_for_run_reset_and_free(self):
        uncreated = _Library()
        for operation in (lambda: uncreated.reuse_rt_run(uncreated.handle, None, None, 3, None, None, None),
                          lambda: uncreated.reuse_rt_reset(uncreated.handle),
                          lambda: uncreated.reuse_rt_free(uncreated.handle)):
            with self.assertRaisesRegex(AssertionError, "invalid or freed handle"):
                operation()
        for handle in (None, self.handle + 1):
            for operation in (self._run, self.library.reuse_rt_reset, self.library.reuse_rt_free):
                with self.subTest(handle=handle, operation=operation):
                    with self.assertRaisesRegex(AssertionError, "invalid or freed handle"):
                        operation(handle)
        self.assertEqual(self._run(self.handle), 0)
        np.testing.assert_array_equal(self.outputs[0], self.magnitude[:1] + np.float32(0.25))
        np.testing.assert_array_equal(self.outputs[1], np.cos(self.phase[:1]))
        np.testing.assert_array_equal(self.outputs[2], np.sin(self.phase[:1]))
        self.assertEqual(self.library.reuse_rt_reset(self.handle), 0)
        self.library.reuse_rt_free(self.handle)
        for operation in (self._run, self.library.reuse_rt_reset, self.library.reuse_rt_free):
            with self.subTest(after_free=operation):
                with self.assertRaisesRegex(AssertionError, "invalid or freed handle"):
                    operation(self.handle)

    def test_fake_mutates_native_state_on_error_and_only_native_reset_reprimes(self):
        self.library.run_status = 1
        self.assertEqual(self._run(self.handle), 1)
        self.assertTrue(self.library.primed)
        self.assertEqual(self.library.ticks, 1)
        self.library.run_status = 0
        with self.assertRaisesRegex(AssertionError, "requires 1 frames, received 3"):
            self._run(self.handle)
        self.assertEqual(self._run(self.handle, frames=1), 0)
        self.library.reset_status = 1
        self.assertEqual(self.library.reuse_rt_reset(self.handle), 1)
        with self.assertRaisesRegex(AssertionError, "requires 1 frames, received 3"):
            self._run(self.handle)
        self.library.reset_status = 0
        self.assertEqual(self.library.reuse_rt_reset(self.handle), 0)
        self.assertEqual(self._run(self.handle), 0)
        np.testing.assert_array_equal(self.outputs[0], self.magnitude[:1] + np.float32(0.25))
        self.library.nonfinite_output = True
        self.assertEqual(self._run(self.handle, frames=1), 0)
        self.assertTrue(np.isnan(self.outputs[2][0, 17]))
        self.assertTrue(self.library.primed)
        self.library.reuse_rt_free(self.handle)


class _Directory:
    def __init__(self):
        self.close_calls = 0

    def close(self):
        self.close_calls += 1


class RealtimeReuseRuntimeTests(_TinyModelTests):
    def setUp(self):
        super().setUp()
        self.model = self.root / "模型音频.safetensors"
        self.model.write_bytes(PAYLOAD)
        self.library_path = self.root / "fake-native" / "reuse.dll"
        self.library_path.parent.mkdir()
        self.library_path.write_bytes(b"fake CDLL fixture; never loaded by the OS")
        self.library = _Library()
        self.directories = []

        def directory(path):
            item = _Directory()
            self.directories.append(item)
            return item

        for target, name, kwargs in ((enhancement.ct, "CDLL", {"return_value": self.library}),
                                     (enhancement.os, "add_dll_directory", {"side_effect": directory, "create": True}),
                                     (enhancement, "ensure_reuse_model", {"side_effect": AssertionError("offline model selected")})):
            patcher = mock.patch.object(target, name, **kwargs)
            patched = patcher.start()
            self.addCleanup(patcher.stop)
            if name == "CDLL":
                self.cdll = patched

    def _construct(self, **options):
        return enhancement.NativeRealtimeReuseRuntime(
            **{"model_path": self.model, "library_path": self.library_path, "backend": "cpu", **options})

    def _make(self, **options):
        try:
            runtime = self._construct(**options)
        except RuntimeError as error:
            self.fail(f"valid real-time runtime construction failed: {error}")
        self.addCleanup(runtime.close)
        return runtime

    def _spectra(self, frames):
        mag = np.arange(frames * BINS, dtype=np.float32).reshape(frames, BINS) / np.float32(1000)
        return mag, mag * np.float32(0.7) - np.float32(0.2)

    def _assert_directories_closed(self):
        if os.name == "nt":
            self.assertTrue(self.directories, "Windows dependency directories were never registered")
        self.assertTrue(all(item.close_calls == 1 for item in self.directories))

    def _assert_runs_blocked(self, runtime, diagnostic=None):
        calls = len(self.library.reuse_rt_run.calls)
        messages = []
        for inputs in (self._spectra(3), self._spectra(1), (None, None)):
            with self.assertRaises(RuntimeError) as failure:
                runtime.run(*inputs)
            if diagnostic is not None:
                self.assertIn(diagnostic, str(failure.exception))
            messages.append(str(failure.exception))
            self.assertEqual(len(self.library.reuse_rt_run.calls), calls,
                             "failed streams must reject all runs before FFI until explicit reset")
        return messages

    def _assert_recovered(self, runtime):
        reset_calls = len(self.library.reuse_rt_reset.calls)
        runtime.reset()
        self.assertEqual(self.library.reuse_rt_reset.calls[-1], (self.library.handle,))
        self.assertEqual(len(self.library.reuse_rt_reset.calls), reset_calls + 1)
        with self.assertRaises(ValueError):
            runtime.run(*self._spectra(1))
        magnitude, phase = self._spectra(3)
        outputs = runtime.run(magnitude, phase)
        expected = (magnitude[:1] + np.float32(0.25), np.cos(phase[:1]), np.sin(phase[:1]))
        for output, value in zip(outputs, expected):
            self.assertTrue(np.isfinite(output).all())
            np.testing.assert_allclose(output, value, rtol=0, atol=1e-7)

    def test_defaults_unicode_model_and_realtime_only_abi_are_bound(self):
        runtime = self._make()
        self.assertEqual((runtime.backend, runtime.exit_layers, runtime.lookahead), ("cpu", 8, 1))
        self.assertEqual(runtime.handle, self.library.handle)
        self.assertEqual(self.library.reuse_rt_create.calls,
                         [(str(self.model.resolve()).encode("utf-8"), b"cpu", 2, 8, 1)])
        signatures = {
            "reuse_rt_create": (ct.c_void_p, [ct.c_char_p, ct.c_char_p, ct.c_int, ct.c_int, ct.c_int]),
            "reuse_rt_run": (ct.c_int, [ct.c_void_p, FLOAT_POINTER, FLOAT_POINTER, ct.c_int,
                                       FLOAT_POINTER, FLOAT_POINTER, FLOAT_POINTER]),
            "reuse_rt_reset": (ct.c_int, [ct.c_void_p]),
            "reuse_rt_free": (None, [ct.c_void_p]),
            "reuse_rt_last_error": (ct.c_char_p, []),
        }
        for name, (result, arguments) in signatures.items():
            function = getattr(self.library, name)
            self.assertEqual(function.restype, result, name)
            self.assertEqual(function.argtypes, arguments, name)
        self.get.assert_not_called()

    def test_exit_layer_and_lookahead_limits_are_passed_to_create(self):
        for exit_layers, lookahead in ((3, 0), (12, 2)):
            with self.subTest(exit_layers=exit_layers, lookahead=lookahead):
                runtime = self._make(exit_layers=exit_layers, lookahead=lookahead, threads=1)
                self.assertEqual((runtime.exit_layers, runtime.lookahead), (exit_layers, lookahead))
                self.assertEqual(self.library.reuse_rt_create.calls[-1][2:], (1, exit_layers, lookahead))
                runtime.close()

    def test_invalid_configuration_is_rejected_before_download_or_library_load(self):
        for options in ({"exit_layers": 2}, {"exit_layers": 13},
                        {"lookahead": -1}, {"lookahead": 3}, {"backend": "unsupported"}):
            with self.subTest(options=options):
                with self.assertRaises((TypeError, ValueError)):
                    self._construct(model_path=None, **options)
        self.get.assert_not_called()
        self.cdll.assert_not_called()
        self.assertEqual(self.directories, [])

    def test_explicit_model_wrong_size_or_digest_is_rejected_before_library_load(self):
        for payload in (PAYLOAD[:-1], b"!" + PAYLOAD[1:]):
            with self.subTest(payload=payload):
                self.model.write_bytes(payload)
                with self.assertRaises((RuntimeError, ValueError)):
                    self._construct()
        self.get.assert_not_called()
        self.cdll.assert_not_called()
        self.assertEqual(self.directories, [])

    def test_default_model_acquisition_uses_realtime_pinned_download(self):
        self._response(PAYLOAD)
        runtime = self._make(model_path=None)
        self.assertEqual(self.library.reuse_rt_create.calls[-1][0], str(self.cached.resolve()).encode("utf-8"))
        self.assertEqual(self.cached.read_bytes(), PAYLOAD)
        self.assertEqual(runtime.backend, "cpu")
        self.get.assert_called_once()
        self.assertEqual(self.get.call_args.args[0], URL)

    def test_default_backend_uses_cuda_when_available(self):
        runtime = self._make(backend=None)
        self.assertEqual(runtime.backend, "cuda")
        self.cdll.assert_called_once()
        self.assertEqual(self.library.reuse_rt_create.calls[0][1], b"cuda")

    def test_default_backend_falls_back_after_cuda_load_or_create_failure(self):
        for failure in ("load", "create"):
            with self.subTest(failure=failure):
                self.directories.clear()
                cuda, cpu = _Library(), _Library()
                cuda.create_ok = False
                attempts = []

                def load(path):
                    attempts.append(path)
                    if len(attempts) == 1:
                        if failure == "load":
                            raise OSError("test CUDA load failure")
                        return cuda
                    # Handles from the first DLL attempt must be gone before
                    # loading the second library. Newly opened CPU handles may
                    # already exist; the first attempt's handles form a prefix.
                    self.assertTrue(all(item.close_calls == 1 for item in failed_directories))
                    return cpu

                failed_directories = []
                original_directory = enhancement.os.add_dll_directory

                def directory(path):
                    item = original_directory(path)
                    if not attempts:
                        failed_directories.append(item)
                    return item

                self.cdll.side_effect = load
                with mock.patch.object(enhancement.os, "add_dll_directory", side_effect=directory):
                    runtime = self._make(backend=None)
                self.assertEqual(runtime.backend, "cpu")
                self.assertEqual(len(attempts), 2)
                self.assertEqual(cpu.reuse_rt_create.calls[0][1], b"cpu")
                if failure == "create":
                    self.assertEqual(cuda.reuse_rt_create.calls[0][1], b"cuda")
                self.assertEqual(cuda.reuse_rt_free.calls, [])
                runtime.close()
                self.assertEqual(cpu.reuse_rt_free.calls, [(cpu.handle,)])
                self._assert_directories_closed()

    def test_explicit_backends_never_fall_back_on_create_failure(self):
        self.library.create_ok = False
        for backend in ("cpu", "cuda"):
            with self.subTest(backend=backend):
                self.cdll.reset_mock()
                self.directories.clear()
                with self.assertRaisesRegex(RuntimeError, "test native diagnostic"):
                    self._construct(backend=backend)
                self.cdll.assert_called_once()
                self.assertEqual(self.library.reuse_rt_create.calls[-1][1], backend.encode())
                self.assertEqual(self.library.reuse_rt_free.calls, [])
                self._assert_directories_closed()

    def test_missing_realtime_export_and_load_failure_close_dependency_directories(self):
        for failure in ("missing export", "load"):
            with self.subTest(failure=failure):
                self.directories.clear()
                library = _Library()
                del library.reuse_rt_reset
                self.cdll.side_effect = OSError("test load failure") if failure == "load" else None
                self.cdll.return_value = library
                with self.assertRaises(RuntimeError):
                    self._construct()
                self.assertEqual(library.reuse_rt_create.calls, [])
                self.assertEqual(library.reuse_rt_free.calls, [])
                self._assert_directories_closed()

    def test_run_converts_strided_and_list_inputs_to_finite_contiguous_float32(self):
        runtime = self._make()
        storage = np.arange(2 * BINS * 2, dtype=np.float64).reshape(2, BINS * 2) / 1000
        magnitude = storage[:, ::2]
        phase = (magnitude - 0.3).tolist()
        before, original_phase = storage.copy(), [row[:] for row in phase]
        outputs = runtime.run(magnitude, phase)
        self.assertEqual(len(outputs), 3)
        actual_mag, actual_phase = self.library.inputs[0]
        np.testing.assert_array_equal(actual_mag, magnitude.astype(np.float32))
        np.testing.assert_array_equal(actual_phase, np.asarray(phase, np.float32))
        expected = (actual_mag[:1] + np.float32(0.25), np.cos(actual_phase[:1]), np.sin(actual_phase[:1]))
        for index, (output, value) in enumerate(zip(outputs, expected)):
            self.assertEqual(output.shape, (1, BINS))
            self.assertEqual(output.dtype, np.float32)
            self.assertTrue(output.flags.c_contiguous)
            self.assertTrue(output.flags.owndata)
            self.assertTrue(np.isfinite(output).all())
            self.assertFalse(np.shares_memory(output, magnitude))
            for other in outputs[:index]:
                self.assertFalse(np.shares_memory(output, other))
            np.testing.assert_allclose(output, value, rtol=0, atol=1e-7)
        snapshots = [output.copy() for output in outputs]
        runtime.run(*self._spectra(1))
        runtime.reset()
        runtime.close()
        for output, snapshot in zip(outputs, snapshots):
            np.testing.assert_array_equal(output, snapshot)
        np.testing.assert_array_equal(storage, before)
        self.assertEqual(phase, original_phase)

    def test_first_run_requires_exact_lookahead_plus_one_then_exactly_one_frame(self):
        for lookahead in (0, 1, 2):
            with self.subTest(lookahead=lookahead):
                runtime = self._make(lookahead=lookahead)
                for frames in (0, lookahead + 2):
                    with self.assertRaises(ValueError):
                        runtime.run(*self._spectra(frames))
                runtime.run(*self._spectra(lookahead + 1))
                for frames in (0, 2, 3):
                    with self.assertRaises(ValueError):
                        runtime.run(*self._spectra(frames))
                runtime.run(*self._spectra(1))
                self.assertEqual([len(mag) for mag, _ in self.library.inputs[-2:]], [lookahead + 1, 1])
                runtime.close()

    def test_invalid_spectra_never_reach_ffi_or_advance_priming(self):
        runtime = self._make(lookahead=2)
        valid = self._spectra(3)
        invalid = ((np.ones((3, 320)), valid[1]), (np.ones((3, 321, 1)), valid[1]),
                   (valid[0], np.ones((2, 321))), (None, None),
                   (np.full((3, 321), np.nan), valid[1]), (valid[0], np.full((3, 321), np.inf)),
                   (np.full((3, 321), -np.inf), valid[1]),
                   (np.full((3, 321), 1e300), valid[1]),
                   (np.ones((1, 321)), np.ones((1, 321))),
                   (np.ones((2, 321)), np.ones((2, 321))))
        for mag, phase in invalid:
            with self.subTest(shape=getattr(mag, "shape", None)):
                with np.errstate(over="ignore", invalid="ignore"):
                    with self.assertRaises((ValueError, TypeError)):
                        runtime.run(mag, phase)
                self.assertEqual(self.library.reuse_rt_run.calls, [])
        runtime.run(*valid)
        calls = len(self.library.reuse_rt_run.calls)
        for inputs in ((np.full((1, BINS), np.nan), np.zeros((1, BINS))), valid):
            with self.assertRaises(ValueError):
                runtime.run(*inputs)
            self.assertEqual(len(self.library.reuse_rt_run.calls), calls)
        runtime.run(*self._spectra(1))
        self.assertEqual([len(mag) for mag, _ in self.library.inputs], [3, 1])

    def test_native_run_failure_blocks_initial_and_primed_streams_until_explicit_reset(self):
        for primed in (False, True):
            with self.subTest(primed=primed):
                self.library = _Library()
                self.cdll.return_value = self.library
                runtime = self._make(lookahead=2)
                if primed:
                    runtime.run(*self._spectra(3))
                self.library.run_status = 1
                self.library.error = b"captured native compute diagnostic"
                with self.assertRaisesRegex(RuntimeError, "captured native compute diagnostic") as failure:
                    runtime.run(*self._spectra(1 if primed else 3))
                original = str(failure.exception)
                self.assertTrue(self.library.primed)
                self.assertEqual(self.library.ticks, 2 if primed else 1)
                self.assertEqual(self.library.reuse_rt_run.calls[-1][0], runtime.handle)
                self.library.run_status = 0
                self.library.error = b"TLS diagnostic changed after failure"
                self._assert_runs_blocked(runtime, "captured native compute diagnostic")
                self.assertEqual(self.library.reuse_rt_reset.calls, [])
                self._assert_recovered(runtime)
                self.assertEqual(str(failure.exception), original)
                runtime.close()

    def test_nonfinite_output_blocks_initial_and_primed_streams_until_explicit_reset(self):
        for primed in (False, True):
            with self.subTest(primed=primed):
                self.library = _Library()
                self.cdll.return_value = self.library
                runtime = self._make(lookahead=2)
                if primed:
                    runtime.run(*self._spectra(3))
                self.library.nonfinite_output = True
                self.library.error = b""
                with self.assertRaises(RuntimeError) as failure:
                    runtime.run(*self._spectra(1 if primed else 3))
                original = str(failure.exception)
                self.assertTrue(original)
                self.assertTrue(self.library.primed)
                self.assertEqual(self.library.ticks, 2 if primed else 1)
                self.library.nonfinite_output = False
                messages = self._assert_runs_blocked(runtime)
                self.library.error = b"unrelated native TLS diagnostic"
                self.assertEqual(self._assert_runs_blocked(runtime), messages)
                self.assertEqual(self.library.reuse_rt_reset.calls, [])
                self._assert_recovered(runtime)
                self.assertEqual(str(failure.exception), original)
                runtime.close()

    def test_reset_calls_native_and_restores_initial_frame_class(self):
        runtime = self._make(lookahead=2)
        first = runtime.run(*self._spectra(3))
        runtime.run(*self._spectra(1))
        runtime.reset()
        self.assertEqual(self.library.reuse_rt_reset.calls, [(runtime.handle,)])
        with self.assertRaises(ValueError):
            runtime.run(*self._spectra(1))
        restarted = runtime.run(*self._spectra(3))
        for actual, expected in zip(restarted, first):
            np.testing.assert_array_equal(actual, expected)

    def test_failed_reset_blocks_initial_healthy_and_failed_streams_until_successful_reset(self):
        for state in ("initial", "healthy", "failed"):
            with self.subTest(state=state):
                self.library = _Library()
                self.cdll.return_value = self.library
                runtime = self._make(lookahead=2)
                run_failure = None
                if state == "healthy":
                    runtime.run(*self._spectra(3))
                elif state == "failed":
                    self.library.run_status = 1
                    self.library.error = b"earlier compute diagnostic"
                    with self.assertRaisesRegex(RuntimeError, "earlier compute diagnostic") as run_failure:
                        runtime.run(*self._spectra(3))
                    original_run = str(run_failure.exception)
                    self.library.run_status = 0
                self.library.reset_status = 1
                self.library.error = b"captured reset rejection diagnostic"
                with self.assertRaisesRegex(RuntimeError, "captured reset rejection diagnostic") as reset_failure:
                    runtime.reset()
                original_reset = str(reset_failure.exception)
                self.assertEqual(self.library.reuse_rt_reset.calls, [(runtime.handle,)])
                self.assertEqual(self.library.reuse_rt_free.calls, [])
                self.library.error = b"TLS changed after failed reset"
                self._assert_runs_blocked(runtime, "captured reset rejection diagnostic")
                self.library.reset_status = 0
                self._assert_recovered(runtime)
                self.assertEqual(str(reset_failure.exception), original_reset)
                if run_failure is not None:
                    self.assertEqual(str(run_failure.exception), original_run)
                runtime.close()

    def test_close_is_idempotent_frees_once_and_rejects_processing_before_ffi(self):
        for failed in (False, True):
            with self.subTest(failed=failed):
                self.library = _Library()
                self.cdll.return_value = self.library
                runtime = self._make()
                if failed:
                    self.library.run_status = 1
                    with self.assertRaisesRegex(RuntimeError, "test native diagnostic"):
                        runtime.run(*self._spectra(2))
                calls = len(self.library.reuse_rt_run.calls)
                runtime.close()
                runtime.close()
                self.assertIsNone(runtime.handle)
                self.assertFalse(self.library.live)
                self.assertEqual(self.library.reuse_rt_free.calls, [(self.library.handle,)])
                self._assert_directories_closed()
                with self.assertRaises(RuntimeError):
                    runtime.run(*self._spectra(2))
                with self.assertRaises(RuntimeError):
                    runtime.run(None, None)
                with self.assertRaises(RuntimeError):
                    runtime.reset()
                self.assertEqual(len(self.library.reuse_rt_run.calls), calls)
                self.assertEqual(self.library.reuse_rt_reset.calls, [])


if __name__ == "__main__":
    unittest.main()
