"""Resident Qwen audio.cpp inference and sample-driven caption previews."""

import ctypes as ct
from concurrent.futures import Future
from threading import Condition, Thread
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

    supports_preview_suppression = True

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

    def _pad_audio(self, pcm):
        # A short first preview/final needs acoustic context, without changing
        # the captured samples retained for the eventual full-clip decode.
        if len(pcm) < self._minimum_samples:
            pcm = np.pad(pcm, (0, self._minimum_samples - len(pcm)))
        return pcm

    def _decode(self, pcm):
        return self._transcribe(self._pad_audio(pcm), context=self._context,
                                language=self._forced_language)

    def _accept_pcm(self, pcm, *, preview):
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
        return preview and self._new_samples >= threshold

    def _publish_preview(self, result, sample_count):
        self._preview_text = result.text
        self._has_preview = self._has_preview or bool(result.text)
        self._language = result.language or self._language
        self._preview_sample_count = sample_count

    def feed(self, pcm, *, preview=True):
        if self._accept_pcm(pcm, preview=preview):
            try:
                result = self._decode(self._window.copy())
                self._publish_preview(result, self._sample_count)
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


class QwenPipelineRuntime(QwenPreviewRuntime):
    """Overlap one CPU preparer and one native worker behind bounded slots."""

    def __init__(self, transcribe, *, prepare_audio, sample_rate=16000,
                 preview_samples=3200, window_samples=192000):
        self._prepare_audio = prepare_audio
        self._condition = Condition()
        self._preparer = None
        self._worker = None
        self._raw_pending = None
        self._preparing = None
        self._pending = None
        self._running = None
        self._jobs = []
        self._failure = None
        self._closed = False
        super().__init__(transcribe, sample_rate=sample_rate,
                         preview_samples=preview_samples, window_samples=window_samples)

    def start(self, context='', language=None):
        if self._closed:
            raise RuntimeError('Qwen3-ASR runtime is closed')
        super().start(context, language)

    def _prepare(self, pcm):
        audio = self._prepare_audio(self._pad_audio(pcm))
        if (not isinstance(audio, np.ndarray) or audio.dtype != np.float32
                or audio.ndim != 1 or not np.isfinite(audio).all()):
            raise ValueError('Prepared audio must be a finite 1-D float32 vector')
        return np.ascontiguousarray(audio)

    def _prepare_loop(self):
        while True:
            with self._condition:
                while self._raw_pending is None:
                    if self._closed:
                        return
                    self._condition.wait()
                audio, future, context, language = self._raw_pending
                self._raw_pending = None
                self._preparing = future
            prepared = None
            try:
                prepared = self._prepare(audio)
                with self._condition:
                    if not future.cancelled() and self._failure is None:
                        if self._worker is None:
                            worker = Thread(target=self._worker_loop,
                                            name='qwen3-asr-native', daemon=True)
                            worker.start()
                            self._worker = worker
                        self._discard_slot('_pending')
                        self._pending = (prepared, future, context, language)
            except BaseException as error:
                with self._condition:
                    self._fail(future, error)
            finally:
                del audio, prepared, future, context, language
                with self._condition:
                    self._preparing = None
                    self._condition.notify_all()

    def _worker_loop(self):
        while True:
            with self._condition:
                while self._pending is None:
                    if self._closed:
                        return
                    self._condition.wait()
                audio, future, context, language = self._pending
                self._pending = None
                if not audio.size:
                    future.set_result(SimpleNamespace(text='', language=language))
                    del audio, future, context, language
                    self._condition.notify_all()
                    continue
                if not future.set_running_or_notify_cancel():
                    del audio, future, context, language
                    continue
                self._running = future
            try:
                result = self._transcribe(audio, context=context, language=language)
            except BaseException as error:
                with self._condition:
                    self._fail(future, error)
            else:
                with self._condition:
                    future.set_result(result)
                del result
            finally:
                # An idle worker must not pin the last prepared audio.
                del audio, future, context, language
                with self._condition:
                    self._running = None
                    self._condition.notify_all()

    def _discard_slot(self, name, *, keep=None):
        job = getattr(self, name)
        if job is not None and job[1] is not keep:
            job[1].cancel()
            setattr(self, name, None)
        self._jobs = [(count, future) for count, future in self._jobs if not future.cancelled()]

    def _discard_pending(self, *, keep=None):
        with self._condition:
            self._discard_slot('_raw_pending', keep=keep)
            self._discard_slot('_pending', keep=keep)
            if self._preparing is not None and self._preparing is not keep:
                # Preparation cannot be aborted, but its result can be skipped.
                self._preparing.cancel()
            self._jobs = [(count, future) for count, future in self._jobs if not future.cancelled()]

    def _fail(self, future, error):
        # A cancelled CPU job can still fail while draining. Preserve that
        # original error even though its Future can no longer accept it.
        if self._failure is None:
            self._failure = error
        if not future.done():
            future.set_exception(error)
        self._discard_pending()
        self._condition.notify_all()

    def _submit(self, audio, sample_count):
        future = Future()
        with self._condition:
            self._collect_previews()
            self._discard_slot('_raw_pending')
            self._raw_pending = (audio, future, self._context, self._forced_language)
            # Futures carry results and counts, never queued PCM work items.
            self._jobs.append((sample_count, future))
            if self._preparer is None:
                preparer = Thread(target=self._prepare_loop,
                                  name='qwen3-asr-prepare', daemon=True)
                try:
                    preparer.start()
                except BaseException as error:
                    self._fail(future, error)
                    raise
                self._preparer = preparer
            self._condition.notify_all()
        self._new_samples = 0

    def _collect_previews(self, *, wait=False):
        with self._condition:
            while True:
                if self._failure is not None:
                    raise self._failure
                if not self._jobs:
                    return
                sample_count, future = self._jobs[0]
                if not future.done():
                    if not wait:
                        return
                    self._condition.wait()
                    continue
                self._jobs.pop(0)
                self._publish_preview(future.result(), sample_count)

    def feed(self, pcm, *, preview=True):
        if not self._active:
            raise RuntimeError('No active Qwen3-ASR stream')
        if not isinstance(preview, bool):
            raise ValueError('preview must be a boolean')
        _validate_pcm(pcm)
        try:
            self._collect_previews()
            if self._accept_pcm(pcm, preview=preview):
                self._submit(self._window.copy(), self._sample_count)
        except BaseException:
            self.reset()
            raise
        return '', self._language

    @property
    def preview_text(self):
        try:
            self._collect_previews()
        except BaseException:
            self.reset()
            raise
        return self._preview_text

    def finish(self):
        if not self._active:
            raise RuntimeError('No active Qwen3-ASR stream')
        try:
            self._collect_previews()
            if not self._chunks:
                return '', self._language
            with self._condition:
                exact = next((future for count, future in self._jobs
                              if count == self._sample_count and not future.cancelled()), None)
                if (self._sample_count <= self._window_samples
                        and (self._preview_sample_count == self._sample_count or exact is not None)):
                    self._discard_pending(keep=exact)
                else:
                    # Cancel obsolete CPU work before it can enter native;
                    # the complete capture is next on the same CPU owner.
                    self._discard_pending()
                    self._submit(np.concatenate(self._chunks), self._sample_count)
            self._collect_previews(wait=True)
            return self._preview_text, self._language
        finally:
            self.reset()

    def reset(self):
        with self._condition:
            self._discard_pending()
            while self._preparing is not None or self._running is not None:
                self._condition.wait()
            self._jobs.clear()
            self._failure = None
        super().reset()

    def close(self):
        if self._closed:
            return
        self.reset()
        with self._condition:
            self._closed = True
            self._condition.notify_all()
        if self._preparer is not None:
            self._preparer.join()
        if self._worker is not None:
            self._worker.join()
