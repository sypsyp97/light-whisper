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
