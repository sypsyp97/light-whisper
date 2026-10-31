"""NVIDIA RE-USE enhancement using the existing GGML CPU/CUDA kernels.

RealtimeReuseEnhancer streams the original Real-time RE-USE FP32 checkpoint;
its separate NVIDIA license limits use to noncommercial research and education.
ReuseEnhancer retains the offline 30-layer model and its one-second windows.
Neither path requires PyTorch, model conversion or quantized weights.
"""

import ctypes as ct
import hashlib
import os
from pathlib import Path
import tempfile

import numpy as np

MODEL_REPO = "nvidia/NVIDIA-NemotronLabs-RE-USE"
MODEL_REVISION = "31f82f2efad1095034492f236e8dac9239ba713b"
MODEL_SHA256 = "06badaf4029b297f97f7fe48d62990b48d5482608210df577354e0d4e0ed70e1"
MODEL_BYTES = 38_592_940

REALTIME_MODEL_REPO = "nvidia/Real-time_RE-USE"
REALTIME_MODEL_REVISION = "4c0c186272ca853cd18d832fc5c28777b62f32d1"
REALTIME_MODEL_SHA256 = "6eac4e8cc721f99a22cde7eac3c9f8e2826ced5dc8a4234fd1977de1ec0d3ade"
REALTIME_MODEL_BYTES = 54_055_764


def ensure_realtime_reuse_model():
    return _ensure_reuse_model(REALTIME_MODEL_REPO, REALTIME_MODEL_REVISION,
                               REALTIME_MODEL_BYTES, REALTIME_MODEL_SHA256)


def _verified_model(path, size=None, sha256=None):
    size = MODEL_BYTES if size is None else size
    sha256 = MODEL_SHA256 if sha256 is None else sha256
    if not path.is_file() or path.stat().st_size != size:
        return False
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest() == sha256


def ensure_reuse_model():
    return _ensure_reuse_model(MODEL_REPO, MODEL_REVISION, MODEL_BYTES, MODEL_SHA256)


def _ensure_reuse_model(repo, revision, size_bytes, sha256):
    from hf_cache_utils import get_hf_cache_root

    path = (Path(get_hf_cache_root()) / ("models--" + repo.replace("/", "--"))
            / "snapshots" / revision / "model.safetensors")
    if _verified_model(path, size_bytes, sha256):
        return path
    # Use the project's download transport directly: resident ASR otherwise sets
    # Hugging Face offline mode. Only this explicit opt-in may fetch this model.
    import requests

    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(dir=path.parent, suffix=".download", delete=False) as target:
            temporary = Path(target.name)
            size, digest = 0, hashlib.sha256()
            url = f"https://huggingface.co/{repo}/resolve/{revision}/model.safetensors"
            with requests.get(url, stream=True, timeout=(10, 30)) as response:
                response.raise_for_status()
                for block in response.iter_content(1024 * 1024):
                    size += len(block)
                    if size > size_bytes:
                        raise RuntimeError("RE-USE 下载大小与固定版本不符")
                    digest.update(block)
                    target.write(block)
            if size != size_bytes or digest.hexdigest() != sha256:
                raise RuntimeError("RE-USE 模型校验失败，请重试")
        os.replace(temporary, path)
        return path
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


