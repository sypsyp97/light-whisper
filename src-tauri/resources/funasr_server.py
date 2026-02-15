#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
FunASR模型服务器
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
logger = setup_rotating_logger(__name__, "funasr_server.log", "FunASR服务器")

from hf_cache_utils import MODEL_REPOS

VAD_MAX_SEGMENT_MS = 30000


class FunASRServer(BaseASRServer):
    def __init__(self):
        super().__init__(engine="sensevoice", logger=logger)
        self.asr_model = None

    def _get_model_repos(self) -> list:
        return MODEL_REPOS

    def _load_asr_model(self):
        """加载ASR模型（SenseVoiceSmall + fsmn-vad）"""
        try:
            logger.info(f"开始加载 SenseVoiceSmall 模型 (device={self.device})...")
            with self.stdout_suppressor.suppress():
                from funasr import AutoModel

                self.asr_model = AutoModel(
                    model="FunAudioLLM/SenseVoiceSmall",
                    vad_model="fsmn-vad",
                    vad_kwargs={"max_single_segment_time": VAD_MAX_SEGMENT_MS, "hub": "hf"},
                    hub="hf",
                    disable_update=True,
                    device=self.device,
                )
            logger.info(f"SenseVoiceSmall 模型加载完成 (device={self.device})")
            return True
        except Exception as e:
            if self.device == "cuda":
                logger.warning(f"ASR模型GPU加载失败: {e}，回退到CPU")
                self.device = "cpu"
                return self._load_asr_model()
            logger.error(f"ASR模型加载失败: {e}")
            return False

    def initialize(self):
        """初始化 FunASR 模型"""
        if self.initialized:
            return {"success": True, "message": "模型已初始化"}

        try:
            import time

            logger.info("正在初始化FunASR模型...")
            start_time = time.time()

            if not self._load_asr_model():
                error_msg = "ASR模型加载失败"
                logger.error(error_msg)
                return {"success": False, "error": error_msg, "type": "init_error"}

            total_time = time.time() - start_time
            self.initialized = True
            logger.info(f"FunASR模型初始化完成，总耗时: {total_time:.2f}秒")
            return {
                "success": True,
                "message": f"FunASR模型初始化成功，耗时: {total_time:.2f}秒",
                "model_loaded": True,
                "engine": self.engine,
            }

        except ImportError:
            error_msg = "FunASR未安装，请先安装FunASR: pip install funasr"
            logger.error(error_msg)
            return {"success": False, "error": error_msg, "type": "import_error", "engine": self.engine}

        except Exception as e:
            error_msg = f"FunASR模型初始化失败: {e}"
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

            # 预先获取音频时长，用于决定是否跳过 VAD
            duration = self._get_audio_duration(audio_path)
            logger.info(f"音频时长: {duration:.2f}秒")

            # 音频过短时 VAD 检测不到语音，会导致空张量索引错误
            if 0 < duration < 0.5:
                logger.warning(f"音频过短 ({duration:.2f}秒)，跳过转录")
                return {"success": True, "text": "", "duration": duration}

            # 执行ASR识别（SenseVoiceSmall 内置 ITN 标点恢复）
            asr_start = time.time()
            import torch
            with self.stdout_suppressor.suppress(), torch.inference_mode():
                asr_result = self.asr_model.generate(
                    input=audio_path,
                    cache={},
                    language="auto",
                    use_itn=True,
                    batch_size_s=60,
                    merge_vad=True,
                    merge_length_s=15,
                )
            asr_elapsed = time.time() - asr_start

            # 提取识别文本并进行富文本后处理（去除 <|zh|><|NEUTRAL|> 等标签）
            from funasr.utils.postprocess_utils import rich_transcription_postprocess
            if isinstance(asr_result, list) and len(asr_result) > 0:
                if isinstance(asr_result[0], dict) and "text" in asr_result[0]:
                    raw_text = asr_result[0]["text"]
                    final_text = rich_transcription_postprocess(raw_text)
                else:
                    final_text = str(asr_result[0])
            else:
                final_text = str(asr_result)

            logger.info(f"ASR识别完成，耗时: {asr_elapsed:.2f}秒，文本: {final_text[:100]}...")

            self.transcription_count += 1
            total_elapsed = time.time() - total_start
            logger.info(f"转录完成，总耗时: {total_elapsed:.2f}秒，最终文本: {final_text[:100]}...")

            result = {
                "success": True,
                "text": final_text,
                "raw_text": final_text,
                "confidence": (
                    getattr(asr_result[0], "confidence", 0.0)
                    if isinstance(asr_result, list)
                    else 0.0
                ),
                "duration": duration,
                "language": "zh-CN",
                "model_type": "pytorch",
            }

            self._maybe_cleanup(duration)
            return result

        except (IndexError, RuntimeError) as e:
            err_str = str(e)
            if "index" in err_str and "out of bounds" in err_str or "size 0" in err_str:
                logger.warning(f"音频中未检测到有效语音: {err_str}")
                return {"success": True, "text": "", "duration": duration}
            error_msg = f"音频转录失败: {err_str}"
            logger.error(error_msg)
            logger.error(traceback.format_exc())
            return {"success": False, "error": error_msg, "type": "transcription_error"}
        except Exception as e:
            error_msg = f"音频转录失败: {e}"
            logger.error(error_msg)
            logger.error(traceback.format_exc())
            return {"success": False, "error": error_msg, "type": "transcription_error"}

    def get_performance_stats(self):
        """获取性能统计信息"""
        models_loaded = {
            "asr": self.asr_model is not None,
            "vad": self.asr_model is not None,
            "punc": self.asr_model is not None,
        }
        return {
            "transcription_count": self.transcription_count,
            "total_audio_duration": round(self.total_audio_duration, 2),
            "average_duration": round(
                self.total_audio_duration / max(1, self.transcription_count), 2
            ),
            "initialized": self.initialized,
            "engine": self.engine,
            "models_loaded": models_loaded,
        }

    def check_status(self):
        """检查FunASR状态"""
        try:
            import funasr

            device_info = self._get_gpu_device_info()
            models = {
                "asr": self.asr_model is not None,
                "vad": self.asr_model is not None,
                "punc": self.asr_model is not None,
            }
            model_loaded = self.asr_model is not None

            return {
                "success": True,
                "installed": True,
                "initialized": self.initialized,
                "version": getattr(funasr, "__version__", "unknown"),
                "engine": self.engine,
                "model_loaded": model_loaded,
                "models": models,
                **device_info,
            }
        except ImportError:
            return {
                "success": False,
                "installed": False,
                "initialized": False,
                "error": "FunASR未安装",
            }


if __name__ == "__main__":
    server = FunASRServer()
    server.run()
