"""Local stdio server adapter for the resident Confucius4-R2T2 runtime."""

import base64
import hashlib
import os
import time
from pathlib import Path

import numpy as np

from firered_vad import FireRedVad
from hf_cache_utils import R2T2_MODEL, get_hf_cache_root
from r2t2_native import NativeRuntime
from r2t2_segmented import SegmentedR2T2
from r2t2_stream import R2T2StreamSession
from server_common import (
    BaseASRServer,
    _has_nvidia_gpu,
    decode_inline_audio,
    setup_rotating_logger,
)


logger = setup_rotating_logger(__name__, "r2t2_asr_server.log", "R2T2服务器")


class R2T2ASRServer(BaseASRServer):
    """Keep the native model resident for streaming and bounded offline work."""

    def __init__(self):
        super().__init__(engine="confucius4-r2t2", logger=logger)
        self.model_config = R2T2_MODEL
        self.native = None
        self.vad_model = None
        self.segmented = None
        self.stream = None
        self.backend = "cuda" if self.device == "cuda" else "cpu"
        self._last_load_error = None
        self._verified_model_path = None
        self._total_inference_ms = 0.0

    def _detect_device(self):
        return "cuda" if _has_nvidia_gpu() else "cpu"

    def _get_model_repos(self):
        return []

    def _resolve_model_path(self):
        verified_model_path = getattr(self, "_verified_model_path", None)
        if verified_model_path:
            cached = Path(verified_model_path)
            if cached.is_file():
                return str(cached)

        model = getattr(self, "model_config", R2T2_MODEL)
        snapshot = (
            Path(get_hf_cache_root())
            / ("models--" + model["repo_id"].replace("/", "--"))
            / "snapshots"
            / model["revision"]
        )
        candidate = snapshot / model["filename"]
        try:
            if candidate.stat().st_size != model["size"]:
                return None
            digest = hashlib.sha256()
            with candidate.open("rb") as source:
                for block in iter(lambda: source.read(1024 * 1024), b""):
                    digest.update(block)
            if digest.hexdigest().lower() != model["sha256"].lower():
                return None
        except OSError:
            return None

        self._verified_model_path = str(candidate)
        return str(candidate)

    def _close_runtime(self):
        runtime = getattr(self, "native", None)
        self.native = None
        if runtime is not None:
            try:
                runtime.close()
            except Exception:
                pass

    def _load_runtime(self, model_path):
        runtime_root = Path(__file__).resolve().with_name("r2t2-native")
        preferred = ["cuda", "cpu"]
        self.native = None
        for backend in preferred:
            candidate = None
            try:
                backend_root = runtime_root / backend
                candidate = NativeRuntime(
                    model_path,
                    backend_root / "audiocpp.dll",
                    backend=backend,
                    # Paired paced playback improved preview cadence on CUDA;
                    # keep the larger batch for slower CPU inference.
                    chunk_ms=160 if backend == "cuda" else 320,
                    rolling=True,
                    # Frozen bundles share identical CUDA/CRT DLLs with Qwen
                    # at _internal; standalone native builds keep local copies.
                    dll_directories=(runtime_root.parent,),
                )
                self.native = candidate
                self.backend = backend
                self.device = backend
                return
            except Exception:
                if candidate is not None:
                    try:
                        candidate.close()
                    except Exception:
                        pass
                self.native = None

        raise RuntimeError("R2T2 native runtime unavailable")

    def initialize(self):
        if getattr(self, "initialized", False):
            return {
                "success": True,
                "message": "模型已初始化",
                "engine": self.engine,
                "backend": self.backend,
            }

        if getattr(self, "native", None) is not None:
            self._close_runtime()
        self.native = None
        self.vad_model = None
        self.segmented = None
        self.stream = None
        model_path = self._resolve_model_path()
        if not model_path:
            self.initialized = False
            return {
                "success": False,
                "error": "R2T2 Q8 模型未下载或校验失败",
                "type": "models_not_downloaded",
                "engine": self.engine,
            }

        try:
            with self.stdout_suppressor.suppress():
                self._load_runtime(model_path)
                self.vad_model = FireRedVad()
                self.segmented = SegmentedR2T2(
                    self.native,
                    self.vad_model,
                    chunk_samples=getattr(self.native, "chunk_samples", 5120),
                    max_segment_samples=None,
                )
                self.stream = R2T2StreamSession(self.segmented)
            self.initialized = True
            self._last_load_error = None
            return {
                "success": True,
                "message": "R2T2 初始化成功",
                "model_loaded": True,
                "engine": self.engine,
                "backend": self.backend,
                "device": self.device,
            }
        except Exception:
            self._close_runtime()
            self.vad_model = None
            self.segmented = None
            self.stream = None
            self.initialized = False
            self._last_load_error = "R2T2 initialization failed"
            self.logger.error("R2T2 初始化失败")
            return {
                "success": False,
                "error": "R2T2 初始化失败",
                "type": "init_error",
                "engine": self.engine,
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
        words = []
        for word in hot_words:
            if not isinstance(word, str):
                raise ValueError("hot_words must contain strings")
            word = word.strip()
            if word:
                words.append(word)
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
            "error": "R2T2 流式命令失败",
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
                if command.get("sample_rate") != 16_000:
                    raise ValueError("sample_rate must be 16000")
                if command.get("audio_format") != "pcm_s16le":
                    raise ValueError("audio_format must be pcm_s16le")
                audio = self._decode_stream_audio(command.get("audio_base64"))
                response = self.stream.feed(
                    command.get("session_id"),
                    audio,
                    offset=command.get("offset"),
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
            self.logger.error("R2T2 流式命令失败")
            return self._stream_error()

    @staticmethod
    def _resample(audio, source_rate):
        if source_rate == 16_000:
            return np.asarray(audio, dtype=np.float32)
        if not isinstance(source_rate, (int, float)) or source_rate <= 0:
            raise ValueError("invalid sample rate")
        target_length = int(round(len(audio) * 16_000 / source_rate))
        if target_length <= 0:
            return np.empty(0, dtype=np.float32)
        return np.interp(
            np.linspace(0, max(0, len(audio) - 1), target_length),
            np.arange(len(audio), dtype=np.float64),
            np.asarray(audio, dtype=np.float32),
        ).astype(np.float32)

    def _load_audio(self, audio_path, audio_base64, audio_format, sample_rate):
        if audio_base64:
            audio, duration = decode_inline_audio(
                audio_base64, audio_format, sample_rate
            )
            input_mode = "memory"
            if isinstance(audio, np.ndarray):
                source_rate = sample_rate or 16_000
                audio = self._resample(audio, source_rate)
            else:
                import soundfile as sf

                audio, source_rate = sf.read(
                    audio, dtype="float32", always_2d=True
                )
                audio = audio.mean(axis=1, dtype=np.float32)
                audio = self._resample(audio, source_rate)
            return np.ascontiguousarray(audio, dtype=np.float32), duration, input_mode

        if not audio_path or not os.path.exists(audio_path):
            raise FileNotFoundError("audio file not found")
        import soundfile as sf

        audio, source_rate = sf.read(
            audio_path, dtype="float32", always_2d=True
        )
        audio = audio.mean(axis=1, dtype=np.float32)
        duration = len(audio) / float(source_rate)
        return (
            np.ascontiguousarray(self._resample(audio, source_rate), dtype=np.float32),
            duration,
            "path",
        )

    def _stream_is_active(self):
        stream = getattr(self, "stream", None)
        return bool(stream is not None and getattr(stream, "active", False))

    def transcribe_audio(
        self,
        audio_path,
        options=None,
        hot_words=None,
        audio_base64=None,
        audio_format=None,
        sample_rate=None,
    ):
        if not getattr(self, "initialized", False):
            init_result = self.initialize()
            if not init_result.get("success"):
                return init_result
        if self._stream_is_active():
            return {
                "success": False,
                "error": "R2T2 流式会话正在进行",
                "type": "stream_busy",
                "engine": self.engine,
                "backend": self.backend,
            }

        options = options if isinstance(options, dict) else {}
        input_mode = "memory" if audio_base64 else "path"
        segmented = None
        try:
            selected_hot_words = (
                hot_words if hot_words is not None else options.get("hot_words", [])
            )
            context = self._compose_context(
                options.get("context", ""), selected_hot_words
            )
            language = self._normalize_language(options.get("language"))
            audio, duration, input_mode = self._load_audio(
                audio_path, audio_base64, audio_format, sample_rate
            )
            audio = np.ascontiguousarray(np.asarray(audio, dtype=np.float32).reshape(-1))
            if not np.isfinite(audio).all():
                raise ValueError("audio contains non-finite samples")

            segmented = SegmentedR2T2(
                self.native,
                self.vad_model,
                chunk_samples=max(1, int(getattr(self.native, "chunk_samples", 5120))),
                max_segment_samples=None,
            )
            started = time.perf_counter()
            with self.stdout_suppressor.suppress():
                segmented.start(context=context, language=language)
                chunk_samples = max(
                    1, int(getattr(self.native, "chunk_samples", 5120))
                )
                for start in range(0, len(audio), chunk_samples):
                    segmented.feed(audio[start : start + chunk_samples])
                text, detected_language = segmented.finish()
            inference_ms = (time.perf_counter() - started) * 1000
            language = detected_language or language
            self.total_audio_duration += float(duration)
            self.transcription_count += 1
            self._total_inference_ms += inference_ms
            return {
                "success": True,
                "text": text,
                "raw_text": text,
                "language": language,
                "duration": duration,
                "engine": self.engine,
                "backend": self.backend,
                "input_mode": input_mode,
                "inference_ms": round(inference_ms, 3),
            }
        except Exception:
            if segmented is not None:
                try:
                    segmented.reset()
                except Exception:
                    pass
            self.logger.error("R2T2 离线转录失败")
            return {
                "success": False,
                "error": "R2T2 离线转录失败",
                "type": "transcription_error",
                "engine": self.engine,
                "backend": self.backend,
                "input_mode": input_mode,
            }

    def check_status(self):
        native_loaded = getattr(self, "native", None) is not None
        vad_loaded = getattr(self, "vad_model", None) is not None
        initialized = bool(getattr(self, "initialized", False))
        return {
            "success": True,
            "installed": native_loaded,
            "initialized": initialized,
            "engine": self.engine,
            "backend": getattr(self, "backend", "unknown"),
            "device": self.device,
            "model_loaded": initialized and native_loaded,
            "native_streaming": initialized and native_loaded and getattr(self, "stream", None) is not None,
            "inline_audio": initialized and native_loaded,
            "models": {"asr": native_loaded, "vad": vad_loaded},
        }

    def get_performance_stats(self):
        count = getattr(self, "transcription_count", 0)
        total = getattr(self, "_total_inference_ms", 0.0)
        return {
            "transcription_count": count,
            "total_audio_duration": round(getattr(self, "total_audio_duration", 0.0), 2),
            "average_inference_ms": round(total / max(1, count), 3),
            "initialized": bool(getattr(self, "initialized", False)),
            "engine": self.engine,
            "backend": getattr(self, "backend", "unknown"),
            "native_streaming": getattr(self, "native", None) is not None,
            "inline_audio": getattr(self, "native", None) is not None,
        }

    def _cleanup_memory(self):
        # Native allocations are owned by the resident model, not PyTorch.
        import gc
        gc.collect()


if __name__ == "__main__":
    R2T2ASRServer().run()
