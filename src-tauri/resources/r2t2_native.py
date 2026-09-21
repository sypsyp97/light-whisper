"""Resident Windows-native R2T2 inference through audio.cpp's C ABI.

The caller serializes access. Native push/finish cannot be interrupted safely
from another thread; cancellation happens between bounded calls or by stopping
the isolated engine process.
"""
import ctypes as ct
import os
from pathlib import Path

import numpy as np


class _ModelConfig(ct.Structure):
    _fields_ = [(name, ct.c_char_p) for name in ('family', 'config', 'weight', 'spec')]


class _BackendConfig(ct.Structure):
    _fields_ = [('backend', ct.c_char_p), ('device', ct.c_int), ('threads', ct.c_int)]


class NativeRuntime:
    SAMPLE_RATE = 16000

    def __init__(self, model_path, library_path, *, backend='cuda', chunk_ms=320,
                 threads=4, dll_directories=(), rolling=False):
        if backend not in ('cpu', 'cuda'):
            raise ValueError('Unsupported R2T2 backend')
        if not isinstance(rolling, bool):
            raise ValueError('rolling must be a boolean')
        self.backend = backend
        self.rolling = rolling
        if not isinstance(chunk_ms, int) or isinstance(chunk_ms, bool) or not 80 <= chunk_ms <= 2000:
            raise ValueError('chunk_ms must be an integer from 80 to 2000')
        self.chunk_samples = chunk_ms * 16
        self.registry = ct.c_void_p()
        self.model = ct.c_void_p()
        self.session = ct.c_void_p()
        self._dll_paths = []
        self._active = False
        self._committed = ''
        self._preview_text = ''
        self._language = None
        self._offset = 0
        library_path = Path(library_path).resolve()
        self.lib = None
        self.functions = {}
        try:
            if os.name == 'nt':
                for directory in (library_path.parent, *map(Path, dll_directories)):
                    self._dll_paths.append(os.add_dll_directory(str(directory.resolve())))
            self.lib = ct.CDLL(str(library_path))
            self._bind()
            self._call('registry_create', None, ct.byref(self.registry))
            config = _ModelConfig(b'confucius4_r2t2', None, None, None)
            self._call('model_load', self.registry, str(Path(model_path).resolve()).encode('utf-8'),
                       ct.byref(config), None, ct.byref(self.model))
            options = self.functions['options_create']()
            if not options:
                raise RuntimeError('Cannot allocate audio.cpp options')
            try:
                # Keep the upstream validated conservative rollback defaults.
                self._call('options_set', options, b'confucius4_r2t2.chunk_size_ms', str(chunk_ms).encode())
                if rolling:
                    self._call('options_set', options, b'confucius4_r2t2.rolling_window', b'true')
                    # Commit sentence endings using R2T2's native policy, so a
                    # completed suffix is not kept behind rollback at a roll.
                    self._call('options_set', options, b'confucius4_r2t2.rollback_punctuation', b'true')
                self._call('session_create', self.model, b'asr', b'streaming',
                           ct.byref(_BackendConfig(backend.encode(), 0, threads)), options,
                           ct.byref(self.session))
            finally:
                self.functions['options_free'](options)
        except BaseException:
            self.close()
            raise

    def _bind(self):
        H, S, I, P = ct.c_void_p, ct.c_char_p, ct.c_int, ct.POINTER
        version = self.lib.audiocpp_abi_version
        version.restype, version.argtypes = ct.c_uint32, []
        abi = version()
        if abi >> 16 != 0 or (abi >> 8) & 0xff < 3:
            raise RuntimeError(f'Unsupported audio.cpp ABI {abi:#x}; preview requires 0.3 or newer')
        self._batch_initial_audio = (
            self.backend == 'cuda'
            and self.rolling
            and (abi >> 8) & 0xff >= 4
        )
        signatures = {
            'abi_version': (ct.c_uint32, []),
            'last_error': (S, []),
            'registry_create': (I, [S, P(H)]),
            'model_load': (I, [H, S, P(_ModelConfig), H, P(H)]),
            'options_create': (H, []),
            'options_set': (I, [H, S, S]),
            'session_create': (I, [H, S, S, P(_BackendConfig), H, P(H)]),
            'request_create': (H, []),
            'request_set_text': (I, [H, S, S]),
            'stream_start': (I, [H, H]),
            'stream_push': (I, [H, P(ct.c_float), ct.c_size_t, I, I, ct.c_int64, P(H)]),
            'stream_finish': (I, [H, P(H)]),
            'stream_reset': (I, [H]),
            'event_as_result': (H, [H]),
            'result_text': (I, [H, P(S), P(S)]),
            'result_preview_text': (I, [H, P(S)]),
        }
        for kind in ('registry', 'model', 'session', 'options', 'request', 'result', 'event'):
            signatures[kind + '_free'] = (None, [H])
        for name, (result, arguments) in signatures.items():
            function = getattr(self.lib, 'audiocpp_' + name)
            function.restype, function.argtypes = result, arguments
            self.functions[name] = function

    def _call(self, name, *args):
        status = self.functions[name](*args)
        if status:
            detail = (self.functions['last_error']() or b'unknown error').decode('utf-8', errors='replace')
            raise RuntimeError(f'R2T2 {name} failed ({status}): {detail}')

    def _read_text(self, result):
        text, language = ct.c_char_p(), ct.c_char_p()
        status = self.functions['result_text'](result, ct.byref(text), ct.byref(language))
        if status == 7:  # Empty events are normal before the first stable word.
            return '', None
        if status:
            detail = self.functions['last_error']().decode('utf-8', errors='replace')
            raise RuntimeError(f'R2T2 result_text failed ({status}): {detail}')
        return (text.value or b'').decode('utf-8'), (language.value or b'').decode('utf-8') or None

    def start(self, context='', language=None):
        if not self.session:
            raise RuntimeError('R2T2 runtime is closed')
        request = self.functions['request_create']()
        if not request:
            raise RuntimeError('Cannot allocate audio.cpp request')
        try:
            # Fresh request avoids retaining the previous stream's forced language.
            self._call('request_set_text', request, context.encode('utf-8'),
                       language.encode('utf-8') if language else None)
            self._call('stream_start', self.session, request)
        finally:
            self.functions['request_free'](request)
        self._active = True
        self._offset = 0
        self._committed = ''
        self._preview_text = ''
        self._language = language or None

    @property
    def preview_text(self):
        return self._preview_text

    def feed(self, pcm):
        if not self._active:
            raise RuntimeError('No active R2T2 stream')
        if not isinstance(pcm, np.ndarray) or pcm.dtype != np.float32 or pcm.ndim != 1 or not np.isfinite(pcm).all():
            raise ValueError('Expected finite 1-D float32 PCM')

        push_size = self.chunk_samples
        # ABI 0.4 decodes the bounded initial VAD prefix once on CUDA.
        if (
            self._offset == 0
            and getattr(self, '_batch_initial_audio', False)
            and len(pcm) > self.chunk_samples
            and len(pcm) <= self.SAMPLE_RATE
            and len(pcm) % self.chunk_samples == 0
        ):
            push_size = len(pcm)

        # All other pushes must decode at most one internal chunk: the C ABI
        # event container otherwise retains only the last delta.
        for start in range(0, len(pcm), push_size):
            chunk = np.ascontiguousarray(pcm[start:start + push_size])
            event = ct.c_void_p()
            try:
                self._call('stream_push', self.session, chunk.ctypes.data_as(ct.POINTER(ct.c_float)),
                           len(chunk), self.SAMPLE_RATE, 1, self._offset, ct.byref(event))
                self._offset += len(chunk)
                if event:
                    result = self.functions['event_as_result'](event)
                    delta, language = self._read_text(result)
                    self._committed += delta
                    preview = ct.c_char_p()
                    status = self.functions['result_preview_text'](result, ct.byref(preview))
                    if status == 0:
                        self._preview_text = (preview.value or b'').decode('utf-8')
                    elif status != 7:  # No decode yet for a partial audio chunk.
                        raise RuntimeError(f'R2T2 result_preview_text failed ({status})')
                    if not self._preview_text.startswith(self._committed):
                        raise RuntimeError('R2T2 preview conflicts with committed text')
                    if language:
                        self._language = language
            finally:
                self.functions['event_free'](event)
        return self._committed, self._language

    def finish(self):
        if not self._active:
            raise RuntimeError('No active R2T2 stream')
        result = ct.c_void_p()
        try:
            self._call('stream_finish', self.session, ct.byref(result))
            text, language = self._read_text(result)
            self._active = False
            self._preview_text = ''
            return text, language or self._language
        finally:
            self.functions['result_free'](result)

    def reset(self):
        was_active = self._active
        self._active = False
        self._committed = ''
        self._preview_text = ''
        self._language = None
        self._offset = 0
        if self.session and was_active:
            self._call('stream_reset', self.session)

    def close(self):
        self._active = False
        for name in ('session', 'model', 'registry'):
            handle = getattr(self, name, None)
            function = self.functions.get(name + '_free')
            if handle and function:
                function(handle)
            setattr(self, name, ct.c_void_p())
        for directory in self._dll_paths:
            directory.close()
        self._dll_paths.clear()
