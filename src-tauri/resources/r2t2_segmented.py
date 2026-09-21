"""Bounded VAD segmentation around the native R2T2 streaming runtime."""

import numpy as np


class SegmentedR2T2:
    """Keep native streaming bounded while preserving its committed text."""

    def __init__(
        self,
        native,
        vad,
        *,
        chunk_samples=5120,
        window_samples=16000,
        min_segment_samples=128000,
        max_segment_samples=480000,
        pause_samples=4800,
    ):
        self.native = native
        self.vad = vad
        self.chunk_samples = chunk_samples
        self.window_samples = window_samples
        self.min_segment_samples = min_segment_samples
        self.max_segment_samples = max_segment_samples
        self.pause_samples = pause_samples
        self._clear_state()

    def _clear_state(self):
        self._active = False
        self._context = ""
        self._start_language = None
        self._language = None
        self._pending = np.empty(0, dtype=np.float32)
        self._vad_tail = np.empty(0, dtype=np.float32)
        self._native_active = False
        self._segment_samples = 0
        self._segment_text = ""
        self._segment_preview = ""
        self._segments = []
        self.preview_text = ""

    def _invalidate_after_error(self):
        self._clear_state()
        try:
            self.native.reset()
        except Exception:
            pass

    def _require_active(self):
        if not self._active:
            raise ValueError("segmented stream is not active")

    def _remember_language(self, language):
        if language is not None and language != "":
            self._language = language

    def _native_preview(self, committed):
        native_dict = getattr(self.native, "__dict__", {})
        declared = any(
            "preview_text" in cls.__dict__ for cls in type(self.native).__mro__
        )
        if not declared and "preview_text" not in native_dict:
            preview = committed
        else:
            preview = self.native.preview_text

        if not isinstance(preview, str) or not preview.startswith(committed):
            raise RuntimeError("native runtime preview regressed")
        return preview

    @staticmethod
    def _is_ascii_alnum(character):
        return (
            "A" <= character <= "Z"
            or "a" <= character <= "z"
            or "0" <= character <= "9"
        )

    def _joined_text(self, include_current=True, current_text=None):
        pieces = list(self._segments)
        if include_current and self._native_active:
            piece = self._segment_text if current_text is None else current_text
            if piece:
                pieces.append(piece)
        if not pieces:
            return ""

        result = pieces[0]
        for piece in pieces[1:]:
            if (
                result
                and piece
                and (self._is_ascii_alnum(result[-1]) or result[-1] in ".!?:;")
                and self._is_ascii_alnum(piece[0])
            ):
                result += " "
            result += piece
        return result

    def _refresh_preview(self):
        self.preview_text = self._joined_text(current_text=self._segment_preview)

    def _vad_window(self, block):
        if self._vad_tail.size:
            observed = np.concatenate((self._vad_tail, block))
        else:
            observed = np.array(block, copy=True)
        window_limit = self.window_samples
        if self.max_segment_samples is not None:
            window_limit = min(window_limit, self.max_segment_samples)
        if observed.size > window_limit:
            observed = observed[-window_limit:]
        return np.array(observed, copy=True)

    @staticmethod
    def _trailing_silence(window, regions):
        if not regions:
            return int(window.size)

        last_end = 0
        for region in regions:
            try:
                end = int(region["end"])
            except (KeyError, TypeError, ValueError):
                continue
            last_end = max(last_end, min(int(window.size), end))
        return max(0, int(window.size) - last_end)

    def _observe(self, block):
        observed = self._vad_window(block)
        regions = self.vad.speech_timestamps(np.array(observed, copy=True))
        self._vad_tail = observed
        return observed, regions or [], self._trailing_silence(observed, regions)

    def _native_feed(self, pcm):
        previous_text = self._segment_text
        try:
            text, language = self.native.feed(np.array(pcm, copy=True))
            if not isinstance(text, str) or not text.startswith(previous_text):
                raise RuntimeError("native runtime text regressed")
            preview = self._native_preview(text)
        except Exception:
            self._invalidate_after_error()
            raise

        self._segment_text = text
        self._segment_preview = preview
        self._remember_language(language)
        self._refresh_preview()

    def _start_segment(self, initial_pcm):
        self._native_active = True
        self._segment_samples = int(initial_pcm.size)
        self._segment_text = ""
        self._segment_preview = ""
        try:
            self.native.start(self._context, self._start_language)
        except Exception:
            self._invalidate_after_error()
            raise
        self._native_feed(initial_pcm)

    def _finish_segment(self):
        previous_text = self._segment_text
        try:
            text, language = self.native.finish()
        except Exception:
            self._invalidate_after_error()
            raise

        if not isinstance(text, str) or not text.startswith(previous_text):
            self._invalidate_after_error()
            raise RuntimeError("native runtime text regressed")

        self._remember_language(language)
        if text:
            self._segments.append(text)
        self._native_active = False
        self._segment_samples = 0
        self._segment_text = ""
        self._segment_preview = ""
        self._vad_tail = np.empty(0, dtype=np.float32)
        self._refresh_preview()

    def _process_block(self, block):
        observed, regions, trailing_silence = self._observe(block)
        if not self._native_active:
            if not regions:
                return
            initial_pcm = observed
            if self.max_segment_samples is not None and initial_pcm.size > self.max_segment_samples:
                initial_pcm = initial_pcm[-self.max_segment_samples:]
            self._start_segment(initial_pcm)
        else:
            self._native_feed(block)
            self._segment_samples += int(block.size)

        if (
            self.max_segment_samples is not None
            and self._segment_samples >= self.max_segment_samples
        ) or (
            self._segment_samples >= self.min_segment_samples
            and trailing_silence >= self.pause_samples
        ):
            self._finish_segment()

    def start(self, context="", language=None):
        self._clear_state()
        self.native.reset()
        self._active = True
        self._context = context
        self._start_language = language
        self._language = language

    def feed(self, pcm):
        self._require_active()
        if self._pending.size:
            combined = np.concatenate((self._pending, pcm))
        else:
            combined = np.array(pcm, copy=True)

        full_size = (combined.size // self.chunk_samples) * self.chunk_samples
        for start in range(0, full_size, self.chunk_samples):
            self._process_block(combined[start : start + self.chunk_samples])
        self._pending = np.array(combined[full_size:], copy=True)
        return self._joined_text(), self._language

    def finish(self):
        self._require_active()
        pending = self._pending
        self._pending = np.empty(0, dtype=np.float32)
        if pending.size:
            self._process_block(pending)
        if self._native_active:
            self._finish_segment()

        text = self._joined_text(include_current=False)
        language = self._language
        self._clear_state()
        return text, language

    def reset(self):
        self._clear_state()
        self.native.reset()
