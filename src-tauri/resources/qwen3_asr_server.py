#!/usr/bin/env python3
# -*- coding: utf-8 -*-

"""Resident Qwen3-ASR GGUF server with a shared audio.cpp streaming path."""

import os
from pathlib import Path
import time
import traceback
from types import SimpleNamespace

from asr_stream_server import ASRStreamServerMixin
from firered_vad import FireRedVad
from qwen3_native import QwenNativeRuntime, QwenPreviewRuntime
from r2t2_stream import R2T2StreamSession

from server_common import (
    BaseASRServer,
    apply_hf_env_defaults,
    decode_inline_audio,
    setup_rotating_logger,
)

apply_hf_env_defaults()
logger = setup_rotating_logger(__name__, "qwen3_asr_server.log", "Qwen3-ASR服务器")

from hf_cache_utils import QWEN3_ASR_MODELS, find_hf_snapshot_file

QWEN3_VAD_SAMPLE_RATE = 16_000


class Qwen3ASRServer(ASRStreamServerMixin, BaseASRServer):
    """Keep one GGUF model and one KV session resident across requests.

    Idle unload, when the user turns it on, closes both and the next
    transcribe calls initialize() again. FireRedVAD stays loaded.
    """

    _stream_display_name = "Qwen3-ASR"

    def __init__(self, engine=None):
        engine = engine or os.environ.get("LIGHT_WHISPER_ASR_ENGINE", "qwen3-asr-0.6b")
        if engine not in QWEN3_ASR_MODELS:
            raise ValueError(f"不支持的 Qwen3-ASR 引擎: {engine}")
        self.model_config = QWEN3_ASR_MODELS[engine]
        super().__init__(engine=engine, logger=logger)
        self.model = None
        self.session = None
        self.stream = None
        self.vad_model = None
        self.backend = "cuda" if self.device == "cuda" else "cpu"
        self._gpu_device_info_cache = None
        self._last_load_error = None
        self._total_inference_ms = 0.0
        self._total_vad_ms = 0.0
        self._vad_calls = 0
        self._vad_rejected = 0

    def _get_gpu_device_info(self):
        return super()._get_gpu_device_info(include_memory=True)

    def _get_model_repos(self):
        # Exact-file validation happens in initialize(); a repository may contain
        # Q4/Q6 files that must not be mistaken for the selected Q8 model.
        return []

    def _resolve_model_path(self):
        return find_hf_snapshot_file(
            self.model_config["repo_id"], self.model_config["filename"]
        )

    def _suspend_gpu_runtime(self):
        """Unload the GGUF model and KV session. FireRedVAD stays on CPU."""
        if not self.initialized or self._stream_is_active():
            return False
        vad = self.vad_model
        self._close_runtime()
        self._gpu_suspended = True
        self.vad_model = vad
        self.logger.info("Qwen3-ASR 空闲超时，已卸载 GPU 模型；FireRedVAD 仍留在 CPU")
        return True

    def _close_runtime(self):
        self.initialized = False
        stream, self.stream = self.stream, None
        if stream is not None:
            try:
                stream.close()
            except Exception:
                pass
        session, self.session = self.session, None
        model, self.model = self.model, None
        for runtime in (session, model if model is not session else None):
            if runtime is None:
                continue
            try:
                runtime.close()
            except Exception:
                pass

    def _load_runtime(self, model_path):
        self._gpu_device_info_cache = None
        runtime_root = Path(__file__).resolve().with_name("r2t2-native")
        preferred = ["cuda", "cpu"] if self.device == "cuda" else ["cpu"]
        errors = []
        for backend in preferred:
            try:
                with self.stdout_suppressor.suppress():
                    runtime = QwenNativeRuntime(
                        model_path, runtime_root / backend / "audiocpp.dll",
                        backend=backend, dll_directories=(runtime_root.parent,),
                    )
                self.model = self.session = runtime
                self.backend = self.device = backend
                return
            except Exception as exc:
                errors.append(f"{backend}: {exc}")
                self._close_runtime()
                logger.warning("Qwen3-ASR %s 后端加载失败: %s", backend, exc)
        raise RuntimeError("；".join(errors))

    def _warmup_inference(self):
        try:
            import numpy as np

            started = time.perf_counter()
            rng = np.random.default_rng(0)
            audio = rng.standard_normal(16_000).astype(np.float32) * 0.002
            self.vad_model.warmup()
            with self.stdout_suppressor.suppress():
                self.session.run(audio, timestamps="none")
            logger.info(
                "Qwen3-ASR 与 FireRedVAD 预热完成，耗时 %.3f 秒",
                time.perf_counter() - started,
            )
        except Exception as exc:
            logger.warning("Qwen3-ASR 预热失败（首次识别可能偏慢）: %s", exc)

    def _filter_speech(self, audio):
        import numpy as np

        started = time.perf_counter()
        chunks = self.vad_model.speech_timestamps(audio)
        vad_ms = (time.perf_counter() - started) * 1000
        self._vad_calls += 1
        self._total_vad_ms += vad_ms

        if not chunks:
            self._vad_rejected += 1
            return np.empty(0, dtype=np.float32), 0, vad_ms

        start = max(0, int(chunks[0]["start"]))
        end = min(len(audio), int(chunks[-1]["end"]))
        if end <= start:
            self._vad_rejected += 1
            return np.empty(0, dtype=np.float32), 0, vad_ms

        # Preserve pauses between spoken regions. Qwen still receives natural
        # phrase timing; only idle time before/after the utterance is removed.
        return np.ascontiguousarray(audio[start:end]), len(chunks), vad_ms

    def _run_inference(self, audio, *, context="", language=None):
        with self.stdout_suppressor.suppress():
            return self.session.run(audio, timestamps="none", context=context, language=language)

    def _transcribe_stream_audio(self, audio, *, context="", language=None):
        # Stream callbacks bypass the offline busy guard. VAD runs once for
        # each preview/final, preserving pauses and skipping silence entirely.
        audio, segments, _ = self._filter_speech(audio)
        if not segments:
            return SimpleNamespace(text="", language=language)
        return self._run_inference(audio, context=context, language=language)

    def initialize(self):
        if self.initialized:
            return {"success": True, "message": "模型已初始化", "engine": self.engine}

        model_path = self._resolve_model_path()
        if not model_path:
            return {
                "success": False,
                "error": f"Qwen3-ASR Q8 模型未下载: {self.model_config['filename']}",
                "type": "models_not_downloaded",
                "engine": self.engine,
            }

        started = time.perf_counter()
        try:
            logger.info("加载 Qwen3-ASR: %s", model_path)
            self._load_runtime(model_path)
            if self.vad_model is None:
                self.vad_model = FireRedVad()
            self._warmup_inference()
            self.stream = R2T2StreamSession(
                QwenPreviewRuntime(self._transcribe_stream_audio)
            )
            self.initialized = True
            self._gpu_suspended = False
            self._last_load_error = None
            elapsed = time.perf_counter() - started
            logger.info("Qwen3-ASR 初始化完成，后端: %s，耗时 %.3f 秒", self.backend, elapsed)
            return {
                "success": True,
                "message": f"Qwen3-ASR 初始化成功，耗时: {elapsed:.2f}秒",
                "model_loaded": True,
                "engine": self.engine,
                "backend": self.backend,
                "native_streaming": self.stream is not None,
                **self._get_gpu_device_info(),
            }
        except ImportError as exc:
            self._close_runtime()
            self.vad_model = None
            self._last_load_error = str(exc)
            logger.error("Qwen3-ASR 依赖加载失败: %s", exc)
            logger.error(traceback.format_exc())
            return {
                "success": False,
                "error": f"Qwen3-ASR 依赖加载失败: {exc}",
                "type": "import_error",
                "engine": self.engine,
            }
        except Exception as exc:
            self._close_runtime()
            self.vad_model = None
            self._last_load_error = str(exc)
            logger.error("Qwen3-ASR 初始化失败: %s", exc)
            logger.error(traceback.format_exc())
            return {
                "success": False,
                "error": f"Qwen3-ASR 初始化失败: {exc}",
                "type": "init_error",
                "engine": self.engine,
            }

    def _load_audio(self, audio_path, audio_base64, audio_format, sample_rate):
        import numpy as np

        if audio_base64:
            audio, duration = decode_inline_audio(audio_base64, audio_format, sample_rate)
            if not isinstance(audio, np.ndarray):
                raise ValueError("Qwen3-ASR 内存输入仅支持 PCM")
            source_rate = sample_rate or 16_000
            audio = self._resample(audio, source_rate)
            return np.ascontiguousarray(audio, dtype=np.float32), duration, "memory"

        if not audio_path or not os.path.exists(audio_path):
            raise FileNotFoundError(f"音频文件不存在: {audio_path}")
        audio, source_rate = self._read_mono_audio(audio_path)
        audio = self._resample(audio, source_rate)
        return np.ascontiguousarray(audio), len(audio) / 16_000.0, "path"

    def transcribe_audio(
        self,
        audio_path,
        options=None,
        hot_words=None,
        audio_base64=None,
        audio_format=None,
        sample_rate=None,
    ):
        if not self.initialized:
            init_result = self.initialize()
            if not init_result["success"]:
                return init_result
        if self._stream_is_active():
            return self._stream_busy_response()

        input_mode = "memory" if audio_base64 else "path"
        try:
            options = options if isinstance(options, dict) else {}
            context = self._compose_context(
                options.get("context", ""),
                hot_words if hot_words is not None else options.get("hot_words", []),
            )
            language = self._normalize_language(options.get("language"))
            audio, duration, input_mode = self._load_audio(
                audio_path, audio_base64, audio_format, sample_rate
            )
            self.total_audio_duration += duration
            if duration < 0.5:
                return {
                    "success": True,
                    "text": "",
                    "duration": duration,
                    "engine": self.engine,
                    "input_mode": input_mode,
                }

            audio, vad_segments, vad_ms = self._filter_speech(audio)
            speech_duration = len(audio) / float(QWEN3_VAD_SAMPLE_RATE)
            if not vad_segments:
                return {
                    "success": True,
                    "text": "",
                    "raw_text": "",
                    "duration": duration,
                    "speech_duration": 0.0,
                    "language": "unknown",
                    "engine": self.engine,
                    "model_type": self.engine,
                    "backend": self.backend,
                    "input_mode": input_mode,
                    "vad_segments": 0,
                    "vad_ms": round(vad_ms, 3),
                    "inference_ms": 0.0,
                }

            started = time.perf_counter()
            result = self._run_inference(audio, context=context, language=language)
            inference_ms = (time.perf_counter() - started) * 1000
            self._total_inference_ms += inference_ms
            self.transcription_count += 1

            text = result.text.strip()
            language = result.language or "unknown"
            self._maybe_cleanup(duration)
            return {
                "success": True,
                "text": text,
                "raw_text": text,
                "confidence": 0.0,
                "duration": duration,
                "speech_duration": round(speech_duration, 3),
                "language": language,
                "engine": self.engine,
                "model_type": self.engine,
                "backend": self.backend,
                "input_mode": input_mode,
                "vad_segments": vad_segments,
                "vad_ms": round(vad_ms, 3),
                "inference_ms": round(inference_ms, 3),
            }
        except Exception as exc:
            logger.error("Qwen3-ASR 转录失败: %s", exc)
            logger.error(traceback.format_exc())
            return {
                "success": False,
                "error": f"音频转录失败: {exc}",
                "type": "transcription_error",
                "input_mode": input_mode,
            }

    def get_performance_stats(self):
        return {
            "transcription_count": self.transcription_count,
            "total_audio_duration": round(self.total_audio_duration, 2),
            "average_inference_ms": round(
                self._total_inference_ms / max(1, self.transcription_count), 3
            ),
            "average_vad_ms": round(self._total_vad_ms / max(1, self._vad_calls), 3),
            "vad_calls": self._vad_calls,
            "vad_rejected": self._vad_rejected,
            "initialized": self.initialized,
            "engine": self.engine,
            "backend": self.backend,
            "native_streaming": self.initialized and self.stream is not None,
            "speculative_decoding": False,
            "models_loaded": {
                "asr": self.model is not None,
                "vad": self.vad_model is not None,
                "punc": True,
            },
        }

    def check_status(self):
        model_loaded = self.model is not None and self.session is not None
        runtime_root = Path(__file__).resolve().with_name("r2t2-native")
        installed = model_loaded or any(
            (runtime_root / backend / "audiocpp.dll").is_file()
            for backend in ("cuda", "cpu")
        )
        return self._with_gpu_suspend_flag({
            "success": True,
            "installed": installed,
            "initialized": self.initialized,
            "engine": self.engine,
            "backend": self.backend,
            "model_loaded": model_loaded,
            "native_streaming": self.initialized and model_loaded and self.stream is not None,
            "models": {
                "asr": model_loaded,
                "vad": self.vad_model is not None,
                "punc": True,
            },
            **self._get_gpu_device_info(),
        })


if __name__ == "__main__":
    Qwen3ASRServer().run()
