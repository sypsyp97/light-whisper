"""Shared stdio stream commands and guards for resident local ASR servers."""

import base64
import time

import numpy as np


# Match Rust's ASR selection limit and profile_service's user hotword length.
MAX_ASR_HOT_WORDS = 100
MAX_HOT_WORD_CHARS = 80


class ASRStreamServerMixin:
    """Use an initialized stream with the common session and PCM protocol."""

    def _stream_is_active(self):
        stream = getattr(self, "stream", None)
        return bool(stream is not None and getattr(stream, "active", False))

    def _gpu_idle_stream_active(self):
        return self._stream_is_active()

    def _stream_busy_response(self):
        return {
            "success": False,
            "error": f"{self._stream_display_name} 流式会话正在进行",
            "type": "stream_busy",
            "engine": self.engine,
            "backend": self.backend,
        }

    @staticmethod
    def _normalize_language(language):
        if language is None:
            return None
        if not isinstance(language, str):
            raise ValueError("language must be a string or null")
        language = language.strip()
        return language or None

    @staticmethod
    def _normalize_hot_words(hot_words):
        if not isinstance(hot_words, list):
            raise ValueError("hot_words must be a list")
        if any(not isinstance(word, str) for word in hot_words):
            raise ValueError("hot_words must contain strings")
        words = []
        seen = set()
        for word in hot_words:
            word = " ".join(word.split())
            if not word or len(word) > MAX_HOT_WORD_CHARS:
                continue
            key = word.lower()
            if key in seen:
                continue
            seen.add(key)
            words.append(word)
            if len(words) == MAX_ASR_HOT_WORDS:
                break
        return words

    @classmethod
    def _compose_context(cls, context, hot_words):
        if not isinstance(context, str):
            raise ValueError("context must be a string")
        parts = []
        context = context.strip()
        if context:
            parts.append(context)
        words = cls._normalize_hot_words(hot_words)
        if words:
            parts.append("Hotwords: " + ", ".join(words))
        return "\n".join(parts)

    def _stream_error(self):
        return {
            "success": False,
            "type": "stream_error",
            "error": f"{self._stream_display_name} 流式命令失败",
            "engine": self.engine,
            "backend": getattr(self, "backend", "unknown"),
        }

    def _stream_response(self, response):
        return {
            **response,
            "engine": self.engine,
            "backend": self.backend,
        }

    @staticmethod
    def _decode_stream_audio(payload):
        if not isinstance(payload, str) or not payload:
            raise ValueError("audio_base64 must be a non-empty string")
        try:
            raw = base64.b64decode(payload, validate=True)
        except (ValueError, TypeError, base64.binascii.Error) as exc:
            raise ValueError("invalid PCM16 base64") from exc
        if not raw or len(raw) % 2:
            raise ValueError("PCM16 payload must be non-empty and even-sized")
        return np.ascontiguousarray(
            np.frombuffer(raw, dtype="<i2").astype(np.float32) / 32768.0
        )

    def handle_stream_command(self, command):
        with self.stdout_suppressor.suppress():
            return self._handle_stream_command(command)

    def _handle_stream_command(self, command):
        if not getattr(self, "initialized", False):
            init_result = self.initialize()
            if not init_result.get("success"):
                return init_result

        try:
            if not isinstance(command, dict):
                raise ValueError("stream command must be an object")
            action = command.get("action")
            started = time.perf_counter()
            if action == "stream_start":
                context = self._compose_context(
                    command.get("context", ""), command.get("hot_words", [])
                )
                language = self._normalize_language(command.get("language"))
                response = self.stream.start(
                    command.get("session_id"),
                    context=context,
                    language=language,
                )
            elif action == "stream_feed":
                preview = command.get("preview", True)
                if not isinstance(preview, bool):
                    raise ValueError("preview must be a boolean")
                if not preview and self.engine != "qwen3-asr-0.6b":
                    raise ValueError("preview suppression requires Qwen3-ASR")
                if command.get("sample_rate") != 16_000:
                    raise ValueError("sample_rate must be 16000")
                if command.get("audio_format") != "pcm_s16le":
                    raise ValueError("audio_format must be pcm_s16le")
                audio = self._decode_stream_audio(command.get("audio_base64"))
                response = self.stream.feed(
                    command.get("session_id"),
                    audio,
                    offset=command.get("offset"),
                    preview=preview,
                )
            elif action == "stream_finish":
                response = self.stream.finish(command.get("session_id"))
            elif action == "stream_cancel":
                response = self.stream.cancel(command.get("session_id"))
            else:
                raise ValueError("unknown stream action")
            elapsed_ms = (time.perf_counter() - started) * 1000
            if action == "stream_start":
                self._stream_inference_ms = elapsed_ms
            elif action == "stream_feed":
                self._stream_inference_ms = getattr(self, "_stream_inference_ms", 0.0) + elapsed_ms
            elif action == "stream_finish":
                self.transcription_count += 1
                self.total_audio_duration += response["sample_count"] / 16_000
                self._total_inference_ms += getattr(self, "_stream_inference_ms", 0.0) + elapsed_ms
                self._stream_inference_ms = 0.0
            elif action == "stream_cancel":
                self._stream_inference_ms = 0.0
            return self._stream_response(response)
        except Exception:
            self.logger.error("%s 流式命令失败", self._stream_display_name)
            return self._stream_error()