class NativeReuseRuntime:
    def __init__(self, model_path=None, library_path=None, backend=None, threads=2):
        model = Path(model_path) if model_path else ensure_reuse_model()
        if not _verified_model(model):
            raise RuntimeError("RE-USE 原始模型版本或校验不符")
        self._load(model, library_path, backend, threads)

    def _load(self, model, library_path, backend, threads, prefix="reuse", options=()):
        self.handle = None
        self.lib = None
        self._dll_paths = []
        self._prefix = prefix
        root = Path(__file__).resolve().with_name("r2t2-native")
        candidates = [backend] if backend else ["cuda", "cpu"]
        errors = []
        for selected in candidates:
            library = Path(library_path) if library_path else root / selected / "reuse.dll"
            if not library.is_file():
                errors.append(f"{selected}: 缺少 {library.name}，请重新构建原生引擎")
                continue
            try:
                if os.name == "nt":
                    for directory in dict.fromkeys((library.resolve().parent, root.parent)):
                        self._dll_paths.append(os.add_dll_directory(str(directory)))
                self.lib = ct.CDLL(str(library.resolve()))
                pointer = ct.POINTER(ct.c_float)
                signatures = {
                    "create": (ct.c_void_p, [ct.c_char_p, ct.c_char_p, ct.c_int] + [ct.c_int] * len(options)),
                    "run": (ct.c_int, [ct.c_void_p, pointer, pointer, ct.c_int, pointer, pointer, pointer]),
                    "free": (None, [ct.c_void_p]),
                    "last_error": (ct.c_char_p, []),
                }
                if options:
                    signatures["reset"] = (ct.c_int, [ct.c_void_p])
                for name, (result, arguments) in signatures.items():
                    function = getattr(self.lib, f"{prefix}_{name}")
                    function.restype, function.argtypes = result, arguments
                self.handle = getattr(self.lib, f"{prefix}_create")(
                    str(model.resolve()).encode("utf-8"), selected.encode(), threads, *options)
                if not self.handle:
                    raise RuntimeError(self._error())
                self.backend = selected
                return
            except Exception as exc:
                errors.append(f"{selected}: {exc}")
                self.close()
        raise RuntimeError("RE-USE 加载失败：" + "；".join(errors))

    def _error(self):
        return (getattr(self.lib, f"{self._prefix}_last_error")() or b"unknown error").decode("utf-8", errors="replace")

    def run(self, magnitude, phase):
        if not self.handle:
            raise RuntimeError("RE-USE runtime is closed")
        magnitude, phase = (np.ascontiguousarray(value, dtype=np.float32) for value in (magnitude, phase))
        if (magnitude.ndim != 2 or magnitude.shape[1] != 321 or phase.shape != magnitude.shape
                or not 9 <= len(magnitude) <= 201
                or not np.isfinite(magnitude).all() or not np.isfinite(phase).all()):
            raise ValueError("RE-USE requires finite [frames,321] spectra, 9..201 frames")
        outputs = [np.empty_like(magnitude) for _ in range(3)]
        pointer = lambda value: value.ctypes.data_as(ct.POINTER(ct.c_float))
        status = self.lib.reuse_run(self.handle, pointer(magnitude), pointer(phase), len(magnitude),
                                   *[pointer(value) for value in outputs])
        if status or not all(np.isfinite(value).all() for value in outputs):
            raise RuntimeError(f"RE-USE inference failed: {self._error()}")
        return outputs

    def close(self):
        handle, self.handle = self.handle, None
        if handle:
            getattr(self.lib, f"{self._prefix}_free")(handle)
        for directory in self._dll_paths:
            directory.close()
        self._dll_paths.clear()
        self.lib = None


