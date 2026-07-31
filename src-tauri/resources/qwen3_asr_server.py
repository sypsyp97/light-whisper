#!/usr/bin/env python3
# -*- coding: utf-8 -*-

"""Low-latency local Qwen3-ASR GGUF server backed by transcribe.cpp."""

import importlib.metadata
import os
import subprocess
import time
import traceback

from firered_vad import FireRedVad

from server_common import (
    BaseASRServer,
    _has_nvidia_gpu,
    apply_hf_env_defaults,
    decode_inline_audio,
    ensure_safe_cuda_env,
    setup_rotating_logger,
)

apply_hf_env_defaults()
ensure_safe_cuda_env()
logger = setup_rotating_logger(__name__, "qwen3_asr_server.log", "Qwen3-ASR服务器")

from hf_cache_utils import QWEN3_ASR_MODELS, find_hf_snapshot_file

QWEN3_ASR_N_CTX = 32_768
QWEN3_VAD_SAMPLE_RATE = 16_000


class Qwen3ASRServer(BaseASRServer):
    """Keep one GGUF model and one KV session resident across requests."""

    def __init__(self, engine=None):
        engine = engine or os.environ.get("LIGHT_WHISPER_ASR_ENGINE", "qwen3-asr-0.6b")
        if engine not in QWEN3_ASR_MODELS:
            raise ValueError(f"不支持的 Qwen3-ASR 引擎: {engine}")
        self.model_config = QWEN3_ASR_MODELS[engine]
        super().__init__(engine=engine, logger=logger)
        self.model = None
        self.session = None
        self.vad_model = None
        self.backend = "cuda" if self.device == "cuda" else "auto"
        self._last_load_error = None
        self._total_inference_ms = 0.0
        self._total_vad_ms = 0.0
        self._vad_calls = 0
        self._vad_rejected = 0

    def _detect_device(self):
        """Avoid importing the 2 GB PyTorch runtime just to probe one CUDA driver."""
        if _has_nvidia_gpu():
            logger.info("检测到 NVIDIA CUDA 驱动，Qwen3-ASR 优先使用 CUDA")
            return "cuda"
        logger.info("未检测到 NVIDIA CUDA 驱动，Qwen3-ASR 自动选择 Vulkan/CPU")
        return "cpu"

    def _get_gpu_device_info(self):
        info = {"device": self.device}
        if self.backend != "cuda":
            return info
        query = subprocess.run(
            [
                "nvidia-smi",
                "--query-gpu=name,memory.total",
                "--format=csv,noheader,nounits",
            ],
            capture_output=True,
            text=True,
            check=False,
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
        )
        try:
            name, memory_mb = [part.strip() for part in query.stdout.splitlines()[0].split(",", 1)]
            info["gpu_name"] = name
            info["gpu_memory_total"] = round(float(memory_mb) / 1024, 1)
        except (IndexError, ValueError):
            pass
        return info

    def _cleanup_memory(self):
        # transcribe.cpp owns its CUDA allocations; torch.empty_cache() cannot
        # release them and importing torch here would add substantial overhead.
        import gc

        gc.collect()

    def _get_model_repos(self):
        # Exact-file validation happens in initialize(); a repository may contain
        # Q4/Q6 files that must not be mistaken for the selected Q8 model.
        return []

    def _resolve_model_path(self):
        return find_hf_snapshot_file(
            self.model_config["repo_id"], self.model_config["filename"]
        )

    def _close_runtime(self):
        if self.session is not None:
            try:
                self.session.close()
            except Exception:
                pass
            self.session = None
        if self.model is not None:
            try:
                self.model.close()
            except Exception:
                pass
            self.model = None

    def _load_runtime(self, model_path):
        import transcribe_cpp

        preferred = ["cuda", "vulkan", "cpu"] if self.device == "cuda" else ["auto", "cpu"]
        errors = []
        for backend in preferred:
            try:
                with self.stdout_suppressor.suppress():
                    model = transcribe_cpp.Model(model_path, backend=backend)
                    session = model.session(kv_type="f16", n_ctx=QWEN3_ASR_N_CTX)
                self.model = model
                self.session = session
                self.backend = backend
                self.device = "cuda" if backend == "cuda" else backend
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
            self.vad_model = FireRedVad()
            self._warmup_inference()
            self.initialized = True
            self._last_load_error = None
            elapsed = time.perf_counter() - started
            return {
                "success": True,
                "message": f"Qwen3-ASR 初始化成功，耗时: {elapsed:.2f}秒",
                "model_loaded": True,
                "engine": self.engine,
                "backend": self.backend,
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

    @staticmethod
    def _resample(audio, source_rate):
        if source_rate == 16_000:
            return audio
        import numpy as np

        target_length = int(round(len(audio) * 16_000 / source_rate))
        if target_length <= 0:
            return np.empty(0, dtype=np.float32)
        return np.interp(
            np.linspace(0, max(0, len(audio) - 1), target_length),
            np.arange(len(audio), dtype=np.float64),
            audio,
        ).astype(np.float32)

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
        import soundfile as sf

        audio, source_rate = sf.read(
            audio_path,
            dtype="float32",
            always_2d=True,
        )
        audio = audio.mean(axis=1, dtype=np.float32)
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

        input_mode = "memory" if audio_base64 else "path"
        try:
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
            with self.stdout_suppressor.suppress():
                # Qwen3-ASR does not advertise speculative decoding. Keeping a
                # persistent KV session is the supported low-latency path.
                result = self.session.run(audio, timestamps="none")
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
            "speculative_decoding": False,
            "models_loaded": {
                "asr": self.model is not None,
                "vad": self.vad_model is not None,
                "punc": True,
            },
        }

    def check_status(self):
        try:
            version = importlib.metadata.version("transcribe-cpp")
            model_loaded = self.model is not None and self.session is not None
            return {
                "success": True,
                "installed": True,
                "initialized": self.initialized,
                "version": version,
                "engine": self.engine,
                "backend": self.backend,
                "model_loaded": model_loaded,
                "models": {
                    "asr": model_loaded,
                    "vad": self.vad_model is not None,
                    "punc": True,
                },
                **self._get_gpu_device_info(),
            }
        except importlib.metadata.PackageNotFoundError as exc:
            return {
                "success": False,
                "installed": False,
                "initialized": False,
                "error": f"Qwen3-ASR 依赖加载失败: {exc}",
                "engine": self.engine,
            }


if __name__ == "__main__":
    Qwen3ASRServer().run()
