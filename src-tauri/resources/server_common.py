#!/usr/bin/env python3
# -*- coding: utf-8 -*-

"""Python ASR server shared utilities."""

import base64
import contextlib
import io
import json
import logging
import os
import signal
import subprocess
import sys
import tempfile
import threading
import time
import traceback
from typing import Optional, Tuple
from logging.handlers import RotatingFileHandler


def _has_nvidia_gpu() -> bool:
    """Probe the NVIDIA driver; native model loading validates CUDA availability."""
    if sys.platform != "win32":
        # On Linux, just check for /dev/nvidia*
        import glob as _glob
        return bool(_glob.glob("/dev/nvidia[0-9]*"))
    try:
        import ctypes
        ctypes.WinDLL("nvcuda.dll")
        return True
    except OSError:
        return False


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


def decode_inline_audio(
    audio_base64: str,
    audio_format: Optional[str],
    sample_rate: Optional[int],
) -> Tuple["object", float]:
    """Decode inline audio payload into an in-memory input accepted by ASR backends."""
    if not audio_base64:
        raise ValueError("缺少内存音频数据")

    try:
        audio_bytes = base64.b64decode(audio_base64, validate=True)
    except (ValueError, base64.binascii.Error) as exc:
        raise ValueError(f"音频 base64 解码失败: {exc}") from exc
    fmt = (audio_format or "pcm_s16le").lower()

    if fmt == "pcm_s16le":
        if not sample_rate or sample_rate <= 0:
            raise ValueError("PCM 内存音频缺少有效采样率")
        if len(audio_bytes) % 2 != 0:
            raise ValueError("PCM s16le 数据字节数必须为偶数")
        import numpy as np

        samples = np.frombuffer(audio_bytes, dtype="<i2")
        duration = len(samples) / float(sample_rate)
        audio = samples.astype(np.float32) / 32768.0
        return audio, duration

    if fmt == "wav":
        if not sample_rate or sample_rate <= 0:
            sample_rate = 16000
        duration = 0.0
        try:
            with io.BytesIO(audio_bytes) as wav_buffer:
                with contextlib.closing(wave_open(wav_buffer)) as wav_reader:
                    frame_rate = wav_reader.getframerate()
                    frame_count = wav_reader.getnframes()
                    if frame_rate > 0:
                        duration = frame_count / float(frame_rate)
        except Exception:
            duration = 0.0
        return io.BytesIO(audio_bytes), duration

    raise ValueError(f"不支持的内存音频格式: {fmt}")


def wave_open(buffer: io.BytesIO):
    import wave

    buffer.seek(0)
    return wave.open(buffer, "rb")


