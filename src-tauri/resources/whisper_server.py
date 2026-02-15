#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
Faster Whisper 模型服务器
保持模型在内存中，通过stdin/stdout进行通信
"""

import os
import traceback

from server_common import (
    apply_hf_env_defaults,
    setup_rotating_logger,
    BaseASRServer,
)

apply_hf_env_defaults()
logger = setup_rotating_logger(__name__, "whisper_server.log", "Whisper服务器")

from hf_cache_utils import WHISPER_MODEL_REPOS


class WhisperServer(BaseASRServer):
    def __init__(self):
        super().__init__(engine="whisper", logger=logger)
        self.model = None
        self.compute_type = "float16" if self.device == "cuda" else "int8"

    def _get_model_repos(self) -> list:
        return WHISPER_MODEL_REPOS

    def _detect_device(self) -> str:
        """Whisper-specific device detection: also checks CTranslate2 CUDA support."""
        try:
            import ctranslate2
            if "cuda" in ctranslate2.get_supported_compute_types("cuda"):
                try:
                    import torch
                    if torch.cuda.is_available():
                        gpu_name = torch.cuda.get_device_name(0)
                        gpu_mem = torch.cuda.get_device_properties(0).total_memory / (1024**3)
                        logger.info(f"检测到 NVIDIA GPU: {gpu_name} ({gpu_mem:.1f}GB)，使用 CUDA 加速")
                        return "cuda"
                except Exception:
                    pass
                logger.info("CTranslate2 CUDA 可用，使用 CUDA 加速")
                return "cuda"
        except Exception:
            pass

        # Fall back to base detection (PyTorch CUDA check)
        return super()._detect_device()

    def _load_model(self):
        """加载 Faster Whisper 模型"""
        try:
            logger.info(f"开始加载 Faster Whisper 模型 (device={self.device}, compute_type={self.compute_type})...")
            with self.stdout_suppressor.suppress():
                from faster_whisper import WhisperModel

                self.model = WhisperModel(
                    "deepdml/faster-whisper-large-v3-turbo-ct2",
                    device=self.device,
                    compute_type=self.compute_type,
                )
            logger.info(f"Faster Whisper 模型加载完成 (device={self.device})")
            return True
        except Exception as e:
            if self.device == "cuda":
                logger.warning(f"Whisper模型GPU加载失败: {e}，回退到CPU")
                self.device = "cpu"
                self.compute_type = "int8"
                return self._load_model()
            logger.error(f"Whisper模型加载失败: {e}")
            return False

    def initialize(self):
        """初始化 Faster Whisper 模型"""
        if self.initialized:
            return {"success": True, "message": "模型已初始化"}

        try:
            import time

            logger.info("正在初始化Faster Whisper模型...")
            start_time = time.time()

            if not self._load_model():
                error_msg = "Whisper模型加载失败"
                logger.error(error_msg)
                return {"success": False, "error": error_msg, "type": "init_error"}

            total_time = time.time() - start_time
            self.initialized = True
            logger.info(f"Faster Whisper模型初始化完成，总耗时: {total_time:.2f}秒")
            return {
                "success": True,
                "message": f"Faster Whisper模型初始化成功，耗时: {total_time:.2f}秒",
                "model_loaded": True,
                "engine": self.engine,
            }

        except ImportError:
            error_msg = "faster-whisper未安装，请先安装: pip install faster-whisper"
            logger.error(error_msg)
            return {"success": False, "error": error_msg, "type": "import_error", "engine": self.engine}

        except Exception as e:
            error_msg = f"Faster Whisper模型初始化失败: {e}"
            logger.error(error_msg)
            logger.error(traceback.format_exc())
            return {"success": False, "error": error_msg, "type": "init_error", "engine": self.engine}

    def transcribe_audio(self, audio_path, options=None):
        """转录音频文件"""
        import time

        if not self.initialized:
            init_result = self.initialize()
            if not init_result["success"]:
                return init_result

        try:
            duration = 0.0

            # 检查音频文件是否存在
            if not os.path.exists(audio_path):
                return {"success": False, "error": f"音频文件不存在: {audio_path}", "type": "transcription_error"}

            total_start = time.time()
            logger.info(f"开始转录音频文件: {audio_path}")

            # 获取音频时长
            duration = self._get_audio_duration(audio_path)
            logger.info(f"音频时长: {duration:.2f}秒")

            # 音频过短时跳过转录
            if 0 < duration < 0.5:
                logger.warning(f"音频过短 ({duration:.2f}秒)，跳过转录")
                return {"success": True, "text": "", "duration": duration}

            # 执行 Whisper 转录（内置 Silero VAD）
            asr_start = time.time()
            with self.stdout_suppressor.suppress():
                segments, info = self.model.transcribe(
                    audio_path,
                    language=None,
                    initial_prompt="Hello, welcome. 你好，欢迎。",
                    condition_on_previous_text=False,
                    vad_filter=True,
                    vad_parameters={"min_silence_duration_ms": 500},
                )
                text_parts = [segment.text for segment in segments]
            asr_elapsed = time.time() - asr_start

            final_text = "".join(text_parts).strip()
            detected_language = info.language if info else "unknown"

            logger.info(f"Whisper识别完成，耗时: {asr_elapsed:.2f}秒，语言: {detected_language}，文本: {final_text[:100]}...")

            self.transcription_count += 1
            total_elapsed = time.time() - total_start
            logger.info(f"转录完成，总耗时: {total_elapsed:.2f}秒，最终文本: {final_text[:100]}...")

            result = {
                "success": True,
                "text": final_text,
                "raw_text": final_text,
                "confidence": info.language_probability if info else 0.0,
                "duration": duration,
                "language": detected_language,
                "model_type": "ctranslate2",
            }

            self._maybe_cleanup(duration)
            return result

        except Exception as e:
            error_msg = f"音频转录失败: {e}"
            logger.error(error_msg)
            logger.error(traceback.format_exc())
            return {"success": False, "error": error_msg, "type": "transcription_error"}

    def get_performance_stats(self):
        """获取性能统计信息"""
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
                "vad": True,   # Whisper 内置 Silero VAD
                "punc": True,  # Whisper 内置标点
            },
        }

    def check_status(self):
        """检查 Whisper 状态"""
        try:
            import faster_whisper

            device_info = self._get_gpu_device_info()
            model_loaded = self.model is not None

            return {
                "success": True,
                "installed": True,
                "initialized": self.initialized,
                "version": getattr(faster_whisper, "__version__", "unknown"),
                "engine": self.engine,
                "model_loaded": model_loaded,
                "models": {
                    "asr": model_loaded,
                    "vad": True,   # 内置 Silero VAD
                    "punc": True,  # 内置标点
                },
                **device_info,
            }
        except ImportError:
            return {
                "success": False,
                "installed": False,
                "initialized": False,
                "error": "faster-whisper未安装",
            }


if __name__ == "__main__":
    server = WhisperServer()
    server.run()
