#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""MLX Whisper 本地模型服务器."""

import os
import platform
import subprocess
import sys
import traceback

from server_common import (
    decode_inline_audio,
    apply_hf_env_defaults,
    setup_rotating_logger,
    BaseASRServer,
)

apply_hf_env_defaults()
logger = setup_rotating_logger(__name__, "local_asr_server.log", "本地 MLX ASR 服务器")

from hf_cache_utils import LOCAL_ASR_REPO_ID, LOCAL_MODEL_REPOS


class WhisperServer(BaseASRServer):
    def __init__(self):
        super().__init__(engine="local", logger=logger)
        self.model = None
        self._mlx_module = None
        self._last_load_error = None

    def _get_model_repos(self) -> list:
        return LOCAL_MODEL_REPOS

    def _detect_device(self) -> str:
        if sys.platform == "darwin" and platform.machine() == "arm64":
            logger.info("检测到 Apple Silicon，使用 MLX 本地推理")
            return "apple-silicon"
        logger.warning("当前平台不是 Apple Silicon，本地 MLX ASR 不可用")
        return "unsupported"

    def _get_gpu_device_info(self) -> dict:
        info = {"device": self.device}
        if self.device == "apple-silicon":
            try:
                chip_name = (
                    subprocess.check_output(
                        ["sysctl", "-n", "machdep.cpu.brand_string"],
                        text=True,
                    )
                    .strip()
                )
                if chip_name:
                    info["gpu_name"] = chip_name
            except Exception:
                pass
        return info

    def _load_model(self):
        """预加载 MLX Whisper 模型。"""
        if self.device != "apple-silicon":
            self._last_load_error = "本地 MLX 模型仅支持 Apple Silicon（macOS arm64）"
            logger.error(self._last_load_error)
            return False

        try:
            logger.info("开始加载 MLX Whisper 模型...")
            with self.stdout_suppressor.suppress():
                import mlx.core as mx
                from mlx_whisper.transcribe import ModelHolder

                self.model = ModelHolder.get_model(
                    LOCAL_ASR_REPO_ID,
                    dtype=mx.float16,
                )
                self._mlx_module = mx
            self._last_load_error = None
            logger.info("MLX Whisper 模型加载完成")
            return True
        except Exception as e:
            self._last_load_error = str(e)
            logger.error(f"MLX Whisper 模型加载失败: {e}")
            logger.error(traceback.format_exc())
            return False

    def initialize(self):
        """初始化 MLX Whisper 模型"""
        if self.initialized:
            return {"success": True, "message": "模型已初始化"}

        try:
            import time

            logger.info("正在初始化本地 MLX 模型...")
            start_time = time.time()

            if not self._load_model():
                error_msg = self._last_load_error or "本地 MLX 模型加载失败"
                logger.error(error_msg)
                return {"success": False, "error": error_msg, "type": "init_error"}

            total_time = time.time() - start_time
            self.initialized = True
            logger.info(f"本地 MLX 模型初始化完成，总耗时: {total_time:.2f}秒")
            return {
                "success": True,
                "message": f"本地 MLX 模型初始化成功，耗时: {total_time:.2f}秒",
                "model_loaded": True,
                "engine": self.engine,
                **self._get_gpu_device_info(),
            }

        except ImportError as e:
            error_msg = f"MLX Whisper 依赖加载失败: {e}"
            logger.error(error_msg)
            logger.error(traceback.format_exc())
            return {"success": False, "error": error_msg, "type": "import_error", "engine": self.engine}

        except Exception as e:
            error_msg = f"本地 MLX 模型初始化失败: {e}"
            logger.error(error_msg)
            logger.error(traceback.format_exc())
            return {"success": False, "error": error_msg, "type": "init_error", "engine": self.engine}

    def transcribe_audio(
        self,
        audio_path,
        options=None,
        hot_words=None,
        audio_base64=None,
        audio_format=None,
        sample_rate=None,
    ):
        """转录音频文件"""
        import time

        if not self.initialized:
            init_result = self.initialize()
            if not init_result["success"]:
                return init_result

        try:
            duration = 0.0
            input_mode = "path"
            audio_input = audio_path

            if audio_base64:
                try:
                    audio_input, duration = decode_inline_audio(
                        audio_base64,
                        audio_format,
                        sample_rate,
                    )
                    input_mode = "memory"
                except Exception as e:
                    return {
                        "success": False,
                        "error": f"内存音频解码失败: {e}",
                        "type": "transcription_error",
                        "input_mode": "memory",
                    }
            else:
                # 检查音频文件是否存在
                if not audio_path or not os.path.exists(audio_path):
                    return {
                        "success": False,
                        "error": f"音频文件不存在: {audio_path}",
                        "type": "transcription_error",
                        "input_mode": input_mode,
                    }

            total_start = time.time()
            logger.info(
                "开始转录音频输入: %s",
                "memory-buffer" if input_mode == "memory" else audio_path,
            )

            # 获取音频时长
            if input_mode == "path":
                duration = self._get_audio_duration(audio_path)
            else:
                self.total_audio_duration += duration
            logger.info(f"音频时长: {duration:.2f}秒")

            # 音频过短时跳过转录
            if 0 < duration < 0.5:
                logger.warning(f"音频过短 ({duration:.2f}秒)，跳过转录")
                return {"success": True, "text": "", "duration": duration}

            # 构建 initial_prompt（注入热词作为 Glossary）
            initial_prompt = "Hello, welcome. 你好，欢迎。"
            if hot_words and isinstance(hot_words, list) and len(hot_words) > 0:
                glossary = ", ".join(hot_words[:100])
                initial_prompt = f"Glossary: {glossary}. {initial_prompt}"
                logger.info(f"MLX Whisper initial_prompt 注入 {len(hot_words)} 个热词")

            # 执行 MLX Whisper 转录
            asr_start = time.time()
            with self.stdout_suppressor.suppress():
                from mlx_whisper import transcribe

                result = transcribe(
                    audio_input,
                    path_or_hf_repo=LOCAL_ASR_REPO_ID,
                    verbose=None,
                    initial_prompt=initial_prompt,
                    condition_on_previous_text=False,
                    no_speech_threshold=0.6,
                    compression_ratio_threshold=2.4,
                    logprob_threshold=-1.0,
                    fp16=True,
                )
            asr_elapsed = time.time() - asr_start

            final_text = result.get("text", "").strip()
            detected_language = result.get("language") or "unknown"
            segments = result.get("segments") or []
            avg_logprob = 0.0
            if segments:
                logprobs = [
                    segment.get("avg_logprob")
                    for segment in segments
                    if isinstance(segment, dict) and isinstance(segment.get("avg_logprob"), (int, float))
                ]
                if logprobs:
                    avg_logprob = sum(logprobs) / len(logprobs)

            logger.info(
                "本地 MLX 识别完成，耗时: %.2f秒，语言: %s，文本: %s...",
                asr_elapsed,
                detected_language,
                final_text[:100],
            )

            self.transcription_count += 1
            total_elapsed = time.time() - total_start
            logger.info(f"转录完成，总耗时: {total_elapsed:.2f}秒，最终文本: {final_text[:100]}...")

            result = {
                "success": True,
                "text": final_text,
                "raw_text": final_text,
                "confidence": avg_logprob,
                "duration": duration,
                "language": detected_language,
                "model_type": "mlx-whisper",
                "input_mode": input_mode,
            }

            self._maybe_cleanup(duration)
            return result

        except Exception as e:
            error_msg = f"音频转录失败: {e}"
            logger.error(error_msg)
            logger.error(traceback.format_exc())
            return {
                "success": False,
                "error": error_msg,
                "type": "transcription_error",
                "input_mode": input_mode,
            }

    def get_performance_stats(self):
        return {
            "transcription_count": self.transcription_count,
            "total_audio_duration": round(self.total_audio_duration, 2),
            "average_duration": round(
                self.total_audio_duration / max(1, self.transcription_count), 2
            ),
            "initialized": self.initialized,
            "engine": self.engine,
            "models_loaded": {
                "asr": self.model is not None,
                "vad": True,
                "punc": True,
            },
        }

    def check_status(self):
        try:
            import mlx_whisper

            device_info = self._get_gpu_device_info()
            model_loaded = self.model is not None

            return {
                "success": True,
                "installed": True,
                "initialized": self.initialized,
                "version": getattr(mlx_whisper, "__version__", "unknown"),
                "engine": self.engine,
                "model_loaded": model_loaded,
                "models": {
                    "asr": model_loaded,
                    "vad": True,
                    "punc": True,
                },
                **device_info,
            }
        except ImportError as e:
            error_msg = f"MLX Whisper 依赖加载失败: {e}"
            logger.error(error_msg)
            logger.error(traceback.format_exc())
            return {
                "success": False,
                "installed": False,
                "initialized": False,
                "error": error_msg,
                "engine": self.engine,
            }


if __name__ == "__main__":
    server = WhisperServer()
    server.run()