class ReuseEnhancer:
    WINDOW = 16_000
    OVERLAP = 2_000
    HOP = WINDOW - OVERLAP
    FFT = 640
    STFT_HOP = 80

    def __init__(self, **runtime_options):
        self.runtime = NativeReuseRuntime(**runtime_options)
        self._closed = False
        self._window = np.hanning(self.FFT + 1)[:-1].astype(np.float32)
        self._blend = np.linspace(0, 1, self.OVERLAP, dtype=np.float32)
        self.reset()

    def _validate(self, audio):
        if self._closed:
            raise RuntimeError("RE-USE enhancer is closed")
        if (not isinstance(audio, np.ndarray) or audio.dtype != np.float32
                or audio.ndim != 1 or not np.isfinite(audio).all()):
            raise ValueError("RE-USE requires finite float32 mono PCM")

    def _enhance_window(self, audio):
        count = len(audio)
        padded_count = max(self.FFT, ((count + self.STFT_HOP - 1) // self.STFT_HOP) * self.STFT_HOP)
        padded = np.pad(audio, (0, padded_count - count), constant_values=1e-8)
        padded = np.pad(padded, (self.FFT // 2, self.FFT // 2), mode="reflect")
        frames = np.lib.stride_tricks.sliding_window_view(padded, self.FFT)[::self.STFT_HOP]
        spectrum = np.fft.rfft(frames * self._window).astype(np.complex64)
        magnitude = np.log1p(np.abs(spectrum)).astype(np.float32)
        phase = np.angle(spectrum).astype(np.float32)
        magnitude, real, imag = self.runtime.run(magnitude, phase)
        magnitude = np.expm1(np.maximum(magnitude, 0))
        magnitude[np.mean(magnitude == 0, axis=1) > 0.5] = 0
        spectrum = magnitude * np.exp(1j * np.arctan2(imag, real))
        restored = np.fft.irfft(spectrum, n=self.FFT) * self._window
        output = np.zeros(padded_count + self.FFT, dtype=np.float64)
        weights = np.zeros_like(output)
        for index, frame in enumerate(restored):
            start = index * self.STFT_HOP
            output[start:start + self.FFT] += frame
            weights[start:start + self.FFT] += self._window ** 2
        start = self.FFT // 2
        return (output[start:start + count] / weights[start:start + count]).astype(np.float32)

    def reset(self):
        self._pending = np.empty(0, dtype=np.float32)
        self._tail = None

    def _join(self, audio):
        if self._tail is not None:
            count = min(len(audio), len(self._tail))
            blend = self._blend[:count]
            audio[:count] = self._tail[:count] * (1 - blend) + audio[:count] * blend
        return audio

    def feed(self, audio):
        self._validate(audio)
        self._pending = np.concatenate((self._pending, audio))
        output = []
        while len(self._pending) >= self.WINDOW:
            enhanced = self._join(self._enhance_window(self._pending[:self.WINDOW]))
            output.append(enhanced[:self.HOP])
            self._tail = enhanced[self.HOP:].copy()
            self._pending = self._pending[self.HOP:].copy()
        return np.concatenate(output) if output else np.empty(0, dtype=np.float32)

    def flush(self):
        self._validate(self._pending)
        try:
            return self._join(self._enhance_window(self._pending)) if len(self._pending) else self._pending.copy()
        finally:
            self.reset()

    def enhance(self, audio):
        self._validate(audio)
        self.reset()
        try:
            return np.concatenate((self.feed(audio), self.flush()))
        finally:
            self.reset()

    def close(self):
        if not self._closed:
            self._closed = True
            self.reset()
            self.runtime.close()


class NativeRealtimeReuseRuntime(NativeReuseRuntime):
    def __init__(self, model_path=None, library_path=None, backend=None, threads=2,
                 exit_layers=8, lookahead=1):
        if type(exit_layers) is not int or not 3 <= exit_layers <= 12:
            raise ValueError("Real-time RE-USE exit layers must be between 3 and 12")
        if type(lookahead) is not int or not 0 <= lookahead <= 2:
            raise ValueError("Real-time RE-USE lookahead must be between 0 and 2")
        if backend not in (None, "cpu", "cuda"):
            raise ValueError("Unsupported real-time RE-USE backend")
        if type(threads) is not int:
            raise TypeError("Real-time RE-USE threads must be an integer")
        model = Path(model_path) if model_path else ensure_realtime_reuse_model()
        if not _verified_model(model, REALTIME_MODEL_BYTES, REALTIME_MODEL_SHA256):
            raise RuntimeError("Real-time RE-USE model version or checksum mismatch")
        self.exit_layers, self.lookahead = exit_layers, lookahead
        self._primed = False
        self._failure = None
        self._load(model, library_path, backend, threads, "reuse_rt", (exit_layers, lookahead))

    def run(self, magnitude, phase):
        if not self.handle:
            raise RuntimeError("Real-time RE-USE runtime is closed")
        if self._failure is not None:
            raise RuntimeError(self._failure)
        magnitude, phase = (np.ascontiguousarray(value, dtype=np.float32) for value in (magnitude, phase))
        frames = 1 if self._primed else self.lookahead + 1
        if (magnitude.shape != (frames, 321) or phase.shape != magnitude.shape
                or not np.isfinite(magnitude).all() or not np.isfinite(phase).all()):
            raise ValueError(f"Real-time RE-USE requires finite [{frames},321] spectra")
        outputs = [np.empty((1, 321), dtype=np.float32) for _ in range(3)]
        pointer = lambda value: value.ctypes.data_as(ct.POINTER(ct.c_float))
        status = self.lib.reuse_rt_run(self.handle, pointer(magnitude), pointer(phase), frames,
                                      *[pointer(value) for value in outputs])
        if status or not all(np.isfinite(value).all() for value in outputs):
            reason = "native error" if status else "non-finite output"
            # Native state may already have advanced. Only a successful reset can recover it.
            self._failure = f"Real-time RE-USE inference failed ({reason}): {self._error()}"
            raise RuntimeError(self._failure)
        self._primed = True
        return outputs

    def reset(self):
        if not self.handle:
            raise RuntimeError("Real-time RE-USE runtime is closed")
        if self.lib.reuse_rt_reset(self.handle):
            self._failure = f"Real-time RE-USE reset failed: {self._error()}"
            raise RuntimeError(self._failure)
        self._primed = False
        self._failure = None


class RealtimeReuseEnhancer:
    FFT = 640
    HOP = 320

    def __init__(self, **runtime_options):
        self.runtime = NativeRealtimeReuseRuntime(**runtime_options)
        self._lookahead = self.runtime.lookahead
        self._window = np.hanning(self.FFT + 1)[:-1].astype(np.float32)
        self._window_squared = self._window ** 2
        self._closed = False
        self._native_started = False
        self.reset()

    def _validate(self, audio):
        if self._closed:
            raise RuntimeError("Real-time RE-USE enhancer is closed")
        if (not isinstance(audio, np.ndarray) or audio.dtype != np.float32
                or audio.ndim != 1 or not np.isfinite(audio).all()):
            raise ValueError("Real-time RE-USE requires finite float32 mono PCM")

    def _push_spectrum(self, magnitude, phase, output):
        if not self._native_started:
            self._magnitudes.append(magnitude)
            self._phases.append(phase)
            if len(self._magnitudes) < self._lookahead + 1:
                return
            magnitude, phase = np.stack(self._magnitudes), np.stack(self._phases)
            self._magnitudes.clear()
            self._phases.clear()
            self._native_started = True
        else:
            magnitude, phase = magnitude[None, :], phase[None, :]
        magnitude, real, imag = self.runtime.run(magnitude, phase)
        magnitude = np.expm1(np.maximum(magnitude[0], 0))
        if np.count_nonzero(magnitude == 0) > len(magnitude) / 2:
            magnitude.fill(0)
        spectrum = magnitude * np.exp(1j * np.arctan2(imag[0], real[0]))
        self._overlap += np.fft.irfft(spectrum, n=self.FFT) * self._window
        self._weights += self._window_squared
        # Frame zero's completed hop lies before the centered waveform origin.
        if self._output_frames:
            count = min(self.HOP, self._samples - self._emitted)
            if count:
                output.append((self._overlap[:count] / self._weights[:count]).astype(np.float32))
                self._emitted += count
        self._output_frames += 1
        for values in (self._overlap, self._weights):
            values[:self.HOP] = values[self.HOP:]
            values[self.HOP:] = 0

    def _push_frame(self, frame, output):
        spectrum = np.fft.rfft(frame * self._window).astype(np.complex64)
        self._push_spectrum(np.log1p(np.abs(spectrum)).astype(np.float32),
                            np.angle(spectrum).astype(np.float32), output)

    def _append_pcm(self, audio, output):
        offset = 0
        while offset < len(audio):
            count = min(self.FFT - len(self._pending), len(audio) - offset)
            self._pending = np.concatenate((self._pending, audio[offset:offset + count]))
            offset += count
            if len(self._pending) < self.FFT:
                continue
            if not self._left_padded:
                self._pending = np.pad(self._pending, (self.HOP, 0), mode="reflect")
                self._left_padded = True
            while len(self._pending) >= self.FFT:
                self._push_frame(self._pending[:self.FFT], output)
                self._previous = self._pending[self.HOP - 1]
                self._pending = self._pending[self.HOP:].copy()

    def feed(self, audio):
        self._validate(audio)
        self._samples += len(audio)
        output = []
        self._append_pcm(audio, output)
        return np.concatenate(output) if output else np.empty(0, dtype=np.float32)

    def flush(self):
        self._validate(self._pending)
        if not self._samples:
            return np.empty(0, dtype=np.float32)
        output = []
        try:
            padded_count = max(self.FFT, ((self._samples + self.HOP - 1) // self.HOP) * self.HOP)
            self._append_pcm(np.full(padded_count - self._samples, 1e-8, dtype=np.float32), output)
            # Keep one extra past sample for the right reflection's outer edge.
            edge = np.concatenate((np.array([self._previous], dtype=np.float32), self._pending))
            self._push_frame(np.concatenate((self._pending, edge[-2::-1])), output)
            zeros = np.zeros(self.FFT // 2 + 1, dtype=np.float32)
            for _ in range(self._lookahead):
                self._push_spectrum(zeros, zeros, output)
            return np.concatenate(output) if output else np.empty(0, dtype=np.float32)
        finally:
            self.reset()

    def enhance(self, audio):
        self._validate(audio)
        self.reset()
        try:
            return np.concatenate((self.feed(audio), self.flush()))
        finally:
            self.reset()

    def reset(self):
        if self._native_started and not self._closed:
            self.runtime.reset()
        self._pending = np.empty(0, dtype=np.float32)
        self._left_padded = False
        self._previous = np.float32(0)
        self._native_started = False
        self._magnitudes = []
        self._phases = []
        self._samples = self._emitted = self._output_frames = 0
        self._overlap = np.zeros(self.FFT, dtype=np.float64)
        self._weights = np.zeros(self.FFT, dtype=np.float64)

    def close(self):
        if not self._closed:
            self._closed = True
            self.reset()
            self.runtime.close()