CLEANUP_EVERY_N = 20


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
        # 0 = 关闭。未开启时不启动计时器，命令路径与以前相同。
        self._gpu_idle_seconds = 0
        self._gpu_idle_lock = threading.Lock()
        self._gpu_idle_stop = None
        self._gpu_idle_thread = None
        self._last_activity_at = None
        self._gpu_suspended = False

        signal.signal(signal.SIGTERM, self._signal_handler)
        signal.signal(signal.SIGINT, self._signal_handler)
        self._setup_runtime_environment()

    # ------------------------------------------------------------------
    # Shared helpers
    # ------------------------------------------------------------------

    def _detect_device(self) -> str:
        return "cuda" if _has_nvidia_gpu() else "cpu"

    def _setup_runtime_environment(self) -> None:
        try:
            os.environ["HF_HUB_OFFLINE"] = "1"
            cpu_count = os.cpu_count() or 1
            thread_count = os.environ.setdefault(
                "OMP_NUM_THREADS", str(max(1, cpu_count - 2))
            )
            self.logger.info(
                f"运行时环境变量设置完成，HF_HUB_OFFLINE=1, OMP_NUM_THREADS={thread_count} (CPU核心数: {cpu_count})"
            )
        except Exception as e:
            self.logger.warning(f"环境设置失败: {e}")

    def _signal_handler(self, signum, frame) -> None:
        self.logger.info(f"收到信号 {signum}，准备退出...")
        self.running = False

    @staticmethod
    def _read_mono_audio(source):
        import numpy as np
        import soundfile as sf

        audio, source_rate = sf.read(source, dtype="float32", always_2d=True)
        return audio.mean(axis=1, dtype=np.float32), source_rate

    @staticmethod
    def _resample(audio, source_rate):
        import numpy as np

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

    def _cleanup_memory(self) -> None:
        # Native runtimes own their model and CUDA allocations.
        import gc

        gc.collect()

    def _maybe_cleanup(self, duration: float) -> None:
        """Run periodic memory cleanup after transcription (in background thread)."""
        if self.transcription_count % CLEANUP_EVERY_N == 0 or duration > 120:
            threading.Thread(target=self._cleanup_memory, daemon=True).start()
            self.logger.info(f"已完成 {self.transcription_count} 次转录，后台执行内存清理")

    def _get_gpu_device_info(self, *, include_memory=False) -> dict:
        """Cache optional metadata for the native runtime's first GPU."""
        info = {"device": self.device}
        if not include_memory:
            info["gpu_name"] = None
        if getattr(self, "backend", self.device) != "cuda":
            return info
        cached = getattr(self, "_gpu_device_info_cache", None)
        if cached is not None:
            return dict(cached)
        try:
            fields = "name,memory.total" if include_memory else "name"
            query = subprocess.run(
                ["nvidia-smi", "--id=0", f"--query-gpu={fields}",
                 "--format=csv,noheader,nounits"],
                capture_output=True,
                text=True,
                check=False,
                timeout=3 if include_memory else 1,
                creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
            )
            if query.returncode == 0 and query.stdout.strip():
                first = query.stdout.strip().splitlines()[0]
                if include_memory:
                    name, memory_mb = [part.strip() for part in first.split(",", 1)]
                    memory = round(float(memory_mb) / 1024, 1)
                    info.update(gpu_name=name, gpu_memory_total=memory)
                else:
                    info["gpu_name"] = first.strip()
        except (OSError, subprocess.SubprocessError, IndexError, ValueError):
            pass
        self._gpu_device_info_cache = dict(info)
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

    def transcribe_audio(
        self,
        audio_path: Optional[str],
        options=None,
        hot_words=None,
        audio_base64: Optional[str] = None,
        audio_format: Optional[str] = None,
        sample_rate: Optional[int] = None,
    ) -> dict:
        raise NotImplementedError

    def handle_stream_command(self, command: dict) -> dict:
        """Handle a stream command in subclasses that provide streaming."""
        return {
            "success": False,
            "error": f"不支持流式命令: {command.get('action')}",
        }


    def _with_gpu_suspend_flag(self, payload):
        """Add gpu_suspended only while the model is actually unloaded."""
        if not isinstance(payload, dict) or not getattr(self, "_gpu_suspended", False):
            return payload
        if payload.get("gpu_suspended") is True:
            return payload
        updated = dict(payload)
        updated["gpu_suspended"] = True
        return updated

    def _gpu_idle_stream_active(self) -> bool:
        return False

    def _suspend_gpu_runtime(self) -> bool:
        """Release the resident GPU model. Subclasses own the close API."""
        return False

    def _adopt_gpu_idle_from_env(self) -> None:
        from gpu_idle import GPU_IDLE_ENV, parse_gpu_idle_seconds

        if GPU_IDLE_ENV not in os.environ:
            return
        seconds = parse_gpu_idle_seconds(os.environ.get(GPU_IDLE_ENV))
        if seconds <= 0:
            return
        self._gpu_idle_seconds = seconds
        self._last_activity_at = time.monotonic()
        self._ensure_gpu_idle_thread()
        self.logger.info("GPU 空闲卸载已启用，超时 %s 秒", seconds)

    def _handle_set_gpu_idle(self, command) -> dict:
        from gpu_idle import parse_gpu_idle_seconds

        if not isinstance(command, dict) or "seconds" not in command:
            return {"success": False, "error": "缺少 seconds"}
        seconds = parse_gpu_idle_seconds(command.get("seconds"))
        with self._gpu_idle_lock:
            if seconds > 0 and self._gpu_idle_seconds <= 0:
                self._last_activity_at = time.monotonic()
            self._gpu_idle_seconds = seconds
            if seconds <= 0:
                self._last_activity_at = None
        if seconds > 0:
            self._ensure_gpu_idle_thread()
        else:
            self._stop_gpu_idle_thread()
        self.logger.info("GPU 空闲卸载设置已更新: %s 秒（0 表示关闭）", seconds)
        return {
            "success": True,
            "gpu_idle_seconds": seconds,
            "enabled": seconds > 0,
        }

    def _ensure_gpu_idle_thread(self) -> None:
        thread = self._gpu_idle_thread
        if thread is not None and thread.is_alive():
            return
        stop = threading.Event()
        self._gpu_idle_stop = stop
        thread = threading.Thread(
            target=self._gpu_idle_loop,
            args=(stop,),
            name="gpu-idle-unload",
            daemon=True,
        )
        self._gpu_idle_thread = thread
        thread.start()

    def _stop_gpu_idle_thread(self) -> None:
        stop = self._gpu_idle_stop
        thread = self._gpu_idle_thread
        if stop is not None:
            stop.set()
        if (
            thread is not None
            and thread.is_alive()
            and thread is not threading.current_thread()
        ):
            thread.join(timeout=2.0)
        if thread is None or not thread.is_alive():
            self._gpu_idle_thread = None
            self._gpu_idle_stop = None

    def _gpu_idle_loop(self, stop: threading.Event) -> None:
        while not stop.wait(0.5):
            try:
                self._gpu_idle_tick()
            except Exception:
                self.logger.warning("GPU 空闲卸载检查失败", exc_info=True)

    def _gpu_idle_tick(self, now: float | None = None) -> bool:
        from gpu_idle import gpu_idle_should_unload

        if not self._gpu_idle_lock.acquire(blocking=False):
            return False
        try:
            idle_seconds = self._gpu_idle_seconds
            if idle_seconds <= 0:
                return False
            if now is None:
                now = time.monotonic()
            if not gpu_idle_should_unload(
                now=now,
                last_activity=self._last_activity_at,
                idle_seconds=idle_seconds,
                stream_active=self._gpu_idle_stream_active(),
            ):
                return False
            return bool(self._suspend_gpu_runtime())
        finally:
            self._gpu_idle_lock.release()

    def _execute_action(self, command, action):
        if action == "transcribe":
            return self.transcribe_audio(
                command.get("audio_path"),
                command.get("options", {}),
                hot_words=command.get("hot_words"),
                audio_base64=command.get("audio_base64"),
                audio_format=command.get("audio_format"),
                sample_rate=command.get("sample_rate"),
            )
        if action == "status":
            return self._with_gpu_suspend_flag(self.check_status())
        if action == "stats":
            return {"success": True, "stats": self.get_performance_stats()}
        if action == "cleanup":
            self._cleanup_memory()
            return {"success": True, "message": "内存清理完成"}
        if action in (
            "stream_start",
            "stream_feed",
            "stream_finish",
            "stream_cancel",
        ):
            return self.handle_stream_command(command)
        return {"success": False, "error": f"未知命令: {action}"}

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

        self._adopt_gpu_idle_from_env()

        while self.running:
            request_id = None
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
                    # 无 request_id 可回传——JSON 解析就失败了。Rust 端按旧协议兼容。
                    result = {"success": False, "error": "无效的JSON命令"}
                    print(json.dumps(result, ensure_ascii=False))
                    sys.stdout.flush()
                    continue

                # Rust 端给每条命令打的 request_id；所有响应必须原样回传，
                # 否则乱序/延迟响应防护退化为旧协议兼容路径。
                if isinstance(command, dict):
                    rid = command.get("request_id")
                    if isinstance(rid, int):
                        request_id = rid

                action = command.get("action")
                if action == "exit":
                    if self._gpu_idle_thread is not None:
                        self._stop_gpu_idle_thread()
                    result = {"success": True, "message": "服务器退出"}
                    if request_id is not None and isinstance(result, dict):
                        result["request_id"] = request_id
                    print(json.dumps(result, ensure_ascii=False))
                    sys.stdout.flush()
                    break
                elif action == "set_gpu_idle":
                    result = self._handle_set_gpu_idle(command)
                elif self._gpu_idle_seconds > 0 and action in (
                    "transcribe",
                    "stream_start",
                    "stream_feed",
                    "stream_finish",
                    "stream_cancel",
                    "status",
                    "stats",
                    "cleanup",
                ):
                    from gpu_idle import GPU_IDLE_ACTIVITY_ACTIONS

                    with self._gpu_idle_lock:
                        result = self._execute_action(command, action)
                        if action in GPU_IDLE_ACTIVITY_ACTIONS:
                            self._last_activity_at = time.monotonic()
                else:
                    result = self._execute_action(command, action)

                if request_id is not None and isinstance(result, dict):
                    result["request_id"] = request_id
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
                if request_id is not None:
                    error_result["request_id"] = request_id
                print(json.dumps(error_result, ensure_ascii=False))
                sys.stdout.flush()

        self.logger.info(f"{self.engine} 服务器退出")
