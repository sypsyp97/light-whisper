#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
Faster Whisper 模型服务器
保持模型在内存中，通过stdin/stdout进行通信
"""

import sys
import json
import os
import logging
import traceback
import signal
import contextlib
import threading

# 防止 HuggingFace Hub 在 Windows 上的 symlink 警告和缓存校验卡顿
os.environ.setdefault("HF_HUB_DISABLE_SYMLINKS_WARNING", "1")
os.environ.setdefault("HF_HUB_ETAG_TIMEOUT", "10")

# 设置日志
import tempfile


# 获取日志文件路径
def get_log_path():
    # 尝试从环境变量获取用户数据目录
    if "LIGHT_WHISPER_DATA_DIR" in os.environ:
        log_dir = os.path.join(os.environ["LIGHT_WHISPER_DATA_DIR"], "logs")
    else:
        # 回退到临时目录
        log_dir = os.path.join(tempfile.gettempdir(), "light_whisper_logs")

    # 确保日志目录存在
    os.makedirs(log_dir, exist_ok=True)
    return os.path.join(log_dir, "whisper_server.log")


log_file_path = get_log_path()

from logging.handlers import RotatingFileHandler

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s - %(levelname)s - %(message)s",
    handlers=[
        RotatingFileHandler(
            log_file_path,
            encoding="utf-8",
            maxBytes=5 * 1024 * 1024,  # 5MB
            backupCount=3,
        ),
        logging.StreamHandler(sys.stderr),  # 显式输出到 stderr，避免干扰 stdout IPC
    ],
)
logger = logging.getLogger(__name__)

# 记录日志文件位置
logger.info(f"Whisper服务器日志文件: {log_file_path}")

_stdout_lock = threading.Lock()
_stdout_refcount = 0
_stdout_original = None
_stdout_devnull = None


@contextlib.contextmanager
def suppress_stdout():
    """上下文管理器：临时重定向stdout到devnull，避免库的非JSON输出干扰IPC通信"""
    global _stdout_refcount, _stdout_original, _stdout_devnull
    with _stdout_lock:
        if _stdout_refcount == 0:
            _stdout_original = sys.stdout
            _stdout_devnull = open(os.devnull, "w")
            sys.stdout = _stdout_devnull
        _stdout_refcount += 1
    try:
        yield
    finally:
        with _stdout_lock:
            _stdout_refcount -= 1
            if _stdout_refcount <= 0:
                sys.stdout = _stdout_original
                if _stdout_devnull:
                    _stdout_devnull.close()
                _stdout_devnull = None
                _stdout_original = None


from hf_cache_utils import get_hf_cache_root, is_hf_repo_ready, WHISPER_MODEL_REPOS

CLEANUP_EVERY_N = 5


class WhisperServer:
    def __init__(self):
        self.model = None
        self.initialized = False
        self.running = True
        self.transcription_count = 0
        self.total_audio_duration = 0.0
        self.engine = "whisper"

        # 自动检测推理设备：优先 GPU，不可用时回退 CPU
        self.device = self._detect_device()
        self.compute_type = "float16" if self.device == "cuda" else "int8"

        signal.signal(signal.SIGTERM, self._signal_handler)
        signal.signal(signal.SIGINT, self._signal_handler)
        self._setup_runtime_environment()

    def _detect_device(self):
        """检测可用的推理设备，优先使用 GPU"""
        try:
            import ctranslate2
            if "cuda" in ctranslate2.get_supported_compute_types("cuda"):
                # CTranslate2 CUDA 可用，再验证 torch CUDA
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

        try:
            import torch
            if torch.cuda.is_available():
                gpu_name = torch.cuda.get_device_name(0)
                gpu_mem = torch.cuda.get_device_properties(0).total_memory / (1024**3)
                logger.info(f"检测到 NVIDIA GPU: {gpu_name} ({gpu_mem:.1f}GB)，使用 CUDA 加速")
                return "cuda"
            else:
                logger.info("CUDA 不可用，使用 CPU 推理")
                return "cpu"
        except ImportError:
            logger.info("PyTorch 未安装，使用 CPU 推理")
            return "cpu"
        except Exception as e:
            logger.warning(f"GPU 检测失败: {e}，回退到 CPU 推理")
            return "cpu"

    def _setup_runtime_environment(self):
        """设置运行时环境变量以优化性能"""
        try:
            # 强制 HuggingFace Hub 离线模式
            os.environ["HF_HUB_OFFLINE"] = "1"
            # 自适应线程数
            cpu_count = os.cpu_count() or 4
            thread_count = max(4, cpu_count - 2)
            os.environ["OMP_NUM_THREADS"] = str(thread_count)
            logger.info(f"运行时环境变量设置完成，HF_HUB_OFFLINE=1, OMP_NUM_THREADS={thread_count} (CPU核心数: {cpu_count})")
        except Exception as e:
            logger.warning(f"环境设置失败: {str(e)}")

    def _signal_handler(self, signum, frame):
        """处理退出信号"""
        logger.info(f"收到信号 {signum}，准备退出...")
        self.running = False

    def _load_model(self):
        """加载 Faster Whisper 模型"""
        try:
            logger.info(f"开始加载 Faster Whisper 模型 (device={self.device}, compute_type={self.compute_type})...")
            with suppress_stdout():
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
            logger.error(f"Whisper模型加载失败: {str(e)}")
            return False

    def initialize(self):
        """初始化 Faster Whisper 模型"""
        if self.initialized:
            return {"success": True, "message": "模型已初始化"}

        try:
            import time

            logger.info("正在初始化Faster Whisper模型...")
            start_time = time.time()

            ok = self._load_model()
            if not ok:
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

        except ImportError as e:
            error_msg = "faster-whisper未安装，请先安装: pip install faster-whisper"
            logger.error(error_msg)
            return {"success": False, "error": error_msg, "type": "import_error", "engine": self.engine}

        except Exception as e:
            error_msg = f"Faster Whisper模型初始化失败: {str(e)}"
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
            with suppress_stdout():
                segments, info = self.model.transcribe(
                    audio_path,
                    language=None,
                    initial_prompt="Hello, welcome. 你好，欢迎。",
                    condition_on_previous_text=False,
                    vad_filter=True,
                    vad_parameters={"min_silence_duration_ms": 500},
                )
                # segments 是生成器，需要遍历收集结果
                text_parts = []
                for segment in segments:
                    text_parts.append(segment.text)
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

            # 定期内存清理
            if self.transcription_count % CLEANUP_EVERY_N == 0 or duration > 120:
                self._cleanup_memory()
                logger.info(f"已完成 {self.transcription_count} 次转录，执行内存清理")

            return result

        except Exception as e:
            error_msg = f"音频转录失败: {str(e)}"
            logger.error(error_msg)
            logger.error(traceback.format_exc())
            return {"success": False, "error": error_msg, "type": "transcription_error"}

    def _get_audio_duration(self, audio_path):
        """从 WAV header 快速获取音频时长"""
        try:
            import struct
            with open(audio_path, "rb") as f:
                riff = f.read(4)
                if riff != b"RIFF":
                    raise ValueError("非 WAV 格式")
                f.seek(28)
                byte_rate = struct.unpack("<I", f.read(4))[0]
                f.seek(40)
                data_size = struct.unpack("<I", f.read(4))[0]
            if byte_rate <= 0:
                raise ValueError(f"无效的 byte rate: {byte_rate}")
            duration = data_size / byte_rate
            self.total_audio_duration += duration
            return duration
        except Exception as e:
            logger.warning(f"从 WAV header 获取音频时长失败: {e}")
            return 0.0

    def _cleanup_memory(self):
        """内存清理"""
        try:
            import gc
            gc.collect()
            if self.device == "cuda":
                import torch
                torch.cuda.empty_cache()
            logger.info("内存清理完成")
        except Exception as e:
            logger.warning(f"内存清理失败: {str(e)}")

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
                "vad": True,  # Whisper 内置 Silero VAD
                "punc": True,  # Whisper 内置标点
            },
        }

    def check_status(self):
        """检查 Whisper 状态"""
        try:
            import faster_whisper

            # 获取 GPU 详情
            device_info = {"device": self.device}
            if self.device == "cuda":
                try:
                    import torch
                    device_info["gpu_name"] = torch.cuda.get_device_name(0)
                    device_info["gpu_memory_total"] = round(torch.cuda.get_device_properties(0).total_memory / (1024**3), 1)
                except Exception:
                    pass

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
                    "vad": True,  # 内置 Silero VAD
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

    def run(self):
        """运行服务器主循环"""
        logger.info("Whisper服务器启动")

        hf_cache = get_hf_cache_root()
        logger.info(f"HuggingFace 缓存根目录: {hf_cache}")

        # 检查模型是否已缓存
        missing = [r for r in WHISPER_MODEL_REPOS if not is_hf_repo_ready(r)]

        if not missing:
            logger.info("模型文件存在，开始初始化")
            init_result = self.initialize()
        else:
            logger.info(f"模型文件不存在或不完整：{', '.join(missing)}，跳过初始化")
            init_result = {
                "success": False,
                "error": "模型文件未下载，请先下载模型",
                "type": "models_not_downloaded",
                "engine": self.engine
            }
        print(json.dumps(init_result, ensure_ascii=False))
        sys.stdout.flush()

        while self.running:
            try:
                # 读取命令
                line = sys.stdin.readline()
                if not line:
                    break

                line = line.strip()
                if not line:
                    continue

                try:
                    command = json.loads(line)
                except json.JSONDecodeError:
                    result = {"success": False, "error": "无效的JSON命令"}
                    print(json.dumps(result, ensure_ascii=False))
                    sys.stdout.flush()
                    continue

                # 处理命令
                if command.get("action") == "transcribe":
                    audio_path = command.get("audio_path")
                    options = command.get("options", {})
                    result = self.transcribe_audio(audio_path, options)
                elif command.get("action") == "status":
                    result = self.check_status()
                elif command.get("action") == "stats":
                    result = {"success": True, "stats": self.get_performance_stats()}
                elif command.get("action") == "cleanup":
                    self._cleanup_memory()
                    result = {"success": True, "message": "内存清理完成"}
                elif command.get("action") == "exit":
                    result = {"success": True, "message": "服务器退出"}
                    print(json.dumps(result, ensure_ascii=False))
                    sys.stdout.flush()
                    break
                else:
                    result = {
                        "success": False,
                        "error": f"未知命令: {command.get('action')}",
                    }

                # 输出结果
                print(json.dumps(result, ensure_ascii=False))
                sys.stdout.flush()

            except KeyboardInterrupt:
                break
            except Exception as e:
                error_result = {
                    "success": False,
                    "error": str(e),
                    "traceback": traceback.format_exc(),
                }
                print(json.dumps(error_result, ensure_ascii=False))
                sys.stdout.flush()

        logger.info("Whisper服务器退出")

if __name__ == "__main__":
    server = WhisperServer()
    server.run()
