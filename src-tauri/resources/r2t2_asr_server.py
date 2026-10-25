"""Local stdio server adapter for the resident Confucius4-R2T2 runtime."""

import hashlib
import os
import time
from pathlib import Path

import numpy as np

from asr_stream_server import ASRStreamServerMixin, MAX_ASR_HOT_WORDS, MAX_HOT_WORD_CHARS
from firered_vad import FireRedVad
from hf_cache_utils import R2T2_MODEL, get_hf_cache_root
from r2t2_native import NativeRuntime
from r2t2_segmented import SegmentedR2T2
from r2t2_stream import R2T2StreamSession
from server_common import (
    BaseASRServer,
    decode_inline_audio,
    setup_rotating_logger,
)


logger = setup_rotating_logger(__name__, "r2t2_asr_server.log", "R2T2服务器")

class R2T2ASRServer(ASRStreamServerMixin, BaseASRServer):
    """Keep the native model resident for streaming and bounded offline work.

    Idle unload, when enabled, frees the native model only while no stream
    is active. The next transcribe or stream_start calls initialize().
    """

    _stream_display_name = "R2T2"

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

    def _suspend_gpu_runtime(self):
        """Free the native model. Caller must hold the command lock.

        An active stream blocks unload; FireRedVAD stays on CPU.
        """
        if not getattr(self, "initialized", False):
            return False
        if self._stream_is_active():
            return False
        vad = getattr(self, "vad_model", None)
        self._close_runtime()
        self.segmented = None
        self.stream = None
        self.initialized = False
        self._gpu_suspended = True
        self.vad_model = vad
        self.logger.info("R2T2 空闲超时，已卸载 GPU 模型；FireRedVAD 仍留在 CPU")
        return True

    def _close_runtime(self):
        runtime = getattr(self, "native", None)
        self.native = None
        if runtime is not None:
            try:
                runtime.close()
            except Exception:
                pass

    def _load_runtime(self, model_path):
        self._gpu_device_info_cache = None
        runtime_root = Path(__file__).resolve().with_name("r2t2-native")
        preferred = ["cuda", "cpu"]
        self.native = None
        for backend in preferred:
            candidate = None
            try:
                logger.info("R2T2 %s 后端开始加载与预热", backend)
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
                if backend == "cuda":
                    # Pay the first CUDA setup cost before reporting ready.
                    # Reset even on failure; warmup must not enter a recording.
                    try:
                        candidate.start()
                        candidate.feed(
                            np.zeros(candidate.chunk_samples, dtype=np.float32)
                        )
                    finally:
                        candidate.reset()
                self.native = candidate
                self.backend = backend
                self.device = backend
                return
            except Exception as exc:
                logger.warning(
                    "R2T2 %s 后端加载或预热失败 (%s)", backend, type(exc).__name__
                )
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
                **self._get_gpu_device_info(),
            }

        started = time.perf_counter()
        resuming = getattr(self, "_gpu_suspended", False)
        self.logger.info("R2T2 %s开始", "空闲恢复" if resuming else "初始化")
        if getattr(self, "native", None) is not None:
            self._close_runtime()
        self.native = None
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
                if self.vad_model is None:
                    self.vad_model = FireRedVad()
                self.segmented = SegmentedR2T2(
                    self.native,
                    self.vad_model,
                    chunk_samples=getattr(self.native, "chunk_samples", 5120),
                    max_segment_samples=None,
                )
                self.stream = R2T2StreamSession(self.segmented)
            self.initialized = True
            self._gpu_suspended = False
            self._last_load_error = None
            self.logger.info(
                "R2T2 %s完成，后端: %s，耗时 %.3f 秒",
                "空闲恢复" if resuming else "初始化",
                self.backend,
                time.perf_counter() - started,
            )
            return {
                "success": True,
                "message": "R2T2 初始化成功",
                "model_loaded": True,
                "engine": self.engine,
                "backend": self.backend,
                **self._get_gpu_device_info(),
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
                audio, source_rate = self._read_mono_audio(audio)
                audio = self._resample(audio, source_rate)
            return np.ascontiguousarray(audio, dtype=np.float32), duration, input_mode

        if not audio_path or not os.path.exists(audio_path):
            raise FileNotFoundError("audio file not found")
        audio, source_rate = self._read_mono_audio(audio_path)
        duration = len(audio) / float(source_rate)
        return (
            np.ascontiguousarray(self._resample(audio, source_rate), dtype=np.float32),
            duration,
            "path",
        )

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
            return self._stream_busy_response()

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
        return self._with_gpu_suspend_flag({
            "success": True,
            "installed": native_loaded,
            "initialized": initialized,
            "engine": self.engine,
            "backend": getattr(self, "backend", "unknown"),
            **self._get_gpu_device_info(),
            "model_loaded": initialized and native_loaded,
            "native_streaming": initialized and native_loaded and getattr(self, "stream", None) is not None,
            "inline_audio": initialized and native_loaded,
            "models": {"asr": native_loaded, "vad": vad_loaded},
        })

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


if __name__ == "__main__":
    R2T2ASRServer().run()
