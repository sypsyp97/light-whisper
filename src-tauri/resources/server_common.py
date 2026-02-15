#!/usr/bin/env python3
# -*- coding: utf-8 -*-

"""Python ASR server shared utilities."""

import contextlib
import logging
import os
import struct
import sys
import tempfile
import threading
from logging.handlers import RotatingFileHandler


def apply_hf_env_defaults() -> None:
    """Apply safe default HF env flags for Windows/offline-first runtime."""
    os.environ.setdefault("HF_HUB_DISABLE_SYMLINKS_WARNING", "1")
    os.environ.setdefault("HF_HUB_ETAG_TIMEOUT", "10")


def get_log_path(log_filename: str) -> str:
    """Resolve the server log path under app data dir or temp fallback."""
    if "LIGHT_WHISPER_DATA_DIR" in os.environ:
        log_dir = os.path.join(os.environ["LIGHT_WHISPER_DATA_DIR"], "logs")
    else:
        log_dir = os.path.join(tempfile.gettempdir(), "light_whisper_logs")

    os.makedirs(log_dir, exist_ok=True)
    return os.path.join(log_dir, log_filename)


def setup_rotating_logger(module_name: str, log_filename: str, service_name: str) -> logging.Logger:
    """Initialize a rotating file logger + stderr stream logger."""
    log_file_path = get_log_path(log_filename)
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
            logging.StreamHandler(sys.stderr),
        ],
    )
    logger = logging.getLogger(module_name)
    logger.info(f"{service_name}日志文件: {log_file_path}")
    return logger


class StdoutSuppressor:
    """Reference-counted stdout suppressor for noisy third-party libs."""

    def __init__(self) -> None:
        self._lock = threading.Lock()
        self._refcount = 0
        self._stdout_original = None
        self._stdout_devnull = None

    @contextlib.contextmanager
    def suppress(self):
        with self._lock:
            if self._refcount == 0:
                self._stdout_original = sys.stdout
                self._stdout_devnull = open(os.devnull, "w")
                sys.stdout = self._stdout_devnull
            self._refcount += 1
        try:
            yield
        finally:
            with self._lock:
                self._refcount -= 1
                if self._refcount <= 0:
                    sys.stdout = self._stdout_original
                    if self._stdout_devnull:
                        self._stdout_devnull.close()
                    self._stdout_devnull = None
                    self._stdout_original = None


def get_wav_duration_seconds(audio_path: str, logger: logging.Logger) -> float:
    """Read WAV duration from header quickly without decoding full audio."""
    try:
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
        return data_size / byte_rate
    except Exception as e:
        logger.warning(f"从 WAV header 获取音频时长失败: {e}")
        return 0.0


import json
import signal
import traceback


CLEANUP_EVERY_N = 5


