"""Resident Qwen audio.cpp inference and sample-driven caption previews."""

import ctypes as ct
from types import SimpleNamespace

import numpy as np

from r2t2_native import NativeRuntime


def _validate_pcm(pcm):
    if (not isinstance(pcm, np.ndarray) or pcm.dtype != np.float32
            or pcm.ndim != 1 or not pcm.size or not np.isfinite(pcm).all()):
        raise ValueError('Expected nonempty finite 1-D float32 PCM')


class QwenNativeRuntime(NativeRuntime):
    # Pinned 0.6B encoder: 1,500 positions, 13 per complete second. Keep at
    # most 115 seconds (1,495 positions) unsplit; native auto chunking handles
    # longer captures in contiguous 30-second pieces, including the last tail.
    MAX_UNCHUNKED_SAMPLES = 115 * NativeRuntime.SAMPLE_RATE

    def __init__(self, model_path, library_path, *, backend='cuda',
                 dll_directories=(), threads=4):
        super().__init__(model_path, library_path, backend=backend,
                         dll_directories=dll_directories, threads=threads,
                         family='qwen3_asr', mode='offline')

    def run(self, pcm, context='', language=None, timestamps='none'):
        if not self.session:
            raise RuntimeError('Qwen3-ASR runtime is closed')
        _validate_pcm(pcm)
        if timestamps != 'none':
            raise ValueError('Qwen3-ASR native timestamps must be none')
        audio = np.ascontiguousarray(pcm)
        request = self.functions['request_create']()
        if not request:
            raise RuntimeError('Cannot allocate audio.cpp request')
        result = ct.c_void_p()
        try:
            # Fresh requests clear both context and forced language between runs.
            self._call('request_set_text', request, context.encode('utf-8'),
                       language.encode('utf-8') if language else None)
            for key, value in ((b'max_tokens', b'512'),
                               (b'return_timestamps', b'false'),
                               (b'audio_chunk_mode', b'none' if len(audio) <= self.MAX_UNCHUNKED_SAMPLES else b'auto')):
                self._call('request_set_option', request, key, value)
            self._call('request_set_audio', request,
                       audio.ctypes.data_as(ct.POINTER(ct.c_float)),
                       len(audio), self.SAMPLE_RATE, 1)
            self._call('session_run', self.session, request, ct.byref(result))
            text, detected_language = self._read_text(result)
            return SimpleNamespace(text=text, language=detected_language)
        finally:
            if result:
                self.functions['result_free'](result)
            self.functions['request_free'](request)


class QwenPreviewRuntime:
    """Revise bounded previews; finalize from the complete captured audio."""

    def __init__(self, transcribe, *, sample_rate=16000, preview_samples=3200,
                 window_samples=192000):
        for name, value in (('sample_rate', sample_rate),
                            ('preview_samples', preview_samples),
                            ('window_samples', window_samples)):
            if not isinstance(value, int) or isinstance(value, bool) or value <= 0:
                raise ValueError(f'{name} must be a positive integer')
        if sample_rate != NativeRuntime.SAMPLE_RATE:
            raise ValueError('Qwen3-ASR requires 16000 Hz PCM')
        self._transcribe = transcribe
        self._minimum_samples = sample_rate // 2
        self._preview_samples = preview_samples
        # Three 40-ms capture polls give the first recognizable word an earlier
        # attempt; steady previews retain their lower inference frequency.
        self._startup_preview_samples = min(preview_samples, sample_rate * 120 // 1000)
        self._window_samples = window_samples
        self.reset()

    def start(self, context='', language=None):
        self.reset()
        self._active = True
        self._context = context
        self._forced_language = language
        self._language = language

    def _decode(self, pcm):
        # A short first preview/final needs acoustic context, without changing
        # the captured samples retained for the eventual full-clip decode.
        if len(pcm) < self._minimum_samples:
            pcm = np.pad(pcm, (0, self._minimum_samples - len(pcm)))
        return self._transcribe(pcm, context=self._context,
                                language=self._forced_language)

    def feed(self, pcm, *, preview=True):
        if not self._active:
            raise RuntimeError('No active Qwen3-ASR stream')
        if not isinstance(preview, bool):
            raise ValueError('preview must be a boolean')
        _validate_pcm(pcm)
        captured = pcm.copy()
        self._chunks.append(captured)
        self._sample_count += len(captured)
        window = np.concatenate((self._window, captured[-self._window_samples:]))
        self._window = window[-self._window_samples:].copy()
        self._new_samples += len(captured)
        threshold = self._preview_samples if self._has_preview else self._startup_preview_samples
        if preview and self._new_samples >= threshold:
            try:
                result = self._decode(self._window.copy())
                self._preview_text = result.text
                self._has_preview = self._has_preview or bool(result.text)
                self._language = result.language or self._language
                self._preview_sample_count = self._sample_count
                self._new_samples = 0
            except BaseException:
                self.reset()
                raise
        # Window hypotheses may change; only finish publishes committed text.
        return '', self._language

    @property
    def preview_text(self):
        return self._preview_text

    def finish(self):
        if not self._active:
            raise RuntimeError('No active Qwen3-ASR stream')
        try:
            if not self._chunks:
                return '', self._language
            # Reuse only a preview of every captured sample. A single new
            # sample or earlier audio outside the window needs a full decode.
            if (self._preview_sample_count == self._sample_count
                    and self._sample_count <= self._window_samples):
                return self._preview_text, self._language
            result = self._decode(np.concatenate(self._chunks))
            return result.text, result.language or self._language
        finally:
            self.reset()

    def reset(self):
        self._active = False
        self._chunks = []
        self._sample_count = 0
        self._preview_sample_count = None
        self._window = np.empty(0, dtype=np.float32)
        self._new_samples = 0
        self._context = ''
        self._forced_language = None
        self._language = None
        self._preview_text = ''
        self._has_preview = False
