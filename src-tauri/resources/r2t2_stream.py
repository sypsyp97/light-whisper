"""Session-safe controller for an injectable native R2T2 stream runtime."""

import numpy as np


class R2T2StreamSession:
    """Own one contiguous native stream at a time."""

    def __init__(self, runtime):
        self.runtime = runtime
        self._last_session_id = 0
        self._active_session_id = None
        self._text = ""
        self._language = None
        self._sample_count = 0

    @staticmethod
    def _is_positive_int(value):
        return isinstance(value, int) and not isinstance(value, bool) and value > 0

    @staticmethod
    def _response(
        session_id, text, language, sample_count, final, tentative_text=""
    ):
        return {
            "success": True,
            "session_id": session_id,
            "text": text,
            "tentative_text": tentative_text,
            "language": language,
            "sample_count": sample_count,
            "final": final,
        }

    def _clear_active(self):
        self._active_session_id = None
        self._text = ""
        self._language = None
        self._sample_count = 0

    def _require_active(self, session_id):
        if not self._is_positive_int(session_id):
            raise ValueError("invalid session_id")
        if session_id != self._active_session_id:
            raise ValueError("session is not active")

    def _invalidate_after_error(self):
        self._clear_active()
        try:
            self.runtime.reset()
        except Exception:
            pass

    def _remember_language(self, language):
        if language is not None and language != "":
            self._language = language

    def _preview_suffix(self, committed):
        runtime_dict = getattr(self.runtime, "__dict__", {})
        declared = any(
            "preview_text" in cls.__dict__ for cls in type(self.runtime).__mro__
        )
        if not declared and "preview_text" not in runtime_dict:
            preview = committed
        else:
            preview = self.runtime.preview_text

        if not isinstance(preview, str) or not preview.startswith(committed):
            raise RuntimeError("native runtime preview regressed")
        return preview[len(committed) :]

    @property
    def active(self):
        return self._active_session_id is not None

    def start(
        self,
        session_id: int,
        *,
        context: str = "",
        language: str | None = None,
    ) -> dict:
        if not self._is_positive_int(session_id) or session_id <= self._last_session_id:
            raise ValueError("session_id must be a new positive integer")

        self._last_session_id = session_id
        if self._active_session_id is not None:
            self._clear_active()
            self.runtime.reset()

        self._active_session_id = session_id
        self._text = ""
        self._language = language
        self._sample_count = 0
        try:
            self.runtime.start(context, language)
        except Exception:
            self._invalidate_after_error()
            raise

        return self._response(session_id, "", language, 0, False)

    def feed(self, session_id: int, pcm: np.ndarray, *, offset: int) -> dict:
        self._require_active(session_id)
        if not isinstance(offset, int) or isinstance(offset, bool) or offset < 0:
            raise ValueError("offset must be a non-negative integer")
        if (
            not isinstance(pcm, np.ndarray)
            or pcm.ndim != 1
            or pcm.dtype != np.dtype(np.float32)
            or pcm.size == 0
            or not np.isfinite(pcm).all()
        ):
            raise ValueError("pcm must be a non-empty finite float32 vector")
        if offset != self._sample_count:
            raise ValueError("offset must equal the accepted sample count")

        try:
            previous_text = self._text
            text, language = self.runtime.feed(pcm.copy())
            if not isinstance(text, str) or not text.startswith(previous_text):
                raise RuntimeError("native runtime text regressed")
            tentative_text = self._preview_suffix(text)
        except Exception:
            self._invalidate_after_error()
            raise

        self._text = text
        self._remember_language(language)
        self._sample_count += int(pcm.size)
        return self._response(
            session_id,
            self._text,
            self._language,
            self._sample_count,
            False,
            tentative_text,
        )

    def finish(self, session_id: int) -> dict:
        self._require_active(session_id)
        sample_count = self._sample_count
        language = self._language

        if sample_count == 0:
            self._clear_active()
            self.runtime.reset()
            return self._response(session_id, "", language, 0, True)

        previous_text = self._text
        try:
            text, native_language = self.runtime.finish()
        except Exception:
            self._invalidate_after_error()
            raise

        if not isinstance(text, str) or not text.startswith(previous_text):
            self._invalidate_after_error()
            raise RuntimeError("native runtime text regressed")

        self._remember_language(native_language)
        language = self._language
        self._clear_active()
        return self._response(session_id, text, language, sample_count, True)

    def cancel(self, session_id: int) -> dict:
        self._require_active(session_id)
        sample_count = self._sample_count
        language = self._language
        self._clear_active()
        self.runtime.reset()
        return self._response(session_id, "", language, sample_count, True)

    def close(self) -> None:
        if self._active_session_id is None:
            return None
        self._clear_active()
        self.runtime.reset()
        return None