class BaseASRServer:
    """Base class for ASR server implementations.

    Subclasses must implement:
      - engine (str attribute)
      - initialize() -> dict
      - check_status() -> dict
      - get_performance_stats() -> dict
      - transcribe_audio(audio_path, options=None) -> dict
      - _get_model_repos() -> list[str]  (repo IDs to check before init)
    """

    def __init__(self, engine: str, logger: logging.Logger) -> None:
        self.engine = engine
        self.logger = logger
        self.initialized = False
        self.running = True
        self.transcription_count = 0
        self.total_audio_duration = 0.0
        self.device = self._detect_device()
        self.stdout_suppressor = StdoutSuppressor()

        signal.signal(signal.SIGTERM, self._signal_handler)
        signal.signal(signal.SIGINT, self._signal_handler)
        self._setup_runtime_environment()

    # ------------------------------------------------------------------
    # Shared helpers
    # ------------------------------------------------------------------

    def _detect_device(self) -> str:
        """Detect inference device. Override for engine-specific detection."""
        try:
            import torch
            if torch.cuda.is_available():
                gpu_name = torch.cuda.get_device_name(0)
                gpu_mem = torch.cuda.get_device_properties(0).total_memory / (1024**3)
                self.logger.info(f"检测到 NVIDIA GPU: {gpu_name} ({gpu_mem:.1f}GB)，使用 CUDA 加速")
                return "cuda"
            else:
                self.logger.info("CUDA 不可用，使用 CPU 推理")
                return "cpu"
        except ImportError:
            self.logger.info("PyTorch 未安装，使用 CPU 推理")
            return "cpu"
        except Exception as e:
            self.logger.warning(f"GPU 检测失败: {e}，回退到 CPU 推理")
            return "cpu"

    def _setup_runtime_environment(self) -> None:
        try:
            os.environ["HF_HUB_OFFLINE"] = "1"
            cpu_count = os.cpu_count() or 4
            thread_count = max(4, cpu_count - 2)
            os.environ["OMP_NUM_THREADS"] = str(thread_count)
            self.logger.info(
                f"运行时环境变量设置完成，HF_HUB_OFFLINE=1, OMP_NUM_THREADS={thread_count} (CPU核心数: {cpu_count})"
            )
        except Exception as e:
            self.logger.warning(f"环境设置失败: {e}")

    def _signal_handler(self, signum, frame) -> None:
        self.logger.info(f"收到信号 {signum}，准备退出...")
        self.running = False

    def _get_audio_duration(self, audio_path: str) -> float:
        duration = get_wav_duration_seconds(audio_path, self.logger)
        self.total_audio_duration += duration
        return duration

    def _cleanup_memory(self) -> None:
        try:
            import gc
            gc.collect()
            if self.device == "cuda":
                import torch
                torch.cuda.empty_cache()
            self.logger.info("内存清理完成")
        except Exception as e:
            self.logger.warning(f"内存清理失败: {e}")

    def _maybe_cleanup(self, duration: float) -> None:
        """Run periodic memory cleanup after transcription."""
        if self.transcription_count % CLEANUP_EVERY_N == 0 or duration > 120:
            self._cleanup_memory()
            self.logger.info(f"已完成 {self.transcription_count} 次转录，执行内存清理")

    def _get_gpu_device_info(self) -> dict:
        """Return device/gpu_name/gpu_memory_total dict for status responses."""
        info = {"device": self.device}
        if self.device == "cuda":
            try:
                import torch
                info["gpu_name"] = torch.cuda.get_device_name(0)
                info["gpu_memory_total"] = round(
                    torch.cuda.get_device_properties(0).total_memory / (1024**3), 1
                )
            except Exception:
                pass
        return info

    # ------------------------------------------------------------------
    # Hooks for subclasses
    # ------------------------------------------------------------------

    def _get_model_repos(self) -> list:
        """Return list of HF repo IDs to check before auto-init."""
        raise NotImplementedError

    def initialize(self) -> dict:
        raise NotImplementedError

    def check_status(self) -> dict:
        raise NotImplementedError

    def get_performance_stats(self) -> dict:
        raise NotImplementedError

    def transcribe_audio(self, audio_path: str, options=None) -> dict:
        raise NotImplementedError

    # ------------------------------------------------------------------
    # Command dispatch loop
    # ------------------------------------------------------------------

    def run(self) -> None:
        self.logger.info(f"{self.engine} 服务器启动")

        from hf_cache_utils import get_hf_cache_root, is_hf_repo_ready

        hf_cache = get_hf_cache_root()
        self.logger.info(f"HuggingFace 缓存根目录: {hf_cache}")

        model_repos = self._get_model_repos()
        missing = [r for r in model_repos if not is_hf_repo_ready(r)]

        if not missing:
            self.logger.info("模型文件存在，开始初始化")
            init_result = self.initialize()
        else:
            self.logger.info(f"模型文件不存在或不完整：{', '.join(missing)}，跳过初始化")
            init_result = {
                "success": False,
                "error": "模型文件未下载，请先下载模型",
                "type": "models_not_downloaded",
                "engine": self.engine,
            }

        print(json.dumps(init_result, ensure_ascii=False))
        sys.stdout.flush()

        while self.running:
            try:
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

                action = command.get("action")
                if action == "transcribe":
                    result = self.transcribe_audio(
                        command.get("audio_path"),
                        command.get("options", {}),
                    )
                elif action == "status":
                    result = self.check_status()
                elif action == "stats":
                    result = {"success": True, "stats": self.get_performance_stats()}
                elif action == "cleanup":
                    self._cleanup_memory()
                    result = {"success": True, "message": "内存清理完成"}
                elif action == "exit":
                    result = {"success": True, "message": "服务器退出"}
                    print(json.dumps(result, ensure_ascii=False))
                    sys.stdout.flush()
                    break
                else:
                    result = {"success": False, "error": f"未知命令: {action}"}

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

        self.logger.info(f"{self.engine} 服务器退出")
